use std::{collections::HashSet, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sprout_domain::{
    BlockScope, BlockerId, BlockerResolutionFacts, BlockerResolutionObservation, BlockerStatus,
    ClaimId, CollaborativeRunState, CollaborativeRunStatus, ContractCondition,
    ContractConditionFacts, ContractEvidenceSubject, EvidenceId, EvidenceKind, EvidenceRecord,
    EvidenceSubject, EvidenceVerificationMode, ExternalBlockerFacts, GlobalContractCandidate,
    GoalContract, GoalStatus, LocalGoalContract, ObservedTerminalOutcome, ResourceId, RunId,
    UserId, WaitingCondition, WorkClaim, WorkItemId, WorkKind,
};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        AuthSession, ProjectAccess, ResourceAccess, require_project_access,
        require_resource_access, set_database_context,
    },
    error::AppError,
};

const SCHEDULER_AGING_STEP: u64 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRunRequest {
    id: RunId,
    source: RunContractSource,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RunContractSource {
    LocalGoal { id: Uuid, revision: u64 },
    GlobalContract { id: Uuid, revision: u64 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyIntent {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SucceedWorkRequest {
    outcome: Option<WorkOutcomeReference>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum WorkOutcomeReference {
    TaskCompletion { id: Uuid },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptEvidenceRequest {
    id: EvidenceId,
    rule_id: u64,
    work_item_id: WorkItemId,
    source: EvidenceSourceReference,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum EvidenceSourceReference {
    TaskCompletion { id: Uuid },
}

#[derive(Serialize)]
pub struct RunResponse {
    id: RunId,
    state_version: u64,
    state: CollaborativeRunState,
}

#[derive(Serialize)]
pub struct ClaimResponse {
    run_id: RunId,
    state_version: u64,
    claim: Option<WorkClaim>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBlockerRequest {
    waiting_rule_ordinal: u64,
    scope: BlockScope,
    condition: WaitingCondition,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolveBlockerRequest {
    HumanTaskTerminal { observation_id: Uuid },
    AdministratorDecision { observation_id: Uuid },
    PrincipalResponse { observation_id: Uuid },
    ExternalOutcome { observation_id: Uuid },
}

impl ResolveBlockerRequest {
    const fn observation(&self) -> (&'static str, Uuid) {
        match self {
            Self::HumanTaskTerminal { observation_id } => ("human_task_terminal", *observation_id),
            Self::AdministratorDecision { observation_id } => {
                ("administrator_decision", *observation_id)
            }
            Self::PrincipalResponse { observation_id } => ("principal_response", *observation_id),
            Self::ExternalOutcome { observation_id } => ("external_outcome", *observation_id),
        }
    }
}

#[derive(Serialize)]
pub struct BlockerResponse {
    run_id: RunId,
    blocker_id: BlockerId,
    status: BlockerStatus,
    state_version: u64,
}

struct LoadedContract {
    contract: GoalContract,
    source: PersistedSource,
    controller: Option<Uuid>,
}

enum PersistedSource {
    Local { id: Uuid, revision: i64 },
    Global { id: Uuid, revision: i64 },
}

struct LockedRun {
    contract: GoalContract,
    state: CollaborativeRunState,
    state_hash: Vec<u8>,
    state_version: i64,
}

struct PersistedTransition {
    id: Uuid,
    version: u64,
}

struct TransitionMetadata {
    kind: &'static str,
    tick: u64,
    observation: Option<(&'static str, Uuid)>,
}

struct ValidatedWorkOutcome {
    work_item_id: WorkItemId,
    claim_id: ClaimId,
    attempt: u16,
    product_event_id: Uuid,
    observed_datetime: DateTime<Utc>,
    provenance_hash: [u8; 32],
}

struct ValidatedEvidence {
    record: EvidenceRecord,
    verification: EvidenceVerificationMode,
    product_event_id: Uuid,
    observed_datetime: DateTime<Utc>,
    provenance_hash: [u8; 32],
}

#[derive(Serialize)]
struct AuthoritativeFactReferences {
    completed_tasks: Vec<ResourceId>,
    discharged_obligations: Vec<Uuid>,
    comment_authors: Vec<UserId>,
    administrator_approvals: Vec<AdministratorApprovalReference>,
}

#[derive(Serialize)]
struct AdministratorApprovalReference {
    administrator: UserId,
    review_work_spec_ordinal: u64,
}

pub async fn create(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    let loaded = load_requested_contract(&mut transaction, project_id, &request.source).await?;
    authorize_run_creation(&app, actor, project_id, &loaded).await?;
    let tick = runtime_tick()?;
    let empty = CollaborativeRunState {
        id: request.id,
        goal: loaded.contract.goal,
        scope: loaded.contract.scope,
        goal_status: GoalStatus::Active,
        run_status: CollaborativeRunStatus::Running,
        participants: HashSet::new(),
        obligations: Default::default(),
        work_slots: Default::default(),
        work_items: Default::default(),
        inactive_work_items: Default::default(),
        work_projection_history: Vec::new(),
        suspended_claim_resolutions: Default::default(),
        dispatches: Default::default(),
        claims: Default::default(),
        blockers: Default::default(),
        blocker_resolutions: Vec::new(),
        evidence: Vec::new(),
        causal_links: Vec::new(),
    };
    let facts =
        authoritative_condition_facts(&mut transaction, project_id, &loaded.contract, &empty)
            .await?;
    let state = CollaborativeRunState::initialize(request.id, &loaded.contract, &facts, tick)
        .map_err(domain_error)?;
    let contract_json = canonical_value(&loaded.contract)?;
    let state_json = canonical_value(&state)?;
    let contract_hash = digest_json(&contract_json)?;
    let state_hash = digest_json(&state_json)?;
    let fact_references = fact_references(&facts);
    let facts_hash = digest_json(&fact_references)?;
    let (local_id, local_revision, global_id, global_revision) = match loaded.source {
        PersistedSource::Local { id, revision } => (Some(id), Some(revision), None, None),
        PersistedSource::Global { id, revision } => (None, None, Some(id), Some(revision)),
    };
    sqlx::query(
        r#"
        INSERT INTO agent_collaborative_runs (
            id, project_id, goal_id, scope_resource_node_id,
            local_goal_id, local_goal_revision,
            global_contract_id, global_contract_revision,
            contract, contract_hash, state, state_hash,
            state_version, goal_status, run_status, created_by_identity_id
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8,
            $9, $10, $11, $12, 1, $13, $14, $15
        )
        "#,
    )
    .bind(Uuid::from(request.id))
    .bind(project_id)
    .bind(Uuid::from(state.goal))
    .bind(Uuid::from(state.scope))
    .bind(local_id)
    .bind(local_revision)
    .bind(global_id)
    .bind(global_revision)
    .bind(&contract_json)
    .bind(contract_hash.as_slice())
    .bind(&state_json)
    .bind(state_hash.as_slice())
    .bind(goal_status_name(state.goal_status))
    .bind(run_status_name(state.run_status))
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    persist_participants(
        &mut transaction,
        project_id,
        request.id,
        actor,
        loaded.controller,
        &state,
    )
    .await?;
    let transition_id = Uuid::new_v4();
    insert_transition(
        &mut transaction,
        transition_id,
        project_id,
        request.id,
        1,
        "initialized",
        Some(actor),
        None,
        &state_hash,
        &facts_hash,
        &state_json,
        &fact_references,
        None,
    )
    .await?;
    persist_kernel_certificates(&mut transaction, project_id, &state, transition_id, tick).await?;
    transaction.commit().await?;
    Ok(Json(RunResponse {
        id: request.id,
        state_version: 1,
        state,
    }))
}

pub async fn get(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RunResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    let row = sqlx::query(
        "SELECT state, state_version FROM agent_collaborative_runs
         WHERE project_id = $1 AND id = $2",
    )
    .bind(project_id)
    .bind(run_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let state: CollaborativeRunState =
        serde_json::from_value(row.try_get("state")?).map_err(|_| AppError::Internal)?;
    let state_version = positive_u64(row.try_get("state_version")?)?;
    transaction.commit().await?;
    Ok(Json(RunResponse {
        id: RunId::from(run_id),
        state_version,
        state,
    }))
}

pub async fn refresh(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id)): Path<(Uuid, Uuid)>,
    Json(_intent): Json<EmptyIntent>,
) -> Result<Json<RunResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&app.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = begin(&app, actor, project_id).await?;
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    let tick = runtime_tick()?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    locked.state.recover_expired_claims(tick);
    locked
        .state
        .refresh_frontier(&locked.contract, &facts, tick)
        .map_err(domain_error)?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: "frontier_refreshed",
            tick,
            observation: None,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(RunResponse {
        id: RunId::from(run_id),
        state_version: transition.version,
        state: locked.state,
    }))
}

pub async fn claim(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id)): Path<(Uuid, Uuid)>,
    Json(_intent): Json<EmptyIntent>,
) -> Result<Json<ClaimResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_authority(&mut transaction, project_id, actor, &locked.state).await?;
    let tick = runtime_tick()?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    locked.state.recover_expired_claims(tick);
    locked
        .state
        .refresh_frontier(&locked.contract, &facts, tick)
        .map_err(domain_error)?;
    let claim = locked
        .state
        .claim_next(
            &locked.contract,
            UserId::from(actor.identity_id),
            &facts,
            tick,
            app.config.agent_work_lease.as_secs(),
            SCHEDULER_AGING_STEP,
        )
        .map_err(domain_error)?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: if claim.is_some() {
                "work_claimed"
            } else {
                "frontier_refreshed"
            },
            tick,
            observation: None,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(ClaimResponse {
        run_id: RunId::from(run_id),
        state_version: transition.version,
        claim,
    }))
}

pub async fn succeed(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, claim_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<SucceedWorkRequest>,
) -> Result<Json<RunResponse>, AppError> {
    terminal_work(
        app,
        actor,
        project_id,
        run_id,
        claim_id,
        true,
        request.outcome,
    )
    .await
}

pub async fn fail(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, claim_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(_intent): Json<EmptyIntent>,
) -> Result<Json<RunResponse>, AppError> {
    terminal_work(app, actor, project_id, run_id, claim_id, false, None).await
}

async fn terminal_work(
    app: Arc<AppState>,
    actor: AuthSession,
    project_id: Uuid,
    run_id: Uuid,
    claim_id: Uuid,
    succeeded: bool,
    outcome_reference: Option<WorkOutcomeReference>,
) -> Result<Json<RunResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_authority(&mut transaction, project_id, actor, &locked.state).await?;
    let tick = runtime_tick()?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    locked.state.recover_expired_claims(tick);
    locked
        .state
        .refresh_frontier(&locked.contract, &facts, tick)
        .map_err(domain_error)?;
    let outcome = if succeeded {
        authoritative_work_outcome(
            &mut transaction,
            project_id,
            actor,
            &locked,
            ClaimId::from(claim_id),
            outcome_reference.as_ref(),
        )
        .await?
    } else {
        if outcome_reference.is_some() {
            return Err(AppError::BadRequest(
                "failed work cannot claim a successful product outcome",
            ));
        }
        None
    };
    if succeeded {
        locked
            .state
            .succeed_work(
                &locked.contract,
                ClaimId::from(claim_id),
                UserId::from(actor.identity_id),
                &facts,
                tick,
            )
            .map_err(domain_error)?;
    } else {
        locked
            .state
            .fail_work(
                &locked.contract,
                ClaimId::from(claim_id),
                UserId::from(actor.identity_id),
                &facts,
                tick,
            )
            .map_err(domain_error)?;
    }
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: if succeeded {
                "work_succeeded"
            } else {
                "work_failed"
            },
            tick,
            observation: outcome
                .as_ref()
                .map(|outcome| ("task_completion", outcome.product_event_id)),
        },
    )
    .await?;
    if let Some(outcome) = &outcome {
        persist_work_outcome(&mut transaction, project_id, run_id, outcome, transition.id).await?;
    }
    transaction.commit().await?;
    Ok(Json(RunResponse {
        id: RunId::from(run_id),
        state_version: transition.version,
        state: locked.state,
    }))
}

