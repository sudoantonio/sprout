use std::{fmt, sync::Arc};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload, array::Array},
};
use axum::{Json, extract::State, http::StatusCode};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::webauthn::{DeviceSessionRequest, SessionResponse, create_device_session, decode_b64};
use crate::{
    AppState,
    auth::set_database_context,
    config::{Config, DeploymentEnvironment},
    error::AppError,
};

const SIGNUP_KIND: &str = "signup_verification";
const RECOVERY_KIND: &str = "account_recovery";

#[derive(Deserialize)]
pub struct SignupStartRequest {
    email: String,
    identity_handle: String,
    encrypted_profile_b64: String,
}

impl fmt::Debug for SignupStartRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SignupStartRequest")
            .field("email", &"[REDACTED]")
            .field("identity_handle", &"[REDACTED]")
            .field("encrypted_profile_b64", &"[REDACTED]")
            .finish()
    }
}

#[derive(Serialize)]
pub struct AcceptedResponse {
    accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dev_verification_token: Option<String>,
}

fn accepted_response(
    config: &Config,
    identity_id: Option<Uuid>,
    token: Option<&str>,
) -> AcceptedResponse {
    let include_dev_fields = config.deployment_environment == DeploymentEnvironment::Development;
    AcceptedResponse {
        accepted: true,
        identity_id: include_dev_fields.then_some(identity_id).flatten(),
        dev_verification_token: include_dev_fields
            .then(|| token.map(str::to_owned))
            .flatten(),
    }
}

pub async fn verification_start(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SignupStartRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), AppError> {
    let normalized_email = normalize_email(&request.email)?;
    let handle = normalize_handle(&request.identity_handle)?;
    let encrypted_profile =
        decode_b64(&request.encrypted_profile_b64, "invalid encrypted profile")?;
    if encrypted_profile.is_empty() {
        return Err(AppError::BadRequest("encrypted profile is empty"));
    }
    if state.config.deployment_environment == DeploymentEnvironment::Development {
        if let Some(existing_id) = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT i.id
            FROM identities i
            INNER JOIN identity_emails e ON e.identity_id = i.id
            WHERE i.identity_handle = $1
              AND e.normalized_email = $2
              AND i.status = 'pending'
            "#,
        )
        .bind(&handle)
        .bind(&normalized_email)
        .fetch_optional(&state.pool)
        .await?
        {
            return refresh_signup_verification(
                &state,
                existing_id,
                &normalized_email,
                encrypted_profile,
            )
            .await;
        }
        if let Some(existing_id) = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT i.id
            FROM identities i
            INNER JOIN identity_emails e ON e.identity_id = i.id
            WHERE i.identity_handle = $1
              AND e.normalized_email = $2
              AND i.status = 'active'
            "#,
        )
        .bind(&handle)
        .bind(&normalized_email)
        .fetch_optional(&state.pool)
        .await?
        {
            return Ok((
                StatusCode::ACCEPTED,
                Json(AcceptedResponse {
                    accepted: true,
                    identity_id: Some(existing_id),
                    dev_verification_token: None,
                }),
            ));
        }
    }
    let identity_id = Uuid::new_v4();
    let token = new_token();
    let token_hash = hash_token(&token);
    let expires_at = Utc::now()
        + Duration::from_std(state.config.email_verification_ttl)
            .map_err(|_| AppError::Internal)?;
    let payload = EmailTokenPayload {
        identity_id,
        token: &token,
    };
    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, identity_id, None, None).await?;
    sqlx::query(
        r#"
        INSERT INTO identities (
            id, identity_handle, encrypted_profile, status
        )
        VALUES ($1, $2, $3, 'pending')
        "#,
    )
    .bind(identity_id)
    .bind(handle)
    .bind(encrypted_profile)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO identity_emails (identity_id, normalized_email)
        VALUES ($1, $2)
        "#,
    )
    .bind(identity_id)
    .bind(&normalized_email)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO email_verification_tokens (
            identity_id, token_hash, expires_at
        )
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(identity_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;
    enqueue_email(
        &mut transaction,
        &state.config,
        identity_id,
        SIGNUP_KIND,
        &normalized_email,
        &token_hash,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(accepted_response(
            &state.config,
            Some(identity_id),
            Some(&token),
        )),
    ))
}

