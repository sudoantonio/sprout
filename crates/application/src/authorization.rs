use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use sprout_domain::{AccessScope, GrantOrigin, PermissionGrant, ResourceId, ResourceNode, UserId};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizedAction {
    ViewContainer,
    ReadContent,
    WriteContent,
    ManagePermissions,
}

pub struct AuthorizationPolicy;

impl AuthorizationPolicy {
    /// Evaluates authorization without I/O. Inherited grants are effective only
    /// while their original direct grant remains active.
    pub fn authorize(
        user_id: UserId,
        target_id: ResourceId,
        action: AuthorizedAction,
        at: DateTime<Utc>,
        resources: &[ResourceNode],
        grants: &[PermissionGrant],
    ) -> Result<(), AuthorizationError> {
        let nodes: HashMap<_, _> = resources.iter().map(|node| (node.id, node)).collect();
        let target = nodes
            .get(&target_id)
            .ok_or(AuthorizationError::ResourceNotFound)?;

        let ancestry = ancestry(target_id, &nodes)?;
        for grant in grants {
            if grant.user_id != user_id
                || !grant.is_active_at(at)
                || !grant_is_origin_valid(grant, at, grants)
            {
                continue;
            }
            let grant_node = match nodes.get(&grant.resource_id) {
                Some(node) if node.project_id == target.project_id => node,
                _ => continue,
            };
            match grant.access_scope {
                AccessScope::Full if ancestry.contains(&grant_node.id) => return Ok(()),
                AccessScope::ContainerOnly
                    if grant.resource_id == target_id
                        && action == AuthorizedAction::ViewContainer =>
                {
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(AuthorizationError::Denied)
    }
}

fn ancestry(
    target_id: ResourceId,
    nodes: &HashMap<ResourceId, &ResourceNode>,
) -> Result<HashSet<ResourceId>, AuthorizationError> {
    let mut result = HashSet::new();
    let mut current = Some(target_id);
    while let Some(id) = current {
        if !result.insert(id) {
            return Err(AuthorizationError::HierarchyCycle);
        }
        let node = nodes.get(&id).ok_or(AuthorizationError::BrokenHierarchy)?;
        current = node.parent_id;
    }
    Ok(result)
}

fn grant_is_origin_valid(
    grant: &PermissionGrant,
    at: DateTime<Utc>,
    grants: &[PermissionGrant],
) -> bool {
    match grant.origin {
        GrantOrigin::Direct | GrantOrigin::Assignment { .. } => grant.id == grant.root_grant_id,
        GrantOrigin::Inherited {
            root_grant_id,
            root_resource_id,
        } => grants.iter().any(|source| {
            source.id == root_grant_id
                && source.root_grant_id == root_grant_id
                && source.resource_id == root_resource_id
                && source.user_id == grant.user_id
                && source.access_scope == AccessScope::Full
                && matches!(
                    source.origin,
                    GrantOrigin::Direct | GrantOrigin::Assignment { .. }
                )
                && source.is_active_at(at)
        }),
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AuthorizationError {
    #[error("resource was not found")]
    ResourceNotFound,
    #[error("resource hierarchy references a missing parent")]
    BrokenHierarchy,
    #[error("resource hierarchy contains a cycle")]
    HierarchyCycle,
    #[error("access denied")]
    Denied,
}

#[cfg(test)]
mod tests {
    use sprout_domain::{GrantId, PermissionError, PermissionLevel, ProjectId, ResourceKind};

    use super::*;

    fn hierarchy() -> (Vec<ResourceNode>, ResourceId, ResourceId) {
        let project = ProjectId::new();
        let root =
            ResourceNode::new(ResourceId::new(), project, ResourceKind::Project, None).unwrap();
        let list = ResourceNode::new(
            ResourceId::new(),
            project,
            ResourceKind::TaskList,
            Some(&root),
        )
        .unwrap();
        let task =
            ResourceNode::new(ResourceId::new(), project, ResourceKind::Task, Some(&list)).unwrap();
        let list_id = list.id;
        let task_id = task.id;
        (vec![root, list, task], list_id, task_id)
    }

    #[test]
    fn full_grant_reaches_descendants_but_container_only_does_not() {
        let now = Utc::now();
        let user = UserId::new();
        let (nodes, list, task) = hierarchy();
        let full = PermissionGrant::direct(GrantId::new(), user, list, PermissionLevel::Full, now);
        assert_eq!(
            AuthorizationPolicy::authorize(
                user,
                task,
                AuthorizedAction::WriteContent,
                now,
                &nodes,
                &[full]
            ),
            Ok(())
        );

        let container = PermissionGrant::direct(
            GrantId::new(),
            user,
            list,
            PermissionLevel::ContainerOnly,
            now,
        );
        assert_eq!(
            AuthorizationPolicy::authorize(
                user,
                list,
                AuthorizedAction::ViewContainer,
                now,
                &nodes,
                &[container.clone()]
            ),
            Ok(())
        );
        assert_eq!(
            AuthorizationPolicy::authorize(
                user,
                task,
                AuthorizedAction::ReadContent,
                now,
                &nodes,
                &[container]
            ),
            Err(AuthorizationError::Denied)
        );
    }

    #[test]
    fn inherited_grant_dies_with_its_origin() {
        let now = Utc::now();
        let user = UserId::new();
        let (nodes, list, task) = hierarchy();
        let mut source =
            PermissionGrant::direct(GrantId::new(), user, list, PermissionLevel::Full, now);
        let inherited = PermissionGrant::inherited(GrantId::new(), task, &source, now).unwrap();
        source.revoke(now).unwrap();

        assert_eq!(
            AuthorizationPolicy::authorize(
                user,
                task,
                AuthorizedAction::ReadContent,
                now,
                &nodes,
                &[source, inherited]
            ),
            Err(AuthorizationError::Denied)
        );
    }

    #[test]
    fn container_only_cannot_form_an_inherited_origin() {
        let now = Utc::now();
        let grant = PermissionGrant::direct(
            GrantId::new(),
            UserId::new(),
            ResourceId::new(),
            PermissionLevel::ContainerOnly,
            now,
        );
        assert_eq!(
            PermissionGrant::inherited(GrantId::new(), ResourceId::new(), &grant, now),
            Err(PermissionError::ContainerOnlyCannotBeInherited)
        );
    }

    #[test]
    fn container_only_never_authorizes_siblings_or_body_reads() {
        let now = Utc::now();
        let user = UserId::new();
        let project = ProjectId::new();
        let root =
            ResourceNode::new(ResourceId::new(), project, ResourceKind::Project, None).unwrap();
        let list_a = ResourceNode::new(
            ResourceId::new(),
            project,
            ResourceKind::TaskList,
            Some(&root),
        )
        .unwrap();
        let list_b = ResourceNode::new(
            ResourceId::new(),
            project,
            ResourceKind::TaskList,
            Some(&root),
        )
        .unwrap();
        let task_a = ResourceNode::new(
            ResourceId::new(),
            project,
            ResourceKind::Task,
            Some(&list_a),
        )
        .unwrap();
        let task_b = ResourceNode::new(
            ResourceId::new(),
            project,
            ResourceKind::Task,
            Some(&list_b),
        )
        .unwrap();
        let nodes = vec![root, list_a.clone(), list_b, task_a, task_b.clone()];
        let header = PermissionGrant::direct(
            GrantId::new(),
            user,
            list_a.id,
            PermissionLevel::ContainerOnly,
            now,
        );

        for action in [
            AuthorizedAction::ReadContent,
            AuthorizedAction::WriteContent,
            AuthorizedAction::ManagePermissions,
        ] {
            assert_eq!(
                AuthorizationPolicy::authorize(
                    user,
                    list_a.id,
                    action,
                    now,
                    &nodes,
                    std::slice::from_ref(&header),
                ),
                Err(AuthorizationError::Denied)
            );
        }
        assert_eq!(
            AuthorizationPolicy::authorize(
                user,
                task_b.id,
                AuthorizedAction::ViewContainer,
                now,
                &nodes,
                &[header],
            ),
            Err(AuthorizationError::Denied)
        );
    }
}
