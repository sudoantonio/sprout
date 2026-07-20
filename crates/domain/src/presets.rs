use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    EncryptedPayload, PresetAssignmentId, PresetId, PresetVersionId, PretaskId, ProjectId,
    RecurrenceOccurrence, ResourceId, Task, TaskId, TaskKind, TaskList, TaskListId, UserId,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pretask {
    pub id: PretaskId,
    /// The only semantic field visible to the service.
    pub kind: TaskKind,
    pub payload: EncryptedPayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PresetVersion {
    pub id: PresetVersionId,
    pub preset_id: PresetId,
    pub project_id: ProjectId,
    pub version_number: u32,
    pub payload: EncryptedPayload,
    pub pretasks: Vec<Pretask>,
    pub created_at: DateTime<Utc>,
}

impl PresetVersion {
    pub fn new(
        id: PresetVersionId,
        preset_id: PresetId,
        project_id: ProjectId,
        version_number: u32,
        payload: EncryptedPayload,
        pretasks: Vec<Pretask>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, PresetError> {
        if version_number == 0 {
            return Err(PresetError::ZeroVersion);
        }
        if pretasks.is_empty() {
            return Err(PresetError::EmptyPreset);
        }
        let mut ids = HashSet::with_capacity(pretasks.len());
        for pretask in &pretasks {
            if !ids.insert(pretask.id) {
                return Err(PresetError::DuplicatePretask(pretask.id));
            }
        }
        Ok(Self {
            id,
            preset_id,
            project_id,
            version_number,
            payload,
            pretasks,
            created_at,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PresetAssignment {
    pub id: PresetAssignmentId,
    pub preset_version_id: PresetVersionId,
    pub destination_list_id: TaskListId,
    pub assigned_to: UserId,
    pub assigned_by: UserId,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PretaskSelection {
    pub pretask_id: PretaskId,
    pub kind: TaskKind,
    /// A separately encrypted user selection, never interpreted by the server.
    pub selected_value: EncryptedPayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MaterializationChoice {
    pub pretask_id: PretaskId,
    pub kind: TaskKind,
    pub task_id: TaskId,
    pub task_resource_id: ResourceId,
    /// The immutable encrypted value fixed onto the concrete task.
    pub selected_value_snapshot: EncryptedPayload,
    /// The immutable encrypted concrete task body.
    pub task_snapshot: EncryptedPayload,
    pub recurrence: Option<RecurrenceOccurrence>,
    /// Indicates that all active assignee devices have a key envelope for
    /// this concrete task resource. Persistence computes this value from the
    /// supplied envelope set; it is not derived from ciphertext.
    pub envelope_coverage_complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresetAssignmentPlan {
    selections: Vec<PretaskSelection>,
}

impl PresetAssignmentPlan {
    pub fn new(
        version: &PresetVersion,
        selections: Vec<PretaskSelection>,
    ) -> Result<Self, PresetError> {
        validate_exact_kinds(
            version,
            selections
                .iter()
                .map(|selection| (selection.pretask_id, selection.kind)),
        )?;
        Ok(Self { selections })
    }

    #[must_use]
    pub fn selections(&self) -> &[PretaskSelection] {
        &self.selections
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationPlan {
    choices: Vec<MaterializationChoice>,
}

impl MaterializationPlan {
    pub fn new(
        version: &PresetVersion,
        choices: Vec<MaterializationChoice>,
    ) -> Result<Self, PresetError> {
        validate_exact_kinds(
            version,
            choices
                .iter()
                .map(|choice| (choice.pretask_id, choice.kind)),
        )?;
        if let Some(choice) = choices
            .iter()
            .find(|choice| !choice.envelope_coverage_complete)
        {
            return Err(PresetError::MissingEnvelopeCoverage(choice.pretask_id));
        }
        let task_ids = choices
            .iter()
            .map(|choice| choice.task_id)
            .collect::<HashSet<_>>();
        let resource_ids = choices
            .iter()
            .map(|choice| choice.task_resource_id)
            .collect::<HashSet<_>>();
        if task_ids.len() != choices.len() {
            return Err(PresetError::DuplicateTaskId);
        }
        if resource_ids.len() != choices.len() {
            return Err(PresetError::DuplicateResourceId);
        }
        Ok(Self { choices })
    }

    #[must_use]
    pub fn choices(&self) -> &[MaterializationChoice] {
        &self.choices
    }

    pub fn materialize(
        &self,
        version: &PresetVersion,
        assignment: &PresetAssignment,
        destination: &TaskList,
        created_at: DateTime<Utc>,
    ) -> Result<Vec<Task>, PresetError> {
        if version.id != assignment.preset_version_id
            || destination.id != assignment.destination_list_id
            || version.project_id != destination.project_id
        {
            return Err(PresetError::AssignmentTargetMismatch);
        }
        self.choices
            .iter()
            .map(|choice| {
                Task::from_pretask_snapshot(
                    choice.task_id,
                    destination,
                    choice.kind,
                    choice.task_snapshot.clone(),
                    choice.selected_value_snapshot.clone(),
                    choice.pretask_id,
                    assignment.id,
                    choice.recurrence,
                    created_at,
                )
                .map_err(PresetError::Task)
            })
            .collect()
    }
}

fn validate_exact_kinds(
    version: &PresetVersion,
    supplied: impl Iterator<Item = (PretaskId, TaskKind)>,
) -> Result<(), PresetError> {
    let expected = version
        .pretasks
        .iter()
        .map(|pretask| (pretask.id, pretask.kind))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::with_capacity(expected.len());
    for (pretask_id, kind) in supplied {
        let expected_kind = expected
            .get(&pretask_id)
            .ok_or(PresetError::UnknownPretask(pretask_id))?;
        if !seen.insert(pretask_id) {
            return Err(PresetError::DuplicateChoice(pretask_id));
        }
        if *expected_kind != kind {
            return Err(PresetError::IncompatibleChoice {
                pretask_id,
                expected: *expected_kind,
                actual: kind,
            });
        }
    }
    if seen.len() != expected.len() {
        let missing = expected
            .keys()
            .filter(|id| !seen.contains(id))
            .copied()
            .collect();
        return Err(PresetError::MissingChoices(missing));
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PresetError {
    #[error("preset version numbers begin at one")]
    ZeroVersion,
    #[error("preset versions must contain at least one pretask")]
    EmptyPreset,
    #[error("pretask id {0} appears more than once")]
    DuplicatePretask(PretaskId),
    #[error("choice references unknown pretask {0}")]
    UnknownPretask(PretaskId),
    #[error("pretask {0} has more than one choice")]
    DuplicateChoice(PretaskId),
    #[error("choices are missing for pretasks {0:?}")]
    MissingChoices(Vec<PretaskId>),
    #[error("pretask {pretask_id} requires {expected:?}, not {actual:?}")]
    IncompatibleChoice {
        pretask_id: PretaskId,
        expected: TaskKind,
        actual: TaskKind,
    },
    #[error("pretask {0} lacks complete key-envelope coverage")]
    MissingEnvelopeCoverage(PretaskId),
    #[error("materialization reuses a task identifier")]
    DuplicateTaskId,
    #[error("materialization reuses a task resource identifier")]
    DuplicateResourceId,
    #[error("preset assignment, version, and destination do not match")]
    AssignmentTargetMismatch,
    #[error(transparent)]
    Task(#[from] crate::TaskError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecurrenceSeriesId;

    fn payload(byte: u8) -> EncryptedPayload {
        EncryptedPayload::new(1, "test", "key", vec![byte], vec![byte]).unwrap()
    }

    fn version() -> PresetVersion {
        PresetVersion::new(
            PresetVersionId::new(),
            PresetId::new(),
            ProjectId::new(),
            1,
            payload(1),
            vec![
                Pretask {
                    id: PretaskId::new(),
                    kind: TaskKind::Priority,
                    payload: payload(2),
                },
                Pretask {
                    id: PretaskId::new(),
                    kind: TaskKind::Deadline,
                    payload: payload(3),
                },
                Pretask {
                    id: PretaskId::new(),
                    kind: TaskKind::Recurring,
                    payload: payload(4),
                },
            ],
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn llr_03_2_missing_and_incompatible_per_pretask_values_fail() {
        let version = version();
        let missing = vec![PretaskSelection {
            pretask_id: version.pretasks[0].id,
            kind: TaskKind::Priority,
            selected_value: payload(5),
        }];
        assert!(matches!(
            PresetAssignmentPlan::new(&version, missing),
            Err(PresetError::MissingChoices(_))
        ));

        let incompatible = version
            .pretasks
            .iter()
            .map(|pretask| PretaskSelection {
                pretask_id: pretask.id,
                kind: if pretask.kind == TaskKind::Deadline {
                    TaskKind::Priority
                } else {
                    pretask.kind
                },
                selected_value: payload(6),
            })
            .collect();
        assert!(matches!(
            PresetAssignmentPlan::new(&version, incompatible),
            Err(PresetError::IncompatibleChoice { .. })
        ));
    }

    #[test]
    fn llr_03_2_every_pretask_requires_envelope_coverage() {
        let version = version();
        let choices = version
            .pretasks
            .iter()
            .map(|pretask| MaterializationChoice {
                pretask_id: pretask.id,
                kind: pretask.kind,
                task_id: TaskId::new(),
                task_resource_id: ResourceId::new(),
                selected_value_snapshot: payload(7),
                task_snapshot: payload(8),
                recurrence: (pretask.kind == TaskKind::Recurring)
                    .then(|| RecurrenceOccurrence::new(RecurrenceSeriesId::new(), 1).unwrap()),
                envelope_coverage_complete: pretask.kind != TaskKind::Deadline,
            })
            .collect();
        assert!(matches!(
            MaterializationPlan::new(&version, choices),
            Err(PresetError::MissingEnvelopeCoverage(_))
        ));
    }

    #[test]
    fn llr_03_3_materialized_tasks_keep_immutable_source_snapshots() {
        let mut version = version();
        let assignment = PresetAssignment {
            id: PresetAssignmentId::new(),
            preset_version_id: version.id,
            destination_list_id: TaskListId::new(),
            assigned_to: UserId::new(),
            assigned_by: UserId::new(),
            created_at: Utc::now(),
        };
        let list = TaskList {
            id: assignment.destination_list_id,
            project_id: version.project_id,
            payload: payload(9),
            payload_version: 1,
            created_at: Utc::now(),
            archived_at: None,
        };
        let first_snapshot = payload(10);
        let choices = version
            .pretasks
            .iter()
            .enumerate()
            .map(|(index, pretask)| MaterializationChoice {
                pretask_id: pretask.id,
                kind: pretask.kind,
                task_id: TaskId::new(),
                task_resource_id: ResourceId::new(),
                selected_value_snapshot: payload(11 + index as u8),
                task_snapshot: if index == 0 {
                    first_snapshot.clone()
                } else {
                    payload(20 + index as u8)
                },
                recurrence: (pretask.kind == TaskKind::Recurring)
                    .then(|| RecurrenceOccurrence::new(RecurrenceSeriesId::new(), 1).unwrap()),
                envelope_coverage_complete: true,
            })
            .collect();
        let plan = MaterializationPlan::new(&version, choices).unwrap();
        let tasks = plan
            .materialize(&version, &assignment, &list, Utc::now())
            .unwrap();

        version.pretasks[0].payload = payload(99);
        assert_eq!(tasks[0].payload(), &first_snapshot);
        assert_eq!(tasks[0].source_pretask_id(), Some(version.pretasks[0].id));
        assert_eq!(tasks[0].preset_assignment_id(), Some(assignment.id));
    }
}
