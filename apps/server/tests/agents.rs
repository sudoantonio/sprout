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
use sqlx::{PgPool, postgres::PgPoolOptions};
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
                                "allowed_actions": ["replace_own_task"]
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
            "allowed_actions": ["replace_own_task"],
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
                .body(Body::from(
                    json!({
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
                            "candidate_operations": ["write"],
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
                                "operation": "write"
                            }],
                            "tool_invocations": [],
                            "encrypted_explanation": encrypted(9)
                        },
                        "action_classification": [{
                            "resource_id": fixture.profile_resource_id,
                            "action": "replace_own_task"
                        }],
                        "confirmation": null
                    })
                    .to_string(),
                ))
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

    let audit_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_audit_log WHERE project_id = $1 AND agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count agent audit records");
    assert_eq!(audit_count, 14);

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
    assert_eq!(remaining_agent_records, 0);
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
