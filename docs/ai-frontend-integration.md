# Collegamento AI frontend/backend

Le due chat hanno confini distinti:

- **Ask to AI** è lo UserProxy personale dell'utente nel progetto aperto. Legge
  il contesto già autorizzato e può preparare azioni che verranno materializzate
  con la sessione e i permessi correnti dell'utente.
- La chat nella scheda **Agenti** mostra il singolo agente come soggetto
  osservato, ma la risposta resta prodotta dallo UserProxy personale sulla base
  del lavoro leggibile del target.

Ask to AI non è un agente governato e non richiede un runner agente. Il modello
interpreta la richiesta; il prodotto risolve gli ID, presenta il piano esatto e
usa le normali API prodotto, che rivalidano l'autorità dell'utente autenticato.

## Ask to AI: chat del progetto

1. `TasksScreen` passa alla chat soltanto il progetto selezionato e le risorse
   già decifrate presenti in memoria: progetto, categorie, liste, task e membri.
2. `buildWorkspaceSources` produce descrittori `resource_body` con ID reali,
   esclude record bloccati e risorse appartenenti a un altro progetto, e limita
   la proiezione a 200 risorse e 120.000 caratteri.
3. `WorkspaceChatService` carica dal vault il profilo AI locale e invia
   `interpret_proxy_request` con schema chiuso, massimo un'azione e liste finite
   di ID e action type candidate. Per una domanda informativa il risultato usa
   `action_type: none`; per un comando produce un piano tipato.
4. Il client rifiuta campi extra, ID esterni al workspace, target ambigui e
   membri non presenti. Il piano accettato viene mostrato con un riepilogo breve
   e rimane immutato fino alla conferma one-shot.
5. Dopo la conferma, `TasksScreen` materializza l'azione tramite le stesse
   callback usate dalla UI. Sono coperte creazione/modifica di categorie,
   tasklist e task, completamento, copia, assegnazione, invito membri e modifica
   delle responsabilità. Le API backend vedono come actor l'utente corrente e
   applicano gli stessi permission gate della UI ordinaria.
6. Nessun `agent_id`, sessione agente o runner agente entra nel flusso.
   Provider, endpoint e credenziali rimangono sul dispositivo; il backend
   Sprout non riceve le credenziali del provider.
   In sviluppo, DeepSeek passa dal relay same-origin di Vite
   `/__sprout-ai/deepseek`, che evita il blocco CORS senza inoltrare la chiave al
   backend Sprout. Il relay riscrive soltanto verso `https://api.deepseek.com`.
   I modelli selezionabili sono `deepseek-v4-flash` e `deepseek-v4-pro`; gli
   alias dismessi `deepseek-chat` e `deepseek-reasoner` vengono rifiutati
   localmente prima della richiesta. Per HTTP 400/404/422 l'interfaccia mostra
   il dettaglio breve del provider dopo aver rimosso eventuali credenziali.
7. Lo storico, inclusi piano e stato dell'esecuzione, è conservato nelle
   impostazioni cifrate del vault, separato per progetto e limitato per rientrare
   nel record locale. **Nuova chat** elimina soltanto lo storico corrente.

Se manca il profilo locale, il popup porta alle impostazioni AI. Le modalità
che richiedono il companion segnalano l'assenza dello Sprout Local Edge Runtime.
Il popup non propone di creare o selezionare un agente.

## Chat del singolo agente

1. La directory restituisce anche `profile_resource_node_id` e `key_epoch`.
2. Il browser cifra domanda e istruzioni con la chiave del profilo, all'epoch
   esatto. Il dominio AAD `agent-chat` è distinto dalle risorse product; ID della
   sessione e progetto sono autenticati. Nessun fallback a chiavi di sviluppo.
3. `POST .../agents/{agent_id}/interrogations` registra il transcript con
   `causal_delta` vuoto. `POST .../invocations/client-provider` accoda un task
   `answer_from_authorized_context`, con il profilo corrente come unica fonte
   esplicita e nessuna authority di scrittura o tool.
4. Il runner autenticato come agente prende il claim, risolve la fonte corrente,
   esegue il provider locale configurato e invia output cifrato e osservazione
   firmata. Il browser non utilizza la sessione umana per prendere un claim.
5. `GET .../interrogations` restituisce transcript, risposta cifrata e stato
   dell'invocazione. Il browser decifra localmente e aggiorna la conversazione.

Le API agentiche usano il tipo domain con `nonce` e `ciphertext` come array di
byte; le API product e il wrapper WASM usano `nonce_b64` e `ciphertext_b64`.
`ai/agent-api.ts` converte esplicitamente i due formati. Anche il transport del
runner converte i claim, mentre output e artifact sono convertiti **prima** del
calcolo dei commitment. Il commitment dell'output usa le chiavi ordinate come
la proiezione `serde_json::Value` del backend. I descrittori `InformationSource`
TypeScript sono allineati all'enum Rust.

