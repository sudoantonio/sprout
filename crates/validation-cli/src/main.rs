use std::io::{self, Read};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload, array::Array},
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sprout_api_contract::{EncryptedPayloadDto, ResourceEpochInputDto, ResourceKeyEnvelopeDto};
use sprout_crypto_protocol::{
    CanonicalHeader, CipherSuite, ContentKind, DataEncryptionKey, DeviceKeyIds,
    DevicePublicPackage, EncryptedPayload, ExperimentalWrappedResourceKey, HybridWrapMetadata,
    KeyAlgorithm, ResourceKey, generate_experimental_device_package, hash_bytes, open_payload,
    seal_payload, sign_ed25519_ml_dsa65, unwrap_resource_key, wrap_resource_key,
};
use uuid::Uuid;

const ALGORITHM: &str = "sprout_aes_256_gcm_v1";
const MAX_STDIN_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(name = "sprout-validation-crypto", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Encrypt stdin with Sprout's authenticated payload protocol.
    Encrypt {
        #[arg(long)]
        resource_id: Uuid,
        #[arg(long)]
        key_id: Uuid,
        #[arg(long)]
        context: String,
    },
    /// Decrypt an EncryptInput JSON object from stdin.
    Decrypt {
        #[arg(long)]
        resource_id: Uuid,
        #[arg(long)]
        context: String,
    },
    /// Decrypt one development email-outbox row.
    DecryptEmail {
        #[arg(long)]
        identity_id: Uuid,
        #[arg(long)]
        message_kind: String,
        #[arg(long)]
        nonce_hex: String,
        #[arg(long)]
        ciphertext_hex: String,
        #[arg(long, env = "SPROUT_EMAIL_OUTBOX_KEY")]
        key_b64: String,
    },
    /// Generate one validation device package and its private signing keys.
    DeviceCreate {
        #[arg(long)]
        device_id: Uuid,
    },
    /// Build one signed epoch-one resource-key envelope from stdin JSON.
    InitialEpoch {
        #[arg(long)]
        project_id: Uuid,
        #[arg(long)]
        resource_id: Uuid,
        #[arg(long)]
        recipient_identity_id: Uuid,
        #[arg(long)]
        recipient_device_id: Uuid,
        #[arg(long, default_value_t = 1)]
        recipient_device_key_version: u32,
        #[arg(long, default_value_t = 1)]
        sender_device_key_version: u32,
        #[arg(long, default_value_t = 1)]
        epoch: u32,
        #[arg(long)]
        previous_epoch_hash_b64: Option<String>,
    },
    /// Unwrap one validation resource envelope from stdin JSON.
    UnwrapEnvelope,
}

#[derive(Deserialize, Serialize)]
struct EncryptOutput {
    payload: EncryptedPayloadDto,
    dek_b64: String,
}

#[derive(Deserialize, Serialize)]
struct DeviceOutput {
    package_b64: String,
    x25519_private_key_b64: String,
    ml_kem_768_private_key_b64: String,
    ed25519_private_key_b64: String,
    ml_dsa_65_private_key_b64: String,
}

#[derive(Deserialize)]
struct InitialEpochInput {
    resource_key_b64: String,
    recipient_package_b64: String,
    ed25519_private_key_b64: String,
    ml_dsa_65_private_key_b64: String,
}

#[derive(Serialize)]
struct InitialEpochOutput {
    epoch: ResourceEpochInputDto,
    envelopes: Vec<ResourceKeyEnvelopeDto>,
}

#[derive(Deserialize)]
struct UnwrapEnvelopeInput {
    encrypted_key_b64: String,
    x25519_private_key_b64: String,
    ml_kem_768_private_key_b64: String,
}

