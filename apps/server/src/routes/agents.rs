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
use sprout_api_contract::{CreateTaskRequest, EncryptedPayloadDto};
use sprout_crypto_protocol::{canonical_governance_json, verify_ed25519_ml_dsa65_signatures};
use sprout_domain::{
    AdministratorResponsibilityDecisionMode, AdministratorResponsibilityReviewDraft,
    AgentActionClass, AgentAvailabilityMode, AgentId, AgentInterrogationCausalDelta,
    AgentInterrogationSession, ApprovedLocalGoalException, AuthorityEnvelope,
    BriefGovernanceSummary, CollaborativeRunState, ContractCondition, ContractConditionFacts,
    CrossOwnerAssignmentRoute, CurrentLocalObligationContext, EncryptedPayload,
    GlobalContractCandidate, GlobalCoverageNeed, GlobalMandateAssignment, GovernedAgent,
    InformationSource, InterrogationId, InvocationId, LOCAL_GOAL_CLASSIFIER_VERSION,
    LocalGoalCompilationEnvelope, LocalGoalCompilerOutput, LocalGoalContract, LocalGoalOrigin,
    LocalPromptReviewDisposition, ModelExposureProjection, ModelInvocationContext,
    ModelInvocationWorkBinding, ModelRuntimeActualObservation, NewAgentForGlobalNeedProposal,
    PersistedTaskIntent, PrincipalKind, ProjectId, ProxyExecution, ProxyRequestId, ProxyThreadId,
    R540ModelRuntimeProjection, ResourceEffect, ResourceId, ResourceOperation,
    ResponsibilityCompilationEnvelope, ResponsibilityCompilerOutput, ResponsibilityContract,
    ResponsibilityExceptionReview, StructuredGlobalSynthesisEnvelope,
    StructuredGlobalWorkGrounding, StructuredLanguageArtifact, StructuredLanguageOutput,
    StructuredLanguageTaskEnvelope, StructuredLanguageTaskKind, TaskObligationProvenance,
    UserEscalationConsent, UserId, UserProxy, UserProxyActionPlan,
    UserProxyOutOfResponsibilityConfirmation, UserProxyPlanningEnvelope, UserProxyRequest,
    UserProxyThread, classify_local_goal_contract, derive_global_coverage_need,
    responsibility_operationally_covers_local_goal, route_cross_owner_assignment,
    validate_approved_local_goal_exception, validate_global_synthesis, validate_information_flow,
    validate_model_runtime_projection, validate_state_grounded_invocation,
};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthSession, ProjectAccess, require_project_access, set_database_context},
    error::AppError,
};

const RUNNER_LEASE: Duration = Duration::from_secs(300);
const COMPILATION_SIGNATURE_CONTEXT: &[u8] = b"sprout-governance-compilation-v1";
const FINAL_PROMPT_APPROVAL_SIGNATURE_CONTEXT: &[u8] = b"sprout-final-prompt-approval-v1";
const ADMINISTRATOR_AGENT_CREATION_SIGNATURE_CONTEXT: &[u8] =
    b"sprout-administrator-agent-creation-v1";
const EXCEPTION_CONSENT_SIGNATURE_CONTEXT: &[u8] = b"sprout-local-goal-exception-consent-v1";
const EXCEPTION_DECISION_SIGNATURE_CONTEXT: &[u8] = b"sprout-local-goal-exception-decision-v1";
const MODEL_RUNTIME_OBSERVATION_SIGNATURE_CONTEXT: &[u8] = b"sprout-model-runtime-observation-v1";
const LOCAL_GOAL_COMPILER_PROTOCOL_MANIFEST: &[u8] =
    include_bytes!("../../tcb/sprout-local-goal-compiler-v1.json");
const RESPONSIBILITY_COMPILER_PROTOCOL_MANIFEST: &[u8] =
    include_bytes!("../../tcb/sprout-responsibility-compiler-v1.json");
