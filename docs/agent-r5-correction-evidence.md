# R5 correction evidence — migration 0028

## Scope and custody

This checkpoint started from
`5fe670207ed127175d5c7c5743d39713b8f58f0d`. The normative input remained
`Sprout_AgentSpec_R5_no_model_memory_draft.lean` with SHA-256
`7e7aa3162a8b44d9c12de1b28a4af6506d189558c37d7e6d5417898c12ade714`.
The Lean file and every frontend source are unchanged.

The correction is intentionally limited to two concrete projections:

1. an exact, persistent WorkItem → task causal link for the implemented local
   `MarkAssignedDone` product-effect path;
2. prefix-append-only operational lists for `TaskObligationProvenance` and
   `PersistedTaskIntent`, including authenticated retention.

This document does not claim complete R5 refinement.

## Concrete projections

### Exact WorkItem → task causality

For the implemented local task-completion path, the authoritative projection is:

```text
Π_causal(DB, run) =
  semantic_run_causal_link_list(project, run)
  ordered by causal_position

WorkToTask(link) iff
  link.predecessor = Work(exact agent_run_task_effects.work_item_id)
  ∧ link.successor = Task(exact agent_run_task_effects.task_resource_node_id)
  ∧ link.task_effect_id = agent_run_task_effects.id
  ∧ link.run_id = effect.run_id
  ∧ link.goal_id = run.goal_id
```

The effect row is accepted only after the runtime has reloaded current
LocalGoal/obligation/WorkSpec provenance, work slot, claim/attempt, task,
assignment, completion, runner authority and current permission in one locked
transaction. `task_intent_id` remains nullable for this generic path;
`cross_owner_effect_id` remains nullable unless cross-owner governance is the
actual origin.

The same transition records the domain `CollaborativeCausalLink`, and
`persist_kernel_certificates` writes the relational edge with the exact effect
witness. Runtime reload replaces snapshot causal links with `Π_causal`, so
evidence and `SemanticState.causalLinks` cannot consume divergent graphs.
`TaskCompleted` evidence is accepted only when the requested task is the exact
causal successor of the requested WorkItem.

Retention moves deleted live causal certificates to
`agent_run_causal_link_retained_history` without changing `causal_position`,
identity or structural fields. The semantic function unions live and retained
rows and orders by the stable position. Replay uses the unique structural edge
and effect identity, so it cannot add a duplicate or reverse edge.

This refines the implemented path of `CollaborativeCausalLink`,
`GlobalCausalSuccessorOf`, `TaskObligationProvenanceValid`,
`ContractEvidenceSubjectMatches` and `MechanicalEvidenceValid`. It does not
provide structured global grounding and therefore does not enable the global
product materializer.

### Prefix-append-only operational history

The authoritative list projections are:

```text
Π_intent(DB, project) =
  semantic_task_intent_list(project)
  ordered by semantic_position

Π_provenance(DB, project) =
  semantic_task_provenance_list(project)
  ordered by semantic_position
```

`agent_semantic_operational_ledger` stores immutable structural witnesses. A
singleton cursor row serializes position allocation. The stable identity is
`(entry_kind, project_id, record_id)`; `semantic_position` is unique and is the
total-order key. Source-table insert triggers append a ledger row in the same
transaction. UPDATE/DELETE of normative ledger content is rejected.

Authenticated retention transfers the product row from live to retained
storage but does not update or delete the ledger entry. The semantic functions
select each identity once from the ledger and resolve its structural witness
from the corresponding live-or-retained source. Consequently a purge leaves
the list byte-identical, and a later insert obtains a greater position:

```text
Π(after) = Π(before) ++ suffix
```

The gate compares elements, not only counts or set membership. It exercises
append/append and append/purge with transactions that are both open and whose
lock wait is observed before the winner commits. Both controlled commit orders
preserve prefix, uniqueness and absence of duplicates. Restart reloads the
same ordered values.

The semantic-list functions are `SECURITY DEFINER` with a fixed search path and
trusted migration owner, but `PUBLIC EXECUTE` is revoked. Their internal
membership check uses the current authenticated identity. A
`NOSUPERUSER/NOBYPASSRLS` application role cannot execute any of the three
lists or use forged identity GUCs to read a foreign project.

This is the concrete list witness for the two list fields governed by
`SemanticOperationalStateExtends`. Other operational histories remain outside
this path-specific proof.

## Migration and upgrade compatibility

Migration 0028 succeeds both on a new database and on a populated database at
the exact current checksum of migration 0027.

