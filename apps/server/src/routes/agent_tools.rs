use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sprout_api_contract::EncryptedPayloadDto;
use sprout_crypto_protocol::{canonical_governance_json, verify_ed25519_ml_dsa65_signatures};
use sprout_domain::{
    AgentActionClass, ClaimStatus, ConcreteWorkAuthorityEvidence, ExactWorkAuthorityOrigin,
    ExternalToolAuthorization, ExternalToolCallRecord, ExternalToolCallStatus, ToolCallId, UserId,
    WorkKind, WorkStatus, initial_tool_required_effects, resolve_exact_work_authority_origin,
    validate_external_tool_authorization,
};
use sprout_storage_postgres::validate_experimental_wrapped_resource_key;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use super::agent_runs::{
    TransitionMetadata, authoritative_condition_facts, begin, lock_run, persist_transition,
    require_active_runner, require_current_run_authority, runtime_tick, tick_datetime,
};
use crate::{
    AppState,
    auth::{AuthSession, ResourceAccess, require_resource_access},
    error::AppError,
};

const TOOL_LEASE_SECONDS: i64 = 60;

#[derive(Serialize)]
pub struct CatalogResponse {
    version: u32,
    tools: Vec<sprout_domain::ExternalToolCatalogEntry>,
}

pub async fn catalog(_actor: AuthSession) -> Json<CatalogResponse> {
    Json(CatalogResponse {
        version: sprout_domain::EXTERNAL_TOOL_CATALOG_VERSION,
        tools: sprout_domain::EXTERNAL_TOOL_CATALOG.to_vec(),
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCapabilityRequest {
    witness_id: Uuid,
    tool_id: String,
    tool_version: u32,
    execution_profile_commitment_hex: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    idempotency_key: Uuid,
    signatures: super::agents::CompilationSignatures,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeCapabilityStatement<'a> {
    signature_context: &'static str,
    witness_id: Uuid,
    project_id: Uuid,
    agent_id: Uuid,
    owner_identity_id: Uuid,
    runner_id: Uuid,
    tool_id: &'a str,
    tool_version: u32,
    manifest_hash_hex: String,
    profile_tool_available: bool,
    runtime_available: bool,
    execution_profile_commitment_hex: &'a str,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    idempotency_key: Uuid,
}

#[derive(Serialize)]
pub struct RuntimeCapabilityResponse {
    witness_id: Uuid,
    replayed: bool,
}

pub async fn attest_runtime_capability(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RuntimeCapabilityRequest>,
) -> Result<Json<RuntimeCapabilityResponse>, AppError> {
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let profile = commitment(&request.execution_profile_commitment_hex)?;
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let row = sqlx::query(
        r#"
        SELECT runner.id AS runner_id, catalog.manifest_hash
        FROM governed_agents agent
        JOIN agent_runners runner
          ON runner.project_id = agent.project_id AND runner.agent_id = agent.id
         AND runner.principal_identity_id = agent.principal_identity_id
         AND runner.device_id = $4 AND runner.state = 'active'
        JOIN agent_external_tool_catalog catalog
          ON catalog.tool_name = $5 AND catalog.version = $6
         AND catalog.availability = 'executable'
        WHERE agent.project_id = $1 AND agent.id = $2
          AND agent.principal_identity_id = $3 AND agent.state = 'active'
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(&request.tool_id)
    .bind(
        i32::try_from(request.tool_version)
            .map_err(|_| AppError::BadRequest("invalid tool version"))?,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let runner_id: Uuid = row.try_get("runner_id")?;
    let manifest_hash: Vec<u8> = row.try_get("manifest_hash")?;
    let now = Utc::now();
    if request.issued_at > now
        || request.expires_at <= now
        || request.expires_at - request.issued_at > Duration::minutes(5)
    {
        return Err(AppError::BadRequest("invalid runtime capability lifetime"));
    }
    let statement = RuntimeCapabilityStatement {
        signature_context: "sprout-external-tool-runtime-capability-v1",
        witness_id: request.witness_id,
        project_id,
        agent_id,
        owner_identity_id: actor.identity_id,
        runner_id,
        tool_id: &request.tool_id,
        tool_version: request.tool_version,
        manifest_hash_hex: hex::encode(&manifest_hash),
        profile_tool_available: true,
        runtime_available: true,
        execution_profile_commitment_hex: &request.execution_profile_commitment_hex,
        issued_at: request.issued_at,
        expires_at: request.expires_at,
        idempotency_key: request.idempotency_key,
    };
    let statement_hash = super::agents::verify_device_statement(
        &mut transaction,
        actor,
        &statement,
        &request.signatures,
        b"sprout-external-tool-runtime-capability-v1",
    )
    .await?;
    if let Some(existing) = sqlx::query(
        "SELECT id, statement_hash FROM agent_tool_runtime_capability_witnesses
         WHERE project_id=$1 AND owner_identity_id=$2 AND idempotency_key=$3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.try_get::<Vec<u8>, _>("statement_hash")? != statement_hash {
            return Err(AppError::Conflict);
        }
        let witness_id = existing.try_get("id")?;
        transaction.commit().await?;
        return Ok(Json(RuntimeCapabilityResponse {
            witness_id,
            replayed: true,
        }));
    }
    sqlx::query(
        r#"
        INSERT INTO agent_tool_runtime_capability_witnesses (
          id, project_id, agent_id, owner_identity_id, runner_id,
          signer_device_id, signer_device_key_version, tool_name, tool_version,
          manifest_hash, execution_profile_commitment, issued_at, expires_at,
          classical_signature, post_quantum_signature, statement_hash, idempotency_key
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
        "#,
    )
    .bind(request.witness_id)
    .bind(project_id)
    .bind(agent_id)
    .bind(actor.identity_id)
    .bind(runner_id)
    .bind(request.signatures.signer_device_id)
    .bind(
        i32::try_from(request.signatures.signer_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid signer key version"))?,
    )
    .bind(&request.tool_id)
    .bind(
        i32::try_from(request.tool_version)
            .map_err(|_| AppError::BadRequest("invalid tool version"))?,
    )
    .bind(manifest_hash)
    .bind(profile.as_slice())
    .bind(request.issued_at)
    .bind(request.expires_at)
    .bind(&request.signatures.classical_signature)
    .bind(&request.signatures.post_quantum_signature)
    .bind(statement_hash.as_slice())
    .bind(request.idempotency_key)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(RuntimeCapabilityResponse {
        witness_id: request.witness_id,
        replayed: false,
    }))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantPermissionRequest {
    id: Uuid,
    tool_version: u32,
    idempotency_key: Uuid,
}

#[derive(Serialize)]
pub struct PermissionResponse {
    id: Uuid,
    active: bool,
    replayed: bool,
}

pub async fn grant_permission(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, tool_id, tool_version)): Path<(Uuid, Uuid, String, u32)>,
    Json(request): Json<GrantPermissionRequest>,
) -> Result<Json<PermissionResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    if request.tool_version != tool_version {
        return Err(AppError::BadRequest("tool version path/body mismatch"));
    }
    let catalog = sprout_domain::EXTERNAL_TOOL_CATALOG
        .iter()
        .find(|entry| entry.id == tool_id && entry.version == request.tool_version)
        .ok_or(AppError::BadRequest("unknown external tool"))?;
    if catalog.availability == sprout_domain::ExternalToolAvailability::FailClosed {
        return Err(AppError::BadRequest("external send surface is fail closed"));
    }
    let row = sqlx::query(
        "SELECT principal_identity_id, controller_identity_id, profile_resource_node_id
         FROM governed_agents WHERE project_id = $1 AND id = $2 AND state = 'active'",
    )
    .bind(project_id)
    .bind(agent_id)
    .fetch_optional(&app.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let agent_identity: Uuid = row.try_get("principal_identity_id")?;
    let controller: Uuid = row.try_get("controller_identity_id")?;
    let profile: Uuid = row.try_get("profile_resource_node_id")?;
    if controller != actor.identity_id {
        return Err(AppError::Forbidden);
    }
    require_resource_access(
        &app.pool,
        actor,
        project_id,
        profile,
        ResourceAccess::Manage,
    )
    .await?;
    let grant_hash = digest(&json!({
        "id": request.id,
        "project_id": project_id,
        "agent_id": agent_id,
        "agent_identity_id": agent_identity,
        "tool_id": tool_id,
        "tool_version": request.tool_version,
        "granted_by": actor.identity_id,
        "idempotency_key": request.idempotency_key,
    }))?;
    let mut transaction = begin(&app, actor, project_id).await?;
    let result = sqlx::query(
        "SELECT permission_id, replayed, active
         FROM sprout_private.grant_agent_tool_permission(
             $1,$2,$3,$4,$5,$6,$7,$8,$9,$10
         )",
    )
    .bind(request.id)
    .bind(project_id)
    .bind(profile)
    .bind(agent_identity)
    .bind(agent_id)
    .bind(&tool_id)
    .bind(
        i32::try_from(request.tool_version)
            .map_err(|_| AppError::BadRequest("invalid tool version"))?,
    )
    .bind(actor.identity_id)
    .bind(request.idempotency_key)
    .bind(&grant_hash)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(PermissionResponse {
        id: result.try_get("permission_id")?,
        active: result.try_get("active")?,
        replayed: result.try_get("replayed")?,
    }))
}

pub async fn revoke_permission(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, tool_id, tool_version)): Path<(Uuid, Uuid, String, u32)>,
) -> Result<Json<PermissionResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let row = sqlx::query(
        "SELECT principal_identity_id, controller_identity_id, profile_resource_node_id
         FROM governed_agents WHERE project_id = $1 AND id = $2 AND state = 'active'",
    )
    .bind(project_id)
    .bind(agent_id)
    .fetch_optional(&app.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.try_get::<Uuid, _>("controller_identity_id")? != actor.identity_id {
        return Err(AppError::Forbidden);
    }
    require_resource_access(
        &app.pool,
        actor,
        project_id,
        row.try_get("profile_resource_node_id")?,
        ResourceAccess::Manage,
    )
    .await?;
    let mut transaction = begin(&app, actor, project_id).await?;
    let agent_identity_id: Uuid = row.try_get("principal_identity_id")?;
    let result = sqlx::query(
        "SELECT permission_id, replayed, active
         FROM sprout_private.revoke_agent_tool_permission($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(project_id)
    .bind(row.try_get::<Uuid, _>("profile_resource_node_id")?)
    .bind(agent_identity_id)
    .bind(agent_id)
    .bind(&tool_id)
    .bind(i32::try_from(tool_version).map_err(|_| AppError::BadRequest("invalid tool version"))?)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(PermissionResponse {
        id: result.try_get("permission_id")?,
        active: result.try_get("active")?,
        replayed: result.try_get("replayed")?,
    }))
}

/// Product-authorization path for the human run sponsor's explicit tool
/// permission. Project role alone is insufficient: the grantor must currently
/// hold `manage` on the exact resource scope that the permission is for.
pub async fn grant_principal_permission(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, scope_id, principal_id, tool_id, tool_version)): Path<(
        Uuid,
        Uuid,
        Uuid,
        String,
        u32,
    )>,
    Json(request): Json<GrantPermissionRequest>,
) -> Result<Json<PermissionResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    if request.tool_version != tool_version {
        return Err(AppError::BadRequest("tool version path/body mismatch"));
    }
    let manifest = sprout_domain::external_tool_catalog_entry(&tool_id, request.tool_version)
        .ok_or(AppError::BadRequest("unknown external tool"))?;
    if manifest.availability == sprout_domain::ExternalToolAvailability::FailClosed {
        return Err(AppError::BadRequest("external send surface is fail closed"));
    }
    require_resource_access(
        &app.pool,
        actor,
        project_id,
        scope_id,
        ResourceAccess::Manage,
    )
    .await?;
    if !sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM project_memberships
         WHERE project_id=$1 AND identity_id=$2)",
    )
    .bind(project_id)
    .bind(principal_id)
    .fetch_one(&app.pool)
    .await?
    {
        return Err(AppError::Forbidden);
    }
    let grant_hash = digest(&json!({
        "id": request.id,
        "project_id": project_id,
        "scope_id": scope_id,
        "principal_identity_id": principal_id,
        "tool_id": tool_id,
        "tool_version": request.tool_version,
        "granted_by": actor.identity_id,
        "idempotency_key": request.idempotency_key,
    }))?;
    let mut transaction = begin(&app, actor, project_id).await?;
    let result = sqlx::query(
        "SELECT permission_id, replayed, active
         FROM sprout_private.grant_agent_tool_permission(
             $1,$2,$3,$4,NULL,$5,$6,$7,$8,$9
         )",
    )
    .bind(request.id)
    .bind(project_id)
    .bind(scope_id)
    .bind(principal_id)
    .bind(&tool_id)
    .bind(
        i32::try_from(request.tool_version)
            .map_err(|_| AppError::BadRequest("invalid tool version"))?,
    )
    .bind(actor.identity_id)
    .bind(request.idempotency_key)
    .bind(grant_hash)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(PermissionResponse {
        id: result.try_get("permission_id")?,
        active: result.try_get("active")?,
        replayed: result.try_get("replayed")?,
    }))
}

