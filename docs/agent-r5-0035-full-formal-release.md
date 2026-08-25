# Sprout R5 checkpoint 0035 — governed Comment and formal release closure

Date: 2026-08-25

Baseline branch: `codex/lean-concrete-refinement`

Starting commit: `5612d19f7adc2407dfd56eccc1d09ff60fffe1f1`

Canonical Lean SHA-256: `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`

## Claim boundary

0035 closes the internally reconstructible `R540ConcreteTraceCertificate` and
the 28-field `R541FormalReleaseCertificate` for a 0035-native completed run.
The served root is not a collection of booleans: each child is type-tagged,
independently reconstructed from authoritative operational rows, list-exact
where the Lean contains a list, and absent when any nested predicate fails.

`R541ExternalReleaseAssumptions` remains a separate boundary. Consequently,
the derived theorem result is claimed only when those assumptions are supplied;
in particular eventual completion is not asserted by the internal root alone.

Comment is a native `AgentAction.postComment`/`ResourceOperation` surface. It
is not an external tool, has no tool catalog alias, and does not create
authority. The frontend was not changed in this checkpoint.

## Architecture

### Two clocks and the canonical run timeline

Formal events use one server-owned `Nat` timeline per run/trace. The database
mapping is `Lean traceId : Nat` ↔ `trace_number bigint > 0`. Allocation is
serialized, gap-free, hash-chained, rollback-safe, and occurs in the same
transaction as its authoritative event. Replay consumes no tick and
equivocation leaves no allocation.

Operational time remains `timestamptz`: claim leases, `requested_at`, dispatch,
wire request, observation, recovery, and wall timeout use the real clock. No
0035-native formal tick is derived from epoch time and no logical tick is
converted into an operational timestamp.

For every pending tool attempt, allocation checks the semantic deadline before
another formal event may advance the run. A server timeout wins the required
slot when necessary, while the independent wall-clock deadline remains active.
Thus both `terminalTick <= requestedTick + timeoutTicks` and the operational
deadline hold under dense unrelated events and retry.

### Authoritative-source pipeline

```text
operational ledgers + canonical transition snapshots
    -> typed exact event/child views
    -> one ordered run inventory and exact child inventories
    -> versioned, prefix-monotone, hash-chained certificates
    -> independently reconstructed 28-field formal release root
```

Hashes protect integrity but never replace typed equality. Missing ciphertext,
an absent source, wrong coordinates, an ordinal gap, substitution, duplicate,
or stale/cross-trace join removes the exact view and closes the served gate.

## FORMALLY SPECIFIED and CONCRETELY REFINED

### Full R540 event families

| Lean family | Authoritative concrete reconstruction |
| --- | --- |
| WorkAttempt | Canonical `work_claimed` transition, exact WorkItem/claim snapshot, actor and semantic tick. `workAttempts` is nonempty. |
| WorkOutcome | Exact terminal transition and state snapshot; tool/generic sources are deduplicated by the formal coordinate. |
| BlockerResolution | Canonical resolution row plus terminal blocker in the same semantic state. For `taskFromWork`, the exact WorkSpec/obligation and canonical Task→Work causal edge are required, and the human task is terminal at the resolution tick. |
| CausalLink | Canonical causal ledger, typed predecessor/successor, same run/goal, observed tick no later than recorded tick, and decreasing causal rank. |
| ToolEvent | The immutable 0034 per-attempt projection, same WorkAttempt coordinate, exact status/origin/commitments and real optional dispatch/request provenance. |
| Evidence | Exact acceptance transition, historical claim selected by work/attempt rather than “latest”, typed subject, obligation/rule/kind and mechanical or persisted judgment witness. |
| Disclosure | Exact supported Lean sink, work/attempt/actor/context, full encrypted payload from the immutable source, and semantic tick. Unsupported mail/Telegram/HTTP sinks are not promoted. |
| ModelInvocation | Exact WorkAttempt/principal/context, full encrypted input/output payloads, direct sources and `hiddenPersistentModelMemoryAvailable=false`. |
| Interrogation | Exact session/question/answer/context/projection and seven-category empty strong-read-only delta; no invented formal work/run fields inside the Lean record. |

