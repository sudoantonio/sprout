use std::{collections::HashSet, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sprout_api_contract::{
    AccessScopeDto, GrantOriginDto, GrantPermissionRequest, GrantPermissionResponse,
    InitializeResourceEpochRequest, ListPermissionsResponse, ListResourceKeyEnvelopesResponse,
    PermissionAccessLevelDto, PermissionGrantDto, ResourceEnvelopePlanItemDto,
    ResourceEnvelopePlanResponse, ResourceEpochRotationDto, ResourceKeyEnvelopeDto,
    ResourceKeyEnvelopeViewDto, ResourceKeyPurposeDto, ResourceRotationPlanItemDto,
    ResourceRotationPlanResponse, RevokePermissionRequest, ShareMemberResourceKeysRequest,
};
use sprout_crypto_protocol::verify_ed25519_ml_dsa65_signatures;
use sprout_storage_postgres::{
    ActiveDeviceKey, ActiveResourceEpoch, ResourceKeyEnvelopeInput, validate_envelope_coverage,
    validate_envelope_coverage_for_purpose, validate_experimental_wrapped_resource_key,
};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        AuthSession, ProjectAccess, ResourceAccess, require_project_access,
        require_resource_access, set_database_context,
    },
    error::AppError,
};

pub async fn list_recipient_envelopes(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ListResourceKeyEnvelopesResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let rows = sqlx::query_as::<_, RecipientEnvelopeRow>(
        r#"
        SELECT
            envelope.envelope_version,
            envelope.resource_node_id,
            envelope.epoch,
            envelope.key_purpose,
            envelope.recipient_identity_id,
            envelope.recipient_device_id,
            envelope.recipient_device_key_version,
            envelope.created_by_device_key_version AS sender_device_key_version,
            envelope.encrypted_key,
            envelope.sender_signature,
            envelope.sender_post_quantum_signature,
            envelope.created_by_identity_id AS sender_identity_id,
            envelope.created_by_device_id AS sender_device_id,
            previous.key_commitment AS previous_epoch_hash
        FROM resource_key_envelopes envelope
        JOIN resource_epochs epoch
          ON epoch.project_id = envelope.project_id
         AND epoch.resource_node_id = envelope.resource_node_id
         AND epoch.epoch = envelope.epoch
         AND epoch.retired_at IS NULL
        LEFT JOIN resource_epochs previous
          ON previous.project_id = epoch.project_id
         AND previous.id = epoch.previous_epoch_id
        WHERE envelope.project_id = $1
          AND envelope.recipient_identity_id = $2
          AND envelope.recipient_device_id = $3
          AND envelope.revoked_at IS NULL
        ORDER BY envelope.resource_node_id, envelope.epoch
        LIMIT 5000
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ListResourceKeyEnvelopesResponse {
        envelopes: rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    }))
}

pub async fn full_envelope_plan(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, resource_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ResourceEnvelopePlanResponse>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
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
    let expected = expected_resources(&mut transaction, project_id, resource_id, "full").await?;
    let rows = sqlx::query_as::<_, (Uuid, Uuid, i32, Vec<u8>, Option<Vec<u8>>)>(
        r#"
        SELECT
            active.resource_node_id, active.id, active.epoch,
            active.key_commitment, previous.key_commitment
        FROM resource_epochs active
        LEFT JOIN resource_epochs previous
          ON previous.project_id = active.project_id
         AND previous.id = active.previous_epoch_id
        WHERE active.project_id = $1
          AND active.resource_node_id = ANY($2)
          AND active.retired_at IS NULL
        ORDER BY active.resource_node_id
        "#,
    )
    .bind(project_id)
    .bind(&expected)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if rows.len() != expected.len() {
        return Err(AppError::BadRequest(
            "every shared resource requires an active key epoch",
        ));
    }
    Ok(Json(ResourceEnvelopePlanResponse {
        resources: rows
            .into_iter()
            .map(
                |(resource_id, epoch_id, epoch, key_commitment, previous_epoch_hash)| {
                    Ok(ResourceEnvelopePlanItemDto {
                        resource_id,
                        epoch_id,
                        epoch: u32::try_from(epoch).map_err(|_| AppError::Internal)?,
                        key_commitment_b64: encode(&key_commitment),
                        previous_epoch_hash_b64: previous_epoch_hash.as_deref().map(encode),
                    })
                },
            )
            .collect::<Result<_, AppError>>()?,
    }))
}

