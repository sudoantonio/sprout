use std::{collections::HashSet, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sprout_api_contract::{
    ActivateProjectRecoveryRequest, FinalizeProjectRecoveryRequest,
    ListMyProjectRecoverySharesResponse, ProjectRecoveryApprovalRequest,
    ProjectRecoveryFinalizedDto, ProjectRecoveryProvisionStatusDto, ProjectRecoveryShareInputDto,
    ProjectRecoveryShareViewDto, ProjectRecoveryStatusDto, ProvisionProjectRecoveryRequest,
    RecoveryApprovalDeliveryDto, RecoveryRotationPlanResponse, ResourceEpochRotationDto,
    ResourceRotationPlanItemDto, StartProjectRecoveryRequest,
};
use sprout_crypto_protocol::verify_ed25519_ml_dsa65_signatures;
use sprout_storage_postgres::{ActiveResourceEpoch, validate_envelope_coverage};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::permissions::{
    active_device_keys, decode_envelopes, require_active_sender_key, store_envelopes,
    verify_envelope_signatures,
};
use crate::{
    AppState,
    auth::{AuthSession, ProjectAccess, require_project_access, set_database_context},
    error::AppError,
};

const RECOVERY_APPROVAL_CONTEXT: &[u8] = b"sprout-project-recovery-approval-v1";

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    ParticipantDevice,
    LostOwner,
}

impl RecoveryKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ParticipantDevice => "participant_device",
            Self::LostOwner => "lost_owner",
        }
    }
}

fn recovery_kind(kind: sprout_api_contract::ProjectRecoveryKindDto) -> RecoveryKind {
    match kind {
        sprout_api_contract::ProjectRecoveryKindDto::ParticipantDevice => {
            RecoveryKind::ParticipantDevice
        }
        sprout_api_contract::ProjectRecoveryKindDto::LostOwner => RecoveryKind::LostOwner,
    }
}

pub async fn provision_status(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ProjectRecoveryProvisionStatusDto>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let status = load_provision_status(&mut transaction, project_id).await?;
    transaction.commit().await?;
    Ok(Json(status))
}

pub async fn provision(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ProvisionProjectRecoveryRequest>,
) -> Result<Json<ProjectRecoveryProvisionStatusDto>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    require_owner(&mut transaction, project_id, actor.identity_id).await?;
    upsert_draft_recovery_set(&mut transaction, actor, project_id, &request).await?;
    let status = load_provision_status(&mut transaction, project_id).await?;
    transaction.commit().await?;
    Ok(Json(status))
}

pub async fn activate(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ActivateProjectRecoveryRequest>,
) -> Result<Json<ProjectRecoveryProvisionStatusDto>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    require_owner(&mut transaction, project_id, actor.identity_id).await?;
    activate_recovery_set(&mut transaction, project_id, request.recovery_set_id).await?;
    let status = load_provision_status(&mut transaction, project_id).await?;
    transaction.commit().await?;
    Ok(Json(status))
}

pub async fn shares_me(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ListMyProjectRecoverySharesResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let shares = sqlx::query_as::<_, ShareViewRow>(
        r#"
        SELECT share.id AS share_id, share.recovery_set_id, recovery.recovery_epoch,
               recovery.membership_epoch, share.share_index, share.encrypted_share,
               share.share_commitment, share.holder_device_id,
               share.holder_device_key_version, recovery.context_hash,
               recovery.secret_commitment
        FROM project_recovery_shares share
        JOIN project_recovery_sets recovery
          ON recovery.project_id = share.project_id
         AND recovery.id = share.recovery_set_id
        WHERE share.project_id = $1
          AND share.holder_identity_id = $2
          AND recovery.state = 'active'
        ORDER BY share.share_index, share.holder_device_id
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ListMyProjectRecoverySharesResponse {
        shares: shares.into_iter().map(ShareViewRow::into_dto).collect(),
    }))
}

pub async fn rotation_plan(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<RecoveryRotationPlanResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let members = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT identity_id FROM project_memberships
        WHERE project_id = $1 AND state = 'active'
        ORDER BY identity_id
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut *transaction)
    .await?;
    let rows = sqlx::query_as::<_, (Uuid, Uuid, i32, Vec<u8>, Option<Vec<u8>>)>(
        r#"
        SELECT id, resource_node_id, epoch, key_commitment, header_key_commitment
        FROM resource_epochs
        WHERE project_id = $1 AND retired_at IS NULL
        ORDER BY resource_node_id
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(RecoveryRotationPlanResponse {
        resources: rows
            .into_iter()
            .map(
                |(
                    previous_epoch_id,
                    resource_id,
                    current_epoch,
                    key_commitment,
                    header_key_commitment,
                )| {
                    ResourceRotationPlanItemDto {
                        resource_id,
                        previous_epoch_id,
                        current_epoch: u32::try_from(current_epoch).unwrap_or(0),
                        previous_key_commitment_b64: encode(&key_commitment),
                        previous_header_key_commitment_b64: header_key_commitment
                            .as_deref()
                            .map(encode),
                        recipient_identity_ids: members.clone(),
                        body_recipient_identity_ids: members.clone(),
                        header_recipient_identity_ids: header_key_commitment
                            .as_ref()
                            .map(|_| members.clone())
                            .unwrap_or_default(),
                    }
                },
            )
            .collect(),
    }))
}