const LOCAL_GOAL_COMPILER_PROTOCOL_MANIFEST_SHA256: [u8; 32] = [
    12, 103, 94, 133, 55, 1, 55, 92, 123, 165, 211, 150, 244, 225, 249, 181, 85, 146, 51, 154, 58,
    78, 69, 133, 155, 159, 44, 46, 143, 219, 191, 194,
];
const RESPONSIBILITY_COMPILER_PROTOCOL_MANIFEST_SHA256: [u8; 32] = [
    120, 189, 131, 219, 121, 17, 33, 145, 248, 26, 161, 24, 81, 32, 146, 247, 234, 84, 168, 119,
    51, 168, 46, 130, 63, 168, 60, 241, 7, 227, 235, 115,
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerIdentity {
    compiler_id: String,
    compiler_version: u32,
    compiler_build_digest_hex: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompilationSignatures {
    pub(crate) signer_identity_id: UserId,
    pub(crate) signer_device_id: Uuid,
    pub(crate) signer_device_key_version: u32,
    pub(crate) classical_signature: Vec<u8>,
    pub(crate) post_quantum_signature: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalCompilationAuthorization {
    Responsibility { id: Uuid, revision: u64 },
    AdministratorException { id: Uuid, revision: u64 },
    GlobalMandate { id: Uuid, revision: u64 },
    AdministratorCreation { approval_id: Uuid },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResponsibilityCompilationStatement {
    certificate_id: Uuid,
    compiler: CompilerIdentity,
    project_id: Uuid,
    responsibility_id: Uuid,
    revision: u64,
    draft_id: Uuid,
    administrator_identity_id: UserId,
    user_identity_id: UserId,
    source_text_commitment_hex: String,
    ciphertext_commitment_hex: String,
    output: ResponsibilityCompilerOutput,
    output_hash_hex: String,
    envelope: ResponsibilityCompilationEnvelope,
    envelope_hash_hex: String,
    idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedResponsibilityCompilation {
    statement: ResponsibilityCompilationStatement,
    signatures: CompilationSignatures,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGoalCompilationStatement {
    certificate_id: Uuid,
    compiler: CompilerIdentity,
    project_id: Uuid,
    local_goal_id: Uuid,
    local_revision: u64,
    draft_id: Uuid,
    agent_principal_identity_id: UserId,
    controller_identity_id: UserId,
    prompt_commitment_hex: String,
    ciphertext_commitment_hex: String,
    output: LocalGoalCompilerOutput,
    output_hash_hex: String,
    envelope: LocalGoalCompilationEnvelope,
    envelope_hash_hex: String,
    authorization: LocalCompilationAuthorization,
    idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLocalGoalCompilation {
    statement: LocalGoalCompilationStatement,
    signatures: CompilationSignatures,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FinalPromptApprovalStatement {
    approval_id: Uuid,
    project_id: Uuid,
    draft_id: Uuid,
    agent_principal_identity_id: UserId,
    controller_identity_id: UserId,
    local_goal_id: Uuid,
    local_revision: u64,
    prompt_commitment_hex: String,
    ciphertext_commitment_hex: String,
    compilation_certificate_id: Uuid,
    structured_output_hash_hex: String,
    approval_identity_hash_hex: String,
    idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedFinalPromptApproval {
    statement: FinalPromptApprovalStatement,
    signatures: CompilationSignatures,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdministratorAgentCreationApprovalStatement {
    approval_id: Uuid,
    project_id: Uuid,
    administrator_identity_id: UserId,
    signer_device_id: Uuid,
    signer_device_key_version: u32,
    proposed_agent_identity_id: UserId,
    governed_agent_id: AgentId,
    proposal_draft_id: Uuid,
    local_goal_id: Uuid,
    local_goal_revision: u64,
    contract_hash_hex: String,
    compilation_certificate_id: Uuid,
    prompt_plaintext_commitment_hex: String,
    ciphertext_commitment_hex: String,
    availability: AgentAvailabilityMode,
    scope: ResourceId,
    canonical_proposal_hash_hex: String,
    idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedAdministratorAgentCreationApproval {
    statement: AdministratorAgentCreationApprovalStatement,
    signatures: CompilationSignatures,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisionAgentRequest {
    id: AgentId,
    principal_identity_id: UserId,
    controller_identity_id: UserId,
    identity_handle: String,
    encrypted_profile: EncryptedPayload,
    profile_resource_node_id: ResourceId,
    key_epoch: u32,
    availability: AgentAvailabilityMode,
    runner_id: Uuid,
    runner_device_id: Uuid,
    encrypted_runner_label: EncryptedPayload,
    initial_local_goal: RecordLocalGoalRequest,
    final_prompt_approval: SignedFinalPromptApproval,
    administrator_creation_approval: Option<SignedAdministratorAgentCreationApproval>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct AdministratorCreationProposalBinding {
    project_id: Uuid,
    administrator_identity_id: UserId,
    proposed_agent_identity_id: UserId,
    governed_agent_id: AgentId,
    proposal_draft_id: Uuid,
    local_goal_id: Uuid,
    local_goal_revision: u64,
    contract_hash_hex: String,
    compilation_certificate_id: Uuid,
    prompt_plaintext_commitment_hex: String,
    ciphertext_commitment_hex: String,
    availability: AgentAvailabilityMode,
    scope: ResourceId,
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
    if request.controller_identity_id != actor.identity_id.into() {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
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
    let local_statement = &request.initial_local_goal.compilation.statement;
    if local_statement.project_id != project_id
        || local_statement.local_revision != 1
        || local_statement.agent_principal_identity_id != request.principal_identity_id
        || local_statement.controller_identity_id != request.controller_identity_id
        || local_statement.envelope.agent != request.principal_identity_id
        || local_statement.envelope.controller != request.controller_identity_id
        || request.initial_local_goal.supersedes_revision.is_some()
    {
        return Err(AppError::BadRequest(
            "initial LocalGoal does not match the proposed agent",
        ));
    }
    local_statement
        .output
        .validate_within_envelope(&local_statement.envelope)
        .map_err(agent_validation_error)?;
    let system_prompt = serialize_ciphertext(&request.initial_local_goal.encrypted_prompt)?;
    let ciphertext_commitment: [u8; 32] = Sha256::digest(&system_prompt).into();
    if decode_commitment(&local_statement.ciphertext_commitment_hex)? != ciphertext_commitment
        || decode_commitment(&local_statement.output_hash_hex)?
            != canonical_hash(&local_statement.output)?
        || decode_commitment(&local_statement.envelope_hash_hex)?
            != canonical_hash(&local_statement.envelope)?
    {
        return Err(AppError::BadRequest(
            "compiler artifact commitment mismatch",
        ));
    }
    let clauses = classify_local_goal_contract(&local_statement.output.contract);
    let classifier_output_hash = canonical_hash(&clauses)?;
    let origin = match &local_statement.authorization {
        LocalCompilationAuthorization::Responsibility { .. } => {
            LocalGoalOrigin::ControllerPrompt {}
        }
        LocalCompilationAuthorization::AdministratorCreation { approval_id } => {
            LocalGoalOrigin::AdministratorCreation {
                approval_id: *approval_id,
            }
        }
        LocalCompilationAuthorization::AdministratorException { .. }
        | LocalCompilationAuthorization::GlobalMandate { .. } => {
            return Err(AppError::BadRequest(
                "initial-agent authorization adapter is not available",
            ));
        }
    };
    let local_contract = LocalGoalContract {
        id: local_statement.local_goal_id.into(),
        revision: 1,
        agent: request.principal_identity_id,
        controller: request.controller_identity_id,
        encrypted_prompt: request.initial_local_goal.encrypted_prompt.clone(),
        contract: local_statement.output.contract.clone(),
        clauses,
        origin,
        supersedes_revision: None,
    };
    local_contract.validate().map_err(agent_validation_error)?;
    let runner_label = serialize_ciphertext(&request.encrypted_runner_label)?;
    let key_epoch = to_i32(request.key_epoch)?;

    let mut transaction = begin(&state, actor, project_id).await?;
    let project_scope_contains_contract = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM resource_closure
            WHERE project_id = $1 AND ancestor_id = $2 AND descendant_id = $3)",
    )
    .bind(project_id)
    .bind(Uuid::from(local_statement.envelope.project_scope))
    .bind(Uuid::from(local_contract.contract.scope))
    .fetch_one(&mut *transaction)
    .await?;
    if !project_scope_contains_contract
        || !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            Uuid::from(request.profile_resource_node_id),
            ResourceOperation::Manage,
        )
        .await?
        || !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            Uuid::from(local_contract.contract.scope),
            ResourceOperation::Manage,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    validate_initial_creation_authorization(
        &mut transaction,
        project_id,
        actor,
        &request,
        &local_contract,
    )
    .await?;
    let build_digest =
        require_pinned_compiler(&mut transaction, "local_goal", &local_statement.compiler).await?;
    let certificate_hash = verify_device_statement(
        &mut transaction,
        actor,
        local_statement,
        &request.initial_local_goal.compilation.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    let approval_statement = &request.final_prompt_approval.statement;
    if approval_statement.project_id != project_id
        || approval_statement.draft_id != local_statement.draft_id
        || approval_statement.agent_principal_identity_id != request.principal_identity_id
        || approval_statement.controller_identity_id != request.controller_identity_id
        || approval_statement.local_goal_id != Uuid::from(local_contract.id)
        || approval_statement.local_revision != 1
        || approval_statement.compilation_certificate_id != local_statement.certificate_id
        || decode_commitment(&approval_statement.structured_output_hash_hex)?
            != decode_commitment(&local_statement.output_hash_hex)?
        || decode_commitment(&approval_statement.prompt_commitment_hex)?
            != decode_commitment(&local_statement.prompt_commitment_hex)?
        || decode_commitment(&approval_statement.ciphertext_commitment_hex)?
            != ciphertext_commitment
        || decode_commitment(&approval_statement.approval_identity_hash_hex)?
            != final_prompt_approval_identity_hash(approval_statement)?
    {
        return Err(AppError::Conflict);
    }
    let approval_hash = verify_device_statement(
        &mut transaction,
        actor,
        approval_statement,
        &request.final_prompt_approval.signatures,
        FINAL_PROMPT_APPROVAL_SIGNATURE_CONTEXT,
    )
    .await?;
    let administrator_creation_approval = match &local_statement.authorization {
        LocalCompilationAuthorization::AdministratorCreation { .. } => {
            let signed = request
                .administrator_creation_approval
                .as_ref()
                .ok_or(AppError::Forbidden)?;
            let local_contract_hash = canonical_hash(&local_contract)?;
            validate_administrator_creation_approval(
                &mut transaction,
                actor,
                project_id,
                &request,
                &local_contract,
                local_contract_hash,
                signed,
            )
            .await?;
            Some((signed, local_contract_hash))
        }
        _ => {
            if request.administrator_creation_approval.is_some() {
                return Err(AppError::BadRequest(
                    "administrator creation approval is not applicable",
                ));
            }
            None
        }
    };
    let (authorization_kind, authorization_id, authorization_revision) =
        local_authorization_columns(&local_statement.authorization);
    persist_compilation_certificate(
        &mut transaction,
        project_id,
        CompilationRecord {
            id: local_statement.certificate_id,
            task_kind: "local_goal",
            compiler: &local_statement.compiler,
            build_digest,
            signer: &request.initial_local_goal.compilation.signatures,
            subject_id: Uuid::from(local_contract.id),
            subject_revision: 1,
            draft_id: local_statement.draft_id,
            agent_principal_identity_id: Some(Uuid::from(request.principal_identity_id)),
            controller_identity_id: Some(Uuid::from(request.controller_identity_id)),
            administrator_identity_id: None,
            user_identity_id: None,
            input_commitment: decode_commitment(&local_statement.prompt_commitment_hex)?,
            ciphertext_commitment,
            output_json: governance_canonical_json(&local_statement.output)?,
            output_hash: decode_commitment(&local_statement.output_hash_hex)?,
            envelope_json: governance_canonical_json(&local_statement.envelope)?,
            envelope_hash: decode_commitment(&local_statement.envelope_hash_hex)?,
            certificate_hash,
            idempotency_key: local_statement.idempotency_key,
            classifier_version: Some(LOCAL_GOAL_CLASSIFIER_VERSION),
            classifier_output_hash: Some(classifier_output_hash),
            authorization_kind,
            authorization_id,
            authorization_revision,
        },
    )
    .await?;
    if let Some((signed, contract_hash)) = administrator_creation_approval {
        persist_administrator_creation_approval(
            &mut transaction,
            project_id,
            signed,
            contract_hash,
        )
        .await?;
    }
    sqlx::query(
        r#"
        SELECT sprout_private.provision_edge_agent(
            $1, $2, $3, $4, $5, $6, $7, $8,
            $9, $10, $11, $12, $13, $14, $15, $16,
            $17, $18, $19, $20, $21, $22
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
    .bind(&system_prompt)
    .bind(key_epoch)
    .bind(availability_name(request.availability))
    .bind(request.runner_id)
    .bind(request.runner_device_id)
    .bind(runner_label)
    .bind(session_id)
    .bind(token_hash)
    .bind(expires_at)
    .bind(Uuid::from(local_contract.id))
    .bind(to_i64(local_contract.revision)?)
    .bind(local_statement.draft_id)
    .bind(local_statement.certificate_id)
    .bind(authorization_kind)
    .bind(authorization_id)
    .execute(&mut *transaction)
    .await?;
    let local_json = canonical_json(&local_contract)?;
    let local_hash: [u8; 32] = Sha256::digest(local_json.as_bytes()).into();
    sqlx::query(
        "INSERT INTO agent_local_goal_contracts (
            id, project_id, agent_id, agent_identity_id,
            controller_identity_id, revision, contract, contract_hash, state,
            compilation_certificate_id, classifier_version, classifier_output_hash,
            administrator_creation_approval_id
         ) VALUES ($1, $2, $3, $4, $5, 1, $6::jsonb, $7, 'draft', $8, $9, $10, $11)",
    )
    .bind(Uuid::from(local_contract.id))
    .bind(project_id)
    .bind(Uuid::from(request.id))
    .bind(Uuid::from(request.principal_identity_id))
    .bind(Uuid::from(request.controller_identity_id))
    .bind(&local_json)
    .bind(local_hash.as_slice())
    .bind(local_statement.certificate_id)
    .bind(to_i32(LOCAL_GOAL_CLASSIFIER_VERSION)?)
    .bind(classifier_output_hash.as_slice())
    .bind(match &local_statement.authorization {
        LocalCompilationAuthorization::AdministratorCreation { approval_id } => Some(*approval_id),
        _ => None,
    })
    .execute(&mut *transaction)
    .await?;
    append_verified_governance_revision(
        &mut transaction,
        project_id,
        "local_goal_revision",
        Uuid::from(local_contract.id),
        1,
        local_statement.certificate_id,
        local_hash,
    )
    .await?;
    sqlx::query(
        "INSERT INTO agent_prompt_revisions (
            project_id, agent_id, local_goal_id, local_goal_revision,
            encrypted_prompt, prompt_hash, state, draft_id
         ) VALUES ($1, $2, $3, 1, $4, $5, 'draft', $6)",
    )
    .bind(project_id)
    .bind(Uuid::from(request.id))
    .bind(Uuid::from(local_contract.id))
    .bind(&system_prompt)
    .bind(ciphertext_commitment.as_slice())
    .bind(local_statement.draft_id)
    .execute(&mut *transaction)
    .await?;
    let activated_local = sqlx::query(
        "UPDATE agent_local_goal_contracts SET state = 'active'
         WHERE project_id = $1 AND agent_id = $2 AND id = $3
           AND revision = 1 AND state = 'draft'",
    )
    .bind(project_id)
    .bind(Uuid::from(request.id))
    .bind(Uuid::from(local_contract.id))
    .execute(&mut *transaction)
    .await?;
    if activated_local.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let activated_prompt = sqlx::query(
        "UPDATE agent_prompt_revisions
         SET state = 'active', approved_by_identity_id = $4,
             activated_at = clock_timestamp()
         WHERE project_id = $1 AND agent_id = $2 AND draft_id = $3
           AND state = 'draft'",
    )
    .bind(project_id)
    .bind(Uuid::from(request.id))
    .bind(local_statement.draft_id)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    if activated_prompt.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    persist_final_prompt_approval(
        &mut transaction,
        project_id,
        local_statement.draft_id,
        Uuid::from(request.id),
        actor.identity_id,
        Uuid::from(local_contract.id),
        1,
        ciphertext_commitment,
        request.principal_identity_id,
        &request.final_prompt_approval,
        approval_hash,
    )
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
    append_audit(
        &mut transaction,
        actor,
        project_id,
        request.id,
        None,
        "local_goal_recorded",
        json!({
            "local_goal_id": local_contract.id,
            "revision": 1,
            "state": "active",
            "compilation_certificate_id": local_statement.certificate_id,
            "prompt_approval_id": approval_statement.approval_id,
            "classifier_version": LOCAL_GOAL_CLASSIFIER_VERSION,
        }),
    )
    .await?;
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        actor.identity_id,
        Some(Uuid::from(request.id)),
        "local_goal_activated",
        json!({
            "local_goal_id": local_contract.id,
            "revision": 1,
            "initial_creation": true,
            "compilation_certificate_id": local_statement.certificate_id,
            "prompt_approval_id": approval_statement.approval_id,
            "authorization_kind": authorization_kind,
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
    Json(approval): Json<SignedFinalPromptApproval>,
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
               local.compilation_certificate_id,
               prompt.draft_id, prompt.encrypted_prompt, prompt.prompt_hash,
               certificate.input_commitment, certificate.ciphertext_commitment,
               certificate.output_hash,
               certificate.authorization_kind, certificate.authorization_id,
               certificate.authorization_revision
        FROM agent_local_goal_contracts local
        JOIN agent_prompt_revisions prompt
          ON prompt.project_id = local.project_id
         AND prompt.agent_id = local.agent_id
         AND prompt.local_goal_id = local.id
         AND prompt.local_goal_revision = local.revision
        JOIN agent_compilation_certificates certificate
          ON certificate.project_id = local.project_id
         AND certificate.id = local.compilation_certificate_id
         AND certificate.task_kind = 'local_goal'
         AND certificate.subject_id = local.id
         AND certificate.subject_revision = local.revision
         AND certificate.verification_state = 'verified'
        JOIN agent_compiler_builds compiler_build
          ON compiler_build.task_kind = certificate.task_kind
         AND compiler_build.compiler_name = certificate.compiler_name
         AND compiler_build.compiler_version = certificate.compiler_version
         AND compiler_build.build_digest = certificate.compiler_build_digest
         AND compiler_build.enabled
        JOIN device_keys compiler_key
          ON compiler_key.identity_id = certificate.signer_identity_id
         AND compiler_key.device_id = certificate.signer_device_id
         AND compiler_key.key_version = certificate.signer_device_key_version
         AND compiler_key.revoked_at IS NULL
        JOIN devices compiler_device
          ON compiler_device.identity_id = compiler_key.identity_id
         AND compiler_device.id = compiler_key.device_id
         AND compiler_device.trust_state = 'trusted'
         AND compiler_device.retired_at IS NULL
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
    let compilation_certificate_id: Uuid = row.try_get("compilation_certificate_id")?;
    let expected_prompt_hash: [u8; 32] = Sha256::digest(&prompt_bytes).into();
    if stored_prompt != prompt_bytes || stored_prompt_hash != expected_prompt_hash {
        return Err(AppError::Conflict);
    }
    let approval_statement = &approval.statement;
    let prompt_input_commitment: Vec<u8> = row.try_get("input_commitment")?;
    let compilation_output_hash: Vec<u8> = row.try_get("output_hash")?;
    if approval_statement.project_id != project_id
        || approval_statement.draft_id != prompt_draft_id
        || approval_statement.agent_principal_identity_id != agent.principal_id
        || approval_statement.controller_identity_id != agent.controller_id
        || approval_statement.local_goal_id != local_goal_id
        || approval_statement.local_revision != revision
        || approval_statement.compilation_certificate_id != compilation_certificate_id
        || decode_commitment(&approval_statement.structured_output_hash_hex)?.as_slice()
            != compilation_output_hash
        || decode_commitment(&approval_statement.prompt_commitment_hex)?.as_slice()
            != prompt_input_commitment
        || decode_commitment(&approval_statement.ciphertext_commitment_hex)? != expected_prompt_hash
        || decode_commitment(&approval_statement.approval_identity_hash_hex)?
            != final_prompt_approval_identity_hash(approval_statement)?
    {
        return Err(AppError::Conflict);
    }
    let approval_hash = verify_device_statement(
        &mut transaction,
        actor,
        approval_statement,
        &approval.signatures,
        FINAL_PROMPT_APPROVAL_SIGNATURE_CONTEXT,
    )
    .await?;
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
    let authorization_kind: String = row.try_get("authorization_kind")?;
    let authorization_id: Option<Uuid> = row.try_get("authorization_id")?;
    let authorization_revision: Option<i64> = row.try_get("authorization_revision")?;
    let persisted_authorization = match (
        authorization_kind.as_str(),
        authorization_id,
        authorization_revision,
    ) {
        ("responsibility", Some(id), Some(revision)) => {
            LocalCompilationAuthorization::Responsibility {
                id,
                revision: u64::try_from(revision).map_err(|_| AppError::Internal)?,
            }
        }
        ("administrator_exception", Some(id), Some(revision)) => {
            LocalCompilationAuthorization::AdministratorException {
                id,
                revision: u64::try_from(revision).map_err(|_| AppError::Internal)?,
            }
        }
        ("global_mandate", Some(id), Some(revision)) => {
            LocalCompilationAuthorization::GlobalMandate {
                id,
                revision: u64::try_from(revision).map_err(|_| AppError::Internal)?,
            }
        }
        _ => return Err(AppError::Internal),
    };
    let responsibility_activation = validate_local_authorization(
        &mut transaction,
        project_id,
        actor,
        &contract,
        &persisted_authorization,
    )
    .await?;
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
    if let Some((responsibility_id, responsibility_revision, supersedes_revision)) =
        responsibility_activation
    {
        let superseded = sqlx::query(
            "UPDATE agent_responsibility_contracts
             SET state='superseded', superseded_at=clock_timestamp()
             WHERE project_id=$1 AND user_identity_id=$2 AND id=$3
               AND revision=$4 AND state='active'",
        )
        .bind(project_id)
        .bind(actor.identity_id)
        .bind(responsibility_id)
        .bind(to_i64(supersedes_revision)?)
        .execute(&mut *transaction)
        .await?;
        if superseded.rows_affected() != 1 {
            return Err(AppError::Conflict);
        }
        let activated = sqlx::query(
            "UPDATE agent_responsibility_contracts
             SET state='active', activated_at=clock_timestamp()
             WHERE project_id=$1 AND user_identity_id=$2 AND id=$3
               AND revision=$4 AND state='draft'",
        )
        .bind(project_id)
        .bind(actor.identity_id)
        .bind(responsibility_id)
        .bind(to_i64(responsibility_revision)?)
        .execute(&mut *transaction)
        .await?;
        if activated.rows_affected() != 1 {
            return Err(AppError::Conflict);
        }
    }
    persist_final_prompt_approval(
        &mut transaction,
        project_id,
        prompt_draft_id,
        agent_id,
        actor.identity_id,
        local_goal_id,
        revision,
        expected_prompt_hash,
        agent.principal_id,
        &approval,
        approval_hash,
    )
    .await?;
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
            "governance": authorization_kind,
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
            "governance": authorization_kind,
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
#[serde(deny_unknown_fields)]
pub struct RecordResponsibilityRequest {
    encrypted_source_text: EncryptedPayload,
    supersedes_revision: Option<u64>,
    compilation: SignedResponsibilityCompilation,
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
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    record_responsibility_common(
        &state,
        actor,
        project_id,
        Uuid::from(agent.controller_id),
        responsibility_id,
        Some(agent_id),
        request,
    )
    .await
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
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    record_responsibility_common(
        &state,
        actor,
        project_id,
        user_id,
        responsibility_id,
        None,
        request,
    )
    .await
}

async fn record_responsibility_common(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    user_id: Uuid,
    responsibility_id: Uuid,
    audit_agent_id: Option<Uuid>,
    request: RecordResponsibilityRequest,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let statement = &request.compilation.statement;
    if statement.project_id != project_id
        || statement.responsibility_id != responsibility_id
        || statement.administrator_identity_id != actor.identity_id.into()
        || statement.user_identity_id != user_id.into()
        || statement.envelope.administrator != actor.identity_id.into()
        || statement.envelope.user != user_id.into()
    {
        return Err(AppError::Forbidden);
    }
    statement
        .output
        .validate_within_envelope(&statement.envelope)
        .map_err(agent_validation_error)?;
    let encrypted_source = serialize_ciphertext(&request.encrypted_source_text)?;
    let ciphertext_commitment: [u8; 32] = Sha256::digest(&encrypted_source).into();
    if decode_commitment(&statement.ciphertext_commitment_hex)? != ciphertext_commitment
        || decode_commitment(&statement.output_hash_hex)? != canonical_hash(&statement.output)?
        || decode_commitment(&statement.envelope_hash_hex)? != canonical_hash(&statement.envelope)?
    {
        return Err(AppError::BadRequest(
            "compiler artifact commitment mismatch",
        ));
    }
    let mut transaction = begin(state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 35))")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    let user_is_human_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM project_memberships membership
            JOIN identities identity ON identity.id = membership.identity_id
            WHERE membership.project_id = $1 AND membership.identity_id = $2
              AND membership.state = 'active' AND identity.status = 'active'
              AND identity.principal_kind = 'user')",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(&mut *transaction)
    .await?;
    if !user_is_human_member {
        return Err(AppError::Forbidden);
    }
    for project_scope in &statement.envelope.project_scopes {
        if !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            Uuid::from(*project_scope),
            ResourceOperation::Manage,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
    }
    for rule in &statement.output.rules {
        let within_envelope = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM resource_closure
                WHERE project_id = $1 AND descendant_id = $2
                  AND ancestor_id = ANY($3::uuid[]))",
        )
        .bind(project_id)
        .bind(Uuid::from(rule.scope))
        .bind(
            statement
                .envelope
                .project_scopes
                .iter()
                .copied()
                .map(Uuid::from)
                .collect::<Vec<_>>(),
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !within_envelope
            || !resource_access_in_transaction(
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
    let build_digest =
        require_pinned_compiler(&mut transaction, "responsibility", &statement.compiler).await?;
    let certificate_hash = verify_device_statement(
        &mut transaction,
        actor,
        statement,
        &request.compilation.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    let contract = ResponsibilityContract {
        id: responsibility_id.into(),
        revision: statement.revision,
        administrator: statement.administrator_identity_id,
        user: statement.user_identity_id,
        encrypted_source_text: request.encrypted_source_text,
        rules: statement.output.rules.clone(),
        supersedes_revision: request.supersedes_revision,
    };
    contract
        .validate(
            |principal| {
                if principal == contract.administrator {
                    Some(PrincipalKind::Administrator)
                } else if principal == contract.user {
                    Some(PrincipalKind::User)
                } else {
                    None
                }
            },
            |_, _| true,
        )
        .map_err(agent_validation_error)?;
    if contract.revision > 1 {
        let supersedes = contract.supersedes_revision.ok_or(AppError::Conflict)?;
        let previous_json = sqlx::query_scalar::<_, String>(
            "SELECT contract::text FROM agent_responsibility_contracts
             WHERE project_id = $1 AND id = $2 AND revision = $3 AND state = 'active'",
        )
        .bind(project_id)
        .bind(responsibility_id)
        .bind(to_i64(supersedes)?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let previous: ResponsibilityContract =
            serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
        contract
            .validate_revision_of(&previous)
            .map_err(agent_validation_error)?;
    } else if contract.supersedes_revision.is_some()
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
    let output_hash = decode_commitment(&statement.output_hash_hex)?;
    let envelope_hash = decode_commitment(&statement.envelope_hash_hex)?;
    persist_compilation_certificate(
        &mut transaction,
        project_id,
        CompilationRecord {
            id: statement.certificate_id,
            task_kind: "responsibility",
            compiler: &statement.compiler,
            build_digest,
            signer: &request.compilation.signatures,
            subject_id: responsibility_id,
            subject_revision: statement.revision,
            draft_id: statement.draft_id,
            agent_principal_identity_id: None,
            controller_identity_id: None,
            administrator_identity_id: Some(actor.identity_id),
            user_identity_id: Some(user_id),
            input_commitment: decode_commitment(&statement.source_text_commitment_hex)?,
            ciphertext_commitment,
            output_json: governance_canonical_json(&statement.output)?,
            output_hash,
            envelope_json: governance_canonical_json(&statement.envelope)?,
            envelope_hash,
            certificate_hash,
            idempotency_key: statement.idempotency_key,
            classifier_version: None,
            classifier_output_hash: None,
            authorization_kind: "responsibility_compilation",
            authorization_id: None,
            authorization_revision: None,
        },
    )
    .await?;
    let contract_json = canonical_json(&contract)?;
    let contract_hash: [u8; 32] = Sha256::digest(contract_json.as_bytes()).into();
    sqlx::query(
        "INSERT INTO agent_responsibility_contracts (
            id, project_id, revision, administrator_identity_id,
            user_identity_id, contract, contract_hash, state,
            compilation_certificate_id
         ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, 'draft', $8)",
    )
    .bind(responsibility_id)
    .bind(project_id)
    .bind(to_i64(contract.revision)?)
    .bind(actor.identity_id)
    .bind(user_id)
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .bind(statement.certificate_id)
    .execute(&mut *transaction)
    .await?;
    append_verified_governance_revision(
        &mut transaction,
        project_id,
        "responsibility_revision",
        responsibility_id,
        contract.revision,
        statement.certificate_id,
        contract_hash,
    )
    .await?;
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        user_id,
        audit_agent_id,
        "responsibility_drafted",
        json!({
            "responsibility_id": responsibility_id,
            "revision": contract.revision,
            "contract_hash": hex::encode(contract_hash),
            "compilation_certificate_id": statement.certificate_id,
            "compiler_build_digest": statement.compiler.compiler_build_digest_hex,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: responsibility_id,
        revision: contract.revision,
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
        "SELECT responsibility.contract::text AS contract,
                responsibility.contract_hash
         FROM agent_responsibility_contracts responsibility
         JOIN agent_compilation_certificates certificate
           ON certificate.project_id = responsibility.project_id
          AND certificate.id = responsibility.compilation_certificate_id
          AND certificate.task_kind = 'responsibility'
          AND certificate.subject_id = responsibility.id
          AND certificate.subject_revision = responsibility.revision
          AND certificate.verification_state = 'verified'
         JOIN device_keys signer_key
           ON signer_key.identity_id = certificate.signer_identity_id
          AND signer_key.device_id = certificate.signer_device_id
          AND signer_key.key_version = certificate.signer_device_key_version
          AND signer_key.revoked_at IS NULL
         JOIN devices signer_device
           ON signer_device.identity_id = signer_key.identity_id
          AND signer_device.id = signer_key.device_id
          AND signer_device.trust_state = 'trusted'
          AND signer_device.retired_at IS NULL
         JOIN agent_compiler_builds compiler_build
           ON compiler_build.task_kind = certificate.task_kind
          AND compiler_build.compiler_name = certificate.compiler_name
          AND compiler_build.compiler_version = certificate.compiler_version
          AND compiler_build.build_digest = certificate.compiler_build_digest
          AND compiler_build.enabled
         WHERE responsibility.project_id = $1
           AND responsibility.user_identity_id = $2
           AND responsibility.id = $3 AND responsibility.revision = $4
           AND responsibility.state = 'draft'
         FOR UPDATE OF responsibility",
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordLocalGoalRequest {
    encrypted_prompt: EncryptedPayload,
    supersedes_revision: Option<u64>,
    compilation: SignedLocalGoalCompilation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordLocalDraftDispositionRequest {
    event_id: Uuid,
    idempotency_key: Uuid,
    disposition: LocalPromptReviewDisposition,
    source: RecordLocalGoalRequest,
    summary: Option<BriefGovernanceSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExceptionConsentStatement {
    event_id: Uuid,
    idempotency_key: Uuid,
    project_id: Uuid,
    consent: UserEscalationConsent,
    summary: BriefGovernanceSummary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedExceptionConsent {
    statement: ExceptionConsentStatement,
    signatures: CompilationSignatures,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordExceptionReviewRequest {
    event_id: Uuid,
    idempotency_key: Uuid,
    review: ResponsibilityExceptionReview,
    review_task: CreateTaskRequest,
    review_assignment_id: Uuid,
    review_permission_grant_id: Uuid,
    encrypted_assignment: EncryptedPayloadDto,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordAdministratorReviewDraftRequest {
    event_id: Uuid,
    idempotency_key: Uuid,
    revision: u64,
    encrypted_prompt: EncryptedPayload,
    local_compilation: SignedLocalGoalCompilation,
    final_responsibility: Option<SignedResponsibilityCompilation>,
    final_responsibility_encrypted_source: Option<EncryptedPayload>,
    final_responsibility_supersedes_revision: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AdministratorReviewDraftEventPayload {
    draft: AdministratorResponsibilityReviewDraft,
    encrypted_prompt: EncryptedPayload,
    local_compilation: SignedLocalGoalCompilation,
    final_responsibility: Option<SignedResponsibilityCompilation>,
    final_responsibility_encrypted_source: Option<EncryptedPayload>,
    final_responsibility_supersedes_revision: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExceptionDecisionStatement {
    event_id: Uuid,
    idempotency_key: Uuid,
    project_id: Uuid,
    decision: sprout_domain::AdministratorResponsibilityDecision,
    summary: BriefGovernanceSummary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedExceptionDecision {
    statement: ExceptionDecisionStatement,
    signatures: CompilationSignatures,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordGlobalCoverageNeedRequest {
    event_id: Uuid,
    idempotency_key: Uuid,
    global_contract_id: Uuid,
    global_revision: u64,
    obligation_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordGlobalMandateRequest {
    event_id: Uuid,
    idempotency_key: Uuid,
    need_id: Uuid,
    supersedes_revision: u64,
    encrypted_prompt: EncryptedPayload,
    compilation: SignedLocalGoalCompilation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordGlobalAgentProposalRequest {
    event_id: Uuid,
    idempotency_key: Uuid,
    need_id: Uuid,
    global_contract_id: Uuid,
    proposal: NewAgentForGlobalNeedProposal,
    encrypted_prompt: EncryptedPayload,
    compilation: SignedLocalGoalCompilation,
}

fn build_certified_local_contract(
    encrypted_prompt: EncryptedPayload,
    compilation: &SignedLocalGoalCompilation,
    supersedes_revision: Option<u64>,
    origin: LocalGoalOrigin,
) -> Result<(LocalGoalContract, [u8; 32], [u8; 32]), AppError> {
    let statement = &compilation.statement;
    statement
        .output
        .validate_within_envelope(&statement.envelope)
        .map_err(agent_validation_error)?;
    let prompt_bytes = serialize_ciphertext(&encrypted_prompt)?;
    let ciphertext_commitment: [u8; 32] = Sha256::digest(&prompt_bytes).into();
    if decode_commitment(&statement.ciphertext_commitment_hex)? != ciphertext_commitment
        || decode_commitment(&statement.output_hash_hex)? != canonical_hash(&statement.output)?
        || decode_commitment(&statement.envelope_hash_hex)? != canonical_hash(&statement.envelope)?
    {
        return Err(AppError::BadRequest(
            "compiler artifact commitment mismatch",
        ));
    }
    let clauses = classify_local_goal_contract(&statement.output.contract);
    let classifier_output_hash = canonical_hash(&clauses)?;
    let contract = LocalGoalContract {
        id: statement.local_goal_id.into(),
        revision: statement.local_revision,
        agent: statement.agent_principal_identity_id,
        controller: statement.controller_identity_id,
        encrypted_prompt,
        contract: statement.output.contract.clone(),
        clauses,
        origin,
        supersedes_revision,
    };
    contract.validate().map_err(agent_validation_error)?;
    Ok((contract, ciphertext_commitment, classifier_output_hash))
}

async fn require_exact_active_local_base(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    agent_id: Uuid,
    contract: &LocalGoalContract,
) -> Result<(), AppError> {
    if contract.revision <= 1 {
        return Err(AppError::Conflict);
    }
    let supersedes = contract.supersedes_revision.ok_or(AppError::Conflict)?;
    let previous_json = sqlx::query_scalar::<_, String>(
        "SELECT contract::text FROM agent_local_goal_contracts
         WHERE project_id = $1 AND agent_id = $2 AND id = $3
           AND revision = $4 AND state = 'active' FOR UPDATE",
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(Uuid::from(contract.id))
    .bind(to_i64(supersedes)?)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let previous: LocalGoalContract =
        serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
    contract
        .validate_revision_of(&previous)
        .map_err(agent_validation_error)
}

#[allow(clippy::too_many_arguments)]
async fn persist_certified_local_draft(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    agent_id: Uuid,
    expected_signer: UserId,
    encrypted_prompt: EncryptedPayload,
    compilation: &SignedLocalGoalCompilation,
    supersedes_revision: Option<u64>,
    origin: LocalGoalOrigin,
) -> Result<LocalGoalContract, AppError> {
    let statement = &compilation.statement;
    let (contract, ciphertext_commitment, classifier_output_hash) =
        build_certified_local_contract(encrypted_prompt, compilation, supersedes_revision, origin)?;
    if statement.project_id != project_id {
        return Err(AppError::Conflict);
    }
    require_exact_active_local_base(transaction, project_id, agent_id, &contract).await?;
    let within_scope = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM resource_closure
         WHERE project_id = $1 AND ancestor_id = $2 AND descendant_id = $3)",
    )
    .bind(project_id)
    .bind(Uuid::from(statement.envelope.project_scope))
    .bind(Uuid::from(contract.contract.scope))
    .fetch_one(&mut **transaction)
    .await?;
    if !within_scope {
        return Err(AppError::Forbidden);
    }
    let build_digest =
        require_pinned_compiler(transaction, "local_goal", &statement.compiler).await?;
    let certificate_hash = verify_device_statement_for_signer(
        transaction,
        expected_signer,
        statement,
        &compilation.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    let (authorization_kind, authorization_id, authorization_revision) =
        local_authorization_columns(&statement.authorization);
    persist_compilation_certificate(
        transaction,
        project_id,
        CompilationRecord {
            id: statement.certificate_id,
            task_kind: "local_goal",
            compiler: &statement.compiler,
            build_digest,
            signer: &compilation.signatures,
            subject_id: Uuid::from(contract.id),
            subject_revision: contract.revision,
            draft_id: statement.draft_id,
            agent_principal_identity_id: Some(Uuid::from(contract.agent)),
            controller_identity_id: Some(Uuid::from(contract.controller)),
            administrator_identity_id: None,
            user_identity_id: None,
            input_commitment: decode_commitment(&statement.prompt_commitment_hex)?,
            ciphertext_commitment,
            output_json: governance_canonical_json(&statement.output)?,
            output_hash: decode_commitment(&statement.output_hash_hex)?,
            envelope_json: governance_canonical_json(&statement.envelope)?,
            envelope_hash: decode_commitment(&statement.envelope_hash_hex)?,
            certificate_hash,
            idempotency_key: statement.idempotency_key,
            classifier_version: Some(LOCAL_GOAL_CLASSIFIER_VERSION),
            classifier_output_hash: Some(classifier_output_hash),
            authorization_kind,
            authorization_id,
            authorization_revision,
        },
    )
    .await?;
    let contract_json = canonical_json(&contract)?;
    let contract_hash: [u8; 32] = Sha256::digest(contract_json.as_bytes()).into();
    let existing = sqlx::query_as::<_, (Vec<u8>, Uuid, Vec<u8>)>(
        "SELECT local.contract_hash, prompt.draft_id, prompt.prompt_hash
         FROM agent_local_goal_contracts local
         JOIN agent_prompt_revisions prompt ON prompt.project_id=local.project_id
          AND prompt.agent_id=local.agent_id AND prompt.local_goal_id=local.id
          AND prompt.local_goal_revision=local.revision
         WHERE local.project_id=$1 AND local.agent_id=$2 AND local.id=$3
           AND local.revision=$4 AND local.state='draft' AND prompt.state='draft'",
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(Uuid::from(contract.id))
    .bind(to_i64(contract.revision)?)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some((stored_hash, stored_draft, stored_prompt_hash)) = existing {
        if stored_hash == contract_hash
            && stored_draft == statement.draft_id
            && stored_prompt_hash == ciphertext_commitment
        {
            return Ok(contract);
        }
        return Err(AppError::Conflict);
    }
    sqlx::query(
        "INSERT INTO agent_local_goal_contracts (
            id, project_id, agent_id, agent_identity_id, controller_identity_id,
            revision, contract, contract_hash, state, compilation_certificate_id,
            classifier_version, classifier_output_hash
         ) VALUES ($1,$2,$3,$4,$5,$6,$7::jsonb,$8,'draft',$9,$10,$11)",
    )
    .bind(Uuid::from(contract.id))
    .bind(project_id)
    .bind(agent_id)
    .bind(Uuid::from(contract.agent))
    .bind(Uuid::from(contract.controller))
    .bind(to_i64(contract.revision)?)
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .bind(statement.certificate_id)
    .bind(to_i32(LOCAL_GOAL_CLASSIFIER_VERSION)?)
    .bind(classifier_output_hash.as_slice())
    .execute(&mut **transaction)
    .await?;
    append_verified_governance_revision(
        transaction,
        project_id,
        "local_goal_revision",
        Uuid::from(contract.id),
        contract.revision,
        statement.certificate_id,
        contract_hash,
    )
    .await?;
    sqlx::query(
        "INSERT INTO agent_prompt_revisions (
            project_id, agent_id, draft_id, local_goal_id, local_goal_revision,
            encrypted_prompt, prompt_hash, state
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,'draft')",
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(statement.draft_id)
    .bind(Uuid::from(contract.id))
    .bind(to_i64(contract.revision)?)
    .bind(serialize_ciphertext(&contract.encrypted_prompt)?)
    .bind(ciphertext_commitment.as_slice())
    .execute(&mut **transaction)
    .await?;
    Ok(contract)
}

async fn persist_certified_responsibility_draft(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    expected_administrator: UserId,
    expected_user: UserId,
    encrypted_source: EncryptedPayload,
    supersedes_revision: u64,
    signed: &SignedResponsibilityCompilation,
) -> Result<ResponsibilityContract, AppError> {
    let statement = &signed.statement;
    if statement.project_id != project_id
        || statement.administrator_identity_id != expected_administrator
        || statement.user_identity_id != expected_user
    {
        return Err(AppError::Conflict);
    }
    statement
        .output
        .validate_within_envelope(&statement.envelope)
        .map_err(agent_validation_error)?;
    let encrypted_bytes = serialize_ciphertext(&encrypted_source)?;
    let ciphertext_commitment: [u8; 32] = Sha256::digest(&encrypted_bytes).into();
    if decode_commitment(&statement.ciphertext_commitment_hex)? != ciphertext_commitment
        || decode_commitment(&statement.output_hash_hex)? != canonical_hash(&statement.output)?
        || decode_commitment(&statement.envelope_hash_hex)? != canonical_hash(&statement.envelope)?
    {
        return Err(AppError::BadRequest(
            "compiler artifact commitment mismatch",
        ));
    }
    let contract = ResponsibilityContract {
        id: statement.responsibility_id.into(),
        revision: statement.revision,
        administrator: expected_administrator,
        user: expected_user,
        encrypted_source_text: encrypted_source,
        rules: statement.output.rules.clone(),
        supersedes_revision: Some(supersedes_revision),
    };
    contract
        .validate(
            |principal| {
                if principal == expected_administrator {
                    Some(PrincipalKind::Administrator)
                } else if principal == expected_user {
                    Some(PrincipalKind::User)
                } else {
                    None
                }
            },
            |_, _| true,
        )
        .map_err(agent_validation_error)?;
    let supersedes = contract.supersedes_revision.ok_or(AppError::Conflict)?;
    let previous_json = sqlx::query_scalar::<_, String>(
        "SELECT contract::text FROM agent_responsibility_contracts
         WHERE project_id=$1 AND user_identity_id=$2 AND id=$3
           AND revision=$4 AND state='active' FOR UPDATE",
    )
    .bind(project_id)
    .bind(Uuid::from(expected_user))
    .bind(Uuid::from(contract.id))
    .bind(to_i64(supersedes)?)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let previous: ResponsibilityContract =
        serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
    contract
        .validate_revision_of(&previous)
        .map_err(agent_validation_error)?;
    let build_digest =
        require_pinned_compiler(transaction, "responsibility", &statement.compiler).await?;
    let certificate_hash = verify_device_statement_for_signer(
        transaction,
        expected_administrator,
        statement,
        &signed.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    persist_compilation_certificate(
        transaction,
        project_id,
        CompilationRecord {
            id: statement.certificate_id,
            task_kind: "responsibility",
            compiler: &statement.compiler,
            build_digest,
            signer: &signed.signatures,
            subject_id: Uuid::from(contract.id),
            subject_revision: contract.revision,
            draft_id: statement.draft_id,
            agent_principal_identity_id: None,
            controller_identity_id: None,
            administrator_identity_id: Some(Uuid::from(expected_administrator)),
            user_identity_id: Some(Uuid::from(expected_user)),
            input_commitment: decode_commitment(&statement.source_text_commitment_hex)?,
            ciphertext_commitment,
            output_json: governance_canonical_json(&statement.output)?,
            output_hash: decode_commitment(&statement.output_hash_hex)?,
            envelope_json: governance_canonical_json(&statement.envelope)?,
            envelope_hash: decode_commitment(&statement.envelope_hash_hex)?,
            certificate_hash,
            idempotency_key: statement.idempotency_key,
            classifier_version: None,
            classifier_output_hash: None,
            authorization_kind: "responsibility_compilation",
            authorization_id: None,
            authorization_revision: None,
        },
    )
    .await?;
    let contract_json = canonical_json(&contract)?;
    let contract_hash: [u8; 32] = Sha256::digest(contract_json.as_bytes()).into();
    let existing = sqlx::query_as::<_, (Vec<u8>, String)>(
        "SELECT contract_hash, state FROM agent_responsibility_contracts
         WHERE project_id=$1 AND id=$2 AND revision=$3",
    )
    .bind(project_id)
    .bind(Uuid::from(contract.id))
    .bind(to_i64(contract.revision)?)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some((stored_hash, stored_state)) = existing {
        if stored_hash == contract_hash && stored_state == "draft" {
            return Ok(contract);
        }
        return Err(AppError::Conflict);
    }
    sqlx::query(
        "INSERT INTO agent_responsibility_contracts (
            id,project_id,revision,administrator_identity_id,user_identity_id,
            contract,contract_hash,state,compilation_certificate_id
         ) VALUES ($1,$2,$3,$4,$5,$6::jsonb,$7,'draft',$8)",
    )
    .bind(Uuid::from(contract.id))
    .bind(project_id)
    .bind(to_i64(contract.revision)?)
    .bind(Uuid::from(expected_administrator))
    .bind(Uuid::from(expected_user))
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .bind(statement.certificate_id)
    .execute(&mut **transaction)
    .await?;
    append_verified_governance_revision(
        transaction,
        project_id,
        "responsibility_revision",
        Uuid::from(contract.id),
        contract.revision,
        statement.certificate_id,
        contract_hash,
    )
    .await?;
    Ok(contract)
}

pub async fn record_local_draft_disposition(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RecordLocalDraftDispositionRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    if agent.controller_id != actor.identity_id.into() {
        return Err(AppError::Forbidden);
    }
    let statement = &request.source.compilation.statement;
    let LocalCompilationAuthorization::Responsibility { id, revision } = statement.authorization
    else {
        return Err(AppError::BadRequest(
            "rewrite or escalation requires the current Responsibility provenance",
        ));
    };
    let (contract, _, _) = build_certified_local_contract(
        request.source.encrypted_prompt.clone(),
        &request.source.compilation,
        request.source.supersedes_revision,
        LocalGoalOrigin::ControllerPrompt {},
    )?;
    if statement.project_id != project_id
        || contract.agent != agent.principal_id
        || contract.controller != agent.controller_id
    {
        return Err(AppError::Forbidden);
    }
    if request.disposition == LocalPromptReviewDisposition::RequestAdministratorReview {
        request
            .summary
            .as_ref()
            .ok_or(AppError::BadRequest("escalation summary required"))?
            .validate_escalation(statement.draft_id, contract.revision)
            .map_err(agent_validation_error)?;
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 44))")
        .bind(agent_id)
        .execute(&mut *transaction)
        .await?;
    require_exact_active_local_base(&mut transaction, project_id, agent_id, &contract).await?;
    let responsibility_json = sqlx::query_scalar::<_, String>(
        "SELECT contract::text FROM agent_responsibility_contracts
         WHERE project_id = $1 AND id = $2 AND revision = $3
           AND user_identity_id = $4 AND state = 'active'
           AND compilation_certificate_id IS NOT NULL",
    )
    .bind(project_id)
    .bind(id)
    .bind(to_i64(revision)?)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let responsibility: ResponsibilityContract =
        serde_json::from_str(&responsibility_json).map_err(|_| AppError::Internal)?;
    if responsibility_operationally_covers(&mut transaction, project_id, &responsibility, &contract)
        .await?
    {
        return Err(AppError::BadRequest(
            "covered draft must use the ordinary local-goal route",
        ));
    }
    require_pinned_compiler(&mut transaction, "local_goal", &statement.compiler).await?;
    verify_device_statement(
        &mut transaction,
        actor,
        statement,
        &request.source.compilation.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    let payload = serde_json::to_value(&request).map_err(|_| AppError::Internal)?;
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: request.event_id,
            event_kind: "local_draft_disposition",
            workflow_id: statement.draft_id,
            workflow_revision: 0,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(actor.identity_id),
            administrator_identity_id: None,
            agent_id: Some(agent_id),
            source_draft_id: Some(statement.draft_id),
            review_task_id: None,
            local_goal_id: Some(Uuid::from(contract.id)),
            local_goal_revision: Some(contract.revision),
            global_contract_id: None,
            global_revision: None,
            obligation_id: None,
            compilation_certificate_id: None,
            responsibility_compilation_id: None,
            idempotency_key: request.idempotency_key,
            payload: &payload,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: request.event_id,
        revision: contract.revision,
        contract_hash_hex: hex::encode(canonical_hash(&contract)?),
    }))
}

pub async fn record_exception_consent(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(signed): Json<SignedExceptionConsent>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    let statement = &signed.statement;
    if agent.controller_id != actor.identity_id.into()
        || statement.project_id != project_id
        || statement.consent.user != actor.identity_id.into()
    {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    let disposition_json = sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM agent_governance_authorization_events
         WHERE project_id = $1 AND event_kind = 'local_draft_disposition'
           AND source_draft_id = $2 AND user_identity_id = $3 AND agent_id = $4
           AND payload->>'disposition' = 'request_administrator_review'",
    )
    .bind(project_id)
    .bind(statement.consent.source_draft_id)
    .bind(actor.identity_id)
    .bind(agent_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let disposition: RecordLocalDraftDispositionRequest =
        serde_json::from_str(&disposition_json).map_err(|_| AppError::Internal)?;
    statement
        .summary
        .validate_escalation(
            statement.consent.source_draft_id,
            disposition.source.compilation.statement.local_revision,
        )
        .map_err(agent_validation_error)?;
    verify_device_statement(
        &mut transaction,
        actor,
        statement,
        &signed.signatures,
        EXCEPTION_CONSENT_SIGNATURE_CONTEXT,
    )
    .await?;
    let payload = serde_json::to_value(&signed).map_err(|_| AppError::Internal)?;
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: statement.event_id,
            event_kind: "exception_consent",
            workflow_id: statement.consent.review_id,
            workflow_revision: 0,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(actor.identity_id),
            administrator_identity_id: None,
            agent_id: Some(agent_id),
            source_draft_id: Some(statement.consent.source_draft_id),
            review_task_id: None,
            local_goal_id: None,
            local_goal_revision: None,
            global_contract_id: None,
            global_revision: None,
            obligation_id: None,
            compilation_certificate_id: None,
            responsibility_compilation_id: None,
            idempotency_key: statement.idempotency_key,
            payload: &payload,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: statement.consent.review_id,
        revision: 0,
        contract_hash_hex: hex::encode(canonical_hash(statement)?),
    }))
}

pub async fn record_exception_review(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RecordExceptionReviewRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    if agent.controller_id != actor.identity_id.into()
        || request.review.user != actor.identity_id.into()
        || request.review.agent != agent.principal_id
    {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 47))")
        .bind(request.review.id)
        .execute(&mut *transaction)
        .await?;
    let consent_json = sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM agent_governance_authorization_events
         WHERE project_id = $1 AND event_kind = 'exception_consent'
           AND workflow_id = $2 AND user_identity_id = $3 AND agent_id = $4",
    )
    .bind(project_id)
    .bind(request.review.id)
    .bind(actor.identity_id)
    .bind(agent_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let consent_signed: SignedExceptionConsent =
        serde_json::from_str(&consent_json).map_err(|_| AppError::Internal)?;
    if !consent_signed.statement.consent.consented {
        return Err(AppError::Conflict);
    }
    verify_device_statement_for_signer(
        &mut transaction,
        consent_signed.statement.consent.user,
        &consent_signed.statement,
        &consent_signed.signatures,
        EXCEPTION_CONSENT_SIGNATURE_CONTEXT,
    )
    .await?;
    let source_json = sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='local_draft_disposition'
           AND source_draft_id=$2 AND agent_id=$3",
    )
    .bind(project_id)
    .bind(request.review.source_draft_id)
    .bind(agent_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let source: RecordLocalDraftDispositionRequest =
        serde_json::from_str(&source_json).map_err(|_| AppError::Internal)?;
    let (source_local, _, _) = build_certified_local_contract(
        source.source.encrypted_prompt,
        &source.source.compilation,
        source.source.supersedes_revision,
        LocalGoalOrigin::ControllerPrompt {},
    )?;
    if request.review.proposed_local != source_local {
        return Err(AppError::Conflict);
    }
    if request.review_task.resource_node_id != Uuid::from(request.review.review_task) {
        return Err(AppError::Conflict);
    }
    let existing_review = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='exception_review'
           AND workflow_id=$2)",
    )
    .bind(project_id)
    .bind(request.review.id)
    .fetch_one(&mut *transaction)
    .await?;
    if !existing_review {
        let historical_task = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM tasks WHERE project_id=$1
             AND (id=$2 OR resource_node_id=$3))",
        )
        .bind(project_id)
        .bind(request.review_task.id)
        .bind(request.review_task.resource_node_id)
        .fetch_one(&mut *transaction)
        .await?;
        if historical_task {
            return Err(AppError::Conflict);
        }
        super::task_flows::materialize_governance_review_task(
            &mut transaction,
            actor,
            project_id,
            Uuid::from(request.review.agent),
            Uuid::from(request.review.administrator),
            &request.review_task,
            request.review_assignment_id,
            request.review_permission_grant_id,
            &request.encrypted_assignment,
        )
        .await?;
    }
    let task_exact = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM tasks task
            JOIN resource_nodes resource ON resource.project_id = task.project_id
             AND resource.id = task.resource_node_id AND resource.node_kind = 'task'
            JOIN task_assignments assignment ON assignment.project_id = task.project_id
             AND assignment.task_id = task.id AND assignment.revoked_at IS NULL
            WHERE task.project_id = $1 AND task.id=$2 AND task.resource_node_id = $3
              AND task.created_by_identity_id = $4
              AND assignment.id=$5 AND assignment.assignee_identity_id = $6
              AND task.deleted_at IS NULL)",
    )
    .bind(project_id)
    .bind(request.review_task.id)
    .bind(Uuid::from(request.review.review_task))
    .bind(Uuid::from(request.review.agent))
    .bind(request.review_assignment_id)
    .bind(Uuid::from(request.review.administrator))
    .fetch_one(&mut *transaction)
    .await?;
    request
        .review
        .validate(&consent_signed.statement.consent, |_, _, _| task_exact)
        .map_err(agent_validation_error)?;
    let administrator = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM project_memberships membership
         JOIN identities identity ON identity.id = membership.identity_id
         WHERE membership.project_id = $1 AND membership.identity_id = $2
           AND membership.state = 'active' AND membership.role IN ('owner', 'admin')
           AND identity.status = 'active' AND identity.principal_kind = 'user')",
    )
    .bind(project_id)
    .bind(Uuid::from(request.review.administrator))
    .fetch_one(&mut *transaction)
    .await?;
    if !administrator {
        return Err(AppError::Forbidden);
    }
    let payload = serde_json::to_value(&request).map_err(|_| AppError::Internal)?;
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: request.event_id,
            event_kind: "exception_review",
            workflow_id: request.review.id,
            workflow_revision: 0,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(actor.identity_id),
            administrator_identity_id: Some(Uuid::from(request.review.administrator)),
            agent_id: Some(agent_id),
            source_draft_id: Some(request.review.source_draft_id),
            review_task_id: Some(Uuid::from(request.review.review_task)),
            local_goal_id: Some(Uuid::from(request.review.proposed_local.id)),
            local_goal_revision: Some(request.review.proposed_local.revision),
            global_contract_id: None,
            global_revision: None,
            obligation_id: None,
            compilation_certificate_id: None,
            responsibility_compilation_id: None,
            idempotency_key: request.idempotency_key,
            payload: &payload,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: request.review.id,
        revision: 0,
        contract_hash_hex: hex::encode(canonical_hash(&request.review)?),
    }))
}

pub async fn record_exception_administrator_draft(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, review_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RecordAdministratorReviewDraftRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent || request.revision == 0 {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 45))")
        .bind(review_id)
        .execute(&mut *transaction)
        .await?;
    let review_json = sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='exception_review'
           AND workflow_id=$2 AND agent_id=$3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(review_id)
    .bind(agent_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let review_payload: Value =
        serde_json::from_str(&review_json).map_err(|_| AppError::Internal)?;
    let review: ResponsibilityExceptionReview = serde_json::from_value(
        review_payload
            .get("review")
            .cloned()
            .ok_or(AppError::Internal)?,
    )
    .map_err(|_| AppError::Internal)?;
    if review.administrator != actor.identity_id.into()
        || review.agent != agent.principal_id
        || review.user != agent.controller_id
    {
        return Err(AppError::Forbidden);
    }
    let statement = &request.local_compilation.statement;
    if statement.project_id != project_id
        || statement.agent_principal_identity_id != agent.principal_id
        || statement.controller_identity_id != agent.controller_id
        || !matches!(
            statement.authorization,
            LocalCompilationAuthorization::AdministratorException { id, revision }
                if id == review_id && revision == request.revision
        )
    {
        return Err(AppError::Conflict);
    }
    let (final_local, _, _) = build_certified_local_contract(
        request.encrypted_prompt.clone(),
        &request.local_compilation,
        review.proposed_local.supersedes_revision,
        LocalGoalOrigin::AdministratorException { review_id },
    )?;
    require_exact_active_local_base(&mut transaction, project_id, agent_id, &final_local).await?;
    require_pinned_compiler(&mut transaction, "local_goal", &statement.compiler).await?;
    verify_device_statement(
        &mut transaction,
        actor,
        statement,
        &request.local_compilation.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    let final_responsibility = match (
        request.final_responsibility.as_ref(),
        request.final_responsibility_encrypted_source.as_ref(),
        request.final_responsibility_supersedes_revision,
    ) {
        (None, None, None) => None,
        (Some(signed), Some(encrypted_source), Some(supersedes)) => {
            let responsibility_statement = &signed.statement;
            responsibility_statement
                .output
                .validate_within_envelope(&responsibility_statement.envelope)
                .map_err(agent_validation_error)?;
            if responsibility_statement.project_id != project_id
                || responsibility_statement.administrator_identity_id != actor.identity_id.into()
                || responsibility_statement.user_identity_id != agent.controller_id
                || responsibility_statement.revision <= supersedes
                || decode_commitment(&responsibility_statement.ciphertext_commitment_hex)?
                    != <[u8; 32]>::from(Sha256::digest(serialize_ciphertext(encrypted_source)?))
            {
                return Err(AppError::Conflict);
            }
            require_pinned_compiler(
                &mut transaction,
                "responsibility",
                &responsibility_statement.compiler,
            )
            .await?;
            verify_device_statement(
                &mut transaction,
                actor,
                responsibility_statement,
                &signed.signatures,
                COMPILATION_SIGNATURE_CONTEXT,
            )
            .await?;
            Some(ResponsibilityContract {
                id: responsibility_statement.responsibility_id.into(),
                revision: responsibility_statement.revision,
                administrator: responsibility_statement.administrator_identity_id,
                user: responsibility_statement.user_identity_id,
                encrypted_source_text: encrypted_source.clone(),
                rules: responsibility_statement.output.rules.clone(),
                supersedes_revision: Some(supersedes),
            })
        }
        _ => {
            return Err(AppError::BadRequest(
                "incomplete Responsibility draft artifact",
            ));
        }
    };
    let draft = AdministratorResponsibilityReviewDraft {
        review_id,
        revision: request.revision,
        administrator: actor.identity_id.into(),
        final_local,
        final_responsibility,
    };
    let payload = AdministratorReviewDraftEventPayload {
        draft: draft.clone(),
        encrypted_prompt: request.encrypted_prompt,
        local_compilation: request.local_compilation,
        final_responsibility: request.final_responsibility,
        final_responsibility_encrypted_source: request.final_responsibility_encrypted_source,
        final_responsibility_supersedes_revision: request.final_responsibility_supersedes_revision,
    };
    let payload_value = serde_json::to_value(&payload).map_err(|_| AppError::Internal)?;
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: request.event_id,
            event_kind: "exception_admin_draft",
            workflow_id: review_id,
            workflow_revision: request.revision,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(Uuid::from(review.user)),
            administrator_identity_id: Some(actor.identity_id),
            agent_id: Some(agent_id),
            source_draft_id: Some(review.source_draft_id),
            review_task_id: Some(Uuid::from(review.review_task)),
            local_goal_id: Some(Uuid::from(draft.final_local.id)),
            local_goal_revision: Some(draft.final_local.revision),
            global_contract_id: None,
            global_revision: None,
            obligation_id: None,
            compilation_certificate_id: None,
            responsibility_compilation_id: None,
            idempotency_key: request.idempotency_key,
            payload: &payload_value,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: review_id,
        revision: request.revision,
        contract_hash_hex: hex::encode(canonical_hash(&draft)?),
    }))
}

pub async fn decide_exception_review(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id, review_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(signed): Json<SignedExceptionDecision>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    let statement = &signed.statement;
    if statement.project_id != project_id
        || statement.decision.review_id != review_id
        || statement.decision.administrator != actor.identity_id.into()
    {
        return Err(AppError::Forbidden);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 45))")
        .bind(review_id)
        .execute(&mut *transaction)
        .await?;
    let review_json = sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='exception_review'
           AND workflow_id=$2 AND agent_id=$3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(review_id)
    .bind(agent_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let review_payload: Value =
        serde_json::from_str(&review_json).map_err(|_| AppError::Internal)?;
    let review: ResponsibilityExceptionReview = serde_json::from_value(
        review_payload
            .get("review")
            .cloned()
            .ok_or(AppError::Internal)?,
    )
    .map_err(|_| AppError::Internal)?;
    let draft_json = sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='exception_admin_draft'
           AND workflow_id=$2 AND workflow_revision=$3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(review_id)
    .bind(to_i64(statement.decision.review_draft_revision)?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let payload: AdministratorReviewDraftEventPayload =
        serde_json::from_str(&draft_json).map_err(|_| AppError::Internal)?;
    if review.administrator != actor.identity_id.into()
        || review.agent != agent.principal_id
        || review.user != agent.controller_id
        || payload.draft.administrator != actor.identity_id.into()
    {
        return Err(AppError::Forbidden);
    }
    statement
        .summary
        .validate_administrator_decision(agent.principal_id, payload.draft.final_local.revision)
        .map_err(agent_validation_error)?;
    let task_done = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM tasks task
         WHERE task.project_id=$1 AND task.resource_node_id=$2
           AND task.state IN ('completed','cancelled','archived')
           AND task.completed_at IS NOT NULL)",
    )
    .bind(project_id)
    .bind(Uuid::from(review.review_task))
    .fetch_one(&mut *transaction)
    .await?;
    if !task_done {
        return Err(AppError::Conflict);
    }
    verify_device_statement(
        &mut transaction,
        actor,
        statement,
        &signed.signatures,
        EXCEPTION_DECISION_SIGNATURE_CONTEXT,
    )
    .await?;
    let decision_payload = serde_json::to_value(&signed).map_err(|_| AppError::Internal)?;
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: statement.event_id,
            event_kind: "exception_decision",
            workflow_id: review_id,
            workflow_revision: statement.decision.review_draft_revision,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(Uuid::from(review.user)),
            administrator_identity_id: Some(actor.identity_id),
            agent_id: Some(agent_id),
            source_draft_id: Some(review.source_draft_id),
            review_task_id: Some(Uuid::from(review.review_task)),
            local_goal_id: Some(Uuid::from(payload.draft.final_local.id)),
            local_goal_revision: Some(payload.draft.final_local.revision),
            global_contract_id: None,
            global_revision: None,
            obligation_id: None,
            compilation_certificate_id: None,
            responsibility_compilation_id: None,
            idempotency_key: statement.idempotency_key,
            payload: &decision_payload,
        },
    )
    .await?;
    let approved_event_id = derived_governance_id(
        b"approved-local-exception",
        &[project_id, review_id, statement.event_id],
    );
    let approved_replay = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='approved_local_exception'
           AND event_id=$2 AND workflow_id=$3 AND workflow_revision=$4
           AND payload->>'decision_event_id'=$5)",
    )
    .bind(project_id)
    .bind(approved_event_id)
    .bind(review_id)
    .bind(to_i64(statement.decision.review_draft_revision)?)
    .bind(statement.event_id.to_string())
    .fetch_one(&mut *transaction)
    .await?;
    if approved_replay {
        transaction.commit().await?;
        return Ok(Json(ContractRecordedResponse {
            id: review_id,
            revision: statement.decision.review_draft_revision,
            contract_hash_hex: hex::encode(canonical_hash(statement)?),
        }));
    }
    if matches!(
        statement.decision.mode,
        AdministratorResponsibilityDecisionMode::Rejected
    ) {
        transaction.commit().await?;
        return Ok(Json(ContractRecordedResponse {
            id: review_id,
            revision: statement.decision.review_draft_revision,
            contract_hash_hex: hex::encode(canonical_hash(statement)?),
        }));
    }
    let approved = ApprovedLocalGoalException {
        review_id,
        administrator: actor.identity_id.into(),
        user: review.user,
        local: payload.draft.final_local.clone(),
    };
    validate_approved_local_goal_exception(
        &review,
        &payload.draft,
        &statement.decision,
        &approved,
        task_done,
    )
    .map_err(agent_validation_error)?;
    let local = persist_certified_local_draft(
        &mut transaction,
        project_id,
        agent_id,
        actor.identity_id.into(),
        payload.encrypted_prompt,
        &payload.local_compilation,
        payload.draft.final_local.supersedes_revision,
        LocalGoalOrigin::AdministratorException { review_id },
    )
    .await?;
    if local != approved.local {
        return Err(AppError::Conflict);
    }
    let responsibility_compilation_id = if matches!(
        statement.decision.mode,
        AdministratorResponsibilityDecisionMode::ApprovedGoalAndResponsibility
    ) {
        let responsibility_signed = payload
            .final_responsibility
            .as_ref()
            .ok_or(AppError::Conflict)?;
        let encrypted_source = payload
            .final_responsibility_encrypted_source
            .clone()
            .ok_or(AppError::Conflict)?;
        let supersedes = payload
            .final_responsibility_supersedes_revision
            .ok_or(AppError::Conflict)?;
        let responsibility = persist_certified_responsibility_draft(
            &mut transaction,
            project_id,
            actor.identity_id.into(),
            review.user,
            encrypted_source,
            supersedes,
            responsibility_signed,
        )
        .await?;
        if payload.draft.final_responsibility.as_ref() != Some(&responsibility) {
            return Err(AppError::Conflict);
        }
        Some(responsibility_signed.statement.certificate_id)
    } else {
        None
    };
    let approved_payload = json!({
        "approved": approved,
        "decision_event_id": statement.event_id,
        "review_draft_revision": statement.decision.review_draft_revision,
        "mode": statement.decision.mode,
    });
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: approved_event_id,
            event_kind: "approved_local_exception",
            workflow_id: review_id,
            workflow_revision: statement.decision.review_draft_revision,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(Uuid::from(review.user)),
            administrator_identity_id: Some(actor.identity_id),
            agent_id: Some(agent_id),
            source_draft_id: Some(review.source_draft_id),
            review_task_id: Some(Uuid::from(review.review_task)),
            local_goal_id: Some(Uuid::from(local.id)),
            local_goal_revision: Some(local.revision),
            global_contract_id: None,
            global_revision: None,
            obligation_id: None,
            compilation_certificate_id: Some(payload.local_compilation.statement.certificate_id),
            responsibility_compilation_id,
            idempotency_key: statement.event_id,
            payload: &approved_payload,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: review_id,
        revision: statement.decision.review_draft_revision,
        contract_hash_hex: hex::encode(canonical_hash(&approved)?),
    }))
}

pub async fn record_global_coverage_need(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<RecordGlobalCoverageNeedRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let candidate_json = sqlx::query_scalar::<_, String>(
        "SELECT candidate::text FROM agent_global_contracts
         WHERE project_id=$1 AND id=$2 AND revision=$3
           AND revision=(SELECT max(revision) FROM agent_global_contracts
                         WHERE project_id=$1 AND id=$2)",
    )
    .bind(project_id)
    .bind(request.global_contract_id)
    .bind(to_i64(request.global_revision)?)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let candidate: GlobalContractCandidate =
        serde_json::from_str(&candidate_json).map_err(|_| AppError::Internal)?;
    let need = derive_global_coverage_need(&candidate, request.obligation_id)
        .map_err(agent_validation_error)?;
    if need.global_revision != request.global_revision {
        return Err(AppError::Conflict);
    }
    let payload = json!({ "need": need });
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: request.event_id,
            event_kind: "global_coverage_need",
            workflow_id: request.event_id,
            workflow_revision: request.global_revision,
            actor_identity_id: actor.identity_id,
            user_identity_id: None,
            administrator_identity_id: Some(actor.identity_id),
            agent_id: None,
            source_draft_id: None,
            review_task_id: None,
            local_goal_id: None,
            local_goal_revision: None,
            global_contract_id: Some(request.global_contract_id),
            global_revision: Some(request.global_revision),
            obligation_id: Some(request.obligation_id),
            compilation_certificate_id: None,
            responsibility_compilation_id: None,
            idempotency_key: request.idempotency_key,
            payload: &payload,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: request.event_id,
        revision: request.global_revision,
        contract_hash_hex: hex::encode(canonical_hash(&need)?),
    }))
}