pub async fn accept_evidence(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<AcceptEvidenceRequest>,
) -> Result<Json<RunResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    if actor.is_agent {
        require_active_runner(&mut transaction, project_id, actor).await?;
    }
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_party(&mut transaction, project_id, actor, &locked.state).await?;
    let evidence_obligation = locked
        .contract
        .evidence_rules
        .iter()
        .find(|rule| rule.id == request.rule_id)
        .map(|rule| rule.obligation)
        .ok_or(AppError::Conflict)?;
    if locked
        .state
        .obligations
        .get(&evidence_obligation)
        .is_some_and(|obligation| {
            obligation.status == sprout_domain::agents::ObligationStatus::Discharged
        })
    {
        return Err(AppError::Conflict);
    }
    let evidence = authoritative_evidence(&mut transaction, project_id, &locked, &request).await?;
    locked
        .state
        .accept_evidence(
            &locked.contract,
            evidence.record.clone(),
            |record, rule| {
                record == &evidence.record
                    && rule.id == request.rule_id
                    && rule.verification == EvidenceVerificationMode::Mechanical
            },
            |_, _| false,
        )
        .map_err(domain_error)?;
    let refreshed_facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    let tick = runtime_tick()?.max(evidence.record.observed_at);
    locked
        .state
        .refresh_frontier(&locked.contract, &refreshed_facts, tick)
        .map_err(domain_error)?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &refreshed_facts,
        TransitionMetadata {
            kind: "evidence_accepted",
            tick,
            observation: Some(("task_completion", evidence.product_event_id)),
        },
    )
    .await?;
    persist_evidence_certificate(&mut transaction, project_id, &evidence, transition.id).await?;
    transaction.commit().await?;
    Ok(Json(RunResponse {
        id: RunId::from(run_id),
        state_version: transition.version,
        state: locked.state,
    }))
}

pub async fn complete(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id)): Path<(Uuid, Uuid)>,
    Json(_intent): Json<EmptyIntent>,
) -> Result<Json<RunResponse>, AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_project_access(&app.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = begin(&app, actor, project_id).await?;
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    let tick = runtime_tick()?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    locked.state.recover_expired_claims(tick);
    locked
        .state
        .refresh_frontier(&locked.contract, &facts, tick)
        .map_err(domain_error)?;
    locked.state.try_complete(&locked.contract, &facts);
    locked.state.complete_run().map_err(domain_error)?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: "run_completed",
            tick,
            observation: None,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(RunResponse {
        id: RunId::from(run_id),
        state_version: transition.version,
        state: locked.state,
    }))
}

