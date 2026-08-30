# Sistema dei permessi di Sprout

## Scopo e fonte di verità

Questo documento descrive il sistema di autorizzazione **attualmente
implementato** nel backend Sprout. Copre identità, dispositivi, ruoli di
progetto, gerarchia delle risorse, topic, task list, task, Info, allegati,
assegnazioni, preset, questionari, sync, retention, recovery E2EE e accessi di
servizio.

Le fonti di verità eseguibili restano:

- `apps/server/src/auth.rs` per le decisioni applicative comuni;
- `apps/server/src/routes/` per le autorizzazioni delle singole operazioni;
- `db/migrations/0009_row_level_security.sql` e le migrazioni successive per
  RLS e isolamento PostgreSQL;
- `db/migrations/0011_hierarchical_authorization.sql` per propagazione ed
  effettività dei grant;
- [ADR-0003](adr/0003-recursive-permissions.md) e
  [ADR-0004](adr/0004-per-resource-keys-recovery-unanimity.md) per le decisioni
  architetturali.

Il permesso server e la capacità di decifrare sono due condizioni distinte:
un utente deve superare l'autorizzazione applicativa/RLS **e** possedere sul
proprio dispositivo gli envelope delle chiavi corretti.

## Livelli di controllo

Ogni richiesta protetta attraversa questi livelli:

1. **Autenticazione della sessione.** Il bearer token identifica identità,
   sessione e dispositivo. La sessione deve esistere, non essere revocata e non
   essere scaduta.
2. **Membership attiva.** Per operare in un progetto, la membership deve avere
   `state = active`. Le membership `suspended` o `left` non autorizzano.
3. **Ruolo di progetto.** `owner` e `admin` hanno capacità amministrative;
   `member` e `guest` passano solo i controlli che richiedono una membership
   generica.
4. **Permesso sulla risorsa.** Topic, task list e task usano grant gerarchici
   con livello e scope.
5. **Regole speciali dell'operazione.** Per esempio, completamento task e
   submission di un questionario richiedono l'assegnatario esatto.
6. **RLS PostgreSQL.** La transazione riceve `app.identity_id`,
   `app.device_id` e `app.project_id`; le policy impediscono accessi fuori
   identità/progetto e falliscono in chiusura quando il contesto manca.
7. **E2EE.** Il client può leggere il contenuto soltanto se dispone della KEK
   della risorsa e dell'epoca corretta.

```text
sessione valida
  -> membership attiva
    -> ruolo di progetto o permesso effettivo sulla risorsa
      -> eventuale regola speciale (assegnatario, requester, approvatore)
        -> RLS PostgreSQL
          -> envelope E2EE sul dispositivo
```

## Identità, sessioni e dispositivi

- Gli endpoint `/v1`, salvo le cerimonie pubbliche di autenticazione e gli
  health check, richiedono `Authorization: Bearer
  v1.<identity-id>.<session-id>.<secret>`.
- Il backend ricava sempre l'attore dalla sessione; non accetta dal payload un
  `actor_id` alternativo.
- Registrazione, rotazione e revoca dei key package sono consentite soltanto
  all'identità proprietaria del dispositivo. La richiesta deve inoltre
  riferirsi al dispositivo della sessione corrente dove previsto.
- Un membro attivo può elencare i key package pubblici dei dispositivi dei
  membri attivi del proprio progetto. Servono per costruire gli envelope E2EE,
  non concedono da soli accesso ai contenuti.
- Ogni dispositivo riceve solo gli envelope destinati alla propria identità e
  al proprio device.

## Ruoli e stati del progetto

I ruoli persistiti sono `owner`, `admin`, `member` e `guest`.

