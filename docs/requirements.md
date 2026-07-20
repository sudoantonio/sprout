# Traceable requirements

## Contract

This file is the normative requirements index. Each high-level requirement (HLR) has the plan's high-level test (HLT). Each low-level requirement (LLR) has exactly one acceptance oracle (`AC-LLR-*`) and one primary automated test (`T-LLR-*`). Secondary tests may add coverage but must reference the same LLR. CI must reject an HLR/LLR without its named test, and a named test without a requirement.

An oracle describes observable pass conditions, not an implementation. “Encrypted” means the classified sensitive value is absent from server-side plaintext surfaces listed in [data-classification.md](data-classification.md). Security claims are subject to [threat-model.md](threat-model.md).

## HLR index

| Requirement | Acceptance oracle | Primary test |
| --- | --- | --- |
| **HLR-01 — Identity, authentication, and invitations** | Email/passkey registration, project creation, invitation of a new user, and a second-device login complete without disclosing classified content. | **HLT-01** end-to-end identity/invitation journey |
| **HLR-02 — Hierarchy and authorization** | A populated project/topic/task-list/task tree applies recursive grant and origin-aware revocation correctly for three users. | **HLT-02** three-user visibility matrix |
| **HLR-03 — Tasks, presets, and recurrence** | All three pretask types can be assigned with chosen values, materialized, completed, copied, and recurred without mutation or duplication. | **HLT-03** task lifecycle journey |
| **HLR-04 — Versioned questionnaires** | A task pins a published questionnaire version; later edits do not change a submitted historical view. | **HLT-04** questionnaire history journey |
| **HLR-05 — Attachments and local storage** | An encrypted template is materialized, downloaded, completed offline by the assignee, synchronized, and read on another authorized device. | **HLT-05** attachment/offline journey |
| **HLR-06 — E2EE, keys, revocation, and recovery** | Two devices and three participants share, revoke, rotate, and recover the owner only with unanimous valid approval. | **HLT-06** cryptographic lifecycle ceremony |
| **HLR-07 — Offline synchronization and conflicts** | Two offline PWAs submit duplicate/out-of-order events, resolve a stale edit client-side, and converge on one snapshot. | **HLT-07** offline convergence journey |
| **HLR-08 — Retention, export, and purge** | A virtual-clock run issues warnings, creates authorized opt-in archives, purges sources independently, and expires archives. | **HLT-08** complete retention lifecycle |
| **HLR-09 — PWA and local behavior** | The PWA installs, requests persistence, works offline, decrypts/filters locally, upgrades safely, and catches up. | **HLT-09** supported-browser PWA journey |
| **HLR-10 — Persistence, operations, and Linux deployment** | A clean Linux install migrates, boots under systemd, survives restart, restores a backup, and upgrades. | **HLT-10** Linux disaster-recovery journey |
| **HLR-11 — Supply chain and release quality** | One immutable release candidate reproducibly passes requirements, security, browser, and Linux gates. | **HLT-11** release-candidate gate |
| **HLR-12 — Disposable API and multi-client validation** | One disposable environment proves session-derived task authorization, protocol-compatible encrypted round trips, invited-user access, outsider denial, and deterministic concurrent mutation outcomes; the full per-device envelope journey remains governed by HLR-06. | **HLT-12** one-command encrypted multi-client API journey |

## HLR-01 — Identity, authentication, and invitations

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-01.1** Email is normalized, visible, and unique; name and phone are encrypted. | **AC-LLR-01.1:** case-equivalent email duplicates fail and no name/phone plaintext appears server-side. | **T-LLR-01.1:** DB normalization, uniqueness, and plaintext-absence test |
| **LLR-01.2** WebAuthn challenges are single-use and require the configured RP ID, origin, and user verification. | **AC-LLR-01.2:** a valid ceremony succeeds once; replay, wrong origin/RP ID, or missing UV fails. | **T-LLR-01.2:** virtual-authenticator browser test |
| **LLR-01.3** Email recovery restores the account, not content keys. | **AC-LLR-01.3:** recovered login cannot decrypt existing content until authorized rekey. | **T-LLR-01.3:** recovery-before-rekey decryption denial |
| **LLR-01.4** Participant suggestions sort by common-project count, then most recently modified common project. | **AC-LLR-01.4:** deterministic fixtures, including ties, produce the specified total order. | **T-LLR-01.4:** suggestion ranking fixture test |
| **LLR-01.5** An unknown invitee remains pending until acceptance. | **AC-LLR-01.5:** invitation creates no implicit full user and activates only after verified acceptance. | **T-LLR-01.5:** pending invitation lifecycle test |