#[derive(Serialize)]
struct UnwrapEnvelopeOutput {
    resource_key_b64: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Encrypt {
            resource_id,
            key_id,
            context,
        } => {
            let plaintext = read_stdin()?;
            let output = encrypt_document(resource_id, key_id, context.as_bytes(), &plaintext)?;
            serde_json::to_writer(io::stdout().lock(), &output)
                .context("failed to write encrypted JSON")?;
        }
        Command::Decrypt {
            resource_id,
            context,
        } => {
            let input: EncryptOutput =
                serde_json::from_slice(&read_stdin()?).context("invalid encrypted input JSON")?;
            let plaintext = decrypt_document(
                resource_id,
                context.as_bytes(),
                &input.payload,
                &input.dek_b64,
            )?;
            use std::io::Write;
            io::stdout()
                .lock()
                .write_all(&plaintext)
                .context("failed to write plaintext")?;
        }
        Command::DecryptEmail {
            identity_id,
            message_kind,
            nonce_hex,
            ciphertext_hex,
            key_b64,
        } => {
            let plaintext = decrypt_email(
                identity_id,
                &message_kind,
                &nonce_hex,
                &ciphertext_hex,
                &key_b64,
            )?;
            use std::io::Write;
            io::stdout()
                .lock()
                .write_all(&plaintext)
                .context("failed to write email plaintext")?;
        }
        Command::DeviceCreate { device_id } => {
            serde_json::to_writer(io::stdout().lock(), &create_device(device_id)?)
                .context("failed to write device JSON")?;
        }
        Command::InitialEpoch {
            project_id,
            resource_id,
            recipient_identity_id,
            recipient_device_id,
            recipient_device_key_version,
            sender_device_key_version,
            epoch,
            previous_epoch_hash_b64,
        } => {
            let input: InitialEpochInput =
                serde_json::from_slice(&read_stdin()?).context("invalid epoch input JSON")?;
            serde_json::to_writer(
                io::stdout().lock(),
                &initial_epoch(
                    project_id,
                    resource_id,
                    recipient_identity_id,
                    recipient_device_id,
                    recipient_device_key_version,
                    sender_device_key_version,
                    epoch,
                    previous_epoch_hash_b64.as_deref(),
                    input,
                )?,
            )
            .context("failed to write epoch JSON")?;
        }
        Command::UnwrapEnvelope => {
            let input: UnwrapEnvelopeInput =
                serde_json::from_slice(&read_stdin()?).context("invalid envelope input JSON")?;
            let envelope = ExperimentalWrappedResourceKey::from_bytes(
                &STANDARD
                    .decode(input.encrypted_key_b64)
                    .context("invalid encrypted key base64")?,
            )?;
            let resource_key = unwrap_resource_key(
                &envelope,
                &STANDARD
                    .decode(input.x25519_private_key_b64)
                    .context("invalid X25519 private key base64")?,
                &STANDARD
                    .decode(input.ml_kem_768_private_key_b64)
                    .context("invalid ML-KEM-768 private key base64")?,
                &envelope.metadata,
            )?;
            serde_json::to_writer(
                io::stdout().lock(),
                &UnwrapEnvelopeOutput {
                    resource_key_b64: STANDARD.encode(resource_key.as_bytes()),
                },
            )
            .context("failed to write unwrapped key JSON")?;
        }
    }
    Ok(())
}

fn create_device(device_id: Uuid) -> Result<DeviceOutput> {
    let generated = generate_experimental_device_package(
        device_id,
        DeviceKeyIds {
            x25519: Uuid::new_v4(),
            ml_kem_768: Uuid::new_v4(),
            ed25519: Uuid::new_v4(),
            ml_dsa_65: Uuid::new_v4(),
        },
    )?;
    Ok(DeviceOutput {
        package_b64: STANDARD.encode(generated.public_package().to_canonical_json()?),
        x25519_private_key_b64: STANDARD.encode(generated.private_keys().x25519()),
        ml_kem_768_private_key_b64: STANDARD.encode(generated.private_keys().ml_kem_768()),
        ed25519_private_key_b64: STANDARD.encode(generated.private_keys().ed25519()),
        ml_dsa_65_private_key_b64: STANDARD.encode(generated.private_keys().ml_dsa_65()),
    })
}