pub async fn record_global_mandate(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RecordGlobalMandateRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let agent = load_agent(&state, actor, project_id, agent_id).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 46))")
        .bind(agent_id)
        .execute(&mut *transaction)
        .await?;
    let need_row = sqlx::query(
        "SELECT payload::text, global_contract_id, global_revision, obligation_id
         FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='global_coverage_need'
           AND event_id=$2 FOR UPDATE",
    )
    .bind(project_id)
    .bind(request.need_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let need_payload: Value =
        serde_json::from_str(need_row.try_get("payload")?).map_err(|_| AppError::Internal)?;
    let need: GlobalCoverageNeed = serde_json::from_value(
        need_payload
            .get("need")
            .cloned()
            .ok_or(AppError::Internal)?,
    )
    .map_err(|_| AppError::Internal)?;
    let global_contract_id: Uuid = need_row.try_get("global_contract_id")?;
    let statement = &request.compilation.statement;
    if !matches!(
        statement.authorization,
        LocalCompilationAuthorization::GlobalMandate { id, revision }
            if id == request.event_id && revision == need.global_revision
    ) || statement.agent_principal_identity_id != agent.principal_id
        || statement.controller_identity_id != agent.controller_id
    {
        return Err(AppError::Conflict);
    }
    let (local, _, _) = build_certified_local_contract(
        request.encrypted_prompt.clone(),
        &request.compilation,
        Some(request.supersedes_revision),
        LocalGoalOrigin::GlobalMandate {
            global_revision: need.global_revision,
        },
    )?;
    require_exact_active_local_base(&mut transaction, project_id, agent_id, &local).await?;
    let administrator_current = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM project_memberships membership
         WHERE membership.project_id=$1 AND membership.identity_id=$2
           AND membership.state='active' AND membership.role IN ('owner','admin'))",
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    let mut resource_permissions = true;
    for effect in &need.required.resource_effects {
        resource_permissions &= resource_access_in_transaction(
            &mut transaction,
            project_id,
            Uuid::from(agent.principal_id),
            Uuid::from(effect.resource_id),
            effect.operation,
        )
        .await?;
    }
    let assignment = GlobalMandateAssignment {
        global_revision: need.global_revision,
        assigned_by: actor.identity_id.into(),
        need: need.clone(),
        local: local.clone(),
    };
    assignment
        .validate(
            &agent,
            administrator_current,
            |_| resource_permissions,
            |_| false,
        )
        .map_err(agent_validation_error)?;
    let latest_global = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_global_contracts
         WHERE project_id=$1 AND id=$2 AND revision=$3
           AND revision=(SELECT max(revision) FROM agent_global_contracts
                         WHERE project_id=$1 AND id=$2))",
    )
    .bind(project_id)
    .bind(global_contract_id)
    .bind(to_i64(need.global_revision)?)
    .fetch_one(&mut *transaction)
    .await?;
    if !latest_global || !need.required.tools.is_empty() {
        return Err(AppError::Forbidden);
    }
    // Persist the exact assignment before the compilation ledger entry.  The
    // assignment's certificate FK is deferred to transaction commit, allowing
    // `append_verified_governance_revision` to require this causal predecessor
    // while still rolling both records back atomically on any later failure.
    let payload = json!({ "assignment": assignment, "need_id": request.need_id });
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: request.event_id,
            event_kind: "global_mandate_assignment",
            workflow_id: request.event_id,
            workflow_revision: need.global_revision,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(Uuid::from(agent.controller_id)),
            administrator_identity_id: Some(actor.identity_id),
            agent_id: Some(agent_id),
            source_draft_id: Some(statement.draft_id),
            review_task_id: None,
            local_goal_id: Some(Uuid::from(local.id)),
            local_goal_revision: Some(local.revision),
            global_contract_id: Some(global_contract_id),
            global_revision: Some(need.global_revision),
            obligation_id: Some(need.obligation),
            compilation_certificate_id: Some(statement.certificate_id),
            responsibility_compilation_id: None,
            idempotency_key: request.idempotency_key,
            payload: &payload,
        },
    )
    .await?;
    let materialized = persist_certified_local_draft(
        &mut transaction,
        project_id,
        agent_id,
        agent.controller_id,
        request.encrypted_prompt,
        &request.compilation,
        local.supersedes_revision,
        LocalGoalOrigin::GlobalMandate {
            global_revision: need.global_revision,
        },
    )
    .await?;
    if materialized != local {
        return Err(AppError::Conflict);
    }
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: request.event_id,
        revision: local.revision,
        contract_hash_hex: hex::encode(canonical_hash(&local)?),
    }))
}

