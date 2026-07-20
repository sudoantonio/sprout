//! Typed `wasm-bindgen` boundary for the Sprout crypto protocol.
//!
//! [`encrypt`] returns the fresh DEK separately from the encrypted payload. The
//! caller must immediately wrap that DEK for authorized recipients and must
//! never serialize it beside the payload. Exported secret-bearing objects own
//! zeroizing Rust buffers and provide an explicit `destroy()` method; copies
//! returned to JavaScript must also be cleared by the caller.

use std::fmt;

use sprout_crypto_protocol::{
    CanonicalHeader, DataEncryptionKey, DeviceSuiteVersion, EncryptedPayload,
    ExperimentalWrappedResourceKey, HybridWrapMetadata, KeyAlgorithm, ProtocolError, RecoveryShare,
    ResourceKey, SuiteAuditStatus, canonical_header_from_parts,
    combine_recovery_secret_bundle_n_of_n, generate_experimental_device_package_from_bytes,
    hash_bytes, open_payload, pack_recovery_shares, rotate_resource_epoch_from_bytes, seal_payload,
    sign_ed25519_ml_dsa65, split_recovery_secret_n_of_n, unpack_recovery_shares,
    unwrap_resource_key, verify_ed25519_ml_dsa65_signatures, wrap_resource_key,
};
use wasm_bindgen::prelude::*;

/// A stable error code and non-secret diagnostic message for foreign callers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[wasm_bindgen]
pub struct CryptoError {
    code: &'static str,
    message: String,
}

/// Native-facing alias retained for Rust callers.
pub type BoundaryError = CryptoError;

impl CryptoError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Human-readable diagnostic. It never includes key or plaintext bytes.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CryptoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CryptoError {}

impl From<ProtocolError> for CryptoError {
    fn from(error: ProtocolError) -> Self {
        let (code, message) = match error {
            ProtocolError::SizeLimit { .. } => {
                ("size_limit", "input exceeds a protocol size limit")
            }
            ProtocolError::InvalidLength { .. } => {
                ("invalid_length", "input has an invalid length")
            }
            ProtocolError::InvalidFormat(_) => ("invalid_format", "input is malformed"),
            ProtocolError::UnsupportedVersion(_) => {
                ("unsupported_version", "protocol version is unsupported")
            }
            ProtocolError::UnsupportedAlgorithm => (
                "unsupported_algorithm",
                "algorithm is unsupported in this context",
            ),
            ProtocolError::RandomnessUnavailable => {
                ("randomness_unavailable", "secure randomness is unavailable")
            }
            ProtocolError::AuthenticationFailed => {
                ("authentication_failed", "ciphertext authentication failed")
            }
            ProtocolError::ContextMismatch => {
                ("context_mismatch", "authenticated context does not match")
            }
            ProtocolError::InvalidHashChain => ("invalid_hash_chain", "hash chain is invalid"),
            ProtocolError::SignatureVerification => {
                ("signature_verification", "signature verification failed")
            }
            ProtocolError::InvalidRecoveryApproval => (
                "invalid_recovery_approval",
                "recovery approval is invalid or replayed",
            ),
            ProtocolError::RecoveryExpired => ("recovery_expired", "recovery ceremony has expired"),
            ProtocolError::RecoveryIncomplete => (
                "recovery_incomplete",
                "recovery ceremony lacks required approvals",
            ),
            ProtocolError::RecoveryConsumed => (
                "recovery_consumed",
                "recovery ceremony was already consumed",
            ),
            ProtocolError::ProductionAuditRequired => (
                "production_audit_required",
                "operation requires an independently audited production suite",
            ),
            ProtocolError::Json(_) => ("invalid_json", "JSON input is invalid"),
        };
        Self {
            code,
            message: message.to_owned(),
        }
    }
}

#[wasm_bindgen]
impl CryptoError {
    /// Stable machine-readable error code.
    #[wasm_bindgen(getter, js_name = code)]
    pub fn js_code(&self) -> String {
        self.code.to_owned()
    }

    /// Redacted non-secret diagnostic.
    #[wasm_bindgen(getter, js_name = message)]
    pub fn js_message(&self) -> String {
        self.message.clone()
    }
}

/// Fresh encryption output. The DEK is zeroed when this value is dropped.
#[wasm_bindgen(js_name = EncryptionResult)]
pub struct EncryptionResult {
    dek: Vec<u8>,
    payload: Vec<u8>,
}

impl EncryptionResult {
    /// Fresh 32-byte DEK. Wrap it immediately and do not persist it in clear.
    pub fn dek(&self) -> &[u8] {
        &self.dek
    }

    /// Strict binary [`EncryptedPayload`] representation.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

impl Drop for EncryptionResult {
    fn drop(&mut self) {
        self.dek.fill(0);
    }
}

#[wasm_bindgen]
impl EncryptionResult {
    /// Copy the fresh DEK into a JavaScript `Uint8Array`.
    #[wasm_bindgen(getter, js_name = dek)]
    pub fn js_dek(&self) -> Vec<u8> {
        self.dek.clone()
    }

    /// Copy the strict encrypted payload into a JavaScript `Uint8Array`.
    #[wasm_bindgen(getter, js_name = payload)]
    pub fn js_payload(&self) -> Vec<u8> {
        self.payload.clone()
    }

    /// Immediately zero the Rust-owned DEK copy.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.dek.fill(0);
        self.dek.clear();
    }
}

