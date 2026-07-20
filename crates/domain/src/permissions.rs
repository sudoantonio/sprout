use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{GrantId, ProjectId, ResourceId, UserId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Project,
    Topic,
    TaskList,
    Task,
    Preset,
    RecurrenceSeries,
    Questionnaire,
    QuestionnaireVersion,
    Attachment,
}

impl ResourceKind {
    #[must_use]
    pub fn accepts_parent(self, parent: Self) -> bool {
        match self {
            Self::Project => false,
            Self::Topic | Self::Preset | Self::Questionnaire => parent == Self::Project,
            Self::TaskList => matches!(parent, Self::Project | Self::Topic),
            Self::Task => matches!(parent, Self::TaskList | Self::RecurrenceSeries),
            Self::RecurrenceSeries => parent == Self::TaskList,
            Self::QuestionnaireVersion => parent == Self::Questionnaire,
            Self::Attachment => !matches!(parent, Self::Attachment),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceNode {
    pub id: ResourceId,
    pub project_id: ProjectId,
    pub kind: ResourceKind,
    pub parent_id: Option<ResourceId>,
}

impl ResourceNode {
    pub fn new(
        id: ResourceId,
        project_id: ProjectId,
        kind: ResourceKind,
        parent: Option<&ResourceNode>,
    ) -> Result<Self, ResourceHierarchyError> {
        match (kind, parent) {
            (ResourceKind::Project, None) => {}
            (ResourceKind::Project, Some(_)) => {
                return Err(ResourceHierarchyError::ProjectCannotHaveParent);
            }
            (_, None) => return Err(ResourceHierarchyError::ParentRequired),
            (child, Some(parent)) => {
                if parent.project_id != project_id {
                    return Err(ResourceHierarchyError::CrossProjectParent);
                }
                if !child.accepts_parent(parent.kind) {
                    return Err(ResourceHierarchyError::InvalidParentKind {
                        child,
                        parent: parent.kind,
                    });
                }
            }
        }
        Ok(Self {
            id,
            project_id,
            kind,
            parent_id: parent.map(|node| node.id),
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessScope {
    /// Access to the resource and all descendants.
    Full,
    /// Access only to this resource's header, never its body or descendants.
    ContainerOnly,
}

pub type PermissionLevel = AccessScope;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GrantOrigin {
    Direct,
    Assignment {
        assignment_id: uuid::Uuid,
    },
    Inherited {
        root_grant_id: GrantId,
        root_resource_id: ResourceId,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PermissionGrant {
    pub id: GrantId,
    pub root_grant_id: GrantId,
    pub user_id: UserId,
    pub resource_id: ResourceId,
    pub access_scope: AccessScope,
    pub origin: GrantOrigin,
    pub granted_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl PermissionGrant {
    #[must_use]
    pub fn direct(
        id: GrantId,
        user_id: UserId,
        resource_id: ResourceId,
        level: PermissionLevel,
        granted_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            root_grant_id: id,
            user_id,
            resource_id,
            access_scope: level,
            origin: GrantOrigin::Direct,
            granted_at,
            revoked_at: None,
        }
    }

    #[must_use]
    pub fn assignment(
        id: GrantId,
        assignment_id: uuid::Uuid,
        user_id: UserId,
        resource_id: ResourceId,
        access_scope: AccessScope,
        granted_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            root_grant_id: id,
            user_id,
            resource_id,
            access_scope,
            origin: GrantOrigin::Assignment { assignment_id },
            granted_at,
            revoked_at: None,
        }
    }

    pub fn inherited(
        id: GrantId,
        resource_id: ResourceId,
        source: &Self,
        granted_at: DateTime<Utc>,
    ) -> Result<Self, PermissionError> {
        Self::materialized(id, resource_id, AccessScope::Full, source, granted_at)
    }

    pub fn materialized(
        id: GrantId,
        resource_id: ResourceId,
        access_scope: AccessScope,
        source: &Self,
        granted_at: DateTime<Utc>,
    ) -> Result<Self, PermissionError> {
        if source.access_scope != AccessScope::Full {
            return Err(PermissionError::ContainerOnlyCannotBeInherited);
        }
        if source.revoked_at.is_some() {
            return Err(PermissionError::SourceGrantRevoked);
        }
        let (root_grant_id, root_resource_id) = match source.origin {
            GrantOrigin::Direct | GrantOrigin::Assignment { .. } => (source.id, source.resource_id),
            GrantOrigin::Inherited {
                root_grant_id,
                root_resource_id,
            } => (root_grant_id, root_resource_id),
        };
        Ok(Self {
            id,
            root_grant_id,
            user_id: source.user_id,
            resource_id,
            access_scope,
            origin: GrantOrigin::Inherited {
                root_grant_id,
                root_resource_id,
            },
            granted_at,
            revoked_at: None,
        })
    }

    #[must_use]
    pub fn is_active_at(&self, at: DateTime<Utc>) -> bool {
        self.granted_at <= at && self.revoked_at.is_none_or(|revoked| revoked > at)
    }

    pub fn revoke(&mut self, at: DateTime<Utc>) -> Result<(), PermissionError> {
        if at < self.granted_at {
            return Err(PermissionError::RevocationBeforeGrant);
        }
        if self.revoked_at.is_some() {
            return Err(PermissionError::AlreadyRevoked);
        }
        self.revoked_at = Some(at);
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResourceHierarchyError {
    #[error("project root cannot have a parent")]
    ProjectCannotHaveParent,
    #[error("non-project resources require a parent")]
    ParentRequired,
    #[error("parent belongs to another project")]
    CrossProjectParent,
    #[error("{child:?} cannot be parented by {parent:?}")]
    InvalidParentKind {
        child: ResourceKind,
        parent: ResourceKind,
    },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PermissionError {
    #[error("container-only grants cannot be inherited")]
    ContainerOnlyCannotBeInherited,
    #[error("cannot inherit from a revoked grant")]
    SourceGrantRevoked,
    #[error("revocation cannot precede the grant")]
    RevocationBeforeGrant,
    #[error("grant is already revoked")]
    AlreadyRevoked,
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn root(project_id: ProjectId) -> ResourceNode {
        ResourceNode::new(ResourceId::new(), project_id, ResourceKind::Project, None).unwrap()
    }

    #[test]
    fn hierarchy_rejects_cross_project_and_invalid_parent_types() {
        let project = ProjectId::new();
        let other = ProjectId::new();
        let root = root(project);
        assert_eq!(
            ResourceNode::new(
                ResourceId::new(),
                other,
                ResourceKind::TaskList,
                Some(&root)
            ),
            Err(ResourceHierarchyError::CrossProjectParent)
        );
        assert!(matches!(
            ResourceNode::new(
                ResourceId::new(),
                project,
                ResourceKind::QuestionnaireVersion,
                Some(&root)
            ),
            Err(ResourceHierarchyError::InvalidParentKind { .. })
        ));
    }

    #[test]
    fn inherited_grants_keep_the_original_origin() {
        let now = Utc::now();
        let direct = PermissionGrant::direct(
            GrantId::new(),
            UserId::new(),
            ResourceId::new(),
            PermissionLevel::Full,
            now,
        );
        let child =
            PermissionGrant::inherited(GrantId::new(), ResourceId::new(), &direct, now).unwrap();
        let grandchild =
            PermissionGrant::inherited(GrantId::new(), ResourceId::new(), &child, now).unwrap();
        assert_eq!(child.origin, grandchild.origin);
    }

    #[test]
    fn container_only_grants_never_propagate() {
        let grant = PermissionGrant::direct(
            GrantId::new(),
            UserId::new(),
            ResourceId::new(),
            AccessScope::ContainerOnly,
            Utc::now(),
        );
        assert_eq!(
            PermissionGrant::inherited(GrantId::new(), ResourceId::new(), &grant, Utc::now()),
            Err(PermissionError::ContainerOnlyCannotBeInherited)
        );
    }

    #[test]
    fn materialized_ancestors_keep_root_lineage() {
        let now = Utc::now();
        let direct = PermissionGrant::direct(
            GrantId::new(),
            UserId::new(),
            ResourceId::new(),
            AccessScope::Full,
            now,
        );
        let ancestor = PermissionGrant::materialized(
            GrantId::new(),
            ResourceId::new(),
            AccessScope::ContainerOnly,
            &direct,
            now,
        )
        .unwrap();
        assert_eq!(ancestor.root_grant_id, direct.id);
        assert_eq!(ancestor.access_scope, AccessScope::ContainerOnly);
    }

    proptest! {
        #[test]
        fn root_lineage_survives_arbitrary_materialization_depth(depth in 1usize..64) {
            let now = Utc::now();
            let mut grant = PermissionGrant::direct(
                GrantId::new(),
                UserId::new(),
                ResourceId::new(),
                AccessScope::Full,
                now,
            );
            let root_grant_id = grant.id;
            for _ in 0..depth {
                grant = PermissionGrant::materialized(
                    GrantId::new(),
                    ResourceId::new(),
                    AccessScope::Full,
                    &grant,
                    now,
                )
                .unwrap();
                prop_assert_eq!(grant.root_grant_id, root_grant_id);
            }
        }

        #[test]
        fn independent_origins_never_merge(materializations in 1usize..32) {
            let now = Utc::now();
            let user = UserId::new();
            let resource = ResourceId::new();
            let mut first = PermissionGrant::direct(
                GrantId::new(),
                user,
                resource,
                AccessScope::Full,
                now,
            );
            let mut second = PermissionGrant::direct(
                GrantId::new(),
                user,
                resource,
                AccessScope::Full,
                now,
            );
            let first_root = first.id;
            let second_root = second.id;
            for _ in 0..materializations {
                first = PermissionGrant::inherited(
                    GrantId::new(),
                    ResourceId::new(),
                    &first,
                    now,
                )
                .unwrap();
                second = PermissionGrant::inherited(
                    GrantId::new(),
                    ResourceId::new(),
                    &second,
                    now,
                )
                .unwrap();
            }
            prop_assert_eq!(first.root_grant_id, first_root);
            prop_assert_eq!(second.root_grant_id, second_root);
            prop_assert_ne!(first.root_grant_id, second.root_grant_id);
        }
    }
}