pub async fn start(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<StartProjectRecoveryRequest>,
) -> Result<Json<ProjectRecoveryStatusDto>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    if !(300..=86_400).contains(&request.expires_in_seconds) {
        return Err(AppError::BadRequest("recovery expiry is out of range"));
    }
    let request_kind = recovery_kind(request.request_kind);
    let challenge = decode_fixed(&request.challenge_b64, 32, "invalid recovery challenge")?;
    let context_hash = decode_fixed(
        &request.context_hash_b64,
        32,
        "invalid recovery context hash",
    )?;
    let expires_at = Utc::now() + Duration::seconds(i64::from(request.expires_in_seconds));
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let (membership_epoch, requester_role) = sqlx::query_as::<_, (i64, String)>(
        r#"
        SELECT project.membership_epoch, membership.role
        FROM projects project
        JOIN project_memberships membership
          ON membership.project_id = project.id
         AND membership.identity_id = $2
         AND membership.state = 'active'
        WHERE project.id = $1
        FOR UPDATE OF project
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    match request_kind {
        RecoveryKind::ParticipantDevice if requester_role == "owner" => {
            return Err(AppError::BadRequest(
                "owner recovery must use lost_owner mode",
            ));
        }
        RecoveryKind::LostOwner if requester_role != "owner" => {
            return Err(AppError::Forbidden);
        }
        _ => {}
    }
    require_active_sender_key(
        &mut transaction,
        actor,
        request.requester_device_key_version,
    )
    .await?;
    let active_set = sqlx::query_as::<_, ActiveRecoverySetRow>(
        r#"
        SELECT id, recovery_epoch, membership_epoch, share_count, secret_commitment,
               encrypted_owner_key_escrow
        FROM project_recovery_sets
        WHERE project_id = $1 AND state = 'active'
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(active_set) = active_set else {
        return Err(AppError::RecoveryUnprovisioned);
    };
    if active_set.membership_epoch != membership_epoch {
        return Err(AppError::RecoveryUnprovisioned);
    }
    let holders = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT DISTINCT holder_identity_id
        FROM project_recovery_shares
        WHERE project_id = $1 AND recovery_set_id = $2
        ORDER BY holder_identity_id
        "#,
    )
    .bind(project_id)
    .bind(active_set.id)
    .fetch_all(&mut *transaction)
    .await?;
    if holders.is_empty() || i64::from(active_set.share_count) != holders.len() as i64 {
        return Err(AppError::BadRequest("recovery has no eligible approvers"));
    }
    let live_non_owners = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT identity_id
        FROM project_memberships
        WHERE project_id = $1 AND role <> 'owner' AND state = 'active'
        ORDER BY identity_id
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut *transaction)
    .await?;
    if live_non_owners != holders {
        return Err(AppError::RecoveryUnprovisioned);
    }
    sqlx::query(
        r#"
        UPDATE project_recovery_requests
        SET status = 'expired'
        WHERE project_id = $1
          AND requester_identity_id = $2
          AND status = 'pending'
          AND expires_at <= clock_timestamp()
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO project_recovery_requests (
            id, project_id, requester_identity_id, request_kind,
            challenge, context_hash, membership_epoch, recovery_epoch,
            recovery_set_id, requester_device_id, requester_device_key_version,
            expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(request.request_id)
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(request_kind.as_str())
    .bind(&challenge)
    .bind(&context_hash)
    .bind(membership_epoch)
    .bind(active_set.recovery_epoch)
    .bind(active_set.id)
    .bind(actor.device_id)
    .bind(request.requester_device_key_version)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;
    let approvers = match request_kind {
        RecoveryKind::ParticipantDevice => {
            sqlx::query_as::<_, (Uuid, String)>(
                r#"
                SELECT identity_id, role
                FROM project_memberships
                WHERE project_id = $1 AND role = 'owner' AND state = 'active'
                "#,
            )
            .bind(project_id)
            .fetch_all(&mut *transaction)
            .await?
        }
        RecoveryKind::LostOwner => {
            sqlx::query_as::<_, (Uuid, String)>(
                r#"
                SELECT membership.identity_id, membership.role
                FROM project_memberships membership
                WHERE membership.project_id = $1
                  AND membership.role <> 'owner'
                  AND membership.state = 'active'
                  AND membership.identity_id = ANY($2)
                ORDER BY membership.identity_id
                "#,
            )
            .bind(project_id)
            .bind(&holders)
            .fetch_all(&mut *transaction)
            .await?
        }
    };
    if approvers.is_empty() {
        return Err(AppError::BadRequest("recovery has no eligible approvers"));
    }
    for (identity_id, role) in &approvers {
        sqlx::query(
            r#"
            INSERT INTO project_recovery_electorate (
                project_id, recovery_request_id, approver_identity_id,
                snapshot_role, membership_epoch
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(project_id)
        .bind(request.request_id)
        .bind(identity_id)
        .bind(role)
        .bind(membership_epoch)
        .execute(&mut *transaction)
        .await?;
    }
    let recovery = RecoveryRow {
        id: request.request_id,
        project_id,
        requester_identity_id: actor.identity_id,
        request_kind: request_kind.as_str().into(),
        challenge: challenge.clone(),
        context_hash: context_hash.clone(),
        membership_epoch,
        recovery_epoch: active_set.recovery_epoch,
        recovery_set_id: active_set.id,
        status: "pending".into(),
        expires_at,
        encrypted_owner_key_escrow: active_set.encrypted_owner_key_escrow.clone(),
        secret_commitment: active_set.secret_commitment.clone(),
    };
    let status = load_status(&mut transaction, &recovery, actor.identity_id).await?;
    transaction.commit().await?;
    Ok(Json(status))
}

pub async fn approve(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, request_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ProjectRecoveryApprovalRequest>,
) -> Result<Json<ProjectRecoveryStatusDto>, AppError> {
    let encrypted_share = decode(&request.encrypted_share_b64)?;
    if encrypted_share.is_empty() {
        return Err(AppError::BadRequest("encrypted recovery share is empty"));
    }
    let classical_signature = decode(&request.classical_signature_b64)?;
    let post_quantum_signature = decode(&request.post_quantum_signature_b64)?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let recovery = sqlx::query_as::<_, RecoveryRow>(
        r#"
        SELECT request.id, request.project_id, request.requester_identity_id,
               request.request_kind, request.challenge, request.context_hash,
               request.membership_epoch, request.recovery_epoch, request.recovery_set_id,
               request.status, request.expires_at,
               recovery.encrypted_owner_key_escrow, recovery.secret_commitment
        FROM project_recovery_requests request
        JOIN project_recovery_electorate electorate
          ON electorate.project_id = request.project_id
         AND electorate.recovery_request_id = request.id
         AND electorate.approver_identity_id = $3
        JOIN project_recovery_sets recovery
          ON recovery.project_id = request.project_id
         AND recovery.id = request.recovery_set_id
        WHERE request.project_id = $1 AND request.id = $2
        FOR UPDATE OF request
        "#,
    )
    .bind(project_id)
    .bind(request_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    if recovery.status != "pending" || recovery.expires_at <= Utc::now() {
        return Err(AppError::Conflict);
    }
    if recovery.request_kind == "lost_owner" {
        let held = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
            FROM project_recovery_shares
            WHERE project_id = $1
              AND recovery_set_id = $2
              AND holder_identity_id = $3
            "#,
        )
        .bind(project_id)
        .bind(recovery.recovery_set_id)
        .bind(actor.identity_id)
        .fetch_one(&mut *transaction)
        .await?;
        if held == 0 {
            return Err(AppError::Forbidden);
        }
    }
    let keys = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        r#"
        SELECT ed25519_public_key, ml_dsa_65_public_key
        FROM device_keys
        WHERE identity_id = $1 AND device_id = $2 AND key_version = $3
          AND suite_version = 32769 AND revoked_at IS NULL
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(request.approver_device_key_version)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let message = approval_signing_bytes(&recovery, actor.identity_id, &encrypted_share);
    verify_ed25519_ml_dsa65_signatures(
        &keys.0,
        &classical_signature,
        &keys.1,
        &post_quantum_signature,
        &message,
        RECOVERY_APPROVAL_CONTEXT,
    )
    .map_err(|_| AppError::BadRequest("recovery approval signature verification failed"))?;
    sqlx::query(
        r#"
        INSERT INTO project_recovery_approvals (
            project_id, recovery_request_id, approver_identity_id,
            approver_device_id, approver_device_key_version,
            encrypted_share, classical_signature, post_quantum_signature
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(project_id)
    .bind(request_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(request.approver_device_key_version)
    .bind(encrypted_share)
    .bind(classical_signature)
    .bind(post_quantum_signature)
    .execute(&mut *transaction)
    .await?;
    let status = load_status(&mut transaction, &recovery, actor.identity_id).await?;
    transaction.commit().await?;
    Ok(Json(status))
}

pub async fn finalize(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, request_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<FinalizeProjectRecoveryRequest>,
) -> Result<Json<ProjectRecoveryFinalizedDto>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let recovery = sqlx::query_as::<_, RecoveryRow>(
        r#"
        SELECT request.id, request.project_id, request.requester_identity_id,
               request.request_kind, request.challenge, request.context_hash,
               request.membership_epoch, request.recovery_epoch, request.recovery_set_id,
               request.status, request.expires_at,
               recovery.encrypted_owner_key_escrow, recovery.secret_commitment
        FROM project_recovery_requests request
        JOIN project_recovery_sets recovery
          ON recovery.project_id = request.project_id
         AND recovery.id = request.recovery_set_id
        WHERE request.project_id = $1 AND request.id = $2
        FOR UPDATE OF request
        "#,
    )
    .bind(project_id)
    .bind(request_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if recovery.requester_identity_id != actor.identity_id
        || recovery.status != "pending"
        || recovery.expires_at <= Utc::now()
    {
        return Err(AppError::Conflict);
    }
    let (expected, approved) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            (SELECT count(*) FROM project_recovery_electorate
             WHERE project_id = $1 AND recovery_request_id = $2),
            (SELECT count(*) FROM project_recovery_approvals
             WHERE project_id = $1 AND recovery_request_id = $2)
        "#,
    )
    .bind(project_id)
    .bind(request_id)
    .fetch_one(&mut *transaction)
    .await?;
    if expected == 0 || approved != expected {
        return Err(AppError::Conflict);
    }
    require_active_sender_key(&mut transaction, actor, request.new_device_key_version).await?;
    let generation = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT generation FROM device_keys
        WHERE identity_id = $1 AND device_id = $2 AND key_version = $3
          AND suite_version = 32769 AND revoked_at IS NULL
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(request.new_device_key_version)
    .fetch_one(&mut *transaction)
    .await?;
    let device_epoch = sqlx::query_scalar::<_, i64>(
        r#"
        UPDATE devices
        SET key_epoch = key_epoch + 1
        WHERE identity_id = $1 AND id = $2 AND retired_at IS NULL
        RETURNING key_epoch
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_one(&mut *transaction)
    .await?;
    rotate_for_recovery(
        &mut transaction,
        actor,
        project_id,
        request.new_device_key_version,
        &request.rotations,
    )
    .await?;
    append_recovery_revocation_logs(
        &mut transaction,
        actor,
        request.new_device_key_version,
        request_id,
    )
    .await?;
    sqlx::query(
        r#"
        UPDATE device_keys
        SET revoked_at = clock_timestamp()
        WHERE identity_id = $1
          AND NOT (device_id = $2 AND key_version = $3)
          AND revoked_at IS NULL
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(request.new_device_key_version)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE project_recovery_sets
        SET state = 'retired', retired_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2 AND state = 'active'
        "#,
    )
    .bind(project_id)
    .bind(recovery.recovery_set_id)
    .execute(&mut *transaction)
    .await?;
    let (owner_epoch, recovery_epoch) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        UPDATE projects
        SET key_epoch = key_epoch + 1,
            owner_epoch = owner_epoch + CASE WHEN $2 = 'lost_owner' THEN 1 ELSE 0 END,
            recovery_epoch = recovery_epoch + 1
        WHERE id = $1
        RETURNING owner_epoch, recovery_epoch
        "#,
    )
    .bind(project_id)
    .bind(&recovery.request_kind)
    .fetch_one(&mut *transaction)
    .await?;
    if request.replacement_recovery.recovery_epoch != recovery_epoch {
        return Err(AppError::BadRequest(
            "replacement recovery epoch must match the advanced project recovery epoch",
        ));
    }
    upsert_draft_recovery_set(
        &mut transaction,
        actor,
        project_id,
        &request.replacement_recovery,
    )
    .await?;
    activate_recovery_set(
        &mut transaction,
        project_id,
        request.replacement_recovery.recovery_set_id,
    )
    .await?;
    sqlx::query(
        r#"
        UPDATE project_recovery_requests
        SET status = 'finalized', finalized_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2 AND status = 'pending'
        "#,
    )
    .bind(project_id)
    .bind(request_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ProjectRecoveryFinalizedDto {
        request_id,
        status: "finalized".into(),
        owner_epoch,
        device_epoch,
        device_generation: generation,
        recovery_epoch,
    }))
}

async fn append_recovery_revocation_logs(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    retained_key_version: i32,
    recovery_request_id: Uuid,
) -> Result<(), AppError> {
    let revoked = sqlx::query_as::<_, (Uuid, i32, i64, Vec<u8>)>(
        r#"
        SELECT device_id, key_version, generation, package_hash
        FROM device_keys
        WHERE identity_id = $1
          AND NOT (device_id = $2 AND key_version = $3)
          AND revoked_at IS NULL
        ORDER BY device_id, generation
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(retained_key_version)
    .fetch_all(&mut **transaction)
    .await?;
    for (device_id, key_version, generation, package_hash) in revoked {
        let previous = sqlx::query_scalar::<_, Vec<u8>>(
            r#"
            SELECT entry_hash FROM device_key_transparency_log
            WHERE identity_id = $1 AND device_id = $2
            ORDER BY log_sequence DESC
            LIMIT 1
            "#,
        )
        .bind(actor.identity_id)
        .bind(device_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let mut digest = Sha256::new();
        digest.update(b"sprout-device-key-transparency-v1");
        digest.update(actor.identity_id.as_bytes());
        digest.update(device_id.as_bytes());
        digest.update(key_version.to_be_bytes());
        digest.update(generation.to_be_bytes());
        digest.update(b"recovery_revoked");
        digest.update(&package_hash);
        if let Some(previous) = &previous {
            digest.update(previous);
        }
        let entry_hash: [u8; 32] = digest.finalize().into();
        sqlx::query(
            r#"
            INSERT INTO device_key_transparency_log (
                identity_id, device_id, key_version, generation, event_kind,
                package_hash, previous_entry_hash, entry_hash,
                authorization_reference
            )
            VALUES ($1, $2, $3, $4, 'recovery_revoked', $5, $6, $7, $8)
            "#,
        )
        .bind(actor.identity_id)
        .bind(device_id)
        .bind(key_version)
        .bind(generation)
        .bind(package_hash)
        .bind(previous)
        .bind(entry_hash.as_slice())
        .bind(recovery_request_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProjectRecoveryStatusDto>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let recovery = sqlx::query_as::<_, RecoveryRow>(
        r#"
        SELECT request.id, request.project_id, request.requester_identity_id,
               request.request_kind, request.challenge, request.context_hash,
               request.membership_epoch, request.recovery_epoch, request.recovery_set_id,
               request.status, request.expires_at,
               recovery.encrypted_owner_key_escrow, recovery.secret_commitment
        FROM project_recovery_requests request
        JOIN project_recovery_sets recovery
          ON recovery.project_id = request.project_id
         AND recovery.id = request.recovery_set_id
        WHERE request.project_id = $1 AND request.id = $2
        "#,
    )
    .bind(project_id)
    .bind(request_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let status = load_status(&mut transaction, &recovery, actor.identity_id).await?;
    transaction.commit().await?;
    Ok(Json(status))
}

#[derive(FromRow)]
struct RecoveryRow {
    id: Uuid,
    project_id: Uuid,
    requester_identity_id: Uuid,
    request_kind: String,
    challenge: Vec<u8>,
    context_hash: Vec<u8>,
    membership_epoch: i64,
    recovery_epoch: i64,
    recovery_set_id: Uuid,
    status: String,
    expires_at: DateTime<Utc>,
    encrypted_owner_key_escrow: Vec<u8>,
    secret_commitment: Vec<u8>,
}

#[derive(FromRow)]
struct ActiveRecoverySetRow {
    id: Uuid,
    recovery_epoch: i64,
    membership_epoch: i64,
    share_count: i16,
    secret_commitment: Vec<u8>,
    encrypted_owner_key_escrow: Vec<u8>,
}

#[derive(FromRow)]
struct ShareViewRow {
    share_id: Uuid,
    recovery_set_id: Uuid,
    recovery_epoch: i64,
    membership_epoch: i64,
    share_index: i16,
    encrypted_share: Vec<u8>,
    share_commitment: Vec<u8>,
    holder_device_id: Uuid,
    holder_device_key_version: i32,
    context_hash: Vec<u8>,
    secret_commitment: Vec<u8>,
}

impl ShareViewRow {
    fn into_dto(self) -> ProjectRecoveryShareViewDto {
        ProjectRecoveryShareViewDto {
            share_id: self.share_id,
            recovery_set_id: self.recovery_set_id,
            recovery_epoch: self.recovery_epoch,
            membership_epoch: self.membership_epoch,
            share_index: self.share_index as u16,
            encrypted_share_b64: encode(&self.encrypted_share),
            share_commitment_b64: encode(&self.share_commitment),
            holder_device_id: self.holder_device_id,
            holder_device_key_version: self.holder_device_key_version,
            context_hash_b64: encode(&self.context_hash),
            secret_commitment_b64: encode(&self.secret_commitment),
        }
    }
}

async fn load_status(
    transaction: &mut Transaction<'_, Postgres>,
    recovery: &RecoveryRow,
    viewer_identity_id: Uuid,
) -> Result<ProjectRecoveryStatusDto, AppError> {
    let required = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT approver_identity_id
        FROM project_recovery_electorate
        WHERE project_id = $1 AND recovery_request_id = $2
        ORDER BY approver_identity_id
        "#,
    )
    .bind(recovery.project_id)
    .bind(recovery.id)
    .fetch_all(&mut **transaction)
    .await?;
    let approved = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT approver_identity_id
        FROM project_recovery_approvals
        WHERE project_id = $1 AND recovery_request_id = $2
        ORDER BY approver_identity_id
        "#,
    )
    .bind(recovery.project_id)
    .bind(recovery.id)
    .fetch_all(&mut **transaction)
    .await?;
    let status = if recovery.status == "pending" && recovery.expires_at <= Utc::now() {
        "expired".into()
    } else {
        recovery.status.clone()
    };
    let is_requester = viewer_identity_id == recovery.requester_identity_id;
    let delivery_available = is_requester
        && !required.is_empty()
        && approved.len() == required.len()
        && status == "pending";
    let deliveries = if is_requester {
        sqlx::query_as::<_, (Uuid, Vec<u8>, Vec<u8>, Vec<u8>)>(
            r#"
            SELECT approver_identity_id, encrypted_share,
                   classical_signature, post_quantum_signature
            FROM project_recovery_approvals
            WHERE project_id = $1 AND recovery_request_id = $2
            ORDER BY approver_identity_id
            "#,
        )
        .bind(recovery.project_id)
        .bind(recovery.id)
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .map(
            |(
                approver_identity_id,
                encrypted_share,
                classical_signature,
                post_quantum_signature,
            )| {
                RecoveryApprovalDeliveryDto {
                    approver_identity_id,
                    encrypted_share_b64: encode(&encrypted_share),
                    classical_signature_b64: encode(&classical_signature),
                    post_quantum_signature_b64: encode(&post_quantum_signature),
                }
            },
        )
        .collect()
    } else {
        Vec::new()
    };
    Ok(ProjectRecoveryStatusDto {
        request_id: recovery.id,
        project_id: recovery.project_id,
        requester_identity_id: recovery.requester_identity_id,
        request_kind: match recovery.request_kind.as_str() {
            "lost_owner" => sprout_api_contract::ProjectRecoveryKindDto::LostOwner,
            _ => sprout_api_contract::ProjectRecoveryKindDto::ParticipantDevice,
        },
        status,
        membership_epoch: recovery.membership_epoch,
        recovery_epoch: recovery.recovery_epoch,
        recovery_set_id: recovery.recovery_set_id,
        challenge_b64: encode(&recovery.challenge),
        context_hash_b64: encode(&recovery.context_hash),
        approval_signature_context_b64: encode(RECOVERY_APPROVAL_CONTEXT),
        canonical_approval_prefix_b64: required.contains(&viewer_identity_id).then(|| {
            encode(&approval_signing_prefix_bytes(
                recovery.project_id,
                recovery.id,
                recovery.requester_identity_id,
                viewer_identity_id,
                recovery.membership_epoch,
                recovery.recovery_epoch,
                recovery.recovery_set_id,
                &recovery.secret_commitment,
                &recovery.challenge,
                &recovery.context_hash,
            ))
        }),
        required_approver_ids: required,
        approved_approver_ids: approved,
        expires_at: recovery.expires_at,
        delivery_available,
        deliveries,
        encrypted_owner_key_escrow_b64: is_requester
            .then(|| encode(&recovery.encrypted_owner_key_escrow)),
    })
}

async fn load_provision_status(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
) -> Result<ProjectRecoveryProvisionStatusDto, AppError> {
    let (membership_epoch, recovery_epoch, non_owner_count) = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
            SELECT project.membership_epoch, project.recovery_epoch,
                   (
                       SELECT count(*)
                       FROM project_memberships membership
                       WHERE membership.project_id = project.id
                         AND membership.role <> 'owner'
                         AND membership.state = 'active'
                   )
            FROM projects project
            WHERE project.id = $1
            "#,
    )
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    let active = sqlx::query_as::<_, (Uuid, i64, i64, i16, String, Vec<u8>)>(
        r#"
        SELECT id, recovery_epoch, membership_epoch, share_count, state, secret_commitment
        FROM project_recovery_sets
        WHERE project_id = $1 AND state IN ('active', 'draft')
        ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END, created_at DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let holders = if let Some((set_id, _, _, _, _, _)) = &active {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT DISTINCT holder_identity_id
            FROM project_recovery_shares
            WHERE project_id = $1 AND recovery_set_id = $2
            ORDER BY holder_identity_id
            "#,
        )
        .bind(project_id)
        .bind(set_id)
        .fetch_all(&mut **transaction)
        .await?
    } else {
        Vec::new()
    };
    let recoverable = non_owner_count >= 1;
    let warning = if !recoverable {
        Some(
            "Owner-only projects cannot use lost-owner recovery. Invite at least one participant before relying on unanimous recovery."
                .into(),
        )
    } else if active
        .as_ref()
        .is_none_or(|row| row.4 != "active" || row.2 != membership_epoch)
    {
        Some(
            "Recovery is unprovisioned for the current membership epoch. Provision and activate a new n-of-n share set before starting recovery. One unreachable participant makes recovery impossible."
                .into(),
        )
    } else {
        None
    };
    let provisioned = active
        .as_ref()
        .is_some_and(|row| row.4 == "active" && row.2 == membership_epoch);
    let next_recovery_epoch = if provisioned {
        recovery_epoch
    } else if active
        .as_ref()
        .is_some_and(|row| row.4 == "active" && row.2 != membership_epoch)
    {
        recovery_epoch + 1
    } else {
        active.as_ref().map(|row| row.1).unwrap_or(recovery_epoch)
    };
    Ok(ProjectRecoveryProvisionStatusDto {
        recovery_set_id: active.as_ref().map(|row| row.0),
        recovery_epoch: next_recovery_epoch,
        membership_epoch,
        share_count: active
            .as_ref()
            .map(|row| row.3 as u16)
            .unwrap_or(non_owner_count as u16),
        state: if provisioned {
            "active".into()
        } else if !recoverable {
            "unrecoverable".into()
        } else {
            "recovery_unprovisioned".into()
        },
        secret_commitment_b64: active
            .as_ref()
            .filter(|row| row.4 == "active" && row.2 == membership_epoch)
            .map(|row| encode(&row.5)),
        holder_identity_ids: holders,
        provisioned,
        recoverable,
        warning,
    })
}

async fn require_owner(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    identity_id: Uuid,
) -> Result<(), AppError> {
    let role = sqlx::query_scalar::<_, String>(
        r#"
        SELECT role FROM project_memberships
        WHERE project_id = $1 AND identity_id = $2 AND state = 'active'
        "#,
    )
    .bind(project_id)
    .bind(identity_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    if role != "owner" {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn upsert_draft_recovery_set(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    request: &ProvisionProjectRecoveryRequest,
) -> Result<(), AppError> {
    if request.shares.is_empty() {
        return Err(AppError::BadRequest("recovery provision requires shares"));
    }
    let secret_commitment = decode_fixed(
        &request.secret_commitment_b64,
        32,
        "invalid recovery secret commitment",
    )?;
    let context_hash = decode_fixed(
        &request.context_hash_b64,
        32,
        "invalid recovery provision context hash",
    )?;
    let escrow = decode(&request.encrypted_owner_key_escrow_b64)?;
    if escrow.is_empty() {
        return Err(AppError::BadRequest("owner key escrow ciphertext is empty"));
    }
    let (membership_epoch, recovery_epoch) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT membership_epoch, recovery_epoch
        FROM projects
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    if request.membership_epoch != membership_epoch {
        return Err(AppError::Conflict);
    }
    let active_membership = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT membership_epoch FROM project_recovery_sets
        WHERE project_id = $1 AND state = 'active'
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let expected_recovery_epoch = match active_membership {
        Some(active_membership_epoch) if active_membership_epoch != membership_epoch => {
            recovery_epoch + 1
        }
        _ => recovery_epoch,
    };
    if request.recovery_epoch != expected_recovery_epoch {
        return Err(AppError::Conflict);
    }
    let non_owners = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT identity_id
        FROM project_memberships
        WHERE project_id = $1 AND role <> 'owner' AND state = 'active'
        ORDER BY identity_id
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut **transaction)
    .await?;
    if non_owners.is_empty() {
        return Err(AppError::BadRequest(
            "owner-only projects cannot provision lost-owner recovery",
        ));
    }
    let holder_ids = request
        .shares
        .iter()
        .map(|share| share.holder_identity_id)
        .collect::<HashSet<_>>();
    let expected_holders = non_owners.iter().copied().collect::<HashSet<_>>();
    if holder_ids != expected_holders {
        return Err(AppError::BadRequest(
            "recovery shares must cover every active non-owner participant exactly once by identity",
        ));
    }
    let indexes = request
        .shares
        .iter()
        .map(|share| share.share_index)
        .collect::<HashSet<_>>();
    if indexes.len() != non_owners.len()
        || indexes
            .iter()
            .any(|index| *index == 0 || usize::from(*index) > non_owners.len())
    {
        return Err(AppError::BadRequest(
            "recovery share indexes must be a contiguous 1..=n set",
        ));
    }
    let share_count = i16::try_from(non_owners.len())
        .map_err(|_| AppError::BadRequest("recovery participant count is out of range"))?;
    let existing_state = sqlx::query_scalar::<_, String>(
        r#"
        SELECT state FROM project_recovery_sets
        WHERE project_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(request.recovery_set_id)
    .fetch_optional(&mut **transaction)
    .await?;
    match existing_state.as_deref() {
        None => {
            sqlx::query(
                r#"
                INSERT INTO project_recovery_sets (
                    id, project_id, recovery_epoch, membership_epoch,
                    created_by_identity_id, share_count, threshold,
                    secret_commitment, context_hash, encrypted_owner_key_escrow, state
                )
                VALUES ($1, $2, $3, $4, $5, $6, $6, $7, $8, $9, 'draft')
                "#,
            )
            .bind(request.recovery_set_id)
            .bind(project_id)
            .bind(request.recovery_epoch)
            .bind(request.membership_epoch)
            .bind(actor.identity_id)
            .bind(share_count)
            .bind(&secret_commitment)
            .bind(&context_hash)
            .bind(&escrow)
            .execute(&mut **transaction)
            .await?;
        }
        Some("draft") => {
            sqlx::query(
                r#"
                UPDATE project_recovery_sets
                SET recovery_epoch = $3,
                    membership_epoch = $4,
                    share_count = $5,
                    threshold = $5,
                    secret_commitment = $6,
                    context_hash = $7,
                    encrypted_owner_key_escrow = $8
                WHERE project_id = $1 AND id = $2 AND state = 'draft'
                "#,
            )
            .bind(project_id)
            .bind(request.recovery_set_id)
            .bind(request.recovery_epoch)
            .bind(request.membership_epoch)
            .bind(share_count)
            .bind(&secret_commitment)
            .bind(&context_hash)
            .bind(&escrow)
            .execute(&mut **transaction)
            .await?;
            sqlx::query(
                r#"
                DELETE FROM project_recovery_shares
                WHERE project_id = $1 AND recovery_set_id = $2
                "#,
            )
            .bind(project_id)
            .bind(request.recovery_set_id)
            .execute(&mut **transaction)
            .await?;
        }
        Some(_) => return Err(AppError::Conflict),
    }
    for share in &request.shares {
        insert_share(transaction, project_id, request.recovery_set_id, share).await?;
    }
    Ok(())
}

async fn insert_share(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    recovery_set_id: Uuid,
    share: &ProjectRecoveryShareInputDto,
) -> Result<(), AppError> {
    let encrypted_share = decode(&share.encrypted_share_b64)?;
    if encrypted_share.is_empty() {
        return Err(AppError::BadRequest("encrypted recovery share is empty"));
    }
    let share_commitment = decode_fixed(
        &share.share_commitment_b64,
        32,
        "invalid recovery share commitment",
    )?;
    let share_index = i16::try_from(share.share_index)
        .map_err(|_| AppError::BadRequest("invalid recovery share index"))?;
    sqlx::query(
        r#"
        INSERT INTO project_recovery_shares (
            id, project_id, recovery_set_id, share_index,
            holder_identity_id, holder_device_id, holder_device_key_version,
            encrypted_share, share_commitment
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(share.share_id)
    .bind(project_id)
    .bind(recovery_set_id)
    .bind(share_index)
    .bind(share.holder_identity_id)
    .bind(share.holder_device_id)
    .bind(share.holder_device_key_version)
    .bind(encrypted_share)
    .bind(share_commitment)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn activate_recovery_set(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    recovery_set_id: Uuid,
) -> Result<(), AppError> {
    let (membership_epoch, recovery_epoch) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT membership_epoch, recovery_epoch
        FROM projects
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    let draft = sqlx::query_as::<_, (i64, i64, String)>(
        r#"
        SELECT membership_epoch, recovery_epoch, state
        FROM project_recovery_sets
        WHERE project_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(recovery_set_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if draft.2 != "draft" || draft.0 != membership_epoch {
        return Err(AppError::Conflict);
    }
    if draft.1 != recovery_epoch && draft.1 != recovery_epoch + 1 {
        return Err(AppError::Conflict);
    }
    sqlx::query(
        r#"
        UPDATE project_recovery_sets
        SET state = 'retired', retired_at = clock_timestamp()
        WHERE project_id = $1 AND state = 'active'
        "#,
    )
    .bind(project_id)
    .execute(&mut **transaction)
    .await?;
    if draft.1 == recovery_epoch + 1 {
        sqlx::query(
            r#"
            UPDATE projects
            SET recovery_epoch = $2
            WHERE id = $1 AND recovery_epoch = $3
            "#,
        )
        .bind(project_id)
        .bind(draft.1)
        .bind(recovery_epoch)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        r#"
        UPDATE project_recovery_sets
        SET state = 'active', activated_at = clock_timestamp(), retired_at = NULL
        WHERE project_id = $1 AND id = $2 AND state = 'draft'
        "#,
    )
    .bind(project_id)
    .bind(recovery_set_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn rotate_for_recovery(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    sender_key_version: i32,
    rotations: &[ResourceEpochRotationDto],
) -> Result<(), AppError> {
    let active_resources = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT resource_node_id
        FROM resource_epochs
        WHERE project_id = $1 AND retired_at IS NULL
        ORDER BY resource_node_id
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut **transaction)
    .await?;
    let expected = active_resources.into_iter().collect::<HashSet<_>>();
    let supplied = rotations
        .iter()
        .map(|rotation| rotation.resource_id)
        .collect::<HashSet<_>>();
    if expected != supplied || supplied.len() != rotations.len() {
        return Err(AppError::BadRequest(
            "recovery rotations must cover every active resource",
        ));
    }
    let recipients = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT identity_id FROM project_memberships
        WHERE project_id = $1 AND state = 'active'
        ORDER BY identity_id
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut **transaction)
    .await?;
    for rotation in rotations {
        let (previous_epoch_id, previous_epoch, has_header_key) =
            sqlx::query_as::<_, (Uuid, i32, bool)>(
                r#"
                SELECT id, epoch, header_key_commitment IS NOT NULL
                FROM resource_epochs
                WHERE project_id = $1 AND resource_node_id = $2 AND retired_at IS NULL
                FOR UPDATE
                "#,
            )
            .bind(project_id)
            .bind(rotation.resource_id)
            .fetch_one(&mut **transaction)
            .await?;
        if rotation.previous_epoch_id != previous_epoch_id
            || i32::try_from(rotation.new_epoch).ok() != Some(previous_epoch + 1)
            || i32::try_from(rotation.creator_device_key_version).ok() != Some(sender_key_version)
        {
            return Err(AppError::Conflict);
        }
        let decoded = decode_envelopes(&rotation.envelopes)?;
        verify_envelope_signatures(transaction, actor, project_id, &decoded).await?;
        let mut covered = HashSet::new();
        for recipient in &recipients {
            let devices = active_device_keys(transaction, project_id, *recipient).await?;
            if devices.is_empty() {
                continue;
            }
            covered.insert(*recipient);
            let recipient_envelopes = decoded
                .iter()
                .filter(|envelope| envelope.recipient_identity_id == *recipient)
                .cloned()
                .collect::<Vec<_>>();
            validate_envelope_coverage(
                *recipient,
                sender_key_version,
                &[ActiveResourceEpoch {
                    resource_id: rotation.resource_id,
                    epoch: previous_epoch + 1,
                }],
                &devices,
                &recipient_envelopes,
            )?;
        }
        let supplied_recipients = decoded
            .iter()
            .map(|envelope| envelope.recipient_identity_id)
            .collect::<HashSet<_>>();
        if supplied_recipients != covered {
            return Err(AppError::BadRequest(
                "recovery envelopes do not exactly cover active devices",
            ));
        }
        let commitment = decode(&rotation.key_commitment_b64)?;
        if commitment.len() < 16 {
            return Err(AppError::BadRequest("resource key commitment is too short"));
        }
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
        sqlx::query(
            "UPDATE resource_epochs SET retired_at = clock_timestamp()
             WHERE project_id = $1 AND resource_node_id = $2 AND id = $3",
        )
        .bind(project_id)
        .bind(rotation.resource_id)
        .bind(previous_epoch_id)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "UPDATE resource_key_envelopes SET revoked_at = clock_timestamp()
             WHERE project_id = $1 AND resource_node_id = $2
               AND epoch = $3 AND revoked_at IS NULL",
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
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'recovery')
            "#,
        )
        .bind(rotation.epoch_id)
        .bind(project_id)
        .bind(rotation.resource_id)
        .bind(previous_epoch + 1)
        .bind(previous_epoch_id)
        .bind(actor.identity_id)
        .bind(actor.device_id)
        .bind(sender_key_version)
        .bind(commitment)
        .bind(header_commitment)
        .execute(&mut **transaction)
        .await?;
        store_envelopes(transaction, actor, project_id, &decoded).await?;
    }
    Ok(())
}

fn approval_signing_bytes(
    recovery: &RecoveryRow,
    approver_identity_id: Uuid,
    encrypted_share: &[u8],
) -> Vec<u8> {
    let mut message = approval_signing_prefix_bytes(
        recovery.project_id,
        recovery.id,
        recovery.requester_identity_id,
        approver_identity_id,
        recovery.membership_epoch,
        recovery.recovery_epoch,
        recovery.recovery_set_id,
        &recovery.secret_commitment,
        &recovery.challenge,
        &recovery.context_hash,
    );
    message.extend_from_slice(Sha256::digest(encrypted_share).as_slice());
    message
}

#[allow(clippy::too_many_arguments)]
fn approval_signing_prefix_bytes(
    project_id: Uuid,
    request_id: Uuid,
    requester_identity_id: Uuid,
    approver_identity_id: Uuid,
    membership_epoch: i64,
    recovery_epoch: i64,
    recovery_set_id: Uuid,
    secret_commitment: &[u8],
    challenge: &[u8],
    context_hash: &[u8],
) -> Vec<u8> {
    let mut message = Vec::with_capacity(256);
    message.extend_from_slice(b"sprout-project-recovery-approval-v1");
    message.extend_from_slice(project_id.as_bytes());
    message.extend_from_slice(request_id.as_bytes());
    message.extend_from_slice(requester_identity_id.as_bytes());
    message.extend_from_slice(approver_identity_id.as_bytes());
    message.extend_from_slice(&membership_epoch.to_be_bytes());
    message.extend_from_slice(&recovery_epoch.to_be_bytes());
    message.extend_from_slice(recovery_set_id.as_bytes());
    message.extend_from_slice(secret_commitment);
    message.extend_from_slice(challenge);
    message.extend_from_slice(context_hash);
    message
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 recovery payload"))
}

fn encode(value: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn decode_fixed(value: &str, length: usize, message: &'static str) -> Result<Vec<u8>, AppError> {
    let decoded = decode(value)?;
    if decoded.len() != length {
        return Err(AppError::BadRequest(message));
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_prefix_binds_recovery_epoch_and_set() {
        let project_id = Uuid::nil();
        let request_id = Uuid::from_u128(1);
        let requester = Uuid::from_u128(2);
        let approver = Uuid::from_u128(3);
        let set_id = Uuid::from_u128(4);
        let commitment = [9u8; 32];
        let challenge = [7u8; 32];
        let context = [8u8; 32];
        let prefix = approval_signing_prefix_bytes(
            project_id,
            request_id,
            requester,
            approver,
            3,
            5,
            set_id,
            &commitment,
            &challenge,
            &context,
        );
        assert!(prefix.windows(8).any(|window| window == 5i64.to_be_bytes()));
        assert!(prefix.windows(16).any(|window| window == set_id.as_bytes()));
        assert!(prefix.windows(32).any(|window| window == commitment));
    }
}
