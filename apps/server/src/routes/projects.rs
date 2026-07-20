use std::{fmt, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use super::email::{checked_token_hash, enqueue_email, hash_token, new_token, normalize_email};
use crate::{
    AppState,
    auth::{AuthSession, ProjectAccess, require_project_access, set_database_context},
    error::AppError,
};

#[derive(Deserialize)]
pub struct CreateProject {
    id: Uuid,
    encrypted_metadata_b64: String,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Json(request): Json<CreateProject>,
) -> Result<Json<ProjectView>, AppError> {
    let encrypted_metadata = decode(&request.encrypted_metadata_b64)?;
    if encrypted_metadata.is_empty() {
        return Err(AppError::BadRequest("encrypted project metadata is empty"));
    }
    let root_id = Uuid::new_v4();
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(request.id),
    )
    .await?;
    let row = sqlx::query_as::<_, ProjectRow>(
        r#"
        INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
        VALUES ($1, $2, $3)
        RETURNING
            id, owner_identity_id, encrypted_metadata, status,
            created_at, updated_at, key_epoch, $4::uuid AS root_resource_id
        "#,
    )
    .bind(request.id)
    .bind(actor.identity_id)
    .bind(&encrypted_metadata)
    .bind(root_id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO project_memberships (project_id, identity_id, role, state)
        VALUES ($1, $2, 'owner', 'active')
        "#,
    )
    .bind(request.id)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO resource_nodes (
            id, project_id, node_kind, encrypted_metadata, created_by_identity_id
        )
        VALUES ($1, $2, 'root', $3, $4)
        "#,
    )
    .bind(root_id)
    .bind(request.id)
    .bind(encrypted_metadata)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ProjectView::from(row)))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
) -> Result<Json<Vec<ProjectView>>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    let projects = sqlx::query_as::<_, ProjectRow>(
        r#"
        SELECT
            project.id, project.owner_identity_id, project.encrypted_metadata,
            project.status, project.created_at, project.updated_at,
            project.key_epoch,
            (
                SELECT node.id
                FROM resource_nodes node
                WHERE node.project_id = project.id
                  AND node.node_kind = 'root'
                  AND node.deleted_at IS NULL
                ORDER BY node.created_at, node.id
                LIMIT 1
            ) AS root_resource_id
        FROM projects project
        WHERE project.deleted_at IS NULL
        ORDER BY project.created_at, project.id
        LIMIT 500
        "#,
    )
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(projects.into_iter().map(ProjectView::from).collect()))
}

pub async fn get_project(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ProjectView>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let row = sqlx::query_as::<_, ProjectRow>(
        r#"
        SELECT
            project.id, project.owner_identity_id, project.encrypted_metadata,
            project.status, project.created_at, project.updated_at,
            project.key_epoch,
            (
                SELECT node.id
                FROM resource_nodes node
                WHERE node.project_id = project.id
                  AND node.node_kind = 'root'
                  AND node.deleted_at IS NULL
                ORDER BY node.created_at, node.id
                LIMIT 1
            ) AS root_resource_id
        FROM projects project
        WHERE project.id = $1 AND project.deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(Json(ProjectView::from(row)))
}

#[derive(FromRow)]
struct ProjectRow {
    id: Uuid,
    owner_identity_id: Uuid,
    encrypted_metadata: Vec<u8>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    key_epoch: i32,
    root_resource_id: Uuid,
}

#[derive(Serialize)]
pub struct ProjectView {
    id: Uuid,
    root_resource_id: Uuid,
    owner_identity_id: Uuid,
    encrypted_metadata_b64: String,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    key_epoch: i32,
}

impl From<ProjectRow> for ProjectView {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: row.id,
            root_resource_id: row.root_resource_id,
            owner_identity_id: row.owner_identity_id,
            encrypted_metadata_b64: base64::engine::general_purpose::STANDARD
                .encode(row.encrypted_metadata),
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
            key_epoch: row.key_epoch,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateInvitation {
    invitee_email: String,
    encrypted_payload_b64: String,
    role: InvitationRole,
    expires_in_seconds: u32,
}

impl fmt::Debug for CreateInvitation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateInvitation")
            .field("invitee_email", &"[REDACTED]")
            .field("encrypted_payload_b64", &"[REDACTED]")
            .field("role", &self.role)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum InvitationRole {
    Admin,
    Member,
    Guest,
}

impl InvitationRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Guest => "guest",
        }
    }
}

