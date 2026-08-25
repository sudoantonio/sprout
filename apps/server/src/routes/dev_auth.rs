use std::{fmt, sync::Arc};

use axum::{Json, extract::State};
use serde::Deserialize;
use uuid::Uuid;

use super::email::{normalize_email, normalize_handle};
use super::webauthn::{DeviceSessionRequest, SessionResponse, create_device_session};
use crate::{AppState, auth::set_database_context, config::DeploymentEnvironment, error::AppError};

#[derive(Deserialize)]
pub struct DevLoginRequest {
    email: Option<String>,
    identity_handle: Option<String>,
    #[serde(flatten)]
    device: DeviceSessionRequest,
}

impl fmt::Debug for DevLoginRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DevLoginRequest")
            .field("email", &"[REDACTED]")
            .field("identity_handle", &"[REDACTED]")
            .field("device", &"[REDACTED]")
            .finish()
    }
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DevLoginRequest>,
) -> Result<Json<SessionResponse>, AppError> {
    if state.config.deployment_environment != DeploymentEnvironment::Development {
        return Err(AppError::NotFound);
    }
    let identity_id = resolve_identity_id(&state, &request).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(&mut transaction, identity_id, None, None).await?;
    sqlx::query(
        r#"
        UPDATE identities
        SET status = 'active'
        WHERE id = $1 AND status = 'pending'
        "#,
    )
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE identity_emails
        SET verified_at = clock_timestamp()
        WHERE identity_id = $1 AND verified_at IS NULL
        "#,
    )
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    let session =
        create_device_session(&mut transaction, &state.config, identity_id, request.device).await?;
    transaction.commit().await?;
    Ok(Json(session))
}

async fn resolve_identity_id(
    state: &AppState,
    request: &DevLoginRequest,
) -> Result<Uuid, AppError> {
    if request.email.is_none() && request.identity_handle.is_none() {
        return Err(AppError::BadRequest("email or identity_handle is required"));
    }
    if let Some(email) = &request.email {
        let normalized_email = normalize_email(email)?;
        if let Some(identity_id) = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT sprout_private.active_identity_for_email($1)",
        )
        .bind(&normalized_email)
        .fetch_one(&state.pool)
        .await?
        {
            return Ok(identity_id);
        }
        if let Some(identity_id) = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT i.id
            FROM identities i
            INNER JOIN identity_emails e ON e.identity_id = i.id
            WHERE e.normalized_email = $1 AND i.status = 'pending'
            "#,
        )
        .bind(&normalized_email)
        .fetch_optional(&state.pool)
        .await?
        {
            return Ok(identity_id);
        }
    }
    if let Some(handle) = &request.identity_handle {
        let normalized_handle = normalize_handle(handle)?;
        if let Some(identity_id) = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
            FROM identities
            WHERE identity_handle = $1 AND status IN ('active', 'pending')
            "#,
        )
        .bind(&normalized_handle)
        .fetch_optional(&state.pool)
        .await?
        {
            return Ok(identity_id);
        }
    }
    Err(AppError::BadRequest("no matching development account"))
}
