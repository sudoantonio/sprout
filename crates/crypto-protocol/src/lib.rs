//! Versioned cryptographic wire primitives for Sprout.
//!
//! The AES-256-GCM payload format in this crate is interoperable and uses a
//! canonical, authenticated header. Every call to [`seal_payload`] creates a
//! fresh 256-bit data-encryption key (DEK) and a fresh 96-bit nonce.
//!
//! # Production audit gate
//!
//! The classical AES-256-GCM and Ed25519 paths are implemented with established
//! RustCrypto/dalek primitives. The ML-KEM-768 and ML-DSA-65 adapters are
//! intentionally marked **experimental**: they expose the pinned libcrux APIs
//! behind narrow traits, but hybrid protocol composition, key lifecycle,
//! side-channel posture, and the complete wire protocol require an independent
//! cryptographic review before production use. [`ProductionHybridAdapter`]
//! therefore fails closed instead of implying that this review has happened.

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload, array::Array},
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::{TryRng, rngs::SysRng};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use uuid::Uuid;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Current binary protocol version.
pub const CURRENT_VERSION: ProtocolVersion = ProtocolVersion::V1;
/// AES-256 key and DEK length.
pub const KEY_BYTES: usize = 32;
/// Required GCM nonce length.
pub const NONCE_BYTES: usize = 12;
/// SHA-256 digest length.
pub const HASH_BYTES: usize = 32;
/// GCM authentication tag length.
pub const GCM_TAG_BYTES: usize = 16;
/// Maximum authenticated header context.
pub const MAX_HEADER_CONTEXT_BYTES: usize = 1_024;
/// Maximum plaintext accepted by this protocol layer.
pub const MAX_PLAINTEXT_BYTES: usize = 8 * 1024 * 1024;
/// Maximum serialized public package.
pub const MAX_PUBLIC_PACKAGE_BYTES: usize = 1024 * 1024;
/// Maximum serialized signed envelope.
pub const MAX_ENVELOPE_BYTES: usize = 64 * 1024;
/// Maximum key material in a single model field.
pub const MAX_KEY_MATERIAL_BYTES: usize = 8 * 1024;
/// Maximum detached signature size.
pub const MAX_SIGNATURE_BYTES: usize = 4 * 1024;
/// Maximum devices represented by one public package.
pub const MAX_DEVICES: usize = 64;
/// Maximum recovery participants.
pub const MAX_RECOVERY_PARTICIPANTS: usize = 16;

const HEADER_MAGIC: &[u8; 8] = b"SPRAAD01";
const PAYLOAD_MAGIC: &[u8; 8] = b"SPRENC01";
const CHAIN_DOMAIN: &[u8] = b"sprout-chain-v1";
const KDF_DOMAIN: &[u8] = b"sprout-key-separation-v1";
const ENVELOPE_DOMAIN: &[u8] = b"sprout-envelope-v1";
const ED25519_CONTEXT_DOMAIN: &[u8] = b"sprout-ed25519-context-v1";
const RECOVERY_SIGNATURE_CONTEXT: &[u8] = b"sprout-recovery-approval-v1";
const HYBRID_METADATA_MAGIC: &[u8; 8] = b"SPRHMD01";
const HYBRID_ENVELOPE_MAGIC: &[u8; 8] = b"SPRHYB01";
const HYBRID_KDF_DOMAIN: &[u8] = b"sprout-experimental-x25519-mlkem768-v1";
const HYBRID_AAD_DOMAIN: &[u8] = b"sprout-hybrid-wrap-aad-v1";
const RECOVERY_SHARE_MAGIC: &[u8; 8] = b"SPRSHR01";
const RECOVERY_BUNDLE_MAGIC: &[u8; 8] = b"SPRSHB01";
const RECOVERY_CONTEXT_DOMAIN: &[u8] = b"sprout-recovery-context-v1";
const RECOVERY_SECRET_COMMITMENT_DOMAIN: &[u8] = b"sprout-recovery-secret-commitment-v1";
const RECOVERY_SHARE_COMMITMENT_DOMAIN: &[u8] = b"sprout-recovery-share-commitment-v1";
const RESOURCE_EPOCH_DOMAIN: &[u8] = b"sprout-resource-epoch-v1";

/// Version of the non-standard X25519 + ML-KEM-768 wrapping construction.
pub const EXPERIMENTAL_HYBRID_SUITE_V1: u16 = 0x8001;
/// Fixed V1 recovery secret/share length.
pub const RECOVERY_SECRET_BYTES: usize = 32;
const RECOVERY_SHARE_PAYLOAD_BYTES: usize = RECOVERY_SECRET_BYTES * 2;

const ED25519_PUBLIC_KEY_BYTES: usize = 32;
const ED25519_SECRET_KEY_BYTES: usize = 32;
const ED25519_SIGNATURE_BYTES: usize = 64;
const ML_KEM_768_PUBLIC_KEY_BYTES: usize = 1_184;
const ML_KEM_768_PRIVATE_KEY_BYTES: usize = 2_400;
const ML_KEM_768_CIPHERTEXT_BYTES: usize = 1_088;
const ML_DSA_65_PUBLIC_KEY_BYTES: usize = 1_952;
const ML_DSA_65_SECRET_KEY_BYTES: usize = 4_032;
const ML_DSA_65_SIGNATURE_BYTES: usize = 3_309;

/// Errors returned by protocol operations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// Input exceeded a protocol limit.
    #[error("{field} exceeds its maximum size of {maximum} bytes/items")]
    SizeLimit {
        /// Name of the rejected field.
        field: &'static str,
        /// Maximum accepted size.
        maximum: usize,
    },
    /// A fixed-width value had the wrong length.
    #[error("{field} must be exactly {expected} bytes")]
    InvalidLength {
        /// Name of the rejected field.
        field: &'static str,
        /// Required size.
        expected: usize,
    },
    /// Binary or JSON input was malformed or non-canonical.
    #[error("invalid format: {0}")]
    InvalidFormat(&'static str),
    /// The wire version is not implemented.
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),
    /// An algorithm identifier is unknown or invalid in this position.
    #[error("unsupported or misplaced algorithm")]
    UnsupportedAlgorithm,
    /// An operating-system CSPRNG failed.
    #[error("secure randomness unavailable")]
    RandomnessUnavailable,
    /// AES-GCM rejected the key, nonce, AAD, or authentication tag.
    #[error("authentication failed")]
    AuthenticationFailed,
    /// The authenticated header did not match the caller's expected context.
    #[error("authenticated context mismatch")]
    ContextMismatch,
    /// A hash-chain invariant failed.
    #[error("hash chain validation failed")]
    InvalidHashChain,
    /// A signature was malformed or invalid.
    #[error("signature verification failed")]
    SignatureVerification,
    /// A recovery approval was invalid or replayed.
    #[error("invalid or replayed recovery approval")]
    InvalidRecoveryApproval,
    /// The recovery ceremony is expired.
    #[error("recovery ceremony expired")]
    RecoveryExpired,
    /// All configured recovery participants have not approved.
    #[error("recovery requires every configured participant")]
    RecoveryIncomplete,
    /// A one-shot ceremony has already been consumed.
    #[error("recovery ceremony already consumed")]
    RecoveryConsumed,
    /// The experimental hybrid suite has not passed the production audit gate.
    #[error("production hybrid adapter unavailable pending independent audit")]
    ProductionAuditRequired,
    /// Strict JSON parsing failed.
    #[error("invalid JSON: {0}")]
    Json(String),
}

/// Canonical JSON used for governance attestations.
///
/// This is the integer-only Sprout profile of RFC 8785: object member names
/// are ordered by their UTF-16 code units, strings use the JSON escaping
/// rules, arrays retain their order, and integers use their shortest decimal
/// representation. Floating-point numbers are deliberately rejected because
/// no signed governance schema contains them. The explicit profile avoids
/// making Rust struct declaration order part of the signature contract.
pub fn canonical_governance_json<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let value =
        serde_json::to_value(value).map_err(|error| ProtocolError::Json(error.to_string()))?;
    let mut output = Vec::new();
    write_canonical_governance_value(&value, &mut output)?;
    Ok(output)
}

fn write_canonical_governance_value(
    value: &Value,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => {
            if !(number.is_i64() || number.is_u64()) {
                return Err(ProtocolError::InvalidFormat(
                    "floating-point governance value",
                ));
            }
            output.extend_from_slice(number.to_string().as_bytes());
        }
        Value::String(text) => {
            let encoded = serde_json::to_string(text)
                .map_err(|error| ProtocolError::Json(error.to_string()))?;
            output.extend_from_slice(encoded.as_bytes());
        }
        Value::Array(items) => {
            output.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_canonical_governance_value(item, output)?;
            }
            output.push(b']');
        }
        Value::Object(object) => {
            let mut fields: Vec<_> = object.iter().collect();
            fields.sort_by(|(left, _), (right, _)| left.encode_utf16().cmp(right.encode_utf16()));
            output.push(b'{');
            for (index, (name, field)) in fields.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                let encoded_name = serde_json::to_string(name)
                    .map_err(|error| ProtocolError::Json(error.to_string()))?;
                output.extend_from_slice(encoded_name.as_bytes());
                output.push(b':');
                write_canonical_governance_value(field, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// Implemented protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolVersion {
    /// Initial canonical binary encoding.
    V1 = 1,
}

impl TryFrom<u8> for ProtocolVersion {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::V1),
            other => Err(ProtocolError::UnsupportedVersion(other)),
        }
    }
}

/// Payload cipher suites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum CipherSuite {
    /// AES-256-GCM payload encryption.
    Aes256Gcm = 1,
    /// Experimental X25519 + ML-KEM-768 composition with AES-256-GCM.
    ///
    /// This identifier reserves a canonical wire value only. It is not
    /// production-approved; use requires an explicit, independently reviewed
    /// composition and the production audit gate remains closed.
    ExperimentalHybridX25519MlKem768Aes256Gcm = 128,
}

impl TryFrom<u8> for CipherSuite {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Aes256Gcm),
            128 => Ok(Self::ExperimentalHybridX25519MlKem768Aes256Gcm),
            _ => Err(ProtocolError::UnsupportedAlgorithm),
        }
    }
}

/// Domain of the object authenticated by a canonical header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    /// User/resource payload.
    ResourcePayload = 1,
    /// Wrapped-key envelope.
    KeyEnvelope = 2,
    /// Recovery ceremony message.
    Recovery = 3,
    /// Public device package.
    PublicPackage = 4,
}

impl TryFrom<u8> for ContentKind {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ResourcePayload),
            2 => Ok(Self::KeyEnvelope),
            3 => Ok(Self::Recovery),
            4 => Ok(Self::PublicPackage),
            _ => Err(ProtocolError::InvalidFormat("unknown content kind")),
        }
    }
}

/// Versioned, canonical additional authenticated data.
///
/// Callers must supply an independently expected header to decryption. Merely
/// trusting the header embedded beside a ciphertext does not prevent a valid
/// ciphertext from being replayed into a different application context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalHeader {
    /// Wire protocol version.
    pub version: ProtocolVersion,
    /// Payload cipher suite.
    pub suite: CipherSuite,
    /// Type of authenticated object.
    pub kind: ContentKind,
    /// Stable resource identifier.
    pub resource_id: Uuid,
    /// Identifier of the DEK/envelope key.
    pub key_id: Uuid,
    /// Monotonic resource sequence.
    pub sequence: u64,
    /// Previous authenticated object hash, or all-zero for sequence zero.
    pub previous_hash: [u8; HASH_BYTES],
    /// Application context, included verbatim in AAD.
    pub context: Vec<u8>,
}

impl CanonicalHeader {
    /// Construct and validate a V1 header.
    pub fn new(
        suite: CipherSuite,
        kind: ContentKind,
        resource_id: Uuid,
        key_id: Uuid,
        sequence: u64,
        previous_hash: [u8; HASH_BYTES],
        context: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        let header = Self {
            version: CURRENT_VERSION,
            suite,
            kind,
            resource_id,
            key_id,
            sequence,
            previous_hash,
            context,
        };
        header.validate()?;
        Ok(header)
    }

    /// Validate IDs, limits, and sequence/hash consistency.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.resource_id.is_nil() || self.key_id.is_nil() {
            return Err(ProtocolError::InvalidFormat("nil header identifier"));
        }
        ensure_limit(
            "header context",
            self.context.len(),
            MAX_HEADER_CONTEXT_BYTES,
        )?;
        if self.sequence == 0 {
            if self.previous_hash != [0; HASH_BYTES] {
                return Err(ProtocolError::InvalidHashChain);
            }
        } else if self.previous_hash == [0; HASH_BYTES] {
            return Err(ProtocolError::InvalidHashChain);
        }
        Ok(())
    }

    /// Encode the unique canonical V1 AAD representation.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let context_len =
            u16::try_from(self.context.len()).map_err(|_| ProtocolError::SizeLimit {
                field: "header context",
                maximum: u16::MAX as usize,
            })?;
        let mut out = Vec::with_capacity(87 + self.context.len());
        out.extend_from_slice(HEADER_MAGIC);
        out.push(self.version as u8);
        out.push(self.suite as u8);
        out.push(self.kind as u8);
        out.extend_from_slice(self.resource_id.as_bytes());
        out.extend_from_slice(self.key_id.as_bytes());
        out.extend_from_slice(&self.sequence.to_be_bytes());
        out.extend_from_slice(&self.previous_hash);
        out.extend_from_slice(&context_len.to_be_bytes());
        out.extend_from_slice(&self.context);
        Ok(out)
    }

    /// Strictly parse a canonical V1 AAD representation.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        ensure_limit(
            "canonical header",
            bytes.len(),
            87 + MAX_HEADER_CONTEXT_BYTES,
        )?;
        let mut reader = Reader::new(bytes);
        if reader.take_array::<8>()? != *HEADER_MAGIC {
            return Err(ProtocolError::InvalidFormat("header magic"));
        }
        let version = ProtocolVersion::try_from(reader.byte()?)?;
        let suite = CipherSuite::try_from(reader.byte()?)?;
        let kind = ContentKind::try_from(reader.byte()?)?;
        let resource_id = Uuid::from_bytes(reader.take_array::<16>()?);
        let key_id = Uuid::from_bytes(reader.take_array::<16>()?);
        let sequence = u64::from_be_bytes(reader.take_array::<8>()?);
        let previous_hash = reader.take_array::<HASH_BYTES>()?;
        let context_len = u16::from_be_bytes(reader.take_array::<2>()?) as usize;
        ensure_limit("header context", context_len, MAX_HEADER_CONTEXT_BYTES)?;
        let context = reader.take(context_len)?.to_vec();
        reader.finish()?;
        let header = Self {
            version,
            suite,
            kind,
            resource_id,
            key_id,
            sequence,
            previous_hash,
            context,
        };
        header.validate()?;
        if header.canonical_bytes()?.as_slice() != bytes {
            return Err(ProtocolError::InvalidFormat("non-canonical header"));
        }
        Ok(header)
    }
}

/// Build canonical header bytes from fixed-width byte inputs.
///
/// This convenience API keeps UUID parsing and validation inside the protocol
/// crate, which is useful for thin foreign-function boundaries.
#[allow(clippy::too_many_arguments)]
pub fn canonical_header_from_parts(
    version: u8,
    suite: u8,
    kind: u8,
    resource_id: &[u8],
    key_id: &[u8],
    sequence: u64,
    previous_hash: &[u8],
    context: &[u8],
) -> Result<Vec<u8>, ProtocolError> {
    let resource_id = exact_array::<16>("resource id", resource_id)?;
    let key_id = exact_array::<16>("key id", key_id)?;
    let previous_hash = exact_array::<HASH_BYTES>("previous hash", previous_hash)?;
    let header = CanonicalHeader {
        version: ProtocolVersion::try_from(version)?,
        suite: CipherSuite::try_from(suite)?,
        kind: ContentKind::try_from(kind)?,
        resource_id: Uuid::from_bytes(resource_id),
        key_id: Uuid::from_bytes(key_id),
        sequence,
        previous_hash,
        context: context.to_vec(),
    };
    header.canonical_bytes()
}

macro_rules! secret_key_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Zeroize, ZeroizeOnDrop)]
        pub struct $name([u8; KEY_BYTES]);

        impl $name {
            /// Construct from exactly 32 bytes.
            pub fn from_slice(bytes: &[u8]) -> Result<Self, ProtocolError> {
                Ok(Self(exact_array::<KEY_BYTES>("secret key", bytes)?))
            }

            /// Borrow the raw key bytes.
            pub fn as_bytes(&self) -> &[u8; KEY_BYTES] {
                &self.0
            }
        }
    };
}

secret_key_type!(
    DataEncryptionKey,
    "A zeroizing 256-bit data-encryption key."
);
secret_key_type!(RootKey, "A zeroizing 256-bit key-hierarchy root.");
secret_key_type!(
    ResourceKey,
    "A domain-separated resource key derived from a root."
);
secret_key_type!(
    HeaderKey,
    "A domain-separated header/envelope key derived from a root."
);

impl DataEncryptionKey {
    /// Generate a fresh DEK using the operating-system CSPRNG.
    pub fn generate() -> Result<Self, ProtocolError> {
        let mut key = [0u8; KEY_BYTES];
        fill_random(&mut key)?;
        Ok(Self(key))
    }
}

impl RootKey {
    /// Generate a fresh hierarchy root using the operating-system CSPRNG.
    pub fn generate() -> Result<Self, ProtocolError> {
        let mut key = [0u8; KEY_BYTES];
        fill_random(&mut key)?;
        Ok(Self(key))
    }
}