pub async fn create_blocker(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateBlockerRequest>,
) -> Result<Json<BlockerResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    require_active_runner(&mut transaction, project_id, actor).await?;
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_authority(&mut transaction, project_id, actor, &locked.state).await?;
    let tick = runtime_tick()?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    locked.state.recover_expired_claims(tick);
    locked
        .state
        .refresh_frontier(&locked.contract, &facts, tick)
        .map_err(domain_error)?;
    let external_facts =
        authoritative_external_blocker_facts(&mut transaction, project_id, &request.condition)
            .await?;
    let blocker_id = locked
        .state
        .create_external_blocker(
            &locked.contract,
            request.waiting_rule_ordinal,
            request.scope,
            request.condition,
            tick,
            &external_facts,
        )
        .map_err(domain_error)?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: "blocker_created",
            tick,
            observation: None,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(BlockerResponse {
        run_id: RunId::from(run_id),
        blocker_id,
        status: BlockerStatus::Waiting,
        state_version: transition.version,
    }))
}

pub async fn resolve_blocker(
    State(app): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, run_id, blocker_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<ResolveBlockerRequest>,
) -> Result<Json<BlockerResponse>, AppError> {
    let mut transaction = begin(&app, actor, project_id).await?;
    if actor.is_agent {
        require_active_runner(&mut transaction, project_id, actor).await?;
    }
    let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
    require_current_run_party(&mut transaction, project_id, actor, &locked.state).await?;
    let blocker_id = BlockerId::from(blocker_id);
    let (observation, resolution_facts) = authoritative_blocker_resolution(
        &mut transaction,
        project_id,
        &locked.state,
        blocker_id,
        &request,
    )
    .await?;
    let facts = authoritative_condition_facts(
        &mut transaction,
        project_id,
        &locked.contract,
        &locked.state,
    )
    .await?;
    let status = locked
        .state
        .resolve_blocker(
            &locked.contract,
            blocker_id,
            observation,
            &resolution_facts,
            &facts,
        )
        .map_err(domain_error)?;
    let tick = locked
        .state
        .blockers
        .get(&blocker_id)
        .and_then(|blocker| blocker.terminal_at)
        .ok_or(AppError::Internal)?;
    let transition = persist_transition(
        &mut transaction,
        project_id,
        Some(actor),
        &locked,
        &facts,
        TransitionMetadata {
            kind: "blocker_resolved",
            tick,
            observation: Some(request.observation()),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(BlockerResponse {
        run_id: RunId::from(run_id),
        blocker_id,
        status,
        state_version: transition.version,
    }))
}

pub(crate) async fn recover_expired_claims(pool: &sqlx::PgPool) -> Result<u64, AppError> {
    let candidates = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT DISTINCT project_id, run_id
        FROM agent_run_claim_leases
        WHERE status = 'active' AND expires_at <= clock_timestamp()
        ORDER BY project_id, run_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    let mut recovered = 0_u64;
    for (project_id, run_id) in candidates {
        let mut transaction = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *transaction)
            .await?;
        let mut locked = lock_run(&mut transaction, project_id, run_id).await?;
        let before = locked
            .state
            .claims
            .values()
            .filter(|claim| matches!(claim.status, sprout_domain::agents::ClaimStatus::Active))
            .count();
        let tick = runtime_tick()?;
        let facts = authoritative_condition_facts(
            &mut transaction,
            project_id,
            &locked.contract,
            &locked.state,
        )
        .await?;
        locked.state.recover_expired_claims(tick);
        locked
            .state
            .refresh_frontier(&locked.contract, &facts, tick)
            .map_err(domain_error)?;
        let after = locked
            .state
            .claims
            .values()
            .filter(|claim| matches!(claim.status, sprout_domain::agents::ClaimStatus::Active))
            .count();
        if after < before {
            persist_transition(
                &mut transaction,
                project_id,
                None,
                &locked,
                &facts,
                TransitionMetadata {
                    kind: "claim_recovered",
                    tick,
                    observation: None,
                },
            )
            .await?;
            recovered = recovered.saturating_add(u64::try_from(before - after).unwrap_or(u64::MAX));
        }
        transaction.commit().await?;
    }
    Ok(recovered)
}

async fn load_requested_contract(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    source: &RunContractSource,
) -> Result<LoadedContract, AppError> {
    match source {
        RunContractSource::LocalGoal { id, revision } => {
            let revision = to_i64(*revision)?;
            let row = sqlx::query(
                r#"
                SELECT contract, controller_identity_id
                FROM agent_local_goal_contracts
                WHERE project_id = $1 AND id = $2 AND revision = $3
                  AND state = 'active'
                "#,
            )
            .bind(project_id)
            .bind(id)
            .bind(revision)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::Conflict)?;
            let local: LocalGoalContract =
                serde_json::from_value(row.try_get("contract")?).map_err(|_| AppError::Internal)?;
            local.validate().map_err(domain_error)?;
            Ok(LoadedContract {
                contract: local.contract,
                source: PersistedSource::Local { id: *id, revision },
                controller: Some(row.try_get("controller_identity_id")?),
            })
        }
        RunContractSource::GlobalContract { id, revision } => {
            let revision = to_i64(*revision)?;
            let row = sqlx::query(
                r#"
                SELECT candidate
                FROM agent_global_contracts current
                WHERE current.project_id = $1 AND current.id = $2 AND current.revision = $3
                  AND NOT EXISTS (
                      SELECT 1 FROM agent_global_contracts newer
                      WHERE newer.project_id = current.project_id
                        AND newer.id = current.id
                        AND newer.revision > current.revision
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM agent_global_contract_sources source
                      JOIN agent_local_goal_contracts local
                        ON local.project_id = source.project_id
                       AND local.id = source.local_goal_id
                       AND local.revision = source.local_revision
                       AND local.agent_id = source.agent_id
                      WHERE source.project_id = current.project_id
                        AND source.global_contract_id = current.id
                        AND source.global_revision = current.revision
                        AND local.state <> 'active'
                  )
                "#,
            )
            .bind(project_id)
            .bind(id)
            .bind(revision)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::Conflict)?;
            let global: GlobalContractCandidate = serde_json::from_value(row.try_get("candidate")?)
                .map_err(|_| AppError::Internal)?;
            global.contract.validate().map_err(domain_error)?;
            Ok(LoadedContract {
                contract: global.contract,
                source: PersistedSource::Global { id: *id, revision },
                controller: None,
            })
        }
    }
}

async fn authorize_run_creation(
    app: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    loaded: &LoadedContract,
) -> Result<(), AppError> {
    if actor.is_agent {
        return Err(AppError::Forbidden);
    }
    require_resource_access(
        &app.pool,
        actor,
        project_id,
        Uuid::from(loaded.contract.scope),
        ResourceAccess::Write,
    )
    .await?;
    match loaded.source {
        PersistedSource::Local { .. } if loaded.controller == Some(actor.identity_id) => Ok(()),
        PersistedSource::Global { .. } => {
            require_project_access(&app.pool, actor, project_id, ProjectAccess::Manage).await
        }
        _ => Err(AppError::Forbidden),
    }
}

