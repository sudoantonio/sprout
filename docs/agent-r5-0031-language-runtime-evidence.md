# Checkpoint 0031 — state-grounded language runtime evidence

Date: 2026-08-21

## Chain of custody and scope

- starting HEAD: `042b3abf7a177f87bb7fdb4f44be083a58d853d3`;
- canonical specification: `Sprout_AgentSpec_R5_no_model_memory_draft.lean`;
- source SHA-256 before and after implementation:
  `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`;
- normative anchors: `StructuredLanguageModelRuntimeBoundary` (Lean line
  14281), `StateGroundedModelInvocationCertificate` (15199),
  `StateGroundedStrongInterrogationCertificate` (15245),
  `R540ModelInvocationEvent` (15464), `R540ModelRuntimeProjection` (15695),
  `R540ModelInvocationEventExact` (15721), and `R541SurfaceGate` (15927);
- Lean and frontend source were not modified. The WASM gate rebuilt the
  existing generated bundle twice and produced byte-identical tracked output.

This checkpoint does not add a production text-generation provider. It
implements and tests the provider-neutral contract with an authorized endpoint
runner fixture. Governance compilers 0029/0030 remain deterministic and do not
depend on this runtime.

## Concrete runtime projection

The product uses three distinct witnesses rather than projecting a request from
itself:

1. The server constructs `ModelInvocationContext` from current product state
   and persists an immutable `agent_model_attempt_dispatches` record containing
   the exact ordered source descriptors and request/context/exposure/transport
   commitments (`agents.rs:3846`, route `agents.rs:5170,5263,5557`).
2. The authorized endpoint TCB returns an independently signed
   `ModelRuntimeActualObservation`. Both Ed25519 and ML-DSA signatures cover the
   same exact observation, including the actual request, exposure and output
   commitments (`agents.rs:3879`, route `agents.rs:5746,6018`).
3. The server checks actual versus `R540ModelRuntimeProjection`, current
   runner/device/signing-key validity, attempt and strict lease fence before it
   persists observation and projection (`agents.rs:3892`, route
   `agents.rs:6028,6149,6192`).

The PostgreSQL witnesses are introduced by migration 0031 at lines 60, 87 and
136. They are append-only and retention-aware. Exact replay is idempotent;
reuse of the same invocation/attempt/idempotency identity with different
commitments conflicts.

When an invocation belongs to collaborative work, run, goal, canonical
WorkItem, claim and attempt must all exist and match the current authoritative
claim. The runner cannot supply permission facts, authority decisions or an
unbounded identifier namespace.

## E2EE and endpoint TCB

Plaintext input, source bodies and output remain on the authorized edge device.
The server persists ciphertext, commitments, typed structural metadata and
source references. It validates current readability before dispatch; a source
that was once readable but has been revoked is not admitted to a later context.

The endpoint signs what was actually sent to and received from the provider.
This is an explicit endpoint-TCB assumption: the server can prove binding,
bounds and authorization, but cannot prove semantic fidelity of plaintext it
cannot read, cannot prove which future provider executable ran, and cannot
attest that a future provider has no internal storage.

## Structured-language behavior

The runtime enforces a closed output schema, grounded identifiers, positive
attempt/output/depth bounds, finite retry, and either a validated typed artifact
or explicit failure. A malformed or ungrounded artifact is never accepted as an
authoritative fallback. Explicit failure is immutable runtime history but does
not witness a successful R5.40 model invocation.

| Task kind | Concrete 0031 status | Evidence and boundary |
| --- | --- | --- |
| `answerFromAuthorizedContext` | **CONCRETELY REFINED for human-controller→agent** | Exact state-grounded context, endpoint observation, ordered-list-exact answer sources, append-only answer and strong causal read-only certifier are exercised E2E. Other user/admin creators and agent→agent remain absent. |
| `interpretProxyRequest` | **CONCRETELY REFINED through plan validation/authorization** | Model-generated plan is exact-bound to the UserProxy request and selected only within candidate resources/operations/tools. Existing Responsibility, permission and one-shot-confirmation gates remain authoritative. Full effect execution remains partial. |
| `summarizeGovernanceDecision` | **FAIL-CLOSED / typed boundary only** | The typed artifact and schema check exist, but no production/E2E language adapter is enabled. Governance facts/reason remain authoritative in 0030. |
| `rewritePrompt` | **FAIL-CLOSED / NOT YET IMPLEMENTED** | No automatic activation, exception or Responsibility mutation was added. |
| governance compiler task kinds | **existing deterministic endpoint TCB** | 0029/0030 compiler certificates are unchanged and are not routed through a model provider. |
| global synthesis and other language kinds | **unchanged / partial or fail-closed** | No provider, global materializer, or semantic oracle was added. |