/// Install the browser-console panic hook once.
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Install the panic hook automatically when the module loads.
#[wasm_bindgen(start)]
pub fn wasm_start() {
    init_panic_hook();
}

/// Explicit initialization export for hosts that prefer deterministic setup.
#[wasm_bindgen(js_name = initialize)]
pub fn wasm_initialize() {
    init_panic_hook();
}

/// Return SHA-256 bytes.
pub fn hash(input: &[u8]) -> Vec<u8> {
    init_panic_hook();
    hash_bytes(input).to_vec()
}

/// SHA-256 export using `Uint8Array` input and output.
#[wasm_bindgen(js_name = hash)]
pub fn wasm_hash(input: &[u8]) -> Vec<u8> {
    hash(input)
}

/// Build the canonical authenticated header from foreign-language primitives.
#[allow(clippy::too_many_arguments)]
pub fn canonical_header(
    version: u8,
    suite: u8,
    kind: u8,
    resource_id: &[u8],
    key_id: &[u8],
    sequence: u64,
    previous_hash: &[u8],
    context: &[u8],
) -> Result<Vec<u8>, BoundaryError> {
    init_panic_hook();
    canonical_header_from_parts(
        version,
        suite,
        kind,
        resource_id,
        key_id,
        sequence,
        previous_hash,
        context,
    )
    .map_err(Into::into)
}

/// Build canonical authenticated-header bytes from typed arrays.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen(js_name = canonicalHeader)]
pub fn wasm_canonical_header(
    version: u8,
    suite: u8,
    kind: u8,
    resource_id: &[u8],
    key_id: &[u8],
    sequence: u64,
    previous_hash: &[u8],
    context: &[u8],
) -> Result<Vec<u8>, BoundaryError> {
    canonical_header(
        version,
        suite,
        kind,
        resource_id,
        key_id,
        sequence,
        previous_hash,
        context,
    )
}

/// Encrypt with a fresh 32-byte DEK and a fresh 12-byte GCM nonce.
///
/// `canonical_header_bytes` must be produced by [`canonical_header`] or be an
/// equivalent strict V1 encoding.
pub fn encrypt(
    canonical_header_bytes: &[u8],
    plaintext: &[u8],
) -> Result<EncryptionResult, BoundaryError> {
    init_panic_hook();
    let header = CanonicalHeader::from_canonical_bytes(canonical_header_bytes)?;
    let sealed = seal_payload(header, plaintext)?;
    Ok(EncryptionResult {
        dek: sealed.dek.as_bytes().to_vec(),
        payload: sealed.payload.to_bytes()?,
    })
}

/// Encrypt from JavaScript and return a typed secret-bearing result object.
#[wasm_bindgen(js_name = encrypt)]
pub fn wasm_encrypt(
    canonical_header_bytes: &[u8],
    plaintext: &[u8],
) -> Result<EncryptionResult, BoundaryError> {
    encrypt(canonical_header_bytes, plaintext)
}

/// Authenticate and decrypt using an independently expected canonical header.
///
/// Requiring the expected header at this boundary prevents a caller from
/// accidentally trusting attacker-selected context embedded beside a valid
/// ciphertext.
pub fn decrypt(
    dek: &[u8],
    encrypted_payload_bytes: &[u8],
    expected_header_bytes: &[u8],
) -> Result<Vec<u8>, BoundaryError> {
    init_panic_hook();
    let dek = DataEncryptionKey::from_slice(dek)?;
    let payload = EncryptedPayload::from_bytes(encrypted_payload_bytes)?;
    let expected_header = CanonicalHeader::from_canonical_bytes(expected_header_bytes)?;
    open_payload(&dek, &payload, &expected_header).map_err(Into::into)
}

/// Authenticate and decrypt typed-array inputs.
#[wasm_bindgen(js_name = decrypt)]
pub fn wasm_decrypt(
    dek: &[u8],
    encrypted_payload_bytes: &[u8],
    expected_header_bytes: &[u8],
) -> Result<Vec<u8>, BoundaryError> {
    decrypt(dek, encrypted_payload_bytes, expected_header_bytes)
}

/// Generated experimental device package with separately accessible key bytes.
///
/// This object contains four independent real key pairs. It does not implement
/// X-Wing or another hybrid secret combiner.
#[wasm_bindgen(js_name = DevicePackageResult)]
pub struct DevicePackageResult {
    suite_version: u16,
    public_package: Vec<u8>,
    x25519_public_key: Vec<u8>,
    ml_kem_768_public_key: Vec<u8>,
    ed25519_public_key: Vec<u8>,
    ml_dsa_65_public_key: Vec<u8>,
    x25519_private_key: Vec<u8>,
    ml_kem_768_private_key: Vec<u8>,
    ed25519_private_key: Vec<u8>,
    ml_dsa_65_private_key: Vec<u8>,
}

impl Drop for DevicePackageResult {
    fn drop(&mut self) {
        self.zero_private_fields();
    }
}

impl DevicePackageResult {
    fn zero_private_fields(&mut self) {
        self.x25519_private_key.fill(0);
        self.ml_kem_768_private_key.fill(0);
        self.ed25519_private_key.fill(0);
        self.ml_dsa_65_private_key.fill(0);
    }
}

