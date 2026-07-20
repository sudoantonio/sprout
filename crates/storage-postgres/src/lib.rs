use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use chrono::{DateTime, Utc};
use sprout_crypto_protocol::ExperimentalWrappedResourceKey;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("postgres operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] MigrateError),
    #[error("invalid storage input: {0}")]
    InvalidInput(&'static str),
    #[error("an idempotency key was reused for a different request")]
    IdempotencyConflict,
}

#[derive(Clone)]
pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self> {
        if max_connections == 0 {
            return Err(StorageError::InvalidInput(
                "max_connections must be greater than zero",
            ));
        }

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await?;
        Ok(Self::new(pool))
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Loads migrations at runtime, so compilation never requires `DATABASE_URL`.
    pub async fn migrate(&self, migrations_dir: impl AsRef<Path>) -> Result<()> {
        let migrator = Migrator::new(migrations_dir.as_ref()).await?;
        migrator.run(&self.pool).await?;
        Ok(())
    }

    pub async fn health(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn begin(&self, context: RequestContext) -> Result<TransactionContext<'_>> {
        let mut transaction = self.pool.begin().await?;
        let identity_id = context.identity_id.to_string();
        let device_id = context
            .device_id
            .map(|value| value.to_string())
            .unwrap_or_default();
        let project_id = context
            .project_id
            .map(|value| value.to_string())
            .unwrap_or_default();

        sqlx::query(
            r#"
            SELECT
                set_config('app.identity_id', $1, true),
                set_config('app.device_id', $2, true),
                set_config('app.project_id', $3, true)
            "#,
        )
        .bind(identity_id)
        .bind(device_id)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;

        Ok(TransactionContext {
            transaction,
            context,
        })
    }

    pub async fn append_sync_event(
        &self,
        context: RequestContext,
        input: &AppendSyncEvent,
    ) -> Result<AppendSyncOutcome> {
        input.validate(&context)?;
        let mut transaction = self.begin(context).await?;

        let epoch_is_active = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM resource_epochs
                WHERE project_id = $1
                  AND resource_node_id = $2
                  AND epoch = $3
                  AND retired_at IS NULL
            )
            "#,
        )
        .bind(input.project_id)
        .bind(input.resource_node_id)
        .bind(input.key_epoch)
        .fetch_one(&mut *transaction.transaction)
        .await?;
        if !epoch_is_active {
            return Err(StorageError::InvalidInput(
                "sync revision must use the active resource key epoch",
            ));
        }

        // Serializes a single idempotency key even when callers target different streams.
        sqlx::query(
            r#"
            SELECT pg_advisory_xact_lock(
                hashtextextended(
                    $1::uuid::text || ':' || $2::uuid::text || ':' || $3::uuid::text,
                    2
                )
            )
            "#,
        )
        .bind(input.project_id)
        .bind(input.actor_device_id)
        .bind(input.idempotency_key)
        .execute(&mut *transaction.transaction)
        .await?;

        let existing = sqlx::query(
            r#"
            SELECT
                idempotency.request_hash,
                event.id,
                event.event_sequence,
                event.project_id,
                event.stream_id,
                event.resource_node_id,
                event.base_version,
                event.aggregate_version,
                event.mutation_kind,
                event.actor_identity_id,
                event.actor_device_id,
                event.actor_device_key_version,
                event.device_sequence,
                event.client_event_id,
                event.event_kind,
                event.key_epoch,
                event.encrypted_payload,
                event.previous_hash,
                event.event_hash,
                event.signature,
                event.post_quantum_signature,
                event.client_created_at,
                event.received_at
            FROM sync_idempotency idempotency
            JOIN sync_events event
              ON event.project_id = idempotency.project_id
             AND event.id = idempotency.sync_event_id
            WHERE idempotency.project_id = $1
              AND idempotency.actor_device_id = $2
              AND idempotency.idempotency_key = $3
            "#,
        )
        .bind(input.project_id)
        .bind(input.actor_device_id)
        .bind(input.idempotency_key)
        .fetch_optional(&mut *transaction.transaction)
        .await?;

        if let Some(row) = existing {
            let stored_request_hash: Vec<u8> = row.try_get("request_hash")?;
            if stored_request_hash != input.request_hash {
                return Err(StorageError::IdempotencyConflict);
            }

            let event = sync_event_from_row(&row)?;
            let projection = load_sync_projection(
                &mut transaction.transaction,
                event.project_id,
                event.resource_node_id,
            )
            .await?;
            transaction.commit().await?;
            return Ok(AppendSyncOutcome {
                event,
                projection,
                replayed: true,
            });
        }

        let row = sqlx::query(
            r#"
            INSERT INTO sync_events (
                project_id,
                stream_id,
                resource_node_id,
                base_version,
                aggregate_version,
                mutation_kind,
                actor_identity_id,
                actor_device_id,
                actor_device_key_version,
                device_sequence,
                client_event_id,
                event_kind,
                key_epoch,
                encrypted_payload,
                previous_hash,
                event_hash,
                signature,
                signature_suite,
                post_quantum_signature,
                client_created_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $12, $13, $14,
                $15, $16, $17, 32769, $18, $19
            )
            RETURNING
                id,
                event_sequence,
                project_id,
                stream_id,
                resource_node_id,
                base_version,
                aggregate_version,
                mutation_kind,
                actor_identity_id,
                actor_device_id,
                actor_device_key_version,
                device_sequence,
                client_event_id,
                event_kind,
                key_epoch,
                encrypted_payload,
                previous_hash,
                event_hash,
                signature,
                post_quantum_signature,
                client_created_at,
                received_at
            "#,
        )
        .bind(input.project_id)
        .bind(input.stream_id)
        .bind(input.resource_node_id)
        .bind(input.base_version)
        .bind(input.aggregate_version)
        .bind(&input.mutation_kind)
        .bind(input.actor_identity_id)
        .bind(input.actor_device_id)
        .bind(input.actor_device_key_version)
        .bind(input.device_sequence)
        .bind(input.client_event_id)
        .bind(&input.event_kind)
        .bind(input.key_epoch)
        .bind(&input.encrypted_payload)
        .bind(&input.previous_hash)
        .bind(&input.event_hash)
        .bind(&input.signature)
        .bind(&input.post_quantum_signature)
        .bind(input.client_created_at)
        .fetch_one(&mut *transaction.transaction)
        .await?;
        let event = sync_event_from_row(&row)?;
        let projection = load_sync_projection(
            &mut transaction.transaction,
            event.project_id,
            event.resource_node_id,
        )
        .await?;

        sqlx::query(
            r#"
            INSERT INTO sync_idempotency (
                project_id,
                actor_device_id,
                idempotency_key,
                request_hash,
                sync_event_id,
                event_sequence,
                expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(input.project_id)
        .bind(input.actor_device_id)
        .bind(input.idempotency_key)
        .bind(&input.request_hash)
        .bind(event.id)
        .bind(event.event_sequence)
        .bind(input.idempotency_expires_at)
        .execute(&mut *transaction.transaction)
        .await?;

        transaction.commit().await?;
        Ok(AppendSyncOutcome {
            event,
            projection,
            replayed: false,
        })
    }

    pub async fn resolve_permission(
        &self,
        context: RequestContext,
        project_id: Uuid,
        resource_kind: ResourceKind,
        resource_id: Uuid,
    ) -> Result<Option<ResolvedPermission>> {
        let mut transaction = self.begin(context.with_project_id(project_id)).await?;
        let row = sqlx::query(PERMISSION_RESOLUTION_SQL)
            .bind(project_id)
            .bind(resource_kind.as_str())
            .bind(resource_id)
            .bind(transaction.context.identity_id)
            .fetch_optional(&mut *transaction.transaction)
            .await?;

        let permission = row
            .map(|row| resolved_permission_from_row(&row))
            .transpose()?;

        transaction.commit().await?;
        Ok(permission)
    }

    pub async fn acquire_retention_lease(
        &self,
        context: RequestContext,
        request: &AcquireRetentionLease,
    ) -> Result<Option<RetentionLease>> {
        self.acquire_retention_lease_at(context, request, Utc::now())
            .await
    }

    pub async fn acquire_retention_lease_at(
        &self,
        context: RequestContext,
        request: &AcquireRetentionLease,
        now: DateTime<Utc>,
    ) -> Result<Option<RetentionLease>> {
        request.validate()?;
        let mut transaction = self
            .begin(context.with_project_id(request.project_id))
            .await?;

        let row = sqlx::query(
            r#"
            INSERT INTO retention_leases (
                project_id,
                lease_scope,
                partition_key,
                lease_owner,
                lease_token,
                acquired_at,
                heartbeat_at,
                expires_at
            )
            VALUES (
                $1, $2, $3, $4, gen_random_uuid(),
                $5,
                $5,
                $5 + make_interval(secs => $6::double precision)
            )
            ON CONFLICT (project_id, lease_scope, partition_key)
            DO UPDATE SET
                lease_owner = EXCLUDED.lease_owner,
                lease_token = gen_random_uuid(),
                acquired_at = $5,
                heartbeat_at = $5,
                expires_at = $5 + make_interval(secs => $6::double precision)
            WHERE retention_leases.expires_at <= $5
               OR retention_leases.lease_owner = EXCLUDED.lease_owner
            RETURNING
                project_id,
                lease_scope,
                partition_key,
                lease_owner,
                lease_token,
                acquired_at,
                heartbeat_at,
                expires_at
            "#,
        )
        .bind(request.project_id)
        .bind(&request.lease_scope)
        .bind(&request.partition_key)
        .bind(request.lease_owner)
        .bind(now)
        .bind(request.ttl_seconds)
        .fetch_optional(&mut *transaction.transaction)
        .await?;

        let lease = row.map(|row| retention_lease_from_row(&row)).transpose()?;
        transaction.commit().await?;
        Ok(lease)
    }

    pub async fn renew_retention_lease(
        &self,
        context: RequestContext,
        lease: &RetentionLease,
        ttl_seconds: i64,
    ) -> Result<Option<RetentionLease>> {
        self.renew_retention_lease_at(context, lease, ttl_seconds, Utc::now())
            .await
    }

    pub async fn renew_retention_lease_at(
        &self,
        context: RequestContext,
        lease: &RetentionLease,
        ttl_seconds: i64,
        now: DateTime<Utc>,
    ) -> Result<Option<RetentionLease>> {
        validate_ttl(ttl_seconds)?;
        let mut transaction = self
            .begin(context.with_project_id(lease.project_id))
            .await?;
        let row = sqlx::query(
            r#"
            UPDATE retention_leases
            SET
                heartbeat_at = $6,
                expires_at = $6 + make_interval(secs => $7::double precision)
            WHERE project_id = $1
              AND lease_scope = $2
              AND partition_key = $3
              AND lease_owner = $4
              AND lease_token = $5
              AND expires_at > $6
            RETURNING
                project_id,
                lease_scope,
                partition_key,
                lease_owner,
                lease_token,
                acquired_at,
                heartbeat_at,
                expires_at
            "#,
        )
        .bind(lease.project_id)
        .bind(&lease.lease_scope)
        .bind(&lease.partition_key)
        .bind(lease.lease_owner)
        .bind(lease.lease_token)
        .bind(now)
        .bind(ttl_seconds)
        .fetch_optional(&mut *transaction.transaction)
        .await?;

        let renewed = row.map(|row| retention_lease_from_row(&row)).transpose()?;
        transaction.commit().await?;
        Ok(renewed)
    }

    pub async fn release_retention_lease(
        &self,
        context: RequestContext,
        lease: &RetentionLease,
    ) -> Result<bool> {
        let mut transaction = self
            .begin(context.with_project_id(lease.project_id))
            .await?;
        let result = sqlx::query(
            r#"
            DELETE FROM retention_leases
            WHERE project_id = $1
              AND lease_scope = $2
              AND partition_key = $3
              AND lease_owner = $4
              AND lease_token = $5
            "#,
        )
        .bind(lease.project_id)
        .bind(&lease.lease_scope)
        .bind(&lease.partition_key)
        .bind(lease.lease_owner)
        .bind(lease.lease_token)
        .execute(&mut *transaction.transaction)
        .await?;

        transaction.commit().await?;
        Ok(result.rows_affected() == 1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestContext {
    pub identity_id: Uuid,
    pub device_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
}

impl RequestContext {
    pub fn new(identity_id: Uuid, device_id: Option<Uuid>) -> Self {
        Self {
            identity_id,
            device_id,
            project_id: None,
        }
    }

    pub fn with_project_id(mut self, project_id: Uuid) -> Self {
        self.project_id = Some(project_id);
        self
    }
}

pub struct TransactionContext<'a> {
    transaction: Transaction<'a, Postgres>,
    context: RequestContext,
}

impl<'a> TransactionContext<'a> {
    pub fn context(&self) -> RequestContext {
        self.context
    }

    pub fn transaction(&mut self) -> &mut Transaction<'a, Postgres> {
        &mut self.transaction
    }

    pub async fn commit(self) -> Result<()> {
        self.transaction.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> Result<()> {
        self.transaction.rollback().await?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct AppendSyncEvent {
    pub project_id: Uuid,
    pub stream_id: Uuid,
    pub resource_node_id: Uuid,
    pub base_version: i64,
    pub aggregate_version: i64,
    pub mutation_kind: String,
    pub actor_identity_id: Uuid,
    pub actor_device_id: Uuid,
    pub actor_device_key_version: i32,
    pub device_sequence: i64,
    pub client_event_id: Uuid,
    pub event_kind: String,
    pub key_epoch: i32,
    pub encrypted_payload: Vec<u8>,
    pub previous_hash: Option<Vec<u8>>,
    pub event_hash: Vec<u8>,
    pub signature: Vec<u8>,
    pub post_quantum_signature: Vec<u8>,
    pub client_created_at: DateTime<Utc>,
    pub idempotency_key: Uuid,
    pub request_hash: Vec<u8>,
    pub idempotency_expires_at: DateTime<Utc>,
}

impl AppendSyncEvent {
    fn validate(&self, context: &RequestContext) -> Result<()> {
        if self.actor_identity_id != context.identity_id {
            return Err(StorageError::InvalidInput(
                "sync actor identity must match request context",
            ));
        }
        if context.device_id != Some(self.actor_device_id) {
            return Err(StorageError::InvalidInput(
                "sync actor device must match request context",
            ));
        }
        if self.actor_device_key_version <= 0
            || self.device_sequence <= 0
            || self.key_epoch <= 0
            || self.base_version < 0
            || self.aggregate_version != self.base_version + 1
            || self.stream_id != self.resource_node_id
        {
            return Err(StorageError::InvalidInput(
                "sync key versions and sequences must be positive",
            ));
        }
        if self.event_kind.is_empty() || self.event_kind.len() > 128 {
            return Err(StorageError::InvalidInput(
                "sync event kind must contain 1 to 128 bytes",
            ));
        }
        if !matches!(self.mutation_kind.as_str(), "upsert" | "tombstone") {
            return Err(StorageError::InvalidInput("invalid sync mutation kind"));
        }
        if self.encrypted_payload.is_empty()
            || self.event_hash.len() < 16
            || self.request_hash.len() < 16
            || self.signature.is_empty()
            || self.post_quantum_signature.is_empty()
            || self
                .previous_hash
                .as_ref()
                .is_some_and(|hash| hash.len() < 16)
        {
            return Err(StorageError::InvalidInput(
                "sync cryptographic fields are missing or too short",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncEvent {
    pub id: Uuid,
    pub event_sequence: i64,
    pub project_id: Uuid,
    pub stream_id: Uuid,
    pub resource_node_id: Uuid,
    pub base_version: i64,
    pub aggregate_version: i64,
    pub mutation_kind: String,
    pub actor_identity_id: Uuid,
    pub actor_device_id: Uuid,
    pub actor_device_key_version: i32,
    pub device_sequence: i64,
    pub client_event_id: Uuid,
    pub event_kind: String,
    pub key_epoch: i32,
    pub encrypted_payload: Vec<u8>,
    pub previous_hash: Option<Vec<u8>>,
    pub event_hash: Vec<u8>,
    pub signature: Vec<u8>,
    pub post_quantum_signature: Option<Vec<u8>>,
    pub client_created_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendSyncOutcome {
    pub event: SyncEvent,
    pub projection: SyncProjection,
    pub replayed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncProjection {
    pub project_id: Uuid,
    pub resource_node_id: Uuid,
    pub aggregate_version: i64,
    pub mutation_kind: String,
    pub key_epoch: i32,
    pub encrypted_payload: Vec<u8>,
    pub event_id: Uuid,
    pub event_hash: Vec<u8>,
    pub updated_at: DateTime<Utc>,
}

async fn load_sync_projection(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    resource_node_id: Uuid,
) -> Result<SyncProjection> {
    let row = sqlx::query_as::<_, SyncProjectionRow>(
        r#"
        SELECT
            project_id,
            resource_node_id,
            aggregate_version,
            mutation_kind,
            key_epoch,
            encrypted_payload,
            event_id,
            event_hash,
            updated_at
        FROM sync_current_projections
        WHERE project_id = $1
          AND resource_node_id = $2
        "#,
    )
    .bind(project_id)
    .bind(resource_node_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(row.into())
}

#[derive(sqlx::FromRow)]
struct SyncProjectionRow {
    project_id: Uuid,
    resource_node_id: Uuid,
    aggregate_version: i64,
    mutation_kind: String,
    key_epoch: i32,
    encrypted_payload: Vec<u8>,
    event_id: Uuid,
    event_hash: Vec<u8>,
    updated_at: DateTime<Utc>,
}

impl From<SyncProjectionRow> for SyncProjection {
    fn from(row: SyncProjectionRow) -> Self {
        Self {
            project_id: row.project_id,
            resource_node_id: row.resource_node_id,
            aggregate_version: row.aggregate_version,
            mutation_kind: row.mutation_kind,
            key_epoch: row.key_epoch,
            encrypted_payload: row.encrypted_payload,
            event_id: row.event_id,
            event_hash: row.event_hash,
            updated_at: row.updated_at,
        }
    }
}

fn sync_event_from_row(row: &PgRow) -> Result<SyncEvent> {
    Ok(SyncEvent {
        id: row.try_get("id")?,
        event_sequence: row.try_get("event_sequence")?,
        project_id: row.try_get("project_id")?,
        stream_id: row.try_get("stream_id")?,
        resource_node_id: row.try_get("resource_node_id")?,
        base_version: row.try_get("base_version")?,
        aggregate_version: row.try_get("aggregate_version")?,
        mutation_kind: row.try_get("mutation_kind")?,
        actor_identity_id: row.try_get("actor_identity_id")?,
        actor_device_id: row.try_get("actor_device_id")?,
        actor_device_key_version: row.try_get("actor_device_key_version")?,
        device_sequence: row.try_get("device_sequence")?,
        client_event_id: row.try_get("client_event_id")?,
        event_kind: row.try_get("event_kind")?,
        key_epoch: row.try_get("key_epoch")?,
        encrypted_payload: row.try_get("encrypted_payload")?,
        previous_hash: row.try_get("previous_hash")?,
        event_hash: row.try_get("event_hash")?,
        signature: row.try_get("signature")?,
        post_quantum_signature: row.try_get("post_quantum_signature")?,
        client_created_at: row.try_get("client_created_at")?,
        received_at: row.try_get("received_at")?,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    Topic,
    TaskList,
    Task,
}

impl ResourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Topic => "topic",
            Self::TaskList => "task_list",
            Self::Task => "task",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPermission {
    pub access_level: String,
    pub access_scope: String,
    pub visibility: String,
    pub grant_origin: String,
    pub grant_origin_id: Option<Uuid>,
    pub root_grant_id: Option<Uuid>,
    pub source_scope: String,
}

fn resolved_permission_from_row(row: &PgRow) -> Result<ResolvedPermission> {
    Ok(ResolvedPermission {
        access_level: row.try_get("access_level")?,
        access_scope: row.try_get("access_scope")?,
        visibility: row.try_get("visibility")?,
        grant_origin: row.try_get("grant_origin")?,
        grant_origin_id: row.try_get("grant_origin_id")?,
        root_grant_id: row.try_get("root_grant_id")?,
        source_scope: row.try_get("source_scope")?,
    })
}

const PERMISSION_RESOLUTION_SQL: &str = r#"
WITH target AS (
    SELECT
        topic.id AS topic_id,
        NULL::uuid AS task_list_id,
        NULL::uuid AS task_id,
        CASE
            WHEN topic.visibility = 'inherited' THEN 'project'
            ELSE topic.visibility
        END AS effective_visibility
    FROM topics topic
    WHERE $2 = 'topic'
      AND topic.project_id = $1
      AND topic.id = $3
      AND topic.deleted_at IS NULL

    UNION ALL

    SELECT
        topic.id,
        task_list.id,
        NULL::uuid,
        CASE
            WHEN task_list.visibility <> 'inherited' THEN task_list.visibility
            WHEN topic.visibility <> 'inherited' THEN topic.visibility
            ELSE 'project'
        END
    FROM task_lists task_list
    JOIN topics topic
      ON topic.project_id = task_list.project_id
     AND topic.id = task_list.topic_id
    WHERE $2 = 'task_list'
      AND task_list.project_id = $1
      AND task_list.id = $3
      AND task_list.deleted_at IS NULL
      AND topic.deleted_at IS NULL

    UNION ALL

    SELECT
        topic.id,
        task_list.id,
        task.id,
        CASE
            WHEN task.visibility <> 'inherited' THEN task.visibility
            WHEN task_list.visibility <> 'inherited' THEN task_list.visibility
            WHEN topic.visibility <> 'inherited' THEN topic.visibility
            ELSE 'project'
        END
    FROM tasks task
    JOIN task_lists task_list
      ON task_list.project_id = task.project_id
     AND task_list.id = task.task_list_id
    JOIN topics topic
      ON topic.project_id = task_list.project_id
     AND topic.id = task_list.topic_id
    WHERE $2 = 'task'
      AND task.project_id = $1
      AND task.id = $3
      AND task.deleted_at IS NULL
      AND task_list.deleted_at IS NULL
      AND topic.deleted_at IS NULL
),
candidates AS (
    SELECT
        permission.access_level,
        permission.access_scope,
        permission.visibility,
        permission.grant_origin,
        permission.grant_origin_id,
        permission.root_grant_id,
        'task'::text AS source_scope,
        3 AS specificity,
        CASE permission.access_level
            WHEN 'manage' THEN 4
            WHEN 'edit' THEN 3
            WHEN 'comment' THEN 2
            ELSE 1
        END AS permission_rank
    FROM target
    JOIN task_permissions permission
      ON permission.project_id = $1
     AND permission.task_id = target.task_id
    WHERE permission.member_identity_id = $4
      AND permission.revoked_at IS NULL

    UNION ALL

    SELECT
        permission.access_level,
        permission.access_scope,
        permission.visibility,
        permission.grant_origin,
        permission.grant_origin_id,
        permission.root_grant_id,
        'task_list',
        2,
        CASE permission.access_level
            WHEN 'manage' THEN 4
            WHEN 'edit' THEN 3
            WHEN 'comment' THEN 2
            ELSE 1
        END
    FROM target
    JOIN task_list_permissions permission
      ON permission.project_id = $1
     AND permission.task_list_id = target.task_list_id
    WHERE permission.member_identity_id = $4
      AND permission.revoked_at IS NULL
      AND (
          permission.access_scope = 'full'
          OR ($2 = 'task_list' AND permission.task_list_id = $3)
      )

    UNION ALL

    SELECT
        permission.access_level,
        permission.access_scope,
        permission.visibility,
        permission.grant_origin,
        permission.grant_origin_id,
        permission.root_grant_id,
        'topic',
        1,
        CASE permission.access_level
            WHEN 'manage' THEN 4
            WHEN 'edit' THEN 3
            WHEN 'comment' THEN 2
            ELSE 1
        END
    FROM target
    JOIN topic_permissions permission
      ON permission.project_id = $1
     AND permission.topic_id = target.topic_id
    WHERE permission.member_identity_id = $4
      AND permission.revoked_at IS NULL
      AND (
          permission.access_scope = 'full'
          OR ($2 = 'topic' AND permission.topic_id = $3)
      )

    UNION ALL

    SELECT
        CASE
            WHEN membership.role IN ('owner', 'admin') THEN 'manage'
            ELSE 'view'
        END,
        'full',
        target.effective_visibility,
        'project_role',
        membership.id,
        NULL::uuid,
        'project',
        0,
        CASE
            WHEN membership.role IN ('owner', 'admin') THEN 4
            ELSE 1
        END
    FROM target
    JOIN project_memberships membership
      ON membership.project_id = $1
     AND membership.identity_id = $4
     AND membership.state = 'active'
    WHERE membership.role IN ('owner', 'admin')
       OR target.effective_visibility = 'project'
)
SELECT
    access_level,
    access_scope,
    visibility,
    grant_origin,
    grant_origin_id,
    root_grant_id,
    source_scope
FROM candidates
ORDER BY
    CASE access_scope WHEN 'full' THEN 2 ELSE 1 END DESC,
    permission_rank DESC,
    specificity DESC
LIMIT 1
"#;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ActiveDeviceKey {
    pub identity_id: Uuid,
    pub device_id: Uuid,
    pub key_version: i32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ActiveResourceEpoch {
    pub resource_id: Uuid,
    pub epoch: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceKeyEnvelopeInput {
    pub version: i16,
    pub resource_id: Uuid,
    pub epoch: i32,
    pub key_purpose: String,
    pub recipient_identity_id: Uuid,
    pub recipient_device_id: Uuid,
    pub recipient_device_key_version: i32,
    pub sender_device_key_version: i32,
    pub encrypted_key: Vec<u8>,
    pub sender_signature: Vec<u8>,
    pub sender_post_quantum_signature: Vec<u8>,
}

/// Strictly parses an experimental wrapped key and binds its authenticated
/// resource, recipient device, and epoch metadata to server-known values.
pub fn validate_experimental_wrapped_resource_key(
    encrypted_key: &[u8],
    expected_resource_id: Uuid,
    expected_recipient_device_id: Uuid,
    expected_epoch: i32,
) -> Result<()> {
    let wrapped_key = ExperimentalWrappedResourceKey::from_bytes(encrypted_key)
        .map_err(|_| StorageError::InvalidInput("invalid experimental wrapped resource key"))?;
    if wrapped_key.metadata.resource_id != expected_resource_id
        || wrapped_key.metadata.recipient_device_id != expected_recipient_device_id
        || u64::try_from(expected_epoch).ok() != Some(wrapped_key.metadata.resource_epoch)
    {
        return Err(StorageError::InvalidInput(
            "wrapped resource key metadata does not match its storage record",
        ));
    }
    Ok(())
}

/// Validates envelope shape and exact resource/device coverage. Callers must
/// cryptographically verify both signatures before invoking persistence.
pub fn validate_envelope_coverage(
    recipient_identity_id: Uuid,
    sender_device_key_version: i32,
    resources: &[ActiveResourceEpoch],
    devices: &[ActiveDeviceKey],
    envelopes: &[ResourceKeyEnvelopeInput],
) -> Result<()> {
    validate_envelope_coverage_for_purpose(
        recipient_identity_id,
        sender_device_key_version,
        resources,
        devices,
        envelopes,
        "body",
    )
}

/// Validates exact device coverage for one cryptographically distinct key
/// purpose. Mixing body and header envelopes in one coverage set is rejected.
pub fn validate_envelope_coverage_for_purpose(
    recipient_identity_id: Uuid,
    sender_device_key_version: i32,
    resources: &[ActiveResourceEpoch],
    devices: &[ActiveDeviceKey],
    envelopes: &[ResourceKeyEnvelopeInput],
    key_purpose: &str,
) -> Result<()> {
    if sender_device_key_version <= 0
        || !matches!(key_purpose, "body" | "header")
        || resources.is_empty()
        || devices.is_empty()
        || devices
            .iter()
            .any(|device| device.identity_id != recipient_identity_id || device.key_version <= 0)
    {
        return Err(StorageError::InvalidInput(
            "invalid envelope coverage context",
        ));
    }

    let epochs: HashMap<_, _> = resources
        .iter()
        .map(|resource| (resource.resource_id, resource.epoch))
        .collect();
    if epochs.len() != resources.len() || epochs.values().any(|epoch| *epoch <= 0) {
        return Err(StorageError::InvalidInput(
            "resource epochs must be positive and unique",
        ));
    }

    let expected: HashSet<_> = resources
        .iter()
        .flat_map(|resource| {
            devices.iter().map(move |device| {
                (
                    resource.resource_id,
                    resource.epoch,
                    device.device_id,
                    device.key_version,
                )
            })
        })
        .collect();
    let mut supplied = HashSet::with_capacity(envelopes.len());
    for envelope in envelopes {
        let key = (
            envelope.resource_id,
            envelope.epoch,
            envelope.recipient_device_id,
            envelope.recipient_device_key_version,
        );
        validate_experimental_wrapped_resource_key(
            &envelope.encrypted_key,
            envelope.resource_id,
            envelope.recipient_device_id,
            envelope.epoch,
        )?;
        if envelope.version != 1
            || envelope.key_purpose != key_purpose
            || envelope.recipient_identity_id != recipient_identity_id
            || envelope.sender_device_key_version != sender_device_key_version
            || envelope.sender_signature.len() != 64
            || envelope.sender_post_quantum_signature.is_empty()
            || epochs.get(&envelope.resource_id) != Some(&envelope.epoch)
            || !supplied.insert(key)
        {
            return Err(StorageError::InvalidInput(
                "invalid or duplicate resource key envelope",
            ));
        }
    }
    if supplied != expected {
        return Err(StorageError::InvalidInput(
            "resource key envelope coverage is incomplete",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcquireRetentionLease {
    pub project_id: Uuid,
    pub lease_scope: String,
    pub partition_key: String,
    pub lease_owner: Uuid,
    pub ttl_seconds: i64,
}

impl AcquireRetentionLease {
    fn validate(&self) -> Result<()> {
        if self.lease_scope.is_empty() || self.lease_scope.len() > 128 {
            return Err(StorageError::InvalidInput(
                "lease scope must contain 1 to 128 bytes",
            ));
        }
        if self.partition_key.is_empty() || self.partition_key.len() > 256 {
            return Err(StorageError::InvalidInput(
                "partition key must contain 1 to 256 bytes",
            ));
        }
        validate_ttl(self.ttl_seconds)
    }
}

fn validate_ttl(ttl_seconds: i64) -> Result<()> {
    if ttl_seconds <= 0 || ttl_seconds > 86_400 {
        return Err(StorageError::InvalidInput(
            "lease TTL must be between 1 and 86400 seconds",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionLease {
    pub project_id: Uuid,
    pub lease_scope: String,
    pub partition_key: String,
    pub lease_owner: Uuid,
    pub lease_token: Uuid,
    pub acquired_at: DateTime<Utc>,
    pub heartbeat_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

fn retention_lease_from_row(row: &PgRow) -> Result<RetentionLease> {
    Ok(RetentionLease {
        project_id: row.try_get("project_id")?,
        lease_scope: row.try_get("lease_scope")?,
        partition_key: row.try_get("partition_key")?,
        lease_owner: row.try_get("lease_owner")?,
        lease_token: row.try_get("lease_token")?,
        acquired_at: row.try_get("acquired_at")?,
        heartbeat_at: row.try_get("heartbeat_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct RetentionArchiveIntegrity<'a> {
    pub declared_size: i64,
    pub declared_sha256: &'a [u8],
    pub canonical_manifest: &'a [u8],
    pub manifest_signature: &'a [u8],
}

/// Storage and HTTP adapters call this before persisting or serving an
/// archive. Cryptographic signature verification remains the caller's job.
pub fn validate_retention_archive_integrity(
    metadata: RetentionArchiveIntegrity<'_>,
    actual_size: usize,
    actual_sha256: &[u8],
) -> Result<()> {
    if metadata.declared_size <= 0
        || usize::try_from(metadata.declared_size).ok() != Some(actual_size)
        || metadata.declared_sha256.len() != 32
        || actual_sha256.len() != 32
        || metadata.declared_sha256 != actual_sha256
        || metadata.canonical_manifest.is_empty()
        || metadata.manifest_signature.len() != 64
    {
        return Err(StorageError::InvalidInput(
            "retention archive integrity metadata is invalid",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlobLimits {
    pub max_file_bytes: u64,
    pub project_quota_bytes: u64,
}

/// Validates only ciphertext-derived and server-routing fields. Plaintext
/// names, MIME values, and client paths are intentionally absent.
pub fn validate_blob_declaration(
    storage_key: &str,
    ciphertext_size: u64,
    ciphertext_sha256: &[u8],
    project_reserved_bytes: u64,
    limits: BlobLimits,
) -> Result<()> {
    let valid_key = storage_key.strip_suffix(".blob").is_some_and(|stem| {
        stem.len() == 32
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    });
    if !valid_key {
        return Err(StorageError::InvalidInput(
            "blob storage key must be an opaque basename",
        ));
    }
    if ciphertext_size == 0
        || limits.max_file_bytes == 0
        || limits.project_quota_bytes < limits.max_file_bytes
        || ciphertext_size > limits.max_file_bytes
        || ciphertext_sha256.len() != 32
        || project_reserved_bytes
            .checked_add(ciphertext_size)
            .is_none_or(|total| total > limits.project_quota_bytes)
    {
        return Err(StorageError::InvalidInput(
            "blob digest, size, or quota declaration is invalid",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuestionnaireAnswerReference {
    pub question_id: Uuid,
    pub selected_option_ids: Vec<Uuid>,
}

/// Enforces exact version membership and option ownership before persistence.
pub fn validate_questionnaire_references(
    questions: &HashMap<Uuid, HashSet<Uuid>>,
    answers: &[QuestionnaireAnswerReference],
) -> Result<()> {
    let mut answered = HashSet::with_capacity(answers.len());
    for answer in answers {
        let valid_options =
            questions
                .get(&answer.question_id)
                .ok_or(StorageError::InvalidInput(
                    "answer question is outside the pinned version",
                ))?;
        if !answered.insert(answer.question_id) {
            return Err(StorageError::InvalidInput(
                "questionnaire answer is duplicated",
            ));
        }
        let mut selected = HashSet::with_capacity(answer.selected_option_ids.len());
        if answer
            .selected_option_ids
            .iter()
            .any(|option_id| !selected.insert(*option_id) || !valid_options.contains(option_id))
        {
            return Err(StorageError::InvalidInput(
                "answer option is outside its question",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sprout_crypto_protocol::{
        EXPERIMENTAL_HYBRID_SUITE_V1, HybridWrapMetadata, ProtocolVersion, SuiteAuditStatus,
    };

    use super::*;

    fn wrapped_key(resource_id: Uuid, device_id: Uuid, epoch: i32) -> Vec<u8> {
        ExperimentalWrappedResourceKey {
            version: ProtocolVersion::V1,
            suite_version: EXPERIMENTAL_HYBRID_SUITE_V1,
            audit_status: SuiteAuditStatus::ProductionAuditRequired,
            metadata: HybridWrapMetadata::new(
                resource_id,
                device_id,
                u64::try_from(epoch).unwrap(),
                [9; 32],
                b"storage-envelope-test".to_vec(),
            )
            .unwrap(),
            ephemeral_x25519_public_key: [1; 32],
            ml_kem_768_ciphertext: vec![2; 1_088],
            nonce: [3; 12],
            wrapped_resource_key: vec![4; 48],
        }
        .to_bytes()
        .unwrap()
    }

    fn resource_key_envelope(
        recipient: Uuid,
        device: ActiveDeviceKey,
        resource: ActiveResourceEpoch,
    ) -> ResourceKeyEnvelopeInput {
        ResourceKeyEnvelopeInput {
            version: 1,
            resource_id: resource.resource_id,
            epoch: resource.epoch,
            key_purpose: "body".into(),
            recipient_identity_id: recipient,
            recipient_device_id: device.device_id,
            recipient_device_key_version: device.key_version,
            sender_device_key_version: 7,
            encrypted_key: wrapped_key(resource.resource_id, device.device_id, resource.epoch),
            sender_signature: vec![2; 64],
            sender_post_quantum_signature: vec![3; 3_309],
        }
    }

    #[test]
    fn resource_kind_uses_database_discriminants() {
        assert_eq!(ResourceKind::Topic.as_str(), "topic");
        assert_eq!(ResourceKind::TaskList.as_str(), "task_list");
        assert_eq!(ResourceKind::Task.as_str(), "task");
    }

    #[test]
    fn rejects_unbounded_lease_ttls() {
        assert!(validate_ttl(0).is_err());
        assert!(validate_ttl(86_401).is_err());
        assert!(validate_ttl(60).is_ok());
    }

    #[test]
    fn llr_08_5_archive_storage_rejects_partial_or_corrupt_packages() {
        let digest = [7_u8; 32];
        let metadata = RetentionArchiveIntegrity {
            declared_size: 4,
            declared_sha256: &digest,
            canonical_manifest: b"{\"version\":1}",
            manifest_signature: &[8; 64],
        };
        assert!(validate_retention_archive_integrity(metadata, 4, &digest).is_ok());
        assert!(validate_retention_archive_integrity(metadata, 3, &digest).is_err());
        assert!(validate_retention_archive_integrity(metadata, 4, &[9; 32]).is_err());
    }

    #[test]
    fn envelope_coverage_requires_every_resource_device_pair() {
        let recipient = Uuid::new_v4();
        let device_a = ActiveDeviceKey {
            identity_id: recipient,
            device_id: Uuid::new_v4(),
            key_version: 1,
        };
        let device_b = ActiveDeviceKey {
            identity_id: recipient,
            device_id: Uuid::new_v4(),
            key_version: 2,
        };
        let resource = ActiveResourceEpoch {
            resource_id: Uuid::new_v4(),
            epoch: 3,
        };

        assert!(
            validate_envelope_coverage(
                recipient,
                7,
                &[resource],
                &[device_a, device_b],
                &[
                    resource_key_envelope(recipient, device_a, resource),
                    resource_key_envelope(recipient, device_b, resource),
                ],
            )
            .is_ok()
        );
        assert!(
            validate_envelope_coverage(
                recipient,
                7,
                &[resource],
                &[device_a, device_b],
                &[resource_key_envelope(recipient, device_a, resource)],
            )
            .is_err()
        );
    }

    #[test]
    fn envelope_shape_validation_does_not_claim_signature_verification() {
        let recipient = Uuid::new_v4();
        let device = ActiveDeviceKey {
            identity_id: recipient,
            device_id: Uuid::new_v4(),
            key_version: 1,
        };
        let resource = ActiveResourceEpoch {
            resource_id: Uuid::new_v4(),
            epoch: 1,
        };
        let mut malformed = resource_key_envelope(recipient, device, resource);
        malformed.sender_device_key_version = 1;
        malformed.sender_signature.truncate(63);
        assert!(
            validate_envelope_coverage(recipient, 1, &[resource], &[device], &[malformed],)
                .is_err()
        );
    }

    #[test]
    fn wrapped_resource_key_must_parse_and_match_outer_metadata() {
        let recipient = Uuid::new_v4();
        let device = ActiveDeviceKey {
            identity_id: recipient,
            device_id: Uuid::new_v4(),
            key_version: 1,
        };
        let resource = ActiveResourceEpoch {
            resource_id: Uuid::new_v4(),
            epoch: 3,
        };
        let valid = resource_key_envelope(recipient, device, resource);
        assert!(
            validate_envelope_coverage(recipient, 7, &[resource], &[device], &[valid.clone()])
                .is_ok()
        );

        let mut malformed = valid.clone();
        malformed.encrypted_key = vec![0; valid.encrypted_key.len()];
        assert!(
            validate_envelope_coverage(recipient, 7, &[resource], &[device], &[malformed]).is_err()
        );

        for encrypted_key in [
            wrapped_key(Uuid::new_v4(), device.device_id, resource.epoch),
            wrapped_key(resource.resource_id, Uuid::new_v4(), resource.epoch),
            wrapped_key(resource.resource_id, device.device_id, resource.epoch + 1),
        ] {
            let mut mismatched = valid.clone();
            mismatched.encrypted_key = encrypted_key;
            assert!(
                validate_envelope_coverage(recipient, 7, &[resource], &[device], &[mismatched],)
                    .is_err()
            );
        }
    }

    #[test]
    fn llr_05_4_blob_declarations_reject_paths_hash_mismatch_and_quota_overflow() {
        let limits = BlobLimits {
            max_file_bytes: 100,
            project_quota_bytes: 200,
        };
        let key = "0123456789abcdef0123456789abcdef.blob";
        assert!(validate_blob_declaration(key, 50, &[1; 32], 100, limits).is_ok());
        assert!(validate_blob_declaration("../../secret.blob", 50, &[1; 32], 0, limits).is_err());
        assert!(validate_blob_declaration(key, 50, &[1; 31], 0, limits).is_err());
        assert!(validate_blob_declaration(key, 50, &[1; 32], 175, limits).is_err());
    }

    #[test]
    fn llr_04_3_question_and_option_membership_is_exact() {
        let question = Uuid::new_v4();
        let option = Uuid::new_v4();
        let questions = HashMap::from([(question, HashSet::from([option]))]);
        assert!(
            validate_questionnaire_references(
                &questions,
                &[QuestionnaireAnswerReference {
                    question_id: question,
                    selected_option_ids: vec![option],
                }],
            )
            .is_ok()
        );
        assert!(
            validate_questionnaire_references(
                &questions,
                &[QuestionnaireAnswerReference {
                    question_id: question,
                    selected_option_ids: vec![Uuid::new_v4()],
                }],
            )
            .is_err()
        );
    }
}
