# Sprout R5 checkpoint 0034 — exact external-tool trace-cluster evidence

Date: 2026-08-24

Baseline: `defee4aedb5f0065ab6dea454e1ad1316fffe0f2`

Canonical Lean SHA-256: `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`

## Claim boundary

0034 adds an independent, append-only projection of the external-tool cluster:
WorkAttempt, ToolEvent and terminal WorkOutcome. It does not reinterpret the
0033 operational audit as a formal trace and it does not claim a full
`R540ConcreteTraceCertificate` or `R541FormalReleaseCertificate`.

The supported claim is an **R540 exact tool-cluster projection/certificate**
for 0034-native runs, exact at the concrete typed-commitment boundary. The
tool/outcome R5.41 inventories are list-exact and may become nonempty only when
all corresponding concrete records pass the independent joins. Blocker,
causal, evidence, disclosure, model and interrogation trace clusters are not
part of this certificate.

## Formal mapping and classification

| Formal surface | Classification | Concrete mapping and limit |
| --- | --- | --- |
| `R540ConcreteExecutionTrace.id : Nat` | **CONCRETELY REFINED for 0034-native run identity** | `trace_number bigint > 0` is a server-owned identity created during exact run initialization, never chosen by a model or edge (`db/migrations/0034_agent_tool_trace_projection.sql:98`, `:411`). |
| `R540WorkAttemptEventExact` | **CONCRETELY REFINED for projected external-tool attempts** | Immutable work/claim snapshots prove run, goal, work, claim, attempt, actor and `acquiredAt <= requestedAt < expiresAt`; the event tick is the same server semantic tick as the tool request (`db/migrations/0034_agent_tool_trace_projection.sql:114`, `:458`, `:852`). |
| `R540ToolEventExact` | **CONCRETELY REFINED for coordinates/commitments; PARTIAL / EXTERNAL TCB for typed plaintext fidelity** | Pending and terminal rows retain exact call/tool/version/input commitment/status/output commitment and share the WorkAttempt coordinate (`db/migrations/0034_agent_tool_trace_projection.sql:139`, `:883`). The server does not see plaintext ToolInput/ToolOutput. |
| `R540WorkOutcomeEventExact` | **CONCRETELY REFINED for terminal projected attempts** | The immutable outcome joins the terminal ToolEvent, operational outcome and exact terminal transition snapshot (`db/migrations/0034_agent_tool_trace_projection.sql:197`, `:958`). |
| `R541SurfaceGate` tool/outcome lists | **CONCRETELY REFINED for the tool cluster** | Enabled means exact nonempty ordered list; disabledFailClosed means empty. Certificate JSON lists must equal the independently reconstructed exact lists, not merely their counts (`db/migrations/0034_agent_tool_trace_projection.sql:269`, `:808`, `:1009`). |
| Full `R541TraceFeatureGateCertificate` | **PARTIAL / NOT IMPLEMENTED** | Only tool and outcome inventories are projected here. Empty blocker/causal/evidence/disclosure modes are reported as fail-closed product inventory, not as a full release certificate over unprojected trace lists. |
| Legacy 0033 run promotion | **FAIL-CLOSED** | No semantic tick, root, trace number, event or certificate is backfilled. A legacy run remains unprojected even if it later contains operational tool history. |

## Trace identity and version semantics

The Lean trace is explicitly one release/run trace: all event lists share one
`traceId`. The concrete mapping is:

```text
Lean traceId : Nat
↔ DB trace_number : bigint, trace_number > 0
↔ exactly one (project_id, run_id, goal_id, initialization_transition_id)
```

The root is created in the same transaction as a new 0034-native run's exact
`initialized` transition. `start_tick` equals that transition's server semantic
tick (`apps/server/src/routes/agent_runs.rs:326`,
`db/migrations/0034_agent_tool_trace_projection.sql:411`). A version-1
certificate exists before any tool event and has disabled/empty tool and
outcome gates. The E2E reads this root before the first invoke
(`apps/server/tests/agents.rs:1325`).

