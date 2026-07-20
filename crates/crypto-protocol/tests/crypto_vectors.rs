use serde_json::Value;
use sprout_crypto_protocol::{
    CanonicalHeader, DataEncryptionKey, Ed25519Adapter, EncryptedPayload,
    ExperimentalWrappedResourceKey, HybridWrapMetadata, KemAdapter, LibcruxMlDsa65Experimental,
    LibcruxMlKem768Experimental, ProtocolError, SignatureAdapter, SuiteAuditStatus,
    combine_recovery_secret_n_of_n, open_payload, pack_recovery_shares, unpack_recovery_shares,
    unwrap_resource_key, verify_ed25519_ml_dsa65_signatures,
};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

const CORPUS_JSON: &str = include_str!("../../../tests/vectors/crypto-v1.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS_JSON).expect("valid checked-in vector corpus")
}

fn bytes(value: &Value, path: &[&str]) -> Vec<u8> {
    let hex = path
        .iter()
        .fold(value, |value, key| match value {
            Value::Array(values) => &values[key.parse::<usize>().expect("numeric array index")],
            _ => &value[*key],
        })
        .as_str()
        .unwrap_or_else(|| panic!("missing vector field {}", path.join(".")));
    assert_eq!(hex.len() % 2, 0, "hex field {}", path.join("."));
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("valid vector hex"))
        .collect()
}

#[test]
fn corpus_version_and_audit_gate_are_frozen() {
    let vectors = corpus();
    assert_eq!(vectors["corpus_version"], 1);
    assert_eq!(vectors["protocol_version"], 1);
    assert_eq!(vectors["audit_status"], "production_audit_required");
    assert_eq!(
        vectors["resource_envelope"]["audit_status"],
        "production_audit_required"
    );
    assert_eq!(vectors["resource_envelope"]["suite_version"], 0x8001);
}

#[test]
fn aes_gcm_aad_vector_is_byte_exact_and_rejects_tamper_and_wrong_context() {
    let vectors = corpus();
    let encoded = bytes(&vectors, &["aes_gcm_aad", "encrypted_payload"]);
    let payload = EncryptedPayload::from_bytes(&encoded).unwrap();
    assert_eq!(payload.to_bytes().unwrap(), encoded);
    assert_eq!(
        payload.header.canonical_bytes().unwrap(),
        bytes(&vectors, &["aes_gcm_aad", "canonical_header"])
    );
    let dek = DataEncryptionKey::from_slice(&bytes(&vectors, &["aes_gcm_aad", "dek"])).unwrap();
    assert_eq!(
        open_payload(&dek, &payload, &payload.header).unwrap(),
        bytes(&vectors, &["aes_gcm_aad", "plaintext"])
    );

    let mut tampered = payload.clone();
    tampered.ciphertext[0] ^= 1;
    assert_eq!(
        open_payload(&dek, &tampered, &payload.header),
        Err(ProtocolError::AuthenticationFailed)
    );
    let mut wrong_header = payload.header.clone();
    wrong_header.context = bytes(&vectors, &["aes_gcm_aad", "wrong_context"]);
    assert_eq!(
        open_payload(&dek, &payload, &wrong_header),
        Err(ProtocolError::ContextMismatch)
    );
}

