use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sprout_domain::{
    AgentActionClass, AgentAvailabilityMode, AgentId, AgentInterrogationCausalDelta,
    AgentInterrogationSession, AuthorityEnvelope, CollaborativeRunState, ContractCondition,
    ContractConditionFacts, CrossOwnerAssignmentRoute, CurrentLocalObligationContext,
    EncryptedPayload, GlobalContractCandidate, GovernedAgent, InformationSource, InterrogationId,
    InvocationId, LocalGoalContract, LocalGoalOrigin, ModelExposureProjection,
    ModelInvocationContext, PersistedTaskIntent, PrincipalKind, ProjectId, ProxyExecution,
    ProxyRequestId, ProxyThreadId, ResourceEffect, ResourceId, ResourceOperation,
    ResponsibilityContract, StructuredGlobalSynthesisEnvelope, StructuredGlobalWorkGrounding,
    StructuredLanguageOutput, StructuredLanguageTaskEnvelope, StructuredLanguageTaskKind,
    TaskObligationProvenance, UserId, UserProxy, UserProxyActionPlan,
    UserProxyOutOfResponsibilityConfirmation, UserProxyPlanningEnvelope, UserProxyRequest,
    UserProxyThread, responsibility_operationally_covers_local_goal, route_cross_owner_assignment,
    validate_global_synthesis, validate_information_flow, validate_state_grounded_invocation,
};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthSession, ProjectAccess, require_project_access, set_database_context},
    error::AppError,
};

const RUNNER_LEASE: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
pub struct ProvisionAgentRequest {
    id: AgentId,
    principal_identity_id: UserId,
    controller_identity_id: UserId,
    identity_handle: String,
    encrypted_profile: EncryptedPayload,
    profile_resource_node_id: ResourceId,
    encrypted_system_prompt: EncryptedPayload,
    key_epoch: u32,
    availability: AgentAvailabilityMode,
    runner_id: Uuid,
    runner_device_id: Uuid,
    encrypted_runner_label: EncryptedPayload,
}

#[derive(Serialize)]
pub struct ProvisionAgentResponse {
    agent_id: AgentId,
    principal_identity_id: UserId,
    runner_id: Uuid,
    runner_device_id: Uuid,
    bootstrap_token: SensitiveToken,
    bootstrap_expires_at: DateTime<Utc>,
    runner_state: &'static str,
}

#[derive(Serialize)]
#[serde(transparent)]
struct SensitiveToken(String);

impl std::fmt::Debug for SensitiveToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SensitiveToken([REDACTED])")
    }
}

pub async fn provision(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ProvisionAgentRequest>,
) -> Result<Json<ProvisionAgentResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    validate_identity_handle(&request.identity_handle)?;
    let governed = GovernedAgent {
        id: request.id,
        principal_id: request.principal_identity_id,
        controller_id: request.controller_identity_id,
        project_id: ProjectId::from(project_id),
        availability: request.availability,
    };
    governed
        .validate(|principal| {
            if principal == request.principal_identity_id {
                Some(PrincipalKind::Agent)
            } else if principal == request.controller_identity_id {
                Some(PrincipalKind::User)
            } else {
                None
            }
        })
        .map_err(agent_validation_error)?;

    let session_id = Uuid::new_v4();
    let token = SensitiveToken(format!(
        "v1.{}.{session_id}.{}{}",
        request.principal_identity_id,
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ));
    let token_hash = Sha256::digest(token.0.as_bytes()).to_vec();
    let expires_at = Utc::now()
        + chrono::Duration::from_std(state.config.session_ttl).map_err(|_| AppError::Internal)?;
    let profile = serialize_ciphertext(&request.encrypted_profile)?;
    let system_prompt = serialize_ciphertext(&request.encrypted_system_prompt)?;
    let runner_label = serialize_ciphertext(&request.encrypted_runner_label)?;
    let key_epoch = to_i32(request.key_epoch)?;

    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query(
        r#"
        SELECT sprout_private.provision_edge_agent(
            $1, $2, $3, $4, $5, $6, $7, $8,
            $9, $10, $11, $12, $13, $14, $15, $16
        )
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(request.id))
    .bind(Uuid::from(request.principal_identity_id))
    .bind(Uuid::from(request.controller_identity_id))
    .bind(&request.identity_handle)
    .bind(profile)
    .bind(Uuid::from(request.profile_resource_node_id))
    .bind(system_prompt)
    .bind(key_epoch)
    .bind(availability_name(request.availability))
    .bind(request.runner_id)
    .bind(request.runner_device_id)
    .bind(runner_label)
    .bind(session_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        request.id,
        None,
        "agent_provisioned",
        json!({
            "principal_identity_id": request.principal_identity_id,
            "controller_identity_id": request.controller_identity_id,
            "runner_id": request.runner_id,
            "runner_device_id": request.runner_device_id,
            "profile_resource_node_id": request.profile_resource_node_id,
            "key_epoch": request.key_epoch,
        }),
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(ProvisionAgentResponse {
        agent_id: request.id,
        principal_identity_id: request.principal_identity_id,
        runner_id: request.runner_id,
        runner_device_id: request.runner_device_id,
        bootstrap_token: token,
        bootstrap_expires_at: expires_at,
        runner_state: "pending_key",
    }))
}

pub async fn activate_local_goal(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, local_goal_id, revision)): Path<(Uuid, Uuid, Uuid, u64)>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    if agent.controller_id != actor.identity_id.into() {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 36))")
        .bind(agent_id)
        .execute(&mut *transaction)
        .await?;
    let row = sqlx::query(
        r#"
        SELECT local.contract::text AS contract, local.contract_hash,
               prompt.draft_id, prompt.encrypted_prompt, prompt.prompt_hash
        FROM agent_local_goal_contracts local
        JOIN agent_prompt_revisions prompt
          ON prompt.project_id = local.project_id
         AND prompt.agent_id = local.agent_id
         AND prompt.local_goal_id = local.id
         AND prompt.local_goal_revision = local.revision
        WHERE local.project_id = $1 AND local.agent_id = $2
          AND local.id = $3 AND local.revision = $4
          AND local.state = 'draft' AND prompt.state = 'draft'
        FOR UPDATE OF local, prompt
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(local_goal_id)
    .bind(to_i64(revision)?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let contract: LocalGoalContract =
        serde_json::from_str(row.try_get("contract")?).map_err(|_| AppError::Internal)?;
    if contract.id != local_goal_id.into()
        || contract.revision != revision
        || contract.agent != agent.principal_id
        || contract.controller != agent.controller_id
    {
        return Err(AppError::Conflict);
    }
    contract.validate().map_err(agent_validation_error)?;
    let prompt_bytes = serialize_ciphertext(&contract.encrypted_prompt)?;
    let stored_prompt: Vec<u8> = row.try_get("encrypted_prompt")?;
    let stored_prompt_hash: Vec<u8> = row.try_get("prompt_hash")?;
    let prompt_draft_id: Uuid = row.try_get("draft_id")?;
    let expected_prompt_hash: [u8; 32] = Sha256::digest(&prompt_bytes).into();
    if stored_prompt != prompt_bytes || stored_prompt_hash != expected_prompt_hash {
        return Err(AppError::Conflict);
    }
    for clause in &contract.clauses {
        if !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            Uuid::from(clause.scope),
            ResourceOperation::Read,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    }
    let controller_role = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_memberships
         WHERE project_id = $1 AND identity_id = $2 AND state = 'active'",
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let admin_governed = matches!(controller_role.as_str(), "owner" | "admin");
    if !admin_governed {
        let responsibility = load_current_responsibility(
            &mut transaction,
            project_id,
            Uuid::from(contract.controller),
        )
        .await?
        .ok_or(AppError::Forbidden)?;
        if !responsibility_operationally_covers(
            &mut transaction,
            project_id,
            &responsibility,
            &contract,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    } else if !resource_access_in_transaction(
        &mut transaction,
        project_id,
        actor.identity_id,
        Uuid::from(contract.contract.scope),
        ResourceOperation::Manage,
    )
    .await?
    {
        return Err(AppError::Forbidden);
    }

    if revision > 1 {
        let supersedes_revision = contract.supersedes_revision.ok_or(AppError::Conflict)?;
        let superseded = sqlx::query(
            "UPDATE agent_local_goal_contracts
             SET state = 'superseded', terminal_at = clock_timestamp()
             WHERE project_id = $1 AND agent_id = $2 AND id = $3
               AND revision = $4 AND state = 'active'",
        )
        .bind(project_id)
        .bind(agent_id)
        .bind(local_goal_id)
        .bind(to_i64(supersedes_revision)?)
        .execute(&mut *transaction)
        .await?;
        if superseded.rows_affected() != 1 {
            return Err(AppError::Conflict);
        }
        let superseded_prompt = sqlx::query(
            "UPDATE agent_prompt_revisions
             SET state = 'superseded', superseded_at = clock_timestamp()
             WHERE project_id = $1 AND agent_id = $2 AND local_goal_id = $3
               AND local_goal_revision = $4 AND state = 'active'",
        )
        .bind(project_id)
        .bind(agent_id)
        .bind(local_goal_id)
        .bind(to_i64(supersedes_revision)?)
        .execute(&mut *transaction)
        .await?;
        if superseded_prompt.rows_affected() != 1 {
            return Err(AppError::Conflict);
        }
    } else if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_local_goal_contracts
         WHERE project_id = $1 AND agent_id = $2 AND state = 'active')",
    )
    .bind(project_id)
    .bind(agent_id)
    .fetch_one(&mut *transaction)
    .await?
    {
        return Err(AppError::Conflict);
    }
    let activated_local = sqlx::query(
        "UPDATE agent_local_goal_contracts SET state = 'active'
         WHERE project_id = $1 AND agent_id = $2 AND id = $3
           AND revision = $4 AND state = 'draft'",
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(local_goal_id)
    .bind(to_i64(revision)?)
    .execute(&mut *transaction)
    .await?;
    if activated_local.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let activated_prompt = sqlx::query(
        "UPDATE agent_prompt_revisions
         SET state = 'active', approved_by_identity_id = $5,
             activated_at = clock_timestamp()
         WHERE project_id = $1 AND agent_id = $2 AND local_goal_id = $3
           AND local_goal_revision = $4 AND state = 'draft'",
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(local_goal_id)
    .bind(to_i64(revision)?)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    if activated_prompt.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let updated_prompt = sqlx::query(
        "UPDATE governed_agents SET encrypted_system_prompt = $3
         WHERE project_id = $1 AND id = $2 AND controller_identity_id = $4
           AND state = 'active'",
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(prompt_bytes)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    if updated_prompt.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let final_approval = sqlx::query(
        "INSERT INTO agent_prompt_final_approvals (
             project_id, draft_id, agent_id, controller_identity_id,
             local_goal_id, local_goal_revision, prompt_hash
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(project_id)
    .bind(prompt_draft_id)
    .bind(agent_id)
    .bind(actor.identity_id)
    .bind(local_goal_id)
    .bind(to_i64(revision)?)
    .bind(expected_prompt_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    if final_approval.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    persist_cross_owner_task_provenance(&mut transaction, project_id, agent_id, &contract).await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        None,
        "local_goal_recorded",
        json!({
            "local_goal_id": local_goal_id,
            "revision": revision,
            "state": "active",
            "prompt_draft_id": prompt_draft_id,
            "prompt_hash": hex::encode(expected_prompt_hash),
            "governance": if admin_governed { "project_administrator" } else { "responsibility" },
        }),
    )
    .await?;
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        actor.identity_id,
        Some(agent_id),
        "local_goal_activated",
        json!({
            "local_goal_id": local_goal_id,
            "revision": revision,
            "prompt_draft_id": prompt_draft_id,
            "prompt_hash": hex::encode(expected_prompt_hash),
            "governance": if admin_governed { "project_administrator" } else { "responsibility" },
        }),
    )
    .await?;
    transaction.commit().await?;
    let contract_hash: Vec<u8> = row.try_get("contract_hash")?;
    Ok(Json(ContractRecordedResponse {
        id: local_goal_id,
        revision,
        contract_hash_hex: hex::encode(contract_hash),
    }))
}

#[derive(Serialize)]
pub struct RunnerStatusResponse {
    agent_id: AgentId,
    runner_id: Uuid,
    device_id: Uuid,
    key_version: i32,
    state: &'static str,
}

pub async fn activate_runner(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RunnerStatusResponse>, AppError> {
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT runner.id, key.key_version
        FROM governed_agents agent
        JOIN agent_runners runner
          ON runner.project_id = agent.project_id AND runner.agent_id = agent.id
        JOIN devices device
          ON device.identity_id = agent.principal_identity_id
         AND device.id = runner.device_id
        JOIN device_keys key
          ON key.identity_id = device.identity_id
         AND key.device_id = device.id
         AND key.revoked_at IS NULL
        WHERE agent.project_id = $1
          AND agent.id = $2
          AND agent.principal_identity_id = $3
          AND runner.device_id = $4
          AND agent.state = 'active'
          AND runner.state IN ('pending_key', 'active')
          AND device.device_kind = 'service'
          AND device.trust_state = 'trusted'
          AND device.retired_at IS NULL
        ORDER BY key.key_version DESC
        LIMIT 1
        FOR UPDATE OF runner
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let runner_id: Uuid = row.try_get("id")?;
    let key_version: i32 = row.try_get("key_version")?;
    sqlx::query(
        r#"
        UPDATE agent_runners
        SET state = 'active', activated_key_version = $3,
            activated_at = COALESCE(activated_at, clock_timestamp()),
            last_seen_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(project_id)
    .bind(runner_id)
    .bind(key_version)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        None,
        "runner_activated",
        json!({
            "runner_id": runner_id,
            "device_id": actor.device_id,
            "key_version": key_version,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(RunnerStatusResponse {
        agent_id: AgentId::from(agent_id),
        runner_id,
        device_id: actor.device_id,
        key_version,
        state: "active",
    }))
}

#[derive(Deserialize)]
pub struct RecordResponsibilityRequest {
    contract: ResponsibilityContract,
}

#[derive(Serialize)]
pub struct ContractRecordedResponse {
    id: Uuid,
    revision: u64,
    contract_hash_hex: String,
}

pub async fn record_responsibility(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, responsibility_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RecordResponsibilityRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent
        || request.contract.id != responsibility_id.into()
        || request.contract.administrator != actor.identity_id.into()
    {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    if request.contract.user != agent.controller_id {
        return Err(AppError::BadRequest(
            "responsibility user must be the agent controller",
        ));
    }
    for rule in &request.contract.rules {
        if !resource_access(
            &state,
            actor,
            project_id,
            Uuid::from(rule.scope),
            ResourceOperation::Manage,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    }
    request
        .contract
        .validate(
            |principal| {
                if principal == request.contract.administrator {
                    Some(PrincipalKind::Administrator)
                } else if principal == request.contract.user {
                    Some(PrincipalKind::User)
                } else {
                    None
                }
            },
            |_, _| true,
        )
        .map_err(agent_validation_error)?;

    let contract_json = canonical_json(&request.contract)?;
    let contract_hash: [u8; 32] = Sha256::digest(contract_json.as_bytes()).into();
    let mut transaction = begin(&state, actor, project_id).await?;
    if request.contract.revision > 1 {
        let previous_json = sqlx::query_scalar::<_, String>(
            r#"
            SELECT contract::text
            FROM agent_responsibility_contracts
            WHERE project_id = $1 AND id = $2 AND revision = $3
              AND state = 'active'
            "#,
        )
        .bind(project_id)
        .bind(responsibility_id)
        .bind(to_i64(request.contract.revision - 1)?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let previous: ResponsibilityContract =
            serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
        request
            .contract
            .validate_revision_of(&previous)
            .map_err(agent_validation_error)?;
    } else {
        if request.contract.supersedes_revision.is_some() {
            return Err(AppError::BadRequest(
                "first responsibility revision cannot supersede another revision",
            ));
        }
        let existing_for_user = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM agent_responsibility_contracts
             WHERE project_id = $1 AND user_identity_id = $2)",
        )
        .bind(project_id)
        .bind(Uuid::from(request.contract.user))
        .fetch_one(&mut *transaction)
        .await?;
        if existing_for_user {
            return Err(AppError::Conflict);
        }
    }
    sqlx::query(
        r#"
        INSERT INTO agent_responsibility_contracts (
            id, project_id, revision, administrator_identity_id,
            user_identity_id, contract, contract_hash, state
        ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, 'draft')
        "#,
    )
    .bind(responsibility_id)
    .bind(project_id)
    .bind(to_i64(request.contract.revision)?)
    .bind(actor.identity_id)
    .bind(Uuid::from(request.contract.user))
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        None,
        "responsibility_recorded",
        json!({
            "responsibility_id": responsibility_id,
            "revision": request.contract.revision,
            "contract_hash": hex::encode(contract_hash),
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: responsibility_id,
        revision: request.contract.revision,
        contract_hash_hex: hex::encode(contract_hash),
    }))
}

/// Authoritative user-level Responsibility endpoint. The agent-scoped route
/// remains a compatibility alias, but neither storage nor lifecycle ownership
/// depends on an agent record.
pub async fn record_user_responsibility(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, user_id, responsibility_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RecordResponsibilityRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent
        || request.contract.id != responsibility_id.into()
        || request.contract.administrator != actor.identity_id.into()
        || request.contract.user != user_id.into()
    {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let user_is_human_member = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM project_memberships membership
            JOIN identities identity ON identity.id = membership.identity_id
            WHERE membership.project_id = $1 AND membership.identity_id = $2
              AND membership.state = 'active' AND identity.status = 'active'
              AND identity.principal_kind = 'user'
        )
        "#,
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if !user_is_human_member {
        return Err(AppError::Forbidden);
    }
    for rule in &request.contract.rules {
        if !resource_access(
            &state,
            actor,
            project_id,
            Uuid::from(rule.scope),
            ResourceOperation::Manage,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    }
    request
        .contract
        .validate(
            |principal| {
                if principal == request.contract.administrator {
                    Some(PrincipalKind::Administrator)
                } else if principal == request.contract.user {
                    Some(PrincipalKind::User)
                } else {
                    None
                }
            },
            |_, _| true,
        )
        .map_err(agent_validation_error)?;

    let contract_json = canonical_json(&request.contract)?;
    let contract_hash: [u8; 32] = Sha256::digest(contract_json.as_bytes()).into();
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 35))")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    if request.contract.revision > 1 {
        let previous_json = sqlx::query_scalar::<_, String>(
            "SELECT contract::text FROM agent_responsibility_contracts
             WHERE project_id = $1 AND id = $2 AND revision = $3 AND state = 'active'",
        )
        .bind(project_id)
        .bind(responsibility_id)
        .bind(to_i64(request.contract.revision - 1)?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let previous: ResponsibilityContract =
            serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
        request
            .contract
            .validate_revision_of(&previous)
            .map_err(agent_validation_error)?;
    } else if request.contract.supersedes_revision.is_some()
        || sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM agent_responsibility_contracts
             WHERE project_id = $1 AND user_identity_id = $2)",
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_one(&mut *transaction)
        .await?
    {
        return Err(AppError::Conflict);
    }
    sqlx::query(
        r#"
        INSERT INTO agent_responsibility_contracts (
            id, project_id, revision, administrator_identity_id,
            user_identity_id, contract, contract_hash, state
        ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, 'draft')
        "#,
    )
    .bind(responsibility_id)
    .bind(project_id)
    .bind(to_i64(request.contract.revision)?)
    .bind(actor.identity_id)
    .bind(user_id)
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        user_id,
        None,
        "responsibility_drafted",
        json!({
            "responsibility_id": responsibility_id,
            "revision": request.contract.revision,
            "contract_hash": hex::encode(contract_hash),
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: responsibility_id,
        revision: request.contract.revision,
        contract_hash_hex: hex::encode(contract_hash),
    }))
}

pub async fn activate_user_responsibility(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, user_id, responsibility_id, revision)): Path<(Uuid, Uuid, Uuid, u64)>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 35))")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    let row = sqlx::query(
        "SELECT contract::text AS contract, contract_hash
         FROM agent_responsibility_contracts
         WHERE project_id = $1 AND user_identity_id = $2 AND id = $3
           AND revision = $4 AND state = 'draft'
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(responsibility_id)
    .bind(to_i64(revision)?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let contract: ResponsibilityContract =
        serde_json::from_str(row.try_get("contract")?).map_err(|_| AppError::Internal)?;
    if contract.administrator != actor.identity_id.into()
        || contract.user != user_id.into()
        || contract.revision != revision
    {
        return Err(AppError::Forbidden);
    }
    for rule in &contract.rules {
        if !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            Uuid::from(rule.scope),
            ResourceOperation::Manage,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    }
    if revision > 1 {
        let supersedes_revision = contract.supersedes_revision.ok_or(AppError::Conflict)?;
        let previous_json = sqlx::query_scalar::<_, String>(
            "SELECT contract::text FROM agent_responsibility_contracts
             WHERE project_id = $1 AND user_identity_id = $2 AND id = $3
               AND revision = $4 AND state = 'active'
             FOR UPDATE",
        )
        .bind(project_id)
        .bind(user_id)
        .bind(responsibility_id)
        .bind(to_i64(supersedes_revision)?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let previous: ResponsibilityContract =
            serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
        contract
            .validate_revision_of(&previous)
            .map_err(agent_validation_error)?;
        let updated = sqlx::query(
            "UPDATE agent_responsibility_contracts
             SET state = 'superseded', superseded_at = clock_timestamp()
             WHERE project_id = $1 AND user_identity_id = $2 AND id = $3
               AND revision = $4 AND state = 'active'",
        )
        .bind(project_id)
        .bind(user_id)
        .bind(responsibility_id)
        .bind(to_i64(supersedes_revision)?)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AppError::Conflict);
        }
    } else if contract.supersedes_revision.is_some()
        || sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM agent_responsibility_contracts
             WHERE project_id = $1 AND user_identity_id = $2 AND state = 'active')",
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_one(&mut *transaction)
        .await?
    {
        return Err(AppError::Conflict);
    }
    let activated = sqlx::query(
        "UPDATE agent_responsibility_contracts
         SET state = 'active', activated_at = clock_timestamp()
         WHERE project_id = $1 AND user_identity_id = $2 AND id = $3
           AND revision = $4 AND state = 'draft'",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(responsibility_id)
    .bind(to_i64(revision)?)
    .execute(&mut *transaction)
    .await?;
    if activated.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        user_id,
        None,
        "responsibility_activated",
        json!({"responsibility_id": responsibility_id, "revision": revision}),
    )
    .await?;
    transaction.commit().await?;
    let contract_hash: Vec<u8> = row.try_get("contract_hash")?;
    Ok(Json(ContractRecordedResponse {
        id: responsibility_id,
        revision,
        contract_hash_hex: hex::encode(contract_hash),
    }))
}

