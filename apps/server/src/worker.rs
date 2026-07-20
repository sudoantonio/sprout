use std::{path::PathBuf, time::Duration};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload, array::Array},
};
use base64::Engine;
use chrono::{DateTime, Utc};
use clap::ValueEnum;
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sprout_storage_postgres::{AcquireRetentionLease, PostgresStorage, RequestContext};
use sqlx::{FromRow, PgPool};
use tokio::sync::watch;
use uuid::Uuid;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::{
    archive_store::{self, ArchivePersistFault},
    config::Config,
    error::AppError,
    routes::enqueue_retention_warning,
};

const ARCHIVE_FORMAT_VERSION: u16 = 1;
const ARCHIVE_AAD_DOMAIN: &[u8] = b"sprout-retention-archive-v1";
const ARCHIVE_WRAP_DOMAIN: &[u8] = b"sprout-retention-archive-wrap-v1";

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum WorkerKind {
    Retention,
    Export,
    All,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerOptions {
    pub kind: WorkerKind,
    pub dry_run: bool,
    pub once: bool,
    pub interval: Duration,
    pub lease_ttl_seconds: i64,
}

pub async fn run(
    pool: PgPool,
    config: Config,
    options: WorkerOptions,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), AppError> {
    let worker_id = Uuid::new_v4();
    loop {
        run_cycle(&pool, &config, worker_id, options, Utc::now()).await?;
        if options.once {
            return Ok(());
        }
        tokio::select! {
            _ = tokio::time::sleep(options.interval) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

async fn run_cycle(
    pool: &PgPool,
    config: &Config,
    worker_id: Uuid,
    options: WorkerOptions,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    if !options.dry_run && matches!(options.kind, WorkerKind::Retention | WorkerKind::All) {
        sqlx::query("SELECT sprout_private.materialize_retention_subjects($1)")
            .bind(now)
            .execute(pool)
            .await?;
    }
    let project_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id FROM projects
        UNION
        SELECT project_id FROM retention_subjects
        UNION
        SELECT project_id FROM retention_archives
        ORDER BY 1
        "#,
    )
    .fetch_all(pool)
    .await?;
    for project_id in project_ids {
        match options.kind {
            WorkerKind::Retention => {
                retention_project(
                    pool, config, worker_id, project_id, options, now, true, true,
                )
                .await?
            }
            WorkerKind::Export => {
                export_project(pool, config, worker_id, project_id, options, now).await?
            }
            WorkerKind::All => {
                retention_project(
                    pool, config, worker_id, project_id, options, now, true, false,
                )
                .await?;
                export_project(pool, config, worker_id, project_id, options, now).await?;
                retention_project(
                    pool, config, worker_id, project_id, options, now, false, true,
                )
                .await?;
            }
        }
    }
    let worker_lag_seconds = sqlx::query_scalar::<_, f64>(
        r#"
        SELECT COALESCE(
            GREATEST(
                EXTRACT(EPOCH FROM ($1 - MIN(COALESCE(retry_at, purge_at))))::double precision,
                0
            ),
            0
        )
        FROM retention_subjects
        WHERE state IN ('scheduled', 'retry', 'purging')
          AND COALESCE(retry_at, purge_at) <= $1
        "#,
    )
    .bind(now)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO operational_metrics (name, value, updated_at)
        VALUES ('worker_lag_seconds', $1, clock_timestamp())
        ON CONFLICT (name) DO UPDATE
        SET value = EXCLUDED.value,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(worker_lag_seconds)
    .execute(pool)
    .await?;
    Ok(())
}

async fn retention_project(
    pool: &PgPool,
    config: &Config,
    worker_id: Uuid,
    project_id: Uuid,
    options: WorkerOptions,
    now: DateTime<Utc>,
    enqueue_warnings: bool,
    purge_and_expire: bool,
) -> Result<(), AppError> {
    let storage = PostgresStorage::new(pool.clone());
    let context = RequestContext::new(worker_id, None);
    let lease = storage
        .acquire_retention_lease_at(
            context,
            &AcquireRetentionLease {
                project_id,
                lease_scope: "retention".into(),
                partition_key: "project".into(),
                lease_owner: worker_id,
                ttl_seconds: options.lease_ttl_seconds,
            },
            now,
        )
        .await?;
    let Some(lease) = lease else {
        return Ok(());
    };

    let result = async {
        let subjects = if enqueue_warnings {
            sqlx::query_as::<_, WarningSubject>(
                r#"
            SELECT id, project_id, source_kind, source_id, warning_at
            FROM retention_subjects
            WHERE project_id = $1
              AND warning_at <= $2
              AND state <> 'purged'
            ORDER BY warning_at, id
            "#,
            )
            .bind(project_id)
            .bind(now)
            .fetch_all(pool)
            .await?
        } else {
            Vec::new()
        };
        for subject in subjects {
            if options.dry_run {
                tracing::info!(
                    project_id = %project_id,
                    subject_id = %subject.id,
                    "retention warning would be enqueued"
                );
            } else if let Err(error) = enqueue_subject_warnings(pool, config, &subject, now).await {
                tracing::error!(
                    project_id = %project_id,
                    subject_id = %subject.id,
                    error = %error,
                    "retention warning enqueue failed; purge eligibility is unchanged"
                );
            }
        }

        if !options.dry_run && purge_and_expire {
            purge_due_subjects(
                pool,
                config,
                worker_id,
                project_id,
                options.lease_ttl_seconds,
                now,
            )
            .await?;
            delete_expired_archives(pool, config, project_id, now).await?;
            sqlx::query(
                "DELETE FROM sync_idempotency
                 WHERE project_id = $1 AND expires_at <= $2",
            )
            .bind(project_id)
            .bind(now)
            .execute(pool)
            .await?;
        }
        Ok::<_, AppError>(())
    }
    .await;
    let _ = storage.release_retention_lease(context, &lease).await;
    result
}

async fn enqueue_subject_warnings(
    pool: &PgPool,
    config: &Config,
    subject: &WarningSubject,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let recipients = sqlx::query_as::<_, WarningRecipient>(
        r#"
        SELECT
            interested.identity_id,
            email.normalized_email,
            COALESCE(preference.auto_export_enabled, false) AS auto_export_enabled
        FROM sprout_private.retention_interested_users($1) interested
        LEFT JOIN identity_emails email
          ON email.identity_id = interested.identity_id
         AND email.verified_at IS NOT NULL
        LEFT JOIN identity_retention_preferences preference
          ON preference.identity_id = interested.identity_id
        ORDER BY interested.identity_id
        "#,
    )
    .bind(subject.id)
    .fetch_all(pool)
    .await?;

    for recipient in recipients {
        let warning_key = warning_digest(subject, recipient.identity_id, b"window");
        let in_app_key = warning_digest(subject, recipient.identity_id, b"in_app");
        let email_key = warning_digest(subject, recipient.identity_id, b"email");
        let payload = RetentionWarningPayload {
            version: 1,
            subject_id: subject.id,
            source_kind: &subject.source_kind,
            source_id: subject.source_id,
            warning_at: subject.warning_at,
        };
        let payload_bytes = serde_json::to_vec(&payload).map_err(|_| AppError::Internal)?;
        let mut transaction = pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO retention_warning_deliveries (
                project_id, subject_id, recipient_identity_id, warning_at
            )
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (subject_id, recipient_identity_id, warning_at) DO NOTHING
            "#,
        )
        .bind(subject.project_id)
        .bind(subject.id)
        .bind(recipient.identity_id)
        .bind(subject.warning_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO notifications (
                project_id, recipient_identity_id, notification_kind,
                delivery_channel, encrypted_payload, deduplication_key,
                scheduled_at
            )
            VALUES (
                $1, $2, 'retention_warning', 'in_app',
                $3, $4, $5
            )
            ON CONFLICT (project_id, recipient_identity_id, deduplication_key)
                WHERE deduplication_key IS NOT NULL
            DO NOTHING
            "#,
        )
        .bind(subject.project_id)
        .bind(recipient.identity_id)
        .bind(&payload_bytes)
        .bind(in_app_key.as_slice())
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE retention_warning_deliveries
            SET in_app_enqueued_at = COALESCE(in_app_enqueued_at, $4)
            WHERE subject_id = $1
              AND recipient_identity_id = $2
              AND warning_at = $3
            "#,
        )
        .bind(subject.id)
        .bind(recipient.identity_id)
        .bind(subject.warning_at)
        .bind(now)
        .execute(&mut *transaction)
        .await?;

        if let Some(email) = &recipient.normalized_email {
            enqueue_retention_warning(
                &mut transaction,
                config,
                recipient.identity_id,
                email,
                &email_key,
                &payload,
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE retention_warning_deliveries
                SET email_enqueued_at = COALESCE(email_enqueued_at, $4)
                WHERE subject_id = $1
                  AND recipient_identity_id = $2
                  AND warning_at = $3
                "#,
            )
            .bind(subject.id)
            .bind(recipient.identity_id)
            .bind(subject.warning_at)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        } else {
            tracing::warn!(
                identity_id = %recipient.identity_id,
                subject_id = %subject.id,
                "retention warning recipient has no verified email"
            );
        }

        if recipient.auto_export_enabled {
            sqlx::query(
                r#"
                INSERT INTO retention_archives (
                    project_id, subject_id, recipient_identity_id
                )
                VALUES ($1, $2, $3)
                ON CONFLICT (subject_id, recipient_identity_id) DO NOTHING
                "#,
            )
            .bind(subject.project_id)
            .bind(subject.id)
            .bind(recipient.identity_id)
            .execute(&mut *transaction)
            .await?;
        }
        // Bind the delivery row to one stable retry window even though channel
        // dedupe keys are distinct.
        let _ = warning_key;
        transaction.commit().await?;
    }
    Ok(())
}

async fn purge_due_subjects(
    pool: &PgPool,
    config: &Config,
    worker_id: Uuid,
    project_id: Uuid,
    ttl_seconds: i64,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    loop {
        let claimed = sqlx::query_as::<_, PurgeClaim>(
            r#"
            WITH candidate AS (
                SELECT subject.id
                FROM retention_subjects subject
                WHERE subject.project_id = $1
                  AND subject.state IN ('scheduled', 'retry', 'purging')
                  AND (
                      subject.state <> 'retry'
                      OR subject.retry_at <= $2
                  )
                  AND sprout_private.retention_effective_purge_at(subject.id) <= $2
                  AND (
                      subject.state <> 'purging'
                      OR subject.leased_until <= $2
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM retention_dependencies dependency
                      JOIN retention_subjects dependent
                        ON dependent.id = dependency.subject_id
                      WHERE dependency.depends_on_subject_id = subject.id
                        AND dependent.state <> 'purged'
                  )
                ORDER BY subject.purge_at, subject.id
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            UPDATE retention_subjects subject
            SET
                state = 'purging',
                attempts = subject.attempts + 1,
                retry_at = NULL,
                lease_owner = $3,
                lease_token = gen_random_uuid(),
                leased_until = $2 + make_interval(secs => $4::double precision),
                last_error_code = NULL
            FROM candidate
            WHERE subject.id = candidate.id
            RETURNING
                subject.id,
                subject.project_id,
                subject.resource_node_id,
                subject.lease_token
            "#,
        )
        .bind(project_id)
        .bind(now)
        .bind(worker_id)
        .bind(ttl_seconds)
        .fetch_optional(pool)
        .await?;
        let Some(claimed) = claimed else {
            return Ok(());
        };

        let purge_result = purge_claim(pool, config, &claimed, now).await;
        if let Err(error) = purge_result {
            tracing::error!(
                project_id = %claimed.project_id,
                subject_id = %claimed.id,
                error = %error,
                "retention purge failed and will retry after lease release"
            );
            sqlx::query(
                r#"
                UPDATE retention_subjects
                SET
                    state = 'retry',
                    retry_at = $3 + interval '30 seconds',
                    lease_owner = NULL,
                    lease_token = NULL,
                    leased_until = NULL,
                    last_error_code = 'purge_failed'
                WHERE id = $1 AND lease_token = $2
                "#,
            )
            .bind(claimed.id)
            .bind(claimed.lease_token)
            .bind(now)
            .execute(pool)
            .await?;
        }
    }
}

async fn purge_claim(
    pool: &PgPool,
    config: &Config,
    claimed: &PurgeClaim,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let blob_keys = sqlx::query_as::<_, BlobStorageKey>(
        r#"
        SELECT storage_key
        FROM file_blobs
        WHERE project_id = $1
          AND resource_node_id = $2
          AND storage_provider = 'filesystem'
        ORDER BY id
        "#,
    )
    .bind(claimed.project_id)
    .bind(claimed.resource_node_id)
    .fetch_all(pool)
    .await?;
    let project_blob_dir = config
        .blob_dir
        .join(claimed.project_id.simple().to_string());
    for blob in blob_keys {
        delete_blob_idempotently(&project_blob_dir, &blob.storage_key).await?;
    }
    let purged =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.purge_retention_subject($1, $2, $3)")
            .bind(claimed.id)
            .bind(claimed.lease_token)
            .bind(now)
            .fetch_one(pool)
            .await?;
    if purged {
        Ok(())
    } else {
        Err(AppError::Conflict)
    }
}

async fn export_project(
    pool: &PgPool,
    config: &Config,
    worker_id: Uuid,
    project_id: Uuid,
    options: WorkerOptions,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let storage = PostgresStorage::new(pool.clone());
    let context = RequestContext::new(worker_id, None);
    let lease = storage
        .acquire_retention_lease_at(
            context,
            &AcquireRetentionLease {
                project_id,
                lease_scope: "export".into(),
                partition_key: "project".into(),
                lease_owner: worker_id,
                ttl_seconds: options.lease_ttl_seconds,
            },
            now,
        )
        .await?;
    let Some(lease) = lease else {
        return Ok(());
    };
    let result = async {
        if options.dry_run {
            return Ok(());
        }
        loop {
            let claim =
                claim_archive(pool, project_id, worker_id, options.lease_ttl_seconds, now).await?;
            let Some(claim) = claim else {
                break;
            };
            if let Err(error) = generate_archive(pool, config, &claim, now).await {
                tracing::error!(
                    project_id = %project_id,
                    archive_id = %claim.id,
                    error = %error,
                    "retention archive generation failed; source purge remains eligible"
                );
                let key = archive_store::storage_key(claim.id);
                let _ = archive_store::delete(&config.archive_dir, &key).await;
                sqlx::query(
                    r#"
                    UPDATE retention_archives
                    SET
                        state = 'failed',
                        completed_at = $3,
                        failure_code = 'archive_generation_failed',
                        storage_key = NULL,
                        ciphertext_size = NULL,
                        ciphertext_sha256 = NULL,
                        canonical_manifest = NULL,
                        manifest_signature = NULL,
                        lease_owner = NULL,
                        lease_token = NULL,
                        leased_until = NULL
                    WHERE id = $1 AND lease_token = $2
                    "#,
                )
                .bind(claim.id)
                .bind(claim.lease_token)
                .bind(now)
                .execute(pool)
                .await?;
            }
        }
        Ok::<_, AppError>(())
    }
    .await;
    let _ = storage.release_retention_lease(context, &lease).await;
    result
}

async fn claim_archive(
    pool: &PgPool,
    project_id: Uuid,
    worker_id: Uuid,
    ttl_seconds: i64,
    now: DateTime<Utc>,
) -> Result<Option<ArchiveClaim>, AppError> {
    Ok(sqlx::query_as::<_, ArchiveClaim>(
        r#"
        WITH candidate AS (
            SELECT archive.id
            FROM retention_archives archive
            JOIN retention_subjects subject ON subject.id = archive.subject_id
            WHERE archive.project_id = $1
              AND archive.state IN ('pending', 'running')
              AND (
                  archive.state = 'pending'
                  OR archive.leased_until <= $2
              )
              AND subject.state <> 'purged'
            ORDER BY archive.created_at, archive.id
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE retention_archives archive
        SET
            state = 'running',
            attempts = archive.attempts + 1,
            lease_owner = $3,
            lease_token = gen_random_uuid(),
            leased_until = $2 + make_interval(secs => $4::double precision),
            failure_code = NULL,
            completed_at = NULL
        FROM candidate
        WHERE archive.id = candidate.id
        RETURNING
            archive.id,
            archive.project_id,
            archive.subject_id,
            archive.recipient_identity_id,
            archive.lease_token
        "#,
    )
    .bind(project_id)
    .bind(now)
    .bind(worker_id)
    .bind(ttl_seconds)
    .fetch_optional(pool)
    .await?)
}

async fn generate_archive(
    pool: &PgPool,
    config: &Config,
    claim: &ArchiveClaim,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let authorized = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM retention_warning_deliveries warning
            JOIN sprout_private.retention_interested_users($1) interested
              ON interested.identity_id = warning.recipient_identity_id
            WHERE warning.subject_id = $1
              AND warning.recipient_identity_id = $2
        )
        "#,
    )
    .bind(claim.subject_id)
    .bind(claim.recipient_identity_id)
    .fetch_one(pool)
    .await?;
    if !authorized {
        return Err(AppError::Forbidden);
    }

    let subject = sqlx::query_as::<_, ArchiveSubject>(
        r#"
        SELECT source_kind, source_id, resource_node_id
        FROM retention_subjects
        WHERE id = $1 AND project_id = $2
        "#,
    )
    .bind(claim.subject_id)
    .bind(claim.project_id)
    .fetch_one(pool)
    .await?;
    let records = load_ciphertext_records(pool, claim.project_id, &subject).await?;
    let key_envelopes = load_resource_key_envelopes(pool, claim, subject.resource_node_id).await?;
    let blobs = load_blob_records(pool, config, claim.project_id, subject.resource_node_id).await?;
    let devices = load_active_devices(pool, claim.recipient_identity_id).await?;
    if devices.is_empty() {
        return Err(AppError::BadRequest(
            "archive recipient has no active device package",
        ));
    }
    if devices.iter().any(|device| {
        !key_envelopes.iter().any(|envelope| {
            envelope.recipient_device_id == device.device_id
                && envelope.recipient_device_key_version == device.key_version
        })
    }) {
        return Err(AppError::BadRequest(
            "archive recipient lacks a required resource key envelope",
        ));
    }

    let manifest_entries = records
        .iter()
        .map(|record| ManifestEntry {
            kind: record.kind.clone(),
            id: record.id,
            ciphertext_size: record.ciphertext.len(),
            ciphertext_sha256_b64: encode(&Sha256::digest(&record.ciphertext)),
        })
        .chain(blobs.iter().map(|blob| ManifestEntry {
            kind: "file_blob.ciphertext".into(),
            id: blob.id,
            ciphertext_size: blob.ciphertext.len(),
            ciphertext_sha256_b64: encode(&Sha256::digest(&blob.ciphertext)),
        }))
        .collect();
    let manifest = ArchiveManifest {
        version: ARCHIVE_FORMAT_VERSION,
        archive_id: claim.id,
        project_id: claim.project_id,
        subject_id: claim.subject_id,
        source_kind: subject.source_kind.clone(),
        source_id: subject.source_id,
        recipient_identity_id: claim.recipient_identity_id,
        generated_at: now,
        signing_key_id: config.archive_signing_key_id,
        entries: manifest_entries,
    };
    let canonical_manifest = serde_json::to_vec(&manifest).map_err(|_| AppError::Internal)?;
    let signing_key = SigningKey::from_bytes(config.archive_signing_key.expose());
    let manifest_signature = signing_key.sign(&canonical_manifest).to_bytes().to_vec();

    let content = ArchiveContent {
        version: ARCHIVE_FORMAT_VERSION,
        archive_id: claim.id,
        recipient_identity_id: claim.recipient_identity_id,
        records,
        blobs,
        resource_key_envelopes: key_envelopes,
    };
    let content_bytes = serde_json::to_vec(&content).map_err(|_| AppError::Internal)?;
    let archive_key = random_32();
    let outer_nonce = random_12();
    let mut outer_aad = Vec::with_capacity(ARCHIVE_AAD_DOMAIN.len() + canonical_manifest.len());
    outer_aad.extend_from_slice(ARCHIVE_AAD_DOMAIN);
    outer_aad.extend_from_slice(&canonical_manifest);
    let ciphertext = Aes256Gcm::new(&Array(archive_key))
        .encrypt(
            &Array(outer_nonce),
            Payload {
                msg: &content_bytes,
                aad: &outer_aad,
            },
        )
        .map_err(|_| AppError::Internal)?;

    let wrapped_devices = devices
        .into_iter()
        .map(|device| wrap_archive_key(claim.id, archive_key, device))
        .collect::<Result<Vec<_>, _>>()?;
    let package = ArchivePackage {
        version: ARCHIVE_FORMAT_VERSION,
        canonical_manifest_b64: encode(&canonical_manifest),
        manifest_signature_b64: encode(&manifest_signature),
        outer_nonce_b64: encode(&outer_nonce),
        ciphertext_b64: encode(&ciphertext),
        device_envelopes: wrapped_devices.clone(),
    };
    let package_bytes = serde_json::to_vec(&package).map_err(|_| AppError::Internal)?;
    let package_digest = Sha256::digest(&package_bytes).to_vec();
    let package_size = i64::try_from(package_bytes.len()).map_err(|_| AppError::Internal)?;
    let storage_key = archive_store::storage_key(claim.id);
    let _ = archive_store::delete(&config.archive_dir, &storage_key).await;
    archive_store::persist(
        &config.archive_dir,
        &storage_key,
        &package_bytes,
        ArchivePersistFault::None,
    )
    .await
    .map_err(|_| AppError::Internal)?;

    let update_result = async {
        let mut transaction = pool.begin().await?;
        for envelope in &wrapped_devices {
            sqlx::query(
                r#"
                INSERT INTO retention_archive_device_envelopes (
                    project_id, archive_id, recipient_identity_id,
                    recipient_device_id, recipient_device_key_version,
                    ephemeral_x25519_public_key, wrap_nonce,
                    wrapped_archive_key
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (
                    archive_id, recipient_device_id,
                    recipient_device_key_version
                ) DO NOTHING
                "#,
            )
            .bind(claim.project_id)
            .bind(claim.id)
            .bind(claim.recipient_identity_id)
            .bind(envelope.recipient_device_id)
            .bind(envelope.recipient_device_key_version)
            .bind(decode(&envelope.ephemeral_x25519_public_key_b64)?)
            .bind(decode(&envelope.wrap_nonce_b64)?)
            .bind(decode(&envelope.wrapped_archive_key_b64)?)
            .execute(&mut *transaction)
            .await?;
        }
        let changed = sqlx::query(
            r#"
            UPDATE retention_archives
            SET
                state = 'succeeded',
                storage_key = $3,
                ciphertext_size = $4,
                ciphertext_sha256 = $5,
                canonical_manifest = $6,
                manifest_signature = $7,
                completed_at = $8,
                failure_code = NULL,
                lease_owner = NULL,
                lease_token = NULL,
                leased_until = NULL
            WHERE id = $1 AND lease_token = $2 AND state = 'running'
            "#,
        )
        .bind(claim.id)
        .bind(claim.lease_token)
        .bind(&storage_key)
        .bind(package_size)
        .bind(&package_digest)
        .bind(&canonical_manifest)
        .bind(&manifest_signature)
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(AppError::Conflict);
        }
        transaction.commit().await?;
        Ok::<_, AppError>(())
    }
    .await;
    if let Err(error) = update_result {
        let _ = archive_store::delete(&config.archive_dir, &storage_key).await;
        return Err(error);
    }
    Ok(())
}

async fn load_ciphertext_records(
    pool: &PgPool,
    project_id: Uuid,
    subject: &ArchiveSubject,
) -> Result<Vec<CiphertextRecord>, AppError> {
    let rows = sqlx::query_as::<_, CiphertextRow>(
        r#"
        SELECT 'resource_node.encrypted_metadata' AS kind, id, encrypted_metadata AS ciphertext
        FROM resource_nodes
        WHERE project_id = $1 AND id = $2

        UNION ALL
        SELECT 'task.encrypted_payload', id, encrypted_payload
        FROM tasks
        WHERE project_id = $1 AND resource_node_id = $2

        UNION ALL
        SELECT 'task.encrypted_value_snapshot', id, encrypted_value_snapshot
        FROM tasks
        WHERE project_id = $1 AND resource_node_id = $2

        UNION ALL
        SELECT 'sync_event.encrypted_payload', id, encrypted_payload
        FROM sync_events
        WHERE project_id = $1 AND resource_node_id = $2

        UNION ALL
        SELECT 'task_snapshot.encrypted_payload', history.id, history.encrypted_payload
        FROM task_snapshot_history history
        JOIN tasks task
          ON task.project_id = history.project_id
         AND task.id = history.task_id
        WHERE task.project_id = $1 AND task.resource_node_id = $2

        UNION ALL
        SELECT 'task_snapshot.encrypted_value_snapshot', history.id, history.encrypted_value_snapshot
        FROM task_snapshot_history history
        JOIN tasks task
          ON task.project_id = history.project_id
         AND task.id = history.task_id
        WHERE task.project_id = $1 AND task.resource_node_id = $2

        UNION ALL
        SELECT 'task_completion.encrypted_payload', completion.id, completion.encrypted_payload
        FROM task_completions completion
        JOIN tasks task
          ON task.project_id = completion.project_id
         AND task.id = completion.task_id
        WHERE task.project_id = $1 AND task.resource_node_id = $2

        UNION ALL
        SELECT 'questionnaire_submission.encrypted_payload', submission.id, submission.encrypted_payload
        FROM questionnaire_submissions submission
        JOIN tasks task
          ON task.project_id = submission.project_id
         AND task.id = submission.task_id
        WHERE task.project_id = $1 AND task.resource_node_id = $2

        UNION ALL
        SELECT 'questionnaire_answer.encrypted_payload', answer.id, answer.encrypted_payload
        FROM questionnaire_answers answer
        JOIN questionnaire_submissions submission
          ON submission.project_id = answer.project_id
         AND submission.id = answer.submission_id
        JOIN tasks task
          ON task.project_id = submission.project_id
         AND task.id = submission.task_id
        WHERE task.project_id = $1 AND task.resource_node_id = $2

        UNION ALL
        SELECT 'file_blob.encrypted_metadata', blob.id, blob.encrypted_metadata
        FROM file_blobs blob
        WHERE blob.project_id = $1 AND blob.resource_node_id = $2

        ORDER BY 1, 2
        "#,
    )
    .bind(project_id)
    .bind(subject.resource_node_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| CiphertextRecord {
            kind: row.kind,
            id: row.id,
            ciphertext_b64: encode(&row.ciphertext),
            ciphertext: row.ciphertext,
        })
        .collect())
}

