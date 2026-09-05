# Sprout frontend: guida autonoma per uno sviluppatore indipendente

Questa guida permette a uno sviluppatore frontend senza contesto precedente di
eseguire, comprendere ed estendere il client web Sprout rispettando i contratti
del backend e i confini E2EE. Descrive lo stato del repository al commit
`10d10337a915f181846344e454fdab24072316a2` sul branch
`codex/lean-concrete-refinement`, dopo la migration 0035.

Sprout è pre-production. Il backend rifiuta la modalità production finché non
è completato l'audit crittografico indipendente. Le istruzioni locali usano
esclusivamente la modalità development e non costituiscono una procedura di
deployment.

## 1. Principi architetturali non negoziabili

Prima di implementare una schermata o una chiamata API, assumere questi
vincoli:

1. Il browser è il client E2EE e il control plane dell'esecuzione locale.
2. Il backend coordina identità, autorizzazioni, ciphertext, commitment,
   provenance e trace; non deve ricevere plaintext o chiavi di contenuto.
3. API key, password, token provider, endpoint privati, path locali, documenti
   decifrati e configurazioni AI restano sul dispositivo.
4. Un output del modello è una proposta strutturata, mai un'autorizzazione.
5. Task, TaskList, Topic, Info e Comment sono surface native Sprout, non tool.
6. Gli external tool passano dal kernel governato e dal local edge user-owned.
7. Un Comment, `sourceComment` o risultato del modello non crea permission,
   Responsibility o `WorkAuthorityOrigin`.
8. Il frontend non deve fabbricare campi server-owned: actor, CommentId,
   agentDepth, semantic tick, trace, audit identity e authority derivano dal
   trusted writer.
9. Digest e commitment non sostituiscono un payload tipato quando il contratto
   richiede il record completo.
10. Fallire chiusi è corretto quando mancano chiave, envelope, provenance,
    capability, permission o certificato esatto.

## 2. Stato funzionale reale

### Disponibile nella UI

- signup/login development e cerimonie email/passkey;
- vault device session-only o persistente via WebAuthn PRF;
- progetti, Topic, TaskList, Task, assegnazioni e permission;
- Info Markdown, questionari, preset, allegati, retention e recovery;
- IndexedDB offline, coda sync firmata, wake WebSocket e conflict handling;
- configurazione AI locale e cifrata nel vault;
- schermate Security, People, Recovery, Retention e AI.

### Backend disponibile ma UI non ancora presente

- provisioning e amministrazione di governed agents;
- Responsibility compiler, LocalGoal compiler e approval workflow;
- collaborative agent runs, claims, blockers ed evidence;
- UserProxy, global synthesis e cross-owner governance;
- surface Comment nativa R5.41;
- visualizzazione delle trace/certificate R540/R541;
- grant/revoke dei tool e capability witness;
- tool execution e terminal observations.

### Esecuzione dei provider sul dispositivo

- `web.read` e `document.local.read` user-owned;
- installazione/rilevamento Ollama;
- firma delle observation per le invocazioni governate degli agenti.

Le modalità esplicite `commercial_api` e `lan_inference` consentono discovery e
generazione direttamente dal dispositivo tramite gli adapter HTTP. Quando è
presente, `LocalEdgeInferenceBridge` resta il percorso preferito. Le modalità
`private_remote` e `commercial_privacy` richiedono il runtime dedicato. Il
backend Sprout non riceve le credenziali del provider.

Durante lo sviluppo, l'adapter DeepSeek usa il relay same-origin Vite
`/__sprout-ai/deepseek`. Il relay ha un target fisso
`https://api.deepseek.com`, non accetta URL arbitrari e non coinvolge il backend
Sprout. Dopo una modifica a `vite.config.ts`, riavviare il server Vite.

Per DeepSeek usare un modello restituito da `GET /v1/models`, attualmente
`deepseek-v4-flash` o `deepseek-v4-pro`. Gli alias `deepseek-chat` e
`deepseek-reasoner` sono dismessi e il client li blocca prima del trasporto. Gli
errori HTTP 400/404/422 includono nel pannello il dettaglio breve restituito dal
provider, con credenziali e token rimossi.

