use std::{env, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use base64::Engine;
use sha2::{Digest, Sha256};
use sprout_server::{
    AppState,
    auth::{
        AuthSession, ResourceAccess, require_assignee_completion_access, require_resource_access,
    },
    build_router,
    config::Config,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

const T_LLR_02_1: &str = "T-LLR-02.1";
const T_LLR_02_2: &str = "T-LLR-02.2";
const T_LLR_02_4: &str = "T-LLR-02.4";
const T_LLR_02_5: &str = "T-LLR-02.5";
const T_LLR_02_8: &str = "T-LLR-02.8";
const T_LLR_06_9: &str = "T-LLR-06.9";

struct Fixture {
    pool: PgPool,
    owner: AuthSession,
    creator: AuthSession,
    assignee: AuthSession,
    unrelated: AuthSession,
    owner_token: String,
    creator_token: String,
    project_id: Uuid,
    owner_only_project_id: Uuid,
    root_resource_id: Uuid,
    list_resource_id: Uuid,
    task_resource_id: Uuid,
    assigned_task_resource_id: Uuid,
    task_id: Uuid,
    assigned_task_id: Uuid,
    assignment_id: Uuid,
    allowed_assignee_id: Uuid,
    denied_assignee_id: Uuid,
}

async fn migrated_pool() -> PgPool {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to a migrated disposable PostgreSQL database");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect to requirements test database")
}

fn session_token(identity_id: Uuid, session_id: Uuid) -> String {
    format!("v1.{identity_id}.{session_id}.{}", "a".repeat(64))
}

async fn insert_session(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity_id: Uuid,
    device_id: Uuid,
) -> (Uuid, String) {
    let session_id = Uuid::new_v4();
    let token = session_token(identity_id, session_id);
    sqlx::query(
        "INSERT INTO sessions (id, identity_id, device_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4, clock_timestamp() + interval '1 hour')",
    )
    .bind(session_id)
    .bind(identity_id)
    .bind(device_id)
    .bind(Sha256::digest(token.as_bytes()).to_vec())
    .execute(&mut **transaction)
    .await
    .expect("insert requirements test session");
    (session_id, token)
}

async fn fixture() -> Fixture {
    let pool = migrated_pool().await;
    let owner_id = Uuid::new_v4();
    let creator_id = Uuid::new_v4();
    let allowed_assignee_id = Uuid::new_v4();
    let denied_assignee_id = Uuid::new_v4();
    let owner_device_id = Uuid::new_v4();
    let creator_device_id = Uuid::new_v4();
    let assignee_device_id = Uuid::new_v4();
    let unrelated_device_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let owner_only_project_id = Uuid::new_v4();
    let root_resource_id = Uuid::new_v4();
    let topic_resource_id = Uuid::new_v4();
    let list_resource_id = Uuid::new_v4();
    let task_resource_id = Uuid::new_v4();
    let assigned_task_resource_id = Uuid::new_v4();
    let topic_id = Uuid::new_v4();
    let list_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let assigned_task_id = Uuid::new_v4();
    let assignment_id = Uuid::new_v4();

    let mut transaction = pool.begin().await.expect("begin fixture transaction");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("requirements tests need a migration-owner or BYPASSRLS connection");

    for (identity_id, label) in [
        (owner_id, "owner"),
        (creator_id, "creator"),
        (allowed_assignee_id, "allowed"),
        (denied_assignee_id, "denied"),
    ] {
        sqlx::query(
            "INSERT INTO identities (id, identity_handle, encrypted_profile)
             VALUES ($1, $2, decode('01', 'hex'))",
        )
        .bind(identity_id)
        .bind(format!("req-{label}-{}", identity_id.simple()))
        .execute(&mut *transaction)
        .await
        .expect("insert requirements identity");
    }
    for (device_id, identity_id) in [
        (owner_device_id, owner_id),
        (creator_device_id, creator_id),
        (assignee_device_id, allowed_assignee_id),
        (unrelated_device_id, denied_assignee_id),
    ] {
        sqlx::query(
            "INSERT INTO devices (
                 id, identity_id, device_kind, encrypted_label, trust_state
             ) VALUES ($1, $2, 'web', decode('01', 'hex'), 'trusted')",
        )
        .bind(device_id)
        .bind(identity_id)
        .execute(&mut *transaction)
        .await
        .expect("insert requirements device");
    }
    sqlx::query(
        r#"
        INSERT INTO device_keys (
            identity_id, device_id, key_version,
            encryption_public_key, signing_public_key,
            previous_package_hash, package_hash,
            x25519_public_key, ed25519_public_key
        ) VALUES
            ($1, $2, 1,
             decode(repeat('11', 32), 'hex'), decode(repeat('22', 32), 'hex'),
             decode(repeat('00', 32), 'hex'), digest($2::text, 'sha256'),
             decode(repeat('11', 32), 'hex'), decode(repeat('22', 32), 'hex')),
            ($3, $4, 1,
             decode(repeat('33', 32), 'hex'), decode(repeat('44', 32), 'hex'),
             decode(repeat('00', 32), 'hex'), digest($4::text, 'sha256'),
             decode(repeat('33', 32), 'hex'), decode(repeat('44', 32), 'hex')),
            ($5, $6, 1,
             decode(repeat('55', 32), 'hex'), decode(repeat('66', 32), 'hex'),
             decode(repeat('00', 32), 'hex'), digest($6::text, 'sha256'),
             decode(repeat('55', 32), 'hex'), decode(repeat('66', 32), 'hex')),
            ($7, $8, 1,
             decode(repeat('77', 32), 'hex'), decode(repeat('88', 32), 'hex'),
             decode(repeat('00', 32), 'hex'), digest($8::text, 'sha256'),
             decode(repeat('77', 32), 'hex'), decode(repeat('88', 32), 'hex'))
        "#,
    )
    .bind(owner_id)
    .bind(owner_device_id)
    .bind(creator_id)
    .bind(creator_device_id)
    .bind(allowed_assignee_id)
    .bind(assignee_device_id)
    .bind(denied_assignee_id)
    .bind(unrelated_device_id)
    .execute(&mut *transaction)
    .await
    .expect("insert requirements device keys");

    sqlx::query(
        "INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
         VALUES ($1, $2, decode('01', 'hex')),
                ($3, $2, decode('02', 'hex'))",
    )
    .bind(project_id)
    .bind(owner_id)
    .bind(owner_only_project_id)
    .execute(&mut *transaction)
    .await
    .expect("insert requirements projects");
    for (identity_id, role) in [
        (owner_id, "owner"),
        (creator_id, "member"),
        (allowed_assignee_id, "member"),
        (denied_assignee_id, "member"),
    ] {
        sqlx::query(
            "INSERT INTO project_memberships (project_id, identity_id, role)
             VALUES ($1, $2, $3)",
        )
        .bind(project_id)
        .bind(identity_id)
        .bind(role)
        .execute(&mut *transaction)
        .await
        .expect("insert requirements project membership");
    }
    sqlx::query(
        "INSERT INTO project_memberships (project_id, identity_id, role)
         VALUES ($1, $2, 'owner')",
    )
    .bind(owner_only_project_id)
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert owner-only membership");

    for (id, parent_id, kind, creator_id_for_node) in [
        (root_resource_id, None, "root", owner_id),
        (topic_resource_id, Some(root_resource_id), "topic", owner_id),
        (
            list_resource_id,
            Some(topic_resource_id),
            "task_list",
            owner_id,
        ),
        (task_resource_id, Some(list_resource_id), "task", creator_id),
        (
            assigned_task_resource_id,
            Some(list_resource_id),
            "task",
            owner_id,
        ),
    ] {
        sqlx::query(
            "INSERT INTO resource_nodes (
                 id, project_id, parent_id, node_kind,
                 encrypted_metadata, created_by_identity_id
             ) VALUES ($1, $2, $3, $4, decode('01', 'hex'), $5)",
        )
        .bind(id)
        .bind(project_id)
        .bind(parent_id)
        .bind(kind)
        .bind(creator_id_for_node)
        .execute(&mut *transaction)
        .await
        .expect("insert requirements resource node");
    }
    for (resource_id, creator_id_for_epoch, creator_device_id_for_epoch) in [
        (root_resource_id, owner_id, owner_device_id),
        (topic_resource_id, owner_id, owner_device_id),
        (list_resource_id, owner_id, owner_device_id),
        (task_resource_id, creator_id, creator_device_id),
        (assigned_task_resource_id, owner_id, owner_device_id),
    ] {
        sqlx::query(
            "INSERT INTO resource_epochs (
                 project_id, resource_node_id, epoch,
                 created_by_identity_id, created_by_device_id,
                 created_by_device_key_version, key_commitment, reason
             ) VALUES (
                 $1, $2, 1, $3, $4, 1,
                 decode(repeat('aa', 32), 'hex'), 'created'
             )",
        )
        .bind(project_id)
        .bind(resource_id)
        .bind(creator_id_for_epoch)
        .bind(creator_device_id_for_epoch)
        .execute(&mut *transaction)
        .await
        .expect("insert requirements resource epoch");
    }
    sqlx::query(
        "INSERT INTO topics (id, project_id, resource_node_id, encrypted_payload)
         VALUES ($1, $2, $3, decode('01', 'hex'))",
    )
    .bind(topic_id)
    .bind(project_id)
    .bind(topic_resource_id)
    .execute(&mut *transaction)
    .await
    .expect("insert requirements topic");
    sqlx::query(
        "INSERT INTO task_lists (
             id, project_id, topic_id, resource_node_id, encrypted_payload
         ) VALUES ($1, $2, $3, $4, decode('01', 'hex'))",
    )
    .bind(list_id)
    .bind(project_id)
    .bind(topic_id)
    .bind(list_resource_id)
    .execute(&mut *transaction)
    .await
    .expect("insert requirements task list");
    for (id, resource_id, creator_id_for_task) in [
        (task_id, task_resource_id, creator_id),
        (assigned_task_id, assigned_task_resource_id, owner_id),
    ] {
        sqlx::query(
            "INSERT INTO tasks (
                 id, project_id, task_list_id, resource_node_id,
                 task_kind, encrypted_payload, encrypted_value_snapshot,
                 created_by_identity_id
             ) VALUES (
                 $1, $2, $3, $4, 'priority',
                 decode('01', 'hex'), decode('02', 'hex'), $5
             )",
        )
        .bind(id)
        .bind(project_id)
        .bind(list_id)
        .bind(resource_id)
        .bind(creator_id_for_task)
        .execute(&mut *transaction)
        .await
        .expect("insert requirements task");
    }

    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'manage', 'full', 'restricted', $4, $5
         )",
    )
    .bind(project_id)
    .bind(task_resource_id)
    .bind(creator_id)
    .bind(Uuid::new_v4())
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("grant creator task management");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'view', 'full', 'restricted', $4, $5
         )",
    )
    .bind(project_id)
    .bind(list_resource_id)
    .bind(allowed_assignee_id)
    .bind(Uuid::new_v4())
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("grant allowed assignee list access");
    sqlx::query(
        "INSERT INTO task_assignments (
             id, project_id, task_id, assignee_identity_id,
             assigned_by_identity_id, encrypted_payload, permission_root_grant_id
         ) VALUES ($1, $2, $3, $4, $5, decode('01', 'hex'), $6)",
    )
    .bind(assignment_id)
    .bind(project_id)
    .bind(assigned_task_id)
    .bind(allowed_assignee_id)
    .bind(owner_id)
    .bind(Uuid::new_v4())
    .execute(&mut *transaction)
    .await
    .expect("insert active assignee");

    let (owner_session_id, owner_token) =
        insert_session(&mut transaction, owner_id, owner_device_id).await;
    let (creator_session_id, creator_token) =
        insert_session(&mut transaction, creator_id, creator_device_id).await;
    transaction.commit().await.expect("commit fixture");

    Fixture {
        pool,
        owner: AuthSession {
            identity_id: owner_id,
            device_id: owner_device_id,
            session_id: owner_session_id,
        },
        creator: AuthSession {
            identity_id: creator_id,
            device_id: creator_device_id,
            session_id: creator_session_id,
        },
        assignee: AuthSession {
            identity_id: allowed_assignee_id,
            device_id: assignee_device_id,
            session_id: Uuid::new_v4(),
        },
        unrelated: AuthSession {
            identity_id: denied_assignee_id,
            device_id: unrelated_device_id,
            session_id: Uuid::new_v4(),
        },
        owner_token,
        creator_token,
        project_id,
        owner_only_project_id,
        root_resource_id,
        list_resource_id,
        task_resource_id,
        assigned_task_resource_id,
        task_id,
        assigned_task_id,
        assignment_id,
        allowed_assignee_id,
        denied_assignee_id,
    }
}