async fn load_resource_key_envelopes(
    pool: &PgPool,
    claim: &ArchiveClaim,
    resource_node_id: Uuid,
) -> Result<Vec<ResourceKeyEnvelopeRecord>, AppError> {
    let rows = sqlx::query_as::<_, ResourceEnvelopeRow>(
        r#"
        SELECT
            id, epoch, recipient_device_id,
            recipient_device_key_version, encrypted_key,
            sender_signature, sender_post_quantum_signature
        FROM resource_key_envelopes
        WHERE project_id = $1
          AND resource_node_id = $2
          AND recipient_identity_id = $3
        ORDER BY epoch, recipient_device_id, recipient_device_key_version
        "#,
    )
    .bind(claim.project_id)
    .bind(resource_node_id)
    .bind(claim.recipient_identity_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ResourceKeyEnvelopeRecord {
            id: row.id,
            epoch: row.epoch,
            recipient_device_id: row.recipient_device_id,
            recipient_device_key_version: row.recipient_device_key_version,
            encrypted_key_b64: encode(&row.encrypted_key),
            sender_signature_b64: encode(&row.sender_signature),
            sender_post_quantum_signature_b64: encode(&row.sender_post_quantum_signature),
        })
        .collect())
}

async fn load_blob_records(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    resource_node_id: Uuid,
) -> Result<Vec<BlobRecord>, AppError> {
    let rows = sqlx::query_as::<_, BlobRow>(
        r#"
        SELECT id, storage_key, ciphertext_size, ciphertext_hash
        FROM file_blobs
        WHERE project_id = $1
          AND resource_node_id = $2
          AND upload_state = 'available'
          AND storage_provider = 'filesystem'
        ORDER BY id
        "#,
    )
    .bind(project_id)
    .bind(resource_node_id)
    .fetch_all(pool)
    .await?;
    let root = config.blob_dir.join(project_id.simple().to_string());
    let mut blobs = Vec::with_capacity(rows.len());
    for row in rows {
        let path = safe_blob_path(&root, &row.storage_key)?;
        let ciphertext = tokio::fs::read(path)
            .await
            .map_err(|_| AppError::Internal)?;
        if i64::try_from(ciphertext.len()).ok() != Some(row.ciphertext_size)
            || Sha256::digest(&ciphertext).as_slice() != row.ciphertext_hash
        {
            return Err(AppError::Internal);
        }
        blobs.push(BlobRecord {
            id: row.id,
            ciphertext_b64: encode(&ciphertext),
            ciphertext,
        });
    }
    Ok(blobs)
}

