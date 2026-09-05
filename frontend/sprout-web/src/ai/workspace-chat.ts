import type { Uuid } from '../api/contracts'
import type { DecryptedTask } from '../domain/models'
import type { BoardMember, ProjectItem, TaskListItem, TopicItem } from '../store/app-store'
import {
  browserDirectInferenceAllowed,
  resolveLocalEdgeInferenceBridge,
  type LocalEdgeInferenceBridge,
} from './execution-boundary'
import { providerForLocalProfile } from './providers'
import type {
  InformationSource,
  JsonSchema,
  LocalAiProfile,
  ProviderGenerationRequest,
} from './contracts'
import { LocalAiProfileStore } from './profile'

const HISTORY_LIMIT = 30
const HISTORY_CONTEXT_LIMIT = 12
const MAX_TURN_CHARACTERS = 8_000
const HISTORY_STORAGE_BYTES = 60_000
const SOURCE_LIMIT = 200
const SOURCE_CHARACTER_LIMIT = 120_000
const CHAT_SETTING_PREFIX = 'device:workspace-ai-chat-v1:'

export interface WorkspaceChatTurn {
  id: Uuid
  role: 'user' | 'assistant'
  content: string
  createdAt: string
  proposal?: WorkspaceActionProposal
}

export type WorkspaceActionKind =
  | 'create_topic'
  | 'rename_topic'
  | 'toggle_topic_favorite'
  | 'delete_topic'
  | 'create_task_list'
  | 'rename_task_list'
  | 'create_task'
  | 'update_task'
  | 'complete_task'
  | 'copy_task'
  | 'assign_task'
  | 'invite_member'
  | 'update_member_responsibilities'

export type WorkspaceActionStatus = 'pending' | 'executing' | 'executed' | 'cancelled' | 'failed'

/**
 * A product-resolved, bounded UserProxy plan. The model may select only one
 * action and identifiers already present in the workspace snapshot; this
 * object is the exact value shown for one-shot confirmation.
 */
export interface WorkspaceActionProposal {
  id: Uuid
  requestId: Uuid
  kind: WorkspaceActionKind
  targetId: Uuid
  title: string
  notes: string
  priority: '' | 'low' | 'normal' | 'high'
  assigneeIdentityId: Uuid | ''
  name: string
  email: string
  role: '' | 'admin' | 'member' | 'guest'
  summary: string
  status: WorkspaceActionStatus
  error?: string
}

export interface WorkspaceSnapshot {
  project: ProjectItem
  topics: TopicItem[]
  taskLists: TaskListItem[]
  tasks: DecryptedTask[]
  members?: BoardMember[]
}

export interface WorkspaceAgentTarget {
  agentId: Uuid
  principalIdentityId: Uuid
  identityHandle: string
}

export interface WorkspaceChatAvailability {
  profileConfigured: boolean
  runtimeConnected: boolean
  model?: string
}

interface WorkspaceChatVault {
  getLocalSetting(key: string): string | undefined
  putLocalSetting(key: string, value: string): Promise<boolean>
  deleteLocalSetting(key: string): Promise<boolean>
}

const ANSWER_SCHEMA: JsonSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['answer'],
  properties: {
    answer: { type: 'string', minLength: 1, maxLength: 32768 },
  },
}

const PROXY_ACTION_KINDS: WorkspaceActionKind[] = [
  'create_topic',
  'rename_topic',
  'toggle_topic_favorite',
  'delete_topic',
  'create_task_list',
  'rename_task_list',
  'create_task',
  'update_task',
  'complete_task',
  'copy_task',
  'assign_task',
  'invite_member',
  'update_member_responsibilities',
]

