# Sprout R5.40/R5.41 validation report

Date: 2026-08-20

## Scope and chain of custody

The validation started at Git HEAD
`75c3d964bc548d189bddaa5925c911b642163f1f` on branch
`codex/lean-concrete-refinement`.

The canonical baseline was:

- file: `Sprout_AgentSpec_R5_no_model_memory_draft.lean`;
- SHA-256: `0b7754cf65b92411269be5b1af70d9895d0ad39e0e697482ec4dee9c57cf254b`;
- 15,351 lines and 576,209 bytes.

The superseded candidate had SHA-256
`365e0abbd0494b69b1acce68db8aa346d9d2eb0370967d16f81905ceb4290456`.
Its diff was additive, but the complete baseline was **not** a literal byte prefix:
R5.40/R5.41 had been inserted before the final `end R5` and
`end Sprout.AgentSpec`. This corrects the inaccurate prefix claim in
`R5_40_41_reconstruction_manifest.md` for that candidate.

The replacement candidate initially had SHA-256
`83301354be93a2a24539bdcdfe69b31caa472e1dc948c62cec602b29a6934f5e`.
Before compilation it satisfied:

- complete baseline as a literal 576,209-byte prefix;
- `cmp -n $(wc -c < baseline) baseline candidate`: PASS;
- 1,528 inserted lines and zero removed lines;
- R5.40/R5.41 body SHA-256
  `e45472ababcf70e887ac91b3fbf3474df82d62cc7196137cff663359bae8651d`;
- body byte-identical to the superseded candidate;
- diff whitespace check: no diagnostics.

After the two mechanical elaboration fixes documented below, the final canonical
source has SHA-256
`c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`,
16,894 lines, and remains a literal extension of the baseline with 1,543
insertions and zero removals.

No Rust, SQL, frontend, migration 0029, ZIP, photo, or unrelated Lean artifact
was modified by this validation.

## Canonical Lean environment

- working directory: `/home/fra/lean-fixed`;
- executable: `/home/fra/lean-fixed/bin/lean`;
- Lean: `4.30.0`, commit
  `d024af099ca4bf2c86f649261ebf59565dc8c622`;
- Lake: `5.0.0-src+d024af0`;
- toolchain checksums in `/home/fra/lean-fixed/CHECKSUMS.sha256`: PASS;
- network, package installation, and toolchain updates: not used.

The full compilation command was:

```sh
cd /home/fra/lean-fixed
/home/fra/lean-fixed/bin/lean \
  /home/fra/lean-fixed/Sprout_AgentSpec_R5_40_41_validation.tmp.lean
```

The temporary source was checked byte-for-byte against the candidate before
each compilation. Final result: exit status 0 with no Lean diagnostics.

## Mechanical corrections

Only two changes were made, both inside the appended R5.40/R5.41 section:

1. `r541_non_noop_action_has_class` now supplies an explicit
   `AgentActionClass` witness for each of the eleven non-`noOp` constructors and
   eliminates the `noOp` branch through the existing `nonNoOp` premise. The
   original one-line `simp` script left eleven goals unsolved; no proposition or
   gate changed.
2. `R541CrossOwnerSurfaceCertificate.everyRequestRouted` now requires
   `Nonempty (CrossOwnerTaskAssignmentRoutingCertificate ...)`. The routed
   certificate is Type-valued, while the enclosing structure is Prop-valued;
   `Nonempty` is the standard proof-valued existence projection and preserves
   the requirement that every declared request has an actual routing
   certificate.

No gate was removed, made optional, replaced with `True`, or weakened. The new
section contains no `sorry`, `admit`, `unsafe`, `by?`, new `axiom`, or opaque
proof used to hide an obligation.

## Axiom audit

`#print axioms` was executed for all 21 new R5.40/R5.41 theorems, including all
requested root, exactness, non-vacuity, and counterexample theorems.

- no `sorryAx`;
- no non-standard axiom was introduced;
- most theorems depend on no axioms;
- `propext` is used by action match/equality and selected structural proofs;
- `sprout_r5_41_formal_release` depends only on `propext`,
  `Classical.choice`, and `Quot.sound`, the same standard axioms admitted by the
  baseline.

In particular:

- both `r540_distinct_traces_*` theorems: no axioms;
- `r540_exact_model_invocation_has_no_hidden_memory`: no axioms;
- `r541_enabled_surface_is_nonempty`: no axioms;
- `r541_compiled_action_has_exact_requirement`: no axioms;
- `r541_agent_creation_does_not_grant_authority`: no axioms;
- `r541_formal_release_is_nonvacuous`: `propext`;
- every `r541_counterexample_*`: no axioms except the disabled-surface theorem,
  which uses `propext`;
- `sprout_r5_41_formal_release`: `propext`, `Classical.choice`, `Quot.sound`.

## Adversarial checks

A temporary positive harness compiled with exit status 0 and derived the required
contradictions for all seven categories:

1. enabled surface plus empty records;
2. `disabledFailClosed` surface plus a present record;
3. exact WorkItem event and exact model invocation reused across distinct trace
   IDs;
4. WorkSpec allowed action with no exact originating requirement;
5. agent creation with changed resource permissions;
6. exact model invocation with hidden persistent model memory;
7. model, disclosure, and interrogation projections differing from their
   respective actual runtimes while retaining `R541ExternalReleaseAssumptions`.

Two direct invalid-construction files were also compiled intentionally:

```sh
/home/fra/lean-fixed/bin/lean /tmp/Sprout_R5_40_41_negative_enabled.lean
/home/fra/lean-fixed/bin/lean /tmp/Sprout_R5_40_41_negative_disabled.lean
```

Both exited with status 1 as required:

```text
invalidEnabledEmptyGate: unsolved goals
⊢ False

invalidDisabledGateWithRecord: unsolved goals
⊢ False
```

The negative files and other temporary Lean harnesses were not added to the
repository.

## Root connectivity matrix

| Cluster | Root path |
|---|---|
| 1. Secure completion kernel | `secureKernel` → `SecureAssumptionMinimalFullSuccessKernelCertificate.completion` |
| 2. Scheduler, claim, dynamics | `secureKernel.completion.progress` → `AssumptionMinimalProgressKernelCertificate.base` and `.dynamics` |
| 3. Failure plan, retry, terminality | completion progress kernel → `CollaborativeKernelCertificate` and execution dynamics |
| 4. Blocker | completion progress kernel plus `concreteTrace.everyBlockerResolutionExact` and `traceFeatureGates.blockersExact` |
| 5. Tool | completion/safety kernel plus `concreteTrace.everyToolExact` and `traceFeatureGates.toolsExact` |
| 6. Evidence, discharge, causal graph | `secureKernel.completion.evidenceDischarge`, concrete evidence/causal events, and causal append-only history |
| 7. Authority and information safety | `secureKernel.safety` |
| 8. Compiler/action exactness | `compilerActionExact` |
| 9. WorkSpec/security compatibility | `securityPoliciesExact` |
| 10. Active governance kernel | `governanceKernel` |
| 11. Revision, creation, Responsibility | `governanceOperational` plus exact trace-bound fields |
| 12. Global synthesis | `governanceOperational.everyGlobalSynthesisCertified`, `globalInventoryExact`, and `global` |
| 13. Comments | `comments` |
| 14. Cross-owner | `crossOwner` plus `crossOwnerTraceBound` |
| 15. UserProxy | `proxy` plus `proxyTraceBound` and `operationalClosure.proxyProvisioned` |
| 16. Interrogation and model/no-memory | `interrogation`, `model`, concrete trace exactness, and actual↔declared assumptions |
| 17. TaskIntent/provenance | `taskOperational` plus both trace-bound fields |
| 18. Append-only operational state | `operationalHistory` and `operationalClosure` |

`governedRunExact` binds `governed.toObservedSemanticRun` to
`secured.certified.run`; `runGoalExact` and `traceStartExact` bind the same run,
goal, and start tick. No R5.40/R5.41 surface certificate declared by the root is
left disconnected. Optional surfaces remain non-vacuous when enabled and empty
when explicitly fail-closed.

## Reproducibility

The corrected temporary candidate was compiled twice:

```sh
/home/fra/lean-fixed/bin/lean -o /tmp/Sprout_R5_40_41_pass1.olean \
  /home/fra/lean-fixed/Sprout_AgentSpec_R5_40_41_validation.tmp.lean
/home/fra/lean-fixed/bin/lean -o /tmp/Sprout_R5_40_41_pass2.olean \
  /home/fra/lean-fixed/Sprout_AgentSpec_R5_40_41_validation.tmp.lean
```

The outputs were byte-identical. Both SHA-256 values are:

`fabeb7fbc19d669484e5047b9cffb70070663789c9f1b8688dcd643cd916e81c`.

The compiled temporary source and promoted canonical source were byte-identical,
both with SHA-256
`c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`.

## Added declarations

The additive section introduces 79 declarations. They are grouped as follows:

- R5.40 event and trace records: `R540WorkAttemptEvent`,
  `R540WorkOutcomeEvent`, `R540BlockerResolutionEvent`,
  `R540CausalLinkEvent`, `R540ToolEvent`, `R540EvidenceEvent`,
  `R540DisclosureEvent`, `R540ModelInvocationEvent`,
  `R540InterrogationEvent`, and `R540ConcreteExecutionTrace`;
- exactness predicates/projections: `R540EventWithinTrace`, all
  `R540*EventExact` definitions, actual/model/disclosure/interrogation runtime
  projections, their exactness predicates, and `R540ConcreteTraceCertificate`;
- R5.40 uniqueness/no-memory theorems: all six `r540_*` declarations;
- R5.41 surface mode/gate and non-vacuity theorem;
- prompt/action exactness certificate and its three action theorems;
- WorkSpec/security compatibility predicate, certificate, and footprint theorem;
- authority frame, typed governance/feature records, creation frame, and creation
  no-grant theorem;
- governance, task, comment, proxy, global, interrogation, model, trace-feature,
  and cross-owner surface certificates;
- external release assumptions, formal root, release guarantees, root extraction
  theorems, and all six `r541_counterexample_*` theorems.

No baseline declaration was edited.

## Remaining boundaries and claim level

This checkpoint validates a formal release certificate; it does **not** establish
100% concrete product implementation.

Remaining explicit boundaries include:

- `MinimalContractSuccessExternalAssumptions`;
- prompt/requirement semantic adequacy;
- authenticity of semantic external/derived evidence through the supplied judge;
- the endpoint/provider actual-runtime observations used by the exact projection
  assumptions;
- surfaces that may legally remain `disabledFailClosed` with an empty inventory;
- all Rust/SQL materialization and reachability work, including migration 0029.

Therefore the justified result is: the R5.40/R5.41 formal root and its stated
counterexample barriers compile and are reproducible under their explicit typed
assumptions. No claim of complete concrete software refinement is made.
