use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{Method, header::AUTHORIZATION, request::Parts},
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, error::AppError};

#[derive(Clone, Copy, Debug)]
pub struct AuthSession {
    pub identity_id: Uuid,
    pub device_id: Uuid,
    pub session_id: Uuid,
    /// Agent sessions are normal Sprout sessions, but their mutating surface is
    /// fail-closed. Product effects must pass through the governed agent API.
    pub is_agent: bool,
}

impl FromRequestParts<Arc<AppState>> for AuthSession {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;
        let actor = authenticate_token(&state.pool, token).await?;
        if actor.is_agent && !agent_request_allowed(&parts.method, parts.uri.path()) {
            return Err(AppError::Forbidden);
        }
        Ok(actor)
    }
}

pub(crate) async fn authenticate_token(
    pool: &PgPool,
    token: &str,
) -> Result<AuthSession, AppError> {
    let (identity_id, session_id) = parse_token_claims(token)?;
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();
    let mut transaction = pool.begin().await?;
    set_database_context(&mut transaction, identity_id, None, None).await?;
    let row = sqlx::query_as::<_, (Uuid, Uuid, String)>(
        r#"
        SELECT session.identity_id, session.device_id, identity.principal_kind
        FROM sessions session
        JOIN identities identity ON identity.id = session.identity_id
        JOIN devices device
          ON device.identity_id = session.identity_id
         AND device.id = session.device_id
        WHERE session.id = $1
          AND session.identity_id = $2
          AND session.token_hash = $3
          AND session.revoked_at IS NULL
          AND session.expires_at > clock_timestamp()
          AND identity.status = 'active'
          AND device.trust_state = 'trusted'
          AND device.retired_at IS NULL
        "#,
    )
    .bind(session_id)
    .bind(identity_id)
    .bind(token_hash)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Unauthorized)?;
    sqlx::query(
        "UPDATE sessions SET last_seen_at = clock_timestamp() WHERE id = $1 AND identity_id = $2",
    )
    .bind(session_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(AuthSession {
        identity_id: row.0,
        device_id: row.1,
        session_id,
        is_agent: row.2 == "agent",
    })
}

fn agent_request_allowed(method: &Method, path: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD) {
        return true;
    }
    if *method == Method::POST && path == "/v1/sync/pull" {
        return true;
    }
    let segments: Vec<_> = path.trim_matches('/').split('/').collect();
    matches!(
        (method, segments.as_slice()),
        (&Method::POST, ["v1", "devices", _, "key-packages"])
            | (&Method::DELETE, ["v1", "devices", _, "key-packages", _])
            | (
                &Method::PUT,
                ["v1", "projects", _, "agents", _, "runner", "activate"]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agents", _, "runner", "claim"]
            )
            | (
                &Method::POST,
                [
                    "v1",
                    "projects",
                    _,
                    "agents",
                    _,
                    "runner",
                    "client-provider",
                    "claim"
                ]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agents", _, "invocations", _, "submit"]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agents", _, "invocations", _, "fail"]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agent-global-contracts"]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agent-runs", _, "claim"]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agent-runs", _, "claims", _, "succeed"]
            )
            | (
                &Method::POST,
                [
                    "v1",
                    "projects",
                    _,
                    "agent-runs",
                    _,
                    "claims",
                    _,
                    "materialize-task-completion"
                ]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agent-runs", _, "claims", _, "fail"]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agent-runs", _, "blockers"]
            )
            | (
                &Method::POST,
                ["v1", "projects", _, "agent-runs", _, "evidence"]
            )
            | (
                &Method::POST,
                [
                    "v1",
                    "projects",
                    _,
                    "agent-runs",
                    _,
                    "blockers",
                    _,
                    "resolve"
                ]
            )
            | (
                &Method::POST,
                [
                    "v1",
                    "projects",
                    _,
                    "agents",
                    _,
                    "effects",
                    _,
                    "apply-info-document"
                ]
            )
    )
}

