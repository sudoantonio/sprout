use chrono::{DateTime, Utc};
use sprout_domain::{
    AttachmentMetadata, CompletedAttachmentId, IdempotencyKey, PresetAssignment,
    PresetAssignmentId, PresetVersion, PresetVersionId, PretaskTemplateAttachment,
    QuestionnaireDraftSubmission, QuestionnaireSubmission, QuestionnaireVersion,
    QuestionnaireVersionId, RecurrenceOccurrence, RecurrenceSeries, RecurrenceSeriesId,
    RequiredAttachmentId, SubmissionId, Task, TaskAssignment, TaskAssignmentId,
    TaskCompletedAttachment, TaskId, TaskList, TaskListId, TaskRequiredAttachment,
    TemplateAttachmentId,
};
use thiserror::Error;

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomicCompletion {
    pub completed: Task,
    pub next: Option<Task>,
    pub replayed: bool,
}

pub trait TaskRepository {
    fn task_list(&self, id: TaskListId) -> Result<Option<TaskList>, RepositoryError>;
    fn task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError>;
    fn assignment(&self, id: TaskAssignmentId) -> Result<Option<TaskAssignment>, RepositoryError>;
    fn task_for_occurrence(
        &self,
        occurrence: RecurrenceOccurrence,
    ) -> Result<Option<Task>, RepositoryError>;
    fn completion_by_idempotency(
        &self,
        idempotency_key: &IdempotencyKey,
    ) -> Result<Option<AtomicCompletion>, RepositoryError>;

    /// Inserts the complete batch atomically. Implementations must enforce
    /// unique task IDs and `(series_id, occurrence_number)` occurrence keys.
    fn insert_tasks(&mut self, tasks: Vec<Task>) -> Result<(), RepositoryError>;

    fn update_task(
        &mut self,
        task: Task,
        expected_payload_version: u64,
    ) -> Result<(), RepositoryError>;

    /// Completion, completion audit, optional next occurrence, and encrypted
    /// outbox event must commit in one transaction. Replaying the same key
    /// returns the first outcome.
    fn complete_atomically(
        &mut self,
        completed: Task,
        expected_payload_version: u64,
        next: Option<Task>,
        idempotency_key: &IdempotencyKey,
    ) -> Result<AtomicCompletion, RepositoryError>;
}

pub trait PresetRepository {
    fn preset_version(&self, id: PresetVersionId)
    -> Result<Option<PresetVersion>, RepositoryError>;
    fn preset_assignment(
        &self,
        id: PresetAssignmentId,
    ) -> Result<Option<PresetAssignment>, RepositoryError>;
}

pub trait RecurrenceRepository {
    fn recurrence_series(
        &self,
        id: RecurrenceSeriesId,
    ) -> Result<Option<RecurrenceSeries>, RepositoryError>;
}

pub trait QuestionnaireRepository {
    fn questionnaire_version(
        &self,
        id: QuestionnaireVersionId,
    ) -> Result<Option<QuestionnaireVersion>, RepositoryError>;
    fn draft_submission_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<Option<QuestionnaireDraftSubmission>, RepositoryError>;

    /// The version row and all question/option rows commit together.
    fn insert_version(&mut self, version: QuestionnaireVersion) -> Result<(), RepositoryError>;

    /// Draft payload and answer/option references are replaced atomically using
    /// optimistic revision matching.
    fn replace_draft_submission(
        &mut self,
        draft: QuestionnaireDraftSubmission,
        expected_revision: u64,
    ) -> Result<(), RepositoryError>;

    /// Finalization is one immutable write. Replaying the same idempotency key
    /// returns the original signed submission.
    fn submit_atomically(
        &mut self,
        draft_id: SubmissionId,
        expected_revision: u64,
        submission: QuestionnaireSubmission,
        idempotency_key: &IdempotencyKey,
    ) -> Result<QuestionnaireSubmission, RepositoryError>;
}

pub trait AttachmentRepository {
    /// Blob declaration, resource link, and template entity commit together.
    fn insert_template_attachment(
        &mut self,
        blob: AttachmentMetadata,
        attachment: PretaskTemplateAttachment,
    ) -> Result<TemplateAttachmentId, RepositoryError>;

    /// Materialized requirement snapshots retain template provenance while
    /// receiving a distinct blob and attachment identity.
    fn insert_required_attachment(
        &mut self,
        blob: AttachmentMetadata,
        attachment: TaskRequiredAttachment,
    ) -> Result<RequiredAttachmentId, RepositoryError>;

    /// Only an already-authorized active assignee may reach this repository
    /// boundary; the implementation must persist blob, link, and entity in one
    /// transaction.
    fn insert_completed_attachment(
        &mut self,
        blob: AttachmentMetadata,
        attachment: TaskCompletedAttachment,
    ) -> Result<CompletedAttachmentId, RepositoryError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RepositoryError {
    #[error("{0} was not found")]
    NotFound(&'static str),
    #[error("repository conflict: {0}")]
    Conflict(String),
    #[error("repository unavailable: {0}")]
    Unavailable(String),
}