async fn lock_run(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
) -> Result<LockedRun, AppError> {
    let row = sqlx::query(
        r#"
        SELECT contract, state, state_hash, state_version
        FROM agent_collaborative_runs
        WHERE project_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .bind(run_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let contract: GoalContract =
        serde_json::from_value(row.try_get("contract")?).map_err(|_| AppError::Internal)?;
    contract.validate().map_err(|_| AppError::Internal)?;
    let state: CollaborativeRunState =
        serde_json::from_value(row.try_get("state")?).map_err(|_| AppError::Internal)?;
    Ok(LockedRun {
        contract,
        state,
        state_hash: row.try_get("state_hash")?,
        state_version: row.try_get("state_version")?,
    })
}

async fn authoritative_work_outcome(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: AuthSession,
    locked: &LockedRun,
    claim_id: ClaimId,
    requested: Option<&WorkOutcomeReference>,
) -> Result<Option<ValidatedWorkOutcome>, AppError> {
    let claim = locked
        .state
        .claims
        .get(&claim_id)
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
        .find(|spec| spec.id == work.work_spec_id)
        .ok_or(AppError::Internal)?;
    match (spec.kind, requested) {
        (WorkKind::TaskAction, Some(WorkOutcomeReference::TaskCompletion { id })) => {
            let row = sqlx::query(
                r#"
                SELECT completion.assignee_identity_id, completion.completed_at,
                       task.resource_node_id
                FROM task_completions completion
                JOIN tasks task
                  ON task.project_id = completion.project_id
                 AND task.id = completion.task_id
                 AND task.state = 'completed'
                 AND task.deleted_at IS NULL
                JOIN agent_run_work_product_bindings binding
                  ON binding.project_id = completion.project_id
                 AND binding.run_id = $4
                 AND binding.work_item_id = $5
                 AND binding.claim_id = $6
                 AND binding.attempt = $7
                 AND binding.resource_node_id = task.resource_node_id
                JOIN agent_effect_proposals effect
                  ON effect.id = binding.effect_id
                 AND effect.project_id = binding.project_id
                 AND effect.invocation_id = binding.invocation_id
                 AND effect.status = 'applied'
                 AND effect.effect #>> '{effect,resource_id}' = task.resource_node_id::text
                 AND effect.effect #>> '{effect,operation}' = 'complete_assigned_task'
                JOIN agent_invocations invocation
                  ON invocation.project_id = effect.project_id
                 AND invocation.id = effect.invocation_id
                 AND invocation.agent_identity_id = completion.assignee_identity_id
                 AND invocation.status = 'succeeded'
                JOIN resource_closure scope
                  ON scope.project_id = task.project_id
                 AND scope.ancestor_id = $3
                 AND scope.descendant_id = task.resource_node_id
                WHERE completion.project_id = $1 AND completion.id = $2
                FOR SHARE OF completion, task
                "#,
            )
            .bind(project_id)
            .bind(id)
            .bind(Uuid::from(locked.state.scope))
            .bind(Uuid::from(locked.state.id))
            .bind(Uuid::from(work.id))
            .bind(Uuid::from(claim.id))
            .bind(i32::from(claim.attempt))
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::Conflict)?;
            let assignee: Uuid = row.try_get("assignee_identity_id")?;
            let observed_datetime: DateTime<Utc> = row.try_get("completed_at")?;
            let observed_at = datetime_tick(observed_datetime)?;
            if assignee != actor.identity_id
                || claim.claimant != UserId::from(actor.identity_id)
                || observed_at < claim.acquired_at
                || observed_at > runtime_tick()?
            {
                return Err(AppError::Conflict);
            }
            let task_resource_id = ResourceId::from(row.try_get::<Uuid, _>("resource_node_id")?);
            let provenance_hash = digest_json(&serde_json::json!({
                "project_id": project_id,
                "run_id": locked.state.id,
                "work_item_id": work.id,
                "claim_id": claim.id,
                "attempt": claim.attempt,
                "outcome_kind": "task_completion",
                "product_event_id": id,
                "task_resource_id": task_resource_id,
                "assignee_identity_id": assignee,
                "observed_at": observed_datetime,
            }))?;
            Ok(Some(ValidatedWorkOutcome {
                work_item_id: work.id,
                claim_id: claim.id,
                attempt: claim.attempt,
                product_event_id: *id,
                observed_datetime,
                provenance_hash,
            }))
        }
        (WorkKind::TaskAction, None) => Err(AppError::BadRequest(
            "task work requires an authoritative task-completion outcome",
        )),
        (_, Some(_)) => Err(AppError::BadRequest(
            "product outcome does not match the work kind",
        )),
        (_, None) => Ok(None),
    }
}

async fn authoritative_evidence(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    locked: &LockedRun,
    request: &AcceptEvidenceRequest,
) -> Result<ValidatedEvidence, AppError> {
    let rule = locked
        .contract
        .evidence_rules
        .iter()
        .find(|rule| rule.id == request.rule_id)
        .ok_or(AppError::Conflict)?;
    let ContractEvidenceSubject::WorkResult { work_spec_id } = rule.subject else {
        return Err(AppError::BadRequest(
            "evidence subject product-event adapter is not available",
        ));
    };
    if rule.kind != EvidenceKind::TaskCompleted
        || rule.verification != EvidenceVerificationMode::Mechanical
    {
        return Err(AppError::BadRequest(
            "evidence verification adapter is not available",
        ));
    }
    let EvidenceSourceReference::TaskCompletion { id } = request.source;
    let row = sqlx::query(
        r#"
        SELECT outcome.observed_at, outcome.provenance_hash,
               task.resource_node_id
        FROM agent_run_work_outcomes outcome
        JOIN task_completions completion
          ON completion.project_id = outcome.project_id
         AND completion.id = outcome.product_event_id
         AND completion.completed_at = outcome.observed_at
        JOIN tasks task
          ON task.project_id = completion.project_id
         AND task.id = completion.task_id
         AND task.state = 'completed'
        WHERE outcome.project_id = $1 AND outcome.run_id = $2
          AND outcome.work_item_id = $3
          AND outcome.outcome_kind = 'task_completion'
          AND outcome.product_event_id = $4
        FOR SHARE OF outcome, completion, task
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(locked.state.id))
    .bind(Uuid::from(request.work_item_id))
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::Conflict)?;
    let work = locked
        .state
        .work_items
        .get(&request.work_item_id)
        .ok_or(AppError::Conflict)?;
    if work.work_spec_id != work_spec_id
        || work.status != sprout_domain::agents::WorkStatus::Succeeded
    {
        return Err(AppError::Conflict);
    }
    let observed_datetime: DateTime<Utc> = row.try_get("observed_at")?;
    let observed_at = datetime_tick(observed_datetime)?;
    let task_resource_id = ResourceId::from(row.try_get::<Uuid, _>("resource_node_id")?);
    let outcome_hash: Vec<u8> = row.try_get("provenance_hash")?;
    let provenance_hash = digest_json(&serde_json::json!({
        "project_id": project_id,
        "run_id": locked.state.id,
        "evidence_id": request.id,
        "rule_id": rule.id,
        "obligation_id": rule.obligation,
        "work_item_id": request.work_item_id,
        "product_event_kind": "task_completion",
        "product_event_id": id,
        "outcome_provenance_hash": hex::encode(outcome_hash),
        "observed_at": observed_datetime,
    }))?;
    Ok(ValidatedEvidence {
        record: EvidenceRecord {
            id: request.id,
            run: locked.state.id,
            obligation: rule.obligation,
            rule_id: rule.id,
            kind: rule.kind,
            subject: EvidenceSubject::Task {
                task: task_resource_id,
            },
            work: Some(request.work_item_id),
            observed_at,
            provenance_hash,
        },
        verification: rule.verification,
        product_event_id: id,
        observed_datetime,
        provenance_hash,
    })
}

