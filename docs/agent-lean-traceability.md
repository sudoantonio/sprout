# Tracciabilità Sprout AgentSpec R5 → concrete product

## Fonte normativa e metodo

La fonte normativa di questo audit è
`Sprout_AgentSpec_R5_no_model_memory_draft.lean`, verificata byte-per-byte con
SHA-256
`c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`.
Il file non è modificato, copiato, rinominato o aggiunto a Git.

La classificazione distingue:

- **completo**: domain, persistence, API/runtime, authorization, audit e test
  necessari sono concretamente raggiungibili;
- **parziale**: esiste un percorso concreto, ma manca almeno un invariante o
  una superficie normativa;
- **solo domain**: il concetto ha tipi/validator Rust ma non è usabile nel
  prodotto;
- **mancante**: il concrete product non rappresenta ancora il concetto;
- **boundary esterna**: soltanto quanto R5.26, R5.30.9–11 e R5.37 dichiarano
  realmente non derivabile dal kernel Sprout.

La matrice conserva gli incrementi verificati a partire dall'HEAD
`83bc6def6132fa87f595725dade64ed4e65963ea` ed è ora riallineata alla root
formale R5.40/R5.41 promossa dall'HEAD normativo
`13ad342c2425e1d984a1c1a59e51f7b2933ad1c3`.

## Incremento domain R5.30: projection corrente e storia operativa

La rappresentazione domain in corso separa esplicitamente due piani che non
devono essere confusi:

- `CollaborativeRunState.work_items` è la sola projection corrente di
  `SemanticState.workItems` e contiene esclusivamente WorkItem la cui
  `WorkSpec.activation` vale nei fatti correnti;
- `work_slots`, `inactive_work_items` e `work_projection_history` conservano
  identità canonica, stato operativo e provenance storica, ma non vengono
  interpretati come WorkItem presenti nel `SemanticState` corrente.

| Requisito Lean | Refinement domain corrente | Invariante/test concrete |
| --- | --- | --- |
| `workActivationSound` | **coperto nel kernel domain** | La materializzazione richiede activation vera; `refresh_frontier` rimuove atomicamente dalla projection corrente il work divenuto inattivo; `validate_current_projection` rifiuta ogni work corrente non attivo. `inactive_required_entry_is_rejected_before_any_work_projection_exists` e `inactive_work_is_not_projected_and_reactivation_preserves_canonical_identity`. |
| `eligibleWorkStatusSound` / `ContractWorkEligible` | **coperto nel kernel domain** | `Eligible` richiede activation corrente, dependency chiuse, attempt sotto bound e assenza di blocker waiting applicabili. La stessa condizione è rivalidata al claim e prima del terminal effect. |
| `blockedWorkHasWaitingBlocker` | **coperto nel kernel domain** | `Blocked` non è più il fallback per activation/dependency false. Può essere introdotto soltanto insieme a un blocker waiting applicabile; il validator rifiuta `Blocked` opachi. Test `blocked_work_always_has_an_applicable_external_waiting_blocker`. |
| `waitingBlockersExternallyControlled` | **coperto nel kernel domain** | La creazione accetta solo principal umani, administrator, task assegnate a umani o external outcome dichiarati dalla waiting rule. Target work/obligation interni sono rifiutati. |
| `BlockerResolution`, `BlockerWaiting` → `BlockerTerminal` | **coperto nel kernel domain** | Il caller non sceglie lo status terminale. Una `BlockerResolutionObservation` schema-closed deve coincidere con la projection autorevole di una task umana terminale, una risposta/commento osservato, una decisione administrator valida oppure un outcome esterno con provenance hash esplicito. L'observation deve inoltre soddisfare `observed_at ≥ blocker.created_at`, anche quando evento e fact sono altrimenti autentici e identici. Il runtime deriva `resolved`/`failed`/`cancelled`, registra una resolution append-only e crea dispatch solo dopo la validazione. La terminalità del blocker non modifica né discharge l'obligation. |
| dependency interne | **coperto nel kernel domain** | Una dependency non chiusa impedisce la materializzazione/eligibility e non crea blocker. Test `internal_dependency_does_not_create_a_false_blocker`. |
| activation entry required | **strengthening concrete necessario** | Il validator richiede che `(obligation.activation ∧ requiredForCompletion)` implichi l'activation della WorkSpec entry. Senza questa proprietà `requiredObligationClosure`, `entryWorkClosure` e `workActivationSound` sarebbero simultaneamente insoddisfacibili. |
| claim durante deactivation / `claimedWorkResolvesWithinSpecBound` | **coperto nel kernel domain** | Il work esce subito dalla projection, la claim viene rilasciata e il terminal effect è rifiutato. Una deadline derivata da `maxResolutionTicks` richiede retry/progresso o porta il goal a un terminale failed; nessuna claim resta sospesa. Test `activation_ceasing_during_claim_rejects_effect_and_resolves_by_bound`. |
| canonical slot/ID stability | **coperto nel kernel domain** | Il record inattivo mantiene lo stesso `(WorkSpecId, slot) → WorkItemId`; uno slot inattivo non è libero e la riattivazione riproietta lo stesso ID. |
| `NoOpenGoalRelevantWork` / `ContractCompletionCriterion` | **coperto nel kernel domain** | La completion considera tutti e soli i WorkItem presenti nella projection corrente e tutti i blocker waiting correnti; la storia inattiva non bypassa work attivo e non rende impossibile la completion. Test `inactive_work_history_neither_bypasses_nor_prevents_completion`. |

La migration `0025_agent_completion_runtime.sql` porta ora questa projection
nel runtime persistente: lo snapshot domain hashato resta l'unica semantica,
mentre slot, claim, blocker, resolution, work-product binding, outcome,
evidence e causal link sono certificati/guard relazionali. Ogni transition è
serializable, versionata e append-only; i facts vengono ricostruiti dal server.
Le API non possono fornire activation, eligibility, condition facts, blocker
status, discharge, completion o authority.

Il checkpoint dei gate persistenti è verificato su PostgreSQL reale dalla
suite DB-enabled `agent_completion_gates` (sei test, esecuzione seriale):

- un nuovo processo/router ricarica lo stesso mapping canonico degli slot, la
  projection current/inactive e la projection history senza duplicare ID;
- un tentativo di completion con obligation, work e blocker aperti esegue
  rollback integrale, mentre `GoalCompleted` resta osservabilmente distinto da
  `RunCompleted` fino alla commit finale;