#[test]
fn x25519_and_ml_kem_768_vectors_match_pinned_primitive_bytes() {
    let vectors = corpus();
    let private: [u8; 32] = bytes(&vectors, &["x25519", "private_key"])
        .try_into()
        .unwrap();
    let peer_public: [u8; 32] = bytes(&vectors, &["x25519", "peer_public_key"])
        .try_into()
        .unwrap();
    let private = X25519StaticSecret::from(private);
    assert_eq!(
        X25519PublicKey::from(&private).as_bytes(),
        bytes(&vectors, &["x25519", "public_key"]).as_slice()
    );
    assert_eq!(
        private
            .diffie_hellman(&X25519PublicKey::from(peer_public))
            .as_bytes(),
        bytes(&vectors, &["x25519", "shared_secret"]).as_slice()
    );

    let kem = LibcruxMlKem768Experimental;
    let secret_key = bytes(&vectors, &["ml_kem_768", "private_key"]);
    let ciphertext = bytes(&vectors, &["ml_kem_768", "ciphertext"]);
    assert_eq!(
        kem.decapsulate(&secret_key, &ciphertext).unwrap(),
        bytes(&vectors, &["ml_kem_768", "shared_secret"]).as_slice()
    );
    let mut tampered = ciphertext;
    tampered[0] ^= 1;
    assert_ne!(
        kem.decapsulate(&secret_key, &tampered).unwrap(),
        bytes(&vectors, &["ml_kem_768", "shared_secret"]).as_slice()
    );
}

#[test]
fn ed25519_ml_dsa_and_dual_vectors_bind_message_and_context() {
    let vectors = corpus();
    for (name, adapter) in [
        ("ed25519", &Ed25519Adapter as &dyn SignatureAdapter),
        (
            "ml_dsa_65",
            &LibcruxMlDsa65Experimental as &dyn SignatureAdapter,
        ),
    ] {
        let public_key = bytes(&vectors, &[name, "public_key"]);
        let message = bytes(&vectors, &[name, "message"]);
        let context = bytes(&vectors, &[name, "context"]);
        let signature = bytes(&vectors, &[name, "signature"]);
        adapter
            .verify(&public_key, &message, &context, &signature)
            .unwrap();
        let mut tampered = signature;
        tampered[0] ^= 1;
        assert_eq!(
            adapter.verify(&public_key, &message, &context, &tampered),
            Err(ProtocolError::SignatureVerification)
        );
    }

    let public_ed = bytes(&vectors, &["dual_signature", "ed25519_public_key"]);
    let signature_ed = bytes(&vectors, &["dual_signature", "ed25519_signature"]);
    let public_ml = bytes(&vectors, &["dual_signature", "ml_dsa_65_public_key"]);
    let signature_ml = bytes(&vectors, &["dual_signature", "ml_dsa_65_signature"]);
    let message = bytes(&vectors, &["dual_signature", "message"]);
    let context = bytes(&vectors, &["dual_signature", "context"]);
    verify_ed25519_ml_dsa65_signatures(
        &public_ed,
        &signature_ed,
        &public_ml,
        &signature_ml,
        &message,
        &context,
    )
    .unwrap();
    assert_eq!(
        verify_ed25519_ml_dsa65_signatures(
            &public_ed,
            &signature_ed,
            &public_ml,
            &signature_ml,
            &message,
            &bytes(&vectors, &["dual_signature", "wrong_context"]),
        ),
        Err(ProtocolError::SignatureVerification)
    );
    let mut tampered_ed = signature_ed;
    tampered_ed[0] ^= 1;
    assert_eq!(
        verify_ed25519_ml_dsa65_signatures(
            &public_ed,
            &tampered_ed,
            &public_ml,
            &signature_ml,
            &message,
            &context,
        ),
        Err(ProtocolError::SignatureVerification)
    );
    let mut tampered_ml = signature_ml;
    tampered_ml[0] ^= 1;
    assert_eq!(
        verify_ed25519_ml_dsa65_signatures(
            &public_ed,
            &bytes(&vectors, &["dual_signature", "ed25519_signature"]),
            &public_ml,
            &tampered_ml,
            &message,
            &context,
        ),
        Err(ProtocolError::SignatureVerification)
    );
}