## HLR-02 — Hierarchy and authorization

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-02.1** Owner may modify all objects; a creator may modify/delete only their own objects. | **AC-LLR-02.1:** every command/resource/role tuple matches this rule. | **T-LLR-02.1:** exhaustive policy matrix |
| **LLR-02.2** Only the assignee may complete, answer a questionnaire, or upload completed attachments. | **AC-LLR-02.2:** even a non-assigned owner is denied by API and RLS. | **T-LLR-02.2:** API/DB assignee matrix |
| **LLR-02.3** An ancestor grant exposes its subtree, including later descendants. | **AC-LLR-02.3:** existing and post-grant descendants become visible and usable at the granted level. | **T-LLR-02.3:** recursive future-descendant test |
| **LLR-02.4** A child grant exposes ancestors as `container_only`, never siblings. | **AC-LLR-02.4:** minimum ancestor headers decrypt; sibling payloads, names, counts, and downloads remain unavailable. | **T-LLR-02.4:** asymmetric-tree visibility test |
| **LLR-02.5** A non-owner assigns only users already able to access the task list; an owner may create task access plus container-only ancestors. | **AC-LLR-02.5:** both permitted paths succeed and every disallowed assignment fails atomically. | **T-LLR-02.5:** owner/non-owner assignment matrix |
| **LLR-02.6** Revoking one permission source preserves all other valid sources. | **AC-LLR-02.6:** effective access is independent of randomized grant/revoke order while any origin remains. | **T-LLR-02.6:** permission-graph property test |
| **LLR-02.7** Removing list access preserves an assigned task, its completion ability, and container-only ancestry, and emits an admin warning. | **AC-LLR-02.7:** all four effects commit together or none do. | **T-LLR-02.7:** transactional list-revocation test |
| **LLR-02.8** References and permissions never cross projects. | **AC-LLR-02.8:** cross-project attempts fail at API, composite FK, and RLS boundaries. | **T-LLR-02.8:** adversarial project-isolation test |

## HLR-03 — Tasks, presets, and recurrence

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-03.1** Task lists may contain mixed types; the server does not order semantic content. | **AC-LLR-03.1:** API returns neutral sync data and decrypted client filters/orderings produce expected views. | **T-LLR-03.1:** mixed-dataset client filtering test |
| **LLR-03.2** Each pretask fixes `priority`, `deadline`, or `recurring`; assignment supplies the corresponding separate value. | **AC-LLR-03.2:** every valid combination succeeds; missing or incompatible values fail. | **T-LLR-03.2:** pretask value combination table |
| **LLR-03.3** Materialized tasks are immutable snapshots with source pretask and assignment IDs. | **AC-LLR-03.3:** editing/deleting a template does not alter a materialized task. | **T-LLR-03.3:** snapshot regression test |
| **LLR-03.4** Completed tasks cannot reopen or reassign; a copy has a new ID and source link. | **AC-LLR-03.4:** forbidden transitions fail and copying preserves audit provenance without identity reuse. | **T-LLR-03.4:** completed-state/copy test |
| **LLR-03.5** Completing a recurring task atomically creates the next task and preserves the previous one. | **AC-LLR-03.5:** injected failures expose neither partial outcome. | **T-LLR-03.5:** atomic recurrence fault test |
| **LLR-03.6** Series/occurrence uniqueness and idempotency prevent duplicate occurrences. | **AC-LLR-03.6:** concurrent devices produce exactly one next occurrence. | **T-LLR-03.6:** concurrent recurrence test |
| **LLR-03.7** An incomplete recurring task remains the same task and is shown overdue after its deadline. | **AC-LLR-03.7:** virtual local time changes presentation to red without creating/replacing the task. | **T-LLR-03.7:** virtual-clock overdue UI test |