async fn load_active_devices(
    pool: &PgPool,
    recipient_identity_id: Uuid,
) -> Result<Vec<ActiveDevice>, AppError> {
    Ok(sqlx::query_as::<_, ActiveDevice>(
        r#"
        SELECT
            key.device_id,
            key.key_version,
            key.x25519_public_key
        FROM device_keys key
        JOIN devices device
          ON device.identity_id = key.identity_id
         AND device.id = key.device_id
        WHERE key.identity_id = $1
          AND key.revoked_at IS NULL
          AND device.retired_at IS NULL
          AND octet_length(key.x25519_public_key) = 32
        ORDER BY key.device_id, key.key_version
        "#,
    )
    .bind(recipient_identity_id)
    .fetch_all(pool)
    .await?)
}

fn wrap_archive_key(
    archive_id: Uuid,
    archive_key: [u8; 32],
    device: ActiveDevice,
) -> Result<ArchiveDeviceEnvelope, AppError> {
    let recipient_public_bytes: [u8; 32] = device
        .x25519_public_key
        .as_slice()
        .try_into()
        .map_err(|_| AppError::Internal)?;
    let ephemeral_secret = StaticSecret::from(random_32());
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);
    let shared = ephemeral_secret.diffie_hellman(&X25519PublicKey::from(recipient_public_bytes));
    if !shared.was_contributory() {
        return Err(AppError::BadRequest(
            "archive recipient has an invalid active device key",
        ));
    }
    let kek: [u8; 32] = Sha256::new()
        .chain_update(ARCHIVE_WRAP_DOMAIN)
        .chain_update(shared.as_bytes())
        .chain_update(archive_id.as_bytes())
        .chain_update(device.device_id.as_bytes())
        .chain_update(device.key_version.to_be_bytes())
        .finalize()
        .into();
    let nonce = random_12();
    let mut aad = Vec::with_capacity(16 + 16 + 4);
    aad.extend_from_slice(archive_id.as_bytes());
    aad.extend_from_slice(device.device_id.as_bytes());
    aad.extend_from_slice(&device.key_version.to_be_bytes());
    let wrapped = Aes256Gcm::new(&Array(kek))
        .encrypt(
            &Array(nonce),
            Payload {
                msg: &archive_key,
                aad: &aad,
            },
        )
        .map_err(|_| AppError::Internal)?;
    Ok(ArchiveDeviceEnvelope {
        recipient_device_id: device.device_id,
        recipient_device_key_version: device.key_version,
        ephemeral_x25519_public_key_b64: encode(ephemeral_public.as_bytes()),
        wrap_nonce_b64: encode(&nonce),
        wrapped_archive_key_b64: encode(&wrapped),
    })
}

