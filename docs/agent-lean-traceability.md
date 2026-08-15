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

## Matrice R4 e continuità R4 → R5

| Requisito Lean | Stato concrete iniziale | Gap verificato |
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
| `PromptObligationLiveness` e prompt corrente | **parziale** | Il prompt E2EE è persistito sul profilo agent e LocalGoal; non esiste activation atomica prompt/LocalGoal né lifecycle obligation/discharge. |
| `UniqueCommentNotifications` | **mancante** | Dipende dal sistema commenti assente. |
| `CorrectionProfileFromRun`, `Outcome`, strategy preference | **mancante** | Non esiste una run osservabile completa da cui derivare revision/comment counts; non va sostituita da metriche client-declared. |
| `ProjectsToR4`, `PreservesR4ValidRun`, `ResponsibleRun` | **mancante** | Il runtime R5 corrente non costruisce una proiezione R4 completa; questa continuità deve diventare un invariante degli eventi agentici, non una dichiarazione documentale. |

## Matrice R5.30–R5.32: completion kernel

| Requisito Lean | Stato concrete iniziale | Gap verificato |
| --- | --- | --- |
| `GoalContract` DSL completa | **parziale** | Rust persiste scope, obligation, dependency e WorkSpec semplificate. Mancano goal identity/status, `ContractCondition` per activation/required/completion, evidence rules, waiting rules e completion condition normalizzata. |
| `GoalContractWellFormed` | **parziale** | Sono validati ID unici, dependency rank, entry cardinality, bounds, continuation e alternative rank. Mancano owner=obligation, evidence/wait subject validity, evidence per ogni obligation, discharge rule membership e condition normalization. |
| revisioni autorevoli/program snapshot | **parziale** | Local/global JSON sono append-only e revisionati. Manca una run/program snapshot attiva con fencing e history del goal status. |
| `ObligationInstance` e birth closure | **mancante** | Nessuna tabella/API/runtime per istanze active/discharged; nessuna materializzazione atomica delle required obligation. |
| `WorkItem`, slot certificate, canonical work universe | **mancante** | Esistono soltanto WorkSpec statiche nel JSON. Mancano slot `(workSpec,maxInstances)`, ID canonici, status runtime, parent e source comment. |
| activation, eligibility e work existence | **mancante** | Nessun evaluator deterministico delle condition/dependency, entry slot 0 o frontier completeness. R5.30.11 lo dichiara interno. |
| waiting rules e typed blocker | **mancante** | Nessuna persistence di blocker scope/condition/status/rule certificate o risoluzione meccanica. |
| dispatch e scheduler position | **mancante** | La coda invocation non è dispatch di WorkItem e non persiste posizione/aging. |
| claim/lease, esclusività, expiry, recovery | **parziale** | È completo per una invocation isolata; manca il binding a WorkItem/attempt, claim certificate, recovery dello stesso work ID e invalidazione di terminal effects dopo expiry. |
| retry generation e failure continuation | **parziale** | Retry della stessa invocation è bounded. Non vengono eseguiti `alternatives`, `dischargeBy`, `failGoal` o continuation WorkSpec; il client dichiara inoltre `retryable`. |
| evidence meccanica/semantica e provenance | **mancante** | Output cifrato ed effect proposal non sono `Evidence`; mancano subject tipizzato, verification mode, causal binding ed evidence judge separato. |
| discharge e accepted-evidence closure | **mancante** | Nessun endpoint/state transition obligation discharge; tool success non deve implicarlo. |
| `CompletionCriterion` bookkeeping | **mancante** | Nessuna chiusura atomica che richieda tutte le obligation required discharged, nessun work aperto e nessun blocker waiting. |
| `RunCompleted ≠ GoalCompleted` | **mancante** | Invocation succeeded è oggi l'unico terminale operativo esposto; non esistono run e goal status distinti. |
| causal graph globale | **mancante** | Nessuna persistence di nodi/link obligation/work/comment/task/tool/blocker e rank causale. |
| finitezza e anti-loop multi-agent | **solo domain parziale** | I bounds/rank WorkSpec sono validati, ma non esistono slot certificate e causal graph runtime dai quali derivare finitezza e well-foundedness. |
| scheduler aging, fairness e anti-starvation | **mancante** | Nessun `AgingSchedulerPolicy`/position descent persistito o testabile. È target interno, pur restando libera la scelta dell'algoritmo concreto. |
| global collaborative completion | **mancante** | Il candidate globale è persistibile, ma non esiste run con participant e closure di tutto il work/evidence/handoff globale. |
| failure/termination dynamics e progress measure | **mancante** | Nessun max-resolution deadline per WorkItem, rank di progresso o distinzione terminal/success. |