## 3. Mappa del repository

| Area | Sorgente autorevole |
| --- | --- |
| Entrypoint React | `frontend/sprout-web/src/main.tsx` |
| Composizione applicazione | `frontend/sprout-web/src/App.tsx` |
| State machine UI | `frontend/sprout-web/src/store/app-store.ts` |
| Client HTTP | `frontend/sprout-web/src/api/client.ts` |
| Contratti TypeScript | `frontend/sprout-web/src/api/contracts.ts` |
| Contratti Rust condivisi | `crates/api-contract/src/lib.rs` |
| Route effettive | `apps/server/src/routes/mod.rs` |
| Request agent/governance | `apps/server/src/routes/agents.rs` |
| Run/claim/work | `apps/server/src/routes/agent_runs.rs` |
| Tool runtime | `apps/server/src/routes/agent_tools.rs` |
| Comment nativo | `apps/server/src/routes/comments.rs` |
| Cifratura WASM | `frontend/sprout-web/src/security/wasm.ts` |
| Vault | `frontend/sprout-web/src/security/key-vault.ts` |
| Auth controller | `frontend/sprout-web/src/security/auth-controller.ts` |
| Envelope/epoch | `frontend/sprout-web/src/domain/envelopes.ts` |
| Resource codec | `frontend/sprout-web/src/domain/resources.ts` |
| IndexedDB | `frontend/sprout-web/src/storage/encrypted-db.ts` |
| Sync | `frontend/sprout-web/src/sync/sync-engine.ts` |
| AI contracts | `frontend/sprout-web/src/ai/contracts.ts` |
| Provider adapters | `frontend/sprout-web/src/ai/providers.ts` |
| Local edge boundary | `frontend/sprout-web/src/ai/execution-boundary.ts` |
| Agent edge loop | `frontend/sprout-web/src/ai/edge-runtime.ts` |
| API overview | `docs/api.md` |
| Wire crypto | `docs/crypto-wire-formats-v1.md` |
| Threat model | `docs/threat-model.md` |
| Permission model | `docs/permissions.md` |
| Formal release evidence | `docs/agent-r5-0035-full-formal-release.md` |

Non esiste ancora un OpenAPI generato. La combinazione route registry + tipi
Rust `Deserialize` + test server è autoritativa. Molti request type usano
`#[serde(deny_unknown_fields)]`: inviare campi UI extra produce HTTP 400.

## 4. Toolchain e avvio locale

### Requisiti

- Rust 1.88 tramite il toolchain pinned del repository;
- Node.js 22.12 o superiore;
- npm e dipendenze installate in `frontend/sprout-web`;
- `wasm-pack` 0.15.0;
- PostgreSQL compatibile con le 35 migration;
- browser moderno con IndexedDB, WebCrypto e WebAuthn;
- autenticatore WebAuthn con estensione PRF per il vault persistente.

### Backend

Usare `.env.example` come schema, mai come file di secret da committare. Le
variabili minime sono:

```bash
export SPROUT_BIND_ADDR=127.0.0.1:18085
export SPROUT_BASE_URL=http://localhost:18085
export SPROUT_CORS_ORIGINS=http://localhost:4176
export SPROUT_ENVIRONMENT=development
export SPROUT_ENABLE_EXPERIMENTAL_CRYPTO_FOR_DEVELOPMENT=true
export DATABASE_URL=postgresql://UTENTE@127.0.0.1:PORTA/DATABASE
export SPROUT_MIGRATIONS_DIR="$PWD/db/migrations"
export SPROUT_BLOB_DIR=/tmp/sprout-dev-blobs
export SPROUT_ARCHIVE_DIR=/tmp/sprout-dev-archives
export SPROUT_EMAIL_OUTBOX_KEY="$(openssl rand -base64 32)"
export SPROUT_ARCHIVE_SIGNING_KEY="$(openssl rand -base64 32)"
export SPROUT_ARCHIVE_SIGNING_KEY_ID="$(uuidgen)"

cargo run -p sprout-server -- serve
```