/// Resource/header keys derived with distinct, fixed domain labels.
pub struct SeparatedKeys {
    /// Key for resource data/key wrapping.
    pub resource: ResourceKey,
    /// Key for headers/envelopes.
    pub header: HeaderKey,
}

impl SeparatedKeys {
    /// Derive separated keys for one resource.
    ///
    /// The root must be uniformly random. The labels are protocol constants and
    /// cannot be supplied by callers, preventing accidental label reuse.
    pub fn derive(root: &RootKey, resource_id: Uuid) -> Result<Self, ProtocolError> {
        if resource_id.is_nil() {
            return Err(ProtocolError::InvalidFormat("nil resource identifier"));
        }
        let resource = derive_separated_key(root.as_bytes(), resource_id, b"resource");
        let header = derive_separated_key(root.as_bytes(), resource_id, b"header");
        if bool::from(resource.ct_eq(&header)) {
            return Err(ProtocolError::InvalidFormat("key separation failure"));
        }
        Ok(Self {
            resource: ResourceKey(resource),
            header: HeaderKey(header),
        })
    }
}

/// AES-GCM ciphertext and its canonical authenticated header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedPayload {
    /// Header whose canonical bytes were supplied as GCM AAD.
    pub header: CanonicalHeader,
    /// Fresh 96-bit GCM nonce.
    pub nonce: [u8; NONCE_BYTES],
    /// Ciphertext followed by the 128-bit authentication tag.
    pub ciphertext: Vec<u8>,
}

impl EncryptedPayload {
    /// Validate all protocol-level size and structure constraints.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.header.validate()?;
        if self.ciphertext.len() < GCM_TAG_BYTES {
            return Err(ProtocolError::InvalidFormat("ciphertext lacks GCM tag"));
        }
        ensure_limit(
            "ciphertext",
            self.ciphertext.len(),
            MAX_PLAINTEXT_BYTES + GCM_TAG_BYTES,
        )
    }

    /// Encode a self-delimiting strict binary representation.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let header = self.header.canonical_bytes()?;
        let header_len = u16::try_from(header.len()).map_err(|_| ProtocolError::SizeLimit {
            field: "canonical header",
            maximum: u16::MAX as usize,
        })?;
        let ciphertext_len =
            u32::try_from(self.ciphertext.len()).map_err(|_| ProtocolError::SizeLimit {
                field: "ciphertext",
                maximum: u32::MAX as usize,
            })?;
        let mut out =
            Vec::with_capacity(8 + 2 + header.len() + NONCE_BYTES + 4 + self.ciphertext.len());
        out.extend_from_slice(PAYLOAD_MAGIC);
        out.extend_from_slice(&header_len.to_be_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&ciphertext_len.to_be_bytes());
        out.extend_from_slice(&self.ciphertext);
        Ok(out)
    }

    /// Parse a strict binary payload, rejecting truncation, trailing bytes, and
    /// oversized allocations before copying attacker-controlled data.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        ensure_limit(
            "encrypted payload",
            bytes.len(),
            MAX_PLAINTEXT_BYTES + MAX_HEADER_CONTEXT_BYTES + 256,
        )?;
        let mut reader = Reader::new(bytes);
        if reader.take_array::<8>()? != *PAYLOAD_MAGIC {
            return Err(ProtocolError::InvalidFormat("payload magic"));
        }
        let header_len = u16::from_be_bytes(reader.take_array::<2>()?) as usize;
        ensure_limit(
            "canonical header",
            header_len,
            87 + MAX_HEADER_CONTEXT_BYTES,
        )?;
        let header = CanonicalHeader::from_canonical_bytes(reader.take(header_len)?)?;
        let nonce = reader.take_array::<NONCE_BYTES>()?;
        let ciphertext_len = u32::from_be_bytes(reader.take_array::<4>()?) as usize;
        ensure_limit(
            "ciphertext",
            ciphertext_len,
            MAX_PLAINTEXT_BYTES + GCM_TAG_BYTES,
        )?;
        let ciphertext = reader.take(ciphertext_len)?.to_vec();
        reader.finish()?;
        let payload = Self {
            header,
            nonce,
            ciphertext,
        };
        payload.validate()?;
        Ok(payload)
    }

    /// Hash the unique strict binary representation.
    pub fn hash(&self) -> Result<[u8; HASH_BYTES], ProtocolError> {
        Ok(hash_bytes(&self.to_bytes()?))
    }
}

/// Result of sealing a payload. The DEK must be wrapped for recipients and
/// stored separately from the ciphertext.
pub struct SealedPayload {
    /// Fresh, zeroizing 256-bit DEK.
    pub dek: DataEncryptionKey,
    /// Authenticated encrypted payload.
    pub payload: EncryptedPayload,
}

/// Encrypt a payload with a new random DEK and nonce.
pub fn seal_payload(
    header: CanonicalHeader,
    plaintext: &[u8],
) -> Result<SealedPayload, ProtocolError> {
    ensure_limit("plaintext", plaintext.len(), MAX_PLAINTEXT_BYTES)?;
    header.validate()?;
    let dek = DataEncryptionKey::generate()?;
    let mut nonce = [0u8; NONCE_BYTES];
    fill_random(&mut nonce)?;
    let payload = encrypt_with_nonce(&dek, header, plaintext, nonce)?;
    Ok(SealedPayload { dek, payload })
}

/// Decrypt only when the authenticated header exactly matches the caller's
/// independently supplied expected context.
pub fn open_payload(
    dek: &DataEncryptionKey,
    payload: &EncryptedPayload,
    expected_header: &CanonicalHeader,
) -> Result<Vec<u8>, ProtocolError> {
    payload.validate()?;
    expected_header.validate()?;
    let actual_aad = payload.header.canonical_bytes()?;
    let expected_aad = expected_header.canonical_bytes()?;
    if actual_aad.len() != expected_aad.len()
        || !bool::from(actual_aad.as_slice().ct_eq(expected_aad.as_slice()))
    {
        return Err(ProtocolError::ContextMismatch);
    }
    let cipher = Aes256Gcm::new(&Array(*dek.as_bytes()));
    cipher
        .decrypt(
            &Array(payload.nonce),
            Payload {
                msg: &payload.ciphertext,
                aad: &actual_aad,
            },
        )
        .map_err(|_| ProtocolError::AuthenticationFailed)
}

fn encrypt_with_nonce(
    dek: &DataEncryptionKey,
    header: CanonicalHeader,
    plaintext: &[u8],
    nonce: [u8; NONCE_BYTES],
) -> Result<EncryptedPayload, ProtocolError> {
    ensure_limit("plaintext", plaintext.len(), MAX_PLAINTEXT_BYTES)?;
    let aad = header.canonical_bytes()?;
    let cipher = Aes256Gcm::new(&Array(*dek.as_bytes()));
    let ciphertext = cipher
        .encrypt(
            &Array(nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| ProtocolError::AuthenticationFailed)?;
    Ok(EncryptedPayload {
        header,
        nonce,
        ciphertext,
    })
}

/// SHA-256 with a fixed-size return type.
pub fn hash_bytes(bytes: &[u8]) -> [u8; HASH_BYTES] {
    Sha256::digest(bytes).into()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; HASH_BYTES] {
    const BLOCK_BYTES: usize = 64;
    let mut key_block = [0u8; BLOCK_BYTES];
    if key.len() > BLOCK_BYTES {
        key_block[..HASH_BYTES].copy_from_slice(&hash_bytes(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; BLOCK_BYTES];
    let mut outer_pad = [0x5cu8; BLOCK_BYTES];
    for index in 0..BLOCK_BYTES {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(data);
    let mut inner_hash: [u8; HASH_BYTES] = inner.finalize().into();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_hash);
    let output = outer.finalize().into();
    key_block.zeroize();
    inner_pad.zeroize();
    outer_pad.zeroize();
    inner_hash.zeroize();
    output
}

fn hkdf_sha256(
    input_key_material: &[u8],
    salt: &[u8],
    info: &[u8],
    output_length: usize,
) -> Result<Vec<u8>, ProtocolError> {
    if output_length > 255 * HASH_BYTES {
        return Err(ProtocolError::SizeLimit {
            field: "HKDF output",
            maximum: 255 * HASH_BYTES,
        });
    }
    let zero_salt = [0u8; HASH_BYTES];
    let mut pseudorandom_key = hmac_sha256(
        if salt.is_empty() { &zero_salt } else { salt },
        input_key_material,
    );
    let blocks = output_length.div_ceil(HASH_BYTES);
    let mut output = Vec::with_capacity(output_length);
    let mut previous = Vec::new();
    for counter in 1..=blocks {
        let mut block_input = Vec::with_capacity(previous.len() + info.len() + 1);
        block_input.extend_from_slice(&previous);
        block_input.extend_from_slice(info);
        block_input.push(counter as u8);
        previous.zeroize();
        previous = hmac_sha256(&pseudorandom_key, &block_input).to_vec();
        block_input.zeroize();
        let remaining = output_length - output.len();
        output.extend_from_slice(&previous[..remaining.min(HASH_BYTES)]);
    }
    previous.zeroize();
    pseudorandom_key.zeroize();
    Ok(output)
}

/// One verified hash-chain link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HashChainLink {
    /// Strictly monotonic sequence.
    pub sequence: u64,
    /// Previous link hash, or all-zero for the genesis link.
    pub previous_hash: [u8; HASH_BYTES],
    /// Hash of the linked canonical object.
    pub content_hash: [u8; HASH_BYTES],
    /// Domain-separated hash over the preceding fields.
    pub link_hash: [u8; HASH_BYTES],
}

impl HashChainLink {
    /// Construct a link and calculate its domain-separated hash.
    pub fn new(
        sequence: u64,
        previous_hash: [u8; HASH_BYTES],
        content_hash: [u8; HASH_BYTES],
    ) -> Result<Self, ProtocolError> {
        if (sequence == 0) != (previous_hash == [0; HASH_BYTES]) {
            return Err(ProtocolError::InvalidHashChain);
        }
        let link_hash = calculate_link_hash(sequence, &previous_hash, &content_hash);
        Ok(Self {
            sequence,
            previous_hash,
            content_hash,
            link_hash,
        })
    }
}

/// Verify genesis, strict sequencing, predecessor hashes, and every link hash.
pub fn verify_hash_chain(chain: &[HashChainLink]) -> Result<(), ProtocolError> {
    if chain.is_empty() {
        return Err(ProtocolError::InvalidHashChain);
    }
    for (index, link) in chain.iter().enumerate() {
        let expected_sequence = index as u64;
        if link.sequence != expected_sequence {
            return Err(ProtocolError::InvalidHashChain);
        }
        let expected_previous = if index == 0 {
            [0; HASH_BYTES]
        } else {
            chain[index - 1].link_hash
        };
        if link.previous_hash != expected_previous
            || link.link_hash
                != calculate_link_hash(link.sequence, &link.previous_hash, &link.content_hash)
        {
            return Err(ProtocolError::InvalidHashChain);
        }
    }
    Ok(())
}

fn calculate_link_hash(
    sequence: u64,
    previous_hash: &[u8; HASH_BYTES],
    content_hash: &[u8; HASH_BYTES],
) -> [u8; HASH_BYTES] {
    let mut digest = Sha256::new();
    digest.update(CHAIN_DOMAIN);
    digest.update(sequence.to_be_bytes());
    digest.update(previous_hash);
    digest.update(content_hash);
    digest.finalize().into()
}

/// Algorithms represented by public packages and envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum KeyAlgorithm {
    /// Ed25519 signature key.
    Ed25519 = 1,
    /// X25519 key agreement key.
    X25519 = 2,
    /// Symmetric AES-256-GCM key (never valid as public key material).
    Aes256Gcm = 3,
    /// Experimental ML-KEM-768 key.
    MlKem768Experimental = 128,
    /// Experimental ML-DSA-65 key.
    MlDsa65Experimental = 129,
}

/// Domain-limited purpose of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyPurpose {
    /// Resource payload encryption/wrapping.
    Resource,
    /// Header and envelope protection.
    Header,
    /// Device recipient encryption.
    DeviceEncryption,
    /// Device signature verification.
    DeviceSigning,
    /// Recovery approval verification.
    RecoverySigning,
}

/// Public key descriptor with an explicit algorithm and purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicKeyDescriptor {
    /// Stable key identifier.
    pub key_id: Uuid,
    /// Key algorithm.
    pub algorithm: KeyAlgorithm,
    /// Allowed protocol use.
    pub purpose: KeyPurpose,
    /// Raw standardized public-key encoding.
    pub public_key: Vec<u8>,
}

impl PublicKeyDescriptor {
    /// Strictly validate algorithm-specific public key lengths.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.key_id.is_nil() {
            return Err(ProtocolError::InvalidFormat("nil key identifier"));
        }
        ensure_limit("public key", self.public_key.len(), MAX_KEY_MATERIAL_BYTES)?;
        let expected = match self.algorithm {
            KeyAlgorithm::Ed25519 | KeyAlgorithm::X25519 => 32,
            KeyAlgorithm::MlKem768Experimental => ML_KEM_768_PUBLIC_KEY_BYTES,
            KeyAlgorithm::MlDsa65Experimental => ML_DSA_65_PUBLIC_KEY_BYTES,
            KeyAlgorithm::Aes256Gcm => return Err(ProtocolError::UnsupportedAlgorithm),
        };
        if self.public_key.len() != expected {
            return Err(ProtocolError::InvalidLength {
                field: "public key",
                expected,
            });
        }
        Ok(())
    }
}

/// Versioned device key suite.
///
/// The experimental V1 suite publishes four independent keys. It does **not**
/// define an X-Wing or other hybrid KEM combiner, and it is not covered by the
/// production hybrid audit gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
#[serde(rename_all = "snake_case")]
pub enum DeviceSuiteVersion {
    /// Independent X25519, ML-KEM-768, Ed25519, and ML-DSA-65 keys.
    ExperimentalIndependentKeysV1 = 0x8001,
}

/// Stable identifiers assigned to one generated device's four keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceKeyIds {
    /// X25519 public-key identifier.
    pub x25519: Uuid,
    /// Experimental ML-KEM-768 public-key identifier.
    pub ml_kem_768: Uuid,
    /// Ed25519 verification-key identifier.
    pub ed25519: Uuid,
    /// Experimental ML-DSA-65 verification-key identifier.
    pub ml_dsa_65: Uuid,
}

impl DeviceKeyIds {
    /// Validate that all key identifiers are non-nil and pairwise distinct.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let ids = [self.x25519, self.ml_kem_768, self.ed25519, self.ml_dsa_65];
        if ids.iter().any(Uuid::is_nil) {
            return Err(ProtocolError::InvalidFormat("nil device key identifier"));
        }
        ensure_unique_ids(ids)
    }
}

/// Public keys and chain state for one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePublicPackage {
    /// Explicit experimental suite version.
    pub suite: DeviceSuiteVersion,
    /// Device identifier.
    pub device_id: Uuid,
    /// Device package generation.
    pub generation: u64,
    /// Previous device package hash.
    pub previous_hash: [u8; HASH_BYTES],
    /// Recipient encryption/KEM keys.
    pub encryption_keys: Vec<PublicKeyDescriptor>,
    /// Classical and optional experimental signature keys.
    pub signing_keys: Vec<PublicKeyDescriptor>,
}

impl DevicePublicPackage {
    /// Validate IDs, generation chaining, key count, purpose, and key sizes.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.device_id.is_nil() {
            return Err(ProtocolError::InvalidFormat("nil device identifier"));
        }
        validate_generation(self.generation, &self.previous_hash)?;
        ensure_nonempty_bounded("device encryption keys", self.encryption_keys.len(), 4)?;
        ensure_nonempty_bounded("device signing keys", self.signing_keys.len(), 4)?;
        for key in &self.encryption_keys {
            key.validate()?;
            if key.purpose != KeyPurpose::DeviceEncryption
                || !matches!(
                    key.algorithm,
                    KeyAlgorithm::X25519 | KeyAlgorithm::MlKem768Experimental
                )
            {
                return Err(ProtocolError::UnsupportedAlgorithm);
            }
        }
        for key in &self.signing_keys {
            key.validate()?;
            if key.purpose != KeyPurpose::DeviceSigning
                || !matches!(
                    key.algorithm,
                    KeyAlgorithm::Ed25519 | KeyAlgorithm::MlDsa65Experimental
                )
            {
                return Err(ProtocolError::UnsupportedAlgorithm);
            }
        }
        ensure_unique_ids(
            self.encryption_keys
                .iter()
                .chain(&self.signing_keys)
                .map(|key| key.key_id),
        )
    }

    /// Serialize in deterministic struct-field order.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let bytes =
            serde_json::to_vec(self).map_err(|error| ProtocolError::Json(error.to_string()))?;
        ensure_limit("device package", bytes.len(), MAX_PUBLIC_PACKAGE_BYTES)?;
        Ok(bytes)
    }

    /// Strictly parse and validate a canonical device package.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProtocolError> {
        strict_json(bytes, MAX_PUBLIC_PACKAGE_BYTES, "device package")
    }
}

impl Validate for DevicePublicPackage {
    fn validate_model(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

/// Zeroizing private fields generated for an experimental device suite.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct DevicePrivateKeys {
    x25519: [u8; KEY_BYTES],
    ml_kem_768: Vec<u8>,
    ed25519: [u8; ED25519_SECRET_KEY_BYTES],
    ml_dsa_65: Vec<u8>,
}

impl DevicePrivateKeys {
    /// Borrow the X25519 static secret bytes.
    pub fn x25519(&self) -> &[u8; KEY_BYTES] {
        &self.x25519
    }