pub async fn share_member_resource_keys(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ShareMemberResourceKeysRequest>,
) -> Result<StatusCode, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let resources = request.resource_ids.iter().copied().collect::<HashSet<_>>();
    if resources.is_empty() || resources.len() != request.resource_ids.len() {
        return Err(AppError::BadRequest(
            "resource key share requires unique resources",
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
    let authorized = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(DISTINCT node.id)
        FROM resource_nodes node
        JOIN project_memberships membership
          ON membership.project_id = node.project_id
         AND membership.identity_id = $3
         AND membership.state = 'active'
        WHERE node.project_id = $1
          AND node.id = ANY($2)
          AND node.deleted_at IS NULL
          AND (
              node.node_kind = 'root'
              OR membership.role IN ('owner', 'admin')
              OR EXISTS (
                  SELECT 1
                  FROM sprout_private.domain_permission_rows permission
                  WHERE permission.project_id = node.project_id
                    AND permission.resource_node_id = node.id
                    AND permission.member_identity_id = membership.identity_id
                    AND permission.revoked_at IS NULL
              )
          )
        "#,
    )
    .bind(project_id)
    .bind(&request.resource_ids)
    .bind(request.recipient_identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if usize::try_from(authorized).ok() != Some(resources.len()) {
        return Err(AppError::Forbidden);
    }
    validate_and_store_current_envelopes(
        &mut transaction,
        actor,
        project_id,
        request.recipient_identity_id,
        &request.resource_ids,
        &request.resource_ids,
        &request.envelopes,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(FromRow)]
struct RecipientEnvelopeRow {
    envelope_version: i16,
    resource_node_id: Uuid,
    epoch: i32,
    key_purpose: String,
    recipient_identity_id: Uuid,
    recipient_device_id: Uuid,
    recipient_device_key_version: i32,
    sender_device_key_version: i32,
    encrypted_key: Vec<u8>,
    sender_signature: Vec<u8>,
    sender_post_quantum_signature: Vec<u8>,
    sender_identity_id: Uuid,
    sender_device_id: Uuid,
    previous_epoch_hash: Option<Vec<u8>>,
}

impl TryFrom<RecipientEnvelopeRow> for ResourceKeyEnvelopeViewDto {
    type Error = AppError;

    fn try_from(row: RecipientEnvelopeRow) -> Result<Self, Self::Error> {
        Ok(Self {
            envelope: ResourceKeyEnvelopeDto {
                version: u16::try_from(row.envelope_version).map_err(|_| AppError::Internal)?,
                resource_id: row.resource_node_id,
                epoch: u32::try_from(row.epoch).map_err(|_| AppError::Internal)?,
                key_purpose: match row.key_purpose.as_str() {
                    "body" => ResourceKeyPurposeDto::Body,
                    "header" => ResourceKeyPurposeDto::Header,
                    _ => return Err(AppError::Internal),
                },
                recipient_identity_id: row.recipient_identity_id,
                recipient_device_id: row.recipient_device_id,
                recipient_device_key_version: u32::try_from(row.recipient_device_key_version)
                    .map_err(|_| AppError::Internal)?,
                sender_device_key_version: u32::try_from(row.sender_device_key_version)
                    .map_err(|_| AppError::Internal)?,
                encrypted_key_b64: encode(&row.encrypted_key),
                sender_signature_b64: encode(&row.sender_signature),
                sender_post_quantum_signature_b64: encode(&row.sender_post_quantum_signature),
            },
            sender_identity_id: row.sender_identity_id,
            sender_device_id: row.sender_device_id,
            previous_epoch_hash_b64: row.previous_epoch_hash.as_deref().map(encode),
        })
    }
}

pub async fn initialize_epoch(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, resource_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<InitializeResourceEpochRequest>,
) -> Result<StatusCode, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
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
    insert_initial_resource_epoch(
        &mut transaction,
        actor,
        project_id,
        resource_id,
        actor.identity_id,
        &request.epoch,
        &request.envelopes,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::CREATED)
}

pub(super) async fn insert_initial_resource_epoch(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    resource_id: Uuid,
    recipient_identity_id: Uuid,
    epoch: &sprout_api_contract::ResourceEpochInputDto,
    envelopes: &[ResourceKeyEnvelopeDto],
) -> Result<(), AppError> {
    if epoch.epoch != 1 {
        return Err(AppError::BadRequest(
            "new resources must begin at key epoch one",
        ));
    }
    let commitment = decode(&epoch.key_commitment_b64)?;
    if commitment.len() < 16 {
        return Err(AppError::BadRequest("key commitment is too short"));
    }
    let header_commitment = epoch
        .header_key_commitment_b64
        .as_deref()
        .map(decode)
        .transpose()?;
    if header_commitment
        .as_ref()
        .is_some_and(|value| value.len() < 16)
    {
        return Err(AppError::BadRequest("header key commitment is too short"));
    }
    sqlx::query(
        r#"
        INSERT INTO resource_epochs (
            id, project_id, resource_node_id, epoch,
            created_by_identity_id, created_by_device_id,
            created_by_device_key_version, key_commitment,
            header_key_commitment, reason
        )
        VALUES ($1, $2, $3, 1, $4, $5, $6, $7, $8, 'created')
        "#,
    )
    .bind(epoch.id)
    .bind(project_id)
    .bind(resource_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(
        i32::try_from(epoch.creator_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid device key version"))?,
    )
    .bind(commitment)
    .bind(header_commitment)
    .execute(&mut **transaction)
    .await?;
    validate_and_store_current_envelopes(
        transaction,
        actor,
        project_id,
        recipient_identity_id,
        &[resource_id],
        &[resource_id],
        envelopes,
    )
    .await
}

pub async fn grant(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, resource_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<GrantPermissionRequest>,
) -> Result<Json<GrantPermissionResponse>, AppError> {
    if request.resource_id != resource_id {
        return Err(AppError::BadRequest(
            "permission resource does not match path",
        ));
    }
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
        ResourceAccess::Manage,
    )
    .await?;

    let access_level = access_level_str(request.access_level);
    let access_scope = access_scope_str(request.access_scope);
    let visibility = visibility_str(&request.visibility)?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;

    let scoped_resources =
        expected_resource_scopes(&mut transaction, project_id, resource_id, access_scope).await?;
    let resources = scoped_resources
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let body_resources = scoped_resources
        .iter()
        .filter_map(|(id, scope)| (*scope == "full").then_some(*id))
        .collect::<Vec<_>>();
    validate_and_store_current_envelopes(
        &mut transaction,
        actor,
        project_id,
        request.user_id,
        &resources,
        &body_resources,
        &request.envelopes,
    )
    .await?;

    sqlx::query(
        r#"
        SELECT sprout_private.grant_hierarchical_permission(
            $1, $2, $3, $4, $5, $6, $7, $8, 'explicit', NULL
        )
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .bind(request.user_id)
    .bind(access_level)
    .bind(access_scope)
    .bind(visibility)
    .bind(request.grant_id)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    let granted_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        SELECT created_at
        FROM sprout_private.domain_permission_rows
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(project_id)
    .bind(request.grant_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Json(GrantPermissionResponse {
        grant: PermissionGrantDto {
            id: request.grant_id,
            root_grant_id: request.grant_id,
            user_id: request.user_id,
            resource_id,
            access_level: request.access_level,
            access_scope: request.access_scope,
            origin: GrantOriginDto::Direct,
            granted_at,
            revoked_at: None,
        },
    }))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, resource_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListPermissionsResponse>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
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
    let rows = sqlx::query_as::<_, PermissionRow>(
        r#"
        SELECT
            id, root_grant_id, member_identity_id, resource_node_id,
            access_level, access_scope, grant_origin, grant_origin_id,
            created_at, revoked_at
        FROM sprout_private.domain_permission_rows
        WHERE project_id = $1
          AND resource_node_id = $2
          AND id = root_grant_id
        ORDER BY created_at, id
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let grants = rows
        .into_iter()
        .map(permission_dto)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ListPermissionsResponse { grants }))
}

pub async fn revoke(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, resource_id, grant_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RevokePermissionRequest>,
) -> Result<Json<PermissionGrantDto>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
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
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2::text, 11))")
        .bind(project_id)
        .bind(request.user_id)
        .execute(&mut *transaction)
        .await?;
    let root = sqlx::query_as::<_, PermissionRow>(
        r#"
        SELECT
            id, root_grant_id, member_identity_id, resource_node_id,
            access_level, access_scope, grant_origin, grant_origin_id,
            created_at, revoked_at
        FROM sprout_private.domain_permission_rows
        WHERE project_id = $1
          AND id = $2
          AND root_grant_id = $2
          AND member_identity_id = $3
          AND resource_node_id = $4
          AND revoked_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(grant_id)
    .bind(request.user_id)
    .bind(resource_id)
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
    .bind(grant_id)
    .bind(request.user_id)
    .fetch_all(&mut *transaction)
    .await?;
    rotate_resource_keys(
        &mut transaction,
        actor,
        project_id,
        grant_id,
        &affected,
        &request.rotations,
    )
    .await?;

    let notification = request
        .encrypted_admin_notification_b64
        .as_deref()
        .map(decode)
        .transpose()?;
    sqlx::query("SELECT sprout_private.revoke_hierarchical_permission($1, $2, $3, $4, $5)")
        .bind(project_id)
        .bind(grant_id)
        .bind(request.user_id)
        .bind(actor.identity_id)
        .bind(notification)
        .execute(&mut *transaction)
        .await?;
    let revoked_at = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await?;
    transaction.commit().await?;

    let mut dto = permission_dto(root)?;
    dto.revoked_at = Some(revoked_at);
    Ok(Json(dto))
}

pub async fn rotation_plan(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, resource_id, grant_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<ResourceRotationPlanResponse>, AppError> {
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_id,
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
    let revoked_identity_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT member_identity_id
        FROM sprout_private.domain_permission_rows
        WHERE project_id = $1
          AND id = $2
          AND root_grant_id = $2
          AND resource_node_id = $3
          AND revoked_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(grant_id)
    .bind(resource_id)
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
    .bind(grant_id)
    .bind(revoked_identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    if affected.is_empty() {
        return Err(AppError::NotFound);
    }

    let mut resources = Vec::with_capacity(affected.len());
    for affected_resource_id in affected {
        let (
            previous_epoch_id,
            current_epoch,
            previous_key_commitment,
            previous_header_key_commitment,
        ) = sqlx::query_as::<_, (Uuid, i32, Vec<u8>, Option<Vec<u8>>)>(
            r#"
                SELECT id, epoch, key_commitment, header_key_commitment
                FROM resource_epochs
                WHERE project_id = $1
                  AND resource_node_id = $2
                  AND retired_at IS NULL
                "#,
        )
        .bind(project_id)
        .bind(affected_resource_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::BadRequest(
            "affected resource has no active key epoch",
        ))?;
        let remaining_recipients = remaining_resource_recipients(
            &mut transaction,
            project_id,
            affected_resource_id,
            grant_id,
        )
        .await?;
        let body_recipients = remaining_resource_body_recipients(
            &mut transaction,
            project_id,
            affected_resource_id,
            grant_id,
        )
        .await?;
        let mut recipient_identity_ids = Vec::new();
        for recipient_identity_id in remaining_recipients {
            if !active_device_keys(&mut transaction, project_id, recipient_identity_id)
                .await?
                .is_empty()
            {
                recipient_identity_ids.push(recipient_identity_id);
            }
        }
        recipient_identity_ids.sort_unstable();
        let mut body_recipient_identity_ids = body_recipients
            .into_iter()
            .filter(|identity_id| recipient_identity_ids.contains(identity_id))
            .collect::<Vec<_>>();
        body_recipient_identity_ids.sort_unstable();
        let header_recipient_identity_ids = if previous_header_key_commitment.is_some() {
            recipient_identity_ids.clone()
        } else {
            Vec::new()
        };
        resources.push(ResourceRotationPlanItemDto {
            resource_id: affected_resource_id,
            previous_epoch_id,
            current_epoch: u32::try_from(current_epoch).map_err(|_| AppError::Internal)?,
            previous_key_commitment_b64: encode(&previous_key_commitment),
            previous_header_key_commitment_b64: previous_header_key_commitment
                .as_deref()
                .map(encode),
            recipient_identity_ids,
            body_recipient_identity_ids,
            header_recipient_identity_ids,
        });
    }
    transaction.commit().await?;
    Ok(Json(ResourceRotationPlanResponse {
        revoked_identity_id,
        resources,
    }))
}

#[derive(FromRow)]
struct PermissionRow {
    id: Uuid,
    root_grant_id: Uuid,
    member_identity_id: Uuid,
    resource_node_id: Uuid,
    access_level: String,
    access_scope: String,
    grant_origin: String,
    grant_origin_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

fn permission_dto(row: PermissionRow) -> Result<PermissionGrantDto, AppError> {
    let access_level = match row.access_level.as_str() {
        "view" => PermissionAccessLevelDto::View,
        "comment" => PermissionAccessLevelDto::Comment,
        "edit" => PermissionAccessLevelDto::Edit,
        "manage" => PermissionAccessLevelDto::Manage,
        _ => return Err(AppError::Internal),
    };
    let access_scope = match row.access_scope.as_str() {
        "full" => AccessScopeDto::Full,
        "container_only" => AccessScopeDto::ContainerOnly,
        _ => return Err(AppError::Internal),
    };
    let origin = match row.grant_origin.as_str() {
        "explicit" => GrantOriginDto::Direct,
        "assignment" => GrantOriginDto::Assignment {
            assignment_id: row.grant_origin_id.ok_or(AppError::Internal)?,
        },
        "materialized" => GrantOriginDto::Inherited {
            root_grant_id: row.root_grant_id,
            root_resource_id: row.resource_node_id,
        },
        _ => GrantOriginDto::Inherited {
            root_grant_id: row.root_grant_id,
            root_resource_id: row.resource_node_id,
        },
    };
    Ok(PermissionGrantDto {
        id: row.id,
        root_grant_id: row.root_grant_id,
        user_id: row.member_identity_id,
        resource_id: row.resource_node_id,
        access_level,
        access_scope,
        origin,
        granted_at: row.created_at,
        revoked_at: row.revoked_at,
    })
}

fn access_level_str(level: PermissionAccessLevelDto) -> &'static str {
    match level {
        PermissionAccessLevelDto::View => "view",
        PermissionAccessLevelDto::Comment => "comment",
        PermissionAccessLevelDto::Edit => "edit",
        PermissionAccessLevelDto::Manage => "manage",
    }
}

fn access_scope_str(scope: AccessScopeDto) -> &'static str {
    match scope {
        AccessScopeDto::Full => "full",
        AccessScopeDto::ContainerOnly => "container_only",
    }
}

fn visibility_str(visibility: &str) -> Result<&str, AppError> {
    match visibility {
        "private" | "restricted" | "project" | "inherited" => Ok(visibility),
        _ => Err(AppError::BadRequest("invalid permission visibility")),
    }
}

pub(super) async fn expected_resources(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    resource_id: Uuid,
    access_scope: &str,
) -> Result<Vec<Uuid>, AppError> {
    let resources = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT resource_node_id
        FROM sprout_private.expected_hierarchical_permission_rows($1, $2, $3)
        ORDER BY resource_node_id
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .bind(access_scope)
    .fetch_all(&mut **transaction)
    .await?;
    if resources.is_empty() {
        return Err(AppError::NotFound);
    }
    Ok(resources)
}

pub(super) async fn expected_resource_scopes(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    resource_id: Uuid,
    access_scope: &str,
) -> Result<Vec<(Uuid, String)>, AppError> {
    let resources = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT resource_node_id, access_scope
        FROM sprout_private.expected_hierarchical_permission_rows($1, $2, $3)
        ORDER BY resource_node_id
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .bind(access_scope)
    .fetch_all(&mut **transaction)
    .await?;
    if resources.is_empty() {
        return Err(AppError::NotFound);
    }
    Ok(resources)
}

pub(super) async fn validate_and_store_current_envelopes(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    recipient_identity_id: Uuid,
    resource_ids: &[Uuid],
    body_resource_ids: &[Uuid],
    envelopes: &[ResourceKeyEnvelopeDto],
) -> Result<(), AppError> {
    let epoch_rows = sqlx::query_as::<_, (Uuid, i32, bool)>(
        r#"
        SELECT resource_node_id, epoch, header_key_commitment IS NOT NULL
        FROM resource_epochs
        WHERE project_id = $1
          AND resource_node_id = ANY($2)
          AND retired_at IS NULL
        ORDER BY resource_node_id
        "#,
    )
    .bind(project_id)
    .bind(resource_ids)
    .fetch_all(&mut **transaction)
    .await?;
    if epoch_rows.len() != resource_ids.len() {
        return Err(AppError::BadRequest(
            "every affected resource requires an active key epoch",
        ));
    }
    let body_ids = body_resource_ids.iter().copied().collect::<HashSet<_>>();
    if !body_ids.is_subset(&resource_ids.iter().copied().collect()) {
        return Err(AppError::BadRequest(
            "body envelope resources are outside the grant",
        ));
    }
    let body_resources = epoch_rows
        .iter()
        .filter(|(id, _, _)| body_ids.contains(id))
        .map(|(resource_id, epoch, _)| ActiveResourceEpoch {
            resource_id: *resource_id,
            epoch: *epoch,
        })
        .collect::<Vec<_>>();
    let header_resources = epoch_rows
        .iter()
        .filter(|(_, _, separated)| *separated)
        .map(|(resource_id, epoch, _)| ActiveResourceEpoch {
            resource_id: *resource_id,
            epoch: *epoch,
        })
        .collect::<Vec<_>>();
    if epoch_rows
        .iter()
        .any(|(id, _, separated)| !body_ids.contains(id) && !separated)
    {
        return Err(AppError::BadRequest(
            "container-only access requires a purpose-separated encrypted header",
        ));
    }

    let devices = active_device_keys(transaction, project_id, recipient_identity_id).await?;
    if devices.is_empty() {
        return Err(AppError::BadRequest(
            "permission recipient has no active device keys",
        ));
    }
    let sender_key_version = envelope_sender_version(envelopes)?;
    require_active_sender_key(transaction, actor, sender_key_version).await?;
    let decoded = decode_envelopes(envelopes)?;
    let owner_identity_id = active_project_owner(transaction, project_id).await?;
    if decoded.iter().any(|envelope| {
        envelope.recipient_identity_id != recipient_identity_id
            && envelope.recipient_identity_id != owner_identity_id
    }) {
        return Err(AppError::BadRequest(
            "envelopes may cover only the recipient and project owner",
        ));
    }
    validate_recipient_purpose_coverage(
        recipient_identity_id,
        sender_key_version,
        &devices,
        &decoded,
        &body_resources,
        &header_resources,
    )?;
    if owner_identity_id != recipient_identity_id {
        let owner_envelopes = decoded
            .iter()
            .filter(|envelope| envelope.recipient_identity_id == owner_identity_id)
            .cloned()
            .collect::<Vec<_>>();
        if !owner_envelopes.is_empty() {
            let owner_devices =
                active_device_keys(transaction, project_id, owner_identity_id).await?;
            validate_recipient_purpose_coverage(
                owner_identity_id,
                sender_key_version,
                &owner_devices,
                &owner_envelopes,
                &body_resources,
                &header_resources,
            )?;
        }
    }
    verify_envelope_signatures(transaction, actor, project_id, &decoded).await?;
    store_envelopes(transaction, actor, project_id, &decoded).await
}

fn validate_recipient_purpose_coverage(
    recipient_identity_id: Uuid,
    sender_key_version: i32,
    devices: &[ActiveDeviceKey],
    envelopes: &[ResourceKeyEnvelopeInput],
    body_resources: &[ActiveResourceEpoch],
    header_resources: &[ActiveResourceEpoch],
) -> Result<(), AppError> {
    let recipient = envelopes
        .iter()
        .filter(|envelope| envelope.recipient_identity_id == recipient_identity_id)
        .cloned()
        .collect::<Vec<_>>();
    for (purpose, resources) in [("body", body_resources), ("header", header_resources)] {
        let purpose_envelopes = recipient
            .iter()
            .filter(|envelope| envelope.key_purpose == purpose)
            .cloned()
            .collect::<Vec<_>>();
        if resources.is_empty() {
            if !purpose_envelopes.is_empty() {
                return Err(AppError::BadRequest(
                    "resource key envelopes exceed the granted scope",
                ));
            }
            continue;
        }
        validate_envelope_coverage_for_purpose(
            recipient_identity_id,
            sender_key_version,
            resources,
            devices,
            &purpose_envelopes,
            purpose,
        )?;
    }
    Ok(())
}

pub(super) async fn rotate_resource_keys(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    revoked_root_grant_id: Uuid,
    affected_resource_ids: &[Uuid],
    rotations: &[ResourceEpochRotationDto],
) -> Result<(), AppError> {
    let expected = affected_resource_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let supplied = rotations
        .iter()
        .map(|rotation| rotation.resource_id)
        .collect::<HashSet<_>>();
    if expected.is_empty() || expected != supplied || supplied.len() != rotations.len() {
        return Err(AppError::BadRequest(
            "revocation requires one rotation per affected resource",
        ));
    }

    for rotation in rotations {
        let (previous_epoch_id, previous_epoch, has_header_key) =
            sqlx::query_as::<_, (Uuid, i32, bool)>(
                r#"
            SELECT id, epoch, header_key_commitment IS NOT NULL
            FROM resource_epochs
            WHERE project_id = $1
              AND resource_node_id = $2
              AND retired_at IS NULL
            FOR UPDATE
            "#,
            )
            .bind(project_id)
            .bind(rotation.resource_id)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::BadRequest(
                "affected resource has no active key epoch",
            ))?;
        if rotation.previous_epoch_id != previous_epoch_id
            || i32::try_from(rotation.new_epoch).ok() != Some(previous_epoch + 1)
        {
            return Err(AppError::BadRequest(
                "resource key rotation epoch is not the next active epoch",
            ));
        }
        let commitment = decode(&rotation.key_commitment_b64)?;
        let header_commitment = rotation
            .header_key_commitment_b64
            .as_deref()
            .map(decode)
            .transpose()?;
        if has_header_key != header_commitment.is_some()
            || header_commitment
                .as_ref()
                .is_some_and(|value| value.len() < 16)
        {
            return Err(AppError::BadRequest(
                "rotation must preserve purpose-separated header keys",
            ));
        }
        if commitment.len() < 16 {
            return Err(AppError::BadRequest("resource key commitment is too short"));
        }
        let creator_key_version = i32::try_from(rotation.creator_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid creator device key version"))?;
        require_active_sender_key(transaction, actor, creator_key_version).await?;

        let recipients = remaining_resource_recipients(
            transaction,
            project_id,
            rotation.resource_id,
            revoked_root_grant_id,
        )
        .await?;
        let body_recipients = remaining_resource_body_recipients(
            transaction,
            project_id,
            rotation.resource_id,
            revoked_root_grant_id,
        )
        .await?;
        validate_rotation_coverage(
            transaction,
            actor,
            project_id,
            rotation,
            &recipients,
            &body_recipients,
            creator_key_version,
            has_header_key,
        )
        .await?;

        sqlx::query(
            r#"
            UPDATE resource_epochs
            SET retired_at = clock_timestamp()
            WHERE project_id = $1
              AND resource_node_id = $2
              AND id = $3
              AND retired_at IS NULL
            "#,
        )
        .bind(project_id)
        .bind(rotation.resource_id)
        .bind(previous_epoch_id)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE resource_key_envelopes
            SET revoked_at = clock_timestamp()
            WHERE project_id = $1
              AND resource_node_id = $2
              AND epoch = $3
              AND revoked_at IS NULL
            "#,
        )
        .bind(project_id)
        .bind(rotation.resource_id)
        .bind(previous_epoch)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO resource_epochs (
                id, project_id, resource_node_id, epoch, previous_epoch_id,
                created_by_identity_id, created_by_device_id,
                created_by_device_key_version, key_commitment,
                header_key_commitment, reason
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'membership_change')
            "#,
        )
        .bind(rotation.epoch_id)
        .bind(project_id)
        .bind(rotation.resource_id)
        .bind(
            i32::try_from(rotation.new_epoch)
                .map_err(|_| AppError::BadRequest("invalid resource epoch"))?,
        )
        .bind(previous_epoch_id)
        .bind(actor.identity_id)
        .bind(actor.device_id)
        .bind(creator_key_version)
        .bind(commitment)
        .bind(header_commitment)
        .execute(&mut **transaction)
        .await?;
        let decoded = decode_envelopes(&rotation.envelopes)?;
        store_envelopes(transaction, actor, project_id, &decoded).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn validate_rotation_coverage(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    rotation: &ResourceEpochRotationDto,
    recipients: &HashSet<Uuid>,
    body_recipients: &HashSet<Uuid>,
    sender_key_version: i32,
    has_header_key: bool,
) -> Result<(), AppError> {
    let supplied_recipients = rotation
        .envelopes
        .iter()
        .map(|envelope| envelope.recipient_identity_id)
        .collect::<HashSet<_>>();
    let mut recipients_with_devices = HashSet::new();
    let resource = ActiveResourceEpoch {
        resource_id: rotation.resource_id,
        epoch: i32::try_from(rotation.new_epoch)
            .map_err(|_| AppError::BadRequest("invalid resource epoch"))?,
    };
    let decoded = decode_envelopes(&rotation.envelopes)?;
    for recipient in recipients {
        let devices = active_device_keys(transaction, project_id, *recipient).await?;
        if devices.is_empty() {
            continue;
        }
        recipients_with_devices.insert(*recipient);
        let recipient_envelopes = decoded
            .iter()
            .filter(|envelope| envelope.recipient_identity_id == *recipient)
            .cloned()
            .collect::<Vec<_>>();
        let body_envelopes = recipient_envelopes
            .iter()
            .filter(|envelope| envelope.key_purpose == "body")
            .cloned()
            .collect::<Vec<_>>();
        if body_recipients.contains(recipient) {
            validate_envelope_coverage(
                *recipient,
                sender_key_version,
                &[resource],
                &devices,
                &body_envelopes,
            )?;
        } else if !body_envelopes.is_empty() {
            return Err(AppError::BadRequest(
                "container-only recipient cannot receive a body envelope",
            ));
        }
        let header_envelopes = recipient_envelopes
            .iter()
            .filter(|envelope| envelope.key_purpose == "header")
            .cloned()
            .collect::<Vec<_>>();
        if has_header_key {
            validate_envelope_coverage_for_purpose(
                *recipient,
                sender_key_version,
                &[resource],
                &devices,
                &header_envelopes,
                "header",
            )?;
        } else if !header_envelopes.is_empty() {
            return Err(AppError::BadRequest(
                "legacy body-only epoch cannot accept header envelopes",
            ));
        }
    }
    if supplied_recipients != recipients_with_devices {
        return Err(AppError::BadRequest(
            "rotated envelopes do not exactly cover remaining recipients",
        ));
    }
    if envelope_sender_version(&rotation.envelopes)? != sender_key_version {
        return Err(AppError::BadRequest(
            "rotation sender device key version does not match epoch",
        ));
    }
    require_active_sender_key(transaction, actor, sender_key_version).await?;
    verify_envelope_signatures(transaction, actor, project_id, &decoded).await
}

async fn remaining_resource_recipients(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    resource_id: Uuid,
    revoked_root_grant_id: Uuid,
) -> Result<HashSet<Uuid>, AppError> {
    let recipients = sqlx::query_scalar::<_, Uuid>(
        r#"
        WITH candidates AS (
            SELECT membership.identity_id
            FROM project_memberships membership
            WHERE membership.project_id = $1
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')

            UNION

            SELECT node.created_by_identity_id
            FROM resource_nodes node
            WHERE node.project_id = $1
              AND node.id = $2
              AND node.deleted_at IS NULL

            UNION

            SELECT permission.member_identity_id
            FROM sprout_private.domain_permission_rows permission
            JOIN resource_closure closure
              ON closure.project_id = permission.project_id
             AND closure.ancestor_id = permission.resource_node_id
             AND closure.descendant_id = $2
            WHERE permission.project_id = $1
              AND permission.revoked_at IS NULL
              AND permission.root_grant_id <> $3
              AND (
                  permission.access_scope = 'full'
                  OR permission.resource_node_id = $2
              )
        )
        SELECT DISTINCT candidate.identity_id
        FROM candidates candidate
        JOIN project_memberships membership
          ON membership.project_id = $1
         AND membership.identity_id = candidate.identity_id
         AND membership.state = 'active'
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .bind(revoked_root_grant_id)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(recipients.into_iter().collect())
}

async fn remaining_resource_body_recipients(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    resource_id: Uuid,
    revoked_root_grant_id: Uuid,
) -> Result<HashSet<Uuid>, AppError> {
    let recipients = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT DISTINCT identity_id
        FROM (
            SELECT membership.identity_id
            FROM project_memberships membership
            WHERE membership.project_id = $1
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')

            UNION

            SELECT node.created_by_identity_id
            FROM resource_nodes node
            WHERE node.project_id = $1
              AND node.id = $2
              AND node.deleted_at IS NULL

            UNION

            SELECT permission.member_identity_id
            FROM sprout_private.domain_permission_rows permission
            WHERE permission.project_id = $1
              AND permission.resource_node_id = $2
              AND permission.access_scope = 'full'
              AND permission.revoked_at IS NULL
              AND permission.root_grant_id <> $3
        ) retained(identity_id)
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .bind(revoked_root_grant_id)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(recipients.into_iter().collect())
}

pub(super) async fn active_device_keys(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    identity_id: Uuid,
) -> Result<Vec<ActiveDeviceKey>, AppError> {
    Ok(sqlx::query_as::<_, (Uuid, i32)>(
        r#"
        SELECT device_id, key_version
        FROM sprout_private.active_project_device_keys($1, $2)
        ORDER BY device_id, key_version
        "#,
    )
    .bind(project_id)
    .bind(identity_id)
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .map(|(device_id, key_version)| ActiveDeviceKey {
        identity_id,
        device_id,
        key_version,
    })
    .collect())
}

async fn active_project_owner(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
) -> Result<Uuid, AppError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT identity_id
        FROM project_memberships
        WHERE project_id = $1
          AND role = 'owner'
          AND state = 'active'
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::BadRequest(
        "project requires an active owner for key envelope coverage",
    ))
}

async fn require_owner_envelope_coverage(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    resources: &[ActiveResourceEpoch],
) -> Result<(), AppError> {
    if resources.is_empty() {
        return Err(AppError::BadRequest(
            "owner envelope coverage requires affected resources",
        ));
    }
    let owner_identity_id = active_project_owner(transaction, project_id).await?;
    let owner_devices = active_device_keys(transaction, project_id, owner_identity_id).await?;
    if owner_devices.is_empty() {
        return Err(AppError::BadRequest(
            "project owner has no active device keys for envelope coverage",
        ));
    }

    let resource_ids = resources
        .iter()
        .map(|resource| resource.resource_id)
        .collect::<Vec<_>>();
    let epochs = resources
        .iter()
        .map(|resource| resource.epoch)
        .collect::<Vec<_>>();
    let rows = sqlx::query_as::<_, (Uuid, i32, Uuid, i32, Vec<u8>)>(
        r#"
        WITH expected(resource_node_id, epoch) AS (
            SELECT * FROM UNNEST($3::uuid[], $4::integer[])
        )
        SELECT
            envelope.resource_node_id,
            envelope.epoch,
            envelope.recipient_device_id,
            envelope.recipient_device_key_version,
            envelope.encrypted_key
        FROM resource_key_envelopes envelope
        JOIN expected
          ON expected.resource_node_id = envelope.resource_node_id
         AND expected.epoch = envelope.epoch
        WHERE envelope.project_id = $1
          AND envelope.recipient_identity_id = $2
          AND envelope.revoked_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(owner_identity_id)
    .bind(resource_ids)
    .bind(epochs)
    .fetch_all(&mut **transaction)
    .await?;
    let mut supplied = HashSet::with_capacity(rows.len());
    for (resource_id, epoch, device_id, key_version, encrypted_key) in rows {
        validate_experimental_wrapped_resource_key(&encrypted_key, resource_id, device_id, epoch)?;
        supplied.insert((resource_id, epoch, device_id, key_version));
    }
    if !owner_envelope_coverage_is_exact(resources, &owner_devices, &supplied) {
        return Err(AppError::BadRequest(
            "every affected resource requires an envelope for every active owner device",
        ));
    }
    Ok(())
}

fn owner_envelope_coverage_is_exact(
    resources: &[ActiveResourceEpoch],
    owner_devices: &[ActiveDeviceKey],
    supplied: &HashSet<(Uuid, i32, Uuid, i32)>,
) -> bool {
    let expected = resources
        .iter()
        .flat_map(|resource| {
            owner_devices.iter().map(move |device| {
                (
                    resource.resource_id,
                    resource.epoch,
                    device.device_id,
                    device.key_version,
                )
            })
        })
        .collect::<HashSet<_>>();
    !expected.is_empty() && supplied == &expected
}

pub(super) async fn require_active_sender_key(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    key_version: i32,
) -> Result<(), AppError> {
    let active = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM devices device
            JOIN device_keys device_key
              ON device_key.identity_id = device.identity_id
             AND device_key.device_id = device.id
            WHERE device.identity_id = $1
              AND device.id = $2
              AND device.trust_state = 'trusted'
              AND device.retired_at IS NULL
              AND device_key.key_version = $3
              AND device_key.revoked_at IS NULL
        )
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(key_version)
    .fetch_one(&mut **transaction)
    .await?;
    if active {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "sender device key is not active or owned by the actor",
        ))
    }
}