fn parse_token_claims(token: &str) -> Result<(Uuid, Uuid), AppError> {
    if token.len() > 512 || token.chars().any(char::is_whitespace) {
        return Err(AppError::Unauthorized);
    }
    let mut segments = token.split('.');
    if segments.next() != Some("v1") {
        return Err(AppError::Unauthorized);
    }
    let identity_id = segments
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(AppError::Unauthorized)?;
    let session_id = segments
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(AppError::Unauthorized)?;
    let secret = segments.next().ok_or(AppError::Unauthorized)?;
    if segments.next().is_some() || secret.len() < 64 {
        return Err(AppError::Unauthorized);
    }
    Ok((identity_id, session_id))
}

pub async fn set_database_context(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: Uuid,
    device_id: Option<Uuid>,
    project_id: Option<Uuid>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        SELECT
            set_config('app.identity_id', $1, true),
            set_config('app.device_id', $2, true),
            set_config('app.project_id', $3, true)
        "#,
    )
    .bind(identity_id.to_string())
    .bind(device_id.map(|id| id.to_string()).unwrap_or_default())
    .bind(project_id.map(|id| id.to_string()).unwrap_or_default())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectAccess {
    Member,
    Manage,
}

pub async fn require_project_access(
    pool: &PgPool,
    actor: AuthSession,
    project_id: Uuid,
    required: ProjectAccess,
) -> Result<(), AppError> {
    let mut transaction = pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let role = sqlx::query_scalar::<_, String>(
        r#"
        SELECT role
        FROM project_memberships
        WHERE project_id = $1
          AND identity_id = $2
          AND state = 'active'
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    transaction.commit().await?;
    if required == ProjectAccess::Manage && !matches!(role.as_str(), "owner" | "admin") {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceAccess {
    ViewHeader,
    Read,
    /// Edit encrypted Info content. Info is collaborative for every caller
    /// with full body visibility, regardless of the resource access level.
    EditInfo,
    Write,
    Manage,
}

pub async fn require_resource_access(
    pool: &PgPool,
    actor: AuthSession,
    project_id: Uuid,
    resource_node_id: Uuid,
    access: ResourceAccess,
) -> Result<(), AppError> {
    let mut transaction = pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let permission = sqlx::query_as::<_, (bool, bool, Option<String>, Option<String>)>(
        r#"
        SELECT
            project.owner_identity_id = $2
                OR membership.role = 'admin' AS owner_or_admin,
            node.created_by_identity_id = $2 AS creator,
            permission.access_level,
            permission.access_scope
        FROM resource_nodes node
        JOIN projects project ON project.id = node.project_id
        JOIN project_memberships membership
          ON membership.project_id = node.project_id
         AND membership.identity_id = $2
         AND membership.state = 'active'
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            $1, $3, $2
        ) permission ON true
        WHERE node.project_id = $1
          AND node.id = $3
          AND node.deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(actor.identity_id)
    .bind(resource_node_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;

    let allowed = authorization_facts_allow(
        AuthorizationFacts {
            owner_or_admin: permission.0,
            creator: permission.1,
            explicit_access: permission.2.as_deref(),
            access_scope: permission.3.as_deref(),
        },
        access,
    );
    if allowed {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[derive(Clone, Copy)]
struct AuthorizationFacts<'a> {
    owner_or_admin: bool,
    creator: bool,
    explicit_access: Option<&'a str>,
    access_scope: Option<&'a str>,
}

fn authorization_facts_allow(facts: AuthorizationFacts<'_>, access: ResourceAccess) -> bool {
    match access {
        ResourceAccess::ViewHeader => {
            facts.owner_or_admin || facts.creator || facts.explicit_access.is_some()
        }
        ResourceAccess::Read | ResourceAccess::EditInfo => {
            facts.owner_or_admin
                || facts.creator
                || (facts.access_scope == Some("full") && facts.explicit_access.is_some())
        }
        ResourceAccess::Write => {
            facts.owner_or_admin
                || facts.creator
                || (facts.access_scope == Some("full")
                    && matches!(facts.explicit_access, Some("edit" | "manage")))
        }
        ResourceAccess::Manage => {
            facts.owner_or_admin
                || (facts.access_scope == Some("full")
                    && matches!(facts.explicit_access, Some("manage")))
        }
    }
}

/// Completing an assigned task is authorized independently from generic task
/// writes. Assignment grants must not make arbitrary updates possible.
pub async fn require_assignee_completion_access(
    pool: &PgPool,
    actor: AuthSession,
    project_id: Uuid,
    task_id: Uuid,
    assignment_id: Uuid,
) -> Result<(), AppError> {
    let mut transaction = pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let assigned = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM task_assignments assignment
            JOIN tasks task
              ON task.project_id = assignment.project_id
             AND task.id = assignment.task_id
             AND task.deleted_at IS NULL
            WHERE assignment.project_id = $1
              AND assignment.task_id = $2
              AND assignment.id = $3
              AND assignment.assignee_identity_id = $4
              AND assignment.revoked_at IS NULL
        )
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .bind(assignment_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if assigned {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// A task assignment is intentionally insufficient for generic writes. This
/// narrow hook authorizes only encrypted file uploads attached to that task.
pub async fn require_assignee_upload_access(
    pool: &PgPool,
    actor: AuthSession,
    project_id: Uuid,
    resource_node_id: Uuid,
) -> Result<(), AppError> {
    match require_resource_access(
        pool,
        actor,
        project_id,
        resource_node_id,
        ResourceAccess::Write,
    )
    .await
    {
        Ok(()) => return Ok(()),
        Err(AppError::Forbidden) => {}
        Err(error) => return Err(error),
    }
    let mut transaction = pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let assigned = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM tasks task
            JOIN task_assignments assignment
              ON assignment.project_id = task.project_id
             AND assignment.task_id = task.id
             AND assignment.revoked_at IS NULL
            WHERE task.project_id = $1
              AND task.resource_node_id = $2
              AND task.deleted_at IS NULL
              AND assignment.assignee_identity_id = $3
        )
        "#,
    )
    .bind(project_id)
    .bind(resource_node_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if assigned {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[must_use]
pub(crate) const fn completed_attachment_upload_allowed(active_assignee: bool) -> bool {
    active_assignee
}

pub fn now() -> chrono::DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_parser_rejects_short_or_extra_secrets() {
        let identity = Uuid::new_v4();
        let session = Uuid::new_v4();
        assert!(parse_token_claims(&format!("v1.{identity}.{session}.short")).is_err());
        assert!(
            parse_token_claims(&format!("v1.{identity}.{session}.{}.extra", "a".repeat(64)))
                .is_err()
        );
    }

    #[test]
    fn assignment_never_grants_generic_write() {
        let assignee_only = AuthorizationFacts {
            owner_or_admin: false,
            creator: false,
            explicit_access: Some("edit"),
            access_scope: Some("container_only"),
        };
        assert!(!authorization_facts_allow(
            assignee_only,
            ResourceAccess::Write
        ));
        assert!(authorization_facts_allow(
            AuthorizationFacts {
                owner_or_admin: false,
                creator: false,
                explicit_access: Some("view"),
                access_scope: Some("full"),
            },
            ResourceAccess::Read
        ));
    }

    #[test]
    fn container_scope_is_header_only() {
        let facts = AuthorizationFacts {
            owner_or_admin: false,
            creator: false,
            explicit_access: Some("manage"),
            access_scope: Some("container_only"),
        };
        assert!(authorization_facts_allow(facts, ResourceAccess::ViewHeader));
        assert!(!authorization_facts_allow(facts, ResourceAccess::Read));
        assert!(!authorization_facts_allow(facts, ResourceAccess::EditInfo));
        assert!(!authorization_facts_allow(facts, ResourceAccess::Manage));
    }

    #[test]
    fn info_edit_follows_full_body_visibility_not_generic_write() {
        for access_level in ["view", "comment", "edit", "manage"] {
            let facts = AuthorizationFacts {
                owner_or_admin: false,
                creator: false,
                explicit_access: Some(access_level),
                access_scope: Some("full"),
            };
            assert!(authorization_facts_allow(facts, ResourceAccess::EditInfo));
            assert_eq!(
                authorization_facts_allow(facts, ResourceAccess::Write),
                matches!(access_level, "edit" | "manage")
            );
        }
    }

    #[test]
    fn collection_authorization_preserves_body_visibility_matrix() {
        // Active assignees enter this matrix through their full assignment
        // grant; container-only viewers retain header visibility only.
        for (facts, visible, body_visible) in [
            (
                AuthorizationFacts {
                    owner_or_admin: true,
                    creator: false,
                    explicit_access: None,
                    access_scope: None,
                },
                true,
                true,
            ),
            (
                AuthorizationFacts {
                    owner_or_admin: false,
                    creator: true,
                    explicit_access: None,
                    access_scope: None,
                },
                true,
                true,
            ),
            (
                AuthorizationFacts {
                    owner_or_admin: false,
                    creator: false,
                    explicit_access: Some("view"),
                    access_scope: Some("full"),
                },
                true,
                true,
            ),
            (
                AuthorizationFacts {
                    owner_or_admin: false,
                    creator: false,
                    explicit_access: Some("view"),
                    access_scope: Some("container_only"),
                },
                true,
                false,
            ),
            (
                AuthorizationFacts {
                    owner_or_admin: false,
                    creator: false,
                    explicit_access: None,
                    access_scope: None,
                },
                false,
                false,
            ),
        ] {
            assert_eq!(
                authorization_facts_allow(facts, ResourceAccess::ViewHeader),
                visible
            );
            assert_eq!(
                authorization_facts_allow(facts, ResourceAccess::Read),
                body_visible
            );
        }
    }

    #[test]
    fn llr_05_3_attachment_authorization_matrix() {
        for (active_assignee, owner, creator, viewer, upload, download) in [
            (true, false, false, true, true, true),
            (false, true, false, false, false, true),
            (false, false, true, false, false, true),
            (false, false, false, true, false, true),
            (false, false, false, false, false, false),
        ] {
            assert_eq!(completed_attachment_upload_allowed(active_assignee), upload);
            let resource_download = authorization_facts_allow(
                AuthorizationFacts {
                    owner_or_admin: owner,
                    creator,
                    explicit_access: viewer.then_some("view"),
                    access_scope: viewer.then_some("full"),
                },
                ResourceAccess::Read,
            );
            assert_eq!(resource_download, download);
        }
    }

    #[test]
    fn agent_sessions_are_fail_closed_outside_the_runner_protocol() {
        assert!(agent_request_allowed(
            &Method::POST,
            "/v1/projects/p/agents/a/runner/claim"
        ));
        assert!(agent_request_allowed(
            &Method::POST,
            "/v1/projects/p/agents/a/runner/client-provider/claim"
        ));
        assert!(agent_request_allowed(
            &Method::POST,
            "/v1/devices/d/key-packages"
        ));
        assert!(agent_request_allowed(&Method::GET, "/v1/projects/p/topics"));
        assert!(agent_request_allowed(
            &Method::POST,
            "/v1/projects/p/agent-global-contracts"
        ));
        assert!(agent_request_allowed(
            &Method::POST,
            "/v1/projects/p/agent-runs/r/claims/c/materialize-task-completion"
        ));
        assert!(!agent_request_allowed(
            &Method::POST,
            "/v1/projects/p/tasks"
        ));
        assert!(!agent_request_allowed(
            &Method::PUT,
            "/v1/projects/p/info-documents/d"
        ));
    }
}