- claimant concorrenti non ottengono due lease, una lease scaduta non può
  autorizzare un effect, il worker recupera lo stesso WorkItem canonico al
  tentativo successivo e la posizione di uno work più giovane scende entro il
  bound della frontier persistita;
- una run da GlobalContract con due agent partecipanti resta active/running
  dopo il terminale del primo; soltanto obligation, work ed evidence causale
  di entrambi consentono prima `GoalCompleted` e poi `RunCompleted` nel kernel
  domain; il product materializer globale resta fail-closed senza
  `StructuredGlobalWorkGrounding` concreto;
- le tre semantic list (`TaskIntent`, `TaskObligationProvenance`, causal link)
  non sono eseguibili da `PUBLIC` e un role applicativo
  `NOSUPERUSER/NOBYPASSRLS` non può usare identity GUC forgiate per leggere un
  altro progetto.

Il gate causale esercita il materializer task generico con provenance
`LocalGoal/obligation/WorkSpec → canonical WorkItem → claim/attempt → task
completion → exact Work→Task link → outcome`; l'acceptance della mechanical
evidence è una transition successiva e distinta, atomica con il discharge.
`TaskIntent` non è una
precondizione del percorso generico; resta opzionale ed è richiesto soltanto
quando la causal origin è il workflow cross-owner. Il gate rifiuta una seconda
task dello stesso agent/scope/periodo priva del binding esatto e prova inoltre
il fencing stretto `applied_at < expires_at`: uguaglianza e superamento della
deadline rollbackano task completion, effect, transition, outcome ed evidence.

## Matrice R4 e continuità R4 → R5

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| `ApiBoundary`, actor/session e `Move.agentMove` | **parziale** | Sessioni e identity agent sono distinte e validate; manca una proiezione persistente uniforme delle mosse R4 che attribuisca ogni task/comment/tool effect all'actor agentico senza confonderlo col controller. |
| `WellFormedState`, permission e frame conditions | **parziale** | Permission/RLS/E2EE esistenti sono riusati e gli agenti non possono usare le normali mutation route. Il dispatcher copre solo Info e non rappresenta tutte le `AgentAction`; non esiste quindi ancora una verifica delle frame condition su tutto il linguaggio R4. |
| Task R4: create/replace/delete/assign/unassign/done/note/attachment | **parziale** | Le operazioni umane esistono nel prodotto. `MarkAssignedDone` dispone ora di un materializer agentico governato con provenance/authority/work binding; gli altri equivalenti agentici restano mancanti salvo Info. |
| `Comment`, `PostCommentEffect`, `CommentAdmissible` | **mancante** | Non esiste persistence/API commenti human/admin/agent→agent nel contesto risorsa. Mancano recipient, E2EE payload, parent/depth, unique root/notification, eventi e audit. |
| `CommentPriorityDiscipline` administrator > user > agent | **mancante** | Nessuna coda/response runtime dei commenti e nessun ordinamento persistito. |
| `ToolCallRecord`, `ToolAuditEntry`, retry/timeout | **parziale** | `agent_invocations` ha lease, expiry e retry bounded del provider LLM. Non è un tool catalog R4: mancano ToolCallId tipizzato, input/output tool, required-effects adapter, audit requested/retry/completed/failed/timedOut e bridge work→tool. |
| `Activates`, `TriggerResponsiveness` | **mancante** | Non esiste event/trigger dispatcher agentico per resource update, comment e tool terminal event. |
| scheduler/runtime fairness R4 | **mancante** | `SKIP LOCKED ORDER BY created_at` seleziona invocation, ma non certifica agent fairness, runtime fairness, responsiveness o anti-starvation R4. |
| `TaskCompletionCausality`, assigned-task liveness | **parziale forte** | Il percorso `MarkAssignedDone` conserva un binding causale generico esatto fra LocalGoal/WorkSpec, WorkItem, claim/attempt, task assignment, completion, outcome ed evidence; stessa identità/scope/tempo non basta. Liveness e gli altri task effect R4 restano aperti. |
| `PromptObligationLiveness` e prompt corrente | **parziale forte** | Prompt revision e LocalGoal hanno draft/active/superseded distinti e l'esatto ciphertext prompt viene attivato atomicamente con la revisione LocalGoal; obligation/discharge R4 end-to-end restano dipendenti dai materializer task/tool ancora mancanti. |
| `UniqueCommentNotifications` | **mancante** | Dipende dal sistema commenti assente. |
| `CorrectionProfileFromRun`, `Outcome`, strategy preference | **mancante** | Non esiste una run osservabile completa da cui derivare revision/comment counts; non va sostituita da metriche client-declared. |
| `ProjectsToR4`, `PreservesR4ValidRun`, `ResponsibleRun` | **mancante** | Il runtime R5 corrente non costruisce una proiezione R4 completa; questa continuità deve diventare un invariante degli eventi agentici, non una dichiarazione documentale. |

