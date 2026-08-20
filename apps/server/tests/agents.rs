use std::{env, sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sprout_crypto_protocol::{
    DeviceKeyIds, KeyAlgorithm, canonical_governance_json, generate_experimental_device_package,
    sign_ed25519_ml_dsa65,
};
use sprout_domain::{
    GlobalContractCandidate, LocalGoalContract, StructuredGlobalSynthesisEnvelope,
    StructuredGlobalWorkGrounding,
};
use sprout_server::{AppState, build_router, config::Config};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tokio::sync::oneshot;
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
    owner_ed25519_private_key: Vec<u8>,
    owner_ml_dsa_65_private_key: Vec<u8>,
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

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn canonical_hash_hex(value: &Value) -> String {
    sha256_hex(&canonical_governance_json(value).expect("canonical governance fixture"))
}

fn signed_statement(fixture: &Fixture, statement: &Value, context: &[u8]) -> Value {
    signed_statement_by(
        fixture.owner_id,
        fixture.owner_device_id,
        &fixture.owner_ed25519_private_key,
        &fixture.owner_ml_dsa_65_private_key,
        statement,
        context,
    )
}

fn signed_statement_by(
    identity_id: Uuid,
    device_id: Uuid,
    ed25519_private_key: &[u8],
    ml_dsa_65_private_key: &[u8],
    statement: &Value,
    context: &[u8],
) -> Value {
    let message = canonical_governance_json(statement).expect("canonical signed fixture");
    let signatures = sign_ed25519_ml_dsa65(
        ed25519_private_key,
        ml_dsa_65_private_key,
        &message,
        context,
    )
    .expect("sign governance fixture");
    json!({
        "signer_identity_id": identity_id,
        "signer_device_id": device_id,
        "signer_device_key_version": 1,
        "classical_signature": signatures.ed25519(),
        "post_quantum_signature": signatures.ml_dsa_65()
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
    let key_ids = DeviceKeyIds {
        x25519: Uuid::new_v4(),
        ml_kem_768: Uuid::new_v4(),
        ed25519: Uuid::new_v4(),
        ml_dsa_65: Uuid::new_v4(),
    };
    let generated = generate_experimental_device_package(owner_device_id, key_ids.clone())
        .expect("generate owner governance signing package");
    let public = generated.public_package();
    let x25519_public = public
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::X25519)
        .expect("X25519 fixture key")
        .public_key
        .clone();
    let ml_kem_public = public
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::MlKem768Experimental)
        .expect("ML-KEM fixture key")
        .public_key
        .clone();
    let ed25519_public = public
        .signing_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::Ed25519)
        .expect("Ed25519 fixture key")
        .public_key
        .clone();
    let ml_dsa_public = public
        .signing_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::MlDsa65Experimental)
        .expect("ML-DSA fixture key")
        .public_key
        .clone();
    let owner_ed25519_private_key = generated.private_keys().ed25519().to_vec();
    let owner_ml_dsa_65_private_key = generated.private_keys().ml_dsa_65().to_vec();
    let package_json = public
        .to_canonical_json()
        .expect("serialize owner governance signing package");
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
            suite_version, generation, previous_package_hash, package_hash,
            package_json, x25519_key_id, ml_kem_768_key_id,
            ed25519_key_id, ml_dsa_65_key_id,
            x25519_public_key, ml_kem_768_public_key,
            ed25519_public_key, ml_dsa_65_public_key
        ) VALUES (
            $1, $2, 1,
            $3, $4, 32769, 0, decode(repeat('00', 32), 'hex'),
            digest($5, 'sha256'), $5, $6, $7, $8, $9,
            $3, $10, $4, $11
        )
        "#,
    )
    .bind(owner_id)
    .bind(owner_device_id)
    .bind(&x25519_public)
    .bind(&ed25519_public)
    .bind(&package_json)
    .bind(key_ids.x25519)
    .bind(key_ids.ml_kem_768)
    .bind(key_ids.ed25519)
    .bind(key_ids.ml_dsa_65)
    .bind(&ml_kem_public)
    .bind(&ml_dsa_public)
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
        owner_ed25519_private_key,
        owner_ml_dsa_65_private_key,
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

struct ProvisionedGovernanceAgent {
    agent_id: Uuid,
    principal_identity_id: Uuid,
    runner_id: Uuid,
    runner_device_id: Uuid,
    bootstrap_token: Option<String>,
    local_goal_id: Uuid,
    compilation_certificate_id: Uuid,
    administrator_approval_id: Uuid,
}

async fn provision_administrator_governed_agent(
    fixture: &Fixture,
    app: &axum::Router,
    seed: u8,
    controller: Option<&SigningMember>,
    responsibility_id: Option<Uuid>,
    mutate: impl FnOnce(&mut Value),
    resign_after_mutation: bool,
) -> (StatusCode, ProvisionedGovernanceAgent) {
    let controller_identity_id =
        controller.map_or(fixture.owner_id, |controller| controller.identity_id);
    let controller_device_id =
        controller.map_or(fixture.owner_device_id, |controller| controller.device_id);
    let controller_token = controller.map_or(fixture.owner_token.as_str(), |controller| {
        controller.bearer.as_str()
    });
    let controller_ed25519_key = controller
        .map_or(fixture.owner_ed25519_private_key.as_slice(), |controller| {
            controller.ed25519_private_key.as_slice()
        });
    let controller_ml_dsa_key = controller.map_or(
        fixture.owner_ml_dsa_65_private_key.as_slice(),
        |controller| controller.ml_dsa_65_private_key.as_slice(),
    );
    assert_eq!(controller.is_some(), responsibility_id.is_some());
    let agent_id = Uuid::new_v4();
    let principal_identity_id = Uuid::new_v4();
    let local_goal_id = Uuid::new_v4();
    let compilation_certificate_id = Uuid::new_v4();
    let administrator_approval_id = Uuid::new_v4();
    let draft_id = Uuid::new_v4();
    let obligation_id = Uuid::new_v4();
    let goal_id = Uuid::new_v4();
    let runner_id = Uuid::new_v4();
    let runner_device_id = Uuid::new_v4();
    let prompt = encrypted(seed.wrapping_add(1));
    let prompt_payload: sprout_domain::EncryptedPayload =
        serde_json::from_value(prompt.clone()).expect("typed encrypted prompt fixture");
    let ciphertext_commitment_hex =
        sha256_hex(&serde_json::to_vec(&prompt_payload).expect("serialize prompt fixture"));
    let prompt_commitment_hex = "11".repeat(32);
    let contract = json!({
        "goal": goal_id,
        "scope": fixture.profile_resource_id,
        "obligations": [{
            "id": obligation_id,
            "goal": goal_id,
            "owner": principal_identity_id,
            "activation": {"kind": "always"},
            "required_for_completion": {"kind": "always"},
            "dependency_rank": 0
        }],
        "dependencies": [],
        "work_specs": [{
            "id": 1,
            "obligation": obligation_id,
            "owner": principal_identity_id,
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
    });
    let output = json!({
        "contract": contract,
        "requirements": [{
            "id": 1,
            "scope": fixture.profile_resource_id,
            "required_actions": ["post_comment"],
            "required_tools": [],
            "required_for_completion": true
        }],
        "bindings": [{
            "requirement_id": 1,
            "obligation": obligation_id,
            "work_spec_id": 1
        }],
        "security_policies": [{
            "work_spec_id": 1,
            "allowed_operations": ["post_comment"],
            "allowed_tools": []
        }]
    });
    let language_task_id = Uuid::new_v4();
    let envelope = json!({
        "language_task": {
            "id": language_task_id,
            "kind": "compile_goal_contract",
            "input_item_count": 1,
            "max_input_items": 1,
            "max_output_items": 8,
            "max_nesting_depth": 8,
            "max_attempts": 1,
            "closed_output_schema": true,
            "grounded_identifiers_only": true,
            "requires_formal_proof": false,
            "requires_permission_decision": false,
            "requires_exact_semantic_equivalence": false,
            "requires_exhaustive_world_knowledge": false,
            "allowed_resource_ids": [fixture.profile_resource_id],
            "allowed_principal_ids": [principal_identity_id, controller_identity_id],
            "allowed_tools": []
        },
        "agent": principal_identity_id,
        "controller": controller_identity_id,
        "project_scope": fixture.profile_resource_id,
        "allowed_actions": ["post_comment"],
        "max_requirements": 8,
        "max_obligations": 8,
        "max_work_specs": 8,
        "max_dependencies": 8
    });
    let compilation_statement = json!({
        "certificate_id": compilation_certificate_id,
        "compiler": {
            "compiler_id": "sprout.local-goal.compiler",
            "compiler_version": 1,
            "compiler_build_digest_hex": "0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2"
        },
        "project_id": fixture.project_id,
        "local_goal_id": local_goal_id,
        "local_revision": 1,
        "draft_id": draft_id,
        "agent_principal_identity_id": principal_identity_id,
        "controller_identity_id": controller_identity_id,
        "prompt_commitment_hex": prompt_commitment_hex,
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "output": output.clone(),
        "output_hash_hex": canonical_hash_hex(&output),
        "envelope": envelope.clone(),
        "envelope_hash_hex": canonical_hash_hex(&envelope),
        "authorization": responsibility_id.map_or_else(
            || json!({"kind": "administrator_creation", "approval_id": administrator_approval_id}),
            |id| json!({"kind": "responsibility", "id": id, "revision": 1})
        ),
        "idempotency_key": Uuid::new_v4()
    });
    let local_contract = json!({
        "id": local_goal_id,
        "revision": 1,
        "agent": principal_identity_id,
        "controller": controller_identity_id,
        "encrypted_prompt": prompt.clone(),
        "contract": contract.clone(),
        "clauses": [{
            "id": 1,
            "domain": 1,
            "scope": fixture.profile_resource_id,
            "work_spec_ids": [1]
        }],
        "origin": responsibility_id.map_or_else(
            || json!({"kind": "administrator_creation", "approval_id": administrator_approval_id}),
            |_| json!({"kind": "controller_prompt"})
        ),
        "supersedes_revision": null
    });
    let local_contract_hash_hex = canonical_hash_hex(&local_contract);
    let proposal_binding = json!({
        "project_id": fixture.project_id,
        "administrator_identity_id": controller_identity_id,
        "proposed_agent_identity_id": principal_identity_id,
        "governed_agent_id": agent_id,
        "proposal_draft_id": draft_id,
        "local_goal_id": local_goal_id,
        "local_goal_revision": 1,
        "contract_hash_hex": local_contract_hash_hex,
        "compilation_certificate_id": compilation_certificate_id,
        "prompt_plaintext_commitment_hex": prompt_commitment_hex,
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "availability": "controller_private",
        "scope": fixture.profile_resource_id
    });
    let administrator_statement = json!({
        "approval_id": administrator_approval_id,
        "project_id": fixture.project_id,
        "administrator_identity_id": controller_identity_id,
        "signer_device_id": controller_device_id,
        "signer_device_key_version": 1,
        "proposed_agent_identity_id": principal_identity_id,
        "governed_agent_id": agent_id,
        "proposal_draft_id": draft_id,
        "local_goal_id": local_goal_id,
        "local_goal_revision": 1,
        "contract_hash_hex": local_contract_hash_hex,
        "compilation_certificate_id": compilation_certificate_id,
        "prompt_plaintext_commitment_hex": prompt_commitment_hex,
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "availability": "controller_private",
        "scope": fixture.profile_resource_id,
        "canonical_proposal_hash_hex": canonical_hash_hex(&proposal_binding),
        "idempotency_key": Uuid::new_v4()
    });
    let final_approval_id = Uuid::new_v4();
    let final_idempotency_key = Uuid::new_v4();
    let approval_identity = json!({
        "signature_context": "sprout-final-prompt-approval-v1",
        "approval_id": final_approval_id,
        "project_id": fixture.project_id,
        "draft_id": draft_id,
        "agent_principal_identity_id": principal_identity_id,
        "controller_identity_id": controller_identity_id,
        "local_goal_id": local_goal_id,
        "local_revision": 1,
        "prompt_commitment_hex": prompt_commitment_hex,
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "compilation_certificate_id": compilation_certificate_id,
        "structured_output_hash_hex": canonical_hash_hex(&output),
        "idempotency_key": final_idempotency_key
    });
    let final_statement = json!({
        "approval_id": final_approval_id,
        "project_id": fixture.project_id,
        "draft_id": draft_id,
        "agent_principal_identity_id": principal_identity_id,
        "controller_identity_id": controller_identity_id,
        "local_goal_id": local_goal_id,
        "local_revision": 1,
        "prompt_commitment_hex": prompt_commitment_hex,
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "compilation_certificate_id": compilation_certificate_id,
        "structured_output_hash_hex": canonical_hash_hex(&output),
        "approval_identity_hash_hex": canonical_hash_hex(&approval_identity),
        "idempotency_key": final_idempotency_key
    });
    let administrator_creation_approval = responsibility_id.is_none().then(|| {
        json!({
            "statement": administrator_statement.clone(),
            "signatures": signed_statement_by(
                controller_identity_id,
                controller_device_id,
                controller_ed25519_key,
                controller_ml_dsa_key,
                &administrator_statement,
                b"sprout-administrator-agent-creation-v1"
            )
        })
    });
    let mut body = json!({
        "id": agent_id,
        "principal_identity_id": principal_identity_id,
        "controller_identity_id": controller_identity_id,
        "identity_handle": format!("compiled-agent-{}", principal_identity_id.simple()),
        "encrypted_profile": encrypted(seed),
        "profile_resource_node_id": fixture.profile_resource_id,
        "key_epoch": 1,
        "availability": "controller_private",
        "runner_id": runner_id,
        "runner_device_id": runner_device_id,
        "encrypted_runner_label": encrypted(seed.wrapping_add(2)),
        "initial_local_goal": {
            "encrypted_prompt": prompt.clone(),
            "supersedes_revision": null,
            "compilation": {
                "statement": compilation_statement.clone(),
                "signatures": signed_statement_by(
                    controller_identity_id,
                    controller_device_id,
                    controller_ed25519_key,
                    controller_ml_dsa_key,
                    &compilation_statement,
                    b"sprout-governance-compilation-v1"
                )
            }
        },
        "final_prompt_approval": {
            "statement": final_statement.clone(),
            "signatures": signed_statement_by(
                controller_identity_id,
                controller_device_id,
                controller_ed25519_key,
                controller_ml_dsa_key,
                &final_statement,
                b"sprout-final-prompt-approval-v1"
            )
        },
        "administrator_creation_approval": administrator_creation_approval
    });
    mutate(&mut body);
    if resign_after_mutation {
        for (pointer, signature_pointer, context) in [
            (
                "/initial_local_goal/compilation/statement",
                "/initial_local_goal/compilation/signatures",
                b"sprout-governance-compilation-v1".as_slice(),
            ),
            (
                "/final_prompt_approval/statement",
                "/final_prompt_approval/signatures",
                b"sprout-final-prompt-approval-v1".as_slice(),
            ),
            (
                "/administrator_creation_approval/statement",
                "/administrator_creation_approval/signatures",
                b"sprout-administrator-agent-creation-v1".as_slice(),
            ),
        ] {
            if let Some(statement) = body.pointer(pointer).cloned() {
                *body
                    .pointer_mut(signature_pointer)
                    .expect("governance signature fixture pointer") = signed_statement_by(
                    controller_identity_id,
                    controller_device_id,
                    controller_ed25519_key,
                    controller_ml_dsa_key,
                    &statement,
                    context,
                );
            }
        }
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agents", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {controller_token}"))
                .body(Body::from(body.to_string()))
                .expect("compiled agent creation request"),
        )
        .await
        .expect("compiled agent creation response");
    let response_status = response.status();
    let bootstrap_token = if response_status == StatusCode::OK {
        let response_body = json_body(response).await;
        Some(
            response_body["bootstrap_token"]
                .as_str()
                .expect("compiled creation bootstrap token")
                .to_owned(),
        )
    } else {
        None
    };
    (
        response_status,
        ProvisionedGovernanceAgent {
            agent_id,
            principal_identity_id,
            runner_id,
            runner_device_id,
            bootstrap_token,
            local_goal_id,
            compilation_certificate_id,
            administrator_approval_id,
        },
    )
}

