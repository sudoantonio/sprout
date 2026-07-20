use sprout_domain::{
    EncryptedPayload, IdempotencyKey, MaterializationChoice, MaterializationPlan,
    NextOccurrenceSnapshot, PresetAssignmentId, PresetError, RecurrenceError, RecurrenceOccurrence,
    RecurrenceSeriesId, Task, TaskAssignmentId, TaskError, TaskId, TaskKind, TaskListId, UserId,
};
use thiserror::Error;

use crate::{
    AtomicCompletion, Clock, PresetRepository, RecurrenceRepository, RepositoryError,
    TaskRepository,
};

pub struct TaskService<'a, R, C> {
    repository: &'a mut R,
    clock: &'a C,
}

impl<'a, R: TaskRepository, C: Clock> TaskService<'a, R, C> {
    #[must_use]
    pub fn new(repository: &'a mut R, clock: &'a C) -> Self {
        Self { repository, clock }
    }

    pub fn create(
        &mut self,
        list_id: TaskListId,
        task_id: TaskId,
        kind: TaskKind,
        payload: EncryptedPayload,
        selected_value_snapshot: EncryptedPayload,
        recurrence: Option<RecurrenceOccurrence>,
    ) -> Result<Task, ServiceError> {
        let list = self
            .repository
            .task_list(list_id)?
            .ok_or(ServiceError::NotFound("task list"))?;
        let task = Task::new(
            task_id,
            &list,
            kind,
            payload,
            selected_value_snapshot,
            recurrence,
            self.clock.now(),
        )?;
        self.repository.insert_tasks(vec![task.clone()])?;
        Ok(task)
    }

    pub fn complete(
        &mut self,
        task_id: TaskId,
        assignment_id: TaskAssignmentId,
        actor_id: UserId,
        idempotency_key: &IdempotencyKey,
        next: Option<NextOccurrenceSnapshot>,
        recurrence_series: Option<RecurrenceSeriesId>,
    ) -> Result<AtomicCompletion, ServiceError>
    where
        R: RecurrenceRepository,
    {
        if let Some(mut replay) = self.repository.completion_by_idempotency(idempotency_key)? {
            replay.replayed = true;
            return Ok(replay);
        }
        let mut task = self
            .repository
            .task(task_id)?
            .ok_or(ServiceError::NotFound("task"))?;
        let assignment = self
            .repository
            .assignment(assignment_id)?
            .ok_or(ServiceError::NotFound("task assignment"))?;
        let expected_payload_version = task.payload_version();
        task.complete(&assignment, actor_id, self.clock.now())?;

        let next_task = match (task.recurrence(), next, recurrence_series) {
            (None, None, None) => None,
            (Some(current), Some(next), Some(series_id)) if current.series_id == series_id => {
                let series = self
                    .repository
                    .recurrence_series(series_id)?
                    .ok_or(ServiceError::NotFound("recurrence series"))?;
                let list = self
                    .repository
                    .task_list(task.list_id())?
                    .ok_or(ServiceError::NotFound("task list"))?;
                Some(next.into_task(&series, &task, &list, self.clock.now())?)
            }
            _ => return Err(ServiceError::InvalidRecurrenceCompletion),
        };

        self.repository
            .complete_atomically(task, expected_payload_version, next_task, idempotency_key)
            .map_err(Into::into)
    }

    pub fn copy_completed(
        &mut self,
        source_task_id: TaskId,
        destination_list_id: TaskListId,
        new_task_id: TaskId,
        payload: EncryptedPayload,
        selected_value_snapshot: EncryptedPayload,
        recurrence: Option<RecurrenceOccurrence>,
    ) -> Result<Task, ServiceError> {
        let source = self
            .repository
            .task(source_task_id)?
            .ok_or(ServiceError::NotFound("task"))?;
        let destination = self
            .repository
            .task_list(destination_list_id)?
            .ok_or(ServiceError::NotFound("task list"))?;
        let copy = source.copy_completed_as_new(
            new_task_id,
            &destination,
            payload,
            selected_value_snapshot,
            recurrence,
            self.clock.now(),
        )?;
        self.repository.insert_tasks(vec![copy.clone()])?;
        Ok(copy)
    }
}

pub struct PresetService<'a, T, P, C> {
    tasks: &'a mut T,
    presets: &'a P,
    clock: &'a C,
}

