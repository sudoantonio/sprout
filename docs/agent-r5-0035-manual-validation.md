# Sprout R5 checkpoint 0035: avvio e collaudo manuale

Questa guida descrive l'istanza locale predisposta il 25 agosto 2026 e una
procedura ripetibile per arrestare, riavviare e collaudare Sprout. È una
configurazione **development, solo loopback e non production-ready**. Non
espone il servizio sulla LAN e non sostituisce i test automatici o una review
di sicurezza.

## Istanza attualmente avviata

| Componente | Valore |
| --- | --- |
| Branch | `codex/lean-concrete-refinement` |
| Commit | `10d10337a915f181846344e454fdab24072316a2` |
| Backend | `http://127.0.0.1:18085` |
| Frontend | `http://127.0.0.1:4176` |
| PostgreSQL | `127.0.0.1:55433` |
| Database manuale | `sprout_r5_0035_manual_20260825` |
| Migration | 35 applicate, versioni `1..35`, tutte successful |
| Backend PID all'avvio | `2616304` |
| Frontend PID all'avvio | `2617645` |
| Account development | `admin@example.test` / `admin.minerva` |

Gli health check verificati all'avvio hanno restituito:

```text
GET /health/live  -> 200 {"status":"ok"}
GET /health/ready -> 200 {"status":"ready"}
GET /health/trace -> 200, con propagazione di x-request-id
Frontend /        -> 200
Frontend proxy /health/ready -> 200
```

Il login development non usa una password. L'account sopra è stato creato in
stato pending; il pulsante **Entra come admin.minerva** lo attiva e registra il
device del browser. Questo endpoint esiste soltanto con
`SPROUT_ENVIRONMENT=development`.

## Arrestare l'istanza attuale

Prima verificare che i PID siano ancora quelli indicati:

```bash
ps -fp 2616304,2617645
```

Arresto graceful del backend e del frontend:

```bash
kill -TERM 2616304
kill -TERM 2617645
```

Se i PID sono cambiati, individuarli senza usare un `pkill` generico:

```bash
pgrep -af '^target/debug/sprout-server serve$'
pgrep -af 'node .*/vite --host 127.0.0.1 --port 4176$'
```

Inviare `SIGTERM` soltanto ai PID esatti restituiti. Verificare l'arresto:

```bash
curl --fail http://127.0.0.1:18085/health/live
curl --fail http://127.0.0.1:4176/
```

Entrambi i comandi devono fallire per connessione rifiutata. PostgreSQL resta
acceso e il database non viene cancellato. Quando backend e frontend vengono
avviati in primo piano con le istruzioni seguenti, il metodo preferito per
fermarli è `Ctrl+C` nei rispettivi terminali.

## Riavvio manuale

### 1. Verificare PostgreSQL

Per riusare l'istanza predisposta:

```bash
/tmp/sprout-postgresql-14.23/bin/pg_isready \
  -h 127.0.0.1 -p 55433 -U sprout_test
```

Il risultato atteso è `accepting connections`. Questa istanza PostgreSQL vive
in un ambiente locale temporaneo. Come alternativa ripetibile del repository,
avviare il container development:

```bash
SPROUT_POSTGRES_PORT=5432 bash scripts/postgres-dev.sh start
SPROUT_POSTGRES_PORT=5432 bash scripts/postgres-dev.sh status
```

In quel caso usare `postgresql://sprout@127.0.0.1:5432/sprout` come
`DATABASE_URL`. Lo stop preserva il volume:

```bash
SPROUT_POSTGRES_PORT=5432 bash scripts/postgres-dev.sh stop
```

Non eseguire `destroy --confirm` se si vogliono conservare i dati.

### 2. Preparare le variabili runtime

Le chiavi non devono essere salvate nel repository. Per una sessione effimera
si possono esportare direttamente. Se lo stesso database deve sopravvivere a
più riavvii, generarle una sola volta e conservarle in un file protetto fuori
dal repository (`chmod 600`): cambiare la chiave outbox o archive rende
illeggibili gli oggetti già cifrati o firmati con la revisione precedente.

Nel primo terminale, dalla root del clone, impostare `SPROUT_TOOLS_ROOT` alla
directory `.tools` pinned disponibile sulla workstation:

