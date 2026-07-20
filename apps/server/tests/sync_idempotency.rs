use std::env;

use sprout_storage_postgres::{AppendSyncEvent, PostgresStorage, RequestContext, StorageError};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

const T_LLR_07_3: &str = "T-LLR-07.3";

async fn migrated_pool() -> PgPool {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to a migrated disposable PostgreSQL database");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect to sync idempotency test database")
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL database"]
async fn identical_idempotency_digest_replays_and_collision_conflicts() {
    let pool = migrated_pool().await;
    let identity_id = Uuid::new_v4();
    let device_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let idempotency_key = Uuid::new_v4();

    let mut transaction = pool.begin().await.expect("begin fixture");
    sqlx::query("SET LOCAL row_security = off")
        .execute(&mut *transaction)
        .await
        .expect("disable rls for fixture");
    sqlx::query(
        "INSERT INTO identities (id, identity_handle, encrypted_profile)
         VALUES ($1, $2, decode('01', 'hex'))",
    )
    .bind(identity_id)
    .bind(format!("sync-{}", identity_id.simple()))
    .execute(&mut *transaction)
    .await
    .expect("identity");
    sqlx::query(
        "INSERT INTO devices (
             id, identity_id, device_kind, encrypted_label, trust_state
         ) VALUES ($1, $2, 'web', decode('01', 'hex'), 'trusted')",
    )
    .bind(device_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await
    .expect("device");
    sqlx::query(
        "INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
         VALUES ($1, $2, decode('01', 'hex'))",
    )
    .bind(project_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await
    .expect("project");
    sqlx::query(
        "INSERT INTO project_memberships (project_id, identity_id, role)
         VALUES ($1, $2, 'owner')",
    )
    .bind(project_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await
    .expect("membership");
    sqlx::query(
        "INSERT INTO resource_nodes (
             id, project_id, parent_id, node_kind,
             encrypted_metadata, created_by_identity_id
         ) VALUES ($1, $2, NULL, 'root', decode('01', 'hex'), $3)",
    )
    .bind(resource_id)
    .bind(project_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await
    .expect("resource");
    sqlx::query(
        "INSERT INTO resource_epochs (
             id, project_id, resource_node_id, epoch,
             created_by_identity_id, created_by_device_id,
             created_by_device_key_version, key_commitment, reason
         ) VALUES (
             $1, $2, $3, 1, $4, $5, 1,
             decode('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'hex'),
             'created'
         )",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(resource_id)
    .bind(identity_id)
    .bind(device_id)
    .execute(&mut *transaction)
    .await
    .expect("epoch");
    transaction.commit().await.expect("commit fixture");

    let storage = PostgresStorage::new(pool);
    let mut event = AppendSyncEvent {
        project_id,
        stream_id: resource_id,
        resource_node_id: resource_id,
        base_version: 0,
        aggregate_version: 1,
        mutation_kind: "upsert".into(),
        actor_identity_id: identity_id,
        actor_device_id: device_id,
        actor_device_key_version: 1,
        device_sequence: 1,
        client_event_id: Uuid::new_v4(),
        event_kind: "updated".into(),
        key_epoch: 1,
        encrypted_payload: vec![0x01],
        previous_hash: None,
        event_hash: vec![0x31; 32],
        signature: vec![0x32; 64],
        post_quantum_signature: vec![0x33; 64],
        client_created_at: chrono::Utc::now(),
        idempotency_key,
        request_hash: vec![0x41; 32],
        idempotency_expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
    };

    let first = storage
        .append_sync_event(RequestContext::new(identity_id, Some(device_id)), &event)
        .await
        .expect("first append");
    assert!(!first.replayed, "{T_LLR_07_3}: first write must not replay");

    let replay = storage
        .append_sync_event(RequestContext::new(identity_id, Some(device_id)), &event)
        .await
        .expect("identical digest replay");
    assert!(
        replay.replayed,
        "{T_LLR_07_3}: identical digest must replay"
    );
    assert_eq!(replay.event.id, first.event.id);

    event.encrypted_payload = vec![0x02];
    event.request_hash = vec![0x42; 32];
    let collision = storage
        .append_sync_event(RequestContext::new(identity_id, Some(device_id)), &event)
        .await;
    assert!(
        matches!(collision, Err(StorageError::IdempotencyConflict)),
        "{T_LLR_07_3}: digest collision must conflict, got {collision:?}"
    );
}