const PROXY_SCHEMA: JsonSchema = {
  type: 'object',
  additionalProperties: false,
  required: [
    'answer',
    'action_type',
    'target_id',
    'title',
    'notes',
    'priority',
    'assignee_identity_id',
    'name',
    'email',
    'role',
  ],
  properties: {
    answer: { type: 'string', maxLength: 32768 },
    action_type: { type: 'string', enum: ['none', ...PROXY_ACTION_KINDS] },
    target_id: { type: 'string' },
    title: { type: 'string', maxLength: 500 },
    notes: { type: 'string', maxLength: 8000 },
    priority: { type: 'string', enum: ['', 'low', 'normal', 'high'] },
    assignee_identity_id: { type: 'string' },
    name: { type: 'string', maxLength: 500 },
    email: { type: 'string', maxLength: 320 },
    role: { type: 'string', enum: ['', 'admin', 'member', 'guest'] },
  },
}

const PROXY_OUTPUT_KEYS = PROXY_SCHEMA.required.slice().sort()

const resourceSource = (
  resourceId: Uuid,
  value: unknown,
): { descriptor: InformationSource; plaintext: string } => ({
  descriptor: { kind: 'resource_body', resource_id: resourceId },
  plaintext: JSON.stringify(value),
})

/**
 * Creates a bounded projection from plaintext that the signed-in user has
 * already decrypted. Locked records never enter the provider context.
 */
export const buildWorkspaceSources = (
  snapshot: WorkspaceSnapshot,
): ProviderGenerationRequest['sources'] => {
  const projectId = snapshot.project.wire.id
  const topicById = new Map(snapshot.topics.map((topic) => [topic.wire.id, topic]))
  const listById = new Map(snapshot.taskLists.map((list) => [list.wire.id, list]))
  const candidates: ProviderGenerationRequest['sources'] = []

  if (snapshot.project.document) {
    candidates.push(resourceSource(snapshot.project.wire.root_resource_id, {
      type: 'project',
      project_id: projectId,
      name: snapshot.project.document.name,
    }))
  }
  for (const topic of snapshot.topics) {
    if (!topic.document || topic.wire.project_id !== projectId) continue
    candidates.push(resourceSource(topic.wire.resource_node_id, {
      type: 'topic',
      topic_id: topic.wire.id,
      name: topic.document.name,
      favorite: Boolean(topic.document.favorite),
    }))
  }
  for (const list of snapshot.taskLists) {
    if (!list.document || list.wire.project_id !== projectId) continue
    candidates.push(resourceSource(list.wire.resource_node_id, {
      type: 'task_list',
      task_list_id: list.wire.id,
      name: list.document.name,
      topic_id: list.wire.topic_id,
      topic_name: topicById.get(list.wire.topic_id)?.document?.name,
    }))
  }
  for (const task of snapshot.tasks) {
    if (task.wire.project_id !== projectId) continue
    const list = listById.get(task.wire.list_id)
    candidates.push(resourceSource(task.wire.resource_node_id, {
      type: 'task',
      task_id: task.wire.id,
      title: task.document.title,
      notes: task.document.notes,
      state: task.wire.state,
      priority: task.document.priority,
      start_at: task.document.start_at,
      due_at: task.document.due_at,
      recurrence: task.document.recurrence,
      assignee_identity_id: task.wire.active_assignee_identity_id,
      task_list_id: task.wire.list_id,
      task_list_name: list?.document?.name,
      topic_id: list?.wire.topic_id,
      topic_name: list ? topicById.get(list.wire.topic_id)?.document?.name : undefined,
    }))
  }
  if (snapshot.members?.length) {
    candidates.push(resourceSource(snapshot.project.wire.root_resource_id, {
      type: 'project_members',
      members: snapshot.members.map((member) => ({
        identity_id: member.identityId,
        label: member.label,
        email: member.email,
        role: member.role,
        responsibilities: member.responsibilities,
      })),
    }))
  }

  const bounded: ProviderGenerationRequest['sources'] = []
  let characterCount = 0
  for (const source of candidates) {
    if (bounded.length >= SOURCE_LIMIT) break
    if (characterCount + source.plaintext.length > SOURCE_CHARACTER_LIMIT) break
    bounded.push(source)
    characterCount += source.plaintext.length
  }
  return bounded
}

const historyKey = (projectId: Uuid, channel = 'workspace'): string =>
  `${CHAT_SETTING_PREFIX}${projectId}:${channel}`

