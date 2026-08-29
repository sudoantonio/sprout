# Info: documenti informativi di progetto, topic e task list

## Scopo

`Info` è lo spazio documentale E2EE associato al progetto, a un topic o a una
task list.
Serve a conservare informazioni durevoli che descrivono il contenitore, senza
confonderle con le task operative o con lo storico delle task.

Nella UI corrente il dettaglio di una task list contiene due viste:

- **Storico**: la vista preesistente con la cronologia delle task;
- **Info**: il documento collaborativo con testo Markdown, link, file,
  immagini e sotto-documenti.

Il backend supporta documenti Info per il progetto radice, per i topic e per
le task list. Il frontend espone il documento del progetto e del topic nella
vista `Overview`, oltre alla vista `Info` nel dettaglio delle task list.

Questo documento descrive il comportamento realmente implementato. Per il
modello generale di autorizzazione vedere
[Sistema dei permessi](permissions.md).

## Esperienza utente attuale

Aprendo il dettaglio di una task list, l'utente può scegliere la tab `Info`.
Al primo accesso il client:

1. carica tutti i documenti Info della task list;
2. individua il documento senza parent, cioè la root;
3. se la root non esiste, ne crea una vuota;
4. in caso di creazione concorrente, ricarica la collection e usa la root
   creata dall'altro client.

La toolbar della vista Info offre tre azioni:

- `Testo`: apre l'editor Markdown;
- `File o immagine`: seleziona e carica un singolo file;
- `Documento`: crea un sotto-documento del documento corrente.

I sotto-documenti si aprono nello stesso pannello. Un breadcrumb permette di
risalire lungo la gerarchia fino alla root `Info`.

## Modello concettuale

Ogni progetto, topic o task list può avere un solo documento root attivo e un
numero arbitrario di documenti annidati:

```text
progetto, topic oppure task list
└── Info root
    ├── testo Markdown
    ├── file o immagini
    ├── sotto-documento A
    │   ├── testo Markdown
    │   └── file
    └── sotto-documento B
        └── sotto-documento C
```

Un documento Info non è un nuovo `resource_node`: riutilizza il nodo e la
chiave della task list o del topic contenitore. Di conseguenza:

- eredita i permessi del contenitore;
- non può avere grant indipendenti;
- non può essere spostato in un altro topic o in un'altra task list;
- tutti i suoi discendenti devono appartenere allo stesso contenitore;
- usa l'epoca crittografica attiva del contenitore.

Non esiste un limite applicativo esplicito alla profondità dei
sotto-documenti. Parent e contenitore sono immutabili dopo la creazione; questa
regola impedisce di costruire cicli tramite le API normali.

## Payload cifrato

Il contenuto semantico è un JSON `InfoDocumentContent` cifrato integralmente
nel browser:

```json
{
  "schema": 1,
  "title": "Specifiche",
  "blocks": [
    {
      "id": "<uuid>",
      "type": "text",
      "markdown": "# Obiettivo\nConsulta https://example.test"
    },
    {
      "id": "<uuid>",
      "type": "file",
      "blob_id": "<uuid>",
      "file_name": "diagramma.png",
      "content_type": "image/png",
      "plaintext_size": 184320
    },
    {
      "id": "<uuid>",
      "type": "document",
      "document_id": "<uuid>",
      "title": "Dettagli tecnici"
    }
  ]
}
```

Il server non vede:

- titolo del documento;
- testo Markdown;
- URL;
- nome e MIME semantico dei file;
- ordine e tipologia dei blocchi;
- titolo usato per mostrare un sotto-documento.

Il server vede soltanto UUID di routing, parent, versione, epoca,
timestamp, tombstone e ciphertext.

### Tipi di blocco

| Tipo | Contenuto | Comportamento corrente |
| --- | --- | --- |
| `text` | stringa Markdown | L'editor modifica il primo blocco testo del documento. Se manca, ne crea uno in testa. |
| `file` | riferimento opaco al blob e metadata client-side | Immagini mostrate come anteprima; altri file come card scaricabile. |
| `document` | riferimento a un documento figlio e label | Mostrato come card navigabile; il record figlio resta separato. |

L'array `blocks` conserva un ordine cifrato. La UI corrente, però, visualizza
prima il primo blocco testo, poi il gruppo dei file e infine il gruppo dei
sotto-documenti. Non è ancora un editor block-by-block con interleaving,
drag-and-drop o riordinamento visuale.

## Markdown e riconoscimento dei link

L'editor conserva il testo sorgente e la preview usa CommonMark con le
estensioni GitHub Flavored Markdown. Sono supportati:

- heading con anchor interni deterministici;
- paragrafi, soft newline e hard break con due spazi finali;
- escape Markdown;
- enfasi, grassetto e testo barrato;
- liste ordinate e non ordinate annidate, incluse numerazioni iniziali diverse
  da `1` e task list;
