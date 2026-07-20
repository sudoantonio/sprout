use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AttachmentId, CompletedAttachmentId, EncryptedPayload, PretaskId, ProjectId,
    RequiredAttachmentId, ResourceId, TaskAssignment, TaskId, TemplateAttachmentId, UserId,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AttachmentState {
    PendingUpload,
    Available {
        uploaded_at: DateTime<Utc>,
    },
    PendingDeletion {
        deleted_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    },
}

/// Blob metadata contains only ciphertext-derived and routing fields. Names,
/// MIME declarations, manifests, and client paths belong inside `payload`.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentMetadata {
    pub id: AttachmentId,
    pub project_id: ProjectId,
    pub resource_id: ResourceId,
    pub uploaded_by: UserId,
    pub byte_length: u64,
    pub ciphertext_digest: [u8; 32],
    pub storage_key: String,
    pub key_epoch: u32,
    pub payload: EncryptedPayload,
    pub state: AttachmentState,
    pub created_at: DateTime<Utc>,
}

impl AttachmentMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: AttachmentId,
        project_id: ProjectId,
        resource_id: ResourceId,
        uploaded_by: UserId,
        byte_length: u64,
        ciphertext_digest: [u8; 32],
        storage_key: impl Into<String>,
        key_epoch: u32,
        payload: EncryptedPayload,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AttachmentError> {
        let storage_key = storage_key.into();
        if byte_length == 0 {
            return Err(AttachmentError::EmptyCiphertext);
        }
        if key_epoch == 0 {
            return Err(AttachmentError::InvalidKeyEpoch);
        }
        validate_storage_key(&storage_key)?;
        Ok(Self {
            id,
            project_id,
            resource_id,
            uploaded_by,
            byte_length,
            ciphertext_digest,
            storage_key,
            key_epoch,
            payload,
            state: AttachmentState::PendingUpload,
            created_at,
        })
    }

    pub fn mark_uploaded(&mut self, at: DateTime<Utc>) -> Result<(), AttachmentError> {
        if at < self.created_at {
            return Err(AttachmentError::TimestampBeforeCreation);
        }
        if !matches!(self.state, AttachmentState::PendingUpload) {
            return Err(AttachmentError::InvalidStateTransition);
        }
        self.state = AttachmentState::Available { uploaded_at: at };
        Ok(())
    }

    pub fn schedule_deletion(
        &mut self,
        deleted_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    ) -> Result<(), AttachmentError> {
        if purge_at <= deleted_at {
            return Err(AttachmentError::InvalidPurgeDeadline);
        }
        if !matches!(self.state, AttachmentState::Available { .. }) {
            return Err(AttachmentError::InvalidStateTransition);
        }
        self.state = AttachmentState::PendingDeletion {
            deleted_at,
            purge_at,
        };
        Ok(())
    }
}

fn validate_storage_key(storage_key: &str) -> Result<(), AttachmentError> {
    let Some(stem) = storage_key.strip_suffix(".blob") else {
        return Err(AttachmentError::InvalidStorageKey);
    };
    if stem.len() != 32
        || !stem
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AttachmentError::InvalidStorageKey);
    }
    Ok(())
}

