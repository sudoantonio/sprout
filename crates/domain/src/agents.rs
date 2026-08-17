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
    AgentId, BlockerId, ClaimId, CommentId, EncryptedPayload, EvidenceId, GoalId,
    GovernanceReviewId, InterrogationId, InvocationId, LanguageTaskId, LocalGoalId, ProjectId,
    ProxyRequestId, ProxyThreadId, ResourceId, ResponsibilityId, RunId, ToolCallId, UserId,
    WorkItemId,
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct StructuredLanguageOutput {
    pub items: Vec<GroundedOutputItem>,
    pub max_observed_nesting_depth: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GroundedOutputItem {
    pub resource_id: Option<ResourceId>,
    pub principal_id: Option<UserId>,
    pub tool: Option<String>,
    pub action: Option<AgentActionClass>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResponsibilityRule {
    pub domain: u64,
    pub scope: ResourceId,
    pub allowed_actions: Vec<AgentActionClass>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
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
        if !matches!(
            principal_kind(self.user),
            Some(PrincipalKind::User | PrincipalKind::Administrator)
        ) {
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContractCondition {
    Always {},
    Never {},
    TaskDone {
        task: ResourceId,
    },
    ObligationDone {
        obligation: Uuid,
    },
    CommentBy {
        principal: UserId,
    },
    AdministratorApproved {
        administrator: UserId,
        review_work_spec_id: u64,
    },
    All {
        left: Box<Self>,
        right: Box<Self>,
    },
    Any {
        left: Box<Self>,
        right: Box<Self>,
    },
    Neg {
        condition: Box<Self>,
    },
}

impl ContractCondition {
    #[must_use]
    pub const fn always() -> Self {
        Self::Always {}
    }

    fn validate_references(
        &self,
        obligations: &HashSet<Uuid>,
        work_specs: &HashSet<u64>,
    ) -> Result<(), AgentValidationError> {
        match self {
            Self::ObligationDone { obligation } if !obligations.contains(obligation) => {
                Err(AgentValidationError::UnknownObligation)
            }
            Self::AdministratorApproved {
                review_work_spec_id,
                ..
            } if !work_specs.contains(review_work_spec_id) => {
                Err(AgentValidationError::UnknownWorkSpec)
            }
            Self::All { left, right } | Self::Any { left, right } => {
                left.validate_references(obligations, work_specs)?;
                right.validate_references(obligations, work_specs)
            }
            Self::Neg { condition } => condition.validate_references(obligations, work_specs),
            _ => Ok(()),
        }
    }

    #[must_use]
    pub fn holds(&self, facts: &ContractConditionFacts) -> bool {
        match self {
            Self::Always {} => true,
            Self::Never {} => false,
            Self::TaskDone { task } => facts.completed_tasks.contains(task),
            Self::ObligationDone { obligation } => {
                facts.discharged_obligations.contains(obligation)
            }
            Self::CommentBy { principal } => facts.comment_authors.contains(principal),
            Self::AdministratorApproved {
                administrator,
                review_work_spec_id,
            } => facts
                .administrator_approvals
                .contains(&(*administrator, *review_work_spec_id)),
            Self::All { left, right } => left.holds(facts) && right.holds(facts),
            Self::Any { left, right } => left.holds(facts) || right.holds(facts),
            Self::Neg { condition } => !condition.holds(facts),
        }
    }

    fn is_implied_by(&self, antecedent: &Self) -> bool {
        if matches!(self, Self::Always {})
            || matches!(antecedent, Self::Never {})
            || self == antecedent
        {
            return true;
        }
        match (antecedent, self) {
            (Self::All { left, right }, consequent) => {
                consequent.is_implied_by(left) || consequent.is_implied_by(right)
            }
            (Self::Any { left, right }, consequent) => {
                consequent.is_implied_by(left) && consequent.is_implied_by(right)
            }
            (antecedent, Self::All { left, right }) => {
                left.is_implied_by(antecedent) && right.is_implied_by(antecedent)
            }
            (antecedent, Self::Any { left, right }) => {
                left.is_implied_by(antecedent) || right.is_implied_by(antecedent)
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContractConditionFacts {
    pub completed_tasks: HashSet<ResourceId>,
    pub discharged_obligations: HashSet<Uuid>,
    pub comment_authors: HashSet<UserId>,
    pub administrator_approvals: HashSet<(UserId, u64)>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    AgentAction,
    ToolInvocation,
    ToolRetry,
    TaskAction,
    Coordination,
    ExternalWait,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    ToolCompleted,
    TaskCompleted,
    CommentObserved,
    PrincipalResponse,
    HumanApproval,
    AdministratorApproval,
    ExternalOutcome,
    DerivedFact,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceVerificationMode {
    Mechanical,
    SemanticJudgment,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContractEvidenceSubject {
    WorkResult {
        work_spec_id: u64,
    },
    Principal {
        principal: UserId,
    },
    Obligation {
        obligation: Uuid,
    },
    AdministratorDecision {
        administrator: UserId,
        review_work_spec_id: u64,
    },
    ExternalCondition {
        condition: Uuid,
    },
    Derived {},
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractEvidenceRule {
    pub id: u64,
    pub obligation: Uuid,
    pub kind: EvidenceKind,
    pub subject: ContractEvidenceSubject,
    pub verification: EvidenceVerificationMode,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContractWaitingTarget {
    WorkTerminal {
        work_spec_id: u64,
    },
    TaskFromWork {
        work_spec_id: u64,
    },
    PrincipalResponse {
        principal: UserId,
    },
    ObligationDischarged {
        obligation: Uuid,
    },
    AdministratorApproval {
        administrator: UserId,
        review_work_spec_id: u64,
    },
    ExternalOutcome {
        condition: Uuid,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractWaitingRule {
    pub id: u64,
    pub obligation: Uuid,
    pub target: ContractWaitingTarget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FailurePlan {
    RetrySame {},
    Alternatives { work_spec_ids: Vec<u64> },
    DischargeBy { evidence_rule_id: u64 },
    FailGoal {},
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractObligation {
    pub id: Uuid,
    pub goal: GoalId,
    pub owner: UserId,
    pub activation: ContractCondition,
    pub required_for_completion: ContractCondition,
    pub dependency_rank: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractDependency {
    pub obligation: Uuid,
    pub prerequisite: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractWorkSpec {
    pub id: u64,
    pub obligation: Uuid,
    pub owner: UserId,
    pub kind: WorkKind,
    pub activation: ContractCondition,
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
#[serde(deny_unknown_fields)]
pub struct GoalContract {
    pub goal: GoalId,
    pub scope: ResourceId,
    pub obligations: Vec<ContractObligation>,
    pub dependencies: Vec<ContractDependency>,
    pub work_specs: Vec<ContractWorkSpec>,
    pub evidence_rules: Vec<ContractEvidenceRule>,
    pub waiting_rules: Vec<ContractWaitingRule>,
    pub completion_condition: ContractCondition,
}

impl GoalContract {
    pub fn validate(&self) -> Result<(), AgentValidationError> {
        if self.obligations.is_empty() || self.work_specs.is_empty() {
            return Err(AgentValidationError::EmptyGoalContract);
        }
        if self.completion_condition != ContractCondition::always() {
            return Err(AgentValidationError::UnnormalizedCompletionCondition);
        }
        ensure_unique_by(&self.obligations, |item| item.id, "obligation")?;
        ensure_unique_by(&self.work_specs, |item| item.id, "work spec")?;
        ensure_unique_by(&self.evidence_rules, |item| item.id, "evidence rule")?;
        ensure_unique_by(&self.waiting_rules, |item| item.id, "waiting rule")?;

        let obligations: HashMap<_, _> = self
            .obligations
            .iter()
            .map(|obligation| (obligation.id, obligation))
            .collect();
        let work_specs: HashMap<_, _> =
            self.work_specs.iter().map(|work| (work.id, work)).collect();
        let obligation_ids: HashSet<_> = obligations.keys().copied().collect();
        let work_spec_ids: HashSet<_> = work_specs.keys().copied().collect();

        self.completion_condition
            .validate_references(&obligation_ids, &work_spec_ids)?;
        for obligation in &self.obligations {
            if obligation.goal != self.goal {
                return Err(AgentValidationError::ObligationGoalMismatch);
            }
            obligation
                .activation
                .validate_references(&obligation_ids, &work_spec_ids)?;
            obligation
                .required_for_completion
                .validate_references(&obligation_ids, &work_spec_ids)?;
        }

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
            let entries: Vec<_> = self
                .work_specs
                .iter()
                .filter(|work| work.obligation == obligation.id && work.is_entry)
                .collect();
            if entries.len() != 1 {
                return Err(AgentValidationError::ObligationEntryCardinality);
            }
            let required = ContractCondition::All {
                left: Box::new(obligation.activation.clone()),
                right: Box::new(obligation.required_for_completion.clone()),
            };
            if !entries[0].activation.is_implied_by(&required) {
                return Err(AgentValidationError::EntryActivationNotRequired);
            }
            if !self
                .evidence_rules
                .iter()
                .any(|rule| rule.obligation == obligation.id)
            {
                return Err(AgentValidationError::MissingObligationEvidenceRule);
            }
        }

        for work in &self.work_specs {
            let obligation = obligations
                .get(&work.obligation)
                .ok_or(AgentValidationError::UnknownObligation)?;
            if work.owner != obligation.owner {
                return Err(AgentValidationError::WorkOwnerMismatch);
            }
            work.activation
                .validate_references(&obligation_ids, &work_spec_ids)?;
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
            if let FailurePlan::DischargeBy { evidence_rule_id } = work.failure_plan {
                let rule = self
                    .evidence_rules
                    .iter()
                    .find(|rule| rule.id == evidence_rule_id)
                    .ok_or(AgentValidationError::UnknownEvidenceRule)?;
                if rule.obligation != work.obligation {
                    return Err(AgentValidationError::InvalidDischargeRule);
                }
            }
        }

        for rule in &self.evidence_rules {
            if !obligation_ids.contains(&rule.obligation) {
                return Err(AgentValidationError::UnknownObligation);
            }
            match rule.subject {
                ContractEvidenceSubject::WorkResult { work_spec_id }
                | ContractEvidenceSubject::AdministratorDecision {
                    review_work_spec_id: work_spec_id,
                    ..
                } if !work_spec_ids.contains(&work_spec_id) => {
                    return Err(AgentValidationError::UnknownWorkSpec);
                }
                ContractEvidenceSubject::Obligation { obligation }
                    if !obligation_ids.contains(&obligation) =>
                {
                    return Err(AgentValidationError::UnknownObligation);
                }
                _ => {}
            }
        }

        for rule in &self.waiting_rules {
            if !obligation_ids.contains(&rule.obligation) {
                return Err(AgentValidationError::UnknownObligation);
            }
            match rule.target {
                ContractWaitingTarget::WorkTerminal { work_spec_id }
                | ContractWaitingTarget::TaskFromWork { work_spec_id }
                | ContractWaitingTarget::AdministratorApproval {
                    review_work_spec_id: work_spec_id,
                    ..
                } if !work_spec_ids.contains(&work_spec_id) => {
                    return Err(AgentValidationError::UnknownWorkSpec);
                }
                ContractWaitingTarget::ObligationDischarged { obligation }
                    if !obligation_ids.contains(&obligation) =>
                {
                    return Err(AgentValidationError::UnknownObligation);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Completed,
    Failed,
    Cancelled,
    Superseded,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborativeRunStatus {
    Running,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationStatus {
    Active,
    Discharged,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Blocked,
    Eligible,
    Claimed,
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    Ready,
    Claimed,
    Closed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Active,
    Expired,
    Released,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockerStatus {
    Waiting,
    Resolved,
    Failed,
    Cancelled,
}

impl BlockerStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Waiting)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WaitingCondition {
    PrincipalResponse { principal: UserId },
    AdministratorApproval { administrator: UserId },
    HumanTaskCompleted { task: ResourceId },
    ExternalOutcome { condition: Uuid },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlockScope {
    Work { work: WorkItemId },
    Obligation { obligation: Uuid },
    Goal { goal: GoalId },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExternalBlockerFacts {
    pub human_principals: HashSet<UserId>,
    pub administrators: HashSet<UserId>,
    pub human_assigned_tasks: HashSet<ResourceId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedTerminalOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl ObservedTerminalOutcome {
    const fn blocker_status(self) -> BlockerStatus {
        match self {
            Self::Succeeded => BlockerStatus::Resolved,
            Self::Failed => BlockerStatus::Failed,
            Self::Cancelled => BlockerStatus::Cancelled,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidAdministratorDecisionFact {
    pub administrator: UserId,
    pub review_task: ResourceId,
    pub review_work_spec_id: u64,
    pub outcome: ObservedTerminalOutcome,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidExternalOutcomeFact {
    pub condition: Uuid,
    pub outcome: ObservedTerminalOutcome,
    pub provenance_hash: [u8; 32],
    pub observed_at: u64,
}

/// Authoritative product-event projection supplied by the deterministic
/// runtime. This is not a client payload.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlockerResolutionFacts {
    pub terminal_human_tasks: HashMap<ResourceId, (ObservedTerminalOutcome, u64)>,
    pub principal_response_comments: HashMap<CommentId, (UserId, u64)>,
    pub valid_administrator_decisions: HashMap<GovernanceReviewId, ValidAdministratorDecisionFact>,
    pub external_outcomes: HashMap<Uuid, ValidExternalOutcomeFact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlockerResolutionObservation {
    HumanTaskTerminal {
        blocker: BlockerId,
        task: ResourceId,
        outcome: ObservedTerminalOutcome,
        observed_at: u64,
    },
    AdministratorDecision {
        blocker: BlockerId,
        decision: GovernanceReviewId,
        administrator: UserId,
        review_task: ResourceId,
        review_work_spec_id: u64,
        outcome: ObservedTerminalOutcome,
        observed_at: u64,
    },
    PrincipalResponse {
        blocker: BlockerId,
        principal: UserId,
        comment: CommentId,
        observed_at: u64,
    },
    ExternalOutcome {
        blocker: BlockerId,
        observation: Uuid,
        condition: Uuid,
        outcome: ObservedTerminalOutcome,
        provenance_hash: [u8; 32],
        observed_at: u64,
    },
}

impl BlockerResolutionObservation {
    const fn blocker(&self) -> BlockerId {
        match self {
            Self::HumanTaskTerminal { blocker, .. }
            | Self::AdministratorDecision { blocker, .. }
            | Self::PrincipalResponse { blocker, .. }
            | Self::ExternalOutcome { blocker, .. } => *blocker,
        }
    }

    const fn observed_at(&self) -> u64 {
        match self {
            Self::HumanTaskTerminal { observed_at, .. }
            | Self::AdministratorDecision { observed_at, .. }
            | Self::PrincipalResponse { observed_at, .. }
            | Self::ExternalOutcome { observed_at, .. } => *observed_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockerResolutionRecord {
    pub blocker: BlockerId,
    pub observation: BlockerResolutionObservation,
    pub terminal_status: BlockerStatus,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObligationInstance {
    pub run: RunId,
    pub spec: Uuid,
    pub owner: UserId,
    pub status: ObligationStatus,
    pub activated_at: u64,
    pub discharged_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    pub id: WorkItemId,
    pub run: RunId,
    pub goal: GoalId,
    pub owner: UserId,
    pub serves: Uuid,
    pub work_spec_id: u64,
    pub slot: u32,
    pub kind: WorkKind,
    pub parent: Option<WorkItemId>,
    pub source_comment: Option<CommentId>,
    pub status: WorkStatus,
    pub attempt: u16,
    pub created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDispatch {
    pub work: WorkItemId,
    pub attempt: u16,
    pub status: DispatchStatus,
    pub enqueued_at: u64,
    pub scheduler_position: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkClaim {
    pub id: ClaimId,
    pub work: WorkItemId,
    pub attempt: u16,
    pub claimant: UserId,
    pub acquired_at: u64,
    pub expires_at: u64,
    pub status: ClaimStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkBlocker {
    pub id: BlockerId,
    pub run: RunId,
    pub goal: GoalId,
    pub scope: BlockScope,
    pub obligation: Uuid,
    pub waiting_rule_id: u64,
    pub condition: WaitingCondition,
    pub status: BlockerStatus,
    pub created_at: u64,
    pub terminal_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceSubject {
    ToolCall {
        call_id: ToolCallId,
    },
    Task {
        task: ResourceId,
    },
    Comment {
        comment: CommentId,
    },
    Principal {
        principal: UserId,
    },
    Obligation {
        obligation: Uuid,
    },
    AdministratorDecision {
        administrator: UserId,
        review_task: ResourceId,
    },
    ExternalCondition {
        condition: Uuid,
    },
    Derived {},
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub id: EvidenceId,
    pub run: RunId,
    pub obligation: Uuid,
    pub rule_id: u64,
    pub kind: EvidenceKind,
    pub subject: EvidenceSubject,
    pub work: Option<WorkItemId>,
    pub observed_at: u64,
    pub provenance_hash: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CollaborativeCausalNode {
    Obligation { obligation: Uuid },
    Work { work: WorkItemId },
    Comment { comment: CommentId },
    Task { task: ResourceId },
    ToolCall { call: ToolCallId },
    Blocker { blocker: BlockerId },
    Evidence { evidence: EvidenceId },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborativeCausalLink {
    pub run: RunId,
    pub goal: GoalId,
    pub predecessor: CollaborativeCausalNode,
    pub successor: CollaborativeCausalNode,
    pub observed_at: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkProjectionEventKind {
    Materialized,
    ActivationCeased,
    Reactivated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkProjectionEvent {
    pub run: RunId,
    pub work: WorkItemId,
    pub work_spec_id: u64,
    pub slot: u32,
    pub prior_status: Option<WorkStatus>,
    pub attempt: u16,
    pub kind: WorkProjectionEventKind,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalWorkSlot {
    pub work_spec_id: u64,
    pub slot: u32,
    pub work: WorkItemId,
}

mod work_slot_map {
    use std::collections::HashMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    use super::{CanonicalWorkSlot, WorkItemId};

    pub fn serialize<S>(
        slots: &HashMap<(u64, u32), WorkItemId>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut values: Vec<_> = slots
            .iter()
            .map(|((work_spec_id, slot), work)| CanonicalWorkSlot {
                work_spec_id: *work_spec_id,
                slot: *slot,
                work: *work,
            })
            .collect();
        values.sort_by_key(|value| (value.work_spec_id, value.slot));
        values.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<(u64, u32), WorkItemId>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<CanonicalWorkSlot>::deserialize(deserializer)?;
        let mut slots = HashMap::with_capacity(values.len());
        for value in values {
            if slots
                .insert((value.work_spec_id, value.slot), value.work)
                .is_some()
            {
                return Err(D::Error::custom("duplicate canonical work slot"));
            }
        }
        Ok(slots)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuspendedClaimResolution {
    pub work: WorkItemId,
    pub attempt: u16,
    pub deadline: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborativeRunState {
    pub id: RunId,
    pub goal: GoalId,
    pub scope: ResourceId,
    pub goal_status: GoalStatus,
    pub run_status: CollaborativeRunStatus,
    pub participants: HashSet<UserId>,
    pub obligations: HashMap<Uuid, ObligationInstance>,
    #[serde(with = "work_slot_map")]
    pub work_slots: HashMap<(u64, u32), WorkItemId>,
    /// Current Lean `SemanticState.workItems` projection only.
    pub work_items: HashMap<WorkItemId, WorkItem>,
    /// Operational records retained outside the current semantic projection.
    pub inactive_work_items: HashMap<WorkItemId, WorkItem>,
    /// Append-only projection history; it is not projected as current work.
    pub work_projection_history: Vec<WorkProjectionEvent>,
    pub suspended_claim_resolutions: HashMap<WorkItemId, SuspendedClaimResolution>,
    pub dispatches: HashMap<WorkItemId, WorkDispatch>,
    pub claims: HashMap<ClaimId, WorkClaim>,
    pub blockers: HashMap<BlockerId, WorkBlocker>,
    pub blocker_resolutions: Vec<BlockerResolutionRecord>,
    pub evidence: Vec<EvidenceRecord>,
    pub causal_links: Vec<CollaborativeCausalLink>,
}

struct WorkMaterialization {
    work_spec_id: u64,
    slot: u32,
    parent: Option<WorkItemId>,
    source_comment: Option<CommentId>,
    tick: u64,
}

impl CollaborativeRunState {
    pub fn initialize(
        id: RunId,
        contract: &GoalContract,
        facts: &ContractConditionFacts,
        tick: u64,
    ) -> Result<Self, AgentValidationError> {
        contract.validate()?;
        let participants = contract
            .obligations
            .iter()
            .map(|obligation| obligation.owner)
            .collect();
        let mut work_slots = HashMap::new();
        for spec in &contract.work_specs {
            for slot in 0..spec.max_instances {
                work_slots.insert((spec.id, slot), WorkItemId::new());
            }
        }
        let mut state = Self {
            id,
            goal: contract.goal,
            scope: contract.scope,
            goal_status: GoalStatus::Active,
            run_status: CollaborativeRunStatus::Running,
            participants,
            obligations: HashMap::new(),
            work_slots,
            work_items: HashMap::new(),
            inactive_work_items: HashMap::new(),
            work_projection_history: Vec::new(),
            suspended_claim_resolutions: HashMap::new(),
            dispatches: HashMap::new(),
            claims: HashMap::new(),
            blockers: HashMap::new(),
            blocker_resolutions: Vec::new(),
            evidence: Vec::new(),
            causal_links: Vec::new(),
        };
        state.refresh_frontier(contract, facts, tick)?;
        Ok(state)
    }

    pub fn refresh_frontier(
        &mut self,
        contract: &GoalContract,
        facts: &ContractConditionFacts,
        tick: u64,
    ) -> Result<(), AgentValidationError> {
        self.ensure_contract(contract)?;
        let effective_facts = self.effective_facts(facts);
        for spec in &contract.obligations {
            if spec.activation.holds(&effective_facts)
                && spec.required_for_completion.holds(&effective_facts)
                && !self.obligations.contains_key(&spec.id)
            {
                self.obligations.insert(
                    spec.id,
                    ObligationInstance {
                        run: self.id,
                        spec: spec.id,
                        owner: spec.owner,
                        status: ObligationStatus::Active,
                        activated_at: tick,
                        discharged_at: None,
                    },
                );
            }
        }

        let deactivate: Vec<_> = self
            .work_items
            .values()
            .filter(|work| {
                contract
                    .work_specs
                    .iter()
                    .find(|spec| spec.id == work.work_spec_id)
                    .is_none_or(|spec| !spec.activation.holds(&effective_facts))
            })
            .map(|work| work.id)
            .collect();
        for work_id in deactivate {
            self.deactivate_work(contract, work_id, tick)?;
        }

        let reactivate: Vec<_> = self
            .inactive_work_items
            .values()
            .filter(|work| {
                self.dependencies_closed(contract, work.serves)
                    && contract
                        .work_specs
                        .iter()
                        .find(|spec| spec.id == work.work_spec_id)
                        .is_some_and(|spec| spec.activation.holds(&effective_facts))
            })
            .map(|work| work.id)
            .collect();
        for work_id in reactivate {
            self.reactivate_work(work_id, tick)?;
        }

        let ready: Vec<_> = self
            .obligations
            .values()
            .filter(|instance| {
                instance.status == ObligationStatus::Active
                    && self.dependencies_closed(contract, instance.spec)
                    && !self
                        .work_items
                        .values()
                        .any(|work| work.serves == instance.spec)
                    && !self
                        .inactive_work_items
                        .values()
                        .any(|work| work.serves == instance.spec)
            })
            .map(|instance| instance.spec)
            .collect();
        for obligation in ready {
            let entry = contract
                .work_specs
                .iter()
                .find(|spec| spec.obligation == obligation && spec.is_entry)
                .ok_or(AgentValidationError::ObligationEntryCardinality)?;
            if !entry.activation.holds(&effective_facts) {
                return Err(AgentValidationError::RequiredEntryWorkInactive);
            }
            self.materialize_work(
                contract,
                WorkMaterialization {
                    work_spec_id: entry.id,
                    slot: 0,
                    parent: None,
                    source_comment: None,
                    tick,
                },
                &effective_facts,
            )?;
        }
        let delayed_continuations: Vec<_> = self
            .work_items
            .values()
            .chain(self.inactive_work_items.values())
            .filter(|parent| parent.status == WorkStatus::Succeeded)
            .flat_map(|parent| {
                contract
                    .work_specs
                    .iter()
                    .find(|spec| spec.id == parent.work_spec_id)
                    .into_iter()
                    .flat_map(move |spec| {
                        spec.continuations
                            .iter()
                            .copied()
                            .map(move |continuation| (parent.id, continuation))
                    })
            })
            .filter(|(parent, continuation)| {
                contract
                    .work_specs
                    .iter()
                    .find(|spec| spec.id == *continuation)
                    .is_some_and(|spec| spec.activation.holds(&effective_facts))
                    && !self
                        .work_items
                        .values()
                        .chain(self.inactive_work_items.values())
                        .any(|work| {
                            work.parent == Some(*parent) && work.work_spec_id == *continuation
                        })
            })
            .collect();
        for (parent, continuation) in delayed_continuations {
            if let Some(slot) = self.first_free_slot(contract, continuation) {
                self.materialize_work(
                    contract,
                    WorkMaterialization {
                        work_spec_id: continuation,
                        slot,
                        parent: Some(parent),
                        source_comment: None,
                        tick,
                    },
                    &effective_facts,
                )?;
            }
        }
        self.enforce_suspended_claim_deadlines(tick);
        self.validate_current_projection(contract, &effective_facts)?;
        self.try_complete(contract, facts);
        Ok(())
    }

    fn ensure_contract(&self, contract: &GoalContract) -> Result<(), AgentValidationError> {
        if self.goal != contract.goal || self.scope != contract.scope {
            return Err(AgentValidationError::RunContractMismatch);
        }
        contract.validate()
    }

    fn dependencies_closed(&self, contract: &GoalContract, obligation: Uuid) -> bool {
        contract
            .dependencies
            .iter()
            .filter(|dependency| dependency.obligation == obligation)
            .all(|dependency| {
                self.obligations
                    .get(&dependency.prerequisite)
                    .is_some_and(|instance| instance.status == ObligationStatus::Discharged)
            })
    }

    fn effective_facts(&self, facts: &ContractConditionFacts) -> ContractConditionFacts {
        let mut effective = facts.clone();
        effective.discharged_obligations.extend(
            self.obligations
                .values()
                .filter(|instance| instance.status == ObligationStatus::Discharged)
                .map(|instance| instance.spec),
        );
        effective
    }

    fn work_has_waiting_blocker(&self, work: &WorkItem) -> bool {
        self.blockers.values().any(|blocker| {
            blocker.run == self.id
                && blocker.goal == self.goal
                && blocker.status == BlockerStatus::Waiting
                && match blocker.scope {
                    BlockScope::Work { work: blocked } => blocked == work.id,
                    BlockScope::Obligation { obligation } => obligation == work.serves,
                    BlockScope::Goal { goal } => goal == work.goal,
                }
        })
    }

    fn deactivate_work(
        &mut self,
        contract: &GoalContract,
        work_id: WorkItemId,
        tick: u64,
    ) -> Result<(), AgentValidationError> {
        let mut work = self
            .work_items
            .remove(&work_id)
            .ok_or(AgentValidationError::UnknownWorkItem)?;
        let prior_status = work.status;
        if prior_status == WorkStatus::Claimed {
            let claim = self
                .claims
                .values_mut()
                .find(|claim| claim.work == work_id && claim.status == ClaimStatus::Active)
                .ok_or(AgentValidationError::ClaimedWorkMissingLease)?;
            claim.status = ClaimStatus::Released;
            let spec = contract
                .work_specs
                .iter()
                .find(|spec| spec.id == work.work_spec_id)
                .ok_or(AgentValidationError::UnknownWorkSpec)?;
            self.suspended_claim_resolutions.insert(
                work_id,
                SuspendedClaimResolution {
                    work: work_id,
                    attempt: work.attempt,
                    deadline: claim
                        .acquired_at
                        .saturating_add(u64::from(spec.max_resolution_ticks)),
                },
            );
            work.status = WorkStatus::Eligible;
        }
        self.dispatches.remove(&work_id);
        self.work_projection_history.push(WorkProjectionEvent {
            run: self.id,
            work: work_id,
            work_spec_id: work.work_spec_id,
            slot: work.slot,
            prior_status: Some(prior_status),
            attempt: work.attempt,
            kind: WorkProjectionEventKind::ActivationCeased,
            observed_at: tick,
        });
        self.inactive_work_items.insert(work_id, work);
        Ok(())
    }

    fn reactivate_work(
        &mut self,
        work_id: WorkItemId,
        tick: u64,
    ) -> Result<(), AgentValidationError> {
        let mut work = self
            .inactive_work_items
            .remove(&work_id)
            .ok_or(AgentValidationError::UnknownWorkItem)?;
        if !work.status.is_terminal() {
            work.status = if self.work_has_waiting_blocker(&work) {
                WorkStatus::Blocked
            } else {
                WorkStatus::Eligible
            };
        }
        if work.status == WorkStatus::Eligible {
            self.dispatches.insert(
                work_id,
                WorkDispatch {
                    work: work_id,
                    attempt: work.attempt,
                    status: DispatchStatus::Ready,
                    enqueued_at: tick,
                    scheduler_position: 0,
                },
            );
        }
        self.work_projection_history.push(WorkProjectionEvent {
            run: self.id,
            work: work_id,
            work_spec_id: work.work_spec_id,
            slot: work.slot,
            prior_status: None,
            attempt: work.attempt,
            kind: WorkProjectionEventKind::Reactivated,
            observed_at: tick,
        });
        self.work_items.insert(work_id, work);
        Ok(())
    }

    fn enforce_suspended_claim_deadlines(&mut self, tick: u64) {
        let resolved: Vec<_> = self
            .suspended_claim_resolutions
            .values()
            .filter(|resolution| {
                self.work_items
                    .get(&resolution.work)
                    .or_else(|| self.inactive_work_items.get(&resolution.work))
                    .and_then(|work| self.obligations.get(&work.serves))
                    .is_some_and(|obligation| obligation.status == ObligationStatus::Discharged)
                    || self.work_items.get(&resolution.work).is_some_and(|work| {
                        work.attempt > resolution.attempt || work.status.is_terminal()
                    })
            })
            .map(|resolution| resolution.work)
            .collect();
        for work in resolved {
            self.suspended_claim_resolutions.remove(&work);
        }
        if self.goal_status == GoalStatus::Active
            && self
                .suspended_claim_resolutions
                .values()
                .any(|resolution| resolution.deadline <= tick)
        {
            self.goal_status = GoalStatus::Failed;
        }
    }

    pub fn validate_current_projection(
        &self,
        contract: &GoalContract,
        facts: &ContractConditionFacts,
    ) -> Result<(), AgentValidationError> {
        self.ensure_contract(contract)?;
        for work in self.work_items.values() {
            let spec = contract
                .work_specs
                .iter()
                .find(|spec| spec.id == work.work_spec_id)
                .ok_or(AgentValidationError::UnknownWorkSpec)?;
            if !spec.activation.holds(facts) {
                return Err(AgentValidationError::CurrentWorkActivationFalse);
            }
            if self.work_slots.get(&(work.work_spec_id, work.slot)) != Some(&work.id)
                || self.inactive_work_items.contains_key(&work.id)
            {
                return Err(AgentValidationError::CanonicalWorkIdentityViolation);
            }
            if work.status == WorkStatus::Eligible
                && (!self.dependencies_closed(contract, work.serves)
                    || work.attempt >= spec.max_attempts
                    || self.work_has_waiting_blocker(work))
            {
                return Err(AgentValidationError::EligibleWorkInvariantViolation);
            }
            if work.status == WorkStatus::Blocked && !self.work_has_waiting_blocker(work) {
                return Err(AgentValidationError::BlockedWorkMissingWaitingBlocker);
            }
        }
        for work in self.inactive_work_items.values() {
            if self.work_slots.get(&(work.work_spec_id, work.slot)) != Some(&work.id)
                || self.work_items.contains_key(&work.id)
            {
                return Err(AgentValidationError::CanonicalWorkIdentityViolation);
            }
        }
        Ok(())
    }

    fn materialize_work(
        &mut self,
        contract: &GoalContract,
        materialization: WorkMaterialization,
        facts: &ContractConditionFacts,
    ) -> Result<WorkItemId, AgentValidationError> {
        let WorkMaterialization {
            work_spec_id,
            slot,
            parent,
            source_comment,
            tick,
        } = materialization;
        let spec = contract
            .work_specs
            .iter()
            .find(|spec| spec.id == work_spec_id)
            .ok_or(AgentValidationError::UnknownWorkSpec)?;
        if slot >= spec.max_instances {
            return Err(AgentValidationError::WorkSlotOutOfRange);
        }
        let id = *self
            .work_slots
            .get(&(work_spec_id, slot))
            .ok_or(AgentValidationError::UnknownWorkSlot)?;
        if self.work_items.contains_key(&id) || self.inactive_work_items.contains_key(&id) {
            return Err(AgentValidationError::WorkSlotAlreadyMaterialized);
        }
        if let Some(parent_id) = parent {
            let parent_work = self
                .work_items
                .get(&parent_id)
                .or_else(|| self.inactive_work_items.get(&parent_id))
                .ok_or(AgentValidationError::UnknownParentWork)?;
            let parent_spec = contract
                .work_specs
                .iter()
                .find(|candidate| candidate.id == parent_work.work_spec_id)
                .ok_or(AgentValidationError::UnknownWorkSpec)?;
            let alternative = matches!(
                &parent_spec.failure_plan,
                FailurePlan::Alternatives { work_spec_ids }
                    if work_spec_ids.contains(&work_spec_id)
            );
            if (!parent_spec.continuations.contains(&work_spec_id) && !alternative)
                || spec.generation_rank >= parent_spec.generation_rank
            {
                return Err(AgentValidationError::InvalidWorkContinuation);
            }
        }
        if !self.dependencies_closed(contract, spec.obligation) {
            return Err(AgentValidationError::WorkDependencyNotClosed);
        }
        if !spec.activation.holds(&self.effective_facts(facts)) {
            return Err(AgentValidationError::WorkActivationDoesNotHold);
        }
        let status = WorkStatus::Eligible;
        self.work_items.insert(
            id,
            WorkItem {
                id,
                run: self.id,
                goal: self.goal,
                owner: spec.owner,
                serves: spec.obligation,
                work_spec_id,
                slot,
                kind: spec.kind,
                parent,
                source_comment,
                status,
                attempt: 0,
                created_at: tick,
            },
        );
        self.dispatches.insert(
            id,
            WorkDispatch {
                work: id,
                attempt: 0,
                status: DispatchStatus::Ready,
                enqueued_at: tick,
                scheduler_position: 0,
            },
        );
        self.work_projection_history.push(WorkProjectionEvent {
            run: self.id,
            work: id,
            work_spec_id,
            slot,
            prior_status: None,
            attempt: 0,
            kind: WorkProjectionEventKind::Materialized,
            observed_at: tick,
        });
        self.push_causal_link(
            CollaborativeCausalNode::Obligation {
                obligation: spec.obligation,
            },
            CollaborativeCausalNode::Work { work: id },
            tick,
        )?;
        if let Some(parent) = parent {
            self.push_causal_link(
                CollaborativeCausalNode::Work { work: parent },
                CollaborativeCausalNode::Work { work: id },
                tick,
            )?;
        }
        if let Some(comment) = source_comment {
            self.push_causal_link(
                CollaborativeCausalNode::Comment { comment },
                CollaborativeCausalNode::Work { work: id },
                tick,
            )?;
        }
        Ok(id)
    }

    fn push_causal_link(
        &mut self,
        predecessor: CollaborativeCausalNode,
        successor: CollaborativeCausalNode,
        tick: u64,
    ) -> Result<(), AgentValidationError> {
        if predecessor == successor || self.causal_reachable(&successor, &predecessor) {
            return Err(AgentValidationError::CausalRankDoesNotDecrease);
        }
        if self
            .causal_links
            .iter()
            .any(|link| link.predecessor == predecessor && link.successor == successor)
        {
            return Ok(());
        }
        self.causal_links.push(CollaborativeCausalLink {
            run: self.id,
            goal: self.goal,
            predecessor,
            successor,
            observed_at: tick,
        });
        Ok(())
    }

    fn causal_reachable(
        &self,
        start: &CollaborativeCausalNode,
        target: &CollaborativeCausalNode,
    ) -> bool {
        let mut frontier = vec![start];
        let mut visited = HashSet::new();
        while let Some(node) = frontier.pop() {
            if node == target {
                return true;
            }
            if visited.insert(node) {
                frontier.extend(
                    self.causal_links
                        .iter()
                        .filter(|link| &link.predecessor == node)
                        .map(|link| &link.successor),
                );
            }
        }
        false
    }

    #[must_use]
    pub fn causal_rank(&self, node: &CollaborativeCausalNode) -> u64 {
        fn longest_path(
            links: &[CollaborativeCausalLink],
            node: &CollaborativeCausalNode,
            memo: &mut HashMap<CollaborativeCausalNode, u64>,
        ) -> u64 {
            if let Some(rank) = memo.get(node) {
                return *rank;
            }
            let rank = links
                .iter()
                .filter(|link| &link.predecessor == node)
                .map(|link| longest_path(links, &link.successor, memo).saturating_add(1))
                .max()
                .unwrap_or(0);
            memo.insert(node.clone(), rank);
            rank
        }
        longest_path(&self.causal_links, node, &mut HashMap::new())
    }

    #[must_use]
    pub fn causal_rank_decreases_on_every_link(&self) -> bool {
        self.causal_links
            .iter()
            .all(|link| self.causal_rank(&link.predecessor) > self.causal_rank(&link.successor))
    }

    pub fn create_external_blocker(
        &mut self,
        contract: &GoalContract,
        waiting_rule_id: u64,
        scope: BlockScope,
        condition: WaitingCondition,
        tick: u64,
        external_facts: &ExternalBlockerFacts,
    ) -> Result<BlockerId, AgentValidationError> {
        self.ensure_contract(contract)?;
        let rule = contract
            .waiting_rules
            .iter()
            .find(|rule| rule.id == waiting_rule_id)
            .ok_or(AgentValidationError::UnknownWaitingRule)?;
        let target_matches = match (&rule.target, &condition) {
            (
                ContractWaitingTarget::PrincipalResponse { principal },
                WaitingCondition::PrincipalResponse {
                    principal: observed,
                },
            ) => principal == observed && external_facts.human_principals.contains(observed),
            (
                ContractWaitingTarget::AdministratorApproval { administrator, .. },
                WaitingCondition::AdministratorApproval {
                    administrator: observed,
                },
            ) => administrator == observed && external_facts.administrators.contains(observed),
            (
                ContractWaitingTarget::TaskFromWork { work_spec_id },
                WaitingCondition::HumanTaskCompleted { task },
            ) => {
                external_facts.human_assigned_tasks.contains(task)
                    && matches!(
                        &scope,
                        BlockScope::Work { work }
                            if self.work_items.get(work).is_some_and(|item| {
                                item.work_spec_id == *work_spec_id
                                    && item.serves == rule.obligation
                            })
                    )
            }
            (
                ContractWaitingTarget::ExternalOutcome { condition },
                WaitingCondition::ExternalOutcome {
                    condition: observed,
                },
            ) => condition == observed,
            (
                ContractWaitingTarget::WorkTerminal { .. }
                | ContractWaitingTarget::ObligationDischarged { .. },
                _,
            ) => return Err(AgentValidationError::InternalBlockerForbidden),
            _ => false,
        };
        if !target_matches {
            return Err(AgentValidationError::WaitingRuleMismatch);
        }
        let scope_matches = match &scope {
            BlockScope::Work { work } => self
                .work_items
                .get(work)
                .is_some_and(|item| item.serves == rule.obligation),
            BlockScope::Obligation { obligation } => *obligation == rule.obligation,
            BlockScope::Goal { goal } => *goal == self.goal,
        };
        if !scope_matches {
            return Err(AgentValidationError::WaitingRuleMismatch);
        }
        let id = BlockerId::new();
        self.blockers.insert(
            id,
            WorkBlocker {
                id,
                run: self.id,
                goal: self.goal,
                scope: scope.clone(),
                obligation: rule.obligation,
                waiting_rule_id,
                condition,
                status: BlockerStatus::Waiting,
                created_at: tick,
                terminal_at: None,
            },
        );
        let applicable: Vec<_> = self
            .work_items
            .values()
            .filter(|work| match scope {
                BlockScope::Work { work: blocked } => blocked == work.id,
                BlockScope::Obligation { obligation } => obligation == work.serves,
                BlockScope::Goal { goal } => goal == work.goal,
            })
            .filter(|work| matches!(work.status, WorkStatus::Eligible | WorkStatus::Claimed))
            .map(|work| work.id)
            .collect();
        for work_id in applicable {
            let prior = self
                .work_items
                .get(&work_id)
                .ok_or(AgentValidationError::UnknownWorkItem)?
                .clone();
            if prior.status == WorkStatus::Claimed {
                let claim = self
                    .claims
                    .values_mut()
                    .find(|claim| claim.work == work_id && claim.status == ClaimStatus::Active)
                    .ok_or(AgentValidationError::ClaimedWorkMissingLease)?;
                claim.status = ClaimStatus::Released;
                let spec = contract
                    .work_specs
                    .iter()
                    .find(|spec| spec.id == prior.work_spec_id)
                    .ok_or(AgentValidationError::UnknownWorkSpec)?;
                self.suspended_claim_resolutions.insert(
                    work_id,
                    SuspendedClaimResolution {
                        work: work_id,
                        attempt: prior.attempt,
                        deadline: claim
                            .acquired_at
                            .saturating_add(u64::from(spec.max_resolution_ticks)),
                    },
                );
            }
            if let Some(work) = self.work_items.get_mut(&work_id) {
                work.status = WorkStatus::Blocked;
            }
            self.dispatches.remove(&work_id);
        }
        Ok(id)
    }

    fn derive_blocker_terminal_status(
        &self,
        contract: &GoalContract,
        blocker: &WorkBlocker,
        observation: &BlockerResolutionObservation,
        resolution_facts: &BlockerResolutionFacts,
    ) -> Result<BlockerStatus, AgentValidationError> {
        if observation.blocker() != blocker.id || observation.observed_at() < blocker.created_at {
            return Err(AgentValidationError::BlockerResolutionMismatch);
        }
        let rule = contract
            .waiting_rules
            .iter()
            .find(|rule| {
                rule.id == blocker.waiting_rule_id && rule.obligation == blocker.obligation
            })
            .ok_or(AgentValidationError::UnknownWaitingRule)?;
        match (&rule.target, &blocker.condition, observation) {
            (
                ContractWaitingTarget::TaskFromWork { .. },
                WaitingCondition::HumanTaskCompleted { task: expected },
                BlockerResolutionObservation::HumanTaskTerminal {
                    task,
                    outcome,
                    observed_at,
                    ..
                },
            ) if task == expected
                && resolution_facts.terminal_human_tasks.get(task)
                    == Some(&(*outcome, *observed_at)) =>
            {
                Ok(outcome.blocker_status())
            }
            (
                ContractWaitingTarget::AdministratorApproval {
                    administrator: expected_administrator,
                    review_work_spec_id: expected_work_spec,
                },
                WaitingCondition::AdministratorApproval {
                    administrator: condition_administrator,
                },
                BlockerResolutionObservation::AdministratorDecision {
                    decision,
                    administrator,
                    review_task,
                    review_work_spec_id,
                    outcome,
                    observed_at,
                    ..
                },
            ) if administrator == expected_administrator
                && administrator == condition_administrator
                && review_work_spec_id == expected_work_spec
                && resolution_facts.valid_administrator_decisions.get(decision)
                    == Some(&ValidAdministratorDecisionFact {
                        administrator: *administrator,
                        review_task: *review_task,
                        review_work_spec_id: *review_work_spec_id,
                        outcome: *outcome,
                        observed_at: *observed_at,
                    }) =>
            {
                Ok(outcome.blocker_status())
            }
            (
                ContractWaitingTarget::PrincipalResponse {
                    principal: expected,
                },
                WaitingCondition::PrincipalResponse {
                    principal: condition_principal,
                },
                BlockerResolutionObservation::PrincipalResponse {
                    principal,
                    comment,
                    observed_at,
                    ..
                },
            ) if principal == expected
                && principal == condition_principal
                && resolution_facts.principal_response_comments.get(comment)
                    == Some(&(*principal, *observed_at)) =>
            {
                Ok(BlockerStatus::Resolved)
            }
            (
                ContractWaitingTarget::ExternalOutcome {
                    condition: expected,
                },
                WaitingCondition::ExternalOutcome {
                    condition: condition_id,
                },
                BlockerResolutionObservation::ExternalOutcome {
                    observation,
                    condition,
                    outcome,
                    provenance_hash,
                    observed_at,
                    ..
                },
            ) if condition == expected
                && condition == condition_id
                && *provenance_hash != [0; 32]
                && resolution_facts.external_outcomes.get(observation)
                    == Some(&ValidExternalOutcomeFact {
                        condition: *condition,
                        outcome: *outcome,
                        provenance_hash: *provenance_hash,
                        observed_at: *observed_at,
                    }) =>
            {
                Ok(outcome.blocker_status())
            }
            _ => Err(AgentValidationError::BlockerResolutionNotObserved),
        }
    }

    pub fn resolve_blocker(
        &mut self,
        contract: &GoalContract,
        blocker_id: BlockerId,
        observation: BlockerResolutionObservation,
        resolution_facts: &BlockerResolutionFacts,
        condition_facts: &ContractConditionFacts,
    ) -> Result<BlockerStatus, AgentValidationError> {
        self.ensure_contract(contract)?;
        let tick = observation.observed_at();
        let blocker = self
            .blockers
            .get(&blocker_id)
            .ok_or(AgentValidationError::UnknownBlocker)?
            .clone();
        if blocker.status != BlockerStatus::Waiting {
            return Err(AgentValidationError::InvalidBlockerTransition);
        }
        let status = self.derive_blocker_terminal_status(
            contract,
            &blocker,
            &observation,
            resolution_facts,
        )?;
        let stored = self
            .blockers
            .get_mut(&blocker_id)
            .ok_or(AgentValidationError::UnknownBlocker)?;
        stored.status = status;
        stored.terminal_at = Some(tick);
        self.blocker_resolutions.push(BlockerResolutionRecord {
            blocker: blocker_id,
            observation,
            terminal_status: status,
            observed_at: tick,
        });
        let effective_facts = self.effective_facts(condition_facts);
        let eligible: Vec<_> = self
            .work_items
            .values()
            .filter(|work| {
                work.status == WorkStatus::Blocked
                    && !self.work_has_waiting_blocker(work)
                    && self.dependencies_closed(contract, work.serves)
                    && contract
                        .work_specs
                        .iter()
                        .find(|spec| spec.id == work.work_spec_id)
                        .is_some_and(|spec| spec.activation.holds(&effective_facts))
            })
            .map(|work| (work.id, work.attempt))
            .collect();
        for (work_id, attempt) in eligible {
            if let Some(work) = self.work_items.get_mut(&work_id) {
                work.status = WorkStatus::Eligible;
            }
            self.dispatches.insert(
                work_id,
                WorkDispatch {
                    work: work_id,
                    attempt,
                    status: DispatchStatus::Ready,
                    enqueued_at: tick,
                    scheduler_position: 0,
                },
            );
        }
        self.enforce_suspended_claim_deadlines(tick);
        self.validate_current_projection(contract, &effective_facts)?;
        Ok(status)
    }

    pub fn recover_expired_claims(&mut self, tick: u64) {
        let expired: Vec<_> = self
            .claims
            .values()
            .filter(|claim| claim.status == ClaimStatus::Active && claim.expires_at <= tick)
            .map(|claim| (claim.id, claim.work, claim.attempt))
            .collect();
        for (claim_id, work_id, attempt) in expired {
            if let Some(claim) = self.claims.get_mut(&claim_id) {
                claim.status = ClaimStatus::Expired;
            }
            if let Some(work) = self.work_items.get_mut(&work_id)
                && work.status == WorkStatus::Claimed
                && work.attempt == attempt
            {
                work.status = WorkStatus::Eligible;
                self.dispatches.insert(
                    work_id,
                    WorkDispatch {
                        work: work_id,
                        attempt,
                        status: DispatchStatus::Ready,
                        enqueued_at: tick,
                        scheduler_position: 0,
                    },
                );
            }
        }
    }

    pub fn claim_next(
        &mut self,
        contract: &GoalContract,
        claimant: UserId,
        facts: &ContractConditionFacts,
        tick: u64,
        lease_ticks: u64,
        aging_step: u64,
    ) -> Result<Option<WorkClaim>, AgentValidationError> {
        self.ensure_contract(contract)?;
        if lease_ticks == 0 || aging_step == 0 {
            return Err(AgentValidationError::InvalidSchedulerBound);
        }
        self.recover_expired_claims(tick);
        let mut eligible: Vec<_> = self
            .work_items
            .values()
            .filter(|work| {
                work.owner == claimant
                    && work.status == WorkStatus::Eligible
                    && self.dependencies_closed(contract, work.serves)
                    && contract
                        .work_specs
                        .iter()
                        .find(|spec| spec.id == work.work_spec_id)
                        .is_some_and(|spec| spec.activation.holds(&self.effective_facts(facts)))
            })
            .map(|work| {
                (
                    work.id,
                    tick.saturating_sub(work.created_at)
                        .saturating_mul(aging_step),
                    work.created_at,
                )
            })
            .collect();
        eligible.sort_by_key(|(id, effective_age, created_at)| {
            (std::cmp::Reverse(*effective_age), *created_at, *id)
        });
        for (position, (work_id, _, _)) in eligible.iter().enumerate() {
            if let Some(dispatch) = self.dispatches.get_mut(work_id) {
                dispatch.scheduler_position = u64::try_from(position).unwrap_or(u64::MAX);
            }
        }
        let Some((work_id, _, _)) = eligible.first().copied() else {
            return Ok(None);
        };
        if self.claims.values().any(|claim| {
            claim.work == work_id && claim.status == ClaimStatus::Active && claim.expires_at > tick
        }) {
            return Err(AgentValidationError::ExclusiveClaimViolation);
        }
        let work = self
            .work_items
            .get_mut(&work_id)
            .ok_or(AgentValidationError::UnknownWorkItem)?;
        let spec = contract
            .work_specs
            .iter()
            .find(|spec| spec.id == work.work_spec_id)
            .ok_or(AgentValidationError::UnknownWorkSpec)?;
        if work.attempt >= spec.max_attempts {
            return Err(AgentValidationError::WorkAttemptsExhausted);
        }
        work.attempt += 1;
        work.status = WorkStatus::Claimed;
        if self
            .suspended_claim_resolutions
            .get(&work_id)
            .is_some_and(|resolution| work.attempt > resolution.attempt)
        {
            self.suspended_claim_resolutions.remove(&work_id);
        }
        // `lease_ticks` is an operational preference. The normative bound is
        // the selected WorkSpec's `max_resolution_ticks` (R5.30 execution
        // dynamics), so a lease may shorten but never extend that certificate.
        let effective_lease_ticks = lease_ticks.min(u64::from(spec.max_resolution_ticks));
        let claim = WorkClaim {
            id: ClaimId::new(),
            work: work_id,
            attempt: work.attempt,
            claimant,
            acquired_at: tick,
            expires_at: tick.saturating_add(effective_lease_ticks),
            status: ClaimStatus::Active,
        };
        self.claims.insert(claim.id, claim.clone());
        if let Some(dispatch) = self.dispatches.get_mut(&work_id) {
            dispatch.status = DispatchStatus::Claimed;
            dispatch.attempt = work.attempt;
        }
        Ok(Some(claim))
    }

    fn valid_claim(
        &self,
        claim_id: ClaimId,
        claimant: UserId,
        tick: u64,
    ) -> Result<&WorkClaim, AgentValidationError> {
        let claim = self
            .claims
            .get(&claim_id)
            .ok_or(AgentValidationError::UnknownClaim)?;
        if claim.claimant != claimant
            || claim.status != ClaimStatus::Active
            || claim.expires_at <= tick
        {
            return Err(AgentValidationError::ExpiredOrForeignClaim);
        }
        let work = self
            .work_items
            .get(&claim.work)
            .ok_or(AgentValidationError::UnknownWorkItem)?;
        if work.status != WorkStatus::Claimed || work.attempt != claim.attempt {
            return Err(AgentValidationError::ExpiredOrForeignClaim);
        }
        Ok(claim)
    }

    pub fn succeed_work(
        &mut self,
        contract: &GoalContract,
        claim_id: ClaimId,
        claimant: UserId,
        facts: &ContractConditionFacts,
        tick: u64,
    ) -> Result<Vec<WorkItemId>, AgentValidationError> {
        self.ensure_contract(contract)?;
        let work_id = self.valid_claim(claim_id, claimant, tick)?.work;
        let work_spec_id = self
            .work_items
            .get(&work_id)
            .ok_or(AgentValidationError::UnknownWorkItem)?
            .work_spec_id;
        let effective_facts = self.effective_facts(facts);
        let continuations = contract
            .work_specs
            .iter()
            .find(|spec| spec.id == work_spec_id)
            .ok_or(AgentValidationError::UnknownWorkSpec)?;
        if !continuations.activation.holds(&effective_facts) {
            return Err(AgentValidationError::WorkActivationNoLongerHolds);
        }
        let continuations = continuations.continuations.clone();
        self.close_claim_and_work(claim_id, work_id, WorkStatus::Succeeded)?;
        let mut children = Vec::new();
        for continuation in continuations {
            if contract
                .work_specs
                .iter()
                .find(|spec| spec.id == continuation)
                .is_none_or(|spec| !spec.activation.holds(&effective_facts))
            {
                continue;
            }
            if let Some(slot) = self.first_free_slot(contract, continuation) {
                children.push(self.materialize_work(
                    contract,
                    WorkMaterialization {
                        work_spec_id: continuation,
                        slot,
                        parent: Some(work_id),
                        source_comment: None,
                        tick,
                    },
                    facts,
                )?);
            }
        }
        Ok(children)
    }

    pub fn fail_work(
        &mut self,
        contract: &GoalContract,
        claim_id: ClaimId,
        claimant: UserId,
        facts: &ContractConditionFacts,
        tick: u64,
    ) -> Result<Vec<WorkItemId>, AgentValidationError> {
        self.fail_work_with_evidence(contract, claim_id, claimant, facts, tick, None)
    }

    pub fn fail_work_with_evidence(
        &mut self,
        contract: &GoalContract,
        claim_id: ClaimId,
        claimant: UserId,
        facts: &ContractConditionFacts,
        tick: u64,
        discharge_evidence: Option<EvidenceId>,
    ) -> Result<Vec<WorkItemId>, AgentValidationError> {
        self.ensure_contract(contract)?;
        let work_id = self.valid_claim(claim_id, claimant, tick)?.work;
        let work = self
            .work_items
            .get(&work_id)
            .ok_or(AgentValidationError::UnknownWorkItem)?
            .clone();
        let spec = contract
            .work_specs
            .iter()
            .find(|spec| spec.id == work.work_spec_id)
            .ok_or(AgentValidationError::UnknownWorkSpec)?;
        match &spec.failure_plan {
            FailurePlan::RetrySame {} if work.attempt < spec.max_attempts => {
                if let Some(claim) = self.claims.get_mut(&claim_id) {
                    claim.status = ClaimStatus::Released;
                }
                let current = self
                    .work_items
                    .get_mut(&work_id)
                    .ok_or(AgentValidationError::UnknownWorkItem)?;
                current.status = WorkStatus::Eligible;
                self.dispatches.insert(
                    work_id,
                    WorkDispatch {
                        work: work_id,
                        attempt: current.attempt,
                        status: DispatchStatus::Ready,
                        enqueued_at: tick,
                        scheduler_position: 0,
                    },
                );
                Ok(Vec::new())
            }
            FailurePlan::Alternatives { work_spec_ids } => {
                let alternatives = work_spec_ids.clone();
                self.close_claim_and_work(claim_id, work_id, WorkStatus::Failed)?;
                let mut children = Vec::new();
                for alternative in alternatives {
                    if contract
                        .work_specs
                        .iter()
                        .find(|spec| spec.id == alternative)
                        .is_none_or(|spec| !spec.activation.holds(&self.effective_facts(facts)))
                    {
                        continue;
                    }
                    if let Some(slot) = self.first_free_slot(contract, alternative) {
                        children.push(self.materialize_work(
                            contract,
                            WorkMaterialization {
                                work_spec_id: alternative,
                                slot,
                                parent: Some(work_id),
                                source_comment: None,
                                tick,
                            },
                            facts,
                        )?);
                    }
                }
                if children.is_empty() {
                    self.goal_status = GoalStatus::Failed;
                }
                Ok(children)
            }
            FailurePlan::DischargeBy { evidence_rule_id } => {
                let evidence = discharge_evidence
                    .and_then(|id| self.evidence.iter().find(|record| record.id == id))
                    .ok_or(AgentValidationError::FailureDischargeEvidenceRequired)?;
                if evidence.rule_id != *evidence_rule_id
                    || evidence.obligation != work.serves
                    || self
                        .obligations
                        .get(&work.serves)
                        .is_none_or(|instance| instance.status != ObligationStatus::Discharged)
                {
                    return Err(AgentValidationError::FailureDischargeEvidenceRequired);
                }
                self.close_claim_and_work(claim_id, work_id, WorkStatus::Failed)?;
                Ok(Vec::new())
            }
            FailurePlan::FailGoal {} | FailurePlan::RetrySame {} => {
                self.close_claim_and_work(claim_id, work_id, WorkStatus::Failed)?;
                self.goal_status = GoalStatus::Failed;
                Ok(Vec::new())
            }
        }
    }

    fn close_claim_and_work(
        &mut self,
        claim_id: ClaimId,
        work_id: WorkItemId,
        status: WorkStatus,
    ) -> Result<(), AgentValidationError> {
        self.claims
            .get_mut(&claim_id)
            .ok_or(AgentValidationError::UnknownClaim)?
            .status = ClaimStatus::Released;
        self.work_items
            .get_mut(&work_id)
            .ok_or(AgentValidationError::UnknownWorkItem)?
            .status = status;
        if let Some(dispatch) = self.dispatches.get_mut(&work_id) {
            dispatch.status = DispatchStatus::Closed;
        }
        Ok(())
    }

    fn first_free_slot(&self, contract: &GoalContract, work_spec_id: u64) -> Option<u32> {
        let spec = contract
            .work_specs
            .iter()
            .find(|candidate| candidate.id == work_spec_id)?;
        (0..spec.max_instances).find(|slot| {
            self.work_slots
                .get(&(work_spec_id, *slot))
                .is_some_and(|id| {
                    !self.work_items.contains_key(id) && !self.inactive_work_items.contains_key(id)
                })
        })
    }

    pub fn accept_evidence(
        &mut self,
        contract: &GoalContract,
        record: EvidenceRecord,
        validate_mechanical: impl Fn(&EvidenceRecord, &ContractEvidenceRule) -> bool,
        validate_semantic: impl Fn(&EvidenceRecord, &ContractEvidenceRule) -> bool,
    ) -> Result<(), AgentValidationError> {
        self.ensure_contract(contract)?;
        if record.run != self.id || self.evidence.iter().any(|item| item.id == record.id) {
            return Err(AgentValidationError::InvalidEvidenceProvenance);
        }
        let rule = contract
            .evidence_rules
            .iter()
            .find(|rule| rule.id == record.rule_id)
            .ok_or(AgentValidationError::UnknownEvidenceRule)?;
        if record.obligation != rule.obligation
            || record.kind != rule.kind
            || !self.evidence_subject_matches(rule, &record)
        {
            return Err(AgentValidationError::InvalidEvidenceProvenance);
        }
        if rule.verification == EvidenceVerificationMode::SemanticJudgment
            && !validate_semantic(&record, rule)
        {
            return Err(AgentValidationError::SemanticEvidenceRejected);
        }
        if rule.verification == EvidenceVerificationMode::Mechanical
            && !validate_mechanical(&record, rule)
        {
            return Err(AgentValidationError::InvalidMechanicalEvidence);
        }
        let obligation = self
            .obligations
            .get_mut(&record.obligation)
            .ok_or(AgentValidationError::UnknownObligationInstance)?;
        obligation.status = ObligationStatus::Discharged;
        obligation.discharged_at = Some(record.observed_at);
        self.evidence.push(record);
        Ok(())
    }

    fn evidence_subject_matches(
        &self,
        rule: &ContractEvidenceRule,
        record: &EvidenceRecord,
    ) -> bool {
        match (&rule.subject, &record.subject) {
            (
                ContractEvidenceSubject::WorkResult { work_spec_id },
                EvidenceSubject::ToolCall { .. } | EvidenceSubject::Task { .. },
            ) => record.work.is_some_and(|work_id| {
                self.work_items
                    .get(&work_id)
                    .is_some_and(|work| work.work_spec_id == *work_spec_id)
            }),
            (
                ContractEvidenceSubject::Principal { principal },
                EvidenceSubject::Principal {
                    principal: observed,
                },
            ) => principal == observed,
            (
                ContractEvidenceSubject::Obligation { obligation },
                EvidenceSubject::Obligation {
                    obligation: observed,
                },
            ) => obligation == observed,
            (
                ContractEvidenceSubject::AdministratorDecision { administrator, .. },
                EvidenceSubject::AdministratorDecision {
                    administrator: observed,
                    ..
                },
            ) => administrator == observed,
            (
                ContractEvidenceSubject::ExternalCondition { condition },
                EvidenceSubject::ExternalCondition {
                    condition: observed,
                },
            ) => condition == observed,
            (ContractEvidenceSubject::Derived {}, EvidenceSubject::Derived {}) => true,
            _ => false,
        }
    }

    pub fn try_complete(&mut self, contract: &GoalContract, facts: &ContractConditionFacts) {
        let effective_facts = self.effective_facts(facts);
        let all_required_discharged = contract.obligations.iter().all(|spec| {
            !spec.activation.holds(&effective_facts)
                || !spec.required_for_completion.holds(&effective_facts)
                || self
                    .obligations
                    .get(&spec.id)
                    .is_some_and(|instance| instance.status == ObligationStatus::Discharged)
        });
        let no_open_work = self
            .work_items
            .values()
            .all(|work| work.status.is_terminal());
        let no_waiting_blockers = self
            .blockers
            .values()
            .all(|blocker| blocker.status.is_terminal());
        if contract.completion_condition.holds(&effective_facts)
            && all_required_discharged
            && no_open_work
            && no_waiting_blockers
        {
            self.goal_status = GoalStatus::Completed;
        }
    }

    pub fn complete_run(&mut self) -> Result<(), AgentValidationError> {
        if self.goal_status != GoalStatus::Completed
            || self
                .obligations
                .values()
                .any(|instance| instance.status != ObligationStatus::Discharged)
            || self
                .work_items
                .values()
                .any(|work| !work.status.is_terminal())
            || self
                .blockers
                .values()
                .any(|blocker| !blocker.status.is_terminal())
        {
            return Err(AgentValidationError::CollaborativeGoalIncomplete);
        }
        self.run_status = CollaborativeRunStatus::Completed;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGoalClause {
    pub id: u64,
    pub domain: u64,
    pub scope: ResourceId,
    pub work_spec_ids: Vec<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalGoalOrigin {
    ControllerPrompt {},
    AdministratorException { review_id: Uuid },
    GlobalMandate { global_revision: u64 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
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

/// Operational bottom-up gate from R5.37. Organizational domains remain
/// metadata: they cannot widen the concrete resource scope or action classes.
pub fn responsibility_operationally_covers_local_goal(
    responsibility: &ResponsibilityContract,
    local_goal: &LocalGoalContract,
    resource_within_scope: impl Fn(ResourceId, ResourceId) -> bool,
) -> bool {
    if responsibility.user != local_goal.controller || local_goal.validate().is_err() {
        return false;
    }
    local_goal.contract.work_specs.iter().all(|work| {
        responsibility.rules.iter().any(|rule| {
            resource_within_scope(rule.scope, local_goal.contract.scope)
                && work
                    .allowed_actions
                    .iter()
                    .all(|action| rule.allowed_actions.contains(action))
        })
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalLocalContribution {
    pub agent: UserId,
    pub local_revision: u64,
    pub local_clause_id: u64,
    pub global_work_spec_ids: Vec<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredGlobalWorkGrounding {
    pub global_work_spec_id: u64,
    pub source_agent: UserId,
    pub source_local_revision: u64,
    pub source_work_spec_id: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalContractCandidate {
    pub revision: u64,
    pub contract: GoalContract,
    pub contributions: Vec<GlobalLocalContribution>,
    pub governance_conflicts: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredGlobalSynthesisEnvelope {
    pub language_task: StructuredLanguageTaskEnvelope,
    pub source_agents: Vec<UserId>,
    pub max_global_obligations: u32,
    pub max_global_work_specs: u32,
    pub max_dependencies: u32,
    pub max_conflicts: u32,
}

/// Validates the automatic bottom-up path for a candidate synthesized on an
/// authorized plaintext-holding client/runner. This function performs no
/// semantic synthesis and sees only the minimum structural contract metadata.
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
    ensure_unique(&envelope.source_agents, "global source agent")?;
    ensure_unique_by(
        &candidate.contributions,
        |contribution| contribution.agent,
        "global contribution agent",
    )?;
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

pub struct CurrentLocalObligationContext<'a> {
    pub active_local_goal: Option<&'a LocalGoalContract>,
    pub provenance: Option<&'a TaskObligationProvenance>,
    pub condition_facts: &'a ContractConditionFacts,
}

/// Exact product provenance for a task materialized from a currently required
/// LocalGoal obligation. This is independent of how the task was routed or
/// assigned; cross-owner governance is only one possible origin.
#[must_use]
pub fn task_obligation_provenance_valid_at(
    task: ResourceId,
    target_agent: &GovernedAgent,
    local_obligation: CurrentLocalObligationContext<'_>,
) -> bool {
    let (Some(local), Some(provenance)) = (
        local_obligation.active_local_goal,
        local_obligation.provenance,
    ) else {
        return false;
    };
    provenance.task == task
        && provenance.agent == target_agent.principal_id
        && provenance.local_revision == local.revision
        && local.contract.obligations.iter().any(|obligation| {
            obligation.id == provenance.obligation
                && obligation.owner == target_agent.principal_id
                && obligation
                    .activation
                    .holds(local_obligation.condition_facts)
                && obligation
                    .required_for_completion
                    .holds(local_obligation.condition_facts)
        })
        && local.contract.work_specs.iter().any(|work| {
            work.id == provenance.work_spec_id
                && work.obligation == provenance.obligation
                && work.owner == target_agent.principal_id
        })
}

/// Cross-owner routing never treats a linguistic label as authority. Exact
/// active obligation provenance enables the automatic route; otherwise only a
/// persisted intent covered by the target controller's responsibility can
/// open a review. Everything else is rejected.
pub fn route_cross_owner_assignment(
    task: ResourceId,
    target_agent: &GovernedAgent,
    local_obligation: CurrentLocalObligationContext<'_>,
    intent: Option<&PersistedTaskIntent>,
    controller_responsibility: Option<&ResponsibilityContract>,
    resource_within_scope: impl Fn(ResourceId, ResourceId) -> bool,
) -> CrossOwnerAssignmentRoute {
    if task_obligation_provenance_valid_at(task, target_agent, local_obligation) {
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
    #[error("goal completion condition must be normalized to always")]
    UnnormalizedCompletionCondition,
    #[error("obligation references a different goal")]
    ObligationGoalMismatch,
    #[error("goal contract references an unknown obligation")]
    UnknownObligation,
    #[error("dependency rank does not strictly decrease")]
    DependencyRankDoesNotDecrease,
    #[error("each obligation must have exactly one entry work specification")]
    ObligationEntryCardinality,
    #[error("required obligation conditions do not imply entry work activation")]
    EntryActivationNotRequired,
    #[error("work owner does not match its obligation owner")]
    WorkOwnerMismatch,
    #[error("every obligation must declare at least one evidence rule")]
    MissingObligationEvidenceRule,
    #[error("goal contract references an unknown evidence rule")]
    UnknownEvidenceRule,
    #[error("failure discharge rule does not serve the same obligation")]
    InvalidDischargeRule,
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
    #[error("run and goal contract identity or scope do not match")]
    RunContractMismatch,
    #[error("work slot is outside the WorkSpec maximum")]
    WorkSlotOutOfRange,
    #[error("work slot is not part of the canonical finite universe")]
    UnknownWorkSlot,
    #[error("canonical work slot has already been materialized")]
    WorkSlotAlreadyMaterialized,
    #[error("work cannot be materialized before its internal dependencies close")]
    WorkDependencyNotClosed,
    #[error("work cannot be projected while its activation condition is false")]
    WorkActivationDoesNotHold,
    #[error("a required minimal obligation has an inactive entry work specification")]
    RequiredEntryWorkInactive,
    #[error("work continuation is not declared or does not decrease rank")]
    InvalidWorkContinuation,
    #[error("work parent does not exist")]
    UnknownParentWork,
    #[error("causal graph rank must strictly decrease")]
    CausalRankDoesNotDecrease,
    #[error("goal contract references an unknown waiting rule")]
    UnknownWaitingRule,
    #[error("internal waiting must use work/dependency progress, not an opaque blocker")]
    InternalBlockerForbidden,
    #[error("runtime blocker does not match its declared waiting rule")]
    WaitingRuleMismatch,
    #[error("blocker resolution observation refers to a different blocker or time")]
    BlockerResolutionMismatch,
    #[error("blocker resolution is not backed by a matching authoritative observation")]
    BlockerResolutionNotObserved,
    #[error("blocker does not exist")]
    UnknownBlocker,
    #[error("blocker transition is invalid")]
    InvalidBlockerTransition,
    #[error("scheduler aging and lease bounds must be positive")]
    InvalidSchedulerBound,
    #[error("a work attempt has more than one valid claim")]
    ExclusiveClaimViolation,
    #[error("work item does not exist")]
    UnknownWorkItem,
    #[error("work attempts are exhausted")]
    WorkAttemptsExhausted,
    #[error("work activation condition no longer holds in the current state")]
    WorkActivationNoLongerHolds,
    #[error("claimed work has no corresponding active lease")]
    ClaimedWorkMissingLease,
    #[error("current semantic work projection contains inactive work")]
    CurrentWorkActivationFalse,
    #[error("current semantic work projection violates canonical slot identity")]
    CanonicalWorkIdentityViolation,
    #[error("eligible work does not satisfy contract-native eligibility")]
    EligibleWorkInvariantViolation,
    #[error("blocked work has no applicable waiting blocker")]
    BlockedWorkMissingWaitingBlocker,
    #[error("failure-plan discharge requires accepted evidence from its declared rule")]
    FailureDischargeEvidenceRequired,
    #[error("claim does not exist")]
    UnknownClaim,
    #[error("claim is expired, released or belongs to another claimant")]
    ExpiredOrForeignClaim,
    #[error("evidence provenance is incomplete or does not match its rule")]
    InvalidEvidenceProvenance,
    #[error("semantic evidence was not accepted by the configured judge")]
    SemanticEvidenceRejected,
    #[error("mechanical evidence is not backed by typed product provenance")]
    InvalidMechanicalEvidence,
    #[error("obligation instance does not exist")]
    UnknownObligationInstance,
    #[error("the collaborative goal is not complete")]
    CollaborativeGoalIncomplete,
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
        let goal = GoalId::new();
        let scope = ResourceId::new();
        LocalGoalContract {
            id: LocalGoalId::new(),
            revision: 1,
            agent,
            controller,
            encrypted_prompt: payload(),
            contract: GoalContract {
                goal,
                scope,
                obligations: vec![ContractObligation {
                    id: obligation,
                    goal,
                    owner: agent,
                    activation: ContractCondition::always(),
                    required_for_completion: ContractCondition::always(),
                    dependency_rank: 0,
                }],
                dependencies: Vec::new(),
                work_specs: vec![ContractWorkSpec {
                    id: 1,
                    obligation,
                    owner: agent,
                    kind: WorkKind::AgentAction,
                    activation: ContractCondition::always(),
                    allowed_actions: vec![AgentActionClass::CreateTask],
                    max_instances: 2,
                    max_attempts: 2,
                    max_resolution_ticks: 5,
                    generation_rank: 0,
                    is_entry: true,
                    continuations: Vec::new(),
                    failure_plan: FailurePlan::FailGoal {},
                }],
                evidence_rules: vec![ContractEvidenceRule {
                    id: 1,
                    obligation,
                    kind: EvidenceKind::DerivedFact,
                    subject: ContractEvidenceSubject::Derived {},
                    verification: EvidenceVerificationMode::SemanticJudgment,
                }],
                waiting_rules: Vec::new(),
                completion_condition: ContractCondition::always(),
            },
            clauses: vec![LocalGoalClause {
                id: 1,
                domain: 7,
                scope,
                work_spec_ids: vec![1],
            }],
            origin: LocalGoalOrigin::ControllerPrompt {},
            supersedes_revision: None,
        }
    }

    fn local_goal_with_conditional_continuation(
        agent: UserId,
        controller: UserId,
        activation: ContractCondition,
    ) -> LocalGoalContract {
        let mut local = local_goal(agent, controller);
        let obligation = local.contract.obligations[0].id;
        local.contract.work_specs[0].generation_rank = 1;
        local.contract.work_specs[0].continuations = vec![2];
        local.contract.work_specs.push(ContractWorkSpec {
            id: 2,
            obligation,
            owner: agent,
            kind: WorkKind::AgentAction,
            activation,
            allowed_actions: vec![AgentActionClass::CreateTask],
            max_instances: 1,
            max_attempts: 2,
            max_resolution_ticks: 5,
            generation_rank: 0,
            is_entry: false,
            continuations: Vec::new(),
            failure_plan: FailurePlan::FailGoal {},
        });
        local.clauses[0].work_spec_ids.push(2);
        local
    }

    fn blocked_run(
        target: ContractWaitingTarget,
        condition: WaitingCondition,
        external_facts: ExternalBlockerFacts,
    ) -> (
        LocalGoalContract,
        CollaborativeRunState,
        WorkItemId,
        BlockerId,
    ) {
        let agent = UserId::new();
        let mut local = local_goal(agent, UserId::new());
        local.contract.waiting_rules.push(ContractWaitingRule {
            id: 1,
            obligation: local.contract.obligations[0].id,
            target,
        });
        let facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &facts, 0).unwrap();
        let work = run.work_items.values().next().unwrap().id;
        let blocker = run
            .create_external_blocker(
                &local.contract,
                1,
                BlockScope::Work { work },
                condition,
                1,
                &external_facts,
            )
            .unwrap();
        (local, run, work, blocker)
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
        assert!(responsibility_operationally_covers_local_goal(
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
        assert!(!responsibility_operationally_covers_local_goal(
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
                CurrentLocalObligationContext {
                    active_local_goal: Some(&local),
                    provenance: Some(&provenance),
                    condition_facts: &ContractConditionFacts::default(),
                },
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
                CurrentLocalObligationContext {
                    active_local_goal: Some(&local),
                    provenance: Some(&stale),
                    condition_facts: &ContractConditionFacts::default(),
                },
                None,
                None,
                |_, _| false,
            ),
            CrossOwnerAssignmentRoute::Rejected
        );
    }

    #[test]
    fn cross_owner_automatic_route_requires_condition_currently_required() {
        let agent_principal = UserId::new();
        let controller = UserId::new();
        let mut local = local_goal(agent_principal, controller);
        let task = ResourceId::new();
        let unmet_task = ResourceId::new();
        local.contract.obligations[0].required_for_completion =
            ContractCondition::TaskDone { task: unmet_task };
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
                CurrentLocalObligationContext {
                    active_local_goal: Some(&local),
                    provenance: Some(&provenance),
                    condition_facts: &ContractConditionFacts::default(),
                },
                None,
                None,
                |_, _| false,
            ),
            CrossOwnerAssignmentRoute::Rejected
        );
    }

    #[test]
    fn cross_owner_automatic_route_requires_condition_currently_active() {
        let agent_principal = UserId::new();
        let controller = UserId::new();
        let administrator = UserId::new();
        let mut local = local_goal(agent_principal, controller);
        let task = ResourceId::new();
        let unmet_task = ResourceId::new();
        local.contract.obligations[0].activation = ContractCondition::TaskDone { task: unmet_task };
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
        let intent = PersistedTaskIntent {
            task,
            scope: local.contract.scope,
            required_actions: vec![AgentActionClass::CreateTask],
            created_by: controller,
            recorded_at: Utc::now(),
        };
        let responsibility = ResponsibilityContract {
            id: ResponsibilityId::new(),
            revision: 1,
            administrator,
            user: controller,
            encrypted_source_text: payload(),
            rules: vec![ResponsibilityRule {
                domain: 7,
                scope: local.contract.scope,
                allowed_actions: vec![AgentActionClass::CreateTask],
            }],
            supersedes_revision: None,
        };

        assert_eq!(
            route_cross_owner_assignment(
                task,
                &governed,
                CurrentLocalObligationContext {
                    active_local_goal: Some(&local),
                    provenance: Some(&provenance),
                    condition_facts: &ContractConditionFacts::default(),
                },
                Some(&intent),
                Some(&responsibility),
                |parent, child| parent == child,
            ),
            CrossOwnerAssignmentRoute::ControllerReview
        );
    }

    #[test]
    fn collaborative_completion_waits_for_every_participant_obligation() {
        let first = UserId::new();
        let second = UserId::new();
        let first_obligation = Uuid::now_v7();
        let second_obligation = Uuid::now_v7();
        let goal = GoalId::new();
        let contract = GoalContract {
            goal,
            scope: ResourceId::new(),
            obligations: vec![
                ContractObligation {
                    id: first_obligation,
                    goal,
                    owner: first,
                    activation: ContractCondition::always(),
                    required_for_completion: ContractCondition::always(),
                    dependency_rank: 0,
                },
                ContractObligation {
                    id: second_obligation,
                    goal,
                    owner: second,
                    activation: ContractCondition::always(),
                    required_for_completion: ContractCondition::always(),
                    dependency_rank: 0,
                },
            ],
            dependencies: Vec::new(),
            work_specs: vec![
                ContractWorkSpec {
                    id: 1,
                    obligation: first_obligation,
                    owner: first,
                    kind: WorkKind::AgentAction,
                    activation: ContractCondition::always(),
                    allowed_actions: vec![AgentActionClass::CreateTask],
                    max_instances: 1,
                    max_attempts: 2,
                    max_resolution_ticks: 5,
                    generation_rank: 0,
                    is_entry: true,
                    continuations: Vec::new(),
                    failure_plan: FailurePlan::FailGoal {},
                },
                ContractWorkSpec {
                    id: 2,
                    obligation: second_obligation,
                    owner: second,
                    kind: WorkKind::AgentAction,
                    activation: ContractCondition::always(),
                    allowed_actions: vec![AgentActionClass::CreateTask],
                    max_instances: 1,
                    max_attempts: 2,
                    max_resolution_ticks: 5,
                    generation_rank: 0,
                    is_entry: true,
                    continuations: Vec::new(),
                    failure_plan: FailurePlan::FailGoal {},
                },
            ],
            evidence_rules: vec![
                ContractEvidenceRule {
                    id: 1,
                    obligation: first_obligation,
                    kind: EvidenceKind::DerivedFact,
                    subject: ContractEvidenceSubject::Derived {},
                    verification: EvidenceVerificationMode::SemanticJudgment,
                },
                ContractEvidenceRule {
                    id: 2,
                    obligation: second_obligation,
                    kind: EvidenceKind::DerivedFact,
                    subject: ContractEvidenceSubject::Derived {},
                    verification: EvidenceVerificationMode::SemanticJudgment,
                },
            ],
            waiting_rules: Vec::new(),
            completion_condition: ContractCondition::always(),
        };
        let facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &contract, &facts, 0).unwrap();
        assert_eq!(run.participants, HashSet::from([first, second]));
        assert_eq!(run.work_items.len(), 2);

        let first_claim = run
            .claim_next(&contract, first, &facts, 1, 5, 1)
            .unwrap()
            .unwrap();
        run.succeed_work(&contract, first_claim.id, first, &facts, 2)
            .unwrap();
        run.accept_evidence(
            &contract,
            EvidenceRecord {
                id: EvidenceId::new(),
                run: run.id,
                obligation: first_obligation,
                rule_id: 1,
                kind: EvidenceKind::DerivedFact,
                subject: EvidenceSubject::Derived {},
                work: Some(first_claim.work),
                observed_at: 2,
                provenance_hash: [1; 32],
            },
            |_, _| false,
            |_, _| true,
        )
        .unwrap();
        run.try_complete(&contract, &facts);
        assert_eq!(run.goal_status, GoalStatus::Active);
        assert_eq!(run.run_status, CollaborativeRunStatus::Running);

        let second_claim = run
            .claim_next(&contract, second, &facts, 3, 5, 1)
            .unwrap()
            .unwrap();
        run.succeed_work(&contract, second_claim.id, second, &facts, 4)
            .unwrap();
        run.accept_evidence(
            &contract,
            EvidenceRecord {
                id: EvidenceId::new(),
                run: run.id,
                obligation: second_obligation,
                rule_id: 2,
                kind: EvidenceKind::DerivedFact,
                subject: EvidenceSubject::Derived {},
                work: Some(second_claim.work),
                observed_at: 4,
                provenance_hash: [2; 32],
            },
            |_, _| false,
            |_, _| true,
        )
        .unwrap();
        run.try_complete(&contract, &facts);
        assert_eq!(run.goal_status, GoalStatus::Completed);
        assert_eq!(run.run_status, CollaborativeRunStatus::Running);
        run.complete_run().unwrap();
        assert_eq!(run.run_status, CollaborativeRunStatus::Completed);
    }

    #[test]
    fn expired_claim_recovers_the_same_canonical_work_slot() {
        let agent = UserId::new();
        let local = local_goal(agent, UserId::new());
        let facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &facts, 10).unwrap();
        let first = run
            .claim_next(&local.contract, agent, &facts, 11, 2, 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            run.succeed_work(&local.contract, first.id, agent, &facts, 13),
            Err(AgentValidationError::ExpiredOrForeignClaim)
        );
        run.recover_expired_claims(13);
        let second = run
            .claim_next(&local.contract, agent, &facts, 13, 2, 1)
            .unwrap()
            .unwrap();
        assert_eq!(first.work, second.work);
        assert_eq!(second.attempt, first.attempt + 1);
    }

    #[test]
    fn operational_lease_cannot_extend_the_work_spec_resolution_bound() {
        let agent = UserId::new();
        let mut local = local_goal(agent, UserId::new());
        local.contract.work_specs[0].max_resolution_ticks = 3;
        let facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &facts, 10).unwrap();

        let bounded = run
            .claim_next(&local.contract, agent, &facts, 11, 300, 1)
            .unwrap()
            .unwrap();
        assert_eq!(bounded.acquired_at, 11);
        assert_eq!(bounded.expires_at, 14);
    }

    #[test]
    fn inactive_required_entry_is_rejected_before_any_work_projection_exists() {
        let agent = UserId::new();
        let mut local = local_goal(agent, UserId::new());
        local.contract.work_specs[0].activation = ContractCondition::TaskDone {
            task: ResourceId::new(),
        };
        assert_eq!(
            CollaborativeRunState::initialize(
                RunId::new(),
                &local.contract,
                &ContractConditionFacts::default(),
                0,
            ),
            Err(AgentValidationError::EntryActivationNotRequired)
        );
    }

    #[test]
    fn inactive_work_is_not_projected_and_reactivation_preserves_canonical_identity() {
        let agent = UserId::new();
        let prerequisite_task = ResourceId::new();
        let local = local_goal_with_conditional_continuation(
            agent,
            UserId::new(),
            ContractCondition::TaskDone {
                task: prerequisite_task,
            },
        );
        let empty_facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &empty_facts, 0)
                .unwrap();
        let canonical_continuation = run.work_slots[&(2, 0)];
        assert!(!run.work_items.contains_key(&canonical_continuation));
        let entry = run
            .claim_next(&local.contract, agent, &empty_facts, 1, 5, 1)
            .unwrap()
            .unwrap();
        assert!(
            run.succeed_work(&local.contract, entry.id, agent, &empty_facts, 2)
                .unwrap()
                .is_empty()
        );
        assert!(!run.work_items.contains_key(&canonical_continuation));
        assert!(
            !run.inactive_work_items
                .contains_key(&canonical_continuation)
        );

        let active_facts = ContractConditionFacts {
            completed_tasks: HashSet::from([prerequisite_task]),
            ..ContractConditionFacts::default()
        };
        run.refresh_frontier(&local.contract, &active_facts, 3)
            .unwrap();
        assert!(run.work_items.contains_key(&canonical_continuation));
        run.refresh_frontier(&local.contract, &empty_facts, 4)
            .unwrap();
        assert!(!run.work_items.contains_key(&canonical_continuation));
        assert!(
            run.inactive_work_items
                .contains_key(&canonical_continuation)
        );
        run.validate_current_projection(&local.contract, &empty_facts)
            .unwrap();
        run.refresh_frontier(&local.contract, &active_facts, 5)
            .unwrap();
        assert!(run.work_items.contains_key(&canonical_continuation));
        assert_eq!(run.work_slots[&(2, 0)], canonical_continuation);
    }

    #[test]
    fn activation_ceasing_during_claim_rejects_effect_and_resolves_by_bound() {
        let agent = UserId::new();
        let prerequisite_task = ResourceId::new();
        let local = local_goal_with_conditional_continuation(
            agent,
            UserId::new(),
            ContractCondition::TaskDone {
                task: prerequisite_task,
            },
        );
        let active_facts = ContractConditionFacts {
            completed_tasks: HashSet::from([prerequisite_task]),
            ..ContractConditionFacts::default()
        };
        let empty_facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &active_facts, 0)
                .unwrap();
        let entry = run
            .claim_next(&local.contract, agent, &active_facts, 1, 5, 1)
            .unwrap()
            .unwrap();
        let child = run
            .succeed_work(&local.contract, entry.id, agent, &active_facts, 2)
            .unwrap()[0];
        let claim = run
            .claim_next(&local.contract, agent, &active_facts, 3, 10, 1)
            .unwrap()
            .unwrap();
        assert_eq!(claim.work, child);

        run.refresh_frontier(&local.contract, &empty_facts, 4)
            .unwrap();
        assert!(!run.work_items.contains_key(&child));
        assert_eq!(
            run.succeed_work(&local.contract, claim.id, agent, &empty_facts, 4),
            Err(AgentValidationError::ExpiredOrForeignClaim)
        );
        run.refresh_frontier(&local.contract, &empty_facts, 8)
            .unwrap();
        assert_eq!(run.goal_status, GoalStatus::Failed);
    }

    #[test]
    fn internal_dependency_does_not_create_a_false_blocker() {
        let first = UserId::new();
        let second = UserId::new();
        let mut contract = local_goal(first, UserId::new()).contract;
        let prerequisite = contract.obligations[0].id;
        let dependent = Uuid::now_v7();
        contract.obligations.push(ContractObligation {
            id: dependent,
            goal: contract.goal,
            owner: second,
            activation: ContractCondition::always(),
            required_for_completion: ContractCondition::always(),
            dependency_rank: 1,
        });
        contract.dependencies.push(ContractDependency {
            obligation: dependent,
            prerequisite,
        });
        contract.work_specs.push(ContractWorkSpec {
            id: 2,
            obligation: dependent,
            owner: second,
            kind: WorkKind::AgentAction,
            activation: ContractCondition::always(),
            allowed_actions: vec![AgentActionClass::CreateTask],
            max_instances: 1,
            max_attempts: 2,
            max_resolution_ticks: 5,
            generation_rank: 0,
            is_entry: true,
            continuations: Vec::new(),
            failure_plan: FailurePlan::FailGoal {},
        });
        contract.evidence_rules.push(ContractEvidenceRule {
            id: 2,
            obligation: dependent,
            kind: EvidenceKind::DerivedFact,
            subject: ContractEvidenceSubject::Derived {},
            verification: EvidenceVerificationMode::SemanticJudgment,
        });
        let run = CollaborativeRunState::initialize(
            RunId::new(),
            &contract,
            &ContractConditionFacts::default(),
            0,
        )
        .unwrap();
        assert!(run.blockers.is_empty());
        assert!(
            run.work_items
                .values()
                .all(|work| work.status != WorkStatus::Blocked)
        );
        assert!(!run.work_items.values().any(|work| work.serves == dependent));
    }

    #[test]
    fn blocked_work_always_has_an_applicable_external_waiting_blocker() {
        let agent = UserId::new();
        let human = UserId::new();
        let mut local = local_goal(agent, UserId::new());
        let obligation = local.contract.obligations[0].id;
        local.contract.waiting_rules.push(ContractWaitingRule {
            id: 1,
            obligation,
            target: ContractWaitingTarget::PrincipalResponse { principal: human },
        });
        let facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &facts, 0).unwrap();
        let work = run.work_items.values().next().unwrap().id;
        let blocker = run
            .create_external_blocker(
                &local.contract,
                1,
                BlockScope::Work { work },
                WaitingCondition::PrincipalResponse { principal: human },
                1,
                &ExternalBlockerFacts {
                    human_principals: HashSet::from([human]),
                    ..ExternalBlockerFacts::default()
                },
            )
            .unwrap();
        assert_eq!(run.work_items[&work].status, WorkStatus::Blocked);
        assert_eq!(run.blockers[&blocker].status, BlockerStatus::Waiting);
        assert!(run.work_has_waiting_blocker(&run.work_items[&work]));
        run.validate_current_projection(&local.contract, &facts)
            .unwrap();
    }

    #[test]
    fn blocker_resolution_rejects_unobserved_principal_response() {
        let principal = UserId::new();
        let (local, mut run, work, blocker) = blocked_run(
            ContractWaitingTarget::PrincipalResponse { principal },
            WaitingCondition::PrincipalResponse { principal },
            ExternalBlockerFacts {
                human_principals: HashSet::from([principal]),
                ..ExternalBlockerFacts::default()
            },
        );
        let observation = BlockerResolutionObservation::PrincipalResponse {
            blocker,
            principal,
            comment: CommentId::new(),
            observed_at: 2,
        };
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                observation,
                &BlockerResolutionFacts::default(),
                &ContractConditionFacts::default(),
            ),
            Err(AgentValidationError::BlockerResolutionNotObserved)
        );
        assert_eq!(run.blockers[&blocker].status, BlockerStatus::Waiting);
        assert_eq!(run.work_items[&work].status, WorkStatus::Blocked);
        assert!(!run.dispatches.contains_key(&work));
    }

    #[test]
    fn blocker_resolution_rejects_a_human_task_that_is_still_open() {
        let task = ResourceId::new();
        let (local, mut run, _, blocker) = blocked_run(
            ContractWaitingTarget::TaskFromWork { work_spec_id: 1 },
            WaitingCondition::HumanTaskCompleted { task },
            ExternalBlockerFacts {
                human_assigned_tasks: HashSet::from([task]),
                ..ExternalBlockerFacts::default()
            },
        );
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                BlockerResolutionObservation::HumanTaskTerminal {
                    blocker,
                    task,
                    outcome: ObservedTerminalOutcome::Succeeded,
                    observed_at: 2,
                },
                &BlockerResolutionFacts::default(),
                &ContractConditionFacts::default(),
            ),
            Err(AgentValidationError::BlockerResolutionNotObserved)
        );
    }

    #[test]
    fn blocker_resolution_rejects_absent_or_non_matching_admin_decision() {
        let administrator = UserId::new();
        let review_task = ResourceId::new();
        let decision = GovernanceReviewId::new();
        let (local, mut run, _, blocker) = blocked_run(
            ContractWaitingTarget::AdministratorApproval {
                administrator,
                review_work_spec_id: 1,
            },
            WaitingCondition::AdministratorApproval { administrator },
            ExternalBlockerFacts {
                human_principals: HashSet::from([administrator]),
                administrators: HashSet::from([administrator]),
                ..ExternalBlockerFacts::default()
            },
        );
        let observation = BlockerResolutionObservation::AdministratorDecision {
            blocker,
            decision,
            administrator,
            review_task,
            review_work_spec_id: 1,
            outcome: ObservedTerminalOutcome::Succeeded,
            observed_at: 2,
        };
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                observation.clone(),
                &BlockerResolutionFacts::default(),
                &ContractConditionFacts::default(),
            ),
            Err(AgentValidationError::BlockerResolutionNotObserved)
        );
        let mut invalid = BlockerResolutionFacts::default();
        invalid.valid_administrator_decisions.insert(
            decision,
            ValidAdministratorDecisionFact {
                administrator,
                review_task: ResourceId::new(),
                review_work_spec_id: 1,
                outcome: ObservedTerminalOutcome::Succeeded,
                observed_at: 2,
            },
        );
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                observation,
                &invalid,
                &ContractConditionFacts::default(),
            ),
            Err(AgentValidationError::BlockerResolutionNotObserved)
        );
    }

    #[test]
    fn blocker_resolution_rejects_external_outcome_without_provenance() {
        let condition = Uuid::now_v7();
        let observation_id = Uuid::now_v7();
        let (local, mut run, _, blocker) = blocked_run(
            ContractWaitingTarget::ExternalOutcome { condition },
            WaitingCondition::ExternalOutcome { condition },
            ExternalBlockerFacts::default(),
        );
        let mut facts = BlockerResolutionFacts::default();
        facts.external_outcomes.insert(
            observation_id,
            ValidExternalOutcomeFact {
                condition,
                outcome: ObservedTerminalOutcome::Succeeded,
                provenance_hash: [0; 32],
                observed_at: 2,
            },
        );
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                BlockerResolutionObservation::ExternalOutcome {
                    blocker,
                    observation: observation_id,
                    condition,
                    outcome: ObservedTerminalOutcome::Succeeded,
                    provenance_hash: [0; 32],
                    observed_at: 2,
                },
                &facts,
                &ContractConditionFacts::default(),
            ),
            Err(AgentValidationError::BlockerResolutionNotObserved)
        );
    }

    #[test]
    fn blocker_resolution_rejects_wrong_blocker_and_condition() {
        let principal = UserId::new();
        let other_principal = UserId::new();
        let comment = CommentId::new();
        let (local, mut run, _, blocker) = blocked_run(
            ContractWaitingTarget::PrincipalResponse { principal },
            WaitingCondition::PrincipalResponse { principal },
            ExternalBlockerFacts {
                human_principals: HashSet::from([principal]),
                ..ExternalBlockerFacts::default()
            },
        );
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                BlockerResolutionObservation::PrincipalResponse {
                    blocker: BlockerId::new(),
                    principal,
                    comment,
                    observed_at: 2,
                },
                &BlockerResolutionFacts::default(),
                &ContractConditionFacts::default(),
            ),
            Err(AgentValidationError::BlockerResolutionMismatch)
        );
        let mut facts = BlockerResolutionFacts::default();
        facts
            .principal_response_comments
            .insert(comment, (other_principal, 2));
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                BlockerResolutionObservation::PrincipalResponse {
                    blocker,
                    principal: other_principal,
                    comment,
                    observed_at: 2,
                },
                &facts,
                &ContractConditionFacts::default(),
            ),
            Err(AgentValidationError::BlockerResolutionNotObserved)
        );
    }

    #[test]
    fn blocker_resolution_rejects_authentic_events_that_predate_the_wait() {
        let mut cases = Vec::new();

        let task = ResourceId::new();
        let (local, run, _, blocker) = blocked_run(
            ContractWaitingTarget::TaskFromWork { work_spec_id: 1 },
            WaitingCondition::HumanTaskCompleted { task },
            ExternalBlockerFacts {
                human_assigned_tasks: HashSet::from([task]),
                ..ExternalBlockerFacts::default()
            },
        );
        let mut facts = BlockerResolutionFacts::default();
        facts
            .terminal_human_tasks
            .insert(task, (ObservedTerminalOutcome::Succeeded, 0));
        cases.push((
            local,
            run,
            blocker,
            BlockerResolutionObservation::HumanTaskTerminal {
                blocker,
                task,
                outcome: ObservedTerminalOutcome::Succeeded,
                observed_at: 0,
            },
            facts,
        ));

        let administrator = UserId::new();
        let review_task = ResourceId::new();
        let decision = GovernanceReviewId::new();
        let (local, run, _, blocker) = blocked_run(
            ContractWaitingTarget::AdministratorApproval {
                administrator,
                review_work_spec_id: 1,
            },
            WaitingCondition::AdministratorApproval { administrator },
            ExternalBlockerFacts {
                human_principals: HashSet::from([administrator]),
                administrators: HashSet::from([administrator]),
                ..ExternalBlockerFacts::default()
            },
        );
        let mut facts = BlockerResolutionFacts::default();
        facts.valid_administrator_decisions.insert(
            decision,
            ValidAdministratorDecisionFact {
                administrator,
                review_task,
                review_work_spec_id: 1,
                outcome: ObservedTerminalOutcome::Succeeded,
                observed_at: 0,
            },
        );
        cases.push((
            local,
            run,
            blocker,
            BlockerResolutionObservation::AdministratorDecision {
                blocker,
                decision,
                administrator,
                review_task,
                review_work_spec_id: 1,
                outcome: ObservedTerminalOutcome::Succeeded,
                observed_at: 0,
            },
            facts,
        ));

        let principal = UserId::new();
        let comment = CommentId::new();
        let (local, run, _, blocker) = blocked_run(
            ContractWaitingTarget::PrincipalResponse { principal },
            WaitingCondition::PrincipalResponse { principal },
            ExternalBlockerFacts {
                human_principals: HashSet::from([principal]),
                ..ExternalBlockerFacts::default()
            },
        );
        let mut facts = BlockerResolutionFacts::default();
        facts
            .principal_response_comments
            .insert(comment, (principal, 0));
        cases.push((
            local,
            run,
            blocker,
            BlockerResolutionObservation::PrincipalResponse {
                blocker,
                principal,
                comment,
                observed_at: 0,
            },
            facts,
        ));

        let condition = Uuid::now_v7();
        let observation_id = Uuid::now_v7();
        let provenance_hash = [7; 32];
        let (local, run, _, blocker) = blocked_run(
            ContractWaitingTarget::ExternalOutcome { condition },
            WaitingCondition::ExternalOutcome { condition },
            ExternalBlockerFacts::default(),
        );
        let mut facts = BlockerResolutionFacts::default();
        facts.external_outcomes.insert(
            observation_id,
            ValidExternalOutcomeFact {
                condition,
                outcome: ObservedTerminalOutcome::Succeeded,
                provenance_hash,
                observed_at: 0,
            },
        );
        cases.push((
            local,
            run,
            blocker,
            BlockerResolutionObservation::ExternalOutcome {
                blocker,
                observation: observation_id,
                condition,
                outcome: ObservedTerminalOutcome::Succeeded,
                provenance_hash,
                observed_at: 0,
            },
            facts,
        ));

        for (local, mut run, blocker, observation, facts) in cases {
            assert_eq!(run.blockers[&blocker].created_at, 1);
            assert_eq!(
                run.resolve_blocker(
                    &local.contract,
                    blocker,
                    observation,
                    &facts,
                    &ContractConditionFacts::default(),
                ),
                Err(AgentValidationError::BlockerResolutionMismatch)
            );
            assert_eq!(run.blockers[&blocker].status, BlockerStatus::Waiting);
            assert!(run.blocker_resolutions.is_empty());
        }
    }

    #[test]
    fn validated_blocker_terminality_does_not_discharge_the_obligation() {
        let principal = UserId::new();
        let comment = CommentId::new();
        let (local, mut run, work, blocker) = blocked_run(
            ContractWaitingTarget::PrincipalResponse { principal },
            WaitingCondition::PrincipalResponse { principal },
            ExternalBlockerFacts {
                human_principals: HashSet::from([principal]),
                ..ExternalBlockerFacts::default()
            },
        );
        assert!(!run.dispatches.contains_key(&work));
        let mut facts = BlockerResolutionFacts::default();
        facts
            .principal_response_comments
            .insert(comment, (principal, 2));
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                BlockerResolutionObservation::PrincipalResponse {
                    blocker,
                    principal,
                    comment,
                    observed_at: 2,
                },
                &facts,
                &ContractConditionFacts::default(),
            ),
            Ok(BlockerStatus::Resolved)
        );
        assert_eq!(run.blockers[&blocker].status, BlockerStatus::Resolved);
        assert_eq!(run.work_items[&work].status, WorkStatus::Eligible);
        assert_eq!(run.dispatches[&work].status, DispatchStatus::Ready);
        assert_eq!(
            run.obligations[&local.contract.obligations[0].id].status,
            ObligationStatus::Active
        );
        run.try_complete(&local.contract, &ContractConditionFacts::default());
        assert_eq!(run.goal_status, GoalStatus::Active);
        assert_eq!(run.blocker_resolutions.len(), 1);
    }

    #[test]
    fn failed_blocker_status_is_derived_from_a_real_terminal_outcome() {
        let condition = Uuid::now_v7();
        let observation_id = Uuid::now_v7();
        let (local, mut run, _, blocker) = blocked_run(
            ContractWaitingTarget::ExternalOutcome { condition },
            WaitingCondition::ExternalOutcome { condition },
            ExternalBlockerFacts::default(),
        );
        let provenance_hash = [9; 32];
        let mut facts = BlockerResolutionFacts::default();
        facts.external_outcomes.insert(
            observation_id,
            ValidExternalOutcomeFact {
                condition,
                outcome: ObservedTerminalOutcome::Failed,
                provenance_hash,
                observed_at: 2,
            },
        );
        assert_eq!(
            run.resolve_blocker(
                &local.contract,
                blocker,
                BlockerResolutionObservation::ExternalOutcome {
                    blocker,
                    observation: observation_id,
                    condition,
                    outcome: ObservedTerminalOutcome::Failed,
                    provenance_hash,
                    observed_at: 2,
                },
                &facts,
                &ContractConditionFacts::default(),
            ),
            Ok(BlockerStatus::Failed)
        );
    }

    #[test]
    fn inactive_work_history_neither_bypasses_nor_prevents_completion() {
        let agent = UserId::new();
        let prerequisite_task = ResourceId::new();
        let local = local_goal_with_conditional_continuation(
            agent,
            UserId::new(),
            ContractCondition::TaskDone {
                task: prerequisite_task,
            },
        );
        let active_facts = ContractConditionFacts {
            completed_tasks: HashSet::from([prerequisite_task]),
            ..ContractConditionFacts::default()
        };
        let empty_facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &active_facts, 0)
                .unwrap();
        let entry = run
            .claim_next(&local.contract, agent, &active_facts, 1, 5, 1)
            .unwrap()
            .unwrap();
        let child = run
            .succeed_work(&local.contract, entry.id, agent, &active_facts, 2)
            .unwrap()[0];
        run.try_complete(&local.contract, &active_facts);
        assert_eq!(run.goal_status, GoalStatus::Active);

        run.refresh_frontier(&local.contract, &empty_facts, 3)
            .unwrap();
        assert!(!run.work_items.contains_key(&child));
        assert!(run.inactive_work_items.contains_key(&child));
        run.accept_evidence(
            &local.contract,
            EvidenceRecord {
                id: EvidenceId::new(),
                run: run.id,
                obligation: local.contract.obligations[0].id,
                rule_id: 1,
                kind: EvidenceKind::DerivedFact,
                subject: EvidenceSubject::Derived {},
                work: Some(entry.work),
                observed_at: 3,
                provenance_hash: [3; 32],
            },
            |_, _| false,
            |_, _| true,
        )
        .unwrap();
        run.try_complete(&local.contract, &empty_facts);
        assert_eq!(run.goal_status, GoalStatus::Completed);
    }

    #[test]
    fn collaborative_causal_graph_rejects_cycles_and_derives_a_rank() {
        let agent = UserId::new();
        let local = local_goal(agent, UserId::new());
        let facts = ContractConditionFacts::default();
        let mut run =
            CollaborativeRunState::initialize(RunId::new(), &local.contract, &facts, 0).unwrap();
        let obligation = CollaborativeCausalNode::Obligation {
            obligation: local.contract.obligations[0].id,
        };
        let work = CollaborativeCausalNode::Work {
            work: run.work_items.values().next().unwrap().id,
        };
        assert!(run.causal_rank_decreases_on_every_link());
        assert_eq!(
            run.push_causal_link(work, obligation, 1),
            Err(AgentValidationError::CausalRankDoesNotDecrease)
        );
    }

    #[test]
    fn schema_closed_contract_rejects_nested_semantic_plaintext() {
        let agent = UserId::new();
        let local = local_goal(agent, UserId::new());
        let mut value = serde_json::to_value(&local.contract).unwrap();
        value["work_specs"][0]["description"] = serde_json::json!("plaintext");
        assert!(serde_json::from_value::<GoalContract>(value).is_err());
    }

    #[test]
    fn blocker_resolution_schema_rejects_nested_semantic_plaintext() {
        let blocker = BlockerId::new();
        let observation = BlockerResolutionObservation::PrincipalResponse {
            blocker,
            principal: UserId::new(),
            comment: CommentId::new(),
            observed_at: 2,
        };
        let mut value = serde_json::to_value(BlockerResolutionRecord {
            blocker,
            observation,
            terminal_status: BlockerStatus::Resolved,
            observed_at: 2,
        })
        .unwrap();
        value["observation"]["description"] = serde_json::json!("plaintext");
        assert!(serde_json::from_value::<BlockerResolutionRecord>(value).is_err());
    }

    #[test]
    fn collaborative_run_snapshot_round_trips_canonical_slots() {
        let agent = UserId::new();
        let local = local_goal(agent, UserId::new());
        let state = CollaborativeRunState::initialize(
            RunId::new(),
            &local.contract,
            &ContractConditionFacts::default(),
            0,
        )
        .unwrap();
        let encoded = serde_json::to_vec(&state).unwrap();
        let decoded: CollaborativeRunState = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, state);
    }
}
