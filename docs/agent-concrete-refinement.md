# Concrete refinement della specifica agentica Lean

## Stato e sorgente normativa

La specifica Lean R5 validata è la sorgente normativa. Il kernel Rust e gli
adapter server implementano un refinement concreto dei suoi gate operativi; non
modificano gli enunciati e non affidano a un LLM permission, authority,
governance, provenance o decisioni sui side effect.

L'architettura target è un **personal/edge agent runner registrato come normale
device Sprout**. Il runner è soltanto un execution environment:

- l'agente è un principal distinto con membership ordinaria;
- il runner usa `devices`, `device_keys`, sessioni, resource key envelope e
  revocation già esistenti;
- Sprout non introduce una ACL o una key hierarchy parallela per gli agenti;
- il server conserva ciphertext e metadati strutturali minimi, mai private key o
  plaintext utente;
- ogni effect viene rivalidato dal server contro permission, authority,
  provenance, audience, key epoch e stato correnti prima della materializzazione.

## Confine E2EE e sintesi globale

Prompt, responsibility text, transcript, invocation input/output e contenuto
utente rimangono `EncryptedPayload`. Il plaintext viene ricostruito soltanto da
un client amministratore oppure da un runner autorizzato che possiede il normale
resource key envelope del proprio device. Le API non accettano campi descrittivi
plaintext nei contratti strutturali; gli oggetti globali e LocalGoal usano schema
chiuso (`deny_unknown_fields`).

In particolare, il backend **non ricostruisce semanticamente i LocalGoal e non
invoca un LLM per generare GlobalGoal/GlobalContract**:

1. il client amministratore o un edge runner autorizzato decritta le source;
2. l'eventuale LLM stateless produce un candidate strutturato entro uno
   `StructuredLanguageTaskEnvelope`;
3. il backend riceve il candidate già strutturato;
4. il backend valida deterministicamente revisioni, unicità della source attiva,
   provenance, responsibility operativa, grounding, bound, governance conflict
   e non-amplification;
5. soltanto dopo i gate il backend persiste candidate, groundings e hash.

Un candidate inviato da runner deve essere collegato a una invocation
`synthesize_global_contract` conclusa dallo stesso agent/device attivo. Un
candidate prodotto sul client amministratore non può dichiarare falsamente una
runner invocation. Il JSON visibile al server contiene soltanto ID, revisioni,
scope, dependency, action class e bound necessari alla validazione; il contenuto
semantico rimane nel payload cifrato dell'invocation.

## Refinement implementato

| Area normativa | Refinement concreto |
| --- | --- |
| principal e controller | agent e controller distinti; controller umano; identity agent marcata esplicitamente |
| runner | device `service` ordinario, key package normale, sessione revocabile e allowlist di protocollo fail-closed |
| authority | envelope finito, attenuation e intersezione con permission correnti |
| structured LLM task | schema chiuso, ID groundati, bound input/output/depth/retry, tool catalog esplicito |
| no model memory | ogni invocation dichiara source correnti; claim non contiene memory; hidden persistent memory obbligatoriamente falsa |
| responsibility | revisioni append-only per controller, scope reale e action class verificati deterministicamente |
| LocalGoal | un solo goal attivo per agente; revisione esatta; ownership, work e responsibility operativa verificati |
| global synthesis | candidate esterno già strutturato; source LocalGoal attive; provenance relazionale; grounding e non-amplification |
| UserProxy | metadata self-only, non principal e non soggetto ACL; permission e responsibility rivalidate |
| interrogation | transcript cifrato creator-only e causal delta completamente vuoto |
| information flow | audience del sink sottoinsieme dell'audience corrente di ogni source |
| side effect | proposal separata; permission, authority, device key, envelope, epoch, audience e optimistic version ricontrollati atomicamente |
| audit | log hash-chained append-only con actor, device, invocation e provenance; delete ammessa soltanto dal retention purge autorizzato |

Le foreign key composite impediscono di associare un'identity a un agent record
diverso e di usare un LocalGoal/revisione appartenente a un altro agente. Un
indice parziale garantisce una sola revisione LocalGoal attiva per agente. La
sintesi seleziona esclusivamente quella revisione attiva e richiede la
responsibility corrente dello stesso controller.

Le nuove foreign key sono tutte restrittive (`RESTRICT`/`NO ACTION`): nessuna
history viene rimossa per cascade implicito. Il normale retention purge calcola
prima gli ID agentici legati alla risorsa e li elimina esplicitamente in ordine
referenziale. I trigger append-only accettano soltanto gli ID marcati nella
stessa transazione con subject e lease retention validi. Il percorso revoca
sessione e key del runner, ritira il device e rimuove audit, global provenance,
interrogation, UserProxy collegati, effect, source, invocation, LocalGoal,
responsibility, runner e agent record prima della risorsa.

## Superfici server

Le migrazioni `0023` e `0024` aggiungono agent, runner, responsibility,
LocalGoal, invocation/source, effect proposal, global contract/source,
UserProxy, interrogation e audit. Tutte le tabelle hanno RLS; i check
deterministici applicativi restano necessari anche quando il processo di test
usa un ruolo PostgreSQL privilegiato.

Il solo materializer di side effect attualmente esposto è
`replace_info_document`: applica ciphertext compatibile col wire format Info e
non decritta il documento. Le normali mutation route restano vietate alle
sessioni agent; un runner non può usare il proprio bearer token per aggirare il
dispatcher centralizzato.

## Parti non ancora coperte e mismatch precisi

Il refinement di persistence/runtime è operativo, ma questi elementi richiedono
ulteriore prodotto e non sono simulati con placeholder permissivi:

- un binario edge runner e un adapter provider LLM concreti; provider, secure key
  store locale e policy di data processing non sono ancora scelti;
- classifier e compiler semantici di LocalGoal. Il server valida la struttura ma
  non finge di poter dimostrare semantic adequacy dal ciphertext;
- persistence dei certificati di `administratorException` e degli assignment
  `globalMandate`. Queste origin vengono rifiutate finché il relativo adapter non
  esiste;
- materializer deterministici per task, assignment, attachment e comment oltre a
  Info;
- tool catalog con adapter, required-effects e side-effect semantics verificati;
- scheduler autonomo, work/evidence lifecycle, cross-owner review workflow e UI
  dei gate di governance.

Queste assenze non concedono authority: i relativi ingressi falliscono chiusi.
Implementarle richiede adapter concreti ai sistemi Sprout esistenti, non nuove
ACL o decrittazione server-side.

## Assumption residue

Il kernel puro assume che gli adapter forniscano fatti correnti e autorevoli:

- principal kind e relazione controller/agent;
- ancestry da `resource_closure`;
- permission effettive e assignment speciali;
- active device key e disponibilità del resource key envelope;
- audience di source e sink;
- required effects deterministici di ogni tool;
- atomicità, RLS, retention e append-only persistence.

Gli adapter server implementati risolvono queste assumption per le superfici
sopra elencate. Nessun booleano dichiarato dal modello o dal client viene
considerato prova di permission, authority o avvenuta materializzazione.