#[wasm_bindgen]
impl DevicePackageResult {
    /// Explicit experimental suite version (`0x8001`).
    #[wasm_bindgen(getter, js_name = suiteVersion)]
    pub fn suite_version(&self) -> u16 {
        self.suite_version
    }

    /// Canonical JSON public package as a UTF-8 `Uint8Array`.
    #[wasm_bindgen(getter, js_name = publicPackage)]
    pub fn public_package(&self) -> Vec<u8> {
        self.public_package.clone()
    }

    /// X25519 public key.
    #[wasm_bindgen(getter, js_name = x25519PublicKey)]
    pub fn x25519_public_key(&self) -> Vec<u8> {
        self.x25519_public_key.clone()
    }

    /// ML-KEM-768 encapsulation key.
    #[wasm_bindgen(getter, js_name = mlKem768PublicKey)]
    pub fn ml_kem_768_public_key(&self) -> Vec<u8> {
        self.ml_kem_768_public_key.clone()
    }

    /// Ed25519 verification key.
    #[wasm_bindgen(getter, js_name = ed25519PublicKey)]
    pub fn ed25519_public_key(&self) -> Vec<u8> {
        self.ed25519_public_key.clone()
    }

    /// ML-DSA-65 verification key.
    #[wasm_bindgen(getter, js_name = mlDsa65PublicKey)]
    pub fn ml_dsa_65_public_key(&self) -> Vec<u8> {
        self.ml_dsa_65_public_key.clone()
    }

    /// X25519 static secret. Clear the returned JavaScript copy after import.
    #[wasm_bindgen(getter, js_name = x25519PrivateKey)]
    pub fn x25519_private_key(&self) -> Vec<u8> {
        self.x25519_private_key.clone()
    }

    /// ML-KEM-768 decapsulation key. Clear the JavaScript copy after import.
    #[wasm_bindgen(getter, js_name = mlKem768PrivateKey)]
    pub fn ml_kem_768_private_key(&self) -> Vec<u8> {
        self.ml_kem_768_private_key.clone()
    }

    /// Ed25519 signing seed. Clear the JavaScript copy after import.
    #[wasm_bindgen(getter, js_name = ed25519PrivateKey)]
    pub fn ed25519_private_key(&self) -> Vec<u8> {
        self.ed25519_private_key.clone()
    }

    /// ML-DSA-65 signing key. Clear the JavaScript copy after import.
    #[wasm_bindgen(getter, js_name = mlDsa65PrivateKey)]
    pub fn ml_dsa_65_private_key(&self) -> Vec<u8> {
        self.ml_dsa_65_private_key.clone()
    }

    /// Zero every Rust-owned private key buffer.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.zero_private_fields();
        self.x25519_private_key.clear();
        self.ml_kem_768_private_key.clear();
        self.ed25519_private_key.clear();
        self.ml_dsa_65_private_key.clear();
    }
}

/// Generate the experimental independent-key device suite.
pub fn generate_device_package(
    device_id: &[u8],
    x25519_key_id: &[u8],
    ml_kem_768_key_id: &[u8],
    ed25519_key_id: &[u8],
    ml_dsa_65_key_id: &[u8],
) -> Result<DevicePackageResult, BoundaryError> {
    init_panic_hook();
    let generated = generate_experimental_device_package_from_bytes(
        device_id,
        x25519_key_id,
        ml_kem_768_key_id,
        ed25519_key_id,
        ml_dsa_65_key_id,
    )?;
    let package = generated.public_package();
    let public_key = |algorithm| {
        package
            .encryption_keys
            .iter()
            .chain(&package.signing_keys)
            .find(|key| key.algorithm == algorithm)
            .map(|key| key.public_key.clone())
            .ok_or(ProtocolError::InvalidFormat("generated device key missing"))
    };
    let private = generated.private_keys();
    Ok(DevicePackageResult {
        suite_version: DeviceSuiteVersion::ExperimentalIndependentKeysV1 as u16,
        public_package: package.to_canonical_json()?,
        x25519_public_key: public_key(KeyAlgorithm::X25519)?,
        ml_kem_768_public_key: public_key(KeyAlgorithm::MlKem768Experimental)?,
        ed25519_public_key: public_key(KeyAlgorithm::Ed25519)?,
        ml_dsa_65_public_key: public_key(KeyAlgorithm::MlDsa65Experimental)?,
        x25519_private_key: private.x25519().to_vec(),
        ml_kem_768_private_key: private.ml_kem_768().to_vec(),
        ed25519_private_key: private.ed25519().to_vec(),
        ml_dsa_65_private_key: private.ml_dsa_65().to_vec(),
    })
}

/// Generate four real key pairs from typed-array identifiers.
#[wasm_bindgen(js_name = generateDevicePackage)]
pub fn wasm_generate_device_package(
    device_id: &[u8],
    x25519_key_id: &[u8],
    ml_kem_768_key_id: &[u8],
    ed25519_key_id: &[u8],
    ml_dsa_65_key_id: &[u8],
) -> Result<DevicePackageResult, BoundaryError> {
    generate_device_package(
        device_id,
        x25519_key_id,
        ml_kem_768_key_id,
        ed25519_key_id,
        ml_dsa_65_key_id,
    )
}

/// Independent Ed25519 and ML-DSA-65 signature result.
#[wasm_bindgen(js_name = DualSignatureResult)]
pub struct DualSignatureResult {
    ed25519: Vec<u8>,
    ml_dsa_65: Vec<u8>,
}

