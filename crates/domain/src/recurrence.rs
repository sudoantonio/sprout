use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    EncryptedPayload, ProjectId, RecurrenceOccurrence, RecurrenceSeriesId, Task, TaskError, TaskId,
    TaskKind, TaskList,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RecurrenceState {
    Active,
    Archived { archived_at: DateTime<Utc> },
}

/// The recurrence rule is client-calculated and remains encrypted. The server
/// only coordinates immutable, monotonically numbered occurrences.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecurrenceSeries {
    pub id: RecurrenceSeriesId,
    pub project_id: ProjectId,
    pub list_id: crate::TaskListId,
    pub encrypted_rule: EncryptedPayload,
    pub state: RecurrenceState,
    pub payload_version: u64,
    pub created_at: DateTime<Utc>,
}

impl RecurrenceSeries {
    #[must_use]
    pub fn new(
        id: RecurrenceSeriesId,
        project_id: ProjectId,
        list_id: crate::TaskListId,
        encrypted_rule: EncryptedPayload,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            project_id,
            list_id,
            encrypted_rule,
            state: RecurrenceState::Active,
            payload_version: 1,
            created_at,
        }
    }

    pub fn occurrence(
        &self,
        occurrence_number: u64,
    ) -> Result<RecurrenceOccurrence, RecurrenceError> {
        if !matches!(self.state, RecurrenceState::Active) {
            return Err(RecurrenceError::SeriesArchived);
        }
        RecurrenceOccurrence::new(self.id, occurrence_number).map_err(RecurrenceError::Task)
    }

    pub fn next_occurrence(
        &self,
        current: RecurrenceOccurrence,
    ) -> Result<RecurrenceOccurrence, RecurrenceError> {
        if current.series_id != self.id {
            return Err(RecurrenceError::SeriesMismatch);
        }
        let next = current
            .occurrence_number
            .checked_add(1)
            .ok_or(RecurrenceError::OccurrenceOverflow)?;
        self.occurrence(next)
    }

    pub fn archive(&mut self, archived_at: DateTime<Utc>) -> Result<(), RecurrenceError> {
        if !matches!(self.state, RecurrenceState::Active) {
            return Err(RecurrenceError::SeriesArchived);
        }
        self.state = RecurrenceState::Archived { archived_at };
        self.payload_version += 1;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NextOccurrenceSnapshot {
    pub task_id: TaskId,
    pub occurrence: RecurrenceOccurrence,
    pub payload: EncryptedPayload,
    pub selected_value_snapshot: EncryptedPayload,
}

impl NextOccurrenceSnapshot {
    pub fn into_task(
        self,
        series: &RecurrenceSeries,
        current_task: &Task,
        list: &TaskList,
        created_at: DateTime<Utc>,
    ) -> Result<Task, RecurrenceError> {
        if current_task.kind() != TaskKind::Recurring
            || current_task.recurrence().map(|value| value.series_id) != Some(series.id)
            || current_task.list_id() != list.id
            || series.list_id != list.id
        {
            return Err(RecurrenceError::SeriesMismatch);
        }
        let expected = series.next_occurrence(
            current_task
                .recurrence()
                .ok_or(RecurrenceError::SeriesMismatch)?,
        )?;
        if self.occurrence != expected {
            return Err(RecurrenceError::NonSequentialOccurrence);
        }
        Task::new(
            self.task_id,
            list,
            TaskKind::Recurring,
            self.payload,
            self.selected_value_snapshot,
            Some(self.occurrence),
            created_at,
        )
        .map_err(RecurrenceError::Task)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RecurrenceError {
    #[error("recurrence series is archived")]
    SeriesArchived,
    #[error("task and recurrence series do not match")]
    SeriesMismatch,
    #[error("next occurrence must increment exactly once")]
    NonSequentialOccurrence,
    #[error("recurrence occurrence number overflow")]
    OccurrenceOverflow,
    #[error(transparent)]
    Task(#[from] TaskError),
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{TaskListId, UserId};

    fn payload(byte: u8) -> EncryptedPayload {
        EncryptedPayload::new(1, "test", "key", vec![byte], vec![byte]).unwrap()
    }

    #[test]
    fn llr_03_5_client_supplied_next_occurrence_must_be_sequential() {
        let now = Utc::now();
        let project = ProjectId::new();
        let list = TaskList {
            id: TaskListId::new(),
            project_id: project,
            payload: payload(1),
            payload_version: 1,
            created_at: now,
            archived_at: None,
        };
        let series =
            RecurrenceSeries::new(RecurrenceSeriesId::new(), project, list.id, payload(2), now);
        let mut current = Task::new(
            TaskId::new(),
            &list,
            TaskKind::Recurring,
            payload(3),
            payload(4),
            Some(series.occurrence(1).unwrap()),
            now,
        )
        .unwrap();
        let assignment = crate::TaskAssignment {
            id: crate::TaskAssignmentId::new(),
            task_id: current.id(),
            assignee_id: UserId::new(),
            assigned_at: now,
            revoked_at: None,
        };
        current
            .complete(&assignment, assignment.assignee_id, now)
            .unwrap();
        let skipped = NextOccurrenceSnapshot {
            task_id: TaskId::new(),
            occurrence: series.occurrence(3).unwrap(),
            payload: payload(5),
            selected_value_snapshot: payload(6),
        };
        assert_eq!(
            skipped.into_task(&series, &current, &list, now),
            Err(RecurrenceError::NonSequentialOccurrence)
        );
    }

    #[test]
    fn llr_03_6_series_and_occurrence_number_form_a_unique_key() {
        let series = RecurrenceSeriesId::new();
        let occurrence = RecurrenceOccurrence::new(series, 2).unwrap();
        let mut concurrent_results = HashSet::new();
        assert!(concurrent_results.insert(occurrence));
        assert!(!concurrent_results.insert(occurrence));
        assert!(
            concurrent_results
                .insert(RecurrenceOccurrence::new(RecurrenceSeriesId::new(), 2).unwrap())
        );
    }
}
