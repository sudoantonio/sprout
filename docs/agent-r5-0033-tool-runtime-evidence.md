# Sprout R5 checkpoint 0033 — governed external-tool runtime evidence

Date: 2026-08-24

Baseline: `a798eed88a5cb478fbc6f9016733cae561acd3a1`

Canonical Lean SHA-256: `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`

## Claim boundary

0033 concretely refines the deterministic authorization, persistence and
device-owned execution boundary for governed external tools. The model may
propose a tool action; it cannot grant permission, choose risk, expand an
authority ceiling or execute the connector. The server coordinates immutable
coordinates, ciphertext, commitments and provenance. Plaintext, connector
credentials, private endpoints and filesystem paths remain at the user-owned
edge (`crates/domain/src/external_tools.rs:1`,
`frontend/sprout-web/src/tools/edge-runtime.ts:1`).

This checkpoint does **not** claim a complete R5.40/R5.41 tool trace. The
operational ledger deliberately exposes an empty formal view because no exact
shared `traceId` projection exists yet
(`db/migrations/0033_agent_external_tool_runtime.sql:2037`). Therefore the
R5.41 tool surface remains **FAIL-CLOSED / NOT IMPLEMENTED**:
`disabledFailClosed`, `records = []`.

## Coverage classification

| Area | Status | Concrete evidence and boundary |
| --- | --- | --- |
| `ToolCallRecord`, terminal shape, retry | **CONCRETELY REFINED** | Domain validation distinguishes pending/succeeded/failed/timed-out and exact retry (`crates/domain/src/external_tools.rs:436`). Call, dispatch, request, observation and audit storage are separate (`db/migrations/0033_agent_external_tool_runtime.sql:496`, `:611`, `:659`, `:694`, `:862`). |
| Tool manifest and security semantics | **CONCRETELY REFINED** for the initial inventory | Migration-owned immutable catalog binds schemas, hashes, adapter protocol, bounds, complete required Sprout effects, trusted owner-derived audience, risk and terminal mapping (`db/migrations/0033_agent_external_tool_runtime.sql:20`). |
| `ToolReady` | **CONCRETELY REFINED** | Invoke/retry require a current device-signed, tool/version/manifest/profile-bound capability witness plus exact current actor permission and authority-principal permission (`apps/server/src/routes/agent_tools.rs:88`, `:539`, `:1779`). Catalog presence alone is insufficient. |
| Run/work authority | **CONCRETELY REFINED** for exact run-sponsored and inherited work | Immutable run/work ceilings come from the certified authority envelope, not the union of WorkSpec policies. The recursive origin resolver requires exact initialization/continuation evidence (`crates/domain/src/external_tools.rs:56`). Possible human-delegation provenance, missing parent, cycle and ambiguity fail closed. |
| Human-delegation origin | **FAIL-CLOSED / NOT IMPLEMENTED** | Existing Task→Work signals do not reconstruct the complete Lean `HumanAgentTaskDelegation`; they are named possible/unsupported provenance and never reclassified as run sponsor or inherited work (`crates/domain/src/external_tools.rs:25`). |
| Required effects | **CONCRETELY REFINED** for executable v1 read tools | Manifest-derived effects are exact and currently empty only because the enabled tools do not mutate a Sprout `ResourceSecurityEffect`. `web.read` remains an external-egress boundary; empty Sprout mutation effects do not make it authority-free. |
| Local execution | **LIVE FEATURE TESTED** in the controlled development edge | `web.read` and `document.local.read` execute through explicit native transport interfaces; controlled loopback HTTP and a temporary file capability were exercised (`frontend/sprout-web/src/tools/edge-runtime.live.test.ts:36`). No production native companion/package is claimed. |
| Document edit | **CONTRACT TESTED / PARTIAL / EXTERNAL TCB** | Consent, expected version, idempotency and atomic-replace contract are tested, but the tool remains `contract_only` and unreachable from the executable catalog (`frontend/sprout-web/src/tools/edge-runtime.test.ts:88`). It is not Sprout Info editing. |
| Mail/Telegram receive | **CONTRACT TESTED** | Typed local-only profiles and fake read connectors exist; credentials are held only by the encrypted device vault (`frontend/sprout-web/src/tools/connector-profiles.test.ts:18`). |
| Mail/Telegram send | **FAIL-CLOSED / NOT IMPLEMENTED** | The Lean vocabulary has no exact external mail/Telegram `DisclosureSink`; both sends are catalogued fail-closed and the edge rejects them (`crates/domain/src/external_tools.rs:227`, `frontend/sprout-web/src/tools/edge-runtime.test.ts:13`). |
| Task/TaskList/Topic/Info/Comment aliases | **STRUCTURALLY UNREACHABLE** | Catalog constraints and domain validation reject native-surface aliases. These remain native `AgentAction`/`ResourceOperation` paths, never tools (`db/migrations/0033_agent_external_tool_runtime.sql:20`, `crates/domain/src/external_tools.rs:460`). |
| Concrete comments | **FAIL-CLOSED / NOT IMPLEMENTED** | 0033 does not implement comments. The R5.41 comment surface remains disabled and empty. |
| Tool output as model context | **CONCRETELY REFINED** for succeeded owner-audience output | `ToolOutput(callId)` follows a separate producer/consumer path and never calls resource access on the run scope (`apps/server/src/routes/agents.rs:7410`). Exact call, attempt, claim-at-request, dispatch, observation, output commitment, trusted principal audience and device key envelope are joined. |
| Output E2EE | **CONCRETELY REFINED** for the current single runner device limit | The E2E constructs a hybrid X25519 + ML-KEM package, verifies dual signatures and performs controlled unwrap with the recipient private keys. Audience remains principal-level; a device envelope does not grant readership (`apps/server/tests/agents.rs:2180`). |
| R5.41 tool surface | **FAIL-CLOSED / NOT IMPLEMENTED** | The production inventory cannot enable from operational rows: `agent_r541_tool_surface_records` is structurally empty until complete R540 trace coordinates exist (`db/migrations/0033_agent_external_tool_runtime.sql:2037`). |
| Production DB role provisioning | **PARTIAL** | Dev/CI `DATABASE_URL` currently uses bootstrap/superuser. A `NOSUPERUSER NOBYPASSRLS` role with narrowly granted trusted-writer execution was tested, but least-privileged production role provisioning is not yet encoded in deployment. RLS does not protect against a compromised owner/superuser. |

