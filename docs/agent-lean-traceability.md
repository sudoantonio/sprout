# Tracciabilità Sprout AgentSpec R5 → concrete product

## Fonte normativa e metodo

La fonte normativa di questo audit è
`Sprout_AgentSpec_R5_no_model_memory_draft.lean`, verificata byte-per-byte con
SHA-256
`7e7aa3162a8b44d9c12de1b28a4af6506d189558c37d7e6d5417898c12ade714`.
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

Questa è la matrice di partenza all'HEAD
`83bc6def6132fa87f595725dade64ed4e65963ea`; verrà aggiornata dopo ogni
incremento.

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
suite DB-enabled `agent_completion_gates` (quattro test, esecuzione seriale):

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
  di entrambi consentono prima `GoalCompleted` e poi `RunCompleted`.

La fixture dell'ultimo gate prepara un ledger prodotto già causalmente legato
`claim transition → invocation succeeded → applied task effect → task
completion`; i trigger DB e gli adapter API lo rivalidano. Questo non colma il
materializer task agentico ancora assente: evidence/discharge rimangono quindi
classificati fail-closed sotto.

## Matrice R4 e continuità R4 → R5

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| `ApiBoundary`, actor/session e `Move.agentMove` | **parziale** | Sessioni e identity agent sono distinte e validate; manca una proiezione persistente uniforme delle mosse R4 che attribuisca ogni task/comment/tool effect all'actor agentico senza confonderlo col controller. |
| `WellFormedState`, permission e frame conditions | **parziale** | Permission/RLS/E2EE esistenti sono riusati e gli agenti non possono usare le normali mutation route. Il dispatcher copre solo Info e non rappresenta tutte le `AgentAction`; non esiste quindi ancora una verifica delle frame condition su tutto il linguaggio R4. |
| Task R4: create/replace/delete/assign/unassign/done/note/attachment | **parziale** | Le operazioni umane esistono nel prodotto. Gli equivalenti agentici governati, con provenance/authority/work binding e task completion causality, non sono materializzabili salvo Info. |
| `Comment`, `PostCommentEffect`, `CommentAdmissible` | **mancante** | Non esiste persistence/API commenti human/admin/agent→agent nel contesto risorsa. Mancano recipient, E2EE payload, parent/depth, unique root/notification, eventi e audit. |
| `CommentPriorityDiscipline` administrator > user > agent | **mancante** | Nessuna coda/response runtime dei commenti e nessun ordinamento persistito. |
| `ToolCallRecord`, `ToolAuditEntry`, retry/timeout | **parziale** | `agent_invocations` ha lease, expiry e retry bounded del provider LLM. Non è un tool catalog R4: mancano ToolCallId tipizzato, input/output tool, required-effects adapter, audit requested/retry/completed/failed/timedOut e bridge work→tool. |
| `Activates`, `TriggerResponsiveness` | **mancante** | Non esiste event/trigger dispatcher agentico per resource update, comment e tool terminal event. |
| scheduler/runtime fairness R4 | **mancante** | `SKIP LOCKED ORDER BY created_at` seleziona invocation, ma non certifica agent fairness, runtime fairness, responsiveness o anti-starvation R4. |
| `TaskCompletionCausality`, assigned-task liveness | **parziale** | Le mutation task sono auditabili nel prodotto generale; il percorso agentico non conserva un causal link R4/R5 e non implementa liveness delle task assegnate. |
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
| claim/lease, esclusività, expiry, recovery | **coperto nel runtime persistente** | Unique active claim per work, unique attempt, lock serializable, authority/runner corrente prima di claim/effect e worker scheduler-only per recovery bounded. I 300 secondi sono soltanto il default operativo: `claim_next` limita la lease effettiva alla `WorkSpec.maxResolutionTicks` certificata, come richiesto dalle dynamics R5.30, e il certificato DB conserva quel deadline. Il gate PostgreSQL concorrente prova un solo vincitore, rifiuto dell'effect dopo expiry e recovery persistente sul medesimo WorkItem con attempt successivo. |
| retry generation e failure continuation | **parziale forte** | `retrySame`, alternative e `failGoal` sono transition domain persistite con attempt/continuation canoniche. `dischargeBy` è validato dal kernel ma l'API failure non può ancora collegare una evidence autorevole e quindi fallisce chiuso. |
| evidence meccanica/semantica e provenance | **parziale fail-closed** | Evidence è schema-closed e derivata dal server. Una task completion vale solo dopo un binding preesistente `claim transition→invocation→applied effect→task resource`; stesso agent/scope/tempo non basta. Poiché manca ancora il materializer task agentico che crea quel binding, l'adapter API reale non può oggi produrlo. Semantic judgment resta boundary esterna ma non ha placeholder permissivi. |
| discharge e accepted-evidence closure | **parziale fail-closed** | Il kernel discharge soltanto tramite `accept_evidence`; il certificato DB richiede outcome causale, rule/mechanical mode e snapshot con obligation discharged. Il percorso task diventerà raggiungibile solo dal futuro materializer autorevole, non da una scelta runner. |
| `CompletionCriterion` bookkeeping | **coperto nel runtime persistente** | Il runtime rivaluta facts, obligation required, work corrente e blocker nella transaction della transition terminale. Il gate API/DB prova il rifiuto con tutti e tre ancora aperti, hash/version/status immutati dopo rollback e commit finale coerente. |
| `RunCompleted ≠ GoalCompleted` | **coperto nel runtime persistente** | `goal_status` e `run_status` sono distinti e il DB vieta `run=completed` con goal non completed. I gate osservano persistentemente `goal=completed, run=running` prima della transition finale e `goal=completed, run=completed` soltanto dopo la commit atomica. |
| causal graph globale | **parziale** | Link domain e certificati relazionali append-only esistono per i nodi generati dal kernel; comment/tool e task-effect R4 non ancora materializzabili restano gap, non link sintetici. |
| finitezza e anti-loop multi-agent | **coperto nel kernel/persistence** | Slot finiti, rank di generation/dependency, bounds e identità canoniche sono nella revisione hashata e nelle transition history. |
| scheduler aging, fairness e anti-starvation | **coperto per la scheduler policy persistente corrente** | Aging e scheduler position sono calcolati dal kernel e persistiti nello snapshot. Il gate crea work in tick distinti tramite un fact prodotto autorevole, prova selezione del più anziano, position descent e servizio del successivo entro due claim; prova inoltre concorrenza, expiry, recovery worker e reload DB senza cambiare semantica scheduler. La fairness R4 generale delle invocation resta separatamente mancante nella matrice R4. |
| global collaborative completion | **coperto nel kernel persistente** | Il gate crea una run dal GlobalContract corrente con due agent participant e obligation/work necessari distinti. Il terminale del primo non completa goal/run; dopo outcome ed evidence causale di entrambi il goal diventa completed ma la run resta running fino alla commit amministrativa finale. |
| failure/termination dynamics e progress measure | **coperto nel kernel persistente** | Attempt bound, max-resolution deadline, suspended-claim recovery, terminal work/goal e run terminale sono distinti e storicizzati. |

