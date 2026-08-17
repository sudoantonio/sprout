use std::{env, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sprout_domain::{
    GlobalContractCandidate, LocalGoalContract, StructuredGlobalSynthesisEnvelope,
    StructuredGlobalWorkGrounding,
};
use sprout_server::{AppState, build_router, config::Config};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    pool: PgPool,
    owner_id: Uuid,
    owner_device_id: Uuid,
    owner_token: String,
    project_id: Uuid,
    profile_resource_id: Uuid,
    info_document_id: Uuid,
}

fn token(identity_id: Uuid, session_id: Uuid) -> String {
    format!("v1.{identity_id}.{session_id}.{}", "a".repeat(64))
}

fn encrypted(seed: u8) -> Value {
    json!({
        "version": 1,
        "algorithm": "aes-256-gcm",
        "key_id": format!("fixture-key-{seed}"),
        "nonce": vec![seed; 12],
        "ciphertext": vec![seed, seed.wrapping_add(1)]
    })
}

async fn fixture() -> Fixture {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to a migrated disposable PostgreSQL database");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect agent test database");
    let owner_id = Uuid::new_v4();
    let owner_device_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let owner_token = token(owner_id, session_id);
    let project_id = Uuid::new_v4();
    let root_resource_id = Uuid::new_v4();
    let profile_resource_id = Uuid::new_v4();
    let topic_id = Uuid::new_v4();
    let info_document_id = Uuid::new_v4();
    let mut transaction = pool.begin().await.expect("begin agent fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("agent tests require migration-owner database access");
    sqlx::query(
        "INSERT INTO identities (id, identity_handle, encrypted_profile)
         VALUES ($1, $2, decode('01', 'hex'))",
    )
    .bind(owner_id)
    .bind(format!("agent-owner-{}", owner_id.simple()))
    .execute(&mut *transaction)
    .await
    .expect("insert owner identity");
    sqlx::query(
        "INSERT INTO devices (id, identity_id, device_kind, encrypted_label, trust_state)
         VALUES ($1, $2, 'web', decode('01', 'hex'), 'trusted')",
    )
    .bind(owner_device_id)
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert owner device");
    sqlx::query(
        r#"
        INSERT INTO device_keys (
            identity_id, device_id, key_version,
            encryption_public_key, signing_public_key,
            previous_package_hash, package_hash,
            x25519_public_key, ed25519_public_key
        ) VALUES (
            $1, $2, 1,
            decode(repeat('11', 32), 'hex'), decode(repeat('22', 32), 'hex'),
            decode(repeat('00', 32), 'hex'), digest($2::text, 'sha256'),
            decode(repeat('11', 32), 'hex'), decode(repeat('22', 32), 'hex')
        )
        "#,
    )
    .bind(owner_id)
    .bind(owner_device_id)
    .execute(&mut *transaction)
    .await
    .expect("insert owner device key");
    sqlx::query(
        "INSERT INTO sessions (id, identity_id, device_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4, clock_timestamp() + interval '1 hour')",
    )
    .bind(session_id)
    .bind(owner_id)
    .bind(owner_device_id)
    .bind(Sha256::digest(owner_token.as_bytes()).to_vec())
    .execute(&mut *transaction)
    .await
    .expect("insert owner session");
    sqlx::query(
        "INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
         VALUES ($1, $2, decode('01', 'hex'))",
    )
    .bind(project_id)
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert project");
    sqlx::query(
        "INSERT INTO project_memberships (project_id, identity_id, role)
         VALUES ($1, $2, 'owner')",
    )
    .bind(project_id)
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert owner membership");
    for (id, parent, kind) in [
        (root_resource_id, None, "root"),
        (profile_resource_id, Some(root_resource_id), "topic"),
    ] {
        sqlx::query(
            "INSERT INTO resource_nodes (
                 id, project_id, parent_id, node_kind,
                 encrypted_metadata, created_by_identity_id
             ) VALUES ($1, $2, $3, $4, decode('01', 'hex'), $5)",
        )
        .bind(id)
        .bind(project_id)
        .bind(parent)
        .bind(kind)
        .bind(owner_id)
        .execute(&mut *transaction)
        .await
        .expect("insert resource");
        sqlx::query(
            "INSERT INTO resource_epochs (
                 project_id, resource_node_id, epoch,
                 created_by_identity_id, created_by_device_id,
                 created_by_device_key_version, key_commitment, reason
             ) VALUES ($1, $2, 1, $3, $4, 1,
                       decode(repeat('aa', 32), 'hex'), 'created')",
        )
        .bind(project_id)
        .bind(id)
        .bind(owner_id)
        .bind(owner_device_id)
        .execute(&mut *transaction)
        .await
        .expect("insert resource epoch");
    }
    sqlx::query(
        "INSERT INTO topics (id, project_id, resource_node_id, encrypted_payload)
         VALUES ($1, $2, $3, decode('01', 'hex'))",
    )
    .bind(topic_id)
    .bind(project_id)
    .bind(profile_resource_id)
    .execute(&mut *transaction)
    .await
    .expect("insert profile topic");
    sqlx::query(
        "INSERT INTO info_documents (
             id, project_id, topic_id, resource_node_id,
             encrypted_payload, key_epoch, created_by_identity_id
         ) VALUES ($1, $2, $3, $4, decode('01', 'hex'), 1, $5)",
    )
    .bind(info_document_id)
    .bind(project_id)
    .bind(topic_id)
    .bind(profile_resource_id)
    .bind(owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert info document");
    transaction.commit().await.expect("commit agent fixture");
    Fixture {
        pool,
        owner_id,
        owner_device_id,
        owner_token,
        project_id,
        profile_resource_id,
        info_document_id,
    }
}

fn app(fixture: &Fixture) -> axum::Router {
    let mut config = Config::for_test();
    config.body_limit_bytes = 64 * 1024;
    config.blob_max_file_bytes = 64 * 1024;
    config.blob_project_quota_bytes = 256 * 1024;
    build_router(Arc::new(
        AppState::new(config, fixture.pool.clone()).expect("agent test app state"),
    ))
    .expect("agent test router")
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read JSON response");
    serde_json::from_slice(&bytes).expect("decode JSON response")
}

async fn add_human_member(fixture: &Fixture, role: &str) -> (Uuid, Uuid, String, Uuid) {
    let identity_id = Uuid::new_v4();
    let device_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let bearer = token(identity_id, session_id);
    let permission_root_id = Uuid::new_v4();
    let mut transaction = fixture.pool.begin().await.expect("begin member fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for member fixture");
    sqlx::query(
        "INSERT INTO identities (id, identity_handle, encrypted_profile)
         VALUES ($1, $2, decode('01', 'hex'))",
    )
    .bind(identity_id)
    .bind(format!("member-{}", identity_id.simple()))
    .execute(&mut *transaction)
    .await
    .expect("insert member identity");
    sqlx::query(
        "INSERT INTO devices (id, identity_id, device_kind, encrypted_label, trust_state)
         VALUES ($1, $2, 'web', decode('01', 'hex'), 'trusted')",
    )
    .bind(device_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await
    .expect("insert member device");
    sqlx::query(
        "INSERT INTO sessions (id, identity_id, device_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4, clock_timestamp() + interval '1 hour')",
    )
    .bind(session_id)
    .bind(identity_id)
    .bind(device_id)
    .bind(Sha256::digest(bearer.as_bytes()).to_vec())
    .execute(&mut *transaction)
    .await
    .expect("insert member session");
    sqlx::query(
        "INSERT INTO project_memberships (project_id, identity_id, role)
         VALUES ($1, $2, $3)",
    )
    .bind(fixture.project_id)
    .bind(identity_id)
    .bind(role)
    .execute(&mut *transaction)
    .await
    .expect("insert project member");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'manage', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(identity_id)
    .bind(permission_root_id)
    .bind(fixture.owner_id)
    .execute(&mut *transaction)
    .await
    .expect("grant member resource permission");
    transaction.commit().await.expect("commit member fixture");
    (identity_id, device_id, bearer, permission_root_id)
}

async fn provision_controlled_agent(
    fixture: &Fixture,
    app: &axum::Router,
    controller_identity_id: Uuid,
    profile_resource_node_id: Uuid,
    seed: u8,
) -> (Uuid, Uuid, Uuid, Uuid) {
    let agent_id = Uuid::new_v4();
    let principal_identity_id = Uuid::new_v4();
    let runner_id = Uuid::new_v4();
    let runner_device_id = Uuid::new_v4();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agents", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": agent_id,
                        "principal_identity_id": principal_identity_id,
                        "controller_identity_id": controller_identity_id,
                        "identity_handle": format!("shared-agent-{}", principal_identity_id.simple()),
                        "encrypted_profile": encrypted(seed),
                        "profile_resource_node_id": profile_resource_node_id,
                        "encrypted_system_prompt": encrypted(seed.wrapping_add(1)),
                        "key_epoch": 1,
                        "availability": "controller_private",
                        "runner_id": runner_id,
                        "runner_device_id": runner_device_id,
                        "encrypted_runner_label": encrypted(seed.wrapping_add(2))
                    })
                    .to_string(),
                ))
                .expect("provision controlled agent request"),
        )
        .await
        .expect("provision controlled agent response");
    assert_eq!(response.status(), StatusCode::OK);
    (agent_id, principal_identity_id, runner_id, runner_device_id)
}