- blockquote annidati con Markdown interno;
- tabelle responsive con Markdown nelle celle;
- link inline, reference link e URL automatici;
- immagini Markdown remote HTTPS con alt text, title, lazy loading e fallback;
- inline code, incluso contenuto delimitato da più backtick;
- fenced code block completi con syntax highlighting.

L'HTML contenuto nel Markdown non viene interpretato: resta disabilitato per
non introdurre script, iframe o markup arbitrario nella superficie E2EE.

I link non sono un tipo di dato server. Vengono riconosciuti esclusivamente nel
client all'interno del testo cifrato:

- `http://...` o `https://...` termina al primo whitespace o doppio apice;
- `"http://..."` o `"https://..."` può contenere spazi e termina al doppio
  apice successivo;
- lo schema è limitato a HTTP e HTTPS;
- il link si apre in una nuova tab con `noopener` e `noreferrer`.

I link usano il colore accent della UI. File e immagini allegate usano il
trattamento visuale warning/arancione già definito dal design della vista.
Le immagini inserite tramite sintassi Markdown vengono renderizzate nel flusso
del documento; la CSP consente soltanto origini HTTPS oltre a `self`, `data:` e
`blob:`. La richiesta usa `Referrer-Policy: no-referrer`, ma il server remoto
può comunque osservare l'indirizzo di rete del dispositivo: un'immagine
allegata e cifrata resta quindi preferibile per contenuti sensibili.

Il riconoscimento resta client-side per evitare che il backend apprenda quali
stringhe sono URL.

## File e immagini

### Flusso di upload

Il caricamento avviene in queste fasi:

1. il browser ottiene la chiave della risorsa e l'epoca attiva;
2. genera `blob_id` e block ID casuali;
3. cifra il file nel browser con la chiave del topic/task list e AAD che lega
   progetto, risorsa, blob ed epoca;
4. conserva temporaneamente il ciphertext in OPFS;
5. cifra separatamente metadata tecnici e metadata visuali, inclusi nome file
   e content type;
6. dichiara il blob al server con dimensione e SHA-256 del ciphertext;
7. invia il ciphertext con `PUT` all'URL opaco restituito;
8. verifica che il blob risulti `available`;
9. aggiunge il blocco `file` al payload cifrato del documento e salva una
   nuova versione del documento.

La dichiarazione server verifica:

- epoca attiva della risorsa;
- dimensione dichiarata;
- hash del ciphertext;
- limite massimo del singolo file;
- quota complessiva del progetto;
- coerenza tra documento e resource node.

Per default il singolo file è limitato dal body limit del server, pari a 8 MiB;
la quota progetto di default è 1 GiB. I valori sono configurabili tramite
`SPROUT_BODY_LIMIT_BYTES`, `SPROUT_BLOB_MAX_FILE_BYTES` e
`SPROUT_BLOB_PROJECT_QUOTA_BYTES`.

### Lettura e download

Il browser:

1. recupera metadata e ciphertext autorizzati;
2. confronta dimensione e SHA-256 con la dichiarazione;
3. decifra localmente con chiave, resource ID, blob ID ed epoca attesi;
4. azzera il buffer plaintext temporaneo dopo aver creato il `Blob` browser;
5. mostra un'anteprima se il MIME cifrato inizia con `image/`, altrimenti una
   card file;
6. scarica usando il nome file decifrato.

Il server risponde sempre come `application/octet-stream`, usa un filename
opaco e imposta `private, no-store` e `nosniff`.

L'associazione tra file e specifico documento è conservata nel blocco cifrato:
nel database il file è collegato alla risorsa contenitore, non al documento in
chiaro. Questo preserva la riservatezza della struttura, ma significa che il
server non può ricostruire semanticamente quale documento mostri il file.

## Sotto-documenti

La creazione di un sotto-documento è composta da due scritture:

1. creazione del nuovo record `info_documents` con `parent_document_id`;
2. aggiunta del blocco `document` al payload cifrato del parent.

Il database verifica che parent e figlio abbiano lo stesso:

- `project_id`;
- topic oppure task list;
- `resource_node_id`.

Il titolo è duplicato nel payload del figlio e nella label del blocco parent.
La UI corrente lo imposta alla creazione ma non offre ancora rename o
sincronizzazione automatica delle due copie.

## Persistenza PostgreSQL

La tabella `info_documents` contiene:

