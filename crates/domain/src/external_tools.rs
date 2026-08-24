//! Governed external-tool runtime primitives.
//!
//! Native Sprout actions are deliberately absent. Connector execution and
//! plaintext live in a user-owned edge runtime; the server validates only the
//! immutable manifest, work coordinates, opaque commitments and signed facts.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    AgentActionClass, ClaimId, ContractWorkSecurityPolicy, ContractWorkSpec, GoalId,
    ResourceAuthority, RunId, ToolCallId, UserId, WorkItem, WorkItemId, WorkKind,
};

pub const EXTERNAL_TOOL_CATALOG_VERSION: u32 = 1;
pub const MAX_TOOL_ATTEMPTS: u16 = 16;
pub const MAX_TOOL_TIMEOUT_SECONDS: u32 = 300;
pub const MAX_RUNTIME_WITNESS_SECONDS: u32 = 300;

pub const WEB_READ_TOOL: &str = "web.read";
pub const DOCUMENT_LOCAL_READ_TOOL: &str = "document.local.read";
pub const DOCUMENT_LOCAL_EDIT_TOOL: &str = "document.local.edit";
pub const MAIL_RECEIVE_TOOL: &str = "mail.receive";
pub const MAIL_SEND_TOOL: &str = "mail.send";
pub const TELEGRAM_RECEIVE_TOOL: &str = "telegram.receive";
pub const TELEGRAM_SEND_TOOL: &str = "telegram.send";

/// Concrete, persisted evidence for the first materialization of a WorkItem.
///
/// 0033 can prove run initialization and contract-generated continuation
/// materialization from the append-only run transition history. The product
/// does not yet persist every field of Lean `HumanAgentTaskDelegation`, so any
/// Task -> Work evidence is represented explicitly and rejected rather than
/// being reclassified as run-sponsored or inherited work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConcreteWorkAuthorityEvidence {
    RunInitialization { sponsor: UserId },
    ContractContinuation { parent: WorkItemId },
    PossibleUnsupportedHumanDelegation,
    Unknown,
    Ambiguous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExactWorkAuthorityOrigin {
    RunSponsor {
        principal: UserId,
    },
    InheritedWork {
        parent: WorkItemId,
        principal: UserId,
    },
}

