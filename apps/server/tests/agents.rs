use std::{env, sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sprout_crypto_protocol::{
    DeviceKeyIds, DevicePublicPackage, ExperimentalWrappedResourceKey, HybridWrapMetadata,
    KeyAlgorithm, ResourceKey, canonical_governance_json, generate_experimental_device_package,
    hash_bytes, sign_ed25519_ml_dsa65, unwrap_resource_key, wrap_resource_key,
};
use sprout_domain::{
    GlobalContractCandidate, LocalGoalContract, LocalGoalOrigin, StructuredGlobalSynthesisEnvelope,
    StructuredGlobalWorkGrounding, classify_local_goal_contract,
};
use sprout_server::{
    AppState, build_router,
    config::Config,
    worker::{self, WorkerKind, WorkerOptions},
};
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

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn native_comments_preserve_exact_depth_replay_and_r541_gate() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let (ordinary_user_id, _ordinary_device_id, ordinary_user_token, _ordinary_permission) =
        add_human_member(&fixture, "member").await;
    let (first_status, first) =
        provision_administrator_governed_agent(&fixture, &app, 211, None, None, |_| {}, true).await;
    assert_eq!(first_status, StatusCode::OK);
    let (second_status, second) =
        provision_administrator_governed_agent(&fixture, &app, 221, None, None, |_| {}, true).await;
    assert_eq!(second_status, StatusCode::OK);
    let first_runner = activate_exact_tool_runner(&fixture, &app, &first).await;
    let second_runner = activate_exact_tool_runner(&fixture, &app, &second).await;
    let mut permission = fixture
        .pool
        .begin()
        .await
        .expect("begin comment permission setup");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *permission)
        .await
        .expect("disable RLS for comment permission fixture");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
           $1,$2,$3,'edit','full','restricted',$4,$3)",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(fixture.owner_id)
    .bind(Uuid::new_v4())
    .execute(&mut *permission)
    .await
    .expect("grant owner exact Comment capability");
    permission
        .commit()
        .await
        .expect("commit comment permission setup");

    async fn create_and_claim_comment_run(
        fixture: &Fixture,
        app: &axum::Router,
        provisioned: &ProvisionedGovernanceAgent,
        bearer: &str,
    ) -> (Uuid, Uuid, Uuid) {
        let run_id = Uuid::new_v4();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/projects/{}/agent-runs", fixture.project_id))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {}", fixture.owner_token))
                    .body(Body::from(
                        json!({
                            "id":run_id,
                            "source":{"kind":"local_goal","id":provisioned.local_goal_id,"revision":1},
                            "authority_envelope":{
                                "resource_authority":[{
                                    "resource_id":fixture.profile_resource_id,
                                    "operation":"post_comment"
                                }],
                                "tool_authority":[]
                            }
                        })
                        .to_string(),
                    ))
                    .expect("create native-comment run"),
            )
            .await
            .expect("create native-comment run response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            json_body(response).await
        );
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}/claim",
                        fixture.project_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::from("{}"))
                    .expect("claim native-comment work"),
            )
            .await
            .expect("claim native-comment work response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            json_body(response).await
        );
        let state = json_body(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri(format!(
                            "/v1/projects/{}/agent-runs/{run_id}",
                            fixture.project_id
                        ))
                        .header("authorization", format!("Bearer {bearer}"))
                        .body(Body::empty())
                        .expect("read native-comment run"),
                )
                .await
                .expect("read native-comment run response"),
        )
        .await;
        let (claim_id, claim) = state["state"]["claims"]
            .as_object()
            .and_then(|claims| claims.iter().next())
            .expect("native-comment claim");
        (
            run_id,
            Uuid::parse_str(claim_id).expect("native-comment claim UUID"),
            Uuid::parse_str(claim["work"].as_str().expect("native-comment work"))
                .expect("native-comment work UUID"),
        )
    }

    async fn post_agent_comment_request(
        fixture: &Fixture,
        app: &axum::Router,
        bearer: &str,
        run_id: Uuid,
        claim_id: Uuid,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}/claims/{claim_id}/comments",
                        fixture.project_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::from(body.to_string()))
                    .expect("post bound agent comment"),
            )
            .await
            .expect("post bound agent comment response");
        let status = response.status();
        (status, json_body(response).await)
    }

    let (first_run, first_claim, first_work) =
        create_and_claim_comment_run(&fixture, &app, &first, &first_runner.bearer).await;
    let payload = |seed: u8| {
        json!({
            "version":1,
            "algorithm":"aes-256-gcm",
            "key_id":format!("comment-key-{seed}"),
            "nonce_b64":STANDARD.encode([seed;12]),
            "ciphertext_b64":STANDARD.encode([seed,seed.wrapping_add(1)])
        })
    };

    let administrator_key = Uuid::new_v4();
    let administrator_body = json!({
        "recipient_id":first.principal_identity_id,
        "target_id":fixture.profile_resource_id,
        "parent_id":null,
        "encrypted_payload":payload(1),
        "key_epoch":1,
        "idempotency_key":administrator_key,
        "run_id":first_run
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/comments", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(administrator_body.to_string()))
                .expect("post administrator comment"),
        )
        .await
        .expect("post administrator comment response");
    let administrator_status = response.status();
    let administrator_response = json_body(response).await;
    assert_eq!(
        administrator_status,
        StatusCode::OK,
        "{administrator_response}"
    );
    let administrator_id = Uuid::parse_str(
        administrator_response["id"]
            .as_str()
            .expect("administrator comment id"),
    )
    .expect("administrator comment UUID");

    let user_comment_body = json!({
        "recipient_id":first.principal_identity_id,
        "target_id":fixture.profile_resource_id,
        "parent_id":null,
        "encrypted_payload":payload(4),
        "key_epoch":1,
        "idempotency_key":Uuid::new_v4(),
        "run_id":first_run
    });
    let user_comment = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/comments", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {ordinary_user_token}"))
                .body(Body::from(user_comment_body.to_string()))
                .expect("post ordinary-user comment"),
        )
        .await
        .expect("post ordinary-user comment response");
    let user_status = user_comment.status();
    let user_response = json_body(user_comment).await;
    assert_eq!(user_status, StatusCode::OK, "{user_response}");
    let user_comment_id = Uuid::parse_str(user_response["id"].as_str().expect("user comment id"))
        .expect("user comment UUID");
    assert_ne!(ordinary_user_id, fixture.owner_id);
    let priority_source_ticks = sqlx::query_as::<_, (i64, i64)>(
        "SELECT high.semantic_tick,low.semantic_tick
         FROM native_comments high,native_comments low
         WHERE high.project_id=$1 AND high.id=$2
           AND low.project_id=$1 AND low.id=$3",
    )
    .bind(fixture.project_id)
    .bind(administrator_id)
    .bind(user_comment_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load non-vacuous Comment priority source ticks");
    assert!(priority_source_ticks.0 < priority_source_ticks.1);

    let root_key = Uuid::new_v4();
    let root_body = json!({
        "recipient_id":second.principal_identity_id,
        "target_id":fixture.profile_resource_id,
        "parent_id":null,
        "encrypted_payload":payload(2),
        "key_epoch":1,
        "idempotency_key":root_key,
        "work_item_id":first_work,
        "attempt":1
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{first_run}/claims/{first_claim}/comments",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", first_runner.bearer))
                .body(Body::from(root_body.to_string()))
                .expect("post agent root comment"),
        )
        .await
        .expect("post agent root comment response");
    let root_status = response.status();
    let root_response = json_body(response).await;
    assert_eq!(root_status, StatusCode::OK, "{root_response}");
    let root_id = Uuid::parse_str(root_response["id"].as_str().expect("root comment id"))
        .expect("root comment UUID");

    let allocations_before_replay = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_semantic_tick_allocations
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("count semantic allocations before Comment replay");

    let replay = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{first_run}/claims/{first_claim}/comments",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", first_runner.bearer))
                .body(Body::from(root_body.to_string()))
                .expect("replay agent root comment"),
        )
        .await
        .expect("replay agent root response");
    assert_eq!(replay.status(), StatusCode::OK);
    let replay = json_body(replay).await;
    assert_eq!(replay["id"], root_response["id"]);
    assert_eq!(replay["replayed"], true);
    let allocations_after_replay = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_semantic_tick_allocations
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("count semantic allocations after Comment replay");
    assert_eq!(allocations_after_replay, allocations_before_replay);

    let priority_reply = |parent_id: Uuid, seed: u8| {
        json!({
            "recipient_id":second.principal_identity_id,
            "target_id":fixture.profile_resource_id,
            "parent_id":parent_id,
            "encrypted_payload":payload(seed),
            "key_epoch":1,
            "idempotency_key":Uuid::new_v4(),
            "work_item_id":first_work,
            "attempt":1
        })
    };
    let (low_first_status, _) = post_agent_comment_request(
        &fixture,
        &app,
        &first_runner.bearer,
        first_run,
        first_claim,
        priority_reply(user_comment_id, 5),
    )
    .await;
    assert_eq!(low_first_status, StatusCode::FORBIDDEN);
    let (high_status, high_body) = post_agent_comment_request(
        &fixture,
        &app,
        &first_runner.bearer,
        first_run,
        first_claim,
        priority_reply(administrator_id, 6),
    )
    .await;
    assert_eq!(high_status, StatusCode::OK, "{high_body}");
    let (low_after_high_status, low_after_high_body) = post_agent_comment_request(
        &fixture,
        &app,
        &first_runner.bearer,
        first_run,
        first_claim,
        priority_reply(user_comment_id, 7),
    )
    .await;
    assert_eq!(
        low_after_high_status,
        StatusCode::OK,
        "{low_after_high_body}"
    );

    let (second_run, second_claim, second_work) =
        create_and_claim_comment_run(&fixture, &app, &second, &second_runner.bearer).await;
    let reply_body = json!({
        "recipient_id":first.principal_identity_id,
        "target_id":fixture.profile_resource_id,
        "parent_id":root_id,
        "encrypted_payload":payload(3),
        "key_epoch":1,
        "idempotency_key":Uuid::new_v4(),
        "work_item_id":second_work,
        "attempt":1
    });
    let reply = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{second_run}/claims/{second_claim}/comments",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", second_runner.bearer))
                .body(Body::from(reply_body.to_string()))
                .expect("post agent reply comment"),
        )
        .await
        .expect("post agent reply comment response");
    assert_eq!(reply.status(), StatusCode::OK, "{}", json_body(reply).await);

    // Two independent Comment events racing on one run serialize through the
    // canonical run clock.  Neither wall-clock delay nor timestamp precision
    // participates in collision freedom.
    let concurrent_body = |seed: u8| {
        json!({
            "recipient_id":first.principal_identity_id,
            "target_id":fixture.profile_resource_id,
            "parent_id":null,
            "encrypted_payload":payload(seed),
            "key_epoch":1,
            "idempotency_key":Uuid::new_v4(),
            "run_id":first_run
        })
    };
    let concurrent_request = |body: Value| {
        app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/comments", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(body.to_string()))
                .expect("build concurrent Comment request"),
        )
    };
    let (concurrent_a, concurrent_b) = tokio::join!(
        concurrent_request(concurrent_body(8)),
        concurrent_request(concurrent_body(9))
    );
    assert_eq!(
        concurrent_a
            .expect("first concurrent Comment response")
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        concurrent_b
            .expect("second concurrent Comment response")
            .status(),
        StatusCode::OK
    );

    let collision_freedom = sqlx::query_as::<_, (i64, i64, i64, bool)>(
        "SELECT count(*),count(DISTINCT comment.semantic_tick),
                count(DISTINCT allocation.semantic_tick),
                bool_and(comment.semantic_tick=allocation.semantic_tick)
         FROM native_comments comment
         JOIN native_comment_events event
           ON event.project_id=comment.project_id AND event.comment_id=comment.id
         JOIN agent_run_semantic_tick_allocations allocation
           ON allocation.project_id=comment.project_id AND allocation.run_id=comment.run_id
          AND allocation.event_key=event.id AND allocation.event_kind='comment_posted'
         WHERE comment.project_id=$1 AND comment.run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify Comment run tick collision freedom");
    assert_eq!(collision_freedom.0, collision_freedom.1);
    assert_eq!(collision_freedom.0, collision_freedom.2);
    assert!(collision_freedom.3);
    let cross_family_ticks = sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*),count(DISTINCT semantic_tick) FROM (
           SELECT transition.semantic_tick
           FROM agent_run_transitions transition
           WHERE transition.project_id=$1 AND transition.run_id=$2
           UNION ALL
           SELECT event.semantic_tick
           FROM native_comment_events event
           WHERE event.project_id=$1 AND event.run_id=$2
         ) formal_events",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify cross-family run Event tick collision freedom");
    assert_eq!(cross_family_ticks.0, cross_family_ticks.1);

    let exact = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64)>(
        "SELECT
           count(*) FILTER (WHERE author_kind='administrator' AND agent_depth=0),
           count(*) FILTER (WHERE author_kind='user' AND agent_depth=0),
           count(*) FILTER (WHERE author_kind='agent' AND parent_comment_id IS NULL AND agent_depth=1),
           count(*) FILTER (WHERE author_kind='agent' AND parent_comment_id IS NOT NULL AND agent_depth=1),
           count(*) FILTER (WHERE author_kind='agent' AND parent_comment_id IS NOT NULL AND agent_depth=2),
           (SELECT count(*) FROM agent_r541_comment_surface_records WHERE project_id=$1),
           (SELECT count(*) FROM native_comment_notifications WHERE project_id=$1)
         FROM native_comments WHERE project_id=$1",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact native comment ledger");
    assert_eq!(exact, (3, 1, 1, 2, 1, 8, 8));
    let semantic_exactness = sqlx::query_as::<_, (i64, i64, i64, bool, bool, i64)>(
        "SELECT
           (SELECT count(*) FROM agent_r541_typed_exact_comment_records WHERE project_id=$1),
           count(*),count(DISTINCT event.comment_id),
           NOT EXISTS (SELECT 1 FROM (
             SELECT project_ordinal,row_number() OVER (ORDER BY project_ordinal) expected
             FROM native_comment_events WHERE project_id=$1
           ) ordered WHERE ordered.project_ordinal<>ordered.expected),
           bool_and(event.semantic_state_hash=digest(convert_to(concat_ws(E'\\n',
             'sprout-native-comment-semantic-state-v1',event.project_id::text,
             event.project_ordinal::text,COALESCE(encode(event.previous_state_hash,'hex'),''),
             encode(event.event_hash,'hex')),'UTF8'),'sha256')),
           (SELECT count(*) FROM native_comment_responses WHERE project_id=$1)
         FROM native_comment_events event WHERE event.project_id=$1",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify canonical Comment semantic-state append");
    assert_eq!(semantic_exactness, (8, 8, 8, true, true, 3));

    let exact_payload = sqlx::query_scalar::<_, Value>(
        "SELECT exact_comment->'payload'
         FROM agent_r541_typed_exact_comment_records
         WHERE project_id=$1 AND comment_id=$2",
    )
    .bind(fixture.project_id)
    .bind(root_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("reconstruct exact encrypted Comment payload");
    assert_eq!(exact_payload, payload(2));
    let comments_exist_in_exact_semantic_state = sqlx::query_scalar::<_, bool>(
        "SELECT bool_and(semantic_state_comments @> jsonb_build_array(exact_comment))
         FROM agent_r541_typed_exact_comment_records WHERE project_id=$1",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify each R541 Comment exists in the canonical semantic-state prefix");
    assert!(comments_exist_in_exact_semantic_state);

    let mut duplicate_event = fixture
        .pool
        .begin()
        .await
        .expect("begin duplicate commentPosted attack");
    sqlx::query("SET LOCAL row_security=off")
        .execute(&mut *duplicate_event)
        .await
        .expect("disable RLS for migration-owner invariant attack");
    let duplicate = sqlx::query(
        "INSERT INTO native_comment_events(
           id,project_id,comment_id,project_ordinal,run_id,trace_number,semantic_tick,
           event_kind,action_path,comment_snapshot,event_hash,previous_state_hash,semantic_state_hash)
         SELECT gen_random_uuid(),project_id,comment_id,
           (SELECT max(project_ordinal)+1 FROM native_comment_events WHERE project_id=$1),
           run_id,trace_number,semantic_tick+1,event_kind,action_path,comment_snapshot,
           digest(convert_to('duplicate-comment-event','UTF8'),'sha256'),semantic_state_hash,
           digest(convert_to('duplicate-comment-state','UTF8'),'sha256')
         FROM native_comment_events WHERE project_id=$1 AND comment_id=$2",
    )
    .bind(fixture.project_id)
    .bind(root_id)
    .execute(&mut *duplicate_event)
    .await;
    assert!(
        duplicate.is_err(),
        "one CommentId cannot have a second commentPosted tick"
    );
    duplicate_event
        .rollback()
        .await
        .expect("rollback duplicate commentPosted attack");

    let gates = sqlx::query_as::<_, (i64, String, i32, i64, i64)>(
        "SELECT gate.trace_number,gate.comment_mode,jsonb_array_length(gate.comment_records),
           (SELECT count(*) FROM agent_r541_exact_comment_certificates exact
             WHERE exact.trace_number=gate.trace_number),
           (SELECT count(*) FROM agent_r541_typed_exact_comment_records exact
             WHERE exact.trace_number=gate.trace_number)
         FROM agent_r541_comment_surface_gates gate
         WHERE gate.project_id=$1 AND gate.run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact first-run comment gate");
    assert!(gates.0 > 0);
    assert_eq!(
        (gates.1.as_str(), gates.2, gates.3, gates.4),
        ("enabled", 7, 1, 7)
    );

    let allocations_before_equivocation = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_semantic_tick_allocations
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("count semantic allocations before Comment equivocation");
    let mut equivocated = root_body;
    equivocated["encrypted_payload"] = payload(9);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{first_run}/claims/{first_claim}/comments",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", first_runner.bearer))
                .body(Body::from(equivocated.to_string()))
                .expect("reject comment equivocation"),
        )
        .await
        .expect("reject comment equivocation response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let allocations_after_equivocation = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_run_semantic_tick_allocations
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("count semantic allocations after Comment equivocation");
    assert_eq!(
        allocations_after_equivocation,
        allocations_before_equivocation
    );

    let comment_retention_state = |pool: PgPool, project_id: Uuid| async move {
        sqlx::query_as::<_, (i64, i64, i64, i64, i64, String)>(
            "SELECT
               (SELECT count(*) FROM native_comments
                 WHERE project_id=$1 AND encrypted_payload IS NOT NULL),
               (SELECT count(*) FROM agent_r541_typed_exact_comment_records
                 WHERE project_id=$1),
               (SELECT count(*) FROM agent_r541_comment_records WHERE project_id=$1),
               (SELECT count(*) FROM agent_r541_comment_inventory WHERE project_id=$1),
               (SELECT count(*) FROM agent_r541_comment_certificates WHERE project_id=$1),
               encode(digest(convert_to(concat_ws(E'\\n',
                 COALESCE((SELECT jsonb_agg(to_jsonb(comment)-'encrypted_payload'
                   -'payload_purged_at'-'recorded_at' ORDER BY comment.id)::text
                   FROM native_comments comment WHERE comment.project_id=$1),'[]'),
                 COALESCE((SELECT jsonb_agg(to_jsonb(event)-'recorded_at'
                   ORDER BY event.project_ordinal)::text FROM native_comment_events event
                   WHERE event.project_id=$1),'[]'),
                 COALESCE((SELECT jsonb_agg(to_jsonb(record)-'recorded_at'
                   ORDER BY record.trace_number,record.semantic_tick,record.id)::text
                   FROM agent_r541_comment_records record WHERE record.project_id=$1),'[]'),
                 COALESCE((SELECT jsonb_agg(to_jsonb(inventory)-'recorded_at'
                   ORDER BY inventory.trace_number,inventory.ordinal)::text
                   FROM agent_r541_comment_inventory inventory WHERE inventory.project_id=$1),'[]'),
                 COALESCE((SELECT jsonb_agg(to_jsonb(certificate)-'recorded_at'
                   ORDER BY certificate.trace_number,certificate.version)::text
                   FROM agent_r541_comment_certificates certificate
                   WHERE certificate.project_id=$1),'[]')),'UTF8'),'sha256'),'hex')",
        )
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .expect("snapshot immutable Comment structure")
    };
    let before_retention = comment_retention_state(fixture.pool.clone(), fixture.project_id).await;
    assert_eq!(before_retention.0, 8);
    assert_eq!(before_retention.1, 8);
    assert_eq!(before_retention.2, 8);
    assert_eq!(before_retention.3, 8);
    assert!(before_retention.4 >= 2);

    purge_resource_through_retention(&fixture, fixture.profile_resource_id, fixture.owner_id).await;

    let after_retention = comment_retention_state(fixture.pool.clone(), fixture.project_id).await;
    assert_eq!(after_retention.0, 0);
    assert_eq!(after_retention.1, 0);
    assert_eq!(after_retention.2, before_retention.2);
    assert_eq!(after_retention.3, before_retention.3);
    assert_eq!(after_retention.4, before_retention.4);
    assert_eq!(after_retention.5, before_retention.5);
    let retained_gate = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT count(*),
           count(*) FILTER (WHERE comment_mode='disabled_fail_closed'
             AND comment_records='[]'::jsonb),
           (SELECT count(*) FROM agent_r541_comment_surface_records
             WHERE project_id=$1 AND run_id=$2)
         FROM agent_r541_comment_surface_gates
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(first_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load fail-closed Comment gate after payload retention");
    assert_eq!(retained_gate, (1, 1, 0));
    let retained_read = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/projects/{}/resources/{}/comments",
                    fixture.project_id, fixture.profile_resource_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("read purged Comment payload"),
        )
        .await
        .expect("read purged Comment response");
    assert_eq!(retained_read.status(), StatusCode::OK);
    assert_eq!(json_body(retained_read).await["comments"], json!([]));
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

fn configure_exact_external_tool_governance(body: &mut Value, tool: &str) {
    let output = body
        .pointer_mut("/initial_local_goal/compilation/statement/output")
        .expect("tool compiler output");
    output["contract"]["work_specs"][0]["kind"] = json!("tool_invocation");
    output["contract"]["work_specs"][0]["allowed_actions"] = json!(["invoke_tool", "retry_tool"]);
    // WorkAttempt uses the Lean-exclusive bound. The fixture permits ToolCall
    // attempts 1 through 4, so the exact WorkSpec upper bound is 5.
    output["contract"]["work_specs"][0]["max_attempts"] = json!(5);
    output["contract"]["work_specs"][0]["failure_plan"] = json!({"kind":"retry_same"});
    output["requirements"][0]["required_actions"] = json!(["invoke_tool", "retry_tool"]);
    output["requirements"][0]["required_tools"] = json!([tool]);
    output["security_policies"][0]["allowed_operations"] = json!([]);
    output["security_policies"][0]["allowed_tools"] = json!([tool]);
    let output = output.clone();

    let envelope = body
        .pointer_mut("/initial_local_goal/compilation/statement/envelope")
        .expect("tool compiler envelope");
    envelope["language_task"]["allowed_tools"] = json!([tool]);
    envelope["allowed_actions"] = json!(["invoke_tool", "retry_tool"]);
    let envelope = envelope.clone();

    let compilation = body
        .pointer_mut("/initial_local_goal/compilation/statement")
        .expect("tool compilation statement");
    compilation["output_hash_hex"] = json!(canonical_hash_hex(&output));
    compilation["envelope_hash_hex"] = json!(canonical_hash_hex(&envelope));
    let compilation = compilation.clone();

    let final_statement = body
        .pointer_mut("/final_prompt_approval/statement")
        .expect("tool final prompt statement");
    final_statement["structured_output_hash_hex"] = json!(canonical_hash_hex(&output));
    let approval_identity = json!({
        "signature_context": "sprout-final-prompt-approval-v1",
        "approval_id": final_statement["approval_id"],
        "project_id": final_statement["project_id"],
        "draft_id": final_statement["draft_id"],
        "agent_principal_identity_id": final_statement["agent_principal_identity_id"],
        "controller_identity_id": final_statement["controller_identity_id"],
        "local_goal_id": final_statement["local_goal_id"],
        "local_revision": final_statement["local_revision"],
        "prompt_commitment_hex": final_statement["prompt_commitment_hex"],
        "ciphertext_commitment_hex": final_statement["ciphertext_commitment_hex"],
        "compilation_certificate_id": final_statement["compilation_certificate_id"],
        "structured_output_hash_hex": final_statement["structured_output_hash_hex"],
        "idempotency_key": final_statement["idempotency_key"],
    });
    final_statement["approval_identity_hash_hex"] = json!(canonical_hash_hex(&approval_identity));

    let encrypted_prompt = body["initial_local_goal"]["encrypted_prompt"].clone();
    let administrator = body
        .pointer_mut("/administrator_creation_approval/statement")
        .expect("tool administrator creation statement");
    let local_contract = json!({
        "id": compilation["local_goal_id"],
        "revision": compilation["local_revision"],
        "agent": compilation["agent_principal_identity_id"],
        "controller": compilation["controller_identity_id"],
        "encrypted_prompt": encrypted_prompt,
        "contract": output["contract"],
        "clauses": [{
            "id": 1,
            "domain": 2,
            "scope": compilation["project_scope"],
            "work_spec_ids": [1]
        }],
        "origin": {
            "kind": "administrator_creation",
            "approval_id": administrator["approval_id"]
        },
        "supersedes_revision": null
    });
    // The compiler envelope, rather than the compilation statement, owns the
    // exact project scope field.
    let mut local_contract = local_contract;
    local_contract["clauses"][0]["scope"] = envelope["project_scope"].clone();
    administrator["contract_hash_hex"] = json!(canonical_hash_hex(&local_contract));
    let proposal_binding = json!({
        "project_id": administrator["project_id"],
        "administrator_identity_id": administrator["administrator_identity_id"],
        "proposed_agent_identity_id": administrator["proposed_agent_identity_id"],
        "governed_agent_id": administrator["governed_agent_id"],
        "proposal_draft_id": administrator["proposal_draft_id"],
        "local_goal_id": administrator["local_goal_id"],
        "local_goal_revision": administrator["local_goal_revision"],
        "contract_hash_hex": administrator["contract_hash_hex"],
        "compilation_certificate_id": administrator["compilation_certificate_id"],
        "prompt_plaintext_commitment_hex": administrator["prompt_plaintext_commitment_hex"],
        "ciphertext_commitment_hex": administrator["ciphertext_commitment_hex"],
        "availability": administrator["availability"],
        "scope": administrator["scope"],
    });
    administrator["canonical_proposal_hash_hex"] = json!(canonical_hash_hex(&proposal_binding));
}

struct ActivatedToolRunner {
    bearer: String,
    x25519_private_key: Vec<u8>,
    ml_kem_768_private_key: Vec<u8>,
    ed25519_private_key: Vec<u8>,
    ml_dsa_65_private_key: Vec<u8>,
}

async fn attest_exact_tool_runtime(
    fixture: &Fixture,
    app: &axum::Router,
    provisioned: &ProvisionedGovernanceAgent,
    runner: &ActivatedToolRunner,
    manifest_hash: &[u8],
    profile_byte: u8,
    lifetime: ChronoDuration,
) -> (Uuid, String) {
    let profile_commitment_hex = format!("{profile_byte:02x}").repeat(32);
    let witness_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();
    let issued_at = Utc::now();
    let expires_at = issued_at + lifetime;
    let statement = json!({
        "signature_context":"sprout-external-tool-runtime-capability-v1",
        "witness_id":witness_id,
        "project_id":fixture.project_id,
        "agent_id":provisioned.agent_id,
        "owner_identity_id":provisioned.principal_identity_id,
        "runner_id":provisioned.runner_id,
        "tool_id":"web.read",
        "tool_version":1,
        "manifest_hash_hex":hex::encode(manifest_hash),
        "profile_tool_available":true,
        "runtime_available":true,
        "execution_profile_commitment_hex":profile_commitment_hex,
        "issued_at":issued_at,
        "expires_at":expires_at,
        "idempotency_key":idempotency_key
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/tool-runtime-capabilities",
                    fixture.project_id, provisioned.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "witness_id":witness_id,
                        "tool_id":"web.read",
                        "tool_version":1,
                        "execution_profile_commitment_hex":profile_commitment_hex,
                        "issued_at":issued_at,
                        "expires_at":expires_at,
                        "idempotency_key":idempotency_key,
                        "signatures":signed_statement_by(
                            provisioned.principal_identity_id,
                            provisioned.runner_device_id,
                            &runner.ed25519_private_key,
                            &runner.ml_dsa_65_private_key,
                            &statement,
                            b"sprout-external-tool-runtime-capability-v1"
                        )
                    })
                    .to_string(),
                ))
                .expect("attest exact tool runtime request"),
        )
        .await
        .expect("attest exact tool runtime response");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        json_body(response).await
    );
    (witness_id, profile_commitment_hex)
}

