use std::{env, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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
    assert_eq!(audit_count, 6);
}