## HLR-04 — Versioned questionnaires

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-04.1** Published versions are immutable; edits create a new version. | **AC-LLR-04.1:** DB/API reject mutation and preserve both versions after editing. | **T-LLR-04.1:** immutable-version DB/API test |
| **LLR-04.2** Questions support open, single/multiple choice, and boolean types with order and required state. | **AC-LLR-04.2:** client validation accepts valid forms, rejects invalid forms, and submits only encrypted answers. | **T-LLR-04.2:** question-type validation test |
| **LLR-04.3** A task pins one version and rejects options from other versions. | **AC-LLR-04.3:** cross-version option references fail without changing the draft. | **T-LLR-04.3:** pinned-version isolation test |
| **LLR-04.4** Only the assignee may send/edit a draft; submitted responses are immutable. | **AC-LLR-04.4:** unauthorized and post-submit mutation/replay attempts fail. | **T-LLR-04.4:** submission policy/replay test |
| **LLR-04.5** Deleted questions/options remain readable in historical versions until dependent purge. | **AC-LLR-04.5:** historical submissions render faithfully throughout their retention closure. | **T-LLR-04.5:** referential-retention history test |

## HLR-05 — Attachments and local storage

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-05.1** Templates, materialized requirements, and completed files are separate entities. | **AC-LLR-05.1:** FKs and snapshots preserve type and provenance without aliasing records. | **T-LLR-05.1:** attachment provenance schema test |
| **LLR-05.2** Server filesystem stores ciphertext only; sensitive names, manifests, and logical paths are encrypted. | **AC-LLR-05.2:** automated disk inspection finds no classified plaintext. | **T-LLR-05.2:** server-filesystem plaintext scan |
| **LLR-05.3** Only the assignee uploads a completed file; effective owner, creator, and viewers may read. | **AC-LLR-05.3:** the complete role/action matrix returns the expected authorization result. | **T-LLR-05.3:** attachment authorization matrix |
| **LLR-05.4** Blob writes are atomic, ciphertext-hashed, quota/size-limited, and traversal/symlink safe. | **AC-LLR-05.4:** faults leave no committed partial blob; malicious paths/links and limit violations fail. | **T-LLR-05.4:** filesystem fault/adversary test |
| **LLR-05.5** Client caches are encrypted and local paths are never synchronized. | **AC-LLR-05.5:** IndexedDB/OPFS inspection finds only ciphertext and outbound traffic contains no local path. | **T-LLR-05.5:** Playwright storage/network inspection |
| **LLR-05.6** Untrusted files are never executed or rendered inline; declared MIME is untrusted. | **AC-LLR-05.6:** hostile fixtures download as attachments with safe headers and do not execute. | **T-LLR-05.6:** hostile-file browser test |

