# Threat model

## Status and scope

This is a design-stage threat model, not an audit report. It covers the PWA, Rust/WASM crypto boundary, API and worker, PostgreSQL, encrypted blob/archive storage, synchronization, authorization, recovery, build pipeline, and deployment. It must be reviewed whenever a trust boundary, primitive, payload format, browser support policy, or recovery rule changes.

## Assets and goals

| Asset | Security goal |
| --- | --- |
| Content plaintext, answers, filenames, logical paths | Confidentiality from the service and unauthorized users |
| Content/resource/device/recovery keys | Confidentiality, integrity, correct lifecycle, non-reuse |
| Events, snapshots, manifests, key packages | Authenticity, ordering/rollback detection, project/resource binding |
| Identity, membership, routing metadata | Integrity, least disclosure, project isolation |
| Authorization and retention state | Correct, atomic enforcement and auditable transitions |
| Client software and protocol implementation | Origin/build integrity and native/WASM equivalence |
| User data and recovery capability | Availability within the documented recovery trade-off |

The design does not promise traffic-analysis resistance, anonymity, protection after endpoint compromise, guaranteed availability, or deletion of plaintext/ciphertext already copied by an authorized recipient.

## Trust boundaries and actors

- **Authorized user/device:** trusted only for resources currently granted. It may be buggy, malicious, revoked, lost, or offline.
- **Browser/PWA origin:** trusted with plaintext and keys during use. The service delivering it is therefore security-critical despite E2EE.
- **Curious service operator:** may inspect DB, blobs, backups, logs, traffic metadata, and operational state but follows the protocol.
- **Malicious/compromised service or insider:** may substitute client code, omit/reorder/rollback ciphertext, manipulate routing metadata, or deny service.
- **External attacker:** may target WebAuthn, sessions, API parsers, XSS, CSRF, supply chain, filesystem paths, or deployment.
- **Dependency/build-system attacker:** may substitute primitives, packages, compilers, or release artifacts.

## Primary threats and controls

