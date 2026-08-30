# Frontend development log

## 2026-08-27 — Navigazione Timeline nell'intestazione

- Spostato il selettore dell'intervallo nel primo riquadro dell'intestazione Timeline, sopra le task list e allineato ai giorni.
- Rimossi i comandi `−`, `Oggi` e `+` dal footer Timeline.
- Estesi griglia e gutter Timeline fino al bordo inferiore, recuperando lo spazio precedentemente occupato dal footer.

## 2026-08-27 — Colonna Timeline continua

- Alleggerito il peso tipografico del selettore dell'intervallo temporale.
- Spostata l'ombra dal singolo riquadro della task list alla colonna sinistra continua della Timeline, mantenendola visibile per tutta l'altezza della griglia.

## 2026-08-27 — Stato vuoto History essenziale

- Rimossi eyebrow, titolo e descrizione dallo stato vuoto della sezione History.
- Aggiunto un unico messaggio centrale: `No completed tasks yet.`

## 2026-08-27 — Allineamento ricerca e board

- Eliminato il margine destro aggiuntivo del gruppo filtro/ricerca desktop: il bordo destro della barra di ricerca ora coincide con quello della board attiva in Overview, Board, Timeline e History.

## 2026-08-27 — Modalità Agenti nella cornice Board

- Rimossa la breadcrumb/percorso file dalla modalità Agenti.
- Aggiunti toolbar con filtro operativo Working/Done/Rest e ricerca agenti, tab `Overview` e azione `Ask to AI` coerenti con le viste task list.
- Eliminata la vista Board dedicata agli agenti; l'Overview ora usa lo stesso contenitore, ingombro e stile della board Overview delle categorie.

## 2026-08-27 — Percorso file nel footer board

- Aggiunto il percorso `Progetto › sezione corrente` sul lato sinistro del footer, allineato orizzontalmente ad `Ask to AI` sulla destra.
- Uniformati dimensione e peso tipografico del percorso a `Ask to AI`.
- Allineato l'inizio del percorso con il testo della tab `Overview` su desktop.
- Per le categorie, completato il breadcrumb con il livello `Generali` (es. `Mita › Generali › prova`).

## 2026-08-27 — Selezione coerente nelle viste sidebar

- Applicato a `Membri` e `Agenti` lo stesso fondo di selezione delle categorie attive nella sidebar, anche nella variante tactical.
- Esteso il fondo selezionato delle due viste all'intera larghezza della riga categoria, senza spostarne icona o testo.
- Rimosso il clipping orizzontale della navigazione desktop per lasciare visibili i bordi completi del fondo selezionato.
- Sostituita l'estensione tramite pseudo-elemento con una riga attiva a larghezza reale: evita il taglio causato dai livelli sovrapposti della sidebar.
- Rimosse le traslazioni negative tra il selettore progetto e le viste: le righe `Membri` e `Agenti` non si sovrappongono più alla fascia superiore della sidebar.

## 2026-08-27 — Gerarchia vista Agenti

- Aumentata la gerarchia tipografica delle sezioni operative (`Working`, `Done`, `Rest`) e rientrate le icone agente rispetto ai rispettivi titoli.
- Verificata la guida backend: la conversazione sicura è una `interrogation` E2EE, non una chat CRUD; il frontend corrente dispone del read model directory/provisioning ma non ancora del flusso crittografico di transcript e delle relative richieste firmate.

## 2026-08-27 — Workspace dettaglio agente

