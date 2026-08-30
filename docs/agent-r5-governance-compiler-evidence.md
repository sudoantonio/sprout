# Evidenza checkpoint 0029 — governance compiler e agent creation

## Custodia e perimetro

- Baseline applicativa: `13ad342c2425e1d984a1c1a59e51f7b2933ad1c3`.
- Spec normativa invariata: `Sprout_AgentSpec_R5_no_model_memory_draft.lean`,
  SHA-256 `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`.
- Migration: `0029_agent_governance_compiler_tcb.sql`.
- Nessuna modifica frontend o Lean.
- Fuori scope e non ampliati: global synthesis, UserProxy e interrogation
  preesistenti; provider LLM, comments, administrator exception e
  GlobalMandate adapter restano mancanti/fail-closed. Il gate storico preserva
  i path positivi global admin-client/runner e UserProxy planning.

## Projection concrete → Lean

| Oggetto concrete | Projection normativa | Enforcement |
| --- | --- | --- |
| `ResponsibilityCompilationStatement` + `agent_compilation_certificates(task_kind=responsibility)` | `ResponsibilityContractCompiledBy`, `ResponsibilityCompilationWithinEnvelope` | Endpoint administrator TCB firma source/ciphertext commitments, output ed envelope canonici. Il server verifica registry, dual signature, device/key correnti, rules/scope/action/bound e current authority prima di attivare la revisione. |
| `LocalGoalCompilationStatement` + certificate locale | `LocalGoalCompilationWithinEnvelope`, `PromptRequirementsAndWorkExact` | Requirement e binding obligation/WorkSpec sono bidirezionali; action e tool policy devono coincidere con l'esatta WorkSpec e restare entro i massimi server-side. |
| `classify_local_goal_contract` + classifier version/hash persistiti | `LocalGoalClassifiedBy` | Input esclusivo: GoalContract già validato. Raggruppamento per `WorkKind`, scope dal goal, WorkSpec ordinati e clause ID deterministici. Output client/model non è autoritativo. |
| `FinalPromptApprovalStatement` + `agent_prompt_final_approvals(verified)` | `ControllerApprovalMatchesDraft` | Atto separato e domain-separated, exact su draft, agent, controller, LocalGoal/revision, compiler certificate, output hash e plaintext/ciphertext commitments. |
| active Responsibility + initial creation transaction | ramo Responsibility di `OperationalAgentCreationActivationCertificate.authorized` | Responsibility corrente deve coprire scope, action e GoalContract; permission e key status sono rivalidati nella stessa transaction. |
| `AdministratorAgentCreationApprovalStatement` + append-only approval | `ApprovedAdministratorAgentCreation`, `AgentCreationApprovedByAdministrator` | Exact proposal/creator/agent/draft/LocalGoal/compiler/prompt/availability/scope; valido solo per initial creation. Non autorizza revisioni, permission, tool grant, Responsibility o contributo bottom-up. |
| `agent_governance_ledger.position` | history governance append-only | Writer private serializzati da advisory lock. Il test apre backend distinti, usa un handshake, osserva il secondo in `wait_event_type='Lock'` e ne verifica la non-completion fino al rilascio. Exact replay non aggiunge entry, equivocation confligge; UPDATE/DELETE sono vietati. |

## Trust boundary E2EE e firma

Il plaintext viene compilato sul device autorizzato del controller per il
LocalGoal e dell'administrator per la Responsibility. Alla creazione iniziale
firma il device del creator/controller, non il runner del nuovo agente. Gli
artifact possono essere inoltrati da un'altra sessione corrente della stessa
identity: il device originale non deve essere online, ma la sua DeviceSigning
key/versione deve ancora appartenere al signer e non essere revocata.

Il verifier Rust è parte del TCB. PostgreSQL non verifica Ed25519 o ML-DSA-65:
le funzioni private accettano soltanto il risultato del verifier applicativo e
proteggono persistenza, append-only e replay. Non viene riciclata la
DeviceEncryption key. Una compromissione del processo Rust o dell'endpoint TCB
non è coperta; permission, scope, cataloghi e bound restano comunque validati
server-side.