const legacyWorkspaceHistoryKey = (projectId: Uuid): string =>
  `${CHAT_SETTING_PREFIX}${projectId}`

const isTurn = (value: unknown): value is WorkspaceChatTurn => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const turn = value as Record<string, unknown>
  return (
    typeof turn.id === 'string' &&
    (turn.role === 'user' || turn.role === 'assistant') &&
    typeof turn.content === 'string' &&
    typeof turn.createdAt === 'string' &&
    (turn.proposal === undefined || isProposal(turn.proposal))
  )
}

const parseAnswer = (value: unknown): string => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Il provider AI ha restituito una risposta non valida.')
  }
  const answer = (value as Record<string, unknown>).answer
  if (typeof answer !== 'string' || !answer.trim()) {
    throw new Error('Il provider AI non ha restituito una risposta testuale.')
  }
  return answer.trim().slice(0, MAX_TURN_CHARACTERS)
}

const isUuid = (value: unknown): value is Uuid =>
  typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)

const isProposal = (value: unknown): value is WorkspaceActionProposal => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const proposal = value as Record<string, unknown>
  return (
    isUuid(proposal.id) &&
    isUuid(proposal.requestId) &&
    PROXY_ACTION_KINDS.includes(proposal.kind as WorkspaceActionKind) &&
    isUuid(proposal.targetId) &&
    typeof proposal.title === 'string' &&
    typeof proposal.notes === 'string' &&
    ['', 'low', 'normal', 'high'].includes(String(proposal.priority)) &&
    (proposal.assigneeIdentityId === '' || isUuid(proposal.assigneeIdentityId)) &&
    typeof proposal.name === 'string' &&
    typeof proposal.email === 'string' &&
    ['', 'admin', 'member', 'guest'].includes(String(proposal.role)) &&
    typeof proposal.summary === 'string' &&
    ['pending', 'executing', 'executed', 'cancelled', 'failed'].includes(String(proposal.status)) &&
    (proposal.error === undefined || typeof proposal.error === 'string')
  )
}

const valueString = (record: Record<string, unknown>, key: string): string => {
  const value = record[key]
  if (typeof value !== 'string') {
    throw new Error('Il provider AI ha restituito un piano azione non valido.')
  }
  return value.trim()
}

const findGroundedTarget = <T extends { wire: { id: Uuid; resource_node_id: Uuid }; document?: { name?: string } }>(
  candidates: T[],
  targetId: string,
  question: string,
): T | undefined => {
  const exact = candidates.find((candidate) =>
    candidate.wire.id === targetId || candidate.wire.resource_node_id === targetId)
  if (exact) return exact
  const normalizedQuestion = question.toLocaleLowerCase()
  const mentioned = candidates.filter((candidate) => {
    const name = candidate.document?.name?.trim().toLocaleLowerCase()
    return Boolean(name && normalizedQuestion.includes(name))
  })
  if (mentioned.length === 1) return mentioned[0]
  return undefined
}

const findGroundedTask = (
  snapshot: WorkspaceSnapshot,
  targetId: string,
  question: string,
): DecryptedTask | undefined => {
  const exact = snapshot.tasks.find((task) =>
    task.wire.id === targetId || task.wire.resource_node_id === targetId)
  if (exact) return exact
  const normalizedQuestion = question.toLocaleLowerCase()
  const mentioned = snapshot.tasks.filter((task) =>
    normalizedQuestion.includes(task.document.title.trim().toLocaleLowerCase()))
  return mentioned.length === 1 ? mentioned[0] : undefined
}

