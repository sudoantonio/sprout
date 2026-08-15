//! Deterministic concrete refinement of the agent-governance kernel.
//!
//! Language models may propose values represented by this module, but they do
//! not validate them and never authorize their effects.  All identifiers,
//! bounds, authority, responsibility, provenance and information-flow checks
//! are performed by ordinary Rust code against the current product state.
//! There is intentionally no persistent model-memory type in this module.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AgentId, EncryptedPayload, InterrogationId, InvocationId, LanguageTaskId, LocalGoalId,
    ProjectId, ProxyRequestId, ProxyThreadId, ResourceId, ResponsibilityId, UserId,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Administrator,
    User,
    Agent,
}

impl PrincipalKind {
    #[must_use]
    pub const fn is_human(self) -> bool {
        matches!(self, Self::Administrator | Self::User)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAvailabilityMode {
    ControllerPrivate,
    ProjectDelegable,
}

/// An agent is an existing Sprout identity principal.  It therefore uses the
/// normal project membership, permission, RLS and E2EE machinery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GovernedAgent {
    pub id: AgentId,
    pub principal_id: UserId,
    pub controller_id: UserId,
    pub project_id: ProjectId,
    pub availability: AgentAvailabilityMode,
}

impl GovernedAgent {
    pub fn validate(
        &self,
        principal_kind: impl Fn(UserId) -> Option<PrincipalKind>,
    ) -> Result<(), AgentValidationError> {
        if principal_kind(self.principal_id) != Some(PrincipalKind::Agent) {
            return Err(AgentValidationError::AgentPrincipalRequired);
        }
        let controller_kind = principal_kind(self.controller_id)
            .ok_or(AgentValidationError::ControllerPrincipalRequired)?;
        if !controller_kind.is_human() {
            return Err(AgentValidationError::HumanControllerRequired);
        }
        if self.principal_id == self.controller_id {
            return Err(AgentValidationError::AgentCannotControlItself);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOperation {
    ViewHeader,
    Read,
    EditInfo,
    Write,
    Manage,
    CompleteAssignedTask,
    DelegateAssignedWork,
    ReadComment,
    PostComment,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionClass {
    CreateTask,
    ReplaceOwnTask,
    DeleteOwnTask,
    AssignOwnTask,
    UnassignOwnTask,
    MarkAssignedDone,
    AppendAssignedNote,
    AddAssignedAttachment,
    PostComment,
    InvokeTool,
    RetryTool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ResourceAuthority {
    pub resource_id: ResourceId,
    pub operation: ResourceOperation,
}

/// Immutable authority ceiling captured when a run/work item is created.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityEnvelope {
    pub resource_authority: Vec<ResourceAuthority>,
    pub tool_authority: Vec<String>,
}

impl AuthorityEnvelope {
    #[must_use]
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        self.resource_authority
            .iter()
            .all(|authority| parent.resource_authority.contains(authority))
            && self
                .tool_authority
                .iter()
                .all(|tool| parent.tool_authority.contains(tool))
    }

    pub fn validate_unique(&self) -> Result<(), AgentValidationError> {
        ensure_unique(&self.resource_authority, "resource authority")?;
        ensure_unique(&self.tool_authority, "tool authority")
    }

    /// Effective authority is the immutable ceiling intersected with current
    /// permissions. A later grant never expands an existing work envelope.
    pub fn authorizes_effect(
        &self,
        effect: ResourceAuthority,
        current_permission_allows: impl Fn(ResourceAuthority) -> bool,
    ) -> bool {
        self.resource_authority.contains(&effect) && current_permission_allows(effect)
    }

    pub fn authorizes_tool(
        &self,
        tool: &str,
        current_tool_permission_allows: impl Fn(&str) -> bool,
    ) -> bool {
        self.tool_authority
            .iter()
            .any(|candidate| candidate == tool)
            && current_tool_permission_allows(tool)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredLanguageTaskKind {
    InterpretProxyRequest,
    ExtractPromptRequirements,
    CompileGoalContract,
    CompileResponsibilityRules,
    DeriveTaskIntent,
    SynthesizeGlobalContract,
    SummarizeGovernanceDecision,
    RewritePrompt,
    AnswerFromAuthorizedContext,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StructuredLanguageTaskEnvelope {
    pub id: LanguageTaskId,
    pub kind: StructuredLanguageTaskKind,
    pub input_item_count: u32,
    pub max_input_items: u32,
    pub max_output_items: u32,
    pub max_nesting_depth: u16,
    pub max_attempts: u16,
    pub closed_output_schema: bool,
    pub grounded_identifiers_only: bool,
    pub requires_formal_proof: bool,
    pub requires_permission_decision: bool,
    pub requires_exact_semantic_equivalence: bool,
    pub requires_exhaustive_world_knowledge: bool,
    pub allowed_resource_ids: Vec<ResourceId>,
    pub allowed_principal_ids: Vec<UserId>,
    pub allowed_tools: Vec<String>,
}

impl StructuredLanguageTaskEnvelope {
    pub fn validate(&self) -> Result<(), AgentValidationError> {
        if self.input_item_count > self.max_input_items {
            return Err(AgentValidationError::InputBoundExceeded);
        }
        if self.max_output_items == 0 || self.max_nesting_depth == 0 || self.max_attempts == 0 {
            return Err(AgentValidationError::ZeroLanguageTaskBound);
        }
        if !self.closed_output_schema || !self.grounded_identifiers_only {
            return Err(AgentValidationError::OpenLanguageTask);
        }
        if self.requires_formal_proof
            || self.requires_permission_decision
            || self.requires_exact_semantic_equivalence
            || self.requires_exhaustive_world_knowledge
        {
            return Err(AgentValidationError::UnrealisticLanguageTask);
        }
        ensure_unique(&self.allowed_resource_ids, "allowed resource")?;
        ensure_unique(&self.allowed_principal_ids, "allowed principal")?;
        ensure_unique(&self.allowed_tools, "allowed tool")
    }

    pub fn validate_grounded_output(
        &self,
        output: &StructuredLanguageOutput,
    ) -> Result<(), AgentValidationError> {
        self.validate()?;
        if output.items.len() > self.max_output_items as usize {
            return Err(AgentValidationError::OutputBoundExceeded);
        }
        if output.max_observed_nesting_depth > self.max_nesting_depth {
            return Err(AgentValidationError::NestingBoundExceeded);
        }
        for item in &output.items {
            if let Some(resource_id) = item.resource_id
                && !self.allowed_resource_ids.contains(&resource_id)
            {
                return Err(AgentValidationError::UngroundedResource);
            }
            if let Some(principal_id) = item.principal_id
                && !self.allowed_principal_ids.contains(&principal_id)
            {
                return Err(AgentValidationError::UngroundedPrincipal);
            }
            if let Some(tool) = &item.tool
                && !self.allowed_tools.contains(tool)
            {
                return Err(AgentValidationError::UngroundedTool);
            }
        }
        Ok(())
    }
}

/// Provider output after JSON/schema decoding. It is still only a proposal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StructuredLanguageOutput {
    pub items: Vec<GroundedOutputItem>,
    pub max_observed_nesting_depth: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GroundedOutputItem {
    pub resource_id: Option<ResourceId>,
    pub principal_id: Option<UserId>,
    pub tool: Option<String>,
    pub action: Option<AgentActionClass>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResponsibilityRule {
    pub domain: u64,
    pub scope: ResourceId,
    pub allowed_actions: Vec<AgentActionClass>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResponsibilityContract {
    pub id: ResponsibilityId,
    pub revision: u64,
    pub administrator: UserId,
    pub user: UserId,
    pub encrypted_source_text: EncryptedPayload,
    pub rules: Vec<ResponsibilityRule>,
    pub supersedes_revision: Option<u64>,
}

impl ResponsibilityContract {
    pub fn validate(
        &self,
        principal_kind: impl Fn(UserId) -> Option<PrincipalKind>,
        administrator_controls_scope: impl Fn(UserId, ResourceId) -> bool,
    ) -> Result<(), AgentValidationError> {
        if principal_kind(self.administrator) != Some(PrincipalKind::Administrator) {
            return Err(AgentValidationError::AdministratorRequired);
        }
        if principal_kind(self.user) != Some(PrincipalKind::User) {
            return Err(AgentValidationError::HumanUserRequired);
        }
        if self.revision == 0 || self.rules.is_empty() {
            return Err(AgentValidationError::EmptyResponsibility);
        }
        for rule in &self.rules {
            if rule.allowed_actions.is_empty() {
                return Err(AgentValidationError::EmptyResponsibilityRule);
            }
            ensure_unique(&rule.allowed_actions, "responsibility action")?;
            if !administrator_controls_scope(self.administrator, rule.scope) {
                return Err(AgentValidationError::AdministratorDoesNotControlScope);
            }
        }
        Ok(())
    }

    pub fn validate_revision_of(&self, previous: &Self) -> Result<(), AgentValidationError> {
        if self.id != previous.id
            || self.revision != previous.revision + 1
            || self.administrator != previous.administrator
            || self.user != previous.user
            || self.supersedes_revision != Some(previous.revision)
        {
            return Err(AgentValidationError::InvalidResponsibilityRevision);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Internal,
    Tool,
    HumanTask,
    Comment,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailurePlan {
    RetrySame,
    Alternatives { work_spec_ids: Vec<u64> },
    DischargeBy { evidence_rule_id: u64 },
    FailGoal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContractObligation {
    pub id: Uuid,
    pub owner: UserId,
    pub required_for_completion: bool,
    pub dependency_rank: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContractDependency {
    pub obligation: Uuid,
    pub prerequisite: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContractWorkSpec {
    pub id: u64,
    pub obligation: Uuid,
    pub owner: UserId,
    pub kind: WorkKind,
    pub allowed_actions: Vec<AgentActionClass>,
    pub max_instances: u32,
    pub max_attempts: u16,
    pub max_resolution_ticks: u32,
    pub generation_rank: u32,
    pub is_entry: bool,
    pub continuations: Vec<u64>,
    pub failure_plan: FailurePlan,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GoalContract {
    pub scope: ResourceId,
    pub obligations: Vec<ContractObligation>,
    pub dependencies: Vec<ContractDependency>,
    pub work_specs: Vec<ContractWorkSpec>,
}

impl GoalContract {
    pub fn validate(&self) -> Result<(), AgentValidationError> {
        if self.obligations.is_empty() || self.work_specs.is_empty() {
            return Err(AgentValidationError::EmptyGoalContract);
        }
        ensure_unique_by(&self.obligations, |item| item.id, "obligation")?;
        ensure_unique_by(&self.work_specs, |item| item.id, "work spec")?;

        let obligations: HashMap<_, _> = self
            .obligations
            .iter()
            .map(|obligation| (obligation.id, obligation))
            .collect();
        let work_specs: HashMap<_, _> =
            self.work_specs.iter().map(|work| (work.id, work)).collect();

        for dependency in &self.dependencies {
            let target = obligations
                .get(&dependency.obligation)
                .ok_or(AgentValidationError::UnknownObligation)?;
            let prerequisite = obligations
                .get(&dependency.prerequisite)
                .ok_or(AgentValidationError::UnknownObligation)?;
            if prerequisite.dependency_rank >= target.dependency_rank {
                return Err(AgentValidationError::DependencyRankDoesNotDecrease);
            }
        }

        for obligation in &self.obligations {
            let entries = self
                .work_specs
                .iter()
                .filter(|work| work.obligation == obligation.id && work.is_entry)
                .count();
            if entries != 1 {
                return Err(AgentValidationError::ObligationEntryCardinality);
            }
        }

        for work in &self.work_specs {
            if !obligations.contains_key(&work.obligation) {
                return Err(AgentValidationError::UnknownObligation);
            }
            if work.allowed_actions.is_empty()
                || work.max_instances == 0
                || work.max_attempts == 0
                || work.max_resolution_ticks == 0
            {
                return Err(AgentValidationError::InvalidWorkSpecBounds);
            }
            ensure_unique(&work.allowed_actions, "work action")?;
            ensure_unique(&work.continuations, "continuation")?;
            for continuation_id in &work.continuations {
                let continuation = work_specs
                    .get(continuation_id)
                    .ok_or(AgentValidationError::UnknownContinuation)?;
                if continuation.generation_rank >= work.generation_rank {
                    return Err(AgentValidationError::GenerationRankDoesNotDecrease);
                }
            }
            if let FailurePlan::Alternatives { work_spec_ids } = &work.failure_plan {
                if work_spec_ids.is_empty() {
                    return Err(AgentValidationError::EmptyAlternativePlan);
                }
                for alternative_id in work_spec_ids {
                    let alternative = work_specs
                        .get(alternative_id)
                        .ok_or(AgentValidationError::UnknownContinuation)?;
                    if alternative.obligation != work.obligation
                        || alternative.generation_rank >= work.generation_rank
                    {
                        return Err(AgentValidationError::InvalidAlternativePlan);
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalGoalClause {
    pub id: u64,
    pub domain: u64,
    pub scope: ResourceId,
    pub work_spec_ids: Vec<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalGoalOrigin {
    ControllerPrompt,
    AdministratorException { review_id: Uuid },
    GlobalMandate { global_revision: u64 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalGoalContract {
    pub id: LocalGoalId,
    pub revision: u64,
    pub agent: UserId,
    pub controller: UserId,
    pub encrypted_prompt: EncryptedPayload,
    pub contract: GoalContract,
    pub clauses: Vec<LocalGoalClause>,
    pub origin: LocalGoalOrigin,
    pub supersedes_revision: Option<u64>,
}

impl LocalGoalContract {
    pub fn validate(&self) -> Result<(), AgentValidationError> {
        self.contract.validate()?;
        if self.revision == 0 || self.clauses.is_empty() {
            return Err(AgentValidationError::EmptyLocalGoal);
        }
        if self
            .contract
            .obligations
            .iter()
            .any(|obligation| obligation.owner != self.agent)
            || self
                .contract
                .work_specs
                .iter()
                .any(|work| work.owner != self.agent)
        {
            return Err(AgentValidationError::LocalGoalOwnedByDifferentAgent);
        }
        ensure_unique_by(&self.clauses, |clause| clause.id, "local clause")?;
        let work_ids: HashSet<_> = self
            .contract
            .work_specs
            .iter()
            .map(|work| work.id)
            .collect();
        let mut classified = HashSet::new();
        for clause in &self.clauses {
            if clause.work_spec_ids.is_empty() {
                return Err(AgentValidationError::EmptyLocalClause);
            }
            ensure_unique(&clause.work_spec_ids, "clause work spec")?;
            for work_id in &clause.work_spec_ids {
                if !work_ids.contains(work_id) {
                    return Err(AgentValidationError::UnknownWorkSpec);
                }
                classified.insert(*work_id);
            }
        }
        if classified != work_ids {
            return Err(AgentValidationError::UnclassifiedWorkSpec);
        }
        Ok(())
    }

    #[must_use]
    pub fn can_contribute_bottom_up(&self) -> bool {
        !matches!(self.origin, LocalGoalOrigin::GlobalMandate { .. })
    }

    pub fn validate_revision_of(&self, previous: &Self) -> Result<(), AgentValidationError> {
        if self.id != previous.id
            || self.revision != previous.revision + 1
            || self.agent != previous.agent
            || self.controller != previous.controller
            || self.supersedes_revision != Some(previous.revision)
        {
            return Err(AgentValidationError::InvalidLocalGoalRevision);
        }
        Ok(())
    }
}

pub fn responsibility_covers_local_goal(
    responsibility: &ResponsibilityContract,
    local_goal: &LocalGoalContract,
    resource_within_scope: impl Fn(ResourceId, ResourceId) -> bool,
) -> bool {
    if responsibility.user != local_goal.controller || local_goal.validate().is_err() {
        return false;
    }
    let work_by_id: HashMap<_, _> = local_goal
        .contract
        .work_specs
        .iter()
        .map(|work| (work.id, work))
        .collect();
    local_goal.clauses.iter().all(|clause| {
        responsibility.rules.iter().any(|rule| {
            rule.domain == clause.domain
                && resource_within_scope(rule.scope, clause.scope)
                && clause.work_spec_ids.iter().all(|work_id| {
                    work_by_id.get(work_id).is_some_and(|work| {
                        work.allowed_actions
                            .iter()
                            .all(|action| rule.allowed_actions.contains(action))
                    })
                })
        })
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GlobalLocalContribution {
    pub agent: UserId,
    pub local_revision: u64,
    pub local_clause_id: u64,
    pub global_work_spec_ids: Vec<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StructuredGlobalWorkGrounding {
    pub global_work_spec_id: u64,
    pub source_agent: UserId,
    pub source_local_revision: u64,
    pub source_work_spec_id: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GlobalContractCandidate {
    pub revision: u64,
    pub contract: GoalContract,
    pub contributions: Vec<GlobalLocalContribution>,
    pub governance_conflicts: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StructuredGlobalSynthesisEnvelope {
    pub language_task: StructuredLanguageTaskEnvelope,
    pub source_agents: Vec<UserId>,
    pub max_global_obligations: u32,
    pub max_global_work_specs: u32,
    pub max_dependencies: u32,
    pub max_conflicts: u32,
}

/// Validates the automatic bottom-up path. Semantic quality can be evaluated
/// separately; activation relies only on structural grounding and governance.
pub fn validate_global_synthesis(
    envelope: &StructuredGlobalSynthesisEnvelope,
    candidate: &GlobalContractCandidate,
    groundings: &[StructuredGlobalWorkGrounding],
    active_local_goals: &HashMap<UserId, LocalGoalContract>,
    source_authorized: impl Fn(&LocalGoalContract) -> bool,
) -> Result<(), AgentValidationError> {
    envelope.language_task.validate()?;
    if envelope.language_task.kind != StructuredLanguageTaskKind::SynthesizeGlobalContract {
        return Err(AgentValidationError::WrongLanguageTaskKind);
    }
    candidate.contract.validate()?;
    if candidate.revision == 0
        || candidate.contract.obligations.len() > envelope.max_global_obligations as usize
        || candidate.contract.work_specs.len() > envelope.max_global_work_specs as usize
        || candidate.contract.dependencies.len() > envelope.max_dependencies as usize
        || candidate.governance_conflicts.len() > envelope.max_conflicts as usize
    {
        return Err(AgentValidationError::GlobalSynthesisBoundExceeded);
    }
    if !candidate.governance_conflicts.is_empty() {
        return Err(AgentValidationError::DeclaredGovernanceConflict);
    }
    ensure_unique_by(
        groundings,
        |grounding| grounding.global_work_spec_id,
        "global grounding",
    )?;

    for contribution in &candidate.contributions {
        if !envelope.source_agents.contains(&contribution.agent) {
            return Err(AgentValidationError::UngroundedPrincipal);
        }
        let local = active_local_goals
            .get(&contribution.agent)
            .ok_or(AgentValidationError::InactiveLocalGoalSource)?;
        if local.revision != contribution.local_revision
            || !local.can_contribute_bottom_up()
            || !source_authorized(local)
            || !local
                .clauses
                .iter()
                .any(|clause| clause.id == contribution.local_clause_id)
        {
            return Err(AgentValidationError::UnauthorizedGlobalContribution);
        }
    }

    for global_work in &candidate.contract.work_specs {
        let grounding = groundings
            .iter()
            .find(|grounding| grounding.global_work_spec_id == global_work.id)
            .ok_or(AgentValidationError::UngroundedGlobalWork)?;
        let local = active_local_goals
            .get(&grounding.source_agent)
            .ok_or(AgentValidationError::InactiveLocalGoalSource)?;
        if local.revision != grounding.source_local_revision {
            return Err(AgentValidationError::InactiveLocalGoalSource);
        }
        let source_work = local
            .contract
            .work_specs
            .iter()
            .find(|work| work.id == grounding.source_work_spec_id)
            .ok_or(AgentValidationError::UnknownWorkSpec)?;
        if global_work.owner != source_work.owner
            || global_work.kind != source_work.kind
            || global_work.allowed_actions != source_work.allowed_actions
            || global_work.max_instances > source_work.max_instances
            || global_work.max_attempts > source_work.max_attempts
            || global_work.failure_plan != source_work.failure_plan
        {
            return Err(AgentValidationError::GlobalWorkAmplifiesLocalSource);
        }
        if !candidate.contributions.iter().any(|contribution| {
            contribution.agent == grounding.source_agent
                && contribution.local_revision == grounding.source_local_revision
                && contribution.global_work_spec_ids.contains(&global_work.id)
        }) {
            return Err(AgentValidationError::UngroundedGlobalWork);
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceEffect {
    pub resource_id: ResourceId,
    pub operation: ResourceOperation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlannedToolInvocation {
    pub tool: String,
    /// Hash of the canonical structured input; plaintext input remains in the
    /// E2EE request payload and is decoded only at the execution boundary.
    pub input_digest: String,
    pub required_effects: Vec<ResourceEffect>,
}

/// A proxy is mediation metadata, never a principal and never an ACL subject.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserProxy {
    pub id: Uuid,
    pub user: UserId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserProxyThread {
    pub id: ProxyThreadId,
    pub proxy_id: Uuid,
    pub creator: UserId,
    pub created_at: DateTime<Utc>,
}

impl UserProxyThread {
    #[must_use]
    pub fn valid_for(&self, proxy: &UserProxy) -> bool {
        self.proxy_id == proxy.id && self.creator == proxy.user
    }

    #[must_use]
    pub fn readable_by(&self, principal: UserId) -> bool {
        principal == self.creator
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserProxyRequest {
    pub id: ProxyRequestId,
    pub thread_id: ProxyThreadId,
    pub user: UserId,
    pub encrypted_payload: EncryptedPayload,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserProxyPlanningEnvelope {
    pub language_task: StructuredLanguageTaskEnvelope,
    pub request_id: ProxyRequestId,
    pub user: UserId,
    pub candidate_resources: Vec<ResourceId>,
    pub candidate_operations: Vec<ResourceOperation>,
    pub available_tools: Vec<String>,
    pub max_plan_steps: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserProxyActionPlan {
    pub request_id: ProxyRequestId,
    pub thread_id: ProxyThreadId,
    pub user: UserId,
    pub intent_id: Uuid,
    pub resource_effects: Vec<ResourceEffect>,
    pub tool_invocations: Vec<PlannedToolInvocation>,
    pub encrypted_explanation: EncryptedPayload,
}

impl UserProxyActionPlan {
    pub fn validate_within_envelope(
        &self,
        envelope: &UserProxyPlanningEnvelope,
    ) -> Result<(), AgentValidationError> {
        envelope.language_task.validate()?;
        if envelope.language_task.kind != StructuredLanguageTaskKind::InterpretProxyRequest
            || self.request_id != envelope.request_id
            || self.user != envelope.user
        {
            return Err(AgentValidationError::ProxyPlanNotBoundToEnvelope);
        }
        if self.resource_effects.len() + self.tool_invocations.len()
            > envelope.max_plan_steps as usize
        {
            return Err(AgentValidationError::ProxyPlanBoundExceeded);
        }
        for effect in &self.resource_effects {
            if !envelope.candidate_resources.contains(&effect.resource_id)
                || !envelope.candidate_operations.contains(&effect.operation)
            {
                return Err(AgentValidationError::ProxyPlanOutsideEnvelope);
            }
        }
        for invocation in &self.tool_invocations {
            if !envelope.available_tools.contains(&invocation.tool) {
                return Err(AgentValidationError::ProxyPlanOutsideEnvelope);
            }
        }
        Ok(())
    }

    pub fn validate_runtime_authority(
        &self,
        permission_allows: impl Fn(UserId, &ResourceEffect) -> bool,
        tool_allows: impl Fn(UserId, &str) -> bool,
    ) -> Result<(), AgentValidationError> {
        for effect in &self.resource_effects {
            if !permission_allows(self.user, effect) {
                return Err(AgentValidationError::RuntimePermissionDenied);
            }
        }
        for invocation in &self.tool_invocations {
            if !tool_allows(self.user, &invocation.tool) {
                return Err(AgentValidationError::RuntimeToolPermissionDenied);
            }
            for required in &invocation.required_effects {
                if !self.resource_effects.contains(required)
                    || !permission_allows(self.user, required)
                {
                    return Err(AgentValidationError::IncompleteToolFootprint);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserProxyOutOfResponsibilityConfirmation {
    pub user: UserId,
    pub thread_id: ProxyThreadId,
    pub request_id: ProxyRequestId,
    pub accepted_plan: UserProxyActionPlan,
    pub summary_id: Uuid,
    pub confirmed_at: DateTime<Utc>,
}

pub struct ProxyExecution<'a> {
    pub proxy: &'a UserProxy,
    pub thread: &'a UserProxyThread,
    pub request: &'a UserProxyRequest,
    pub envelope: &'a UserProxyPlanningEnvelope,
    pub plan: &'a UserProxyActionPlan,
    pub within_responsibility: bool,
    pub confirmation: Option<&'a UserProxyOutOfResponsibilityConfirmation>,
}

impl ProxyExecution<'_> {
    pub fn validate(
        &self,
        permission_allows: impl Fn(UserId, &ResourceEffect) -> bool,
        tool_allows: impl Fn(UserId, &str) -> bool,
    ) -> Result<(), AgentValidationError> {
        if !self.thread.valid_for(self.proxy)
            || self.request.thread_id != self.thread.id
            || self.request.user != self.proxy.user
            || self.plan.request_id != self.request.id
            || self.plan.thread_id != self.thread.id
            || self.plan.user != self.proxy.user
        {
            return Err(AgentValidationError::ProxyPlanNotBoundToRequest);
        }
        self.plan.validate_within_envelope(self.envelope)?;
        self.plan
            .validate_runtime_authority(permission_allows, tool_allows)?;
        if !self.within_responsibility {
            let confirmation = self
                .confirmation
                .ok_or(AgentValidationError::ConfirmationRequired)?;
            if confirmation.user != self.proxy.user
                || confirmation.thread_id != self.thread.id
                || confirmation.request_id != self.plan.request_id
                || confirmation.accepted_plan != *self.plan
            {
                return Err(AgentValidationError::ConfirmationDoesNotMatchPlan);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InformationSource {
    ResourceBody {
        resource_id: ResourceId,
    },
    Comment {
        resource_id: ResourceId,
        comment_id: Uuid,
    },
    InfoDocument {
        resource_id: ResourceId,
        document_id: Uuid,
    },
    InfoFile {
        resource_id: ResourceId,
        file_id: Uuid,
    },
    ToolOutput {
        call_id: Uuid,
    },
    ProxyTranscript {
        thread_id: ProxyThreadId,
    },
    EventHistory {
        event_id: Uuid,
    },
    Provenance {
        provenance_id: Uuid,
    },
}

/// Context reconstructed for one invocation from authoritative product data.
/// There is deliberately no recalled/persistent model-memory field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelInvocationContext {
    pub invocation_id: InvocationId,
    pub principal: UserId,
    pub sources: Vec<InformationSource>,
    pub reconstructed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelExposureProjection {
    pub exposed_sources: Vec<InformationSource>,
    pub hidden_persistent_model_memory_available: bool,
}

pub fn validate_state_grounded_invocation(
    context: &ModelInvocationContext,
    exposure: &ModelExposureProjection,
    currently_readable: impl Fn(UserId, &InformationSource) -> bool,
) -> Result<(), AgentValidationError> {
    ensure_unique(&context.sources, "invocation source")?;
    ensure_unique(&exposure.exposed_sources, "exposed source")?;
    if context
        .sources
        .iter()
        .any(|source| !currently_readable(context.principal, source))
    {
        return Err(AgentValidationError::UnreadableInvocationSource);
    }
    if exposure.hidden_persistent_model_memory_available {
        return Err(AgentValidationError::HiddenModelMemoryForbidden);
    }
    if as_set(&context.sources) != as_set(&exposure.exposed_sources) {
        return Err(AgentValidationError::InvocationExposureNotExact);
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentInterrogationCausalDelta {
    pub resource_effects: Vec<ResourceEffect>,
    pub tool_invocations: Vec<PlannedToolInvocation>,
    pub prompt_revisions: Vec<UserId>,
    pub local_goal_revisions: Vec<UserId>,
    pub created_work: Vec<Uuid>,
    pub activated_obligations: Vec<Uuid>,
    pub assigned_tasks: Vec<ResourceId>,
}

impl AgentInterrogationCausalDelta {
    pub fn validate_read_only(&self) -> Result<(), AgentValidationError> {
        if self.resource_effects.is_empty()
            && self.tool_invocations.is_empty()
            && self.prompt_revisions.is_empty()
            && self.local_goal_revisions.is_empty()
            && self.created_work.is_empty()
            && self.activated_obligations.is_empty()
            && self.assigned_tasks.is_empty()
        {
            Ok(())
        } else {
            Err(AgentValidationError::InterrogationHasSideEffects)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentInterrogationSession {
    pub id: InterrogationId,
    pub creator: UserId,
    pub target_agent: UserId,
    pub created_at: DateTime<Utc>,
    pub via_tool_call: Option<Uuid>,
}

impl AgentInterrogationSession {
    #[must_use]
    pub fn transcript_readable_by(&self, principal: UserId) -> bool {
        principal == self.creator
    }
}

/// Every eventual reader of a sink must currently be able to read every
/// source used to derive the persisted output.
pub fn validate_information_flow(
    source_audiences: &[HashSet<UserId>],
    sink_audience: &HashSet<UserId>,
) -> Result<(), AgentValidationError> {
    if source_audiences
        .iter()
        .all(|source_audience| sink_audience.is_subset(source_audience))
    {
        Ok(())
    } else {
        Err(AgentValidationError::InformationFlowWouldExpandAudience)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskObligationProvenance {
    pub task: ResourceId,
    pub agent: UserId,
    pub local_revision: u64,
    pub obligation: Uuid,
    pub work_spec_id: u64,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedTaskIntent {
    pub task: ResourceId,
    pub scope: ResourceId,
    pub required_actions: Vec<AgentActionClass>,
    pub created_by: UserId,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrossOwnerAssignmentRoute {
    AutomaticFromActiveObligation,
    ControllerReview,
    Rejected,
}

/// Cross-owner routing never treats a linguistic label as authority. Exact
/// active obligation provenance enables the automatic route; otherwise only a
/// persisted intent covered by the target controller's responsibility can
/// open a review. Everything else is rejected.
pub fn route_cross_owner_assignment(
    task: ResourceId,
    target_agent: &GovernedAgent,
    active_local_goal: Option<&LocalGoalContract>,
    provenance: Option<&TaskObligationProvenance>,
    intent: Option<&PersistedTaskIntent>,
    controller_responsibility: Option<&ResponsibilityContract>,
    resource_within_scope: impl Fn(ResourceId, ResourceId) -> bool,
) -> CrossOwnerAssignmentRoute {
    if let (Some(local), Some(provenance)) = (active_local_goal, provenance)
        && provenance.task == task
        && provenance.agent == target_agent.principal_id
        && provenance.local_revision == local.revision
        && local.contract.obligations.iter().any(|obligation| {
            obligation.id == provenance.obligation
                && obligation.owner == target_agent.principal_id
                && obligation.required_for_completion
        })
        && local.contract.work_specs.iter().any(|work| {
            work.id == provenance.work_spec_id
                && work.obligation == provenance.obligation
                && work.owner == target_agent.principal_id
        })
    {
        return CrossOwnerAssignmentRoute::AutomaticFromActiveObligation;
    }

    if let (Some(intent), Some(responsibility)) = (intent, controller_responsibility)
        && intent.task == task
        && responsibility.user == target_agent.controller_id
        && responsibility.rules.iter().any(|rule| {
            resource_within_scope(rule.scope, intent.scope)
                && intent
                    .required_actions
                    .iter()
                    .all(|action| rule.allowed_actions.contains(action))
        })
    {
        CrossOwnerAssignmentRoute::ControllerReview
    } else {
        CrossOwnerAssignmentRoute::Rejected
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanGovernanceDecisionReason {
    FinalAgentPrompt,
    ProxyActionOutsideResponsibility,
    SendResponsibilityExceptionToAdministrator,
    AdministratorResponsibilityExceptionDecision,
    CrossOwnerTaskControllerDecision,
    ExplicitPermissionGrant,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GovernanceFactRef {
    LocalRevision { revision: u64 },
    ResponsibilityRevision { revision: u64 },
    UncoveredWorkSpec { work_spec_id: u64 },
    Task { task: ResourceId },
    Agent { agent: UserId },
    ActionIntent { intent_id: Uuid },
    PermissionScope { resource: ResourceId },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BriefGovernanceSummary {
    pub id: Uuid,
    pub reason: HumanGovernanceDecisionReason,
    pub facts: Vec<GovernanceFactRef>,
    pub encrypted_payload: EncryptedPayload,
    pub generated_at: DateTime<Utc>,
}

impl BriefGovernanceSummary {
    pub fn validate(&self) -> Result<(), AgentValidationError> {
        if self.facts.is_empty() || self.facts.len() > 5 {
            return Err(AgentValidationError::InvalidGovernanceSummary);
        }
        ensure_unique(&self.facts, "governance fact")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserProxyMediatedAuditEntry {
    pub user: UserId,
    pub proxy_id: Uuid,
    pub thread_id: ProxyThreadId,
    pub request_id: ProxyRequestId,
    pub plan: UserProxyActionPlan,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SemanticOperationalState {
    pub proxy_audit: Vec<UserProxyMediatedAuditEntry>,
    pub task_obligation_provenance: Vec<TaskObligationProvenance>,
    pub task_intents: Vec<PersistedTaskIntent>,
}

impl SemanticOperationalState {
    #[must_use]
    pub fn extends(&self, previous: &Self) -> bool {
        is_prefix(&previous.proxy_audit, &self.proxy_audit)
            && is_prefix(
                &previous.task_obligation_provenance,
                &self.task_obligation_provenance,
            )
            && is_prefix(&previous.task_intents, &self.task_intents)
    }
}

fn ensure_unique<T: Eq + std::hash::Hash>(
    values: &[T],
    field: &'static str,
) -> Result<(), AgentValidationError> {
    let mut seen = HashSet::with_capacity(values.len());
    if values.iter().all(|value| seen.insert(value)) {
        Ok(())
    } else {
        Err(AgentValidationError::DuplicateValue(field))
    }
}

fn ensure_unique_by<T, K: Eq + std::hash::Hash>(
    values: &[T],
    key: impl Fn(&T) -> K,
    field: &'static str,
) -> Result<(), AgentValidationError> {
    let mut seen = HashSet::with_capacity(values.len());
    if values.iter().all(|value| seen.insert(key(value))) {
        Ok(())
    } else {
        Err(AgentValidationError::DuplicateValue(field))
    }
}

fn as_set<T: Clone + Eq + std::hash::Hash>(values: &[T]) -> HashSet<T> {
    values.iter().cloned().collect()
}

fn is_prefix<T: PartialEq>(prefix: &[T], values: &[T]) -> bool {
    values.starts_with(prefix)
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AgentValidationError {
    #[error("agent principal must have agent kind")]
    AgentPrincipalRequired,
    #[error("agent controller principal is missing")]
    ControllerPrincipalRequired,
    #[error("agent controller must be a human user or administrator")]
    HumanControllerRequired,
    #[error("an agent cannot control itself")]
    AgentCannotControlItself,
    #[error("administrator principal required")]
    AdministratorRequired,
    #[error("human user principal required")]
    HumanUserRequired,
    #[error("responsibility must have a positive revision and non-empty rules")]
    EmptyResponsibility,
    #[error("responsibility rule must allow at least one action")]
    EmptyResponsibilityRule,
    #[error("administrator does not control responsibility scope")]
    AdministratorDoesNotControlScope,
    #[error("responsibility revision does not exactly supersede its predecessor")]
    InvalidResponsibilityRevision,
    #[error("language-task input bound exceeded")]
    InputBoundExceeded,
    #[error("language-task bounds must be positive")]
    ZeroLanguageTaskBound,
    #[error("language task must use a closed schema and grounded identifiers")]
    OpenLanguageTask,
    #[error("language task asks the model to decide a deterministic or unbounded property")]
    UnrealisticLanguageTask,
    #[error("language-task output bound exceeded")]
    OutputBoundExceeded,
    #[error("language-task nesting bound exceeded")]
    NestingBoundExceeded,
    #[error("model output contains a resource outside its envelope")]
    UngroundedResource,
    #[error("model output contains a principal outside its envelope")]
    UngroundedPrincipal,
    #[error("model output contains a tool outside its envelope")]
    UngroundedTool,
    #[error("goal contract must contain obligations and work")]
    EmptyGoalContract,
    #[error("goal contract references an unknown obligation")]
    UnknownObligation,
    #[error("dependency rank does not strictly decrease")]
    DependencyRankDoesNotDecrease,
    #[error("each obligation must have exactly one entry work specification")]
    ObligationEntryCardinality,
    #[error("work specification has an invalid zero/empty bound")]
    InvalidWorkSpecBounds,
    #[error("work specification references an unknown continuation")]
    UnknownContinuation,
    #[error("continuation generation rank does not strictly decrease")]
    GenerationRankDoesNotDecrease,
    #[error("alternative failure plan cannot be empty")]
    EmptyAlternativePlan,
    #[error("alternative failure plan changes obligation or does not decrease rank")]
    InvalidAlternativePlan,
    #[error("local goal must have a positive revision and non-empty clauses")]
    EmptyLocalGoal,
    #[error("local goal contains work owned by another principal")]
    LocalGoalOwnedByDifferentAgent,
    #[error("local goal clause cannot be empty")]
    EmptyLocalClause,
    #[error("local goal clause references unknown work")]
    UnknownWorkSpec,
    #[error("every work specification must be classified")]
    UnclassifiedWorkSpec,
    #[error("local goal revision does not exactly supersede its predecessor")]
    InvalidLocalGoalRevision,
    #[error("proxy plan is not bound to its planning envelope")]
    ProxyPlanNotBoundToEnvelope,
    #[error("proxy plan exceeds its finite step bound")]
    ProxyPlanBoundExceeded,
    #[error("proxy plan uses an identifier or operation outside its envelope")]
    ProxyPlanOutsideEnvelope,
    #[error("proxy plan is not causally bound to its request/thread/user")]
    ProxyPlanNotBoundToRequest,
    #[error("current resource permission denied")]
    RuntimePermissionDenied,
    #[error("current tool permission denied")]
    RuntimeToolPermissionDenied,
    #[error("tool required effects are absent or unauthorized")]
    IncompleteToolFootprint,
    #[error("one-shot out-of-responsibility confirmation required")]
    ConfirmationRequired,
    #[error("confirmation does not bind the exact accepted plan")]
    ConfirmationDoesNotMatchPlan,
    #[error("invocation contains a source not currently readable by its principal")]
    UnreadableInvocationSource,
    #[error("persistent hidden model memory is forbidden")]
    HiddenModelMemoryForbidden,
    #[error("the exposed model context is not exactly the declared source set")]
    InvocationExposureNotExact,
    #[error("interrogation produced a causal side effect")]
    InterrogationHasSideEffects,
    #[error("persisted disclosure would expand the source audience")]
    InformationFlowWouldExpandAudience,
    #[error("duplicate value in {0}")]
    DuplicateValue(&'static str),
    #[error("language task kind does not match the workflow")]
    WrongLanguageTaskKind,
    #[error("global synthesis exceeds its finite envelope")]
    GlobalSynthesisBoundExceeded,
    #[error("global synthesis declared a governance conflict and cannot auto-activate")]
    DeclaredGovernanceConflict,
    #[error("global contribution does not reference an active local goal")]
    InactiveLocalGoalSource,
    #[error("global contribution is not operationally authorized")]
    UnauthorizedGlobalContribution,
    #[error("global work is not exactly grounded in authorized local work")]
    UngroundedGlobalWork,
    #[error("global work amplifies its local source")]
    GlobalWorkAmplifiesLocalSource,
    #[error("governance summary must contain between one and five unique facts")]
    InvalidGovernanceSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> EncryptedPayload {
        EncryptedPayload::new(1, "aes-256-gcm", "key", vec![1; 12], vec![2; 16]).unwrap()
    }

    fn feasible_task(kind: StructuredLanguageTaskKind) -> StructuredLanguageTaskEnvelope {
        StructuredLanguageTaskEnvelope {
            id: LanguageTaskId::new(),
            kind,
            input_item_count: 1,
            max_input_items: 4,
            max_output_items: 4,
            max_nesting_depth: 3,
            max_attempts: 3,
            closed_output_schema: true,
            grounded_identifiers_only: true,
            requires_formal_proof: false,
            requires_permission_decision: false,
            requires_exact_semantic_equivalence: false,
            requires_exhaustive_world_knowledge: false,
            allowed_resource_ids: Vec::new(),
            allowed_principal_ids: Vec::new(),
            allowed_tools: Vec::new(),
        }
    }

    fn local_goal(agent: UserId, controller: UserId) -> LocalGoalContract {
        let obligation = Uuid::now_v7();
        LocalGoalContract {
            id: LocalGoalId::new(),
            revision: 1,
            agent,
            controller,
            encrypted_prompt: payload(),
            contract: GoalContract {
                scope: ResourceId::new(),
                obligations: vec![ContractObligation {
                    id: obligation,
                    owner: agent,
                    required_for_completion: true,
                    dependency_rank: 0,
                }],
                dependencies: Vec::new(),
                work_specs: vec![ContractWorkSpec {
                    id: 1,
                    obligation,
                    owner: agent,
                    kind: WorkKind::Internal,
                    allowed_actions: vec![AgentActionClass::CreateTask],
                    max_instances: 2,
                    max_attempts: 2,
                    max_resolution_ticks: 5,
                    generation_rank: 0,
                    is_entry: true,
                    continuations: Vec::new(),
                    failure_plan: FailurePlan::FailGoal,
                }],
            },
            clauses: vec![LocalGoalClause {
                id: 1,
                domain: 7,
                scope: ResourceId::new(),
                work_spec_ids: vec![1],
            }],
            origin: LocalGoalOrigin::ControllerPrompt,
            supersedes_revision: None,
        }
    }

    #[test]
    fn authority_is_attenuated_and_rechecks_current_permissions() {
        let resource = ResourceId::new();
        let effect = ResourceAuthority {
            resource_id: resource,
            operation: ResourceOperation::Write,
        };
        let parent = AuthorityEnvelope {
            resource_authority: vec![
                effect,
                ResourceAuthority {
                    resource_id: resource,
                    operation: ResourceOperation::Read,
                },
            ],
            tool_authority: vec!["search".into()],
        };
        let child = AuthorityEnvelope {
            resource_authority: vec![effect],
            tool_authority: Vec::new(),
        };
        assert!(child.is_subset_of(&parent));
        assert!(!parent.is_subset_of(&child));
        assert!(child.authorizes_effect(effect, |_| true));
        assert!(!child.authorizes_effect(effect, |_| false));
    }

    #[test]
    fn language_model_cannot_invent_identifiers_or_decide_permissions() {
        let allowed = ResourceId::new();
        let mut task = feasible_task(StructuredLanguageTaskKind::CompileGoalContract);
        task.allowed_resource_ids.push(allowed);
        assert_eq!(task.validate(), Ok(()));

        let invented = StructuredLanguageOutput {
            items: vec![GroundedOutputItem {
                resource_id: Some(ResourceId::new()),
                principal_id: None,
                tool: None,
                action: Some(AgentActionClass::CreateTask),
            }],
            max_observed_nesting_depth: 1,
        };
        assert_eq!(
            task.validate_grounded_output(&invented),
            Err(AgentValidationError::UngroundedResource)
        );

        task.requires_permission_decision = true;
        assert_eq!(
            task.validate(),
            Err(AgentValidationError::UnrealisticLanguageTask)
        );
    }

    #[test]
    fn state_grounded_context_rejects_revoked_sources_and_hidden_memory() {
        let principal = UserId::new();
        let source = InformationSource::ResourceBody {
            resource_id: ResourceId::new(),
        };
        let context = ModelInvocationContext {
            invocation_id: InvocationId::new(),
            principal,
            sources: vec![source.clone()],
            reconstructed_at: Utc::now(),
        };
        let exact = ModelExposureProjection {
            exposed_sources: vec![source.clone()],
            hidden_persistent_model_memory_available: false,
        };
        assert_eq!(
            validate_state_grounded_invocation(&context, &exact, |_, candidate| candidate
                == &source),
            Ok(())
        );
        assert_eq!(
            validate_state_grounded_invocation(&context, &exact, |_, _| false),
            Err(AgentValidationError::UnreadableInvocationSource)
        );
        let hidden = ModelExposureProjection {
            exposed_sources: vec![source],
            hidden_persistent_model_memory_available: true,
        };
        assert_eq!(
            validate_state_grounded_invocation(&context, &hidden, |_, _| true),
            Err(AgentValidationError::HiddenModelMemoryForbidden)
        );
    }

    #[test]
    fn proxy_confirmation_never_bypasses_runtime_permissions() {
        let user = UserId::new();
        let proxy = UserProxy {
            id: Uuid::now_v7(),
            user,
        };
        let thread = UserProxyThread {
            id: ProxyThreadId::new(),
            proxy_id: proxy.id,
            creator: user,
            created_at: Utc::now(),
        };
        let request = UserProxyRequest {
            id: ProxyRequestId::new(),
            thread_id: thread.id,
            user,
            encrypted_payload: payload(),
            submitted_at: Utc::now(),
        };
        let resource = ResourceId::new();
        let mut language_task = feasible_task(StructuredLanguageTaskKind::InterpretProxyRequest);
        language_task.allowed_resource_ids.push(resource);
        let envelope = UserProxyPlanningEnvelope {
            language_task,
            request_id: request.id,
            user,
            candidate_resources: vec![resource],
            candidate_operations: vec![ResourceOperation::Write],
            available_tools: Vec::new(),
            max_plan_steps: 1,
        };
        let plan = UserProxyActionPlan {
            request_id: request.id,
            thread_id: thread.id,
            user,
            intent_id: Uuid::now_v7(),
            resource_effects: vec![ResourceEffect {
                resource_id: resource,
                operation: ResourceOperation::Write,
            }],
            tool_invocations: Vec::new(),
            encrypted_explanation: payload(),
        };
        let confirmation = UserProxyOutOfResponsibilityConfirmation {
            user,
            thread_id: thread.id,
            request_id: request.id,
            accepted_plan: plan.clone(),
            summary_id: Uuid::now_v7(),
            confirmed_at: Utc::now(),
        };
        assert_eq!(
            ProxyExecution {
                proxy: &proxy,
                thread: &thread,
                request: &request,
                envelope: &envelope,
                plan: &plan,
                within_responsibility: false,
                confirmation: Some(&confirmation),
            }
            .validate(|_, _| false, |_, _| true),
            Err(AgentValidationError::RuntimePermissionDenied)
        );
    }

    #[test]
    fn interrogation_is_strictly_read_only() {
        assert_eq!(
            AgentInterrogationCausalDelta::default().validate_read_only(),
            Ok(())
        );
        let mut delta = AgentInterrogationCausalDelta::default();
        delta.assigned_tasks.push(ResourceId::new());
        assert_eq!(
            delta.validate_read_only(),
            Err(AgentValidationError::InterrogationHasSideEffects)
        );
    }

    #[test]
    fn shared_sink_audience_must_be_an_intersection_of_source_audiences() {
        let first = UserId::new();
        let second = UserId::new();
        let source_a = HashSet::from([first, second]);
        let source_b = HashSet::from([first]);
        assert_eq!(
            validate_information_flow(&[source_a.clone(), source_b], &HashSet::from([first])),
            Ok(())
        );
        assert_eq!(
            validate_information_flow(&[source_a], &HashSet::from([first, second])),
            Ok(())
        );
        let source_only_first = HashSet::from([first]);
        assert_eq!(
            validate_information_flow(&[source_only_first], &HashSet::from([first, second]),),
            Err(AgentValidationError::InformationFlowWouldExpandAudience)
        );
    }

    #[test]
    fn semantic_operational_histories_are_append_only() {
        let previous = SemanticOperationalState::default();
        let mut next = previous.clone();
        next.task_intents.push(PersistedTaskIntent {
            task: ResourceId::new(),
            scope: ResourceId::new(),
            required_actions: vec![AgentActionClass::CreateTask],
            created_by: UserId::new(),
            recorded_at: Utc::now(),
        });
        assert!(next.extends(&previous));
        assert!(!previous.extends(&next));
    }

    #[test]
    fn responsibility_and_local_goal_remain_distinct_gates() {
        let agent = UserId::new();
        let controller = UserId::new();
        let administrator = UserId::new();
        let local = local_goal(agent, controller);
        assert_eq!(local.validate(), Ok(()));
        let responsibility = ResponsibilityContract {
            id: ResponsibilityId::new(),
            revision: 1,
            administrator,
            user: controller,
            encrypted_source_text: payload(),
            rules: vec![ResponsibilityRule {
                domain: 7,
                scope: local.clauses[0].scope,
                allowed_actions: vec![AgentActionClass::CreateTask],
            }],
            supersedes_revision: None,
        };
        assert!(responsibility_covers_local_goal(
            &responsibility,
            &local,
            |parent, child| parent == child,
        ));

        let mut insufficient = responsibility;
        insufficient.rules[0].allowed_actions = vec![AgentActionClass::PostComment];
        assert!(!responsibility_covers_local_goal(
            &insufficient,
            &local,
            |parent, child| parent == child,
        ));
    }

    #[test]
    fn global_synthesis_cannot_amplify_grounded_local_work() {
        let agent = UserId::new();
        let local = local_goal(agent, UserId::new());
        let mut language_task = feasible_task(StructuredLanguageTaskKind::SynthesizeGlobalContract);
        language_task.allowed_principal_ids.push(agent);
        let envelope = StructuredGlobalSynthesisEnvelope {
            language_task,
            source_agents: vec![agent],
            max_global_obligations: 2,
            max_global_work_specs: 2,
            max_dependencies: 2,
            max_conflicts: 2,
        };
        let candidate = GlobalContractCandidate {
            revision: 1,
            contract: local.contract.clone(),
            contributions: vec![GlobalLocalContribution {
                agent,
                local_revision: local.revision,
                local_clause_id: local.clauses[0].id,
                global_work_spec_ids: vec![1],
            }],
            governance_conflicts: Vec::new(),
        };
        let grounding = StructuredGlobalWorkGrounding {
            global_work_spec_id: 1,
            source_agent: agent,
            source_local_revision: local.revision,
            source_work_spec_id: 1,
        };
        let active = HashMap::from([(agent, local.clone())]);
        assert_eq!(
            validate_global_synthesis(
                &envelope,
                &candidate,
                std::slice::from_ref(&grounding),
                &active,
                |_| true,
            ),
            Ok(())
        );

        let mut amplified = candidate;
        amplified.contract.work_specs[0].max_attempts += 1;
        assert_eq!(
            validate_global_synthesis(&envelope, &amplified, &[grounding], &active, |_| true,),
            Err(AgentValidationError::GlobalWorkAmplifiesLocalSource)
        );
    }

    #[test]
    fn cross_owner_automatic_route_requires_exact_active_provenance() {
        let agent_principal = UserId::new();
        let controller = UserId::new();
        let local = local_goal(agent_principal, controller);
        let task = ResourceId::new();
        let governed = GovernedAgent {
            id: AgentId::new(),
            principal_id: agent_principal,
            controller_id: controller,
            project_id: ProjectId::new(),
            availability: AgentAvailabilityMode::ProjectDelegable,
        };
        let provenance = TaskObligationProvenance {
            task,
            agent: agent_principal,
            local_revision: local.revision,
            obligation: local.contract.obligations[0].id,
            work_spec_id: local.contract.work_specs[0].id,
            recorded_at: Utc::now(),
        };
        assert_eq!(
            route_cross_owner_assignment(
                task,
                &governed,
                Some(&local),
                Some(&provenance),
                None,
                None,
                |_, _| false,
            ),
            CrossOwnerAssignmentRoute::AutomaticFromActiveObligation
        );

        let mut stale = provenance;
        stale.local_revision += 1;
        assert_eq!(
            route_cross_owner_assignment(
                task,
                &governed,
                Some(&local),
                Some(&stale),
                None,
                None,
                |_, _| false,
            ),
            CrossOwnerAssignmentRoute::Rejected
        );
    }
}