| Capacità di progetto | Owner | Admin | Member | Guest |
| --- | ---: | ---: | ---: | ---: |
| Visualizzare un progetto di cui ha membership attiva | sì | sì | sì | sì |
| Gestire inviti e condividerne le chiavi | sì | sì | no | no |
| Invitare come admin/member/guest | sì | sì | no | no |
| Bypass applicativo sui permessi di ogni risorsa | sì | sì | no | no |
| Gestire recovery provision | solo owner | no | no | no |
| Consultare stato/piano recovery | sì | sì | sì | sì |
| Creare e leggere preset/questionari di progetto | sì | sì | sì | sì |
| Versionare/pubblicare preset o questionari | sì | sì | no | no |

Note importanti:

- Il creatore di un progetto diventa l'unico `owner` attivo e crea la risorsa
  `root` del progetto.
- Gli inviti non possono assegnare il ruolo `owner`; possono assegnare soltanto
  `admin`, `member` o `guest`.
- Nei controlli applicativi correnti `member` e `guest` sono equivalenti ogni
  volta che una route richiede semplicemente `ProjectAccess::Member`. La loro
  visibilità dei contenuti dipende poi dai grant sulle risorse.
- `owner` e `admin` superano i controlli puntuali `ViewHeader`, `Read`, `Write`
  e `Manage` sulle risorse. Questo bypass applicativo non sostituisce la
  distribuzione delle chiavi E2EE.
- Non è attualmente esposta una route HTTP generale per cambiare ruolo,
  sospendere un membro, trasferire ownership o abbandonare un progetto.

## Gerarchia delle risorse

La gerarchia autorizzativa principale è:

```text
project / root
└── topic
    └── task list
        └── task
```

`resource_nodes` contiene anche i tipi tecnici `file` e `other`, ma la
materializzazione dei grant di dominio è definita per `topic`, `task_list` e
`task`. I file e i documenti Info riutilizzano il nodo della risorsa contenitore
e quindi non hanno un ACL indipendente.

Il database mantiene una closure table della gerarchia, rifiuta cicli e usa
foreign key composte con `project_id` per impedire collegamenti tra progetti.

### Livelli di accesso

Un grant usa uno dei seguenti livelli:

| Livello | Lettura body | Modifica risorsa | Gestione ACL/eliminazione |
| --- | ---: | ---: | ---: |
| `view` | sì, se scope `full` | no | no |
| `comment` | sì, se scope `full` | no | no |
| `edit` | sì, se scope `full` | sì | no |
| `manage` | sì, se scope `full` | sì | sì |

`comment` è oggi un livello riservato: per le operazioni generiche equivale a
`view`, perché non esiste ancora una capability/route commenti dedicata.

### Scope del grant

| Scope | Effetto |
| --- | --- |
| `full` | Espone body della risorsa target e propaga lo stesso livello a tutti i discendenti presenti e futuri. Gli antenati necessari alla navigazione ricevono `container_only`. |
| `container_only` | Espone solo il contenitore/header minimo della risorsa target. Non espone body, Info, download o discendenti e non si propaga. |

Un grant `full` su una task non espone il body della task list o del topic:
materializza soltanto gli antenati come `container_only`. Non concede accesso
ai sibling.

Se più grant si sovrappongono, il permesso effettivo preferisce:

1. `full` rispetto a `container_only`;
2. `manage` > `edit` > `comment` > `view`;
3. a parità, la sorgente gerarchicamente più vicina.

I grant conservano origine e lineage:

- `explicit`: grant diretto creato da un gestore della risorsa;
- `assignment`: grant generato da un'assegnazione task;
- `materialized`: riga derivata su antenati o discendenti, collegata al root
  grant.

La revoca rimuove soltanto il lineage del grant selezionato. Un altro grant
diretto o una diversa origine valida continua a produrre accesso.

Il campo `visibility` accetta oggi `private`, `restricted`, `project` e
`inherited`, ma non modifica la decisione di `require_resource_access`: il
controllo effettivo usa livello, scope, ruolo, creator e stato della membership.

## Matrice comune sulla singola risorsa

La funzione applicativa comune applica questa matrice:

| Attore | Header | Body / read | Modifica Info | Update generico | Manage/delete/ACL |
| --- | ---: | ---: | ---: | ---: | ---: |
| Owner o admin attivo | sì | sì | sì | sì | sì |
| Creatore della risorsa | sì | sì | sì | sì | no |
| Grant `full/manage` | sì | sì | sì | sì | sì |
| Grant `full/edit` | sì | sì | sì | sì | no |
| Grant `full/comment` | sì | sì | sì | no | no |
| Grant `full/view` | sì | sì | sì | no | no |
| Qualunque grant `container_only` | sì | no | no | no | no |
| Membro attivo senza ruolo privilegiato né grant | no | no | no | no | no |
| Membership non attiva/assente | no | no | no | no | no |

Il creatore può aggiornare la risorsa ma non può eliminarla, amministrarne i
permessi, inizializzarne le epoche o gestirne la rotazione senza un grant
`manage` o un ruolo owner/admin.

### Eccezione collaborativa Info

`EditInfo` coincide intenzionalmente con `Read`: chiunque possa vedere il body
completo del topic o della task list può creare, modificare ed eliminare i
documenti Info e caricarvi file, anche con livello `view` o `comment`.

Questa eccezione:

- vale per owner, admin, creator e qualunque grant con scope `full`;
- non vale per `container_only`;
- non concede l'update generico del topic o della task list;
- si applica sia al documento principale sia ai sotto-documenti;
- non crea ACL separati per i sotto-documenti.

## Permessi per elemento e operazione

### Progetto e inviti

| Operazione | Requisito |
| --- | --- |
| Creare un progetto | sessione autenticata; il chiamante diventa owner |
| Elencare i propri progetti | sessione autenticata; RLS restituisce solo membership visibili |
| Leggere un progetto | membership attiva |
| Creare/elencare inviti | owner o admin |
| Accettare un invito | sessione valida e token di invito valido/non scaduto |
| Cercare partecipanti | qualunque membership attiva |
| Condividere chiavi di risorse a un membro | owner o admin; destinatario e risorse devono essere autorizzati |

L'accettazione crea la membership, ma non sostituisce la consegna degli
envelope. Finché le chiavi non sono condivise il nuovo membro può risultare
autorizzato a livello server senza poter decifrare i contenuti.

### Topic

| Operazione | Requisito |
| --- | --- |
| Creare | `Write` sul parent, normalmente la root di progetto |
| Elencare | membership attiva; la query filtra per grant/creator |
| Leggere singolo topic | `Read` sul topic |
| Aggiornare | `Write` sul topic |
| Eliminare | `Manage` sul topic |
| Leggere Info | `Read` sul topic |
| Modificare Info | `EditInfo`, quindi qualunque accesso `full` al topic |

Poiché i grant gerarchici di dominio non vengono materializzati sulla root,
nello stato corrente la creazione di topic top-level è normalmente riservata a
owner, admin o creator della root.

### Task list

| Operazione | Requisito |
| --- | --- |
| Creare sotto un topic | `Write` sul topic |
| Elencare sotto un topic | `ViewHeader` sul topic; ogni lista è poi filtrata per grant/creator |
| Recuperare una lista | `ViewHeader` sulla lista |
| Aggiornare | `Write` sulla lista |
| Eliminare | `Manage` sulla lista |
| Elencare le task | `Read` sulla lista; le task sono poi filtrate per grant/creator |
| Leggere Info | `Read` sulla lista |
| Modificare Info | `EditInfo`, quindi qualunque accesso `full` alla lista |

Un accesso `container_only` consente la navigazione verso la task list ma non
consente di elencarne le task o aprirne le Info.

### Task