    /// Borrow the ML-KEM-768 decapsulation-key bytes.
    pub fn ml_kem_768(&self) -> &[u8] {
        &self.ml_kem_768
    }

    /// Borrow the Ed25519 signing-key seed.
    pub fn ed25519(&self) -> &[u8; ED25519_SECRET_KEY_BYTES] {
        &self.ed25519
    }

    /// Borrow the ML-DSA-65 signing-key bytes.
    pub fn ml_dsa_65(&self) -> &[u8] {
        &self.ml_dsa_65
    }
}

/// Public package plus zeroizing private key fields for a new device.
pub struct GeneratedDevicePackage {
    public_package: DevicePublicPackage,
    private_keys: DevicePrivateKeys,
}

impl GeneratedDevicePackage {
    /// Borrow the serializable public package.
    pub fn public_package(&self) -> &DevicePublicPackage {
        &self.public_package
    }

    /// Borrow the zeroizing private fields.
    pub fn private_keys(&self) -> &DevicePrivateKeys {
        &self.private_keys
    }
}

/// Generate all real key pairs in the experimental independent-key suite.
///
/// This is key generation only. It does not combine X25519 and ML-KEM shared
/// secrets and therefore does not claim X-Wing compatibility.
pub fn generate_experimental_device_package(
    device_id: Uuid,
    key_ids: DeviceKeyIds,
) -> Result<GeneratedDevicePackage, ProtocolError> {
    if device_id.is_nil() {
        return Err(ProtocolError::InvalidFormat("nil device identifier"));
    }
    key_ids.validate()?;

    let mut x25519_seed = [0u8; KEY_BYTES];
    fill_random(&mut x25519_seed)?;
    let x25519_secret = X25519StaticSecret::from(x25519_seed);
    x25519_seed.zeroize();
    let x25519_public = X25519PublicKey::from(&x25519_secret);

    let ml_kem = LibcruxMlKem768Experimental.generate_key_pair()?;
    let ed25519 = Ed25519Adapter.generate_key_pair()?;
    let ml_dsa = LibcruxMlDsa65Experimental.generate_key_pair()?;

    let public_package = DevicePublicPackage {
        suite: DeviceSuiteVersion::ExperimentalIndependentKeysV1,
        device_id,
        generation: 0,
        previous_hash: [0; HASH_BYTES],
        encryption_keys: vec![
            PublicKeyDescriptor {
                key_id: key_ids.x25519,
                algorithm: KeyAlgorithm::X25519,
                purpose: KeyPurpose::DeviceEncryption,
                public_key: x25519_public.as_bytes().to_vec(),
            },
            PublicKeyDescriptor {
                key_id: key_ids.ml_kem_768,
                algorithm: KeyAlgorithm::MlKem768Experimental,
                purpose: KeyPurpose::DeviceEncryption,
                public_key: ml_kem.public_key().to_vec(),
            },
        ],
        signing_keys: vec![
            PublicKeyDescriptor {
                key_id: key_ids.ed25519,
                algorithm: KeyAlgorithm::Ed25519,
                purpose: KeyPurpose::DeviceSigning,
                public_key: ed25519.public_key().to_vec(),
            },
            PublicKeyDescriptor {
                key_id: key_ids.ml_dsa_65,
                algorithm: KeyAlgorithm::MlDsa65Experimental,
                purpose: KeyPurpose::DeviceSigning,
                public_key: ml_dsa.public_key().to_vec(),
            },
        ],
    };
    public_package.validate()?;

    Ok(GeneratedDevicePackage {
        public_package,
        private_keys: DevicePrivateKeys {
            x25519: x25519_secret.to_bytes(),
            ml_kem_768: ml_kem.secret_key().to_vec(),
            ed25519: exact_array::<ED25519_SECRET_KEY_BYTES>(
                "Ed25519 secret key",
                ed25519.secret_key(),
            )?,
            ml_dsa_65: ml_dsa.secret_key().to_vec(),
        },
    })
}

/// Byte-slice convenience wrapper for foreign-function boundaries.
#[allow(clippy::too_many_arguments)]
pub fn generate_experimental_device_package_from_bytes(
    device_id: &[u8],
    x25519_key_id: &[u8],
    ml_kem_768_key_id: &[u8],
    ed25519_key_id: &[u8],
    ml_dsa_65_key_id: &[u8],
) -> Result<GeneratedDevicePackage, ProtocolError> {
    generate_experimental_device_package(
        Uuid::from_bytes(exact_array::<16>("device id", device_id)?),
        DeviceKeyIds {
            x25519: Uuid::from_bytes(exact_array::<16>("X25519 key id", x25519_key_id)?),
            ml_kem_768: Uuid::from_bytes(exact_array::<16>(
                "ML-KEM-768 key id",
                ml_kem_768_key_id,
            )?),
            ed25519: Uuid::from_bytes(exact_array::<16>("Ed25519 key id", ed25519_key_id)?),
            ml_dsa_65: Uuid::from_bytes(exact_array::<16>("ML-DSA-65 key id", ml_dsa_65_key_id)?),
        },
    )
}

/// An n-of-n recovery policy. Thresholds below participant count are not
/// representable in V1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPolicy {
    /// Every listed participant must approve.
    pub participant_ids: Vec<Uuid>,
}

impl RecoveryPolicy {
    /// Create a non-empty n-of-n policy with unique, non-nil participants.
    pub fn new(participant_ids: Vec<Uuid>) -> Result<Self, ProtocolError> {
        let policy = Self { participant_ids };
        policy.validate()?;
        Ok(policy)
    }

    /// Validate participant count and uniqueness.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        ensure_nonempty_bounded(
            "recovery participants",
            self.participant_ids.len(),
            MAX_RECOVERY_PARTICIPANTS,
        )?;
        if self.participant_ids.iter().any(Uuid::is_nil) {
            return Err(ProtocolError::InvalidFormat(
                "nil recovery participant identifier",
            ));
        }
        ensure_unique_ids(self.participant_ids.iter().copied())
    }

    /// The fixed V1 threshold, equal to participant count.
    pub fn threshold(&self) -> usize {
        self.participant_ids.len()
    }
}

/// Account-level public device/recovery package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicPackage {
    /// Wire version.
    pub version: ProtocolVersion,
    /// Package identifier.
    pub package_id: Uuid,
    /// Owning account identifier.
    pub account_id: Uuid,
    /// Monotonic package generation.
    pub generation: u64,
    /// Previous canonical package hash.
    pub previous_hash: [u8; HASH_BYTES],
    /// Public device packages.
    pub devices: Vec<DevicePublicPackage>,
    /// n-of-n recovery policy.
    pub recovery: RecoveryPolicy,
}

impl PublicPackage {
    /// Validate all nested package constraints.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.package_id.is_nil() || self.account_id.is_nil() {
            return Err(ProtocolError::InvalidFormat("nil package identifier"));
        }
        validate_generation(self.generation, &self.previous_hash)?;
        ensure_nonempty_bounded("devices", self.devices.len(), MAX_DEVICES)?;
        ensure_unique_ids(self.devices.iter().map(|device| device.device_id))?;
        for device in &self.devices {
            device.validate()?;
        }
        self.recovery.validate()
    }

    /// Serialize in deterministic struct-field order with no insignificant
    /// whitespace. Maps are intentionally absent from canonical V1 models.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let bytes =
            serde_json::to_vec(self).map_err(|error| ProtocolError::Json(error.to_string()))?;
        ensure_limit("public package", bytes.len(), MAX_PUBLIC_PACKAGE_BYTES)?;
        Ok(bytes)
    }

    /// Strictly parse and validate a bounded public package.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProtocolError> {
        strict_json(bytes, MAX_PUBLIC_PACKAGE_BYTES, "public package")
    }

    /// Hash the canonical JSON representation.
    pub fn hash(&self) -> Result<[u8; HASH_BYTES], ProtocolError> {
        Ok(hash_bytes(&self.to_canonical_json()?))
    }
}

impl Validate for PublicPackage {
    fn validate_model(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

/// Wrapped DEK envelope body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEnvelope {
    /// Wire version.
    pub version: ProtocolVersion,
    /// Envelope identifier.
    pub envelope_id: Uuid,
    /// Resource identifier.
    pub resource_id: Uuid,
    /// Recipient device identifier.
    pub recipient_device_id: Uuid,
    /// Resource-key identifier.
    pub resource_key_id: Uuid,
    /// Distinct header-key identifier.
    pub header_key_id: Uuid,
    /// Monotonic resource sequence.
    pub sequence: u64,
    /// Previous envelope hash.
    pub previous_hash: [u8; HASH_BYTES],
    /// Recipient KEM/key agreement algorithm.
    pub recipient_algorithm: KeyAlgorithm,
    /// KEM ciphertext or ephemeral classical public key.
    pub encapsulation: Vec<u8>,
    /// Fresh nonce used to wrap the DEK under a derived KEK.
    pub wrap_nonce: [u8; NONCE_BYTES],
    /// AES-GCM wrapped 32-byte DEK plus tag.
    pub wrapped_dek: Vec<u8>,
    /// Hash of the payload's canonical header.
    pub payload_header_hash: [u8; HASH_BYTES],
}

impl KeyEnvelope {
    /// Validate key separation, chain state, algorithms, and bounded fields.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if [
            self.envelope_id,
            self.resource_id,
            self.recipient_device_id,
            self.resource_key_id,
            self.header_key_id,
        ]
        .iter()
        .any(Uuid::is_nil)
        {
            return Err(ProtocolError::InvalidFormat("nil envelope identifier"));
        }
        if self.resource_key_id == self.header_key_id {
            return Err(ProtocolError::InvalidFormat(
                "resource and header keys must be distinct",
            ));
        }
        validate_generation(self.sequence, &self.previous_hash)?;
        let expected_encapsulation = match self.recipient_algorithm {
            KeyAlgorithm::X25519 => 32,
            KeyAlgorithm::MlKem768Experimental => ML_KEM_768_CIPHERTEXT_BYTES,
            _ => return Err(ProtocolError::UnsupportedAlgorithm),
        };
        if self.encapsulation.len() != expected_encapsulation {
            return Err(ProtocolError::InvalidLength {
                field: "encapsulation",
                expected: expected_encapsulation,
            });
        }
        if self.wrapped_dek.len() != KEY_BYTES + GCM_TAG_BYTES {
            return Err(ProtocolError::InvalidLength {
                field: "wrapped DEK",
                expected: KEY_BYTES + GCM_TAG_BYTES,
            });
        }
        Ok(())
    }

    /// Canonical bytes signed by both signature algorithms.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let encapsulation_len =
            u16::try_from(self.encapsulation.len()).map_err(|_| ProtocolError::SizeLimit {
                field: "encapsulation",
                maximum: u16::MAX as usize,
            })?;
        let mut out = Vec::with_capacity(256 + self.encapsulation.len() + self.wrapped_dek.len());
        out.extend_from_slice(ENVELOPE_DOMAIN);
        out.push(self.version as u8);
        out.extend_from_slice(self.envelope_id.as_bytes());
        out.extend_from_slice(self.resource_id.as_bytes());
        out.extend_from_slice(self.recipient_device_id.as_bytes());
        out.extend_from_slice(self.resource_key_id.as_bytes());
        out.extend_from_slice(self.header_key_id.as_bytes());
        out.extend_from_slice(&self.sequence.to_be_bytes());
        out.extend_from_slice(&self.previous_hash);
        out.push(self.recipient_algorithm as u8);
        out.extend_from_slice(&encapsulation_len.to_be_bytes());
        out.extend_from_slice(&self.encapsulation);
        out.extend_from_slice(&self.wrap_nonce);
        out.extend_from_slice(&(self.wrapped_dek.len() as u16).to_be_bytes());
        out.extend_from_slice(&self.wrapped_dek);
        out.extend_from_slice(&self.payload_header_hash);
        Ok(out)
    }

    /// Hash the canonical envelope body.
    pub fn hash(&self) -> Result<[u8; HASH_BYTES], ProtocolError> {
        Ok(hash_bytes(&self.signing_bytes()?))
    }
}

/// Detached signature with signer identity and algorithm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetachedSignature {
    /// Signature algorithm.
    pub algorithm: KeyAlgorithm,
    /// Public key identifier.
    pub key_id: Uuid,
    /// Raw standardized signature bytes.
    pub signature: Vec<u8>,
}

impl DetachedSignature {
    /// Validate algorithm-specific signature length.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.key_id.is_nil() {
            return Err(ProtocolError::InvalidFormat("nil signature key id"));
        }
        ensure_limit("signature", self.signature.len(), MAX_SIGNATURE_BYTES)?;
        let expected = match self.algorithm {
            KeyAlgorithm::Ed25519 => ED25519_SIGNATURE_BYTES,
            KeyAlgorithm::MlDsa65Experimental => ML_DSA_65_SIGNATURE_BYTES,
            _ => return Err(ProtocolError::UnsupportedAlgorithm),
        };
        if self.signature.len() != expected {
            return Err(ProtocolError::InvalidLength {
                field: "signature",
                expected,
            });
        }
        Ok(())
    }
}

/// Envelope carrying mandatory classical and post-quantum signatures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DualSignatureEnvelope {
    /// Canonical envelope body.
    pub envelope: KeyEnvelope,
    /// Mandatory Ed25519 signature.
    pub classical_signature: DetachedSignature,
    /// Mandatory experimental ML-DSA-65 signature.
    pub post_quantum_signature: DetachedSignature,
}

impl DualSignatureEnvelope {
    /// Validate the body and require exactly the V1 dual-signature algorithms.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.envelope.validate()?;
        self.classical_signature.validate()?;
        self.post_quantum_signature.validate()?;
        if self.classical_signature.algorithm != KeyAlgorithm::Ed25519
            || self.post_quantum_signature.algorithm != KeyAlgorithm::MlDsa65Experimental
        {
            return Err(ProtocolError::UnsupportedAlgorithm);
        }
        Ok(())
    }

    /// Strict canonical JSON serialization.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let bytes =
            serde_json::to_vec(self).map_err(|error| ProtocolError::Json(error.to_string()))?;
        ensure_limit("signed envelope", bytes.len(), MAX_ENVELOPE_BYTES)?;
        Ok(bytes)
    }

    /// Strictly parse a bounded, fully validated dual-signature envelope.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProtocolError> {
        strict_json(bytes, MAX_ENVELOPE_BYTES, "signed envelope")
    }

    /// Verify both signatures in the same caller-supplied context.
    pub fn verify<C: SignatureAdapter, P: SignatureAdapter>(
        &self,
        classical: &C,
        classical_public_key: &[u8],
        post_quantum: &P,
        post_quantum_public_key: &[u8],
        context: &[u8],
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        if classical.algorithm() != KeyAlgorithm::Ed25519
            || post_quantum.algorithm() != KeyAlgorithm::MlDsa65Experimental
        {
            return Err(ProtocolError::UnsupportedAlgorithm);
        }
        let message = self.envelope.signing_bytes()?;
        classical.verify(
            classical_public_key,
            &message,
            context,
            &self.classical_signature.signature,
        )?;
        post_quantum.verify(
            post_quantum_public_key,
            &message,
            context,
            &self.post_quantum_signature.signature,
        )
    }
}

impl Validate for DualSignatureEnvelope {
    fn validate_model(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

/// Create both mandatory envelope signatures.
#[allow(clippy::too_many_arguments)]
pub fn sign_envelope<C: SignatureAdapter, P: SignatureAdapter>(
    envelope: KeyEnvelope,
    classical: &C,
    classical_key_id: Uuid,
    classical_secret_key: &[u8],
    post_quantum: &P,
    post_quantum_key_id: Uuid,
    post_quantum_secret_key: &[u8],
    context: &[u8],
) -> Result<DualSignatureEnvelope, ProtocolError> {
    if classical.algorithm() != KeyAlgorithm::Ed25519
        || post_quantum.algorithm() != KeyAlgorithm::MlDsa65Experimental
    {
        return Err(ProtocolError::UnsupportedAlgorithm);
    }
    let message = envelope.signing_bytes()?;
    let signed = DualSignatureEnvelope {
        envelope,
        classical_signature: DetachedSignature {
            algorithm: classical.algorithm(),
            key_id: classical_key_id,
            signature: classical.sign(classical_secret_key, &message, context)?,
        },
        post_quantum_signature: DetachedSignature {
            algorithm: post_quantum.algorithm(),
            key_id: post_quantum_key_id,
            signature: post_quantum.sign(post_quantum_secret_key, &message, context)?,
        },
    };
    signed.validate()?;
    Ok(signed)
}

/// Verify independent Ed25519 and ML-DSA-65 signatures over one message.
///
/// This helper is suitable for server-side verification. It does not combine
/// the algorithms into a new signature scheme: both signatures must verify in
/// the same explicit context.
#[allow(clippy::too_many_arguments)]
pub fn verify_ed25519_ml_dsa65_signatures(
    ed25519_public_key: &[u8],
    ed25519_signature: &[u8],
    ml_dsa_65_public_key: &[u8],
    ml_dsa_65_signature: &[u8],
    message: &[u8],
    context: &[u8],
) -> Result<(), ProtocolError> {
    ensure_limit("signed message", message.len(), MAX_PLAINTEXT_BYTES)?;
    Ed25519Adapter.verify(ed25519_public_key, message, context, ed25519_signature)?;
    LibcruxMlDsa65Experimental.verify(ml_dsa_65_public_key, message, context, ml_dsa_65_signature)
}

/// Zeroizing pair of independent Ed25519 and ML-DSA-65 signatures.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct DualSignatureBytes {
    ed25519: Vec<u8>,
    ml_dsa_65: Vec<u8>,
}

impl DualSignatureBytes {
    /// Borrow the Ed25519 signature.
    pub fn ed25519(&self) -> &[u8] {
        &self.ed25519
    }

