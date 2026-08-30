use std::{env, sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header::CONTENT_TYPE},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sprout_server::{
    AppState, build_router,
    config::Config,
    worker::{self, WorkerKind, WorkerOptions},
};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tokio::sync::watch;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct AgentFixture {
    agent_id: Uuid,
    identity_id: Uuid,
    token: String,
}

struct Fixture {
    pool: PgPool,
    owner_id: Uuid,
    owner_device_id: Uuid,
    owner_token: String,
    project_id: Uuid,
    scope_resource_id: Uuid,
    topic_id: Uuid,
    agents: [AgentFixture; 2],
}

fn token(identity_id: Uuid, session_id: Uuid) -> String {
    format!("v1.{identity_id}.{session_id}.{}", "a".repeat(64))
}

fn encrypted(seed: u8) -> Value {
    json!({
        "version": 1,
        "algorithm": "aes-256-gcm",
        "key_id": format!("completion-gate-key-{seed}"),
        "nonce": vec![seed; 12],
        "ciphertext": vec![seed, seed.wrapping_add(1)]
    })
}

async fn fixture() -> Fixture {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to a migrated disposable PostgreSQL database");
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&database_url)
        .await
        .expect("connect completion gate database");
    let owner_id = Uuid::new_v4();
    let owner_device_id = Uuid::new_v4();
    let owner_session_id = Uuid::new_v4();
    let owner_token = token(owner_id, owner_session_id);
    let project_id = Uuid::new_v4();
    let root_resource_id = Uuid::new_v4();
    let scope_resource_id = Uuid::new_v4();
    let topic_id = Uuid::new_v4();
    let agents = [agent_fixture(), agent_fixture()];

    let mut transaction = pool.begin().await.expect("begin completion fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for fixture provisioning");
    insert_identity_session(
        &mut transaction,
        owner_id,
        owner_device_id,
        owner_session_id,
        &owner_token,
        "user",
        "web",
        0x11,
    )
    .await;
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
        (scope_resource_id, Some(root_resource_id), "topic"),
    ] {
        insert_resource(
            &mut transaction,
            project_id,
            id,
            parent,
            kind,
            owner_id,
            owner_device_id,
        )
        .await;
    }
    sqlx::query(
        "INSERT INTO topics (id, project_id, resource_node_id, encrypted_payload)
         VALUES ($1, $2, $3, decode('01', 'hex'))",
    )
    .bind(topic_id)
    .bind(project_id)
    .bind(scope_resource_id)
    .execute(&mut *transaction)
    .await
    .expect("insert scope topic");

    for (index, agent) in agents.iter().enumerate() {
        let device_id = Uuid::new_v4();
        let session_id = token_session(&agent.token);
        insert_identity_session(
            &mut transaction,
            agent.identity_id,
            device_id,
            session_id,
            &agent.token,
            "agent",
            "service",
            u8::try_from(0x30 + index).expect("fixture seed"),
        )
        .await;
        sqlx::query(
            "INSERT INTO project_memberships (project_id, identity_id, role)
             VALUES ($1, $2, 'member')",
        )
        .bind(project_id)
        .bind(agent.identity_id)
        .execute(&mut *transaction)
        .await
        .expect("insert agent membership");
        sqlx::query(
            r#"
            INSERT INTO governed_agents (
                id, project_id, principal_identity_id, controller_identity_id,
                profile_resource_node_id, encrypted_system_prompt, key_epoch,
                availability
            ) VALUES ($1, $2, $3, $4, $5, decode('01', 'hex'), 1,
                      'controller_private')
            "#,
        )
        .bind(agent.agent_id)
        .bind(project_id)
        .bind(agent.identity_id)
        .bind(owner_id)
        .bind(scope_resource_id)
        .execute(&mut *transaction)
        .await
        .expect("insert governed agent");
        sqlx::query(
            r#"
            INSERT INTO agent_runners (
                id, project_id, agent_id, principal_identity_id, device_id,
                activated_key_version, state, activated_at
            ) VALUES ($1, $2, $3, $4, $5, 1, 'active', clock_timestamp())
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(agent.agent_id)
        .bind(agent.identity_id)
        .bind(device_id)
        .execute(&mut *transaction)
        .await
        .expect("insert active edge runner");
        sqlx::query(
            "SELECT sprout_private.grant_hierarchical_permission(
                 $1, $2, $3, 'edit', 'full', 'restricted', $4, $5
             )",
        )
        .bind(project_id)
        .bind(scope_resource_id)
        .bind(agent.identity_id)
        .bind(Uuid::new_v4())
        .bind(owner_id)
        .execute(&mut *transaction)
        .await
        .expect("grant agent permission through the product permission engine");
    }
    transaction
        .commit()
        .await
        .expect("commit completion fixture");
    Fixture {
        pool,
        owner_id,
        owner_device_id,
        owner_token,
        project_id,
        scope_resource_id,
        topic_id,
        agents,
    }
}

fn agent_fixture() -> AgentFixture {
    let identity_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    AgentFixture {
        agent_id: Uuid::new_v4(),
        identity_id,
        token: token(identity_id, session_id),
    }
}

fn token_session(value: &str) -> Uuid {
    value
        .split('.')
        .nth(2)
        .expect("session token segment")
        .parse()
        .expect("session UUID")
}

#[allow(clippy::too_many_arguments)]
async fn insert_identity_session(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity_id: Uuid,
    device_id: Uuid,
    session_id: Uuid,
    session_token: &str,
    principal_kind: &str,
    device_kind: &str,
    key_seed: u8,
) {
    sqlx::query(
        "INSERT INTO identities (
             id, identity_handle, encrypted_profile, principal_kind
         ) VALUES ($1, $2, decode('01', 'hex'), $3)",
    )
    .bind(identity_id)
    .bind(format!("completion-gate-{}", identity_id.simple()))
    .bind(principal_kind)
    .execute(&mut **transaction)
    .await
    .expect("insert identity");
    sqlx::query(
        "INSERT INTO devices (
             id, identity_id, device_kind, encrypted_label, trust_state
         ) VALUES ($1, $2, $3, decode('01', 'hex'), 'trusted')",
    )
    .bind(device_id)
    .bind(identity_id)
    .bind(device_kind)
    .execute(&mut **transaction)
    .await
    .expect("insert device");
    sqlx::query(
        r#"
        INSERT INTO device_keys (
            identity_id, device_id, key_version,
            encryption_public_key, signing_public_key,
            previous_package_hash, package_hash,
            x25519_public_key, ed25519_public_key
        ) VALUES (
            $1, $2, 1, $3, $4, decode(repeat('00', 32), 'hex'),
            digest($2::text, 'sha256'), $3, $4
        )
        "#,
    )
    .bind(identity_id)
    .bind(device_id)
    .bind(vec![key_seed; 32])
    .bind(vec![key_seed.wrapping_add(1); 32])
    .execute(&mut **transaction)
    .await
    .expect("insert device key");
    sqlx::query(
        "INSERT INTO sessions (id, identity_id, device_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4, clock_timestamp() + interval '1 hour')",
    )
    .bind(session_id)
    .bind(identity_id)
    .bind(device_id)
    .bind(Sha256::digest(session_token.as_bytes()).to_vec())
    .execute(&mut **transaction)
    .await
    .expect("insert session");
}

#[allow(clippy::too_many_arguments)]
async fn insert_resource(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    resource_id: Uuid,
    parent_id: Option<Uuid>,
    kind: &str,
    owner_id: Uuid,
    owner_device_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO resource_nodes (
             id, project_id, parent_id, node_kind,
             encrypted_metadata, created_by_identity_id
         ) VALUES ($1, $2, $3, $4, decode('01', 'hex'), $5)",
    )
    .bind(resource_id)
    .bind(project_id)
    .bind(parent_id)
    .bind(kind)
    .bind(owner_id)
    .execute(&mut **transaction)
    .await
    .expect("insert resource node");
    sqlx::query(
        "INSERT INTO resource_epochs (
             project_id, resource_node_id, epoch,
             created_by_identity_id, created_by_device_id,
             created_by_device_key_version, key_commitment, reason
         ) VALUES ($1, $2, 1, $3, $4, 1,
                   decode(repeat('aa', 32), 'hex'), 'created')",
    )
    .bind(project_id)
    .bind(resource_id)
    .bind(owner_id)
    .bind(owner_device_id)
    .execute(&mut **transaction)
    .await
    .expect("insert resource epoch");
}

fn app(fixture: &Fixture, lease: Duration) -> axum::Router {
    let mut config = Config::for_test();
    config.database_max_connections = 12;
    config.body_limit_bytes = 64 * 1024;
    config.blob_max_file_bytes = 64 * 1024;
    config.blob_project_quota_bytes = 256 * 1024;
    config.agent_work_lease = lease;
    build_router(Arc::new(
        AppState::new(config, fixture.pool.clone()).expect("completion gate app state"),
    ))
    .expect("completion gate router")
}

async fn request_json(
    app: axum::Router,
    method: Method,
    uri: String,
    bearer: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::from(body.to_string()))
                .expect("JSON request"),
        )
        .await
        .expect("JSON response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .expect("read JSON response");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "unparsed": String::from_utf8_lossy(&bytes).into_owned() }))
    };
    (status, body)
}

type WorkFixture<'a> = (Uuid, Uuid, u64, Value, bool, Vec<u64>, u32, &'a str);