async fn authoritative_condition_facts(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    contract: &GoalContract,
    state: &CollaborativeRunState,
) -> Result<ContractConditionFacts, AppError> {
    let mut referenced_tasks = HashSet::new();
    let mut referenced_comments = HashSet::new();
    let mut referenced_approvals = HashSet::new();
    collect_contract_references(
        &contract.completion_condition,
        &mut referenced_tasks,
        &mut referenced_comments,
        &mut referenced_approvals,
    );
    for obligation in &contract.obligations {
        collect_contract_references(
            &obligation.activation,
            &mut referenced_tasks,
            &mut referenced_comments,
            &mut referenced_approvals,
        );
        collect_contract_references(
            &obligation.required_for_completion,
            &mut referenced_tasks,
            &mut referenced_comments,
            &mut referenced_approvals,
        );
    }
    for work in &contract.work_specs {
        collect_contract_references(
            &work.activation,
            &mut referenced_tasks,
            &mut referenced_comments,
            &mut referenced_approvals,
        );
    }
    let task_ids: Vec<Uuid> = referenced_tasks.iter().copied().map(Uuid::from).collect();
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
    // No comments or administrator decisions are inferred from client
    // metadata. Until their typed product-event adapters are queried below,
    // these conditions remain false (fail closed).
    let _ = (referenced_comments, referenced_approvals);
    Ok(ContractConditionFacts {
        completed_tasks,
        discharged_obligations: state
            .obligations
            .iter()
            .filter_map(|(id, obligation)| {
                matches!(
                    obligation.status,
                    sprout_domain::agents::ObligationStatus::Discharged
                )
                .then_some(*id)
            })
            .collect(),
        comment_authors: HashSet::new(),
        administrator_approvals: HashSet::new(),
    })
}

async fn authoritative_external_blocker_facts(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    condition: &WaitingCondition,
) -> Result<ExternalBlockerFacts, AppError> {
    let mut facts = ExternalBlockerFacts::default();
    match condition {
        WaitingCondition::PrincipalResponse { principal } => {
            let is_human = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                    FROM project_memberships membership
                    JOIN identities identity ON identity.id = membership.identity_id
                    WHERE membership.project_id = $1 AND membership.identity_id = $2
                      AND membership.state = 'active' AND identity.status = 'active'
                      AND identity.principal_kind = 'user'
                )
                "#,
            )
            .bind(project_id)
            .bind(Uuid::from(*principal))
            .fetch_one(&mut **transaction)
            .await?;
            if is_human {
                facts.human_principals.insert(*principal);
            }
        }
        WaitingCondition::AdministratorApproval { administrator } => {
            let is_administrator = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                    FROM project_memberships membership
                    JOIN identities identity ON identity.id = membership.identity_id
                    WHERE membership.project_id = $1 AND membership.identity_id = $2
                      AND membership.state = 'active' AND identity.status = 'active'
                      AND identity.principal_kind = 'user'
                      AND membership.role IN ('owner', 'admin')
                )
                "#,
            )
            .bind(project_id)
            .bind(Uuid::from(*administrator))
            .fetch_one(&mut **transaction)
            .await?;
            if is_administrator {
                facts.administrators.insert(*administrator);
            }
        }
        WaitingCondition::HumanTaskCompleted { task } => {
            let is_human_assigned = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                    FROM tasks task
                    JOIN task_assignments assignment
                      ON assignment.project_id = task.project_id
                     AND assignment.task_id = task.id
                     AND assignment.revoked_at IS NULL
                    JOIN identities identity ON identity.id = assignment.assignee_identity_id
                    WHERE task.project_id = $1 AND task.resource_node_id = $2
                      AND task.deleted_at IS NULL
                      AND identity.status = 'active'
                      AND identity.principal_kind = 'user'
                )
                "#,
            )
            .bind(project_id)
            .bind(Uuid::from(*task))
            .fetch_one(&mut **transaction)
            .await?;
            if is_human_assigned {
                facts.human_assigned_tasks.insert(*task);
            }
        }
        WaitingCondition::ExternalOutcome { .. } => {}
    }
    Ok(facts)
}

async fn authoritative_blocker_resolution(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    state: &CollaborativeRunState,
    blocker_id: BlockerId,
    request: &ResolveBlockerRequest,
) -> Result<(BlockerResolutionObservation, BlockerResolutionFacts), AppError> {
    let blocker = state.blockers.get(&blocker_id).ok_or(AppError::NotFound)?;
    match (&blocker.condition, request) {
        (
            WaitingCondition::HumanTaskCompleted { task },
            ResolveBlockerRequest::HumanTaskTerminal { observation_id },
        ) if Uuid::from(*task) == *observation_id => {
            let row = sqlx::query(
                r#"
                SELECT state, COALESCE(completed_at, updated_at) AS observed_at
                FROM tasks
                WHERE project_id = $1 AND resource_node_id = $2 AND deleted_at IS NULL
                  AND state IN ('completed', 'cancelled')
                FOR SHARE
                "#,
            )
            .bind(project_id)
            .bind(observation_id)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AppError::Conflict)?;
            let outcome = match row.try_get::<String, _>("state")?.as_str() {
                "completed" => ObservedTerminalOutcome::Succeeded,
                "cancelled" => ObservedTerminalOutcome::Cancelled,
                _ => return Err(AppError::Conflict),
            };
            let observed_at = datetime_tick(row.try_get("observed_at")?)?;
            let mut facts = BlockerResolutionFacts::default();
            facts
                .terminal_human_tasks
                .insert(*task, (outcome, observed_at));
            Ok((
                BlockerResolutionObservation::HumanTaskTerminal {
                    blocker: blocker_id,
                    task: *task,
                    outcome,
                    observed_at,
                },
                facts,
            ))
        }
        (
            WaitingCondition::AdministratorApproval { .. },
            ResolveBlockerRequest::AdministratorDecision { .. },
        ) => Err(AppError::BadRequest(
            "administrator decision product-event adapter is not available",
        )),
        (
            WaitingCondition::PrincipalResponse { .. },
            ResolveBlockerRequest::PrincipalResponse { .. },
        ) => Err(AppError::BadRequest(
            "principal response product-event adapter is not available",
        )),
        (
            WaitingCondition::ExternalOutcome { .. },
            ResolveBlockerRequest::ExternalOutcome { .. },
        ) => Err(AppError::BadRequest(
            "external outcome product-event adapter is not available",
        )),
        _ => Err(AppError::BadRequest(
            "observation does not match blocker waiting condition",
        )),
    }
}