Every later prefix is a new immutable certificate version. One total ordinal
space is unique on `(trace_number, ordinal)` and each position binds exactly
one typed event ID plus event hash. Filtered WorkAttempt, ToolEvent and
WorkOutcome lists preserve that total order. Each certificate commits the
three full ordered lists, the last ordinal, end tick, modes and previous
certificate hash (`db/migrations/0034_agent_tool_trace_projection.sql:231`,
`:269`, `:331`). Replay with an unchanged inventory is a no-op; semantic equivocation
cannot create a second binding at the same unique coordinate.

## Event exactness

### WorkAttempt

Invoke/retry first persist the unchanged semantic observation transition
`tool_attempt_opened`, then the private projector copies the exact work and
claim objects from that append-only snapshot. `tool_attempt_opened` is a
concrete observation transition, not a new Lean SemanticEvent. The projector
requires the call owner, transition actor and work claimant to coincide and
requires the call's real `requested_tick` to lie inside the historical claim
lease (`apps/server/src/routes/agent_tools.rs:851`,
`db/migrations/0034_agent_tool_trace_projection.sql:458`).

Tool attempt and WorkAttempt are the same coordinate. Retry keeps the same call
ID, tool and canonical input but obtains exact claim/attempt N+1. The failed
WorkOutcome N remains linked to the failed transition N; `tool_retry_rearmed`
is a later transition and never substitutes for the terminal snapshot.

### ToolEvent and terminal origins

The pending row captures the exact call state at `requestedAt`. The terminal
row is created atomically after the 0033 terminal writer has closed ToolCall,
WorkItem, claim and WorkOutcome. Signed edge and server timeout use separate
private wrappers (`db/migrations/0034_agent_tool_trace_projection.sql:764`,
`:776`). The signed branch requires real dispatch/request/signer provenance.
The server-timeout branch is unsigned and preserves exactly one of the three
real shapes: no dispatch, dispatch without request, or dispatch with request.
It never fabricates a dispatch, request, wire commitment or signature.

Terminal processing retains 0033 semantics: it does not recheck current tool
permission, runtime witness, claim activity or WorkSpec activation. Those were
required at `requestedAt`; retry is a new action and rechecks all current gates.
Permission/runtime revocation or claim expiry after request therefore does not
erase an already-caused exact result.

### WorkOutcome

The outcome view requires exact project/run/goal/work/claim/attempt, the same
terminal observation, the same semantic tick, and exact equality between the
stored transition state and outcome state. `failed` attempt N associated with
an `eligible`/rearmed snapshot is excluded. This preserves the observable
failure followed by a distinct retry-rearm transition.

## ToolInput and ToolOutput representation

The backend remains plaintext-blind. The concrete projection distinguishes:

1. canonical typed ToolInput commitment;
2. signed edge input statement binding that commitment to call/tool/version and
   exact WorkAttempt;
3. encrypted-input ciphertext commitment;
4. post-serialization external wire-request commitment;
5. hiding execution-profile commitment;
6. canonical typed ToolOutput commitment for success;
7. signed terminal statement binding it to the exact call/attempt;
8. encrypted-output ciphertext commitment.

Thus “exact” means exact equality of concrete typed commitment identities and
their signed provenance, not server verification of hidden plaintext bytes.
Collision resistance and edge fidelity of typed value → commitment remain an
**EXTERNAL TCB ASSUMPTION**. No plaintext-exactness claim is made.

## ToolOutput producer, effect actor, reader and runner

`ToolOutput(callId)` remains a non-resource source; no
`ResourceOperation.read` over the producer run scope is introduced. Four roles
are explicit:

- producer owner: governed-agent principal owning the ToolCall;
- effect actor: must equal the call owner, as required by
  `toolContextSourceOwned`;
- reader principal: independently checked against trusted
  `ToolSecuritySemantics.outputReadableBy`;
- runner identity/device: independently checked against dispatch and envelope
  sender provenance.

Reader device/key is resolved separately from the runner and the recipient
envelope must match it. The current manifest policy remains owner-only, so the
roles are semantically separate even when producer/effect actor/reader are the
same principal in v1 (`apps/server/src/routes/agents.rs:7415`, `:7435`,
`:7542`). A
consumer WorkBinding may belong to a later run/work; producer and consumer
coordinates are intentionally not equated. The consumer model trace relation
is not folded into this tool-cluster certificate.