| Campo | Scopo |
| --- | --- |
| `id`, `project_id` | identità e isolamento progetto |
| `topic_id` / `task_list_id` | entrambi `NULL` per il progetto, altrimenti identifica il contenitore |
| `parent_document_id` | gerarchia ricorsiva; `NULL` identifica la root |
| `resource_node_id` | risorsa che governa permessi e chiavi |
| `encrypted_payload` | contenuto opaco serializzato |
| `key_epoch` | epoca E2EE usata dal payload |
| `payload_version` | optimistic concurrency, iniziale `1` |
| `created_by_identity_id` | audit del creatore |
| `created_at`, `updated_at` | metadata operativi |
| `deleted_at` | soft delete |

Vincoli principali:

- al massimo uno tra `topic_id` e `task_list_id` può essere valorizzato;
- entrambi `NULL` identificano il contenitore progetto e richiedono il suo
  resource node radice;
- può esistere una sola root attiva per contenitore;
- un documento non può essere parent di sé stesso;
- il payload cifrato non può essere vuoto;
- la coppia risorsa/epoca deve esistere;
- il creator deve appartenere al progetto;
- update di progetto, contenitore, parent o resource node sono vietati;
- un figlio deve condividere il contenitore del parent;
- RLS è abilitata e forzata; la policy di progetto richiede membership attiva.

Le collection vengono restituite in ordine `created_at, id`, ma il contratto
non assegna a tale ordine un significato documentale. Parent/child e ordine di
presentazione restano nel payload cifrato.

## API

### Documento generale del progetto

```text
GET  /v1/projects/{project_id}/info-documents
POST /v1/projects/{project_id}/info-documents
```

### Documenti di topic

```text
GET  /v1/projects/{project_id}/topics/{topic_id}/info-documents
POST /v1/projects/{project_id}/topics/{topic_id}/info-documents
```

### Documenti di task list

```text
GET  /v1/projects/{project_id}/task-lists/{list_id}/info-documents
POST /v1/projects/{project_id}/task-lists/{list_id}/info-documents
```

### Documento singolo e file

```text
GET    /v1/projects/{project_id}/info-documents/{document_id}
PUT    /v1/projects/{project_id}/info-documents/{document_id}
DELETE /v1/projects/{project_id}/info-documents/{document_id}
POST   /v1/projects/{project_id}/info-documents/{document_id}/files
GET    /v1/projects/{project_id}/files/{blob_id}
PUT    /v1/projects/{project_id}/files/{blob_id}/content
GET    /v1/projects/{project_id}/files/{blob_id}/content
```

I DTO canonici sono in `crates/api-contract/src/lib.rs`; il client TypeScript
generato è in `frontend/sprout-web/src/api/contracts.ts`.

### Creazione e update

La creazione richiede:

- UUID scelto dal client;
- parent opzionale;
- resource node del contenitore;
- epoca attiva;
- payload E2EE;
- idempotency key.

L'update richiede anche `expected_payload_version`. Il client cifra il nuovo
payload usando `aggregateVersion = payload_version + 1`; il server applica
l'update soltanto se la versione attesa coincide e incrementa la versione in
modo atomico. Un conflitto restituisce `409`.

La idempotency key deduplica attualmente l'evento outbox; non trasforma da sola
una seconda `POST` con lo stesso document ID in un replay applicativo valido.

Create e update scrivono un record outbox con aggregate
`info_document`. Il soft delete corrente non genera un evento outbox.

## Autorizzazione

Info è intenzionalmente collaborativa. Chiunque abbia visibilità `full` del
body del topic o della task list può leggere e modificare Info, a prescindere
dal livello `view`, `comment`, `edit` o `manage`.

| Operazione Info | Requisito |
| --- | --- |
| Lista e lettura documenti | `Read` sulla risorsa contenitore |
| Creazione, update e soft delete | `EditInfo`, equivalente alla visibilità body `full` |
| Dichiarazione e upload file | `EditInfo` |
| Lettura metadata e download file | `Read` |

Sono quindi autorizzati:

- owner e admin attivi;
- creator del topic/task list;
- destinatari di qualunque grant `full`, incluso `view/full` e
  `comment/full`.

Non sono autorizzati:

- membri senza grant né diritto creator;
- grant `container_only`, anche se il livello è `manage`;
- membership sospese, lasciate o assenti.

Il diritto di modificare Info non concede automaticamente update o delete del
topic/task list e non permette di gestirne gli ACL.

## Cifratura end-to-end

Il documento riutilizza la resource key del contenitore, ma il ciphertext è
legato al proprio document ID. Il contesto crittografico include:

- `project_id`;
- `document_id` come resource ID del payload;
- kind del contenitore: `topic` oppure `task-list`;
- `payload_version`;
- `key_epoch`;
- resource key del topic/task list.

Questa separazione impedisce di copiare validamente il ciphertext:

- da un documento a un altro;
- tra topic e task list;
- tra versioni o epoche differenti;
- tra progetti differenti.

I file hanno un contesto distinto che include anche il `blob_id`. Il server non
possiede plaintext, DEK o KEK e non esegue link detection, parsing Markdown,
generazione preview o lettura dei nomi file.