```bash
export SPROUT_REPO_ROOT="$PWD"
export SPROUT_TOOLS_ROOT=/percorso/assoluto/alla/.tools
export PATH="$SPROUT_TOOLS_ROOT/cargo/bin:/usr/bin:/bin"
export RUSTUP_HOME="$SPROUT_TOOLS_ROOT/rustup"
export CARGO_HOME="$SPROUT_TOOLS_ROOT/cargo"
export OPENSSL_DIR="$SPROUT_TOOLS_ROOT/openssl"
export OPENSSL_LIB_DIR="$SPROUT_TOOLS_ROOT/openssl/lib64"
export OPENSSL_INCLUDE_DIR="$SPROUT_TOOLS_ROOT/openssl/include"
export LD_LIBRARY_PATH="$SPROUT_TOOLS_ROOT/openssl/lib64"

export DATABASE_URL=postgresql://sprout_test@127.0.0.1:55433/sprout_r5_0035_manual_20260825
export SPROUT_BIND_ADDR=127.0.0.1:18085
export SPROUT_BASE_URL=http://localhost:18085
export SPROUT_CORS_ORIGINS=http://localhost:4176,http://127.0.0.1:4176,http://localhost:18085
export SPROUT_ENVIRONMENT=development
export SPROUT_ENABLE_EXPERIMENTAL_CRYPTO_FOR_DEVELOPMENT=true
export SPROUT_MIGRATIONS_DIR="$SPROUT_REPO_ROOT/db/migrations"
export SPROUT_BLOB_DIR=/tmp/sprout-r5-0035-manual-blobs
export SPROUT_ARCHIVE_DIR=/tmp/sprout-r5-0035-manual-archives
export SPROUT_EMAIL_OUTBOX_KEY="$(openssl rand -base64 32)"
export SPROUT_ARCHIVE_SIGNING_KEY="$(openssl rand -base64 32)"
export SPROUT_ARCHIVE_SIGNING_KEY_ID="$(uuidgen)"
export RUST_LOG=sprout_server=info,tower_http=info
```

Per un database diverso, sostituire soltanto `DATABASE_URL`. Non usare queste
impostazioni development in produzione.

### 3. Avviare il backend

Nello stesso terminale:

```bash
cargo run -p sprout-server -- serve
```

Il server applica automaticamente le migration pendenti. Attendere la riga:

```text
Sprout HTTP server listening bind_addr=127.0.0.1:18085
```

In un secondo terminale eseguire:

```bash
curl --fail --silent --show-error http://127.0.0.1:18085/health/live
curl --fail --silent --show-error http://127.0.0.1:18085/health/ready
curl --fail --silent --show-error \
  -H 'x-request-id: manual-check' \
  http://127.0.0.1:18085/health/trace
```

### 4. Bootstrap di un account development, se il database è nuovo

Il quick login non crea implicitamente identità inesistenti. Creare una riga
pending tramite il normale endpoint di signup, senza salvare il token:

```bash
PROFILE_B64="$(openssl rand -base64 32)"
curl --fail --silent --show-error --output /dev/null \
  --write-out 'signup bootstrap HTTP %{http_code}\n' \
  -H 'content-type: application/json' \
  --data "{\"email\":\"admin@example.test\",\"identity_handle\":\"admin.minerva\",\"encrypted_profile_b64\":\"${PROFILE_B64}\"}" \
  http://127.0.0.1:18085/v1/auth/email/verification/start
unset PROFILE_B64
```

Risultato atteso: HTTP `202`. Su un account già attivo l'endpoint è
convergente. Non stampare o conservare il token development.

### 5. Avviare il frontend

Nel secondo terminale:

```bash
cd "$SPROUT_REPO_ROOT"
export PATH="$SPROUT_TOOLS_ROOT/node-v22.16.0/bin:/usr/bin:/bin"
export SPROUT_DEV_API_PROXY_TARGET=http://127.0.0.1:18085
npm --prefix frontend/sprout-web run dev -- --host 127.0.0.1 --port 4176
```

Aprire `http://127.0.0.1:4176/`. Non aprire il bind su `0.0.0.0`: il quick
login development non deve essere esposto alla LAN.

## Collaudo manuale definitivo

Registrare per ogni punto: ora, browser/versione, risultato atteso, risultato
osservato, PASS/FAIL e screenshot privo di dati sensibili. Usare nomi di test
univoci come `Manuale-0035-<data-ora>`.