impl Drop for DualSignatureResult {
    fn drop(&mut self) {
        self.ed25519.fill(0);
        self.ml_dsa_65.fill(0);
    }
}

#[wasm_bindgen]
impl DualSignatureResult {
    /// Ed25519 signature as a `Uint8Array`.
    #[wasm_bindgen(getter, js_name = ed25519)]
    pub fn ed25519(&self) -> Vec<u8> {
        self.ed25519.clone()
    }

    /// ML-DSA-65 signature as a `Uint8Array`.
    #[wasm_bindgen(getter, js_name = mlDsa65)]
    pub fn ml_dsa_65(&self) -> Vec<u8> {
        self.ml_dsa_65.clone()
    }

    /// Zero both Rust-owned result buffers.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.ed25519.fill(0);
        self.ml_dsa_65.fill(0);
        self.ed25519.clear();
        self.ml_dsa_65.clear();
    }
}

/// Sign one message independently with Ed25519 and ML-DSA-65.
#[wasm_bindgen(js_name = signDual)]
pub fn wasm_sign_dual(
    ed25519_private_key: &[u8],
    ml_dsa_65_private_key: &[u8],
    message: &[u8],
    context: &[u8],
) -> Result<DualSignatureResult, BoundaryError> {
    init_panic_hook();
    let signatures =
        sign_ed25519_ml_dsa65(ed25519_private_key, ml_dsa_65_private_key, message, context)?;
    Ok(DualSignatureResult {
        ed25519: signatures.ed25519().to_vec(),
        ml_dsa_65: signatures.ml_dsa_65().to_vec(),
    })
}

/// Verify that both signatures are valid in the exact supplied context.
#[wasm_bindgen(js_name = verifyDual)]
pub fn wasm_verify_dual(
    ed25519_public_key: &[u8],
    ed25519_signature: &[u8],
    ml_dsa_65_public_key: &[u8],
    ml_dsa_65_signature: &[u8],
    message: &[u8],
    context: &[u8],
) -> Result<bool, BoundaryError> {
    init_panic_hook();
    match verify_ed25519_ml_dsa65_signatures(
        ed25519_public_key,
        ed25519_signature,
        ml_dsa_65_public_key,
        ml_dsa_65_signature,
        message,
        context,
    ) {
        Ok(()) => Ok(true),
        Err(
            ProtocolError::SignatureVerification
            | ProtocolError::InvalidLength { .. }
            | ProtocolError::InvalidFormat(_),
        ) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Audit-gated wrapped resource-key envelope.
#[wasm_bindgen(js_name = WrappedResourceKeyResult)]
pub struct WrappedResourceKeyResult {
    envelope: Vec<u8>,
}

impl Drop for WrappedResourceKeyResult {
    fn drop(&mut self) {
        self.envelope.fill(0);
    }
}

#[wasm_bindgen]
impl WrappedResourceKeyResult {
    /// Versioned envelope as a `Uint8Array`.
    #[wasm_bindgen(getter, js_name = envelope)]
    pub fn envelope(&self) -> Vec<u8> {
        self.envelope.clone()
    }

    /// Non-standard Sprout suite version (`0x8001`).
    #[wasm_bindgen(getter, js_name = suiteVersion)]
    pub fn suite_version(&self) -> u16 {
        sprout_crypto_protocol::EXPERIMENTAL_HYBRID_SUITE_V1
    }

    /// Always `"production_audit_required"` for this experimental suite.
    #[wasm_bindgen(getter, js_name = auditStatus)]
    pub fn audit_status(&self) -> String {
        "production_audit_required".to_owned()
    }

    /// Zero the Rust-owned envelope result.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.envelope.fill(0);
        self.envelope.clear();
    }
}

/// Wrap a 32-byte resource key for one recipient with the versioned
/// experimental X25519 + ML-KEM-768 construction.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen(js_name = wrapResourceKey)]
pub fn wasm_wrap_resource_key(
    resource_key: &[u8],
    recipient_x25519_public_key: &[u8],
    recipient_ml_kem_768_public_key: &[u8],
    resource_id: &[u8],
    recipient_device_id: &[u8],
    resource_epoch: u64,
    previous_epoch_hash: &[u8],
    context: &[u8],
) -> Result<WrappedResourceKeyResult, BoundaryError> {
    init_panic_hook();
    let resource_key = ResourceKey::from_slice(resource_key)?;
    let metadata = HybridWrapMetadata::from_parts_bytes(
        resource_id,
        recipient_device_id,
        resource_epoch,
        previous_epoch_hash,
        context,
    )?;
    let envelope = wrap_resource_key(
        &resource_key,
        recipient_x25519_public_key,
        recipient_ml_kem_768_public_key,
        metadata,
    )?;
    Ok(WrappedResourceKeyResult {
        envelope: envelope.to_bytes()?,
    })
}

/// Zeroizing unwrapped resource key.
#[derive(Debug)]
#[wasm_bindgen(js_name = ResourceKeyResult)]
pub struct ResourceKeyResult {
    resource_key: Vec<u8>,
    audit_status: SuiteAuditStatus,
}

impl Drop for ResourceKeyResult {
    fn drop(&mut self) {
        self.resource_key.fill(0);
    }
}

#[wasm_bindgen]
impl ResourceKeyResult {
    /// Resource key as a `Uint8Array`.
    #[wasm_bindgen(getter, js_name = resourceKey)]
    pub fn resource_key(&self) -> Vec<u8> {
        self.resource_key.clone()
    }

    /// Always `"production_audit_required"` for this experimental suite.
    #[wasm_bindgen(getter, js_name = auditStatus)]
    pub fn audit_status(&self) -> String {
        match self.audit_status {
            SuiteAuditStatus::ProductionAuditRequired => "production_audit_required".to_owned(),
        }
    }

    /// Zero the Rust-owned resource key.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.resource_key.fill(0);
        self.resource_key.clear();
    }
}