## Matrice R5.30–R5.32: completion kernel

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| `GoalContract` DSL completa | **coperto nel kernel persistente** | Contract schema-closed con goal/scope, condition ricorsive, obligation, dependency, WorkSpec, evidence/waiting rule e completion normalizzata; la revisione autorevole viene copiata e hashata nella run. |
| `GoalContractWellFormed` | **coperto nel kernel domain** | Il validator chiude riferimenti, ownership, rank/bounds, entry, continuation/failure plan, evidence/waiting subject e normalizzazione prima della persistenza. |
| revisioni autorevoli/program snapshot | **coperto per la run** | La creazione accetta soltanto LocalGoal attivo alla revisione esatta o GlobalContract corrente con source LocalGoal attive; contract/state hash, optimistic version e snapshot append-only fanno fencing. |
| `ObligationInstance` e birth closure | **coperto nel kernel persistente** | Le istanze sono nella projection canonica hashata e in ogni transition snapshot; activation e birth closure sono costruite da facts server-side. |
| `WorkItem`, slot certificate, canonical work universe | **coperto nel kernel persistente** | Slot relazionali immutabili certificano `(WorkSpecId,slot)→WorkItemId`; projection corrente, inactive history e projection events sono nello snapshot. `restart_reload_preserves_canonical_projection_and_history` materializza e disattiva work tramite facts prodotto, ricostruisce il server sullo stesso PostgreSQL e prova mapping, ID, current/inactive projection e history invariati e non duplicati. |
| activation, eligibility e work existence | **coperto nel kernel persistente** | Facts da task/stato autorevole, refresh/claim/effect nella stessa transaction serializable e projection validator domain. Nessun facts payload è accettato dall'API. |
| waiting rules e typed blocker | **parziale forte** | Blocker/status/resolution sono persistiti e certificati dalla transition domain. Task terminal è risolto da stato prodotto; decisione admin, risposta principal e outcome esterno restano fail-closed finché mancano i rispettivi ledger tipizzati. |
| dispatch e scheduler position | **coperto nel kernel persistente** | Dispatch, attempt, enqueue tick e scheduler position sono parte dello snapshot autorevole; la claim relazionale ne è il guard di concorrenza. |
| claim/lease, esclusività, expiry, recovery | **coperto nel runtime persistente** | Unique active claim per work, unique attempt, lock serializable, authority/runner corrente prima di claim/effect e worker scheduler-only per recovery bounded. I 300 secondi sono soltanto il default operativo: `claim_next` limita la lease effettiva alla `WorkSpec.maxResolutionTicks` certificata, come richiesto dalle dynamics R5.30, e il certificato DB conserva quel deadline. I gate PostgreSQL provano un solo vincitore, recovery sul medesimo WorkItem e la semantica stretta di `LogicalClaimValidAt`: effect ammesso solo con `applied_at < expires_at`, mentre uguaglianza e superamento rollbackano ogni stato prodotto/kernel parziale. |
| retry generation e failure continuation | **parziale forte** | `retrySame`, alternative e `failGoal` sono transition domain persistite con attempt/continuation canoniche. `dischargeBy` è validato dal kernel ma l'API failure non può ancora collegare una evidence autorevole e quindi fallisce chiuso. |
| evidence meccanica/semantica e provenance | **parziale forte** | Evidence è schema-closed e derivata dal server. Per `TaskCompleted` il materializer locale crea il binding generico autorevole run/work/claim/attempt/task e l'exact Work→Task link; l'outcome validato è l'unica fonte della successiva mechanical evidence. Stesso agent/scope/tempo e un task ID scelto dal runner non bastano. La projection temporale SQL usa lo stesso secondo contenitore (`floor(epoch)`) di `DateTime::timestamp()`. Gli adapter mechanical non-task e semantic judgment restano fail-closed/boundary. |
| discharge e accepted-evidence closure | **coperto per TaskCompleted mechanical locale** | Task effect/outcome e acceptance non sono fusi: una completion osservata non è evidence accettata. La successiva `accept_evidence` richiede exact causal successor, rule/mechanical mode, subject WorkSpec esatto e committa evidence con lo snapshot in cui l'obligation è discharged; un errore rollbacka entrambi. Blocker terminality non entra in questo percorso. Altri evidence kind restano parziali/fail-closed. |
| `CompletionCriterion` bookkeeping | **coperto nel runtime persistente** | Il runtime rivaluta facts, obligation required, work corrente e blocker nella transaction della transition terminale. Il gate API/DB prova il rifiuto con tutti e tre ancora aperti, hash/version/status immutati dopo rollback e commit finale coerente. |
| `RunCompleted ≠ GoalCompleted` | **coperto nel runtime persistente** | `goal_status` e `run_status` sono distinti e il DB vieta `run=completed` con goal non completed. I gate osservano persistentemente `goal=completed, run=running` prima della transition finale e `goal=completed, run=completed` soltanto dopo la commit atomica. |
| causal graph globale | **parziale** | Il path locale `MarkAssignedDone` proietta l'esatto edge Work→Task dalla riga `agent_run_task_effects`; la semantic list relazionale è la sorgente usata da reload runtime ed evidence, preserva gli altri edge e sopravvive a restart/retention. Comment/tool, altri task effect R4 e global grounding non ancora materializzabili restano gap, non link sintetici. |
| finitezza e anti-loop multi-agent | **coperto nel kernel/persistence** | Slot finiti, rank di generation/dependency, bounds e identità canoniche sono nella revisione hashata e nelle transition history. |
| scheduler aging, fairness e anti-starvation | **coperto per la scheduler policy persistente corrente** | Aging e scheduler position sono calcolati dal kernel e persistiti nello snapshot. Il gate crea work in tick distinti tramite un fact prodotto autorevole, prova selezione del più anziano, position descent e servizio del successivo entro due claim; prova inoltre concorrenza, expiry, recovery worker e reload DB senza cambiare semantica scheduler. La fairness R4 generale delle invocation resta separatamente mancante nella matrice R4. |
| global collaborative completion | **coperto nel kernel domain; product path fail-closed** | Il test domain crea due participant con obligation/work necessari distinti: il terminale del primo non completa il goal e `GoalCompleted` resta distinto da `RunCompleted`. L'API product rifiuta senza residui una contribution con ID coincidenti ma priva di structured global grounding; sintesi/materializzazione globale restano aperte. |
| failure/termination dynamics e progress measure | **coperto nel kernel persistente** | Attempt bound, max-resolution deadline, suspended-claim recovery, terminal work/goal e run terminale sono distinti e storicizzati. |

## Matrice R5.33–R5.34: authority e information flow

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| permission engine/RLS/E2EE senza ACL parallele | **completo per le superfici esistenti** | Agent identity/device usa membership, device key, envelope e revocation esistenti. I nuovi oggetti ancora mancanti dovranno riusare lo stesso modello. |
| actor/controller/authority separati | **parziale** | I record sono distinti e gli effect verificano actor + envelope. Manca `workAuthorityPrincipal` persistito per WorkItem e run sponsor. |
| authority attenuation run→work→child | **solo domain** | `AuthorityEnvelope::is_subset_of` esiste; non ci sono work runtime/parent né certificate persistiti che impediscano amplification lungo continuation/delegation. |
| current permission/revocation | **coperto sui path invocation/Info/task completion/cross-owner enumerati** | Permission e runner/device key correnti sono rivalidati nella transaction dell'effect. Il materializer cross-owner richiede l'esatto `manage/full` dalla projection canonica (non creator metadata), e una revoca fra `ready` ed effect fallisce chiusa. Non è una prova globale: va esteso ai futuri effect/tool. |
| human task isolation e `DelegateAssignedWork` | **parziale** | `ResourceOperation` esiste, ma non esiste un materializer di nuova task delegata con source task invariata e causal provenance. |
| tool footprint resource-sensitive | **parziale** | Il proxy accetta `required_effects` nel piano ma non ha adapter registrati; invocation agent non persiste un tool security catalog verificabile. |
| authorized context e `contextSources` exact | **parziale forte; concretamente raffinato per i path linguistici 0031** | Il server ricostruisce ogni context da source prodotto correnti, ne rivalida la leggibilità per il principal della superficie e persiste un dispatch immutabile. L'endpoint TCB firma una observation distinta del request/exposure realmente eseguito e la projection viene accettata soltanto se coincide. Per interrogation `answer.contextSources` deve essere la stessa lista ordinata di `context.directSources`; stesso insieme in ordine diverso, duplicati, omissioni ed extra falliscono. Mancano ancora source transitive/tool-output e le superfici non abilitate. |
| disclosure audience intersection | **completo per Info effect corrente** | È verificata usando audience source/sink correnti. Mancano label/provenance transitivi persistiti sul body per rivalidare future audience expansion. |
| canonical resource body | **completo per storage esistente** | Il prodotto mantiene un solo ciphertext/versione per risorsa/Info, non varianti per-reader. |
| autonomous private/shared e contextual chat | **parziale** | Proxy chat è separata e creator-only; gli autonomous effect non persistono interaction mode/trust-circle classification e provenance mode-aware. |
| information readability ≠ action authority | **parziale** | I due gate sono distinti per Info/proxy; va provato per tutti i materializer e per work/tool. |

