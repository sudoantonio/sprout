//! Native governed Comment semantics.
//!
//! Comment is part of Sprout's closed native action/resource language.  It is
//! deliberately independent from the external-tool catalog and carries only
//! opaque encrypted payload bytes at the server boundary.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CommentId, ResourceId, RunId, UserId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCommentAuthorKind {
    Administrator,
    User,
    Agent,
}

impl NativeCommentAuthorKind {
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::Administrator => 3,
            Self::User => 2,
            Self::Agent => 1,
        }
    }
}

/// Exact concrete refinement of Lean `NewComment`.  Identity, author, depth,
/// project, tick and trace coordinates are intentionally absent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewNativeComment {
    pub recipient: UserId,
    pub target: ResourceId,
    pub parent: Option<CommentId>,
    pub encrypted_payload: Vec<u8>,
    pub key_epoch: u32,
}

/// Server-derived concrete refinement of Lean `Comment`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeComment {
    pub id: CommentId,
    pub author: UserId,
    pub recipient: UserId,
    pub target: ResourceId,
    pub parent: Option<CommentId>,
    pub agent_depth: u32,
    pub encrypted_payload: Vec<u8>,
    pub key_epoch: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommentAdmissionFacts {
    pub author_kind: NativeCommentAuthorKind,
    pub recipient_is_agent: bool,
    pub author_can_see_target: bool,
    pub author_can_post: bool,
    pub recipient_can_see_or_created_target: bool,
    pub active_key_epoch: u32,
    pub max_agent_comment_depth: u32,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum NativeCommentValidationError {
    #[error("comment recipient must be an agent")]
    RecipientMustBeAgent,
    #[error("comment author and recipient must differ")]
    SelfComment,
    #[error("comment target is not visible or postable")]
    TargetNotPostable,
    #[error("recipient cannot participate on the target")]
    RecipientCannotSeeTarget,
    #[error("comment key epoch is stale")]
    StaleKeyEpoch,
    #[error("encrypted comment payload is empty")]
    EmptyPayload,
    #[error("human comments must have depth zero")]
    HumanDepth,
    #[error("agent root comment is not fresh")]
    DuplicateAgentRoot,
    #[error("agent comment parent is missing or incompatible")]
    InvalidParent,
    #[error("agent comment depth is exhausted")]
    DepthExhausted,
    #[error("comment history contains a cycle")]
    ParentCycle,
    #[error("comment priority discipline was violated")]
    PriorityViolation,
    #[error("R5.41 Comment inventory is not exact")]
    InvalidSurfaceCertificate,
}

/// Derive the only admissible depth.  This is the server-owned part of
/// `CommentAdmissible`, `AgentRootCommentFresh` and `AgentCommentShape`.
pub fn derive_native_comment_depth(
    draft: &NewNativeComment,
    author: UserId,
    facts: CommentAdmissionFacts,
    comments: &HashMap<CommentId, NativeComment>,
) -> Result<u32, NativeCommentValidationError> {
    if !facts.recipient_is_agent {
        return Err(NativeCommentValidationError::RecipientMustBeAgent);
    }
    if author == draft.recipient {
        return Err(NativeCommentValidationError::SelfComment);
    }
    if !facts.author_can_see_target || !facts.author_can_post {
        return Err(NativeCommentValidationError::TargetNotPostable);
    }
    if !facts.recipient_can_see_or_created_target {
        return Err(NativeCommentValidationError::RecipientCannotSeeTarget);
    }
    if draft.key_epoch == 0 || draft.key_epoch != facts.active_key_epoch {
        return Err(NativeCommentValidationError::StaleKeyEpoch);
    }
    if draft.encrypted_payload.is_empty() {
        return Err(NativeCommentValidationError::EmptyPayload);
    }

    if facts.author_kind != NativeCommentAuthorKind::Agent {
        return Ok(0);
    }
    match draft.parent {
        None => {
            if facts.max_agent_comment_depth == 0 {
                return Err(NativeCommentValidationError::DepthExhausted);
            }
            if comments.values().any(|comment| {
                comment.author == author
                    && comment.recipient == draft.recipient
                    && comment.target == draft.target
                    && comment.parent.is_none()
                    && comment.agent_depth > 0
            }) {
                return Err(NativeCommentValidationError::DuplicateAgentRoot);
            }
            Ok(1)
        }
        Some(parent_id) => {
            let parent = comments
                .get(&parent_id)
                .ok_or(NativeCommentValidationError::InvalidParent)?;
            if parent.recipient != author || parent.target != draft.target {
                return Err(NativeCommentValidationError::InvalidParent);
            }
            let next = parent
                .agent_depth
                .checked_add(1)
                .ok_or(NativeCommentValidationError::DepthExhausted)?;
            if next > facts.max_agent_comment_depth {
                return Err(NativeCommentValidationError::DepthExhausted);
            }
            let mut cursor = Some(parent_id);
            let mut visited = HashSet::new();
            while let Some(id) = cursor {
                if !visited.insert(id) {
                    return Err(NativeCommentValidationError::ParentCycle);
                }
                cursor = comments.get(&id).and_then(|comment| comment.parent);
            }
            Ok(next)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommentPriorityEvent {
    pub comment: CommentId,
    pub recipient: UserId,
    pub author_kind: NativeCommentAuthorKind,
    pub posted_tick: u64,
    pub responded_tick: Option<u64>,
}

/// Finite-ledger form of `CommentPriorityDiscipline`.  For the same recipient,
/// a lower-priority response cannot precede an older still-activating higher
/// priority response.
pub fn validate_comment_priority_discipline(
    events: &[CommentPriorityEvent],
) -> Result<(), NativeCommentValidationError> {
    for high in events {
        for low in events {
            if high.recipient != low.recipient
                || high.posted_tick > low.posted_tick
                || high.author_kind.priority() <= low.author_kind.priority()
            {
                continue;
            }
            if let Some(low_response) = low.responded_tick {
                if low_response >= low.posted_tick
                    && high.responded_tick.is_none_or(|tick| tick > low_response)
                {
                    return Err(NativeCommentValidationError::PriorityViolation);
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentSurfaceMode {
    Enabled,
    DisabledFailClosed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderedCommentBinding {
    pub ordinal: u64,
    pub comment: CommentId,
    pub semantic_tick: u64,
    pub record_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactCommentSurfaceCertificate {
    pub trace_number: u64,
    pub run: RunId,
    pub version: u32,
    pub previous_certificate_hash: Option<[u8; 32]>,
    pub inventory: Vec<OrderedCommentBinding>,
    pub certified_records: Vec<OrderedCommentBinding>,
    pub mode: CommentSurfaceMode,
}

impl ExactCommentSurfaceCertificate {
    pub fn validate(&self) -> Result<(), NativeCommentValidationError> {
        if self.trace_number == 0 || self.version == 0 || self.inventory != self.certified_records {
            return Err(NativeCommentValidationError::InvalidSurfaceCertificate);
        }
        for (index, record) in self.inventory.iter().enumerate() {
            if record.ordinal != u64::try_from(index + 1).unwrap_or(u64::MAX) {
                return Err(NativeCommentValidationError::InvalidSurfaceCertificate);
            }
        }
        match self.mode {
            CommentSurfaceMode::Enabled if self.inventory.is_empty() => {
                Err(NativeCommentValidationError::InvalidSurfaceCertificate)
            }
            CommentSurfaceMode::DisabledFailClosed if !self.inventory.is_empty() => {
                Err(NativeCommentValidationError::InvalidSurfaceCertificate)
            }
            _ => Ok(()),
        }
    }

    pub fn extends(&self, previous: &Self) -> Result<(), NativeCommentValidationError> {
        if self.trace_number != previous.trace_number
            || self.run != previous.run
            || self.version != previous.version + 1
            || self.previous_certificate_hash.is_none()
            || !self.inventory.starts_with(&previous.inventory)
        {
            return Err(NativeCommentValidationError::InvalidSurfaceCertificate);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(recipient: UserId, target: ResourceId) -> NewNativeComment {
        NewNativeComment {
            recipient,
            target,
            parent: None,
            encrypted_payload: vec![1],
            key_epoch: 4,
        }
    }

    fn facts(kind: NativeCommentAuthorKind) -> CommentAdmissionFacts {
        CommentAdmissionFacts {
            author_kind: kind,
            recipient_is_agent: true,
            author_can_see_target: true,
            author_can_post: true,
            recipient_can_see_or_created_target: true,
            active_key_epoch: 4,
            max_agent_comment_depth: 3,
        }
    }

    #[test]
    fn human_and_administrator_depth_is_server_derived_zero() {
        for kind in [
            NativeCommentAuthorKind::User,
            NativeCommentAuthorKind::Administrator,
        ] {
            assert_eq!(
                derive_native_comment_depth(
                    &draft(UserId::new(), ResourceId::new()),
                    UserId::new(),
                    facts(kind),
                    &HashMap::new()
                ),
                Ok(0)
            );
        }
    }

    #[test]
    fn agent_root_and_reply_shape_are_exact() {
        let agent = UserId::new();
        let recipient = UserId::new();
        let target = ResourceId::new();
        let root = draft(recipient, target);
        assert_eq!(
            derive_native_comment_depth(
                &root,
                agent,
                facts(NativeCommentAuthorKind::Agent),
                &HashMap::new()
            ),
            Ok(1)
        );
        let parent_id = CommentId::new();
        let parent = NativeComment {
            id: parent_id,
            author: recipient,
            recipient: agent,
            target,
            parent: None,
            agent_depth: 1,
            encrypted_payload: vec![2],
            key_epoch: 4,
        };
        let mut reply = draft(recipient, target);
        reply.parent = Some(parent_id);
        assert_eq!(
            derive_native_comment_depth(
                &reply,
                agent,
                facts(NativeCommentAuthorKind::Agent),
                &HashMap::from([(parent_id, parent)])
            ),
            Ok(2)
        );
    }

    #[test]
    fn agent_root_freshness_parent_and_depth_fail_closed() {
        let agent = UserId::new();
        let recipient = UserId::new();
        let target = ResourceId::new();
        let root_id = CommentId::new();
        let existing = NativeComment {
            id: root_id,
            author: agent,
            recipient,
            target,
            parent: None,
            agent_depth: 3,
            encrypted_payload: vec![1],
            key_epoch: 4,
        };
        let history = HashMap::from([(root_id, existing)]);
        assert_eq!(
            derive_native_comment_depth(
                &draft(recipient, target),
                agent,
                facts(NativeCommentAuthorKind::Agent),
                &history
            ),
            Err(NativeCommentValidationError::DuplicateAgentRoot)
        );
        let mut reply = draft(recipient, target);
        reply.parent = Some(root_id);
        assert_eq!(
            derive_native_comment_depth(
                &reply,
                agent,
                facts(NativeCommentAuthorKind::Agent),
                &history
            ),
            Err(NativeCommentValidationError::InvalidParent)
        );
    }

    #[test]
    fn comment_priority_is_temporal_and_recipient_local() {
        let recipient = UserId::new();
        let other = UserId::new();
        let high = CommentPriorityEvent {
            comment: CommentId::new(),
            recipient,
            author_kind: NativeCommentAuthorKind::Administrator,
            posted_tick: 10,
            responded_tick: Some(14),
        };
        let low = CommentPriorityEvent {
            comment: CommentId::new(),
            recipient,
            author_kind: NativeCommentAuthorKind::Agent,
            posted_tick: 11,
            responded_tick: Some(13),
        };
        assert_eq!(
            validate_comment_priority_discipline(&[high, low]),
            Err(NativeCommentValidationError::PriorityViolation)
        );
        assert!(
            validate_comment_priority_discipline(&[
                high,
                CommentPriorityEvent {
                    recipient: other,
                    ..low
                }
            ])
            .is_ok()
        );
    }

    #[test]
    fn comment_surface_is_list_exact_nonvacuous_and_gap_free() {
        let record = OrderedCommentBinding {
            ordinal: 1,
            comment: CommentId::new(),
            semantic_tick: 10,
            record_hash: [7; 32],
        };
        let exact = ExactCommentSurfaceCertificate {
            trace_number: 1,
            run: RunId::new(),
            version: 1,
            previous_certificate_hash: None,
            inventory: vec![record.clone()],
            certified_records: vec![record],
            mode: CommentSurfaceMode::Enabled,
        };
        assert!(exact.validate().is_ok());
        let mut missing = exact.clone();
        missing.certified_records.clear();
        assert_eq!(
            missing.validate(),
            Err(NativeCommentValidationError::InvalidSurfaceCertificate)
        );
        let vacuous = ExactCommentSurfaceCertificate {
            trace_number: 1,
            run: RunId::new(),
            version: 1,
            previous_certificate_hash: None,
            inventory: vec![],
            certified_records: vec![],
            mode: CommentSurfaceMode::Enabled,
        };
        assert_eq!(
            vacuous.validate(),
            Err(NativeCommentValidationError::InvalidSurfaceCertificate)
        );
    }
}
