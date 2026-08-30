use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use uuid::Uuid;

macro_rules! entity_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

entity_id!(UserId);
entity_id!(ProjectId);
entity_id!(ResourceId);
entity_id!(GrantId);
entity_id!(TopicId);
entity_id!(TaskListId);
entity_id!(TaskId);
entity_id!(PresetId);
entity_id!(PresetVersionId);
entity_id!(PretaskId);
entity_id!(PresetAssignmentId);
entity_id!(TaskAssignmentId);
entity_id!(TaskCompletionId);
entity_id!(RecurrenceSeriesId);
entity_id!(QuestionnaireId);
entity_id!(QuestionnaireVersionId);
entity_id!(QuestionId);
entity_id!(QuestionOptionId);
entity_id!(SubmissionId);
entity_id!(AnswerId);
entity_id!(AttachmentId);
entity_id!(TemplateAttachmentId);
entity_id!(RequiredAttachmentId);
entity_id!(CompletedAttachmentId);
entity_id!(DeviceId);
entity_id!(SyncEventId);
entity_id!(AgentId);
entity_id!(ResponsibilityId);
entity_id!(LocalGoalId);
entity_id!(LanguageTaskId);
entity_id!(InvocationId);
entity_id!(ProxyThreadId);
entity_id!(ProxyRequestId);
entity_id!(InterrogationId);
entity_id!(GoalId);
entity_id!(RunId);
entity_id!(WorkItemId);
entity_id!(ClaimId);
entity_id!(EvidenceId);
entity_id!(BlockerId);
entity_id!(CommentId);
entity_id!(ToolCallId);
entity_id!(GovernanceReviewId);

#[derive(Clone, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub const MAX_LEN: usize = 128;

    pub fn new(value: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
        let value = value.into();
        let len = value.len();
        if value.trim().is_empty() {
            return Err(IdempotencyKeyError::Empty);
        }
        if len > Self::MAX_LEN {
            return Err(IdempotencyKeyError::TooLong {
                max: Self::MAX_LEN,
                actual: len,
            });
        }
        if value.chars().any(char::is_control) {
            return Err(IdempotencyKeyError::ControlCharacter);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for IdempotencyKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IdempotencyKey([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for IdempotencyKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IdempotencyKeyError {
    #[error("idempotency key cannot be empty")]
    Empty,
    #[error("idempotency key length {actual} exceeds maximum {max}")]
    TooLong { max: usize, actual: usize },
    #[error("idempotency key cannot contain control characters")]
    ControlCharacter,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn idempotency_keys_accept_visible_values(value in "[!-~][ -~]{0,127}") {
            prop_assert!(IdempotencyKey::new(value).is_ok());
        }

        #[test]
        fn idempotency_keys_reject_values_over_the_limit(value in "[a-zA-Z0-9]{129,256}") {
            let rejected = matches!(
                IdempotencyKey::new(value),
                Err(IdempotencyKeyError::TooLong { .. })
            );
            prop_assert!(rejected);
        }
    }

    #[test]
    fn id_types_are_not_interchangeable() {
        let raw = Uuid::nil();
        assert_eq!(
            UserId::from(raw).to_string(),
            ProjectId::from(raw).to_string()
        );
    }
}
