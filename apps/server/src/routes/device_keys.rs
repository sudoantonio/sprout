use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sprout_crypto_protocol::{
    DevicePublicPackage, DeviceSuiteVersion, KeyAlgorithm, PublicKeyDescriptor,
    verify_ed25519_ml_dsa65_signatures,
};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthSession, ProjectAccess, require_project_access, set_database_context},
    error::AppError,
};

const ROTATION_CONTEXT: &[u8] = b"sprout-device-key-rotation-v1";
const REVOCATION_CONTEXT: &[u8] = b"sprout-device-key-revocation-v1";

#[derive(Deserialize)]
pub struct RegisterDeviceKeyPackage {
    package_b64: String,
    previous_classical_signature_b64: Option<String>,
    previous_post_quantum_signature_b64: Option<String>,
}

#[derive(Serialize)]
pub struct DeviceKeyPackageStatus {
    device_id: Uuid,
    key_version: i32,
    generation: i64,
    package_hash_b64: String,
    status: &'static str,
    suite_status: &'static str,
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(device_id): Path<Uuid>,
    Json(request): Json<RegisterDeviceKeyPackage>,
) -> Result<Json<DeviceKeyPackageStatus>, AppError> {
    if device_id != actor.device_id {
        return Err(AppError::Forbidden);
    }
    let package_bytes = decode(&request.package_b64)?;
    let package = DevicePublicPackage::from_json(&package_bytes)
        .map_err(|_| AppError::BadRequest("invalid device key package"))?;
    if package.device_id != device_id
        || package.suite != DeviceSuiteVersion::ExperimentalIndependentKeysV1
        || package.to_canonical_json().ok().as_deref() != Some(package_bytes.as_slice())
    {
        return Err(AppError::BadRequest("invalid device key package"));
    }
    let x25519 = key(&package.encryption_keys, KeyAlgorithm::X25519)?;
    let ml_kem = key(&package.encryption_keys, KeyAlgorithm::MlKem768Experimental)?;
    let ed25519 = key(&package.signing_keys, KeyAlgorithm::Ed25519)?;
    let ml_dsa = key(&package.signing_keys, KeyAlgorithm::MlDsa65Experimental)?;
    let package_hash: [u8; 32] = Sha256::digest(&package_bytes).into();

    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 12))")
        .bind(device_id)
        .execute(&mut *transaction)
        .await?;
    let device_owner = sqlx::query_scalar::<_, Uuid>(
        "SELECT identity_id FROM devices WHERE id = $1 AND retired_at IS NULL FOR UPDATE",
    )
    .bind(device_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if device_owner != actor.identity_id {
        return Err(AppError::Forbidden);
    }
    let previous = sqlx::query_as::<_, ActivePackage>(
        r#"
        SELECT key_version, generation, package_hash,
               ed25519_public_key, ml_dsa_65_public_key
        FROM device_keys
        WHERE identity_id = $1 AND device_id = $2 AND revoked_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(actor.identity_id)
    .bind(device_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let (key_version, event_kind, classical_signature, post_quantum_signature) =
        match (package.generation, previous) {
            (0, None) if package.previous_hash == [0; 32] => {
                if request.previous_classical_signature_b64.is_some()
                    || request.previous_post_quantum_signature_b64.is_some()
                {
                    return Err(AppError::BadRequest(
                        "initial package must not contain rotation signatures",
                    ));
                }
                (1, "registered", None, None)
            }
            (generation, Some(previous))
                if generation
                    == u64::try_from(previous.generation)
                        .map_err(|_| AppError::Internal)?
                        .saturating_add(1)
                    && package.previous_hash.as_slice() == previous.package_hash =>
            {
                let classical = decode_required(
                    request.previous_classical_signature_b64.as_deref(),
                    "rotation requires both signatures",
                )?;
                let post_quantum = decode_required(
                    request.previous_post_quantum_signature_b64.as_deref(),
                    "rotation requires both signatures",
                )?;
                let previous_pq = previous.ml_dsa_65_public_key.ok_or(AppError::BadRequest(
                    "legacy keys cannot authorize rotation",
                ))?;
                verify_ed25519_ml_dsa65_signatures(
                    &previous.ed25519_public_key,
                    &classical,
                    &previous_pq,
                    &post_quantum,
                    &package_bytes,
                    ROTATION_CONTEXT,
                )
                .map_err(|_| AppError::BadRequest("device key rotation signature failed"))?;
                sqlx::query(
                    "UPDATE device_keys SET revoked_at = clock_timestamp()
                     WHERE identity_id = $1 AND device_id = $2 AND key_version = $3",
                )
                .bind(actor.identity_id)
                .bind(device_id)
                .bind(previous.key_version)
                .execute(&mut *transaction)
                .await?;
                (
                    previous
                        .key_version
                        .checked_add(1)
                        .ok_or(AppError::BadRequest("device key version exhausted"))?,
                    "rotated",
                    Some(classical),
                    Some(post_quantum),
                )
            }
            _ => {
                return Err(AppError::Conflict);
            }
        };
    let generation = i64::try_from(package.generation)
        .map_err(|_| AppError::BadRequest("generation too large"))?;
    sqlx::query(
        r#"
        INSERT INTO device_keys (
            identity_id, device_id, key_version,
            encryption_public_key, signing_public_key,
            suite_version, generation, previous_package_hash,
            package_hash, package_json,
            x25519_key_id, ml_kem_768_key_id, ed25519_key_id, ml_dsa_65_key_id,
            x25519_public_key, ml_kem_768_public_key,
            ed25519_public_key, ml_dsa_65_public_key
        )
        VALUES (
            $1, $2, $3, $4, $5, 32769, $6, $7, $8, $9,
            $10, $11, $12, $13, $4, $14, $5, $15
        )
        "#,
    )
    .bind(actor.identity_id)
    .bind(device_id)
    .bind(key_version)
    .bind(&x25519.public_key)
    .bind(&ed25519.public_key)
    .bind(generation)
    .bind(package.previous_hash.as_slice())
    .bind(package_hash.as_slice())
    .bind(&package_bytes)
    .bind(x25519.key_id)
    .bind(ml_kem.key_id)
    .bind(ed25519.key_id)
    .bind(ml_dsa.key_id)
    .bind(&ml_kem.public_key)
    .bind(&ml_dsa.public_key)
    .execute(&mut *transaction)
    .await?;
    append_transparency(
        &mut transaction,
        actor,
        TransparencyAppend {
            key_version,
            generation,
            event_kind,
            package_hash: &package_hash,
            classical_signature: classical_signature.as_deref(),
            post_quantum_signature: post_quantum_signature.as_deref(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(DeviceKeyPackageStatus {
        device_id,
        key_version,
        generation,
        package_hash_b64: encode(&package_hash),
        status: "active",
        suite_status: "experimental_not_production_approved",
    }))
}

#[derive(FromRow)]
struct ActivePackage {
    key_version: i32,
    generation: i64,
    package_hash: Vec<u8>,
    ed25519_public_key: Vec<u8>,
    ml_dsa_65_public_key: Option<Vec<u8>>,
}

#[derive(Serialize)]
pub struct DeviceKeyPackageView {
    device_id: Uuid,
    key_version: i32,
    generation: i64,
    package_b64: String,
    package_hash_b64: String,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    suite_status: &'static str,
}

#[derive(FromRow)]
struct PackageRow {
    device_id: Uuid,
    key_version: i32,
    generation: i64,
    package_json: Vec<u8>,
    package_hash: Vec<u8>,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(device_id): Path<Uuid>,
) -> Result<Json<Vec<DeviceKeyPackageView>>, AppError> {
    if device_id != actor.device_id {
        return Err(AppError::Forbidden);
    }
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    let rows = sqlx::query_as::<_, PackageRow>(
        r#"
        SELECT device_id, key_version, generation, package_json,
               package_hash, created_at, revoked_at
        FROM device_keys
        WHERE identity_id = $1 AND device_id = $2 AND suite_version = 32769
        ORDER BY generation
        "#,
    )
    .bind(actor.identity_id)
    .bind(device_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(rows.into_iter().map(package_view).collect()))
}

#[derive(Deserialize)]
pub struct RevokeDeviceKeyPackage {
    classical_signature_b64: String,
    post_quantum_signature_b64: String,
}

pub async fn revoke(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((device_id, key_version)): Path<(Uuid, i32)>,
    Json(request): Json<RevokeDeviceKeyPackage>,
) -> Result<Json<DeviceKeyPackageStatus>, AppError> {
    if device_id != actor.device_id || key_version <= 0 {
        return Err(AppError::Forbidden);
    }
    let classical = decode(&request.classical_signature_b64)?;
    let post_quantum = decode(&request.post_quantum_signature_b64)?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    let package = sqlx::query_as::<_, ActivePackage>(
        r#"
        SELECT key_version, generation, package_hash,
               ed25519_public_key, ml_dsa_65_public_key
        FROM device_keys
        WHERE identity_id = $1 AND device_id = $2 AND key_version = $3
          AND suite_version = 32769 AND revoked_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(actor.identity_id)
    .bind(device_id)
    .bind(key_version)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let message = revocation_signing_bytes(device_id, key_version, &package.package_hash);
    verify_ed25519_ml_dsa65_signatures(
        &package.ed25519_public_key,
        &classical,
        package
            .ml_dsa_65_public_key
            .as_deref()
            .ok_or(AppError::Forbidden)?,
        &post_quantum,
        &message,
        REVOCATION_CONTEXT,
    )
    .map_err(|_| AppError::BadRequest("device key revocation signature failed"))?;
    sqlx::query(
        "UPDATE device_keys SET revoked_at = clock_timestamp()
         WHERE identity_id = $1 AND device_id = $2 AND key_version = $3",
    )
    .bind(actor.identity_id)
    .bind(device_id)
    .bind(key_version)
    .execute(&mut *transaction)
    .await?;
    append_transparency(
        &mut transaction,
        actor,
        TransparencyAppend {
            key_version,
            generation: package.generation,
            event_kind: "revoked",
            package_hash: package
                .package_hash
                .as_slice()
                .try_into()
                .map_err(|_| AppError::Internal)?,
            classical_signature: Some(&classical),
            post_quantum_signature: Some(&post_quantum),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(DeviceKeyPackageStatus {
        device_id,
        key_version,
        generation: package.generation,
        package_hash_b64: encode(&package.package_hash),
        status: "revoked",
        suite_status: "experimental_not_production_approved",
    }))
}

#[derive(Serialize)]
pub struct TransparencyEntry {
    log_sequence: i64,
    key_version: i32,
    generation: i64,
    event_kind: String,
    package_hash_b64: String,
    previous_entry_hash_b64: Option<String>,
    entry_hash_b64: String,
    recorded_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct TransparencyRow {
    log_sequence: i64,
    key_version: i32,
    generation: i64,
    event_kind: String,
    package_hash: Vec<u8>,
    previous_entry_hash: Option<Vec<u8>>,
    entry_hash: Vec<u8>,
    recorded_at: DateTime<Utc>,
}

pub async fn transparency(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(device_id): Path<Uuid>,
) -> Result<Json<Vec<TransparencyEntry>>, AppError> {
    if device_id != actor.device_id {
        return Err(AppError::Forbidden);
    }
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        None,
    )
    .await?;
    let rows = sqlx::query_as::<_, TransparencyRow>(
        r#"
        SELECT log_sequence, key_version, generation, event_kind,
               package_hash, previous_entry_hash, entry_hash, recorded_at
        FROM device_key_transparency_log
        WHERE identity_id = $1 AND device_id = $2
        ORDER BY log_sequence
        "#,
    )
    .bind(actor.identity_id)
    .bind(device_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| TransparencyEntry {
                log_sequence: row.log_sequence,
                key_version: row.key_version,
                generation: row.generation,
                event_kind: row.event_kind,
                package_hash_b64: encode(&row.package_hash),
                previous_entry_hash_b64: row.previous_entry_hash.as_deref().map(encode),
                entry_hash_b64: encode(&row.entry_hash),
                recorded_at: row.recorded_at,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct ProjectDeviceKeyPackage {
    identity_id: Uuid,
    device_id: Uuid,
    key_version: i32,
    generation: i64,
    package_b64: String,
    package_hash_b64: String,
    suite_status: &'static str,
}

pub async fn list_project(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<ProjectDeviceKeyPackage>>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    let rows = sqlx::query_as::<_, (Uuid, Uuid, i32, i64, Vec<u8>, Vec<u8>)>(
        r#"
        SELECT key.identity_id, key.device_id, key.key_version, key.generation,
               key.package_json, key.package_hash
        FROM project_memberships membership
        CROSS JOIN LATERAL sprout_private.active_project_device_keys(
            $1, membership.identity_id
        ) key
        WHERE membership.project_id = $1 AND membership.state = 'active'
        ORDER BY key.identity_id, key.device_id
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(identity_id, device_id, key_version, generation, package, package_hash)| {
                    ProjectDeviceKeyPackage {
                        identity_id,
                        device_id,
                        key_version,
                        generation,
                        package_b64: encode(&package),
                        package_hash_b64: encode(&package_hash),
                        suite_status: "experimental_not_production_approved",
                    }
                },
            )
            .collect(),
    ))
}

struct TransparencyAppend<'a> {
    key_version: i32,
    generation: i64,
    event_kind: &'static str,
    package_hash: &'a [u8; 32],
    classical_signature: Option<&'a [u8]>,
    post_quantum_signature: Option<&'a [u8]>,
}

async fn append_transparency(
    transaction: &mut Transaction<'_, Postgres>,
    actor: AuthSession,
    entry: TransparencyAppend<'_>,
) -> Result<(), AppError> {
    let previous = sqlx::query_scalar::<_, Vec<u8>>(
        r#"
        SELECT entry_hash
        FROM device_key_transparency_log
        WHERE identity_id = $1 AND device_id = $2
        ORDER BY log_sequence DESC
        LIMIT 1
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let mut digest = Sha256::new();
    digest.update(b"sprout-device-key-transparency-v1");
    digest.update(actor.identity_id.as_bytes());
    digest.update(actor.device_id.as_bytes());
    digest.update(entry.key_version.to_be_bytes());
    digest.update(entry.generation.to_be_bytes());
    digest.update(entry.event_kind.as_bytes());
    digest.update(entry.package_hash);
    if let Some(previous) = &previous {
        digest.update(previous);
    }
    let entry_hash: [u8; 32] = digest.finalize().into();
    sqlx::query(
        r#"
        INSERT INTO device_key_transparency_log (
            identity_id, device_id, key_version, generation, event_kind,
            package_hash, previous_entry_hash, entry_hash,
            classical_signature, post_quantum_signature
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(actor.identity_id)
    .bind(actor.device_id)
    .bind(entry.key_version)
    .bind(entry.generation)
    .bind(entry.event_kind)
    .bind(entry.package_hash.as_slice())
    .bind(previous)
    .bind(entry_hash.as_slice())
    .bind(entry.classical_signature)
    .bind(entry.post_quantum_signature)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn package_view(row: PackageRow) -> DeviceKeyPackageView {
    DeviceKeyPackageView {
        device_id: row.device_id,
        key_version: row.key_version,
        generation: row.generation,
        package_b64: encode(&row.package_json),
        package_hash_b64: encode(&row.package_hash),
        created_at: row.created_at,
        revoked_at: row.revoked_at,
        suite_status: "experimental_not_production_approved",
    }
}

fn key(
    keys: &[PublicKeyDescriptor],
    algorithm: KeyAlgorithm,
) -> Result<&PublicKeyDescriptor, AppError> {
    let mut matching = keys.iter().filter(|key| key.algorithm == algorithm);
    let key = matching
        .next()
        .ok_or(AppError::BadRequest("device package key is missing"))?;
    if matching.next().is_some() {
        return Err(AppError::BadRequest("device package key is duplicated"));
    }
    Ok(key)
}

fn revocation_signing_bytes(device_id: Uuid, key_version: i32, package_hash: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(96);
    message.extend_from_slice(b"sprout-device-key-revocation-v1");
    message.extend_from_slice(device_id.as_bytes());
    message.extend_from_slice(&key_version.to_be_bytes());
    message.extend_from_slice(package_hash);
    message
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 key material"))
}

fn decode_required(value: Option<&str>, message: &'static str) -> Result<Vec<u8>, AppError> {
    value
        .map(decode)
        .transpose()?
        .ok_or(AppError::BadRequest(message))
}

fn encode(value: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}
