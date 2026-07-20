# ADR-0002: Encrypted payload boundary

- Status: Accepted
- Date: 2026-07-18

## Context

The service must route, authorize, synchronize, retain, and export records without learning task, questionnaire, attachment, or profile semantics. Server-side semantic fields, filtering, filenames, or archive manifests would weaken the E2EE claim and spread plaintext into logs, backups, and administration.

Browser-delivered code still receives plaintext and keys. E2EE against storage or API operators does not protect users from malicious JavaScript served by a compromised origin.

## Decision

Treat the client-to-service API as an encrypted-payload boundary:

- sensitive content is encrypted in Rust/WASM before transport;
- server-visible fields are limited to identity, opaque routing, authorization relations, operational timestamps, protocol/version/epoch, synchronization, retention, size, and ciphertext integrity metadata;
- filenames, logical paths, questionnaire material, titles/bodies, and semantic filter/order values remain encrypted;
- the PWA decrypts, filters, sorts, and resolves content conflicts locally;
- server storage, logs, metrics, traces, backups, and archives contain no sensitive plaintext or unwrapped client keys;
- all new DTO/schema fields default to sensitive until classification review.

Strict CSP, Trusted Types where available, no third-party scripts, immutable signed artifacts, reproducible builds, and controlled service-worker deployment are mandatory defense-in-depth, not a solution to origin compromise.

## Consequences

- A database/blob/backup disclosure should expose ciphertext plus restricted metadata, not content.
- The service cannot perform semantic search, ordering, validation, moderation, or plaintext conflict merge.
- Metadata still leaks collaboration graph, hierarchy, timing, sizes, and activity patterns and must be minimized/disclosed.
- Client complexity and cross-runtime protocol testing increase.
- A malicious service can still deny/rollback data and can substitute PWA JavaScript unless distribution controls detect/prevent it; a stronger endpoint trust model may require a separately distributed signed native client.

## Rejected alternatives

- **Server-side field encryption with server keys:** rejected because operators/compromise can decrypt content.
- **Encrypt only bodies, leave titles/filenames/filter fields clear:** rejected because those fields are semantically sensitive.
- **Claim CSP makes the PWA trusted:** rejected; CSP mitigates injection but cannot stop an origin authorized to replace the application.