/// Unwrap only when independently expected resource metadata matches.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen(js_name = unwrapResourceKey)]
pub fn wasm_unwrap_resource_key(
    envelope: &[u8],
    recipient_x25519_private_key: &[u8],
    recipient_ml_kem_768_private_key: &[u8],
    resource_id: &[u8],
    recipient_device_id: &[u8],
    resource_epoch: u64,
    previous_epoch_hash: &[u8],
    context: &[u8],
) -> Result<ResourceKeyResult, BoundaryError> {
    init_panic_hook();
    let envelope = ExperimentalWrappedResourceKey::from_bytes(envelope)?;
    let expected_metadata = HybridWrapMetadata::from_parts_bytes(
        resource_id,
        recipient_device_id,
        resource_epoch,
        previous_epoch_hash,
        context,
    )?;
    let resource_key = unwrap_resource_key(
        &envelope,
        recipient_x25519_private_key,
        recipient_ml_kem_768_private_key,
        &expected_metadata,
    )?;
    Ok(ResourceKeyResult {
        resource_key: resource_key.as_bytes().to_vec(),
        audit_status: envelope.audit_status,
    })
}

/// Zeroizing bundle of committed n-of-n recovery shares.
#[wasm_bindgen(js_name = RecoverySplitResult)]
pub struct RecoverySplitResult {
    bundle: Vec<u8>,
    share_count: u8,
    commitment: Vec<u8>,
}

impl Drop for RecoverySplitResult {
    fn drop(&mut self) {
        self.bundle.fill(0);
        self.commitment.fill(0);
    }
}

#[wasm_bindgen]
impl RecoverySplitResult {
    /// Number of required shares.
    #[wasm_bindgen(getter, js_name = shareCount)]
    pub fn share_count(&self) -> u8 {
        self.share_count
    }

    /// Commitment to the secret and split context.
    #[wasm_bindgen(getter, js_name = commitment)]
    pub fn commitment(&self) -> Vec<u8> {
        self.commitment.clone()
    }

    /// Return one independently storable encoded share as a `Uint8Array`.
    #[wasm_bindgen(js_name = share)]
    pub fn share(&self, position: u32) -> Result<Vec<u8>, BoundaryError> {
        let shares = unpack_recovery_shares(&self.bundle)?;
        shares
            .get(position as usize)
            .ok_or_else(|| ProtocolError::InvalidFormat("recovery share position").into())
            .and_then(|share| share.to_bytes().map_err(Into::into))
    }

    /// Return the strict bundle accepted by `combineRecoverySecretNOfN`.
    #[wasm_bindgen(getter, js_name = bundle)]
    pub fn bundle(&self) -> Vec<u8> {
        self.bundle.clone()
    }

    /// Zero all Rust-owned encoded shares and commitment bytes.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.bundle.fill(0);
        self.commitment.fill(0);
        self.bundle.clear();
        self.commitment.clear();
    }
}

/// Split a local 32-byte recovery secret into committed XOR n-of-n shares.
#[wasm_bindgen(js_name = splitRecoverySecretNOfN)]
pub fn wasm_split_recovery_secret_n_of_n(
    secret: &[u8],
    participant_count: u8,
    context: &[u8],
) -> Result<RecoverySplitResult, BoundaryError> {
    init_panic_hook();
    let split = split_recovery_secret_n_of_n(secret, participant_count, context)?;
    Ok(RecoverySplitResult {
        bundle: split.bundle()?,
        share_count: split.share_count() as u8,
        commitment: split.secret_commitment().to_vec(),
    })
}

/// Local collector for independently retrieved recovery-share `Uint8Array`s.
#[derive(Default)]
#[wasm_bindgen(js_name = RecoveryShareSet)]
pub struct RecoveryShareSet {
    encoded_shares: Vec<Vec<u8>>,
}

impl Drop for RecoveryShareSet {
    fn drop(&mut self) {
        self.zero_shares();
    }
}

impl RecoveryShareSet {
    fn zero_shares(&mut self) {
        for share in &mut self.encoded_shares {
            share.fill(0);
        }
    }

    fn strict_bundle(&self) -> Result<Vec<u8>, BoundaryError> {
        let shares = self
            .encoded_shares
            .iter()
            .map(|share| RecoveryShare::from_bytes(share))
            .collect::<Result<Vec<_>, _>>()?;
        pack_recovery_shares(&shares).map_err(Into::into)
    }
}

#[wasm_bindgen]
impl RecoveryShareSet {
    /// Create an empty local collector.
    #[wasm_bindgen(constructor)]
    pub fn new() -> RecoveryShareSet {
        RecoveryShareSet::default()
    }