## HLR-06 — E2EE, keys, revocation, and recovery

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-06.1** AES-GCM, ML-KEM, ML-DSA, X25519, and dual-signature vectors match in native Rust and WASM. | **AC-LLR-06.1:** all published known-answer and cross-runtime vectors are byte-for-byte compatible. | **T-LLR-06.1:** native/WASM vector suite |
| **LLR-06.2** Nonces are unique and canonical AAD binds suite, project, resource, version, actor, and recipient. | **AC-LLR-06.2:** crash/retry never repeats a key/nonce pair and changing any bound field fails authentication. | **T-LLR-06.2:** nonce property and AAD tamper test |
| **LLR-06.3** Every device has independent keys; add/revoke never reuses old material. | **AC-LLR-06.3:** lifecycle inspection shows distinct material and revoked devices cannot receive future epochs. | **T-LLR-06.3:** device key-lifecycle test |
| **LLR-06.4** Every resource has independent keys and a mandatory owner envelope. | **AC-LLR-06.4:** compromising one resource key opens neither siblings nor unrelated resources; owner envelope is always present. | **T-LLR-06.4:** resource-compromise isolation test |
| **LLR-06.5** Revocation blocks future revisions but cannot erase downloaded copies. | **AC-LLR-06.5:** revoked online/offline devices cannot decrypt new epochs while old cached plaintext remains explicitly outside the claim. | **T-LLR-06.5:** online/offline revocation test |
| **LLR-06.6** `container_only` decrypts only a minimum header. | **AC-LLR-06.6:** it cannot derive or decrypt body, sibling, or descendant keys. | **T-LLR-06.6:** container-only crypto/API test |
| **LLR-06.7** Owner recovery requires every share from the frozen membership epoch. | **AC-LLR-06.7:** `n-of-n` succeeds; `n-1`, duplicate, expired, or wrong-epoch shares fail. | **T-LLR-06.7:** complete recovery ceremony test |
| **LLR-06.8** Recovery rotates owner wrapping key, recovery secret/shares, and envelopes; approvals are single-use. | **AC-LLR-06.8:** replay/rollback fails and no pre-recovery recovery material remains active. | **T-LLR-06.8:** post-recovery rotation/replay test |
| **LLR-06.9** Owner-only projects and projects with an unreachable participant are unrecoverable. | **AC-LLR-06.9:** recovery is refused and the UX warns before the risky state is accepted. | **T-LLR-06.9:** unrecoverable-project negative/UX test |

## HLR-07 — Offline synchronization and conflicts

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-07.1** Operations are encrypted, signed, versioned, and device-chained; replay, omission, and rollback are detected. | **AC-LLR-07.1:** each adversarial sequence is rejected or surfaced as a verifiable gap. | **T-LLR-07.1:** adversarial event-chain test |
| **LLR-07.2** Incremental cursor is authoritative; WebSocket is notification only. | **AC-LLR-07.2:** after missed/disconnected notifications, REST catch-up reaches the same state. | **T-LLR-07.2:** WebSocket-disconnect catch-up test |
| **LLR-07.3** Reusing an idempotency key with the same payload returns the original result; a different payload fails. | **AC-LLR-07.3:** retries have one effect and key collisions cannot change it. | **T-LLR-07.3:** idempotent retry/collision test |
| **LLR-07.4** A stale `base_version` creates a client-resolved conflict. | **AC-LLR-07.4:** server returns encrypted alternatives and never merges plaintext; resolved clients converge. | **T-LLR-07.4:** two-editor conflict test |
| **LLR-07.5** Completion plus next recurrence is one retry-safe batch. | **AC-LLR-07.5:** crashes/concurrency produce exactly one completion and next occurrence. | **T-LLR-07.5:** recurrence batch crash/concurrency test |
| **LLR-07.6** Snapshots rebuild from events/checkpoints; purged events cannot be resurrected by stale clients. | **AC-LLR-07.6:** reconstruction matches current state and post-purge replay is rejected. | **T-LLR-07.6:** reconstruction/post-purge replay test |