## Matrice R5.35: responsibility e governance

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| Responsibility administrator→user, user-level | **coperto** | API e chiave autorevole sono project+user+revision, con un solo draft/active corrente per user. Il gate PostgreSQL same-user/two-agent prova condivisione della stessa responsibility e isolamento del purge di un agent. |
| admin-controller senza self-responsibility artificiale | **coperto per initial creation** | La sola membership owner/admin non è un ramo LocalGoal. La creazione iniziale richiede una `ApprovedAdministratorAgentCreation` firmata e append-only, legata all'esatta proposta/agent/draft/LocalGoal/compiler/prompt/availability/scope. Questa provenance non è accettata per revisioni successive, non crea Responsibility e non contribuisce bottom-up. |
| responsibility E2EE + compiler certificato | **coperto per il path endpoint-TCB 0029** | Il source text resta E2EE sul device administrator autorizzato. Un compiler manifest/version pinning produce output/envelope chiusi e una dual signature DeviceSigning; il server verifica build abilitata, device/key non revocati, commitments, output/hash, scope/action catalog/bound e current governance/permission. Il manifest di protocollo è realmente hashato; non prova quale executable endpoint sia stato eseguito. |
| revision/history/provenance responsibility | **coperto** | Revision fencing, active/draft pointer, supersession esatta e audit user-level append-only sono persistiti. L'activation risolve `contract.supersedes_revision` contro l'active esatto e poi applica il validator domain (che richiede anche contiguità); il gate stale prova rollback e audit invariato. Purge agent/resource preserva Responsibility e audit dopo rivalidazione retention; sono verdi i test agent singolo e same-user/two-agent. |
| prompt/LocalGoal draft separato dall'active | **coperto** | Draft LocalGoal e draft prompt sono separati dagli active; retry stale fallisce senza audit parziale e la revisione precedente viene superseded soltanto usando l'esatto `supersedes_revision`. |
| requirements/GoalContract compilation bounded | **coperto per il compiler locale endpoint-TCB** | `PromptRequirement`, binding bidirezionali requirement/obligation/WorkSpec, security policy per WorkSpec, exact action/tool policy e massimi server-side sono validati prima della persistenza. Per `retryTool` il compiler certifica l'insieme esatto dei tool originali ammessi; il runtime 0033 deriva l'identità soltanto dalla ToolCall originale. Call assente o non exact resta fail-closed. |
| classifier deterministico LocalGoal | **coperto per GoalContract strutturati 0029** | Il server deriva esclusivamente dal GoalContract validato clause/domain/scope/WorkSpec binding, raggruppa e ordina per `WorkKind`, assegna ID deterministici e persiste versione/hash. Campi classifier forniti da client/model non sono accettati come authority. Il claim è sintattico/strutturale, non adeguatezza linguistica generale. |
| exact final prompt approval | **coperto per nuove activation certificate 0029 sotto endpoint-TCB** | Un atto dual-signed separato lega draft, agent principal, controller, LocalGoal/revision, compilation certificate, structured output hash e gli stessi exact plaintext/ciphertext commitments. Device/key, permission e governance sono rivalidati all'activation; mismatch e revoca rollbackano. L'uguaglianza del plaintext è una endpoint-TCB assumption: il server E2EE verifica commitment e firma, non vede il prompt. Le approval legacy restano `legacy_unverified` e non soddisfano il nuovo certificato. |
| activation atomica prompt + LocalGoal | **coperto per Responsibility revision e i due initial-creation path 0029** | Una sola transaction fenced persiste certificate/approval/provenance e attiva exact prompt+LocalGoal; initial creation include identity, membership, device/runner, governed agent e audit. Stale Responsibility/LocalGoal, signature, permission, key o provisioning mismatch rollbackano stato e audit senza identity/agent/prompt/LocalGoal parziali. |
| stessa governance alla creazione iniziale agent | **coperto per Responsibility e ApprovedAdministratorAgentCreation** | Test API completi esercitano user/controller con active compiled Responsibility e administrator creator con exact signed approval. `ApprovedLocalGoalException` e `GlobalMandateAssignment` hanno adapter solo per revisioni di agent esistenti e restano intenzionalmente fail-closed per initial creation; non esiste un ramo generico `ProjectAdministrator`. |
| rewrite/escalation consent | **concretamente raffinato per revisioni 0030** | Una draft fuori Responsibility può essere marcata `rewrite` oppure `requestAdministratorReview`; nessuna disposition concede authority o attiva parzialmente. Il consent dual-signed è exact-bound a review/user/source draft e `consented=false` non crea review. |
| admin exception review/decision | **concretamente raffinato per revisioni rejected, goal-only e goal+Responsibility** | Il trusted materializer crea una vera task R4 solo dopo l'exact consent e la lega causalmente alla review; task storiche/di altre review falliscono. Draft amministrative immutabili e decisioni terminali exact precedono una distinta final controller approval. I gate API provano rejection idempotente/non-authorizing, goal-only e l'activation atomica goal+Responsibility con rollback su approval stale. |
| certificate `administratorException` | **concretamente raffinato per revisioni; initial creation fail-closed** | `ApprovedLocalGoalException` è ricostruita da consent→review→exact R4 task terminale→admin draft/compiler→decisione. Il verified LocalGoal usa l'esatta `administratorException(reviewId)`; replay converge sul medesimo artifact deterministico ed equivocation confligge. |
| source-grounded global synthesis | **parziale, superficie esistente preservata** | Candidate strutturato da administrator client o runner autorizzato, source attive con Responsibility corrente e grounding non-amplifying sono verificati. I gate positivi coprono entrambi i boundary; LocalGoal con origine `administratorCreation` o `globalMandate` sono rifiutate come source bottom-up. La sintesi semantica/projection totale R5.40/R5.41 resta aperta; 0030 materializza soltanto l'assegnazione exact-bound a una global revision già autorevole. |
| global revisions/active pointer/history | **parziale** | Revision JSON append-only esiste, ma manca previous revision/origin/active state e workflow conflict. |
| LocalGoal clause→global work provenance | **parziale** | Contribution e grounding sono persistiti, ma la tabella source non conserva clause/work mapping in forma interrogabile completa. |
| bottom-up no feedback | **coperto per il path existing-agent GlobalMandate** | Una LocalGoal attivata da mandato resta rifiutata come source della sintesi bottom-up; il gate API verifica il rifiuto. |
| coverage need/existing agent/new least privilege | **parziale forte, path existing-agent concretamente raffinato** | Il need strutturato è exact-bound alla global revision/obligation. Le resource action concrete usano `GoalContract.scope`, con requirement scope exact e policy 0029; scope distinti producono footprint distinti. L'adeguatezza semantica plaintext→resource resta **EXTERNAL TCB ASSUMPTION** e tool identity non grounded fallisce chiusa. La proposta new-agent valida exact least privilege ma non crea identity/runner/grant/envelope. |
| certificate `globalMandate` | **concretamente raffinato per revisioni di agent esistenti projectDelegable; initial creation fail-closed** | Il verified ledger richiede l'esatto assignment event, global revision/contract/need/obligation, assignedBy autorevole, agent, LocalGoal/revision/origin, compilation certificate e payload LocalGoal. ID inesistente, altro mandato, LocalGoal diversa o revisione stale falliscono senza residui; replay esatto è idempotente. Current resource/tool permission e availability sono rivalidate all'activation e il mandato non concede authority. |