async fn create_active_compiled_responsibility(
    fixture: &Fixture,
    app: &axum::Router,
    user_identity_id: Uuid,
    seed: u8,
) -> (StatusCode, Uuid) {
    create_active_compiled_responsibility_for_scope(
        fixture,
        app,
        user_identity_id,
        fixture.profile_resource_id,
        seed,
    )
    .await
}

async fn create_active_compiled_responsibility_for_scope(
    fixture: &Fixture,
    app: &axum::Router,
    user_identity_id: Uuid,
    scope: Uuid,
    seed: u8,
) -> (StatusCode, Uuid) {
    create_active_compiled_responsibility_for_scope_and_action(
        fixture,
        app,
        user_identity_id,
        scope,
        "post_comment",
        seed,
    )
    .await
}

async fn create_active_compiled_responsibility_for_scope_and_action(
    fixture: &Fixture,
    app: &axum::Router,
    user_identity_id: Uuid,
    scope: Uuid,
    action: &str,
    seed: u8,
) -> (StatusCode, Uuid) {
    let responsibility_id = Uuid::new_v4();
    let draft_id = Uuid::new_v4();
    let certificate_id = Uuid::new_v4();
    let source = encrypted(seed);
    let source_payload: sprout_domain::EncryptedPayload =
        serde_json::from_value(source.clone()).expect("typed responsibility ciphertext");
    let ciphertext_commitment_hex = sha256_hex(
        &serde_json::to_vec(&source_payload).expect("serialize responsibility ciphertext"),
    );
    let output = json!({
        "rules": [{
            "domain": 1,
            "scope": scope,
            "allowed_actions": [action]
        }]
    });
    let envelope = json!({
        "language_task": {
            "id": Uuid::new_v4(),
            "kind": "compile_responsibility_rules",
            "input_item_count": 1,
            "max_input_items": 1,
            "max_output_items": 8,
            "max_nesting_depth": 8,
            "max_attempts": 1,
            "closed_output_schema": true,
            "grounded_identifiers_only": true,
            "requires_formal_proof": false,
            "requires_permission_decision": false,
            "requires_exact_semantic_equivalence": false,
            "requires_exhaustive_world_knowledge": false,
            "allowed_resource_ids": [scope],
            "allowed_principal_ids": [fixture.owner_id, user_identity_id],
            "allowed_tools": []
        },
        "administrator": fixture.owner_id,
        "user": user_identity_id,
        "project_scopes": [scope],
        "allowed_actions": [action],
        "max_rules": 8
    });
    let statement = json!({
        "certificate_id": certificate_id,
        "compiler": {
            "compiler_id": "sprout.responsibility.compiler",
            "compiler_version": 1,
            "compiler_build_digest_hex": "78bd83db79112191f81aa118512092f7ea54a87733a82e823fa83cf107e3eb73"
        },
        "project_id": fixture.project_id,
        "responsibility_id": responsibility_id,
        "revision": 1,
        "draft_id": draft_id,
        "administrator_identity_id": fixture.owner_id,
        "user_identity_id": user_identity_id,
        "source_text_commitment_hex": "44".repeat(32),
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "output": output.clone(),
        "output_hash_hex": canonical_hash_hex(&output),
        "envelope": envelope.clone(),
        "envelope_hash_hex": canonical_hash_hex(&envelope),
        "idempotency_key": Uuid::new_v4()
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/users/{user_identity_id}/responsibilities/{responsibility_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "encrypted_source_text": source,
                        "supersedes_revision": null,
                        "compilation": {
                            "statement": statement.clone(),
                            "signatures": signed_statement(
                                fixture,
                                &statement,
                                b"sprout-governance-compilation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .expect("compiled responsibility request"),
        )
        .await
        .expect("compiled responsibility response");
    if response.status() != StatusCode::OK {
        return (response.status(), responsibility_id);
    }
    let activation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/users/{user_identity_id}/responsibilities/{responsibility_id}/revisions/1/activate",
                    fixture.project_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate compiled responsibility request"),
        )
        .await
        .expect("activate compiled responsibility response");
    (activation.status(), responsibility_id)
}

async fn record_compiled_responsibility_revision(
    fixture: &Fixture,
    app: &axum::Router,
    user_identity_id: Uuid,
    responsibility_id: Uuid,
    revision: u64,
    supersedes_revision: u64,
    seed: u8,
) -> StatusCode {
    let source = encrypted(seed);
    let source_payload: sprout_domain::EncryptedPayload =
        serde_json::from_value(source.clone()).expect("typed responsibility ciphertext");
    let ciphertext_commitment_hex = sha256_hex(
        &serde_json::to_vec(&source_payload).expect("serialize responsibility ciphertext"),
    );
    let output = json!({
        "rules": [{
            "domain": 1,
            "scope": fixture.profile_resource_id,
            "allowed_actions": ["post_comment"]
        }]
    });
    let envelope = json!({
        "language_task": {
            "id": Uuid::new_v4(),
            "kind": "compile_responsibility_rules",
            "input_item_count": 1,
            "max_input_items": 1,
            "max_output_items": 8,
            "max_nesting_depth": 8,
            "max_attempts": 1,
            "closed_output_schema": true,
            "grounded_identifiers_only": true,
            "requires_formal_proof": false,
            "requires_permission_decision": false,
            "requires_exact_semantic_equivalence": false,
            "requires_exhaustive_world_knowledge": false,
            "allowed_resource_ids": [fixture.profile_resource_id],
            "allowed_principal_ids": [fixture.owner_id, user_identity_id],
            "allowed_tools": []
        },
        "administrator": fixture.owner_id,
        "user": user_identity_id,
        "project_scopes": [fixture.profile_resource_id],
        "allowed_actions": ["post_comment"],
        "max_rules": 8
    });
    let statement = json!({
        "certificate_id": Uuid::new_v4(),
        "compiler": {
            "compiler_id": "sprout.responsibility.compiler",
            "compiler_version": 1,
            "compiler_build_digest_hex": "78bd83db79112191f81aa118512092f7ea54a87733a82e823fa83cf107e3eb73"
        },
        "project_id": fixture.project_id,
        "responsibility_id": responsibility_id,
        "revision": revision,
        "draft_id": Uuid::new_v4(),
        "administrator_identity_id": fixture.owner_id,
        "user_identity_id": user_identity_id,
        "source_text_commitment_hex": "44".repeat(32),
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "output": output.clone(),
        "output_hash_hex": canonical_hash_hex(&output),
        "envelope": envelope.clone(),
        "envelope_hash_hex": canonical_hash_hex(&envelope),
        "idempotency_key": Uuid::new_v4()
    });
    app.clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/users/{user_identity_id}/responsibilities/{responsibility_id}",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "encrypted_source_text": source,
                        "supersedes_revision": supersedes_revision,
                        "compilation": {
                            "statement": statement.clone(),
                            "signatures": signed_statement(
                                fixture,
                                &statement,
                                b"sprout-governance-compilation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .expect("compiled responsibility revision request"),
        )
        .await
        .expect("compiled responsibility revision response")
        .status()
}

async fn activate_certified_local_goal_revision(
    fixture: &Fixture,
    app: &axum::Router,
    controller: &SigningMember,
    agent_id: Uuid,
    local_goal_id: Uuid,
    responsibility_id: Uuid,
    seed: u8,
) -> (StatusCode, StatusCode, Value) {
    let previous = sqlx::query(
        "SELECT certificate.canonical_output::text AS output,
                certificate.compilation_envelope::text AS envelope
         FROM agent_local_goal_contracts local
         JOIN agent_compilation_certificates certificate
           ON certificate.project_id = local.project_id
          AND certificate.id = local.compilation_certificate_id
         WHERE local.project_id = $1 AND local.agent_id = $2
           AND local.id = $3 AND local.revision = 1 AND local.state = 'active'",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .bind(local_goal_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load active compiler output for revision");
    let output: Value = serde_json::from_str(previous.try_get("output").unwrap()).unwrap();
    let envelope: Value = serde_json::from_str(previous.try_get("envelope").unwrap()).unwrap();
    let agent_identity_id = output["contract"]["work_specs"][0]["owner"]
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("revision agent identity");
    let draft_id = Uuid::new_v4();
    let certificate_id = Uuid::new_v4();
    let prompt = encrypted(seed);
    let prompt_payload: sprout_domain::EncryptedPayload =
        serde_json::from_value(prompt.clone()).unwrap();
    let ciphertext_commitment_hex = sha256_hex(&serde_json::to_vec(&prompt_payload).unwrap());
    let statement = json!({
        "certificate_id": certificate_id,
        "compiler": {
            "compiler_id": "sprout.local-goal.compiler",
            "compiler_version": 1,
            "compiler_build_digest_hex": "0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2"
        },
        "project_id": fixture.project_id,
        "local_goal_id": local_goal_id,
        "local_revision": 2,
        "draft_id": draft_id,
        "agent_principal_identity_id": agent_identity_id,
        "controller_identity_id": controller.identity_id,
        "prompt_commitment_hex": "55".repeat(32),
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "output": output.clone(),
        "output_hash_hex": canonical_hash_hex(&output),
        "envelope": envelope.clone(),
        "envelope_hash_hex": canonical_hash_hex(&envelope),
        "authorization": {"kind": "responsibility", "id": responsibility_id, "revision": 1},
        "idempotency_key": Uuid::new_v4()
    });
    let draft = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(
                    json!({
                        "encrypted_prompt": prompt,
                        "supersedes_revision": 1,
                        "compilation": {
                            "statement": statement.clone(),
                            "signatures": signed_statement_by(
                                controller.identity_id,
                                controller.device_id,
                                &controller.ed25519_private_key,
                                &controller.ml_dsa_65_private_key,
                                &statement,
                                b"sprout-governance-compilation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    if draft.status() != StatusCode::OK {
        return (draft.status(), draft.status(), Value::Null);
    }
    let approval_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();
    let approval_identity = json!({
        "signature_context": "sprout-final-prompt-approval-v1",
        "approval_id": approval_id,
        "project_id": fixture.project_id,
        "draft_id": draft_id,
        "agent_principal_identity_id": agent_identity_id,
        "controller_identity_id": controller.identity_id,
        "local_goal_id": local_goal_id,
        "local_revision": 2,
        "prompt_commitment_hex": "55".repeat(32),
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "compilation_certificate_id": certificate_id,
        "structured_output_hash_hex": canonical_hash_hex(&output),
        "idempotency_key": idempotency_key
    });
    let approval_statement = json!({
        "approval_id": approval_id,
        "project_id": fixture.project_id,
        "draft_id": draft_id,
        "agent_principal_identity_id": agent_identity_id,
        "controller_identity_id": controller.identity_id,
        "local_goal_id": local_goal_id,
        "local_revision": 2,
        "prompt_commitment_hex": "55".repeat(32),
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "compilation_certificate_id": certificate_id,
        "structured_output_hash_hex": canonical_hash_hex(&output),
        "approval_identity_hash_hex": canonical_hash_hex(&approval_identity),
        "idempotency_key": idempotency_key
    });
    let activation_body = json!({
        "statement": approval_statement.clone(),
        "signatures": signed_statement_by(
            controller.identity_id,
            controller.device_id,
            &controller.ed25519_private_key,
            &controller.ml_dsa_65_private_key,
            &approval_statement,
            b"sprout-final-prompt-approval-v1"
        )
    });
    let activation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/local-goals/{local_goal_id}/revisions/2/activate",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(activation_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    (draft.status(), activation.status(), activation_body)
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read JSON response");
    serde_json::from_slice(&bytes).expect("decode JSON response")
}

async fn semantic_operational_lists(
    pool: &PgPool,
    project_id: Uuid,
    identity_id: Uuid,
    device_id: Uuid,
) -> (Vec<Value>, Vec<Value>) {
    let mut transaction = pool.begin().await.expect("begin semantic-list projection");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(identity_id.to_string())
    .bind(device_id.to_string())
    .bind(project_id.to_string())
    .execute(&mut *transaction)
    .await
    .expect("set authenticated semantic-list context");
    let intents = sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(item) ORDER BY item.semantic_position), '[]'::jsonb
         )
         FROM sprout_private.semantic_task_intent_list($1) item",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("project ordered semantic TaskIntent list");
    let provenance = sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(item) ORDER BY item.semantic_position), '[]'::jsonb
         )
         FROM sprout_private.semantic_task_provenance_list($1) item",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("project ordered semantic task provenance list");
    transaction
        .commit()
        .await
        .expect("commit semantic-list projection");
    (
        intents.as_array().cloned().expect("TaskIntent list"),
        provenance
            .as_array()
            .cloned()
            .expect("task provenance list"),
    )
}

fn assert_prefix(before: &[Value], after: &[Value]) {
    assert!(
        after.starts_with(before),
        "semantic list lost its exact prefix"
    );
    let positions = after
        .iter()
        .map(|entry| entry["semantic_position"].as_i64().expect("position"))
        .collect::<Vec<_>>();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "semantic positions must be strictly monotone"
    );
}

async fn retained_history_snapshot(pool: &PgPool, project_id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>(
        r#"
        SELECT jsonb_build_object(
          'intents', COALESCE((
            SELECT jsonb_agg(to_jsonb(row) ORDER BY row.id)
            FROM agent_task_intent_retained_history row
            WHERE row.project_id = $1
          ), '[]'::jsonb),
          'provenance', COALESCE((
            SELECT jsonb_agg(to_jsonb(row) ORDER BY row.id)
            FROM agent_task_obligation_retained_history row
            WHERE row.project_id = $1
          ), '[]'::jsonb),
          'effects', COALESCE((
            SELECT jsonb_agg(to_jsonb(row) ORDER BY row.effect_kind, row.effect_id)
            FROM agent_product_effect_retained_history row
            WHERE row.project_id = $1
          ), '[]'::jsonb),
          'causal_links', COALESCE((
            SELECT jsonb_agg(to_jsonb(row) ORDER BY row.causal_position)
            FROM agent_run_causal_link_retained_history row
            WHERE row.project_id = $1
          ), '[]'::jsonb)
        )
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .expect("snapshot exact retained semantic history")
}

async fn semantic_ledger_snapshot(pool: &PgPool, project_id: Uuid) -> Value {
    sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(row) ORDER BY row.semantic_position), '[]'::jsonb
         )
         FROM agent_semantic_operational_ledger row
         WHERE row.project_id = $1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .expect("snapshot exact semantic ledger")
}

async fn wait_for_backend_lock(pool: &PgPool, backend_pid: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting = sqlx::query_scalar::<_, bool>(
                "SELECT COALESCE(wait_event_type = 'Lock', false)
                 FROM pg_stat_activity WHERE pid = $1",
            )
            .bind(backend_pid)
            .fetch_one(pool)
            .await
            .expect("observe concurrent semantic append backend");
            if waiting {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("concurrent semantic append never reached a real lock wait");
}

async fn controlled_concurrent_intent_pair(
    pool: &PgPool,
    project_id: Uuid,
    task_resource_id: Uuid,
    controller_id: Uuid,
    winner_id: Uuid,
    waiter_id: Uuid,
) -> (i64, i64) {
    let mut winner = pool.begin().await.expect("open winning append transaction");
    let mut waiter = pool.begin().await.expect("open waiting append transaction");
    let winner_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *winner)
        .await
        .expect("load winning backend PID");
    let waiter_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *waiter)
        .await
        .expect("load waiting backend PID");
    assert_ne!(winner_pid, waiter_pid, "both transactions must be open");
    for transaction in [&mut winner, &mut waiter] {
        sqlx::query("SET LOCAL row_security = off")
            .execute(&mut **transaction)
            .await
            .expect("disable RLS for controlled concurrency fixture");
    }
    sqlx::query(
        "UPDATE sprout_private.semantic_operational_cursor
         SET last_position = last_position WHERE singleton",
    )
    .execute(&mut *winner)
    .await
    .expect("winner acquires semantic cursor lock");

    let waiter_pool = pool.clone();
    let (attempting_tx, attempting_rx) = oneshot::channel();
    let waiter_task = tokio::spawn(async move {
        attempting_tx
            .send(())
            .expect("signal waiting append attempt");
        sqlx::query(
            "INSERT INTO agent_task_intents (
                 id, project_id, task_resource_node_id, scope_resource_node_id,
                 required_actions, derived_by_identity_id
             ) VALUES ($1, $2, $3, $3, '[\"assign_own_task\"]'::jsonb, $4)",
        )
        .bind(waiter_id)
        .bind(project_id)
        .bind(task_resource_id)
        .bind(controller_id)
        .execute(&mut *waiter)
        .await
        .expect("waiting concurrent append succeeds after release");
        waiter
            .commit()
            .await
            .expect("commit waiting concurrent append");
    });
    attempting_rx.await.expect("waiting append started");
    wait_for_backend_lock(&waiter_pool, waiter_pid).await;

    sqlx::query(
        "INSERT INTO agent_task_intents (
             id, project_id, task_resource_node_id, scope_resource_node_id,
             required_actions, derived_by_identity_id
         ) VALUES ($1, $2, $3, $3, '[\"assign_own_task\"]'::jsonb, $4)",
    )
    .bind(winner_id)
    .bind(project_id)
    .bind(task_resource_id)
    .bind(controller_id)
    .execute(&mut *winner)
    .await
    .expect("insert winning concurrent TaskIntent");
    winner
        .commit()
        .await
        .expect("commit winning concurrent append");
    waiter_task.await.expect("join waiting concurrent append");

    let positions = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT record_id, semantic_position
         FROM agent_semantic_operational_ledger
         WHERE project_id = $1 AND entry_kind = 'task_intent'
           AND record_id = ANY($2)
         ORDER BY semantic_position",
    )
    .bind(project_id)
    .bind(vec![winner_id, waiter_id])
    .fetch_all(pool)
    .await
    .expect("load controlled concurrent positions");
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].0, winner_id);
    assert_eq!(positions[1].0, waiter_id);
    assert_eq!(positions[1].1, positions[0].1 + 1);
    (positions[0].1, positions[1].1)
}