const parseProxyResult = (
  value: unknown,
  snapshot: WorkspaceSnapshot,
  question: string,
  requestId: Uuid,
): { answer: string; proposal?: WorkspaceActionProposal } => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Il provider AI ha restituito una risposta non valida.')
  }
  const output = value as Record<string, unknown>
  const outputKeys = Object.keys(output).sort()
  if (
    outputKeys.length !== PROXY_OUTPUT_KEYS.length ||
    outputKeys.some((key, index) => key !== PROXY_OUTPUT_KEYS[index])
  ) {
    throw new Error('Il provider AI ha restituito un piano che non rispetta lo schema chiuso.')
  }
  const answer = valueString(output, 'answer').slice(0, MAX_TURN_CHARACTERS)
  const actionType = valueString(output, 'action_type')
  if (actionType === 'none') {
    if (!answer) throw new Error('Il provider AI non ha restituito una risposta testuale.')
    return { answer }
  }
  if (!PROXY_ACTION_KINDS.includes(actionType as WorkspaceActionKind)) {
    throw new Error('Il provider AI ha proposto un tipo di azione non supportato.')
  }

  const kind = actionType as WorkspaceActionKind
  const rawTargetId = valueString(output, 'target_id')
  const title = valueString(output, 'title').slice(0, 500)
  const notes = valueString(output, 'notes').slice(0, 8_000)
  const name = valueString(output, 'name').slice(0, 500)
  const email = valueString(output, 'email').slice(0, 320)
  const rawPriority = valueString(output, 'priority')
  const priority = ['', 'low', 'normal', 'high'].includes(rawPriority)
    ? rawPriority as WorkspaceActionProposal['priority']
    : ''
  const rawRole = valueString(output, 'role')
  const role = ['', 'admin', 'member', 'guest'].includes(rawRole)
    ? rawRole as WorkspaceActionProposal['role']
    : ''
  const rawAssigneeId = valueString(output, 'assignee_identity_id')
  const projectId = snapshot.project.wire.id
  let targetId: Uuid = projectId
  let assigneeIdentityId: Uuid | '' = ''
  let summary = ''

  if (kind === 'create_topic') {
    if (rawTargetId !== projectId && rawTargetId !== snapshot.project.wire.root_resource_id) {
      throw new Error('Il piano non è collegato al progetto aperto.')
    }
    if (!name) throw new Error('Il piano non contiene il nome della nuova categoria.')
    summary = `Crea la categoria “${name}” nel progetto.`
  } else if (kind === 'invite_member') {
    if (rawTargetId !== projectId && rawTargetId !== snapshot.project.wire.root_resource_id) {
      throw new Error('Il piano non è collegato al progetto aperto.')
    }
    if (!name || !/^\S+@\S+\.\S+$/.test(email)) {
      throw new Error('Il piano non contiene nome ed email validi per il nuovo membro.')
    }
    summary = `Invita ${name} (${email}) nel progetto con ruolo ${role || 'member'}.`
  } else if (kind === 'create_task_list') {
    const topic = findGroundedTarget(snapshot.topics, rawTargetId, question)
    if (!topic?.document || !name) throw new Error('La categoria o il nome della nuova tasklist non sono validi.')
    targetId = topic.wire.id
    summary = `Crea la tasklist “${name}” nella categoria “${topic.document.name}”.`
  } else if (kind === 'rename_topic' || kind === 'toggle_topic_favorite' || kind === 'delete_topic') {
    const topic = findGroundedTarget(snapshot.topics, rawTargetId, question)
    if (!topic?.document) throw new Error('La categoria indicata non appartiene al progetto aperto.')
    targetId = topic.wire.id
    if (kind === 'rename_topic') {
      if (!name) throw new Error('Il piano non contiene il nuovo nome della categoria.')
      summary = `Rinomina la categoria “${topic.document.name}” in “${name}”.`
    } else if (kind === 'toggle_topic_favorite') {
      summary = `${topic.document.favorite ? 'Rimuovi' : 'Aggiungi'} la categoria “${topic.document.name}” ${topic.document.favorite ? 'dai' : 'ai'} preferiti.`
    } else {
      summary = `Elimina la categoria “${topic.document.name}” e il suo contenuto.`
    }
  } else if (kind === 'rename_task_list' || kind === 'create_task') {
    const list = findGroundedTarget(snapshot.taskLists, rawTargetId, question)
    if (!list?.document) throw new Error('La tasklist indicata non appartiene al progetto aperto.')
    targetId = list.wire.id
    if (kind === 'rename_task_list') {
      if (!name) throw new Error('Il piano non contiene il nuovo nome della tasklist.')
      summary = `Rinomina la tasklist “${list.document.name}” in “${name}”.`
    } else {
      if (!title) throw new Error('Il piano non contiene il titolo del nuovo task.')
      summary = `Crea il task “${title}” nella tasklist “${list.document.name}” con priorità ${priority || 'normal'}.`
    }
  } else if (kind === 'update_member_responsibilities') {
    const mentionedMembers = snapshot.members?.filter((candidate) =>
      question.toLocaleLowerCase().includes(candidate.label.toLocaleLowerCase())) ?? []
    const member = snapshot.members?.find((candidate) => candidate.identityId === rawTargetId)
      ?? (mentionedMembers.length === 1 ? mentionedMembers[0] : undefined)
    if (!member || !notes) throw new Error('Il membro o le responsabilità indicate non sono validi.')
    targetId = member.identityId
    summary = `Aggiorna le responsabilità di ${member.label}: “${notes}”.`
  } else {
    const task = findGroundedTask(snapshot, rawTargetId, question)
    if (!task) throw new Error('Il task indicato non appartiene al progetto aperto.')
    targetId = task.wire.id
    if (kind === 'update_task') {
      if (!title && !notes && rawPriority === '') throw new Error('Il piano non contiene modifiche per il task.')
      summary = `Modifica il task “${task.document.title}”.`
    } else if (kind === 'complete_task') {
      summary = `Segna come completato il task “${task.document.title}”.`
    } else if (kind === 'copy_task') {
      summary = `Crea una copia del task “${task.document.title}”.`
    } else {
      const member = snapshot.members?.find((candidate) => candidate.identityId === rawAssigneeId)
      if (!member) throw new Error('L’assegnatario indicato non appartiene al progetto aperto.')
      assigneeIdentityId = member.identityId
      summary = `Assegna il task “${task.document.title}” a ${member.label}.`
    }
  }

  const proposal: WorkspaceActionProposal = {
    id: crypto.randomUUID(),
    requestId,
    kind,
    targetId,
    title,
    notes,
    priority,
    assigneeIdentityId,
    name,
    email,
    role,
    summary,
    status: 'pending',
  }
  return {
    answer: answer || 'Ho preparato l’azione richiesta. Controlla il riepilogo e conferma per eseguirla.',
    proposal,
  }
}