pub async fn revoke_principal_permission(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, scope_id, principal_id, tool_id, tool_version)): Path<(
        Uuid,
        Uuid,
        Uuid,
        String,
        u32,
    )>,
) -> Result<Json<PermissionResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_resource_access(
        &app.pool,
        actor,
        project_id,
        scope_id,
        ResourceAccess::Manage,
    )
    .await?;
    let mut transaction = begin(&app, actor, project_id).await?;
    let result = sqlx::query(
        "SELECT permission_id, replayed, active
         FROM sprout_private.revoke_agent_tool_permission($1,$2,$3,NULL,$4,$5,$6)",
    )
    .bind(project_id)
    .bind(scope_id)
    .bind(principal_id)
    .bind(&tool_id)
    .bind(i32::try_from(tool_version).map_err(|_| AppError::BadRequest("invalid tool version"))?)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(PermissionResponse {
        id: result.try_get("permission_id")?,
        active: result.try_get("active")?,
        replayed: result.try_get("replayed")?,
    }))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeToolRequest {
    id: Uuid,
    tool_id: String,
    tool_version: u32,
    runtime_capability_witness_id: Uuid,
    encrypted_input: EncryptedPayloadDto,
    structured_input_commitment_hex: String,
    max_attempts: u16,
    timeout_seconds: u32,
    idempotency_key: Uuid,
    signatures: super::agents::CompilationSignatures,
}

#[derive(Serialize)]
struct ToolInputStatement<'a> {
    signature_context: &'static str,
    project_id: Uuid,
    run_id: Uuid,
    goal_id: Uuid,
    work_item_id: Uuid,
    claim_id: Uuid,
    attempt: u16,
    owner_identity_id: Uuid,
    tool_id: &'a str,
    tool_version: u32,
    runtime_capability_witness_id: Uuid,
    encrypted_input_payload_commitment_hex: String,
    structured_input_commitment_hex: &'a str,
    idempotency_key: Uuid,
}

#[derive(Serialize)]
pub struct ToolCallResponse {
    id: Uuid,
    status: &'static str,
    attempt: u16,
    replayed: bool,
}