La canonicalizzazione firmata usa il profilo Sprout governance integer-only:
key sort UTF-16, escaping JSON deterministico, array order preservato,
interi signed/unsigned in forma decimale minima e float rifiutati. I golden
vector fissano byte letterali e SHA-256; il vettore complesso vale
`8a138d4838e23d54d510e96c715ba313b0ac5c8373bc8b07f2944b7219a0acdd`.

## Registry e artifact identity

La chiave logica della registry è
`(task_kind, compiler_name, compiler_version)` e ammette un solo digest. I
manifest pinning verificati sono:

- LocalGoal protocol manifest:
  `0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2`;
- Responsibility protocol manifest:
  `78bd83db79112191f81aa118512092f7ea54a87733a82e823fa83cf107e3eb73`.

Questo dimostra il pinning del protocol manifest, non l'esecuzione di uno
specifico executable compiler riproducibile sul device.

## Sicurezza DB, atomicità e legacy

- DML diretto sulle tabelle certificate e sul ledger è revocato al ruolo app.
- Le writer sono `SECURITY DEFINER`, con owner trusted, `search_path` fissato,
  `PUBLIC EXECUTE` revocato e input `verification_state` non esposto.
- Trigger append-only rifiutano UPDATE/DELETE anche con GUC di identity
  contraffatta; il test crea inoltre oggetti shadow in `pg_temp` senza ottenere
  escalation.
- Initial creation persiste certificate, approval/provenance, identity,
  membership, device, runner, governed agent, LocalGoal, prompt e audit in una
  sola transaction. Ogni mismatch lascia zero residui.
- La creazione non modifica permission roots, grant, key envelope,
  Responsibility o GlobalMandate.
- Le righe 0028 senza witness indipendente restano `legacy_unverified`; non
  vengono sintetizzate firme o certificate. Una nuova activation richiede una
  nuova revisione certificata.
- Una seconda exact approval per lo stesso proposed principal può essere
  storicizzata; non introduce un blocco di liveness nello schema. L'activation
  resta fail-closed se la principal identity è già stata materializzata.

## Controesempi bloccati

- vecchio authorization payload `project_administrator`;
- Responsibility stale/superseded o revocata, con state/audit invariati;
- scope/action/WorkSpec/requirement fuori envelope o oltre massimi server;
- `invokeTool(tool-A)` con policy `[tool-B]`, policy della WorkSpec errata,
  duplicate tool e retry privo di provenance sufficiente;
- compiler sconosciuto/disabilitato, build digest alternativo e output/envelope
  hash contraffatti;
- firma Ed25519 o ML-DSA-65 mancante, errata o riferita a un altro messaggio;
- approval con draft, agent, controller, proposal, prompt, LocalGoal, revision,
  compiler certificate, availability o scope diversi;
- device signer, permission o Responsibility revocati prima dell'activation;
- app role che inserisce direttamente `verified`, modifica/cancella history o
  sfrutta `search_path`/GUC;
- stesso idempotency key con artifact differente e replay concorrenti che
  tentano di duplicare ledger/approval.

## Audit dei test storici adattati

