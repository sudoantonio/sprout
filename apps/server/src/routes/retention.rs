use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{
        HeaderValue, Response, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE,
            X_CONTENT_TYPE_OPTIONS,
        },
    },
};
use base64::Engine;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, SigningKey};
use sha2::{Digest, Sha256};
use sprout_api_contract::{
    ArchiveReceiptDto, ArchiveReceiptResponse, ListRetentionArchivesResponse,
    ListRetentionWarningsResponse, OpaqueDigestDto, RecordArchiveReceiptRequest,
    RetentionArchiveDto, RetentionArchiveStateDto, RetentionPreferenceDto,
    RetentionPreferenceResponse, RetentionWarningDto, UpdateRetentionPreferenceRequest,
};
use sprout_storage_postgres::{RetentionArchiveIntegrity, validate_retention_archive_integrity};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState, archive_store,
    auth::{AuthSession, set_database_context},
    error::AppError,
};

pub async fn get_preference(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
) -> Result<Json<RetentionPreferenceResponse>, AppError> {
    let mut transaction = begin(&state, actor).await?;
    sqlx::query(
        r#"
        INSERT INTO identity_retention_preferences (identity_id)
        VALUES ($1)
        ON CONFLICT (identity_id) DO NOTHING
        "#,
    )
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    let preference = sqlx::query_as::<_, (bool, DateTime<Utc>)>(
        r#"
        SELECT auto_export_enabled, updated_at
        FROM identity_retention_preferences
        WHERE identity_id = $1
        "#,
    )
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(RetentionPreferenceResponse {
        preference: RetentionPreferenceDto {
            auto_export_enabled: preference.0,
            updated_at: preference.1,
        },
    }))
}

pub async fn update_preference(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Json(request): Json<UpdateRetentionPreferenceRequest>,
) -> Result<Json<RetentionPreferenceResponse>, AppError> {
    let mut transaction = begin(&state, actor).await?;
    let preference = sqlx::query_as::<_, (bool, DateTime<Utc>)>(
        r#"
        INSERT INTO identity_retention_preferences (
            identity_id, auto_export_enabled
        )
        VALUES ($1, $2)
        ON CONFLICT (identity_id) DO UPDATE
        SET auto_export_enabled = EXCLUDED.auto_export_enabled
        RETURNING auto_export_enabled, updated_at
        "#,
    )
    .bind(actor.identity_id)
    .bind(request.auto_export_enabled)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(RetentionPreferenceResponse {
        preference: RetentionPreferenceDto {
            auto_export_enabled: preference.0,
            updated_at: preference.1,
        },
    }))
}

pub async fn list_archives(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
) -> Result<Json<ListRetentionArchivesResponse>, AppError> {
    let mut transaction = begin(&state, actor).await?;
    let rows = sqlx::query_as::<_, ArchiveListRow>(
        r#"
        SELECT
            archive.id,
            archive.project_id,
            subject.source_kind,
            subject.source_id,
            archive.state,
            archive.created_at,
            archive.completed_at,
            archive.source_purged_at,
            archive.expires_at,
            receipt.received_at AS downloaded_at
        FROM retention_archives archive
        JOIN retention_subjects subject ON subject.id = archive.subject_id
        LEFT JOIN retention_archive_receipts receipt
          ON receipt.archive_id = archive.id
         AND receipt.recipient_identity_id = archive.recipient_identity_id
        WHERE archive.recipient_identity_id = $1
          AND (
              archive.expires_at IS NULL
              OR archive.expires_at > clock_timestamp()
          )
        ORDER BY archive.created_at DESC, archive.id
        "#,
    )
    .bind(actor.identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let archives = rows
        .into_iter()
        .map(ArchiveListRow::into_dto)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ListRetentionArchivesResponse { archives }))
}

pub async fn list_warnings(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
) -> Result<Json<ListRetentionWarningsResponse>, AppError> {
    let mut transaction = begin(&state, actor).await?;
    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
        r#"
        WITH visible AS (
            SELECT id
            FROM notifications
            WHERE recipient_identity_id = $1
              AND notification_kind = 'retention_warning'
              AND delivery_channel = 'in_app'
              AND state IN ('pending', 'delivered')
            ORDER BY created_at DESC, id
            LIMIT 100
        )
        UPDATE notifications notification
        SET
            state = CASE
                WHEN notification.state = 'pending' THEN 'delivered'
                ELSE notification.state
            END,
            delivered_at = COALESCE(notification.delivered_at, clock_timestamp())
        FROM visible
        WHERE notification.id = visible.id
        RETURNING
            notification.id,
            notification.project_id,
            notification.state,
            notification.scheduled_at,
            notification.created_at
        "#,
    )
    .bind(actor.identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ListRetentionWarningsResponse {
        warnings: rows
            .into_iter()
            .map(
                |(id, project_id, state, scheduled_at, created_at)| RetentionWarningDto {
                    id,
                    project_id,
                    state,
                    scheduled_at,
                    created_at,
                },
            )
            .collect(),
    }))
}