Per riusare dati cifrati tra riavvii, conservare le chiavi runtime in un file
protetto fuori dal repository e non rigenerarle a ogni start.

Health check:

```bash
curl --fail http://localhost:18085/health/live
curl --fail http://localhost:18085/health/ready
curl --fail -H 'x-request-id: frontend-dev' \
  http://localhost:18085/health/trace
```

### Frontend

```bash
npm --prefix frontend/sprout-web install
npm --prefix frontend/sprout-web run wasm:build

SPROUT_DEV_API_PROXY_TARGET=http://127.0.0.1:18085 \
  npm --prefix frontend/sprout-web run dev -- \
  --host 127.0.0.1 --port 4176
```

Aprire **`http://localhost:4176`**, non `http://127.0.0.1:4176`, quando si
usano passkey: il RP ID configurato dal backend è `localhost`. Cambiare host
crea inoltre un'altra origin con IndexedDB/localStorage distinti.

Vite inoltra soltanto `/v1` e `/health` al backend. Il frontend deve continuare
a usare URL same-origin; non incorporare un endpoint API assoluto nella build.

### Account development

`POST /v1/auth/dev/login` è disponibile solo in development e risolve un
account già esistente. Non crea implicitamente un utente. Su database nuovi,
iniziare il normale signup oppure creare una identità pending tramite
`/v1/auth/email/verification/start`, poi usare il quick login.

Non progettare flussi production dipendenti da dev login o token restituiti
dall'outbox development.

## 5. Architettura frontend

L'app corrente è una SPA React con una state machine centrale e servizi
device-local:

```text
React UI
  -> App reducer
  -> ApiClient ------------------------> Sprout backend
  -> resource codecs -> crypto WASM
  -> KeyVault -> IndexedDB
  -> SyncEngine -> signed queue + REST/WebSocket
  -> LocalAiProfileStore -> KeyVault only
  -> LocalEdgeInferenceBridge ---------> native edge (non ancora collegato)
```

`AppState.phase` distingue:

- `signed-out`: nessuna sessione;
- `authenticating`: cerimonia in corso;
- `locked`: sessione valida ma chiavi device assenti;
- `local-ready`: vault sbloccato offline, sync server disabilitato;
- `ready`: sessione e vault disponibili.

Non ridurre questi stati a un boolean `loggedIn`: autenticazione e disponibilità
delle chiavi sono proprietà indipendenti.

## 6. Autenticazione, sessione e vault

### Sessione

Le route protette usano:

```http
Authorization: Bearer <session-token>
```

Il token rimane in memoria dentro `ApiClient`; non deve entrare in URL, log,
analytics, localStorage applicativo o messaggi di errore.

### Device provisioning

Dopo l'autenticazione il client:

1. riusa il device UUID locale quando valido;
2. genera chiavi device tramite WASM;
3. registra il key package pubblico;
4. conserva privatamente X25519, ML-KEM, Ed25519 e ML-DSA;
5. importa gli envelope indirizzati a identity + device + key version.

### Stati del vault

| Stato | Significato |
| --- | --- |
| `locked` | nessun secret device in memoria |
| `session-only` | chiavi in memoria; perse alla chiusura salvo helper DEV |
| `prf-wrapped` | vault AES-GCM in IndexedDB, wrapping key derivata da WebAuthn PRF |

Per abilitare il vault persistente: menu utente → **Sicurezza** →
**Register passkey**. Se l'autenticatore restituisce PRF, il vault diventa
`prf-wrapped`. **Request persistence** invoca la persistenza storage del
browser, ma non sostituisce il wrapping crittografico.

Il fallback snapshot in localStorage è esclusivamente development. Non copiarlo
in un frontend production e non presentarlo come protezione equivalente a PRF.

## 7. E2EE: modello dati e regola di implementazione

Il wire payload comune è:

```ts
interface EncryptedPayloadDto {
  version: number
  algorithm: string
  key_id: string
  nonce_b64: string
  ciphertext_b64: string
}
```