pub async fn record_global_agent_proposal(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<RecordGlobalAgentProposalRequest>,
) -> Result<Json<ContractRecordedResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    request
        .proposal
        .validate()
        .map_err(agent_validation_error)?;
    let statement = &request.compilation.statement;
    if statement.project_id != project_id
        || statement.agent_principal_identity_id != request.proposal.proposed_agent
        || statement.controller_identity_id != request.proposal.controller
        || !matches!(
            statement.authorization,
            LocalCompilationAuthorization::GlobalMandate { id, revision }
                if id == request.event_id && revision == request.proposal.need.global_revision
        )
        || statement.output.contract != request.proposal.local.contract
        || statement.local_goal_id != Uuid::from(request.proposal.local.id)
        || statement.local_revision != request.proposal.local.revision
    {
        return Err(AppError::Conflict);
    }
    let (local, _, _) = build_certified_local_contract(
        request.encrypted_prompt.clone(),
        &request.compilation,
        request.proposal.local.supersedes_revision,
        request.proposal.local.origin.clone(),
    )?;
    if local != request.proposal.local {
        return Err(AppError::Conflict);
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    let need_payload = sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='global_coverage_need'
           AND event_id=$2 AND global_contract_id=$3",
    )
    .bind(project_id)
    .bind(request.need_id)
    .bind(request.global_contract_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let need_value: Value = serde_json::from_str(&need_payload).map_err(|_| AppError::Internal)?;
    let need: GlobalCoverageNeed =
        serde_json::from_value(need_value.get("need").cloned().ok_or(AppError::Internal)?)
            .map_err(|_| AppError::Internal)?;
    if need != request.proposal.need || request.proposal.requested != need.required {
        return Err(AppError::Conflict);
    }
    require_pinned_compiler(&mut transaction, "local_goal", &statement.compiler).await?;
    verify_device_statement_for_signer(
        &mut transaction,
        request.proposal.controller,
        statement,
        &request.compilation.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    let principal_absent =
        sqlx::query_scalar::<_, bool>("SELECT NOT EXISTS (SELECT 1 FROM identities WHERE id=$1)")
            .bind(Uuid::from(request.proposal.proposed_agent))
            .fetch_one(&mut *transaction)
            .await?;
    if !principal_absent {
        return Err(AppError::Conflict);
    }
    let payload = json!({
        "proposal": request.proposal,
        "need_id": request.need_id,
        "compilation": request.compilation,
    });
    persist_governance_authorization_event(
        &mut transaction,
        project_id,
        GovernanceAuthorizationEvent {
            event_id: request.event_id,
            event_kind: "global_agent_proposal",
            workflow_id: request.event_id,
            workflow_revision: need.global_revision,
            actor_identity_id: actor.identity_id,
            user_identity_id: Some(Uuid::from(local.controller)),
            administrator_identity_id: Some(actor.identity_id),
            agent_id: None,
            source_draft_id: Some(statement.draft_id),
            review_task_id: None,
            local_goal_id: Some(Uuid::from(local.id)),
            local_goal_revision: Some(local.revision),
            global_contract_id: Some(request.global_contract_id),
            global_revision: Some(need.global_revision),
            obligation_id: Some(need.obligation),
            compilation_certificate_id: None,
            responsibility_compilation_id: None,
            idempotency_key: request.idempotency_key,
            payload: &payload,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: request.event_id,
        revision: need.global_revision,
        contract_hash_hex: hex::encode(canonical_hash(&request.proposal)?),
    }))
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
    let statement = &request.compilation.statement;
    if agent.controller_id != actor.identity_id.into()
        || statement.project_id != project_id
        || statement.agent_principal_identity_id != agent.principal_id
        || statement.controller_identity_id != agent.controller_id
        || statement.envelope.agent != agent.principal_id
        || statement.envelope.controller != agent.controller_id
    {
        return Err(AppError::Forbidden);
    }
    statement
        .output
        .validate_within_envelope(&statement.envelope)
        .map_err(agent_validation_error)?;
    let prompt_bytes = serialize_ciphertext(&request.encrypted_prompt)?;
    let ciphertext_commitment: [u8; 32] = Sha256::digest(&prompt_bytes).into();
    if decode_commitment(&statement.ciphertext_commitment_hex)? != ciphertext_commitment
        || decode_commitment(&statement.output_hash_hex)? != canonical_hash(&statement.output)?
        || decode_commitment(&statement.envelope_hash_hex)? != canonical_hash(&statement.envelope)?
    {
        return Err(AppError::BadRequest(
            "compiler artifact commitment mismatch",
        ));
    }
    let clauses = classify_local_goal_contract(&statement.output.contract);
    let classifier_output_hash = canonical_hash(&clauses)?;
    let origin = match &statement.authorization {
        LocalCompilationAuthorization::Responsibility { .. } => {
            LocalGoalOrigin::ControllerPrompt {}
        }
        LocalCompilationAuthorization::AdministratorException { .. }
        | LocalCompilationAuthorization::GlobalMandate { .. }
        | LocalCompilationAuthorization::AdministratorCreation { .. } => {
            return Err(AppError::BadRequest(
                "local-goal authorization adapter is not available",
            ));
        }
    };
    let contract = LocalGoalContract {
        id: statement.local_goal_id.into(),
        revision: statement.local_revision,
        agent: statement.agent_principal_identity_id,
        controller: statement.controller_identity_id,
        encrypted_prompt: request.encrypted_prompt,
        contract: statement.output.contract.clone(),
        clauses,
        origin,
        supersedes_revision: request.supersedes_revision,
    };
    contract.validate().map_err(agent_validation_error)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 36))")
        .bind(agent_id)
        .execute(&mut *transaction)
        .await?;
    let project_scope_contains_contract = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM resource_closure
            WHERE project_id = $1 AND ancestor_id = $2 AND descendant_id = $3)",
    )
    .bind(project_id)
    .bind(Uuid::from(statement.envelope.project_scope))
    .bind(Uuid::from(contract.contract.scope))
    .fetch_one(&mut *transaction)
    .await?;
    if !project_scope_contains_contract
        || !resource_access_in_transaction(
            &mut transaction,
            project_id,
            actor.identity_id,
            Uuid::from(contract.contract.scope),
            ResourceOperation::Read,
        )
        .await?
    {
        return Err(AppError::Forbidden);
    }
    validate_local_authorization(
        &mut transaction,
        project_id,
        actor,
        &contract,
        &statement.authorization,
    )
    .await?;
    let build_digest =
        require_pinned_compiler(&mut transaction, "local_goal", &statement.compiler).await?;
    let certificate_hash = verify_device_statement(
        &mut transaction,
        actor,
        statement,
        &request.compilation.signatures,
        COMPILATION_SIGNATURE_CONTEXT,
    )
    .await?;
    if contract.revision > 1 {
        let supersedes = contract.supersedes_revision.ok_or(AppError::Conflict)?;
        let previous_json = sqlx::query_scalar::<_, String>(
            r#"
            SELECT contract::text
            FROM agent_local_goal_contracts
            WHERE project_id = $1 AND id = $2 AND revision = $3 AND agent_id = $4
              AND state = 'active'
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(contract.id))
        .bind(to_i64(supersedes)?)
        .bind(agent_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let previous: LocalGoalContract =
            serde_json::from_str(&previous_json).map_err(|_| AppError::Internal)?;
        contract
            .validate_revision_of(&previous)
            .map_err(agent_validation_error)?;
    } else if contract.supersedes_revision.is_some() {
        return Err(AppError::BadRequest(
            "first local-goal revision cannot supersede another revision",
        ));
    }
    let (authorization_kind, authorization_id, authorization_revision) =
        local_authorization_columns(&statement.authorization);
    persist_compilation_certificate(
        &mut transaction,
        project_id,
        CompilationRecord {
            id: statement.certificate_id,
            task_kind: "local_goal",
            compiler: &statement.compiler,
            build_digest,
            signer: &request.compilation.signatures,
            subject_id: Uuid::from(contract.id),
            subject_revision: contract.revision,
            draft_id: statement.draft_id,
            agent_principal_identity_id: Some(Uuid::from(contract.agent)),
            controller_identity_id: Some(Uuid::from(contract.controller)),
            administrator_identity_id: None,
            user_identity_id: None,
            input_commitment: decode_commitment(&statement.prompt_commitment_hex)?,
            ciphertext_commitment,
            output_json: governance_canonical_json(&statement.output)?,
            output_hash: decode_commitment(&statement.output_hash_hex)?,
            envelope_json: governance_canonical_json(&statement.envelope)?,
            envelope_hash: decode_commitment(&statement.envelope_hash_hex)?,
            certificate_hash,
            idempotency_key: statement.idempotency_key,
            classifier_version: Some(LOCAL_GOAL_CLASSIFIER_VERSION),
            classifier_output_hash: Some(classifier_output_hash),
            authorization_kind,
            authorization_id,
            authorization_revision,
        },
    )
    .await?;
    let contract_json = canonical_json(&contract)?;
    let contract_hash: [u8; 32] = Sha256::digest(contract_json.as_bytes()).into();
    sqlx::query(
        r#"
        INSERT INTO agent_local_goal_contracts (
            id, project_id, agent_id, agent_identity_id,
            controller_identity_id, revision, contract, contract_hash, state,
            compilation_certificate_id, classifier_version,
            classifier_output_hash
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7::jsonb, $8, 'draft', $9, $10, $11
        )
        "#,
    )
    .bind(Uuid::from(contract.id))
    .bind(project_id)
    .bind(agent_id)
    .bind(Uuid::from(contract.agent))
    .bind(Uuid::from(contract.controller))
    .bind(to_i64(contract.revision)?)
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .bind(statement.certificate_id)
    .bind(to_i32(LOCAL_GOAL_CLASSIFIER_VERSION)?)
    .bind(classifier_output_hash.as_slice())
    .execute(&mut *transaction)
    .await?;
    append_verified_governance_revision(
        &mut transaction,
        project_id,
        "local_goal_revision",
        Uuid::from(contract.id),
        contract.revision,
        statement.certificate_id,
        contract_hash,
    )
    .await?;
    let prompt_hash = ciphertext_commitment;
    sqlx::query(
        r#"
        INSERT INTO agent_prompt_revisions (
            project_id, agent_id, draft_id, local_goal_id, local_goal_revision,
            encrypted_prompt, prompt_hash, state
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'draft')
        "#,
    )
    .bind(project_id)
    .bind(agent_id)
    .bind(statement.draft_id)
    .bind(Uuid::from(contract.id))
    .bind(to_i64(contract.revision)?)
    .bind(prompt_bytes)
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
            "local_goal_id": contract.id,
            "revision": contract.revision,
            "contract_hash": hex::encode(contract_hash),
            "state": "draft",
            "prompt_hash": hex::encode(prompt_hash),
            "compilation_certificate_id": statement.certificate_id,
            "classifier_version": LOCAL_GOAL_CLASSIFIER_VERSION,
        }),
    )
    .await?;
    append_user_governance_audit(
        &mut transaction,
        actor,
        project_id,
        Uuid::from(contract.controller),
        Some(agent_id),
        "local_goal_drafted",
        json!({
            "local_goal_id": contract.id,
            "revision": contract.revision,
            "contract_hash": hex::encode(contract_hash),
            "prompt_hash": hex::encode(prompt_hash),
            "compilation_certificate_id": statement.certificate_id,
            "classifier_output_hash": hex::encode(classifier_output_hash),
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ContractRecordedResponse {
        id: Uuid::from(contract.id),
        revision: contract.revision,
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
    encrypted_answer: Option<EncryptedPayload>,
    answered_at: Option<DateTime<Utc>>,
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
               interrogation.created_at, answer.encrypted_answer,
               answer.answered_at
        FROM agent_interrogations interrogation
        LEFT JOIN agent_interrogation_answers answer
          ON answer.project_id = interrogation.project_id
         AND answer.interrogation_id = interrogation.id
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
        encrypted_answer: row
            .try_get::<Option<Vec<u8>>, _>("encrypted_answer")?
            .map(|bytes| deserialize_ciphertext(&bytes))
            .transpose()?,
        answered_at: row.try_get("answered_at")?,
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
        let invocation = sqlx::query(
            r#"
            SELECT invocation.context_principal_identity_id,
                   invocation.proxy_request_id,
                   invocation.language_task::text AS language_task,
                   projection.structured_artifact::text AS structured_artifact
            FROM agent_invocations invocation
            JOIN agent_model_invocation_projections projection
              ON projection.project_id = invocation.project_id
             AND projection.invocation_id = invocation.id
            WHERE invocation.project_id = $1 AND invocation.id = $2
              AND invocation.status = 'succeeded'
              AND invocation.invocation_surface = 'user_proxy'
              AND projection.status = 'succeeded'
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(invocation_id))
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let task: StructuredLanguageTaskEnvelope =
            serde_json::from_str(invocation.try_get("language_task")?)
                .map_err(|_| AppError::Internal)?;
        let artifact: StructuredLanguageArtifact =
            serde_json::from_str(invocation.try_get("structured_artifact")?)
                .map_err(|_| AppError::Internal)?;
        let StructuredLanguageArtifact::UserProxyPlan { envelope, plan } = artifact else {
            return Err(AppError::Conflict);
        };
        if invocation.try_get::<Uuid, _>("context_principal_identity_id")? != actor.identity_id
            || invocation.try_get::<Option<Uuid>, _>("proxy_request_id")? != Some(request_id)
            || task.kind != StructuredLanguageTaskKind::InterpretProxyRequest
            || *envelope != request.envelope
            || plan != request.plan
        {
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationSurface {
    #[default]
    Generic,
    UserProxy,
    Interrogation,
    GovernanceSummary,
}

impl InvocationSurface {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::UserProxy => "user_proxy",
            Self::Interrogation => "interrogation",
            Self::GovernanceSummary => "governance_summary",
        }
    }
}

#[derive(Clone, Copy)]
struct InvocationSurfaceBinding {
    surface: InvocationSurface,
    proxy_request_id: Option<ProxyRequestId>,
    interrogation_id: Option<InterrogationId>,
    task_kind: StructuredLanguageTaskKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueInvocationRequest {
    id: InvocationId,
    local_goal_id: Option<Uuid>,
    local_goal_revision: Option<u64>,
    language_task: StructuredLanguageTaskEnvelope,
    authority_envelope: AuthorityEnvelope,
    sources: Vec<InformationSource>,
    encrypted_input: EncryptedPayload,
    #[serde(default)]
    surface: InvocationSurface,
    proxy_request_id: Option<ProxyRequestId>,
    interrogation_id: Option<InterrogationId>,
    work_binding: Option<ModelInvocationWorkBinding>,
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
    queue_invocation_for_runtime(state, actor, project_id, agent_id, request, "legacy_0031").await
}

pub async fn queue_client_provider_invocation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<QueueInvocationRequest>,
) -> Result<Json<QueueInvocationResponse>, AppError> {
    queue_invocation_for_runtime(
        state,
        actor,
        project_id,
        agent_id,
        request,
        "client_provider_v1",
    )
    .await
}

async fn queue_invocation_for_runtime(
    state: Arc<AppState>,
    actor: AuthSession,
    project_id: Uuid,
    agent_id: Uuid,
    request: QueueInvocationRequest,
    required_runtime_kind: &'static str,
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
    let context_principal = resolve_invocation_surface_context(
        &state,
        actor,
        project_id,
        agent_id,
        InvocationSurfaceBinding {
            surface: request.surface,
            proxy_request_id: request.proxy_request_id,
            interrogation_id: request.interrogation_id,
            task_kind: request.language_task.kind,
        },
    )
    .await?;
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
        if matches!(source, InformationSource::ToolOutput { .. }) {
            validate_tool_output_context_source(
                &state,
                actor,
                project_id,
                ToolOutputContextBinding {
                    producer_principal: agent.principal_id,
                    context_principal,
                    source,
                    runner_device_id: runner.device_id,
                    runner_key_version: runner.key_version,
                },
            )
            .await?;
            continue;
        }
        let resource_id = supported_source_resource(&state, actor, project_id, source).await?;
        if !resource_access_for_identity(
            &state,
            actor,
            project_id,
            Uuid::from(context_principal),
            resource_id,
            ResourceOperation::Read,
        )
        .await?
        {
            return Err(AppError::Forbidden);
        }
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
        principal: context_principal,
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
        "surface": request.surface,
        "proxy_request_id": request.proxy_request_id,
        "interrogation_id": request.interrogation_id,
        "work_binding": request.work_binding,
        "required_runtime_kind": required_runtime_kind,
    });
    let request_json = canonical_json(&request_projection)?;
    let request_hash: [u8; 32] = Sha256::digest(request_json.as_bytes()).into();
    let language_task_json = canonical_json(&request.language_task)?;
    let authority_json = canonical_json(&request.authority_envelope)?;
    let encrypted_input = serialize_ciphertext(&request.encrypted_input)?;
    let context_json = canonical_json(&context)?;
    let context_hash: [u8; 32] = Sha256::digest(context_json.as_bytes()).into();
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
    if let Some(binding) = &request.work_binding {
        validate_invocation_work_binding(&mut transaction, project_id, agent.principal_id, binding)
            .await?;
    }
    sqlx::query(
        r#"
        INSERT INTO agent_invocations (
            id, project_id, agent_id, agent_identity_id,
            local_goal_id, local_goal_revision, language_task,
            authority_envelope, encrypted_input, request_hash,
            max_attempts, created_by_identity_id,
            context_principal_identity_id, invocation_surface,
            proxy_request_id, interrogation_id,
            trace_id, run_id, goal_id, work_item_id, work_claim_id, work_attempt,
            context_hash, required_runtime_kind
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7::jsonb,
            $8::jsonb, $9, $10, $11, $12,
            $13, $14, $15, $16,
            $17, $18, $19, $20, $21, $22, $23, $24
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
    .bind(Uuid::from(context_principal))
    .bind(request.surface.as_str())
    .bind(request.proxy_request_id.map(Uuid::from))
    .bind(request.interrogation_id.map(Uuid::from))
    .bind(
        request
            .work_binding
            .as_ref()
            .map(|binding| binding.trace_id),
    )
    .bind(
        request
            .work_binding
            .as_ref()
            .map(|binding| Uuid::from(binding.run)),
    )
    .bind(
        request
            .work_binding
            .as_ref()
            .map(|binding| Uuid::from(binding.goal)),
    )
    .bind(
        request
            .work_binding
            .as_ref()
            .map(|binding| Uuid::from(binding.work)),
    )
    .bind(
        request
            .work_binding
            .as_ref()
            .map(|binding| Uuid::from(binding.claim)),
    )
    .bind(
        request
            .work_binding
            .as_ref()
            .map(|binding| to_i32(binding.attempt))
            .transpose()?,
    )
    .bind(context_hash.as_slice())
    .bind(required_runtime_kind)
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
    dispatch_id: Uuid,
    lease_id: Uuid,
    lease_expires_at: DateTime<Utc>,
    attempt: i32,
    language_task: StructuredLanguageTaskEnvelope,
    authority_envelope: AuthorityEnvelope,
    sources: Vec<InformationSource>,
    encrypted_input: EncryptedPayload,
    context_principal_identity_id: UserId,
    request_commitment_hex: String,
    context_commitment_hex: String,
    transport_commitment_hex: String,
    runtime_kind: &'static str,
}

pub async fn claim_invocation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Option<ClaimedInvocationResponse>>, AppError> {
    claim_invocation_for_runtime(state, actor, project_id, agent_id, "legacy_0031", None).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientProviderClaimRequest {
    execution_profile_commitment_hex: String,
}

pub async fn claim_client_provider_invocation(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ClientProviderClaimRequest>,
) -> Result<Json<Option<ClaimedInvocationResponse>>, AppError> {
    let execution_profile_commitment =
        decode_commitment(&request.execution_profile_commitment_hex)?;
    claim_invocation_for_runtime(
        state,
        actor,
        project_id,
        agent_id,
        "client_provider_v1",
        Some(execution_profile_commitment),
    )
    .await
}

async fn claim_invocation_for_runtime(
    state: Arc<AppState>,
    actor: AuthSession,
    project_id: Uuid,
    agent_id: Uuid,
    runtime_kind: &'static str,
    execution_profile_commitment: Option<[u8; 32]>,
) -> Result<Json<Option<ClaimedInvocationResponse>>, AppError> {
    if (runtime_kind == "client_provider_v1") != execution_profile_commitment.is_some() {
        return Err(AppError::Conflict);
    }
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let runner = authenticated_runner(&state, actor, project_id, agent_id).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let candidate = sqlx::query(
        r#"
        SELECT id, language_task::text AS language_task,
               authority_envelope::text AS authority_envelope,
               encrypted_input, attempt, context_principal_identity_id,
               request_hash
        FROM agent_invocations
        WHERE project_id = $1 AND agent_id = $2
          AND required_runtime_kind = $3
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
    .bind(runtime_kind)
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
    let dispatch_id = Uuid::new_v4();
    let dispatched_at = Utc::now();
    let lease_expires_at =
        dispatched_at + chrono::Duration::from_std(RUNNER_LEASE).map_err(|_| AppError::Internal)?;
    let attempt: i32 = candidate.try_get::<i32, _>("attempt")? + 1;
    let context_principal_identity_id: Uuid = candidate.try_get("context_principal_identity_id")?;
    let source_json = governance_canonical_json(&sources)?;
    let context_commitment: [u8; 32] = Sha256::digest(source_json.as_bytes()).into();
    let request_commitment: Vec<u8> = candidate.try_get("request_hash")?;
    let transport_projection = json!({
        "dispatch_id": dispatch_id,
        "invocation_id": invocation_id,
        "attempt": attempt,
        "lease_id": lease_id,
        "context_principal_identity_id": context_principal_identity_id,
        "language_task": serde_json::from_str::<Value>(candidate.try_get("language_task")?)
            .map_err(|_| AppError::Internal)?,
        "authority_envelope": serde_json::from_str::<Value>(
            candidate.try_get("authority_envelope")?,
        )
        .map_err(|_| AppError::Internal)?,
        "sources": sources,
        "encrypted_input": deserialize_ciphertext(candidate.try_get("encrypted_input")?)?,
        "request_commitment_hex": hex::encode(&request_commitment),
        "context_commitment_hex": hex::encode(context_commitment),
        "runtime_kind": runtime_kind,
        "execution_profile_commitment_hex": execution_profile_commitment.map(hex::encode),
    });
    let transport_commitment: [u8; 32] =
        Sha256::digest(governance_canonical_bytes(&transport_projection)?).into();
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
    sqlx::query(
        r#"
        INSERT INTO agent_model_attempt_dispatches (
            id, project_id, invocation_id, attempt, lease_id, runner_id,
            runner_identity_id, runner_device_id, runner_key_version,
            context_principal_identity_id, request_commitment,
            context_commitment, exposure_commitment, transport_commitment,
            source_descriptors, dispatched_at, lease_expires_at, runtime_kind,
            execution_profile_commitment
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
            $11, $12, $12, $13, $14::jsonb, $15, $16, $17, $18
        )
        "#,
    )
    .bind(dispatch_id)
    .bind(project_id)
    .bind(invocation_id)
    .bind(attempt)
    .bind(lease_id)
    .bind(runner.id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(runner.key_version)
    .bind(context_principal_identity_id)
    .bind(&request_commitment)
    .bind(context_commitment.as_slice())
    .bind(transport_commitment.as_slice())
    .bind(source_json)
    .bind(dispatched_at)
    .bind(lease_expires_at)
    .bind(runtime_kind)
    .bind(
        execution_profile_commitment
            .as_ref()
            .map(<[u8; 32]>::as_slice),
    )
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
        dispatch_id,
        lease_id,
        lease_expires_at,
        attempt,
        language_task,
        authority_envelope,
        sources,
        encrypted_input,
        context_principal_identity_id: context_principal_identity_id.into(),
        request_commitment_hex: hex::encode(request_commitment),
        context_commitment_hex: hex::encode(context_commitment),
        transport_commitment_hex: hex::encode(transport_commitment),
        runtime_kind,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointObservationStatement {
    observation_id: Uuid,
    dispatch_id: Uuid,
    invocation_id: InvocationId,
    attempt: u32,
    lease_id: Uuid,
    principal_identity_id: UserId,
    exposed_sources: Vec<InformationSource>,
    request_commitment_hex: String,
    context_commitment_hex: String,
    transport_commitment_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoint_request_commitment_hex: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    endpoint_request_exact: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_profile_commitment_hex: Option<String>,
    output_commitment_hex: Option<String>,
    artifact_commitment_hex: Option<String>,
    provider_status: String,
    hidden_persistent_model_memory_available: bool,
    idempotency_key: Uuid,
    observed_at: DateTime<Utc>,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedModelEndpointObservation {
    statement: ModelEndpointObservationStatement,
    signatures: CompilationSignatures,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitInvocationRequest {
    lease_id: Uuid,
    structured_output: StructuredLanguageOutput,
    encrypted_output: EncryptedPayload,
    effects: Vec<EffectProposalRequest>,
    artifact: Option<StructuredLanguageArtifact>,
    observation: Option<SignedModelEndpointObservation>,
    #[serde(default)]
    endpoint_request_commitment_hex: Option<String>,
    #[serde(default)]
    endpoint_request_exact: bool,
    #[serde(default)]
    runtime_kind: Option<String>,
    #[serde(default)]
    execution_profile_commitment_hex: Option<String>,
}

#[derive(Serialize)]
pub struct SubmitInvocationResponse {
    id: InvocationId,
    status: &'static str,
    accepted_effect_ids: Vec<Uuid>,
    output_hash_hex: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailInvocationRequest {
    lease_id: Uuid,
    failure_code: RunnerFailureCode,
    retryable: bool,
    observation: Option<SignedModelEndpointObservation>,
    #[serde(default)]
    endpoint_request_commitment_hex: Option<String>,
    #[serde(default)]
    endpoint_request_exact: bool,
    #[serde(default)]
    runtime_kind: Option<String>,
    #[serde(default)]
    execution_profile_commitment_hex: Option<String>,
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

    const fn requires_endpoint_request_witness(self) -> bool {
        matches!(
            self,
            Self::ProviderUnavailable | Self::ProviderTimeout | Self::InvalidStructuredOutput
        )
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
               authority_envelope::text AS authority_envelope,
               attempt, context_principal_identity_id, invocation_surface,
               proxy_request_id, interrogation_id,
               trace_id, run_id, goal_id, work_item_id, work_claim_id, work_attempt,
               request_hash, context_hash
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
    .await?;
    let Some(row) = row else {
        if let Some(observation) = &request.observation {
            let observation_hash: [u8; 32] =
                Sha256::digest(governance_canonical_bytes(&observation.statement)?).into();
            let replay = sqlx::query(
                r#"
                SELECT invocation.output_hash
                FROM agent_invocations invocation
                JOIN agent_model_attempt_observations observed
                  ON observed.project_id = invocation.project_id
                 AND observed.invocation_id = invocation.id
                WHERE invocation.project_id = $1 AND invocation.id = $2
                  AND invocation.agent_id = $3 AND invocation.status = 'succeeded'
                  AND observed.id = $4 AND observed.idempotency_key = $5
                  AND observed.observation_hash = $6 AND observed.status = 'succeeded'
                "#,
            )
            .bind(project_id)
            .bind(invocation_id)
            .bind(agent_id)
            .bind(observation.statement.observation_id)
            .bind(observation.statement.idempotency_key)
            .bind(observation_hash.as_slice())
            .fetch_optional(&mut *transaction)
            .await?;
            if let Some(replay) = replay {
                let effect_ids = sqlx::query_scalar::<_, Uuid>(
                    "SELECT id FROM agent_effect_proposals
                     WHERE project_id = $1 AND invocation_id = $2 ORDER BY ordinal",
                )
                .bind(project_id)
                .bind(invocation_id)
                .fetch_all(&mut *transaction)
                .await?;
                let output_hash: Vec<u8> = replay.try_get("output_hash")?;
                transaction.commit().await?;
                return Ok(Json(SubmitInvocationResponse {
                    id: InvocationId::from(invocation_id),
                    status: "succeeded",
                    accepted_effect_ids: effect_ids,
                    output_hash_hex: hex::encode(output_hash),
                }));
            }
        }
        return Err(AppError::Conflict);
    };
    let language_task: StructuredLanguageTaskEnvelope =
        serde_json::from_str(row.try_get("language_task")?).map_err(|_| AppError::Internal)?;
    language_task
        .validate_grounded_output(&request.structured_output)
        .map_err(agent_validation_error)?;
    let artifact =
        request
            .artifact
            .clone()
            .unwrap_or_else(|| StructuredLanguageArtifact::GroundedOutput {
                output: request.structured_output.clone(),
            });
    artifact
        .validate_for(&language_task)
        .map_err(agent_validation_error)?;
    let invocation_surface: String = row.try_get("invocation_surface")?;
    if invocation_surface != "generic" && request.observation.is_none() {
        return Err(AppError::BadRequest(
            "language surface requires a signed endpoint observation",
        ));
    }
    if invocation_surface == "interrogation" && !request.effects.is_empty() {
        return Err(AppError::BadRequest(
            "interrogation language invocation is answer-only",
        ));
    }
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
    let context_principal_identity_id: Uuid = row.try_get("context_principal_identity_id")?;
    let mut source_audiences = Vec::with_capacity(sources.len());
    for source in &sources {
        let resource_id = source_resource(source).ok_or(AppError::Forbidden)?;
        if !resource_access_in_transaction(
            &mut transaction,
            project_id,
            context_principal_identity_id,
            resource_id,
            ResourceOperation::Read,
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
    let artifact_json = governance_canonical_json(&artifact)?;
    let artifact_hash: [u8; 32] = Sha256::digest(artifact_json.as_bytes()).into();
    let projected_endpoint_request_commitment = request
        .endpoint_request_commitment_hex
        .as_deref()
        .map(decode_commitment)
        .transpose()?;
    let projected_execution_profile_commitment = request
        .execution_profile_commitment_hex
        .as_deref()
        .map(decode_commitment)
        .transpose()?;
    if request.endpoint_request_exact != projected_endpoint_request_commitment.is_some() {
        return Err(AppError::Conflict);
    }

    let verified_observation = if let Some(observation) = &request.observation {
        let dispatch = sqlx::query(
            r#"
            SELECT id, attempt, lease_id, runner_identity_id, runner_device_id,
                   runner_key_version, context_principal_identity_id,
                   request_commitment, context_commitment, exposure_commitment,
                   transport_commitment, source_descriptors::text AS source_descriptors,
                   dispatched_at, lease_expires_at, runtime_kind,
                   execution_profile_commitment
            FROM agent_model_attempt_dispatches
            WHERE project_id = $1 AND id = $2 AND invocation_id = $3
              AND lease_id = $4 AND attempt = $5
            "#,
        )
        .bind(project_id)
        .bind(observation.statement.dispatch_id)
        .bind(invocation_id)
        .bind(request.lease_id)
        .bind(row.try_get::<i32, _>("attempt")?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Conflict)?;
        let sources_json: String = dispatch.try_get("source_descriptors")?;
        let dispatched_sources: Vec<InformationSource> =
            serde_json::from_str(&sources_json).map_err(|_| AppError::Internal)?;
        let request_commitment: Vec<u8> = dispatch.try_get("request_commitment")?;
        let context_commitment: Vec<u8> = dispatch.try_get("context_commitment")?;
        let transport_commitment: Vec<u8> = dispatch.try_get("transport_commitment")?;
        let observed_request = decode_commitment(&observation.statement.request_commitment_hex)?;
        let observed_context = decode_commitment(&observation.statement.context_commitment_hex)?;
        let observed_transport =
            decode_commitment(&observation.statement.transport_commitment_hex)?;
        let endpoint_request_commitment = observation
            .statement
            .endpoint_request_commitment_hex
            .as_deref()
            .map(decode_commitment)
            .transpose()?;
        let observed_execution_profile_commitment = observation
            .statement
            .execution_profile_commitment_hex
            .as_deref()
            .map(decode_commitment)
            .transpose()?;
        let observed_output = decode_commitment(
            observation
                .statement
                .output_commitment_hex
                .as_deref()
                .ok_or(AppError::Conflict)?,
        )?;
        let observed_artifact = decode_commitment(
            observation
                .statement
                .artifact_commitment_hex
                .as_deref()
                .ok_or(AppError::Conflict)?,
        )?;
        let dispatched_at: DateTime<Utc> = dispatch.try_get("dispatched_at")?;
        let expires_at: DateTime<Utc> = dispatch.try_get("lease_expires_at")?;
        let runtime_kind: String = dispatch.try_get("runtime_kind")?;
        let dispatched_execution_profile_commitment: Option<Vec<u8>> =
            dispatch.try_get("execution_profile_commitment")?;
        if (runtime_kind == "client_provider_v1"
            && (!request.endpoint_request_exact
                || projected_endpoint_request_commitment.is_none()
                || projected_execution_profile_commitment.is_none()
                || request.runtime_kind.as_deref() != Some("client_provider_v1")))
            || (runtime_kind == "legacy_0031"
                && (request.endpoint_request_exact
                    || projected_endpoint_request_commitment.is_some()
                    || projected_execution_profile_commitment.is_some()
                    || request.runtime_kind.is_some()))
        {
            return Err(AppError::Conflict);
        }
        if dispatched_execution_profile_commitment.as_deref()
            != projected_execution_profile_commitment
                .as_ref()
                .map(<[u8; 32]>::as_slice)
        {
            return Err(AppError::Conflict);
        }
        let attempt =
            u32::try_from(row.try_get::<i32, _>("attempt")?).map_err(|_| AppError::Internal)?;
        let context_principal: Uuid = row.try_get("context_principal_identity_id")?;
        if observation.statement.invocation_id != InvocationId::from(invocation_id)
            || observation.statement.attempt != attempt
            || observation.statement.lease_id != request.lease_id
            || observation.statement.principal_identity_id != UserId::from(context_principal)
            || observation.statement.exposed_sources != dispatched_sources
            || observed_request.as_slice() != request_commitment.as_slice()
            || observed_context.as_slice() != context_commitment.as_slice()
            || observed_transport.as_slice() != transport_commitment.as_slice()
            || observation.statement.endpoint_request_exact != request.endpoint_request_exact
            || endpoint_request_commitment != projected_endpoint_request_commitment
            || observation.statement.runtime_kind.as_deref() != request.runtime_kind.as_deref()
            || observed_execution_profile_commitment != projected_execution_profile_commitment
            || observed_output != output_hash
            || observed_artifact != artifact_hash
            || observation
                .statement
                .hidden_persistent_model_memory_available
            || observation.statement.provider_status.trim().is_empty()
            || observation.statement.provider_status.len() > 128
            || observation.statement.observed_at < dispatched_at
            || observation.statement.observed_at >= expires_at
            || observation.signatures.signer_identity_id != actor.identity_id.into()
            || observation.signatures.signer_device_id != actor.device_id
            || to_i32(observation.signatures.signer_device_key_version)? != runner.key_version
        {
            return Err(AppError::Conflict);
        }
        verify_device_statement_for_signer(
            &mut transaction,
            actor.identity_id.into(),
            &observation.statement,
            &observation.signatures,
            MODEL_RUNTIME_OBSERVATION_SIGNATURE_CONTEXT,
        )
        .await?;
        let actual = ModelRuntimeActualObservation {
            invocation_id: InvocationId::from(invocation_id),
            attempt,
            principal: context_principal.into(),
            exposed_sources: dispatched_sources.clone(),
            request_commitment: observed_request,
            output_commitment: Some(observed_output),
            explicit_failure: false,
            hidden_persistent_model_memory_available: false,
        };
        let projection = R540ModelRuntimeProjection {
            invocation_id: InvocationId::from(invocation_id),
            attempt,
            principal: context_principal.into(),
            context_sources: sources.clone(),
            request_commitment: request_commitment
                .as_slice()
                .try_into()
                .map_err(|_| AppError::Internal)?,
            output_commitment: Some(output_hash),
            explicit_failure: false,
        };
        validate_model_runtime_projection(&actual, &projection).map_err(agent_validation_error)?;
        Some((
            observation,
            dispatched_sources,
            request_commitment,
            context_commitment,
            transport_commitment,
            projected_endpoint_request_commitment,
            projected_execution_profile_commitment,
            runtime_kind,
            context_principal,
            dispatched_at,
        ))
    } else {
        None
    };
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
        sqlx::query(
            "INSERT INTO agent_language_causal_mutations (
                 id, project_id, invocation_id, category, record_id
             ) VALUES ($1, $2, $3, 'resource_effect', $4)",
        )
        .bind(derived_governance_id(
            b"language-resource-effect",
            &[project_id, invocation_id, proposal.id],
        ))
        .bind(project_id)
        .bind(invocation_id)
        .bind(proposal.id)
        .execute(&mut *transaction)
        .await?;
    }
    if let Some((
        observation,
        dispatched_sources,
        request_commitment,
        context_commitment,
        transport_commitment,
        projected_endpoint_request_commitment,
        projected_execution_profile_commitment,
        runtime_kind,
        context_principal,
        dispatched_at,
    )) = verified_observation
    {
        if let Some(run_id) = row.try_get::<Option<Uuid>, _>("run_id")? {
            let binding = ModelInvocationWorkBinding {
                trace_id: row
                    .try_get::<Option<Uuid>, _>("trace_id")?
                    .ok_or(AppError::Internal)?,
                run: run_id.into(),
                goal: row
                    .try_get::<Option<Uuid>, _>("goal_id")?
                    .ok_or(AppError::Internal)?
                    .into(),
                work: row
                    .try_get::<Option<Uuid>, _>("work_item_id")?
                    .ok_or(AppError::Internal)?
                    .into(),
                claim: row
                    .try_get::<Option<Uuid>, _>("work_claim_id")?
                    .ok_or(AppError::Internal)?
                    .into(),
                attempt: u32::try_from(
                    row.try_get::<Option<i32>, _>("work_attempt")?
                        .ok_or(AppError::Internal)?,
                )
                .map_err(|_| AppError::Internal)?,
            };
            validate_invocation_work_binding(
                &mut transaction,
                project_id,
                actor.identity_id.into(),
                &binding,
            )
            .await?;
        }
        let observation_hash = governance_canonical_bytes(&observation.statement)?;
        let observation_hash: [u8; 32] = Sha256::digest(observation_hash).into();
        sqlx::query(
            r#"
            INSERT INTO agent_model_attempt_observations (
                id, project_id, dispatch_id, invocation_id, attempt, lease_id,
                principal_identity_id, status, provider_status,
                request_commitment, context_commitment, exposure_commitment,
                endpoint_request_exact, endpoint_request_commitment,
                runtime_kind, execution_profile_commitment,
                output_commitment, artifact_commitment, structured_artifact,
                transport_commitment, exposed_source_descriptors,
                hidden_persistent_model_memory_available,
                signer_identity_id, signer_device_id, signer_key_version,
                classical_signature, post_quantum_signature,
                observation_hash, idempotency_key, observed_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, 'succeeded', $8,
                $9, $10, $10, $11, $12, $13, $14,
                $15, $16, $17::jsonb, $18, $19::jsonb,
                false, $20, $21, $22, $23, $24, $25, $26, $27
            )
            "#,
        )
        .bind(observation.statement.observation_id)
        .bind(project_id)
        .bind(observation.statement.dispatch_id)
        .bind(invocation_id)
        .bind(to_i32(observation.statement.attempt)?)
        .bind(request.lease_id)
        .bind(context_principal)
        .bind(&observation.statement.provider_status)
        .bind(&request_commitment)
        .bind(&context_commitment)
        .bind(request.endpoint_request_exact)
        .bind(
            projected_endpoint_request_commitment
                .as_ref()
                .map(<[u8; 32]>::as_slice),
        )
        .bind(&runtime_kind)
        .bind(
            projected_execution_profile_commitment
                .as_ref()
                .map(<[u8; 32]>::as_slice),
        )
        .bind(output_hash.as_slice())
        .bind(artifact_hash.as_slice())
        .bind(&artifact_json)
        .bind(&transport_commitment)
        .bind(governance_canonical_json(&dispatched_sources)?)
        .bind(Uuid::from(observation.signatures.signer_identity_id))
        .bind(observation.signatures.signer_device_id)
        .bind(to_i32(observation.signatures.signer_device_key_version)?)
        .bind(&observation.signatures.classical_signature)
        .bind(&observation.signatures.post_quantum_signature)
        .bind(observation_hash.as_slice())
        .bind(observation.statement.idempotency_key)
        .bind(observation.statement.observed_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO agent_model_invocation_projections (
                id, project_id, invocation_id, observation_id, provider_attempt,
                trace_id, run_id, goal_id, work_item_id, work_claim_id, work_attempt,
                principal_identity_id, status, invocation_surface, language_task,
                context_source_descriptors, request_commitment, context_commitment,
                endpoint_request_exact, endpoint_request_commitment,
                runtime_kind, execution_profile_commitment, output_commitment,
                artifact_commitment, structured_artifact, invoked_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                $12, 'succeeded', $13, $14::jsonb, $15::jsonb, $16, $17,
                $18, $19, $20, $21, $22, $23, $24::jsonb, $25
            )
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(invocation_id)
        .bind(observation.statement.observation_id)
        .bind(to_i32(observation.statement.attempt)?)
        .bind(row.try_get::<Option<Uuid>, _>("trace_id")?)
        .bind(row.try_get::<Option<Uuid>, _>("run_id")?)
        .bind(row.try_get::<Option<Uuid>, _>("goal_id")?)
        .bind(row.try_get::<Option<Uuid>, _>("work_item_id")?)
        .bind(row.try_get::<Option<Uuid>, _>("work_claim_id")?)
        .bind(row.try_get::<Option<i32>, _>("work_attempt")?)
        .bind(context_principal)
        .bind(&invocation_surface)
        .bind(canonical_json(&language_task)?)
        .bind(governance_canonical_json(&dispatched_sources)?)
        .bind(&request_commitment)
        .bind(&context_commitment)
        .bind(request.endpoint_request_exact)
        .bind(
            projected_endpoint_request_commitment
                .as_ref()
                .map(<[u8; 32]>::as_slice),
        )
        .bind(&runtime_kind)
        .bind(
            projected_execution_profile_commitment
                .as_ref()
                .map(<[u8; 32]>::as_slice),
        )
        .bind(output_hash.as_slice())
        .bind(artifact_hash.as_slice())
        .bind(&artifact_json)
        .bind(dispatched_at)
        .execute(&mut *transaction)
        .await?;
        if invocation_surface == "interrogation" {
            persist_interrogation_answer(
                &mut transaction,
                project_id,
                invocation_id,
                row.try_get::<Option<Uuid>, _>("interrogation_id")?
                    .ok_or(AppError::Internal)?,
                &artifact,
                &dispatched_sources,
                observation.statement.observed_at,
            )
            .await?;
        }
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
    let attempts = sqlx::query(
        r#"
        SELECT attempt, max_attempts, invocation_surface,
               context_principal_identity_id, language_task::text AS language_task,
               trace_id, run_id, goal_id, work_item_id, work_claim_id, work_attempt
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
    let attempt: i32 = attempts.try_get("attempt")?;
    let max_attempts: i32 = attempts.try_get("max_attempts")?;
    let invocation_surface: String = attempts.try_get("invocation_surface")?;
    let projected_endpoint_request_commitment = request
        .endpoint_request_commitment_hex
        .as_deref()
        .map(decode_commitment)
        .transpose()?;
    let projected_execution_profile_commitment = request
        .execution_profile_commitment_hex
        .as_deref()
        .map(decode_commitment)
        .transpose()?;
    if request.endpoint_request_exact != projected_endpoint_request_commitment.is_some() {
        return Err(AppError::Conflict);
    }
    if invocation_surface != "generic" && request.observation.is_none() {
        return Err(AppError::BadRequest(
            "language surface failure requires a signed endpoint observation",
        ));
    }
    if let Some(observation) = &request.observation {
        persist_signed_failure_observation(
            &mut transaction,
            actor,
            project_id,
            agent_id,
            invocation_id,
            runner.id,
            runner.key_version,
            request.lease_id,
            request.failure_code,
            request.endpoint_request_exact,
            projected_endpoint_request_commitment,
            request.runtime_kind.as_deref(),
            projected_execution_profile_commitment,
            observation,
            &attempts,
        )
        .await?;
    }
    let exhausted = !request.retryable || attempt >= max_attempts;
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
            "attempt": attempt,
            "max_attempts": max_attempts,
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

struct ToolOutputContextBinding<'a> {
    producer_principal: UserId,
    context_principal: UserId,
    source: &'a InformationSource,
    runner_device_id: Uuid,
    runner_key_version: i32,
}

async fn validate_tool_output_context_source(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    binding: ToolOutputContextBinding<'_>,
) -> Result<(), AppError> {
    let InformationSource::ToolOutput { call_id } = binding.source else {
        return Err(AppError::Internal);
    };
    let mut transaction = begin(state, actor, project_id).await?;
    let exact = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM agent_tool_calls call
            JOIN agent_tool_attempt_dispatches dispatch
              ON dispatch.project_id = call.project_id
             AND dispatch.call_id = call.id
             AND dispatch.attempt = call.current_attempt
            JOIN agent_tool_attempt_observations observation
              ON observation.project_id = call.project_id
             AND observation.call_id = call.id
             AND observation.dispatch_id = dispatch.id
             AND observation.attempt = call.current_attempt
            JOIN agent_tool_output_key_envelopes envelope
              ON envelope.project_id = observation.project_id
             AND envelope.observation_id = observation.id
             AND envelope.call_id = call.id
            JOIN agent_external_tool_catalog catalog
              ON catalog.tool_name = call.tool_name
             AND catalog.version = call.tool_version
             AND catalog.output_audience_kind = 'owner_from_canonical_input'
            JOIN agent_run_work_slots slot
              ON slot.project_id = call.project_id
             AND slot.run_id = call.run_id
             AND slot.work_item_id = call.work_item_id
             AND slot.work_spec_ordinal = call.work_spec_ordinal
            JOIN agent_run_claim_leases claim
              ON claim.project_id = call.project_id
             AND claim.id = call.work_claim_id
             AND claim.run_id = call.run_id
             AND claim.work_item_id = call.work_item_id
             AND claim.attempt = call.work_attempt
             AND claim.claimant_identity_id = call.owner_identity_id
            JOIN agent_run_external_tool_work_outcomes outcome
              ON outcome.project_id = call.project_id
             AND outcome.run_id = call.run_id
             AND outcome.work_item_id = call.work_item_id
             AND outcome.claim_id = call.work_claim_id
             AND outcome.attempt = call.current_attempt
             AND outcome.observation_id = observation.id
             AND outcome.work_status = 'succeeded'
            JOIN device_keys key
              ON key.identity_id = envelope.recipient_identity_id
             AND key.device_id = envelope.recipient_device_id
             AND key.key_version = envelope.recipient_device_key_version
            JOIN devices device
              ON device.identity_id = key.identity_id AND device.id = key.device_id
            WHERE call.project_id = $1 AND call.id = $2
              AND call.owner_identity_id = $3
              AND call.current_status = 'succeeded'
              AND call.output_readable_by = jsonb_build_array(call.owner_identity_id)
              AND call.output_readable_by @> jsonb_build_array($4::uuid)
              AND observation.terminal_status = 'succeeded'
              AND observation.canonical_output_commitment IS NOT NULL
              AND observation.canonical_output_commitment = call.current_output_commitment
              AND observation.encrypted_output_payload_commitment = digest(observation.encrypted_output, 'sha256')
              AND observation.output_readable_by = call.output_readable_by
              AND dispatch.runner_identity_id = call.owner_identity_id
              AND dispatch.attempt = call.work_attempt
              AND dispatch.canonical_input_commitment = call.canonical_input_commitment
              AND dispatch.execution_profile_commitment = observation.execution_profile_commitment
              AND claim.acquired_at <= call.requested_at
              AND call.requested_at < claim.expires_at
              AND dispatch.dispatched_at < claim.expires_at
              AND envelope.recipient_identity_id = $4
              AND envelope.recipient_device_id = $5
              AND envelope.recipient_device_key_version = $6
              AND envelope.envelope_commitment = digest(envelope.encrypted_key, 'sha256')
              AND key.revoked_at IS NULL
              AND device.trust_state = 'trusted'
              AND device.retired_at IS NULL
        )
        "#,
    )
    .bind(project_id)
    .bind(*call_id)
    .bind(Uuid::from(binding.producer_principal))
    .bind(Uuid::from(binding.context_principal))
    .bind(binding.runner_device_id)
    .bind(binding.runner_key_version)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if exact {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
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

async fn resolve_invocation_surface_context(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    agent_id: Uuid,
    binding: InvocationSurfaceBinding,
) -> Result<UserId, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let principal = match binding.surface {
        InvocationSurface::Generic => {
            if binding.proxy_request_id.is_some() || binding.interrogation_id.is_some() {
                return Err(AppError::BadRequest(
                    "generic invocation has surface reference",
                ));
            }
            sqlx::query_scalar::<_, Uuid>(
                "SELECT principal_identity_id FROM governed_agents
                 WHERE project_id = $1 AND id = $2 AND state = 'active'",
            )
            .bind(project_id)
            .bind(agent_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AppError::NotFound)?
        }
        InvocationSurface::UserProxy => {
            if binding.task_kind != StructuredLanguageTaskKind::InterpretProxyRequest
                || binding.interrogation_id.is_some()
            {
                return Err(AppError::BadRequest("invalid proxy language task binding"));
            }
            let request_id = binding.proxy_request_id.ok_or(AppError::BadRequest(
                "proxy invocation requires exact request",
            ))?;
            sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT request.user_identity_id
                FROM user_proxy_requests request
                JOIN user_proxy_threads thread
                  ON thread.project_id = request.project_id
                 AND thread.id = request.thread_id
                WHERE request.project_id = $1 AND request.id = $2
                  AND request.user_identity_id = $3 AND thread.closed_at IS NULL
                "#,
            )
            .bind(project_id)
            .bind(Uuid::from(request_id))
            .bind(actor.identity_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AppError::NotFound)?
        }
        InvocationSurface::Interrogation => {
            if binding.task_kind != StructuredLanguageTaskKind::AnswerFromAuthorizedContext
                || binding.proxy_request_id.is_some()
            {
                return Err(AppError::BadRequest(
                    "invalid interrogation language task binding",
                ));
            }
            let session_id = binding.interrogation_id.ok_or(AppError::BadRequest(
                "interrogation invocation requires exact session",
            ))?;
            sqlx::query_scalar::<_, Uuid>(
                "SELECT creator_identity_id FROM agent_interrogations
                 WHERE project_id = $1 AND id = $2 AND target_agent_id = $3
                   AND creator_identity_id = $4
                   AND NOT EXISTS (
                       SELECT 1 FROM agent_interrogation_answers answer
                       WHERE answer.project_id = agent_interrogations.project_id
                         AND answer.interrogation_id = agent_interrogations.id
                   )",
            )
            .bind(project_id)
            .bind(Uuid::from(session_id))
            .bind(agent_id)
            .bind(actor.identity_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AppError::NotFound)?
        }
        InvocationSurface::GovernanceSummary => {
            if binding.task_kind != StructuredLanguageTaskKind::SummarizeGovernanceDecision
                || binding.proxy_request_id.is_some()
                || binding.interrogation_id.is_some()
            {
                return Err(AppError::BadRequest("invalid governance summary binding"));
            }
            actor.identity_id
        }
    };
    transaction.commit().await?;
    Ok(principal.into())
}

async fn validate_invocation_work_binding(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    agent_principal: UserId,
    binding: &ModelInvocationWorkBinding,
) -> Result<(), AppError> {
    let valid = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM agent_collaborative_runs run
            JOIN agent_run_work_slots slot
              ON slot.project_id = run.project_id AND slot.run_id = run.id
            JOIN agent_run_claim_leases claim
              ON claim.project_id = slot.project_id
             AND claim.run_id = slot.run_id
             AND claim.work_item_id = slot.work_item_id
            WHERE run.project_id = $1 AND run.id = $2 AND run.goal_id = $3
              AND slot.work_item_id = $4 AND claim.id = $5
              AND claim.attempt = $6 AND claim.claimant_identity_id = $7
              AND claim.status = 'active'
              AND claim.acquired_at <= clock_timestamp()
              AND clock_timestamp() < claim.expires_at
              AND run.goal_status = 'active' AND run.run_status = 'running'
        )
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(binding.run))
    .bind(Uuid::from(binding.goal))
    .bind(Uuid::from(binding.work))
    .bind(Uuid::from(binding.claim))
    .bind(to_i32(binding.attempt)?)
    .bind(Uuid::from(agent_principal))
    .fetch_one(&mut **transaction)
    .await?;
    if valid {
        Ok(())
    } else {
        Err(AppError::Conflict)
    }
}

async fn interrogation_read_only_fingerprint(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
) -> Result<[u8; 32], AppError> {
    let snapshot = sqlx::query_scalar::<_, String>(
        r#"
        SELECT jsonb_build_object(
            'resource_effects', (SELECT count(*) FROM agent_effect_proposals
                                 WHERE project_id = $1),
            'prompt_revisions', (SELECT count(*) FROM agent_prompt_revisions
                                 WHERE project_id = $1),
            'local_goal_revisions', (SELECT count(*) FROM agent_local_goal_contracts
                                     WHERE project_id = $1),
            'work_items', (SELECT count(*) FROM agent_run_work_slots
                           WHERE project_id = $1),
            'assigned_tasks', (SELECT count(*) FROM task_assignments
                               WHERE project_id = $1),
            'run_states', COALESCE((
                SELECT jsonb_agg(jsonb_build_array(id, state_version, encode(state_hash, 'hex'))
                                 ORDER BY id)
                FROM agent_collaborative_runs WHERE project_id = $1
            ), '[]'::jsonb)
        )::text
        "#,
    )
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(Sha256::digest(snapshot.as_bytes()).into())
}

async fn persist_interrogation_answer(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    invocation_id: Uuid,
    interrogation_id: Uuid,
    artifact: &StructuredLanguageArtifact,
    actual_sources: &[InformationSource],
    answered_at: DateTime<Utc>,
) -> Result<(), AppError> {
    let StructuredLanguageArtifact::InterrogationAnswer {
        session_id,
        encrypted_answer,
        context_sources,
    } = artifact
    else {
        return Err(AppError::BadRequest(
            "interrogation requires an answer artifact",
        ));
    };
    if Uuid::from(*session_id) != interrogation_id || context_sources != actual_sources {
        return Err(AppError::Conflict);
    }
    let question = sqlx::query(
        "SELECT creator_identity_id, target_agent_identity_id,
                transcript_resource_node_id, key_epoch, encrypted_transcript,
                causal_delta::text AS causal_delta, created_at
         FROM agent_interrogations
         WHERE project_id = $1 AND id = $2
           AND NOT EXISTS (
               SELECT 1 FROM agent_interrogation_answers answer
               WHERE answer.project_id = agent_interrogations.project_id
                 AND answer.interrogation_id = agent_interrogations.id
           )
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(interrogation_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    if !interrogation_invocation_is_read_only(transaction, project_id, invocation_id).await? {
        return Err(AppError::Conflict);
    }
    let after = interrogation_read_only_fingerprint(transaction, project_id).await?;
    let question_fingerprint = canonical_hash(&json!({
        "interrogation_id": interrogation_id,
        "creator_identity_id": question.try_get::<Uuid, _>("creator_identity_id")?,
        "target_agent_identity_id": question.try_get::<Uuid, _>("target_agent_identity_id")?,
        "transcript_resource_node_id": question.try_get::<Uuid, _>("transcript_resource_node_id")?,
        "key_epoch": question.try_get::<i32, _>("key_epoch")?,
        "encrypted_transcript_commitment": hex::encode(Sha256::digest(
            question.try_get::<Vec<u8>, _>("encrypted_transcript")?,
        )),
        "causal_delta": serde_json::from_str::<Value>(question.try_get("causal_delta")?)
            .map_err(|_| AppError::Internal)?,
        "created_at": question.try_get::<DateTime<Utc>, _>("created_at")?,
    }))?;
    let rows = sqlx::query(
        r#"
        INSERT INTO agent_interrogation_answers (
            id, project_id, interrogation_id, invocation_id,
            encrypted_answer, context_source_descriptors,
            question_state_fingerprint, answer_state_fingerprint, answered_at
        ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8, $9)
        "#,
    )
    .bind(derived_governance_id(
        b"interrogation-answer",
        &[interrogation_id, invocation_id],
    ))
    .bind(project_id)
    .bind(interrogation_id)
    .bind(invocation_id)
    .bind(serialize_ciphertext(encrypted_answer)?)
    .bind(governance_canonical_json(context_sources)?)
    .bind(question_fingerprint.as_slice())
    .bind(after.as_slice())
    .bind(answered_at)
    .execute(&mut **transaction)
    .await?;
    if rows.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AppError::Conflict)
    }
}

async fn interrogation_invocation_is_read_only(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    invocation_id: Uuid,
) -> Result<bool, AppError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT sprout_private.interrogation_invocation_is_read_only($1, $2)",
    )
    .bind(project_id)
    .bind(invocation_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(AppError::from)
}

#[allow(clippy::too_many_arguments)]
async fn persist_signed_failure_observation(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    _agent_id: Uuid,
    invocation_id: Uuid,
    runner_id: Uuid,
    runner_key_version: i32,
    lease_id: Uuid,
    failure_code: RunnerFailureCode,
    endpoint_request_exact: bool,
    projected_endpoint_request_commitment: Option<[u8; 32]>,
    projected_runtime_kind: Option<&str>,
    projected_execution_profile_commitment: Option<[u8; 32]>,
    observation: &SignedModelEndpointObservation,
    invocation: &PgRow,
) -> Result<(), AppError> {
    let attempt_i32: i32 = invocation.try_get("attempt")?;
    let attempt = u32::try_from(attempt_i32).map_err(|_| AppError::Internal)?;
    let dispatch = sqlx::query(
        r#"
        SELECT id, runner_id, runner_identity_id, runner_device_id,
               runner_key_version, context_principal_identity_id,
               request_commitment, context_commitment, transport_commitment,
               source_descriptors::text AS source_descriptors,
               dispatched_at, lease_expires_at, runtime_kind,
               execution_profile_commitment
        FROM agent_model_attempt_dispatches
        WHERE project_id = $1 AND id = $2 AND invocation_id = $3
          AND attempt = $4 AND lease_id = $5
        "#,
    )
    .bind(project_id)
    .bind(observation.statement.dispatch_id)
    .bind(invocation_id)
    .bind(attempt_i32)
    .bind(lease_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let sources: Vec<InformationSource> =
        serde_json::from_str(dispatch.try_get("source_descriptors")?)
            .map_err(|_| AppError::Internal)?;
    let request_commitment: Vec<u8> = dispatch.try_get("request_commitment")?;
    let context_commitment: Vec<u8> = dispatch.try_get("context_commitment")?;
    let transport_commitment: Vec<u8> = dispatch.try_get("transport_commitment")?;
    let endpoint_request_commitment = observation
        .statement
        .endpoint_request_commitment_hex
        .as_deref()
        .map(decode_commitment)
        .transpose()?;
    let observed_execution_profile_commitment = observation
        .statement
        .execution_profile_commitment_hex
        .as_deref()
        .map(decode_commitment)
        .transpose()?;
    let principal: Uuid = dispatch.try_get("context_principal_identity_id")?;
    let dispatched_at: DateTime<Utc> = dispatch.try_get("dispatched_at")?;
    let expires_at: DateTime<Utc> = dispatch.try_get("lease_expires_at")?;
    let runtime_kind: String = dispatch.try_get("runtime_kind")?;
    let dispatched_execution_profile_commitment: Option<Vec<u8>> =
        dispatch.try_get("execution_profile_commitment")?;
    if (runtime_kind == "client_provider_v1"
        && (projected_runtime_kind != Some("client_provider_v1")
            || projected_execution_profile_commitment.is_none()
            || (failure_code.requires_endpoint_request_witness()
                && (!endpoint_request_exact || projected_endpoint_request_commitment.is_none()))))
        || (runtime_kind == "legacy_0031"
            && (projected_runtime_kind.is_some()
                || endpoint_request_exact
                || projected_endpoint_request_commitment.is_some()
                || projected_execution_profile_commitment.is_some()))
    {
        return Err(AppError::Conflict);
    }
    if dispatched_execution_profile_commitment.as_deref()
        != projected_execution_profile_commitment
            .as_ref()
            .map(<[u8; 32]>::as_slice)
    {
        return Err(AppError::Conflict);
    }
    if observation.statement.invocation_id != InvocationId::from(invocation_id)
        || observation.statement.attempt != attempt
        || observation.statement.lease_id != lease_id
        || observation.statement.principal_identity_id != UserId::from(principal)
        || observation.statement.exposed_sources != sources
        || observation.statement.output_commitment_hex.is_some()
        || observation.statement.artifact_commitment_hex.is_some()
        || observation.statement.request_commitment_hex != hex::encode(&request_commitment)
        || observation.statement.context_commitment_hex != hex::encode(&context_commitment)
        || observation.statement.transport_commitment_hex != hex::encode(&transport_commitment)
        || observation.statement.endpoint_request_exact != endpoint_request_exact
        || endpoint_request_commitment != projected_endpoint_request_commitment
        || observation.statement.runtime_kind.as_deref() != projected_runtime_kind
        || observed_execution_profile_commitment != projected_execution_profile_commitment
        || observation
            .statement
            .hidden_persistent_model_memory_available
        || observation.statement.provider_status != failure_code.as_str()
        || observation.statement.observed_at < dispatched_at
        || observation.statement.observed_at >= expires_at
        || dispatch.try_get::<Uuid, _>("runner_id")? != runner_id
        || dispatch.try_get::<Uuid, _>("runner_identity_id")? != actor.identity_id
        || dispatch.try_get::<Uuid, _>("runner_device_id")? != actor.device_id
        || dispatch.try_get::<i32, _>("runner_key_version")? != runner_key_version
        || observation.signatures.signer_identity_id != actor.identity_id.into()
        || observation.signatures.signer_device_id != actor.device_id
        || to_i32(observation.signatures.signer_device_key_version)? != runner_key_version
    {
        return Err(AppError::Conflict);
    }
    verify_device_statement_for_signer(
        transaction,
        actor.identity_id.into(),
        &observation.statement,
        &observation.signatures,
        MODEL_RUNTIME_OBSERVATION_SIGNATURE_CONTEXT,
    )
    .await?;
    validate_model_runtime_projection(
        &ModelRuntimeActualObservation {
            invocation_id: InvocationId::from(invocation_id),
            attempt,
            principal: principal.into(),
            exposed_sources: sources.clone(),
            request_commitment: request_commitment
                .as_slice()
                .try_into()
                .map_err(|_| AppError::Internal)?,
            output_commitment: None,
            explicit_failure: true,
            hidden_persistent_model_memory_available: false,
        },
        &R540ModelRuntimeProjection {
            invocation_id: InvocationId::from(invocation_id),
            attempt,
            principal: principal.into(),
            context_sources: sources.clone(),
            request_commitment: request_commitment
                .as_slice()
                .try_into()
                .map_err(|_| AppError::Internal)?,
            output_commitment: None,
            explicit_failure: true,
        },
    )
    .map_err(agent_validation_error)?;
    if let Some(run_id) = invocation.try_get::<Option<Uuid>, _>("run_id")? {
        let binding = ModelInvocationWorkBinding {
            trace_id: invocation
                .try_get::<Option<Uuid>, _>("trace_id")?
                .ok_or(AppError::Internal)?,
            run: run_id.into(),
            goal: invocation
                .try_get::<Option<Uuid>, _>("goal_id")?
                .ok_or(AppError::Internal)?
                .into(),
            work: invocation
                .try_get::<Option<Uuid>, _>("work_item_id")?
                .ok_or(AppError::Internal)?
                .into(),
            claim: invocation
                .try_get::<Option<Uuid>, _>("work_claim_id")?
                .ok_or(AppError::Internal)?
                .into(),
            attempt: u32::try_from(
                invocation
                    .try_get::<Option<i32>, _>("work_attempt")?
                    .ok_or(AppError::Internal)?,
            )
            .map_err(|_| AppError::Internal)?,
        };
        validate_invocation_work_binding(
            transaction,
            project_id,
            actor.identity_id.into(),
            &binding,
        )
        .await?;
    }
    let observation_hash: [u8; 32] =
        Sha256::digest(governance_canonical_bytes(&observation.statement)?).into();
    sqlx::query(
        r#"
        INSERT INTO agent_model_attempt_observations (
            id, project_id, dispatch_id, invocation_id, attempt, lease_id,
            principal_identity_id, status, provider_status,
            request_commitment, context_commitment, exposure_commitment,
            endpoint_request_exact, endpoint_request_commitment,
            runtime_kind, execution_profile_commitment,
            transport_commitment, exposed_source_descriptors,
            hidden_persistent_model_memory_available,
            signer_identity_id, signer_device_id, signer_key_version,
            classical_signature, post_quantum_signature,
            observation_hash, idempotency_key, observed_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, 'explicit_failure', $8,
            $9, $10, $10, $11, $12, $13, $14, $15, $16::jsonb, false,
            $17, $18, $19, $20, $21, $22, $23, $24
        )
        "#,
    )
    .bind(observation.statement.observation_id)
    .bind(project_id)
    .bind(observation.statement.dispatch_id)
    .bind(invocation_id)
    .bind(attempt_i32)
    .bind(lease_id)
    .bind(principal)
    .bind(&observation.statement.provider_status)
    .bind(&request_commitment)
    .bind(&context_commitment)
    .bind(endpoint_request_exact)
    .bind(
        projected_endpoint_request_commitment
            .as_ref()
            .map(<[u8; 32]>::as_slice),
    )
    .bind(&runtime_kind)
    .bind(
        projected_execution_profile_commitment
            .as_ref()
            .map(<[u8; 32]>::as_slice),
    )
    .bind(&transport_commitment)
    .bind(governance_canonical_json(&sources)?)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(runner_key_version)
    .bind(&observation.signatures.classical_signature)
    .bind(&observation.signatures.post_quantum_signature)
    .bind(observation_hash.as_slice())
    .bind(observation.statement.idempotency_key)
    .bind(observation.statement.observed_at)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO agent_model_invocation_projections (
            id, project_id, invocation_id, observation_id, provider_attempt,
            trace_id, run_id, goal_id, work_item_id, work_claim_id, work_attempt,
            principal_identity_id, status, invocation_surface, language_task,
            context_source_descriptors, request_commitment, context_commitment,
            endpoint_request_exact, endpoint_request_commitment, invoked_at
            , runtime_kind, execution_profile_commitment
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
            $12, 'explicit_failure', $13, $14::jsonb, $15::jsonb,
            $16, $17, $18, $19, $20, $21, $22
        )
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(invocation_id)
    .bind(observation.statement.observation_id)
    .bind(attempt_i32)
    .bind(invocation.try_get::<Option<Uuid>, _>("trace_id")?)
    .bind(invocation.try_get::<Option<Uuid>, _>("run_id")?)
    .bind(invocation.try_get::<Option<Uuid>, _>("goal_id")?)
    .bind(invocation.try_get::<Option<Uuid>, _>("work_item_id")?)
    .bind(invocation.try_get::<Option<Uuid>, _>("work_claim_id")?)
    .bind(invocation.try_get::<Option<i32>, _>("work_attempt")?)
    .bind(principal)
    .bind(invocation.try_get::<String, _>("invocation_surface")?)
    .bind(invocation.try_get::<String, _>("language_task")?)
    .bind(governance_canonical_json(&sources)?)
    .bind(&request_commitment)
    .bind(&context_commitment)
    .bind(endpoint_request_exact)
    .bind(
        projected_endpoint_request_commitment
            .as_ref()
            .map(<[u8; 32]>::as_slice),
    )
    .bind(dispatched_at)
    .bind(&runtime_kind)
    .bind(
        projected_execution_profile_commitment
            .as_ref()
            .map(<[u8; 32]>::as_slice),
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
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

fn decode_commitment(value: &str) -> Result<[u8; 32], AppError> {
    let bytes = hex::decode(value).map_err(|_| AppError::BadRequest("invalid commitment"))?;
    bytes
        .try_into()
        .map_err(|_| AppError::BadRequest("invalid commitment"))
}

fn final_prompt_approval_identity_hash(
    statement: &FinalPromptApprovalStatement,
) -> Result<[u8; 32], AppError> {
    canonical_hash(&json!({
        "signature_context": "sprout-final-prompt-approval-v1",
        "approval_id": statement.approval_id,
        "project_id": statement.project_id,
        "draft_id": statement.draft_id,
        "agent_principal_identity_id": statement.agent_principal_identity_id,
        "controller_identity_id": statement.controller_identity_id,
        "local_goal_id": statement.local_goal_id,
        "local_revision": statement.local_revision,
        "prompt_commitment_hex": statement.prompt_commitment_hex,
        "ciphertext_commitment_hex": statement.ciphertext_commitment_hex,
        "compilation_certificate_id": statement.compilation_certificate_id,
        "structured_output_hash_hex": statement.structured_output_hash_hex,
        "idempotency_key": statement.idempotency_key,
    }))
}

fn administrator_creation_proposal_binding(
    project_id: Uuid,
    request: &ProvisionAgentRequest,
    local: &LocalGoalContract,
    contract_hash: [u8; 32],
) -> AdministratorCreationProposalBinding {
    let statement = &request.initial_local_goal.compilation.statement;
    AdministratorCreationProposalBinding {
        project_id,
        administrator_identity_id: request.controller_identity_id,
        proposed_agent_identity_id: request.principal_identity_id,
        governed_agent_id: request.id,
        proposal_draft_id: statement.draft_id,
        local_goal_id: Uuid::from(local.id),
        local_goal_revision: local.revision,
        contract_hash_hex: hex::encode(contract_hash),
        compilation_certificate_id: statement.certificate_id,
        prompt_plaintext_commitment_hex: statement.prompt_commitment_hex.clone(),
        ciphertext_commitment_hex: statement.ciphertext_commitment_hex.clone(),
        availability: request.availability,
        scope: local.contract.scope,
    }
}

async fn validate_initial_creation_authorization(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: AuthSession,
    request: &ProvisionAgentRequest,
    local: &LocalGoalContract,
) -> Result<(), AppError> {
    match &request
        .initial_local_goal
        .compilation
        .statement
        .authorization
    {
        LocalCompilationAuthorization::Responsibility { .. } => {
            if request.administrator_creation_approval.is_some()
                || !matches!(local.origin, LocalGoalOrigin::ControllerPrompt {})
            {
                return Err(AppError::Conflict);
            }
            validate_local_authorization(
                transaction,
                project_id,
                actor,
                local,
                &request
                    .initial_local_goal
                    .compilation
                    .statement
                    .authorization,
            )
            .await
            .map(|_| ())
        }
        LocalCompilationAuthorization::AdministratorCreation { approval_id } => {
            if !matches!(
                local.origin,
                LocalGoalOrigin::AdministratorCreation {
                    approval_id: origin_id
                } if origin_id == *approval_id
            ) || request
                .administrator_creation_approval
                .as_ref()
                .is_none_or(|approval| approval.statement.approval_id != *approval_id)
            {
                return Err(AppError::Conflict);
            }
            let administrator = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (
                    SELECT 1 FROM project_memberships membership
                    JOIN identities identity ON identity.id = membership.identity_id
                    WHERE membership.project_id = $1 AND membership.identity_id = $2
                      AND membership.state = 'active'
                      AND membership.role IN ('owner', 'admin')
                      AND identity.status = 'active'
                      AND identity.principal_kind = 'user')",
            )
            .bind(project_id)
            .bind(actor.identity_id)
            .fetch_one(&mut **transaction)
            .await?;
            if !administrator
                || !resource_access_in_transaction(
                    transaction,
                    project_id,
                    actor.identity_id,
                    Uuid::from(local.contract.scope),
                    ResourceOperation::Manage,
                )
                .await?
            {
                return Err(AppError::Forbidden);
            }
            Ok(())
        }
        LocalCompilationAuthorization::AdministratorException { .. }
        | LocalCompilationAuthorization::GlobalMandate { .. } => Err(AppError::BadRequest(
            "initial-agent authorization adapter is not available",
        )),
    }
}

async fn validate_administrator_creation_approval(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    project_id: Uuid,
    request: &ProvisionAgentRequest,
    local: &LocalGoalContract,
    contract_hash: [u8; 32],
    signed: &SignedAdministratorAgentCreationApproval,
) -> Result<(), AppError> {
    let statement = &signed.statement;
    let compiler_statement = &request.initial_local_goal.compilation.statement;
    let LocalCompilationAuthorization::AdministratorCreation {
        approval_id: expected_approval_id,
    } = &compiler_statement.authorization
    else {
        return Err(AppError::Conflict);
    };
    let expected_binding =
        administrator_creation_proposal_binding(project_id, request, local, contract_hash);
    if statement.approval_id != *expected_approval_id
        || statement.project_id != project_id
        || statement.administrator_identity_id != actor.identity_id.into()
        || statement.administrator_identity_id != request.controller_identity_id
        || statement.signer_device_id != signed.signatures.signer_device_id
        || statement.signer_device_key_version != signed.signatures.signer_device_key_version
        || signed.signatures.signer_identity_id != statement.administrator_identity_id
        || statement.proposed_agent_identity_id != request.principal_identity_id
        || statement.governed_agent_id != request.id
        || statement.proposal_draft_id != compiler_statement.draft_id
        || statement.local_goal_id != Uuid::from(local.id)
        || statement.local_goal_revision != local.revision
        || decode_commitment(&statement.contract_hash_hex)? != contract_hash
        || statement.compilation_certificate_id != compiler_statement.certificate_id
        || statement.prompt_plaintext_commitment_hex != compiler_statement.prompt_commitment_hex
        || statement.ciphertext_commitment_hex != compiler_statement.ciphertext_commitment_hex
        || statement.availability != request.availability
        || statement.scope != local.contract.scope
        || decode_commitment(&statement.canonical_proposal_hash_hex)?
            != canonical_hash(&expected_binding)?
    {
        return Err(AppError::Conflict);
    }
    verify_device_statement(
        transaction,
        actor,
        statement,
        &signed.signatures,
        ADMINISTRATOR_AGENT_CREATION_SIGNATURE_CONTEXT,
    )
    .await?;
    Ok(())
}

async fn persist_administrator_creation_approval(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    signed: &SignedAdministratorAgentCreationApproval,
    contract_hash: [u8; 32],
) -> Result<(), AppError> {
    let statement = &signed.statement;
    let approval_hash = canonical_hash(statement)?;
    sqlx::query(
        "SELECT sprout_private.insert_verified_administrator_creation_approval(
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, $17, $18, $19, $20, $21)",
    )
    .bind(project_id)
    .bind(statement.approval_id)
    .bind(Uuid::from(statement.administrator_identity_id))
    .bind(statement.signer_device_id)
    .bind(to_i32(statement.signer_device_key_version)?)
    .bind(Uuid::from(statement.proposed_agent_identity_id))
    .bind(Uuid::from(statement.governed_agent_id))
    .bind(statement.proposal_draft_id)
    .bind(statement.local_goal_id)
    .bind(to_i64(statement.local_goal_revision)?)
    .bind(contract_hash.as_slice())
    .bind(statement.compilation_certificate_id)
    .bind(decode_commitment(&statement.prompt_plaintext_commitment_hex)?.as_slice())
    .bind(decode_commitment(&statement.ciphertext_commitment_hex)?.as_slice())
    .bind(availability_name(statement.availability))
    .bind(Uuid::from(statement.scope))
    .bind(decode_commitment(&statement.canonical_proposal_hash_hex)?.as_slice())
    .bind(statement.idempotency_key)
    .bind(approval_hash.as_slice())
    .bind(&signed.signatures.classical_signature)
    .bind(&signed.signatures.post_quantum_signature)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn persist_final_prompt_approval(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    draft_id: Uuid,
    agent_id: Uuid,
    controller_identity_id: Uuid,
    local_goal_id: Uuid,
    local_goal_revision: u64,
    prompt_hash: [u8; 32],
    agent_principal_identity_id: UserId,
    signed: &SignedFinalPromptApproval,
    approval_hash: [u8; 32],
) -> Result<(), AppError> {
    let statement = &signed.statement;
    sqlx::query(
        "SELECT sprout_private.insert_verified_final_prompt_approval(
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            $13, $14, $15, $16, $17, $18, $19, $20)",
    )
    .bind(project_id)
    .bind(draft_id)
    .bind(agent_id)
    .bind(controller_identity_id)
    .bind(local_goal_id)
    .bind(to_i64(local_goal_revision)?)
    .bind(prompt_hash.as_slice())
    .bind(statement.approval_id)
    .bind(statement.idempotency_key)
    .bind(Uuid::from(agent_principal_identity_id))
    .bind(signed.signatures.signer_device_id)
    .bind(to_i32(signed.signatures.signer_device_key_version)?)
    .bind(decode_commitment(&statement.prompt_commitment_hex)?.as_slice())
    .bind(decode_commitment(&statement.ciphertext_commitment_hex)?.as_slice())
    .bind(statement.compilation_certificate_id)
    .bind(decode_commitment(&statement.structured_output_hash_hex)?.as_slice())
    .bind(decode_commitment(&statement.approval_identity_hash_hex)?.as_slice())
    .bind(approval_hash.as_slice())
    .bind(&signed.signatures.classical_signature)
    .bind(&signed.signatures.post_quantum_signature)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn append_verified_governance_revision(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    entry_kind: &'static str,
    subject_id: Uuid,
    subject_revision: u64,
    compilation_id: Uuid,
    contract_hash: [u8; 32],
) -> Result<(), AppError> {
    sqlx::query(
        "SELECT sprout_private.append_verified_governance_revision(
            $1, $2, $3, $4, $5, $6)",
    )
    .bind(project_id)
    .bind(entry_kind)
    .bind(subject_id)
    .bind(to_i64(subject_revision)?)
    .bind(compilation_id)
    .bind(contract_hash.as_slice())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

struct GovernanceAuthorizationEvent<'a> {
    event_id: Uuid,
    event_kind: &'static str,
    workflow_id: Uuid,
    workflow_revision: u64,
    actor_identity_id: Uuid,
    user_identity_id: Option<Uuid>,
    administrator_identity_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    source_draft_id: Option<Uuid>,
    review_task_id: Option<Uuid>,
    local_goal_id: Option<Uuid>,
    local_goal_revision: Option<u64>,
    global_contract_id: Option<Uuid>,
    global_revision: Option<u64>,
    obligation_id: Option<Uuid>,
    compilation_certificate_id: Option<Uuid>,
    responsibility_compilation_id: Option<Uuid>,
    idempotency_key: Uuid,
    payload: &'a Value,
}

async fn persist_governance_authorization_event(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    event: GovernanceAuthorizationEvent<'_>,
) -> Result<i64, AppError> {
    let event_envelope = json!({
        "project_id": project_id,
        "event_id": event.event_id,
        "event_kind": event.event_kind,
        "workflow_id": event.workflow_id,
        "workflow_revision": event.workflow_revision,
        "actor_identity_id": event.actor_identity_id,
        "user_identity_id": event.user_identity_id,
        "administrator_identity_id": event.administrator_identity_id,
        "agent_id": event.agent_id,
        "source_draft_id": event.source_draft_id,
        "review_task_id": event.review_task_id,
        "local_goal_id": event.local_goal_id,
        "local_goal_revision": event.local_goal_revision,
        "global_contract_id": event.global_contract_id,
        "global_revision": event.global_revision,
        "obligation_id": event.obligation_id,
        "compilation_certificate_id": event.compilation_certificate_id,
        "responsibility_compilation_id": event.responsibility_compilation_id,
        "idempotency_key": event.idempotency_key,
        "payload": event.payload,
    });
    let event_hash = canonical_hash(&event_envelope)?;
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT sprout_private.insert_agent_governance_authorization_event(
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
            $12, $13, $14, $15, $16, $17, $18, $19, $20, $21::jsonb
        )
        "#,
    )
    .bind(project_id)
    .bind(event.event_id)
    .bind(event.event_kind)
    .bind(event.workflow_id)
    .bind(to_i64(event.workflow_revision)?)
    .bind(event.actor_identity_id)
    .bind(event.user_identity_id)
    .bind(event.administrator_identity_id)
    .bind(event.agent_id)
    .bind(event.source_draft_id)
    .bind(event.review_task_id)
    .bind(event.local_goal_id)
    .bind(event.local_goal_revision.map(to_i64).transpose()?)
    .bind(event.global_contract_id)
    .bind(event.global_revision.map(to_i64).transpose()?)
    .bind(event.obligation_id)
    .bind(event.compilation_certificate_id)
    .bind(event.responsibility_compilation_id)
    .bind(event.idempotency_key)
    .bind(event_hash.as_slice())
    .bind(governance_canonical_json(event.payload)?)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .and_then(|database| database.code())
            .is_some_and(|code| code == "40001" || code == "23505")
        {
            AppError::Conflict
        } else {
            AppError::from(error)
        }
    })
}

fn local_authorization_columns(
    authorization: &LocalCompilationAuthorization,
) -> (&'static str, Option<Uuid>, Option<u64>) {
    match authorization {
        LocalCompilationAuthorization::Responsibility { id, revision } => {
            ("responsibility", Some(*id), Some(*revision))
        }
        LocalCompilationAuthorization::AdministratorException { id, revision } => {
            ("administrator_exception", Some(*id), Some(*revision))
        }
        LocalCompilationAuthorization::GlobalMandate { id, revision } => {
            ("global_mandate", Some(*id), Some(*revision))
        }
        LocalCompilationAuthorization::AdministratorCreation { approval_id } => {
            ("administrator_creation", Some(*approval_id), None)
        }
    }
}

async fn validate_local_authorization(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: AuthSession,
    contract: &LocalGoalContract,
    authorization: &LocalCompilationAuthorization,
) -> Result<Option<(Uuid, u64, u64)>, AppError> {
    match authorization {
        LocalCompilationAuthorization::Responsibility { id, revision } => {
            let responsibility_json = sqlx::query_scalar::<_, String>(
                "SELECT contract::text FROM agent_responsibility_contracts
                 WHERE project_id = $1 AND id = $2 AND revision = $3
                   AND user_identity_id = $4 AND state = 'active'
                   AND compilation_certificate_id IS NOT NULL",
            )
            .bind(project_id)
            .bind(*id)
            .bind(to_i64(*revision)?)
            .bind(actor.identity_id)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::Forbidden)?;
            let responsibility: ResponsibilityContract =
                serde_json::from_str(&responsibility_json).map_err(|_| AppError::Internal)?;
            if !responsibility_operationally_covers(
                transaction,
                project_id,
                &responsibility,
                contract,
            )
            .await?
            {
                return Err(AppError::Forbidden);
            }
            Ok(None)
        }
        LocalCompilationAuthorization::AdministratorException { id, revision } => {
            let row = sqlx::query(
                "SELECT approved.payload::text AS approved_payload,
                        decision.payload::text AS decision_payload,
                        approved.responsibility_compilation_id
                 FROM agent_governance_authorization_events approved
                 JOIN agent_governance_authorization_events decision
                   ON decision.project_id=approved.project_id
                  AND decision.workflow_id=approved.workflow_id
                  AND decision.workflow_revision=approved.workflow_revision
                  AND decision.event_kind='exception_decision'
                 WHERE approved.project_id=$1
                   AND approved.event_kind='approved_local_exception'
                   AND approved.workflow_id=$2 AND approved.workflow_revision=$3
                   AND approved.user_identity_id=$4
                   AND approved.agent_id=(SELECT id FROM governed_agents
                     WHERE project_id=$1 AND principal_identity_id=$5)
                   AND approved.local_goal_id=$6 AND approved.local_goal_revision=$7
                   AND approved.compilation_certificate_id=(SELECT compilation_certificate_id
                     FROM agent_local_goal_contracts WHERE project_id=$1 AND id=$6
                       AND revision=$7)
                 FOR UPDATE OF approved, decision",
            )
            .bind(project_id)
            .bind(*id)
            .bind(to_i64(*revision)?)
            .bind(actor.identity_id)
            .bind(Uuid::from(contract.agent))
            .bind(Uuid::from(contract.id))
            .bind(to_i64(contract.revision)?)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::Forbidden)?;
            let approved_payload: Value = serde_json::from_str(row.try_get("approved_payload")?)
                .map_err(|_| AppError::Internal)?;
            let approved: ApprovedLocalGoalException = serde_json::from_value(
                approved_payload
                    .get("approved")
                    .cloned()
                    .ok_or(AppError::Internal)?,
            )
            .map_err(|_| AppError::Internal)?;
            if approved.local != *contract || approved.review_id != *id {
                return Err(AppError::Conflict);
            }
            let signed_decision: SignedExceptionDecision =
                serde_json::from_str(row.try_get("decision_payload")?)
                    .map_err(|_| AppError::Internal)?;
            let administrator = approved.administrator;
            let administrator_current = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM project_memberships membership
                 JOIN identities identity ON identity.id=membership.identity_id
                 WHERE membership.project_id=$1 AND membership.identity_id=$2
                   AND membership.state='active' AND membership.role IN ('owner','admin')
                   AND identity.status='active')",
            )
            .bind(project_id)
            .bind(Uuid::from(administrator))
            .fetch_one(&mut **transaction)
            .await?;
            if !administrator_current
                || !resource_access_in_transaction(
                    transaction,
                    project_id,
                    Uuid::from(administrator),
                    Uuid::from(contract.contract.scope),
                    ResourceOperation::Manage,
                )
                .await?
            {
                return Err(AppError::Forbidden);
            }
            verify_device_statement_for_signer(
                transaction,
                administrator,
                &signed_decision.statement,
                &signed_decision.signatures,
                EXCEPTION_DECISION_SIGNATURE_CONTEXT,
            )
            .await?;
            let responsibility_compilation_id: Option<Uuid> =
                row.try_get("responsibility_compilation_id")?;
            if let Some(compilation_id) = responsibility_compilation_id {
                let responsibility = sqlx::query_as::<_, (Uuid, i64, String)>(
                    "SELECT id, revision, contract::text
                     FROM agent_responsibility_contracts
                     WHERE project_id=$1 AND compilation_certificate_id=$2
                       AND user_identity_id=$3 AND state='draft' FOR UPDATE",
                )
                .bind(project_id)
                .bind(compilation_id)
                .bind(actor.identity_id)
                .fetch_optional(&mut **transaction)
                .await?
                .ok_or(AppError::Conflict)?;
                let responsibility_contract: ResponsibilityContract =
                    serde_json::from_str(&responsibility.2).map_err(|_| AppError::Internal)?;
                Ok(Some((
                    responsibility.0,
                    u64::try_from(responsibility.1).map_err(|_| AppError::Internal)?,
                    responsibility_contract
                        .supersedes_revision
                        .ok_or(AppError::Conflict)?,
                )))
            } else {
                Ok(None)
            }
        }
        LocalCompilationAuthorization::GlobalMandate { id, revision } => {
            let row = sqlx::query(
                "SELECT assignment.payload::text AS payload,
                        assignment.global_contract_id,
                        assignment.global_revision,
                        agent.id AS agent_id, agent.availability,
                        agent.controller_identity_id
                 FROM agent_governance_authorization_events assignment
                 JOIN governed_agents agent ON agent.project_id=assignment.project_id
                  AND agent.id=assignment.agent_id AND agent.state='active'
                 WHERE assignment.project_id=$1
                   AND assignment.event_kind='global_mandate_assignment'
                   AND assignment.event_id=$2 AND assignment.global_revision=$3
                   AND assignment.local_goal_id=$4
                   AND assignment.local_goal_revision=$5
                   AND assignment.compilation_certificate_id=(
                     SELECT compilation_certificate_id FROM agent_local_goal_contracts
                     WHERE project_id=$1 AND id=$4 AND revision=$5)
                 FOR UPDATE OF assignment, agent",
            )
            .bind(project_id)
            .bind(*id)
            .bind(to_i64(*revision)?)
            .bind(Uuid::from(contract.id))
            .bind(to_i64(contract.revision)?)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::Forbidden)?;
            if row.try_get::<String, _>("availability")? != "project_delegable"
                || row.try_get::<Uuid, _>("controller_identity_id")? != actor.identity_id
            {
                return Err(AppError::Forbidden);
            }
            let payload: Value =
                serde_json::from_str(row.try_get("payload")?).map_err(|_| AppError::Internal)?;
            let assignment: GlobalMandateAssignment = serde_json::from_value(
                payload
                    .get("assignment")
                    .cloned()
                    .ok_or(AppError::Internal)?,
            )
            .map_err(|_| AppError::Internal)?;
            if assignment.local != *contract || assignment.global_revision != *revision {
                return Err(AppError::Conflict);
            }
            let global_contract_id: Uuid = row.try_get("global_contract_id")?;
            let latest = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM agent_global_contracts
                 WHERE project_id=$1 AND id=$2 AND revision=$3
                   AND revision=(SELECT max(revision) FROM agent_global_contracts
                                 WHERE project_id=$1 AND id=$2))",
            )
            .bind(project_id)
            .bind(global_contract_id)
            .bind(to_i64(*revision)?)
            .fetch_one(&mut **transaction)
            .await?;
            if !latest || !assignment.need.required.tools.is_empty() {
                return Err(AppError::Forbidden);
            }
            for effect in &assignment.need.required.resource_effects {
                if !resource_access_in_transaction(
                    transaction,
                    project_id,
                    Uuid::from(contract.agent),
                    Uuid::from(effect.resource_id),
                    effect.operation,
                )
                .await?
                {
                    return Err(AppError::Forbidden);
                }
            }
            Ok(None)
        }
        LocalCompilationAuthorization::AdministratorCreation { .. } => Err(AppError::BadRequest(
            "local-goal authorization adapter is not available",
        )),
    }
}