| Operazione | Requisito |
| --- | --- |
| Creare | `Write` sulla task list di destinazione |
| Leggere | `Read` sulla task |
| Aggiornare | `Write` sulla task |
| Eliminare | `Manage` sulla task |
| Copiare una task completata | task sorgente esistente/completata e `Write` sulla lista di destinazione |
| Assegnare, elencare o revocare assegnazioni | `Manage` sulla task |
| Completare | corrispondenza esatta con l'assegnazione attiva indicata |

Il completamento è una capability specifica dell'assegnatario: non è sufficiente
un generico `Write` e non è sostituita dal ruolo admin. La richiesta deve
contenere l'ID della stessa assegnazione attiva associata all'identità corrente.

### Assegnazioni

- Il destinatario deve essere un membro attivo del progetto.
- Un owner/admin può assegnare a qualunque membro attivo. Un altro gestore può
  assegnare soltanto se il destinatario ha già accesso `full` alla task list
  contenitore.
- L'assegnazione crea attualmente un grant `edit/full` sulla task, con origine
  `assignment`, più i permessi `container_only` necessari sugli antenati.
- La revoca ruota le chiavi interessate e rimuove il lineage
  dell'assegnazione, preservando eventuali grant indipendenti.
- Solo l'assegnatario attivo può completare la task, creare un allegato di
  completamento o finalizzare il relativo questionario.

### Documenti Info e sotto-documenti

- I documenti Info sono associati a un topic oppure a una task list.
- Un sotto-documento deve appartenere allo stesso contenitore del parent.
- Lista e lettura richiedono `Read` sulla risorsa contenitore.
- Creazione, update e soft delete richiedono `EditInfo`.
- I file Info dichiarati e caricati richiedono `EditInfo`; il download richiede
  `Read`.
- Documento, sotto-documenti e file usano la stessa risorsa/epoca crittografica
  del topic o della task list. ID documento e tipo del contenuto sono vincolati
  nell'AAD, quindi un ciphertext non può essere spostato tra contesti senza
  fallire l'autenticazione.

### Allegati e file

| Tipo/operazione | Requisito |
| --- | --- |
| Allegato template di un pretask | creazione owner/admin; elenco membership attiva con filtro risorsa |
| Allegato richiesto di una task | `Write` sulla task |
| Allegato di completamento | assegnatario esatto e attivo |
| Allegato Info | `EditInfo` sul topic/task list contenitore |
| Metadati e download blob | `Read` sulla risorsa associata |
| Upload blob già dichiarato | stessa capability usata per dichiararne il tipo |

Nomi file, MIME semantici e contenuto sono cifrati. Il filesystem server vede
soltanto nomi opachi, dimensioni, hash e ciphertext.

### Ricorrenze

| Operazione | Requisito |
| --- | --- |
| Creare una serie | `Write` sulla task list |
| Leggere una serie | `Read` sulla task list |
| Archiviare una serie | `Manage` sulla task list |

La generazione della prossima occorrenza avviene nel flusso autorizzato di
completamento della task assegnata.

### Preset

| Operazione | Requisito |
| --- | --- |
| Creare, elencare o leggere un preset | membership attiva, incluso guest |
| Eliminare un preset | owner o admin |
| Creare una versione | owner o admin |
| Leggere una versione | membership attiva |
| Creare un'assegnazione preset | `Manage` sulla task list di destinazione |
| Materializzare | autore dell'assegnazione, destinatario, owner/admin o `manage/full` sulla lista |

Per creare un'assegnazione preset il destinatario deve essere membro attivo;
se l'attore non è owner/admin, il destinatario deve già avere accesso `full`
alla lista.

### Questionari

| Operazione | Requisito |
| --- | --- |
| Creare, elencare o leggere un questionario | membership attiva, incluso guest |
| Creare/aggiornare una versione draft | owner o admin |
| Pubblicare una versione | owner o admin |
| Leggere versioni | membership attiva |
| Creare/aggiornare un submission draft | assegnatario esatto e attivo della task |
| Finalizzare il submission | stesso assegnatario, autore del draft, firme device valide |
| Leggere un draft | assegnatario esatto e attivo |
| Leggere un submission finalizzato | `Read` sulla task |