#[derive(Deserialize)]
pub struct RecordLocalGoalRequest {
    contract: LocalGoalContract,
}

pub async fn record_local_goal(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RecordLocalGoalRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    if agent.controller_id != actor.identity_id.into()
        || request.contract.agent != agent.principal_id
        || request.contract.controller != agent.controller_id
    {
        return Err(AppError::Forbidden);
    }
    request
        .contract
        .validate()
        .map_err(agent_validation_error)?;
    if !matches!(
        request.contract.origin,
        LocalGoalOrigin::ControllerPrompt {}
    ) {
        return Err(AppError::BadRequest(
            "local-goal origin requires a persisted governance certificate adapter",
        ));
    }
    for clause in &request.contract.clauses {
        if !resource_access(
            &state,
            actor,
            project_id,
            Uuid::from(clause.scope),
            ResourceOperation::Read,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    }
    let contract_json = canonical_json(&request.contract)?;
    let contract_hash: [u8; 32] = Sha256::digest(contract_json.as_bytes()).into();
    let mut transaction = begin(&state, actor, project_id).await?;
    if request.contract.revision > 1 {
        let previous_json = sqlx::query_scalar::<_, String>(
            r#"
            SELECT contract::text
            FROM agent_local_goal_contracts
            WHERE project_id = $1 AND id = $2 AND revision = $3 AND agent_id = $4
              AND state = 'active'
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(request.contract.id))
        .bind(to_i64(request.contract.revision - 1)?)
        .bind(agent_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let previous: LocalGoalContract =
            serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
        request
            .contract
            .validate_revision_of(&previous)
            .map_err(agent_validation_error)?;
    } else if request.contract.supersedes_revision.is_some() {
        return Err(AppError::BadRequest(
            "first local-goal revision cannot supersede another revision",
        ));
    }
    sqlx::query(
        r#"
        INSERT INTO agent_local_goal_contracts (
            id, project_id, agent_id, agent_identity_id,
            controller_identity_id, revision, contract, contract_hash, state
        ) VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8, 'draft')
        "#,
    )
    .bind(Uuid::from(request.contract.id))
    .bind(project_id)
    .bind(agent_id)
    .bind(Uuid::from(request.contract.agent))
    .bind(Uuid::from(request.contract.controller))
    .bind(to_i64(request.contract.revision)?)
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    let encrypted_prompt = serialize_ciphertext(&request.contract.encrypted_prompt)?;
    let prompt_hash: [u8; 32] = Sha256::digest(&encrypted_prompt).into();
    sqlx::query(
        r#"
        INSERT INTO agent_prompt_revisions (
            project_id, agent_id, local_goal_id, local_goal_revision,
            encrypted_prompt, prompt_hash, state
        ) VALUES ($1, $2, $3, $4, $5, $6, 'draft')
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(Uuid::from(request.contract.id))
    .bind(to_i64(request.contract.revision)?)
    .bind(encrypted_prompt)
    .bind(prompt_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        None,
        "local_goal_recorded",
        json!({
            "local_goal_id": request.contract.id,
            "revision": request.contract.revision,
            "contract_hash": hex::encode(contract_hash),
            "state": "draft",
            "prompt_hash": hex::encode(prompt_hash),
        }),
    )
    .await?;
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        Uuid::from(request.contract.controller),
        Some(agent_id),
        "local_goal_drafted",
        json!({
            "local_goal_id": request.contract.id,
            "revision": request.contract.revision,
            "contract_hash": hex::encode(contract_hash),
            "prompt_hash": hex::encode(prompt_hash),
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: Uuid::from(request.contract.id),
        revision: request.contract.revision,
        contract_hash_hex: hex::encode(contract_hash),
    }))
}

#[derive(Deserialize)]
pub struct RecordGlobalContractRequest {
    id: Uuid,
    #[serde(default)]
    synthesis_invocation_id: Option<InvocationId>,
    envelope: StructuredGlobalSynthesisEnvelope,
    candidate: GlobalContractCandidate,
    groundings: Vec<StructuredGlobalWorkGrounding>,
}

pub async fn record_global_contract(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<RecordGlobalContractRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    let synthesizer_agent_id = if actor.is_agent {
        let invocation_id = request.synthesis_invocation_id.ok_or(AppError::Forbidden)?;
        Some(
            validate_synthesis_runner(
                &state,
                actor,
                project_id,
                invocation_id,
                &request.envelope.language_task,
            )
            .await?,
        )
    } else {
        if request.synthesis_invocation_id.is_some() {
            return Err(AppError::BadRequest(
                "administrator-client synthesis must not claim a runner invocation",
            ));
        }
        require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
        None
    };
    let mut transaction = begin(&state, actor, project_id).await?;
    if request.candidate.revision > 1 {
        let previous_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM agent_global_contracts
             WHERE project_id = $1 AND id = $2 AND revision = $3)",
        )
        .bind(project_id)
        .bind(request.id)
        .bind(to_i64(request.candidate.revision - 1)?)
        .fetch_one(&mut *transaction)
        .await?;
        if !previous_exists {
            return Err(AppError::Conflict);
        }
    }
    let mut local_goals = HashMap::new();
    let mut authorized_sources = HashSet::new();
    let mut source_records = Vec::new();
    for source_agent in &request.envelope.source_agents {
        let contribution = request
            .candidate
            .contributions
            .iter()
            .find(|contribution| contribution.agent == *source_agent)
            .ok_or(AppError::BadRequest(
                "global source agent has no local contribution",
            ))?;
        let row = sqlx::query(
            r#"
            SELECT local.id, local.contract::text AS contract, agent.id AS agent_id
            FROM agent_local_goal_contracts local
            JOIN governed_agents agent
              ON agent.project_id = local.project_id
             AND agent.id = local.agent_id
            WHERE local.project_id = $1
              AND local.agent_identity_id = $2
              AND local.revision = $3
              AND local.state = 'active'
              AND agent.state = 'active'
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(*source_agent))
        .bind(to_i64(contribution.local_revision)?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let local_goal: LocalGoalContract =
            serde_json::from_str(row.try_get("contract")?).map_err(|_| AppError::Internal)?;
        if !local_goal.can_contribute_bottom_up() {
            return Err(AppError::BadRequest(
                "global mandates cannot be recycled as bottom-up sources",
            ));
        }
        let responsibility = load_current_responsibility(
            &mut transaction,
            project_id,
            Uuid::from(local_goal.controller),
        )
        .await?
        .ok_or(AppError::Conflict)?;
        if !responsibility_operationally_covers(
            &mut transaction,
            project_id,
            &responsibility,
            &local_goal,
        )
        .await?
        {
            return Err(AppError::Conflict);
        }
        authorized_sources.insert(*source_agent);
        source_records.push((
            row.try_get::<Uuid, _>("agent_id")?,
            row.try_get::<Uuid, _>("id")?,
            contribution.local_revision,
        ));
        local_goals.insert(*source_agent, local_goal);
    }
    validate_global_synthesis(
        &request.envelope,
        &request.candidate,
        &request.groundings,
        &local_goals,
        |local| authorized_sources.contains(&local.agent),
    )
    .map_err(agent_validation_error)?;
    let envelope_json = canonical_json(&request.envelope)?;
    let candidate_json = canonical_json(&request.candidate)?;
    let groundings_json = canonical_json(&request.groundings)?;
    let contract_hash: [u8; 32] = Sha256::digest(
        canonical_json(&json!({
            "id": request.id,
            "envelope": request.envelope,
            "candidate": request.candidate,
            "groundings": request.groundings,
        }))?
        .as_bytes(),
    )
    .into();
    sqlx::query(
        r#"
        INSERT INTO agent_global_contracts (
            id, project_id, revision, synthesis_envelope, candidate,
            groundings, synthesis_invocation_id, synthesized_by_agent_id,
            contract_hash, recorded_by_identity_id
        ) VALUES (
            $1, $2, $3, $4::jsonb, $5::jsonb, $6::jsonb,
            $7, $8, $9, $10
        )
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(to_i64(request.candidate.revision)?)
    .bind(envelope_json)
    .bind(candidate_json)
    .bind(groundings_json)
    .bind(request.synthesis_invocation_id.map(Uuid::from))
    .bind(synthesizer_agent_id)
    .bind(contract_hash.as_slice())
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    for (agent_id, local_goal_id, local_revision) in source_records {
        sqlx::query(
            r#"
            INSERT INTO agent_global_contract_sources (
                project_id, global_contract_id, global_revision,
                agent_id, local_goal_id, local_revision
            ) VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(project_id)
        .bind(request.id)
        .bind(to_i64(request.candidate.revision)?)
        .bind(agent_id)
        .bind(local_goal_id)
        .bind(to_i64(local_revision)?)
        .execute(&mut *transaction)
        .await?;
        if synthesizer_agent_id.is_none() {
            append_audit(
                &mut transaction,
                actor,
                project_id,
                AgentId::from(agent_id),
                None,
                "global_contract_recorded",
                json!({
                    "global_contract_id": request.id,
                    "revision": request.candidate.revision,
                    "local_goal_id": local_goal_id,
                    "local_revision": local_revision,
                    "contract_hash": hex::encode(contract_hash),
                    "synthesis_boundary": "administrator_client",
                }),
            )
            .await?;
        }
    }
    if let Some(agent_id) = synthesizer_agent_id {
        append_audit(
            &mut transaction,
            actor,
            project_id,
            AgentId::from(agent_id),
            request.synthesis_invocation_id,
            "global_contract_recorded",
            json!({
                "global_contract_id": request.id,
                "revision": request.candidate.revision,
                "contract_hash": hex::encode(contract_hash),
                "synthesis_boundary": "authorized_edge_runner",
            }),
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: request.id,
        revision: request.candidate.revision,
        contract_hash_hex: hex::encode(contract_hash),
    }))
}

#[derive(Deserialize)]
pub struct RecordInterrogationRequest {
    id: InterrogationId,
    transcript_resource_node_id: ResourceId,
    key_epoch: u32,
    encrypted_transcript: EncryptedPayload,
    causal_delta: AgentInterrogationCausalDelta,
}

#[derive(Serialize)]
pub struct RecordedInterrogationResponse {
    id: InterrogationId,
    target_agent: UserId,
    created_at: DateTime<Utc>,
    read_only: bool,
}

#[derive(Serialize)]
pub struct InterrogationResponse {
    id: InterrogationId,
    target_agent: UserId,
    transcript_resource_node_id: ResourceId,
    key_epoch: u32,
    encrypted_transcript: EncryptedPayload,
    created_at: DateTime<Utc>,
}

pub async fn record_interrogation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RecordInterrogationRequest>,
) -> Result<Json<RecordedInterrogationResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    if agent.controller_id != actor.identity_id.into() {
        return Err(AppError::Forbidden);
    }
    request
        .causal_delta
        .validate_read_only()
        .map_err(agent_validation_error)?;
    let created_at = Utc::now();
    let interrogation = AgentInterrogationSession {
        id: request.id,
        creator: actor.identity_id.into(),
        target_agent: agent.principal_id,
        created_at,
        via_tool_call: None,
    };
    if !interrogation.transcript_readable_by(actor.identity_id.into())
        || !resource_access(
            &state,
            actor,
            project_id,
            request.transcript_resource_node_id.into(),
            ResourceOperation::Read,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    let transcript = serialize_ciphertext(&request.encrypted_transcript)?;
    let delta_json = canonical_json(&request.causal_delta)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let epoch_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM resource_epochs
         WHERE project_id = $1 AND resource_node_id = $2 AND epoch = $3
           AND retired_at IS NULL)",
    )
    .bind(project_id)
    .bind(Uuid::from(request.transcript_resource_node_id))
    .bind(to_i32(request.key_epoch)?)
    .fetch_one(&mut *transaction)
    .await?;
    if !epoch_active {
        return Err(AppError::BadRequest("resource key epoch is not active"));
    }
    sqlx::query(
        r#"
        INSERT INTO agent_interrogations (
            id, project_id, creator_identity_id, target_agent_id,
            target_agent_identity_id, transcript_resource_node_id,
            key_epoch, encrypted_transcript, causal_delta, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::jsonb, $10)
        "#,
    )
    .bind(Uuid::from(request.id))
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(agent_id)
    .bind(Uuid::from(agent.principal_id))
    .bind(Uuid::from(request.transcript_resource_node_id))
    .bind(to_i32(request.key_epoch)?)
    .bind(transcript)
    .bind(delta_json)
    .bind(created_at)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        None,
        "interrogation_recorded",
        json!({
            "interrogation_id": request.id,
            "creator": actor.identity_id,
            "transcript_resource_node_id": request.transcript_resource_node_id,
            "key_epoch": request.key_epoch,
            "causal_delta_empty": true,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(RecordedInterrogationResponse {
        id: request.id,
        target_agent: agent.principal_id,
        created_at,
        read_only: true,
    }))
}

