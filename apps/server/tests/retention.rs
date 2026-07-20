use std::{env, sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_DISPOSITION},
};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sprout_server::{
    AppState, build_router,
    config::Config,
    worker::{self, WorkerKind, WorkerOptions},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::sync::watch;
use tower::ServiceExt;
use uuid::Uuid;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

const T_LLR_08_5: &str = "T-LLR-08.5";
const T_LLR_08_4: &str = "T-LLR-08.4";
const T_LLR_08_7: &str = "T-LLR-08.7";
const T_LLR_08_8: &str = "T-LLR-08.8";

async fn migrated_pool() -> PgPool {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to a migrated disposable PostgreSQL database");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect to retention requirements database")
}

fn session_token(identity_id: Uuid, session_id: Uuid, secret: char) -> String {
    format!(
        "v1.{identity_id}.{session_id}.{}",
        secret.to_string().repeat(64)
    )
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn hlt_08_complete_virtual_clock_retention_lifecycle() {
    let pool = migrated_pool().await;
    let owner_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    let owner_device_id = Uuid::new_v4();
    let other_device_id = Uuid::new_v4();
    let owner_session_id = Uuid::new_v4();
    let other_session_id = Uuid::new_v4();
    let owner_token = session_token(owner_id, owner_session_id, 'b');
    let other_token = session_token(other_id, other_session_id, 'c');
    let project_id = Uuid::new_v4();
    let root_resource_id = Uuid::new_v4();
    let subject_resource_id = Uuid::new_v4();
    let unrelated_resource_id = Uuid::new_v4();
    let subject_id = Uuid::new_v4();
    let owner_public = X25519PublicKey::from(&StaticSecret::from([7_u8; 32]));
    let other_public = X25519PublicKey::from(&StaticSecret::from([9_u8; 32]));

    let mut transaction = pool.begin().await.expect("begin retention fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("retention tests need a migration-owner or BYPASSRLS connection");
    for (identity_id, label) in [(owner_id, "owner"), (other_id, "other")] {
        sqlx::query(
            "INSERT INTO identities (id, identity_handle, encrypted_profile)
             VALUES ($1, $2, decode('01', 'hex'))",
        )
        .bind(identity_id)
        .bind(format!("ret-{label}-{}", identity_id.simple()))
        .execute(&mut *transaction)
        .await
        .expect("insert retention identity");
        sqlx::query(
            "INSERT INTO identity_emails (
                 identity_id, normalized_email, created_at, verified_at
             ) VALUES (
                 $1, $2, clock_timestamp() - interval '1 second',
                 clock_timestamp()
             )",
        )
        .bind(identity_id)
        .bind(format!("ret-{label}-{}@example.test", identity_id.simple()))
        .execute(&mut *transaction)
        .await
        .expect("insert verified retention email");
    }
    for (identity_id, device_id) in [(owner_id, owner_device_id), (other_id, other_device_id)] {
        sqlx::query(
            "INSERT INTO devices (
                 id, identity_id, device_kind, encrypted_label, trust_state
             ) VALUES ($1, $2, 'web', decode('01', 'hex'), 'trusted')",
        )
        .bind(device_id)
        .bind(identity_id)
        .execute(&mut *transaction)
        .await
        .expect("insert retention device");
    }
    for (identity_id, device_id, public_key, marker) in [
        (owner_id, owner_device_id, owner_public.as_bytes(), 31_u8),
        (other_id, other_device_id, other_public.as_bytes(), 32_u8),
    ] {
        sqlx::query(
            "INSERT INTO device_keys (
                 identity_id, device_id, key_version,
                 encryption_public_key, signing_public_key,
                 previous_package_hash, package_hash,
                 x25519_public_key, ed25519_public_key
             ) VALUES ($1, $2, 1, $3, $4, $5, $6, $3, $4)",
        )
        .bind(identity_id)
        .bind(device_id)
        .bind(public_key.as_slice())
        .bind(vec![marker; 32])
        .bind(vec![0_u8; 32])
        .bind(vec![marker; 32])
        .execute(&mut *transaction)
        .await
        .expect("insert retention device key");
    }

    sqlx::query(
        "INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
         VALUES ($1, $2, decode('01', 'hex'))",
    )
    .bind(project_id)
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert retention project");
    for (identity_id, role) in [(owner_id, "owner"), (other_id, "admin")] {
        sqlx::query(
            "INSERT INTO project_memberships (project_id, identity_id, role)
             VALUES ($1, $2, $3)",
        )
        .bind(project_id)
        .bind(identity_id)
        .bind(role)
        .execute(&mut *transaction)
        .await
        .expect("insert retention membership");
    }
    sqlx::query(
        "INSERT INTO resource_nodes (
             id, project_id, parent_id, node_kind,
             encrypted_metadata, created_by_identity_id
         ) VALUES
             ($1, $2, NULL, 'root', decode('aa', 'hex'), $3),
             ($4, $2, $1, 'other', decode('aabb', 'hex'), $3),
             ($5, $2, $1, 'other', decode('deadbeef', 'hex'), $3)",
    )
    .bind(root_resource_id)
    .bind(project_id)
    .bind(owner_id)
    .bind(subject_resource_id)
    .bind(unrelated_resource_id)
    .execute(&mut *transaction)
    .await
    .expect("insert isolated retention resources");
    sqlx::query(
        "INSERT INTO resource_epochs (
             id, project_id, resource_node_id, epoch,
             created_by_identity_id, created_by_device_id,
             created_by_device_key_version, key_commitment, reason
         ) VALUES (
             $1, $2, $3, 1, $4, $5, 1, $6, 'created'
         )",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(subject_resource_id)
    .bind(owner_id)
    .bind(owner_device_id)
    .bind(vec![41_u8; 16])
    .execute(&mut *transaction)
    .await
    .expect("insert retention resource epoch");
    sqlx::query(
        "INSERT INTO resource_key_envelopes (
             project_id, resource_node_id, epoch, envelope_version,
             recipient_identity_id, recipient_device_id,
             recipient_device_key_version, encrypted_key,
             sender_signature, sender_post_quantum_signature,
             created_by_identity_id, created_by_device_id,
             created_by_device_key_version
         ) VALUES (
             $1, $2, 1, 1, $3, $4, 1, $5, $6, $7, $3, $4, 1
         )",
    )
    .bind(project_id)
    .bind(subject_resource_id)
    .bind(owner_id)
    .bind(owner_device_id)
    .bind(vec![42_u8; 48])
    .bind(vec![43_u8; 64])
    .bind(vec![44_u8; 1])
    .execute(&mut *transaction)
    .await
    .expect("insert owner resource envelope");
    let source_at = Utc::now() - ChronoDuration::days(16);
    sqlx::query("UPDATE resource_nodes SET deleted_at = $2 WHERE id = $1")
        .bind(subject_resource_id)
        .bind(source_at)
        .execute(&mut *transaction)
        .await
        .expect("mark retention subject resource deleted");
    sqlx::query(
        "INSERT INTO retention_subjects (
             id, project_id, source_kind, source_id, resource_node_id,
             owner_identity_id, retention_class, source_at, warning_at, purge_at
         ) VALUES (
             $1, $2, 'resource_deleted', $3, $4, $5,
             'deleted_or_obsolete', $6, $7, $8
         )",
    )
    .bind(subject_id)
    .bind(project_id)
    .bind(subject_resource_id)
    .bind(subject_resource_id)
    .bind(owner_id)
    .bind(source_at)
    .bind(Utc::now() - ChronoDuration::minutes(1))
    .bind(Utc::now() + ChronoDuration::days(14))
    .execute(&mut *transaction)
    .await
    .expect("insert retention subject");
    sqlx::query(
        "INSERT INTO identity_retention_preferences (
             identity_id, auto_export_enabled
         ) VALUES ($1, true), ($2, false)",
    )
    .bind(owner_id)
    .bind(other_id)
    .execute(&mut *transaction)
    .await
    .expect("insert per-user export preferences");
    for (session_id, identity_id, device_id, token) in [
        (owner_session_id, owner_id, owner_device_id, &owner_token),
        (other_session_id, other_id, other_device_id, &other_token),
    ] {
        sqlx::query(
            "INSERT INTO sessions (
                 id, identity_id, device_id, token_hash, expires_at
             ) VALUES ($1, $2, $3, $4, clock_timestamp() + interval '1 hour')",
        )
        .bind(session_id)
        .bind(identity_id)
        .bind(device_id)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .execute(&mut *transaction)
        .await
        .expect("insert retention session");
    }
    transaction
        .commit()
        .await
        .expect("commit retention fixture");

    let root = env::temp_dir().join(format!("sprout-retention-oracle-{}", Uuid::new_v4()));
    let mut config = Config::for_test();
    config.archive_dir = root.join("archives");
    config.blob_dir = root.join("blobs");
    let options = |kind| WorkerOptions {
        kind,
        dry_run: false,
        once: true,
        interval: Duration::from_secs(1),
        lease_ttl_seconds: 60,
    };
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    worker::run(
        pool.clone(),
        config.clone(),
        options(WorkerKind::Retention),
        shutdown_rx,
    )
    .await
    .expect("run retention warning cycle");

    let (_shutdown_a, receiver_a) = watch::channel(false);
    let (_shutdown_b, receiver_b) = watch::channel(false);
    let (worker_a, worker_b) = tokio::join!(
        worker::run(
            pool.clone(),
            config.clone(),
            options(WorkerKind::Retention),
            receiver_a,
        ),
        worker::run(
            pool.clone(),
            config.clone(),
            options(WorkerKind::Retention),
            receiver_b,
        ),
    );
    worker_a.expect("first concurrent retention worker");
    worker_b.expect("second concurrent retention worker");
    let warning_deliveries = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM retention_warning_deliveries
         WHERE subject_id = $1",
    )
    .bind(subject_id)
    .fetch_one(&pool)
    .await
    .expect("count warning deliveries");
    let in_app_notices = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM notifications
         WHERE project_id = $1 AND notification_kind = 'retention_warning'",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .expect("count in-app retention warnings");
    let email_notices = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_outbox
         WHERE message_kind = 'retention_warning'
           AND identity_id IN ($1, $2)",
    )
    .bind(owner_id)
    .bind(other_id)
    .fetch_one(&pool)
    .await
    .expect("count email retention warnings");
    assert_eq!(
        warning_deliveries, 2,
        "{T_LLR_08_4}: duplicate delivery rows"
    );
    assert_eq!(in_app_notices, 2, "{T_LLR_08_4}: duplicate in-app warnings");
    assert_eq!(email_notices, 2, "{T_LLR_08_4}: duplicate email warnings");

    let recipients = sqlx::query_scalar::<_, Uuid>(
        "SELECT recipient_identity_id FROM retention_archives
         WHERE subject_id = $1 ORDER BY recipient_identity_id",
    )
    .bind(subject_id)
    .fetch_all(&pool)
    .await
    .expect("load generated archive recipients");
    assert_eq!(
        recipients,
        vec![owner_id],
        "{T_LLR_08_5}: opt-out or unauthorized recipient received an archive"
    );

    sqlx::query(
        "UPDATE retention_subjects
         SET purge_at = clock_timestamp() - interval '1 second'
         WHERE id = $1",
    )
    .bind(subject_id)
    .execute(&pool)
    .await
    .expect("make export and purge due in the same catch-up cycle");
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    worker::run(
        pool.clone(),
        config.clone(),
        options(WorkerKind::All),
        shutdown_rx,
    )
    .await
    .expect("run combined export-before-purge catch-up cycle");
    let (archive_id, storage_key, canonical_manifest) =
        sqlx::query_as::<_, (Uuid, String, Vec<u8>)>(
            "SELECT id, storage_key, canonical_manifest
             FROM retention_archives
             WHERE subject_id = $1 AND recipient_identity_id = $2
               AND state = 'succeeded'",
        )
        .bind(subject_id)
        .bind(owner_id)
        .fetch_one(&pool)
        .await
        .expect("load successful archive");
    let subject_state =
        sqlx::query_scalar::<_, String>("SELECT state FROM retention_subjects WHERE id = $1")
            .bind(subject_id)
            .fetch_one(&pool)
            .await
            .expect("load purged retention subject");
    assert_eq!(
        subject_state, "purged",
        "T-LLR-08.6: combined worker did not purge after export"
    );
    let manifest: Value =
        serde_json::from_slice(&canonical_manifest).expect("parse canonical archive manifest");
    assert_eq!(
        manifest["recipient_identity_id"],
        owner_id.to_string(),
        "{T_LLR_08_5}: manifest is not recipient-bound"
    );
    let manifest_text = String::from_utf8(canonical_manifest.clone()).expect("UTF-8 manifest");
    assert!(
        manifest_text.contains(&subject_resource_id.to_string()),
        "{T_LLR_08_5}: subject resource is absent from archive manifest"
    );
    assert!(
        !manifest_text.contains(&unrelated_resource_id.to_string()),
        "{T_LLR_08_5}: unrelated project resource leaked into archive manifest"
    );

    let app = build_router(Arc::new(
        AppState::new(config.clone(), pool.clone()).expect("retention test app state"),
    ))
    .expect("retention test router");
    let warning_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/retention/warnings")
                .header("authorization", format!("Bearer {owner_token}"))
                .body(Body::empty())
                .expect("warning request"),
        )
        .await
        .expect("owner warning response");
    assert_eq!(warning_response.status(), StatusCode::OK);
    let warning_body: Value = serde_json::from_slice(
        &to_bytes(warning_response.into_body(), usize::MAX)
            .await
            .expect("read warning response"),
    )
    .expect("parse warning response");
    assert_eq!(
        warning_body["warnings"]
            .as_array()
            .expect("warning response array")
            .len(),
        1,
        "{T_LLR_08_4}: owner did not receive exactly one in-app warning"
    );
    assert_eq!(warning_body["warnings"][0]["state"], "delivered");
    let archives_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/retention/archives")
                .header("authorization", format!("Bearer {owner_token}"))
                .body(Body::empty())
                .expect("archive-list request"),
        )
        .await
        .expect("owner archive-list response");
    assert_eq!(archives_response.status(), StatusCode::OK);
    let archives_body: Value = serde_json::from_slice(
        &to_bytes(archives_response.into_body(), usize::MAX)
            .await
            .expect("read archive-list response"),
    )
    .expect("parse archive-list response");
    assert!(
        archives_body["archives"]
            .as_array()
            .expect("archive-list response array")
            .iter()
            .any(|archive| archive["id"] == archive_id.to_string()),
        "HLT-08: successful archive was not available at the next authenticated read"
    );
    let request = |token: &str| {
        Request::builder()
            .uri(format!("/v1/retention/archives/{archive_id}/download"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .expect("archive request")
    };
    let response = app
        .clone()
        .oneshot(request(&owner_token))
        .await
        .expect("owner archive response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[CONTENT_DISPOSITION]
            .to_str()
            .expect("content disposition")
            .starts_with("attachment;"),
        "{T_LLR_08_8}: archive was not forced to a standard download"
    );
    let original_bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read archive response")
        .to_vec();
    let other_response = app
        .clone()
        .oneshot(request(&other_token))
        .await
        .expect("other-user archive response");
    assert_eq!(
        other_response.status(),
        StatusCode::NOT_FOUND,
        "{T_LLR_08_5}: another user downloaded the owner archive"
    );

    let archive_path = config.archive_dir.join(&storage_key);
    let mut corrupt_bytes = original_bytes.clone();
    corrupt_bytes[0] ^= 0xff;
    tokio::fs::write(&archive_path, &corrupt_bytes)
        .await
        .expect("tamper archive package");
    let corrupt_response = app
        .clone()
        .oneshot(request(&owner_token))
        .await
        .expect("corrupt archive response");
    assert_eq!(
        corrupt_response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "{T_LLR_08_8}: checksum corruption was accepted"
    );

    tokio::fs::write(&archive_path, &original_bytes)
        .await
        .expect("restore archive package");
    sqlx::query(
        "UPDATE retention_archives
         SET manifest_signature = set_byte(manifest_signature, 0, 255 - get_byte(manifest_signature, 0))
         WHERE id = $1",
    )
    .bind(archive_id)
    .execute(&pool)
    .await
    .expect("tamper archive manifest signature");
    let signature_response = app
        .oneshot(request(&owner_token))
        .await
        .expect("signature-corrupt archive response");
    assert_eq!(
        signature_response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "{T_LLR_08_8}: manifest signature corruption was accepted"
    );

    let expires_at = Utc::now() - ChronoDuration::seconds(1);
    let source_purged_at = expires_at - ChronoDuration::days(30);
    sqlx::query(
        "UPDATE retention_archives
         SET source_purged_at = $2, expires_at = $3
         WHERE id = $1",
    )
    .bind(archive_id)
    .bind(source_purged_at)
    .bind(expires_at)
    .execute(&pool)
    .await
    .expect("set exact archive expiry boundary");
    let lifetime = sqlx::query_scalar::<_, i64>(
        "SELECT EXTRACT(EPOCH FROM (expires_at - source_purged_at))::bigint
         FROM retention_archives WHERE id = $1",
    )
    .bind(archive_id)
    .fetch_one(&pool)
    .await
    .expect("load archive lifetime");
    assert_eq!(
        lifetime,
        ChronoDuration::days(30).num_seconds(),
        "{T_LLR_08_7}: archive lifetime was not exactly thirty days"
    );
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    worker::run(
        pool.clone(),
        config.clone(),
        options(WorkerKind::Retention),
        shutdown_rx,
    )
    .await
    .expect("run exact archive expiry cycle");
    let archive_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM retention_archives WHERE id = $1)",
    )
    .bind(archive_id)
    .fetch_one(&pool)
    .await
    .expect("check expired archive row");
    assert!(
        !archive_exists,
        "{T_LLR_08_7}: expired archive row survived"
    );
    assert!(
        !tokio::fs::try_exists(&archive_path)
            .await
            .expect("inspect expired archive path"),
        "{T_LLR_08_7}: expired archive file survived"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}