The total inventory is ordinal, gap-free and append-only. The outcome, blocker,
causal, tool, evidence and disclosure gates compare their complete ordered
record lists with the independently reconstructed trace lists. Enabled means a
nonempty exact list; disabled-fail-closed means exactly `[]`.

### Native Comment

The canonical ledger stores a server-owned Comment ID and author, exact agent
recipient, target, optional parent, server-derived depth, opaque encrypted
payload, key epoch, semantic tick, request hash and retention marker. User and
administrator comments have depth 0. An agent root has depth 1 and is unique
for author/recipient/target; an agent reply requires the same target, a parent
addressed to the posting agent, and `parent.depth + 1` within policy.

Human/administrator posting is a native path distinct from the governed agent
action path. The latter requires the exact run/goal/work/claim/attempt,
`postComment` action and security policy, current authority and permission.
The writer rejects server-owned body fields, self-comment, non-agent recipient,
cross-project/target parent, stale epoch, generic agent-route bypass and
idempotency equivocation.

`agent_native_comment_run_semantic_states` reconstructs the exact Comment
prefix in `semanticState(tick).base.comments`; the project ledger is only an
independent cross-check. Comment records contain the complete typed encrypted
payload, not only its commitment. The certificate proves the start prefix is a
prefix of the end prefix and the ordered gate is list-exact.

One Comment ID maps to one canonical `commentPosted` event/tick and at most one
run notification. Priority is temporal response discipline scoped to a
recipient (`administrator > user > agent`), not query/UI ordering.

Comment is an information source/sink only. Posting requires `postComment`,
reading requires `readComment`/`commentReadable`, and disclosure enforces the
intersection of source readability with the comment audience. A Comment or
`sourceComment` never creates WorkSpec, permission, tool permission,
responsibility, authority origin or authority ceiling.

### R541FormalReleaseCertificate: 28 top-level fields

| # | Field | Exact child source and closure |
| -: | --- | --- |
| 1 | `runGoalExact` | Root, run contract and completed state agree on goal. |
| 2 | `traceStartExact` | Root start tick equals the canonical initialized transition. |
| 3 | `governedRunExact` | Active governed agent/LocalGoal/run identities and revisions agree. |
| 4 | `secureKernel` | Field-specific completion, evidence-discharge and authority-information sources; nested audit is 6/6, 3/3 and 32/32. |
| 5 | `governanceKernel` | Append-only governance history plus exact active responsibility, agent, exception, assignment, creation, prompt/local and global revision directories. |
| 6 | `concreteTrace` | Full typed R540 inventory and exact current prefix certificate. |
| 7 | `traceFeatureGates` | Six Lean trace gates equal their ordered trace lists. |
| 8 | `compilerActionExact` | Verified compiler output, requirements, bindings and contract actions agree. |
| 9 | `securityPoliciesExact` | Every WorkSpec policy and observed effect footprint/tool use is exact and bounded. |
| 10 | `governanceOperational` | Operational revision/creation/responsibility/global lists are reconstructed, not hardcoded empty. |
| 11 | `localRevisionTraceBound` | Exact local revision records share this run trace. |
| 12 | `creationTraceBound` | Exact agent creation records share this trace. |
| 13 | `responsibilityTraceBound` | Exact responsibility activations share this trace. |
| 14 | `globalTraceBound` | Exact global synthesis records share this trace. |
| 15 | `proxyTraceBound` | Exact proxy records share this trace. |
| 16 | `crossOwnerTraceBound` | Exact cross-owner records share this trace. |
| 17 | `comments` | Comment semantic-state membership, admissibility, append-only prefix and exact gate. |
| 18 | `proxy` | Current grounded candidate, user actor, permission/responsibility or one-shot confirmation; model plan is never authority. |
| 19 | `globalInventoryExact` | Ordered global inventory equals the authoritative synthesis list. |
| 20 | `global` | Base state, responsibility directory, exceptions, local goals and global gate are exact. |
| 21 | `crossOwner` | Exact classifier/directories/locals and only Lean-authorized routing branches. |
| 22 | `interrogation` | Exact transcript/runtime projection, privacy and strong read-only certificate. |
| 23 | `model` | Exact model runtime projection/context and no hidden persistent memory. |
| 24 | `taskOperational` | Operational task intents/provenance are reconstructed; empty gates are allowed only when the authoritative lists are empty. |
| 25 | `taskIntentTraceBound` | Every served task intent is exact and trace-bound. |
| 26 | `taskProvenanceTraceBound` | Every served obligation provenance record is exact and trace-bound. |
| 27 | `operationalHistory` | Four typed prefix checks: proxy transcripts, proxy audit, task provenance and task intents. |
| 28 | `operationalClosure` | Exact proxy provisioning, feasible language-task inventory and actual schema-valid-or-explicit-failure runtime boundary. |