pub async fn get_interrogation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, interrogation_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<InterrogationResponse>, AppError> {
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT interrogation.id, interrogation.target_agent_identity_id,
               interrogation.transcript_resource_node_id,
               interrogation.key_epoch, interrogation.encrypted_transcript,
               interrogation.created_at
        FROM agent_interrogations interrogation
        WHERE interrogation.project_id = $1
          AND interrogation.target_agent_id = $2
          AND interrogation.id = $3
          AND interrogation.creator_identity_id = $4
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(interrogation_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(Json(InterrogationResponse {
        id: InterrogationId::from(row.try_get::<Uuid, _>("id")?),
        target_agent: UserId::from(row.try_get::<Uuid, _>("target_agent_identity_id")?),
        transcript_resource_node_id: ResourceId::from(
            row.try_get::<Uuid, _>("transcript_resource_node_id")?,
        ),
        key_epoch: u32::try_from(row.try_get::<i32, _>("key_epoch")?)
            .map_err(|_| AppError::Internal)?,
        encrypted_transcript: deserialize_ciphertext(row.try_get("encrypted_transcript")?)?,
        created_at: row.try_get("created_at")?,
    }))
}

#[derive(Deserialize)]
pub struct CreateProxyThreadRequest {
    proxy_id: Uuid,
    thread_id: ProxyThreadId,
}

#[derive(Serialize)]
pub struct ProxyThreadResponse {
    proxy_id: Uuid,
    thread_id: ProxyThreadId,
    created_at: DateTime<Utc>,
}

pub async fn create_proxy_thread(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateProxyThreadRequest>,
) -> Result<Json<ProxyThreadResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let created_at = Utc::now();
    let proxy = UserProxy {
        id: request.proxy_id,
        user: actor.identity_id.into(),
    };
    let thread = UserProxyThread {
        id: request.thread_id,
        proxy_id: request.proxy_id,
        creator: actor.identity_id.into(),
        created_at,
    };
    if !thread.valid_for(&proxy) || !thread.readable_by(actor.identity_id.into()) {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query(
        r#"
        INSERT INTO user_proxies (id, project_id, user_identity_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (project_id, user_identity_id) DO NOTHING
        "#,
    )
    .bind(request.proxy_id)
    .bind(project_id)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    let actual_proxy_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM user_proxies WHERE project_id = $1 AND user_identity_id = $2",
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if actual_proxy_id != request.proxy_id {
        return Err(AppError::Conflict);
    }
    sqlx::query(
        "INSERT INTO user_proxy_threads (
             id, project_id, proxy_id, creator_identity_id, created_at
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::from(request.thread_id))
    .bind(project_id)
    .bind(request.proxy_id)
    .bind(actor.identity_id)
    .bind(created_at)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ProxyThreadResponse {
        proxy_id: request.proxy_id,
        thread_id: request.thread_id,
        created_at,
    }))
}

#[derive(Deserialize)]
pub struct SubmitProxyRequest {
    id: ProxyRequestId,
    encrypted_payload: EncryptedPayload,
}

#[derive(Serialize)]
pub struct ProxyRequestResponse {
    id: ProxyRequestId,
    thread_id: ProxyThreadId,
    submitted_at: DateTime<Utc>,
}