## Matrice R5.36–R5.37: proxy, interrogation, cross-owner e LLM boundary

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| un solo logical UserProxy per umano, N thread | **parziale** | Unique per project/user e più thread creator-only esistono; provisioning avviene lazy e richiede ID client, non per default per ogni umano. |
| proxy non-principal/no ACL/key/tool grant | **completo** | È mediation metadata legato all'identity utente. |
| transcript E2EE creator-only append-only | **parziale** | Request E2EE sono creator-only; manca una sequenza message completa human/proxy/tool con previous-message e append-only transcript history. |
| planning structured/bounded | **parziale forte** | Il percorso legacy deterministico resta valido. Il nuovo path model-mediated richiede una invocation `interpretProxyRequest` succeeded, exact-bound a request, principal, context, dispatch/observation firmata e artifact `UserProxyPlan` schema-closed; il piano passa poi dagli stessi validator Responsibility/permission esistenti. L'esecuzione completa degli effect proxy resta aperta. |
| footprint-derived responsibility | **parziale forte/fail-closed** | `action_classification` non è più accettato e un campo forgiato è rifiutato dallo schema. Il server deriva le classi dai `resource_effects` supportati; `tool_invocations` e operation ambigue sono incluse nella valutazione ma restano fail-closed finché manca un catalog adapter deterministico. |
| automatic in-responsibility / one-shot outside | **parziale** | Il gate e binding esatto confirmation esistono; nessun effect proxy viene materializzato e auditato come actor=user/mediatedBy. |
| interrogation human→agent | **parziale forte** | Per il controller del target esiste ora sessione/question E2EE creator-only → invocation `answerFromAuthorizedContext` → context corrente exact → observation endpoint firmata → answer append-only. Altri user/admin autorizzati dalla specifica restano non supportati. |
| interrogation agent→agent dedicated tool | **mancante** | Le sessioni rifiutano actor agent e non verificano ToolCall binding. |
| answer/context/provenance read-only forte | **concretamente raffinato per il path human-controller→agent 0031** | L'answer usa la lista ordinata exact del context ricostruito, una succeeded projection distinta dall'actual endpoint observation e un certifier causale. `resource_effect` è grounded a una vera `agent_effect_proposals` con exact invocation edge; l'answer route rifiuta e rollbacka ogni proposal. Tool invocation, prompt revision, LocalGoal revision, created Work, activated obligation e assigned task sono strutturalmente irraggiungibili dal call graph interrogation corrente e lo schema/writer DB rifiuta witness sintetici per tali categorie. Una mutation concorrente non causata dall'interrogation non invalida l'answer. Agent→agent resta mancante. |
| TaskIntent e obligation provenance persistiti | **coperto per la projection e i path implementati** | `TaskObligationProvenance` ha identità autonoma e collega task→agent principal→LocalGoal revision→obligation→WorkSpec. `TaskIntent` è opzionale nel bridge generico e obbligatorio nel governance contract cross-owner. Il ledger immutabile assegna una `semantic_position` totale; le semantic list ordinate live+retained provano `after = before ++ suffix` attraverso append, purge, replay, restart e interleaving controllati. |
| cross-owner automatic/review/reject | **coperto per routing e materialization task assignment** | Request, route, review, decision, `ready` ed effect sono persistiti. Il runtime riusa `route_cross_owner_assignment`; fuori governance è rejected, user-controller usa Responsibility e admin-controller project governance. Il materializer rivalida `manage/full`, exact TaskIntent/provenance/LocalGoal e idempotency hash-bound, crea una sola assignment e non crea grant/envelope. Revoca dopo `ready` impedisce l'effect. |
| prompt+LocalGoal attivi prima di assignment reviewed | **coperto** | Controller approval produce solo `approved_pending_mandate`. `ready` e materialization richiedono exact active prompt+LocalGoal, final-prompt certificate e provenance della specifica TaskIntent nella obligation/WorkSpec attiva; un'altra task con `AssignOwnTask` non basta. |
| superfici linguistiche strutturate R5.37 | **parziale, con due task kind model-mediated concretamente esercitati** | Responsibility/LocalGoal compiler restano deterministici nel boundary 0029/0030. Il runtime 0031/0032 esercita `answerFromAuthorizedContext` e `interpretProxyRequest` con schema chiuso, ID grounded, claim runtime-specifico, tentativi persistiti e failure esplicita. DeepSeek è live-testato sull'adapter e sul boundary edge per entrambi i task; Ollama è live-testato localmente. Non esiste però ancora un companion edge nativo né un E2E continuo provider→server/PostgreSQL: Mode A/B restano adapter/contract refined, non production-concrete. `summarizeGovernanceDecision`, `rewritePrompt` e gli altri adapter restano fail-closed. |
| no placeholder/provider permissivo | **completo** | I path senza adapter falliscono chiusi. |
| no model memory | **concretamente raffinato per le invocation 0031/0032; external provider assumption residua** | Sprout non persiste memoria cognitiva, session ID provider, embedding o cache semantica parallela. Ogni invocation ricostruisce source correnti; il client-provider edge esegue una request semanticamente self-contained per dispatch e non riusa conversation/session ID. Dispatch, signed actual observation e projection devono coincidere. Questo non attesta che il provider non conservi dati internamente. |
| operational histories append-only | **parziale forte complessiva; coperto per TaskIntent/provenance, governance e runtime linguistico 0031** | Oltre ai ledger precedenti, dispatch, endpoint observation, model projection, interrogation answer e causal edge sono append-only e retention-aware. App role/PUBLIC non hanno DML o EXECUTE sulle writer private; exact replay converge ed equivocation fallisce. Per TaskIntent/provenance resta valida la projection ordinata prefix-append-only. Transcript message completo e gli altri materializer R4 restano gap. |
| schema server-visible chiuso/no plaintext annidato | **parziale forte** | I tipi principali usano `deny_unknown_fields` e test negativi ricorsivi esistono. Alcuni tipi (`ResourceEffect`, proxy structs, source) non hanno schema chiuso uniforme; ogni nuovo tipo va chiuso e sottoposto a test ricorsivo. |