#[allow(clippy::too_many_arguments)]
fn initial_epoch(
    project_id: Uuid,
    resource_id: Uuid,
    recipient_identity_id: Uuid,
    recipient_device_id: Uuid,
    recipient_device_key_version: u32,
    sender_device_key_version: u32,
    epoch: u32,
    previous_epoch_hash_b64: Option<&str>,
    input: InitialEpochInput,
) -> Result<InitialEpochOutput> {
    let resource_key_bytes = STANDARD
        .decode(input.resource_key_b64)
        .context("invalid resource key base64")?;
    let resource_key = ResourceKey::from_slice(&resource_key_bytes)?;
    let package_bytes = STANDARD
        .decode(input.recipient_package_b64)
        .context("invalid recipient package base64")?;
    let package = DevicePublicPackage::from_json(&package_bytes)?;
    if package.device_id != recipient_device_id {
        bail!("recipient package device does not match");
    }
    let x25519 = package
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::X25519)
        .context("recipient package has no X25519 key")?;
    let ml_kem = package
        .encryption_keys
        .iter()
        .find(|key| key.algorithm == KeyAlgorithm::MlKem768Experimental)
        .context("recipient package has no ML-KEM-768 key")?;
    let previous_epoch_hash = match (epoch, previous_epoch_hash_b64) {
        (1, None) => hash_bytes(
            format!("sprout-resource-key-genesis-v1/{project_id}/{resource_id}").as_bytes(),
        ),
        (_, Some(encoded)) => STANDARD
            .decode(encoded)
            .context("invalid previous epoch hash base64")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("previous epoch hash must contain 32 bytes"))?,
        _ => bail!("rotated epochs require the previous epoch hash"),
    };
    let context = format!(
        "sprout/resource-envelope/v2/{project_id}/{resource_id}/{recipient_identity_id}/{recipient_device_id}"
    );
    let wrapped = wrap_resource_key(
        &resource_key,
        &x25519.public_key,
        &ml_kem.public_key,
        HybridWrapMetadata::new(
            resource_id,
            recipient_device_id,
            u64::from(epoch),
            previous_epoch_hash,
            context.into_bytes(),
        )?,
    )?
    .to_bytes()?;
    let mut signing_message = Vec::with_capacity(160);
    signing_message.extend_from_slice(b"sprout-resource-key-envelope-v2");
    signing_message.extend_from_slice(project_id.as_bytes());
    signing_message.extend_from_slice(&1_i16.to_be_bytes());
    signing_message.extend_from_slice(resource_id.as_bytes());
    signing_message.extend_from_slice(
        &i32::try_from(epoch)
            .context("invalid resource epoch")?
            .to_be_bytes(),
    );
    signing_message.extend_from_slice(recipient_identity_id.as_bytes());
    signing_message.extend_from_slice(recipient_device_id.as_bytes());
    signing_message.extend_from_slice(
        &i32::try_from(recipient_device_key_version)
            .context("invalid recipient device key version")?
            .to_be_bytes(),
    );
    signing_message.extend_from_slice(
        &i32::try_from(sender_device_key_version)
            .context("invalid sender device key version")?
            .to_be_bytes(),
    );
    signing_message.extend_from_slice(&hash_bytes(&wrapped));
    let signatures = sign_ed25519_ml_dsa65(
        &STANDARD
            .decode(input.ed25519_private_key_b64)
            .context("invalid Ed25519 private key base64")?,
        &STANDARD
            .decode(input.ml_dsa_65_private_key_b64)
            .context("invalid ML-DSA-65 private key base64")?,
        &signing_message,
        b"sprout-resource-key-envelope-v2",
    )?;
    let mut commitment_input = Vec::new();
    commitment_input.extend_from_slice(b"sprout-resource-key-commitment-v1");
    commitment_input.extend_from_slice(project_id.as_bytes());
    commitment_input.extend_from_slice(resource_id.as_bytes());
    commitment_input.extend_from_slice(&resource_key_bytes);
    Ok(InitialEpochOutput {
        epoch: ResourceEpochInputDto {
            id: Uuid::new_v4(),
            epoch,
            creator_device_key_version: sender_device_key_version,
            key_commitment_b64: STANDARD.encode(hash_bytes(&commitment_input)),
            header_key_commitment_b64: None,
        },
        envelopes: vec![ResourceKeyEnvelopeDto {
            version: 1,
            resource_id,
            epoch,
            key_purpose: sprout_api_contract::ResourceKeyPurposeDto::Body,
            recipient_identity_id,
            recipient_device_id,
            recipient_device_key_version,
            sender_device_key_version,
            encrypted_key_b64: STANDARD.encode(wrapped),
            sender_signature_b64: STANDARD.encode(signatures.ed25519()),
            sender_post_quantum_signature_b64: STANDARD.encode(signatures.ml_dsa_65()),
        }],
    })
}

fn read_stdin() -> Result<Vec<u8>> {
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_STDIN_BYTES + 1)
        .read_to_end(&mut input)
        .context("failed to read stdin")?;
    if input.len() as u64 > MAX_STDIN_BYTES {
        bail!("stdin exceeds the validation client limit");
    }
    Ok(input)
}