const fitHistoryToVault = (turns: WorkspaceChatTurn[]): WorkspaceChatTurn[] => {
  const bounded = turns.slice(-HISTORY_LIMIT).map((turn) => ({
    ...turn,
    content: turn.content.slice(0, MAX_TURN_CHARACTERS),
  }))
  while (
    bounded.length > 2 &&
    new TextEncoder().encode(JSON.stringify(bounded)).byteLength > HISTORY_STORAGE_BYTES
  ) {
    bounded.shift()
  }
  return bounded
}

export class WorkspaceChatService {
  readonly #profiles: LocalAiProfileStore

  constructor(
    private readonly vault: WorkspaceChatVault,
    private readonly resolveBridge: () => LocalEdgeInferenceBridge | undefined = resolveLocalEdgeInferenceBridge,
  ) {
    this.#profiles = new LocalAiProfileStore(vault)
  }

  availability(): WorkspaceChatAvailability {
    let profile: LocalAiProfile | undefined
    try {
      profile = this.#profiles.load()
    } catch {
      profile = undefined
    }
    return {
      profileConfigured: Boolean(profile),
      runtimeConnected: Boolean(
        this.resolveBridge() || (profile && browserDirectInferenceAllowed(profile)),
      ),
      model: profile?.model,
    }
  }

  history(projectId: Uuid, channel = 'workspace'): WorkspaceChatTurn[] {
    const encoded = this.vault.getLocalSetting(historyKey(projectId, channel))
      ?? (channel === 'workspace'
        ? this.vault.getLocalSetting(legacyWorkspaceHistoryKey(projectId))
        : undefined)
    if (!encoded) return []
    try {
      const parsed: unknown = JSON.parse(encoded)
      return Array.isArray(parsed)
        ? fitHistoryToVault(parsed.filter(isTurn))
        : []
    } catch {
      return []
    }
  }