pub async fn submit_proxy_request(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, thread_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<SubmitProxyRequest>,
) -> Result<Json<ProxyRequestResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let submitted_at = Utc::now();
    let encrypted_payload = serialize_ciphertext(&request.encrypted_payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let owned = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM user_proxy_threads
         WHERE project_id = $1 AND id = $2 AND creator_identity_id = $3
           AND closed_at IS NULL)",
    )
    .bind(project_id)
    .bind(thread_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if !owned {
        return Err(AppError::NotFound);
    }
    sqlx::query(
        "INSERT INTO user_proxy_requests (
             id, project_id, thread_id, user_identity_id,
             encrypted_payload, submitted_at
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::from(request.id))
    .bind(project_id)
    .bind(thread_id)
    .bind(actor.identity_id)
    .bind(encrypted_payload)
    .bind(submitted_at)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ProxyRequestResponse {
        id: request.id,
        thread_id: ProxyThreadId::from(thread_id),
        submitted_at,
    }))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyActionClassification {
    resource_id: ResourceId,
    action: AgentActionClass,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordProxyPlanRequest {
    id: Uuid,
    invocation_id: Option<InvocationId>,
    envelope: UserProxyPlanningEnvelope,
    plan: UserProxyActionPlan,
    confirmation: Option<UserProxyOutOfResponsibilityConfirmation>,
}

#[derive(Serialize)]
pub struct ProxyPlanResponse {
    id: Uuid,
    within_responsibility: bool,
    confirmation_required: bool,
    plan_hash_hex: String,
}

pub async fn record_proxy_plan(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, request_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RecordProxyPlanRequest>,
) -> Result<Json<ProxyPlanResponse>, AppError> {
    if actor.is_agent
        || request.plan.request_id != ProxyRequestId::from(request_id)
        || request.envelope.request_id != ProxyRequestId::from(request_id)
        || request.plan.user != actor.identity_id.into()
    {
        return Err(AppError::Forbidden);
    }
    let action_classification = derive_proxy_action_classification(&request.plan)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let mut permission_facts = HashSet::new();
    for effect in &request.plan.resource_effects {
        if resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            Uuid::from(effect.resource_id),
            effect.operation,
        )
        .await?
        {
            permission_facts.insert((effect.resource_id, effect.operation));
        }
    }
    let record = sqlx::query(
        r#"
        SELECT request.thread_id, request.encrypted_payload, request.submitted_at,
               thread.proxy_id, thread.created_at AS thread_created_at
        FROM user_proxy_requests request
        JOIN user_proxy_threads thread
          ON thread.project_id = request.project_id AND thread.id = request.thread_id
        WHERE request.project_id = $1 AND request.id = $2
          AND request.user_identity_id = $3 AND thread.closed_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(request_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let thread_id: Uuid = record.try_get("thread_id")?;
    let proxy = UserProxy {
        id: record.try_get("proxy_id")?,
        user: actor.identity_id.into(),
    };
    let thread = UserProxyThread {
        id: ProxyThreadId::from(thread_id),
        proxy_id: proxy.id,
        creator: actor.identity_id.into(),
        created_at: record.try_get("thread_created_at")?,
    };
    let proxy_request = UserProxyRequest {
        id: ProxyRequestId::from(request_id),
        thread_id: thread.id,
        user: actor.identity_id.into(),
        encrypted_payload: deserialize_ciphertext(record.try_get("encrypted_payload")?)?,
        submitted_at: record.try_get("submitted_at")?,
    };
    let (responsibility, within_responsibility) = load_proxy_responsibility(
        &mut transaction,
        project_id,
        actor.identity_id,
        &action_classification,
    )
    .await?;
    let execution = ProxyExecution {
        proxy: &proxy,
        thread: &thread,
        request: &proxy_request,
        envelope: &request.envelope,
        plan: &request.plan,
        within_responsibility,
        confirmation: request.confirmation.as_ref(),
    };
    execution
        .validate(
            |user, effect| {
                user == actor.identity_id.into()
                    && permission_facts.contains(&(effect.resource_id, effect.operation))
            },
            |_, _| false,
        )
        .map_err(agent_validation_error)?;
    if let Some(invocation_id) = request.invocation_id {
        let valid_invocation = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM agent_invocations
             WHERE project_id = $1 AND id = $2 AND status = 'succeeded')",
        )
        .bind(project_id)
        .bind(Uuid::from(invocation_id))
        .fetch_one(&mut *transaction)
        .await?;
        if !valid_invocation {
            return Err(AppError::Conflict);
        }
    }
    let plan_hash: [u8; 32] = Sha256::digest(
        canonical_json(&json!({
            "id": request.id,
            "request_id": request_id,
            "invocation_id": request.invocation_id,
            "envelope": request.envelope,
            "plan": request.plan,
            "action_classification": &action_classification,
            "responsibility": responsibility.as_ref().map(|contract| (contract.id, contract.revision)),
            "confirmation": request.confirmation,
        }))?
        .as_bytes(),
    )
    .into();
    sqlx::query(
        r#"
        INSERT INTO user_proxy_plans (
            id, project_id, request_id, invocation_id,
            planning_envelope, action_plan, action_classification,
            responsibility_id, responsibility_revision, confirmation, plan_hash
        ) VALUES (
            $1, $2, $3, $4, $5::jsonb, $6::jsonb, $7::jsonb,
            $8, $9, $10::jsonb, $11
        )
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(request_id)
    .bind(request.invocation_id.map(Uuid::from))
    .bind(canonical_json(&request.envelope)?)
    .bind(canonical_json(&request.plan)?)
    .bind(canonical_json(&action_classification)?)
    .bind(
        responsibility
            .as_ref()
            .map(|contract| Uuid::from(contract.id)),
    )
    .bind(
        responsibility
            .as_ref()
            .map(|contract| to_i64(contract.revision))
            .transpose()?,
    )
    .bind(
        request
            .confirmation
            .as_ref()
            .map(canonical_json)
            .transpose()?,
    )
    .bind(plan_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(ProxyPlanResponse {
        id: request.id,
        within_responsibility,
        confirmation_required: !within_responsibility,
        plan_hash_hex: hex::encode(plan_hash),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCrossOwnerAssignmentRequest {
    id: Uuid,
    target_agent_id: AgentId,
    review_task_resource_node_id: Option<ResourceId>,
}

#[derive(Serialize)]
pub struct CrossOwnerAssignmentResponse {
    id: Uuid,
    route: &'static str,
    state: &'static str,
}

pub async fn route_cross_owner_task_assignment(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_resource_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RouteCrossOwnerAssignmentRequest>,
) -> Result<Json<CrossOwnerAssignmentResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let target_agent_id = Uuid::from(request.target_agent_id);
    let mut transaction = begin(&state, actor, project_id).await?;
    if !resource_access_in_transaction(
        &mut transaction,
        project_id,
        actor.identity_id,
        task_resource_id,
        ResourceOperation::Manage,
    )
    .await?
    {
        return Err(AppError::Forbidden);
    }
    let target_row = sqlx::query(
        r#"
        SELECT principal_identity_id, controller_identity_id, availability,
               controller_is_administrator,
               responsibility_contract::text AS responsibility_contract,
               automatic_contract::text AS automatic_contract,
               automatic_state::text AS automatic_state,
               automatic_local_contract::text AS automatic_local_contract,
               automatic_work_item_id, automatic_bound_at
        FROM sprout_private.cross_owner_routing_snapshot($1, $2, $3)
        "#,
    )
    .bind(project_id)
    .bind(task_resource_id)
    .bind(target_agent_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let target_principal_id: Uuid = target_row.try_get("principal_identity_id")?;
    let target_controller_id: Uuid = target_row.try_get("controller_identity_id")?;
    if target_controller_id == actor.identity_id {
        return Err(AppError::BadRequest(
            "cross-owner routing requires a different target controller",
        ));
    }
    let target = GovernedAgent {
        id: request.target_agent_id,
        principal_id: target_principal_id.into(),
        controller_id: target_controller_id.into(),
        project_id: project_id.into(),
        availability: match target_row.try_get::<String, _>("availability")?.as_str() {
            "controller_private" => AgentAvailabilityMode::ControllerPrivate,
            "project_delegable" => AgentAvailabilityMode::ProjectDelegable,
            _ => return Err(AppError::Internal),
        },
    };
    let recorded_at = Utc::now();
    let intent_id = Uuid::new_v4();
    let intent = PersistedTaskIntent {
        task: task_resource_id.into(),
        scope: task_resource_id.into(),
        required_actions: vec![AgentActionClass::AssignOwnTask],
        created_by: actor.identity_id.into(),
        recorded_at,
    };

    let mut automatic_local = None;
    let mut automatic_provenance = None;
    let mut automatic_facts = ContractConditionFacts::default();
    if let (Some(contract_json), Some(state_json), Some(local_json), Some(work_item_uuid)) = (
        target_row.try_get::<Option<String>, _>("automatic_contract")?,
        target_row.try_get::<Option<String>, _>("automatic_state")?,
        target_row.try_get::<Option<String>, _>("automatic_local_contract")?,
        target_row.try_get::<Option<Uuid>, _>("automatic_work_item_id")?,
    ) {
        let contract = serde_json::from_str(&contract_json).map_err(|_| AppError::Internal)?;
        let run_state: CollaborativeRunState =
            serde_json::from_str(&state_json).map_err(|_| AppError::Internal)?;
        let local: LocalGoalContract =
            serde_json::from_str(&local_json).map_err(|_| AppError::Internal)?;
        let work_item_id = work_item_uuid.into();
        if let Some(work) = run_state.work_items.get(&work_item_id) {
            automatic_facts = super::agent_runs::authoritative_condition_facts(
                &mut transaction,
                project_id,
                &contract,
                &run_state,
            )
            .await?;
            automatic_provenance = Some(TaskObligationProvenance {
                task: task_resource_id.into(),
                agent: target_principal_id.into(),
                local_revision: local.revision,
                obligation: work.serves,
                work_spec_id: work.work_spec_id,
                recorded_at: target_row
                    .try_get::<Option<DateTime<Utc>>, _>("automatic_bound_at")?
                    .ok_or(AppError::Internal)?,
            });
            automatic_local = Some(local);
        }
    }

    let active_responsibility: Option<ResponsibilityContract> = target_row
        .try_get::<Option<String>, _>("responsibility_contract")?
        .map(|value| serde_json::from_str(&value).map_err(|_| AppError::Internal))
        .transpose()?;
    let mut covered_rule_scopes = HashSet::new();
    if let Some(responsibility) = &active_responsibility {
        for rule in &responsibility.rules {
            if sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM resource_closure
                 WHERE project_id = $1 AND ancestor_id = $2 AND descendant_id = $3)",
            )
            .bind(project_id)
            .bind(Uuid::from(rule.scope))
            .bind(task_resource_id)
            .fetch_one(&mut *transaction)
            .await?
            {
                covered_rule_scopes.insert(rule.scope);
            }
        }
    }
    let automatic_route = route_cross_owner_assignment(
        task_resource_id.into(),
        &target,
        CurrentLocalObligationContext {
            active_local_goal: automatic_local.as_ref(),
            provenance: automatic_provenance.as_ref(),
            condition_facts: &automatic_facts,
        },
        Some(&intent),
        active_responsibility.as_ref(),
        |rule_scope, intent_scope| {
            intent_scope == task_resource_id.into() && covered_rule_scopes.contains(&rule_scope)
        },
    );
    let controller_is_administrator: bool = target_row.try_get("controller_is_administrator")?;
    let route = if automatic_route == CrossOwnerAssignmentRoute::AutomaticFromActiveObligation {
        automatic_route
    } else if controller_is_administrator
        || automatic_route == CrossOwnerAssignmentRoute::ControllerReview
    {
        CrossOwnerAssignmentRoute::ControllerReview
    } else {
        CrossOwnerAssignmentRoute::Rejected
    };
    let (route_name, state_name) = match route {
        CrossOwnerAssignmentRoute::AutomaticFromActiveObligation => {
            if request.review_task_resource_node_id.is_some() {
                return Err(AppError::BadRequest(
                    "automatic route cannot carry a controller review task",
                ));
            }
            ("automatic_existing_obligation", "ready")
        }
        CrossOwnerAssignmentRoute::ControllerReview => {
            let review_task = request
                .review_task_resource_node_id
                .ok_or(AppError::BadRequest(
                    "controller review route requires an existing assigned review task",
                ))?;
            let valid_review = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM tasks task
                    JOIN resource_nodes node
                      ON node.project_id = task.project_id
                     AND node.id = task.resource_node_id
                    JOIN task_assignments assignment
                      ON assignment.project_id = task.project_id
                     AND assignment.task_id = task.id
                     AND assignment.revoked_at IS NULL
                    WHERE task.project_id = $1 AND task.resource_node_id = $2
                      AND task.state = 'open' AND task.deleted_at IS NULL
                      AND node.created_by_identity_id = $3
                      AND assignment.assignee_identity_id = $4
                )
                "#,
            )
            .bind(project_id)
            .bind(Uuid::from(review_task))
            .bind(actor.identity_id)
            .bind(target_controller_id)
            .fetch_one(&mut *transaction)
            .await?;
            if !valid_review {
                return Err(AppError::BadRequest(
                    "controller review task is not an authoritative open assignment",
                ));
            }
            ("controller_review", "pending_review")
        }
        CrossOwnerAssignmentRoute::Rejected => {
            if request.review_task_resource_node_id.is_some() {
                return Err(AppError::BadRequest(
                    "rejected route cannot carry a review task",
                ));
            }
            ("rejected", "rejected")
        }
    };
    sqlx::query(
        "INSERT INTO agent_task_intents (
             id, project_id, task_resource_node_id, scope_resource_node_id,
             required_actions, derived_by_identity_id, recorded_at
         ) VALUES ($1, $2, $3, $3, $4::jsonb, $5, $6)",
    )
    .bind(intent_id)
    .bind(project_id)
    .bind(task_resource_id)
    .bind(canonical_json(&intent.required_actions)?)
    .bind(actor.identity_id)
    .bind(recorded_at)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO agent_cross_owner_assignments (
            id, project_id, task_resource_node_id, requester_identity_id,
            target_agent_id, target_controller_identity_id, task_intent_id,
            route, review_task_resource_node_id,
            responsibility_id, responsibility_revision, state, requested_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9,
            $10, $11, $12, $13
        )
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(task_resource_id)
    .bind(actor.identity_id)
    .bind(target_agent_id)
    .bind(target_controller_id)
    .bind(intent_id)
    .bind(route_name)
    .bind(request.review_task_resource_node_id.map(Uuid::from))
    .bind(
        active_responsibility
            .as_ref()
            .map(|item| Uuid::from(item.id)),
    )
    .bind(
        active_responsibility
            .as_ref()
            .map(|item| to_i64(item.revision))
            .transpose()?,
    )
    .bind(state_name)
    .bind(recorded_at)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(CrossOwnerAssignmentResponse {
        id: request.id,
        route: route_name,
        state: state_name,
    }))
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossOwnerControllerDecision {
    Approved,
    Rejected,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideCrossOwnerAssignmentRequest {
    decision: CrossOwnerControllerDecision,
}

pub async fn decide_cross_owner_task_assignment(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, assignment_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<DecideCrossOwnerAssignmentRequest>,
) -> Result<Json<CrossOwnerAssignmentResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query(
        "SELECT target_agent_id, target_controller_identity_id
         FROM agent_cross_owner_assignments
         WHERE project_id = $1 AND id = $2 AND route = 'controller_review'
           AND state = 'pending_review' AND decision IS NULL
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let controller_id: Uuid = row.try_get("target_controller_identity_id")?;
    let target_agent_id: Uuid = row.try_get("target_agent_id")?;
    let current_controller = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM governed_agents
         WHERE project_id = $1 AND id = $2 AND controller_identity_id = $3
           AND state = 'active')",
    )
    .bind(project_id)
    .bind(target_agent_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if controller_id != actor.identity_id || !current_controller {
        return Err(AppError::Forbidden);
    }
    let (decision_name, state_name) = match request.decision {
        CrossOwnerControllerDecision::Approved => ("approved", "approved_pending_mandate"),
        CrossOwnerControllerDecision::Rejected => ("rejected", "rejected"),
    };
    let updated = sqlx::query(
        "UPDATE agent_cross_owner_assignments
         SET decision = $3, state = $4, decided_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2 AND state = 'pending_review'
           AND decision IS NULL",
    )
    .bind(project_id)
    .bind(assignment_id)
    .bind(decision_name)
    .bind(state_name)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        controller_id,
        Some(target_agent_id),
        "cross_owner_decided",
        json!({"assignment_id": assignment_id, "decision": decision_name}),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(CrossOwnerAssignmentResponse {
        id: assignment_id,
        route: "controller_review",
        state: state_name,
    }))
}