pub async fn invoke(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, claim_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<InvokeToolRequest>,
) -> Result<Json<ToolCallResponse>, AppError> {
    let structured_input_commitment = commitment(&request.structured_input_commitment_hex)?;
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_authority(&mut transaction, project_id, actor, &locked.state).await?;
    let tick = runtime_tick()?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    let claim = locked
        .state
        .claims
        .get(&claim_id.into())
        .ok_or(AppError::Conflict)?;
    let work = locked
        .state
        .work_items
        .get(&claim.work)
        .ok_or(AppError::Conflict)?;
    let spec = locked
        .contract
        .work_specs
        .iter()
        .find(|candidate| candidate.id == work.work_spec_id)
        .ok_or(AppError::Conflict)?;
    let obligation = locked
        .contract
        .obligations
        .iter()
        .find(|candidate| candidate.id == work.serves)
        .ok_or(AppError::Conflict)?;
    if claim.claimant != UserId::from(actor.identity_id)
        || claim.status != ClaimStatus::Active
        || claim.acquired_at > tick
        || claim.expires_at <= tick
        || claim.attempt != work.attempt
        || work.status != WorkStatus::Claimed
        || work.owner != UserId::from(actor.identity_id)
        || work.goal != locked.contract.goal
        || !spec.activation.holds(&facts)
        || !obligation.required_for_completion.holds(&facts)
        || work.kind != WorkKind::ToolInvocation
        || !spec.allowed_actions.contains(&AgentActionClass::InvokeTool)
    {
        return Err(AppError::Forbidden);
    }
    let snapshot =
        load_tool_security_snapshot(&mut transaction, project_id, run_id, &locked.state, work)
            .await?;
    let policy = snapshot.policy.clone();
    let work_ceiling = snapshot.work_ceiling.clone();
    let run_ceiling = snapshot.run_ceiling.clone();
    let actor_permission_current = current_tool_permission(
        &mut transaction,
        project_id,
        actor.identity_id,
        &request.tool_id,
        request.tool_version,
    )
    .await?;
    let authority_permission_current = current_tool_permission(
        &mut transaction,
        project_id,
        snapshot.work_authority_principal,
        &request.tool_id,
        request.tool_version,
    )
    .await?;
    let required_effects = initial_tool_required_effects(&request.tool_id, request.tool_version)
        .ok_or(AppError::BadRequest(
            "unknown or non-executable external tool",
        ))?;
    // Catalog v1 has the trusted `owner_only` output semantics. This audience
    // is derived here and is never accepted from the caller/model.
    let output_readable_by = vec![UserId::from(actor.identity_id)];
    let output_readable_by_ids = vec![actor.identity_id];
    let encrypted_input = canonical_governance_json(&request.encrypted_input)
        .map_err(|_| AppError::BadRequest("invalid encrypted input"))?;
    let encrypted_input_payload_commitment: [u8; 32] = Sha256::digest(&encrypted_input).into();
    let input_statement = ToolInputStatement {
        signature_context: "sprout-external-tool-input-v1",
        project_id,
        run_id,
        goal_id: Uuid::from(work.goal),
        work_item_id: Uuid::from(work.id),
        claim_id,
        attempt: claim.attempt,
        owner_identity_id: actor.identity_id,
        tool_id: &request.tool_id,
        tool_version: request.tool_version,
        runtime_capability_witness_id: request.runtime_capability_witness_id,
        encrypted_input_payload_commitment_hex: hex::encode(encrypted_input_payload_commitment),
        structured_input_commitment_hex: &request.structured_input_commitment_hex,
        idempotency_key: request.idempotency_key,
    };
    let canonical_input_commitment = super::agents::verify_device_statement(
        &mut transaction,
        actor,
        &input_statement,
        &request.signatures,
        b"sprout-external-tool-input-v1",
    )
    .await?;
    let canonical_input_statement = String::from_utf8(
        canonical_governance_json(&input_statement)
            .map_err(|_| AppError::BadRequest("invalid canonical tool input"))?,
    )
    .map_err(|_| AppError::Internal)?;
    let exact_runtime_capability = current_runtime_capability(
        &mut transaction,
        project_id,
        actor,
        request.runtime_capability_witness_id,
        &request.tool_id,
        request.tool_version,
    )
    .await?;
    let call = ExternalToolCallRecord {
        id: ToolCallId::from(request.id),
        run: run_id.into(),
        goal: work.goal,
        work: work.id,
        claim: claim_id.into(),
        work_attempt: claim.attempt,
        owner: UserId::from(actor.identity_id),
        tool: request.tool_id.clone(),
        tool_version: request.tool_version,
        encrypted_input_payload_commitment,
        canonical_input_commitment,
        attempt: claim.attempt,
        max_attempts: request.max_attempts,
        timeout_seconds: request.timeout_seconds,
        requested_at: tick,
        tool_deadline_at: tick.saturating_add(u64::from(request.timeout_seconds)),
        status: ExternalToolCallStatus::Pending,
        canonical_output_commitment: None,
        failure_code: None,
    };
    call.validate()
        .map_err(|_| AppError::BadRequest("invalid tool call"))?;
    let authorization = ExternalToolAuthorization {
        call: &call,
        work,
        work_spec: spec,
        policy: &policy,
        run_tool_ceiling: &run_ceiling,
        work_tool_ceiling: &work_ceiling,
        current_authority_tool_permission: authority_permission_current,
        current_actor_tool_permission: actor_permission_current,
        claimed_by_owner: true,
        exact_runtime_capability,
        required_effects: &required_effects,
        expected_required_effects: &required_effects,
        required_effects_currently_authorized: true,
        output_readable_by: &output_readable_by,
        owner_can_read_all_sources: true,
        claim_acquired_at: claim.acquired_at,
        claim_expires_at: claim.expires_at,
    };
    validate_external_tool_authorization(&authorization).map_err(|_| AppError::Forbidden)?;
    let request_hash_for = |exact_call: &ExternalToolCallRecord| {
        digest(&json!({
            "call": exact_call,
            "run_id": run_id,
            "goal_id": Uuid::from(work.goal),
            "work_item_id": Uuid::from(work.id),
            "work_claim_id": claim_id,
            "work_spec_id": spec.id,
            "encrypted_input_payload_commitment": hex::encode(encrypted_input_payload_commitment),
            "structured_input_commitment": hex::encode(structured_input_commitment),
            "canonical_input_commitment": hex::encode(canonical_input_commitment),
            "runtime_capability_witness_id": request.runtime_capability_witness_id,
            "security_policy": policy,
            "run_tool_ceiling": run_ceiling,
            "work_tool_ceiling": work_ceiling,
            "required_effects": required_effects,
            "output_readable_by": &output_readable_by_ids,
            "idempotency_key": request.idempotency_key,
        }))
    };
    let request_hash = request_hash_for(&call)?;
    if let Some(existing) = sqlx::query(
        "SELECT id, request_hash, current_status, current_attempt,
                requested_tick, tool_deadline_tick
         FROM agent_tool_calls
         WHERE project_id = $1 AND owner_identity_id = $2 AND idempotency_key = $3
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *transaction)
    .await?
    {
        let mut replay_call = call.clone();
        replay_call.requested_at = u64::try_from(existing.try_get::<i64, _>("requested_tick")?)
            .map_err(|_| AppError::Internal)?;
        replay_call.tool_deadline_at =
            u64::try_from(existing.try_get::<i64, _>("tool_deadline_tick")?)
                .map_err(|_| AppError::Internal)?;
        if existing.try_get::<Vec<u8>, _>("request_hash")? != request_hash_for(&replay_call)? {
            return Err(AppError::Conflict);
        }
        let response = ToolCallResponse {
            id: existing.try_get("id")?,
            status: status_response(existing.try_get("current_status")?),
            attempt: u16::try_from(existing.try_get::<i32, _>("current_attempt")?)
                .map_err(|_| AppError::Internal)?,
            replayed: true,
        };
        transaction.commit().await?;
        return Ok(Json(response));
    }
    let policy_hash = snapshot.policy_hash;
    let run_ceiling_hash = snapshot.run_ceiling_hash;
    let work_ceiling_hash = snapshot.work_ceiling_hash;
    let adapter_protocol =
        sprout_domain::external_tool_catalog_entry(&request.tool_id, request.tool_version)
            .ok_or(AppError::BadRequest("unknown external tool"))?
            .adapter_protocol;
    sqlx::query(
        "INSERT INTO agent_tool_calls (
            id, project_id, run_id, goal_id, work_item_id, work_claim_id,
            work_attempt, work_spec_ordinal, owner_identity_id,
            work_authority_origin, work_authority_parent_id,
            work_authority_principal_id, tool_name, tool_version,
            runtime_capability_witness_id, adapter_protocol,
            encrypted_input, encrypted_input_payload_commitment,
            canonical_input_commitment, canonical_input_statement,
            input_signer_device_id, input_signer_device_key_version,
            input_classical_signature, input_post_quantum_signature,
            security_policy_hash,
            run_tool_ceiling_hash, work_tool_ceiling_hash, work_tool_ceiling, required_effects,
            output_readable_by, max_attempts, timeout_seconds, current_attempt,
            idempotency_key, request_hash, requested_tick, requested_at,
            tool_deadline_tick, tool_deadline_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,
                   $17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,
                   $31,$32,$33,$34,$35,$36,$37,$38,$39)",
    )
    .bind(request.id)
    .bind(project_id)
    .bind(run_id)
    .bind(Uuid::from(work.goal))
    .bind(Uuid::from(work.id))
    .bind(claim_id)
    .bind(i32::from(claim.attempt))
    .bind(i64::try_from(spec.id).map_err(|_| AppError::Internal)?)
    .bind(actor.identity_id)
    .bind(snapshot.work_authority_origin)
    .bind(snapshot.work_authority_parent)
    .bind(snapshot.work_authority_principal)
    .bind(&request.tool_id)
    .bind(
        i32::try_from(request.tool_version)
            .map_err(|_| AppError::BadRequest("invalid tool version"))?,
    )
    .bind(request.runtime_capability_witness_id)
    .bind(adapter_protocol)
    .bind(encrypted_input)
    .bind(encrypted_input_payload_commitment.as_slice())
    .bind(canonical_input_commitment.as_slice())
    .bind(canonical_input_statement)
    .bind(request.signatures.signer_device_id)
    .bind(
        i32::try_from(request.signatures.signer_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid signer key version"))?,
    )
    .bind(&request.signatures.classical_signature)
    .bind(&request.signatures.post_quantum_signature)
    .bind(policy_hash.as_slice())
    .bind(run_ceiling_hash.as_slice())
    .bind(work_ceiling_hash.as_slice())
    .bind(serde_json::to_value(&work_ceiling).map_err(|_| AppError::Internal)?)
    .bind(serde_json::to_value(&required_effects).map_err(|_| AppError::Internal)?)
    .bind(json!(&output_readable_by_ids))
    .bind(i32::from(request.max_attempts))
    .bind(
        i32::try_from(request.timeout_seconds)
            .map_err(|_| AppError::BadRequest("invalid timeout"))?,
    )
    .bind(i32::from(claim.attempt))
    .bind(request.idempotency_key)
    .bind(&request_hash)
    .bind(i64::try_from(tick).map_err(|_| AppError::Internal)?)
    .bind(tick_datetime(tick)?)
    .bind(
        i64::try_from(tick.saturating_add(u64::from(request.timeout_seconds)))
            .map_err(|_| AppError::Internal)?,
    )
    .bind(tick_datetime(
        tick.saturating_add(u64::from(request.timeout_seconds)),
    )?)
    .execute(&mut *transaction)
    .await?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: "tool_attempt_opened",
            tick,
            observation: Some(("tool_attempt", request.id)),
        },
    )
    .await?;
    insert_audit(
        &mut transaction,
        AuditCoordinates::new(
            project_id,
            request.id,
            run_id,
            Uuid::from(work.goal),
            Uuid::from(work.id),
            claim_id,
            claim.attempt,
            actor.identity_id,
            &request.tool_id,
            request.tool_version,
            claim.attempt,
        ),
        "requested",
        "pending",
        canonical_input_commitment,
        request.max_attempts,
        request.timeout_seconds,
        None,
        request.idempotency_key,
    )
    .await?;
    project_r540_tool_attempt(
        &mut transaction,
        project_id,
        run_id,
        request.id,
        transition.id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ToolCallResponse {
        id: request.id,
        status: "pending",
        attempt: claim.attempt,
        replayed: false,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimToolRequest {
    dispatch_id: Uuid,
    lease_id: Uuid,
}

#[derive(Serialize)]
pub struct ClaimToolResponse {
    dispatch_id: Uuid,
    lease_id: Uuid,
    call_id: Uuid,
    attempt: u16,
    tool_id: String,
    tool_version: u32,
    encrypted_input: EncryptedPayloadDto,
    canonical_input_commitment_hex: String,
    adapter_protocol: String,
    lease_expires_at: DateTime<Utc>,
    tool_deadline_at: DateTime<Utc>,
}

pub async fn claim(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, call_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<ClaimToolRequest>,
) -> Result<Json<ClaimToolResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_authority(&mut transaction, project_id, actor, &locked.state).await?;
    let call = lock_call(&mut transaction, project_id, run_id, call_id).await?;
    validate_live_call(&mut transaction, actor, &locked, &call, runtime_tick()?).await?;
    if call.status != "pending" {
        return Err(AppError::Conflict);
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_tool_attempt_dispatches
         WHERE project_id = $1 AND call_id = $2 AND attempt = $3)",
    )
    .bind(project_id)
    .bind(call_id)
    .bind(i32::from(call.attempt))
    .fetch_one(&mut *transaction)
    .await?
    {
        return Err(AppError::Conflict);
    }
    let runner = sqlx::query(
        "SELECT id, device_id, activated_key_version FROM agent_runners
         WHERE project_id = $1 AND principal_identity_id = $2 AND device_id = $3
           AND state = 'active' FOR SHARE",
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let profile = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT execution_profile_commitment
         FROM agent_tool_runtime_capability_witnesses
         WHERE id=$1 AND project_id=$2 AND owner_identity_id=$3
           AND issued_at <= clock_timestamp() AND clock_timestamp() < expires_at",
    )
    .bind(call.runtime_capability_witness_id)
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let dispatched_at = Utc::now();
    if dispatched_at >= call.tool_deadline_at {
        return Err(AppError::Conflict);
    }
    let lease_expires_at = dispatched_at + Duration::seconds(TOOL_LEASE_SECONDS);
    sqlx::query(
        "INSERT INTO agent_tool_attempt_dispatches (
            id, project_id, call_id, run_id, goal_id, work_item_id,
            work_claim_id, work_attempt, owner_identity_id,
            attempt, lease_id, runner_id,
            runner_identity_id, runner_device_id, runner_key_version,
            runtime_capability_witness_id, adapter_protocol,
            canonical_input_commitment, execution_profile_commitment,
            requested_at, dispatched_at, lease_expires_at, tool_deadline_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,
                   $16,$17,$18,$19,$20,$21,$22,$23)",
    )
    .bind(request.dispatch_id)
    .bind(project_id)
    .bind(call_id)
    .bind(run_id)
    .bind(call.goal_id)
    .bind(call.work_item_id)
    .bind(call.work_claim_id)
    .bind(i32::from(call.work_attempt))
    .bind(call.owner)
    .bind(i32::from(call.attempt))
    .bind(request.lease_id)
    .bind(runner.try_get::<Uuid, _>("id")?)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(runner.try_get::<i32, _>("activated_key_version")?)
    .bind(call.runtime_capability_witness_id)
    .bind(&call.adapter_protocol)
    .bind(call.input_commitment.as_slice())
    .bind(&profile)
    .bind(call.requested_at)
    .bind(dispatched_at)
    .bind(lease_expires_at)
    .bind(call.tool_deadline_at)
    .execute(&mut *transaction)
    .await?;
    let encrypted_input: EncryptedPayloadDto =
        serde_json::from_slice(&call.encrypted_input).map_err(|_| AppError::Internal)?;
    transaction.commit().await?;
    Ok(Json(ClaimToolResponse {
        dispatch_id: request.dispatch_id,
        lease_id: request.lease_id,
        call_id,
        attempt: call.attempt,
        tool_id: call.tool_id,
        tool_version: call.tool_version,
        encrypted_input,
        canonical_input_commitment_hex: hex::encode(call.input_commitment),
        adapter_protocol: call.adapter_protocol,
        lease_expires_at,
        tool_deadline_at: call.tool_deadline_at,
    }))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordToolRequest {
    request_id: Uuid,
    dispatch_id: Uuid,
    wire_request_commitment_hex: String,
    signed_at: DateTime<Utc>,
    idempotency_key: Uuid,
    signatures: super::agents::CompilationSignatures,
}

#[derive(Serialize)]
struct ToolRequestStatement<'a> {
    signature_context: &'static str,
    project_id: Uuid,
    run_id: Uuid,
    call_id: Uuid,
    request_id: Uuid,
    dispatch_id: Uuid,
    attempt: u16,
    adapter_protocol: &'a str,
    canonical_input_commitment_hex: String,
    wire_request_commitment_hex: &'a str,
    execution_profile_commitment_hex: String,
    signed_at: DateTime<Utc>,
    idempotency_key: Uuid,
}

#[derive(Serialize)]
pub struct ToolRequestResponse {
    request_id: Uuid,
    replayed: bool,
}

