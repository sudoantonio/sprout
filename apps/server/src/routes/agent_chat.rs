//! Creator-only read projections for the browser chat control plane.
//! Generation and all effects still pass through the governed runner routes.
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sprout_domain::EncryptedPayload;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthSession, ProjectAccess, require_project_access, set_database_context},
    error::AppError,
};

#[derive(Deserialize)]
pub struct HistoryQuery {
    before: Option<Uuid>,
}

#[derive(Serialize)]
pub struct InvocationStatus {
    id: Uuid,
    status: String,
    attempt: i32,
    max_attempts: i32,
}

#[derive(Serialize)]
pub struct ChatMessage {
    id: Uuid,
    transcript_resource_node_id: Uuid,
    key_epoch: i32,
    encrypted_transcript: EncryptedPayload,
    encrypted_answer: Option<EncryptedPayload>,
    created_at: DateTime<Utc>,
    answered_at: Option<DateTime<Utc>>,
    invocation: Option<InvocationStatus>,
}

#[derive(Serialize)]
pub struct HistoryResponse {
    messages: Vec<ChatMessage>,
    next_cursor: Option<Uuid>,
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut tx = state.pool.begin().await?;
    set_database_context(
        &mut tx,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let rows = sqlx::query(
        r#"
        SELECT chat.id, chat.transcript_resource_node_id, chat.key_epoch,
               chat.encrypted_transcript, chat.created_at,
               answer.encrypted_answer, answer.answered_at,
               invocation.id AS invocation_id, invocation.status,
               invocation.attempt, invocation.max_attempts
        FROM agent_interrogations chat
        LEFT JOIN agent_interrogation_answers answer
          ON answer.project_id = chat.project_id AND answer.interrogation_id = chat.id
        LEFT JOIN LATERAL (
            SELECT id, status, attempt, max_attempts FROM agent_invocations
            WHERE project_id = chat.project_id AND agent_id = chat.target_agent_id
              AND interrogation_id = chat.id AND created_by_identity_id = $3
            ORDER BY created_at DESC, id DESC LIMIT 1
        ) invocation ON true
        WHERE chat.project_id = $1 AND chat.target_agent_id = $2
          AND chat.creator_identity_id = $3
          AND ($4::uuid IS NULL OR (chat.created_at, chat.id) < (
              SELECT created_at, id FROM agent_interrogations
              WHERE project_id = $1 AND target_agent_id = $2
                AND creator_identity_id = $3 AND id = $4
          ))
        ORDER BY chat.created_at DESC, chat.id DESC LIMIT 31
    "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(actor.identity_id)
    .bind(query.before)
    .fetch_all(&mut *tx)
    .await?;
    let has_more = rows.len() > 30;
    let messages = rows
        .into_iter()
        .take(30)
        .map(|row| -> Result<ChatMessage, AppError> {
            let invocation = row
                .try_get::<Option<Uuid>, _>("invocation_id")?
                .map(|id| -> Result<InvocationStatus, sqlx::Error> {
                    Ok(InvocationStatus {
                        id,
                        status: row.try_get("status")?,
                        attempt: row.try_get("attempt")?,
                        max_attempts: row.try_get("max_attempts")?,
                    })
                })
                .transpose()?;
            Ok(ChatMessage {
                id: row.try_get("id")?,
                transcript_resource_node_id: row.try_get("transcript_resource_node_id")?,
                key_epoch: row.try_get("key_epoch")?,
                encrypted_transcript: serde_json::from_slice(row.try_get("encrypted_transcript")?)
                    .map_err(|_| AppError::Internal)?,
                encrypted_answer: row
                    .try_get::<Option<Vec<u8>>, _>("encrypted_answer")?
                    .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| AppError::Internal))
                    .transpose()?,
                created_at: row.try_get("created_at")?,
                answered_at: row.try_get("answered_at")?,
                invocation,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if has_more {
        messages.last().map(|message| message.id)
    } else {
        None
    };
    tx.commit().await?;
    Ok(Json(HistoryResponse {
        messages,
        next_cursor,
    }))
}

pub async fn invocation_status(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, invocation_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<InvocationStatus>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut tx = state.pool.begin().await?;
    set_database_context(
        &mut tx,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let row = sqlx::query("SELECT id, status, attempt, max_attempts FROM agent_invocations WHERE project_id = $1 AND agent_id = $2 AND id = $3 AND created_by_identity_id = $4")
        .bind(project_id).bind(agent_id).bind(invocation_id).bind(actor.identity_id)
        .fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    let response = InvocationStatus {
        id: row.try_get("id")?,
        status: row.try_get("status")?,
        attempt: row.try_get("attempt")?,
        max_attempts: row.try_get("max_attempts")?,
    };
    tx.commit().await?;
    Ok(Json(response))
}