impl fmt::Debug for AttachmentMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttachmentMetadata")
            .field("id", &self.id)
            .field("project_id", &self.project_id)
            .field("resource_id", &self.resource_id)
            .field("uploaded_by", &self.uploaded_by)
            .field("byte_length", &self.byte_length)
            .field("ciphertext_digest", &"[REDACTED]")
            .field("storage_key", &"[REDACTED]")
            .field("key_epoch", &self.key_epoch)
            .field("payload", &self.payload)
            .field("state", &self.state)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PretaskTemplateAttachment {
    pub id: TemplateAttachmentId,
    pub pretask_id: PretaskId,
    pub blob_id: AttachmentId,
    pub encrypted_metadata: EncryptedPayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskRequiredAttachment {
    pub id: RequiredAttachmentId,
    pub task_id: TaskId,
    pub source_template_id: Option<TemplateAttachmentId>,
    pub blob_id: AttachmentId,
    pub encrypted_snapshot: EncryptedPayload,
}

impl TaskRequiredAttachment {
    pub fn from_template(
        id: RequiredAttachmentId,
        task_id: TaskId,
        task_source_pretask_id: PretaskId,
        template: &PretaskTemplateAttachment,
        blob_id: AttachmentId,
        encrypted_snapshot: EncryptedPayload,
    ) -> Result<Self, AttachmentError> {
        if template.pretask_id != task_source_pretask_id {
            return Err(AttachmentError::TemplateTaskMismatch);
        }
        Ok(Self {
            id,
            task_id,
            source_template_id: Some(template.id),
            blob_id,
            encrypted_snapshot,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskCompletedAttachment {
    pub id: CompletedAttachmentId,
    pub task_id: TaskId,
    pub assignment_id: crate::TaskAssignmentId,
    pub required_attachment_id: Option<RequiredAttachmentId>,
    pub blob_id: AttachmentId,
    pub uploaded_by: UserId,
    pub encrypted_metadata: EncryptedPayload,
}

impl TaskCompletedAttachment {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: CompletedAttachmentId,
        task_id: TaskId,
        assignment: &TaskAssignment,
        required: Option<&TaskRequiredAttachment>,
        blob_id: AttachmentId,
        uploaded_by: UserId,
        encrypted_metadata: EncryptedPayload,
    ) -> Result<Self, AttachmentError> {
        if assignment.task_id != task_id
            || !assignment.is_active()
            || assignment.assignee_id != uploaded_by
        {
            return Err(AttachmentError::OnlyActiveAssigneeMayUpload);
        }
        if required.is_some_and(|required| required.task_id != task_id) {
            return Err(AttachmentError::RequiredAttachmentTaskMismatch);
        }
        Ok(Self {
            id,
            task_id,
            assignment_id: assignment.id,
            required_attachment_id: required.map(|required| required.id),
            blob_id,
            uploaded_by,
            encrypted_metadata,
        })
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AttachmentError {
    #[error("attachment ciphertext cannot be empty")]
    EmptyCiphertext,
    #[error("attachment key epoch must be positive")]
    InvalidKeyEpoch,
    #[error("attachment storage key must be an opaque server-generated basename")]
    InvalidStorageKey,
    #[error("attachment timestamp cannot precede creation")]
    TimestampBeforeCreation,
    #[error("invalid attachment state transition")]
    InvalidStateTransition,
    #[error("purge deadline must follow deletion")]
    InvalidPurgeDeadline,
    #[error("template attachment does not belong to the task pretask")]
    TemplateTaskMismatch,
    #[error("only the active assignee may upload a completed attachment")]
    OnlyActiveAssigneeMayUpload,
    #[error("required attachment belongs to another task")]
    RequiredAttachmentTaskMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskAssignmentId;

    fn payload(byte: u8) -> EncryptedPayload {
        EncryptedPayload::new(1, "test", "key", vec![byte], vec![byte]).unwrap()
    }

    #[test]
    fn llr_05_1_attachment_entities_have_distinct_identity_and_provenance() {
        let task_id = TaskId::new();
        let pretask_id = PretaskId::new();
        let template = PretaskTemplateAttachment {
            id: TemplateAttachmentId::new(),
            pretask_id,
            blob_id: AttachmentId::new(),
            encrypted_metadata: payload(1),
        };
        let required = TaskRequiredAttachment::from_template(
            RequiredAttachmentId::new(),
            task_id,
            pretask_id,
            &template,
            AttachmentId::new(),
            payload(2),
        )
        .unwrap();
        let assignee = UserId::new();
        let assignment = TaskAssignment {
            id: TaskAssignmentId::new(),
            task_id,
            assignee_id: assignee,
            assigned_at: Utc::now(),
            revoked_at: None,
        };
        let completed = TaskCompletedAttachment::new(
            CompletedAttachmentId::new(),
            task_id,
            &assignment,
            Some(&required),
            AttachmentId::new(),
            assignee,
            payload(3),
        )
        .unwrap();
        assert_ne!(template.blob_id, required.blob_id);
        assert_ne!(required.blob_id, completed.blob_id);
    }

    #[test]
    fn llr_05_2_rejects_paths_as_storage_keys() {
        assert_eq!(
            validate_storage_key("../../secret.blob"),
            Err(AttachmentError::InvalidStorageKey)
        );
        assert!(validate_storage_key("0123456789abcdef0123456789abcdef.blob").is_ok());
    }
}
