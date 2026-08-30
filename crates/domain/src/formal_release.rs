//! Closed domain certificates for the R5.40/R5.41 release projection.
//!
//! Persistence recomputes these inventories from authoritative ledgers.  The
//! types here prevent a partial set of conjuncts from being called a formal
//! release certificate.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{ClaimId, ExternalToolTraceId, GoalId, RunId, WorkItemId};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum R540ReleaseEventKind {
    WorkAttempt,
    WorkOutcome,
    BlockerResolution,
    CausalLink,
    ToolEvent,
    Evidence,
    Disclosure,
    ModelInvocation,
    Interrogation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderedR540ReleaseEvent {
    pub ordinal: u64,
    pub kind: R540ReleaseEventKind,
    pub semantic_tick: u64,
    pub event_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactR540ReleaseTraceCertificate {
    pub trace: ExternalToolTraceId,
    pub run: RunId,
    pub goal: GoalId,
    pub start_tick: u64,
    pub end_tick: u64,
    pub ordered: Vec<OrderedR540ReleaseEvent>,
    pub work_attempts: Vec<OrderedR540ReleaseEvent>,
    pub work_outcomes: Vec<OrderedR540ReleaseEvent>,
    pub blocker_resolutions: Vec<OrderedR540ReleaseEvent>,
    pub causal_links: Vec<OrderedR540ReleaseEvent>,
    pub tool_events: Vec<OrderedR540ReleaseEvent>,
    pub evidence: Vec<OrderedR540ReleaseEvent>,
    pub disclosures: Vec<OrderedR540ReleaseEvent>,
    pub model_invocations: Vec<OrderedR540ReleaseEvent>,
    pub interrogations: Vec<OrderedR540ReleaseEvent>,
}

impl ExactR540ReleaseTraceCertificate {
    pub fn validate(&self) -> Result<(), FormalReleaseValidationError> {
        if self.trace.0 == 0 || self.start_tick > self.end_tick || self.work_attempts.is_empty() {
            return Err(FormalReleaseValidationError::InvalidR540Trace);
        }
        let mut seen = HashSet::new();
        for (index, event) in self.ordered.iter().enumerate() {
            if event.ordinal != u64::try_from(index + 1).unwrap_or(u64::MAX)
                || event.semantic_tick < self.start_tick
                || event.semantic_tick > self.end_tick
                || !seen.insert((event.kind, event.event_hash))
            {
                return Err(FormalReleaseValidationError::InvalidR540Trace);
            }
        }
        let projected = |kind| {
            self.ordered
                .iter()
                .filter(|event| event.kind == kind)
                .cloned()
                .collect::<Vec<_>>()
        };
        if self.work_attempts != projected(R540ReleaseEventKind::WorkAttempt)
            || self.work_outcomes != projected(R540ReleaseEventKind::WorkOutcome)
            || self.blocker_resolutions != projected(R540ReleaseEventKind::BlockerResolution)
            || self.causal_links != projected(R540ReleaseEventKind::CausalLink)
            || self.tool_events != projected(R540ReleaseEventKind::ToolEvent)
            || self.evidence != projected(R540ReleaseEventKind::Evidence)
            || self.disclosures != projected(R540ReleaseEventKind::Disclosure)
            || self.model_invocations != projected(R540ReleaseEventKind::ModelInvocation)
            || self.interrogations != projected(R540ReleaseEventKind::Interrogation)
        {
            return Err(FormalReleaseValidationError::InvalidR540Trace);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormalReleaseIdentity {
    pub trace: ExternalToolTraceId,
    pub run: RunId,
    pub goal: GoalId,
    pub start_tick: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FormalReleaseRootField {
    RunGoalExact,
    TraceStartExact,
    GovernedRunExact,
    SecureKernel,
    GovernanceKernel,
    ConcreteTrace,
    TraceFeatureGates,
    CompilerActionExact,
    SecurityPoliciesExact,
    GovernanceOperational,
    LocalRevisionTraceBound,
    CreationTraceBound,
    ResponsibilityTraceBound,
    GlobalTraceBound,
    ProxyTraceBound,
    CrossOwnerTraceBound,
    Comments,
    Proxy,
    GlobalInventoryExact,
    Global,
    CrossOwner,
    Interrogation,
    Model,
    TaskOperational,
    TaskIntentTraceBound,
    TaskProvenanceTraceBound,
    OperationalHistory,
    OperationalClosure,
}

/// Handle returned only after a field-specific typed source has been
/// reconstructed. Its fields are private so callers cannot assert exactness by
/// setting a boolean or by manufacturing a digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactReleaseChildCertificate {
    identity: FormalReleaseIdentity,
    certificate_id: Uuid,
    certificate_hash: [u8; 32],
    child_kind: FormalReleaseRootField,
    source_record_id: Uuid,
    source_record_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactFormalReleaseCertificate {
    pub identity: FormalReleaseIdentity,
    pub run_goal_exact: ExactReleaseChildCertificate,
    pub trace_start_exact: ExactReleaseChildCertificate,
    pub governed_run_exact: ExactReleaseChildCertificate,
    pub secure_kernel: ExactReleaseChildCertificate,
    pub governance_kernel: ExactReleaseChildCertificate,
    pub concrete_trace: ExactReleaseChildCertificate,
    pub trace_feature_gates: ExactReleaseChildCertificate,
    pub compiler_action_exact: ExactReleaseChildCertificate,
    pub security_policies_exact: ExactReleaseChildCertificate,
    pub governance_operational: ExactReleaseChildCertificate,
    pub local_revision_trace_bound: ExactReleaseChildCertificate,
    pub creation_trace_bound: ExactReleaseChildCertificate,
    pub responsibility_trace_bound: ExactReleaseChildCertificate,
    pub global_trace_bound: ExactReleaseChildCertificate,
    pub proxy_trace_bound: ExactReleaseChildCertificate,
    pub cross_owner_trace_bound: ExactReleaseChildCertificate,
    pub comments: ExactReleaseChildCertificate,
    pub proxy: ExactReleaseChildCertificate,
    pub global_inventory_exact: ExactReleaseChildCertificate,
    pub global: ExactReleaseChildCertificate,
    pub cross_owner: ExactReleaseChildCertificate,
    pub interrogation: ExactReleaseChildCertificate,
    pub model: ExactReleaseChildCertificate,
    pub task_operational: ExactReleaseChildCertificate,
    pub task_intent_trace_bound: ExactReleaseChildCertificate,
    pub task_provenance_trace_bound: ExactReleaseChildCertificate,
    pub operational_history: ExactReleaseChildCertificate,
    pub operational_closure: ExactReleaseChildCertificate,
}

impl ExactFormalReleaseCertificate {
    pub fn validate(&self) -> Result<(), FormalReleaseValidationError> {
        if self.identity.trace.0 == 0 {
            return Err(FormalReleaseValidationError::IncompleteFormalRelease);
        }
        let children = [
            (FormalReleaseRootField::RunGoalExact, self.run_goal_exact),
            (
                FormalReleaseRootField::TraceStartExact,
                self.trace_start_exact,
            ),
            (
                FormalReleaseRootField::GovernedRunExact,
                self.governed_run_exact,
            ),
            (FormalReleaseRootField::SecureKernel, self.secure_kernel),
            (
                FormalReleaseRootField::GovernanceKernel,
                self.governance_kernel,
            ),
            (FormalReleaseRootField::ConcreteTrace, self.concrete_trace),
            (
                FormalReleaseRootField::TraceFeatureGates,
                self.trace_feature_gates,
            ),
            (
                FormalReleaseRootField::CompilerActionExact,
                self.compiler_action_exact,
            ),
            (
                FormalReleaseRootField::SecurityPoliciesExact,
                self.security_policies_exact,
            ),
            (
                FormalReleaseRootField::GovernanceOperational,
                self.governance_operational,
            ),
            (
                FormalReleaseRootField::LocalRevisionTraceBound,
                self.local_revision_trace_bound,
            ),
            (
                FormalReleaseRootField::CreationTraceBound,
                self.creation_trace_bound,
            ),
            (
                FormalReleaseRootField::ResponsibilityTraceBound,
                self.responsibility_trace_bound,
            ),
            (
                FormalReleaseRootField::GlobalTraceBound,
                self.global_trace_bound,
            ),
            (
                FormalReleaseRootField::ProxyTraceBound,
                self.proxy_trace_bound,
            ),
            (
                FormalReleaseRootField::CrossOwnerTraceBound,
                self.cross_owner_trace_bound,
            ),
            (FormalReleaseRootField::Comments, self.comments),
            (FormalReleaseRootField::Proxy, self.proxy),
            (
                FormalReleaseRootField::GlobalInventoryExact,
                self.global_inventory_exact,
            ),
            (FormalReleaseRootField::Global, self.global),
            (FormalReleaseRootField::CrossOwner, self.cross_owner),
            (FormalReleaseRootField::Interrogation, self.interrogation),
            (FormalReleaseRootField::Model, self.model),
            (
                FormalReleaseRootField::TaskOperational,
                self.task_operational,
            ),
            (
                FormalReleaseRootField::TaskIntentTraceBound,
                self.task_intent_trace_bound,
            ),
            (
                FormalReleaseRootField::TaskProvenanceTraceBound,
                self.task_provenance_trace_bound,
            ),
            (
                FormalReleaseRootField::OperationalHistory,
                self.operational_history,
            ),
            (
                FormalReleaseRootField::OperationalClosure,
                self.operational_closure,
            ),
        ];
        let mut certificate_ids = HashSet::new();
        if children.iter().any(|(expected_kind, child)| {
            child.identity != self.identity
                || child.child_kind != *expected_kind
                || !certificate_ids.insert(child.certificate_id)
                || child.certificate_id.is_nil()
                || child.source_record_id.is_nil()
                || child.certificate_hash == [0; 32]
                || child.source_record_hash == [0; 32]
        }) {
            return Err(FormalReleaseValidationError::IncompleteFormalRelease);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FormalReleaseValidationError {
    #[error("R540 release trace is not ordered and list-exact")]
    InvalidR540Trace,
    #[error("R541 formal release is missing a required conjunct")]
    IncompleteFormalRelease,
    #[error("R541 child was not reconstructed from its field-specific exact source")]
    InexactReleaseChild,
    #[error("evidence is not bound to one exact historical WorkAttempt")]
    InexactEvidenceWorkAttempt,
    #[error("formal encrypted payload cannot be reconstructed exactly")]
    MissingOrMismatchedFormalPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormalEncryptedPayloadPointer {
    pub source_record: Uuid,
    pub commitment: [u8; 32],
}

/// Resolve a content-addressed pointer to the complete canonical encrypted
/// payload bytes.  A commitment without its authoritative bytes is not a
/// formal `V.EncryptedPayload` and therefore fails closed.
pub fn reconstruct_exact_encrypted_payload(
    pointer: &FormalEncryptedPayloadPointer,
    authoritative_source_record: Uuid,
    authoritative_payload: Option<&[u8]>,
) -> Result<Vec<u8>, FormalReleaseValidationError> {
    let payload = authoritative_payload
        .filter(|_| pointer.source_record == authoritative_source_record)
        .ok_or(FormalReleaseValidationError::MissingOrMismatchedFormalPayload)?;
    if <[u8; 32]>::from(Sha256::digest(payload)) != pointer.commitment {
        return Err(FormalReleaseValidationError::MissingOrMismatchedFormalPayload);
    }
    Ok(payload.to_vec())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerSemanticTickBinding {
    pub semantic_tick: u64,
    /// Operational timestamp retained only to prove that projection ignores it.
    pub wall_clock_epoch_seconds: i64,
}

impl ServerSemanticTickBinding {
    #[must_use]
    pub const fn formal_tick(self) -> u64 {
        self.semantic_tick
    }
}

/// Minimal immutable coordinate needed to refine `R540EvidenceEventExact`.
/// The resolver intentionally receives every historical claim candidate so a
/// caller cannot substitute "latest claim for work" for the exact attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoricalEvidenceClaim {
    pub claim: ClaimId,
    pub work: WorkItemId,
    pub attempt: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectedEvidenceWorkAttempt {
    pub claim: ClaimId,
    pub work: WorkItemId,
    pub attempt: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceAcceptedWorkSnapshot {
    pub work: WorkItemId,
    pub attempt: u16,
    pub serves: Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceOutcomeCoordinate {
    pub claim: ClaimId,
    pub work: WorkItemId,
    pub attempt: u16,
    pub subject: ExactEvidenceSubject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExactEvidenceSubject {
    Task(Uuid),
    ToolCall(Uuid),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactR540EvidenceBinding {
    pub claim: ClaimId,
    pub work: WorkItemId,
    pub attempt: u16,
    pub obligation: Uuid,
    pub subject: ExactEvidenceSubject,
}

pub fn resolve_exact_r540_evidence_binding(
    accepted: EvidenceAcceptedWorkSnapshot,
    evidence_obligation: Uuid,
    evidence_subject: ExactEvidenceSubject,
    outcome: EvidenceOutcomeCoordinate,
    historical_claims: &[HistoricalEvidenceClaim],
    projected_work_attempts: &[ProjectedEvidenceWorkAttempt],
) -> Result<ExactR540EvidenceBinding, FormalReleaseValidationError> {
    if accepted.work != outcome.work
        || accepted.attempt != outcome.attempt
        || accepted.serves != evidence_obligation
        || evidence_subject != outcome.subject
    {
        return Err(FormalReleaseValidationError::InexactEvidenceWorkAttempt);
    }
    let claims = historical_claims
        .iter()
        .filter(|claim| claim.work == outcome.work && claim.attempt == outcome.attempt)
        .collect::<Vec<_>>();
    let projected = projected_work_attempts
        .iter()
        .filter(|event| {
            event.work == outcome.work
                && event.claim == outcome.claim
                && event.attempt == outcome.attempt
        })
        .collect::<Vec<_>>();
    if claims.len() != 1 || claims[0].claim != outcome.claim || projected.len() != 1 {
        return Err(FormalReleaseValidationError::InexactEvidenceWorkAttempt);
    }
    Ok(ExactR540EvidenceBinding {
        claim: outcome.claim,
        work: outcome.work,
        attempt: outcome.attempt,
        obligation: evidence_obligation,
        subject: evidence_subject,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(ordinal: u64, kind: R540ReleaseEventKind) -> OrderedR540ReleaseEvent {
        OrderedR540ReleaseEvent {
            ordinal,
            kind,
            semantic_tick: 10 + ordinal,
            event_hash: [u8::try_from(ordinal).unwrap_or(u8::MAX); 32],
        }
    }

    fn child(
        identity: FormalReleaseIdentity,
        kind: FormalReleaseRootField,
        ordinal: u128,
    ) -> ExactReleaseChildCertificate {
        ExactReleaseChildCertificate {
            identity,
            certificate_id: Uuid::from_u128(ordinal),
            certificate_hash: [u8::try_from(ordinal).expect("test ordinal"); 32],
            child_kind: kind,
            source_record_id: Uuid::from_u128(100 + ordinal),
            source_record_hash: [u8::try_from(100 + ordinal).expect("test source ordinal"); 32],
        }
    }

    fn exact_root(identity: FormalReleaseIdentity) -> ExactFormalReleaseCertificate {
        ExactFormalReleaseCertificate {
            identity,
            run_goal_exact: child(identity, FormalReleaseRootField::RunGoalExact, 1),
            trace_start_exact: child(identity, FormalReleaseRootField::TraceStartExact, 2),
            governed_run_exact: child(identity, FormalReleaseRootField::GovernedRunExact, 3),
            secure_kernel: child(identity, FormalReleaseRootField::SecureKernel, 4),
            governance_kernel: child(identity, FormalReleaseRootField::GovernanceKernel, 5),
            concrete_trace: child(identity, FormalReleaseRootField::ConcreteTrace, 6),
            trace_feature_gates: child(identity, FormalReleaseRootField::TraceFeatureGates, 7),
            compiler_action_exact: child(identity, FormalReleaseRootField::CompilerActionExact, 8),
            security_policies_exact: child(
                identity,
                FormalReleaseRootField::SecurityPoliciesExact,
                9,
            ),
            governance_operational: child(
                identity,
                FormalReleaseRootField::GovernanceOperational,
                10,
            ),
            local_revision_trace_bound: child(
                identity,
                FormalReleaseRootField::LocalRevisionTraceBound,
                11,
            ),
            creation_trace_bound: child(identity, FormalReleaseRootField::CreationTraceBound, 12),
            responsibility_trace_bound: child(
                identity,
                FormalReleaseRootField::ResponsibilityTraceBound,
                13,
            ),
            global_trace_bound: child(identity, FormalReleaseRootField::GlobalTraceBound, 14),
            proxy_trace_bound: child(identity, FormalReleaseRootField::ProxyTraceBound, 15),
            cross_owner_trace_bound: child(
                identity,
                FormalReleaseRootField::CrossOwnerTraceBound,
                16,
            ),
            comments: child(identity, FormalReleaseRootField::Comments, 17),
            proxy: child(identity, FormalReleaseRootField::Proxy, 18),
            global_inventory_exact: child(
                identity,
                FormalReleaseRootField::GlobalInventoryExact,
                19,
            ),
            global: child(identity, FormalReleaseRootField::Global, 20),
            cross_owner: child(identity, FormalReleaseRootField::CrossOwner, 21),
            interrogation: child(identity, FormalReleaseRootField::Interrogation, 22),
            model: child(identity, FormalReleaseRootField::Model, 23),
            task_operational: child(identity, FormalReleaseRootField::TaskOperational, 24),
            task_intent_trace_bound: child(
                identity,
                FormalReleaseRootField::TaskIntentTraceBound,
                25,
            ),
            task_provenance_trace_bound: child(
                identity,
                FormalReleaseRootField::TaskProvenanceTraceBound,
                26,
            ),
            operational_history: child(identity, FormalReleaseRootField::OperationalHistory, 27),
            operational_closure: child(identity, FormalReleaseRootField::OperationalClosure, 28),
        }
    }

    #[test]
    fn full_r540_trace_is_ordered_gap_free_and_list_exact() {
        let work = event(1, R540ReleaseEventKind::WorkAttempt);
        let outcome = event(2, R540ReleaseEventKind::WorkOutcome);
        let exact = ExactR540ReleaseTraceCertificate {
            trace: ExternalToolTraceId(1),
            run: RunId::new(),
            goal: GoalId::new(),
            start_tick: 10,
            end_tick: 12,
            ordered: vec![work.clone(), outcome.clone()],
            work_attempts: vec![work],
            work_outcomes: vec![outcome],
            blocker_resolutions: vec![],
            causal_links: vec![],
            tool_events: vec![],
            evidence: vec![],
            disclosures: vec![],
            model_invocations: vec![],
            interrogations: vec![],
        };
        assert!(exact.validate().is_ok());
        let mut reordered = exact.clone();
        reordered.ordered.swap(0, 1);
        assert_eq!(
            reordered.validate(),
            Err(FormalReleaseValidationError::InvalidR540Trace)
        );
        let mut missing = exact;
        missing.work_outcomes.clear();
        assert_eq!(
            missing.validate(),
            Err(FormalReleaseValidationError::InvalidR540Trace)
        );
    }

    #[test]
    fn blocker_gate_inventory_rejects_reorder_duplicate_and_wrong_tick() {
        let work = event(1, R540ReleaseEventKind::WorkAttempt);
        let mut blocker_one = event(2, R540ReleaseEventKind::BlockerResolution);
        blocker_one.semantic_tick = 11;
        let mut blocker_two = event(3, R540ReleaseEventKind::BlockerResolution);
        blocker_two.semantic_tick = 12;
        let exact = ExactR540ReleaseTraceCertificate {
            trace: ExternalToolTraceId(1),
            run: RunId::new(),
            goal: GoalId::new(),
            start_tick: 10,
            end_tick: 12,
            ordered: vec![work.clone(), blocker_one.clone(), blocker_two.clone()],
            work_attempts: vec![work],
            work_outcomes: vec![],
            blocker_resolutions: vec![blocker_one.clone(), blocker_two.clone()],
            causal_links: vec![],
            tool_events: vec![],
            evidence: vec![],
            disclosures: vec![],
            model_invocations: vec![],
            interrogations: vec![],
        };
        assert!(exact.validate().is_ok());

        let mut reordered = exact.clone();
        reordered.blocker_resolutions.swap(0, 1);
        assert!(reordered.validate().is_err());

        let mut duplicated = exact.clone();
        duplicated.blocker_resolutions.push(blocker_two);
        assert!(duplicated.validate().is_err());

        let mut wrong_tick = exact;
        wrong_tick.blocker_resolutions[0].semantic_tick = 12;
        assert!(wrong_tick.validate().is_err());
    }

    #[test]
    fn formal_release_requires_every_lean_root_conjunct() {
        let identity = FormalReleaseIdentity {
            trace: ExternalToolTraceId(1),
            run: RunId::new(),
            goal: GoalId::new(),
            start_tick: 10,
        };
        let mut certificate = exact_root(identity);
        assert!(certificate.validate().is_ok());
        certificate.secure_kernel = certificate.governance_kernel;
        assert_eq!(
            certificate.validate(),
            Err(FormalReleaseValidationError::IncompleteFormalRelease)
        );
    }

    #[test]
    fn release_children_are_kind_identity_source_and_id_exact() {
        let identity = FormalReleaseIdentity {
            trace: ExternalToolTraceId(7),
            run: RunId::new(),
            goal: GoalId::new(),
            start_tick: 41,
        };
        let exact = exact_root(identity);
        assert!(exact.validate().is_ok());

        let mut wrong_kind = exact.clone();
        wrong_kind.comments = wrong_kind.model;
        assert!(wrong_kind.validate().is_err());

        let mut wrong_identity = exact.clone();
        wrong_identity.model.identity = FormalReleaseIdentity {
            trace: ExternalToolTraceId(8),
            ..identity
        };
        assert!(wrong_identity.validate().is_err());

        let mut reused_id = exact;
        reused_id.model.certificate_id = reused_id.comments.certificate_id;
        assert!(reused_id.validate().is_err());

        let mut nil_source = exact_root(identity);
        nil_source.secure_kernel.source_record_id = Uuid::nil();
        assert!(nil_source.validate().is_err());

        let mut zero_source_hash = exact_root(identity);
        zero_source_hash.secure_kernel.source_record_hash = [0; 32];
        assert!(zero_source_hash.validate().is_err());
    }

    #[test]
    fn migration_exposes_all_28_field_specific_root_sources() {
        let migration =
            include_str!("../../../db/migrations/0035_agent_formal_release_comments.sql");
        for field in [
            "run_goal_exact",
            "trace_start_exact",
            "governed_run_exact",
            "secure_kernel",
            "governance_kernel",
            "concrete_trace",
            "trace_feature_gates",
            "compiler_action_exact",
            "security_policies_exact",
            "governance_operational",
            "local_revision_trace_bound",
            "creation_trace_bound",
            "responsibility_trace_bound",
            "global_trace_bound",
            "proxy_trace_bound",
            "cross_owner_trace_bound",
            "comments",
            "proxy",
            "global_inventory_exact",
            "global",
            "cross_owner",
            "interrogation",
            "model",
            "task_operational",
            "task_intent_trace_bound",
            "task_provenance_trace_bound",
            "operational_history",
            "operational_closure",
        ] {
            assert!(
                migration.contains(&format!("CREATE VIEW agent_r541_exact_child_{field} AS")),
                "missing field-specific exact source for {field}"
            );
        }
        assert!(migration.contains("source.source_relation=child.source_relation"));
        assert!(migration.contains("source_count<>28"));
        assert!(migration.contains("sprout_private.jsonb_array_is_prefix"));
        assert!(migration.contains("count(DISTINCT child.root_field)=28"));
        assert!(!migration.contains(
            "agent_r541_exact_formal_release_source_snapshots WHERE root_field='secure_kernel';"
        ));
        assert!(!migration.contains("'intent_records','[]'::jsonb"));
        assert!(!migration.contains("'provenance_records','[]'::jsonb"));
        assert!(!migration.contains("proven_conjuncts"));
        assert!(migration.contains("CREATE VIEW agent_r541_progress_kernel_field_sources AS"));
        assert!(migration.contains("CREATE VIEW agent_r541_evidence_discharge_field_sources AS"));
        assert!(
            migration.contains("CREATE VIEW agent_r541_authority_information_field_sources AS")
        );
        assert!(migration.contains("CREATE VIEW agent_r541_exact_agent_security_effects AS"));
        assert!(migration.contains("nested_certificates}')=41"));
        for field in [
            "progress.base",
            "progress.completionCommit",
            "progress.dynamics",
            "progress.measureLaws",
            "progress.goalValidityPersistsUntilTerminal",
            "progress.goalValidAtStart",
            "evidence_discharge.dischargeSound",
            "evidence_discharge.acceptedEvidenceCloses",
            "evidence_discharge.completionCommit",
            "authority_information.sponsorIsHuman",
            "authority_information.runAuthorityBoundedAtStart",
            "authority_information.runToolAuthorityBoundedAtStart",
            "authority_information.workOriginSound",
            "authority_information.humanDelegationAuthorityBound",
            "authority_information.humanDelegationToolAuthorityBound",
            "authority_information.childWorkAuthorityAttenuates",
            "authority_information.childWorkToolAuthorityAttenuates",
            "authority_information.agentMoveHasSecurityEffect",
            "authority_information.coreAgentActionFootprintComplete",
            "authority_information.coreAgentActionAllowedByContract",
            "authority_information.securityPolicyBoundToContract",
            "authority_information.effectWorkCertified",
            "authority_information.effectWorkSemanticallyEnabled",
            "authority_information.effectSecurityPolicyAllowed",
            "authority_information.effectWorkOwned",
            "authority_information.humanAssignedTaskControlIsolation",
            "authority_information.effectAuthoritySafe",
            "authority_information.toolUseAuthoritySafe",
            "authority_information.toolInvocationMatchesCall",
            "authority_information.toolFootprintComplete",
            "authority_information.effectWithinRunScope",
            "authority_information.modelContextProjectionExact",
            "authority_information.modelContextAuthoritySafe",
            "authority_information.modelContextWithinRunScope",
            "authority_information.infoContextContainerValid",
            "authority_information.toolContextSourceOwned",
            "authority_information.disclosureFootprintSound",
            "authority_information.canonicalResourceBody",
            "authority_information.contextualChatActionSafe",
            "authority_information.disclosureContextSafe",
            "authority_information.persistedDisclosureContextSafe",
        ] {
            assert!(
                migration.contains(&format!("('{field}')")),
                "missing {field}"
            );
        }
    }

    #[test]
    fn evidence_binds_to_exact_attempt_not_latest_claim() {
        let work = WorkItemId::new();
        let attempt_one = ClaimId::new();
        let attempt_two = ClaimId::new();
        let obligation = Uuid::from_u128(1);
        let subject = ExactEvidenceSubject::Task(Uuid::from_u128(3));
        let binding = resolve_exact_r540_evidence_binding(
            EvidenceAcceptedWorkSnapshot {
                work,
                attempt: 1,
                serves: obligation,
            },
            obligation,
            subject,
            EvidenceOutcomeCoordinate {
                claim: attempt_one,
                work,
                attempt: 1,
                subject,
            },
            &[
                HistoricalEvidenceClaim {
                    claim: attempt_one,
                    work,
                    attempt: 1,
                },
                HistoricalEvidenceClaim {
                    claim: attempt_two,
                    work,
                    attempt: 2,
                },
            ],
            &[ProjectedEvidenceWorkAttempt {
                claim: attempt_one,
                work,
                attempt: 1,
            }],
        )
        .expect("attempt one remains exact after attempt two exists");
        assert_eq!(binding.claim, attempt_one);
        assert_eq!(binding.attempt, 1);
    }

    #[test]
    fn evidence_missing_ambiguous_or_wrong_obligation_fails_closed() {
        let work = WorkItemId::new();
        let claim = ClaimId::new();
        let other = ClaimId::new();
        let obligation = Uuid::from_u128(1);
        let subject = ExactEvidenceSubject::Task(Uuid::from_u128(3));
        let snapshot = EvidenceAcceptedWorkSnapshot {
            work,
            attempt: 1,
            serves: obligation,
        };
        let outcome = EvidenceOutcomeCoordinate {
            claim,
            work,
            attempt: 1,
            subject,
        };
        let projected = [ProjectedEvidenceWorkAttempt {
            claim,
            work,
            attempt: 1,
        }];
        assert!(
            resolve_exact_r540_evidence_binding(
                snapshot,
                obligation,
                subject,
                outcome,
                &[],
                &projected
            )
            .is_err()
        );
        assert!(
            resolve_exact_r540_evidence_binding(
                snapshot,
                obligation,
                subject,
                outcome,
                &[
                    HistoricalEvidenceClaim {
                        claim,
                        work,
                        attempt: 1,
                    },
                    HistoricalEvidenceClaim {
                        claim: other,
                        work,
                        attempt: 1,
                    },
                ],
                &projected
            )
            .is_err()
        );
        assert!(
            resolve_exact_r540_evidence_binding(
                snapshot,
                Uuid::from_u128(2),
                subject,
                outcome,
                &[HistoricalEvidenceClaim {
                    claim,
                    work,
                    attempt: 1,
                }],
                &projected
            )
            .is_err()
        );
        assert!(
            resolve_exact_r540_evidence_binding(
                snapshot,
                obligation,
                ExactEvidenceSubject::Task(Uuid::from_u128(4)),
                outcome,
                &[HistoricalEvidenceClaim {
                    claim,
                    work,
                    attempt: 1,
                }],
                &projected
            )
            .is_err()
        );
    }

    #[test]
    fn formal_tick_uses_server_semantic_binding_not_wall_clock() {
        let binding = ServerSemanticTickBinding {
            semantic_tick: 12,
            wall_clock_epoch_seconds: 4_000_000_000,
        };
        assert_eq!(binding.formal_tick(), 12);
        assert_ne!(
            binding.formal_tick(),
            binding.wall_clock_epoch_seconds as u64
        );
    }

    #[test]
    fn evidence_subject_is_exact_and_wrong_subject_is_rejected() {
        let work = WorkItemId::new();
        let claim = ClaimId::new();
        let obligation = Uuid::from_u128(20);
        let task = ExactEvidenceSubject::Task(Uuid::from_u128(21));
        let snapshot = EvidenceAcceptedWorkSnapshot {
            work,
            attempt: 1,
            serves: obligation,
        };
        let outcome = EvidenceOutcomeCoordinate {
            claim,
            work,
            attempt: 1,
            subject: task,
        };
        let claims = [HistoricalEvidenceClaim {
            claim,
            work,
            attempt: 1,
        }];
        let attempts = [ProjectedEvidenceWorkAttempt {
            claim,
            work,
            attempt: 1,
        }];
        assert!(
            resolve_exact_r540_evidence_binding(
                snapshot, obligation, task, outcome, &claims, &attempts
            )
            .is_ok()
        );
        assert!(
            resolve_exact_r540_evidence_binding(
                snapshot,
                obligation,
                ExactEvidenceSubject::Task(Uuid::from_u128(22)),
                outcome,
                &claims,
                &attempts
            )
            .is_err()
        );
    }

    #[test]
    fn model_and_disclosure_payloads_require_exact_authoritative_bytes() {
        let source = Uuid::from_u128(10);
        let input = br#"{"version":1,"algorithm":"x","key_id":"k","nonce_b64":"AQ==","ciphertext_b64":"Ag=="}"#;
        let pointer = FormalEncryptedPayloadPointer {
            source_record: source,
            commitment: Sha256::digest(input).into(),
        };
        assert_eq!(
            reconstruct_exact_encrypted_payload(&pointer, source, Some(input)).unwrap(),
            input
        );
        assert!(reconstruct_exact_encrypted_payload(&pointer, source, None).is_err());
        assert!(
            reconstruct_exact_encrypted_payload(&pointer, Uuid::from_u128(11), Some(input))
                .is_err()
        );
        assert!(reconstruct_exact_encrypted_payload(&pointer, source, Some(b"different")).is_err());
    }

    #[test]
    fn migration_projectors_use_semantic_ticks_and_typed_sources() {
        let migration =
            include_str!("../../../db/migrations/0035_agent_formal_release_comments.sql");
        let legacy_epoch = migration
            .find("floor(extract(epoch")
            .expect("legacy evidence compatibility branch remains explicit");
        let semantic_allocator = migration
            .find("One canonical logical clock")
            .expect("0035-native semantic allocator marker");
        assert!(legacy_epoch < semantic_allocator);
        assert!(!migration[semantic_allocator..].contains("floor(extract(epoch"));
        for required in [
            "'subject',evidence_subject",
            "source.semantic_tick=event.semantic_tick",
            "dispatch.semantic_tick=event.semantic_tick",
            "source.applied_semantic_tick=event.semantic_tick",
            "agent_r540_typed_exact_release_events",
            "'input_payload',sprout_private.try_parse_encrypted_payload",
            "'output_payload',sprout_private.try_parse_encrypted_payload",
            "'payload',sprout_private.try_parse_encrypted_payload",
            "'question',jsonb_build_object",
            "'answer',jsonb_build_object",
            "'delta',session.causal_delta",
        ] {
            assert!(
                migration.contains(required),
                "missing exact projector: {required}"
            );
        }
    }

    #[test]
    fn blocker_resolution_uses_the_event_semantic_tick() {
        let migration =
            include_str!("../../../db/migrations/0035_agent_formal_release_comments.sql");
        assert!(migration.contains(
            "'terminal_status',NEW.terminal_status,'observed_at',transition_row.semantic_tick"
        ));
        assert!(migration.contains("'observed_at',transition_row.semantic_tick"));
    }

    #[test]
    fn model_interrogation_and_disclosure_ticks_are_server_semantic_ticks() {
        let migration =
            include_str!("../../../db/migrations/0035_agent_formal_release_comments.sql");
        for required in [
            "dispatch.semantic_tick=event.semantic_tick",
            "source.semantic_tick=event.semantic_tick",
            "'observed_at',NEW.semantic_tick",
            "applied_tick:=NEW.applied_semantic_tick",
            "source.observed_tick=event.semantic_tick",
        ] {
            assert!(migration.contains(required));
        }
        let semantic_allocator = migration
            .find("One canonical logical clock")
            .expect("0035-native semantic allocator marker");
        assert!(!migration[semantic_allocator..].contains("floor(extract(epoch"));
    }

    #[test]
    fn interrogation_reconstructs_exact_question_answer_and_context_payloads() {
        let session = Uuid::from_u128(30);
        let answer = Uuid::from_u128(31);
        let question_payload = br#"{"version":1,"algorithm":"x","key_id":"q","nonce_b64":"AQ==","ciphertext_b64":"Ag=="}"#;
        let answer_payload = br#"{"version":1,"algorithm":"x","key_id":"a","nonce_b64":"Aw==","ciphertext_b64":"BA=="}"#;
        let question_pointer = FormalEncryptedPayloadPointer {
            source_record: session,
            commitment: Sha256::digest(question_payload).into(),
        };
        let answer_pointer = FormalEncryptedPayloadPointer {
            source_record: answer,
            commitment: Sha256::digest(answer_payload).into(),
        };
        assert_eq!(
            reconstruct_exact_encrypted_payload(&question_pointer, session, Some(question_payload))
                .unwrap(),
            question_payload
        );
        assert_eq!(
            reconstruct_exact_encrypted_payload(&answer_pointer, answer, Some(answer_payload))
                .unwrap(),
            answer_payload
        );
        let migration =
            include_str!("../../../db/migrations/0035_agent_formal_release_comments.sql");
        for required in [
            "'session',exact.event_snapshot->'session'",
            "'question',jsonb_build_object",
            "'answer',jsonb_build_object",
            "'delta',session.causal_delta",
            "'context',jsonb_build_object('direct_sources'",
            "'projection',jsonb_build_object('direct_sources_exposed'",
        ] {
            assert!(migration.contains(required));
        }
    }

    #[test]
    fn missing_payload_or_required_json_coordinate_fails_closed() {
        let source = Uuid::from_u128(40);
        let pointer = FormalEncryptedPayloadPointer {
            source_record: source,
            commitment: [0; 32],
        };
        assert!(reconstruct_exact_encrypted_payload(&pointer, source, None).is_err());
        let migration =
            include_str!("../../../db/migrations/0035_agent_formal_release_comments.sql");
        for required in [
            "work_snapshot->>'run' IS DISTINCT FROM",
            "work_snapshot->>'goal' IS DISTINCT FROM",
            "work_snapshot->>'id' IS DISTINCT FROM",
            "work_snapshot->>'serves' IS DISTINCT FROM",
            "work_snapshot->>'attempt' IS NULL",
            "claim_json->>'claimant' IS DISTINCT FROM",
        ] {
            assert!(migration.contains(required));
        }
    }
}