impl<'a, T: TaskRepository, P: PresetRepository, C: Clock> PresetService<'a, T, P, C> {
    #[must_use]
    pub fn new(tasks: &'a mut T, presets: &'a P, clock: &'a C) -> Self {
        Self {
            tasks,
            presets,
            clock,
        }
    }

    pub fn materialize(
        &mut self,
        assignment_id: PresetAssignmentId,
        choices: Vec<MaterializationChoice>,
    ) -> Result<Vec<Task>, ServiceError> {
        let assignment = self
            .presets
            .preset_assignment(assignment_id)?
            .ok_or(ServiceError::NotFound("preset assignment"))?;
        let version = self
            .presets
            .preset_version(assignment.preset_version_id)?
            .ok_or(ServiceError::NotFound("preset version"))?;
        let destination = self
            .tasks
            .task_list(assignment.destination_list_id)?
            .ok_or(ServiceError::NotFound("task list"))?;
        let plan = MaterializationPlan::new(&version, choices)?;
        let tasks = plan.materialize(&version, &assignment, &destination, self.clock.now())?;
        self.tasks.insert_tasks(tasks.clone())?;
        Ok(tasks)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ServiceError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Task(#[from] TaskError),
    #[error(transparent)]
    Preset(#[from] PresetError),
    #[error(transparent)]
    Recurrence(#[from] RecurrenceError),
    #[error("{0} was not found")]
    NotFound(&'static str),
    #[error("recurring completion requires exactly one sequential client snapshot")]
    InvalidRecurrenceCompletion,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{DateTime, Utc};
    use sprout_domain::{
        PresetAssignment, PresetAssignmentPlan, PresetId, PresetVersion, PresetVersionId, Pretask,
        PretaskId, PretaskSelection, ProjectId, RecurrenceSeries, TaskAssignment, TaskAssignmentId,
        TaskList,
    };

    use super::*;

    #[derive(Clone)]
    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    #[derive(Default)]
    struct MemoryRepository {
        lists: HashMap<TaskListId, TaskList>,
        tasks: HashMap<TaskId, Task>,
        assignments: HashMap<TaskAssignmentId, TaskAssignment>,
        occurrences: HashMap<RecurrenceOccurrence, TaskId>,
        series: HashMap<RecurrenceSeriesId, RecurrenceSeries>,
        completions: HashMap<String, AtomicCompletion>,
    }

    impl TaskRepository for MemoryRepository {
        fn task_list(&self, id: TaskListId) -> Result<Option<TaskList>, RepositoryError> {
            Ok(self.lists.get(&id).cloned())
        }

        fn task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError> {
            Ok(self.tasks.get(&id).cloned())
        }

        fn assignment(
            &self,
            id: TaskAssignmentId,
        ) -> Result<Option<TaskAssignment>, RepositoryError> {
            Ok(self.assignments.get(&id).cloned())
        }

        fn task_for_occurrence(
            &self,
            occurrence: RecurrenceOccurrence,
        ) -> Result<Option<Task>, RepositoryError> {
            Ok(self
                .occurrences
                .get(&occurrence)
                .and_then(|id| self.tasks.get(id))
                .cloned())
        }

        fn completion_by_idempotency(
            &self,
            idempotency_key: &IdempotencyKey,
        ) -> Result<Option<AtomicCompletion>, RepositoryError> {
            Ok(self.completions.get(idempotency_key.as_str()).cloned())
        }

        fn insert_tasks(&mut self, tasks: Vec<Task>) -> Result<(), RepositoryError> {
            for task in &tasks {
                if self.tasks.contains_key(&task.id())
                    || task
                        .recurrence()
                        .is_some_and(|occurrence| self.occurrences.contains_key(&occurrence))
                {
                    return Err(RepositoryError::Conflict("duplicate task".into()));
                }
            }
            for task in tasks {
                if let Some(occurrence) = task.recurrence() {
                    self.occurrences.insert(occurrence, task.id());
                }
                self.tasks.insert(task.id(), task);
            }
            Ok(())
        }

        fn update_task(
            &mut self,
            task: Task,
            expected_payload_version: u64,
        ) -> Result<(), RepositoryError> {
            if self
                .tasks
                .get(&task.id())
                .is_none_or(|stored| stored.payload_version() != expected_payload_version)
            {
                return Err(RepositoryError::Conflict("payload version".into()));
            }
            self.tasks.insert(task.id(), task);
            Ok(())
        }

        fn complete_atomically(
            &mut self,
            completed: Task,
            expected_payload_version: u64,
            next: Option<Task>,
            idempotency_key: &IdempotencyKey,
        ) -> Result<AtomicCompletion, RepositoryError> {
            if let Some(existing) = self.completions.get(idempotency_key.as_str()) {
                let mut replay = existing.clone();
                replay.replayed = true;
                return Ok(replay);
            }
            if self
                .tasks
                .get(&completed.id())
                .is_none_or(|stored| stored.payload_version() != expected_payload_version)
            {
                return Err(RepositoryError::Conflict("payload version".into()));
            }
            if let Some(next) = &next {
                if next
                    .recurrence()
                    .is_some_and(|occurrence| self.occurrences.contains_key(&occurrence))
                {
                    return Err(RepositoryError::Conflict("duplicate occurrence".into()));
                }
            }
            self.tasks.insert(completed.id(), completed.clone());
            if let Some(next) = &next {
                if let Some(occurrence) = next.recurrence() {
                    self.occurrences.insert(occurrence, next.id());
                }
                self.tasks.insert(next.id(), next.clone());
            }
            let outcome = AtomicCompletion {
                completed,
                next,
                replayed: false,
            };
            self.completions
                .insert(idempotency_key.as_str().into(), outcome.clone());
            Ok(outcome)
        }
    }

    impl RecurrenceRepository for MemoryRepository {
        fn recurrence_series(
            &self,
            id: RecurrenceSeriesId,
        ) -> Result<Option<RecurrenceSeries>, RepositoryError> {
            Ok(self.series.get(&id).cloned())
        }
    }

    fn payload(byte: u8) -> EncryptedPayload {
        EncryptedPayload::new(1, "test", "key", vec![byte], vec![byte]).unwrap()
    }

    #[test]
    fn llr_03_5_and_03_6_recurring_completion_is_atomic_and_idempotent() {
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
        let current = Task::new(
            TaskId::new(),
            &list,
            TaskKind::Recurring,
            payload(3),
            payload(4),
            Some(series.occurrence(1).unwrap()),
            now,
        )
        .unwrap();
        let actor = UserId::new();
        let assignment = TaskAssignment {
            id: TaskAssignmentId::new(),
            task_id: current.id(),
            assignee_id: actor,
            assigned_at: now,
            revoked_at: None,
        };
        let mut repository = MemoryRepository::default();
        repository.lists.insert(list.id, list);
        repository.series.insert(series.id, series.clone());
        repository.tasks.insert(current.id(), current.clone());
        repository
            .assignments
            .insert(assignment.id, assignment.clone());
        let clock = FixedClock(now);
        let key = IdempotencyKey::new("completion-1").unwrap();
        let next = NextOccurrenceSnapshot {
            task_id: TaskId::new(),
            occurrence: series.occurrence(2).unwrap(),
            payload: payload(5),
            selected_value_snapshot: payload(6),
        };
        let first = TaskService::new(&mut repository, &clock)
            .complete(
                current.id(),
                assignment.id,
                actor,
                &key,
                Some(next.clone()),
                Some(series.id),
            )
            .unwrap();
        let second = TaskService::new(&mut repository, &clock)
            .complete(
                current.id(),
                assignment.id,
                actor,
                &key,
                Some(next),
                Some(series.id),
            )
            .unwrap();
        assert!(!first.replayed);
        assert!(second.replayed);
        assert_eq!(
            first.next.as_ref().map(Task::id),
            second.next.as_ref().map(Task::id)
        );
    }

    #[test]
    fn llr_03_2_preset_assignments_validate_all_kinds_before_writing() {
        let now = Utc::now();
        let version = PresetVersion::new(
            PresetVersionId::new(),
            PresetId::new(),
            ProjectId::new(),
            1,
            payload(1),
            vec![Pretask {
                id: PretaskId::new(),
                kind: TaskKind::Priority,
                payload: payload(2),
            }],
            now,
        )
        .unwrap();
        let plan = PresetAssignmentPlan::new(
            &version,
            vec![PretaskSelection {
                pretask_id: version.pretasks[0].id,
                kind: TaskKind::Priority,
                selected_value: payload(3),
            }],
        );
        assert!(plan.is_ok());
        let _assignment = PresetAssignment {
            id: PresetAssignmentId::new(),
            preset_version_id: version.id,
            destination_list_id: TaskListId::new(),
            assigned_to: UserId::new(),
            assigned_by: UserId::new(),
            created_at: now,
        };
    }
}