## Exact interrogation and strong read-only matrix

`persist_interrogation_answer` requires
`context_sources == actual_sources` as an ordered list and rejects duplicates
(`apps/server/src/routes/agents.rs:7395`). Equal sets in a different order,
missing sources, extra sources and duplicates all fail. The old whole-project
quiet-period fingerprint is not an authorization condition: an unrelated
authorized project mutation between question and answer is accepted.

The final certifier is `sprout_private.interrogation_invocation_is_read_only`
(`0031_agent_language_runtime_projection.sql:618`; Rust call at
`agents.rs:7482`). Its `SECURITY DEFINER` environment uses
`search_path=pg_catalog`, fully qualified product relations, `row_security=off`
only after an in-function current-party check, and no PUBLIC/app-role EXECUTE.

| Lean category | Authoritative product record | Exact causal binding / runtime writer | Status and proof strategy |
| --- | --- | --- | --- |
| `resource_effect` | `agent_effect_proposals` | `(project_id, invocation_id, record_id)` FK plus same-transaction `agent_language_causal_mutations`; the generic effect writer inserts both | **CONCRETELY GROUNDED.** The interrogation route rejects a non-empty effect proposal before projection/answer persistence. A real effect plus exact edge makes the certifier false. A missing effect ID cannot be inserted. |
| `tool_invocation` | Existing tool/runtime paths are outside the interrogation submit call graph | No language/interrogation writer; closed request/artifact schema has no tool-call field | **STRUCTURALLY UNREACHABLE FROM INTERROGATION.** The DB causal extension rejects this unsupported category; this is defense in depth, not a synthetic product witness. |
| `prompt_revision` | Governance prompt-revision ledger | Interrogation submit never invokes governance writers and accepts no prompt-revision field | **STRUCTURALLY UNREACHABLE FROM INTERROGATION.** Schema-closed API negatives and call-graph separation prove the current path. |
| `local_goal_revision` | `agent_local_goal_contracts` and governance revision ledger | No call from interrogation submit to compiler/activation writers | **STRUCTURALLY UNREACHABLE FROM INTERROGATION.** An invocation ID is not authority for governance activation. |
| `created_work` | Collaborative run transition/snapshot and canonical work slots | No WorkItem materializer is reachable from interrogation submit | **STRUCTURALLY UNREACHABLE FROM INTERROGATION.** `agent_run_work_product_bindings` is deliberately not used: it binds an already existing WorkItem to a product effect and does not prove WorkItem creation. |
| `activated_obligation` | Collaborative run transition/snapshot | No obligation transition writer is reachable and no condition facts can be supplied by this API | **STRUCTURALLY UNREACHABLE FROM INTERROGATION.** Closed input plus subsystem call-graph separation is the proof. |
| `assigned_task` | Task assignment subsystem and task event history | No assignment writer is called; the language artifact cannot carry an assignment command | **STRUCTURALLY UNREACHABLE FROM INTERROGATION.** Existing task APIs require their own authority/provenance and do not accept an interrogation invocation as a certificate. |

`agent_language_causal_mutations` is only an exact cross-subsystem edge. It
does not replace the product record. Its trigger currently accepts only
`resource_effect`, for which the composite FK requires the real effect in the
same project and invocation. The other six enum values reserve no production
capability: their insertion is rejected until a future authoritative subsystem
writer and grounding rule are introduced.

The E2E retains all seven domain delta negatives and the closed-schema HTTP
negatives. The latter demonstrate that clients cannot request those effects;
they are not presented as standalone proof that a product mutation exists.

## R5.41 surface inventory

The views at migration lines 665–725 distinguish successful certified records
from generic runtime history:

- `model` is enabled only by a succeeded projection with matching dispatch,
  signed actual observation and exact commitments;
- `interrogation` is enabled only by an answered session exact-bound to that
  succeeded model projection and the read-only certificate;
- `proxy` is enabled only by a model-mediated plan exact-bound to a succeeded
  `interpretProxyRequest` projection;
- `comment` and `disclosure` remain `disabledFailClosed` with zero records.

An explicit failure alone leaves model/interrogation/proxy disabled. Failure
plus success enables only the certifiable success record. Legacy non-model
UserProxy planning does not enable the model-mediated proxy surface. No
synthetic comment or disclosure trace/tick record is generated.

## No model memory and authority frame