async fn purge_resource_through_retention(
    fixture: &Fixture,
    resource_node_id: Uuid,
    owner_identity_id: Uuid,
) {
    let subject_id = Uuid::new_v4();
    let lease_token = Uuid::new_v4();
    let now = Utc::now();
    let deleted_at = now - ChronoDuration::days(20);
    let mut transaction = fixture.pool.begin().await.expect("begin retention purge");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for retention fixture");
    sqlx::query(
        "UPDATE resource_nodes SET deleted_at = $3
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(resource_node_id)
    .bind(deleted_at)
    .execute(&mut *transaction)
    .await
    .expect("soft-delete retention resource");
    sqlx::query(
        r#"
        INSERT INTO retention_subjects (
            id, project_id, source_kind, source_id, resource_node_id,
            owner_identity_id, retention_class, source_at, warning_at,
            purge_at, state, lease_owner, lease_token, leased_until
        ) VALUES (
            $1, $2, 'resource_deleted', $3, $3, $4,
            'deleted_or_obsolete', $5, $6, $7, 'purging', $8, $9, $10
        )
        "#,
    )
    .bind(subject_id)
    .bind(fixture.project_id)
    .bind(resource_node_id)
    .bind(owner_identity_id)
    .bind(deleted_at)
    .bind(deleted_at + ChronoDuration::days(1))
    .bind(deleted_at + ChronoDuration::days(15))
    .bind(Uuid::new_v4())
    .bind(lease_token)
    .bind(now + ChronoDuration::hours(1))
    .execute(&mut *transaction)
    .await
    .expect("insert retention subject and lease");
    transaction
        .commit()
        .await
        .expect("commit retention purge setup");
    let purged =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.purge_retention_subject($1, $2, $3)")
            .bind(subject_id)
            .bind(lease_token)
            .bind(now)
            .fetch_one(&fixture.pool)
            .await
            .expect("execute retention purge");
    assert!(purged);
}

async fn create_cross_owner_tasks(
    fixture: &Fixture,
    requester_id: Uuid,
    target_controller_id: Uuid,
) -> (Uuid, Uuid) {
    let task_list_resource_id = Uuid::new_v4();
    let task_list_id = Uuid::new_v4();
    let source_task_resource_id = Uuid::new_v4();
    let source_task_id = Uuid::new_v4();
    let review_task_resource_id = Uuid::new_v4();
    let review_task_id = Uuid::new_v4();
    let mut transaction = fixture.pool.begin().await.expect("begin task fixtures");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for task fixtures");
    let topic_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM topics WHERE project_id = $1 AND resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("load fixture topic");
    for (resource_id, parent_id, kind) in [
        (
            task_list_resource_id,
            fixture.profile_resource_id,
            "task_list",
        ),
        (source_task_resource_id, task_list_resource_id, "task"),
        (review_task_resource_id, task_list_resource_id, "task"),
    ] {
        sqlx::query(
            "INSERT INTO resource_nodes (
                 id, project_id, parent_id, node_kind,
                 encrypted_metadata, created_by_identity_id
             ) VALUES ($1, $2, $3, $4, decode('01', 'hex'), $5)",
        )
        .bind(resource_id)
        .bind(fixture.project_id)
        .bind(parent_id)
        .bind(kind)
        .bind(requester_id)
        .execute(&mut *transaction)
        .await
        .expect("insert task resource");
        sqlx::query(
            "INSERT INTO resource_epochs (
                 project_id, resource_node_id, epoch,
                 created_by_identity_id, created_by_device_id,
                 created_by_device_key_version, key_commitment, reason
             ) VALUES ($1, $2, 1, $3, $4, 1,
                       decode(repeat('ab', 32), 'hex'), 'created')",
        )
        .bind(fixture.project_id)
        .bind(resource_id)
        .bind(fixture.owner_id)
        .bind(fixture.owner_device_id)
        .execute(&mut *transaction)
        .await
        .expect("insert task resource epoch");
    }
    sqlx::query(
        "INSERT INTO task_lists (
             id, project_id, topic_id, resource_node_id, encrypted_payload
         ) VALUES ($1, $2, $3, $4, decode('01', 'hex'))",
    )
    .bind(task_list_id)
    .bind(fixture.project_id)
    .bind(topic_id)
    .bind(task_list_resource_id)
    .execute(&mut *transaction)
    .await
    .expect("insert task list");
    for (task_id, resource_id) in [
        (source_task_id, source_task_resource_id),
        (review_task_id, review_task_resource_id),
    ] {
        sqlx::query(
            "INSERT INTO tasks (
                 id, project_id, task_list_id, resource_node_id,
                 encrypted_payload, task_kind, encrypted_value_snapshot,
                 created_by_identity_id
             ) VALUES ($1, $2, $3, $4, decode('01', 'hex'),
                       'priority', decode('01', 'hex'), $5)",
        )
        .bind(task_id)
        .bind(fixture.project_id)
        .bind(task_list_id)
        .bind(resource_id)
        .bind(requester_id)
        .execute(&mut *transaction)
        .await
        .expect("insert task");
    }
    sqlx::query(
        "INSERT INTO task_assignments (
             id, project_id, task_id, assignee_identity_id,
             assigned_by_identity_id, encrypted_payload, permission_root_grant_id
         ) VALUES ($1, $2, $3, $4, $5, decode('01', 'hex'), $6)",
    )
    .bind(Uuid::new_v4())
    .bind(fixture.project_id)
    .bind(review_task_id)
    .bind(target_controller_id)
    .bind(requester_id)
    .bind(Uuid::new_v4())
    .execute(&mut *transaction)
    .await
    .expect("assign review task to target controller");
    transaction.commit().await.expect("commit task fixtures");
    (source_task_resource_id, review_task_resource_id)
}

fn local_goal_value(
    local_goal_id: Uuid,
    revision: u64,
    agent_identity_id: Uuid,
    controller_id: Uuid,
    contract_scope: Uuid,
    clause_scope: Uuid,
    action: &str,
) -> Value {
    let obligation_id = Uuid::new_v4();
    let goal_id = Uuid::new_v4();
    json!({
        "id": local_goal_id,
        "revision": revision,
        "agent": agent_identity_id,
        "controller": controller_id,
        "encrypted_prompt": encrypted(40_u8.wrapping_add(revision as u8)),
        "contract": {
            "goal": goal_id,
            "scope": contract_scope,
            "obligations": [{
                "id": obligation_id,
                "goal": goal_id,
                "owner": agent_identity_id,
                "activation": {"kind": "always"},
                "required_for_completion": {"kind": "always"},
                "dependency_rank": 0
            }],
            "dependencies": [],
            "work_specs": [{
                "id": 1,
                "obligation": obligation_id,
                "owner": agent_identity_id,
                "kind": "agent_action",
                "activation": {"kind": "always"},
                "allowed_actions": [action],
                "max_instances": 1,
                "max_attempts": 1,
                "max_resolution_ticks": 10,
                "generation_rank": 0,
                "is_entry": true,
                "continuations": [],
                "failure_plan": {"kind": "fail_goal"}
            }],
            "evidence_rules": [{
                "id": 1,
                "obligation": obligation_id,
                "kind": "derived_fact",
                "subject": {"kind": "derived"},
                "verification": "semantic_judgment"
            }],
            "waiting_rules": [],
            "completion_condition": {"kind": "always"}
        },
        "clauses": [{
            "id": 1,
            "domain": 1,
            "scope": clause_scope,
            "work_spec_ids": [1]
        }],
        "origin": {"kind": "controller_prompt"},
        "supersedes_revision": (revision > 1).then_some(revision - 1)
    })
}