/// Resolve the Lean `WorkAuthorityOrigin` only from exact materialization
/// evidence. Parentage alone is never sufficient: the root must be certified
/// by the initialized transition and every child must have an exact contract
/// continuation transition. Human delegation remains deliberately fail-closed
/// until its complete concrete certificate exists.
pub fn resolve_exact_work_authority_origin(
    state: &crate::CollaborativeRunState,
    work_id: WorkItemId,
    evidence: &HashMap<WorkItemId, ConcreteWorkAuthorityEvidence>,
) -> Result<ExactWorkAuthorityOrigin, ExternalToolValidationError> {
    let target = state
        .work_items
        .get(&work_id)
        .or_else(|| state.inactive_work_items.get(&work_id))
        .ok_or(ExternalToolValidationError::UnknownWorkAuthorityOrigin)?;
    let target_parent = target.parent;
    let mut cursor = work_id;
    let mut visited = HashSet::new();
    let principal = loop {
        if !visited.insert(cursor) {
            return Err(ExternalToolValidationError::CyclicWorkAuthorityOrigin);
        }
        let work = state
            .work_items
            .get(&cursor)
            .or_else(|| state.inactive_work_items.get(&cursor))
            .ok_or(ExternalToolValidationError::UnknownWorkAuthorityOrigin)?;
        match evidence
            .get(&cursor)
            .copied()
            .unwrap_or(ConcreteWorkAuthorityEvidence::Unknown)
        {
            ConcreteWorkAuthorityEvidence::RunInitialization { sponsor } => {
                if work.parent.is_some() {
                    return Err(ExternalToolValidationError::AmbiguousWorkAuthorityOrigin);
                }
                break sponsor;
            }
            ConcreteWorkAuthorityEvidence::ContractContinuation { parent } => {
                if work.parent != Some(parent) {
                    return Err(ExternalToolValidationError::AmbiguousWorkAuthorityOrigin);
                }
                cursor = parent;
            }
            ConcreteWorkAuthorityEvidence::PossibleUnsupportedHumanDelegation => {
                return Err(ExternalToolValidationError::HumanDelegationUnsupported);
            }
            ConcreteWorkAuthorityEvidence::Unknown => {
                return Err(ExternalToolValidationError::UnknownWorkAuthorityOrigin);
            }
            ConcreteWorkAuthorityEvidence::Ambiguous => {
                return Err(ExternalToolValidationError::AmbiguousWorkAuthorityOrigin);
            }
        }
    };
    Ok(match target_parent {
        Some(parent) => ExactWorkAuthorityOrigin::InheritedWork { parent, principal },
        None => ExactWorkAuthorityOrigin::RunSponsor { principal },
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskTier {
    Tr0,
    Tr1,
    Tr2,
    Tr3,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolAvailability {
    Executable,
    ContractOnly,
    FailClosed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolOperation {
    Read,
    Edit,
    Send,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalEffectClass {
    NoSproutMutation,
    ExternalNetworkEgressBoundary,
    ExternalSideEffectBoundary,
    ExternalDisclosureUnsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutputAudiencePolicy {
    OwnerFromCanonicalInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ExternalToolManifest {
    pub id: &'static str,
    pub version: u32,
    pub adapter_protocol: &'static str,
    pub operation: ExternalToolOperation,
    pub risk: ToolRiskTier,
    pub availability: ExternalToolAvailability,
    pub effect_class: ExternalEffectClass,
    pub output_audience: ToolOutputAudiencePolicy,
    pub max_attempts: u16,
    pub max_timeout_seconds: u32,
    pub max_input_bytes: u32,
    pub max_output_bytes: u32,
    pub input_schema: &'static str,
    pub output_schema: &'static str,
}

/// Compatibility name used by the provider-neutral catalog DTO.
pub type ExternalToolCatalogEntry = ExternalToolManifest;

pub const EXTERNAL_TOOL_CATALOG: [ExternalToolManifest; 7] = [
    ExternalToolManifest {
        id: WEB_READ_TOOL,
        version: 1,
        adapter_protocol: "sprout-edge-web-read-v1",
        operation: ExternalToolOperation::Read,
        risk: ToolRiskTier::Tr2,
        availability: ExternalToolAvailability::Executable,
        effect_class: ExternalEffectClass::ExternalNetworkEgressBoundary,
        output_audience: ToolOutputAudiencePolicy::OwnerFromCanonicalInput,
        max_attempts: 4,
        max_timeout_seconds: 60,
        max_input_bytes: 16_384,
        max_output_bytes: 1_048_576,
        input_schema: r#"{"additionalProperties":false,"properties":{"url":{"type":"string"}},"required":["url"],"type":"object"}"#,
        output_schema: r#"{"additionalProperties":false,"properties":{"content_type":{"type":"string"},"final_url":{"type":"string"},"text":{"type":"string"},"title":{"type":["string","null"]}},"required":["final_url","content_type","text","title"],"type":"object"}"#,
    },
    ExternalToolManifest {
        id: DOCUMENT_LOCAL_READ_TOOL,
        version: 1,
        adapter_protocol: "sprout-edge-document-read-v1",
        operation: ExternalToolOperation::Read,
        risk: ToolRiskTier::Tr1,
        availability: ExternalToolAvailability::Executable,
        effect_class: ExternalEffectClass::NoSproutMutation,
        output_audience: ToolOutputAudiencePolicy::OwnerFromCanonicalInput,
        max_attempts: 4,
        max_timeout_seconds: 60,
        max_input_bytes: 16_384,
        max_output_bytes: 1_048_576,
        input_schema: r#"{"additionalProperties":false,"properties":{"document_capability_id":{"type":"string"}},"required":["document_capability_id"],"type":"object"}"#,
        output_schema: r#"{"additionalProperties":false,"properties":{"content":{"type":"string"},"version_hash":{"type":"string"}},"required":["content","version_hash"],"type":"object"}"#,
    },
    ExternalToolManifest {
        id: DOCUMENT_LOCAL_EDIT_TOOL,
        version: 1,
        adapter_protocol: "sprout-edge-document-edit-v1",
        operation: ExternalToolOperation::Edit,
        risk: ToolRiskTier::Tr1,
        availability: ExternalToolAvailability::ContractOnly,
        effect_class: ExternalEffectClass::ExternalSideEffectBoundary,
        output_audience: ToolOutputAudiencePolicy::OwnerFromCanonicalInput,
        max_attempts: 1,
        max_timeout_seconds: 60,
        max_input_bytes: 1_048_576,
        max_output_bytes: 16_384,
        input_schema: r#"{"additionalProperties":false,"properties":{"document_capability_id":{"type":"string"},"expected_version_hash":{"type":"string"},"replacement":{"type":"string"}},"required":["document_capability_id","expected_version_hash","replacement"],"type":"object"}"#,
        output_schema: r#"{"additionalProperties":false,"properties":{"version_hash":{"type":"string"}},"required":["version_hash"],"type":"object"}"#,
    },
    ExternalToolManifest {
        id: MAIL_RECEIVE_TOOL,
        version: 1,
        adapter_protocol: "sprout-edge-mail-receive-v1",
        operation: ExternalToolOperation::Read,
        risk: ToolRiskTier::Tr2,
        availability: ExternalToolAvailability::ContractOnly,
        effect_class: ExternalEffectClass::NoSproutMutation,
        output_audience: ToolOutputAudiencePolicy::OwnerFromCanonicalInput,
        max_attempts: 4,
        max_timeout_seconds: 60,
        max_input_bytes: 16_384,
        max_output_bytes: 1_048_576,
        input_schema: r#"{"additionalProperties":false,"type":"object"}"#,
        output_schema: r#"{"additionalProperties":false,"type":"object"}"#,
    },
    ExternalToolManifest {
        id: MAIL_SEND_TOOL,
        version: 1,
        adapter_protocol: "sprout-edge-mail-send-v1",
        operation: ExternalToolOperation::Send,
        risk: ToolRiskTier::Tr3,
        availability: ExternalToolAvailability::FailClosed,
        effect_class: ExternalEffectClass::ExternalDisclosureUnsupported,
        output_audience: ToolOutputAudiencePolicy::OwnerFromCanonicalInput,
        max_attempts: 1,
        max_timeout_seconds: 60,
        max_input_bytes: 16_384,
        max_output_bytes: 16_384,
        input_schema: r#"{"additionalProperties":false,"type":"object"}"#,
        output_schema: r#"{"additionalProperties":false,"type":"object"}"#,
    },
    ExternalToolManifest {
        id: TELEGRAM_RECEIVE_TOOL,
        version: 1,
        adapter_protocol: "sprout-edge-telegram-receive-v1",
        operation: ExternalToolOperation::Read,
        risk: ToolRiskTier::Tr2,
        availability: ExternalToolAvailability::ContractOnly,
        effect_class: ExternalEffectClass::NoSproutMutation,
        output_audience: ToolOutputAudiencePolicy::OwnerFromCanonicalInput,
        max_attempts: 4,
        max_timeout_seconds: 60,
        max_input_bytes: 16_384,
        max_output_bytes: 1_048_576,
        input_schema: r#"{"additionalProperties":false,"type":"object"}"#,
        output_schema: r#"{"additionalProperties":false,"type":"object"}"#,
    },
    ExternalToolManifest {
        id: TELEGRAM_SEND_TOOL,
        version: 1,
        adapter_protocol: "sprout-edge-telegram-send-v1",
        operation: ExternalToolOperation::Send,
        risk: ToolRiskTier::Tr3,
        availability: ExternalToolAvailability::FailClosed,
        effect_class: ExternalEffectClass::ExternalDisclosureUnsupported,
        output_audience: ToolOutputAudiencePolicy::OwnerFromCanonicalInput,
        max_attempts: 1,
        max_timeout_seconds: 60,
        max_input_bytes: 16_384,
        max_output_bytes: 16_384,
        input_schema: r#"{"additionalProperties":false,"type":"object"}"#,
        output_schema: r#"{"additionalProperties":false,"type":"object"}"#,
    },
];

#[must_use]
pub fn external_tool_catalog_entry(
    tool: &str,
    version: u32,
) -> Option<&'static ExternalToolManifest> {
    EXTERNAL_TOOL_CATALOG
        .iter()
        .find(|entry| entry.id == tool && entry.version == version)
}

/// Detects native-resource aliases independently from the closed catalog. The
/// normalization also catches separator/case variants such as `TaskList` or
/// `workspace-task-create`; unknown identities remain rejected by catalog lookup.
#[must_use]
pub fn is_native_sprout_tool_alias(tool: &str) -> bool {
    let normalized: String = tool
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    ["tasklist", "task", "topic", "info", "comment"]
        .iter()
        .any(|native| normalized.contains(native))
}

/// Initial read-only adapters do not mutate a Sprout resource. External local
/// edits and external disclosure are not claimed as closed formal semantics.
#[must_use]
pub fn initial_tool_required_effects(tool: &str, version: u32) -> Option<Vec<ResourceAuthority>> {
    let manifest = external_tool_catalog_entry(tool, version)?;
    matches!(
        manifest.effect_class,
        ExternalEffectClass::NoSproutMutation | ExternalEffectClass::ExternalNetworkEgressBoundary
    )
    .then(Vec::new)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolCallStatus {
    Pending,
    Succeeded,
    Failed,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolAuditKind {
    Requested,
    RetryStarted,
    Completed,
    Failed,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolTerminalOrigin {
    SignedEdgeObservation,
    ServerTimeout,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalExternalToolInput {
    pub owner: UserId,
    pub tool: String,
    pub tool_version: u32,
    pub encrypted_payload_commitment: [u8; 32],
    pub structured_input_commitment: [u8; 32],
}

impl CanonicalExternalToolInput {
    #[must_use]
    pub fn output_readable_by(&self) -> Vec<UserId> {
        vec![self.owner]
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolRuntimeCapabilityWitness {
    pub owner: UserId,
    pub tool: String,
    pub tool_version: u32,
    pub execution_profile_commitment: [u8; 32],
    pub manifest_commitment: [u8; 32],
    pub issued_at: u64,
    pub expires_at: u64,
}

impl ExternalToolRuntimeCapabilityWitness {
    pub fn validate_at(&self, now: u64) -> Result<(), ExternalToolValidationError> {
        if external_tool_catalog_entry(&self.tool, self.tool_version)
            .is_none_or(|manifest| manifest.availability != ExternalToolAvailability::Executable)
            || self.issued_at > now
            || now >= self.expires_at
            || self.expires_at.saturating_sub(self.issued_at)
                > u64::from(MAX_RUNTIME_WITNESS_SECONDS)
        {
            return Err(ExternalToolValidationError::RuntimeUnavailable);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolDispatchRecord {
    pub call: ToolCallId,
    pub run: RunId,
    pub goal: GoalId,
    pub work: WorkItemId,
    pub claim: ClaimId,
    pub attempt: u16,
    pub owner: UserId,
    pub tool: String,
    pub tool_version: u32,
    pub canonical_input_commitment: [u8; 32],
    pub execution_profile_commitment: [u8; 32],
    pub requested_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolTerminalObservation {
    pub call: ToolCallId,
    pub attempt: u16,
    pub owner: UserId,
    pub terminal_origin: ExternalToolTerminalOrigin,
    pub observed_at: u64,
    pub status: ExternalToolCallStatus,
    pub wire_request_commitment: Option<[u8; 32]>,
    pub execution_profile_commitment: [u8; 32],
    pub encrypted_output_payload_commitment: Option<[u8; 32]>,
    pub canonical_output_commitment: Option<[u8; 32]>,
    pub failure_code: Option<String>,
    pub output_readable_by: Vec<UserId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolCallRecord {
    pub id: ToolCallId,
    pub run: RunId,
    pub goal: GoalId,
    pub work: WorkItemId,
    pub claim: ClaimId,
    pub work_attempt: u16,
    pub owner: UserId,
    pub tool: String,
    pub tool_version: u32,
    pub encrypted_input_payload_commitment: [u8; 32],
    pub canonical_input_commitment: [u8; 32],
    pub attempt: u16,
    pub max_attempts: u16,
    pub timeout_seconds: u32,
    /// Server-authoritative semantic tick at which invoke/retry made the call
    /// pending. One concrete tick is one UTC second in 0033.
    pub requested_at: u64,
    pub tool_deadline_at: u64,
    pub status: ExternalToolCallStatus,
    pub canonical_output_commitment: Option<[u8; 32]>,
    pub failure_code: Option<String>,
}

impl ExternalToolCallRecord {
    pub fn validate(&self) -> Result<(), ExternalToolValidationError> {
        if is_native_sprout_tool_alias(&self.tool) {
            return Err(ExternalToolValidationError::NativeSurfaceForbidden);
        }
        let manifest = external_tool_catalog_entry(&self.tool, self.tool_version)
            .ok_or(ExternalToolValidationError::UnknownTool)?;
        if manifest.availability != ExternalToolAvailability::Executable {
            return Err(ExternalToolValidationError::ToolFailClosed);
        }
        if self.attempt == 0
            || self.work_attempt != self.attempt
            || self.max_attempts == 0
            || self.attempt > self.max_attempts
            || self.max_attempts > manifest.max_attempts
            || self.timeout_seconds == 0
            || self.timeout_seconds > manifest.max_timeout_seconds
            || self.tool_deadline_at
                != self
                    .requested_at
                    .saturating_add(u64::from(self.timeout_seconds))
        {
            return Err(ExternalToolValidationError::InvalidAttemptBound);
        }
        let terminal_shape = match self.status {
            ExternalToolCallStatus::Pending => {
                self.canonical_output_commitment.is_none() && self.failure_code.is_none()
            }
            ExternalToolCallStatus::Succeeded => {
                self.canonical_output_commitment.is_some() && self.failure_code.is_none()
            }
            ExternalToolCallStatus::Failed => {
                self.canonical_output_commitment.is_none()
                    && self
                        .failure_code
                        .as_ref()
                        .is_some_and(|code| !code.is_empty())
            }
            ExternalToolCallStatus::TimedOut => {
                self.canonical_output_commitment.is_none() && self.failure_code.is_some()
            }
        };
        terminal_shape
            .then_some(())
            .ok_or(ExternalToolValidationError::InvalidTerminalShape)
    }
}

/// Receipt of a terminal result depends on the pending call and its immutable
/// dispatch, never on current readiness. New invoke/retry perform that check.
pub fn validate_external_tool_terminal_observation(
    pending: &ExternalToolCallRecord,
    dispatch: &ExternalToolDispatchRecord,
    observation: &ExternalToolTerminalObservation,
) -> Result<(), ExternalToolValidationError> {
    pending.validate()?;
    if pending.status != ExternalToolCallStatus::Pending
        || dispatch.call != pending.id
        || dispatch.run != pending.run
        || dispatch.goal != pending.goal
        || dispatch.work != pending.work
        || dispatch.claim != pending.claim
        || dispatch.attempt != pending.attempt
        || dispatch.owner != pending.owner
        || dispatch.tool != pending.tool
        || dispatch.tool_version != pending.tool_version
        || dispatch.canonical_input_commitment != pending.canonical_input_commitment
        || observation.call != pending.id
        || observation.attempt != pending.attempt
        || observation.owner != pending.owner
        || observation.observed_at < dispatch.requested_at
        || observation.execution_profile_commitment != dispatch.execution_profile_commitment
        || observation.output_readable_by != [pending.owner]
    {
        return Err(ExternalToolValidationError::TerminalDispatchMismatch);
    }
    let shape = match (observation.terminal_origin, observation.status) {
        (ExternalToolTerminalOrigin::SignedEdgeObservation, ExternalToolCallStatus::Succeeded) => {
            observation.wire_request_commitment.is_some()
                && observation.encrypted_output_payload_commitment.is_some()
                && observation.canonical_output_commitment.is_some()
                && observation.failure_code.is_none()
        }
        (ExternalToolTerminalOrigin::SignedEdgeObservation, ExternalToolCallStatus::Failed) => {
            observation.wire_request_commitment.is_some()
                && observation.encrypted_output_payload_commitment.is_none()
                && observation.canonical_output_commitment.is_none()
                && observation
                    .failure_code
                    .as_ref()
                    .is_some_and(|code| !code.is_empty())
        }
        (ExternalToolTerminalOrigin::SignedEdgeObservation, ExternalToolCallStatus::TimedOut) => {
            observation.wire_request_commitment.is_some()
                && observation.encrypted_output_payload_commitment.is_none()
                && observation.canonical_output_commitment.is_none()
                && observation
                    .failure_code
                    .as_ref()
                    .is_some_and(|code| !code.is_empty())
        }
        (ExternalToolTerminalOrigin::ServerTimeout, ExternalToolCallStatus::TimedOut) => {
            observation.encrypted_output_payload_commitment.is_none()
                && observation.canonical_output_commitment.is_none()
                && observation.failure_code.as_deref() == Some("server_timeout")
        }
        _ => false,
    };
    shape
        .then_some(())
        .ok_or(ExternalToolValidationError::InvalidTerminalShape)
}

#[derive(Clone, Copy)]
pub struct ExternalToolAuthorization<'a> {
    pub call: &'a ExternalToolCallRecord,
    pub work: &'a WorkItem,
    pub work_spec: &'a ContractWorkSpec,
    pub policy: &'a ContractWorkSecurityPolicy,
    pub run_tool_ceiling: &'a [String],
    pub work_tool_ceiling: &'a [String],
    pub current_authority_tool_permission: bool,
    pub current_actor_tool_permission: bool,
    pub claimed_by_owner: bool,
    pub exact_runtime_capability: bool,
    pub required_effects: &'a [ResourceAuthority],
    pub expected_required_effects: &'a [ResourceAuthority],
    pub required_effects_currently_authorized: bool,
    pub output_readable_by: &'a [UserId],
    pub owner_can_read_all_sources: bool,
    pub claim_acquired_at: u64,
    pub claim_expires_at: u64,
}

pub fn validate_external_tool_authorization(
    authorization: &ExternalToolAuthorization<'_>,
) -> Result<(), ExternalToolValidationError> {
    let call = authorization.call;
    call.validate()?;
    let expected_action = if call.attempt == 1 {
        AgentActionClass::InvokeTool
    } else {
        AgentActionClass::RetryTool
    };
    if authorization.work.id != call.work
        || authorization.work.run != call.run
        || authorization.work.goal != call.goal
        || authorization.work.owner != call.owner
        || authorization.work.attempt != call.attempt
        || !matches!(
            authorization.work.kind,
            WorkKind::ToolInvocation | WorkKind::ToolRetry
        )
        || authorization.work_spec.id != authorization.work.work_spec_id
        || authorization.work_spec.owner != call.owner
        || authorization.work_spec.obligation != authorization.work.serves
        // ToolCall attempts are one-based and inclusive; WorkAttempt uses the
        // Lean-exclusive `attempt < WorkSpec.maxAttempts` bound.
        || call.max_attempts >= authorization.work_spec.max_attempts
        || !authorization
            .work_spec
            .allowed_actions
            .contains(&expected_action)
        || authorization.policy.work_spec_id != authorization.work_spec.id
        || !authorization.policy.allowed_tools.contains(&call.tool)
        || !authorization.run_tool_ceiling.contains(&call.tool)
        || !authorization.work_tool_ceiling.contains(&call.tool)
        || !authorization.current_authority_tool_permission
        || !authorization.current_actor_tool_permission
        || !authorization.claimed_by_owner
        || !authorization.exact_runtime_capability
        || authorization.claim_acquired_at > call.requested_at
        || call.requested_at >= authorization.claim_expires_at
    {
        return Err(ExternalToolValidationError::AuthorityMismatch);
    }
    if authorization.required_effects != authorization.expected_required_effects
        || !authorization.required_effects_currently_authorized
    {
        return Err(ExternalToolValidationError::RequiredEffectsMismatch);
    }
    if authorization.output_readable_by != [call.owner] || !authorization.owner_can_read_all_sources
    {
        return Err(ExternalToolValidationError::OutputAudienceMismatch);
    }
    Ok(())
}

/// The server may close a pending call at its call-level deadline even if no
/// edge dispatch ever existed. This is intentionally separate from signed
/// terminal observation validation and never fabricates dispatch provenance.
pub fn validate_external_tool_server_timeout_without_dispatch(
    pending: &ExternalToolCallRecord,
    observation: &ExternalToolTerminalObservation,
) -> Result<(), ExternalToolValidationError> {
    pending.validate()?;
    if pending.status != ExternalToolCallStatus::Pending
        || observation.call != pending.id
        || observation.attempt != pending.attempt
        || observation.owner != pending.owner
        || observation.terminal_origin != ExternalToolTerminalOrigin::ServerTimeout
        || observation.status != ExternalToolCallStatus::TimedOut
        || observation.observed_at < pending.tool_deadline_at
        || observation.wire_request_commitment.is_some()
        || observation.encrypted_output_payload_commitment.is_some()
        || observation.canonical_output_commitment.is_some()
        || observation.failure_code.as_deref() != Some("server_timeout")
        || observation.output_readable_by != [pending.owner]
    {
        return Err(ExternalToolValidationError::InvalidTerminalShape);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExternalToolValidationError {
    #[error("native Sprout actions cannot be registered as external tools")]
    NativeSurfaceForbidden,
    #[error("unknown external tool identity or version")]
    UnknownTool,
    #[error("tool is contract-only or fail-closed")]
    ToolFailClosed,
    #[error("signed runtime capability is missing, stale or invalid")]
    RuntimeUnavailable,
    #[error("invalid tool attempt, WorkAttempt coordinate or timeout bound")]
    InvalidAttemptBound,
    #[error("invalid tool terminal shape")]
    InvalidTerminalShape,
    #[error("tool retry is not eligible")]
    RetryNotAllowed,
    #[error("work, claim, policy, ceiling, readiness or current permission mismatch")]
    AuthorityMismatch,
    #[error("tool required effects are incomplete or unauthorized")]
    RequiredEffectsMismatch,
    #[error("tool output audience is not the exact safe audience")]
    OutputAudienceMismatch,
    #[error("terminal observation does not match the exact pending call dispatch")]
    TerminalDispatchMismatch,
    #[error("work authority origin has no exact concrete provenance")]
    UnknownWorkAuthorityOrigin,
    #[error("work authority origin is ambiguous")]
    AmbiguousWorkAuthorityOrigin,
    #[error("work authority ancestry is cyclic")]
    CyclicWorkAuthorityOrigin,
    #[error("human task delegation is not concretely certified in checkpoint 0033")]
    HumanDelegationUnsupported,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FailurePlan, WorkStatus};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn call() -> ExternalToolCallRecord {
        ExternalToolCallRecord {
            id: ToolCallId::new(),
            run: RunId::new(),
            goal: GoalId::new(),
            work: WorkItemId::new(),
            claim: ClaimId::new(),
            work_attempt: 1,
            owner: UserId::new(),
            tool: WEB_READ_TOOL.to_owned(),
            tool_version: 1,
            encrypted_input_payload_commitment: [6; 32],
            canonical_input_commitment: [7; 32],
            attempt: 1,
            max_attempts: 3,
            timeout_seconds: 30,
            requested_at: 10,
            tool_deadline_at: 40,
            status: ExternalToolCallStatus::Pending,
            canonical_output_commitment: None,
            failure_code: None,
        }
    }

    fn dispatch(call: &ExternalToolCallRecord) -> ExternalToolDispatchRecord {
        ExternalToolDispatchRecord {
            call: call.id,
            run: call.run,
            goal: call.goal,
            work: call.work,
            claim: call.claim,
            attempt: call.attempt,
            owner: call.owner,
            tool: call.tool.clone(),
            tool_version: call.tool_version,
            canonical_input_commitment: call.canonical_input_commitment,
            execution_profile_commitment: [8; 32],
            requested_at: 10,
        }
    }

    fn authority_work(
        id: WorkItemId,
        run: RunId,
        goal: GoalId,
        owner: UserId,
        parent: Option<WorkItemId>,
    ) -> WorkItem {
        WorkItem {
            id,
            run,
            goal,
            owner,
            serves: Uuid::now_v7(),
            work_spec_id: 9,
            slot: 0,
            kind: WorkKind::ToolInvocation,
            parent,
            source_comment: None,
            status: WorkStatus::Claimed,
            attempt: 1,
            created_at: 1,
        }
    }

    fn authority_state(
        work_items: impl IntoIterator<Item = WorkItem>,
    ) -> crate::CollaborativeRunState {
        let work_items: HashMap<_, _> =
            work_items.into_iter().map(|work| (work.id, work)).collect();
        let first = work_items.values().next().expect("authority state work");
        crate::CollaborativeRunState {
            id: first.run,
            goal: first.goal,
            scope: crate::ResourceId::new(),
            goal_status: crate::GoalStatus::Active,
            run_status: crate::CollaborativeRunStatus::Running,
            participants: Default::default(),
            obligations: Default::default(),
            work_slots: Default::default(),
            work_items,
            inactive_work_items: Default::default(),
            work_projection_history: Vec::new(),
            suspended_claim_resolutions: Default::default(),
            dispatches: Default::default(),
            claims: Default::default(),
            blockers: Default::default(),
            blocker_resolutions: Vec::new(),
            evidence: Vec::new(),
            causal_links: Vec::new(),
        }
    }

    #[test]
    fn catalog_contains_no_native_sprout_surface_or_synonym() {
        assert!(
            EXTERNAL_TOOL_CATALOG
                .iter()
                .all(|entry| !is_native_sprout_tool_alias(entry.id))
        );
        for forbidden in [
            "workspace.task.create",
            "WORKSPACE-TASK-CREATE",
            "sproutTaskListMutation",
            "topic_manager",
            "workspace.info.edit",
            "comment.read",
            "discussion-comment-post",
        ] {
            assert!(is_native_sprout_tool_alias(forbidden));
            assert!(external_tool_catalog_entry(forbidden, 1).is_none());
        }
    }

    #[test]
    fn external_send_and_external_edit_stay_fail_closed_or_contract_only() {
        for tool in [MAIL_SEND_TOOL, TELEGRAM_SEND_TOOL, DOCUMENT_LOCAL_EDIT_TOOL] {
            let mut candidate = call();
            candidate.tool = tool.to_owned();
            assert_eq!(
                candidate.validate(),
                Err(ExternalToolValidationError::ToolFailClosed)
            );
        }
    }

    #[test]
    fn runtime_witness_is_short_lived_and_manifest_scoped() {
        let witness = ExternalToolRuntimeCapabilityWitness {
            owner: UserId::new(),
            tool: WEB_READ_TOOL.into(),
            tool_version: 1,
            execution_profile_commitment: [1; 32],
            manifest_commitment: [2; 32],
            issued_at: 10,
            expires_at: 20,
        };
        assert_eq!(witness.validate_at(19), Ok(()));
        assert_eq!(
            witness.validate_at(20),
            Err(ExternalToolValidationError::RuntimeUnavailable)
        );
        let wrong_version = ExternalToolRuntimeCapabilityWitness {
            tool_version: 2,
            ..witness
        };
        assert_eq!(
            wrong_version.validate_at(19),
            Err(ExternalToolValidationError::RuntimeUnavailable)
        );
    }

    #[test]
    fn exact_run_sponsor_and_inherited_work_origins_are_proven() {
        let run = RunId::new();
        let goal = GoalId::new();
        let owner = UserId::new();
        let sponsor = UserId::new();
        let root_id = WorkItemId::new();
        let child_id = WorkItemId::new();
        let state = authority_state([
            authority_work(root_id, run, goal, owner, None),
            authority_work(child_id, run, goal, owner, Some(root_id)),
        ]);
        let evidence = HashMap::from([
            (
                root_id,
                ConcreteWorkAuthorityEvidence::RunInitialization { sponsor },
            ),
            (
                child_id,
                ConcreteWorkAuthorityEvidence::ContractContinuation { parent: root_id },
            ),
        ]);
        assert_eq!(
            resolve_exact_work_authority_origin(&state, root_id, &evidence),
            Ok(ExactWorkAuthorityOrigin::RunSponsor { principal: sponsor })
        );
        assert_eq!(
            resolve_exact_work_authority_origin(&state, child_id, &evidence),
            Ok(ExactWorkAuthorityOrigin::InheritedWork {
                parent: root_id,
                principal: sponsor,
            })
        );
    }

    #[test]
    fn missing_fake_cycle_human_and_ambiguous_origins_fail_closed() {
        let run = RunId::new();
        let goal = GoalId::new();
        let owner = UserId::new();
        let sponsor = UserId::new();
        let root_id = WorkItemId::new();
        let child_id = WorkItemId::new();
        let missing_parent = WorkItemId::new();

        let ordinary = authority_state([
            authority_work(root_id, run, goal, owner, None),
            authority_work(child_id, run, goal, owner, Some(root_id)),
        ]);
        assert_eq!(
            resolve_exact_work_authority_origin(&ordinary, root_id, &HashMap::new()),
            Err(ExternalToolValidationError::UnknownWorkAuthorityOrigin)
        );
        let fake_parent = authority_state([authority_work(
            child_id,
            run,
            goal,
            owner,
            Some(missing_parent),
        )]);
        let fake_evidence = HashMap::from([(
            child_id,
            ConcreteWorkAuthorityEvidence::ContractContinuation {
                parent: missing_parent,
            },
        )]);
        assert_eq!(
            resolve_exact_work_authority_origin(&fake_parent, child_id, &fake_evidence),
            Err(ExternalToolValidationError::UnknownWorkAuthorityOrigin)
        );

        let cycle_a = WorkItemId::new();
        let cycle_b = WorkItemId::new();
        let cycle_state = authority_state([
            authority_work(cycle_a, run, goal, owner, Some(cycle_b)),
            authority_work(cycle_b, run, goal, owner, Some(cycle_a)),
        ]);
        let cycle_evidence = HashMap::from([
            (
                cycle_a,
                ConcreteWorkAuthorityEvidence::ContractContinuation { parent: cycle_b },
            ),
            (
                cycle_b,
                ConcreteWorkAuthorityEvidence::ContractContinuation { parent: cycle_a },
            ),
        ]);
        assert_eq!(
            resolve_exact_work_authority_origin(&cycle_state, cycle_a, &cycle_evidence),
            Err(ExternalToolValidationError::CyclicWorkAuthorityOrigin)
        );

        let human = HashMap::from([(
            root_id,
            ConcreteWorkAuthorityEvidence::PossibleUnsupportedHumanDelegation,
        )]);
        assert_eq!(
            resolve_exact_work_authority_origin(&ordinary, root_id, &human),
            Err(ExternalToolValidationError::HumanDelegationUnsupported)
        );
        let ambiguous = HashMap::from([(root_id, ConcreteWorkAuthorityEvidence::Ambiguous)]);
        assert_eq!(
            resolve_exact_work_authority_origin(&ordinary, root_id, &ambiguous),
            Err(ExternalToolValidationError::AmbiguousWorkAuthorityOrigin)
        );

        let wrong_root = HashMap::from([(
            child_id,
            ConcreteWorkAuthorityEvidence::RunInitialization { sponsor },
        )]);
        assert_eq!(
            resolve_exact_work_authority_origin(&ordinary, child_id, &wrong_root),
            Err(ExternalToolValidationError::AmbiguousWorkAuthorityOrigin)
        );
    }

    #[test]
    fn terminal_of_pending_call_does_not_recheck_current_readiness() {
        let pending = call();
        let observation = ExternalToolTerminalObservation {
            call: pending.id,
            attempt: pending.attempt,
            owner: pending.owner,
            terminal_origin: ExternalToolTerminalOrigin::SignedEdgeObservation,
            observed_at: 100,
            status: ExternalToolCallStatus::Succeeded,
            wire_request_commitment: Some([9; 32]),
            execution_profile_commitment: [8; 32],
            encrypted_output_payload_commitment: Some([10; 32]),
            canonical_output_commitment: Some([11; 32]),
            failure_code: None,
            output_readable_by: vec![pending.owner],
        };
        assert_eq!(
            validate_external_tool_terminal_observation(
                &pending,
                &dispatch(&pending),
                &observation
            ),
            Ok(())
        );
    }

    #[test]
    fn server_timeout_is_unsigned_and_output_free() {
        let pending = call();
        let observation = ExternalToolTerminalObservation {
            call: pending.id,
            attempt: pending.attempt,
            owner: pending.owner,
            terminal_origin: ExternalToolTerminalOrigin::ServerTimeout,
            observed_at: 100,
            status: ExternalToolCallStatus::TimedOut,
            wire_request_commitment: None,
            execution_profile_commitment: [8; 32],
            encrypted_output_payload_commitment: None,
            canonical_output_commitment: None,
            failure_code: Some("server_timeout".into()),
            output_readable_by: vec![pending.owner],
        };
        assert_eq!(
            validate_external_tool_terminal_observation(
                &pending,
                &dispatch(&pending),
                &observation
            ),
            Ok(())
        );
    }

    #[test]
    fn exact_work_policy_ceiling_effects_readiness_and_audience_are_conjunctive() {
        let call = call();
        let work = WorkItem {
            id: call.work,
            run: call.run,
            goal: call.goal,
            owner: call.owner,
            serves: Uuid::now_v7(),
            work_spec_id: 9,
            slot: 0,
            kind: WorkKind::ToolInvocation,
            parent: None,
            source_comment: None,
            status: WorkStatus::Claimed,
            attempt: 1,
            created_at: 1,
        };
        let work_spec = ContractWorkSpec {
            id: 9,
            obligation: work.serves,
            owner: call.owner,
            kind: WorkKind::ToolInvocation,
            activation: crate::ContractCondition::always(),
            allowed_actions: vec![AgentActionClass::InvokeTool],
            max_instances: 1,
            max_attempts: 4,
            max_resolution_ticks: 4,
            generation_rank: 0,
            is_entry: true,
            continuations: Vec::new(),
            failure_plan: FailurePlan::FailGoal {},
        };
        let policy = ContractWorkSecurityPolicy {
            work_spec_id: 9,
            allowed_operations: Vec::new(),
            allowed_tools: vec![WEB_READ_TOOL.to_owned()],
        };
        let ceiling = vec![WEB_READ_TOOL.to_owned()];
        let authorization = ExternalToolAuthorization {
            call: &call,
            work: &work,
            work_spec: &work_spec,
            policy: &policy,
            run_tool_ceiling: &ceiling,
            work_tool_ceiling: &ceiling,
            current_authority_tool_permission: true,
            current_actor_tool_permission: true,
            claimed_by_owner: true,
            exact_runtime_capability: true,
            required_effects: &[],
            expected_required_effects: &[],
            required_effects_currently_authorized: true,
            output_readable_by: &[call.owner],
            owner_can_read_all_sources: true,
            claim_acquired_at: 10,
            claim_expires_at: 20,
        };
        assert_eq!(validate_external_tool_authorization(&authorization), Ok(()));
        let denied = ExternalToolAuthorization {
            exact_runtime_capability: false,
            ..authorization
        };
        assert_eq!(
            validate_external_tool_authorization(&denied),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        let authority_principal_revoked = ExternalToolAuthorization {
            current_authority_tool_permission: false,
            ..authorization
        };
        assert_eq!(
            validate_external_tool_authorization(&authority_principal_revoked),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        let actor_revoked = ExternalToolAuthorization {
            current_actor_tool_permission: false,
            ..authorization
        };
        assert_eq!(
            validate_external_tool_authorization(&actor_revoked),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        let no_run_ceiling: Vec<String> = Vec::new();
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                run_tool_ceiling: &no_run_ceiling,
                ..authorization
            }),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        let no_work_ceiling: Vec<String> = Vec::new();
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                work_tool_ceiling: &no_work_ceiling,
                ..authorization
            }),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        let policy_without_tool = ContractWorkSecurityPolicy {
            allowed_tools: Vec::new(),
            ..policy.clone()
        };
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                policy: &policy_without_tool,
                ..authorization
            }),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                claimed_by_owner: false,
                ..authorization
            }),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                required_effects_currently_authorized: false,
                ..authorization
            }),
            Err(ExternalToolValidationError::RequiredEffectsMismatch)
        );
        let unexpected_effects = vec![ResourceAuthority {
            resource_id: crate::ResourceId::new(),
            operation: crate::ResourceOperation::Read,
        }];
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                required_effects: &unexpected_effects,
                ..authorization
            }),
            Err(ExternalToolValidationError::RequiredEffectsMismatch)
        );
        let wrong_audience = [UserId::new()];
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                output_readable_by: &wrong_audience,
                ..authorization
            }),
            Err(ExternalToolValidationError::OutputAudienceMismatch)
        );
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                owner_can_read_all_sources: false,
                ..authorization
            }),
            Err(ExternalToolValidationError::OutputAudienceMismatch)
        );
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                claim_acquired_at: call.requested_at + 1,
                ..authorization
            }),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
        assert_eq!(
            validate_external_tool_authorization(&ExternalToolAuthorization {
                claim_expires_at: call.requested_at,
                ..authorization
            }),
            Err(ExternalToolValidationError::AuthorityMismatch)
        );
    }
}