Sprout adds no model-memory table, embeddings store, hidden provider thread or
provider conversation ID. Every invocation reconstructs its source list from
current authorized product state. Transcripts remain ordinary product state
and enter context only as explicit source references.

Invocation, failure or answer alone do not alter permissions, tool grants,
Responsibility, controller, agent availability, key envelopes or project
membership. UserProxy mutations, task effects and governance transitions must
continue through their existing deterministic authorization subsystems.

## PostgreSQL trust boundary and retention

The Rust server/endpoint verifier is part of the TCB; PostgreSQL is not claimed
to verify endpoint plaintext semantics. PostgreSQL nevertheless prevents
untrusted fabrication:

- new history is append-only;
- PUBLIC and the tested `NOSUPERUSER NOBYPASSRLS` app role cannot write causal
  history, read cross-project surface inventory, or execute private certifiers;
- private retention/certifier functions use trusted owner, fixed
  `pg_catalog` search path, qualified objects and exact subject checks;
- temp/public shadowing and forged identity GUCs do not grant access;
- interrogation, proxy and effect retention remove dependent runtime records in
  explicit order without weakening earlier historical retention invariants.

Migration upgrade was tested on a populated schema stopped at 0030. A legacy
invocation retained its ID/status, was classified `generic`, received the
compatible context principal backfill, and did not acquire invented dispatch,
observation, projection or answer witnesses. Fresh installation applied all
31 migrations.

## Verification matrix

| Gate | Observed result |
| --- | --- |
| migration static validation / fresh 1→31 | **PASS** — 31 files validated and 31 applied |
| populated 0030→0031 upgrade | **PASS** — legacy identity preserved; 0 invented 0031 certificate rows |
| `verify_schema.sql` | **PASS** — `sprout schema verification passed` |
| `verify_behavior.sql` | **PASS** — `sprout behavioral verification passed` |
| targeted 0031 E2E | **PASS** — 1 passed, 0 failed, 17 filtered |
| targeted DB trust-boundary test | **PASS** — 1 passed, 0 failed, 17 filtered |
| all ignored DB-enabled workspace tests | **PASS** — 27 passed, 0 failed, 0 ignored, 212 filtered |
| ordinary `cargo test --workspace --all-targets` | **PASS** — 212 passed, 0 failed, 27 ignored |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** |
| `cargo deny check advisories licenses bans sources` | **PASS** — allowed duplicate-version diagnostics only |
| `cargo audit --deny warnings` | **PASS** — no vulnerable dependency reported |
| WASM parity | **PASS** — 1 file, 4 tests |
| WASM reproducibility | **PASS** — two release builds byte-for-byte identical |
| Lean compile | **PASS** — Lean 4.30.0, complete byte-identical copy, no diagnostics |

The principal PostgreSQL E2E is
`edge_runner_is_a_revocable_device_and_cannot_bypass_governance`
(`apps/server/tests/agents.rs:3170`). The DB adversarial test is
`governance_verified_history_rejects_app_role_dml_and_shadowing` (line 5690).
The domain seven-category test is `interrogation_is_strictly_read_only`
(`crates/domain/src/agents.rs:5109`). All earlier 0028/0029/0030 DB tests remain
present and passed in the same serial run.

## Residual boundaries

- no production provider is configured;
- provider semantic fidelity, provider-side retention and executable identity
  are external TCB assumptions;
- interrogation agent→agent and non-controller human/admin paths are not yet
  concretely refined;
- complete UserProxy effect execution remains partial;
- governance summary is typed but not E2E-enabled;
- comment and disclosure R5.41 projections remain disabled/fail-closed;
- global synthesis/materialization, global liveness and full R5 concrete
  refinement are not claimed.

## TEXT GENERATION API BOUNDARY READY

The boundary is ready for separately authorized integration of real generation
for `answerFromAuthorizedContext` and `interpretProxyRequest`.

- plaintext and decrypted source bodies would leave only the authorized edge
  runner, never the Sprout server;
- the provider request must be built from the exact persisted dispatch source
  list and commitments;
- output must match the closed `InterrogationAnswer` or `UserProxyPlan` schema
  and use only grounded candidate identifiers;
- retry is bounded by `maxAttempts`; timeout/error ends as explicit failure;
- Sprout persists ciphertext, commitments, source references, attempt status,
  signed actual observation and exact projection, not hidden provider memory;
- the conformance tests above protect exposure exactness, revocation,
  cross-trace substitution, bounded failure, no-authority, interrogation
  read-only behavior, surface non-vacuity, RLS and retention.

A real provider must not be added without separate authorization and a review
of its plaintext transport, retention and internal-state assumptions.