  async clear(projectId: Uuid, channel = 'workspace'): Promise<void> {
    await this.vault.deleteLocalSetting(historyKey(projectId, channel))
    if (channel === 'workspace') {
      await this.vault.deleteLocalSetting(legacyWorkspaceHistoryKey(projectId))
    }
  }

  async updateProposalStatus(
    projectId: Uuid,
    turnId: Uuid,
    status: WorkspaceActionStatus,
    error?: string,
  ): Promise<WorkspaceChatTurn[]> {
    const updated = this.history(projectId).map((turn) =>
      turn.id === turnId && turn.proposal
        ? {
            ...turn,
            proposal: {
              ...turn.proposal,
              status,
              ...(error ? { error: error.slice(0, 1_000) } : { error: undefined }),
            },
          }
        : turn)
    await this.vault.putLocalSetting(historyKey(projectId), JSON.stringify(updated))
    return updated
  }

  async ask(
    snapshot: WorkspaceSnapshot,
    question: string,
    signal?: AbortSignal,
  ): Promise<WorkspaceChatTurn[]> {
    return this.#ask(snapshot, question, 'workspace', undefined, signal)
  }

  async askAboutAgent(
    snapshot: WorkspaceSnapshot,
    target: WorkspaceAgentTarget,
    question: string,
    signal?: AbortSignal,
  ): Promise<WorkspaceChatTurn[]> {
    return this.#ask(snapshot, question, `agent:${target.agentId}`, target, signal)
  }