| Test storico | Comportamento precedente | Comportamento attuale | Motivo normativo | Coverage positiva equivalente |
| --- | --- | --- | --- | --- |
| `edge_runner_is_a_revocable_device_and_cannot_bypass_governance` — provision | Creazione admin diretta senza LocalGoal certificate | Initial creation con compilation, final approval ed exact administrator approval | La sola authority `ProjectAdministrator` non è un ramo; R5.40/R5.41 richiede l'esatta `ApprovedAdministratorAgentCreation` | `exact_administrator_creation_is_atomic_and_does_not_grant_authority` e lo stesso test storico esercitano il path API positivo |
| stesso test — self-Responsibility/LocalGoal admin | Responsibility administrator→administrator e activation manuale | Nessuna self-Responsibility; initial LocalGoal nasce atomicamente dalla exact proposal | Administrator creation è distinta da Responsibility e non autorizza revisioni successive | `normal_controller_can_atomically_activate_exact_local_goal_and_stale_retry_rolls_back` copre revisioni Responsibility; `normal_user_creation_requires_active_compiled_responsibility` copre initial user creation |
| stesso test — global synthesis | Admin client e runner usavano una source sostenuta dalla self-Responsibility admin | La source `administratorCreation` è rifiutata; admin client e runner restano positivi usando una source con active compiled Responsibility | `LocalGoalCanContributeBottomUp = False` soltanto per la nuova provenance administrator creation | Due revisioni dello stesso GlobalContract, una admin-client e una runner, sono persistite nel test |
| stesso test — UserProxy plan | Piano positivo dell'owner tramite self-Responsibility | Piano positivo di un normal user con active compiled Responsibility su resource indipendente; forged classification resta negativa | Responsibility non può essere simulata dal ruolo admin; footprint e coverage sono server-derived | Proxy identity, thread, request e plan API restano positivi e sopravvivono al purge di un diverso agent resource |
| stesso test — interrogation | Record/read creator positivi e target non autorizzato `NOT_FOUND` | Invariato | Nessun nuovo vincolo 0029 vieta la superficie read-only | Lo stesso percorso API positivo è preservato |
| `cross_owner_review_requires_exact_active_task_provenance_and_current_permission` — setup | Provision/LocalGoal legacy tramite API non certificata | Fixture migration-owner per stato storico certificato | Evita di duplicare nel test cross-owner il protocollo compiler, senza alterare la route sotto test | Initial admin/user creation e revision activation sono coperte da test API dedicati; routing/review/materialization cross-owner resta E2E nello stesso test |

## Gate osservati

| Gate | Risultato |
| --- | --- |
| migration validation e fresh install | PASS, 29/29 |
| populated upgrade 0028→0029 | PASS; approval legacy rimasta `legacy_unverified`, campi witness null, zero certificate sintetici |
| `verify_schema.sql` | PASS |
| `verify_behavior.sql` | PASS |
| PostgreSQL ignored seriali | PASS, 23 passed / 0 failed / 0 ignored; un test ordinario `agents` filtrato dal comando `--ignored` |
| workspace ordinario | PASS, 209 passed / 0 failed / 23 ignored |
| rustfmt | PASS |
| Clippy `-D warnings` | PASS |
| cargo deny / cargo audit | PASS / PASS |
| WASM parity | PASS, 4/4 |
| WASM reproducibility | PASS, byte-for-byte |
| Lean canonica invariata | PASS con Lean 4.30.0 da `/home/fra/lean-fixed`; SHA-256 sorgente `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`, compilazione completa riuscita |

I test PostgreSQL dedicati includono initial administrator creation via exact
approval, initial user/controller creation via active compiled Responsibility,
stale Responsibility rollback con audit invariato, dual signatures, mismatch
atomici, revoca, app-role DML/shadowing, re-approval e concurrency/replay del
ledger. Le fixture migration-owner usate dai gate R5.30 rappresentano soltanto
stato storico già certificato e non sostituiscono questi percorsi API.

## Limiti residui

- Nessun executable compiler artifact è attestato o riprodotto dal server.
- L'uguaglianza commitment↔plaintext E2EE è un'assunzione endpoint-TCB.
- `retryTool` richiede ancora exact original ToolCall provenance al runtime e
  resta fail-closed in compilation quando tale binding non è disponibile.
- Administrator exception e GlobalMandate non hanno adapter autorevoli.
- Non esiste ancora una projection totale di ogni surface R5.40/R5.41. Global
  synthesis, UserProxy planning e interrogation hanno path preesistenti
  parziali preservati; le sole surface prive di adapter restano
  disabled/fail-closed.
- Questo checkpoint non dimostra full R5 refinement né liveness globale.