async fn rearm_claim_and_retry_exact_tool(
    fixture: &Fixture,
    app: &axum::Router,
    runner: &ActivatedToolRunner,
    run_id: Uuid,
    call_id: Uuid,
    witness_id: Uuid,
    expected_attempt: i64,
) -> Uuid {
    for phase in ["re-arm", "claim"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}/claim",
                        fixture.project_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {}", runner.bearer))
                    .body(Body::from("{}"))
                    .expect("advance exact retry WorkAttempt"),
            )
            .await
            .expect("advance exact retry WorkAttempt response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{phase}: {}",
            json_body(response).await
        );
    }
    let state = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {}", runner.bearer))
                    .body(Body::empty())
                    .expect("read exact claimed retry WorkAttempt"),
            )
            .await
            .expect("read exact claimed retry WorkAttempt response"),
    )
    .await;
    let claim_id = state["state"]["claims"]
        .as_object()
        .and_then(|claims| {
            claims.iter().find_map(|(id, claim)| {
                (claim["status"] == "active" && claim["attempt"] == expected_attempt).then_some(id)
            })
        })
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("exact active retry claim");
    let retry = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/retry",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "work_claim_id":claim_id,
                        "runtime_capability_witness_id":witness_id,
                        "idempotency_key":Uuid::new_v4()
                    })
                    .to_string(),
                ))
                .expect("retry exact ToolCall"),
        )
        .await
        .expect("retry exact ToolCall response");
    let status = retry.status();
    let body = json_body(retry).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["attempt"], expected_attempt);
    claim_id
}

async fn claim_exact_tool_dispatch(
    fixture: &Fixture,
    app: &axum::Router,
    runner: &ActivatedToolRunner,
    run_id: Uuid,
    call_id: Uuid,
) -> (Uuid, Uuid) {
    let dispatch_id = Uuid::new_v4();
    let lease_id = Uuid::new_v4();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({"dispatch_id":dispatch_id,"lease_id":lease_id}).to_string(),
                ))
                .expect("claim exact tool dispatch"),
        )
        .await
        .expect("claim exact tool dispatch response");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        json_body(response).await
    );
    (dispatch_id, lease_id)
}

#[allow(clippy::too_many_arguments)]
async fn record_exact_tool_request(
    fixture: &Fixture,
    app: &axum::Router,
    provisioned: &ProvisionedGovernanceAgent,
    runner: &ActivatedToolRunner,
    run_id: Uuid,
    call_id: Uuid,
    dispatch_id: Uuid,
    attempt: i64,
    canonical_input_commitment_hex: &str,
    profile_commitment_hex: &str,
) -> (Uuid, String) {
    let request_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();
    let wire_request_commitment_hex = format!("{:02x}", 0x95_u8 + attempt as u8).repeat(32);
    let signed_at = Utc::now();
    let statement = json!({
        "signature_context":"sprout-external-tool-request-v1",
        "project_id":fixture.project_id,
        "run_id":run_id,
        "call_id":call_id,
        "request_id":request_id,
        "dispatch_id":dispatch_id,
        "attempt":attempt,
        "adapter_protocol":"sprout-edge-web-read-v1",
        "canonical_input_commitment_hex":canonical_input_commitment_hex,
        "wire_request_commitment_hex":wire_request_commitment_hex,
        "execution_profile_commitment_hex":profile_commitment_hex,
        "signed_at":signed_at,
        "idempotency_key":idempotency_key
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/requests",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "request_id":request_id,
                        "dispatch_id":dispatch_id,
                        "wire_request_commitment_hex":wire_request_commitment_hex,
                        "signed_at":signed_at,
                        "idempotency_key":idempotency_key,
                        "signatures":signed_statement_by(
                            provisioned.principal_identity_id,
                            provisioned.runner_device_id,
                            &runner.ed25519_private_key,
                            &runner.ml_dsa_65_private_key,
                            &statement,
                            b"sprout-external-tool-request-v1"
                        )
                    })
                    .to_string(),
                ))
                .expect("record exact external request"),
        )
        .await
        .expect("record exact external request response");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        json_body(response).await
    );
    (request_id, wire_request_commitment_hex)
}

