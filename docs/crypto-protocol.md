# Cryptographic protocol

## Status

**Draft; not approved for production.** Sprout's internal V1 byte encodings, domain-separation labels, pinned implementations, and compatibility vectors are frozen in [Internal cryptographic wire formats V1](crypto-wire-formats-v1.md). The experimental hybrid-KEM composition remains non-standard and audit-gated. Stable reviewed standards/libraries and an independent audit are still required before production. No independent audit is claimed.

Implementations must fail closed on unknown protocol versions, suites, algorithms, field values, malformed lengths, duplicate fields, non-canonical encodings, invalid signatures, or authentication failures. Parsing untrusted objects is bounded and side-effect free until validation completes.

## Suite intent

The planned initial suite uses:

- AES-256-GCM for payload/blob authenticated encryption;
- X25519 plus ML-KEM-768 for hybrid device encapsulation;
- Ed25519 plus ML-DSA-65 for dual signatures, with **both** signatures required;
- a vetted cryptographic random number generator for keys and nonces.

These names do not define the hybrid construction. The project must not create an ad-hoc combiner. The production suite must use a standardized or independently reviewed hybrid construction through a versioned adapter, with explicit KDF, domain separation, failure handling, and test vectors. OpenMLS is outside the storage protocol's critical path until post-quantum MLS suites are standardized and mature.

## Key hierarchy

```text
device KEM/signing key pairs
  └─ authenticated per-device envelopes
       └─ per-resource, per-epoch KEK
            ├─ wraps fresh payload/blob DEKs
            └─ never derives sibling or child KEKs

separate container-header KEK
  └─ decrypts only the minimum ancestor header

owner recovery secret
  └─ n-of-n shares held by active non-owner participants
       └─ recovers/rotates owner wrapping capability, not a global content key
```

Every device has independent versioned key pairs. Every resource has an independent KEK for each epoch, and every authorized device receives an authenticated envelope. An owner envelope is mandatory. There is no project-wide content key that implicitly opens every resource.

A payload revision or blob uses a fresh random 256-bit DEK. A resource KEK wraps that DEK under the suite's reviewed key-wrapping/envelope construction. The exact envelope format and recipient-key authentication are production-blocking parts of the frozen specification.

## Versioned objects

Every cryptographic object has a type tag, format version, suite ID, project ID, and object-specific identifiers. At minimum:

| Object | Authenticated fields |
| --- | --- |
| Device key package | user, device, key versions, KEM public keys, signing public keys, creation/expiry, enrollment context |
| Resource-key envelope | project, resource, key epoch, visibility mode, recipient user/device/key version, encapsulation, wrapped KEK |
| Payload/blob | project, resource, payload kind/version, resource epoch, revision/blob ID, actor, intended recipient/scope, nonce, ciphertext |
| Signed event | project, resource, event/version, actor/device, sequence, predecessor hash, base version, idempotency key, ciphertext hash |
| Recovery epoch/share | project, frozen membership epoch, participant, threshold/count, recovery-key version, share commitment/encrypted share |
| Recovery approval | project, recovery epoch, requester/new device, participant device, nonce, expiry, approval digest |
| Archive manifest | user/project scope, source purge receipt, entries/hashes, creation/expiry, archive format/version |

An implementation may not infer omitted security context. Recipient/scope is explicit: for broadcast resource payloads it identifies the authorized resource epoch; for envelopes it identifies the exact device key.

## Canonical AAD

AEAD additional authenticated data must canonically bind:

1. domain-separation label and object type;
2. protocol format and suite;
3. project and resource;
4. payload kind and schema version;
5. resource-key epoch and object revision;
6. actor device;
7. intended recipient device or authorized scope;
8. any immutable routing fields whose substitution would change meaning.

The internal V1 ordering, integer encoding, and byte representation are frozen in [Internal cryptographic wire formats V1](crypto-wire-formats-v1.md) and tested byte-for-byte by native Rust and WASM. Changing any bound field makes authentication fail. Human-readable JSON serialization is not canonical AAD.

## Nonce and retry rule

AES-GCM requires a unique nonce for each key. Sprout enforces a stronger lifecycle rule:

- create a fresh DEK for every new payload revision or blob;
- generate the suite-required nonce with the approved CSPRNG;
- persist the complete immutable ciphertext artifact before a retry can occur;
- an exact retry reuses the same ciphertext artifact and idempotency key;
- a changed plaintext or context creates a fresh DEK and nonce;
- never retry encryption under the same DEK with a newly generated nonce after an ambiguous commit;
- reject detected duplicate object/revision identities and test crash/retry paths.