## Exact inventory

| Tool/version | Product risk | 0033 state | Notes |
| --- | --- | --- | --- |
| `web.read@1` | TR2 | executable; development-edge live tested | GET/HEAD only, bounded redirects, DNS re-resolution, private/link-local/metadata denial, no ambient cookies/auth, bounded MIME/bytes/time, passive extraction. Public network egress remains an external boundary. |
| `document.local.read@1` | TR1 | executable; development-edge live tested | Opaque user-granted capability, text/Markdown only, size/type bound, native realpath/symlink fence. No path is sent to Sprout. |
| `document.local.edit@1` | TR1 | contract only | External filesystem mutation; not a Sprout resource mutation and not formally closed. |
| `mail.receive@1` | TR2 | contract only | Fake adapter only; no live credential. |
| `telegram.receive@1` | TR2 | contract only | Fake adapter only; no live credential. |
| `mail.send@1` | TR3 | fail closed | Missing exact external disclosure sink. |
| `telegram.send@1` | TR3 | fail closed | Missing exact external disclosure sink. |

No tool identity contains `task`, `task_list`, `topic`, `info` or `comment`.
Risk is migration/manifest-derived and can only restrict; it is neither
permission nor authority (`db/migrations/0033_agent_external_tool_runtime.sql:63`).

## Authority and readiness

At run initialization, the server persists the finite tool ceiling already
present in the certified `AuthorityEnvelope`; historical 0032 runs receive no
synthetic snapshot. Every work ceiling is a subset of that immutable run
ceiling. The exact resolver returns only:

- `RunSponsor(principal)` when append-only initialization evidence proves an
  unparented root;
- `InheritedWork(parent, principal)` when every continuation edge is exact and
  recursively reaches a certified root.

Unknown, ambiguous, cyclic, missing-parent and possible human-delegation cases
reject. Invoke/retry independently check the current permission of both the
effect actor and `workAuthorityPrincipal`, so one cannot substitute for the
other (`crates/domain/src/external_tools.rs:594`). Grants/revocations are exact
on `(project, principal, tool_id, tool_version)` and go through private hardened
writers; direct app-role DML is rejected
(`db/migrations/0033_agent_external_tool_runtime.sql:167`, `:306`,
`db/tests/verify_behavior.sql:4487`). Granting v1 does not grant v2.

The runtime capability witness attests both installed/profile availability and
current executable readiness for one exact tool version, runner device,
manifest and opaque execution-profile commitment. It is signed, short-lived
and contains no endpoint or secret (`crates/domain/src/external_tools.rs:375`).
Invoke and retry require it; a terminal for an already-pending request does not.

## ToolInput and commitments

The concrete `V.ToolInput` is the canonical structured tool input identified
by `structured_input_commitment_hex`. It includes the server-derived owner
binding required to derive `owner_only` audience. The signed invocation
statement is a distinct provenance wrapper and has its own statement hash
(`apps/server/src/routes/agent_tools.rs:498`). The runtime keeps distinct:

1. server-computed encrypted-input payload commitment;
2. edge-signed canonical ToolInput commitment;
3. signed input-statement hash;
4. post-serialization external wire-request commitment;
5. opaque execution-profile commitment;
6. server-computed encrypted-output payload commitment;
7. edge-signed canonical ToolOutput commitment;
8. terminal-statement hash.

Dispatch and terminal statements bind tool/version, canonical input, exact
WorkAttempt, adapter protocol, profile and request witness. There is exactly
one external request record per persisted attempt; adapters do not hide retry
inside an attempt (`apps/server/src/routes/agent_tools.rs:888`, `:1032`).

## Time, terminal and retry state machine

`requestedAt` is the server semantic tick of invoke/retry, not claim
acquisition. Authorization checks `acquiredAt <= requestedAt < expiresAt`, and
the same tick is the concrete WorkAttempt/tool-event coordinate. The deadline
is `requestedAt + timeoutTicks`; dispatch is optional and cannot move the
deadline.

Server timeout covers pending attempts with no dispatch, dispatch without a
wire request, and dispatch with an exact request witness. It is unsigned and
never fabricates a device observation, output or missing request. A
signed-edge terminal and server timeout race under one locked transition; only
one wins.

Terminal completion/failure/timeout closes atomically the ToolCall, claim,
WorkItem, WorkOutcome, transition and append-only audit. It relies on immutable
valid-at-request provenance and therefore does not recheck current permission,
runtime witness or claim activity. This preserves the formal rule that later
revocation/expiry does not invalidate an already-caused terminal event.

For failed/timed-out attempt N, the immediate semantic snapshot remains
`WorkStatus.failed`, attempt N, with exact failed WorkOutcome. A distinct
`tool_retry_rearmed` transition may later materialize attempt N+1 as
eligible/blocked; only then can normal claim create exact claim N+1 and
`RetryTool` reopen the same ToolCall ID with the same tool/input/bounds
(`crates/domain/src/agents.rs:2633`, `:2674`). Every materialized work attempt
directly satisfies the Lean-exclusive `attempt < WorkSpec.maxAttempts`; this is
not inferred from comparing two max fields.

The aggregated PostgreSQL/API E2E asserts the immediate failed-attempt state,
separate re-arm, retry current-readiness checks, all three timeout provenance
shapes, terminal race, late attempt rejection, replay/equivocation, atomic
WorkOutcome consistency and restart-readable append-only history
(`apps/server/tests/agents.rs:1230`).

## ToolOutput producer/consumer separation