fn goal_contract(scope: Uuid, work: &[WorkFixture<'_>]) -> Value {
    let goal = Uuid::new_v4();
    let obligations = work
        .iter()
        .filter(|(_, _, _, _, entry, _, _, _)| *entry)
        .map(|(owner, obligation, _, _, _, _, _, _)| {
            json!({
                "id": obligation,
                "goal": goal,
                "owner": owner,
                "activation": { "kind": "always" },
                "required_for_completion": { "kind": "always" },
                "dependency_rank": 0
            })
        })
        .collect::<Vec<_>>();
    let work_specs = work
        .iter()
        .map(
            |(owner, obligation, id, activation, entry, continuations, generation_rank, kind)| {
                json!({
                    "id": id,
                    "obligation": obligation,
                    "owner": owner,
                    "kind": kind,
                    "activation": activation,
                    "allowed_actions": ["replace_own_task"],
                    "max_instances": 1,
                    "max_attempts": 3,
                    "max_resolution_ticks": 4,
                    "generation_rank": generation_rank,
                    "is_entry": entry,
                    "continuations": continuations,
                    "failure_plan": { "kind": "retry_same" }
                })
            },
        )
        .collect::<Vec<_>>();
    let evidence_rules = work
        .iter()
        .filter(|(_, _, _, _, entry, _, _, _)| *entry)
        .enumerate()
        .map(|(index, (_, obligation, work_spec_id, _, _, _, _, _))| {
            json!({
                "id": u64::try_from(index + 1).expect("evidence id"),
                "obligation": obligation,
                "kind": "task_completed",
                "subject": { "kind": "work_result", "work_spec_id": work_spec_id },
                "verification": "mechanical"
            })
        })
        .collect::<Vec<_>>();
    json!({
        "goal": goal,
        "scope": scope,
        "obligations": obligations,
        "dependencies": [],
        "work_specs": work_specs,
        "evidence_rules": evidence_rules,
        "waiting_rules": [],
        "completion_condition": { "kind": "always" }
    })
}

async fn insert_local_goal(fixture: &Fixture, agent: &AgentFixture, contract: Value) -> Uuid {
    let local_goal_id = Uuid::new_v4();
    let compilation_certificate_id = Uuid::new_v4();
    let draft_id = Uuid::new_v4();
    let authorization_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();
    let work_spec_ids = contract["work_specs"]
        .as_array()
        .expect("work specs")
        .iter()
        .map(|spec| spec["id"].clone())
        .collect::<Vec<_>>();
    let requirements = contract["work_specs"]
        .as_array()
        .expect("work specs")
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            json!({
                "id": u64::try_from(index + 1).expect("requirement id"),
                "scope": fixture.scope_resource_id,
                "required_actions": spec["allowed_actions"],
                "required_tools": [],
                "required_for_completion": true
            })
        })
        .collect::<Vec<_>>();
    let bindings = contract["work_specs"]
        .as_array()
        .expect("work specs")
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            json!({
                "requirement_id": u64::try_from(index + 1).expect("requirement id"),
                "obligation": spec["obligation"],
                "work_spec_id": spec["id"]
            })
        })
        .collect::<Vec<_>>();
    let security_policies = contract["work_specs"]
        .as_array()
        .expect("work specs")
        .iter()
        .map(|spec| {
            let allowed_operations = spec["allowed_actions"]
                .as_array()
                .expect("allowed actions")
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|action| match action {
                    "create_task"
                    | "replace_own_task"
                    | "delete_own_task"
                    | "append_assigned_note"
                    | "add_assigned_attachment" => Some("write"),
                    "assign_own_task" | "unassign_own_task" => Some("delegate_assigned_work"),
                    "mark_assigned_done" => Some("complete_assigned_task"),
                    "post_comment" => Some("post_comment"),
                    "invoke_tool" | "retry_tool" => None,
                    unsupported => panic!("unsupported fixture action {unsupported}"),
                })
                .collect::<Vec<_>>();
            json!({
                "work_spec_id": spec["id"],
                "allowed_operations": allowed_operations,
                "allowed_tools": []
            })
        })
        .collect::<Vec<_>>();
    let compiler_output = json!({
        "contract": contract,
        "requirements": requirements,
        "bindings": bindings,
        "security_policies": security_policies
    });
    let local = json!({
        "id": local_goal_id,
        "revision": 1,
        "agent": agent.identity_id,
        "controller": fixture.owner_id,
        "encrypted_prompt": encrypted(0x71),
        "contract": contract,
        "clauses": [{
            "id": 1,
            "domain": 1,
            "scope": fixture.scope_resource_id,
            "work_spec_ids": work_spec_ids
        }],
        "origin": { "kind": "controller_prompt" },
        "supersedes_revision": null
    });
    let contract_hash = Sha256::digest(local.to_string().as_bytes()).to_vec();
    let compiler_output_hash = Sha256::digest(compiler_output.to_string().as_bytes()).to_vec();
    let classifier_output_hash = Sha256::digest(
        serde_json::to_vec(&local["clauses"]).expect("serialize fixture classifier output"),
    )
    .to_vec();
    let mut transaction = fixture
        .pool
        .begin()
        .await
        .expect("begin certified local goal fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("set migration-owner LocalGoal fixture boundary");
    sqlx::query(
        "UPDATE agent_local_goal_contracts
         SET state = 'superseded', terminal_at = clock_timestamp()
         WHERE project_id = $1 AND agent_id = $2 AND state = 'active'",
    )
    .bind(fixture.project_id)
    .bind(agent.agent_id)
    .execute(&mut *transaction)
    .await
    .expect("supersede prior fixture local goal");
    // The completion gates exercise the persistent R5.30 kernel, not endpoint
    // signature acceptance. The migration-owner fixture supplies the minimal
    // exact structural witness required by 0029; application roles cannot
    // insert a verified certificate directly, and dedicated API tests cover
    // compilation signature and initial-creation authorization end to end.
    sqlx::query(
        r#"
        INSERT INTO agent_compilation_certificates (
            id, project_id, task_kind, compiler_name, compiler_version,
            compiler_build_digest, signer_identity_id, signer_device_id,
            signer_device_key_version, subject_id, subject_revision, draft_id,
            agent_principal_identity_id, controller_identity_id,
            input_commitment, ciphertext_commitment, canonical_output, output_hash,
            compilation_envelope, envelope_hash, certificate_hash, idempotency_key,
            classical_signature, post_quantum_signature, classifier_version,
            classifier_output_hash, authorization_kind, authorization_id,
            verification_state, verified_at
        ) VALUES (
            $1, $2, 'local_goal', 'sprout.local-goal.compiler', 1,
            decode('0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2', 'hex'),
            $3, $4, 1, $5, 1, $6, $7, $3,
            decode(repeat('71', 32), 'hex'), $8, $9, $10,
            '{}'::jsonb, decode(repeat('72', 32), 'hex'),
            decode(repeat('73', 32), 'hex'), $11,
            decode(repeat('74', 64), 'hex'), decode('75', 'hex'), 1,
            $12, 'administrator_creation', $13, 'verified', clock_timestamp()
        )
        "#,
    )
    .bind(compilation_certificate_id)
    .bind(fixture.project_id)
    .bind(fixture.owner_id)
    .bind(fixture.owner_device_id)
    .bind(local_goal_id)
    .bind(draft_id)
    .bind(agent.identity_id)
    .bind(&contract_hash)
    .bind(&compiler_output)
    .bind(&compiler_output_hash)
    .bind(idempotency_key)
    .bind(&classifier_output_hash)
    .bind(authorization_id)
    .execute(&mut *transaction)
    .await
    .expect("insert preexisting exact LocalGoal compiler witness");
    sqlx::query(
        r#"
        INSERT INTO agent_local_goal_contracts (
            id, project_id, agent_id, agent_identity_id,
            controller_identity_id, revision, contract, contract_hash,
            compilation_certificate_id, classifier_version,
            classifier_output_hash
        ) VALUES ($1, $2, $3, $4, $5, 1, $6, $7, $8, 1, $9)
        "#,
    )
    .bind(local_goal_id)
    .bind(fixture.project_id)
    .bind(agent.agent_id)
    .bind(agent.identity_id)
    .bind(fixture.owner_id)
    .bind(&local)
    .bind(&contract_hash)
    .bind(compilation_certificate_id)
    .bind(&classifier_output_hash)
    .execute(&mut *transaction)
    .await
    .expect("insert active local goal fixture");
    transaction
        .commit()
        .await
        .expect("commit certified local goal fixture");
    local_goal_id
}