async fn refresh_signup_verification(
    state: &AppState,
    identity_id: Uuid,
    normalized_email: &str,
    encrypted_profile: Vec<u8>,
) -> Result<(StatusCode, Json<AcceptedResponse>), AppError> {
    let token = new_token();
    let token_hash = hash_token(&token);
    let expires_at = Utc::now()
        + Duration::from_std(state.config.email_verification_ttl)
            .map_err(|_| AppError::Internal)?;
    let payload = EmailTokenPayload {
        identity_id,
        token: &token,
    };
    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, identity_id, None, None).await?;
    sqlx::query(
        r#"
        UPDATE identities
        SET encrypted_profile = $2
        WHERE id = $1 AND status = 'pending'
        "#,
    )
    .bind(identity_id)
    .bind(encrypted_profile)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE email_verification_tokens
        SET consumed_at = clock_timestamp()
        WHERE identity_id = $1 AND consumed_at IS NULL
        "#,
    )
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO email_verification_tokens (
            identity_id, token_hash, expires_at
        )
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(identity_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;
    enqueue_email(
        &mut transaction,
        &state.config,
        identity_id,
        SIGNUP_KIND,
        normalized_email,
        &token_hash,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(accepted_response(
            &state.config,
            Some(identity_id),
            Some(&token),
        )),
    ))
}

#[derive(Deserialize)]
pub struct VerificationFinishRequest {
    identity_id: Uuid,
    token: String,
    #[serde(flatten)]
    device: DeviceSessionRequest,
}

pub async fn verification_finish(
    State(state): State<Arc<AppState>>,
    Json(request): Json<VerificationFinishRequest>,
) -> Result<Json<SessionResponse>, AppError> {
    let token_hash = checked_token_hash(&request.token)?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, request.identity_id, None, None).await?;
    let consumed = sqlx::query_scalar::<_, Uuid>(
        r#"
        UPDATE email_verification_tokens
        SET consumed_at = clock_timestamp()
        WHERE identity_id = $1
          AND token_hash = $2
          AND consumed_at IS NULL
          AND expires_at > clock_timestamp()
        RETURNING id
        "#,
    )
    .bind(request.identity_id)
    .bind(token_hash)
    .fetch_optional(&mut *transaction)
    .await?
    .is_some();
    if !consumed {
        let already_active =
            sqlx::query_scalar::<_, bool>("SELECT status = 'active' FROM identities WHERE id = $1")
                .bind(request.identity_id)
                .fetch_optional(&mut *transaction)
                .await?
                .unwrap_or(false);
        if already_active {
            return Err(AppError::BadRequest(
                "account is already verified; use passkey sign-in or account recovery",
            ));
        }
        return Err(AppError::BadRequest(
            "verification token is invalid or expired",
        ));
    }
    let activated = sqlx::query(
        r#"
        UPDATE identities
        SET status = 'active'
        WHERE id = $1 AND status = 'pending'
        "#,
    )
    .bind(request.identity_id)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if activated != 1 {
        return Err(AppError::BadRequest(
            "verification token is invalid or expired",
        ));
    }
    sqlx::query(
        r#"
        UPDATE identity_emails
        SET verified_at = clock_timestamp()
        WHERE identity_id = $1 AND verified_at IS NULL
        "#,
    )
    .bind(request.identity_id)
    .execute(&mut *transaction)
    .await?;
    let session = create_device_session(
        &mut transaction,
        &state.config,
        request.identity_id,
        request.device,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(session))
}

#[derive(Deserialize)]
pub struct RecoveryStartRequest {
    email: String,
}

impl fmt::Debug for RecoveryStartRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecoveryStartRequest")
            .field("email", &"[REDACTED]")
            .finish()
    }
}

