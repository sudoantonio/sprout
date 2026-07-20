# Stato dei macrotask

## #5 — E2EE Rust/WASM, lifecycle chiavi, recovery unanime, sync offline

Il macrotask è chiuso per proseguire lo sviluppo. I seguenti gap sono differiti
e non bloccano i macrotask successivi:

- **HLT-06:** il journey Docker verifica condivisione, revoca, rotazione e
  fail-closed della recovery non provisionata, ma non automatizza ancora
  l'intera cerimonia con tre partecipanti, due dispositivi, consenso unanime,
  finalize e decrittazione dal dispositivo recuperato.
- **HLT-07:** gli oracle coprono idempotenza, conflitto, REST catch-up e
  isolamento di due contesti browser, ma non eseguono ancora un journey
  completo con due PWA autenticate offline, modifica concorrente, risoluzione
  client-side e convergenza finale.
- **T-LLR-06.5:** la revoca online e il diniego sulle epoche successive sono
  automatizzati; manca una matrice dedicata per dispositivi revocati rimasti
  offline.
- **Gate produzione:** l'audit crittografico indipendente
  **T-LLR-11.5** rimane esterno e il protocollo continua a fallire chiuso in
  produzione finché l'evidenza non è disponibile.

Questi gap saranno ripresi nella fase finale di hardening. Non riducono le
garanzie dichiarate: le evidenze più strette restano esplicitamente limitate e
non vengono presentate come prove delle cerimonie complete.

## #6 — Tasklist, task, preset/pretask, snapshot, copie e ricorrenze

Chiuso. La PWA implementa il journey preset versionato con pretask `priority`,
`deadline` e `recurring`, selezioni separate, assegnazione e materializzazione
di snapshot cifrati. Completamento periodico e copia creano nuove risorse con
header/body key separation, epoca ed envelope.

La cerimonia `scripts/validation/hlt03-task-domain.sh` verifica **HLT-03** e
avvia due worker concorrenti sulla stessa occorrenza per **T-LLR-03.6** e
**T-LLR-07.5**. Vincolo univoco, transazione e idempotenza lasciano esattamente
una nuova occorrenza.

## #7 — Questionari versionati, file cifrati e allegati

Chiuso per proseguire lo sviluppo. **HLR-04** è funzionalmente completo; la validazione client copre
domande aperte, scelta singola/multipla, booleane, obbligatorietà e isolamento
delle opzioni. Le bozze e le submission storiche vengono nuovamente
decrittate nel form, mentre il retry verifica prima una submission già
confermata e riusa un idempotency key stabile.

Per **HLR-05** sono automatizzati cifratura/AAD del file, cache OPFS con soli ID
opachi, upload same-origin senza path locali e download da un secondo
dispositivo autorizzato. La PWA consente ora di allegare file ai tre pretask,
li ricifra sotto la chiave del task materializzato e conserva il riferimento
`source_template_attachment_id`; il completamento può selezionare il requisito
concreto. Gli oracle browser verificano che OPFS e traffico outbound non
contengano plaintext o path locali e che contenuti HTML/SVG ostili siano
forzati in download opaco.

**T-LLR-04.4** è ora automatizzato anche nel browser: dopo un commit seguito
dalla perdita della risposta, la PWA esegue una lettura autorevole, accetta
solo la stessa submission nello stato `submitted` e conserva l'errore ambiguo
se identità o stato non coincidono.

Per **HLT-05** la PWA persiste ora in OPFS il ciphertext completato e in una
coda IndexedDB separata soltanto ID opachi, hash e metadati cifrati. Al ritorno
online riusa dichiarazione e idempotency key stabili, carica il ciphertext e
finalizza. Un oracle Chromium a due contesti esegue provenance
template→required→completed, staging offline, sync e lettura/decrittazione dal
secondo dispositivo autorizzato. Esiste ora anche il bridge unico verso il
backend Docker reale: il journey disposable registra e invita gli utenti,
crea via recovery un secondo device autenticato, consegna e verifica i suoi
envelope, prepara la provenance immutabile e passa al browser soltanto
identificatori e materiale cifrato. Il browser dichiara template e requisito
sul backend reale, completa offline, sincronizza e legge/decritta dal secondo
device. Il gate è chiuso da un'esecuzione Docker/Chromium verde; durante la
cerimonia sono stati corretti sia la copertura envelope di tutti i device
attivi durante la rotazione, sia il cast `SUM(bigint)` usato per la quota blob.

## #8 — Retention, export e purge

Chiuso per proseguire lo sviluppo. Le finestre UTC e calendar-month, la chiusura
delle dipendenze, l'opt-in per utente, l'isolamento degli archivi e la verifica
di checksum/firma sono automatizzati. Due worker concorrenti producono una sola
notifica in-app e una sola email per destinatario e finestra.

La PWA carica notifiche e archivi al login, mostra gli avvisi di retention e
offre esclusivamente download espliciti. Il worker elimina riga e file
dell'archivio esattamente alla scadenza di 30 giorni, indipendentemente dalla
ricevuta di download.

**HLT-08** è chiuso da un singolo oracle a clock controllato: warning
concorrenti, opt-in e isolamento, export-before-purge, lettura autenticata al
login, download forzato, corruzione fail-closed ed expiry esatta vengono
eseguiti nello stesso ciclo. Il clock viene mosso soltanto nel database
disposable del test, senza introdurre un control plane temporale nel server.

**T-LLR-08.6** è ora automatizzato anche per il riavvio reale di PostgreSQL:
un lock Docker controllato ferma il worker dopo il claim durevole, PostgreSQL
viene riavviato, l'oracle verifica l'assenza di effetti DB parziali, porta il
lease alla scadenza e dimostra un solo retry con marker e cancellazione
esattamente una volta. Restano inoltre verdi export-before-purge, disk-full,
crash nei punti di persistenza e lease scaduta.

## #9 — PWA e comportamento locale

Chiuso. Sono automatizzati:

- richiesta e rifiuto della persistenza browser;
- conservazione della coda cifrata quando una scrittura IndexedDB fallisce con
  `QuotaExceededError`;
- ispezione reale delle Cache Storage dopo una risposta API contenente marker
  classificati e materiale chiave;
- CSP, Trusted Types, blocco di un payload XSS inline e assenza di script
  third-party;
- matrice browser, filtro locale su dati decifrati e fallback filesystem già
  presenti.

Durante l'oracle CSP è emerso e stato corretto un bug di produzione: con
`require-trusted-types-for 'script'`, la registrazione del service worker usava
una stringa e veniva rifiutata da Chromium. Ora `/sw.js` passa attraverso la
policy Trusted Types `sprout`, limitata a quell'unico URL.

**HLT-09** esegue ora nello stesso browser il rifiuto della persistenza,
l'upgrade IndexedDB v1 → v2, il recupero selettivo della coda firmata, il
caricamento della shell sotto controllo del service worker, il reload offline
e il successivo catch-up della mutazione cifrata.

La policy v1 → v2 è approvata e documentata in ADR-0005: lo schema locale viene
ricostruito, vengono conservati esclusivamente gli item che superano
`isRecoverableSignedQueueItem`, mentre vault, projection, tombstone, conflict,
store sconosciuti e item malformati/non firmati vengono eliminati. Le
proiezioni possono essere ricostruite dal server dopo la riautorizzazione; una
mutazione firmata non ancora sincronizzata non viene persa.