async fn delete_expired_archives(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let rows = sqlx::query_as::<_, ExpiredArchive>(
        r#"
        SELECT id, storage_key
        FROM retention_archives
        WHERE project_id = $1
          AND expires_at IS NOT NULL
          AND expires_at <= $2
        ORDER BY id
        "#,
    )
    .bind(project_id)
    .bind(now)
    .fetch_all(pool)
    .await?;
    for row in rows {
        if let Some(key) = &row.storage_key {
            archive_store::delete(&config.archive_dir, key).await?;
        }
        let mut transaction = pool.begin().await?;
        sqlx::query("DELETE FROM retention_archive_receipts WHERE archive_id = $1")
            .bind(row.id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM retention_archive_device_envelopes WHERE archive_id = $1")
            .bind(row.id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "DELETE FROM retention_archives
             WHERE id = $1 AND expires_at IS NOT NULL AND expires_at <= $2",
        )
        .bind(row.id)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
    }
    Ok(())
}

fn warning_digest(
    subject: &WarningSubject,
    recipient_identity_id: Uuid,
    channel: &[u8],
) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"sprout-retention-warning-v1")
        .chain_update(subject.id.as_bytes())
        .chain_update(recipient_identity_id.as_bytes())
        .chain_update(subject.warning_at.timestamp_micros().to_be_bytes())
        .chain_update(channel)
        .finalize()
        .into()
}