Il backend può vedere UUID, relazioni, epoch, versioni, dimensioni, state e
commitment; non deve vedere nome progetto, titolo task, markdown, commento,
filename, path logico o output del modello in plaintext.

### Creazione di una risorsa

Il pattern corretto è:

1. generare UUID client-side;
2. costruire il documento tipato in memoria;
3. ottenere/generare la resource key dal vault;
4. cifrare con `createEncryptedResource` o helper specifico;
5. legare project, resource kind, resource ID, aggregate version e key epoch
   nell'AAD canonico;
6. costruire epoch e envelope per i device destinatari;
7. inviare ciphertext + epoch + envelope nella stessa operazione prevista;
8. azzerare copie temporanee delle chiavi;
9. salvare nel database locale solo record cifrati.

Non chiamare direttamente `crypto.subtle.encrypt` per inventare un formato
alternativo. Usare gli adapter in `security/wasm.ts` e i codec in
`domain/resources.ts`.

### Epoch ed envelope

- ogni risorsa ha key epoch versionato;
- body e header possono usare purpose distinti;
- gli envelope sono legati a identity, device e key version;
- il client verifica package digest e firme Ed25519 + ML-DSA prima dell'unwrap;
- una revoca richiede rotazione verso un nuovo epoch;
- non ricifrare con una chiave vecchia dopo revoca;
- `container_only` riceve header, non body o Info content.

## 8. Convenzioni API

### HTTP e JSON

- base path: `/v1`;
- JSON snake_case;
- UUID stringa canonicale;
- date ISO-8601 UTC;
- GET non invia body;
- POST/PUT/DELETE mutativi usano idempotency key quando prevista;
- `cache: no-store`;
- `credentials: same-origin`;
- allegati ciphertext usano `application/octet-stream`.

### Campi trusted

Non inserire arbitrariamente nel body:

- actor/author;
- principal kind;
- project derivabile dal path;
- status calcolato;
- semantic tick;
- trace number;
- comment depth;
- audit/event ID;
- permission decision;
- authority ceiling;
- risk tier o audience non previsti dal contract.

Path/body mismatch deve essere trattato come errore, non corretto
silenziosamente dal client.

### Idempotenza e conflitto

Una operazione mutativa deve conservare la stessa idempotency key nei retry.

- replay identico: stessa risposta/identità, nessun secondo effetto;
- stessa key con semantica diversa: HTTP 409;
- optimistic version errata: HTTP 409;
- non generare una nuova key dopo un timeout ambiguo finché non è risolta la
  sorte della richiesta originale.

### Errori pubblici

| HTTP | Codice | Trattamento UI |
| --- | --- | --- |
| 400 | `invalid_request` | errore input/contract, non retry automatico |
| 401 | `unauthorized` | sessione assente/scaduta, tornare all'auth |
| 403 | `forbidden` | permesso corrente insufficiente |
| 404 | `not_found` | risorsa assente o non visibile |
| 409 | `conflict` | convergenza/reload o risoluzione conflitto |
| 409 | `recovery_unprovisioned` | richiedere setup recovery |
| 413 | `payload_too_large` | ridurre input prima del retry |
| 429 | rate limited | backoff visibile e bounded |
| 503 | `unavailable` | retry bounded, stato offline controllato |
| 500 | `internal` | mostrare request ID, mai dettagli sensibili |

## 9. Inventario API per area

Questa sezione è una mappa di navigazione. Per il body esatto leggere il tipo
request nella route indicata o il metodo già presente in `api/client.ts`.

### Health e autenticazione

```text
GET  /health/live
GET  /health/ready
GET  /health/trace
POST /v1/auth/email/verification/start|finish
POST /v1/auth/email/recovery/start|finish
POST /v1/auth/passkeys/register/start|finish
POST /v1/auth/passkeys/authenticate/start|finish
POST /v1/auth/dev/login                 development only
```

### Device, progetti, membership e permission