    /// Borrow the ML-DSA-65 signature.
    pub fn ml_dsa_65(&self) -> &[u8] {
        &self.ml_dsa_65
    }
}

/// Sign one message independently with Ed25519 and ML-DSA-65 in an explicit
/// shared context.
pub fn sign_ed25519_ml_dsa65(
    ed25519_private_key: &[u8],
    ml_dsa_65_private_key: &[u8],
    message: &[u8],
    context: &[u8],
) -> Result<DualSignatureBytes, ProtocolError> {
    ensure_limit("signed message", message.len(), MAX_PLAINTEXT_BYTES)?;
    Ok(DualSignatureBytes {
        ed25519: Ed25519Adapter.sign(ed25519_private_key, message, context)?,
        ml_dsa_65: LibcruxMlDsa65Experimental.sign(ml_dsa_65_private_key, message, context)?,
    })
}

/// Verify both signatures on an already parsed dual-signature envelope.
pub fn verify_dual_signature_envelope(
    envelope: &DualSignatureEnvelope,
    ed25519_public_key: &[u8],
    ml_dsa_65_public_key: &[u8],
    context: &[u8],
) -> Result<(), ProtocolError> {
    envelope.verify(
        &Ed25519Adapter,
        ed25519_public_key,
        &LibcruxMlDsa65Experimental,
        ml_dsa_65_public_key,
        context,
    )
}

/// Strictly parse canonical JSON and verify both envelope signatures.
pub fn verify_dual_signature_envelope_json(
    envelope_json: &[u8],
    ed25519_public_key: &[u8],
    ml_dsa_65_public_key: &[u8],
    context: &[u8],
) -> Result<DualSignatureEnvelope, ProtocolError> {
    let envelope = DualSignatureEnvelope::from_json(envelope_json)?;
    verify_dual_signature_envelope(&envelope, ed25519_public_key, ml_dsa_65_public_key, context)?;
    Ok(envelope)
}

/// Zeroizing generated signature key pair.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SignatureKeyPair {
    public_key: Vec<u8>,
    secret_key: Vec<u8>,
}

impl SignatureKeyPair {
    /// Borrow the standardized public-key bytes.
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Borrow the secret-key bytes. Keep these in protected storage.
    pub fn secret_key(&self) -> &[u8] {
        &self.secret_key
    }
}

/// Narrow signature adapter used by dual envelopes and recovery approvals.
pub trait SignatureAdapter {
    /// Algorithm represented by this adapter.
    fn algorithm(&self) -> KeyAlgorithm;
    /// Generate a fresh key pair.
    fn generate_key_pair(&self) -> Result<SignatureKeyPair, ProtocolError>;
    /// Sign in an explicit protocol context.
    fn sign(
        &self,
        secret_key: &[u8],
        message: &[u8],
        context: &[u8],
    ) -> Result<Vec<u8>, ProtocolError>;
    /// Verify in the same explicit protocol context.
    fn verify(
        &self,
        public_key: &[u8],
        message: &[u8],
        context: &[u8],
        signature: &[u8],
    ) -> Result<(), ProtocolError>;
}

/// Audited-library Ed25519 adapter with protocol-level context binding.
#[derive(Debug, Default, Clone, Copy)]
pub struct Ed25519Adapter;

impl SignatureAdapter for Ed25519Adapter {
    fn algorithm(&self) -> KeyAlgorithm {
        KeyAlgorithm::Ed25519
    }

    fn generate_key_pair(&self) -> Result<SignatureKeyPair, ProtocolError> {
        let mut secret = [0u8; ED25519_SECRET_KEY_BYTES];
        fill_random(&mut secret)?;
        let key = SigningKey::from_bytes(&secret);
        let pair = SignatureKeyPair {
            public_key: key.verifying_key().to_bytes().to_vec(),
            secret_key: secret.to_vec(),
        };
        secret.zeroize();
        Ok(pair)
    }

    fn sign(
        &self,
        secret_key: &[u8],
        message: &[u8],
        context: &[u8],
    ) -> Result<Vec<u8>, ProtocolError> {
        validate_signature_context(context)?;
        let secret = exact_array::<ED25519_SECRET_KEY_BYTES>("Ed25519 secret key", secret_key)?;
        let key = SigningKey::from_bytes(&secret);
        Ok(key
            .sign(&contextual_ed25519_message(context, message))
            .to_bytes()
            .to_vec())
    }

    fn verify(
        &self,
        public_key: &[u8],
        message: &[u8],
        context: &[u8],
        signature: &[u8],
    ) -> Result<(), ProtocolError> {
        validate_signature_context(context)?;
        let public = exact_array::<ED25519_PUBLIC_KEY_BYTES>("Ed25519 public key", public_key)?;
        let key =
            VerifyingKey::from_bytes(&public).map_err(|_| ProtocolError::SignatureVerification)?;
        let signature =
            Signature::from_slice(signature).map_err(|_| ProtocolError::SignatureVerification)?;
        key.verify_strict(&contextual_ed25519_message(context, message), &signature)
            .map_err(|_| ProtocolError::SignatureVerification)
    }
}

/// Zeroizing generated KEM key pair.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct KemKeyPair {
    public_key: Vec<u8>,
    secret_key: Vec<u8>,
}

impl KemKeyPair {
    /// Borrow the standardized public-key bytes.
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Borrow the secret-key bytes. Keep these in protected storage.
    pub fn secret_key(&self) -> &[u8] {
        &self.secret_key
    }
}

/// Result of a KEM encapsulation.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct KemEncapsulation {
    /// Standardized KEM ciphertext sent to the recipient.
    pub ciphertext: Vec<u8>,
    shared_secret: [u8; KEY_BYTES],
}

impl KemEncapsulation {
    /// Borrow the zeroizing 256-bit shared secret.
    pub fn shared_secret(&self) -> &[u8; KEY_BYTES] {
        &self.shared_secret
    }
}

/// Narrow KEM adapter suitable for wrapping a random payload DEK.
pub trait KemAdapter {
    /// Algorithm represented by this adapter.
    fn algorithm(&self) -> KeyAlgorithm;
    /// Generate a fresh KEM key pair.
    fn generate_key_pair(&self) -> Result<KemKeyPair, ProtocolError>;
    /// Encapsulate to a validated public key.
    fn encapsulate(&self, public_key: &[u8]) -> Result<KemEncapsulation, ProtocolError>;
    /// Decapsulate a validated ciphertext.
    fn decapsulate(
        &self,
        secret_key: &[u8],
        ciphertext: &[u8],
    ) -> Result<[u8; KEY_BYTES], ProtocolError>;
}

/// Experimental libcrux ML-KEM-768 adapter.
///
/// The pinned libcrux API is used directly; no substitute or placeholder
/// cryptography is present. This adapter is not a reviewed hybrid construction
/// and does not pass the production audit gate by itself.
#[derive(Debug, Default, Clone, Copy)]
pub struct LibcruxMlKem768Experimental;

impl KemAdapter for LibcruxMlKem768Experimental {
    fn algorithm(&self) -> KeyAlgorithm {
        KeyAlgorithm::MlKem768Experimental
    }

    fn generate_key_pair(&self) -> Result<KemKeyPair, ProtocolError> {
        let mut randomness = [0u8; libcrux_ml_kem::KEY_GENERATION_SEED_SIZE];
        fill_random(&mut randomness)?;
        let key_pair = libcrux_ml_kem::mlkem768::generate_key_pair(randomness);
        Ok(KemKeyPair {
            public_key: key_pair.pk().to_vec(),
            secret_key: key_pair.sk().to_vec(),
        })
    }

    fn encapsulate(&self, public_key: &[u8]) -> Result<KemEncapsulation, ProtocolError> {
        if public_key.len() != ML_KEM_768_PUBLIC_KEY_BYTES {
            return Err(ProtocolError::InvalidLength {
                field: "ML-KEM-768 public key",
                expected: ML_KEM_768_PUBLIC_KEY_BYTES,
            });
        }
        let public_key = libcrux_ml_kem::mlkem768::MlKem768PublicKey::try_from(public_key)
            .map_err(|_| ProtocolError::InvalidFormat("ML-KEM-768 public key"))?;
        if !libcrux_ml_kem::mlkem768::validate_public_key(&public_key) {
            return Err(ProtocolError::InvalidFormat("ML-KEM-768 public key"));
        }
        let mut randomness = [0u8; libcrux_ml_kem::SHARED_SECRET_SIZE];
        fill_random(&mut randomness)?;
        let (ciphertext, shared_secret) =
            libcrux_ml_kem::mlkem768::encapsulate(&public_key, randomness);
        Ok(KemEncapsulation {
            ciphertext: ciphertext.as_slice().to_vec(),
            shared_secret,
        })
    }

    fn decapsulate(
        &self,
        secret_key: &[u8],
        ciphertext: &[u8],
    ) -> Result<[u8; KEY_BYTES], ProtocolError> {
        if secret_key.len() != ML_KEM_768_PRIVATE_KEY_BYTES {
            return Err(ProtocolError::InvalidLength {
                field: "ML-KEM-768 secret key",
                expected: ML_KEM_768_PRIVATE_KEY_BYTES,
            });
        }
        if ciphertext.len() != ML_KEM_768_CIPHERTEXT_BYTES {
            return Err(ProtocolError::InvalidLength {
                field: "ML-KEM-768 ciphertext",
                expected: ML_KEM_768_CIPHERTEXT_BYTES,
            });
        }
        let secret_key = libcrux_ml_kem::mlkem768::MlKem768PrivateKey::try_from(secret_key)
            .map_err(|_| ProtocolError::InvalidFormat("ML-KEM-768 secret key"))?;
        let ciphertext = libcrux_ml_kem::mlkem768::MlKem768Ciphertext::try_from(ciphertext)
            .map_err(|_| ProtocolError::InvalidFormat("ML-KEM-768 ciphertext"))?;
        Ok(libcrux_ml_kem::mlkem768::decapsulate(
            &secret_key,
            &ciphertext,
        ))
    }
}

/// Experimental libcrux ML-DSA-65 signature adapter.
///
/// The adapter uses the stable pinned byte-oriented API and FIPS 204 context
/// parameter. It remains behind the production audit gate with the hybrid suite.
#[derive(Debug, Default, Clone, Copy)]
pub struct LibcruxMlDsa65Experimental;

impl SignatureAdapter for LibcruxMlDsa65Experimental {
    fn algorithm(&self) -> KeyAlgorithm {
        KeyAlgorithm::MlDsa65Experimental
    }

    fn generate_key_pair(&self) -> Result<SignatureKeyPair, ProtocolError> {
        let mut randomness = [0u8; libcrux_ml_dsa::KEY_GENERATION_RANDOMNESS_SIZE];
        fill_random(&mut randomness)?;
        let key_pair = libcrux_ml_dsa::ml_dsa_65::generate_key_pair(randomness);
        Ok(SignatureKeyPair {
            public_key: key_pair.verification_key.as_slice().to_vec(),
            secret_key: key_pair.signing_key.as_slice().to_vec(),
        })
    }

    fn sign(
        &self,
        secret_key: &[u8],
        message: &[u8],
        context: &[u8],
    ) -> Result<Vec<u8>, ProtocolError> {
        validate_signature_context(context)?;
        let secret = exact_array::<ML_DSA_65_SECRET_KEY_BYTES>("ML-DSA-65 secret key", secret_key)?;
        let key = libcrux_ml_dsa::ml_dsa_65::MLDSA65SigningKey::new(secret);
        let mut randomness = [0u8; libcrux_ml_dsa::SIGNING_RANDOMNESS_SIZE];
        fill_random(&mut randomness)?;
        libcrux_ml_dsa::ml_dsa_65::sign(&key, message, context, randomness)
            .map(|signature| signature.as_slice().to_vec())
            .map_err(|_| ProtocolError::SignatureVerification)
    }

    fn verify(
        &self,
        public_key: &[u8],
        message: &[u8],
        context: &[u8],
        signature: &[u8],
    ) -> Result<(), ProtocolError> {
        validate_signature_context(context)?;
        let public = exact_array::<ML_DSA_65_PUBLIC_KEY_BYTES>("ML-DSA-65 public key", public_key)?;
        let signature = exact_array::<ML_DSA_65_SIGNATURE_BYTES>("ML-DSA-65 signature", signature)?;
        let public_key = libcrux_ml_dsa::ml_dsa_65::MLDSA65VerificationKey::new(public);
        let signature = libcrux_ml_dsa::ml_dsa_65::MLDSA65Signature::new(signature);
        libcrux_ml_dsa::ml_dsa_65::verify(&public_key, message, context, &signature)
            .map_err(|_| ProtocolError::SignatureVerification)
    }
}

/// Explicitly unavailable production hybrid adapter.
///
/// This type exists to make the audit boundary machine-visible. It performs no
/// cryptography and every operation fails closed with
/// [`ProtocolError::ProductionAuditRequired`].
#[derive(Debug, Default, Clone, Copy)]
pub struct ProductionHybridAdapter;

impl KemAdapter for ProductionHybridAdapter {
    fn algorithm(&self) -> KeyAlgorithm {
        KeyAlgorithm::MlKem768Experimental
    }

    fn generate_key_pair(&self) -> Result<KemKeyPair, ProtocolError> {
        Err(ProtocolError::ProductionAuditRequired)
    }

    fn encapsulate(&self, _public_key: &[u8]) -> Result<KemEncapsulation, ProtocolError> {
        Err(ProtocolError::ProductionAuditRequired)
    }

    fn decapsulate(
        &self,
        _secret_key: &[u8],
        _ciphertext: &[u8],
    ) -> Result<[u8; KEY_BYTES], ProtocolError> {
        Err(ProtocolError::ProductionAuditRequired)
    }
}

/// Audit status carried by every non-standard hybrid envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum SuiteAuditStatus {
    /// The construction is experimental and requires an independent audit.
    ProductionAuditRequired = 1,
}

/// Authenticated metadata for one resource-key wrapping operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HybridWrapMetadata {
    /// Resource whose epoch key is wrapped.
    pub resource_id: Uuid,
    /// Intended recipient device.
    pub recipient_device_id: Uuid,
    /// Resource epoch bound to the wrapped key.
    pub resource_epoch: u64,
    /// Previous epoch commitment, or all-zero for epoch zero.
    pub previous_epoch_hash: [u8; HASH_BYTES],
    /// Caller-defined tenant/operation context.
    pub context: Vec<u8>,
}

impl HybridWrapMetadata {
    /// Construct validated metadata from native types.
    pub fn new(
        resource_id: Uuid,
        recipient_device_id: Uuid,
        resource_epoch: u64,
        previous_epoch_hash: [u8; HASH_BYTES],
        context: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        let metadata = Self {
            resource_id,
            recipient_device_id,
            resource_epoch,
            previous_epoch_hash,
            context,
        };
        metadata.validate()?;
        Ok(metadata)
    }

    /// Construct metadata from fixed-width foreign-function inputs.
    pub fn from_parts_bytes(
        resource_id: &[u8],
        recipient_device_id: &[u8],
        resource_epoch: u64,
        previous_epoch_hash: &[u8],
        context: &[u8],
    ) -> Result<Self, ProtocolError> {
        Self::new(
            Uuid::from_bytes(exact_array::<16>("resource id", resource_id)?),
            Uuid::from_bytes(exact_array::<16>(
                "recipient device id",
                recipient_device_id,
            )?),
            resource_epoch,
            exact_array::<HASH_BYTES>("previous epoch hash", previous_epoch_hash)?,
            context.to_vec(),
        )
    }

    /// Validate IDs, context bounds, and epoch chaining.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.resource_id.is_nil() || self.recipient_device_id.is_nil() {
            return Err(ProtocolError::InvalidFormat("nil hybrid metadata id"));
        }
        ensure_limit(
            "hybrid metadata context",
            self.context.len(),
            MAX_HEADER_CONTEXT_BYTES,
        )?;
        validate_generation(self.resource_epoch, &self.previous_epoch_hash)
    }

    /// Unique canonical metadata encoding used as authenticated data.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let context_len =
            u16::try_from(self.context.len()).map_err(|_| ProtocolError::SizeLimit {
                field: "hybrid metadata context",
                maximum: u16::MAX as usize,
            })?;
        let mut out = Vec::with_capacity(83 + self.context.len());
        out.extend_from_slice(HYBRID_METADATA_MAGIC);
        out.push(CURRENT_VERSION as u8);
        out.extend_from_slice(self.resource_id.as_bytes());
        out.extend_from_slice(self.recipient_device_id.as_bytes());
        out.extend_from_slice(&self.resource_epoch.to_be_bytes());
        out.extend_from_slice(&self.previous_epoch_hash);
        out.extend_from_slice(&context_len.to_be_bytes());
        out.extend_from_slice(&self.context);
        Ok(out)
    }

    /// Strictly parse canonical metadata.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        ensure_limit(
            "hybrid metadata",
            bytes.len(),
            83 + MAX_HEADER_CONTEXT_BYTES,
        )?;
        let mut reader = Reader::new(bytes);
        if reader.take_array::<8>()? != *HYBRID_METADATA_MAGIC {
            return Err(ProtocolError::InvalidFormat("hybrid metadata magic"));
        }
        ProtocolVersion::try_from(reader.byte()?)?;
        let resource_id = Uuid::from_bytes(reader.take_array::<16>()?);
        let recipient_device_id = Uuid::from_bytes(reader.take_array::<16>()?);
        let resource_epoch = u64::from_be_bytes(reader.take_array::<8>()?);
        let previous_epoch_hash = reader.take_array::<HASH_BYTES>()?;
        let context_len = u16::from_be_bytes(reader.take_array::<2>()?) as usize;
        ensure_limit(
            "hybrid metadata context",
            context_len,
            MAX_HEADER_CONTEXT_BYTES,
        )?;
        let context = reader.take(context_len)?.to_vec();
        reader.finish()?;
        let metadata = Self::new(
            resource_id,
            recipient_device_id,
            resource_epoch,
            previous_epoch_hash,
            context,
        )?;
        if metadata.canonical_bytes()?.as_slice() != bytes {
            return Err(ProtocolError::InvalidFormat(
                "non-canonical hybrid metadata",
            ));
        }
        Ok(metadata)
    }
}