  async #ask(
    snapshot: WorkspaceSnapshot,
    question: string,
    channel: string,
    target?: WorkspaceAgentTarget,
    signal?: AbortSignal,
  ): Promise<WorkspaceChatTurn[]> {
    const normalized = question.trim()
    if (!normalized) throw new Error('Scrivi una domanda sul progetto.')
    if (normalized.length > 4_000) throw new Error('La domanda può contenere al massimo 4.000 caratteri.')
    const profile: LocalAiProfile | undefined = this.#profiles.load()
    if (!profile) throw new Error('Configura prima un provider nelle impostazioni AI.')
    const bridge = this.resolveBridge()
    if (!bridge && !browserDirectInferenceAllowed(profile)) {
      throw new Error('Questa modalità richiede lo Sprout Local Edge Runtime.')
    }

    const projectId = snapshot.project.wire.id
    const history = this.history(projectId, channel)
    const targetTasks = target
      ? snapshot.tasks.filter(
          (task) => task.wire.active_assignee_identity_id === target.principalIdentityId,
        )
      : snapshot.tasks
    const sources = buildWorkspaceSources({ ...snapshot, tasks: targetTasks })
    if (target) {
      sources.unshift(resourceSource(snapshot.project.wire.root_resource_id, {
        type: 'observed_agent',
        agent_id: target.agentId,
        principal_identity_id: target.principalIdentityId,
        identity_handle: target.identityHandle,
        assigned_work_items: targetTasks.length,
      }))
    }
    const requestId = crypto.randomUUID()
    const request: ProviderGenerationRequest = {
      task: target ? 'answer_from_authorized_context' : 'interpret_proxy_request',
      model: profile.model,
      instructions: [
        target
          ? `Sei l’agente personale dell’utente. La UI mostra una conversazione con l’agente ${target.identityHandle}, ma sei tu a rispondere sulla base del suo lavoro osservabile.`
          : 'Sei lo UserProxy personale del workspace Sprout aperto dall’utente. La tua autorità coincide esattamente con quella dell’utente corrente; non decidere mai i permessi.',
        target
          ? 'Usa soltanto le fonti fornite relative all’agente osservato e al loro contesto di progetto.'
          : 'Interpreta la richiesta usando soltanto le fonti del progetto fornite. Puoi rispondere oppure proporre una sola azione concreta.',
        'Se il contesto non contiene la risposta, dichiaralo chiaramente.',
        target
          ? 'Questa risposta è answer-only: non comandare il target e non affermare di avere modificato task, commenti, prompt, liste o permessi.'
          : [
              `Le sole azioni candidate sono: ${PROXY_ACTION_KINDS.join(', ')}.`,
              'Per ogni azione usa esclusivamente un ID presente nelle fonti: target_id è il topic_id, task_list_id, task_id o identity_id pertinente. Non inventare ID.',
              'Per create_task usa target_id della tasklist, title per il titolo, notes per le note e priority.',
              'Per create_task_list usa target_id della categoria e name. Per create_topic e invite_member usa target_id del progetto.',
              'Per rename_topic o rename_task_list usa target_id e name. Per update_task usa target_id e soltanto i nuovi title/notes/priority richiesti.',
              'Per assign_task usa target_id del task e assignee_identity_id del membro. Per invite_member usa name, email e role.',
              'Per update_member_responsibilities usa target_id del membro e notes per il nuovo testo.',
              'Se la richiesta è informativa usa action_type "none". Non dichiarare mai che un’azione è già stata eseguita: il prodotto la mostrerà all’utente per conferma e applicherà i permission gate.',
            ].join(' '),
        target
          ? 'Restituisci esclusivamente JSON nel formato {"answer":"testo della risposta"}.'
          : 'Restituisci esclusivamente l’oggetto JSON chiuso richiesto dallo schema. Compila tutte le stringhe; usa stringa vuota per i campi non pertinenti.',
      ].join(' '),
      sources,
      input: {
        project_id: projectId,
        request_id: requestId,
        ...(target ? { observed_agent: target } : {}),
        ...(!target ? {
          allowed_action_types: PROXY_ACTION_KINDS,
          candidate_topic_ids: snapshot.topics.filter((topic) => topic.document).map((topic) => topic.wire.id),
          candidate_task_list_ids: snapshot.taskLists.filter((list) => list.document).map((list) => list.wire.id),
          candidate_task_ids: snapshot.tasks.map((task) => task.wire.id),
          candidate_principal_ids: snapshot.members?.map((member) => member.identityId) ?? [],
          candidate_resource_ids: [
            snapshot.project.wire.root_resource_id,
            ...snapshot.topics.filter((topic) => topic.document).map((topic) => topic.wire.resource_node_id),
            ...snapshot.taskLists.filter((list) => list.document).map((list) => list.wire.resource_node_id),
            ...snapshot.tasks.map((task) => task.wire.resource_node_id),
          ],
          max_plan_steps: 1,
        } : {}),
        question: normalized,
        conversation: history.slice(-HISTORY_CONTEXT_LIMIT).map(({ role, content }) => ({ role, content })),
      },
      outputSchema: target ? ANSWER_SCHEMA : PROXY_SCHEMA,
      preferences: profile.preferences,
    }
    const result = bridge
      ? await bridge.generateStructured(profile, request, signal)
      : await providerForLocalProfile(profile).generateStructured(request, signal)
    const parsed = target
      ? { answer: parseAnswer(result.value) }
      : parseProxyResult(result.value, snapshot, normalized, requestId)
    const now = new Date().toISOString()
    const newTurns: WorkspaceChatTurn[] = [
      { id: requestId, role: 'user', content: normalized, createdAt: now },
      {
        id: crypto.randomUUID(),
        role: 'assistant',
        content: parsed.answer,
        createdAt: new Date().toISOString(),
        ...(parsed.proposal ? { proposal: parsed.proposal } : {}),
      },
    ]
    const updated = fitHistoryToVault([
      ...history,
      ...newTurns,
    ])
    await this.vault.putLocalSetting(historyKey(projectId, channel), JSON.stringify(updated))
    return updated
  }
}

export const createWorkspaceChatService = (
  vault: WorkspaceChatVault,
  resolveBridge?: () => LocalEdgeInferenceBridge | undefined,
): WorkspaceChatService => new WorkspaceChatService(vault, resolveBridge)