async fn insert_global_contract(
    fixture: &Fixture,
    contract: Value,
    local_sources: &[(AgentFixture, Uuid)],
) -> Uuid {
    let global_id = Uuid::new_v4();
    let contributions = local_sources
        .iter()
        .map(|(agent, _)| {
            let work_ids = contract["work_specs"]
                .as_array()
                .expect("global work specs")
                .iter()
                .filter(|spec| spec["owner"] == agent.identity_id.to_string())
                .map(|spec| spec["id"].clone())
                .collect::<Vec<_>>();
            json!({
                "agent": agent.identity_id,
                "local_revision": 1,
                "local_clause_id": 1,
                "global_work_spec_ids": work_ids
            })
        })
        .collect::<Vec<_>>();
    let candidate = json!({
        "revision": 1,
        "contract": contract,
        "contributions": contributions,
        "governance_conflicts": []
    });
    let mut transaction = fixture.pool.begin().await.expect("begin global fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for global fixture");
    sqlx::query(
        r#"
        INSERT INTO agent_global_contracts (
            id, project_id, revision, synthesis_envelope, candidate,
            groundings, contract_hash, recorded_by_identity_id
        ) VALUES ($1, $2, 1, '{}'::jsonb, $3, '[]'::jsonb, $4, $5)
        "#,
    )
    .bind(global_id)
    .bind(fixture.project_id)
    .bind(&candidate)
    .bind(Sha256::digest(candidate.to_string().as_bytes()).to_vec())
    .bind(fixture.owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert current global contract fixture");
    for (agent, local_goal_id) in local_sources {
        sqlx::query(
            r#"
            INSERT INTO agent_global_contract_sources (
                project_id, global_contract_id, global_revision,
                agent_id, local_goal_id, local_revision
            ) VALUES ($1, $2, 1, $3, $4, 1)
            "#,
        )
        .bind(fixture.project_id)
        .bind(global_id)
        .bind(agent.agent_id)
        .bind(local_goal_id)
        .execute(&mut *transaction)
        .await
        .expect("insert global source grounding");
    }
    transaction.commit().await.expect("commit global fixture");
    global_id
}

async fn create_run(
    app: &axum::Router,
    fixture: &Fixture,
    source_kind: &str,
    source_id: Uuid,
) -> (Uuid, Value) {
    let run_id = Uuid::new_v4();
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        format!("/v1/projects/{}/agent-runs", fixture.project_id),
        &fixture.owner_token,
        json!({
            "id": run_id,
            "source": { "kind": source_kind, "id": source_id, "revision": 1 }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create run: {body}");
    (run_id, body)
}

async fn claim(app: &axum::Router, fixture: &Fixture, run_id: Uuid, agent: &AgentFixture) -> Value {
    let (status, body) = request_json(
        app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/claim",
            fixture.project_id
        ),
        &agent.token,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "claim work: {body}");
    body["claim"].clone()
}

async fn succeed(
    app: &axum::Router,
    fixture: &Fixture,
    run_id: Uuid,
    agent: &AgentFixture,
    claim_id: Uuid,
    outcome: Option<Uuid>,
) -> (StatusCode, Value) {
    request_json(
        app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/claims/{claim_id}/succeed",
            fixture.project_id
        ),
        &agent.token,
        json!({
            "outcome": outcome.map(|id| json!({ "kind": "task_completion", "id": id }))
        }),
    )
    .await
}

async fn read_run(app: &axum::Router, fixture: &Fixture, run_id: Uuid, bearer: &str) -> Value {
    let (status, body) = request_json(
        app.clone(),
        Method::GET,
        format!("/v1/projects/{}/agent-runs/{run_id}", fixture.project_id),
        bearer,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "read run: {body}");
    body
}

struct ProductTask {
    task_id: Uuid,
    resource_id: Uuid,
    assignment_id: Uuid,
}

async fn create_open_task(
    fixture: &Fixture,
    assignee_identity_id: Uuid,
    assigned: bool,
) -> ProductTask {
    let task_list_resource_id = Uuid::new_v4();
    let task_list_id = Uuid::new_v4();
    let task_resource_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let assignment_id = Uuid::new_v4();
    let mut transaction = fixture.pool.begin().await.expect("begin product task");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for product fixture");
    insert_resource(
        &mut transaction,
        fixture.project_id,
        task_list_resource_id,
        Some(fixture.scope_resource_id),
        "task_list",
        fixture.owner_id,
        fixture.owner_device_id,
    )
    .await;
    insert_resource(
        &mut transaction,
        fixture.project_id,
        task_resource_id,
        Some(task_list_resource_id),
        "task",
        fixture.owner_id,
        fixture.owner_device_id,
    )
    .await;
    sqlx::query(
        "INSERT INTO task_lists (
             id, project_id, topic_id, resource_node_id, encrypted_payload
         ) VALUES ($1, $2, $3, $4, decode('01', 'hex'))",
    )
    .bind(task_list_id)
    .bind(fixture.project_id)
    .bind(fixture.topic_id)
    .bind(task_list_resource_id)
    .execute(&mut *transaction)
    .await
    .expect("insert task list");
    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, project_id, task_list_id, resource_node_id, task_kind,
            encrypted_payload, encrypted_value_snapshot, created_by_identity_id
        ) VALUES ($1, $2, $3, $4, 'priority', decode('01', 'hex'),
                  decode('02', 'hex'), $5)
        "#,
    )
    .bind(task_id)
    .bind(fixture.project_id)
    .bind(task_list_id)
    .bind(task_resource_id)
    .bind(fixture.owner_id)
    .execute(&mut *transaction)
    .await
    .expect("insert task");
    if assigned {
        sqlx::query(
            r#"
            INSERT INTO task_assignments (
                id, project_id, task_id, assignee_identity_id,
                assigned_by_identity_id, encrypted_payload, permission_root_grant_id
            ) VALUES ($1, $2, $3, $4, $5, decode('03', 'hex'), $6)
            "#,
        )
        .bind(assignment_id)
        .bind(fixture.project_id)
        .bind(task_id)
        .bind(assignee_identity_id)
        .bind(fixture.owner_id)
        .bind(Uuid::new_v4())
        .execute(&mut *transaction)
        .await
        .expect("insert task assignment");
    }
    transaction.commit().await.expect("commit product task");
    ProductTask {
        task_id,
        resource_id: task_resource_id,
        assignment_id,
    }
}

async fn complete_task_event(
    fixture: &Fixture,
    assignee_identity_id: Uuid,
    task: &ProductTask,
) -> Uuid {
    let completion_id = Uuid::new_v4();
    let mut transaction = fixture.pool.begin().await.expect("begin task completion");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for product event");
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .expect("defer task completion consistency");
    let observed_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
        "SELECT date_trunc('second', clock_timestamp())",
    )
    .fetch_one(&mut *transaction)
    .await
    .expect("product observation time");
    sqlx::query(
        r#"
        INSERT INTO task_completions (
            id, project_id, task_id, assignment_id,
            assignee_identity_id, recorded_by_identity_id,
            occurrence_key, encrypted_payload, completed_at
        ) VALUES ($1, $2, $3, $4, $5, $5, $6, decode('04', 'hex'), $7)
        "#,
    )
    .bind(completion_id)
    .bind(fixture.project_id)
    .bind(task.task_id)
    .bind(task.assignment_id)
    .bind(assignee_identity_id)
    .bind(Uuid::new_v4())
    .bind(observed_at)
    .execute(&mut *transaction)
    .await
    .expect("insert authoritative task completion");
    sqlx::query(
        r#"
        UPDATE tasks
        SET state = 'completed', completed_by_identity_id = $3,
            completed_at = $4, payload_version = payload_version + 1
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(fixture.project_id)
    .bind(task.task_id)
    .bind(assignee_identity_id)
    .bind(observed_at)
    .execute(&mut *transaction)
    .await
    .expect("project task terminal state");
    transaction
        .commit()
        .await
        .expect("commit authoritative task completion");
    completion_id
}