```text
GET|POST /v1/devices/{device}/key-packages
DELETE   /v1/devices/{device}/key-packages/{version}
GET      /v1/devices/{device}/key-transparency
GET|POST /v1/projects
GET      /v1/projects/{project}
GET|POST /v1/projects/{project}/invitations
POST     /v1/projects/{project}/invitations/accept
POST     /v1/projects/{project}/participant-suggestions
GET      /v1/projects/{project}/device-key-packages
GET      /v1/projects/{project}/resource-key-envelopes
POST     /v1/projects/{project}/member-resource-keys
GET|POST /v1/projects/{project}/resources/{resource}/permissions
DELETE   /v1/projects/{project}/resources/{resource}/permissions/{grant}
GET      /v1/projects/{project}/resources/{resource}/permissions/{grant}/rotation-plan
POST     /v1/projects/{project}/resources/{resource}/epochs
GET      /v1/projects/{project}/resources/{resource}/envelope-plan
```

### Workspace nativo

```text
GET|POST       /v1/projects/{project}/topics
GET|PUT|DELETE /v1/projects/{project}/topics/{topic}
GET|POST       /v1/projects/{project}/topics/{topic}/task-lists
GET|PUT|DELETE /v1/projects/{project}/task-lists/{list}
GET            /v1/projects/{project}/task-lists/{list}/tasks
POST           /v1/projects/{project}/tasks
GET|PUT|DELETE /v1/projects/{project}/tasks/{task}
POST           /v1/projects/{project}/tasks/{task}/complete|copy
GET|POST       /v1/projects/{project}/tasks/{task}/assignments
DELETE         /v1/projects/{project}/tasks/{task}/assignments/{assignment}
GET|POST       /v1/projects/{project}/topics/{topic}/info-documents
GET|POST       /v1/projects/{project}/task-lists/{list}/info-documents
GET|PUT|DELETE /v1/projects/{project}/info-documents/{document}
POST           /v1/projects/{project}/info-documents/{document}/files
```

Preset, recurrence, questionnaire e attachment sono già incapsulati da metodi
tipati in `api/client.ts`. Non duplicare una seconda API layer.

### Sync, retention e recovery

```text
POST /v1/sync/push
POST /v1/sync/pull
GET  /v1/sync/wake                  WebSocket con subprotocol auth
GET|PUT /v1/retention/preferences
GET     /v1/retention/archives
GET     /v1/retention/warnings
POST    /v1/retention/archives/{archive}/receipt
GET|PUT /v1/projects/{project}/recovery-provision
POST    /v1/projects/{project}/recovery-provision/activate
GET     /v1/projects/{project}/recovery-provision/shares/me
GET     /v1/projects/{project}/recovery-rotation-plan
POST    /v1/projects/{project}/recovery-requests
GET     /v1/projects/{project}/recovery-requests/{request}
POST    /v1/projects/{project}/recovery-requests/{request}/approvals
POST    /v1/projects/{project}/recovery-requests/{request}/finalize
```

### Comment nativo

```text
POST /v1/projects/{project}/comments
GET  /v1/projects/{project}/resources/{target}/comments
POST /v1/projects/{project}/agent-runs/{run}/claims/{claim}/comments
```

Human/admin request:

```ts
{
  recipient_id: Uuid
  target_id: Uuid
  parent_id: Uuid | null
  encrypted_payload: EncryptedPayloadDto
  key_epoch: number
  idempotency_key: Uuid
  run_id?: Uuid
}
```

Agent request aggiunge `work_item_id` e `attempt`; run e claim sono nel path.
Il server deriva CommentId, author, author kind, depth e semantic tick. La UI
deve cifrare il payload per l'audience autorizzata e rispettare
`commentReadable/readComment`; non trasformare Comment in external tool.

### Governed agents e governance

Route principali:

```text
POST /v1/projects/{project}/agents
PUT  /v1/projects/{project}/agents/{agent}/runner/activate
PUT  /v1/projects/{project}/agents/{agent}/responsibilities/{responsibility}
PUT  /v1/projects/{project}/users/{user}/responsibilities/{responsibility}
POST /v1/projects/{project}/users/{user}/responsibilities/{responsibility}/revisions/{revision}/activate
PUT  /v1/projects/{project}/agents/{agent}/local-goal
POST /v1/projects/{project}/agents/{agent}/local-goals/{goal}/revisions/{revision}/activate
POST /v1/projects/{project}/agents/{agent}/interrogations
GET  /v1/projects/{project}/agents/{agent}/interrogations/{interrogation}
POST /v1/projects/{project}/agent-global-contracts
POST /v1/projects/{project}/agent-global-coverage-needs
POST /v1/projects/{project}/agents/{agent}/global-mandates
POST /v1/projects/{project}/agent-global-new-agent-proposals
```

`POST /agents` non è una create CRUD semplice. Richiede:

- agent ID e principal identity ID distinti;
- controller uguale alla session identity;
- profile e runner label cifrati;
- profile resource e key epoch;
- availability;
- runner e runner device ID;
- `initial_local_goal` con output compiler strutturato;
- firme classica e post-quantum;
- final prompt approval;
- administrator creation approval quando richiesto.

Usare come fixture canoniche le funzioni dei test in
`apps/server/tests/agents.rs`. Non generare certificate nel componente React e
non sostituirli con un boolean `approved`.

### Run, work, claim, evidence e blocker

```text
POST /v1/projects/{project}/agent-runs
GET  /v1/projects/{project}/agent-runs/{run}
POST /v1/projects/{project}/agent-runs/{run}/refresh
POST /v1/projects/{project}/agent-runs/{run}/claim
POST /v1/projects/{project}/agent-runs/{run}/claims/{claim}/succeed
POST /v1/projects/{project}/agent-runs/{run}/claims/{claim}/fail
POST /v1/projects/{project}/agent-runs/{run}/claims/{claim}/materialize-task-completion
POST /v1/projects/{project}/agent-runs/{run}/complete
POST /v1/projects/{project}/agent-runs/{run}/evidence
POST /v1/projects/{project}/agent-runs/{run}/blockers
POST /v1/projects/{project}/agent-runs/{run}/blockers/{blocker}/resolve
```

Il client mostra coordinate e outcome restituiti dal server; non calcola
eligibility, authority principal, semantic tick o retry state.

### Tool runtime

```text
GET    /v1/agent-tools/catalog
PUT|DELETE /v1/projects/{project}/agents/{agent}/tool-permissions/{tool}/versions/{version}
PUT|DELETE /v1/projects/{project}/resources/{scope}/principals/{principal}/tool-permissions/{tool}/versions/{version}
POST /v1/projects/{project}/agents/{agent}/tool-runtime-capabilities
POST /v1/projects/{project}/agent-runs/{run}/claims/{claim}/tool-calls
POST /v1/projects/{project}/agent-runs/{run}/tool-calls/{call}/claim
POST /v1/projects/{project}/agent-runs/{run}/tool-calls/{call}/requests
POST /v1/projects/{project}/agent-runs/{run}/tool-calls/{call}/terminal
POST /v1/projects/{project}/agent-runs/{run}/tool-calls/{call}/retry
```

Catalog presence non equivale a runtime readiness. La UI deve mostrare
separatamente catalogo, permission, capability witness, run/work ceiling e
stato call. Il terminale non richiede current readiness; il retry sì.

### UserProxy, model runtime e cross-owner

```text
POST /v1/projects/{project}/user-proxy/threads
POST /v1/projects/{project}/user-proxy/threads/{thread}/requests
POST /v1/projects/{project}/user-proxy/requests/{request}/plan
POST /v1/projects/{project}/agents/{agent}/invocations
POST /v1/projects/{project}/agents/{agent}/invocations/client-provider
POST /v1/projects/{project}/agents/{agent}/runner/claim
POST /v1/projects/{project}/agents/{agent}/runner/client-provider/claim
POST /v1/projects/{project}/agents/{agent}/invocations/{invocation}/submit
POST /v1/projects/{project}/agents/{agent}/invocations/{invocation}/fail
POST /v1/projects/{project}/tasks/{task}/cross-owner-assignments
POST /v1/projects/{project}/cross-owner-assignments/{assignment}/decision
POST /v1/projects/{project}/cross-owner-assignments/{assignment}/finalize
POST /v1/projects/{project}/cross-owner-assignments/{assignment}/materialize
```