async fn tool_trace_structural_hash(pool: &PgPool, project_id: Uuid) -> Vec<u8> {
    sqlx::query_scalar(
        r#"
        SELECT digest(convert_to(concat_ws(E'\n',
          COALESCE((SELECT string_agg(encode(root_hash,'hex'), ',' ORDER BY trace_number)
                    FROM agent_r540_tool_trace_roots WHERE project_id=$1), ''),
          COALESCE((SELECT string_agg(encode(event_hash,'hex'), ',' ORDER BY trace_number,ordinal)
                    FROM agent_r540_tool_trace_inventory WHERE project_id=$1), ''),
          COALESCE((SELECT string_agg(encode(certificate_hash,'hex'), ','
                                      ORDER BY trace_number,version)
                    FROM agent_r540_tool_trace_certificates WHERE project_id=$1), '')
        ), 'UTF8'), 'sha256')
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .expect("hash structural tool trace")
}

async fn run_agent_completion_once(fixture: &Fixture, config: Config) {
    let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    worker::run(
        fixture.pool.clone(),
        config,
        WorkerOptions {
            kind: WorkerKind::AgentCompletion,
            dry_run: false,
            once: true,
            interval: Duration::from_secs(1),
            lease_ttl_seconds: 30,
        },
        shutdown_rx,
    )
    .await
    .expect("materialize exact tool timeout before generic claim recovery");
}

async fn activate_exact_tool_runner(
    fixture: &Fixture,
    app: &axum::Router,
    provisioned: &ProvisionedGovernanceAgent,
) -> ActivatedToolRunner {
    let key_ids = DeviceKeyIds {
        x25519: Uuid::new_v4(),
        ml_kem_768: Uuid::new_v4(),
        ed25519: Uuid::new_v4(),
        ml_dsa_65: Uuid::new_v4(),
    };
    let generated =
        generate_experimental_device_package(provisioned.runner_device_id, key_ids.clone())
            .expect("generate exact tool runner package");
    let public = generated.public_package();
    let key = |algorithm| {
        public
            .encryption_keys
            .iter()
            .chain(&public.signing_keys)
            .find(|key| key.algorithm == algorithm)
            .expect("tool runner public key")
            .public_key
            .clone()
    };
    let package_json = public
        .to_canonical_json()
        .expect("serialize exact tool runner package");
    let mut transaction = fixture.pool.begin().await.expect("begin tool runner keys");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable RLS for tool runner fixture");
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
    .bind(provisioned.principal_identity_id)
    .bind(provisioned.runner_device_id)
    .bind(key(KeyAlgorithm::X25519))
    .bind(key(KeyAlgorithm::Ed25519))
    .bind(package_json)
    .bind(key_ids.x25519)
    .bind(key_ids.ml_kem_768)
    .bind(key_ids.ed25519)
    .bind(key_ids.ml_dsa_65)
    .bind(key(KeyAlgorithm::MlKem768Experimental))
    .bind(key(KeyAlgorithm::MlDsa65Experimental))
    .execute(&mut *transaction)
    .await
    .expect("insert exact tool runner key package");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
             $1, $2, $3, 'edit', 'full', 'restricted', $4, $5
         )",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(provisioned.principal_identity_id)
    .bind(Uuid::new_v4())
    .bind(fixture.owner_id)
    .execute(&mut *transaction)
    .await
    .expect("grant runner exact profile permission");
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
    .bind(provisioned.principal_identity_id)
    .bind(provisioned.runner_device_id)
    .bind(fixture.owner_id)
    .bind(fixture.owner_device_id)
    .execute(&mut *transaction)
    .await
    .expect("insert runner profile key envelope");
    transaction.commit().await.expect("commit tool runner keys");

    let bearer = provisioned
        .bootstrap_token
        .clone()
        .expect("tool runner bootstrap token");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/runner/activate",
                    fixture.project_id, provisioned.agent_id
                ))
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .expect("activate exact tool runner request"),
        )
        .await
        .expect("activate exact tool runner response");
    assert_eq!(response.status(), StatusCode::OK);
    ActivatedToolRunner {
        bearer,
        x25519_private_key: generated.private_keys().x25519().to_vec(),
        ml_kem_768_private_key: generated.private_keys().ml_kem_768().to_vec(),
        ed25519_private_key: generated.private_keys().ed25519().to_vec(),
        ml_dsa_65_private_key: generated.private_keys().ml_dsa_65().to_vec(),
    }
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn governed_external_tool_attempts_preserve_historical_terminal_and_retry_fences() {
    let fixture = fixture().await;
    let mut config = Config::for_test();
    config.agent_work_lease = Duration::from_secs(5);
    config.body_limit_bytes = 64 * 1024;
    let app = build_router(Arc::new(
        AppState::new(config.clone(), fixture.pool.clone()).expect("tool test app state"),
    ))
    .expect("tool test router");
    let (status, provisioned) = provision_administrator_governed_agent(
        &fixture,
        &app,
        201,
        None,
        None,
        |body| configure_exact_external_tool_governance(body, "web.read"),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let runner = activate_exact_tool_runner(&fixture, &app, &provisioned).await;

    for (path, permission_id, idempotency_key) in [
        (
            format!(
                "/v1/projects/{}/resources/{}/principals/{}/tool-permissions/web.read/versions/1",
                fixture.project_id, fixture.profile_resource_id, fixture.owner_id
            ),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ),
        (
            format!(
                "/v1/projects/{}/agents/{}/tool-permissions/web.read/versions/1",
                fixture.project_id, provisioned.agent_id
            ),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(path)
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {}", fixture.owner_token))
                    .body(Body::from(
                        json!({
                            "id": permission_id,
                            "tool_version": 1,
                            "idempotency_key": idempotency_key
                        })
                        .to_string(),
                    ))
                    .expect("grant exact tool permission request"),
            )
            .await
            .expect("grant exact tool permission response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            json_body(response).await
        );
    }

    let run_id = Uuid::new_v4();
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
                        "id": run_id,
                        "source": {"kind":"local_goal", "id":provisioned.local_goal_id, "revision":1},
                        "authority_envelope": {"resource_authority":[], "tool_authority":["web.read"]}
                    })
                    .to_string(),
                ))
                .expect("create exact tool run request"),
        )
        .await
        .expect("create exact tool run response");
    assert_eq!(
        create_run.status(),
        StatusCode::OK,
        "{}",
        json_body(create_run).await
    );
    let initialized_trace = sqlx::query_as::<_, (i64, i64, i64, String, String, Value, Value)>(
        r#"
        SELECT root.trace_number, root.start_tick, transition.semantic_tick,
               certificate.tool_gate_mode, certificate.outcome_gate_mode,
               certificate.tool_event_inventory, certificate.work_outcome_inventory
        FROM agent_r540_tool_trace_roots root
        JOIN agent_run_transitions transition
          ON transition.id=root.initialization_transition_id
        JOIN agent_r540_exact_tool_trace_certificates certificate
          ON certificate.trace_number=root.trace_number AND certificate.version=1
        WHERE root.project_id=$1 AND root.run_id=$2
        "#,
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load run-level trace root before the first tool event");
    assert!(initialized_trace.0 > 0);
    assert_eq!(initialized_trace.1, initialized_trace.2);
    assert_eq!(
        (initialized_trace.3.as_str(), initialized_trace.4.as_str()),
        ("disabled_fail_closed", "disabled_fail_closed")
    );
    assert_eq!(
        (initialized_trace.5, initialized_trace.6),
        (json!([]), json!([]))
    );

    // Consume more than twenty logical events without waiting for the wall
    // clock.  Comment is a native Event on the same run timeline and therefore
    // exercises the real allocator rather than a test-only counter.
    let mut comment_permission = fixture
        .pool
        .begin()
        .await
        .expect("begin rapid Comment grant");
    sqlx::query("SET LOCAL row_security=off")
        .execute(&mut *comment_permission)
        .await
        .expect("disable RLS for rapid Comment grant fixture");
    sqlx::query(
        "SELECT sprout_private.grant_hierarchical_permission(
           $1,$2,$3,'edit','full','restricted',$4,$3)",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(fixture.owner_id)
    .bind(Uuid::new_v4())
    .execute(&mut *comment_permission)
    .await
    .expect("grant rapid Comment capability");
    comment_permission
        .commit()
        .await
        .expect("commit rapid Comment grant");
    let rapid_wall_start = Utc::now();
    for ordinal in 0_u8..20 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/projects/{}/comments", fixture.project_id))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {}", fixture.owner_token))
                    .body(Body::from(
                        json!({
                            "recipient_id":provisioned.principal_identity_id,
                            "target_id":fixture.profile_resource_id,
                            "parent_id":null,
                            "encrypted_payload":{
                                "version":1,
                                "algorithm":"aes-256-gcm",
                                "key_id":format!("rapid-comment-{ordinal}"),
                                "nonce_b64":STANDARD.encode([ordinal;12]),
                                "ciphertext_b64":STANDARD.encode([ordinal,ordinal.wrapping_add(1)])
                            },
                            "key_epoch":1,
                            "idempotency_key":Uuid::new_v4(),
                            "run_id":run_id
                        })
                        .to_string(),
                    ))
                    .expect("post rapid canonical Comment"),
            )
            .await
            .expect("post rapid canonical Comment response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            json_body(response).await
        );
    }
    let rapid_wall_end = Utc::now();
    let rapid_clock =
        sqlx::query_as::<_, (i64, i64, i64, chrono::DateTime<Utc>, chrono::DateTime<Utc>)>(
            "SELECT count(*),count(DISTINCT allocation.semantic_tick),
                max(allocation.semantic_tick)-min(allocation.semantic_tick),
                min(comment.recorded_at),max(comment.recorded_at)
         FROM agent_run_semantic_tick_allocations allocation
         JOIN native_comment_events event
           ON event.project_id=allocation.project_id AND event.run_id=allocation.run_id
          AND event.id=allocation.event_key
         JOIN native_comments comment
           ON comment.project_id=event.project_id AND comment.id=event.comment_id
         WHERE allocation.project_id=$1 AND allocation.run_id=$2
           AND allocation.event_kind='comment_posted'",
        )
        .bind(fixture.project_id)
        .bind(run_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("verify rapid canonical event clock");
    assert_eq!((rapid_clock.0, rapid_clock.1, rapid_clock.2), (20, 20, 19));
    assert!(rapid_clock.3 >= rapid_wall_start - ChronoDuration::seconds(1));
    assert!(rapid_clock.4 <= rapid_wall_end + ChronoDuration::seconds(1));

    let claim = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from("{}"))
                .expect("claim exact tool work request"),
        )
        .await
        .expect("claim exact tool work response");
    assert_eq!(claim.status(), StatusCode::OK, "{}", json_body(claim).await);
    let state = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {}", runner.bearer))
                    .body(Body::empty())
                    .expect("read exact tool run"),
            )
            .await
            .expect("read exact tool run response"),
    )
    .await;
    let (claim_id, claim_value) = state["state"]["claims"]
        .as_object()
        .and_then(|claims| claims.iter().next())
        .expect("exact tool claim");
    let claim_id = Uuid::parse_str(claim_id).expect("exact tool claim id");
    let work_id = Uuid::parse_str(claim_value["work"].as_str().expect("exact tool claim work"))
        .expect("exact tool work id");
    let goal_id = Uuid::parse_str(state["state"]["goal"].as_str().expect("tool goal id"))
        .expect("exact tool goal UUID");

    let manifest_hash = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT manifest_hash FROM agent_external_tool_catalog
         WHERE tool_name='web.read' AND version=1",
    )
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact web tool manifest");
    let (witness_id, profile_commitment_hex) = attest_exact_tool_runtime(
        &fixture,
        &app,
        &provisioned,
        &runner,
        &manifest_hash,
        0x91,
        ChronoDuration::seconds(3),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(1100)).await;
    let call_id = Uuid::new_v4();
    let call_idempotency = Uuid::new_v4();
    let encrypted_input = json!({
        "version": 1,
        "algorithm": "aes-256-gcm",
        "key_id": "fixture-tool-input-key",
        "nonce_b64": "y8vLy8vLy8vLy8vL",
        "ciphertext_b64": "ysw="
    });
    let typed_input: sprout_api_contract::EncryptedPayloadDto =
        serde_json::from_value(encrypted_input.clone()).expect("typed tool input");
    let encrypted_input_commitment =
        sha256_hex(&canonical_governance_json(&typed_input).expect("canonical encrypted input"));
    let structured_input_commitment_hex = "92".repeat(32);
    let input_statement = json!({
        "signature_context":"sprout-external-tool-input-v1",
        "project_id":fixture.project_id,
        "run_id":run_id,
        "goal_id":goal_id,
        "work_item_id":work_id,
        "claim_id":claim_id,
        "attempt":1,
        "owner_identity_id":provisioned.principal_identity_id,
        "tool_id":"web.read",
        "tool_version":1,
        "runtime_capability_witness_id":witness_id,
        "encrypted_input_payload_commitment_hex":encrypted_input_commitment,
        "structured_input_commitment_hex":structured_input_commitment_hex,
        "idempotency_key":call_idempotency
    });
    let invoke_body = json!({
        "id":call_id,
        "tool_id":"web.read",
        "tool_version":1,
        "runtime_capability_witness_id":witness_id,
        "encrypted_input":encrypted_input,
        "structured_input_commitment_hex":structured_input_commitment_hex,
        "max_attempts":4,
        "timeout_seconds":3,
        "idempotency_key":call_idempotency,
        "signatures":signed_statement_by(
            provisioned.principal_identity_id,
            provisioned.runner_device_id,
            &runner.ed25519_private_key,
            &runner.ml_dsa_65_private_key,
            &input_statement,
            b"sprout-external-tool-input-v1"
        )
    });
    let invoke = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/claims/{claim_id}/tool-calls",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(invoke_body.to_string()))
                .expect("invoke exact external tool request"),
        )
        .await
        .expect("invoke exact external tool response");
    assert_eq!(
        invoke.status(),
        StatusCode::OK,
        "{}",
        json_body(invoke).await
    );
    let (acquired_tick, requested_tick, expires_tick, requested_at, database_now, deadline_at) =
        sqlx::query_as::<
            _,
            (
                i64,
                i64,
                i64,
                chrono::DateTime<Utc>,
                chrono::DateTime<Utc>,
                chrono::DateTime<Utc>,
            ),
        >(
            "SELECT claim.acquired_semantic_tick,
                    call.requested_tick,
                    claim.expires_semantic_tick,
                    call.requested_at, clock_timestamp(), call.tool_deadline_at
             FROM agent_tool_calls call
             JOIN agent_run_claim_leases claim
               ON claim.project_id=call.project_id AND claim.id=call.work_claim_id
             WHERE call.project_id=$1 AND call.id=$2",
        )
        .bind(fixture.project_id)
        .bind(call_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("load exact requestedAt coordinates");
    assert!(acquired_tick < requested_tick && requested_tick < expires_tick);
    assert!(requested_at <= database_now);
    assert!(database_now - requested_at < ChronoDuration::seconds(5));
    assert_eq!(deadline_at - requested_at, ChronoDuration::seconds(3));

    let replay = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/claims/{claim_id}/tool-calls",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(invoke_body.to_string()))
                .expect("replay exact external tool request"),
        )
        .await
        .expect("replay exact external tool response");
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await["replayed"], true);

    let dispatch_id = Uuid::new_v4();
    let lease_id = Uuid::new_v4();
    let dispatch = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({"dispatch_id":dispatch_id,"lease_id":lease_id}).to_string(),
                ))
                .expect("claim exact tool dispatch request"),
        )
        .await
        .expect("claim exact tool dispatch response");
    assert_eq!(
        dispatch.status(),
        StatusCode::OK,
        "{}",
        json_body(dispatch).await
    );
    let canonical_input_commitment_hex = canonical_hash_hex(&input_statement);
    let request_id = Uuid::new_v4();
    let request_idempotency = Uuid::new_v4();
    let wire_request_commitment_hex = "93".repeat(32);
    let request_signed_at = Utc::now();
    let request_statement = json!({
        "signature_context":"sprout-external-tool-request-v1",
        "project_id":fixture.project_id,
        "run_id":run_id,
        "call_id":call_id,
        "request_id":request_id,
        "dispatch_id":dispatch_id,
        "attempt":1,
        "adapter_protocol":"sprout-edge-web-read-v1",
        "canonical_input_commitment_hex":canonical_input_commitment_hex,
        "wire_request_commitment_hex":wire_request_commitment_hex,
        "execution_profile_commitment_hex":profile_commitment_hex,
        "signed_at":request_signed_at,
        "idempotency_key":request_idempotency
    });
    let request = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/requests",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "request_id":request_id,
                        "dispatch_id":dispatch_id,
                        "wire_request_commitment_hex":wire_request_commitment_hex,
                        "signed_at":request_signed_at,
                        "idempotency_key":request_idempotency,
                        "signatures":signed_statement_by(
                            provisioned.principal_identity_id,
                            provisioned.runner_device_id,
                            &runner.ed25519_private_key,
                            &runner.ml_dsa_65_private_key,
                            &request_statement,
                            b"sprout-external-tool-request-v1"
                        )
                    })
                    .to_string(),
                ))
                .expect("record exact external request"),
        )
        .await
        .expect("record exact external request response");
    assert_eq!(
        request.status(),
        StatusCode::OK,
        "{}",
        json_body(request).await
    );

    let revoke = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/tool-permissions/web.read/versions/1",
                    fixture.project_id, provisioned.agent_id
                ))
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::empty())
                .expect("revoke exact actor tool permission"),
        )
        .await
        .expect("revoke exact actor tool permission response");
    assert_eq!(revoke.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_secs(5)).await;

    let terminal_id = Uuid::new_v4();
    let terminal_idempotency = Uuid::new_v4();
    let terminal_signed_at = Utc::now();
    let terminal_statement = json!({
        "signature_context":"sprout-external-tool-observation-v1",
        "project_id":fixture.project_id,
        "run_id":run_id,
        "call_id":call_id,
        "observation_id":terminal_id,
        "dispatch_id":dispatch_id,
        "lease_id":lease_id,
        "request_id":request_id,
        "attempt":1,
        "tool_id":"web.read",
        "tool_version":1,
        "adapter_protocol":"sprout-edge-web-read-v1",
        "canonical_input_commitment_hex":canonical_input_commitment_hex,
        "wire_request_commitment_hex":wire_request_commitment_hex,
        "execution_profile_commitment_hex":profile_commitment_hex,
        "status":"failed",
        "encrypted_output_payload_commitment_hex":null,
        "canonical_output_commitment_hex":null,
        "output_readable_by":[provisioned.principal_identity_id],
        "failure_code":"controlled_failure",
        "output_key_envelopes":[],
        "signed_at":terminal_signed_at,
        "idempotency_key":terminal_idempotency
    });
    let terminal_body = json!({
        "observation_id":terminal_id,
        "dispatch_id":dispatch_id,
        "lease_id":lease_id,
        "request_id":request_id,
        "status":"failed",
        "encrypted_output":null,
        "canonical_output_commitment_hex":null,
        "failure_code":"controlled_failure",
        "output_key_envelopes":[],
        "signed_at":terminal_signed_at,
        "idempotency_key":terminal_idempotency,
        "signatures":signed_statement_by(
            provisioned.principal_identity_id,
            provisioned.runner_device_id,
            &runner.ed25519_private_key,
            &runner.ml_dsa_65_private_key,
            &terminal_statement,
            b"sprout-external-tool-observation-v1"
        )
    });
    let terminal = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/terminal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(terminal_body.to_string()))
                .expect("record historical terminal observation"),
        )
        .await
        .expect("record historical terminal response");
    assert_eq!(
        terminal.status(),
        StatusCode::OK,
        "{}",
        json_body(terminal).await
    );
    let terminal_replay = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/terminal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(terminal_body.to_string()))
                .expect("replay exact terminal observation"),
        )
        .await
        .expect("replay exact terminal observation response");
    let replay_status = terminal_replay.status();
    let replay_body = json_body(terminal_replay).await;
    assert_eq!(replay_status, StatusCode::OK, "{replay_body}");
    assert_eq!(replay_body["replayed"], true);
    let terminal_audit_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_tool_audit
         WHERE project_id=$1 AND call_id=$2 AND kind='failed' AND attempt=1",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exact replay terminal audit");
    assert_eq!(terminal_audit_count, 1);

    let terminal_snapshot = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {}", runner.bearer))
                    .body(Body::empty())
                    .expect("read terminal WorkAttempt snapshot"),
            )
            .await
            .expect("read terminal WorkAttempt response"),
    )
    .await;
    let terminal_work = &terminal_snapshot["state"]["work_items"][work_id.to_string()];
    assert_eq!(terminal_work["status"], "failed");
    assert_eq!(terminal_work["attempt"], 1);
    let terminal_rows = sqlx::query_as::<_, (String, i32, String, i32)>(
        "SELECT call.current_status, call.current_attempt,
                outcome.work_status, outcome.attempt
         FROM agent_tool_calls call
         JOIN agent_run_external_tool_work_outcomes outcome
           ON outcome.project_id=call.project_id
          AND outcome.run_id=call.run_id
          AND outcome.work_item_id=call.work_item_id
          AND outcome.attempt=call.current_attempt
         WHERE call.project_id=$1 AND call.id=$2",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact failed ToolCall/WorkOutcome snapshot");
    assert_eq!(terminal_rows, ("failed".into(), 1, "failed".into(), 1));

    let rearm_retry = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from("{}"))
                .expect("re-arm retry WorkAttempt"),
        )
        .await
        .expect("re-arm retry WorkAttempt response");
    assert_eq!(
        rearm_retry.status(),
        StatusCode::OK,
        "{}",
        json_body(rearm_retry).await
    );
    let rearmed_state = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {}", runner.bearer))
                    .body(Body::empty())
                    .expect("read re-armed WorkAttempt"),
            )
            .await
            .expect("read re-armed WorkAttempt response"),
    )
    .await;
    assert_eq!(
        rearmed_state["state"]["work_items"][work_id.to_string()]["status"],
        "eligible"
    );
    assert_eq!(
        rearmed_state["state"]["work_items"][work_id.to_string()]["attempt"],
        2
    );

    let claim_retry = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from("{}"))
                .expect("claim exact retry WorkAttempt"),
        )
        .await
        .expect("claim exact retry WorkAttempt response");
    assert_eq!(claim_retry.status(), StatusCode::OK);
    let retry_state = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{run_id}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {}", runner.bearer))
                    .body(Body::empty())
                    .expect("read claimed retry WorkAttempt"),
            )
            .await
            .expect("read claimed retry WorkAttempt response"),
    )
    .await;
    let retry_claim_id = retry_state["state"]["claims"]
        .as_object()
        .and_then(|claims| {
            claims.iter().find_map(|(id, claim)| {
                (claim["status"] == "active" && claim["attempt"] == 2).then_some(id)
            })
        })
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("active retry claim");
    let denied_retry = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/retry",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "work_claim_id":retry_claim_id,
                        "runtime_capability_witness_id":witness_id,
                        "idempotency_key":Uuid::new_v4()
                    })
                    .to_string(),
                ))
                .expect("retry after readiness revocation"),
        )
        .await
        .expect("retry after readiness revocation response");
    assert_eq!(denied_retry.status(), StatusCode::FORBIDDEN);

    let regrant = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/tool-permissions/web.read/versions/1",
                    fixture.project_id, provisioned.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id":Uuid::new_v4(),
                        "tool_version":1,
                        "idempotency_key":Uuid::new_v4()
                    })
                    .to_string(),
                ))
                .expect("restore exact actor tool permission"),
        )
        .await
        .expect("restore exact actor tool permission response");
    assert_eq!(
        regrant.status(),
        StatusCode::OK,
        "{}",
        json_body(regrant).await
    );
    let (retry_witness_id, _) = attest_exact_tool_runtime(
        &fixture,
        &app,
        &provisioned,
        &runner,
        &manifest_hash,
        0x94,
        ChronoDuration::seconds(30),
    )
    .await;
    let retry = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/retry",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "work_claim_id":retry_claim_id,
                        "runtime_capability_witness_id":retry_witness_id,
                        "idempotency_key":Uuid::new_v4()
                    })
                    .to_string(),
                ))
                .expect("retry exact failed ToolCall"),
        )
        .await
        .expect("retry exact failed ToolCall response");
    assert_eq!(retry.status(), StatusCode::OK, "{}", json_body(retry).await);
    let retried_call = sqlx::query_as::<_, (String, i32, Uuid, i32)>(
        "SELECT current_status, current_attempt, work_claim_id, work_attempt
         FROM agent_tool_calls WHERE project_id=$1 AND id=$2",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact retry ToolCall");
    assert_eq!(retried_call, ("pending".into(), 2, retry_claim_id, 2));

    let late_attempt_one = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/terminal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "observation_id":Uuid::new_v4(),
                        "dispatch_id":dispatch_id,
                        "lease_id":lease_id,
                        "request_id":request_id,
                        "status":"failed",
                        "encrypted_output":null,
                        "canonical_output_commitment_hex":null,
                        "failure_code":"late_attempt_one",
                        "output_key_envelopes":[],
                        "signed_at":Utc::now(),
                        "idempotency_key":Uuid::new_v4(),
                        "signatures":signed_statement_by(
                            provisioned.principal_identity_id,
                            provisioned.runner_device_id,
                            &runner.ed25519_private_key,
                            &runner.ml_dsa_65_private_key,
                            &terminal_statement,
                            b"sprout-external-tool-observation-v1"
                        )
                    })
                    .to_string(),
                ))
                .expect("reject late attempt-one observation"),
        )
        .await
        .expect("reject late attempt-one observation response");
    assert!(
        matches!(
            late_attempt_one.status(),
            StatusCode::CONFLICT | StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST
        ),
        "{}",
        json_body(late_attempt_one).await
    );
    let still_retry = sqlx::query_as::<_, (String, i32, Uuid)>(
        "SELECT current_status, current_attempt, work_claim_id
         FROM agent_tool_calls WHERE project_id=$1 AND id=$2",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load ToolCall after late attempt-one rejection");
    assert_eq!(still_retry, ("pending".into(), 2, retry_claim_id));

    let retry_clock =
        sqlx::query_as::<_, (i64, i64, chrono::DateTime<Utc>, chrono::DateTime<Utc>, i64)>(
            "SELECT binding.requested_tick,binding.tool_deadline_tick,
                binding.requested_at,binding.tool_deadline_at,cursor.allocation_count
         FROM agent_tool_attempt_clock_bindings binding
         JOIN agent_run_semantic_tick_cursors cursor
           ON cursor.project_id=binding.project_id AND cursor.run_id=binding.run_id
         WHERE binding.project_id=$1 AND binding.call_id=$2 AND binding.attempt=2",
        )
        .bind(fixture.project_id)
        .bind(call_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("load retry semantic and operational deadlines");
    assert_eq!(retry_clock.1 - retry_clock.0, 3);
    assert_eq!(retry_clock.3 - retry_clock.2, ChronoDuration::seconds(3));

    // Pressure the same run with far more events than the remaining semantic
    // budget. Exactly the slots before the deadline may commit; every later
    // request rolls back its allocation until the trusted timeout worker
    // consumes the terminal slot.
    let mut rapid_successes = 0_i64;
    let mut rapid_conflicts = 0_i64;
    for ordinal in 20_u8..41 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/projects/{}/comments", fixture.project_id))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {}", fixture.owner_token))
                    .body(Body::from(
                        json!({
                            "recipient_id":provisioned.principal_identity_id,
                            "target_id":fixture.profile_resource_id,
                            "parent_id":null,
                            "encrypted_payload":{
                                "version":1,
                                "algorithm":"aes-256-gcm",
                                "key_id":format!("pending-tool-pressure-{ordinal}"),
                                "nonce_b64":STANDARD.encode([ordinal;12]),
                                "ciphertext_b64":STANDARD.encode([ordinal,ordinal.wrapping_add(1)])
                            },
                            "key_epoch":1,
                            "idempotency_key":Uuid::new_v4(),
                            "run_id":run_id
                        })
                        .to_string(),
                    ))
                    .expect("pressure pending ToolCall with canonical Comment"),
            )
            .await
            .expect("pending ToolCall pressure response");
        match response.status() {
            StatusCode::OK => rapid_successes += 1,
            StatusCode::CONFLICT => rapid_conflicts += 1,
            status => panic!("unexpected semantic deadline fence response: {status}"),
        }
    }
    assert_eq!((rapid_successes, rapid_conflicts), (2, 19));
    let fenced = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT call.current_status,cursor.last_tick,binding.tool_deadline_tick,
                cursor.allocation_count
         FROM agent_tool_calls call
         JOIN agent_tool_attempt_clock_bindings binding
           ON binding.project_id=call.project_id AND binding.call_id=call.id
          AND binding.attempt=call.current_attempt
         JOIN agent_run_semantic_tick_cursors cursor
           ON cursor.project_id=call.project_id AND cursor.run_id=call.run_id
         WHERE call.project_id=$1 AND call.id=$2",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify pending semantic timeout fence");
    assert_eq!(fenced.0, "pending");
    assert_eq!(fenced.1, fenced.2 - 1);
    assert_eq!(fenced.3, retry_clock.4 + rapid_successes);

    // The retry attempt is already pending even though the edge never claims
    // a dispatch. Its ToolCall deadline, not claim recovery, terminalizes the
    // exact attempt and preserves both historical outcomes.
    tokio::time::sleep(Duration::from_secs(4)).await;
    run_agent_completion_once(&fixture, config.clone()).await;
    let timed_out = sqlx::query_as::<_, (String, i32, String, i32, i64, i64, i64)>(
        "SELECT call.current_status, call.current_attempt,
                transition.state_snapshot #>> ARRAY['work_items', call.work_item_id::text, 'status'],
                (transition.state_snapshot #>> ARRAY['work_items', call.work_item_id::text, 'attempt'])::integer,
                (SELECT count(*) FROM agent_run_external_tool_work_outcomes history
                  WHERE history.project_id=call.project_id AND history.run_id=call.run_id
                    AND history.work_item_id=call.work_item_id),
                (SELECT count(*) FROM agent_tool_attempt_dispatches dispatch
                  WHERE dispatch.project_id=call.project_id AND dispatch.call_id=call.id
                    AND dispatch.attempt=2),
                transition.semantic_tick
         FROM agent_tool_calls call
         JOIN agent_run_external_tool_work_outcomes outcome
           ON outcome.project_id=call.project_id AND outcome.run_id=call.run_id
          AND outcome.work_item_id=call.work_item_id AND outcome.attempt=2
         JOIN agent_run_transitions transition
           ON transition.project_id=outcome.project_id AND transition.id=outcome.transition_id
         WHERE call.project_id=$1 AND call.id=$2",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact no-dispatch server timeout");
    assert_eq!(
        timed_out,
        (
            "timed_out".into(),
            2,
            "failed".into(),
            2,
            2,
            0,
            retry_clock.1
        )
    );

    // Attempt 3 reaches dispatch but no outbound request. Timeout retains the
    // dispatch provenance and does not invent a request witness.
    rearm_claim_and_retry_exact_tool(
        &fixture,
        &app,
        &runner,
        run_id,
        call_id,
        retry_witness_id,
        3,
    )
    .await;
    let (dispatch_three, _) =
        claim_exact_tool_dispatch(&fixture, &app, &runner, run_id, call_id).await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    run_agent_completion_once(&fixture, config.clone()).await;
    let timeout_three = sqlx::query_as::<_, (Option<Uuid>, Option<Uuid>, Option<Vec<u8>>, i64)>(
        "SELECT observation.dispatch_id, observation.request_id,
                observation.wire_request_commitment, outcome.attempt::bigint
         FROM agent_tool_attempt_observations observation
         JOIN agent_run_external_tool_work_outcomes outcome
           ON outcome.project_id=observation.project_id
          AND outcome.observation_id=observation.id
         WHERE observation.project_id=$1 AND observation.call_id=$2
           AND observation.attempt=3 AND observation.terminal_origin='server_timeout'",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load dispatch-without-request timeout");
    assert_eq!(timeout_three, (Some(dispatch_three), None, None, 3));

    // Attempt 4 persists the exact outbound witness before the edge vanishes;
    // server timeout preserves both request identity and wire commitment.
    rearm_claim_and_retry_exact_tool(
        &fixture,
        &app,
        &runner,
        run_id,
        call_id,
        retry_witness_id,
        4,
    )
    .await;
    let (dispatch_four, lease_four) =
        claim_exact_tool_dispatch(&fixture, &app, &runner, run_id, call_id).await;
    let profile_four = format!("{:02x}", 0x94_u8).repeat(32);
    let (request_four, wire_four) = record_exact_tool_request(
        &fixture,
        &app,
        &provisioned,
        &runner,
        run_id,
        call_id,
        dispatch_four,
        4,
        &canonical_input_commitment_hex,
        &profile_four,
    )
    .await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    run_agent_completion_once(&fixture, config).await;
    let timeout_four = sqlx::query_as::<_, (Uuid, Vec<u8>, i64)>(
        "SELECT observation.request_id, observation.wire_request_commitment,
                outcome.attempt::bigint
         FROM agent_tool_attempt_observations observation
         JOIN agent_run_external_tool_work_outcomes outcome
           ON outcome.project_id=observation.project_id
          AND outcome.observation_id=observation.id
         WHERE observation.project_id=$1 AND observation.call_id=$2
           AND observation.attempt=4 AND observation.terminal_origin='server_timeout'",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load request-preserving timeout");
    assert_eq!(
        timeout_four,
        (request_four, hex::decode(&wire_four).unwrap(), 4)
    );
    let late_four_id = Uuid::new_v4();
    let late_four_idempotency = Uuid::new_v4();
    let late_four_signed_at = Utc::now();
    let late_four_statement = json!({
        "signature_context":"sprout-external-tool-observation-v1",
        "project_id":fixture.project_id,
        "run_id":run_id,
        "call_id":call_id,
        "observation_id":late_four_id,
        "dispatch_id":dispatch_four,
        "lease_id":lease_four,
        "request_id":request_four,
        "attempt":4,
        "tool_id":"web.read",
        "tool_version":1,
        "adapter_protocol":"sprout-edge-web-read-v1",
        "canonical_input_commitment_hex":canonical_input_commitment_hex,
        "wire_request_commitment_hex":wire_four,
        "execution_profile_commitment_hex":profile_four,
        "status":"failed",
        "encrypted_output_payload_commitment_hex":null,
        "canonical_output_commitment_hex":null,
        "output_readable_by":[provisioned.principal_identity_id],
        "failure_code":"late_after_server_timeout",
        "output_key_envelopes":[],
        "signed_at":late_four_signed_at,
        "idempotency_key":late_four_idempotency
    });
    let late_four = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{run_id}/tool-calls/{call_id}/terminal",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from(
                    json!({
                        "observation_id":late_four_id,
                        "dispatch_id":dispatch_four,
                        "lease_id":lease_four,
                        "request_id":request_four,
                        "status":"failed",
                        "encrypted_output":null,
                        "canonical_output_commitment_hex":null,
                        "failure_code":"late_after_server_timeout",
                        "output_key_envelopes":[],
                        "signed_at":late_four_signed_at,
                        "idempotency_key":late_four_idempotency,
                        "signatures":signed_statement_by(
                            provisioned.principal_identity_id,
                            provisioned.runner_device_id,
                            &runner.ed25519_private_key,
                            &runner.ml_dsa_65_private_key,
                            &late_four_statement,
                            b"sprout-external-tool-observation-v1"
                        )
                    })
                    .to_string(),
                ))
                .expect("reject late result after exact server timeout"),
        )
        .await
        .expect("reject late result after exact server timeout response");
    assert_eq!(late_four.status(), StatusCode::FORBIDDEN);

    let exact_counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT
            (SELECT count(*) FROM agent_tool_calls WHERE project_id=$1 AND id=$2),
            (SELECT count(*) FROM agent_tool_attempt_dispatches WHERE project_id=$1 AND call_id=$2),
            (SELECT count(*) FROM agent_tool_attempt_requests WHERE project_id=$1 AND call_id=$2),
            (SELECT count(*) FROM agent_run_external_tool_work_outcomes WHERE project_id=$1 AND run_id=$3 AND work_item_id=$4)",
    )
    .bind(fixture.project_id)
    .bind(call_id)
    .bind(run_id)
    .bind(work_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count exact append-only attempt records");
    assert_eq!(exact_counts, (1, 3, 2, 4));

    let tool_timeline = sqlx::query_as::<_, (i64, i64, i64, i64, bool, i64)>(
        "SELECT
           (SELECT count(*) FROM agent_run_transitions
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(DISTINCT semantic_tick) FROM agent_run_transitions
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(*) FROM agent_run_semantic_tick_allocations
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(DISTINCT semantic_tick) FROM agent_run_semantic_tick_allocations
             WHERE project_id=$1 AND run_id=$2),
           EXISTS (SELECT 1 FROM agent_run_exact_semantic_timelines
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(*) FROM agent_run_transitions transition
             WHERE transition.project_id=$1 AND transition.run_id=$2
               AND transition.transition_kind <> 'initialized'
               AND NOT EXISTS (
                 SELECT 1 FROM agent_run_semantic_tick_allocations allocation
                 WHERE allocation.project_id=transition.project_id
                   AND allocation.run_id=transition.run_id
                   AND allocation.semantic_tick=transition.semantic_tick))",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify canonical tool-run semantic timeline");
    assert_eq!(tool_timeline.0, tool_timeline.1);
    assert_eq!(tool_timeline.2, tool_timeline.3);
    assert!(
        tool_timeline.4,
        "cross-family semantic timeline must be exact"
    );
    assert_eq!(
        tool_timeline.5, 0,
        "every post-init transition is allocated"
    );

    // R5.40 tool-cluster projection: one server-owned run trace, four exact
    // WorkAttempts, pending+terminal ToolEvents per attempt, and four exact
    // WorkOutcomes. The latest certificate stores the same ordered lists that
    // are independently rebuilt from the immutable ordinal inventory.
    let trace_exactness = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            i64,
            i32,
            i32,
            i32,
            i32,
            String,
            String,
            bool,
            bool,
        ),
    >(
        r#"
        SELECT root.trace_number, root.start_tick, initialization.semantic_tick,
               certificate.version,
               jsonb_array_length(certificate.work_attempt_inventory),
               jsonb_array_length(certificate.tool_event_inventory),
               jsonb_array_length(certificate.work_outcome_inventory),
               certificate.tool_gate_mode, certificate.outcome_gate_mode,
               certificate.work_attempt_inventory = actual.work_attempt_inventory
                 AND certificate.tool_event_inventory = actual.tool_event_inventory
                 AND certificate.work_outcome_inventory = actual.work_outcome_inventory,
               NOT EXISTS (
                 SELECT 1 FROM (
                   SELECT ordinal, row_number() OVER (ORDER BY ordinal) AS expected
                   FROM agent_r540_tool_trace_inventory
                   WHERE trace_number = root.trace_number
                 ) ordered WHERE ordered.ordinal <> ordered.expected
               )
        FROM agent_r540_tool_trace_roots root
        JOIN agent_run_transitions initialization
          ON initialization.id = root.initialization_transition_id
        JOIN agent_r540_exact_tool_trace_certificates certificate
          ON certificate.trace_number = root.trace_number
        JOIN agent_r540_tool_trace_inventory_state actual
          ON actual.trace_number = root.trace_number
        WHERE root.project_id=$1 AND root.run_id=$2
        "#,
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact ordered R540 tool trace certificate");
    assert!(trace_exactness.0 > 0);
    assert_eq!(trace_exactness.1, trace_exactness.2);
    assert_eq!(trace_exactness.3, 9); // initialization + 4 pending + 4 terminal prefixes
    assert_eq!(
        (trace_exactness.4, trace_exactness.5, trace_exactness.6),
        (4, 8, 4)
    );
    assert_eq!(
        (trace_exactness.7.as_str(), trace_exactness.8.as_str()),
        ("enabled", "enabled")
    );
    assert!(
        trace_exactness.9,
        "certificate inventories must be list-exact"
    );
    assert!(
        trace_exactness.10,
        "trace ordinals must be gap-free and total"
    );

    let projected_surface_counts = sqlx::query_as::<_, (i64, i64, String, String)>(
        "SELECT
           (SELECT count(*) FROM agent_r541_tool_surface_records
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(*) FROM agent_r541_tool_outcome_surface_records
             WHERE project_id=$1 AND run_id=$2),
           tool_mode, outcome_mode
         FROM agent_r541_tool_run_surface_gates WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load list-exact R5.41 tool and outcome gates");
    assert_eq!(
        projected_surface_counts,
        (8, 4, "enabled".into(), "enabled".into())
    );

    // The 0035 release inventory must independently reconstruct the complete
    // typed tool cluster before retention removes any operational payload or
    // provenance.  A non-empty operational ledger with an empty exact view is
    // not a certificate.
    let release_exactness =
        sqlx::query_as::<_, (i64, i64, i64, i64, i64, String, String, i32, i32, bool)>(
            "SELECT
           (SELECT count(*) FROM agent_r540_release_events
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(*) FROM agent_r540_typed_exact_release_events
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(*) FROM agent_r540_release_inventory inventory
             JOIN agent_r541_release_roots root USING (trace_number,project_id)
             WHERE root.project_id=$1 AND root.run_id=$2),
           (SELECT count(*) FROM agent_r540_release_certificates certificate
             JOIN agent_r541_release_roots root USING (trace_number,project_id)
             WHERE root.project_id=$1 AND root.run_id=$2),
           (SELECT count(*) FROM agent_r540_exact_release_trace_certificates certificate
             JOIN agent_r541_release_roots root USING (trace_number,project_id)
             WHERE root.project_id=$1 AND root.run_id=$2),
           tool_mode,outcome_mode,
           jsonb_array_length(tool_records),jsonb_array_length(outcome_records),
           blocker_mode='disabled_fail_closed'
             AND causal_mode='enabled'
             AND evidence_mode='disabled_fail_closed'
             AND disclosure_mode='disabled_fail_closed'
             AND model_mode='disabled_fail_closed'
             AND interrogation_mode='disabled_fail_closed'
             AND blocker_records='[]'::jsonb
             AND jsonb_array_length(causal_records)=1
             AND evidence_records='[]'::jsonb
             AND disclosure_records='[]'::jsonb
             AND model_records='[]'::jsonb
             AND interrogation_records='[]'::jsonb
         FROM agent_r541_release_trace_surface_gates
         WHERE project_id=$1 AND run_id=$2",
        )
        .bind(fixture.project_id)
        .bind(run_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("load full typed-exact 0035 tool-cluster inventory before retention");
    assert_eq!(release_exactness.0, 17);
    assert_eq!(release_exactness.1, release_exactness.0);
    assert_eq!(release_exactness.2, release_exactness.0);
    assert_eq!(release_exactness.3, 17);
    assert_eq!(release_exactness.4, 1);
    assert_eq!(
        (
            release_exactness.5.as_str(),
            release_exactness.6.as_str(),
            release_exactness.7,
            release_exactness.8,
            release_exactness.9,
        ),
        ("enabled", "enabled", 8, 4, true)
    );

    let timeout_shapes = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
           count(*) FILTER (WHERE terminal_origin='server_timeout'
             AND dispatch_id IS NULL AND request_id IS NULL),
           count(*) FILTER (WHERE terminal_origin='server_timeout'
             AND dispatch_id IS NOT NULL AND request_id IS NULL),
           count(*) FILTER (WHERE terminal_origin='server_timeout'
             AND dispatch_id IS NOT NULL AND request_id IS NOT NULL
             AND wire_request_commitment IS NOT NULL)
         FROM agent_r540_tool_attempt_events
         WHERE project_id=$1 AND run_id=$2 AND phase='terminal'",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact server-timeout trace shapes");
    assert_eq!(timeout_shapes, (1, 1, 1));

    let terminal_transitions_are_exact = sqlx::query_scalar::<_, bool>(
        "SELECT bool_and(
           outcome.status = transition.state_snapshot #>>
             ARRAY['work_items', outcome.work_item_id::text, 'status']
           AND outcome.attempt = (transition.state_snapshot #>>
             ARRAY['work_items', outcome.work_item_id::text, 'attempt'])::integer
           AND transition.transition_kind IN ('work_succeeded','work_failed')
         )
         FROM agent_r540_work_outcome_events outcome
         JOIN agent_run_transitions transition ON transition.id=outcome.transition_id
         WHERE outcome.project_id=$1 AND outcome.run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("verify failed/succeeded snapshots are not retry-rearm transitions");
    assert!(terminal_transitions_are_exact);

    // A second run proves the separate non-resource ToolOutput source path.
    let output_run = Uuid::new_v4();
    let create_output_run = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/agent-runs", fixture.project_id))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id":output_run,
                        "source":{"kind":"local_goal","id":provisioned.local_goal_id,"revision":1},
                        "authority_envelope":{"resource_authority":[],"tool_authority":["web.read"]}
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_output_run.status(), StatusCode::OK);
    let claim_output = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-runs/{output_run}/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", runner.bearer))
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(claim_output.status(), StatusCode::OK);
    let output_state = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agent-runs/{output_run}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {}", runner.bearer))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let (output_claim_text, output_claim) = output_state["state"]["claims"]
        .as_object()
        .and_then(|claims| claims.iter().find(|(_, claim)| claim["status"] == "active"))
        .expect("output producer claim");
    let output_claim_id = Uuid::parse_str(output_claim_text).unwrap();
    let output_work_id = Uuid::parse_str(output_claim["work"].as_str().unwrap()).unwrap();
    let output_goal_id = Uuid::parse_str(output_state["state"]["goal"].as_str().unwrap()).unwrap();
    let (output_witness, output_profile) = attest_exact_tool_runtime(
        &fixture,
        &app,
        &provisioned,
        &runner,
        &manifest_hash,
        0xa1,
        ChronoDuration::seconds(30),
    )
    .await;
    let output_call_id = Uuid::new_v4();
    let output_call_idempotency = Uuid::new_v4();
    let output_input = json!({
        "version":1,"algorithm":"aes-256-gcm","key_id":"output-source-input",
        "nonce_b64":"y8vLy8vLy8vLy8vL","ciphertext_b64":"ysw="
    });
    let output_input_typed: sprout_api_contract::EncryptedPayloadDto =
        serde_json::from_value(output_input.clone()).unwrap();
    let output_input_payload_commitment =
        sha256_hex(&canonical_governance_json(&output_input_typed).unwrap());
    let output_input_statement = json!({
        "signature_context":"sprout-external-tool-input-v1",
        "project_id":fixture.project_id,"run_id":output_run,"goal_id":output_goal_id,
        "work_item_id":output_work_id,"claim_id":output_claim_id,"attempt":1,
        "owner_identity_id":provisioned.principal_identity_id,
        "tool_id":"web.read","tool_version":1,
        "runtime_capability_witness_id":output_witness,
        "encrypted_input_payload_commitment_hex":output_input_payload_commitment,
        "structured_input_commitment_hex":"a2".repeat(32),
        "idempotency_key":output_call_idempotency
    });
    let output_invoke = app.clone().oneshot(
        Request::builder().method("POST")
            .uri(format!("/v1/projects/{}/agent-runs/{output_run}/claims/{output_claim_id}/tool-calls", fixture.project_id))
            .header(CONTENT_TYPE, "application/json")
            .header("authorization", format!("Bearer {}", runner.bearer))
            .body(Body::from(json!({
                "id":output_call_id,"tool_id":"web.read","tool_version":1,
                "runtime_capability_witness_id":output_witness,"encrypted_input":output_input,
                "structured_input_commitment_hex":"a2".repeat(32),"max_attempts":1,
                "timeout_seconds":10,"idempotency_key":output_call_idempotency,
                "signatures":signed_statement_by(
                    provisioned.principal_identity_id, provisioned.runner_device_id,
                    &runner.ed25519_private_key, &runner.ml_dsa_65_private_key,
                    &output_input_statement, b"sprout-external-tool-input-v1")
            }).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(
        output_invoke.status(),
        StatusCode::OK,
        "{}",
        json_body(output_invoke).await
    );
    let (output_dispatch, output_lease) =
        claim_exact_tool_dispatch(&fixture, &app, &runner, output_run, output_call_id).await;
    let output_input_commitment = canonical_hash_hex(&output_input_statement);
    let (output_request, output_wire) = record_exact_tool_request(
        &fixture,
        &app,
        &provisioned,
        &runner,
        output_run,
        output_call_id,
        output_dispatch,
        1,
        &output_input_commitment,
        &output_profile,
    )
    .await;

    let package_json = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT package_json FROM device_keys WHERE identity_id=$1 AND device_id=$2 AND key_version=1"
    ).bind(provisioned.principal_identity_id).bind(provisioned.runner_device_id)
        .fetch_one(&fixture.pool).await.unwrap();
    let package = DevicePublicPackage::from_json(&package_json).unwrap();
    let x25519 = package
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::X25519)
        .unwrap();
    let ml_kem = package
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::MlKem768Experimental)
        .unwrap();
    let output_key = ResourceKey::from_slice(&[0x5a; 32]).unwrap();
    let output_metadata = HybridWrapMetadata::new(
        output_call_id,
        provisioned.runner_device_id,
        1,
        hash_bytes(
            format!(
                "sprout-resource-key-genesis-v1/{}/{}",
                fixture.project_id, output_call_id
            )
            .as_bytes(),
        ),
        b"sprout-tool-output-test-v1".to_vec(),
    )
    .unwrap();
    let wrapped = wrap_resource_key(
        &output_key,
        &x25519.public_key,
        &ml_kem.public_key,
        output_metadata.clone(),
    )
    .unwrap();
    let wrapped_bytes = wrapped.to_bytes().unwrap();
    let parsed = ExperimentalWrappedResourceKey::from_bytes(&wrapped_bytes).unwrap();
    let opened = unwrap_resource_key(
        &parsed,
        &runner.x25519_private_key,
        &runner.ml_kem_768_private_key,
        &output_metadata,
    )
    .unwrap();
    assert_eq!(opened.as_bytes(), output_key.as_bytes());
    let envelope_id = Uuid::new_v4();
    let envelope_statement = json!({
        "signature_context":"sprout-tool-output-key-envelope-v1",
        "project_id":fixture.project_id,"call_id":output_call_id,"attempt":1,
        "envelope_version":2,"key_purpose":"tool_output",
        "recipient_identity_id":provisioned.principal_identity_id,
        "recipient_device_id":provisioned.runner_device_id,
        "recipient_device_key_version":1,
        "sender_identity_id":provisioned.principal_identity_id,
        "sender_device_id":provisioned.runner_device_id,"sender_device_key_version":1,
        "encrypted_key_commitment_hex":sha256_hex(&wrapped_bytes)
    });
    let envelope_signatures = sign_ed25519_ml_dsa65(
        &runner.ed25519_private_key,
        &runner.ml_dsa_65_private_key,
        &canonical_governance_json(&envelope_statement).unwrap(),
        b"sprout-tool-output-key-envelope-v1",
    )
    .unwrap();
    let output_envelope = json!({
        "id":envelope_id,"envelope_version":2,"key_purpose":"tool_output",
        "recipient_identity_id":provisioned.principal_identity_id,
        "recipient_device_id":provisioned.runner_device_id,"recipient_device_key_version":1,
        "sender_device_key_version":1,
        "encrypted_key_b64":STANDARD.encode(&wrapped_bytes),
        "sender_signature_b64":STANDARD.encode(envelope_signatures.ed25519()),
        "sender_post_quantum_signature_b64":STANDARD.encode(envelope_signatures.ml_dsa_65())
    });
    let output_payload = json!({
        "version":1,"algorithm":"aes-256-gcm","key_id":"tool-output-key",
        "nonce_b64":"zMzMzMzMzMzMzMzM","ciphertext_b64":"zc4="
    });
    let output_payload_typed: sprout_api_contract::EncryptedPayloadDto =
        serde_json::from_value(output_payload.clone()).unwrap();
    let output_payload_commitment =
        sha256_hex(&canonical_governance_json(&output_payload_typed).unwrap());
    let output_observation = Uuid::new_v4();
    let output_terminal_idempotency = Uuid::new_v4();
    let output_signed_at = Utc::now();
    let output_statement = json!({
        "signature_context":"sprout-external-tool-observation-v1",
        "project_id":fixture.project_id,"run_id":output_run,"call_id":output_call_id,
        "observation_id":output_observation,"dispatch_id":output_dispatch,"lease_id":output_lease,
        "request_id":output_request,"attempt":1,"tool_id":"web.read","tool_version":1,
        "adapter_protocol":"sprout-edge-web-read-v1",
        "canonical_input_commitment_hex":output_input_commitment,
        "wire_request_commitment_hex":output_wire,
        "execution_profile_commitment_hex":output_profile,"status":"succeeded",
        "encrypted_output_payload_commitment_hex":output_payload_commitment,
        "canonical_output_commitment_hex":"a3".repeat(32),
        "output_readable_by":[provisioned.principal_identity_id],"failure_code":null,
        "output_key_envelopes":[output_envelope],"signed_at":output_signed_at,
        "idempotency_key":output_terminal_idempotency
    });
    let output_terminal = app.clone().oneshot(
        Request::builder().method("POST")
            .uri(format!("/v1/projects/{}/agent-runs/{output_run}/tool-calls/{output_call_id}/terminal", fixture.project_id))
            .header(CONTENT_TYPE, "application/json")
            .header("authorization", format!("Bearer {}", runner.bearer))
            .body(Body::from(json!({
                "observation_id":output_observation,"dispatch_id":output_dispatch,"lease_id":output_lease,
                "request_id":output_request,"status":"succeeded","encrypted_output":output_payload,
                "canonical_output_commitment_hex":"a3".repeat(32),"failure_code":null,
                "output_key_envelopes":[output_envelope],"signed_at":output_signed_at,
                "idempotency_key":output_terminal_idempotency,
                "signatures":signed_statement_by(
                    provisioned.principal_identity_id, provisioned.runner_device_id,
                    &runner.ed25519_private_key, &runner.ml_dsa_65_private_key,
                    &output_statement, b"sprout-external-tool-observation-v1")
            }).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(
        output_terminal.status(),
        StatusCode::OK,
        "{}",
        json_body(output_terminal).await
    );
    let output_trace = sqlx::query_as::<_, (i64, i64, i64, String, String)>(
        "SELECT root.trace_number,
                jsonb_array_length(certificate.tool_event_inventory)::bigint,
                jsonb_array_length(certificate.work_outcome_inventory)::bigint,
                certificate.tool_gate_mode, certificate.outcome_gate_mode
         FROM agent_r540_tool_trace_roots root
         JOIN agent_r540_exact_tool_trace_certificates certificate
           ON certificate.trace_number=root.trace_number
         WHERE root.project_id=$1 AND root.run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(output_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load succeeded ToolOutput producer trace");
    assert!(output_trace.0 > 0);
    assert_eq!(output_trace.1, 2);
    assert_eq!(output_trace.2, 1);
    assert_eq!(
        (output_trace.3.as_str(), output_trace.4.as_str()),
        ("enabled", "enabled")
    );
    let mut corrupted_projection = fixture.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *corrupted_projection)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE agent_r540_tool_attempt_events SET owner_identity_id=$3
         WHERE project_id=$1 AND run_id=$2 AND phase='terminal'",
    )
    .bind(fixture.project_id)
    .bind(output_run)
    .bind(fixture.owner_id)
    .execute(&mut *corrupted_projection)
    .await
    .unwrap();
    let corrupted_gate = sqlx::query_as::<_, (String, Value, i64)>(
        "SELECT gate.tool_mode, gate.tool_records,
                (SELECT count(*) FROM agent_r541_tool_surface_records surface
                  WHERE surface.project_id=$1 AND surface.run_id=$2)
         FROM agent_r541_tool_run_surface_gates gate
         WHERE gate.project_id=$1 AND gate.run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(output_run)
    .fetch_one(&mut *corrupted_projection)
    .await
    .unwrap();
    assert_eq!(
        corrupted_gate,
        ("disabled_fail_closed".into(), json!([]), 0)
    );
    corrupted_projection.rollback().await.unwrap();

    let queue_tool_output = |source_call: Uuid| {
        json!({
            "id":Uuid::new_v4(),"local_goal_id":provisioned.local_goal_id,"local_goal_revision":1,
            "language_task":{
                "id":Uuid::new_v4(),"kind":"answer_from_authorized_context",
                "input_item_count":2,"max_input_items":2,"max_output_items":1,
                "max_nesting_depth":2,"max_attempts":1,"closed_output_schema":true,
                "grounded_identifiers_only":true,"requires_formal_proof":false,
                "requires_permission_decision":false,"requires_exact_semantic_equivalence":false,
                "requires_exhaustive_world_knowledge":false,
                "allowed_resource_ids":[fixture.profile_resource_id],
                "allowed_principal_ids":[provisioned.principal_identity_id],"allowed_tools":[]
            },
            "authority_envelope":{"resource_authority":[],"tool_authority":[]},
            "sources":[
                {"kind":"resource_body","resource_id":fixture.profile_resource_id},
                {"kind":"tool_output","call_id":source_call}
            ],
            "encrypted_input":encrypted(203)
        })
    };
    let valid_source = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/invocations",
                    fixture.project_id, provisioned.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(queue_tool_output(output_call_id).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        valid_source.status(),
        StatusCode::OK,
        "{}",
        json_body(valid_source).await
    );
    let wrong_call_source = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/invocations",
                    fixture.project_id, provisioned.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(queue_tool_output(Uuid::new_v4()).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_call_source.status(), StatusCode::FORBIDDEN);

    sqlx::raw_sql(
        "DO $role$ BEGIN
           IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='sprout_0034_trace_app') THEN
             CREATE ROLE sprout_0034_trace_app NOSUPERUSER NOBYPASSRLS NOLOGIN;
           END IF;
         END $role$;
         GRANT sprout_0034_trace_app TO CURRENT_USER;
         GRANT USAGE ON SCHEMA public, sprout_private TO sprout_0034_trace_app;
         GRANT SELECT, INSERT, UPDATE, DELETE ON
           agent_r540_tool_trace_roots, agent_r540_work_attempt_events,
           agent_r540_tool_attempt_events, agent_r540_work_outcome_events,
           agent_r540_tool_trace_inventory, agent_r540_tool_trace_certificates
         TO sprout_0034_trace_app",
    )
    .execute(&fixture.pool)
    .await
    .expect("prepare non-bypass trace application role");
    let before_structural_rows = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_r540_tool_trace_inventory WHERE project_id=$1",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let before_structural_hash =
        tool_trace_structural_hash(&fixture.pool, fixture.project_id).await;
    let mut untrusted = fixture.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE sprout_0034_trace_app")
        .execute(&mut *untrusted)
        .await
        .unwrap();
    sqlx::query(
        "SELECT set_config('app.identity_id',$1,true), set_config('app.project_id',$2,true)",
    )
    .bind(provisioned.principal_identity_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *untrusted)
    .await
    .unwrap();
    let direct_insert = sqlx::query(
        "INSERT INTO agent_r540_tool_trace_roots
           (project_id,run_id,goal_id,start_tick,initialization_transition_id,root_hash)
         VALUES ($1,$2,$3,0,$4,digest('forged','sha256'))",
    )
    .bind(fixture.project_id)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .execute(&mut *untrusted)
    .await;
    assert!(direct_insert.is_err());
    untrusted.rollback().await.unwrap();
    assert_eq!(
        tool_trace_structural_hash(&fixture.pool, fixture.project_id).await,
        before_structural_hash
    );

    let mut untrusted = fixture.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE sprout_0034_trace_app")
        .execute(&mut *untrusted)
        .await
        .unwrap();
    sqlx::query(
        "SELECT set_config('app.identity_id',$1,true), set_config('app.project_id',$2,true)",
    )
    .bind(provisioned.principal_identity_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *untrusted)
    .await
    .unwrap();
    let updated = sqlx::query(
        "UPDATE agent_r540_tool_trace_roots SET root_hash=root_hash
         WHERE project_id=$1 AND run_id=$2",
    )
    .bind(fixture.project_id)
    .bind(output_run)
    .execute(&mut *untrusted)
    .await;
    assert!(updated.is_err() || updated.unwrap().rows_affected() == 0);
    untrusted.rollback().await.unwrap();
    assert_eq!(
        tool_trace_structural_hash(&fixture.pool, fixture.project_id).await,
        before_structural_hash
    );

    let mut untrusted = fixture.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE sprout_0034_trace_app")
        .execute(&mut *untrusted)
        .await
        .unwrap();
    sqlx::query(
        "SELECT set_config('app.identity_id',$1,true), set_config('app.project_id',$2,true)",
    )
    .bind(provisioned.principal_identity_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *untrusted)
    .await
    .unwrap();
    let deleted = sqlx::query(
        "DELETE FROM agent_r540_tool_trace_inventory
         WHERE project_id=$1 AND trace_number=$2",
    )
    .bind(fixture.project_id)
    .bind(output_trace.0)
    .execute(&mut *untrusted)
    .await;
    assert!(deleted.is_err() || deleted.unwrap().rows_affected() == 0);
    let private_projector =
        sqlx::query("SELECT sprout_private.project_agent_tool_attempt($1,$2,$3,$4)")
            .bind(fixture.project_id)
            .bind(output_run)
            .bind(output_call_id)
            .bind(Uuid::new_v4())
            .execute(&mut *untrusted)
            .await;
    assert!(private_projector.is_err());
    untrusted.rollback().await.unwrap();
    assert_eq!(
        tool_trace_structural_hash(&fixture.pool, fixture.project_id).await,
        before_structural_hash
    );

    let mut forged_context = fixture.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE sprout_0034_trace_app")
        .execute(&mut *forged_context)
        .await
        .unwrap();
    sqlx::query(
        "SELECT set_config('app.identity_id',$1,true), set_config('app.project_id',$2,true)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(Uuid::new_v4().to_string())
    .execute(&mut *forged_context)
    .await
    .unwrap();
    let forged_guc_insert = sqlx::query(
        "INSERT INTO agent_r540_tool_trace_roots
           (project_id,run_id,goal_id,start_tick,initialization_transition_id,root_hash)
         VALUES ($1,$2,$3,0,$4,digest('forged-guc','sha256'))",
    )
    .bind(fixture.project_id)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .execute(&mut *forged_context)
    .await;
    assert!(forged_guc_insert.is_err());
    forged_context.rollback().await.unwrap();
    assert_eq!(
        tool_trace_structural_hash(&fixture.pool, fixture.project_id).await,
        before_structural_hash
    );

    let mut cross_project_context = fixture.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE sprout_0034_trace_app")
        .execute(&mut *cross_project_context)
        .await
        .unwrap();
    sqlx::query(
        "SELECT set_config('app.identity_id',$1,true), set_config('app.project_id',$2,true)",
    )
    .bind(provisioned.principal_identity_id.to_string())
    .bind(fixture.project_id.to_string())
    .execute(&mut *cross_project_context)
    .await
    .unwrap();
    let cross_project_projector =
        sqlx::query("SELECT sprout_private.project_agent_tool_attempt($1,$2,$3,$4)")
            .bind(Uuid::new_v4())
            .bind(output_run)
            .bind(output_call_id)
            .bind(Uuid::new_v4())
            .execute(&mut *cross_project_context)
            .await;
    assert!(cross_project_projector.is_err());
    cross_project_context.rollback().await.unwrap();
    assert_eq!(
        tool_trace_structural_hash(&fixture.pool, fixture.project_id).await,
        before_structural_hash
    );
    let after_structural_rows = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_r540_tool_trace_inventory WHERE project_id=$1",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(after_structural_rows, before_structural_rows);
    assert_eq!(
        tool_trace_structural_hash(&fixture.pool, fixture.project_id).await,
        before_structural_hash
    );

    let retained_tool_trace_before = sqlx::query_scalar::<_, String>(
        r#"
        SELECT jsonb_build_object(
          'root', (SELECT COALESCE(jsonb_agg(to_jsonb(root) - 'recorded_at'
                    ORDER BY root.trace_number), '[]'::jsonb)
                   FROM agent_r540_tool_trace_roots root
                   WHERE root.project_id=$1 AND root.run_id=$2),
          'work_attempts', (SELECT COALESCE(jsonb_agg(to_jsonb(event) - 'recorded_at'
                    ORDER BY event.id), '[]'::jsonb)
                   FROM agent_r540_work_attempt_events event
                   WHERE event.project_id=$1 AND event.run_id=$2),
          'tool_events', (SELECT COALESCE(jsonb_agg(to_jsonb(event) - 'recorded_at'
                    ORDER BY event.id), '[]'::jsonb)
                   FROM agent_r540_tool_attempt_events event
                   WHERE event.project_id=$1 AND event.run_id=$2),
          'work_outcomes', (SELECT COALESCE(jsonb_agg(to_jsonb(event) - 'recorded_at'
                    ORDER BY event.id), '[]'::jsonb)
                   FROM agent_r540_work_outcome_events event
                   WHERE event.project_id=$1 AND event.run_id=$2),
          'inventory', (SELECT COALESCE(jsonb_agg(to_jsonb(item) - 'recorded_at'
                    ORDER BY item.ordinal), '[]'::jsonb)
                   FROM agent_r540_tool_trace_inventory item
                   JOIN agent_r540_tool_trace_roots root
                     ON root.trace_number=item.trace_number
                   WHERE root.project_id=$1 AND root.run_id=$2),
          'certificates', (SELECT COALESCE(jsonb_agg(to_jsonb(certificate) - 'recorded_at'
                    ORDER BY certificate.version), '[]'::jsonb)
                   FROM agent_r540_tool_trace_certificates certificate
                   JOIN agent_r540_tool_trace_roots root
                     ON root.trace_number=certificate.trace_number
                   WHERE root.project_id=$1 AND root.run_id=$2)
        )::text
        "#,
    )
    .bind(fixture.project_id)
    .bind(output_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("snapshot complete structural tool trace before retention");
    let payloads_before = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        "SELECT
           (SELECT count(*) FROM agent_tool_calls
             WHERE project_id=$1 AND id=$2 AND encrypted_input IS NOT NULL),
           (SELECT count(*) FROM agent_tool_attempt_observations
             WHERE project_id=$1 AND call_id=$2 AND encrypted_output IS NOT NULL),
           (SELECT count(*) FROM agent_tool_output_key_envelopes
             WHERE project_id=$1 AND call_id=$2),
           (SELECT count(*) FROM agent_r541_tool_surface_records
             WHERE project_id=$1 AND run_id=$3),
           (SELECT count(*) FROM agent_r541_tool_outcome_surface_records
             WHERE project_id=$1 AND run_id=$3)",
    )
    .bind(fixture.project_id)
    .bind(output_call_id)
    .bind(output_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load payload and surface counts before retention");
    assert_eq!(payloads_before, (1, 1, 1, 2, 1));

    let retention_subject_id = Uuid::new_v4();
    let retention_lease_token = Uuid::new_v4();
    let retention_now = Utc::now();
    let deleted_at = retention_now - ChronoDuration::days(20);
    let mut retention = fixture.pool.begin().await.expect("begin tool retention");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *retention)
        .await
        .expect("disable RLS for retention fixture");
    sqlx::query("UPDATE resource_nodes SET deleted_at=$3 WHERE project_id=$1 AND id=$2")
        .bind(fixture.project_id)
        .bind(fixture.profile_resource_id)
        .bind(deleted_at)
        .execute(&mut *retention)
        .await
        .expect("soft-delete governed-agent profile for tool retention");
    sqlx::query(
        r#"
        INSERT INTO retention_subjects (
          id, project_id, source_kind, source_id, resource_node_id,
          owner_identity_id, retention_class, source_at, warning_at, purge_at,
          state, lease_owner, lease_token, leased_until
        ) VALUES ($1,$2,'resource_deleted',$3,$3,$4,'deleted_or_obsolete',
                  $5,$6,$7,'purging',$8,$9,$10)
        "#,
    )
    .bind(retention_subject_id)
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(fixture.owner_id)
    .bind(deleted_at)
    .bind(deleted_at + ChronoDuration::days(1))
    .bind(deleted_at + ChronoDuration::days(15))
    .bind(Uuid::new_v4())
    .bind(retention_lease_token)
    .bind(retention_now + ChronoDuration::hours(1))
    .execute(&mut *retention)
    .await
    .expect("insert active tool retention lease");
    retention
        .commit()
        .await
        .expect("commit tool retention setup");
    let purged =
        sqlx::query_scalar::<_, bool>("SELECT sprout_private.purge_retention_subject($1,$2,$3)")
            .bind(retention_subject_id)
            .bind(retention_lease_token)
            .bind(retention_now)
            .fetch_one(&fixture.pool)
            .await
            .expect("purge governed external-tool payloads");
    assert!(purged);

    let retained_tool_trace_after = sqlx::query_scalar::<_, String>(
        r#"
        SELECT jsonb_build_object(
          'root', (SELECT COALESCE(jsonb_agg(to_jsonb(root) - 'recorded_at'
                    ORDER BY root.trace_number), '[]'::jsonb)
                   FROM agent_r540_tool_trace_roots root
                   WHERE root.project_id=$1 AND root.run_id=$2),
          'work_attempts', (SELECT COALESCE(jsonb_agg(to_jsonb(event) - 'recorded_at'
                    ORDER BY event.id), '[]'::jsonb)
                   FROM agent_r540_work_attempt_events event
                   WHERE event.project_id=$1 AND event.run_id=$2),
          'tool_events', (SELECT COALESCE(jsonb_agg(to_jsonb(event) - 'recorded_at'
                    ORDER BY event.id), '[]'::jsonb)
                   FROM agent_r540_tool_attempt_events event
                   WHERE event.project_id=$1 AND event.run_id=$2),
          'work_outcomes', (SELECT COALESCE(jsonb_agg(to_jsonb(event) - 'recorded_at'
                    ORDER BY event.id), '[]'::jsonb)
                   FROM agent_r540_work_outcome_events event
                   WHERE event.project_id=$1 AND event.run_id=$2),
          'inventory', (SELECT COALESCE(jsonb_agg(to_jsonb(item) - 'recorded_at'
                    ORDER BY item.ordinal), '[]'::jsonb)
                   FROM agent_r540_tool_trace_inventory item
                   JOIN agent_r540_tool_trace_roots root
                     ON root.trace_number=item.trace_number
                   WHERE root.project_id=$1 AND root.run_id=$2),
          'certificates', (SELECT COALESCE(jsonb_agg(to_jsonb(certificate) - 'recorded_at'
                    ORDER BY certificate.version), '[]'::jsonb)
                   FROM agent_r540_tool_trace_certificates certificate
                   JOIN agent_r540_tool_trace_roots root
                     ON root.trace_number=certificate.trace_number
                   WHERE root.project_id=$1 AND root.run_id=$2)
        )::text
        "#,
    )
    .bind(fixture.project_id)
    .bind(output_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("snapshot complete structural tool trace after retention");
    assert_eq!(retained_tool_trace_after, retained_tool_trace_before);
    let payloads_after = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        "SELECT
           (SELECT count(*) FROM agent_tool_calls
             WHERE project_id=$1 AND id=$2 AND encrypted_input IS NOT NULL),
           (SELECT count(*) FROM agent_tool_attempt_observations
             WHERE project_id=$1 AND call_id=$2 AND encrypted_output IS NOT NULL),
           (SELECT count(*) FROM agent_tool_output_key_envelopes
             WHERE project_id=$1 AND call_id=$2),
           (SELECT count(*) FROM agent_r541_tool_surface_records
             WHERE project_id=$1 AND run_id=$3),
           (SELECT count(*) FROM agent_r541_tool_outcome_surface_records
             WHERE project_id=$1 AND run_id=$3)",
    )
    .bind(fixture.project_id)
    .bind(output_call_id)
    .bind(output_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("load payload and surface counts after retention");
    assert_eq!(payloads_after, (0, 0, 0, 0, 0));

    let unavailable_after_purge = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/invocations",
                    fixture.project_id, provisioned.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(queue_tool_output(output_call_id).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unavailable_after_purge.status(), StatusCode::NOT_FOUND);
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

struct LocalRevisionArtifact {
    prompt: Value,
    compilation: Value,
    local: Value,
    draft_id: Uuid,
    certificate_id: Uuid,
    prompt_commitment_hex: String,
    ciphertext_commitment_hex: String,
    output_hash_hex: String,
}

#[allow(clippy::too_many_arguments)]
async fn local_revision_artifact(
    fixture: &Fixture,
    created: &ProvisionedGovernanceAgent,
    controller: &SigningMember,
    authorization: Value,
    origin: LocalGoalOrigin,
    signer_identity_id: Uuid,
    signer_device_id: Uuid,
    signer_ed25519: &[u8],
    signer_ml_dsa: &[u8],
    replace_action_with_create_task: bool,
    seed: u8,
) -> LocalRevisionArtifact {
    let previous = sqlx::query(
        "SELECT certificate.canonical_output::text AS output,
                certificate.compilation_envelope::text AS envelope
         FROM agent_local_goal_contracts local
         JOIN agent_compilation_certificates certificate
           ON certificate.project_id=local.project_id
          AND certificate.id=local.compilation_certificate_id
         WHERE local.project_id=$1 AND local.agent_id=$2
           AND local.id=$3 AND local.revision=1 AND local.state='active'",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .bind(created.local_goal_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load initial compiled local goal");
    let mut output: Value = serde_json::from_str(previous.try_get("output").unwrap()).unwrap();
    let mut envelope: Value = serde_json::from_str(previous.try_get("envelope").unwrap()).unwrap();
    if replace_action_with_create_task {
        output["contract"]["work_specs"][0]["allowed_actions"] = json!(["create_task"]);
        output["requirements"][0]["required_actions"] = json!(["create_task"]);
        output["security_policies"][0]["allowed_operations"] = json!(["write"]);
        envelope["allowed_actions"] = json!(["create_task"]);
    }
    let draft_id = Uuid::new_v4();
    let certificate_id = Uuid::new_v4();
    let prompt = encrypted(seed);
    let typed_prompt: sprout_domain::EncryptedPayload =
        serde_json::from_value(prompt.clone()).unwrap();
    let ciphertext_commitment_hex =
        sha256_hex(&serde_json::to_vec(&typed_prompt).expect("serialize revision prompt"));
    let prompt_commitment_hex = hex::encode([seed; 32]);
    let output_hash_hex = canonical_hash_hex(&output);
    let statement = json!({
        "certificate_id": certificate_id,
        "compiler": {
            "compiler_id": "sprout.local-goal.compiler",
            "compiler_version": 1,
            "compiler_build_digest_hex": "0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2"
        },
        "project_id": fixture.project_id,
        "local_goal_id": created.local_goal_id,
        "local_revision": 2,
        "draft_id": draft_id,
        "agent_principal_identity_id": created.principal_identity_id,
        "controller_identity_id": controller.identity_id,
        "prompt_commitment_hex": prompt_commitment_hex,
        "ciphertext_commitment_hex": ciphertext_commitment_hex,
        "output": output,
        "output_hash_hex": output_hash_hex,
        "envelope": envelope,
        "envelope_hash_hex": canonical_hash_hex(&envelope),
        "authorization": authorization,
        "idempotency_key": Uuid::new_v4()
    });
    let typed_output: sprout_domain::LocalGoalCompilerOutput =
        serde_json::from_value(statement["output"].clone()).unwrap();
    let local = LocalGoalContract {
        id: created.local_goal_id.into(),
        revision: 2,
        agent: created.principal_identity_id.into(),
        controller: controller.identity_id.into(),
        encrypted_prompt: typed_prompt,
        contract: typed_output.contract.clone(),
        clauses: classify_local_goal_contract(&typed_output.contract),
        origin,
        supersedes_revision: Some(1),
    };
    LocalRevisionArtifact {
        prompt,
        compilation: json!({
            "statement": statement.clone(),
            "signatures": signed_statement_by(
                signer_identity_id,
                signer_device_id,
                signer_ed25519,
                signer_ml_dsa,
                &statement,
                b"sprout-governance-compilation-v1"
            )
        }),
        local: serde_json::to_value(local).unwrap(),
        draft_id,
        certificate_id,
        prompt_commitment_hex,
        ciphertext_commitment_hex,
        output_hash_hex,
    }
}

fn opaque_encrypted(seed: u8) -> Value {
    json!({
        "version": 1,
        "algorithm": "aes-256-gcm",
        "key_id": format!("opaque-{seed}"),
        "nonce_b64": STANDARD.encode([seed; 12]),
        "ciphertext_b64": STANDARD.encode([seed, seed.wrapping_add(1)])
    })
}

fn exact_final_prompt_approval(
    fixture: &Fixture,
    created: &ProvisionedGovernanceAgent,
    controller: &SigningMember,
    artifact: &LocalRevisionArtifact,
) -> Value {
    let approval_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();
    let approval_identity = json!({
        "signature_context":"sprout-final-prompt-approval-v1",
        "approval_id":approval_id,
        "project_id":fixture.project_id,
        "draft_id":artifact.draft_id,
        "agent_principal_identity_id":created.principal_identity_id,
        "controller_identity_id":controller.identity_id,
        "local_goal_id":created.local_goal_id,
        "local_revision":2,
        "prompt_commitment_hex":artifact.prompt_commitment_hex,
        "ciphertext_commitment_hex":artifact.ciphertext_commitment_hex,
        "compilation_certificate_id":artifact.certificate_id,
        "structured_output_hash_hex":artifact.output_hash_hex,
        "idempotency_key":idempotency_key
    });
    let statement = json!({
        "approval_id":approval_id,
        "project_id":fixture.project_id,
        "draft_id":artifact.draft_id,
        "agent_principal_identity_id":created.principal_identity_id,
        "controller_identity_id":controller.identity_id,
        "local_goal_id":created.local_goal_id,
        "local_revision":2,
        "prompt_commitment_hex":artifact.prompt_commitment_hex,
        "ciphertext_commitment_hex":artifact.ciphertext_commitment_hex,
        "compilation_certificate_id":artifact.certificate_id,
        "structured_output_hash_hex":artifact.output_hash_hex,
        "approval_identity_hash_hex":canonical_hash_hex(&approval_identity),
        "idempotency_key":idempotency_key
    });
    json!({
        "statement":statement,
        "signatures":signed_statement_by(
            controller.identity_id,
            controller.device_id,
            &controller.ed25519_private_key,
            &controller.ml_dsa_65_private_key,
            &statement,
            b"sprout-final-prompt-approval-v1"
        )
    })
}

fn resign_local_compilation(body: &mut Value, controller: &SigningMember) {
    let statement = body["compilation"]["statement"].clone();
    body["compilation"]["signatures"] = signed_statement_by(
        controller.identity_id,
        controller.device_id,
        &controller.ed25519_private_key,
        &controller.ml_dsa_65_private_key,
        &statement,
        b"sprout-governance-compilation-v1",
    );
}

async fn governance_review_task_fixture(
    fixture: &Fixture,
    controller: &SigningMember,
    agent_identity_id: Uuid,
    administrator_identity_id: Uuid,
) -> Value {
    let list_resource_id = Uuid::new_v4();
    let list_id = Uuid::new_v4();
    let topic_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM topics WHERE project_id=$1 AND resource_node_id=$2",
    )
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO resource_nodes
           (id,project_id,parent_id,node_kind,encrypted_metadata,created_by_identity_id)
         VALUES ($1,$2,$3,'task_list',decode('01','hex'),$4)",
    )
    .bind(list_resource_id)
    .bind(fixture.project_id)
    .bind(fixture.profile_resource_id)
    .bind(controller.identity_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO resource_epochs (
            project_id,resource_node_id,epoch,created_by_identity_id,
            created_by_device_id,created_by_device_key_version,key_commitment,reason
         ) VALUES ($1,$2,1,$3,$4,1,decode(repeat('ab',32),'hex'),'created')",
    )
    .bind(fixture.project_id)
    .bind(list_resource_id)
    .bind(controller.identity_id)
    .bind(controller.device_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_lists
           (id,project_id,topic_id,resource_node_id,encrypted_payload)
         VALUES ($1,$2,$3,$4,decode('01','hex'))",
    )
    .bind(list_id)
    .bind(fixture.project_id)
    .bind(topic_id)
    .bind(list_resource_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let task_id = Uuid::new_v4();
    let task_resource_id = Uuid::new_v4();
    let package_json = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT package_json FROM device_keys
         WHERE identity_id=$1 AND device_id=$2 AND key_version=1",
    )
    .bind(administrator_identity_id)
    .bind(fixture.owner_device_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let package = DevicePublicPackage::from_json(&package_json).unwrap();
    let x25519 = package
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::X25519)
        .unwrap();
    let ml_kem = package
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::MlKem768Experimental)
        .unwrap();
    let resource_key_bytes = [0x6b_u8; 32];
    let resource_key = ResourceKey::from_slice(&resource_key_bytes).unwrap();
    let previous_epoch_hash = hash_bytes(
        format!(
            "sprout-resource-key-genesis-v1/{}/{}",
            fixture.project_id, task_resource_id
        )
        .as_bytes(),
    );
    let wrapped = wrap_resource_key(
        &resource_key,
        &x25519.public_key,
        &ml_kem.public_key,
        HybridWrapMetadata::new(
            task_resource_id,
            fixture.owner_device_id,
            1,
            previous_epoch_hash,
            format!(
                "sprout/resource-envelope/v2/{}/{}/{}/{}",
                fixture.project_id,
                task_resource_id,
                administrator_identity_id,
                fixture.owner_device_id
            )
            .into_bytes(),
        )
        .unwrap(),
    )
    .unwrap()
    .to_bytes()
    .unwrap();
    let mut message = Vec::new();
    message.extend_from_slice(b"sprout-resource-key-envelope-v2");
    message.extend_from_slice(fixture.project_id.as_bytes());
    message.extend_from_slice(&1_i16.to_be_bytes());
    message.extend_from_slice(task_resource_id.as_bytes());
    message.extend_from_slice(&1_i32.to_be_bytes());
    message.extend_from_slice(administrator_identity_id.as_bytes());
    message.extend_from_slice(fixture.owner_device_id.as_bytes());
    message.extend_from_slice(&1_i32.to_be_bytes());
    message.extend_from_slice(&1_i32.to_be_bytes());
    message.extend_from_slice(&hash_bytes(&wrapped));
    let signatures = sign_ed25519_ml_dsa65(
        &controller.ed25519_private_key,
        &controller.ml_dsa_65_private_key,
        &message,
        b"sprout-resource-key-envelope-v2",
    )
    .unwrap();
    let mut commitment_input = Vec::new();
    commitment_input.extend_from_slice(b"sprout-resource-key-commitment-v1");
    commitment_input.extend_from_slice(fixture.project_id.as_bytes());
    commitment_input.extend_from_slice(task_resource_id.as_bytes());
    commitment_input.extend_from_slice(&resource_key_bytes);
    json!({
        "id": task_id,
        "list_id": list_id,
        "resource_node_id": task_resource_id,
        "task_kind": "priority",
        "payload": opaque_encrypted(151),
        "header": null,
        "selected_value_snapshot": opaque_encrypted(152),
        "questionnaire_version_id": null,
        "recurrence_series_id": null,
        "occurrence_number": null,
        "epoch": {
            "id": Uuid::new_v4(),
            "epoch": 1,
            "creator_device_key_version": 1,
            "key_commitment_b64": STANDARD.encode(hash_bytes(&commitment_input)),
            "header_key_commitment_b64": null
        },
        "envelopes": [{
            "version": 1,
            "resource_id": task_resource_id,
            "epoch": 1,
            "key_purpose": "body",
            "recipient_identity_id": administrator_identity_id,
            "recipient_device_id": fixture.owner_device_id,
            "recipient_device_key_version": 1,
            "sender_device_key_version": 1,
            "encrypted_key_b64": STANDARD.encode(wrapped),
            "sender_signature_b64": STANDARD.encode(signatures.ed25519()),
            "sender_post_quantum_signature_b64": STANDARD.encode(signatures.ml_dsa_65())
        }],
        "idempotency_key": Uuid::new_v4(),
        "agent_identity_id": agent_identity_id
    })
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
    for case in 0_u8..15 {
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
                13 => {
                    body["initial_local_goal"]["compilation"]["statement"]["authorization"] =
                        json!({"kind":"administrator_exception","id":Uuid::new_v4(),"revision":1});
                }
                14 => {
                    body["initial_local_goal"]["compilation"]["statement"]["authorization"] =
                        json!({"kind":"global_mandate","id":Uuid::new_v4(),"revision":1});
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

    let runner_key_ids = DeviceKeyIds {
        x25519: Uuid::new_v4(),
        ml_kem_768: Uuid::new_v4(),
        ed25519: Uuid::new_v4(),
        ml_dsa_65: Uuid::new_v4(),
    };
    let generated_runner =
        generate_experimental_device_package(runner_device_id, runner_key_ids.clone())
            .expect("generate runner endpoint-TCB signing package");
    let runner_public = generated_runner.public_package();
    let runner_public_key = |algorithm| {
        runner_public
            .encryption_keys
            .iter()
            .chain(&runner_public.signing_keys)
            .find(|key| key.algorithm == algorithm)
            .expect("runner endpoint-TCB public key")
            .public_key
            .clone()
    };
    let runner_x25519_public = runner_public_key(KeyAlgorithm::X25519);
    let runner_ml_kem_public = runner_public_key(KeyAlgorithm::MlKem768Experimental);
    let runner_ed25519_public = runner_public_key(KeyAlgorithm::Ed25519);
    let runner_ml_dsa_public = runner_public_key(KeyAlgorithm::MlDsa65Experimental);
    let runner_package_json = runner_public
        .to_canonical_json()
        .expect("serialize runner endpoint-TCB package");
    let runner_ed25519_private_key = generated_runner.private_keys().ed25519().to_vec();
    let runner_ml_dsa_65_private_key = generated_runner.private_keys().ml_dsa_65().to_vec();

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
    .bind(agent_identity_id)
    .bind(runner_device_id)
    .bind(&runner_x25519_public)
    .bind(&runner_ed25519_public)
    .bind(&runner_package_json)
    .bind(runner_key_ids.x25519)
    .bind(runner_key_ids.ml_kem_768)
    .bind(runner_key_ids.ed25519)
    .bind(runner_key_ids.ml_dsa_65)
    .bind(&runner_ml_kem_public)
    .bind(&runner_ml_dsa_public)
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

    // A real endpoint-TCB invocation answers the previously created
    // interrogation from an exact, ordered source list. The collaborative run
    // and foreign-runner checks above are unrelated project changes after the
    // question; they must not invalidate the causally read-only answer.
    let collaborative_claim_id =
        Uuid::parse_str(&collaborative_claim_id).expect("collaborative claim UUID");
    let (collaborative_goal_id, collaborative_work_item_id, collaborative_attempt) =
        sqlx::query_as::<_, (Uuid, Uuid, i32)>(
            r#"
            SELECT run.goal_id, claim.work_item_id, claim.attempt
            FROM agent_run_claim_leases claim
            JOIN agent_collaborative_runs run
              ON run.project_id = claim.project_id AND run.id = claim.run_id
            WHERE claim.project_id = $1 AND claim.id = $2
            "#,
        )
        .bind(fixture.project_id)
        .bind(collaborative_claim_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("load exact collaborative work binding");
    let model_invocation_id = Uuid::new_v4();
    let model_trace_id = Uuid::new_v4();
    let model_language_task = json!({
        "id": Uuid::new_v4(),
        "kind": "answer_from_authorized_context",
        "input_item_count": 2,
        "max_input_items": 2,
        "max_output_items": 1,
        "max_nesting_depth": 2,
        "max_attempts": 2,
        "closed_output_schema": true,
        "grounded_identifiers_only": true,
        "requires_formal_proof": false,
        "requires_permission_decision": false,
        "requires_exact_semantic_equivalence": false,
        "requires_exhaustive_world_knowledge": false,
        "allowed_resource_ids": [fixture.profile_resource_id],
        "allowed_principal_ids": [fixture.owner_id],
        "allowed_tools": []
    });
    let exact_context_sources = json!([
        {
            "kind": "resource_body",
            "resource_id": fixture.profile_resource_id
        },
        {
            "kind": "info_document",
            "resource_id": fixture.profile_resource_id,
            "document_id": fixture.info_document_id
        }
    ]);
    let failed_model_invocation_id = Uuid::new_v4();
    let queue_failed_model = app
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
                        "id": failed_model_invocation_id,
                        "local_goal_id": local_goal_id,
                        "local_goal_revision": 1,
                        "language_task": model_language_task,
                        "authority_envelope": {
                            "resource_authority": [],
                            "tool_authority": []
                        },
                        "sources": exact_context_sources,
                        "encrypted_input": encrypted(40),
                        "surface": "interrogation",
                        "interrogation_id": interrogation_id,
                        "work_binding": {
                            "trace_id": Uuid::new_v4(),
                            "run": collaborative_run_id,
                            "goal": collaborative_goal_id,
                            "work": collaborative_work_item_id,
                            "claim": collaborative_claim_id,
                            "attempt": collaborative_attempt
                        }
                    })
                    .to_string(),
                ))
                .expect("queue explicit-failure model invocation"),
        )
        .await
        .expect("queue explicit-failure model response");
    assert_eq!(
        queue_failed_model.status(),
        StatusCode::OK,
        "{}",
        json_body(queue_failed_model).await
    );
    let claim_failed_model = app
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
                .expect("claim explicit-failure model invocation"),
        )
        .await
        .expect("claim explicit-failure model response");
    assert_eq!(claim_failed_model.status(), StatusCode::OK);
    let claim_failed_model = json_body(claim_failed_model).await;
    let failed_observation_id = Uuid::new_v4();
    let failed_statement = json!({
        "observation_id": failed_observation_id,
        "dispatch_id": claim_failed_model["dispatch_id"],
        "invocation_id": failed_model_invocation_id,
        "attempt": claim_failed_model["attempt"],
        "lease_id": claim_failed_model["lease_id"],
        "principal_identity_id": fixture.owner_id,
        "exposed_sources": exact_context_sources,
        "request_commitment_hex": claim_failed_model["request_commitment_hex"],
        "context_commitment_hex": claim_failed_model["context_commitment_hex"],
        "transport_commitment_hex": claim_failed_model["transport_commitment_hex"],
        "output_commitment_hex": null,
        "artifact_commitment_hex": null,
        "provider_status": "invalid_structured_output",
        "hidden_persistent_model_memory_available": false,
        "idempotency_key": failed_observation_id,
        "observed_at": Utc::now()
    });
    let fail_model = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{failed_model_invocation_id}/fail",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "lease_id": claim_failed_model["lease_id"],
                        "failure_code": "invalid_structured_output",
                        "retryable": false,
                        "observation": {
                            "statement": failed_statement,
                            "signatures": signed_statement_by(
                                agent_identity_id,
                                runner_device_id,
                                &runner_ed25519_private_key,
                                &runner_ml_dsa_65_private_key,
                                &failed_statement,
                                b"sprout-model-runtime-observation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .expect("persist explicit-failure model observation"),
        )
        .await
        .expect("persist explicit-failure model response");
    assert_eq!(
        fail_model.status(),
        StatusCode::OK,
        "{}",
        json_body(fail_model).await
    );
    let legacy_endpoint_witness = sqlx::query_as::<_, (bool, Option<Vec<u8>>)>(
        "SELECT endpoint_request_exact, endpoint_request_commitment
         FROM agent_model_attempt_observations
         WHERE project_id = $1 AND id = $2",
    )
    .bind(fixture.project_id)
    .bind(failed_observation_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load legacy 0031 endpoint witness");
    assert!(!legacy_endpoint_witness.0);
    assert!(legacy_endpoint_witness.1.is_none());
    let mut failure_surface = fixture.pool.begin().await.expect("begin failure inventory");
    sqlx::query("SELECT set_config('app.identity_id', $1, true)")
        .bind(fixture.owner_id.to_string())
        .execute(&mut *failure_surface)
        .await
        .expect("set failure inventory identity");
    let failure_inventory = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT surface, mode, record_count
         FROM agent_r541_language_surface_inventory ORDER BY surface",
    )
    .fetch_all(&mut *failure_surface)
    .await
    .expect("load explicit-failure surface inventory");
    for surface_name in ["model", "interrogation", "proxy"] {
        assert!(failure_inventory.contains(&(
            surface_name.to_owned(),
            "disabled_fail_closed".to_owned(),
            0
        )));
    }
    failure_surface
        .rollback()
        .await
        .expect("rollback failure inventory");
    let queue_model = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/client-provider",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": model_invocation_id,
                        "local_goal_id": local_goal_id,
                        "local_goal_revision": 1,
                        "language_task": model_language_task,
                        "authority_envelope": {
                            "resource_authority": [],
                            "tool_authority": []
                        },
                        "sources": exact_context_sources,
                        "encrypted_input": encrypted(41),
                        "surface": "interrogation",
                        "interrogation_id": interrogation_id,
                        "work_binding": {
                            "trace_id": model_trace_id,
                            "run": collaborative_run_id,
                            "goal": collaborative_goal_id,
                            "work": collaborative_work_item_id,
                            "claim": collaborative_claim_id,
                            "attempt": collaborative_attempt
                        }
                    })
                    .to_string(),
                ))
                .expect("queue exact client-provider interrogation model invocation"),
        )
        .await
        .expect("queue exact interrogation model response");
    assert_eq!(
        queue_model.status(),
        StatusCode::OK,
        "{}",
        json_body(queue_model).await
    );
    let legacy_downgrade_claim = app
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
                .expect("attempt legacy claim for client-provider invocation"),
        )
        .await
        .expect("legacy downgrade claim response");
    assert_eq!(legacy_downgrade_claim.status(), StatusCode::OK);
    assert_eq!(json_body(legacy_downgrade_claim).await, Value::Null);
    let legacy_dispatch_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_model_attempt_dispatches
         WHERE project_id = $1 AND invocation_id = $2 AND runtime_kind = 'legacy_0031'",
    )
    .bind(fixture.project_id)
    .bind(model_invocation_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count legacy downgrade dispatches");
    assert_eq!(legacy_dispatch_count, 0);
    let claim_model = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/client-provider/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({ "execution_profile_commitment_hex": "66".repeat(32) }).to_string(),
                ))
                .expect("claim exact client-provider interrogation model invocation"),
        )
        .await
        .expect("claim exact interrogation model response");
    assert_eq!(claim_model.status(), StatusCode::OK);
    let claim_model = json_body(claim_model).await;
    let dispatch_id = claim_model["dispatch_id"].as_str().expect("dispatch id");
    let model_lease_id = claim_model["lease_id"].as_str().expect("model lease id");
    let model_attempt = claim_model["attempt"].as_i64().expect("model attempt");
    let model_output = encrypted(42);
    let grounded_output = json!({
        "items": [{
            "resource_id": fixture.profile_resource_id,
            "principal_id": fixture.owner_id,
            "tool": null,
            "action": null
        }],
        "max_observed_nesting_depth": 1
    });
    let interrogation_artifact = json!({
        "kind": "interrogation_answer",
        "session_id": interrogation_id,
        "encrypted_answer": model_output,
        "context_sources": exact_context_sources
    });
    let output_projection = json!({
        "structured_output": grounded_output,
        "encrypted_output": model_output,
        "effects": []
    });
    let output_commitment = sha256_hex(
        serde_json::to_string(&output_projection)
            .unwrap()
            .as_bytes(),
    );
    let artifact_commitment = canonical_hash_hex(&interrogation_artifact);
    let make_model_submit = |sources: Value, observation_id: Uuid| {
        let statement = json!({
            "observation_id": observation_id,
            "dispatch_id": dispatch_id,
            "invocation_id": model_invocation_id,
            "attempt": model_attempt,
            "lease_id": model_lease_id,
            "principal_identity_id": fixture.owner_id,
            "exposed_sources": sources,
            "request_commitment_hex": claim_model["request_commitment_hex"],
            "context_commitment_hex": claim_model["context_commitment_hex"],
            "transport_commitment_hex": claim_model["transport_commitment_hex"],
            "endpoint_request_commitment_hex": "44".repeat(32),
            "endpoint_request_exact": true,
            "runtime_kind": "client_provider_v1",
            "execution_profile_commitment_hex": "66".repeat(32),
            "output_commitment_hex": output_commitment,
            "artifact_commitment_hex": artifact_commitment,
            "provider_status": "schema_valid",
            "hidden_persistent_model_memory_available": false,
            "idempotency_key": observation_id,
            "observed_at": Utc::now()
        });
        json!({
            "lease_id": model_lease_id,
            "structured_output": grounded_output,
            "encrypted_output": model_output,
            "effects": [],
            "artifact": interrogation_artifact,
            "endpoint_request_commitment_hex": "44".repeat(32),
            "endpoint_request_exact": true,
            "runtime_kind": "client_provider_v1",
            "execution_profile_commitment_hex": "66".repeat(32),
            "observation": {
                "statement": statement,
                "signatures": signed_statement_by(
                    agent_identity_id,
                    runner_device_id,
                    &runner_ed25519_private_key,
                    &runner_ml_dsa_65_private_key,
                    &statement,
                    b"sprout-model-runtime-observation-v1"
                )
            }
        })
    };
    let exact_sources = exact_context_sources
        .as_array()
        .expect("exact context list")
        .clone();
    for forbidden_mutation in [
        "tool_invocations",
        "prompt_revisions",
        "local_goal_revisions",
        "created_work",
        "activated_obligations",
        "assigned_tasks",
    ] {
        let mut mutation_attempt = make_model_submit(exact_context_sources.clone(), Uuid::new_v4());
        mutation_attempt
            .as_object_mut()
            .expect("model submit object")
            .insert(forbidden_mutation.to_owned(), json!([Uuid::new_v4()]));
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                        fixture.project_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {runner_token}"))
                    .body(Body::from(mutation_attempt.to_string()))
                    .expect("forbidden interrogation mutation request"),
            )
            .await
            .expect("forbidden interrogation mutation response");
        assert_eq!(
            rejected.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "mutation category {forbidden_mutation} must be schema-closed"
        );
    }
    let mut resource_effect_attempt =
        make_model_submit(exact_context_sources.clone(), Uuid::new_v4());
    resource_effect_attempt["effects"] = json!([{
        "id": Uuid::new_v4(),
        "effect": {
            "resource_id": fixture.profile_resource_id,
            "operation": "post_comment"
        },
        "materialization": {
            "kind": "replace_info_document",
            "document_id": fixture.info_document_id,
            "expected_payload_version": 1,
            "key_epoch": 1,
            "idempotency_key": Uuid::new_v4(),
            "payload": encrypted(47)
        }
    }]);
    let resource_effect_rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(resource_effect_attempt.to_string()))
                .expect("interrogation resource-effect attempt"),
        )
        .await
        .expect("interrogation resource-effect response");
    assert_eq!(resource_effect_rejected.status(), StatusCode::BAD_REQUEST);
    let partial_interrogation_records = sqlx::query_scalar::<_, i64>(
        "SELECT
             (SELECT count(*) FROM agent_effect_proposals
              WHERE project_id = $1 AND invocation_id = $2)
           + (SELECT count(*) FROM agent_language_causal_mutations
              WHERE project_id = $1 AND invocation_id = $2)
           + (SELECT count(*) FROM agent_model_attempt_observations
              WHERE project_id = $1 AND invocation_id = $2)
           + (SELECT count(*) FROM agent_model_invocation_projections
              WHERE project_id = $1 AND invocation_id = $2)
           + (SELECT count(*) FROM agent_interrogation_answers
              WHERE project_id = $1 AND invocation_id = $2)",
    )
    .bind(fixture.project_id)
    .bind(model_invocation_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count rejected interrogation mutation residue");
    assert_eq!(partial_interrogation_records, 0);
    let mut grounded_effect_probe = fixture.pool.begin().await.expect("begin effect probe");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *grounded_effect_probe)
        .await
        .expect("enter trusted grounded-effect fixture boundary");
    sqlx::query("SELECT set_config('app.identity_id', $1, true)")
        .bind(fixture.owner_id.to_string())
        .execute(&mut *grounded_effect_probe)
        .await
        .expect("set grounded-effect identity");
    let grounded_effect_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_effect_proposals (
             id, project_id, invocation_id, agent_id, ordinal,
             effect, encrypted_materialization, proposal_hash
         ) VALUES ($1, $2, $3, $4, 99, $5::jsonb, NULL, $6)",
    )
    .bind(grounded_effect_id)
    .bind(fixture.project_id)
    .bind(model_invocation_id)
    .bind(agent_id)
    .bind(
        json!({
            "resource_id": fixture.profile_resource_id,
            "operation": "post_comment"
        })
        .to_string(),
    )
    .bind(Sha256::digest(grounded_effect_id.as_bytes()).as_slice())
    .execute(&mut *grounded_effect_probe)
    .await
    .expect("insert real authoritative effect record");
    sqlx::query(
        "INSERT INTO agent_language_causal_mutations (
             id, project_id, invocation_id, category, record_id
         ) VALUES ($1, $2, $3, 'resource_effect', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(fixture.project_id)
    .bind(model_invocation_id)
    .bind(grounded_effect_id)
    .execute(&mut *grounded_effect_probe)
    .await
    .expect("bind interrogation to real authoritative effect");
    let effect_read_only = sqlx::query_scalar::<_, bool>(
        "SELECT sprout_private.interrogation_invocation_is_read_only($1, $2)",
    )
    .bind(fixture.project_id)
    .bind(model_invocation_id)
    .fetch_one(&mut *grounded_effect_probe)
    .await
    .expect("evaluate grounded resource-effect delta");
    assert!(!effect_read_only);
    grounded_effect_probe
        .rollback()
        .await
        .expect("rollback grounded resource-effect probe");

    let mut nonexistent_effect_probe = fixture
        .pool
        .begin()
        .await
        .expect("begin missing effect probe");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *nonexistent_effect_probe)
        .await
        .expect("enter missing-effect fixture boundary");
    let missing_effect = sqlx::query(
        "INSERT INTO agent_language_causal_mutations (
             id, project_id, invocation_id, category, record_id
         ) VALUES ($1, $2, $3, 'resource_effect', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(fixture.project_id)
    .bind(model_invocation_id)
    .bind(Uuid::new_v4())
    .execute(&mut *nonexistent_effect_probe)
    .await;
    assert!(
        missing_effect.is_err(),
        "causal edge requires a real effect record"
    );
    nonexistent_effect_probe
        .rollback()
        .await
        .expect("rollback missing-effect probe");

    for category in [
        "tool_invocation",
        "prompt_revision",
        "local_goal_revision",
        "created_work",
        "activated_obligation",
        "assigned_task",
    ] {
        let mut causal_probe = fixture.pool.begin().await.expect("begin causal probe");
        sqlx::query("SET LOCAL row_security = off")
            .execute(&mut *causal_probe)
            .await
            .expect("enter trusted causal-probe fixture boundary");
        let unsupported = sqlx::query(
            "INSERT INTO agent_language_causal_mutations (
                 id, project_id, invocation_id, category, record_id
             ) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(fixture.project_id)
        .bind(model_invocation_id)
        .bind(category)
        .bind(Uuid::new_v4())
        .execute(&mut *causal_probe)
        .await;
        assert!(
            unsupported.is_err(),
            "structurally unreachable category {category} must have no DB writer"
        );
        causal_probe
            .rollback()
            .await
            .expect("rollback adversarial causal probe");
    }
    let adversarial_sources = [
        Value::Array(exact_sources.iter().cloned().rev().collect()),
        Value::Array(vec![exact_sources[0].clone()]),
        Value::Array(vec![
            exact_sources[0].clone(),
            exact_sources[1].clone(),
            json!({
                "kind": "resource_body",
                "resource_id": Uuid::new_v4()
            }),
        ]),
        Value::Array(vec![exact_sources[0].clone(), exact_sources[0].clone()]),
    ];
    for exposed_sources in adversarial_sources {
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                        fixture.project_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {runner_token}"))
                    .body(Body::from(
                        make_model_submit(exposed_sources, Uuid::new_v4()).to_string(),
                    ))
                    .expect("adversarial exact-source submit"),
            )
            .await
            .expect("adversarial exact-source response");
        assert_eq!(rejected.status(), StatusCode::CONFLICT);
    }
    let model_observation_id = Uuid::new_v4();
    let exact_submit_payload =
        make_model_submit(exact_context_sources.clone(), model_observation_id);
    let mut downgraded_client_provider = exact_submit_payload.clone();
    downgraded_client_provider["endpoint_request_commitment_hex"] = Value::Null;
    downgraded_client_provider["endpoint_request_exact"] = json!(false);
    downgraded_client_provider["runtime_kind"] = Value::Null;
    downgraded_client_provider["execution_profile_commitment_hex"] = Value::Null;
    downgraded_client_provider["observation"]["statement"]["endpoint_request_commitment_hex"] =
        Value::Null;
    downgraded_client_provider["observation"]["statement"]["endpoint_request_exact"] = json!(false);
    downgraded_client_provider["observation"]["statement"]["runtime_kind"] = Value::Null;
    downgraded_client_provider["observation"]["statement"]["execution_profile_commitment_hex"] =
        Value::Null;
    let downgraded_statement = downgraded_client_provider["observation"]["statement"].clone();
    downgraded_client_provider["observation"]["signatures"] = signed_statement_by(
        agent_identity_id,
        runner_device_id,
        &runner_ed25519_private_key,
        &runner_ml_dsa_65_private_key,
        &downgraded_statement,
        b"sprout-model-runtime-observation-v1",
    );
    let downgraded_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(downgraded_client_provider.to_string()))
                .expect("client-provider downgrade request"),
        )
        .await
        .expect("client-provider downgrade response");
    assert_eq!(downgraded_response.status(), StatusCode::CONFLICT);
    let mut mismatched_profile_projection = exact_submit_payload.clone();
    mismatched_profile_projection["execution_profile_commitment_hex"] = json!("77".repeat(32));
    let mismatched_profile_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(mismatched_profile_projection.to_string()))
                .expect("mismatched execution-profile projection request"),
        )
        .await
        .expect("mismatched execution-profile projection response");
    assert_eq!(mismatched_profile_response.status(), StatusCode::CONFLICT);
    let mut mismatched_endpoint_projection = exact_submit_payload.clone();
    mismatched_endpoint_projection["endpoint_request_commitment_hex"] = json!("55".repeat(32));
    let mismatched_endpoint_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(mismatched_endpoint_projection.to_string()))
                .expect("mismatched endpoint projection request"),
        )
        .await
        .expect("mismatched endpoint projection response");
    assert_eq!(mismatched_endpoint_response.status(), StatusCode::CONFLICT);
    let exact_submit = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(exact_submit_payload.to_string()))
                .expect("submit exact interrogation model observation"),
        )
        .await
        .expect("submit exact interrogation model response");
    assert_eq!(
        exact_submit.status(),
        StatusCode::OK,
        "response={} payload={}",
        json_body(exact_submit).await,
        exact_submit_payload
    );
    let exact_replay = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{model_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(exact_submit_payload.to_string()))
                .expect("replay exact model observation"),
        )
        .await
        .expect("replay exact model response");
    assert_eq!(exact_replay.status(), StatusCode::OK);
    let endpoint_commitments = sqlx::query_as::<_, (bool, Option<Vec<u8>>, bool, Option<Vec<u8>>)>(
        "SELECT observation.endpoint_request_exact,
                observation.endpoint_request_commitment,
                projection.endpoint_request_exact,
                projection.endpoint_request_commitment
         FROM agent_model_attempt_observations observation
         JOIN agent_model_invocation_projections projection
           ON projection.project_id = observation.project_id
          AND projection.observation_id = observation.id
         WHERE observation.project_id = $1 AND observation.id = $2",
    )
    .bind(fixture.project_id)
    .bind(model_observation_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact endpoint request commitment");
    assert!(endpoint_commitments.0);
    assert_eq!(endpoint_commitments.1, Some(vec![0x44; 32]));
    assert!(endpoint_commitments.2);
    assert_eq!(endpoint_commitments.3, Some(vec![0x44; 32]));
    let answered = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/projects/{}/agents/{agent_id}/interrogations/{interrogation_id}",
                        fixture.project_id
                    ))
                    .header("authorization", format!("Bearer {}", fixture.owner_token))
                    .body(Body::empty())
                    .expect("read answered interrogation"),
            )
            .await
            .expect("read answered interrogation response"),
    )
    .await;
    assert_eq!(answered["encrypted_answer"], model_output);
    let mut surface = fixture.pool.begin().await.expect("begin surface inventory");
    sqlx::query("SELECT set_config('app.identity_id', $1, true)")
        .bind(fixture.owner_id.to_string())
        .execute(&mut *surface)
        .await
        .expect("set surface inventory identity");
    let inventory = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT surface, mode, record_count
         FROM agent_r541_language_surface_inventory ORDER BY surface",
    )
    .fetch_all(&mut *surface)
    .await
    .expect("load exact R5.41 surface inventory");
    assert!(inventory.contains(&("model".to_owned(), "enabled".to_owned(), 1)));
    assert!(inventory.contains(&("interrogation".to_owned(), "enabled".to_owned(), 1)));
    assert!(inventory.contains(&("proxy".to_owned(), "disabled_fail_closed".to_owned(), 0)));
    surface
        .rollback()
        .await
        .expect("rollback surface inventory");

    // A client-provider retry is a new server-owned dispatch/attempt. The
    // first exact wire witness remains append-only when attempt two succeeds.
    let retry_invocation_id = Uuid::new_v4();
    let queue_retry = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/client-provider",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": retry_invocation_id,
                        "local_goal_id": local_goal_id,
                        "local_goal_revision": 1,
                        "language_task": model_language_task,
                        "authority_envelope": {
                            "resource_authority": [],
                            "tool_authority": []
                        },
                        "sources": exact_context_sources,
                        "encrypted_input": encrypted(43),
                        "surface": "generic"
                    })
                    .to_string(),
                ))
                .expect("queue client-provider retry lifecycle"),
        )
        .await
        .expect("queue client-provider retry response");
    assert_eq!(queue_retry.status(), StatusCode::OK);

    let claim_retry_one = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/client-provider/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({ "execution_profile_commitment_hex": "aa".repeat(32) }).to_string(),
                ))
                .expect("claim client-provider attempt one"),
        )
        .await
        .expect("claim client-provider attempt one response");
    assert_eq!(claim_retry_one.status(), StatusCode::OK);
    let claim_retry_one = json_body(claim_retry_one).await;
    assert_eq!(claim_retry_one["attempt"], 1);
    let missing_witness_id = Uuid::new_v4();
    let missing_witness_statement = json!({
        "observation_id": missing_witness_id,
        "dispatch_id": claim_retry_one["dispatch_id"],
        "invocation_id": retry_invocation_id,
        "attempt": 1,
        "lease_id": claim_retry_one["lease_id"],
        "principal_identity_id": claim_retry_one["context_principal_identity_id"],
        "exposed_sources": exact_context_sources,
        "request_commitment_hex": claim_retry_one["request_commitment_hex"],
        "context_commitment_hex": claim_retry_one["context_commitment_hex"],
        "transport_commitment_hex": claim_retry_one["transport_commitment_hex"],
        "endpoint_request_exact": false,
        "runtime_kind": "client_provider_v1",
        "execution_profile_commitment_hex": "aa".repeat(32),
        "output_commitment_hex": null,
        "artifact_commitment_hex": null,
        "provider_status": "provider_timeout",
        "hidden_persistent_model_memory_available": false,
        "idempotency_key": missing_witness_id,
        "observed_at": Utc::now()
    });
    let missing_witness_failure = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{retry_invocation_id}/fail",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "lease_id": claim_retry_one["lease_id"],
                        "failure_code": "provider_timeout",
                        "retryable": true,
                        "endpoint_request_exact": false,
                        "runtime_kind": "client_provider_v1",
                        "execution_profile_commitment_hex": "aa".repeat(32),
                        "observation": {
                            "statement": missing_witness_statement,
                            "signatures": signed_statement_by(
                                agent_identity_id,
                                runner_device_id,
                                &runner_ed25519_private_key,
                                &runner_ml_dsa_65_private_key,
                                &missing_witness_statement,
                                b"sprout-model-runtime-observation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .expect("reject post-request failure without exact wire witness"),
        )
        .await
        .expect("missing wire witness response");
    assert_eq!(missing_witness_failure.status(), StatusCode::CONFLICT);
    let missing_witness_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_model_attempt_observations
         WHERE project_id = $1 AND invocation_id = $2",
    )
    .bind(fixture.project_id)
    .bind(retry_invocation_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count rejected attempt observations");
    assert_eq!(missing_witness_rows, 0);
    let failed_attempt_id = Uuid::new_v4();
    let failed_attempt_statement = json!({
        "observation_id": failed_attempt_id,
        "dispatch_id": claim_retry_one["dispatch_id"],
        "invocation_id": retry_invocation_id,
        "attempt": 1,
        "lease_id": claim_retry_one["lease_id"],
        "principal_identity_id": claim_retry_one["context_principal_identity_id"],
        "exposed_sources": exact_context_sources,
        "request_commitment_hex": claim_retry_one["request_commitment_hex"],
        "context_commitment_hex": claim_retry_one["context_commitment_hex"],
        "transport_commitment_hex": claim_retry_one["transport_commitment_hex"],
        "endpoint_request_commitment_hex": "a1".repeat(32),
        "endpoint_request_exact": true,
        "runtime_kind": "client_provider_v1",
        "execution_profile_commitment_hex": "aa".repeat(32),
        "output_commitment_hex": null,
        "artifact_commitment_hex": null,
        "provider_status": "provider_timeout",
        "hidden_persistent_model_memory_available": false,
        "idempotency_key": failed_attempt_id,
        "observed_at": Utc::now()
    });
    let fail_attempt_one = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{retry_invocation_id}/fail",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "lease_id": claim_retry_one["lease_id"],
                        "failure_code": "provider_timeout",
                        "retryable": true,
                        "endpoint_request_commitment_hex": "a1".repeat(32),
                        "endpoint_request_exact": true,
                        "runtime_kind": "client_provider_v1",
                        "execution_profile_commitment_hex": "aa".repeat(32),
                        "observation": {
                            "statement": failed_attempt_statement,
                            "signatures": signed_statement_by(
                                agent_identity_id,
                                runner_device_id,
                                &runner_ed25519_private_key,
                                &runner_ml_dsa_65_private_key,
                                &failed_attempt_statement,
                                b"sprout-model-runtime-observation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .expect("persist client-provider timeout attempt"),
        )
        .await
        .expect("persist client-provider timeout response");
    let fail_attempt_one_status = fail_attempt_one.status();
    let fail_attempt_one_body = json_body(fail_attempt_one).await;
    assert_eq!(
        fail_attempt_one_status,
        StatusCode::OK,
        "{fail_attempt_one_body}"
    );
    assert_eq!(fail_attempt_one_body["status"], "pending");

    let claim_retry_two = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/client-provider/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({ "execution_profile_commitment_hex": "aa".repeat(32) }).to_string(),
                ))
                .expect("claim client-provider attempt two"),
        )
        .await
        .expect("claim client-provider attempt two response");
    assert_eq!(claim_retry_two.status(), StatusCode::OK);
    let claim_retry_two = json_body(claim_retry_two).await;
    assert_eq!(claim_retry_two["attempt"], 2);
    let retry_output = encrypted(44);
    let retry_structured_output = json!({
        "items": [],
        "max_observed_nesting_depth": 1
    });
    let retry_artifact = json!({
        "kind": "grounded_output",
        "output": retry_structured_output
    });
    let retry_output_projection = json!({
        "structured_output": retry_structured_output,
        "encrypted_output": retry_output,
        "effects": []
    });
    let retry_output_commitment = sha256_hex(
        serde_json::to_string(&retry_output_projection)
            .unwrap()
            .as_bytes(),
    );
    let retry_artifact_commitment = canonical_hash_hex(&retry_artifact);
    let success_attempt_id = Uuid::new_v4();
    let success_attempt_statement = json!({
        "observation_id": success_attempt_id,
        "dispatch_id": claim_retry_two["dispatch_id"],
        "invocation_id": retry_invocation_id,
        "attempt": 2,
        "lease_id": claim_retry_two["lease_id"],
        "principal_identity_id": claim_retry_two["context_principal_identity_id"],
        "exposed_sources": exact_context_sources,
        "request_commitment_hex": claim_retry_two["request_commitment_hex"],
        "context_commitment_hex": claim_retry_two["context_commitment_hex"],
        "transport_commitment_hex": claim_retry_two["transport_commitment_hex"],
        "endpoint_request_commitment_hex": "a2".repeat(32),
        "endpoint_request_exact": true,
        "runtime_kind": "client_provider_v1",
        "execution_profile_commitment_hex": "aa".repeat(32),
        "output_commitment_hex": retry_output_commitment,
        "artifact_commitment_hex": retry_artifact_commitment,
        "provider_status": "schema_valid",
        "hidden_persistent_model_memory_available": false,
        "idempotency_key": success_attempt_id,
        "observed_at": Utc::now()
    });
    let success_attempt_payload = json!({
        "lease_id": claim_retry_two["lease_id"],
        "structured_output": retry_structured_output,
        "encrypted_output": retry_output,
        "effects": [],
        "artifact": retry_artifact,
        "endpoint_request_commitment_hex": "a2".repeat(32),
        "endpoint_request_exact": true,
        "runtime_kind": "client_provider_v1",
        "execution_profile_commitment_hex": "aa".repeat(32),
        "observation": {
            "statement": success_attempt_statement,
            "signatures": signed_statement_by(
                agent_identity_id,
                runner_device_id,
                &runner_ed25519_private_key,
                &runner_ml_dsa_65_private_key,
                &success_attempt_statement,
                b"sprout-model-runtime-observation-v1"
            )
        }
    });
    let success_attempt_two = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{retry_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(success_attempt_payload.to_string()))
                .expect("submit client-provider attempt two"),
        )
        .await
        .expect("submit client-provider attempt two response");
    assert_eq!(success_attempt_two.status(), StatusCode::OK);
    let retry_observations = sqlx::query_as::<_, (i32, String, Vec<u8>)>(
        "SELECT attempt, status, endpoint_request_commitment
         FROM agent_model_attempt_observations
         WHERE project_id = $1 AND invocation_id = $2 ORDER BY attempt",
    )
    .bind(fixture.project_id)
    .bind(retry_invocation_id)
    .fetch_all(&fixture.pool)
    .await
    .expect("load exact client-provider retry observations");
    assert_eq!(
        retry_observations,
        vec![
            (1, "explicit_failure".to_owned(), vec![0xa1; 32]),
            (2, "succeeded".to_owned(), vec![0xa2; 32]),
        ]
    );
    let success_attempt_replay = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{retry_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(success_attempt_payload.to_string()))
                .expect("replay client-provider attempt two"),
        )
        .await
        .expect("replay client-provider attempt two response");
    assert_eq!(success_attempt_replay.status(), StatusCode::OK);
    let retry_observation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_model_attempt_observations
         WHERE project_id = $1 AND invocation_id = $2",
    )
    .bind(fixture.project_id)
    .bind(retry_invocation_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count replay-stable client-provider observations");
    assert_eq!(retry_observation_count, 2);

    let auth_failure_invocation_id = Uuid::new_v4();
    let queue_auth_failure = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/client-provider",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": auth_failure_invocation_id,
                        "local_goal_id": local_goal_id,
                        "local_goal_revision": 1,
                        "language_task": model_language_task,
                        "authority_envelope": {
                            "resource_authority": [],
                            "tool_authority": []
                        },
                        "sources": exact_context_sources,
                        "encrypted_input": encrypted(45),
                        "surface": "generic"
                    })
                    .to_string(),
                ))
                .expect("queue non-retryable client-provider invocation"),
        )
        .await
        .expect("queue non-retryable client-provider response");
    assert_eq!(queue_auth_failure.status(), StatusCode::OK);
    let claim_auth_failure = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/client-provider/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({ "execution_profile_commitment_hex": "ab".repeat(32) }).to_string(),
                ))
                .expect("claim non-retryable client-provider invocation"),
        )
        .await
        .expect("claim non-retryable client-provider response");
    assert_eq!(claim_auth_failure.status(), StatusCode::OK);
    let claim_auth_failure = json_body(claim_auth_failure).await;
    let auth_failure_observation_id = Uuid::new_v4();
    let auth_failure_statement = json!({
        "observation_id": auth_failure_observation_id,
        "dispatch_id": claim_auth_failure["dispatch_id"],
        "invocation_id": auth_failure_invocation_id,
        "attempt": 1,
        "lease_id": claim_auth_failure["lease_id"],
        "principal_identity_id": claim_auth_failure["context_principal_identity_id"],
        "exposed_sources": exact_context_sources,
        "request_commitment_hex": claim_auth_failure["request_commitment_hex"],
        "context_commitment_hex": claim_auth_failure["context_commitment_hex"],
        "transport_commitment_hex": claim_auth_failure["transport_commitment_hex"],
        "endpoint_request_commitment_hex": "af".repeat(32),
        "endpoint_request_exact": true,
        "runtime_kind": "client_provider_v1",
        "execution_profile_commitment_hex": "ab".repeat(32),
        "output_commitment_hex": null,
        "artifact_commitment_hex": null,
        "provider_status": "provider_unavailable",
        "hidden_persistent_model_memory_available": false,
        "idempotency_key": auth_failure_observation_id,
        "observed_at": Utc::now()
    });
    let fail_auth = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{auth_failure_invocation_id}/fail",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "lease_id": claim_auth_failure["lease_id"],
                        "failure_code": "provider_unavailable",
                        "retryable": false,
                        "endpoint_request_commitment_hex": "af".repeat(32),
                        "endpoint_request_exact": true,
                        "runtime_kind": "client_provider_v1",
                        "execution_profile_commitment_hex": "ab".repeat(32),
                        "observation": {
                            "statement": auth_failure_statement,
                            "signatures": signed_statement_by(
                                agent_identity_id,
                                runner_device_id,
                                &runner_ed25519_private_key,
                                &runner_ml_dsa_65_private_key,
                                &auth_failure_statement,
                                b"sprout-model-runtime-observation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .expect("persist non-retryable provider auth failure"),
        )
        .await
        .expect("persist non-retryable provider auth response");
    assert_eq!(fail_auth.status(), StatusCode::OK);
    assert_eq!(json_body(fail_auth).await["status"], "failed");
    let non_retryable_state = sqlx::query_as::<_, (String, i32, i64)>(
        "SELECT invocation.status, invocation.attempt,
                (SELECT count(*) FROM agent_model_attempt_observations observation
                 WHERE observation.project_id = invocation.project_id
                   AND observation.invocation_id = invocation.id)
         FROM agent_invocations invocation
         WHERE invocation.project_id = $1 AND invocation.id = $2",
    )
    .bind(fixture.project_id)
    .bind(auth_failure_invocation_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load non-retryable provider attempt state");
    assert_eq!(non_retryable_state, ("failed".to_owned(), 1, 1));
    let no_auth_retry = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/runner/client-provider/claim",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({ "execution_profile_commitment_hex": "ab".repeat(32) }).to_string(),
                ))
                .expect("attempt claim after non-retryable failure"),
        )
        .await
        .expect("claim after non-retryable failure response");
    assert_eq!(no_auth_retry.status(), StatusCode::OK);
    assert_eq!(json_body(no_auth_retry).await, Value::Null);

    // A legacy/deterministic proxy plan remains supported, but only an exact
    // succeeded model projection may enable the R5.41 model-mediated proxy
    // surface. Use the owner's already-authorized agent runner so the endpoint
    // TCB and the user actor remain distinct.
    let model_proxy_id = Uuid::new_v4();
    let model_proxy_thread_id = Uuid::new_v4();
    let create_model_proxy_thread = app
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
                        "proxy_id": model_proxy_id,
                        "thread_id": model_proxy_thread_id
                    })
                    .to_string(),
                ))
                .expect("create model-mediated proxy thread request"),
        )
        .await
        .expect("create model-mediated proxy thread response");
    assert_eq!(create_model_proxy_thread.status(), StatusCode::OK);
    let model_proxy_request_id = Uuid::new_v4();
    let create_model_proxy_request = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/user-proxy/threads/{model_proxy_thread_id}/requests",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": model_proxy_request_id,
                        "encrypted_payload": encrypted(43)
                    })
                    .to_string(),
                ))
                .expect("create model-mediated proxy request"),
        )
        .await
        .expect("create model-mediated proxy response");
    assert_eq!(create_model_proxy_request.status(), StatusCode::OK);

    let model_proxy_envelope = json!({
        "language_task": {
            "id": Uuid::new_v4(),
            "kind": "interpret_proxy_request",
            "input_item_count": 1,
            "max_input_items": 1,
            "max_output_items": 1,
            "max_nesting_depth": 1,
            "max_attempts": 2,
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
        "request_id": model_proxy_request_id,
        "user": fixture.owner_id,
        "candidate_resources": [fixture.profile_resource_id],
        "candidate_operations": ["post_comment"],
        "available_tools": [],
        "max_plan_steps": 1
    });
    let model_proxy_plan = json!({
        "request_id": model_proxy_request_id,
        "thread_id": model_proxy_thread_id,
        "user": fixture.owner_id,
        "intent_id": Uuid::new_v4(),
        "resource_effects": [{
            "resource_id": fixture.profile_resource_id,
            "operation": "post_comment"
        }],
        "tool_invocations": [],
        "encrypted_explanation": encrypted(44)
    });
    let interrogation_as_proxy_witness = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/user-proxy/requests/{model_proxy_request_id}/plan",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": Uuid::new_v4(),
                        "invocation_id": model_invocation_id,
                        "envelope": model_proxy_envelope,
                        "plan": model_proxy_plan,
                        "confirmation": {
                            "user": fixture.owner_id,
                            "thread_id": model_proxy_thread_id,
                            "request_id": model_proxy_request_id,
                            "accepted_plan": model_proxy_plan,
                            "summary_id": Uuid::new_v4(),
                            "confirmed_at": Utc::now()
                        }
                    })
                    .to_string(),
                ))
                .expect("reuse interrogation as proxy witness request"),
        )
        .await
        .expect("reuse interrogation as proxy witness response");
    assert_eq!(
        interrogation_as_proxy_witness.status(),
        StatusCode::CONFLICT
    );
    let forged_proxy_residue = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM user_proxy_plans
         WHERE project_id = $1 AND request_id = $2",
    )
    .bind(fixture.project_id)
    .bind(model_proxy_request_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("count rejected cross-surface proxy residue");
    assert_eq!(forged_proxy_residue, 0);
    let proxy_model_invocation_id = Uuid::new_v4();
    let queue_proxy_model = app
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
                        "id": proxy_model_invocation_id,
                        "local_goal_id": local_goal_id,
                        "local_goal_revision": 1,
                        "language_task": model_proxy_envelope["language_task"],
                        "authority_envelope": {
                            "resource_authority": [],
                            "tool_authority": []
                        },
                        "sources": [{
                            "kind": "resource_body",
                            "resource_id": fixture.profile_resource_id
                        }],
                        "encrypted_input": encrypted(45),
                        "surface": "user_proxy",
                        "proxy_request_id": model_proxy_request_id,
                        "interrogation_id": null,
                        "work_binding": null
                    })
                    .to_string(),
                ))
                .expect("queue model-mediated proxy invocation"),
        )
        .await
        .expect("queue model-mediated proxy response");
    assert_eq!(
        queue_proxy_model.status(),
        StatusCode::OK,
        "{}",
        json_body(queue_proxy_model).await
    );
    let claim_proxy_model = app
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
                .expect("claim model-mediated proxy invocation"),
        )
        .await
        .expect("claim model-mediated proxy response");
    assert_eq!(claim_proxy_model.status(), StatusCode::OK);
    let claim_proxy_model = json_body(claim_proxy_model).await;
    let proxy_model_lease_id = claim_proxy_model["lease_id"]
        .as_str()
        .expect("proxy model lease id");
    let proxy_model_output = encrypted(46);
    let proxy_grounded_output = json!({
        "items": [{
            "resource_id": fixture.profile_resource_id,
            "principal_id": fixture.owner_id,
            "tool": null,
            "action": null
        }],
        "max_observed_nesting_depth": 1
    });
    let proxy_artifact = json!({
        "kind": "user_proxy_plan",
        "envelope": model_proxy_envelope,
        "plan": model_proxy_plan
    });
    let proxy_output_projection = json!({
        "structured_output": proxy_grounded_output,
        "encrypted_output": proxy_model_output,
        "effects": []
    });
    let proxy_output_commitment = sha256_hex(
        serde_json::to_string(&proxy_output_projection)
            .unwrap()
            .as_bytes(),
    );
    let proxy_artifact_commitment = canonical_hash_hex(&proxy_artifact);
    let proxy_observation_id = Uuid::new_v4();
    let proxy_observation_statement = json!({
        "observation_id": proxy_observation_id,
        "dispatch_id": claim_proxy_model["dispatch_id"],
        "invocation_id": proxy_model_invocation_id,
        "attempt": claim_proxy_model["attempt"],
        "lease_id": proxy_model_lease_id,
        "principal_identity_id": fixture.owner_id,
        "exposed_sources": [{
            "kind": "resource_body",
            "resource_id": fixture.profile_resource_id
        }],
        "request_commitment_hex": claim_proxy_model["request_commitment_hex"],
        "context_commitment_hex": claim_proxy_model["context_commitment_hex"],
        "transport_commitment_hex": claim_proxy_model["transport_commitment_hex"],
        "output_commitment_hex": proxy_output_commitment,
        "artifact_commitment_hex": proxy_artifact_commitment,
        "provider_status": "schema_valid",
        "hidden_persistent_model_memory_available": false,
        "idempotency_key": proxy_observation_id,
        "observed_at": Utc::now()
    });
    let submit_proxy_model = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{agent_id}/invocations/{proxy_model_invocation_id}/submit",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {runner_token}"))
                .body(Body::from(
                    json!({
                        "lease_id": proxy_model_lease_id,
                        "structured_output": proxy_grounded_output,
                        "encrypted_output": proxy_model_output,
                        "effects": [],
                        "artifact": proxy_artifact,
                        "observation": {
                            "statement": proxy_observation_statement,
                            "signatures": signed_statement_by(
                                agent_identity_id,
                                runner_device_id,
                                &runner_ed25519_private_key,
                                &runner_ml_dsa_65_private_key,
                                &proxy_observation_statement,
                                b"sprout-model-runtime-observation-v1"
                            )
                        }
                    })
                    .to_string(),
                ))
                .expect("submit model-mediated proxy invocation"),
        )
        .await
        .expect("submit model-mediated proxy response");
    assert_eq!(
        submit_proxy_model.status(),
        StatusCode::OK,
        "{}",
        json_body(submit_proxy_model).await
    );
    let record_model_proxy_plan = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/user-proxy/requests/{model_proxy_request_id}/plan",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "id": Uuid::new_v4(),
                        "invocation_id": proxy_model_invocation_id,
                        "envelope": model_proxy_envelope,
                        "plan": model_proxy_plan,
                        "confirmation": {
                            "user": fixture.owner_id,
                            "thread_id": model_proxy_thread_id,
                            "request_id": model_proxy_request_id,
                            "accepted_plan": model_proxy_plan,
                            "summary_id": Uuid::new_v4(),
                            "confirmed_at": Utc::now()
                        }
                    })
                    .to_string(),
                ))
                .expect("record exact model-mediated proxy plan"),
        )
        .await
        .expect("record exact model-mediated proxy plan response");
    assert_eq!(
        record_model_proxy_plan.status(),
        StatusCode::OK,
        "{}",
        json_body(record_model_proxy_plan).await
    );
    let mut proxy_surface = fixture.pool.begin().await.expect("begin proxy inventory");
    sqlx::query("SELECT set_config('app.identity_id', $1, true)")
        .bind(fixture.owner_id.to_string())
        .execute(&mut *proxy_surface)
        .await
        .expect("set proxy inventory identity");
    let proxy_inventory = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT surface, mode, record_count
         FROM agent_r541_language_surface_inventory ORDER BY surface",
    )
    .fetch_all(&mut *proxy_surface)
    .await
    .expect("load model-mediated proxy surface inventory");
    assert!(proxy_inventory.contains(&("proxy".to_owned(), "enabled".to_owned(), 1)));
    proxy_surface
        .rollback()
        .await
        .expect("rollback proxy inventory");

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
    let retained_trace_structure = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
           (SELECT count(*) FROM agent_r540_tool_trace_roots
             WHERE project_id=$1 AND run_id=$2),
           (SELECT count(*) FROM agent_r540_tool_trace_certificates certificate
             JOIN agent_r540_tool_trace_roots root
               ON root.trace_number=certificate.trace_number
             WHERE root.project_id=$1 AND root.run_id=$2),
           (SELECT count(*) FROM agent_r541_tool_run_surface_gates
             WHERE project_id=$1 AND run_id=$2)",
    )
    .bind(fixture.project_id)
    .bind(collaborative_run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load retained structural trace after operational run purge");
    assert_eq!(retained_trace_structure, (1, 1, 0));
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
        "GRANT SELECT ON agent_compilation_certificates, agent_prompt_final_approvals, agent_administrator_creation_approvals, agent_governance_ledger, agent_governance_authorization_events TO sprout_0029_untrusted_app",
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
              'append_verified_governance_revision',
              'insert_agent_governance_authorization_event'
          )
        "#,
    )
    .fetch_one(&fixture.pool)
    .await
    .expect("verify SECURITY DEFINER ownership and fixed search path");
    assert!(definer_boundary);
    let language_definer_boundary = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT bool_and(
            procedure.prosecdef
            AND procedure.proowner = (SELECT oid FROM pg_roles WHERE rolname = current_user)
            AND procedure.proconfig @> ARRAY['search_path=pg_catalog']::text[]
            AND procedure.proconfig @> ARRAY['row_security=off']::text[]
            AND NOT has_function_privilege(
                'sprout_0029_untrusted_app', procedure.oid, 'EXECUTE'
            )
        )
        FROM pg_proc procedure
        JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
        WHERE namespace.nspname = 'sprout_private'
          AND procedure.proname IN (
              'agent_language_retention_delete_allowed',
              'purge_agent_language_for_interrogation',
              'purge_agent_language_for_proxy_request',
              'purge_agent_language_for_invocation',
              'purge_agent_language_for_effect',
              'interrogation_invocation_is_read_only'
          )
        "#,
    )
    .fetch_one(&fixture.pool)
    .await
    .expect("verify 0031 definer ownership and fixed search path");
    assert!(language_definer_boundary);
    let counts_before = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_array(
             (SELECT count(*) FROM agent_compilation_certificates WHERE project_id = $1),
             (SELECT count(*) FROM agent_prompt_final_approvals WHERE project_id = $1),
             (SELECT count(*) FROM agent_administrator_creation_approvals WHERE project_id = $1),
             (SELECT count(*) FROM agent_governance_authorization_events WHERE project_id = $1),
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
                INSERT INTO public.agent_governance_authorization_events (
                    project_id,event_id,event_kind,workflow_id,workflow_revision,
                    actor_identity_id,idempotency_key,event_hash,payload,ledger_position
                ) VALUES (
                    current_setting('app.project_id')::uuid,gen_random_uuid(),
                    'exception_decision',gen_random_uuid(),1,
                    current_setting('app.identity_id')::uuid,gen_random_uuid(),
                    decode(repeat('00',32),'hex'),'{}'::jsonb,1
                );
                RAISE EXCEPTION 'untrusted app inserted a governance event';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                UPDATE public.agent_governance_authorization_events
                SET event_hash=decode(repeat('00',32),'hex')
                WHERE project_id=current_setting('app.project_id')::uuid;
                RAISE EXCEPTION 'untrusted app updated a governance event';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                DELETE FROM public.agent_governance_authorization_events
                WHERE project_id=current_setting('app.project_id')::uuid;
                RAISE EXCEPTION 'untrusted app deleted a governance event';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                PERFORM sprout_private.append_verified_governance_revision(
                    NULL, NULL, NULL, NULL, NULL, NULL
                );
                RAISE EXCEPTION 'untrusted app executed private governance writer';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                PERFORM sprout_private.insert_agent_governance_authorization_event(
                    NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,
                    NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL
                );
                RAISE EXCEPTION 'untrusted app executed private 0030 writer';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                PERFORM count(*) FROM public.agent_r541_language_surface_inventory;
                RAISE EXCEPTION 'untrusted app read certified language inventory';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                INSERT INTO public.agent_language_causal_mutations (
                    id, project_id, invocation_id, category, record_id
                ) VALUES (
                    gen_random_uuid(), current_setting('app.project_id')::uuid,
                    gen_random_uuid(), 'tool_invocation', gen_random_uuid()
                );
                RAISE EXCEPTION 'untrusted app forged language causal history';
            EXCEPTION WHEN insufficient_privilege THEN NULL;
            END;
            BEGIN
                PERFORM sprout_private.interrogation_invocation_is_read_only(
                    current_setting('app.project_id')::uuid, gen_random_uuid()
                );
                RAISE EXCEPTION 'untrusted app executed private read-only certifier';
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
             (SELECT count(*) FROM agent_governance_authorization_events WHERE project_id = $1),
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

fn certified_responsibility_revision_artifact(
    fixture: &Fixture,
    user_identity_id: Uuid,
    responsibility_id: Uuid,
    revision: u64,
    seed: u8,
) -> (Value, Value) {
    let source = encrypted(seed);
    let source_payload: sprout_domain::EncryptedPayload =
        serde_json::from_value(source.clone()).expect("typed responsibility ciphertext");
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
        "ciphertext_commitment_hex": sha256_hex(
            &serde_json::to_vec(&source_payload).expect("serialize responsibility ciphertext")
        ),
        "output": output.clone(),
        "output_hash_hex": canonical_hash_hex(&output),
        "envelope": envelope.clone(),
        "envelope_hash_hex": canonical_hash_hex(&envelope),
        "idempotency_key": Uuid::new_v4()
    });
    let signed = json!({
        "statement": statement.clone(),
        "signatures": signed_statement(
            fixture,
            &statement,
            b"sprout-governance-compilation-v1"
        )
    });
    (signed, source)
}

