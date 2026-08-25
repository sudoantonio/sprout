use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sprout_api_contract::{
    CreateInfoDocumentRequest, EncryptedPayloadDto, InfoDocumentDto, InfoDocumentResponse,
    ListInfoDocumentsResponse, UpdateInfoDocumentRequest,
};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthSession, ResourceAccess, require_resource_access, set_database_context},
    error::AppError,
};

#[derive(Clone, Copy)]
enum Container {
    Topic(Uuid),
    TaskList(Uuid),
}

impl Container {
    const fn topic_id(self) -> Option<Uuid> {
        match self {
            Self::Topic(id) => Some(id),
            Self::TaskList(_) => None,
        }
    }

    const fn task_list_id(self) -> Option<Uuid> {
        match self {
            Self::Topic(_) => None,
            Self::TaskList(id) => Some(id),
        }
    }
}

pub async fn list_topic_documents(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, topic_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListInfoDocumentsResponse>, AppError> {
    list_documents(&state, actor, project_id, Container::Topic(topic_id)).await
}

pub async fn create_topic_document(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, topic_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateInfoDocumentRequest>,
) -> Result<Json<InfoDocumentResponse>, AppError> {
    create_document(
        &state,
        actor,
        project_id,
        Container::Topic(topic_id),
        request,
    )
    .await
}

pub async fn list_task_list_documents(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_list_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListInfoDocumentsResponse>, AppError> {
    list_documents(&state, actor, project_id, Container::TaskList(task_list_id)).await
}

pub async fn create_task_list_document(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_list_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateInfoDocumentRequest>,
) -> Result<Json<InfoDocumentResponse>, AppError> {
    create_document(
        &state,
        actor,
        project_id,
        Container::TaskList(task_list_id),
        request,
    )
    .await
}

pub async fn get_document(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<InfoDocumentResponse>, AppError> {
    let row = document_row(&state, actor, project_id, document_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        row.resource_node_id,
        ResourceAccess::Read,
    )
    .await?;
    Ok(Json(InfoDocumentResponse {
        document: row.try_into()?,
    }))
}

pub async fn update_document(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, document_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateInfoDocumentRequest>,
) -> Result<Json<InfoDocumentResponse>, AppError> {
    let current = document_row(&state, actor, project_id, document_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::EditInfo,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, InfoDocumentRow>(
        r#"
        UPDATE info_documents
        SET encrypted_payload = $3,
            key_epoch = $5,
            payload_version = payload_version + 1
        WHERE project_id = $1
          AND id = $2
          AND payload_version = $4
          AND deleted_at IS NULL
          AND EXISTS (
              SELECT 1
              FROM resource_epochs epoch
              WHERE epoch.project_id = info_documents.project_id
                AND epoch.resource_node_id = info_documents.resource_node_id
                AND epoch.epoch = $5
                AND epoch.retired_at IS NULL
          )
        RETURNING id, project_id, topic_id, task_list_id,
                  parent_document_id, resource_node_id, encrypted_payload,
                  key_epoch, payload_version, created_at, updated_at
        "#,
    )
    .bind(project_id)
    .bind(document_id)
    .bind(&payload)
    .bind(to_i64(request.expected_payload_version)?)
    .bind(to_i32(request.key_epoch)?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    insert_outbox(
        &mut transaction,
        project_id,
        document_id,
        "updated",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(InfoDocumentResponse {
        document: row.try_into()?,
    }))
}

pub async fn delete_document(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let current = document_row(&state, actor, project_id, document_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::EditInfo,
    )
    .await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let affected = sqlx::query(
        r#"
        WITH RECURSIVE descendants AS (
            SELECT id
            FROM info_documents
            WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
            UNION ALL
            SELECT child.id
            FROM info_documents child
            JOIN descendants parent ON parent.id = child.parent_document_id
            WHERE child.project_id = $1 AND child.deleted_at IS NULL
        )
        UPDATE info_documents
        SET deleted_at = clock_timestamp()
        WHERE project_id = $1 AND id IN (SELECT id FROM descendants)
        "#,
    )
    .bind(project_id)
    .bind(document_id)
    .execute(&mut *transaction)
    .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_documents(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    container: Container,
) -> Result<Json<ListInfoDocumentsResponse>, AppError> {
    let resource_node_id = container_resource(state, actor, project_id, container).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_node_id,
        ResourceAccess::Read,
    )
    .await?;
    let mut transaction = begin(state, actor, project_id).await?;
    let rows = sqlx::query_as::<_, InfoDocumentRow>(
        r#"
        SELECT id, project_id, topic_id, task_list_id, parent_document_id,
               resource_node_id, encrypted_payload, key_epoch,
               payload_version, created_at, updated_at
        FROM info_documents
        WHERE project_id = $1
          AND topic_id IS NOT DISTINCT FROM $2
          AND task_list_id IS NOT DISTINCT FROM $3
          AND deleted_at IS NULL
        ORDER BY created_at, id
        "#,
    )
    .bind(project_id)
    .bind(container.topic_id())
    .bind(container.task_list_id())
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ListInfoDocumentsResponse {
        documents: rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    }))
}

async fn create_document(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    container: Container,
    request: CreateInfoDocumentRequest,
) -> Result<Json<InfoDocumentResponse>, AppError> {
    let resource_node_id = container_resource(state, actor, project_id, container).await?;
    if request.resource_node_id != resource_node_id {
        return Err(AppError::BadRequest(
            "info document must use the container resource",
        ));
    }
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_node_id,
        ResourceAccess::EditInfo,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query_as::<_, InfoDocumentRow>(
        r#"
        INSERT INTO info_documents (
            id, project_id, topic_id, task_list_id, parent_document_id,
            resource_node_id, encrypted_payload, key_epoch,
            created_by_identity_id
        )
        SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9
        WHERE EXISTS (
            SELECT 1
            FROM resource_epochs epoch
            WHERE epoch.project_id = $2
              AND epoch.resource_node_id = $6
              AND epoch.epoch = $8
              AND epoch.retired_at IS NULL
        )
        RETURNING id, project_id, topic_id, task_list_id,
                  parent_document_id, resource_node_id, encrypted_payload,
                  key_epoch, payload_version, created_at, updated_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(container.topic_id())
    .bind(container.task_list_id())
    .bind(request.parent_document_id)
    .bind(resource_node_id)
    .bind(&payload)
    .bind(to_i32(request.key_epoch)?)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::BadRequest("resource key epoch is not active"))?;
    insert_outbox(
        &mut transaction,
        project_id,
        request.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(InfoDocumentResponse {
        document: row.try_into()?,
    }))
}

async fn container_resource(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    container: Container,
) -> Result<Uuid, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let resource_node_id = match container {
        Container::Topic(topic_id) => {
            sqlx::query_scalar(
                "SELECT resource_node_id FROM topics
                 WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
            )
            .bind(project_id)
            .bind(topic_id)
            .fetch_optional(&mut *transaction)
            .await?
        }
        Container::TaskList(task_list_id) => {
            sqlx::query_scalar(
                "SELECT resource_node_id FROM task_lists
                 WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
            )
            .bind(project_id)
            .bind(task_list_id)
            .fetch_optional(&mut *transaction)
            .await?
        }
    }
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(resource_node_id)
}

async fn document_row(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<InfoDocumentRow, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query_as::<_, InfoDocumentRow>(
        r#"
        SELECT id, project_id, topic_id, task_list_id, parent_document_id,
               resource_node_id, encrypted_payload, key_epoch,
               payload_version, created_at, updated_at
        FROM info_documents
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(document_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(row)
}

async fn begin<'a>(
    state: &'a AppState,
    actor: AuthSession,
    project_id: Uuid,
) -> Result<Transaction<'a, Postgres>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    Ok(transaction)
}

async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    document_id: Uuid,
    event_kind: &str,
    idempotency_key: Uuid,
    encrypted_payload: &[u8],
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO outbox (
            project_id, aggregate_kind, aggregate_id, event_kind,
            deduplication_key, encrypted_payload
        )
        VALUES ($1, 'info_document', $2, $3, $4, $5)
        ON CONFLICT (project_id, deduplication_key) DO NOTHING
        "#,
    )
    .bind(project_id)
    .bind(document_id)
    .bind(event_kind)
    .bind(idempotency_key)
    .bind(encrypted_payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn opaque_payload(payload: &EncryptedPayloadDto) -> Result<Vec<u8>, AppError> {
    if payload.version == 0
        || payload.algorithm.trim().is_empty()
        || payload.key_id.trim().is_empty()
        || decode(&payload.nonce_b64)?.is_empty()
        || decode(&payload.ciphertext_b64)?.is_empty()
    {
        return Err(AppError::BadRequest("encrypted payload is incomplete"));
    }
    serde_json::to_vec(payload).map_err(|_| AppError::BadRequest("invalid encrypted payload"))
}

fn payload_from_bytes(bytes: &[u8]) -> Result<EncryptedPayloadDto, AppError> {
    serde_json::from_slice(bytes).map_err(|_| AppError::Internal)
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 ciphertext"))
}

fn to_i32(value: u32) -> Result<i32, AppError> {
    i32::try_from(value).map_err(|_| AppError::BadRequest("invalid key epoch"))
}

fn to_i64(value: u64) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| AppError::BadRequest("numeric value is too large"))
}

#[derive(FromRow)]
struct InfoDocumentRow {
    id: Uuid,
    project_id: Uuid,
    topic_id: Option<Uuid>,
    task_list_id: Option<Uuid>,
    parent_document_id: Option<Uuid>,
    resource_node_id: Uuid,
    encrypted_payload: Vec<u8>,
    key_epoch: i32,
    payload_version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<InfoDocumentRow> for InfoDocumentDto {
    type Error = AppError;

    fn try_from(row: InfoDocumentRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            topic_id: row.topic_id,
            task_list_id: row.task_list_id,
            parent_document_id: row.parent_document_id,
            resource_node_id: row.resource_node_id,
            payload: payload_from_bytes(&row.encrypted_payload)?,
            key_epoch: u32::try_from(row.key_epoch).map_err(|_| AppError::Internal)?,
            payload_version: u64::try_from(row.payload_version).map_err(|_| AppError::Internal)?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}