/// Experimental X25519 + ML-KEM-768 resource-key envelope.
///
/// The two shared secrets are combined with RFC 5869 HKDF-SHA-256 and the
/// resource key is encrypted with AES-256-GCM. This is a versioned Sprout
/// construction, **not** standardized X-Wing. Every envelope carries
/// [`SuiteAuditStatus::ProductionAuditRequired`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentalWrappedResourceKey {
    /// Protocol version.
    pub version: ProtocolVersion,
    /// Non-standard suite version.
    pub suite_version: u16,
    /// Explicit production audit status.
    pub audit_status: SuiteAuditStatus,
    /// Authenticated resource/recipient/epoch metadata.
    pub metadata: HybridWrapMetadata,
    /// Sender's ephemeral X25519 public key.
    pub ephemeral_x25519_public_key: [u8; KEY_BYTES],
    /// ML-KEM-768 ciphertext.
    pub ml_kem_768_ciphertext: Vec<u8>,
    /// Fresh AES-GCM nonce.
    pub nonce: [u8; NONCE_BYTES],
    /// Encrypted 32-byte resource key plus GCM tag.
    pub wrapped_resource_key: Vec<u8>,
}

impl ExperimentalWrappedResourceKey {
    /// Validate all fixed fields and bounded nested data.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != CURRENT_VERSION
            || self.suite_version != EXPERIMENTAL_HYBRID_SUITE_V1
            || self.audit_status != SuiteAuditStatus::ProductionAuditRequired
        {
            return Err(ProtocolError::ProductionAuditRequired);
        }
        self.metadata.validate()?;
        if self.ml_kem_768_ciphertext.len() != ML_KEM_768_CIPHERTEXT_BYTES {
            return Err(ProtocolError::InvalidLength {
                field: "ML-KEM-768 ciphertext",
                expected: ML_KEM_768_CIPHERTEXT_BYTES,
            });
        }
        if self.wrapped_resource_key.len() != KEY_BYTES + GCM_TAG_BYTES {
            return Err(ProtocolError::InvalidLength {
                field: "wrapped resource key",
                expected: KEY_BYTES + GCM_TAG_BYTES,
            });
        }
        Ok(())
    }

    fn aad_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let metadata = self.metadata.canonical_bytes()?;
        let mut aad = Vec::with_capacity(
            HYBRID_AAD_DOMAIN.len() + 4 + metadata.len() + KEY_BYTES + ML_KEM_768_CIPHERTEXT_BYTES,
        );
        aad.extend_from_slice(HYBRID_AAD_DOMAIN);
        aad.push(self.version as u8);
        aad.extend_from_slice(&self.suite_version.to_be_bytes());
        aad.push(self.audit_status as u8);
        aad.extend_from_slice(&(metadata.len() as u16).to_be_bytes());
        aad.extend_from_slice(&metadata);
        aad.extend_from_slice(&self.ephemeral_x25519_public_key);
        aad.extend_from_slice(&self.ml_kem_768_ciphertext);
        Ok(aad)
    }

    /// Strict bounded binary envelope encoding.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let metadata = self.metadata.canonical_bytes()?;
        let mut out = Vec::with_capacity(
            16 + metadata.len()
                + KEY_BYTES
                + ML_KEM_768_CIPHERTEXT_BYTES
                + NONCE_BYTES
                + KEY_BYTES
                + GCM_TAG_BYTES,
        );
        out.extend_from_slice(HYBRID_ENVELOPE_MAGIC);
        out.push(self.version as u8);
        out.extend_from_slice(&self.suite_version.to_be_bytes());
        out.push(self.audit_status as u8);
        out.extend_from_slice(&(metadata.len() as u16).to_be_bytes());
        out.extend_from_slice(&metadata);
        out.extend_from_slice(&self.ephemeral_x25519_public_key);
        out.extend_from_slice(&self.ml_kem_768_ciphertext);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.wrapped_resource_key);
        Ok(out)
    }

    /// Strictly parse a bounded binary hybrid envelope.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let maximum = 16
            + 83
            + MAX_HEADER_CONTEXT_BYTES
            + KEY_BYTES
            + ML_KEM_768_CIPHERTEXT_BYTES
            + NONCE_BYTES
            + KEY_BYTES
            + GCM_TAG_BYTES;
        ensure_limit("hybrid envelope", bytes.len(), maximum)?;
        let mut reader = Reader::new(bytes);
        if reader.take_array::<8>()? != *HYBRID_ENVELOPE_MAGIC {
            return Err(ProtocolError::InvalidFormat("hybrid envelope magic"));
        }
        let version = ProtocolVersion::try_from(reader.byte()?)?;
        let suite_version = u16::from_be_bytes(reader.take_array::<2>()?);
        let audit_status = match reader.byte()? {
            1 => SuiteAuditStatus::ProductionAuditRequired,
            _ => return Err(ProtocolError::ProductionAuditRequired),
        };
        let metadata_len = u16::from_be_bytes(reader.take_array::<2>()?) as usize;
        ensure_limit(
            "hybrid metadata",
            metadata_len,
            83 + MAX_HEADER_CONTEXT_BYTES,
        )?;
        let metadata = HybridWrapMetadata::from_canonical_bytes(reader.take(metadata_len)?)?;
        let ephemeral_x25519_public_key = reader.take_array::<KEY_BYTES>()?;
        let ml_kem_768_ciphertext = reader.take(ML_KEM_768_CIPHERTEXT_BYTES)?.to_vec();
        let nonce = reader.take_array::<NONCE_BYTES>()?;
        let wrapped_resource_key = reader.take(KEY_BYTES + GCM_TAG_BYTES)?.to_vec();
        reader.finish()?;
        let envelope = Self {
            version,
            suite_version,
            audit_status,
            metadata,
            ephemeral_x25519_public_key,
            ml_kem_768_ciphertext,
            nonce,
            wrapped_resource_key,
        };
        envelope.validate()?;
        Ok(envelope)
    }
}

/// Wrap a resource key with the experimental, audit-gated hybrid construction.
pub fn wrap_resource_key(
    resource_key: &ResourceKey,
    recipient_x25519_public_key: &[u8],
    recipient_ml_kem_768_public_key: &[u8],
    metadata: HybridWrapMetadata,
) -> Result<ExperimentalWrappedResourceKey, ProtocolError> {
    metadata.validate()?;
    let recipient_x25519_public_key = X25519PublicKey::from(exact_array::<KEY_BYTES>(
        "recipient X25519 public key",
        recipient_x25519_public_key,
    )?);
    let mut ephemeral_seed = [0u8; KEY_BYTES];
    fill_random(&mut ephemeral_seed)?;
    let ephemeral_secret = X25519StaticSecret::from(ephemeral_seed);
    ephemeral_seed.zeroize();
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);
    let mut x25519_shared = *ephemeral_secret
        .diffie_hellman(&recipient_x25519_public_key)
        .as_bytes();
    if bool::from(x25519_shared.ct_eq(&[0; KEY_BYTES])) {
        x25519_shared.zeroize();
        return Err(ProtocolError::InvalidFormat("invalid X25519 public key"));
    }

    let kem = LibcruxMlKem768Experimental;
    let encapsulated = kem.encapsulate(recipient_ml_kem_768_public_key)?;
    let mut ml_kem_shared = *encapsulated.shared_secret();
    let mut nonce = [0u8; NONCE_BYTES];
    fill_random(&mut nonce)?;

    let mut envelope = ExperimentalWrappedResourceKey {
        version: CURRENT_VERSION,
        suite_version: EXPERIMENTAL_HYBRID_SUITE_V1,
        audit_status: SuiteAuditStatus::ProductionAuditRequired,
        metadata,
        ephemeral_x25519_public_key: *ephemeral_public.as_bytes(),
        ml_kem_768_ciphertext: encapsulated.ciphertext.clone(),
        nonce,
        wrapped_resource_key: vec![0; KEY_BYTES + GCM_TAG_BYTES],
    };
    let aad = envelope.aad_bytes()?;
    let mut kek = derive_experimental_hybrid_kek(&x25519_shared, &ml_kem_shared, &aad)?;
    x25519_shared.zeroize();
    ml_kem_shared.zeroize();
    let cipher = Aes256Gcm::new(&Array(kek));
    envelope.wrapped_resource_key = cipher
        .encrypt(
            &Array(nonce),
            Payload {
                msg: resource_key.as_bytes(),
                aad: &aad,
            },
        )
        .map_err(|_| ProtocolError::AuthenticationFailed)?;
    kek.zeroize();
    envelope.validate()?;
    Ok(envelope)
}

/// Unwrap only when the envelope metadata equals independently expected data.
pub fn unwrap_resource_key(
    envelope: &ExperimentalWrappedResourceKey,
    recipient_x25519_private_key: &[u8],
    recipient_ml_kem_768_private_key: &[u8],
    expected_metadata: &HybridWrapMetadata,
) -> Result<ResourceKey, ProtocolError> {
    envelope.validate()?;
    expected_metadata.validate()?;
    let actual_metadata = envelope.metadata.canonical_bytes()?;
    let expected_metadata = expected_metadata.canonical_bytes()?;
    if actual_metadata.len() != expected_metadata.len()
        || !bool::from(
            actual_metadata
                .as_slice()
                .ct_eq(expected_metadata.as_slice()),
        )
    {
        return Err(ProtocolError::ContextMismatch);
    }

    let recipient_secret = X25519StaticSecret::from(exact_array::<KEY_BYTES>(
        "recipient X25519 private key",
        recipient_x25519_private_key,
    )?);
    let ephemeral_public = X25519PublicKey::from(envelope.ephemeral_x25519_public_key);
    let mut x25519_shared = *recipient_secret
        .diffie_hellman(&ephemeral_public)
        .as_bytes();
    if bool::from(x25519_shared.ct_eq(&[0; KEY_BYTES])) {
        x25519_shared.zeroize();
        return Err(ProtocolError::AuthenticationFailed);
    }
    let mut ml_kem_shared = LibcruxMlKem768Experimental.decapsulate(
        recipient_ml_kem_768_private_key,
        &envelope.ml_kem_768_ciphertext,
    )?;
    let aad = envelope.aad_bytes()?;
    let mut kek = derive_experimental_hybrid_kek(&x25519_shared, &ml_kem_shared, &aad)?;
    x25519_shared.zeroize();
    ml_kem_shared.zeroize();
    let cipher = Aes256Gcm::new(&Array(kek));
    let mut plaintext = cipher
        .decrypt(
            &Array(envelope.nonce),
            Payload {
                msg: &envelope.wrapped_resource_key,
                aad: &aad,
            },
        )
        .map_err(|_| ProtocolError::AuthenticationFailed)?;
    kek.zeroize();
    let key = ResourceKey::from_slice(&plaintext)?;
    plaintext.zeroize();
    Ok(key)
}

fn derive_experimental_hybrid_kek(
    x25519_shared: &[u8; KEY_BYTES],
    ml_kem_shared: &[u8; KEY_BYTES],
    aad: &[u8],
) -> Result<[u8; KEY_BYTES], ProtocolError> {
    let mut ikm = [0u8; KEY_BYTES * 2];
    ikm[..KEY_BYTES].copy_from_slice(x25519_shared);
    ikm[KEY_BYTES..].copy_from_slice(ml_kem_shared);
    let aad_hash = hash_bytes(aad);
    let mut salt_input = Vec::with_capacity(HYBRID_KDF_DOMAIN.len() + HASH_BYTES);
    salt_input.extend_from_slice(HYBRID_KDF_DOMAIN);
    salt_input.extend_from_slice(&aad_hash);
    let salt = hash_bytes(&salt_input);
    let mut info = Vec::with_capacity(HYBRID_KDF_DOMAIN.len() + 4 + HASH_BYTES);
    info.extend_from_slice(HYBRID_KDF_DOMAIN);
    info.extend_from_slice(&EXPERIMENTAL_HYBRID_SUITE_V1.to_be_bytes());
    info.extend_from_slice(&aad_hash);
    let mut output = hkdf_sha256(&ikm, &salt, &info, KEY_BYTES)?;
    ikm.zeroize();
    let key = exact_array::<KEY_BYTES>("hybrid KEK", &output)?;
    output.zeroize();
    Ok(key)
}

/// One committed share of a uniformly random 32-byte recovery secret.
#[derive(Debug, Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct RecoveryShare {
    total: u8,
    index: u8,
    context_hash: [u8; HASH_BYTES],
    secret_commitment: [u8; HASH_BYTES],
    share_commitment: [u8; HASH_BYTES],
    share: [u8; RECOVERY_SHARE_PAYLOAD_BYTES],
}

impl RecoveryShare {
    /// One-based share index.
    pub fn index(&self) -> u8 {
        self.index
    }

    /// Required total number of shares.
    pub fn total(&self) -> u8 {
        self.total
    }

    /// Commitment to the recovered secret and context.
    pub fn secret_commitment(&self) -> &[u8; HASH_BYTES] {
        &self.secret_commitment
    }

    /// Validate bounds and the individual share commitment.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.total < 2
            || self.total as usize > MAX_RECOVERY_PARTICIPANTS
            || self.index == 0
            || self.index > self.total
        {
            return Err(ProtocolError::InvalidFormat("recovery share index"));
        }
        let expected = recovery_share_commitment(
            self.total,
            self.index,
            &self.context_hash,
            &self.secret_commitment,
            &self.share,
        );
        if !bool::from(expected.ct_eq(&self.share_commitment)) {
            return Err(ProtocolError::AuthenticationFailed);
        }
        Ok(())
    }

    /// Strict fixed-size binary representation.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let mut out = Vec::with_capacity(171);
        out.extend_from_slice(RECOVERY_SHARE_MAGIC);
        out.push(CURRENT_VERSION as u8);
        out.push(self.total);
        out.push(self.index);
        out.extend_from_slice(&self.context_hash);
        out.extend_from_slice(&self.secret_commitment);
        out.extend_from_slice(&self.share_commitment);
        out.extend_from_slice(&self.share);
        Ok(out)
    }

    /// Strictly parse one bounded share.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != 171 {
            return Err(ProtocolError::InvalidLength {
                field: "recovery share",
                expected: 171,
            });
        }
        let mut reader = Reader::new(bytes);
        if reader.take_array::<8>()? != *RECOVERY_SHARE_MAGIC {
            return Err(ProtocolError::InvalidFormat("recovery share magic"));
        }
        ProtocolVersion::try_from(reader.byte()?)?;
        let share = Self {
            total: reader.byte()?,
            index: reader.byte()?,
            context_hash: reader.take_array::<HASH_BYTES>()?,
            secret_commitment: reader.take_array::<HASH_BYTES>()?,
            share_commitment: reader.take_array::<HASH_BYTES>()?,
            share: reader.take_array::<RECOVERY_SHARE_PAYLOAD_BYTES>()?,
        };
        reader.finish()?;
        share.validate()?;
        Ok(share)
    }
}

/// Zeroizing result of an n-of-n recovery split.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct RecoverySplit {
    shares: Vec<RecoveryShare>,
    secret_commitment: [u8; HASH_BYTES],
}

impl RecoverySplit {
    /// Number of generated shares.
    pub fn share_count(&self) -> usize {
        self.shares.len()
    }

    /// Borrow a share by zero-based result position.
    pub fn share(&self, position: usize) -> Result<&RecoveryShare, ProtocolError> {
        self.shares
            .get(position)
            .ok_or(ProtocolError::InvalidFormat("recovery share position"))
    }

    /// Commitment shared by all encoded shares.
    pub fn secret_commitment(&self) -> &[u8; HASH_BYTES] {
        &self.secret_commitment
    }

    /// Encode every share into a strict bounded bundle.
    pub fn bundle(&self) -> Result<Vec<u8>, ProtocolError> {
        pack_recovery_shares(&self.shares)
    }
}

/// Zeroizing recovered secret.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct RecoverySecret([u8; RECOVERY_SECRET_BYTES]);

impl RecoverySecret {
    /// Borrow the recovered 32-byte secret.
    pub fn as_bytes(&self) -> &[u8; RECOVERY_SECRET_BYTES] {
        &self.0
    }
}