- Il click su un agente (incluso l'esempio Atlas) apre ora il suo workspace dedicato, composto da identità/stato, area chat e colonna task.
- Le task sono lette dalle assegnazioni già presenti e sono apribili dalla colonna laterale; sono separate in aperte e completate.
- L'area chat espone il punto di ingresso della conversazione ma non simula invii: il wiring E2EE per le interrogation resta il requisito successivo per renderla operativa.
- Verifiche: lint, build e test di `AgentManagementPanel` superati.
- Ridisegnata la composizione visiva con le stesse superfici, bordi arrotondati, ombra e ritmo tipografico delle board principali: conversazione e task sono ora due card coerenti e compatte, non una dashboard vuota.
- Semplificata ulteriormente su richiesta: chat e task sono ora integrati nella board, senza card, titoli o stati vuoti; il composer usa textarea e invio circolare stile ChatGPT, mentre la colonna task usa righe con cerchio e azione `+`.
- Portata l'identità dell'agente più in alto, rimosso il sottotitolo di esempio e trasformato il composer in una superficie alta con area testo e controlli inferiori `+` / invio, sul modello dell'interfaccia ChatGPT.
- Allineati avatar e nome dell'agente al margine alto-sinistro della board, aumentata la gerarchia del nome e reso il composer più compatto, basso e con ombra attenuata.
- Chiarita la superficie del composer e aggiunto il controllo modello `Sprout 1` con freccia accanto all'azione di invio.
- Aggiornato il placeholder del composer in `Ask everything` e aumentata la dimensione del testo inserito.
- Uniformati dimensione e peso del selettore `Sprout 1` al testo del composer; il placeholder usa ora peso regolare.
- Ingrandita l'azione di invio, aumentata la distanza dal selettore modello e sostituita la freccia del selettore con una chevron grafica.
- Uniformato il bordo del composer al bordo della board principale.
- Sostituiti i colori hard-coded delle icone agente con i token cromatici dell'interfaccia e assegnata l'icona gufo all'agente demo Atlas, sia nella directory sia nel dettaglio.
- Rimossa l'azione testuale `Tutti gli agenti`: il rientro alla directory usa ora una freccia minimale sopra l'avatar. Aumentata inoltre la dimensione dell'emoji nel dettaglio agente.
- Posizionata la freccia di ritorno nella fascia superiore della board e sostituita la chevron testuale con una freccia sinistra vettoriale coerente con la navigazione.
- Separata la freccia dall'avatar: ora occupa una riga superiore autonoma nella vista, evitando la sovrapposizione sull'icona agente.
- Aggiunti il titolo `Task Agente` e l'azione testuale `+ Aggiungi Task` nella colonna task del workspace agente.
- Convertita l'azione `Aggiungi task` da pulsante a campo testo inline, senza superficie o bordo da controllo.
- Spostato il composer AI in una riga dedicata della board agente: è ora centrato sull'intera superficie e ancorato al fondo, indipendentemente dalla colonna task.
- Ridotto il margine inferiore esclusivamente nel dettaglio agente per abbassare il composer nella board senza alterare la directory.
- Riordinate freccia di ritorno, avatar e nome su una singola riga: l'identità dell'agente è ora a destra della freccia.
- Compattati avatar, emoji e nome nel dettaglio agente; ridotto il padding superiore della board per portare l'intestazione più in alto.
- Ridotta ulteriormente la scala dell'identità e il padding superiore del dettaglio agente.
- Spostata a sinistra la freccia di ritorno e aumentata la dimensione del titolo `Task Agente`.
- Sostituito il simbolo `+` del campo di inserimento task con il pallino vuoto coerente con le task della lista.
- Aumentata la dimensione del pallino nel campo di inserimento task.
- Spostate freccia, avatar e nome dell'agente fuori dalla board, nella fascia superiore centrata e allineata alla navigazione `Overview`; la board resta dedicata a chat e task.
- Allineati pallino, dimensione e peso tipografico del campo `Aggiungi task` ai controlli e ai titoli delle task nelle board.
- Reso autoespandibile il composer AI: con testo su più righe aumenta l'altezza della textarea e della superficie fino a 18rem, poi abilita lo scroll interno.
- Rimosso il clipping dal contenitore interno del solo dettaglio agente: identità e freccia, poste nella fascia sopra la board, restano visibili.
- Sostituita la tab `Overview` nel dettaglio agente con l'azione `← Back`; identità dell'agente ora nella toolbar reale, centrata e non più affidata a posizionamento fuori dalla board.
- Spostata l'identità dell'agente nella board, in alto a destra sopra la colonna task, con avatar e nome leggermente più grandi; l'azione `Back` resta nella toolbar.
- Corretto l'allineamento dell'identità nel dettaglio agente: avatar e nome sono in alto a sinistra sopra l'area chat; la colonna destra contiene solo le task.
- Uniformata la tipografia del nome agente al titolo `Task Agente`.
- Allineato il composer AI alla colonna chat di sinistra, anziché al centro dell'intera board.
- Ridotto l'avatar nel dettaglio agente e alleggerito il peso dei testi `Task Agente` e `Aggiungi task`.
- Ripristinata la dimensione precedente del nome agente, mantenendo compatti avatar e colonna task.
- Spostato leggermente verso destra il composer AI nella colonna chat.
- Ridotto il diametro dell'azione di invio e regolata la dimensione della freccia verso l'alto sul riferimento della chat.
- Scurita la sola superficie del composer AI nel tema scuro, mantenendo invariati contrasto, bordo e controlli.
- Convertite le icone/avatar degli agenti da cerchio a riquadro con angoli arrotondati, coerente con le board dell'interfaccia.
- Avvicinata l'identità dell'agente (icona e nome) all'angolo alto-sinistro della board del workspace.
- Esteso ulteriormente l'allineamento a sinistra dell'identità agente, per farla aderire visivamente al margine della board.
- Portata l'identità agente a cavallo del bordo superiore della board e rimosso il clipping dedicato al dettaglio, così avatar e nome restano interamente visibili.
- Ripristinata la posizione laterale dell'identità agente; il contenitore board del workspace ora lascia fuoriuscire l'avatar verso sinistra senza troncarlo.
- Disattivato anche il clipping del livello `agent-management` nel solo workspace: l'avatar resta allineato al padding sinistro reale e viene mostrato per intero.
- Rimosso il clipping residuo del pannello stage nel dettaglio agente, ultimo contenitore che mascherava il lato sinistro dell'avatar.
- Consolidate le precedenti eccezioni di overflow in un unico override, posto dopo la regola effettiva del pannello stage: è il livello che tagliava l'avatar nonostante le modifiche precedenti.
- In modalità scura ridotta la luminosità dei colori degli avatar agente, preservandone le rispettive tinte semantiche.
- Uniformati gli angoli del tasto Utente al raggio delle board e degli avatar agente.
- Rimossa l'intestazione testuale `Task Agente` e sostituita con un'icona To-do in alto a destra della colonna task.
- Sostituita l'icona generica con l'asset SVG To-do esatto indicato dall'utente (SVG Repo, licenza CC0).
- Applicata all'icona To-do una maschera cromatica: usa lo stesso token di `Aggiungi task` e quindi si adatta automaticamente a modalità chiara e scura.
- Ripristinato il testo `Task Agente`; l'icona To-do è ora allineata a destra nella stessa intestazione, con il campo `Aggiungi task` sotto.
- Sostituita l'icona To-do con una freccia di compressione coerente con la sidebar: chiude e riapre campo e lista delle task dell'agente, lasciando visibile l'intestazione.
- Compattata e riallineata a destra l'intestazione task: `Task Agente` è ora accanto alla freccia, con dimensione tipografica ridotta.
- Ridotte le dimensioni di avatar e nome nel dettaglio agente; aggiunto menu orizzontale `•••` con campi inline per modificare localmente nome e icona.
- Compattata ulteriormente l'identità agente: nome alla stessa scala di `Task Agente`, avatar ridotto e separatore discreto sotto entrambe le intestazioni del workspace.
- Sostituiti i due separatori locali dell'header con una singola linea continua, estesa da identità agente a colonna task.
- Esteso il separatore unico fino ai bordi della board; quando le task sono compresse, il composer resta ancorato alla colonna chat sinistra invece di ricentrarsi.
- Corretto lo stato compresso delle task: intestazione mantenuta su una riga e composer AI centrato sull'intera board.
- Spostato il separatore dall'area con padding al contenitore board: ora attraversa l'intera superficie, bordo interno incluso.
- Ridotta la distanza verticale fra header agente/task e separatore continuo.
- Uniformate scala, peso e line-height dei titoli agente/task e allineata verticalmente l'identità agente all'intestazione `Task Agente`.
- Rifinito l'allineamento ottico della baseline: l'intestazione `Task Agente` è traslata di 0,2rem per coincidere col nome agente.
- Allineate verticalmente le intestazioni dell'agente e delle task al centro della fascia superiore, tra bordo della board e separatore.
- Corretto il posizionamento della fascia header: nome agente e `Task Agente` sono risaliti di 0,5rem per centrarsi otticamente nello spazio disponibile.
- Semplificata l'intestazione task in `Task` e spostata sul margine destro con la stessa spaziatura laterale dell'identità agente.
- Reso il colore di nome agente e intestazione task coerente con il controllo `Ask to AI`.
- Avvicinata ulteriormente l'intestazione `Task` al margine destro della sua colonna.
- Distaccato leggermente il campo `Aggiungi task` dal separatore dell'header.
- Aggiornata la freccia del pulsante di invio con un'icona SVG pulita, mantenendo lo sfondo circolare originale.
- Centrata geometricamente l'icona di invio nel relativo pulsante circolare.
- Ricostruito l'allineamento del pulsante di invio con layout flex centrato e senza offset manuali dell'icona.
- Rifinito il centraggio ottico della freccia di invio con un offset di 0,5px verso destra.
- Sostituiti testo e chevron dell'header task agente con un unico controllo a icona lista, usato per aprire e chiudere il pannello task.
- Portata l'icona del pannello task a due righe, come da riferimento visivo.
- Con pannello task aperto, spostato leggermente a destra il composer AI; la modalità a pannello chiuso resta centrata.
- Aumentato ulteriormente il rientro a sinistra del composer AI con pannello task aperto.
- Esteso ulteriormente il rientro del composer AI nella vista con pannello task aperto.
- Aggiunta un'animazione slide del composer AI verso il centro alla chiusura del pannello task.
- Aggiunta l'animazione inversa del composer AI alla riapertura del pannello task.
- Aumentata la dimensione del nome agente nell'header della workspace.
- Corretto lo stato dell'agente demo Atlas: senza attività reale compare ora nella sezione `Rest`, non in `Working`.
- Resa circolare l'icona di Atlas nella panoramica e fissato il suo colore standard, indipendente dallo stato operativo.
- Reso circolare il pulsante `+` per creare un agente e applicato lo stesso sfondo del controllo `Nuova task list`.
- Spostato il controllo `+` di creazione agente sul lato sinistro della riga agenti.
- Aggiunta l'etichetta `New` sotto al controllo circolare per creare un agente.
- Aggiunto un bordo leggero al controllo circolare `New`.
- Alleggerito il colore dell'etichetta `New` usando il tono secondario dell'interfaccia.
- Aumentato il contrasto dell'hover delle tasklist rispetto allo sfondo della board, in entrambi i temi.

## 2026-08-27 — Spaziatura sidebar

- Mantenuta la distanza fra i blocchi `Membri`/`Agenti` e categorie; rese invece più compatte le righe interne di entrambe le aree.

Registro cronologico delle modifiche significative al frontend Sprout e delle
attività necessarie a verificarle. Non inserire secret, token, payload
decifrati, dati personali o credenziali.

Ogni voce deve riportare:

- obiettivo e motivazione;
- file o aree coinvolte;
- comportamento modificato;
- verifiche eseguite e relativo esito;
- limiti, problemi noti o operazioni ancora necessarie.

## 2026-08-26 01:13 CEST — Baseline locale R5 checkpoint 0035

### Obiettivo

Preparare una copia verificabile del branch `codex/lean-concrete-refinement`
per il successivo lavoro frontend, usando il precedente lavoro sul branch
`frontend/split` soltanto come riferimento visivo e funzionale.

### Attività

- collegata la copia locale al repository remoto e scaricato il branch
  `codex/lean-concrete-refinement`;
- installate le dipendenze frontend con Node.js 24;
- generato il modulo crittografico WebAssembly con `wasm-pack` 0.15.0;
- creato il database development isolato `sprout2_dev` su PostgreSQL 14;
- avviato il backend in modalità development, limitato a loopback;
- applicate automaticamente le migration 1–35;
- predisposto l'account development `admin.minerva`;
- avviato il frontend Vite su porta 4176 con proxy verso il backend su porta
  18085.

### Verifiche

- lint frontend: PASS;
- build frontend: PASS;
- test frontend senza il test live loopback: 276 PASS, 6 SKIP;
- health backend `live`, `ready` e `trace`: HTTP 200;
- frontend e proxy `/health/ready`: HTTP 200;
- migration database: 35 presenti, versioni 1–35, tutte successful.

### Limiti e note

- il test `src/tools/edge-runtime.live.test.ts` richiede l'apertura di una
  porta loopback ed è stato escluso dalla suite eseguita nel sandbox;
- la build segnala chunk JavaScript superiori a 500 kB, senza impedire la
  compilazione;
- le chiavi runtime usate per questa sessione development sono effimere;
- la UI dedicata alla surface Comment nativa R5.41 non è ancora presente,
  coerentemente con il checkpoint 0035.

## 2026-08-26 01:19 CEST — Analisi navigazione e viste di categoria

### Obiettivo

Mappare la schermata di riferimento per introdurre breadcrumb e quattro viste
di categoria: Overview, Board, Timeline e History, verificando prima il
supporto backend necessario.

### Risultato dell'analisi

- il breadcrumb `Generali > <categoria>` è derivabile dallo stato frontend e
  dai nomi E2EE già decifrati; non richiede una nuova API;
- Board e Timeline sono già implementate e filtrano task list e task in base
  al topic selezionato; richiedono soprattutto una nuova navigazione comune;
- il backend espone già CRUD E2EE per gli InfoDocument di topic e task list,
  inclusi documenti annidati e file;
- il client API TypeScript include già list/create per InfoDocument di topic,
  mentre `App.tsx` collega attualmente soltanto i flussi per task list;
- la precedente Overview di riferimento persisteva HTML/Markdown in
  `localStorage`: non deve essere portata così, perché perderebbe
  collaborazione, sincronizzazione e garanzie E2EE multi-device;
- la vista History della schermata di riferimento mostra i task completati,
  ordinati per `completed_at`; questo dato è già incluso nei Task DTO e non
  richiede nuove route;
- un vero audit event-by-event di creazioni, modifiche, spostamenti e
  completamenti non è equivalente alla History di riferimento e
  richiederebbe un contratto backend dedicato e permission-aware.

### Backend riutilizzabile

- `GET|POST /v1/projects/{project_id}/topics/{topic_id}/info-documents`;
- `GET|PUT|DELETE /v1/projects/{project_id}/info-documents/{document_id}`;
- `POST /v1/projects/{project_id}/info-documents/{document_id}/files`;
- route blob esistenti per upload e download cifrati;
- route task/task list correnti per Board, Timeline e History completati.

### Vincoli

- testo, titoli, URL, nomi file e struttura documentale restano cifrati nel
  browser;
- l'Overview deve riusare la chiave e l'epoca del topic contenitore;
- optimistic concurrency degli InfoDocument deve conservare il comportamento
  `409 Conflict`, senza last-write-wins silenzioso;
- breadcrumb e filtri non devono fabbricare metadata server-owned.

## 2026-08-26 01:32 CEST — Navigazione e viste di categoria implementate

### Obiettivo

Rendere visibili nell'interfaccia corrente il percorso della categoria e le
quattro viste richieste, collegando Overview ai documenti E2EE già supportati
dal backend.

### Modifiche

- aggiunto il breadcrumb contestuale `Generali > <categoria>` nella toolbar;
- sostituito lo switch a due modalità con le tab `Overview`, `Board`,
  `Timeline` e `History`, disponibili anche nella navigazione mobile;
- impostata Overview come vista iniziale per le nuove sessioni e al cambio
  progetto, conservando la scelta corrente nel browser;
- collegata l'Overview di categoria alle route InfoDocument del topic:
  caricamento, creazione del documento radice, sotto-documenti, aggiornamento
  Markdown e file cifrati;
- generalizzato il pannello InfoDocument già usato dalle task list, evitando
  una seconda implementazione dell'editor e del flusso E2EE;
- aggiunta History di categoria, con soli task completati ordinati per
  `completed_at` decrescente e apertura del dettaglio task;
- aggiunto un riepilogo di progetto nell'Overview `Generali`;
- corretto il fallback delle categorie non decifrabili: breadcrumb, Overview
  e History mostrano tutti `Categoria protetta`, senza ricadere erroneamente
  su `Generali`.

### File principali

- `frontend/sprout-web/src/components/TasksScreen.tsx` e `App.css`;
- `frontend/sprout-web/src/App.tsx` e `store/app-store.ts`;
- `frontend/sprout-web/src/components/TaskListInfoPanel.tsx`;
- `frontend/sprout-web/src/components/TaskListHistoryPanel.tsx`;
- `frontend/sprout-web/src/components/TasksScreen.test.tsx`.

### Verifiche

- test mirati TasksScreen e InfoMarkdown: 49 PASS;
- suite frontend senza il test live loopback: 278 PASS, 6 SKIP;
- lint frontend: PASS;
- build TypeScript/Vite: PASS;
- `git diff --check`: PASS;
- verifica nel browser integrato: breadcrumb e quattro tab presenti; Overview
  e History renderizzate; fallback `Categoria protetta` verificato su una
  categoria priva di chiave locale.

### Limiti e note

- l'Overview `Generali` è per ora un riepilogo in sola lettura: il backend
  espone InfoDocument per topic e task list, non per il progetto radice;
- History rappresenta lo storico dei task completati, come nel riferimento,
  non un audit log completo di ogni mutazione;
- il test live `src/tools/edge-runtime.live.test.ts` resta escluso perché il
  sandbox non consente l'apertura della porta loopback richiesta;
- la build mantiene il warning già noto sui chunk JavaScript oltre 500 kB.

## 2026-08-26 01:43 CEST — Editor Markdown abilitato in Generali

### Problema osservato

La nuova Overview mostrava `Generali` come riepilogo di progetto in sola
lettura. L'editor InfoDocument era collegato soltanto alle categorie topic,
quindi il pulsante di modifica non era disponibile nella schermata segnalata.

### Correzione

- aggiunta la migration `0036_project_info_documents.sql` per consentire
  InfoDocument E2EE governati dal resource node radice del progetto;
- aggiunte le route `GET|POST
  /v1/projects/{project_id}/info-documents`;
- esteso il contenitore InfoDocument backend con la variante progetto e con i
  controlli su resource node, epoca, permesso `edit_info`, unicità della root
  e gerarchia dei sotto-documenti;
- aggiunti i metodi client per list/create dei documenti generali;
- estesi cifratura e decifratura InfoDocument con AAD `project` quando
  `topic_id` e `task_list_id` sono entrambi `NULL`;
- sostituito il riepilogo statico di `Generali` con lo stesso editor Markdown,
  file e sotto-documenti già usato dalle categorie e dalle task list;
- aggiornata `docs/info-documents.md` con il nuovo contenitore e le route.

### Verifiche

- migration database corrente: versione 36 applicata;
- verifica strutturale PostgreSQL: una root InfoDocument di progetto collegata
  a un resource node `root`;
- compilazione backend `sprout-server` con Rust 1.88: PASS;
- test mirati TasksScreen e InfoMarkdown: 50 PASS;
- suite frontend senza test live loopback: 279 PASS, 6 SKIP;
- lint, build TypeScript/Vite, `cargo fmt --check` e `git diff --check`: PASS;
- test reale nel browser: apertura editor `Testo`, inserimento Markdown,
  salvataggio server e rendering della preview riusciti; il testo di prova è
  stato poi rimosso e il documento ripristinato vuoto.

### Stato runtime

- backend riavviato su `127.0.0.1:18085` con migration 36;
- frontend Vite ancora attivo su `127.0.0.1:4176`;
- le chiavi operative development del backend restano effimere, come nella
  baseline locale; i contenuti InfoDocument usano invece le chiavi E2EE delle
  risorse progetto.

## 2026-08-26 01:49 CEST — Riparazione chiave Generali e recovery UI

### Incidente

La verifica browser precedente aveva creato la root InfoDocument del progetto
da una sessione development che non possedeva le chiavi originali di `Mita`.
Il fallback development aveva generato una nuova chiave per il resource node
radice: il documento risultava quindi valido per il server ma non decifrabile
dalla sessione corretta dell'utente, che mostrava un `OperationError` WebCrypto.

### Riparazione dati

- identificato esattamente l'unico documento root di progetto creato dal test;
- verificato che fosse il documento vuoto di prova e non contenesse modifiche
  utente;
- applicato un soft-delete soltanto a quel record, vincolando l'operazione a
  UUID progetto/documento, versione payload e contenitore progetto;
- il record resta recuperabile nel database perché non è stato eliminato
  fisicamente dalla pipeline retention.

### Correzione applicativa

- introdotta la sincronizzazione bidirezionale tra l'alias chiave del progetto
  e quello del suo `root_resource_id`; i due alias riusano sempre la stessa
  chiave originaria ed epoca;
- disabilitata esplicitamente la generazione development di una nuova chiave
  casuale per la root progetto;
- mantenuto il recovery da envelope prima di dichiarare la chiave assente;
- aggiunto il pulsante `Riprova` alla vista Info quando load/decrypt fallisce;
- tradotto `OperationError` in un messaggio comprensibile senza esporre
  dettagli crittografici;
- aggiornata la sezione limiti di `docs/info-documents.md`, eliminando la voce
  ormai obsoleta che indicava la UI disponibile soltanto per task list.

### Verifiche

- test di regressione: alias progetto → root e assenza di mint quando entrambi
  gli alias mancano;
- test component: errore di decifratura, click `Riprova` e successivo
  caricamento dell'editor `Generali`;
- suite frontend senza test live loopback: 282 PASS, 6 SKIP;
- lint, build TypeScript/Vite, `cargo fmt --check` e `git diff --check`: PASS;
- backend e frontend development restano attivi sulle porte 18085 e 4176.

## 2026-08-26 02:10 CEST — Agent directory: contratto backend e client

### Analisi del contratto

- verificato il piano UI agenti della guida frontend-independent: la prima
  slice richiesta è un read model server-derived con stato agente,
  disponibilità, controller e runner;
- confermato che `POST /v1/projects/{project_id}/agents` è provisioning
  governato e non una semplice create CRUD: la UI non può sostituire
  certificati, firme e final approval con campi locali o booleani;
- mantenuta la visibilità esistente: RLS restituisce agenti controllati
  dall'utente corrente oppure visibili a owner/admin.

### Implementazione

- aggiunta `GET /v1/projects/{project_id}/agents` sulla route già usata dalla
  POST di provisioning;
- il read model include handle, controller, availability, state, runner
  state/last seen e LocalGoal corrente, senza ciphertext o token;
- aggiunti i contratti TypeScript `AgentDirectoryItemDto`,
  `ListAgentsResponse`, `ProvisionAgentResponse` e i metodi client
  `listAgents`/`provisionAgent`;
- il bootstrap token resta escluso dalla directory e sarà mostrato dalla UI
  soltanto nella risposta immediata al provisioning.

## 2026-08-26 02:20 CEST — UI/UX gestione agenti

### Navigazione e directory

- aggiunta la voce `Agenti` sotto `Membri` nella sidebar, con icona, conteggio
  e stato attivo; collegato anche il pulsante agenti della navigazione mobile,
  che prima non aveva alcuna azione;
- esteso `BoardFocus` con directory e dettaglio agente, mantenendo il percorso
  `Generali › Agenti › {handle}` e rimuovendo filtri/view mode della board
  quando non pertinenti;
- implementata una directory responsive con ricerca, refresh, riepilogo di
  agenti attivi e runner connessi, empty/loading state e scheda di dettaglio;
- il dettaglio mostra soltanto read model server-derived: state, availability,
  runner, last seen, LocalGoal corrente e identificativi tecnici espandibili.

### Creazione governata

- aggiunto il dialog `Nuovo agente` per validare e inviare un
  `ProvisionAgentRequest` completo e firmato;
- la UI rifiuta localmente JSON non valido o envelope incompleti, ma lascia al
  backend la verifica di certificati, firme, controller, scope e policy;
- il bootstrap token è mostrato nella sola risposta di successo, con copia
  esplicita e scadenza, e viene azzerato alla chiusura senza localStorage o log;
- aggiunti test component per directory/dettaglio, envelope incompleto e
  provisioning con token one-shot: 48 test mirati PASS; lint e TypeScript PASS.

### Verifica finale

- backend Rust compilato con toolchain 1.88 e riavviato su
  `127.0.0.1:18085`; la directory vuota è stata letta con successo dalla
  sessione reale, confermando la nuova GET senza errori o fallback client;
- frontend riavviato su `127.0.0.1:4176` con proxy development corretto;
- verifica browser desktop (1440×900): sidebar, conteggio, breadcrumb,
  riepilogo, ricerca, empty state e pulsante `Nuovo agente` corretti;
- verifica browser mobile: pulsante Agenti attivo e stessa directory
  responsive; dialog scrollabile e validazione locale dell'envelope vuoto;
- suite frontend escluso il test live loopback: 285 PASS, 6 SKIP; il solo
  `edge-runtime.live.test.ts` richiede bind loopback e fallisce nella sandbox
  con `EPERM`, invariato e non collegato alla feature;
- build Vite/TypeScript, oxlint, `cargo fmt --check`, `cargo check -p
  sprout-server --all-targets` e `git diff --check`: PASS.

### Stato runtime e limite dichiarato

- frontend e backend development restano attivi rispettivamente su 4176 e
  18085; la scheda browser è lasciata sulla directory Agenti;
- la sessione corrente mostra ancora il warning preesistente sulle resource
  key mancanti per topic/list/task, ma la directory agenti non dipende da
  quelle chiavi e funziona;
- questa slice implementa directory, dettaglio e provisioning completo. Gli
  editor dedicati Responsibility/LocalGoal, runner activation e run monitor
  restano slice successive previste dalla guida e non vengono simulati come
  disponibili.

## 2026-08-26 02:30 CEST — Redesign agenti su reference Working/Done/Rest

### Richiesta visuale

- sostituita la dashboard a card multiple con un solo grande pannello agenti,
  coerente con la reference fornita;
- organizzati gli agenti in tre gruppi centrali `Working`, `Done` e `Rest`,
  identificati anche da indicatore verde, giallo e rosso;
- ogni agente è rappresentato da un avatar circolare con icona, handle e
  stato runner; gli agenti reali restano selezionabili e aprono il dettaglio;
- la classificazione usa esclusivamente dati backend: `Working` richiede
  agente/runner/LocalGoal attivi, `Done` comprende goal completati/falliti o
  agenti ritirati, gli altri stati confluiscono in `Rest`.

### Demo e creazione

- aggiunto `Atlas`, agente dimostrativo sempre marcato `Esempio · Working` e
  non persistito nel database: evita di creare un'identità apparentemente
  governata senza certificati e firme valide;
- aggiunto in `Rest` un tile circolare `+ Nuovo agente`, collegato allo stesso
  dialog di provisioning completo già verificato;
- la sidebar mostra i tre indicatori operativi e conta anche l'agente demo,
  dichiarandolo esplicitamente nell'etichetta accessibile;
- migliorato il comportamento mobile con pannello singolo, gruppi compatti e
  righe di avatar scorrevoli orizzontalmente.

### Verifiche intermedie

- test mirati AgentManagementPanel + TasksScreen: 48 PASS;
- TypeScript/Vite build e oxlint: PASS.

### Verifica finale reference

- verifica browser desktop 1440×900: pannello unico, gruppi centrati, Atlas,
  indicatori sidebar e tile `+` completamente visibili senza clipping;
- click sul tile `+`: apertura corretta del dialog `Nuovo agente AI` con
  focus sull'envelope di provisioning;
- verifica browser mobile 430×900: Working/Done/Rest leggibili, Atlas e tile
  di creazione accessibili, navigazione mobile Agenti ancora attiva;
- suite frontend senza test live loopback: 285 PASS, 6 SKIP;
- build Vite/TypeScript, oxlint e `git diff --check`: PASS;
- frontend e backend locali restano attivi su 4176 e 18085; la scheda viene
  lasciata aperta sulla nuova vista Agenti.

## 2026-08-26 02:45 CEST — Raffinamento Notion-like agenti

- rimossi il refresh manuale e il relativo stato UI; la directory viene
  aggiornata automaticamente ogni 30 secondi e subito dopo un provisioning;
- nascosti i gruppi senza agenti: le sezioni Working, Done e Rest appaiono
  soltanto quando hanno contenuto, lasciando la vista libera da stati vuoti;
- eliminata l'intestazione `Workspace AI / Agenti` e il testo descrittivo dal
  pannello, mantenendo un titolo accessibile non visibile;
- spostato `+ Nuovo agente` nell'angolo del pannello;
- sostituito lo sfondo nero con `--surface-column-add`, lo stesso grigio usato
  dalle task list appena create, e applicato `--radius-card` per gli angoli
  smussati della board;
- verifica browser: nessuna scritta intestazione o pulsante Aggiorna visibile,
  pannello grigio arrotondato e comando di creazione presenti correttamente.

## 2026-08-26 02:50 CEST — Pannello agenti esteso alla board

- aggiunta una variante scoped del layout Board per la modalità Agenti: il
  pannello ora occupa senza margini l'intera area disponibile sotto la toolbar,
  inclusi il bordo destro e quello inferiore;
- lasciati invariati lo sfondo `--surface-column-add` e `--radius-card`, così
  il pannello continua a corrispondere alle task list appena create;
- applicato anche il comportamento equivalente su mobile, rispettando lo
  spazio riservato alla navigazione inferiore.

## 2026-08-26 02:55 CEST — Avatar agenti essenziali

- sostituite le icone generiche nella directory e nel dettaglio con la prima
  lettera dell'handle di ciascun agente;
- trasformato `Nuovo agente` in un solo avatar circolare con icona `+`, della
  stessa dimensione degli avatar degli agenti e con etichetta accessibile.

## 2026-08-26 03:00 CEST — Gerarchia della vista agenti

- spostate le intestazioni di stato più in alto nella stage e aumentato lo
  spazio verticale tra `Working`/`Done`/`Rest` e i rispettivi agenti, per una
  lettura da categorie;
- aumentate dimensioni di avatar, iniziali e pulsante circolare di creazione,
  mantenendo l'allineamento e la proporzione tra elementi.

## 2026-08-26 03:05 CEST — Spaziatura categorie agenti

- rimossa la didascalia `Esempio · Working` dall'agente dimostrativo Atlas;
- portata l'intestazione `Working` più in alto e aumentata ulteriormente la
  distanza verticale tra intestazione e avatar, con un adattamento equivalente
  per mobile.

## 2026-08-26 03:10 CEST — Directory agenti Notion-like

- ridisegnata la directory come superficie piatta e ariosa: contenuto a
  larghezza della board, allineato a sinistra, senza card o indicatori
  decorativi;
- semplificate le intestazioni Working/Done/Rest e resa più netta la gerarchia
  tipografica di categorie, agenti e stato;
- conservati il pannello grigio arrotondato, gli avatar circolari con iniziale
  e il comando `+` già concordati.

## 2026-08-26 03:15 CEST — Rifinitura pannello e creazione agenti

- reintrodotti gli spazi di respiro a destra e in basso del pannello Agenti;
- sostituita l'iniziale dell'agente dimostrativo Atlas con l'emoji `🤖`;
- spostato il comando di creazione nella riga Working, accanto agli avatar, e
  reso un cerchio `+` tratteggiato e semitrasparente.

## 2026-08-26 03:25 CEST — Overview categorie: documento diretto

- trasformata la Overview in una singola pagina Markdown vuota e direttamente
  editabile, senza preview, template, toolbar, file o sotto-documenti;
- salvaggio automatico dopo 900 ms di inattività e al cambio focus, usando le
  già esistenti route InfoDocument E2EE del progetto o della categoria;
- eliminati bordo, sfondo e comandi dall'area di scrittura per una superficie
  essenziale, nello stile di una pagina Linear.app.

## 2026-08-26 03:30 CEST — Superficie Markdown a pagina intera

- esteso il pannello Overview/Markdown all'intera area disponibile della board;
- applicato lo sfondo grigio chiaro della reference (`#f3f2f1`) e rimosse le
  limitazioni di larghezza del documento;
- aggiunto padding interno responsivo ai bordi della superficie di scrittura.

## 2026-08-26 03:32 CEST — Angoli Overview

- applicato al pannello Markdown `--radius-card`, lo stesso raggio usato dalle
  task list, senza ridurne l'estensione nella board.

## 2026-08-26 03:35 CEST — Scorrimento interno Overview

- ridotta la superficie Markdown anche sul bordo inferiore;
- convertita l'Overview in un pannello di altezza fissa nell'area Board: il
  contenuto ora scorre al suo interno, con overscroll contenuto, mentre la
  cornice grigia arrotondata resta ferma.

## 2026-08-26 03:40 CEST — Titolo Overview editabile

- rimosso il testo descrittivo statico `Riepilogo generale del progetto.`;
- trasformato il titolo della pagina in un input diretto, con la stessa
  esperienza di scrittura del corpo Markdown;
- il titolo viene incluso nel payload InfoDocument E2EE e salvato insieme al
  testo dal meccanismo di autosalvataggio esistente.

## 2026-08-26 03:45 CEST — Colonna documento centrata

- mantenuto il pannello Overview a tutta board e centrata al suo interno la
  colonna di titolo e Markdown, con larghezza massima leggibile di 60rem.

## 2026-08-26 03:50 CEST — Focus e impaginazione Notion

- eliminato ogni bordo e alone di focus colorato dal titolo editabile;
- ridotta a 48rem la colonna centrale del documento, ottenendo un'impaginazione
  più raccolta e centrata, ispirata alle pagine Notion.

## 2026-08-26 03:55 CEST — Tipografia corpo Overview

- aumentata a 1.15rem la dimensione del testo Markdown base per migliorarne
  lettura e coerenza con il titolo della pagina.

## 2026-08-26 04:00 CEST — Inseritore blocchi Markdown

- aggiunto sotto al testo un comando `+` in stile Notion;
- il menu inserisce nel documento titoli grande/medio/piccolo, elenco puntato
  e task Markdown, lasciando il cursore pronto per continuare a scrivere;
- il campo Markdown cresce con le righe effettive, così il comando resta subito
  dopo l'ultima riga anziché in fondo a un textarea fisso.

## 2026-08-26 04:05 CEST — Spaziatura titolo Overview

- aumentato il padding superiore del pannello, abbassando titolo e colonna
  documento rispetto al bordo alto della superficie.

## 2026-08-26 04:10 CEST — Spaziatura e menu blocchi Notion-like

- ridotto il distacco superiore e quello tra titolo e corpo per una pagina
  documento più compatta;
- arricchito il menu `+` con icone e preview H1/H2/H3 a dimensione relativa,
  oltre alle icone dedicate per elenco puntato e task.

## 2026-08-26 04:15 CEST — Comando blocco contestuale

- rimossa dalla superficie di scrittura la dicitura `Salvato`;
- reso il comando `+` discreto: compare al passaggio del cursore nell'editor,
  resta visibile durante l'interazione con menu e tastiera.

## 2026-08-26 04:20 CEST — Editor a blocchi Overview

- sostituito il campo raw con un editor a blocchi: heading, elenco e task sono
  ora formattati visivamente, ma serializzati e salvati come Markdown E2EE;
- il comando `+` segue l'altezza reale dell'ultima riga dell'editor;
- aggiunte al menu le azioni Immagine e Documento, collegate all'upload
  cifrato già esistente.

## 2026-08-26 04:25 CEST — Ripristino runtime frontend

- rilevato che il server Vite locale non era più in ascolto su `4176`, causa
  della schermata bianca;
- riavviato il frontend e verificata risposta HTTP 200 su
  `http://127.0.0.1:4176/`.

## 2026-08-26 04:30 CEST — Ripristino stabile editor Overview

- rimosso l'esperimento `contenteditable` introdotto per la formattazione live,
  che poteva interrompere il rendering dell'app;
- ripristinato l'editor Markdown diretto, con salvataggio automatico, titolo
  modificabile, menu `+` e azioni per immagini e documenti;
- verificati 45 test frontend, lint e build di produzione senza errori.

## 2026-08-26 04:35 CEST — Posizione comando blocco Overview

- corretto il vincolo di altezza del campo testo Overview: il textarea ora si
  adatta alle righe scritte, anziché restare alto quanto il pannello;
- il comando `+` segue quindi subito l'ultima riga e compare al passaggio del
  cursore sull'area di scrittura;
- rieseguiti i 45 test della schermata e la build frontend con esito positivo.

## 2026-08-26 04:40 CEST — Rendering Markdown Overview

- introdotto un editor a blocchi robusto per la vista Overview: le righe
  Markdown esistenti vengono visualizzate come titoli, elenchi e task;
- il contenuto rimane direttamente modificabile e viene riconvertito in
  Markdown E2EE al salvataggio, senza esporre la sintassi nella pagina;
- aggiornati test di persistenza del documento; 45 test, lint e build passano.

## 2026-08-26 04:45 CEST — Stabilità aggiornamento live frontend

- individuato il crash della schermata bianca nel client HMR di Vite: la CSP
  richiedeva `TrustedScriptURL` anche per il `SharedWorker` di sviluppo;
- aggiunta una trasformazione `serve`-only che disabilita Trusted Types durante
  lo sviluppo locale, lasciando invariata la CSP rigorosa della build finale;
- ricaricata e verificata la scheda utente: l'interfaccia torna a renderizzare;
- confermati 45 test, lint e build; verificata la presenza di Trusted Types nel
  file `dist/index.html` di produzione.

## 2026-08-26 16:45 CEST — Blocchi interattivi e allegati Overview

- rese interattive le checklist: il click aggiorna e persiste lo stato Markdown
  come `- [ ]` oppure `- [x]`;
- il comando `+` segue la riga vuota attiva e il blocco scelto sostituisce quella
  stessa riga, invece di essere aggiunto sempre in fondo;
- separati nel menu i comandi Immagine, File e Documento: i primi due usano
  l'upload cifrato esistente, il terzo apre la creazione del sotto-documento;
- resi visibili allegati e sotto-documenti anche nella Overview;
- aggiunto un test integrato per checklist, upload immagine e creazione
  documento; 46 test, lint e build passano.

## 2026-08-26 16:50 CEST — Allegati integrati nel documento

- rimossa la presentazione a card con bordi e sfondo dagli allegati Overview;
- le immagini vengono mostrate interamente, senza ritaglio, mantenendo le
  proporzioni originali e con il nome-link di download sotto;
- i file non immagine vengono mostrati come icona documento più link di
  download, senza contenitore decorativo;
- verificati 46 test, lint, build e integrità delle patch.

## 2026-08-26 16:55 CEST — Continuazione documento e resize immagini

- convertito il titolo Overview in un campo multilinea auto-espandibile e
  abilitato il ritorno a capo per parole lunghe in titolo e blocchi Markdown;
- aggiunta alle immagini una superficie ridimensionabile orizzontalmente con
  maniglia nativa in basso a destra, mantenendo proporzioni e limite pagina;
- creato un blocco di testo persistente dopo immagini, file e sotto-documenti:
  il click nell'area vuota posiziona il cursore alla fine e il `+` compare in
  prossimità per inserire un nuovo blocco;
- il testo successivo agli allegati viene salvato come secondo blocco Markdown
  nel documento cifrato, preservando l'ordine logico degli elementi;
- esteso il test integrato alla scrittura dopo un'immagine; 46 test, lint e
  build passano.

## 2026-08-26 17:00 CEST — Controlli editor più visibili e caret stabile

- sostituito il resize nativo delle immagini con una maniglia esplicita in
  basso a destra, trascinabile e utilizzabile anche da tastiera;
- ingrandite e riallineate le checkbox, con stile minimale personalizzato e
  segno di spunta più leggibile;
- portato il comando `+` a 36 px e aumentata la dimensione dell'icona;
- corretto il salto del cursore dopo la scelta dal menu: focus e selezione
  vengono applicati al nuovo blocco prima dell'aggiornamento dello stato, così
  la sincronizzazione React non invalida più il nodo attivo;
- aggiunti test per presenza della maniglia e permanenza del caret nel blocco
  inserito; 46 test, lint e build passano.

## 2026-08-26 17:05 CEST — Focus pulito e ripristino server

- corretto il selettore CSS dell'editor Overview affinché bordo e outline siano
  rimossi anche dal nuovo blocco di testo sotto gli allegati;
- riavviato Vite su `127.0.0.1:4176` dopo l'arresto del processo e verificata
  una risposta HTTP 200;
- confermati 46 test, lint e build di produzione.

## 2026-08-26 17:15 CEST — Ripristino progetti e sessione API

- individuata la causa del `404` durante la creazione/caricamento progetti: il
  frontend Vite era avviato senza backend raggiungibile e senza proxy `/v1`;
- riattivato il backend su `127.0.0.1:18085` usando il database di sviluppo
  esistente nel container PostgreSQL esposto sulla porta `5433`, quindi
  riavviato il frontend su `127.0.0.1:4176` con proxy API esplicito;
- verificato che `/v1/projects` attraversi ora il proxy (risposta `401` senza
  credenziali, anziché `404` del server statico);
- gestita la sessione server scaduta: su `401` l'app elimina la sessione DEV
  obsoleta e torna all'accesso con un messaggio chiaro, così un nuovo login
  ricarica l'intero elenco dei progetti;
- dopo la creazione di un progetto, il selettore viene ora riallineato con
  `GET /v1/projects` e reidrata tutti i progetti restituiti dal backend, invece
  di derivare l'elenco da uno snapshot locale potenzialmente incompleto;
- protette le letture IndexedDB dai normali race di teardown durante logout e
  HMR, evitando rejection non gestite e possibili schermate vuote;
- aggiunto il test di regressione della sessione scaduta; test applicativi
  `App` + `TasksScreen` (50/50), lint e build di produzione passano;
- la suite globale arriva al solo test live `edge-runtime.live.test.ts`, che va
  in timeout perché richiede una chiamata loopback non disponibile nel sandbox.

## 2026-08-26 17:25 CEST — Recupero autenticato delle chiavi legacy

- analizzato il blocco `exact-hits=0`: le chiavi DEV erano presenti, ma nessuno
  slot backup coincideva con i `resource_node_id` delle otto risorse wire;
- aggiunto un recupero compatibile con backup storici: ogni chiave locale viene
  provata contro il ciphertext e il relativo AAD, e viene associata al nuovo
  slot soltanto dopo una decrittazione AEAD autenticata;
- esteso il recupero a payload body, header gerarchici e metadata progetto;
  quando il body non è recuperabile, il client prova anche l'header anziché
  lasciare immediatamente la categoria in stato `Locked`;
- l'associazione autenticata viene persistita nel vault e nel backup DEV, così
  i reload successivi non richiedono una nuova scansione;
- sostituito il messaggio generico «ricrea le risorse» con una diagnosi corretta
  quando nessuna chiave disponibile autentica i ciphertext;
- aggiunto un test di regressione per il rebinding crittografico degli slot;
  test applicativi e sicurezza selezionati (63/63), lint e build passano.

## 2026-08-26 17:30 CEST — Inseritore Overview allineato per blocco

- eliminato il posizionamento globale e intermittente del comando `+`: ora il
  blocco sotto il puntatore viene risolto direttamente e il controllo segue la
  sua ultima riga visiva;
- allineati verticalmente pulsante e linea di testo usando la stessa altezza di
  riferimento da 36 px, senza la precedente traslazione CSS;
- il `+` compare soltanto vicino al blocco attivo/hover oppure mentre il menu è
  aperto, evitando che rimanga visibile in punti casuali del documento;
- al click viene creata subito una nuova riga di testo immediatamente sotto il
  blocco scelto, con focus e caret nella nuova riga; il menu trasforma quella
  stessa riga in titolo, bullet o task senza riportare il cursore altrove;
- applicato lo stesso comportamento al testo successivo ad allegati e documenti;
- aggiunta una regressione sulla posizione della nuova riga; 46 test della
  schermata, lint e build passano.

## 2026-08-26 17:40 CEST — Dimensioni immagini persistenti e percorso documenti

- aggiunto `display_width` opzionale ai blocchi file del documento cifrato,
  mantenendo compatibilità con gli allegati creati in precedenza;
- la larghezza viene ora salvata nel documento al rilascio della maniglia o
  dopo un ridimensionamento da tastiera e ripristinata a ogni reload;
- l'aggiornamento è ottimistico, ma ripristina il valore precedente e mostra un
  errore esplicito se il salvataggio E2EE non riesce;
- abilitato il percorso documenti anche nella vista Overview: entrando in un
  sotto-documento mostra radice e livelli intermedi, tutti cliccabili per
  tornare direttamente al documento desiderato;
- centralizzato il cambio documento per riallineare titolo, testo principale,
  testo dopo gli allegati, modalità di editing e menu aperti;
- aggiunti test per persistenza della larghezza e navigazione al documento
  padre; 47 test della schermata, lint e build passano.

## 2026-08-26 17:50 CEST — Sidebar Notion/Linear adattiva al tema

- ricostruita la sidebar desktop seguendo il riferimento: intestazione workspace
  minimale, accessi Membri e Agenti, sezione Spazio e profilo fissato in basso;
- spostata la creazione categoria nel comando `+` accanto a Spazio, eliminando
  il precedente pulsante testuale che alterava la gerarchia visiva;
- uniformate le icone di Generali e categorie con il glifo a livelli, aumentato
  il rientro delle categorie e applicato uno stato attivo compatto e arrotondato;
- resi trasparenti e minimali i controlli di workspace e profilo, con contatori
  e avatar allineati sul lato destro come nel riferimento;
- eliminati i colori scuri forzati: superfici, testo, icone, hover, menu e stato
  selezionato usano ora i token del tema, risultando chiari nel tema chiaro e
  scuri nel tema scuro;
- aggiornato il test del comando nuova categoria per la sua versione icon-only;
  47 test della schermata, lint e build di produzione passano.

## 2026-08-26 17:55 CEST — Sidebar fusa con lo sfondo della board

- sostituita la superficie dedicata della sidebar con la stessa superficie
  principale usata dalla board, sia nel tema chiaro sia nel tema scuro;
- uniformato anche il raccordo tra intestazione e corpo della sidebar, evitando
  una fascia di colore differente tra le due aree;
- mantenuto distinto soltanto lo stato della categoria selezionata per non
  perdere la leggibilità della navigazione; lint e build passano.

## 2026-08-26 18:00 CEST — Proporzioni compatte della sidebar

- ridotto lo spazio verticale tra il nome del progetto e la navigazione
  principale, senza spostare il contenuto della board;
- avvicinate le righe Membri e Agenti eliminando il margine da sezione che le
  faceva apparire come due gruppi separati;
- conservata una pausa visiva più ampia prima di Spazio, così la gerarchia tra
  navigazione globale e categorie rimane leggibile;
- ridotti i padding superflui tra intestazione e corpo della sidebar; lint e
  build di produzione passano.

## 2026-08-26 18:05 CEST — Allineamento toolbar e filtro icon-only

- portati selettore progetto e percorso categoria sulla stessa altezza, baseline
  e dimensione tipografica, aumentando leggermente entrambi;
- sostituito il filtro testuale «Aperti» con un pulsante quadrato contenente la
  sola icona a imbuto; stato corrente e funzione restano disponibili tramite
  etichetta accessibile e il menu mantiene tutte le opzioni;
- ingranditi contenitore e glifo layer di Generali e delle categorie, senza
  alterare rientri e allineamento delle etichette;
- aggiunta una regressione che garantisce l'assenza di testo visibile nel
  comando filtro; 47 test, lint e build di produzione passano.

## 2026-08-26 18:10 CEST — Allineamento progetto e nuove icone sidebar

- corretto il disallineamento verticale visibile nella toolbar: il selettore
  progetto ora parte dall'alto come il percorso categoria, invece di essere
  centrato rispetto all'intero blocco composto da percorso e tab;
- ridisegnata l'icona Membri come vettore adattivo al tema, seguendo la sagoma
  circolare e il profilo a busto del riferimento allegato;
- aggiunta un'icona dedicata Agenti usando il tracciato dell'icona Cursor CC0
  indicata su SVG Repo, convertito a `currentColor` per tema chiaro e scuro;
- mantenuta l'icona robot nelle viste operative dove rappresenta l'agente come
  entità, limitando il cursore alla navigazione della sidebar;
- 47 test della schermata, lint e build di produzione passano.

## 2026-08-26 18:15 CEST — Raggruppamento verticale della sidebar

- avvicinato il gruppo Membri/Agenti al selettore progetto compensando lo
  spazio occupato dalle tab della toolbar nella colonna principale;
- mantenuto invariato lo stacco prima di Spazio, così progetto, navigazione
  globale e categorie risultano tre gruppi distinti senza grandi vuoti;
- la modifica è limitata alla sidebar desktop e non altera la posizione della
  board o della navigazione mobile; lint e build di produzione passano.

## 2026-08-26 18:20 CEST — Controllo Utente rialzato e arrotondato

- aggiunto spazio sotto il controllo Utente per separarlo leggermente dal bordo
  inferiore della sidebar;
- ripristinato il contenitore a pillola con bordo sottile, colori adattivi al
  tema e stato hover coerente con gli altri controlli della navigazione;
- riequilibrati i padding interni mantenendo avatar, etichetta e chevron
  allineati; lint e build di produzione passano.

## 2026-08-26 18:25 CEST — Nuova icona filtro e controllo Utente ampliato

- sostituita l'icona filtro precedente con il tracciato vettoriale CC0 indicato
  su SVG Repo, mantenendo `currentColor` e quindi il supporto ai due temi;
- aumentati leggermente altezza, padding e avatar del controllo Utente, senza
  cambiare la sua posizione rialzata o il bordo a pillola;
- confermata la natura icon-only del filtro tramite la regressione esistente;
  47 test della schermata, lint e build di produzione passano.

## 2026-08-26 18:30 CEST — Sidebar stabile nella modalità Agenti

- individuata la sovrapposizione: la compensazione verticale prevista per la
  toolbar con tab veniva applicata anche alla vista Agenti, che ha una sola
  riga di intestazione;
- aggiunta una distanza specifica per `board-layout--agents`, impedendo a
  Membri e Agenti di risalire sopra il selettore progetto;
- mantenute invariate le proporzioni compatte delle normali viste categoria;
  47 test della schermata, lint e build di produzione passano.

## 2026-08-26 18:35 CEST — Sidebar immobile tra categorie e Agenti

- eliminato l'offset verticale specifico della modalità Agenti, responsabile
  del piccolo movimento residuo durante il cambio vista;
- resa costante l'altezza della toolbar principale anche quando le tab
  Overview/Board/Timeline/History non sono presenti;
- sidebar e contenuto mantengono così la stessa griglia e la stessa coordinata
  verticale durante la transizione; 47 test, lint e build passano.

## 2026-08-26 18:40 CEST — Riga toolbar desktop realmente invariabile

- confrontate le due catture e rilevato uno scarto residuo di circa 12 px: la
  precedente `min-height` della vista Agenti non comprendeva il padding della
  toolbar e non uguagliava l'altezza naturale della vista con tab;
- rimossa la correzione specifica per modalità e fissata direttamente a 6,1 rem
  la prima riga della griglia desktop per tutte le viste;
- la sidebar nasce ora dalla stessa coordinata strutturale, indipendentemente
  dal contenuto della toolbar, evitando spostamenti durante il cambio focus;
- 47 test della schermata, lint e build di produzione passano.

## 2026-08-26 18:45 CEST — Icone sidebar uniformi e categoria attiva evidente

- ridotte le icone layer di Generali/categorie a 16 px e uniformati i relativi
  contenitori, eliminando il peso visivo eccessivo precedente;
- portate Membri e Agenti allo stesso box da circa 15 px, mantenendo glifi
  distinti ma proporzioni e allineamento identici;
- rafforzato lo sfondo della categoria selezionata usando il token
  `--surface-active`, adattivo a tema chiaro e scuro;
- aggiunto `aria-current="page"` a Generali o alla categoria corrente e una
  regressione dedicata; 47 test, lint e build di produzione passano.

## 2026-08-26 18:50 CEST — Contrasto reale della categoria selezionata

- verificato dalla cattura che `--surface-active` risultava troppo vicino allo
  sfondo principale nel tema chiaro, pur essendo applicato correttamente;
- sostituito con una miscela al 10% del colore testo sulla superficie della
  board, ottenendo uno stato visibile e coerente anche nel tema scuro;
- aumentato a 600 il peso dell'etichetta attiva per rafforzare la selezione
  senza introdurre bordi; lint e build di produzione passano.

## 2026-08-26 18:55 CEST — Pannelli condivisi per Timeline e History

- centralizzato il colore del pannello Overview nel token locale
  `--board-view-surface`, evitando differenze tra le viste;
- aggiunto a Timeline un contenitore a tutta area con lo stesso colore e lo
  stesso raggio di Overview, mantenendo scroll e controlli interni;
- uniformati al pannello anche intestazione, righe, etichette laterali e footer
  della timeline, eliminando fasce di colore disallineate;
- espansa History sull'intera area disponibile con pannello arrotondato e scroll
  interno, coerente con Overview; 47 test, lint e build passano.

## 2026-08-26 19:00 CEST — Selezione categoria più chiara

- ridotta dal 10% al 6% la componente del colore testo miscelata nello sfondo
  della categoria attiva;
- mantenuti testo marcato e adattamento ai temi, ottenendo una selezione più
  leggera ma ancora riconoscibile; lint e build di produzione passano.

## 2026-08-26 19:05 CEST — Creazione categoria inline minimale

- spostato il comando `+` verso il margine destro, allineandolo alla fine delle
  righe categoria e del relativo sfondo di selezione;
- eliminato il precedente riquadro con bordo, pulsante Crea e pulsante Annulla;
- al click viene ora aggiunta direttamente in fondo all'elenco una riga
  categoria con icona layer, campo già focalizzato e nessuno sfondo aggiuntivo;
- la creazione si conferma con Invio o con la sola icona check, priva di bordo e
  sfondo; `Esc` annulla e ripulisce la riga;
- nascosto lo stato «Nessuna categoria» mentre è aperta la nuova riga e aggiornata
  la regressione per verificare l'assenza dei vecchi controlli; 47 test, lint e
  build di produzione passano.

## 2026-08-26 19:10 CEST — Altezza viste allineata alle task list

- individuato il padding duplicato di Overview, che accorciava il pannello
  rispetto alle colonne della Board;
- centralizzato in `--board-view-bottom-gap` lo spazio inferiore da 0,35 rem già
  usato dal contenitore delle task list;
- applicata la stessa quota visibile a Overview, Timeline e History, così bordo
  superiore e inferiore coincidono con quelli delle task list;
- eliminata anche la seconda spaziatura destra di Overview, lasciando al
  contenitore Board la gestione uniforme dei margini; 47 test, lint e build
  passano.

## 2026-08-26 19:15 CEST — Bande Timeline estese a tutta la board

- aumentato il contrasto delle fasce alternate dal precedente effetto quasi
  invisibile al 4% per i giorni alterni e all'8% per i weekend;
- introdotto un unico layer temporale assoluto che copre verticalmente tutta
  l'area Gantt, inclusa la porzione vuota sotto l'ultima task list;
- rese trasparenti le lane per mostrare il layer continuo ed eliminate le bande
  duplicate riga per riga;
- estesa anche la linea del giorno corrente fino al fondo della board, lasciando
  task e controlli sopra il nuovo sfondo;
- aggiunta una regressione sulla presenza del layer globale; 47 test, lint e
  build di produzione passano.

## 2026-08-26 19:20 CEST — Timeline chiara con colonna utenti continua

- ridotto nettamente il contrasto richiesto dopo il controllo visivo: giorni
  alterni all'1,5% e weekend al 3% del colore testo;
- aggiunta una banda verticale fissa larga quanto la colonna delle task list,
  colorata con lo stesso sfondo della board e continua fino al footer;
- regolati gli stacking context: etichette e avatar restano sopra la banda,
  mentre fasce temporali e task che scorrono orizzontalmente rimangono dietro;
- estesa la regressione alla presenza del gutter sinistro; 47 test, lint e build
  di produzione passano.

## 2026-08-26 19:25 CEST — Superfici board più chiare e ombra leggera

- sostituito il grigio fisso delle viste con una superficie molto più chiara,
  derivata dal tema per restare coerente anche in modalità scura;
- uniformato lo stesso colore tra colonne Board, Overview, Timeline e History;
- aggiunta una doppia ombra appena percettibile ai pannelli, sufficiente a
  separarli dallo sfondo senza reintrodurre bordi marcati;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:40 CEST — Ombra board più sfumata

- ampliato il raggio dei due livelli d'ombra e ridotta la loro opacità vicino
  ai pannelli, ottenendo una transizione più morbida verso lo sfondo;
- portata l'area di rispetto a 12 px per conservare integralmente la sfumatura.

## 2026-08-26 19:45 CEST — Navigazione viste dentro le board

- lasciato il percorso categoria nella toolbar e spostati i tab Overview, Board,
  Timeline e History dentro la superficie di ciascuna vista;
- aggiunto un contenitore comune anche alla vista Board per ospitare i tab senza
  alterare la posizione della sidebar o del breadcrumb;
- resa ulteriormente più chiara l'ombra, riducendo l'opacità dei due livelli;
- aggiornata la regressione UI per verificare la nuova posizione dei tab.

## 2026-08-26 19:50 CEST — Board estesa nello spazio liberato

- recuperato il precedente gap verticale tra toolbar e contenuto dopo lo
  spostamento dei tab dentro la board;
- fatta risalire esclusivamente l'area board, lasciando ferme sidebar,
  breadcrumb, selettore progetto, filtro e ricerca;
- escluse dal riallineamento le modalità Agenti e dettaglio task list, che hanno
  una struttura indipendente;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:55 CEST — Tab fissi durante lo scorrimento

- separata in Overview l'intestazione dei tab dal contenitore scrollabile del
  documento Markdown;
- applicata la stessa struttura a History, facendo scorrere solo intestazione e
  task completati sotto la navigazione fissa;
- confermato il comportamento già corretto di Board e Timeline, dove i tab sono
  esterni alle rispettive aree di scorrimento;
- estesa la regressione UI alla nuova area scrollabile di Overview;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:00 CEST — Tab ripristinati e board sotto la ricerca

- ripristinata la posizione originaria dei tab Overview, Board, Timeline e
  History nella toolbar, rendendoli naturalmente fissi durante lo scorrimento;
- estesa la superficie della board verso l'alto fino alla zona immediatamente
  sotto filtro e ricerca, recuperando lo spazio dietro la riga dei tab;
- eliminato il padding laterale e inferiore dai contenuti di Overview, Board e
  History, mantenendo esclusivamente una distanza superiore compatta;
- mantenute separate le aree scrollabili di documento e storico;
- rimossi gli stili intermedi dei tab interni ormai inutilizzati;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:05 CEST — Eliminata sovrapposizione dei tab

- rilevata dalle schermate la sovrapposizione della navigazione con titolo
  Overview, intestazioni delle task list e asse Timeline;
- introdotta un'unica quota superiore condivisa, riservata ai tab mentre la
  superficie della board continua a estendersi dietro di essi;
- mantenuti a zero i padding laterale e inferiore richiesti;
- portata la toolbar sopra la superficie tramite uno stacking esplicito;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:10 CEST — Tab distanziati da bordo e sidebar

- spostata la navigazione delle viste di 12 px verso destra per separarla
  visivamente dalla sidebar;
- abbassati i tab di circa 6 px rispetto al bordo superiore della board;
- aumentata in modo coordinato la sola quota superiore riservata ai contenuti,
  evitando sovrapposizioni senza aggiungere padding laterale o inferiore;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:15 CEST — Spaziatura tab rifinita

- aumentato da 12 a 24 px il distacco della navigazione dalla sidebar;
- ridotto il padding inferiore delle etichette per avvicinare la linea attiva ai
  nomi Overview, Board, Timeline e History senza modificare il pannello;
- lint, build di produzione e controllo diff passano.

## 2026-08-26 20:20 CEST — Toolbar allineata alla board

- condiviso l'inset esterno della board anche con la toolbar;
- allineato il bordo sinistro del breadcrumb Generali all'inizio del pannello e
  il bordo destro della ricerca alla sua fine;
- abbassata di altri 4 px la navigazione Overview, Board, Timeline e History;
- aumentata della stessa quota la riserva superiore dei contenuti;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:25 CEST — Rimosso pannello esterno dalla vista Board

- eliminati superficie, bordo, ombra, arrotondamento e margine dal contenitore
  generale della sola modalità Board;
- mantenute come elementi visivi autonomi esclusivamente le task list e la
  colonna per crearne una nuova;
- lasciati invariati i pannelli di Overview, Timeline e History;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:30 CEST — Ricerca e controllo utente compattati

- ridotta a 36 px l'altezza della barra di ricerca desktop, insieme a campo e
  tipografia interna;
- portato a circa 37 px il controllo utente e ridotto l'avatar a 27 px;
- eliminato l'ultimo margine inferiore della sidebar desktop per allineare il
  fondo del controllo utente al bordo inferiore delle task list;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:35 CEST — Main e sidebar bianchi

- portate a bianco puro le superfici principali e della sidebar nel tema
  chiaro;
- mantenuti invariati i token del tema scuro;
- conservata la separazione delle board tramite il loro leggero grigio, bordo e
  ombra;
- lint, build di produzione e controllo diff passano.

## 2026-08-26 20:40 CEST — Board leggermente più scure

- aumentata dal 1,5% al 2,5% la componente del colore testo nella superficie
  condivisa delle board;
- mantenuti bianchi main e sidebar e conservato l'adattamento al tema scuro;
- lint, build di produzione e controllo diff passano.

## 2026-08-26 20:45 CEST — Nuove icone uniformi della sidebar

- integrate localmente le icone Noun Project indicate per Membri, Agenti e
  apertura/chiusura sidebar;
- applicate come maschere monocromatiche per adattarne il colore al tema;
- aumentata l'icona layer delle categorie e uniformate tutte le icone principali
  della sidebar a 1,15 rem;
- aggiunto il documento con fonti, autori e riferimenti di licenza;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:30 CEST — Ombra board resa visibile

- aumentata l'intensità dell'ombra condivisa da Board, Overview, Timeline e
  History, mantenendo due livelli morbidi e senza aggiungere bordi;
- il distacco dallo sfondo è ora percepibile anche sulle superfici quasi bianche;
- lint, build di produzione e controllo diff passano.

## 2026-08-26 19:35 CEST — Ombre non tagliate e bordo minimale

- riservato uno spazio uniforme attorno a colonne Board, Overview, Timeline e
  History affinché la sfocatura dell'ombra non venga più troncata dai container;
- compattata l'ombra entro la nuova area di rispetto, mantenendola morbida;
- aggiunto a tutte le board un bordo tematico al 7%, appena visibile sulle
  superfici chiare e coerente anche in modalità scura;
- mantenuto lo stesso inset tra tutte le viste per conservarne l'allineamento;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:00 CEST — Estetica sidebar derivata dal frontend allegato

- confrontato il frontend corrente con
  `/Users/antoniodeluca/Projects/sprout/frontend/sprout-web` e trasferiti i
  relativi token di larghezza, distanza dal contenuto, superficie e raggi;
- sostituita la composizione visiva spezzata tra toolbar e navigazione con la
  superficie continua usata dalla sidebar di riferimento;
- raggruppati Membri e Agenti nella navigazione primaria compatta della sorgente,
  preservando avatar, conteggi, stati attivi e azioni esistenti;
- uniformate righe, icone, tipografia, categorie e controllo utente alle
  proporzioni del codice allegato senza importarne logica o dati;
- 47 test UI, lint, build di produzione e controllo diff passano;
- il controllo browser raggiunge l'app, ma la sessione disponibile è mobile e
  bloccata dalle chiavi di decifratura mancanti, quindi non è stata usata come
  conferma visiva della variante desktop.

## 2026-08-26 19:15 CEST — Struttura sidebar sostituita con quella allegata

- corretto il primo porting, che aveva mantenuto troppo markup della sidebar
  precedente e trasferito soltanto una parte delle proporzioni;
- adottata la struttura `board-sidebar-primary-nav` / `board-nav-section--views`
  del frontend allegato per le righe Membri e Agenti, inclusi avatar sovrapposti
  e contatore;
- sostituiti i contenitori-avatar delle categorie con le icone file dirette
  usate dalla sorgente e portati gli stessi stati attivo, preferito e locked;
- riallineati al codice sorgente switcher progetto, superfici, tipografia,
  controllo utente e selezioni, conservando i callback e i dati del frontend
  corrente;
- preservati come comandi accessibili non visibili la selezione diretta dei
  membri e l'overflow, così il nuovo aspetto non elimina le funzioni esistenti;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:20 CEST — Header sidebar semplificato

- rimossa l'icona cartella accanto al nome del progetto;
- aumentata a 1,35 rem esclusivamente l'icona di apertura/chiusura della
  sidebar, mantenendo invariate le dimensioni delle altre icone;
- lint, build di produzione e controllo diff passano.

## 2026-08-26 19:30 CEST — Board, tab e Ask to AI copiati dalla sorgente

- spostati definitivamente Overview, Board, Timeline e History fuori dalla
  superficie della board, nella toolbar superiore come nel frontend allegato;
- adottati altezza, margini, raggio e colore del contenitore
  `board-secondary-view-panel` per Overview, Timeline e History;
- rimossi dalle tre viste i pannelli interni duplicati e riportato Overview alla
  larghezza contenuto di 56 rem prevista dalla sorgente;
- copiate spaziatura da 1,35 rem e geometria delle colonne della vista Board;
- aggiunto nel footer il pulsante `Ask to AI`, collegato all'apertura della
  sezione Agenti e dimensionato come il controllo agente della sorgente;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:40 CEST — Percorso e Ask to AI allineati

- spostato `Ask to AI` dal footer inferiore alla toolbar superiore, sulla stessa
  riga verticale del percorso file;
- eliminato il footer ormai vuoto, recuperando 1,75 rem di altezza utile per le
  board;
- ridotta da 6,1 a 5,7 rem la fascia toolbar desktop e compattato lo spazio tra
  percorso e tab;
- alzati Overview, Board, Timeline e History e aumentata di conseguenza
  l'altezza disponibile per tutte le viste;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:45 CEST — Ripristinato layout precedente

- annullato su richiesta l'allineamento tra percorso file e `Ask to AI`;
- riportato il percorso nella toolbar superiore e `Ask to AI` nel footer;
- ripristinate la fascia toolbar desktop a 6,1 rem e la distanza di 0,5 rem tra
  percorso e tab;
- rimossa la variante temporanea che consentiva di nascondere il breadcrumb;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:45 CEST — Percorso e Ask to AI spostati sotto la board

- corretta l'interpretazione precedente: percorso categoria e `Ask to AI` sono
  ora entrambi nel footer sotto la board, rispettivamente a sinistra e destra;
- lasciati Overview, Board, Timeline e History da soli nella toolbar superiore;
- ridotta la fascia superiore desktop da 5,7 a 3,6 rem per mantenere i tab in
  alto e compensare il footer, conservando una board leggermente più alta;
- mantenuta la navigazione completa del percorso anche nella nuova posizione;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:34 CEST — Timeline riallineata alla superficie della board

- eliminata nel layout desktop la fascia superiore vuota da 3,35 rem rimasta
  dalla precedente posizione interna dei tab;
- spostata di conseguenza l'intestazione con i giorni sul bordo alto utile della
  timeline;
- uniformati intestazione, banda delle task list, griglia e footer al colore
  della board sia in tema chiaro sia in tema scuro;
- ridotto il contrasto delle bande alternate e dei weekend, mantenendo una
  differenza minima sufficiente a leggere le colonne;
- 47 test UI, lint e build di produzione passano; il primo tentativo dei test
  con Node 25 è stato scartato perché il runtime disabilitava `localStorage`,
  mentre la suite passa integralmente con il Node 24 previsto dal workspace.

## 2026-08-26 19:42 CEST — Filtro e ricerca allineati ai tab

- spostato il gruppo filtro/ricerca sul fondo della toolbar desktop, separandolo
  dall'asse verticale del percorso categoria;
- uniformata a 2 rem l'altezza del filtro e della ricerca, coincidente con la
  riga `Overview / Board / Timeline / History`;
- mantenuti invariati layout mobile e altezza disponibile delle board;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:43 CEST — Percorso e tab avvicinati

- ridotto da 0,5 rem a 0,2 rem lo spazio verticale desktop tra percorso file e
  `Overview / Board / Timeline / History`;
- alzati dello stesso delta filtro e ricerca per conservarne l'allineamento con
  la riga dei tab;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:44 CEST — Spazio recuperato a favore delle board

- ridotta da 6,1 rem a 5,8 rem l'altezza della fascia toolbar desktop;
- assegnati alle board i 0,3 rem recuperati avvicinando percorso e tab;
- rimossa la compensazione temporanea del gruppo filtro/ricerca, ora allineato
  naturalmente ai tab nella fascia più compatta;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:45 CEST — Bordi dei controlli sidebar

- ripristinato sul selettore progetto desktop il bordo leggero basato sul token
  `border-sidebar-control`;
- applicato lo stesso bordo al controllo utente in fondo alla sidebar;
- conservati raggi, superfici, hover e adattamento automatico al tema;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:47 CEST — Controlli sidebar allineati ai bordi della board

- corretto da 1,6 rem a 2,1 rem l'offset inferiore desktop della sidebar,
  includendo l'intera altezza del footer sotto la board;
- allineato così il bordo inferiore del tasto utente al bordo inferiore della
  board;
- introdotto un inset superiore desktop condiviso da toolbar sidebar e toolbar
  principale, usato da pulsante sidebar, selettore progetto e percorso file;
- esplicitato il `border-box` sui tre controlli per impedire che i nuovi bordi
  alterino le rispettive coordinate esterne;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:48 CEST — Progetto allineato al percorso visibile

- corretta la precedente misura, che allineava il tasto progetto al contenitore
  invisibile del breadcrumb invece che alla parte visibile di `Generali`;
- abbassati di 1,25 rem sia il selettore progetto sia il pulsante sidebar,
  mantenendoli allineati tra loro;
- lasciati invariati percorso, tab e altezza delle board;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:49 CEST — Offset corretto e sidebar leggermente ingrandita

- ridotto da 1,25 rem a 0,45 rem l'abbassamento di selettore progetto e
  pulsante sidebar, eliminando il taglio causato dalla sovrapposizione con la
  seconda riga del layout;
- aumentata a 1 rem la tipografia di progetto, viste e categorie;
- aumentate leggermente altezza e spaziatura delle righe Membri, Agenti e
  categorie;
- portate a 1,1 rem le icone delle viste e a 1,25 rem le icone categoria;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:50 CEST — Taglio toolbar eliminato e pulsante sidebar ampliato

- mantenuto invariato l'offset verticale approvato di 0,45 rem;
- portata la toolbar sidebar sopra la navigazione tramite livello esplicito e
  overflow visibile, eliminando il taglio residuo dei controlli superiori;
- aumentato il pulsante sidebar da 2,5 rem a 2,75 rem e la relativa icona da
  1,35 rem a 1,5 rem;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:51 CEST — Prima voce sidebar nuovamente visibile

- rimosso il livello superiore dall'intero contenitore toolbar, il cui sfondo
  copriva la parte alta della voce `Membri`;
- applicata la priorità visiva esclusivamente alla riga che contiene pulsante
  sidebar e selettore progetto;
- conservati dimensione e posizionamento approvati dei controlli superiori;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:53 CEST — Assi interni della sidebar uniformati

- spostate di 0,375 rem verso sinistra le sole icone Membri e Agenti per
  allinearle all'icona del pulsante sidebar;
- spostati di 0,6875 rem verso destra avatar membri e contatore agenti per
  allinearli alla freccia del selettore progetto;
- eliminato il padding destro del contenitore utente, estendendo il relativo
  pulsante fino al margine utile della sidebar;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:54 CEST — Posizione superiore ripristinata

- azzerato l'offset verticale di 0,45 rem introdotto sui controlli superiori;
- riportati selettore progetto e pulsante sidebar sull'asse precedente di
  `Generali`;
- aumentata di conseguenza la distanza visiva tra il selettore progetto e le
  voci Membri/Agenti, senza spostare la navigazione;
- conservati gli allineamenti orizzontali di icone, indicatori e tasto utente;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:55 CEST — Blocco Membri e Agenti più distanziato

- aggiunto un margine superiore di 0,5 rem al gruppo Membri/Agenti per
  separarlo maggiormente dal selettore progetto;
- aumentato da 1,125 rem a 1,5 rem lo spazio tra il gruppo Membri/Agenti e la
  sezione `SPAZIO`;
- mantenuti invariati dimensioni delle righe e allineamenti orizzontali;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:56 CEST — Tasto utente esteso

- rimosso il padding sinistro residuo dal contenitore utente desktop;
- esteso il tasto utente a tutta la larghezza utile della sidebar, mantenendo
  invariati altezza, bordo e allineamento inferiore con la board;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:57 CEST — Altezza controlli superiori uniformata

- assegnata esplicitamente al selettore progetto l'altezza condivisa di 2,5
  rem;
- aumentato il filtro desktop da 2 rem a 2,5 rem;
- normalizzato il pulsante sidebar allo stesso quadrato di 2,5 rem, conservando
  l'icona interna da 1,5 rem;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 19:58 CEST — Altezza ricerca corretta

- corretta l'omissione della modifica precedente portando anche la barra di
  ricerca desktop da 2 rem a 2,5 rem;
- filtro, ricerca, selettore progetto e pulsante sidebar condividono ora la
  stessa altezza effettiva;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:00 CEST — Pallino Agenti sincronizzato al colore dell'agente

- estratta in `domain/agents.ts` la risoluzione condivisa dello stato visivo
  Working, Done e Rest;
- applicato al pallino Agenti della sidebar lo stesso verde, giallo o grigio
  usato dall'agente nella relativa vista;
- quando è selezionato un agente, il pallino segue il suo stato; in assenza di
  agenti reali mantiene il verde dell'agente demo Atlas;
- 50 test UI mirati, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:01 CEST — Azione categoria allineata al progetto

- spostato orizzontalmente di 0,4 rem il pulsante `+` della sezione `SPAZIO`;
- allineato il suo centro alla freccia del selettore progetto, senza modificare
  la geometria delle categorie;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:06 CEST — Tipografia sidebar alleggerita

- ridotto da 600 a 575 il peso del selettore progetto e delle voci attive;
- ridotto da 500 a 475 il peso delle voci Membri, Agenti e categorie non attive;
- mantenuto invariato il testo utente, già impostato su un peso più leggero;
- 47 test UI, lint, build di produzione e controllo diff passano.

## 2026-08-26 20:10 CEST — Navigazione sidebar semplificata

- rimossi avatar, contatori e pallini di stato dalle righe `Membri` e `Agenti`;
- mantenuti i collegamenti accessibili ai singoli membri, senza indicatori visivi
  aggiuntivi;
- anticipata verticalmente la sezione Membri/Agenti per allinearla alla riga
  `Overview` della navigazione principale.

## 2026-08-26 20:14 CEST — Selettore progetto minimale

- eliminati bordo, fondo, sottolineatura e freccia dal selettore progetto;
- resa visibile l'icona cartella e allineata alla colonna delle icone di
  Membri e Agenti;
- sostituita l'icona pannello della sidebar con una freccia laterale dedicata,
  che cambia direzione quando la sidebar è aperta o chiusa.

## 2026-08-26 20:17 CEST — Griglia progetto riallineata

- spostate icona cartella e nome progetto sulle stesse due colonne visive di
  icona e testo delle righe Membri e Agenti;
- preservata la freccia laterale di controllo nella posizione di chiusura
  della sidebar.

## 2026-08-26 20:20 CEST — Progetto come voce di navigazione

- allineato il selettore progetto alle dimensioni, al peso e alla spaziatura
  delle righe Membri e Agenti;
- eliminata la percezione di intestazione separata, conservando il menu di
  scelta progetto e il controllo laterale della sidebar.

## 2026-08-26 20:24 CEST — Assi sidebar e ritaglio Membri corretti

- introdotta una griglia orizzontale condivisa fra Progetto, Membri e Agenti:
  icone e testi ora occupano esattamente le stesse colonne;
- uniformati raggio e stato hover del progetto alle normali voci di
  navigazione;
- distanziata leggermente la prima riga dall'area di sovrapposizione superiore,
  evitando che il separatore ritagli l'icona Membri.

## 2026-08-26 20:23 CEST — Colonne e sovrapposizione sidebar corrette

- allineate puntualmente icona e testo progetto alle colonne delle rispettive
  icone e testi di Membri e Agenti;
- sollevato il gruppo di navigazione sopra la separazione del toolbar, evitando
  che la linea attraversi l'icona Membri.

## 2026-08-26 20:30 CEST — Backend e frontend riavviati

- riavviato `sprout-server` su `127.0.0.1:18085` usando il database di sviluppo
  persistente `sprout2_dev`;
- riavviato Vite su `127.0.0.1:4176` con proxy API esplicito verso il backend;
- verificati health `live` e `ready`, risposta frontend e instradamento
  `/v1/projects` al backend (`401` atteso senza sessione, non `404/502`).

## 2026-08-26 20:38 CEST — Ritmo verticale e azioni sidebar allineati

- applicata fra Progetto e Membri la stessa distanza verticale presente fra
  Membri e Agenti;
- spostata la freccia di apertura/chiusura sullo stesso asse orizzontale del
  pulsante `+` della sezione SPAZIO.

## 2026-08-26 21:10 CEST — Pulsante aggiunta editor stabile al passaggio del cursore

- ritardata di 140 ms la scomparsa del pulsante `+` dell'editor Overview;
- annullata la scomparsa quando il cursore raggiunge il pulsante o il suo menu,
  evitando che il controllo sparisca prima del click;
- applicato lo stesso comportamento anche all'inseritore dopo gli allegati.

## 2026-08-26 21:15 CEST — Menu editor applicato alla riga selezionata

- il pulsante `+` non aggiunge più preventivamente una riga vuota sotto il
  blocco selezionato;
- titoli, elenchi e task trasformano ora direttamente la riga a cui è
  affiancato il controllo, mantenendone il testo;
- reso visibile l'overflow del canvas per evitare ritagli del pulsante e del
  suo menu.

## 2026-08-26 21:20 CEST — Menu slash per l'editor Overview

- rimosso il pulsante `+` e tutta la logica hover associata;
- premendo `/` su una riga vuota appare il menu per titoli, elenchi, task,
  immagini, file e documenti;
- inserito un gutter interno al canvas: il menu resta nel suo contenitore e non
  può essere ritagliato dallo sfondo della board.
- verificati test Overview (47/47), lint, build e health di frontend/backend.

## 2026-08-27 — Bordo e ombra delle board

- reso appena più definito il bordo delle board Overview, Timeline, History e
  delle task list;
- applicata un'ombra corta e molto leggera, coerente anche con tema scuro.

## 2026-08-27 — Contrasto board corretto

- aumentato il contrasto del bordo e dell'ombra, prima impercettibili;
- rimosso il clipping del contenitore Overview, così l'ombra può essere visibile
  anche lungo i bordi esterni.

## 2026-08-27 — Bordo board coerente con sidebar

- le board usano ora esattamente `--border-sidebar-control`, lo stesso bordo
  del tasto Utente.

## 2026-08-27 — Override delle board secondarie corretto

- ripristinati bordo e ombra sulle board usate realmente dal layout desktop:
  un precedente override le azzerava per Overview, Timeline e History;
- il bordo è ora direttamente `1px solid var(--border-sidebar-control)`, come
  il tasto Utente.

## 2026-08-27 — Bordo spostato alla board Overview

- rimosso bordo e ombra dal documento interno bianco;
- applicati al pannello grigio esterno `.board-secondary-view-panel`, che è la
  board Overview mostrata nella schermata.

## 2026-08-27 — Board estesa sotto i controlli

- ripristinata la posizione invariata di tab, filtro e ricerca;
- estesa la board verso l'alto, dietro ai controlli, riservando internamente lo
  stesso spazio così il contenuto dell'Overview non si sposta.

## 2026-08-27 — Rifinitura percorso e tab

- alzato leggermente il percorso della categoria;
- eliminato lo sfondo del tab attivo: restano testo e sottolineatura, come gli
  altri controlli della navigazione.

## 2026-08-27 — Fondo board unificato

- alzato ulteriormente il percorso della categoria;
- schiarito il fondo della board e applicato anche alla vista Board dietro le
  task list, con equivalente nel tema scuro.

## 2026-08-27 — Correzione vista Board

- ripristinato il padding interno della vista Board: task list e area
  scorrevole non partono più a filo del pannello né risultano tagliate.

## 2026-08-27 — Pannello condiviso per la vista Board

- la vista Board usa ora lo stesso pannello esterno dell'Overview;
- il pannello si estende dietro tab, filtro e ricerca mantenendo i controlli
  fermi, con le colonne contenute e scorrevoli nel suo spazio interno.

## 2026-08-27 — Allineamento percorso e tab

- spostati leggermente a destra insieme il percorso della categoria e i tab
  Overview, Board, Timeline e History, mantenendoli allineati tra loro.

## 2026-08-27 — Fondo board più chiaro

- schiarito in modo netto il pannello della board nel tema chiaro e nel tema
  scuro, senza modificare bordo e ombra.

## 2026-08-27 — Contrasto sfondo e sidebar

- scuriti sfondo principale e sidebar nel tema scuro; la board resta più chiara
  e quindi più leggibile come area di lavoro separata.

## 2026-08-27 — Contrasto anche nel tema chiaro

- resi leggermente più scuri sfondo principale e sidebar nel tema chiaro;
- la board resta quasi bianca per mantenere la separazione visiva.

## 2026-08-27 — Colori tema chiaro/scuro ricalibrati

- ripristinati i colori originari di sfondo, sidebar e board nel tema scuro;
- scuriti ulteriormente solo sfondo e sidebar nel tema chiaro, conservando una
  board più chiara.

## 2026-08-27 — Tasto Utente coerente con la board

- introdotto il token condiviso `--surface-board`;
- il tasto Utente della sidebar usa ora lo stesso colore della board in tema
  chiaro e scuro.

## 2026-08-27 — Override responsive tasto Utente

- corretta la regola responsive che sovrascriveva il colore del tasto Utente;
- anche nel layout desktop il tasto usa ora effettivamente `--surface-board`.

## 2026-08-27 — Ombra tasto Utente

- aggiunta al tasto Utente una lieve ombra `--shadow-card`, coerente con il
  trattamento delle board.

## 2026-08-27 — Tasto Progetto allineato alla board

- aggiunti al tasto Progetto fondo `--surface-board`, bordo della sidebar e
  ombra leggera;
- corretta anche la regola desktop che in precedenza rimuoveva bordo e colore.

## 2026-08-27 — Fondo controlli sidebar ricalibrato

- sostituito il bianco quasi puro con il grigio chiaro effettivo della board;
- Progetto e Utente restano coerenti tra loro, ma non sembrano più pillole
  bianche staccate dalla sidebar.

## 2026-08-27 — Correzione controlli sidebar

- ripristinato il fondo precedente della board, senza ulteriori modifiche al
  colore dei controlli;
- resa visibile l'ombra esclusivamente sul tasto Utente;
- aumentata a `2.5rem` l'altezza del solo tasto Progetto: il controllo di
  chiusura sidebar resta separato e invariato.

## 2026-08-27 — Ombra Utente resa percepibile

- aumentata la definizione dell'ombra del solo tasto Utente con due livelli
  morbidi, mantenendo un risultato leggero ma visibile.

## 2026-08-27 — Ombra Utente non ritagliata

- reso visibile l'overflow della sidebar espansa così l'ombra del tasto Utente
  può uscire dal suo contenitore;
- mantenuto `overflow: hidden` solo quando la sidebar è collassata.

## 2026-08-27 — Ombra Utente uguale alla board

- applicati al tasto Utente gli stessi due livelli e valori dell'ombra della
  board.

## 2026-08-27 — Ripristino layout board precedente

- ripristinata la struttura precedente alla richiesta di estendere la board
  sotto tab, filtro e ricerca;
- la vista Board torna al suo contenitore originale e le board secondarie non
  riservano più spazio sopra al contenuto.

## 2026-08-27 — Bordo tasto Progetto

- reso più visibile il bordo del tasto Progetto, allineandone la definizione
  visiva a quella del tasto Utente.

## 2026-08-27 — Larghezza tasto Progetto

- esteso il tasto Progetto alla stessa larghezza utile del tasto Utente;
- mantenuto l'allineamento interno di icona e testo e lasciato separato il
  controllo di chiusura della sidebar.

## 2026-08-27 — Rimozione percorso e recupero spazio board

- rimosso il percorso file dalla toolbar delle categorie;
- rialzati tab di vista e area board sfruttando lo spazio liberato, senza
  spostare filtro e ricerca dentro al pannello.

## 2026-08-27 — Toolbar e sidebar categorie

- corretto il riallineamento della sidebar dopo la riduzione della toolbar,
  così la voce Membri resta sempre visibile;
- rimossi i bordi del filtro e della ricerca desktop, lasciando sulla ricerca
  solo una linea inferiore discreta.

## 2026-08-27 — Superficie board chiara

- schiarite sensibilmente le superfici della board nel tema chiaro;
- resi bianchi e coerenti i tasti Progetto e Utente, che condividono la
  superficie board.

## 2026-08-27 — Ombre ridotte

- ridotta l'intensità e la diffusione dell'ombra su board e tasto Utente,
  mantenendone una separazione visiva molto leggera.

## 2026-08-27 — Ricerca allineata ai tab

- resi testo, icona e linea inferiore della ricerca dello stesso colore del
  tab Overview attivo.

## 2026-08-27 — Ricerca colore categorie

- ammorbidito il colore di testo, icona e linea della ricerca usando il grigio
  delle intestazioni categoria della sidebar.

## 2026-08-27 — Linea ricerca alleggerita

- resa più chiara la sola linea inferiore della ricerca, conservando il colore
  delle categorie per testo e icona.

## 2026-08-27 — Filtro colore categorie

- allineato il colore dell'icona filtro al grigio tenue di ricerca e categorie,
  mantenendo il controllo privo di bordo.

## 2026-08-27 — Progetto senza pillola

- rimosso bordo e sfondo dal tasto Progetto desktop;
- aggiunta una linea inferiore chiara, coerente con quella della ricerca.

## 2026-08-27 — Etichetta Progetto semplificata

- rimossa l'icona cartella dal controllo Progetto desktop;
- aumentati leggermente dimensione e peso del nome progetto, mantenendo la
  freccia laterale.

## 2026-08-27 — Progetto come intestazione

- rimossa la linea inferiore del controllo Progetto;
- reso il nome un'intestazione più netta e spostata la freccia subito dopo il
  testo, come nel riferimento fornito.

## 2026-08-27 — Spazio sidebar-board

- ridotto il margine orizzontale tra sidebar e board nella vista desktop.

## 2026-08-27 — Spazio sidebar-board ridotto

- ridotto ulteriormente il distacco fra sidebar e board a `0.4rem`.

## 2026-08-27 — Tipografia Geist

- aggiunto il font variabile Geist nel bundle frontend;
- impostato Geist come font primario dell'interfaccia, con fallback Helvetica e
  di sistema per mantenere affidabilità di rendering.

## 2026-08-27 — Board rialzata

- rialzata leggermente l'area board desktop rispetto ai tab, senza spostare la
  sidebar.

## 2026-08-27 — Board rialzata ulteriormente

- aumentato di un ulteriore `0.25rem` il rialzo dell'area board desktop.

## 2026-08-27 — Ricerca uguale a Overview

- allineati colore e spessore della linea della ricerca al tab Overview attivo;
- impostati testo e icona della ricerca sullo stesso colore di Overview.

## 2026-08-27 — Ricerca allineata ai tab

- allineati verticalmente testo e linea della ricerca alla base del tab
  Overview; mantenuto il precedente grigio tenue della ricerca.

## 2026-08-27 — Tab attivo e altezza tasklist

- sostituita la sottolineatura del tab attivo con lo sfondo tenue usato per le
  categorie selezionate nella sidebar;
- resa esplicita l'altezza piena delle colonne tasklist per allinearle al bordo
  inferiore delle viste Overview, Timeline e History.

## 2026-08-27 — Ombra tasklist

- applicata alle colonne tasklist la stessa ombra molto leggera della board
  Overview, senza aggiungere un bordo visibile.

## 2026-08-27 — Selezione tab e ombra tasklist

- ridotta l'altezza del fondo sui tab di vista selezionati;
- resa percepibile ma morbida l'ombra sotto le colonne tasklist.

## 2026-08-27 — Tipografia tab

- aumentata la dimensione delle etichette Overview, Board, Timeline e History,
  mantenendo invariata l'altezza compatta dello sfondo selezionato.

## 2026-08-27 — Spaziatura tab

- aumentata leggermente la distanza tra i tab;
- aumentata di pochi pixel l'altezza del loro sfondo selezionato.

## 2026-08-27 — Sfondo tab aumentato

- aumentata ulteriormente e in modo minimo l'altezza dello sfondo del tab
  selezionato.

## 2026-08-27 — Ricerca come tab selezionato

- rimossa la linea inferiore della ricerca;
- applicati alla ricerca altezza, angoli e sfondo del tab selezionato.

## 2026-08-27 — Altezza ricerca

- aumentata leggermente l'altezza dello sfondo della ricerca.

## 2026-08-27 — Ricerca riallineata alla board

- aumentata ulteriormente la ricerca e avvicinata leggermente all'area board,
  senza modificare la posizione dei tab.

## 2026-08-27 — Overview Membri

- creata una panoramica Membri con la stessa board minimale degli Agenti,
  includendo avatar circolari e il controllo `New` per invitare un membro;
- mantenute per Membri le viste Overview, Board, Timeline e History;
- semplificate le colonne della Board Membri: niente avatar né controlli di
  creazione all'interno delle colonne.

## 2026-08-27 — Avatar Membri

- portati gli avatar della panoramica Membri alla stessa dimensione del
  controllo `New`;
- ripristinati gli avatar nell'intestazione delle colonne della Board Membri,
  senza ripristinare i controlli di aggiunta duplicati.

## 2026-08-27 — Tactical: controlli squadrati

- nella modalità Tactical resi spigolosi l'input AI, la barra di ricerca e lo
  sfondo del tab di vista selezionato.
- corretta la precedenza delle regole desktop: ricerca e tab mantengono ora
  effettivamente gli angoli a spigolo vivo anche su schermi larghi.
