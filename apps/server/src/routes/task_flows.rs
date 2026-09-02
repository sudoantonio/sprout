use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sprout_api_contract::{
    ArchiveRecurrenceSeriesRequest, CompleteTaskRequest, CompleteTaskResponse,
    CopyCompletedTaskRequest, CreatePresetAssignmentRequest, CreatePresetRequest,
    CreatePresetVersionRequest, CreateRecurrenceSeriesRequest, CreateTaskListRequest,
    CreateTaskRequest, CreateTopicRequest, EncryptedPayloadDto, ListPresetsResponse,
    ListTaskListsResponse, ListTasksResponse, ListTopicsResponse, MaterializationChoiceDto,
    MaterializePresetRequest, MaterializePresetResponse, PresetAssignmentDto,
    PresetAssignmentResponse, PresetDto, PresetResponse, PresetVersionDto, PresetVersionResponse,
    PretaskDto, RecurrenceSeriesDto, RecurrenceSeriesResponse, RecurrenceStateDto,
    ResourceEpochInputDto, ResourceKeyEnvelopeDto, TaskDto, TaskKindDto, TaskListDto,
    TaskListResponse, TaskResponse, TaskStateDto, TopicDto, TopicResponse,
    UpdateEncryptedResourceRequest, UpdatePresetRequest, UpdateTaskRequest,
};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::{
    pagination::{finish_page, parse_page},
    permissions::insert_initial_resource_epoch,
};
use crate::{
    AppState,
    auth::{
        AuthSession, ProjectAccess, ResourceAccess, require_project_access,
        require_resource_access, set_database_context,
    },
    error::AppError,
};

pub async fn create_topic(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateTopicRequest>,
) -> Result<Json<TopicResponse>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        request.parent_resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let header = request.header.as_ref().map(opaque_payload).transpose()?;
    if request.epoch.header_key_commitment_b64.is_some() != header.is_some() {
        return Err(AppError::BadRequest(
            "encrypted header and header key commitment must be supplied together",
        ));
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    insert_resource_node(
        &mut transaction,
        actor,
        project_id,
        request.resource_node_id,
        request.parent_resource_node_id,
        "topic",
        &payload,
    )
    .await?;
    insert_initial_resource_epoch(
        &mut transaction,
        actor,
        project_id,
        request.resource_node_id,
        actor.identity_id,
        &request.epoch,
        &request.envelopes,
    )
    .await?;
    let row = sqlx::query_as::<_, TopicRow>(
        r#"
        INSERT INTO topics (
            id, project_id, resource_node_id, encrypted_payload, encrypted_header
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING
            id, project_id, resource_node_id, encrypted_payload, encrypted_header,
            payload_version, created_at, deleted_at, key_epoch
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request.resource_node_id)
    .bind(&payload)
    .bind(&header)
    .fetch_one(&mut *transaction)
    .await?;
    insert_outbox(
        &mut transaction,
        project_id,
        "topic",
        row.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TopicResponse {
        topic: row.try_into()?,
    }))
}

pub async fn list_topics(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ListTopicsResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    // Deliberately no ORDER BY: task semantics and ordering stay client-side.
    let rows = sqlx::query_as::<_, TopicRow>(
        r#"
        SELECT
            topic.id, topic.project_id, topic.resource_node_id,
            CASE WHEN permission.access_scope = 'full'
                OR node.created_by_identity_id = $2
                THEN topic.encrypted_payload END AS encrypted_payload,
            topic.encrypted_header,
            topic.payload_version, topic.created_at, topic.deleted_at, topic.key_epoch
        FROM topics topic
        JOIN resource_nodes node
          ON node.project_id = topic.project_id
         AND node.id = topic.resource_node_id
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            topic.project_id, topic.resource_node_id, $2
        ) permission ON true
        WHERE topic.project_id = $1
          AND topic.deleted_at IS NULL
          AND (permission.access_scope IS NOT NULL OR node.created_by_identity_id = $2)
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ListTopicsResponse {
        topics: rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    }))
}