pub async fn record_request(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, call_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RecordToolRequest>,
) -> Result<Json<ToolRequestResponse>, AppError> {
    let wire = commitment(&request.wire_request_commitment_hex)?;
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let call = lock_call(&mut transaction, project_id, run_id, call_id).await?;
    if call.owner != actor.identity_id || call.status != "pending" {
        return Err(AppError::Forbidden);
    }
    let dispatch = sqlx::query(
        "SELECT execution_profile_commitment, runner_identity_id, runner_device_id,
                runner_key_version, adapter_protocol, canonical_input_commitment
         FROM agent_tool_attempt_dispatches
         WHERE project_id=$1 AND id=$2 AND call_id=$3 AND attempt=$4 FOR SHARE",
    )
    .bind(project_id)
    .bind(request.dispatch_id)
    .bind(call_id)
    .bind(i32::from(call.attempt))
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let profile: Vec<u8> = dispatch.try_get("execution_profile_commitment")?;
    if dispatch.try_get::<Uuid, _>("runner_identity_id")? != actor.identity_id
        || dispatch.try_get::<Uuid, _>("runner_device_id")? != request.signatures.signer_device_id
        || dispatch.try_get::<i32, _>("runner_key_version")?
            != i32::try_from(request.signatures.signer_device_key_version)
                .map_err(|_| AppError::BadRequest("invalid signer key version"))?
        || dispatch.try_get::<String, _>("adapter_protocol")? != call.adapter_protocol
        || dispatch.try_get::<Vec<u8>, _>("canonical_input_commitment")? != call.input_commitment
    {
        return Err(AppError::Forbidden);
    }
    let statement = ToolRequestStatement {
        signature_context: "sprout-external-tool-request-v1",
        project_id,
        run_id,
        call_id,
        request_id: request.request_id,
        dispatch_id: request.dispatch_id,
        attempt: call.attempt,
        adapter_protocol: &call.adapter_protocol,
        canonical_input_commitment_hex: hex::encode(call.input_commitment),
        wire_request_commitment_hex: &request.wire_request_commitment_hex,
        execution_profile_commitment_hex: hex::encode(&profile),
        signed_at: request.signed_at,
        idempotency_key: request.idempotency_key,
    };
    let statement_hash = super::agents::verify_device_statement(
        &mut transaction,
        actor,
        &statement,
        &request.signatures,
        b"sprout-external-tool-request-v1",
    )
    .await?;
    if let Some(existing) = sqlx::query(
        "SELECT id, statement_hash FROM agent_tool_attempt_requests
         WHERE project_id=$1 AND call_id=$2 AND idempotency_key=$3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(call_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.try_get::<Vec<u8>, _>("statement_hash")? != statement_hash {
            return Err(AppError::Conflict);
        }
        let request_id = existing.try_get("id")?;
        transaction.commit().await?;
        return Ok(Json(ToolRequestResponse {
            request_id,
            replayed: true,
        }));
    }
    sqlx::query(
        r#"
        INSERT INTO agent_tool_attempt_requests (
          id, project_id, dispatch_id, call_id, attempt, adapter_protocol,
          canonical_input_commitment, wire_request_commitment,
          execution_profile_commitment, signer_identity_id, signer_device_id,
          signer_device_key_version, signed_at, classical_signature,
          post_quantum_signature, statement_hash, idempotency_key
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
        "#,
    )
    .bind(request.request_id)
    .bind(project_id)
    .bind(request.dispatch_id)
    .bind(call_id)
    .bind(i32::from(call.attempt))
    .bind(&call.adapter_protocol)
    .bind(call.input_commitment.as_slice())
    .bind(wire.as_slice())
    .bind(profile)
    .bind(actor.identity_id)
    .bind(request.signatures.signer_device_id)
    .bind(
        i32::try_from(request.signatures.signer_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid signer key version"))?,
    )
    .bind(request.signed_at)
    .bind(&request.signatures.classical_signature)
    .bind(&request.signatures.post_quantum_signature)
    .bind(statement_hash.as_slice())
    .bind(request.idempotency_key)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ToolRequestResponse {
        request_id: request.request_id,
        replayed: false,
    }))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalToolStatus {
    Succeeded,
    Failed,
    TimedOut,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExactTerminalToolRequest {
    observation_id: Uuid,
    dispatch_id: Uuid,
    lease_id: Uuid,
    request_id: Option<Uuid>,
    status: TerminalToolStatus,
    encrypted_output: Option<EncryptedPayloadDto>,
    canonical_output_commitment_hex: Option<String>,
    failure_code: Option<String>,
    output_key_envelopes: Vec<ExactToolOutputKeyEnvelope>,
    signed_at: DateTime<Utc>,
    idempotency_key: Uuid,
    signatures: super::agents::CompilationSignatures,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExactToolOutputKeyEnvelope {
    id: Uuid,
    envelope_version: u16,
    key_purpose: String,
    recipient_identity_id: Uuid,
    recipient_device_id: Uuid,
    recipient_device_key_version: u32,
    sender_device_key_version: u32,
    encrypted_key_b64: String,
    sender_signature_b64: String,
    sender_post_quantum_signature_b64: String,
}

#[derive(Serialize)]
struct ExactTerminalToolStatement<'a> {
    signature_context: &'static str,
    project_id: Uuid,
    run_id: Uuid,
    call_id: Uuid,
    observation_id: Uuid,
    dispatch_id: Uuid,
    lease_id: Uuid,
    request_id: Option<Uuid>,
    attempt: u16,
    tool_id: &'a str,
    tool_version: u32,
    adapter_protocol: &'a str,
    canonical_input_commitment_hex: String,
    wire_request_commitment_hex: Option<String>,
    execution_profile_commitment_hex: String,
    status: &'a TerminalToolStatus,
    encrypted_output_payload_commitment_hex: Option<String>,
    canonical_output_commitment_hex: &'a Option<String>,
    output_readable_by: Vec<Uuid>,
    failure_code: &'a Option<String>,
    output_key_envelopes: &'a [ExactToolOutputKeyEnvelope],
    signed_at: DateTime<Utc>,
    idempotency_key: Uuid,
}

pub async fn terminal_exact(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, call_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<ExactTerminalToolRequest>,
) -> Result<Json<ToolCallResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    let call = lock_call(&mut transaction, project_id, run_id, call_id).await?;
    if !actor.is_agent || call.owner != actor.identity_id {
        return Err(AppError::Forbidden);
    }
    let replay_candidate = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_tool_attempt_observations
         WHERE project_id=$1 AND call_id=$2 AND idempotency_key=$3)",
    )
    .bind(project_id)
    .bind(call_id)
    .bind(request.idempotency_key)
    .fetch_one(&mut *transaction)
    .await?;
    if call.status != "pending" && !replay_candidate {
        return Err(AppError::Forbidden);
    }
    let dispatch = sqlx::query(
        r#"
        SELECT execution_profile_commitment, runner_identity_id, runner_device_id,
               runner_key_version, attempt, adapter_protocol,
               canonical_input_commitment, dispatched_at
        FROM agent_tool_attempt_dispatches
        WHERE project_id=$1 AND id=$2 AND call_id=$3 AND lease_id=$4
        FOR SHARE
        "#,
    )
    .bind(project_id)
    .bind(request.dispatch_id)
    .bind(call_id)
    .bind(request.lease_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let profile: Vec<u8> = dispatch.try_get("execution_profile_commitment")?;
    let signer_identity: Uuid = dispatch.try_get("runner_identity_id")?;
    let signer_device: Uuid = dispatch.try_get("runner_device_id")?;
    let signer_key_version: i32 = dispatch.try_get("runner_key_version")?;
    if signer_identity != call.owner
        || dispatch.try_get::<i32, _>("attempt")? != i32::from(call.attempt)
        || dispatch.try_get::<String, _>("adapter_protocol")? != call.adapter_protocol
        || dispatch.try_get::<Vec<u8>, _>("canonical_input_commitment")? != call.input_commitment
        || Uuid::from(request.signatures.signer_identity_id) != signer_identity
        || request.signatures.signer_device_id != signer_device
        || i32::try_from(request.signatures.signer_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid signer key version"))?
            != signer_key_version
    {
        return Err(AppError::Forbidden);
    }
    let wire_request = match request.request_id {
        Some(request_id) => Some(
            sqlx::query_scalar::<_, Vec<u8>>(
                r#"
                SELECT wire_request_commitment FROM agent_tool_attempt_requests
                WHERE project_id=$1 AND id=$2 AND dispatch_id=$3 AND call_id=$4
                  AND attempt=$5 AND adapter_protocol=$6
                  AND canonical_input_commitment=$7
                  AND execution_profile_commitment=$8
                "#,
            )
            .bind(project_id)
            .bind(request_id)
            .bind(request.dispatch_id)
            .bind(call_id)
            .bind(i32::from(call.attempt))
            .bind(&call.adapter_protocol)
            .bind(call.input_commitment.as_slice())
            .bind(&profile)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AppError::Conflict)?,
        ),
        None => None,
    };
    let encrypted_output = request
        .encrypted_output
        .as_ref()
        .map(canonical_governance_json)
        .transpose()
        .map_err(|_| AppError::BadRequest("invalid encrypted tool output"))?;
    let encrypted_output_commitment = encrypted_output
        .as_ref()
        .map(|bytes| <[u8; 32]>::from(Sha256::digest(bytes)));
    let canonical_output_commitment = request
        .canonical_output_commitment_hex
        .as_deref()
        .map(commitment)
        .transpose()?;
    let (status, audit_kind, work_succeeded) = match request.status {
        TerminalToolStatus::Succeeded => ("succeeded", "completed", true),
        TerminalToolStatus::Failed => ("failed", "failed", false),
        TerminalToolStatus::TimedOut => ("timed_out", "timed_out", false),
    };
    let succeeded = status == "succeeded";
    if succeeded
        != (encrypted_output.is_some()
            && encrypted_output_commitment.is_some()
            && canonical_output_commitment.is_some()
            && request.failure_code.is_none())
        || (!succeeded && request.failure_code.as_deref().is_none_or(str::is_empty))
        || wire_request.is_none()
        || (succeeded
            && (request.output_key_envelopes.len() != 1
                || request.output_key_envelopes[0].recipient_identity_id != call.owner
                || request.output_key_envelopes[0].envelope_version != 2
                || request.output_key_envelopes[0].key_purpose != "tool_output"))
        || (!succeeded && !request.output_key_envelopes.is_empty())
    {
        return Err(AppError::BadRequest("terminal tool observation mismatch"));
    }
    let statement = ExactTerminalToolStatement {
        signature_context: "sprout-external-tool-observation-v1",
        project_id,
        run_id,
        call_id,
        observation_id: request.observation_id,
        dispatch_id: request.dispatch_id,
        lease_id: request.lease_id,
        request_id: request.request_id,
        attempt: call.attempt,
        tool_id: &call.tool_id,
        tool_version: call.tool_version,
        adapter_protocol: &call.adapter_protocol,
        canonical_input_commitment_hex: hex::encode(call.input_commitment),
        wire_request_commitment_hex: wire_request.as_ref().map(hex::encode),
        execution_profile_commitment_hex: hex::encode(&profile),
        status: &request.status,
        encrypted_output_payload_commitment_hex: encrypted_output_commitment.map(hex::encode),
        canonical_output_commitment_hex: &request.canonical_output_commitment_hex,
        output_readable_by: vec![call.owner],
        failure_code: &request.failure_code,
        output_key_envelopes: &request.output_key_envelopes,
        signed_at: request.signed_at,
        idempotency_key: request.idempotency_key,
    };
    let statement_hash = verify_temporal_tool_statement(
        &mut transaction,
        TemporalToolSigner {
            identity: signer_identity,
            device: signer_device,
            key_version: signer_key_version,
        },
        request.signed_at,
        &statement,
        &request.signatures,
        b"sprout-external-tool-observation-v1",
    )
    .await?;
    if let Some(existing) = sqlx::query(
        "SELECT observation_hash FROM agent_tool_attempt_observations
         WHERE project_id=$1 AND call_id=$2 AND idempotency_key=$3",
    )
    .bind(project_id)
    .bind(call_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.try_get::<Vec<u8>, _>("observation_hash")? != statement_hash {
            return Err(AppError::Conflict);
        }
        transaction.commit().await?;
        return Ok(Json(ToolCallResponse {
            id: call_id,
            status: status_response(call.status),
            attempt: call.attempt,
            replayed: true,
        }));
    }
    let observed_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO agent_tool_attempt_observations (
          id, project_id, dispatch_id, call_id, attempt, lease_id,
          terminal_origin, terminal_status, request_id, wire_request_commitment,
          canonical_input_commitment, execution_profile_commitment,
          encrypted_output, encrypted_output_payload_commitment,
          canonical_output_commitment, output_readable_by, failure_code,
          signer_identity_id, signer_device_id, signer_device_key_version,
          signed_at, classical_signature, post_quantum_signature,
          statement_hash, idempotency_key, observation_hash
        ) VALUES ($1,$2,$3,$4,$5,$6,'signed_edge_observation',$7,$8,$9,$10,$11,
                  $12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25)
        RETURNING observed_at
        "#,
    )
    .bind(request.observation_id)
    .bind(project_id)
    .bind(request.dispatch_id)
    .bind(call_id)
    .bind(i32::from(call.attempt))
    .bind(request.lease_id)
    .bind(status)
    .bind(request.request_id)
    .bind(wire_request)
    .bind(call.input_commitment.as_slice())
    .bind(&profile)
    .bind(encrypted_output)
    .bind(encrypted_output_commitment.map(|value| value.to_vec()))
    .bind(canonical_output_commitment.map(|value| value.to_vec()))
    .bind(json!([call.owner]))
    .bind(request.failure_code.as_deref())
    .bind(signer_identity)
    .bind(signer_device)
    .bind(signer_key_version)
    .bind(request.signed_at)
    .bind(&request.signatures.classical_signature)
    .bind(&request.signatures.post_quantum_signature)
    .bind(statement_hash.as_slice())
    .bind(request.idempotency_key)
    .bind(statement_hash.as_slice())
    .fetch_one(&mut *transaction)
    .await?;
    for envelope in &request.output_key_envelopes {
        validate_and_store_tool_output_envelope(
            &mut transaction,
            project_id,
            call_id,
            call.attempt,
            request.observation_id,
            envelope,
            signer_identity,
            signer_device,
            signer_key_version,
            request.signed_at,
        )
        .await?;
    }
    sqlx::query(
        "UPDATE agent_tool_calls SET current_status=$3,
         current_output_commitment=$4, terminal_at=$5
         WHERE project_id=$1 AND id=$2 AND current_status='pending'",
    )
    .bind(project_id)
    .bind(call_id)
    .bind(status)
    .bind(canonical_output_commitment.map(|value| value.to_vec()))
    .bind(observed_at)
    .execute(&mut *transaction)
    .await?;
    locked
        .state
        .close_dispatched_tool_work(
            call.work_claim_id.into(),
            UserId::from(call.owner),
            call.attempt,
            work_succeeded,
        )
        .map_err(|_| AppError::Conflict)?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: if work_succeeded {
                "work_succeeded"
            } else {
                "work_failed"
            },
            tick: runtime_tick()?,
            observation: Some(("tool_terminal", request.observation_id)),
        },
    )
    .await?;
    persist_tool_work_outcome(
        &mut transaction,
        project_id,
        &call,
        ToolWorkOutcomeObservation {
            id: request.observation_id,
            observed_at,
            succeeded: work_succeeded,
            terminal_status: status,
            transition_id: transition.id,
        },
    )
    .await?;
    insert_audit(
        &mut transaction,
        call.coordinates(project_id),
        audit_kind,
        status,
        call.input_commitment,
        call.max_attempts,
        call.timeout_seconds,
        Some(request.observation_id),
        request.idempotency_key,
    )
    .await?;
    project_r540_signed_tool_terminal(
        &mut transaction,
        project_id,
        run_id,
        call_id,
        call.attempt,
        request.observation_id,
        transition.id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ToolCallResponse {
        id: call_id,
        status: status_response(status.to_owned()),
        attempt: call.attempt,
        replayed: false,
    }))
}

