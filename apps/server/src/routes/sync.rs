use std::sync::Arc;

use axum::{
    Json,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, header::AUTHORIZATION},
    response::Response,
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sprout_crypto_protocol::verify_ed25519_ml_dsa65_signatures;
use sprout_storage_postgres::{
    AppendSyncEvent, PostgresStorage, RequestContext, SyncEvent as StoredSyncEvent,
    SyncProjection as StoredSyncProjection,
};
use uuid::Uuid;

use crate::{
    AppState, SyncWake,
    auth::{
        AuthSession, ProjectAccess, ResourceAccess, authenticate_token, require_project_access,
        require_resource_access, set_database_context,
    },
    error::AppError,
};

const SYNC_SIGNATURE_CONTEXT: &[u8] = b"sprout-sync-event-v2";

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMutation {
    Upsert,
    Tombstone,
}

impl SyncMutation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Upsert => "upsert",
            Self::Tombstone => "tombstone",
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct PushRequest {
    project_id: Uuid,
    resource_node_id: Uuid,
    base_version: i64,
    aggregate_version: i64,
    actor_device_key_version: i32,
    device_sequence: i64,
    client_event_id: Uuid,
    event_kind: String,
    mutation: SyncMutation,
    key_epoch: i32,
    encrypted_payload_b64: String,
    previous_hash_b64: Option<String>,
    event_hash_b64: String,
    classical_signature_b64: String,
    post_quantum_signature_b64: String,
    client_created_at: DateTime<Utc>,
    idempotency_key: Uuid,
}

pub async fn push(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Json(request): Json<PushRequest>,
) -> Result<Json<PushResponse>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        request.project_id,
        request.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    if request.base_version < 0 || request.aggregate_version != request.base_version + 1 {
        return Err(AppError::Conflict);
    }
    let encrypted_payload = decode(&request.encrypted_payload_b64)?;
    if encrypted_payload.is_empty() {
        return Err(AppError::BadRequest("sync payload is empty"));
    }
    let previous_hash = request
        .previous_hash_b64
        .as_deref()
        .map(decode)
        .transpose()?;
    let event_hash = decode(&request.event_hash_b64)?;
    let classical_signature = decode(&request.classical_signature_b64)?;
    let post_quantum_signature = decode(&request.post_quantum_signature_b64)?;
    if event_hash.len() != 32 || classical_signature.len() != 64 {
        return Err(AppError::BadRequest(
            "invalid sync cryptographic field length",
        ));
    }
    if previous_hash.as_ref().is_some_and(|hash| hash.len() != 32) {
        return Err(AppError::BadRequest("invalid previous hash length"));
    }
    let expected_hash = calculate_event_hash(
        actor,
        &request,
        &encrypted_payload,
        previous_hash.as_deref(),
    );
    if event_hash.as_slice() != expected_hash {
        return Err(AppError::BadRequest(
            "sync event hash does not match payload",
        ));
    }
    let keys = load_signing_keys(
        &state,
        actor,
        request.project_id,
        request.actor_device_key_version,
    )
    .await?;
    verify_sync_signatures(
        &keys.0,
        &classical_signature,
        &keys.1,
        &post_quantum_signature,
        &event_hash,
    )
    .map_err(|_| AppError::BadRequest("sync dual signature verification failed"))?;

    let canonical_request = serde_json::to_vec(&request).map_err(|_| AppError::Internal)?;
    let request_hash = Sha256::digest(canonical_request).to_vec();
    let storage = PostgresStorage::new(state.pool.clone());
    let outcome = storage
        .append_sync_event(
            RequestContext::new(actor.identity_id, Some(actor.device_id)),
            &AppendSyncEvent {
                project_id: request.project_id,
                stream_id: request.resource_node_id,
                resource_node_id: request.resource_node_id,
                base_version: request.base_version,
                aggregate_version: request.aggregate_version,
                mutation_kind: request.mutation.as_str().into(),
                actor_identity_id: actor.identity_id,
                actor_device_id: actor.device_id,
                actor_device_key_version: request.actor_device_key_version,
                device_sequence: request.device_sequence,
                client_event_id: request.client_event_id,
                event_kind: request.event_kind,
                key_epoch: request.key_epoch,
                encrypted_payload,
                previous_hash,
                event_hash,
                signature: classical_signature,
                post_quantum_signature,
                client_created_at: request.client_created_at,
                idempotency_key: request.idempotency_key,
                request_hash,
                idempotency_expires_at: Utc::now() + Duration::hours(24),
            },
        )
        .await?;
    let _ = state.sync_wake_tx.send(SyncWake {
        project_id: request.project_id,
        cursor: outcome.event.event_sequence,
    });
    Ok(Json(PushResponse {
        event: SyncEventView::from(outcome.event),
        projection: SyncProjectionView::from(outcome.projection),
        replayed: outcome.replayed,
    }))
}