### Sync

- `push` richiede `Write` sulla risorsa target oltre a firme, versione base e
  catena device valide.
- `pull` e WebSocket wake richiedono membership attiva; gli eventi/projection
  restituiti sono ulteriormente filtrati secondo la visibilità effettiva della
  risorsa e RLS.
- Un WebSocket wake è soltanto un segnale: il client deve effettuare un pull
  autorizzato per ottenere i dati.

### Retention ed export

Le preferenze retention sono personali: ogni identità può leggere e modificare
solo le proprie. Warning, elenco archivi, download e receipt sono limitati al
`recipient_identity_id` della sessione. Un altro membro, compresi owner/admin,
non può scaricare l'archivio personale del destinatario attraverso queste
route.

I job cross-project di retention/export usano un ruolo PostgreSQL operativo
separato con `BYPASSRLS`. Questo ruolo:

- non è una service role disponibile agli utenti;
- non deve essere usato dalle connessioni HTTP;
- non può essere attivato impostando una variabile di sessione;
- deve essere provisionato e custodito separatamente dal ruolo applicativo.

### Recovery E2EE

- Qualunque membro attivo può consultare lo stato di provision, il piano di
  rotazione e le proprie share.
- Solo l'owner può creare/aggiornare e attivare il recovery set, anche se il
  controllo preliminare è di tipo project-manage.
- Una recovery `participant_device` può essere iniziata da un non-owner e deve
  essere approvata dall'owner.
- Una recovery `lost_owner` può essere iniziata soltanto dall'owner e richiede
  l'approvazione unanime di tutti i membri attivi non-owner inclusi
  nell'elettorato congelato.
- Può approvare soltanto un'identità presente nell'elettorato della richiesta,
  con firme Ed25519 e ML-DSA valide; per `lost_owner` deve anche possedere una
  share del recovery set.
- Soltanto il requester può finalizzare, dopo tutte le approvazioni e prima
  della scadenza.
- Un progetto con solo owner o con un partecipante/share non disponibile non
  può completare la recovery `lost_owner`. Non esiste bypass server.

## E2EE e revoca

Ogni risorsa ha una KEK distinta per epoca. Body e header minimo usano chiavi
distinte:

- accesso `full`: envelope della body key e, se presente, della header key;
- accesso `container_only`: solo header key;
- nessun accesso: nessun envelope corrente.

Grant, assegnazioni e revoche validano la copertura esatta dei dispositivi
attivi. La revoca crea una nuova epoca e redistribuisce le nuove chiavi ai soli
destinatari rimasti autorizzati. Protegge le revisioni future, ma non può
cancellare plaintext, screenshot, export, ciphertext o chiavi già scaricati da
un dispositivo precedentemente autorizzato.

Owner e admin hanno un bypass **autorizzativo**, non una master key implicita:
se non hanno ricevuto gli envelope corretti il server può restituire il
ciphertext, ma il client non può decifrarlo.

## RLS e separazione dei ruoli database

- Il ruolo API deve essere diverso dal proprietario delle tabelle/migrazioni e
  non deve avere `BYPASSRLS`.
- Le transazioni utente impostano il contesto con `set_config(..., true)`, che
  resta locale alla transazione.
- Le tabelle identità sono isolate per `identity_id`; le tabelle di progetto
  sono isolate per membership e `project_id`; envelope, recovery share,
  retention e sync hanno policy più specifiche per destinatario/risorsa.
- `projects` e `project_memberships` non sono `FORCE ROW LEVEL SECURITY` perché
  sono policy-root lette da funzioni `SECURITY DEFINER`; il ruolo applicativo
  resta comunque soggetto alle policy.
- Molte altre tabelle usano `FORCE ROW LEVEL SECURITY`.
- RLS è un backstop di isolamento e non sostituisce i controlli di livello
  `view/edit/manage` eseguiti dal server.