/// Materializes due external-tool deadlines before generic claim recovery. The
/// worker owns no connector credential and performs no external request.
pub(crate) async fn materialize_server_timeouts(pool: &sqlx::PgPool) -> Result<u64, AppError> {
    let candidates = sqlx::query_as::<_, (Uuid, Uuid, Uuid)>(
        r#"
        SELECT call.project_id, call.run_id, call.id
        FROM agent_tool_calls call
        LEFT JOIN agent_tool_attempt_observations observation
          ON observation.project_id=call.project_id
         AND observation.call_id=call.id
         AND observation.attempt=call.current_attempt
        WHERE call.current_status='pending'
          AND call.tool_deadline_at <= clock_timestamp()
          AND observation.id IS NULL
        ORDER BY call.project_id, call.run_id, call.id
        "#,
    )
    .fetch_all(pool)
    .await?;
    let mut count = 0_u64;
    for (project_id, run_id, call_id) in candidates {
        let mut transaction = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *transaction)
            .await?;
        let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
        let call = lock_call(&mut transaction, project_id, run_id, call_id).await?;
        if call.status != "pending" {
            transaction.commit().await?;
            continue;
        }
        if Utc::now() < call.tool_deadline_at {
            transaction.commit().await?;
            continue;
        }
        let dispatch = sqlx::query(
            "SELECT id, lease_id, execution_profile_commitment,
                    canonical_input_commitment, attempt, tool_deadline_at
             FROM agent_tool_attempt_dispatches
             WHERE project_id=$1 AND call_id=$2 AND attempt=$3 FOR SHARE",
        )
        .bind(project_id)
        .bind(call_id)
        .bind(i32::from(call.attempt))
        .fetch_optional(&mut *transaction)
        .await?;
        let (dispatch_id, lease_id, profile, exact_request) = if let Some(dispatch) = dispatch {
            if dispatch.try_get::<Vec<u8>, _>("canonical_input_commitment")?
                != call.input_commitment
                || dispatch.try_get::<DateTime<Utc>, _>("tool_deadline_at")?
                    != call.tool_deadline_at
            {
                return Err(AppError::Conflict);
            }
            let dispatch_id: Uuid = dispatch.try_get("id")?;
            (
                Some(dispatch_id),
                Some(dispatch.try_get::<Uuid, _>("lease_id")?),
                dispatch.try_get::<Vec<u8>, _>("execution_profile_commitment")?,
                load_attempt_request(
                    &mut transaction,
                    project_id,
                    dispatch_id,
                    call_id,
                    call.attempt,
                )
                .await?,
            )
        } else {
            let profile = sqlx::query_scalar::<_, Vec<u8>>(
                "SELECT execution_profile_commitment
                 FROM agent_tool_runtime_capability_witnesses WHERE id=$1",
            )
            .bind(call.runtime_capability_witness_id)
            .fetch_one(&mut *transaction)
            .await?;
            (None, None, profile, None)
        };
        let observation_id = derived_tool_uuid(
            b"server-timeout-observation",
            &[project_id, call_id, call.work_claim_id],
        );
        let idempotency_key = derived_tool_uuid(
            b"server-timeout-idempotency",
            &[project_id, call_id, call.work_claim_id],
        );
        let observation_hash = digest(&json!({
            "origin": "server_timeout",
            "project_id": project_id,
            "run_id": run_id,
            "call_id": call_id,
            "dispatch_id": dispatch_id,
            "lease_id": lease_id,
            "attempt": call.attempt,
            "canonical_input_commitment": hex::encode(call.input_commitment),
            "execution_profile_commitment": hex::encode(&profile),
            "request_id": exact_request.as_ref().map(|request| request.0),
            "wire_request_commitment": exact_request.as_ref().map(|request| hex::encode(&request.1)),
            "idempotency_key": idempotency_key,
        }))?;
        let observed_at: DateTime<Utc> = sqlx::query_scalar(
            r#"
            INSERT INTO agent_tool_attempt_observations (
              id, project_id, dispatch_id, call_id, attempt, lease_id,
              terminal_origin, terminal_status, request_id, wire_request_commitment,
              canonical_input_commitment, execution_profile_commitment,
              encrypted_output, encrypted_output_payload_commitment,
              canonical_output_commitment, output_readable_by, failure_code,
              signer_identity_id, signer_device_id, signer_device_key_version,
              signed_at, classical_signature, post_quantum_signature,
              statement_hash, idempotency_key, observation_hash
            ) VALUES ($1,$2,$3,$4,$5,$6,'server_timeout','timed_out',$7,$8,$9,$10,
                      NULL,NULL,NULL,$11,'server_timeout',NULL,NULL,NULL,NULL,NULL,NULL,
                      NULL,$12,$13)
            RETURNING observed_at
            "#,
        )
        .bind(observation_id)
        .bind(project_id)
        .bind(dispatch_id)
        .bind(call_id)
        .bind(i32::from(call.attempt))
        .bind(lease_id)
        .bind(exact_request.as_ref().map(|request| request.0))
        .bind(exact_request.as_ref().map(|request| request.1.clone()))
        .bind(call.input_commitment.as_slice())
        .bind(&profile)
        .bind(json!([call.owner]))
        .bind(idempotency_key)
        .bind(&observation_hash)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE agent_tool_calls SET current_status='timed_out',
             current_output_commitment=NULL, terminal_at=$3
             WHERE project_id=$1 AND id=$2 AND current_status='pending'",
        )
        .bind(project_id)
        .bind(call_id)
        .bind(observed_at)
        .execute(&mut *transaction)
        .await?;
        locked
            .state
            .close_dispatched_tool_work(
                call.work_claim_id.into(),
                UserId::from(call.owner),
                call.attempt,
                false,
            )
            .map_err(|_| AppError::Conflict)?;
        let facts = authoritative_condition_facts(
            &mut transaction,
            project_id,
            &locked.contract,
            &locked.state,
        )
        .await?;
        let transition = persist_transition(
            &mut transaction,
            project_id,
            None,
            &locked,
            &facts,
            TransitionMetadata {
                kind: "work_failed",
                tick: runtime_tick()?,
                observation: Some(("tool_terminal", observation_id)),
            },
        )
        .await?;
        persist_tool_work_outcome(
            &mut transaction,
            project_id,
            &call,
            ToolWorkOutcomeObservation {
                id: observation_id,
                observed_at,
                succeeded: false,
                terminal_status: "timed_out",
                transition_id: transition.id,
            },
        )
        .await?;
        insert_audit(
            &mut transaction,
            call.coordinates(project_id),
            "timed_out",
            "timed_out",
            call.input_commitment,
            call.max_attempts,
            call.timeout_seconds,
            Some(observation_id),
            idempotency_key,
        )
        .await?;
        project_r540_server_timeout(
            &mut transaction,
            project_id,
            run_id,
            call_id,
            call.attempt,
            observation_id,
            transition.id,
        )
        .await?;
        transaction.commit().await?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn derived_tool_uuid(context: &[u8], parts: &[Uuid]) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(b"sprout-external-tool-derived-id-v1");
    digest.update(context);
    for part in parts {
        digest.update(part.as_bytes());
    }
    let bytes: [u8; 32] = digest.finalize().into();
    let mut id = [0_u8; 16];
    id.copy_from_slice(&bytes[..16]);
    id[6] = (id[6] & 0x0f) | 0x80;
    id[8] = (id[8] & 0x3f) | 0x80;
    Uuid::from_bytes(id)
}

