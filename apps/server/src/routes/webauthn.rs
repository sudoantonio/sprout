use std::{fmt, sync::Arc};

use axum::{Json, extract::State};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PasskeyAuthentication, PasskeyRegistration,
    PublicKeyCredential, RegisterPublicKeyCredential, RequestChallengeResponse,
};

use crate::{
    AppState,
    auth::{AuthSession, set_database_context},
    config::Config,
    error::AppError,
};

const REGISTRATION_KIND: &str = "registration";
const AUTHENTICATION_KIND: &str = "authentication";

#[derive(Deserialize)]
pub struct CeremonyFinish<T> {
    challenge_id: Uuid,
    credential: T,
}

#[derive(Serialize)]
pub struct ChallengeResponse<T> {
    challenge_id: Uuid,
    options: T,
}

pub async fn registration_start(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
) -> Result<Json<ChallengeResponse<CreationChallengeResponse>>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    let handle = sqlx::query_scalar::<_, String>(
        "SELECT identity_handle FROM identities WHERE id = $1 AND status = 'active'",
    )
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let passkeys = load_passkeys(&mut transaction, actor.identity_id).await?;
    let excluded = passkeys
        .iter()
        .map(|passkey| passkey.cred_id().to_owned())
        .collect::<Vec<_>>();
    let (options, registration) = state
        .webauthn
        .start_passkey_registration(
            actor.identity_id,
            &handle,
            &handle,
            (!excluded.is_empty()).then_some(excluded),
        )
        .map_err(|_| AppError::BadRequest("passkey registration could not be started"))?;
    let challenge_id = Uuid::new_v4();
    store_ceremony(
        &mut transaction,
        actor.identity_id,
        challenge_id,
        REGISTRATION_KIND,
        &registration,
        state.config.ceremony_ttl,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ChallengeResponse {
        challenge_id,
        options,
    }))
}

pub async fn registration_finish(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Json(request): Json<CeremonyFinish<RegisterPublicKeyCredential>>,
) -> Result<Json<RegistrationResponse>, AppError> {
    let serialized = consume_ceremony(
        &state,
        actor.identity_id,
        Some(actor.device_id),
        request.challenge_id,
        REGISTRATION_KIND,
    )
    .await?;
    let registration: PasskeyRegistration =
        deserialize_ceremony(&serialized, "invalid registration ceremony")?;
    let passkey = state
        .webauthn
        .finish_passkey_registration(&request.credential, &registration)
        .map_err(|_| AppError::BadRequest("passkey registration failed"))?;
    let credential_id = passkey.cred_id().as_ref().to_vec();
    let serialized_passkey = serde_json::to_vec(&passkey).map_err(|_| AppError::Internal)?;
    let passkey_id = Uuid::new_v4();
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    sqlx::query(
        r#"
        INSERT INTO passkeys (
            id, identity_id, credential_id, public_key_cose,
            sign_count, backup_eligible, backup_state
        )
        VALUES ($1, $2, $3, $4, 0, false, false)
        "#,
    )
    .bind(passkey_id)
    .bind(actor.identity_id)
    .bind(credential_id)
    .bind(serialized_passkey)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(RegistrationResponse { passkey_id }))
}

#[derive(Serialize)]
pub struct RegistrationResponse {
    passkey_id: Uuid,
}

#[derive(Deserialize)]
pub struct AuthenticationStart {
    identity_id: Uuid,
    identity_handle: String,
}

impl fmt::Debug for AuthenticationStart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticationStart")
            .field("identity_id", &self.identity_id)
            .field("identity_handle", &"[REDACTED]")
            .finish()
    }
}