fn verify_sync_signatures(
    ed25519_public_key: &[u8],
    classical_signature: &[u8],
    ml_dsa_65_public_key: &[u8],
    post_quantum_signature: &[u8],
    event_hash: &[u8],
) -> Result<(), sprout_crypto_protocol::ProtocolError> {
    verify_ed25519_ml_dsa65_signatures(
        ed25519_public_key,
        classical_signature,
        ml_dsa_65_public_key,
        post_quantum_signature,
        event_hash,
        SYNC_SIGNATURE_CONTEXT,
    )
}

async fn load_signing_keys(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    key_version: i32,
) -> Result<(Vec<u8>, Vec<u8>), AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let keys = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        r#"
        SELECT ed25519_public_key, ml_dsa_65_public_key
        FROM device_keys
        WHERE identity_id = $1
          AND device_id = $2
          AND key_version = $3
          AND suite_version = 32769
          AND revoked_at IS NULL
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(key_version)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    transaction.commit().await?;
    Ok(keys)
}

fn calculate_event_hash(
    actor: AuthSession,
    request: &PushRequest,
    encrypted_payload: &[u8],
    previous_hash: Option<&[u8]>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"sprout-sync-event-v2");
    digest.update(request.project_id.as_bytes());
    digest.update(request.resource_node_id.as_bytes());
    digest.update(actor.identity_id.as_bytes());
    digest.update(actor.device_id.as_bytes());
    digest.update(request.actor_device_key_version.to_be_bytes());
    digest.update(request.device_sequence.to_be_bytes());
    digest.update(request.base_version.to_be_bytes());
    digest.update(request.aggregate_version.to_be_bytes());
    digest.update(request.client_event_id.as_bytes());
    digest.update((request.event_kind.len() as u64).to_be_bytes());
    digest.update(request.event_kind.as_bytes());
    digest.update(request.mutation.as_str().as_bytes());
    digest.update(request.key_epoch.to_be_bytes());
    digest.update(Sha256::digest(encrypted_payload));
    if let Some(previous_hash) = previous_hash {
        digest.update(previous_hash);
    }
    digest.finalize().into()
}

#[derive(Serialize)]
pub struct PushResponse {
    event: SyncEventView,
    projection: SyncProjectionView,
    replayed: bool,
}

#[derive(Serialize)]
pub struct SyncProjectionView {
    project_id: Uuid,
    resource_node_id: Uuid,
    aggregate_version: i64,
    mutation: String,
    key_epoch: i32,
    encrypted_payload_b64: String,
    event_id: Uuid,
    event_hash_b64: String,
    updated_at: DateTime<Utc>,
}