pub async fn create_invitation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateInvitation>,
) -> Result<Json<InvitationCreated>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    if !(300..=2_592_000).contains(&request.expires_in_seconds) {
        return Err(AppError::BadRequest("invitation expiry is out of range"));
    }
    let normalized_email = normalize_email(&request.invitee_email)?;
    let lookup_hash = Sha256::digest(normalized_email.as_bytes()).to_vec();
    let encrypted_payload = decode(&request.encrypted_payload_b64)?;
    if encrypted_payload.is_empty() {
        return Err(AppError::BadRequest("invalid invitation payload"));
    }
    let invitation_id = Uuid::new_v4();
    let token = new_token();
    let token_hash = hash_token(&token);
    let expires_at = Utc::now() + Duration::seconds(i64::from(request.expires_in_seconds));
    let email_payload = InvitationEmailPayload {
        project_id,
        invitation_id,
        token: &token,
    };
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
        INSERT INTO project_invitations (
            id, project_id, invited_by_identity_id, invitee_lookup_hash,
            token_hash, encrypted_payload, role, expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(invitation_id)
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(lookup_hash)
    .bind(token_hash.as_slice())
    .bind(encrypted_payload)
    .bind(request.role.as_str())
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;
    enqueue_email(
        &mut transaction,
        &state.config,
        actor.identity_id,
        "project_invitation",
        &normalized_email,
        &token_hash,
        &email_payload,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(InvitationCreated {
        id: invitation_id,
        expires_at,
    }))
}

#[derive(Serialize)]
pub struct InvitationCreated {
    id: Uuid,
    expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct InvitationEmailPayload<'a> {
    project_id: Uuid,
    invitation_id: Uuid,
    token: &'a str,
}

pub async fn list_invitations(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<InvitationView>>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let rows = sqlx::query_as::<_, InvitationRow>(
        r#"
        SELECT
            invitation.id,
            invitation.role,
            invitation.state,
            invitation.accepted_by_identity_id,
            invitation.created_at,
            invitation.expires_at,
            EXISTS (
                SELECT 1
                FROM resource_nodes root
                JOIN resource_key_envelopes envelope
                  ON envelope.project_id = root.project_id
                 AND envelope.resource_node_id = root.id
                 AND envelope.recipient_identity_id =
                     invitation.accepted_by_identity_id
                 AND envelope.revoked_at IS NULL
                WHERE root.project_id = invitation.project_id
                  AND root.node_kind = 'root'
                  AND root.deleted_at IS NULL
            ) AS keys_shared
        FROM project_invitations invitation
        WHERE invitation.project_id = $1
        ORDER BY created_at DESC
        LIMIT 500
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(rows.into_iter().map(InvitationView::from).collect()))
}

#[derive(FromRow)]
struct InvitationRow {
    id: Uuid,
    role: String,
    state: String,
    accepted_by_identity_id: Option<Uuid>,
    keys_shared: bool,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct InvitationView {
    id: Uuid,
    role: String,
    state: String,
    accepted_by_identity_id: Option<Uuid>,
    keys_shared: bool,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<InvitationRow> for InvitationView {
    fn from(row: InvitationRow) -> Self {
        Self {
            id: row.id,
            role: row.role,
            state: row.state,
            accepted_by_identity_id: row.accepted_by_identity_id,
            keys_shared: row.keys_shared,
            created_at: row.created_at,
            expires_at: row.expires_at,
        }
    }
}

#[derive(Deserialize)]
pub struct AcceptInvitation {
    invitation_id: Uuid,
    token: String,
}

#[derive(Serialize)]
pub struct InvitationAccepted {
    accepted: bool,
}

pub async fn accept_invitation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<AcceptInvitation>,
) -> Result<Json<InvitationAccepted>, AppError> {
    let token_hash = checked_token_hash(&request.token)?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let accepted = sqlx::query_scalar::<_, bool>(
        "SELECT sprout_private.accept_project_invitation($1, $2, $3, $4)",
    )
    .bind(project_id)
    .bind(request.invitation_id)
    .bind(token_hash.as_slice())
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if !accepted {
        return Err(AppError::BadRequest(
            "invitation token is invalid or expired",
        ));
    }
    Ok(Json(InvitationAccepted { accepted: true }))
}

#[derive(Deserialize)]
pub struct SuggestParticipants {
    prefix: String,
    #[serde(default = "default_suggestion_limit")]
    limit: u16,
}

impl fmt::Debug for SuggestParticipants {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SuggestParticipants")
            .field("prefix", &"[REDACTED]")
            .field("limit", &self.limit)
            .finish()
    }
}

fn default_suggestion_limit() -> u16 {
    20
}

pub async fn participant_suggestions(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<SuggestParticipants>,
) -> Result<Json<Vec<ParticipantSuggestion>>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    if request.prefix.len() > 128 || request.limit == 0 || request.limit > 50 {
        return Err(AppError::BadRequest("invalid participant suggestion query"));
    }
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let rows = sqlx::query_as::<_, (Uuid, String, i64, DateTime<Utc>)>(
        r#"
        SELECT
            identity_id,
            identity_handle,
            shared_project_count,
            most_recent_shared_project_at
        FROM sprout_private.suggest_project_participants($1, $2, $3)
        "#,
    )
    .bind(project_id)
    .bind(&request.prefix)
    .bind(i64::from(request.limit))
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    identity_id,
                    identity_handle,
                    shared_project_count,
                    most_recent_shared_project_at,
                )| ParticipantSuggestion {
                    identity_id,
                    identity_handle,
                    shared_project_count,
                    most_recent_shared_project_at,
                },
            )
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct ParticipantSuggestion {
    identity_id: Uuid,
    identity_handle: String,
    shared_project_count: i64,
    most_recent_shared_project_at: DateTime<Utc>,
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 payload"))
}