fn encrypt_document(
    resource_id: Uuid,
    key_id: Uuid,
    context: &[u8],
    plaintext: &[u8],
) -> Result<EncryptOutput> {
    let header = CanonicalHeader::new(
        CipherSuite::Aes256Gcm,
        ContentKind::ResourcePayload,
        resource_id,
        key_id,
        0,
        [0; 32],
        context.to_vec(),
    )?;
    let sealed = seal_payload(header, plaintext)?;
    Ok(EncryptOutput {
        payload: EncryptedPayloadDto {
            version: 1,
            algorithm: ALGORITHM.to_owned(),
            key_id: key_id.to_string(),
            nonce_b64: STANDARD.encode(sealed.payload.nonce),
            ciphertext_b64: STANDARD.encode(&sealed.payload.ciphertext),
        },
        dek_b64: STANDARD.encode(sealed.dek.as_bytes()),
    })
}

fn decrypt_document(
    resource_id: Uuid,
    context: &[u8],
    dto: &EncryptedPayloadDto,
    dek_b64: &str,
) -> Result<Vec<u8>> {
    if dto.version != 1 || dto.algorithm != ALGORITHM {
        bail!("unsupported validation payload suite");
    }
    let key_id = Uuid::parse_str(&dto.key_id).context("invalid key identifier")?;
    let key = STANDARD.decode(dek_b64).context("invalid DEK base64")?;
    let dek = DataEncryptionKey::from_slice(&key)?;
    let nonce: [u8; 12] = STANDARD
        .decode(&dto.nonce_b64)
        .context("invalid nonce base64")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("nonce must contain 12 bytes"))?;
    let payload = EncryptedPayload {
        header: CanonicalHeader::new(
            CipherSuite::Aes256Gcm,
            ContentKind::ResourcePayload,
            resource_id,
            key_id,
            0,
            [0; 32],
            context.to_vec(),
        )?,
        nonce,
        ciphertext: STANDARD
            .decode(&dto.ciphertext_b64)
            .context("invalid ciphertext base64")?,
    };
    open_payload(&dek, &payload, &payload.header).map_err(Into::into)
}

fn decrypt_email(
    identity_id: Uuid,
    message_kind: &str,
    nonce_hex: &str,
    ciphertext_hex: &str,
    key_b64: &str,
) -> Result<Vec<u8>> {
    let key: [u8; 32] = STANDARD
        .decode(key_b64)
        .context("invalid email key base64")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("email key must contain 32 bytes"))?;
    let nonce: [u8; 12] = hex::decode(nonce_hex)
        .context("invalid email nonce hex")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("email nonce must contain 12 bytes"))?;
    let ciphertext = hex::decode(ciphertext_hex).context("invalid email ciphertext hex")?;
    let mut aad = Vec::with_capacity(message_kind.len() + 16);
    aad.extend_from_slice(message_kind.as_bytes());
    aad.extend_from_slice(identity_id.as_bytes());
    Aes256Gcm::new(&Array(key))
        .decrypt(
            &Array(nonce),
            Payload {
                msg: &ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("email payload authentication failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_llr_12_2_validation_payload_round_trip_and_context_rejection() {
        let resource_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();
        let encrypted = encrypt_document(
            resource_id,
            key_id,
            b"validation/task",
            b"classified-canary",
        )
        .unwrap();
        assert!(
            !serde_json::to_vec(&encrypted)
                .unwrap()
                .windows(b"classified-canary".len())
                .any(|window| window == b"classified-canary")
        );
        assert_eq!(
            decrypt_document(
                resource_id,
                b"validation/task",
                &encrypted.payload,
                &encrypted.dek_b64,
            )
            .unwrap(),
            b"classified-canary"
        );
        assert!(
            decrypt_document(
                resource_id,
                b"validation/other",
                &encrypted.payload,
                &encrypted.dek_b64,
            )
            .is_err()
        );
    }

    #[test]
    fn development_email_helper_uses_the_server_aad_shape() {
        let identity_id = Uuid::new_v4();
        let message_kind = "signup_verification";
        let key = [7_u8; 32];
        let nonce = [9_u8; 12];
        let mut aad = Vec::new();
        aad.extend_from_slice(message_kind.as_bytes());
        aad.extend_from_slice(identity_id.as_bytes());
        let ciphertext = Aes256Gcm::new(&Array(key))
            .encrypt(
                &Array(nonce),
                Payload {
                    msg: b"{\"token\":\"test\"}",
                    aad: &aad,
                },
            )
            .unwrap();
        assert_eq!(
            decrypt_email(
                identity_id,
                message_kind,
                &hex::encode(nonce),
                &hex::encode(ciphertext),
                &STANDARD.encode(key),
            )
            .unwrap(),
            b"{\"token\":\"test\"}"
        );
    }
}