pub async fn finalize_cross_owner_task_assignment(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, assignment_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<CrossOwnerAssignmentResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query(
        "SELECT task_resource_node_id, requester_identity_id,
                target_agent_id, target_controller_identity_id
         FROM agent_cross_owner_assignments
         WHERE project_id = $1 AND id = $2 AND route = 'controller_review'
           AND decision = 'approved' AND state = 'approved_pending_mandate'
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let task_resource_id: Uuid = row.try_get("task_resource_node_id")?;
    let requester_id: Uuid = row.try_get("requester_identity_id")?;
    let target_agent_id: Uuid = row.try_get("target_agent_id")?;
    let target_controller_id: Uuid = row.try_get("target_controller_identity_id")?;
    if requester_id != actor.identity_id
        || !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            task_resource_id,
            ResourceOperation::Manage,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    let active = sqlx::query(
        r#"
        SELECT task_resource_node_id, task_intent_id,
               intent_required_actions::text AS intent_required_actions,
               intent_recorded_at, target_agent_id,
               target_controller_identity_id, target_principal_identity_id,
               target_availability, local_contract::text AS local_contract,
               obligation_id, work_spec_ordinal, provenance_recorded_at,
               exact_prompt
        FROM sprout_private.cross_owner_active_mandate_snapshot($1, $2)
        "#,
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    if !active.try_get::<bool, _>("exact_prompt")? {
        return Err(AppError::Conflict);
    }
    if active.try_get::<Uuid, _>("task_resource_node_id")? != task_resource_id
        || active.try_get::<Uuid, _>("target_agent_id")? != target_agent_id
        || active.try_get::<Uuid, _>("target_controller_identity_id")? != target_controller_id
    {
        return Err(AppError::Conflict);
    }
    let target_principal_id: Uuid = active.try_get("target_principal_identity_id")?;
    let local: LocalGoalContract =
        serde_json::from_str(active.try_get("local_contract")?).map_err(|_| AppError::Internal)?;
    let required_actions: Vec<AgentActionClass> =
        serde_json::from_str(active.try_get("intent_required_actions")?)
            .map_err(|_| AppError::Internal)?;
    let intent = PersistedTaskIntent {
        task: task_resource_id.into(),
        scope: task_resource_id.into(),
        required_actions,
        created_by: requester_id.into(),
        recorded_at: active.try_get("intent_recorded_at")?,
    };
    let provenance = TaskObligationProvenance {
        task: task_resource_id.into(),
        agent: target_principal_id.into(),
        local_revision: local.revision,
        obligation: active.try_get("obligation_id")?,
        work_spec_id: u64::try_from(active.try_get::<i64, _>("work_spec_ordinal")?)
            .map_err(|_| AppError::Internal)?,
        recorded_at: active.try_get("provenance_recorded_at")?,
    };
    let target = GovernedAgent {
        id: target_agent_id.into(),
        principal_id: target_principal_id.into(),
        controller_id: target_controller_id.into(),
        project_id: project_id.into(),
        availability: match active.try_get::<String, _>("target_availability")?.as_str() {
            "controller_private" => AgentAvailabilityMode::ControllerPrivate,
            "project_delegable" => AgentAvailabilityMode::ProjectDelegable,
            _ => return Err(AppError::Internal),
        },
    };
    let projected = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT
            EXISTS (
                SELECT 1 FROM sprout_private.semantic_task_intent_list($1) intent
                WHERE intent.id = $2
                  AND intent.task_resource_node_id = $3
                  AND intent.scope_resource_node_id = $3
                  AND intent.required_actions = $4::jsonb
                  AND intent.derived_by_identity_id = $5
                  AND intent.recorded_at = $6
            )
            AND EXISTS (
                SELECT 1 FROM sprout_private.semantic_task_provenance_list($1) provenance
                WHERE provenance.task_intent_id = $2
                  AND provenance.task_resource_node_id = $3
                  AND provenance.agent_identity_id = $7
                  AND provenance.local_goal_id = $8
                  AND provenance.local_goal_revision = $9
                  AND provenance.obligation_id = $10
                  AND provenance.work_spec_ordinal = $11
                  AND provenance.recorded_at = $12
            )
        "#,
    )
    .bind(project_id)
    .bind(active.try_get::<Uuid, _>("task_intent_id")?)
    .bind(task_resource_id)
    .bind(serde_json::to_value(&intent.required_actions).map_err(|_| AppError::Internal)?)
    .bind(actor.identity_id)
    .bind(intent.recorded_at)
    .bind(target_principal_id)
    .bind(Uuid::from(local.id))
    .bind(to_i64(local.revision)?)
    .bind(provenance.obligation)
    .bind(to_i64(provenance.work_spec_id)?)
    .bind(provenance.recorded_at)
    .fetch_one(&mut *transaction)
    .await?;
    if !projected {
        return Err(AppError::Conflict);
    }
    let facts = authoritative_local_condition_facts(&mut transaction, project_id, &local).await?;
    let provenance_obligation = local
        .contract
        .obligations
        .iter()
        .find(|obligation| obligation.id == provenance.obligation)
        .ok_or(AppError::Conflict)?;
    if !cross_owner_condition_is_authoritative(&provenance_obligation.activation)
        || !cross_owner_condition_is_authoritative(&provenance_obligation.required_for_completion)
        || route_cross_owner_assignment(
            task_resource_id.into(),
            &target,
            CurrentLocalObligationContext {
                active_local_goal: Some(&local),
                provenance: Some(&provenance),
                condition_facts: &facts,
            },
            Some(&intent),
            None,
            |_, _| false,
        ) != CrossOwnerAssignmentRoute::AutomaticFromActiveObligation
    {
        return Err(AppError::Conflict);
    }
    let updated = sqlx::query(
        "UPDATE agent_cross_owner_assignments SET state = 'ready'
         WHERE project_id = $1 AND id = $2
           AND decision = 'approved' AND state = 'approved_pending_mandate'",
    )
    .bind(project_id)
    .bind(assignment_id)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    transaction.commit().await?;
    Ok(Json(CrossOwnerAssignmentResponse {
        id: assignment_id,
        route: "controller_review",
        state: "ready",
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializeCrossOwnerAssignmentRequest {
    effect_id: Uuid,
    task_assignment_id: Uuid,
    idempotency_key: Uuid,
    encrypted_assignment_payload_b64: String,
}

#[derive(Serialize)]
pub struct MaterializeCrossOwnerAssignmentResponse {
    cross_owner_assignment_id: Uuid,
    effect_id: Uuid,
    task_assignment_id: Uuid,
    task_id: Uuid,
    assignee_identity_id: Uuid,
    replayed: bool,
}

pub async fn materialize_cross_owner_task_assignment(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, assignment_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<MaterializeCrossOwnerAssignmentRequest>,
) -> Result<Json<MaterializeCrossOwnerAssignmentResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let encrypted_payload = super::assignments::decode(&request.encrypted_assignment_payload_b64)?;
    if encrypted_payload.is_empty() {
        return Err(AppError::BadRequest(
            "encrypted assignment payload is empty",
        ));
    }
    let request_hash: [u8; 32] = Sha256::digest(
        canonical_json(&json!({
            "assignment_id": assignment_id,
            "effect_id": request.effect_id,
            "task_assignment_id": request.task_assignment_id,
            "idempotency_key": request.idempotency_key,
            "encrypted_assignment_payload_b64": request.encrypted_assignment_payload_b64,
        }))?
        .as_bytes(),
    )
    .into();
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 37))")
        .bind(assignment_id)
        .execute(&mut *transaction)
        .await?;
    if let Some(existing) = sqlx::query(
        "SELECT id, task_assignment_id, task_id, assignee_identity_id, request_hash
         FROM agent_cross_owner_assignment_effects
         WHERE project_id = $1 AND cross_owner_assignment_id = $2",
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        let stored_hash: Vec<u8> = existing.try_get("request_hash")?;
        if stored_hash != request_hash {
            return Err(AppError::Conflict);
        }
        transaction.commit().await?;
        return Ok(Json(MaterializeCrossOwnerAssignmentResponse {
            cross_owner_assignment_id: assignment_id,
            effect_id: existing.try_get("id")?,
            task_assignment_id: existing.try_get("task_assignment_id")?,
            task_id: existing.try_get("task_id")?,
            assignee_identity_id: existing.try_get("assignee_identity_id")?,
            replayed: true,
        }));
    }
    let routed_task_resource_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT task_resource_node_id
         FROM agent_cross_owner_assignments
         WHERE project_id = $1 AND id = $2
           AND requester_identity_id = $3 AND state = 'ready'
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(assignment_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    if !resource_access_in_transaction(
        &mut transaction,
        project_id,
        actor.identity_id,
        routed_task_resource_id,
        ResourceOperation::Manage,
    )
    .await?
    {
        return Err(AppError::Forbidden);
    }
    let row = sqlx::query(
        r#"
        SELECT task_resource_node_id, task_id,
               task_intent_id, intent_required_actions::text AS intent_required_actions,
               intent_recorded_at, target_agent_id, target_controller_identity_id,
               target_principal_identity_id, target_availability,
               local_contract::text AS local_contract, obligation_id,
               work_spec_ordinal, provenance_recorded_at, exact_prompt
        FROM sprout_private.cross_owner_materialization_snapshot($1, $2)
        "#,
    )
    .bind(project_id)
    .bind(assignment_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    if !row.try_get::<bool, _>("exact_prompt")? {
        return Err(AppError::Conflict);
    }
    let task_resource_id: Uuid = row.try_get("task_resource_node_id")?;
    let task_id: Uuid = row.try_get("task_id")?;
    let task_intent_id: Uuid = row.try_get("task_intent_id")?;
    let target_agent_id: Uuid = row.try_get("target_agent_id")?;
    let target_controller_id: Uuid = row.try_get("target_controller_identity_id")?;
    let target_principal_id: Uuid = row.try_get("target_principal_identity_id")?;
    if task_resource_id != routed_task_resource_id {
        return Err(AppError::Conflict);
    }
    let local: LocalGoalContract =
        serde_json::from_str(row.try_get("local_contract")?).map_err(|_| AppError::Internal)?;
    let intent = PersistedTaskIntent {
        task: task_resource_id.into(),
        scope: task_resource_id.into(),
        required_actions: serde_json::from_str(row.try_get("intent_required_actions")?)
            .map_err(|_| AppError::Internal)?,
        created_by: actor.identity_id.into(),
        recorded_at: row.try_get("intent_recorded_at")?,
    };
    let provenance = TaskObligationProvenance {
        task: task_resource_id.into(),
        agent: target_principal_id.into(),
        local_revision: local.revision,
        obligation: row.try_get("obligation_id")?,
        work_spec_id: u64::try_from(row.try_get::<i64, _>("work_spec_ordinal")?)
            .map_err(|_| AppError::Internal)?,
        recorded_at: row.try_get("provenance_recorded_at")?,
    };
    let target = GovernedAgent {
        id: target_agent_id.into(),
        principal_id: target_principal_id.into(),
        controller_id: target_controller_id.into(),
        project_id: project_id.into(),
        availability: match row.try_get::<String, _>("target_availability")?.as_str() {
            "controller_private" => AgentAvailabilityMode::ControllerPrivate,
            "project_delegable" => AgentAvailabilityMode::ProjectDelegable,
            _ => return Err(AppError::Internal),
        },
    };
    let projected = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT
            EXISTS (
                SELECT 1 FROM sprout_private.semantic_task_intent_list($1) intent
                WHERE intent.id = $2
                  AND intent.task_resource_node_id = $3
                  AND intent.scope_resource_node_id = $3
                  AND intent.required_actions = $4::jsonb
                  AND intent.derived_by_identity_id = $5
                  AND intent.recorded_at = $6
            )
            AND EXISTS (
                SELECT 1 FROM sprout_private.semantic_task_provenance_list($1) provenance
                WHERE provenance.task_intent_id = $2
                  AND provenance.task_resource_node_id = $3
                  AND provenance.target_agent_id = $7
                  AND provenance.agent_identity_id = $8
                  AND provenance.local_goal_id = $9
                  AND provenance.local_goal_revision = $10
                  AND provenance.obligation_id = $11
                  AND provenance.work_spec_ordinal = $12
                  AND provenance.recorded_at = $13
            )
        "#,
    )
    .bind(project_id)
    .bind(task_intent_id)
    .bind(task_resource_id)
    .bind(serde_json::to_value(&intent.required_actions).map_err(|_| AppError::Internal)?)
    .bind(actor.identity_id)
    .bind(intent.recorded_at)
    .bind(target_agent_id)
    .bind(target_principal_id)
    .bind(Uuid::from(local.id))
    .bind(to_i64(local.revision)?)
    .bind(provenance.obligation)
    .bind(to_i64(provenance.work_spec_id)?)
    .bind(provenance.recorded_at)
    .fetch_one(&mut *transaction)
    .await?;
    if !projected {
        return Err(AppError::Conflict);
    }
    let facts = authoritative_local_condition_facts(&mut transaction, project_id, &local).await?;
    if route_cross_owner_assignment(
        task_resource_id.into(),
        &target,
        CurrentLocalObligationContext {
            active_local_goal: Some(&local),
            provenance: Some(&provenance),
            condition_facts: &facts,
        },
        Some(&intent),
        None,
        |_, _| false,
    ) != CrossOwnerAssignmentRoute::AutomaticFromActiveObligation
    {
        return Err(AppError::Conflict);
    }
    sqlx::query(
        r#"
        INSERT INTO task_assignments (
            id, project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, encrypted_payload, permission_root_grant_id,
            permission_managed_by_assignment
        ) VALUES ($1, $2, $3, $4, $5, $6, $1, false)
        "#,
    )
    .bind(request.task_assignment_id)
    .bind(project_id)
    .bind(task_id)
    .bind(target_principal_id)
    .bind(actor.identity_id)
    .bind(&encrypted_payload)
    .execute(&mut *transaction)
    .await?;
    let applied_at = Utc::now();
    let provenance_hash = Sha256::digest(
        canonical_json(&json!({
            "project_id": project_id,
            "cross_owner_assignment_id": assignment_id,
            "task_intent_id": task_intent_id,
            "task_resource_node_id": task_resource_id,
            "task_id": task_id,
            "task_assignment_id": request.task_assignment_id,
            "target_agent_id": target_agent_id,
            "assignee_identity_id": target_principal_id,
            "materialized_by_identity_id": actor.identity_id,
            "applied_at": applied_at,
        }))?
        .as_bytes(),
    );
    sqlx::query(
        r#"
        INSERT INTO agent_cross_owner_assignment_effects (
            id, project_id, cross_owner_assignment_id, task_intent_id,
            task_resource_node_id, task_id, task_assignment_id,
            target_agent_id, assignee_identity_id,
            materialized_by_identity_id, materialized_by_device_id,
            idempotency_key, request_hash, provenance_hash, applied_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9,
            $10, $11, $12, $13, $14, $15
        )
        "#,
    )
    .bind(request.effect_id)
    .bind(project_id)
    .bind(assignment_id)
    .bind(task_intent_id)
    .bind(task_resource_id)
    .bind(task_id)
    .bind(request.task_assignment_id)
    .bind(target_agent_id)
    .bind(target_principal_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(request.idempotency_key)
    .bind(request_hash.as_slice())
    .bind(provenance_hash.as_slice())
    .bind(applied_at)
    .execute(&mut *transaction)
    .await?;
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        target_controller_id,
        Some(target_agent_id),
        "cross_owner_materialized",
        json!({
            "cross_owner_assignment_id": assignment_id,
            "effect_id": request.effect_id,
            "task_intent_id": task_intent_id,
            "task_assignment_id": request.task_assignment_id,
            "task_resource_node_id": task_resource_id,
            "provenance_hash": hex::encode(provenance_hash),
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(MaterializeCrossOwnerAssignmentResponse {
        cross_owner_assignment_id: assignment_id,
        effect_id: request.effect_id,
        task_assignment_id: request.task_assignment_id,
        task_id,
        assignee_identity_id: target_principal_id,
        replayed: false,
    }))
}

#[derive(Deserialize)]
pub struct QueueInvocationRequest {
    id: InvocationId,
    local_goal_id: Option<Uuid>,
    local_goal_revision: Option<u64>,
    language_task: StructuredLanguageTaskEnvelope,
    authority_envelope: AuthorityEnvelope,
    sources: Vec<InformationSource>,
    encrypted_input: EncryptedPayload,
}

#[derive(Serialize)]
pub struct QueueInvocationResponse {
    id: InvocationId,
    status: &'static str,
    request_hash_hex: String,
}

pub async fn queue_invocation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<QueueInvocationRequest>,
) -> Result<Json<QueueInvocationResponse>, AppError> {
    if actor.is_agent || request.local_goal_id.is_some() != request.local_goal_revision.is_some() {
        return Err(AppError::BadRequest("invalid invocation goal reference"));
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    if agent.controller_id != actor.identity_id.into() {
        return Err(AppError::Forbidden);
    }
    request
        .language_task
        .validate()
        .map_err(agent_validation_error)?;
    request
        .authority_envelope
        .validate_unique()
        .map_err(agent_validation_error)?;
    if !request.authority_envelope.tool_authority.is_empty()
        || !request.language_task.allowed_tools.is_empty()
    {
        return Err(AppError::BadRequest(
            "tools require a registered deterministic security adapter",
        ));
    }
    if !request.sources.iter().any(|source| {
        matches!(
            source,
            InformationSource::ResourceBody { resource_id }
                if *resource_id == agent_profile_resource(&agent)
        )
    }) {
        return Err(AppError::BadRequest(
            "invocation context must include the current agent system prompt resource",
        ));
    }

    let runner = active_runner(&state, actor, project_id, &agent).await?;
    for source in &request.sources {
        let resource_id = supported_source_resource(&state, actor, project_id, source).await?;
        ensure_runner_can_read(
            &state,
            actor,
            project_id,
            agent.principal_id,
            runner.device_id,
            runner.key_version,
            resource_id,
        )
        .await?;
    }
    for authority in &request.authority_envelope.resource_authority {
        if !resource_access_for_identity(
            &state,
            actor,
            project_id,
            Uuid::from(agent.principal_id),
            Uuid::from(authority.resource_id),
            authority.operation,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    }
    let context = ModelInvocationContext {
        invocation_id: request.id,
        principal: agent.principal_id,
        sources: request.sources.clone(),
        reconstructed_at: Utc::now(),
    };
    validate_state_grounded_invocation(
        &context,
        &ModelExposureProjection {
            exposed_sources: request.sources.clone(),
            hidden_persistent_model_memory_available: false,
        },
        |_, _| true,
    )
    .map_err(agent_validation_error)?;

    let request_projection = json!({
        "id": request.id,
        "local_goal_id": request.local_goal_id,
        "local_goal_revision": request.local_goal_revision,
        "language_task": request.language_task,
        "authority_envelope": request.authority_envelope,
        "sources": request.sources,
        "encrypted_input": request.encrypted_input,
    });
    let request_json = canonical_json(&request_projection)?;
    let request_hash: [u8; 32] = Sha256::digest(request_json.as_bytes()).into();
    let language_task_json = canonical_json(&request.language_task)?;
    let authority_json = canonical_json(&request.authority_envelope)?;
    let encrypted_input = serialize_ciphertext(&request.encrypted_input)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    if let (Some(local_goal_id), Some(revision)) =
        (request.local_goal_id, request.local_goal_revision)
    {
        let matches_agent = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM agent_local_goal_contracts
                WHERE project_id = $1 AND id = $2 AND revision = $3
                  AND agent_id = $4 AND state = 'active'
            )
            "#,
        )
        .bind(project_id)
        .bind(local_goal_id)
        .bind(to_i64(revision)?)
        .bind(agent_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !matches_agent {
            return Err(AppError::Conflict);
        }
    }
    sqlx::query(
        r#"
        INSERT INTO agent_invocations (
            id, project_id, agent_id, agent_identity_id,
            local_goal_id, local_goal_revision, language_task,
            authority_envelope, encrypted_input, request_hash,
            max_attempts, created_by_identity_id
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7::jsonb,
            $8::jsonb, $9, $10, $11, $12
        )
        "#,
    )
    .bind(Uuid::from(request.id))
    .bind(project_id)
    .bind(agent_id)
    .bind(Uuid::from(agent.principal_id))
    .bind(request.local_goal_id)
    .bind(request.local_goal_revision.map(to_i64).transpose()?)
    .bind(language_task_json)
    .bind(authority_json)
    .bind(encrypted_input)
    .bind(request_hash.as_slice())
    .bind(i32::from(request.language_task.max_attempts))
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    for (ordinal, source) in request.sources.iter().enumerate() {
        let (kind, resource_id, source_id) = source_columns(source);
        sqlx::query(
            r#"
            INSERT INTO agent_invocation_sources (
                project_id, invocation_id, ordinal, source_kind,
                resource_node_id, source_id, source_descriptor
            ) VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(request.id))
        .bind(i32::try_from(ordinal).map_err(|_| AppError::PayloadTooLarge)?)
        .bind(kind)
        .bind(resource_id)
        .bind(source_id)
        .bind(canonical_json(source)?)
        .execute(&mut *transaction)
        .await?;
    }
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        Some(request.id),
        "invocation_queued",
        json!({
            "request_hash": hex::encode(request_hash),
            "source_count": request.sources.len(),
            "max_attempts": request.language_task.max_attempts,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(QueueInvocationResponse {
        id: request.id,
        status: "pending",
        request_hash_hex: hex::encode(request_hash),
    }))
}

#[derive(Serialize)]
pub struct ClaimedInvocationResponse {
    id: InvocationId,
    lease_id: Uuid,
    lease_expires_at: DateTime<Utc>,
    attempt: i32,
    language_task: StructuredLanguageTaskEnvelope,
    authority_envelope: AuthorityEnvelope,
    sources: Vec<InformationSource>,
    encrypted_input: EncryptedPayload,
}

pub async fn claim_invocation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Option<ClaimedInvocationResponse>>, AppError> {
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let runner = authenticated_runner(&state, actor, project_id, agent_id).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let candidate = sqlx::query(
        r#"
        SELECT id, language_task::text AS language_task,
               authority_envelope::text AS authority_envelope,
               encrypted_input, attempt
        FROM agent_invocations
        WHERE project_id = $1 AND agent_id = $2
          AND attempt < max_attempts
          AND (status = 'pending'
               OR (status = 'leased' AND lease_expires_at <= clock_timestamp()))
        ORDER BY created_at, id
        LIMIT 1
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(candidate) = candidate else {
        transaction.commit().await?;
        return Ok(Json(None));
    };
    let invocation_id: Uuid = candidate.try_get("id")?;
    let sources = load_sources(&mut transaction, project_id, invocation_id).await?;
    for source in &sources {
        let resource_id = source_resource(source).ok_or(AppError::Forbidden)?;
        ensure_runner_can_read_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            actor.device_id,
            runner.key_version,
            resource_id,
        )
        .await?;
    }
    let lease_id = Uuid::new_v4();
    let lease_expires_at =
        Utc::now() + chrono::Duration::from_std(RUNNER_LEASE).map_err(|_| AppError::Internal)?;
    let attempt: i32 = candidate.try_get::<i32, _>("attempt")? + 1;
    sqlx::query(
        r#"
        UPDATE agent_invocations
        SET status = 'leased', attempt = $3, runner_id = $4,
            lease_id = $5, leased_at = clock_timestamp(),
            lease_expires_at = $6, completed_at = NULL,
            encrypted_output = NULL, output_hash = NULL, failure_code = NULL
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(project_id)
    .bind(invocation_id)
    .bind(attempt)
    .bind(runner.id)
    .bind(lease_id)
    .bind(lease_expires_at)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        Some(InvocationId::from(invocation_id)),
        "invocation_leased",
        json!({
            "runner_id": runner.id,
            "lease_id": lease_id,
            "attempt": attempt,
            "lease_expires_at": lease_expires_at,
        }),
    )
    .await?;
    let language_task = serde_json::from_str(candidate.try_get("language_task")?)
        .map_err(|_| AppError::Internal)?;
    let authority_envelope = serde_json::from_str(candidate.try_get("authority_envelope")?)
        .map_err(|_| AppError::Internal)?;
    let encrypted_input = deserialize_ciphertext(candidate.try_get("encrypted_input")?)?;
    transaction.commit().await?;
    Ok(Json(Some(ClaimedInvocationResponse {
        id: InvocationId::from(invocation_id),
        lease_id,
        lease_expires_at,
        attempt,
        language_task,
        authority_envelope,
        sources,
        encrypted_input,
    })))
}

#[derive(Deserialize, Serialize)]
pub struct EffectProposalRequest {
    id: Uuid,
    effect: ResourceEffect,
    materialization: Option<EffectMaterializationRequest>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectMaterializationRequest {
    ReplaceInfoDocument {
        document_id: Uuid,
        expected_payload_version: u64,
        key_epoch: u32,
        idempotency_key: Uuid,
        payload: EncryptedPayload,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedEffectMaterialization {
    ReplaceInfoDocument {
        document_id: Uuid,
        expected_payload_version: u64,
        key_epoch: u32,
        idempotency_key: Uuid,
    },
}

#[derive(Deserialize, Serialize)]
struct PersistedEffectDescriptor {
    effect: ResourceEffect,
    materialization: PersistedEffectMaterialization,
}

#[derive(Deserialize)]
pub struct SubmitInvocationRequest {
    lease_id: Uuid,
    structured_output: StructuredLanguageOutput,
    encrypted_output: EncryptedPayload,
    effects: Vec<EffectProposalRequest>,
}

#[derive(Serialize)]
pub struct SubmitInvocationResponse {
    id: InvocationId,
    status: &'static str,
    accepted_effect_ids: Vec<Uuid>,
    output_hash_hex: String,
}

#[derive(Deserialize)]
pub struct FailInvocationRequest {
    lease_id: Uuid,
    failure_code: RunnerFailureCode,
    retryable: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerFailureCode {
    ProviderUnavailable,
    ProviderTimeout,
    InvalidStructuredOutput,
    ContextDecryptionFailed,
    LocalExecutionFailed,
}

impl RunnerFailureCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderTimeout => "provider_timeout",
            Self::InvalidStructuredOutput => "invalid_structured_output",
            Self::ContextDecryptionFailed => "context_decryption_failed",
            Self::LocalExecutionFailed => "local_execution_failed",
        }
    }
}

#[derive(Serialize)]
pub struct FailedInvocationResponse {
    id: InvocationId,
    status: &'static str,
    attempts_exhausted: bool,
}

pub async fn submit_invocation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, invocation_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<SubmitInvocationRequest>,
) -> Result<Json<SubmitInvocationResponse>, AppError> {
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let runner = authenticated_runner(&state, actor, project_id, agent_id).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT language_task::text AS language_task,
               authority_envelope::text AS authority_envelope
        FROM agent_invocations
        WHERE project_id = $1 AND id = $2 AND agent_id = $3
          AND runner_id = $4 AND lease_id = $5
          AND status = 'leased' AND lease_expires_at > clock_timestamp()
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(invocation_id)
    .bind(agent_id)
    .bind(runner.id)
    .bind(request.lease_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let language_task: StructuredLanguageTaskEnvelope =
        serde_json::from_str(row.try_get("language_task")?).map_err(|_| AppError::Internal)?;
    language_task
        .validate_grounded_output(&request.structured_output)
        .map_err(agent_validation_error)?;
    let authority: AuthorityEnvelope =
        serde_json::from_str(row.try_get("authority_envelope")?).map_err(|_| AppError::Internal)?;
    ensure_unique_effect_ids(&request.effects)?;
    let output_resources: HashSet<_> = request
        .structured_output
        .items
        .iter()
        .filter_map(|item| item.resource_id)
        .collect();
    let sources = load_sources(&mut transaction, project_id, invocation_id).await?;
    let mut source_audiences = Vec::with_capacity(sources.len());
    for source in &sources {
        let resource_id = source_resource(source).ok_or(AppError::Forbidden)?;
        ensure_runner_can_read_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            actor.device_id,
            runner.key_version,
            resource_id,
        )
        .await?;
        source_audiences
            .push(resource_reader_audience(&mut transaction, project_id, resource_id).await?);
    }
    for proposal in &request.effects {
        if !output_resources.contains(&proposal.effect.resource_id)
            || !authority.resource_authority.iter().any(|entry| {
                entry.resource_id == proposal.effect.resource_id
                    && entry.operation == proposal.effect.operation
            })
            || !resource_access_in_transaction(
                &mut transaction,
                project_id,
                actor.identity_id,
                Uuid::from(proposal.effect.resource_id),
                proposal.effect.operation,
            )
            .await?
        {
            return Err(AppError::Forbidden);
        }
        let sink_audience = resource_reader_audience(
            &mut transaction,
            project_id,
            Uuid::from(proposal.effect.resource_id),
        )
        .await?;
        validate_information_flow(&source_audiences, &sink_audience)
            .map_err(agent_validation_error)?;
        validate_effect_materialization(
            &mut transaction,
            project_id,
            proposal,
            Uuid::from(proposal.effect.resource_id),
        )
        .await?;
    }

    let encrypted_output = serialize_ciphertext(&request.encrypted_output)?;
    let output_projection = json!({
        "structured_output": request.structured_output,
        "encrypted_output": request.encrypted_output,
        "effects": request.effects.iter().map(|effect| json!({
            "id": effect.id,
            "effect": effect.effect,
            "materialization": effect.materialization,
        })).collect::<Vec<_>>(),
    });
    let output_json = canonical_json(&output_projection)?;
    let output_hash: [u8; 32] = Sha256::digest(output_json.as_bytes()).into();
    for (ordinal, proposal) in request.effects.iter().enumerate() {
        let (descriptor, materialization) = persisted_materialization(proposal)?;
        let proposal_json = canonical_json(&descriptor)?;
        let proposal_hash: [u8; 32] = Sha256::digest(
            canonical_json(&json!({
                "invocation_id": invocation_id,
                "ordinal": ordinal,
                "proposal": proposal,
            }))?
            .as_bytes(),
        )
        .into();
        sqlx::query(
            r#"
            INSERT INTO agent_effect_proposals (
                id, project_id, invocation_id, agent_id, ordinal,
                effect, encrypted_materialization, proposal_hash
            ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8)
            "#,
        )
        .bind(proposal.id)
        .bind(project_id)
        .bind(invocation_id)
        .bind(agent_id)
        .bind(i32::try_from(ordinal).map_err(|_| AppError::PayloadTooLarge)?)
        .bind(proposal_json)
        .bind(materialization)
        .bind(proposal_hash.as_slice())
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        r#"
        UPDATE agent_invocations
        SET status = 'succeeded', completed_at = clock_timestamp(),
            encrypted_output = $3, output_hash = $4, failure_code = NULL
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(project_id)
    .bind(invocation_id)
    .bind(encrypted_output)
    .bind(output_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        Some(InvocationId::from(invocation_id)),
        "invocation_succeeded",
        json!({
            "output_hash": hex::encode(output_hash),
            "accepted_effect_ids": request.effects.iter().map(|effect| effect.id).collect::<Vec<_>>(),
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(SubmitInvocationResponse {
        id: InvocationId::from(invocation_id),
        status: "succeeded",
        accepted_effect_ids: request.effects.iter().map(|effect| effect.id).collect(),
        output_hash_hex: hex::encode(output_hash),
    }))
}

pub async fn fail_invocation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, invocation_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<FailInvocationRequest>,
) -> Result<Json<FailedInvocationResponse>, AppError> {
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let runner = authenticated_runner(&state, actor, project_id, agent_id).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let attempts = sqlx::query_as::<_, (i32, i32)>(
        r#"
        SELECT attempt, max_attempts
        FROM agent_invocations
        WHERE project_id = $1 AND id = $2 AND agent_id = $3
          AND runner_id = $4 AND lease_id = $5
          AND status = 'leased' AND lease_expires_at > clock_timestamp()
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(invocation_id)
    .bind(agent_id)
    .bind(runner.id)
    .bind(request.lease_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let exhausted = !request.retryable || attempts.0 >= attempts.1;
    if exhausted {
        sqlx::query(
            r#"
            UPDATE agent_invocations
            SET status = 'failed', completed_at = clock_timestamp(),
                encrypted_output = NULL, output_hash = NULL, failure_code = $3
            WHERE project_id = $1 AND id = $2
            "#,
        )
        .bind(project_id)
        .bind(invocation_id)
        .bind(request.failure_code.as_str())
        .execute(&mut *transaction)
        .await?;
    } else {
        sqlx::query(
            r#"
            UPDATE agent_invocations
            SET status = 'pending', runner_id = NULL, lease_id = NULL,
                leased_at = NULL, lease_expires_at = NULL, completed_at = NULL,
                encrypted_output = NULL, output_hash = NULL, failure_code = NULL
            WHERE project_id = $1 AND id = $2
            "#,
        )
        .bind(project_id)
        .bind(invocation_id)
        .execute(&mut *transaction)
        .await?;
    }
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        Some(InvocationId::from(invocation_id)),
        "invocation_failed",
        json!({
            "failure_code": request.failure_code.as_str(),
            "retryable": request.retryable,
            "attempt": attempts.0,
            "max_attempts": attempts.1,
            "terminal": exhausted,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(FailedInvocationResponse {
        id: InvocationId::from(invocation_id),
        status: if exhausted { "failed" } else { "pending" },
        attempts_exhausted: exhausted,
    }))
}

#[derive(Serialize)]
pub struct AppliedInfoEffectResponse {
    effect_id: Uuid,
    document_id: Uuid,
    payload_version: u64,
    status: &'static str,
}

pub async fn apply_info_effect(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, effect_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<AppliedInfoEffectResponse>, AppError> {
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let runner = authenticated_runner(&state, actor, project_id, agent_id).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT proposal.invocation_id, proposal.effect::text AS effect,
               proposal.encrypted_materialization
        FROM agent_effect_proposals proposal
        JOIN agent_invocations invocation
          ON invocation.project_id = proposal.project_id
         AND invocation.id = proposal.invocation_id
        WHERE proposal.project_id = $1 AND proposal.id = $2
          AND proposal.agent_id = $3 AND proposal.status = 'accepted'
          AND invocation.status = 'succeeded' AND invocation.runner_id = $4
        FOR UPDATE OF proposal
        "#,
    )
    .bind(project_id)
    .bind(effect_id)
    .bind(agent_id)
    .bind(runner.id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let invocation_id: Uuid = row.try_get("invocation_id")?;
    let descriptor: PersistedEffectDescriptor =
        serde_json::from_str(row.try_get("effect")?).map_err(|_| AppError::Internal)?;
    if descriptor.effect.operation != ResourceOperation::EditInfo {
        return Err(AppError::BadRequest(
            "effect has no info-document materializer",
        ));
    }
    let encrypted_materialization: Vec<u8> = row.try_get("encrypted_materialization")?;
    let payload = deserialize_ciphertext(&encrypted_materialization)?;
    let PersistedEffectMaterialization::ReplaceInfoDocument {
        document_id,
        expected_payload_version,
        key_epoch,
        idempotency_key,
    } = descriptor.materialization;
    let resource_id = Uuid::from(descriptor.effect.resource_id);
    let current_resource = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM info_documents
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(document_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if current_resource != resource_id
        || !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            resource_id,
            ResourceOperation::EditInfo,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    ensure_runner_can_read_in_transaction(
        &mut transaction,
        project_id,
        actor.identity_id,
        actor.device_id,
        runner.key_version,
        resource_id,
    )
    .await?;
    let sources = load_sources(&mut transaction, project_id, invocation_id).await?;
    let mut source_audiences = Vec::with_capacity(sources.len());
    for source in &sources {
        let source_resource_id = source_resource(source).ok_or(AppError::Forbidden)?;
        ensure_runner_can_read_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            actor.device_id,
            runner.key_version,
            source_resource_id,
        )
        .await?;
        source_audiences.push(
            resource_reader_audience(&mut transaction, project_id, source_resource_id).await?,
        );
    }
    let sink_audience = resource_reader_audience(&mut transaction, project_id, resource_id).await?;
    validate_information_flow(&source_audiences, &sink_audience).map_err(agent_validation_error)?;
    let stored_payload = info_payload_bytes(&payload)?;
    let next_version = sqlx::query_scalar::<_, i64>(
        r#"
        UPDATE info_documents
        SET encrypted_payload = $3, key_epoch = $5,
            payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2
          AND payload_version = $4 AND deleted_at IS NULL
          AND EXISTS (
              SELECT 1 FROM resource_epochs epoch
              WHERE epoch.project_id = info_documents.project_id
                AND epoch.resource_node_id = info_documents.resource_node_id
                AND epoch.epoch = $5 AND epoch.retired_at IS NULL
          )
        RETURNING payload_version
        "#,
    )
    .bind(project_id)
    .bind(document_id)
    .bind(&stored_payload)
    .bind(to_i64(expected_payload_version)?)
    .bind(to_i32(key_epoch)?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    sqlx::query(
        r#"
        INSERT INTO outbox (
            project_id, aggregate_kind, aggregate_id, event_kind,
            deduplication_key, encrypted_payload
        ) VALUES ($1, 'info_document', $2, 'agent_updated', $3, $4)
        ON CONFLICT (project_id, deduplication_key) DO NOTHING
        "#,
    )
    .bind(project_id)
    .bind(document_id)
    .bind(idempotency_key)
    .bind(&stored_payload)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE agent_effect_proposals
         SET status = 'applied', applied_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2 AND status = 'accepted'",
    )
    .bind(project_id)
    .bind(effect_id)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        actor,
        project_id,
        AgentId::from(agent_id),
        Some(InvocationId::from(invocation_id)),
        "effect_applied",
        json!({
            "effect_id": effect_id,
            "operation": "edit_info",
            "resource_id": resource_id,
            "document_id": document_id,
            "payload_version": next_version,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(AppliedInfoEffectResponse {
        effect_id,
        document_id,
        payload_version: u64::try_from(next_version).map_err(|_| AppError::Internal)?,
        status: "applied",
    }))
}

async fn validate_effect_materialization(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    proposal: &EffectProposalRequest,
    resource_id: Uuid,
) -> Result<(), AppError> {
    let Some(EffectMaterializationRequest::ReplaceInfoDocument { document_id, .. }) =
        &proposal.materialization
    else {
        return Err(AppError::BadRequest(
            "effect requires a supported deterministic materializer",
        ));
    };
    if proposal.effect.operation != ResourceOperation::EditInfo {
        return Err(AppError::BadRequest(
            "info-document materialization requires edit_info authority",
        ));
    }
    let matches = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM info_documents
         WHERE project_id = $1 AND id = $2 AND resource_node_id = $3
           AND deleted_at IS NULL)",
    )
    .bind(project_id)
    .bind(document_id)
    .bind(resource_id)
    .fetch_one(&mut **transaction)
    .await?;
    if matches {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "effect materialization does not match its resource",
        ))
    }
}

fn persisted_materialization(
    proposal: &EffectProposalRequest,
) -> Result<(PersistedEffectDescriptor, Vec<u8>), AppError> {
    match proposal
        .materialization
        .as_ref()
        .ok_or(AppError::BadRequest("effect materialization is required"))?
    {
        EffectMaterializationRequest::ReplaceInfoDocument {
            document_id,
            expected_payload_version,
            key_epoch,
            idempotency_key,
            payload,
        } => Ok((
            PersistedEffectDescriptor {
                effect: proposal.effect.clone(),
                materialization: PersistedEffectMaterialization::ReplaceInfoDocument {
                    document_id: *document_id,
                    expected_payload_version: *expected_payload_version,
                    key_epoch: *key_epoch,
                    idempotency_key: *idempotency_key,
                },
            },
            serialize_ciphertext(payload)?,
        )),
    }
}

fn info_payload_bytes(payload: &EncryptedPayload) -> Result<Vec<u8>, AppError> {
    let dto = sprout_api_contract::EncryptedPayloadDto {
        version: payload.version(),
        algorithm: payload.algorithm().to_owned(),
        key_id: payload.key_id().to_owned(),
        nonce_b64: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            payload.nonce(),
        ),
        ciphertext_b64: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            payload.ciphertext(),
        ),
    };
    serde_json::to_vec(&dto).map_err(|_| AppError::Internal)
}

fn derive_proxy_action_classification(
    plan: &UserProxyActionPlan,
) -> Result<Vec<ProxyActionClassification>, AppError> {
    // Tool calls and every required resource effect are part of the normative
    // footprint. Until a product tool registry can deterministically map both
    // to Responsibility action classes, accepting them would omit authority-
    // relevant work from classification. Keep the entire tool plan closed.
    if !plan.tool_invocations.is_empty() {
        return Err(AppError::BadRequest(
            "proxy tool footprint has no registered deterministic responsibility classifier",
        ));
    }
    plan.resource_effects
        .iter()
        .map(|effect| {
            let action = match effect.operation {
                ResourceOperation::CompleteAssignedTask => AgentActionClass::MarkAssignedDone,
                ResourceOperation::PostComment => AgentActionClass::PostComment,
                // Generic write/manage operations do not identify which
                // semantic product action will be materialized. Treating a
                // caller/model label as that missing fact would make the LLM
                // an authority source, so unsupported footprints fail closed.
                ResourceOperation::ViewHeader
                | ResourceOperation::Read
                | ResourceOperation::ReadComment
                | ResourceOperation::EditInfo
                | ResourceOperation::Write
                | ResourceOperation::Manage
                | ResourceOperation::DelegateAssignedWork => {
                    return Err(AppError::BadRequest(
                        "proxy effect has no registered deterministic responsibility classifier",
                    ));
                }
            };
            Ok(ProxyActionClassification {
                resource_id: effect.resource_id,
                action,
            })
        })
        .collect()
}

async fn load_proxy_responsibility(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    user_identity_id: Uuid,
    classifications: &[ProxyActionClassification],
) -> Result<(Option<ResponsibilityContract>, bool), AppError> {
    if classifications.is_empty() {
        return Ok((None, true));
    }
    let contract_json = sqlx::query_scalar::<_, String>(
        r#"
        SELECT contract::text
        FROM agent_responsibility_contracts
        WHERE project_id = $1 AND user_identity_id = $2
          AND state = 'active'
        ORDER BY revision DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(user_identity_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(contract_json) = contract_json else {
        return Ok((None, false));
    };
    let contract: ResponsibilityContract =
        serde_json::from_str(&contract_json).map_err(|_| AppError::Internal)?;
    let mut covered = true;
    for classification in classifications {
        let mut classification_covered = false;
        for rule in &contract.rules {
            if !rule.allowed_actions.contains(&classification.action) {
                continue;
            }
            let within_scope = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM resource_closure
                 WHERE project_id = $1 AND ancestor_id = $2 AND descendant_id = $3)",
            )
            .bind(project_id)
            .bind(Uuid::from(rule.scope))
            .bind(Uuid::from(classification.resource_id))
            .fetch_one(&mut **transaction)
            .await?;
            if within_scope {
                classification_covered = true;
                break;
            }
        }
        covered &= classification_covered;
    }
    Ok((Some(contract), covered))
}

async fn load_current_responsibility(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    user_identity_id: Uuid,
) -> Result<Option<ResponsibilityContract>, AppError> {
    let contract_json = sqlx::query_scalar::<_, String>(
        r#"
        SELECT contract::text
        FROM agent_responsibility_contracts
        WHERE project_id = $1 AND user_identity_id = $2
          AND state = 'active'
        ORDER BY revision DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(user_identity_id)
    .fetch_optional(&mut **transaction)
    .await?;
    contract_json
        .map(|value| serde_json::from_str(&value).map_err(|_| AppError::Internal))
        .transpose()
}

async fn responsibility_operationally_covers(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    responsibility: &ResponsibilityContract,
    local_goal: &LocalGoalContract,
) -> Result<bool, AppError> {
    let mut active_scopes = HashSet::new();
    for rule in &responsibility.rules {
        let within_scope = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM resource_closure
             WHERE project_id = $1 AND ancestor_id = $2 AND descendant_id = $3)",
        )
        .bind(project_id)
        .bind(Uuid::from(rule.scope))
        .bind(Uuid::from(local_goal.contract.scope))
        .fetch_one(&mut **transaction)
        .await?;
        if within_scope {
            active_scopes.insert(rule.scope);
        }
    }
    Ok(responsibility_operationally_covers_local_goal(
        responsibility,
        local_goal,
        |rule_scope, goal_scope| {
            goal_scope == local_goal.contract.scope && active_scopes.contains(&rule_scope)
        },
    ))
}

async fn authoritative_local_condition_facts(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    local: &LocalGoalContract,
) -> Result<ContractConditionFacts, AppError> {
    let mut tasks = HashSet::new();
    for obligation in &local.contract.obligations {
        collect_condition_tasks(&obligation.activation, &mut tasks);
        collect_condition_tasks(&obligation.required_for_completion, &mut tasks);
    }
    let task_ids: Vec<Uuid> = tasks.iter().copied().map(Uuid::from).collect();
    let completed_tasks = if task_ids.is_empty() {
        HashSet::new()
    } else {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT resource_node_id FROM tasks
             WHERE project_id = $1 AND resource_node_id = ANY($2)
               AND state = 'completed' AND deleted_at IS NULL",
        )
        .bind(project_id)
        .bind(&task_ids)
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .map(ResourceId::from)
        .collect()
    };
    Ok(ContractConditionFacts {
        completed_tasks,
        // Cross-owner mandate activation has no run-local discharge state and
        // no typed comment/admin condition adapter. Such conditions are
        // rejected by `cross_owner_condition_is_authoritative` below rather
        // than interpreted from an empty client-provided fact set.
        discharged_obligations: HashSet::new(),
        comment_authors: HashSet::new(),
        administrator_approvals: HashSet::new(),
    })
}

async fn persist_cross_owner_task_provenance(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    agent_id: Uuid,
    local: &LocalGoalContract,
) -> Result<(), AppError> {
    let pending = sqlx::query(
        "SELECT task_intent_id, task_resource_node_id
         FROM agent_cross_owner_assignments
         WHERE project_id = $1 AND target_agent_id = $2
           AND route = 'controller_review' AND decision = 'approved'
           AND state = 'approved_pending_mandate'
         FOR SHARE",
    )
    .bind(project_id)
    .bind(agent_id)
    .fetch_all(&mut **transaction)
    .await?;
    if pending.is_empty() {
        return Ok(());
    }
    let facts = authoritative_local_condition_facts(transaction, project_id, local).await?;
    for row in pending {
        let task_intent_id: Uuid = row.try_get("task_intent_id")?;
        let task_resource_id: Uuid = row.try_get("task_resource_node_id")?;
        let mut candidates = Vec::new();
        for clause in &local.clauses {
            if Uuid::from(clause.scope) != task_resource_id {
                continue;
            }
            for work_spec_id in &clause.work_spec_ids {
                let Some(work) = local
                    .contract
                    .work_specs
                    .iter()
                    .find(|work| work.id == *work_spec_id)
                else {
                    continue;
                };
                let Some(obligation) = local
                    .contract
                    .obligations
                    .iter()
                    .find(|obligation| obligation.id == work.obligation)
                else {
                    continue;
                };
                if work.owner == local.agent
                    && work
                        .allowed_actions
                        .contains(&AgentActionClass::AssignOwnTask)
                    && obligation.owner == local.agent
                    && cross_owner_condition_is_authoritative(&obligation.activation)
                    && cross_owner_condition_is_authoritative(&obligation.required_for_completion)
                    && obligation.activation.holds(&facts)
                    && obligation.required_for_completion.holds(&facts)
                {
                    candidates.push((obligation.id, work.id));
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        // Ambiguous or absent semantic classification remains fail-closed.
        if let [(obligation_id, work_spec_id)] = candidates.as_slice() {
            sqlx::query(
                "INSERT INTO agent_task_obligation_provenance (
                     project_id, task_intent_id, task_resource_node_id,
                     target_agent_id, local_goal_id, local_goal_revision,
                     obligation_id, work_spec_ordinal
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (project_id, task_intent_id) DO NOTHING",
            )
            .bind(project_id)
            .bind(task_intent_id)
            .bind(task_resource_id)
            .bind(agent_id)
            .bind(Uuid::from(local.id))
            .bind(to_i64(local.revision)?)
            .bind(*obligation_id)
            .bind(to_i64(*work_spec_id)?)
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

fn collect_condition_tasks(condition: &ContractCondition, tasks: &mut HashSet<ResourceId>) {
    match condition {
        ContractCondition::TaskDone { task } => {
            tasks.insert(*task);
        }
        ContractCondition::All { left, right } | ContractCondition::Any { left, right } => {
            collect_condition_tasks(left, tasks);
            collect_condition_tasks(right, tasks);
        }
        ContractCondition::Neg { condition } => collect_condition_tasks(condition, tasks),
        ContractCondition::Always {}
        | ContractCondition::Never {}
        | ContractCondition::ObligationDone { .. }
        | ContractCondition::CommentBy { .. }
        | ContractCondition::AdministratorApproved { .. } => {}
    }
}

fn cross_owner_condition_is_authoritative(condition: &ContractCondition) -> bool {
    match condition {
        ContractCondition::Always {}
        | ContractCondition::Never {}
        | ContractCondition::TaskDone { .. } => true,
        ContractCondition::All { left, right } | ContractCondition::Any { left, right } => {
            cross_owner_condition_is_authoritative(left)
                && cross_owner_condition_is_authoritative(right)
        }
        ContractCondition::Neg { condition } => cross_owner_condition_is_authoritative(condition),
        ContractCondition::ObligationDone { .. }
        | ContractCondition::CommentBy { .. }
        | ContractCondition::AdministratorApproved { .. } => false,
    }
}

#[derive(Clone, Copy)]
struct RunnerRecord {
    id: Uuid,
    device_id: Uuid,
    key_version: i32,
}

struct AgentRecord {
    governed: GovernedAgent,
    profile_resource_id: ResourceId,
}

impl std::ops::Deref for AgentRecord {
    type Target = GovernedAgent;

    fn deref(&self) -> &Self::Target {
        &self.governed
    }
}

async fn active_runner(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    agent: &GovernedAgent,
) -> Result<RunnerRecord, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT runner.id, runner.device_id, runner.activated_key_version
        FROM agent_runners runner
        JOIN devices device
          ON device.identity_id = runner.principal_identity_id
         AND device.id = runner.device_id
        JOIN device_keys key
          ON key.identity_id = runner.principal_identity_id
         AND key.device_id = runner.device_id
         AND key.key_version = runner.activated_key_version
        WHERE runner.project_id = $1 AND runner.agent_id = $2
          AND runner.state = 'active' AND key.revoked_at IS NULL
          AND device.trust_state = 'trusted' AND device.retired_at IS NULL
        ORDER BY runner.activated_at DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(agent.id))
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Unavailable)?;
    transaction.commit().await?;
    Ok(RunnerRecord {
        id: row.try_get("id")?,
        device_id: row.try_get("device_id")?,
        key_version: row.try_get("activated_key_version")?,
    })
}

async fn authenticated_runner(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    agent_id: Uuid,
) -> Result<RunnerRecord, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT runner.id, runner.device_id, runner.activated_key_version
        FROM agent_runners runner
        JOIN governed_agents agent
          ON agent.project_id = runner.project_id AND agent.id = runner.agent_id
        JOIN devices device
          ON device.identity_id = runner.principal_identity_id
         AND device.id = runner.device_id
        JOIN device_keys key
          ON key.identity_id = runner.principal_identity_id
         AND key.device_id = runner.device_id
         AND key.key_version = runner.activated_key_version
        WHERE runner.project_id = $1 AND runner.agent_id = $2
          AND runner.principal_identity_id = $3 AND runner.device_id = $4
          AND runner.state = 'active' AND agent.state = 'active'
          AND device.device_kind = 'service'
          AND device.trust_state = 'trusted' AND device.retired_at IS NULL
          AND key.revoked_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    sqlx::query(
        "UPDATE agent_runners SET last_seen_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2",
    )
    .bind(project_id)
    .bind(row.try_get::<Uuid, _>("id")?)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(RunnerRecord {
        id: row.try_get("id")?,
        device_id: row.try_get("device_id")?,
        key_version: row.try_get("activated_key_version")?,
    })
}

async fn validate_synthesis_runner(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    invocation_id: InvocationId,
    expected_task: &StructuredLanguageTaskEnvelope,
) -> Result<Uuid, AppError> {
    if expected_task.kind != StructuredLanguageTaskKind::SynthesizeGlobalContract {
        return Err(AppError::BadRequest(
            "global candidate requires a synthesis language task",
        ));
    }
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT invocation.agent_id, invocation.language_task::text AS language_task
        FROM agent_invocations invocation
        JOIN governed_agents agent
          ON agent.project_id = invocation.project_id
         AND agent.id = invocation.agent_id
        JOIN agent_runners runner
          ON runner.project_id = invocation.project_id
         AND runner.id = invocation.runner_id
         AND runner.agent_id = invocation.agent_id
        JOIN devices device
          ON device.identity_id = runner.principal_identity_id
         AND device.id = runner.device_id
        JOIN device_keys key
          ON key.identity_id = runner.principal_identity_id
         AND key.device_id = runner.device_id
         AND key.key_version = runner.activated_key_version
        WHERE invocation.project_id = $1 AND invocation.id = $2
          AND invocation.agent_identity_id = $3
          AND invocation.status = 'succeeded'
          AND agent.principal_identity_id = $3 AND agent.state = 'active'
          AND runner.device_id = $4 AND runner.state = 'active'
          AND device.device_kind = 'service'
          AND device.trust_state = 'trusted' AND device.retired_at IS NULL
          AND key.revoked_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(invocation_id))
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let stored_task: StructuredLanguageTaskEnvelope =
        serde_json::from_str(row.try_get("language_task")?).map_err(|_| AppError::Internal)?;
    if stored_task != *expected_task {
        return Err(AppError::Conflict);
    }
    transaction.commit().await?;
    Ok(row.try_get("agent_id")?)
}

async fn load_agent(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    agent_id: Uuid,
) -> Result<AgentRecord, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query(
        r#"
        SELECT principal_identity_id, controller_identity_id,
               profile_resource_node_id, availability
        FROM governed_agents
        WHERE project_id = $1 AND id = $2 AND state = 'active'
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    let availability = match row.try_get::<String, _>("availability")?.as_str() {
        "controller_private" => AgentAvailabilityMode::ControllerPrivate,
        "project_delegable" => AgentAvailabilityMode::ProjectDelegable,
        _ => return Err(AppError::Internal),
    };
    Ok(AgentRecord {
        governed: GovernedAgent {
            id: AgentId::from(agent_id),
            principal_id: UserId::from(row.try_get::<Uuid, _>("principal_identity_id")?),
            controller_id: UserId::from(row.try_get::<Uuid, _>("controller_identity_id")?),
            project_id: ProjectId::from(project_id),
            availability,
        },
        profile_resource_id: ResourceId::from(row.try_get::<Uuid, _>("profile_resource_node_id")?),
    })
}

fn agent_profile_resource(agent: &AgentRecord) -> ResourceId {
    agent.profile_resource_id
}

async fn supported_source_resource(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    source: &InformationSource,
) -> Result<Uuid, AppError> {
    let resource_id = source_resource(source).ok_or(AppError::BadRequest(
        "source kind has no concrete product adapter",
    ))?;
    let mut transaction = begin(state, actor, project_id).await?;
    let exists = match source {
        InformationSource::ResourceBody { .. } => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM resource_nodes
             WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL)",
            )
            .bind(project_id)
            .bind(resource_id)
            .fetch_one(&mut *transaction)
            .await?
        }
        InformationSource::InfoDocument { document_id, .. } => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM info_documents
                 WHERE project_id = $1 AND id = $2 AND resource_node_id = $3
                   AND deleted_at IS NULL)",
            )
            .bind(project_id)
            .bind(document_id)
            .bind(resource_id)
            .fetch_one(&mut *transaction)
            .await?
        }
        InformationSource::InfoFile { file_id, .. } => {
            sqlx::query_scalar::<_, bool>(
                r#"
            SELECT EXISTS (
                SELECT 1 FROM file_blobs blob
                JOIN file_links link
                  ON link.project_id = blob.project_id AND link.blob_id = blob.id
                WHERE blob.project_id = $1 AND blob.id = $2
                  AND link.resource_node_id = $3
                  AND blob.upload_state = 'available'
            )
            "#,
            )
            .bind(project_id)
            .bind(file_id)
            .bind(resource_id)
            .fetch_one(&mut *transaction)
            .await?
        }
        _ => false,
    };
    transaction.commit().await?;
    if exists {
        Ok(resource_id)
    } else {
        Err(AppError::NotFound)
    }
}