async fn assert_global_task_materialization_without_grounding_fails_closed(
    app: &axum::Router,
    fixture: &Fixture,
    agent: &AgentFixture,
    run_id: Uuid,
    claim: &Value,
    task: &ProductTask,
) {
    let claim_id: Uuid = claim["id"]
        .as_str()
        .expect("claim id")
        .parse()
        .expect("UUID");
    let work_item_id: Uuid = claim["work"]
        .as_str()
        .expect("work item id")
        .parse()
        .expect("UUID");
    let completion_id = Uuid::new_v4();
    let mut transaction = fixture
        .pool
        .begin()
        .await
        .expect("begin exact task provenance");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for provenance fixture");
    let source = sqlx::query(
        r#"
        SELECT source.local_goal_id, source.local_revision,
               (spec ->> 'obligation')::uuid AS obligation_id,
               slot.work_spec_ordinal
        FROM agent_collaborative_runs run
        JOIN agent_run_work_slots slot
          ON slot.project_id = run.project_id
         AND slot.run_id = run.id
         AND slot.work_item_id = $3
        JOIN agent_global_contract_sources source
          ON source.project_id = run.project_id
         AND source.global_contract_id = run.global_contract_id
         AND source.global_revision = run.global_contract_revision
         AND source.agent_id = $4
        JOIN agent_local_goal_contracts local
          ON local.project_id = source.project_id
         AND local.id = source.local_goal_id
         AND local.revision = source.local_revision
         AND local.agent_id = source.agent_id
         AND local.state = 'active'
        JOIN LATERAL jsonb_array_elements(local.contract -> 'contract' -> 'work_specs') spec
          ON (spec ->> 'id')::bigint = slot.work_spec_ordinal
        WHERE run.project_id = $1 AND run.id = $2
        "#,
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .bind(work_item_id)
    .bind(agent.agent_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("load exact grounded local source");
    let provenance_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO agent_task_obligation_provenance (
            id, project_id, task_intent_id, task_resource_node_id,
            target_agent_id, local_goal_id, local_goal_revision,
            obligation_id, work_spec_ordinal
        ) VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(provenance_id)
    .bind(fixture.project_id)
    .bind(task.resource_id)
    .bind(agent.agent_id)
    .bind(source.try_get::<Uuid, _>("local_goal_id").unwrap())
    .bind(source.try_get::<i64, _>("local_revision").unwrap())
    .bind(source.try_get::<Uuid, _>("obligation_id").unwrap())
    .bind(source.try_get::<i64, _>("work_spec_ordinal").unwrap())
    .execute(&mut *transaction)
    .await
    .expect("insert exact task-obligation provenance");
    transaction
        .commit()
        .await
        .expect("commit exact task provenance");

    let (status, materialized) = request_json(
        app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/claims/{claim_id}/materialize-task-completion",
            fixture.project_id
        ),
        &agent.token,
        json!({
            "effect_id": Uuid::new_v4(),
            "completion_id": completion_id,
            "expected_payload_version": 1,
            "encrypted_completion": {
                "version": 1,
                "algorithm": "aes-256-gcm",
                "key_id": "global-causal-completion-key",
                "nonce_b64": "AQEBAQEBAQEBAQEB",
                "ciphertext_b64": "AgM="
            },
            "idempotency_key": Uuid::new_v4()
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a contribution with coincident IDs is not structured global grounding: {materialized}"
    );
    let residues = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        r#"
        SELECT
          (SELECT count(*) FROM task_completions
           WHERE project_id = $1 AND id = $2),
          (SELECT count(*) FROM agent_run_task_effects
           WHERE project_id = $1 AND run_id = $3 AND work_item_id = $4),
          (SELECT count(*) FROM agent_run_work_outcomes
           WHERE project_id = $1 AND run_id = $3 AND work_item_id = $4),
          (SELECT count(*) FROM agent_run_causal_links
           WHERE project_id = $1 AND run_id = $3
             AND predecessor = jsonb_build_object('kind', 'work', 'work', $4::uuid)
             AND successor ->> 'kind' = 'task'),
          (SELECT count(*) FROM agent_run_transitions
           WHERE project_id = $1 AND run_id = $3
             AND transition_kind = 'work_succeeded')
        "#,
    )
    .bind(fixture.project_id)
    .bind(completion_id)
    .bind(run_id)
    .bind(work_item_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count failed-closed global materialization residues");
    assert_eq!(residues, (0, 0, 0, 0, 0));
    let task_state = sqlx::query_scalar::<_, String>(
        "SELECT state FROM tasks WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(task.task_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load task after rejected global materialization");
    assert_eq!(task_state, "open");
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn generic_task_effect_binds_exact_work_claim_and_mechanical_evidence() {
    let fixture = fixture().await;
    let agent = &fixture.agents[0];
    let bound_task = create_open_task(&fixture, agent.identity_id, true).await;
    let unrelated_task = create_open_task(&fixture, agent.identity_id, true).await;
    let obligation = Uuid::new_v4();
    let mut contract = goal_contract(
        fixture.scope_resource_id,
        &[(
            agent.identity_id,
            obligation,
            1,
            json!({ "kind": "always" }),
            true,
            vec![],
            0,
            "task_action",
        )],
    );
    contract["work_specs"][0]["allowed_actions"] = json!(["mark_assigned_done"]);
    let local_goal_id = insert_local_goal(&fixture, agent, contract).await;
    let task_provenance_id = Uuid::new_v4();
    let mut provenance = fixture.pool.begin().await.expect("begin causal provenance");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *provenance)
        .await
        .expect("disable RLS for causal provenance fixture");
    sqlx::query(
        r#"
        INSERT INTO agent_task_obligation_provenance (
            id, project_id, task_intent_id, task_resource_node_id,
            target_agent_id, local_goal_id, local_goal_revision,
            obligation_id, work_spec_ordinal
        ) VALUES ($1, $2, NULL, $3, $4, $5, 1, $6, 1)
        "#,
    )
    .bind(task_provenance_id)
    .bind(fixture.project_id)
    .bind(bound_task.resource_id)
    .bind(agent.agent_id)
    .bind(local_goal_id)
    .bind(obligation)
    .execute(&mut *provenance)
    .await
    .expect("insert generic task-obligation provenance");
    provenance
        .commit()
        .await
        .expect("commit generic causal provenance");

    let app = app(&fixture, Duration::from_secs(300));
    let (run_id, _) = create_run(&app, &fixture, "local_goal", local_goal_id).await;
    let trace_number = sqlx::query_scalar::<_, i64>(
        "SELECT trace_number FROM agent_r541_release_roots
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load 0035 release trace");
    let premature_root = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT sprout_private.issue_agent_r541_formal_release($1)",
    )
    .bind(trace_number)
    .fetch_one(&fixture.pool)
    .await
    .expect("incomplete root fails closed");
    assert_eq!(premature_root, None);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_r541_formal_release_certificates
             WHERE trace_number=$1",
        )
        .bind(trace_number)
        .fetch_one(&fixture.pool)
        .await
        .expect("count premature root residue"),
        0
    );
    let work_claim = claim(&app, &fixture, run_id, agent).await;
    let claim_id: Uuid = work_claim["id"]
        .as_str()
        .expect("claim id")
        .parse()
        .expect("claim UUID");
    let work_item_id: Uuid = work_claim["work"]
        .as_str()
        .expect("work item id")
        .parse()
        .expect("work UUID");

    let unrelated_completion =
        complete_task_event(&fixture, agent.identity_id, &unrelated_task).await;
    let (status, _) = succeed(
        &app,
        &fixture,
        run_id,
        agent,
        claim_id,
        Some(unrelated_completion),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "same agent/scope/time is not causal provenance"
    );

    let actor_device_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT device_id FROM agent_runners
         WHERE project_id = $1 AND agent_id = $2 AND state = 'active'",
    )
    .bind(fixture.project_id)
    .bind(agent.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load active runner device");
    for (case_name, after_expiry) in [("equal", false), ("after", true)] {
        let completion_id = Uuid::new_v4();
        let effect_id = Uuid::new_v4();
        let mut boundary = fixture
            .pool
            .begin()
            .await
            .expect("begin lease boundary gate");
        sqlx::query("SET LOCAL row_security = off")
            .execute(&mut *boundary)
            .await
            .expect("disable RLS for lease boundary gate");
        sqlx::query("SET CONSTRAINTS ALL DEFERRED")
            .execute(&mut *boundary)
            .await
            .expect("defer product completion consistency");
        let expires_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT clock_timestamp() + interval '1 minute'",
        )
        .fetch_one(&mut *boundary)
        .await
        .expect("choose exact lease boundary");
        let applied_at = if after_expiry {
            expires_at + chrono::Duration::microseconds(1)
        } else {
            expires_at
        };
        sqlx::query(
            "UPDATE agent_run_claim_leases SET expires_at = $3
             WHERE project_id = $1 AND id = $2",
        )
        .bind(fixture.project_id)
        .bind(claim_id)
        .bind(expires_at)
        .execute(&mut *boundary)
        .await
        .expect("set exact lease boundary");
        sqlx::query(
            r#"
            INSERT INTO task_completions (
                id, project_id, task_id, assignment_id,
                assignee_identity_id, recorded_by_identity_id,
                occurrence_key, encrypted_payload, completed_at
            ) VALUES ($1, $2, $3, $4, $5, $5, $1, decode('04', 'hex'), $6)
            "#,
        )
        .bind(completion_id)
        .bind(fixture.project_id)
        .bind(bound_task.task_id)
        .bind(bound_task.assignment_id)
        .bind(agent.identity_id)
        .bind(applied_at)
        .execute(&mut *boundary)
        .await
        .expect("stage boundary task completion");
        sqlx::query(
            "UPDATE tasks
             SET state = 'completed', completed_by_identity_id = $3,
                 completed_at = $4, payload_version = payload_version + 1
             WHERE project_id = $1 AND id = $2 AND state = 'open'",
        )
        .bind(fixture.project_id)
        .bind(bound_task.task_id)
        .bind(agent.identity_id)
        .bind(applied_at)
        .execute(&mut *boundary)
        .await
        .expect("stage boundary task state");
        let rejected = sqlx::query(
            r#"
            INSERT INTO agent_run_task_effects (
                id, project_id, run_id, work_item_id, claim_id, attempt,
                task_provenance_id, task_intent_id, task_resource_node_id, task_id,
                task_assignment_id, task_completion_id, target_agent_id,
                cross_owner_effect_id, actor_identity_id, actor_device_id,
                idempotency_key, request_hash, provenance_hash, applied_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, NULL, $8, $9,
                $10, $11, $12, NULL, $13, $14, $15,
                decode(repeat('01', 32), 'hex'),
                decode(repeat('02', 32), 'hex'), $16
            )
            "#,
        )
        .bind(effect_id)
        .bind(fixture.project_id)
        .bind(run_id)
        .bind(work_item_id)
        .bind(claim_id)
        .bind(work_claim["attempt"].as_u64().expect("claim attempt") as i32)
        .bind(task_provenance_id)
        .bind(bound_task.resource_id)
        .bind(bound_task.task_id)
        .bind(bound_task.assignment_id)
        .bind(completion_id)
        .bind(agent.agent_id)
        .bind(agent.identity_id)
        .bind(actor_device_id)
        .bind(Uuid::new_v4())
        .bind(applied_at)
        .execute(&mut *boundary)
        .await
        .expect_err("effect at or after lease expiry must fail closed");
        assert_eq!(
            rejected.as_database_error().and_then(|error| error.code()),
            Some(std::borrow::Cow::Borrowed("55000")),
            "unexpected SQLSTATE for {case_name} boundary"
        );
        boundary
            .rollback()
            .await
            .expect("rollback rejected boundary effect atomically");

        let task_state = sqlx::query_as::<_, (String, Option<Uuid>)>(
            "SELECT state, completed_by_identity_id FROM tasks
             WHERE project_id = $1 AND id = $2",
        )
        .bind(fixture.project_id)
        .bind(bound_task.task_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("verify boundary task rollback");
        assert_eq!(task_state, ("open".to_owned(), None));
        let completion_residue = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM task_completions
             WHERE project_id = $1 AND id = $2",
        )
        .bind(fixture.project_id)
        .bind(completion_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("count rejected completion residue");
        let effect_residue = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_run_task_effects
             WHERE project_id = $1 AND id = $2",
        )
        .bind(fixture.project_id)
        .bind(effect_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("count rejected effect residue");
        let outcome_residue = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_run_work_outcomes
             WHERE project_id = $1 AND run_id = $2 AND work_item_id = $3",
        )
        .bind(fixture.project_id)
        .bind(run_id)
        .bind(work_item_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("count rejected outcome residue");
        let evidence_residue = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_run_evidence_provenance
             WHERE project_id = $1 AND run_id = $2 AND work_item_id = $3",
        )
        .bind(fixture.project_id)
        .bind(run_id)
        .bind(work_item_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("count rejected evidence residue");
        assert_eq!(completion_residue, 0, "{case_name} completion residue");
        assert_eq!(effect_residue, 0, "{case_name} effect residue");
        assert_eq!(outcome_residue, 0, "{case_name} outcome residue");
        assert_eq!(evidence_residue, 0, "{case_name} evidence residue");
        let terminal_audit = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_run_transitions
             WHERE project_id = $1 AND run_id = $2
               AND transition_kind = 'work_succeeded'",
        )
        .bind(fixture.project_id)
        .bind(run_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("count rejected boundary terminal audit");
        assert_eq!(
            terminal_audit, 0,
            "{case_name} boundary left terminal audit"
        );
    }

    let effect_id = Uuid::new_v4();
    let completion_id = Uuid::new_v4();
    let evidence_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();
    let body = json!({
        "effect_id": effect_id,
        "completion_id": completion_id,
        "evidence_id": evidence_id,
        "evidence_rule_id": 1,
        "expected_payload_version": 1,
        "encrypted_completion": {
            "version": 1,
            "algorithm": "aes-256-gcm",
            "key_id": "causal-completion-key",
            "nonce_b64": "AQEBAQEBAQEBAQEB",
            "ciphertext_b64": "AgM="
        },
        "idempotency_key": idempotency_key
    });
    let endpoint = format!(
        "/v1/projects/{}/agent-runs/{run_id}/claims/{claim_id}/materialize-task-completion",
        fixture.project_id
    );
    let (status, materialized) = request_json(
        app.clone(),
        Method::POST,
        endpoint.clone(),
        &agent.token,
        body.clone(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "materialize causal task effect: {materialized}"
    );
    assert_eq!(materialized["replayed"], false);
    assert_eq!(
        materialized["state"]["work_items"][work_item_id.to_string()]["status"],
        "succeeded"
    );
    assert_eq!(
        materialized["state"]["obligations"][obligation.to_string()]["status"],
        "discharged",
        "0035-native task materialization must close mechanical evidence atomically"
    );
    let causal_links = materialized["state"]["causal_links"]
        .as_array()
        .expect("authoritative semantic causal links");
    assert!(causal_links.iter().any(|link| {
        link["predecessor"] == json!({ "kind": "work", "work": work_item_id })
            && link["successor"] == json!({ "kind": "task", "task": bound_task.resource_id })
    }));
    assert!(!causal_links.iter().any(|link| {
        link["predecessor"] == json!({ "kind": "task", "task": bound_task.resource_id })
            && link["successor"] == json!({ "kind": "work", "work": work_item_id })
    }));
    assert!(
        causal_links
            .iter()
            .any(|link| link["predecessor"]["kind"] == "obligation"),
        "relational hydration must retain pre-existing non-task causal links"
    );
    let persisted_causal = sqlx::query_as::<_, (Uuid, i64, i64)>(
        r#"
        SELECT link.task_effect_id, link.causal_position,
               count(*) OVER ()
        FROM agent_run_causal_links link
        WHERE link.project_id = $1 AND link.run_id = $2
          AND link.predecessor = jsonb_build_object('kind', 'work', 'work', $3::uuid)
          AND link.successor = jsonb_build_object('kind', 'task', 'task', $4::uuid)
        "#,
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .bind(work_item_id)
    .bind(bound_task.resource_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact persisted Work -> Task edge");
    assert_eq!(persisted_causal.0, effect_id);
    assert_eq!(persisted_causal.2, 1);

    let (status, replayed) = request_json(
        app.clone(),
        Method::POST,
        endpoint.clone(),
        &agent.token,
        body.clone(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "replay causal task effect: {replayed}"
    );
    assert_eq!(replayed["replayed"], true);
    let causal_count_after_replay = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_causal_links
         WHERE project_id = $1 AND run_id = $2
           AND predecessor = jsonb_build_object('kind', 'work', 'work', $3::uuid)
           AND successor = jsonb_build_object('kind', 'task', 'task', $4::uuid)",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .bind(work_item_id)
    .bind(bound_task.resource_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count causal edge after idempotent replay");
    assert_eq!(causal_count_after_replay, 1);
    let mut mismatched = body;
    mismatched["encrypted_completion"]["ciphertext_b64"] = json!("BAU=");
    let (status, _) = request_json(
        app.clone(),
        Method::POST,
        endpoint,
        &agent.token,
        mismatched,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let reloaded = read_run(&app, &fixture, run_id, &agent.token).await;
    assert_eq!(
        reloaded["state"]["causal_links"],
        materialized["state"]["causal_links"]
    );
    let persisted_position_after_reload = sqlx::query_scalar::<_, i64>(
        "SELECT causal_position FROM agent_run_causal_links
         WHERE project_id = $1 AND task_effect_id = $2",
    )
    .bind(fixture.project_id)
    .bind(effect_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("reload stable causal position");
    assert_eq!(persisted_position_after_reload, persisted_causal.1);

    let wrong_evidence_id = Uuid::new_v4();
    let (status, _) = request_json(
        app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/evidence",
            fixture.project_id
        ),
        &agent.token,
        json!({
            "id": wrong_evidence_id,
            "rule_id": 1,
            "work_item_id": work_item_id,
            "source": { "kind": "task_completion", "id": unrelated_completion }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let wrong_evidence_residue = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_evidence_provenance
         WHERE project_id = $1 AND run_id = $2 AND evidence_id = $3",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .bind(wrong_evidence_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count rejected non-successor evidence");
    assert_eq!(wrong_evidence_residue, 0);

    let (status, evidenced) = request_json(
        app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/evidence",
            fixture.project_id
        ),
        &agent.token,
        json!({
            "id": evidence_id,
            "rule_id": 1,
            "work_item_id": work_item_id,
            "source": { "kind": "task_completion", "id": completion_id }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "evidence replay: {evidenced}");

    let evidence_projection = sqlx::query_as::<_, (Value, String, i32, i64, i64, i64, i64)>(
        "SELECT exact.formal_record, gate.evidence_mode,
                jsonb_array_length(gate.evidence_records),
                (SELECT count(*) FROM agent_r540_typed_exact_release_events candidate
                  WHERE candidate.project_id=$1 AND candidate.run_id=$2),
                (SELECT count(*) FROM agent_r540_release_inventory inventory
                  JOIN agent_r541_release_roots root USING (trace_number,project_id)
                  WHERE root.project_id=$1 AND root.run_id=$2),
                (SELECT count(*) FROM agent_r540_exact_release_trace_certificates certificate
                  JOIN agent_r541_release_roots root USING (trace_number,project_id)
                  WHERE root.project_id=$1 AND root.run_id=$2),
                (SELECT semantic_tick FROM agent_r540_typed_exact_release_events outcome
                  WHERE outcome.project_id=$1 AND outcome.run_id=$2
                    AND outcome.event_kind='work_outcome'
                    AND outcome.formal_record->>'work'=$4::text
                    AND outcome.formal_record->>'claim'=$5::text
                    AND (outcome.formal_record->>'attempt')::integer=1)
         FROM agent_r540_typed_exact_release_events exact
         JOIN agent_r541_release_trace_surface_gates gate
           ON gate.project_id=exact.project_id AND gate.run_id=exact.run_id
         WHERE exact.project_id=$1 AND exact.run_id=$2
           AND exact.event_kind='evidence' AND exact.source_record_id=$3",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .bind(evidence_id)
    .bind(work_item_id)
    .bind(claim_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact typed R540 Evidence projection");
    assert_eq!(evidence_projection.0["work"], work_item_id.to_string());
    assert_eq!(evidence_projection.0["claim"], claim_id.to_string());
    assert_eq!(evidence_projection.0["attempt"], 1);
    assert_eq!(
        evidence_projection.0["evidence"]["id"],
        evidence_id.to_string()
    );
    assert_eq!(
        evidence_projection.0["evidence"]["subject"],
        json!({"kind":"task","task":bound_task.resource_id})
    );
    assert_eq!(
        evidence_projection.0["evidence"]["observed_at"],
        evidence_projection.6
    );
    assert!(evidence_projection.6 <= evidence_projection.0["accepted_at"].as_i64().unwrap());
    assert_eq!(
        (
            evidence_projection.1.as_str(),
            evidence_projection.2,
            evidence_projection.3,
            evidence_projection.4,
            evidence_projection.5,
        ),
        ("enabled", 1, 5, 5, 1)
    );

    let effect = sqlx::query(
        "SELECT effect.task_resource_node_id, effect.task_completion_id,
                effect.task_intent_id, effect.cross_owner_effect_id,
                effect.actor_identity_id, agent.principal_identity_id,
                ledger.principal_identity_id AS projected_identity_id,
                effect.applied_at, claim.expires_at
         FROM agent_run_task_effects effect
         JOIN agent_run_claim_leases claim
           ON claim.project_id = effect.project_id AND claim.id = effect.claim_id
         JOIN governed_agents agent
           ON agent.project_id = effect.project_id AND agent.id = effect.target_agent_id
         JOIN agent_semantic_operational_ledger ledger
           ON ledger.entry_kind = 'task_provenance'
          AND ledger.project_id = effect.project_id
          AND ledger.record_id = effect.task_provenance_id
         WHERE effect.project_id = $1 AND effect.id = $2",
    )
    .bind(fixture.project_id)
    .bind(effect_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load generic task effect certificate");
    assert_eq!(
        effect.try_get::<Uuid, _>("task_resource_node_id").unwrap(),
        bound_task.resource_id
    );
    assert_eq!(
        effect.try_get::<Uuid, _>("task_completion_id").unwrap(),
        completion_id
    );
    assert_eq!(
        effect.try_get::<Option<Uuid>, _>("task_intent_id").unwrap(),
        None,
        "TaskIntent remains optional on the generic bridge"
    );
    assert_eq!(
        effect
            .try_get::<Option<Uuid>, _>("cross_owner_effect_id")
            .unwrap(),
        None
    );
    assert_eq!(
        effect.try_get::<Uuid, _>("actor_identity_id").unwrap(),
        agent.identity_id
    );
    assert_eq!(
        effect.try_get::<Uuid, _>("principal_identity_id").unwrap(),
        agent.identity_id
    );
    assert_eq!(
        effect.try_get::<Uuid, _>("projected_identity_id").unwrap(),
        agent.identity_id
    );
    assert_ne!(agent.identity_id, agent.agent_id);
    assert!(
        effect
            .try_get::<chrono::DateTime<chrono::Utc>, _>("applied_at")
            .unwrap()
            < effect
                .try_get::<chrono::DateTime<chrono::Utc>, _>("expires_at")
                .unwrap(),
        "accepted effect must be strictly before claim expiry"
    );

    let (status, completed) = request_json(
        app,
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/complete",
            fixture.project_id
        ),
        &fixture.owner_token,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "complete exact release run: {completed}"
    );
    let mut secure_kernel_fields = Vec::new();
    for (source, statement) in [
        (
            "agent_r541_collaborative_kernel_field_sources",
            "SELECT count(*) FROM agent_r541_collaborative_kernel_field_sources WHERE trace_number=$1",
        ),
        (
            "agent_r541_scheduler_kernel_field_sources",
            "SELECT count(*) FROM agent_r541_scheduler_kernel_field_sources WHERE trace_number=$1",
        ),
        (
            "progress kernel field projector",
            "SELECT count(*) FROM sprout_private.reconstruct_agent_r541_progress_kernel_fields($1)",
        ),
        (
            "evidence-discharge field projector",
            "SELECT count(*) FROM sprout_private.reconstruct_agent_r541_evidence_discharge_fields($1)",
        ),
        (
            "authority-information field projector",
            "SELECT count(*) FROM sprout_private.reconstruct_agent_r541_authority_information($1)",
        ),
    ] {
        secure_kernel_fields.push(
            sqlx::query_scalar::<_, i64>(statement)
                .bind(trace_number)
                .fetch_one(&fixture.pool)
                .await
                .unwrap_or_else(|error| panic!("reconstruct {source}: {error}")),
        );
    }
    assert_eq!(
        secure_kernel_fields,
        vec![22, 4, 6, 3, 32],
        "secure kernel must expose the complete exact 22+4, 6, 3, and 32 field sets"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_r541_secure_kernel_nested_sources
             WHERE trace_number=$1",
        )
        .bind(trace_number)
        .fetch_one(&fixture.pool)
        .await
        .expect("count exact secure-kernel nested sources"),
        41
    );
    let exact_root = sqlx::query_as::<_, (Uuid, i64, i64, i64)>(
        "SELECT certificate.id,
                (SELECT count(*) FROM agent_r541_exact_formal_release_child_certificates child
                  WHERE child.trace_number=certificate.trace_number
                    AND child.release_version=certificate.version),
                (SELECT count(DISTINCT child.root_field)
                   FROM agent_r541_exact_formal_release_child_certificates child
                  WHERE child.trace_number=certificate.trace_number
                    AND child.release_version=certificate.version),
                (SELECT count(DISTINCT child.id)
                   FROM agent_r541_exact_formal_release_child_certificates child
                  WHERE child.trace_number=certificate.trace_number
                    AND child.release_version=certificate.version)
         FROM agent_r541_exact_formal_release_certificates certificate
         WHERE certificate.trace_number=$1",
    )
    .bind(trace_number)
    .fetch_one(&fixture.pool)
    .await
    .expect("all 28 field-specific children compose an exact root");
    assert_eq!((exact_root.1, exact_root.2, exact_root.3), (28, 28, 28));
    let replayed_root = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT sprout_private.issue_agent_r541_formal_release($1)",
    )
    .bind(trace_number)
    .fetch_one(&fixture.pool)
    .await
    .expect("replay exact root issuance");
    assert_eq!(replayed_root, Some(exact_root.0));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_r541_formal_release_certificates
             WHERE trace_number=$1",
        )
        .bind(trace_number)
        .fetch_one(&fixture.pool)
        .await
        .expect("count root versions after replay"),
        1,
        "exact replay must not mint a second root version"
    );

    let mut missing_source = fixture
        .pool
        .begin()
        .await
        .expect("begin secure-kernel source tamper");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *missing_source)
        .await
        .expect("migration-owner adversarial trigger bypass");
    sqlx::query(
        "DELETE FROM agent_run_resource_authority_snapshots
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .execute(&mut *missing_source)
    .await
    .expect("remove required authority source inside rollback-only adversarial transaction");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_r541_exact_formal_release_certificates
             WHERE trace_number=$1",
        )
        .bind(trace_number)
        .fetch_one(&mut *missing_source)
        .await
        .expect("served root must disappear with a missing secure-kernel source"),
        0,
        "a historical root descriptor cannot serve as exact after its typed source disappears"
    );
    missing_source
        .rollback()
        .await
        .expect("restore exact authority source after adversarial test");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_r541_exact_formal_release_certificates
             WHERE trace_number=$1",
        )
        .bind(trace_number)
        .fetch_one(&fixture.pool)
        .await
        .expect("served root recovers after adversarial rollback"),
        1
    );
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn semantic_list_security_definer_is_not_public_or_identity_spoofable() {
    let foreign = fixture().await;
    let fixture = fixture().await;
    sqlx::query(
        r#"
        DO $role$
        BEGIN
            IF NOT EXISTS (
                SELECT 1 FROM pg_roles WHERE rolname = 'sprout_semantic_list_app'
            ) THEN
                CREATE ROLE sprout_semantic_list_app NOSUPERUSER NOBYPASSRLS NOLOGIN;
            END IF;
        END
        $role$
        "#,
    )
    .execute(&fixture.pool)
    .await
    .expect("create unprivileged semantic-list app role");
    let migration_owner = sqlx::query_scalar::<_, String>("SELECT current_user")
        .fetch_one(&fixture.pool)
        .await
        .expect("load migration owner");
    sqlx::query("GRANT sprout_semantic_list_app TO CURRENT_USER")
        .execute(&fixture.pool)
        .await
        .expect("allow test owner to assume semantic-list app role");

    for signature in [
        "sprout_private.semantic_task_intent_list(uuid)",
        "sprout_private.semantic_task_provenance_list(uuid)",
        "sprout_private.semantic_run_causal_link_list(uuid,uuid)",
    ] {
        let metadata = sqlx::query_as::<_, (bool, bool, String, String)>(
            "SELECT procedure.prosecdef,
                    has_function_privilege('sprout_semantic_list_app', procedure.oid, 'EXECUTE'),
                    owner.rolname,
                    COALESCE(array_to_string(procedure.proconfig, ','), '')
             FROM pg_proc procedure
             JOIN pg_roles owner ON owner.oid = procedure.proowner
             WHERE procedure.oid = $1::regprocedure",
        )
        .bind(signature)
        .fetch_one(&fixture.pool)
        .await
        .expect("load semantic-list function security metadata");
        assert!(metadata.0, "{signature} must remain SECURITY DEFINER");
        assert!(
            !metadata.1,
            "{signature} must not be executable by app/PUBLIC"
        );
        assert_eq!(metadata.2, migration_owner);
        assert!(metadata.3.contains("search_path=public, pg_temp"));
        assert!(metadata.3.contains("row_security=off"));
    }

    let mut spoof = fixture
        .pool
        .begin()
        .await
        .expect("begin identity spoof gate");
    sqlx::query("SET LOCAL ROLE sprout_semantic_list_app")
        .execute(&mut *spoof)
        .await
        .expect("assume unprivileged app role");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(foreign.owner_id.to_string())
    .bind(foreign.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *spoof)
    .await
    .expect("forge cross-project custom GUCs");
    sqlx::query(
        r#"
        DO $spoof$
        BEGIN
            BEGIN
                PERFORM 1 FROM sprout_private.semantic_task_intent_list(
                    current_setting('app.project_id')::uuid
                );
                RAISE EXCEPTION 'forged identity read TaskIntent list';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                PERFORM 1 FROM sprout_private.semantic_task_provenance_list(
                    current_setting('app.project_id')::uuid
                );
                RAISE EXCEPTION 'forged identity read provenance list';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                PERFORM 1 FROM sprout_private.semantic_run_causal_link_list(
                    current_setting('app.project_id')::uuid, gen_random_uuid()
                );
                RAISE EXCEPTION 'forged identity read causal-link list';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
        END
        $spoof$
        "#,
    )
    .execute(&mut *spoof)
    .await
    .expect("all semantic list functions reject forged app-role identity");
    spoof
        .rollback()
        .await
        .expect("rollback identity spoof gate");
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn restart_reload_preserves_canonical_projection_and_history() {
    let fixture = fixture().await;
    let trigger_task = create_open_task(&fixture, fixture.agents[0].identity_id, true).await;
    let obligation = Uuid::new_v4();
    let contract = goal_contract(
        fixture.scope_resource_id,
        &[
            (
                fixture.agents[0].identity_id,
                obligation,
                1,
                json!({ "kind": "always" }),
                true,
                vec![2],
                1,
                "agent_action",
            ),
            (
                fixture.agents[0].identity_id,
                obligation,
                2,
                json!({
                    "kind": "neg",
                    "condition": {
                        "kind": "task_done",
                        "task": trigger_task.resource_id
                    }
                }),
                false,
                vec![],
                0,
                "agent_action",
            ),
        ],
    );
    let local_goal_id = insert_local_goal(&fixture, &fixture.agents[0], contract).await;
    let first_app = app(&fixture, Duration::from_secs(300));
    let (run_id, initialized) = create_run(&first_app, &fixture, "local_goal", local_goal_id).await;
    let slot_before = initialized["state"]["work_slots"].clone();
    let entry_claim = claim(&first_app, &fixture, run_id, &fixture.agents[0]).await;
    let entry_claim_id = entry_claim["id"]
        .as_str()
        .expect("entry claim id")
        .parse()
        .expect("entry claim UUID");
    let (status, succeeded) = succeed(
        &first_app,
        &fixture,
        run_id,
        &fixture.agents[0],
        entry_claim_id,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "succeed entry work: {succeeded}");
    let child = succeeded["state"]["work_items"]
        .as_object()
        .expect("current work projection")
        .iter()
        .find(|(_, work)| work["work_spec_id"] == 2)
        .map(|(id, _)| id.clone())
        .expect("continuation materialized while activation holds");

    complete_task_event(&fixture, fixture.agents[0].identity_id, &trigger_task).await;
    let (status, deactivated) = request_json(
        first_app,
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/refresh",
            fixture.project_id
        ),
        &fixture.owner_token,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "deactivate projection: {deactivated}"
    );
    assert!(deactivated["state"]["work_items"].get(&child).is_none());
    assert_eq!(
        deactivated["state"]["inactive_work_items"][&child]["work_spec_id"],
        2
    );
    assert!(
        deactivated["state"]["work_projection_history"]
            .as_array()
            .is_some_and(|history| history.iter().any(|event| {
                event["work"].as_str() == Some(child.as_str())
                    && event["kind"] == "activation_ceased"
            }))
    );

    // A newly constructed server/router is the concrete process restart. It
    // has no in-memory run object and must reload the authoritative DB state.
    let restarted_app = app(&fixture, Duration::from_secs(300));
    let reloaded = read_run(&restarted_app, &fixture, run_id, &fixture.owner_token).await;
    assert_eq!(reloaded["state"]["work_slots"], slot_before);
    assert_eq!(
        reloaded["state"]["inactive_work_items"][&child]["id"],
        child
    );
    assert_eq!(
        reloaded["state"]["work_projection_history"],
        deactivated["state"]["work_projection_history"]
    );
    let (slot_count, distinct_work_count) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*), count(DISTINCT work_item_id)
         FROM agent_run_work_slots WHERE project_id = $1 AND run_id = $2",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count persisted canonical slots");
    assert_eq!((slot_count, distinct_work_count), (2, 2));
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn completion_is_atomic_and_goal_is_distinct_from_run() {
    let fixture = fixture().await;
    let blocker_task = create_open_task(&fixture, fixture.owner_id, false).await;
    let obligation = Uuid::new_v4();
    let mut incomplete_contract = goal_contract(
        fixture.scope_resource_id,
        &[(
            fixture.agents[0].identity_id,
            obligation,
            1,
            json!({ "kind": "always" }),
            true,
            vec![],
            0,
            "agent_action",
        )],
    );
    incomplete_contract["waiting_rules"] = json!([{
        "id": 1,
        "obligation": obligation,
        "target": {
            "kind": "task_from_work",
            "work_spec_id": 1
        }
    }]);
    let incomplete_goal = incomplete_contract["goal"]
        .as_str()
        .expect("goal id")
        .parse::<Uuid>()
        .expect("goal UUID");
    let incomplete_local =
        insert_local_goal(&fixture, &fixture.agents[0], incomplete_contract).await;
    let test_app = app(&fixture, Duration::from_secs(300));
    let (incomplete_run, initialized) =
        create_run(&test_app, &fixture, "local_goal", incomplete_local).await;
    sqlx::query(
        "INSERT INTO task_assignments (
           id,project_id,task_id,assignee_identity_id,assigned_by_identity_id,
           encrypted_payload,permission_root_grant_id
         ) VALUES ($1,$2,$3,$4,$5,decode('03','hex'),$6)",
    )
    .bind(blocker_task.assignment_id)
    .bind(fixture.project_id)
    .bind(blocker_task.task_id)
    .bind(fixture.owner_id)
    .bind(fixture.owner_id)
    .bind(Uuid::new_v4())
    .execute(&fixture.pool)
    .await
    .expect("activate exact blocker task assignment");
    let work_id = initialized["state"]["work_items"]
        .as_object()
        .and_then(|items| items.keys().next())
        .cloned()
        .expect("open work");
    let work_uuid = work_id.parse::<Uuid>().expect("work UUID");
    let initial_claim = claim(&test_app, &fixture, incomplete_run, &fixture.agents[0]).await;
    assert_eq!(initial_claim["work"], work_id);
    assert_eq!(initial_claim["attempt"], 1);
    let (status, blocker) = request_json(
        test_app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{incomplete_run}/blockers",
            fixture.project_id
        ),
        &fixture.agents[0].token,
        json!({
            "waiting_rule_ordinal": 1,
            "scope": { "kind": "work", "work": work_id },
            "condition": {
                "kind": "human_task_completed",
                "task": blocker_task.resource_id
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create waiting blocker: {blocker}");
    let blocker_id = blocker["blocker_id"]
        .as_str()
        .expect("blocker id")
        .parse::<Uuid>()
        .expect("blocker UUID");
    complete_task_event(&fixture, fixture.owner_id, &blocker_task).await;
    let (status, resolved) = request_json(
        test_app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{incomplete_run}/blockers/{blocker_id}/resolve",
            fixture.project_id
        ),
        &fixture.agents[0].token,
        json!({
            "kind": "human_task_terminal",
            "observation_id": blocker_task.resource_id
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "resolve exact blocker: {resolved}");
    assert_eq!(resolved["status"], "resolved");
    let exact_blocker_events = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_r540_typed_exact_release_events
         WHERE project_id=$1 AND run_id=$2 AND event_kind='blocker_resolution'",
    )
    .bind(fixture.project_id)
    .bind(incomplete_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exact blocker events");
    assert_eq!(exact_blocker_events, 1);
    let causal_grounding = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
           SELECT 1 FROM agent_run_causal_links link
           WHERE link.project_id=$1 AND link.run_id=$2
             AND link.goal_id=$3
             AND link.predecessor=jsonb_build_object('kind','task','task',$4::uuid)
             AND link.successor=jsonb_build_object('kind','work','work',$5::uuid)
             AND link.observed_tick <= (
               SELECT semantic_tick FROM agent_run_transitions
               WHERE project_id=$1 AND run_id=$2 AND transition_kind='blocker_resolved'
               ORDER BY state_version DESC LIMIT 1))",
    )
    .bind(fixture.project_id)
    .bind(incomplete_run)
    .bind(incomplete_goal)
    .bind(blocker_task.resource_id)
    .bind(work_uuid)
    .fetch_one(&fixture.pool)
    .await
    .expect("load canonical Task -> Work grounding");
    assert!(causal_grounding);
    let typed_work_attempts = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_r540_typed_exact_release_events
         WHERE project_id=$1 AND run_id=$2 AND event_kind='work_attempt'",
    )
    .bind(fixture.project_id)
    .bind(incomplete_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exact WorkAttempt events");
    assert_eq!(typed_work_attempts, 1);
    let blocker_gate_exact = sqlx::query_scalar::<_, bool>(
        "SELECT gate.blocker_mode='enabled'
             AND jsonb_array_length(gate.blocker_records)=1
             AND gate.blocker_records=inventory.blocker_inventory
         FROM agent_r541_release_trace_surface_gates gate
         JOIN agent_r540_release_inventory_state inventory
           ON inventory.trace_number=gate.trace_number
         WHERE gate.project_id=$1 AND gate.run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(incomplete_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact blocker gate");
    assert!(blocker_gate_exact);
    let before = sqlx::query(
        "SELECT state_version, state_hash, goal_status, run_status
         FROM agent_collaborative_runs WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(incomplete_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load incomplete state before completion attempt");
    let before_version: i64 = before.get("state_version");
    let before_hash: Vec<u8> = before.get("state_hash");
    let (status, _) = request_json(
        test_app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{incomplete_run}/complete",
            fixture.project_id
        ),
        &fixture.owner_token,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let after = sqlx::query(
        "SELECT state_version, state_hash, goal_status, run_status
         FROM agent_collaborative_runs WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(incomplete_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load incomplete state after rollback");
    assert_eq!(after.get::<i64, _>("state_version"), before_version);
    assert_eq!(after.get::<Vec<u8>, _>("state_hash"), before_hash);
    assert_eq!(after.get::<String, _>("goal_status"), "active");
    assert_eq!(after.get::<String, _>("run_status"), "running");

    let dormant_obligation = Uuid::new_v4();
    let mut completable_contract = goal_contract(
        fixture.scope_resource_id,
        &[(
            fixture.agents[0].identity_id,
            dormant_obligation,
            1,
            json!({ "kind": "never" }),
            true,
            vec![],
            0,
            "agent_action",
        )],
    );
    completable_contract["obligations"][0]["activation"] = json!({ "kind": "never" });
    let completable_local =
        insert_local_goal(&fixture, &fixture.agents[0], completable_contract).await;
    let (completable_run, created) =
        create_run(&test_app, &fixture, "local_goal", completable_local).await;
    assert_eq!(created["state"]["goal_status"], "completed");
    assert_eq!(created["state"]["run_status"], "running");
    let (status, completed) = request_json(
        test_app,
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{completable_run}/complete",
            fixture.project_id
        ),
        &fixture.owner_token,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "complete run atomically: {completed}"
    );
    assert_eq!(completed["state"]["goal_status"], "completed");
    assert_eq!(completed["state"]["run_status"], "completed");
    let persisted = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT goal_status, run_status, state_version
         FROM agent_collaborative_runs WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(completable_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load atomically completed run");
    assert_eq!(persisted.0, "completed");
    assert_eq!(persisted.1, "completed");
    assert_eq!(persisted.2, 2);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn claim_concurrency_recovery_and_scheduler_bounds_are_persistent() {
    let fixture = fixture().await;
    let obligation = Uuid::new_v4();
    let contract = goal_contract(
        fixture.scope_resource_id,
        &[(
            fixture.agents[0].identity_id,
            obligation,
            1,
            json!({ "kind": "always" }),
            true,
            vec![],
            0,
            "agent_action",
        )],
    );
    let local_goal = insert_local_goal(&fixture, &fixture.agents[0], contract).await;
    let short_lease_app = app(&fixture, Duration::from_secs(1));
    let (run_id, initialized) =
        create_run(&short_lease_app, &fixture, "local_goal", local_goal).await;
    let canonical_work_id = initialized["state"]["work_items"]
        .as_object()
        .and_then(|items| items.keys().next())
        .cloned()
        .expect("canonical work item");
    let claim_uri = format!(
        "/v1/projects/{}/agent-runs/{run_id}/claim",
        fixture.project_id
    );
    let first_request = request_json(
        short_lease_app.clone(),
        Method::POST,
        claim_uri.clone(),
        &fixture.agents[0].token,
        json!({}),
    );
    let second_request = request_json(
        short_lease_app.clone(),
        Method::POST,
        claim_uri,
        &fixture.agents[0].token,
        json!({}),
    );
    let (first, second) = tokio::join!(first_request, second_request);
    assert!(
        matches!(first.0, StatusCode::OK | StatusCode::CONFLICT),
        "first claimant: {}",
        first.1
    );
    assert!(
        matches!(second.0, StatusCode::OK | StatusCode::CONFLICT),
        "second claimant: {}",
        second.1
    );
    assert!(
        first.0 == StatusCode::OK || second.0 == StatusCode::OK,
        "at least one concurrent claimant must make bounded progress"
    );
    let claims = [
        (first.0 == StatusCode::OK).then_some(&first.1["claim"]),
        (second.0 == StatusCode::OK).then_some(&second.1["claim"]),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.is_null())
    .collect::<Vec<_>>();
    assert_eq!(claims.len(), 1, "exactly one concurrent claimant wins");
    let first_claim = claims[0];
    assert_eq!(first_claim["work"], canonical_work_id);
    assert_eq!(first_claim["attempt"], 1);
    let first_claim_id: Uuid = first_claim["id"]
        .as_str()
        .expect("claim id")
        .parse()
        .expect("claim UUID");
    let active_claims = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_claim_leases
         WHERE project_id = $1 AND run_id = $2 AND status = 'active'",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exclusive persistent leases");
    assert_eq!(active_claims, 1);

    tokio::time::sleep(Duration::from_secs(2)).await;
    let (status, _) = succeed(
        &short_lease_app,
        &fixture,
        run_id,
        &fixture.agents[0],
        first_claim_id,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "expired lease cannot commit");
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut worker_config = Config::for_test();
    worker_config.agent_work_lease = Duration::from_secs(1);
    worker::run(
        fixture.pool.clone(),
        worker_config,
        WorkerOptions {
            kind: WorkerKind::AgentCompletion,
            dry_run: false,
            once: true,
            interval: Duration::from_secs(1),
            lease_ttl_seconds: 5,
        },
        shutdown_rx,
    )
    .await
    .expect("persistent claim recovery worker");
    drop(shutdown_tx);
    let old_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM agent_run_claim_leases
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(first_claim_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load expired claim certificate");
    assert_eq!(old_status, "expired");
    let recovered_state = read_run(
        &app(&fixture, Duration::from_secs(1)),
        &fixture,
        run_id,
        &fixture.owner_token,
    )
    .await;
    assert_eq!(
        recovered_state["state"]["work_items"][&canonical_work_id]["status"],
        "eligible"
    );
    let replacement = claim(
        &app(&fixture, Duration::from_secs(1)),
        &fixture,
        run_id,
        &fixture.agents[0],
    )
    .await;
    assert_eq!(replacement["work"], canonical_work_id);
    assert_eq!(replacement["attempt"], 2);
    assert_ne!(replacement["id"], first_claim_id.to_string());

    let first_obligation = Uuid::new_v4();
    let second_obligation = Uuid::new_v4();
    let aging_trigger = create_open_task(&fixture, fixture.agents[0].identity_id, true).await;
    let task_done = json!({
        "kind": "task_done",
        "task": aging_trigger.resource_id
    });
    let mut scheduler_contract = goal_contract(
        fixture.scope_resource_id,
        &[
            (
                fixture.agents[0].identity_id,
                first_obligation,
                1,
                json!({ "kind": "always" }),
                true,
                vec![],
                0,
                "agent_action",
            ),
            (
                fixture.agents[0].identity_id,
                second_obligation,
                2,
                task_done.clone(),
                true,
                vec![],
                0,
                "agent_action",
            ),
        ],
    );
    scheduler_contract["obligations"][1]["activation"] = task_done.clone();
    scheduler_contract["obligations"][1]["required_for_completion"] = task_done;
    let scheduler_goal = insert_local_goal(&fixture, &fixture.agents[0], scheduler_contract).await;
    let scheduler_app = app(&fixture, Duration::from_secs(300));
    let (scheduler_run, _) =
        create_run(&scheduler_app, &fixture, "local_goal", scheduler_goal).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    complete_task_event(&fixture, fixture.agents[0].identity_id, &aging_trigger).await;
    let (status, aged_frontier) = request_json(
        scheduler_app.clone(),
        Method::POST,
        format!(
            "/v1/projects/{}/agent-runs/{scheduler_run}/refresh",
            fixture.project_id
        ),
        &fixture.owner_token,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "activate younger work: {aged_frontier}"
    );
    let created_ticks = aged_frontier["state"]["work_items"]
        .as_object()
        .expect("aged work projection")
        .values()
        .map(|work| {
            (
                work["work_spec_id"].as_u64().expect("work spec"),
                work["created_at"].as_u64().expect("created tick"),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert!(created_ticks[&1] < created_ticks[&2]);
    let scheduled_first = claim(&scheduler_app, &fixture, scheduler_run, &fixture.agents[0]).await;
    let scheduled_first_id: Uuid = scheduled_first["id"]
        .as_str()
        .expect("scheduled claim id")
        .parse()
        .expect("scheduled claim UUID");
    let persisted_lease_ticks = sqlx::query_scalar::<_, f64>(
        "SELECT EXTRACT(EPOCH FROM (expires_at - acquired_at))::double precision
         FROM agent_run_claim_leases WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(scheduled_first_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load WorkSpec-bounded persistent lease");
    assert_eq!(persisted_lease_ticks, 4.0);
    let after_first = read_run(
        &scheduler_app,
        &fixture,
        scheduler_run,
        &fixture.owner_token,
    )
    .await;
    assert_eq!(
        after_first["state"]["work_items"][scheduled_first["work"].as_str().expect("work")]["work_spec_id"],
        1,
        "aging selects the older eligible work first"
    );
    let first_work = scheduled_first["work"].as_str().expect("first work");
    assert_eq!(
        after_first["state"]["dispatches"][first_work]["scheduler_position"],
        0
    );
    let waiting = after_first["state"]["dispatches"]
        .as_object()
        .expect("dispatch projection")
        .iter()
        .find(|(id, _)| id.as_str() != first_work)
        .map(|(id, dispatch)| (id.clone(), dispatch["scheduler_position"].clone()))
        .expect("second dispatch");
    assert_eq!(waiting.1, 1);
    let scheduled_second = claim(&scheduler_app, &fixture, scheduler_run, &fixture.agents[0]).await;
    assert_eq!(scheduled_second["work"], waiting.0);
    let restarted_scheduler = read_run(
        &app(&fixture, Duration::from_secs(300)),
        &fixture,
        scheduler_run,
        &fixture.owner_token,
    )
    .await;
    assert_eq!(
        restarted_scheduler["state"]["dispatches"][&waiting.0]["scheduler_position"],
        0
    );
    assert_eq!(
        restarted_scheduler["state"]["work_items"][&waiting.0]["status"],
        "claimed"
    );
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn global_task_materialization_without_structured_grounding_fails_closed() {
    let fixture = fixture().await;
    let local_one_obligation = Uuid::new_v4();
    let local_two_obligation = Uuid::new_v4();
    let mut local_one_contract = goal_contract(
        fixture.scope_resource_id,
        &[(
            fixture.agents[0].identity_id,
            local_one_obligation,
            1,
            json!({ "kind": "always" }),
            true,
            vec![],
            0,
            "task_action",
        )],
    );
    local_one_contract["work_specs"][0]["allowed_actions"] = json!(["mark_assigned_done"]);
    let mut local_two_contract = goal_contract(
        fixture.scope_resource_id,
        &[(
            fixture.agents[1].identity_id,
            local_two_obligation,
            2,
            json!({ "kind": "always" }),
            true,
            vec![],
            0,
            "task_action",
        )],
    );
    local_two_contract["work_specs"][0]["allowed_actions"] = json!(["mark_assigned_done"]);
    let local_one = insert_local_goal(&fixture, &fixture.agents[0], local_one_contract).await;
    let local_two = insert_local_goal(&fixture, &fixture.agents[1], local_two_contract).await;

    let mut global_contract = goal_contract(
        fixture.scope_resource_id,
        &[
            (
                fixture.agents[0].identity_id,
                local_one_obligation,
                1,
                json!({ "kind": "always" }),
                true,
                vec![],
                0,
                "task_action",
            ),
            (
                fixture.agents[1].identity_id,
                local_two_obligation,
                2,
                json!({ "kind": "always" }),
                true,
                vec![],
                0,
                "task_action",
            ),
        ],
    );
    global_contract["work_specs"][0]["allowed_actions"] = json!(["mark_assigned_done"]);
    global_contract["work_specs"][1]["allowed_actions"] = json!(["mark_assigned_done"]);
    let global_id = insert_global_contract(
        &fixture,
        global_contract,
        &[
            (fixture.agents[0].clone(), local_one),
            (fixture.agents[1].clone(), local_two),
        ],
    )
    .await;
    let test_app = app(&fixture, Duration::from_secs(300));
    let (run_id, initialized) = create_run(&test_app, &fixture, "global_contract", global_id).await;
    assert_eq!(
        initialized["state"]["participants"]
            .as_array()
            .expect("global participants")
            .len(),
        2
    );
    let participant_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_participants
         WHERE project_id = $1 AND run_id = $2 AND participant_role = 'agent'",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count persisted global participants");
    assert_eq!(participant_count, 2);

    let first_claim = claim(&test_app, &fixture, run_id, &fixture.agents[0]).await;
    let first_task = create_open_task(&fixture, fixture.agents[0].identity_id, true).await;
    assert_global_task_materialization_without_grounding_fails_closed(
        &test_app,
        &fixture,
        &fixture.agents[0],
        run_id,
        &first_claim,
        &first_task,
    )
    .await;
    let reloaded = read_run(&test_app, &fixture, run_id, &fixture.owner_token).await;
    assert_eq!(reloaded["state"]["goal_status"], "active");
    assert_eq!(reloaded["state"]["run_status"], "running");
    assert_eq!(
        reloaded["state"]["work_items"][first_claim["work"].as_str().unwrap()]["status"],
        "claimed"
    );
}