pub async fn authentication_start(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AuthenticationStart>,
) -> Result<Json<ChallengeResponse<RequestChallengeResponse>>, AppError> {
    if request.identity_handle.len() < 3 || request.identity_handle.len() > 128 {
        return Err(AppError::Unauthorized);
    }
    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, request.identity_id, None, None).await?;
    let matched = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM identities
            WHERE id = $1 AND identity_handle = $2 AND status = 'active'
        )
        "#,
    )
    .bind(request.identity_id)
    .bind(&request.identity_handle)
    .fetch_one(&mut *transaction)
    .await?;
    if !matched {
        return Err(AppError::Unauthorized);
    }
    let passkeys = load_passkeys(&mut transaction, request.identity_id).await?;
    if passkeys.is_empty() {
        return Err(AppError::Unauthorized);
    }
    let (options, authentication) = state
        .webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(|_| AppError::Unauthorized)?;
    let challenge_id = Uuid::new_v4();
    store_ceremony(
        &mut transaction,
        request.identity_id,
        challenge_id,
        AUTHENTICATION_KIND,
        &authentication,
        state.config.ceremony_ttl,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ChallengeResponse {
        challenge_id,
        options,
    }))
}

#[derive(Deserialize)]
pub struct AuthenticationFinish {
    identity_id: Uuid,
    challenge_id: Uuid,
    credential: PublicKeyCredential,
    #[serde(flatten)]
    device: DeviceSessionRequest,
}

#[derive(Deserialize)]
pub(super) struct DeviceSessionRequest {
    pub device_id: Uuid,
    pub device_kind: DeviceKind,
    pub encrypted_device_label_b64: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DeviceKind {
    Web,
    Ios,
    Android,
    Desktop,
    Other,
}

impl DeviceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Ios => "ios",
            Self::Android => "android",
            Self::Desktop => "desktop",
            Self::Other => "other",
        }
    }
}

pub async fn authentication_finish(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AuthenticationFinish>,
) -> Result<Json<SessionResponse>, AppError> {
    let serialized = consume_ceremony(
        &state,
        request.identity_id,
        None,
        request.challenge_id,
        AUTHENTICATION_KIND,
    )
    .await?;
    let authentication: PasskeyAuthentication =
        deserialize_ceremony(&serialized, "invalid authentication ceremony")?;
    let result = state
        .webauthn
        .finish_passkey_authentication(&request.credential, &authentication)
        .map_err(|_| AppError::Unauthorized)?;
    if !result.user_verified() {
        return Err(AppError::Unauthorized);
    }

    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, request.identity_id, None, None).await?;
    let mut passkeys = load_passkeys(&mut transaction, request.identity_id).await?;
    let passkey = passkeys
        .iter_mut()
        .find(|passkey| passkey.cred_id() == result.cred_id())
        .ok_or(AppError::Unauthorized)?;
    passkey
        .update_credential(&result)
        .ok_or(AppError::Unauthorized)?;
    let serialized_passkey = serde_json::to_vec(passkey).map_err(|_| AppError::Internal)?;
    let changed = sqlx::query(
        r#"
        UPDATE passkeys
        SET public_key_cose = $1,
            sign_count = $2,
            backup_eligible = $3,
            backup_state = $4,
            last_used_at = clock_timestamp()
        WHERE identity_id = $5 AND credential_id = $6 AND revoked_at IS NULL
        "#,
    )
    .bind(serialized_passkey)
    .bind(i64::from(result.counter()))
    .bind(result.backup_eligible())
    .bind(result.backup_state())
    .bind(request.identity_id)
    .bind(result.cred_id().as_ref())
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(AppError::Unauthorized);
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
pub(super) struct SessionResponse {
    token: SessionToken,
    expires_at: DateTime<Utc>,
    identity_id: Uuid,
    device_id: Uuid,
}

#[derive(Serialize)]
#[serde(transparent)]
struct SessionToken(String);

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken([REDACTED])")
    }
}