- Nessun input del client abilita il bypass worker.

## Risposte di autorizzazione

- `401 Unauthorized`: token mancante, malformato, scaduto o revocato.
- `403 Forbidden`: attore autenticato ma privo del ruolo, grant o diritto
  speciale richiesto.
- `404 Not Found`: risorsa inesistente/eliminata; in alcune query RLS o filtri
  di visibilità rendono una risorsa non visibile come se non esistesse.
- `409 Conflict`: versione ottimistica non corrente, stato non compatibile,
  replay non equivalente o operazione concorrente.

## Note sull'implementazione corrente

Queste note descrivono differenze osservabili tra intenti/commenti e codice;
non sono capacità da assumere come design definitivo.

1. **Assegnazione e update generico.** I commenti in `auth.rs` dichiarano che
   un'assegnazione non dovrebbe consentire scritture generiche, ma la route di
   assegnazione materializza `edit/full`. Poiché `require_resource_access`
   decide usando livello e scope senza conoscere l'origine, l'assegnatario
   supera oggi anche `Write` sulla task. Il completamento e gli allegati
   completati restano comunque vincolati all'assegnazione esatta.
2. **Collection owner/admin.** I controlli puntuali riconoscono sempre il
   bypass owner/admin. Alcune query collection di topic/task list/task filtrano
   invece esplicitamente solo `permission.access_scope` o creator. Un
   owner/admin senza una riga di permesso e non creator può quindi vedere una
   differenza tra accesso puntuale e presenza nelle collection.
3. **GET task list con header-only.** L'endpoint puntuale richiede
   `ViewHeader`, mentre il loader recupera anche il ciphertext del body. Un
   utente `container_only` non riceve la body key e non può decifrarlo, ma il
   trasporto del ciphertext è più ampio della separazione header/body descritta
   dall'ADR. Le collection, invece, omettono il body senza scope `full`.
4. **Ruolo guest.** Dove è richiesto soltanto `ProjectAccess::Member`, `guest`
   e `member` sono equivalenti. Questo include attualmente creazione/lettura di
   preset e questionari di progetto.

Qualunque modifica di questi punti deve essere trattata come cambiamento di
policy, accompagnata da test API, RLS e regressione E2EE; non va corretta
silenziosamente nel frontend.

## Scenari rapidi

- Un membro con `view/full` su una task list legge lista e discendenti, non
  modifica le task, ma può modificare le Info della lista.
- Un membro con `edit/full` sulla stessa lista può anche creare e aggiornare le
  task discendenti; non può eliminarle o gestirne gli ACL senza `manage`.
- Un utente con `manage/container_only` su un antenato vede soltanto l'header:
  il livello alto non supera la limitazione dello scope.
- Un utente con grant diretto sulla task vede gli header minimi di topic e
  lista, non i loro body né i sibling.
- Un admin supera i controlli puntuali su tutte le risorse del progetto, ma ha
  comunque bisogno degli envelope per decifrare.
- Un membro con sola membership, senza grant e senza essere creator, non vede
  automaticamente topic, task list o task.

## Checklist per nuove funzionalità

Per ogni nuovo elemento associato a topic, task list o task occorre definire e
testare esplicitamente:

1. quale `resource_node_id` governa l'elemento;
2. quale operazione comune usa (`ViewHeader`, `Read`, `EditInfo`, `Write` o
   `Manage`);
3. se esiste una capability speciale più stretta;
4. come si comportano `full` e `container_only`;
5. quali key envelope vengono consegnati e quale AAD lega il ciphertext;
6. come grant e revoca influenzano le epoche;
7. policy RLS per select, insert, update e delete;
8. isolamento cross-project e tra sibling;
9. comportamento di owner, admin, creator, member, guest e membership non
   attiva;
10. test diretto API, SQL/RLS e crittografico.