fn envelope_sender_version(envelopes: &[ResourceKeyEnvelopeDto]) -> Result<i32, AppError> {
    let first = envelopes
        .first()
        .ok_or(AppError::BadRequest("resource key envelopes are required"))?;
    let version = i32::try_from(first.sender_device_key_version)
        .map_err(|_| AppError::BadRequest("invalid sender device key version"))?;
    if envelopes
        .iter()
        .any(|envelope| i32::try_from(envelope.sender_device_key_version).ok() != Some(version))
    {
        return Err(AppError::BadRequest(
            "envelopes must use one sender device key version",
        ));
    }
    Ok(version)
}

pub(super) fn decode_envelopes(
    envelopes: &[ResourceKeyEnvelopeDto],
) -> Result<Vec<ResourceKeyEnvelopeInput>, AppError> {
    envelopes
        .iter()
        .map(|envelope| {
            Ok(ResourceKeyEnvelopeInput {
                version: i16::try_from(envelope.version)
                    .map_err(|_| AppError::BadRequest("invalid envelope version"))?,
                resource_id: envelope.resource_id,
                epoch: i32::try_from(envelope.epoch)
                    .map_err(|_| AppError::BadRequest("invalid envelope epoch"))?,
                key_purpose: match envelope.key_purpose {
                    ResourceKeyPurposeDto::Body => "body",
                    ResourceKeyPurposeDto::Header => "header",
                }
                .into(),
                recipient_identity_id: envelope.recipient_identity_id,
                recipient_device_id: envelope.recipient_device_id,
                recipient_device_key_version: i32::try_from(envelope.recipient_device_key_version)
                    .map_err(|_| AppError::BadRequest("invalid recipient key version"))?,
                sender_device_key_version: i32::try_from(envelope.sender_device_key_version)
                    .map_err(|_| AppError::BadRequest("invalid sender key version"))?,
                encrypted_key: decode(&envelope.encrypted_key_b64)?,
                sender_signature: decode(&envelope.sender_signature_b64)?,
                sender_post_quantum_signature: decode(&envelope.sender_post_quantum_signature_b64)?,
            })
        })
        .collect()
}