## Matrice R5.33–R5.34: authority e information flow

| Requisito Lean | Stato concrete iniziale | Gap verificato |
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

| Requisito Lean | Stato concrete iniziale | Gap verificato |
| --- | --- | --- |
| Responsibility administrator→user, user-level | **parziale/non conforme** | La chiave logica è già user-level, ma l'API è innestata sotto un agent e impone che lo user sia quel controller. Retention agent elimina responsibility col profilo. Deve diventare lifecycle user-level indipendente dagli agenti. |
| admin-controller senza self-responsibility artificiale | **non conforme** | LocalGoal richiede sempre una responsibility corrente; va usata governance amministrativa del progetto per controller administrator. |
| responsibility E2EE + regole strutturali minime | **parziale** | Source text è E2EE e rules server-visible. Mancano draft/active/superseded state, compilation envelope/certificate e server-derived compiler binding. |
| revision/history/provenance responsibility | **parziale** | Revision append-only e admin/user invarianti esistono. Manca active pointer/state e isolamento retention corretto. |
| prompt/LocalGoal draft separato dall'active | **mancante** | L'endpoint inserisce direttamente `active` e supersede la precedente revisione. |
| requirements/GoalContract compilation bounded | **solo domain parziale** | Envelope generico esiste; mancano PromptRequirement/binding, compilation record e validator/API specifici R5.37D. |
| classifier deterministico LocalGoal | **non conforme** | `clauses` domain/scope arrivano dal client e sono usate nel gate; non c'è output classifier persistito/provenance. |
| exact final prompt approval | **mancante** | Nessun record/certificate che leghi controller, draft, ciphertext esatto e local revision. |
| activation atomica prompt + LocalGoal | **non conforme** | Viene attivato solo LocalGoal; `governed_agents.encrypted_system_prompt` non è aggiornato atomicamente. |
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

| Requisito Lean | Stato concrete iniziale | Gap verificato |
| --- | --- | --- |
| un solo logical UserProxy per umano, N thread | **parziale** | Unique per project/user e più thread creator-only esistono; provisioning avviene lazy e richiede ID client, non per default per ogni umano. |
| proxy non-principal/no ACL/key/tool grant | **completo** | È mediation metadata legato all'identity utente. |
| transcript E2EE creator-only append-only | **parziale** | Request E2EE sono creator-only; manca una sequenza message completa human/proxy/tool con previous-message e append-only transcript history. |
| planning structured/bounded | **parziale** | Envelope e piano chiuso sono validati e persistiti; manca output/execution lifecycle completo. |
| footprint-derived responsibility | **non conforme** | L'API accetta `action_classification` dal client. Controlla coerenza parziale con gli effect, ma non usa un classifier deterministico product-side per tool/operation→action class. |
| automatic in-responsibility / one-shot outside | **parziale** | Il gate e binding esatto confirmation esistono; nessun effect proxy viene materializzato e auditato come actor=user/mediatedBy. |
| interrogation human→agent | **parziale** | Sessione/transcript E2EE creator-only e delta vuoto esistono; è limitata al controller del target, mentre la specifica consente user/admin senza control requirement. |
| interrogation agent→agent dedicated tool | **mancante** | Le sessioni rifiutano actor agent e non verificano ToolCall binding. |
| answer/context/provenance read-only forte | **parziale** | Delta vuoto è validato alla creazione, ma non esiste answer submission, state-grounded context exact o causality comparison prima/dopo. |
| TaskIntent e obligation provenance persistiti | **solo domain** | Tipi e routing puro esistono; nessuna tabella/API crea e valida i record contro task e LocalGoal attivo. |
| cross-owner automatic/review/reject | **solo domain** | Il router puro implementa le tre classi per user-controller; manca admin-controller project governance, request validation, persistence, review task e assignment gate. |
| prompt+LocalGoal attivi prima di assignment reviewed | **mancante** | Nessun workflow cross-owner concreto. |
| superfici LLM structured R5.37 | **parziale** | Kind/envelope generico schema-closed esiste. Mancano envelope/output specifici persistiti per requirements, responsibility compilation, TaskIntent, summaries e interrogation answer. |
| no placeholder/provider permissivo | **completo** | I path senza adapter falliscono chiusi. |
| no model memory | **parziale forte** | Non esiste store cognitivo parallelo; invocation source correnti persistite e rivalidate. Manca una attestazione dell'adapter runner che exposure reale = source dichiarate e ricostruzione per tutte le future superfici. |
| operational histories append-only | **solo domain/parziale DB** | Audit agent è append-only; `SemanticOperationalState` non include transcript e i task intent/provenance non sono persistiti. |
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