Il model plan non esegue direttamente un effetto. Il frontend deve presentare
candidate e confirmation esatte, senza consentire al modello di fornire actor,
authority, risk tier, audience o tool sostitutivo.

## 10. Offline e sync

IndexedDB `sprout-encrypted-workspace` contiene:

- encrypted records;
- signed sync queue;
- sync cursor e device sequence;
- PRF-wrapped vault record;
- tombstone;
- encrypted conflict.

Una mutazione offline:

1. aggiorna il record cifrato locale;
2. calcola event hash con project/resource/device/version/epoch;
3. firma Ed25519 e ML-DSA;
4. inserisce la richiesta completa nella queue;
5. incrementa la device sequence;
6. al reconnect esegue `push`, poi `pull` dal cursor;
7. conserva un conflitto 409 senza sovrascrivere una versione remota.

Il WebSocket `/v1/sync/wake` è soltanto una notifica: dopo reconnect eseguire
sempre REST catch-up. Il token passa nel subprotocol `sprout-auth.<token>`, non
nella query string.

## 11. AI e Local Edge Runtime

Le configurazioni AI sono un discriminated union:

- `commercial_api`;
- `lan_inference`;
- `private_remote`;
- `commercial_privacy`.

`LocalAiProfileStore` conserva profile, secret del commitment e revision nel
vault device-local. Nessuno dei tre viene sincronizzato al backend.

Per completare il companion nativo implementare:

```ts
interface LocalEdgeInferenceBridge {
  readonly protocolVersion: 'sprout-client-inference-edge-v1'
  discoverModels(profile, signal?): Promise<ProviderModel[]>
  generateStructured(profile, request, signal?): Promise<ProviderGenerationResult>
  detectOllama(): Promise<{ installed: boolean; version?: string; models: string[] }>
  installOfficialOllama(): Promise<{ installed: true; version: string }>
  pullOllamaModel(model: string): Promise<void>
}
```

Requisiti del transport browser↔edge:

- autenticato e origin/session-bound;
- nessun localhost endpoint anonimo;
- bind loopback, non LAN di default;
- one execution per exact attempt;
- cancellazione e timeout reali;
- nessun secret nei log;
- dispatch, profile commitment, exact wire witness e observation firmata;
- modello selezionato esattamente, nessun fallback silenzioso;
- credential esclusa dalla wire witness ma inclusa solo nel request header
  effettivo locale.

L'`edge-runtime.ts` già implementa claim→execute→signed observation→submit/fail.
Il nuovo frontend deve fornirgli un bridge nativo e un signer device, non
riscrivere il protocollo.

## 12. Piano consigliato per aggiungere la UI agenti

Implementare per slice verificabili:

1. **Agent directory read model**: lista agenti, state, controller, availability
   e runner status; non dedurre permission dal ruolo UI.
2. **Responsibility workflow**: editor strutturato, compiler result tipato,
   review, dual signature e activation.
3. **LocalGoal wizard**: prompt cifrato, commitment locale, compiler envelope,
   approval e exact scope.
4. **Agent provisioning**: inviare `ProvisionAgentRequest` completo e mostrare
   una sola volta il bootstrap token senza log/localStorage.
5. **Runner activation**: key package, runner device, current state e revoke.
6. **Run monitor**: WorkItem, claim, attempt, blocker, evidence e terminal
   outcome come read model server-derived.
7. **Comment nativo**: composer E2EE human/admin e composer agent nel claim
   governato; depth sempre letto dalla risposta.
8. **Tool panel**: catalogo, permission versionata, capability witness,
   call/attempt history e retry gated.
9. **Trace inspector**: mostrare projection/certificate, senza trasformare
   l'assenza in un falso stato enabled.