## Gate exactness and corruption behavior

The gate is driven by the latest certificate only when:

- stored lists equal the complete independent inventory lists;
- commitments recompute from those lists;
- certificate hash and previous-version link recompute;
- every listed event survives the exact WorkAttempt/ToolEvent/WorkOutcome joins;
- the exact filtered inventories equal the certified inventories.

The surface records are selected from those exact rows. Consequently, an
operational audit row, terminal call, outcome, transition or trace number alone
cannot enable a surface. The adversarial E2E corrupts one terminal owner under
a rollback-only DB-owner probe and observes `disabled_fail_closed`, empty
records, and zero surface rows (`apps/server/tests/agents.rs:2637`).

## Legacy, retention and trust boundary

Migration 0034 is additive; migration 0033 is unchanged. The populated upgrade
contains real 0033-native tool failures/retries, a succeeded encrypted
ToolOutput, and a native-only run. It preserves every operational row and
creates zero roots/events/certificates. Only new post-upgrade runs can obtain a
root and certified records.

Structural projection history is append-only. Ciphertext/key-envelope purge
does not delete structural event IDs or commitments. If any structurally
required provenance is absent/corrupted, recomputation disables the gate rather
than serving a stale cached certificate.

All projection tables have RLS plus FORCE RLS; updates/deletes are rejected;
projectors are `SECURITY DEFINER`, use `search_path=pg_catalog`, disable row
security only internally, and have PUBLIC execution revoked
(`db/migrations/0034_agent_tool_trace_projection.sql:1049`, `:1094`). The
restricted app-role test rejects direct DML and private projector calls. DB
owner/superuser remains in the TCB. Dev/CI still uses a bootstrap/superuser
`DATABASE_URL`; least-privileged production role provisioning is a **PARTIAL
/ production deployment hardening residual**.

## Preserved boundaries

- Task, TaskList, Topic, Info and Comment remain native surfaces, not tools.
- Concrete Comment remains **FAIL-CLOSED / NOT IMPLEMENTED**.
- `mail.send` and `telegram.send` remain **FAIL-CLOSED**.
- `document.local.edit` remains **CONTRACT TESTED / PARTIAL / EXTERNAL TCB**.
- Backend Sprout executes no connector and sees no connector secret, private
  endpoint, local path or tool plaintext.
- Human-delegation authority origin without the complete certificate remains
  fail-closed.
- The production native edge companion remains a residual; 0034 does not
  expand the executable tool inventory.

## Classification summary

### FORMALLY SPECIFIED

The normative source specifies the run-level `traceId`, exact WorkAttempt,
ToolEvent and WorkOutcome coordinates, append-only trace lists, and the R5.41
list-exact feature gates. It also keeps ToolOutput authorization outside the
ordinary resource-read requirement.

### CONCRETELY REFINED

For 0034-native runs, Sprout persists the run-level root, server semantic tick,
one gap-free total inventory, its three ordered filtered inventories, immutable
per-attempt projections, and append-only hash-chained prefix certificates.
Tool and outcome surface records are exposed only by the independently
recomputed exact certificate views.

### LIVE FEATURE TESTED

The PostgreSQL/server E2E exercises invoke, signed terminal, all three server
timeout provenance shapes, failure followed by distinct retry re-arm, retry,
terminal-after-revocation, ToolOutput producer/consumer separation, corruption
fail-closed behavior, replay, retention, restart and restricted-role attacks.
No external connector or AI-provider live test is part of 0034.

### CONTRACT TESTED

The domain certificate validator proves positive trace numbers, gap-free total
order, list-exact filtered projection, non-vacuous enabled gates, exact empty
disabled gates, and a strict monotone prefix/hash-chain successor.

### PARTIAL / EXTERNAL TCB

Typed ToolInput/ToolOutput value-to-commitment fidelity and collision
resistance remain an edge/cryptographic TCB assumption. The native companion,
possible human-delegation provenance and production least-privileged DB-role
provisioning remain residuals. `document.local.edit` remains contract-only and
an external-side-effect TCB.