async fn load_attempt_request(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    dispatch_id: Uuid,
    call_id: Uuid,
    attempt: u16,
) -> Result<Option<(Uuid, Vec<u8>)>, AppError> {
    sqlx::query_as(
        "SELECT id, wire_request_commitment
         FROM agent_tool_attempt_requests
         WHERE project_id=$1 AND dispatch_id=$2 AND call_id=$3 AND attempt=$4",
    )
    .bind(project_id)
    .bind(dispatch_id)
    .bind(call_id)
    .bind(i32::from(attempt))
    .fetch_optional(&mut **transaction)
    .await
    .map_err(AppError::from)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetryToolRequest {
    work_claim_id: Uuid,
    runtime_capability_witness_id: Uuid,
    idempotency_key: Uuid,
}

pub async fn retry(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, call_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RetryToolRequest>,
) -> Result<Json<ToolCallResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_authority(&mut transaction, project_id, actor, &locked.state).await?;
    let mut call = lock_call(&mut transaction, project_id, run_id, call_id).await?;
    if !matches!(call.status.as_str(), "failed" | "timed_out") || call.attempt >= call.max_attempts
    {
        return Err(AppError::Conflict);
    }
    let claim = locked
        .state
        .claims
        .get(&request.work_claim_id.into())
        .ok_or(AppError::Conflict)?;
    let work = locked
        .state
        .work_items
        .get(&claim.work)
        .ok_or(AppError::Conflict)?;
    let spec = locked
        .contract
        .work_specs
        .iter()
        .find(|candidate| candidate.id == work.work_spec_id)
        .ok_or(AppError::Conflict)?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    let obligation = locked
        .contract
        .obligations
        .iter()
        .find(|candidate| candidate.id == work.serves)
        .ok_or(AppError::Conflict)?;
    let next_attempt = call.attempt.checked_add(1).ok_or(AppError::Conflict)?;
    let requested_tick = runtime_tick()?;
    if claim.claimant != UserId::from(actor.identity_id)
        || claim.status != ClaimStatus::Active
        || claim.acquired_at > requested_tick
        || claim.expires_at <= requested_tick
        || claim.attempt != next_attempt
        || work.status != WorkStatus::Claimed
        || !matches!(work.kind, WorkKind::ToolInvocation | WorkKind::ToolRetry)
        || !spec.allowed_actions.contains(&AgentActionClass::RetryTool)
        || spec.owner != UserId::from(actor.identity_id)
        || !spec.activation.holds(&facts)
        || !obligation.required_for_completion.holds(&facts)
    {
        return Err(AppError::Forbidden);
    }
    let snapshot =
        load_tool_security_snapshot(&mut transaction, project_id, run_id, &locked.state, work)
            .await?;
    let required_effects = initial_tool_required_effects(&call.tool_id, call.tool_version)
        .ok_or(AppError::Forbidden)?;
    let runtime_ready = current_runtime_capability(
        &mut transaction,
        project_id,
        actor,
        request.runtime_capability_witness_id,
        &call.tool_id,
        call.tool_version,
    )
    .await?;
    if !snapshot.policy.allowed_tools.contains(&call.tool_id)
        || !snapshot.run_ceiling.contains(&call.tool_id)
        || !snapshot.work_ceiling.contains(&call.tool_id)
        || !runtime_ready
        || !current_tool_permission(
            &mut transaction,
            project_id,
            actor.identity_id,
            &call.tool_id,
            call.tool_version,
        )
        .await?
        || !current_tool_permission(
            &mut transaction,
            project_id,
            snapshot.work_authority_principal,
            &call.tool_id,
            call.tool_version,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    let replay_hash = digest(&json!({
        "call_id": call_id,
        "work_claim_id": request.work_claim_id,
        "attempt": next_attempt,
        "runtime_capability_witness_id": request.runtime_capability_witness_id,
        "security_policy_hash": hex::encode(snapshot.policy_hash),
        "run_tool_ceiling_hash": hex::encode(snapshot.run_ceiling_hash),
        "work_tool_ceiling_hash": hex::encode(snapshot.work_ceiling_hash),
        "required_effects": required_effects,
        "idempotency_key": request.idempotency_key,
    }))?;
    if let Some(existing) = sqlx::query(
        "SELECT event_hash FROM agent_tool_audit
         WHERE project_id = $1 AND owner_identity_id = $2 AND idempotency_key = $3",
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.try_get::<Vec<u8>, _>("event_hash")? != replay_hash {
            return Err(AppError::Conflict);
        }
        transaction.commit().await?;
        return Ok(Json(ToolCallResponse {
            id: call_id,
            status: "pending",
            attempt: next_attempt,
            replayed: true,
        }));
    }
    sqlx::query(
        "UPDATE agent_tool_calls SET work_item_id=$3, work_claim_id=$4,
             work_attempt=$5, work_spec_ordinal=$6, current_attempt=$5,
             runtime_capability_witness_id=$7, security_policy_hash=$8,
             run_tool_ceiling_hash=$9, work_tool_ceiling_hash=$10,
             work_tool_ceiling=$11, work_authority_origin=$12,
             work_authority_parent_id=$13, work_authority_principal_id=$14,
             required_effects=$15, requested_tick=$16, requested_at=$17,
             tool_deadline_tick=$18, tool_deadline_at=$19,
             current_status='pending', current_output_commitment=NULL, terminal_at=NULL
         WHERE project_id=$1 AND id=$2",
    )
    .bind(project_id)
    .bind(call_id)
    .bind(Uuid::from(work.id))
    .bind(request.work_claim_id)
    .bind(i32::from(next_attempt))
    .bind(i64::try_from(spec.id).map_err(|_| AppError::Internal)?)
    .bind(request.runtime_capability_witness_id)
    .bind(snapshot.policy_hash.as_slice())
    .bind(snapshot.run_ceiling_hash.as_slice())
    .bind(snapshot.work_ceiling_hash.as_slice())
    .bind(serde_json::to_value(&snapshot.work_ceiling).map_err(|_| AppError::Internal)?)
    .bind(snapshot.work_authority_origin)
    .bind(snapshot.work_authority_parent)
    .bind(snapshot.work_authority_principal)
    .bind(serde_json::to_value(&required_effects).map_err(|_| AppError::Internal)?)
    .bind(i64::try_from(requested_tick).map_err(|_| AppError::Internal)?)
    .bind(tick_datetime(requested_tick)?)
    .bind(
        i64::try_from(requested_tick.saturating_add(u64::from(call.timeout_seconds)))
            .map_err(|_| AppError::Internal)?,
    )
    .bind(tick_datetime(
        requested_tick.saturating_add(u64::from(call.timeout_seconds)),
    )?)
    .execute(&mut *transaction)
    .await?;
    call.work_item_id = Uuid::from(work.id);
    call.work_claim_id = request.work_claim_id;
    call.work_attempt = next_attempt;
    call.attempt = next_attempt;
    call.runtime_capability_witness_id = request.runtime_capability_witness_id;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: "tool_attempt_opened",
            tick: requested_tick,
            observation: Some(("tool_attempt", call_id)),
        },
    )
    .await?;
    insert_audit_with_hash(
        &mut transaction,
        call.coordinates(project_id),
        "retry_started",
        "pending",
        call.input_commitment,
        call.max_attempts,
        call.timeout_seconds,
        None,
        request.idempotency_key,
        replay_hash,
    )
    .await?;
    project_r540_tool_attempt(&mut transaction, project_id, run_id, call_id, transition.id).await?;
    transaction.commit().await?;
    Ok(Json(ToolCallResponse {
        id: call_id,
        status: "pending",
        attempt: next_attempt,
        replayed: false,
    }))
}

#[derive(Clone, Copy)]
struct TemporalToolSigner {
    identity: Uuid,
    device: Uuid,
    key_version: i32,
}

async fn verify_temporal_tool_statement(
    transaction: &mut Transaction<'_, Postgres>,
    expected: TemporalToolSigner,
    signed_at: DateTime<Utc>,
    statement: &impl Serialize,
    signatures: &super::agents::CompilationSignatures,
    context: &[u8],
) -> Result<[u8; 32], AppError> {
    if Uuid::from(signatures.signer_identity_id) != expected.identity
        || signatures.signer_device_id != expected.device
        || i32::try_from(signatures.signer_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid signer key version"))?
            != expected.key_version
    {
        return Err(AppError::Forbidden);
    }
    let keys = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        r#"
        SELECT key.ed25519_public_key, key.ml_dsa_65_public_key
        FROM device_keys key
        JOIN devices device
          ON device.identity_id=key.identity_id AND device.id=key.device_id
        WHERE key.identity_id=$1 AND key.device_id=$2 AND key.key_version=$3
          AND key.suite_version=32769
          AND key.created_at <= $4
          AND (key.revoked_at IS NULL OR $4 < key.revoked_at)
          AND device.created_at <= $4
          AND (device.retired_at IS NULL OR $4 < device.retired_at)
        "#,
    )
    .bind(expected.identity)
    .bind(expected.device)
    .bind(expected.key_version)
    .bind(signed_at)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let bytes = canonical_governance_json(statement)
        .map_err(|_| AppError::BadRequest("invalid canonical tool statement"))?;
    verify_ed25519_ml_dsa65_signatures(
        &keys.0,
        &signatures.classical_signature,
        &keys.1,
        &signatures.post_quantum_signature,
        &bytes,
        context,
    )
    .map_err(|_| AppError::BadRequest("tool statement signature verification failed"))?;
    Ok(Sha256::digest(bytes).into())
}

#[derive(Serialize)]
struct ToolOutputEnvelopeStatement {
    signature_context: &'static str,
    project_id: Uuid,
    call_id: Uuid,
    attempt: u16,
    envelope_version: u16,
    key_purpose: String,
    recipient_identity_id: Uuid,
    recipient_device_id: Uuid,
    recipient_device_key_version: u32,
    sender_identity_id: Uuid,
    sender_device_id: Uuid,
    sender_device_key_version: u32,
    encrypted_key_commitment_hex: String,
}