pub(super) async fn verify_envelope_signatures(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    envelopes: &[ResourceKeyEnvelopeInput],
) -> Result<(), AppError> {
    let sender_key_version = envelopes
        .first()
        .map(|envelope| envelope.sender_device_key_version)
        .ok_or(AppError::BadRequest("resource key envelopes are required"))?;
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
    .bind(sender_key_version)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    for envelope in envelopes {
        let message = envelope_signing_bytes(project_id, envelope);
        verify_ed25519_ml_dsa65_signatures(
            &keys.0,
            &envelope.sender_signature,
            &keys.1,
            &envelope.sender_post_quantum_signature,
            &message,
            b"sprout-resource-key-envelope-v2",
        )
        .map_err(|_| AppError::BadRequest("resource key envelope signature verification failed"))?;
    }
    Ok(())
}

fn envelope_signing_bytes(project_id: Uuid, envelope: &ResourceKeyEnvelopeInput) -> Vec<u8> {
    let encrypted_hash = Sha256::digest(&envelope.encrypted_key);
    let mut bytes = Vec::with_capacity(160);
    bytes.extend_from_slice(b"sprout-resource-key-envelope-v2");
    bytes.extend_from_slice(project_id.as_bytes());
    bytes.extend_from_slice(&envelope.version.to_be_bytes());
    bytes.extend_from_slice(envelope.resource_id.as_bytes());
    bytes.extend_from_slice(&envelope.epoch.to_be_bytes());
    bytes.extend_from_slice(envelope.recipient_identity_id.as_bytes());
    bytes.extend_from_slice(envelope.recipient_device_id.as_bytes());
    bytes.extend_from_slice(&envelope.recipient_device_key_version.to_be_bytes());
    bytes.extend_from_slice(&envelope.sender_device_key_version.to_be_bytes());
    bytes.extend_from_slice(&encrypted_hash);
    if envelope.key_purpose == "header" {
        bytes.extend_from_slice(b"header");
    }
    bytes
}