Each field has a distinct type-tagged child certificate ID. The issuer requires
all and only the 28 canonical fields, one row per field, distinct IDs, one
trace/run/goal/start identity, and one row in every field-specific exact view.
Before the last child issuance returns NULL; exact replay returns the same
immutable root. Retention can preserve its historical descriptor while making
the currently served exact root unavailable.

### Secure kernel nested audit

The completion kernel reconstructs its base kernel, completion commit,
execution dynamics, measure laws, goal validity persistence and validity at
start (6/6). Evidence discharge independently proves discharge soundness,
accepted evidence closure and completion commit (3/3).

The authority-information child reconstructs all 32 Lean fields over the
actual certified history, including effect/work ownership and enablement,
policy/action footprints, model context, disclosure safety, canonical bodies,
tool call/footprint/authority, sponsor/run/work attenuation and both explicit
human-delegation obligations. Complete human delegation is unsupported; an
exact root therefore requires the authoritative human-delegation set to be
empty and fails closed otherwise.

## EXTERNAL RELEASE ASSUMPTIONS

These seven fields are not child certificates of the internal root:

| Assumption | Concrete boundary and failure behavior |
| --- | --- |
| `completionBoundary` | Fair external actors/conditions and successful terminal environment. Failure means no derived eventual-completion claim. |
| `promptContractFaithful` | Authorized endpoint fidelity from encrypted intent to structured contract. Server verifies structure/bounds, not plaintext meaning. |
| `requirementsFaithful` | Structured extraction semantic fidelity. Missing/invalid output fails closed. |
| `modelProjectionExact` | Actual device/provider encrypted runtime equals its independently signed projection. A digest alone is insufficient. |
| `disclosureProjectionExact` | Actual sink payload equals the typed encrypted payload projection. Missing payload removes the exact event. |
| `interrogationProjectionExact` | Actual transcript/runtime equals the typed projection. Missing answer/context fails closed. |
| `externalEvidenceAuthentic` | Authenticity of genuinely external facts or semantic judgments; internally mechanical evidence is revalidated. |

## DERIVED RELEASE GUARANTEES

The concrete mapping of `sprout_r5_41_formal_release` consumes the exact
internal root plus the seven assumptions above. It then maps field-by-field to
`R541ReleaseGuarantees`: eventual completion, authority/information safety,
nonempty concrete trace, prompt/work/action exactness, security-policy
exactness, append-only operational history and no hidden persistent model
memory. Eventual completion is deliberately not derived from the root alone.

## LIVE FEATURE TESTED

- Fresh authoritative database `sprout_r5_0035_fresh_20260825_ap`: migrations
  1→35, schema and behavior verification all PASS.
- PostgreSQL serial ignored suite: 29 passed, 0 failed.
- Full Rust workspace: 257 passed, 0 failed, 29 ignored.
- Domain: 116 passed, including blocker list substitution/reorder/duplicate/
  wrong-tick checks and exact Task→Work grounding.