| Threat | Controls | Residual risk / validation |
| --- | --- | --- |
| Server reads content | Client-side encryption, independent resource keys, encrypted filenames/manifests, no server keys | Metadata and sizes remain visible; disk/telemetry plaintext scans |
| Malicious JavaScript from the PWA origin steals keys/plaintext | Strict CSP, Trusted Types where available, no third-party scripts, immutable signed artifacts, reproducible WASM/builds, separate static hosting, short key lifetime in memory | **Not eliminated.** A compromised origin can serve code that exfiltrates secrets before encryption or after decryption. Native/signed-client options may be needed for a stronger distribution trust model |
| XSS executes in the trusted origin | Context-safe rendering, CSP, Trusted Types, no inline/third-party scripts, hostile-content tests | Browser/extension defects and new injection sinks remain possible |
| Key-package or device substitution | Authenticated device enrollment, dual signatures, key transparency/anti-substitution log, user-visible device changes | Transparency is a production gate and must be independently reviewed |
| Ciphertext tampering or cross-context replay | AEAD, canonical AAD binding suite/project/resource/version/actor/recipient, signatures, strict version parsing | Nonce/AAD/vector/tamper tests and fuzzing required |
| Replay, omission, reordering, or rollback | Per-device signed hash chains, monotonic cursors/checkpoints, idempotency keys, stale-base conflicts | A malicious service can delay/withhold data; clients must surface gaps and cannot force availability |
| Cross-project authorization | Composite project FKs, application policy, RLS, transactional propagation | Direct-SQL adversarial tests; privileged DB/operator can still deny or corrupt service |
| Permission-source confusion | Origin-aware permission records; effective access is union of valid sources | Property tests over randomized grant/revoke graphs |
| Child access leaks siblings | Separate `container_only` header keys and asymmetric query policy | Ancestor existence and minimum routing metadata are intentionally exposed |
| Revoked user reads new content | New key epoch and new envelopes for future revisions | **Revocation cannot erase keys, plaintext, screenshots, exports, or ciphertext already obtained.** Offline devices may retain past access indefinitely |
| One resource key compromises others | Independent per-resource KEKs and per-revision/blob DEKs; no global project content key | Owner/device compromise may expose every resource whose envelope that device has |
| Compromised endpoint | Device revocation, new epochs, user-visible device inventory, minimal in-memory/key storage | Malware/extensions can capture plaintext and keys while active; E2EE cannot protect a compromised endpoint |
| Owner recovery abuse | Frozen membership epoch, `n-of-n` shares, signed scoped/time-limited approvals, complete post-recovery rotation | **Availability risk:** one absent participant, an owner-only project, or lost shares makes owner recovery impossible |
| Recovery replay/rollback | Nonce/expiry/project/device/epoch binding; single-use approvals; rotate wrapping/recovery keys, shares, envelopes | Recovery ceremony and rollback tests required |
| Account recovery mistaken for E2EE recovery | Email recovery restores authentication only; explicit locked-content state | Social engineering remains possible; UX must not imply key recovery |
| Blob traversal/symlink/execution | Server-generated opaque paths, safe open/write, atomic rename, quotas, ciphertext hash, forced download, untrusted MIME | Decrypted hostile files can attack external viewers after download |
| Retention/export leaks or deletes incorrectly | Per-user authorization, encrypted archives, signed manifests, dependency closure, UTC virtual clock, idempotent workers | Authorized users can retain exports; purge does not claw back copies |
| Secrets enter logs/metrics/backups/cache | Structured allow-listed telemetry, encrypted local caches, canary scans, service-worker cache tests | Crash dumps, browser extensions, and host compromise require operational controls |
| Dependency/build compromise | Lockfiles, SBOM, license allow-list, advisory gate, pinned adapters, reproducible native/WASM artifacts, independent audit | Reproducibility detects only when independently rebuilt and compared |
| Denial of service/quota exhaustion | Size/quota/rate limits, worker leases, bounded parsing, backups, readiness/monitoring | Availability is not guaranteed; E2EE limits server-side content recovery |

## Metadata leakage

The service necessarily learns some information: normalized email, user/device/project/resource routing IDs, membership and permission relationships, creator/assignee relationships, operational timestamps, protocol/key epochs, event ordering, cursors, tombstones, ciphertext/blob/archive lengths, access/upload timing, IP/network metadata, and retention/export state. These can reveal social graphs, activity patterns, approximate content size, hierarchy shape, and recurrence.

Mitigations are minimization, opaque IDs, coarse or bucketed sizes where useful, short operational retention, no semantic server-side filtering, and restricted telemetry. Padding does not hide all sizes or timing. The product and privacy notice must not describe E2EE as hiding this metadata.

## Security invariants

1. Classified sensitive plaintext and unwrapped keys never cross the encrypted-payload boundary.
2. Every authenticated ciphertext is versioned and context-bound; unknown suites/versions fail closed.
3. No global project content key grants implicit access to all resources.
4. Effective authorization and delivered envelopes agree for each resource epoch.
5. Removing one permission origin cannot remove another valid origin.
6. A server transaction commits domain state, permission effects, event, and outbox together.
7. WebSocket messages are hints; cursor-based synchronization is authoritative.
8. A purge tombstone/checkpoint prevents stale clients from resurrecting purged data.
9. Recovery approvals are scoped, expiring, epoch-bound, and single-use.
10. Production is forbidden until every cryptographic production gate is evidenced.

## Review and evidence

Owners must maintain requirement-linked tests, abuse-case tests, a data-flow review, dependency/SBOM reports, reproducible-build evidence, recovery and restore drills, and a risk register. Before production, an independent cryptographic audit and penetration test are required; critical/high findings must be closed. This repository does **not** claim that either has occurred.
