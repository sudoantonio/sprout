# ADR-0005: Encrypted event sync with client conflict resolution

- Status: Accepted
- Date: 2026-07-18

## Context

The PWA must work offline across devices while the service cannot inspect or merge plaintext. Networks duplicate, delay, omit, and reorder requests; clients may edit stale snapshots or reconnect after purge. WebSocket delivery is not durable.

## Decision

Use versioned encrypted, dual-signed, append-only operations plus encrypted current snapshots and periodic checkpoints:

- each device event carries a sequence, predecessor hash, `base_version`, resource/key epoch, and idempotency key;
- an incremental server cursor is authoritative; WebSocket carries only a “changes available” hint;
- retry with the same idempotency key and payload returns the original result; a different payload with that key fails;
- a current `base_version` may advance atomically; a stale version creates an explicit encrypted conflict;
- clients decrypt alternatives, ask the user or apply a domain-specific local rule, then submit a new resolution event;
- the server never merges semantic plaintext;
- completion plus next recurrence is one atomic retry-safe batch;
- purge creates durable tombstones/checkpoints that reject resurrection from stale clients.

IndexedDB/OPFS hold encrypted snapshots, events, files, and queue state. Queued operations survive persistence refusal and quota handling through explicit user-visible recovery/export paths.

Completed attachments use a separate versioned IndexedDB queue. OPFS holds
only the encrypted file container; the queue holds opaque project/task/blob
identifiers, ciphertext hashes, encrypted metadata, provenance IDs and a
stable idempotency key. The item is written before network declaration and is
removed only after declaration, ciphertext upload and availability
confirmation all succeed. Reconnection serializes queue flushing so UI effects
and online events cannot race duplicate uploads.

For the legacy IndexedDB v1 to v2 upgrade, Sprout intentionally rebuilds the
local schema. Only queue entries that pass the complete signed-operation shape
check are recovered. Legacy vaults, projections, tombstones, conflicts, unknown
stores, and malformed or unsigned queue entries are deleted. They are local
derivatives or do not meet the current integrity contract and can be fetched
again or recreated after authorization; silently migrating them would preserve
data whose encryption and signature guarantees cannot be established.

## Consequences

- Devices can catch up after missed WebSocket notifications and converge under tested resolution rules.
- Signed chains expose replay, predecessor mismatch, and rollback/gaps, but a malicious service can still withhold data or deny availability.
- User-visible conflicts are unavoidable for concurrent semantic edits.
- Snapshot/checkpoint retention and event purge must remain jointly testable.
- Local storage, service-worker update, multi-tab coordination, quotas, and browser differences become correctness/security concerns.
- A v1 upgrade can require reauthorization and a projection catch-up, but it
  does not discard a valid signed mutation that has not reached the server.

## Rejected alternatives

- **WebSocket as source of truth:** rejected because delivery is transient.
- **Last-write-wins:** rejected because it silently loses encrypted concurrent edits.
- **Server-side merge/CRDT over plaintext:** rejected because it crosses the encrypted-payload boundary.
- **Opaque retries without idempotency:** rejected because crashes and multiple devices duplicate effects, especially recurrence.
- **Allow stale replay after purge:** rejected because it defeats retention.