## HLR-08 — Retention, export, and purge

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-08.1** Deleted/obsolete versions warn at 15 days and purge at 30 days. | **AC-LLR-08.1:** behavior is correct immediately before, exactly at, and after each UTC threshold. | **T-LLR-08.1:** 15/30-day boundary test |
| **LLR-08.2** Completed items warn at six calendar months and purge at twelve calendar months. | **AC-LLR-08.2:** UTC calendar arithmetic is correct at month ends and leap years. | **T-LLR-08.2:** 6/12-month calendar boundary test |
| **LLR-08.3** Historical dependencies extend retention to the maximum dependent deadline. | **AC-LLR-08.3:** no referenced task/submission data purges before its retention closure. | **T-LLR-08.3:** referential-closure retention test |
| **LLR-08.4** Owner and users with access receive one in-app/email notice per window. | **AC-LLR-08.4:** concurrent workers emit exactly one notice per recipient/channel/window. | **T-LLR-08.4:** concurrent notification deduplication test |
| **LLR-08.5** Only opted-in users get a per-user encrypted archive limited to authorized data. | **AC-LLR-08.5:** archives exclude other users' data and no archive exists for opt-out. | **T-LLR-08.5:** export authorization/isolation test |
| **LLR-08.6** Missing/failed export does not block purge; failed purge retries idempotently. | **AC-LLR-08.6:** disk-full, DB restart, and worker crash do not violate either rule. | **T-LLR-08.6:** retention fault-injection test |
| **LLR-08.7** Archive is available at next login and deleted 30 days after source purge, downloaded or not. | **AC-LLR-08.7:** browser sees it before expiry and worker removes it exactly at/after expiry. | **T-LLR-08.7:** archive delivery/expiry test |
| **LLR-08.8** Checksum, signature, and manifest detect corruption; browser offers standard download and manual fallback. | **AC-LLR-08.8:** corrupt archives fail verification and blocked automatic downloads expose a usable fallback. | **T-LLR-08.8:** corruption/download-fallback test |

## HLR-09 — PWA and local behavior

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-09.1** PWA requests persistent storage, reports the result, and preserves queued operations on refusal/quota exhaustion. | **AC-LLR-09.1:** both failure modes remain visible and recoverable without operation loss. | **T-LLR-09.1:** browser persistence/quota test |
| **LLR-09.2** Filters, deadlines, priorities, and ordering run on decrypted client data. | **AC-LLR-09.2:** mixed fixtures produce expected UI while server receives no semantic filter values. | **T-LLR-09.2:** local-filtering UI/network test |
| **LLR-09.3** Service worker stores no plaintext responses and intercepts no secrets. | **AC-LLR-09.3:** cache/request inspection contains neither classified content nor key material. | **T-LLR-09.3:** service-worker cache inspection |
| **LLR-09.4** Supported Chromium, Firefox, and Safari builds pass; limited filesystem APIs fall back to standard download. | **AC-LLR-09.4:** supported-version matrix passes with equivalent safe outcomes. | **T-LLR-09.4:** cross-browser compatibility matrix |
| **LLR-09.5** PWA uses strict CSP, Trusted Types where available, and no third-party scripts. | **AC-LLR-09.5:** header checks pass and XSS fixtures cannot execute. | **T-LLR-09.5:** CSP/Trusted-Types XSS test |

## HLR-10 — Persistence, operations, and Linux deployment

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-10.1** Domain changes, permission propagation, events, and outbox commit in one transaction. | **AC-LLR-10.1:** injected faults leave no externally visible partial state. | **T-LLR-10.1:** transaction rollback/fault test |
| **LLR-10.2** Composite FKs and RLS block cross-project access even without the API. | **AC-LLR-10.2:** direct SQL attempts cannot create/read/update cross-project relations. | **T-LLR-10.2:** direct-SQL isolation test |
| **LLR-10.3** Forward migrations and restore support the target PostgreSQL major. | **AC-LLR-10.3:** empty DB and previous snapshot both migrate/restore to a valid current schema. | **T-LLR-10.3:** migration/restore compatibility test |
| **LLR-10.4** Worker leases prevent duplicate notice/purge and recover after crash. | **AC-LLR-10.4:** competing workers have one effect and an expired lease is safely resumed. | **T-LLR-10.4:** worker lease/crash test |
| **LLR-10.5** systemd uses a non-privileged user, restricted directories, controlled restart, and graceful shutdown. | **AC-LLR-10.5:** Linux harness verifies hardening and no interrupted committed work. | **T-LLR-10.5:** systemd VM/container test |
| **LLR-10.6** PostgreSQL, blobs, and archive manifests restore ciphertext and relations without client keys. | **AC-LLR-10.6:** a consistency-set restore passes integrity/relation checks without decrypting content. | **T-LLR-10.6:** disaster-recovery drill |
| **LLR-10.7** Health/readiness, redacted structured logs, and error/lag/quota metrics are available. | **AC-LLR-10.7:** smoke checks pass and secret/plaintext canaries never appear in telemetry. | **T-LLR-10.7:** observability/redaction smoke test |