fn canonical_hash(value: &impl Serialize) -> Result<[u8; 32], AppError> {
    Ok(Sha256::digest(governance_canonical_bytes(value)?).into())
}

fn derived_governance_id(context: &[u8], parts: &[Uuid]) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(b"sprout-governance-derived-id-v1");
    digest.update(context);
    for part in parts {
        digest.update(part.as_bytes());
    }
    let hash: [u8; 32] = digest.finalize().into();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn governance_canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, AppError> {
    canonical_governance_json(value)
        .map_err(|_| AppError::BadRequest("invalid canonical governance payload"))
}

fn governance_canonical_json(value: &impl Serialize) -> Result<String, AppError> {
    String::from_utf8(governance_canonical_bytes(value)?)
        .map_err(|_| AppError::BadRequest("invalid canonical governance payload"))
}

pub(crate) async fn verify_device_statement(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    statement: &impl Serialize,
    signatures: &CompilationSignatures,
    context: &[u8],
) -> Result<[u8; 32], AppError> {
    if signatures.signer_identity_id != actor.identity_id.into() {
        return Err(AppError::Forbidden);
    }
    let key_version = to_i32(signatures.signer_device_key_version)?;
    let keys = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        r#"
        SELECT key.ed25519_public_key, key.ml_dsa_65_public_key
        FROM device_keys key
        JOIN devices device
          ON device.identity_id = key.identity_id
         AND device.id = key.device_id
        WHERE key.identity_id = $1 AND key.device_id = $2
          AND key.key_version = $3 AND key.suite_version = 32769
          AND key.revoked_at IS NULL
          AND device.trust_state = 'trusted' AND device.retired_at IS NULL
        "#,
    )
    .bind(Uuid::from(signatures.signer_identity_id))
    .bind(signatures.signer_device_id)
    .bind(key_version)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let statement_bytes = governance_canonical_bytes(statement)?;
    verify_ed25519_ml_dsa65_signatures(
        &keys.0,
        &signatures.classical_signature,
        &keys.1,
        &signatures.post_quantum_signature,
        &statement_bytes,
        context,
    )
    .map_err(|_| AppError::BadRequest("device attestation signature verification failed"))?;
    Ok(Sha256::digest(statement_bytes).into())
}