## Matrice R5.33–R5.34: authority e information flow

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| permission engine/RLS/E2EE senza ACL parallele | **completo per le superfici esistenti** | Agent identity/device usa membership, device key, envelope e revocation esistenti. I nuovi oggetti ancora mancanti dovranno riusare lo stesso modello. |
| actor/controller/authority separati | **parziale** | I record sono distinti e gli effect verificano actor + envelope. Manca `workAuthorityPrincipal` persistito per WorkItem e run sponsor. |
| authority attenuation run→work→child | **solo domain** | `AuthorityEnvelope::is_subset_of` esiste; non ci sono work runtime/parent né certificate persistiti che impediscano amplification lungo continuation/delegation. |
| current permission/revocation | **completo per invocation/Info** | Permission, active device key, resource envelope ed epoch sono rivalidati a queue/claim/submit/apply. Va esteso a ogni nuovo effect/tool/work. |
| human task isolation e `DelegateAssignedWork` | **parziale** | `ResourceOperation` esiste, ma non esiste un materializer di nuova task delegata con source task invariata e causal provenance. |
| tool footprint resource-sensitive | **parziale** | Il proxy accetta `required_effects` nel piano ma non ha adapter registrati; invocation agent non persiste un tool security catalog verificabile. |
| authorized context e `contextSources` exact | **parziale** | Invocation source sono persistite, correntemente leggibili e dichiarate; l'exposure exact è costruita uguale lato server anziché attestata dall'execution adapter. Mancano transitive sources e tool-output audience. |
| disclosure audience intersection | **completo per Info effect corrente** | È verificata usando audience source/sink correnti. Mancano label/provenance transitivi persistiti sul body per rivalidare future audience expansion. |
| canonical resource body | **completo per storage esistente** | Il prodotto mantiene un solo ciphertext/versione per risorsa/Info, non varianti per-reader. |
| autonomous private/shared e contextual chat | **parziale** | Proxy chat è separata e creator-only; gli autonomous effect non persistono interaction mode/trust-circle classification e provenance mode-aware. |
| information readability ≠ action authority | **parziale** | I due gate sono distinti per Info/proxy; va provato per tutti i materializer e per work/tool. |