- Frontend regression only: 278 passed, 0 failed, 6 skipped; lint and build PASS.
- WASM parity: 4 passed; two release builds reproduced byte-for-byte.
- Lean 4.30.0 compiled a byte-identical temporary copy; no proof placeholders
  in code and canonical hash unchanged.

The blocker positive E2E creates a real WorkAttempt, persists the Task→Work
edge before resolution, observes a terminal human task, reconstructs the full
blocker event from the resolution semantic state, and compares
`blockerGate.records` directly with `trace.blockerResolutions`. The final test
database retained one blocker-resolution event, 17 causal links, 16 work
attempts, 7 work outcomes, 10 tool events, 1 evidence event, 4 disclosures, 1
model invocation and 1 interrogation.

## Populated 0034→0035 upgrade

Database `sprout_r5_0035_upgrade_20260825_aq` was populated by detached baseline
0034 server tests before applying only migration 0035. Nineteen pre-0035 tables
were hashed from canonically ordered row JSON. Every PRE/POST count and hash was
identical. Representative counts were: 2 runs, 4 tool-trace roots, 20 tool
inventory rows, 14 tool certificates, 5 WorkAttempt rows, 10 ToolEvent rows, 5
WorkOutcome rows, 2 calls and 10 audit rows. Synthetic 0035
root/event/inventory/certificate/Comment rows: 0. Post-upgrade schema and
behavior verification passed.

## Retention and adversarial behavior

Retention preserves immutable descriptors, inventories, ordinals and hashes.
It may purge ciphertext/envelopes. When a full typed payload is no longer
reconstructible, the corresponding served records become empty, its gate is
`disabled_fail_closed`, and the served root disappears; hashes never recreate
payload. The Comment E2E proves the retained-root/no-live-run/no-ciphertext
case yields exactly one disabled empty gate and no served Comment.

Tests cover direct app-role DML/EXECUTE rejection, forged identity/project,
cross-project substitution, replay/equivocation, semantic-clock concurrency,
pending-tool deadlines, terminal/timeout races, child/hash tampering, missing
payload, wrong task/evidence/blocker/model/interrogation/disclosure data and
root composition with missing or substituted children.

## CONTRACT TESTED

The external provider/native-edge fidelity and connector boundaries inherited
from 0033/0034 remain contract or development-local-edge tested. Mail and
Telegram receive remain contract-only; send remains fail-closed.

## PARTIAL / EXTERNAL TCB

- The seven `R541ExternalReleaseAssumptions` above remain external by Lean
  design and are never stored as internal child proofs.
- Backend projections prove exact encrypted bytes and provenance, not plaintext
  semantic equality.
- Dev/CI uses a bootstrap/owner database URL. A `NOSUPERUSER NOBYPASSRLS` role
  is adversarially tested, but least-privileged production role provisioning is
  not yet encoded in the deployment repository. DB owner/superuser is TCB and
  RLS does not protect against its compromise.
- The native production companion/local-edge packaging residual from 0033
  remains; 0035 adds no frontend or connector.

## FAIL-CLOSED / NOT IMPLEMENTED

- Legacy 0034 and earlier runs receive no semantic tick, event, Comment, child
  or root backfill and cannot be promoted to an exact release.
- Possible/unsupported human-delegation provenance prevents root issuance.
- Purged/missing typed payload, an unsupported disclosure sink, unsupported
  agent-to-agent interrogation, mail/Telegram send, or incomplete operational
  provenance remains unavailable rather than approximated.

## Supply-chain and release gates

`cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny` for
advisories/licenses/bans/sources, `cargo audit --deny warnings`, and
`npm audit --audit-level=high` passed. Cargo deny reported only its configured
duplicate-version warnings. No frontend source file changed.

Production database role provisioning and external semantic/runtime
assumptions are explicit boundaries, not postponed internal certificate
conjuncts. No 0036 work is included.
