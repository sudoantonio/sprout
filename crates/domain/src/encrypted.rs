use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// An opaque, authenticated ciphertext. Plaintext never crosses this boundary.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct EncryptedPayload {
    version: u16,
    algorithm: String,
    key_id: String,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

impl EncryptedPayload {
    pub const MAX_ALGORITHM_LEN: usize = 64;
    pub const MAX_KEY_ID_LEN: usize = 128;

    pub fn new(
        version: u16,
        algorithm: impl Into<String>,
        key_id: impl Into<String>,
        nonce: Vec<u8>,
        ciphertext: Vec<u8>,
    ) -> Result<Self, EncryptedPayloadError> {
        let payload = Self {
            version,
            algorithm: algorithm.into(),
            key_id: key_id.into(),
            nonce,
            ciphertext,
        };
        payload.validate()?;
        Ok(payload)
    }

    fn validate(&self) -> Result<(), EncryptedPayloadError> {
        if self.version == 0 {
            return Err(EncryptedPayloadError::ZeroVersion);
        }
        validate_label("algorithm", &self.algorithm, Self::MAX_ALGORITHM_LEN)?;
        validate_label("key id", &self.key_id, Self::MAX_KEY_ID_LEN)?;
        if self.nonce.is_empty() {
            return Err(EncryptedPayloadError::EmptyNonce);
        }
        if self.ciphertext.is_empty() {
            return Err(EncryptedPayloadError::EmptyCiphertext);
        }
        Ok(())
    }

    #[must_use]
    pub fn version(&self) -> u16 {
        self.version
    }

    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    #[must_use]
    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }

    #[must_use]
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
}

fn validate_label(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<(), EncryptedPayloadError> {
    if value.trim().is_empty() {
        return Err(EncryptedPayloadError::EmptyLabel(field));
    }
    if value.len() > max {
        return Err(EncryptedPayloadError::LabelTooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(EncryptedPayloadError::InvalidLabel(field));
    }
    Ok(())
}

impl fmt::Debug for EncryptedPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedPayload")
            .field("version", &self.version)
            .field("algorithm", &self.algorithm)
            .field("key_id", &"[REDACTED]")
            .field(
                "nonce",
                &format_args!("[REDACTED; {} bytes]", self.nonce.len()),
            )
            .field(
                "ciphertext",
                &format_args!("[REDACTED; {} bytes]", self.ciphertext.len()),
            )
            .finish()
    }
}

impl<'de> Deserialize<'de> for EncryptedPayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WirePayload {
            version: u16,
            algorithm: String,
            key_id: String,
            nonce: Vec<u8>,
            ciphertext: Vec<u8>,
        }

        let wire = WirePayload::deserialize(deserializer)?;
        Self::new(
            wire.version,
            wire.algorithm,
            wire.key_id,
            wire.nonce,
            wire.ciphertext,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum EncryptedPayloadError {
    #[error("encrypted payload version must be non-zero")]
    ZeroVersion,
    #[error("{0} cannot be empty")]
    EmptyLabel(&'static str),
    #[error("{field} exceeds maximum length {max}")]
    LabelTooLong { field: &'static str, max: usize },
    #[error("{0} contains invalid characters")]
    InvalidLabel(&'static str),
    #[error("encrypted payload nonce cannot be empty")]
    EmptyNonce,
    #[error("encrypted payload ciphertext cannot be empty")]
    EmptyCiphertext,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_exposes_sensitive_bytes_or_key_id() {
        let payload =
            EncryptedPayload::new(1, "xchacha20poly1305", "secret-key", vec![42], vec![99])
                .unwrap();
        let debug = format!("{payload:?}");
        assert!(!debug.contains("secret-key"));
        assert!(!debug.contains("42"));
        assert!(!debug.contains("99"));
        assert!(debug.contains("REDACTED"));
    }
}