## Projection concreta del checkpoint 0029 verso R5.40/R5.41

| Cluster formale | Stato concrete del checkpoint | Limite esplicito |
| --- | --- | --- |
| `R541PromptRequirementsAndWorkExactCertificate` | **concretamente raffinato per LocalGoal compiler 0029** | Requirement, binding obligation/WorkSpec, action e tool policy sono exact e bidirezionali sul GoalContract strutturato. L'allineamento semantico del plaintext E2EE dipende dall'endpoint TCB autorizzato. |
| `R541ContractSecurityPolicyCertificate` | **concretamente raffinato per action staticamente risolvibili e retry runtime-grounded** | Ogni WorkSpec ha policy con ID esatto; resource operation e tool nominato devono coincidere ed essere nell'envelope server-side. `retryTool` conserva l'insieme tool certificato e il runtime 0033 richiede l'esatta ToolCall originale; una provenance assente o discordante resta fail-closed. |
| `ResponsibilityContractCompiledBy` / `ResponsibilityCompilationWithinEnvelope` | **concretamente raffinato per nuove revisioni certificate** | Administrator endpoint firma source/ciphertext commitments e output canonico; server rivalida rules, scope, action catalog, bound e current authority. Legacy resta leggibile ma non viene promosso a verified. |
| `LocalGoalClassifiedBy` / `LocalGoalCompilationWithinEnvelope` | **concretamente raffinato per nuove activation certificate** | Compiler endpoint produce requirements/contract; classifier server-side produce clauses autorevoli/versionate. Nessun campo classifier client/model entra nel gate. |
| `ControllerApprovalMatchesDraft` | **concretamente raffinato sotto endpoint-TCB assumption** | Final approval è un evento distinto, domain-separated e exact sui commitment della stessa draft/revision/compiler output. Non è una prova server-side dei byte plaintext. |
| `OperationalLocalRevisionActivationCertificate` | **concretamente raffinato per revisioni via Responsibility, exact administrator exception e existing-agent GlobalMandate** | Current Responsibility/exception/mandate, permission, membership, availability, compiler build e signing keys vengono rivalidati nella transaction. I gate API provano rollback stale/revoked senza state o audit parziali. Exception e GlobalMandate non sono ammessi per initial creation. |
| `OperationalAgentCreationActivationCertificate` | **concretamente raffinato per Responsibility e exact administrator creation** | Initial creation è atomica e non modifica permission/grant/key envelope/Responsibility/GlobalMandate. Administrator creation usa una approval append-only esatta, non il ruolo admin da solo. Exception/global mandate non sono materializzati. |
| surface inventory R5.40/R5.41 | **parziale** | Le record surface governance implementate sono persistite e non inventate. Global synthesis admin-client/runner, UserProxy planning e interrogation preesistenti restano abilitate nei rispettivi path testati, senza essere ampliate da 0029; comments, answer adapter e provider LLM restano mancanti/fail-closed. Non esiste ancora una projection totale di tutte le surface concrete alla root formale. |
| compiler build identity | **external TCB assumption documentata** | Il protocol manifest è versionato, hashato e pinning verificato. Non esiste prova che un particolare executable compiler riproducibile sia stato eseguito su un endpoint autorizzato, né protezione da endpoint TCB compromesso. |

## Projection concreta del checkpoint 0030

| Cluster formale | Stato concrete del checkpoint | Limite esplicito |
| --- | --- | --- |
| `LocalDraftRequiresRewriteOrEscalation` | **CONCRETELY REFINED per revisioni** | Disposition e consent sono persistiti senza attivazione o authority; initial creation resta fuori da questo workflow. |
| `ApprovedLocalGoalException` | **CONCRETELY REFINED per agent esistenti** | Catena exact consent→review→task R4→admin draft/decision→compiler→controller approval. `approvedGoalOnly` preserva la Responsibility; `approvedGoalAndResponsibility` attiva Responsibility+prompt+LocalGoal nello stesso boundary. Initial creation resta **FAIL-CLOSED / NOT YET IMPLEMENTED**. |
| `GlobalCoverageNeed` | **CONCRETELY REFINED strutturalmente; EXTERNAL TCB ASSUMPTION semanticamente** | Revision/obligation/footprint sono exact-bound. Il resource target concreto è `GoalContract.scope`, imposto anche ai requirement esatti; la correttezza semantica del target rispetto al plaintext E2EE non è provata dal server. Tool identity non grounded resta fail-closed. |
| `GlobalMandateAssignment` | **CONCRETELY REFINED per agent esistenti projectDelegable** | Assignment e verified ledger condividono exact event/revision/need/agent/LocalGoal/compiler provenance; current permissions/tool permissions/availability sono rivalidate e nessuna authority viene creata. Initial creation resta **FAIL-CLOSED / NOT YET IMPLEMENTED**. |
| `NewAgentForGlobalNeedProposal` | **CONCRETELY REFINED come proposta non-authorizing** | Footprint deve coincidere con il need; il path non crea principal, agent, runner, permission, grant, envelope o LocalGoal attivo. |
| global synthesis e liveness | **FAIL-CLOSED / NOT YET IMPLEMENTED oltre i path preesistenti** | 0030 non aggiunge provider LLM, algoritmo di sintesi, materializzazione globale completa o prova di liveness. |