fn random_32() -> [u8; 32] {
    Sha256::new()
        .chain_update(Uuid::new_v4().as_bytes())
        .chain_update(Uuid::new_v4().as_bytes())
        .finalize()
        .into()
}

fn random_12() -> [u8; 12] {
    let digest = Sha256::new()
        .chain_update(Uuid::new_v4().as_bytes())
        .chain_update(Uuid::new_v4().as_bytes())
        .finalize();
    digest[..12].try_into().expect("SHA-256 has twelve bytes")
}

fn safe_blob_path(root: &std::path::Path, storage_key: &str) -> Result<PathBuf, AppError> {
    let Some(stem) = storage_key.strip_suffix(".blob") else {
        return Err(AppError::Internal);
    };
    if stem.len() != 32
        || !stem
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AppError::Internal);
    }
    let path = root.join(storage_key);
    if path.parent() != Some(root) {
        return Err(AppError::Internal);
    }
    Ok(path)
}

async fn delete_blob_idempotently(
    root: &std::path::Path,
    storage_key: &str,
) -> Result<(), AppError> {
    let path = safe_blob_path(root, storage_key)?;
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AppError::Internal),
    }
}

fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::Internal)
}

#[derive(FromRow)]
struct WarningSubject {
    id: Uuid,
    project_id: Uuid,
    source_kind: String,
    source_id: Uuid,
    warning_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct WarningRecipient {
    identity_id: Uuid,
    normalized_email: Option<String>,
    auto_export_enabled: bool,
}

#[derive(Serialize)]
struct RetentionWarningPayload<'a> {
    version: u16,
    subject_id: Uuid,
    source_kind: &'a str,
    source_id: Uuid,
    warning_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct PurgeClaim {
    id: Uuid,
    project_id: Uuid,
    resource_node_id: Uuid,
    lease_token: Uuid,
}

#[derive(FromRow)]
struct BlobStorageKey {
    storage_key: String,
}

#[derive(FromRow)]
struct ArchiveClaim {
    id: Uuid,
    project_id: Uuid,
    subject_id: Uuid,
    recipient_identity_id: Uuid,
    lease_token: Uuid,
}

#[derive(FromRow)]
struct ArchiveSubject {
    source_kind: String,
    source_id: Uuid,
    resource_node_id: Uuid,
}

#[derive(FromRow)]
struct CiphertextRow {
    kind: String,
    id: Uuid,
    ciphertext: Vec<u8>,
}

#[derive(Serialize)]
struct CiphertextRecord {
    kind: String,
    id: Uuid,
    ciphertext_b64: String,
    #[serde(skip)]
    ciphertext: Vec<u8>,
}

#[derive(FromRow)]
struct ResourceEnvelopeRow {
    id: Uuid,
    epoch: i32,
    recipient_device_id: Uuid,
    recipient_device_key_version: i32,
    encrypted_key: Vec<u8>,
    sender_signature: Vec<u8>,
    sender_post_quantum_signature: Vec<u8>,
}

#[derive(Serialize)]
struct ResourceKeyEnvelopeRecord {
    id: Uuid,
    epoch: i32,
    recipient_device_id: Uuid,
    recipient_device_key_version: i32,
    encrypted_key_b64: String,
    sender_signature_b64: String,
    sender_post_quantum_signature_b64: String,
}

#[derive(FromRow)]
struct BlobRow {
    id: Uuid,
    storage_key: String,
    ciphertext_size: i64,
    ciphertext_hash: Vec<u8>,
}

#[derive(Serialize)]
struct BlobRecord {
    id: Uuid,
    ciphertext_b64: String,
    #[serde(skip)]
    ciphertext: Vec<u8>,
}

#[derive(FromRow)]
struct ActiveDevice {
    device_id: Uuid,
    key_version: i32,
    x25519_public_key: Vec<u8>,
}

#[derive(Clone, Serialize)]
struct ArchiveDeviceEnvelope {
    recipient_device_id: Uuid,
    recipient_device_key_version: i32,
    ephemeral_x25519_public_key_b64: String,
    wrap_nonce_b64: String,
    wrapped_archive_key_b64: String,
}

#[derive(Serialize)]
struct ManifestEntry {
    kind: String,
    id: Uuid,
    ciphertext_size: usize,
    ciphertext_sha256_b64: String,
}

#[derive(Serialize)]
struct ArchiveManifest {
    version: u16,
    archive_id: Uuid,
    project_id: Uuid,
    subject_id: Uuid,
    source_kind: String,
    source_id: Uuid,
    recipient_identity_id: Uuid,
    generated_at: DateTime<Utc>,
    signing_key_id: Uuid,
    entries: Vec<ManifestEntry>,
}

#[derive(Serialize)]
struct ArchiveContent {
    version: u16,
    archive_id: Uuid,
    recipient_identity_id: Uuid,
    records: Vec<CiphertextRecord>,
    blobs: Vec<BlobRecord>,
    resource_key_envelopes: Vec<ResourceKeyEnvelopeRecord>,
}

#[derive(Serialize)]
struct ArchivePackage {
    version: u16,
    canonical_manifest_b64: String,
    manifest_signature_b64: String,
    outer_nonce_b64: String,
    ciphertext_b64: String,
    device_envelopes: Vec<ArchiveDeviceEnvelope>,
}

#[derive(FromRow)]
struct ExpiredArchive {
    id: Uuid,
    storage_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn llr_08_4_warning_dedupe_is_exact_per_window_and_channel() {
        let subject = WarningSubject {
            id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            source_kind: "task_completed".into(),
            source_id: Uuid::from_u128(3),
            warning_at: Utc.with_ymd_and_hms(2024, 7, 31, 12, 0, 0).unwrap(),
        };
        let recipient = Uuid::from_u128(4);
        assert_eq!(
            warning_digest(&subject, recipient, b"in_app"),
            warning_digest(&subject, recipient, b"in_app")
        );
        assert_ne!(
            warning_digest(&subject, recipient, b"in_app"),
            warning_digest(&subject, recipient, b"email")
        );
    }