fn source_resource(source: &InformationSource) -> Option<Uuid> {
    match source {
        InformationSource::ResourceBody { resource_id }
        | InformationSource::Comment { resource_id, .. }
        | InformationSource::InfoDocument { resource_id, .. }
        | InformationSource::InfoFile { resource_id, .. } => Some((*resource_id).into()),
        InformationSource::ToolOutput { .. }
        | InformationSource::ProxyTranscript { .. }
        | InformationSource::EventHistory { .. }
        | InformationSource::Provenance { .. } => None,
    }
}

fn source_columns(source: &InformationSource) -> (&'static str, Option<Uuid>, Option<Uuid>) {
    match source {
        InformationSource::ResourceBody { resource_id } => {
            ("resource_body", Some((*resource_id).into()), None)
        }
        InformationSource::Comment {
            resource_id,
            comment_id,
        } => ("comment", Some((*resource_id).into()), Some(*comment_id)),
        InformationSource::InfoDocument {
            resource_id,
            document_id,
        } => (
            "info_document",
            Some((*resource_id).into()),
            Some(*document_id),
        ),
        InformationSource::InfoFile {
            resource_id,
            file_id,
        } => ("info_file", Some((*resource_id).into()), Some(*file_id)),
        InformationSource::ToolOutput { call_id } => ("tool_output", None, Some(*call_id)),
        InformationSource::ProxyTranscript { thread_id } => {
            ("proxy_transcript", None, Some((*thread_id).into()))
        }
        InformationSource::EventHistory { event_id } => ("event_history", None, Some(*event_id)),
        InformationSource::Provenance { provenance_id } => {
            ("provenance", None, Some(*provenance_id))
        }
    }
}