/// Split a 32-byte secret with information-theoretic XOR n-of-n sharing.
///
/// Any `n-1` shares are statistically independent of the secret. A second
/// random 32-byte blinding value is shared with the secret, making the public
/// commitment hiding even if the input secret has lower entropy. Splitting and
/// combining are local operations; no plaintext secret is sent to a server.
pub fn split_recovery_secret_n_of_n(
    secret: &[u8],
    participant_count: u8,
    context: &[u8],
) -> Result<RecoverySplit, ProtocolError> {
    if participant_count < 2 || participant_count as usize > MAX_RECOVERY_PARTICIPANTS {
        return Err(ProtocolError::InvalidFormat("recovery participant count"));
    }
    ensure_limit("recovery context", context.len(), MAX_HEADER_CONTEXT_BYTES)?;
    let mut secret = exact_array::<RECOVERY_SECRET_BYTES>("recovery secret", secret)?;
    let mut recovery_payload = [0u8; RECOVERY_SHARE_PAYLOAD_BYTES];
    recovery_payload[..RECOVERY_SECRET_BYTES].copy_from_slice(&secret);
    secret.zeroize();
    fill_random(&mut recovery_payload[RECOVERY_SECRET_BYTES..])?;
    let context_hash = recovery_context_hash(context);
    let secret_commitment =
        recovery_secret_commitment(participant_count, &context_hash, &recovery_payload);
    let mut aggregate = [0u8; RECOVERY_SHARE_PAYLOAD_BYTES];
    let mut shares = Vec::with_capacity(participant_count as usize);
    for index in 1..participant_count {
        let mut share = [0u8; RECOVERY_SHARE_PAYLOAD_BYTES];
        fill_random(&mut share)?;
        for (aggregate_byte, share_byte) in aggregate.iter_mut().zip(&share) {
            *aggregate_byte ^= share_byte;
        }
        shares.push(RecoveryShare {
            total: participant_count,
            index,
            context_hash,
            secret_commitment,
            share_commitment: recovery_share_commitment(
                participant_count,
                index,
                &context_hash,
                &secret_commitment,
                &share,
            ),
            share,
        });
    }
    let mut final_share = [0u8; RECOVERY_SHARE_PAYLOAD_BYTES];
    for ((final_byte, secret_byte), aggregate_byte) in final_share
        .iter_mut()
        .zip(&recovery_payload)
        .zip(&aggregate)
    {
        *final_byte = secret_byte ^ aggregate_byte;
    }
    shares.push(RecoveryShare {
        total: participant_count,
        index: participant_count,
        context_hash,
        secret_commitment,
        share_commitment: recovery_share_commitment(
            participant_count,
            participant_count,
            &context_hash,
            &secret_commitment,
            &final_share,
        ),
        share: final_share,
    });
    recovery_payload.zeroize();
    aggregate.zeroize();
    Ok(RecoverySplit {
        shares,
        secret_commitment,
    })
}

/// Combine all unique, committed shares in the original context.
pub fn combine_recovery_secret_n_of_n(
    shares: &[RecoveryShare],
    context: &[u8],
) -> Result<RecoverySecret, ProtocolError> {
    ensure_limit("recovery context", context.len(), MAX_HEADER_CONTEXT_BYTES)?;
    if shares.is_empty() || shares.len() > MAX_RECOVERY_PARTICIPANTS {
        return Err(ProtocolError::RecoveryIncomplete);
    }
    let expected_context_hash = recovery_context_hash(context);
    let total = shares[0].total;
    if total < 2 || shares.len() != total as usize {
        return Err(ProtocolError::RecoveryIncomplete);
    }
    let secret_commitment = shares[0].secret_commitment;
    let mut seen = [false; MAX_RECOVERY_PARTICIPANTS + 1];
    let mut recovery_payload = [0u8; RECOVERY_SHARE_PAYLOAD_BYTES];
    for share in shares {
        share.validate()?;
        let index = share.index as usize;
        if share.context_hash != expected_context_hash {
            recovery_payload.zeroize();
            return Err(ProtocolError::ContextMismatch);
        }
        if share.total != total || share.secret_commitment != secret_commitment || seen[index] {
            recovery_payload.zeroize();
            return Err(ProtocolError::InvalidFormat(
                "inconsistent or duplicate recovery share",
            ));
        }
        seen[index] = true;
        for (secret_byte, share_byte) in recovery_payload.iter_mut().zip(&share.share) {
            *secret_byte ^= share_byte;
        }
    }
    if seen[1..=total as usize].iter().any(|seen| !seen)
        || recovery_secret_commitment(total, &expected_context_hash, &recovery_payload)
            != secret_commitment
    {
        recovery_payload.zeroize();
        return Err(ProtocolError::AuthenticationFailed);
    }
    let mut secret = [0u8; RECOVERY_SECRET_BYTES];
    secret.copy_from_slice(&recovery_payload[..RECOVERY_SECRET_BYTES]);
    recovery_payload.zeroize();
    Ok(RecoverySecret(secret))
}

/// Strictly pack independently stored shares for a typed-array boundary.
pub fn pack_recovery_shares(shares: &[RecoveryShare]) -> Result<Vec<u8>, ProtocolError> {
    ensure_nonempty_bounded("recovery shares", shares.len(), MAX_RECOVERY_PARTICIPANTS)?;
    let mut out = Vec::with_capacity(10 + shares.len() * 173);
    out.extend_from_slice(RECOVERY_BUNDLE_MAGIC);
    out.push(CURRENT_VERSION as u8);
    out.push(shares.len() as u8);
    for share in shares {
        let bytes = share.to_bytes()?;
        out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

/// Strictly parse a bounded recovery share bundle.
pub fn unpack_recovery_shares(bytes: &[u8]) -> Result<Vec<RecoveryShare>, ProtocolError> {
    ensure_limit(
        "recovery share bundle",
        bytes.len(),
        10 + MAX_RECOVERY_PARTICIPANTS * 173,
    )?;
    let mut reader = Reader::new(bytes);
    if reader.take_array::<8>()? != *RECOVERY_BUNDLE_MAGIC {
        return Err(ProtocolError::InvalidFormat("recovery bundle magic"));
    }
    ProtocolVersion::try_from(reader.byte()?)?;
    let count = reader.byte()? as usize;
    ensure_nonempty_bounded("recovery shares", count, MAX_RECOVERY_PARTICIPANTS)?;
    let mut shares = Vec::with_capacity(count);
    for _ in 0..count {
        let length = u16::from_be_bytes(reader.take_array::<2>()?) as usize;
        if length != 171 {
            return Err(ProtocolError::InvalidLength {
                field: "recovery share",
                expected: 171,
            });
        }
        shares.push(RecoveryShare::from_bytes(reader.take(length)?)?);
    }
    reader.finish()?;
    Ok(shares)
}

/// Parse a bundle and combine all n-of-n shares locally.
pub fn combine_recovery_secret_bundle_n_of_n(
    bundle: &[u8],
    context: &[u8],
) -> Result<RecoverySecret, ProtocolError> {
    let shares = unpack_recovery_shares(bundle)?;
    combine_recovery_secret_n_of_n(&shares, context)
}

fn recovery_context_hash(context: &[u8]) -> [u8; HASH_BYTES] {
    let mut digest = Sha256::new();
    digest.update(RECOVERY_CONTEXT_DOMAIN);
    digest.update((context.len() as u32).to_be_bytes());
    digest.update(context);
    digest.finalize().into()
}

fn recovery_secret_commitment(
    total: u8,
    context_hash: &[u8; HASH_BYTES],
    secret: &[u8; RECOVERY_SHARE_PAYLOAD_BYTES],
) -> [u8; HASH_BYTES] {
    let mut digest = Sha256::new();
    digest.update(RECOVERY_SECRET_COMMITMENT_DOMAIN);
    digest.update([CURRENT_VERSION as u8, total]);
    digest.update(context_hash);
    digest.update(secret);
    digest.finalize().into()
}

fn recovery_share_commitment(
    total: u8,
    index: u8,
    context_hash: &[u8; HASH_BYTES],
    secret_commitment: &[u8; HASH_BYTES],
    share: &[u8; RECOVERY_SHARE_PAYLOAD_BYTES],
) -> [u8; HASH_BYTES] {
    let mut digest = Sha256::new();
    digest.update(RECOVERY_SHARE_COMMITMENT_DOMAIN);
    digest.update([CURRENT_VERSION as u8, total, index]);
    digest.update(context_hash);
    digest.update(secret_commitment);
    digest.update(share);
    digest.finalize().into()
}

/// Fresh, zeroizing key material for the next resource epoch.
pub struct ResourceEpochMaterial {
    /// Protocol version.
    pub version: ProtocolVersion,
    /// Resource receiving the new epoch.
    pub resource_id: Uuid,
    /// New epoch number.
    pub epoch: u64,
    /// Random epoch identifier.
    pub epoch_id: Uuid,
    /// Previous epoch commitment/hash.
    pub previous_epoch_hash: [u8; HASH_BYTES],
    /// Hash of caller context.
    pub context_hash: [u8; HASH_BYTES],
    /// Commitment to metadata and both fresh keys.
    pub epoch_commitment: [u8; HASH_BYTES],
    resource_key: [u8; KEY_BYTES],
    header_key: [u8; KEY_BYTES],
}

impl Drop for ResourceEpochMaterial {
    fn drop(&mut self) {
        self.resource_key.zeroize();
        self.header_key.zeroize();
    }
}

impl ResourceEpochMaterial {
    /// Borrow the fresh resource key.
    pub fn resource_key(&self) -> &[u8; KEY_BYTES] {
        &self.resource_key
    }

    /// Borrow the independently domain-separated header key.
    pub fn header_key(&self) -> &[u8; KEY_BYTES] {
        &self.header_key
    }

    /// Canonical public metadata for storage and hash chaining.
    pub fn public_metadata_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(145);
        out.extend_from_slice(RESOURCE_EPOCH_DOMAIN);
        out.push(self.version as u8);
        out.extend_from_slice(self.resource_id.as_bytes());
        out.extend_from_slice(self.epoch_id.as_bytes());
        out.extend_from_slice(&self.epoch.to_be_bytes());
        out.extend_from_slice(&self.previous_epoch_hash);
        out.extend_from_slice(&self.context_hash);
        out.extend_from_slice(&self.epoch_commitment);
        out
    }
}

/// Generate fresh, separated resource/header keys for the next epoch.
pub fn rotate_resource_epoch(
    resource_id: Uuid,
    current_epoch: u64,
    previous_epoch_hash: [u8; HASH_BYTES],
    context: &[u8],
) -> Result<ResourceEpochMaterial, ProtocolError> {
    if resource_id.is_nil() || previous_epoch_hash == [0; HASH_BYTES] {
        return Err(ProtocolError::InvalidHashChain);
    }
    ensure_limit(
        "resource epoch context",
        context.len(),
        MAX_HEADER_CONTEXT_BYTES,
    )?;
    let epoch = current_epoch
        .checked_add(1)
        .ok_or(ProtocolError::InvalidFormat("resource epoch overflow"))?;
    let root = RootKey::generate()?;
    let separated = SeparatedKeys::derive(&root, resource_id)?;
    let mut epoch_id_bytes = [0u8; 16];
    fill_random(&mut epoch_id_bytes)?;
    let epoch_id = Uuid::from_bytes(epoch_id_bytes);
    let context_hash = hash_bytes(context);
    let mut commitment_input = Vec::with_capacity(200);
    commitment_input.extend_from_slice(RESOURCE_EPOCH_DOMAIN);
    commitment_input.push(CURRENT_VERSION as u8);
    commitment_input.extend_from_slice(resource_id.as_bytes());
    commitment_input.extend_from_slice(epoch_id.as_bytes());
    commitment_input.extend_from_slice(&epoch.to_be_bytes());
    commitment_input.extend_from_slice(&previous_epoch_hash);
    commitment_input.extend_from_slice(&context_hash);
    commitment_input.extend_from_slice(separated.resource.as_bytes());
    commitment_input.extend_from_slice(separated.header.as_bytes());
    let epoch_commitment = hash_bytes(&commitment_input);
    commitment_input.zeroize();
    Ok(ResourceEpochMaterial {
        version: CURRENT_VERSION,
        resource_id,
        epoch,
        epoch_id,
        previous_epoch_hash,
        context_hash,
        epoch_commitment,
        resource_key: *separated.resource.as_bytes(),
        header_key: *separated.header.as_bytes(),
    })
}

/// Byte-slice convenience wrapper for foreign-function boundaries.
pub fn rotate_resource_epoch_from_bytes(
    resource_id: &[u8],
    current_epoch: u64,
    previous_epoch_hash: &[u8],
    context: &[u8],
) -> Result<ResourceEpochMaterial, ProtocolError> {
    rotate_resource_epoch(
        Uuid::from_bytes(exact_array::<16>("resource id", resource_id)?),
        current_epoch,
        exact_array::<HASH_BYTES>("previous epoch hash", previous_epoch_hash)?,
        context,
    )
}

/// A participant's ceremony-bound approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryApproval {
    /// Participant listed in the policy.
    pub participant_id: Uuid,
    /// One-shot ceremony identifier.
    pub ceremony_id: Uuid,
    /// Random challenge copied from the ceremony.
    pub challenge: [u8; HASH_BYTES],
    /// Hash of the external recovery request/context.
    pub context_hash: [u8; HASH_BYTES],
    /// Detached participant signature over the canonical approval message.
    pub signature: DetachedSignature,
}

impl RecoveryApproval {
    /// Canonical bytes a participant signs.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        if self.participant_id.is_nil() || self.ceremony_id.is_nil() {
            return Err(ProtocolError::InvalidRecoveryApproval);
        }
        self.signature.validate()?;
        let mut out = Vec::with_capacity(128);
        out.extend_from_slice(b"sprout-recovery-approval-v1");
        out.extend_from_slice(self.participant_id.as_bytes());
        out.extend_from_slice(self.ceremony_id.as_bytes());
        out.extend_from_slice(&self.challenge);
        out.extend_from_slice(&self.context_hash);
        Ok(out)
    }
}

/// Stateful one-shot n-of-n recovery ceremony.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCeremony {
    /// Unique one-shot identifier.
    pub ceremony_id: Uuid,
    /// Resource being recovered.
    pub resource_id: Uuid,
    /// Random anti-replay challenge.
    pub challenge: [u8; HASH_BYTES],
    /// Hash of the complete external recovery request.
    pub context_hash: [u8; HASH_BYTES],
    /// Inclusive start time in Unix seconds.
    pub created_at: u64,
    /// Exclusive expiry time in Unix seconds.
    pub expires_at: u64,
    /// Fixed n-of-n participant policy.
    pub policy: RecoveryPolicy,
    /// Unique accepted approvals.
    pub approvals: Vec<RecoveryApproval>,
    /// Whether a grant was already issued.
    pub consumed: bool,
}

impl RecoveryCeremony {
    /// Start a fresh ceremony with a random challenge.
    pub fn new(
        ceremony_id: Uuid,
        resource_id: Uuid,
        context_hash: [u8; HASH_BYTES],
        created_at: u64,
        expires_at: u64,
        policy: RecoveryPolicy,
    ) -> Result<Self, ProtocolError> {
        if ceremony_id.is_nil() || resource_id.is_nil() || expires_at <= created_at {
            return Err(ProtocolError::InvalidFormat("recovery ceremony"));
        }
        policy.validate()?;
        let mut challenge = [0u8; HASH_BYTES];
        fill_random(&mut challenge)?;
        Ok(Self {
            ceremony_id,
            resource_id,
            challenge,
            context_hash,
            created_at,
            expires_at,
            policy,
            approvals: Vec::new(),
            consumed: false,
        })
    }

    /// Verify and record one unique, ceremony-bound approval.
    ///
    /// Participant identifiers are V1 recovery signing-key identifiers. The
    /// signature is checked before it can count toward the n-of-n threshold.
    pub fn record_approval<A: SignatureAdapter>(
        &mut self,
        approval: RecoveryApproval,
        adapter: &A,
        public_key: &[u8],
        now: u64,
    ) -> Result<(), ProtocolError> {
        if self.consumed {
            return Err(ProtocolError::RecoveryConsumed);
        }
        if now < self.created_at || now >= self.expires_at {
            return Err(ProtocolError::RecoveryExpired);
        }
        let signing_bytes = approval.signing_bytes()?;
        if approval.ceremony_id != self.ceremony_id
            || approval.challenge != self.challenge
            || approval.context_hash != self.context_hash
            || approval.signature.key_id != approval.participant_id
            || approval.signature.algorithm != adapter.algorithm()
            || !self
                .policy
                .participant_ids
                .contains(&approval.participant_id)
            || self
                .approvals
                .iter()
                .any(|existing| existing.participant_id == approval.participant_id)
        {
            return Err(ProtocolError::InvalidRecoveryApproval);
        }
        adapter.verify(
            public_key,
            &signing_bytes,
            RECOVERY_SIGNATURE_CONTEXT,
            &approval.signature.signature,
        )?;
        self.approvals.push(approval);
        Ok(())
    }

    /// Consume the ceremony only after every configured participant approved.
    pub fn finalize(&mut self, now: u64) -> Result<RecoveryGrant, ProtocolError> {
        if self.consumed {
            return Err(ProtocolError::RecoveryConsumed);
        }
        if now < self.created_at || now >= self.expires_at {
            return Err(ProtocolError::RecoveryExpired);
        }
        if self.approvals.len() != self.policy.threshold() {
            return Err(ProtocolError::RecoveryIncomplete);
        }
        let mut approval_hashes = Vec::with_capacity(self.approvals.len() * HASH_BYTES);
        for participant in &self.policy.participant_ids {
            let approval = self
                .approvals
                .iter()
                .find(|approval| approval.participant_id == *participant)
                .ok_or(ProtocolError::RecoveryIncomplete)?;
            approval_hashes.extend_from_slice(&hash_bytes(&approval.signing_bytes()?));
            approval_hashes.extend_from_slice(&hash_bytes(&approval.signature.signature));
        }
        self.consumed = true;
        Ok(RecoveryGrant {
            ceremony_id: self.ceremony_id,
            resource_id: self.resource_id,
            context_hash: self.context_hash,
            approvals_hash: hash_bytes(&approval_hashes),
        })
    }
}