async fn exception_operational_snapshot(
    fixture: &Fixture,
    user_identity_id: Uuid,
    agent_id: Uuid,
) -> String {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT jsonb_build_object(
          'responsibilities', COALESCE((
            SELECT jsonb_agg(
              jsonb_build_object('id', id, 'revision', revision, 'state', state)
              ORDER BY revision
            )
            FROM agent_responsibility_contracts
            WHERE project_id=$1 AND user_identity_id=$2
          ), '[]'::jsonb),
          'locals', COALESCE((
            SELECT jsonb_agg(
              jsonb_build_object(
                'id', id, 'revision', revision, 'state', state, 'contract', contract
              ) ORDER BY revision
            )
            FROM agent_local_goal_contracts
            WHERE project_id=$1 AND agent_id=$3
          ), '[]'::jsonb),
          'prompts', COALESCE((
            SELECT jsonb_agg(
              jsonb_build_object(
                'draft', draft_id,
                'revision', local_goal_revision,
                'state', state,
                'ciphertext', encrypted_prompt
              ) ORDER BY local_goal_revision
            )
            FROM agent_prompt_revisions
            WHERE project_id=$1 AND agent_id=$3
          ), '[]'::jsonb),
          'agent_prompt', (
            SELECT encrypted_system_prompt FROM governed_agents
            WHERE project_id=$1 AND id=$3
          ),
          'authority', jsonb_build_object(
            'topic', (SELECT count(*) FROM topic_permissions WHERE project_id=$1),
            'task_list', (SELECT count(*) FROM task_list_permissions WHERE project_id=$1),
            'task', (SELECT count(*) FROM task_permissions WHERE project_id=$1),
            'envelopes', (SELECT count(*) FROM resource_key_envelopes WHERE project_id=$1)
          )
        )::text
        "#,
    )
    .bind(fixture.project_id)
    .bind(user_identity_id)
    .bind(agent_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("load exact exception operational snapshot")
}