fn app(fixture: &Fixture) -> axum::Router {
    build_router(Arc::new(
        AppState::new(Config::for_test(), fixture.pool.clone()).expect("test app state"),
    ))
    .expect("test router")
}

fn assignment_request(
    project_id: Uuid,
    task_id: Uuid,
    token: &str,
    assignee_identity_id: Uuid,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!(
            "/v1/projects/{project_id}/tasks/{task_id}/assignments"
        ))
        .header(CONTENT_TYPE, "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "assignment_id": Uuid::new_v4(),
                "permission_grant_id": Uuid::new_v4(),
                "assignee_identity_id": assignee_identity_id,
                "encrypted_payload_b64": "AQ==",
                "envelopes": [],
                "idempotency_key": Uuid::new_v4().to_string()
            })
            .to_string(),
        ))
        .expect("assignment request")
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn owner_creator_assignee_and_assignment_policy_matrices() {
    let fixture = fixture().await;

    for access in [
        ResourceAccess::ViewHeader,
        ResourceAccess::Read,
        ResourceAccess::Write,
        ResourceAccess::Manage,
    ] {
        assert!(
            require_resource_access(
                &fixture.pool,
                fixture.owner,
                fixture.project_id,
                fixture.task_resource_id,
                access,
            )
            .await
            .is_ok(),
            "{T_LLR_02_1}: owner denied {access:?}"
        );
    }
    for access in [
        ResourceAccess::ViewHeader,
        ResourceAccess::Read,
        ResourceAccess::Write,
    ] {
        assert!(
            require_resource_access(
                &fixture.pool,
                fixture.creator,
                fixture.project_id,
                fixture.task_resource_id,
                access,
            )
            .await
            .is_ok(),
            "{T_LLR_02_1}: creator denied own-resource {access:?}"
        );
    }
    assert!(
        require_resource_access(
            &fixture.pool,
            fixture.creator,
            fixture.project_id,
            fixture.root_resource_id,
            ResourceAccess::Write,
        )
        .await
        .is_err(),
        "{T_LLR_02_1}: creator modified an owner-created ancestor"
    );
    for access in [
        ResourceAccess::Read,
        ResourceAccess::Write,
        ResourceAccess::Manage,
    ] {
        assert!(
            require_resource_access(
                &fixture.pool,
                fixture.unrelated,
                fixture.project_id,
                fixture.task_resource_id,
                access,
            )
            .await
            .is_err(),
            "{T_LLR_02_1}: unrelated member received {access:?}"
        );
    }
    assert!(
        require_resource_access(
            &fixture.pool,
            fixture.creator,
            fixture.project_id,
            fixture.list_resource_id,
            ResourceAccess::ViewHeader,
        )
        .await
        .is_ok(),
        "{T_LLR_02_4}: child grant did not expose the minimum ancestor header"
    );
    for access in [
        ResourceAccess::Read,
        ResourceAccess::Write,
        ResourceAccess::Manage,
    ] {
        assert!(
            require_resource_access(
                &fixture.pool,
                fixture.creator,
                fixture.project_id,
                fixture.list_resource_id,
                access,
            )
            .await
            .is_err(),
            "{T_LLR_02_4}: container-only ancestor exposed {access:?}"
        );
    }
    assert!(
        require_resource_access(
            &fixture.pool,
            fixture.creator,
            fixture.project_id,
            fixture.assigned_task_resource_id,
            ResourceAccess::ViewHeader,
        )
        .await
        .is_err(),
        "{T_LLR_02_4}: child grant exposed a sibling header"
    );
    assert!(
        require_resource_access(
            &fixture.pool,
            fixture.creator,
            fixture.owner_only_project_id,
            fixture.root_resource_id,
            ResourceAccess::ViewHeader,
        )
        .await
        .is_err(),
        "{T_LLR_02_8}: authorization accepted a cross-project resource pair"
    );

    assert!(
        require_assignee_completion_access(
            &fixture.pool,
            fixture.assignee,
            fixture.project_id,
            fixture.assigned_task_id,
            fixture.assignment_id,
        )
        .await
        .is_ok(),
        "{T_LLR_02_2}: active assignee could not complete"
    );
    assert!(
        require_assignee_completion_access(
            &fixture.pool,
            fixture.owner,
            fixture.project_id,
            fixture.assigned_task_id,
            fixture.assignment_id,
        )
        .await
        .is_err(),
        "{T_LLR_02_2}: non-assigned owner could complete"
    );

    let app = app(&fixture);
    let denied = app
        .clone()
        .oneshot(assignment_request(
            fixture.project_id,
            fixture.task_id,
            &fixture.creator_token,
            fixture.denied_assignee_id,
        ))
        .await
        .expect("non-owner denied assignment response");
    assert_eq!(
        denied.status(),
        StatusCode::FORBIDDEN,
        "{T_LLR_02_5}: non-owner assigned a member without list access"
    );

    let allowed = app
        .clone()
        .oneshot(assignment_request(
            fixture.project_id,
            fixture.task_id,
            &fixture.creator_token,
            fixture.allowed_assignee_id,
        ))
        .await
        .expect("non-owner allowed assignment response");
    assert_eq!(
        allowed.status(),
        StatusCode::BAD_REQUEST,
        "{T_LLR_02_5}: list-authorized recipient did not reach key-envelope validation"
    );

    let owner = app
        .oneshot(assignment_request(
            fixture.project_id,
            fixture.task_id,
            &fixture.owner_token,
            fixture.denied_assignee_id,
        ))
        .await
        .expect("owner assignment response");
    assert_eq!(
        owner.status(),
        StatusCode::BAD_REQUEST,
        "{T_LLR_02_5}: owner did not reach key-envelope validation"
    );
    let assignment_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM task_assignments WHERE project_id = $1 AND task_id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.task_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count policy-test assignments");
    assert_eq!(
        assignment_count, 0,
        "{T_LLR_02_5}: a rejected assignment left partial state"
    );
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn unrecoverable_projects_fail_closed() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let challenge = base64::engine::general_purpose::STANDARD.encode([3_u8; 32]);
    let context_hash = base64::engine::general_purpose::STANDARD.encode([4_u8; 32]);
    let owner_only_request_id = Uuid::new_v4();
    let owner_only = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/recovery-requests",
                    fixture.owner_only_project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": owner_only_request_id,
                        "request_kind": "lost_owner",
                        "challenge_b64": challenge,
                        "context_hash_b64": context_hash,
                        "expires_in_seconds": 600,
                        "requester_device_key_version": 1
                    })
                    .to_string(),
                ))
                .expect("owner-only recovery request"),
        )
        .await
        .expect("owner-only recovery response");
    assert!(
        owner_only.status() == StatusCode::BAD_REQUEST
            || owner_only.status() == StatusCode::CONFLICT,
        "{T_LLR_06_9}: owner-only recovery was not refused"
    );
    let persisted = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM project_recovery_requests WHERE id = $1",
    )
    .bind(owner_only_request_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count owner-only requests");
    assert_eq!(
        persisted, 0,
        "{T_LLR_06_9}: refused recovery left a request behind"
    );

    // T-LLR-06.8: provision an active recovery set with holder shares so start
    // can freeze the electorate and finalize still cannot bypass approvals.
    let recovery_set_id = Uuid::new_v4();
    let mut provision = fixture
        .pool
        .begin()
        .await
        .expect("begin recovery provision");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *provision)
        .await
        .expect("bypass rls for recovery set fixture");
    sqlx::query(
        r#"
        INSERT INTO project_recovery_sets (
            id, project_id, recovery_epoch, membership_epoch,
            created_by_identity_id, share_count, threshold,
            secret_commitment, context_hash, encrypted_owner_key_escrow,
            state, activated_at
        )
        SELECT
            $1, $2, 1, project.membership_epoch,
            $3, 3, 3,
            decode(repeat('aa', 32), 'hex'),
            decode(repeat('cc', 32), 'hex'),
            decode(repeat('bb', 32), 'hex'),
            'draft', NULL
        FROM projects project
        WHERE project.id = $2
        "#,
    )
    .bind(recovery_set_id)
    .bind(fixture.project_id)
    .bind(fixture.owner.identity_id)
    .execute(&mut *provision)
    .await
    .expect("insert draft recovery set for electorate freeze");
    for (index, (identity_id, device_id)) in [
        (fixture.creator.identity_id, fixture.creator.device_id),
        (fixture.assignee.identity_id, fixture.assignee.device_id),
        (fixture.unrelated.identity_id, fixture.unrelated.device_id),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            r#"
            INSERT INTO project_recovery_shares (
                project_id, recovery_set_id, share_index,
                holder_identity_id, holder_device_id, holder_device_key_version,
                encrypted_share, share_commitment
            ) VALUES (
                $1, $2, $3, $4, $5, 1,
                decode(repeat('dd', 32), 'hex'),
                decode(repeat('ee', 32), 'hex')
            )
            "#,
        )
        .bind(fixture.project_id)
        .bind(recovery_set_id)
        .bind((index + 1) as i16)
        .bind(identity_id)
        .bind(device_id)
        .execute(&mut *provision)
        .await
        .expect("insert recovery share fixture");
    }
    sqlx::query(
        r#"
        UPDATE project_recovery_sets
        SET state = 'active', activated_at = clock_timestamp()
        WHERE id = $1
        "#,
    )
    .bind(recovery_set_id)
    .execute(&mut *provision)
    .await
    .expect("activate recovery set fixture");
    provision.commit().await.expect("commit recovery provision");

    let unreachable_request_id = Uuid::new_v4();
    let started = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/recovery-requests",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": unreachable_request_id,
                        "request_kind": "lost_owner",
                        "challenge_b64": challenge,
                        "context_hash_b64": context_hash,
                        "expires_in_seconds": 600,
                        "requester_device_key_version": 1
                    })
                    .to_string(),
                ))
                .expect("participant recovery request"),
        )
        .await
        .expect("participant recovery response");
    assert_eq!(started.status(), StatusCode::OK);

    let finalized = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/recovery-requests/{unreachable_request_id}/finalize",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    serde_json::json!({
                        "new_device_key_version": 1,
                        "rotations": [],
                        "replacement_recovery": {
                            "recovery_set_id": Uuid::new_v4(),
                            "recovery_epoch": 2,
                            "membership_epoch": 1,
                            "secret_commitment_b64": challenge,
                            "context_hash_b64": context_hash,
                            "encrypted_owner_key_escrow_b64": challenge,
                            "shares": []
                        }
                    })
                    .to_string(),
                ))
                .expect("unanimity bypass request"),
        )
        .await
        .expect("unanimity bypass response");
    assert_eq!(
        finalized.status(),
        StatusCode::CONFLICT,
        "{T_LLR_06_9}: an unreachable electorate was bypassed"
    );

    let electorate = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM project_recovery_electorate
         WHERE recovery_request_id = $1",
    )
    .bind(unreachable_request_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count frozen electorate");
    assert_eq!(electorate, 3, "{T_LLR_06_9}: electorate was not frozen");
}
