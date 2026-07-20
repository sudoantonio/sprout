use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sprout_api_contract::{
    AssignTaskRequest, AssignmentDto, AssignmentResponse, ListAssignmentsResponse,
    RevokeAssignmentRequest,
};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::permissions::{
    expected_resource_scopes, rotate_resource_keys, validate_and_store_current_envelopes,
};
use crate::{
    AppState,
    auth::{AuthSession, ResourceAccess, require_resource_access, set_database_context},
    error::AppError,
};

pub async fn assign(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<AssignTaskRequest>,
) -> Result<Json<AssignmentResponse>, AppError> {
    let (task_resource_id, list_resource_id) =
        task_resource_ids(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        task_resource_id,
        ResourceAccess::Manage,
    )
    .await?;
    let encrypted_payload = decode(&request.encrypted_payload_b64)?;
    if encrypted_payload.is_empty() {
        return Err(AppError::BadRequest(
            "encrypted assignment payload is empty",
        ));
    }

    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    require_assignment_recipient_access(
        &mut transaction,
        project_id,
        actor.identity_id,
        request.assignee_identity_id,
        list_resource_id,
    )
    .await?;
    let scoped_resources =
        expected_resource_scopes(&mut transaction, project_id, task_resource_id, "full").await?;
    let resources = scoped_resources
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let body_resources = scoped_resources
        .iter()
        .filter_map(|(id, scope)| (scope == "full").then_some(*id))
        .collect::<Vec<_>>();
    validate_and_store_current_envelopes(
        &mut transaction,
        actor,
        project_id,
        request.assignee_identity_id,
        &resources,
        &body_resources,
        &request.envelopes,
    )
    .await?;

    let row = sqlx::query_as::<_, AssignmentRow>(
        r#"
        INSERT INTO task_assignments (
            id, project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, encrypted_payload,
            permission_root_grant_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING
            id, project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, permission_root_grant_id,
            assigned_at, revoked_at
        "#,
    )
    .bind(request.assignment_id)
    .bind(project_id)
    .bind(task_id)
    .bind(request.assignee_identity_id)
    .bind(actor.identity_id)
    .bind(encrypted_payload)
    .bind(request.permission_grant_id)
    .fetch_one(&mut *transaction)
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
    .bind(request.assignee_identity_id)
    .bind(request.permission_grant_id)
    .bind(actor.identity_id)
    .bind(request.assignment_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(AssignmentResponse {
        assignment: row.into(),
    }))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListAssignmentsResponse>, AppError> {
    let (task_resource_id, _) = task_resource_ids(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        task_resource_id,
        ResourceAccess::Manage,
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
    let rows = sqlx::query_as::<_, AssignmentRow>(
        r#"
        SELECT
            id, project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, permission_root_grant_id,
            assigned_at, revoked_at
        FROM task_assignments
        WHERE project_id = $1 AND task_id = $2
        ORDER BY assigned_at, id
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_all(&mut *transaction)
    .await?;
    let active_assignment_id = rows
        .iter()
        .find(|assignment| assignment.revoked_at.is_none())
        .map(|assignment| assignment.id);
    let assignments = rows.into_iter().map(Into::into).collect();
    transaction.commit().await?;
    Ok(Json(ListAssignmentsResponse {
        assignments,
        active_assignment_id,
    }))
}

pub async fn revoke(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id, assignment_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RevokeAssignmentRequest>,
) -> Result<Json<AssignmentResponse>, AppError> {
    let (task_resource_id, _) = task_resource_ids(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        task_resource_id,
        ResourceAccess::Manage,
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
    let mut row = sqlx::query_as::<_, AssignmentRow>(
        r#"
        SELECT
            id, project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, permission_root_grant_id,
            assigned_at, revoked_at
        FROM task_assignments
        WHERE project_id = $1
          AND task_id = $2
          AND id = $3
          AND revoked_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .bind(assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let affected = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT resource_node_id
        FROM sprout_private.permission_lineage_resources($1, $2, $3)
        ORDER BY resource_node_id
        "#,
    )
    .bind(project_id)
    .bind(row.permission_root_grant_id)
    .bind(row.assignee_identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    rotate_resource_keys(
        &mut transaction,
        actor,
        project_id,
        row.permission_root_grant_id,
        &affected,
        &request.rotations,
    )
    .await?;
    let revoked_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        UPDATE task_assignments
        SET revoked_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2 AND revoked_at IS NULL
        RETURNING revoked_at
        "#,
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query("SELECT sprout_private.revoke_hierarchical_permission($1, $2, $3, $4, NULL)")
        .bind(project_id)
        .bind(row.permission_root_grant_id)
        .bind(row.assignee_identity_id)
        .bind(actor.identity_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    row.revoked_at = Some(revoked_at);
    Ok(Json(AssignmentResponse {
        assignment: row.into(),
    }))
}

async fn task_resource_ids(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<(Uuid, Uuid), AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let ids = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT task.resource_node_id, task_list.resource_node_id
        FROM tasks task
        JOIN task_lists task_list
          ON task_list.project_id = task.project_id
         AND task_list.id = task.task_list_id
         AND task_list.deleted_at IS NULL
        WHERE task.project_id = $1
          AND task.id = $2
          AND task.deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(ids)
}

async fn require_assignment_recipient_access(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor_identity_id: Uuid,
    assignee_identity_id: Uuid,
    list_resource_id: Uuid,
) -> Result<(), AppError> {
    let actor_role = sqlx::query_scalar::<_, String>(
        r#"
        SELECT role
        FROM project_memberships
        WHERE project_id = $1
          AND identity_id = $2
          AND state = 'active'
        "#,
    )
    .bind(project_id)
    .bind(actor_identity_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let assignee_is_member = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM project_memberships
            WHERE project_id = $1
              AND identity_id = $2
              AND state = 'active'
        )
        "#,
    )
    .bind(project_id)
    .bind(assignee_identity_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !assignee_is_member {
        return Err(AppError::BadRequest(
            "assignment recipient is not an active project member",
        ));
    }
    if matches!(actor_role.as_str(), "owner" | "admin") {
        return Ok(());
    }
    let has_full_list_access = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM sprout_private.effective_domain_permission($1, $2, $3)
            WHERE access_scope = 'full'
        )
        "#,
    )
    .bind(project_id)
    .bind(list_resource_id)
    .bind(assignee_identity_id)
    .fetch_one(&mut **transaction)
    .await?;
    if has_full_list_access {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[derive(FromRow)]
struct AssignmentRow {
    id: Uuid,
    project_id: Uuid,
    task_id: Uuid,
    assignee_identity_id: Uuid,
    assigned_by_identity_id: Uuid,
    permission_root_grant_id: Uuid,
    assigned_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl From<AssignmentRow> for AssignmentDto {
    fn from(row: AssignmentRow) -> Self {
        Self {
            id: row.id,
            project_id: row.project_id,
            task_id: row.task_id,
            assignee_identity_id: row.assignee_identity_id,
            assigned_by_identity_id: row.assigned_by_identity_id,
            permission_root_grant_id: row.permission_root_grant_id,
            assigned_at: row.assigned_at,
            revoked_at: row.revoked_at,
        }
    }
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 encrypted payload"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_payloads_remain_opaque() {
        assert_eq!(decode("AQI=").unwrap(), vec![1, 2]);
        assert!(decode("not base64!").is_err());
    }
}