async fn verify_device_statement_for_signer(
    transaction: &mut Transaction<'_, Postgres>,
    expected_signer: UserId,
    statement: &impl Serialize,
    signatures: &CompilationSignatures,
    context: &[u8],
) -> Result<[u8; 32], AppError> {
    if signatures.signer_identity_id != expected_signer {
        return Err(AppError::Forbidden);
    }
    let key_version = to_i32(signatures.signer_device_key_version)?;
    let keys = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        r#"
        SELECT key.ed25519_public_key, key.ml_dsa_65_public_key
        FROM device_keys key
        JOIN devices device
          ON device.identity_id = key.identity_id AND device.id = key.device_id
        WHERE key.identity_id = $1 AND key.device_id = $2
          AND key.key_version = $3 AND key.suite_version = 32769
          AND key.revoked_at IS NULL
          AND device.trust_state = 'trusted' AND device.retired_at IS NULL
        "#,
    )
    .bind(Uuid::from(expected_signer))
    .bind(signatures.signer_device_id)
    .bind(key_version)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let statement_bytes = governance_canonical_bytes(statement)?;
    verify_ed25519_ml_dsa65_signatures(
        &keys.0,
        &signatures.classical_signature,
        &keys.1,
        &signatures.post_quantum_signature,
        &statement_bytes,
        context,
    )
    .map_err(|_| AppError::BadRequest("device attestation signature verification failed"))?;
    Ok(Sha256::digest(statement_bytes).into())
}

