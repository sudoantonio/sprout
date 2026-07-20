use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AnswerId, DeviceId, EncryptedPayload, IdempotencyKey, ProjectId, QuestionId, QuestionOptionId,
    QuestionnaireId, QuestionnaireVersionId, SubmissionId, TaskAssignment, TaskId, UserId,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    Open,
    SingleChoice,
    MultipleChoice,
    Boolean,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestionnaireOption {
    pub id: QuestionOptionId,
    pub ordinal: u32,
    pub payload: EncryptedPayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestionnaireQuestion {
    pub id: QuestionId,
    pub kind: QuestionKind,
    pub ordinal: u32,
    pub required: bool,
    pub payload: EncryptedPayload,
    pub options: Vec<QuestionnaireOption>,
}

impl QuestionnaireQuestion {
    fn validate(&self) -> Result<(), QuestionnaireError> {
        match (self.kind, self.options.is_empty()) {
            (QuestionKind::SingleChoice | QuestionKind::MultipleChoice, false)
            | (QuestionKind::Open | QuestionKind::Boolean, true) => {}
            _ => return Err(QuestionnaireError::QuestionOptionShape),
        }
        let mut ids = HashSet::with_capacity(self.options.len());
        let mut ordinals = HashSet::with_capacity(self.options.len());
        if self
            .options
            .iter()
            .any(|option| !ids.insert(option.id) || !ordinals.insert(option.ordinal))
        {
            return Err(QuestionnaireError::DuplicateQuestionMetadata);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Questionnaire {
    pub id: QuestionnaireId,
    pub project_id: ProjectId,
    pub payload: EncryptedPayload,
    pub latest_version: u32,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl Questionnaire {
    #[must_use]
    pub fn new(
        id: QuestionnaireId,
        project_id: ProjectId,
        payload: EncryptedPayload,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            project_id,
            payload,
            latest_version: 0,
            created_at,
            archived_at: None,
        }
    }

    pub fn create_draft(
        &mut self,
        id: QuestionnaireVersionId,
        schema: EncryptedPayload,
        questions: Vec<QuestionnaireQuestion>,
        created_at: DateTime<Utc>,
    ) -> Result<QuestionnaireVersion, QuestionnaireError> {
        if self.archived_at.is_some() {
            return Err(QuestionnaireError::QuestionnaireArchived);
        }
        if created_at < self.created_at {
            return Err(QuestionnaireError::VersionBeforeQuestionnaire);
        }
        validate_questions(&questions)?;
        self.latest_version = self
            .latest_version
            .checked_add(1)
            .ok_or(QuestionnaireError::VersionOverflow)?;
        Ok(QuestionnaireVersion {
            id,
            questionnaire_id: self.id,
            project_id: self.project_id,
            number: self.latest_version,
            schema,
            questions,
            revision: 1,
            created_at,
            published_at: None,
        })
    }

    pub fn publish_version(
        &mut self,
        id: QuestionnaireVersionId,
        schema: EncryptedPayload,
        published_at: DateTime<Utc>,
    ) -> Result<QuestionnaireVersion, QuestionnaireError> {
        let mut version = self.create_draft(id, schema, Vec::new(), published_at)?;
        version.published_at = Some(published_at);
        Ok(version)
    }
}

fn validate_questions(questions: &[QuestionnaireQuestion]) -> Result<(), QuestionnaireError> {
    let mut ids = HashSet::with_capacity(questions.len());
    let mut ordinals = HashSet::with_capacity(questions.len());
    for question in questions {
        if !ids.insert(question.id) || !ordinals.insert(question.ordinal) {
            return Err(QuestionnaireError::DuplicateQuestionMetadata);
        }
        question.validate()?;
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestionnaireVersion {
    id: QuestionnaireVersionId,
    questionnaire_id: QuestionnaireId,
    project_id: ProjectId,
    number: u32,
    schema: EncryptedPayload,
    questions: Vec<QuestionnaireQuestion>,
    revision: u64,
    created_at: DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,
}

impl QuestionnaireVersion {
    #[must_use]
    pub fn id(&self) -> QuestionnaireVersionId {
        self.id
    }

    #[must_use]
    pub fn questionnaire_id(&self) -> QuestionnaireId {
        self.questionnaire_id
    }

    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }

    #[must_use]
    pub fn schema(&self) -> &EncryptedPayload {
        &self.schema
    }

    #[must_use]
    pub fn questions(&self) -> &[QuestionnaireQuestion] {
        &self.questions
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn is_published(&self) -> bool {
        self.published_at.is_some()
    }

    #[must_use]
    pub fn published_at(&self) -> Option<DateTime<Utc>> {
        self.published_at
    }

    pub fn replace_draft(
        &mut self,
        expected_revision: u64,
        schema: EncryptedPayload,
        questions: Vec<QuestionnaireQuestion>,
    ) -> Result<(), QuestionnaireError> {
        if self.is_published() {
            return Err(QuestionnaireError::PublishedVersionImmutable);
        }
        if expected_revision != self.revision {
            return Err(QuestionnaireError::RevisionConflict);
        }
        validate_questions(&questions)?;
        self.schema = schema;
        self.questions = questions;
        self.revision += 1;
        Ok(())
    }

    pub fn publish(
        &mut self,
        expected_revision: u64,
        published_at: DateTime<Utc>,
    ) -> Result<(), QuestionnaireError> {
        if self.is_published() {
            return Err(QuestionnaireError::PublishedVersionImmutable);
        }
        if expected_revision != self.revision {
            return Err(QuestionnaireError::RevisionConflict);
        }
        if published_at < self.created_at {
            return Err(QuestionnaireError::VersionBeforeQuestionnaire);
        }
        if self.questions.is_empty() {
            return Err(QuestionnaireError::EmptyVersion);
        }
        validate_questions(&self.questions)?;
        self.published_at = Some(published_at);
        self.revision += 1;
        Ok(())
    }

    pub fn validate_answer(&self, answer: &QuestionnaireAnswer) -> Result<(), QuestionnaireError> {
        let question = self
            .questions
            .iter()
            .find(|question| question.id == answer.question_id)
            .ok_or(QuestionnaireError::QuestionNotInVersion)?;
        let option_ids: HashSet<_> = question.options.iter().map(|option| option.id).collect();
        if answer
            .selected_option_ids
            .iter()
            .any(|option| !option_ids.contains(option))
        {
            return Err(QuestionnaireError::OptionNotInQuestion);
        }
        match question.kind {
            QuestionKind::Open | QuestionKind::Boolean if answer.selected_option_ids.is_empty() => {
            }
            QuestionKind::SingleChoice if answer.selected_option_ids.len() == 1 => {}
            QuestionKind::MultipleChoice if !answer.selected_option_ids.is_empty() => {}
            _ => return Err(QuestionnaireError::AnswerOptionShape),
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestionnaireAnswer {
    pub id: AnswerId,
    pub question_id: QuestionId,
    pub selected_option_ids: Vec<QuestionOptionId>,
    pub payload: EncryptedPayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestionnaireDraftSubmission {
    id: SubmissionId,
    task_id: TaskId,
    assignment: TaskAssignment,
    version_id: QuestionnaireVersionId,
    submitted_by: UserId,
    encrypted_payload: EncryptedPayload,
    answers: Vec<QuestionnaireAnswer>,
    revision: u64,
}

impl QuestionnaireDraftSubmission {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: SubmissionId,
        task_id: TaskId,
        assignment: TaskAssignment,
        version: &QuestionnaireVersion,
        submitted_by: UserId,
        encrypted_payload: EncryptedPayload,
        answers: Vec<QuestionnaireAnswer>,
    ) -> Result<Self, QuestionnaireError> {
        ensure_assignee_and_answers(task_id, &assignment, version, submitted_by, &answers)?;
        Ok(Self {
            id,
            task_id,
            assignment,
            version_id: version.id,
            submitted_by,
            encrypted_payload,
            answers,
            revision: 1,
        })
    }

    pub fn replace(
        &mut self,
        expected_revision: u64,
        version: &QuestionnaireVersion,
        actor: UserId,
        encrypted_payload: EncryptedPayload,
        answers: Vec<QuestionnaireAnswer>,
    ) -> Result<(), QuestionnaireError> {
        if expected_revision != self.revision {
            return Err(QuestionnaireError::RevisionConflict);
        }
        if version.id != self.version_id {
            return Err(QuestionnaireError::WrongVersion);
        }
        ensure_assignee_and_answers(self.task_id, &self.assignment, version, actor, &answers)?;
        self.encrypted_payload = encrypted_payload;
        self.answers = answers;
        self.revision += 1;
        Ok(())
    }

    #[must_use]
    pub fn id(&self) -> SubmissionId {
        self.id
    }

    #[must_use]
    pub fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        self,
        expected_revision: u64,
        version: &QuestionnaireVersion,
        actor: UserId,
        signer_device_id: DeviceId,
        signer_device_key_version: u32,
        classical_signature: Vec<u8>,
        post_quantum_signature: Vec<u8>,
        idempotency_key: IdempotencyKey,
        submitted_at: DateTime<Utc>,
    ) -> Result<QuestionnaireSubmission, QuestionnaireError> {
        if expected_revision != self.revision {
            return Err(QuestionnaireError::RevisionConflict);
        }
        ensure_assignee_and_answers(
            self.task_id,
            &self.assignment,
            version,
            actor,
            &self.answers,
        )?;
        ensure_required_answers(version, &self.answers)?;
        if signer_device_key_version == 0
            || classical_signature.len() != 64
            || post_quantum_signature.is_empty()
        {
            return Err(QuestionnaireError::InvalidDualSignature);
        }
        let published_at = version
            .published_at
            .ok_or(QuestionnaireError::VersionNotPublished)?;
        if submitted_at < published_at {
            return Err(QuestionnaireError::SubmissionBeforeVersion);
        }
        Ok(QuestionnaireSubmission {
            id: self.id,
            task_id: self.task_id,
            questionnaire_id: version.questionnaire_id,
            version_id: version.id,
            version_number: version.number,
            project_id: version.project_id,
            submitted_by: self.submitted_by,
            answers: self.encrypted_payload,
            signer_device_id,
            signer_device_key_version,
            classical_signature,
            post_quantum_signature,
            idempotency_key,
            submitted_at,
        })
    }
}

fn ensure_assignee_and_answers(
    task_id: TaskId,
    assignment: &TaskAssignment,
    version: &QuestionnaireVersion,
    actor: UserId,
    answers: &[QuestionnaireAnswer],
) -> Result<(), QuestionnaireError> {
    if !version.is_published() {
        return Err(QuestionnaireError::VersionNotPublished);
    }
    if assignment.task_id != task_id || !assignment.is_active() || assignment.assignee_id != actor {
        return Err(QuestionnaireError::OnlyActiveAssigneeMayDraft);
    }
    let mut questions = HashSet::with_capacity(answers.len());
    for answer in answers {
        if !questions.insert(answer.question_id) {
            return Err(QuestionnaireError::DuplicateAnswer);
        }
        version.validate_answer(answer)?;
    }
    Ok(())
}

fn ensure_required_answers(
    version: &QuestionnaireVersion,
    answers: &[QuestionnaireAnswer],
) -> Result<(), QuestionnaireError> {
    let answered: HashSet<_> = answers.iter().map(|answer| answer.question_id).collect();
    if version
        .questions
        .iter()
        .any(|question| question.required && !answered.contains(&question.id))
    {
        return Err(QuestionnaireError::RequiredAnswerMissing);
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestionnaireSubmission {
    id: SubmissionId,
    task_id: TaskId,
    questionnaire_id: QuestionnaireId,
    version_id: QuestionnaireVersionId,
    version_number: u32,
    project_id: ProjectId,
    submitted_by: UserId,
    answers: EncryptedPayload,
    signer_device_id: DeviceId,
    signer_device_key_version: u32,
    classical_signature: Vec<u8>,
    post_quantum_signature: Vec<u8>,
    idempotency_key: IdempotencyKey,
    submitted_at: DateTime<Utc>,
}

impl QuestionnaireSubmission {
    pub fn new(
        id: SubmissionId,
        version: &QuestionnaireVersion,
        submitted_by: UserId,
        answers: EncryptedPayload,
        submitted_at: DateTime<Utc>,
    ) -> Result<Self, QuestionnaireError> {
        let published_at = version
            .published_at
            .ok_or(QuestionnaireError::VersionNotPublished)?;
        if submitted_at < published_at {
            return Err(QuestionnaireError::SubmissionBeforeVersion);
        }
        Ok(Self {
            id,
            task_id: TaskId::new(),
            questionnaire_id: version.questionnaire_id,
            version_id: version.id,
            version_number: version.number,
            project_id: version.project_id,
            submitted_by,
            answers,
            signer_device_id: DeviceId::new(),
            signer_device_key_version: 1,
            classical_signature: vec![0; 64],
            post_quantum_signature: vec![0],
            idempotency_key: IdempotencyKey::new("legacy-constructor")
                .expect("static idempotency key is valid"),
            submitted_at,
        })
    }

    #[must_use]
    pub fn id(&self) -> SubmissionId {
        self.id
    }

    #[must_use]
    pub fn version_id(&self) -> QuestionnaireVersionId {
        self.version_id
    }

    #[must_use]
    pub fn version_number(&self) -> u32 {
        self.version_number
    }

    #[must_use]
    pub fn answers(&self) -> &EncryptedPayload {
        &self.answers
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum QuestionnaireError {
    #[error("questionnaire is archived")]
    QuestionnaireArchived,
    #[error("version cannot be created before questionnaire creation")]
    VersionBeforeQuestionnaire,
    #[error("questionnaire version number overflow")]
    VersionOverflow,
    #[error("published questionnaire versions are immutable")]
    PublishedVersionImmutable,
    #[error("questionnaire revision conflict")]
    RevisionConflict,
    #[error("a published questionnaire version requires at least one question")]
    EmptyVersion,
    #[error("question and option metadata contains duplicate identifiers or ordinals")]
    DuplicateQuestionMetadata,
    #[error("question options do not match the question kind")]
    QuestionOptionShape,
    #[error("question does not belong to the pinned version")]
    QuestionNotInVersion,
    #[error("option does not belong to the answered question")]
    OptionNotInQuestion,
    #[error("selected options do not match the question kind")]
    AnswerOptionShape,
    #[error("questionnaire version is not published")]
    VersionNotPublished,
    #[error("submission references another questionnaire version")]
    WrongVersion,
    #[error("only the active assignee may create or edit a draft submission")]
    OnlyActiveAssigneeMayDraft,
    #[error("a submission contains duplicate answers")]
    DuplicateAnswer,
    #[error("a required answer is missing")]
    RequiredAnswerMissing,
    #[error("submission requires classical and post-quantum device signatures")]
    InvalidDualSignature,
    #[error("submission cannot precede version publication")]
    SubmissionBeforeVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(byte: u8) -> EncryptedPayload {
        EncryptedPayload::new(1, "test", "key", vec![byte], vec![byte]).unwrap()
    }

    fn open_question(required: bool) -> QuestionnaireQuestion {
        QuestionnaireQuestion {
            id: QuestionId::new(),
            kind: QuestionKind::Open,
            ordinal: 0,
            required,
            payload: payload(2),
            options: Vec::new(),
        }
    }

    #[test]
    fn llr_04_1_published_versions_are_immutable_and_edits_use_a_new_version() {
        let now = Utc::now();
        let mut questionnaire =
            Questionnaire::new(QuestionnaireId::new(), ProjectId::new(), payload(1), now);
        let mut first = questionnaire
            .create_draft(
                QuestionnaireVersionId::new(),
                payload(2),
                vec![open_question(false)],
                now,
            )
            .unwrap();
        first.publish(1, now).unwrap();
        assert_eq!(
            first.replace_draft(2, payload(3), vec![open_question(false)]),
            Err(QuestionnaireError::PublishedVersionImmutable)
        );
        let second = questionnaire
            .create_draft(
                QuestionnaireVersionId::new(),
                payload(4),
                vec![open_question(false)],
                now,
            )
            .unwrap();
        assert_eq!(first.number(), 1);
        assert_eq!(second.number(), 2);
    }

    #[test]
    fn llr_04_2_and_04_3_enforce_question_and_option_membership() {
        let now = Utc::now();
        let option = QuestionnaireOption {
            id: QuestionOptionId::new(),
            ordinal: 0,
            payload: payload(3),
        };
        let question = QuestionnaireQuestion {
            id: QuestionId::new(),
            kind: QuestionKind::SingleChoice,
            ordinal: 0,
            required: true,
            payload: payload(2),
            options: vec![option.clone()],
        };
        let mut questionnaire =
            Questionnaire::new(QuestionnaireId::new(), ProjectId::new(), payload(1), now);
        let mut version = questionnaire
            .create_draft(
                QuestionnaireVersionId::new(),
                payload(4),
                vec![question.clone()],
                now,
            )
            .unwrap();
        version.publish(1, now).unwrap();
        assert!(
            version
                .validate_answer(&QuestionnaireAnswer {
                    id: AnswerId::new(),
                    question_id: question.id,
                    selected_option_ids: vec![option.id],
                    payload: payload(5),
                })
                .is_ok()
        );
        assert_eq!(
            version.validate_answer(&QuestionnaireAnswer {
                id: AnswerId::new(),
                question_id: question.id,
                selected_option_ids: vec![QuestionOptionId::new()],
                payload: payload(6),
            }),
            Err(QuestionnaireError::OptionNotInQuestion)
        );
    }

    #[test]
    fn llr_04_4_only_active_assignee_edits_and_final_submission_is_signed() {
        let now = Utc::now();
        let question = open_question(false);
        let mut questionnaire =
            Questionnaire::new(QuestionnaireId::new(), ProjectId::new(), payload(1), now);
        let mut version = questionnaire
            .create_draft(
                QuestionnaireVersionId::new(),
                payload(2),
                vec![question.clone()],
                now,
            )
            .unwrap();
        version.publish(1, now).unwrap();
        let task_id = TaskId::new();
        let assignee = UserId::new();
        let assignment = TaskAssignment {
            id: crate::TaskAssignmentId::new(),
            task_id,
            assignee_id: assignee,
            assigned_at: now,
            revoked_at: None,
        };
        let answer = QuestionnaireAnswer {
            id: AnswerId::new(),
            question_id: question.id,
            selected_option_ids: Vec::new(),
            payload: payload(3),
        };
        let mut draft = QuestionnaireDraftSubmission::new(
            SubmissionId::new(),
            task_id,
            assignment,
            &version,
            assignee,
            payload(4),
            vec![answer.clone()],
        )
        .unwrap();
        assert_eq!(
            draft.replace(1, &version, UserId::new(), payload(5), vec![answer.clone()],),
            Err(QuestionnaireError::OnlyActiveAssigneeMayDraft)
        );
        assert_eq!(
            draft.clone().submit(
                1,
                &version,
                assignee,
                DeviceId::new(),
                1,
                vec![0; 63],
                vec![1],
                IdempotencyKey::new("submit-1").unwrap(),
                now,
            ),
            Err(QuestionnaireError::InvalidDualSignature)
        );
        assert!(
            draft
                .submit(
                    1,
                    &version,
                    assignee,
                    DeviceId::new(),
                    1,
                    vec![0; 64],
                    vec![1],
                    IdempotencyKey::new("submit-1").unwrap(),
                    now,
                )
                .is_ok()
        );
    }
}
