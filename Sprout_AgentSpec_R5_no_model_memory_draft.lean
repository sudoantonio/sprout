import Std

/-!


Specifica formale del comportamento
dell'agente Sprout
Revisione 4 — estensioni correnti
Questa revisione consolida il nucleo precedente e introduce cinque estensioni
verificabili, identificate con etichette R4.x :

         R4.1 — Tool call e audit. Ogni invocazione asincrona riceve un
          ToolCallId , mantiene stato, tentativo, policy di retry/timeout e audit log.
         R4.2 — PromptSemantics derivata. La semantica astratta viene derivata da
         un PromptCompiler e vengono dimostrate le relative leggi di coerenza.
         R4.3 — Fairness e runtime. Disponibilità dinamica del runtime, fairness,
         timeout e retry diventano proprietà esplicite delle run.
         R4.4 — Causalità e coordinamento multi-agente. Le mosse sono attribuite
         al principal che le esegue; commenti e deleghe agent-to-agent hanno priorità
         e profondità limitate per evitare cicli non terminanti.
         R4.5 — CorrectionProfile verificabile. Le correzioni non sono più un
         dato arbitrario dell'esito: vengono contate direttamente da un prefisso
         finito della run.


Vincolo intenzionale sulla crittografia
La revisione non raffina algoritmi crittografici, envelope, rotazione delle
chiavi o gestione concreta del materiale segreto. EncryptedPayload ,
EncryptedEnvelope e keyEpoch restano astrazioni dell'ambiente di
integrazione. Le sole condizioni mantenute sono quelle strutturali già
necessarie al contratto applicativo.


Finalità
La specifica separa:

      1. vocabolario astratto e stato;
      2. semantica del prompt;
      3. osservazioni e invarianti;
      4. autorizzazione;
      5. effetti atomici e audit;
      6. semantica temporale, fairness e causalità;
      7. coordinamento multi-agente;
   8. preferenze e valutazione verificabile.
      -/

namespace Sprout.AgentSpec

universe u

/-! ## 1. Vocabolario astratto e stato -/

/-- Tipi primitivi forniti dall'implementazione o dal refinement concreto. -/
structure Vocabulary where
PrincipalId : Type u
ProjectId : Type u
ResourceId : Type u
SessionId : Type u
SystemPrompt : Type u
EncryptedPayload : Type u
EncryptedEnvelope : Type u
BlobId : Type u
Tool : Type u
ToolInput : Type u
ToolOutput : Type u
ToolError : Type u
ToolCallId : Type u
CommentId : Type u
ObligationId : Type u
HumanMove : Type u

/-- Ruolo organizzativo di un principal. Il ruolo non concede privilegi impliciti. -/
inductive PrincipalKind where
| administrator
| user
| agent
deriving DecidableEq, Repr

/-- Tassonomia minima delle risorse Sprout. -/
inductive ResourceKind where
| project
| topic
| taskList
| task
deriving DecidableEq, Repr

/-- Capacità effettive sulle risorse. -/
inductive Capability where
| view
| comment
| createTask
| manageOwnTask
| useTool
deriving DecidableEq, Repr

/-- Stato logico minimo di una task. -/
inductive TaskStatus where
| open
| done
deriving DecidableEq, Repr

/-- Stato persistito di un'invocazione tool. -/
inductive ToolCallStatus where
| pending
| succeeded
| failed
| timedOut
deriving DecidableEq, Repr

/-- Tipi di record append-only nel log di audit dei tool. -/
inductive ToolAuditKind where
| requested
| retryStarted
| completed
| failed
| timedOut
deriving DecidableEq, Repr

/-- Policy astratta di retry/timeout associata a una singola invocazione. -/
structure ToolRetryPolicy where
maxAttempts : Nat
timeoutTicks : Nat
deriving DecidableEq, Repr

/-- Limiti strutturali della collaborazione agent-to-agent. -/
structure CoordinationPolicy where
maxAgentCommentDepth : Nat
maxAgentTaskDelegationDepth : Nat
deriving DecidableEq, Repr

/-- Metadati di provenienza e concorrenza controllati dal sistema. -/
structure ResourceMeta (V : Vocabulary) where
id : V.ResourceId
projectId : V.ProjectId
kind : ResourceKind
parent : Option V.ResourceId
creator : V.PrincipalId
version : Nat
deleted : Bool
sourceTask : Option V.ResourceId
agentDelegationDepth : Nat

/-- Nota indivisibile e append-only. -/
structure NoteEntry (V : Vocabulary) where
payload : V.EncryptedPayload
keyEpoch : Nat

/-- Riferimento a un blob con metadati semantici cifrati. -/
structure AttachmentRef (V : Vocabulary) where
blobId : V.BlobId
metadata : V.EncryptedPayload
keyEpoch : Nat

/-- Documento mutabile della task. -/
structure TaskData (V : Vocabulary) where
status : TaskStatus
assignees : V.PrincipalId → Prop
payload : V.EncryptedPayload
keyEpoch : Nat
notes : List (NoteEntry V)
attachments : List (AttachmentRef V)

/-- Bozza di creazione. Creator, id e profondità sono derivati server-side. -/
structure NewTask (V : Vocabulary) where
parentList : V.ResourceId
sourceTask : Option V.ResourceId
data : TaskData V
envelopes : List V.EncryptedEnvelope

/-- Bozza di commento dell'agente; autore, id e profondità sono server-side. -/
structure NewComment (V : Vocabulary) where
recipient : V.PrincipalId
target : V.ResourceId
parent : Option V.CommentId
payload : V.EncryptedPayload
keyEpoch : Nat

/-- Commento indirizzato a un agente nel contesto di una risorsa. -/
structure Comment (V : Vocabulary) where
id : V.CommentId
author : V.PrincipalId
recipient : V.PrincipalId
target : V.ResourceId
parent : Option V.CommentId
agentDepth : Nat
payload : V.EncryptedPayload
keyEpoch : Nat

/-- Record persistito di una chiamata tool. -/
structure ToolCallRecord (V : Vocabulary) where
id : V.ToolCallId
owner : V.PrincipalId
tool : V.Tool
input : V.ToolInput
attempt : Nat
maxAttempts : Nat
timeoutTicks : Nat
status : ToolCallStatus
output : Option V.ToolOutput
failure : Option V.ToolError

/-- Voce append-only dell'audit log tool. -/
structure ToolAuditEntry (V : Vocabulary) where
callId : V.ToolCallId
owner : V.PrincipalId
tool : V.Tool
attempt : Nat
kind : ToolAuditKind

/-- Fotografia logica del sistema, inclusa la parte operativa dei tool. -/
structure State (V : Vocabulary) where
principals : V.PrincipalId → Option PrincipalKind
resources : V.ResourceId → Option (ResourceMeta V)
tasks : V.ResourceId → Option (TaskData V)
permissions : V.PrincipalId → V.ResourceId → Capability → Prop
toolPermission : V.PrincipalId → V.Tool → Prop
systemPrompts : V.PrincipalId → Option V.SystemPrompt
activeEpoch : V.ResourceId → Nat
comments : List (Comment V)
toolCalls : V.ToolCallId → Option (ToolCallRecord V)
toolAudit : List (ToolAuditEntry V)
runtimeAvailable : V.Tool → Prop
coordination : CoordinationPolicy

/-- Identità operativa stabile e tool installati nel runtime dell'agente. -/
structure AgentProfile (V : Vocabulary) where
principal : V.PrincipalId
session : V.SessionId
availableTools : V.Tool → Prop

/-- Directory dei profili agenti usata dalle run multi-agente. -/
abbrev AgentDirectory (V : Vocabulary) :=
V.PrincipalId → Option (AgentProfile V)

/-- Boundary server-side che risolve la sessione nell'attore autenticato. -/
structure ApiBoundary (V : Vocabulary) where
actorOf : V.SessionId → Option V.PrincipalId

/-- Linguaggio chiuso delle intenzioni dell'agente. -/
inductive AgentAction (V : Vocabulary) where
| createTask (draft : NewTask V)
| replaceOwnTask (task : V.ResourceId) (next : TaskData V)
| deleteOwnTask (task : V.ResourceId)
| assignOwnTask (task : V.ResourceId) (assignee : V.PrincipalId)
| unassignOwnTask (task : V.ResourceId) (assignee : V.PrincipalId)
| markAssignedDone (task : V.ResourceId)
| appendAssignedNote (task : V.ResourceId) (note : NoteEntry V)
| addAssignedAttachment (task : V.ResourceId) (attachment : AttachmentRef V)
| postComment (draft : NewComment V)
| invokeTool (tool : V.Tool) (input : V.ToolInput) (policy : ToolRetryPolicy)
| retryTool (callId : V.ToolCallId)
| noOp

/-! ## 2. R4.2 — derivazione concreta di PromptSemantics -/

/-- Interfaccia semantica consumata dal nucleo di autorizzazione e liveness. -/
structure PromptSemantics (V : Vocabulary) where
serves : V.SystemPrompt → State V → AgentAction V → Prop
mayWait : V.SystemPrompt → State V → Prop
obligation : V.SystemPrompt → State V → V.ObligationId → Prop
discharged : V.SystemPrompt → State V → V.ObligationId → Prop

/-- Leggi che ogni semantica del prompt deve rispettare. -/
structure PromptSemanticsLaws
(V : Vocabulary)
(P : PromptSemantics V) : Prop where
discharged_not_active :
∀ prompt state obligation,
P.discharged prompt state obligation →
¬ P.obligation prompt state obligation
serves_non_noop :
∀ prompt state action,
P.serves prompt state action →
action ≠ AgentAction.noOp

/-
Programma logico compilato da un prompt. La compilazione linguistica concreta
resta esterna; una volta prodotto questo oggetto, la semantica è determinata.
-/
structure PromptProgram (V : Vocabulary) where
actionSupports : State V → AgentAction V → Prop
waitingAllowed : State V → Prop
activeObligation : State V → V.ObligationId → Prop
dischargeEvidence : State V → V.ObligationId → Prop
evidenceCloses :
∀ state obligation,
dischargeEvidence state obligation →
¬ activeObligation state obligation

/-- Compilatore astratto dall'oggetto prompt al programma logico verificabile. -/
structure PromptCompiler (V : Vocabulary) where
compile : V.SystemPrompt → PromptProgram V

/-- Semantica derivata meccanicamente dal programma compilato. -/
def DerivedPromptSemantics
(compiler : PromptCompiler V) : PromptSemantics V where
serves := fun prompt state action =>
action ≠ AgentAction.noOp ∧
(compiler.compile prompt).actionSupports state action
mayWait := fun prompt state =>
(compiler.compile prompt).waitingAllowed state
obligation := fun prompt state obligation =>
(compiler.compile prompt).activeObligation state obligation
discharged := fun prompt state obligation =>
(compiler.compile prompt).dischargeEvidence state obligation

/-- La derivazione soddisfa sempre le leggi richieste. -/
theorem derived_prompt_semantics_laws
(compiler : PromptCompiler V) :
PromptSemanticsLaws V (DerivedPromptSemantics compiler) := by
constructor
· intro prompt state obligation h
  exact (compiler.compile prompt).evidenceCloses state obligation h
· intro prompt state action h
  exact h.1

/-! ## 3. Eventi, mosse e transizioni -/

/-- Eventi osservabili. I risultati tool sono correlati tramite ToolCallId . -/
inductive Event (V : Vocabulary) where
| resourceUpdated (actor : V.PrincipalId) (target : V.ResourceId)
| commentPosted (comment : Comment V)
| toolCompleted (callId : V.ToolCallId) (output : V.ToolOutput)
| toolFailed (callId : V.ToolCallId) (failure : V.ToolError)
| toolTimedOut (callId : V.ToolCallId)
| runtimeAvailabilityChanged (tool : V.Tool) (available : Bool)

/-- Relazione astratta fra evento e risposta semanticamente pertinente. -/
structure ResponseSemantics (V : Vocabulary) where
respondsTo : State V → Event V → AgentAction V → Prop

/-- Ogni mossa porta esplicitamente l'identità dell'attore. -/
inductive Move (V : Vocabulary) where
| agentMove (actor : V.PrincipalId) (action : AgentAction V)
| humanMove (actor : V.PrincipalId) (move : V.HumanMove)

/-- Relazioni dichiarate dall'implementazione concreta. -/
structure TransitionSystem (V : Vocabulary) where
agentStep : State V → V.SessionId → AgentAction V → State V → Prop
humanStep : State V → V.PrincipalId → V.HumanMove → State V → Prop
runtimeStep : State V → Event V → State V → Prop

variable {V : Vocabulary}

/-! ## 4. Osservazioni e invarianti -/

/-- Il principal esiste ed è classificato con il ruolo indicato. -/
def HasKind (s : State V) (p : V.PrincipalId) (k : PrincipalKind) : Prop :=
s.principals p = some k

/-- La risorsa è visibile al principal. -/
def Visible (s : State V) (p : V.PrincipalId) (r : V.ResourceId) : Prop :=
s.permissions p r Capability.view

/-- Il principal possiede la capability indicata sulla risorsa. -/
def HasCapability
(s : State V) (p : V.PrincipalId) (r : V.ResourceId)
(cap : Capability) : Prop :=
s.permissions p r cap

/-- La risorsa esiste, ha il tipo atteso e non è cancellata. -/
def IsResourceKind
(s : State V) (r : V.ResourceId) (k : ResourceKind) : Prop :=
∃ _meta,
s.resources r = some _meta ∧
_meta.kind = k ∧
_meta.deleted = false

/-- Provenienza storica, indipendente dalla tombstone. -/
def HistoricallyCreatedBy
(s : State V) (r : V.ResourceId) (p : V.PrincipalId) : Prop :=
∃ _meta,
s.resources r = some _meta ∧
_meta.creator = p

/-- Ownership operativa corrente. -/
def CreatedBy
(s : State V) (r : V.ResourceId) (p : V.PrincipalId) : Prop :=
∃ _meta,
s.resources r = some _meta ∧
_meta.creator = p ∧
_meta.deleted = false

/-- Task attiva creata dal principal. -/
def IsOwnTask
(s : State V) (p : V.PrincipalId) (task : V.ResourceId) : Prop :=
IsResourceKind s task ResourceKind.task ∧ CreatedBy s task p

/-- Il principal appartiene agli assegnatari della task. -/
def AssignedTo
(s : State V) (p : V.PrincipalId) (task : V.ResourceId) : Prop :=
∃ data, s.tasks task = some data ∧ data.assignees p

/-- La task è aperta. -/
def OpenTask (s : State V) (task : V.ResourceId) : Prop :=
∃ data, s.tasks task = some data ∧ data.status = TaskStatus.open

/-- La task è completata. -/
def DoneTask (s : State V) (task : V.ResourceId) : Prop :=
∃ data, s.tasks task = some data ∧ data.status = TaskStatus.done

/-- Amministratori e utenti sono umani; gli agenti non lo sono. -/
def IsHumanKind : PrincipalKind → Prop
| PrincipalKind.administrator => True
| PrincipalKind.user => True
| PrincipalKind.agent => False

/-- Priorità dei commenti: amministratore > utente > agente. -/
def CommentPriority : PrincipalKind → Nat
| PrincipalKind.administrator => 3
| PrincipalKind.user => 2
| PrincipalKind.agent => 1

/-- Tipi di risorsa che producono trigger significativi. -/
def RelevantTriggerKind : ResourceKind → Prop
| ResourceKind.topic => True
| ResourceKind.taskList => True
| ResourceKind.task => True
| ResourceKind.project => False

/-- Risorsa visibile, attiva e appartenente a un tipo di trigger. -/
def RelevantVisibleResource
(s : State V) (agent : V.PrincipalId) (r : V.ResourceId) : Prop :=
Visible s agent r ∧
∃ _meta,
s.resources r = some _meta ∧
RelevantTriggerKind _meta.kind ∧
_meta.deleted = false

/-- Recupera logicamente un commento dal suo identificativo. -/
def HasCommentId
(s : State V) (commentId : V.CommentId) (comment : Comment V) : Prop :=
comment ∈ s.comments ∧ comment.id = commentId

/-- Un agente può aprire al massimo una radice per coppia autore/destinatario/target. -/
def AgentRootCommentFresh
(s : State V)
(author recipient : V.PrincipalId)
(target : V.ResourceId) : Prop :=
¬ ∃ existing,
existing ∈ s.comments ∧
existing.author = author ∧
existing.recipient = recipient ∧
existing.target = target ∧
existing.parent = none ∧
existing.agentDepth > 0

/-- Forma consentita per un commento prodotto da un agente già persistito. -/
def AgentCommentShape
(s : State V) (c : Comment V) : Prop :=
c.agentDepth > 0 ∧
c.agentDepth ≤ s.coordination.maxAgentCommentDepth ∧
match c.parent with
| none =>
c.agentDepth = 1
| some parentId =>
∃ parentComment,
HasCommentId s parentId parentComment ∧
parentComment.recipient = c.author ∧
parentComment.target = c.target ∧
c.agentDepth = parentComment.agentDepth + 1

/-- Ammissibilità uniforme dei commenti di umani e agenti. -/
def CommentAdmissible (s : State V) (c : Comment V) : Prop :=
(∃ kind,
s.principals c.author = some kind ∧
match kind with
| PrincipalKind.agent => AgentCommentShape s c
| PrincipalKind.administrator => c.agentDepth = 0
| PrincipalKind.user => c.agentDepth = 0) ∧
HasKind s c.recipient PrincipalKind.agent ∧
c.author ≠ c.recipient ∧
Visible s c.author c.target ∧
HasCapability s c.author c.target Capability.comment ∧
(Visible s c.recipient c.target ∨ CreatedBy s c.target c.recipient) ∧
c.keyEpoch = s.activeEpoch c.target

/-- Vincolo sulla bozza di un nuovo commento agent-to-agent. -/
def AgentCommentDraftAllowed
(s : State V) (profile : AgentProfile V) (draft : NewComment V) : Prop :=
HasKind s draft.recipient PrincipalKind.agent ∧
profile.principal ≠ draft.recipient ∧
Visible s profile.principal draft.target ∧
HasCapability s profile.principal draft.target Capability.comment ∧
(Visible s draft.recipient draft.target ∨ CreatedBy s draft.target draft.recipient) ∧
draft.keyEpoch = s.activeEpoch draft.target ∧
match draft.parent with
| none =>
s.coordination.maxAgentCommentDepth > 0 ∧
AgentRootCommentFresh s profile.principal draft.recipient draft.target
| some parentId =>
∃ parentComment,
HasCommentId s parentId parentComment ∧
parentComment.recipient = profile.principal ∧
parentComment.target = draft.target ∧
parentComment.agentDepth < s.coordination.maxAgentCommentDepth

/-- Tool installato, autorizzato e attualmente disponibile per una nuova chiamata. -/
def ToolReady
(s : State V) (profile : AgentProfile V) (tool : V.Tool) : Prop :=
profile.availableTools tool ∧
s.toolPermission profile.principal tool ∧
s.runtimeAvailable tool

/-- Chiamata tool pendente e appartenente al principal indicato. -/
def PendingToolCallOwnedBy
(s : State V) (owner : V.PrincipalId) (callId : V.ToolCallId) : Prop :=
∃ call,
s.toolCalls callId = some call ∧
call.owner = owner ∧
call.status = ToolCallStatus.pending

/-- Stato terminale di una chiamata tool. -/
def ToolCallTerminal (call : ToolCallRecord V) : Prop :=
match call.status with
| ToolCallStatus.pending => False
| _ => True

/-- La chiamata può essere ritentata senza superare il limite di tentativi. -/
def RetryEligible
(s : State V) (profile : AgentProfile V) (callId : V.ToolCallId) : Prop :=
∃ call,
s.toolCalls callId = some call ∧
call.owner = profile.principal ∧
(call.status = ToolCallStatus.failed ∨
call.status = ToolCallStatus.timedOut) ∧
call.attempt < call.maxAttempts ∧
ToolReady s profile call.tool

/-- Una delega agentica da una task sorgente è strutturalmente limitata. -/
def AgentDelegationAllowed
(s : State V) (profile : AgentProfile V) (draft : NewTask V) : Prop :=
match draft.sourceTask with
| none => True
| some source =>
IsResourceKind s source ResourceKind.task ∧
Visible s profile.principal source ∧
(IsOwnTask s profile.principal source ∨ AssignedTo s profile.principal source) ∧
(∃ _sourceMeta,
s.resources source = some _sourceMeta ∧
_sourceMeta.agentDelegationDepth <
s.coordination.maxAgentTaskDelegationDepth) ∧
¬ ∃ child _childMeta,
s.resources child = some _childMeta ∧
_childMeta.deleted = false ∧
_childMeta.creator = profile.principal ∧
_childMeta.sourceTask = some source

/-- Un evento è rilevante per il profilo nello stato corrente. -/
def Activates
(s : State V) (profile : AgentProfile V) : Event V → Prop
| Event.resourceUpdated _ target =>
RelevantVisibleResource s profile.principal target
| Event.commentPosted comment =>
comment.recipient = profile.principal ∧ CommentAdmissible s comment
| Event.toolCompleted callId _ =>
PendingToolCallOwnedBy s profile.principal callId
| Event.toolFailed callId _ =>
PendingToolCallOwnedBy s profile.principal callId
| Event.toolTimedOut callId =>
PendingToolCallOwnedBy s profile.principal callId
| Event.runtimeAvailabilityChanged tool true =>
∃ callId call,
s.toolCalls callId = some call ∧
call.owner = profile.principal ∧
(call.status = ToolCallStatus.failed ∨
call.status = ToolCallStatus.timedOut) ∧
call.attempt < call.maxAttempts ∧
call.tool = tool
| Event.runtimeAvailabilityChanged _ false => False

/-
R4.1: ricevere un completamento non dipende dalla disponibilità corrente del
tool. L'unico requisito è che esista una chiamata pendente attribuita
all'agente.
-/
theorem pending_tool_completion_activates
(s : State V) (profile : AgentProfile V)
(callId : V.ToolCallId) (output : V.ToolOutput)
(h : PendingToolCallOwnedBy s profile.principal callId) :
Activates s profile (Event.toolCompleted callId output) := by
exact h

/-- Coerenza minima di un record tool. -/
def ToolCallWellFormed
(s : State V) (callId : V.ToolCallId) (call : ToolCallRecord V) : Prop :=
call.id = callId ∧
HasKind s call.owner PrincipalKind.agent ∧
1 ≤ call.attempt ∧
1 ≤ call.maxAttempts ∧
call.attempt ≤ call.maxAttempts ∧
1 ≤ call.timeoutTicks ∧
match call.status with
| ToolCallStatus.pending => call.output = none ∧ call.failure = none
| ToolCallStatus.succeeded => ∃ output, call.output = some output ∧ call.failure = none
| ToolCallStatus.failed => ∃ failure, call.failure = some failure ∧ call.output = none
| ToolCallStatus.timedOut => call.output = none

/-- Coerenza strutturale minima dello stato applicativo. -/
def WellFormedState (s : State V) : Prop :=
(∀ r _meta, s.resources r = some _meta → _meta.id = r) ∧
(∀ r data,
s.tasks r = some data →
IsResourceKind s r ResourceKind.task ∧
data.keyEpoch = s.activeEpoch r ∧
(∀ p, data.assignees p → ∃ kind, s.principals p = some kind) ∧
(∀ note, note ∈ data.notes → note.keyEpoch ≤ s.activeEpoch r) ∧
(∀ attachment,
attachment ∈ data.attachments →
attachment.keyEpoch ≤ s.activeEpoch r)) ∧
(∀ r _meta,
s.resources r = some _meta →
_meta.kind = ResourceKind.task →
_meta.deleted = false →
∃ data, s.tasks r = some data) ∧
(∀ r _meta,
s.resources r = some _meta →
_meta.kind = ResourceKind.task →
_meta.deleted = false →
∃ parent _parentMeta,
_meta.parent = some parent ∧
s.resources parent = some _parentMeta ∧
_parentMeta.kind = ResourceKind.taskList ∧
_parentMeta.projectId = _meta.projectId ∧
_parentMeta.deleted = false) ∧
(∀ r _meta,
s.resources r = some _meta →
∃ kind, s.principals _meta.creator = some kind) ∧
(∀ r _meta,
s.resources r = some _meta →
_meta.kind = ResourceKind.task →
match _meta.sourceTask with
| none => _meta.agentDelegationDepth = 0
| some source =>
∃ _sourceMeta,
s.resources source = some _sourceMeta ∧
_sourceMeta.kind = ResourceKind.task ∧
_sourceMeta.projectId = _meta.projectId ∧
_meta.agentDelegationDepth = _sourceMeta.agentDelegationDepth + 1 ∧
_meta.agentDelegationDepth ≤
s.coordination.maxAgentTaskDelegationDepth) ∧
(∀ p prompt,
s.systemPrompts p = some prompt →
HasKind s p PrincipalKind.agent) ∧
(∀ comment,
comment ∈ s.comments →
(∃ kind, s.principals comment.author = some kind) ∧
HasKind s comment.recipient PrincipalKind.agent ∧
comment.author ≠ comment.recipient ∧
(∃ _targetMeta,
s.resources comment.target = some _targetMeta ∧
_targetMeta.deleted = false) ∧
comment.keyEpoch ≤ s.activeEpoch comment.target ∧
(match s.principals comment.author with
| some PrincipalKind.agent =>
comment.agentDepth > 0 ∧
comment.agentDepth ≤ s.coordination.maxAgentCommentDepth
| some PrincipalKind.administrator => comment.agentDepth = 0
| some PrincipalKind.user => comment.agentDepth = 0
| none => False)) ∧
(∀ callId call,
s.toolCalls callId = some call → ToolCallWellFormed s callId call) ∧
(∀ entry,
entry ∈ s.toolAudit →
∃ call,
s.toolCalls entry.callId = some call ∧
call.owner = entry.owner ∧
call.tool = entry.tool)

/-! ## 5. Autorizzazione operativa e normativa -/

/-- Materiale minimo richiesto alla prima epoca crittografica. -/
def InitialCryptoMaterial (draft : NewTask V) : Prop :=
draft.data.keyEpoch = 1 ∧ draft.envelopes ≠ []

/-- Autorizzazione tecnica derivata da stato, ownership, scope e runtime. -/
def OperationallyAllowed
(s : State V) (profile : AgentProfile V) : AgentAction V → Prop
| AgentAction.createTask draft =>
IsResourceKind s draft.parentList ResourceKind.taskList ∧
Visible s profile.principal draft.parentList ∧
HasCapability s profile.principal draft.parentList Capability.createTask ∧
InitialCryptoMaterial draft ∧
AgentDelegationAllowed s profile draft

| AgentAction.replaceOwnTask task next =>
IsOwnTask s profile.principal task ∧
Visible s profile.principal task ∧
HasCapability s profile.principal task Capability.manageOwnTask ∧
next.keyEpoch = s.activeEpoch task

| AgentAction.deleteOwnTask task =>
IsOwnTask s profile.principal task ∧
Visible s profile.principal task ∧
HasCapability s profile.principal task Capability.manageOwnTask

| AgentAction.assignOwnTask task assignee =>
IsOwnTask s profile.principal task ∧
Visible s profile.principal task ∧
HasCapability s profile.principal task Capability.manageOwnTask ∧
(∃ kind, s.principals assignee = some kind)

| AgentAction.unassignOwnTask task assignee =>
IsOwnTask s profile.principal task ∧
Visible s profile.principal task ∧
HasCapability s profile.principal task Capability.manageOwnTask ∧
AssignedTo s assignee task

| AgentAction.markAssignedDone task =>
IsResourceKind s task ResourceKind.task ∧
Visible s profile.principal task ∧
AssignedTo s profile.principal task ∧
OpenTask s task

| AgentAction.appendAssignedNote task note =>
IsResourceKind s task ResourceKind.task ∧
Visible s profile.principal task ∧
AssignedTo s profile.principal task ∧
note.keyEpoch = s.activeEpoch task

| AgentAction.addAssignedAttachment task attachment =>
IsResourceKind s task ResourceKind.task ∧
Visible s profile.principal task ∧
AssignedTo s profile.principal task ∧
attachment.keyEpoch = s.activeEpoch task

| AgentAction.postComment draft =>
AgentCommentDraftAllowed s profile draft

| AgentAction.invokeTool tool _ policy =>
ToolReady s profile tool ∧
1 ≤ policy.maxAttempts ∧
1 ≤ policy.timeoutTicks

| AgentAction.retryTool callId =>
RetryEligible s profile callId

| AgentAction.noOp => True

/-- Autorizzazione normativa derivata dal prompt. -/
def NormativelyAllowed
(P : PromptSemantics V) (s : State V) (profile : AgentProfile V)
(action : AgentAction V) : Prop :=
match action with
| AgentAction.markAssignedDone _ => True
| AgentAction.appendAssignedNote _ _ => True
| AgentAction.addAssignedAttachment _ _ => True
| AgentAction.retryTool _ => True
| AgentAction.noOp =>
∃ prompt,
s.systemPrompts profile.principal = some prompt ∧
P.mayWait prompt s
| _ =>
∃ prompt,
s.systemPrompts profile.principal = some prompt ∧
P.serves prompt s action

/-- Un'azione è ammissibile se supera entrambi i livelli di controllo. -/
def Admissible
(P : PromptSemantics V) (s : State V) (profile : AgentProfile V)
(action : AgentAction V) : Prop :=
OperationallyAllowed s profile action ∧
NormativelyAllowed P s profile action

/-! ## 6. Frame conditions ed effetti esatti -/

/-- I principal e i relativi ruoli restano invariati. -/
def PrincipalsPreserved (s s' : State V) : Prop :=
∀ p, s'.principals p = s.principals p

/-- I prompt restano invariati. -/
def PromptsPreserved (s s' : State V) : Prop :=
∀ p, s'.systemPrompts p = s.systemPrompts p

/-- Permessi sulle risorse e sui tool restano logicamente equivalenti. -/
def PermissionsPreserved (s s' : State V) : Prop :=
(∀ p r cap, s'.permissions p r cap ↔ s.permissions p r cap) ∧
(∀ p tool, s'.toolPermission p tool ↔ s.toolPermission p tool)

/-- Configurazione di coordinamento invariata. -/
def CoordinationPreserved (s s' : State V) : Prop :=
s'.coordination = s.coordination
/-- Disponibilità runtime invariata. -/
def RuntimeAvailabilityPreserved (s s' : State V) : Prop :=
∀ tool, s'.runtimeAvailable tool ↔ s.runtimeAvailable tool

/-- Commenti invariati. -/
def CommentsPreserved (s s' : State V) : Prop :=
s'.comments = s.comments

/-- Audit tool invariato. -/
def ToolAuditPreserved (s s' : State V) : Prop :=
s'.toolAudit = s.toolAudit

/-- Record tool invariati. -/
def ToolCallsPreserved (s s' : State V) : Prop :=
∀ callId, s'.toolCalls callId = s.toolCalls callId

/-- Stato tool complessivamente invariato. -/
def ToolStatePreserved (s s' : State V) : Prop :=
ToolCallsPreserved s s' ∧ ToolAuditPreserved s s'

/-- Tutte le epoch restano invariate. -/
def EpochsPreserved (s s' : State V) : Prop :=
∀ r, s'.activeEpoch r = s.activeEpoch r

/-- Le epoch diverse dal target restano invariate. -/
def OtherEpochsUnchanged
(s s' : State V) (target : V.ResourceId) : Prop :=
∀ r, r ≠ target → s'.activeEpoch r = s.activeEpoch r

/-- Le risorse diverse dal target restano invariate. -/
def OtherResourcesUnchanged
(s s' : State V) (target : V.ResourceId) : Prop :=
∀ r, r ≠ target → s'.resources r = s.resources r

/-- Le task diverse dal target restano invariate. -/
def OtherTasksUnchanged
(s s' : State V) (target : V.ResourceId) : Prop :=
∀ task, task ≠ target → s'.tasks task = s.tasks task

/-- Le chiamate tool diverse dal target restano invariate. -/
def OtherToolCallsUnchanged
(s s' : State V) (target : V.ToolCallId) : Prop :=
∀ callId, callId ≠ target → s'.toolCalls callId = s.toolCalls callId

/-- Tutte le risorse restano invariate. -/
def AllResourcesUnchanged (s s' : State V) : Prop :=
∀ r, s'.resources r = s.resources r

/-- Tutte le task restano invariate. -/
def AllTasksUnchanged (s s' : State V) : Prop :=
∀ task, s'.tasks task = s.tasks task
/-- Stato applicativo non-tool invariato. -/
def CoreDomainPreserved (s s' : State V) : Prop :=
PrincipalsPreserved s s' ∧
AllResourcesUnchanged s s' ∧
AllTasksUnchanged s s' ∧
EpochsPreserved s s' ∧
CommentsPreserved s s'

/-- Provenienza strutturale invariata fra due versioni della risorsa. -/
def SameResourceProvenance
(before after : ResourceMeta V) : Prop :=
after.id = before.id ∧
after.projectId = before.projectId ∧
after.kind = before.kind ∧
after.parent = before.parent ∧
after.creator = before.creator ∧
after.sourceTask = before.sourceTask ∧
after.agentDelegationDepth = before.agentDelegationDepth

/-- Aggiornamento ordinario con versione crescente. -/
def SameResourceIdentityExceptVersion
(before after : ResourceMeta V) : Prop :=
SameResourceProvenance before after ∧
after.deleted = before.deleted ∧
before.version < after.version

/-- Cancellazione logica tramite tombstone. -/
def TombstoneAdvanced
(before after : ResourceMeta V) : Prop :=
SameResourceProvenance before after ∧
before.deleted = false ∧
after.deleted = true ∧
before.version < after.version

/-- Una task non può regredire da done a open . -/
def StatusProgresses (before after : TaskStatus) : Prop :=
match before, after with
| TaskStatus.open, _ => True
| TaskStatus.done, TaskStatus.done => True
| TaskStatus.done, TaskStatus.open => False

/-- Aggiornamento dei soli metadati del target. -/
def UpdatedResourceMeta
(s s' : State V) (target : V.ResourceId) : Prop :=
∃ before after,
s.resources target = some before ∧
s'.resources target = some after ∧
SameResourceIdentityExceptVersion before after ∧
OtherResourcesUnchanged s s' target
/-- Tombstone del solo target. -/
def DeletedResourceMeta
(s s' : State V) (target : V.ResourceId) : Prop :=
∃ before after,
s.resources target = some before ∧
s'.resources target = some after ∧
TombstoneAdvanced before after ∧
OtherResourcesUnchanged s s' target

/-- Effetto esatto della creazione di una task con identificativo fresco. -/
def CreateTaskEffect
(s s' : State V) (profile : AgentProfile V) (draft : NewTask V) : Prop :=
∃ newId _parentMeta _newMeta,
s.resources draft.parentList = some _parentMeta ∧
s.resources newId = none ∧
s.tasks newId = none ∧
_newMeta.id = newId ∧
_newMeta.projectId = _parentMeta.projectId ∧
_newMeta.kind = ResourceKind.task ∧
_newMeta.parent = some draft.parentList ∧
_newMeta.creator = profile.principal ∧
_newMeta.version = 1 ∧
_newMeta.deleted = false ∧
_newMeta.sourceTask = draft.sourceTask ∧
(match draft.sourceTask with
| none => _newMeta.agentDelegationDepth = 0
| some source =>
∃ _sourceMeta,
s.resources source = some _sourceMeta ∧
_newMeta.agentDelegationDepth = _sourceMeta.agentDelegationDepth + 1 ∧
_newMeta.agentDelegationDepth ≤
s.coordination.maxAgentTaskDelegationDepth) ∧
s'.resources newId = some _newMeta ∧
s'.tasks newId = some draft.data ∧
s'.activeEpoch newId = 1 ∧
OtherResourcesUnchanged s s' newId ∧
OtherTasksUnchanged s s' newId ∧
OtherEpochsUnchanged s s' newId ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'

/-- Sostituzione di una task propria senza cancellare cronologia o riaprirla. -/
def ReplaceOwnTaskEffect
(s s' : State V) (task : V.ResourceId) (next : TaskData V) : Prop :=
∃ before,
s.tasks task = some before ∧
next.notes = before.notes ∧
next.attachments = before.attachments ∧
StatusProgresses before.status next.status ∧
s'.tasks task = some next ∧
UpdatedResourceMeta s s' task ∧
OtherTasksUnchanged s s' task ∧
EpochsPreserved s s' ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'

/-- Cancellazione logica di una task propria. -/
def DeleteOwnTaskEffect
(s s' : State V) (task : V.ResourceId) : Prop :=
DeletedResourceMeta s s' task ∧
AllTasksUnchanged s s' ∧
EpochsPreserved s s' ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'

/-- Aggiunta puntuale di un assegnatario. -/
def AssignOwnTaskEffect
(s s' : State V) (task : V.ResourceId) (assignee : V.PrincipalId) : Prop :=
∃ before after,
s.tasks task = some before ∧
s'.tasks task = some after ∧
after.status = before.status ∧
(∀ p, after.assignees p ↔ before.assignees p ∨ p = assignee) ∧
after.payload = before.payload ∧
after.keyEpoch = before.keyEpoch ∧
after.notes = before.notes ∧
after.attachments = before.attachments ∧
UpdatedResourceMeta s s' task ∧
OtherTasksUnchanged s s' task ∧
EpochsPreserved s s' ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'

/-- Rimozione puntuale di un assegnatario. -/
def UnassignOwnTaskEffect
(s s' : State V) (task : V.ResourceId) (assignee : V.PrincipalId) : Prop :=
∃ before after,
s.tasks task = some before ∧
s'.tasks task = some after ∧
after.status = before.status ∧
(∀ p, after.assignees p ↔ before.assignees p ∧ p ≠ assignee) ∧
after.payload = before.payload ∧
after.keyEpoch = before.keyEpoch ∧
after.notes = before.notes ∧
after.attachments = before.attachments ∧
UpdatedResourceMeta s s' task ∧
OtherTasksUnchanged s s' task ∧
EpochsPreserved s s' ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'

/-- Completamento di una task assegnata. -/
def MarkDoneEffect
(s s' : State V) (task : V.ResourceId) : Prop :=
∃ before after,
s.tasks task = some before ∧
s'.tasks task = some after ∧
before.status = TaskStatus.open ∧
after.status = TaskStatus.done ∧
(∀ p, after.assignees p ↔ before.assignees p) ∧
after.payload = before.payload ∧
after.keyEpoch = before.keyEpoch ∧
after.notes = before.notes ∧
after.attachments = before.attachments ∧
UpdatedResourceMeta s s' task ∧
OtherTasksUnchanged s s' task ∧
EpochsPreserved s s' ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'

/-- Aggiunta append-only di una nota. -/
def AppendNoteEffect
(s s' : State V) (task : V.ResourceId) (note : NoteEntry V) : Prop :=
∃ before after,
s.tasks task = some before ∧
s'.tasks task = some after ∧
after.status = before.status ∧
(∀ p, after.assignees p ↔ before.assignees p) ∧
after.payload = before.payload ∧
after.keyEpoch = before.keyEpoch ∧
after.notes = before.notes ++ [note] ∧
after.attachments = before.attachments ∧
UpdatedResourceMeta s s' task ∧
OtherTasksUnchanged s s' task ∧
EpochsPreserved s s' ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'
/-- Aggiunta append-only di un allegato. -/
def AddAttachmentEffect
(s s' : State V) (task : V.ResourceId)
(attachment : AttachmentRef V) : Prop :=
∃ before after,
s.tasks task = some before ∧
s'.tasks task = some after ∧
after.status = before.status ∧
(∀ p, after.assignees p ↔ before.assignees p) ∧
after.payload = before.payload ∧
after.keyEpoch = before.keyEpoch ∧
after.notes = before.notes ∧
after.attachments = before.attachments ++ [attachment] ∧
UpdatedResourceMeta s s' task ∧
OtherTasksUnchanged s s' task ∧
EpochsPreserved s s' ∧
PrincipalsPreserved s s' ∧
CommentsPreserved s s' ∧
ToolStatePreserved s s'

/-- R4.4: effetto di un commento agent-to-agent con id fresco e profondità derivata. -/
def PostCommentEffect
(s s' : State V) (profile : AgentProfile V) (draft : NewComment V) : Prop :=
∃ newId newComment,
(∀ existing, existing ∈ s.comments → existing.id ≠ newId) ∧
newComment.id = newId ∧
newComment.author = profile.principal ∧
newComment.recipient = draft.recipient ∧
newComment.target = draft.target ∧
newComment.parent = draft.parent ∧
newComment.payload = draft.payload ∧
newComment.keyEpoch = draft.keyEpoch ∧
(match draft.parent with
| none => newComment.agentDepth = 1
| some parentId =>
∃ parentComment,
HasCommentId s parentId parentComment ∧
newComment.agentDepth = parentComment.agentDepth + 1) ∧
s'.comments = s.comments ++ [newComment] ∧
PrincipalsPreserved s s' ∧
AllResourcesUnchanged s s' ∧
AllTasksUnchanged s s' ∧
EpochsPreserved s s' ∧
ToolStatePreserved s s'

/-- R4.1: apertura di una nuova chiamata tool e append dell'audit requested . -/
def InvokeToolEffect
(s s' : State V) (profile : AgentProfile V)
(tool : V.Tool) (input : V.ToolInput) (policy : ToolRetryPolicy) : Prop :=
∃ callId call entry,
s.toolCalls callId = none ∧
call.id = callId ∧
call.owner = profile.principal ∧
call.tool = tool ∧
call.input = input ∧
call.attempt = 1 ∧
call.maxAttempts = policy.maxAttempts ∧
call.timeoutTicks = policy.timeoutTicks ∧
call.status = ToolCallStatus.pending ∧
call.output = none ∧
call.failure = none ∧
entry.callId = callId ∧
entry.owner = profile.principal ∧
entry.tool = tool ∧
entry.attempt = 1 ∧
entry.kind = ToolAuditKind.requested ∧
s'.toolCalls callId = some call ∧
OtherToolCallsUnchanged s s' callId ∧
s'.toolAudit = s.toolAudit ++ [entry] ∧
CoreDomainPreserved s s'

/-- R4.3: retry della stessa call, con incremento del tentativo e audit. -/
def RetryToolEffect
(s s' : State V) (profile : AgentProfile V) (callId : V.ToolCallId) : Prop :=
∃ before after entry,
s.toolCalls callId = some before ∧
before.owner = profile.principal ∧
(before.status = ToolCallStatus.failed ∨
before.status = ToolCallStatus.timedOut) ∧
before.attempt < before.maxAttempts ∧
after.id = before.id ∧
after.owner = before.owner ∧
after.tool = before.tool ∧
after.input = before.input ∧
after.attempt = before.attempt + 1 ∧
after.maxAttempts = before.maxAttempts ∧
after.timeoutTicks = before.timeoutTicks ∧
after.status = ToolCallStatus.pending ∧
after.output = none ∧
after.failure = none ∧
entry.callId = callId ∧
entry.owner = before.owner ∧
entry.tool = before.tool ∧
entry.attempt = after.attempt ∧
entry.kind = ToolAuditKind.retryStarted ∧
s'.toolCalls callId = some after ∧
OtherToolCallsUnchanged s s' callId ∧
s'.toolAudit = s.toolAudit ++ [entry] ∧
CoreDomainPreserved s s'

/-- Dispatcher centrale degli effetti delle azioni agentiche. -/
def ActionEffect
(s s' : State V) (profile : AgentProfile V) : AgentAction V → Prop
| AgentAction.createTask draft =>
CreateTaskEffect s s' profile draft
| AgentAction.replaceOwnTask task next =>
ReplaceOwnTaskEffect s s' task next
| AgentAction.deleteOwnTask task =>
DeleteOwnTaskEffect s s' task
| AgentAction.assignOwnTask task assignee =>
AssignOwnTaskEffect s s' task assignee
| AgentAction.unassignOwnTask task assignee =>
UnassignOwnTaskEffect s s' task assignee
| AgentAction.markAssignedDone task =>
MarkDoneEffect s s' task
| AgentAction.appendAssignedNote task note =>
AppendNoteEffect s s' task note
| AgentAction.addAssignedAttachment task attachment =>
AddAttachmentEffect s s' task attachment
| AgentAction.postComment draft =>
PostCommentEffect s s' profile draft
| AgentAction.invokeTool tool input policy =>
InvokeToolEffect s s' profile tool input policy
| AgentAction.retryTool callId =>
RetryToolEffect s s' profile callId
| AgentAction.noOp =>
s' = s

/-- Certificato di conformità di una transizione atomica dell'agente. -/
structure LegalAgentStep
(B : ApiBoundary V)
(P : PromptSemantics V)
(M : TransitionSystem V)
(profile : AgentProfile V)
(s : State V)
(action : AgentAction V)
(s' : State V) : Prop where
wellFormedBefore : WellFormedState s
authenticated : B.actorOf profile.session = some profile.principal
agentRole : HasKind s profile.principal PrincipalKind.agent
admissible : Admissible P s profile action
implemented : M.agentStep s profile.session action s'
promptFrame : PromptsPreserved s s'
permissionFrame : PermissionsPreserved s s'
runtimeFrame : RuntimeAvailabilityPreserved s s'
coordinationFrame : CoordinationPreserved s s'
exactEffect : ActionEffect s s' profile action
wellFormedAfter : WellFormedState s'

/-! ### R4.1 — effetti degli eventi tool e audit -/

/-- Completamento causale della call indicata. -/
def ToolCompletionEffect
(s s' : State V) (callId : V.ToolCallId) (output : V.ToolOutput) : Prop :=
∃ before after entry,
s.toolCalls callId = some before ∧
before.status = ToolCallStatus.pending ∧
after.id = before.id ∧
after.owner = before.owner ∧
after.tool = before.tool ∧
after.input = before.input ∧
after.attempt = before.attempt ∧
after.maxAttempts = before.maxAttempts ∧
after.timeoutTicks = before.timeoutTicks ∧
after.status = ToolCallStatus.succeeded ∧
after.output = some output ∧
after.failure = none ∧
entry.callId = callId ∧
entry.owner = before.owner ∧
entry.tool = before.tool ∧
entry.attempt = before.attempt ∧
entry.kind = ToolAuditKind.completed ∧
s'.toolCalls callId = some after ∧
OtherToolCallsUnchanged s s' callId ∧
s'.toolAudit = s.toolAudit ++ [entry] ∧
CoreDomainPreserved s s' ∧
PromptsPreserved s s' ∧
PermissionsPreserved s s' ∧
RuntimeAvailabilityPreserved s s' ∧
CoordinationPreserved s s'

/-- Fallimento causale della call indicata. -/
def ToolFailureEffect
(s s' : State V) (callId : V.ToolCallId) (failure : V.ToolError) : Prop :=
∃ before after entry,
s.toolCalls callId = some before ∧
before.status = ToolCallStatus.pending ∧
after.id = before.id ∧
after.owner = before.owner ∧
after.tool = before.tool ∧
after.input = before.input ∧
after.attempt = before.attempt ∧
after.maxAttempts = before.maxAttempts ∧
after.timeoutTicks = before.timeoutTicks ∧
after.status = ToolCallStatus.failed ∧
after.output = none ∧
after.failure = some failure ∧
entry.callId = callId ∧
entry.owner = before.owner ∧
entry.tool = before.tool ∧
entry.attempt = before.attempt ∧
entry.kind = ToolAuditKind.failed ∧
s'.toolCalls callId = some after ∧
OtherToolCallsUnchanged s s' callId ∧
s'.toolAudit = s.toolAudit ++ [entry] ∧
CoreDomainPreserved s s' ∧
PromptsPreserved s s' ∧
PermissionsPreserved s s' ∧
RuntimeAvailabilityPreserved s s' ∧
CoordinationPreserved s s'

/-- Timeout causale della call indicata. -/
def ToolTimeoutEffect
(s s' : State V) (callId : V.ToolCallId) : Prop :=
∃ before after entry,
s.toolCalls callId = some before ∧
before.status = ToolCallStatus.pending ∧
after.id = before.id ∧
after.owner = before.owner ∧
after.tool = before.tool ∧
after.input = before.input ∧
after.attempt = before.attempt ∧
after.maxAttempts = before.maxAttempts ∧
after.timeoutTicks = before.timeoutTicks ∧
after.status = ToolCallStatus.timedOut ∧
after.output = none ∧
entry.callId = callId ∧
entry.owner = before.owner ∧
entry.tool = before.tool ∧
entry.attempt = before.attempt ∧
entry.kind = ToolAuditKind.timedOut ∧
s'.toolCalls callId = some after ∧
OtherToolCallsUnchanged s s' callId ∧
s'.toolAudit = s.toolAudit ++ [entry] ∧
CoreDomainPreserved s s' ∧
PromptsPreserved s s' ∧
PermissionsPreserved s s' ∧
RuntimeAvailabilityPreserved s s' ∧
CoordinationPreserved s s'
/-- Un cambio di disponibilità modifica soltanto il tool indicato. -/
def RuntimeAvailabilityEffect
(s s' : State V) (tool : V.Tool) (available : Bool) : Prop :=
(s'.runtimeAvailable tool ↔ available = true) ∧
(∀ other, other ≠ tool →
(s'.runtimeAvailable other ↔ s.runtimeAvailable other)) ∧
CoreDomainPreserved s s' ∧
ToolStatePreserved s s' ∧
PromptsPreserved s s' ∧
PermissionsPreserved s s' ∧
CoordinationPreserved s s'

/-- Effetto richiesto a un tick-evento. Le notifiche di dominio non mutano lo stato. -/
def EventEffect (s s' : State V) : Event V → Prop
| Event.resourceUpdated _ _ => s' = s
| Event.commentPosted comment => comment ∈ s.comments ∧ s' = s
| Event.toolCompleted callId output => ToolCompletionEffect s s' callId output
| Event.toolFailed callId failure => ToolFailureEffect s s' callId failure
| Event.toolTimedOut callId => ToolTimeoutEffect s s' callId
| Event.runtimeAvailabilityChanged tool available =>
RuntimeAvailabilityEffect s s' tool available

/-- Tick-evento implementato dal runtime e conforme all'effetto formale. -/
def LegalRuntimeEventStep
(M : TransitionSystem V)
(s : State V) (event : Event V) (s' : State V) : Prop :=
WellFormedState s ∧
M.runtimeStep s event s' ∧
EventEffect s s' event ∧
WellFormedState s'

/-! ## 7. R4.3/R4.4 — run multi-agente, fairness, retry e causalità -/

/-- Traccia temporale discreta condivisa da più agenti e dagli umani. -/
structure Run (V : Vocabulary) where
state : Nat → State V
event : Nat → Option (Event V)
move : Nat → Option (Move V)

/-- Esiste un tick non precedente a n nel quale vale Q . -/
def EventuallyAfter (n : Nat) (Q : Nat → Prop) : Prop :=
∃ m, n ≤ m ∧ Q m

/-- Esiste un tick entro un bound finito. -/
def EventuallyWithin (n delta : Nat) (Q : Nat → Prop) : Prop :=
∃ m, n ≤ m ∧ m ≤ n + delta ∧ Q m

/-- Una mossa umana può essere eseguita soltanto da un principal umano. -/
def LegalHumanStep
(M : TransitionSystem V)
(s : State V) (actor : V.PrincipalId) (humanMove : V.HumanMove)
(s' : State V) : Prop :=
WellFormedState s ∧
(∃ kind, HasKind s actor kind ∧ IsHumanKind kind) ∧
M.humanStep s actor humanMove s' ∧
ToolStatePreserved s s' ∧
WellFormedState s'

/-- Semantica interleaved di un singolo tick. -/
def ValidTick
(B : ApiBoundary V)
(P : PromptSemantics V)
(M : TransitionSystem V)
(directory : AgentDirectory V)
(run : Run V)
(n : Nat) : Prop :=
match run.event n, run.move n with
| none, none =>
run.state (n + 1) = run.state n
| some event, none =>
LegalRuntimeEventStep M (run.state n) event (run.state (n + 1))
| none, some (Move.agentMove actor action) =>
  ∃ profile,
    directory actor = some profile ∧
    profile.principal = actor ∧
    LegalAgentStep B P M profile
      (run.state n) action (run.state (n + 1))
| none, some (Move.humanMove actor humanMove) =>
  LegalHumanStep M
    (run.state n) actor humanMove (run.state (n + 1))
| some _, some _ => False

/-- Una run valida è composta soltanto da tick giustificati. -/
def ValidRun
(B : ApiBoundary V)
(P : PromptSemantics V)
(M : TransitionSystem V)
(directory : AgentDirectory V)
(run : Run V) : Prop :=
(∀ n, WellFormedState (run.state n)) ∧
(∀ n, ValidTick B P M directory run n)

/-- Azione di un determinato agente osservata al tick n . -/
def AgentActionAt
(run : Run V) (actor : V.PrincipalId)
(action : AgentAction V) (n : Nat) : Prop :=
run.move n = some (Move.agentMove actor action)

/-- Un agente è schedulato al tick n . -/
def AgentScheduledAt
(run : Run V) (actor : V.PrincipalId) (n : Nat) : Prop :=
∃ action, AgentActionAt run actor action n

/-- Esiste almeno un'azione non vuota ammissibile per l'agente. -/
def AgentEnabledAt
(P : PromptSemantics V)
(directory : AgentDirectory V)
(run : Run V)
(actor : V.PrincipalId)
(n : Nat) : Prop :=
∃ profile action,
directory actor = some profile ∧
profile.principal = actor ∧
action ≠ AgentAction.noOp ∧
Admissible P (run.state n) profile action

/-- Nessuna schedulazione dell'agente è avvenuta prima di m , a partire da n . -/
def NoAgentScheduleBefore
(run : Run V) (actor : V.PrincipalId) (n m : Nat) : Prop :=
∀ k, n ≤ k → k < m → ¬ AgentScheduledAt run actor k

/-- L'agente rimane abilitato finché non riceve la prima schedulazione. -/
def AgentEnabledUntilScheduled
(P : PromptSemantics V)
(directory : AgentDirectory V)
(run : Run V)
(actor : V.PrincipalId)
(n : Nat) : Prop :=
∀ m,
n≤m→
NoAgentScheduleBefore run actor n m →
AgentEnabledAt P directory run actor m

/-- Weak fairness dello scheduler degli agenti, formulata fino alla prima occorrenza. -/
def AgentSchedulerWeakFairness
(P : PromptSemantics V)
(directory : AgentDirectory V)
(run : Run V) : Prop :=
∀ actor n,
AgentEnabledUntilScheduled P directory run actor n →
EventuallyAfter n (fun m => AgentScheduledAt run actor m)

/-- Evento terminale della specifica call. -/
def ToolTerminalEventAt
(run : Run V) (callId : V.ToolCallId) (n : Nat) : Prop :=
(∃ output, run.event n = some (Event.toolCompleted callId output)) ∨
(∃ failure, run.event n = some (Event.toolFailed callId failure)) ∨
run.event n = some (Event.toolTimedOut callId)
/-- Call pendente con runtime disponibile al tick indicato. -/
def PendingWithRuntimeAt
(run : Run V) (callId : V.ToolCallId) (n : Nat) : Prop :=
∃ call,
(run.state n).toolCalls callId = some call ∧
call.status = ToolCallStatus.pending ∧
(run.state n).runtimeAvailable call.tool

/-- Nessun evento terminale della call è avvenuto prima di m . -/
def NoToolTerminalBefore
(run : Run V) (callId : V.ToolCallId) (n m : Nat) : Prop :=
∀ k, n ≤ k → k < m → ¬ ToolTerminalEventAt run callId k

/-- La call resta pendente e servibile finché non arriva il primo evento terminale. -/
def RuntimeEnabledUntilTerminal
(run : Run V) (callId : V.ToolCallId) (n : Nat) : Prop :=
∀ m,
n≤m→
NoToolTerminalBefore run callId n m →
PendingWithRuntimeAt run callId m

/-- Weak fairness del runtime, formulata fino alla prima terminazione. -/
def RuntimeWeakFairness (run : Run V) : Prop :=
∀ callId n,
RuntimeEnabledUntilTerminal run callId n →
EventuallyAfter n (fun m => ToolTerminalEventAt run callId m)

/-- Timeout bounded: una call pendente raggiunge un evento terminale entro il bound
persistito. -/
def ToolTimeoutGuarantee (run : Run V) : Prop :=
∀ n callId call,
(run.state n).toolCalls callId = some call →
call.status = ToolCallStatus.pending →
EventuallyWithin n call.timeoutTicks
(fun m => ToolTerminalEventAt run callId m)

/-- Osservazione di un retry dello specifico agente. -/
def RetryActionAt
(run : Run V) (profile : AgentProfile V)
(callId : V.ToolCallId) (n : Nat) : Prop :=
AgentActionAt run profile.principal (AgentAction.retryTool callId) n

/-- Nessun retry della call è avvenuto prima di m . -/
def NoRetryBefore
(run : Run V) (profile : AgentProfile V)
(callId : V.ToolCallId) (n m : Nat) : Prop :=
∀ k, n ≤ k → k < m → ¬ RetryActionAt run profile callId k

/-- Se un retry resta abilitato fino alla prima esecuzione, prima o poi viene eseguito. -/
def RetryLiveness
(directory : AgentDirectory V)
(run : Run V) : Prop :=
∀ n actor profile callId,
directory actor = some profile →
profile.principal = actor →
RetryEligible (run.state n) profile callId →
(∀ m, n ≤ m →
NoRetryBefore run profile callId n m →
RetryEligible (run.state m) profile callId) →
EventuallyAfter n (fun m => RetryActionAt run profile callId m)

/-- Ogni evento attivante riceve una risposta pertinente e non vuota. -/
def TriggerResponsiveness
(R : ResponseSemantics V)
(profile : AgentProfile V)
(run : Run V) : Prop :=
∀ n event,
run.event n = some event →
Activates (run.state n) profile event →
EventuallyAfter n (fun m =>
∃ action,
AgentActionAt run profile.principal action m ∧
action ≠ AgentAction.noOp ∧
R.respondsTo (run.state n) event action)

/-- Una risposta dell'agente a un evento nato al tick sourceTick . -/
def RespondsAt
(R : ResponseSemantics V)
(profile : AgentProfile V)
(run : Run V)
(sourceTick responseTick : Nat) : Prop :=
∃ event action,
run.event sourceTick = some event ∧
AgentActionAt run profile.principal action responseTick ∧
action ≠ AgentAction.noOp ∧
R.respondsTo (run.state sourceTick) event action

/-- Il commento high ha priorità maggiore del commento low . -/
def HigherPriorityComment
(s : State V) (high low : Comment V) : Prop :=
∃ highKind lowKind,
s.principals high.author = some highKind ∧
s.principals low.author = some lowKind ∧
CommentPriority highKind > CommentPriority lowKind

/-
R4.4: un commento agentico non può sorpassare una correzione umana ancora
pendente. La stessa relazione ordina anche amministratore > utente.
-/
def CommentPriorityDiscipline
(R : ResponseSemantics V)
(profile : AgentProfile V)
(run : Run V) : Prop :=
∀ highTick lowTick high low,
highTick ≤ lowTick →
run.event highTick = some (Event.commentPosted high) →
run.event lowTick = some (Event.commentPosted low) →
high.recipient = profile.principal →
low.recipient = profile.principal →
HigherPriorityComment (run.state highTick) high low →
Activates (run.state highTick) profile (Event.commentPosted high) →
Activates (run.state lowTick) profile (Event.commentPosted low) →
(∀ k, highTick ≤ k → k < lowTick →
¬ RespondsAt R profile run highTick k) →
∀ m,
lowTick ≤ m →
RespondsAt R profile run lowTick m →
∃ k, highTick ≤ k ∧ k ≤ m ∧ RespondsAt R profile run highTick k

/-- Azione agentica che può essere causa diretta del passaggio di una task a done . -/
def AgentActionCompletesTask
(action : AgentAction V) (task : V.ResourceId) : Prop :=
action = AgentAction.markAssignedDone task ∨
∃ next,
action = AgentAction.replaceOwnTask task next ∧
next.status = TaskStatus.done

/-
R4.4: ogni transizione osservata da open a done deve avere un attore nel tick:
un agente con un'azione di completamento oppure una mossa umana esplicita.
-/
def TaskCompletionCausality (run : Run V) : Prop :=
∀ n task,
OpenTask (run.state n) task →
DoneTask (run.state (n + 1)) task →
(∃ actor action,
run.move n = some (Move.agentMove actor action) ∧
AgentActionCompletesTask action task) ∨
(∃ actor humanMove,
run.move n = some (Move.humanMove actor humanMove))

/-- Finché la task non è completata, visibilità e assegnazione restano attive. -/
def AssignedTaskRemainsInScopeUntilDone
(profile : AgentProfile V)
(run : Run V)
(n : Nat)
(task : V.ResourceId) : Prop :=
∀ m,
n≤m→
¬ DoneTask (run.state m) task →
Visible (run.state m) profile.principal task ∧
AssignedTo (run.state m) profile.principal task

/-- Liveness risultante delle task assegnate. -/
def AssignedTaskLiveness
(profile : AgentProfile V)
(run : Run V) : Prop :=
∀ n task,
Visible (run.state n) profile.principal task →
AssignedTo (run.state n) profile.principal task →
OpenTask (run.state n) task →
AssignedTaskRemainsInScopeUntilDone profile run n task →
EventuallyAfter n (fun m => DoneTask (run.state m) task)

/-- L'obbligo resta attivo fino al discharge. -/
def PromptObligationRemainsActiveUntilDischarged
(P : PromptSemantics V)
(profile : AgentProfile V)
(run : Run V)
(n : Nat)
(prompt : V.SystemPrompt)
(obligation : V.ObligationId) : Prop :=
∀ m,
n≤m→
¬ P.discharged prompt (run.state m) obligation →
(run.state m).systemPrompts profile.principal = some prompt ∧
P.obligation prompt (run.state m) obligation

/-- Ogni obbligo persistente del prompt viene prima o poi soddisfatto. -/
def PromptObligationLiveness
(P : PromptSemantics V)
(profile : AgentProfile V)
(run : Run V) : Prop :=
∀ n prompt obligation,
(run.state n).systemPrompts profile.principal = some prompt →
P.obligation prompt (run.state n) obligation →
PromptObligationRemainsActiveUntilDischarged
P profile run n prompt obligation →
EventuallyAfter n (fun m =>
P.discharged prompt (run.state m) obligation)

/-- Ogni CommentId viene notificato al massimo una volta nella run. -/
def UniqueCommentNotifications (run : Run V) : Prop :=
∀ n m first second,
run.event n = some (Event.commentPosted first) →
run.event m = some (Event.commentPosted second) →
first.id = second.id →
n=m

/-- Contratto temporale complessivo di una run multi-agente. -/
def ResponsibleRun
(B : ApiBoundary V)
(P : PromptSemantics V)
(M : TransitionSystem V)
(R : ResponseSemantics V)
(directory : AgentDirectory V)
(profile : AgentProfile V)
(run : Run V) : Prop :=
ValidRun B P M directory run ∧
AgentSchedulerWeakFairness P directory run ∧
RuntimeWeakFairness run ∧
ToolTimeoutGuarantee run ∧
RetryLiveness directory run ∧
TriggerResponsiveness R profile run ∧
CommentPriorityDiscipline R profile run ∧
TaskCompletionCausality run ∧
UniqueCommentNotifications run ∧
AssignedTaskLiveness profile run ∧
PromptObligationLiveness P profile run

/-! ## 8. Gioco collaborativo e preferenze -/

/-- Gioco cooperativo con mosse attribuite ai singoli principal. -/
structure CollaborativeGame (V : Vocabulary) where
players : V.PrincipalId → Prop
roleOf : V.PrincipalId → Option PrincipalKind
legalMove : State V → Move V → Prop
transition : State V → Move V → State V → Prop
teamObjective : Run V → Prop

/-- Interventi correttivi; livelli inferiori hanno peso subordinato. -/
structure CorrectionProfile where
promptRevisions : Nat
adminComments : Nat
userComments : Nat
agentComments : Nat
deriving DecidableEq, Repr

/-- Ordine lessicografico: prompt > admin > user > agent. -/
def BetterCorrection (x y : CorrectionProfile) : Prop :=
x.promptRevisions < y.promptRevisions ∨
(x.promptRevisions = y.promptRevisions ∧
(x.adminComments < y.adminComments ∨
(x.adminComments = y.adminComments ∧
(x.userComments < y.userComments ∨
(x.userComments = y.userComments ∧
x.agentComments < y.agentComments)))))

/-! ### R4.5 — derivazione verificabile di CorrectionProfile dalla run -/

/-- Conta i tick che soddisfano un predicato booleano in [0, horizon) . -/
def CountTicks (predicate : Nat → Bool) : Nat → Nat
| 0 => 0
| horizon + 1 =>
CountTicks predicate horizon +
if predicate horizon then 1 else 0

/-- Revisione del prompt dell'agente fra n e n+1 . -/
def PromptRevisionAt
[DecidableEq V.SystemPrompt]
(run : Run V) (profile : AgentProfile V) (n : Nat) : Bool :=
decide
((run.state n).systemPrompts profile.principal ≠
(run.state (n + 1)).systemPrompts profile.principal)

/-- Commento correttivo del ruolo indicato, diretto all'agente. -/
def CommentCorrectionAt
[DecidableEq V.PrincipalId]
(run : Run V) (profile : AgentProfile V)
(kind : PrincipalKind) (n : Nat) : Bool :=
match run.event n with
| some (Event.commentPosted comment) =>
if comment.recipient = profile.principal then
match (run.state n).principals comment.author with
| some authorKind => decide (authorKind = kind)
| none => false
else false
| _ => false

/-- Profilo di correzione calcolato esclusivamente dalla run osservata. -/
def CorrectionProfileFromRun
[DecidableEq V.SystemPrompt]
[DecidableEq V.PrincipalId]
(run : Run V) (profile : AgentProfile V) (horizon : Nat) : CorrectionProfile :=
{
  promptRevisions := CountTicks (PromptRevisionAt run profile) horizon,
  adminComments := CountTicks
    (CommentCorrectionAt run profile PrincipalKind.administrator) horizon,
  userComments := CountTicks
    (CommentCorrectionAt run profile PrincipalKind.user) horizon,
  agentComments := CountTicks
    (CommentCorrectionAt run profile PrincipalKind.agent) horizon
}
/-- La componente prompt è per definizione il conteggio verificabile della run. -/
theorem correction_profile_prompt_is_derived
[DecidableEq V.SystemPrompt]
[DecidableEq V.PrincipalId]
(run : Run V) (profile : AgentProfile V) (horizon : Nat) :
(CorrectionProfileFromRun run profile horizon).promptRevisions =
CountTicks (PromptRevisionAt run profile) horizon := by
rfl

/-- La componente dei commenti agentici è subordinata ma anch'essa derivata. -/
theorem correction_profile_agent_comments_are_derived
[DecidableEq V.SystemPrompt]
[DecidableEq V.PrincipalId]
(run : Run V) (profile : AgentProfile V) (horizon : Nat) :
(CorrectionProfileFromRun run profile horizon).agentComments =
CountTicks
(CommentCorrectionAt run profile PrincipalKind.agent) horizon := by
rfl

/-- Esito di una run, con correzioni non arbitrabili dall'evaluator. -/
structure Outcome where
safetySatisfied : Prop
responsibilitySatisfied : Prop
teamObjectiveSatisfied : Prop
corrections : CorrectionProfile

/-- Un esito supera le barriere non negoziabili. -/
def AcceptableOutcome (outcome : Outcome) : Prop :=
outcome.safetySatisfied ∧ outcome.responsibilitySatisfied

/-- Preferenza collaborativa dell'agente. -/
def AgentPrefers (x y : Outcome) : Prop :=
(AcceptableOutcome x ∧ ¬ AcceptableOutcome y) ∨
(AcceptableOutcome x ∧ AcceptableOutcome y ∧
((x.teamObjectiveSatisfied ∧ ¬ y.teamObjectiveSatisfied) ∨
((x.teamObjectiveSatisfied ↔ y.teamObjectiveSatisfied) ∧
BetterCorrection x.corrections y.corrections)))

/-- Elemento della storia finita osservata da una strategia. -/
structure HistoryEntry (V : Vocabulary) where
state : State V
event : Option (Event V)
move : Option (Move V)

/-- Storia finita osservata prima di una decisione. -/
abbrev History (V : Vocabulary) := List (HistoryEntry V)

/-- Strategia locale di un singolo agente. -/
structure AgentStrategy
(P : PromptSemantics V)
(profile : AgentProfile V) where
choose : History V → State V → AgentAction V
admissible :
∀ history state,
Admissible P state profile (choose history state)

/-
Valutazione di strategia basata su run osservabili. L'evaluator sceglie la run,
il suo orizzonte e le proprietà di safety/team; le correzioni vengono calcolate.
-/
structure StrategyEvaluation
(P : PromptSemantics V)
(profile : AgentProfile V) where
runOf : AgentStrategy P profile → Run V
horizonOf : AgentStrategy P profile → Nat
safetyOf : Run V → Prop
teamObjective : Run V → Prop

/-- Outcome derivato dalla run della strategia. -/
def OutcomeOfStrategy
[DecidableEq V.SystemPrompt]
[DecidableEq V.PrincipalId]
(B : ApiBoundary V)
(P : PromptSemantics V)
(M : TransitionSystem V)
(R : ResponseSemantics V)
(directory : AgentDirectory V)
(profile : AgentProfile V)
(evaluation : StrategyEvaluation P profile)
(strategy : AgentStrategy P profile) : Outcome :=
let run := evaluation.runOf strategy
{
safetySatisfied := evaluation.safetyOf run
responsibilitySatisfied :=
ResponsibleRun B P M R directory profile run
teamObjectiveSatisfied := evaluation.teamObjective run
corrections :=
CorrectionProfileFromRun run profile (evaluation.horizonOf strategy)
}

/-- Best response collaborativa rispetto all'ambiente fissato. -/
def RationalStrategy
[DecidableEq V.SystemPrompt]
[DecidableEq V.PrincipalId]
(B : ApiBoundary V)
(P : PromptSemantics V)
(M : TransitionSystem V)
(R : ResponseSemantics V)
(directory : AgentDirectory V)
(profile : AgentProfile V)
(evaluation : StrategyEvaluation P profile)
(strategy : AgentStrategy P profile) : Prop :=
∀ alternative,
¬ AgentPrefers
(OutcomeOfStrategy B P M R directory profile evaluation alternative)
(OutcomeOfStrategy B P M R directory profile evaluation strategy)

/-! ## 9. Conseguenze immediate -/

/-- Una transizione legale preserva tutti i prompt di sistema. -/
theorem legal_step_preserves_system_prompts
{B : ApiBoundary V}
{P : PromptSemantics V}
{M : TransitionSystem V}
{profile : AgentProfile V}
{s s' : State V}
{action : AgentAction V}
(h : LegalAgentStep B P M profile s action s') :
∀ principal,
s'.systemPrompts principal = s.systemPrompts principal := by
exact h.promptFrame

/-- Una transizione legale preserva i permessi sulle risorse. -/
theorem legal_step_preserves_permissions
{B : ApiBoundary V}
{P : PromptSemantics V}
{M : TransitionSystem V}
{profile : AgentProfile V}
{s s' : State V}
{action : AgentAction V}
(h : LegalAgentStep B P M profile s action s') :
∀ principal resource capability,
s'.permissions principal resource capability ↔
s.permissions principal resource capability := by
exact h.permissionFrame.1

/-- Una transizione legale preserva i permessi dei tool. -/
theorem legal_step_preserves_tool_permissions
{B : ApiBoundary V}
{P : PromptSemantics V}
{M : TransitionSystem V}
{profile : AgentProfile V}
{s s' : State V}
{action : AgentAction V}
(h : LegalAgentStep B P M profile s action s') :
∀ principal tool,
s'.toolPermission principal tool ↔
s.toolPermission principal tool := by
exact h.permissionFrame.2

/-- L'ammissibilità implica l'autorizzazione operativa. -/
theorem admissible_implies_operationally_allowed
{P : PromptSemantics V}
{s : State V}
{profile : AgentProfile V}
{action : AgentAction V}
(h : Admissible P s profile action) :
OperationallyAllowed s profile action := by
exact h.1

/-- L'ammissibilità implica l'autorizzazione normativa. -/
theorem admissible_implies_normatively_allowed
{P : PromptSemantics V}
{s : State V}
{profile : AgentProfile V}
{action : AgentAction V}
(h : Admissible P s profile action) :
NormativelyAllowed P s profile action := by
exact h.2

/-- Meno revisioni del prompt dominano ogni differenza nei commenti. -/
theorem fewer_prompt_revisions_dominate_all_comments
{x y : CorrectionProfile}
(h : x.promptRevisions < y.promptRevisions) :
BetterCorrection x y := by
exact Or.inl h

/-- A parità di livelli superiori, i commenti utente dominano quelli agentici. -/
theorem fewer_user_comments_dominate_agent_comments
{x y : CorrectionProfile}
(samePrompt : x.promptRevisions = y.promptRevisions)
(sameAdmin : x.adminComments = y.adminComments)
(fewerUsers : x.userComments < y.userComments) :
BetterCorrection x y := by
exact Or.inr ⟨samePrompt,
Or.inr ⟨sameAdmin, Or.inl fewerUsers⟩⟩

/-- I commenti agentici risolvono il confronto solo a parità di livelli superiori. -/
theorem fewer_agent_comments_break_complete_tie
{x y : CorrectionProfile}
(samePrompt : x.promptRevisions = y.promptRevisions)
(sameAdmin : x.adminComments = y.adminComments)
(sameUsers : x.userComments = y.userComments)
(fewerAgents : x.agentComments < y.agentComments) :
BetterCorrection x y := by
exact Or.inr ⟨samePrompt,
Or.inr ⟨sameAdmin,
Or.inr ⟨sameUsers, fewerAgents⟩⟩⟩

/-!


Confini dei prossimi refinement
Restano intenzionalmente esterni al presente modulo:

      mapping di DTO, handler API e persistenza concreta;
      compilazione linguistica effettiva da testo del prompt a PromptProgram ;
      implementazione concreta dei tool e del loro scheduler;
      politiche quantitative di rate limiting oltre ai limiti strutturali qui definiti;
      algoritmi crittografici, envelope, key management e rotazione delle chiavi.

La crittografia non deve essere raffinata in questo file: dipende dall'ambiente
in cui il contratto viene integrato.
-/

/-! ## 10. R5 — estensione conservativa: goal, obligation, work e completamento

R5 è una estensione conservativa della Revisione 4.

Tutte le definizioni R4 precedenti restano normative e disponibili senza
modifica. In particolare restano invariati:

* Vocabulary e il suo carattere parametrico;
* ResourceMeta, TaskData, keyEpoch, tombstone e versioning;
* il linguaggio completo AgentAction;
* PromptSemantics R4 e le sue leggi;
* WellFormedState;
* autorizzazione, frame conditions ed effetti esatti;
* LegalAgentStep, LegalHumanStep e LegalRuntimeEventStep;
* Run, ValidTick e ValidRun;
* fairness R4, timeout, retry e responsiveness;
* coordinamento multi-agente e CommentPriorityDiscipline;
* TaskCompletionCausality, AssignedTaskLiveness e PromptObligationLiveness;
* CorrectionProfile, Outcome, strategie e preferenze.

R5 aggiunge un livello semantico sopra tali oggetti. Il refinement concreto
deve dimostrare che l'overlay R5 proietta sulla run R4 sottostante e non può
usare i nuovi concetti per eludere alcuna proprietà R4.

Aggiornamento R5:
* soltanto un administrator può autorizzare una revisione del goal;
* commenti di user e agent sono informativi rispetto al goal, pur restando
  causalmente rilevanti per risposta, evidence, blocker e nuovo lavoro;
* le attese obbligatorie sono blocker espliciti;
* blocker e dependency possono dipendere anche da altri agenti;
* una proposta utente di modifica del goal deve essere escalata tramite task assegnata a un administrator;
* task completata e decisione amministrativa restano concetti distinti;
* una revisione proposta da user richiede una decisione amministrativa approved; un administrator può anche revisionare direttamente il goal con propria autorità.
-/

namespace R5

/-! ### R5.1 — Vocabolario aggiuntivo, senza concretizzare gli identificativi R4 -/

/-- Tipi primitivi aggiuntivi richiesti dal refinement R5. -/
structure ExtensionVocabulary (V : Vocabulary) where
  GoalId : Type u
  RunId : Type u
  WorkItemId : Type u
  ClaimId : Type u
  EvidenceId : Type u
  BlockerId : Type u
  GoalRevisionId : Type u
  ProgramRevisionId : Type u
  ExternalConditionId : Type u

variable {V : Vocabulary}
variable {X : ExtensionVocabulary V}

/-! ### R5.2 — Goal e criterio di completamento -/

inductive GoalStatus where
  | active
  | completed
  | failed
  | cancelled
  | superseded
  deriving DecidableEq, Repr

/-- Esito esplicito della valutazione amministrativa di una proposta di revisione. -/
inductive AdministratorDecision where
  | approved
  | rejected
  deriving DecidableEq, Repr

/-- Specifica statica e immutabile di una revisione dell'obiettivo. -/
structure GoalSpec (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : X.GoalId
  scope : V.ResourceId

/--
Una modifica amministrativa non muta in-place il goal precedente: crea una
nuova GoalSpec che lo supersede. In questo modo la storia normativa rimane
append-only e il teorema di completamento può essere applicato a una revisione
stabile del goal.
-/
structure GoalRevision (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : X.GoalRevisionId
  run : X.RunId
  previousGoal : X.GoalId
  revisedGoal : GoalSpec V X
  author : V.PrincipalId
  sourceComment : V.CommentId
  /--
  none = revisione amministrativa originata direttamente dall'administrator.
  some c = revisione causalmente derivata da una proposta user c e quindi
  soggetta obbligatoriamente al workflow di escalation amministrativa.
  -/
  proposalSource : Option V.CommentId
  observedAt : Nat

/-- Proposta informativa di revisione del goal proveniente da un user. -/
structure GoalChangeProposal (V : Vocabulary) (X : ExtensionVocabulary V) where
  run : X.RunId
  goal : X.GoalId
  proposer : V.PrincipalId
  sourceComment : V.CommentId
  observedAt : Nat

/-- Escalation persistente della proposta verso un administrator tramite task R4. -/
structure AdministratorEscalation (V : Vocabulary) (X : ExtensionVocabulary V) where
  run : X.RunId
  goal : X.GoalId
  proposalComment : V.CommentId
  proposer : V.PrincipalId
  createdByAgent : V.PrincipalId
  administrator : V.PrincipalId
  reviewTask : V.ResourceId
  createdAt : Nat

/-- Decisione amministrativa distinta dal semplice completamento della task. -/
structure AdministratorGoalDecision (V : Vocabulary) (X : ExtensionVocabulary V) where
  run : X.RunId
  goal : X.GoalId
  administrator : V.PrincipalId
  reviewTask : V.ResourceId
  decision : AdministratorDecision
  decisionComment : V.CommentId
  observedAt : Nat

/-- Stato operativo di una run R5. Non sostituisce la Run R4. -/
inductive RunStatus where
  | running
  | completed
  | cancelled
  deriving DecidableEq, Repr

/-! ### R5.3 — Specifica statica delle obligation -/

structure ObligationSpec (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : V.ObligationId
  goal : X.GoalId
  owner : V.PrincipalId
  /-- Condizione statica/dinamica che rende l'obligation applicabile. -/
  activationCondition : State V → Prop
  /--
  Condizione che rende l'obligation necessaria alla chiusura semantica.
  Permette obligation condizionali senza imporre l'istanziazione ingenua di
  tutte le specifiche presenti nel programma.
  -/
  requiredForCompletion : State V → Prop

structure Dependency (V : Vocabulary) where
  obligation : V.ObligationId
  prerequisite : V.ObligationId

inductive EvidenceKind where
  | toolCompleted
  | taskCompleted
  | commentObserved
  | principalResponse
  | humanApproval
  | administratorApproval
  | externalOutcome
  | derivedFact
  deriving DecidableEq, Repr

/--
Subject tipato dell'evidence. Il kind da solo non basta: taskCompleted deve
riferirsi alla task corretta, principalResponse al principal corretto, ecc.
-/
inductive EvidenceSubject (V : Vocabulary) (X : ExtensionVocabulary V) where
  | toolCall (callId : V.ToolCallId)
  | task (taskId : V.ResourceId)
  | comment (commentId : V.CommentId)
  | principal (principal : V.PrincipalId)
  | obligation (obligation : V.ObligationId)
  | administratorDecision (administrator : V.PrincipalId) (reviewTask : V.ResourceId)
  | externalCondition (condition : X.ExternalConditionId)
  | derived

structure EvidenceRequirement (V : Vocabulary) (X : ExtensionVocabulary V) where
  obligation : V.ObligationId
  kind : EvidenceKind
  subject : EvidenceSubject V X

/-!
Programma semantico STATICO derivato dal prompt.

A differenza del PromptProgram R4, questo oggetto non contiene lo stato
runtime active/discharged delle obligation. Quelle informazioni appartengono
all'overlay dinamico R5.
-/
structure SemanticProgram (V : Vocabulary) (X : ExtensionVocabulary V) where
  goal : GoalSpec V X
  obligationSpecs : List (ObligationSpec V X)
  dependencies : List (Dependency V)
  evidenceRequirements : List (EvidenceRequirement V X)
  actionSupports : State V → AgentAction V → Prop
  waitingAllowed : State V → Prop
  /--
  Criterio semantico statico ulteriore per consentire la chiusura del goal.
  Evita di definire GoalCompleted come mera abbreviazione di "tutte le
  obligation note sono discharged".
  -/
  completionGuard : State V → Prop

/-- Compilatore linguistico/semantico astratto. -/
structure SemanticCompiler (V : Vocabulary) (X : ExtensionVocabulary V) where
  compile : V.SystemPrompt → SemanticProgram V X

/-- Leggi strutturali minime del programma compilato. -/
structure SemanticCompilerLaws
    (compiler : SemanticCompiler V X) : Prop where
  obligationGoalConsistency :
    ∀ prompt spec,
      spec ∈ (compiler.compile prompt).obligationSpecs →
      spec.goal = (compiler.compile prompt).goal.id

  dependencySourceKnown :
    ∀ prompt dep,
      dep ∈ (compiler.compile prompt).dependencies →
      ∃ spec,
        spec ∈ (compiler.compile prompt).obligationSpecs ∧
        spec.id = dep.obligation

  dependencyTargetKnown :
    ∀ prompt dep,
      dep ∈ (compiler.compile prompt).dependencies →
      ∃ spec,
        spec ∈ (compiler.compile prompt).obligationSpecs ∧
        spec.id = dep.prerequisite

  evidenceRequirementKnown :
    ∀ prompt req,
      req ∈ (compiler.compile prompt).evidenceRequirements →
      ∃ spec,
        spec ∈ (compiler.compile prompt).obligationSpecs ∧
        spec.id = req.obligation

  noNoOpService :
    ∀ prompt state,
      ¬ (compiler.compile prompt).actionSupports state (AgentAction.noOp)

/-!
R5 non assume che il compilatore linguistico concreto esista già.
SemanticCompilerLaws è il contratto che una futura implementazione o
attestazione deve soddisfare.
-/

/--
Snapshot append-only del programma semantico autorevole usato da una run.
Una revisione del goal deve attivare un nuovo snapshot invece di mutare quello
precedente in-place.
-/
structure ProgramSnapshot (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : X.ProgramRevisionId
  run : X.RunId
  prompt : V.SystemPrompt
  program : SemanticProgram V X
  sourceGoalRevision : Option X.GoalRevisionId
  activatedAt : Nat

/-! ### R5.4 — Obligation dinamiche ed evidence osservabili -/

inductive ObligationStatus where
  | active
  | discharged
  deriving DecidableEq, Repr

structure ObligationInstance (V : Vocabulary) (X : ExtensionVocabulary V) where
  run : X.RunId
  spec : V.ObligationId
  owner : V.PrincipalId
  status : ObligationStatus

structure Evidence (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : X.EvidenceId
  run : X.RunId
  obligation : V.ObligationId
  kind : EvidenceKind
  subject : EvidenceSubject V X
  observedAt : Nat

/-- L'istanza dinamica deve provenire da una specifica statica del programma. -/
def Instantiates
    (program : SemanticProgram V X)
    (obligationInstance : ObligationInstance V X) : Prop :=
  ∃ spec,
    spec ∈ program.obligationSpecs ∧
    spec.id = obligationInstance.spec ∧
    spec.owner = obligationInstance.owner

/-- Dipendenza statica fra due obligation. -/
def DependsOn
    (program : SemanticProgram V X)
    (obligation prerequisite : V.ObligationId) : Prop :=
  ∃ dep,
    dep ∈ program.dependencies ∧
    dep.obligation = obligation ∧
    dep.prerequisite = prerequisite

/-! ### R5.4A — Attese obbligatorie, blocker e dipendenze fra principal -/

/--
Un blocker rappresenta una condizione che deve essere risolta prima che il
lavoro a cui si applica possa diventare eleggibile.

La condizione può dipendere da un tool, da una task, da un'obligation, da un
utente, da un amministratore, da un altro agente o dall'ambiente.
-/
inductive WaitingCondition (V : Vocabulary) (X : ExtensionVocabulary V) where
  | toolTerminal (callId : V.ToolCallId)
  | principalResponse (principal : V.PrincipalId)
  | taskCompleted (task : V.ResourceId)
  | obligationDischarged (obligation : V.ObligationId)
  | administratorApproval (administrator : V.PrincipalId)
  | externalOutcome (condition : X.ExternalConditionId)
  | derivedCondition (condition : X.ExternalConditionId)

/--
Il blocker può essere locale a un work item, condiviso da tutta un'obligation
oppure costituire una barriera per l'intero goal.
-/
inductive BlockScope (V : Vocabulary) (X : ExtensionVocabulary V) where
  | work (work : X.WorkItemId)
  | obligation (obligation : V.ObligationId)
  | goal (goal : X.GoalId)

inductive BlockerStatus where
  | waiting
  | resolved
  | failed
  | cancelled
  deriving DecidableEq, Repr

structure Blocker (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : X.BlockerId
  run : X.RunId
  goal : X.GoalId
  scope : BlockScope V X
  condition : WaitingCondition V X
  status : BlockerStatus
  createdAt : Nat

/-- Un blocker è terminale quando non impone più attesa. -/
def BlockerTerminal (blocker : Blocker V X) : Prop :=
  blocker.status = BlockerStatus.resolved ∨
  blocker.status = BlockerStatus.failed ∨
  blocker.status = BlockerStatus.cancelled

/-- Il blocker è ancora attivo e impone attesa. -/
def BlockerWaiting (blocker : Blocker V X) : Prop :=
  blocker.status = BlockerStatus.waiting

/--
La risoluzione di un blocker è distinta dalla evidence che discharge
un'obligation. Per esempio un toolFailed o toolTimedOut può terminare
legittimamente un'attesa senza soddisfare l'obligation.
-/
structure BlockerResolution (V : Vocabulary) (X : ExtensionVocabulary V) where
  blocker : X.BlockerId
  observedAt : Nat

/--
Le dependency fra obligation sono già generali: prerequisite e obligation
possono appartenere a owner differenti. Questa osservazione rende esplicito il
caso inter-agent senza introdurre un secondo meccanismo di dependency.
-/
def InterAgentDependency
    (program : SemanticProgram V X)
    (obligation prerequisite : V.ObligationId) : Prop :=
  DependsOn program obligation prerequisite ∧
  ∃ targetSpec sourceSpec,
    targetSpec ∈ program.obligationSpecs ∧
    sourceSpec ∈ program.obligationSpecs ∧
    targetSpec.id = obligation ∧
    sourceSpec.id = prerequisite ∧
    targetSpec.owner ≠ sourceSpec.owner

/-! ### R5.5 — Work item, dispatch, claim e recovery astratti -/

inductive WorkKind where
  | agentAction
  | toolInvocation
  | toolRetry
  | taskAction
  | coordination
  | externalWait
  deriving DecidableEq, Repr

inductive WorkStatus where
  | blocked
  | eligible
  | claimed
  | succeeded
  | failed
  | cancelled
  deriving DecidableEq, Repr

structure WorkItem (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : X.WorkItemId
  run : X.RunId
  goal : X.GoalId
  owner : V.PrincipalId
  serves : V.ObligationId
  kind : WorkKind
  /-- Parent causale opzionale per controllare l'espansione dinamica del work graph. -/
  parent : Option X.WorkItemId
  /-- Commento che ha originato il lavoro, se presente. -/
  sourceComment : Option V.CommentId
  status : WorkStatus
  attempt : Nat
  maxAttempts : Nat
  createdAt : Nat

inductive DispatchStatus where
  | ready
  | claimed
  | closed
  deriving DecidableEq, Repr

structure Dispatch (V : Vocabulary) (X : ExtensionVocabulary V) where
  work : X.WorkItemId
  attempt : Nat
  status : DispatchStatus
  enqueuedAt : Nat

structure Claim (V : Vocabulary) (X : ExtensionVocabulary V) where
  id : X.ClaimId
  work : X.WorkItemId
  attempt : Nat
  claimant : V.PrincipalId

/-! ### R5.5A — Grafo causale collaborativo globale -/

/--
Nodo causale globale del sistema collaborativo. Il grafo non appartiene a un
singolo agente: rappresenta il goal condiviso attraverso obligation, work,
commenti, task, tool e blocker di tutti i participant.
-/
inductive CollaborativeCausalNode (V : Vocabulary) (X : ExtensionVocabulary V) where
  | obligation (obligation : V.ObligationId)
  | work (work : X.WorkItemId)
  | comment (comment : V.CommentId)
  | task (task : V.ResourceId)
  | toolCall (call : V.ToolCallId)
  | blocker (blocker : X.BlockerId)

/--
Link append-only orientato dalla causa al suo effetto. `predecessor` è la causa,
`successor` è il nuovo nodo causalmente prodotto/abilitato.
-/
structure CollaborativeCausalLink (V : Vocabulary) (X : ExtensionVocabulary V) where
  run : X.RunId
  goal : X.GoalId
  predecessor : CollaborativeCausalNode V X
  successor : CollaborativeCausalNode V X
  observedAt : Nat

/-- Recovery preserva l'identità semantica del lavoro. -/
def Recoverable
    (before after : WorkItem V X) : Prop :=
  before.id = after.id ∧
  before.run = after.run ∧
  before.goal = after.goal ∧
  before.owner = after.owner ∧
  before.serves = after.serves ∧
  before.kind = after.kind

/-! ### R5.6 — Overlay semantico su State e Run R4 -/

/--
L'overlay contiene soltanto concetti nuovi. `base` resta lo State R4 completo
con risorse, task, prompt, tool call, audit, permessi e coordinamento.
-/
structure SemanticState (V : Vocabulary) (X : ExtensionVocabulary V) where
  base : State V

  goals : X.GoalId → Option (GoalSpec V X)
  goalStatus : X.GoalId → Option GoalStatus

  runGoal : X.RunId → Option X.GoalId
  runScope : X.RunId → Option V.ResourceId
  runStatus : X.RunId → Option RunStatus
  runParticipants : X.RunId → V.PrincipalId → Prop

  obligations : V.ObligationId → Option (ObligationInstance V X)
  evidences : List (Evidence V X)

  goalRevisions : List (GoalRevision V X)
  goalChangeProposals : List (GoalChangeProposal V X)
  administratorEscalations : List (AdministratorEscalation V X)
  administratorGoalDecisions : List (AdministratorGoalDecision V X)

  programSnapshots : List (ProgramSnapshot V X)
  activeProgram : X.RunId → Option X.ProgramRevisionId

  blockers : X.BlockerId → Option (Blocker V X)
  blockerResolutions : List (BlockerResolution V X)

  workItems : X.WorkItemId → Option (WorkItem V X)
  dispatches : X.WorkItemId → Option (Dispatch V X)
  claims : List (Claim V X)

  /-- Grafo causale append-only del sistema collaborativo per goal/run. -/
  causalLinks : List (CollaborativeCausalLink V X)

/--
Una run R5 è una run R4 più uno stato semantico aggiuntivo a ogni tick.
La prima legge di validità richiederà che `semanticState n`.base coincida con
`baseRun.state n`.
-/
structure SemanticRun (V : Vocabulary) (X : ExtensionVocabulary V) where
  baseRun : Run V
  semanticState : Nat → SemanticState V X

/-- Proiezione letterale sulla run R4. -/
def SemanticRun.toR4 (run : SemanticRun V X) : Run V :=
  run.baseRun

/-- Coerenza punto-per-punto fra overlay e State R4. -/
def ProjectsToR4 (run : SemanticRun V X) : Prop :=
  ∀ n,
    (run.semanticState n).base = run.baseRun.state n

/-- Una R5 validamente proiettata conserva integralmente ValidRun R4. -/
def PreservesR4ValidRun
    (B : ApiBoundary V)
    (P : PromptSemantics V)
    (M : TransitionSystem V)
    (directory : AgentDirectory V)
    (run : SemanticRun V X) : Prop :=
  ProjectsToR4 run ∧
  ValidRun B P M directory run.baseRun

/-! ### R5.7 — Osservazioni dinamiche -/

inductive SemanticEvent (V : Vocabulary) (X : ExtensionVocabulary V) where
  | goalActivated (goal : X.GoalId)
  | goalChangeProposed (proposal : GoalChangeProposal V X)
  | administratorEscalated (escalation : AdministratorEscalation V X)
  | administratorGoalDecided (decision : AdministratorGoalDecision V X)
  | goalRevised (revision : GoalRevision V X)
  | obligationActivated (obligation : V.ObligationId)
  | obligationDischarged (obligation : V.ObligationId) (evidence : X.EvidenceId)
  | workBecameEligible (work : X.WorkItemId)
  | workClaimed (work : X.WorkItemId) (claim : X.ClaimId)
  | claimExpired (claim : X.ClaimId)
  | goalCompleted (goal : X.GoalId)
  | goalFailed (goal : X.GoalId)
  | runCompleted (run : X.RunId)
  | runCancelled (run : X.RunId)

structure ObservedSemanticRun (V : Vocabulary) (X : ExtensionVocabulary V) extends
    SemanticRun V X where
  semanticEvent : Nat → Option (SemanticEvent V X)

/-! ### R5.8 — Goal, run e obligation observations -/

def GoalValid
    (s : SemanticState V X)
    (goal : X.GoalId) : Prop :=
  s.goalStatus goal = some GoalStatus.active ∨
  s.goalStatus goal = some GoalStatus.completed

/-- Successo semantico, distinto dalla terminalizzazione operativa della run. -/
def GoalCompleted
    (s : SemanticState V X)
    (goal : X.GoalId) : Prop :=
  s.goalStatus goal = some GoalStatus.completed

/-- Fallimento semantico esplicito. -/
def GoalFailed
    (s : SemanticState V X)
    (goal : X.GoalId) : Prop :=
  s.goalStatus goal = some GoalStatus.failed

/-- Cancellazione semantica esplicita. -/
def GoalCancelled
    (s : SemanticState V X)
    (goal : X.GoalId) : Prop :=
  s.goalStatus goal = some GoalStatus.cancelled

/-- Stato operativo della run R5. -/
def RunRunning
    (s : SemanticState V X)
    (runId : X.RunId) : Prop :=
  s.runStatus runId = some RunStatus.running

/-- La run è stata chiusa operativamente con successo. -/
def RunCompleted
    (s : SemanticState V X)
    (runId : X.RunId) : Prop :=
  s.runStatus runId = some RunStatus.completed

/-- La run è stata cancellata. -/
def RunCancelled
    (s : SemanticState V X)
    (runId : X.RunId) : Prop :=
  s.runStatus runId = some RunStatus.cancelled

/-! ### R5.8A — Autorità sui goal e semantica dei commenti -/

/-- Una proposta di modifica del goal è valida soltanto se proviene da un user e da un commento osservato. -/
def UserGoalChangeProposalValid
    (s : SemanticState V X)
    (proposal : GoalChangeProposal V X) : Prop :=
  ∃ comment,
    comment ∈ s.base.comments ∧
    comment.id = proposal.sourceComment ∧
    comment.author = proposal.proposer ∧
    HasKind s.base proposal.proposer PrincipalKind.user ∧
    s.runGoal proposal.run = some proposal.goal

/-- L'escalation amministrativa deve essere realizzata tramite una task R4 assegnata all'administrator. -/
def AdministratorEscalationValid
    (s : SemanticState V X)
    (escalation : AdministratorEscalation V X) : Prop :=
  HasKind s.base escalation.proposer PrincipalKind.user ∧
  HasKind s.base escalation.createdByAgent PrincipalKind.agent ∧
  HasKind s.base escalation.administrator PrincipalKind.administrator ∧
  s.runGoal escalation.run = some escalation.goal ∧
  (∃ proposal,
    proposal ∈ s.goalChangeProposals ∧
    proposal.run = escalation.run ∧
    proposal.goal = escalation.goal ∧
    proposal.proposer = escalation.proposer ∧
    proposal.sourceComment = escalation.proposalComment) ∧
  IsResourceKind s.base escalation.reviewTask ResourceKind.task ∧
  CreatedBy s.base escalation.reviewTask escalation.createdByAgent ∧
  AssignedTo s.base escalation.administrator escalation.reviewTask ∧
  OpenTask s.base escalation.reviewTask

/-- Ogni proposta utente valida di modifica del goal deve essere eventualmente escalata tramite task a un administrator. -/
def UserGoalProposalEscalationLiveness
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n proposal,
    proposal ∈ (run.semanticState n).goalChangeProposals →
    UserGoalChangeProposalValid (run.semanticState n) proposal →
    EventuallyAfter n (fun m =>
      ∃ escalation,
        escalation ∈ (run.semanticState m).administratorEscalations ∧
        escalation.run = proposal.run ∧
        escalation.goal = proposal.goal ∧
        escalation.proposer = proposal.proposer ∧
        escalation.proposalComment = proposal.sourceComment ∧
        AdministratorEscalationValid (run.semanticState m) escalation)

/-- Finché non esiste una decisione amministrativa, il lavoro dipendente dalla revisione deve restare bloccato. -/
def EscalationRequiresBlocking
    (s : SemanticState V X)
    (escalation : AdministratorEscalation V X) : Prop :=
  AdministratorEscalationValid s escalation →
  ∃ blockerId blocker,
    s.blockers blockerId = some blocker ∧
    blocker.run = escalation.run ∧
    blocker.goal = escalation.goal ∧
    BlockerWaiting blocker ∧
    (blocker.condition = WaitingCondition.administratorApproval escalation.administrator ∨
     blocker.condition = WaitingCondition.taskCompleted escalation.reviewTask)

/-- Una decisione amministrativa è valida solo dopo il completamento della review task e con commento dell'administrator. -/
def AdministratorGoalDecisionValid
    (s : SemanticState V X)
    (decision : AdministratorGoalDecision V X) : Prop :=
  HasKind s.base decision.administrator PrincipalKind.administrator ∧
  DoneTask s.base decision.reviewTask ∧
  (∃ escalation,
    escalation ∈ s.administratorEscalations ∧
    escalation.run = decision.run ∧
    escalation.goal = decision.goal ∧
    escalation.administrator = decision.administrator ∧
    escalation.reviewTask = decision.reviewTask) ∧
  ∃ comment,
    comment ∈ s.base.comments ∧
    comment.id = decision.decisionComment ∧
    comment.author = decision.administrator

/-- Nel workflow originato da user, solo una decisione approved può autorizzare la GoalRevision. -/
def ApprovedDecisionAuthorizesRevision
    (s : SemanticState V X)
    (decision : AdministratorGoalDecision V X)
    (revision : GoalRevision V X) : Prop :=
  AdministratorGoalDecisionValid s decision ∧
  decision.decision = AdministratorDecision.approved ∧
  revision.run = decision.run ∧
  revision.previousGoal = decision.goal ∧
  revision.author = decision.administrator ∧
  revision.sourceComment = decision.decisionComment ∧
  ∃ escalation,
    escalation ∈ s.administratorEscalations ∧
    escalation.run = decision.run ∧
    escalation.goal = decision.goal ∧
    escalation.reviewTask = decision.reviewTask ∧
    revision.proposalSource = some escalation.proposalComment

/-- Un rifiuto amministrativo preserva il goal corrente. -/
def RejectedDecisionPreservesGoal
    (before after : SemanticState V X)
    (decision : AdministratorGoalDecision V X) : Prop :=
  AdministratorGoalDecisionValid before decision ∧
  decision.decision = AdministratorDecision.rejected ∧
  after.runGoal decision.run = before.runGoal decision.run ∧
  after.goalStatus decision.goal = before.goalStatus decision.goal

/--
Un commento di un amministratore può essere la fonte di una revisione del goal.
Non ogni commento amministrativo modifica il goal: l'autorità rende la
revisione lecita, non automatica.
-/
def DirectAdministratorRevisionAuthorized
    (s : SemanticState V X)
    (revision : GoalRevision V X) : Prop :=
  revision.proposalSource = none ∧
  ∃ source previous,
    source ∈ s.base.comments ∧
    source.id = revision.sourceComment ∧
    source.author = revision.author ∧
    HasKind s.base revision.author PrincipalKind.administrator ∧
    s.goals revision.previousGoal = some previous ∧
    previous.scope = revision.revisedGoal.scope

def EscalatedAdministratorRevisionAuthorized
    (s : SemanticState V X)
    (revision : GoalRevision V X) : Prop :=
  ∃ previous decision,
    s.goals revision.previousGoal = some previous ∧
    previous.scope = revision.revisedGoal.scope ∧
    decision ∈ s.administratorGoalDecisions ∧
    ApprovedDecisionAuthorizesRevision s decision revision

def GoalRevisionAuthorized
    (s : SemanticState V X)
    (revision : GoalRevision V X) : Prop :=
  DirectAdministratorRevisionAuthorized s revision ∨
  EscalatedAdministratorRevisionAuthorized s revision

/--
Gli utenti non amministratori e gli agenti possono influenzare l'esecuzione
come informazione, risposta, evidence o coordinamento, ma non possiedono
autorità normativa per revisionare il goal.
-/
def InformationalComment
    (s : SemanticState V X)
    (comment : Comment V) : Prop :=
  (HasKind s.base comment.author PrincipalKind.user ∨
   HasKind s.base comment.author PrincipalKind.agent)

/--
Effetto semantico minimo di una revisione lecita: il goal precedente viene
superseded, il nuovo goal diventa active e la run viene associata alla nuova
revisione. Il cambio di scope non è consentito all'interno della stessa run;
un cambio di scope richiede una nuova run.
-/
def GoalRevisionEffect
    (before after : SemanticState V X)
    (revision : GoalRevision V X) : Prop :=
  GoalRevisionAuthorized before revision ∧
  before.goalStatus revision.previousGoal = some GoalStatus.active ∧
  after.goals revision.revisedGoal.id = some revision.revisedGoal ∧
  after.goalStatus revision.previousGoal = some GoalStatus.superseded ∧
  after.goalStatus revision.revisedGoal.id = some GoalStatus.active ∧
  after.runGoal revision.run = some revision.revisedGoal.id ∧
  revision ∈ after.goalRevisions

/--
La policy è intenzionalmente asimmetrica:
* administrator: può autorizzare una GoalRevision;
* user: informativo rispetto al goal;
* agent: informativo/coordinativo rispetto al goal.

User e agent restano comunque pienamente rilevanti per Activates,
ResponseSemantics, blocker resolution, evidence e lavoro successivo.
-/
structure GoalEscalationLaws
    (run : ObservedSemanticRun V X) : Prop where
  proposalEscalation :
    UserGoalProposalEscalationLiveness run

  uniqueEscalationPerProposal :
    ∀ n first second,
      first ∈ (run.semanticState n).administratorEscalations →
      second ∈ (run.semanticState n).administratorEscalations →
      first.run = second.run →
      first.proposalComment = second.proposalComment →
      first.reviewTask = second.reviewTask ∧
      first.administrator = second.administrator

  escalationBlocked :
    ∀ n escalation,
      escalation ∈ (run.semanticState n).administratorEscalations →
      EscalationRequiresBlocking (run.semanticState n) escalation

  everyDecisionValid :
    ∀ n decision,
      decision ∈ (run.semanticState n).administratorGoalDecisions →
      AdministratorGoalDecisionValid (run.semanticState n) decision

  decisionEventuallyReleasesEscalationBlocker :
    ∀ n decision,
      decision ∈ (run.semanticState n).administratorGoalDecisions →
      AdministratorGoalDecisionValid (run.semanticState n) decision →
      EventuallyAfter n (fun m =>
        ∀ blockerId blocker,
          (run.semanticState m).blockers blockerId = some blocker →
          blocker.run = decision.run →
          blocker.goal = decision.goal →
          (blocker.condition =
             WaitingCondition.administratorApproval decision.administrator ∨
           blocker.condition =
             WaitingCondition.taskCompleted decision.reviewTask) →
          BlockerTerminal blocker)

  rejectedDoesNotRevise :
    ∀ n decision,
      decision ∈ (run.semanticState n).administratorGoalDecisions →
      decision.decision = AdministratorDecision.rejected →
      ¬ ∃ revision,
        revision ∈ (run.semanticState n).goalRevisions ∧
        revision.run = decision.run ∧
        revision.previousGoal = decision.goal ∧
        revision.sourceComment = decision.decisionComment ∧
        revision.proposalSource ≠ none

structure GoalRevisionLaws
    (run : ObservedSemanticRun V X) : Prop where
  everyRevisionAuthorized :
    ∀ n revision,
      revision ∈ (run.semanticState n).goalRevisions →
      GoalRevisionAuthorized (run.semanticState n) revision

  userNeverAuthorizesRevision :
    ∀ n revision,
      HasKind (run.semanticState n).base revision.author PrincipalKind.user →
      ¬ GoalRevisionAuthorized (run.semanticState n) revision

  agentNeverAuthorizesRevision :
    ∀ n revision,
      HasKind (run.semanticState n).base revision.author PrincipalKind.agent →
      ¬ GoalRevisionAuthorized (run.semanticState n) revision

/-- Una obligation è discharged nello stato dinamico. -/
def ObligationDischarged
    (s : SemanticState V X)
    (obligation : V.ObligationId) : Prop :=
  ∃ obligationInstance,
    s.obligations obligation = some obligationInstance ∧
    obligationInstance.status = ObligationStatus.discharged

/-- Tutti i prerequisiti di una obligation sono discharged. -/
def DependencyClosed
    (s : SemanticState V X)
    (program : SemanticProgram V X)
    (obligationInstance : ObligationInstance V X) : Prop :=
  ∀ prerequisite,
    DependsOn program obligationInstance.spec prerequisite →
    ObligationDischarged s prerequisite

/-! ### R5.9 — Evidence distinta dal mero fatto osservato -/

/-- L'evidence deve riferirsi a un tick realmente osservato. -/
def EvidenceObserved
    (run : ObservedSemanticRun V X)
    (evidence : Evidence V X) : Prop :=
  ∃ event,
    run.baseRun.event evidence.observedAt = some event

/-- Il kind dell'evidence deve essere previsto dalla specifica statica. -/
def EvidenceRequirementSatisfied
    (program : SemanticProgram V X)
    (evidence : Evidence V X) : Prop :=
  ∃ requirement,
    requirement ∈ program.evidenceRequirements ∧
    requirement.obligation = evidence.obligation ∧
    requirement.kind = evidence.kind ∧
    requirement.subject = evidence.subject

/--
ValidEvidence è intenzionalmente più forte di EvidenceObserved.
Il refinement concreto deve aggiungere la regola semantica che collega il
contenuto/risultato osservato al requisito dell'obligation.
-/
structure EvidenceSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  validEvidence :
    SemanticProgram V X →
    ObservedSemanticRun V X →
    Evidence V X →
    Prop

structure EvidenceSemanticsLaws
    (E : EvidenceSemantics V X) : Prop where
  validImpliesObserved :
    ∀ program run evidence,
      E.validEvidence program run evidence →
      EvidenceObserved run evidence

  validImpliesRequiredKind :
    ∀ program run evidence,
      E.validEvidence program run evidence →
      EvidenceRequirementSatisfied program evidence

/--
Provenance tipata minima. Queste leggi non decidono la sufficienza linguistica
del contenuto, ma impediscono evidence strutturalmente riferita al subject
sbagliato.
-/
structure EvidenceProvenanceLaws
    (E : EvidenceSemantics V X) : Prop where
  toolEvidenceHasMatchingTerminalEvent :
    ∀ program run evidence callId,
      E.validEvidence program run evidence →
      evidence.kind = EvidenceKind.toolCompleted →
      evidence.subject = EvidenceSubject.toolCall callId →
      ∃ output,
        run.baseRun.event evidence.observedAt =
          some (Event.toolCompleted callId output)

  taskEvidenceHasMatchingDoneTask :
    ∀ program run evidence taskId,
      E.validEvidence program run evidence →
      evidence.kind = EvidenceKind.taskCompleted →
      evidence.subject = EvidenceSubject.task taskId →
      DoneTask (run.baseRun.state evidence.observedAt) taskId

  principalResponseHasMatchingAuthor :
    ∀ program run evidence principal,
      E.validEvidence program run evidence →
      evidence.kind = EvidenceKind.principalResponse →
      evidence.subject = EvidenceSubject.principal principal →
      ∃ comment,
        run.baseRun.event evidence.observedAt =
          some (Event.commentPosted comment) ∧
        comment.author = principal

  administratorApprovalHasApprovedDecision :
    ∀ program run evidence administrator reviewTask,
      E.validEvidence program run evidence →
      evidence.kind = EvidenceKind.administratorApproval →
      evidence.subject =
        EvidenceSubject.administratorDecision administrator reviewTask →
      ∃ decision,
        decision ∈ (run.semanticState evidence.observedAt).administratorGoalDecisions ∧
        decision.administrator = administrator ∧
        decision.reviewTask = reviewTask ∧
        decision.decision = AdministratorDecision.approved ∧
        AdministratorGoalDecisionValid
          (run.semanticState evidence.observedAt)
          decision

/-! ### R5.10 — Birth completeness e discharge soundness -/

/-- Una specifica è attualmente attivata e necessaria alla chiusura. -/
def RequiredSpecAt
    (s : SemanticState V X)
    (spec : ObligationSpec V X) : Prop :=
  spec.activationCondition s.base ∧
  spec.requiredForCompletion s.base

/--
Nessuna obligation attivata e richiesta dal programma può essere semplicemente
omessa. Le obligation condizionali non ancora attivate non sono istanziate
forzatamente.
-/
def AllRequiredObligationsInstantiated
    (s : SemanticState V X)
    (runId : X.RunId)
    (program : SemanticProgram V X) : Prop :=
  ∀ spec,
    spec ∈ program.obligationSpecs →
    RequiredSpecAt s spec →
    ∃ obligationInstance,
      s.obligations spec.id = some obligationInstance ∧
      obligationInstance.run = runId ∧
      obligationInstance.spec = spec.id ∧
      obligationInstance.owner = spec.owner

/-- Tutte le obligation attualmente richieste risultano discharged. -/
def AllRequiredObligationsDischarged
    (s : SemanticState V X)
    (program : SemanticProgram V X) : Prop :=
  ∀ spec,
    spec ∈ program.obligationSpecs →
    RequiredSpecAt s spec →
    ObligationDischarged s spec.id

/--
Birth progress: quando una specification diventa richiesta, deve eventualmente
esistere la relativa istanza. Questo evita di pretendere nascita istantanea
nello stesso tick.
-/
def ObligationBirthProgress
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X) : Prop :=
  ∀ n spec,
    spec ∈ program.obligationSpecs →
    RequiredSpecAt (run.semanticState n) spec →
    EventuallyAfter n (fun m =>
      ∃ obligationInstance,
        (run.semanticState m).obligations spec.id = some obligationInstance ∧
        obligationInstance.run = runId ∧
        obligationInstance.spec = spec.id ∧
        obligationInstance.owner = spec.owner)


def ObligationBirthProgressAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop :=
  ∀ n spec,
    start ≤ n →
    spec ∈ program.obligationSpecs →
    RequiredSpecAt (run.semanticState n) spec →
    EventuallyAfter n (fun m =>
      ∃ obligationInstance,
        (run.semanticState m).obligations spec.id = some obligationInstance ∧
        obligationInstance.run = runId ∧
        obligationInstance.spec = spec.id ∧
        obligationInstance.owner = spec.owner)

/-- Ogni discharge deve essere sostenuto da evidence semanticamente valida. -/
def DischargeSoundness
    (E : EvidenceSemantics V X)
    (program : SemanticProgram V X)
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n obligation obligationInstance,
    (run.semanticState n).obligations obligation = some obligationInstance →
    obligationInstance.status = ObligationStatus.discharged →
    ∃ evidence,
      evidence ∈ (run.semanticState n).evidences ∧
      evidence.obligation = obligation ∧
      E.validEvidence program run evidence

/-- Evidence valida persistita deve eventualmente chiudere l'obligation. -/
def DischargeProgress
    (E : EvidenceSemantics V X)
    (program : SemanticProgram V X)
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n evidence,
    evidence ∈ (run.semanticState n).evidences →
    E.validEvidence program run evidence →
    EventuallyAfter n (fun m =>
      ObligationDischarged (run.semanticState m) evidence.obligation)

/-! ### R5.10A — Risoluzione delle attese -/

/--
Semantica astratta di risoluzione. Il refinement concreto può usare:
* toolCompleted/toolFailed/toolTimedOut per toolTerminal;
* commentPosted dell'esatto principal per principalResponse;
* DoneTask per taskCompleted;
* discharge semantico per obligationDischarged;
* commento/atto dell'amministratore per administratorApproval;
* osservazioni dell'ambiente per externalOutcome/derivedCondition.
-/
structure WaitingSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  validResolution :
    ObservedSemanticRun V X →
    Blocker V X →
    BlockerResolution V X →
    Prop

structure WaitingSemanticsLaws
    (W : WaitingSemantics V X) : Prop where
  resolutionMatchesBlocker :
    ∀ run blocker resolution,
      W.validResolution run blocker resolution →
      resolution.blocker = blocker.id

  toolResolutionObserved :
    ∀ run blocker resolution callId,
      blocker.condition = WaitingCondition.toolTerminal callId →
      W.validResolution run blocker resolution →
      ToolTerminalEventAt run.baseRun callId resolution.observedAt

  principalResponseObserved :
    ∀ run blocker resolution principal,
      blocker.condition = WaitingCondition.principalResponse principal →
      W.validResolution run blocker resolution →
      ∃ comment,
        run.baseRun.event resolution.observedAt =
          some (Event.commentPosted comment) ∧
        comment.author = principal

  taskCompletionObserved :
    ∀ run blocker resolution task,
      blocker.condition = WaitingCondition.taskCompleted task →
      W.validResolution run blocker resolution →
      DoneTask (run.baseRun.state resolution.observedAt) task

  obligationResolutionObserved :
    ∀ run blocker resolution obligation,
      blocker.condition = WaitingCondition.obligationDischarged obligation →
      W.validResolution run blocker resolution →
      ObligationDischarged (run.semanticState resolution.observedAt) obligation

  administratorApprovalObserved :
    ∀ run blocker resolution administrator,
      blocker.condition =
        WaitingCondition.administratorApproval administrator →
      W.validResolution run blocker resolution →
      HasKind
        (run.semanticState resolution.observedAt).base
        administrator
        PrincipalKind.administrator ∧
      ∃ comment,
        run.baseRun.event resolution.observedAt =
          some (Event.commentPosted comment) ∧
        comment.author = administrator

/-! ### R5.11 — Work causale ed eleggibilità -/

/-- Il work item serve causalmente l'istanza indicata. -/
def WorkServesObligation
    (work : WorkItem V X)
    (obligation : ObligationInstance V X) : Prop :=
  work.run = obligation.run ∧
  work.owner = obligation.owner ∧
  work.serves = obligation.spec

/-- Il work deve appartenere davvero a una obligation del programma corrente. -/
def WorkMatchesProgram
    (program : SemanticProgram V X)
    (work : WorkItem V X) : Prop :=
  ∃ spec,
    spec ∈ program.obligationSpecs ∧
    spec.id = work.serves ∧
    spec.goal = work.goal ∧
    spec.owner = work.owner

/-- Un blocker si applica al work item secondo il proprio scope. -/
def BlockerAppliesToWork
    (blocker : Blocker V X)
    (work : WorkItem V X) : Prop :=
  match blocker.scope with
  | BlockScope.work workId =>
      work.id = workId
  | BlockScope.obligation obligation =>
      work.serves = obligation
  | BlockScope.goal goal =>
      work.goal = goal

/-- Esiste almeno un blocker irrisolto che vieta l'esecuzione del work. -/
def WorkBlocked
    (s : SemanticState V X)
    (work : WorkItem V X) : Prop :=
  ∃ blockerId blocker,
    s.blockers blockerId = some blocker ∧
    blocker.run = work.run ∧
    blocker.goal = work.goal ∧
    BlockerAppliesToWork blocker work ∧
    BlockerWaiting blocker

/--
Eleggibilità semantica astratta. Un refinement può raffinarla tramite permessi,
availability, dependency, retry policy, task state e scheduler concreto.

La novità R5 è normativa: un work con blocker irrisolto NON è eleggibile.
L'attesa è quindi obbligatoria per quel lavoro, non un semplice noOp opzionale.
-/
def WorkEligible
    (s : SemanticState V X)
    (program : SemanticProgram V X)
    (obligation : ObligationInstance V X)
    (work : WorkItem V X) : Prop :=
  obligation.status = ObligationStatus.active ∧
  DependencyClosed s program obligation ∧
  WorkServesObligation work obligation ∧
  WorkMatchesProgram program work ∧
  work.attempt < work.maxAttempts ∧
  work.status = WorkStatus.eligible ∧
  ¬ WorkBlocked s work

/-- Obligation minimalmente abilitata rispetto alle dependency. -/
def MinimalActiveObligation
    (s : SemanticState V X)
    (program : SemanticProgram V X)
    (obligation : ObligationInstance V X) : Prop :=
  obligation.status = ObligationStatus.active ∧
  DependencyClosed s program obligation

/--
Un goal incompleto non può contenere una obligation minimalmente attiva ma
inerte. La frontiera di progresso deve essere in uno dei due stati:
* lavoro immediatamente eligible;
* lavoro esplicitamente blocked da una condizione osservabile.

Questo è il punto formale che consente attese obbligatorie senza confonderle
con starvation.
-/
def WorkExistence
    (s : SemanticState V X)
    (program : SemanticProgram V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  ∀ obligation,
    obligation.run = runId →
    MinimalActiveObligation s program obligation →
    ¬ GoalCompleted s goal →
    ∃ work,
      s.workItems work.id = some work ∧
      WorkServesObligation work obligation ∧
      (WorkEligible s program obligation work ∨ WorkBlocked s work)

/-! ### R5.12 — Claim, esclusività e recovery -/

/-- Validità astratta di una lease/claim corrente. -/
structure ClaimSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  validClaim : SemanticState V X → Claim V X → Prop
  expired : SemanticState V X → Claim V X → Prop

/-- Al massimo un claimant valido per item e attempt. -/
def ExclusiveValidClaims
    (C : ClaimSemantics V X)
    (s : SemanticState V X) : Prop :=
  ∀ first second,
    C.validClaim s first →
    C.validClaim s second →
    first.work = second.work →
    first.attempt = second.attempt →
    first = second

/-- Una claim scaduta non autorizza un terminal effect nel refinement concreto. -/
def ExpiredClaimCannotAuthorize
    (C : ClaimSemantics V X)
    (s : SemanticState V X)
    (claim : Claim V X) : Prop :=
  C.expired s claim → ¬ C.validClaim s claim

/-- Coerenza minima fra work item, dispatch e claim persistenti. -/
structure PersistentSchedulerSafety
    (C : ClaimSemantics V X)
    (run : ObservedSemanticRun V X)
    (program : SemanticProgram V X) : Prop where
  exclusiveClaims :
    ∀ n, ExclusiveValidClaims C (run.semanticState n)

  validClaimMatchesPersistentWork :
    ∀ n claim,
      C.validClaim (run.semanticState n) claim →
      ∃ work dispatch,
        (run.semanticState n).workItems claim.work = some work ∧
        (run.semanticState n).dispatches claim.work = some dispatch ∧
        dispatch.work = claim.work ∧
        dispatch.attempt = claim.attempt ∧
        work.attempt = claim.attempt ∧
        work.status = WorkStatus.claimed ∧
        dispatch.status = DispatchStatus.claimed

  readyDispatchRequiresEligibleWork :
    ∀ n workId dispatch,
      (run.semanticState n).dispatches workId = some dispatch →
      dispatch.status = DispatchStatus.ready →
      ∃ work obligation,
        (run.semanticState n).workItems workId = some work ∧
        (run.semanticState n).obligations work.serves = some obligation ∧
        WorkEligible (run.semanticState n) program obligation work

  expiredClaimInvalid :
    ∀ n claim,
      C.expired (run.semanticState n) claim →
      ¬ C.validClaim (run.semanticState n) claim

/--
Versione segment-aware della safety della coda. Serve quando il programma
semantico corrente è diventato stabile soltanto dopo una GoalRevision.
-/
structure PersistentSchedulerSafetyAfter
    (C : ClaimSemantics V X)
    (run : ObservedSemanticRun V X)
    (program : SemanticProgram V X)
    (start : Nat) : Prop where
  exclusiveClaims :
    ∀ n,
      start ≤ n →
      ExclusiveValidClaims C (run.semanticState n)

  validClaimMatchesPersistentWork :
    ∀ n claim,
      start ≤ n →
      C.validClaim (run.semanticState n) claim →
      ∃ work dispatch,
        (run.semanticState n).workItems claim.work = some work ∧
        (run.semanticState n).dispatches claim.work = some dispatch ∧
        dispatch.work = claim.work ∧
        dispatch.attempt = claim.attempt ∧
        work.attempt = claim.attempt ∧
        work.status = WorkStatus.claimed ∧
        dispatch.status = DispatchStatus.claimed

  readyDispatchRequiresEligibleWork :
    ∀ n workId dispatch,
      start ≤ n →
      (run.semanticState n).dispatches workId = some dispatch →
      dispatch.status = DispatchStatus.ready →
      ∃ work obligation,
        (run.semanticState n).workItems workId = some work ∧
        (run.semanticState n).obligations work.serves = some obligation ∧
        WorkEligible (run.semanticState n) program obligation work

  expiredClaimInvalid :
    ∀ n claim,
      start ≤ n →
      C.expired (run.semanticState n) claim →
      ¬ C.validClaim (run.semanticState n) claim

/--
Recovery generale: una claim scaduta deve eventualmente lasciare lo stesso
WorkItem nuovamente dispatchabile, oppure portarlo in uno stato terminale.
Non viene creata una nuova identità semantica del lavoro.
-/
def ClaimRecoveryProgress
    (C : ClaimSemantics V X)
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n claim work,
    C.expired (run.semanticState n) claim →
    (run.semanticState n).workItems claim.work = some work →
    EventuallyAfter n (fun m =>
      (∃ later dispatch,
        (run.semanticState m).workItems claim.work = some later ∧
        Recoverable work later ∧
        (run.semanticState m).dispatches claim.work = some dispatch ∧
        dispatch.status = DispatchStatus.ready) ∨
      (∃ later,
        (run.semanticState m).workItems claim.work = some later ∧
        Recoverable work later ∧
        (later.status = WorkStatus.succeeded ∨
         later.status = WorkStatus.failed ∨
         later.status = WorkStatus.cancelled)))

/-! ### R5.13 — Continuità esplicita con tool semantics R4 -/

/--
Una ToolCall R4 può essere collegata causalmente a un work item R5 senza
modificare ToolCallRecord R4.
-/
structure ToolWorkLink (V : Vocabulary) (X : ExtensionVocabulary V) where
  callId : V.ToolCallId
  workId : X.WorkItemId

/--
Il refinement concreto deve garantire che apertura/retry R4 siano collegati a
work R5 senza indebolire ToolReady, RetryEligible o gli effetti R4.
-/
def ToolWorkConsistent
    (s : SemanticState V X)
    (link : ToolWorkLink V X) : Prop :=
  (∃ call, s.base.toolCalls link.callId = some call) ∧
  (∃ work, s.workItems link.workId = some work ∧
    (work.kind = WorkKind.toolInvocation ∨ work.kind = WorkKind.toolRetry))

/--
Continuità normativa R4.1: il completamento di una call già pending resta
indipendente dalla runtime availability corrente. Questa proprietà rimane
quella già dimostrata da pending_tool_completion_activates in R4.
-/
theorem r4_pending_completion_preserved
    (s : State V)
    (profile : AgentProfile V)
    (callId : V.ToolCallId)
    (output : V.ToolOutput)
    (h : PendingToolCallOwnedBy s profile.principal callId) :
    Activates s profile (Event.toolCompleted callId output) := by
  exact pending_tool_completion_activates s profile callId output h

/-! ### R5.14 — Scheduler su work item e bridge verso fairness R4 -/

/-- Un work resta continuously eligible finché non viene selezionato. -/
def ContinuouslyEligibleAfter
    (run : ObservedSemanticRun V X)
    (program : SemanticProgram V X)
    (obligation : ObligationInstance V X)
    (work : WorkItem V X)
    (n : Nat) : Prop :=
  ∀ m,
    n ≤ m →
    WorkEligible (run.semanticState m) program obligation work

/-- Osservazione astratta della selezione di un work item. -/
def SelectedAt
    (run : ObservedSemanticRun V X)
    (work : X.WorkItemId)
    (n : Nat) : Prop :=
  ∃ claimId,
    run.semanticEvent n = some (SemanticEvent.workClaimed work claimId)

/--
Disciplina di attesa obbligatoria: un work blocked non può essere selezionato.
L'agente può però continuare su altri work item indipendenti che siano eligible.
Una barriera di scope goal blocca invece tutti i work del goal a cui si applica.
-/
def BlockedWorkNotSelected
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n workId work,
    (run.semanticState n).workItems workId = some work →
    WorkBlocked (run.semanticState n) work →
    ¬ SelectedAt run workId n

/--
Liveness dei blocker: una condizione che rimane waiting deve eventualmente
raggiungere uno stato terminale. Questa proprietà è condizionale e richiede
progress dell'altro agente/utente/tool/task/ambiente coinvolto.
-/
def BlockerProgress
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n blockerId blocker,
    (run.semanticState n).blockers blockerId = some blocker →
    BlockerWaiting blocker →
    EventuallyAfter n (fun m =>
      ∃ later,
        (run.semanticState m).blockers blockerId = some later ∧
        BlockerTerminal later)

/-- Weak fairness per work agentico. -/
def AgentWorkWeakFairness
    (run : ObservedSemanticRun V X)
    (program : SemanticProgram V X) : Prop :=
  ∀ obligation work n,
    work.kind = WorkKind.agentAction →
    ContinuouslyEligibleAfter run program obligation work n →
    EventuallyAfter n (fun m => SelectedAt run work.id m)

/-- Weak fairness per work del runtime/tool. -/
def RuntimeWorkWeakFairness
    (run : ObservedSemanticRun V X)
    (program : SemanticProgram V X) : Prop :=
  ∀ obligation work n,
    (work.kind = WorkKind.toolInvocation ∨ work.kind = WorkKind.toolRetry) →
    ContinuouslyEligibleAfter run program obligation work n →
    EventuallyAfter n (fun m => SelectedAt run work.id m)

/--
Bridge necessario per non sostituire silenziosamente AgentSchedulerWeakFairness
R4 con la fairness sui work item R5.
-/
structure SchedulerRefinement
    (P : PromptSemantics V)
    (directory : AgentDirectory V)
    (run : ObservedSemanticRun V X)
    (program : SemanticProgram V X) : Prop where
  r4AgentFairness :
    AgentSchedulerWeakFairness P directory run.baseRun

  r4RuntimeFairness :
    RuntimeWeakFairness run.baseRun

  r5AgentWorkFairness :
    AgentWorkWeakFairness run program

  r5RuntimeWorkFairness :
    RuntimeWorkWeakFairness run program

/-! ### R5.15 — Priority e anti-starvation -/

/--
R4 CommentPriorityDiscipline resta normativa. R5 aggiunge soltanto il requisito
che la priorità non possa essere implementata come starvation permanente.
-/
def PriorityDoesNotStarve
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ work n,
    (∃ item,
      (run.semanticState n).workItems work = some item ∧
      item.status = WorkStatus.eligible) →
    (∀ m, n ≤ m →
      ∃ item,
        (run.semanticState m).workItems work = some item ∧
        item.status = WorkStatus.eligible) →
    EventuallyAfter n (fun m => SelectedAt run work m)

/-! ### R5.16 — Completion criterion e distinzione RunCompleted ≠ GoalCompleted -/

/--
Criterio minimo derivato dal programma: tutte le obligation richieste sono
state istanziate e discharged. Un compiler più ricco può raffinare questa
condizione, ma non indebolirla senza prova di equivalenza.
-/
def NoOpenGoalRelevantWork
    (s : SemanticState V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  ∀ workId work,
    s.workItems workId = some work →
    work.run = runId →
    work.goal = goal →
    (work.status = WorkStatus.succeeded ∨
     work.status = WorkStatus.failed ∨
     work.status = WorkStatus.cancelled)

def NoWaitingGoalBlockers
    (s : SemanticState V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  ∀ blockerId blocker,
    s.blockers blockerId = some blocker →
    blocker.run = runId →
    blocker.goal = goal →
    BlockerTerminal blocker

def CompletionCriterion
    (s : SemanticState V X)
    (runId : X.RunId)
    (program : SemanticProgram V X) : Prop :=
  program.completionGuard s.base ∧
  AllRequiredObligationsInstantiated s runId program ∧
  AllRequiredObligationsDischarged s program ∧
  NoOpenGoalRelevantWork s runId program.goal.id ∧
  NoWaitingGoalBlockers s runId program.goal.id

/--
La chiusura operativa della run non implica da sola il completamento semantico.
Il bridge è una proprietà separata.
-/
def RunCompletionSoundness
    (s : SemanticState V X)
    (runId : X.RunId)
    (program : SemanticProgram V X) : Prop :=
  RunCompleted s runId →
  CompletionCriterion s runId program →
  GoalCompleted s program.goal.id

/-- Tutte le obligation discharged devono essere sufficienti al goal. -/
def ProgramCompletionSoundness
    (s : SemanticState V X)
    (runId : X.RunId)
    (program : SemanticProgram V X) : Prop :=
  CompletionCriterion s runId program →
  GoalValid s program.goal.id →
  GoalCompleted s program.goal.id

/-! ### R5.17 — Continuità con PromptSemantics R4 -/

/--
Relazione di refinement: la semantica statica R5 deve essere compatibile con
la PromptSemantics R4 consumata dalle regole di autorizzazione e liveness.
-/
def RefinesR4PromptSemantics
    (P : PromptSemantics V)
    (prompt : V.SystemPrompt)
    (program : SemanticProgram V X)
    (run : ObservedSemanticRun V X) : Prop :=
  (∀ n action,
    P.serves prompt (run.baseRun.state n) action ↔
      (action ≠ AgentAction.noOp ∧
       program.actionSupports (run.baseRun.state n) action)) ∧
  (∀ n,
    P.mayWait prompt (run.baseRun.state n) ↔
      program.waitingAllowed (run.baseRun.state n)) ∧
  (∀ n obligation,
    P.obligation prompt (run.baseRun.state n) obligation ↔
      ∃ obligationInstance,
        (run.semanticState n).obligations obligation = some obligationInstance ∧
        Instantiates program obligationInstance ∧
        obligationInstance.status = ObligationStatus.active) ∧
  (∀ n obligation,
    P.discharged prompt (run.baseRun.state n) obligation ↔
      ObligationDischarged (run.semanticState n) obligation)

/-!
Questa relazione è il punto di continuità R4.2 → R5: il nuovo programma
statico non cancella PromptSemantics R4; deve raffinarlo lungo la run.
-/

/-! ### R5.18 — Continuità con causalità task e liveness R4 -/

/-
R5 non sostituisce TaskCompletionCausality. Il refinement deve conservarla
letteralmente sulla baseRun.
-/

/-!
### R5.17A — Prompt/program refinement dinamico attraverso le revisioni

La relazione R5.17 con un prompt fisso resta valida per segmenti stabili.
Una run che ammette GoalRevision deve però seguire il SystemPrompt effettivo
osservato a ogni tick.
-/

def PromptActiveAt
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X)
    (n : Nat)
    (prompt : V.SystemPrompt) : Prop :=
  (run.baseRun.state n).systemPrompts profile.principal = some prompt

def DynamicPromptRefinement
    (P : PromptSemantics V)
    (compiler : SemanticCompiler V X)
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n prompt,
    PromptActiveAt profile run n prompt →
    (∀ action,
      P.serves prompt (run.baseRun.state n) action ↔
        (action ≠ AgentAction.noOp ∧
         (compiler.compile prompt).actionSupports (run.baseRun.state n) action)) ∧
    (P.mayWait prompt (run.baseRun.state n) ↔
      (compiler.compile prompt).waitingAllowed (run.baseRun.state n)) ∧
    (∀ obligation,
      P.obligation prompt (run.baseRun.state n) obligation ↔
        ∃ obligationInstance,
          (run.semanticState n).obligations obligation = some obligationInstance ∧
          Instantiates (compiler.compile prompt) obligationInstance ∧
          obligationInstance.status = ObligationStatus.active) ∧
    (∀ obligation,
      P.discharged prompt (run.baseRun.state n) obligation ↔
        ObligationDischarged (run.semanticState n) obligation)

def PromptStableFrom
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X)
    (prompt : V.SystemPrompt)
    (start : Nat) : Prop :=
  ∀ n, start ≤ n → PromptActiveAt profile run n prompt

def DynamicAgentWorkWeakFairness
    (compiler : SemanticCompiler V X)
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ prompt obligation work n,
    PromptStableFrom profile run prompt n →
    (work.kind = WorkKind.agentAction ∨
     work.kind = WorkKind.taskAction ∨
     work.kind = WorkKind.coordination) →
    ContinuouslyEligibleAfter run (compiler.compile prompt) obligation work n →
    EventuallyAfter n (fun m => SelectedAt run work.id m)

def DynamicRuntimeWorkWeakFairness
    (compiler : SemanticCompiler V X)
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ prompt obligation work n,
    PromptStableFrom profile run prompt n →
    (work.kind = WorkKind.toolInvocation ∨ work.kind = WorkKind.toolRetry) →
    ContinuouslyEligibleAfter run (compiler.compile prompt) obligation work n →
    EventuallyAfter n (fun m => SelectedAt run work.id m)

structure DynamicSchedulerRefinement
    (P : PromptSemantics V)
    (directory : AgentDirectory V)
    (compiler : SemanticCompiler V X)
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X) : Prop where
  r4AgentFairness :
    AgentSchedulerWeakFairness P directory run.baseRun
  r4RuntimeFairness :
    RuntimeWeakFairness run.baseRun
  r5AgentWorkFairness :
    DynamicAgentWorkWeakFairness compiler profile run
  r5RuntimeWorkFairness :
    DynamicRuntimeWorkWeakFairness compiler profile run

def PreservesR4TaskCausality
    (run : ObservedSemanticRun V X) : Prop :=
  TaskCompletionCausality run.baseRun

/--
Analogamente, AssignedTaskLiveness resta un requisito R4 indipendente dal nuovo
completion theorem del goal.
-/
def PreservesR4AssignedTaskLiveness
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X) : Prop :=
  AssignedTaskLiveness profile run.baseRun

/-! ### R5.18A — Autorità e persistenza del SemanticProgram -/

def ProgramSnapshotActive
    (s : SemanticState V X)
    (runId : X.RunId)
    (snapshot : ProgramSnapshot V X) : Prop :=
  snapshot ∈ s.programSnapshots ∧
  snapshot.run = runId ∧
  s.activeProgram runId = some snapshot.id ∧
  s.runGoal runId = some snapshot.program.goal.id


def WorkHasActiveProgram
    (s : SemanticState V X)
    (work : WorkItem V X) : Prop :=
  ∃ snapshot,
    ProgramSnapshotActive s work.run snapshot ∧
    snapshot.program.goal.id = work.goal

def SelectedWorkHasActiveProgram
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n workId work,
    (run.semanticState n).workItems workId = some work →
    SelectedAt run workId n →
    WorkHasActiveProgram (run.semanticState n) work

structure ProgramRevisionLaws
    (compiler : SemanticCompiler V X)
    (profile : AgentProfile V)
    (run : ObservedSemanticRun V X) : Prop where
  activeSnapshotMatchesObservedPrompt :
    ∀ n runId snapshot,
      ProgramSnapshotActive (run.semanticState n) runId snapshot →
      PromptActiveAt profile run n snapshot.prompt ∧
      snapshot.program = compiler.compile snapshot.prompt

  goalRevisionActivatesProgramRevision :
    ∀ n revision,
      run.semanticEvent n = some (SemanticEvent.goalRevised revision) →
      EventuallyAfter n (fun m =>
        ∃ snapshot,
          ProgramSnapshotActive (run.semanticState m) revision.run snapshot ∧
          snapshot.program.goal.id = revision.revisedGoal.id ∧
          snapshot.sourceGoalRevision = some revision.id)

  snapshotsAppendOnly :
    ∀ n m snapshot,
      n ≤ m →
      snapshot ∈ (run.semanticState n).programSnapshots →
      snapshot ∈ (run.semanticState m).programSnapshots


  selectedWorkUsesActiveProgram :
    SelectedWorkHasActiveProgram run

/-! ### R5.19A — Stabilità della revisione del goal -/

/--
Una revisione amministrativa è lecita, ma il teorema di completamento riguarda
una specifica revisione stabile del goal. Se l'amministratore modifica
indefinitamente il goal, non è corretto promettere il completamento della
revisione precedente.
-/
def GoalRevisionStableAfter
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  ∀ n revision,
    start ≤ n →
    run.semanticEvent n = some (SemanticEvent.goalRevised revision) →
    revision.previousGoal ≠ goal


/-! ### R5.19B — Predicati concreti per le assunzioni residue -/

def DependencyPrecedes
    (program : SemanticProgram V X)
    (prerequisite obligation : V.ObligationId) : Prop :=
  DependsOn program obligation prerequisite

def DependencyGraphWellFounded
    (program : SemanticProgram V X) : Prop :=
  WellFounded (DependencyPrecedes program)

def WorkParentPrecedes
    (s : SemanticState V X)
    (parent child : X.WorkItemId) : Prop :=
  ∃ childWork,
    s.workItems child = some childWork ∧
    childWork.parent = some parent

def WorkExpansionWellFoundedAt
    (s : SemanticState V X) : Prop :=
  /-
  IMPORTANT: la relazione usata da WellFounded deve essere orientata
  child→parent per escludere una catena infinita parent→child→grandchild...
  -/
  WellFounded (fun child parent => WorkParentPrecedes s parent child)

/--
Assunzione forte ma verificabile: durante il segmento considerato il numero
totale di identità WorkItem rilevanti al goal è finito. Un refinement futuro
può sostituirla con un ranking ordinale più generale, purché dimostri lo stesso
LocalProgress.
-/
def FiniteGoalRelevantWorkAcrossRun
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  ∃ ids : List X.WorkItemId,
    ∀ n workId work,
      (run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = goal →
      workId ∈ ids

def AdministratorRevisionEventuallyStable
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId) : Prop :=
  ∃ start, GoalRevisionStableAfter run goal start

def AdministratorDecisionProgress
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n escalation,
    escalation ∈ (run.semanticState n).administratorEscalations →
    AdministratorEscalationValid (run.semanticState n) escalation →
    EventuallyAfter n (fun m =>
      ∃ decision,
        decision ∈ (run.semanticState m).administratorGoalDecisions ∧
        decision.run = escalation.run ∧
        decision.goal = escalation.goal ∧
        decision.reviewTask = escalation.reviewTask ∧
        AdministratorGoalDecisionValid (run.semanticState m) decision)

def ExternalDependencyProgress
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n blockerId blocker condition,
    (run.semanticState n).blockers blockerId = some blocker →
    blocker.condition = WaitingCondition.externalOutcome condition ∨
    blocker.condition = WaitingCondition.derivedCondition condition →
    BlockerWaiting blocker →
    EventuallyAfter n (fun m =>
      ∃ later,
        (run.semanticState m).blockers blockerId = some later ∧
        BlockerTerminal later)

def CrossAgentDependencyProgress
    (run : ObservedSemanticRun V X)
    (program : SemanticProgram V X) : Prop :=
  ∀ n target prerequisite targetInstance sourceInstance,
    InterAgentDependency program target prerequisite →
    (run.semanticState n).obligations target = some targetInstance →
    (run.semanticState n).obligations prerequisite = some sourceInstance →
    targetInstance.status = ObligationStatus.active →
    sourceInstance.status = ObligationStatus.active →
    EventuallyAfter n (fun m =>
      ObligationDischarged (run.semanticState m) prerequisite)

def RetryWorkProgress
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n workId work,
    (run.semanticState n).workItems workId = some work →
    work.status = WorkStatus.failed →
    work.attempt + 1 < work.maxAttempts →
    EventuallyAfter n (fun m =>
      ∃ later,
        (run.semanticState m).workItems workId = some later ∧
        Recoverable work later ∧
        later.attempt = work.attempt + 1 ∧
        (later.status = WorkStatus.eligible ∨
         later.status = WorkStatus.blocked ∨
         later.status = WorkStatus.claimed))


/--
Per il theorem di SUCCESSO, un work definitivamente fallito non può essere
ignorato. Se i retry sono esauriti, deve eventualmente esistere un percorso
compensativo sulla stessa obligation oppure l'obligation deve essere discharged
da altra evidence valida. Un theorem separato potrebbe invece ammettere
GoalFailed come esito terminale.
-/
def ExhaustedFailureCompensation
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  ∀ n workId work,
    (run.semanticState n).workItems workId = some work →
    work.run = runId →
    work.goal = goal →
    work.status = WorkStatus.failed →
    work.maxAttempts ≤ work.attempt + 1 →
    EventuallyAfter n (fun m =>
      ObligationDischarged (run.semanticState m) work.serves ∨
      ∃ replacementId replacement,
        (run.semanticState m).workItems replacementId = some replacement ∧
        replacement.run = work.run ∧
        replacement.goal = work.goal ∧
        replacement.serves = work.serves ∧
        replacement.owner = work.owner ∧
        replacementId ≠ workId ∧
        (replacement.status = WorkStatus.eligible ∨
         replacement.status = WorkStatus.blocked ∨
         replacement.status = WorkStatus.claimed))

def ScopeAuthorizationPersistence
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  ∀ n m workId work,
    n ≤ m →
    (run.semanticState n).workItems workId = some work →
    work.run = runId →
    work.goal = goal →
    (run.semanticState m).runParticipants runId work.owner ∧
    (HasKind (run.semanticState m).base work.owner PrincipalKind.agent ∨
     HasKind (run.semanticState m).base work.owner PrincipalKind.user ∨
     HasKind (run.semanticState m).base work.owner PrincipalKind.administrator)


/-! ### R5.18B — Chiusura collaborativa su tutti i participant -/

/-- Un principal è un agente participant della run al tick corrente. -/
def ParticipatingAgentAt
    (s : SemanticState V X)
    (runId : X.RunId)
    (actor : V.PrincipalId) : Prop :=
  s.runParticipants runId actor ∧
  HasKind s.base actor PrincipalKind.agent

/--
Tutti gli agenti participant, non soltanto il profilo usato come controller
del programma semantico, devono soddisfare le discipline R4 dipendenti dal
profilo: responsiveness, priority, task liveness e prompt-obligation liveness.

`ResponsibleRun` contiene inoltre ValidRun e le fairness globali R4; richiederlo
per ogni agent participant rende esplicito che il completion theorem non è
locale a un singolo AgentProfile.
-/
def AllParticipatingAgentsResponsibleAfter
    (B : ApiBoundary V)
    (P : PromptSemantics V)
    (M : TransitionSystem V)
    (R : ResponseSemantics V)
    (directory : AgentDirectory V)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (start : Nat) : Prop :=
  ∀ n actor,
    start ≤ n →
    ParticipatingAgentAt (run.semanticState n) runId actor →
    ∃ participantProfile,
      directory actor = some participantProfile ∧
      participantProfile.principal = actor ∧
      ResponsibleRun B P M R directory participantProfile run.baseRun

/--
Chiusura degli owner: ogni obligation e ogni work rilevante al programma
collaborativo appartengono a un participant della stessa run.
-/
def CollaborativeParticipantClosureAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop :=
  (∀ n obligation obligationInstance,
    start ≤ n →
    (run.semanticState n).obligations obligation = some obligationInstance →
    Instantiates program obligationInstance →
    obligationInstance.run = runId →
    (run.semanticState n).runParticipants runId obligationInstance.owner) ∧
  (∀ n workId work,
    start ≤ n →
    (run.semanticState n).workItems workId = some work →
    work.run = runId →
    work.goal = program.goal.id →
    (run.semanticState n).runParticipants runId work.owner)

/-- Tipi di work che devono essere schedulati se persistentemente eligible. -/
def CollaborativeExecutableWorkKind (kind : WorkKind) : Prop :=
  kind = WorkKind.agentAction ∨
  kind = WorkKind.toolInvocation ∨
  kind = WorkKind.toolRetry ∨
  kind = WorkKind.taskAction ∨
  kind = WorkKind.coordination

/--
Fairness globale sui work item: quantifica il work graph dell'intera run/goal,
senza fissare un singolo owner. Quindi copre passaggi A→B→C fra agenti diversi.
-/
def CollaborativeWorkWeakFairnessAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop :=
  ∀ obligation work n,
    start ≤ n →
    work.run = runId →
    work.goal = program.goal.id →
    CollaborativeExecutableWorkKind work.kind →
    ContinuouslyEligibleAfter run program obligation work n →
    EventuallyAfter n (fun m => SelectedAt run work.id m)

/-! ### R5.18C — Global Multi-Agent Anti-Loop -/

/--
Relazione causale globale ACROSS-RUN orientata come `successor-of`:
`GlobalCausalSuccessorOf run runId goal child parent` significa che, in qualche
tick della run, `parent` ha causato/abilitato `child`.

L'orientamento successor→predecessor è intenzionale: WellFounded su questa
relazione esclude una catena causale infinita
N0 → N1 → N2 → ... di nuovo lavoro/reazioni.
-/
def GlobalCausalSuccessorOf
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (successor predecessor : CollaborativeCausalNode V X) : Prop :=
  ∃ n link,
    link ∈ (run.semanticState n).causalLinks ∧
    link.run = runId ∧
    link.goal = goal ∧
    link.predecessor = predecessor ∧
    link.successor = successor

def GlobalCausalGraphWellFounded
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  WellFounded (GlobalCausalSuccessorOf run runId goal)

/--
Completezza minima del grafo causale. I link devono coprire almeno:
* dependency fra obligation;
* obligation→work che la serve;
* parent-work→child-work;
* comment→work quando sourceComment è presente.

Il refinement concreto deve inoltre registrare nei causalLinks ogni ulteriore
handoff inter-agent che generi task, commenti, obligation, retry o nuovo work.
-/
def CollaborativeCausalLinkCompletenessAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop :=
  (∀ target prerequisite,
    DependsOn program target prerequisite →
    GlobalCausalSuccessorOf
      run runId program.goal.id
      (CollaborativeCausalNode.obligation target)
      (CollaborativeCausalNode.obligation prerequisite)) ∧
  (∀ n workId work,
    start ≤ n →
    (run.semanticState n).workItems workId = some work →
    work.run = runId →
    work.goal = program.goal.id →
    GlobalCausalSuccessorOf
      run runId program.goal.id
      (CollaborativeCausalNode.work workId)
      (CollaborativeCausalNode.obligation work.serves)) ∧
  (∀ n childId child parentId,
    start ≤ n →
    (run.semanticState n).workItems childId = some child →
    child.run = runId →
    child.goal = program.goal.id →
    child.parent = some parentId →
    GlobalCausalSuccessorOf
      run runId program.goal.id
      (CollaborativeCausalNode.work childId)
      (CollaborativeCausalNode.work parentId)) ∧
  (∀ n workId work commentId,
    start ≤ n →
    (run.semanticState n).workItems workId = some work →
    work.run = runId →
    work.goal = program.goal.id →
    work.sourceComment = some commentId →
    GlobalCausalSuccessorOf
      run runId program.goal.id
      (CollaborativeCausalNode.work workId)
      (CollaborativeCausalNode.comment commentId))

/--
Anti-loop globale del SISTEMA COLLABORATIVO.

Non è un limite locale su un singolo agente. Congiunge:
1. dependency graph ben fondato;
2. causal graph across-run ben fondato;
3. work rilevante globalmente finito nel segmento stabile;
4. work expansion locale ben fondata;
5. causal links sufficientemente completi da non nascondere handoff.

In particolare, una sequenza infinita A→B→C→A→... che continui a generare
nuovo work/comment/task/obligation rilevante al goal viola almeno la finitezza
globale o la well-foundedness causale e quindi non soddisfa queste assunzioni.
-/
structure GlobalMultiAgentAntiLoop
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop where
  dependencyGraph :
    DependencyGraphWellFounded program

  causalGraph :
    GlobalCausalGraphWellFounded run runId program.goal.id

  finiteRelevantWork :
    FiniteGoalRelevantWorkAcrossRun run runId program.goal.id

  workExpansion :
    ∀ n,
      start ≤ n →
      WorkExpansionWellFoundedAt (run.semanticState n)

  causalCompleteness :
    CollaborativeCausalLinkCompletenessAfter run runId program start

/-! ### R5.19 — Run semanticamente responsabile -/

structure SemanticResponsibleRun
    (B : ApiBoundary V)
    (P : PromptSemantics V)
    (M : TransitionSystem V)
    (R : ResponseSemantics V)
    (directory : AgentDirectory V)
    (profile : AgentProfile V)
    (compiler : SemanticCompiler V X)
    (run : ObservedSemanticRun V X) : Prop where

  projectsToR4 :
    ProjectsToR4 run.toSemanticRun

  r4Responsible :
    ResponsibleRun B P M R directory profile run.baseRun

  compilerLaws :
    SemanticCompilerLaws compiler

  promptRefinement :
    DynamicPromptRefinement P compiler profile run

  schedulerRefinement :
    DynamicSchedulerRefinement P directory compiler profile run

  programRevisionLaws :
    ProgramRevisionLaws compiler profile run

  priorityAntiStarvation :
    PriorityDoesNotStarve run

  goalRevisionLaws :
    GoalRevisionLaws run

  goalEscalationLaws :
    GoalEscalationLaws run

  blockedWorkDiscipline :
    BlockedWorkNotSelected run

/--
Versione esplicitamente collaborativa della run responsabile.
`controllerProfile` è soltanto il profilo che lega il programma/prompt del
segmento; NON limita il dominio del theorem. Tutti gli agent participant sono
quantificati separatamente tramite `allParticipatingAgentsResponsible`.
-/
structure CollaborativeSemanticResponsibleRun
    (B : ApiBoundary V)
    (P : PromptSemantics V)
    (M : TransitionSystem V)
    (R : ResponseSemantics V)
    (directory : AgentDirectory V)
    (controllerProfile : AgentProfile V)
    (compiler : SemanticCompiler V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop where
  controllerLayer :
    SemanticResponsibleRun
      B P M R directory controllerProfile compiler run

  allParticipatingAgentsResponsible :
    AllParticipatingAgentsResponsibleAfter
      B P M R directory run runId start

  participantClosure :
    CollaborativeParticipantClosureAfter
      run runId program start

  globalWorkFairness :
    CollaborativeWorkWeakFairnessAfter
      run runId program start

  globalAntiLoop :
    GlobalMultiAgentAntiLoop
      run runId program start

/-! ### R5.20 — Progress measure reale e work expansion -/

/--
La misura è fornita dal refinement e deve rappresentare il work graph reale.
R5 non identifica il rango con il mero numero di obligation attive.
-/
structure ProgressMeasure
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  rank : SemanticState V X → X.RunId → X.GoalId → Nat

/-- Progresso stretto fra due tick. -/
def StrictProgress
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (fromTick toTick : Nat) : Prop :=
  fromTick < toTick ∧
  measure.rank (run.semanticState toTick) runId goal <
  measure.rank (run.semanticState fromTick) runId goal

/-- Persistenza della validità del goal durante il tratto considerato. -/
def GoalValidityPersists
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId) : Prop :=
  ∀ n m,
    n ≤ m →
    GoalValid (run.semanticState n) goal →
    GoalValid (run.semanticState m) goal

/-- Il completamento del goal è stabile. -/
def GoalCompletionStable
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId) : Prop :=
  ∀ n m,
    n ≤ m →
    GoalCompleted (run.semanticState n) goal →
    GoalCompleted (run.semanticState m) goal

/--
Lemma-obiettivo locale: da uno stato valido e non completo si raggiunge in un
numero finito di tick il completamento oppure un rango strettamente minore.
-/
def LocalProgress
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId) : Prop :=
  ∀ n,
    GoalValid (run.semanticState n) goal →
    ¬ GoalCompleted (run.semanticState n) goal →
    ∃ m,
      n < m ∧
      (GoalCompleted (run.semanticState m) goal ∨
       measure.rank (run.semanticState m) runId goal <
       measure.rank (run.semanticState n) runId goal)

/-- Eventuale completamento semantico del goal. -/
def EventuallyGoalCompleted
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  ∃ m,
    start ≤ m ∧
    GoalCompleted (run.semanticState m) goal

/-! ### R5.21A — Segmento stabile dopo una revisione -/

/--
LocalProgressAfter evita di chiedere progresso rispetto a un programma che è
già stato superseded. Il completion theorem applicativo deve partire da un
tick in cui goal e prompt/program sono stabili.
-/
def LocalProgressAfter
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  ∀ n,
    start ≤ n →
    GoalValid (run.semanticState n) goal →
    ¬ GoalCompleted (run.semanticState n) goal →
    ∃ m,
      n < m ∧
      (GoalCompleted (run.semanticState m) goal ∨
       measure.rank (run.semanticState m) runId goal <
       measure.rank (run.semanticState n) runId goal)


/-! ### R5.21B — Completion del sistema collaborativo nel suo complesso -/

/--
Completion GLOBALE del sistema collaborativo per la run/goal:
non basta `GoalStatus.completed`; deve valere anche il CompletionCriterion,
che quantifica tutte le obligation richieste, tutti i work item e tutti i
blocker rilevanti al goal indipendentemente dal loro owner.
-/
def CollaborativeSystemCompletedAt
    (s : SemanticState V X)
    (runId : X.RunId)
    (program : SemanticProgram V X) : Prop :=
  GoalCompleted s program.goal.id ∧
  CompletionCriterion s runId program

def EventuallyCollaborativeSystemCompleted
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop :=
  ∃ m,
    start ≤ m ∧
    CollaborativeSystemCompletedAt (run.semanticState m) runId program

/--
Local progress GLOBALE: se il sistema collaborativo non è ancora completo,
l'intero residual rank del goal deve diminuire oppure il sistema completo
(tutti gli agenti/work/obligation/blocker rilevanti) deve essere raggiunto.
-/
def CollaborativeLocalProgressAfter
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (start : Nat) : Prop :=
  ∀ n,
    start ≤ n →
    GoalValid (run.semanticState n) program.goal.id →
    ¬ CollaborativeSystemCompletedAt (run.semanticState n) runId program →
    ∃ m,
      n < m ∧
      (CollaborativeSystemCompletedAt (run.semanticState m) runId program ∨
       measure.rank (run.semanticState m) runId program.goal.id <
       measure.rank (run.semanticState n) runId program.goal.id)

/-!
### R5.21 — Teorema generale di completamento

Questo teorema non assume direttamente fairness o successo del provider.
Tali ipotesi devono essere utilizzate nel futuro refinement per dimostrare
LocalProgress. Il teorema globale usa soltanto la discesa ben fondata su Nat.
-/
theorem goal_completion_from_well_founded_progress
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (validity : GoalValidityPersists run goal)
    (progress : LocalProgress measure run runId goal)
    (start : Nat)
    (validStart : GoalValid (run.semanticState start) goal) :
    EventuallyGoalCompleted run goal start := by

  let initialRank := measure.rank (run.semanticState start) runId goal

  have aux :
      ∀ rank n,
        measure.rank (run.semanticState n) runId goal = rank →
        GoalValid (run.semanticState n) goal →
        EventuallyGoalCompleted run goal n := by
    intro rank
    induction rank using Nat.strongRecOn with
    | ind rank ih =>
      intro n rankEq validN
      by_cases completeN : GoalCompleted (run.semanticState n) goal
      · exact ⟨n, Nat.le_refl n, completeN⟩
      · obtain ⟨m, nLtM, result⟩ := progress n validN completeN
        cases result with
        | inl completeM =>
            exact ⟨m, Nat.le_of_lt nLtM, completeM⟩
        | inr smallerRank =>
            have validM : GoalValid (run.semanticState m) goal :=
              validity n m (Nat.le_of_lt nLtM) validN
            have rankMlt :
                measure.rank (run.semanticState m) runId goal < rank := by
              simpa [rankEq] using smallerRank
            have recursive :=
              ih
                (measure.rank (run.semanticState m) runId goal)
                rankMlt
                m
                rfl
                validM
            obtain ⟨finish, mLeFinish, finishComplete⟩ := recursive
            exact
              ⟨finish,
               Nat.le_trans (Nat.le_of_lt nLtM) mLeFinish,
               finishComplete⟩

  exact aux initialRank start rfl validStart

/--
Versione segment-aware del teorema generale. È la forma da usare per Sprout
quando una run può contenere revisioni del goal.
-/
theorem goal_completion_from_well_founded_progress_after
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (validity : GoalValidityPersists run goal)
    (start : Nat)
    (progress : LocalProgressAfter measure run runId goal start)
    (validStart : GoalValid (run.semanticState start) goal) :
    EventuallyGoalCompleted run goal start := by

  let initialRank := measure.rank (run.semanticState start) runId goal

  have aux :
      ∀ rank n,
        start ≤ n →
        measure.rank (run.semanticState n) runId goal = rank →
        GoalValid (run.semanticState n) goal →
        EventuallyGoalCompleted run goal n := by
    intro rank
    induction rank using Nat.strongRecOn with
    | ind rank ih =>
      intro n startLeN rankEq validN
      by_cases completeN : GoalCompleted (run.semanticState n) goal
      · exact ⟨n, Nat.le_refl n, completeN⟩
      · obtain ⟨m, nLtM, result⟩ :=
          progress n startLeN validN completeN
        cases result with
        | inl completeM =>
            exact ⟨m, Nat.le_of_lt nLtM, completeM⟩
        | inr rankDecrease =>
            have startLeM : start ≤ m :=
              Nat.le_trans startLeN (Nat.le_of_lt nLtM)
            have validM : GoalValid (run.semanticState m) goal :=
              validity n m (Nat.le_of_lt nLtM) validN
            have smaller :
                measure.rank (run.semanticState m) runId goal < rank := by
              simpa [rankEq] using rankDecrease
            have recursive :=
              ih
                (measure.rank (run.semanticState m) runId goal)
                smaller
                m
                startLeM
                rfl
                validM
            obtain ⟨finish, mLeFinish, finishComplete⟩ := recursive
            exact
              ⟨finish,
               Nat.le_trans (Nat.le_of_lt nLtM) mLeFinish,
               finishComplete⟩

  exact aux initialRank start (Nat.le_refl start) rfl validStart

/-!
### R5.21C — Teorema GLOBALE del sistema multi-agente

Questo è il theorem applicativo da citare per Sprout multi-agent.
La quantificazione è sulla run collaborativa e sul residual rank del goal
condiviso, non sulla terminazione di un singolo AgentProfile.
-/
theorem collaborative_system_completion_from_well_founded_progress_after
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (program : SemanticProgram V X)
    (validity : GoalValidityPersists run program.goal.id)
    (start : Nat)
    (progress :
      CollaborativeLocalProgressAfter measure run runId program start)
    (validStart : GoalValid (run.semanticState start) program.goal.id) :
    EventuallyCollaborativeSystemCompleted run runId program start := by

  let initialRank := measure.rank (run.semanticState start) runId program.goal.id

  have aux :
      ∀ rank n,
        start ≤ n →
        measure.rank (run.semanticState n) runId program.goal.id = rank →
        GoalValid (run.semanticState n) program.goal.id →
        EventuallyCollaborativeSystemCompleted run runId program n := by
    intro rank
    induction rank using Nat.strongRecOn with
    | ind rank ih =>
      intro n startLeN rankEq validN
      by_cases completeN :
        CollaborativeSystemCompletedAt (run.semanticState n) runId program
      · exact ⟨n, Nat.le_refl n, completeN⟩
      · obtain ⟨m, nLtM, result⟩ :=
          progress n startLeN validN completeN
        cases result with
        | inl completeM =>
            exact ⟨m, Nat.le_of_lt nLtM, completeM⟩
        | inr rankDecrease =>
            have validM : GoalValid (run.semanticState m) program.goal.id :=
              validity n m (Nat.le_of_lt nLtM) validN
            have smaller :
                measure.rank (run.semanticState m) runId program.goal.id < rank := by
              simpa [rankEq] using rankDecrease
            have recursive :=
              ih
                (measure.rank (run.semanticState m) runId program.goal.id)
                smaller
                m
                (Nat.le_trans startLeN (Nat.le_of_lt nLtM))
                rfl
                validM
            obtain ⟨finish, mLeFinish, finishComplete⟩ := recursive
            exact
              ⟨finish,
               Nat.le_trans (Nat.le_of_lt nLtM) mLeFinish,
               finishComplete⟩

  exact aux initialRank start (Nat.le_refl start) rfl validStart

/-! ### R5.22 — Bundle legacy di proprietà necessarie a LocalProgress -/

/--
NOTA R5.30:
questa struttura è mantenuta per continuità documentale, ma NON rappresenta
più l'insieme minimale delle assunzioni esterne del completion theorem.

Molti campi sono proprietà composite che la sezione R5.30 richiede di derivare
da un GoalContract strutturato e da certificati locali del kernel. Il theorem
applicativo assumption-minimal usa invece `MinimalContractSuccessExternalAssumptions`; la fedeltà semantica al prompt usa separatamente `MinimalPromptFaithfulSuccessExternalAssumptions`.
-/
structure CompletionAssumptions
    (B : ApiBoundary V)
    (P : PromptSemantics V)
    (M : TransitionSystem V)
    (R : ResponseSemantics V)
    (directory : AgentDirectory V)
    (profile : AgentProfile V)
    (compiler : SemanticCompiler V X)
    (prompt : V.SystemPrompt)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (start : Nat) where

  /--
  `profile` è il controller del programma semantico. La proprietà globale non
  termina qui: tutti gli agent participant vengono quantificati in
  `collaborativeResponsible`.
  -/
  responsible :
    SemanticResponsibleRun
      B P M R directory profile compiler run

  collaborativeResponsible :
    CollaborativeSemanticResponsibleRun
      B P M R directory profile compiler run runId
      (compiler.compile prompt) start

  /-- Il prompt/programma del segmento non cambia più da start in avanti. -/
  stablePrompt :
    PromptStableFrom profile run prompt start

  stableGoal :
    GoalRevisionStableAfter
      run
      (compiler.compile prompt).goal.id
      start

  /-- Il programma corrente deve restare persistito e autorevole nel segmento. -/
  authoritativeProgram :
    ∀ n,
      start ≤ n →
      ∃ snapshot,
        ProgramSnapshotActive (run.semanticState n) runId snapshot ∧
        snapshot.prompt = prompt ∧
        snapshot.program = compiler.compile prompt

  birthProgress :
    ObligationBirthProgressAfter
      run
      runId
      (compiler.compile prompt)
      start

  workExists :
    ∀ n,
      start ≤ n →
      WorkExistence
        (run.semanticState n)
        (compiler.compile prompt)
        runId
        (compiler.compile prompt).goal.id

  evidenceSemantics :
    EvidenceSemantics V X

  evidenceLaws :
    EvidenceSemanticsLaws evidenceSemantics

  evidenceProvenance :
    EvidenceProvenanceLaws evidenceSemantics

  dischargeSound :
    DischargeSoundness
      evidenceSemantics
      (compiler.compile prompt)
      run

  dischargeProgress :
    DischargeProgress
      evidenceSemantics
      (compiler.compile prompt)
      run

  completionSound :
    ∀ n,
      start ≤ n →
      ProgramCompletionSoundness
        (run.semanticState n)
        runId
        (compiler.compile prompt)

  waitingSemantics :
    WaitingSemantics V X

  waitingLaws :
    WaitingSemanticsLaws waitingSemantics

  blockerProgress :
    BlockerProgress run

  claimSemantics :
    ClaimSemantics V X

  schedulerSafety :
    PersistentSchedulerSafetyAfter
      claimSemantics
      run
      (compiler.compile prompt)
      start

  claimRecovery :
    ClaimRecoveryProgress claimSemantics run

  administratorEscalationProgress :
    UserGoalProposalEscalationLiveness run

  administratorDecisionProgress :
    AdministratorDecisionProgress run

  externalDependencyProgress :
    ExternalDependencyProgress run

  crossAgentDependencyProgress :
    CrossAgentDependencyProgress
      run
      (compiler.compile prompt)

  retryProgress :
    RetryWorkProgress run

  exhaustedFailureCompensation :
    ExhaustedFailureCompensation
      run
      runId
      (compiler.compile prompt).goal.id

  finiteGoalWork :
    FiniteGoalRelevantWorkAcrossRun
      run
      runId
      (compiler.compile prompt).goal.id

  /--
  Proprietà nominata anti-loop globale: copre l'intero sistema collaborativo,
  inclusi handoff fra owner differenti.
  -/
  globalMultiAgentAntiLoop :
    GlobalMultiAgentAntiLoop
      run runId (compiler.compile prompt) start

  dependencyWellFounded :
    DependencyGraphWellFounded
      (compiler.compile prompt)

  workExpansionWellFounded :
    ∀ n,
      start ≤ n →
      WorkExpansionWellFoundedAt (run.semanticState n)

  scopeAuthorizationPersists :
    ScopeAuthorizationPersistence
      run
      runId
      (compiler.compile prompt).goal.id

/- R5.22 usa ora predicati tipati e legati alla run; non restano placeholder
bare Prop per retry, dipendenze, stabilità o autorizzazione. -/

/-! ### R5.23 — Percorso legacy di refinement (superseded da R5.30) -/

/--
Queste definizioni appartengono al percorso SemanticProgram/CompletionAssumptions
precedente. Sono mantenute esclusivamente per continuità R4→R5 e NON sono più
obblighi normativi aperti: R5.30 le sostituisce con GoalContract, certificati
locali e theorem assumption-minimal.
-/
def LegacySproutLocalProgressCondition
    (measure : ProgressMeasure V X)
    (B : ApiBoundary V)
    (P : PromptSemantics V)
    (M : TransitionSystem V)
    (R : ResponseSemantics V)
    (directory : AgentDirectory V)
    (profile : AgentProfile V)
    (compiler : SemanticCompiler V X)
    (prompt : V.SystemPrompt)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (start : Nat) : Prop :=
  CompletionAssumptions
      B P M R directory profile compiler prompt run runId start →
  LocalProgressAfter
      measure
      run
      runId
      (compiler.compile prompt).goal.id
      start

/--
Obbligo di refinement GLOBALE. È più forte della vecchia LocalProgress:
il branch terminale richiede CollaborativeSystemCompletedAt, quindi non può
concludere finché resta lavoro/obligation/blocker rilevante di QUALSIASI agent.
-/
def LegacySproutCollaborativeSystemLocalProgressCondition
    (measure : ProgressMeasure V X)
    (B : ApiBoundary V)
    (P : PromptSemantics V)
    (M : TransitionSystem V)
    (R : ResponseSemantics V)
    (directory : AgentDirectory V)
    (controllerProfile : AgentProfile V)
    (compiler : SemanticCompiler V X)
    (prompt : V.SystemPrompt)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (start : Nat) : Prop :=
  CompletionAssumptions
      B P M R directory controllerProfile compiler prompt run runId start →
  CollaborativeLocalProgressAfter
      measure
      run
      runId
      (compiler.compile prompt)
      start

/--
Alias legacy del vecchio percorso collaborativo. Non è normativo per R5.30.
-/
def LegacySproutStableSegmentCollaborativeCompletionCondition :=
  @LegacySproutCollaborativeSystemLocalProgressCondition

/--
Alias legacy del vecchio LocalProgress.
-/
def LegacySproutStableSegmentLocalProgressCondition :=
  @LegacySproutLocalProgressCondition

/-! ### R5.24 — Continuità di CorrectionProfile e strategie -/

/--
CorrectionProfile R4 resta l'unica metrica normativa delle correzioni. R5 non
introduce una seconda metrica; il refinement concreto deve continuare a usare
CorrectionProfileFromRun sulla baseRun R4.
-/
def CorrectionProfilePreserved
    [DecidableEq V.SystemPrompt]
    [DecidableEq V.PrincipalId]
    (run : ObservedSemanticRun V X)
    (profile : AgentProfile V)
    (horizon : Nat) : CorrectionProfile :=
  CorrectionProfileFromRun run.baseRun profile horizon

/-
Analogamente Outcome, AgentPrefers, AgentStrategy, StrategyEvaluation e
RationalStrategy R4 restano invariati. GoalCompleted R5 può essere usato da un
nuovo evaluator come refinement del teamObjective, ma non sostituisce la
struttura di preferenza R4.
-/

/-! ### R5.25 — Distinzioni normative -/

/-
R5 richiede di non confondere:

1. SessionId con PrincipalId;
2. controller autenticato con attore agentico semantico;
3. SystemPrompt testuale con SemanticProgram;
4. metadati semantici dichiarati dal client con compilazione verificata;
5. GoalSpec con GoalStatus;
6. RunCompleted con GoalCompleted;
7. ObligationSpec con ObligationInstance;
8. obligation richiesta con obligation semplicemente creata;
9. EvidenceObserved con evidence semanticamente valida;
10. tool succeeded con obligation discharged;
11. obligation discharged con goal completed;
12. work esistente con work eligible;
13. work eligible con work eventualmente selezionato;
14. FIFO con fairness;
15. claim con ownership permanente;
16. expiry con eventuale recovery;
17. maxAttempts di una ToolCallId con finitezza del work graph globale;
18. bound di profondità del coordinamento con terminazione globale;
19. timeout operativo con liveness incondizionata;
20. safety locale con progresso globale;
21. commento informativo con autorità di modifica del goal;
22. user/agent response con administrator goal revision;
23. waitingAllowed con blocker obbligatorio;
24. blocker resolved con obligation discharged;
25. attesa di un altro agente con starvation dello scheduler;
26. proposta utente con revisione autorizzata del goal;
27. task amministrativa completata con approvazione amministrativa;
28. escalation persistente con decisione finale;
29. obligation statica con obligation condizionalmente richiesta;
30. EvidenceKind con EvidenceSubject;
31. task done con task semanticamente sufficiente;
32. programma compilato in memoria con ProgramSnapshot autorevole;
33. work esistente con work appartenente al programma corrente;
34. claim scaduta con recovery effettivo;
35. prompt iniziale con prompt corrente dopo GoalRevision;
36. finitezza per tick con finitezza globale del work rilevante.
-/

/-! ### R5.26 — Confini intenzionalmente astratti -/

/-
Continuano a restare esterni al modello normativo:

* PostgreSQL o altri database;
* SQL, trigger, RLS e lock;
* FOR UPDATE SKIP LOCKED;
* UUID, lease token e clock wall-clock;
* Rust, TypeScript e browser;
* HTTP/API concrete;
* OpenAI o altri provider;
* algoritmo concreto di scheduling/priority aging (il contratto persistente,
  l'esclusività, la recovery e la fairness restano invece formalizzati);
* algoritmi crittografici, envelope e key management.

Tali meccanismi possono soltanto raffinare le proprietà astratte R4/R5.
-/

end R5

/-!
## 11. Contratto di continuità R4 → R5

La Revisione 5 è normativa soltanto se vengono rispettate contemporaneamente:

1. tutte le proprietà R4 precedenti;
2. la proiezione ProjectsToR4 della run semantica;
3. la relazione RefinesR4PromptSemantics;
4. la preservazione esplicita di ResponsibleRun R4;
5. i nuovi invarianti R5 su goal, obligation, evidence, work e claim;
6. nessuna assunzione di completion nascosta dentro una definizione di goal;
7. LocalProgress derivato separatamente dalle fairness/environment assumptions;
8. applicazione del teorema goal_completion_from_well_founded_progress solo
   dopo la dimostrazione del relativo LocalProgress;
9. ogni revisione del goal è append-only, proviene da un commento di un
   administrator e supersede la revisione precedente senza cambio di scope
   nella stessa run;
10. commenti di user e agent possono informare, risolvere blocker, produrre
    evidence o causare nuovo lavoro, ma non autorizzano GoalRevision;
11. work con blocker irrisolto non è selezionabile; l'attesa può dipendere da
    tool, task, obligation, user, administrator, altro agent o ambiente;
12. le dependency inter-agent sono ammesse esplicitamente e la loro liveness
    resta condizionale a fairness e progress del principal dipendente;
13. una proposta di modifica goal proveniente da user non può produrre GoalRevision direttamente;
14. tale proposta deve essere escalata tramite una task R4 creata da un agent e assegnata a un administrator;
15. il completamento della review task non equivale ad approvazione: deve esistere AdministratorGoalDecision approved/rejected;
16. per revisioni originate da user, soltanto una decisione approved può autorizzare GoalRevision; una decisione rejected preserva il goal corrente; l'administrator conserva anche la facoltà di revisione diretta.

In questo senso R5 estende R4 senza indebolirla.
-/


/-! ### R5.27 — Chiusura dei gap formali identificati nell'audioguida -/

/-
Questa revisione aggiunge esplicitamente:

* obligation condizionali con activationCondition/requiredForCompletion;
* birth progress non istantaneo;
* EvidenceSubject e provenance tipata;
* completionGuard distinto dal mero discharge;
* assenza di work/blocker aperti nel CompletionCriterion;
* ProgramSnapshot append-only e programma attivo obbligatorio per work selezionato;
* refinement dinamico del prompt attraverso revisioni;
* direct administrator revision distinta da user-proposal escalation;
* work parent/source/createdAt per il work graph;
* persistent scheduler safety e claim recovery;
* fairness agentica anche per taskAction/coordination;
* dependency graph well-founded;
* retry progress e compensazione obbligatoria dei failure esauriti nel theorem di successo;
* work expansion well-founded e finitezza globale del work rilevante;
* assunzioni R5.22 espresse come predicati concreti legati a run/program,
  eliminando i precedenti placeholder bare Prop;
* LocalProgressAfter per applicare il theorem solo a revisioni stabili;
* CollaborativeSystemCompletedAt e CollaborativeLocalProgressAfter;
* quantificazione esplicita di tutti gli agent participant;
* CollaborativeWorkWeakFairnessAfter sull'intero work graph;
* causalLinks globali run/goal e GlobalCausalGraphWellFounded;
* GlobalMultiAgentAntiLoop introdotto nel percorso intermedio;
* theorem applicativo collaborative_system_completion_from_well_founded_progress_after.

R5.30 supersede il percorso di prova precedente: anti-loop, finitezza,
work existence, fairness e LocalProgress non sono più boundary assumptions,
ma vengono ricondotti a certificati locali e theorem derivati.

Restano intenzionalmente esterni soltanto i confini semantici/ambientali
esplicitati in R5.30.11 e il refinement del prodotto concreto.
-/




namespace R5

variable {V : Vocabulary}
variable {X : ExtensionVocabulary V}

/-! ## R5.30 — Assumption minimization: GoalContract strutturato e boundary residue -/

/-
OBIETTIVO NORMATIVO

Il completion theorem non deve assumere direttamente proprietà composite come
WorkExistence, BlockerProgress, CrossAgentDependencyProgress,
FiniteGoalRelevantWorkAcrossRun o GlobalMultiAgentAntiLoop quando queste
possono essere derivate da fatti più elementari e verificabili.

Questa sezione introduce quindi una gerarchia esplicita:

1. GoalContract finito prodotto dal compiler;
2. validazione strutturale deterministica;
3. certificati locali/objective del kernel;
4. lemmi/obblighi di derivazione interna;
5. poche boundary assumptions realmente esterne;
6. termination theorem e successful-completion theorem globali.

Le precedenti CompletionAssumptions R5.22 restano disponibili soltanto come
bundle legacy/intermedio per compatibilità.
-/

/-! ### R5.30.1 — DSL finita del contratto -/

/--
Condizione strutturata. Non contiene funzioni arbitrarie State → Prop.
Le condizioni realmente esterne non vengono valutate qui: sono rappresentate
da blocker/evidence esterni e dalle boundary assumptions tipate.
-/
inductive ContractCondition (V : Vocabulary) (X : ExtensionVocabulary V) where
  | always
  | never
  | taskDone (task : V.ResourceId)
  | obligationDone (obligation : V.ObligationId)
  | commentBy (principal : V.PrincipalId)
  | administratorApproved
      (administrator : V.PrincipalId)
      (reviewTask : V.ResourceId)
  | all (left right : ContractCondition V X)
  | any (left right : ContractCondition V X)
  | neg (condition : ContractCondition V X)

/-- Evaluatore fisso delle condizioni interne del contratto. -/
def ContractConditionHolds
    (s : SemanticState V X) :
    ContractCondition V X → Prop
  | ContractCondition.always => True
  | ContractCondition.never => False
  | ContractCondition.taskDone task =>
      DoneTask s.base task
  | ContractCondition.obligationDone obligation =>
      ObligationDischarged s obligation
  | ContractCondition.commentBy principal =>
      ∃ comment,
        comment ∈ s.base.comments ∧
        comment.author = principal
  | ContractCondition.administratorApproved administrator reviewTask =>
      ∃ decision,
        decision ∈ s.administratorGoalDecisions ∧
        decision.administrator = administrator ∧
        decision.reviewTask = reviewTask ∧
        decision.decision = AdministratorDecision.approved ∧
        AdministratorGoalDecisionValid s decision
  | ContractCondition.all left right =>
      ContractConditionHolds s left ∧ ContractConditionHolds s right
  | ContractCondition.any left right =>
      ContractConditionHolds s left ∨ ContractConditionHolds s right
  | ContractCondition.neg condition =>
      ¬ ContractConditionHolds s condition

/-- Classificazione finita delle azioni consentibili da una WorkSpec. -/
inductive AgentActionClass where
  | createTask
  | replaceOwnTask
  | deleteOwnTask
  | assignOwnTask
  | unassignOwnTask
  | markAssignedDone
  | appendAssignedNote
  | addAssignedAttachment
  | postComment
  | invokeTool
  | retryTool
  deriving DecidableEq, Repr

def ActionHasClass
    (action : AgentAction V)
    (actionClass : AgentActionClass) : Prop :=
  match action, actionClass with
  | AgentAction.createTask _, AgentActionClass.createTask => True
  | AgentAction.replaceOwnTask _ _, AgentActionClass.replaceOwnTask => True
  | AgentAction.deleteOwnTask _, AgentActionClass.deleteOwnTask => True
  | AgentAction.assignOwnTask _ _, AgentActionClass.assignOwnTask => True
  | AgentAction.unassignOwnTask _ _, AgentActionClass.unassignOwnTask => True
  | AgentAction.markAssignedDone _, AgentActionClass.markAssignedDone => True
  | AgentAction.appendAssignedNote _ _, AgentActionClass.appendAssignedNote => True
  | AgentAction.addAssignedAttachment _ _, AgentActionClass.addAssignedAttachment => True
  | AgentAction.postComment _, AgentActionClass.postComment => True
  | AgentAction.invokeTool _ _ _, AgentActionClass.invokeTool => True
  | AgentAction.retryTool _, AgentActionClass.retryTool => True
  | _, _ => False

/--
Modalità di verifica dell'evidence.
`mechanical` significa che la validità deriva integralmente da provenance
tipata e stato/eventi. `semanticJudgment` delimita la boundary non riducibile
a controlli strutturali.
-/
inductive EvidenceVerificationMode where
  | mechanical
  | semanticJudgment
  deriving DecidableEq, Repr

/--
Subject STATICO dell'evidence. Non contiene ToolCallId/TaskId futuri che il
compiler non può conoscere. Gli ID runtime vengono collegati tramite
WorkInstanceCertificate + causalLinks.
-/
inductive ContractEvidenceSubject
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  | workResult (workSpecId : Nat)
  | principal (principal : V.PrincipalId)
  | obligation (obligation : V.ObligationId)
  | administratorDecision
      (administrator : V.PrincipalId)
      (reviewWorkSpecId : Nat)
  | externalCondition (condition : X.ExternalConditionId)
  | derived

structure ContractEvidenceRule
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  obligation : V.ObligationId
  kind : EvidenceKind
  subject : ContractEvidenceSubject V X
  verification : EvidenceVerificationMode

/--
FailurePlan è finito e dichiarato staticamente. Un failure non può inventare
continuation arbitrari a runtime.
-/
inductive ContractFailurePlan (V : Vocabulary) (X : ExtensionVocabulary V) where
  | retrySame
  | alternatives (workSpecIds : List Nat)
  | dischargeBy (rule : ContractEvidenceRule V X)
  | failGoal

/--
Waiting target STATICO. ToolCallId e task ResourceId runtime vengono ricondotti
alla WorkSpec che li ha generati.
-/
inductive ContractWaitingTarget
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  | workTerminal (workSpecId : Nat)
  | taskFromWork (workSpecId : Nat)
  | principalResponse (principal : V.PrincipalId)
  | obligationDischarged (obligation : V.ObligationId)
  | administratorApproval
      (administrator : V.PrincipalId)
      (reviewWorkSpecId : Nat)
  | externalOutcome (condition : X.ExternalConditionId)

structure ContractWaitingRule
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  obligation : V.ObligationId
  target : ContractWaitingTarget V X

structure ContractObligationSpec
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  id : V.ObligationId
  goal : X.GoalId
  owner : V.PrincipalId
  activation : ContractCondition V X
  requiredForCompletion : ContractCondition V X
  /--
  Rank statico delle dependency. Ogni prerequisite deve avere rank strettamente
  minore dell'obligation che dipende da essa.
  -/
  dependencyRank : Nat

/--
Work template finito. `maxInstances` limita quante identità WorkItem possono
essere materializzate da questo template nel segmento stabile.
`generationRank` deve diminuire lungo ogni continuation causale.
-/
structure ContractWorkSpec
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  id : Nat
  obligation : V.ObligationId
  owner : V.PrincipalId
  kind : WorkKind
  activation : ContractCondition V X
  allowedActions : List AgentActionClass
  maxInstances : Nat
  maxAttempts : Nat
  /--
  Bound logico entro cui una selezione/claim interna deve produrre un
  avanzamento osservabile o un terminale del goal.
  -/
  maxResolutionTicks : Nat
  generationRank : Nat
  /--
  Entry point deterministico dell'obligation. Ogni obligation deve avere
  esattamente una WorkSpec entry; il kernel materializza il suo slot 0.
  -/
  isEntry : Bool
  /--
  Continuation che questo work può generare anche dopo un esito positivo o
  durante una decomposizione. Ogni continuation deve diminuire generationRank.
  -/
  continuations : List Nat
  failurePlan : ContractFailurePlan V X

/--
GoalContract: output normativo strutturato del compiler AI.
La lista finita delle obligation e WorkSpec costituisce il piano statico
massimo del segmento; il runtime materializza solo le istanze necessarie.
-/
structure GoalContract
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  goal : GoalSpec V X
  obligations : List (ContractObligationSpec V X)
  dependencies : List (Dependency V)
  workSpecs : List (ContractWorkSpec V X)
  evidenceRules : List (ContractEvidenceRule V X)
  waitingRules : List (ContractWaitingRule V X)
  completionCondition : ContractCondition V X

def ContractObligationKnown
    (contract : GoalContract V X)
    (obligation : V.ObligationId) : Prop :=
  ∃ spec,
    spec ∈ contract.obligations ∧
    spec.id = obligation

def ContractWorkSpecKnown
    (contract : GoalContract V X)
    (workSpecId : Nat) : Prop :=
  ∃ spec,
    spec ∈ contract.workSpecs ∧
    spec.id = workSpecId

def ContractEvidenceRuleKnown
    (contract : GoalContract V X)
    (rule : ContractEvidenceRule V X) : Prop :=
  rule ∈ contract.evidenceRules

def ContractRequiredAt
    (s : SemanticState V X)
    (spec : ContractObligationSpec V X) : Prop :=
  ContractConditionHolds s spec.activation ∧
  ContractConditionHolds s spec.requiredForCompletion


def ContractDependsOn
    (contract : GoalContract V X)
    (obligation prerequisite : V.ObligationId) : Prop :=
  ∃ dependency,
    dependency ∈ contract.dependencies ∧
    dependency.obligation = obligation ∧
    dependency.prerequisite = prerequisite

def ContractDependencyClosed
    (s : SemanticState V X)
    (contract : GoalContract V X)
    (obligation : V.ObligationId) : Prop :=
  ∀ prerequisite,
    ContractDependsOn contract obligation prerequisite →
    ObligationDischarged s prerequisite

def ContractMinimalActiveObligation
    (s : SemanticState V X)
    (contract : GoalContract V X)
    (obligationInstance : ObligationInstance V X) : Prop :=
  obligationInstance.status = ObligationStatus.active ∧
  ContractDependencyClosed s contract obligationInstance.spec

/-! ### R5.30.2 — Validazione strutturale deterministica -/

/--
Tutto ciò che segue è controllabile sul GoalContract senza interpretare il
significato linguistico del system prompt.
-/
structure GoalContractWellFormed
    (contract : GoalContract V X) : Prop where

  /--
  Nel percorso assumption-minimal ogni criterio finale deve essere espresso
  come obligation/evidence. Non resta una guard semantica indipendente capace
  di bloccare il goal senza un work frontier.
  -/
  completionConditionNormalized :
    contract.completionCondition = ContractCondition.always

  obligationGoalConsistency :
    ∀ spec,
      spec ∈ contract.obligations →
      spec.goal = contract.goal.id

  uniqueObligationIds :
    ∀ left right,
      left ∈ contract.obligations →
      right ∈ contract.obligations →
      left.id = right.id →
      left = right

  uniqueWorkSpecIds :
    ∀ left right,
      left ∈ contract.workSpecs →
      right ∈ contract.workSpecs →
      left.id = right.id →
      left = right

  dependenciesKnown :
    ∀ dep,
      dep ∈ contract.dependencies →
      ContractObligationKnown contract dep.obligation ∧
      ContractObligationKnown contract dep.prerequisite

  dependencyRanksDecrease :
    ∀ dep target prerequisite,
      dep ∈ contract.dependencies →
      target ∈ contract.obligations →
      prerequisite ∈ contract.obligations →
      target.id = dep.obligation →
      prerequisite.id = dep.prerequisite →
      prerequisite.dependencyRank < target.dependencyRank

  workReferencesKnownObligation :
    ∀ workSpec,
      workSpec ∈ contract.workSpecs →
      ContractObligationKnown contract workSpec.obligation

  workOwnerMatchesObligation :
    ∀ workSpec obligationSpec,
      workSpec ∈ contract.workSpecs →
      obligationSpec ∈ contract.obligations →
      workSpec.obligation = obligationSpec.id →
      workSpec.owner = obligationSpec.owner

  positiveWorkBounds :
    ∀ workSpec,
      workSpec ∈ contract.workSpecs →
      0 < workSpec.maxInstances ∧
      0 < workSpec.maxAttempts ∧
      0 < workSpec.maxResolutionTicks

  everyObligationHasWorkTemplate :
    ∀ obligationSpec,
      obligationSpec ∈ contract.obligations →
      ∃ workSpec,
        workSpec ∈ contract.workSpecs ∧
        workSpec.obligation = obligationSpec.id

  uniqueEntryWorkPerObligation :
    ∀ obligationSpec,
      obligationSpec ∈ contract.obligations →
      ∃ entry,
        entry ∈ contract.workSpecs ∧
        entry.obligation = obligationSpec.id ∧
        entry.isEntry = true ∧
        ∀ other,
          other ∈ contract.workSpecs →
          other.obligation = obligationSpec.id →
          other.isEntry = true →
          other.id = entry.id

  evidenceRulesKnown :
    ∀ rule,
      rule ∈ contract.evidenceRules →
      ContractObligationKnown contract rule.obligation

  evidenceSubjectsKnown :
    ∀ rule,
      rule ∈ contract.evidenceRules →
      match rule.subject with
      | ContractEvidenceSubject.workResult workSpecId =>
          ContractWorkSpecKnown contract workSpecId
      | ContractEvidenceSubject.principal _ => True
      | ContractEvidenceSubject.obligation obligation =>
          ContractObligationKnown contract obligation
      | ContractEvidenceSubject.administratorDecision _ reviewWorkSpecId =>
          ContractWorkSpecKnown contract reviewWorkSpecId
      | ContractEvidenceSubject.externalCondition _ => True
      | ContractEvidenceSubject.derived => True

  waitingRulesKnown :
    ∀ rule,
      rule ∈ contract.waitingRules →
      ContractObligationKnown contract rule.obligation ∧
      match rule.target with
      | ContractWaitingTarget.workTerminal workSpecId =>
          ContractWorkSpecKnown contract workSpecId
      | ContractWaitingTarget.taskFromWork workSpecId =>
          ContractWorkSpecKnown contract workSpecId
      | ContractWaitingTarget.principalResponse _ => True
      | ContractWaitingTarget.obligationDischarged obligation =>
          ContractObligationKnown contract obligation
      | ContractWaitingTarget.administratorApproval _ reviewWorkSpecId =>
          ContractWorkSpecKnown contract reviewWorkSpecId
      | ContractWaitingTarget.externalOutcome _ => True

  everyObligationHasEvidenceRule :
    ∀ obligationSpec,
      obligationSpec ∈ contract.obligations →
      ∃ rule,
        rule ∈ contract.evidenceRules ∧
        rule.obligation = obligationSpec.id

  continuationsKnownAndDecrease :
    ∀ source target,
      source ∈ contract.workSpecs →
      target ∈ source.continuations →
      ∃ targetSpec,
        targetSpec ∈ contract.workSpecs ∧
        targetSpec.id = target ∧
        targetSpec.generationRank < source.generationRank

  alternativesKnownAndDecrease :
    ∀ source alternatives target,
      source ∈ contract.workSpecs →
      source.failurePlan = ContractFailurePlan.alternatives alternatives →
      target ∈ alternatives →
      ∃ targetSpec,
        targetSpec ∈ contract.workSpecs ∧
        targetSpec.id = target ∧
        targetSpec.obligation = source.obligation ∧
        targetSpec.generationRank < source.generationRank

  dischargeRulesDeclared :
    ∀ source rule,
      source ∈ contract.workSpecs →
      source.failurePlan = ContractFailurePlan.dischargeBy rule →
      rule ∈ contract.evidenceRules ∧
      rule.obligation = source.obligation

/--
Il rank statico rende la well-foundedness delle dependency un risultato da
derivare, non una boundary assumption del theorem.
-/
def ContractDependencyRanked
    (contract : GoalContract V X) : Prop :=
  ∀ dep target prerequisite,
    dep ∈ contract.dependencies →
    target ∈ contract.obligations →
    prerequisite ∈ contract.obligations →
    target.id = dep.obligation →
    prerequisite.id = dep.prerequisite →
    prerequisite.dependencyRank < target.dependencyRank

/--
Analogo per l'espansione di work: ogni continuation alternativa dichiarata
deve diminuire generationRank.
-/
def ContractGenerationRanked
    (contract : GoalContract V X) : Prop :=
  (∀ source target,
    source ∈ contract.workSpecs →
    target ∈ source.continuations →
    ∃ targetSpec,
      targetSpec ∈ contract.workSpecs ∧
      targetSpec.id = target ∧
      targetSpec.generationRank < source.generationRank) ∧
  (∀ source alternatives target,
    source ∈ contract.workSpecs →
    source.failurePlan = ContractFailurePlan.alternatives alternatives →
    target ∈ alternatives →
    ∃ targetSpec,
      targetSpec ∈ contract.workSpecs ∧
      targetSpec.id = target ∧
      targetSpec.generationRank < source.generationRank)


theorem contract_dependency_ranked_of_well_formed
    (contract : GoalContract V X)
    (wellFormed : GoalContractWellFormed contract) :
    ContractDependencyRanked contract := by
  intro dep target prerequisite depIn targetIn prerequisiteIn targetEq prerequisiteEq
  exact
    wellFormed.dependencyRanksDecrease
      dep target prerequisite
      depIn targetIn prerequisiteIn targetEq prerequisiteEq

theorem contract_generation_ranked_of_well_formed
    (contract : GoalContract V X)
    (wellFormed : GoalContractWellFormed contract) :
    ContractGenerationRanked contract := by
  constructor
  · intro source target sourceIn targetIn
    exact
      wellFormed.continuationsKnownAndDecrease
        source target sourceIn targetIn
  · intro source alternatives target sourceIn failureEq targetIn
    obtain ⟨targetSpec, targetSpecIn, targetSpecId,
      _sameObligation, rankDecrease⟩ :=
      wellFormed.alternativesKnownAndDecrease
        source alternatives target sourceIn failureEq targetIn
    exact ⟨targetSpec, targetSpecIn, targetSpecId, rankDecrease⟩

/-! ### R5.30.3 — Compiler AI: struttura verificata, semantica isolata -/

structure ContractCompiler
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  compile : V.SystemPrompt → GoalContract V X

/--
Certificato puramente strutturale dell'output AI.
Può essere prodotto da parser/schema/validator senza fidarsi del modello AI.
-/
structure VerifiedCompiledContract
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt) : Prop where
  wellFormed :
    GoalContractWellFormed (compiler.compile prompt)

/--
Boundary linguistica residua: il contratto strutturato rappresenta davvero il
significato normativo del prompt. La specifica non finge di derivare questo
fatto dalla sola sintassi del contratto.
-/
structure PromptContractSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  adequate :
    V.SystemPrompt →
    GoalContract V X →
    Prop

def PromptContractAdequacy
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt) : Prop :=
  meaning.adequate prompt (compiler.compile prompt)

/-! ### R5.30.4 — Evidence: parte meccanica derivata, parte semantica isolata -/

def MechanicalEvidenceValid
    (run : ObservedSemanticRun V X)
    (evidence : Evidence V X) : Prop :=
  match evidence.kind, evidence.subject with
  | EvidenceKind.toolCompleted, EvidenceSubject.toolCall callId =>
      ∃ output,
        run.baseRun.event evidence.observedAt =
          some (Event.toolCompleted callId output)

  | EvidenceKind.taskCompleted, EvidenceSubject.task taskId =>
      DoneTask (run.baseRun.state evidence.observedAt) taskId

  | EvidenceKind.commentObserved, EvidenceSubject.comment commentId =>
      ∃ comment,
        run.baseRun.event evidence.observedAt =
          some (Event.commentPosted comment) ∧
        comment.id = commentId

  | EvidenceKind.principalResponse, EvidenceSubject.principal principal =>
      ∃ comment,
        run.baseRun.event evidence.observedAt =
          some (Event.commentPosted comment) ∧
        comment.author = principal

  | EvidenceKind.administratorApproval,
      EvidenceSubject.administratorDecision administrator reviewTask =>
      ∃ decision,
        decision ∈
          (run.semanticState evidence.observedAt).administratorGoalDecisions ∧
        decision.administrator = administrator ∧
        decision.reviewTask = reviewTask ∧
        decision.decision = AdministratorDecision.approved ∧
        AdministratorGoalDecisionValid
          (run.semanticState evidence.observedAt)
          decision

  | _, _ => False

/--
L'unico hook semantico rimasto per evidence non riducibile a provenance.
-/
structure SemanticEvidenceJudge
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  adequate :
    GoalContract V X →
    ObservedSemanticRun V X →
    Evidence V X →
    Prop


/--
Semantica intenzionale del dominio per le evidence non meccaniche.
È una boundary esterna esplicita, non un predicato nascosto nel kernel.
-/
structure IntendedEvidenceSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  satisfies :
    GoalContract V X →
    ObservedSemanticRun V X →
    Evidence V X →
    Prop

/-! ### R5.30.5 — Waiting: risoluzione interna deterministica -/

/--
Risoluzione osservabile dei blocker che non richiedono un giudizio semantico
sull'ambiente. La progress property resta distinta dalla correctness
dell'osservazione.
-/
def MechanicalWaitingResolvedAt
    (run : ObservedSemanticRun V X)
    (blocker : Blocker V X)
    (tick : Nat) : Prop :=
  match blocker.condition with
  | WaitingCondition.toolTerminal callId =>
      ToolTerminalEventAt run.baseRun callId tick

  | WaitingCondition.principalResponse principal =>
      ∃ comment,
        run.baseRun.event tick = some (Event.commentPosted comment) ∧
        comment.author = principal

  | WaitingCondition.taskCompleted task =>
      DoneTask (run.baseRun.state tick) task

  | WaitingCondition.obligationDischarged obligation =>
      ObligationDischarged (run.semanticState tick) obligation

  | WaitingCondition.administratorApproval administrator =>
      ∃ decision,
        decision ∈ (run.semanticState tick).administratorGoalDecisions ∧
        decision.administrator = administrator ∧
        decision.decision = AdministratorDecision.approved ∧
        AdministratorGoalDecisionValid (run.semanticState tick) decision

  | WaitingCondition.externalOutcome _ => False
  | WaitingCondition.derivedCondition _ => False

/-! ### R5.30.6 — Certificazione finita del work graph -/

structure WorkInstanceCertificate
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  work : X.WorkItemId
  workSpecId : Nat
  slot : Nat

/--
Overlay certificato. `workCertificateAt` non modifica la run R4/R5: attesta
come ogni WorkItem rilevante deriva da uno slot finito di una WorkSpec.
-/
structure BlockerRuleCertificate
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  blocker : X.BlockerId
  obligation : V.ObligationId
  waitingRuleIndex : Nat

structure ClaimLeaseCertificate
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  claim : X.ClaimId
  work : X.WorkItemId
  attempt : Nat
  claimant : V.PrincipalId
  acquiredAt : Nat
  expiresAt : Nat

structure CertifiedCollaborativeRun
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  run : ObservedSemanticRun V X

  /--
  ID canonico preallocato per ogni coppia (WorkSpecId, slot). La finitezza del
  work graph diventa costruttiva: gli ID runtime possibili appartengono a una
  lista finita calcolabile dal GoalContract.
  -/
  workIdForSlot : Nat → Nat → X.WorkItemId

  workCertificateAt :
    Nat → X.WorkItemId → Option (WorkInstanceCertificate V X)

  blockerCertificateAt :
    Nat → X.BlockerId → Option (BlockerRuleCertificate V X)

  claimLeaseAt :
    Nat → X.ClaimId → Option (ClaimLeaseCertificate V X)

  /-- Posizione logica nella queue per il work al tick indicato. -/
  schedulerPositionAt : Nat → X.WorkItemId → Nat

  /--
  Rank globale dei nodi causali. È un certificato meccanicamente verificabile:
  ogni causal link deve diminuire questo rank.
  -/
  causalRank : CollaborativeCausalNode V X → Nat

def LogicalClaimValidAt
    (certified : CertifiedCollaborativeRun V X)
    (tick : Nat)
    (claimId : X.ClaimId) : Prop :=
  ∃ lease work dispatch,
    certified.claimLeaseAt tick claimId = some lease ∧
    lease.claim = claimId ∧
    lease.acquiredAt ≤ tick ∧
    tick < lease.expiresAt ∧
    (certified.run.semanticState tick).workItems lease.work = some work ∧
    (certified.run.semanticState tick).dispatches lease.work = some dispatch ∧
    work.status = WorkStatus.claimed ∧
    dispatch.status = DispatchStatus.claimed ∧
    work.attempt = lease.attempt ∧
    dispatch.attempt = lease.attempt

def LogicalClaimExpiredAt
    (certified : CertifiedCollaborativeRun V X)
    (tick : Nat)
    (claimId : X.ClaimId) : Prop :=
  ∃ lease,
    certified.claimLeaseAt tick claimId = some lease ∧
    lease.expiresAt ≤ tick

def ContractWorkEligible
    (s : SemanticState V X)
    (contract : GoalContract V X)
    (work : WorkItem V X) : Prop :=
  work.status = WorkStatus.eligible ∧
  ContractDependencyClosed s contract work.serves ∧
  ∃ workSpec,
    workSpec ∈ contract.workSpecs ∧
    workSpec.obligation = work.serves ∧
    workSpec.owner = work.owner ∧
    workSpec.kind = work.kind ∧
    ContractConditionHolds s workSpec.activation ∧
    work.attempt < workSpec.maxAttempts

def ContractWorkEligibleAt
    (run : ObservedSemanticRun V X)
    (contract : GoalContract V X)
    (work : WorkItem V X)
    (tick : Nat) : Prop :=
  (run.semanticState tick).workItems work.id = some work ∧
  ContractWorkEligible (run.semanticState tick) contract work

def ContractContinuouslyEligibleAfter
    (run : ObservedSemanticRun V X)
    (contract : GoalContract V X)
    (work : WorkItem V X)
    (start : Nat) : Prop :=
  ∀ n,
    start ≤ n →
    ContractWorkEligibleAt run contract work n

/--
Una subject statica viene collegata all'ID runtime tramite la certificazione
della WorkSpec e il causal graph.
-/
def ContractEvidenceSubjectMatches
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (evidence : Evidence V X)
    (subject : ContractEvidenceSubject V X) : Prop :=
  match subject, evidence.subject with
  | ContractEvidenceSubject.workResult workSpecId,
      EvidenceSubject.toolCall callId =>
      ∃ workId certificate,
        certified.workCertificateAt evidence.observedAt workId =
          some certificate ∧
        certificate.workSpecId = workSpecId ∧
        GlobalCausalSuccessorOf
          certified.run
          runId
          contract.goal.id
          (CollaborativeCausalNode.toolCall callId)
          (CollaborativeCausalNode.work workId)

  | ContractEvidenceSubject.workResult workSpecId,
      EvidenceSubject.task taskId =>
      ∃ workId certificate,
        certified.workCertificateAt evidence.observedAt workId =
          some certificate ∧
        certificate.workSpecId = workSpecId ∧
        GlobalCausalSuccessorOf
          certified.run
          runId
          contract.goal.id
          (CollaborativeCausalNode.task taskId)
          (CollaborativeCausalNode.work workId)

  | ContractEvidenceSubject.principal principal,
      EvidenceSubject.principal observed =>
      observed = principal

  | ContractEvidenceSubject.obligation obligation,
      EvidenceSubject.obligation observed =>
      observed = obligation

  | ContractEvidenceSubject.administratorDecision administrator reviewWorkSpecId,
      EvidenceSubject.administratorDecision observedAdministrator reviewTask =>
      observedAdministrator = administrator ∧
      ∃ workId certificate,
        certified.workCertificateAt evidence.observedAt workId =
          some certificate ∧
        certificate.workSpecId = reviewWorkSpecId ∧
        GlobalCausalSuccessorOf
          certified.run
          runId
          contract.goal.id
          (CollaborativeCausalNode.task reviewTask)
          (CollaborativeCausalNode.work workId)

  | ContractEvidenceSubject.externalCondition condition,
      EvidenceSubject.externalCondition observed =>
      observed = condition

  | ContractEvidenceSubject.derived, EvidenceSubject.derived =>
      True

  | _, _ => False

def ContractEvidenceValid
    (judge : SemanticEvidenceJudge V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (evidence : Evidence V X) : Prop :=
  ∃ rule,
    rule ∈ contract.evidenceRules ∧
    rule.obligation = evidence.obligation ∧
    rule.kind = evidence.kind ∧
    ContractEvidenceSubjectMatches
      certified runId contract evidence rule.subject ∧
    match rule.verification with
    | EvidenceVerificationMode.mechanical =>
        MechanicalEvidenceValid certified.run evidence
    | EvidenceVerificationMode.semanticJudgment =>
        judge.adequate contract certified.run evidence

/--
Matching statico→runtime anche per i blocker.
-/
def ContractWaitingRuleMatches
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (tick : Nat)
    (blocker : Blocker V X)
    (rule : ContractWaitingRule V X) : Prop :=
  rule.obligation =
    (match blocker.scope with
    | BlockScope.obligation obligation => obligation
    | BlockScope.work workId =>
        match (certified.run.semanticState tick).workItems workId with
        | some work => work.serves
        | none => rule.obligation
    | BlockScope.goal _ => rule.obligation) ∧
  match rule.target, blocker.condition with
  | ContractWaitingTarget.workTerminal workSpecId,
      WaitingCondition.toolTerminal callId =>
      ∃ workId certificate,
        certified.workCertificateAt tick workId = some certificate ∧
        certificate.workSpecId = workSpecId ∧
        GlobalCausalSuccessorOf
          certified.run runId contract.goal.id
          (CollaborativeCausalNode.toolCall callId)
          (CollaborativeCausalNode.work workId)

  | ContractWaitingTarget.taskFromWork workSpecId,
      WaitingCondition.taskCompleted taskId =>
      ∃ workId certificate,
        certified.workCertificateAt tick workId = some certificate ∧
        certificate.workSpecId = workSpecId ∧
        GlobalCausalSuccessorOf
          certified.run runId contract.goal.id
          (CollaborativeCausalNode.task taskId)
          (CollaborativeCausalNode.work workId)

  | ContractWaitingTarget.principalResponse principal,
      WaitingCondition.principalResponse observed =>
      observed = principal

  | ContractWaitingTarget.obligationDischarged obligation,
      WaitingCondition.obligationDischarged observed =>
      observed = obligation

  | ContractWaitingTarget.administratorApproval administrator reviewWorkSpecId,
      WaitingCondition.administratorApproval observedAdministrator =>
      observedAdministrator = administrator ∧
      ∃ workId certificate,
        certified.workCertificateAt tick workId = some certificate ∧
        certificate.workSpecId = reviewWorkSpecId

  | ContractWaitingTarget.externalOutcome condition,
      WaitingCondition.externalOutcome observed =>
      observed = condition

  | _, _ => False

/--
Finitezza del work rilevante limitata al segmento stabile del theorem.
-/
def FiniteGoalRelevantWorkAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  ∃ ids : List X.WorkItemId,
    ∀ n workId work,
      start ≤ n →
      (run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = goal →
      workId ∈ ids

def GlobalCausalSuccessorOfAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (start : Nat)
    (successor predecessor : CollaborativeCausalNode V X) : Prop :=
  ∃ n link,
    start ≤ n ∧
    link ∈ (run.semanticState n).causalLinks ∧
    link.run = runId ∧
    link.goal = goal ∧
    link.predecessor = predecessor ∧
    link.successor = successor

def GlobalCausalGraphWellFoundedAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  WellFounded
    (GlobalCausalSuccessorOfAfter run runId goal start)

def ContractCausalLinkCompletenessAfter
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  (∀ dependency,
    dependency ∈ contract.dependencies →
    GlobalCausalSuccessorOfAfter
      certified.run
      runId
      contract.goal.id
      start
      (CollaborativeCausalNode.obligation dependency.obligation)
      (CollaborativeCausalNode.obligation dependency.prerequisite)) ∧
  (∀ n workId work,
    start ≤ n →
    (certified.run.semanticState n).workItems workId = some work →
    work.run = runId →
    work.goal = contract.goal.id →
    GlobalCausalSuccessorOfAfter
      certified.run
      runId
      contract.goal.id
      start
      (CollaborativeCausalNode.work workId)
      (CollaborativeCausalNode.obligation work.serves)) ∧
  (∀ n childId child parentId,
    start ≤ n →
    (certified.run.semanticState n).workItems childId = some child →
    child.run = runId →
    child.goal = contract.goal.id →
    child.parent = some parentId →
    GlobalCausalSuccessorOfAfter
      certified.run
      runId
      contract.goal.id
      start
      (CollaborativeCausalNode.work childId)
      (CollaborativeCausalNode.work parentId)) ∧
  (∀ n workId work commentId,
    start ≤ n →
    (certified.run.semanticState n).workItems workId = some work →
    work.run = runId →
    work.goal = contract.goal.id →
    work.sourceComment = some commentId →
    GlobalCausalSuccessorOfAfter
      certified.run
      runId
      contract.goal.id
      start
      (CollaborativeCausalNode.work workId)
      (CollaborativeCausalNode.comment commentId))

/-- Blocker controllato da un principal umano. -/
def HumanControlledBlockerAt
    (s : SemanticState V X)
    (blocker : Blocker V X) : Prop :=
  (∃ principal,
      blocker.condition = WaitingCondition.principalResponse principal ∧
      (HasKind s.base principal PrincipalKind.user ∨
       HasKind s.base principal PrincipalKind.administrator)) ∨
  (∃ administrator,
      blocker.condition =
        WaitingCondition.administratorApproval administrator) ∨
  (∃ task principal,
      blocker.condition = WaitingCondition.taskCompleted task ∧
      AssignedTo s.base principal task ∧
      (HasKind s.base principal PrincipalKind.user ∨
       HasKind s.base principal PrincipalKind.administrator))

def ExternalControlledBlocker
    (blocker : Blocker V X) : Prop :=
  ∃ condition,
    blocker.condition = WaitingCondition.externalOutcome condition


/--
Fatti locali/objective del kernel. Non includono le vecchie proprietà globali
composite come CrossAgentDependencyProgress o GlobalMultiAgentAntiLoop.
-/
structure CollaborativeKernelCertificate
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where

  contractWellFormed :
    GoalContractWellFormed contract

  /--
  Birth completeness resa INVARIANTE locale: nel modello assumption-minimal,
  l'attivazione di una required obligation e la materializzazione della sua
  istanza appartengono alla stessa chiusura del kernel.
  -/
  requiredObligationClosure :
    ∀ n spec,
      start ≤ n →
      spec ∈ contract.obligations →
      ContractRequiredAt (certified.run.semanticState n) spec →
      ∃ obligationInstance,
        (certified.run.semanticState n).obligations spec.id = some obligationInstance ∧
        obligationInstance.run = runId ∧
        obligationInstance.spec = spec.id ∧
        obligationInstance.owner = spec.owner

  /--
  Ogni required obligation active ha una prerequisite active minimalmente
  abilitata (eventualmente se stessa). È una proprietà finita verificabile
  tramite dependencyRank, non una liveness assumption.
  -/
  requiredActiveHasMinimalFrontier :
    ∀ n spec obligationInstance,
      start ≤ n →
      spec ∈ contract.obligations →
      ContractRequiredAt (certified.run.semanticState n) spec →
      (certified.run.semanticState n).obligations spec.id = some obligationInstance →
      obligationInstance.run = runId →
      obligationInstance.status = ObligationStatus.active →
      ∃ minimal,
        minimal.run = runId ∧
        ContractMinimalActiveObligation
          (certified.run.semanticState n)
          contract
          minimal

  /--
  Ogni obligation minimalmente active possiede già il work entry slot 0.
  `ContractWorkExistence` diventa quindi derivabile e non una liveness
  assumption autonoma.
  -/
  entryWorkClosure :
    ∀ n obligation,
      start ≤ n →
      obligation.run = runId →
      ContractMinimalActiveObligation
        (certified.run.semanticState n)
        contract
        obligation →
      ∃ entry workId work certificate,
        entry ∈ contract.workSpecs ∧
        entry.obligation = obligation.spec ∧
        entry.isEntry = true ∧
        (certified.run.semanticState n).workItems workId = some work ∧
        certified.workCertificateAt n workId = some certificate ∧
        certificate.workSpecId = entry.id ∧
        certificate.slot = 0 ∧
        work.run = runId ∧
        work.goal = contract.goal.id ∧
        work.serves = obligation.spec ∧
        (work.status = WorkStatus.eligible ∨
         work.status = WorkStatus.blocked ∨
         work.status = WorkStatus.claimed)

  allRelevantWorkCertified :
    ∀ n workId work,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      ∃ certificate workSpec,
        certified.workCertificateAt n workId = some certificate ∧
        certificate.work = workId ∧
        workSpec ∈ contract.workSpecs ∧
        workSpec.id = certificate.workSpecId ∧
        workSpec.obligation = work.serves ∧
        workSpec.owner = work.owner ∧
        workSpec.kind = work.kind ∧
        certificate.slot < workSpec.maxInstances ∧
        work.attempt < workSpec.maxAttempts

  /-- La chiave persistita di un WorkItem coincide sempre con il suo id. -/
  workIdentity :
    ∀ n workId work,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      work.id = workId

  /--
  Ogni WorkItem runtime certificato usa l'ID canonico del proprio slot.
  -/
  canonicalWorkId :
    ∀ n workId certificate,
      start ≤ n →
      certified.workCertificateAt n workId = some certificate →
      workId =
        certified.workIdForSlot
          certificate.workSpecId
          certificate.slot

  slotIdentityStable :
    ∀ n m workId left right,
      start ≤ n →
      n ≤ m →
      certified.workCertificateAt n workId = some left →
      certified.workCertificateAt m workId = some right →
      left.workSpecId = right.workSpecId ∧
      left.slot = right.slot

  slotUnique :
    ∀ n leftId rightId left right,
      start ≤ n →
      certified.workCertificateAt n leftId = some left →
      certified.workCertificateAt n rightId = some right →
      left.workSpecId = right.workSpecId →
      left.slot = right.slot →
      leftId = rightId

  /--
  Lo stesso slot di una WorkSpec non può essere riutilizzato con una nuova
  WorkItemId in un tick futuro.
  -/
  slotUniqueAcrossRun :
    ∀ n m leftId rightId left right,
      start ≤ n →
      start ≤ m →
      certified.workCertificateAt n leftId = some left →
      certified.workCertificateAt m rightId = some right →
      left.workSpecId = right.workSpecId →
      left.slot = right.slot →
      leftId = rightId

  workActivationSound :
    ∀ n workId work certificate workSpec,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      certified.workCertificateAt n workId = some certificate →
      workSpec ∈ contract.workSpecs →
      workSpec.id = certificate.workSpecId →
      ContractConditionHolds
        (certified.run.semanticState n)
        workSpec.activation

  /--
  Lo status `eligible` non è soltanto un'etichetta: deve soddisfare la
  eligibility contract-native completa.
  -/
  eligibleWorkStatusSound :
    ∀ n workId work,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      work.status = WorkStatus.eligible →
      ContractWorkEligible
        (certified.run.semanticState n)
        contract
        work

  /--
  Nessun WorkStatus.blocked è una ragione opaca di inattività.
  -/
  blockedWorkHasWaitingBlocker :
    ∀ n workId work,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      work.status = WorkStatus.blocked →
      ∃ blockerId blocker,
        (certified.run.semanticState n).blockers blockerId = some blocker ∧
        blocker.run = runId ∧
        blocker.goal = contract.goal.id ∧
        BlockerAppliesToWork blocker work ∧
        BlockerWaiting blocker

  /--
  Nel percorso assumption-minimal una frontiera `blocked` può dipendere solo
  da un umano o dal mondo esterno. Attese interne (altri agent, obligation,
  tool runtime, task agentiche) devono essere rappresentate come dependency o
  WorkItem/claim, non come blocker opaco.
  -/
  waitingBlockersExternallyControlled :
    ∀ n blockerId blocker,
      start ≤ n →
      (certified.run.semanticState n).blockers blockerId = some blocker →
      blocker.run = runId →
      blocker.goal = contract.goal.id →
      BlockerWaiting blocker →
      HumanControlledBlockerAt
          (certified.run.semanticState n) blocker ∨
        ExternalControlledBlocker blocker

  allRelevantBlockersCertified :
    ∀ n blockerId blocker,
      start ≤ n →
      (certified.run.semanticState n).blockers blockerId = some blocker →
      blocker.run = runId →
      blocker.goal = contract.goal.id →
      ∃ certificate rule,
        certified.blockerCertificateAt n blockerId = some certificate ∧
        certificate.blocker = blockerId ∧
        rule ∈ contract.waitingRules ∧
        ContractWaitingRuleMatches
          certified
          runId
          contract
          n
          blocker
          rule

  blockerRuleStable :
    ∀ n m blockerId left right,
      start ≤ n →
      n ≤ m →
      certified.blockerCertificateAt n blockerId = some left →
      certified.blockerCertificateAt m blockerId = some right →
      left.obligation = right.obligation ∧
      left.waitingRuleIndex = right.waitingRuleIndex

  ownerIsParticipant :
    ∀ n workId work,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      (certified.run.semanticState n).runParticipants runId work.owner

  /--
  Ogni child work deve essere una continuation dichiarata dal parent oppure
  un'alternativa del suo FailurePlan, e deve avere generationRank minore.
  -/
  parentGenerationSound :
    ∀ n childId child parentId parent childCert parentCert childSpec parentSpec,
      start ≤ n →
      (certified.run.semanticState n).workItems childId = some child →
      child.parent = some parentId →
      (certified.run.semanticState n).workItems parentId = some parent →
      certified.workCertificateAt n childId = some childCert →
      certified.workCertificateAt n parentId = some parentCert →
      childSpec ∈ contract.workSpecs →
      parentSpec ∈ contract.workSpecs →
      childSpec.id = childCert.workSpecId →
      parentSpec.id = parentCert.workSpecId →
      (childSpec.id ∈ parentSpec.continuations ∨
       ∃ alternatives,
         parentSpec.failurePlan =
           ContractFailurePlan.alternatives alternatives ∧
         childSpec.id ∈ alternatives) ∧
      childSpec.generationRank < parentSpec.generationRank

  validClaimsExclusive :
    ∀ n leftId rightId leftLease rightLease,
      start ≤ n →
      LogicalClaimValidAt certified n leftId →
      LogicalClaimValidAt certified n rightId →
      certified.claimLeaseAt n leftId = some leftLease →
      certified.claimLeaseAt n rightId = some rightLease →
      leftLease.work = rightLease.work →
      leftLease.attempt = rightLease.attempt →
      leftId = rightId

  expiredClaimsInvalid :
    ∀ n claimId,
      start ≤ n →
      LogicalClaimExpiredAt certified n claimId →
      ¬ LogicalClaimValidAt certified n claimId

  /--
  Ogni arco causale del goal diminuisce strettamente il rank globale.
  Questo è verificabile localmente su ogni record causalLinks.
  -/
  causalRankDecreases :
    ∀ n link,
      start ≤ n →
      link ∈ (certified.run.semanticState n).causalLinks →
      link.run = runId →
      link.goal = contract.goal.id →
      certified.causalRank link.successor <
        certified.causalRank link.predecessor

  causalLinksComplete :
    ContractCausalLinkCompletenessAfter
      certified
      runId
      contract
      start


/--
Universo finito e canonico degli ID WorkItem possibili nel contratto.
Ogni WorkSpec possiede `maxInstances` slot e ogni slot ha un ID deterministico.
-/
def ContractWorkUniverse
    (certified : CertifiedCollaborativeRun V X)
    (contract : GoalContract V X) : List X.WorkItemId :=
  contract.workSpecs.flatMap (fun workSpec =>
    (List.range workSpec.maxInstances).map (fun slot =>
      certified.workIdForSlot workSpec.id slot))

/--
FINITENZA PROVATA: dal certificato degli slot segue costruttivamente che ogni
WorkItem rilevante nel segmento stabile appartiene a `ContractWorkUniverse`.
-/
theorem finite_goal_relevant_work_after_of_contract
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (kernel : CollaborativeKernelCertificate
      certified runId contract start) :
    FiniteGoalRelevantWorkAfter
      certified.run runId contract.goal.id start := by
  refine ⟨ContractWorkUniverse certified contract, ?_⟩
  intro n workId work startLeN workAt runEq goalEq
  obtain
    ⟨certificate, workSpec,
      certificateAt, certificateWork, workSpecIn,
      workSpecId, workSpecObligation, workSpecOwner,
      workSpecKind, slotBound, attemptBound⟩ :=
    kernel.allRelevantWorkCertified
      n workId work startLeN workAt runEq goalEq
  have canonical :=
    kernel.canonicalWorkId
      n workId certificate startLeN certificateAt
  rw [canonical]
  apply List.mem_flatMap.mpr
  refine ⟨workSpec, workSpecIn, ?_⟩
  apply List.mem_map.mpr
  exact
    ⟨certificate.slot, List.mem_range.mpr slotBound, by simp [workSpecId]⟩

/--
Compatibilità con il nome dell'obbligo precedente: non è più un'assunzione,
ma un theorem.
-/
theorem FiniteWorkFromContractObligation
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) :
    CollaborativeKernelCertificate certified runId contract start →
    FiniteGoalRelevantWorkAfter
      certified.run runId contract.goal.id start := by
  intro kernel
  exact
    finite_goal_relevant_work_after_of_contract
      certified runId contract start kernel

/-
WELL-FOUNDEDNESS CAUSALE PROVATA dal rank globale certificato.
Ogni causal link è una sottorelazione della misura naturale `causalRank`.
-/
/--
Lemma generico: una relazione che diminuisce strettamente un rank Nat è
well-founded. Dipende soltanto da Nat.strongRecOn/Acc, disponibili in
Lean core/Std.
-/
theorem wellFounded_of_nat_rank_decrease
    {α : Sort u}
    (relation : α → α → Prop)
    (rank : α → Nat)
    (decreases :
      ∀ child parent,
        relation child parent →
        rank child < rank parent) :
    WellFounded relation := by

  have aux :
      ∀ n node,
        rank node = n →
        Acc relation node := by
    intro n
    induction n using Nat.strongRecOn with
    | ind n ih =>
      intro node rankEq
      apply Acc.intro
      intro child childRel
      have childLt :
          rank child < n := by
        rw [← rankEq]
        exact decreases child node childRel
      exact
        ih
          (rank child)
          childLt
          child
          rfl

  apply WellFounded.intro
  intro node
  exact aux (rank node) node rfl

/--
WELL-FOUNDEDNESS CAUSALE PROVATA dal rank globale certificato.
Ogni causal link è una sottorelazione della misura naturale `causalRank`.
-/
theorem global_causal_graph_well_founded_after_of_rank
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (kernel : CollaborativeKernelCertificate
      certified runId contract start) :
    GlobalCausalGraphWellFoundedAfter
      certified.run runId contract.goal.id start := by

  apply
    wellFounded_of_nat_rank_decrease
      (GlobalCausalSuccessorOfAfter
        certified.run runId contract.goal.id start)
      certified.causalRank

  intro successor predecessor causal
  obtain
    ⟨n, link, startLeN, linkIn, linkRun, linkGoal,
      predecessorEq, successorEq⟩ := causal

  rw [← successorEq, ← predecessorEq]

  exact
    kernel.causalRankDecreases
      n link startLeN linkIn linkRun linkGoal

/--
Anti-loop causale: l'obbligo precedente è ora un theorem derivato dal
certificato locale del rank.
-/
theorem GlobalAntiLoopFromContractObligation
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) :
    CollaborativeKernelCertificate certified runId contract start →
    ContractDependencyRanked contract →
    ContractGenerationRanked contract →
    GlobalCausalGraphWellFoundedAfter
      certified.run runId contract.goal.id start := by
  intro kernel dependencyRanked generationRanked
  exact
    global_causal_graph_well_founded_after_of_rank
      certified runId contract start kernel

/-! ### R5.30.7 — Scheduler concreto come fonte della fairness -/

structure AgingSchedulerPolicy where
  agingStep : Nat
  positiveAging : 0 < agingStep

def EffectiveAgePriority
    (policy : AgingSchedulerPolicy)
    (work : WorkItem V X)
    (tick : Nat) : Nat :=
  policy.agingStep * (tick - work.createdAt)

/-
Certificato locale dello scheduler. Il theorem di fairness deve essere
DERIVATO da queste regole più la finitezza degli slot, non assunto a parte.
-/
/-! ### R5.30.7 — Scheduler contract-native e fairness derivabile -/

structure ContractAgingSchedulerCertificate
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where

  selectedOnlyEligible :
    ∀ n workId work,
      start ≤ n →
      SelectedAt certified.run workId n →
      (certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      ContractWorkEligible
        (certified.run.semanticState n)
        contract
        work

  /--
  Se un work eligible ha posizione 0, viene selezionato nello stesso tick.
  -/
  positionZeroSelected :
    ∀ n work,
      start ≤ n →
      (certified.run.semanticState n).workItems work.id = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      ContractWorkEligible
        (certified.run.semanticState n)
        contract
        work →
      certified.schedulerPositionAt n work.id = 0 →
      SelectedAt certified.run work.id n

  /--
  Un work eligible non può sparire dalla queue senza essere selezionato.
  Se non viene selezionato al tick n, resta lo stesso work eligible a n+1.
  -/
  unselectedEligiblePersists :
    ∀ n work,
      start ≤ n →
      ContractWorkEligibleAt
        certified.run contract work n →
      work.run = runId →
      work.goal = contract.goal.id →
      ¬ SelectedAt certified.run work.id n →
      ContractWorkEligibleAt
        certified.run contract work (n + 1)

  /--
  Se il work non viene selezionato ma rimane eligible, la sua posizione nella
  queue diminuisce strettamente al tick successivo.
  -/
  unselectedPositionDecreases :
    ∀ n work,
      start ≤ n →
      (certified.run.semanticState n).workItems work.id = some work →
      (certified.run.semanticState (n + 1)).workItems work.id = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      ContractWorkEligible
        (certified.run.semanticState n)
        contract
        work →
      ContractWorkEligible
        (certified.run.semanticState (n + 1))
        contract
        work →
      ¬ SelectedAt certified.run work.id n →
      certified.schedulerPositionAt (n + 1) work.id <
        certified.schedulerPositionAt n work.id

def ContractCollaborativeWorkWeakFairnessAfter
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  ∀ work n,
    start ≤ n →
    work.run = runId →
    work.goal = contract.goal.id →
    ContractContinuouslyEligibleAfter run contract work n →
    EventuallyAfter n (fun m => SelectedAt run work.id m)

/-
Fairness non è una boundary esterna: deve essere derivata da aging + finitezza
degli slot del GoalContract.
-/
/--
Da una singola eligibility, la posizione naturale non può diminuire
all'infinito: il work viene selezionato e al tick di selezione è ancora
lo stesso work eligible.
-/
theorem contract_scheduler_eventually_selects_eligible
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (scheduler :
      ContractAgingSchedulerCertificate
        policy certified runId contract start)
    (work : WorkItem V X)
    (n : Nat)
    (startLeN : start ≤ n)
    (workRun : work.run = runId)
    (workGoal : work.goal = contract.goal.id)
    (eligibleAtN :
      ContractWorkEligibleAt
        certified.run contract work n) :
    ∃ m,
      n ≤ m ∧
      SelectedAt certified.run work.id m ∧
      ContractWorkEligibleAt
        certified.run contract work m := by

  let initialPosition :=
    certified.schedulerPositionAt n work.id

  have aux :
      ∀ position tick,
        start ≤ tick →
        work.run = runId →
        work.goal = contract.goal.id →
        ContractWorkEligibleAt
          certified.run contract work tick →
        certified.schedulerPositionAt tick work.id = position →
        ∃ m,
          tick ≤ m ∧
          SelectedAt certified.run work.id m ∧
          ContractWorkEligibleAt
            certified.run contract work m := by
    intro position
    induction position using Nat.strongRecOn with
    | ind position ih =>
      intro tick startLeTick runEq goalEq eligible positionEq
      by_cases selected : SelectedAt certified.run work.id tick
      · exact
          ⟨tick, Nat.le_refl tick, selected, eligible⟩
      ·
        have nextEligible :=
          scheduler.unselectedEligiblePersists
            tick work startLeTick eligible runEq goalEq selected

        have decreases :=
          scheduler.unselectedPositionDecreases
            tick
            work
            startLeTick
            eligible.1
            nextEligible.1
            runEq
            goalEq
            eligible.2
            nextEligible.2
            selected

        have smaller :
            certified.schedulerPositionAt (tick + 1) work.id < position := by
          rw [← positionEq]
          exact decreases

        have recursive :=
          ih
            (certified.schedulerPositionAt (tick + 1) work.id)
            smaller
            (tick + 1)
            (Nat.le_trans startLeTick (Nat.le_succ tick))
            runEq
            goalEq
            nextEligible
            rfl

        obtain ⟨m, nextLeM, selectedM, eligibleM⟩ := recursive
        exact
          ⟨m,
           Nat.le_trans (Nat.le_succ tick) nextLeM,
           selectedM,
           eligibleM⟩

  exact
    aux
      initialPosition
      n
      startLeN
      workRun
      workGoal
      eligibleAtN
      rfl

theorem contract_scheduler_fairness_of_position_descent
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (scheduler :
      ContractAgingSchedulerCertificate
        policy certified runId contract start) :
    ContractCollaborativeWorkWeakFairnessAfter
      certified.run runId contract start := by
  intro work n startLeN workRun workGoal continuouslyEligible
  have eligibleAtN :=
    continuouslyEligible n (Nat.le_refl n)
  obtain ⟨m, nLeM, selectedM, eligibleM⟩ :=
    contract_scheduler_eventually_selects_eligible
      policy certified runId contract start scheduler
      work n startLeN workRun workGoal eligibleAtN
  exact ⟨m, nLeM, selectedM⟩

/--
Nome di continuità dell'obbligo precedente, ora chiuso da theorem.
-/
theorem ContractSchedulerFairnessDerivationObligation
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) :
    ContractAgingSchedulerCertificate
      policy certified runId contract start →
    ContractCollaborativeWorkWeakFairnessAfter
      certified.run runId contract start := by
  intro scheduler
  exact
    contract_scheduler_fairness_of_position_descent
      policy certified runId contract start scheduler

/--
Bundle di fatti verificabili del kernel usato dal theorem assumption-minimal.
Non contiene PromptContractAdequacy, HumanProgress o feasibility esterna.
-/
structure AssumptionMinimalKernelCertificate
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where
  core :
    CollaborativeKernelCertificate certified runId contract start

  scheduler :
    ContractAgingSchedulerCertificate
      policy certified runId contract start

/-! ### R5.30.7A — Semantica contract-native: nessun SemanticProgram arbitrario -/

/--
Frontiera di work contract-native. Non dipende da `SemanticProgram.actionSupports`
o da altri Prop arbitrari legacy.
-/
def ContractWorkExistence
    (s : SemanticState V X)
    (contract : GoalContract V X)
    (runId : X.RunId) : Prop :=
  ∀ obligation,
    obligation.run = runId →
    ContractMinimalActiveObligation s contract obligation →
    ∃ workId work,
      s.workItems workId = some work ∧
      work.run = runId ∧
      work.goal = contract.goal.id ∧
      work.serves = obligation.spec ∧
      (work.status = WorkStatus.eligible ∨
       work.status = WorkStatus.blocked ∨
       work.status = WorkStatus.claimed)

def AllContractRequiredObligationsInstantiated
    (s : SemanticState V X)
    (runId : X.RunId)
    (contract : GoalContract V X) : Prop :=
  ∀ spec,
    spec ∈ contract.obligations →
    ContractRequiredAt s spec →
    ∃ obligationInstance,
      s.obligations spec.id = some obligationInstance ∧
      obligationInstance.run = runId ∧
      obligationInstance.spec = spec.id ∧
      obligationInstance.owner = spec.owner

def AllContractRequiredObligationsDischarged
    (s : SemanticState V X)
    (contract : GoalContract V X) : Prop :=
  ∀ spec,
    spec ∈ contract.obligations →
    ContractRequiredAt s spec →
    ObligationDischarged s spec.id

def ContractCompletionCriterion
    (s : SemanticState V X)
    (runId : X.RunId)
    (contract : GoalContract V X) : Prop :=
  ContractConditionHolds s contract.completionCondition ∧
  AllContractRequiredObligationsInstantiated s runId contract ∧
  AllContractRequiredObligationsDischarged s contract ∧
  NoOpenGoalRelevantWork s runId contract.goal.id ∧
  NoWaitingGoalBlockers s runId contract.goal.id

/--
Il kernel non può impostare GoalCompleted senza il criterion e, quando il
criterion è raggiunto, la commit di completion è parte della stessa chiusura
semantica. Questo elimina ProgramCompletionSoundness come assumption esterna.
-/
def ContractCompletionCommitSound
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  (∀ n,
    start ≤ n →
    GoalCompleted (certified.run.semanticState n) contract.goal.id →
    ContractCompletionCriterion
      (certified.run.semanticState n)
      runId
      contract) ∧
  (∀ n,
    start ≤ n →
    ContractCompletionCriterion
      (certified.run.semanticState n)
      runId
      contract →
    GoalCompleted
      (certified.run.semanticState n)
      contract.goal.id)


/--
Frontiera concreta del sistema collaborativo: o esiste work immediatamente
eseguibile/claimed, oppure esiste un blocker waiting esplicito.
-/
def ContractProgressFrontierAt
    (s : SemanticState V X)
    (runId : X.RunId)
    (contract : GoalContract V X) : Prop :=
  (∃ workId work,
      s.workItems workId = some work ∧
      work.run = runId ∧
      work.goal = contract.goal.id ∧
      (work.status = WorkStatus.eligible ∨
       work.status = WorkStatus.claimed)) ∨
  (∃ blockerId blocker,
      s.blockers blockerId = some blocker ∧
      blocker.run = runId ∧
      blocker.goal = contract.goal.id ∧
      BlockerWaiting blocker)

/--
Le required obligation sono istanziate per costruzione del kernel.
-/
theorem all_contract_required_instantiated_of_kernel
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start n : Nat)
    (kernel :
      CollaborativeKernelCertificate
        certified runId contract start)
    (startLeN : start ≤ n) :
    AllContractRequiredObligationsInstantiated
      (certified.run.semanticState n)
      runId
      contract := by
  intro spec specIn required
  exact
    kernel.requiredObligationClosure
      n spec startLeN specIn required

/--
FRONTIER COMPLETENESS PROVATA dai fatti locali del kernel.
Una run non può essere semanticamente incompleta e contemporaneamente priva di
work/blocker osservabile.
-/
theorem contract_progress_frontier_of_not_completed
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start n : Nat)
    (kernel :
      CollaborativeKernelCertificate
        certified runId contract start)
    (completionCommit :
      ContractCompletionCommitSound
        certified runId contract start)
    (startLeN : start ≤ n)
    (goalNotCompleted :
      ¬ GoalCompleted
          (certified.run.semanticState n)
          contract.goal.id) :
    ContractProgressFrontierAt
      (certified.run.semanticState n)
      runId
      contract := by
  classical

  by_cases allDischarged :
      AllContractRequiredObligationsDischarged
        (certified.run.semanticState n)
        contract

  · by_cases noOpen :
        NoOpenGoalRelevantWork
          (certified.run.semanticState n)
          runId
          contract.goal.id

    · by_cases noWaiting :
          NoWaitingGoalBlockers
            (certified.run.semanticState n)
            runId
            contract.goal.id

      · have completionCondition :
            ContractConditionHolds
              (certified.run.semanticState n)
              contract.completionCondition := by
          rw [kernel.contractWellFormed.completionConditionNormalized]
          trivial

        have instantiated :=
          all_contract_required_instantiated_of_kernel
            certified runId contract start n kernel startLeN

        have criterion :
            ContractCompletionCriterion
              (certified.run.semanticState n)
              runId
              contract :=
          ⟨completionCondition,
           instantiated,
           allDischarged,
           noOpen,
           noWaiting⟩

        have completed :=
          completionCommit.2 n startLeN criterion

        exact (goalNotCompleted completed).elim

      ·
        simp [NoWaitingGoalBlockers] at noWaiting
        obtain
          ⟨blockerId, blocker, blockerAt, blockerRun,
            blockerGoal, blockerNotTerminal⟩ := noWaiting

        have blockerWaiting : BlockerWaiting blocker := by
          cases statusEq : blocker.status with
          | waiting =>
              exact statusEq
          | resolved =>
              exfalso
              apply blockerNotTerminal
              exact Or.inl statusEq
          | failed =>
              exfalso
              apply blockerNotTerminal
              exact Or.inr (Or.inl statusEq)
          | cancelled =>
              exfalso
              apply blockerNotTerminal
              exact Or.inr (Or.inr statusEq)

        exact
          Or.inr
            ⟨blockerId, blocker, blockerAt,
             blockerRun, blockerGoal, blockerWaiting⟩

    ·
      simp [NoOpenGoalRelevantWork] at noOpen
      obtain
        ⟨workId, work, workAt, workRun, workGoal,
          notSucceeded, notFailed, notCancelled⟩ := noOpen

      cases statusEq : work.status with
      | blocked =>
          obtain
            ⟨blockerId, blocker, blockerAt, blockerRun,
              blockerGoal, applies, waiting⟩ :=
            kernel.blockedWorkHasWaitingBlocker
              n workId work startLeN workAt
              workRun workGoal statusEq
          exact
            Or.inr
              ⟨blockerId, blocker, blockerAt,
               blockerRun, blockerGoal, waiting⟩

      | eligible =>
          exact
            Or.inl
              ⟨workId, work, workAt, workRun, workGoal,
               Or.inl statusEq⟩

      | claimed =>
          exact
            Or.inl
              ⟨workId, work, workAt, workRun, workGoal,
               Or.inr statusEq⟩

      | succeeded =>
          exact (notSucceeded statusEq).elim

      | failed =>
          exact (notFailed statusEq).elim

      | cancelled =>
          exact (notCancelled statusEq).elim

  ·
    simp [AllContractRequiredObligationsDischarged] at allDischarged
    obtain
      ⟨spec, specIn, required, notDischarged⟩ := allDischarged

    obtain
      ⟨obligationInstance, instanceAt, instanceRun,
        instanceSpec, instanceOwner⟩ :=
      kernel.requiredObligationClosure
        n spec startLeN specIn required

    have instanceActive :
        obligationInstance.status = ObligationStatus.active := by
      cases statusEq : obligationInstance.status with
      | active =>
          rfl
      | discharged =>
          exfalso
          apply notDischarged
          exact ⟨obligationInstance, instanceAt, statusEq⟩

    obtain ⟨minimal, minimalRun, minimalActive⟩ :=
      kernel.requiredActiveHasMinimalFrontier
        n spec obligationInstance startLeN specIn required
        instanceAt instanceRun instanceActive

    obtain
      ⟨entry, workId, work, certificate,
        entryIn, entryObligation, entryFlag,
        workAt, certificateAt, certificateSpec,
        certificateSlot, workRun, workGoal,
        workServes, workStatus⟩ :=
      kernel.entryWorkClosure
        n minimal startLeN minimalRun minimalActive

    cases workStatus with
    | inl eligible =>
        exact
          Or.inl
            ⟨workId, work, workAt, workRun, workGoal,
             Or.inl eligible⟩
    | inr remaining =>
        cases remaining with
        | inl blocked =>
            obtain
              ⟨blockerId, blocker, blockerAt, blockerRun,
                blockerGoal, applies, waiting⟩ :=
              kernel.blockedWorkHasWaitingBlocker
                n workId work startLeN workAt
                workRun workGoal blocked
            exact
              Or.inr
                ⟨blockerId, blocker, blockerAt,
                 blockerRun, blockerGoal, waiting⟩
        | inr claimed =>
            exact
              Or.inl
                ⟨workId, work, workAt, workRun, workGoal,
                 Or.inr claimed⟩

def CollaborativeContractCompletedAt
    (s : SemanticState V X)
    (runId : X.RunId)
    (contract : GoalContract V X) : Prop :=
  GoalCompleted s contract.goal.id ∧
  ContractCompletionCriterion s runId contract

def EventuallyCollaborativeContractCompleted
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  ∃ m,
    start ≤ m ∧
    CollaborativeContractCompletedAt
      (run.semanticState m)
      runId
      contract

def CollaborativeContractLocalProgressAfter
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  ∀ n,
    start ≤ n →
    GoalValid (run.semanticState n) contract.goal.id →
    ¬ CollaborativeContractCompletedAt
        (run.semanticState n)
        runId
        contract →
    ∃ m,
      n < m ∧
      (CollaborativeContractCompletedAt
          (run.semanticState m)
          runId
          contract ∨
       measure.rank (run.semanticState m) runId contract.goal.id <
       measure.rank (run.semanticState n) runId contract.goal.id)

/--
Anti-loop contract-native. Non richiede un SemanticProgram intermedio.
-/
structure ContractGlobalMultiAgentAntiLoop
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where
  dependencyRanked :
    ContractDependencyRanked contract

  generationRanked :
    ContractGenerationRanked contract

  finiteRelevantWork :
    FiniteGoalRelevantWorkAfter
      certified.run
      runId
      contract.goal.id
      start

  causalGraph :
    GlobalCausalGraphWellFoundedAfter
      certified.run
      runId
      contract.goal.id
      start

  certifiedGeneration :
    CollaborativeKernelCertificate
      certified
      runId
      contract
      start

/-! ### R5.30.7B — Evidence/discharge/completion come closure del kernel -/

def ContractDischargeSoundness
    (judge : SemanticEvidenceJudge V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  ∀ n obligation obligationInstance,
    start ≤ n →
    (certified.run.semanticState n).obligations obligation = some obligationInstance →
    obligationInstance.status = ObligationStatus.discharged →
    ∃ evidence,
      evidence ∈ (certified.run.semanticState n).evidences ∧
      evidence.obligation = obligation ∧
      ContractEvidenceValid judge certified runId contract evidence

def ContractAcceptedEvidenceCloses
    (judge : SemanticEvidenceJudge V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  ∀ n evidence,
    start ≤ n →
    evidence ∈ (certified.run.semanticState n).evidences →
    ContractEvidenceValid judge certified runId contract evidence →
    ObligationDischarged
      (certified.run.semanticState n)
      evidence.obligation

structure EvidenceDischargeKernelCertificate
    (judge : SemanticEvidenceJudge V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where
  dischargeSound :
    ContractDischargeSoundness
      judge certified runId contract start

  acceptedEvidenceCloses :
    ContractAcceptedEvidenceCloses
      judge certified runId contract start

  completionCommit :
    ContractCompletionCommitSound
      certified runId contract start


structure AssumptionMinimalSuccessKernelCertificate
    (policy : AgingSchedulerPolicy)
    (judge : SemanticEvidenceJudge V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where
  base :
    AssumptionMinimalKernelCertificate
      policy certified runId contract start

  evidenceDischarge :
    EvidenceDischargeKernelCertificate
      judge certified runId contract start


/--
WORK EXISTENCE PROVATO dalla closure entry-slot del kernel.
-/
theorem contract_work_existence_of_kernel
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start n : Nat)
    (kernel :
      CollaborativeKernelCertificate
        certified runId contract start)
    (startLeN : start ≤ n) :
    ContractWorkExistence
      (certified.run.semanticState n)
      contract
      runId := by
  intro obligation obligationRun minimal
  obtain
    ⟨entry, workId, work, certificate,
      entryIn, entryObligation, entryFlag,
      workAt, certificateAt, certificateSpec,
      certificateSlot, workRun, workGoal,
      workServes, workStatus⟩ :=
    kernel.entryWorkClosure
      n obligation startLeN obligationRun minimal
  exact
    ⟨workId, work, workAt, workRun, workGoal, workServes, workStatus⟩

/--
ANTI-LOOP GLOBALE PROVATO: rank statici + slot finiti + causal rank.
-/
theorem contract_global_multi_agent_anti_loop_of_kernel
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (kernel :
      CollaborativeKernelCertificate
        certified runId contract start) :
    ContractGlobalMultiAgentAntiLoop
      certified runId contract start := by
  refine
    { dependencyRanked := ?_
      generationRanked := ?_
      finiteRelevantWork := ?_
      causalGraph := ?_
      certifiedGeneration := kernel }

  · exact
      contract_dependency_ranked_of_well_formed
        contract kernel.contractWellFormed

  · exact
      contract_generation_ranked_of_well_formed
        contract kernel.contractWellFormed

  · exact
      finite_goal_relevant_work_after_of_contract
        certified runId contract start kernel

  · exact
      global_causal_graph_well_founded_after_of_rank
        certified runId contract start kernel

/--
Il bound dei retry per ogni work certificato segue dal WorkSpec del suo slot.
-/
theorem certified_work_attempt_bounded
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start n : Nat)
    (kernel :
      CollaborativeKernelCertificate
        certified runId contract start)
    (workId : X.WorkItemId)
    (work : WorkItem V X)
    (certificate : WorkInstanceCertificate V X)
    (workSpec : ContractWorkSpec V X)
    (startLeN : start ≤ n)
    (workAt :
      (certified.run.semanticState n).workItems workId = some work)
    (certificateAt :
      certified.workCertificateAt n workId = some certificate)
    (workRun : work.run = runId)
    (workGoal : work.goal = contract.goal.id)
    (workSpecIn : workSpec ∈ contract.workSpecs)
    (workSpecId : workSpec.id = certificate.workSpecId) :
    work.attempt < workSpec.maxAttempts := by

  obtain
    ⟨observedCertificate, observedSpec,
      observedCertificateAt, observedCertificateWork,
      observedSpecIn, observedSpecId, observedObligation,
      observedOwner, observedKind, observedSlotBound,
      observedAttemptBound⟩ :=
    kernel.allRelevantWorkCertified
      n workId work startLeN workAt workRun workGoal

  have certificateEq : observedCertificate = certificate := by
    rw [certificateAt] at observedCertificateAt
    cases observedCertificateAt
    rfl

  have specIdEq : observedSpec.id = workSpec.id := by
    calc
      observedSpec.id = observedCertificate.workSpecId :=
        observedSpecId
      _ = certificate.workSpecId := by
        rw [certificateEq]
      _ = workSpec.id := workSpecId.symm

  have specEq : observedSpec = workSpec :=
    kernel.contractWellFormed.uniqueWorkSpecIds
      observedSpec
      workSpec
      observedSpecIn
      workSpecIn
      specIdEq

  rw [← specEq]
  exact observedAttemptBound

/-! ### R5.30.8 — Derivazioni interne che NON sono boundary assumptions -/

/--
Queste proprietà sono obiettivi di lemma del kernel. Il completion theorem
assumption-minimal non le classifica come fatti esterni.
-/
structure InternalDerivationTargets
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where

  finiteWorkDerived :
    FiniteGoalRelevantWorkAfter
      certified.run
      runId
      contract.goal.id
      start

  contractWorkExistenceDerived :
    ∀ n,
      start ≤ n →
      ContractWorkExistence
        (certified.run.semanticState n)
        contract
        runId

  schedulerFairnessDerived :
    ContractCollaborativeWorkWeakFairnessAfter
      certified.run
      runId
      contract
      start

  globalAntiLoopDerived :
    ContractGlobalMultiAgentAntiLoop
      certified
      runId
      contract
      start

  /--
  Ogni claim/retry/recovery resta vincolato agli slot e ai bound della WorkSpec;
  nessun retry può creare capacità di lavoro non prevista dal contratto.
  -/
  boundedRetryDerived :
    ∀ n workId work certificate workSpec,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      certified.workCertificateAt n workId = some certificate →
      work.run = runId →
      work.goal = contract.goal.id →
      workSpec ∈ contract.workSpecs →
      workSpec.id = certificate.workSpecId →
      work.attempt < workSpec.maxAttempts

  /--
  Le dependency inter-agent non sono un'assunzione separata: la prerequisite
  segue la stessa pipeline work/evidence/discharge di qualunque owner.
  -/
  crossAgentUsesSameContractFrontier :
    ∀ n target prerequisite targetInstance sourceInstance,
      start ≤ n →
      ContractDependsOn contract target prerequisite →
      (certified.run.semanticState n).obligations target = some targetInstance →
      (certified.run.semanticState n).obligations prerequisite = some sourceInstance →
      targetInstance.owner ≠ sourceInstance.owner →
      sourceInstance.status = ObligationStatus.active →
      ContractWorkExistence
        (certified.run.semanticState n)
        contract
        runId

/--
OBBLIGO CENTRALE DI RIDUZIONE DELLE ASSUNZIONI.
Qui devono essere dimostrati i vecchi predicati aggregati usando solamente
GoalContractWellFormed, certificati locali del kernel, fairness derivata e le
boundary esterne strettamente necessarie per i blocker non controllabili.
-/
theorem DeriveInternalTargetsFromStructureObligation
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) :
    AssumptionMinimalKernelCertificate
        policy certified runId contract start →
    GoalContractWellFormed contract →
    InternalDerivationTargets
        certified runId contract start := by
  intro bundle wellFormed
  let kernel := bundle.core

  refine
    { finiteWorkDerived := ?_
      contractWorkExistenceDerived := ?_
      schedulerFairnessDerived := ?_
      globalAntiLoopDerived := ?_
      boundedRetryDerived := ?_
      crossAgentUsesSameContractFrontier := ?_ }

  · exact
      finite_goal_relevant_work_after_of_contract
        certified runId contract start kernel

  · intro n startLeN
    exact
      contract_work_existence_of_kernel
        certified runId contract start n kernel startLeN

  · exact
      contract_scheduler_fairness_of_position_descent
        policy
        certified
        runId
        contract
        start
        bundle.scheduler

  · exact
      contract_global_multi_agent_anti_loop_of_kernel
        certified runId contract start kernel

  · intro n workId work certificate workSpec
      startLeN workAt certificateAt workRun workGoal workSpecIn workSpecId
    exact
      certified_work_attempt_bounded
        certified
        runId
        contract
        start
        n
        kernel
        workId
        work
        certificate
        workSpec
        startLeN
        workAt
        certificateAt
        workRun
        workGoal
        workSpecIn
        workSpecId

  · intro n target prerequisite targetInstance sourceInstance
      startLeN dependency targetAt sourceAt differentOwners sourceActive
    exact
      contract_work_existence_of_kernel
        certified runId contract start n kernel startLeN

/-! ### R5.30.9 — Boundary assumptions realmente residue -/

/--
Progress umano: la risposta è l'unica liveness fisica che il kernel non può
forzare quando non esiste fallback interno. Il witness è strettamente futuro.
-/
def HumanProgressBoundary
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n blockerId blocker,
    (run.semanticState n).blockers blockerId = some blocker →
    BlockerWaiting blocker →
    HumanControlledBlockerAt (run.semanticState n) blocker →
    ∃ m later,
      n < m ∧
      (run.semanticState m).blockers blockerId = some later ∧
      BlockerTerminal later

/-- Progress/terminalità di vere condizioni esterne al sistema Sprout. -/
def ExternalEnvironmentProgressBoundary
    (run : ObservedSemanticRun V X) : Prop :=
  ∀ n blockerId blocker,
    (run.semanticState n).blockers blockerId = some blocker →
    ExternalControlledBlocker blocker →
    BlockerWaiting blocker →
    ∃ m later,
      n < m ∧
      (run.semanticState m).blockers blockerId = some later ∧
      BlockerTerminal later

/--
Boundary completa per la sola TERMINAZIONE del contratto strutturato.
Non include PromptContractAdequacy: il sistema può terminare rispetto a un
contratto anche se il compiler AI lo ha interpretato male.
-/
structure MinimalTerminationExternalAssumptions
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start : Nat) : Prop where
  humanProgress :
    HumanProgressBoundary run

  externalEnvironmentProgress :
    ExternalEnvironmentProgressBoundary run

  stableGovernanceSegment :
    GoalRevisionStableAfter run goal start

/--
Boundary aggiuntive per poter parlare di SUCCESSO rispetto al prompt originale.
-/
structure MinimalContractSuccessExternalAssumptions
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start : Nat) : Prop where

  terminationBoundary :
    MinimalTerminationExternalAssumptions
      run
      goal
      start

  /--
  Boundary di SUCCESSO concreta: nel segmento non si osserva un terminale
  fallito/cancellato/superseded. Nessuna assunzione linguistica è necessaria
  per dimostrare il successo rispetto al GoalContract stesso.
  -/
  noUnsuccessfulTerminal :
    ∀ n,
      start ≤ n →
      ¬ GoalFailed (run.semanticState n) goal ∧
      ¬ GoalCancelled (run.semanticState n) goal ∧
      (run.semanticState n).goalStatus goal ≠
        some GoalStatus.superseded

def SemanticEvidenceJudgeAdequateForContract
    (evidenceJudge : SemanticEvidenceJudge V X)
    (intendedEvidence : IntendedEvidenceSemantics V X)
    (contract : GoalContract V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId) : Prop :=
  ∀ evidence rule,
    rule ∈ contract.evidenceRules →
    rule.verification = EvidenceVerificationMode.semanticJudgment →
    rule.obligation = evidence.obligation →
    rule.kind = evidence.kind →
    ContractEvidenceSubjectMatches
      certified
      runId
      contract
      evidence
      rule.subject →
    (evidenceJudge.adequate contract certified.run evidence ↔
     intendedEvidence.satisfies contract certified.run evidence)

/--
Boundary SEMANTICA aggiuntiva. Non serve alla matematica del GoalContract:
serve soltanto a interpretare la sua completion come fedele al prompt e al
significato intenzionale delle evidence non meccaniche.
-/
structure MinimalPromptFaithfulSuccessExternalAssumptions
    (meaning : PromptContractSemantics V X)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (intendedEvidence : IntendedEvidenceSemantics V X)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat) : Prop where

  contractSuccess :
    MinimalContractSuccessExternalAssumptions
      certified.run
      (compiler.compile prompt).goal.id
      start

  promptAdequacy :
    PromptContractAdequacy meaning compiler prompt

  semanticEvidenceAdequacy :
    SemanticEvidenceJudgeAdequateForContract
      evidenceJudge
      intendedEvidence
      (compiler.compile prompt)
      certified
      runId


/--
Validità evidence rispetto alla semantica INTENZIONALE del dominio.
Per le regole mechanical coincide con il verificatore meccanico; per le
semanticJudgment usa IntendedEvidenceSemantics anziché il judge operativo.
-/
def IntendedContractEvidenceValid
    (intended : IntendedEvidenceSemantics V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (evidence : Evidence V X) : Prop :=
  ∃ rule,
    rule ∈ contract.evidenceRules ∧
    rule.obligation = evidence.obligation ∧
    rule.kind = evidence.kind ∧
    ContractEvidenceSubjectMatches
      certified runId contract evidence rule.subject ∧
    match rule.verification with
    | EvidenceVerificationMode.mechanical =>
        MechanicalEvidenceValid certified.run evidence
    | EvidenceVerificationMode.semanticJudgment =>
        intended.satisfies contract certified.run evidence

def IntendedContractDischargeSoundness
    (intended : IntendedEvidenceSemantics V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop :=
  ∀ n obligation obligationInstance,
    start ≤ n →
    (certified.run.semanticState n).obligations obligation = some obligationInstance →
    obligationInstance.status = ObligationStatus.discharged →
    ∃ evidence,
      evidence ∈ (certified.run.semanticState n).evidences ∧
      evidence.obligation = obligation ∧
      IntendedContractEvidenceValid
        intended certified runId contract evidence

/-! ### R5.30.10 — Terminazione distinta dal successo -/

def GoalTerminal
    (s : SemanticState V X)
    (goal : X.GoalId) : Prop :=
  GoalCompleted s goal ∨
  GoalFailed s goal ∨
  GoalCancelled s goal ∨
  s.goalStatus goal = some GoalStatus.superseded

/--
Persistenza appropriata al theorem di terminazione: il goal resta valido
finché non è diventato terminale. Non esclude failed/cancelled.
-/
def GoalValidityPersistsUntilTerminal
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId) : Prop :=
  ∀ n m,
    n ≤ m →
    GoalValid (run.semanticState n) goal →
    ¬ GoalTerminal (run.semanticState m) goal →
    GoalValid (run.semanticState m) goal

def WorkStatusTerminal (status : WorkStatus) : Prop :=
  status = WorkStatus.succeeded ∨
  status = WorkStatus.failed ∨
  status = WorkStatus.cancelled

/--
Avanzamento osservabile di uno specifico WorkItem tra due tick.
Non contiene alcuna nozione semantica opaca: attempt, terminal status,
discharge e child-work sono tutti fatti persistiti.
-/
def ContractWorkAdvancedBetween
    (run : ObservedSemanticRun V X)
    (workId : X.WorkItemId)
    (fromTick toTick : Nat) : Prop :=
  ∃ before,
    (run.semanticState fromTick).workItems workId = some before ∧
    (ObligationDischarged
        (run.semanticState toTick)
        before.serves ∨
     (∃ after,
        (run.semanticState toTick).workItems workId = some after ∧
        (before.attempt < after.attempt ∨
         WorkStatusTerminal after.status)) ∨
     (∃ childId child,
        (run.semanticState toTick).workItems childId = some child ∧
        child.parent = some workId))

/--
Dinamica interna con witness di deadline. I witness sono dati certificabili
della run, non assunzioni sull'ambiente.
-/
structure ContractExecutionDynamicsCertificate
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where

  /--
  La deadline non è scelta liberamente: è `n + maxResolutionTicks` della
  WorkSpec statica certificata.
  -/
  selectedWorkResolvesWithinSpecBound :
    ∀ n workId work certificate workSpec,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      certified.workCertificateAt n workId = some certificate →
      workSpec ∈ contract.workSpecs →
      workSpec.id = certificate.workSpecId →
      SelectedAt certified.run workId n →
      let m := n + workSpec.maxResolutionTicks
      n < m ∧
      (GoalTerminal
          (certified.run.semanticState m)
          contract.goal.id ∨
       ContractWorkAdvancedBetween
          certified.run workId n m)

  claimedWorkResolvesWithinSpecBound :
    ∀ n workId work certificate workSpec,
      start ≤ n →
      (certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      certified.workCertificateAt n workId = some certificate →
      workSpec ∈ contract.workSpecs →
      workSpec.id = certificate.workSpecId →
      work.status = WorkStatus.claimed →
      let m := n + workSpec.maxResolutionTicks
      n < m ∧
      (GoalTerminal
          (certified.run.semanticState m)
          contract.goal.id ∨
       ContractWorkAdvancedBetween
          certified.run workId n m)

/--
Leggi locali della misura. Sono verificabili su coppie di stati/eventi e non
assumono direttamente eventual completion.
-/
structure ContractProgressMeasureLaws
    (measure : ProgressMeasure V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where

  nonIncreasing :
    ∀ n m,
      start ≤ n →
      n ≤ m →
      ¬ GoalTerminal
          (certified.run.semanticState n)
          contract.goal.id →
      measure.rank
          (certified.run.semanticState m)
          runId
          contract.goal.id ≤
        measure.rank
          (certified.run.semanticState n)
          runId
          contract.goal.id

  workAdvanceStrict :
    ∀ n m workId,
      start ≤ n →
      n < m →
      ContractWorkAdvancedBetween
        certified.run workId n m →
      measure.rank
          (certified.run.semanticState m)
          runId
          contract.goal.id <
        measure.rank
          (certified.run.semanticState n)
          runId
          contract.goal.id

  blockerResolutionStrict :
    ∀ n m blockerId blocker later,
      start ≤ n →
      n < m →
      (certified.run.semanticState n).blockers blockerId = some blocker →
      BlockerWaiting blocker →
      (certified.run.semanticState m).blockers blockerId = some later →
      BlockerTerminal later →
      measure.rank
          (certified.run.semanticState m)
          runId
          contract.goal.id <
        measure.rank
          (certified.run.semanticState n)
          runId
          contract.goal.id

/--
Certificato completo ma ancora INTERNO: combina queue, transition deadlines,
completion commit e leggi locali del rank.
-/
structure AssumptionMinimalProgressKernelCertificate
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where

  base :
    AssumptionMinimalKernelCertificate
      policy certified runId contract start

  completionCommit :
    ContractCompletionCommitSound
      certified runId contract start

  dynamics :
    ContractExecutionDynamicsCertificate
      certified runId contract start

  measureLaws :
    ContractProgressMeasureLaws
      measure certified runId contract start

  goalValidityPersistsUntilTerminal :
    GoalValidityPersistsUntilTerminal
      certified.run
      contract.goal.id

  goalValidAtStart :
    GoalValid
      (certified.run.semanticState start)
      contract.goal.id

/--
Bundle interno completo per il theorem di successo.
-/
structure AssumptionMinimalFullSuccessKernelCertificate
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (judge : SemanticEvidenceJudge V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where

  progress :
    AssumptionMinimalProgressKernelCertificate
      measure policy certified runId contract start

  evidenceDischarge :
    EvidenceDischargeKernelCertificate
      judge certified runId contract start

def EventuallyCollaborativeTerminal
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  ∃ m,
    start ≤ m ∧
    GoalTerminal (run.semanticState m) goal

/--
Progresso locale per la TERMINAZIONE: non pretende che ogni failure esterno
diventi successo.
-/
def CollaborativeTerminationLocalProgressAfter
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  ∀ n,
    start ≤ n →
    GoalValid (run.semanticState n) goal →
    ¬ GoalTerminal (run.semanticState n) goal →
    ∃ m,
      n < m ∧
      (GoalTerminal (run.semanticState m) goal ∨
       measure.rank (run.semanticState m) runId goal <
       measure.rank (run.semanticState n) runId goal)

/--
LOCAL PROGRESS DI TERMINAZIONE PROVATO dalla composizione dei certificati.
-/
theorem collaborative_termination_local_progress_of_certificates
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (progressKernel :
      AssumptionMinimalProgressKernelCertificate
        measure policy certified runId contract start)
    (external :
      MinimalTerminationExternalAssumptions
        certified.run contract.goal.id start) :
    CollaborativeTerminationLocalProgressAfter
      measure
      certified.run
      runId
      contract.goal.id
      start := by

  intro n startLeN goalValid notTerminal

  let kernel := progressKernel.base.core

  have notCompleted :
      ¬ GoalCompleted
          (certified.run.semanticState n)
          contract.goal.id := by
    intro completed
    exact notTerminal (Or.inl completed)

  have frontier :=
    contract_progress_frontier_of_not_completed
      certified
      runId
      contract
      start
      n
      kernel
      progressKernel.completionCommit
      startLeN
      notCompleted

  cases frontier with
  | inl workFrontier =>
      obtain
        ⟨workId, work, workAt, workRun, workGoal, status⟩ :=
        workFrontier

      cases status with
      | inl eligibleStatus =>
          have eligibleSemantic :=
            kernel.eligibleWorkStatusSound
              n workId work startLeN workAt
              workRun workGoal eligibleStatus

          have eligibleAtN :
              ContractWorkEligibleAt
                certified.run contract work n :=
            ⟨by
               rw [kernel.workIdentity n workId work startLeN workAt]
               exact workAt,
             eligibleSemantic⟩

          have workIdentity : work.id = workId :=
            kernel.workIdentity n workId work startLeN workAt

          obtain
            ⟨selectedTick, nLeSelected,
              selectedAt, eligibleAtSelected⟩ :=
            contract_scheduler_eventually_selects_eligible
              policy
              certified
              runId
              contract
              start
              progressKernel.base.scheduler
              work
              n
              startLeN
              workRun
              workGoal
              eligibleAtN

          have startLeSelected :
              start ≤ selectedTick :=
            Nat.le_trans startLeN nLeSelected

          obtain
            ⟨certificate, workSpec,
              certificateAt, certificateWork,
              workSpecIn, workSpecId,
              workSpecObligation, workSpecOwner,
              workSpecKind, slotBound, attemptBound⟩ :=
            kernel.allRelevantWorkCertified
              selectedTick
              workId
              work
              startLeSelected
              (by simpa [workIdentity] using eligibleAtSelected.1)
              workRun
              workGoal

          have resolution :=
            progressKernel.dynamics.selectedWorkResolvesWithinSpecBound
              selectedTick
              workId
              work
              certificate
              workSpec
              startLeSelected
              (by simpa [workIdentity] using eligibleAtSelected.1)
              workRun
              workGoal
              certificateAt
              workSpecIn
              workSpecId
              (by simpa [workIdentity] using selectedAt)

          dsimp only at resolution

          obtain ⟨selectedLtResolution, result⟩ := resolution

          cases result with
          | inl terminal =>
              exact
                ⟨selectedTick + workSpec.maxResolutionTicks,
                 Nat.lt_of_le_of_lt
                   nLeSelected
                   selectedLtResolution,
                 Or.inl terminal⟩

          | inr advance =>
              have rankAfterLtSelected :=
                progressKernel.measureLaws.workAdvanceStrict
                  selectedTick
                  (selectedTick + workSpec.maxResolutionTicks)
                  workId
                  startLeSelected
                  selectedLtResolution
                  advance

              have rankSelectedLeStart :=
                progressKernel.measureLaws.nonIncreasing
                  n
                  selectedTick
                  startLeN
                  nLeSelected
                  notTerminal

              have rankAfterLtStart :
                  measure.rank
                      (certified.run.semanticState
                        (selectedTick + workSpec.maxResolutionTicks))
                      runId
                      contract.goal.id <
                    measure.rank
                      (certified.run.semanticState n)
                      runId
                      contract.goal.id :=
                Nat.lt_of_lt_of_le
                  rankAfterLtSelected
                  rankSelectedLeStart

              exact
                ⟨selectedTick + workSpec.maxResolutionTicks,
                 Nat.lt_of_le_of_lt
                   nLeSelected
                   selectedLtResolution,
                 Or.inr rankAfterLtStart⟩

      | inr claimedStatus =>
          obtain
            ⟨certificate, workSpec,
              certificateAt, certificateWork,
              workSpecIn, workSpecId,
              workSpecObligation, workSpecOwner,
              workSpecKind, slotBound, attemptBound⟩ :=
            kernel.allRelevantWorkCertified
              n
              workId
              work
              startLeN
              workAt
              workRun
              workGoal

          have resolution :=
            progressKernel.dynamics.claimedWorkResolvesWithinSpecBound
              n
              workId
              work
              certificate
              workSpec
              startLeN
              workAt
              workRun
              workGoal
              certificateAt
              workSpecIn
              workSpecId
              claimedStatus

          dsimp only at resolution

          obtain ⟨nLtResolution, result⟩ := resolution

          cases result with
          | inl terminal =>
              exact
                ⟨n + workSpec.maxResolutionTicks,
                 nLtResolution,
                 Or.inl terminal⟩

          | inr advance =>
              have rankDecrease :=
                progressKernel.measureLaws.workAdvanceStrict
                  n
                  (n + workSpec.maxResolutionTicks)
                  workId
                  startLeN
                  nLtResolution
                  advance

              exact
                ⟨n + workSpec.maxResolutionTicks,
                 nLtResolution,
                 Or.inr rankDecrease⟩

  | inr blockerFrontier =>
      obtain
        ⟨blockerId, blocker, blockerAt,
          blockerRun, blockerGoal, waiting⟩ :=
        blockerFrontier

      have boundaryClass :=
        kernel.waitingBlockersExternallyControlled
          n blockerId blocker
          startLeN blockerAt
          blockerRun blockerGoal
          waiting

      cases boundaryClass with
      | inl human =>
          obtain
            ⟨m, later, nLtM, laterAt, terminalLater⟩ :=
            external.humanProgress
              n blockerId blocker
              blockerAt waiting human

          have rankDecrease :=
            progressKernel.measureLaws.blockerResolutionStrict
              n m blockerId blocker later
              startLeN nLtM blockerAt waiting
              laterAt terminalLater

          exact ⟨m, nLtM, Or.inr rankDecrease⟩

      | inr environment =>
          obtain
            ⟨m, later, nLtM, laterAt, terminalLater⟩ :=
            external.externalEnvironmentProgress
              n blockerId blocker
              blockerAt environment waiting

          have rankDecrease :=
            progressKernel.measureLaws.blockerResolutionStrict
              n m blockerId blocker later
              startLeN nLtM blockerAt waiting
              laterAt terminalLater

          exact ⟨m, nLtM, Or.inr rankDecrease⟩

/--
Terminazione globale da LocalProgress di terminazione + discesa su Nat.
-/
theorem collaborative_termination_from_well_founded_progress_after
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (validity : GoalValidityPersistsUntilTerminal run goal)
    (start : Nat)
    (progress :
      CollaborativeTerminationLocalProgressAfter
        measure run runId goal start)
    (validStart :
      GoalValid (run.semanticState start) goal) :
    EventuallyCollaborativeTerminal run goal start := by

  let initialRank :=
    measure.rank (run.semanticState start) runId goal

  have aux :
      ∀ rank n,
        start ≤ n →
        measure.rank (run.semanticState n) runId goal = rank →
        GoalValid (run.semanticState n) goal →
        EventuallyCollaborativeTerminal run goal n := by
    intro rank
    induction rank using Nat.strongRecOn with
    | ind rank ih =>
      intro n startLeN rankEq validN

      by_cases terminalN :
          GoalTerminal (run.semanticState n) goal

      · exact
          ⟨n, Nat.le_refl n, terminalN⟩

      · obtain ⟨m, nLtM, result⟩ :=
          progress n startLeN validN terminalN

        cases result with
        | inl terminalM =>
            exact
              ⟨m, Nat.le_of_lt nLtM, terminalM⟩

        | inr rankDecrease =>
            by_cases terminalM :
                GoalTerminal (run.semanticState m) goal

            · exact
                ⟨m, Nat.le_of_lt nLtM, terminalM⟩

            · have validM :
                  GoalValid (run.semanticState m) goal :=
                validity
                  n m
                  (Nat.le_of_lt nLtM)
                  validN
                  terminalM

              have smaller :
                  measure.rank
                      (run.semanticState m)
                      runId
                      goal < rank := by
                rw [← rankEq]
                exact rankDecrease

              have recursive :=
                ih
                  (measure.rank
                    (run.semanticState m)
                    runId
                    goal)
                  smaller
                  m
                  (Nat.le_trans startLeN (Nat.le_of_lt nLtM))
                  rfl
                  validM

              obtain
                ⟨finish, mLeFinish, finishTerminal⟩ :=
                recursive

              exact
                ⟨finish,
                 Nat.le_trans (Nat.le_of_lt nLtM) mLeFinish,
                 finishTerminal⟩

  exact
    aux
      initialRank
      start
      (Nat.le_refl start)
      rfl
      validStart

/--
Obbligo di derivazione della terminazione globale dalle sole strutture interne
certificate + boundary realmente esterne.
-/
theorem SproutCollaborativeTerminationDerivationObligation
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) :
    AssumptionMinimalProgressKernelCertificate
        measure policy certified runId contract start →
    MinimalTerminationExternalAssumptions
        certified.run contract.goal.id start →
    CollaborativeTerminationLocalProgressAfter
        measure
        certified.run
        runId
        contract.goal.id
        start := by
  intro progressKernel external
  exact
    collaborative_termination_local_progress_of_certificates
      measure
      policy
      certified
      runId
      contract
      start
      progressKernel
      external

/--
THEOREM CHIUSO DI TERMINAZIONE COLLABORATIVA.
-/
theorem sprout_assumption_minimal_collaborative_termination
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (progressKernel :
      AssumptionMinimalProgressKernelCertificate
        measure policy certified runId contract start)
    (external :
      MinimalTerminationExternalAssumptions
        certified.run contract.goal.id start) :
    EventuallyCollaborativeTerminal
      certified.run contract.goal.id start := by

  have localProgress :=
    collaborative_termination_local_progress_of_certificates
      measure
      policy
      certified
      runId
      contract
      start
      progressKernel
      external

  exact
    collaborative_termination_from_well_founded_progress_after
      measure
      certified.run
      runId
      contract.goal.id
      progressKernel.goalValidityPersistsUntilTerminal
      start
      localProgress
      progressKernel.goalValidAtStart

/--
Nel boundary di successo del CONTRATTO ogni GoalTerminal è necessariamente
GoalCompleted. Nessuna assunzione linguistica entra in questo lemma.
-/
theorem goal_terminal_is_completed_on_contract_success_path
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start n : Nat)
    (boundary :
      MinimalContractSuccessExternalAssumptions
        run goal start)
    (startLeN : start ≤ n)
    (terminal :
      GoalTerminal (run.semanticState n) goal) :
    GoalCompleted (run.semanticState n) goal := by

  obtain
    ⟨notFailed, notCancelled, notSuperseded⟩ :=
    boundary.noUnsuccessfulTerminal n startLeN

  cases terminal with
  | inl completed =>
      exact completed

  | inr remaining =>
      cases remaining with
      | inl failed =>
          exact (notFailed failed).elim

      | inr remaining =>
          cases remaining with
          | inl cancelled =>
              exact (notCancelled cancelled).elim

          | inr superseded =>
              exact (notSuperseded superseded).elim

/--
Persistenza della validità sul solo segmento stabile.
-/
def GoalValidityPersistsAfter
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start : Nat) : Prop :=
  ∀ n m,
    start ≤ n →
    n ≤ m →
    GoalValid (run.semanticState n) goal →
    GoalValid (run.semanticState m) goal

theorem goal_validity_persists_after_on_contract_success_path
    (run : ObservedSemanticRun V X)
    (goal : X.GoalId)
    (start : Nat)
    (validUntil :
      GoalValidityPersistsUntilTerminal run goal)
    (boundary :
      MinimalContractSuccessExternalAssumptions
        run goal start) :
    GoalValidityPersistsAfter run goal start := by

  intro n m startLeN nLeM validN

  by_cases terminalM :
      GoalTerminal (run.semanticState m) goal

  · have completedM :=
      goal_terminal_is_completed_on_contract_success_path
        run
        goal
        start
        m
        boundary
        (Nat.le_trans startLeN nLeM)
        terminalM

    exact Or.inr completedM

  · exact
      validUntil n m nLeM validN terminalM

/--
Conversione PROVATA da local progress di terminazione a local progress di
successo del GoalContract. Il solo passo aggiuntivo è l'esclusione dei
terminali negativi.
-/
theorem collaborative_success_local_progress_of_termination
    (measure : ProgressMeasure V X)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (completionCommit :
      ContractCompletionCommitSound
        certified runId contract start)
    (terminationProgress :
      CollaborativeTerminationLocalProgressAfter
        measure certified.run runId contract.goal.id start)
    (boundary :
      MinimalContractSuccessExternalAssumptions
        certified.run contract.goal.id start) :
    CollaborativeContractLocalProgressAfter
      measure certified.run runId contract start := by

  intro n startLeN validN notCompleted

  have notTerminal :
      ¬ GoalTerminal
          (certified.run.semanticState n)
          contract.goal.id := by
    intro terminalN

    have completedN :=
      goal_terminal_is_completed_on_contract_success_path
        certified.run
        contract.goal.id
        start
        n
        boundary
        startLeN
        terminalN

    have criterionN :=
      completionCommit.1 n startLeN completedN

    exact notCompleted ⟨completedN, criterionN⟩

  obtain ⟨m, nLtM, result⟩ :=
    terminationProgress
      n startLeN validN notTerminal

  cases result with
  | inl terminalM =>
      have startLeM :
          start ≤ m :=
        Nat.le_trans startLeN (Nat.le_of_lt nLtM)

      have completedM :=
        goal_terminal_is_completed_on_contract_success_path
          certified.run
          contract.goal.id
          start
          m
          boundary
          startLeM
          terminalM

      have criterionM :=
        completionCommit.1 m startLeM completedM

      exact
        ⟨m,
         nLtM,
         Or.inl ⟨completedM, criterionM⟩⟩

  | inr rankDecrease =>
      exact
        ⟨m, nLtM, Or.inr rankDecrease⟩

/--
Obbligo applicativo di SUCCESSO assumption-minimal.
I vecchi campi WorkExistence, BlockerProgress, CrossAgentDependencyProgress,
finiteGoalWork e GlobalMultiAgentAntiLoop NON compaiono come assumptions
esterne: devono essere lemmi derivati dal kernel/GoalContract.
-/
theorem SproutAssumptionMinimalSuccessfulCompletionObligation
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat) :
    AssumptionMinimalFullSuccessKernelCertificate
      measure
      policy
      evidenceJudge
      certified
      runId
      (compiler.compile prompt)
      start →
    MinimalContractSuccessExternalAssumptions
      certified.run
      (compiler.compile prompt).goal.id
      start →
    CollaborativeContractLocalProgressAfter
      measure
      certified.run
      runId
      (compiler.compile prompt)
      start := by

  intro fullKernel boundary

  have terminationProgress :=
    collaborative_termination_local_progress_of_certificates
      measure
      policy
      certified
      runId
      (compiler.compile prompt)
      start
      fullKernel.progress
      boundary.terminationBoundary

  exact
    collaborative_success_local_progress_of_termination
      measure
      certified
      runId
      (compiler.compile prompt)
      start
      fullKernel.progress.completionCommit
      terminationProgress
      boundary

/-!
### R5.30.10A — Teorema matematico contract-native

Una volta dimostrato CollaborativeContractLocalProgressAfter, la conclusione
globale non dipende più dal compiler linguistico o dal runtime concreto.
-/
theorem collaborative_contract_completion_from_well_founded_progress_after
    (measure : ProgressMeasure V X)
    (run : ObservedSemanticRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (validity :
      GoalValidityPersistsAfter
        run contract.goal.id start)
    (progress :
      CollaborativeContractLocalProgressAfter
        measure run runId contract start)
    (validStart :
      GoalValid (run.semanticState start) contract.goal.id) :
    EventuallyCollaborativeContractCompleted
      run runId contract start := by

  let initialRank :=
    measure.rank (run.semanticState start) runId contract.goal.id

  have aux :
      ∀ rank n,
        start ≤ n →
        measure.rank (run.semanticState n) runId contract.goal.id = rank →
        GoalValid (run.semanticState n) contract.goal.id →
        EventuallyCollaborativeContractCompleted
          run runId contract n := by
    intro rank
    induction rank using Nat.strongRecOn with
    | ind rank ih =>
      intro n startLeN rankEq validN
      by_cases completeN :
        CollaborativeContractCompletedAt
          (run.semanticState n)
          runId
          contract
      · exact ⟨n, Nat.le_refl n, completeN⟩
      · obtain ⟨m, nLtM, result⟩ :=
          progress n startLeN validN completeN
        cases result with
        | inl completeM =>
            exact ⟨m, Nat.le_of_lt nLtM, completeM⟩
        | inr rankDecrease =>
            have validM :
                GoalValid (run.semanticState m) contract.goal.id :=
              validity
                n m
                startLeN
                (Nat.le_of_lt nLtM)
                validN
            have smaller :
                measure.rank
                    (run.semanticState m)
                    runId
                    contract.goal.id < rank := by
              simpa [rankEq] using rankDecrease
            have recursive :=
              ih
                (measure.rank
                  (run.semanticState m)
                  runId
                  contract.goal.id)
                smaller
                m
                (Nat.le_trans startLeN (Nat.le_of_lt nLtM))
                rfl
                validM
            obtain ⟨finish, mLeFinish, finishComplete⟩ := recursive
            exact
              ⟨finish,
               Nat.le_trans (Nat.le_of_lt nLtM) mLeFinish,
               finishComplete⟩

  exact aux initialRank start (Nat.le_refl start) rfl validStart

/--
THEOREM FINALE assumption-minimal:
dalle sole boundary residue + certificati interni segue eventual completion
del sistema collaborativo completo rispetto al GoalContract verificato.
-/
theorem sprout_assumption_minimal_successful_completion
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat)
    (fullKernel :
      AssumptionMinimalFullSuccessKernelCertificate
        measure
        policy
        evidenceJudge
        certified
        runId
        (compiler.compile prompt)
        start)
    (boundary :
      MinimalContractSuccessExternalAssumptions
        certified.run
        (compiler.compile prompt).goal.id
        start) :
    EventuallyCollaborativeContractCompleted
      certified.run
      runId
      (compiler.compile prompt)
      start := by

  have localProgress :=
    SproutAssumptionMinimalSuccessfulCompletionObligation
      measure
      policy
      evidenceJudge
      compiler
      prompt
      certified
      runId
      start
      fullKernel
      boundary

  have validity :=
    goal_validity_persists_after_on_contract_success_path
      certified.run
      (compiler.compile prompt).goal.id
      start
      fullKernel.progress.goalValidityPersistsUntilTerminal
      boundary

  exact
    collaborative_contract_completion_from_well_founded_progress_after
      measure
      certified.run
      runId
      (compiler.compile prompt)
      start
      validity
      localProgress
      fullKernel.progress.goalValidAtStart

/--
La boundary semanticEvidenceAdequacy è usata per trasferire la discharge
soundness dal judge operativo alla semantica intenzionale.
-/
theorem intended_discharge_soundness_of_prompt_faithful_boundary
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (meaning : PromptContractSemantics V X)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (intendedEvidence : IntendedEvidenceSemantics V X)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat)
    (fullKernel :
      AssumptionMinimalFullSuccessKernelCertificate
        measure policy evidenceJudge
        certified runId (compiler.compile prompt) start)
    (boundary :
      MinimalPromptFaithfulSuccessExternalAssumptions
        meaning evidenceJudge intendedEvidence
        compiler prompt certified runId start) :
    IntendedContractDischargeSoundness
      intendedEvidence
      certified
      runId
      (compiler.compile prompt)
      start := by

  intro n obligation obligationInstance startLeN instanceAt discharged

  obtain
    ⟨evidence, evidenceIn, evidenceObligation, validEvidence⟩ :=
    fullKernel.evidenceDischarge.dischargeSound
      n obligation obligationInstance
      startLeN instanceAt discharged

  obtain
    ⟨rule, ruleIn, ruleObligation,
      ruleKind, subjectMatches, verification⟩ :=
    validEvidence

  refine
    ⟨evidence,
     evidenceIn,
     evidenceObligation,
     rule,
     ruleIn,
     ruleObligation,
     ruleKind,
     subjectMatches,
     ?_⟩

  cases modeEq : rule.verification with
  | mechanical =>
      simpa [modeEq] using verification

  | semanticJudgment =>
      have adequacy :=
        boundary.semanticEvidenceAdequacy
          evidence
          rule
          ruleIn
          modeEq
          ruleObligation
          ruleKind
          subjectMatches

      have judged :
          evidenceJudge.adequate
            (compiler.compile prompt)
            certified.run
            evidence := by
        simpa [modeEq] using verification

      have intended :
          intendedEvidence.satisfies
            (compiler.compile prompt)
            certified.run
            evidence :=
        adequacy.mp judged

      simpa [modeEq] using intended

/--
Corollario SEMANTICO: oltre alla completion del GoalContract, la boundary
attesta la fedeltà prompt→contratto e la correttezza del semantic evidence
judge rispetto alla semantica intenzionale.
-/
theorem sprout_prompt_faithful_successful_completion
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (meaning : PromptContractSemantics V X)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (intendedEvidence : IntendedEvidenceSemantics V X)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (certified : CertifiedCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat)
    (fullKernel :
      AssumptionMinimalFullSuccessKernelCertificate
        measure policy evidenceJudge
        certified runId (compiler.compile prompt) start)
    (boundary :
      MinimalPromptFaithfulSuccessExternalAssumptions
        meaning evidenceJudge intendedEvidence
        compiler prompt certified runId start) :
    EventuallyCollaborativeContractCompleted
        certified.run runId (compiler.compile prompt) start ∧
      PromptContractAdequacy meaning compiler prompt ∧
      IntendedContractDischargeSoundness
        intendedEvidence
        certified
        runId
        (compiler.compile prompt)
        start := by

  constructor

  · exact
      sprout_assumption_minimal_successful_completion
        measure
        policy
        evidenceJudge
        compiler
        prompt
        certified
        runId
        start
        fullKernel
        boundary.contractSuccess

  · constructor

    · exact boundary.promptAdequacy

    · exact
        intended_discharge_soundness_of_prompt_faithful_boundary
          measure
          policy
          meaning
          evidenceJudge
          intendedEvidence
          compiler
          prompt
          certified
          runId
          start
          fullKernel
          boundary

/-!
### R5.30.11 — Dichiarazione normativa sulle assunzioni residue

Per il completion theorem R5 assumption-minimal:

NON sono boundary assumptions:
* aciclicità/well-foundedness delle dependency;
* finitezza del work graph;
* maxAttempts e maxInstances;
* work existence;
* retry generation;
* failure continuation;
* evidence provenance;
* blocker typing;
* cross-agent dependency progress;
* global multi-agent anti-loop;
* scheduler fairness, quando derivabile dalla policy certificata;
* bookkeeping del CompletionCriterion.

Queste proprietà devono essere validate o dimostrate da GoalContract e kernel.

Le boundary residue sono stratificate:

TERMINATION del GoalContract:
1. eventuale risposta/decisione umana quando non esiste fallback interno;
2. terminalità delle vere condizioni esterne richieste;
3. stabilità della revisione amministrativa nel segmento del theorem.

SUCCESS del GoalContract:
4. nessun terminale failed/cancelled/superseded nel segmento considerato.

FEDELTÀ al prompt/dominio:
5. fedeltà linguistica prompt → GoalContract;
6. correttezza dei soli giudizi semantici di evidence non meccanica.

Le proprietà 5-6 NON sono usate dal theorem matematico di completion del
GoalContract: servono soltanto al corollario prompt-faithful.
-/


/-! ### R5.31 — Dichiarazione normativa sulla globalità del completion theorem -/

/-
INTERPRETAZIONE NORMATIVA INEQUIVOCABILE

Per Sprout R5, il completion theorem applicativo primario è
`sprout_assumption_minimal_successful_completion`.

`SproutAssumptionMinimalSuccessfulCompletionObligation` resta come lemma
intermedio già provato che deriva il LocalProgress di successo. Il corollario
`sprout_prompt_faithful_successful_completion` aggiunge le sole boundary
linguistiche/semantiche necessarie a interpretare la completion come fedele al
system prompt. Le precedenti `CompletionAssumptions` sono un bundle legacy e
non sono più considerate la boundary minimale.

Il soggetto della proprietà NON è un singolo agente. È il sistema
collaborativo identificato da:
* una runId;
* un goal condiviso/program snapshot stabile;
* l'insieme completo dei runParticipants;
* tutte le ObligationInstance richieste dal programma;
* tutti i WorkItem rilevanti, indipendentemente dall'owner;
* tutti i blocker, task, tool outcome ed evidence necessari;
* tutti gli handoff causali inter-agent registrati nel global causal graph.

Pertanto il theorem non è soddisfatto se l'agente A termina mentre B o C hanno
ancora work/obligation/blocker necessario al goal.

Una catena infinita di reazioni A→B→C→A→... che continui a generare nuovo
lavoro rilevante deve violare i rank/slot finiti del `GoalContract` oppure
`ContractGlobalMultiAgentAntiLoop`. Quest'ultima proprietà non è più una
boundary assumption: è un target di derivazione interno.

Le precedenti `CompletionAssumptions` R5.22 restano legacy. Il percorso
normativo assumption-minimal usa:
GoalContractWellFormed → certificati locali del kernel →
contract_progress_frontier_of_not_completed →
contract_scheduler_eventually_selects_eligible / execution dynamics →
collaborative_termination_local_progress_of_certificates →
collaborative_termination_from_well_founded_progress_after →
MinimalContractSuccessExternalAssumptions →
collaborative_success_local_progress_of_termination →
collaborative_contract_completion_from_well_founded_progress_after →
sprout_assumption_minimal_successful_completion.

Le proprietà locali R4 (depth, retry bounds, single-call safety, ecc.) restano
necessarie ma NON sono considerate sufficienti per la terminazione globale.
-/

/-! ### R5.32 — Chiusura degli obblighi formali assumption-minimal -/

/-
Nel sorgente R5.30 gli obblighi precedentemente lasciati come `def ... : Prop`
nel percorso assumption-minimal sono stati sostituiti da theorem con proof body.

Sono chiusi a livello di sorgente:

* FiniteWorkFromContractObligation;
* GlobalAntiLoopFromContractObligation;
* ContractSchedulerFairnessDerivationObligation;
* DeriveInternalTargetsFromStructureObligation;
* contract_progress_frontier_of_not_completed;
* collaborative_termination_local_progress_of_certificates;
* SproutCollaborativeTerminationDerivationObligation;
* sprout_assumption_minimal_collaborative_termination;
* collaborative_success_local_progress_of_termination;
* SproutAssumptionMinimalSuccessfulCompletionObligation;
* collaborative_contract_completion_from_well_founded_progress_after;
* sprout_assumption_minimal_successful_completion;
* sprout_prompt_faithful_successful_completion.

Non esistono `sorry`, `admit` o `by?` in questa specifica.

STATUS DI VERIFICA:
questo file contiene proof term/tactic proof completi, ma la qualifica
"Lean kernel checked" richiede l'esecuzione del toolchain Lean sulla sorgente.
Il contratto normativo non deve confondere "proof body presente" con
"type-check completato".
-/


/-! ### R5.33 — Authority attenuation e information-flow safety collaborativa -/

/-
Questa sezione è un'estensione conservativa del kernel R5.30-R5.32.
Non modifica i theorem di termination/completion già definiti: aggiunge un
safety layer ortogonale che il refinement concreto Sprout deve soddisfare.

Obiettivi normativi:
1. una task assegnata a un principal umano NON è implicitamente assegnata a un
   agente;
2. un umano può delegare lavoro necessario tramite una NUOVA task agentica,
   senza trasferire il controllo della task sorgente;
3. la delegazione user→agent→agent attenua l'autorità transitivamente;
4. un commento è informativo e non costituisce authority provenance;
5. i documenti Info non hanno ACL indipendenti: Read ed EditInfo sono valutati
   sul topic/task-list contenitore e EditInfo coincide intenzionalmente con Read
   del body completo;
6. nessun agente può usare privilegi propri più ampi per produrre effetti oltre
   l'autorità effettiva del work;
7. nessun output agentico può trasferire informazione da una sorgente a un sink
   osservabile da principal che non potevano osservare la sorgente;
8. il contenuto persistito di topic/task-list/task è CANONICO e univoco: non
   esistono varianti per-reader dello stesso body;
9. la personalizzazione per-permesso è ammessa soltanto nella chat contestuale
   privata e supervisionata da un utente;
10. in un autonomous resource privato al controller dell'agente, il ceiling
    informativo è quello del controller; administrator e agent del controller/
    administrator appartengono al trust circle privato;
11. in un autonomous resource condiviso con principal esterni al trust circle,
    il context informativo è l'intersezione delle informazioni leggibili da
    tutta l'audience effettiva del sink condiviso;
12. l'intersezione dell'audience riguarda la CONFIDENZIALITÀ del payload, non
    richiede che ogni reader possieda anche il diritto Write/Manage dell'agente.
-/

/-- Operazioni concrete rilevanti per il refinement del permission engine Sprout. -/
inductive ResourceOperation where
  | viewHeader
  | read
  | editInfo
  | write
  | manage
  | completeAssignedTask
  | delegateAssignedWork
  | readComment
  | postComment
  deriving DecidableEq, Repr

/--
Proiezione del permission engine concreto di Sprout.

La R5 non duplica grant gerarchici, ruoli owner/admin, RLS o distribuzione E2EE:
la concreta implementazione deve raffinare queste relazioni con le decisioni
server/RLS effettive. `editInfo` non è un campo separato: viene derivato
normativamente dalla visibilità plaintext del body, coerentemente con la policy
collaborativa Info e con la separazione authorization/RLS + E2EE.
-/
structure ProductAuthorizationProjection (V : Vocabulary) where
  viewHeaderAllowed : State V → V.PrincipalId → V.ResourceId → Prop
  /-- Visibilità plaintext effettiva: authorization/RLS + materiale E2EE necessario. -/
  bodyReadable : State V → V.PrincipalId → V.ResourceId → Prop
  writeAllowed : State V → V.PrincipalId → V.ResourceId → Prop
  manageAllowed : State V → V.PrincipalId → V.ResourceId → Prop
  /-- Il principal può pubblicare un commento nel contesto della risorsa. -/
  commentAllowed : State V → V.PrincipalId → V.ResourceId → Prop
  /-- Il principal può osservare i commenti pubblicati nel contesto della risorsa. -/
  commentReadable : State V → V.PrincipalId → V.ResourceId → Prop
  toolAllowed : State V → V.PrincipalId → V.Tool → Prop
  /-- Controller umano proprietario/configuratore dell'agente, risolto server-side. -/
  agentController : State V → V.PrincipalId → Option V.PrincipalId
  /-- Ruolo amministrativo effettivo sul progetto concreto della risorsa. -/
  projectAdministrator : State V → V.PrincipalId → V.ProjectId → Prop

/-- Info può essere contenuta soltanto da topic o task-list. -/
def InfoContainer
    (s : State V)
    (resource : V.ResourceId) : Prop :=
  IsResourceKind s resource ResourceKind.topic ∨
  IsResourceKind s resource ResourceKind.taskList

/-- Lettura Info = visibilità plaintext effettiva del body completo del contenitore. -/
def InfoReadAllowed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (principal : V.PrincipalId)
    (container : V.ResourceId) : Prop :=
  InfoContainer s container ∧
  authorization.bodyReadable s principal container

/--
Eccezione collaborativa Info normativa: chi può leggere il body completo del
contenitore può anche creare/modificare/soft-delete Info e i suoi file.
Non viene introdotto alcun ACL indipendente per il documento Info.
-/
def InfoEditAllowed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (principal : V.PrincipalId)
    (container : V.ResourceId) : Prop :=
  InfoReadAllowed authorization s principal container

@[simp] theorem info_edit_allowed_iff_info_read_allowed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (principal : V.PrincipalId)
    (container : V.ResourceId) :
    InfoEditAllowed authorization s principal container ↔
      InfoReadAllowed authorization s principal container := by
  rfl

/-- Valutazione uniforme di un'operazione su una risorsa. -/
def ResourceOperationAllowed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (principal : V.PrincipalId)
    (resource : V.ResourceId) : ResourceOperation → Prop
  | ResourceOperation.viewHeader =>
      authorization.viewHeaderAllowed s principal resource
  | ResourceOperation.read =>
      authorization.bodyReadable s principal resource
  | ResourceOperation.editInfo =>
      InfoEditAllowed authorization s principal resource
  | ResourceOperation.write =>
      authorization.writeAllowed s principal resource
  | ResourceOperation.manage =>
      authorization.manageAllowed s principal resource
  | ResourceOperation.completeAssignedTask =>
      AssignedTo s principal resource ∧ OpenTask s resource
  | ResourceOperation.delegateAssignedWork =>
      (∃ kind, HasKind s principal kind ∧ IsHumanKind kind) ∧
      AssignedTo s principal resource ∧
      OpenTask s resource ∧
      authorization.bodyReadable s principal resource
  | ResourceOperation.readComment =>
      authorization.commentReadable s principal resource
  | ResourceOperation.postComment =>
      authorization.commentAllowed s principal resource

/-- Mutazioni che costituiscono controllo della task e non mera osservazione/commento. -/
def TaskControlOperation : ResourceOperation → Prop
  | ResourceOperation.write => True
  | ResourceOperation.manage => True
  | ResourceOperation.completeAssignedTask => True
  | _ => False

/--
La task è sotto responsabilità umana rispetto all'agente indicato: esiste almeno
un assegnatario umano e l'agente non è fra gli assegnatari. Il creator non è
sufficiente a riottenere controllo implicito.
-/
def HumanAssignedTaskWithoutAgent
    (s : State V)
    (agent : V.PrincipalId)
    (task : V.ResourceId) : Prop :=
  IsResourceKind s task ResourceKind.task ∧
  ¬ AssignedTo s agent task ∧
  ∃ human kind,
    AssignedTo s human task ∧
    HasKind s human kind ∧
    IsHumanKind kind

/-- Un envelope di autorità è un upper bound statico per risorsa/operazione. -/
abbrev AuthorityEnvelope (V : Vocabulary) :=
  V.ResourceId → ResourceOperation → Prop

/-- Inclusione di autorità; `child ⊑ parent` significa nessuna amplificazione. -/
def AuthoritySubset
    (child parent : AuthorityEnvelope V) : Prop :=
  ∀ resource operation,
    child resource operation → parent resource operation

@[simp] theorem authority_subset_refl
    (authority : AuthorityEnvelope V) :
    AuthoritySubset authority authority := by
  intro resource operation allowed
  exact allowed

theorem authority_subset_trans
    {first second third : AuthorityEnvelope V}
    (hFirstSecond : AuthoritySubset first second)
    (hSecondThird : AuthoritySubset second third) :
    AuthoritySubset first third := by
  intro resource operation allowed
  exact hSecondThird resource operation (hFirstSecond resource operation allowed)

/-- Ceiling statico dei tool utilizzabili dal work. -/
abbrev ToolAuthorityEnvelope (V : Vocabulary) := V.Tool → Prop

/-- Inclusione del ceiling dei tool. -/
def ToolAuthoritySubset
    (child parent : ToolAuthorityEnvelope V) : Prop :=
  ∀ tool, child tool → parent tool

@[simp] theorem tool_authority_subset_refl
    (authority : ToolAuthorityEnvelope V) :
    ToolAuthoritySubset authority authority := by
  intro tool allowed
  exact allowed

theorem tool_authority_subset_trans
    {first second third : ToolAuthorityEnvelope V}
    (hFirstSecond : ToolAuthoritySubset first second)
    (hSecondThird : ToolAuthoritySubset second third) :
    ToolAuthoritySubset first third := by
  intro tool allowed
  exact hSecondThird tool (hFirstSecond tool allowed)

/--
Autorità effettiva: il ceiling immutabile del work viene intersecato con i
permessi CORRENTI del principal che ne costituisce l'authority source.
Una revoca può quindi restringere una run già attiva; un nuovo grant non amplia
silenziosamente il ceiling storico del work.
-/
def EffectiveAuthorityAt
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (ceiling : AuthorityEnvelope V)
    (authorityPrincipal : V.PrincipalId)
    (resource : V.ResourceId)
    (operation : ResourceOperation) : Prop :=
  ceiling resource operation ∧
  ResourceOperationAllowed authorization s authorityPrincipal resource operation

/--
Relazione strutturale di containment del resource tree. `scope` contiene se
stesso e tutti i discendenti raggiungibili seguendo `ResourceMeta.parent`.
-/
inductive ResourceWithinScope
    (s : State V)
    (scope : V.ResourceId) : V.ResourceId → Prop where
  | root : ResourceWithinScope s scope scope
  | descend
      {parent resource : V.ResourceId}
      {parentMeta resourceMeta : ResourceMeta V} :
      ResourceWithinScope s scope parent →
      s.resources parent = some parentMeta →
      s.resources resource = some resourceMeta →
      resourceMeta.parent = some parent →
      resourceMeta.projectId = parentMeta.projectId →
      ResourceWithinScope s scope resource

/--
Una task assegnata a un umano non conferisce implicitamente all'agente le
azioni che richiedono assignment esatto sulla stessa task.
-/
theorem unassigned_agent_cannot_mark_human_task_done
    (s : State V)
    (profile : AgentProfile V)
    (task : V.ResourceId)
    (notAssigned : ¬ AssignedTo s profile.principal task) :
    ¬ OperationallyAllowed s profile (AgentAction.markAssignedDone task) := by
  intro allowed
  exact notAssigned allowed.2.2.1

theorem unassigned_agent_cannot_append_human_task_note
    (s : State V)
    (profile : AgentProfile V)
    (task : V.ResourceId)
    (note : NoteEntry V)
    (notAssigned : ¬ AssignedTo s profile.principal task) :
    ¬ OperationallyAllowed s profile (AgentAction.appendAssignedNote task note) := by
  intro allowed
  exact notAssigned allowed.2.2.1

theorem unassigned_agent_cannot_add_human_task_attachment
    (s : State V)
    (profile : AgentProfile V)
    (task : V.ResourceId)
    (attachment : AttachmentRef V)
    (notAssigned : ¬ AssignedTo s profile.principal task) :
    ¬ OperationallyAllowed s profile (AgentAction.addAssignedAttachment task attachment) := by
  intro allowed
  exact notAssigned allowed.2.2.1

/-- Link causale già persistito nello stato semantic corrente. -/
def CausalLinkPresentAt
    (s : SemanticState V X)
    (runId : X.RunId)
    (goal : X.GoalId)
    (predecessor successor : CollaborativeCausalNode V X) : Prop :=
  ∃ link,
    link ∈ s.causalLinks ∧
    link.run = runId ∧
    link.goal = goal ∧
    link.predecessor = predecessor ∧
    link.successor = successor

/-- Record esplicito user/admin → agent per una NUOVA task derivata. -/
structure HumanAgentTaskDelegation
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  run : X.RunId
  sourceTask : V.ResourceId
  delegatedTask : V.ResourceId
  delegatedWork : X.WorkItemId
  delegator : V.PrincipalId
  agent : V.PrincipalId
  observedAt : Nat

/--
Validità locale della delegazione umana: la source task resta assegnata
all'umano, la nuova task è distinta ed è assegnata all'agente. La relazione
`sourceTask` conserva provenance e stessa appartenenza di progetto.
-/
def HumanAgentTaskDelegationValid
    (authorization : ProductAuthorizationProjection V)
    (certified : CertifiedCollaborativeRun V X)
    (contract : GoalContract V X)
    (delegation : HumanAgentTaskDelegation V X)
    (tick : Nat) : Prop :=
  let s := certified.run.semanticState tick
  delegation.observedAt ≤ tick ∧
  (HasKind s.base delegation.delegator PrincipalKind.user ∨
   HasKind s.base delegation.delegator PrincipalKind.administrator) ∧
  HasKind s.base delegation.agent PrincipalKind.agent ∧
  AssignedTo s.base delegation.delegator delegation.sourceTask ∧
  OpenTask s.base delegation.sourceTask ∧
  ¬ AssignedTo s.base delegation.agent delegation.sourceTask ∧
  AssignedTo s.base delegation.agent delegation.delegatedTask ∧
  OpenTask s.base delegation.delegatedTask ∧
  delegation.sourceTask ≠ delegation.delegatedTask ∧
  ResourceWithinScope s.base contract.goal.scope delegation.sourceTask ∧
  ResourceWithinScope s.base contract.goal.scope delegation.delegatedTask ∧
  ResourceOperationAllowed
    authorization s.base delegation.delegator delegation.sourceTask
    ResourceOperation.delegateAssignedWork ∧
  CausalLinkPresentAt s delegation.run contract.goal.id
    (CollaborativeCausalNode.task delegation.sourceTask)
    (CollaborativeCausalNode.task delegation.delegatedTask) ∧
  CausalLinkPresentAt s delegation.run contract.goal.id
    (CollaborativeCausalNode.task delegation.delegatedTask)
    (CollaborativeCausalNode.work delegation.delegatedWork) ∧
  (∃ sourceMeta delegatedMeta,
      s.base.resources delegation.sourceTask = some sourceMeta ∧
      s.base.resources delegation.delegatedTask = some delegatedMeta ∧
      sourceMeta.kind = ResourceKind.task ∧
      delegatedMeta.kind = ResourceKind.task ∧
      sourceMeta.projectId = delegatedMeta.projectId ∧
      delegatedMeta.creator = delegation.delegator ∧
      delegatedMeta.sourceTask = some delegation.sourceTask ∧
      ∃ sourceList delegatedList,
        sourceMeta.parent = some sourceList ∧
        delegatedMeta.parent = some delegatedList ∧
        IsResourceKind s.base sourceList ResourceKind.taskList ∧
        IsResourceKind s.base delegatedList ResourceKind.taskList ∧
        (delegatedList = sourceList ∨
         ResourceOperationAllowed
           authorization s.base delegation.delegator delegatedList
           ResourceOperation.write)) ∧
  (∃ work,
      s.workItems delegation.delegatedWork = some work ∧
      work.run = delegation.run ∧
      work.goal = contract.goal.id ∧
      work.owner = delegation.agent ∧
      ∃ spec,
        spec ∈ contract.obligations ∧
        spec.id = work.serves ∧
        ContractRequiredAt s spec)

/-- Una delegazione valida non assegna implicitamente la source task all'agente. -/
theorem delegated_agent_task_does_not_transfer_source_assignment
    (authorization : ProductAuthorizationProjection V)
    (certified : CertifiedCollaborativeRun V X)
    (contract : GoalContract V X)
    (delegation : HumanAgentTaskDelegation V X)
    (tick : Nat)
    (valid : HumanAgentTaskDelegationValid
      authorization certified contract delegation tick) :
    ¬ AssignedTo
      (certified.run.semanticState tick).base
      delegation.agent
      delegation.sourceTask := by
  unfold HumanAgentTaskDelegationValid at valid
  exact valid.2.2.2.2.2.1

/--
Provenienza dell'autorità di un WorkItem.
Non esiste intenzionalmente un costruttore `comment`: un commento può essere
source informativa/evidence, ma non può creare authority.
-/
inductive WorkAuthorityOrigin
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  | runSponsor (principal : V.PrincipalId)
  | humanDelegation (delegation : HumanAgentTaskDelegation V X)
  | inheritedWork (parent : X.WorkItemId)

/-- Side effect resource-sensitive prodotto dal runtime agentico/tooling. -/
structure ResourceSecurityEffect (V : Vocabulary) where
  resource : V.ResourceId
  operation : ResourceOperation

/--
Footprint minimo deterministico delle AgentAction R4. Gli effetti tool-specific
sono aggiunti separatamente da `ToolSecuritySemantics.requiredEffects`.
-/
def AgentActionCoreSecurityFootprint
    (action : AgentAction V) : List (ResourceSecurityEffect V) :=
  match action with
  | AgentAction.createTask draft =>
      [{ resource := draft.parentList, operation := ResourceOperation.write }]
  | AgentAction.replaceOwnTask task _ =>
      [{ resource := task, operation := ResourceOperation.write }]
  | AgentAction.deleteOwnTask task =>
      [{ resource := task, operation := ResourceOperation.manage }]
  | AgentAction.assignOwnTask task _ =>
      [{ resource := task, operation := ResourceOperation.manage }]
  | AgentAction.unassignOwnTask task _ =>
      [{ resource := task, operation := ResourceOperation.manage }]
  | AgentAction.markAssignedDone task =>
      [{ resource := task, operation := ResourceOperation.completeAssignedTask }]
  | AgentAction.appendAssignedNote task _ =>
      [{ resource := task, operation := ResourceOperation.write }]
  | AgentAction.addAssignedAttachment task _ =>
      [{ resource := task, operation := ResourceOperation.write }]
  | AgentAction.postComment draft =>
      [{ resource := draft.target, operation := ResourceOperation.postComment }]
  | AgentAction.invokeTool _ _ _ => []
  | AgentAction.retryTool _ => []
  | AgentAction.noOp => []

/--
Overlay contrattuale immutabile per le operazioni product-specific non presenti
nel linguaggio R4 `AgentAction`, incluso `EditInfo`. È indicizzato dalla stessa
WorkSpec e deve essere compilato/validato insieme al ProgramSnapshot.
-/
structure ContractWorkSecurityPolicy (V : Vocabulary) where
  workSpecId : Nat
  allowedOperations : List ResourceOperation
  allowedTools : List V.Tool

/-- Le allowedActions della WorkSpec diventano un vincolo operativo effettivo. -/
def ContractWorkSpecAllowsAgentAction
    (workSpec : ContractWorkSpec V X)
    (action : AgentAction V) : Prop :=
  match action with
  | AgentAction.noOp => True
  | _ =>
      ∃ actionClass,
        actionClass ∈ workSpec.allowedActions ∧
        ActionHasClass action actionClass

/--
Enablement semantico di un work indipendente dal solo status di queue. Serve a
vincolare gli effetti claimed/selected alle condizioni già autorizzate dal
GoalContract.
-/
def ContractWorkSemanticallyEnabledForEffect
    (s : SemanticState V X)
    (contract : GoalContract V X)
    (work : WorkItem V X) : Prop :=
  ContractDependencyClosed s contract work.serves ∧
  ∃ workSpec,
    workSpec ∈ contract.workSpecs ∧
    workSpec.obligation = work.serves ∧
    workSpec.owner = work.owner ∧
    workSpec.kind = work.kind ∧
    ContractConditionHolds s workSpec.activation ∧
    work.attempt < workSpec.maxAttempts

/-- Invocation tool tipata usata dal security projection. -/
structure ToolSecurityInvocation (V : Vocabulary) where
  callId : V.ToolCallId
  tool : V.Tool
  input : V.ToolInput

/--
Semantica di sicurezza di un tool: il refinement concreto deve fornire un
footprint conservativo completo per ogni input. Questo chiude il bypass in cui
`toolPermission` autorizzerebbe il tool ma non i resource side effect dell'input.
-/
structure ToolSecuritySemantics (V : Vocabulary) where
  requiredEffects :
    State V → V.Tool → V.ToolInput → List (ResourceSecurityEffect V)
  /-- Audience massima consentita per l'output prodotto da quell'input. -/
  outputReadableBy :
    State V → V.Tool → V.ToolInput → V.PrincipalId → Prop

/-- Sorgenti informative disponibili a una singola invocation agentica. -/
inductive InformationSource
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  | resourceBody (resource : V.ResourceId)
  | commentOn (target : V.ResourceId)
  | infoDocument (container : V.ResourceId)
  | infoFile (container : V.ResourceId)
  | toolOutput (callId : V.ToolCallId)

/--
Sink di disclosure. I commenti sono sink condivisi del target; `contextualChat`
modella esclusivamente la chat privata e supervisionata da un utente. Non è un
canale disponibile all'agente autonomo su topic/task-list/task condivisi.
-/
inductive DisclosureSink
    (V : Vocabulary) where
  /-- Update di un body esistente: l'authority side-effect è sulla stessa risorsa. -/
  | resourceBody (resource : V.ResourceId)
  /-- Creazione di una nuova risorsa: audience sul child, authority di create/write sul parent. -/
  | newResourceBody (parent resource : V.ResourceId)
  | commentOn (target : V.ResourceId)
  | infoDocument (container : V.ResourceId)
  | infoFile (container : V.ResourceId)
  | contextualChat (supervisor : V.PrincipalId)

/-- Effetto osservabile associato a un work; `contextSources` è il contesto plaintext/resource-derived esposto alla generazione dell'output. -/
inductive AgentInteractionMode (V : Vocabulary) where
  /-- Modalità privata, sincrona/supervisionata: info e azioni sono user-scoped. -/
  | contextualChat (supervisor : V.PrincipalId)
  /-- Modalità agentica autonoma: il payload persistito segue la policy private/shared. -/
  | autonomousResource

structure AgentSecurityEffect
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  run : X.RunId
  actor : V.PrincipalId
  work : X.WorkItemId
  mode : AgentInteractionMode V
  footprint : List (ResourceSecurityEffect V)
  toolInvocations : List (ToolSecurityInvocation V)
  contextSources : List (InformationSource V X)
  disclosure : Option (DisclosureSink V)

/--
Overlay di sicurezza sul CertifiedCollaborativeRun. Authority e provenance sono
immutabili per WorkItemId; i permessi correnti vengono rivalutati al tick.
-/
structure SecuredCollaborativeRun
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  certified : CertifiedCollaborativeRun V X
  runSponsor : X.RunId → V.PrincipalId
  /-- Ceiling immutabile catturato all'inizio della run. -/
  runAuthority : X.RunId → AuthorityEnvelope V
  runToolAuthority : X.RunId → ToolAuthorityEnvelope V
  workAuthorityPrincipal : X.WorkItemId → V.PrincipalId
  workAuthority : X.WorkItemId → AuthorityEnvelope V
  workToolAuthority : X.WorkItemId → ToolAuthorityEnvelope V
  workAuthorityOrigin : X.WorkItemId → WorkAuthorityOrigin V X
  /-- Policy security-side immutabile per WorkSpecId. -/
  workSecurityPolicy : Nat → ContractWorkSecurityPolicy V
  humanDelegations : List (HumanAgentTaskDelegation V X)
  securityEffectAt : Nat → Option (AgentSecurityEffect V X)
  /--
  La disclosure prodotta al tick sorgente è ancora osservabile nello stesso sink
  al tick successivo indicato. Il refinement concreto la lega a versioni/record
  persistenti, così una futura espansione dell'audience non aggira il label di
  confidenzialità stabilito al momento della scrittura.
  -/
  disclosureObservableAt : Nat → DisclosureSink V → Nat → Prop
  /-- Sorgenti plaintext/resource-derived realmente esposte al modello. -/
  modelSourceExposedAt :
    Nat → X.WorkItemId → InformationSource V X → Prop
  toolSecurity : ToolSecuritySemantics V
  /-- Audience consentita per output di tool che non derivano direttamente da una ResourceId. -/
  toolOutputReadableBy : Nat → V.ToolCallId → V.PrincipalId → Prop
  /--
  Proiezione del body persistito osservato dal principal. Per topic/task-list/task
  condivisi il kernel richiede un'unica informazione canonica: i reader non
  ricevono varianti generate in funzione dei propri permessi su altre risorse.
  -/
  observedResourceBodyAt :
    Nat → V.PrincipalId → V.ResourceId → Option V.EncryptedPayload

/--
Boundary semantica stretta per l'overlay product-specific della WorkSpec.
`PromptContractAdequacy` continua a coprire il GoalContract; questa relazione
copre soltanto il fatto che operazioni aggiuntive come `EditInfo` e i tool
ammessi dalla security policy rappresentino davvero l'intento del prompt.
-/
structure PromptSecurityPolicySemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  adequate :
    V.SystemPrompt →
    GoalContract V X →
    (Nat → ContractWorkSecurityPolicy V) →
    Prop

def PromptSecurityPolicyAdequacy
    (meaning : PromptSecurityPolicySemantics V X)
    (prompt : V.SystemPrompt)
    (contract : GoalContract V X)
    (secured : SecuredCollaborativeRun V X) : Prop :=
  meaning.adequate prompt contract secured.workSecurityPolicy

/-- Audience effettiva del sink al tick. -/
def DisclosureSinkReadableBy
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (sink : DisclosureSink V)
    (principal : V.PrincipalId) : Prop :=
  match sink with
  | DisclosureSink.resourceBody resource =>
      authorization.bodyReadable s principal resource
  | DisclosureSink.newResourceBody _ resource =>
      authorization.bodyReadable s principal resource
  | DisclosureSink.commentOn target =>
      authorization.commentReadable s principal target
  | DisclosureSink.infoDocument container =>
      InfoReadAllowed authorization s principal container
  | DisclosureSink.infoFile container =>
      InfoReadAllowed authorization s principal container
  | DisclosureSink.contextualChat recipient =>
      principal = recipient

/-- Principal autorizzato a osservare una sorgente informativa. -/
def InformationSourceReadableBy
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (s : State V)
    (source : InformationSource V X)
    (principal : V.PrincipalId) : Prop :=
  match source with
  | InformationSource.resourceBody resource =>
      authorization.bodyReadable s principal resource
  | InformationSource.commentOn target =>
      authorization.commentReadable s principal target
  | InformationSource.infoDocument container =>
      InfoReadAllowed authorization s principal container
  | InformationSource.infoFile container =>
      InfoReadAllowed authorization s principal container
  | InformationSource.toolOutput callId =>
      secured.toolOutputReadableBy tick callId principal

/- Operazione resource-sensitive necessaria per materializzare un sink. -/
/-- Risorsa il cui dominio di visibilità governa un sink persistito/condiviso. -/
def DisclosureSinkSecurityResource
    (sink : DisclosureSink V) : Option V.ResourceId :=
  match sink with
  | DisclosureSink.resourceBody resource => some resource
  | DisclosureSink.newResourceBody _ resource => some resource
  | DisclosureSink.commentOn target => some target
  | DisclosureSink.infoDocument container => some container
  | DisclosureSink.infoFile container => some container
  | DisclosureSink.contextualChat _ => none

/-- Un principal appartiene al trust circle privato del controller dell'agente. -/
def PrivateAgentTrustCircleMember
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (_actor controller principal : V.PrincipalId)
    (resource : V.ResourceId) : Prop :=
  principal = controller ∨
  (∃ resourceMeta,
      s.resources resource = some resourceMeta ∧
      authorization.projectAdministrator s principal resourceMeta.projectId) ∨
  (∃ owner,
      HasKind s principal PrincipalKind.agent ∧
      authorization.agentController s principal = some owner ∧
      (owner = controller ∨
       ∃ resourceMeta,
         s.resources resource = some resourceMeta ∧
         authorization.projectAdministrator s owner resourceMeta.projectId))

/--
Un autonomous resource è privato per l'agente se ogni reader effettivo del body
appartiene al trust circle del controller: controller, project administrator o
agent controllato dal controller/administrator.
-/
def AutonomousResourcePrivateForActor
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (actor : V.PrincipalId)
    (resource : V.ResourceId) : Prop :=
  ∃ controller,
    authorization.agentController s actor = some controller ∧
    ∀ principal,
      authorization.bodyReadable s principal resource →
      PrivateAgentTrustCircleMember
        authorization s actor controller principal resource

/--
Un autonomous resource è condiviso quando almeno un reader effettivo del body
è esterno al trust circle privato del controller dell'agente.
-/
def AutonomousResourceSharedForActor
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (actor : V.PrincipalId)
    (resource : V.ResourceId) : Prop :=
  ∃ controller principal,
    authorization.agentController s actor = some controller ∧
    authorization.bodyReadable s principal resource ∧
    ¬ PrivateAgentTrustCircleMember
        authorization s actor controller principal resource

/- Tutte le sorgenti del context sono leggibili da uno specifico principal. -/
/--
Classificazione PRIVATA del singolo sink persistito: tutta la sua audience
reale appartiene al trust circle del controller dell'agente. È sink-specific,
quindi copre correttamente anche commenti e Info quando la loro audience differisce
formalmente dalla sola `bodyReadable`.
-/
def AutonomousSinkPrivateForActor
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (actor : V.PrincipalId)
    (sink : DisclosureSink V) : Prop :=
  ∃ resource controller,
    DisclosureSinkSecurityResource sink = some resource ∧
    authorization.agentController s actor = some controller ∧
    ∀ principal,
      DisclosureSinkReadableBy authorization s sink principal →
      PrivateAgentTrustCircleMember
        authorization s actor controller principal resource

/--
Classificazione SHARED del singolo sink persistito: almeno un osservatore reale
è esterno al trust circle privato del controller dell'agente.
-/
def AutonomousSinkSharedForActor
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (actor : V.PrincipalId)
    (sink : DisclosureSink V) : Prop :=
  ∃ resource controller principal,
    DisclosureSinkSecurityResource sink = some resource ∧
    authorization.agentController s actor = some controller ∧
    DisclosureSinkReadableBy authorization s sink principal ∧
    ¬ PrivateAgentTrustCircleMember
        authorization s actor controller principal resource

def ContextReadableByPrincipal
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (sources : List (InformationSource V X))
    (principal : V.PrincipalId) : Prop :=
  let s := (secured.certified.run.semanticState tick).base
  ∀ source,
    source ∈ sources →
    InformationSourceReadableBy authorization secured tick s source principal

/--
La chat è supervisionata: ogni side effect e tool deve essere consentito
all'utente supervisor, oltre ai normali gate del work/actor.
-/
def ContextualChatActionSafe
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (effect : AgentSecurityEffect V X) : Prop :=
  let s := (secured.certified.run.semanticState tick).base
  match effect.mode with
  | AgentInteractionMode.contextualChat supervisor =>
      secured.workAuthorityPrincipal effect.work = supervisor ∧
      (∀ resourceEffect,
        resourceEffect ∈ effect.footprint →
        ResourceOperationAllowed authorization s supervisor
          resourceEffect.resource resourceEffect.operation) ∧
      (∀ invocation,
        invocation ∈ effect.toolInvocations →
        authorization.toolAllowed s supervisor invocation.tool)
  | AgentInteractionMode.autonomousResource => True

theorem autonomous_resource_private_shared_disjoint
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (actor : V.PrincipalId)
    (resource : V.ResourceId) :
    ¬ (AutonomousResourcePrivateForActor authorization s actor resource ∧
       AutonomousResourceSharedForActor authorization s actor resource) := by
  intro both
  rcases both.1 with ⟨privateController, privateControllerAt, allReadersTrusted⟩
  rcases both.2 with
    ⟨sharedController, externalReader, sharedControllerAt,
      externalReads, externalNotTrusted⟩
  have controllerEq : privateController = sharedController := by
    have someEq : some privateController = some sharedController :=
      privateControllerAt.symm.trans sharedControllerAt
    exact Option.some.inj someEq
  apply externalNotTrusted
  have trusted := allReadersTrusted externalReader externalReads
  simpa [controllerEq] using trusted

theorem autonomous_sink_private_shared_disjoint
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (actor : V.PrincipalId)
    (sink : DisclosureSink V) :
    ¬ (AutonomousSinkPrivateForActor authorization s actor sink ∧
       AutonomousSinkSharedForActor authorization s actor sink) := by
  intro both
  rcases both.1 with
    ⟨privateResource, privateController, privateSinkResource,
      privateControllerAt, allReadersTrusted⟩
  rcases both.2 with
    ⟨sharedResource, sharedController, externalReader, sharedSinkResource,
      sharedControllerAt, externalReads, externalNotTrusted⟩
  have resourceEq : privateResource = sharedResource := by
    rw [privateSinkResource] at sharedSinkResource
    exact Option.some.inj sharedSinkResource
  have controllerEq : privateController = sharedController := by
    have someEq : some privateController = some sharedController :=
      privateControllerAt.symm.trans sharedControllerAt
    exact Option.some.inj someEq
  apply externalNotTrusted
  have trusted := allReadersTrusted externalReader externalReads
  simpa [resourceEq, controllerEq] using trusted

def DisclosureSinkRequiredEffect
    (sink : DisclosureSink V) : Option (ResourceSecurityEffect V) :=
  match sink with
  | DisclosureSink.resourceBody resource =>
      some { resource := resource, operation := ResourceOperation.write }
  | DisclosureSink.newResourceBody parent _ =>
      some { resource := parent, operation := ResourceOperation.write }
  | DisclosureSink.commentOn target =>
      some { resource := target, operation := ResourceOperation.postComment }
  | DisclosureSink.infoDocument container =>
      some { resource := container, operation := ResourceOperation.editInfo }
  | DisclosureSink.infoFile container =>
      some { resource := container, operation := ResourceOperation.editInfo }
  | DisclosureSink.contextualChat _ => none

/--
Intersezione per un sink CONDIVISO: ogni principal che può osservare il sink
deve poter osservare ogni sorgente resource-derived messa nel context del
modello. Non produce varianti per-reader: il payload del sink rimane unico.
-/
def ContextSafeForDisclosure
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (sources : List (InformationSource V X))
    (sink : DisclosureSink V) : Prop :=
  let s := (secured.certified.run.semanticState tick).base
  ∀ source,
    source ∈ sources →
    ∀ principal,
      DisclosureSinkReadableBy authorization s sink principal →
      InformationSourceReadableBy authorization secured tick s source principal

/--
Policy informativa mode-aware.

* contextualChat: context e disclosure sono ristrette al supervisor;
* autonomous private: il context è ristretto ai permessi del controller;
* autonomous shared: il payload è unico e usa l'intersezione dell'audience
  effettiva del sink (`ContextSafeForDisclosure`).
-/
def ModeAwareContextSafeForDisclosure
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (effect : AgentSecurityEffect V X)
    (sink : DisclosureSink V) : Prop :=
  let s := (secured.certified.run.semanticState tick).base
  match effect.mode with
  | AgentInteractionMode.contextualChat supervisor =>
      sink = DisclosureSink.contextualChat supervisor ∧
      ContextReadableByPrincipal
        authorization secured tick effect.contextSources supervisor
  | AgentInteractionMode.autonomousResource =>
      ((AutonomousSinkPrivateForActor authorization s effect.actor sink ∧
        ∃ controller,
          authorization.agentController s effect.actor = some controller ∧
          ContextReadableByPrincipal
            authorization secured tick effect.contextSources controller) ∨
       (AutonomousSinkSharedForActor authorization s effect.actor sink ∧
        ContextSafeForDisclosure
          authorization secured tick effect.contextSources sink))

/--
La chat contestuale non viene abbassata al principal meno privilegiato presente
altrove: supervisor, context informativo e authority delle azioni coincidono con
l'utente che supervisiona quella chat.
-/
theorem contextual_chat_context_safe
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (recipient : V.PrincipalId)
    (sources : List (InformationSource V X))
    (recipientCanRead :
      ∀ source,
        source ∈ sources →
        InformationSourceReadableBy
          authorization secured tick
          (secured.certified.run.semanticState tick).base
          source recipient) :
    ContextSafeForDisclosure
      authorization secured tick sources
      (DisclosureSink.contextualChat recipient) := by
  intro source sourceIn principal sinkReadable
  change principal = recipient at sinkReadable
  cases sinkReadable
  exact recipientCanRead source sourceIn

/--
Requisito di read dell'esecuzione per una source resource-derived.
I tool output sono autorizzati separatamente dalla causalità/tool policy.
-/
def InformationSourceReadRequirement
    (source : InformationSource V X) :
    Option (V.ResourceId × ResourceOperation) :=
  match source with
  | InformationSource.resourceBody resource =>
      some (resource, ResourceOperation.read)
  | InformationSource.commentOn target =>
      some (target, ResourceOperation.readComment)
  | InformationSource.infoDocument container =>
      some (container, ResourceOperation.read)
  | InformationSource.infoFile container =>
      some (container, ResourceOperation.read)
  | InformationSource.toolOutput _ => none

/-- Parent runtime di un WorkItem nel medesimo stato. -/
def WorkParentAt
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (parent child : X.WorkItemId) : Prop :=
  ∃ childWork,
    (secured.certified.run.semanticState tick).workItems child = some childWork ∧
    childWork.parent = some parent

/-- Chiusura riflessivo-transitiva della delegazione/continuation causale fra work. -/
inductive WorkDescendsFromAt
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat) : X.WorkItemId → X.WorkItemId → Prop where
  | refl (work : X.WorkItemId) :
      WorkDescendsFromAt secured tick work work
  | step
      {root parent child : X.WorkItemId} :
      WorkDescendsFromAt secured tick root parent →
      WorkParentAt secured tick parent child →
      WorkDescendsFromAt secured tick root child

/--
Certificato locale di safety. Non contiene come campo i theorem globali di
non-amplification/noninterference: contiene invarianti locali da cui essi
seguono.
-/
structure AuthorityInformationKernelCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat) : Prop where

  sponsorIsHuman :
    ∀ n,
      start ≤ n →
      HasKind
        (secured.certified.run.semanticState n).base
        (secured.runSponsor runId)
        PrincipalKind.user ∨
      HasKind
        (secured.certified.run.semanticState n).base
        (secured.runSponsor runId)
        PrincipalKind.administrator

  /--
  Il ceiling della run è autorizzato dal principal sponsor al tick iniziale.
  Nuovi grant successivi non ampliano automaticamente la run.
  -/
  runAuthorityBoundedAtStart :
    ∀ resource operation,
      secured.runAuthority runId resource operation →
      ResourceOperationAllowed
        authorization
        (secured.certified.run.semanticState start).base
        (secured.runSponsor runId) resource operation

  runToolAuthorityBoundedAtStart :
    ∀ tool,
      secured.runToolAuthority runId tool →
      authorization.toolAllowed
        (secured.certified.run.semanticState start).base
        (secured.runSponsor runId) tool

  /-- Ogni work rilevante possiede una provenance di authority non-comment. -/
  workOriginSound :
    ∀ n workId work,
      start ≤ n →
      (secured.certified.run.semanticState n).workItems workId = some work →
      work.run = runId →
      work.goal = contract.goal.id →
      match secured.workAuthorityOrigin workId with
      | WorkAuthorityOrigin.runSponsor principal =>
          principal = secured.runSponsor runId ∧
          secured.workAuthorityPrincipal workId = principal ∧
          AuthoritySubset
            (secured.workAuthority workId)
            (secured.runAuthority runId) ∧
          ToolAuthoritySubset
            (secured.workToolAuthority workId)
            (secured.runToolAuthority runId)
      | WorkAuthorityOrigin.humanDelegation delegation =>
          delegation ∈ secured.humanDelegations ∧
          delegation.delegatedWork = workId ∧
          HumanAgentTaskDelegationValid
            authorization secured.certified contract delegation n ∧
          secured.workAuthorityPrincipal workId = delegation.delegator ∧
          AuthoritySubset
            (secured.workAuthority workId)
            (secured.runAuthority runId) ∧
          ToolAuthoritySubset
            (secured.workToolAuthority workId)
            (secured.runToolAuthority runId)
      | WorkAuthorityOrigin.inheritedWork parent =>
          WorkParentAt secured n parent workId ∧
          secured.workAuthorityPrincipal workId =
            secured.workAuthorityPrincipal parent ∧
          AuthoritySubset
            (secured.workAuthority workId)
            (secured.workAuthority parent) ∧
          ToolAuthoritySubset
            (secured.workToolAuthority workId)
            (secured.workToolAuthority parent)

  /-- Una delegazione umana attenua il ceiling ai permessi del delegator. -/
  humanDelegationAuthorityBound :
    ∀ n delegation work,
      start ≤ n →
      delegation ∈ secured.humanDelegations →
      HumanAgentTaskDelegationValid
        authorization secured.certified contract delegation n →
      (secured.certified.run.semanticState n).workItems
        delegation.delegatedWork = some work →
      ∀ resource operation,
        secured.workAuthority delegation.delegatedWork resource operation →
        ResourceOperationAllowed
          authorization
          (secured.certified.run.semanticState delegation.observedAt).base
          delegation.delegator resource operation

  /-- Anche il ceiling tool delegato dall'umano è attenuato. -/
  humanDelegationToolAuthorityBound :
    ∀ n delegation work,
      start ≤ n →
      delegation ∈ secured.humanDelegations →
      HumanAgentTaskDelegationValid
        authorization secured.certified contract delegation n →
      (secured.certified.run.semanticState n).workItems
        delegation.delegatedWork = some work →
      ∀ tool,
        secured.workToolAuthority delegation.delegatedWork tool →
        authorization.toolAllowed
          (secured.certified.run.semanticState delegation.observedAt).base
          delegation.delegator tool

  /-- Ogni edge parent→child preserva o restringe l'authority envelope. -/
  childWorkAuthorityAttenuates :
    ∀ n parent child,
      start ≤ n →
      WorkParentAt secured n parent child →
      AuthoritySubset
        (secured.workAuthority child)
        (secured.workAuthority parent)

  /-- Ogni edge parent→child preserva o restringe anche il ceiling dei tool. -/
  childWorkToolAuthorityAttenuates :
    ∀ n parent child,
      start ≤ n →
      WorkParentAt secured n parent child →
      ToolAuthoritySubset
        (secured.workToolAuthority child)
        (secured.workToolAuthority parent)

  /--
  Ogni AgentMove R4 del segmento ha un corrispondente SecurityEffect nello
  stesso tick: il safety projection non può omettere selettivamente una mossa.
  -/
  agentMoveHasSecurityEffect :
    ∀ n actor action,
      start ≤ n →
      secured.certified.run.baseRun.move n =
        some (Move.agentMove actor action) →
      ∃ effect,
        secured.securityEffectAt n = some effect ∧
        effect.actor = actor

  /-- Il footprint security contiene almeno il footprint deterministico R4. -/
  coreAgentActionFootprintComplete :
    ∀ n effect action requiredEffect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      secured.certified.run.baseRun.move n =
        some (Move.agentMove effect.actor action) →
      requiredEffect ∈ AgentActionCoreSecurityFootprint action →
      requiredEffect ∈ effect.footprint

  /--
  Ogni AgentAction R4 è ammessa dalle `allowedActions` della WorkSpec certificata.
  Il commento può fornire informazione/attivazione, ma non cambiare questa lista.
  -/
  coreAgentActionAllowedByContract :
    ∀ n effect action certificate workSpec,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      secured.certified.run.baseRun.move n =
        some (Move.agentMove effect.actor action) →
      secured.certified.workCertificateAt n effect.work = some certificate →
      workSpec ∈ contract.workSpecs →
      certificate.workSpecId = workSpec.id →
      ContractWorkSpecAllowsAgentAction workSpec action

  /-- Ogni WorkSpec del contratto ha una policy security-side con stesso ID. -/
  securityPolicyBoundToContract :
    ∀ workSpec,
      workSpec ∈ contract.workSpecs →
      (secured.workSecurityPolicy workSpec.id).workSpecId = workSpec.id

  /--
  Ogni effetto appartiene a un WorkItem certificato da una WorkSpec del
  GoalContract. Un commento può attivare/evidenziare lavoro già previsto, ma
  non può inventare un'azione fuori contratto.
  -/
  effectWorkCertified :
    ∀ n effect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      ∃ work certificate workSpec,
        (secured.certified.run.semanticState n).workItems effect.work = some work ∧
        secured.certified.workCertificateAt n effect.work = some certificate ∧
        workSpec ∈ contract.workSpecs ∧
        certificate.work = effect.work ∧
        certificate.workSpecId = workSpec.id ∧
        workSpec.obligation = work.serves ∧
        workSpec.owner = work.owner ∧
        workSpec.kind = work.kind

  /--
  Ogni effetto appartiene a un work semanticamente già abilitato dal contratto.
  Un commento può soddisfare una condition prevista dal contratto, ma non crearne
  una nuova fuori dal linguaggio strutturato.
  -/
  effectWorkSemanticallyEnabled :
    ∀ n effect work,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      (secured.certified.run.semanticState n).workItems effect.work = some work →
      ContractWorkSemanticallyEnabledForEffect
        (secured.certified.run.semanticState n) contract work

  /--
  Le operazioni e i tool realmente usati sono autorizzati dalla policy della
  WorkSpec certificata; `EditInfo` non può quindi comparire come side effect
  implicito di un work che non lo dichiara.
  -/
  effectSecurityPolicyAllowed :
    ∀ n effect certificate,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      secured.certified.workCertificateAt n effect.work = some certificate →
      (∀ resourceEffect,
        resourceEffect ∈ effect.footprint →
        resourceEffect.operation ∈
          (secured.workSecurityPolicy certificate.workSpecId).allowedOperations) ∧
      (∀ invocation,
        invocation ∈ effect.toolInvocations →
        invocation.tool ∈
          (secured.workSecurityPolicy certificate.workSpecId).allowedTools)

  /-- Ogni effetto è attribuito a un work della stessa run e al suo owner. -/
  effectWorkOwned :
    ∀ n effect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      effect.run = runId ∧
      ∃ work,
        (secured.certified.run.semanticState n).workItems effect.work = some work ∧
        work.run = runId ∧
        work.goal = contract.goal.id ∧
        work.owner = effect.actor

  /--
  Assignment isolation: una task assegnata a un umano e non all'agente non può
  essere mutata/gestita/completata dall'agente, neppure se l'agente ne era il
  creator R4. Vale anche per side effect prodotti tramite tool perché passano
  dallo stesso footprint.
  -/
  humanAssignedTaskControlIsolation :
    ∀ n effect resourceEffect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      resourceEffect ∈ effect.footprint →
      HumanAssignedTaskWithoutAgent
        (secured.certified.run.semanticState n).base
        effect.actor resourceEffect.resource →
      TaskControlOperation resourceEffect.operation →
      False

  /--
  Intersezione actor ∩ work ceiling ∩ authority principal corrente.
  Questa è la regola centrale anti-confused-deputy.
  -/
  effectAuthoritySafe :
    ∀ n effect resourceEffect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      resourceEffect ∈ effect.footprint →
      EffectiveAuthorityAt
        authorization
        (secured.certified.run.semanticState n).base
        (secured.workAuthority effect.work)
        (secured.workAuthorityPrincipal effect.work)
        resourceEffect.resource
        resourceEffect.operation ∧
      ResourceOperationAllowed
        authorization
        (secured.certified.run.semanticState n).base
        effect.actor
        resourceEffect.resource
        resourceEffect.operation

  /--
  Tool invocation: actor, authority principal e ceiling del work devono tutti
  autorizzare il tool. Il footprint resource-sensitive resta poi soggetto a
  `effectAuthoritySafe`.
  -/
  toolUseAuthoritySafe :
    ∀ n effect invocation,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      invocation ∈ effect.toolInvocations →
      secured.workToolAuthority effect.work invocation.tool ∧
      authorization.toolAllowed
        (secured.certified.run.semanticState n).base
        (secured.workAuthorityPrincipal effect.work) invocation.tool ∧
      authorization.toolAllowed
        (secured.certified.run.semanticState n).base
        effect.actor invocation.tool

  /-- ToolCall concreta e security invocation devono coincidere. -/
  toolInvocationMatchesCall :
    ∀ n effect invocation,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      invocation ∈ effect.toolInvocations →
      ∃ call,
        (secured.certified.run.semanticState n).base.toolCalls invocation.callId =
          some call ∧
        call.owner = effect.actor ∧
        call.tool = invocation.tool ∧
        call.input = invocation.input

  /--
  Completezza del footprint: ogni side effect dichiarato dalla semantica di
  sicurezza del tool compare nel footprint globale dell'effetto e ricade quindi
  sotto `effectAuthoritySafe`.
  -/
  toolFootprintComplete :
    ∀ n effect invocation requiredEffect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      invocation ∈ effect.toolInvocations →
      requiredEffect ∈
        secured.toolSecurity.requiredEffects
          (secured.certified.run.semanticState n).base
          invocation.tool invocation.input →
      requiredEffect ∈ effect.footprint

  /-- Tutti gli effetti rimangono nel resource scope della run. -/
  effectWithinRunScope :
    ∀ n effect resourceEffect scope,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      resourceEffect ∈ effect.footprint →
      (secured.certified.run.semanticState n).runScope runId = some scope →
      ResourceWithinScope
        (secured.certified.run.semanticState n).base
        scope
        resourceEffect.resource

  /--
  Completezza del context projection: `contextSources` coincide con tutte le
  sorgenti resource-derived realmente esposte alla invocation. Il runtime non
  può omettere una sorgente dalla provenance per aggirare il disclosure check.
  -/
  modelContextProjectionExact :
    ∀ n effect source,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      (source ∈ effect.contextSources ↔
       secured.modelSourceExposedAt n effect.work source)

  /--
  Le source resource-derived fornite al modello sono anch'esse dentro
  l'authority effettiva del work e leggibili dall'actor. Questo impedisce di
  usare la maggiore visibilità personale dell'agente come context escalation.
  -/
  modelContextAuthoritySafe :
    ∀ n effect source resource operation,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      source ∈ effect.contextSources →
      InformationSourceReadRequirement source = some (resource, operation) →
      EffectiveAuthorityAt
        authorization
        (secured.certified.run.semanticState n).base
        (secured.workAuthority effect.work)
        (secured.workAuthorityPrincipal effect.work)
        resource operation ∧
      ResourceOperationAllowed
        authorization
        (secured.certified.run.semanticState n).base
        effect.actor resource operation

  /-- Anche le source resource-derived del modello restano nel run scope. -/
  modelContextWithinRunScope :
    ∀ n effect source resource operation scope,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      source ∈ effect.contextSources →
      InformationSourceReadRequirement source = some (resource, operation) →
      (secured.certified.run.semanticState n).runScope runId = some scope →
      ResourceWithinScope
        (secured.certified.run.semanticState n).base scope resource

  /-- Le source Info devono riferirsi realmente a topic/task-list container. -/
  infoContextContainerValid :
    ∀ n effect container,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      (InformationSource.infoDocument container ∈ effect.contextSources ∨
       InformationSource.infoFile container ∈ effect.contextSources) →
      InfoContainer (secured.certified.run.semanticState n).base container

  /--
  Un tool output usato come context deve provenire da una call dell'actor e la
  sua audience dichiarata deve coincidere con la ToolSecuritySemantics.
  -/
  toolContextSourceOwned :
    ∀ n effect callId,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      InformationSource.toolOutput callId ∈ effect.contextSources →
      ∃ call,
        (secured.certified.run.semanticState n).base.toolCalls callId = some call ∧
        call.owner = effect.actor ∧
        ∀ principal,
          (secured.toolOutputReadableBy n callId principal ↔
           secured.toolSecurity.outputReadableBy
             (secured.certified.run.semanticState n).base
             call.tool call.input principal)

  /-- Ogni sink persistito/condiviso compare anche nel footprint autorizzato. -/
  disclosureFootprintSound :
    ∀ n effect sink resourceEffect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      effect.disclosure = some sink →
      DisclosureSinkRequiredEffect sink = some resourceEffect →
      resourceEffect ∈ effect.footprint

  /--
  Il body persistito è canonico/univoco: due reader effettivi della stessa
  risorsa osservano la stessa informazione persistita al tick, non una variante
  generata in base ai loro permessi su altre risorse.
  -/
  canonicalResourceBody :
    ∀ n resource first second,
      start ≤ n →
      authorization.bodyReadable
        (secured.certified.run.semanticState n).base first resource →
      authorization.bodyReadable
        (secured.certified.run.semanticState n).base second resource →
      secured.observedResourceBodyAt n first resource =
        secured.observedResourceBodyAt n second resource

  /-- La chat contestuale restringe anche le azioni ai permessi del supervisor. -/
  contextualChatActionSafe :
    ∀ n effect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      ContextualChatActionSafe authorization secured n effect

  /--
  Ogni output information-bearing segue la policy mode-aware: user-scoped nella
  chat, creator/controller-scoped nel private autonomous resource, intersezione
  dell'audience nel shared autonomous resource.
  -/
  disclosureContextSafe :
    ∀ n effect sink,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      effect.disclosure = some sink →
      ModeAwareContextSafeForDisclosure
        authorization secured n effect sink

  /--
  Persistenza del label di confidenzialità: se un output agentico rimane
  osservabile in un sink a un tick futuro, ogni lettore del sink deve poter
  leggere anche TUTTE le sorgenti di provenance a quel tick futuro. Questo
  chiude il caso in cui B è sicura oggi ma viene condivisa domani con chi non
  può leggere una sorgente A, senza bloccare chi in futuro ottiene legittimamente
  accesso sia ad A sia a B.
  -/
  persistedDisclosureContextSafe :
    ∀ n m effect sink,
      start ≤ n →
      n ≤ m →
      secured.securityEffectAt n = some effect →
      effect.disclosure = some sink →
      secured.disclosureObservableAt n sink m →
      ModeAwareContextSafeForDisclosure
        authorization secured m effect sink

/--
Theorem locale di non-amplificazione: ogni side effect osservato è permesso sia
dal principal concreto agente sia dall'autorità attenuata del work.
-/
theorem agent_effect_authority_non_amplification
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (resourceEffect : ResourceSecurityEffect V)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (inFootprint : resourceEffect ∈ effect.footprint) :
    EffectiveAuthorityAt
      authorization
      (secured.certified.run.semanticState n).base
      (secured.workAuthority effect.work)
      (secured.workAuthorityPrincipal effect.work)
      resourceEffect.resource
      resourceEffect.operation ∧
    ResourceOperationAllowed
      authorization
      (secured.certified.run.semanticState n).base
      effect.actor
      resourceEffect.resource
      resourceEffect.operation := by
  exact kernel.effectAuthoritySafe n effect resourceEffect startLe effectAt inFootprint

/--
Theorem di assignment isolation: il solo fatto che l'agente sia creator o abbia
permessi tecnici non gli consente controllo su una task assegnata a un umano
quando l'agente non è assegnatario.
-/
theorem human_task_assignment_blocks_unassigned_agent_control
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (resourceEffect : ResourceSecurityEffect V)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (inFootprint : resourceEffect ∈ effect.footprint)
    (humanAssigned :
      HumanAssignedTaskWithoutAgent
        (secured.certified.run.semanticState n).base
        effect.actor resourceEffect.resource)
    (controlOperation : TaskControlOperation resourceEffect.operation) :
    False := by
  exact kernel.humanAssignedTaskControlIsolation
    n effect resourceEffect startLe effectAt inFootprint
    humanAssigned controlOperation

/--
Trace-level assignment isolation: una AgentAction R4 che contiene un footprint
di controllo task non può comparire in una run R5.33 sicura quando la task è
assegnata a un umano e non all'agente. Questo chiude anche il caso in cui R4
considererebbe la task `own` perché creata originariamente dall'agente.
-/
theorem secure_agent_move_cannot_control_human_assigned_task
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (actor : V.PrincipalId)
    (action : AgentAction V)
    (requiredEffect : ResourceSecurityEffect V)
    (startLe : start ≤ n)
    (moveAt :
      secured.certified.run.baseRun.move n =
        some (Move.agentMove actor action))
    (requiredIn :
      requiredEffect ∈ AgentActionCoreSecurityFootprint action)
    (humanAssigned :
      HumanAssignedTaskWithoutAgent
        (secured.certified.run.semanticState n).base
        actor requiredEffect.resource)
    (controlOperation : TaskControlOperation requiredEffect.operation) :
    False := by
  obtain ⟨effect, effectAt, effectActor⟩ :=
    kernel.agentMoveHasSecurityEffect n actor action startLe moveAt
  have moveForEffect :
      secured.certified.run.baseRun.move n =
        some (Move.agentMove effect.actor action) := by
    simpa [effectActor] using moveAt
  have inFootprint :=
    kernel.coreAgentActionFootprintComplete
      n effect action requiredEffect startLe effectAt moveForEffect requiredIn
  have humanAssignedForEffect :
      HumanAssignedTaskWithoutAgent
        (secured.certified.run.semanticState n).base
        effect.actor requiredEffect.resource := by
    simpa [effectActor] using humanAssigned
  exact
    kernel.humanAssignedTaskControlIsolation
      n effect requiredEffect startLe effectAt inFootprint
      humanAssignedForEffect controlOperation

/--
Ogni side effect richiesto dalla semantica di un tool eredita gli stessi check
di authority del footprint generale; `toolAllowed` da solo non basta.
-/
theorem tool_required_effect_authorized
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (invocation : ToolSecurityInvocation V)
    (requiredEffect : ResourceSecurityEffect V)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (invocationIn : invocation ∈ effect.toolInvocations)
    (requiredIn :
      requiredEffect ∈
        secured.toolSecurity.requiredEffects
          (secured.certified.run.semanticState n).base
          invocation.tool invocation.input) :
    EffectiveAuthorityAt
      authorization
      (secured.certified.run.semanticState n).base
      (secured.workAuthority effect.work)
      (secured.workAuthorityPrincipal effect.work)
      requiredEffect.resource requiredEffect.operation ∧
    ResourceOperationAllowed
      authorization
      (secured.certified.run.semanticState n).base
      effect.actor requiredEffect.resource requiredEffect.operation := by
  have inFootprint :=
    kernel.toolFootprintComplete
      n effect invocation requiredEffect startLe effectAt invocationIn requiredIn
  exact
    kernel.effectAuthoritySafe
      n effect requiredEffect startLe effectAt inFootprint

/--
Forma utilizzabile del theorem multi-hop, parametrica su una witness chain.
La prova usa soltanto l'attenuazione locale di ogni edge.
-/
theorem collaborative_authority_attenuation
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (tick : Nat)
    (root child : X.WorkItemId)
    (startLe : start ≤ tick)
    (chain : WorkDescendsFromAt secured tick root child) :
    AuthoritySubset
      (secured.workAuthority child)
      (secured.workAuthority root) := by
  induction chain with
  | refl =>
      exact authority_subset_refl (secured.workAuthority root)
  | step path edge ih =>
      exact authority_subset_trans
        (kernel.childWorkAuthorityAttenuates tick _ _ startLe edge)
        ih

/--
Caso user→agent→agent esplicito: ogni discendente di un work nato da una
HumanAgentTaskDelegation resta entro i permessi posseduti dal delegator al tick
della delegazione. La rivalutazione corrente avviene poi in `EffectiveAuthorityAt`.
-/
theorem collaborative_human_delegation_authority_attenuation
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (tick : Nat)
    (delegation : HumanAgentTaskDelegation V X)
    (rootWork : WorkItem V X)
    (child : X.WorkItemId)
    (startLe : start ≤ tick)
    (delegationIn : delegation ∈ secured.humanDelegations)
    (delegationValid : HumanAgentTaskDelegationValid
      authorization secured.certified contract delegation tick)
    (rootAt :
      (secured.certified.run.semanticState tick).workItems
        delegation.delegatedWork = some rootWork)
    (chain :
      WorkDescendsFromAt secured tick delegation.delegatedWork child) :
    ∀ resource operation,
      secured.workAuthority child resource operation →
      ResourceOperationAllowed
        authorization
        (secured.certified.run.semanticState delegation.observedAt).base
        delegation.delegator resource operation := by
  have childWithinRoot :
      AuthoritySubset
        (secured.workAuthority child)
        (secured.workAuthority delegation.delegatedWork) :=
    collaborative_authority_attenuation
      authorization secured contract runId start kernel
      tick delegation.delegatedWork child startLe chain
  have rootBound :=
    kernel.humanDelegationAuthorityBound
      tick delegation rootWork startLe delegationIn delegationValid rootAt
  intro resource operation childAllowed
  exact rootBound resource operation
    (childWithinRoot resource operation childAllowed)

/--
Catena completa sponsor→agent→agent: un discendente di un root work sponsorizzato
resta sotto il ceiling immutabile della run.
-/
theorem collaborative_authority_bounded_by_run_ceiling
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (tick : Nat)
    (root child : X.WorkItemId)
    (rootWork : WorkItem V X)
    (startLe : start ≤ tick)
    (rootAt :
      (secured.certified.run.semanticState tick).workItems root = some rootWork)
    (rootRun : rootWork.run = runId)
    (rootGoal : rootWork.goal = contract.goal.id)
    (rootOrigin :
      secured.workAuthorityOrigin root =
        WorkAuthorityOrigin.runSponsor (secured.runSponsor runId))
    (chain : WorkDescendsFromAt secured tick root child) :
    AuthoritySubset
      (secured.workAuthority child)
      (secured.runAuthority runId) := by
  have rootSound :=
    kernel.workOriginSound tick root rootWork startLe rootAt rootRun rootGoal
  rw [rootOrigin] at rootSound
  have rootWithinRun :
      AuthoritySubset
        (secured.workAuthority root)
        (secured.runAuthority runId) :=
    rootSound.2.2.1
  exact authority_subset_trans
    (collaborative_authority_attenuation
      authorization secured contract runId start kernel
      tick root child startLe chain)
    rootWithinRun

/-- La stessa attenuazione multi-hop vale per i tool. -/
theorem collaborative_tool_authority_attenuation
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (tick : Nat)
    (root child : X.WorkItemId)
    (startLe : start ≤ tick)
    (chain : WorkDescendsFromAt secured tick root child) :
    ToolAuthoritySubset
      (secured.workToolAuthority child)
      (secured.workToolAuthority root) := by
  induction chain with
  | refl =>
      exact tool_authority_subset_refl (secured.workToolAuthority root)
  | step path edge ih =>
      exact tool_authority_subset_trans
        (kernel.childWorkToolAuthorityAttenuates tick _ _ startLe edge)
        ih

/-- Anche una catena user→agent→agent non può ampliare il ceiling dei tool. -/
theorem collaborative_human_delegation_tool_authority_attenuation
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (tick : Nat)
    (delegation : HumanAgentTaskDelegation V X)
    (rootWork : WorkItem V X)
    (child : X.WorkItemId)
    (startLe : start ≤ tick)
    (delegationIn : delegation ∈ secured.humanDelegations)
    (delegationValid : HumanAgentTaskDelegationValid
      authorization secured.certified contract delegation tick)
    (rootAt :
      (secured.certified.run.semanticState tick).workItems
        delegation.delegatedWork = some rootWork)
    (chain :
      WorkDescendsFromAt secured tick delegation.delegatedWork child) :
    ∀ tool,
      secured.workToolAuthority child tool →
      authorization.toolAllowed
        (secured.certified.run.semanticState delegation.observedAt).base
        delegation.delegator tool := by
  have childWithinRoot :
      ToolAuthoritySubset
        (secured.workToolAuthority child)
        (secured.workToolAuthority delegation.delegatedWork) :=
    collaborative_tool_authority_attenuation
      authorization secured contract runId start kernel
      tick delegation.delegatedWork child startLe chain
  have rootBound :=
    kernel.humanDelegationToolAuthorityBound
      tick delegation rootWork startLe delegationIn delegationValid rootAt
  intro tool childAllowed
  exact rootBound tool (childWithinRoot tool childAllowed)

/-- Anche il ceiling tool di ogni discendente resta sotto quello della run. -/
theorem collaborative_tool_authority_bounded_by_run_ceiling
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (tick : Nat)
    (root child : X.WorkItemId)
    (rootWork : WorkItem V X)
    (startLe : start ≤ tick)
    (rootAt :
      (secured.certified.run.semanticState tick).workItems root = some rootWork)
    (rootRun : rootWork.run = runId)
    (rootGoal : rootWork.goal = contract.goal.id)
    (rootOrigin :
      secured.workAuthorityOrigin root =
        WorkAuthorityOrigin.runSponsor (secured.runSponsor runId))
    (chain : WorkDescendsFromAt secured tick root child) :
    ToolAuthoritySubset
      (secured.workToolAuthority child)
      (secured.runToolAuthority runId) := by
  have rootSound :=
    kernel.workOriginSound tick root rootWork startLe rootAt rootRun rootGoal
  rw [rootOrigin] at rootSound
  have rootWithinRun :
      ToolAuthoritySubset
        (secured.workToolAuthority root)
        (secured.runToolAuthority runId) :=
    rootSound.2.2.2
  exact tool_authority_subset_trans
    (collaborative_tool_authority_attenuation
      authorization secured contract runId start kernel
      tick root child startLe chain)
    rootWithinRun

/--
Commenti e sourceComment possono causare attenzione/evidence/work, ma non sono
una authority origin: ogni work deve comunque avere una delle provenance
chiuse `runSponsor | humanDelegation | inheritedWork`.
-/
theorem comment_does_not_confer_work_authority
    (secured : SecuredCollaborativeRun V X)
    (workId : X.WorkItemId) :
    (∃ principal,
        secured.workAuthorityOrigin workId =
          WorkAuthorityOrigin.runSponsor principal) ∨
    (∃ delegation,
        secured.workAuthorityOrigin workId =
          WorkAuthorityOrigin.humanDelegation delegation) ∨
    (∃ parent,
        secured.workAuthorityOrigin workId =
          WorkAuthorityOrigin.inheritedWork parent) := by
  cases hOrigin : secured.workAuthorityOrigin workId with
  | runSponsor principal =>
      exact Or.inl ⟨principal, rfl⟩
  | humanDelegation delegation =>
      exact Or.inr (Or.inl ⟨delegation, rfl⟩)
  | inheritedWork parent =>
      exact Or.inr (Or.inr ⟨parent, rfl⟩)

/--
Anche quando un WorkItem porta `sourceComment`, il commento resta soltanto
provenance informativa: l'authority origin deve essere giustificata
indipendentemente dal commento.
-/
theorem comment_sourced_work_requires_independent_authority
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (workId : X.WorkItemId)
    (work : WorkItem V X)
    (commentId : V.CommentId)
    (_workAt :
      (secured.certified.run.semanticState tick).workItems workId = some work)
    (_sourceComment : work.sourceComment = some commentId) :
    (∃ principal,
        secured.workAuthorityOrigin workId =
          WorkAuthorityOrigin.runSponsor principal) ∨
    (∃ delegation,
        secured.workAuthorityOrigin workId =
          WorkAuthorityOrigin.humanDelegation delegation) ∨
    (∃ parent,
        secured.workAuthorityOrigin workId =
          WorkAuthorityOrigin.inheritedWork parent) := by
  exact comment_does_not_confer_work_authority secured workId

/--
Un commento può essere source informativa di un work, ma l'effetto risultante
resta semanticamente abilitato dal GoalContract e conserva una authority origin
indipendente dal commento. Il testo del commento non diventa quindi una nuova
istruzione normativa del runtime.
-/
theorem comment_sourced_effect_remains_contract_enabled
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (work : WorkItem V X)
    (commentId : V.CommentId)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (workAt :
      (secured.certified.run.semanticState n).workItems effect.work = some work)
    (sourceComment : work.sourceComment = some commentId) :
    ContractWorkSemanticallyEnabledForEffect
        (secured.certified.run.semanticState n) contract work ∧
      ((∃ principal,
          secured.workAuthorityOrigin effect.work =
            WorkAuthorityOrigin.runSponsor principal) ∨
       (∃ delegation,
          secured.workAuthorityOrigin effect.work =
            WorkAuthorityOrigin.humanDelegation delegation) ∨
       (∃ parent,
          secured.workAuthorityOrigin effect.work =
            WorkAuthorityOrigin.inheritedWork parent)) := by
  constructor
  · exact
      kernel.effectWorkSemanticallyEnabled
        n effect work startLe effectAt workAt
  · exact
      comment_sourced_work_requires_independent_authority
        secured n effect.work work commentId workAt sourceComment

/--
Information confinement locale: se un output è osservabile da P, allora P può
osservare ogni source resa disponibile alla invocation che ha prodotto
l'output.
-/
theorem agent_shared_disclosure_respects_sink_audience
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (sink : DisclosureSink V)
    (source : InformationSource V X)
    (principal : V.PrincipalId)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (sinkAt : effect.disclosure = some sink)
    (autonomous : effect.mode = AgentInteractionMode.autonomousResource)
    (shared : AutonomousSinkSharedForActor
      authorization (secured.certified.run.semanticState n).base
      effect.actor sink)
    (sourceIn : source ∈ effect.contextSources)
    (principalSeesSink :
      DisclosureSinkReadableBy
        authorization
        (secured.certified.run.semanticState n).base
        sink principal) :
    InformationSourceReadableBy
      authorization secured n
      (secured.certified.run.semanticState n).base
      source principal := by
  have safe := kernel.disclosureContextSafe n effect sink startLe effectAt sinkAt
  unfold ModeAwareContextSafeForDisclosure at safe
  rw [autonomous] at safe
  rcases safe with privateCase | sharedCase
  · exact False.elim
      ((autonomous_sink_private_shared_disjoint
        authorization (secured.certified.run.semanticState n).base
        effect.actor sink) ⟨privateCase.1, shared⟩)
  · exact sharedCase.2 source sourceIn principal principalSeesSink

/--
Una disclosure persistente non può diventare un canale di leakage in seguito a
un grant futuro sul sink. La provenance resta fissata; l'audience viene
rivalutata al tick futuro rispetto alle stesse sorgenti.
-/
theorem persisted_shared_agent_disclosure_respects_future_audience
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n m : Nat)
    (effect : AgentSecurityEffect V X)
    (sink : DisclosureSink V)
    (source : InformationSource V X)
    (principal : V.PrincipalId)
    (startLe : start ≤ n)
    (nLeM : n ≤ m)
    (effectAt : secured.securityEffectAt n = some effect)
    (sinkAt : effect.disclosure = some sink)
    (persists : secured.disclosureObservableAt n sink m)
    (autonomous : effect.mode = AgentInteractionMode.autonomousResource)
    (sharedAtM : AutonomousSinkSharedForActor
      authorization (secured.certified.run.semanticState m).base
      effect.actor sink)
    (sourceIn : source ∈ effect.contextSources)
    (futureReader :
      DisclosureSinkReadableBy authorization
        (secured.certified.run.semanticState m).base sink principal) :
    InformationSourceReadableBy
      authorization secured m
      (secured.certified.run.semanticState m).base
      source principal := by
  have safe := kernel.persistedDisclosureContextSafe
    n m effect sink startLe nLeM effectAt sinkAt persists
  unfold ModeAwareContextSafeForDisclosure at safe
  rw [autonomous] at safe
  rcases safe with privateCase | sharedCase
  · exact False.elim
      ((autonomous_sink_private_shared_disjoint
        authorization (secured.certified.run.semanticState m).base
        effect.actor sink) ⟨privateCase.1, sharedAtM⟩)
  · exact sharedCase.2 source sourceIn principal futureReader

/--
`source ⟶ target` è sicuro quando ogni lettore plaintext del target è anche
lettore plaintext della source. È la relazione no-write-down sulle risorse.
-/
def ResourceAudienceFlowSafe
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (source target : V.ResourceId) : Prop :=
  ∀ principal,
    authorization.bodyReadable s principal target →
    authorization.bodyReadable s principal source

@[simp] theorem resource_audience_flow_safe_refl
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (resource : V.ResourceId) :
    ResourceAudienceFlowSafe authorization s resource resource := by
  intro principal readable
  exact readable

theorem resource_audience_flow_safe_trans
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    {first middle last : V.ResourceId}
    (firstToMiddle :
      ResourceAudienceFlowSafe authorization s first middle)
    (middleToLast :
      ResourceAudienceFlowSafe authorization s middle last) :
    ResourceAudienceFlowSafe authorization s first last := by
  intro principal lastReadable
  exact firstToMiddle principal (middleToLast principal lastReadable)

/-- Chiusura di una catena di disclosure resource→resource sicure. -/
inductive ResourceInformationFlowPath
    (authorization : ProductAuthorizationProjection V)
    (s : State V) : V.ResourceId → V.ResourceId → Prop where
  | refl (resource : V.ResourceId) :
      ResourceInformationFlowPath authorization s resource resource
  | step
      {source intermediate target : V.ResourceId} :
      ResourceInformationFlowPath authorization s source intermediate →
      ResourceAudienceFlowSafe authorization s intermediate target →
      ResourceInformationFlowPath authorization s source target

/--
Noninterference collaborativa per una catena arbitraria di hop: la possibilità
di passare attraverso più agenti/risorse non può ampliare l'audience finale.
-/
theorem collaborative_information_noninterference
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (source target : V.ResourceId)
    (path : ResourceInformationFlowPath authorization s source target) :
    ResourceAudienceFlowSafe authorization s source target := by
  induction path <;> simp_all [ResourceAudienceFlowSafe]

/--
Specializzazione al rischio descritto: una source task/resource non può essere
riversata in una task/resource target leggibile da un principal che non può
leggere la source.
-/
theorem agent_cannot_disclose_resource_across_visibility_boundary
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (sourceResource targetResource : V.ResourceId)
    (principal : V.PrincipalId)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (autonomous : effect.mode = AgentInteractionMode.autonomousResource)
    (sharedTarget : AutonomousSinkSharedForActor
      authorization (secured.certified.run.semanticState n).base
      effect.actor (DisclosureSink.resourceBody targetResource))
    (sourceIn :
      InformationSource.resourceBody sourceResource ∈ effect.contextSources)
    (targetSink :
      effect.disclosure = some (DisclosureSink.resourceBody targetResource))
    (principalReadsTarget :
      authorization.bodyReadable
        (secured.certified.run.semanticState n).base
        principal targetResource) :
    authorization.bodyReadable
      (secured.certified.run.semanticState n).base
      principal sourceResource := by
  exact
    agent_shared_disclosure_respects_sink_audience
      authorization secured contract runId start kernel
      n effect (DisclosureSink.resourceBody targetResource)
      (InformationSource.resourceBody sourceResource)
      principal startLe effectAt targetSink autonomous sharedTarget
      sourceIn principalReadsTarget

/--
Specializzazione ai commenti: un agente non può riversare in un commento
osservabile da P informazione proveniente da una resource body che P non può
leggere, anche se P è autorizzato a commentare/interagire con l'agente.
-/
theorem agent_cannot_disclose_resource_into_comment_for_unauthorized_reader
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (sourceResource commentTarget : V.ResourceId)
    (principal : V.PrincipalId)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (autonomous : effect.mode = AgentInteractionMode.autonomousResource)
    (sharedCommentTarget : AutonomousSinkSharedForActor
      authorization (secured.certified.run.semanticState n).base
      effect.actor (DisclosureSink.commentOn commentTarget))
    (sourceIn :
      InformationSource.resourceBody sourceResource ∈ effect.contextSources)
    (commentSink :
      effect.disclosure = some (DisclosureSink.commentOn commentTarget))
    (principalReadsComment :
      authorization.commentReadable
        (secured.certified.run.semanticState n).base
        principal commentTarget) :
    authorization.bodyReadable
      (secured.certified.run.semanticState n).base
      principal sourceResource := by
  exact
    agent_shared_disclosure_respects_sink_audience
      authorization secured contract runId start kernel
      n effect (DisclosureSink.commentOn commentTarget)
      (InformationSource.resourceBody sourceResource)
      principal startLe effectAt commentSink autonomous sharedCommentTarget
      sourceIn principalReadsComment

/-- Una disclosure resource→resource valida stabilisce un edge no-write-down. -/
theorem agent_resource_disclosure_establishes_safe_audience_flow
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (sourceResource targetResource : V.ResourceId)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (autonomous : effect.mode = AgentInteractionMode.autonomousResource)
    (sharedTarget : AutonomousSinkSharedForActor
      authorization (secured.certified.run.semanticState n).base
      effect.actor (DisclosureSink.resourceBody targetResource))
    (sourceIn :
      InformationSource.resourceBody sourceResource ∈ effect.contextSources)
    (targetSink :
      effect.disclosure = some (DisclosureSink.resourceBody targetResource)) :
    ResourceAudienceFlowSafe
      authorization
      (secured.certified.run.semanticState n).base
      sourceResource targetResource := by
  intro principal targetReadable
  exact
    agent_cannot_disclose_resource_across_visibility_boundary
      authorization secured contract runId start kernel
      n effect sourceResource targetResource principal
      startLe effectAt autonomous sharedTarget sourceIn targetSink targetReadable

/--
Corollario anti-laundering: due passaggi sicuri restano sicuri. Per induzione
la stessa proprietà vale per una catena arbitraria agent→agent/resource→resource
all'interno di uno stato di autorizzazione fissato.
-/
theorem two_hop_agent_information_flow_cannot_amplify_audience
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    {source intermediate target : V.ResourceId}
    (firstHop : ResourceAudienceFlowSafe authorization s source intermediate)
    (secondHop : ResourceAudienceFlowSafe authorization s intermediate target) :
    ResourceAudienceFlowSafe authorization s source target := by
  exact resource_audience_flow_safe_trans authorization s firstHop secondHop

/--
Info usa esclusivamente il container come security domain. Non esiste un ACL
Info indipendente nel modello normativo.
-/
theorem info_disclosure_uses_container_audience
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (source : InformationSource V X)
    (container : V.ResourceId)
    (principal : V.PrincipalId)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (autonomous : effect.mode = AgentInteractionMode.autonomousResource)
    (sharedContainer : AutonomousSinkSharedForActor
      authorization (secured.certified.run.semanticState n).base
      effect.actor (DisclosureSink.infoDocument container))
    (sourceIn : source ∈ effect.contextSources)
    (sinkAt :
      effect.disclosure = some (DisclosureSink.infoDocument container))
    (principalReadsInfo :
      InfoReadAllowed authorization
        (secured.certified.run.semanticState n).base
        principal container) :
    InformationSourceReadableBy
      authorization secured n
      (secured.certified.run.semanticState n).base
      source principal := by
  exact
    agent_shared_disclosure_respects_sink_audience
      authorization secured contract runId start kernel
      n effect (DisclosureSink.infoDocument container)
      source principal startLe effectAt sinkAt autonomous sharedContainer
      sourceIn principalReadsInfo

/--
Proprietà osservabile finale del safety layer. È una struttura nominata anziché
un grande congiunto, così ogni garanzia resta auditabile separatamente nel
refinement concreto.
-/
structure AuthorityInformationSafetyHolds
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat) : Prop where

  /-- Tutti gli invarianti locali/coverage del security refinement restano parte della proprietà finale. -/
  kernelConformant :
    AuthorityInformationKernelCertificate
      authorization secured contract runId start

  effectAuthoritySafe :
    ∀ n effect resourceEffect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      resourceEffect ∈ effect.footprint →
      EffectiveAuthorityAt
        authorization
        (secured.certified.run.semanticState n).base
        (secured.workAuthority effect.work)
        (secured.workAuthorityPrincipal effect.work)
        resourceEffect.resource resourceEffect.operation ∧
      ResourceOperationAllowed
        authorization
        (secured.certified.run.semanticState n).base
        effect.actor resourceEffect.resource resourceEffect.operation

  humanTaskControlIsolation :
    ∀ n effect resourceEffect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      resourceEffect ∈ effect.footprint →
      HumanAssignedTaskWithoutAgent
        (secured.certified.run.semanticState n).base
        effect.actor resourceEffect.resource →
      TaskControlOperation resourceEffect.operation →
      False

  effectWithinRunScope :
    ∀ n effect resourceEffect scope,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      resourceEffect ∈ effect.footprint →
      (secured.certified.run.semanticState n).runScope runId = some scope →
      ResourceWithinScope
        (secured.certified.run.semanticState n).base
        scope resourceEffect.resource

  modelContextWithinRunScope :
    ∀ n effect source resource operation scope,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      source ∈ effect.contextSources →
      InformationSourceReadRequirement source = some (resource, operation) →
      (secured.certified.run.semanticState n).runScope runId = some scope →
      ResourceWithinScope
        (secured.certified.run.semanticState n).base scope resource

  effectWorkSemanticallyEnabled :
    ∀ n effect work,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      (secured.certified.run.semanticState n).workItems effect.work = some work →
      ContractWorkSemanticallyEnabledForEffect
        (secured.certified.run.semanticState n) contract work

  effectSecurityPolicyAllowed :
    ∀ n effect certificate,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      secured.certified.workCertificateAt n effect.work = some certificate →
      (∀ resourceEffect,
        resourceEffect ∈ effect.footprint →
        resourceEffect.operation ∈
          (secured.workSecurityPolicy certificate.workSpecId).allowedOperations) ∧
      (∀ invocation,
        invocation ∈ effect.toolInvocations →
        invocation.tool ∈
          (secured.workSecurityPolicy certificate.workSpecId).allowedTools)

  coreAgentActionAllowedByContract :
    ∀ n effect action certificate workSpec,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      secured.certified.run.baseRun.move n =
        some (Move.agentMove effect.actor action) →
      secured.certified.workCertificateAt n effect.work = some certificate →
      workSpec ∈ contract.workSpecs →
      certificate.workSpecId = workSpec.id →
      ContractWorkSpecAllowsAgentAction workSpec action

  canonicalResourceBody :
    ∀ n resource first second,
      start ≤ n →
      authorization.bodyReadable
        (secured.certified.run.semanticState n).base first resource →
      authorization.bodyReadable
        (secured.certified.run.semanticState n).base second resource →
      secured.observedResourceBodyAt n first resource =
        secured.observedResourceBodyAt n second resource

  contextualChatActionSafe :
    ∀ n effect,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      ContextualChatActionSafe authorization secured n effect

  disclosureSafeAtWrite :
    ∀ n effect sink,
      start ≤ n →
      secured.securityEffectAt n = some effect →
      effect.disclosure = some sink →
      ModeAwareContextSafeForDisclosure
        authorization secured n effect sink

  persistedDisclosureSafe :
    ∀ n m effect sink,
      start ≤ n →
      n ≤ m →
      secured.securityEffectAt n = some effect →
      effect.disclosure = some sink →
      secured.disclosureObservableAt n sink m →
      ModeAwareContextSafeForDisclosure
        authorization secured m effect sink

  authorityAttenuatesTransitively :
    ∀ tick root child,
      start ≤ tick →
      WorkDescendsFromAt secured tick root child →
      AuthoritySubset
        (secured.workAuthority child)
        (secured.workAuthority root)

  toolAuthorityAttenuatesTransitively :
    ∀ tick root child,
      start ≤ tick →
      WorkDescendsFromAt secured tick root child →
      ToolAuthoritySubset
        (secured.workToolAuthority child)
        (secured.workToolAuthority root)

/-- Bundle finale dei safety target aggiuntivi, separato dal completion kernel. -/
structure AssumptionMinimalAuthorityInformationSafetyCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat) : Prop where
  kernel : AuthorityInformationKernelCertificate
    authorization secured contract runId start

/--
Safety theorem applicativo: il bundle interno implica simultaneamente authority
non-amplification e information confinement per ogni effetto/disclosure
osservato. Non introduce nuove boundary umane/ambientali.
-/
theorem sprout_authority_information_safety
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (safety : AssumptionMinimalAuthorityInformationSafetyCertificate
      authorization secured contract runId start) :
    AuthorityInformationSafetyHolds
      authorization secured contract runId start := by
  refine {
    kernelConformant := safety.kernel
    effectAuthoritySafe := safety.kernel.effectAuthoritySafe
    humanTaskControlIsolation := safety.kernel.humanAssignedTaskControlIsolation
    effectWithinRunScope := safety.kernel.effectWithinRunScope
    modelContextWithinRunScope := safety.kernel.modelContextWithinRunScope
    effectWorkSemanticallyEnabled := safety.kernel.effectWorkSemanticallyEnabled
    effectSecurityPolicyAllowed := safety.kernel.effectSecurityPolicyAllowed
    coreAgentActionAllowedByContract := safety.kernel.coreAgentActionAllowedByContract
    canonicalResourceBody := safety.kernel.canonicalResourceBody
    contextualChatActionSafe := safety.kernel.contextualChatActionSafe
    disclosureSafeAtWrite := safety.kernel.disclosureContextSafe
    persistedDisclosureSafe := safety.kernel.persistedDisclosureContextSafe
    authorityAttenuatesTransitively := ?_
    toolAuthorityAttenuatesTransitively := ?_
  }
  · intro tick root child startLe chain
    exact
      collaborative_authority_attenuation
        authorization secured contract runId start safety.kernel
        tick root child startLe chain
  · intro tick root child startLe chain
    exact
      collaborative_tool_authority_attenuation
        authorization secured contract runId start safety.kernel
        tick root child startLe chain

/--
Bundle applicativo che combina il kernel di completion R5.30 con il nuovo
kernel di safety R5.33 senza trasformare la safety in external assumption.
-/
structure SecureAssumptionMinimalFullSuccessKernelCertificate
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (judge : SemanticEvidenceJudge V X)
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat) : Prop where
  completion :
    AssumptionMinimalFullSuccessKernelCertificate
      measure policy judge secured.certified runId contract start
  safety :
    AssumptionMinimalAuthorityInformationSafetyCertificate
      authorization secured contract runId start

/--
Theorem applicativo sicuro: sullo stesso concrete refinement valgono insieme
eventual successful completion del GoalContract e le proprietà di authority /
information-flow safety. Le boundary di completion restano quelle R5.30; la
safety aggiunta è un certificate interno del refinement.
-/
theorem sprout_secure_assumption_minimal_successful_completion
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (authorization : ProductAuthorizationProjection V)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (secured : SecuredCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel :
      SecureAssumptionMinimalFullSuccessKernelCertificate
        measure policy evidenceJudge authorization
        secured runId (compiler.compile prompt) start)
    (boundary :
      MinimalContractSuccessExternalAssumptions
        secured.certified.run
        (compiler.compile prompt).goal.id
        start) :
    EventuallyCollaborativeContractCompleted
        secured.certified.run runId (compiler.compile prompt) start ∧
      AuthorityInformationSafetyHolds
        authorization secured (compiler.compile prompt) runId start := by
  constructor
  · exact
      sprout_assumption_minimal_successful_completion
        measure policy evidenceJudge compiler prompt
        secured.certified runId start kernel.completion boundary
  · exact
      sprout_authority_information_safety
        authorization secured (compiler.compile prompt) runId start kernel.safety

/--
Boundary semantica finale per poter parlare di successo prompt-faithful anche
quando il work usa operazioni product-specific della security policy. Non è
necessaria per i theorem meccanici di authority/information safety.
-/
structure MinimalPromptFaithfulSecureExternalAssumptions
    (meaning : PromptContractSemantics V X)
    (securityMeaning : PromptSecurityPolicySemantics V X)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (intendedEvidence : IntendedEvidenceSemantics V X)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (secured : SecuredCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat) : Prop where
  promptFaithful :
    MinimalPromptFaithfulSuccessExternalAssumptions
      meaning evidenceJudge intendedEvidence
      compiler prompt secured.certified runId start
  securityPolicyAdequacy :
    PromptSecurityPolicyAdequacy
      securityMeaning prompt (compiler.compile prompt) secured

/--
Corollario prompt-faithful sicuro: combina il corollario semantico R5.30 con il
nuovo safety theorem e rende esplicita anche l'adeguatezza linguistica
prompt→security-policy per `EditInfo`/tool product-specific.
-/
theorem sprout_secure_prompt_faithful_successful_completion
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (meaning : PromptContractSemantics V X)
    (securityMeaning : PromptSecurityPolicySemantics V X)
    (evidenceJudge : SemanticEvidenceJudge V X)
    (intendedEvidence : IntendedEvidenceSemantics V X)
    (authorization : ProductAuthorizationProjection V)
    (compiler : ContractCompiler V X)
    (prompt : V.SystemPrompt)
    (secured : SecuredCollaborativeRun V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel :
      SecureAssumptionMinimalFullSuccessKernelCertificate
        measure policy evidenceJudge authorization
        secured runId (compiler.compile prompt) start)
    (boundary :
      MinimalPromptFaithfulSecureExternalAssumptions
        meaning securityMeaning evidenceJudge intendedEvidence
        compiler prompt secured runId start) :
    EventuallyCollaborativeContractCompleted
        secured.certified.run runId (compiler.compile prompt) start ∧
      AuthorityInformationSafetyHolds
        authorization secured (compiler.compile prompt) runId start ∧
      PromptContractAdequacy meaning compiler prompt ∧
      IntendedContractDischargeSoundness
        intendedEvidence secured.certified runId
        (compiler.compile prompt) start ∧
      PromptSecurityPolicyAdequacy
        securityMeaning prompt (compiler.compile prompt) secured := by
  have semantic :=
    sprout_prompt_faithful_successful_completion
      measure policy meaning evidenceJudge intendedEvidence
      compiler prompt secured.certified runId start
      kernel.completion boundary.promptFaithful
  have safety :=
    sprout_authority_information_safety
      authorization secured (compiler.compile prompt) runId start kernel.safety
  constructor
  · exact semantic.1
  · constructor
    · exact safety
    · constructor
      · exact semantic.2.1
      · constructor
        · exact semantic.2.2
        · exact boundary.securityPolicyAdequacy


/-! ### R5.34 — Canonical resource information e modalità chat/private/shared -/

/--
Classificazione normativa aggiuntiva richiesta dal prodotto:
* `contextualChat` è l'unico contesto per-user e supervisionato;
* un autonomous resource privato usa il controller dell'agente come ceiling
  informativo;
* un autonomous resource condiviso usa un payload canonico costruibile soltanto
  da sorgenti compatibili con l'intera audience effettiva del sink.
-/

theorem shared_autonomous_context_is_audience_intersection
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (effect : AgentSecurityEffect V X)
    (sink : DisclosureSink V)
    (mode : effect.mode = AgentInteractionMode.autonomousResource)
    (shared : AutonomousSinkSharedForActor
      authorization (secured.certified.run.semanticState tick).base
      effect.actor sink)
    (safe : ModeAwareContextSafeForDisclosure
      authorization secured tick effect sink) :
    ContextSafeForDisclosure
      authorization secured tick effect.contextSources sink := by
  unfold ModeAwareContextSafeForDisclosure at safe
  rw [mode] at safe
  rcases safe with privateCase | sharedCase
  · exact False.elim
      ((autonomous_sink_private_shared_disjoint
        authorization (secured.certified.run.semanticState tick).base
        effect.actor sink) ⟨privateCase.1, shared⟩)
  · exact sharedCase.2

theorem private_autonomous_context_is_controller_scoped
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (effect : AgentSecurityEffect V X)
    (sink : DisclosureSink V)
    (mode : effect.mode = AgentInteractionMode.autonomousResource)
    (privateSink : AutonomousSinkPrivateForActor
      authorization (secured.certified.run.semanticState tick).base
      effect.actor sink)
    (safe : ModeAwareContextSafeForDisclosure
      authorization secured tick effect sink) :
    ∃ controller,
      authorization.agentController
          (secured.certified.run.semanticState tick).base
          effect.actor = some controller ∧
      ContextReadableByPrincipal
        authorization secured tick effect.contextSources controller := by
  unfold ModeAwareContextSafeForDisclosure at safe
  rw [mode] at safe
  rcases safe with privateCase | sharedCase
  · exact privateCase.2
  · exact False.elim
      ((autonomous_sink_private_shared_disjoint
        authorization (secured.certified.run.semanticState tick).base
        effect.actor sink) ⟨privateSink, sharedCase.1⟩)

/-- Due reader della stessa risorsa persistita osservano lo stesso body canonico. -/
theorem canonical_resource_body_is_not_per_reader
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (resource : V.ResourceId)
    (first second : V.PrincipalId)
    (startLe : start ≤ n)
    (firstReads : authorization.bodyReadable
      (secured.certified.run.semanticState n).base first resource)
    (secondReads : authorization.bodyReadable
      (secured.certified.run.semanticState n).base second resource) :
    secured.observedResourceBodyAt n first resource =
      secured.observedResourceBodyAt n second resource := by
  exact kernel.canonicalResourceBody
    n resource first second startLe firstReads secondReads

/--
La chat supervisionata usa un solo principal come ceiling sia informativo sia
operativo; non è una variante privata di un body persistito condiviso.
-/
theorem contextual_chat_is_user_scoped
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (effect : AgentSecurityEffect V X)
    (supervisor : V.PrincipalId)
    (mode : effect.mode = AgentInteractionMode.contextualChat supervisor)
    (contextSafe : ModeAwareContextSafeForDisclosure
      authorization secured tick effect
      (DisclosureSink.contextualChat supervisor))
    (actionSafe : ContextualChatActionSafe authorization secured tick effect) :
    ContextReadableByPrincipal
        authorization secured tick effect.contextSources supervisor ∧
      secured.workAuthorityPrincipal effect.work = supervisor := by
  constructor
  · unfold ModeAwareContextSafeForDisclosure at contextSafe
    rw [mode] at contextSafe
    exact contextSafe.2
  · unfold ContextualChatActionSafe at actionSafe
    rw [mode] at actionSafe
    exact actionSafe.1

/--
Il contenuto canonico non forza l'intersezione dei diritti di SCRITTURA dei
reader: la shared-audience intersection riguarda le sorgenti informative del
payload. L'authority dell'effetto continua a essere verificata separatamente da
`effectAuthoritySafe`.
-/
theorem canonical_information_and_action_authority_are_orthogonal
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (kernel : AuthorityInformationKernelCertificate
      authorization secured contract runId start)
    (n : Nat)
    (effect : AgentSecurityEffect V X)
    (resourceEffect : ResourceSecurityEffect V)
    (startLe : start ≤ n)
    (effectAt : secured.securityEffectAt n = some effect)
    (effectIn : resourceEffect ∈ effect.footprint) :
    EffectiveAuthorityAt
        authorization (secured.certified.run.semanticState n).base
        (secured.workAuthority effect.work)
        (secured.workAuthorityPrincipal effect.work)
        resourceEffect.resource resourceEffect.operation ∧
      ResourceOperationAllowed
        authorization (secured.certified.run.semanticState n).base
        effect.actor resourceEffect.resource resourceEffect.operation := by
  exact kernel.effectAuthoritySafe n effect resourceEffect startLe effectAt effectIn

/-
REFINEMENT NOTE R5.33

Le proprietà di authority/information safety sono INTERNAL SAFETY TARGETS, non
external assumptions. Il prodotto concreto deve quindi costruire/provare il
certificate a partire da:
* permission engine Rust + RLS PostgreSQL + E2EE envelope delivery;
* `bodyReadable` come visibilità plaintext effettiva, non semplice 2xx server;
* `commentAllowed` e `commentReadable` come policy reale del contesto in cui
  l'agente è presente/commentabile;
* `agentController` e `projectAdministrator` risolti server-side, senza fidarsi
  di identità/controller dichiarati dal client;
* mapping preciso di owner/admin/member/guest e grant full/container_only;
* capability speciale `DelegateAssignedWork` senza concedere Write/Manage generici;
* isolamento della source task umana anche quando l'agente ne era creator;
* Info sempre mappato al resource_node_id del topic/task-list contenitore;
* work authority immutabile e attenuata lungo ogni parent/child;
* rivalutazione dei permessi correnti per rendere effettive le revoche;
* tool footprint resource-sensitive;
* context builder che espone al modello soltanto source autorizzate per il work;
* `contextSources` completo rispetto al plaintext realmente esposto al modello;
* body persistito di topic/task-list/task canonico e univoco per tutti i reader;
* chat contestuale privata user-scoped per informazioni E azioni;
* autonomous resource privato controller-scoped per il context informativo;
* autonomous resource condiviso con context pari all'intersezione informativa
  dell'audience effettiva del sink, senza intersecare i diritti Write/Manage dei reader;
* provenance persistita insieme alla versione/record dell'output, così futuri
  grant sul sink non amplificano l'audience mentre quel contenuto resta osservabile;
* provenance transitiva attraverso agent→agent, commenti, tool e work continuation.

La sicurezza non dipende dalla buona volontà del modello: prompt engineering è
soltanto defense-in-depth. Il certificate richiede che authorization/context/
disclosure siano enforce dal runtime/refinement concreto.

`PromptSecurityPolicyAdequacy` resta invece una boundary SEMANTICA soltanto per
la pretesa prompt-faithful: attesta che le operazioni product-specific ammesse
dalla policy di WorkSpec (per esempio EditInfo) rappresentano davvero l'intento
linguistico del prompt. Non è necessaria per i theorem meccanici di safety.
-/


/-! ### R5.35 — Responsabilità delegate, goal locali e sintesi globale bottom-up -/

/-
Questa sezione aggiunge il layer di governance concordato senza modificare il
kernel R5.30 di completion né il safety kernel R5.33/R5.34.

Principi normativi:
1. l'administrator assegna a ciascun user un ResponsibilityContract versionato;
2. user e administrator possono creare agenti, ma la creazione non amplifica
   authority e ogni agente ha un controller umano server-side;
3. il prompt di un agente produce un LocalGoalContract, non direttamente il
   GoalContract globale;
4. una revisione locale può attivarsi automaticamente soltanto quando TUTTO il
   nuovo LocalGoalContract è coperto dalla responsabilità corrente del controller;
5. non esiste accettazione parziale invisibile: se una parte non è coperta, la
   nuova coppia prompt/LocalGoalContract resta bozza e il prompt attivo precedente
   continua a essere autorevole;
6. il controller può riscrivere la bozza oppure scegliere esplicitamente di
   escalare la parte eccedente tramite una task assegnata a un administrator;
7. l'administrator può rifiutare, approvare il solo goal locale, oppure approvare
   il goal locale e contemporaneamente revisionare la responsabilità dell'user;
8. il testo finale del prompt e la specifica locale approvati sono editabili in
   review, ma l'editing non produce effetto prima della decisione finale;
9. la configurazione attiva mantiene allineati prompt visibile, LocalGoalContract
   e provenance di authorization;
10. i LocalGoalContract autorizzati e originati da intent reale contribuiscono
    bottom-up alla sintesi globale; i mandati derivati dal globale non vengono
    reimmessi come nuova sorgente, evitando feedback loop;
11. una revisione globale automatica è ammessa quando è interamente sostenuta da
    contributi locali già delegati/approvati e non contiene conflitti di governance;
12. quando un administrator introduce nuovo work globale, Sprout può proporre un
    agente esistente project-delegable compatibile oppure un nuovo agente con
    footprint minimo di authority/tool; i grant restano decisioni separate;
13. responsibility e runtime permission restano ortogonali: la prima autorizza
    il mandato organizzativo, la seconda autorizza gli effetti concreti.
-/

/--
Carrier testuale conservativo per la responsabilità amministrativa.
R5 non assume una nuova concretezza del testo: nel refinement di prodotto questo
campo è il testo leggibile mostrato all'administrator/user. Il significato
strutturato è espresso dalle `ResponsibilityRule` sottostanti.
-/
abbrev ResponsibilityText (V : Vocabulary) := V.SystemPrompt

/-- Regola strutturata finita di responsabilità organizzativa. -/
structure ResponsibilityRule (V : Vocabulary) where
  /-- Categoria organizzativa deterministica assegnata dal responsibility compiler. -/
  domain : Nat
  /-- Scope resource nel quale il controller può definire autonomamente work locale. -/
  scope : V.ResourceId
  /-- Classi di azione che i goal locali possono richiedere sotto questa regola. -/
  allowedActions : List AgentActionClass


/-- Compiler strutturale del testo di responsabilità amministrativa. -/
structure ResponsibilityCompiler (V : Vocabulary) where
  compile : ResponsibilityText V → List (ResponsibilityRule V)

/-- Boundary semantica esplicita testo responsibility → regole strutturate. -/
structure ResponsibilityTextSemantics (V : Vocabulary) where
  adequate : ResponsibilityText V → List (ResponsibilityRule V) → Prop


/-- Snapshot append-only della responsabilità delegata da un administrator a un user. -/
structure ResponsibilityContract (V : Vocabulary) where
  id : Nat
  revision : Nat
  administrator : V.PrincipalId
  user : V.PrincipalId
  sourceText : ResponsibilityText V
  rules : List (ResponsibilityRule V)
  supersedesRevision : Option Nat


/-- Le regole persistite devono essere esattamente l'output del compiler. -/
def ResponsibilityContractCompiledBy
    (compiler : ResponsibilityCompiler V)
    (responsibility : ResponsibilityContract V) : Prop :=
  responsibility.rules = compiler.compile responsibility.sourceText

/-- Adeguatezza intenzionale del testo finale mostrato all'administrator. -/
def ResponsibilityTextAdequacy
    (meaning : ResponsibilityTextSemantics V)
    (compiler : ResponsibilityCompiler V)
    (responsibility : ResponsibilityContract V) : Prop :=
  meaning.adequate responsibility.sourceText (compiler.compile responsibility.sourceText)


/--
Validità organizzativa di una ResponsibilityContract rispetto al progetto reale.
L'administrator deve essere administrator del progetto di ogni scope incluso.
-/
def ResponsibilityContractValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (responsibility : ResponsibilityContract V) : Prop :=
  HasKind s responsibility.administrator PrincipalKind.administrator ∧
  HasKind s responsibility.user PrincipalKind.user ∧
  responsibility.rules ≠ [] ∧
  ∀ rule,
    rule ∈ responsibility.rules →
    ∃ resourceMeta,
      s.resources rule.scope = some resourceMeta ∧
      authorization.projectAdministrator
        s responsibility.administrator resourceMeta.projectId


/--
Certificate necessario per attivare una ResponsibilityContract editata. Anche
l'administrator non può attivare regole che divergono dal testo finale mostrato.
-/
structure ResponsibilityActivationCertificate
    (authorization : ProductAuthorizationProjection V)
    (compiler : ResponsibilityCompiler V)
    (meaning : ResponsibilityTextSemantics V)
    (s : State V)
    (responsibility : ResponsibilityContract V) : Prop where
  organizationallyValid : ResponsibilityContractValid authorization s responsibility
  compiled : ResponsibilityContractCompiledBy compiler responsibility
  textAdequate : ResponsibilityTextAdequacy meaning compiler responsibility


/-- Una revisione della responsabilità è append-only e conserva identità/user/admin. -/
def ResponsibilityRevisionOf
    (previous next : ResponsibilityContract V) : Prop :=
  next.id = previous.id ∧
  next.revision = previous.revision + 1 ∧
  next.administrator = previous.administrator ∧
  next.user = previous.user ∧
  next.supersedesRevision = some previous.revision

/--
Clause strutturata del goal locale. `domain` è la categoria organizzativa usata
per il responsibility check; `workSpecIds` collega la clause al GoalContract
eseguibile senza introdurre un secondo linguaggio di work.
-/
structure LocalGoalClause (V : Vocabulary) where
  id : Nat
  domain : Nat
  scope : V.ResourceId
  workSpecIds : List Nat

/-- Provenance normativa di una revisione locale. -/
inductive LocalGoalOrigin where
  | controllerPrompt
  | administratorException (reviewId : Nat)
  | administratorCreation (approvalId : Nat)
  | globalMandate (globalRevision : Nat)
  deriving DecidableEq, Repr

/--
Contratto locale di un singolo agente. `contract` riusa integralmente il DSL
GoalContract R5.30; il nuovo wrapper aggiunge controller, prompt, provenance e
clausole necessarie alla governance bottom-up.
-/
structure LocalGoalContract
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  id : Nat
  revision : Nat
  agent : V.PrincipalId
  controller : V.PrincipalId
  prompt : V.SystemPrompt
  contract : GoalContract V X
  clauses : List (LocalGoalClause V)
  origin : LocalGoalOrigin
  supersedesRevision : Option Nat


/--
Classifier deterministico GoalContract→clausole di responsabilità. Il runtime non
può accettare categorie `domain` dichiarate liberamente dal client/modello.
-/
structure LocalGoalClassifier
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  classify : GoalContract V X → List (LocalGoalClause V)

/-- Boundary semantica della classificazione organizzativa del goal locale. -/
structure LocalGoalClassificationSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  adequate : GoalContract V X → List (LocalGoalClause V) → Prop

/-- Le clauses persistite sono esattamente quelle prodotte dal classifier. -/
def LocalGoalClassifiedBy
    (classifier : LocalGoalClassifier V X)
    (localGoal : LocalGoalContract V X) : Prop :=
  localGoal.clauses = classifier.classify localGoal.contract

/-- Adeguatezza semantica della classificazione usata dal responsibility gate. -/
def LocalGoalClassificationAdequacy
    (meaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (localGoal : LocalGoalContract V X) : Prop :=
  meaning.adequate localGoal.contract (classifier.classify localGoal.contract)


/-- Directory dei goal locali attivi. -/
abbrev LocalGoalDirectory
    (V : Vocabulary)
    (X : ExtensionVocabulary V) :=
  V.PrincipalId → Option (LocalGoalContract V X)

/-- Directory delle responsabilità attive per user. -/
abbrev ResponsibilityDirectory (V : Vocabulary) :=
  V.PrincipalId → Option (ResponsibilityContract V)

/-- Una clause locale può riferire soltanto WorkSpec presenti nel contratto locale. -/
def LocalGoalClauseReferencesKnownWork
    (localGoal : LocalGoalContract V X)
    (clause : LocalGoalClause V) : Prop :=
  ∀ workSpecId,
    workSpecId ∈ clause.workSpecIds →
    ContractWorkSpecKnown localGoal.contract workSpecId

/-- Ogni WorkSpec del contratto locale deve essere classificata da almeno una clause. -/
def EveryLocalWorkClassified
    (localGoal : LocalGoalContract V X) : Prop :=
  ∀ workSpec,
    workSpec ∈ localGoal.contract.workSpecs →
    ∃ clause,
      clause ∈ localGoal.clauses ∧
      workSpec.id ∈ clause.workSpecIds

/--
Un GoalContract locale appartiene realmente al singolo agente: obligation e
WorkSpec eseguibili sono owned dallo stesso principal agentico.
-/
def LocalContractOwnedByAgent
    (localGoal : LocalGoalContract V X) : Prop :=
  (∀ obligation,
    obligation ∈ localGoal.contract.obligations →
    obligation.owner = localGoal.agent) ∧
  (∀ workSpec,
    workSpec ∈ localGoal.contract.workSpecs →
    workSpec.owner = localGoal.agent)

/-- Validazione strutturale deterministica del wrapper locale. -/
structure LocalGoalContractWellFormed
    (localGoal : LocalGoalContract V X) : Prop where
  contractWellFormed : GoalContractWellFormed localGoal.contract
  clausesNonempty : localGoal.clauses ≠ []
  clauseWorkNonempty :
    ∀ clause,
      clause ∈ localGoal.clauses →
      clause.workSpecIds ≠ []
  clauseWorkKnown :
    ∀ clause,
      clause ∈ localGoal.clauses →
      LocalGoalClauseReferencesKnownWork localGoal clause
  everyWorkClassified : EveryLocalWorkClassified localGoal
  ownedByAgent : LocalContractOwnedByAgent localGoal

/-- Il compiler locale è lo stesso compiler R5.30, applicato al prompt del singolo agente. -/
def LocalPromptCompiledContract
    (compiler : ContractCompiler V X)
    (localGoal : LocalGoalContract V X) : Prop :=
  localGoal.contract = compiler.compile localGoal.prompt

/--
Coerenza semantica forte prompt visibile ↔ contratto locale. La parte
`PromptContractAdequacy` resta la stessa boundary linguistica esplicita di R5.30;
non viene finta come conseguenza del solo parser/schema.
-/
def LocalPromptContractAgreement
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (localGoal : LocalGoalContract V X) : Prop :=
  LocalPromptCompiledContract compiler localGoal ∧
  PromptContractAdequacy meaning compiler localGoal.prompt

/-- Una ResponsibilityRule copre tutte le classi di azione del work riferito. -/
def ResponsibilityRuleCoversWork
    (rule : ResponsibilityRule V)
    (workSpec : ContractWorkSpec V X) : Prop :=
  ∀ actionClass,
    actionClass ∈ workSpec.allowedActions →
    actionClass ∈ rule.allowedActions

/-- Copertura strutturale di una singola clause locale. -/
def ResponsibilityRuleCoversClause
    (responsibilityRule : ResponsibilityRule V)
    (localGoal : LocalGoalContract V X)
    (clause : LocalGoalClause V) : Prop :=
  responsibilityRule.domain = clause.domain ∧
  responsibilityRule.scope = clause.scope ∧
  ∀ workSpec,
    workSpec ∈ localGoal.contract.workSpecs →
    workSpec.id ∈ clause.workSpecIds →
    ResponsibilityRuleCoversWork responsibilityRule workSpec

/--
Tutto il LocalGoalContract è dentro la responsabilità delegata. Non basta che
una sotto-parte sia coperta: ogni clause deve avere una regola corrispondente.
-/
def ResponsibilityCoversLocalGoal
    (responsibility : ResponsibilityContract V)
    (localGoal : LocalGoalContract V X) : Prop :=
  responsibility.user = localGoal.controller ∧
  ∀ clause,
    clause ∈ localGoal.clauses →
    ∃ rule,
      rule ∈ responsibility.rules ∧
      ResponsibilityRuleCoversClause rule localGoal clause

/--
Separazione normativa: responsibility autorizza a DEFINIRE il mandato; i
permission gate concreti continuano a decidere se un effetto è eseguibile.
-/
def ResponsibilityDoesNotImplyRuntimePermission
    (responsibility : ResponsibilityContract V)
    (localGoal : LocalGoalContract V X) : Prop :=
  ResponsibilityCoversLocalGoal responsibility localGoal

/-- Disponibilità organizzativa di un agente rispetto a nuovi mandati globali. -/
inductive AgentAvailabilityMode where
  | controllerPrivate
  | projectDelegable
  deriving DecidableEq, Repr

/-- Record di governance dell'agente separato dal suo AgentProfile runtime. -/
structure GovernedAgentRecord (V : Vocabulary) where
  agent : V.PrincipalId
  controller : V.PrincipalId
  availability : AgentAvailabilityMode

/-- Directory della governance agenti. -/
abbrev GovernedAgentDirectory (V : Vocabulary) :=
  V.PrincipalId → Option (GovernedAgentRecord V)

/--
La relazione controller osservata dal prodotto deve coincidere con la directory
di governance e con un principal umano.
-/
def GovernedAgentRecordValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (record : GovernedAgentRecord V) : Prop :=
  HasKind s record.agent PrincipalKind.agent ∧
  (∃ kind,
    HasKind s record.controller kind ∧
    IsHumanKind kind) ∧
  authorization.agentController s record.agent = some record.controller

/--
Review amministrativa aperta dopo consenso esplicito dell'user all'escalation.
La task è una task R4 reale assegnata all'administrator.
-/
structure ResponsibilityExceptionReview
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  id : Nat
  user : V.PrincipalId
  agent : V.PrincipalId
  administrator : V.PrincipalId
  sourceDraftId : Nat
  reviewTask : V.ResourceId
  /-- Riassunto leggibile della parte che eccede il responsibility contract. -/
  excessSummary : ResponsibilityText V
  /-- Versione proposta dall'user prima dell'editing amministrativo. -/
  proposedPrompt : V.SystemPrompt
  proposedLocal : LocalGoalContract V X

/-- Il sistema può creare la review task soltanto dopo consenso esplicito dell'user. -/
structure UserEscalationConsent (V : Vocabulary) where
  reviewId : Nat
  user : V.PrincipalId
  sourceDraftId : Nat
  consented : Bool

/-- Validità della review task per responsibility exception. -/
def ResponsibilityExceptionReviewValid
    (s : State V)
    (review : ResponsibilityExceptionReview V X)
    (consent : UserEscalationConsent V) : Prop :=
  consent.reviewId = review.id ∧
  consent.user = review.user ∧
  consent.sourceDraftId = review.sourceDraftId ∧
  consent.consented = true ∧
  HasKind s review.user PrincipalKind.user ∧
  HasKind s review.agent PrincipalKind.agent ∧
  HasKind s review.administrator PrincipalKind.administrator ∧
  IsResourceKind s review.reviewTask ResourceKind.task ∧
  CreatedBy s review.reviewTask review.agent ∧
  AssignedTo s review.administrator review.reviewTask

/--
Draft editabile dall'administrator. Ogni edit produce una nuova revisione draft;
nessun campo di questa struttura è di per sé attivo.
-/
structure AdministratorResponsibilityReviewDraft
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  reviewId : Nat
  revision : Nat
  administrator : V.PrincipalId
  finalPrompt : V.SystemPrompt
  finalLocal : LocalGoalContract V X
  /-- none = nessun ampliamento permanente; some = proposta editabile. -/
  finalResponsibility : Option (ResponsibilityContract V)

/-- Le tre decisioni UI concordate. -/
inductive AdministratorResponsibilityDecisionMode where
  | rejected
  | approvedGoalOnly
  | approvedGoalAndResponsibility
  deriving DecidableEq, Repr

/-- Decisione amministrativa finale riferita a una specifica revisione editata. -/
structure AdministratorResponsibilityDecision (V : Vocabulary) where
  reviewId : Nat
  reviewDraftRevision : Nat
  administrator : V.PrincipalId
  mode : AdministratorResponsibilityDecisionMode

/--
Una decisione usa esattamente la bozza finale letta dall'administrator.
`approvedGoalAndResponsibility` richiede una ResponsibilityContract finale;
`approvedGoalOnly` non la attiva, anche se la UI ne aveva mostrato una proposta.
-/
def AdministratorResponsibilityDecisionValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V) : Prop :=
  decision.reviewId = review.id ∧
  draft.reviewId = review.id ∧
  decision.reviewDraftRevision = draft.revision ∧
  decision.administrator = review.administrator ∧
  draft.administrator = review.administrator ∧
  HasKind s decision.administrator PrincipalKind.administrator ∧
  DoneTask s review.reviewTask ∧
  draft.finalLocal.agent = review.agent ∧
  draft.finalLocal.controller = review.user ∧
  draft.finalLocal.prompt = draft.finalPrompt ∧
  match decision.mode with
  | AdministratorResponsibilityDecisionMode.rejected => True
  | AdministratorResponsibilityDecisionMode.approvedGoalOnly => True
  | AdministratorResponsibilityDecisionMode.approvedGoalAndResponsibility =>
      ∃ responsibility,
        draft.finalResponsibility = some responsibility ∧
        responsibility.user = review.user ∧
        responsibility.administrator = review.administrator ∧
        ResponsibilityContractValid authorization s responsibility


/--
Validazione della versione finale editata dall'administrator prima di una
approvazione normativa. Usa gli stessi gate di prompt/classificazione del flusso
locale: l'autorità amministrativa non sostituisce well-formedness o adequacy.
-/
structure AdministratorEditedLocalDraftValidation
    (promptMeaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (draft : AdministratorResponsibilityReviewDraft V X) : Prop where
  localWellFormed : LocalGoalContractWellFormed draft.finalLocal
  promptCompiled : LocalPromptCompiledContract compiler draft.finalLocal
  promptAdequate : PromptContractAdequacy promptMeaning compiler draft.finalPrompt
  classified : LocalGoalClassifiedBy classifier draft.finalLocal
  classificationAdequate :
    LocalGoalClassificationAdequacy classificationMeaning classifier draft.finalLocal

/--
Certificate forte del ramo amministrativo approvato. Se l'administrator sceglie
anche di aggiornare la responsibility, viene validato il testo/regole finale.
-/
structure AdministratorResponsibilityApprovalCertificate
    (authorization : ProductAuthorizationProjection V)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (s : State V)
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V) : Prop where
  decisionValid :
    AdministratorResponsibilityDecisionValid authorization s review draft decision
  approved :
    decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalOnly ∨
    decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalAndResponsibility
  finalLocalValidated :
    AdministratorEditedLocalDraftValidation
      promptMeaning compiler classificationMeaning classifier draft
  finalResponsibilityValidated :
    match decision.mode with
    | AdministratorResponsibilityDecisionMode.approvedGoalAndResponsibility =>
        ∃ responsibility,
          draft.finalResponsibility = some responsibility ∧
          ResponsibilityActivationCertificate
            authorization responsibilityCompiler responsibilityMeaning s responsibility
    | _ => True


/-- Record append-only dell'eccezione locale effettivamente approvata. -/
structure ApprovedLocalGoalException
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  reviewId : Nat
  administrator : V.PrincipalId
  user : V.PrincipalId
  «local» : LocalGoalContract V X

/--
Solo `approvedGoalOnly` o `approvedGoalAndResponsibility` possono materializzare
un'eccezione; il record deve corrispondere alla versione finale editata.
-/
def ApprovedLocalGoalExceptionValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V)
    (approved : ApprovedLocalGoalException V X) : Prop :=
  AdministratorResponsibilityDecisionValid authorization s review draft decision ∧
  (decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalOnly ∨
   decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalAndResponsibility) ∧
  approved.reviewId = review.id ∧
  approved.administrator = review.administrator ∧
  approved.user = review.user ∧
  approved.«local» = draft.finalLocal ∧
  approved.«local».origin = LocalGoalOrigin.administratorException review.id


/--
Versione normativa forte dell'eccezione: oltre alla decisione, la versione finale
editata è stata ricompilata, classificata e validata prima dell'approvazione.
-/
def ApprovedLocalGoalExceptionCertified
    (authorization : ProductAuthorizationProjection V)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (s : State V)
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V)
    (approved : ApprovedLocalGoalException V X) : Prop :=
  AdministratorResponsibilityApprovalCertificate
    authorization responsibilityCompiler responsibilityMeaning
    promptMeaning compiler classificationMeaning classifier
    s review draft decision ∧
  approved.reviewId = review.id ∧
  approved.administrator = review.administrator ∧
  approved.user = review.user ∧
  approved.«local» = draft.finalLocal ∧
  approved.«local».origin = LocalGoalOrigin.administratorException review.id


/-- L'approvazione goal-only non revisiona implicitamente la responsibility. -/
def ApprovedGoalOnlyPreservesResponsibility
    (before after : ResponsibilityDirectory V)
    (review : ResponsibilityExceptionReview V X)
    (decision : AdministratorResponsibilityDecision V) : Prop :=
  decision.reviewId = review.id ∧
  decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalOnly ∧
  after review.user = before review.user

/--
Nel caso goal+responsibility, la nuova responsabilità attiva è esattamente la
versione finale editata e deve supersedere quella precedente quando esiste.
-/
def ApprovedGoalAndResponsibilityRevision
    (before after : ResponsibilityDirectory V)
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V) : Prop :=
  decision.reviewId = review.id ∧
  decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalAndResponsibility ∧
  ∃ next,
    draft.finalResponsibility = some next ∧
    after review.user = some next ∧
    match before review.user with
    | none => next.revision = 1 ∧ next.supersedesRevision = none
    | some previous => ResponsibilityRevisionOf previous next

/--
Un local goal originato da un mandato globale o da una creazione amministrativa
diretta non diventa automaticamente una nuova sorgente bottom-up.
-/
def LocalGoalCanContributeBottomUp
    («local» : LocalGoalContract V X) : Prop :=
  match «local».origin with
  | LocalGoalOrigin.controllerPrompt => True
  | LocalGoalOrigin.administratorException _ => True
  | LocalGoalOrigin.administratorCreation _ => False
  | LocalGoalOrigin.globalMandate _ => False

/-- Collegamento causale fra una clause locale e work sintetizzato nel globale. -/
structure GlobalLocalContribution (V : Vocabulary) where
  agent : V.PrincipalId
  localRevision : Nat
  localClauseId : Nat
  globalWorkSpecIds : List Nat

/-- Candidato globale prodotto dal synthesizer; non è ancora una revisione attiva. -/
structure GlobalContractCandidate
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  revision : Nat
  contract : GoalContract V X
  contributions : List (GlobalLocalContribution V)
  /-- Identificativi strutturati di conflitti che richiedono governance. -/
  governanceConflicts : List Nat


/--
Boundary semantica della sintesi bottom-up. Isola il fatto non puramente sintattico
che il contratto globale rappresenti correttamente i LocalGoalContract sorgente,
le loro dipendenze e i conflitti reali.
-/
structure GlobalSynthesisSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  adequate : LocalGoalDirectory V X → GlobalContractCandidate V X → Prop

/-- Adeguatezza intenzionale del candidate globale rispetto alle sorgenti locali. -/
def GlobalSynthesisAdequacy
    (meaning : GlobalSynthesisSemantics V X)
    (locals : LocalGoalDirectory V X)
    (candidate : GlobalContractCandidate V X) : Prop :=
  meaning.adequate locals candidate


/-- Una contribution deve provenire da una clause locale attiva e non feedback-derived. -/
def GlobalLocalContributionValid
    (locals : LocalGoalDirectory V X)
    (contribution : GlobalLocalContribution V) : Prop :=
  ∃ «local» clause,
    locals contribution.agent = some «local» ∧
    «local».revision = contribution.localRevision ∧
    LocalGoalCanContributeBottomUp «local» ∧
    clause ∈ «local».clauses ∧
    clause.id = contribution.localClauseId

/--
Certificato strutturale della sintesi globale. Non autorizza da solo authority:
attesta provenienza, copertura del work e assenza di conflitti per il percorso
automatico.
-/
structure GlobalSynthesisCertificate
    (locals : LocalGoalDirectory V X)
    (candidate : GlobalContractCandidate V X) : Prop where
  contractWellFormed : GoalContractWellFormed candidate.contract
  contributionValid :
    ∀ contribution,
      contribution ∈ candidate.contributions →
      GlobalLocalContributionValid locals contribution
  everyGlobalWorkSupported :
    ∀ workSpec,
      workSpec ∈ candidate.contract.workSpecs →
      ∃ contribution,
        contribution ∈ candidate.contributions ∧
        workSpec.id ∈ contribution.globalWorkSpecIds
  noUnresolvedGovernanceConflict : candidate.governanceConflicts = []

/--
Eccezione admin approvata valida come authorization locale per la sintesi
bottom-up, senza ampliare automaticamente la responsibility dell'user.
-/
def LocalGoalApprovedByException
    (exceptions : List (ApprovedLocalGoalException V X))
    («local» : LocalGoalContract V X) : Prop :=
  ∃ approved,
    approved ∈ exceptions ∧
    approved.«local» = «local»

/--
Authorization bottom-up: o il goal è integralmente coperto dalla responsibility
attiva oppure deriva da un'eccezione amministrativa esplicita.
-/
def LocalGoalAuthorizedForBottomUp
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    («local» : LocalGoalContract V X) : Prop :=
  LocalGoalCanContributeBottomUp «local» ∧
  ((∃ responsibility,
      responsibilities «local».controller = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility «local») ∨
   LocalGoalApprovedByException exceptions «local»)

/--
La revisione globale può essere automatica solo se tutte le sorgenti locali sono
già autorizzate dalla delega amministrativa o da un'eccezione approvata.
-/
structure AutomaticGlobalRevisionCertificate
    (globalMeaning : GlobalSynthesisSemantics V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (locals : LocalGoalDirectory V X)
    (candidate : GlobalContractCandidate V X) : Prop where
  synthesis : GlobalSynthesisCertificate locals candidate
  semanticAdequacy : GlobalSynthesisAdequacy globalMeaning locals candidate
  everySourceAuthorized :
    ∀ contribution «local»,
      contribution ∈ candidate.contributions →
      locals contribution.agent = some «local» →
      LocalGoalAuthorizedForBottomUp responsibilities exceptions «local»

/-- Origine della revisione globale autorevole. -/
inductive GlobalContractRevisionOrigin (V : Vocabulary) where
  | automaticDelegated
  | administratorEdited (administrator : V.PrincipalId)

/-- Snapshot append-only della revisione globale. -/
structure GlobalContractRevision
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  revision : Nat
  previousRevision : Option Nat
  contract : GoalContract V X
  origin : GlobalContractRevisionOrigin V

/--
Modifica diretta dell'administrator: deve essere amministratore del progetto dello
scope globale e il contratto finale deve restare strutturalmente valido.
-/
def AdministratorGlobalRevisionValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (revision : GlobalContractRevision V X)
    (administrator : V.PrincipalId) : Prop :=
  revision.origin = GlobalContractRevisionOrigin.administratorEdited administrator ∧
  GoalContractWellFormed revision.contract ∧
  ∃ «meta»,
    s.resources revision.contract.goal.scope = some «meta» ∧
    authorization.projectAdministrator s administrator «meta».projectId

/--
Una revisione globale automatica usa esattamente un candidate certificato e non
richiede una seconda approvazione dell'administrator per authority già delegata.
-/
def AutomaticGlobalRevisionValid
    (globalMeaning : GlobalSynthesisSemantics V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (locals : LocalGoalDirectory V X)
    (candidate : GlobalContractCandidate V X)
    (_certificate : AutomaticGlobalRevisionCertificate
      globalMeaning responsibilities exceptions locals candidate)
    (revision : GlobalContractRevision V X) : Prop :=
  revision.origin = GlobalContractRevisionOrigin.automaticDelegated ∧
  revision.revision = candidate.revision ∧
  revision.contract = candidate.contract

/-- Footprint minimo proposto per coprire nuovo work globale. -/
structure ProposedPermissionFootprint (V : Vocabulary) where
  resourceEffects : List (ResourceSecurityEffect V)
  tools : List V.Tool

/-- Equivalenza estensionale di due footprint proposti. -/
def PermissionFootprintEquivalent
    (left right : ProposedPermissionFootprint V) : Prop :=
  (∀ effect, effect ∈ left.resourceEffects ↔ effect ∈ right.resourceEffects) ∧
  (∀ tool, tool ∈ left.tools ↔ tool ∈ right.tools)

/-- Work globale introdotto/modificato che richiede un owner locale. -/
structure GlobalCoverageNeed (V : Vocabulary) where
  globalRevision : Nat
  obligation : V.ObligationId
  required : ProposedPermissionFootprint V

/--
Compatibilità pre-assignment di un agente esistente. Questa è una verifica di
permission/tool attuali e availability organizzativa; l'information-flow safety
R5.33 continua a essere verificata sugli effetti/context reali.
-/
def ExistingAgentCompatibleWithCoverageNeed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (agents : GovernedAgentDirectory V)
    (need : GlobalCoverageNeed V)
    (agent : V.PrincipalId) : Prop :=
  ∃ record,
    agents agent = some record ∧
    record.agent = agent ∧
    record.availability = AgentAvailabilityMode.projectDelegable ∧
    GovernedAgentRecordValid authorization s record ∧
    (∀ effect,
      effect ∈ need.required.resourceEffects →
      ResourceOperationAllowed authorization s agent effect.resource effect.operation) ∧
    (∀ tool,
      tool ∈ need.required.tools →
      authorization.toolAllowed s agent tool)

/-- Mandato locale proposto a un agente esistente in seguito a revisione globale. -/
structure GlobalMandateAssignment
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  globalRevision : Nat
  assignedBy : V.PrincipalId
  need : GlobalCoverageNeed V
  «local» : LocalGoalContract V X

/--
Un mandato globale può essere assegnato soltanto da administrator a un agente
project-delegable compatibile; il LocalGoalContract deve dichiarare provenance
globalMandate, così non rientra nel synthesizer bottom-up.
-/
def GlobalMandateAssignmentValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (agents : GovernedAgentDirectory V)
    (assignment : GlobalMandateAssignment V X) : Prop :=
  assignment.need.globalRevision = assignment.globalRevision ∧
  assignment.«local».origin = LocalGoalOrigin.globalMandate assignment.globalRevision ∧
  HasKind s assignment.assignedBy PrincipalKind.administrator ∧
  ExistingAgentCompatibleWithCoverageNeed
    authorization s agents assignment.need assignment.«local».agent

/-- Proposta di nuovo agente quando nessun agente esistente è adatto. -/
structure NewAgentForGlobalNeedProposal
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  proposedAgent : V.PrincipalId
  controller : V.PrincipalId
  need : GlobalCoverageNeed V
  prompt : V.SystemPrompt
  «local» : LocalGoalContract V X
  requested : ProposedPermissionFootprint V

/--
Least privilege della proposta: il footprint richiesto coincide estensionalmente
con quello calcolato per il need. Non significa che i grant siano già concessi.
-/
def NewAgentProposalLeastPrivilege
    (proposal : NewAgentForGlobalNeedProposal V X) : Prop :=
  PermissionFootprintEquivalent proposal.requested proposal.need.required

/--
La proposta di nuovo agente non è un grant. L'administrator/controller e il
permission engine concreto devono ancora autorizzare creation, tool e resource grant.
-/
def NewAgentForGlobalNeedProposalValid
    (compiler : ContractCompiler V X)
    (proposal : NewAgentForGlobalNeedProposal V X) : Prop :=
  proposal.«local».agent = proposal.proposedAgent ∧
  proposal.«local».controller = proposal.controller ∧
  proposal.«local».prompt = proposal.prompt ∧
  proposal.«local».origin = LocalGoalOrigin.globalMandate proposal.need.globalRevision ∧
  LocalGoalContractWellFormed proposal.«local» ∧
  LocalPromptCompiledContract compiler proposal.«local» ∧
  NewAgentProposalLeastPrivilege proposal

/--
Bozza di revisione prompt+goal. La bozza non è attiva; `baseRevision` identifica
la configurazione che continua a governare l'agente finché la nuova non passa
tutti i gate.
-/
structure LocalPromptGoalRevisionDraft
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  id : Nat
  agent : V.PrincipalId
  controller : V.PrincipalId
  baseRevision : Nat
  proposedPrompt : V.SystemPrompt
  proposedLocal : LocalGoalContract V X


/-- Scelte offerte al controller quando una bozza non è ancora autorizzabile. -/
inductive LocalPromptReviewDisposition where
  | rewrite
  | requestAdministratorReview
  deriving DecidableEq, Repr

/-- Scelta esplicita del controller; nessuna delle due opzioni attiva la bozza. -/
structure ControllerLocalPromptReviewChoice (V : Vocabulary) where
  draftId : Nat
  controller : V.PrincipalId
  disposition : LocalPromptReviewDisposition


/-- Coerenza puramente strutturale della bozza col compiler. -/
def LocalPromptGoalRevisionDraftWellFormed
    (compiler : ContractCompiler V X)
    (draft : LocalPromptGoalRevisionDraft V X) : Prop :=
  draft.proposedLocal.agent = draft.agent ∧
  draft.proposedLocal.controller = draft.controller ∧
  draft.proposedLocal.prompt = draft.proposedPrompt ∧
  draft.proposedLocal.supersedesRevision = some draft.baseRevision ∧
  LocalGoalContractWellFormed draft.proposedLocal ∧
  LocalPromptCompiledContract compiler draft.proposedLocal

/-- Approvazione finale del controller sulla versione ESATTA del prompt da attivare. -/
structure ControllerFinalPromptApproval (V : Vocabulary) where
  draftId : Nat
  agent : V.PrincipalId
  controller : V.PrincipalId
  prompt : V.SystemPrompt
  localRevision : Nat

/-- L'approvazione del controller deve corrispondere byte/identità alla bozza finale. -/
def ControllerApprovalMatchesDraft
    (draft : LocalPromptGoalRevisionDraft V X)
    (approval : ControllerFinalPromptApproval V) : Prop :=
  approval.draftId = draft.id ∧
  approval.agent = draft.agent ∧
  approval.controller = draft.controller ∧
  approval.prompt = draft.proposedPrompt ∧
  approval.localRevision = draft.proposedLocal.revision


/--
Dopo un'approvazione amministrativa, la versione editata diventa la nuova bozza
mostrata al controller: non si ritorna mai al prompt originario non conforme.
-/
def AdministratorFinalDraftBecomesControllerDraft
    (review : ResponsibilityExceptionReview V X)
    (adminDraft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V)
    (controllerDraft : LocalPromptGoalRevisionDraft V X) : Prop :=
  (decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalOnly ∨
   decision.mode = AdministratorResponsibilityDecisionMode.approvedGoalAndResponsibility) ∧
  decision.reviewId = review.id ∧
  adminDraft.reviewId = review.id ∧
  controllerDraft.agent = review.agent ∧
  controllerDraft.controller = review.user ∧
  controllerDraft.proposedPrompt = adminDraft.finalPrompt ∧
  controllerDraft.proposedLocal = adminDraft.finalLocal


/--
Authorization locale per una bozza finale. Il terzo ramo copre un mandato top-down
amministrativo già validato; i tre percorsi restano distinti in provenance.
-/
def LocalDraftAuthorized
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X) : Prop :=
  (∃ responsibility,
      responsibilities draft.controller = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility draft.proposedLocal) ∨
  LocalGoalApprovedByException exceptions draft.proposedLocal ∨
  (∃ assignment,
      assignment ∈ globalAssignments ∧
      assignment.«local» = draft.proposedLocal)

/--
Certificato necessario per rendere ATTIVA una nuova coppia prompt/local goal.
La semantica del prompt è esplicita e il controller approva la versione finale.
-/
structure LocalRevisionActivationCertificate
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X)
    (approval : ControllerFinalPromptApproval V) : Prop where
  draftWellFormed : LocalPromptGoalRevisionDraftWellFormed compiler draft
  classified : LocalGoalClassifiedBy classifier draft.proposedLocal
  classificationAdequate :
    LocalGoalClassificationAdequacy classificationMeaning classifier draft.proposedLocal
  promptAgreement : LocalPromptContractAgreement meaning compiler draft.proposedLocal
  finalControllerApproval : ControllerApprovalMatchesDraft draft approval
  authorized : LocalDraftAuthorized responsibilities exceptions globalAssignments draft

/--
Transizione atomica della configurazione visibile: prompt e LocalGoalContract
cambiano insieme. Questa forma non ricostruisce la revisione precedente; il
predicato `ActivateLocalRevisionAtomicallyFrom` sottostante la identifica.
-/
def ActivateLocalRevisionAtomically
    (beforePrompts afterPrompts : V.PrincipalId → Option V.SystemPrompt)
    (beforeLocals afterLocals : LocalGoalDirectory V X)
    (draft : LocalPromptGoalRevisionDraft V X) : Prop :=
  afterPrompts draft.agent = some draft.proposedPrompt ∧
  afterLocals draft.agent = some draft.proposedLocal ∧
  (∀ other,
    other ≠ draft.agent →
    afterPrompts other = beforePrompts other) ∧
  (∀ other,
    other ≠ draft.agent →
    afterLocals other = beforeLocals other)

/--
Predicato più stretto usato dai refinement concreti: la vecchia revisione viene
identificata esplicitamente senza ricostruire un record sintetico.
-/
def ActivateLocalRevisionAtomicallyFrom
    (beforePrompts afterPrompts : V.PrincipalId → Option V.SystemPrompt)
    (beforeLocals afterLocals : LocalGoalDirectory V X)
    (previous : LocalGoalContract V X)
    (draft : LocalPromptGoalRevisionDraft V X) : Prop :=
  previous.agent = draft.agent ∧
  previous.revision = draft.baseRevision ∧
  beforePrompts draft.agent = some previous.prompt ∧
  beforeLocals draft.agent = some previous ∧
  afterPrompts draft.agent = some draft.proposedPrompt ∧
  afterLocals draft.agent = some draft.proposedLocal ∧
  (∀ other,
    other ≠ draft.agent →
    afterPrompts other = beforePrompts other) ∧
  (∀ other,
    other ≠ draft.agent →
    afterLocals other = beforeLocals other)


/--
Effetto coordinato finale del ramo goal+responsibility. La nuova responsibility,
il prompt visibile e il LocalGoalContract diventano coerenti nello stesso boundary
operativo dopo la final approval del controller.
-/
def ApprovedGoalAndResponsibilityAtomicActivation
    (beforeResponsibilities afterResponsibilities : ResponsibilityDirectory V)
    (beforePrompts afterPrompts : V.PrincipalId → Option V.SystemPrompt)
    (beforeLocals afterLocals : LocalGoalDirectory V X)
    (previousLocal : LocalGoalContract V X)
    (review : ResponsibilityExceptionReview V X)
    (adminDraft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V)
    (controllerDraft : LocalPromptGoalRevisionDraft V X)
    (approval : ControllerFinalPromptApproval V) : Prop :=
  ApprovedGoalAndResponsibilityRevision
    beforeResponsibilities afterResponsibilities review adminDraft decision ∧
  AdministratorFinalDraftBecomesControllerDraft
    review adminDraft decision controllerDraft ∧
  ControllerApprovalMatchesDraft controllerDraft approval ∧
  ActivateLocalRevisionAtomicallyFrom
    beforePrompts afterPrompts beforeLocals afterLocals previousLocal controllerDraft


/--
Quando la bozza è fuori responsibility e non esiste exception/global mandate,
la nuova configurazione non ha un percorso di authorization.
-/
def LocalDraftRequiresRewriteOrEscalation
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X) : Prop :=
  ¬ LocalDraftAuthorized responsibilities exceptions globalAssignments draft

/--
Proposta di creazione agente da parte di user o administrator. La configurazione
iniziale deve già avere una coppia prompt/local goal coerente; non esiste un
agente operativo con prompt "più ampio" del proprio contratto.
-/
structure AgentCreationProposal
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  creator : V.PrincipalId
  proposedAgent : V.PrincipalId
  prompt : V.SystemPrompt
  «local» : LocalGoalContract V X
  availability : AgentAvailabilityMode

/-- Validità organizzativa della proposta di creazione. -/
def AgentCreationProposalValid
    (compiler : ContractCompiler V X)
    (s : State V)
    (proposal : AgentCreationProposal V X) : Prop :=
  (HasKind s proposal.creator PrincipalKind.user ∨
   HasKind s proposal.creator PrincipalKind.administrator) ∧
  s.principals proposal.proposedAgent = none ∧
  proposal.«local».agent = proposal.proposedAgent ∧
  proposal.«local».controller = proposal.creator ∧
  proposal.«local».prompt = proposal.prompt ∧
  LocalGoalContractWellFormed proposal.«local» ∧
  LocalPromptCompiledContract compiler proposal.«local»


/--
Atto append-only con cui un administrator autorizza la creazione iniziale
dell'esatto agente, prompt e LocalGoal descritti dalla proposta. Il record non
sostituisce compilation, classificazione, final prompt approval o permission
runtime: aggiunge soltanto il quarto percorso di authority organizzativa.
-/
structure ApprovedAdministratorAgentCreation
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  id : Nat
  administrator : V.PrincipalId
  proposal : AgentCreationProposal V X

/--
Validità dell'approvazione amministrativa diretta. L'authority è limitata al
progetto dello scope dell'esatto LocalGoal e la provenance locale riferisce
l'identificativo append-only dell'approvazione.
-/
def ApprovedAdministratorAgentCreationValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (approved : ApprovedAdministratorAgentCreation V X) : Prop :=
  approved.administrator = approved.proposal.creator ∧
  HasKind s approved.administrator PrincipalKind.administrator ∧
  approved.proposal.«local».origin =
    LocalGoalOrigin.administratorCreation approved.id ∧
  ∃ «meta»,
    s.resources approved.proposal.«local».contract.goal.scope = some «meta» ∧
    authorization.projectAdministrator
      s approved.administrator «meta».projectId

/-- L'approvazione persistita deve coincidere con l'intera proposta iniziale. -/
def AgentCreationApprovedByAdministrator
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (approvals : List (ApprovedAdministratorAgentCreation V X))
    (proposal : AgentCreationProposal V X) : Prop :=
  ∃ approved,
    approved ∈ approvals ∧
    approved.proposal = proposal ∧
    ApprovedAdministratorAgentCreationValid authorization s approved


/--
Creation activation: la proposta iniziale deve superare gli stessi gate semantici,
classificatori e di authorization di una revisione successiva. Il quarto ramo è
un atto amministrativo esplicito sull'esatta proposta, non la sola appartenenza
al ruolo administrator. La creazione di un agent non è un bypass dei gate di
compilation, final prompt approval o permission runtime.
-/
structure AgentCreationActivationCertificate
    (authorization : ProductAuthorizationProjection V)
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (s : State V)
    (proposal : AgentCreationProposal V X)
    (approval : ControllerFinalPromptApproval V) : Prop where
  proposalValid : AgentCreationProposalValid compiler s proposal
  classified : LocalGoalClassifiedBy classifier proposal.«local»
  classificationAdequate :
    LocalGoalClassificationAdequacy classificationMeaning classifier proposal.«local»
  promptAgreement : LocalPromptContractAgreement meaning compiler proposal.«local»
  controllerApproval :
    approval.agent = proposal.proposedAgent ∧
    approval.controller = proposal.creator ∧
    approval.prompt = proposal.prompt ∧
    approval.localRevision = proposal.«local».revision
  authorized :
    (∃ responsibility,
      responsibilities proposal.creator = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility proposal.«local») ∨
    LocalGoalApprovedByException exceptions proposal.«local» ∨
    (∃ assignment,
      assignment ∈ globalAssignments ∧
      assignment.«local» = proposal.«local») ∨
    AgentCreationApprovedByAdministrator
      authorization s administratorCreationApprovals proposal


/--
Stato governance aggiuntivo, separato da SemanticState per conservare il
refinement R4/R5 esistente. I prompt operativi continuano a vivere in State.
-/
structure ResponsibilityGovernanceState
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  responsibilities : ResponsibilityDirectory V
  localGoals : LocalGoalDirectory V X
  agents : GovernedAgentDirectory V
  approvedExceptions : List (ApprovedLocalGoalException V X)
  globalAssignments : List (GlobalMandateAssignment V X)
  administratorCreationApprovals :
    List (ApprovedAdministratorAgentCreation V X)
  escalationConsents : List (UserEscalationConsent V)
  exceptionReviews : List (ResponsibilityExceptionReview V X)
  administratorReviewDrafts : List (AdministratorResponsibilityReviewDraft V X)
  administratorDecisions : List (AdministratorResponsibilityDecision V)
  globalCandidates : List (GlobalContractCandidate V X)
  responsibilityHistory : List (ResponsibilityContract V)
  localGoalHistory : List (LocalGoalContract V X)
  globalRevisionHistory : List (GlobalContractRevision V X)
  activeGlobalRevision : Option (GlobalContractRevision V X)

/-- Run R5 osservata con il nuovo layer governance; proietta senza perdita alla run precedente. -/
structure ResponsibilityGovernedRun
    (V : Vocabulary)
    (X : ExtensionVocabulary V) extends ObservedSemanticRun V X where
  governanceState : Nat → ResponsibilityGovernanceState V X


/-- Le storie normative sono append-only lungo la run governata. -/
def ResponsibilityGovernanceHistoriesAppendOnly
    (run : ResponsibilityGovernedRun V X) : Prop :=
  ∀ n m,
    n ≤ m →
    (∀ responsibility,
      responsibility ∈ (run.governanceState n).responsibilityHistory →
      responsibility ∈ (run.governanceState m).responsibilityHistory) ∧
    (∀ «local»,
      «local» ∈ (run.governanceState n).localGoalHistory →
      «local» ∈ (run.governanceState m).localGoalHistory) ∧
    (∀ approved,
      approved ∈ (run.governanceState n).administratorCreationApprovals →
      approved ∈ (run.governanceState m).administratorCreationApprovals) ∧
    (∀ globalRevision,
      globalRevision ∈ (run.governanceState n).globalRevisionHistory →
      globalRevision ∈ (run.governanceState m).globalRevisionHistory)


/-- Tutte le responsibility attive sono versioni amministrativamente certificate. -/
structure ActiveResponsibilityDirectoryCertificate
    (authorization : ProductAuthorizationProjection V)
    (compiler : ResponsibilityCompiler V)
    (meaning : ResponsibilityTextSemantics V)
    (run : ResponsibilityGovernedRun V X) : Prop where
  activeValid :
    ∀ n user responsibility,
      (run.governanceState n).responsibilities user = some responsibility →
      responsibility.user = user ∧
      ResponsibilityActivationCertificate
        authorization compiler meaning (run.semanticState n).base responsibility


/-- Tutti i record agent attivi corrispondono al controller server-side reale. -/
structure ActiveGovernedAgentsCertificate
    (authorization : ProductAuthorizationProjection V)
    (run : ResponsibilityGovernedRun V X) : Prop where
  activeValid :
    ∀ n agent record,
      (run.governanceState n).agents agent = some record →
      record.agent = agent ∧
      GovernedAgentRecordValid authorization (run.semanticState n).base record

/-- Ogni eccezione attiva è ricostruibile dalla task, consenso, draft finale e decisione certificata. -/
structure ActiveApprovedExceptionsCertificate
    (authorization : ProductAuthorizationProjection V)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (run : ResponsibilityGovernedRun V X) : Prop where
  activeValid :
    ∀ n approved,
      approved ∈ (run.governanceState n).approvedExceptions →
      ∃ review consent adminDraft decision,
        review ∈ (run.governanceState n).exceptionReviews ∧
        consent ∈ (run.governanceState n).escalationConsents ∧
        adminDraft ∈ (run.governanceState n).administratorReviewDrafts ∧
        decision ∈ (run.governanceState n).administratorDecisions ∧
        ResponsibilityExceptionReviewValid (run.semanticState n).base review consent ∧
        ApprovedLocalGoalExceptionCertified
          authorization responsibilityCompiler responsibilityMeaning
          promptMeaning compiler classificationMeaning classifier
          (run.semanticState n).base review adminDraft decision approved

/-- Ogni mandato top-down presente nello stato è compatibile con l'agent directory e i permission correnti. -/
structure ActiveGlobalAssignmentsCertificate
    (authorization : ProductAuthorizationProjection V)
    (run : ResponsibilityGovernedRun V X) : Prop where
  activeValid :
    ∀ n assignment,
      assignment ∈ (run.governanceState n).globalAssignments →
      GlobalMandateAssignmentValid
        authorization
        (run.semanticState n).base
        (run.governanceState n).agents
        assignment

/--
Ogni approvazione amministrativa di creazione presente nello stato conserva
l'esatto record firmato e resta limitata al progetto amministrato.
-/
structure ActiveAdministratorAgentCreationApprovalsCertificate
    (authorization : ProductAuthorizationProjection V)
    (run : ResponsibilityGovernedRun V X) : Prop where
  activeValid :
    ∀ n approved,
      approved ∈
        (run.governanceState n).administratorCreationApprovals →
      ApprovedAdministratorAgentCreationValid
        authorization (run.semanticState n).base approved

/--
Authorization dell'attuale revisione globale: automatic bottom-up certificata
oppure modifica diretta di un administrator sul progetto dello scope globale.
-/
structure ActiveGlobalRevisionCertificate
    (authorization : ProductAuthorizationProjection V)
    (globalMeaning : GlobalSynthesisSemantics V X)
    (run : ResponsibilityGovernedRun V X) : Prop where
  activeAuthorized :
    ∀ n revision,
      (run.governanceState n).activeGlobalRevision = some revision →
      (revision.origin = GlobalContractRevisionOrigin.automaticDelegated ∧
        ∃ candidate,
          candidate ∈ (run.governanceState n).globalCandidates ∧
          candidate.revision = revision.revision ∧
          candidate.contract = revision.contract ∧
          AutomaticGlobalRevisionCertificate
            globalMeaning
            (run.governanceState n).responsibilities
            (run.governanceState n).approvedExceptions
            (run.governanceState n).localGoals
            candidate) ∨
      (∃ administrator,
        revision.origin = GlobalContractRevisionOrigin.administratorEdited administrator ∧
        AdministratorGlobalRevisionValid
          authorization (run.semanticState n).base revision administrator)




/-- Proiezione conservativa al modello R5 già usato dai theorem di completion/safety. -/
def ResponsibilityGovernedRun.toObservedRun
    (run : ResponsibilityGovernedRun V X) : ObservedSemanticRun V X :=
  run.toObservedSemanticRun

/--
Invariante fondamentale richiesto dal prodotto: se un LocalGoalContract è attivo,
il prompt visibile/operativo dello State è esattamente il suo prompt.
-/
def ActivePromptMatchesLocalGoal
    (run : ResponsibilityGovernedRun V X) : Prop :=
  ∀ n agent «local»,
    (run.governanceState n).localGoals agent = some «local» →
    (run.semanticState n).base.systemPrompts agent = some «local».prompt

/-- Ogni local goal attivo deve restare compilato dal proprio prompt attivo. -/
def ActiveLocalGoalCompiled
    (compiler : ContractCompiler V X)
    (run : ResponsibilityGovernedRun V X) : Prop :=
  ∀ n agent «local»,
    (run.governanceState n).localGoals agent = some «local» →
    LocalPromptCompiledContract compiler «local»


/-- Ogni LocalGoalContract attivo possiede un percorso di authorization osservabile. -/
def ActiveLocalGoalAuthorized
    (authorization : ProductAuthorizationProjection V)
    (run : ResponsibilityGovernedRun V X) : Prop :=
  ∀ n agent «local»,
    (run.governanceState n).localGoals agent = some «local» →
    (LocalDraftAuthorized
        (run.governanceState n).responsibilities
        (run.governanceState n).approvedExceptions
        (run.governanceState n).globalAssignments
        { id := «local».revision,
          agent := «local».agent,
          controller := «local».controller,
          baseRevision := «local».supersedesRevision.getD 0,
          proposedPrompt := «local».prompt,
          proposedLocal := «local» }) ∨
      (∃ approved,
        approved ∈
          (run.governanceState n).administratorCreationApprovals ∧
        approved.proposal.«local» = «local» ∧
        ApprovedAdministratorAgentCreationValid
          authorization (run.semanticState n).base approved)


/--
Certificate di allineamento visibile. La boundary di adeguatezza linguistica è
esplicita per ogni prompt locale attivo, come già in R5.30 per il prompt globale.
-/
structure ActivePromptLocalGoalConsistency
    (authorization : ProductAuthorizationProjection V)
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (run : ResponsibilityGovernedRun V X) : Prop where
  promptMatches : ActivePromptMatchesLocalGoal run
  compiled : ActiveLocalGoalCompiled compiler run
  classified :
    ∀ n agent «local»,
      (run.governanceState n).localGoals agent = some «local» →
      LocalGoalClassifiedBy classifier «local»
  classificationAdequate :
    ∀ n agent «local»,
      (run.governanceState n).localGoals agent = some «local» →
      LocalGoalClassificationAdequacy classificationMeaning classifier «local»
  authorized : ActiveLocalGoalAuthorized authorization run
  semanticallyAdequate :
    ∀ n agent «local»,
      (run.governanceState n).localGoals agent = some «local» →
      PromptContractAdequacy meaning compiler «local».prompt

/--
Bundle normativo del nuovo kernel di governance. Non sostituisce i certificate di
completion/safety: certifica soltanto la catena responsibility→local→global.
-/
structure ResponsibilityGovernanceKernelCertificate
    (authorization : ProductAuthorizationProjection V)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (globalMeaning : GlobalSynthesisSemantics V X)
    (run : ResponsibilityGovernedRun V X) : Prop where
  historiesAppendOnly : ResponsibilityGovernanceHistoriesAppendOnly run
  responsibilities :
    ActiveResponsibilityDirectoryCertificate
      authorization responsibilityCompiler responsibilityMeaning run
  agents : ActiveGovernedAgentsCertificate authorization run
  exceptions :
    ActiveApprovedExceptionsCertificate
      authorization responsibilityCompiler responsibilityMeaning
      promptMeaning compiler classificationMeaning classifier run
  assignments : ActiveGlobalAssignmentsCertificate authorization run
  administratorCreations :
    ActiveAdministratorAgentCreationApprovalsCertificate authorization run
  promptLocalConsistency :
    ActivePromptLocalGoalConsistency
      authorization promptMeaning compiler classificationMeaning classifier run
  globalRevision : ActiveGlobalRevisionCertificate authorization globalMeaning run



/--
Il goal globale attivo resta un normale GoalContract R5.30: il nuovo layer cambia
COME viene governato/sintetizzato, non il significato del completion kernel.
-/
def ActiveGlobalContractAt
    (run : ResponsibilityGovernedRun V X)
    (n : Nat)
    (contract : GoalContract V X) : Prop :=
  ∃ revision,
    (run.governanceState n).activeGlobalRevision = some revision ∧
    revision.contract = contract

/--
Responsibility governance e permission safety sono composibili: nessuna
responsibility/global revision sostituisce il certificate R5.33.
-/
structure ResponsibilityGovernanceSafetyBridge
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (governed : ResponsibilityGovernedRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat) : Prop where
  sameObservedBase : governed.toObservedRun = secured.certified.run
  authorityInformationSafety :
    AuthorityInformationSafetyHolds authorization secured contract runId start

/-! #### R5.35 — theorem di governance e trasparenza -/

/-- Una responsibility valida può essere emessa soltanto da un administrator. -/
theorem responsibility_contract_requires_administrator
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (responsibility : ResponsibilityContract V)
    (valid : ResponsibilityContractValid authorization s responsibility) :
    HasKind s responsibility.administrator PrincipalKind.administrator := by
  exact valid.1

/-- La copertura organizzativa non implica, né inventa, permission runtime. -/
theorem responsibility_and_runtime_permission_are_distinct
    (responsibility : ResponsibilityContract V)
    («local» : LocalGoalContract V X)
    (covered : ResponsibilityCoversLocalGoal responsibility «local») :
    ResponsibilityDoesNotImplyRuntimePermission responsibility «local» := by
  exact covered

/--
Se la bozza non ha alcun percorso di authorization, non può esistere un
activation certificate. Questo formalizza "riscrivi o escala" invece di
attivare silenziosamente solo la parte conforme.
-/
theorem nonconforming_local_draft_cannot_activate
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X)
    (approval : ControllerFinalPromptApproval V)
    (needsRewrite : LocalDraftRequiresRewriteOrEscalation
      responsibilities exceptions globalAssignments draft) :
    ¬ LocalRevisionActivationCertificate
        meaning compiler classificationMeaning classifier
        responsibilities exceptions globalAssignments draft approval := by
  intro certificate
  exact needsRewrite certificate.authorized

/-- Anche una bozza autorizzata non si attiva senza approvazione esatta del controller. -/
theorem unapproved_prompt_cannot_activate
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X)
    (approval : ControllerFinalPromptApproval V)
    (notApproved : ¬ ControllerApprovalMatchesDraft draft approval) :
    ¬ LocalRevisionActivationCertificate
        meaning compiler classificationMeaning classifier
        responsibilities exceptions globalAssignments draft approval := by
  intro certificate
  exact notApproved certificate.finalControllerApproval

/-- Ogni activation certificate rende il contratto locale esattamente compilato dal prompt approvato. -/
theorem activated_local_prompt_and_contract_are_aligned
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X)
    (approval : ControllerFinalPromptApproval V)
    (certificate : LocalRevisionActivationCertificate
      meaning compiler classificationMeaning classifier
      responsibilities exceptions globalAssignments draft approval) :
    draft.proposedLocal.prompt = draft.proposedPrompt ∧
    draft.proposedLocal.contract = compiler.compile draft.proposedPrompt ∧
    PromptContractAdequacy meaning compiler draft.proposedPrompt := by
  constructor
  · exact certificate.draftWellFormed.2.2.1
  · constructor
    · rw [← certificate.draftWellFormed.2.2.1]
      exact certificate.promptAgreement.1
    · rw [← certificate.draftWellFormed.2.2.1]
      exact certificate.promptAgreement.2

/-- Una approvazione amministrativa certificata riguarda la versione finale editata, non la proposta iniziale. -/
theorem administrator_approval_uses_final_edited_local_goal
    (authorization : ProductAuthorizationProjection V)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (s : State V)
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V)
    (approved : ApprovedLocalGoalException V X)
    (certified : ApprovedLocalGoalExceptionCertified
      authorization responsibilityCompiler responsibilityMeaning
      promptMeaning compiler classificationMeaning classifier
      s review draft decision approved) :
    approved.«local» = draft.finalLocal := by
  exact certified.2.2.2.2.1

/--
La creazione iniziale di un agent possiede uno dei tre percorsi preesistenti
oppure l'approvazione amministrativa esplicita dell'esatta proposta.
-/
theorem agent_creation_does_not_bypass_local_goal_authorization
    (authorization : ProductAuthorizationProjection V)
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (s : State V)
    (proposal : AgentCreationProposal V X)
    (approval : ControllerFinalPromptApproval V)
    (certificate : AgentCreationActivationCertificate
      authorization meaning compiler classificationMeaning classifier
      responsibilities exceptions globalAssignments
      administratorCreationApprovals s proposal approval) :
    (∃ responsibility,
      responsibilities proposal.creator = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility proposal.«local») ∨
    LocalGoalApprovedByException exceptions proposal.«local» ∨
    (∃ assignment,
      assignment ∈ globalAssignments ∧ assignment.«local» = proposal.«local») ∨
    AgentCreationApprovedByAdministrator
      authorization s administratorCreationApprovals proposal := by
  exact certificate.authorized

/-- Il quarto ramo esibisce sempre un record append-only sull'esatta proposta. -/
theorem administrator_agent_creation_uses_exact_approval
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (approvals : List (ApprovedAdministratorAgentCreation V X))
    (proposal : AgentCreationProposal V X)
    (authorized : AgentCreationApprovedByAdministrator
      authorization s approvals proposal) :
    ∃ approved,
      approved ∈ approvals ∧
      approved.proposal = proposal ∧
      ApprovedAdministratorAgentCreationValid authorization s approved := by
  exact authorized

/-- La storia delle approvazioni amministrative di creazione è append-only. -/
theorem administrator_agent_creation_approval_history_is_append_only
    (run : ResponsibilityGovernedRun V X)
    (histories : ResponsibilityGovernanceHistoriesAppendOnly run)
    (n m : Nat)
    (before : n ≤ m)
    (approved : ApprovedAdministratorAgentCreation V X)
    (present :
      approved ∈
        (run.governanceState n).administratorCreationApprovals) :
    approved ∈
      (run.governanceState m).administratorCreationApprovals := by
  exact (histories n m before).2.2.1 approved present

/-- Il record esatto identifica un creator administrator del progetto dello scope. -/
theorem administrator_agent_creation_has_authorized_creator
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (approvals : List (ApprovedAdministratorAgentCreation V X))
    (proposal : AgentCreationProposal V X)
    (authorized : AgentCreationApprovedByAdministrator
      authorization s approvals proposal) :
    ∃ approved «meta»,
      approved ∈ approvals ∧
      approved.proposal = proposal ∧
      approved.administrator = proposal.creator ∧
      HasKind s proposal.creator PrincipalKind.administrator ∧
      s.resources proposal.«local».contract.goal.scope = some «meta» ∧
      authorization.projectAdministrator
        s proposal.creator «meta».projectId := by
  rcases authorized with ⟨approved, present, exactProposal, valid⟩
  rcases valid with
    ⟨administratorCreator, administratorKind, origin, «meta», scope, projectAdmin⟩
  subst proposal
  rw [administratorCreator] at administratorKind projectAdmin
  exact ⟨approved, «meta», present, rfl, administratorCreator,
    administratorKind, scope, projectAdmin⟩

/-- La sola membership projectAdministrator non crea il record richiesto. -/
theorem project_administrator_alone_cannot_approve_agent_creation
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (proposal : AgentCreationProposal V X)
    (projectId : V.ProjectId)
    (_projectAdmin :
      authorization.projectAdministrator s proposal.creator projectId) :
    ¬ AgentCreationApprovedByAdministrator authorization s [] proposal := by
  intro authorized
  rcases authorized with ⟨approved, present, _⟩
  simp at present

/-- Un'approvazione non può essere riutilizzata per una proposta differente. -/
theorem administrator_agent_creation_approval_is_proposal_exact
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (approved : ApprovedAdministratorAgentCreation V X)
    (other : AgentCreationProposal V X)
    (different : approved.proposal ≠ other) :
    ¬ AgentCreationApprovedByAdministrator authorization s [approved] other := by
  intro authorized
  rcases authorized with ⟨witness, present, exactProposal, _⟩
  simp only [List.mem_singleton] at present
  subst witness
  exact different exactProposal

/-- Il ramo Responsibility preesistente resta disponibile senza variazioni. -/
theorem responsibility_agent_creation_authorization_is_preserved
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (proposal : AgentCreationProposal V X)
    (covered : ∃ responsibility,
      responsibilities proposal.creator = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility proposal.«local») :
    (∃ responsibility,
      responsibilities proposal.creator = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility proposal.«local») ∨
    LocalGoalApprovedByException exceptions proposal.«local» ∨
    (∃ assignment,
      assignment ∈ globalAssignments ∧ assignment.«local» = proposal.«local») ∨
    AgentCreationApprovedByAdministrator
      authorization s administratorCreationApprovals proposal := by
  exact Or.inl covered

/-- Il ramo exception preesistente resta disponibile senza variazioni. -/
theorem exception_agent_creation_authorization_is_preserved
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (proposal : AgentCreationProposal V X)
    (approved : LocalGoalApprovedByException exceptions proposal.«local») :
    (∃ responsibility,
      responsibilities proposal.creator = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility proposal.«local») ∨
    LocalGoalApprovedByException exceptions proposal.«local» ∨
    (∃ assignment,
      assignment ∈ globalAssignments ∧ assignment.«local» = proposal.«local») ∨
    AgentCreationApprovedByAdministrator
      authorization s administratorCreationApprovals proposal := by
  exact Or.inr (Or.inl approved)

/-- Il ramo global mandate preesistente resta disponibile senza variazioni. -/
theorem global_mandate_agent_creation_authorization_is_preserved
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (proposal : AgentCreationProposal V X)
    (assigned : ∃ assignment,
      assignment ∈ globalAssignments ∧ assignment.«local» = proposal.«local») :
    (∃ responsibility,
      responsibilities proposal.creator = some responsibility ∧
      ResponsibilityCoversLocalGoal responsibility proposal.«local») ∨
    LocalGoalApprovedByException exceptions proposal.«local» ∨
    (∃ assignment,
      assignment ∈ globalAssignments ∧ assignment.«local» = proposal.«local») ∨
    AgentCreationApprovedByAdministrator
      authorization s administratorCreationApprovals proposal := by
  exact Or.inr (Or.inr (Or.inl assigned))

/-- Un rifiuto amministrativo non può materializzare un'eccezione locale approvata. -/
theorem rejected_review_cannot_authorize_local_exception
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V)
    (approved : ApprovedLocalGoalException V X)
    (rejected : decision.mode = AdministratorResponsibilityDecisionMode.rejected) :
    ¬ ApprovedLocalGoalExceptionValid authorization s review draft decision approved := by
  intro valid
  rcases valid.2.1 with goalOnly | goalAndResponsibility
  · rw [rejected] at goalOnly
    contradiction
  · rw [rejected] at goalAndResponsibility
    contradiction

/-- L'approvazione goal-only lascia immutata la responsibility attiva. -/
theorem approved_goal_only_does_not_expand_user_responsibility
    (before after : ResponsibilityDirectory V)
    (review : ResponsibilityExceptionReview V X)
    (decision : AdministratorResponsibilityDecision V)
    (preserves : ApprovedGoalOnlyPreservesResponsibility before after review decision) :
    after review.user = before review.user := by
  exact preserves.2.2

/-- L'attivazione concreta aggiorna prompt e LocalGoalContract nella stessa transizione. -/
theorem local_prompt_and_goal_revision_activate_together
    (beforePrompts afterPrompts : V.PrincipalId → Option V.SystemPrompt)
    (beforeLocals afterLocals : LocalGoalDirectory V X)
    (previous : LocalGoalContract V X)
    (draft : LocalPromptGoalRevisionDraft V X)
    (activation : ActivateLocalRevisionAtomicallyFrom
      beforePrompts afterPrompts beforeLocals afterLocals previous draft) :
    afterPrompts draft.agent = some draft.proposedPrompt ∧
    afterLocals draft.agent = some draft.proposedLocal := by
  exact ⟨activation.2.2.2.2.1, activation.2.2.2.2.2.1⟩

/--
Un mandato derivato dal globale non può essere riutilizzato come nuova sorgente
bottom-up: il feedback global→local→global è chiuso per costruzione.
-/
theorem global_derived_local_goal_does_not_resynthesize
    («local» : LocalGoalContract V X)
    (revision : Nat)
    (origin : «local».origin = LocalGoalOrigin.globalMandate revision) :
    ¬ LocalGoalCanContributeBottomUp «local» := by
  unfold LocalGoalCanContributeBottomUp
  rw [origin]
  simp

/-- Una creazione amministrativa diretta non diventa una sorgente bottom-up. -/
theorem administrator_created_local_goal_does_not_resynthesize
    («local» : LocalGoalContract V X)
    (approvalId : Nat)
    (origin : «local».origin =
      LocalGoalOrigin.administratorCreation approvalId) :
    ¬ LocalGoalCanContributeBottomUp «local» := by
  unfold LocalGoalCanContributeBottomUp
  rw [origin]
  simp

/-- Ogni sorgente di una revisione automatica globale è già autorizzata localmente. -/
theorem automatic_global_revision_uses_authorized_local_sources
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (locals : LocalGoalDirectory V X)
    (candidate : GlobalContractCandidate V X)
    (globalMeaning : GlobalSynthesisSemantics V X)
    (certificate : AutomaticGlobalRevisionCertificate
      globalMeaning responsibilities exceptions locals candidate)
    (contribution : GlobalLocalContribution V)
    («local» : LocalGoalContract V X)
    (contributionIn : contribution ∈ candidate.contributions)
    (source : locals contribution.agent = some «local») :
    LocalGoalAuthorizedForBottomUp responsibilities exceptions «local» := by
  exact certificate.everySourceAuthorized contribution «local» contributionIn source

/-- La proposta di nuovo agente non può chiedere un footprint più ampio del need certificato. -/
theorem new_agent_global_need_proposal_is_least_privilege
    (compiler : ContractCompiler V X)
    (proposal : NewAgentForGlobalNeedProposal V X)
    (valid : NewAgentForGlobalNeedProposalValid compiler proposal) :
    PermissionFootprintEquivalent proposal.requested proposal.need.required := by
  exact valid.2.2.2.2.2.2

/-- Un agente esistente selezionabile per nuovo work globale deve essere project-delegable. -/
theorem existing_global_candidate_is_project_delegable
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (agents : GovernedAgentDirectory V)
    (need : GlobalCoverageNeed V)
    (agent : V.PrincipalId)
    (compatible : ExistingAgentCompatibleWithCoverageNeed authorization s agents need agent) :
    ∃ record,
      agents agent = some record ∧
      record.availability = AgentAvailabilityMode.projectDelegable := by
  rcases compatible with ⟨record, inDirectory, identity, availability, valid, effects, tools⟩
  exact ⟨record, inDirectory, availability⟩

/-- Il layer governance proietta conservativamente alla stessa ObservedSemanticRun R5. -/
theorem responsibility_governance_projects_to_existing_r5
    (run : ResponsibilityGovernedRun V X) :
    run.toObservedRun = run.toObservedSemanticRun := by
  rfl

/--
Se il certificate di consistenza è disponibile, leggere il prompt attivo non può
mostrare una versione diversa dal prompt del LocalGoalContract attivo.
-/
theorem active_agent_prompt_is_exact_local_contract_prompt
    (authorization : ProductAuthorizationProjection V)
    (meaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (run : ResponsibilityGovernedRun V X)
    (consistency : ActivePromptLocalGoalConsistency
      authorization meaning compiler classificationMeaning classifier run)
    (n : Nat)
    (agent : V.PrincipalId)
    («local» : LocalGoalContract V X)
    (active : (run.governanceState n).localGoals agent = some «local») :
    (run.semanticState n).base.systemPrompts agent = some «local».prompt ∧
    «local».contract = compiler.compile «local».prompt ∧
    PromptContractAdequacy meaning compiler «local».prompt := by
  constructor
  · exact consistency.promptMatches n agent «local» active
  · constructor
    · exact consistency.compiled n agent «local» active
    · exact consistency.semanticallyAdequate n agent «local» active

/-- Il kernel di governance rende l'origine dell'attuale global revision esplicitamente autorizzata. -/
theorem active_global_revision_has_governance_authorization
    (authorization : ProductAuthorizationProjection V)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (compiler : ContractCompiler V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (classifier : LocalGoalClassifier V X)
    (globalMeaning : GlobalSynthesisSemantics V X)
    (run : ResponsibilityGovernedRun V X)
    (kernel : ResponsibilityGovernanceKernelCertificate
      authorization responsibilityCompiler responsibilityMeaning
      promptMeaning compiler classificationMeaning classifier globalMeaning run)
    (n : Nat)
    (revision : GlobalContractRevision V X)
    (active : (run.governanceState n).activeGlobalRevision = some revision) :
    (revision.origin = GlobalContractRevisionOrigin.automaticDelegated ∧
      ∃ candidate,
        candidate ∈ (run.governanceState n).globalCandidates ∧
        candidate.revision = revision.revision ∧
        candidate.contract = revision.contract ∧
        AutomaticGlobalRevisionCertificate
          globalMeaning
          (run.governanceState n).responsibilities
          (run.governanceState n).approvedExceptions
          (run.governanceState n).localGoals
          candidate) ∨
    (∃ administrator,
      revision.origin = GlobalContractRevisionOrigin.administratorEdited administrator ∧
      AdministratorGlobalRevisionValid
        authorization (run.semanticState n).base revision administrator) := by
  exact kernel.globalRevision.activeAuthorized n revision active

/--
Il completion theorem matematico esistente può essere applicato direttamente al
GoalContract globale attivo: non serve reintrodurre un finto prompt globale.
-/
theorem responsibility_governed_global_completion_from_existing_kernel
    (measure : ProgressMeasure V X)
    (run : ResponsibilityGovernedRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (_active : ActiveGlobalContractAt run start contract)
    (validity :
      GoalValidityPersistsAfter
        run.toObservedRun contract.goal.id start)
    (progress :
      CollaborativeContractLocalProgressAfter
        measure run.toObservedRun runId contract start)
    (validStart :
      GoalValid (run.semanticState start) contract.goal.id) :
    EventuallyCollaborativeContractCompleted
      run.toObservedRun runId contract start := by
  exact collaborative_contract_completion_from_well_founded_progress_after
    measure run.toObservedRun runId contract start validity progress validStart

/--
Corollario di composizione: la nuova governance non sostituisce la safety R5.33;
la stessa run osservata conserva il theorem authority/information già certificato.
-/
theorem responsibility_governance_preserves_authority_information_boundary
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (governed : ResponsibilityGovernedRun V X)
    (contract : GoalContract V X)
    (runId : X.RunId)
    (start : Nat)
    (bridge : ResponsibilityGovernanceSafetyBridge
      authorization secured governed contract runId start) :
    governed.toObservedRun = secured.certified.run ∧
    AuthorityInformationSafetyHolds authorization secured contract runId start := by
  exact ⟨bridge.sameObservedBase, bridge.authorityInformationSafety⟩

/-
REFINEMENT NOTE R5.35

Il prodotto concreto deve realizzare questo layer come dati/versioni persistenti,
non come campi affidati al modello:
* ResponsibilityContract revisionato solo da administrator e derivato da un testo
  umano leggibile mediante ResponsibilityCompiler + validator strutturale;
* LocalGoalClassifier deterministico: `domain`/scope usati dal responsibility gate
  non possono essere metadata arbitrari dichiarati dal client o dal modello;
* LocalGoalContract persistito per agente, con active revision distinta dalle draft;
* creazione iniziale diretta da administrator soltanto tramite
  `ApprovedAdministratorAgentCreation` append-only legata all'esatta proposta;
  il solo ruolo administrator non autorizza la creazione e questa provenance non
  diventa automaticamente una sorgente bottom-up;
* prompt revision e LocalGoalContract revision attivati nello stesso commit/fencing
  boundary; non deve esistere una nuova versione del prompt attiva senza la stessa
  revisione locale;
* UI controller: se il draft eccede responsibility, mostra summary e propone
  riscrittura; nessuna parte della nuova revisione viene attivata parzialmente;
* escalation soltanto dopo consenso esplicito dell'user e tramite task reale
  assegnata all'administrator;
* UI administrator: tre decisioni `rejected`, `approvedGoalOnly`,
  `approvedGoalAndResponsibility`; testo prompt/local goal e proposta responsibility
  sono editabili, ma la decisione firma/riferisce la revisione finale esatta;
* dopo editing admin, ricompilare, riclassificare e rivalidare la VERSIONE FINALE
  prima che `AdministratorResponsibilityApprovalCertificate` possa esistere;
* dopo decisione admin, il controller vede e approva il prompt finale esatto;
* `approvedGoalAndResponsibility` deve applicare responsibility revision e local
  goal authorization atomicamente lato governance; i permission grant restano una
  transazione/policy separata;
* synthesizer globale conserva provenance LocalGoalClause→global WorkSpec e non
  reimporta `LocalGoalOrigin.globalMandate` come nuova sorgente;
* auto-revision globale ammessa solo senza governance conflict, con GlobalSynthesisAdequacy
  esplicita e con tutte le source già coperte da responsibility o exception approvata;
* nuove obligation introdotte direttamente dall'administrator producono coverage
  need; la UI propone agenti project-delegable realmente compatibili oppure un
  nuovo agente con footprint minimo;
* la proposal di footprint NON concede permission: Rust/RLS/E2EE/tool policy restano
  gli enforcement point concreti;
* ogni esecuzione deve costruire `ResponsibilityGovernanceKernelCertificate` oltre
  ai certificate R5.30 e R5.33/R5.34;
* il completion globale riusa il theorem contract-native R5.30 direttamente sul
  GoalContract globale attivo, senza richiedere un unico prompt globale fittizio.

Boundary residue:
* la traduzione linguistica ResponsibilityText→ResponsibilityRule e
  SystemPrompt→GoalContract conserva una componente semantica non derivabile dalla
  sola sintassi; come in R5.30 va isolata in adequacy/validation, non nascosta;
* la sintesi globale può usare AI per proporre dependency/conflitti, ma provenance,
  well-formedness, authorization delle source e absence-of-feedback sono target
  deterministici del refinement concreto.
-/

/-! ### R5.36 — UserProxy chat isolate, interrogazione read-only e task cross-owner -/

/-
Questa sezione SUPERA la modellazione precedente di `contextualChat` come
`AgentInteractionMode` del work autonomo. Il sink `DisclosureSink.contextualChat`
resta riusabile come destinazione privata, ma la chat utente non appartiene al
work graph, non ha LocalGoalContract e non possiede authority propria.

Principi normativi aggiuntivi:
1. ogni principal umano attivo possiede un UserProxy di sistema, non un nuovo
   PrincipalId autonomo; il proxy deriva in tempo reale visibility/authority
   dall'utente e non può amplificarle;
2. sullo stesso UserProxy possono esistere più chat thread; ogni transcript è
   leggibile soltanto dal creator del thread;
3. il UserProxy non ha SystemPrompt normativo, LocalGoalContract, obligation,
   work item, claim, scheduler position o contributo bottom-up al goal globale;
4. una operazione della chat tecnicamente permessa ma fuori ResponsibilityContract
   richiede conferma esplicita dell'utente; una permission negata non può essere
   superata dalla conferma;
5. il proxy può controllare agenti del proprio controller; un administrator può
   controllare tutti gli agenti dei progetti che amministra;
6. il controllo consente interrogazione, task assignment e prompt revision, ma
   prompt e LocalGoalContract restano atomici e semanticamente allineati;
7. qualunque principal umano o agent può interrogare un altro agent, ma
   l'interrogazione è answer-only: non può produrre side effect sul target;
8. per un agent initiator l'interrogazione avviene mediante un Tool dedicato;
9. il transcript dell'interrogazione è privato al creator: il controller del
   target non lo eredita e non lo vede soltanto perché controlla il target;
10. l'answer non può rivelare source non leggibili dal creator dell'interrogazione;
11. per una task assegnata da un user non-administrator a un agent controllato da
    un altro principal: se la task rientra già nelle obligation richieste del
    LocalGoalContract attivo viene accettata; altrimenti, se il task intent rientra
    nella ResponsibilityContract del controller del target, viene creata una task
    di review assegnata a quel controller; altrimenti la richiesta è rifiutata;
12. l'approvazione cross-owner non può introdurre lavoro nascosto: prima della
    assegnazione effettiva il prompt e il LocalGoalContract del target devono essere
    revisionati/attivati affinché la task rientri nelle obligation dichiarate.
-/

/--
Il proxy non è un PrincipalId: è una capability di interazione legata a un umano.
Questa scelta impedisce per costruzione ACL indipendenti che possano divergere
rispetto all'utente dopo revoche o cambi di membership.
-/
structure UserProxyAgent (V : Vocabulary) where
  id : Nat
  user : V.PrincipalId

/-- Un UserProxy esiste soltanto per un principal umano. -/
def UserProxyValid
    (s : State V)
    (proxy : UserProxyAgent V) : Prop :=
  ∃ kind,
    HasKind s proxy.user kind ∧
    IsHumanKind kind

/-- Directory product-side: un unico proxy di sistema per ogni umano. -/
abbrev UserProxyDirectory (V : Vocabulary) :=
  V.PrincipalId → Option (UserProxyAgent V)

/--
Invariante di default provisioning: ogni umano ha esattamente il proxy indicizzato
con il proprio principal e nessun non-umano compare nella directory.
-/
def UserProxyDefaultProvisioning
    (s : State V)
    (directory : UserProxyDirectory V) : Prop :=
  (∀ user kind,
      HasKind s user kind →
      IsHumanKind kind →
      ∃ proxy,
        directory user = some proxy ∧
        proxy.user = user ∧
        UserProxyValid s proxy) ∧
  (∀ user proxy,
      directory user = some proxy →
      proxy.user = user ∧
      UserProxyValid s proxy)

/-- Più thread possono vivere sullo stesso proxy; non esiste un singleton-thread. -/
structure UserProxyChatThread (V : Vocabulary) where
  id : Nat
  proxyId : Nat
  creator : V.PrincipalId
  createdAt : Nat

/-- Un thread umano appartiene esattamente al creator/proprietario del proxy. -/
def UserProxyChatThreadValid
    (proxy : UserProxyAgent V)
    (thread : UserProxyChatThread V) : Prop :=
  thread.proxyId = proxy.id ∧
  thread.creator = proxy.user

/-- Confidentiality del transcript: solo il creator umano del thread. -/
def UserProxyChatReadableBy
    (thread : UserProxyChatThread V)
    (principal : V.PrincipalId) : Prop :=
  principal = thread.creator

@[simp] theorem user_proxy_chat_only_creator_reads
    (thread : UserProxyChatThread V)
    (principal : V.PrincipalId) :
    UserProxyChatReadableBy thread principal ↔ principal = thread.creator := by
  rfl

/-- Il proxy non possiede authority: usa direttamente l'authorization del suo user. -/
def UserProxyResourceOperationAllowed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (resource : V.ResourceId)
    (operation : ResourceOperation) : Prop :=
  ResourceOperationAllowed authorization s proxy.user resource operation

/-- Anche i tool della chat sono quelli permessi all'utente, non al proxy. -/
def UserProxyToolAllowed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (tool : V.Tool) : Prop :=
  authorization.toolAllowed s proxy.user tool

@[simp] theorem user_proxy_authority_exactly_matches_user
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (resource : V.ResourceId)
    (operation : ResourceOperation) :
    UserProxyResourceOperationAllowed authorization s proxy resource operation ↔
      ResourceOperationAllowed authorization s proxy.user resource operation := by
  rfl

/-- Intent di una singola operazione supervisionata via chat. -/
structure UserProxyActionIntent (V : Vocabulary) where
  id : Nat
  domain : Nat
  scope : V.ResourceId
  requiredActions : List AgentActionClass
  resourceEffects : List (ResourceSecurityEffect V)
  tools : List V.Tool

/-- La responsibility organizzativa copre l'intent corrente della chat. -/
def UserProxyIntentWithinResponsibility
    (responsibilities : ResponsibilityDirectory V)
    (proxy : UserProxyAgent V)
    (intent : UserProxyActionIntent V) : Prop :=
  ∃ responsibility rule,
    responsibilities proxy.user = some responsibility ∧
    rule ∈ responsibility.rules ∧
    rule.domain = intent.domain ∧
    rule.scope = intent.scope ∧
    ∀ actionClass,
      actionClass ∈ intent.requiredActions →
      actionClass ∈ rule.allowedActions

/-- Runtime permissions della singola operazione mediata. -/
def UserProxyIntentRuntimeAllowed
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (intent : UserProxyActionIntent V) : Prop :=
  (∀ effect,
      effect ∈ intent.resourceEffects →
      ResourceOperationAllowed
        authorization s proxy.user effect.resource effect.operation) ∧
  (∀ tool,
      tool ∈ intent.tools →
      authorization.toolAllowed s proxy.user tool)

/-- Conferma esplicita one-shot per un intent fuori responsibility ma dentro ACL. -/
structure UserProxyResponsibilityConfirmation (V : Vocabulary) where
  user : V.PrincipalId
  chatId : Nat
  intentId : Nat
  confirmedAt : Nat

/-- La conferma deve riferirsi esattamente allo user e all'intent corrente. -/
def UserProxyConfirmationMatches
    (proxy : UserProxyAgent V)
    (thread : UserProxyChatThread V)
    (intent : UserProxyActionIntent V)
    (confirmation : UserProxyResponsibilityConfirmation V) : Prop :=
  confirmation.user = proxy.user ∧
  confirmation.chatId = thread.id ∧
  confirmation.intentId = intent.id

/--
Una operazione proxy è eseguibile solo se passa sempre i permission gate; la
responsibility decide soltanto se serve una conferma supervisionata one-shot.
-/
def UserProxyIntentExecutable
    (authorization : ProductAuthorizationProjection V)
    (responsibilities : ResponsibilityDirectory V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (thread : UserProxyChatThread V)
    (intent : UserProxyActionIntent V)
    (confirmation : Option (UserProxyResponsibilityConfirmation V)) : Prop :=
  UserProxyChatThreadValid proxy thread ∧
  UserProxyIntentRuntimeAllowed authorization s proxy intent ∧
  (UserProxyIntentWithinResponsibility responsibilities proxy intent ∨
    ∃ approved,
      confirmation = some approved ∧
      UserProxyConfirmationMatches proxy thread intent approved)

/-- Una conferma organizzativa non può rendere vero un permission gate falso. -/
theorem user_proxy_confirmation_cannot_bypass_permissions
    (authorization : ProductAuthorizationProjection V)
    (responsibilities : ResponsibilityDirectory V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (thread : UserProxyChatThread V)
    (intent : UserProxyActionIntent V)
    (confirmation : Option (UserProxyResponsibilityConfirmation V))
    (executable : UserProxyIntentExecutable
      authorization responsibilities s proxy thread intent confirmation) :
    UserProxyIntentRuntimeAllowed authorization s proxy intent := by
  exact executable.2.1

/--
Projection product-side necessaria per stabilire su quale progetto vive un agent.
Serve soltanto al controllo amministrativo; non concede authority di per sé.
-/
structure AgentControlProjection (V : Vocabulary) where
  agentProject : V.PrincipalId → Option V.ProjectId

/--
Un user controlla gli agenti di cui è controller; un administrator controlla gli
agenti dei progetti che amministra. Il controllo non modifica le ACL delle risorse.
-/
def PrincipalControlsAgent
    (authorization : ProductAuthorizationProjection V)
    (control : AgentControlProjection V)
    (s : State V)
    (agents : GovernedAgentDirectory V)
    (principal agent : V.PrincipalId) : Prop :=
  ∃ record,
    agents agent = some record ∧
    record.agent = agent ∧
    GovernedAgentRecordValid authorization s record ∧
    (record.controller = principal ∨
      ∃ project,
        control.agentProject agent = some project ∧
        HasKind s principal PrincipalKind.administrator ∧
        authorization.projectAdministrator s principal project)

/--
R5.36 generalizza la firma finale R5.35: il controller può sempre approvare la
propria configurazione; un administrator che controlla il project agent può
approvare esplicitamente una revisione amministrativa. L'active prompt resta
comunque accoppiato atomicamente al LocalGoalContract finale.
-/
structure AuthorizedFinalPromptApproval (V : Vocabulary) where
  agent : V.PrincipalId
  approver : V.PrincipalId
  prompt : V.SystemPrompt
  localRevision : Nat
  approvedAt : Nat

/-- Validità dell'approver finale rispetto al controllo dell'agent. -/
def AuthorizedFinalPromptApprovalValid
    (authorization : ProductAuthorizationProjection V)
    (control : AgentControlProjection V)
    (s : State V)
    (agents : GovernedAgentDirectory V)
    («local» : LocalGoalContract V X)
    (approval : AuthorizedFinalPromptApproval V) : Prop :=
  approval.agent = «local».agent ∧
  approval.prompt = «local».prompt ∧
  approval.localRevision = «local».revision ∧
  PrincipalControlsAgent
    authorization control s agents approval.approver «local».agent

/-! #### R5.36A — Interrogazione read-only tra principal e agent -/

/-- Tool concreto/astratto dedicato all'interrogazione agent→agent. -/
structure AgentInterrogationToolBinding (V : Vocabulary) where
  tool : V.Tool

/-- Sessione privata di interrogazione; il creator è l'unico reader del transcript. -/
structure AgentInterrogationSession (V : Vocabulary) where
  id : Nat
  creator : V.PrincipalId
  targetAgent : V.PrincipalId
  createdAt : Nat
  viaToolCall : Option V.ToolCallId

/-- Una agent initiation deve essere materializzata tramite il tool dedicato. -/
def AgentInterrogationToolCallValid
    (binding : AgentInterrogationToolBinding V)
    (s : State V)
    (creator : V.PrincipalId)
    (callId : V.ToolCallId) : Prop :=
  ∃ call,
    s.toolCalls callId = some call ∧
    call.owner = creator ∧
    call.tool = binding.tool

/--
Human user/admin possono iniziare dal proprio proxy senza ToolCall; un agent deve
invece usare la tool call dedicata. Non è richiesto controllo sul target per
porre domande, ma questo non concede side effect né disclosure aggiuntiva.
-/
def AgentInterrogationSessionValid
    (binding : AgentInterrogationToolBinding V)
    (s : State V)
    (session : AgentInterrogationSession V) : Prop :=
  HasKind s session.targetAgent PrincipalKind.agent ∧
  session.creator ≠ session.targetAgent ∧
  match s.principals session.creator with
  | some PrincipalKind.administrator => session.viaToolCall = none
  | some PrincipalKind.user => session.viaToolCall = none
  | some PrincipalKind.agent =>
      ∃ callId,
        session.viaToolCall = some callId ∧
        AgentInterrogationToolCallValid binding s session.creator callId
  | none => False

/-- Transcript privato: il controller del target non eredita la conversazione. -/
def AgentInterrogationTranscriptReadableBy
    (session : AgentInterrogationSession V)
    (principal : V.PrincipalId) : Prop :=
  principal = session.creator

@[simp] theorem interrogation_transcript_only_creator_reads
    (session : AgentInterrogationSession V)
    (principal : V.PrincipalId) :
    AgentInterrogationTranscriptReadableBy session principal ↔
      principal = session.creator := by
  rfl

/-- Se il controller del target è diverso dal creator non può leggere il transcript. -/
theorem target_controller_does_not_inherit_interrogation_transcript
    (session : AgentInterrogationSession V)
    (controller : V.PrincipalId)
    (different : controller ≠ session.creator) :
    ¬ AgentInterrogationTranscriptReadableBy session controller := by
  intro readable
  exact different readable

/-- Domanda/risposta E2EE della sessione privata. -/
structure AgentInterrogationQuestion (V : Vocabulary) where
  sessionId : Nat
  payload : V.EncryptedPayload
  askedAt : Nat

structure AgentInterrogationAnswer
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  sessionId : Nat
  responder : V.PrincipalId
  payload : V.EncryptedPayload
  answeredAt : Nat
  /-- Provenance completa delle source plaintext usate per formulare l'answer. -/
  contextSources : List (InformationSource V X)

/--
L'answer è disclosure-safe per il creator. La sessione resta fuori dal work graph:
`secured` è usato soltanto per valutare la readability delle source al tick.
-/
def AgentInterrogationAnswerSafe
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (session : AgentInterrogationSession V)
    (answer : AgentInterrogationAnswer V X) : Prop :=
  answer.sessionId = session.id ∧
  answer.responder = session.targetAgent ∧
  ContextReadableByPrincipal
    authorization secured tick answer.contextSources session.creator

/--
Interrogazione answer-only: il target non riceve un nuovo work e non è autorizzato
ad aggiungere resource/tool side effect come conseguenza della domanda.
-/
structure ReadOnlyAgentInterrogationCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (session : AgentInterrogationSession V)
    (answer : AgentInterrogationAnswer V X) where
  answerSafe : AgentInterrogationAnswerSafe
    authorization secured tick session answer
  targetResourceEffects : List (ResourceSecurityEffect V)
  targetToolInvocations : List (ToolSecurityInvocation V)
  noResourceEffects : targetResourceEffects = []
  noToolInvocations : targetToolInvocations = []

/-- L'interrogazione non può essere usata come comando nascosto al target. -/
theorem interrogation_cannot_command_target_side_effect
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (session : AgentInterrogationSession V)
    (answer : AgentInterrogationAnswer V X)
    (certificate : ReadOnlyAgentInterrogationCertificate
      authorization secured tick session answer) :
    certificate.targetResourceEffects = [] ∧
    certificate.targetToolInvocations = [] := by
  exact ⟨certificate.noResourceEffects, certificate.noToolInvocations⟩

/-! #### R5.36B — Task cross-owner verso agent di un altro controller -/

/--
Classifier deterministico task→obligation. La semantica linguistica resta una
boundary esplicita, ma la decisione operativa usa l'output strutturato del
classifier e non un'etichetta arbitraria dichiarata dall'LLM.
-/
structure TaskObligationClassifier
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  «matches» : State V → V.ResourceId → ContractObligationSpec V X → Bool

structure TaskObligationClassificationSemantics
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  adequate : State V → V.ResourceId → ContractObligationSpec V X → Bool → Prop

/-- Intent organizzativo deterministico estratto dalla task per il responsibility gate. -/
structure TaskResponsibilityIntent (V : Vocabulary) where
  domain : Nat
  scope : V.ResourceId
  requiredActions : List AgentActionClass

structure TaskResponsibilityClassifier (V : Vocabulary) where
  classify : State V → V.ResourceId → Option (TaskResponsibilityIntent V)

structure TaskResponsibilityClassificationSemantics (V : Vocabulary) where
  adequate : State V → V.ResourceId → Option (TaskResponsibilityIntent V) → Prop

/-- La task è già parte di una obligation richiesta del LocalGoalContract target. -/
def TaskWithinRequiredLocalObligation
    (classifier : TaskObligationClassifier V X)
    (s : SemanticState V X)
    («local» : LocalGoalContract V X)
    (task : V.ResourceId) : Prop :=
  ∃ spec,
    spec ∈ «local».contract.obligations ∧
    spec.owner = «local».agent ∧
    ContractRequiredAt s spec ∧
    classifier.matches s.base task spec = true

/-- Un intent task è coperto da una ResponsibilityContract del controller. -/
def ResponsibilityCoversTaskIntent
    (responsibility : ResponsibilityContract V)
    (intent : TaskResponsibilityIntent V) : Prop :=
  ∃ rule,
    rule ∈ responsibility.rules ∧
    rule.domain = intent.domain ∧
    rule.scope = intent.scope ∧
    ∀ actionClass,
      actionClass ∈ intent.requiredActions →
      actionClass ∈ rule.allowedActions

/--
Il controller può ricevere una review cross-owner se la task è dentro la propria
responsibility; un administrator controller usa invece la governance del progetto.
-/
def TaskWithinTargetControllerGovernance
    (authorization : ProductAuthorizationProjection V)
    (taskClassifier : TaskResponsibilityClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : State V)
    (controller : V.PrincipalId)
    (task : V.ResourceId) : Prop :=
  (∃ responsibility intent,
      HasKind s controller PrincipalKind.user ∧
      responsibilities controller = some responsibility ∧
      taskClassifier.classify s task = some intent ∧
      ResponsibilityCoversTaskIntent responsibility intent) ∨
  (HasKind s controller PrincipalKind.administrator ∧
    ∃ «meta»,
      s.resources task = some «meta» ∧
      authorization.projectAdministrator s controller «meta».projectId)

/-- Richiesta di assegnazione da user non-admin a un agent di un altro controller. -/
structure CrossOwnerTaskAssignmentRequest (V : Vocabulary) where
  requester : V.PrincipalId
  task : V.ResourceId
  targetAgent : V.PrincipalId
  requestedAt : Nat

/-- Il requester deve poter realmente gestire/assegnare la task. -/
def CrossOwnerTaskAssignmentRequestValid
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (agents : GovernedAgentDirectory V)
    (request : CrossOwnerTaskAssignmentRequest V) : Prop :=
  HasKind s request.requester PrincipalKind.user ∧
  HasKind s request.targetAgent PrincipalKind.agent ∧
  ResourceOperationAllowed
    authorization s request.requester request.task ResourceOperation.manage ∧
  ∃ record,
    agents request.targetAgent = some record ∧
    record.agent = request.targetAgent ∧
    record.controller ≠ request.requester

/-- Caso 1: il nuovo assignment non amplia il mandato del target. -/
def CrossOwnerTaskAutomaticallyAcceptable
    (authorization : ProductAuthorizationProjection V)
    (obligationClassifier : TaskObligationClassifier V X)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (request : CrossOwnerTaskAssignmentRequest V) : Prop :=
  CrossOwnerTaskAssignmentRequestValid authorization s.base agents request ∧
  ∃ «local»,
    locals request.targetAgent = some «local» ∧
    «local».agent = request.targetAgent ∧
    TaskWithinRequiredLocalObligation obligationClassifier s «local» request.task

/-- Caso 2: serve consenso del controller del target e revisione del suo mandato. -/
def CrossOwnerTaskRequiresControllerReview
    (authorization : ProductAuthorizationProjection V)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskClassifier : TaskResponsibilityClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (request : CrossOwnerTaskAssignmentRequest V) : Prop :=
  CrossOwnerTaskAssignmentRequestValid authorization s.base agents request ∧
  ¬ CrossOwnerTaskAutomaticallyAcceptable
      authorization obligationClassifier s agents locals request ∧
  ∃ record,
    agents request.targetAgent = some record ∧
    TaskWithinTargetControllerGovernance
      authorization taskClassifier responsibilities s.base record.controller request.task

/-- Caso 3: fuori obligation e fuori governance del controller => rifiuto. -/
def CrossOwnerTaskRejected
    (authorization : ProductAuthorizationProjection V)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskClassifier : TaskResponsibilityClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (request : CrossOwnerTaskAssignmentRequest V) : Prop :=
  CrossOwnerTaskAssignmentRequestValid authorization s.base agents request ∧
  ¬ CrossOwnerTaskAutomaticallyAcceptable
      authorization obligationClassifier s agents locals request ∧
  ∀ record,
    agents request.targetAgent = some record →
    ¬ TaskWithinTargetControllerGovernance
      authorization taskClassifier responsibilities s.base record.controller request.task

/-- Review task reale assegnata al controller del target agent. -/
structure CrossOwnerTaskControllerReview (V : Vocabulary) where
  id : Nat
  requester : V.PrincipalId
  targetAgent : V.PrincipalId
  targetController : V.PrincipalId
  sourceTask : V.ResourceId
  reviewTask : V.ResourceId
  createdAt : Nat

/-- La review è privata/operativa del controller e usa una task R4 reale. -/
def CrossOwnerTaskControllerReviewValid
    (s : State V)
    (agents : GovernedAgentDirectory V)
    (request : CrossOwnerTaskAssignmentRequest V)
    (review : CrossOwnerTaskControllerReview V) : Prop :=
  review.requester = request.requester ∧
  review.targetAgent = request.targetAgent ∧
  review.sourceTask = request.task ∧
  (∃ record,
    agents request.targetAgent = some record ∧
    review.targetController = record.controller) ∧
  IsResourceKind s review.reviewTask ResourceKind.task ∧
  CreatedBy s review.reviewTask request.requester ∧
  AssignedTo s review.targetController review.reviewTask ∧
  OpenTask s review.reviewTask

inductive CrossOwnerTaskControllerDecisionMode where
  | approved
  | rejected
  deriving DecidableEq, Repr

structure CrossOwnerTaskControllerDecision (V : Vocabulary) where
  reviewId : Nat
  controller : V.PrincipalId
  mode : CrossOwnerTaskControllerDecisionMode
  decidedAt : Nat

/--
Dopo approval del controller, la task può essere assegnata solo quando la
configurazione ATTIVA del target la include ormai in una obligation richiesta.
Questo forza il normale ciclo R5.35 prompt+LocalGoalContract invece di introdurre
lavoro nascosto attraverso la sola assignment.
-/
def CrossOwnerControllerApprovalReadyForAssignment
    (obligationClassifier : TaskObligationClassifier V X)
    (s : SemanticState V X)
    (locals : LocalGoalDirectory V X)
    (review : CrossOwnerTaskControllerReview V)
    (decision : CrossOwnerTaskControllerDecision V) : Prop :=
  decision.reviewId = review.id ∧
  decision.controller = review.targetController ∧
  decision.mode = CrossOwnerTaskControllerDecisionMode.approved ∧
  ∃ «local»,
    locals review.targetAgent = some «local» ∧
    «local».agent = review.targetAgent ∧
    «local».controller = review.targetController ∧
    TaskWithinRequiredLocalObligation obligationClassifier s «local» review.sourceTask

/-- Routing esclusivo della richiesta cross-owner. -/
inductive CrossOwnerTaskAssignmentRoute where
  | automaticExistingObligation
  | controllerReview
  | rejected
  deriving DecidableEq, Repr

/-- Certificato del route scelto senza un quarto percorso implicito. -/
structure CrossOwnerTaskAssignmentRoutingCertificate
    (authorization : ProductAuthorizationProjection V)
    (obligationMeaning : TaskObligationClassificationSemantics V X)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskMeaning : TaskResponsibilityClassificationSemantics V)
    (taskClassifier : TaskResponsibilityClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (request : CrossOwnerTaskAssignmentRequest V) where
  obligationClassificationAdequate :
    ∀ «local» spec,
      locals request.targetAgent = some «local» →
      spec ∈ «local».contract.obligations →
      obligationMeaning.adequate
        s.base request.task spec (obligationClassifier.matches s.base request.task spec)
  taskClassificationAdequate :
    taskMeaning.adequate s.base request.task (taskClassifier.classify s.base request.task)
  route : CrossOwnerTaskAssignmentRoute
  justified :
    match route with
    | CrossOwnerTaskAssignmentRoute.automaticExistingObligation =>
        CrossOwnerTaskAutomaticallyAcceptable
          authorization obligationClassifier s agents locals request
    | CrossOwnerTaskAssignmentRoute.controllerReview =>
        CrossOwnerTaskRequiresControllerReview
          authorization obligationClassifier taskClassifier responsibilities
          s agents locals request
    | CrossOwnerTaskAssignmentRoute.rejected =>
        CrossOwnerTaskRejected
          authorization obligationClassifier taskClassifier responsibilities
          s agents locals request

/-- Un route automatico certifica che il mandato attivo già contiene la task. -/
theorem cross_owner_automatic_assignment_does_not_expand_local_goal
    (authorization : ProductAuthorizationProjection V)
    (obligationClassifier : TaskObligationClassifier V X)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (request : CrossOwnerTaskAssignmentRequest V)
    (automatic : CrossOwnerTaskAutomaticallyAcceptable
      authorization obligationClassifier s agents locals request) :
    ∃ «local»,
      locals request.targetAgent = some «local» ∧
      TaskWithinRequiredLocalObligation obligationClassifier s «local» request.task := by
  rcases automatic.2 with ⟨«local», active, identity, within⟩
  exact ⟨«local», active, within⟩

/--
Se la richiesta è certificata rejected non può essere trattata come automatic
acceptance; l'unica alternativa organizzativa sarebbe cambiare esplicitamente la
responsibility/governance in una transizione separata.
-/
theorem rejected_cross_owner_assignment_is_not_automatic
    (authorization : ProductAuthorizationProjection V)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskClassifier : TaskResponsibilityClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (request : CrossOwnerTaskAssignmentRequest V)
    (rejected : CrossOwnerTaskRejected
      authorization obligationClassifier taskClassifier responsibilities
      s agents locals request) :
    ¬ CrossOwnerTaskAutomaticallyAcceptable
      authorization obligationClassifier s agents locals request := by
  exact rejected.2.1

/-
REFINEMENT NOTE R5.36

Implementazione concreta richiesta:
* un solo UserProxy logical instance per principal umano, creato di default;
* N chat thread per proxy, con chiavi/transcript separati e plaintext leggibile
  soltanto dal creator; la confidentiality del transcript non implica che le
  mutazioni compiute dall'utente tramite proxy siano nascoste nell'audit/resource;
* il proxy non deve ottenere record ACL, keyring o tool grant propri: tutte le
  verifiche usano l'identità/sessione dell'utente corrente;
* una conferma "fuori responsibility" è one-shot e legata a chat+intent; non può
  confermare accessi che il permission engine/RLS/E2EE nega;
* le azioni proxy sono auditabili come actor=user, mediatedBy=proxy/thread;
* `contextualChat` legacy non deve più richiedere WorkItem/claim/scheduler; il sink
  privato può restare, ma il path runtime va separato da AgentSecurityEffect;
* interrogazione user/admin→agent può partire dal proxy; agent→agent deve invocare
  il tool dedicato e crea una sessione privata al creator;
* il target di una interrogazione non può materializzare task, commenti, tool
  side effect, prompt revision o work item per effetto della domanda; risponde
  soltanto entro la disclosure policy del creator;
* il controller del target non riceve automaticamente il transcript di una
  interrogazione creata da admin, admin-agent o altro agent; eventuale disclosure
  successiva deve essere una nuova azione autorizzata;
* per cross-owner task, il prodotto deve classificare deterministicamente sia il
  match task→obligation sia task→responsibility intent; le adequacy semantiche
  restano boundary esplicite e non vengono sostituite da un giudizio LLM trusted;
* il route controllerReview crea una task reale assegnata al controller del target;
  approval della review non basta: prima dell'assignment il target deve avere
  prompt+LocalGoalContract attivi e allineati che includono la nuova task;
* fuori dalle responsibility del controller il route è rejected e non crea una
  escalation admin implicita;
* l'administrator può comunque intervenire con i workflow amministrativi/globali
  già previsti da R5.35, ma come azione separata e tracciata, non come fallback
  nascosto della cross-owner assignment.
-/


/-! ### R5.37 — Chiusura semantico-operativa con LLM strutturato e stato corrente autorevole -/

/-
Questa sezione SUPERA la prima bozza R5.37 che introduceva human acceptance
aggiuntive della struttura interna. Il prodotto non usa l'umano come semantic
validator dell'LLM: le persone approvano/decidono soltanto nei gate di governance
gia' richiesti da R5.35/R5.36, mediante summary brevi.

Principi normativi:
1. ogni compito linguistico affidato all'LLM ha input finito, schema di output
   chiuso, riferimenti groundati nell'envelope e bound espliciti di dimensione/retry;
2. l'LLM interpreta e genera strutture; validator/provenance/authorization decidono
   well-formedness, scope, authority, information-flow e side effect;
3. nessun workflow operativo richiede che l'LLM produca una prova formale,
   certifichi permission, dimostri equivalenza semantica perfetta o scopra in modo
   esaustivo tutti i fatti del mondo;
4. le vecchie `...Adequacy` restano disponibili soltanto per theorem opzionali
   semanticamente forti e non sono proof obligation del runtime normale;
5. le sole conferme/decisioni umane aggiuntive sono quelle gia' concordate:
   final prompt approval, conferma one-shot chat fuori responsibility, consenso
   all'escalation admin, decisione admin sulla exception/responsibility, review
   cross-owner del controller e permission/grant espliciti gia' separati;
6. tali decisioni vengono presentate mediante summary brevi groundati in fatti
   strutturati; l'umano non deve revisionare GoalContract/AST interni;
7. non esiste memoria cognitiva persistente del modello separata dallo stato Sprout:
   ogni invocation ricostruisce il context leggendo lo State/SemanticState corrente,
   transcript, event history e provenance persistenti autorizzati;
8. i fatti strutturati correnti sono autorevoli nello State/SemanticState e non
   possono essere sostituiti da ricordi derivati del modello;
9. transcript/history/provenance possono persistere come dati del prodotto, ma non
   esiste uno store `ModelMemoryEntry` o altro stato cognitivo user/project-specific
   nascosto tra invocation.
-/

/-! #### R5.37A — Contratto realistico delle capacita' linguistiche -/

inductive StructuredLanguageTaskKind where
  | interpretProxyRequest
  | extractPromptRequirements
  | compileGoalContract
  | compileResponsibilityRules
  | deriveTaskIntent
  | synthesizeGlobalContract
  | summarizeGovernanceDecision
  | rewritePrompt
  | answerFromAuthorizedContext
  deriving DecidableEq, Repr

/--
Envelope generale: non misura "intelligenza" del modello, ma impedisce che il
prodotto gli chieda un problema non delimitato. I flag `requires...` devono essere
false nei task operativi normali.
-/
structure StructuredLanguageTaskEnvelope where
  kind : StructuredLanguageTaskKind
  inputItemCount : Nat
  maxInputItems : Nat
  maxOutputItems : Nat
  maxNestingDepth : Nat
  maxAttempts : Nat
  closedOutputSchema : Bool
  groundedIdentifiersOnly : Bool
  requiresFormalProof : Bool
  requiresPermissionDecision : Bool
  requiresExactSemanticEquivalence : Bool
  requiresExhaustiveWorldKnowledge : Bool

/-- Requisiti minimi per considerare ragionevole un task affidato a un LLM. -/
def StructuredLanguageTaskFeasible
    (task : StructuredLanguageTaskEnvelope) : Prop :=
  task.inputItemCount ≤ task.maxInputItems ∧
  0 < task.maxOutputItems ∧
  0 < task.maxNestingDepth ∧
  0 < task.maxAttempts ∧
  task.closedOutputSchema = true ∧
  task.groundedIdentifiersOnly = true ∧
  task.requiresFormalProof = false ∧
  task.requiresPermissionDecision = false ∧
  task.requiresExactSemanticEquivalence = false ∧
  task.requiresExhaustiveWorldKnowledge = false

/--
Boundary di disponibilita' realistica: per un task fattibile il provider deve
terminare con una risposta schema-valid oppure con failure esplicito entro i retry.
Non assume che ogni proposta sia semanticamente perfetta.
-/
structure StructuredLanguageModelRuntimeBoundary where
  returnsSchemaValidOrExplicitFailure : StructuredLanguageTaskEnvelope → Prop

/-- La boundary viene richiesta soltanto sui task gia' certificati fattibili. -/
def StructuredLanguageModelRuntimeBoundaryValid
    (boundary : StructuredLanguageModelRuntimeBoundary)
    (tasks : List StructuredLanguageTaskEnvelope) : Prop :=
  ∀ task,
    task ∈ tasks →
    StructuredLanguageTaskFeasible task →
    boundary.returnsSchemaValidOrExplicitFailure task

/-- Claim semantica opzionale, separata dal runtime operativo. -/
structure OptionalLanguageSemanticQualityBoundary
    (Input Output : Type u) where
  faithful : Input → Output → Prop

/-! #### R5.37B — Summary brevi solo nei gate umani gia' richiesti -/

inductive HumanGovernanceDecisionReason where
  | finalAgentPrompt
  | proxyActionOutsideResponsibility
  | sendResponsibilityExceptionToAdministrator
  | administratorResponsibilityExceptionDecision
  | crossOwnerTaskControllerDecision
  | explicitPermissionGrant
  deriving DecidableEq, Repr

/-- Riferimenti strutturati ai fatti che il summary deve spiegare. -/
inductive GovernanceFactRef (V : Vocabulary) where
  | localRevision (revision : Nat)
  | responsibilityRevision (revision : Nat)
  | uncoveredWorkSpec (workSpecId : Nat)
  | task (task : V.ResourceId)
  | agent (agent : V.PrincipalId)
  | actionIntent (intentId : Nat)
  | permissionScope (resource : V.ResourceId)

/--
Summary UI breve. `facts` e' autorevole per il binding; il payload e' soltanto una
verbalizzazione compatta prodotta dal modello.
-/
structure BriefGovernanceSummary (V : Vocabulary) where
  id : Nat
  reason : HumanGovernanceDecisionReason
  facts : List (GovernanceFactRef V)
  payload : V.EncryptedPayload
  generatedAt : Nat

/-- Default: massimo cinque fatti principali per una decisione umana. -/
def BriefGovernanceSummaryValid
    (summary : BriefGovernanceSummary V) : Prop :=
  summary.facts ≠ [] ∧ summary.facts.length ≤ 5

/-- Task LLM fortemente ristretto per verbalizzare fatti gia' determinati. -/
def GovernanceSummaryLanguageTask
    (summary : BriefGovernanceSummary V) : StructuredLanguageTaskEnvelope :=
  { kind := StructuredLanguageTaskKind.summarizeGovernanceDecision
    inputItemCount := summary.facts.length
    maxInputItems := 5
    maxOutputItems := 5
    maxNestingDepth := 2
    maxAttempts := 3
    closedOutputSchema := true
    groundedIdentifiersOnly := true
    requiresFormalProof := false
    requiresPermissionDecision := false
    requiresExactSemanticEquivalence := false
    requiresExhaustiveWorldKnowledge := false }

/-- Qualunque richiesta esplicita di approval nel nuovo layer deve dichiarare uno dei soli motivi ammessi. -/
structure HumanGovernanceDecisionRequest (V : Vocabulary) where
  principal : V.PrincipalId
  summary : BriefGovernanceSummary V

/-- Il prompt finale richiesto dal workflow viene approvato tramite un summary breve, non tramite AST tecnico. -/
def FinalPromptApprovalPresentedBriefly
    (approval : ControllerFinalPromptApproval V)
    (summary : BriefGovernanceSummary V) : Prop :=
  BriefGovernanceSummaryValid summary ∧
  summary.reason = HumanGovernanceDecisionReason.finalAgentPrompt ∧
  GovernanceFactRef.agent approval.agent ∈ summary.facts ∧
  GovernanceFactRef.localRevision approval.localRevision ∈ summary.facts

/-- Il consenso a escalare usa il summary breve della parte fuori responsibility. -/
def EscalationConsentPresentedBriefly
    (consent : UserEscalationConsent V)
    (summary : BriefGovernanceSummary V) : Prop :=
  BriefGovernanceSummaryValid summary ∧
  summary.reason =
    HumanGovernanceDecisionReason.sendResponsibilityExceptionToAdministrator ∧
  GovernanceFactRef.localRevision consent.sourceDraftId ∈ summary.facts

/-- La decisione amministrativa usa soltanto fatti strutturati della review/draft. -/
def AdministratorExceptionDecisionPresentedBriefly
    (review : ResponsibilityExceptionReview V X)
    (draft : AdministratorResponsibilityReviewDraft V X)
    (decision : AdministratorResponsibilityDecision V)
    (summary : BriefGovernanceSummary V) : Prop :=
  BriefGovernanceSummaryValid summary ∧
  summary.reason =
    HumanGovernanceDecisionReason.administratorResponsibilityExceptionDecision ∧
  decision.reviewId = review.id ∧
  draft.reviewId = review.id ∧
  GovernanceFactRef.agent review.agent ∈ summary.facts ∧
  GovernanceFactRef.localRevision draft.finalLocal.revision ∈ summary.facts

/-- Review cross-owner: il controller vede task, agent e una sintesi breve. -/
def CrossOwnerDecisionPresentedBriefly
    (review : CrossOwnerTaskControllerReview V)
    (decision : CrossOwnerTaskControllerDecision V)
    (summary : BriefGovernanceSummary V) : Prop :=
  BriefGovernanceSummaryValid summary ∧
  summary.reason = HumanGovernanceDecisionReason.crossOwnerTaskControllerDecision ∧
  decision.reviewId = review.id ∧
  GovernanceFactRef.task review.sourceTask ∈ summary.facts ∧
  GovernanceFactRef.agent review.targetAgent ∈ summary.facts

/-! #### R5.37C — Chat: request→plan strutturato, senza approval quando in responsibility -/

inductive UserProxyChatMessageAuthor (V : Vocabulary) where
  | human (principal : V.PrincipalId)
  | proxy (user : V.PrincipalId)
  | tool (tool : V.Tool)

structure UserProxyChatMessage (V : Vocabulary) where
  id : Nat
  threadId : Nat
  author : UserProxyChatMessageAuthor V
  payload : V.EncryptedPayload
  previousMessage : Option Nat
  createdAt : Nat

structure UserProxyChatTranscript (V : Vocabulary) where
  thread : UserProxyChatThread V
  messages : List (UserProxyChatMessage V)

/-- Transcript append-only e legato al solo owner del proxy. -/
def UserProxyChatTranscriptWellFormed
    (proxy : UserProxyAgent V)
    (transcript : UserProxyChatTranscript V) : Prop :=
  UserProxyChatThreadValid proxy transcript.thread ∧
  (∀ message,
    message ∈ transcript.messages →
    message.threadId = transcript.thread.id ∧
    match message.author with
    | UserProxyChatMessageAuthor.human principal => principal = proxy.user
    | UserProxyChatMessageAuthor.proxy user => user = proxy.user
    | UserProxyChatMessageAuthor.tool _ => True) ∧
  (∀ message,
    message ∈ transcript.messages →
    match message.previousMessage with
    | none => True
    | some previousId =>
        ∃ previous,
          previous ∈ transcript.messages ∧
          previous.id = previousId ∧
          previous.createdAt ≤ message.createdAt)

/-- Una nuova fotografia del transcript puo' soltanto aggiungere un suffisso. -/
def UserProxyTranscriptExtends
    (before after : UserProxyChatTranscript V) : Prop :=
  after.thread = before.thread ∧
  ∃ suffix, after.messages = before.messages ++ suffix

structure UserProxyRequest (V : Vocabulary) where
  id : Nat
  threadId : Nat
  user : V.PrincipalId
  payload : V.EncryptedPayload
  submittedAt : Nat

structure UserProxyPlannedToolInvocation (V : Vocabulary) where
  tool : V.Tool
  input : V.ToolInput

/--
L'envelope fornisce al modello soltanto ID e capability candidate gia' risolte dal
prodotto. Il modello non deve inventare PrincipalId/ResourceId/Tool fuori contesto.
-/
structure UserProxyPlanningEnvelope (V : Vocabulary) where
  task : StructuredLanguageTaskEnvelope
  requestId : Nat
  user : V.PrincipalId
  candidateResources : List V.ResourceId
  candidateOperations : List ResourceOperation
  availableTools : List V.Tool
  maxPlanSteps : Nat

structure UserProxyActionPlan (V : Vocabulary) where
  requestId : Nat
  threadId : Nat
  user : V.PrincipalId
  intentId : Nat
  resourceEffects : List (ResourceSecurityEffect V)
  toolInvocations : List (UserProxyPlannedToolInvocation V)
  explanation : V.EncryptedPayload

/-- Il piano puo' usare soltanto riferimenti messi nell'envelope. -/
def UserProxyActionPlanWithinEnvelope
    (envelope : UserProxyPlanningEnvelope V)
    (plan : UserProxyActionPlan V) : Prop :=
  StructuredLanguageTaskFeasible envelope.task ∧
  envelope.task.kind = StructuredLanguageTaskKind.interpretProxyRequest ∧
  plan.requestId = envelope.requestId ∧
  plan.user = envelope.user ∧
  plan.resourceEffects.length + plan.toolInvocations.length ≤ envelope.maxPlanSteps ∧
  (∀ effect,
    effect ∈ plan.resourceEffects →
    effect.resource ∈ envelope.candidateResources ∧
    effect.operation ∈ envelope.candidateOperations) ∧
  (∀ invocation,
    invocation ∈ plan.toolInvocations →
    invocation.tool ∈ envelope.availableTools)

/-- Il piano e' causalmente legato alla richiesta/thread. -/
def UserProxyActionPlanBoundToRequest
    (proxy : UserProxyAgent V)
    (thread : UserProxyChatThread V)
    (request : UserProxyRequest V)
    (plan : UserProxyActionPlan V) : Prop :=
  UserProxyChatThreadValid proxy thread ∧
  request.threadId = thread.id ∧
  request.user = proxy.user ∧
  plan.requestId = request.id ∧
  plan.threadId = thread.id ∧
  plan.user = proxy.user

/-- Requirement responsibility derivato dal footprint concreto. -/
structure UserProxyResponsibilityRequirement (V : Vocabulary) where
  scope : V.ResourceId
  actionClass : AgentActionClass

structure UserProxyResponsibilityFootprintClassifier (V : Vocabulary) where
  classify :
    State V →
    List (ResourceSecurityEffect V) →
    List (UserProxyPlannedToolInvocation V) →
    List (UserProxyResponsibilityRequirement V)

/-- Il gate responsibility usa scope/action reali, non label linguistiche. -/
def UserProxyPlanWithinResponsibility
    (classifier : UserProxyResponsibilityFootprintClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (plan : UserProxyActionPlan V) : Prop :=
  ∃ responsibility,
    responsibilities proxy.user = some responsibility ∧
    ∀ requirement,
      requirement ∈ classifier.classify s plan.resourceEffects plan.toolInvocations →
      ∃ rule,
        rule ∈ responsibility.rules ∧
        ResourceWithinScope s rule.scope requirement.scope ∧
        requirement.actionClass ∈ rule.allowedActions

/-- Permission/tool footprint reali del piano. -/
def UserProxyPlanRuntimeAllowed
    (authorization : ProductAuthorizationProjection V)
    (toolSecurity : ToolSecuritySemantics V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (plan : UserProxyActionPlan V) : Prop :=
  (∀ effect,
    effect ∈ plan.resourceEffects →
    ResourceOperationAllowed authorization s proxy.user effect.resource effect.operation) ∧
  (∀ invocation,
    invocation ∈ plan.toolInvocations →
    authorization.toolAllowed s proxy.user invocation.tool ∧
    ∀ requiredEffect,
      requiredEffect ∈ toolSecurity.requiredEffects s invocation.tool invocation.input →
      requiredEffect ∈ plan.resourceEffects ∧
      ResourceOperationAllowed authorization s proxy.user
        requiredEffect.resource requiredEffect.operation)

/--
Conferma one-shot SOLO per il caso gia' richiesto: azione permessa ma fuori
responsibility. E' legata all'intero piano, non a un semplice intentId riusabile.
-/
structure UserProxyOutOfResponsibilityConfirmation (V : Vocabulary) where
  user : V.PrincipalId
  threadId : Nat
  requestId : Nat
  acceptedPlan : UserProxyActionPlan V
  summaryId : Nat
  confirmedAt : Nat

/-- Binding esatto della conferma al piano presentato. -/
def UserProxyOutOfResponsibilityConfirmationMatches
    (proxy : UserProxyAgent V)
    (thread : UserProxyChatThread V)
    (plan : UserProxyActionPlan V)
    (confirmation : UserProxyOutOfResponsibilityConfirmation V) : Prop :=
  confirmation.user = proxy.user ∧
  confirmation.threadId = thread.id ∧
  confirmation.requestId = plan.requestId ∧
  confirmation.acceptedPlan = plan

/-- La conferma one-shot viene mostrata con un summary breve del piano esatto. -/
def UserProxyConfirmationPresentedBriefly
    (plan : UserProxyActionPlan V)
    (confirmation : UserProxyOutOfResponsibilityConfirmation V)
    (summary : BriefGovernanceSummary V) : Prop :=
  BriefGovernanceSummaryValid summary ∧
  summary.id = confirmation.summaryId ∧
  summary.reason = HumanGovernanceDecisionReason.proxyActionOutsideResponsibility ∧
  GovernanceFactRef.actionIntent plan.intentId ∈ summary.facts

/--
Esecuzione normale: nessuna approval se il piano e' dentro responsibility. Se e'
fuori ma i permission gate passano, serve soltanto la conferma one-shot concordata.
-/
structure UserProxyPlanExecutionCertificate
    (authorization : ProductAuthorizationProjection V)
    (toolSecurity : ToolSecuritySemantics V)
    (responsibilityClassifier : UserProxyResponsibilityFootprintClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : State V)
    (proxy : UserProxyAgent V)
    (thread : UserProxyChatThread V)
    (request : UserProxyRequest V)
    (envelope : UserProxyPlanningEnvelope V)
    (plan : UserProxyActionPlan V)
    (confirmation : Option (UserProxyOutOfResponsibilityConfirmation V)) : Prop where
  bound : UserProxyActionPlanBoundToRequest proxy thread request plan
  withinEnvelope : UserProxyActionPlanWithinEnvelope envelope plan
  runtimeAllowed : UserProxyPlanRuntimeAllowed authorization toolSecurity s proxy plan
  responsibilityOrRequestedConfirmation :
    UserProxyPlanWithinResponsibility
      responsibilityClassifier responsibilities s proxy plan ∨
    ∃ accepted,
      confirmation = some accepted ∧
      UserProxyOutOfResponsibilityConfirmationMatches proxy thread plan accepted

/-- Semantic fidelity opzionale: utile per evaluation, non per permission safety. -/
structure UserProxyRequestSemantics (V : Vocabulary) where
  faithful : UserProxyRequest V → UserProxyActionPlan V → Prop

/-- Audit: actor normativo=user, proxy/thread sono mediation metadata. -/
structure UserProxyMediatedAuditEntry (V : Vocabulary) where
  user : V.PrincipalId
  proxyId : Nat
  threadId : Nat
  requestId : Nat
  plan : UserProxyActionPlan V
  recordedAt : Nat

/-! #### R5.37D — Prompt→LocalGoal: estrazione requisiti finita, nessuna review tecnica umana -/

/-- Requirement linguistico finito estratto dal prompt. -/
structure PromptRequirement (V : Vocabulary) where
  id : Nat
  scope : V.ResourceId
  requiredActions : List AgentActionClass
  requiredForCompletion : Bool

structure PromptRequirementWorkBinding (V : Vocabulary) where
  requirementId : Nat
  obligation : V.ObligationId
  workSpecId : Nat

/--
Compiler strutturato a due output: requisiti finiti + GoalContract. L'implementazione
puo' usare uno o piu' LLM call, ma i due output sono poi validati deterministicamente.
-/
structure StructuredLocalContractCompiler
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  contractCompiler : ContractCompiler V X
  extractRequirements : V.SystemPrompt → List (PromptRequirement V)
  bindings : V.SystemPrompt → List (PromptRequirementWorkBinding V)

structure LocalGoalCompilationEnvelope (V : Vocabulary) where
  task : StructuredLanguageTaskEnvelope
  agent : V.PrincipalId
  controller : V.PrincipalId
  projectScope : V.ResourceId
  allowedActions : List AgentActionClass
  maxRequirements : Nat
  maxObligations : Nat
  maxWorkSpecs : Nat
  maxDependencies : Nat

/-- Ogni requirement estratto e' rappresentato da almeno un WorkSpec e viceversa. -/
def PromptRequirementsAndWorkExact
    (compiler : StructuredLocalContractCompiler V X)
    (prompt : V.SystemPrompt)
    (contract : GoalContract V X) : Prop :=
  let requirements := compiler.extractRequirements prompt
  let links := compiler.bindings prompt
  (∀ requirement,
    requirement ∈ requirements →
    ∃ link obligationSpec workSpec,
      link ∈ links ∧
      link.requirementId = requirement.id ∧
      obligationSpec ∈ contract.obligations ∧
      obligationSpec.id = link.obligation ∧
      workSpec ∈ contract.workSpecs ∧
      workSpec.id = link.workSpecId ∧
      workSpec.obligation = link.obligation ∧
      (requirement.requiredForCompletion = true →
        obligationSpec.requiredForCompletion ≠ ContractCondition.never) ∧
      (∀ action,
        action ∈ requirement.requiredActions →
        action ∈ workSpec.allowedActions)) ∧
  (∀ obligationSpec,
    obligationSpec ∈ contract.obligations →
    ∃ requirement link,
      requirement ∈ requirements ∧
      link ∈ links ∧
      link.requirementId = requirement.id ∧
      link.obligation = obligationSpec.id) ∧
  (∀ workSpec,
    workSpec ∈ contract.workSpecs →
    ∃ requirement link,
      requirement ∈ requirements ∧
      link ∈ links ∧
      link.requirementId = requirement.id ∧
      link.workSpecId = workSpec.id)

/-- Il compiler non puo' introdurre azioni fuori dal catalogo fornito. -/
def LocalGoalCompilationWithinEnvelope
    (s : State V)
    (compiler : StructuredLocalContractCompiler V X)
    (envelope : LocalGoalCompilationEnvelope V)
    (prompt : V.SystemPrompt)
    («local» : LocalGoalContract V X) : Prop :=
  StructuredLanguageTaskFeasible envelope.task ∧
  envelope.task.kind = StructuredLanguageTaskKind.compileGoalContract ∧
  «local».agent = envelope.agent ∧
  «local».controller = envelope.controller ∧
  «local».prompt = prompt ∧
  «local».contract = compiler.contractCompiler.compile prompt ∧
  ResourceWithinScope s envelope.projectScope «local».contract.goal.scope ∧
  (compiler.extractRequirements prompt).length ≤ envelope.maxRequirements ∧
  «local».contract.obligations.length ≤ envelope.maxObligations ∧
  «local».contract.workSpecs.length ≤ envelope.maxWorkSpecs ∧
  «local».contract.dependencies.length ≤ envelope.maxDependencies ∧
  GoalContractWellFormed «local».contract ∧
  PromptRequirementsAndWorkExact compiler prompt «local».contract ∧
  ∀ workSpec,
    workSpec ∈ «local».contract.workSpecs →
    ∀ action,
      action ∈ workSpec.allowedActions →
      action ∈ envelope.allowedActions

/-- Optional semantic claim: l'extraction rappresenta davvero tutto l'intento del prompt. -/
structure PromptRequirementSemantics
    (V : Vocabulary) where
  faithful : V.SystemPrompt → List (PromptRequirement V) → Prop

/--
Responsibility operativa: scope reale + action class dei WorkSpec. `domain` resta
utile per UI/organizzazione ma non puo' ampliare authority.
-/
def ResponsibilityOperationallyCoversLocalGoal
    (s : State V)
    (responsibility : ResponsibilityContract V)
    («local» : LocalGoalContract V X) : Prop :=
  responsibility.user = «local».controller ∧
  ∀ workSpec,
    workSpec ∈ «local».contract.workSpecs →
    ∃ rule,
      rule ∈ responsibility.rules ∧
      ResourceWithinScope s rule.scope «local».contract.goal.scope ∧
      ResponsibilityRuleCoversWork rule workSpec

/-- Authorization locale normale senza semantic oracle. -/
def OperationalLocalDraftAuthorized
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X) : Prop :=
  (∃ responsibility,
      responsibilities draft.controller = some responsibility ∧
      ResponsibilityOperationallyCoversLocalGoal s responsibility draft.proposedLocal) ∨
  LocalGoalApprovedByException exceptions draft.proposedLocal ∨
  (∃ assignment,
      assignment ∈ globalAssignments ∧
      assignment.«local» = draft.proposedLocal)

/--
Activation del prompt: conserva SOLO l'approvazione finale del prompt richiesta dal
workflow R5.35. Nessuna seconda approval della struttura tecnica GoalContract.
-/
structure OperationalLocalRevisionActivationCertificate
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (envelope : LocalGoalCompilationEnvelope V)
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (draft : LocalPromptGoalRevisionDraft V X)
    (approval : ControllerFinalPromptApproval V) : Prop where
  draftWellFormed :
    LocalPromptGoalRevisionDraftWellFormed structuredCompiler.contractCompiler draft
  compilationBounded :
    LocalGoalCompilationWithinEnvelope
      s structuredCompiler envelope draft.proposedPrompt draft.proposedLocal
  classified : LocalGoalClassifiedBy classifier draft.proposedLocal
  authorized : OperationalLocalDraftAuthorized
    s responsibilities exceptions globalAssignments draft
  finalPromptApproval : ControllerApprovalMatchesDraft draft approval

/--
Creation usa lo stesso compiler strutturato. Un administrator puo' autorizzare
direttamente soltanto l'esatta proposta mediante un record append-only distinto;
ruolo e permission generici non sostituiscono tale record.
-/
structure OperationalAgentCreationActivationCertificate
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (envelope : LocalGoalCompilationEnvelope V)
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (proposal : AgentCreationProposal V X)
    (approval : ControllerFinalPromptApproval V) : Prop where
  proposalValid : AgentCreationProposalValid structuredCompiler.contractCompiler s proposal
  compilationBounded :
    LocalGoalCompilationWithinEnvelope
      s structuredCompiler envelope proposal.prompt proposal.«local»
  classified : LocalGoalClassifiedBy classifier proposal.«local»
  authorized :
    (∃ responsibility,
      responsibilities proposal.creator = some responsibility ∧
      ResponsibilityOperationallyCoversLocalGoal s responsibility proposal.«local») ∨
    LocalGoalApprovedByException exceptions proposal.«local» ∨
    (∃ assignment,
      assignment ∈ globalAssignments ∧ assignment.«local» = proposal.«local») ∨
    AgentCreationApprovedByAdministrator
      authorization s administratorCreationApprovals proposal
  finalPromptApproval :
    approval.agent = proposal.proposedAgent ∧
    approval.controller = proposal.creator ∧
    approval.prompt = proposal.prompt ∧
    approval.localRevision = proposal.«local».revision

/-! #### R5.37E — Responsibility text→rules strutturato, senza seconda approvazione umana -/

structure ResponsibilityCompilationEnvelope (V : Vocabulary) where
  task : StructuredLanguageTaskEnvelope
  administrator : V.PrincipalId
  user : V.PrincipalId
  projectScopes : List V.ResourceId
  allowedActions : List AgentActionClass
  maxRules : Nat

/--
L'admin ha gia' espresso la decisione organizzativa scrivendo/aggiornando la
responsibility. Il compiler puo' usare l'LLM, ma le regole devono restare entro
scope amministrati e catalogo azioni chiuso.
-/
def ResponsibilityCompilationWithinEnvelope
    (authorization : ProductAuthorizationProjection V)
    (compiler : ResponsibilityCompiler V)
    (s : State V)
    (envelope : ResponsibilityCompilationEnvelope V)
    (responsibility : ResponsibilityContract V) : Prop :=
  StructuredLanguageTaskFeasible envelope.task ∧
  envelope.task.kind = StructuredLanguageTaskKind.compileResponsibilityRules ∧
  responsibility.administrator = envelope.administrator ∧
  responsibility.user = envelope.user ∧
  responsibility.rules = compiler.compile responsibility.sourceText ∧
  responsibility.rules.length ≤ envelope.maxRules ∧
  ResponsibilityContractValid authorization s responsibility ∧
  ∀ rule,
    rule ∈ responsibility.rules →
    (∃ projectScope,
      projectScope ∈ envelope.projectScopes ∧
      ResourceWithinScope s projectScope rule.scope) ∧
    (∀ action, action ∈ rule.allowedActions → action ∈ envelope.allowedActions)

structure OperationalResponsibilityActivationCertificate
    (authorization : ProductAuthorizationProjection V)
    (compiler : ResponsibilityCompiler V)
    (s : State V)
    (envelope : ResponsibilityCompilationEnvelope V)
    (responsibility : ResponsibilityContract V) : Prop where
  boundedCompilation :
    ResponsibilityCompilationWithinEnvelope authorization compiler s envelope responsibility

/-! #### R5.37F — Diff strutturale e summary brevi delle eccezioni -/

structure ResponsibilityWorkCoverageDiff where
  localRevision : Nat
  coveredWorkSpecIds : List Nat
  uncoveredWorkSpecIds : List Nat

/-- Exact diff sui WorkSpec reali; il summary LLM non decide cosa e' fuori responsibility. -/
def ResponsibilityWorkCoverageDiffExact
    (s : State V)
    (responsibility : ResponsibilityContract V)
    («local» : LocalGoalContract V X)
    (diff : ResponsibilityWorkCoverageDiff) : Prop :=
  diff.localRevision = «local».revision ∧
  (∀ workSpec,
    workSpec ∈ «local».contract.workSpecs →
    (workSpec.id ∈ diff.coveredWorkSpecIds ↔
      ∃ rule,
        rule ∈ responsibility.rules ∧
        ResourceWithinScope s rule.scope «local».contract.goal.scope ∧
        ResponsibilityRuleCoversWork rule workSpec)) ∧
  (∀ workSpec,
    workSpec ∈ «local».contract.workSpecs →
    (workSpec.id ∈ diff.uncoveredWorkSpecIds ↔
      ¬ ∃ rule,
        rule ∈ responsibility.rules ∧
        ResourceWithinScope s rule.scope «local».contract.goal.scope ∧
        ResponsibilityRuleCoversWork rule workSpec))

/-! #### R5.37G — Task provenance e TaskIntent persistito -/

/-- Link esatto creato quando una task materializza work gia' derivato da un obligation. -/
structure TaskObligationProvenance (V : Vocabulary) where
  task : V.ResourceId
  agent : V.PrincipalId
  localRevision : Nat
  obligation : V.ObligationId
  workSpecId : Nat
  recordedAt : Nat

/-- La provenance automatica deve puntare a LocalGoal attivo e oggetti realmente noti. -/
def TaskObligationProvenanceValid
    (s : SemanticState V X)
    (locals : LocalGoalDirectory V X)
    (provenance : TaskObligationProvenance V) : Prop :=
  IsResourceKind s.base provenance.task ResourceKind.task ∧
  ∃ «local» obligationSpec workSpec,
    locals provenance.agent = some «local» ∧
    «local».revision = provenance.localRevision ∧
    obligationSpec ∈ «local».contract.obligations ∧
    obligationSpec.id = provenance.obligation ∧
    obligationSpec.owner = provenance.agent ∧
    ContractRequiredAt s obligationSpec ∧
    workSpec ∈ «local».contract.workSpecs ∧
    workSpec.id = provenance.workSpecId ∧
    workSpec.obligation = provenance.obligation ∧
    workSpec.owner = provenance.agent

/-- Intent organizzativo persistito alla creazione della task. Non concede assignment automatico. -/
structure PersistedTaskIntent (V : Vocabulary) where
  task : V.ResourceId
  scope : V.ResourceId
  requiredActions : List AgentActionClass
  createdBy : V.PrincipalId
  recordedAt : Nat

structure TaskIntentDerivationEnvelope (V : Vocabulary) where
  task : StructuredLanguageTaskEnvelope
  taskResource : V.ResourceId
  projectScope : V.ResourceId
  allowedActions : List AgentActionClass
  maxActions : Nat

/-- Intent task bounded: scope deve essere nel progetto e le action nel catalogo. -/
def PersistedTaskIntentWithinEnvelope
    (s : State V)
    (envelope : TaskIntentDerivationEnvelope V)
    (intent : PersistedTaskIntent V) : Prop :=
  StructuredLanguageTaskFeasible envelope.task ∧
  envelope.task.kind = StructuredLanguageTaskKind.deriveTaskIntent ∧
  intent.task = envelope.taskResource ∧
  ResourceWithinScope s envelope.projectScope intent.scope ∧
  intent.requiredActions.length ≤ envelope.maxActions ∧
  ∀ action,
    action ∈ intent.requiredActions →
    action ∈ envelope.allowedActions

/-- Responsibility del controller rispetto a un TaskIntent gia' persistito. -/
def ResponsibilityCoversPersistedTaskIntent
    (s : State V)
    (responsibility : ResponsibilityContract V)
    (intent : PersistedTaskIntent V) : Prop :=
  ∃ rule,
    rule ∈ responsibility.rules ∧
    ResourceWithinScope s rule.scope intent.scope ∧
    ∀ action,
      action ∈ intent.requiredActions →
      action ∈ rule.allowedActions

/-- Caso automatico cross-owner: SOLO provenance esatta verso obligation attiva. -/
def CrossOwnerTaskAutomaticallyAcceptableByProvenance
    (authorization : ProductAuthorizationProjection V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (provenances : List (TaskObligationProvenance V))
    (request : CrossOwnerTaskAssignmentRequest V) : Prop :=
  CrossOwnerTaskAssignmentRequestValid authorization s.base agents request ∧
  ∃ provenance,
    provenance ∈ provenances ∧
    provenance.task = request.task ∧
    provenance.agent = request.targetAgent ∧
    TaskObligationProvenanceValid s locals provenance

/-- Caso review: non automatico, ma TaskIntent rientra nella responsibility del controller target. -/
def CrossOwnerTaskRequiresControllerReviewByPersistedIntent
    (authorization : ProductAuthorizationProjection V)
    (responsibilities : ResponsibilityDirectory V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (provenances : List (TaskObligationProvenance V))
    (intents : List (PersistedTaskIntent V))
    (request : CrossOwnerTaskAssignmentRequest V) : Prop :=
  CrossOwnerTaskAssignmentRequestValid authorization s.base agents request ∧
  ¬ CrossOwnerTaskAutomaticallyAcceptableByProvenance
      authorization s agents locals provenances request ∧
  ∃ record responsibility intent,
    agents request.targetAgent = some record ∧
    responsibilities record.controller = some responsibility ∧
    intent ∈ intents ∧
    intent.task = request.task ∧
    responsibility.user = record.controller ∧
    ResponsibilityCoversPersistedTaskIntent s.base responsibility intent

/-- Fuori obligation e fuori responsibility target: rifiuto come richiesto. -/
def CrossOwnerTaskRejectedByPersistedIntent
    (authorization : ProductAuthorizationProjection V)
    (responsibilities : ResponsibilityDirectory V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (provenances : List (TaskObligationProvenance V))
    (intents : List (PersistedTaskIntent V))
    (request : CrossOwnerTaskAssignmentRequest V) : Prop :=
  CrossOwnerTaskAssignmentRequestValid authorization s.base agents request ∧
  ¬ CrossOwnerTaskAutomaticallyAcceptableByProvenance
      authorization s agents locals provenances request ∧
  ∀ record responsibility intent,
    agents request.targetAgent = some record →
    responsibilities record.controller = some responsibility →
    intent ∈ intents →
    intent.task = request.task →
    ¬ ResponsibilityCoversPersistedTaskIntent s.base responsibility intent

/-! #### R5.37H — Sintesi globale LLM strutturata, automatica se source-grounded e senza conflict gate -/

/-- Mapping esplicito di un WorkSpec globale alla sua sorgente locale. -/
structure StructuredGlobalWorkGrounding (V : Vocabulary) where
  globalWorkSpecId : Nat
  sourceAgent : V.PrincipalId
  sourceLocalRevision : Nat
  sourceWorkSpecId : Nat

/--
Grounding conservativo: il synthesizer puo' riorganizzare IDs/dependency, ma non
puo' espandere owner/action/failure behavior di un WorkSpec sorgente.
-/
def StructuredGlobalWorkGroundingValid
    (locals : LocalGoalDirectory V X)
    (candidate : GlobalContractCandidate V X)
    (grounding : StructuredGlobalWorkGrounding V) : Prop :=
  ∃ «local» localWork globalWork,
    locals grounding.sourceAgent = some «local» ∧
    «local».revision = grounding.sourceLocalRevision ∧
    localWork ∈ «local».contract.workSpecs ∧
    localWork.id = grounding.sourceWorkSpecId ∧
    globalWork ∈ candidate.contract.workSpecs ∧
    globalWork.id = grounding.globalWorkSpecId ∧
    globalWork.owner = localWork.owner ∧
    globalWork.kind = localWork.kind ∧
    globalWork.allowedActions = localWork.allowedActions ∧
    globalWork.maxInstances ≤ localWork.maxInstances ∧
    globalWork.maxAttempts ≤ localWork.maxAttempts ∧
    globalWork.failurePlan = localWork.failurePlan

structure StructuredGlobalSynthesisEnvelope (V : Vocabulary) where
  task : StructuredLanguageTaskEnvelope
  sourceAgents : List V.PrincipalId
  maxGlobalObligations : Nat
  maxGlobalWorkSpecs : Nat
  maxDependencies : Nat
  maxConflicts : Nat

/-- Authorization source bottom-up usando il gate operativo, non semantic adequacy. -/
def OperationalLocalGoalAuthorizedForBottomUp
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    («local» : LocalGoalContract V X) : Prop :=
  LocalGoalCanContributeBottomUp «local» ∧
  ((∃ responsibility,
      responsibilities «local».controller = some responsibility ∧
      ResponsibilityOperationallyCoversLocalGoal s responsibility «local») ∨
   LocalGoalApprovedByException exceptions «local»)

/--
Automatic global synthesis: l'LLM puo' inferire dependency/conflitti, ma tutto il
work autorevole deve essere groundato a work locale gia' autorizzato. Non serve
approval umana per ogni dependency. Se il modello segnala `governanceConflicts`,
il normale conflict workflow R5.35 resta il gate; se non li segnala, la revisione
puo' essere automatica senza assumere conflict-completeness perfetta.
-/
structure StructuredGlobalSynthesisCertificate
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (locals : LocalGoalDirectory V X)
    (envelope : StructuredGlobalSynthesisEnvelope V)
    (candidate : GlobalContractCandidate V X)
    (groundings : List (StructuredGlobalWorkGrounding V)) : Prop where
  taskFeasible : StructuredLanguageTaskFeasible envelope.task
  correctTaskKind : envelope.task.kind = StructuredLanguageTaskKind.synthesizeGlobalContract
  contractWellFormed : GoalContractWellFormed candidate.contract
  bounded :
    candidate.contract.obligations.length ≤ envelope.maxGlobalObligations ∧
    candidate.contract.workSpecs.length ≤ envelope.maxGlobalWorkSpecs ∧
    candidate.contract.dependencies.length ≤ envelope.maxDependencies ∧
    candidate.governanceConflicts.length ≤ envelope.maxConflicts
  contributionsValid :
    ∀ contribution,
      contribution ∈ candidate.contributions →
      GlobalLocalContributionValid locals contribution
  everySourceOperationallyAuthorized :
    ∀ contribution «local»,
      contribution ∈ candidate.contributions →
      locals contribution.agent = some «local» →
      OperationalLocalGoalAuthorizedForBottomUp s responsibilities exceptions «local»
  everyGlobalWorkGrounded :
    ∀ workSpec,
      workSpec ∈ candidate.contract.workSpecs →
      ∃ grounding,
        grounding ∈ groundings ∧
        grounding.globalWorkSpecId = workSpec.id ∧
        StructuredGlobalWorkGroundingValid locals candidate grounding
  noAutomaticActivationWithDeclaredConflict : candidate.governanceConflicts = []

/-- Optional strong claim: utile per evaluation/theorem "semanticamente completo", non per activation normale. -/
structure StructuredGlobalSynthesisSemanticQuality
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  faithfulAndConflictComplete : LocalGoalDirectory V X → GlobalContractCandidate V X → Prop

/-! #### R5.37I — Interrogazione read-only forte -/

structure AgentInterrogationCausalDelta
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  resourceEffects : List (ResourceSecurityEffect V)
  toolInvocations : List (ToolSecurityInvocation V)
  promptRevisions : List V.PrincipalId
  localGoalRevisions : List V.PrincipalId
  createdWork : List X.WorkItemId
  activatedObligations : List V.ObligationId
  assignedTasks : List V.ResourceId

structure StrongReadOnlyAgentInterrogationCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (session : AgentInterrogationSession V)
    (answer : AgentInterrogationAnswer V X)
    (delta : AgentInterrogationCausalDelta V X) : Prop where
  answerSafe : AgentInterrogationAnswerSafe authorization secured tick session answer
  noResourceEffects : delta.resourceEffects = []
  noToolInvocations : delta.toolInvocations = []
  noPromptRevisions : delta.promptRevisions = []
  noLocalGoalRevisions : delta.localGoalRevisions = []
  noCreatedWork : delta.createdWork = []
  noActivatedObligations : delta.activatedObligations = []
  noAssignments : delta.assignedTasks = []

/-! #### R5.37J — Invocation state-grounded, senza memoria cognitiva persistente -/

/--
Contesto di una singola invocation. Ogni source deve provenire dallo stato corrente,
dalla history/provenance o da transcript persistenti del prodotto; non esiste un
campo di memoria cognitiva recuperata da uno store parallelo del modello.
-/
structure ModelInvocationContext
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  directSources : List (InformationSource V X)

/-- Ogni source realmente fornita al modello deve essere leggibile ORA dal principal. -/
def ModelInvocationContextSafe
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (principal : V.PrincipalId)
    (context : ModelInvocationContext V X) : Prop :=
  ∀ source,
    source ∈ context.directSources →
    InformationSourceReadableBy
      authorization secured tick
      (secured.certified.run.semanticState tick).base source principal

/--
Projection di tutto il context Sprout-specific realmente disponibile alla invocation.
`hiddenPersistentModelMemoryAvailable` rappresenterebbe uno stato cognitivo persistente
appreso da Sprout ma non ricostruito dalle source dichiarate: nel refinement deve essere false.
-/
structure ModelExposureProjection
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  directSourceExposed : InformationSource V X → Prop
  hiddenPersistentModelMemoryAvailable : Prop

/-- Context esatto e nessuna memoria LLM persistente occulta. -/
def ModelExposureExact
    (projection : ModelExposureProjection V X)
    (context : ModelInvocationContext V X) : Prop :=
  (∀ source,
    projection.directSourceExposed source ↔ source ∈ context.directSources) ∧
  ¬ projection.hiddenPersistentModelMemoryAvailable

/--
Certificate per ogni invocation LLM: context autorizzato nello stato corrente e
assenza di memoria cognitiva persistente separata dal product state.
-/
structure StateGroundedModelInvocationCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (principal : V.PrincipalId)
    (context : ModelInvocationContext V X)
    (projection : ModelExposureProjection V X) : Prop where
  contextSafe : ModelInvocationContextSafe authorization secured tick principal context
  exposureExact : ModelExposureExact projection context

/-- Nessuna source non leggibile puo' entrare nel context corrente. -/
theorem state_grounded_invocation_uses_only_current_authorized_sources
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (principal : V.PrincipalId)
    (context : ModelInvocationContext V X)
    (projection : ModelExposureProjection V X)
    (certificate : StateGroundedModelInvocationCertificate
      authorization secured tick principal context projection)
    (source : InformationSource V X)
    (sourceIn : source ∈ context.directSources) :
    InformationSourceReadableBy
      authorization secured tick
      (secured.certified.run.semanticState tick).base source principal := by
  exact certificate.contextSafe source sourceIn

/-- Il refinement conforme non rende disponibile memoria cognitiva persistente occulta. -/
theorem hidden_persistent_model_memory_is_forbidden
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (principal : V.PrincipalId)
    (context : ModelInvocationContext V X)
    (projection : ModelExposureProjection V X)
    (certificate : StateGroundedModelInvocationCertificate
      authorization secured tick principal context projection) :
    ¬ projection.hiddenPersistentModelMemoryAvailable := by
  exact certificate.exposureExact.2

/-- Interrogazione: l'answer usa esattamente le source state-grounded della invocation. -/
def AgentInterrogationAnswerContextExact
    (answer : AgentInterrogationAnswer V X)
    (context : ModelInvocationContext V X) : Prop :=
  answer.contextSources = context.directSources

structure StateGroundedStrongInterrogationCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (tick : Nat)
    (session : AgentInterrogationSession V)
    (answer : AgentInterrogationAnswer V X)
    (delta : AgentInterrogationCausalDelta V X)
    (context : ModelInvocationContext V X)
    (projection : ModelExposureProjection V X) : Prop where
  readOnly : StrongReadOnlyAgentInterrogationCertificate
    authorization secured tick session answer delta
  invocationSafe : StateGroundedModelInvocationCertificate
    authorization secured tick session.creator context projection
  answerContextExact : AgentInterrogationAnswerContextExact answer context

/-! #### R5.37K — Stato product-side e invarianti di closure -/

def AppendOnlyList {α : Type u} (before after : List α) : Prop :=
  ∃ suffix, after = before ++ suffix

structure SemanticOperationalState
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  proxyTranscripts : List (UserProxyChatTranscript V)
  proxyAudit : List (UserProxyMediatedAuditEntry V)
  taskObligationProvenance : List (TaskObligationProvenance V)
  taskIntents : List (PersistedTaskIntent V)

/-- Storia product-side: audit/provenance/transcript append-only; nessuna memoria LLM separata. -/
def SemanticOperationalStateExtends
    (before after : SemanticOperationalState V X) : Prop :=
  AppendOnlyList before.proxyTranscripts after.proxyTranscripts ∧
  AppendOnlyList before.proxyAudit after.proxyAudit ∧
  AppendOnlyList before.taskObligationProvenance after.taskObligationProvenance ∧
  AppendOnlyList before.taskIntents after.taskIntents

/-- Bundle finale della closure operativa. -/
structure SemanticOperationalClosureCertificate
    (authorization : ProductAuthorizationProjection V)
    (s : State V)
    (proxyDirectory : UserProxyDirectory V)
    (languageTasks : List StructuredLanguageTaskEnvelope)
    (languageRuntime : StructuredLanguageModelRuntimeBoundary) : Prop where
  proxyProvisioned : UserProxyDefaultProvisioning s proxyDirectory
  everyLanguageTaskFeasible :
    ∀ task,
      task ∈ languageTasks →
      StructuredLanguageTaskFeasible task
  languageRuntimeBounded :
    StructuredLanguageModelRuntimeBoundaryValid languageRuntime languageTasks

/-
REFINEMENT NOTE R5.37 — confine concreto LLM/runtime

A. Nessuna human review aggiuntiva
* in-responsibility UserProxy plan: esecuzione automatica dopo validator/permission;
* LocalGoalContract: il controller approva solo il PROMPT finale come gia' richiesto;
  non revisiona AST/GoalContract tecnico;
* ResponsibilityContract: l'admin scrive/modifica la responsibility e il compiler
  strutturato produce le rules senza una seconda approval;
* global synthesis source-grounded: automatico se non esiste conflict dichiarato;
  non serve approvare ogni dependency proposta;
* gli unici approval/confirmation restano i reason chiusi di
  `HumanGovernanceDecisionReason` e usano `BriefGovernanceSummary`.

B. Cosa chiediamo realisticamente all'LLM
* scegliere/compilare oggetti in schema JSON/typed chiuso;
* usare solo ID/resource/tool presenti nell'envelope;
* estrarre una lista finita di requirements da un prompt;
* mappare requirements a WorkSpec con bound piccoli e verificabili;
* compilare responsibility rules entro project scope e action catalog;
* derivare un TaskIntent finito;
* sintetizzare un candidato globale usando solo LocalGoal sorgente e grounding;
* verbalizzare al massimo pochi fatti per summary di governance;
* rispondere da context ricostruito da State/SemanticState, history, provenance e transcript autorizzati.

Non chiediamo all'LLM:
* prove Lean o proof certificate;
* decisioni ACL/RLS/E2EE/tool authorization;
* garanzia di equivalenza semantica perfetta col linguaggio naturale;
* conflict/world-knowledge completeness assoluta;
* theorem di termination/liveness/noninterference;
* scelta di ID o capability non presenti nell'envelope.

C. Nessuna memoria cognitiva persistente del modello
* non esiste `ModelMemoryEntry` ne' uno store di memoria LLM parallelo;
* ogni invocation ricostruisce il context dalle source correnti autorizzate, incluse
  State/SemanticState, event/history/provenance e transcript persistenti del prodotto;
* i transcript e la history possono persistere, ma sono dati Sprout riletti sotto i
  permission correnti, non ricordi interni autorevoli del modello;
* i fatti strutturati correnti devono essere letti dallo stato autorevole;
* qualunque stato cognitivo persistente user/project-specific appreso dal modello ma
  non rappresentato come source del prodotto e' una violazione del refinement;
* in caso di conflitto fra testo derivato precedente e stato corrente, lo stato
  strutturato corrente e' autorevole per ogni decisione operativa.

D. Boundary semanticamente forti
`PromptContractAdequacy`, `ResponsibilityTextAdequacy`,
`LocalGoalClassificationAdequacy`, `GlobalSynthesisAdequacy` e le nuove optional
semantic-quality relation possono ancora essere assunte per theorem che vogliono
affermare fedelta' intenzionale perfetta. Non sono condizioni del normale path di
permission safety, responsibility authorization o activation operativa R5.37.
-/

end R5

end Sprout.AgentSpec

namespace Sprout.AgentSpec

namespace R5

/-!
R5.40/R5.41 — candidata ricostruita per closure formale di release.

Estensione puramente additiva della baseline con hash
0b7754cf65b92411269be5b1af70d9895d0ad39e0e697482ec4dee9c57cf254b.
Non compilata in questa sessione: deve essere verificata esclusivamente nel
toolchain canonico ~/lean-fixed prima di sostituire la specifica canonica.
-/

/-! ### R5.40 — Trace concreta, content binding e sostituzione cross-trace -/

/-- Evento canonico di un singolo attempt di WorkItem. -/
structure R540WorkAttemptEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  work : X.WorkItemId
  claim : X.ClaimId
  attempt : Nat
  actor : V.PrincipalId
  tick : Nat


/-- Esito terminale di un WorkItem nello stesso attempt certificato. -/
structure R540WorkOutcomeEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  work : X.WorkItemId
  claim : X.ClaimId
  attempt : Nat
  status : WorkStatus
  observedAt : Nat

/-- Risoluzione concreta di un blocker tipato. -/
structure R540BlockerResolutionEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  blocker : X.BlockerId
  resolution : BlockerResolution V X
  observedAt : Nat

/-- Link causale persistito nella medesima trace. -/
structure R540CausalLinkEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  link : CollaborativeCausalLink V X
  recordedAt : Nat

/-- Richiesta/terminale tool legati allo stesso work attempt. -/
structure R540ToolEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  work : X.WorkItemId
  claim : X.ClaimId
  attempt : Nat
  owner : V.PrincipalId
  callId : V.ToolCallId
  tool : V.Tool
  input : V.ToolInput
  status : ToolCallStatus
  output : Option V.ToolOutput
  requestedAt : Nat
  observedAt : Nat

/-- Evidence accettata, content-addressed dal record tipato completo. -/
structure R540EvidenceEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  work : X.WorkItemId
  claim : X.ClaimId
  attempt : Nat
  evidence : Evidence V X
  acceptedAt : Nat

/-- Disclosure prodotta da un effetto concreto. -/
structure R540DisclosureEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  work : X.WorkItemId
  attempt : Nat
  actor : V.PrincipalId
  sink : DisclosureSink V
  sources : List (InformationSource V X)
  payload : V.EncryptedPayload
  observedAt : Nat

/-- Invocation LLM effettiva: input e output cifrati restano parte del record. -/
structure R540ModelInvocationEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  work : X.WorkItemId
  attempt : Nat
  principal : V.PrincipalId
  context : ModelInvocationContext V X
  projection : ModelExposureProjection V X
  inputPayload : V.EncryptedPayload
  outputPayload : V.EncryptedPayload
  invokedAt : Nat

/-- Interrogazione privata completa: question, answer e delta sono nello stesso record. -/
structure R540InterrogationEvent
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  session : AgentInterrogationSession V
  question : AgentInterrogationQuestion V
  answer : AgentInterrogationAnswer V X
  delta : AgentInterrogationCausalDelta V X
  context : ModelInvocationContext V X
  projection : ModelExposureProjection V X
  observedAt : Nat

/-- Trace unica della release/run. Tutti i registri sottostanti portano lo stesso traceId. -/
structure R540ConcreteExecutionTrace
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  id : Nat
  run : X.RunId
  goal : X.GoalId
  startTick : Nat
  endTick : Nat
  ordered : startTick ≤ endTick
  workAttempts : List (R540WorkAttemptEvent V X)
  workOutcomes : List (R540WorkOutcomeEvent V X)
  blockerResolutions : List (R540BlockerResolutionEvent V X)
  causalLinks : List (R540CausalLinkEvent V X)
  toolEvents : List (R540ToolEvent V X)
  evidenceEvents : List (R540EvidenceEvent V X)
  disclosureEvents : List (R540DisclosureEvent V X)
  modelInvocations : List (R540ModelInvocationEvent V X)
  interrogations : List (R540InterrogationEvent V X)

/-- Un evento appartiene temporalmente e nominalmente all'esatta trace. -/
def R540EventWithinTrace
    (traceId : Nat)
    (run : X.RunId)
    (goal : X.GoalId)
    (tick : Nat)
    (trace : R540ConcreteExecutionTrace V X) : Prop :=
  traceId = trace.id ∧
  run = trace.run ∧
  goal = trace.goal ∧
  trace.startTick ≤ tick ∧
  tick ≤ trace.endTick

/-- Il work attempt coincide con stato, lease, claimant e attempt runtime. -/
def R540WorkAttemptEventExact
    (certified : CertifiedCollaborativeRun V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540WorkAttemptEvent V X) : Prop :=
  event ∈ trace.workAttempts ∧
  R540EventWithinTrace event.traceId event.run event.goal event.tick trace ∧
  LogicalClaimValidAt certified event.tick event.claim ∧
  ∃ lease work,
    certified.claimLeaseAt event.tick event.claim = some lease ∧
    lease.claim = event.claim ∧
    lease.work = event.work ∧
    lease.attempt = event.attempt ∧
    lease.claimant = event.actor ∧
    (certified.run.semanticState event.tick).workItems event.work = some work ∧
    work.run = event.run ∧
    work.goal = event.goal ∧
    work.owner = event.actor ∧
    work.attempt = event.attempt


/-- Esito terminale legato all'esatto WorkItem/claim/attempt. -/
def R540WorkOutcomeEventExact
    (certified : CertifiedCollaborativeRun V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540WorkOutcomeEvent V X) : Prop :=
  event ∈ trace.workOutcomes ∧
  R540EventWithinTrace
    event.traceId event.run event.goal event.observedAt trace ∧
  (event.status = WorkStatus.succeeded ∨
   event.status = WorkStatus.failed ∨
   event.status = WorkStatus.cancelled) ∧
  (∃ workEvent,
    R540WorkAttemptEventExact certified trace workEvent ∧
    workEvent.work = event.work ∧
    workEvent.claim = event.claim ∧
    workEvent.attempt = event.attempt ∧
    workEvent.run = event.run ∧
    workEvent.goal = event.goal ∧
    workEvent.tick ≤ event.observedAt) ∧
  ∃ work,
    (certified.run.semanticState event.observedAt).workItems event.work = some work ∧
    work.run = event.run ∧
    work.goal = event.goal ∧
    work.attempt = event.attempt ∧
    work.status = event.status

/-- Il blocker terminale e la sua resolution appartengono alla stessa trace. -/
def R540BlockerResolutionEventExact
    (certified : CertifiedCollaborativeRun V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540BlockerResolutionEvent V X) : Prop :=
  event ∈ trace.blockerResolutions ∧
  R540EventWithinTrace
    event.traceId event.run event.goal event.observedAt trace ∧
  event.resolution.blocker = event.blocker ∧
  event.resolution.observedAt = event.observedAt ∧
  event.resolution ∈
    (certified.run.semanticState event.observedAt).blockerResolutions ∧
  ∃ blocker,
    (certified.run.semanticState event.observedAt).blockers event.blocker =
      some blocker ∧
    blocker.run = event.run ∧
    blocker.goal = event.goal ∧
    BlockerTerminal blocker

/-- Il link causale è osservato nella stessa run/goal e non è retrodatato. -/
def R540CausalLinkEventExact
    (certified : CertifiedCollaborativeRun V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540CausalLinkEvent V X) : Prop :=
  event ∈ trace.causalLinks ∧
  R540EventWithinTrace
    event.traceId event.run event.goal event.recordedAt trace ∧
  event.link.run = event.run ∧
  event.link.goal = event.goal ∧
  event.link.observedAt ≤ event.recordedAt ∧
  event.link ∈ (certified.run.semanticState event.recordedAt).causalLinks ∧
  certified.causalRank event.link.successor <
    certified.causalRank event.link.predecessor

/-- L'evento tool è legato a un attempt certificato e al record tool esatto. -/
def R540ToolEventExact
    (certified : CertifiedCollaborativeRun V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540ToolEvent V X) : Prop :=
  event ∈ trace.toolEvents ∧
  R540EventWithinTrace event.traceId event.run event.goal event.observedAt trace ∧
  event.requestedAt ≤ event.observedAt ∧
  (∃ workEvent,
    R540WorkAttemptEventExact certified trace workEvent ∧
    workEvent.run = event.run ∧
    workEvent.goal = event.goal ∧
    workEvent.work = event.work ∧
    workEvent.claim = event.claim ∧
    workEvent.attempt = event.attempt ∧
    workEvent.actor = event.owner ∧
    workEvent.tick = event.requestedAt) ∧
  ∃ call,
    (certified.run.semanticState event.observedAt).base.toolCalls event.callId = some call ∧
    call.id = event.callId ∧
    call.owner = event.owner ∧
    call.tool = event.tool ∧
    call.input = event.input ∧
    call.attempt = event.attempt ∧
    call.status = event.status ∧
    call.output = event.output

/-- L'evidence appartiene alla trace, è temporalmente causale e valida per il contratto. -/
def R540EvidenceEventExact
    (judge : SemanticEvidenceJudge V X)
    (certified : CertifiedCollaborativeRun V X)
    (contract : GoalContract V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540EvidenceEvent V X) : Prop :=
  event ∈ trace.evidenceEvents ∧
  R540EventWithinTrace
    event.traceId event.run event.goal event.acceptedAt trace ∧
  event.evidence.run = event.run ∧
  event.evidence.observedAt ≤ event.acceptedAt ∧
  event.evidence ∈ (certified.run.semanticState event.acceptedAt).evidences ∧
  ContractEvidenceValid judge certified event.run contract event.evidence ∧
  (∃ work,
    (certified.run.semanticState event.acceptedAt).workItems event.work = some work ∧
    work.run = event.run ∧
    work.goal = event.goal ∧
    work.attempt = event.attempt ∧
    work.serves = event.evidence.obligation) ∧
  ∃ workEvent,
    R540WorkAttemptEventExact certified trace workEvent ∧
    workEvent.work = event.work ∧
    workEvent.claim = event.claim ∧
    workEvent.attempt = event.attempt ∧
    workEvent.run = event.run ∧
    workEvent.goal = event.goal

/-- Projection concreta dei payload osservati nei sink di disclosure. -/
structure R540DisclosurePayloadProjection
    (V : Vocabulary) where
  payloadAt : Nat → DisclosureSink V → Option V.EncryptedPayload

/-- Ledger esterno osservato dal refinement concreto, distinto dalla projection dichiarata. -/
structure R540ActualDisclosureRuntime
    (V : Vocabulary) where
  payloadAt : Nat → DisclosureSink V → Option V.EncryptedPayload

def R540DisclosureProjectionExact
    (actual : R540ActualDisclosureRuntime V)
    (projection : R540DisclosurePayloadProjection V) : Prop :=
  actual.payloadAt = projection.payloadAt

/-- L'effetto disclosure, le source e il payload coincidono con la projection concreta. -/
def R540DisclosureEventExact
    (secured : SecuredCollaborativeRun V X)
    (payloads : R540DisclosurePayloadProjection V)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540DisclosureEvent V X) : Prop :=
  event ∈ trace.disclosureEvents ∧
  R540EventWithinTrace
    event.traceId event.run event.goal event.observedAt trace ∧
  payloads.payloadAt event.observedAt event.sink = some event.payload ∧
  ∃ effect,
    secured.securityEffectAt event.observedAt = some effect ∧
    effect.run = event.run ∧
    effect.actor = event.actor ∧
    effect.work = event.work ∧
    effect.contextSources = event.sources ∧
    effect.disclosure = some event.sink

/-- Projection adapter content-exact dell'effettiva invocation LLM. -/
structure R540ModelRuntimeProjection
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  inputAt : Nat → Option V.EncryptedPayload
  outputAt : Nat → Option V.EncryptedPayload
  principalAt : Nat → Option V.PrincipalId
  contextAt : Nat → Option (ModelInvocationContext V X)

/-- Runtime provider osservato dall'adapter concreto, non una projection autocertificata. -/
structure R540ActualModelRuntime
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  inputAt : Nat → Option V.EncryptedPayload
  outputAt : Nat → Option V.EncryptedPayload
  principalAt : Nat → Option V.PrincipalId
  contextAt : Nat → Option (ModelInvocationContext V X)

def R540ModelRuntimeProjectionExact
    (actual : R540ActualModelRuntime V X)
    (projection : R540ModelRuntimeProjection V X) : Prop :=
  actual.inputAt = projection.inputAt ∧
  actual.outputAt = projection.outputAt ∧
  actual.principalAt = projection.principalAt ∧
  actual.contextAt = projection.contextAt

/-- Il certificato state-grounded è legato agli esatti byte cifrati della invocation. -/
def R540ModelInvocationEventExact
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540ModelRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540ModelInvocationEvent V X) : Prop :=
  event ∈ trace.modelInvocations ∧
  R540EventWithinTrace
    event.traceId event.run event.goal event.invokedAt trace ∧
  runtime.inputAt event.invokedAt = some event.inputPayload ∧
  runtime.outputAt event.invokedAt = some event.outputPayload ∧
  runtime.principalAt event.invokedAt = some event.principal ∧
  runtime.contextAt event.invokedAt = some event.context ∧
  StateGroundedModelInvocationCertificate
    authorization secured event.invokedAt event.principal
    event.context event.projection ∧
  ∃ workEvent,
    R540WorkAttemptEventExact secured.certified trace workEvent ∧
    workEvent.work = event.work ∧
    workEvent.attempt = event.attempt ∧
    workEvent.run = event.run ∧
    workEvent.goal = event.goal

/-- Projection product-side dell'interrogazione realmente persistita. -/
structure R540InterrogationRuntimeProjection
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  sessionAt : Nat → Option (AgentInterrogationSession V)
  questionAt : Nat → Option (AgentInterrogationQuestion V)
  answerAt : Nat → Option (AgentInterrogationAnswer V X)
  deltaAt : Nat → Option (AgentInterrogationCausalDelta V X)

structure R540ActualInterrogationRuntime
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  sessionAt : Nat → Option (AgentInterrogationSession V)
  questionAt : Nat → Option (AgentInterrogationQuestion V)
  answerAt : Nat → Option (AgentInterrogationAnswer V X)
  deltaAt : Nat → Option (AgentInterrogationCausalDelta V X)

def R540InterrogationRuntimeProjectionExact
    (actual : R540ActualInterrogationRuntime V X)
    (projection : R540InterrogationRuntimeProjection V X) : Prop :=
  actual.sessionAt = projection.sessionAt ∧
  actual.questionAt = projection.questionAt ∧
  actual.answerAt = projection.answerAt ∧
  actual.deltaAt = projection.deltaAt

/-- Interrogazione content-exact e read-only sullo stesso record/trace. -/
def R540InterrogationEventExact
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540InterrogationRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540InterrogationEvent V X) : Prop :=
  event ∈ trace.interrogations ∧
  event.traceId = trace.id ∧
  runtime.sessionAt event.observedAt = some event.session ∧
  runtime.questionAt event.observedAt = some event.question ∧
  runtime.answerAt event.observedAt = some event.answer ∧
  runtime.deltaAt event.observedAt = some event.delta ∧
  trace.startTick ≤ event.observedAt ∧
  event.observedAt ≤ trace.endTick ∧
  event.question.sessionId = event.session.id ∧
  event.answer.sessionId = event.session.id ∧
  event.question.askedAt ≤ event.answer.answeredAt ∧
  StateGroundedStrongInterrogationCertificate
    authorization secured event.observedAt event.session event.answer
    event.delta event.context event.projection

/-- Chiusura esatta di tutti gli eventi presenti nella trace. -/
structure R540ConcreteTraceCertificate
    (judge : SemanticEvidenceJudge V X)
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runtime : R540ModelRuntimeProjection V X)
    (payloads : R540DisclosurePayloadProjection V)
    (interrogationRuntime : R540InterrogationRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X) : Prop where
  traceGoalAtStart :
    (secured.certified.run.semanticState trace.startTick).runGoal trace.run =
      some trace.goal
  nonemptyCore : trace.workAttempts ≠ []
  everyWorkExact :
    ∀ event, event ∈ trace.workAttempts →
      R540WorkAttemptEventExact secured.certified trace event
  everyOutcomeExact :
    ∀ event, event ∈ trace.workOutcomes →
      R540WorkOutcomeEventExact secured.certified trace event
  everyBlockerResolutionExact :
    ∀ event, event ∈ trace.blockerResolutions →
      R540BlockerResolutionEventExact secured.certified trace event
  everyCausalLinkExact :
    ∀ event, event ∈ trace.causalLinks →
      R540CausalLinkEventExact secured.certified trace event
  causalHistoryAppendOnly :
    ∀ link,
      link ∈ (secured.certified.run.semanticState trace.startTick).causalLinks →
      link ∈ (secured.certified.run.semanticState trace.endTick).causalLinks
  everyToolExact :
    ∀ event, event ∈ trace.toolEvents →
      R540ToolEventExact secured.certified trace event
  everyEvidenceExact :
    ∀ event, event ∈ trace.evidenceEvents →
      R540EvidenceEventExact judge secured.certified contract trace event
  everyDisclosureExact :
    ∀ event, event ∈ trace.disclosureEvents →
      R540DisclosureEventExact secured payloads trace event
  everyInvocationExact :
    ∀ event, event ∈ trace.modelInvocations →
      R540ModelInvocationEventExact authorization secured runtime trace event
  everyInterrogationExact :
    ∀ event, event ∈ trace.interrogations →
      R540InterrogationEventExact
        authorization secured interrogationRuntime trace event

/-- Un record esatto non può essere riutilizzato sotto un traceId differente. -/
theorem r540_work_event_trace_id_unique
    (certified : CertifiedCollaborativeRun V X)
    (left right : R540ConcreteExecutionTrace V X)
    (event : R540WorkAttemptEvent V X)
    (leftExact : R540WorkAttemptEventExact certified left event)
    (rightExact : R540WorkAttemptEventExact certified right event) :
    left.id = right.id := by
  have hLeft : event.traceId = left.id := leftExact.2.1.1
  have hRight : event.traceId = right.id := rightExact.2.1.1
  exact hLeft.symm.trans hRight

/-- Nessuna sostituzione cross-trace è possibile quando gli ID sono distinti. -/
theorem r540_distinct_traces_cannot_share_exact_work_event
    (certified : CertifiedCollaborativeRun V X)
    (left right : R540ConcreteExecutionTrace V X)
    (event : R540WorkAttemptEvent V X)
    (different : left.id ≠ right.id)
    (leftExact : R540WorkAttemptEventExact certified left event) :
    ¬ R540WorkAttemptEventExact certified right event := by
  intro rightExact
  exact different (r540_work_event_trace_id_unique certified left right event leftExact rightExact)

/-- Ogni invocation concreta certificata esclude memoria persistente occulta. -/
theorem r540_exact_model_invocation_has_no_hidden_memory
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540ModelRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540ModelInvocationEvent V X)
    (exact : R540ModelInvocationEventExact
      authorization secured runtime trace event) :
    ¬ event.projection.hiddenPersistentModelMemoryAvailable := by
  rcases exact with
    ⟨_, _, _, _, _, _, invocationSafe, _⟩
  exact invocationSafe.exposureExact.2

/-- Anche una invocation content-exact non può essere ricertificata sotto una trace distinta. -/
theorem r540_model_event_trace_id_unique
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540ModelRuntimeProjection V X)
    (left right : R540ConcreteExecutionTrace V X)
    (event : R540ModelInvocationEvent V X)
    (leftExact : R540ModelInvocationEventExact
      authorization secured runtime left event)
    (rightExact : R540ModelInvocationEventExact
      authorization secured runtime right event) :
    left.id = right.id := by
  have hLeft : event.traceId = left.id := leftExact.2.1.1
  have hRight : event.traceId = right.id := rightExact.2.1.1
  exact hLeft.symm.trans hRight

/-- Qualunque evento con binding nominale esatto determina un solo traceId. -/
theorem r540_event_binding_trace_id_unique
    (left right : R540ConcreteExecutionTrace V X)
    (traceId : Nat)
    (run : X.RunId)
    (goal : X.GoalId)
    (tick : Nat)
    (leftBound : R540EventWithinTrace traceId run goal tick left)
    (rightBound : R540EventWithinTrace traceId run goal tick right) :
    left.id = right.id := by
  exact leftBound.1.symm.trans rightBound.1

/-- Controesempio cross-trace: due trace distinte non possono certificare lo stesso binding. -/
theorem r540_distinct_traces_reject_same_event_binding
    (left right : R540ConcreteExecutionTrace V X)
    (traceId : Nat)
    (run : X.RunId)
    (goal : X.GoalId)
    (tick : Nat)
    (different : left.id ≠ right.id)
    (leftBound : R540EventWithinTrace traceId run goal tick left) :
    ¬ R540EventWithinTrace traceId run goal tick right := by
  intro rightBound
  exact different
    (r540_event_binding_trace_id_unique
      left right traceId run goal tick leftBound rightBound)

/-! ### R5.41 — Semantic hardening, non-vacuità e root certificate finale -/

/-- Modalità esplicita di una superficie opzionale del prodotto. -/
inductive R541SurfaceMode where
  | enabled
  | disabledFailClosed
  deriving DecidableEq, Repr

/-- Una superficie vuota è accettabile soltanto se dichiarata fail-closed. -/
structure R541SurfaceGate (α : Type u) where
  mode : R541SurfaceMode
  records : List α
  enabledNonempty : mode = R541SurfaceMode.enabled → records ≠ []
  disabledEmpty : mode = R541SurfaceMode.disabledFailClosed → records = []

/-- Nessun claim enabled può essere soddisfatto da una lista vuota. -/
theorem r541_enabled_surface_is_nonempty
    {α : Type u}
    (gate : R541SurfaceGate α)
    (enabled : gate.mode = R541SurfaceMode.enabled) :
    gate.records ≠ [] := by
  exact gate.enabledNonempty enabled

/-! #### R5.41A — Exactness prompt→requirement→WorkSpec→action -/

/--
Chiusura bidirezionale: ogni azione compilata deriva da un requirement e ogni
binding punta a oggetti reali. Questo rafforza, senza sostituirlo, il predicato
`PromptRequirementsAndWorkExact` precedente.
-/
structure R541PromptRequirementsAndWorkExactCertificate
    (compiler : StructuredLocalContractCompiler V X)
    (prompt : V.SystemPrompt)
    (contract : GoalContract V X) : Prop where
  baseExact : PromptRequirementsAndWorkExact compiler prompt contract
  uniqueRequirementIds :
    ∀ left right,
      left ∈ compiler.extractRequirements prompt →
      right ∈ compiler.extractRequirements prompt →
      left.id = right.id →
      left = right
  uniqueBindings :
    ∀ left right,
      left ∈ compiler.bindings prompt →
      right ∈ compiler.bindings prompt →
      left.requirementId = right.requirementId →
      left.obligation = right.obligation →
      left.workSpecId = right.workSpecId →
      left = right
  everyBindingResolved :
    ∀ link,
      link ∈ compiler.bindings prompt →
      ∃ requirement obligationSpec workSpec,
        requirement ∈ compiler.extractRequirements prompt ∧
        requirement.id = link.requirementId ∧
        obligationSpec ∈ contract.obligations ∧
        obligationSpec.id = link.obligation ∧
        workSpec ∈ contract.workSpecs ∧
        workSpec.id = link.workSpecId ∧
        workSpec.obligation = link.obligation
  actionsBidirectionallyExact :
    ∀ workSpec actionClass,
      workSpec ∈ contract.workSpecs →
      (actionClass ∈ workSpec.allowedActions ↔
        ∃ requirement link,
          requirement ∈ compiler.extractRequirements prompt ∧
          link ∈ compiler.bindings prompt ∧
          link.requirementId = requirement.id ∧
          link.workSpecId = workSpec.id ∧
          actionClass ∈ requirement.requiredActions)

/-- Un'azione compilata non può essere introdotta senza requirement sorgente. -/
theorem r541_compiled_action_has_exact_requirement
    (compiler : StructuredLocalContractCompiler V X)
    (prompt : V.SystemPrompt)
    (contract : GoalContract V X)
    (certificate : R541PromptRequirementsAndWorkExactCertificate
      compiler prompt contract)
    (workSpec : ContractWorkSpec V X)
    (actionClass : AgentActionClass)
    (workKnown : workSpec ∈ contract.workSpecs)
    (allowed : actionClass ∈ workSpec.allowedActions) :
    ∃ requirement link,
      requirement ∈ compiler.extractRequirements prompt ∧
      link ∈ compiler.bindings prompt ∧
      link.requirementId = requirement.id ∧
      link.workSpecId = workSpec.id ∧
      actionClass ∈ requirement.requiredActions := by
  exact (certificate.actionsBidirectionallyExact workSpec actionClass workKnown).1 allowed

/-- Ogni azione concreta non-noop possiede una classe nel linguaggio chiuso. -/
theorem r541_non_noop_action_has_class
    (action : AgentAction V)
    (nonNoOp : action ≠ AgentAction.noOp) :
    ∃ actionClass, ActionHasClass action actionClass := by
  cases action with
  | createTask _ => exact ⟨AgentActionClass.createTask, trivial⟩
  | replaceOwnTask _ _ => exact ⟨AgentActionClass.replaceOwnTask, trivial⟩
  | deleteOwnTask _ => exact ⟨AgentActionClass.deleteOwnTask, trivial⟩
  | assignOwnTask _ _ => exact ⟨AgentActionClass.assignOwnTask, trivial⟩
  | unassignOwnTask _ _ => exact ⟨AgentActionClass.unassignOwnTask, trivial⟩
  | markAssignedDone _ => exact ⟨AgentActionClass.markAssignedDone, trivial⟩
  | appendAssignedNote _ _ =>
      exact ⟨AgentActionClass.appendAssignedNote, trivial⟩
  | addAssignedAttachment _ _ =>
      exact ⟨AgentActionClass.addAssignedAttachment, trivial⟩
  | postComment _ => exact ⟨AgentActionClass.postComment, trivial⟩
  | invokeTool _ _ _ => exact ⟨AgentActionClass.invokeTool, trivial⟩
  | retryTool _ => exact ⟨AgentActionClass.retryTool, trivial⟩
  | noOp => exact False.elim (nonNoOp rfl)

/-- La classificazione delle azioni concrete è funzionale. -/
theorem r541_action_class_is_functional
    (action : AgentAction V)
    (left right : AgentActionClass)
    (leftClass : ActionHasClass action left)
    (rightClass : ActionHasClass action right) :
    left = right := by
  cases action <;> cases left <;> cases right <;>
    simp [ActionHasClass] at leftClass rightClass ⊢

/-! #### R5.41B — Compatibilità WorkSpec↔security policy -/

/-- Policy product-side coerente con tutte le azioni ammesse dalla WorkSpec. -/
def R541WorkSpecSecurityPolicyCompatible
    (workSpec : ContractWorkSpec V X)
    (policy : ContractWorkSecurityPolicy V) : Prop :=
  policy.workSpecId = workSpec.id ∧
  (∀ action,
    ContractWorkSpecAllowsAgentAction workSpec action →
    ∀ effect,
      effect ∈ AgentActionCoreSecurityFootprint action →
      effect.operation ∈ policy.allowedOperations) ∧
  (∀ tool input retryPolicy,
    ContractWorkSpecAllowsAgentAction
      workSpec (AgentAction.invokeTool tool input retryPolicy) →
    tool ∈ policy.allowedTools)

/-- Ogni WorkSpec possiede esattamente una policy compatibile. -/
structure R541ContractSecurityPolicyCertificate
    (contract : GoalContract V X)
    (policies : List (ContractWorkSecurityPolicy V)) : Prop where
  everyWorkSpecHasExactPolicy :
    ∀ workSpec,
      workSpec ∈ contract.workSpecs →
      ∃ policy,
        policy ∈ policies ∧
        R541WorkSpecSecurityPolicyCompatible workSpec policy ∧
        ∀ other,
          other ∈ policies →
          other.workSpecId = workSpec.id →
          other = policy
  noOrphanPolicy :
    ∀ policy,
      policy ∈ policies →
      ∃ workSpec,
        workSpec ∈ contract.workSpecs ∧
        workSpec.id = policy.workSpecId

/-- Il footprint core di un'azione consentita è contenuto nella policy esatta. -/
theorem r541_allowed_action_core_footprint_is_policy_bounded
    (workSpec : ContractWorkSpec V X)
    (policy : ContractWorkSecurityPolicy V)
    (compatible : R541WorkSpecSecurityPolicyCompatible workSpec policy)
    (action : AgentAction V)
    (allowed : ContractWorkSpecAllowsAgentAction workSpec action)
    (effect : ResourceSecurityEffect V)
    (effectIn : effect ∈ AgentActionCoreSecurityFootprint action) :
    effect.operation ∈ policy.allowedOperations := by
  exact compatible.2.1 action allowed effect effectIn

/-! #### R5.41C — Creazione/revisione e frame conditions di authority -/

/-- Creazione e activation non concedono permission o tool permission. -/
def R541AuthorityProjectionUnchanged
    (before after : State V) : Prop :=
  before.permissions = after.permissions ∧
  before.toolPermission = after.toolPermission

structure R541LocalRevisionRecord
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  envelope : LocalGoalCompilationEnvelope V
  draft : LocalPromptGoalRevisionDraft V X
  approval : ControllerFinalPromptApproval V

structure R541AgentCreationRecord
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  envelope : LocalGoalCompilationEnvelope V
  proposal : AgentCreationProposal V X
  approval : ControllerFinalPromptApproval V
  before : State V
  after : State V

structure R541ResponsibilityRecord (V : Vocabulary) where
  traceId : Nat
  envelope : ResponsibilityCompilationEnvelope V
  responsibility : ResponsibilityContract V

structure R541GlobalSynthesisRecord
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  envelope : StructuredGlobalSynthesisEnvelope V
  candidate : GlobalContractCandidate V X
  groundings : List (StructuredGlobalWorkGrounding V)

structure R541ProxyPlanRecord (V : Vocabulary) where
  traceId : Nat
  proxy : UserProxyAgent V
  thread : UserProxyChatThread V
  request : UserProxyRequest V
  envelope : UserProxyPlanningEnvelope V
  plan : UserProxyActionPlan V
  confirmation : Option (UserProxyOutOfResponsibilityConfirmation V)

structure R541CrossOwnerRecord (V : Vocabulary) where
  traceId : Nat
  request : CrossOwnerTaskAssignmentRequest V

structure R541CommentRecord
    (V : Vocabulary)
    (X : ExtensionVocabulary V) where
  traceId : Nat
  run : X.RunId
  goal : X.GoalId
  tick : Nat
  comment : Comment V

structure R541TaskIntentRecord (V : Vocabulary) where
  traceId : Nat
  envelope : TaskIntentDerivationEnvelope V
  intent : PersistedTaskIntent V

structure R541TaskProvenanceRecord (V : Vocabulary) where
  traceId : Nat
  provenance : TaskObligationProvenance V

/-- Frame completo della creazione iniziale concreta. -/
structure R541AgentCreationFrameCertificate
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (record : R541AgentCreationRecord V X) : Prop where
  operational :
    OperationalAgentCreationActivationCertificate
      authorization structuredCompiler classifier record.envelope record.before
      responsibilities exceptions globalAssignments
      administratorCreationApprovals record.proposal record.approval
  traceBound : record.traceId ≠ 0
  principalAbsentBefore :
    record.before.principals record.proposal.proposedAgent = none
  principalActiveAfter :
    record.after.principals record.proposal.proposedAgent =
      some PrincipalKind.agent
  exactPromptAfter :
    record.after.systemPrompts record.proposal.proposedAgent =
      some record.proposal.prompt
  noAuthorityGrant : R541AuthorityProjectionUnchanged record.before record.after

/-- La creazione certificata non amplia permission né tool permission. -/
theorem r541_agent_creation_does_not_grant_authority
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (record : R541AgentCreationRecord V X)
    (certificate : R541AgentCreationFrameCertificate
      authorization structuredCompiler classifier responsibilities exceptions
      globalAssignments administratorCreationApprovals record) :
    R541AuthorityProjectionUnchanged record.before record.after := by
  exact certificate.noAuthorityGrant

/-! #### R5.41D — Chiusure operative per tutti i record dichiarati -/

structure R541GovernanceOperationalClosureCertificate
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (locals : LocalGoalDirectory V X)
    (localRevisions : List (R541LocalRevisionRecord V X))
    (creations : List (R541AgentCreationRecord V X))
    (responsibilityRecords : List (R541ResponsibilityRecord V))
    (globalRecords : List (R541GlobalSynthesisRecord V X)) : Prop where
  everyLocalRevisionCertified :
    ∀ record,
      record ∈ localRevisions →
      OperationalLocalRevisionActivationCertificate
        structuredCompiler classifier record.envelope s responsibilities
        exceptions globalAssignments record.draft record.approval
  everyCreationCertified :
    ∀ record,
      record ∈ creations →
      R541AgentCreationFrameCertificate
        authorization structuredCompiler classifier responsibilities exceptions
        globalAssignments administratorCreationApprovals record
  everyResponsibilityCertified :
    ∀ record,
      record ∈ responsibilityRecords →
      OperationalResponsibilityActivationCertificate
        authorization responsibilityCompiler s record.envelope
        record.responsibility
  everyGlobalSynthesisCertified :
    ∀ record,
      record ∈ globalRecords →
      StructuredGlobalSynthesisCertificate
        s responsibilities exceptions locals record.envelope
        record.candidate record.groundings

/-- TaskIntent e provenance sono validi, completi e legati alla trace. -/
structure R541TaskOperationalSurfaceCertificate
    (s : SemanticState V X)
    (locals : LocalGoalDirectory V X)
    (operational : SemanticOperationalState V X)
    (intentGate : R541SurfaceGate (R541TaskIntentRecord V))
    (provenanceGate : R541SurfaceGate (R541TaskProvenanceRecord V)) : Prop where
  everyIntentValid :
    ∀ record,
      record ∈ intentGate.records →
      PersistedTaskIntentWithinEnvelope s.base record.envelope record.intent
  everyProvenanceValid :
    ∀ record,
      record ∈ provenanceGate.records →
      TaskObligationProvenanceValid s locals record.provenance
  everyOperationalIntentRepresented :
    ∀ intent,
      intent ∈ operational.taskIntents ↔
      ∃ record,
        record ∈ intentGate.records ∧
        record.intent = intent
  everyOperationalProvenanceRepresented :
    ∀ provenance,
      provenance ∈ operational.taskObligationProvenance ↔
      ∃ record,
        record ∈ provenanceGate.records ∧
        record.provenance = provenance

/-- Commenti presenti nella trace sono ammissibili e la history è append-only. -/
structure R541CommentSurfaceCertificate
    (secured : SecuredCollaborativeRun V X)
    (trace : R540ConcreteExecutionTrace V X)
    (gate : R541SurfaceGate (R541CommentRecord V X)) : Prop where
  everyCommentExact :
    ∀ record,
      record ∈ gate.records →
      R540EventWithinTrace
        record.traceId record.run record.goal record.tick trace ∧
      record.comment ∈
        (secured.certified.run.semanticState record.tick).base.comments ∧
      CommentAdmissible
        (secured.certified.run.semanticState record.tick).base record.comment
  commentsAppendOnly :
    ∀ comment,
      comment ∈
        (secured.certified.run.semanticState trace.startTick).base.comments →
      comment ∈
        (secured.certified.run.semanticState trace.endTick).base.comments

structure R541ProxySurfaceCertificate
    (authorization : ProductAuthorizationProjection V)
    (toolSecurity : ToolSecuritySemantics V)
    (responsibilityClassifier : UserProxyResponsibilityFootprintClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : State V)
    (gate : R541SurfaceGate (R541ProxyPlanRecord V)) : Prop where
  everyPlanCertified :
    ∀ record,
      record ∈ gate.records →
      UserProxyPlanExecutionCertificate
        authorization toolSecurity responsibilityClassifier responsibilities s
        record.proxy record.thread record.request record.envelope record.plan
        record.confirmation

structure R541GlobalSurfaceCertificate
    (s : State V)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (locals : LocalGoalDirectory V X)
    (gate : R541SurfaceGate (R541GlobalSynthesisRecord V X)) : Prop where
  everyGlobalRecordCertified :
    ∀ record,
      record ∈ gate.records →
      StructuredGlobalSynthesisCertificate
        s responsibilities exceptions locals record.envelope
        record.candidate record.groundings

structure R541InterrogationSurfaceCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540InterrogationRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (gate : R541SurfaceGate (R540InterrogationEvent V X)) : Prop where
  gateMatchesTrace : gate.records = trace.interrogations
  everyInterrogationCertified :
    ∀ record,
      record ∈ gate.records →
      R540InterrogationEventExact authorization secured runtime trace record

structure R541ModelSurfaceCertificate
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540ModelRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (gate : R541SurfaceGate (R540ModelInvocationEvent V X)) : Prop where
  gateMatchesTrace : gate.records = trace.modelInvocations
  everyInvocationCertified :
    ∀ record,
      record ∈ gate.records →
      R540ModelInvocationEventExact authorization secured runtime trace record

/-! #### R5.41E — Gate espliciti, cross-owner e inventario della trace -/

/-- I registri opzionali coincidono esattamente con i registri della trace. -/
structure R541TraceFeatureGateCertificate
    (trace : R540ConcreteExecutionTrace V X)
    (outcomeGate : R541SurfaceGate (R540WorkOutcomeEvent V X))
    (blockerGate : R541SurfaceGate (R540BlockerResolutionEvent V X))
    (causalGate : R541SurfaceGate (R540CausalLinkEvent V X))
    (toolGate : R541SurfaceGate (R540ToolEvent V X))
    (evidenceGate : R541SurfaceGate (R540EvidenceEvent V X))
    (disclosureGate : R541SurfaceGate (R540DisclosureEvent V X)) : Prop where
  outcomesExact : outcomeGate.records = trace.workOutcomes
  blockersExact : blockerGate.records = trace.blockerResolutions
  causalExact : causalGate.records = trace.causalLinks
  toolsExact : toolGate.records = trace.toolEvents
  evidencesExact : evidenceGate.records = trace.evidenceEvents
  disclosuresExact : disclosureGate.records = trace.disclosureEvents

/-- Tutte le richieste cross-owner dichiarate sono instradate da uno dei soli tre rami. -/
structure R541CrossOwnerSurfaceCertificate
    (authorization : ProductAuthorizationProjection V)
    (obligationMeaning : TaskObligationClassificationSemantics V X)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskMeaning : TaskResponsibilityClassificationSemantics V)
    (taskClassifier : TaskResponsibilityClassifier V)
    (responsibilities : ResponsibilityDirectory V)
    (s : SemanticState V X)
    (agents : GovernedAgentDirectory V)
    (locals : LocalGoalDirectory V X)
    (gate : R541SurfaceGate (R541CrossOwnerRecord V)) : Prop where
  everyRequestRouted :
    ∀ record,
      record ∈ gate.records →
      Nonempty
        (CrossOwnerTaskAssignmentRoutingCertificate
          authorization obligationMeaning obligationClassifier
          taskMeaning taskClassifier responsibilities s agents locals record.request)

/-! #### R5.41F — Assumptions esterne separate dalle guarantees interne -/

/--
Assunzioni non dimostrate dal kernel: progress esterno e fedeltà dei boundary
endpoint/provider. Non concedono permission e non sostituiscono i certificate.
-/
structure R541ExternalReleaseAssumptions
    (secured : SecuredCollaborativeRun V X)
    (goal : X.GoalId)
    (start : Nat)
    (promptMeaning : PromptContractSemantics V X)
    (requirementMeaning : PromptRequirementSemantics V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (prompt : V.SystemPrompt)
    (judge : SemanticEvidenceJudge V X)
    (trace : R540ConcreteExecutionTrace V X)
    (actualModel : R540ActualModelRuntime V X)
    (runtime : R540ModelRuntimeProjection V X)
    (actualDisclosure : R540ActualDisclosureRuntime V)
    (payloads : R540DisclosurePayloadProjection V)
    (actualInterrogation : R540ActualInterrogationRuntime V X)
    (interrogationRuntime : R540InterrogationRuntimeProjection V X) : Prop where
  completionBoundary :
    MinimalContractSuccessExternalAssumptions secured.certified.run goal start
  promptContractFaithful :
    PromptContractAdequacy
      promptMeaning structuredCompiler.contractCompiler prompt
  requirementsFaithful :
    requirementMeaning.faithful
      prompt (structuredCompiler.extractRequirements prompt)
  modelProjectionExact : R540ModelRuntimeProjectionExact actualModel runtime
  disclosureProjectionExact :
    R540DisclosureProjectionExact actualDisclosure payloads
  interrogationProjectionExact :
    R540InterrogationRuntimeProjectionExact
      actualInterrogation interrogationRuntime
  externalEvidenceAuthentic :
    ∀ event,
      event ∈ trace.evidenceEvents →
      (event.evidence.kind = EvidenceKind.externalOutcome ∨
       event.evidence.kind = EvidenceKind.derivedFact) →
      judge.adequate
        (structuredCompiler.contractCompiler.compile prompt)
        secured.certified.run event.evidence

/--
Root certificate: ogni certificate locale usa lo stesso contract, run, trace e
inventario. Le superfici opzionali sono enabled/nonempty oppure fail-closed/empty.
-/
structure R541FormalReleaseCertificate
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (judge : SemanticEvidenceJudge V X)
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (globalMeaning : GlobalSynthesisSemantics V X)
    (secured : SecuredCollaborativeRun V X)
    (governed : ResponsibilityGovernedRun V X)
    (runId : X.RunId)
    (prompt : V.SystemPrompt)
    (start : Nat)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (locals : LocalGoalDirectory V X)
    (agents : GovernedAgentDirectory V)
    (runtime : R540ModelRuntimeProjection V X)
    (payloads : R540DisclosurePayloadProjection V)
    (interrogationRuntime : R540InterrogationRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (policies : List (ContractWorkSecurityPolicy V))
    (localRevisions : List (R541LocalRevisionRecord V X))
    (creations : List (R541AgentCreationRecord V X))
    (responsibilityRecords : List (R541ResponsibilityRecord V))
    (globalRecords : List (R541GlobalSynthesisRecord V X))
    (commentGate : R541SurfaceGate (R541CommentRecord V X))
    (proxyGate : R541SurfaceGate (R541ProxyPlanRecord V))
    (globalGate : R541SurfaceGate (R541GlobalSynthesisRecord V X))
    (crossOwnerGate : R541SurfaceGate (R541CrossOwnerRecord V))
    (outcomeGate : R541SurfaceGate (R540WorkOutcomeEvent V X))
    (blockerGate : R541SurfaceGate (R540BlockerResolutionEvent V X))
    (causalGate : R541SurfaceGate (R540CausalLinkEvent V X))
    (toolGate : R541SurfaceGate (R540ToolEvent V X))
    (evidenceGate : R541SurfaceGate (R540EvidenceEvent V X))
    (disclosureGate : R541SurfaceGate (R540DisclosureEvent V X))
    (interrogationGate : R541SurfaceGate (R540InterrogationEvent V X))
    (modelGate : R541SurfaceGate (R540ModelInvocationEvent V X))
    (taskIntentGate : R541SurfaceGate (R541TaskIntentRecord V))
    (taskProvenanceGate : R541SurfaceGate (R541TaskProvenanceRecord V))
    (toolSecurity : ToolSecuritySemantics V)
    (proxyClassifier : UserProxyResponsibilityFootprintClassifier V)
    (obligationMeaning : TaskObligationClassificationSemantics V X)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskMeaning : TaskResponsibilityClassificationSemantics V)
    (taskClassifier : TaskResponsibilityClassifier V)
    (operationalBefore operationalAfter : SemanticOperationalState V X)
    (proxyDirectory : UserProxyDirectory V)
    (languageTasks : List StructuredLanguageTaskEnvelope)
    (languageRuntime : StructuredLanguageModelRuntimeBoundary) : Prop where
  runGoalExact :
    trace.run = runId ∧
    trace.goal = (structuredCompiler.contractCompiler.compile prompt).goal.id
  traceStartExact : trace.startTick = start
  governedRunExact : governed.toObservedSemanticRun = secured.certified.run
  secureKernel :
    SecureAssumptionMinimalFullSuccessKernelCertificate
      measure policy judge authorization secured runId
      (structuredCompiler.contractCompiler.compile prompt) start
  governanceKernel :
    ResponsibilityGovernanceKernelCertificate
      authorization responsibilityCompiler responsibilityMeaning promptMeaning
      structuredCompiler.contractCompiler classificationMeaning classifier
      globalMeaning governed
  concreteTrace :
    R540ConcreteTraceCertificate
      judge authorization secured
      (structuredCompiler.contractCompiler.compile prompt)
      runtime payloads interrogationRuntime trace
  traceFeatureGates :
    R541TraceFeatureGateCertificate trace outcomeGate blockerGate causalGate
      toolGate evidenceGate disclosureGate
  compilerActionExact :
    R541PromptRequirementsAndWorkExactCertificate
      structuredCompiler prompt
      (structuredCompiler.contractCompiler.compile prompt)
  securityPoliciesExact :
    R541ContractSecurityPolicyCertificate
      (structuredCompiler.contractCompiler.compile prompt) policies
  governanceOperational :
    R541GovernanceOperationalClosureCertificate
      authorization structuredCompiler classifier responsibilityCompiler
      (secured.certified.run.semanticState start).base responsibilities
      exceptions globalAssignments administratorCreationApprovals locals
      localRevisions creations responsibilityRecords globalRecords
  localRevisionTraceBound :
    ∀ record, record ∈ localRevisions → record.traceId = trace.id
  creationTraceBound :
    ∀ record, record ∈ creations → record.traceId = trace.id
  responsibilityTraceBound :
    ∀ record, record ∈ responsibilityRecords → record.traceId = trace.id
  globalTraceBound :
    ∀ record, record ∈ globalRecords → record.traceId = trace.id
  proxyTraceBound :
    ∀ record, record ∈ proxyGate.records → record.traceId = trace.id
  crossOwnerTraceBound :
    ∀ record, record ∈ crossOwnerGate.records → record.traceId = trace.id
  comments : R541CommentSurfaceCertificate secured trace commentGate
  proxy :
    R541ProxySurfaceCertificate
      authorization toolSecurity proxyClassifier responsibilities
      (secured.certified.run.semanticState start).base proxyGate
  globalInventoryExact : globalGate.records = globalRecords
  global :
    R541GlobalSurfaceCertificate
      (secured.certified.run.semanticState start).base responsibilities
      exceptions locals globalGate
  crossOwner :
    R541CrossOwnerSurfaceCertificate
      authorization obligationMeaning obligationClassifier taskMeaning
      taskClassifier responsibilities
      (secured.certified.run.semanticState start) agents locals crossOwnerGate
  interrogation :
    R541InterrogationSurfaceCertificate
      authorization secured interrogationRuntime trace interrogationGate
  model :
    R541ModelSurfaceCertificate authorization secured runtime trace modelGate
  taskOperational :
    R541TaskOperationalSurfaceCertificate
      (secured.certified.run.semanticState start) locals operationalAfter
      taskIntentGate taskProvenanceGate
  taskIntentTraceBound :
    ∀ record, record ∈ taskIntentGate.records → record.traceId = trace.id
  taskProvenanceTraceBound :
    ∀ record, record ∈ taskProvenanceGate.records → record.traceId = trace.id
  operationalHistory :
    SemanticOperationalStateExtends operationalBefore operationalAfter
  operationalClosure :
    SemanticOperationalClosureCertificate
      authorization (secured.certified.run.semanticState start).base
      proxyDirectory languageTasks languageRuntime

/-- Garanzie complete estratte dal root certificate. -/
structure R541ReleaseGuarantees
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runId : X.RunId)
    (contract : GoalContract V X)
    (start : Nat)
    (trace : R540ConcreteExecutionTrace V X)
    (policies : List (ContractWorkSecurityPolicy V))
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (prompt : V.SystemPrompt)
    (operationalBefore operationalAfter : SemanticOperationalState V X) : Prop where
  eventualCompletion :
    EventuallyCollaborativeContractCompleted
      secured.certified.run runId contract start
  authorityInformationSafety :
    AuthorityInformationSafetyHolds authorization secured contract runId start
  concreteTraceNonempty : trace.workAttempts ≠ []
  promptWorkActionsExact :
    R541PromptRequirementsAndWorkExactCertificate
      structuredCompiler prompt contract
  securityPoliciesExact :
    R541ContractSecurityPolicyCertificate contract policies
  operationalHistoryAppendOnly :
    SemanticOperationalStateExtends operationalBefore operationalAfter
  noHiddenPersistentModelMemory :
    ∀ event,
      event ∈ trace.modelInvocations →
      ¬ event.projection.hiddenPersistentModelMemoryAvailable

/-- Il root certificate è non vacuo: contiene almeno un work attempt concreto. -/
theorem r541_formal_release_is_nonvacuous
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (judge : SemanticEvidenceJudge V X)
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (globalMeaning : GlobalSynthesisSemantics V X)
    (secured : SecuredCollaborativeRun V X)
    (governed : ResponsibilityGovernedRun V X)
    (runId : X.RunId)
    (prompt : V.SystemPrompt)
    (start : Nat)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (locals : LocalGoalDirectory V X)
    (agents : GovernedAgentDirectory V)
    (runtime : R540ModelRuntimeProjection V X)
    (payloads : R540DisclosurePayloadProjection V)
    (interrogationRuntime : R540InterrogationRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (policies : List (ContractWorkSecurityPolicy V))
    (localRevisions : List (R541LocalRevisionRecord V X))
    (creations : List (R541AgentCreationRecord V X))
    (responsibilityRecords : List (R541ResponsibilityRecord V))
    (globalRecords : List (R541GlobalSynthesisRecord V X))
    (commentGate : R541SurfaceGate (R541CommentRecord V X))
    (proxyGate : R541SurfaceGate (R541ProxyPlanRecord V))
    (globalGate : R541SurfaceGate (R541GlobalSynthesisRecord V X))
    (crossOwnerGate : R541SurfaceGate (R541CrossOwnerRecord V))
    (outcomeGate : R541SurfaceGate (R540WorkOutcomeEvent V X))
    (blockerGate : R541SurfaceGate (R540BlockerResolutionEvent V X))
    (causalGate : R541SurfaceGate (R540CausalLinkEvent V X))
    (toolGate : R541SurfaceGate (R540ToolEvent V X))
    (evidenceGate : R541SurfaceGate (R540EvidenceEvent V X))
    (disclosureGate : R541SurfaceGate (R540DisclosureEvent V X))
    (interrogationGate : R541SurfaceGate (R540InterrogationEvent V X))
    (modelGate : R541SurfaceGate (R540ModelInvocationEvent V X))
    (taskIntentGate : R541SurfaceGate (R541TaskIntentRecord V))
    (taskProvenanceGate : R541SurfaceGate (R541TaskProvenanceRecord V))
    (toolSecurity : ToolSecuritySemantics V)
    (proxyClassifier : UserProxyResponsibilityFootprintClassifier V)
    (obligationMeaning : TaskObligationClassificationSemantics V X)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskMeaning : TaskResponsibilityClassificationSemantics V)
    (taskClassifier : TaskResponsibilityClassifier V)
    (operationalBefore operationalAfter : SemanticOperationalState V X)
    (proxyDirectory : UserProxyDirectory V)
    (languageTasks : List StructuredLanguageTaskEnvelope)
    (languageRuntime : StructuredLanguageModelRuntimeBoundary)
    (certificate : R541FormalReleaseCertificate
      measure policy judge authorization structuredCompiler classifier
      responsibilityCompiler responsibilityMeaning promptMeaning
      classificationMeaning globalMeaning secured governed runId prompt start
      responsibilities exceptions globalAssignments administratorCreationApprovals
      locals agents runtime payloads interrogationRuntime trace policies localRevisions creations
      responsibilityRecords globalRecords commentGate proxyGate globalGate
      crossOwnerGate outcomeGate blockerGate causalGate toolGate evidenceGate
      disclosureGate interrogationGate modelGate taskIntentGate
      taskProvenanceGate toolSecurity proxyClassifier
      obligationMeaning obligationClassifier taskMeaning taskClassifier
      operationalBefore operationalAfter proxyDirectory languageTasks
      languageRuntime) :
    trace.workAttempts ≠ [] := by
  exact certificate.concreteTrace.nonemptyCore

/-- Una invocation certificata dal root non dispone di memoria persistente occulta. -/
theorem r541_root_model_invocation_has_no_hidden_memory
    (judge : SemanticEvidenceJudge V X)
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (contract : GoalContract V X)
    (runtime : R540ModelRuntimeProjection V X)
    (payloads : R540DisclosurePayloadProjection V)
    (interrogationRuntime : R540InterrogationRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (traceCertificate : R540ConcreteTraceCertificate
      judge authorization secured contract runtime payloads
      interrogationRuntime trace)
    (event : R540ModelInvocationEvent V X)
    (eventIn : event ∈ trace.modelInvocations) :
    ¬ event.projection.hiddenPersistentModelMemoryAvailable := by
  exact r540_exact_model_invocation_has_no_hidden_memory
    authorization secured runtime trace event
    (traceCertificate.everyInvocationExact event eventIn)

/--
Teorema generale: dal root certificate e dalle sole assumptions esterne dichiarate
seguono completion, safety, exactness, append-only e no-model-memory.
-/
theorem sprout_r5_41_formal_release
    (measure : ProgressMeasure V X)
    (policy : AgingSchedulerPolicy)
    (judge : SemanticEvidenceJudge V X)
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilityCompiler : ResponsibilityCompiler V)
    (responsibilityMeaning : ResponsibilityTextSemantics V)
    (promptMeaning : PromptContractSemantics V X)
    (requirementMeaning : PromptRequirementSemantics V)
    (classificationMeaning : LocalGoalClassificationSemantics V X)
    (globalMeaning : GlobalSynthesisSemantics V X)
    (secured : SecuredCollaborativeRun V X)
    (governed : ResponsibilityGovernedRun V X)
    (runId : X.RunId)
    (prompt : V.SystemPrompt)
    (start : Nat)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (locals : LocalGoalDirectory V X)
    (agents : GovernedAgentDirectory V)
    (runtime : R540ModelRuntimeProjection V X)
    (payloads : R540DisclosurePayloadProjection V)
    (interrogationRuntime : R540InterrogationRuntimeProjection V X)
    (actualModel : R540ActualModelRuntime V X)
    (actualDisclosure : R540ActualDisclosureRuntime V)
    (actualInterrogation : R540ActualInterrogationRuntime V X)
    (trace : R540ConcreteExecutionTrace V X)
    (policies : List (ContractWorkSecurityPolicy V))
    (localRevisions : List (R541LocalRevisionRecord V X))
    (creations : List (R541AgentCreationRecord V X))
    (responsibilityRecords : List (R541ResponsibilityRecord V))
    (globalRecords : List (R541GlobalSynthesisRecord V X))
    (commentGate : R541SurfaceGate (R541CommentRecord V X))
    (proxyGate : R541SurfaceGate (R541ProxyPlanRecord V))
    (globalGate : R541SurfaceGate (R541GlobalSynthesisRecord V X))
    (crossOwnerGate : R541SurfaceGate (R541CrossOwnerRecord V))
    (outcomeGate : R541SurfaceGate (R540WorkOutcomeEvent V X))
    (blockerGate : R541SurfaceGate (R540BlockerResolutionEvent V X))
    (causalGate : R541SurfaceGate (R540CausalLinkEvent V X))
    (toolGate : R541SurfaceGate (R540ToolEvent V X))
    (evidenceGate : R541SurfaceGate (R540EvidenceEvent V X))
    (disclosureGate : R541SurfaceGate (R540DisclosureEvent V X))
    (interrogationGate : R541SurfaceGate (R540InterrogationEvent V X))
    (modelGate : R541SurfaceGate (R540ModelInvocationEvent V X))
    (taskIntentGate : R541SurfaceGate (R541TaskIntentRecord V))
    (taskProvenanceGate : R541SurfaceGate (R541TaskProvenanceRecord V))
    (toolSecurity : ToolSecuritySemantics V)
    (proxyClassifier : UserProxyResponsibilityFootprintClassifier V)
    (obligationMeaning : TaskObligationClassificationSemantics V X)
    (obligationClassifier : TaskObligationClassifier V X)
    (taskMeaning : TaskResponsibilityClassificationSemantics V)
    (taskClassifier : TaskResponsibilityClassifier V)
    (operationalBefore operationalAfter : SemanticOperationalState V X)
    (proxyDirectory : UserProxyDirectory V)
    (languageTasks : List StructuredLanguageTaskEnvelope)
    (languageRuntime : StructuredLanguageModelRuntimeBoundary)
    (certificate : R541FormalReleaseCertificate
      measure policy judge authorization structuredCompiler classifier
      responsibilityCompiler responsibilityMeaning promptMeaning
      classificationMeaning globalMeaning secured governed runId prompt start
      responsibilities exceptions globalAssignments administratorCreationApprovals
      locals agents runtime payloads interrogationRuntime trace policies localRevisions creations
      responsibilityRecords globalRecords commentGate proxyGate globalGate
      crossOwnerGate outcomeGate blockerGate causalGate toolGate evidenceGate
      disclosureGate interrogationGate modelGate taskIntentGate
      taskProvenanceGate toolSecurity proxyClassifier
      obligationMeaning obligationClassifier taskMeaning taskClassifier
      operationalBefore operationalAfter proxyDirectory languageTasks
      languageRuntime)
    (assumptions : R541ExternalReleaseAssumptions
      secured (structuredCompiler.contractCompiler.compile prompt).goal.id start
      promptMeaning requirementMeaning structuredCompiler prompt judge trace
      actualModel runtime actualDisclosure payloads
      actualInterrogation interrogationRuntime) :
    R541ReleaseGuarantees
      authorization secured runId
      (structuredCompiler.contractCompiler.compile prompt) start trace policies
      structuredCompiler prompt operationalBefore operationalAfter := by
  have secure :=
    sprout_secure_assumption_minimal_successful_completion
      measure policy judge authorization structuredCompiler.contractCompiler prompt
      secured runId start certificate.secureKernel assumptions.completionBoundary
  refine
    { eventualCompletion := secure.1
      authorityInformationSafety := secure.2
      concreteTraceNonempty := certificate.concreteTrace.nonemptyCore
      promptWorkActionsExact := certificate.compilerActionExact
      securityPoliciesExact := certificate.securityPoliciesExact
      operationalHistoryAppendOnly := certificate.operationalHistory
      noHiddenPersistentModelMemory := ?_ }
  intro event eventIn
  exact r540_exact_model_invocation_has_no_hidden_memory
    authorization secured runtime trace event
    (certificate.concreteTrace.everyInvocationExact event eventIn)

/-! ### R5.41G — Controesempi espliciti di vacuità e sostituzione -/

/-- Una superficie enabled non può essere certificata con inventario vuoto. -/
theorem r541_counterexample_enabled_empty_surface_impossible
    {α : Type u}
    (gate : R541SurfaceGate α)
    (enabled : gate.mode = R541SurfaceMode.enabled)
    (empty : gate.records = []) : False := by
  exact gate.enabledNonempty enabled empty

/-- Una superficie disabilitata fail-closed non può contenere un record operativo. -/
theorem r541_counterexample_disabled_surface_record_impossible
    {α : Type u}
    (gate : R541SurfaceGate α)
    (disabled : gate.mode = R541SurfaceMode.disabledFailClosed)
    (record : α)
    (present : record ∈ gate.records) : False := by
  have empty : gate.records = [] := gate.disabledEmpty disabled
  rw [empty] at present
  simp at present

/-- Il compiler non può aggiungere una action class priva di requirement sorgente. -/
theorem r541_counterexample_unrequested_compiled_action_impossible
    (compiler : StructuredLocalContractCompiler V X)
    (prompt : V.SystemPrompt)
    (contract : GoalContract V X)
    (certificate : R541PromptRequirementsAndWorkExactCertificate
      compiler prompt contract)
    (workSpec : ContractWorkSpec V X)
    (actionClass : AgentActionClass)
    (workKnown : workSpec ∈ contract.workSpecs)
    (noRequirement :
      ¬ ∃ requirement link,
        requirement ∈ compiler.extractRequirements prompt ∧
        link ∈ compiler.bindings prompt ∧
        link.requirementId = requirement.id ∧
        link.workSpecId = workSpec.id ∧
        actionClass ∈ requirement.requiredActions) :
    actionClass ∉ workSpec.allowedActions := by
  intro allowed
  exact noRequirement
    (r541_compiled_action_has_exact_requirement
      compiler prompt contract certificate workSpec actionClass workKnown allowed)

/-- Un certificato di creazione non può nascondere un nuovo grant. -/
theorem r541_counterexample_creation_permission_escalation_impossible
    (authorization : ProductAuthorizationProjection V)
    (structuredCompiler : StructuredLocalContractCompiler V X)
    (classifier : LocalGoalClassifier V X)
    (responsibilities : ResponsibilityDirectory V)
    (exceptions : List (ApprovedLocalGoalException V X))
    (globalAssignments : List (GlobalMandateAssignment V X))
    (administratorCreationApprovals :
      List (ApprovedAdministratorAgentCreation V X))
    (record : R541AgentCreationRecord V X)
    (certificate : R541AgentCreationFrameCertificate
      authorization structuredCompiler classifier responsibilities exceptions
      globalAssignments administratorCreationApprovals record)
    (permissionChanged :
      record.before.permissions ≠ record.after.permissions) : False := by
  exact permissionChanged certificate.noAuthorityGrant.1

/-- La stessa invocation content-exact non può appartenere a due trace distinte. -/
theorem r541_counterexample_cross_trace_model_substitution_impossible
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540ModelRuntimeProjection V X)
    (left right : R540ConcreteExecutionTrace V X)
    (event : R540ModelInvocationEvent V X)
    (different : left.id ≠ right.id)
    (leftExact : R540ModelInvocationEventExact
      authorization secured runtime left event) :
    ¬ R540ModelInvocationEventExact authorization secured runtime right event := by
  intro rightExact
  exact different
    (r540_model_event_trace_id_unique
      authorization secured runtime left right event leftExact rightExact)

/-- Un modello certificato non può dichiarare memoria persistente occulta. -/
theorem r541_counterexample_hidden_model_memory_impossible
    (authorization : ProductAuthorizationProjection V)
    (secured : SecuredCollaborativeRun V X)
    (runtime : R540ModelRuntimeProjection V X)
    (trace : R540ConcreteExecutionTrace V X)
    (event : R540ModelInvocationEvent V X)
    (exact : R540ModelInvocationEventExact
      authorization secured runtime trace event)
    (hidden : event.projection.hiddenPersistentModelMemoryAvailable) : False := by
  exact (r540_exact_model_invocation_has_no_hidden_memory
    authorization secured runtime trace event exact) hidden

end R5

end Sprout.AgentSpec