#[allow(clippy::too_many_arguments)]
async fn validate_and_store_tool_output_envelope(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    call_id: Uuid,
    attempt: u16,
    observation_id: Uuid,
    envelope: &ExactToolOutputKeyEnvelope,
    sender_identity: Uuid,
    sender_device: Uuid,
    sender_key_version: i32,
    signed_at: DateTime<Utc>,
) -> Result<(), AppError> {
    if envelope.envelope_version != 2
        || envelope.key_purpose != "tool_output"
        || i32::try_from(envelope.sender_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid sender key version"))?
            != sender_key_version
    {
        return Err(AppError::BadRequest(
            "invalid tool output envelope metadata",
        ));
    }
    let encrypted_key = base64::engine::general_purpose::STANDARD
        .decode(&envelope.encrypted_key_b64)
        .map_err(|_| AppError::BadRequest("invalid tool output key envelope"))?;
    validate_experimental_wrapped_resource_key(
        &encrypted_key,
        call_id,
        envelope.recipient_device_id,
        i32::from(attempt),
    )
    .map_err(|_| AppError::BadRequest("invalid hybrid tool output envelope"))?;
    let classical = base64::engine::general_purpose::STANDARD
        .decode(&envelope.sender_signature_b64)
        .map_err(|_| AppError::BadRequest("invalid tool output envelope signature"))?;
    let post_quantum = base64::engine::general_purpose::STANDARD
        .decode(&envelope.sender_post_quantum_signature_b64)
        .map_err(|_| AppError::BadRequest("invalid tool output envelope signature"))?;
    let envelope_commitment: [u8; 32] = Sha256::digest(&encrypted_key).into();
    let statement = ToolOutputEnvelopeStatement {
        signature_context: "sprout-tool-output-key-envelope-v1",
        project_id,
        call_id,
        attempt,
        envelope_version: envelope.envelope_version,
        key_purpose: envelope.key_purpose.clone(),
        recipient_identity_id: envelope.recipient_identity_id,
        recipient_device_id: envelope.recipient_device_id,
        recipient_device_key_version: envelope.recipient_device_key_version,
        sender_identity_id: sender_identity,
        sender_device_id: sender_device,
        sender_device_key_version: envelope.sender_device_key_version,
        encrypted_key_commitment_hex: hex::encode(envelope_commitment),
    };
    let bytes = canonical_governance_json(&statement)
        .map_err(|_| AppError::BadRequest("invalid canonical tool envelope"))?;
    let keys = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        r#"
        SELECT ed25519_public_key, ml_dsa_65_public_key FROM device_keys
        WHERE identity_id=$1 AND device_id=$2 AND key_version=$3
          AND suite_version=32769 AND created_at <= $4
          AND (revoked_at IS NULL OR $4 < revoked_at)
        "#,
    )
    .bind(sender_identity)
    .bind(sender_device)
    .bind(sender_key_version)
    .bind(signed_at)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    verify_ed25519_ml_dsa65_signatures(
        &keys.0,
        &classical,
        &keys.1,
        &post_quantum,
        &bytes,
        b"sprout-tool-output-key-envelope-v1",
    )
    .map_err(|_| AppError::BadRequest("tool output envelope signature verification failed"))?;
    sqlx::query(
        r#"
        INSERT INTO agent_tool_output_key_envelopes (
          id, project_id, observation_id, call_id, recipient_identity_id,
          recipient_device_id, recipient_device_key_version, envelope_version,
          key_purpose, encrypted_key, envelope_commitment, sender_identity_id,
          sender_device_id, sender_device_key_version, sender_signature,
          sender_post_quantum_signature
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
        "#,
    )
    .bind(envelope.id)
    .bind(project_id)
    .bind(observation_id)
    .bind(call_id)
    .bind(envelope.recipient_identity_id)
    .bind(envelope.recipient_device_id)
    .bind(
        i32::try_from(envelope.recipient_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid recipient key version"))?,
    )
    .bind(
        i16::try_from(envelope.envelope_version)
            .map_err(|_| AppError::BadRequest("invalid envelope version"))?,
    )
    .bind(&envelope.key_purpose)
    .bind(&encrypted_key)
    .bind(envelope_commitment.as_slice())
    .bind(sender_identity)
    .bind(sender_device)
    .bind(sender_key_version)
    .bind(classical)
    .bind(post_quantum)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[derive(Clone, Copy)]
struct ToolWorkOutcomeObservation<'a> {
    id: Uuid,
    observed_at: DateTime<Utc>,
    succeeded: bool,
    terminal_status: &'a str,
    transition_id: Uuid,
}

async fn persist_tool_work_outcome(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    call: &LockedCall,
    observation: ToolWorkOutcomeObservation<'_>,
) -> Result<(), AppError> {
    let canonical_output = sqlx::query_scalar::<_, Option<Vec<u8>>>(
        "SELECT canonical_output_commitment FROM agent_tool_attempt_observations
         WHERE project_id=$1 AND id=$2",
    )
    .bind(project_id)
    .bind(observation.id)
    .fetch_one(&mut **transaction)
    .await?
    .unwrap_or_default();
    let provenance = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        call.id,
        call.run_id,
        call.goal_id,
        call.work_item_id,
        call.work_claim_id,
        call.attempt,
        observation.id,
        observation.terminal_status,
        hex::encode(call.input_commitment),
        hex::encode(canonical_output),
    );
    let provenance_hash: [u8; 32] = Sha256::digest(provenance.as_bytes()).into();
    sqlx::query(
        r#"
        INSERT INTO agent_run_external_tool_work_outcomes (
          project_id, run_id, work_item_id, claim_id, attempt, work_status,
          observation_id, observed_at, provenance_hash, transition_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        "#,
    )
    .bind(project_id)
    .bind(call.run_id)
    .bind(call.work_item_id)
    .bind(call.work_claim_id)
    .bind(i32::from(call.attempt))
    .bind(if observation.succeeded {
        "succeeded"
    } else {
        "failed"
    })
    .bind(observation.id)
    .bind(observation.observed_at)
    .bind(provenance_hash.as_slice())
    .bind(observation.transition_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn project_r540_tool_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
    call_id: Uuid,
    transition_id: Uuid,
) -> Result<Option<i64>, AppError> {
    sqlx::query_scalar("SELECT sprout_private.project_agent_tool_attempt($1,$2,$3,$4)")
        .bind(project_id)
        .bind(run_id)
        .bind(call_id)
        .bind(transition_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(AppError::from)
}

#[allow(clippy::too_many_arguments)]
async fn project_r540_signed_tool_terminal(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
    call_id: Uuid,
    attempt: u16,
    observation_id: Uuid,
    transition_id: Uuid,
) -> Result<Option<i64>, AppError> {
    sqlx::query_scalar(
        "SELECT sprout_private.project_agent_tool_signed_terminal($1,$2,$3,$4,$5,$6)",
    )
    .bind(project_id)
    .bind(run_id)
    .bind(call_id)
    .bind(i32::from(attempt))
    .bind(observation_id)
    .bind(transition_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(AppError::from)
}

#[allow(clippy::too_many_arguments)]
async fn project_r540_server_timeout(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
    call_id: Uuid,
    attempt: u16,
    observation_id: Uuid,
    transition_id: Uuid,
) -> Result<Option<i64>, AppError> {
    sqlx::query_scalar("SELECT sprout_private.project_agent_tool_server_timeout($1,$2,$3,$4,$5,$6)")
        .bind(project_id)
        .bind(run_id)
        .bind(call_id)
        .bind(i32::from(attempt))
        .bind(observation_id)
        .bind(transition_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(AppError::from)
}

#[derive(Clone)]
struct LockedCall {
    project_id: Uuid,
    id: Uuid,
    run_id: Uuid,
    goal_id: Uuid,
    work_item_id: Uuid,
    work_claim_id: Uuid,
    work_attempt: u16,
    work_spec_id: u64,
    owner: Uuid,
    tool_id: String,
    tool_version: u32,
    runtime_capability_witness_id: Uuid,
    adapter_protocol: String,
    encrypted_input: Vec<u8>,
    input_commitment: [u8; 32],
    max_attempts: u16,
    timeout_seconds: u32,
    attempt: u16,
    status: String,
    requested_at: DateTime<Utc>,
    tool_deadline_at: DateTime<Utc>,
}

impl LockedCall {
    fn coordinates(&self, project_id: Uuid) -> AuditCoordinates<'_> {
        AuditCoordinates::new(
            project_id,
            self.id,
            self.run_id,
            self.goal_id,
            self.work_item_id,
            self.work_claim_id,
            self.work_attempt,
            self.owner,
            &self.tool_id,
            self.tool_version,
            self.attempt,
        )
    }
}

async fn lock_call(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
    call_id: Uuid,
) -> Result<LockedCall, AppError> {
    let row = sqlx::query(
        "SELECT * FROM agent_tool_calls
         WHERE project_id = $1 AND run_id = $2 AND id = $3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(run_id)
    .bind(call_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(LockedCall {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        run_id: row.try_get("run_id")?,
        goal_id: row.try_get("goal_id")?,
        work_item_id: row.try_get("work_item_id")?,
        work_claim_id: row.try_get("work_claim_id")?,
        work_attempt: u16::try_from(row.try_get::<i32, _>("work_attempt")?)
            .map_err(|_| AppError::Internal)?,
        work_spec_id: u64::try_from(row.try_get::<i64, _>("work_spec_ordinal")?)
            .map_err(|_| AppError::Internal)?,
        owner: row.try_get("owner_identity_id")?,
        tool_id: row.try_get("tool_name")?,
        tool_version: u32::try_from(row.try_get::<i32, _>("tool_version")?)
            .map_err(|_| AppError::Internal)?,
        runtime_capability_witness_id: row.try_get("runtime_capability_witness_id")?,
        adapter_protocol: row.try_get("adapter_protocol")?,
        encrypted_input: row.try_get("encrypted_input")?,
        input_commitment: row
            .try_get::<Vec<u8>, _>("canonical_input_commitment")?
            .try_into()
            .map_err(|_| AppError::Internal)?,
        max_attempts: u16::try_from(row.try_get::<i32, _>("max_attempts")?)
            .map_err(|_| AppError::Internal)?,
        timeout_seconds: u32::try_from(row.try_get::<i32, _>("timeout_seconds")?)
            .map_err(|_| AppError::Internal)?,
        attempt: u16::try_from(row.try_get::<i32, _>("current_attempt")?)
            .map_err(|_| AppError::Internal)?,
        status: row.try_get("current_status")?,
        requested_at: row.try_get("requested_at")?,
        tool_deadline_at: row.try_get("tool_deadline_at")?,
    })
}

async fn validate_live_call(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    locked: &super::agent_runs::LockedRun,
    call: &LockedCall,
    tick: u64,
) -> Result<(), AppError> {
    let claim = locked
        .state
        .claims
        .get(&call.work_claim_id.into())
        .ok_or(AppError::Conflict)?;
    let work = locked
        .state
        .work_items
        .get(&call.work_item_id.into())
        .ok_or(AppError::Conflict)?;
    if call.owner != actor.identity_id
        || claim.claimant != UserId::from(actor.identity_id)
        || claim.status != ClaimStatus::Active
        || claim.expires_at <= tick
        || claim.attempt != call.work_attempt
        || work.status != WorkStatus::Claimed
        || work.attempt != call.work_attempt
        || work.work_spec_id != call.work_spec_id
        || Uuid::from(work.goal) != call.goal_id
    {
        return Err(AppError::Forbidden);
    }
    let permission = current_tool_permission(
        transaction,
        call.project_id,
        actor.identity_id,
        &call.tool_id,
        call.tool_version,
    )
    .await?;
    if !permission {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn current_tool_permission(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    identity_id: Uuid,
    tool_id: &str,
    version: u32,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_tool_permissions
         WHERE project_id = $1 AND principal_identity_id = $2 AND tool_name = $3
           AND tool_version = $4 AND revoked_at IS NULL)",
    )
    .bind(project_id)
    .bind(identity_id)
    .bind(tool_id)
    .bind(i32::try_from(version).map_err(|_| AppError::BadRequest("invalid tool version"))?)
    .fetch_one(&mut **transaction)
    .await?)
}

#[derive(Deserialize)]
struct PersistedWorkToolPolicy {
    work_spec_id: u64,
    max_attempts: u16,
    policy: sprout_domain::ContractWorkSecurityPolicy,
    policy_hash_hex: String,
    tool_ceiling: Vec<String>,
}

struct ToolSecuritySnapshot {
    policy: sprout_domain::ContractWorkSecurityPolicy,
    policy_hash: [u8; 32],
    run_ceiling: Vec<String>,
    run_ceiling_hash: [u8; 32],
    work_ceiling: Vec<String>,
    work_ceiling_hash: [u8; 32],
    work_authority_origin: &'static str,
    work_authority_parent: Option<Uuid>,
    work_authority_principal: Uuid,
}

async fn load_tool_security_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
    state: &sprout_domain::CollaborativeRunState,
    work: &sprout_domain::WorkItem,
) -> Result<ToolSecuritySnapshot, AppError> {
    let row = sqlx::query(
        "SELECT run_sponsor_identity_id, run_tool_ceiling, run_tool_ceiling_hash, work_policies
         FROM agent_run_tool_security_snapshots
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(project_id)
    .bind(run_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let run_ceiling: Vec<String> =
        serde_json::from_value(row.try_get("run_tool_ceiling")?).map_err(|_| AppError::Internal)?;
    let run_ceiling_hash = row
        .try_get::<Vec<u8>, _>("run_tool_ceiling_hash")?
        .try_into()
        .map_err(|_| AppError::Internal)?;
    let policies: Vec<PersistedWorkToolPolicy> =
        serde_json::from_value(row.try_get("work_policies")?).map_err(|_| AppError::Internal)?;
    let policy = policies
        .iter()
        .find(|candidate| candidate.work_spec_id == work.work_spec_id)
        .ok_or(AppError::Forbidden)?;
    if work.attempt >= policy.max_attempts {
        return Err(AppError::Forbidden);
    }
    let mut work_ceiling = run_ceiling.clone();
    let mut cursor = Some(work);
    let mut visited = std::collections::HashSet::new();
    let mut origin_evidence = HashMap::new();
    let run_sponsor_identity_id: Uuid = row.try_get("run_sponsor_identity_id")?;
    while let Some(current) = cursor {
        if !visited.insert(current.id) {
            return Err(AppError::Conflict);
        }
        origin_evidence.insert(
            current.id,
            load_exact_work_authority_evidence(
                transaction,
                project_id,
                run_id,
                current,
                run_sponsor_identity_id,
            )
            .await?,
        );
        let current_policy = policies
            .iter()
            .find(|candidate| candidate.work_spec_id == current.work_spec_id)
            .ok_or(AppError::Forbidden)?;
        work_ceiling.retain(|tool| current_policy.tool_ceiling.contains(tool));
        cursor = match current.parent {
            Some(parent) => Some(
                state
                    .work_items
                    .get(&parent)
                    .or_else(|| state.inactive_work_items.get(&parent))
                    .ok_or(AppError::Forbidden)?,
            ),
            None => None,
        };
    }
    work_ceiling.sort();
    work_ceiling.dedup();
    let work_ceiling_hash: [u8; 32] =
        Sha256::digest(canonical_governance_json(&work_ceiling).map_err(|_| AppError::Internal)?)
            .into();
    let origin = resolve_exact_work_authority_origin(state, work.id, &origin_evidence)
        .map_err(|_| AppError::Forbidden)?;
    let (work_authority_origin, work_authority_parent, work_authority_principal) = match origin {
        ExactWorkAuthorityOrigin::RunSponsor { principal } => {
            ("run_sponsor", None, Uuid::from(principal))
        }
        ExactWorkAuthorityOrigin::InheritedWork { parent, principal } => (
            "inherited_work",
            Some(Uuid::from(parent)),
            Uuid::from(principal),
        ),
    };
    Ok(ToolSecuritySnapshot {
        policy: policy.policy.clone(),
        policy_hash: commitment(&policy.policy_hash_hex)?,
        run_ceiling,
        run_ceiling_hash,
        work_ceiling,
        work_ceiling_hash,
        work_authority_origin,
        work_authority_parent,
        work_authority_principal,
    })
}

/// Resolve only provenance already committed by the append-only run history.
/// A Task -> Work causal edge is possible/unsupported human-delegation
/// provenance, not its complete certificate. 0033 therefore fails closed.
async fn load_exact_work_authority_evidence(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
    work: &sprout_domain::WorkItem,
    run_sponsor_identity_id: Uuid,
) -> Result<ConcreteWorkAuthorityEvidence, AppError> {
    let work_id = Uuid::from(work.id);
    let task_to_work_links = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*)
        FROM agent_run_causal_links link
        WHERE link.project_id=$1 AND link.run_id=$2
          AND link.predecessor ->> 'kind' = 'task'
          AND link.successor ->> 'kind' = 'work'
          AND link.successor ->> 'work' = $3
        "#,
    )
    .bind(project_id)
    .bind(run_id)
    .bind(work_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    if task_to_work_links > 1 {
        return Ok(ConcreteWorkAuthorityEvidence::Ambiguous);
    }
    if task_to_work_links == 1 {
        return Ok(ConcreteWorkAuthorityEvidence::PossibleUnsupportedHumanDelegation);
    }
    let first = sqlx::query(
        r#"
        SELECT transition_kind, state_version, actor_identity_id,
               state_snapshot -> 'work_items' -> $3 AS work_snapshot
        FROM agent_run_transitions
        WHERE project_id=$1 AND run_id=$2
          AND state_snapshot -> 'work_items' ? $3
        ORDER BY state_version
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(run_id)
    .bind(work_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(first) = first else {
        return Ok(ConcreteWorkAuthorityEvidence::Unknown);
    };
    let first_work: sprout_domain::WorkItem =
        serde_json::from_value(first.try_get("work_snapshot")?).map_err(|_| AppError::Internal)?;
    if first_work.id != work.id
        || first_work.run != work.run
        || first_work.goal != work.goal
        || first_work.owner != work.owner
        || first_work.serves != work.serves
        || first_work.work_spec_id != work.work_spec_id
        || first_work.slot != work.slot
        || first_work.kind != work.kind
        || first_work.parent != work.parent
        || first_work.source_comment != work.source_comment
        || first_work.created_at != work.created_at
    {
        return Ok(ConcreteWorkAuthorityEvidence::Ambiguous);
    }
    let transition_kind: String = first.try_get("transition_kind")?;
    let state_version: i64 = first.try_get("state_version")?;
    let transition_actor: Option<Uuid> = first.try_get("actor_identity_id")?;
    match work.parent {
        None if state_version == 1
            && transition_kind == "initialized"
            && transition_actor == Some(run_sponsor_identity_id) =>
        {
            Ok(ConcreteWorkAuthorityEvidence::RunInitialization {
                sponsor: run_sponsor_identity_id.into(),
            })
        }
        Some(parent)
            if state_version > 1
                && work.source_comment.is_none()
                && matches!(
                    transition_kind.as_str(),
                    "frontier_refreshed" | "work_succeeded" | "work_failed"
                ) =>
        {
            Ok(ConcreteWorkAuthorityEvidence::ContractContinuation { parent })
        }
        _ => Ok(ConcreteWorkAuthorityEvidence::Unknown),
    }
}

async fn current_runtime_capability(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: AuthSession,
    witness_id: Uuid,
    tool_id: &str,
    tool_version: u32,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
          SELECT 1
          FROM agent_tool_runtime_capability_witnesses witness
          JOIN agent_runners runner
            ON runner.project_id=witness.project_id AND runner.id=witness.runner_id
          JOIN agent_external_tool_catalog catalog
            ON catalog.tool_name=witness.tool_name AND catalog.version=witness.tool_version
          WHERE witness.id=$1 AND witness.project_id=$2
            AND witness.owner_identity_id=$3 AND witness.signer_device_id=$4
            AND witness.tool_name=$5 AND witness.tool_version=$6
            AND witness.manifest_hash=catalog.manifest_hash
            AND witness.issued_at <= clock_timestamp()
            AND clock_timestamp() < witness.expires_at
            AND runner.principal_identity_id=$3 AND runner.device_id=$4
            AND runner.state='active'
        )
        "#,
    )
    .bind(witness_id)
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(tool_id)
    .bind(i32::try_from(tool_version).map_err(|_| AppError::BadRequest("invalid tool version"))?)
    .fetch_one(&mut **transaction)
    .await?)
}

fn commitment(value: &str) -> Result<[u8; 32], AppError> {
    let bytes = hex::decode(value).map_err(|_| AppError::BadRequest("invalid commitment"))?;
    bytes
        .try_into()
        .map_err(|_| AppError::BadRequest("invalid commitment"))
}

fn digest(value: &impl Serialize) -> Result<Vec<u8>, AppError> {
    let canonical = canonical_governance_json(value).map_err(|_| AppError::Internal)?;
    Ok(Sha256::digest(canonical).to_vec())
}

fn status_response(status: String) -> &'static str {
    match status.as_str() {
        "pending" => "pending",
        "succeeded" => "succeeded",
        "failed" => "failed",
        "timed_out" => "timed_out",
        _ => "invalid",
    }
}

struct AuditCoordinates<'a> {
    project_id: Uuid,
    call_id: Uuid,
    run_id: Uuid,
    goal_id: Uuid,
    work_item_id: Uuid,
    work_claim_id: Uuid,
    work_attempt: u16,
    owner: Uuid,
    tool_id: &'a str,
    tool_version: u32,
    attempt: u16,
}

impl<'a> AuditCoordinates<'a> {
    #[allow(clippy::too_many_arguments)]
    const fn new(
        project_id: Uuid,
        call_id: Uuid,
        run_id: Uuid,
        goal_id: Uuid,
        work_item_id: Uuid,
        work_claim_id: Uuid,
        work_attempt: u16,
        owner: Uuid,
        tool_id: &'a str,
        tool_version: u32,
        attempt: u16,
    ) -> Self {
        Self {
            project_id,
            call_id,
            run_id,
            goal_id,
            work_item_id,
            work_claim_id,
            work_attempt,
            owner,
            tool_id,
            tool_version,
            attempt,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    coordinates: AuditCoordinates<'_>,
    kind: &str,
    status: &str,
    input_commitment: [u8; 32],
    max_attempts: u16,
    timeout_seconds: u32,
    observation_id: Option<Uuid>,
    idempotency_key: Uuid,
) -> Result<(), AppError> {
    let event_hash = digest(&json!({
        "project_id": coordinates.project_id,
        "call_id": coordinates.call_id,
        "run_id": coordinates.run_id,
        "goal_id": coordinates.goal_id,
        "work_item_id": coordinates.work_item_id,
        "work_claim_id": coordinates.work_claim_id,
        "work_attempt": coordinates.work_attempt,
        "owner": coordinates.owner,
        "tool_id": coordinates.tool_id,
        "tool_version": coordinates.tool_version,
        "attempt": coordinates.attempt,
        "kind": kind,
        "status": status,
        "observation_id": observation_id,
        "idempotency_key": idempotency_key,
    }))?;
    insert_audit_with_hash(
        transaction,
        coordinates,
        kind,
        status,
        input_commitment,
        max_attempts,
        timeout_seconds,
        observation_id,
        idempotency_key,
        event_hash,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit_with_hash(
    transaction: &mut Transaction<'_, Postgres>,
    coordinates: AuditCoordinates<'_>,
    kind: &str,
    status: &str,
    input_commitment: [u8; 32],
    max_attempts: u16,
    timeout_seconds: u32,
    observation_id: Option<Uuid>,
    idempotency_key: Uuid,
    event_hash: Vec<u8>,
) -> Result<(), AppError> {
    let output_commitment = sqlx::query_scalar::<_, Option<Vec<u8>>>(
        "SELECT current_output_commitment FROM agent_tool_calls
         WHERE project_id=$1 AND id=$2",
    )
    .bind(coordinates.project_id)
    .bind(coordinates.call_id)
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO agent_tool_audit (
            id, project_id, call_id, run_id, goal_id, work_item_id, work_claim_id,
            work_attempt, owner_identity_id, tool_name, tool_version, attempt, kind,
            call_snapshot, observation_id, event_hash, idempotency_key
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
    )
    .bind(Uuid::now_v7())
    .bind(coordinates.project_id)
    .bind(coordinates.call_id)
    .bind(coordinates.run_id)
    .bind(coordinates.goal_id)
    .bind(coordinates.work_item_id)
    .bind(coordinates.work_claim_id)
    .bind(i32::from(coordinates.work_attempt))
    .bind(coordinates.owner)
    .bind(coordinates.tool_id)
    .bind(i32::try_from(coordinates.tool_version).map_err(|_| AppError::Internal)?)
    .bind(i32::from(coordinates.attempt))
    .bind(kind)
    .bind(json!({
        "call_id": coordinates.call_id,
        "canonical_input_commitment_hex": hex::encode(input_commitment),
        "canonical_output_commitment_hex": output_commitment.map(hex::encode),
        "max_attempts": max_attempts,
        "timeout_seconds": timeout_seconds,
        "status": status,
    }))
    .bind(observation_id)
    .bind(event_hash)
    .bind(idempotency_key)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitment_is_exactly_sha256_sized() {
        assert!(commitment(&"00".repeat(32)).is_ok());
        assert!(commitment(&"00".repeat(31)).is_err());
    }
}
