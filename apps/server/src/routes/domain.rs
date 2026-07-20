use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        AuthSession, ProjectAccess, ResourceAccess, require_project_access,
        require_resource_access, set_database_context,
    },
    error::AppError,
};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Topic,
    TaskList,
    Task,
    File,
    Other,
}

impl NodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Topic => "topic",
            Self::TaskList => "task_list",
            Self::Task => "task",
            Self::File => "file",
            Self::Other => "other",
        }
    }
}

#[derive(Deserialize)]
pub struct CreateResource {
    id: Uuid,
    parent_id: Uuid,
    kind: NodeKind,
    encrypted_metadata_b64: String,
}

pub async fn create_resource(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateResource>,
) -> Result<Json<ResourceView>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        request.parent_id,
        ResourceAccess::Write,
    )
    .await?;
    let encrypted_metadata = decode(&request.encrypted_metadata_b64)?;
    if encrypted_metadata.is_empty() {
        return Err(AppError::BadRequest("encrypted resource metadata is empty"));
    }
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let row = sqlx::query_as::<_, ResourceRow>(
        r#"
        INSERT INTO resource_nodes (
            id, project_id, parent_id, node_kind,
            encrypted_metadata, created_by_identity_id
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, project_id, parent_id, node_kind, encrypted_metadata,
                  created_by_identity_id, created_at, updated_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request.parent_id)
    .bind(request.kind.as_str())
    .bind(encrypted_metadata)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ResourceView::from(row)))
}

pub async fn get_resource(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, resource_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ResourceView>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
        ResourceAccess::ViewHeader,
    )
    .await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let row = sqlx::query_as::<_, ResourceRow>(
        r#"
        SELECT id, project_id, parent_id, node_kind, encrypted_metadata,
               created_by_identity_id, created_at, updated_at
        FROM resource_nodes
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(Json(ResourceView::from(row)))
}

#[derive(FromRow)]
struct ResourceRow {
    id: Uuid,
    project_id: Uuid,
    parent_id: Option<Uuid>,
    node_kind: String,
    encrypted_metadata: Vec<u8>,
    created_by_identity_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct ResourceView {
    id: Uuid,
    project_id: Uuid,
    parent_id: Option<Uuid>,
    kind: String,
    encrypted_metadata_b64: String,
    created_by_identity_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ResourceRow> for ResourceView {
    fn from(row: ResourceRow) -> Self {
        Self {
            id: row.id,
            project_id: row.project_id,
            parent_id: row.parent_id,
            kind: row.node_kind,
            encrypted_metadata_b64: encode(row.encrypted_metadata),
            created_by_identity_id: row.created_by_identity_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct CreateTask {
    id: Uuid,
    task_list_id: Uuid,
    resource_node_id: Uuid,
    encrypted_payload_b64: String,
}

#[allow(dead_code)]
pub async fn create_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateTask>,
) -> Result<Json<TaskView>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut lookup = state.pool.begin().await?;
    set_database_context(
        &mut lookup,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let parent_node_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM task_lists WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(request.task_list_id)
    .fetch_optional(&mut *lookup)
    .await?
    .ok_or(AppError::NotFound)?;
    lookup.commit().await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        parent_node_id,
        ResourceAccess::Write,
    )
    .await?;
    let encrypted_payload = decode(&request.encrypted_payload_b64)?;
    if encrypted_payload.is_empty() {
        return Err(AppError::BadRequest("encrypted task payload is empty"));
    }
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    sqlx::query(
        r#"
        INSERT INTO resource_nodes (
            id, project_id, parent_id, node_kind,
            encrypted_metadata, created_by_identity_id
        ) VALUES ($1, $2, $3, 'task', $4, $5)
        "#,
    )
    .bind(request.resource_node_id)
    .bind(project_id)
    .bind(parent_node_id)
    .bind(&encrypted_payload)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    let row = sqlx::query_as::<_, TaskRow>(
        r#"
        INSERT INTO tasks (
            id, project_id, task_list_id, resource_node_id, encrypted_payload
        ) VALUES ($1, $2, $3, $4, $5)
        RETURNING id, project_id, task_list_id, resource_node_id,
                  encrypted_payload, payload_version, state, created_at, updated_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request.task_list_id)
    .bind(request.resource_node_id)
    .bind(encrypted_payload)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskView::from(row)))
}

#[allow(dead_code)]
pub async fn get_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskView>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let resource_node_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM tasks WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_node_id,
        ResourceAccess::Read,
    )
    .await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let row = sqlx::query_as::<_, TaskRow>(
        r#"
        SELECT id, project_id, task_list_id, resource_node_id,
               encrypted_payload, payload_version, state, created_at, updated_at
        FROM tasks
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskView::from(row)))
}