fn collect_contract_references(
    condition: &ContractCondition,
    tasks: &mut HashSet<ResourceId>,
    comments: &mut HashSet<UserId>,
    approvals: &mut HashSet<(UserId, u64)>,
) {
    match condition {
        ContractCondition::TaskDone { task } => {
            tasks.insert(*task);
        }
        ContractCondition::CommentBy { principal } => {
            comments.insert(*principal);
        }
        ContractCondition::AdministratorApproved {
            administrator,
            review_work_spec_id,
        } => {
            approvals.insert((*administrator, *review_work_spec_id));
        }
        ContractCondition::All { left, right } | ContractCondition::Any { left, right } => {
            collect_contract_references(left, tasks, comments, approvals);
            collect_contract_references(right, tasks, comments, approvals);
        }
        ContractCondition::Neg { condition } => {
            collect_contract_references(condition, tasks, comments, approvals);
        }
        ContractCondition::Always {}
        | ContractCondition::Never {}
        | ContractCondition::ObligationDone { .. } => {}
    }
}

async fn persist_transition(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: Option<AuthSession>,
    locked: &LockedRun,
    facts: &ContractConditionFacts,
    metadata: TransitionMetadata,
) -> Result<PersistedTransition, AppError> {
    let next_version = locked
        .state_version
        .checked_add(1)
        .ok_or(AppError::Internal)?;
    let state_json = canonical_value(&locked.state)?;
    let state_hash = digest_json(&state_json)?;
    let fact_references = fact_references(facts);
    let facts_hash = digest_json(&fact_references)?;
    let updated = sqlx::query(
        r#"
        UPDATE agent_collaborative_runs
        SET state = $4, state_hash = $5, state_version = $6,
            goal_status = $7, run_status = $8,
            updated_at = clock_timestamp()
        WHERE project_id = $1 AND id = $2 AND state_version = $3
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(locked.state.id))
    .bind(locked.state_version)
    .bind(&state_json)
    .bind(state_hash.as_slice())
    .bind(next_version)
    .bind(goal_status_name(locked.state.goal_status))
    .bind(run_status_name(locked.state.run_status))
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let transition_id = Uuid::new_v4();
    insert_transition(
        transaction,
        transition_id,
        project_id,
        locked.state.id,
        next_version,
        metadata.kind,
        actor,
        Some(&locked.state_hash),
        &state_hash,
        &facts_hash,
        &state_json,
        &fact_references,
        metadata.observation,
    )
    .await?;
    persist_kernel_certificates(
        transaction,
        project_id,
        &locked.state,
        transition_id,
        metadata.tick,
    )
    .await?;
    Ok(PersistedTransition {
        id: transition_id,
        version: positive_u64(next_version)?,
    })
}