#[derive(Clone, Copy)]
struct RetentionGate {
    subject_id: Uuid,
    lease_token: Uuid,
    now: chrono::DateTime<Utc>,
}

async fn prepare_retention_gate(
    fixture: &Fixture,
    resource_node_id: Uuid,
    owner_identity_id: Uuid,
) -> RetentionGate {
    let gate = RetentionGate {
        subject_id: Uuid::new_v4(),
        lease_token: Uuid::new_v4(),
        now: Utc::now(),
    };
    let deleted_at = gate.now - ChronoDuration::days(20);
    let mut transaction = fixture.pool.begin().await.expect("begin retention gate");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for retention gate");
    sqlx::query(
        "UPDATE resource_nodes SET deleted_at = $3
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(resource_node_id)
    .bind(deleted_at)
    .execute(&mut *transaction)
    .await
    .expect("soft-delete retention gate resource");
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
    .bind(gate.subject_id)
    .bind(fixture.project_id)
    .bind(resource_node_id)
    .bind(owner_identity_id)
    .bind(deleted_at)
    .bind(deleted_at + ChronoDuration::days(1))
    .bind(deleted_at + ChronoDuration::days(15))
    .bind(Uuid::new_v4())
    .bind(gate.lease_token)
    .bind(gate.now + ChronoDuration::hours(1))
    .execute(&mut *transaction)
    .await
    .expect("insert retention gate subject");
    transaction.commit().await.expect("commit retention gate");
    gate
}

async fn controlled_append_before_purge(
    pool: &PgPool,
    gate: RetentionGate,
    project_id: Uuid,
    task_resource_id: Uuid,
    controller_id: Uuid,
    intent_id: Uuid,
) -> bool {
    let mut append = pool
        .begin()
        .await
        .expect("open append-before-purge transaction");
    let mut purge = pool.begin().await.expect("open blocked purge transaction");
    let append_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *append)
        .await
        .expect("append-before-purge PID");
    let purge_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *purge)
        .await
        .expect("blocked purge PID");
    assert_ne!(append_pid, purge_pid);
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *append)
        .await
        .expect("disable RLS for controlled append");
    sqlx::query("SELECT id FROM retention_subjects WHERE id = $1 FOR UPDATE")
        .bind(gate.subject_id)
        .execute(&mut *append)
        .await
        .expect("append transaction fences retention subject");
    sqlx::query(
        "INSERT INTO agent_task_intents (
             id, project_id, task_resource_node_id, scope_resource_node_id,
             required_actions, derived_by_identity_id
         ) VALUES ($1, $2, $3, $3, '[\"assign_own_task\"]'::jsonb, $4)",
    )
    .bind(intent_id)
    .bind(project_id)
    .bind(task_resource_id)
    .bind(controller_id)
    .execute(&mut *append)
    .await
    .expect("append intent before blocked purge");

    let observe_pool = pool.clone();
    let (attempting_tx, attempting_rx) = oneshot::channel();
    let purge_task = tokio::spawn(async move {
        attempting_tx.send(()).expect("signal purge attempt");
        let purged = sqlx::query_scalar::<_, bool>(
            "SELECT sprout_private.purge_retention_subject($1, $2, $3)",
        )
        .bind(gate.subject_id)
        .bind(gate.lease_token)
        .bind(gate.now)
        .fetch_one(&mut *purge)
        .await
        .expect("purge succeeds after append commit");
        purge.commit().await.expect("commit purge after append");
        purged
    });
    attempting_rx.await.expect("purge attempt started");
    wait_for_backend_lock(&observe_pool, purge_pid).await;
    append.commit().await.expect("commit append before purge");
    purge_task.await.expect("join purge after append")
}

async fn controlled_purge_before_append(
    pool: &PgPool,
    gate: RetentionGate,
    project_id: Uuid,
    task_resource_id: Uuid,
    controller_id: Uuid,
    intent_id: Uuid,
) -> bool {
    let mut purge = pool
        .begin()
        .await
        .expect("open purge-before-append transaction");
    let mut append = pool.begin().await.expect("open blocked append transaction");
    let purge_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *purge)
        .await
        .expect("purge-before-append PID");
    let append_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *append)
        .await
        .expect("blocked append PID");
    assert_ne!(purge_pid, append_pid);
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *append)
        .await
        .expect("disable RLS for blocked append");
    sqlx::query(
        "UPDATE sprout_private.semantic_operational_cursor
         SET last_position = last_position WHERE singleton",
    )
    .execute(&mut *purge)
    .await
    .expect("purge transaction fences semantic cursor");

    let observe_pool = pool.clone();
    let (attempting_tx, attempting_rx) = oneshot::channel();
    let append_task = tokio::spawn(async move {
        attempting_tx.send(()).expect("signal append attempt");
        sqlx::query(
            "INSERT INTO agent_task_intents (
                 id, project_id, task_resource_node_id, scope_resource_node_id,
                 required_actions, derived_by_identity_id
             ) VALUES ($1, $2, $3, $3, '[\"assign_own_task\"]'::jsonb, $4)",
        )
        .bind(intent_id)
        .bind(project_id)
        .bind(task_resource_id)
        .bind(controller_id)
        .execute(&mut *append)
        .await
        .expect("append succeeds after purge commit");
        append.commit().await.expect("commit append after purge");
    });
    attempting_rx.await.expect("append attempt started");
    wait_for_backend_lock(&observe_pool, append_pid).await;
    let purged =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.purge_retention_subject($1, $2, $3)")
            .bind(gate.subject_id)
            .bind(gate.lease_token)
            .bind(gate.now)
            .fetch_one(&mut *purge)
            .await
            .expect("purge while append is blocked");
    purge.commit().await.expect("commit purge before append");
    append_task.await.expect("join append after purge");
    purged
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

struct SigningMember {
    identity_id: Uuid,
    device_id: Uuid,
    bearer: String,
    permission_root_id: Uuid,
    ed25519_private_key: Vec<u8>,
    ml_dsa_65_private_key: Vec<u8>,
}

async fn add_signing_human_member(fixture: &Fixture, role: &str) -> SigningMember {
    let (identity_id, device_id, bearer, permission_root_id) =
        add_human_member(fixture, role).await;
    let key_ids = DeviceKeyIds {
        x25519: Uuid::new_v4(),
        ml_kem_768: Uuid::new_v4(),
        ed25519: Uuid::new_v4(),
        ml_dsa_65: Uuid::new_v4(),
    };
    let generated = generate_experimental_device_package(device_id, key_ids.clone())
        .expect("generate member governance signing package");
    let public = generated.public_package();
    let key = |algorithm| {
        public
            .encryption_keys
            .iter()
            .chain(&public.signing_keys)
            .find(|key| key.algorithm == algorithm)
            .expect("member fixture public key")
            .public_key
            .clone()
    };
    let x25519_public = key(KeyAlgorithm::X25519);
    let ml_kem_public = key(KeyAlgorithm::MlKem768Experimental);
    let ed25519_public = key(KeyAlgorithm::Ed25519);
    let ml_dsa_public = key(KeyAlgorithm::MlDsa65Experimental);
    let package_json = public
        .to_canonical_json()
        .expect("serialize member governance signing package");
    let mut transaction = fixture.pool.begin().await.expect("begin member keys");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for member keys");
    sqlx::query(
        r#"
        INSERT INTO device_keys (
            identity_id, device_id, key_version, encryption_public_key,
            signing_public_key, suite_version, generation,
            previous_package_hash, package_hash, package_json,
            x25519_key_id, ml_kem_768_key_id, ed25519_key_id, ml_dsa_65_key_id,
            x25519_public_key, ml_kem_768_public_key,
            ed25519_public_key, ml_dsa_65_public_key
        ) VALUES (
            $1, $2, 1, $3, $4, 32769, 0, decode(repeat('00', 32), 'hex'),
            digest($5, 'sha256'), $5, $6, $7, $8, $9, $3, $10, $4, $11
        )
        "#,
    )
    .bind(identity_id)
    .bind(device_id)
    .bind(&x25519_public)
    .bind(&ed25519_public)
    .bind(package_json)
    .bind(key_ids.x25519)
    .bind(key_ids.ml_kem_768)
    .bind(key_ids.ed25519)
    .bind(key_ids.ml_dsa_65)
    .bind(ml_kem_public)
    .bind(ml_dsa_public)
    .execute(&mut *transaction)
    .await
    .expect("insert member governance keys");
    transaction.commit().await.expect("commit member keys");
    SigningMember {
        identity_id,
        device_id,
        bearer,
        permission_root_id,
        ed25519_private_key: generated.private_keys().ed25519().to_vec(),
        ml_dsa_65_private_key: generated.private_keys().ml_dsa_65().to_vec(),
    }
}