## HLR-11 — Supply chain and release quality

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-11.1** Application dependencies are open source under the allow-list; copyleft needs explicit review. | **AC-LLR-11.1:** MIT, Apache-2.0, BSD, ISC, PostgreSQL, and Zlib are accepted; MPL/LGPL/GPL are blocked pending documented review. | **T-LLR-11.1:** `cargo-deny`, npm audit, and license-report gate |
| **LLR-11.2** No open critical/high vulnerability; lockfiles, SBOM, and advisory monitoring are current. | **AC-LLR-11.2:** release gate has zero unresolved critical/high findings and emits lockfile-derived SBOMs. | **T-LLR-11.2:** vulnerability/SBOM CI gate |
| **LLR-11.3** Protocol parsers are fuzzed against malformed ciphertext, keys, events, and manifests. | **AC-LLR-11.3:** continuous fuzz suites complete with no crash, panic, hang, or unsafe acceptance. | **T-LLR-11.3:** parser/protocol fuzz suite |
| **LLR-11.4** Threat model covers curious/malicious server, XSS, compromised device, rollback, insider, and key loss. | **AC-LLR-11.4:** a versioned review records coverage, owners, and unresolved risk acceptance. | **T-LLR-11.4:** signed threat-model review gate |
| **LLR-11.5** Independent protocol and Rust/WASM build audit plus penetration test precede production. | **AC-LLR-11.5:** independent reports exist and all critical/high findings are closed before production approval. | **T-LLR-11.5:** production evidence gate |

## HLR-12 — Disposable API and multi-client validation

| Requirement | Acceptance oracle | Primary automated test |
| --- | --- | --- |
| **LLR-12.1** Task API actors always come from authenticated sessions; missing sessions and unrelated users cannot read or mutate task state. | **AC-LLR-12.1:** all task CRUD routes reject missing authentication, an unrelated authenticated user is denied without state change, and the same user becomes authorized only after an explicit invitation is accepted. | **T-LLR-12.1:** authenticated task authorization transition |
| **LLR-12.2** The validation client uses the production protocol crate for encryption and independently expected decryption context. | **AC-LLR-12.2:** an API ciphertext round trip decrypts with the retained DEK and expected context, fails with a wrong key/context, and a populated PostgreSQL dump contains no classified plaintext canary. | **T-LLR-12.2:** protocol round-trip and server plaintext scan |
| **LLR-12.3** Concurrent authenticated task mutations have a deterministic single committed outcome. | **AC-LLR-12.3:** two valid updates against one expected version produce exactly one success and one conflict, and the committed ciphertext decrypts to the successful command. | **T-LLR-12.3:** concurrent task update race |
| **LLR-12.4** A clean machine can run the validation journey and its crypto helpers through documented Docker commands only. | **AC-LLR-12.4:** one Compose command builds, migrates, runs the encrypted journey, reports each oracle, and exits non-zero on any failure. | **T-LLR-12.4:** disposable Docker validation harness |
| **LLR-12.5** Multi-client cryptographic authorization uses independent device packages and API-propagated resource-key envelopes, not an out-of-band shared DEK. | **AC-LLR-12.5:** an invited device unwraps its own envelope, an outsider cannot obtain or unwrap one, and revocation prevents the invited device from decrypting the next epoch. | **T-LLR-12.5:** three-client envelope and revocation journey |

## Production release rule

Passing ordinary CI is insufficient for production. Production additionally requires the cryptographic gate in [crypto-protocol.md](crypto-protocol.md): public format specification, stable/pinned primitive implementations, test vectors, native/WASM parity, reproducible WASM, key transparency/anti-substitution, suite migration, and an independent audit with critical/high findings closed. No such independent audit is claimed by this repository today.
