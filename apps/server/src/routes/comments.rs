use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sprout_api_contract::EncryptedPayloadDto;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthSession, set_database_context},
    error::AppError,
};

use super::agent_runs::runtime_tick;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostCommentRequest {
    recipient_id: Uuid,
    target_id: Uuid,
    parent_id: Option<Uuid>,
    encrypted_payload: EncryptedPayloadDto,
    key_epoch: u32,
    idempotency_key: Uuid,
    /// Optional for a human/admin comment.  When supplied it must name an
    /// exact 0035-native run; it never turns this path into AgentAction.
    run_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostAgentCommentRequest {
    recipient_id: Uuid,
    target_id: Uuid,
    parent_id: Option<Uuid>,
    encrypted_payload: EncryptedPayloadDto,
    key_epoch: u32,
    idempotency_key: Uuid,
    work_item_id: Uuid,
    attempt: u16,
}

#[derive(Serialize)]
pub struct PostCommentResponse {
    id: Uuid,
    replayed: bool,
}

#[derive(Serialize)]
pub struct CommentResponse {
    id: Uuid,
    author_id: Uuid,
    author_kind: String,
    recipient_id: Uuid,
    target_id: Uuid,
    parent_id: Option<Uuid>,
    agent_depth: u32,
    encrypted_payload: EncryptedPayloadDto,
    key_epoch: u32,
    semantic_tick: u64,
    run_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct ListCommentsResponse {
    comments: Vec<CommentResponse>,
}

pub async fn post_human(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<PostCommentRequest>,
) -> Result<Json<PostCommentResponse>, AppError> {
    if actor.is_agent || request.run_id.is_some_and(|run_id| run_id.is_nil()) {
        return Err(AppError::Forbidden);
    }
    post(
        &state,
        actor,
        project_id,
        request.recipient_id,
        request.target_id,
        request.parent_id,
        &request.encrypted_payload,
        request.key_epoch,
        request.idempotency_key,
        request.run_id,
        None,
        None,
        None,
    )
    .await
}

pub async fn post_agent(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, claim_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<PostAgentCommentRequest>,
) -> Result<Json<PostCommentResponse>, AppError> {
    if !actor.is_agent || request.attempt == 0 {
        return Err(AppError::Forbidden);
    }
    post(
        &state,
        actor,
        project_id,
        request.recipient_id,
        request.target_id,
        request.parent_id,
        &request.encrypted_payload,
        request.key_epoch,
        request.idempotency_key,
        Some(run_id),
        Some(request.work_item_id),
        Some(claim_id),
        Some(i32::from(request.attempt)),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn post(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    recipient_id: Uuid,
    target_id: Uuid,
    parent_id: Option<Uuid>,
    encrypted_payload: &EncryptedPayloadDto,
    key_epoch: u32,
    idempotency_key: Uuid,
    run_id: Option<Uuid>,
    work_item_id: Option<Uuid>,
    claim_id: Option<Uuid>,
    attempt: Option<i32>,
) -> Result<Json<PostCommentResponse>, AppError> {
    let opaque = opaque_payload(encrypted_payload)?;
    let unbound_operational_tick = run_id
        .is_none()
        .then(runtime_tick)
        .transpose()?
        .map(|tick| i64::try_from(tick).map_err(|_| AppError::Internal))
        .transpose()?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let row = sqlx::query(
        r#"SELECT comment_id,replayed FROM sprout_private.post_native_comment(
          $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"#,
    )
    .bind(project_id)
    .bind(recipient_id)
    .bind(target_id)
    .bind(parent_id)
    .bind(opaque)
    .bind(i32::try_from(key_epoch).map_err(|_| AppError::BadRequest("invalid key epoch"))?)
    .bind(idempotency_key)
    .bind(run_id)
    .bind(work_item_id)
    .bind(claim_id)
    .bind(attempt)
    // A run-bound Comment receives its formal tick inside the trusted writer
    // from the canonical per-run allocator.  Wall clock is retained only for
    // an operational Comment that is not part of a certified run trace.
    .bind(unbound_operational_tick)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(PostCommentResponse {
        id: row.try_get("comment_id")?,
        replayed: row.try_get("replayed")?,
    }))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, target_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListCommentsResponse>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let rows = sqlx::query(
        r#"SELECT id,author_identity_id,author_kind,recipient_identity_id,
          target_resource_node_id,parent_comment_id,agent_depth,encrypted_payload,
          key_epoch,semantic_tick,run_id
        FROM native_comment_readable
        WHERE project_id=$1 AND target_resource_node_id=$2
        ORDER BY semantic_tick,id"#,
    )
    .bind(project_id)
    .bind(target_id)
    .fetch_all(&mut *transaction)
    .await?;
    let comments = rows
        .into_iter()
        .map(|row| {
            Ok(CommentResponse {
                id: row.try_get("id")?,
                author_id: row.try_get("author_identity_id")?,
                author_kind: row.try_get("author_kind")?,
                recipient_id: row.try_get("recipient_identity_id")?,
                target_id: row.try_get("target_resource_node_id")?,
                parent_id: row.try_get("parent_comment_id")?,
                agent_depth: u32::try_from(row.try_get::<i32, _>("agent_depth")?)
                    .map_err(|_| AppError::Internal)?,
                encrypted_payload: payload_from_bytes(
                    &row.try_get::<Vec<u8>, _>("encrypted_payload")?,
                )?,
                key_epoch: u32::try_from(row.try_get::<i32, _>("key_epoch")?)
                    .map_err(|_| AppError::Internal)?,
                semantic_tick: u64::try_from(row.try_get::<i64, _>("semantic_tick")?)
                    .map_err(|_| AppError::Internal)?,
                run_id: row.try_get("run_id")?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    transaction.commit().await?;
    Ok(Json(ListCommentsResponse { comments }))
}

fn opaque_payload(payload: &EncryptedPayloadDto) -> Result<Vec<u8>, AppError> {
    if payload.version == 0
        || payload.algorithm.trim().is_empty()
        || payload.key_id.trim().is_empty()
        || base64::engine::general_purpose::STANDARD
            .decode(&payload.nonce_b64)
            .map_err(|_| AppError::BadRequest("invalid base64 ciphertext"))?
            .is_empty()
        || base64::engine::general_purpose::STANDARD
            .decode(&payload.ciphertext_b64)
            .map_err(|_| AppError::BadRequest("invalid base64 ciphertext"))?
            .is_empty()
    {
        return Err(AppError::BadRequest("encrypted payload is incomplete"));
    }
    serde_json::to_vec(payload).map_err(|_| AppError::BadRequest("invalid encrypted payload"))
}

fn payload_from_bytes(bytes: &[u8]) -> Result<EncryptedPayloadDto, AppError> {
    serde_json::from_slice(bytes).map_err(|_| AppError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_wire_schema_rejects_server_owned_fields() {
        let value = serde_json::json!({
            "recipient_id": Uuid::new_v4(), "target_id": Uuid::new_v4(),
            "parent_id": null, "encrypted_payload": {
              "version": 1, "algorithm": "x", "key_id": "opaque",
              "nonce_b64": "AQ==", "ciphertext_b64": "Ag=="
            }, "key_epoch": 1, "idempotency_key": Uuid::new_v4(),
            "run_id": null, "author_id": Uuid::new_v4()
        });
        assert!(serde_json::from_value::<PostCommentRequest>(value).is_err());
    }
}