No nonce counter may reset under a reused key. Implementations must not expose an API that accepts caller-selected production nonces.

## Encryption and event flow

1. Authorize the local command against the client's current state.
2. Serialize the versioned plaintext format deterministically.
3. Allocate a fresh DEK/nonce and construct canonical AAD.
4. Encrypt with AEAD; wrap the DEK for the current resource epoch.
5. Construct a versioned event with previous device-event hash, base version, and idempotency key.
6. Sign the event bytes with Ed25519 and ML-DSA-65. Verification succeeds only when both signatures, the device key package, and chain context are valid.
7. The server validates authentication, authorization-visible metadata, versions, bounds, signatures, chain relation, and idempotency, then stores ciphertext atomically.
8. A recipient verifies signatures/chain/AAD before exposing plaintext.

Signature and AEAD failures return indistinguishable protocol errors to untrusted callers where practical and never include key or plaintext diagnostics.

## `container_only`

Ancestor navigation uses a distinct header key and payload. It contains only the minimum reviewed container label/navigation state. It cannot derive the body key or any resource, child, or sibling key. Queries also suppress sibling existence, names, counts, payloads, and downloads. Cryptography does not hide the ancestor's routing existence from the service.

## Membership, sharing, and revocation

Granting effective access creates device envelopes for the applicable resource epoch; ancestor grants cover the authorized subtree, while descendant grants create only necessary container-header envelopes for ancestors. Envelope delivery must track permission origin and current membership/device state.

Revocation creates a new resource epoch, new resource KEK, and new envelopes for remaining devices before future revisions are accepted. Where scope requires, descendant resources are rekeyed. Old epochs remain readable to anyone who retained their keys or plaintext. Re-encrypting current snapshots limits subsequent server retrieval but cannot erase downloaded copies. Revocation is prospective, never retroactive.

## Owner recovery

At a recovery epoch, the owner recovery secret is divided among every active non-owner participant using a reviewed secret-sharing implementation with threshold `n` of `n`. The electorate and participant key bindings are frozen to the membership epoch.

Each approval is signed and binds project, recovery epoch, requester, proposed owner device/key package, participant device, random nonce, and expiry. Duplicate, expired, prior-epoch, or replayed approvals fail. Recovery proceeds only after every distinct valid share is provided.

Immediately after recovery, clients rotate:

- owner wrapping key and affected owner envelopes;
- recovery secret, recovery epoch, and every participant share;
- approval nonces/receipts;
- any resource epoch whose compromise is suspected.

The unanimity rule is a security/availability trade-off. One unreachable participant, missing share, or an owner-only project makes owner recovery impossible. The UI must warn before creating or entering such a state; the service has no bypass.

## Local key handling

Private/content/recovery material stays in trusted client or authenticator memory and approved wrapped client storage. It never enters logs, analytics, crash reports, service-worker caches, server backups, or plaintext export manifests. APIs use opaque handles where possible, minimize copies, and zeroize secret buffers where the platform/runtime permits. JavaScript memory cannot guarantee zeroization.

## Crypto agility and downgrade resistance

Suite IDs are explicit and authenticated. A policy defines accepted suites per protocol version and minimum key epoch. Migration creates new envelopes/ciphertexts without silently reinterpreting old bytes. Clients reject a server-requested downgrade below policy. Adapters isolate primitive libraries; versions and feature flags are pinned and covered by vectors, fuzzing, and SBOM review.

## Mandatory production gate

Production deployment is prohibited until all evidence below exists and is approved:

1. published byte-level formats, canonical encoding/AAD, hybrid construction, KDF, labels, and error behavior;
2. stable, pinned, reviewed primitive and secret-sharing implementations;
3. known-answer, negative, tamper, malformed-input, cross-runtime, lifecycle, and recovery vectors;
4. byte-for-byte native Rust/WASM interoperability;
5. reproducible native/WASM release build and independent artifact comparison;
6. authenticated device enrollment plus key transparency/anti-substitution design and tests;
7. protocol/parser fuzzing and side-channel review appropriate to the implementations;
8. suite migration, rollback, compromise, and emergency-disable procedures;
9. independent cryptographic protocol/implementation audit and Rust/WASM build review;
10. penetration test, with all critical/high findings closed;
11. completed threat-model, dependency/license, backup/restore, browser, and Linux release gates.