## Matrice R5.35: responsibility e governance

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| Responsibility administrator→user, user-level | **coperto** | API e chiave autorevole sono project+user+revision, con un solo draft/active corrente per user. Il gate PostgreSQL same-user/two-agent prova condivisione della stessa responsibility e isolamento del purge di un agent. |
| admin-controller senza self-responsibility artificiale | **coperto** | L'activation LocalGoal usa la membership owner/admin e il permission engine corrente; il test API dimostra activation di un agent admin-controlled con zero self-Responsibility. |
| responsibility E2EE + regole strutturali minime | **parziale forte** | Source text resta E2EE, rules strutturali schema-closed e lifecycle draft/active/superseded sono persistiti. Compilation linguistica/certificate da runner autorizzato non ha ancora un adapter concreto e resta fail-closed, come boundary provider R5.37. |
| revision/history/provenance responsibility | **coperto** | Revision fencing, active/draft pointer, supersession esatta e audit user-level append-only sono persistiti. L'activation risolve `contract.supersedes_revision` contro l'active esatto e poi applica il validator domain (che richiede anche contiguità); il gate stale prova rollback e audit invariato. Purge agent/resource preserva Responsibility e audit dopo rivalidazione retention; sono verdi i test agent singolo e same-user/two-agent. |
| prompt/LocalGoal draft separato dall'active | **coperto** | Draft LocalGoal e draft prompt sono separati dagli active; retry stale fallisce senza audit parziale e la revisione precedente viene superseded soltanto usando l'esatto `supersedes_revision`. |
| requirements/GoalContract compilation bounded | **solo domain parziale** | Envelope generico esiste; mancano PromptRequirement/binding, compilation record e validator/API specifici R5.37D. |
| classifier deterministico LocalGoal | **non conforme** | `clauses` domain/scope arrivano dal client e sono usate nel gate; non c'è output classifier persistito/provenance. |
| exact final prompt approval | **coperto per activation manuale** | `agent_prompt_revisions` lega ciphertext+hash, agent, LocalGoal id/revision e approver. L'activation controlla nuovamente l'esatto ciphertext/hash e persiste approver/audit nella stessa transaction; compilation/approval mediata da futuro adapter LLM non è simulata. |
| activation atomica prompt + LocalGoal | **coperto** | Una sola transaction fenced rivalida authority/permission/responsibility e richiede `rows_affected = 1` per predecessor LocalGoal/prompt, draft LocalGoal/prompt e update dell'esatto ciphertext in `governed_agents`; ogni mismatch rollbacka anche gli audit. |
| stessa governance alla creazione iniziale agent | **non conforme** | Provisioning crea agent/prompt prima di LocalGoal e responsibility authorization. |
| rewrite/escalation consent | **mancante** | Nessuna draft disposition, coverage diff, consent o summary bounded. |
| admin exception review/decision | **mancante** | Nessuna review task, editable draft, decision rejected/goal-only/goal+responsibility o final controller approval. |
| certificate `administratorException` | **mancante/fail-closed** | L'origine è rifiutata, non implementata. |
| source-grounded global synthesis | **parziale** | Candidate strutturato da client/runner, source attive e grounding non-amplifying sono verificati. |
| global revisions/active pointer/history | **parziale** | Revision JSON append-only esiste, ma manca previous revision/origin/active state e workflow conflict. |
| LocalGoal clause→global work provenance | **parziale** | Contribution e grounding sono persistiti, ma la tabella source non conserva clause/work mapping in forma interrogabile completa. |
| bottom-up no feedback | **completo nel validator corrente** | `globalMandate` non può contribuire; manca però il workflow che crea mandate validi. |
| coverage need/existing agent/new least privilege | **solo domain parziale** | Sono modellati alcuni tipi Lean-equivalenti indiretti, senza persistence/API/runtime. |
| certificate `globalMandate` | **mancante/fail-closed** | L'origine è rifiutata. |