async fn provision_controlled_agent(
    fixture: &Fixture,
    _app: &axum::Router,
    controller_identity_id: Uuid,
    profile_resource_node_id: Uuid,
    seed: u8,
) -> (Uuid, Uuid, Uuid, Uuid) {
    let agent_id = Uuid::new_v4();
    let principal_identity_id = Uuid::new_v4();
    let runner_id = Uuid::new_v4();
    let runner_device_id = Uuid::new_v4();
    let mut transaction = fixture.pool.begin().await.expect("begin agent fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for agent fixture");
    sqlx::query(
        "INSERT INTO identities (id, identity_handle, encrypted_profile, principal_kind)
         VALUES ($1, $2, $3, 'agent')",
    )
    .bind(principal_identity_id)
    .bind(format!("shared-agent-{}", principal_identity_id.simple()))
    .bind(serde_json::to_vec(&encrypted(seed)).unwrap())
    .execute(&mut *transaction)
    .await
    .expect("insert fixture agent identity");
    sqlx::query(
        "INSERT INTO project_memberships (project_id, identity_id, role)
         VALUES ($1, $2, 'member')",
    )
    .bind(fixture.project_id)
    .bind(principal_identity_id)
    .execute(&mut *transaction)
    .await
    .expect("insert fixture agent membership");
    sqlx::query(
        "INSERT INTO devices (id, identity_id, device_kind, encrypted_label, trust_state)
         VALUES ($1, $2, 'service', $3, 'trusted')",
    )
    .bind(runner_device_id)
    .bind(principal_identity_id)
    .bind(serde_json::to_vec(&encrypted(seed.wrapping_add(2))).unwrap())
    .execute(&mut *transaction)
    .await
    .expect("insert fixture agent device");
    sqlx::query(
        "INSERT INTO sessions (id, identity_id, device_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4, clock_timestamp() + interval '1 hour')",
    )
    .bind(Uuid::new_v4())
    .bind(principal_identity_id)
    .bind(runner_device_id)
    .bind(Sha256::digest(Uuid::new_v4().as_bytes()).to_vec())
    .execute(&mut *transaction)
    .await
    .expect("insert fixture runner session");
    sqlx::query(
        "INSERT INTO governed_agents (
            id, project_id, principal_identity_id, controller_identity_id,
            profile_resource_node_id, encrypted_system_prompt, key_epoch, availability
         ) VALUES ($1, $2, $3, $4, $5, $6, 1, 'controller_private')",
    )
    .bind(agent_id)
    .bind(fixture.project_id)
    .bind(principal_identity_id)
    .bind(controller_identity_id)
    .bind(profile_resource_node_id)
    .bind(serde_json::to_vec(&encrypted(seed.wrapping_add(1))).unwrap())
    .execute(&mut *transaction)
    .await
    .expect("insert fixture governed agent");
    sqlx::query(
        "INSERT INTO agent_runners (
            id, project_id, agent_id, principal_identity_id, device_id
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(runner_id)
    .bind(fixture.project_id)
    .bind(agent_id)
    .bind(principal_identity_id)
    .bind(runner_device_id)
    .execute(&mut *transaction)
    .await
    .expect("insert fixture runner");
    transaction.commit().await.expect("commit agent fixture");
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

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn exact_administrator_creation_is_atomic_and_does_not_grant_authority() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let authority_before = sqlx::query_scalar::<_, i64>(
        "SELECT
            (SELECT count(*) FROM topic_permissions WHERE project_id = $1)
          + (SELECT count(*) FROM task_list_permissions WHERE project_id = $1)
          + (SELECT count(*) FROM task_permissions WHERE project_id = $1)
          + (SELECT count(*) FROM resource_key_envelopes WHERE project_id = $1)",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count authority projection before creation");
    let (status, created) =
        provision_administrator_governed_agent(&fixture, &app, 111, None, None, |_| {}, false)
            .await;
    assert_eq!(status, StatusCode::OK);
    let exact_projection = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT local.state = 'active'
           AND prompt.state = 'active'
           AND certificate.verification_state = 'verified'
           AND approval.approval_id = local.administrator_creation_approval_id
           AND local.contract #>> '{origin,kind}' = 'administrator_creation'
           AND (local.contract #>> '{origin,approval_id}')::uuid = approval.approval_id
           AND approval.proposed_agent_identity_id = agent.principal_identity_id
           AND approval.governed_agent_id = agent.id
           AND approval.compilation_certificate_id = certificate.id
           AND final.compilation_certificate_id = certificate.id
           AND final.structured_output_hash = certificate.output_hash
        FROM governed_agents agent
        JOIN agent_local_goal_contracts local
          ON local.project_id = agent.project_id AND local.agent_id = agent.id
        JOIN agent_prompt_revisions prompt
          ON prompt.project_id = local.project_id AND prompt.agent_id = local.agent_id
         AND prompt.local_goal_id = local.id AND prompt.local_goal_revision = local.revision
        JOIN agent_compilation_certificates certificate
          ON certificate.project_id = local.project_id
         AND certificate.id = local.compilation_certificate_id
        JOIN agent_administrator_creation_approvals approval
          ON approval.project_id = local.project_id
         AND approval.approval_id = local.administrator_creation_approval_id
        JOIN agent_prompt_final_approvals final
          ON final.project_id = prompt.project_id AND final.draft_id = prompt.draft_id
        WHERE agent.project_id = $1 AND agent.id = $2
        "#,
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact administrator creation projection");
    assert!(exact_projection);
    let ledger_entries = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_governance_ledger
         WHERE project_id = $1 AND subject_id = $2",
    )
    .bind(fixture.project_id)
    .bind(created.local_goal_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count ordered governance witnesses");
    assert_eq!(ledger_entries, 4);
    let responsibility_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_responsibility_contracts
         WHERE project_id = $1 AND user_identity_id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.owner_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count fake administrator responsibilities");
    assert_eq!(responsibility_count, 0);
    let authority_after = sqlx::query_scalar::<_, i64>(
        "SELECT
            (SELECT count(*) FROM topic_permissions WHERE project_id = $1)
          + (SELECT count(*) FROM task_list_permissions WHERE project_id = $1)
          + (SELECT count(*) FROM task_permissions WHERE project_id = $1)
          + (SELECT count(*) FROM resource_key_envelopes WHERE project_id = $1)",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count authority projection after creation");
    assert_eq!(authority_after, authority_before);
    assert_ne!(created.principal_identity_id, fixture.owner_id);
    assert_ne!(created.compilation_certificate_id, Uuid::nil());
    assert_ne!(created.administrator_approval_id, Uuid::nil());
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn normal_user_creation_requires_active_compiled_responsibility() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let controller = add_signing_human_member(&fixture, "member").await;
    let (responsibility_status, responsibility_id) =
        create_active_compiled_responsibility(&fixture, &app, controller.identity_id, 112).await;
    assert_eq!(responsibility_status, StatusCode::OK);
    let (status, created) = provision_administrator_governed_agent(
        &fixture,
        &app,
        113,
        Some(&controller),
        Some(responsibility_id),
        |_| {},
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let exact = sqlx::query_scalar::<_, bool>(
        "SELECT certificate.authorization_kind = 'responsibility'
             AND certificate.authorization_id = $3
             AND responsibility.state = 'active'
             AND responsibility.user_identity_id = $4
             AND local.administrator_creation_approval_id IS NULL
         FROM agent_local_goal_contracts local
         JOIN agent_compilation_certificates certificate
           ON certificate.project_id = local.project_id
          AND certificate.id = local.compilation_certificate_id
         JOIN agent_responsibility_contracts responsibility
           ON responsibility.project_id = certificate.project_id
          AND responsibility.id = certificate.authorization_id
          AND responsibility.revision = certificate.authorization_revision
         WHERE local.project_id = $1 AND local.agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .bind(responsibility_id)
    .bind(controller.identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load responsibility-authorized creation");
    assert!(exact);
    let administrator_approval_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_administrator_creation_approvals
         WHERE project_id = $1 AND governed_agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count inapplicable administrator approvals");
    assert_eq!(administrator_approval_count, 0);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn responsibility_or_permission_revoked_before_creation_fails_closed() {
    let fixture = fixture().await;
    let app = app(&fixture);

    let revoked_responsibility_controller = add_signing_human_member(&fixture, "member").await;
    let (status, revoked_responsibility_id) = create_active_compiled_responsibility(
        &fixture,
        &app,
        revoked_responsibility_controller.identity_id,
        114,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query(
        "UPDATE agent_responsibility_contracts
         SET state = 'superseded', superseded_at = clock_timestamp()
         WHERE project_id = $1 AND id = $2 AND revision = 1 AND state = 'active'",
    )
    .bind(fixture.project_id)
    .bind(revoked_responsibility_id)
    .execute(&fixture.pool)
    .await
    .expect("supersede responsibility before creation");
    let (revoked_status, revoked_candidate) = provision_administrator_governed_agent(
        &fixture,
        &app,
        115,
        Some(&revoked_responsibility_controller),
        Some(revoked_responsibility_id),
        |_| {},
        false,
    )
    .await;
    assert_ne!(revoked_status, StatusCode::OK);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identities WHERE id = $1")
            .bind(revoked_candidate.principal_identity_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        0
    );

    let revoked_permission_controller = add_signing_human_member(&fixture, "member").await;
    let (status, responsibility_id) = create_active_compiled_responsibility(
        &fixture,
        &app,
        revoked_permission_controller.identity_id,
        116,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut revoke = fixture
        .pool
        .begin()
        .await
        .expect("begin permission revocation");
    sqlx::query("SELECT set_config('app.identity_id', $1::text, true)")
        .bind(fixture.owner_id)
        .execute(&mut *revoke)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('app.device_id', $1::text, true)")
        .bind(fixture.owner_device_id)
        .execute(&mut *revoke)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('app.project_id', $1::text, true)")
        .bind(fixture.project_id)
        .execute(&mut *revoke)
        .await
        .unwrap();
    sqlx::query("SELECT sprout_private.revoke_hierarchical_permission($1, $2, $3, $4)")
        .bind(fixture.project_id)
        .bind(revoked_permission_controller.permission_root_id)
        .bind(revoked_permission_controller.identity_id)
        .bind(fixture.owner_id)
        .execute(&mut *revoke)
        .await
        .expect("revoke controller permission before creation");
    revoke.commit().await.unwrap();
    let (revoked_status, revoked_candidate) = provision_administrator_governed_agent(
        &fixture,
        &app,
        117,
        Some(&revoked_permission_controller),
        Some(responsibility_id),
        |_| {},
        false,
    )
    .await;
    assert_ne!(revoked_status, StatusCode::OK);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identities WHERE id = $1")
            .bind(revoked_candidate.principal_identity_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn administrator_creation_mismatches_roll_back_every_phase() {
    let fixture = fixture().await;
    let app = app(&fixture);
    for case in 0_u8..13 {
        let (status, candidate) = provision_administrator_governed_agent(
            &fixture,
            &app,
            120_u8.wrapping_add(case),
            None,
            None,
            |body| match case {
                0 => {
                    body["administrator_creation_approval"]["statement"]["project_id"] =
                        json!(Uuid::new_v4());
                }
                1 => {
                    body["administrator_creation_approval"]["statement"]
                        ["proposed_agent_identity_id"] = json!(Uuid::new_v4());
                }
                2 => {
                    body["administrator_creation_approval"]["statement"]["proposal_draft_id"] =
                        json!(Uuid::new_v4());
                }
                3 => {
                    body["administrator_creation_approval"]["statement"]
                        ["prompt_plaintext_commitment_hex"] = json!("22".repeat(32));
                }
                4 => {
                    body["administrator_creation_approval"]["statement"]
                        ["ciphertext_commitment_hex"] = json!("22".repeat(32));
                }
                5 => {
                    body["administrator_creation_approval"]["statement"]["local_goal_id"] =
                        json!(Uuid::new_v4());
                }
                6 => {
                    body["administrator_creation_approval"]["statement"]
                        ["local_goal_revision"] = json!(2);
                }
                7 => {
                    body["administrator_creation_approval"]["statement"]
                        ["compilation_certificate_id"] = json!(Uuid::new_v4());
                }
                8 => {
                    body["administrator_creation_approval"]["statement"]["availability"] =
                        json!("project_delegable");
                }
                9 => {
                    body["administrator_creation_approval"]["statement"]["scope"] =
                        json!(fixture.info_document_id);
                }
                10 => {
                    body["final_prompt_approval"]["statement"]["draft_id"] =
                        json!(Uuid::new_v4());
                }
                11 => {
                    body["initial_local_goal"]["compilation"]["statement"]["compiler"]
                        ["compiler_build_digest_hex"] = json!("ff".repeat(32));
                }
                12 => {
                    body["initial_local_goal"]["compilation"]["statement"]["authorization"] =
                        json!({"kind": "project_administrator"});
                }
                _ => unreachable!(),
            },
            true,
        )
        .await;
        assert_ne!(status, StatusCode::OK, "mismatch case {case} succeeded");
        let residual = sqlx::query_scalar::<_, i64>(
            "SELECT
                (SELECT count(*) FROM identities WHERE id = $1)
              + (SELECT count(*) FROM governed_agents WHERE id = $2)
              + (SELECT count(*) FROM agent_local_goal_contracts
                   WHERE project_id = $3 AND id = $4)
              + (SELECT count(*) FROM agent_compilation_certificates
                   WHERE project_id = $3 AND id = $5)
              + (SELECT count(*) FROM agent_administrator_creation_approvals
                   WHERE project_id = $3 AND approval_id = $6)",
        )
        .bind(candidate.principal_identity_id)
        .bind(candidate.agent_id)
        .bind(fixture.project_id)
        .bind(candidate.local_goal_id)
        .bind(candidate.compilation_certificate_id)
        .bind(candidate.administrator_approval_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("count failed creation residue");
        assert_eq!(residual, 0, "mismatch case {case} left partial state");
    }
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn governance_dual_signatures_are_both_required_and_bound_to_one_message() {
    let fixture = fixture().await;
    let app = app(&fixture);
    for case in 0_u8..3 {
        let (status, candidate) = provision_administrator_governed_agent(
            &fixture,
            &app,
            150_u8.wrapping_add(case),
            None,
            None,
            |body| match case {
                0 => {
                    body["initial_local_goal"]["compilation"]["signatures"]
                        ["classical_signature"] = json!([]);
                }
                1 => {
                    body["initial_local_goal"]["compilation"]["signatures"]
                        ["post_quantum_signature"] = json!([]);
                }
                2 => {
                    let compilation_signature = body["initial_local_goal"]["compilation"]
                        ["signatures"]["post_quantum_signature"]
                        .clone();
                    body["final_prompt_approval"]["signatures"]["post_quantum_signature"] =
                        compilation_signature;
                }
                _ => unreachable!(),
            },
            false,
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        let residual =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM identities WHERE id = $1")
                .bind(candidate.principal_identity_id)
                .fetch_one(&fixture.pool)
                .await
                .expect("count signature failure residue");
        assert_eq!(residual, 0);
    }
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

async fn install_preexisting_active_local_goal_fixture(
    fixture: &Fixture,
    controller: &SigningMember,
    agent_id: Uuid,
    responsibility_id: Uuid,
    local: &Value,
) {
    let local_goal_id = Uuid::parse_str(local["id"].as_str().expect("fixture LocalGoal id"))
        .expect("fixture LocalGoal UUID");
    let revision = local["revision"].as_u64().expect("fixture revision") as i64;
    let agent_identity_id =
        Uuid::parse_str(local["agent"].as_str().expect("fixture agent identity"))
            .expect("fixture agent UUID");
    let certificate_id = Uuid::new_v4();
    let draft_id = Uuid::new_v4();
    let prompt: sprout_domain::EncryptedPayload =
        serde_json::from_value(local["encrypted_prompt"].clone())
            .expect("fixture encrypted prompt");
    let prompt_bytes = serde_json::to_vec(&prompt).expect("serialize fixture prompt");
    let prompt_hash = Sha256::digest(&prompt_bytes).to_vec();
    let contract_json = local.to_string();
    let contract_hash = Sha256::digest(contract_json.as_bytes()).to_vec();
    let compiler_output = json!({"contract": local["contract"].clone()});
    let compiler_output_hash = Sha256::digest(
        canonical_governance_json(&compiler_output).expect("canonical fixture compiler output"),
    )
    .to_vec();
    let mut transaction = fixture
        .pool
        .begin()
        .await
        .expect("begin existing LocalGoal fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("set migration-owner LocalGoal fixture boundary");
    if revision > 1 {
        sqlx::query(
            "UPDATE agent_local_goal_contracts
             SET state = 'superseded', terminal_at = clock_timestamp()
             WHERE project_id = $1 AND agent_id = $2 AND id = $3
               AND revision = $4 AND state = 'active'",
        )
        .bind(fixture.project_id)
        .bind(agent_id)
        .bind(local_goal_id)
        .bind(revision - 1)
        .execute(&mut *transaction)
        .await
        .expect("supersede fixture LocalGoal predecessor");
        sqlx::query(
            "UPDATE agent_prompt_revisions
             SET state = 'superseded', superseded_at = clock_timestamp()
             WHERE project_id = $1 AND agent_id = $2 AND local_goal_id = $3
               AND local_goal_revision = $4 AND state = 'active'",
        )
        .bind(fixture.project_id)
        .bind(agent_id)
        .bind(local_goal_id)
        .bind(revision - 1)
        .execute(&mut *transaction)
        .await
        .expect("supersede fixture prompt predecessor");
    }
    // This is a migration-owner fixture for an already-certified historical
    // LocalGoal. Compiler/signature acceptance itself remains covered through
    // the dedicated API tests; the cross-owner workflow below remains E2E.
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
            authorization_revision, verification_state, verified_at
        ) VALUES (
            $1, $2, 'local_goal', 'sprout.local-goal.compiler', 1,
            decode('0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2', 'hex'),
            $3, $4, 1, $5, $6, $7, $8, $3,
            decode(repeat('d1', 32), 'hex'), $9, $10::jsonb, $11,
            '{}'::jsonb, decode(repeat('d2', 32), 'hex'),
            decode(repeat('d3', 32), 'hex'), $12,
            decode(repeat('d4', 64), 'hex'), decode('d5', 'hex'), 1,
            decode(repeat('d6', 32), 'hex'), 'responsibility', $13, 1,
            'verified', clock_timestamp()
        )
        "#,
    )
    .bind(certificate_id)
    .bind(fixture.project_id)
    .bind(controller.identity_id)
    .bind(controller.device_id)
    .bind(local_goal_id)
    .bind(revision)
    .bind(draft_id)
    .bind(agent_identity_id)
    .bind(&prompt_hash)
    .bind(compiler_output.to_string())
    .bind(&compiler_output_hash)
    .bind(Uuid::new_v4())
    .bind(responsibility_id)
    .execute(&mut *transaction)
    .await
    .expect("insert preexisting exact LocalGoal compiler witness");
    sqlx::query(
        "INSERT INTO agent_local_goal_contracts (
             id, project_id, agent_id, agent_identity_id, controller_identity_id,
             revision, contract, contract_hash, state, compilation_certificate_id,
             classifier_version, classifier_output_hash
         ) VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8, 'active', $9, 1,
                   decode(repeat('d6', 32), 'hex'))",
    )
    .bind(local_goal_id)
    .bind(fixture.project_id)
    .bind(agent_id)
    .bind(agent_identity_id)
    .bind(controller.identity_id)
    .bind(revision)
    .bind(&contract_json)
    .bind(&contract_hash)
    .bind(certificate_id)
    .execute(&mut *transaction)
    .await
    .expect("insert preexisting active LocalGoal fixture");
    sqlx::query(
        "INSERT INTO agent_prompt_revisions (
             project_id, agent_id, draft_id, local_goal_id, local_goal_revision,
             encrypted_prompt, prompt_hash, state, approved_by_identity_id, activated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'active', $8, clock_timestamp())",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .bind(draft_id)
    .bind(local_goal_id)
    .bind(revision)
    .bind(&prompt_bytes)
    .bind(&prompt_hash)
    .bind(controller.identity_id)
    .execute(&mut *transaction)
    .await
    .expect("insert preexisting active exact prompt fixture");
    sqlx::query(
        r#"
        INSERT INTO agent_prompt_final_approvals (
            project_id, draft_id, agent_id, controller_identity_id,
            local_goal_id, local_goal_revision, prompt_hash, approval_id,
            idempotency_key, agent_principal_identity_id, signer_device_id,
            signer_device_key_version, prompt_input_commitment,
            ciphertext_commitment, compilation_certificate_id,
            structured_output_hash, approval_identity_hash, approval_hash,
            classical_signature, post_quantum_signature, verification_state
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 1,
            decode(repeat('d1', 32), 'hex'), $7, $12, $13,
            decode(repeat('d7', 32), 'hex'), decode(repeat('d8', 32), 'hex'),
            decode(repeat('d9', 64), 'hex'), decode('da', 'hex'), 'verified'
        )
        "#,
    )
    .bind(fixture.project_id)
    .bind(draft_id)
    .bind(agent_id)
    .bind(controller.identity_id)
    .bind(local_goal_id)
    .bind(revision)
    .bind(&prompt_hash)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(agent_identity_id)
    .bind(controller.device_id)
    .bind(certificate_id)
    .bind(&compiler_output_hash)
    .execute(&mut *transaction)
    .await
    .expect("insert preexisting exact final approval fixture");
    sqlx::query(
        "UPDATE governed_agents SET encrypted_system_prompt = $3
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .bind(&prompt_bytes)
    .execute(&mut *transaction)
    .await
    .expect("bind agent to exact fixture prompt");
    if local["contract"]["work_specs"][0]["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| action == "assign_own_task"))
    {
        let task_scope = Uuid::parse_str(
            local["clauses"][0]["scope"]
                .as_str()
                .expect("fixture task clause scope"),
        )
        .expect("fixture task scope UUID");
        let obligation_id = Uuid::parse_str(
            local["contract"]["obligations"][0]["id"]
                .as_str()
                .expect("fixture obligation id"),
        )
        .expect("fixture obligation UUID");
        let work_spec_id = local["contract"]["work_specs"][0]["id"]
            .as_u64()
            .expect("fixture WorkSpec id") as i64;
        sqlx::query(
            "INSERT INTO agent_task_obligation_provenance (
                 project_id, task_intent_id, task_resource_node_id,
                 target_agent_id, local_goal_id, local_goal_revision,
                 obligation_id, work_spec_ordinal
             )
             SELECT assignment.project_id, assignment.task_intent_id,
                    assignment.task_resource_node_id, assignment.target_agent_id,
                    $3, $4, $5, $6
             FROM agent_cross_owner_assignments assignment
             WHERE assignment.project_id = $1 AND assignment.target_agent_id = $2
               AND assignment.task_resource_node_id = $7
               AND assignment.route = 'controller_review'
               AND assignment.decision = 'approved'
               AND assignment.state = 'approved_pending_mandate'
             ON CONFLICT (project_id, task_intent_id) DO NOTHING",
        )
        .bind(fixture.project_id)
        .bind(agent_id)
        .bind(local_goal_id)
        .bind(revision)
        .bind(obligation_id)
        .bind(work_spec_id)
        .bind(task_scope)
        .execute(&mut *transaction)
        .await
        .expect("materialize exact preexisting task-obligation provenance");
    }
    transaction
        .commit()
        .await
        .expect("commit existing LocalGoal fixture");
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
    let (status, provision) =
        provision_administrator_governed_agent(&fixture, &app, 1, None, None, |_| {}, false).await;
    assert_eq!(status, StatusCode::OK);
    let agent_id = provision.agent_id;
    let agent_identity_id = provision.principal_identity_id;
    let _runner_id = provision.runner_id;
    let runner_device_id = provision.runner_device_id;
    let local_goal_id = provision.local_goal_id;
    let runner_token = provision
        .bootstrap_token
        .expect("exact creation bootstrap token");

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

    // The exact administrator-creation API above already atomically activated
    // the certified initial LocalGoal used by this runtime test. No synthetic
    // administrator→administrator Responsibility is introduced.
    let local_goal_contract = sqlx::query_scalar::<_, String>(
        "SELECT contract::text FROM agent_local_goal_contracts
         WHERE project_id = $1 AND agent_id = $2 AND id = $3
           AND revision = 1 AND state = 'active'",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .bind(local_goal_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact initial LocalGoal fixture");
    let local_goal_contract: Value =
        serde_json::from_str(&local_goal_contract).expect("decode exact initial LocalGoal");
    let goal_contract = local_goal_contract["contract"].clone();
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
    let (foreign_status, foreign_provision) =
        provision_administrator_governed_agent(&fixture, &app, 31, None, None, |_| {}, false).await;
    assert_eq!(foreign_status, StatusCode::OK);
    let foreign_agent_id = foreign_provision.agent_id;
    let foreign_identity_id = foreign_provision.principal_identity_id;
    let foreign_device_id = foreign_provision.runner_device_id;
    let foreign_token = foreign_provision
        .bootstrap_token
        .expect("foreign exact-creation bootstrap token");
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
    let administrator_creation_global_id = Uuid::new_v4();
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
                        "id": administrator_creation_global_id,
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
    assert_eq!(global_contract.status(), StatusCode::BAD_REQUEST);

    // Global synthesis remains a supported surface. Its positive source uses
    // the pre-existing normative Responsibility provenance rather than the
    // administrator-creation origin, which is deliberately not bottom-up.
    let source_controller = add_signing_human_member(&fixture, "member").await;
    let (source_responsibility_status, source_responsibility_id) =
        create_active_compiled_responsibility(&fixture, &app, source_controller.identity_id, 91)
            .await;
    assert_eq!(source_responsibility_status, StatusCode::OK);
    let (source_creation_status, source_agent) = provision_administrator_governed_agent(
        &fixture,
        &app,
        92,
        Some(&source_controller),
        Some(source_responsibility_id),
        |_| {},
        false,
    )
    .await;
    assert_eq!(source_creation_status, StatusCode::OK);
    let source_local_goal_contract = sqlx::query_scalar::<_, String>(
        "SELECT contract::text FROM agent_local_goal_contracts
         WHERE project_id = $1 AND agent_id = $2 AND id = $3
           AND revision = 1 AND state = 'active'",
    )
    .bind(fixture.project_id)
    .bind(source_agent.agent_id)
    .bind(source_agent.local_goal_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load Responsibility-authorized global source LocalGoal");
    let source_local_goal_contract: Value = serde_json::from_str(&source_local_goal_contract)
        .expect("decode Responsibility-authorized global source LocalGoal");
    let source_goal_contract = source_local_goal_contract["contract"].clone();
    let source_agent_identity_id = source_agent.principal_identity_id;
    let global_contract_id = Uuid::new_v4();
    let administrator_global = app
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
                                "allowed_principal_ids": [source_agent_identity_id],
                                "allowed_tools": []
                            },
                            "source_agents": [source_agent_identity_id],
                            "max_global_obligations": 1,
                            "max_global_work_specs": 1,
                            "max_dependencies": 0,
                            "max_conflicts": 0
                        },
                        "candidate": {
                            "revision": 1,
                            "contract": source_goal_contract,
                            "contributions": [{
                                "agent": source_agent_identity_id,
                                "local_revision": 1,
                                "local_clause_id": 1,
                                "global_work_spec_ids": [1]
                            }],
                            "governance_conflicts": []
                        },
                        "groundings": [{
                            "global_work_spec_id": 1,
                            "source_agent": source_agent_identity_id,
                            "source_local_revision": 1,
                            "source_work_spec_id": 1
                        }]
                    })
                    .to_string(),
                ))
                .expect("record Responsibility-grounded global contract"),
        )
        .await
        .expect("record Responsibility-grounded global response");
    assert_eq!(
        administrator_global.status(),
        StatusCode::OK,
        "{}",
        json_body(administrator_global).await
    );

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
        "allowed_principal_ids": [source_agent_identity_id],
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
                                "principal_id": source_agent_identity_id,
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
                            "source_agents": [source_agent_identity_id],
                            "max_global_obligations": 1,
                            "max_global_work_specs": 1,
                            "max_dependencies": 0,
                            "max_conflicts": 0
                        },
                        "candidate": {
                            "revision": 2,
                            "contract": source_goal_contract,
                            "contributions": [{
                                "agent": source_agent_identity_id,
                                "local_revision": 1,
                                "local_clause_id": 1,
                                "global_work_spec_ids": [1]
                            }],
                            "governance_conflicts": []
                        },
                        "groundings": [{
                            "global_work_spec_id": 1,
                            "source_agent": source_agent_identity_id,
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
    let rejected_global_records = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_global_contracts
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(administrator_creation_global_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count rejected administrator-creation bottom-up global records");
    assert_eq!(rejected_global_records, 0);
    let supported_global_revisions = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_global_contracts
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(global_contract_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count Responsibility-grounded global revisions");
    assert_eq!(supported_global_revisions, 2);

    let proxy_controller = add_signing_human_member(&fixture, "member").await;
    let proxy_resource_id = Uuid::new_v4();
    let proxy_topic_id = Uuid::new_v4();
    let root_resource_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT parent_id FROM resource_nodes
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load project root for independent UserProxy resource");
    let mut proxy_resource = fixture
        .pool
        .begin()
        .await
        .expect("begin independent UserProxy resource fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *proxy_resource)
        .await
        .expect("set migration-owner UserProxy resource fixture boundary");
    sqlx::query(
        "INSERT INTO resource_nodes (
             id, project_id, parent_id, node_kind,
             encrypted_metadata, created_by_identity_id
         ) VALUES ($1, $2, $3, 'topic', decode('01', 'hex'), $4)",
    )
    .bind(proxy_resource_id)
    .bind(fixture.project_id)
    .bind(root_resource_id)
    .bind(fixture.owner_id)
    .execute(&mut *proxy_resource)
    .await
    .expect("insert independent UserProxy resource");
    sqlx::query(
        "INSERT INTO resource_epochs (
             project_id, resource_node_id, epoch,
             created_by_identity_id, created_by_device_id,
             created_by_device_key_version, key_commitment, reason
         ) VALUES ($1, $2, 1, $3, $4, 1,
                   decode(repeat('ab', 32), 'hex'), 'created')",
    )
    .bind(fixture.project_id)
    .bind(proxy_resource_id)
    .bind(fixture.owner_id)
    .bind(fixture.owner_device_id)
    .execute(&mut *proxy_resource)
    .await
    .expect("insert independent UserProxy resource epoch");
    sqlx::query(
        "INSERT INTO topics (id, project_id, resource_node_id, encrypted_payload)
         VALUES ($1, $2, $3, decode('01', 'hex'))",
    )
    .bind(proxy_topic_id)
    .bind(fixture.project_id)
    .bind(proxy_resource_id)
    .execute(&mut *proxy_resource)
    .await
    .expect("materialize independent UserProxy topic");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'manage', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(proxy_resource_id)
    .bind(proxy_controller.identity_id)
    .bind(Uuid::new_v4())
    .bind(fixture.owner_id)
    .execute(&mut *proxy_resource)
    .await
    .expect("grant proxy user permission on independent resource");
    proxy_resource
        .commit()
        .await
        .expect("commit independent UserProxy resource fixture");
    let (proxy_responsibility_status, _) = create_active_compiled_responsibility_for_scope(
        &fixture,
        &app,
        proxy_controller.identity_id,
        proxy_resource_id,
        93,
    )
    .await;
    assert_eq!(proxy_responsibility_status, StatusCode::OK);

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
                .header(
                    "authorization",
                    format!("Bearer {}", proxy_controller.bearer),
                )
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
                .header(
                    "authorization",
                    format!("Bearer {}", proxy_controller.bearer),
                )
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
                "allowed_resource_ids": [proxy_resource_id],
                "allowed_principal_ids": [proxy_controller.identity_id],
                "allowed_tools": []
            },
            "request_id": proxy_request_id,
            "user": proxy_controller.identity_id,
            "candidate_resources": [proxy_resource_id],
            "candidate_operations": ["post_comment"],
            "available_tools": [],
            "max_plan_steps": 1
        },
        "plan": {
            "request_id": proxy_request_id,
            "thread_id": proxy_thread_id,
            "user": proxy_controller.identity_id,
            "intent_id": Uuid::new_v4(),
            "resource_effects": [{
                "resource_id": proxy_resource_id,
                "operation": "post_comment"
            }],
            "tool_invocations": [],
            "encrypted_explanation": encrypted(9)
        },
        "confirmation": null
    });
    let mut forged_proxy_plan = proxy_plan_payload.clone();
    forged_proxy_plan["action_classification"] = json!([{
        "resource_id": proxy_resource_id,
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
                .header(
                    "authorization",
                    format!("Bearer {}", proxy_controller.bearer),
                )
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
                .header(
                    "authorization",
                    format!("Bearer {}", proxy_controller.bearer),
                )
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

    let audit_lifecycle_complete = sqlx::query_scalar::<_, bool>(
        "SELECT count(*) FILTER (WHERE event_kind = 'agent_provisioned') = 1
             AND count(*) FILTER (WHERE event_kind = 'local_goal_recorded') = 1
             AND count(*) FILTER (WHERE event_kind = 'runner_activated') = 1
             AND count(*) FILTER (WHERE event_kind = 'interrogation_recorded') = 1
             AND count(*) FILTER (WHERE event_kind = 'invocation_queued') >= 1
             AND count(*) FILTER (WHERE event_kind = 'invocation_leased') >= 1
             AND count(*) FILTER (WHERE event_kind = 'invocation_succeeded') >= 1
             AND EXISTS (
                 SELECT 1 FROM agent_user_governance_audit_log governance
                 WHERE governance.project_id = $1 AND governance.agent_id = $2
                   AND governance.event_kind = 'local_goal_activated'
             )
         FROM agent_audit_log WHERE project_id = $1 AND agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify required agent audit lifecycle records");
    assert!(audit_lifecycle_complete);

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
    let remaining_agent_owned_records = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT sum(row_count)::bigint FROM (
            SELECT count(*) AS row_count FROM governed_agents WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_runners WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_local_goal_contracts WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_invocations WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_invocation_sources WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_effect_proposals WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_audit_log WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_global_contracts WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_global_contract_sources WHERE project_id = $1
            UNION ALL SELECT count(*) FROM agent_interrogations WHERE project_id = $1
        ) records
        "#,
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count records after agent retention purge");
    assert_eq!(remaining_agent_owned_records, 0);
    let user_proxy_records = sqlx::query_scalar::<_, i64>(
        "SELECT (SELECT count(*) FROM user_proxies WHERE project_id = $1)
              + (SELECT count(*) FROM user_proxy_threads WHERE project_id = $1)
              + (SELECT count(*) FROM user_proxy_requests WHERE project_id = $1)
              + (SELECT count(*) FROM user_proxy_plans WHERE project_id = $1)",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count user-owned proxy records after agent purge");
    // Agent retention removes agent-owned runtime state but not the human's
    // separate private UserProxy history.
    assert_eq!(
        user_proxy_records, 4,
        "proxy identity, thread, request and accepted plan must all survive"
    );
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
async fn stale_responsibility_activation_rolls_back_state_and_audit() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let controller = add_signing_human_member(&fixture, "member").await;
    let (status, responsibility_id) =
        create_active_compiled_responsibility(&fixture, &app, controller.identity_id, 133).await;
    assert_eq!(status, StatusCode::OK);
    let recorded = record_compiled_responsibility_revision(
        &fixture,
        &app,
        controller.identity_id,
        responsibility_id,
        2,
        1,
        134,
    )
    .await;
    assert_eq!(recorded, StatusCode::OK);
    let activation_uri = format!(
        "/v1/projects/{}/users/{}/responsibilities/{responsibility_id}/revisions/2/activate",
        fixture.project_id, controller.identity_id
    );
    let activate = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&activation_uri)
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("activate exact responsibility revision"),
        )
        .await
        .expect("responsibility activation response");
    assert_eq!(activate.status(), StatusCode::OK);
    let audit_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2
           AND event_kind = 'responsibility_activated'",
    )
    .bind(fixture.project_id)
    .bind(controller.identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count responsibility activation audit");
    let state_before = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_agg(jsonb_build_object('revision', revision, 'state', state)
                          ORDER BY revision)
         FROM agent_responsibility_contracts
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(responsibility_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("snapshot responsibility revision states");

    let stale = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&activation_uri)
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("stale responsibility activation retry"),
        )
        .await
        .expect("stale responsibility activation response");
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let audit_after = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2
           AND event_kind = 'responsibility_activated'",
    )
    .bind(fixture.project_id)
    .bind(controller.identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count audit after stale activation");
    let state_after = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_agg(jsonb_build_object('revision', revision, 'state', state)
                          ORDER BY revision)
         FROM agent_responsibility_contracts
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(responsibility_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("snapshot state after stale activation");
    assert_eq!(audit_after, audit_before);
    assert_eq!(state_after, state_before);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn governance_verified_history_rejects_app_role_dml_and_shadowing() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let (status, created) =
        provision_administrator_governed_agent(&fixture, &app, 135, None, None, |_| {}, false)
            .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query(
        r#"
        DO $role$
        BEGIN
            IF NOT EXISTS (
                SELECT 1 FROM pg_roles WHERE rolname = 'sprout_0029_untrusted_app'
            ) THEN
                CREATE ROLE sprout_0029_untrusted_app NOSUPERUSER NOBYPASSRLS NOLOGIN;
            END IF;
        END
        $role$
        "#,
    )
    .execute(&fixture.pool)
    .await
    .expect("create untrusted 0029 app role");
    for grant in [
        "GRANT sprout_0029_untrusted_app TO sprout_test",
        "GRANT USAGE ON SCHEMA public, sprout_private TO sprout_0029_untrusted_app",
        "GRANT SELECT ON agent_compilation_certificates, agent_prompt_final_approvals, agent_administrator_creation_approvals, agent_governance_ledger TO sprout_0029_untrusted_app",
    ] {
        sqlx::query(grant)
            .execute(&fixture.pool)
            .await
            .expect("grant read-only governance test boundary");
    }
    let definer_boundary = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT bool_and(
            procedure.prosecdef
            AND procedure.proowner = (SELECT oid FROM pg_roles WHERE rolname = current_user)
            AND procedure.proconfig @> ARRAY['search_path=public, pg_temp']::text[]
            AND procedure.proconfig @> ARRAY['row_security=off']::text[]
            AND NOT has_function_privilege(
                'sprout_0029_untrusted_app', procedure.oid, 'EXECUTE'
            )
        )
        FROM pg_proc procedure
        JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
        WHERE namespace.nspname = 'sprout_private'
          AND procedure.proname IN (
              'insert_verified_compilation_certificate',
              'insert_verified_administrator_creation_approval',
              'insert_verified_final_prompt_approval',
              'append_verified_governance_revision'
          )
        "#,
    )
    .fetch_one(&fixture.pool)
    .await
    .expect("verify SECURITY DEFINER ownership and fixed search path");
    assert!(definer_boundary);
    let counts_before = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_array(
             (SELECT count(*) FROM agent_compilation_certificates WHERE project_id = $1),
             (SELECT count(*) FROM agent_prompt_final_approvals WHERE project_id = $1),
             (SELECT count(*) FROM agent_administrator_creation_approvals WHERE project_id = $1),
             (SELECT count(*) FROM agent_governance_ledger WHERE project_id = $1))",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("snapshot verified governance history");
    let mut untrusted = fixture
        .pool
        .begin()
        .await
        .expect("begin untrusted DML gate");
    sqlx::query("SET LOCAL ROLE sprout_0029_untrusted_app")
        .execute(&mut *untrusted)
        .await
        .expect("assume untrusted app role");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(fixture.owner_id.to_string())
    .bind(fixture.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *untrusted)
    .await
    .expect("set forged trusted identity GUCs");
    sqlx::query(
        "CREATE TEMP TABLE agent_compiler_builds (
             task_kind text, compiler_name text, compiler_version integer,
             build_digest bytea, enabled boolean)",
    )
    .execute(&mut *untrusted)
    .await
    .expect("create temp compiler registry shadow");
    sqlx::query(
        r#"
        DO $negative$
        BEGIN
            BEGIN
                INSERT INTO agent_compilation_certificates (
                    id, project_id, task_kind, compiler_name, compiler_version,
                    compiler_build_digest, signer_identity_id, signer_device_id,
                    signer_device_key_version, subject_id, subject_revision, draft_id,
                    input_commitment, ciphertext_commitment, canonical_output,
                    output_hash, compilation_envelope, envelope_hash, certificate_hash,
                    idempotency_key, classical_signature, post_quantum_signature,
                    authorization_kind, verification_state, verified_at
                )
                SELECT gen_random_uuid(), project_id, task_kind, compiler_name,
                       compiler_version, compiler_build_digest, signer_identity_id,
                       signer_device_id, signer_device_key_version, gen_random_uuid(),
                       99, gen_random_uuid(), input_commitment, ciphertext_commitment,
                       canonical_output, output_hash, compilation_envelope, envelope_hash,
                       certificate_hash, gen_random_uuid(), classical_signature,
                       post_quantum_signature, authorization_kind, 'verified', clock_timestamp()
                FROM public.agent_compilation_certificates
                WHERE project_id = current_setting('app.project_id')::uuid
                LIMIT 1;
                RAISE EXCEPTION 'untrusted app inserted a verified artifact';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                UPDATE public.agent_compilation_certificates
                SET certificate_hash = decode(repeat('00', 32), 'hex')
                WHERE project_id = current_setting('app.project_id')::uuid;
                RAISE EXCEPTION 'untrusted app updated verified history';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                DELETE FROM public.agent_governance_ledger
                WHERE project_id = current_setting('app.project_id')::uuid;
                RAISE EXCEPTION 'untrusted app deleted verified history';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                PERFORM sprout_private.append_verified_governance_revision(
                    NULL, NULL, NULL, NULL, NULL, NULL
                );
                RAISE EXCEPTION 'untrusted app executed private governance writer';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
        END
        $negative$
        "#,
    )
    .execute(&mut *untrusted)
    .await
    .expect("verified history and private writer must reject untrusted app role");
    untrusted
        .commit()
        .await
        .expect("commit negative trust-boundary gate");
    let counts_after = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_array(
             (SELECT count(*) FROM agent_compilation_certificates WHERE project_id = $1),
             (SELECT count(*) FROM agent_prompt_final_approvals WHERE project_id = $1),
             (SELECT count(*) FROM agent_administrator_creation_approvals WHERE project_id = $1),
             (SELECT count(*) FROM agent_governance_ledger WHERE project_id = $1))",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("snapshot verified governance history after attacks");
    assert_eq!(counts_after, counts_before);
    assert_ne!(created.compilation_certificate_id, Uuid::nil());
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn exact_reapproval_for_same_proposed_principal_has_no_schema_liveness_lock() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let (status, created) =
        provision_administrator_governed_agent(&fixture, &app, 136, None, None, |_| {}, false)
            .await;
    assert_eq!(status, StatusCode::OK);
    let second_certificate_id = Uuid::new_v4();
    let second_approval_id = Uuid::new_v4();
    let second_draft_id = Uuid::new_v4();
    let second_local_goal_id = Uuid::new_v4();
    let second_governed_agent_id = Uuid::new_v4();
    let second_idempotency_key = Uuid::new_v4();
    let mut tcb_fixture = fixture
        .pool
        .begin()
        .await
        .expect("begin exact reapproval fixture");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(fixture.owner_id.to_string())
    .bind(fixture.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *tcb_fixture)
    .await
    .expect("set migration-owner TCB fixture context");
    // This fixture exercises the relational liveness contract only. The API
    // tests above remain the proof that real approvals pass Rust signature
    // verification before reaching these private writers.
    sqlx::query(
        r#"
        SELECT sprout_private.insert_verified_compilation_certificate(
            $2, certificate.project_id, certificate.task_kind,
            certificate.compiler_name, certificate.compiler_version,
            certificate.compiler_build_digest, certificate.signer_identity_id,
            certificate.signer_device_id, certificate.signer_device_key_version,
            $3, 1, $4, certificate.agent_principal_identity_id,
            certificate.controller_identity_id, certificate.administrator_identity_id,
            certificate.user_identity_id, certificate.input_commitment,
            certificate.ciphertext_commitment, certificate.canonical_output,
            certificate.output_hash, certificate.compilation_envelope,
            certificate.envelope_hash, decode(repeat('b1', 32), 'hex'),
            $5, certificate.classical_signature, certificate.post_quantum_signature,
            certificate.classifier_version, certificate.classifier_output_hash,
            'administrator_creation', $6, NULL
        )
        FROM agent_compilation_certificates certificate
        WHERE certificate.project_id = $1 AND certificate.id = $7
        "#,
    )
    .bind(fixture.project_id)
    .bind(second_certificate_id)
    .bind(second_local_goal_id)
    .bind(second_draft_id)
    .bind(second_idempotency_key)
    .bind(second_approval_id)
    .bind(created.compilation_certificate_id)
    .execute(&mut *tcb_fixture)
    .await
    .expect("persist second exact proposal compilation fixture");
    sqlx::query(
        r#"
        SELECT sprout_private.insert_verified_administrator_creation_approval(
            certificate.project_id, $2, certificate.controller_identity_id,
            certificate.signer_device_id, certificate.signer_device_key_version,
            certificate.agent_principal_identity_id, $3, certificate.draft_id,
            certificate.subject_id, certificate.subject_revision,
            decode(repeat('b2', 32), 'hex'), certificate.id,
            certificate.input_commitment, certificate.ciphertext_commitment,
            'controller_private', $4, decode(repeat('b3', 32), 'hex'),
            $5, decode(repeat('b4', 32), 'hex'),
            certificate.classical_signature, certificate.post_quantum_signature
        )
        FROM agent_compilation_certificates certificate
        WHERE certificate.project_id = $1 AND certificate.id = $6
        "#,
    )
    .bind(fixture.project_id)
    .bind(second_approval_id)
    .bind(second_governed_agent_id)
    .bind(fixture.profile_resource_id)
    .bind(Uuid::new_v4())
    .bind(second_certificate_id)
    .execute(&mut *tcb_fixture)
    .await
    .expect("record a second exact approval for the same proposed principal");
    tcb_fixture
        .commit()
        .await
        .expect("commit exact reapproval fixture");
    let approvals = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_administrator_creation_approvals
         WHERE project_id = $1 AND proposed_agent_identity_id = $2",
    )
    .bind(fixture.project_id)
    .bind(created.principal_identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exact proposals for the same principal");
    assert_eq!(approvals, 2);
    let second_agent_materialized = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM governed_agents
         WHERE project_id = $1 AND id = $2)",
    )
    .bind(fixture.project_id)
    .bind(second_governed_agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("check reapproval is not activation");
    assert!(!second_agent_materialized);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn governance_ledger_concurrent_append_and_exact_replay_are_deterministic() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let (status, created) =
        provision_administrator_governed_agent(&fixture, &app, 137, None, None, |_| {}, false)
            .await;
    assert_eq!(status, StatusCode::OK);
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let first_subject = Uuid::new_v4();
    let second_subject = Uuid::new_v4();
    let first_draft = Uuid::new_v4();
    let second_draft = Uuid::new_v4();
    let first_idempotency = Uuid::new_v4();
    let second_idempotency = Uuid::new_v4();
    let insert_clone = r#"
        SELECT sprout_private.insert_verified_compilation_certificate(
            $2, certificate.project_id, certificate.task_kind,
            certificate.compiler_name, certificate.compiler_version,
            certificate.compiler_build_digest, certificate.signer_identity_id,
            certificate.signer_device_id, certificate.signer_device_key_version,
            $3, 1, $4, certificate.agent_principal_identity_id,
            certificate.controller_identity_id, certificate.administrator_identity_id,
            certificate.user_identity_id, certificate.input_commitment,
            certificate.ciphertext_commitment, certificate.canonical_output,
            certificate.output_hash, certificate.compilation_envelope,
            certificate.envelope_hash, $5, $6, certificate.classical_signature,
            certificate.post_quantum_signature, certificate.classifier_version,
            certificate.classifier_output_hash, certificate.authorization_kind,
            certificate.authorization_id, certificate.authorization_revision
        )
        FROM agent_compilation_certificates certificate
        WHERE certificate.project_id = $1 AND certificate.id = $7
    "#;
    let mut first = fixture
        .pool
        .begin()
        .await
        .expect("open first governance append");
    let mut second = fixture
        .pool
        .begin()
        .await
        .expect("open second governance append");
    let first_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *first)
        .await
        .expect("load first governance writer PID");
    let second_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()")
        .fetch_one(&mut *second)
        .await
        .expect("load second governance writer PID");
    assert_ne!(
        first_pid, second_pid,
        "both governance writers must be open"
    );
    for transaction in [&mut first, &mut second] {
        sqlx::query(
            "SELECT set_config('app.identity_id', $1, true),
                    set_config('app.device_id', $2, true),
                    set_config('app.project_id', $3, true)",
        )
        .bind(fixture.owner_id.to_string())
        .bind(fixture.owner_device_id.to_string())
        .bind(fixture.project_id.to_string())
        .execute(&mut **transaction)
        .await
        .expect("set concurrent writer context");
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 40))")
        .bind(fixture.project_id)
        .execute(&mut *first)
        .await
        .expect("hold first governance append lock");
    let first_holds_project_lock = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_locks
         WHERE pid = $1 AND locktype = 'advisory' AND granted)",
    )
    .bind(first_pid)
    .fetch_one(&fixture.pool)
    .await
    .expect("observe first governance advisory lock");
    assert!(first_holds_project_lock);
    let project_id = fixture.project_id;
    let source_certificate = created.compilation_certificate_id;
    let second_pool = fixture.pool.clone();
    let (attempting_tx, attempting_rx) = oneshot::channel();
    let second_task = tokio::spawn(async move {
        attempting_tx
            .send(())
            .expect("signal second governance append attempt");
        sqlx::query(insert_clone)
            .bind(project_id)
            .bind(second_id)
            .bind(second_subject)
            .bind(second_draft)
            .bind(vec![0xc2_u8; 32])
            .bind(second_idempotency)
            .bind(source_certificate)
            .execute(&mut *second)
            .await
            .expect("second overlapping governance append");
        second
            .commit()
            .await
            .expect("commit second governance append");
    });
    attempting_rx
        .await
        .expect("second governance writer reached the append call");
    wait_for_backend_lock(&second_pool, second_pid).await;
    let exact_advisory_contention = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM pg_locks held
             JOIN pg_locks waiting
               ON waiting.locktype = held.locktype
              AND waiting.database IS NOT DISTINCT FROM held.database
              AND waiting.classid IS NOT DISTINCT FROM held.classid
              AND waiting.objid IS NOT DISTINCT FROM held.objid
              AND waiting.objsubid IS NOT DISTINCT FROM held.objsubid
             WHERE held.pid = $1 AND held.locktype = 'advisory' AND held.granted
               AND waiting.pid = $2 AND NOT waiting.granted
         )",
    )
    .bind(first_pid)
    .bind(second_pid)
    .fetch_one(&second_pool)
    .await
    .expect("observe exact governance advisory-lock contention");
    assert!(exact_advisory_contention);
    assert!(
        !second_task.is_finished(),
        "second writer completed while the first still held the ledger lock"
    );
    sqlx::query(insert_clone)
        .bind(fixture.project_id)
        .bind(first_id)
        .bind(first_subject)
        .bind(first_draft)
        .bind(vec![0xc1_u8; 32])
        .bind(first_idempotency)
        .bind(created.compilation_certificate_id)
        .execute(&mut *first)
        .await
        .expect("first governance append while holding lock");
    first
        .commit()
        .await
        .expect("commit first governance append");
    second_task.await.expect("join second governance append");
    let positions = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT entry_id, position FROM agent_governance_ledger
         WHERE project_id = $1 AND entry_kind = 'compilation'
           AND entry_id = ANY($2::uuid[])
         ORDER BY position",
    )
    .bind(fixture.project_id)
    .bind(vec![first_id, second_id])
    .fetch_all(&fixture.pool)
    .await
    .expect("load deterministic concurrent ledger order");
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].0, first_id);
    assert_eq!(positions[1].0, second_id);
    assert!(positions[0].1 < positions[1].1);

    let ledger_count_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_governance_ledger
         WHERE project_id = $1 AND entry_kind = 'compilation'
           AND entry_id = $2",
    )
    .bind(fixture.project_id)
    .bind(second_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count ledger entry before replay");
    let mut replay = fixture.pool.begin().await.expect("begin exact replay");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)",
    )
    .bind(fixture.owner_id.to_string())
    .bind(fixture.owner_device_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *replay)
    .await
    .unwrap();
    sqlx::query(insert_clone)
        .bind(fixture.project_id)
        .bind(second_id)
        .bind(second_subject)
        .bind(second_draft)
        .bind(vec![0xc2_u8; 32])
        .bind(second_idempotency)
        .bind(created.compilation_certificate_id)
        .execute(&mut *replay)
        .await
        .expect("exact artifact replay is idempotent");
    replay.commit().await.expect("commit exact replay");
    let ledger_count_after = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_governance_ledger
         WHERE project_id = $1 AND entry_kind = 'compilation'
           AND entry_id = $2",
    )
    .bind(fixture.project_id)
    .bind(second_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count ledger entry after replay");
    assert_eq!(ledger_count_after, ledger_count_before);

    let mut equivocation = fixture.pool.begin().await.expect("begin replay conflict");
    sqlx::query("SELECT set_config('app.identity_id', $1, true)")
        .bind(fixture.owner_id.to_string())
        .execute(&mut *equivocation)
        .await
        .unwrap();
    let conflict = sqlx::query(insert_clone)
        .bind(fixture.project_id)
        .bind(second_id)
        .bind(second_subject)
        .bind(second_draft)
        .bind(vec![0xee_u8; 32])
        .bind(second_idempotency)
        .bind(created.compilation_certificate_id)
        .execute(&mut *equivocation)
        .await
        .expect_err("same artifact identity with a different hash must conflict");
    assert_eq!(
        conflict.as_database_error().and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("23505"))
    );
    equivocation
        .rollback()
        .await
        .expect("rollback equivocation attempt");
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn normal_controller_can_atomically_activate_exact_local_goal_and_stale_retry_rolls_back() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let controller = add_signing_human_member(&fixture, "member").await;
    let (responsibility_status, responsibility_id) =
        create_active_compiled_responsibility(&fixture, &app, controller.identity_id, 130).await;
    assert_eq!(responsibility_status, StatusCode::OK);
    let (creation_status, created) = provision_administrator_governed_agent(
        &fixture,
        &app,
        131,
        Some(&controller),
        Some(responsibility_id),
        |_| {},
        false,
    )
    .await;
    assert_eq!(creation_status, StatusCode::OK);

    let (draft_status, activation_status, activation_body) =
        activate_certified_local_goal_revision(
            &fixture,
            &app,
            &controller,
            created.agent_id,
            created.local_goal_id,
            responsibility_id,
            132,
        )
        .await;
    assert_eq!(draft_status, StatusCode::OK);
    assert_eq!(activation_status, StatusCode::OK);

    let exact_activation = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT count(*) FILTER (WHERE local.revision = 1 AND local.state = 'superseded') = 1
           AND count(*) FILTER (
                 WHERE local.revision = 2 AND local.state = 'active'
                   AND prompt.state = 'active'
                   AND prompt.local_goal_revision = local.revision
                   AND agent.encrypted_system_prompt = prompt.encrypted_prompt
                   AND approval.verification_state = 'verified'
                   AND approval.compilation_certificate_id = local.compilation_certificate_id
                   AND approval.structured_output_hash = certificate.output_hash
               ) = 1
        FROM agent_local_goal_contracts local
        LEFT JOIN agent_prompt_revisions prompt
          ON prompt.project_id = local.project_id
         AND prompt.agent_id = local.agent_id
         AND prompt.local_goal_id = local.id
         AND prompt.local_goal_revision = local.revision
        LEFT JOIN agent_prompt_final_approvals approval
          ON approval.project_id = prompt.project_id
         AND approval.draft_id = prompt.draft_id
        LEFT JOIN agent_compilation_certificates certificate
          ON certificate.project_id = local.project_id
         AND certificate.id = local.compilation_certificate_id
        JOIN governed_agents agent
          ON agent.project_id = local.project_id AND agent.id = local.agent_id
        WHERE local.project_id = $1 AND local.agent_id = $2 AND local.id = $3
        "#,
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .bind(created.local_goal_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify exact revision activation projection");
    assert!(exact_activation);

    let audit_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2
           AND event_kind = 'local_goal_activated'",
    )
    .bind(fixture.project_id)
    .bind(controller.identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count revision activation audit");
    let approvals_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_prompt_final_approvals
         WHERE project_id = $1 AND agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exact final approvals");

    let stale = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/local-goals/{}/revisions/2/activate",
                    fixture.project_id, created.agent_id, created.local_goal_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(activation_body.to_string()))
                .expect("stale exact activation retry"),
        )
        .await
        .expect("stale activation response");
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let audit_after = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_user_governance_audit_log
         WHERE project_id = $1 AND subject_user_identity_id = $2
           AND event_kind = 'local_goal_activated'",
    )
    .bind(fixture.project_id)
    .bind(controller.identity_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count audit after stale retry");
    let approvals_after = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_prompt_final_approvals
         WHERE project_id = $1 AND agent_id = $2",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count approvals after stale retry");
    assert_eq!(audit_after, audit_before);
    assert_eq!(approvals_after, approvals_before);
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

    let (responsibility_status, responsibility_id) =
        create_active_compiled_responsibility_for_scope(
            &fixture,
            &app,
            controller_id,
            root_resource_id,
            67,
        )
        .await;
    assert_eq!(responsibility_status, StatusCode::OK);

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
    let controller = add_signing_human_member(&fixture, "member").await;
    let controller_id = controller.identity_id;
    let controller_device = controller.device_id;
    let controller_token = controller.bearer.clone();
    let (source_task, review_task) =
        create_cross_owner_tasks(&fixture, requester_id, controller_id).await;
    let (agent_id, agent_identity_id, _runner_id, _runner_device_id) = provision_controlled_agent(
        &fixture,
        &app,
        controller_id,
        fixture.profile_resource_id,
        50,
    )
    .await;

    let (responsibility_status, responsibility_id) =
        create_active_compiled_responsibility_for_scope_and_action(
            &fixture,
            &app,
            controller_id,
            source_task,
            "assign_own_task",
            53,
        )
        .await;
    assert_eq!(responsibility_status, StatusCode::OK);

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
    install_preexisting_active_local_goal_fixture(
        &fixture,
        &controller,
        agent_id,
        responsibility_id,
        &wrong_task_local,
    )
    .await;
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
    install_preexisting_active_local_goal_fixture(
        &fixture,
        &controller,
        agent_id,
        responsibility_id,
        &exact_task_local,
    )
    .await;

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
    let (semantic_intent_list_before_purge, semantic_provenance_list_before_purge) =
        semantic_operational_lists(
            &fixture.pool,
            fixture.project_id,
            controller_id,
            controller_device,
        )
        .await;
    assert_eq!(semantic_intent_list_before_purge.len(), 2);
    assert_eq!(semantic_provenance_list_before_purge.len(), 1);
    assert_eq!(
        semantic_provenance_list_before_purge[0]["agent_identity_id"],
        agent_identity_id.to_string()
    );
    assert_ne!(agent_identity_id, agent_id);
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
    let concurrent_intent_id = Uuid::new_v4();
    let purged = controlled_append_before_purge(
        &fixture.pool,
        RetentionGate {
            subject_id: retention_subject_id,
            lease_token: retention_lease_token,
            now: retention_now,
        },
        fixture.project_id,
        review_task,
        controller_id,
        concurrent_intent_id,
    )
    .await;
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
    let (semantic_intent_list_after_purge, semantic_provenance_list_after_purge) =
        semantic_operational_lists(
            &fixture.pool,
            fixture.project_id,
            controller_id,
            controller_device,
        )
        .await;
    assert_eq!(
        semantic_intent_list_after_purge.len(),
        semantic_intent_list_before_purge.len() + 1
    );
    assert_eq!(
        semantic_intent_list_after_purge.last().unwrap()["id"],
        concurrent_intent_id.to_string()
    );
    assert_eq!(
        semantic_provenance_list_after_purge, semantic_provenance_list_before_purge,
        "retention must preserve every provenance list element and position"
    );
    assert_prefix(
        &semantic_intent_list_before_purge,
        &semantic_intent_list_after_purge,
    );
    assert_prefix(
        &semantic_provenance_list_before_purge,
        &semantic_provenance_list_after_purge,
    );
    let (secondary_purge_resource, _) =
        create_cross_owner_tasks(&fixture, requester_id, controller_id).await;
    let secondary_gate =
        prepare_retention_gate(&fixture, secondary_purge_resource, requester_id).await;
    let purge_first_intent_id = Uuid::new_v4();
    let secondary_purged = controlled_purge_before_append(
        &fixture.pool,
        secondary_gate,
        fixture.project_id,
        review_task,
        controller_id,
        purge_first_intent_id,
    )
    .await;
    assert!(secondary_purged);
    let (semantic_intent_list_after_purge_orders, semantic_provenance_list_after_purge_orders) =
        semantic_operational_lists(
            &fixture.pool,
            fixture.project_id,
            controller_id,
            controller_device,
        )
        .await;
    assert_prefix(
        &semantic_intent_list_after_purge,
        &semantic_intent_list_after_purge_orders,
    );
    assert_eq!(
        semantic_intent_list_after_purge_orders.len(),
        semantic_intent_list_after_purge.len() + 1
    );
    assert_eq!(
        semantic_intent_list_after_purge_orders.last().unwrap()["id"],
        purge_first_intent_id.to_string()
    );
    assert_eq!(
        semantic_provenance_list_after_purge_orders,
        semantic_provenance_list_after_purge
    );
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

    let appended_intent_id = Uuid::new_v4();
    let appended_provenance_id = Uuid::new_v4();
    let appended_local_goal_id = Uuid::new_v4();
    let appended_local = local_goal_value(
        appended_local_goal_id,
        1,
        agent_identity_id,
        controller_id,
        review_task,
        review_task,
        "assign_own_task",
    );
    let exact_obligation: Uuid = appended_local["contract"]["obligations"][0]["id"]
        .as_str()
        .expect("appended obligation id")
        .parse()
        .expect("appended obligation UUID");
    install_preexisting_active_local_goal_fixture(
        &fixture,
        &controller,
        agent_id,
        responsibility_id,
        &appended_local,
    )
    .await;
    let mut append = fixture.pool.begin().await.expect("begin post-purge append");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *append)
        .await
        .expect("disable RLS for post-purge append fixture");
    sqlx::query(
        "INSERT INTO agent_task_intents (
             id, project_id, task_resource_node_id, scope_resource_node_id,
             required_actions, derived_by_identity_id
         ) VALUES ($1, $2, $3, $3, '[\"assign_own_task\"]'::jsonb, $4)",
    )
    .bind(appended_intent_id)
    .bind(fixture.project_id)
    .bind(review_task)
    .bind(controller_id)
    .execute(&mut *append)
    .await
    .expect("append TaskIntent after retention");
    sqlx::query(
        "INSERT INTO agent_task_obligation_provenance (
             id, project_id, task_intent_id, task_resource_node_id,
             target_agent_id, local_goal_id, local_goal_revision,
             obligation_id, work_spec_ordinal
         ) VALUES ($1, $2, $3, $4, $5, $6, 1, $7, 1)",
    )
    .bind(appended_provenance_id)
    .bind(fixture.project_id)
    .bind(appended_intent_id)
    .bind(review_task)
    .bind(agent_id)
    .bind(appended_local_goal_id)
    .bind(exact_obligation)
    .execute(&mut *append)
    .await
    .expect("append task provenance after retention");
    append.commit().await.expect("commit post-purge append");
    let (semantic_intent_list_after_append, semantic_provenance_list_after_append) =
        semantic_operational_lists(
            &fixture.pool,
            fixture.project_id,
            controller_id,
            controller_device,
        )
        .await;
    assert_prefix(
        &semantic_intent_list_after_purge_orders,
        &semantic_intent_list_after_append,
    );
    assert_prefix(
        &semantic_provenance_list_after_purge_orders,
        &semantic_provenance_list_after_append,
    );
    assert_eq!(
        semantic_intent_list_after_append.len(),
        semantic_intent_list_after_purge_orders.len() + 1
    );
    assert_eq!(
        semantic_provenance_list_after_append.len(),
        semantic_provenance_list_after_purge_orders.len() + 1
    );

    let concurrent_ids = [
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    let first_order = controlled_concurrent_intent_pair(
        &fixture.pool,
        fixture.project_id,
        review_task,
        controller_id,
        concurrent_ids[0],
        concurrent_ids[1],
    )
    .await;
    let reverse_declared_order = controlled_concurrent_intent_pair(
        &fixture.pool,
        fixture.project_id,
        review_task,
        controller_id,
        concurrent_ids[3],
        concurrent_ids[2],
    )
    .await;
    assert!(first_order.0 < first_order.1);
    assert!(reverse_declared_order.0 < reverse_declared_order.1);
    let (semantic_intent_list_after_concurrency, semantic_provenance_after_concurrency) =
        semantic_operational_lists(
            &fixture.pool,
            fixture.project_id,
            controller_id,
            controller_device,
        )
        .await;
    assert_prefix(
        &semantic_intent_list_after_append,
        &semantic_intent_list_after_concurrency,
    );
    assert_eq!(
        semantic_intent_list_after_concurrency.len(),
        semantic_intent_list_after_append.len() + 4
    );
    let mut concurrent_positions = semantic_intent_list_after_concurrency
        .iter()
        .filter(|entry| {
            concurrent_ids
                .iter()
                .any(|id| entry["id"] == id.to_string())
        })
        .map(|entry| entry["semantic_position"].as_i64().expect("position"))
        .collect::<Vec<_>>();
    concurrent_positions.sort_unstable();
    assert_eq!(concurrent_positions.len(), 4);
    assert_eq!(concurrent_positions[1], concurrent_positions[0] + 1);
    assert_eq!(concurrent_positions[2], concurrent_positions[1] + 1);
    assert_eq!(concurrent_positions[3], concurrent_positions[2] + 1);
    assert_eq!(
        semantic_provenance_after_concurrency,
        semantic_provenance_list_after_append
    );

    let retained_before_replay = retained_history_snapshot(&fixture.pool, fixture.project_id).await;
    let ledger_before_replay = semantic_ledger_snapshot(&fixture.pool, fixture.project_id).await;
    let second_purge =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.purge_retention_subject($1, $2, $3)")
            .bind(retention_subject_id)
            .bind(retention_lease_token)
            .bind(retention_now)
            .fetch_one(&fixture.pool)
            .await
            .expect("repeat already-completed retention purge");
    assert!(
        second_purge,
        "the established purge contract returns true for an already-purged subject"
    );
    let (semantic_intent_list_after_second_purge, semantic_provenance_after_second_purge) =
        semantic_operational_lists(
            &fixture.pool,
            fixture.project_id,
            controller_id,
            controller_device,
        )
        .await;
    assert_eq!(
        semantic_intent_list_after_second_purge,
        semantic_intent_list_after_concurrency
    );
    assert_eq!(
        semantic_provenance_after_second_purge,
        semantic_provenance_after_concurrency
    );
    assert_eq!(
        semantic_ledger_snapshot(&fixture.pool, fixture.project_id).await,
        ledger_before_replay
    );
    assert_eq!(
        retained_history_snapshot(&fixture.pool, fixture.project_id).await,
        retained_before_replay
    );

    let database_url = env::var("DATABASE_URL").expect("database URL for restart projection");
    let restarted_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("reconnect semantic projection after restart");
    let (semantic_intent_list_after_restart, semantic_provenance_after_restart) =
        semantic_operational_lists(
            &restarted_pool,
            fixture.project_id,
            controller_id,
            controller_device,
        )
        .await;
    assert_eq!(
        semantic_intent_list_after_restart,
        semantic_intent_list_after_concurrency
    );
    assert_eq!(
        semantic_provenance_after_restart,
        semantic_provenance_after_concurrency
    );
}