pub async fn recovery_start(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RecoveryStartRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), AppError> {
    let normalized_email = normalize_email(&request.email)?;
    // Generate and hash a token even for unknown accounts so the public path
    // performs the same cryptographic work and always returns the same shape.
    let token = new_token();
    let token_hash = hash_token(&token);
    let identity_id = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT sprout_private.active_identity_for_email($1)",
    )
    .bind(&normalized_email)
    .fetch_one(&state.pool)
    .await?;
    if let Some(identity_id) = identity_id {
        let expires_at = Utc::now()
            + Duration::from_std(state.config.account_recovery_ttl)
                .map_err(|_| AppError::Internal)?;
        let payload = EmailTokenPayload {
            identity_id,
            token: &token,
        };
        let mut transaction = state.pool.begin().await?;
        set_database_context(&mut transaction, identity_id, None, None).await?;
        sqlx::query(
            r#"
            UPDATE account_recovery_tokens
            SET consumed_at = clock_timestamp()
            WHERE identity_id = $1 AND consumed_at IS NULL
            "#,
        )
        .bind(identity_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            DELETE FROM account_recovery_tokens
            WHERE identity_id = $1
              AND (
                  expires_at < clock_timestamp() - interval '1 day'
                  OR consumed_at < clock_timestamp() - interval '1 day'
              )
            "#,
        )
        .bind(identity_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO account_recovery_tokens (
                identity_id, token_hash, expires_at
            )
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(identity_id)
        .bind(token_hash)
        .bind(expires_at)
        .execute(&mut *transaction)
        .await?;
        enqueue_email(
            &mut transaction,
            &state.config,
            identity_id,
            RECOVERY_KIND,
            &normalized_email,
            &token_hash,
            &payload,
        )
        .await?;
        transaction.commit().await?;
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(accepted_response(
            &state.config,
            identity_id,
            identity_id.is_some().then_some(token.as_str()),
        )),
    ))
}

#[derive(Deserialize)]
pub struct RecoveryFinishRequest {
    identity_id: Uuid,
    token: String,
    #[serde(flatten)]
    device: DeviceSessionRequest,
}

pub async fn recovery_finish(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RecoveryFinishRequest>,
) -> Result<Json<SessionResponse>, AppError> {
    let token_hash = checked_token_hash(&request.token)?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, request.identity_id, None, None).await?;
    let valid_identity = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM identities WHERE id = $1 AND status = 'active')",
    )
    .bind(request.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if !valid_identity {
        return Err(AppError::BadRequest("recovery token is invalid or expired"));
    }
    let consumed = sqlx::query_scalar::<_, Uuid>(
        r#"
        UPDATE account_recovery_tokens
        SET consumed_at = clock_timestamp()
        WHERE identity_id = $1
          AND token_hash = $2
          AND consumed_at IS NULL
          AND expires_at > clock_timestamp()
        RETURNING id
        "#,
    )
    .bind(request.identity_id)
    .bind(token_hash)
    .fetch_optional(&mut *transaction)
    .await?
    .is_some();
    if !consumed {
        return Err(AppError::BadRequest("recovery token is invalid or expired"));
    }
    let session = create_device_session(
        &mut transaction,
        &state.config,
        request.identity_id,
        request.device,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(session))
}

#[derive(Serialize)]
struct EmailTokenPayload<'a> {
    identity_id: Uuid,
    token: &'a str,
}