#[test]
fn resource_envelope_vector_is_byte_exact_audit_gated_and_context_bound() {
    let vectors = corpus();
    let encoded = bytes(&vectors, &["resource_envelope", "envelope"]);
    let envelope = ExperimentalWrappedResourceKey::from_bytes(&encoded).unwrap();
    assert_eq!(envelope.to_bytes().unwrap(), encoded);
    assert_eq!(
        envelope.audit_status,
        SuiteAuditStatus::ProductionAuditRequired
    );
    let metadata = HybridWrapMetadata::from_canonical_bytes(&bytes(
        &vectors,
        &["resource_envelope", "canonical_metadata"],
    ))
    .unwrap();
    assert_eq!(envelope.metadata, metadata);
    let opened = unwrap_resource_key(
        &envelope,
        &bytes(
            &vectors,
            &["resource_envelope", "recipient_x25519_private_key"],
        ),
        &bytes(
            &vectors,
            &["resource_envelope", "recipient_ml_kem_768_private_key"],
        ),
        &metadata,
    )
    .unwrap();
    assert_eq!(
        opened.as_bytes(),
        bytes(&vectors, &["resource_envelope", "resource_key"]).as_slice()
    );

    let mut wrong_metadata = metadata.clone();
    wrong_metadata.context = bytes(&vectors, &["resource_envelope", "wrong_context"]);
    assert!(matches!(
        unwrap_resource_key(
            &envelope,
            &bytes(
                &vectors,
                &["resource_envelope", "recipient_x25519_private_key"],
            ),
            &bytes(
                &vectors,
                &["resource_envelope", "recipient_ml_kem_768_private_key"],
            ),
            &wrong_metadata,
        ),
        Err(ProtocolError::ContextMismatch)
    ));
    let mut tampered = envelope;
    tampered.wrapped_resource_key[0] ^= 1;
    assert!(matches!(
        unwrap_resource_key(
            &tampered,
            &bytes(
                &vectors,
                &["resource_envelope", "recipient_x25519_private_key"],
            ),
            &bytes(
                &vectors,
                &["resource_envelope", "recipient_ml_kem_768_private_key"],
            ),
            &metadata,
        ),
        Err(ProtocolError::AuthenticationFailed)
    ));
}

#[test]
fn recovery_vector_requires_every_byte_exact_share_and_exact_context() {
    let vectors = corpus();
    let bundle = bytes(&vectors, &["recovery_n_of_n", "bundle"]);
    let shares = unpack_recovery_shares(&bundle).unwrap();
    assert_eq!(pack_recovery_shares(&shares).unwrap(), bundle);
    for (index, share) in shares.iter().enumerate() {
        assert_eq!(
            share.to_bytes().unwrap(),
            bytes(&vectors, &["recovery_n_of_n", "shares", &index.to_string()])
        );
    }
    let context = bytes(&vectors, &["recovery_n_of_n", "context"]);
    assert_eq!(
        combine_recovery_secret_n_of_n(&shares, &context)
            .unwrap()
            .as_bytes(),
        bytes(&vectors, &["recovery_n_of_n", "secret"]).as_slice()
    );
    assert!(matches!(
        combine_recovery_secret_n_of_n(&shares[..shares.len() - 1], &context),
        Err(ProtocolError::RecoveryIncomplete)
    ));
    assert!(matches!(
        combine_recovery_secret_n_of_n(
            &shares,
            &bytes(&vectors, &["recovery_n_of_n", "wrong_context"]),
        ),
        Err(ProtocolError::ContextMismatch)
    ));
    let mut tampered_share = bytes(&vectors, &["recovery_n_of_n", "shares", "0"]);
    let last = tampered_share.len() - 1;
    tampered_share[last] ^= 1;
    assert!(sprout_crypto_protocol::RecoveryShare::from_bytes(&tampered_share).is_err());
}

#[test]
fn canonical_header_vector_parser_round_trips_exact_bytes() {
    let vectors = corpus();
    let encoded = bytes(&vectors, &["aes_gcm_aad", "canonical_header"]);
    assert_eq!(
        CanonicalHeader::from_canonical_bytes(&encoded)
            .unwrap()
            .canonical_bytes()
            .unwrap(),
        encoded
    );
}
