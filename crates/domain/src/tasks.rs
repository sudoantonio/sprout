use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    EncryptedPayload, PresetAssignmentId, PretaskId, ProjectId, QuestionnaireVersion,
    QuestionnaireVersionId, RecurrenceSeriesId, TaskAssignmentId, TaskId, TaskListId, UserId,
};

/// The only task semantic the service may inspect.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Priority,
    Deadline,
    Recurring,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskList {
    pub id: TaskListId,
    pub project_id: ProjectId,
    pub payload: EncryptedPayload,
    pub payload_version: u64,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct RecurrenceOccurrence {
    pub series_id: RecurrenceSeriesId,
    pub occurrence_number: u64,
}

impl RecurrenceOccurrence {
    pub fn new(series_id: RecurrenceSeriesId, occurrence_number: u64) -> Result<Self, TaskError> {
        if occurrence_number == 0 {
            return Err(TaskError::ZeroOccurrenceNumber);
        }
        Ok(Self {
            series_id,
            occurrence_number,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskAssignment {
    pub id: TaskAssignmentId,
    pub task_id: TaskId,
    pub assignee_id: UserId,
    pub assigned_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl TaskAssignment {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskCompletion {
    pub assignment_id: TaskAssignmentId,
    pub completed_by: UserId,
    pub completed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskState {
    Open,
    Completed(TaskCompletion),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Task {
    id: TaskId,
    project_id: ProjectId,
    list_id: TaskListId,
    kind: TaskKind,
    payload: EncryptedPayload,
    selected_value_snapshot: EncryptedPayload,
    state: TaskState,
    source_pretask_id: Option<PretaskId>,
    preset_assignment_id: Option<PresetAssignmentId>,
    copied_from_task_id: Option<TaskId>,
    questionnaire_version_id: Option<QuestionnaireVersionId>,
    recurrence: Option<RecurrenceOccurrence>,
    created_at: DateTime<Utc>,
    payload_version: u64,
}

impl Task {
    pub fn new(
        id: TaskId,
        list: &TaskList,
        kind: TaskKind,
        payload: EncryptedPayload,
        selected_value_snapshot: EncryptedPayload,
        recurrence: Option<RecurrenceOccurrence>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, TaskError> {
        if list.archived_at.is_some() {
            return Err(TaskError::ListArchived);
        }
        validate_recurrence_shape(kind, recurrence)?;
        Ok(Self {
            id,
            project_id: list.project_id,
            list_id: list.id,
            kind,
            payload,
            selected_value_snapshot,
            state: TaskState::Open,
            source_pretask_id: None,
            preset_assignment_id: None,
            copied_from_task_id: None,
            questionnaire_version_id: None,
            recurrence,
            created_at,
            payload_version: 1,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_pretask_snapshot(
        id: TaskId,
        list: &TaskList,
        kind: TaskKind,
        payload: EncryptedPayload,
        selected_value_snapshot: EncryptedPayload,
        source_pretask_id: PretaskId,
        preset_assignment_id: PresetAssignmentId,
        recurrence: Option<RecurrenceOccurrence>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, TaskError> {
        let mut task = Self::new(
            id,
            list,
            kind,
            payload,
            selected_value_snapshot,
            recurrence,
            created_at,
        )?;
        task.source_pretask_id = Some(source_pretask_id);
        task.preset_assignment_id = Some(preset_assignment_id);
        Ok(task)
    }

    #[must_use]
    pub fn id(&self) -> TaskId {
        self.id
    }

    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    #[must_use]
    pub fn list_id(&self) -> TaskListId {
        self.list_id
    }

    #[must_use]
    pub fn kind(&self) -> TaskKind {
        self.kind
    }

    #[must_use]
    pub fn payload(&self) -> &EncryptedPayload {
        &self.payload
    }

    #[must_use]
    pub fn selected_value_snapshot(&self) -> &EncryptedPayload {
        &self.selected_value_snapshot
    }

    #[must_use]
    pub fn state(&self) -> &TaskState {
        &self.state
    }

    #[must_use]
    pub fn source_pretask_id(&self) -> Option<PretaskId> {
        self.source_pretask_id
    }

    #[must_use]
    pub fn preset_assignment_id(&self) -> Option<PresetAssignmentId> {
        self.preset_assignment_id
    }

    #[must_use]
    pub fn copied_from_task_id(&self) -> Option<TaskId> {
        self.copied_from_task_id
    }

    #[must_use]
    pub fn questionnaire_version_id(&self) -> Option<QuestionnaireVersionId> {
        self.questionnaire_version_id
    }

    #[must_use]
    pub fn recurrence(&self) -> Option<RecurrenceOccurrence> {
        self.recurrence
    }

    #[must_use]
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    #[must_use]
    pub fn payload_version(&self) -> u64 {
        self.payload_version
    }

    #[must_use]
    pub fn is_completed(&self) -> bool {
        matches!(self.state, TaskState::Completed(_))
    }

    pub fn replace_encrypted_snapshot(
        &mut self,
        payload: EncryptedPayload,
        selected_value_snapshot: EncryptedPayload,
    ) -> Result<(), TaskError> {
        self.ensure_open()?;
        self.payload = payload;
        self.selected_value_snapshot = selected_value_snapshot;
        self.payload_version += 1;
        Ok(())
    }

    pub fn pin_questionnaire_version(
        &mut self,
        version: &QuestionnaireVersion,
    ) -> Result<(), TaskError> {
        self.ensure_open()?;
        if self.questionnaire_version_id.is_some() {
            return Err(TaskError::QuestionnaireVersionAlreadyPinned);
        }
        if version.project_id() != self.project_id {
            return Err(TaskError::ProjectMismatch);
        }
        if !version.is_published() {
            return Err(TaskError::QuestionnaireVersionNotPublished);
        }
        self.questionnaire_version_id = Some(version.id());
        Ok(())
    }

    pub fn complete(
        &mut self,
        assignment: &TaskAssignment,
        actor_id: UserId,
        completed_at: DateTime<Utc>,
    ) -> Result<(), TaskError> {
        self.ensure_open()?;
        if assignment.task_id != self.id {
            return Err(TaskError::AssignmentTaskMismatch);
        }
        if !assignment.is_active() || assignment.assignee_id != actor_id {
            return Err(TaskError::OnlyActiveAssigneeMayComplete);
        }
        if completed_at < self.created_at {
            return Err(TaskError::CompletionBeforeCreation);
        }
        self.state = TaskState::Completed(TaskCompletion {
            assignment_id: assignment.id,
            completed_by: actor_id,
            completed_at,
        });
        self.payload_version += 1;
        Ok(())
    }

    pub fn copy_completed_as_new(
        &self,
        new_id: TaskId,
        destination: &TaskList,
        payload: EncryptedPayload,
        selected_value_snapshot: EncryptedPayload,
        recurrence: Option<RecurrenceOccurrence>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, TaskError> {
        if !self.is_completed() {
            return Err(TaskError::CopyRequiresCompletedTask);
        }
        if destination.project_id != self.project_id {
            return Err(TaskError::ProjectMismatch);
        }
        if new_id == self.id {
            return Err(TaskError::CopyRequiresNewId);
        }
        let mut copy = Self::new(
            new_id,
            destination,
            self.kind,
            payload,
            selected_value_snapshot,
            recurrence,
            created_at,
        )?;
        copy.copied_from_task_id = Some(self.id);
        copy.questionnaire_version_id = self.questionnaire_version_id;
        Ok(copy)
    }

    pub fn ensure_assignable(&self) -> Result<(), TaskError> {
        self.ensure_open()
    }

    fn ensure_open(&self) -> Result<(), TaskError> {
        if self.is_completed() {
            Err(TaskError::CompletedTaskImmutable)
        } else {
            Ok(())
        }
    }
}

fn validate_recurrence_shape(
    kind: TaskKind,
    recurrence: Option<RecurrenceOccurrence>,
) -> Result<(), TaskError> {
    match (kind, recurrence) {
        (TaskKind::Recurring, Some(_)) | (TaskKind::Priority | TaskKind::Deadline, None) => Ok(()),
        _ => Err(TaskError::InvalidRecurrenceShape),
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum TaskError {
    #[error("task list is archived")]
    ListArchived,
    #[error("recurring tasks require a series and occurrence; other tasks forbid them")]
    InvalidRecurrenceShape,
    #[error("recurrence occurrence numbers begin at one")]
    ZeroOccurrenceNumber,
    #[error("completed tasks are immutable")]
    CompletedTaskImmutable,
    #[error("completion assignment belongs to another task")]
    AssignmentTaskMismatch,
    #[error("only the active assignee may complete a task")]
    OnlyActiveAssigneeMayComplete,
    #[error("completion cannot precede task creation")]
    CompletionBeforeCreation,
    #[error("only a completed task can be copied")]
    CopyRequiresCompletedTask,
    #[error("a copied task requires a new identifier")]
    CopyRequiresNewId,
    #[error("resources belong to different projects")]
    ProjectMismatch,
    #[error("task already pins a questionnaire version")]
    QuestionnaireVersionAlreadyPinned,
    #[error("tasks may pin only published questionnaire versions")]
    QuestionnaireVersionNotPublished,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(byte: u8) -> EncryptedPayload {
        EncryptedPayload::new(1, "test", "key", vec![byte], vec![byte]).unwrap()
    }

    fn list() -> TaskList {
        TaskList {
            id: TaskListId::new(),
            project_id: ProjectId::new(),
            payload: payload(1),
            payload_version: 1,
            created_at: Utc::now(),
            archived_at: None,
        }
    }

    #[test]
    fn llr_03_1_mixed_task_kinds_share_one_list() {
        let list = list();
        let priority = Task::new(
            TaskId::new(),
            &list,
            TaskKind::Priority,
            payload(2),
            payload(3),
            None,
            Utc::now(),
        );
        let deadline = Task::new(
            TaskId::new(),
            &list,
            TaskKind::Deadline,
            payload(4),
            payload(5),
            None,
            Utc::now(),
        );
        assert!(priority.is_ok());
        assert!(deadline.is_ok());
    }

    #[test]
    fn llr_03_4_only_active_assignee_completes_and_completion_freezes() {
        let now = Utc::now();
        let list = list();
        let actor = UserId::new();
        let mut task = Task::new(
            TaskId::new(),
            &list,
            TaskKind::Priority,
            payload(2),
            payload(3),
            None,
            now,
        )
        .unwrap();
        let assignment = TaskAssignment {
            id: TaskAssignmentId::new(),
            task_id: task.id(),
            assignee_id: actor,
            assigned_at: now,
            revoked_at: None,
        };
        assert_eq!(
            task.complete(&assignment, UserId::new(), now),
            Err(TaskError::OnlyActiveAssigneeMayComplete)
        );
        task.complete(&assignment, actor, now).unwrap();
        assert_eq!(
            task.replace_encrypted_snapshot(payload(4), payload(5)),
            Err(TaskError::CompletedTaskImmutable)
        );
        assert_eq!(
            task.ensure_assignable(),
            Err(TaskError::CompletedTaskImmutable)
        );
    }

    #[test]
    fn llr_03_4_copy_has_new_identity_and_is_open() {
        let now = Utc::now();
        let list = list();
        let actor = UserId::new();
        let mut task = Task::new(
            TaskId::new(),
            &list,
            TaskKind::Deadline,
            payload(2),
            payload(3),
            None,
            now,
        )
        .unwrap();
        let assignment = TaskAssignment {
            id: TaskAssignmentId::new(),
            task_id: task.id(),
            assignee_id: actor,
            assigned_at: now,
            revoked_at: None,
        };
        task.complete(&assignment, actor, now).unwrap();
        let copy = task
            .copy_completed_as_new(TaskId::new(), &list, payload(4), payload(5), None, now)
            .unwrap();
        assert!(!copy.is_completed());
        assert_eq!(copy.copied_from_task_id(), Some(task.id()));
    }

    #[test]
    fn llr_03_7_incomplete_recurring_task_keeps_server_identity_as_time_passes() {
        let created_at = Utc::now();
        let list = list();
        let occurrence =
            RecurrenceOccurrence::new(RecurrenceSeriesId::new(), 1).expect("valid occurrence");
        let task = Task::new(
            TaskId::new(),
            &list,
            TaskKind::Recurring,
            payload(6),
            payload(7),
            Some(occurrence),
            created_at,
        )
        .unwrap();

        let later = created_at + chrono::Duration::days(30);
        assert!(later > task.created_at());
        assert_eq!(task.recurrence(), Some(occurrence));
        assert!(!task.is_completed());
    }
}