pub(super) async fn enqueue_email<T: Serialize>(
    transaction: &mut Transaction<'_, Postgres>,
    config: &Config,
    identity_id: Uuid,
    message_kind: &'static str,
    recipient_email: &str,
    token_hash: &[u8; 32],
    payload: &T,
) -> Result<(), AppError> {
    let plaintext = serde_json::to_vec(payload).map_err(|_| AppError::Internal)?;
    let (nonce, encrypted_payload) =
        encrypt_outbox_payload(config, message_kind, identity_id, &plaintext)?;
    sqlx::query(
        r#"
        INSERT INTO email_outbox (
            identity_id, message_kind, recipient_email, token_hash,
            payload_nonce, encrypted_payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(identity_id)
    .bind(message_kind)
    .bind(recipient_email)
    .bind(token_hash.as_slice())
    .bind(nonce.as_slice())
    .bind(encrypted_payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn enqueue_retention_warning<T: Serialize>(
    transaction: &mut Transaction<'_, Postgres>,
    config: &Config,
    identity_id: Uuid,
    recipient_email: &str,
    deduplication_key: &[u8; 32],
    payload: &T,
) -> Result<(), AppError> {
    let plaintext = serde_json::to_vec(payload).map_err(|_| AppError::Internal)?;
    let (nonce, encrypted_payload) =
        encrypt_outbox_payload(config, "retention_warning", identity_id, &plaintext)?;
    sqlx::query(
        r#"
        INSERT INTO email_outbox (
            identity_id, message_kind, recipient_email, token_hash,
            payload_nonce, encrypted_payload, deduplication_key
        )
        VALUES ($1, 'retention_warning', $2, $3, $4, $5, $3)
        ON CONFLICT (identity_id, message_kind, deduplication_key)
            WHERE deduplication_key IS NOT NULL
        DO NOTHING
        "#,
    )
    .bind(identity_id)
    .bind(recipient_email)
    .bind(deduplication_key.as_slice())
    .bind(nonce.as_slice())
    .bind(encrypted_payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn encrypt_outbox_payload(
    config: &Config,
    message_kind: &str,
    identity_id: Uuid,
    plaintext: &[u8],
) -> Result<([u8; 12], Vec<u8>), AppError> {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let digest = Sha256::new()
        .chain_update(first.as_bytes())
        .chain_update(second.as_bytes())
        .finalize();
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&digest[..12]);
    let mut aad = Vec::with_capacity(message_kind.len() + 16);
    aad.extend_from_slice(message_kind.as_bytes());
    aad.extend_from_slice(identity_id.as_bytes());
    let cipher = Aes256Gcm::new(&Array(*config.email_outbox_key.expose()));
    let encrypted = cipher
        .encrypt(
            &Array(nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| AppError::Internal)?;
    Ok((nonce, encrypted))
}

pub(super) fn normalize_email(value: &str) -> Result<String, AppError> {
    let normalized = value.trim().to_lowercase();
    let mut parts = normalized.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if normalized.len() < 3
        || normalized.len() > 320
        || local.is_empty()
        || domain.is_empty()
        || parts.next().is_some()
        || normalized.chars().any(|character| {
            character.is_whitespace() || character.is_control() || !character.is_ascii()
        })
    {
        return Err(AppError::BadRequest("invalid email address"));
    }
    Ok(normalized)
}

pub(super) fn normalize_handle(value: &str) -> Result<String, AppError> {
    let normalized = value.trim().to_lowercase();
    if normalized.len() < 3
        || normalized.len() > 128
        || normalized.chars().any(|character| {
            character.is_whitespace() || character.is_control() || !character.is_ascii()
        })
    {
        return Err(AppError::BadRequest("invalid identity handle"));
    }
    Ok(normalized)
}

pub(super) fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub(super) fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub(super) fn checked_token_hash(token: &str) -> Result<[u8; 32], AppError> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::BadRequest("token is invalid or expired"));
    }
    Ok(hash_token(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_normalization_is_strict_and_lowercase() {
        assert_eq!(
            normalize_email("  User@Example.COM ").unwrap(),
            "user@example.com"
        );
        assert!(normalize_email("not-an-email").is_err());
        assert!(normalize_email("a@b@example.com").is_err());
    }

    #[test]
    fn encrypted_outbox_payload_does_not_contain_token() {
        let config = Config::for_test();
        let token = new_token();
        let payload = serde_json::to_vec(&EmailTokenPayload {
            identity_id: Uuid::nil(),
            token: &token,
        })
        .unwrap();
        let (_, encrypted) =
            encrypt_outbox_payload(&config, SIGNUP_KIND, Uuid::nil(), &payload).unwrap();
        assert!(
            !encrypted
                .windows(token.len())
                .any(|window| window == token.as_bytes())
        );
    }
}