async fn require_pinned_compiler(
    transaction: &mut Transaction<'_, Postgres>,
    task_kind: &'static str,
    compiler: &CompilerIdentity,
) -> Result<[u8; 32], AppError> {
    let digest = decode_commitment(&compiler.compiler_build_digest_hex)?;
    let (expected_digest, manifest) = match (
        task_kind,
        compiler.compiler_id.as_str(),
        compiler.compiler_version,
    ) {
        ("local_goal", "sprout.local-goal.compiler", 1) => (
            LOCAL_GOAL_COMPILER_PROTOCOL_MANIFEST_SHA256,
            LOCAL_GOAL_COMPILER_PROTOCOL_MANIFEST,
        ),
        ("responsibility", "sprout.responsibility.compiler", 1) => (
            RESPONSIBILITY_COMPILER_PROTOCOL_MANIFEST_SHA256,
            RESPONSIBILITY_COMPILER_PROTOCOL_MANIFEST,
        ),
        _ => return Err(AppError::BadRequest("compiler build is not pinned")),
    };
    if digest != expected_digest || Sha256::digest(manifest).as_slice() != expected_digest {
        return Err(AppError::BadRequest("compiler build is not pinned"));
    }
    let registered = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM agent_compiler_builds
            WHERE task_kind = $1 AND compiler_name = $2
              AND compiler_version = $3 AND build_digest = $4
              AND enabled
        )
        "#,
    )
    .bind(task_kind)
    .bind(&compiler.compiler_id)
    .bind(to_i32(compiler.compiler_version)?)
    .bind(digest.as_slice())
    .fetch_one(&mut **transaction)
    .await?;
    if !registered {
        return Err(AppError::BadRequest("compiler build is not pinned"));
    }
    Ok(digest)
}