La revoca del grant sul contenitore ruota l'epoca della risorsa. Le successive
modifiche Info usano la nuova epoca; i payload storici mantengono l'epoca con
cui furono cifrati. Come nel resto di Sprout, la revoca non cancella dati o
chiavi già ricevuti da un dispositivo prima della revoca.

## Eliminazione e retention

`DELETE` applica un soft delete ricorsivo al documento selezionato e a tutti i
suoi discendenti. Le normali operazioni non cancellano fisicamente le righe.

La cancellazione fisica è riservata alla pipeline retention. Quando un topic o
una task list viene purgato fisicamente, trigger dedicati eliminano anche i
relativi documenti Info, sempre sotto il controllo della retention.

La UI corrente non espone ancora un comando per eliminare documenti, file o
blocchi, anche se il backend espone il soft delete del documento.

## Errori e concorrenza

| Stato | Significato tipico |
| --- | --- |
| `401` | sessione assente o non valida |
| `403` | nessuna visibilità body `full` sul contenitore |
| `404` | contenitore/documento/blob inesistente, eliminato o non visibile |
| `409` | versione del documento superata oppure stato upload non coerente |
| `413` | dimensione file o quota progetto superata |

L'editor mostra il messaggio d'errore ma non implementa ancora merge,
ricaricamento automatico o risoluzione visuale di un `409`. Salva e upload
richiedono una sessione server e connettività; non usano attualmente la coda
offline generica delle task.

## Copertura di test presente

La feature dispone attualmente di verifiche per:

- riconoscimento di link HTTP/HTTPS quotati e non quotati;
- immagini remote con title, alt, lazy loading e fallback;
- GFM: URL automatici, tabelle, barrato e reference link;
- heading/anchor, hard break, newline ed escape;
- liste ordinate/non ordinate annidate e numerazione iniziale;
- blockquote annidati e Markdown interno;
- inline code con backtick e code block JavaScript completo con highlighting;
- contesto di cifratura del documento Info;
- apertura della tab Info, caricamento, rendering link e salvataggio testo nel
  test component React;
- sincronizzazione deterministica tra alias della chiave progetto e del
  resource node radice, senza generare una chiave sostitutiva incompatibile;
- retry della Overview `Generali` dopo un errore di decifratura;
- presenza di schema, indici, FK, trigger e RLS nelle verifiche SQL;
- unicità della root e impossibilità di spostare un documento fuori dal
  contenitore nei test PostgreSQL;
- autenticazione richiesta sulle route HTTP;
- policy `EditInfo` per owner/admin, creator e tutti i livelli con scope
  `full`, con negazione per `container_only`.

Non è presente al momento uno scenario Playwright dedicato che attraversi
l'intero flusso Info con backend reale, upload file, navigazione annidata e
concorrenza tra due utenti.

## Limiti e casi parziali attuali

1. **Un solo blocco testo modificabile.** Il renderer copre CommonMark/GFM, ma
   l'editor modifica soltanto il primo blocco testo del documento.
2. **Ordine visuale raggruppato.** File e documenti sono mostrati in sezioni
   separate, non intercalati nel testo secondo l'ordine completo dei blocchi.
3. **Operazioni composte non atomiche.** Upload file e creazione figlio
   precedono l'update del payload parent. Se l'ultimo update fallisce, può
   restare un blob o un figlio valido ma non referenziato nella UI finché una
   procedura di cleanup non lo gestisce.
4. **Nessuna gestione blocchi.** La UI non consente ancora rename, delete,
   unlink, reorder o drag-and-drop.
5. **Conflitti manuali.** Un `409` non viene automaticamente risolto o unito.
6. **Nessun E2E automatizzato dedicato.** La copertura è distribuita tra test unitari,
   component, route-auth e SQL.

Questi limiti non autorizzano a spostare plaintext o struttura semantica sul
server. Le evoluzioni devono preservare il confine E2EE.

## Invarianti per evoluzioni future

Qualunque modifica a Info deve mantenere:

1. un solo contenitore per documento;
2. una sola root attiva per progetto/topic/task list;
3. parent e figli nello stesso progetto, contenitore e resource node;
4. contenuto, URL, ordine, titoli, filename e MIME semantici cifrati;
5. AAD legata a progetto, document/blob ID, kind, versione ed epoca;
6. nessun ACL separato capace di divergere dal contenitore;
7. accesso negato a `container_only`;
8. modifica Info concessa a tutti e soli i viewer `full`;
9. optimistic concurrency senza last-write-wins silenzioso;
10. cancellazione fisica soltanto tramite retention;
11. nessuna decifratura o preview lato server;
12. test API, RLS, crittografici e browser per ogni nuovo tipo di blocco.