impl From<StoredSyncProjection> for SyncProjectionView {
    fn from(projection: StoredSyncProjection) -> Self {
        Self {
            project_id: projection.project_id,
            resource_node_id: projection.resource_node_id,
            aggregate_version: projection.aggregate_version,
            mutation: projection.mutation_kind,
            key_epoch: projection.key_epoch,
            encrypted_payload_b64: encode(&projection.encrypted_payload),
            event_id: projection.event_id,
            event_hash_b64: encode(&projection.event_hash),
            updated_at: projection.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct PullRequest {
    project_id: Uuid,
    #[serde(default)]
    after_sequence: i64,
    #[serde(default = "default_pull_limit")]
    limit: u16,
}

fn default_pull_limit() -> u16 {
    100
}

pub async fn pull(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Json(request): Json<PullRequest>,
) -> Result<Json<PullResponse>, AppError> {
    if request.after_sequence < 0 || request.limit == 0 || request.limit > 500 {
        return Err(AppError::BadRequest("invalid sync cursor or limit"));
    }
    require_project_access(
        &state.pool,
        actor,
        request.project_id,
        ProjectAccess::Member,
    )
    .await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(request.project_id),
    )
    .await?;
    let rows = sqlx::query_as::<_, SyncEventRow>(
        r#"
        SELECT event.id, event.event_sequence, event.project_id,
               event.resource_node_id, event.base_version, event.aggregate_version,
               event.mutation_kind, event.actor_identity_id,
               event.actor_device_id, event.actor_device_key_version,
               event.device_sequence, event.client_event_id, event.event_kind,
               event.key_epoch, event.encrypted_payload, event.previous_hash,
               event.event_hash, event.signature, event.post_quantum_signature,
               event.client_created_at, event.received_at
        FROM sync_events event
        JOIN resource_nodes node
          ON node.project_id = event.project_id
         AND node.id = event.resource_node_id
         AND node.deleted_at IS NULL
        JOIN projects project ON project.id = event.project_id
        JOIN project_memberships membership
          ON membership.project_id = event.project_id
         AND membership.identity_id = $4
         AND membership.state = 'active'
        WHERE event.project_id = $1
          AND event.event_sequence > $2
          AND (
              project.owner_identity_id = $4
              OR membership.role = 'admin'
              OR node.created_by_identity_id = $4
              OR EXISTS (
                  SELECT 1
                  FROM sprout_private.effective_domain_permission(
                      event.project_id, event.resource_node_id, $4
                  )
              )
          )
        ORDER BY event.event_sequence
        LIMIT $3
        "#,
    )
    .bind(request.project_id)
    .bind(request.after_sequence)
    .bind(i64::from(request.limit) + 1)
    .bind(actor.identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let has_more = rows.len() > usize::from(request.limit);
    let events = rows
        .into_iter()
        .take(usize::from(request.limit))
        .map(SyncEventView::from)
        .collect::<Vec<_>>();
    let next_sequence = events
        .last()
        .map_or(request.after_sequence, |event| event.event_sequence);
    Ok(Json(PullResponse {
        project_id: request.project_id,
        from_sequence: request.after_sequence,
        next_sequence,
        has_more,
        events,
    }))
}

#[derive(sqlx::FromRow)]
struct SyncEventRow {
    id: Uuid,
    event_sequence: i64,
    project_id: Uuid,
    resource_node_id: Uuid,
    base_version: i64,
    aggregate_version: i64,
    mutation_kind: String,
    actor_identity_id: Uuid,
    actor_device_id: Uuid,
    actor_device_key_version: i32,
    device_sequence: i64,
    client_event_id: Uuid,
    event_kind: String,
    key_epoch: i32,
    encrypted_payload: Vec<u8>,
    previous_hash: Option<Vec<u8>>,
    event_hash: Vec<u8>,
    signature: Vec<u8>,
    post_quantum_signature: Option<Vec<u8>>,
    client_created_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct SyncEventView {
    id: Uuid,
    event_sequence: i64,
    project_id: Uuid,
    resource_node_id: Uuid,
    base_version: i64,
    aggregate_version: i64,
    mutation: String,
    actor_identity_id: Uuid,
    actor_device_id: Uuid,
    actor_device_key_version: i32,
    device_sequence: i64,
    client_event_id: Uuid,
    event_kind: String,
    key_epoch: i32,
    encrypted_payload_b64: String,
    previous_hash_b64: Option<String>,
    event_hash_b64: String,
    classical_signature_b64: String,
    post_quantum_signature_b64: Option<String>,
    client_created_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
}

impl From<StoredSyncEvent> for SyncEventView {
    fn from(event: StoredSyncEvent) -> Self {
        Self {
            id: event.id,
            event_sequence: event.event_sequence,
            project_id: event.project_id,
            resource_node_id: event.resource_node_id,
            base_version: event.base_version,
            aggregate_version: event.aggregate_version,
            mutation: event.mutation_kind,
            actor_identity_id: event.actor_identity_id,
            actor_device_id: event.actor_device_id,
            actor_device_key_version: event.actor_device_key_version,
            device_sequence: event.device_sequence,
            client_event_id: event.client_event_id,
            event_kind: event.event_kind,
            key_epoch: event.key_epoch,
            encrypted_payload_b64: encode(&event.encrypted_payload),
            previous_hash_b64: event.previous_hash.as_deref().map(encode),
            event_hash_b64: encode(&event.event_hash),
            classical_signature_b64: encode(&event.signature),
            post_quantum_signature_b64: event.post_quantum_signature.as_deref().map(encode),
            client_created_at: event.client_created_at,
            received_at: event.received_at,
        }
    }
}

impl From<SyncEventRow> for SyncEventView {
    fn from(event: SyncEventRow) -> Self {
        Self {
            id: event.id,
            event_sequence: event.event_sequence,
            project_id: event.project_id,
            resource_node_id: event.resource_node_id,
            base_version: event.base_version,
            aggregate_version: event.aggregate_version,
            mutation: event.mutation_kind,
            actor_identity_id: event.actor_identity_id,
            actor_device_id: event.actor_device_id,
            actor_device_key_version: event.actor_device_key_version,
            device_sequence: event.device_sequence,
            client_event_id: event.client_event_id,
            event_kind: event.event_kind,
            key_epoch: event.key_epoch,
            encrypted_payload_b64: encode(&event.encrypted_payload),
            previous_hash_b64: event.previous_hash.as_deref().map(encode),
            event_hash_b64: encode(&event.event_hash),
            classical_signature_b64: encode(&event.signature),
            post_quantum_signature_b64: event.post_quantum_signature.as_deref().map(encode),
            client_created_at: event.client_created_at,
            received_at: event.received_at,
        }
    }
}

#[derive(Serialize)]
pub struct PullResponse {
    project_id: Uuid,
    from_sequence: i64,
    next_sequence: i64,
    has_more: bool,
    events: Vec<SyncEventView>,
}

#[derive(Deserialize)]
pub struct WakeQuery {
    project_id: Uuid,
}

pub async fn websocket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WakeQuery>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let token = websocket_token(&headers).ok_or(AppError::Unauthorized)?;
    let actor = authenticate_token(&state.pool, token).await?;
    require_project_access(&state.pool, actor, query.project_id, ProjectAccess::Member).await?;
    let receiver = state.sync_wake_tx.subscribe();
    Ok(upgrade.on_upgrade(move |socket| wake_socket(socket, query.project_id, receiver)))
}

fn websocket_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .find_map(|protocol| protocol.strip_prefix("sprout-auth."))
                })
        })
}