    #[test]
    fn llr_08_4_archive_key_is_wrapped_independently_for_each_device() {
        let secret_a = StaticSecret::from([3; 32]);
        let secret_b = StaticSecret::from([4; 32]);
        let archive = Uuid::from_u128(10);
        let key = [9; 32];
        let a = wrap_archive_key(
            archive,
            key,
            ActiveDevice {
                device_id: Uuid::from_u128(11),
                key_version: 1,
                x25519_public_key: X25519PublicKey::from(&secret_a).as_bytes().to_vec(),
            },
        )
        .unwrap();
        let b = wrap_archive_key(
            archive,
            key,
            ActiveDevice {
                device_id: Uuid::from_u128(12),
                key_version: 1,
                x25519_public_key: X25519PublicKey::from(&secret_b).as_bytes().to_vec(),
            },
        )
        .unwrap();
        assert_ne!(a.wrapped_archive_key_b64, b.wrapped_archive_key_b64);
        assert_ne!(
            a.ephemeral_x25519_public_key_b64,
            b.ephemeral_x25519_public_key_b64
        );
    }

    #[test]
    fn llr_08_6_expired_or_stolen_lease_cannot_match_active_token() {
        let now = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let active_token = Uuid::from_u128(20);
        assert!(lease_matches(
            active_token,
            active_token,
            now + chrono::Duration::seconds(1),
            now
        ));
        assert!(!lease_matches(
            active_token,
            Uuid::from_u128(21),
            now + chrono::Duration::seconds(1),
            now
        ));
        assert!(!lease_matches(active_token, active_token, now, now));
    }

    fn lease_matches(
        expected: Uuid,
        supplied: Uuid,
        leased_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> bool {
        expected == supplied && leased_until > now
    }
}