### A. Integrità e startup

- [ ] `git rev-parse HEAD` restituisce
      `10d10337a915f181846344e454fdab24072316a2`.
- [ ] `bash scripts/validate-migrations.sh` restituisce 35 migration valide.
- [ ] `/health/live`, `/health/ready` e il proxy frontend rispondono 200.
- [ ] Il backend è in development e ascolta soltanto su `127.0.0.1:18085`.
- [ ] I log mostrano database e chiavi come `[REDACTED]` e non mostrano
      payload, token o plaintext.

### B. Login e device locale

- [ ] Aprire il frontend in una finestra normale pulita.
- [ ] Nella schermata Crea lasciare `admin@example.test` e `admin.minerva`.
- [ ] Premere **Entra come admin.minerva**.
- [ ] L'app entra senza password solo perché il server è in development.
- [ ] Il device viene registrato e il vault locale risulta disponibile.
- [ ] Ricaricare la pagina: la sessione e il device restano coerenti e non
      compare un nuovo device a ogni reload.
- [ ] Da una finestra privata non autenticata, una route `/v1/projects`
      protetta risponde 401.

### C. Percorso workspace E2EE

- [ ] Creare un progetto con nome univoco.
- [ ] Verificare che il progetto compaia nell'elenco e resti presente dopo un
      reload completo.
- [ ] Creare una Topic, una TaskList e una Task usando le surface native.
- [ ] Modificare e completare la Task; verificare che lo stato sopravviva al
      reload.
- [ ] Aggiungere un commento Task dalla UI e verificare reload/idempotenza.
      Questo è il commento product preesistente: non va confuso con la nuova
      ledger nativa R5.41 descritta nella sezione F.
- [ ] In DevTools/Network verificare che titolo, descrizione e contenuto siano
      trasmessi come payload cifrati/opaque, non come plaintext server-side.
- [ ] Cercare una stringa-canary immessa nel browser nei log backend: deve
      essere assente.

### D. Persistenza, errore e recovery locale

- [ ] Con il browser online, effettuare una modifica e attendere il sync.
- [ ] Portare il browser offline, effettuare una modifica supportata e
      verificare che sia accodata localmente.
- [ ] Tornare online e verificare convergenza senza duplicati.
- [ ] Fermare soltanto il backend: il frontend deve mostrare lo stato offline
      o un errore controllato, senza perdere il vault locale.
- [ ] Riavviare il backend con lo stesso database e la stessa configurazione;
      `/health/ready` torna 200 e il client recupera senza duplicare eventi.

### E. Isolamento fra utenti

- [ ] Bootstrapare una seconda identità development con email/handle diversi.
- [ ] Usare un secondo profilo browser, non la stessa IndexedDB/localStorage.
- [ ] Prima di invito/condivisione, il secondo utente non vede il progetto.
- [ ] Dopo la procedura di invito e condivisione chiavi, vede soltanto le
      risorse autorizzate.
- [ ] Dopo revoca, non riceve nuovi epoch/key envelope; la revoca non viene
      presentata come cancellazione retroattiva del plaintext già scaricato.
- [ ] Un tentativo cross-project via API restituisce 403/404 e non crea righe.

### F. Comment nativo R5.41 e formal release 0035

Il frontend 0035 non espone ancora una UI dedicata alla nuova surface Comment:
questa era esplicitamente fuori scope. Non dichiarare fallita la ledger perché
la UI non la mostra, e non confonderla con `workspace.comment.*` o con un
external tool. Il collaudo definitivo del backend usa le route native:

```text
POST /v1/projects/{project_id}/comments
GET  /v1/projects/{project_id}/resources/{target_id}/comments
POST /v1/projects/{project_id}/agent-runs/{run_id}/claims/{claim_id}/comments
```

La POST umana accetta soltanto recipient, target, parent opzionale, payload
cifrato, key epoch, idempotency key e run opzionale. ID, autore, kind, depth e
semantic tick sono server-owned. La POST agentica richiede inoltre work,
claim e attempt esatti.

Per una verifica ripetibile, isolata dal database usato dal browser, creare un
database disposable migrato e lanciare il test E2E normativo:

```bash
export DATABASE_URL=postgresql://sprout_test@127.0.0.1:55433/DB_DISPOSABLE_GIA_MIGRATO
cargo test -p sprout-server --test agents \
  native_comments_preserve_exact_depth_replay_and_r541_gate \
  -- --ignored --exact --test-threads=1
```