async fn wake_socket(
    mut socket: WebSocket,
    project_id: Uuid,
    mut receiver: tokio::sync::broadcast::Receiver<SyncWake>,
) {
    loop {
        match receiver.recv().await {
            Ok(wake) if wake.project_id == project_id => {
                let Ok(payload) = serde_json::to_string(&wake) else {
                    break;
                };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 payload"))
}

fn encode(value: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprout_crypto_protocol::{Ed25519Adapter, LibcruxMlDsa65Experimental, SignatureAdapter};

    #[test]
    fn sync_requires_both_untampered_signatures() {
        let classical = Ed25519Adapter.generate_key_pair().unwrap();
        let post_quantum = LibcruxMlDsa65Experimental.generate_key_pair().unwrap();
        let event_hash = [7u8; 32];
        let classical_signature = Ed25519Adapter
            .sign(classical.secret_key(), &event_hash, SYNC_SIGNATURE_CONTEXT)
            .unwrap();
        let mut post_quantum_signature = LibcruxMlDsa65Experimental
            .sign(
                post_quantum.secret_key(),
                &event_hash,
                SYNC_SIGNATURE_CONTEXT,
            )
            .unwrap();
        assert!(
            verify_sync_signatures(
                classical.public_key(),
                &classical_signature,
                post_quantum.public_key(),
                &post_quantum_signature,
                &event_hash,
            )
            .is_ok()
        );
        post_quantum_signature[0] ^= 1;
        assert!(
            verify_sync_signatures(
                classical.public_key(),
                &classical_signature,
                post_quantum.public_key(),
                &post_quantum_signature,
                &event_hash,
            )
            .is_err()
        );
    }
}