## Storico e recupero della chat agente

- Nuova GET sulla collection delle interrogazioni: 30 elementi, ordine
  `(created_at, id)` decrescente, `before=<next_cursor>` per le pagine precedenti.
- Nuova `GET .../invocations/{invocation_id}`: solo ID, stato, attempt e limite.
  Non restituisce endpoint, provider, chiavi, failure text o output plaintext.
- Le nuove letture verificano membership e creatore, oltre alla RLS. Gli agenti
  non possono usare queste letture umane. Un altro membro non vede le chat.
- Polling ogni quattro secondi, seriale, sospeso in una scheda nascosta/offline;
  le risposte tardive sono ignorate al cambio di agente/progetto.
- L'invio salva gli envelope nel vault locale prima delle POST. Dopo una risposta
  persa, verifica la presenza degli stessi ID prima di ripetere la richiesta.
  **Riprendi invio** conserva ID e ciphertext e non rigenera il messaggio.
- Gli envelope pendenti sono isolati per account, progetto e agente. La loro
  sopravvivenza alla chiusura dipende dalla persistenza del vault: un vault
  session-only non promette recovery su un nuovo avvio. Lo storico già salvato
  nel backend resta leggibile quando le chiavi tornano disponibili.
- Massimo 4.000 caratteri per domanda. I transcript legacy con altro formato
  o chiavi mancanti sono mostrati come bloccati, senza errori non gestiti.

## Collegamento del runner agente

Il requisito R5 del runtime nativo resta effettivo. Questo cambiamento collega
il browser al backend e fornisce il codec condiviso; **non installa né avvia un
companion nativo o un provider**. Servono un agente provisionato con envelope
firmato, un runner attivato e le chiavi autorizzate. La UI blocca gli invii per
runner `pending_key`/`revoked`, agenti sospesi e utenti diversi dal controller.

Nel processo nativo usare `ApiAgentLanguageTransport` con la sessione agente e
`runOneClientOwnedInvocation`. Per i messaggi prodotti dalla scheda agente,
`createAgentChatCrypto` fornisce `EdgeLanguageCrypto`: riceve vault del runner,
progetto, risorsa/epoch del profilo e il resolver delle fonti autorizzate. Creare
un codec per invocazione; nessuna memoria nascosta del modello viene aggiunta.
Il provider, le credenziali, il signer del device e il commitment del profilo
rimangono responsabilità del runner. Un epoch non più disponibile fallisce
senza tentare una chiave diversa. Le impostazioni AI del browser restano locali
al dispositivo e non configurano automaticamente un processo runner agente
separato. Ask to AI legge lo stesso profilo locale, ma invia la generazione al
bridge nativo dell'utente senza passare dal runner di un agente.

La chat è in sola lettura. Assegnazioni, commenti nativi R5.41, strumenti e run
collaborative continuano a usare i rispettivi percorsi governati. Le bozze
cosmetiche dell'editor non modificano prompt approvati o permessi; per gli agenti
reali l'editor mostra soltanto la personalizzazione visiva.

## Verifiche effettuate

- Build TypeScript/Vite e compilazione Rust del server.
- Test frontend AI/API/chat/App, inclusa la proiezione del workspace e l'assenza
  di `agent_id`; i test live provider restano saltati quando mancano endpoint o
  credenziali esterni.
- 5 test con il WASM reale, incluso browser → codec nativo → risposta cifrata →
  browser e rifiuto di sessione, progetto ed epoch errati.
- Test PostgreSQL `browser_chat_history_and_invocation_status_are_creator_scoped`:
  migrazioni, provisioning, 32 interrogazioni, paginazione 30+2, enqueue, claim,
  submit con firma del runner, lettura della risposta e isolamento. L'endpoint
  del modello è simulato; non è una prova di inferenza commerciale live.
- Verifica visiva dei componenti reali in una preview con dati fittizi, incluso
  invio della chat agente e contenimento del popup.
- La suite TasksScreen ha 6 fallimenti preesistenti, riprodotti anche da `HEAD`
  prima delle modifiche: History categoria, filtro task, hover/modifica/salvataggio/
  annullamento lista e passaggio da history a info lista. Nessuna nuova regressione
  rilevata in quei casi.

Test backend ripetibile, **solo con un database disposable**; il test applica
le migrazioni del repository:

```bash
DATABASE_URL=postgresql://USER@127.0.0.1:PORT/DATABASE_DISPOSABLE \
  cargo test -p sprout-server --test agents --locked \
  browser_chat_history_and_invocation_status_are_creator_scoped \
  -- --ignored --exact --test-threads=1
```

Per avvio backend/frontend e proxy restano valide le istruzioni in
`agent-r5-0035-manual-validation.md`, adattando i percorsi e le porte all'istanza
reale. Il collaudo di questa integrazione non modifica i database indicati nella
guida e non dichiara eseguito il suo intero protocollo di formal release F–G.
