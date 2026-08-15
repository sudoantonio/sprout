# Concrete refinement della specifica agentica Lean

## Stato

Questo documento descrive il confine fra la specifica formale R5 e il prodotto
Sprout corrente. La specifica Lean validata resta la sorgente normativa; il
codice Rust non ne modifica gli enunciati e non sostituisce le proof obligation
con decisioni affidate a un modello linguistico.

Il primo incremento implementato in `crates/domain/src/agents.rs` è un kernel
deterministico, privo di I/O e indipendente dal provider LLM. Il kernel deve
essere chiamato prima di persistere o materializzare qualunque proposta del
modello.

## Refinement implementato nel kernel Rust

| Area Lean | Refinement concreto |
| --- | --- |
| `PrincipalKind`, `GovernedAgentRecord` | agent e controller distinti; controller obbligatoriamente umano |
| `AuthorityEnvelope`, attenuation | ceiling immutabile per risorse/tool, subset transitivo e intersezione con permission correnti |
| `StructuredLanguageTaskEnvelope` | schema chiuso, ID groundati, bound input/output/depth/retry e divieto di delegare permission/proof al modello |
| `ResponsibilityContract` | snapshot revisionato, regole finite per domain/scope/action e controllo amministrativo dello scope |
| `GoalContract`, `LocalGoalContract` | obligation/work finiti, entry unica, rank decrescenti, owner agent esatto e classificazione completa |
| responsibility vs LocalGoal | gate separati; la responsibility non concede runtime permission |
| sintesi globale | ogni work globale deve essere groundato a work locale attivo e autorizzato senza amplificarne owner, action o bound |
| `UserProxy` | mediation metadata legata allo user, mai principal o soggetto ACL indipendente |
| proxy execution | envelope, request/thread binding e permission/tool check correnti precedono l'eventuale conferma one-shot |
| cross-owner | automatico solo con provenance esatta verso obligation locale attiva; altrimenti review se coperto dalla responsibility, oppure reject |
| interrogation | transcript leggibile solo dal creator e causal delta completamente vuoto |
| information flow | audience del sink sottoinsieme dell'audience di ogni source |
| state-grounded invocation | source correntemente leggibili, exposure esatta e flag di memoria persistente nascosta obbligatoriamente falso |
| audit/provenance | record tipizzati e verifica append-only dello stato operativo |

Queste verifiche non sono autorizzazioni alternative. Il loro adapter server
deve continuare a interrogare `require_resource_access`, membership, permission
gerarchiche, RLS, resource epoch e key envelope già esistenti.

## Mismatch architetturale bloccante: plaintext e key custody

La specifica Lean lascia intenzionalmente `EncryptedPayload`, envelope e key
management come astrazioni dell'ambiente di integrazione. Il prodotto concreto,
invece, dichiara nel threat model che il server non vede il plaintext e non
possiede le chiavi delle risorse.

Un LLM deve ricevere plaintext per interpretare prompt, task, transcript e
source. Non è quindi possibile collegare onestamente un provider LLM al worker
server corrente senza scegliere dove avvengono decrittazione e inferenza.
Inviare ciphertext al modello non implementa la specifica. Consegnare al server
le chiavi esistenti indebolirebbe E2EE e violerebbe il threat model.

Prima di introdurre tabelle di invocation, code worker o API di esecuzione deve
essere scelta e revisionata una delle seguenti architetture:

1. **Agent runner sul dispositivo del controller.** Il browser/app autorizzato
   ricostruisce e decritta il context, invoca il provider e invia soltanto output
   strutturato al validator server. Preserva il modello E2EE, ma l'autonomia
   schedulata richiede che un device fidato sia online.
2. **Runner personale/edge separato.** Un daemon controllato dall'utente viene
   registrato come device Sprout, riceve normali key envelope e mantiene le chiavi
   fuori dal server. Preserva meglio autonomia ed E2EE, ma introduce un nuovo
   deployable, provisioning, revocation e protocollo di attestazione.
3. **Agent device custodito dal servizio.** Ogni agente è un principal/device e
   riceve key envelope normali, ma il servizio custodisce le sue private key.
   Consente worker sempre attivi, però rende il servizio capace di leggere le
   risorse concesse all'agente e richiede una modifica esplicita del threat model,
   isolamento delle chiavi e nuove procedure operative.

Anche la destinazione del plaintext presso un provider LLM esterno richiede una
decisione di data classification, retention e consenso. Il divieto di memoria
cognitiva persistente impone inoltre chiamate stateless: transcript, history e
provenance possono essere riletti soltanto da Sprout e rivalidati a ogni
invocation; non può essere usato uno store di memoria del provider.

## Parti volutamente non implementate prima della decisione

- creazione automatica delle identità/device agent;
- distribuzione delle resource key agli agent runner;
- invocazioni reali verso un provider LLM;
- persistenza/API di prompt, LocalGoal e transcript che presuppongano uno dei
  modelli di key custody sopra;
- scheduling autonomo e retry del provider;
- UI dei gate di governance agentici.

Implementare queste parti scegliendo implicitamente il server come decryptor
sarebbe un bypass degli invarianti E2EE, non un concrete refinement fedele.

## Assumption residue

Il kernel Rust assume che i suoi adapter forniscano fatti correnti e autorevoli:

- kind dei principal e relazione controller/agent;
- ancestry delle risorse dal grafo `resource_closure`;
- permission effettive da `effective_domain_permission` e controlli speciali di
  assignment;
- leggibilità plaintext, inclusa disponibilità dell'epoca/chiave E2EE;
- required effects deterministici di ogni tool;
- audience corrente di source e sink;
- atomicità e append-only persistence di audit/provenance.

Queste assumption devono diventare query/transaction concrete dopo la decisione
sull'execution boundary; non devono essere implementate dall'LLM o accettate dal
client come booleani autorevoli.