#[derive(FromRow)]
struct TaskRow {
    id: Uuid,
    project_id: Uuid,
    task_list_id: Uuid,
    resource_node_id: Uuid,
    encrypted_payload: Vec<u8>,
    payload_version: i64,
    state: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct TaskView {
    id: Uuid,
    project_id: Uuid,
    task_list_id: Uuid,
    resource_node_id: Uuid,
    encrypted_payload_b64: String,
    payload_version: i64,
    state: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TaskRow> for TaskView {
    fn from(row: TaskRow) -> Self {
        Self {
            id: row.id,
            project_id: row.project_id,
            task_list_id: row.task_list_id,
            resource_node_id: row.resource_node_id,
            encrypted_payload_b64: encode(row.encrypted_payload),
            payload_version: row.payload_version,
            state: row.state,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateMetadataEntity {
    id: Uuid,
    encrypted_metadata_b64: String,
}

#[allow(dead_code)]
pub async fn create_preset(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateMetadataEntity>,
) -> Result<Json<MetadataEntityView>, AppError> {
    create_metadata_entity(&state, actor, project_id, request, MetadataKind::Preset).await
}

#[allow(dead_code)]
pub async fn get_preset(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, preset_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<MetadataEntityView>, AppError> {
    get_metadata_entity(&state, actor, project_id, preset_id, MetadataKind::Preset).await
}

#[allow(dead_code)]
pub async fn create_questionnaire(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateMetadataEntity>,
) -> Result<Json<MetadataEntityView>, AppError> {
    create_metadata_entity(
        &state,
        actor,
        project_id,
        request,
        MetadataKind::Questionnaire,
    )
    .await
}

#[allow(dead_code)]
pub async fn get_questionnaire(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, questionnaire_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<MetadataEntityView>, AppError> {
    get_metadata_entity(
        &state,
        actor,
        project_id,
        questionnaire_id,
        MetadataKind::Questionnaire,
    )
    .await
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum MetadataKind {
    Preset,
    Questionnaire,
}

async fn create_metadata_entity(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    request: CreateMetadataEntity,
    kind: MetadataKind,
) -> Result<Json<MetadataEntityView>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let metadata = decode(&request.encrypted_metadata_b64)?;
    if metadata.is_empty() {
        return Err(AppError::BadRequest("encrypted metadata is empty"));
    }
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let (query, initial_state) = match kind {
        MetadataKind::Preset => (
            r#"
            INSERT INTO presets (id, project_id, encrypted_metadata, created_by_identity_id)
            VALUES ($1, $2, $3, $4)
            RETURNING id, project_id, encrypted_metadata, state, created_by_identity_id, created_at, updated_at
            "#,
            "active",
        ),
        MetadataKind::Questionnaire => (
            r#"
            INSERT INTO questionnaires (id, project_id, encrypted_metadata, created_by_identity_id)
            VALUES ($1, $2, $3, $4)
            RETURNING id, project_id, encrypted_metadata, state, created_by_identity_id, created_at, updated_at
            "#,
            "draft",
        ),
    };
    let row = sqlx::query_as::<_, MetadataEntityRow>(query)
        .bind(request.id)
        .bind(project_id)
        .bind(metadata)
        .bind(actor.identity_id)
        .fetch_one(&mut *transaction)
        .await?;
    debug_assert_eq!(row.state, initial_state);
    transaction.commit().await?;
    Ok(Json(MetadataEntityView::from(row)))
}

async fn get_metadata_entity(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    id: Uuid,
    kind: MetadataKind,
) -> Result<Json<MetadataEntityView>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let query = match kind {
        MetadataKind::Preset => {
            r#"
            SELECT id, project_id, encrypted_metadata, state,
                   created_by_identity_id, created_at, updated_at
            FROM presets
            WHERE project_id = $1 AND id = $2 AND state <> 'deleted'
            "#
        }
        MetadataKind::Questionnaire => {
            r#"
            SELECT id, project_id, encrypted_metadata, state,
                   created_by_identity_id, created_at, updated_at
            FROM questionnaires
            WHERE project_id = $1 AND id = $2
            "#
        }
    };
    let row = sqlx::query_as::<_, MetadataEntityRow>(query)
        .bind(project_id)
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(Json(MetadataEntityView::from(row)))
}

#[derive(FromRow)]
struct MetadataEntityRow {
    id: Uuid,
    project_id: Uuid,
    encrypted_metadata: Vec<u8>,
    state: String,
    created_by_identity_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct MetadataEntityView {
    id: Uuid,
    project_id: Uuid,
    encrypted_metadata_b64: String,
    state: String,
    created_by_identity_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<MetadataEntityRow> for MetadataEntityView {
    fn from(row: MetadataEntityRow) -> Self {
        Self {
            id: row.id,
            project_id: row.project_id,
            encrypted_metadata_b64: encode(row.encrypted_metadata),
            state: row.state,
            created_by_identity_id: row.created_by_identity_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 payload"))
}

fn encode(value: Vec<u8>) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}