pub(super) async fn store_envelopes(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    envelopes: &[ResourceKeyEnvelopeInput],
) -> Result<(), AppError> {
    for envelope in envelopes {
        let inserted = sqlx::query(
            r#"
            INSERT INTO resource_key_envelopes (
                project_id, resource_node_id, epoch, envelope_version,
                key_purpose,
                recipient_identity_id, recipient_device_id,
                recipient_device_key_version, encrypted_key, sender_signature,
                sender_post_quantum_signature,
                created_by_identity_id, created_by_device_id,
                created_by_device_key_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (
                project_id, resource_node_id, epoch,
                key_purpose, recipient_device_id, recipient_device_key_version
            ) DO NOTHING
            "#,
        )
        .bind(project_id)
        .bind(envelope.resource_id)
        .bind(envelope.epoch)
        .bind(envelope.version)
        .bind(&envelope.key_purpose)
        .bind(envelope.recipient_identity_id)
        .bind(envelope.recipient_device_id)
        .bind(envelope.recipient_device_key_version)
        .bind(&envelope.encrypted_key)
        .bind(&envelope.sender_signature)
        .bind(&envelope.sender_post_quantum_signature)
        .bind(actor.identity_id)
        .bind(actor.device_id)
        .bind(envelope.sender_device_key_version)
        .execute(&mut **transaction)
        .await?
        .rows_affected();
        if inserted == 0 {
            let active = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                    FROM resource_key_envelopes
                    WHERE project_id = $1
                      AND resource_node_id = $2
                      AND epoch = $3
                      AND key_purpose = $4
                      AND recipient_identity_id = $5
                      AND recipient_device_id = $6
                      AND recipient_device_key_version = $7
                      AND revoked_at IS NULL
                )
                "#,
            )
            .bind(project_id)
            .bind(envelope.resource_id)
            .bind(envelope.epoch)
            .bind(&envelope.key_purpose)
            .bind(envelope.recipient_identity_id)
            .bind(envelope.recipient_device_id)
            .bind(envelope.recipient_device_key_version)
            .fetch_one(&mut **transaction)
            .await?;
            if !active {
                return Err(AppError::BadRequest(
                    "a revoked envelope cannot be reused without rotation",
                ));
            }
        }
    }
    let resources = envelopes
        .iter()
        .map(|envelope| ActiveResourceEpoch {
            resource_id: envelope.resource_id,
            epoch: envelope.epoch,
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    require_owner_envelope_coverage(transaction, project_id, &resources).await
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 key material"))
}

fn encode(value: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_wire_values_match_database_discriminants() {
        assert_eq!(access_level_str(PermissionAccessLevelDto::Manage), "manage");
        assert_eq!(
            access_scope_str(AccessScopeDto::ContainerOnly),
            "container_only"
        );
        assert!(visibility_str("restricted").is_ok());
        assert!(visibility_str("public").is_err());
    }

    #[test]
    fn envelope_decoder_checks_base64_but_not_signature_cryptography() {
        let envelope = ResourceKeyEnvelopeDto {
            version: 1,
            resource_id: Uuid::new_v4(),
            epoch: 1,
            key_purpose: ResourceKeyPurposeDto::Body,
            recipient_identity_id: Uuid::new_v4(),
            recipient_device_id: Uuid::new_v4(),
            recipient_device_key_version: 1,
            sender_device_key_version: 1,
            encrypted_key_b64: "not base64!".into(),
            sender_signature_b64: "also not base64!".into(),
            sender_post_quantum_signature_b64: "also not base64!".into(),
        };
        assert!(decode_envelopes(&[envelope]).is_err());
    }

    #[test]
    fn owner_coverage_requires_every_resource_device_pair() {
        let owner = Uuid::new_v4();
        let devices = [
            ActiveDeviceKey {
                identity_id: owner,
                device_id: Uuid::new_v4(),
                key_version: 1,
            },
            ActiveDeviceKey {
                identity_id: owner,
                device_id: Uuid::new_v4(),
                key_version: 2,
            },
        ];
        let resources = [
            ActiveResourceEpoch {
                resource_id: Uuid::new_v4(),
                epoch: 3,
            },
            ActiveResourceEpoch {
                resource_id: Uuid::new_v4(),
                epoch: 4,
            },
        ];
        let mut supplied = resources
            .iter()
            .flat_map(|resource| {
                devices.iter().map(move |device| {
                    (
                        resource.resource_id,
                        resource.epoch,
                        device.device_id,
                        device.key_version,
                    )
                })
            })
            .collect::<HashSet<_>>();
        assert!(owner_envelope_coverage_is_exact(
            &resources, &devices, &supplied
        ));
        supplied.remove(&(
            resources[0].resource_id,
            resources[0].epoch,
            devices[0].device_id,
            devices[0].key_version,
        ));
        assert!(!owner_envelope_coverage_is_exact(
            &resources, &devices, &supplied
        ));
    }
}