async fn load_sources(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    invocation_id: Uuid,
) -> Result<Vec<InformationSource>, AppError> {
    let descriptors = sqlx::query_scalar::<_, String>(
        r#"
        SELECT source_descriptor::text
        FROM agent_invocation_sources
        WHERE project_id = $1 AND invocation_id = $2
        ORDER BY ordinal
        "#,
    )
    .bind(project_id)
    .bind(invocation_id)
    .fetch_all(&mut **transaction)
    .await?;
    descriptors
        .into_iter()
        .map(|descriptor| serde_json::from_str(&descriptor).map_err(|_| AppError::Internal))
        .collect()
}

async fn ensure_runner_can_read(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    principal_id: UserId,
    device_id: Uuid,
    key_version: i32,
    resource_id: Uuid,
) -> Result<(), AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    ensure_runner_can_read_in_transaction(
        &mut transaction,
        project_id,
        principal_id.into(),
        device_id,
        key_version,
        resource_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn ensure_runner_can_read_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    principal_id: Uuid,
    device_id: Uuid,
    key_version: i32,
    resource_id: Uuid,
) -> Result<(), AppError> {
    if !resource_access_in_transaction(
        transaction,
        project_id,
        principal_id,
        resource_id,
        ResourceOperation::Read,
    )
    .await?
    {
        return Err(AppError::Forbidden);
    }
    let has_envelope = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM resource_epochs epoch
            JOIN resource_key_envelopes envelope
              ON envelope.project_id = epoch.project_id
             AND envelope.resource_node_id = epoch.resource_node_id
             AND envelope.epoch = epoch.epoch
             AND envelope.key_purpose = 'body'
            WHERE epoch.project_id = $1 AND epoch.resource_node_id = $2
              AND epoch.retired_at IS NULL
              AND envelope.recipient_identity_id = $3
              AND envelope.recipient_device_id = $4
              AND envelope.recipient_device_key_version = $5
              AND envelope.revoked_at IS NULL
        )
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .bind(principal_id)
    .bind(device_id)
    .bind(key_version)
    .fetch_one(&mut **transaction)
    .await?;
    if has_envelope {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn resource_access(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    resource_id: Uuid,
    operation: ResourceOperation,
) -> Result<bool, AppError> {
    resource_access_for_identity(
        state,
        actor,
        project_id,
        actor.identity_id,
        resource_id,
        operation,
    )
    .await
}

async fn resource_access_for_identity(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    principal_id: Uuid,
    resource_id: Uuid,
    operation: ResourceOperation,
) -> Result<bool, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let allowed = resource_access_in_transaction(
        &mut transaction,
        project_id,
        principal_id,
        resource_id,
        operation,
    )
    .await?;
    transaction.commit().await?;
    Ok(allowed)
}