async fn certified_exception_is_causal_exact_atomic_and_replay_safe(decision_mode: &str) {
    let fixture = fixture().await;
    let app = app(&fixture);
    let controller = add_signing_human_member(&fixture, "member").await;
    let (responsibility_status, responsibility_id) =
        create_active_compiled_responsibility(&fixture, &app, controller.identity_id, 160).await;
    assert_eq!(responsibility_status, StatusCode::OK);
    let (creation_status, created) = provision_administrator_governed_agent(
        &fixture,
        &app,
        161,
        Some(&controller),
        Some(responsibility_id),
        |_| {},
        false,
    )
    .await;
    assert_eq!(creation_status, StatusCode::OK);

    let source = local_revision_artifact(
        &fixture,
        &created,
        &controller,
        json!({"kind":"responsibility","id":responsibility_id,"revision":1}),
        LocalGoalOrigin::ControllerPrompt {},
        controller.identity_id,
        controller.device_id,
        &controller.ed25519_private_key,
        &controller.ml_dsa_65_private_key,
        true,
        162,
    )
    .await;
    let source_event_id = Uuid::new_v4();
    let disposition_body = json!({
        "event_id": source_event_id,
        "idempotency_key": Uuid::new_v4(),
        "disposition": "request_administrator_review",
        "source": {
            "encrypted_prompt": source.prompt,
            "supersedes_revision": 1,
            "compilation": source.compilation
        },
        "summary": {
            "id": Uuid::new_v4(),
            "reason": "send_responsibility_exception_to_administrator",
            "facts": [
                {"kind":"draft","draft_id":source.draft_id},
                {"kind":"local_revision","revision":2}
            ],
            "encrypted_payload": encrypted(163),
            "generated_at": Utc::now()
        }
    });
    let disposition = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/local-goal-dispositions",
                    fixture.project_id, created.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(disposition_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disposition.status(), StatusCode::OK);

    let review_id = Uuid::new_v4();
    let consent_statement = json!({
        "event_id": Uuid::new_v4(),
        "idempotency_key": Uuid::new_v4(),
        "project_id": fixture.project_id,
        "consent": {
            "review_id": review_id,
            "user": controller.identity_id,
            "source_draft_id": source.draft_id,
            "consented": true
        },
        "summary": {
            "id": Uuid::new_v4(),
            "reason": "send_responsibility_exception_to_administrator",
            "facts": [
                {"kind":"draft","draft_id":source.draft_id},
                {"kind":"local_revision","revision":2}
            ],
            "encrypted_payload": encrypted(164),
            "generated_at": Utc::now()
        }
    });
    let consent_body = json!({
        "statement": consent_statement,
        "signatures": signed_statement_by(
            controller.identity_id,
            controller.device_id,
            &controller.ed25519_private_key,
            &controller.ml_dsa_65_private_key,
            &consent_statement,
            b"sprout-local-goal-exception-consent-v1"
        )
    });
    let consent = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/exception-consents",
                    fixture.project_id, created.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(consent_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(consent.status(), StatusCode::OK);

    let review_task = governance_review_task_fixture(
        &fixture,
        &controller,
        created.principal_identity_id,
        fixture.owner_id,
    )
    .await;
    let review_event_id = Uuid::new_v4();
    let review_body = json!({
        "event_id": review_event_id,
        "idempotency_key": Uuid::new_v4(),
        "review": {
            "id": review_id,
            "user": controller.identity_id,
            "agent": created.principal_identity_id,
            "administrator": fixture.owner_id,
            "source_draft_id": source.draft_id,
            "review_task": review_task["resource_node_id"],
            "excess_summary": encrypted(165),
            "proposed_local": source.local
        },
        "review_task": review_task,
        "review_assignment_id": Uuid::new_v4(),
        "review_permission_grant_id": Uuid::new_v4(),
        "encrypted_assignment": opaque_encrypted(166)
    });
    let review_request = || {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/v1/projects/{}/agents/{}/exception-reviews",
                fixture.project_id, created.agent_id
            ))
            .header(CONTENT_TYPE, "application/json")
            .header("authorization", format!("Bearer {}", controller.bearer))
            .body(Body::from(review_body.to_string()))
            .unwrap()
    };
    let review = app.clone().oneshot(review_request()).await.unwrap();
    assert_eq!(review.status(), StatusCode::OK);
    let review_replay = app.clone().oneshot(review_request()).await.unwrap();
    assert_eq!(review_replay.status(), StatusCode::OK);
    let task_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM tasks WHERE project_id=$1 AND resource_node_id=$2",
    )
    .bind(fixture.project_id)
    .bind(
        review_body["review_task"]["resource_node_id"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap(),
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        task_count, 1,
        "exact review replay must not create a second task"
    );

    let mut review_equivocation = review_body.clone();
    review_equivocation["review"]["excess_summary"] = encrypted(250);
    let review_equivocation_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/exception-reviews",
                    fixture.project_id, created.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(review_equivocation.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(review_equivocation_response.status(), StatusCode::CONFLICT);

    // A real task materialized for one review cannot be rebound to another
    // exact consent, even when agent creator and administrator assignee are
    // otherwise identical.  It also predates the second consent, proving that
    // a merely compatible historical R4 task is not accepted as its effect.
    let unrelated_review_id = Uuid::new_v4();
    let unrelated_consent_statement = json!({
        "event_id": Uuid::new_v4(),
        "idempotency_key": Uuid::new_v4(),
        "project_id": fixture.project_id,
        "consent": {
            "review_id": unrelated_review_id,
            "user": controller.identity_id,
            "source_draft_id": source.draft_id,
            "consented": true
        },
        "summary": consent_statement["summary"].clone()
    });
    let unrelated_consent_body = json!({
        "statement": unrelated_consent_statement,
        "signatures": signed_statement_by(
            controller.identity_id,
            controller.device_id,
            &controller.ed25519_private_key,
            &controller.ml_dsa_65_private_key,
            &unrelated_consent_statement,
            b"sprout-local-goal-exception-consent-v1"
        )
    });
    let unrelated_consent = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/exception-consents",
                    fixture.project_id, created.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(unrelated_consent_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unrelated_consent.status(), StatusCode::OK);
    let mut unrelated_review = review_body.clone();
    unrelated_review["event_id"] = json!(Uuid::new_v4());
    unrelated_review["idempotency_key"] = json!(Uuid::new_v4());
    unrelated_review["review"]["id"] = json!(unrelated_review_id);
    let unrelated_review_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/exception-reviews",
                    fixture.project_id, created.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", controller.bearer))
                .body(Body::from(unrelated_review.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unrelated_review_response.status(), StatusCode::CONFLICT);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM tasks WHERE project_id=$1 AND resource_node_id=$2",
        )
        .bind(fixture.project_id)
        .bind(
            review_body["review_task"]["resource_node_id"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap(),
        1
    );
    let completion = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/tasks/{}/complete",
                    fixture.project_id,
                    review_body["review_task"]["id"].as_str().unwrap()
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "completion_id":Uuid::new_v4(),
                        "assignment_id":review_body["review_assignment_id"],
                        "expected_payload_version":1,
                        "encrypted_completion":opaque_encrypted(169),
                        "completed_at":Utc::now(),
                        "recurrence_series_id":null,
                        "occurrence_number":null,
                        "next_occurrence":null,
                        "idempotency_key":Uuid::new_v4()
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(completion.status(), StatusCode::OK);

    let final_artifact = local_revision_artifact(
        &fixture,
        &created,
        &controller,
        json!({"kind":"administrator_exception","id":review_id,"revision":1}),
        LocalGoalOrigin::AdministratorException { review_id },
        fixture.owner_id,
        fixture.owner_device_id,
        &fixture.owner_ed25519_private_key,
        &fixture.owner_ml_dsa_65_private_key,
        true,
        167,
    )
    .await;
    let expected_final_prompt = final_artifact.prompt.clone();
    let (final_responsibility, final_responsibility_source) =
        if decision_mode == "approved_goal_and_responsibility" {
            let (signed, source) = certified_responsibility_revision_artifact(
                &fixture,
                controller.identity_id,
                responsibility_id,
                2,
                170,
            );
            (Some(signed), Some(source))
        } else {
            (None, None)
        };
    let admin_draft_body = json!({
        "event_id": Uuid::new_v4(),
        "idempotency_key": Uuid::new_v4(),
        "revision": 1,
        "encrypted_prompt": expected_final_prompt.clone(),
        "local_compilation": final_artifact.compilation,
        "final_responsibility": final_responsibility,
        "final_responsibility_encrypted_source": final_responsibility_source,
        "final_responsibility_supersedes_revision":
            (decision_mode == "approved_goal_and_responsibility").then_some(1)
    });
    let admin_draft = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/exception-reviews/{review_id}/drafts",
                    fixture.project_id, created.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(admin_draft_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    if admin_draft.status() != StatusCode::OK {
        panic!(
            "administrator draft failed: {}",
            json_body(admin_draft).await
        );
    }
    let operational_before_decision =
        exception_operational_snapshot(&fixture, controller.identity_id, created.agent_id).await;

    let decision_statement = json!({
        "event_id": Uuid::new_v4(),
        "idempotency_key": Uuid::new_v4(),
        "project_id": fixture.project_id,
        "decision": {
            "review_id": review_id,
            "review_draft_revision": 1,
            "administrator": fixture.owner_id,
            "mode": decision_mode
        },
        "summary": {
            "id": Uuid::new_v4(),
            "reason": "administrator_responsibility_exception_decision",
            "facts": [
                {"kind":"agent","agent":created.principal_identity_id},
                {"kind":"local_revision","revision":2}
            ],
            "encrypted_payload": encrypted(168),
            "generated_at": Utc::now()
        }
    });
    let decision_body = json!({
        "statement": decision_statement,
        "signatures": signed_statement(
            &fixture,
            &decision_statement,
            b"sprout-local-goal-exception-decision-v1"
        )
    });
    let decision_request = || {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/v1/projects/{}/agents/{}/exception-reviews/{review_id}/decision",
                fixture.project_id, created.agent_id
            ))
            .header(CONTENT_TYPE, "application/json")
            .header("authorization", format!("Bearer {}", fixture.owner_token))
            .body(Body::from(decision_body.to_string()))
            .unwrap()
    };
    let decision_response = app.clone().oneshot(decision_request()).await.unwrap();
    if decision_response.status() != StatusCode::OK {
        panic!(
            "exception decision failed: {}",
            json_body(decision_response).await
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(decision_request())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let approved_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_kind='approved_local_exception'
           AND workflow_id=$2",
    )
    .bind(fixture.project_id)
    .bind(review_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    if decision_mode == "rejected" {
        assert_eq!(approved_count, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM agent_governance_authorization_events
                 WHERE project_id=$1 AND event_kind='exception_decision'
                   AND workflow_id=$2",
            )
            .bind(fixture.project_id)
            .bind(review_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            exception_operational_snapshot(&fixture, controller.identity_id, created.agent_id,)
                .await,
            operational_before_decision,
            "rejection must preserve Responsibility, prompt, LocalGoal and authority projections"
        );

        let mut discordant_statement = decision_statement.clone();
        discordant_statement["event_id"] = json!(Uuid::new_v4());
        discordant_statement["idempotency_key"] = json!(Uuid::new_v4());
        discordant_statement["decision"]["mode"] = json!("approved_goal_only");
        let discordant_signatures = signed_statement(
            &fixture,
            &discordant_statement,
            b"sprout-local-goal-exception-decision-v1",
        );
        let discordant = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agents/{}/exception-reviews/{review_id}/decision",
                        fixture.project_id, created.agent_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {}", fixture.owner_token))
                    .body(Body::from(
                        json!({
                            "statement":discordant_statement,
                            "signatures":discordant_signatures
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(discordant.status(), StatusCode::CONFLICT);
        assert_eq!(
            exception_operational_snapshot(&fixture, controller.identity_id, created.agent_id,)
                .await,
            operational_before_decision
        );
        return;
    }
    assert_eq!(approved_count, 1);

    let approval_id = Uuid::new_v4();
    let approval_idempotency = Uuid::new_v4();
    let approval_identity = json!({
        "signature_context":"sprout-final-prompt-approval-v1",
        "approval_id":approval_id,
        "project_id":fixture.project_id,
        "draft_id":final_artifact.draft_id,
        "agent_principal_identity_id":created.principal_identity_id,
        "controller_identity_id":controller.identity_id,
        "local_goal_id":created.local_goal_id,
        "local_revision":2,
        "prompt_commitment_hex":final_artifact.prompt_commitment_hex,
        "ciphertext_commitment_hex":final_artifact.ciphertext_commitment_hex,
        "compilation_certificate_id":final_artifact.certificate_id,
        "structured_output_hash_hex":final_artifact.output_hash_hex,
        "idempotency_key":approval_idempotency
    });
    let approval_statement = json!({
        "approval_id":approval_id,
        "project_id":fixture.project_id,
        "draft_id":final_artifact.draft_id,
        "agent_principal_identity_id":created.principal_identity_id,
        "controller_identity_id":controller.identity_id,
        "local_goal_id":created.local_goal_id,
        "local_revision":2,
        "prompt_commitment_hex":final_artifact.prompt_commitment_hex,
        "ciphertext_commitment_hex":final_artifact.ciphertext_commitment_hex,
        "compilation_certificate_id":final_artifact.certificate_id,
        "structured_output_hash_hex":final_artifact.output_hash_hex,
        "approval_identity_hash_hex":canonical_hash_hex(&approval_identity),
        "idempotency_key":approval_idempotency
    });
    if decision_mode == "approved_goal_and_responsibility" {
        let operational_before_stale =
            exception_operational_snapshot(&fixture, controller.identity_id, created.agent_id)
                .await;
        let audit_before_stale = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_user_governance_audit_log
             WHERE project_id=$1 AND subject_user_identity_id=$2
               AND event_kind IN ('local_goal_activated','responsibility_activated')",
        )
        .bind(fixture.project_id)
        .bind(controller.identity_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        let approvals_before_stale = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_prompt_final_approvals
             WHERE project_id=$1 AND agent_id=$2",
        )
        .bind(fixture.project_id)
        .bind(created.agent_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        let stale_approval_id = Uuid::new_v4();
        let stale_idempotency = Uuid::new_v4();
        let mut stale_identity = approval_identity.clone();
        stale_identity["approval_id"] = json!(stale_approval_id);
        stale_identity["local_revision"] = json!(1);
        stale_identity["idempotency_key"] = json!(stale_idempotency);
        let mut stale_statement = approval_statement.clone();
        stale_statement["approval_id"] = json!(stale_approval_id);
        stale_statement["local_revision"] = json!(1);
        stale_statement["idempotency_key"] = json!(stale_idempotency);
        stale_statement["approval_identity_hash_hex"] = json!(canonical_hash_hex(&stale_identity));
        let stale_signatures = signed_statement_by(
            controller.identity_id,
            controller.device_id,
            &controller.ed25519_private_key,
            &controller.ml_dsa_65_private_key,
            &stale_statement,
            b"sprout-final-prompt-approval-v1",
        );
        let stale_activation = app
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
                    .body(Body::from(
                        json!({
                            "statement":stale_statement,
                            "signatures":stale_signatures
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stale_activation.status(), StatusCode::CONFLICT);
        assert_eq!(
            exception_operational_snapshot(&fixture, controller.identity_id, created.agent_id,)
                .await,
            operational_before_stale
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM agent_user_governance_audit_log
                 WHERE project_id=$1 AND subject_user_identity_id=$2
                   AND event_kind IN ('local_goal_activated','responsibility_activated')",
            )
            .bind(fixture.project_id)
            .bind(controller.identity_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
            audit_before_stale
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM agent_prompt_final_approvals
                 WHERE project_id=$1 AND agent_id=$2",
            )
            .bind(fixture.project_id)
            .bind(created.agent_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
            approvals_before_stale
        );
    }
    let activation = app
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
                .body(Body::from(
                    json!({
                        "statement":approval_statement,
                        "signatures":signed_statement_by(
                            controller.identity_id,
                            controller.device_id,
                            &controller.ed25519_private_key,
                            &controller.ml_dsa_65_private_key,
                            &approval_statement,
                            b"sprout-final-prompt-approval-v1"
                        )
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(activation.status(), StatusCode::OK);
    let expected_responsibility_revision = if decision_mode == "approved_goal_and_responsibility" {
        2
    } else {
        1
    };
    let expected_final_prompt_bytes = serde_json::to_vec(
        &serde_json::from_value::<sprout_domain::EncryptedPayload>(expected_final_prompt)
            .expect("typed final exception prompt"),
    )
    .expect("serialize final exception prompt");
    let exact_atomic_state = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT
          (SELECT count(*) = 1 FROM agent_responsibility_contracts
           WHERE project_id=$1 AND user_identity_id=$2 AND state='active')
          AND (SELECT count(*) = 1 FROM agent_responsibility_contracts
               WHERE project_id=$1 AND user_identity_id=$2
                 AND revision=$4 AND state='active')
          AND CASE WHEN $4 = 2 THEN
                (SELECT count(*) = 1 FROM agent_responsibility_contracts
                 WHERE project_id=$1 AND user_identity_id=$2
                   AND revision=1 AND state='superseded')
              ELSE
                (SELECT count(*) = 1 FROM agent_responsibility_contracts
                 WHERE project_id=$1 AND user_identity_id=$2
                   AND revision=1 AND state='active')
              END
          AND (SELECT count(*) = 1 FROM agent_local_goal_contracts
               WHERE project_id=$1 AND agent_id=$3 AND state='active')
          AND (SELECT count(*) = 1 FROM agent_local_goal_contracts
               WHERE project_id=$1 AND agent_id=$3 AND revision=1
                 AND state='superseded')
          AND (SELECT count(*) = 1 FROM agent_local_goal_contracts
               WHERE project_id=$1 AND agent_id=$3 AND revision=2
                 AND state='active' AND contract=$5::jsonb
                 AND contract #>> '{origin,kind}'='administrator_exception')
          AND (SELECT count(*) = 1 FROM agent_prompt_revisions
               WHERE project_id=$1 AND agent_id=$3 AND local_goal_revision=2
                 AND state='active' AND encrypted_prompt=$6)
          AND (SELECT count(*) = 1 FROM governed_agents agent
               WHERE agent.project_id=$1 AND agent.id=$3
                 AND agent.encrypted_system_prompt=$6)
        "#,
    )
    .bind(fixture.project_id)
    .bind(controller.identity_id)
    .bind(created.agent_id)
    .bind(expected_responsibility_revision)
    .bind(serde_json::to_value(&final_artifact.local).unwrap())
    .bind(expected_final_prompt_bytes)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert!(exact_atomic_state);
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn certified_exception_goal_only_is_causal_exact_atomic_and_replay_safe() {
    certified_exception_is_causal_exact_atomic_and_replay_safe("approved_goal_only").await;
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn certified_exception_goal_and_responsibility_activate_atomically() {
    certified_exception_is_causal_exact_atomic_and_replay_safe("approved_goal_and_responsibility")
        .await;
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn certified_exception_rejection_is_terminal_idempotent_and_non_authorizing() {
    certified_exception_is_causal_exact_atomic_and_replay_safe("rejected").await;
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn certified_global_mandate_for_existing_agent_rechecks_permission_and_grants_nothing() {
    let fixture = fixture().await;
    let app = app(&fixture);
    let controller = add_signing_human_member(&fixture, "member").await;
    let (responsibility_status, responsibility_id) =
        create_active_compiled_responsibility(&fixture, &app, controller.identity_id, 180).await;
    assert_eq!(responsibility_status, StatusCode::OK);
    let (creation_status, created) = provision_administrator_governed_agent(
        &fixture,
        &app,
        181,
        Some(&controller),
        Some(responsibility_id),
        |_| {},
        false,
    )
    .await;
    assert_eq!(creation_status, StatusCode::OK);

    let permission_id = Uuid::new_v4();
    sqlx::query(
        "UPDATE governed_agents SET availability='project_delegable'
         WHERE project_id=$1 AND id=$2",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO topic_permissions (
             id,project_id,topic_id,member_identity_id,access_level,visibility,
             grant_origin,root_grant_id,access_scope,granted_by_identity_id
         ) SELECT $2,$1,topic.id,$3,'manage','restricted','explicit',$2,'full',$4
           FROM topics topic WHERE topic.project_id=$1 AND topic.resource_node_id=$5",
    )
    .bind(fixture.project_id)
    .bind(permission_id)
    .bind(created.principal_identity_id)
    .bind(fixture.owner_id)
    .bind(fixture.profile_resource_id)
    .execute(&fixture.pool)
    .await
    .unwrap();

    let source_local_json = sqlx::query_scalar::<_, String>(
        "SELECT contract::text FROM agent_local_goal_contracts
         WHERE project_id=$1 AND agent_id=$2 AND id=$3
           AND revision=1 AND state='active'",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .bind(created.local_goal_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let source_local: Value = serde_json::from_str(&source_local_json).unwrap();
    let goal_contract = source_local["contract"].clone();
    let obligation_id = goal_contract["obligations"][0]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let global_contract_id = Uuid::new_v4();
    let global = app
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
                        "id":global_contract_id,
                        "synthesis_invocation_id":null,
                        "envelope":{
                            "language_task":{
                                "id":Uuid::new_v4(),
                                "kind":"synthesize_global_contract",
                                "input_item_count":1,
                                "max_input_items":1,
                                "max_output_items":4,
                                "max_nesting_depth":4,
                                "max_attempts":1,
                                "closed_output_schema":true,
                                "grounded_identifiers_only":true,
                                "requires_formal_proof":false,
                                "requires_permission_decision":false,
                                "requires_exact_semantic_equivalence":false,
                                "requires_exhaustive_world_knowledge":false,
                                "allowed_resource_ids":[fixture.profile_resource_id],
                                "allowed_principal_ids":[created.principal_identity_id],
                                "allowed_tools":[]
                            },
                            "source_agents":[created.principal_identity_id],
                            "max_global_obligations":1,
                            "max_global_work_specs":1,
                            "max_dependencies":0,
                            "max_conflicts":0
                        },
                        "candidate":{
                            "revision":1,
                            "contract":goal_contract,
                            "contributions":[{
                                "agent":created.principal_identity_id,
                                "local_revision":1,
                                "local_clause_id":1,
                                "global_work_spec_ids":[1]
                            }],
                            "governance_conflicts":[]
                        },
                        "groundings":[{
                            "global_work_spec_id":1,
                            "source_agent":created.principal_identity_id,
                            "source_local_revision":1,
                            "source_work_spec_id":1
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        global.status(),
        StatusCode::OK,
        "{}",
        json_body(global).await
    );

    let need_id = Uuid::new_v4();
    let need = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-global-coverage-needs",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(
                    json!({
                        "event_id":need_id,
                        "idempotency_key":Uuid::new_v4(),
                        "global_contract_id":global_contract_id,
                        "global_revision":1,
                        "obligation_id":obligation_id
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(need.status(), StatusCode::OK, "{}", json_body(need).await);

    let mandate_id = Uuid::new_v4();
    let artifact = local_revision_artifact(
        &fixture,
        &created,
        &controller,
        json!({"kind":"global_mandate","id":mandate_id,"revision":1}),
        LocalGoalOrigin::GlobalMandate { global_revision: 1 },
        controller.identity_id,
        controller.device_id,
        &controller.ed25519_private_key,
        &controller.ml_dsa_65_private_key,
        false,
        182,
    )
    .await;
    let mandate_body = json!({
        "event_id":mandate_id,
        "idempotency_key":Uuid::new_v4(),
        "need_id":need_id,
        "supersedes_revision":1,
        "encrypted_prompt":artifact.prompt,
        "compilation":artifact.compilation
    });
    for mut forged in [
        {
            let mut value = mandate_body.clone();
            value["compilation"]["statement"]["authorization"]["id"] = json!(Uuid::new_v4());
            value
        },
        {
            let mut value = mandate_body.clone();
            value["compilation"]["statement"]["authorization"]["revision"] = json!(2);
            value
        },
        {
            let mut value = mandate_body.clone();
            value["compilation"]["statement"]["local_goal_id"] = json!(Uuid::new_v4());
            value
        },
    ] {
        resign_local_compilation(&mut forged, &controller);
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/projects/{}/agents/{}/global-mandates",
                        fixture.project_id, created.agent_id
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {}", fixture.owner_token))
                    .body(Body::from(forged.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(rejected.status(), StatusCode::OK);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT
               (SELECT count(*) FROM agent_governance_authorization_events
                 WHERE project_id=$1 AND event_kind='global_mandate_assignment')
             + (SELECT count(*) FROM agent_compilation_certificates
                 WHERE project_id=$1 AND id=$2)
             + (SELECT count(*) FROM agent_local_goal_contracts
                 WHERE project_id=$1 AND agent_id=$3 AND revision=2)
             + (SELECT count(*) FROM agent_governance_ledger
                 WHERE project_id=$1 AND entry_id=$2)",
        )
        .bind(fixture.project_id)
        .bind(artifact.certificate_id)
        .bind(created.agent_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap(),
        0
    );
    let mandate_request = || {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/v1/projects/{}/agents/{}/global-mandates",
                fixture.project_id, created.agent_id
            ))
            .header(CONTENT_TYPE, "application/json")
            .header("authorization", format!("Bearer {}", fixture.owner_token))
            .body(Body::from(mandate_body.to_string()))
            .unwrap()
    };
    let mandate = app.clone().oneshot(mandate_request()).await.unwrap();
    assert_eq!(
        mandate.status(),
        StatusCode::OK,
        "{}",
        json_body(mandate).await
    );
    assert_eq!(
        app.clone()
            .oneshot(mandate_request())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mut other_mandate = mandate_body.clone();
    other_mandate["event_id"] = json!(Uuid::new_v4());
    other_mandate["idempotency_key"] = json!(Uuid::new_v4());
    // The compilation deliberately retains the already valid first mandate
    // as its authorization_id; it cannot authorize this second workflow.
    let other_mandate_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agents/{}/global-mandates",
                    fixture.project_id, created.agent_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(other_mandate.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(other_mandate_response.status(), StatusCode::CONFLICT);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_governance_authorization_events
             WHERE project_id=$1 AND event_kind='global_mandate_assignment'",
        )
        .bind(fixture.project_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap(),
        1
    );

    let approval = exact_final_prompt_approval(&fixture, &created, &controller, &artifact);
    sqlx::query(
        "UPDATE governed_agents SET availability='controller_private'
         WHERE project_id=$1 AND id=$2",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let activate = || {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/v1/projects/{}/agents/{}/local-goals/{}/revisions/2/activate",
                fixture.project_id, created.agent_id, created.local_goal_id
            ))
            .header(CONTENT_TYPE, "application/json")
            .header("authorization", format!("Bearer {}", controller.bearer))
            .body(Body::from(approval.to_string()))
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(activate()).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    sqlx::query(
        "UPDATE governed_agents SET availability='project_delegable'
         WHERE project_id=$1 AND id=$2",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE topic_permissions SET revoked_at=clock_timestamp()
         WHERE project_id=$1 AND id=$2",
    )
    .bind(fixture.project_id)
    .bind(permission_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        app.clone().oneshot(activate()).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM agent_local_goal_contracts
             WHERE project_id=$1 AND agent_id=$2 AND revision=2 AND state='active'",
        )
        .bind(fixture.project_id)
        .bind(created.agent_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap(),
        0
    );

    sqlx::query(
        "INSERT INTO topic_permissions (
             id,project_id,topic_id,member_identity_id,access_level,visibility,
             grant_origin,root_grant_id,access_scope,granted_by_identity_id
         ) SELECT $1,$2,topic.id,$3,'manage','restricted','explicit',$1,'full',$4
           FROM topics topic WHERE topic.project_id=$2 AND topic.resource_node_id=$5",
    )
    .bind(Uuid::new_v4())
    .bind(fixture.project_id)
    .bind(created.principal_identity_id)
    .bind(fixture.owner_id)
    .bind(fixture.profile_resource_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let authority_before = sqlx::query_as::<_, (i64, i64)>(
        "SELECT
           (SELECT count(*) FROM topic_permissions WHERE project_id=$1),
           (SELECT count(*) FROM resource_key_envelopes WHERE project_id=$1)",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let activation = app.clone().oneshot(activate()).await.unwrap();
    assert_eq!(
        activation.status(),
        StatusCode::OK,
        "{}",
        json_body(activation).await
    );
    let authority_after = sqlx::query_as::<_, (i64, i64)>(
        "SELECT
           (SELECT count(*) FROM topic_permissions WHERE project_id=$1),
           (SELECT count(*) FROM resource_key_envelopes WHERE project_id=$1)",
    )
    .bind(fixture.project_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(authority_after, authority_before);
    let active_origin = sqlx::query_scalar::<_, String>(
        "SELECT contract->'origin'->>'kind' FROM agent_local_goal_contracts
         WHERE project_id=$1 AND agent_id=$2 AND revision=2 AND state='active'",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(active_origin, "global_mandate");

    let compiler_source = sqlx::query(
        "SELECT certificate.canonical_output::text AS output,
                certificate.compilation_envelope::text AS envelope
         FROM agent_local_goal_contracts local
         JOIN agent_compilation_certificates certificate
           ON certificate.project_id=local.project_id
          AND certificate.id=local.compilation_certificate_id
         WHERE local.project_id=$1 AND local.agent_id=$2
           AND local.revision=1",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let mut proposal_output: Value =
        serde_json::from_str(compiler_source.try_get("output").unwrap()).unwrap();
    let mut proposal_envelope: Value =
        serde_json::from_str(compiler_source.try_get("envelope").unwrap()).unwrap();
    let proposed_agent = Uuid::new_v4();
    proposal_output["contract"]["obligations"][0]["owner"] = json!(proposed_agent);
    proposal_output["contract"]["work_specs"][0]["owner"] = json!(proposed_agent);
    proposal_envelope["agent"] = json!(proposed_agent);
    proposal_envelope["controller"] = json!(controller.identity_id);
    proposal_envelope["language_task"]["allowed_principal_ids"] =
        json!([proposed_agent, controller.identity_id]);
    let proposal_event_id = Uuid::new_v4();
    let proposal_local_id = Uuid::new_v4();
    let proposal_draft_id = Uuid::new_v4();
    let proposal_certificate_id = Uuid::new_v4();
    let proposal_prompt = encrypted(183);
    let typed_proposal_prompt: sprout_domain::EncryptedPayload =
        serde_json::from_value(proposal_prompt.clone()).unwrap();
    let proposal_prompt_commitment = "33".repeat(32);
    let proposal_ciphertext_commitment =
        sha256_hex(&serde_json::to_vec(&typed_proposal_prompt).expect("serialize proposal prompt"));
    let proposal_statement = json!({
        "certificate_id":proposal_certificate_id,
        "compiler":{
            "compiler_id":"sprout.local-goal.compiler","compiler_version":1,
            "compiler_build_digest_hex":"0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2"
        },
        "project_id":fixture.project_id,
        "local_goal_id":proposal_local_id,
        "local_revision":1,
        "draft_id":proposal_draft_id,
        "agent_principal_identity_id":proposed_agent,
        "controller_identity_id":controller.identity_id,
        "prompt_commitment_hex":proposal_prompt_commitment,
        "ciphertext_commitment_hex":proposal_ciphertext_commitment,
        "output":proposal_output,
        "output_hash_hex":canonical_hash_hex(&proposal_output),
        "envelope":proposal_envelope,
        "envelope_hash_hex":canonical_hash_hex(&proposal_envelope),
        "authorization":{"kind":"global_mandate","id":proposal_event_id,"revision":1},
        "idempotency_key":Uuid::new_v4()
    });
    let proposal_compilation = json!({
        "statement":proposal_statement,
        "signatures":signed_statement_by(
            controller.identity_id,controller.device_id,
            &controller.ed25519_private_key,&controller.ml_dsa_65_private_key,
            &proposal_statement,b"sprout-governance-compilation-v1"
        )
    });
    let proposal_need = sqlx::query_scalar::<_, Value>(
        "SELECT payload->'need' FROM agent_governance_authorization_events
         WHERE project_id=$1 AND event_id=$2",
    )
    .bind(fixture.project_id)
    .bind(need_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let proposal_local = json!({
        "id":proposal_local_id,"revision":1,"agent":proposed_agent,
        "controller":controller.identity_id,"encrypted_prompt":proposal_prompt,
        "contract":proposal_output["contract"].clone(),
        "clauses":[{"id":1,"domain":1,"scope":fixture.profile_resource_id,"work_spec_ids":[1]}],
        "origin":{"kind":"global_mandate","global_revision":1},
        "supersedes_revision":null
    });
    let proposal_body = json!({
        "event_id":proposal_event_id,"idempotency_key":Uuid::new_v4(),
        "need_id":need_id,"global_contract_id":global_contract_id,
        "proposal":{
            "proposed_agent":proposed_agent,"controller":controller.identity_id,
            "need":proposal_need,"local":proposal_local,
            "requested":proposal_need["required"].clone()
        },
        "encrypted_prompt":typed_proposal_prompt,
        "compilation":proposal_compilation
    });
    let proposal = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-global-new-agent-proposals",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(proposal_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        proposal.status(),
        StatusCode::OK,
        "{}",
        json_body(proposal).await
    );
    let proposal_materialization = sqlx::query_scalar::<_, i64>(
        "SELECT
           (SELECT count(*) FROM identities WHERE id=$1)
         + (SELECT count(*) FROM governed_agents WHERE principal_identity_id=$1)
         + (SELECT count(*) FROM agent_runners runner JOIN governed_agents agent
              ON agent.project_id=runner.project_id AND agent.id=runner.agent_id
              WHERE agent.principal_identity_id=$1)",
    )
    .bind(proposed_agent)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(proposal_materialization, 0);
    let mut oversized_proposal = proposal_body.clone();
    oversized_proposal["event_id"] = json!(Uuid::new_v4());
    oversized_proposal["idempotency_key"] = json!(Uuid::new_v4());
    oversized_proposal["proposal"]["requested"]["resource_effects"] = json!([]);
    let oversized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/projects/{}/agent-global-new-agent-proposals",
                    fixture.project_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {}", fixture.owner_token))
                .body(Body::from(oversized_proposal.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);

    let active_local_json = sqlx::query_scalar::<_, String>(
        "SELECT contract::text FROM agent_local_goal_contracts
         WHERE project_id=$1 AND agent_id=$2 AND revision=2 AND state='active'",
    )
    .bind(fixture.project_id)
    .bind(created.agent_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let active_local: Value = serde_json::from_str(&active_local_json).unwrap();
    let bottom_up = app
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
                        "id":Uuid::new_v4(),
                        "synthesis_invocation_id":null,
                        "envelope":{
                            "language_task":{
                                "id":Uuid::new_v4(),
                                "kind":"synthesize_global_contract",
                                "input_item_count":1,"max_input_items":1,
                                "max_output_items":4,"max_nesting_depth":4,"max_attempts":1,
                                "closed_output_schema":true,"grounded_identifiers_only":true,
                                "requires_formal_proof":false,"requires_permission_decision":false,
                                "requires_exact_semantic_equivalence":false,
                                "requires_exhaustive_world_knowledge":false,
                                "allowed_resource_ids":[fixture.profile_resource_id],
                                "allowed_principal_ids":[created.principal_identity_id],
                                "allowed_tools":[]
                            },
                            "source_agents":[created.principal_identity_id],
                            "max_global_obligations":1,"max_global_work_specs":1,
                            "max_dependencies":0,"max_conflicts":0
                        },
                        "candidate":{
                            "revision":1,"contract":active_local["contract"].clone(),
                            "contributions":[{
                                "agent":created.principal_identity_id,
                                "local_revision":2,"local_clause_id":1,
                                "global_work_spec_ids":[1]
                            }],
                            "governance_conflicts":[]
                        },
                        "groundings":[{
                            "global_work_spec_id":1,"source_agent":created.principal_identity_id,
                            "source_local_revision":2,"source_work_spec_id":1
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bottom_up.status(), StatusCode::BAD_REQUEST);
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