    /// Add one strict encoded recovery share.
    #[wasm_bindgen(js_name = addShare)]
    pub fn add_share(&mut self, encoded_share: &[u8]) -> Result<(), BoundaryError> {
        if self.encoded_shares.len() >= sprout_crypto_protocol::MAX_RECOVERY_PARTICIPANTS {
            return Err(ProtocolError::SizeLimit {
                field: "recovery shares",
                maximum: sprout_crypto_protocol::MAX_RECOVERY_PARTICIPANTS,
            }
            .into());
        }
        RecoveryShare::from_bytes(encoded_share)?;
        self.encoded_shares.push(encoded_share.to_vec());
        Ok(())
    }

    /// Number of collected shares.
    #[wasm_bindgen(getter, js_name = shareCount)]
    pub fn share_count(&self) -> u8 {
        self.encoded_shares.len() as u8
    }

    /// Strict bundle, primarily for local persistence/debugging.
    #[wasm_bindgen(getter, js_name = bundle)]
    pub fn bundle(&self) -> Result<Vec<u8>, BoundaryError> {
        self.strict_bundle()
    }

    /// Zero every Rust-owned share.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.zero_shares();
        self.encoded_shares.clear();
    }
}

/// Zeroizing combined recovery secret.
#[derive(Debug)]
#[wasm_bindgen(js_name = RecoverySecretResult)]
pub struct RecoverySecretResult {
    secret: Vec<u8>,
}

impl Drop for RecoverySecretResult {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

#[wasm_bindgen]
impl RecoverySecretResult {
    /// Recovered secret as a `Uint8Array`.
    #[wasm_bindgen(getter, js_name = secret)]
    pub fn secret(&self) -> Vec<u8> {
        self.secret.clone()
    }

    /// Zero the Rust-owned recovered secret.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.secret.fill(0);
        self.secret.clear();
    }
}

/// Combine individually recollected shares locally. An n-1 set always fails.
#[wasm_bindgen(js_name = combineRecoverySecretNOfN)]
pub fn wasm_combine_recovery_secret_n_of_n(
    share_set: &RecoveryShareSet,
    context: &[u8],
) -> Result<RecoverySecretResult, BoundaryError> {
    init_panic_hook();
    let share_bundle = share_set.strict_bundle()?;
    let secret = combine_recovery_secret_bundle_n_of_n(&share_bundle, context)?;
    Ok(RecoverySecretResult {
        secret: secret.as_bytes().to_vec(),
    })
}

/// Combine a strict all-shares bundle for callers already using that format.
#[wasm_bindgen(js_name = combineRecoverySecretBundleNOfN)]
pub fn wasm_combine_recovery_secret_bundle_n_of_n(
    share_bundle: &[u8],
    context: &[u8],
) -> Result<RecoverySecretResult, BoundaryError> {
    init_panic_hook();
    let secret = combine_recovery_secret_bundle_n_of_n(share_bundle, context)?;
    Ok(RecoverySecretResult {
        secret: secret.as_bytes().to_vec(),
    })
}

/// Fresh zeroizing material for a resource epoch rotation.
#[wasm_bindgen(js_name = ResourceEpochResult)]
pub struct ResourceEpochResult {
    epoch: u64,
    epoch_id: Vec<u8>,
    resource_key: Vec<u8>,
    header_key: Vec<u8>,
    epoch_commitment: Vec<u8>,
    public_metadata: Vec<u8>,
}

impl Drop for ResourceEpochResult {
    fn drop(&mut self) {
        self.resource_key.fill(0);
        self.header_key.fill(0);
    }
}

#[wasm_bindgen]
impl ResourceEpochResult {
    /// New epoch number.
    #[wasm_bindgen(getter, js_name = epoch)]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Random 16-byte epoch identifier.
    #[wasm_bindgen(getter, js_name = epochId)]
    pub fn epoch_id(&self) -> Vec<u8> {
        self.epoch_id.clone()
    }

    /// Fresh resource key.
    #[wasm_bindgen(getter, js_name = resourceKey)]
    pub fn resource_key(&self) -> Vec<u8> {
        self.resource_key.clone()
    }

    /// Fresh domain-separated header key.
    #[wasm_bindgen(getter, js_name = headerKey)]
    pub fn header_key(&self) -> Vec<u8> {
        self.header_key.clone()
    }

    /// Commitment to metadata and both fresh keys.
    #[wasm_bindgen(getter, js_name = epochCommitment)]
    pub fn epoch_commitment(&self) -> Vec<u8> {
        self.epoch_commitment.clone()
    }

    /// Canonical public epoch metadata.
    #[wasm_bindgen(getter, js_name = publicMetadata)]
    pub fn public_metadata(&self) -> Vec<u8> {
        self.public_metadata.clone()
    }

    /// Zero both Rust-owned private keys.
    #[wasm_bindgen(js_name = destroy)]
    pub fn destroy(&mut self) {
        self.resource_key.fill(0);
        self.header_key.fill(0);
        self.resource_key.clear();
        self.header_key.clear();
    }
}

