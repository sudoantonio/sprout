# Internal cryptographic wire formats V1

## Status and compatibility

These encodings are frozen for Sprout's internal native/WASM interoperability at protocol version `1`. Existing V1 bytes must not be reinterpreted. Any incompatible change requires a new magic value or protocol/suite version and a new vector corpus.

This freeze is **not production cryptographic approval**. The X25519 + ML-KEM-768 resource-key construction remains non-standard, has suite ID `0x8001`, always carries `production_audit_required`, and does not make `ProductionHybridAdapter` available. The production fail-closed and independent-audit requirements in [crypto-protocol.md](crypto-protocol.md) remain unchanged.

All integers are unsigned, big-endian. Byte strings are used verbatim; no text normalization is performed. Parsers reject unknown versions/algorithms/status values, invalid lengths, non-canonical JSON, truncation, and trailing bytes.

## Canonical payload AAD

Magic: ASCII `SPRAAD01`.

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | magic |
| 8 | 1 | protocol version (`1`) |
| 9 | 1 | cipher suite (`1` = AES-256-GCM) |
| 10 | 1 | content kind |
| 11 | 16 | resource UUID bytes |
| 27 | 16 | key UUID bytes |
| 43 | 8 | sequence |
| 51 | 32 | previous object hash |
| 83 | 2 | context length |
| 85 | variable | context bytes |

Sequence zero requires an all-zero previous hash; nonzero sequences require a nonzero previous hash. The entire canonical header is AES-GCM AAD.

## Encrypted payload

Magic: ASCII `SPRENC01`.

`magic[8] || header_length[u16] || canonical_header[header_length] || nonce[12] || ciphertext_length[u32] || ciphertext_and_tag[ciphertext_length]`

The ciphertext field is AES-256-GCM ciphertext followed by its 16-byte tag. The production API always creates a fresh 32-byte DEK and 12-byte nonce; caller-selected nonces exist only in checked-in known-answer material.

## Hybrid resource metadata

Magic: ASCII `SPRHMD01`.

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | magic |
| 8 | 1 | protocol version (`1`) |
| 9 | 16 | resource UUID bytes |
| 25 | 16 | recipient device UUID bytes |
| 41 | 8 | resource epoch |
| 49 | 32 | previous epoch hash |
| 81 | 2 | context length |
| 83 | variable | context bytes |

Epoch zero requires an all-zero previous hash; later epochs require a nonzero hash.

## Experimental resource-key envelope

Magic: ASCII `SPRHYB01`.

`magic[8] || protocol_version[u8] || suite_version[u16] || audit_status[u8] || metadata_length[u16] || metadata[metadata_length] || ephemeral_x25519_public_key[32] || ml_kem_768_ciphertext[1088] || nonce[12] || wrapped_resource_key_and_tag[48]`

The only V1 suite value is experimental `0x8001`; the only accepted audit status is `1`, meaning `production_audit_required`. X25519 and ML-KEM-768 shared secrets are combined by the explicitly experimental implementation documented in the Rust source. This is not X-Wing or a production-approved hybrid KEM.

The envelope AAD is:

`"sprout-hybrid-wrap-aad-v1" || protocol_version[u8] || suite_version[u16] || audit_status[u8] || metadata_length[u16] || metadata || ephemeral_x25519_public_key || ml_kem_768_ciphertext`

## Recovery share and bundle

Share magic: ASCII `SPRSHR01`. Every V1 share is exactly 171 bytes:

`magic[8] || protocol_version[u8] || total[u8] || one_based_index[u8] || context_hash[32] || secret_commitment[32] || share_commitment[32] || xor_share[64]`

Bundle magic: ASCII `SPRSHB01`:

`magic[8] || protocol_version[u8] || share_count[u8] || repeated(share_length[u16] || share[171])`

V1 supports only `n-of-n`, with `2 <= n <= 16`. Every unique committed share and the exact original context are required.

## Signatures and canonical JSON

Ed25519 signs:

`"sprout-ed25519-context-v1" || context_length[u16] || context || message`

ML-DSA-65 uses the same caller-supplied context and message through the pinned libcrux API. A dual signature is valid only when both signatures verify under the exact same message and context.

`DevicePublicPackage`, `PublicPackage`, and `DualSignatureEnvelope` use compact UTF-8 JSON in Rust struct-field order, with no maps or insignificant whitespace. Parsing reserializes and requires byte-for-byte equality; unknown and duplicate/non-canonical representations are rejected.

## Shared vector corpus

[`tests/vectors/crypto-v1.json`](../tests/vectors/crypto-v1.json) is corpus version `1`. It is consumed directly by native Rust and generated WASM/frontend tests and contains:

- AES-256-GCM payload/AAD bytes;
- X25519 and ML-KEM-768 key-agreement/KEM values;
- Ed25519, ML-DSA-65, and dual-signature values;
- a complete experimental resource-key envelope;
- a complete three-of-three recovery bundle;
- wrong-context values used with tamper tests.

The private keys are public test fixtures, never production material. Changing any corpus byte is a protocol compatibility change and requires explicit review.
