use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sprout_domain::{
    AgentAvailabilityMode, AgentId, AuthorityEnvelope, EncryptedPayload, GovernedAgent,
    InformationSource, InvocationId, LocalGoalContract, ModelExposureProjection,
    ModelInvocationContext, PrincipalKind, ProjectId, ResourceEffect, ResourceId,
    ResourceOperation, ResponsibilityContract, StructuredLanguageOutput,
    StructuredLanguageTaskEnvelope, UserId, validate_information_flow,
    validate_state_grounded_invocation,
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
    } else if request.contract.supersedes_revision.is_some() {
        return Err(AppError::BadRequest(
            "first responsibility revision cannot supersede another revision",
        ));
    }
    sqlx::query(
        r#"
        INSERT INTO agent_responsibility_contracts (
            id, project_id, revision, administrator_identity_id,
            user_identity_id, contract, contract_hash
        ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7)
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
            controller_identity_id, revision, contract, contract_hash
        ) VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8)
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