pub(super) async fn create_device_session(
    transaction: &mut Transaction<'_, Postgres>,
    config: &Config,
    identity_id: Uuid,
    request: DeviceSessionRequest,
) -> Result<SessionResponse, AppError> {
    let label = decode_b64(
        &request.encrypted_device_label_b64,
        "invalid encrypted device label",
    )?;
    if label.is_empty() {
        return Err(AppError::BadRequest("encrypted device label is empty"));
    }
    sqlx::query(
        r#"
        INSERT INTO devices (id, identity_id, device_kind, encrypted_label, trust_state)
        VALUES ($1, $2, $3, $4, 'trusted')
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(request.device_id)
    .bind(identity_id)
    .bind(request.device_kind.as_str())
    .bind(label)
    .execute(&mut **transaction)
    .await?;
    let device_owner = sqlx::query_scalar::<_, Uuid>(
        "SELECT identity_id FROM devices WHERE id = $1 AND retired_at IS NULL",
    )
    .bind(request.device_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    if device_owner != identity_id {
        return Err(AppError::Forbidden);
    }

    let session_id = Uuid::new_v4();
    let token = SessionToken(format!(
        "v1.{identity_id}.{session_id}.{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ));
    let token_hash = Sha256::digest(token.0.as_bytes()).to_vec();
    let expires_at =
        Utc::now() + Duration::from_std(config.session_ttl).map_err(|_| AppError::Internal)?;
    sqlx::query(
        r#"
        INSERT INTO sessions (id, identity_id, device_id, token_hash, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(session_id)
    .bind(identity_id)
    .bind(request.device_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await?;
    Ok(SessionResponse {
        token,
        expires_at,
        identity_id,
        device_id: request.device_id,
    })
}

async fn store_ceremony<T: Serialize>(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: Uuid,
    ceremony_id: Uuid,
    ceremony_kind: &'static str,
    state: &T,
    ttl: std::time::Duration,
) -> Result<(), AppError> {
    let serialized_state = serde_json::to_vec(state).map_err(|_| AppError::Internal)?;
    let expires_at = Utc::now() + Duration::from_std(ttl).map_err(|_| AppError::Internal)?;
    sqlx::query(
        r#"
        UPDATE webauthn_ceremonies
        SET consumed_at = clock_timestamp()
        WHERE identity_id = $1
          AND ceremony_kind = $2
          AND consumed_at IS NULL
        "#,
    )
    .bind(identity_id)
    .bind(ceremony_kind)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        DELETE FROM webauthn_ceremonies
        WHERE identity_id = $1
          AND (
              expires_at < clock_timestamp() - interval '1 day'
              OR consumed_at < clock_timestamp() - interval '1 day'
          )
        "#,
    )
    .bind(identity_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO webauthn_ceremonies (
            id, identity_id, ceremony_kind, serialized_state, expires_at
        )
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(ceremony_id)
    .bind(identity_id)
    .bind(ceremony_kind)
    .bind(serialized_state)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn consume_ceremony(
    state: &AppState,
    identity_id: Uuid,
    device_id: Option<Uuid>,
    ceremony_id: Uuid,
    ceremony_kind: &'static str,
) -> Result<Vec<u8>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, identity_id, device_id, None).await?;
    let serialized = sqlx::query_scalar::<_, Vec<u8>>(
        r#"
        UPDATE webauthn_ceremonies
        SET consumed_at = clock_timestamp()
        WHERE id = $1
          AND identity_id = $2
          AND ceremony_kind = $3
          AND consumed_at IS NULL
          AND expires_at > clock_timestamp()
        RETURNING serialized_state
        "#,
    )
    .bind(ceremony_id)
    .bind(identity_id)
    .bind(ceremony_kind)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Unauthorized)?;
    transaction.commit().await?;
    Ok(serialized)
}

fn deserialize_ceremony<T: DeserializeOwned>(
    serialized: &[u8],
    message: &'static str,
) -> Result<T, AppError> {
    serde_json::from_slice(serialized).map_err(|_| {
        tracing::error!("stored WebAuthn ceremony state could not be decoded");
        AppError::BadRequest(message)
    })
}

async fn load_passkeys(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: Uuid,
) -> Result<Vec<Passkey>, AppError> {
    let rows = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT public_key_cose FROM passkeys WHERE identity_id = $1 AND revoked_at IS NULL",
    )
    .bind(identity_id)
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| AppError::Internal))
        .collect()
}

pub(super) fn decode_b64(value: &str, message: &'static str) -> Result<Vec<u8>, AppError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_request_debug_redacts_identity_handle() {
        let request = AuthenticationStart {
            identity_id: Uuid::nil(),
            identity_handle: "private-handle".into(),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("private-handle"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn session_tokens_are_redacted() {
        let token = SessionToken("credential-material".into());
        assert!(!format!("{token:?}").contains("credential-material"));
    }
}