## Projection concreta del checkpoint 0031

| Cluster formale | Stato concrete del checkpoint | Limite esplicito |
| --- | --- | --- |
| `StructuredLanguageModelRuntimeBoundary` | **CONCRETELY REFINED per `answerFromAuthorizedContext` e `interpretProxyRequest`** | Envelope schema-closed e bounded, retry entro `maxAttempts`, output tipizzato oppure explicit failure. Il runner deterministico di test prova il boundary; non esiste ancora un provider semantico production. |
| `StateGroundedModelInvocationCertificate` / `R540ModelRuntimeProjection` / `R540ModelInvocationEventExact` | **CONCRETELY REFINED per invocation succeeded 0031** | Context ricostruito server-side con permission corrente; dispatch, signed endpoint actual observation e persisted projection sono fonti distinte exact-bound. Work/claim/attempt sono richiesti quando l'invocation appartiene a una run. L'endpoint autorizzato e un futuro provider restano TCB esterni per plaintext e semantic fidelity. |
| `StateGroundedStrongInterrogationCertificate` | **CONCRETELY REFINED per human-controller→agent** | `resource_effect` è **CONCRETELY GROUNDED** su `agent_effect_proposals` più exact causal edge. `tool_invocation`, `prompt_revision`, `local_goal_revision`, `created_work`, `activated_obligation` e `assigned_task` sono **STRUCTURALLY UNREACHABLE FROM INTERROGATION**: l'answer path termina nella projection+answer e non chiama quei materializer; il DB non consente causal witness non grounded. Non è una prova per agent→agent o per user/admin non-controller. |
| UserProxy model-mediated plan | **CONCRETELY REFINED fino al planning/authorization** | Invocation succeeded exact-bound al request produce un piano entro candidate resources/operations/tools; Responsibility e permission restano deterministici e la confirmation non bypassa permission. Effect execution completo resta parziale. |
| R5.41 surface gates | **CONCRETELY REFINED per model/interrogation/proxy; FAIL-CLOSED per comment/disclosure** | Solo succeeded exact projection abilita `model`; interrogation richiede answer exact e read-only; proxy richiede plan model-mediated exact. Explicit failure e record legacy non abilitano le surface. Comment e disclosure restano `disabledFailClosed` con inventario vuoto perché manca una projection trace/tick content-exact. |
| no model memory | **CONCRETELY REFINED nel prodotto; EXTERNAL PROVIDER TCB ASSUMPTION** | Nessun model-memory store o provider session riusato; source dichiarate sono ricostruite a ogni invocation. Sprout non prova l'assenza di storage interno in un provider futuro. |
| comment surface | **FAIL-CLOSED / NOT YET IMPLEMENTED** | Il subsystem comment R5.41 exact trace/run/goal/tick non è stato inventato; `commentGate` resta disabled e vuoto. |
| disclosure surface | **FAIL-CLOSED / NOT YET IMPLEMENTED** | Nessun disclosure event sintetico: manca ancora actual sink payload ↔ trace/work/context projection content-exact. |

## Projection concreta del checkpoint 0032

| Cluster formale | Stato concrete del checkpoint | Limite esplicito |
| --- | --- | --- |
| runtime-kind e claim exact | **CONCRETELY REFINED nel server/DB** | `required_runtime_kind` è fissato alla queue: `/runner/claim` può reclamare solo `legacy_0031`, mentre `/runner/client-provider/claim` può reclamare solo `client_provider_v1`. La route client-provider pre-binda nel dispatch il commitment opaco del profilo; nessuna history 0031 viene promossa. |
| request/actual/projection exactness | **CONCRETELY REFINED per il boundary client-provider** | Il commitment copre protocollo versionato, metodo/path, body JSON realmente serializzato, modello nel wire e header semantici non segreti fissati dal protocol manifest. Dispatch, actual observation dual-signed e projection devono condividere runtime, endpoint commitment e profile commitment. Il server vede soltanto hash opachi. |
| provider attempts e replay | **CONCRETELY REFINED nel server/DB** | Una request provider per dispatch; timeout/malformed post-request conservano la witness, retry crea un nuovo attempt/lease/dispatch e non sovrascrive la storia. Auth/non-retryable termina al primo attempt ed exact replay non invoca di nuovo il provider. Il DB E2E prova due ordinal distinti e `maxAttempts` server-authoritative. |
| profilo locale e server-blind boundary | **CONCRETELY REFINED per storage/contratto** | Provider, modello, endpoint, credential, TLS pin e topology restano nel KeyVault locale e non sono sincronizzati/esportati. Il commitment è HMAC con secret device-only persistente e revisionato; nessuna API/schema backend contiene la configurazione. |
| `answerFromAuthorizedContext` / `interpretProxyRequest` | **LIVE ADAPTER + EDGE-BOUNDARY TESTED** | DeepSeek `deepseek-v4-flash` è il primary live cloud per entrambi i task; il test attraversa claim transport fixture, request reale, validation, actual observation e submit catturato. Non è un E2E continuo col server/PostgreSQL reale e manca il companion nativo, quindi non viene classificato production-concrete. |
| Mode A/B | **ADAPTER/CONTRACT REFINED; LIVE FEATURE TESTED; NOT PRODUCTION E2E** | OpenAI/Anthropic-compatible sono contract-tested; Ollama locale è live-testato con `qwen2.5:0.5b-instruct`. DS4 LAN development è live-testato sul modello esatto con discovery, strict structured generation, witness, cancellation e timeout; il protocollo capability-aware non invia `response_format` non dichiarato. L'interfaccia `LocalEdgeInferenceBridge` non ha ancora un'implementazione nativa user-owned, e Node `fetch` non prova la viabilità browser/CORS. Il trasporto DS4 HTTP di sviluppo non prova TLS/pinning production. |
| Mode C | **FAIL-CLOSED / NOT LIVE-VALIDATED** | Parsing reale IPv4 `/32` e IPv6 `/128`; nessun WireGuard, route change o transport remoto è stato attivato. |
| Mode D | **EXPERIMENTAL / NOT YET FORMALLY ENABLED** | Modulo privacy isolato e contract-tested senza fallback; manca la provenance exact della trasformazione composita e non abilita surface R5.41. |
| no hidden model memory | **CONCRETELY REFINED lato Sprout; EXTERNAL PROVIDER TCB ASSUMPTION** | Nessun conversation/session ID provider entra nel contesto o nello storage Sprout. La retention/stato interno del provider e l'identità dell'executable edge non sono attestati dal backend. |