struct CompilationRecord<'a> {
    id: Uuid,
    task_kind: &'static str,
    compiler: &'a CompilerIdentity,
    build_digest: [u8; 32],
    signer: &'a CompilationSignatures,
    subject_id: Uuid,
    subject_revision: u64,
    draft_id: Uuid,
    agent_principal_identity_id: Option<Uuid>,
    controller_identity_id: Option<Uuid>,
    administrator_identity_id: Option<Uuid>,
    user_identity_id: Option<Uuid>,
    input_commitment: [u8; 32],
    ciphertext_commitment: [u8; 32],
    output_json: String,
    output_hash: [u8; 32],
    envelope_json: String,
    envelope_hash: [u8; 32],
    certificate_hash: [u8; 32],
    idempotency_key: Uuid,
    classifier_version: Option<u32>,
    classifier_output_hash: Option<[u8; 32]>,
    authorization_kind: &'static str,
    authorization_id: Option<Uuid>,
    authorization_revision: Option<u64>,
}

async fn persist_compilation_certificate(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    record: CompilationRecord<'_>,
) -> Result<(), AppError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 39))")
        .bind(format!(
            "{}:{}:{}:{}",
            project_id, record.task_kind, record.subject_id, record.subject_revision
        ))
        .execute(&mut **transaction)
        .await?;
    let existing = sqlx::query_as::<_, (Uuid, Vec<u8>)>(
        r#"
        SELECT id, certificate_hash
        FROM agent_compilation_certificates
        WHERE project_id = $1 AND task_kind = $2
          AND subject_id = $3 AND subject_revision = $4
        "#,
    )
    .bind(project_id)
    .bind(record.task_kind)
    .bind(record.subject_id)
    .bind(to_i64(record.subject_revision)?)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some((id, certificate_hash)) = existing {
        if id == record.id && certificate_hash == record.certificate_hash {
            return Ok(());
        }
        return Err(AppError::Conflict);
    }
    sqlx::query(
        r#"
        SELECT sprout_private.insert_verified_compilation_certificate(
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            $13, $14, $15, $16, $17, $18, $19::jsonb, $20,
            $21::jsonb, $22, $23, $24, $25, $26, $27, $28, $29,
            $30, $31
        )
        "#,
    )
    .bind(record.id)
    .bind(project_id)
    .bind(record.task_kind)
    .bind(&record.compiler.compiler_id)
    .bind(to_i32(record.compiler.compiler_version)?)
    .bind(record.build_digest.as_slice())
    .bind(Uuid::from(record.signer.signer_identity_id))
    .bind(record.signer.signer_device_id)
    .bind(to_i32(record.signer.signer_device_key_version)?)
    .bind(record.subject_id)
    .bind(to_i64(record.subject_revision)?)
    .bind(record.draft_id)
    .bind(record.agent_principal_identity_id)
    .bind(record.controller_identity_id)
    .bind(record.administrator_identity_id)
    .bind(record.user_identity_id)
    .bind(record.input_commitment.as_slice())
    .bind(record.ciphertext_commitment.as_slice())
    .bind(record.output_json)
    .bind(record.output_hash.as_slice())
    .bind(record.envelope_json)
    .bind(record.envelope_hash.as_slice())
    .bind(record.certificate_hash.as_slice())
    .bind(record.idempotency_key)
    .bind(&record.signer.classical_signature)
    .bind(&record.signer.post_quantum_signature)
    .bind(record.classifier_version.map(to_i32).transpose()?)
    .bind(
        record
            .classifier_output_hash
            .as_ref()
            .map(<[u8; 32]>::as_slice),
    )
    .bind(record.authorization_kind)
    .bind(record.authorization_id)
    .bind(record.authorization_revision.map(to_i64).transpose()?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
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

    #[test]
    fn pinned_compiler_digests_are_hashes_of_versioned_protocol_manifests() {
        assert_eq!(
            Sha256::digest(LOCAL_GOAL_COMPILER_PROTOCOL_MANIFEST).as_slice(),
            LOCAL_GOAL_COMPILER_PROTOCOL_MANIFEST_SHA256
        );
        assert_eq!(
            Sha256::digest(RESPONSIBILITY_COMPILER_PROTOCOL_MANIFEST).as_slice(),
            RESPONSIBILITY_COMPILER_PROTOCOL_MANIFEST_SHA256
        );
    }

    #[test]
    fn final_prompt_approval_identity_is_domain_separated_and_exact() {
        let mut statement = FinalPromptApprovalStatement {
            approval_id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            draft_id: Uuid::from_u128(3),
            agent_principal_identity_id: UserId::from(Uuid::from_u128(4)),
            controller_identity_id: UserId::from(Uuid::from_u128(5)),
            local_goal_id: Uuid::from_u128(6),
            local_revision: 1,
            prompt_commitment_hex: "11".repeat(32),
            ciphertext_commitment_hex: "22".repeat(32),
            compilation_certificate_id: Uuid::from_u128(7),
            structured_output_hash_hex: "33".repeat(32),
            approval_identity_hash_hex: "00".repeat(32),
            idempotency_key: Uuid::from_u128(8),
        };
        let original = final_prompt_approval_identity_hash(&statement).unwrap();
        statement.local_revision = 2;
        assert_ne!(
            original,
            final_prompt_approval_identity_hash(&statement).unwrap()
        );
    }
}
