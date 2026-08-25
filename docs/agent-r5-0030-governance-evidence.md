# Checkpoint 0030 — governance exception and GlobalMandate evidence

## Scope and chain of custody

- Starting HEAD: `0bce5f152319952f62a414b308713b1bcca235d7`.
- Normative source: `Sprout_AgentSpec_R5_no_model_memory_draft.lean`.
- Normative source SHA-256 before and after implementation:
  `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`.
- Migration: `0030_agent_governance_exception_global_mandate.sql`.
- Lean and frontend source files are unchanged by this checkpoint.

This checkpoint refines only existing-agent LocalGoal revisions through an
administrator exception or a GlobalMandate. Initial creation through either
provenance remains fail-closed.

## Concrete projections

### Administrator exception

The authoritative projection is the ordered, immutable governance event chain:

```text
local_draft_disposition(requestAdministratorReview)
  -> exception_consent(consented=true, exact review/user/source draft)
  -> exception_review(exact real R4 task causally materialized after consent)
  -> exception_admin_draft(revision, exact compiled LocalGoal, optional compiled Responsibility)
  -> exception_decision(exact terminal task + exact draft revision)
  -> approved_local_exception(deterministic event identity)
  -> local compilation ledger + distinct final controller prompt approval
  -> atomic activation
```

The review endpoint uses the product task subsystem in the same transaction and
persists an exact review/task causal binding. A compatible historical task or a
task belonging to another review is rejected. Exact replay reuses the same task
and the same deterministically derived approved-exception event; any changed
authoritative binding conflicts.

`approvedGoalOnly` leaves the active Responsibility unchanged.
`approvedGoalAndResponsibility` activates the compiled Responsibility revision,
prompt and LocalGoal in one transaction after the separate controller approval.
`rejected` persists exactly one terminal decision but creates no approved
exception and leaves Responsibility, prompt, LocalGoal, permissions and key
envelopes byte-for-byte/structurally unchanged. Exact replay is idempotent and a
later discordant terminal decision conflicts.

### GlobalMandate

The existing-agent projection is:

```text
active global contract/revision + exact obligation
  -> GlobalCoverageNeed
  -> global_mandate_assignment event
  -> exact LocalGoal compilation certificate/ledger entry
  -> distinct final controller prompt approval
  -> atomic LocalGoal activation
```

No verified `global_mandate` compilation can enter the ledger unless its
authorization ID and revision identify the exact persisted assignment. The DB
writer additionally matches event kind, authoritative administrator,
project-delegable agent, LocalGoal ID/revision/origin/payload, compilation
certificate, global contract/revision, exact need and obligation. Nonexistent,
stale, foreign-mandate and wrong-LocalGoal references fail without partial
events, certificates, LocalGoals or ledger entries. Exact replay is idempotent.

Activation revalidates current project/controller relation, availability,
resource permissions, tool permissions, compiler build and signing keys. The
mandate never creates permission, tool grants, membership, Responsibility or key
envelopes, and its LocalGoal cannot contribute bottom-up.

### CoverageNeed resource footprint

For the currently compiled model every resource action in a WorkSpec targets
`GoalContract.scope`. The 0029 compiler validator independently requires each
exact prompt requirement bound to that WorkSpec to use the same scope and the
security policy to match the exact operation/tool. Consequently two contracts
with identical action classes but different scopes derive different resource
effects.

This is a structural concrete policy, not a claim that the backend understands
the E2EE plaintext. Plaintext-to-resource semantic adequacy remains an
**EXTERNAL TCB ASSUMPTION** of the authorized endpoint compiler. A tool identity
that is not structurally grounded remains fail-closed.

### New-agent proposal

`NewAgentForGlobalNeedProposal` is persisted only when its requested footprint
equals the need. It is non-authorizing and creates no principal, governed agent,
runner, permission, tool grant, key envelope or active LocalGoal.

## Security and persistence boundary

- Rust hybrid-signature verification remains part of the trusted computing
  base; PostgreSQL does not claim to verify Ed25519 or ML-DSA itself.
- Governance history is append-only. The normal app role cannot directly
  insert verified events or update/delete verified history.
- Private writer functions retain trusted ownership, fixed `search_path`, no
  PUBLIC execute and exact replay/equivocation fencing.
- No GUC, generic project-admin role or client/model flag is authority.
- Exception and mandate paths reuse current permission, RLS, E2EE, compiler,
  classifier and final-approval systems; no agent-specific ACL is introduced.

## Verification results

| Gate | Observed result |
| --- | --- |
| Migration static validation | PASS — 30 migration files validated |
| Fresh PostgreSQL install | PASS — migrations 1→30 |
| Populated upgrade | PASS — 1→29, populated certified data, then 0030; no synthetic 0030 witness |
| `verify_schema.sql` | PASS |
| `verify_behavior.sql` | PASS |
| PostgreSQL ignored tests, serial | PASS — 27 passed, 0 failed, 0 ignored |
| Workspace ordinary tests | PASS — 210 passed, 0 failed; DB-only tests ignored in this mode |
| `cargo fmt --all --check` | PASS |
| Clippy `-D warnings` | PASS |
| `cargo deny` | PASS — advisories/licenses/bans/sources |
| `cargo audit --deny warnings` | PASS |
| WASM parity | PASS — 4/4 |
| WASM reproducibility | PASS — byte-for-byte |
| Lean 4.30.0 compile | PASS from `/home/fra/lean-fixed` on a byte-identical temporary copy |

Targeted API/domain gates cover:

- goal-only and goal+Responsibility exception activation;
- rejected exception persistence, idempotent replay and non-authorizing frame;
- stale final controller approval rollback with Responsibility revision 1,
  prompt revision 1 and LocalGoal revision 1 still solely active;
- exact post-activation cardinality: one active Responsibility at revision 2,
  revision 1 superseded, one active exact final LocalGoal and exact final prompt;
- exact consent→review→real R4 task causality and replay;
- stale/foreign task and event equivocation rejection;
- exact GlobalMandate positive/replay and nonexistent, foreign, wrong-LocalGoal
  and stale-revision negatives with zero residue;
- permission and availability revocation before activation;
- no authority amplification and no bottom-up contribution;
- scope-distinct resource footprints and requirement-scope exactness;
- non-materializing least-privilege new-agent proposal;
- app-role verified-history DML and SECURITY DEFINER shadowing rejection.

## Remaining limitations

- Exception and GlobalMandate initial agent creation are intentionally
  **FAIL-CLOSED / NOT YET IMPLEMENTED**.
- The global synthesis semantic algorithm/provider, total global grounding and
  global liveness are not implemented by 0030.
- CoverageNeed semantic adequacy is an endpoint-TCB boundary; only structural
  exactness is server-validated.
- Tool identity without exact structural grounding remains fail-closed.
- No executable compiler attestation is claimed; the pinned protocol manifest
  and authorized endpoint remain the existing TCB assumption.
- Comments, complete UserProxy, interrogation answer adapter and provider LLM
  are unchanged and remain outside this checkpoint.
- This report does not claim full R5 refinement.
