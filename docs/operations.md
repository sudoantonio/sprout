# Operations and production readiness

## Current readiness

Sprout is pre-production. Documentation or passing unit tests do not authorize production use. There is no claim that an independent cryptographic audit or penetration test has occurred.

The server therefore fails closed in `production` mode. Local and CI
environments must set `SPROUT_ENVIRONMENT=development` together with the
explicit `SPROUT_ENABLE_EXPERIMENTAL_CRYPTO_FOR_DEVELOPMENT=true` opt-in.
Changing these variables cannot bypass the independent production-audit gate.

## Environments and artifact promotion

Build once, verify the immutable artifact, and promote that same digest. Production builds use lockfiles, pinned toolchains, frozen dependency resolution, generated SBOM/notices, and reproducible Rust/WASM verification. Runtime configuration may change by environment; executable/static assets may not.

The PWA and its service worker are security-critical artifacts. Serve only first-party code over HTTPS with strict CSP, Trusted Types where supported, no third-party script origins, immutable hashed assets, and controlled service-worker rollout/rollback. Separate static hosting and signatures reduce substitution risk but do not eliminate the malicious-JavaScript limitation: compromise of the serving origin can expose client plaintext and keys.

## Service layout

API and retention/export worker run as separate systemd units under dedicated non-privileged identities. Units must have:

- explicit read/write directories and restrictive filesystem/system-call capabilities;
- no shell login and no access to client keys;
- bounded restart with backoff;
- graceful shutdown that stops admission, drains bounded work, and releases/lets leases expire;
- configuration/secrets supplied by a protected runtime mechanism, never committed or logged;
- dependency ordering for network/storage without treating ordering as readiness.

PostgreSQL stores metadata/events/ciphertext. The blob root stores only opaque ciphertext objects. Temporary archives use a separate restricted root and independent expiry monitoring.

## Startup, migration, and shutdown

1. Verify configuration schema, directory ownership/permissions, supported PostgreSQL major, and available space.
2. Back up before schema changes and acquire a migration lock.
3. Apply forward migrations with a versioned, tested binary; destructive data purge is never an implicit migration.
4. Start API/worker, then admit traffic only after readiness passes.
5. On shutdown, fail readiness, stop new jobs/requests, finish or safely abandon bounded transactions, and close connections.

Rollback normally means restoring the previous immutable application artifact while retaining forward-compatible schema. A database rollback uses a tested restore, not ad-hoc down migrations.

## Health and telemetry

- **Liveness:** process event loop and fatal-state status only; it must not fail merely because a dependency is transiently unavailable.
- **Readiness:** migration compatibility, PostgreSQL connectivity, required ciphertext directories, write capacity, and critical configuration.
- **Metrics:** request/error rate, latency, DB pool, sync cursor lag, event-chain gaps, worker lease/job lag, retries, purge/archive backlog, quota/disk capacity, key/suite-version counts, and CSP/reporting signals.
- **Logs:** structured, allow-listed fields; correlation/operation IDs, coarse outcome, and error class. Never request/response bodies, ciphertext payloads, filenames, email tokens, session tokens, WebAuthn data beyond safe IDs, private/content/recovery keys, decrypted errors, or local paths.

Canary tests seed unique plaintext/secrets and assert absence from logs, traces, metrics, PostgreSQL plaintext columns, filesystems, caches, backups, and archives.

## Backup and restore

A recoverable consistency set contains:

1. PostgreSQL backup with schema/migration version;
2. ciphertext blob tree;
3. encrypted archive tree;
4. blob/archive manifests and hashes;
5. purge receipts/tombstones and retention job state;
6. artifact/config version and SBOM, excluding secrets.

Backups are encrypted and access-controlled operationally, but the service still must not possess client content keys. Capture and restore ordering must preserve DB-to-file references. Retention for backup media is separately configured and bounded; a restore must apply purge receipts before traffic so deleted data does not reappear.

At least one automated restore and a periodic disaster-recovery drill must verify hashes, relationships, migrations, API readiness, synchronization catch-up, and retention state on the same PostgreSQL major as the target. A successful file copy is not a successful restore.

## Worker safety

Retention/export jobs use expiring leases, stable operation IDs, unique effect keys, checkpoints, and retry-safe state transitions. Competing workers may perform at most one notification, archive registration, or purge effect. Technical purge failures alert and retry; export failures never postpone source purge. Monitor both source-purge age and archive-expiry age.

## Incident response

| Signal | Immediate action |
| --- | --- |
| Sensitive plaintext or key in server surface | Stop affected emission/access, preserve restricted evidence, rotate reachable credentials/keys, identify scope, remove exposed artifacts where safe, notify according to policy |
| Suspected client-origin/build compromise | Stop serving/promoting artifact, revoke sessions/device packages as appropriate, publish trusted notice, rebuild independently, rotate affected resource epochs after clean client recovery |
| Key-package substitution/transparency failure | Freeze enrollment/sharing, preserve log proofs, reject unverified packages, investigate operator/build compromise |
| Nonce reuse, signature bypass, parser flaw | Disable affected suite/version, halt production writes if confidentiality/integrity is at risk, invoke migration/rotation plan |
| Cross-project authorization | Disable affected route/job, preserve audit evidence, fix application and RLS layers, assess all affected resources |
| Blob/database corruption | Quarantine writes, verify manifests/hashes, restore consistency set, replay only validated events and purge receipts |
| Lost owner recovery quorum | Explain unrecoverability; do not create a server bypass or claim content recovery |

Revocation protects future epochs only. Incident communication must not imply that previously downloaded plaintext or keys have been erased.

## Production approval checklist

All items are release blockers:

- every HLR/LLR test and traceability/orphan check is green;
- [threat model](threat-model.md) reviewed and residual risks accepted;
- [data classification](data-classification.md) canary scans pass;
- byte-level [crypto protocol](crypto-protocol.md), vectors, native/WASM parity, reproducibility, key transparency, migration, and compromise procedures are complete;
- independent cryptographic audit and Rust/WASM build review completed; critical/high findings closed;
- penetration test completed; critical/high findings closed;
- dependency allow-list, advisory, lockfile, SBOM, and notices gates pass;
- Chromium, Firefox, and Safari support matrix passes, including safe filesystem fallback and service-worker update;
- migration, backup/restore, worker crash, retention boundary, and systemd hardening tests pass;
- dashboards, alerts, incident contacts, recovery runbooks, capacity limits, and rollback decision authority are assigned;
- exact immutable artifact digest approved and promoted without rebuild.

If evidence is missing, the system remains non-production regardless of schedule.