## Projection concreta del checkpoint 0033

| Cluster formale | Stato concrete del checkpoint | Limite esplicito |
| --- | --- | --- |
| `ToolCallRecord` / `ToolCallWellFormed` / `ToolAuditEntry` | **CONCRETELY REFINED per il ledger operativo 0033** | Call corrente, storia per-attempt, dispatch, wire request, terminal observation, WorkOutcome e audit sono separati. Pending/succeeded/failed/timedOut hanno shape esatta; replay converge ed equivocation rollbacka. Il current row più snapshot audit immutabili raffina lo stato R4 operativo, non una projection temporale R540 completa. |
| `ToolReady` | **CONCRETELY REFINED per `web.read@1` e `document.local.read@1`** | Profile availability e runtime availability sono attestate da un witness device-signed short-lived exact su tool/version/manifest/runner/profile. Invoke/retry richiedono inoltre permission corrente actor, permission corrente `workAuthorityPrincipal`, WorkSpec policy e run/work ceiling. Catalog presence da sola non abilita il tool. |
| `runToolAuthorityBoundedAtStart` / `workAuthorityPrincipal` | **CONCRETELY REFINED per runSponsor e inheritedWork esatti** | Run ceiling deriva dall'AuthorityEnvelope certificato e non dalla union delle policy. Child ceiling è subset del parent/run ceiling. Origine umana possibile ma non completamente certificabile resta **FAIL-CLOSED** e non viene riclassificata. |
| `InvokeToolEffect` / `RetryToolEffect` | **CONCRETELY REFINED per gli executable v1** | Tool attempt e WorkAttempt sono la stessa coordinata. `requestedAt` è il tick server dell'action con `acquiredAt <= requestedAt < expiresAt`. Retry conserva call ID, tool, input e bounds, richiede un distinto WorkAttempt N+1 e rivalida tutti i gate correnti. Ogni attempt soddisfa direttamente `attempt < WorkSpec.maxAttempts`. |
| `ToolCompletionEffect` / `ToolFailureEffect` / `ToolTimeoutEffect` | **CONCRETELY REFINED** | Terminale atomico conserva l'esatto WorkOutcome attempt N. Permission/runtime/claim revocati dopo `requestedAt` non invalidano il terminale già causato. Server timeout parte dallo stato pending e copre anche edge morto prima del dispatch, senza firma o request sintetiche. Failure resta osservabile; il re-arm retry è una transizione successiva distinta. |
| `ToolSecuritySemantics.requiredEffects` | **CONCRETELY REFINED per read v1; PARTIAL per effetti esterni** | Required Sprout effects sono manifest-derived e vuoti soltanto per tool che non mutano `ResourceSecurityEffect`. `web.read` è comunque TR2/external network egress. `document.local.edit` resta contract-only **PARTIAL / EXTERNAL TCB**. |
| `ToolSecuritySemantics.outputReadableBy` / `toolContextSourceOwned` | **CONCRETELY REFINED per owner-only, singolo runner device** | `ToolOutput(callId)` non è una resource e non usa la run scope come source ID. Producer WorkAttempt e consumer WorkBinding possono differire; exact succeeded call/output, trusted principal audience ed envelope device-level corrente sono verificati separatamente. Il test effettua unwrap ibrido controllato. |
| local edge `web.read` / `document.local.read` | **LIVE FEATURE TESTED in development; PARTIAL production packaging** | GET/HEAD bounded, redirect/DNS/SSRF/no-cookie/no-auth e capability documento opaca text/Markdown sono esercitati su target controllati. Browser è control plane; manca ancora un companion nativo production-ready. Backend non esegue connector e non vede plaintext, path, endpoint o secret. |
| native Sprout surfaces | **STRUCTURALLY UNREACHABLE dal catalogo tool** | Task, TaskList, Topic, Info, Comment, permission e governance restano `AgentAction`/`ResourceOperation`; catalogo/domain/edge rifiutano alias. Comment concreto resta **FAIL-CLOSED / NOT IMPLEMENTED**. |
| mail/Telegram | **CONTRACT TESTED receive; FAIL-CLOSED send** | Profili locali cifrati e fake SPI read-only non sincronizzano col backend. Send resta bloccato perché manca un external `DisclosureSink` exact; nessun sink esistente viene riusato impropriamente. |
| `R540ToolEventExact` / R5.41 `toolGate` | **FAIL-CLOSED / NOT IMPLEMENTED** | L'audit operativo non possiede ancora una projection completa e indipendente con shared `traceId` e tutti i binding R540. `agent_r541_tool_surface_records` resta strutturalmente vuota e l'inventario dichiara `disabledFailClosed`; nessun claim di surface enabled. |
| DB trust/deployment | **CONCRETELY REFINED nei writer e test; PARTIAL nel provisioning production** | App role `NOSUPERUSER NOBYPASSRLS` non può fare DML diretto e usa writer privati exact-bound. Dev/CI usa ancora bootstrap/superuser e il provisioning least-privilege non è codificato nel deployment; RLS non protegge da owner/superuser compromesso. |

## Boundary realmente esterne

Queste sole categorie possono restare boundary, senza essere usate per rinviare
gli invarianti interni sopra elencati:

1. provider LLM concreto e sua disponibilità: per task fattibili deve restituire
   schema valido oppure failure esplicito entro il retry bound;
2. fedeltà linguistica intenzionale prompt→GoalContract,
   responsibility text→rules, classificazione semantica e qualità globale;
3. giudizio semantico delle sole evidence marcate `semanticJudgment`;
4. progress fisico di persone e condizioni realmente esterne, stabilità del
   segmento governance e assenza di terminali unsuccessful quando si pretende
   successo anziché sola terminazione;
5. algoritmo crittografico, secure key store e provider-specific data policy,
   preservando però il sistema E2EE/key envelope/revocation Sprout esistente;
6. scelta tecnica dell'algoritmo scheduler, del database, dei lock e del clock;
   finitezza, fairness derivabile, lease safety, recovery e completion restano
   comunque obblighi interni del concrete product.