#[test]
fn server_visible_agent_contracts_are_recursively_closed() {
    let agent = Uuid::new_v4();
    let controller = Uuid::new_v4();
    let resource = Uuid::new_v4();
    let obligation = Uuid::new_v4();
    let goal_id = Uuid::new_v4();
    let goal = json!({
        "goal": goal_id,
        "scope": resource,
        "obligations": [{
            "id": obligation,
            "goal": goal_id,
            "owner": agent,
            "activation": { "kind": "always" },
            "required_for_completion": { "kind": "always" },
            "dependency_rank": 0
        }],
        "dependencies": [],
        "work_specs": [{
            "id": 1,
            "obligation": obligation,
            "owner": agent,
            "kind": "agent_action",
            "activation": { "kind": "always" },
            "allowed_actions": ["replace_own_task"],
            "max_instances": 1,
            "max_attempts": 1,
            "max_resolution_ticks": 1,
            "generation_rank": 0,
            "is_entry": true,
            "continuations": [],
            "failure_plan": { "kind": "fail_goal" }
        }],
        "evidence_rules": [{
            "id": 1,
            "obligation": obligation,
            "kind": "derived_fact",
            "subject": { "kind": "derived" },
            "verification": "semantic_judgment"
        }],
        "waiting_rules": [],
        "completion_condition": { "kind": "always" }
    });
    let local = json!({
        "id": Uuid::new_v4(),
        "revision": 1,
        "agent": agent,
        "controller": controller,
        "encrypted_prompt": encrypted(90),
        "contract": goal,
        "clauses": [{
            "id": 1,
            "domain": 1,
            "scope": resource,
            "work_spec_ids": [1]
        }],
        "origin": { "kind": "controller_prompt" },
        "supersedes_revision": null
    });

    let mut obligation_extra = local.clone();
    obligation_extra["contract"]["obligations"][0]["description"] = json!("plaintext");
    assert!(serde_json::from_value::<LocalGoalContract>(obligation_extra).is_err());

    let mut work_extra = local.clone();
    work_extra["contract"]["work_specs"][0]["description"] = json!("plaintext");
    assert!(serde_json::from_value::<LocalGoalContract>(work_extra).is_err());

    let mut clause_extra = local.clone();
    clause_extra["clauses"][0]["notes"] = json!("plaintext");
    assert!(serde_json::from_value::<LocalGoalContract>(clause_extra).is_err());

    let mut origin_extra = local.clone();
    origin_extra["origin"]["prompt"] = json!("plaintext");
    assert!(serde_json::from_value::<LocalGoalContract>(origin_extra).is_err());

    let candidate = json!({
        "revision": 1,
        "contract": goal,
        "contributions": [{
            "agent": agent,
            "local_revision": 1,
            "local_clause_id": 1,
            "global_work_spec_ids": [1]
        }],
        "governance_conflicts": []
    });
    let mut candidate_extra = candidate.clone();
    candidate_extra["summary"] = json!("plaintext");
    assert!(serde_json::from_value::<GlobalContractCandidate>(candidate_extra).is_err());
    let mut contribution_extra = candidate;
    contribution_extra["contributions"][0]["rationale"] = json!("plaintext");
    assert!(serde_json::from_value::<GlobalContractCandidate>(contribution_extra).is_err());

    let mut grounding = json!({
        "global_work_spec_id": 1,
        "source_agent": agent,
        "source_local_revision": 1,
        "source_work_spec_id": 1
    });
    grounding["explanation"] = json!("plaintext");
    assert!(serde_json::from_value::<StructuredGlobalWorkGrounding>(grounding).is_err());

    let task = json!({
        "id": Uuid::new_v4(),
        "kind": "synthesize_global_contract",
        "input_item_count": 1,
        "max_input_items": 1,
        "max_output_items": 1,
        "max_nesting_depth": 1,
        "max_attempts": 1,
        "closed_output_schema": true,
        "grounded_identifiers_only": true,
        "requires_formal_proof": false,
        "requires_permission_decision": false,
        "requires_exact_semantic_equivalence": false,
        "requires_exhaustive_world_knowledge": false,
        "allowed_resource_ids": [resource],
        "allowed_principal_ids": [agent],
        "allowed_tools": []
    });
    let mut envelope = json!({
        "language_task": task,
        "source_agents": [agent],
        "max_global_obligations": 1,
        "max_global_work_specs": 1,
        "max_dependencies": 0,
        "max_conflicts": 0
    });
    envelope["semantic_summary"] = json!("plaintext");
    assert!(serde_json::from_value::<StructuredGlobalSynthesisEnvelope>(envelope).is_err());
    let mut nested_task = json!({
        "language_task": task,
        "source_agents": [agent],
        "max_global_obligations": 1,
        "max_global_work_specs": 1,
        "max_dependencies": 0,
        "max_conflicts": 0
    });
    nested_task["language_task"]["plaintext_context"] = json!("plaintext");
    assert!(serde_json::from_value::<StructuredGlobalSynthesisEnvelope>(nested_task).is_err());
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn edge_runner_is_a_revocable_device_and_cannot_bypass_governance() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let agent_id = Uuid::new_v4();
    let agent_identity_id = Uuid::new_v4();
    let runner_id = Uuid::new_v4();
    let runner_device_id = Uuid::new_v4();
    let provision = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agents", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": agent_id,
                        "principal_identity_id": agent_identity_id,
                        "controller_identity_id": fixture.owner_id,
                        "identity_handle": format!("edge-agent-{}", agent_identity_id.simple()),
                        "encrypted_profile": encrypted(1),
                        "profile_resource_node_id": fixture.profile_resource_id,
                        "encrypted_system_prompt": encrypted(2),
                        "key_epoch": 1,
                        "availability": "controller_private",
                        "runner_id": runner_id,
                        "runner_device_id": runner_device_id,
                        "encrypted_runner_label": encrypted(3)
                    })
                    .to_string(),
                ))
                .expect("provision request"),
        )
        .await
        .expect("provision response");
    assert_eq!(provision.status(), StatusCode::OK);
    let provision = json_body(provision).await;
    let runner_token = provision["bootstrap_token"]
        .as_str()
        .expect("bootstrap token")
        .to_owned();

    let mut prepare = fixture.pool.begin().await.expect("begin runner prepare");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *prepare)
        .await
        .expect("disable RLS for runner fixture data");
    sqlx::query(
        r#"
        INSERT INTO device_keys (
            identity_id, device_id, key_version,
            encryption_public_key, signing_public_key,
            previous_package_hash, package_hash,
            x25519_public_key, ed25519_public_key
        ) VALUES (
            $1, $2, 1,
            decode(repeat('33', 32), 'hex'), decode(repeat('44', 32), 'hex'),
            decode(repeat('00', 32), 'hex'), digest($2::text, 'sha256'),
            decode(repeat('33', 32), 'hex'), decode(repeat('44', 32), 'hex')
        )
        "#,
    )
    .bind(agent_identity_id)
    .bind(runner_device_id)
    .execute(&mut *prepare)
    .await
    .expect("insert runner device key");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'edit', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(agent_identity_id)
    .bind(Uuid::new_v4())
    .bind(fixture.owner_id)
    .execute(&mut *prepare)
    .await
    .expect("grant runner access through the existing permission engine");
    sqlx::query(
        r#"
        INSERT INTO resource_key_envelopes (
            project_id, resource_node_id, epoch, key_purpose,
            recipient_identity_id, recipient_device_id,
            recipient_device_key_version, encrypted_key, sender_signature,
            sender_post_quantum_signature, created_by_identity_id,
            created_by_device_id, created_by_device_key_version
        ) VALUES (
            $1, $2, 1, 'body', $3, $4, 1,
            decode(repeat('55', 32), 'hex'), decode(repeat('66', 64), 'hex'),
            decode('77', 'hex'), $5, $6, 1
        )
        "#,
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(agent_identity_id)
    .bind(runner_device_id)
    .bind(fixture.owner_id)
    .bind(fixture.owner_device_id)
    .execute(&mut *prepare)
    .await
    .expect("insert normal resource key envelope for runner");
    prepare.commit().await.expect("commit runner preparation");

    let activate = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::empty())
                .expect("activate runner request"),
        )
        .await
        .expect("activate runner response");
    assert_eq!(activate.status(), StatusCode::OK);

    let direct_mutation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/tasks", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from("{}"))
                .expect("direct mutation request"),
        )
        .await
        .expect("direct mutation response");
    assert_eq!(direct_mutation.status(), StatusCode::FORBIDDEN);

    let interrogation_id = Uuid::new_v4();
    let interrogation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/interrogations",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": interrogation_id,
                        "transcript_resource_node_id": fixture.profile_resource_id,
                        "key_epoch": 1,
                        "encrypted_transcript": encrypted(7),
                        "causal_delta": {
                            "resource_effects": [],
                            "tool_invocations": [],
                            "prompt_revisions": [],
                            "local_goal_revisions": [],
                            "created_work": [],
                            "activated_obligations": [],
                            "assigned_tasks": []
                        }
                    })
                    .to_string(),
                ))
                .expect("record interrogation request"),
        )
        .await
        .expect("record interrogation response");
    assert_eq!(interrogation.status(), StatusCode::OK);
    let owner_reads_interrogation = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/interrogations/{interrogation_id}",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("creator interrogation read request"),
        )
        .await
        .expect("creator interrogation read response");
    assert_eq!(owner_reads_interrogation.status(), StatusCode::OK);
    let target_cannot_read_interrogation = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/interrogations/{interrogation_id}",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::empty())
                .expect("target interrogation read request"),
        )
        .await
        .expect("target interrogation read response");
    assert_eq!(
        target_cannot_read_interrogation.status(),
        StatusCode::NOT_FOUND
    );

    let responsibility_id = Uuid::new_v4();
    let responsibility = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/responsibilities/{responsibility_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "contract": {
                            "id": responsibility_id,
                            "revision": 1,
                            "administrator": fixture.owner_id,
                            "user": fixture.owner_id,
                            "encrypted_source_text": encrypted(11),
                            "rules": [{
                                "domain": 1,
                                "scope": fixture.profile_resource_id,
                                "allowed_actions": ["post_comment"]
                            }],
                            "supersedes_revision": null
                        }
                    })
                    .to_string(),
                ))
                .expect("record responsibility request"),
        )
        .await
        .expect("record responsibility response");
    assert_eq!(responsibility.status(), StatusCode::OK);
    let activate_responsibility = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/users/{}/responsibilities/{responsibility_id}/revisions/1/activate",
                    fixture.project_id, fixture.owner_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate responsibility request"),
        )
        .await
        .expect("activate responsibility response");
    assert_eq!(activate_responsibility.status(), StatusCode::OK);

    let local_goal_id = Uuid::new_v4();
    let obligation_id = Uuid::new_v4();
    let goal_id = Uuid::new_v4();
    let goal_contract = json!({
        "goal": goal_id,
        "scope": fixture.profile_resource_id,
        "obligations": [{
            "id": obligation_id,
            "goal": goal_id,
            "owner": agent_identity_id,
            "activation": { "kind": "always" },
            "required_for_completion": { "kind": "always" },
            "dependency_rank": 0
        }],
        "dependencies": [],
        "work_specs": [{
            "id": 1,
            "obligation": obligation_id,
            "owner": agent_identity_id,
            "kind": "agent_action",
            "activation": { "kind": "always" },
            "allowed_actions": ["post_comment"],
            "max_instances": 1,
            "max_attempts": 2,
            "max_resolution_ticks": 10,
            "generation_rank": 0,
            "is_entry": true,
            "continuations": [],
            "failure_plan": { "kind": "fail_goal" }
        }],
        "evidence_rules": [{
            "id": 1,
            "obligation": obligation_id,
            "kind": "derived_fact",
            "subject": { "kind": "derived" },
            "verification": "semantic_judgment"
        }],
        "waiting_rules": [],
        "completion_condition": { "kind": "always" }
    });
    let local_goal_contract = json!({
        "id": local_goal_id,
        "revision": 1,
        "agent": agent_identity_id,
        "controller": fixture.owner_id,
        "encrypted_prompt": encrypted(10),
        "contract": goal_contract,
        "clauses": [{
            "id": 1,
            "domain": 1,
            "scope": fixture.profile_resource_id,
            "work_spec_ids": [1]
        }],
        "origin": { "kind": "controller_prompt" },
        "supersedes_revision": null
    });
    let local_goal = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "contract": local_goal_contract
                    })
                    .to_string(),
                ))
                .expect("record local goal request"),
        )
        .await
        .expect("record local goal response");
    assert_eq!(local_goal.status(), StatusCode::OK);
    let activate_local_goal = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goals/{local_goal_id}/revisions/1/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate local goal request"),
        )
        .await
        .expect("activate local goal response");
    assert_eq!(activate_local_goal.status(), StatusCode::OK);
    let collaborative_run_id = Uuid::new_v4();
    let create_run = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agent-runs", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": collaborative_run_id,
                        "source": {
                            "kind": "local_goal",
                            "id": local_goal_id,
                            "revision": 1
                        }
                    })
                    .to_string(),
                ))
                .expect("create collaborative run request"),
        )
        .await
        .expect("create collaborative run response");
    assert_eq!(
        create_run.status(),
        StatusCode::OK,
        "{}",
        json_body(create_run).await
    );
    let claim_run = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{collaborative_run_id}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from("{}"))
                .expect("claim collaborative work request"),
        )
        .await
        .expect("claim collaborative work response");
    assert_eq!(
        claim_run.status(),
        StatusCode::OK,
        "{}",
        json_body(claim_run).await
    );
    let claimed_work = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{collaborative_run_id}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {runner_token}"))
                    .body(Body::empty())
                    .expect("read collaborative run request"),
            )
            .await
            .expect("read collaborative run response"),
    )
    .await;
    let collaborative_claim_id = claimed_work["state"]["claims"]
        .as_object()
        .and_then(|claims| claims.keys().next())
        .expect("persisted collaborative claim")
        .to_owned();
    let foreign_agent_id = Uuid::new_v4();
    let foreign_identity_id = Uuid::new_v4();
    let foreign_runner_id = Uuid::new_v4();
    let foreign_device_id = Uuid::new_v4();
    let foreign_provision = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agents", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": foreign_agent_id,
                        "principal_identity_id": foreign_identity_id,
                        "controller_identity_id": fixture.owner_id,
                        "identity_handle": format!("foreign-agent-{}", foreign_identity_id.simple()),
                        "encrypted_profile": encrypted(31),
                        "profile_resource_node_id": fixture.profile_resource_id,
                        "encrypted_system_prompt": encrypted(32),
                        "key_epoch": 1,
                        "availability": "controller_private",
                        "runner_id": foreign_runner_id,
                        "runner_device_id": foreign_device_id,
                        "encrypted_runner_label": encrypted(33)
                    })
                    .to_string(),
                ))
                .expect("provision foreign runner request"),
        )
        .await
        .expect("provision foreign runner response");
    assert_eq!(foreign_provision.status(), StatusCode::OK);
    let foreign_token = json_body(foreign_provision).await["bootstrap_token"]
        .as_str()
        .expect("foreign bootstrap token")
        .to_owned();
    let mut foreign_key = fixture.pool.begin().await.expect("begin foreign key setup");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *foreign_key)
        .await
        .expect("disable RLS for foreign key setup");
    sqlx::query(
        r#"
        INSERT INTO device_keys (
            identity_id, device_id, key_version,
            encryption_public_key, signing_public_key,
            previous_package_hash, package_hash,
            x25519_public_key, ed25519_public_key
        ) VALUES (
            $1, $2, 1,
            decode(repeat('88', 32), 'hex'), decode(repeat('99', 32), 'hex'),
            decode(repeat('00', 32), 'hex'), digest($2::text, 'sha256'),
            decode(repeat('88', 32), 'hex'), decode(repeat('99', 32), 'hex')
        )
        "#,
    )
    .bind(foreign_identity_id)
    .bind(foreign_device_id)
    .execute(&mut *foreign_key)
    .await
    .expect("insert foreign runner key");
    foreign_key
        .commit()
        .await
        .expect("commit foreign key setup");
    let foreign_activate = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{foreign_agent_id}/runner/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {foreign_token}"))
                .body(Body::empty())
                .expect("activate foreign runner request"),
        )
        .await
        .expect("activate foreign runner response");
    assert_eq!(foreign_activate.status(), StatusCode::OK);
    for foreign_uri in [
        format!(
            "/v1/projects/{}/agent-runs/{collaborative_run_id}/claim",
            fixture.project_id
        ),
        format!(
            "/v1/projects/{}/agent-runs/{collaborative_run_id}/claims/{collaborative_claim_id}/succeed",
            fixture.project_id
        ),
        format!(
            "/v1/projects/{}/agent-runs/{collaborative_run_id}/claims/{collaborative_claim_id}/fail",
            fixture.project_id
        ),
    ] {
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(foreign_uri)
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {foreign_token}"))
                    .body(Body::from("{}"))
                    .expect("foreign runner completion request"),
            )
            .await
            .expect("foreign runner completion response");
        let status = rejected.status();
        let body = json_body(rejected).await;
        assert!(
            matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
            "foreign runner status={status} body={body}"
        );
    }
    let mut plaintext_local_goal = local_goal_contract.clone();
    plaintext_local_goal
        .as_object_mut()
        .expect("local goal object")
        .insert(
            "plaintext_prompt".to_owned(),
            Value::String("must remain encrypted".to_owned()),
        );
    let plaintext_rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({ "contract": plaintext_local_goal }).to_string(),
                ))
                .expect("reject plaintext local goal request"),
        )
        .await
        .expect("reject plaintext local goal response");
    assert_eq!(
        plaintext_rejected.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut nested_plaintext_local_goal = local_goal_contract.clone();
    nested_plaintext_local_goal
        .get_mut("contract")
        .and_then(|contract| contract.get_mut("work_specs"))
        .and_then(Value::as_array_mut)
        .and_then(|work_specs| work_specs.first_mut())
        .and_then(Value::as_object_mut)
        .expect("nested work spec object")
        .insert(
            "description".to_owned(),
            Value::String("sensitive semantic plaintext".to_owned()),
        );
    let nested_plaintext_rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({ "contract": nested_plaintext_local_goal }).to_string(),
                ))
                .expect("reject nested plaintext local goal request"),
        )
        .await
        .expect("reject nested plaintext local goal response");
    assert_eq!(
        nested_plaintext_rejected.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let global_contract_id = Uuid::new_v4();
    let global_contract = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-global-contracts",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": global_contract_id,
                        "synthesis_invocation_id": null,
                        "envelope": {
                            "language_task": {
                                "id": Uuid::new_v4(),
                                "kind": "synthesize_global_contract",
                                "input_item_count": 1,
                                "max_input_items": 1,
                                "max_output_items": 4,
                                "max_nesting_depth": 4,
                                "max_attempts": 1,
                                "closed_output_schema": true,
                                "grounded_identifiers_only": true,
                                "requires_formal_proof": false,
                                "requires_permission_decision": false,
                                "requires_exact_semantic_equivalence": false,
                                "requires_exhaustive_world_knowledge": false,
                                "allowed_resource_ids": [fixture.profile_resource_id],
                                "allowed_principal_ids": [agent_identity_id],
                                "allowed_tools": []
                            },
                            "source_agents": [agent_identity_id],
                            "max_global_obligations": 1,
                            "max_global_work_specs": 1,
                            "max_dependencies": 0,
                            "max_conflicts": 0
                        },
                        "candidate": {
                            "revision": 1,
                            "contract": goal_contract,
                            "contributions": [{
                                "agent": agent_identity_id,
                                "local_revision": 1,
                                "local_clause_id": 1,
                                "global_work_spec_ids": [1]
                            }],
                            "governance_conflicts": []
                        },
                        "groundings": [{
                            "global_work_spec_id": 1,
                            "source_agent": agent_identity_id,
                            "source_local_revision": 1,
                            "source_work_spec_id": 1
                        }]
                    })
                    .to_string(),
                ))
                .expect("record global contract request"),
        )
        .await
        .expect("record global contract response");
    assert_eq!(global_contract.status(), StatusCode::OK);

    let synthesis_invocation_id = Uuid::new_v4();
    let synthesis_task = json!({
        "id": Uuid::new_v4(),
        "kind": "synthesize_global_contract",
        "input_item_count": 1,
        "max_input_items": 1,
        "max_output_items": 2,
        "max_nesting_depth": 4,
        "max_attempts": 1,
        "closed_output_schema": true,
        "grounded_identifiers_only": true,
        "requires_formal_proof": false,
        "requires_permission_decision": false,
        "requires_exact_semantic_equivalence": false,
        "requires_exhaustive_world_knowledge": false,
        "allowed_resource_ids": [fixture.profile_resource_id],
        "allowed_principal_ids": [agent_identity_id],
        "allowed_tools": []
    });
    let queue_synthesis = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": synthesis_invocation_id,
                        "local_goal_id": local_goal_id,
                        "local_goal_revision": 1,
                        "language_task": synthesis_task,
                        "authority_envelope": {
                            "resource_authority": [{
                                "resource_id": fixture.profile_resource_id,
                                "operation": "read"
                            }],
                            "tool_authority": []
                        },
                        "sources": [{
                            "kind": "resource_body",
                            "resource_id": fixture.profile_resource_id
                        }],
                        "encrypted_input": encrypted(12)
                    })
                    .to_string(),
                ))
                .expect("queue global synthesis invocation"),
        )
        .await
        .expect("queue global synthesis response");
    assert_eq!(queue_synthesis.status(), StatusCode::OK);
    let claim_synthesis = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/claim",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::empty())
                .expect("claim global synthesis invocation"),
        )
        .await
        .expect("claim global synthesis response");
    assert_eq!(claim_synthesis.status(), StatusCode::OK);
    let claim_synthesis = json_body(claim_synthesis).await;
    let synthesis_lease_id = claim_synthesis["lease_id"]
        .as_str()
        .expect("global synthesis lease id");
    let submit_synthesis = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{synthesis_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "lease_id": synthesis_lease_id,
                        "structured_output": {
                            "items": [{
                                "resource_id": fixture.profile_resource_id,
                                "principal_id": agent_identity_id,
                                "tool": null,
                                "action": null
                            }],
                            "max_observed_nesting_depth": 1
                        },
                        "encrypted_output": encrypted(13),
                        "effects": []
                    })
                    .to_string(),
                ))
                .expect("submit global synthesis invocation"),
        )
        .await
        .expect("submit global synthesis response");
    assert_eq!(submit_synthesis.status(), StatusCode::OK);
    let runner_global_contract = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-global-contracts",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "id": global_contract_id,
                        "synthesis_invocation_id": synthesis_invocation_id,
                        "envelope": {
                            "language_task": synthesis_task,
                            "source_agents": [agent_identity_id],
                            "max_global_obligations": 1,
                            "max_global_work_specs": 1,
                            "max_dependencies": 0,
                            "max_conflicts": 0
                        },
                        "candidate": {
                            "revision": 2,
                            "contract": goal_contract,
                            "contributions": [{
                                "agent": agent_identity_id,
                                "local_revision": 1,
                                "local_clause_id": 1,
                                "global_work_spec_ids": [1]
                            }],
                            "governance_conflicts": []
                        },
                        "groundings": [{
                            "global_work_spec_id": 1,
                            "source_agent": agent_identity_id,
                            "source_local_revision": 1,
                            "source_work_spec_id": 1
                        }]
                    })
                    .to_string(),
                ))
                .expect("record runner-synthesized global contract"),
        )
        .await
        .expect("record runner-synthesized global response");
    assert_eq!(
        runner_global_contract.status(),
        StatusCode::OK,
        "{}",
        json_body(runner_global_contract).await
    );

    let proxy_id = Uuid::new_v4();
    let proxy_thread_id = Uuid::new_v4();
    let create_thread = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/user-proxy/threads",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "proxy_id": proxy_id,
                        "thread_id": proxy_thread_id
                    })
                    .to_string(),
                ))
                .expect("create proxy thread request"),
        )
        .await
        .expect("create proxy thread response");
    assert_eq!(create_thread.status(), StatusCode::OK);
    let proxy_request_id = Uuid::new_v4();
    let proxy_request = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/user-proxy/threads/{proxy_thread_id}/requests",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": proxy_request_id,
                        "encrypted_payload": encrypted(8)
                    })
                    .to_string(),
                ))
                .expect("submit proxy request"),
        )
        .await
        .expect("submit proxy response");
    assert_eq!(proxy_request.status(), StatusCode::OK);
    let proxy_plan_payload = json!({
        "id": Uuid::new_v4(),
        "invocation_id": null,
        "envelope": {
            "language_task": {
                "id": Uuid::new_v4(),
                "kind": "interpret_proxy_request",
                "input_item_count": 1,
                "max_input_items": 1,
                "max_output_items": 1,
                "max_nesting_depth": 1,
                "max_attempts": 1,
                "closed_output_schema": true,
                "grounded_identifiers_only": true,
                "requires_formal_proof": false,
                "requires_permission_decision": false,
                "requires_exact_semantic_equivalence": false,
                "requires_exhaustive_world_knowledge": false,
                "allowed_resource_ids": [fixture.profile_resource_id],
                "allowed_principal_ids": [fixture.owner_id],
                "allowed_tools": []
            },
            "request_id": proxy_request_id,
            "user": fixture.owner_id,
            "candidate_resources": [fixture.profile_resource_id],
            "candidate_operations": ["post_comment"],
            "available_tools": [],
            "max_plan_steps": 1
        },
        "plan": {
            "request_id": proxy_request_id,
            "thread_id": proxy_thread_id,
            "user": fixture.owner_id,
            "intent_id": Uuid::new_v4(),
            "resource_effects": [{
                "resource_id": fixture.profile_resource_id,
                "operation": "post_comment"
            }],
            "tool_invocations": [],
            "encrypted_explanation": encrypted(9)
        },
        "confirmation": null
    });
    let mut forged_proxy_plan = proxy_plan_payload.clone();
    forged_proxy_plan["action_classification"] = json!([{
        "resource_id": fixture.profile_resource_id,
        "action": "replace_own_task"
    }]);
    let forged = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/user-proxy/requests/{proxy_request_id}/plan",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(forged_proxy_plan.to_string()))
                .expect("forged proxy classification request"),
        )
        .await
        .expect("forged proxy classification response");
    assert_eq!(forged.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let proxy_plan = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/user-proxy/requests/{proxy_request_id}/plan",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(proxy_plan_payload.to_string()))
                .expect("record proxy plan request"),
        )
        .await
        .expect("record proxy plan response");
    assert_eq!(proxy_plan.status(), StatusCode::OK);
    let proxy_plan = json_body(proxy_plan).await;
    assert_eq!(proxy_plan["within_responsibility"], true);
    assert_eq!(proxy_plan["confirmation_required"], false);

    let invocation_id = Uuid::new_v4();
    let queue = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": invocation_id,
                        "local_goal_id": null,
                        "local_goal_revision": null,
                        "language_task": {
                            "id": Uuid::new_v4(),
                            "kind": "answer_from_authorized_context",
                            "input_item_count": 1,
                            "max_input_items": 1,
                            "max_output_items": 2,
                            "max_nesting_depth": 2,
                            "max_attempts": 2,
                            "closed_output_schema": true,
                            "grounded_identifiers_only": true,
                            "requires_formal_proof": false,
                            "requires_permission_decision": false,
                            "requires_exact_semantic_equivalence": false,
                            "requires_exhaustive_world_knowledge": false,
                            "allowed_resource_ids": [fixture.profile_resource_id],
                            "allowed_principal_ids": [agent_identity_id],
                            "allowed_tools": []
                        },
                        "authority_envelope": {
                            "resource_authority": [
                                {
                                    "resource_id": fixture.profile_resource_id,
                                    "operation": "read"
                                },
                                {
                                    "resource_id": fixture.profile_resource_id,
                                    "operation": "edit_info"
                                }
                            ],
                            "tool_authority": []
                        },
                        "sources": [{
                            "kind": "resource_body",
                            "resource_id": fixture.profile_resource_id
                        }],
                        "encrypted_input": encrypted(4)
                    })
                    .to_string(),
                ))
                .expect("queue invocation request"),
        )
        .await
        .expect("queue invocation response");
    assert_eq!(queue.status(), StatusCode::OK, "{}", json_body(queue).await);

    let claim = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/claim",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::empty())
                .expect("claim invocation request"),
        )
        .await
        .expect("claim invocation response");
    assert_eq!(claim.status(), StatusCode::OK);
    let claim = json_body(claim).await;
    let lease_id = claim["lease_id"].as_str().expect("lease id");
    assert!(claim.get("memory").is_none());
    assert!(claim.get("model_memory").is_none());

    let effect_id = Uuid::new_v4();
    let submit = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "lease_id": lease_id,
                        "structured_output": {
                            "items": [{
                                "resource_id": fixture.profile_resource_id,
                                "principal_id": null,
                                "tool": null,
                                "action": null
                            }],
                            "max_observed_nesting_depth": 1
                        },
                        "encrypted_output": encrypted(5),
                        "effects": [{
                            "id": effect_id,
                            "effect": {
                                "resource_id": fixture.profile_resource_id,
                                "operation": "edit_info"
                            },
                            "materialization": {
                                "kind": "replace_info_document",
                                "document_id": fixture.info_document_id,
                                "expected_payload_version": 1,
                                "key_epoch": 1,
                                "idempotency_key": Uuid::new_v4(),
                                "payload": encrypted(6)
                            }
                        }]
                    })
                    .to_string(),
                ))
                .expect("submit invocation request"),
        )
        .await
        .expect("submit invocation response");
    assert_eq!(
        submit.status(),
        StatusCode::OK,
        "{}",
        json_body(submit).await
    );

    let apply = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/effects/{effect_id}/apply-info-document",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::empty())
                .expect("apply info effect request"),
        )
        .await
        .expect("apply info effect response");
    assert_eq!(apply.status(), StatusCode::OK);
    let stored = sqlx::query_as::<_, (i64, Vec<u8>)>(
        "SELECT payload_version, encrypted_payload FROM info_documents
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.info_document_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load agent-updated info document");
    assert_eq!(stored.0, 2);
    let stored_payload: Value =
        serde_json::from_slice(&stored.1).expect("stored info payload remains wire ciphertext");
    assert_eq!(stored_payload["algorithm"], "aes-256-gcm");
    assert!(stored_payload["ciphertext_b64"].is_string());

    let mut revoke = fixture.pool.begin().await.expect("begin runner revocation");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *revoke)
        .await
        .expect("disable RLS for revocation");
    sqlx::query(
        "UPDATE device_keys SET revoked_at = clock_timestamp()
         WHERE identity_id = $1 AND device_id = $2 AND key_version = 1",
    )
    .bind(agent_identity_id)
    .bind(runner_device_id)
    .execute(&mut *revoke)
    .await
    .expect("revoke runner key");
    revoke.commit().await.expect("commit runner revocation");
    let revoked_claim = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/claim",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::empty())
                .expect("revoked runner claim request"),
        )
        .await
        .expect("revoked runner claim response");
    assert_eq!(revoked_claim.status(), StatusCode::FORBIDDEN);
    let revoked_completion_claim = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{collaborative_run_id}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from("{}"))
                .expect("revoked completion runner claim request"),
        )
        .await
        .expect("revoked completion runner claim response");
    assert_eq!(revoked_completion_claim.status(), StatusCode::FORBIDDEN);
    for terminal_action in ["succeed", "fail"] {
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{collaborative_run_id}/claims/{collaborative_claim_id}/{terminal_action}",
                        fixture.project_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {runner_token}"))
                    .body(Body::from("{}"))
                    .expect("revoked runner terminal work request"),
            )
            .await
            .expect("revoked runner terminal work response");
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    }

    let audit_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_audit_log WHERE project_id = $1 AND agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count agent audit records");
    // Draft and activation are separate fenced lifecycle events.
    assert_eq!(audit_count, 15);

    let retention_subject_id = Uuid::new_v4();
    let retention_lease_owner = Uuid::new_v4();
    let retention_lease_token = Uuid::new_v4();
    let retention_now = Utc::now();
    let deleted_at = retention_now - ChronoDuration::days(20);
    let mut retention = fixture.pool.begin().await.expect("begin agent retention");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *retention)
        .await
        .expect("disable RLS for retention fixture");
    sqlx::query(
        "UPDATE resource_nodes SET deleted_at = $3
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(deleted_at)
    .execute(&mut *retention)
    .await
    .expect("soft-delete agent profile resource");
    sqlx::query(
        r#"
        INSERT INTO retention_subjects (
            id, project_id, source_kind, source_id, resource_node_id,
            owner_identity_id, retention_class, source_at, warning_at,
            purge_at, state, lease_owner, lease_token, leased_until
        ) VALUES (
            $1, $2, 'resource_deleted', $3, $3, $4,
            'deleted_or_obsolete', $5, $6, $7, 'purging', $8, $9, $10
        )
        "#,
    )
    .bind(retention_subject_id)
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(fixture.owner_id)
    .bind(deleted_at)
    .bind(deleted_at + ChronoDuration::days(1))
    .bind(deleted_at + ChronoDuration::days(15))
    .bind(retention_lease_owner)
    .bind(retention_lease_token)
    .bind(retention_now + ChronoDuration::hours(1))
    .execute(&mut *retention)
    .await
    .expect("insert active agent retention lease");
    retention
        .commit()
        .await
        .expect("commit agent retention setup");
    let purged =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.purge_retention_subject($1, $2, $3)")
            .bind(retention_subject_id)
            .bind(retention_lease_token)
            .bind(retention_now)
            .fetch_one(&fixture.pool)
            .await
            .expect("purge agent resource through retention pipeline");
    assert!(purged);
    let remaining_agent_records = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT sum(row_count)::bigint FROM (
            SELECT count(*) AS row_count FROM governed_agents WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_runners WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_responsibility_contracts WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_local_goal_contracts WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_invocations WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_invocation_sources WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_effect_proposals WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_audit_log WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_global_contracts WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_global_contract_sources WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_interrogations WHERE project_id = $1
            UNION ALL SELECT count(*) FROM user_proxies WHERE project_id = $1
            UNION ALL SELECT count(*) FROM user_proxy_threads WHERE project_id = $1
            UNION ALL SELECT count(*) FROM user_proxy_requests WHERE project_id = $1
            UNION ALL SELECT count(*) FROM user_proxy_plans WHERE project_id = $1
        ) records
        "#,
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count records after agent retention purge");
    // Responsibility is user-level governance and survives deletion of the
    // controlled agent profile. Every agent-owned runtime record is removed.
    assert_eq!(remaining_agent_records, 1);
    let runner_session_revoked = sqlx::query_scalar::<_, bool>(
        "SELECT revoked_at IS NOT NULL FROM sessions
         WHERE identity_id = $1 AND device_id = $2",
    )
    .bind(agent_identity_id)
    .bind(runner_device_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("check runner session revocation after purge");
    assert!(runner_session_revoked);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn normal_controller_can_atomically_activate_exact_local_goal_and_stale_retry_rolls_back() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let (controller_id, _controller_device_id, controller_token, _controller_permission_root) =
        add_human_member(&fixture, "member").await;
    let agent_id = Uuid::new_v4();
    let agent_identity_id = Uuid::new_v4();
    let provision = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agents", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": agent_id,
                        "principal_identity_id": agent_identity_id,
                        "controller_identity_id": controller_id,
                        "identity_handle": format!("member-agent-{}", agent_identity_id.simple()),
                        "encrypted_profile": encrypted(31),
                        "profile_resource_node_id": fixture.profile_resource_id,
                        "encrypted_system_prompt": encrypted(32),
                        "key_epoch": 1,
                        "availability": "controller_private",
                        "runner_id": Uuid::new_v4(),
                        "runner_device_id": Uuid::new_v4(),
                        "encrypted_runner_label": encrypted(33)
                    })
                    .to_string(),
                ))
                .expect("provision normal-user agent request"),
        )
        .await
        .expect("provision normal-user agent response");
    assert_eq!(provision.status(), StatusCode::OK);

    let responsibility_id = Uuid::new_v4();
    let responsibility = json!({
        "id": responsibility_id,
        "revision": 1,
        "administrator": fixture.owner_id,
        "user": controller_id,
        "encrypted_source_text": encrypted(34),
        "rules": [{
            "domain": 1,
            "scope": fixture.profile_resource_id,
            "allowed_actions": ["post_comment"]
        }],
        "supersedes_revision": null
    });
    let drafted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(json!({"contract": responsibility}).to_string()))
                .expect("draft user responsibility request"),
        )
        .await
        .expect("draft user responsibility response");
    assert_eq!(drafted.status(), StatusCode::OK);
    let activated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}/revisions/1/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate user responsibility request"),
        )
        .await
        .expect("activate user responsibility response");
    assert_eq!(activated.status(), StatusCode::OK);

    let local_goal_id = Uuid::new_v4();
    let obligation_id = Uuid::new_v4();
    let goal_id = Uuid::new_v4();
    let exact_prompt = encrypted(35);
    let local_goal = json!({
        "id": local_goal_id,
        "revision": 1,
        "agent": agent_identity_id,
        "controller": controller_id,
        "encrypted_prompt": exact_prompt,
        "contract": {
            "goal": goal_id,
            "scope": fixture.profile_resource_id,
            "obligations": [{
                "id": obligation_id,
                "goal": goal_id,
                "owner": agent_identity_id,
                "activation": {"kind": "always"},
                "required_for_completion": {"kind": "always"},
                "dependency_rank": 0
            }],
            "dependencies": [],
            "work_specs": [{
                "id": 1,
                "obligation": obligation_id,
                "owner": agent_identity_id,
                "kind": "agent_action",
                "activation": {"kind": "always"},
                "allowed_actions": ["post_comment"],
                "max_instances": 1,
                "max_attempts": 1,
                "max_resolution_ticks": 10,
                "generation_rank": 0,
                "is_entry": true,
                "continuations": [],
                "failure_plan": {"kind": "fail_goal"}
            }],
            "evidence_rules": [{
                "id": 1,
                "obligation": obligation_id,
                "kind": "derived_fact",
                "subject": {"kind": "derived"},
                "verification": "semantic_judgment"
            }],
            "waiting_rules": [],
            "completion_condition": {"kind": "always"}
        },
        "clauses": [{
            "id": 1,
            "domain": 1,
            "scope": fixture.profile_resource_id,
            "work_spec_ids": [1]
        }],
        "origin": {"kind": "controller_prompt"},
        "supersedes_revision": null
    });
    let drafted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::from(json!({"contract": local_goal}).to_string()))
                .expect("draft local goal request"),
        )
        .await
        .expect("draft local goal response");
    assert_eq!(drafted.status(), StatusCode::OK);
    let activation_uri = format!(
        "/v1/projects/{}/agents/{agent_id}/local-goals/{local_goal_id}/revisions/1/activate",
        fixture.project_id
    );
    let activated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&activation_uri)
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::empty())
                .expect("activate local goal request"),
        )
        .await
        .expect("activate local goal response");
    assert_eq!(activated.status(), StatusCode::OK);

    let exact_prompt_bound = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT agent.encrypted_system_prompt = prompt.encrypted_prompt
          AND prompt.state = 'active' AND local.state = 'active'
        FROM governed_agents agent
        JOIN agent_prompt_revisions prompt
          ON prompt.project_id = agent.project_id AND prompt.agent_id = agent.id
        JOIN agent_local_goal_contracts local
          ON local.project_id = prompt.project_id
         AND local.id = prompt.local_goal_id
         AND local.revision = prompt.local_goal_revision
        WHERE agent.project_id = $1 AND agent.id = $2
        "#,
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify exact active prompt binding");
    assert!(exact_prompt_bound);
    let final_approval = sqlx::query(
        r#"
        SELECT prompt.draft_id, approval.agent_id,
               approval.controller_identity_id, approval.local_goal_id,
               approval.local_goal_revision,
               approval.prompt_hash = prompt.prompt_hash AS exact_hash
        FROM agent_prompt_revisions prompt
        JOIN agent_prompt_final_approvals approval
          ON approval.project_id = prompt.project_id
         AND approval.draft_id = prompt.draft_id
        WHERE prompt.project_id = $1 AND prompt.agent_id = $2
          AND prompt.state = 'active'
        "#,
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact final prompt approval certificate");
    assert_ne!(
        final_approval.try_get::<Uuid, _>("draft_id").unwrap(),
        Uuid::nil()
    );
    assert_eq!(
        final_approval.try_get::<Uuid, _>("agent_id").unwrap(),
        agent_id
    );
    assert_eq!(
        final_approval
            .try_get::<Uuid, _>("controller_identity_id")
            .unwrap(),
        controller_id
    );
    assert_eq!(
        final_approval.try_get::<Uuid, _>("local_goal_id").unwrap(),
        local_goal_id
    );
    assert_eq!(
        final_approval
            .try_get::<i64, _>("local_goal_revision")
            .unwrap(),
        1
    );
    assert!(final_approval.try_get::<bool, _>("exact_hash").unwrap());
    let forged_approval = sqlx::query(
        "INSERT INTO agent_prompt_final_approvals (
             project_id, draft_id, agent_id, controller_identity_id,
             local_goal_id, local_goal_revision, prompt_hash
         ) VALUES ($1, $2, $3, $4, $5, 1, decode(repeat('00', 32), 'hex'))",
    )
    .bind(fixture.project_id)
    .bind(Uuid::new_v4())
    .bind(agent_id)
    .bind(controller_id)
    .bind(local_goal_id)
    .execute(&fixture.pool)
    .await
    .expect_err("mismatched prompt/draft approval must fail closed");
    assert_eq!(
        forged_approval
            .as_database_error()
            .and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("55000"))
    );

    let audit_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2
           AND event_kind = 'local_goal_activated'",
    )
    .bind(fixture.project_id)
    .bind(controller_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count activation audit before stale retry");
    assert_eq!(audit_before, 1);
    let approvals_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_prompt_final_approvals
         WHERE project_id = $1 AND agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count final approvals before stale retry");
    assert_eq!(approvals_before, 1);
    let stale = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&activation_uri)
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::empty())
                .expect("stale local activation request"),
        )
        .await
        .expect("stale local activation response");
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let audit_after = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2
           AND event_kind = 'local_goal_activated'",
    )
    .bind(fixture.project_id)
    .bind(controller_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count activation audit after stale retry");
    assert_eq!(audit_after, audit_before);
    let approvals_after = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_prompt_final_approvals
         WHERE project_id = $1 AND agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count final approvals after stale retry");
    assert_eq!(approvals_after, approvals_before);

    let responsibility_revision_2 = json!({
        "id": responsibility_id,
        "revision": 2,
        "administrator": fixture.owner_id,
        "user": controller_id,
        "encrypted_source_text": encrypted(39),
        "rules": [{
            "domain": 1,
            "scope": fixture.profile_resource_id,
            "allowed_actions": ["post_comment"]
        }],
        "supersedes_revision": 1
    });
    let drafted_revision_2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({"contract": responsibility_revision_2}).to_string(),
                ))
                .expect("draft second responsibility revision request"),
        )
        .await
        .expect("draft second responsibility revision response");
    assert_eq!(drafted_revision_2.status(), StatusCode::OK);
    let activated_revision_2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}/revisions/2/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate second responsibility revision request"),
        )
        .await
        .expect("activate second responsibility revision response");
    assert_eq!(activated_revision_2.status(), StatusCode::OK);
    let responsibility_history_is_fenced = sqlx::query_scalar::<_, bool>(
        "SELECT count(*) FILTER (WHERE revision = 1 AND state = 'superseded') = 1
             AND count(*) FILTER (WHERE revision = 2 AND state = 'active') = 1
         FROM agent_responsibility_contracts
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(responsibility_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify fenced responsibility history");
    assert!(responsibility_history_is_fenced);

    // The domain requires contiguous revisions, so there is no positive
    // non-contiguous case. Persist a stale/corrupt draft as migration owner to
    // prove activation follows its explicit supersedes pointer and fails
    // closed instead of inferring the active predecessor arithmetically.
    let stale_revision = json!({
        "id": responsibility_id,
        "revision": 3,
        "administrator": fixture.owner_id,
        "user": controller_id,
        "encrypted_source_text": encrypted(41),
        "rules": [{
            "domain": 1,
            "scope": fixture.profile_resource_id,
            "allowed_actions": ["post_comment"]
        }],
        "supersedes_revision": 1
    });
    let stale_revision_json = stale_revision.to_string();
    let stale_revision_hash: [u8; 32] = Sha256::digest(stale_revision_json.as_bytes()).into();
    sqlx::query(
        "INSERT INTO agent_responsibility_contracts (
             id, project_id, revision, administrator_identity_id,
             user_identity_id, contract, contract_hash, state
         ) VALUES ($1, $2, 3, $3, $4, $5::jsonb, $6, 'draft')",
    )
    .bind(responsibility_id)
    .bind(fixture.project_id)
    .bind(fixture.owner_id)
    .bind(controller_id)
    .bind(stale_revision_json)
    .bind(stale_revision_hash.as_slice())
    .execute(&fixture.pool)
    .await
    .expect("insert stale supersedes draft fixture");
    let governance_audit_before_stale = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2",
    )
    .bind(fixture.project_id)
    .bind(controller_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count governance audit before stale responsibility activation");
    let stale_activation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}/revisions/3/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate stale supersedes responsibility request"),
        )
        .await
        .expect("activate stale supersedes responsibility response");
    assert_eq!(stale_activation.status(), StatusCode::CONFLICT);
    let stale_activation_rolled_back = sqlx::query_scalar::<_, bool>(
        "SELECT count(*) FILTER (WHERE revision = 2 AND state = 'active') = 1
             AND count(*) FILTER (WHERE revision = 3 AND state = 'draft') = 1
         FROM agent_responsibility_contracts
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(responsibility_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify stale responsibility activation rollback");
    assert!(stale_activation_rolled_back);
    let governance_audit_after_stale = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2",
    )
    .bind(fixture.project_id)
    .bind(controller_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count governance audit after stale responsibility activation");
    assert_eq!(governance_audit_after_stale, governance_audit_before_stale);

    // An administrator-controlled agent is governed directly by current
    // project-administrator authority; no administrator→administrator
    // ResponsibilityContract is synthesized.
    let admin_agent_id = Uuid::new_v4();
    let admin_agent_identity_id = Uuid::new_v4();
    let provision_admin_agent = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agents", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": admin_agent_id,
                        "principal_identity_id": admin_agent_identity_id,
                        "controller_identity_id": fixture.owner_id,
                        "identity_handle": format!("admin-agent-{}", admin_agent_identity_id.simple()),
                        "encrypted_profile": encrypted(36),
                        "profile_resource_node_id": fixture.profile_resource_id,
                        "encrypted_system_prompt": encrypted(37),
                        "key_epoch": 1,
                        "availability": "controller_private",
                        "runner_id": Uuid::new_v4(),
                        "runner_device_id": Uuid::new_v4(),
                        "encrypted_runner_label": encrypted(38)
                    })
                    .to_string(),
                ))
                .expect("provision admin-controlled agent request"),
        )
        .await
        .expect("provision admin-controlled agent response");
    assert_eq!(provision_admin_agent.status(), StatusCode::OK);
    let admin_local_goal_id = Uuid::new_v4();
    let admin_local_goal = local_goal_value(
        admin_local_goal_id,
        1,
        admin_agent_identity_id,
        fixture.owner_id,
        fixture.profile_resource_id,
        fixture.profile_resource_id,
        "post_comment",
    );
    let drafted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{admin_agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({"contract": admin_local_goal}).to_string(),
                ))
                .expect("draft admin local goal request"),
        )
        .await
        .expect("draft admin local goal response");
    assert_eq!(drafted.status(), StatusCode::OK);
    let activated = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{admin_agent_id}/local-goals/{admin_local_goal_id}/revisions/1/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate admin local goal request"),
        )
        .await
        .expect("activate admin local goal response");
    assert_eq!(activated.status(), StatusCode::OK);
    let fake_admin_responsibilities = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_responsibility_contracts
         WHERE project_id = $1 AND user_identity_id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.owner_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count fake administrator responsibilities");
    assert_eq!(fake_admin_responsibilities, 0);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn purging_one_of_two_same_user_agents_preserves_shared_governance_and_peer_runtime() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let (controller_id, controller_device_id, _controller_token, _permission_root) =
        add_human_member(&fixture, "member").await;
    let root_resource_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT parent_id FROM resource_nodes WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load common governance root");
    let peer_profile_resource_id = Uuid::new_v4();
    let peer_topic_id = Uuid::new_v4();
    let mut prepare = fixture
        .pool
        .begin()
        .await
        .expect("begin peer profile setup");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *prepare)
        .await
        .expect("disable RLS for peer profile setup");
    sqlx::query(
        "INSERT INTO resource_nodes (
             id, project_id, parent_id, node_kind,
             encrypted_metadata, created_by_identity_id
         ) VALUES ($1, $2, $3, 'topic', decode('01', 'hex'), $4)",
    )
    .bind(peer_profile_resource_id)
    .bind(fixture.project_id)
    .bind(root_resource_id)
    .bind(fixture.owner_id)
    .execute(&mut *prepare)
    .await
    .expect("insert peer profile resource");
    sqlx::query(
        "INSERT INTO resource_epochs (
             project_id, resource_node_id, epoch,
             created_by_identity_id, created_by_device_id,
             created_by_device_key_version, key_commitment, reason
         ) VALUES ($1, $2, 1, $3, $4, 1,
                   decode(repeat('ac', 32), 'hex'), 'created')",
    )
    .bind(fixture.project_id)
    .bind(peer_profile_resource_id)
    .bind(fixture.owner_id)
    .bind(fixture.owner_device_id)
    .execute(&mut *prepare)
    .await
    .expect("insert peer profile epoch");
    sqlx::query(
        "INSERT INTO topics (id, project_id, resource_node_id, encrypted_payload)
         VALUES ($1, $2, $3, decode('01', 'hex'))",
    )
    .bind(peer_topic_id)
    .bind(fixture.project_id)
    .bind(peer_profile_resource_id)
    .execute(&mut *prepare)
    .await
    .expect("insert peer profile topic");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'manage', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(peer_profile_resource_id)
    .bind(controller_id)
    .bind(Uuid::new_v4())
    .bind(fixture.owner_id)
    .execute(&mut *prepare)
    .await
    .expect("grant controller access to peer profile");
    prepare.commit().await.expect("commit peer profile setup");

    let (purged_agent_id, _purged_principal_id, _purged_runner_id, _purged_device_id) =
        provision_controlled_agent(
            &fixture,
            &app,
            controller_id,
            fixture.profile_resource_id,
            61,
        )
        .await;
    let (peer_agent_id, peer_principal_id, peer_runner_id, peer_runner_device_id) =
        provision_controlled_agent(&fixture, &app, controller_id, peer_profile_resource_id, 64)
            .await;

    let responsibility_id = Uuid::new_v4();
    let drafted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "contract": {
                            "id": responsibility_id,
                            "revision": 1,
                            "administrator": fixture.owner_id,
                            "user": controller_id,
                            "encrypted_source_text": encrypted(67),
                            "rules": [{
                                "domain": 1,
                                "scope": root_resource_id,
                                "allowed_actions": ["post_comment"]
                            }],
                            "supersedes_revision": null
                        }
                    })
                    .to_string(),
                ))
                .expect("draft shared user responsibility request"),
        )
        .await
        .expect("draft shared user responsibility response");
    assert_eq!(drafted.status(), StatusCode::OK);
    let activated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}/revisions/1/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate shared user responsibility request"),
        )
        .await
        .expect("activate shared user responsibility response");
    assert_eq!(activated.status(), StatusCode::OK);

    let agents_governed_by_same_active_contract = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM governed_agents agent
         JOIN agent_responsibility_contracts responsibility
           ON responsibility.project_id = agent.project_id
          AND responsibility.user_identity_id = agent.controller_identity_id
          AND responsibility.state = 'active'
         WHERE agent.project_id = $1 AND agent.controller_identity_id = $2",
    )
    .bind(fixture.project_id)
    .bind(controller_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count agents sharing active user responsibility");
    assert_eq!(agents_governed_by_same_active_contract, 2);

    purge_resource_through_retention(&fixture, fixture.profile_resource_id, fixture.owner_id).await;

    let purged_agent_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM governed_agents WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(purged_agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count purged agent");
    assert_eq!(purged_agent_count, 0);
    let peer_runtime_intact = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM governed_agents agent
             JOIN agent_runners runner
               ON runner.project_id = agent.project_id AND runner.agent_id = agent.id
             JOIN identities principal ON principal.id = agent.principal_identity_id
             JOIN devices device
               ON device.identity_id = principal.id AND device.id = runner.device_id
             JOIN sessions session
               ON session.identity_id = principal.id AND session.device_id = device.id
             WHERE agent.project_id = $1 AND agent.id = $2
               AND agent.principal_identity_id = $3 AND runner.id = $4
               AND device.id = $5 AND device.retired_at IS NULL
               AND session.revoked_at IS NULL
         )",
    )
    .bind(fixture.project_id)
    .bind(peer_agent_id)
    .bind(peer_principal_id)
    .bind(peer_runner_id)
    .bind(peer_runner_device_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify peer agent runtime isolation");
    assert!(peer_runtime_intact);
    let shared_governance_intact = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM agent_responsibility_contracts
             WHERE project_id = $1 AND id = $2 AND revision = 1
               AND user_identity_id = $3 AND state = 'active'
         ) AND EXISTS (
             SELECT 1 FROM agent_user_governance_audit_log
             WHERE project_id = $1 AND subject_user_identity_id = $3
               AND event_kind = 'responsibility_activated'
         ) AND EXISTS (
             SELECT 1 FROM sessions
             WHERE identity_id = $3 AND device_id = $4 AND revoked_at IS NULL
         )",
    )
    .bind(fixture.project_id)
    .bind(responsibility_id)
    .bind(controller_id)
    .bind(controller_device_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify shared governance and controller identity isolation");
    assert!(shared_governance_intact);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn cross_owner_review_requires_exact_active_task_provenance_and_current_permission() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let (requester_id, requester_device, requester_token, requester_permission_root) =
        add_human_member(&fixture, "member").await;
    let (controller_id, controller_device, controller_token, _controller_permission_root) =
        add_human_member(&fixture, "member").await;
    let (source_task, review_task) =
        create_cross_owner_tasks(&fixture, requester_id, controller_id).await;
    let agent_id = Uuid::new_v4();
    let agent_identity_id = Uuid::new_v4();
    let provision = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agents", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": agent_id,
                        "principal_identity_id": agent_identity_id,
                        "controller_identity_id": controller_id,
                        "identity_handle": format!("cross-agent-{}", agent_identity_id.simple()),
                        "encrypted_profile": encrypted(50),
                        "profile_resource_node_id": fixture.profile_resource_id,
                        "encrypted_system_prompt": encrypted(51),
                        "key_epoch": 1,
                        "availability": "project_delegable",
                        "runner_id": Uuid::new_v4(),
                        "runner_device_id": Uuid::new_v4(),
                        "encrypted_runner_label": encrypted(52)
                    })
                    .to_string(),
                ))
                .expect("provision cross-owner target request"),
        )
        .await
        .expect("provision cross-owner target response");
    assert_eq!(provision.status(), StatusCode::OK);

    let responsibility_id = Uuid::new_v4();
    let responsibility = json!({
        "id": responsibility_id,
        "revision": 1,
        "administrator": fixture.owner_id,
        "user": controller_id,
        "encrypted_source_text": encrypted(53),
        "rules": [{
            "domain": 1,
            "scope": source_task,
            "allowed_actions": ["assign_own_task"]
        }],
        "supersedes_revision": null
    });
    let drafted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(json!({"contract": responsibility}).to_string()))
                .expect("draft target responsibility request"),
        )
        .await
        .expect("draft target responsibility response");
    assert_eq!(drafted.status(), StatusCode::OK);
    let activated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/users/{controller_id}/responsibilities/{responsibility_id}/revisions/1/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate target responsibility request"),
        )
        .await
        .expect("activate target responsibility response");
    assert_eq!(activated.status(), StatusCode::OK);

    let rejected_assignment_id = Uuid::new_v4();
    let rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/tasks/{review_task}/cross-owner-assignments",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::from(
                    json!({
                        "id": rejected_assignment_id,
                        "target_agent_id": agent_id,
                        "review_task_resource_node_id": null
                    })
                    .to_string(),
                ))
                .expect("route out-of-governance request"),
        )
        .await
        .expect("route out-of-governance response");
    assert_eq!(rejected.status(), StatusCode::OK);
    assert_eq!(json_body(rejected).await["route"], "rejected");

    let assignment_id = Uuid::new_v4();
    let routed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/tasks/{source_task}/cross-owner-assignments",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::from(
                    json!({
                        "id": assignment_id,
                        "target_agent_id": agent_id,
                        "review_task_resource_node_id": review_task
                    })
                    .to_string(),
                ))
                .expect("route controller review request"),
        )
        .await
        .expect("route controller review response");
    assert_eq!(routed.status(), StatusCode::OK);
    assert_eq!(json_body(routed).await["state"], "pending_review");
    let decided = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/cross-owner-assignments/{assignment_id}/decision",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::from(json!({"decision": "approved"}).to_string()))
                .expect("approve controller review request"),
        )
        .await
        .expect("approve controller review response");
    assert_eq!(decided.status(), StatusCode::OK);
    assert_eq!(
        json_body(decided).await["state"],
        "approved_pending_mandate"
    );
    let finalize_uri = format!(
        "/v1/projects/{}/cross-owner-assignments/{assignment_id}/finalize",
        fixture.project_id
    );
    let approval_alone = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&finalize_uri)
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::empty())
                .expect("finalize without mandate request"),
        )
        .await
        .expect("finalize without mandate response");
    assert_eq!(approval_alone.status(), StatusCode::CONFLICT);

    let local_goal_id = Uuid::new_v4();
    let wrong_task_local = local_goal_value(
        local_goal_id,
        1,
        agent_identity_id,
        controller_id,
        source_task,
        review_task,
        "assign_own_task",
    );
    let drafted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::from(
                    json!({"contract": wrong_task_local}).to_string(),
                ))
                .expect("draft wrong-task local goal request"),
        )
        .await
        .expect("draft wrong-task local goal response");
    assert_eq!(drafted.status(), StatusCode::OK);
    let activate_local_uri = format!(
        "/v1/projects/{}/agents/{agent_id}/local-goals/{local_goal_id}/revisions/1/activate",
        fixture.project_id
    );
    let activated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&activate_local_uri)
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::empty())
                .expect("activate wrong-task local goal request"),
        )
        .await
        .expect("activate wrong-task local goal response");
    assert_eq!(activated.status(), StatusCode::OK);
    let wrong_task_not_ready = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&finalize_uri)
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::empty())
                .expect("finalize wrong-task mandate request"),
        )
        .await
        .expect("finalize wrong-task mandate response");
    assert_eq!(wrong_task_not_ready.status(), StatusCode::CONFLICT);
    let still_pending = sqlx::query_scalar::<_, String>(
        "SELECT state FROM agent_cross_owner_assignments
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(assignment_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load assignment after wrong task activation");
    assert_eq!(still_pending, "approved_pending_mandate");

    let exact_task_local = local_goal_value(
        local_goal_id,
        2,
        agent_identity_id,
        controller_id,
        source_task,
        source_task,
        "assign_own_task",
    );
    let drafted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::from(
                    json!({"contract": exact_task_local}).to_string(),
                ))
                .expect("draft exact-task local goal request"),
        )
        .await
        .expect("draft exact-task local goal response");
    assert_eq!(drafted.status(), StatusCode::OK);
    let activated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goals/{local_goal_id}/revisions/2/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::empty())
                .expect("activate exact-task local goal request"),
        )
        .await
        .expect("activate exact-task local goal response");
    assert_eq!(activated.status(), StatusCode::OK);

    let provenance_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_task_obligation_provenance
         WHERE project_id = $1 AND task_resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exact task obligation provenance");
    assert_eq!(provenance_count, 1);

    sqlx::query(
        r#"
        DO $role$
        BEGIN
            IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'sprout_governance_app') THEN
                CREATE ROLE sprout_governance_app NOSUPERUSER NOBYPASSRLS NOLOGIN;
            END IF;
        END
        $role$
        "#,
    )
    .execute(&fixture.pool)
    .await
    .expect("create non-bypass governance app role");
    let app_role_is_unprivileged = sqlx::query_scalar::<_, bool>(
        "SELECT NOT rolsuper AND NOT rolbypassrls
         FROM pg_roles WHERE rolname = 'sprout_governance_app'",
    )
    .fetch_one(&fixture.pool)
    .await
    .expect("verify governance app role attributes");
    assert!(app_role_is_unprivileged);
    for grant in [
        "GRANT sprout_governance_app TO sprout_test",
        "GRANT SELECT, DELETE ON agent_task_obligation_provenance TO sprout_governance_app",
        "GRANT SELECT ON agent_cross_owner_assignments, governed_agents TO sprout_governance_app",
    ] {
        sqlx::query(grant)
            .execute(&fixture.pool)
            .await
            .expect("grant minimal governance app test privilege");
    }
    let mut forged_retention = fixture
        .pool
        .begin()
        .await
        .expect("begin forged retention test");
    sqlx::query("SET LOCAL ROLE sprout_governance_app")
        .execute(&mut *forged_retention)
        .await
        .expect("assume non-bypass app role");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true),
                set_config('app.agent_retention_resource_id', $4, true)",
    )
    .bind(controller_id.to_string())
    .bind(controller_device.to_string())
    .bind(fixture.project_id.to_string())
    .bind(source_task.to_string())
    .execute(&mut *forged_retention)
    .await
    .expect("set forged retention identifier without a lease");
    sqlx::query(
        r#"
        DO $delete$
        BEGIN
            BEGIN
                DELETE FROM agent_task_obligation_provenance
                WHERE project_id = '30000000-0000-0000-0000-000000000000'::uuid;
            EXCEPTION WHEN SQLSTATE '55000' THEN
                NULL;
            END;
            BEGIN
                DELETE FROM agent_task_obligation_provenance
                WHERE project_id = current_setting('app.project_id')::uuid;
                RAISE EXCEPTION 'forged retention GUC authorized provenance deletion';
            EXCEPTION WHEN SQLSTATE '55000' THEN
                NULL;
            END;
        END
        $delete$
        "#,
    )
    .execute(&mut *forged_retention)
    .await
    .expect("manual retention GUC must fail at append-only trigger");
    forged_retention
        .commit()
        .await
        .expect("commit forged retention negative test");
    let provenance_after_forgery = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_task_obligation_provenance
         WHERE project_id = $1 AND task_resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count provenance after forged retention GUC");
    assert_eq!(provenance_after_forgery, 1);

    let mut revoke = fixture.pool.begin().await.expect("begin requester revoke");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(fixture.owner_id.to_string())
    .bind(fixture.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *revoke)
    .await
    .expect("set owner context for revoke");
    sqlx::query_scalar::<_, i64>(
        "SELECT sprout_private.revoke_hierarchical_permission($1, $2, $3, $4, NULL)",
    )
    .bind(fixture.project_id)
    .bind(requester_permission_root)
    .bind(requester_id)
    .bind(fixture.owner_id)
    .fetch_one(&mut *revoke)
    .await
    .expect("revoke requester permission");
    revoke.commit().await.expect("commit requester revoke");
    let revoked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&finalize_uri)
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::empty())
                .expect("finalize after permission revoke request"),
        )
        .await
        .expect("finalize after permission revoke response");
    assert_eq!(revoked.status(), StatusCode::FORBIDDEN);

    let requester_regrant_id = Uuid::new_v4();
    let mut regrant = fixture.pool.begin().await.expect("begin requester regrant");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(fixture.owner_id.to_string())
    .bind(fixture.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *regrant)
    .await
    .expect("set owner context for regrant");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'manage', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(requester_id)
    .bind(requester_regrant_id)
    .bind(fixture.owner_id)
    .execute(&mut *regrant)
    .await
    .expect("regrant requester permission");
    regrant.commit().await.expect("commit requester regrant");
    let ready = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&finalize_uri)
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::empty())
                .expect("finalize exact mandate request"),
        )
        .await
        .expect("finalize exact mandate response");
    assert_eq!(ready.status(), StatusCode::OK);
    assert_eq!(json_body(ready).await["state"], "ready");

    let materialize_uri = format!(
        "/v1/projects/{}/cross-owner-assignments/{assignment_id}/materialize",
        fixture.project_id
    );
    let effect_id = Uuid::new_v4();
    let task_assignment_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();
    let materialize_body = json!({
        "effect_id": effect_id,
        "task_assignment_id": task_assignment_id,
        "idempotency_key": idempotency_key,
        "encrypted_assignment_payload_b64": "AQ=="
    });
    let target_permission_roots_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(DISTINCT root_grant_id)
         FROM sprout_private.domain_permission_rows
         WHERE project_id = $1 AND member_identity_id = $2 AND revoked_at IS NULL",
    )
    .bind(fixture.project_id)
    .bind(agent_identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count target permission roots before materialization");

    let mut revoke_after_ready = fixture
        .pool
        .begin()
        .await
        .expect("begin requester revoke after ready");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(fixture.owner_id.to_string())
    .bind(fixture.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *revoke_after_ready)
    .await
    .expect("set owner context for post-ready revoke");
    sqlx::query_scalar::<_, i64>(
        "SELECT sprout_private.revoke_hierarchical_permission($1, $2, $3, $4, NULL)",
    )
    .bind(fixture.project_id)
    .bind(requester_regrant_id)
    .bind(requester_id)
    .bind(fixture.owner_id)
    .fetch_one(&mut *revoke_after_ready)
    .await
    .expect("revoke requester permission after ready");
    revoke_after_ready
        .commit()
        .await
        .expect("commit requester revoke after ready");

    let mut snapshot_without_manage = fixture
        .pool
        .begin()
        .await
        .expect("begin direct snapshot permission negative");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(requester_id.to_string())
    .bind(requester_device.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *snapshot_without_manage)
    .await
    .expect("set requester context for direct snapshot negative");
    let snapshot_error =
        sqlx::query("SELECT * FROM sprout_private.cross_owner_materialization_snapshot($1, $2)")
            .bind(fixture.project_id)
            .bind(assignment_id)
            .fetch_all(&mut *snapshot_without_manage)
            .await
            .expect_err("SECURITY DEFINER snapshot must reject caller without current manage");
    assert_eq!(
        snapshot_error
            .as_database_error()
            .and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("42501"))
    );
    snapshot_without_manage
        .rollback()
        .await
        .expect("rollback direct snapshot negative");

    let revoked_materialization = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&materialize_uri)
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::from(materialize_body.to_string()))
                .expect("materialize after permission revoke request"),
        )
        .await
        .expect("materialize after permission revoke response");
    assert_eq!(revoked_materialization.status(), StatusCode::FORBIDDEN);
    let effects_after_revoke = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_cross_owner_assignment_effects
         WHERE project_id = $1 AND cross_owner_assignment_id = $2",
    )
    .bind(fixture.project_id)
    .bind(assignment_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count effects after revoked materialization");
    assert_eq!(effects_after_revoke, 0);

    let mut restore_after_ready = fixture
        .pool
        .begin()
        .await
        .expect("begin requester restore after ready");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(fixture.owner_id.to_string())
    .bind(fixture.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *restore_after_ready)
    .await
    .expect("set owner context for requester restore after ready");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'manage', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(requester_id)
    .bind(Uuid::new_v4())
    .bind(fixture.owner_id)
    .execute(&mut *restore_after_ready)
    .await
    .expect("restore requester permission after ready");
    restore_after_ready
        .commit()
        .await
        .expect("commit requester restore after ready");

    let materialized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&materialize_uri)
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::from(materialize_body.to_string()))
                .expect("materialize ready cross-owner request"),
        )
        .await
        .expect("materialize ready cross-owner response");
    assert_eq!(materialized.status(), StatusCode::OK);
    let materialized = json_body(materialized).await;
    assert_eq!(materialized["effect_id"], effect_id.to_string());
    assert_eq!(
        materialized["task_assignment_id"],
        task_assignment_id.to_string()
    );
    assert_eq!(materialized["replayed"], false);

    let replayed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&materialize_uri)
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::from(materialize_body.to_string()))
                .expect("retry materialization request"),
        )
        .await
        .expect("retry materialization response");
    assert_eq!(replayed.status(), StatusCode::OK);
    assert_eq!(json_body(replayed).await["replayed"], true);

    let hash_mismatch = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&materialize_uri)
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::from(
                    json!({
                        "effect_id": effect_id,
                        "task_assignment_id": task_assignment_id,
                        "idempotency_key": idempotency_key,
                        "encrypted_assignment_payload_b64": "Ag=="
                    })
                    .to_string(),
                ))
                .expect("hash-mismatched retry request"),
        )
        .await
        .expect("hash-mismatched retry response");
    assert_eq!(hash_mismatch.status(), StatusCode::CONFLICT);

    let (effect_count, task_assignment_count, assignment_owns_permission) =
        sqlx::query_as::<_, (i64, i64, bool)>(
            "SELECT
                 (SELECT count(*) FROM agent_cross_owner_assignment_effects
                  WHERE project_id = $1 AND cross_owner_assignment_id = $2),
                 (SELECT count(*) FROM task_assignments
                  WHERE project_id = $1 AND id = $3),
                 (SELECT permission_managed_by_assignment FROM task_assignments
                  WHERE project_id = $1 AND id = $3)",
        )
        .bind(fixture.project_id)
        .bind(assignment_id)
        .bind(task_assignment_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("load materialization idempotency result");
    assert_eq!(effect_count, 1);
    assert_eq!(task_assignment_count, 1);
    assert!(!assignment_owns_permission);
    let target_permission_roots_after = sqlx::query_scalar::<_, i64>(
        "SELECT count(DISTINCT root_grant_id)
         FROM sprout_private.domain_permission_rows
         WHERE project_id = $1 AND member_identity_id = $2 AND revoked_at IS NULL",
    )
    .bind(fixture.project_id)
    .bind(agent_identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count target permission roots after materialization");
    assert_eq!(
        target_permission_roots_after,
        target_permission_roots_before
    );

    let preexisting_target_permission = Uuid::new_v4();
    let mut add_preexisting_target_permission = fixture
        .pool
        .begin()
        .await
        .expect("begin pre-existing target permission setup");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *add_preexisting_target_permission)
        .await
        .expect("disable RLS for pre-existing target permission setup");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'view', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(agent_identity_id)
    .bind(preexisting_target_permission)
    .bind(fixture.owner_id)
    .execute(&mut *add_preexisting_target_permission)
    .await
    .expect("create pre-existing target permission after materialization");
    add_preexisting_target_permission
        .commit()
        .await
        .expect("commit pre-existing target permission setup");
    let (source_task_id, source_epoch_id, source_epoch) = sqlx::query_as::<_, (Uuid, Uuid, i32)>(
        "SELECT task.id, epoch.id, epoch.epoch
             FROM tasks task
             JOIN resource_epochs epoch
               ON epoch.project_id = task.project_id
              AND epoch.resource_node_id = task.resource_node_id
              AND epoch.retired_at IS NULL
             WHERE task.project_id = $1 AND task.resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("load task and key epoch before cross-owner revocation");
    let revoked_assignment = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/v1/projects/{}/tasks/{source_task_id}/assignments/{task_assignment_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {requester_token}"))
                .body(Body::from(
                    json!({
                        "rotations": [],
                        "idempotency_key": "cross-owner-revoke-isolation"
                    })
                    .to_string(),
                ))
                .expect("revoke cross-owner assignment request"),
        )
        .await
        .expect("revoke cross-owner assignment response");
    assert_eq!(revoked_assignment.status(), StatusCode::OK);
    let preexisting_permission_still_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM sprout_private.domain_permission_rows
             WHERE project_id = $1 AND member_identity_id = $2
               AND root_grant_id = $3 AND revoked_at IS NULL
         )",
    )
    .bind(fixture.project_id)
    .bind(agent_identity_id)
    .bind(preexisting_target_permission)
    .fetch_one(&fixture.pool)
    .await
    .expect("check pre-existing target permission after assignment revoke");
    assert!(preexisting_permission_still_active);
    let current_epoch_after_revoke = sqlx::query_as::<_, (Uuid, i32)>(
        "SELECT id, epoch FROM resource_epochs
         WHERE project_id = $1 AND resource_node_id = $2 AND retired_at IS NULL",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("load task key epoch after cross-owner assignment revoke");
    assert_eq!(current_epoch_after_revoke, (source_epoch_id, source_epoch));

    let retention_subject_id = Uuid::new_v4();
    let retention_lease_token = Uuid::new_v4();
    let retention_now = Utc::now();
    let deleted_at = retention_now - ChronoDuration::days(20);
    let semantic_provenance_before_purge = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM sprout_private.semantic_task_obligation_provenance_projection
         WHERE project_id = $1 AND task_resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count semantic provenance before retention purge");
    let semantic_intents_before_purge = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM sprout_private.semantic_task_intent_projection
         WHERE project_id = $1 AND task_resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count semantic intents before retention purge");
    assert_eq!(semantic_provenance_before_purge, 1);
    assert_eq!(semantic_intents_before_purge, 1);
    let mut retention = fixture
        .pool
        .begin()
        .await
        .expect("begin legitimate provenance retention");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *retention)
        .await
        .expect("disable RLS for retention fixture setup");
    sqlx::query(
        "UPDATE resource_nodes SET deleted_at = $3
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .bind(deleted_at)
    .execute(&mut *retention)
    .await
    .expect("soft-delete task resource for retention");
    sqlx::query(
        r#"
        INSERT INTO retention_subjects (
            id, project_id, source_kind, source_id, resource_node_id,
            owner_identity_id, retention_class, source_at, warning_at,
            purge_at, state, lease_owner, lease_token, leased_until
        ) VALUES (
            $1, $2, 'resource_deleted', $3, $3, $4,
            'deleted_or_obsolete', $5, $6, $7, 'purging', $8, $9, $10
        )
        "#,
    )
    .bind(retention_subject_id)
    .bind(fixture.project_id)
    .bind(source_task)
    .bind(requester_id)
    .bind(deleted_at)
    .bind(deleted_at + ChronoDuration::days(1))
    .bind(deleted_at + ChronoDuration::days(15))
    .bind(Uuid::new_v4())
    .bind(retention_lease_token)
    .bind(retention_now + ChronoDuration::hours(1))
    .execute(&mut *retention)
    .await
    .expect("insert legitimate retention lease");
    retention
        .commit()
        .await
        .expect("commit legitimate retention setup");
    let purged =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.purge_retention_subject($1, $2, $3)")
            .bind(retention_subject_id)
            .bind(retention_lease_token)
            .bind(retention_now)
            .fetch_one(&fixture.pool)
            .await
            .expect("execute legitimate retention purge");
    assert!(purged);
    let provenance_after_purge = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_task_obligation_provenance
         WHERE project_id = $1 AND task_resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count provenance after legitimate retention purge");
    assert_eq!(provenance_after_purge, 0);
    let semantic_provenance_after_purge = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM sprout_private.semantic_task_obligation_provenance_projection
         WHERE project_id = $1 AND task_resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count retained semantic provenance after purge");
    let semantic_intents_after_purge = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM sprout_private.semantic_task_intent_projection
         WHERE project_id = $1 AND task_resource_node_id = $2",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count retained semantic intents after purge");
    assert_eq!(
        semantic_provenance_after_purge,
        semantic_provenance_before_purge
    );
    assert_eq!(semantic_intents_after_purge, semantic_intents_before_purge);
    let retained_effects = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_product_effect_retained_history
         WHERE project_id = $1 AND task_resource_node_id = $2
           AND effect_kind = 'cross_owner_assignment'",
    )
    .bind(fixture.project_id)
    .bind(source_task)
    .fetch_one(&fixture.pool)
    .await
    .expect("count retained cross-owner effect provenance");
    assert_eq!(retained_effects, 1);
}