## Matrice R5.36–R5.37: proxy, interrogation, cross-owner e LLM boundary

| Requisito Lean | Stato concrete corrente | Gap verificato |
| --- | --- | --- |
| un solo logical UserProxy per umano, N thread | **parziale** | Unique per project/user e più thread creator-only esistono; provisioning avviene lazy e richiede ID client, non per default per ogni umano. |
| proxy non-principal/no ACL/key/tool grant | **completo** | È mediation metadata legato all'identity utente. |
| transcript E2EE creator-only append-only | **parziale** | Request E2EE sono creator-only; manca una sequenza message completa human/proxy/tool con previous-message e append-only transcript history. |
| planning structured/bounded | **parziale** | Envelope e piano chiuso sono validati e persistiti; manca output/execution lifecycle completo. |
| footprint-derived responsibility | **parziale forte/fail-closed** | `action_classification` non è più accettato e un campo forgiato è rifiutato dallo schema. Il server deriva le classi dai `resource_effects` supportati; `tool_invocations` e operation ambigue sono incluse nella valutazione ma restano fail-closed finché manca un catalog adapter deterministico. |
| automatic in-responsibility / one-shot outside | **parziale** | Il gate e binding esatto confirmation esistono; nessun effect proxy viene materializzato e auditato come actor=user/mediatedBy. |
| interrogation human→agent | **parziale** | Sessione/transcript E2EE creator-only e delta vuoto esistono; è limitata al controller del target, mentre la specifica consente user/admin senza control requirement. |
| interrogation agent→agent dedicated tool | **mancante** | Le sessioni rifiutano actor agent e non verificano ToolCall binding. |
| answer/context/provenance read-only forte | **parziale** | Delta vuoto è validato alla creazione, ma non esiste answer submission, state-grounded context exact o causality comparison prima/dopo. |
| TaskIntent e obligation provenance persistiti | **coperto per il governance contract** | `agent_task_intents` e provenance esatta TaskIntent→task→agent→LocalGoal revision→obligation→WorkSpec sono persistiti. Facts e route sono ricostruiti server-side; la provenance è append-only salvo purge retention autenticato e causalmente scoped. |
| cross-owner automatic/review/reject | **parziale forte** | Request, route, review task, decision e stato sono persistiti; il runtime riusa `route_cross_owner_assignment` con facts autorevoli. Fuori governance è rejected, user-controller usa Responsibility e admin-controller project governance. Manca ancora il materializer che esegue l'assegnazione task dopo lo stato `ready`, quindi nessun placeholder la simula. |
| prompt+LocalGoal attivi prima di assignment reviewed | **parziale forte/fail-closed** | Controller approval produce solo `approved_pending_mandate`. `ready` richiede exact active prompt+LocalGoal e provenance della specifica TaskIntent nella obligation/WorkSpec attiva; un'altra task con `AssignOwnTask` non basta. L'effetto assignment resta indisponibile finché manca il materializer autorevole. |
| superfici LLM structured R5.37 | **parziale** | Kind/envelope generico schema-closed esiste. Mancano envelope/output specifici persistiti per requirements, responsibility compilation, TaskIntent, summaries e interrogation answer. |
| no placeholder/provider permissivo | **completo** | I path senza adapter falliscono chiusi. |
| no model memory | **parziale forte** | Non esiste store cognitivo parallelo; invocation source correnti persistite e rivalidate. Manca una attestazione dell'adapter runner che exposure reale = source dichiarate e ricostruzione per tutte le future superfici. |
| operational histories append-only | **parziale forte** | Audit agent e user-governance, TaskIntent e obligation provenance sono persistiti; provenance e audit rifiutano mutation dirette. Il DELETE provenance è ammesso solo dal retention workflow rivalidato sul subject esatto. Transcript/message history completa e materializer R4 restano gap. |
| schema server-visible chiuso/no plaintext annidato | **parziale forte** | I tipi principali usano `deny_unknown_fields` e test negativi ricorsivi esistono. Alcuni tipi (`ResourceEffect`, proxy structs, source) non hanno schema chiuso uniforme; ogni nuovo tipo va chiuso e sottoposto a test ricorsivo. |

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