Checklist del risultato:

- [ ] user/admin depth 0, agent root depth 1 e reply `parent + 1`.
- [ ] seconda root agentica, parent errato, self-comment e stale epoch sono
      rifiutati.
- [ ] replay identico restituisce lo stesso CommentId e non crea un secondo
      `commentPosted`; equivocation viene rifiutata.
- [ ] tick concorrenti sono distinti e la priorità administrator > user > agent
      è applicata alle risposte, non al solo ordinamento UI.
- [ ] il Comment completo è presente nello stato semantico della run.
- [ ] comment gate e inventory sono list-exact; corruption o payload mancante
      li rende `disabled_fail_closed` con lista vuota.
- [ ] il Comment non crea permission, tool authority o WorkAuthorityOrigin.

### G. Tool runtime e trace R540/R541

Su un altro database disposable migrato, eseguire:

```bash
export DATABASE_URL=postgresql://sprout_test@127.0.0.1:55433/ALTRO_DB_DISPOSABLE_GIA_MIGRATO
cargo test -p sprout-server --test agents \
  governed_external_tool_attempts_preserve_historical_terminal_and_retry_fences \
  -- --ignored --exact --test-threads=1
```

- [ ] invoke/retry conservano stesso ToolCall, tool/input/bounds e attempt
      storici distinti.
- [ ] WorkAttempt, ToolEvent e WorkOutcome sono exact/list-exact.
- [ ] timeout no-dispatch, dispatch senza request e request esatta restano
      distinguibili.
- [ ] terminale dopo revoca/expiry è accettato dalla provenance storica;
      retry rivalida readiness e authority correnti.
- [ ] allocator semantic tick e wall-clock operativo restano separati.
- [ ] nessuna call pending supera il proprio semantic timeout deadline.
- [ ] il root formale è assente prima dell'ultimo child, viene emesso soltanto
      con 28/28 child esatti e il replay conserva lo stesso ID.

### H. Negative/security checks

- [ ] `workspace.comment.*`, `comment.read` e `comment.post` non compaiono nel
      catalogo external tool.
- [ ] Task/TaskList/Topic/Info/Comment restano surface native.
- [ ] `mail.send`, `telegram.send`, shell/code execution e filesystem mutation
      restano fail-closed.
- [ ] Un agent non può usare la route Comment umana.
- [ ] Un reader senza `commentReadable/readComment` non legge il payload.
- [ ] Un output del modello, un Comment o `sourceComment` non conferiscono
      authority.
- [ ] Nessun secret, endpoint privato, path locale o plaintext tool/comment è
      visibile nei log o nelle risposte backend.

## Query diagnostiche non distruttive

Controllo migration:

```bash
/tmp/sprout-postgresql-14.23/bin/psql -X \
  -h 127.0.0.1 -p 55433 -U sprout_test \
  -d sprout_r5_0035_manual_20260825 \
  -c 'SELECT count(*), min(version), max(version), bool_and(success) FROM _sqlx_migrations'
```

Controllo che le tabelle 0035 esistano:

```bash
/tmp/sprout-postgresql-14.23/bin/psql -X \
  -h 127.0.0.1 -p 55433 -U sprout_test \
  -d sprout_r5_0035_manual_20260825 \
  -c "SELECT to_regclass('public.native_comments'), to_regclass('public.agent_r541_formal_release_certificates')"
```

Non usare DML diretto sulle ledger governate per preparare la prova: aggirerebbe
route, trusted writer, RLS, idempotency e certificate exactness.

## Criterio finale di accettazione

Il collaudo utente è PASS soltanto se:

1. tutti i punti applicabili A-E e H sono verdi;
2. i due E2E isolati F-G sono verdi sui rispettivi database disposable;
3. non vi sono errori console/backend inattesi, duplicati o plaintext;
4. restart e replay convergono;
5. i limiti sono riportati correttamente: ambiente development, UI Comment
   R5.41 non inclusa, local edge/native companion e provisioning DB production
   least-privilege non vengono overclaimati.

In caso di FAIL annotare request ID, endpoint, status code, ora e passaggi
minimi di riproduzione. Non allegare token, payload decifrati, chiavi, cookie,
path personali o dump del database.