### FAIL-CLOSED / NOT IMPLEMENTED

Full non-tool R540 clusters and the full `R541FormalReleaseCertificate` are not
claimed. Legacy/incomplete traces, Comment, mail/Telegram send, unsupported
human delegation and any missing/corrupt exact conjunct remain fail-closed.

## Verification evidence

- Static migration validation: **34/34 PASS**.
- Fresh disposable PostgreSQL 1→34: **PASS** on
  `sprout_r5_0034_fresh_final_20260824_b`; all 34 migrations were applied.
- `verify_schema.sql`: **PASS**, with the original global `*_id ⇒ UUID`
  invariant and no 0034 exception.
- Adversarial schema probe: a public `invalid_trace_id bigint` is detected by
  the global validator predicate.
- `verify_behavior.sql`: **PASS**.
- Populated 0033→0034: **PASS** on
  `sprout_r5_0034_upgrade_20260824_c`. The following PRE/POST counts were
  identical:

  | Preserved 0033 table | PRE | POST |
  | --- | ---: | ---: |
  | projects | 2 | 2 |
  | project_memberships | 4 | 4 |
  | governed_agents | 2 | 2 |
  | agent_local_goal_contracts | 2 | 2 |
  | agent_collaborative_runs | 3 | 3 |
  | agent_run_transitions | 16 | 16 |
  | agent_tool_calls | 2 | 2 |
  | agent_tool_attempt_dispatches | 4 | 4 |
  | agent_tool_attempt_requests | 3 | 3 |
  | agent_tool_attempt_observations | 5 | 5 |
  | agent_run_external_tool_work_outcomes | 5 | 5 |
  | agent_tool_audit | 10 | 10 |
  | agent_tool_output_key_envelopes | 1 | 1 |

  Logical row hashes were identical. The only physical row-shape addition was
  the nullable 0034 `semantic_tick`; every legacy value remained `NULL`, and
  the hash excluding that new column matched exactly. Synthetic root,
  WorkAttempt, ToolEvent, WorkOutcome, inventory, certificate, tool-surface and
  outcome-surface counts were all **0**. Both schema and behavior validators
  passed after upgrade.
- Final targeted PostgreSQL/server trace E2E: **1 passed, 0 failed, 18
  filtered** on `sprout_r5_0034_targeted_final_20260824_e`. Before its
  retention phase, its two 0034-native traces contained 5 WorkAttempt events,
  10 ToolEvents, 5 WorkOutcomes, 20 total inventory rows, 12 certificate
  versions, 10 enabled tool-surface records and 5 enabled outcome-surface
  records. After authorized purge the structural counts and ordered hashes
  were unchanged, encrypted input/output/envelope counts changed from 1/1/1
  to 0/0/0, and both served surface counts became 0 because the exact views
  fail closed when their purgeable operational provenance is absent. A
  consumer requiring the removed envelope was rejected.
- Restricted application-role and projector adversarial cases are included in
  that E2E: direct insert/update/delete, private projector execution, forged
  identity/project GUCs and cross-project arguments were rejected, with the
  structural hash unchanged after every attempt. The trusted route path
  succeeded.
- Targeted R540 domain tests: **3 passed, 0 failed, 91 filtered**.
- PostgreSQL ignored serial matrix: **28 passed, 0 failed**.
- Rust workspace/all-targets: **234 passed, 0 failed, 28 ignored**.
- Frontend: **48 files passed, 1 skipped; 278 tests passed, 0 failed, 6
  skipped**. The controlled loopback local-edge test passed in the environment
  that permits loopback listen. Lint and production build passed.
- Rustfmt, workspace check and Clippy `-D warnings`: **PASS**.
- Cargo deny, cargo audit and npm audit: **PASS**; audit tools reported zero
  known vulnerabilities.
- WASM parity: **4 passed**; byte-for-byte reproducibility: **PASS**.
- Lean 4.30.0 full-file compile from a byte-identical temporary copy:
  **PASS**. Source/copy comparison passed and the final source SHA-256 remained
  `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`.

No previous conformance test was removed, newly ignored or weakened. Lean
remains byte-identical at the canonical hash.