/// Proof that an n-of-n recovery ceremony completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryGrant {
    /// Consumed ceremony identifier.
    pub ceremony_id: Uuid,
    /// Recovered resource.
    pub resource_id: Uuid,
    /// Recovery request context.
    pub context_hash: [u8; HASH_BYTES],
    /// Ordered commitment to every accepted approval.
    pub approvals_hash: [u8; HASH_BYTES],
}

trait Validate {
    fn validate_model(&self) -> Result<(), ProtocolError>;
}

fn strict_json<T: DeserializeOwned + Serialize + Validate>(
    bytes: &[u8],
    maximum: usize,
    field: &'static str,
) -> Result<T, ProtocolError> {
    ensure_limit(field, bytes.len(), maximum)?;
    let value: T =
        serde_json::from_slice(bytes).map_err(|error| ProtocolError::Json(error.to_string()))?;
    value.validate_model()?;
    let canonical =
        serde_json::to_vec(&value).map_err(|error| ProtocolError::Json(error.to_string()))?;
    if canonical.as_slice() != bytes {
        return Err(ProtocolError::InvalidFormat("non-canonical JSON"));
    }
    Ok(value)
}

fn fill_random(output: &mut [u8]) -> Result<(), ProtocolError> {
    SysRng
        .try_fill_bytes(output)
        .map_err(|_| ProtocolError::RandomnessUnavailable)
}

fn derive_separated_key(
    root: &[u8; KEY_BYTES],
    resource_id: Uuid,
    label: &[u8],
) -> [u8; KEY_BYTES] {
    let mut digest = Sha256::new();
    digest.update(KDF_DOMAIN);
    digest.update((label.len() as u16).to_be_bytes());
    digest.update(label);
    digest.update(resource_id.as_bytes());
    digest.update(root);
    digest.finalize().into()
}

fn contextual_ed25519_message(context: &[u8], message: &[u8]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(ED25519_CONTEXT_DOMAIN.len() + 1 + context.len() + message.len());
    out.extend_from_slice(ED25519_CONTEXT_DOMAIN);
    out.push(context.len() as u8);
    out.extend_from_slice(context);
    out.extend_from_slice(message);
    out
}

fn validate_signature_context(context: &[u8]) -> Result<(), ProtocolError> {
    ensure_limit("signature context", context.len(), u8::MAX as usize)
}

fn validate_generation(
    generation: u64,
    previous_hash: &[u8; HASH_BYTES],
) -> Result<(), ProtocolError> {
    if (generation == 0) != (*previous_hash == [0; HASH_BYTES]) {
        return Err(ProtocolError::InvalidHashChain);
    }
    Ok(())
}

fn ensure_nonempty_bounded(
    field: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), ProtocolError> {
    if actual == 0 {
        return Err(ProtocolError::InvalidFormat("required collection is empty"));
    }
    ensure_limit(field, actual, maximum)
}

fn ensure_limit(field: &'static str, actual: usize, maximum: usize) -> Result<(), ProtocolError> {
    if actual > maximum {
        Err(ProtocolError::SizeLimit { field, maximum })
    } else {
        Ok(())
    }
}

fn ensure_unique_ids(ids: impl IntoIterator<Item = Uuid>) -> Result<(), ProtocolError> {
    let mut seen = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            return Err(ProtocolError::InvalidFormat("duplicate identifier"));
        }
        seen.push(id);
    }
    Ok(())
}