pub async fn get_topic(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, topic_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TopicResponse>, AppError> {
    let row = topic_row(&state, actor, project_id, topic_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        row.resource_node_id,
        ResourceAccess::Read,
    )
    .await?;
    Ok(Json(TopicResponse {
        topic: row.try_into()?,
    }))
}

pub async fn update_topic(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, topic_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateEncryptedResourceRequest>,
) -> Result<Json<TopicResponse>, AppError> {
    let current = topic_row(&state, actor, project_id, topic_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, TopicRow>(
        r#"
        UPDATE topics
        SET
            encrypted_payload = $3,
            key_epoch = $5,
            payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2
          AND payload_version = $4 AND deleted_at IS NULL
          AND EXISTS (
              SELECT 1 FROM resource_epochs epoch
              WHERE epoch.project_id = topics.project_id
                AND epoch.resource_node_id = topics.resource_node_id
                AND epoch.epoch = $5
                AND epoch.retired_at IS NULL
          )
        RETURNING
            id, project_id, resource_node_id, encrypted_payload,
            payload_version, created_at, deleted_at, key_epoch
        "#,
    )
    .bind(project_id)
    .bind(topic_id)
    .bind(&payload)
    .bind(to_i64(request.expected_payload_version)?)
    .bind(i32::try_from(request.key_epoch).map_err(|_| AppError::BadRequest("invalid key epoch"))?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    insert_outbox(
        &mut transaction,
        project_id,
        "topic",
        topic_id,
        "updated",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TopicResponse {
        topic: row.try_into()?,
    }))
}

pub async fn delete_topic(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, topic_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let current = topic_row(&state, actor, project_id, topic_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::Manage,
    )
    .await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let affected = sqlx::query(
        r#"
        UPDATE topics SET deleted_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(topic_id)
    .execute(&mut *transaction)
    .await?;
    if affected.rows_affected() != 1 {
        return Err(AppError::NotFound);
    }
    sqlx::query(
        "UPDATE resource_nodes SET deleted_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(current.resource_node_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_task_list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, topic_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateTaskListRequest>,
) -> Result<Json<TaskListResponse>, AppError> {
    if request.topic_id != topic_id {
        return Err(AppError::BadRequest("topic path and request do not match"));
    }
    let topic = topic_row(&state, actor, project_id, topic_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        topic.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let header = request.header.as_ref().map(opaque_payload).transpose()?;
    if request.epoch.header_key_commitment_b64.is_some() != header.is_some() {
        return Err(AppError::BadRequest(
            "encrypted header and header key commitment must be supplied together",
        ));
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    insert_resource_node(
        &mut transaction,
        actor,
        project_id,
        request.resource_node_id,
        topic.resource_node_id,
        "task_list",
        &payload,
    )
    .await?;
    insert_initial_resource_epoch(
        &mut transaction,
        actor,
        project_id,
        request.resource_node_id,
        actor.identity_id,
        &request.epoch,
        &request.envelopes,
    )
    .await?;
    let row = sqlx::query_as::<_, TaskListRow>(
        r#"
        INSERT INTO task_lists (
            id, project_id, topic_id, resource_node_id,
            encrypted_payload, encrypted_header
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING
            id, project_id, topic_id, resource_node_id,
            encrypted_payload, encrypted_header,
            payload_version, created_at, archived_at, key_epoch
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request.topic_id)
    .bind(request.resource_node_id)
    .bind(&payload)
    .bind(&header)
    .fetch_one(&mut *transaction)
    .await?;
    insert_outbox(
        &mut transaction,
        project_id,
        "task_list",
        row.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskListResponse {
        task_list: row.try_into()?,
    }))
}

pub async fn list_task_lists(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, topic_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListTaskListsResponse>, AppError> {
    let topic = topic_row(&state, actor, project_id, topic_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        topic.resource_node_id,
        ResourceAccess::ViewHeader,
    )
    .await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let rows = sqlx::query_as::<_, TaskListRow>(
        r#"
        SELECT
            task_list.id, task_list.project_id, task_list.topic_id,
            task_list.resource_node_id,
            CASE WHEN permission.access_scope = 'full'
                OR node.created_by_identity_id = $3
                THEN task_list.encrypted_payload END AS encrypted_payload,
            task_list.encrypted_header,
            task_list.payload_version, task_list.created_at,
            task_list.archived_at, task_list.key_epoch
        FROM task_lists task_list
        JOIN resource_nodes node
          ON node.project_id = task_list.project_id
         AND node.id = task_list.resource_node_id
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            task_list.project_id, task_list.resource_node_id, $3
        ) permission ON true
        WHERE task_list.project_id = $1
          AND task_list.topic_id = $2
          AND task_list.deleted_at IS NULL
          AND (permission.access_scope IS NOT NULL OR node.created_by_identity_id = $3)
        "#,
    )
    .bind(project_id)
    .bind(topic_id)
    .bind(actor.identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ListTaskListsResponse {
        task_lists: rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    }))
}

pub async fn get_task_list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, list_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskListResponse>, AppError> {
    let row = task_list_row(&state, actor, project_id, list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        row.resource_node_id,
        ResourceAccess::ViewHeader,
    )
    .await?;
    Ok(Json(TaskListResponse {
        task_list: row.try_into()?,
    }))
}

pub async fn update_task_list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, list_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateEncryptedResourceRequest>,
) -> Result<Json<TaskListResponse>, AppError> {
    let current = task_list_row(&state, actor, project_id, list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, TaskListRow>(
        r#"
        UPDATE task_lists
        SET
            encrypted_payload = $3,
            key_epoch = $5,
            payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2
          AND payload_version = $4 AND deleted_at IS NULL
          AND EXISTS (
              SELECT 1 FROM resource_epochs epoch
              WHERE epoch.project_id = task_lists.project_id
                AND epoch.resource_node_id = task_lists.resource_node_id
                AND epoch.epoch = $5
                AND epoch.retired_at IS NULL
          )
        RETURNING
            id, project_id, topic_id, resource_node_id, encrypted_payload,
            payload_version, created_at, archived_at, key_epoch
        "#,
    )
    .bind(project_id)
    .bind(list_id)
    .bind(&payload)
    .bind(to_i64(request.expected_payload_version)?)
    .bind(i32::try_from(request.key_epoch).map_err(|_| AppError::BadRequest("invalid key epoch"))?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    insert_outbox(
        &mut transaction,
        project_id,
        "task_list",
        list_id,
        "updated",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskListResponse {
        task_list: row.try_into()?,
    }))
}

pub async fn delete_task_list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, list_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let current = task_list_row(&state, actor, project_id, list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::Manage,
    )
    .await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query(
        r#"
        UPDATE task_lists
        SET archived_at = clock_timestamp(), deleted_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(list_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE resource_nodes SET deleted_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(current.resource_node_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    let list = task_list_row(&state, actor, project_id, request.list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        list.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    validate_task_kind_shape(
        request.task_kind,
        request.recurrence_series_id,
        request.occurrence_number,
    )?;
    let payload = opaque_payload(&request.payload)?;
    let header = request.header.as_ref().map(opaque_payload).transpose()?;
    if request.epoch.header_key_commitment_b64.is_some() != header.is_some() {
        return Err(AppError::BadRequest(
            "encrypted header and header key commitment must be supplied together",
        ));
    }
    let selected = opaque_payload(&request.selected_value_snapshot)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    validate_series(
        &mut transaction,
        project_id,
        request.list_id,
        request.recurrence_series_id,
    )
    .await?;
    insert_resource_node(
        &mut transaction,
        actor,
        project_id,
        request.resource_node_id,
        list.resource_node_id,
        "task",
        &payload,
    )
    .await?;
    insert_initial_resource_epoch(
        &mut transaction,
        actor,
        project_id,
        request.resource_node_id,
        actor.identity_id,
        &request.epoch,
        &request.envelopes,
    )
    .await?;
    let mut row = insert_task_row(
        &mut transaction,
        actor,
        project_id,
        request.id,
        request.list_id,
        request.resource_node_id,
        request.task_kind,
        &payload,
        &selected,
        None,
        None,
        None,
        request.questionnaire_version_id,
        request.recurrence_series_id,
        request.occurrence_number,
    )
    .await?;
    if let Some(header) = header {
        sqlx::query(
            "UPDATE tasks SET encrypted_header = $3
             WHERE project_id = $1 AND id = $2",
        )
        .bind(project_id)
        .bind(row.id)
        .bind(&header)
        .execute(&mut *transaction)
        .await?;
        row.encrypted_header = Some(header);
    }
    insert_outbox(
        &mut transaction,
        project_id,
        "task",
        row.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskResponse {
        task: row.try_into()?,
    }))
}

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, list_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListTasksResponse>, AppError> {
    let list = task_list_row(&state, actor, project_id, list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        list.resource_node_id,
        ResourceAccess::Read,
    )
    .await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let rows = sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} WHERE task.project_id = $1 AND task.task_list_id = $2
         AND task.deleted_at IS NULL
         AND (permission.access_scope IS NOT NULL OR resource.created_by_identity_id = $3)"
    ))
    .bind(project_id)
    .bind(list_id)
    .bind(actor.identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ListTasksResponse {
        tasks: rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    }))
}

pub async fn get_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskResponse>, AppError> {
    let row = task_row(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        row.resource_node_id,
        ResourceAccess::Read,
    )
    .await?;
    Ok(Json(TaskResponse {
        task: row.try_into()?,
    }))
}

pub async fn update_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateTaskRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    let current = task_row(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let selected = opaque_payload(&request.selected_value_snapshot)?;
    let (task_kind, questionnaire_version_id, recurrence_series_id, occurrence_number) =
        if request.update_task_metadata {
            (
                request.task_kind.ok_or(AppError::BadRequest(
                    "task kind is required when updating task metadata",
                ))?,
                request.questionnaire_version_id,
                request.recurrence_series_id,
                request.occurrence_number,
            )
        } else {
            (
                parse_task_kind(&current.task_kind)?,
                current.questionnaire_version_id,
                current.recurrence_series_id,
                current.occurrence_number.map(i64_to_u64).transpose()?,
            )
        };
    validate_task_kind_shape(task_kind, recurrence_series_id, occurrence_number)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    validate_series(
        &mut transaction,
        project_id,
        current.task_list_id,
        recurrence_series_id,
    )
    .await?;
    let query = format!(
        r#"
        UPDATE tasks
        SET
            encrypted_payload = $3,
            encrypted_value_snapshot = $4,
            key_epoch = $6,
            task_kind = $7,
            questionnaire_version_id = $8,
            recurrence_series_id = $9,
            occurrence_number = $10,
            payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2
          AND state = 'open' AND deleted_at IS NULL
          AND payload_version = $5
          AND EXISTS (
              SELECT 1 FROM resource_epochs epoch
              WHERE epoch.project_id = tasks.project_id
                AND epoch.resource_node_id = tasks.resource_node_id
                AND epoch.epoch = $6
                AND epoch.retired_at IS NULL
          )
        RETURNING {TASK_RETURNING}
        "#
    );
    let row = sqlx::query_as::<_, TaskRow>(&query)
        .bind(project_id)
        .bind(task_id)
        .bind(&payload)
        .bind(&selected)
        .bind(to_i64(request.expected_payload_version)?)
        .bind(
            i32::try_from(request.key_epoch)
                .map_err(|_| AppError::BadRequest("invalid key epoch"))?,
        )
        .bind(task_kind_str(task_kind))
        .bind(questionnaire_version_id)
        .bind(recurrence_series_id)
        .bind(occurrence_number.map(to_i64).transpose()?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
    insert_outbox(
        &mut transaction,
        project_id,
        "task",
        task_id,
        "updated",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskResponse {
        task: row.try_into()?,
    }))
}

pub async fn delete_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let current = task_row(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        current.resource_node_id,
        ResourceAccess::Manage,
    )
    .await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let affected = sqlx::query(
        "UPDATE tasks SET deleted_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2 AND state = 'open' AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(task_id)
    .execute(&mut *transaction)
    .await?;
    if affected.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    sqlx::query(
        "UPDATE resource_nodes SET deleted_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(current.resource_node_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn copy_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CopyCompletedTaskRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    let source = task_row(&state, actor, project_id, task_id).await?;
    if source.state != "completed" || source.id == request.new_task_id {
        return Err(AppError::Conflict);
    }
    let destination = task_list_row(&state, actor, project_id, request.destination_list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        destination.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    validate_task_kind_shape(
        parse_task_kind(&source.task_kind)?,
        request.recurrence_series_id,
        request.occurrence_number,
    )?;
    let payload = opaque_payload(&request.payload)?;
    let header = opaque_payload(&request.header)?;
    let selected = opaque_payload(&request.selected_value_snapshot)?;
    let assignment_payload = opaque_payload(&request.encrypted_assignment)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    validate_series(
        &mut transaction,
        project_id,
        request.destination_list_id,
        request.recurrence_series_id,
    )
    .await?;
    insert_resource_node(
        &mut transaction,
        actor,
        project_id,
        request.new_resource_node_id,
        destination.resource_node_id,
        "task",
        &payload,
    )
    .await?;
    insert_epoch_and_envelopes(
        &mut transaction,
        actor,
        project_id,
        request.new_resource_node_id,
        actor.identity_id,
        &request.epoch,
        &request.envelopes,
    )
    .await?;
    let mut row = insert_task_row(
        &mut transaction,
        actor,
        project_id,
        request.new_task_id,
        request.destination_list_id,
        request.new_resource_node_id,
        parse_task_kind(&source.task_kind)?,
        &payload,
        &selected,
        None,
        None,
        Some(task_id),
        source.questionnaire_version_id,
        request.recurrence_series_id,
        request.occurrence_number,
    )
    .await?;
    sqlx::query(
        "UPDATE tasks SET encrypted_header = $3
         WHERE project_id = $1 AND id = $2",
    )
    .bind(project_id)
    .bind(row.id)
    .bind(&header)
    .execute(&mut *transaction)
    .await?;
    row.encrypted_header = Some(header);
    insert_task_assignment_and_permission(
        &mut transaction,
        actor,
        project_id,
        row.id,
        request.new_resource_node_id,
        request.assignment_id,
        request.permission_grant_id,
        actor.identity_id,
        &assignment_payload,
    )
    .await?;
    row.active_assignment_id = Some(request.assignment_id);
    row.active_assignee_identity_id = Some(actor.identity_id);
    insert_outbox(
        &mut transaction,
        project_id,
        "task",
        row.id,
        "copied",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskResponse {
        task: row.try_into()?,
    }))
}

pub async fn complete_task(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CompleteTaskRequest>,
) -> Result<Json<CompleteTaskResponse>, AppError> {
    let mut request_hasher = Sha256::new();
    request_hasher.update(project_id.as_bytes());
    request_hasher.update(task_id.as_bytes());
    request_hasher.update(
        serde_json::to_vec(&request).map_err(|_| AppError::BadRequest("invalid completion"))?,
    );
    let request_hash = request_hasher.finalize().to_vec();
    let completion_payload = opaque_payload(&request.encrypted_completion)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text || ':' || $2::uuid::text, 13))",
    )
    .bind(project_id)
    .bind(task_id)
    .execute(&mut *transaction)
    .await?;

    if let Some(existing) = completion_by_idempotency(
        &mut transaction,
        project_id,
        actor.identity_id,
        request.idempotency_key,
    )
    .await?
    {
        if existing.request_hash.as_deref() != Some(request_hash.as_slice()) {
            return Err(AppError::Conflict);
        }
        let response = completion_response(&mut transaction, project_id, existing, true).await?;
        transaction.commit().await?;
        return Ok(Json(response));
    }

    let current = fetch_task_for_update(&mut transaction, project_id, task_id).await?;
    if current.state == "completed" {
        let existing = completion_for_task(&mut transaction, project_id, task_id)
            .await?
            .ok_or(AppError::Conflict)?;
        if request.recurrence_series_id == existing.recurrence_series_id
            && request.occurrence_number
                == existing.occurrence_number.map(i64_to_u64).transpose()?
        {
            let response =
                completion_response(&mut transaction, project_id, existing, true).await?;
            transaction.commit().await?;
            return Ok(Json(response));
        }
        return Err(AppError::Conflict);
    }
    if current.payload_version != to_i64(request.expected_payload_version)? {
        return Err(AppError::Conflict);
    }
    let assignee = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT assignee_identity_id
        FROM task_assignments
        WHERE project_id = $1 AND task_id = $2 AND id = $3
          AND revoked_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .bind(request.assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    if assignee != actor.identity_id {
        return Err(AppError::Forbidden);
    }

    let next_row = match (
        current.recurrence_series_id,
        current.occurrence_number,
        request.recurrence_series_id,
        request.occurrence_number,
        request.next_occurrence.as_ref(),
    ) {
        (None, None, None, None, None) => None,
        (
            Some(series_id),
            Some(current_occurrence),
            Some(request_series),
            Some(request_occurrence),
            Some(next),
        ) if series_id == request_series
            && next.recurrence_series_id == series_id
            && request_occurrence == i64_to_u64(current_occurrence)? + 1
            && next.occurrence_number == request_occurrence =>
        {
            sqlx::query(
                "SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text || ':' || $2::uuid::text, 14))",
            )
            .bind(project_id)
            .bind(series_id)
            .execute(&mut *transaction)
            .await?;
            if let Some(existing) =
                task_for_occurrence(&mut transaction, project_id, series_id, request_occurrence)
                    .await?
            {
                Some(existing)
            } else {
                let list_resource_id = sqlx::query_scalar::<_, Uuid>(
                    "SELECT resource_node_id FROM task_lists
                     WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
                )
                .bind(project_id)
                .bind(current.task_list_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(AppError::NotFound)?;
                let next_payload = opaque_payload(&next.payload)?;
                let next_header = opaque_payload(&next.header)?;
                let next_selected = opaque_payload(&next.selected_value_snapshot)?;
                insert_resource_node(
                    &mut transaction,
                    actor,
                    project_id,
                    next.resource_node_id,
                    list_resource_id,
                    "task",
                    &next_payload,
                )
                .await?;
                insert_epoch_and_envelopes(
                    &mut transaction,
                    actor,
                    project_id,
                    next.resource_node_id,
                    actor.identity_id,
                    &next.epoch,
                    &next.envelopes,
                )
                .await?;
                let mut next_row = insert_task_row(
                    &mut transaction,
                    actor,
                    project_id,
                    next.id,
                    current.task_list_id,
                    next.resource_node_id,
                    TaskKindDto::Recurring,
                    &next_payload,
                    &next_selected,
                    None,
                    None,
                    None,
                    current.questionnaire_version_id,
                    Some(series_id),
                    Some(request_occurrence),
                )
                .await?;
                sqlx::query(
                    "UPDATE tasks SET encrypted_header = $3
                     WHERE project_id = $1 AND id = $2",
                )
                .bind(project_id)
                .bind(next_row.id)
                .bind(&next_header)
                .execute(&mut *transaction)
                .await?;
                next_row.encrypted_header = Some(next_header);
                let assignment_payload = opaque_payload(&next.encrypted_assignment)?;
                insert_task_assignment_and_permission(
                    &mut transaction,
                    actor,
                    project_id,
                    next.id,
                    next.resource_node_id,
                    next.assignment_id,
                    next.permission_grant_id,
                    actor.identity_id,
                    &assignment_payload,
                )
                .await?;
                Some(next_row)
            }
        }
        _ => {
            return Err(AppError::BadRequest(
                "recurring completion requires one sequential client occurrence",
            ));
        }
    };

    let occurrence_key = request.completion_id;
    let _completion = sqlx::query_as::<_, CompletionRow>(
        r#"
        INSERT INTO task_completions (
            id, project_id, task_id, assignment_id,
            assignee_identity_id, recorded_by_identity_id,
            occurrence_key, encrypted_payload, completed_at,
            idempotency_key, request_hash,
            recurrence_series_id, occurrence_number, next_task_id
        )
        VALUES (
            $1, $2, $3, $4, $5, $5, $6, $7, $8,
            $9, $10, $11, $12, $13
        )
        RETURNING
            task_id, request_hash, recurrence_series_id,
            occurrence_number, next_task_id
        "#,
    )
    .bind(request.completion_id)
    .bind(project_id)
    .bind(task_id)
    .bind(request.assignment_id)
    .bind(actor.identity_id)
    .bind(occurrence_key)
    .bind(&completion_payload)
    .bind(request.completed_at)
    .bind(request.idempotency_key)
    .bind(&request_hash)
    .bind(request.recurrence_series_id)
    .bind(request.occurrence_number.map(to_i64).transpose()?)
    .bind(next_row.as_ref().map(|row| row.id))
    .fetch_one(&mut *transaction)
    .await?;

    let update = format!(
        r#"
        UPDATE tasks
        SET
            state = 'completed',
            completed_by_identity_id = $3,
            completed_at = $4,
            payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2 AND state = 'open'
        RETURNING {TASK_RETURNING}
        "#
    );
    let completed = sqlx::query_as::<_, TaskRow>(&update)
        .bind(project_id)
        .bind(task_id)
        .bind(actor.identity_id)
        .bind(request.completed_at)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
    insert_outbox(
        &mut transaction,
        project_id,
        "task",
        task_id,
        "completed",
        request.idempotency_key,
        &completion_payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(CompleteTaskResponse {
        completed_task: completed.try_into()?,
        next_task: next_row.map(TryInto::try_into).transpose()?,
        replayed: false,
    }))
}

pub async fn create_recurrence(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateRecurrenceSeriesRequest>,
) -> Result<Json<RecurrenceSeriesResponse>, AppError> {
    let list = task_list_row(&state, actor, project_id, request.list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        list.resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    let rule = opaque_payload(&request.encrypted_rule)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, RecurrenceRow>(
        r#"
        INSERT INTO recurrence_series (
            id, project_id, task_list_id, encrypted_rule,
            created_by_identity_id
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING
            id, project_id, task_list_id, encrypted_rule,
            payload_version, state, created_at, archived_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request.list_id)
    .bind(&rule)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    insert_outbox(
        &mut transaction,
        project_id,
        "recurrence_series",
        row.id,
        "created",
        request.idempotency_key,
        &rule,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(RecurrenceSeriesResponse {
        series: row.try_into()?,
    }))
}

pub async fn get_recurrence(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, series_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RecurrenceSeriesResponse>, AppError> {
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = recurrence_row(&mut transaction, project_id, series_id).await?;
    let resource_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM task_lists
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(row.task_list_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
        ResourceAccess::Read,
    )
    .await?;
    Ok(Json(RecurrenceSeriesResponse {
        series: row.try_into()?,
    }))
}

pub async fn archive_recurrence(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, series_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ArchiveRecurrenceSeriesRequest>,
) -> Result<Json<RecurrenceSeriesResponse>, AppError> {
    let mut transaction = begin(&state, actor, project_id).await?;
    let current = recurrence_row(&mut transaction, project_id, series_id).await?;
    let resource_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM task_lists
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(current.task_list_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
        ResourceAccess::Manage,
    )
    .await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, RecurrenceRow>(
        r#"
        UPDATE recurrence_series
        SET
            state = 'archived',
            archived_at = clock_timestamp(),
            payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2 AND state = 'active'
          AND payload_version = $3
        RETURNING
            id, project_id, task_list_id, encrypted_rule,
            payload_version, state, created_at, archived_at
        "#,
    )
    .bind(project_id)
    .bind(series_id)
    .bind(to_i64(request.expected_payload_version)?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    insert_outbox(
        &mut transaction,
        project_id,
        "recurrence_series",
        series_id,
        "archived",
        request.idempotency_key,
        &row.encrypted_rule,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(RecurrenceSeriesResponse {
        series: row.try_into()?,
    }))
}

pub async fn create_preset(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreatePresetRequest>,
) -> Result<Json<PresetResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, PresetRow>(
        r#"
        INSERT INTO presets (
            id, project_id, encrypted_metadata, created_by_identity_id
        )
        VALUES ($1, $2, $3, $4)
        RETURNING
            id, project_id, encrypted_metadata, created_at, deleted_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(&payload)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    insert_outbox(
        &mut transaction,
        project_id,
        "preset",
        row.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(PresetResponse {
        preset: row.try_into()?,
    }))
}

pub async fn list_presets(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Query(query): Query<sprout_api_contract::CollectionPageQuery>,
) -> Result<Json<ListPresetsResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let page = parse_page(query)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let mut rows = sqlx::query_as::<_, PresetRow>(
        r#"
        SELECT
            preset.id, preset.project_id, preset.encrypted_metadata,
            preset.created_at, preset.deleted_at
        FROM presets preset
        WHERE preset.project_id = $1
          AND preset.state <> 'deleted'
          AND (
              $2::timestamptz IS NULL
              OR (preset.created_at, preset.id) > ($2::timestamptz, $3::uuid)
          )
        ORDER BY preset.created_at, preset.id
        LIMIT $4
        "#,
    )
    .bind(project_id)
    .bind(page.after_created_at)
    .bind(page.after_id)
    .bind(page.sql_limit()?)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let next_cursor = finish_page(&mut rows, page, |row| (row.created_at, row.id))?;
    Ok(Json(ListPresetsResponse {
        presets: rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
        next_cursor,
    }))
}

pub async fn get_preset(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, preset_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<PresetResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, PresetRow>(
        r#"
        SELECT id, project_id, encrypted_metadata, created_at, deleted_at
        FROM presets
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(preset_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(Json(PresetResponse {
        preset: row.try_into()?,
    }))
}

pub async fn update_preset(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, preset_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdatePresetRequest>,
) -> Result<Json<PresetResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, PresetRow>(
        r#"
        UPDATE presets
        SET encrypted_metadata = $3
        WHERE project_id = $1 AND id = $2 AND state = 'active'
        RETURNING id, project_id, encrypted_metadata, created_at, deleted_at
        "#,
    )
    .bind(project_id)
    .bind(preset_id)
    .bind(&payload)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    insert_outbox(
        &mut transaction,
        project_id,
        "preset",
        preset_id,
        "updated",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(PresetResponse {
        preset: row.try_into()?,
    }))
}

pub async fn delete_preset(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, preset_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let affected = sqlx::query(
        r#"
        UPDATE presets
        SET state = 'deleted', deleted_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2 AND state <> 'deleted'
        "#,
    )
    .bind(project_id)
    .bind(preset_id)
    .execute(&mut *transaction)
    .await?;
    if affected.rows_affected() != 1 {
        return Err(AppError::NotFound);
    }
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_preset_version(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, preset_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreatePresetVersionRequest>,
) -> Result<Json<PresetVersionResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    if request.pretasks.is_empty() {
        return Err(AppError::BadRequest(
            "preset version requires at least one pretask",
        ));
    }
    let mut ids = HashSet::with_capacity(request.pretasks.len());
    if request
        .pretasks
        .iter()
        .any(|pretask| !ids.insert(pretask.id))
    {
        return Err(AppError::BadRequest("duplicate pretask"));
    }
    let payload = opaque_payload(&request.payload)?;
    let content_hash = decode(&request.content_hash_b64)?;
    if content_hash.len() < 16 {
        return Err(AppError::BadRequest("content hash is too short"));
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text || ':' || $2::uuid::text, 16))",
    )
    .bind(project_id)
    .bind(preset_id)
    .execute(&mut *transaction)
    .await?;
    let version_number = sqlx::query_scalar::<_, i32>(
        r#"
        SELECT COALESCE(max(version_number), 0) + 1
        FROM preset_versions
        WHERE project_id = $1 AND preset_id = $2
        "#,
    )
    .bind(project_id)
    .bind(preset_id)
    .fetch_one(&mut *transaction)
    .await?;
    let created_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        INSERT INTO preset_versions (
            id, project_id, preset_id, version_number,
            encrypted_payload, content_hash, created_by_identity_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING created_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(preset_id)
    .bind(version_number)
    .bind(&payload)
    .bind(content_hash)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    for (ordinal, pretask) in request.pretasks.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO preset_pretasks (
                id, project_id, preset_version_id, client_key,
                ordinal, task_kind, encrypted_payload
            )
            VALUES ($1, $2, $3, $1, $4, $5, $6)
            "#,
        )
        .bind(pretask.id)
        .bind(project_id)
        .bind(request.id)
        .bind(i32::try_from(ordinal).map_err(|_| AppError::BadRequest("too many pretasks"))?)
        .bind(task_kind_str(pretask.task_kind))
        .bind(opaque_payload(&pretask.payload)?)
        .execute(&mut *transaction)
        .await?;
    }
    insert_outbox(
        &mut transaction,
        project_id,
        "preset_version",
        request.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(PresetVersionResponse {
        version: PresetVersionDto {
            id: request.id,
            preset_id,
            project_id,
            version_number: u32::try_from(version_number).map_err(|_| AppError::Internal)?,
            payload: request.payload,
            pretasks: request.pretasks,
            created_at,
        },
    }))
}

pub async fn get_preset_version(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, preset_id, version_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<PresetVersionResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, PresetVersionRow>(
        r#"
        SELECT
            id, project_id, preset_id, version_number,
            encrypted_payload, created_at
        FROM preset_versions
        WHERE project_id = $1 AND preset_id = $2 AND id = $3
        "#,
    )
    .bind(project_id)
    .bind(preset_id)
    .bind(version_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let pretasks = sqlx::query_as::<_, PretaskRow>(
        r#"
        SELECT id, task_kind, encrypted_payload
        FROM preset_pretasks
        WHERE project_id = $1 AND preset_version_id = $2
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(PresetVersionResponse {
        version: PresetVersionDto {
            id: row.id,
            preset_id: row.preset_id,
            project_id: row.project_id,
            version_number: u32::try_from(row.version_number).map_err(|_| AppError::Internal)?,
            payload: payload_from_bytes(&row.encrypted_payload)?,
            pretasks: pretasks
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            created_at: row.created_at,
        },
    }))
}

pub async fn create_preset_assignment(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreatePresetAssignmentRequest>,
) -> Result<Json<PresetAssignmentResponse>, AppError> {
    let list = task_list_row(&state, actor, project_id, request.destination_list_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        list.resource_node_id,
        ResourceAccess::Manage,
    )
    .await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    require_assignment_recipient(
        &mut transaction,
        project_id,
        actor.identity_id,
        request.assigned_to_identity_id,
        list.resource_node_id,
    )
    .await?;
    let pretasks =
        load_pretask_kinds(&mut transaction, project_id, request.preset_version_id).await?;
    validate_selections(&pretasks, &request.selections)?;
    let row = sqlx::query_as::<_, PresetAssignmentRow>(
        r#"
        INSERT INTO preset_assignments (
            id, project_id, preset_version_id, destination_task_list_id,
            assigned_to_identity_id, assigned_by_identity_id, encrypted_payload
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING
            id, preset_version_id, destination_task_list_id,
            assigned_to_identity_id, assigned_by_identity_id,
            payload_version, state, created_at, materialized_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request.preset_version_id)
    .bind(request.destination_list_id)
    .bind(request.assigned_to_identity_id)
    .bind(actor.identity_id)
    .bind(&payload)
    .fetch_one(&mut *transaction)
    .await?;
    for selection in &request.selections {
        sqlx::query(
            r#"
            INSERT INTO preset_assignment_values (
                project_id, preset_assignment_id, preset_version_id,
                pretask_id, task_kind, encrypted_selected_value
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(project_id)
        .bind(request.id)
        .bind(request.preset_version_id)
        .bind(selection.pretask_id)
        .bind(task_kind_str(selection.task_kind))
        .bind(opaque_payload(&selection.selected_value)?)
        .execute(&mut *transaction)
        .await?;
    }
    insert_outbox(
        &mut transaction,
        project_id,
        "preset_assignment",
        row.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(PresetAssignmentResponse {
        assignment: row.into(),
    }))
}

pub async fn materialize_preset(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, assignment_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<MaterializePresetRequest>,
) -> Result<Json<MaterializePresetResponse>, AppError> {
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text || ':' || $2::uuid::text, 15))",
    )
    .bind(project_id)
    .bind(assignment_id)
    .execute(&mut *transaction)
    .await?;
    let assignment = sqlx::query_as::<_, PresetAssignmentRow>(
        r#"
        SELECT
            id, preset_version_id, destination_task_list_id,
            assigned_to_identity_id, assigned_by_identity_id,
            payload_version, state, created_at, materialized_at
        FROM preset_assignments
        WHERE project_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let list = sqlx::query_as::<_, (Uuid, Uuid, bool)>(
        r#"
        SELECT id, resource_node_id,
               deleted_at IS NULL AND archived_at IS NULL AS active
        FROM task_lists
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(project_id)
    .bind(assignment.destination_task_list_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    require_assignment_materializer(
        &mut transaction,
        project_id,
        actor.identity_id,
        assignment.assigned_by_identity_id,
        assignment.assigned_to_identity_id,
        list.1,
    )
    .await?;
    if assignment.state == "materialized" {
        let rows = sqlx::query_as::<_, TaskRow>(&format!(
            "{TASK_SELECT} JOIN preset_assignment_materialized_tasks materialized ON materialized.project_id = task.project_id AND materialized.task_id = task.id WHERE materialized.project_id = $1 AND materialized.preset_assignment_id = $2"
        ))
        .bind(project_id)
        .bind(assignment_id)
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(Json(MaterializePresetResponse {
            tasks: rows
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        }));
    }
    if assignment.state != "active"
        || assignment.payload_version != to_i64(request.expected_assignment_version)?
        || !list.2
    {
        return Err(AppError::Conflict);
    }
    let pretasks =
        load_pretask_kinds(&mut transaction, project_id, assignment.preset_version_id).await?;
    validate_materialization_choices(&pretasks, &request.choices)?;

    let stored_values = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT pretask_id, task_kind
        FROM preset_assignment_values
        WHERE project_id = $1 AND preset_assignment_id = $2
        "#,
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_all(&mut *transaction)
    .await?
    .into_iter()
    .collect::<HashMap<_, _>>();
    if stored_values.len() != pretasks.len()
        || pretasks
            .iter()
            .any(|(id, kind)| stored_values.get(id) != Some(kind))
    {
        return Err(AppError::BadRequest(
            "preset assignment values are incomplete or incompatible",
        ));
    }

    let mut tasks = Vec::with_capacity(request.choices.len());
    for choice in &request.choices {
        validate_task_kind_shape(
            choice.task_kind,
            choice.recurrence_series_id,
            choice.occurrence_number,
        )?;
        validate_series(
            &mut transaction,
            project_id,
            assignment.destination_task_list_id,
            choice.recurrence_series_id,
        )
        .await?;
        let snapshot = opaque_payload(&choice.task_snapshot)?;
        let header = opaque_payload(&choice.header)?;
        let selected = opaque_payload(&choice.selected_value_snapshot)?;
        insert_resource_node(
            &mut transaction,
            actor,
            project_id,
            choice.task_resource_node_id,
            list.1,
            "task",
            &snapshot,
        )
        .await?;
        insert_epoch_and_envelopes(
            &mut transaction,
            actor,
            project_id,
            choice.task_resource_node_id,
            assignment.assigned_to_identity_id,
            &choice.epoch,
            &choice.envelopes,
        )
        .await?;
        let mut task = insert_task_row(
            &mut transaction,
            actor,
            project_id,
            choice.task_id,
            assignment.destination_task_list_id,
            choice.task_resource_node_id,
            choice.task_kind,
            &snapshot,
            &selected,
            Some(choice.pretask_id),
            Some(assignment_id),
            None,
            None,
            choice.recurrence_series_id,
            choice.occurrence_number,
        )
        .await?;
        sqlx::query(
            "UPDATE tasks SET encrypted_header = $3
             WHERE project_id = $1 AND id = $2",
        )
        .bind(project_id)
        .bind(task.id)
        .bind(&header)
        .execute(&mut *transaction)
        .await?;
        task.encrypted_header = Some(header);
        let assignment_payload = opaque_payload(&choice.encrypted_assignment)?;
        insert_task_assignment_and_permission(
            &mut transaction,
            actor,
            project_id,
            choice.task_id,
            choice.task_resource_node_id,
            choice.assignment_id,
            choice.permission_grant_id,
            assignment.assigned_to_identity_id,
            &assignment_payload,
        )
        .await?;
        task.active_assignment_id = Some(choice.assignment_id);
        task.active_assignee_identity_id = Some(assignment.assigned_to_identity_id);
        sqlx::query(
            r#"
            INSERT INTO preset_assignment_materialized_tasks (
                project_id, preset_assignment_id, preset_version_id,
                pretask_id, task_id, task_kind,
                encrypted_selected_value_snapshot, encrypted_task_snapshot
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(project_id)
        .bind(assignment_id)
        .bind(assignment.preset_version_id)
        .bind(choice.pretask_id)
        .bind(choice.task_id)
        .bind(task_kind_str(choice.task_kind))
        .bind(&selected)
        .bind(&snapshot)
        .execute(&mut *transaction)
        .await?;
        tasks.push(task);
    }
    let affected = sqlx::query(
        r#"
        UPDATE preset_assignments
        SET
            state = 'materialized',
            materialized_at = clock_timestamp(),
            payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2
          AND state = 'active' AND payload_version = $3
        "#,
    )
    .bind(project_id)
    .bind(assignment_id)
    .bind(to_i64(request.expected_assignment_version)?)
    .execute(&mut *transaction)
    .await?;
    if affected.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let outbox_payload = opaque_payload(&request.choices[0].task_snapshot)?;
    insert_outbox(
        &mut transaction,
        project_id,
        "preset_assignment",
        assignment_id,
        "materialized",
        request.idempotency_key,
        &outbox_payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(MaterializePresetResponse {
        tasks: tasks
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    }))
}

async fn begin(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
) -> Result<Transaction<'_, Postgres>, AppError> {
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

async fn insert_resource_node(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    id: Uuid,
    parent_id: Uuid,
    node_kind: &str,
    encrypted_metadata: &[u8],
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO resource_nodes (
            id, project_id, parent_id, node_kind,
            encrypted_metadata, created_by_identity_id
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(project_id)
    .bind(parent_id)
    .bind(node_kind)
    .bind(encrypted_metadata)
    .bind(actor.identity_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_epoch_and_envelopes(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    resource_id: Uuid,
    recipient_identity_id: Uuid,
    epoch: &ResourceEpochInputDto,
    envelopes: &[ResourceKeyEnvelopeDto],
) -> Result<(), AppError> {
    insert_initial_resource_epoch(
        transaction,
        actor,
        project_id,
        resource_id,
        recipient_identity_id,
        epoch,
        envelopes,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_task_assignment_and_permission(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    task_id: Uuid,
    task_resource_id: Uuid,
    assignment_id: Uuid,
    permission_grant_id: Uuid,
    assignee_id: Uuid,
    encrypted_payload: &[u8],
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO task_assignments (
            id, project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, encrypted_payload,
            permission_root_grant_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(assignment_id)
    .bind(project_id)
    .bind(task_id)
    .bind(assignee_id)
    .bind(actor.identity_id)
    .bind(encrypted_payload)
    .bind(permission_grant_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        SELECT sprout_private.grant_hierarchical_permission(
            $1, $2, $3, 'edit', 'full', 'restricted',
            $4, $5, 'assignment', $6
        )
        "#,
    )
    .bind(project_id)
    .bind(task_resource_id)
    .bind(assignee_id)
    .bind(permission_grant_id)
    .bind(actor.identity_id)
    .bind(assignment_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Materializes the exact R4 task used by a certified governance review.
/// The human controller supplies only E2EE ciphertext/key envelopes; the
/// trusted governance route fixes creator, assignee and causal identity.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn materialize_governance_review_task(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    creator_agent_identity_id: Uuid,
    administrator_identity_id: Uuid,
    request: &CreateTaskRequest,
    assignment_id: Uuid,
    permission_grant_id: Uuid,
    encrypted_assignment: &EncryptedPayloadDto,
) -> Result<(), AppError> {
    validate_task_kind_shape(
        request.task_kind,
        request.recurrence_series_id,
        request.occurrence_number,
    )?;
    if request.recurrence_series_id.is_some()
        || request.occurrence_number.is_some()
        || request.questionnaire_version_id.is_some()
    {
        return Err(AppError::BadRequest(
            "governance review task must be a standalone task",
        ));
    }
    let list_resource_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM task_lists
         WHERE project_id=$1 AND id=$2 AND deleted_at IS NULL FOR SHARE",
    )
    .bind(project_id)
    .bind(request.list_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let payload = opaque_payload(&request.payload)?;
    let selected = opaque_payload(&request.selected_value_snapshot)?;
    let encrypted_assignment = opaque_payload(encrypted_assignment)?;
    let header = request.header.as_ref().map(opaque_payload).transpose()?;
    if request.epoch.header_key_commitment_b64.is_some() != header.is_some() {
        return Err(AppError::BadRequest(
            "encrypted header and header key commitment must be supplied together",
        ));
    }
    sqlx::query(
        "INSERT INTO resource_nodes (
            id,project_id,parent_id,node_kind,encrypted_metadata,created_by_identity_id
         ) VALUES ($1,$2,$3,'task',$4,$5)",
    )
    .bind(request.resource_node_id)
    .bind(project_id)
    .bind(list_resource_id)
    .bind(&payload)
    .bind(creator_agent_identity_id)
    .execute(&mut **transaction)
    .await?;
    insert_initial_resource_epoch(
        transaction,
        actor,
        project_id,
        request.resource_node_id,
        administrator_identity_id,
        &request.epoch,
        &request.envelopes,
    )
    .await?;
    sqlx::query(
        "INSERT INTO tasks (
            id,project_id,task_list_id,resource_node_id,task_kind,
            encrypted_payload,encrypted_value_snapshot,created_by_identity_id,
            encrypted_header
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request.list_id)
    .bind(request.resource_node_id)
    .bind(task_kind_str(request.task_kind))
    .bind(&payload)
    .bind(&selected)
    .bind(creator_agent_identity_id)
    .bind(header)
    .execute(&mut **transaction)
    .await?;
    insert_task_assignment_and_permission(
        transaction,
        actor,
        project_id,
        request.id,
        request.resource_node_id,
        assignment_id,
        permission_grant_id,
        administrator_identity_id,
        &encrypted_assignment,
    )
    .await?;
    insert_outbox(
        transaction,
        project_id,
        "task",
        request.id,
        "created",
        request.idempotency_key,
        &payload,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_task_row(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    id: Uuid,
    list_id: Uuid,
    resource_node_id: Uuid,
    kind: TaskKindDto,
    payload: &[u8],
    selected: &[u8],
    source_pretask_id: Option<Uuid>,
    preset_assignment_id: Option<Uuid>,
    copied_from_task_id: Option<Uuid>,
    questionnaire_version_id: Option<Uuid>,
    recurrence_series_id: Option<Uuid>,
    occurrence_number: Option<u64>,
) -> Result<TaskRow, AppError> {
    let query = format!(
        r#"
        INSERT INTO tasks (
            id, project_id, task_list_id, resource_node_id,
            task_kind, encrypted_payload, encrypted_value_snapshot,
            source_pretask_id, preset_assignment_id, copied_from_task_id,
            questionnaire_version_id, recurrence_series_id,
            occurrence_number, created_by_identity_id
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10, $11, $12, $13, $14
        )
        RETURNING {TASK_RETURNING}
        "#
    );
    Ok(sqlx::query_as::<_, TaskRow>(&query)
        .bind(id)
        .bind(project_id)
        .bind(list_id)
        .bind(resource_node_id)
        .bind(task_kind_str(kind))
        .bind(payload)
        .bind(selected)
        .bind(source_pretask_id)
        .bind(preset_assignment_id)
        .bind(copied_from_task_id)
        .bind(questionnaire_version_id)
        .bind(recurrence_series_id)
        .bind(occurrence_number.map(to_i64).transpose()?)
        .bind(actor.identity_id)
        .fetch_one(&mut **transaction)
        .await?)
}

pub(crate) async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    aggregate_kind: &str,
    aggregate_id: Uuid,
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
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (project_id, deduplication_key) DO NOTHING
        "#,
    )
    .bind(project_id)
    .bind(aggregate_kind)
    .bind(aggregate_id)
    .bind(event_kind)
    .bind(idempotency_key)
    .bind(encrypted_payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn topic_row(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    topic_id: Uuid,
) -> Result<TopicRow, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query_as::<_, TopicRow>(
        r#"
        SELECT
            id, project_id, resource_node_id, encrypted_payload,
            payload_version, created_at, deleted_at, key_epoch
        FROM topics
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(topic_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(row)
}

async fn task_list_row(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    list_id: Uuid,
) -> Result<TaskListRow, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query_as::<_, TaskListRow>(
        r#"
        SELECT
            id, project_id, topic_id, resource_node_id, encrypted_payload,
            payload_version, created_at, archived_at, key_epoch
        FROM task_lists
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(list_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(row)
}

async fn task_row(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<TaskRow, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} WHERE task.project_id = $1 AND task.id = $2 AND task.deleted_at IS NULL"
    ))
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(row)
}

async fn fetch_task_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<TaskRow, AppError> {
    sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} WHERE task.project_id = $1 AND task.id = $2 AND task.deleted_at IS NULL FOR UPDATE OF task"
    ))
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

async fn task_for_occurrence(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    series_id: Uuid,
    occurrence_number: u64,
) -> Result<Option<TaskRow>, AppError> {
    Ok(sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} WHERE task.project_id = $1 AND task.recurrence_series_id = $2 AND task.occurrence_number = $3 AND task.deleted_at IS NULL"
    ))
    .bind(project_id)
    .bind(series_id)
    .bind(to_i64(occurrence_number)?)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn recurrence_row(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    series_id: Uuid,
) -> Result<RecurrenceRow, AppError> {
    sqlx::query_as::<_, RecurrenceRow>(
        r#"
        SELECT
            id, project_id, task_list_id, encrypted_rule,
            payload_version, state, created_at, archived_at
        FROM recurrence_series
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(project_id)
    .bind(series_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

async fn validate_series(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    list_id: Uuid,
    series_id: Option<Uuid>,
) -> Result<(), AppError> {
    if let Some(series_id) = series_id {
        let valid = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM recurrence_series
                WHERE project_id = $1 AND id = $2
                  AND task_list_id = $3 AND state = 'active'
            )
            "#,
        )
        .bind(project_id)
        .bind(series_id)
        .bind(list_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !valid {
            return Err(AppError::BadRequest(
                "recurrence series is not active on the destination list",
            ));
        }
    }
    Ok(())
}

async fn completion_by_idempotency(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor_id: Uuid,
    idempotency_key: Uuid,
) -> Result<Option<CompletionRow>, AppError> {
    Ok(sqlx::query_as::<_, CompletionRow>(
        r#"
        SELECT
            task_id, request_hash, recurrence_series_id,
            occurrence_number, next_task_id
        FROM task_completions
        WHERE project_id = $1
          AND recorded_by_identity_id = $2
          AND idempotency_key = $3
        "#,
    )
    .bind(project_id)
    .bind(actor_id)
    .bind(idempotency_key)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn completion_for_task(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<Option<CompletionRow>, AppError> {
    Ok(sqlx::query_as::<_, CompletionRow>(
        r#"
        SELECT
            task_id, request_hash, recurrence_series_id,
            occurrence_number, next_task_id
        FROM task_completions
        WHERE project_id = $1 AND task_id = $2
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn completion_response(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    completion: CompletionRow,
    replayed: bool,
) -> Result<CompleteTaskResponse, AppError> {
    let completed = sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT} WHERE task.project_id = $1 AND task.id = $2"
    ))
    .bind(project_id)
    .bind(completion.task_id)
    .fetch_one(&mut **transaction)
    .await?;
    let next = if let Some(next_task_id) = completion.next_task_id {
        Some(
            sqlx::query_as::<_, TaskRow>(&format!(
                "{TASK_SELECT} WHERE task.project_id = $1 AND task.id = $2"
            ))
            .bind(project_id)
            .bind(next_task_id)
            .fetch_one(&mut **transaction)
            .await?,
        )
    } else {
        None
    };
    Ok(CompleteTaskResponse {
        completed_task: completed.try_into()?,
        next_task: next.map(TryInto::try_into).transpose()?,
        replayed,
    })
}

async fn load_pretask_kinds(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    version_id: Uuid,
) -> Result<HashMap<Uuid, String>, AppError> {
    let version_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM preset_versions
            WHERE project_id = $1 AND id = $2
        )",
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !version_exists {
        return Err(AppError::NotFound);
    }
    Ok(sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, task_kind FROM preset_pretasks
         WHERE project_id = $1 AND preset_version_id = $2",
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .collect())
}

fn validate_selections(
    expected: &HashMap<Uuid, String>,
    selections: &[sprout_api_contract::PretaskSelectionDto],
) -> Result<(), AppError> {
    if expected.is_empty() || expected.len() != selections.len() {
        return Err(AppError::BadRequest(
            "exactly one selected value is required per pretask",
        ));
    }
    let mut seen = HashSet::with_capacity(selections.len());
    for selection in selections {
        if !seen.insert(selection.pretask_id)
            || expected.get(&selection.pretask_id).map(String::as_str)
                != Some(task_kind_str(selection.task_kind))
        {
            return Err(AppError::BadRequest(
                "selected value is missing, duplicated, or incompatible",
            ));
        }
        opaque_payload(&selection.selected_value)?;
    }
    Ok(())
}

fn validate_materialization_choices(
    expected: &HashMap<Uuid, String>,
    choices: &[MaterializationChoiceDto],
) -> Result<(), AppError> {
    if expected.is_empty() || expected.len() != choices.len() {
        return Err(AppError::BadRequest(
            "exactly one materialization choice is required per pretask",
        ));
    }
    let mut pretasks = HashSet::with_capacity(choices.len());
    let mut task_ids = HashSet::with_capacity(choices.len());
    let mut resource_ids = HashSet::with_capacity(choices.len());
    let mut assignment_ids = HashSet::with_capacity(choices.len());
    let mut permission_ids = HashSet::with_capacity(choices.len());
    for choice in choices {
        if !pretasks.insert(choice.pretask_id)
            || !task_ids.insert(choice.task_id)
            || !resource_ids.insert(choice.task_resource_node_id)
            || !assignment_ids.insert(choice.assignment_id)
            || !permission_ids.insert(choice.permission_grant_id)
            || expected.get(&choice.pretask_id).map(String::as_str)
                != Some(task_kind_str(choice.task_kind))
            || choice.envelopes.is_empty()
        {
            return Err(AppError::BadRequest(
                "materialization choice is missing, duplicated, incompatible, or uncovered",
            ));
        }
        opaque_payload(&choice.selected_value_snapshot)?;
        opaque_payload(&choice.task_snapshot)?;
        opaque_payload(&choice.encrypted_assignment)?;
    }
    Ok(())
}

async fn require_assignment_recipient(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor_id: Uuid,
    assignee_id: Uuid,
    list_resource_id: Uuid,
) -> Result<(), AppError> {
    let actor_role = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_memberships
         WHERE project_id = $1 AND identity_id = $2 AND state = 'active'",
    )
    .bind(project_id)
    .bind(actor_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let assignee_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM project_memberships
            WHERE project_id = $1 AND identity_id = $2 AND state = 'active'
        )",
    )
    .bind(project_id)
    .bind(assignee_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !assignee_exists {
        return Err(AppError::BadRequest(
            "assignee is not an active project member",
        ));
    }
    if matches!(actor_role.as_str(), "owner" | "admin") {
        return Ok(());
    }
    let assignee_has_list = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1
            FROM sprout_private.effective_domain_permission($1, $2, $3)
            WHERE access_scope = 'full'
        )",
    )
    .bind(project_id)
    .bind(list_resource_id)
    .bind(assignee_id)
    .fetch_one(&mut **transaction)
    .await?;
    if assignee_has_list {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn require_assignment_materializer(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor_id: Uuid,
    assigned_by: Uuid,
    assigned_to: Uuid,
    list_resource_id: Uuid,
) -> Result<(), AppError> {
    if actor_id == assigned_by || actor_id == assigned_to {
        return Ok(());
    }
    let can_manage = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM projects project
            JOIN project_memberships membership
              ON membership.project_id = project.id
             AND membership.identity_id = $2
             AND membership.state = 'active'
            WHERE project.id = $1
              AND (
                  project.owner_identity_id = $2
                  OR membership.role = 'admin'
                  OR EXISTS (
                      SELECT 1
                      FROM sprout_private.effective_domain_permission($1, $3, $2)
                      WHERE access_scope = 'full' AND access_level = 'manage'
                  )
              )
        )
        "#,
    )
    .bind(project_id)
    .bind(actor_id)
    .bind(list_resource_id)
    .fetch_one(&mut **transaction)
    .await?;
    if can_manage {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn validate_task_kind_shape(
    kind: TaskKindDto,
    series_id: Option<Uuid>,
    occurrence_number: Option<u64>,
) -> Result<(), AppError> {
    match (kind, series_id, occurrence_number) {
        (TaskKindDto::Recurring, Some(_), Some(number)) if number > 0 => Ok(()),
        (TaskKindDto::Priority | TaskKindDto::Deadline, None, None) => Ok(()),
        _ => Err(AppError::BadRequest(
            "task kind and recurrence fields are incompatible",
        )),
    }
}

fn task_kind_str(kind: TaskKindDto) -> &'static str {
    match kind {
        TaskKindDto::Priority => "priority",
        TaskKindDto::Deadline => "deadline",
        TaskKindDto::Recurring => "recurring",
    }
}

fn parse_task_kind(kind: &str) -> Result<TaskKindDto, AppError> {
    match kind {
        "priority" => Ok(TaskKindDto::Priority),
        "deadline" => Ok(TaskKindDto::Deadline),
        "recurring" => Ok(TaskKindDto::Recurring),
        _ => Err(AppError::Internal),
    }
}

pub(crate) fn opaque_payload(payload: &EncryptedPayloadDto) -> Result<Vec<u8>, AppError> {
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

fn to_i64(value: u64) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| AppError::BadRequest("numeric value is too large"))
}

fn i64_to_u64(value: i64) -> Result<u64, AppError> {
    u64::try_from(value).map_err(|_| AppError::Internal)
}

const TASK_SELECT: &str = r#"
SELECT
    task.id, task.project_id, task.task_list_id, task.resource_node_id,
    task.task_kind,
    CASE WHEN permission.access_scope = 'full'
        OR resource.created_by_identity_id =
            NULLIF(current_setting('app.identity_id', true), '')::uuid
        THEN task.encrypted_payload END AS encrypted_payload,
    task.encrypted_header,
    CASE WHEN permission.access_scope = 'full'
        OR resource.created_by_identity_id =
            NULLIF(current_setting('app.identity_id', true), '')::uuid
        THEN task.encrypted_value_snapshot END AS encrypted_value_snapshot,
    task.state, task.source_pretask_id, task.preset_assignment_id,
    task.copied_from_task_id, task.questionnaire_version_id,
    task.recurrence_series_id,
    task.occurrence_number, task.created_at, task.payload_version,
    task.completed_by_identity_id, task.completed_at,
    task.key_epoch,
    (
        SELECT assignment.id
        FROM task_assignments assignment
        WHERE assignment.project_id = task.project_id
          AND assignment.task_id = task.id
          AND assignment.revoked_at IS NULL
        ORDER BY assignment.assigned_at DESC, assignment.id
        LIMIT 1
    ) AS active_assignment_id,
    (
        SELECT assignment.assignee_identity_id
        FROM task_assignments assignment
        WHERE assignment.project_id = task.project_id
          AND assignment.task_id = task.id
          AND assignment.revoked_at IS NULL
        ORDER BY assignment.assigned_at DESC, assignment.id
        LIMIT 1
    ) AS active_assignee_identity_id
FROM tasks task
JOIN resource_nodes resource
  ON resource.project_id = task.project_id
 AND resource.id = task.resource_node_id
LEFT JOIN LATERAL sprout_private.effective_domain_permission(
    task.project_id,
    task.resource_node_id,
    NULLIF(current_setting('app.identity_id', true), '')::uuid
) permission ON true
"#;

const TASK_RETURNING: &str = r#"
id, project_id, task_list_id, resource_node_id,
task_kind, encrypted_payload, encrypted_value_snapshot,
state, source_pretask_id, preset_assignment_id,
copied_from_task_id, questionnaire_version_id, recurrence_series_id,
occurrence_number, created_at, payload_version,
completed_by_identity_id, completed_at,
key_epoch,
NULL::uuid AS active_assignment_id,
NULL::uuid AS active_assignee_identity_id
"#;

#[derive(FromRow)]
struct TopicRow {
    id: Uuid,
    project_id: Uuid,
    resource_node_id: Uuid,
    encrypted_payload: Option<Vec<u8>>,
    #[sqlx(default)]
    encrypted_header: Option<Vec<u8>>,
    payload_version: i64,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    key_epoch: i32,
}

impl TryFrom<TopicRow> for TopicDto {
    type Error = AppError;

    fn try_from(row: TopicRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            resource_node_id: row.resource_node_id,
            payload: row
                .encrypted_payload
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            header: row
                .encrypted_header
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            payload_version: i64_to_u64(row.payload_version)?,
            key_epoch: u32::try_from(row.key_epoch).map_err(|_| AppError::Internal)?,
            created_at: row.created_at,
            deleted_at: row.deleted_at,
        })
    }
}

#[derive(FromRow)]
struct TaskListRow {
    id: Uuid,
    project_id: Uuid,
    topic_id: Uuid,
    resource_node_id: Uuid,
    encrypted_payload: Option<Vec<u8>>,
    #[sqlx(default)]
    encrypted_header: Option<Vec<u8>>,
    payload_version: i64,
    created_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
    key_epoch: i32,
}

impl TryFrom<TaskListRow> for TaskListDto {
    type Error = AppError;

    fn try_from(row: TaskListRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            topic_id: row.topic_id,
            resource_node_id: row.resource_node_id,
            payload: row
                .encrypted_payload
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            header: row
                .encrypted_header
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            payload_version: i64_to_u64(row.payload_version)?,
            key_epoch: u32::try_from(row.key_epoch).map_err(|_| AppError::Internal)?,
            created_at: row.created_at,
            archived_at: row.archived_at,
        })
    }
}

#[derive(FromRow)]
struct TaskRow {
    id: Uuid,
    project_id: Uuid,
    task_list_id: Uuid,
    resource_node_id: Uuid,
    task_kind: String,
    encrypted_payload: Option<Vec<u8>>,
    #[sqlx(default)]
    encrypted_header: Option<Vec<u8>>,
    encrypted_value_snapshot: Option<Vec<u8>>,
    state: String,
    source_pretask_id: Option<Uuid>,
    preset_assignment_id: Option<Uuid>,
    copied_from_task_id: Option<Uuid>,
    questionnaire_version_id: Option<Uuid>,
    recurrence_series_id: Option<Uuid>,
    occurrence_number: Option<i64>,
    created_at: DateTime<Utc>,
    payload_version: i64,
    completed_by_identity_id: Option<Uuid>,
    completed_at: Option<DateTime<Utc>>,
    key_epoch: i32,
    active_assignment_id: Option<Uuid>,
    active_assignee_identity_id: Option<Uuid>,
}

impl TryFrom<TaskRow> for TaskDto {
    type Error = AppError;

    fn try_from(row: TaskRow) -> Result<Self, Self::Error> {
        let state = match (
            row.state.as_str(),
            row.completed_by_identity_id,
            row.completed_at,
        ) {
            ("open", None, None) => TaskStateDto::Open,
            ("completed", Some(completed_by), Some(completed_at)) => TaskStateDto::Completed {
                completed_by,
                completed_at,
            },
            _ => return Err(AppError::Internal),
        };
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            list_id: row.task_list_id,
            resource_node_id: row.resource_node_id,
            task_kind: parse_task_kind(&row.task_kind)?,
            payload: row
                .encrypted_payload
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            header: row
                .encrypted_header
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            selected_value_snapshot: row
                .encrypted_value_snapshot
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            key_epoch: u32::try_from(row.key_epoch).map_err(|_| AppError::Internal)?,
            state,
            source_pretask_id: row.source_pretask_id,
            preset_assignment_id: row.preset_assignment_id,
            copied_from_task_id: row.copied_from_task_id,
            questionnaire_version_id: row.questionnaire_version_id,
            recurrence_series_id: row.recurrence_series_id,
            occurrence_number: row.occurrence_number.map(i64_to_u64).transpose()?,
            active_assignment_id: row.active_assignment_id,
            active_assignee_identity_id: row.active_assignee_identity_id,
            created_at: row.created_at,
            payload_version: i64_to_u64(row.payload_version)?,
        })
    }
}

#[derive(FromRow)]
struct CompletionRow {
    task_id: Uuid,
    request_hash: Option<Vec<u8>>,
    recurrence_series_id: Option<Uuid>,
    occurrence_number: Option<i64>,
    next_task_id: Option<Uuid>,
}

#[derive(FromRow)]
struct RecurrenceRow {
    id: Uuid,
    project_id: Uuid,
    task_list_id: Uuid,
    encrypted_rule: Vec<u8>,
    payload_version: i64,
    state: String,
    created_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

impl TryFrom<RecurrenceRow> for RecurrenceSeriesDto {
    type Error = AppError;

    fn try_from(row: RecurrenceRow) -> Result<Self, Self::Error> {
        let state = match (row.state.as_str(), row.archived_at) {
            ("active", None) => RecurrenceStateDto::Active,
            ("archived", Some(archived_at)) => RecurrenceStateDto::Archived { archived_at },
            _ => return Err(AppError::Internal),
        };
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            list_id: row.task_list_id,
            encrypted_rule: payload_from_bytes(&row.encrypted_rule)?,
            state,
            payload_version: i64_to_u64(row.payload_version)?,
            created_at: row.created_at,
        })
    }
}

#[derive(FromRow)]
struct PresetRow {
    id: Uuid,
    project_id: Uuid,
    encrypted_metadata: Vec<u8>,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl TryFrom<PresetRow> for PresetDto {
    type Error = AppError;

    fn try_from(row: PresetRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            payload: payload_from_bytes(&row.encrypted_metadata)?,
            created_at: row.created_at,
            deleted_at: row.deleted_at,
        })
    }
}

#[derive(FromRow)]
struct PresetVersionRow {
    id: Uuid,
    project_id: Uuid,
    preset_id: Uuid,
    version_number: i32,
    encrypted_payload: Vec<u8>,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct PretaskRow {
    id: Uuid,
    task_kind: String,
    encrypted_payload: Vec<u8>,
}

impl TryFrom<PretaskRow> for PretaskDto {
    type Error = AppError;

    fn try_from(row: PretaskRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            task_kind: parse_task_kind(&row.task_kind)?,
            payload: payload_from_bytes(&row.encrypted_payload)?,
        })
    }
}

#[derive(FromRow)]
struct PresetAssignmentRow {
    id: Uuid,
    preset_version_id: Uuid,
    destination_task_list_id: Uuid,
    assigned_to_identity_id: Uuid,
    assigned_by_identity_id: Uuid,
    payload_version: i64,
    state: String,
    created_at: DateTime<Utc>,
    materialized_at: Option<DateTime<Utc>>,
}

impl From<PresetAssignmentRow> for PresetAssignmentDto {
    fn from(row: PresetAssignmentRow) -> Self {
        Self {
            id: row.id,
            preset_version_id: row.preset_version_id,
            destination_list_id: row.destination_task_list_id,
            assigned_to_identity_id: row.assigned_to_identity_id,
            assigned_by_identity_id: row.assigned_by_identity_id,
            payload_version: u64::try_from(row.payload_version)
                .expect("preset assignment payload version must be non-negative"),
            state: row.state,
            created_at: row.created_at,
            materialized_at: row.materialized_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encrypted(ciphertext: &str) -> EncryptedPayloadDto {
        EncryptedPayloadDto {
            version: 1,
            algorithm: "xchacha20poly1305".into(),
            key_id: "opaque-key".into(),
            nonce_b64: base64::engine::general_purpose::STANDARD.encode([1; 24]),
            ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
        }
    }

    #[test]
    fn llr_03_1_task_kind_validation_never_inspects_plaintext() {
        for (kind, series, occurrence, valid) in [
            (TaskKindDto::Priority, None, None, true),
            (TaskKindDto::Deadline, None, None, true),
            (TaskKindDto::Recurring, Some(Uuid::new_v4()), Some(1), true),
            (TaskKindDto::Recurring, None, None, false),
            (TaskKindDto::Priority, Some(Uuid::new_v4()), Some(1), false),
        ] {
            assert_eq!(
                validate_task_kind_shape(kind, series, occurrence).is_ok(),
                valid
            );
        }
        assert!(opaque_payload(&encrypted("not plaintext inspected")).is_ok());
    }

    #[test]
    fn llr_03_2_missing_and_incompatible_pretask_choices_fail_before_writes() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let expected = HashMap::from([
            (first, "priority".to_owned()),
            (second, "deadline".to_owned()),
        ]);
        let missing = vec![sprout_api_contract::PretaskSelectionDto {
            pretask_id: first,
            task_kind: TaskKindDto::Priority,
            selected_value: encrypted("one"),
        }];
        assert!(validate_selections(&expected, &missing).is_err());

        let incompatible = vec![
            sprout_api_contract::PretaskSelectionDto {
                pretask_id: first,
                task_kind: TaskKindDto::Priority,
                selected_value: encrypted("one"),
            },
            sprout_api_contract::PretaskSelectionDto {
                pretask_id: second,
                task_kind: TaskKindDto::Priority,
                selected_value: encrypted("two"),
            },
        ];
        assert!(validate_selections(&expected, &incompatible).is_err());
    }
}