The populated base contained 9 projects, 14 governed agents, one live
TaskIntent, one live TaskObligationProvenance, one task effect and three work
outcomes. The upgrade preserved the existing source identities. Its ledger
contains four live/retained witnesses at unique positions `1..4`; its causal
projection contains eleven unique positions and retains all pre-existing
non-task edges.

Migration 0025 already protects causal history with an append-only trigger.
0028 disables that trigger only inside the migration transaction while adding
new projection metadata to legacy rows, then reenables it before installing
the stricter 0028 validators. Existing normative causal fields are not changed;
failure rolls back the trigger state and all backfill work.

Missing legacy Work→Task edges are backfilled only from an exact
`agent_run_task_effects` plus matching `agent_run_work_outcomes` witness. The
new edge uses the stable effect UUID and recorded timestamp, with explicit
ordering. Two independent upgrades from the same populated 0027 database
produced identical ordered digests:

- semantic ledger: `1cfb5909d9d0f8d4df1155a60719ba7e` (`4/4` unique positions);
- causal graph: `1c8d02df9290e72255068427470033b8` (`11/11` unique positions).

Legacy task-correlated records without the exact witness are not promoted to a
causal certificate.

## Counterexamples now rejected

- A task with the same agent, scope and timestamp but no exact effect witness.
- A different task, WorkItem, WorkSpec, LocalGoal revision, obligation, claim
  or attempt substituted by the runner.
- A Work→Task link with reversed direction or without `task_effect_id`.
- `TaskCompleted` evidence for a task that is not the exact causal successor of
  the evidence WorkItem.
- A global contribution with coincident IDs but no structured global grounding.
- Replay that changes the idempotency hash or duplicates an assignment/effect,
  causal edge, TaskIntent or provenance entry.
- Claim use at or after expiry; the strict fence remains
  `acquired_at <= applied_at < expires_at`.
- UPDATE/DELETE of live normative source rows, ledger rows or retained history.
- Forged retention GUC, wrong subject, expired lease or application-role
  identity spoofing against semantic lists.
- Retention/restart that loses, duplicates or reorders the logical prefix.

Task materialization and evidence acceptance remain distinct. A task
completion can be observed before evidence acceptance. The later acceptance
transaction must persist both accepted evidence and discharged obligation, or
neither.

## Verification results

All commands below were run against disposable PostgreSQL 14 databases.

- Migration validation: 28/28 files passed.
- Clean install: migrations 1→28 passed.
- Populated upgrade: current 0027→0028 passed on two independent clones.
- `verify_schema.sql`: passed on clean install and populated upgrade.
- `verify_behavior.sql`: passed on clean install.
- DB-enabled ignored matrix, serial: 14 passed, 0 failed, 0 ignored in the
  selected binaries. Breakdown: 6 completion/causal/security gates, 4
  governance/RLS gates, 2 requirement/RLS gates, 1 retention gate and 1 sync
  idempotency gate. Filtered tests were not counted as passes.
- Ordinary `cargo test --workspace --all-targets`: 197 passed, 0 failed, 14
  ignored. The same 14 ignored tests are the DB matrix executed above.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo deny check advisories licenses bans sources`: passed; configured
  duplicate-version warnings remain non-fatal.
- `cargo audit --deny warnings`: passed.
- WASM parity: 1 file, 4 tests passed.
- WASM reproducibility: byte-for-byte passed.

The DB tests must run serially because their fixtures create shared PostgreSQL
roles and exercise database-global security configuration. A preliminary
parallel invocation produced fixture-state conflicts; the required serial
matrix is green and is the reported gate.

## Residual limits

- Exact Work→Task materialization is covered only for the local
  `MarkAssignedDone` path. Other task effects remain unimplemented.
- Global synthesis and `StructuredGlobalWorkGroundingValid` remain open; the
  product API deliberately fails closed without the grounding certificate.
- Mechanical evidence kinds other than local `TaskCompleted`, semantic
  judgment adapters and the remaining blocker adapters remain fail-closed or
  partial as recorded in the traceability matrix.
- Exact final-prompt approval remains partial: the manual activation path has
  structural binding, but the server cannot compare E2EE plaintext bytes and
  the historical backfill lacks an independent pre-0027 approval witness.
- LocalGoalClassifier, StructuredLocalContractCompiler,
  ResponsibilityCompiler, initial-agent governance, comments, UserProxy,
  interrogation, global synthesis, LLM adapters and global fairness/liveness
  were not changed by this checkpoint.
- Current-permission and cross-owner claims remain limited to the enumerated
  tested paths; no global authority-completeness claim is made.