pub async fn download_archive(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(archive_id): Path<Uuid>,
) -> Result<Response<Body>, AppError> {
    let mut transaction = begin(&state, actor).await?;
    let row = sqlx::query_as::<_, ArchiveDownloadRow>(
        r#"
        SELECT
            storage_key,
            ciphertext_size,
            ciphertext_sha256,
            canonical_manifest,
            manifest_signature
        FROM retention_archives
        WHERE id = $1
          AND recipient_identity_id = $2
          AND state = 'succeeded'
          AND (
              expires_at IS NULL
              OR expires_at > clock_timestamp()
          )
        "#,
    )
    .bind(archive_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;

    let bytes = archive_store::read(&state.config.archive_dir, &row.storage_key).await?;
    let digest = Sha256::digest(&bytes);
    validate_retention_archive_integrity(
        RetentionArchiveIntegrity {
            declared_size: row.ciphertext_size,
            declared_sha256: &row.ciphertext_sha256,
            canonical_manifest: &row.canonical_manifest,
            manifest_signature: &row.manifest_signature,
        },
        bytes.len(),
        &digest,
    )
    .map_err(|_| AppError::Internal)?;
    verify_manifest_signature(&state, &row.canonical_manifest, &row.manifest_signature)?;

    Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        )
        .header(
            CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!(
                "attachment; filename=\"sprout-retention-{}.archive\"",
                archive_id.simple()
            ))
            .map_err(|_| AppError::Internal)?,
        )
        .header(CACHE_CONTROL, HeaderValue::from_static("private, no-store"))
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .map_err(|_| AppError::Internal)
}

pub async fn record_receipt(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(archive_id): Path<Uuid>,
    Json(request): Json<RecordArchiveReceiptRequest>,
) -> Result<Json<ArchiveReceiptResponse>, AppError> {
    let supplied = base64::engine::general_purpose::STANDARD
        .decode(&request.ciphertext_sha256.0)
        .map_err(|_| AppError::BadRequest("invalid archive checksum"))?;
    let mut transaction = begin(&state, actor).await?;
    let expected = sqlx::query_scalar::<_, Vec<u8>>(
        r#"
        SELECT ciphertext_sha256
        FROM retention_archives
        WHERE id = $1
          AND recipient_identity_id = $2
          AND state = 'succeeded'
          AND (
              expires_at IS NULL
              OR expires_at > clock_timestamp()
          )
        "#,
    )
    .bind(archive_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if supplied != expected {
        return Err(AppError::BadRequest("archive checksum does not match"));
    }
    let received_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        INSERT INTO retention_archive_receipts (
            project_id, archive_id, recipient_identity_id, ciphertext_sha256
        )
        SELECT project_id, id, recipient_identity_id, ciphertext_sha256
        FROM retention_archives
        WHERE id = $1 AND recipient_identity_id = $2
        ON CONFLICT (archive_id, recipient_identity_id) DO UPDATE
        SET ciphertext_sha256 = retention_archive_receipts.ciphertext_sha256
        RETURNING received_at
        "#,
    )
    .bind(archive_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ArchiveReceiptResponse {
        receipt: ArchiveReceiptDto {
            archive_id,
            received_at,
            ciphertext_sha256: OpaqueDigestDto(
                base64::engine::general_purpose::STANDARD.encode(expected),
            ),
        },
    }))
}

fn verify_manifest_signature(
    state: &AppState,
    manifest: &[u8],
    signature: &[u8],
) -> Result<(), AppError> {
    let signing = SigningKey::from_bytes(state.config.archive_signing_key.expose());
    let signature = Signature::from_slice(signature).map_err(|_| AppError::Internal)?;
    signing
        .verifying_key()
        .verify_strict(manifest, &signature)
        .map_err(|_| AppError::Internal)
}

async fn begin(
    state: &AppState,
    actor: AuthSession,
) -> Result<Transaction<'_, Postgres>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    Ok(transaction)
}

fn parse_state(value: &str) -> Result<RetentionArchiveStateDto, AppError> {
    match value {
        "pending" => Ok(RetentionArchiveStateDto::Pending),
        "running" => Ok(RetentionArchiveStateDto::Running),
        "succeeded" => Ok(RetentionArchiveStateDto::Succeeded),
        "failed" => Ok(RetentionArchiveStateDto::Failed),
        _ => Err(AppError::Internal),
    }
}

#[derive(FromRow)]
struct ArchiveListRow {
    id: Uuid,
    project_id: Uuid,
    source_kind: String,
    source_id: Uuid,
    state: String,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    source_purged_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    downloaded_at: Option<DateTime<Utc>>,
}

impl ArchiveListRow {
    fn into_dto(self) -> Result<RetentionArchiveDto, AppError> {
        Ok(RetentionArchiveDto {
            id: self.id,
            project_id: self.project_id,
            source_kind: self.source_kind,
            source_id: self.source_id,
            state: parse_state(&self.state)?,
            created_at: self.created_at,
            completed_at: self.completed_at,
            source_purged_at: self.source_purged_at,
            expires_at: self.expires_at,
            downloaded_at: self.downloaded_at,
        })
    }
}

#[derive(FromRow)]
struct ArchiveDownloadRow {
    storage_key: String,
    ciphertext_size: i64,
    ciphertext_sha256: Vec<u8>,
    canonical_manifest: Vec<u8>,
    manifest_signature: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llr_08_8_download_is_forced_and_never_claimed_automatic() {
        let archive_id = Uuid::from_u128(42);
        let disposition = format!(
            "attachment; filename=\"sprout-retention-{}.archive\"",
            archive_id.simple()
        );
        assert!(disposition.starts_with("attachment;"));
        assert!(!disposition.contains("automatic"));
    }
}