fn exact_array<const N: usize>(
    field: &'static str,
    bytes: &[u8],
) -> Result<[u8; N], ProtocolError> {
    bytes
        .try_into()
        .map_err(|_| ProtocolError::InvalidLength { field, expected: N })
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ProtocolError::InvalidFormat("length overflow"))?;
        if end > self.bytes.len() {
            return Err(ProtocolError::InvalidFormat("truncated input"));
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        exact_array::<N>("binary field", self.take(N)?)
    }

    fn finish(self) -> Result<(), ProtocolError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ProtocolError::InvalidFormat("trailing bytes"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governance_canonical_json_has_stable_golden_vector() {
        let left: Value =
            serde_json::from_str(r#"{"z":1,"a":[3,{"y":true,"x":"line\n"}]}"#).unwrap();
        let right: Value =
            serde_json::from_str(r#"{"a":[3,{"x":"line\n","y":true}],"z":1}"#).unwrap();
        let expected = br#"{"a":[3,{"x":"line\n","y":true}],"z":1}"#;
        let left_bytes = canonical_governance_json(&left).unwrap();
        let right_bytes = canonical_governance_json(&right).unwrap();
        assert_eq!(left_bytes, expected);
        assert_eq!(right_bytes, expected);
        assert_eq!(
            Sha256::digest(expected).as_slice(),
            [
                119, 116, 220, 130, 100, 203, 22, 100, 102, 15, 87, 122, 121, 154, 12, 236, 191,
                115, 167, 176, 177, 64, 128, 221, 120, 168, 184, 147, 242, 230, 162, 241,
            ]
        );
    }

    #[test]
    fn governance_canonical_json_complex_golden_vector_is_byte_exact() {
        let value = serde_json::json!({
            "😀": ["雪", {"k": "v"}],
            "é": "café",
            "z": {
                "β": "unicode",
                "a": [0, -1, i64::MIN, i64::MAX, u64::MAX]
            },
            "A": "quote:\" slash:\\ newline:\n control:\u{001f}"
        });
        let expected = r#"{"A":"quote:\" slash:\\ newline:\n control:\u001f","z":{"a":[0,-1,-9223372036854775808,9223372036854775807,18446744073709551615],"β":"unicode"},"é":"café","😀":["雪",{"k":"v"}]}"#
            .as_bytes();
        let actual = canonical_governance_json(&value).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            Sha256::digest(expected).as_slice(),
            [
                138, 19, 141, 72, 56, 226, 61, 84, 213, 16, 233, 108, 113, 91, 163, 19, 176, 172,
                92, 131, 115, 188, 139, 7, 242, 148, 75, 114, 25, 160, 172, 221,
            ]
        );
    }

    #[test]
    fn governance_canonical_json_ignores_rust_field_declaration_order() {
        #[derive(Serialize)]
        struct ZThenA {
            z: u64,
            a: String,
        }
        #[derive(Serialize)]
        struct AThenZ {
            a: String,
            z: u64,
        }
        let left = canonical_governance_json(&ZThenA {
            z: 1,
            a: "x".to_owned(),
        })
        .unwrap();
        let right = canonical_governance_json(&AThenZ {
            a: "x".to_owned(),
            z: 1,
        })
        .unwrap();
        assert_eq!(left, br#"{"a":"x","z":1}"#);
        assert_eq!(right, br#"{"a":"x","z":1}"#);
    }

    #[test]
    fn governance_canonical_json_rejects_floating_point_values() {
        assert_eq!(
            canonical_governance_json(&serde_json::json!({"value": 1.5})),
            Err(ProtocolError::InvalidFormat(
                "floating-point governance value"
            ))
        );
    }

    fn header(context: &[u8]) -> CanonicalHeader {
        CanonicalHeader::new(
            CipherSuite::Aes256Gcm,
            ContentKind::ResourcePayload,
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            0,
            [0; HASH_BYTES],
            context.to_vec(),
        )
        .unwrap()
    }

    fn recovery_approval(
        ceremony: &RecoveryCeremony,
        participant: Uuid,
        keys: &SignatureKeyPair,
    ) -> RecoveryApproval {
        let adapter = Ed25519Adapter;
        let mut approval = RecoveryApproval {
            participant_id: participant,
            ceremony_id: ceremony.ceremony_id,
            challenge: ceremony.challenge,
            context_hash: ceremony.context_hash,
            signature: DetachedSignature {
                algorithm: KeyAlgorithm::Ed25519,
                key_id: participant,
                signature: vec![0; ED25519_SIGNATURE_BYTES],
            },
        };
        approval.signature.signature = adapter
            .sign(
                keys.secret_key(),
                &approval.signing_bytes().unwrap(),
                RECOVERY_SIGNATURE_CONTEXT,
            )
            .unwrap();
        approval
    }

    #[test]
    fn canonical_header_round_trip_and_strict_trailing_rejection() {
        let header = header(b"tenant/example");
        let bytes = header.canonical_bytes().unwrap();
        assert_eq!(
            CanonicalHeader::from_canonical_bytes(&bytes).unwrap(),
            header
        );

        let mut trailing = bytes;
        trailing.push(0);
        assert!(matches!(
            CanonicalHeader::from_canonical_bytes(&trailing),
            Err(ProtocolError::InvalidFormat(_))
        ));
    }

    #[test]
    fn aes_256_gcm_matches_nist_empty_plaintext_vector() {
        let cipher = Aes256Gcm::new(&Array([0u8; KEY_BYTES]));
        let ciphertext = cipher
            .encrypt(&Array([0u8; NONCE_BYTES]), Payload { msg: b"", aad: b"" })
            .unwrap();
        assert_eq!(
            ciphertext,
            [
                0x53, 0x0f, 0x8a, 0xfb, 0xc7, 0x45, 0x36, 0xb9, 0xa9, 0x63, 0xb4, 0xf1, 0xc4, 0xcb,
                0x73, 0x8b,
            ]
        );
    }

    #[test]
    fn hkdf_sha_256_matches_rfc_5869_case_one() {
        let output = hkdf_sha256(
            &[0x0b; 22],
            &[
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
            ],
            &[0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9],
            42,
        )
        .unwrap();
        assert_eq!(
            output,
            [
                0x3c, 0xb2, 0x5f, 0x25, 0xfa, 0xac, 0xd5, 0x7a, 0x90, 0x43, 0x4f, 0x64, 0xd0, 0x36,
                0x2f, 0x2a, 0x2d, 0x2d, 0x0a, 0x90, 0xcf, 0x1a, 0x5a, 0x4c, 0x5d, 0xb0, 0x2d, 0x56,
                0xec, 0xc4, 0xc5, 0xbf, 0x34, 0x00, 0x72, 0x08, 0xd5, 0xb8, 0x87, 0x18, 0x58, 0x65,
            ]
        );
    }

    #[test]
    fn payload_round_trip_and_tamper_detection() {
        let expected = header(b"record/read");
        let sealed = seal_payload(expected.clone(), b"secret payload").unwrap();
        let encoded = sealed.payload.to_bytes().unwrap();
        let decoded = EncryptedPayload::from_bytes(&encoded).unwrap();
        assert_eq!(
            open_payload(&sealed.dek, &decoded, &expected).unwrap(),
            b"secret payload"
        );

        let mut tampered = decoded.clone();
        tampered.ciphertext[0] ^= 1;
        assert_eq!(
            open_payload(&sealed.dek, &tampered, &expected),
            Err(ProtocolError::AuthenticationFailed)
        );
    }

    #[test]
    fn wrong_key_and_wrong_aad_fail() {
        let expected = header(b"record/read");
        let sealed = seal_payload(expected.clone(), b"secret payload").unwrap();
        let wrong_key = DataEncryptionKey::generate().unwrap();
        assert_eq!(
            open_payload(&wrong_key, &sealed.payload, &expected),
            Err(ProtocolError::AuthenticationFailed)
        );

        let wrong_context = header(b"record/write");
        assert_eq!(
            open_payload(&sealed.dek, &sealed.payload, &wrong_context),
            Err(ProtocolError::ContextMismatch)
        );

        let mut rebound = sealed.payload.clone();
        rebound.header = wrong_context.clone();
        assert_eq!(
            open_payload(&sealed.dek, &rebound, &wrong_context),
            Err(ProtocolError::AuthenticationFailed)
        );
    }

    #[test]
    fn random_deks_and_nonces_are_fresh() {
        let first = seal_payload(header(b"nonce-test"), b"same").unwrap();
        let second = seal_payload(header(b"nonce-test"), b"same").unwrap();
        assert_ne!(first.dek.as_bytes(), second.dek.as_bytes());
        assert_ne!(first.payload.nonce, second.payload.nonce);
        assert_ne!(first.payload.ciphertext, second.payload.ciphertext);
    }

    #[test]
    fn nonce_tampering_fails_authentication() {
        let expected = header(b"nonce-test");
        let sealed = seal_payload(expected.clone(), b"secret").unwrap();
        let mut changed = sealed.payload.clone();
        changed.nonce[0] ^= 1;
        assert_eq!(
            open_payload(&sealed.dek, &changed, &expected),
            Err(ProtocolError::AuthenticationFailed)
        );
    }

    #[test]
    fn replayed_context_is_rejected() {
        let original = header(b"tenant-a/resource");
        let sealed = seal_payload(original.clone(), b"secret").unwrap();
        let replay_target = header(b"tenant-b/resource");
        assert_eq!(
            open_payload(&sealed.dek, &sealed.payload, &replay_target),
            Err(ProtocolError::ContextMismatch)
        );
    }

    #[test]
    fn key_separation_is_deterministic_and_distinct() {
        let root = RootKey::from_slice(&[9; KEY_BYTES]).unwrap();
        let a = SeparatedKeys::derive(&root, Uuid::from_u128(10)).unwrap();
        let b = SeparatedKeys::derive(&root, Uuid::from_u128(10)).unwrap();
        assert_eq!(a.resource.as_bytes(), b.resource.as_bytes());
        assert_eq!(a.header.as_bytes(), b.header.as_bytes());
        assert_ne!(a.resource.as_bytes(), a.header.as_bytes());
    }

    #[test]
    fn container_only_header_key_cannot_open_body_ciphertext() {
        // T-LLR-06.6: a distinct header key must not authenticate body ciphertext.
        let root = RootKey::generate().unwrap();
        let keys = SeparatedKeys::derive(&root, Uuid::from_u128(66)).unwrap();
        let body_header = header(b"tenant/resource/body");
        let sealed = seal_payload(body_header.clone(), b"classified body").unwrap();
        let cipher = Aes256Gcm::new(&Array(*keys.resource.as_bytes()));
        let mut nonce = [0u8; NONCE_BYTES];
        fill_random(&mut nonce).unwrap();
        let wrapped = cipher
            .encrypt(
                &Array(nonce),
                Payload {
                    msg: sealed.dek.as_bytes(),
                    aad: &body_header.canonical_bytes().unwrap(),
                },
            )
            .expect("wrap body DEK under resource key");
        let header_cipher = Aes256Gcm::new(&Array(*keys.header.as_bytes()));
        assert!(
            header_cipher
                .decrypt(
                    &Array(nonce),
                    Payload {
                        msg: &wrapped,
                        aad: &body_header.canonical_bytes().unwrap(),
                    },
                )
                .is_err(),
            "header key must not unwrap a body DEK"
        );
        let opened = cipher
            .decrypt(
                &Array(nonce),
                Payload {
                    msg: &wrapped,
                    aad: &body_header.canonical_bytes().unwrap(),
                },
            )
            .expect("resource key unwraps body DEK");
        assert_eq!(opened.as_slice(), sealed.dek.as_bytes());
    }

    #[test]
    fn experimental_device_generation_produces_four_interoperable_key_pairs() {
        let generated = generate_experimental_device_package(
            Uuid::from_u128(20),
            DeviceKeyIds {
                x25519: Uuid::from_u128(21),
                ml_kem_768: Uuid::from_u128(22),
                ed25519: Uuid::from_u128(23),
                ml_dsa_65: Uuid::from_u128(24),
            },
        )
        .unwrap();
        let package = generated.public_package();
        package.validate().unwrap();
        assert_eq!(
            package.suite,
            DeviceSuiteVersion::ExperimentalIndependentKeysV1
        );
        assert_eq!(
            DevicePublicPackage::from_json(&package.to_canonical_json().unwrap()).unwrap(),
            *package
        );

        let public_key = |algorithm| {
            package
                .encryption_keys
                .iter()
                .chain(&package.signing_keys)
                .find(|key| key.algorithm == algorithm)
                .unwrap()
                .public_key
                .as_slice()
        };
        let private = generated.private_keys();

        let x25519_secret = X25519StaticSecret::from(*private.x25519());
        assert_eq!(
            X25519PublicKey::from(&x25519_secret).as_bytes(),
            public_key(KeyAlgorithm::X25519)
        );

        let ml_kem = LibcruxMlKem768Experimental;
        let encapsulated = ml_kem
            .encapsulate(public_key(KeyAlgorithm::MlKem768Experimental))
            .unwrap();
        assert_eq!(
            ml_kem
                .decapsulate(private.ml_kem_768(), &encapsulated.ciphertext)
                .unwrap(),
            *encapsulated.shared_secret()
        );

        let ed25519 = Ed25519Adapter;
        let ed25519_signature = ed25519
            .sign(private.ed25519(), b"device proof", b"device-v1")
            .unwrap();
        ed25519
            .verify(
                public_key(KeyAlgorithm::Ed25519),
                b"device proof",
                b"device-v1",
                &ed25519_signature,
            )
            .unwrap();

        let ml_dsa = LibcruxMlDsa65Experimental;
        let ml_dsa_signature = ml_dsa
            .sign(private.ml_dsa_65(), b"device proof", b"device-v1")
            .unwrap();
        ml_dsa
            .verify(
                public_key(KeyAlgorithm::MlDsa65Experimental),
                b"device proof",
                b"device-v1",
                &ml_dsa_signature,
            )
            .unwrap();
    }

    #[test]
    fn experimental_hybrid_wrap_round_trip_and_tamper_context_replay_failures() {
        let generated = generate_experimental_device_package(
            Uuid::from_u128(30),
            DeviceKeyIds {
                x25519: Uuid::from_u128(31),
                ml_kem_768: Uuid::from_u128(32),
                ed25519: Uuid::from_u128(33),
                ml_dsa_65: Uuid::from_u128(34),
            },
        )
        .unwrap();
        let package = generated.public_package();
        let public_key = |algorithm| {
            package
                .encryption_keys
                .iter()
                .find(|key| key.algorithm == algorithm)
                .unwrap()
                .public_key
                .as_slice()
        };
        let metadata = HybridWrapMetadata::new(
            Uuid::from_u128(35),
            package.device_id,
            0,
            [0; HASH_BYTES],
            b"tenant-a/resource-key-wrap".to_vec(),
        )
        .unwrap();
        let key = ResourceKey::from_slice(&[0x42; KEY_BYTES]).unwrap();
        let envelope = wrap_resource_key(
            &key,
            public_key(KeyAlgorithm::X25519),
            public_key(KeyAlgorithm::MlKem768Experimental),
            metadata.clone(),
        )
        .unwrap();
        assert_eq!(
            envelope.audit_status,
            SuiteAuditStatus::ProductionAuditRequired
        );
        let encoded = envelope.to_bytes().unwrap();
        let parsed = ExperimentalWrappedResourceKey::from_bytes(&encoded).unwrap();
        let opened = unwrap_resource_key(
            &parsed,
            generated.private_keys().x25519(),
            generated.private_keys().ml_kem_768(),
            &metadata,
        )
        .unwrap();
        assert_eq!(opened.as_bytes(), key.as_bytes());

        let mut tampered = parsed.clone();
        tampered.wrapped_resource_key[0] ^= 1;
        assert!(matches!(
            unwrap_resource_key(
                &tampered,
                generated.private_keys().x25519(),
                generated.private_keys().ml_kem_768(),
                &metadata,
            ),
            Err(ProtocolError::AuthenticationFailed)
        ));

        let wrong_context = HybridWrapMetadata::new(
            metadata.resource_id,
            metadata.recipient_device_id,
            0,
            [0; HASH_BYTES],
            b"tenant-b/resource-key-wrap".to_vec(),
        )
        .unwrap();
        assert!(matches!(
            unwrap_resource_key(
                &parsed,
                generated.private_keys().x25519(),
                generated.private_keys().ml_kem_768(),
                &wrong_context,
            ),
            Err(ProtocolError::ContextMismatch)
        ));

        let replay_epoch = HybridWrapMetadata::new(
            metadata.resource_id,
            metadata.recipient_device_id,
            1,
            hash_bytes(b"epoch-zero"),
            metadata.context.clone(),
        )
        .unwrap();
        assert!(matches!(
            unwrap_resource_key(
                &parsed,
                generated.private_keys().x25519(),
                generated.private_keys().ml_kem_768(),
                &replay_epoch,
            ),
            Err(ProtocolError::ContextMismatch)
        ));

        let mut trailing = encoded;
        trailing.push(0);
        assert!(ExperimentalWrappedResourceKey::from_bytes(&trailing).is_err());
    }

    #[test]
    fn recovery_xor_n_of_n_enforces_all_unique_committed_shares() {
        let secret = [0xa5; RECOVERY_SECRET_BYTES];
        let split = split_recovery_secret_n_of_n(&secret, 3, b"account/recovery-v1").unwrap();
        let second_split =
            split_recovery_secret_n_of_n(&secret, 3, b"account/recovery-v1").unwrap();
        assert_ne!(split.secret_commitment(), second_split.secret_commitment());
        let bundle = split.bundle().unwrap();
        let shares = unpack_recovery_shares(&bundle).unwrap();
        assert_eq!(
            combine_recovery_secret_n_of_n(&shares, b"account/recovery-v1")
                .unwrap()
                .as_bytes(),
            &secret
        );

        assert!(matches!(
            combine_recovery_secret_n_of_n(&shares[..2], b"account/recovery-v1"),
            Err(ProtocolError::RecoveryIncomplete)
        ));

        let mut duplicate = shares.clone();
        duplicate[2] = duplicate[0].clone();
        assert!(matches!(
            combine_recovery_secret_n_of_n(&duplicate, b"account/recovery-v1"),
            Err(ProtocolError::InvalidFormat(_))
        ));

        assert!(matches!(
            combine_recovery_secret_n_of_n(&shares, b"other/context"),
            Err(ProtocolError::ContextMismatch)
        ));

        let mut tampered = shares[0].to_bytes().unwrap();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(
            RecoveryShare::from_bytes(&tampered),
            Err(ProtocolError::AuthenticationFailed)
        );
    }

    #[test]
    fn resource_epoch_rotation_is_fresh_separated_and_chained() {
        let previous = hash_bytes(b"epoch zero commitment");
        let first =
            rotate_resource_epoch(Uuid::from_u128(40), 0, previous, b"tenant/resource").unwrap();
        let second =
            rotate_resource_epoch(Uuid::from_u128(40), 0, previous, b"tenant/resource").unwrap();
        assert_eq!(first.epoch, 1);
        assert_eq!(first.previous_epoch_hash, previous);
        assert_ne!(first.resource_key(), first.header_key());
        assert_ne!(first.resource_key(), second.resource_key());
        assert_ne!(first.header_key(), second.header_key());
        assert_ne!(first.epoch_commitment, second.epoch_commitment);
        assert!(!first.public_metadata_bytes().is_empty());
    }

    #[test]
    fn hash_chain_detects_reordering_and_tamper() {
        let first = HashChainLink::new(0, [0; HASH_BYTES], hash_bytes(b"one")).unwrap();
        let second = HashChainLink::new(1, first.link_hash, hash_bytes(b"two")).unwrap();
        let chain = vec![first.clone(), second.clone()];
        verify_hash_chain(&chain).unwrap();

        let mut tampered = chain;
        tampered[1].content_hash[0] ^= 1;
        assert_eq!(
            verify_hash_chain(&tampered),
            Err(ProtocolError::InvalidHashChain)
        );
    }

    #[test]
    fn recovery_requires_all_participants_and_rejects_replay() {
        let adapter = Ed25519Adapter;
        let participants = vec![
            Uuid::from_u128(101),
            Uuid::from_u128(102),
            Uuid::from_u128(103),
        ];
        let keys: Vec<_> = participants
            .iter()
            .map(|_| adapter.generate_key_pair().unwrap())
            .collect();
        let mut ceremony = RecoveryCeremony::new(
            Uuid::from_u128(201),
            Uuid::from_u128(202),
            hash_bytes(b"recover resource"),
            10,
            20,
            RecoveryPolicy::new(participants.clone()).unwrap(),
        )
        .unwrap();

        for index in 0..2 {
            let approval = recovery_approval(&ceremony, participants[index], &keys[index]);
            ceremony
                .record_approval(approval, &adapter, keys[index].public_key(), 15)
                .unwrap();
        }
        assert_eq!(
            ceremony.finalize(15),
            Err(ProtocolError::RecoveryIncomplete)
        );

        let last = participants[2];
        let approval = recovery_approval(&ceremony, last, &keys[2]);
        ceremony
            .record_approval(approval, &adapter, keys[2].public_key(), 15)
            .unwrap();
        ceremony.finalize(15).unwrap();
        assert_eq!(ceremony.finalize(15), Err(ProtocolError::RecoveryConsumed));
    }

    #[test]
    fn recovery_approval_is_bound_to_ceremony_and_challenge() {
        let adapter = Ed25519Adapter;
        let keys = adapter.generate_key_pair().unwrap();
        let participant = Uuid::from_u128(301);
        let policy = RecoveryPolicy::new(vec![participant]).unwrap();
        let mut target = RecoveryCeremony::new(
            Uuid::from_u128(302),
            Uuid::from_u128(303),
            hash_bytes(b"context"),
            1,
            9,
            policy,
        )
        .unwrap();
        let mut replay = recovery_approval(&target, participant, &keys);
        replay.ceremony_id = Uuid::from_u128(999);
        replay.signature.signature = adapter
            .sign(
                keys.secret_key(),
                &replay.signing_bytes().unwrap(),
                RECOVERY_SIGNATURE_CONTEXT,
            )
            .unwrap();
        assert_eq!(
            target.record_approval(replay, &adapter, keys.public_key(), 5),
            Err(ProtocolError::InvalidRecoveryApproval)
        );
    }

    #[test]
    fn recovery_rejects_forged_participant_signature() {
        let adapter = Ed25519Adapter;
        let participant = Uuid::from_u128(311);
        let legitimate_keys = adapter.generate_key_pair().unwrap();
        let attacker_keys = adapter.generate_key_pair().unwrap();
        let mut ceremony = RecoveryCeremony::new(
            Uuid::from_u128(312),
            Uuid::from_u128(313),
            hash_bytes(b"context"),
            1,
            9,
            RecoveryPolicy::new(vec![participant]).unwrap(),
        )
        .unwrap();
        let forged = recovery_approval(&ceremony, participant, &attacker_keys);
        assert_eq!(
            ceremony.record_approval(forged, &adapter, legitimate_keys.public_key(), 5),
            Err(ProtocolError::SignatureVerification)
        );
    }

    #[test]
    fn ed25519_context_and_message_are_bound() {
        let adapter = Ed25519Adapter;
        let keys = adapter.generate_key_pair().unwrap();
        let signature = adapter
            .sign(keys.secret_key(), b"envelope", b"tenant-a")
            .unwrap();
        adapter
            .verify(keys.public_key(), b"envelope", b"tenant-a", &signature)
            .unwrap();
        assert_eq!(
            adapter.verify(keys.public_key(), b"envelope", b"tenant-b", &signature,),
            Err(ProtocolError::SignatureVerification)
        );
    }

    #[test]
    fn direct_dual_signatures_require_both_algorithms_and_exact_context() {
        let ed25519 = Ed25519Adapter.generate_key_pair().unwrap();
        let ml_dsa = LibcruxMlDsa65Experimental.generate_key_pair().unwrap();
        let signatures = sign_ed25519_ml_dsa65(
            ed25519.secret_key(),
            ml_dsa.secret_key(),
            b"client mutation",
            b"tenant-a/device",
        )
        .unwrap();
        verify_ed25519_ml_dsa65_signatures(
            ed25519.public_key(),
            signatures.ed25519(),
            ml_dsa.public_key(),
            signatures.ml_dsa_65(),
            b"client mutation",
            b"tenant-a/device",
        )
        .unwrap();
        assert_eq!(
            verify_ed25519_ml_dsa65_signatures(
                ed25519.public_key(),
                signatures.ed25519(),
                ml_dsa.public_key(),
                signatures.ml_dsa_65(),
                b"client mutation",
                b"tenant-b/device",
            ),
            Err(ProtocolError::SignatureVerification)
        );

        let mut tampered_ml_dsa = signatures.ml_dsa_65().to_vec();
        tampered_ml_dsa[0] ^= 1;
        assert_eq!(
            verify_ed25519_ml_dsa65_signatures(
                ed25519.public_key(),
                signatures.ed25519(),
                ml_dsa.public_key(),
                &tampered_ml_dsa,
                b"client mutation",
                b"tenant-a/device",
            ),
            Err(ProtocolError::SignatureVerification)
        );
    }

    #[test]
    fn libcrux_ml_kem_adapter_interoperates() {
        let adapter = LibcruxMlKem768Experimental;
        let keys = adapter.generate_key_pair().unwrap();
        let encapsulated = adapter.encapsulate(keys.public_key()).unwrap();
        let decapsulated = adapter
            .decapsulate(keys.secret_key(), &encapsulated.ciphertext)
            .unwrap();
        assert_eq!(encapsulated.shared_secret(), &decapsulated);
    }

    #[test]
    fn libcrux_ml_dsa_adapter_interoperates_and_binds_context() {
        let adapter = LibcruxMlDsa65Experimental;
        let keys = adapter.generate_key_pair().unwrap();
        let signature = adapter
            .sign(keys.secret_key(), b"envelope", b"tenant-a")
            .unwrap();
        adapter
            .verify(keys.public_key(), b"envelope", b"tenant-a", &signature)
            .unwrap();
        assert_eq!(
            adapter.verify(keys.public_key(), b"envelope", b"tenant-b", &signature,),
            Err(ProtocolError::SignatureVerification)
        );
    }

    #[test]
    fn dual_signature_envelope_serializes_and_verifies_both_signatures() {
        let classical = Ed25519Adapter;
        let post_quantum = LibcruxMlDsa65Experimental;
        let classical_keys = classical.generate_key_pair().unwrap();
        let post_quantum_keys = post_quantum.generate_key_pair().unwrap();
        let envelope = KeyEnvelope {
            version: CURRENT_VERSION,
            envelope_id: Uuid::from_u128(501),
            resource_id: Uuid::from_u128(502),
            recipient_device_id: Uuid::from_u128(503),
            resource_key_id: Uuid::from_u128(504),
            header_key_id: Uuid::from_u128(505),
            sequence: 0,
            previous_hash: [0; HASH_BYTES],
            recipient_algorithm: KeyAlgorithm::X25519,
            encapsulation: vec![8; 32],
            wrap_nonce: [9; NONCE_BYTES],
            wrapped_dek: vec![10; KEY_BYTES + GCM_TAG_BYTES],
            payload_header_hash: hash_bytes(b"payload header"),
        };
        let signed = sign_envelope(
            envelope,
            &classical,
            Uuid::from_u128(506),
            classical_keys.secret_key(),
            &post_quantum,
            Uuid::from_u128(507),
            post_quantum_keys.secret_key(),
            b"tenant-a",
        )
        .unwrap();
        let json = signed.to_canonical_json().unwrap();
        let decoded = DualSignatureEnvelope::from_json(&json).unwrap();
        verify_dual_signature_envelope_json(
            &json,
            classical_keys.public_key(),
            post_quantum_keys.public_key(),
            b"tenant-a",
        )
        .unwrap();
        let message = decoded.envelope.signing_bytes().unwrap();
        verify_ed25519_ml_dsa65_signatures(
            classical_keys.public_key(),
            &decoded.classical_signature.signature,
            post_quantum_keys.public_key(),
            &decoded.post_quantum_signature.signature,
            &message,
            b"tenant-a",
        )
        .unwrap();
        assert_eq!(
            verify_dual_signature_envelope(
                &decoded,
                classical_keys.public_key(),
                post_quantum_keys.public_key(),
                b"tenant-b",
            ),
            Err(ProtocolError::SignatureVerification)
        );
    }

    #[test]
    fn production_hybrid_adapter_fails_closed() {
        assert!(matches!(
            ProductionHybridAdapter.generate_key_pair(),
            Err(ProtocolError::ProductionAuditRequired)
        ));
    }

    #[test]
    fn public_package_serialization_is_canonical_and_strict() {
        let signing = PublicKeyDescriptor {
            key_id: Uuid::from_u128(401),
            algorithm: KeyAlgorithm::Ed25519,
            purpose: KeyPurpose::DeviceSigning,
            public_key: vec![3; ED25519_PUBLIC_KEY_BYTES],
        };
        let encryption = PublicKeyDescriptor {
            key_id: Uuid::from_u128(402),
            algorithm: KeyAlgorithm::X25519,
            purpose: KeyPurpose::DeviceEncryption,
            public_key: vec![4; 32],
        };
        let package = PublicPackage {
            version: CURRENT_VERSION,
            package_id: Uuid::from_u128(403),
            account_id: Uuid::from_u128(404),
            generation: 0,
            previous_hash: [0; HASH_BYTES],
            devices: vec![DevicePublicPackage {
                suite: DeviceSuiteVersion::ExperimentalIndependentKeysV1,
                device_id: Uuid::from_u128(405),
                generation: 0,
                previous_hash: [0; HASH_BYTES],
                encryption_keys: vec![encryption],
                signing_keys: vec![signing],
            }],
            recovery: RecoveryPolicy::new(vec![Uuid::from_u128(406)]).unwrap(),
        };
        let json = package.to_canonical_json().unwrap();
        assert_eq!(PublicPackage::from_json(&json).unwrap(), package);

        let mut whitespace = json.clone();
        whitespace.push(b'\n');
        assert!(matches!(
            PublicPackage::from_json(&whitespace),
            Err(ProtocolError::InvalidFormat("non-canonical JSON"))
        ));
    }

    #[test]
    fn strict_payload_parser_rejects_oversized_and_trailing_data() {
        let sealed = seal_payload(header(b"strict"), b"payload").unwrap();
        let mut encoded = sealed.payload.to_bytes().unwrap();
        encoded.push(0);
        assert!(matches!(
            EncryptedPayload::from_bytes(&encoded),
            Err(ProtocolError::InvalidFormat("trailing bytes"))
        ));

        assert!(matches!(
            CanonicalHeader::new(
                CipherSuite::Aes256Gcm,
                ContentKind::ResourcePayload,
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                0,
                [0; HASH_BYTES],
                vec![0; MAX_HEADER_CONTEXT_BYTES + 1],
            ),
            Err(ProtocolError::SizeLimit { .. })
        ));
    }
}