async fn resource_access_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    principal_id: Uuid,
    resource_id: Uuid,
    operation: ResourceOperation,
) -> Result<bool, AppError> {
    let facts = sqlx::query_as::<_, (bool, bool, Option<String>, Option<String>)>(
        r#"
        SELECT
            project.owner_identity_id = $2 OR membership.role = 'admin',
            node.created_by_identity_id = $2,
            permission.access_level,
            permission.access_scope
        FROM resource_nodes node
        JOIN projects project ON project.id = node.project_id
        JOIN project_memberships membership
          ON membership.project_id = node.project_id
         AND membership.identity_id = $2
         AND membership.state = 'active'
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            $1, $3, $2
        ) permission ON true
        WHERE node.project_id = $1 AND node.id = $3 AND node.deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(principal_id)
    .bind(resource_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((owner_or_admin, creator, access, scope)) = facts else {
        return Ok(false);
    };
    let full = scope.as_deref() == Some("full");
    Ok(match operation {
        ResourceOperation::ViewHeader => owner_or_admin || creator || access.is_some(),
        ResourceOperation::Read | ResourceOperation::ReadComment | ResourceOperation::EditInfo => {
            owner_or_admin || creator || (full && access.is_some())
        }
        ResourceOperation::Write | ResourceOperation::PostComment => {
            owner_or_admin
                || creator
                || (full && matches!(access.as_deref(), Some("edit" | "manage")))
        }
        ResourceOperation::Manage | ResourceOperation::DelegateAssignedWork => {
            owner_or_admin || (full && access.as_deref() == Some("manage"))
        }
        ResourceOperation::CompleteAssignedTask => false,
    })
}

async fn resource_reader_audience(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    resource_id: Uuid,
) -> Result<HashSet<UserId>, AppError> {
    let readers = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT membership.identity_id
        FROM project_memberships membership
        JOIN projects project ON project.id = membership.project_id
        JOIN resource_nodes node
          ON node.project_id = membership.project_id AND node.id = $2
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            $1, $2, membership.identity_id
        ) permission ON true
        WHERE membership.project_id = $1 AND membership.state = 'active'
          AND node.deleted_at IS NULL
          AND (
              project.owner_identity_id = membership.identity_id
              OR membership.role = 'admin'
              OR node.created_by_identity_id = membership.identity_id
              OR (permission.access_scope = 'full' AND permission.access_level IS NOT NULL)
          )
        "#,
    )
    .bind(project_id)
    .bind(resource_id)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(readers.into_iter().map(UserId::from).collect())
}

async fn append_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    agent_id: AgentId,
    invocation_id: Option<InvocationId>,
    event_kind: &'static str,
    facts: Value,
) -> Result<(), AppError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 23))")
        .bind(Uuid::from(agent_id))
        .execute(&mut **transaction)
        .await?;
    let previous_hash = sqlx::query_scalar::<_, Vec<u8>>(
        r#"
        SELECT entry_hash FROM agent_audit_log
        WHERE project_id = $1 AND agent_id = $2
        ORDER BY sequence DESC LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(agent_id))
    .fetch_optional(&mut **transaction)
    .await?;
    let facts_json = canonical_json(&facts)?;
    let mut digest = Sha256::new();
    digest.update(b"sprout-agent-audit-v1");
    digest.update(project_id.as_bytes());
    digest.update(Uuid::from(agent_id).as_bytes());
    if let Some(invocation_id) = invocation_id {
        digest.update(Uuid::from(invocation_id).as_bytes());
    }
    digest.update(actor.identity_id.as_bytes());
    digest.update(actor.device_id.as_bytes());
    digest.update(event_kind.as_bytes());
    digest.update(facts_json.as_bytes());
    if let Some(previous_hash) = &previous_hash {
        digest.update(previous_hash);
    }
    let entry_hash: [u8; 32] = digest.finalize().into();
    sqlx::query(
        r#"
        INSERT INTO agent_audit_log (
            project_id, agent_id, invocation_id, actor_identity_id,
            actor_device_id, event_kind, facts, previous_hash, entry_hash
        ) VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8, $9)
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(agent_id))
    .bind(invocation_id.map(Uuid::from))
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(event_kind)
    .bind(facts_json)
    .bind(previous_hash)
    .bind(entry_hash.as_slice())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn append_user_governance_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    subject_user_identity_id: Uuid,
    agent_id: Option<Uuid>,
    event_kind: &'static str,
    facts: Value,
) -> Result<(), AppError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 37))")
        .bind(subject_user_identity_id)
        .execute(&mut **transaction)
        .await?;
    let previous_hash = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT entry_hash FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2
         ORDER BY sequence DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(subject_user_identity_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let facts_json = canonical_json(&facts)?;
    let mut digest = Sha256::new();
    digest.update(b"sprout-user-governance-audit-v1");
    digest.update(project_id.as_bytes());
    digest.update(subject_user_identity_id.as_bytes());
    if let Some(agent_id) = agent_id {
        digest.update(agent_id.as_bytes());
    }
    digest.update(actor.identity_id.as_bytes());
    digest.update(actor.device_id.as_bytes());
    digest.update(event_kind.as_bytes());
    digest.update(facts_json.as_bytes());
    if let Some(previous_hash) = &previous_hash {
        digest.update(previous_hash);
    }
    let entry_hash: [u8; 32] = digest.finalize().into();
    sqlx::query(
        r#"
        INSERT INTO agent_user_governance_audit_log (
            project_id, subject_user_identity_id, agent_id,
            actor_identity_id, actor_device_id, event_kind,
            facts, previous_hash, entry_hash
        ) VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8, $9)
        "#,
    )
    .bind(project_id)
    .bind(subject_user_identity_id)
    .bind(agent_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(event_kind)
    .bind(facts_json)
    .bind(previous_hash)
    .bind(entry_hash.as_slice())
    .execute(&mut **transaction)
    .await?;
    Ok(())
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

fn serialize_ciphertext(payload: &EncryptedPayload) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec(payload).map_err(|_| AppError::BadRequest("invalid encrypted payload"))
}

fn deserialize_ciphertext(bytes: &[u8]) -> Result<EncryptedPayload, AppError> {
    serde_json::from_slice(bytes).map_err(|_| AppError::Internal)
}

fn canonical_json(value: &impl Serialize) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|_| AppError::BadRequest("invalid structured payload"))
}

fn availability_name(availability: AgentAvailabilityMode) -> &'static str {
    match availability {
        AgentAvailabilityMode::ControllerPrivate => "controller_private",
        AgentAvailabilityMode::ProjectDelegable => "project_delegable",
    }
}

fn validate_identity_handle(handle: &str) -> Result<(), AppError> {
    if handle.len() < 3
        || handle.len() > 128
        || handle != handle.to_lowercase()
        || handle.chars().any(char::is_whitespace)
    {
        return Err(AppError::BadRequest("invalid agent identity handle"));
    }
    Ok(())
}

fn ensure_unique_effect_ids(effects: &[EffectProposalRequest]) -> Result<(), AppError> {
    let mut ids = HashSet::with_capacity(effects.len());
    if effects.iter().all(|effect| ids.insert(effect.id)) {
        Ok(())
    } else {
        Err(AppError::BadRequest("duplicate effect proposal id"))
    }
}

fn to_i32(value: u32) -> Result<i32, AppError> {
    i32::try_from(value).map_err(|_| AppError::BadRequest("numeric value is too large"))
}

fn to_i64(value: u64) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| AppError::BadRequest("numeric value is too large"))
}

fn agent_validation_error(_: sprout_domain::AgentValidationError) -> AppError {
    AppError::BadRequest("agent governance invariant failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_tokens_are_redacted_from_debug_output() {
        let token = SensitiveToken("secret-bootstrap-token".into());
        assert!(!format!("{token:?}").contains("secret-bootstrap-token"));
    }

    #[test]
    fn agent_handles_are_canonical_and_bounded() {
        assert!(validate_identity_handle("agent-alpha").is_ok());
        assert!(validate_identity_handle("Agent Alpha").is_err());
        assert!(validate_identity_handle("a").is_err());
    }

    #[test]
    fn source_columns_do_not_confuse_resource_and_record_ids() {
        let resource_id = ResourceId::new();
        let document_id = Uuid::new_v4();
        assert_eq!(
            source_columns(&InformationSource::InfoDocument {
                resource_id,
                document_id,
            }),
            (
                "info_document",
                Some(Uuid::from(resource_id)),
                Some(document_id)
            )
        );
    }
}