10. **Local edge integration**: pairing autenticato e loop provider.

Ogni slice deve includere un positivo, un permission denial, replay,
equivocation, cross-project e retention/payload-missing quando applicabile.

## 13. UI e accessibilità

- riusare i componenti e token CSS esistenti;
- conservare navigazione tastiera e focus visibile;
- associare label reali agli input;
- usare `role=status` per esiti non bloccanti e `role=alert` per errori;
- non mostrare ciphertext, token o commitment completi come debug UI;
- non usare colore come unico indicatore di permission/stato;
- distinguere `locked`, `unavailable`, `forbidden`, `fail-closed` e `empty`;
- non chiamare “disponibile” una feature contract-only;
- non dichiarare produzione completa per bridge/provider non live.

## 14. Test e gate

### Loop rapido frontend

```bash
npm --prefix frontend/sprout-web test -- --run
npm --prefix frontend/sprout-web run lint
npm --prefix frontend/sprout-web run build
```

### Crypto WASM

```bash
npm --prefix frontend/sprout-web run wasm:build
npm --prefix frontend/sprout-web run test:wasm:parity
bash scripts/verify-wasm-reproducible.sh
```

### Browser E2E

```bash
npm --prefix frontend/sprout-web run test:e2e
npm --prefix frontend/sprout-web run test:e2e:pwa
```

### Backend contract regression

```bash
cargo check -p sprout-server --all-targets
cargo test --workspace --all-targets
bash scripts/validate-migrations.sh
```

Per i test PostgreSQL `#[ignore]`, usare sempre un database disposable migrato
e `--test-threads=1`; non puntarli al database manuale o di sviluppo.

### Test minimi per una nuova feature

- render e interaction test;
- errore 401/403/409/503;
- vault locked e missing key;
- plaintext-canary assente da request/log/storage non cifrato;
- idempotent replay;
- optimistic conflict;
- offline queue + reconnect;
- cross-project denial;
- reload/restart;
- browser senza PRF;
- retention/payload purgato.

## 15. Checklist di sicurezza per code review

- [ ] Nessun plaintext semantico attraversa `/v1`.
- [ ] Nessuna key/credential/token entra in log, URL o analytics.
- [ ] L'AAD include l'identità esatta richiesta dal codec corrente.
- [ ] Le chiavi temporanee vengono azzerate nei `finally`.
- [ ] Il payload usa il key epoch esatto.
- [ ] Gli envelope verificano digest e dual signature.
- [ ] Il client non invia actor/authority/status/tick server-owned.
- [ ] Idempotency key riusata nei retry identici.
- [ ] 409 non viene convertito in last-write-wins.
- [ ] Nessun provider viene chiamato dal backend o dal browser non autorizzato.
- [ ] Comment resta native surface.
- [ ] Model output resta candidate data.
- [ ] Agent generic-route bypass non viene introdotto.
- [ ] Feature incomplete mostrata come non disponibile/fail-closed.
- [ ] Test non usa secret o endpoint privati reali.

## 16. Definition of done per un frontend indipendente

Un frontend alternativo è compatibile soltanto quando:

1. usa gli stessi wire contracts, codec E2EE e WASM verificato;
2. implementa la state machine auth/vault separando sessione e chiavi;
3. gestisce epoch, envelope, dual signature e key transparency;
4. conserva idempotenza, optimistic concurrency e sync hash chain;
5. non amplia authority tramite UI, model, Comment o tool;
6. usa il local edge per provider/tool e non introduce backend plaintext;
7. supera unit, browser, WASM parity/repro e backend contract tests;
8. documenta esplicitamente tutte le feature non implementate;
9. non dipende dal dev login in release;
10. non afferma production readiness prima dei gate in `docs/operations.md`.

Quando una shape API non è chiara, non indovinarla: seguire nell'ordine
`routes/mod.rs` → request/response Rust della route → `api/contracts.ts` →
`api/client.ts` → test server corrispondente. Questo evita drift fra una UI
plausibile e il contract realmente autorizzato dal kernel.