/// Generate fresh resource/header keys and public chain metadata for the next
/// epoch.
#[wasm_bindgen(js_name = rotateResourceEpoch)]
pub fn wasm_rotate_resource_epoch(
    resource_id: &[u8],
    current_epoch: u64,
    previous_epoch_hash: &[u8],
    context: &[u8],
) -> Result<ResourceEpochResult, BoundaryError> {
    init_panic_hook();
    let material =
        rotate_resource_epoch_from_bytes(resource_id, current_epoch, previous_epoch_hash, context)?;
    Ok(ResourceEpochResult {
        epoch: material.epoch,
        epoch_id: material.epoch_id.as_bytes().to_vec(),
        resource_key: material.resource_key().to_vec(),
        header_key: material.header_key().to_vec(),
        epoch_commitment: material.epoch_commitment.to_vec(),
        public_metadata: material.public_metadata_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(context: &[u8]) -> Vec<u8> {
        canonical_header(
            1,
            1,
            1,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2],
            0,
            &[0; 32],
            context,
        )
        .unwrap()
    }

    #[test]
    fn boundary_encrypt_decrypt_round_trip() {
        let header = header(b"wasm/tenant/resource");
        let encrypted = encrypt(&header, b"boundary secret").unwrap();
        assert_eq!(
            decrypt(encrypted.dek(), encrypted.payload(), &header).unwrap(),
            b"boundary secret"
        );
    }

    #[test]
    fn boundary_returns_structured_tamper_error() {
        let header = header(b"wasm/tamper");
        let encrypted = encrypt(&header, b"boundary secret").unwrap();
        let mut tampered = encrypted.payload().to_vec();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        let error = decrypt(encrypted.dek(), &tampered, &header).unwrap_err();
        assert_eq!(error.code(), "authentication_failed");
    }

    #[test]
    fn boundary_rejects_wrong_key_and_context() {
        let expected_header = header(b"wasm/tenant-a");
        let other_header = header(b"wasm/tenant-b");
        let encrypted = encrypt(&expected_header, b"boundary secret").unwrap();

        let error = decrypt(&[9; 32], encrypted.payload(), &expected_header).unwrap_err();
        assert_eq!(error.code(), "authentication_failed");

        let error = decrypt(encrypted.dek(), encrypted.payload(), &other_header).unwrap_err();
        assert_eq!(error.code(), "context_mismatch");
    }

    #[test]
    fn boundary_has_strict_header_and_key_lengths() {
        let error =
            canonical_header(1, 1, 1, &[1; 15], &[2; 16], 0, &[0; 32], b"context").unwrap_err();
        assert_eq!(error.code(), "invalid_length");
        assert_eq!(error.message(), "input has an invalid length");

        let header = header(b"wasm/key");
        let encrypted = encrypt(&header, b"secret").unwrap();
        let error = decrypt(&[1; 31], encrypted.payload(), &header).unwrap_err();
        assert_eq!(error.code(), "invalid_length");
    }

    #[test]
    fn hash_matches_sha_256_abc_vector() {
        assert_eq!(
            hash(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
        assert_ne!(hash(b"sprout"), hash(b"sprout!"));
    }

    #[test]
    fn native_side_matches_cross_runtime_interop_vectors() {
        let previous_hash = hash(b"sprout-wasm-parity-v1");
        assert_eq!(
            previous_hash,
            [
                0x3e, 0x87, 0x9a, 0xde, 0x3d, 0xd7, 0x3a, 0xd3, 0x2d, 0xdb, 0x31, 0xea, 0x61, 0xf4,
                0xd3, 0x72, 0xf8, 0x66, 0x43, 0x47, 0xc5, 0x11, 0xe3, 0x68, 0xc6, 0x1a, 0xb7, 0xe0,
                0x65, 0x43, 0xde, 0x09,
            ]
        );
        let resource_id = std::array::from_fn::<_, 16, _>(|index| index as u8);
        let key_id = std::array::from_fn::<_, 16, _>(|index| (index + 16) as u8);
        let canonical = canonical_header(
            1,
            1,
            4,
            &resource_id,
            &key_id,
            42,
            &previous_hash,
            b"sprout/interop/v1",
        )
        .unwrap();
        assert_eq!(canonical.len(), 102);
        assert_eq!(
            hash(&canonical),
            [
                0xea, 0xe3, 0x44, 0x56, 0xf7, 0xb2, 0xef, 0x19, 0x98, 0x6f, 0xac, 0x6d, 0x56, 0x43,
                0x5d, 0xe8, 0xa1, 0xb3, 0xa1, 0x10, 0x68, 0xfa, 0xa5, 0x42, 0x33, 0x2d, 0x01, 0x17,
                0xf5, 0xa1, 0xfa, 0x8a,
            ]
        );
    }

    #[test]
    fn device_package_exports_real_public_and_private_fields() {
        let mut package = generate_device_package(
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 14],
        )
        .unwrap();
        assert_eq!(
            package.suite_version,
            DeviceSuiteVersion::ExperimentalIndependentKeysV1 as u16
        );
        assert_eq!(package.x25519_public_key.len(), 32);
        assert_eq!(package.ml_kem_768_public_key.len(), 1_184);
        assert_eq!(package.ed25519_public_key.len(), 32);
        assert_eq!(package.ml_dsa_65_public_key.len(), 1_952);
        assert_eq!(package.x25519_private_key.len(), 32);
        assert_eq!(package.ml_kem_768_private_key.len(), 2_400);
        assert_eq!(package.ed25519_private_key.len(), 32);
        assert_eq!(package.ml_dsa_65_private_key.len(), 4_032);
        sprout_crypto_protocol::DevicePublicPackage::from_json(&package.public_package).unwrap();

        package.destroy();
        assert!(package.x25519_private_key.is_empty());
        assert!(package.ml_kem_768_private_key.is_empty());
        assert!(package.ed25519_private_key.is_empty());
        assert!(package.ml_dsa_65_private_key.is_empty());
    }

    #[test]
    fn wasm_dual_signatures_round_trip_and_bind_context() {
        let package = generate_device_package(
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 21],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 22],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 23],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 24],
        )
        .unwrap();
        let mut signatures = wasm_sign_dual(
            &package.ed25519_private_key,
            &package.ml_dsa_65_private_key,
            b"offline mutation",
            b"tenant-a/device",
        )
        .unwrap();
        assert!(
            wasm_verify_dual(
                &package.ed25519_public_key,
                &signatures.ed25519,
                &package.ml_dsa_65_public_key,
                &signatures.ml_dsa_65,
                b"offline mutation",
                b"tenant-a/device",
            )
            .unwrap()
        );
        assert!(
            !wasm_verify_dual(
                &package.ed25519_public_key,
                &signatures.ed25519,
                &package.ml_dsa_65_public_key,
                &signatures.ml_dsa_65,
                b"offline mutation",
                b"tenant-b/device",
            )
            .unwrap()
        );
        signatures.destroy();
        assert!(signatures.ed25519.is_empty());
        assert!(signatures.ml_dsa_65.is_empty());
    }

    #[test]
    fn wasm_hybrid_wrap_round_trip_rejects_tamper_and_replay() {
        let package = generate_device_package(
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 30],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 31],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 33],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 34],
        )
        .unwrap();
        let resource_id = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 35];
        let device_id = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 30];
        let resource_key = [0x61; 32];
        let mut wrapped = wasm_wrap_resource_key(
            &resource_key,
            &package.x25519_public_key,
            &package.ml_kem_768_public_key,
            &resource_id,
            &device_id,
            0,
            &[0; 32],
            b"tenant/resource-wrap",
        )
        .unwrap();
        assert_eq!(wrapped.audit_status(), "production_audit_required");
        let mut opened = wasm_unwrap_resource_key(
            &wrapped.envelope,
            &package.x25519_private_key,
            &package.ml_kem_768_private_key,
            &resource_id,
            &device_id,
            0,
            &[0; 32],
            b"tenant/resource-wrap",
        )
        .unwrap();
        assert_eq!(opened.resource_key, resource_key);

        let replay = wasm_unwrap_resource_key(
            &wrapped.envelope,
            &package.x25519_private_key,
            &package.ml_kem_768_private_key,
            &resource_id,
            &device_id,
            1,
            &hash_bytes(b"epoch-zero"),
            b"tenant/resource-wrap",
        )
        .unwrap_err();
        assert_eq!(replay.code(), "context_mismatch");

        let mut tampered = wrapped.envelope.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        let error = wasm_unwrap_resource_key(
            &tampered,
            &package.x25519_private_key,
            &package.ml_kem_768_private_key,
            &resource_id,
            &device_id,
            0,
            &[0; 32],
            b"tenant/resource-wrap",
        )
        .unwrap_err();
        assert_eq!(error.code(), "authentication_failed");
        opened.destroy();
        wrapped.destroy();
    }

    #[test]
    fn wasm_recovery_split_requires_n_of_n_and_exact_context() {
        let mut split =
            wasm_split_recovery_secret_n_of_n(&[0x77; 32], 3, b"account/recovery").unwrap();
        assert_eq!(split.share_count, 3);
        assert_eq!(split.share(0).unwrap().len(), 171);
        let mut share_set = RecoveryShareSet::new();
        for position in 0..split.share_count {
            share_set
                .add_share(&split.share(position as u32).unwrap())
                .unwrap();
        }
        let mut recovered =
            wasm_combine_recovery_secret_n_of_n(&share_set, b"account/recovery").unwrap();
        assert_eq!(recovered.secret, [0x77; 32]);

        let mut shares = unpack_recovery_shares(&split.bundle).unwrap();
        shares.pop();
        let mut incomplete = RecoveryShareSet::new();
        for share in shares {
            incomplete.add_share(&share.to_bytes().unwrap()).unwrap();
        }
        let error =
            wasm_combine_recovery_secret_n_of_n(&incomplete, b"account/recovery").unwrap_err();
        assert_eq!(error.code(), "recovery_incomplete");

        let error = wasm_combine_recovery_secret_n_of_n(&share_set, b"other/context").unwrap_err();
        assert_eq!(error.code(), "context_mismatch");
        recovered.destroy();
        share_set.destroy();
        incomplete.destroy();
        split.destroy();
    }

    #[test]
    fn wasm_rotation_returns_fresh_separated_epoch_material() {
        let resource_id = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 40];
        let previous = hash_bytes(b"epoch zero");
        let mut first =
            wasm_rotate_resource_epoch(&resource_id, 0, &previous, b"tenant/resource").unwrap();
        let second =
            wasm_rotate_resource_epoch(&resource_id, 0, &previous, b"tenant/resource").unwrap();
        assert_eq!(first.epoch, 1);
        assert_ne!(first.resource_key, first.header_key);
        assert_ne!(first.resource_key, second.resource_key);
        assert_ne!(first.epoch_commitment, second.epoch_commitment);
        assert!(!first.public_metadata.is_empty());
        first.destroy();
        assert!(first.resource_key.is_empty());
        assert!(first.header_key.is_empty());
    }
}