async fn persist_work_outcome(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: Uuid,
    outcome: &ValidatedWorkOutcome,
    transition_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO agent_run_work_outcomes (
            project_id, run_id, work_item_id, claim_id, attempt,
            outcome_kind, product_event_id, observed_at,
            provenance_hash, transition_id
        ) VALUES ($1, $2, $3, $4, $5, 'task_completion', $6, $7, $8, $9)
        "#,
    )
    .bind(project_id)
    .bind(run_id)
    .bind(Uuid::from(outcome.work_item_id))
    .bind(Uuid::from(outcome.claim_id))
    .bind(i32::from(outcome.attempt))
    .bind(outcome.product_event_id)
    .bind(outcome.observed_datetime)
    .bind(outcome.provenance_hash.as_slice())
    .bind(transition_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn persist_evidence_certificate(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    evidence: &ValidatedEvidence,
    transition_id: Uuid,
) -> Result<(), AppError> {
    let work_item_id = evidence.record.work.ok_or(AppError::Internal)?;
    sqlx::query(
        r#"
        INSERT INTO agent_run_evidence_provenance (
            evidence_id, project_id, run_id, obligation_id, work_item_id,
            evidence_rule_ordinal, evidence_kind, verification_mode,
            product_event_kind, product_event_id, observed_at,
            provenance_hash, transition_id
        ) VALUES (
            $1, $2, $3, $4, $5, $6, 'task_completed', $7,
            'task_completion', $8, $9, $10, $11
        )
        "#,
    )
    .bind(Uuid::from(evidence.record.id))
    .bind(project_id)
    .bind(Uuid::from(evidence.record.run))
    .bind(evidence.record.obligation)
    .bind(Uuid::from(work_item_id))
    .bind(to_i64(evidence.record.rule_id)?)
    .bind(evidence_verification_name(evidence.verification))
    .bind(evidence.product_event_id)
    .bind(evidence.observed_datetime)
    .bind(evidence.provenance_hash.as_slice())
    .bind(transition_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn persist_kernel_certificates(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    state: &CollaborativeRunState,
    transition_id: Uuid,
    tick: u64,
) -> Result<(), AppError> {
    for ((work_spec_ordinal, slot), work_item_id) in &state.work_slots {
        sqlx::query(
            r#"
            INSERT INTO agent_run_work_slots (
                project_id, run_id, work_spec_ordinal, slot, work_item_id
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (project_id, run_id, work_spec_ordinal, slot) DO NOTHING
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(state.id))
        .bind(to_i64(*work_spec_ordinal)?)
        .bind(i32::try_from(*slot).map_err(|_| AppError::Internal)?)
        .bind(Uuid::from(*work_item_id))
        .execute(&mut **transaction)
        .await?;
        let canonical = sqlx::query_scalar::<_, Uuid>(
            "SELECT work_item_id FROM agent_run_work_slots
             WHERE project_id = $1 AND run_id = $2
               AND work_spec_ordinal = $3 AND slot = $4",
        )
        .bind(project_id)
        .bind(Uuid::from(state.id))
        .bind(to_i64(*work_spec_ordinal)?)
        .bind(i32::try_from(*slot).map_err(|_| AppError::Internal)?)
        .fetch_one(&mut **transaction)
        .await?;
        if canonical != Uuid::from(*work_item_id) {
            return Err(AppError::Conflict);
        }
    }
    for claim in state
        .claims
        .values()
        .filter(|claim| !matches!(claim.status, sprout_domain::agents::ClaimStatus::Active))
    {
        sqlx::query(
            "UPDATE agent_run_claim_leases
             SET status = $3, terminal_at = COALESCE(terminal_at, $4)
             WHERE project_id = $1 AND id = $2",
        )
        .bind(project_id)
        .bind(Uuid::from(claim.id))
        .bind(claim_status_name(claim.status))
        .bind(tick_datetime(tick)?)
        .execute(&mut **transaction)
        .await?;
    }
    for claim in state.claims.values() {
        let terminal_at = (!matches!(claim.status, sprout_domain::agents::ClaimStatus::Active))
            .then_some(tick_datetime(tick)?);
        sqlx::query(
            r#"
            INSERT INTO agent_run_claim_leases (
                id, project_id, run_id, work_item_id, attempt,
                claimant_identity_id, acquired_at, expires_at, status, terminal_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (project_id, id) DO UPDATE
            SET status = EXCLUDED.status,
                terminal_at = COALESCE(agent_run_claim_leases.terminal_at, EXCLUDED.terminal_at)
            "#,
        )
        .bind(Uuid::from(claim.id))
        .bind(project_id)
        .bind(Uuid::from(state.id))
        .bind(Uuid::from(claim.work))
        .bind(i32::from(claim.attempt))
        .bind(Uuid::from(claim.claimant))
        .bind(tick_datetime(claim.acquired_at)?)
        .bind(tick_datetime(claim.expires_at)?)
        .bind(claim_status_name(claim.status))
        .bind(terminal_at)
        .execute(&mut **transaction)
        .await?;
    }
    for resolution in &state.blocker_resolutions {
        let (observation_kind, observation_id) =
            blocker_observation_reference(&resolution.observation);
        let provenance_hash = digest_json(&resolution.observation)?;
        sqlx::query(
            r#"
            INSERT INTO agent_run_blocker_resolutions (
                project_id, run_id, blocker_id, observation_kind,
                observation_id, terminal_status, observed_at,
                provenance_hash, transition_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (project_id, run_id, blocker_id) DO NOTHING
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(state.id))
        .bind(Uuid::from(resolution.blocker))
        .bind(observation_kind)
        .bind(observation_id)
        .bind(blocker_status_name(resolution.terminal_status))
        .bind(tick_datetime(resolution.observed_at)?)
        .bind(provenance_hash.as_slice())
        .bind(transition_id)
        .execute(&mut **transaction)
        .await?;
    }
    for blocker in state.blockers.values() {
        sqlx::query(
            r#"
            INSERT INTO agent_run_blockers (
                id, project_id, run_id, obligation_id, waiting_rule_ordinal,
                scope, waiting_condition, current_status, created_tick, terminal_tick
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (project_id, id) DO UPDATE
            SET current_status = EXCLUDED.current_status,
                terminal_tick = EXCLUDED.terminal_tick
            "#,
        )
        .bind(Uuid::from(blocker.id))
        .bind(project_id)
        .bind(Uuid::from(state.id))
        .bind(blocker.obligation)
        .bind(to_i64(blocker.waiting_rule_id)?)
        .bind(canonical_value(&blocker.scope)?)
        .bind(canonical_value(&blocker.condition)?)
        .bind(blocker_status_name(blocker.status))
        .bind(to_i64(blocker.created_at)?)
        .bind(blocker.terminal_at.map(to_i64).transpose()?)
        .execute(&mut **transaction)
        .await?;
    }
    for link in &state.causal_links {
        sqlx::query(
            r#"
            INSERT INTO agent_run_causal_links (
                project_id, run_id, goal_id, predecessor, successor,
                observed_tick, transition_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (project_id, run_id, predecessor, successor) DO NOTHING
            "#,
        )
        .bind(project_id)
        .bind(Uuid::from(state.id))
        .bind(Uuid::from(state.goal))
        .bind(canonical_value(&link.predecessor)?)
        .bind(canonical_value(&link.successor)?)
        .bind(to_i64(link.observed_at)?)
        .bind(transition_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_transition(
    transaction: &mut Transaction<'_, Postgres>,
    transition_id: Uuid,
    project_id: Uuid,
    run_id: RunId,
    state_version: i64,
    kind: &'static str,
    actor: Option<AuthSession>,
    previous_hash: Option<&[u8]>,
    next_hash: &[u8; 32],
    facts_hash: &[u8; 32],
    state_snapshot: &serde_json::Value,
    fact_references: &AuthoritativeFactReferences,
    observation: Option<(&'static str, Uuid)>,
) -> Result<(), AppError> {
    let (observation_kind, observation_id) = observation.unzip();
    let actor_identity_id = actor.map(|value| value.identity_id);
    let actor_device_id = actor
        .filter(|value| value.device_id != Uuid::nil())
        .map(|value| value.device_id);
    sqlx::query(
        r#"
        INSERT INTO agent_run_transitions (
            id, project_id, run_id, state_version, transition_kind,
            runtime_actor_kind, actor_identity_id, actor_device_id,
            observation_kind, observation_id,
            previous_state_hash, next_state_hash, facts_hash,
            state_snapshot, fact_references
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
            $11, $12, $13, $14, $15
        )
        "#,
    )
    .bind(transition_id)
    .bind(project_id)
    .bind(Uuid::from(run_id))
    .bind(state_version)
    .bind(kind)
    .bind(if actor.is_some() {
        "principal"
    } else {
        "scheduler"
    })
    .bind(actor_identity_id)
    .bind(actor_device_id)
    .bind(observation_kind)
    .bind(observation_id)
    .bind(previous_hash)
    .bind(next_hash.as_slice())
    .bind(facts_hash.as_slice())
    .bind(state_snapshot)
    .bind(canonical_value(fact_references)?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn persist_participants(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    run_id: RunId,
    actor: AuthSession,
    controller: Option<Uuid>,
    state: &CollaborativeRunState,
) -> Result<(), AppError> {
    for participant in &state.participants {
        sqlx::query(
            "INSERT INTO agent_run_participants (
                 project_id, run_id, identity_id, participant_role
             ) VALUES ($1, $2, $3, 'agent')",
        )
        .bind(project_id)
        .bind(Uuid::from(run_id))
        .bind(Uuid::from(*participant))
        .execute(&mut **transaction)
        .await?;
    }
    if let Some(controller) = controller {
        sqlx::query(
            "INSERT INTO agent_run_participants (
                 project_id, run_id, identity_id, participant_role
             ) VALUES ($1, $2, $3, 'controller')
             ON CONFLICT DO NOTHING",
        )
        .bind(project_id)
        .bind(Uuid::from(run_id))
        .bind(controller)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO agent_run_participants (
             project_id, run_id, identity_id, participant_role
         ) VALUES ($1, $2, $3, 'sponsor')
         ON CONFLICT DO NOTHING",
    )
    .bind(project_id)
    .bind(Uuid::from(run_id))
    .bind(actor.identity_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn require_active_runner(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: AuthSession,
) -> Result<(), AppError> {
    if !actor.is_agent {
        return Err(AppError::Forbidden);
    }
    let active_runner = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT runner.id
        FROM governed_agents agent
        JOIN agent_runners runner
          ON runner.project_id = agent.project_id AND runner.agent_id = agent.id
        JOIN device_keys key
          ON key.identity_id = runner.principal_identity_id
         AND key.device_id = runner.device_id
         AND key.key_version = runner.activated_key_version
        WHERE agent.project_id = $1
          AND agent.principal_identity_id = $2
          AND agent.state = 'active'
          AND runner.device_id = $3
          AND runner.state = 'active'
          AND key.revoked_at IS NULL
        LIMIT 1
        FOR SHARE OF agent, runner, key
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if active_runner.is_some() {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn require_current_run_authority(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: AuthSession,
    state: &CollaborativeRunState,
) -> Result<(), AppError> {
    if !state
        .participants
        .contains(&UserId::from(actor.identity_id))
    {
        return Err(AppError::Forbidden);
    }
    let current_scope_access =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.can_access_resource($1, $2, 'read')")
            .bind(project_id)
            .bind(Uuid::from(state.scope))
            .fetch_one(&mut **transaction)
            .await?;
    if current_scope_access {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn require_current_run_party(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: AuthSession,
    state: &CollaborativeRunState,
) -> Result<(), AppError> {
    if actor.is_agent {
        return require_current_run_authority(transaction, project_id, actor, state).await;
    }
    let is_party = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM agent_run_participants
            WHERE project_id = $1 AND run_id = $2 AND identity_id = $3
        ) AND sprout_private.can_access_resource($1, $4, 'read')
        "#,
    )
    .bind(project_id)
    .bind(Uuid::from(state.id))
    .bind(actor.identity_id)
    .bind(Uuid::from(state.scope))
    .fetch_one(&mut **transaction)
    .await?;
    if is_party {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn begin<'a>(
    app: &'a AppState,
    actor: AuthSession,
    project_id: Uuid,
) -> Result<Transaction<'a, Postgres>, AppError> {
    let mut transaction = app.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    Ok(transaction)
}

fn fact_references(facts: &ContractConditionFacts) -> AuthoritativeFactReferences {
    let mut completed_tasks: Vec<_> = facts.completed_tasks.iter().copied().collect();
    completed_tasks.sort();
    let mut discharged_obligations: Vec<_> = facts.discharged_obligations.iter().copied().collect();
    discharged_obligations.sort();
    let mut comment_authors: Vec<_> = facts.comment_authors.iter().copied().collect();
    comment_authors.sort();
    let mut administrator_approvals: Vec<_> = facts
        .administrator_approvals
        .iter()
        .map(
            |(administrator, review_work_spec_ordinal)| AdministratorApprovalReference {
                administrator: *administrator,
                review_work_spec_ordinal: *review_work_spec_ordinal,
            },
        )
        .collect();
    administrator_approvals.sort_by_key(|item| (item.administrator, item.review_work_spec_ordinal));
    AuthoritativeFactReferences {
        completed_tasks,
        discharged_obligations,
        comment_authors,
        administrator_approvals,
    }
}

fn canonical_value(value: &impl Serialize) -> Result<serde_json::Value, AppError> {
    serde_json::to_value(value).map_err(|_| AppError::Internal)
}

fn digest_json(value: &impl Serialize) -> Result<[u8; 32], AppError> {
    let encoded = serde_json::to_vec(value).map_err(|_| AppError::Internal)?;
    Ok(Sha256::digest(encoded).into())
}

fn runtime_tick() -> Result<u64, AppError> {
    u64::try_from(Utc::now().timestamp()).map_err(|_| AppError::Internal)
}

fn tick_datetime(tick: u64) -> Result<DateTime<Utc>, AppError> {
    let seconds = i64::try_from(tick).map_err(|_| AppError::Internal)?;
    Utc.timestamp_opt(seconds, 0)
        .single()
        .ok_or(AppError::Internal)
}

fn datetime_tick(value: DateTime<Utc>) -> Result<u64, AppError> {
    u64::try_from(value.timestamp()).map_err(|_| AppError::Internal)
}

fn to_i64(value: u64) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| AppError::BadRequest("numeric value is out of range"))
}

fn positive_u64(value: i64) -> Result<u64, AppError> {
    u64::try_from(value).map_err(|_| AppError::Internal)
}

fn goal_status_name(status: GoalStatus) -> &'static str {
    match status {
        GoalStatus::Active => "active",
        GoalStatus::Completed => "completed",
        GoalStatus::Failed => "failed",
        GoalStatus::Cancelled => "cancelled",
        GoalStatus::Superseded => "superseded",
    }
}

fn run_status_name(status: CollaborativeRunStatus) -> &'static str {
    match status {
        CollaborativeRunStatus::Running => "running",
        CollaborativeRunStatus::Completed => "completed",
        CollaborativeRunStatus::Cancelled => "cancelled",
    }
}

fn claim_status_name(status: sprout_domain::agents::ClaimStatus) -> &'static str {
    match status {
        sprout_domain::agents::ClaimStatus::Active => "active",
        sprout_domain::agents::ClaimStatus::Expired => "expired",
        sprout_domain::agents::ClaimStatus::Released => "released",
    }
}

fn blocker_status_name(status: BlockerStatus) -> &'static str {
    match status {
        BlockerStatus::Waiting => "waiting",
        BlockerStatus::Resolved => "resolved",
        BlockerStatus::Failed => "failed",
        BlockerStatus::Cancelled => "cancelled",
    }
}

fn evidence_verification_name(mode: EvidenceVerificationMode) -> &'static str {
    match mode {
        EvidenceVerificationMode::Mechanical => "mechanical",
        EvidenceVerificationMode::SemanticJudgment => "semantic_judgment",
    }
}

fn blocker_observation_reference(
    observation: &BlockerResolutionObservation,
) -> (&'static str, Uuid) {
    match observation {
        BlockerResolutionObservation::HumanTaskTerminal { task, .. } => {
            ("human_task_terminal", Uuid::from(*task))
        }
        BlockerResolutionObservation::AdministratorDecision { decision, .. } => {
            ("administrator_decision", Uuid::from(*decision))
        }
        BlockerResolutionObservation::PrincipalResponse { comment, .. } => {
            ("principal_response", Uuid::from(*comment))
        }
        BlockerResolutionObservation::ExternalOutcome { observation, .. } => {
            ("external_outcome", *observation)
        }
    }
}

fn domain_error(error: sprout_domain::agents::AgentValidationError) -> AppError {
    tracing::warn!(error = %error, "agent completion kernel rejected transition");
    AppError::Conflict
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_payloads_are_schema_closed_and_cannot_supply_facts_or_status() {
        assert!(serde_json::from_str::<EmptyIntent>("{}").is_ok());
        assert!(serde_json::from_str::<EmptyIntent>(r#"{"eligible":true}"#).is_err());
        assert!(serde_json::from_str::<SucceedWorkRequest>("{}").is_ok());
        assert!(
            serde_json::from_str::<SucceedWorkRequest>(
                r#"{
                    "outcome":{
                        "kind":"task_completion",
                        "id":"018f0000-0000-7000-8000-000000000004",
                        "causally_linked":true
                    }
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<AcceptEvidenceRequest>(
                r#"{
                    "id":"018f0000-0000-7000-8000-000000000005",
                    "rule_id":1,
                    "work_item_id":"018f0000-0000-7000-8000-000000000006",
                    "source":{
                        "kind":"task_completion",
                        "id":"018f0000-0000-7000-8000-000000000007"
                    },
                    "obligation":"018f0000-0000-7000-8000-000000000008",
                    "discharge":true,
                    "verification":"mechanical"
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<CreateRunRequest>(
                r#"{
                    "id":"018f0000-0000-7000-8000-000000000001",
                    "source":{
                        "kind":"local_goal",
                        "id":"018f0000-0000-7000-8000-000000000002",
                        "revision":1,
                        "condition_facts":{"completed_tasks":[]}
                    }
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ResolveBlockerRequest>(
                r#"{
                    "kind":"human_task_terminal",
                    "observation_id":"018f0000-0000-7000-8000-000000000003",
                    "outcome":"succeeded",
                    "observed_at":123,
                    "facts":{"task_terminal":true}
                }"#,
            )
            .is_err()
        );
    }
}