The producer is the governed-agent principal owning the ToolCall; the consumer
context principal belongs to the later model invocation; the runner identity
and device decrypt the ciphertext. They are not collapsed. Producer
run/work/claim/attempt provenance must be exact and the claim must have been
valid at `requestedAt`. Consumer run/work may be later and different: access
depends on exact call descriptor, trusted `ToolSecuritySemantics.outputReadableBy`,
current recipient device/key and exact ciphertext/output commitment, not on
`ResourceOperation.read` over the producer run scope. Recording a succeeded
output therefore does not itself authorize later consumption.

The v1 concrete audience is owner-only and one current runner device. That is
an implementation limit, not the formal meaning of principal-level audience.
Project owner/admin may read audit metadata but does not automatically receive
the output envelope.

## Trust, history and retention

Catalog, witness, dispatch, request, observation, envelope, WorkOutcome and
audit mutations use project-scoped RLS and private writer patterns with pinned
`pg_catalog` search paths, row-security fencing and revoked PUBLIC execution.
The normal app role cannot directly forge permissions or verified history.
Audit has a monotone semantic position; replay converges and equivocation
rolls back. Retention may purge ciphertext and device envelopes but preserves
structural commitments/provenance needed for prefix-append-only history
(`db/migrations/0033_agent_external_tool_runtime.sql:1833`).

The backend has no web/mail/Telegram/filesystem execution client and receives
no connector credential, private endpoint, path or plaintext. The browser is a
control plane. The present native transport is a development/local-edge
refinement, not a packaged production companion.

## Regression and migration evidence

- Static migration validation: **33/33 PASS**.
- Fresh disposable PostgreSQL 1→33: **PASS**.
- Populated 0032→0033: **PASS**; existing projects/agents/runs/transitions were
  preserved and all new tool-history/snapshot/witness tables remained empty.
  No historic authority witness or R540/R541 record was synthesized.
- `verify_schema.sql`: **PASS**.
- `verify_behavior.sql`: **PASS**, including restricted-role trusted writer,
  direct DML rejection, forged identity/project rejection and version-exact
  grant/revoke.
- DB-enabled ignored tests, serial: **28 passed, 0 failed**.
- Rust workspace/all-targets: **224 passed, 0 failed, 28 ignored**.
- Aggregated tool DB/API E2E rerun after final refactor: **1 passed, 0 failed,
  18 filtered**.
- Frontend full suite: **278 passed, 0 failed, 6 skipped**; 48 files passed,
  one file skipped.
- Controlled development local-edge feature tests: **2 passed**.
- Frontend lint/build: **PASS**.
- Rustfmt and Clippy `-D warnings`: **PASS**.
- Cargo deny, cargo audit and npm audit: **PASS**; zero known vulnerabilities
  from both audit tools.
- WASM parity: **4 passed**; byte-for-byte reproducibility: **PASS**.
- Lean 4.30.0 full-file compile from a byte-identical copy: **PASS**; source
  hash unchanged.

No 0028–0032 conformance test was removed or weakened. Strong interrogation
continues to have no route that creates ToolCall/dispatch/request/observation/
audit/WorkOutcome; UserProxy output remains a candidate plan and cannot invoke
a tool without the ordinary deterministic tool authorization path.

## Residuals

- Complete R540 trace projection and R5.41 nonempty tool surface: **FAIL-CLOSED / NOT IMPLEMENTED**.
- Concrete comment surface: **FAIL-CLOSED / NOT IMPLEMENTED**.
- Human-delegation tool authority origin: **FAIL-CLOSED / NOT IMPLEMENTED**.
- Production packaged native edge companion: **PARTIAL / EXTERNAL TCB**.
- `document.local.edit`: **PARTIAL / EXTERNAL TCB**.
- Mail/Telegram receive: **CONTRACT TESTED**, not live.
- Mail/Telegram send and other external disclosure: **FAIL-CLOSED**.
- Least-privileged production DB-role provisioning: **PARTIAL deployment hardening residual**.
