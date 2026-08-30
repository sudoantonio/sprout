import { createPortal } from 'react-dom'
import { useEffect, useState, type FormEvent } from 'react'
import type {
  AgentDirectoryItemDto,
  ProvisionAgentResponse,
  Uuid,
} from '../api/contracts'
import type { DecryptedTask, TaskListColumnColor } from '../domain/models'
import type { TaskListIcon } from '../domain/task-list-icon'
import {
  activityForAgent,
  type AgentActivity,
} from '../domain/agents'
import {
  ChevronDownIcon,
  PlusIcon,
  XIcon,
} from './icons'
import { TaskListIconPanel } from './TaskListIconPanel'
import { TaskListAvatarContent } from './TaskListAvatarContent'

interface AgentManagementPanelProps {
  agents: AgentDirectoryItemDto[]
  searchQuery?: string
  activityFilter?: 'all' | AgentActivity
  selectedAgentId?: Uuid
  tasks?: DecryptedTask[]
  onSelectAgent(agentId: Uuid): void
  onWorkspaceChange?(
    workspace: { agentId?: Uuid; name: string; avatar: string } | undefined,
  ): void
  restoreDemoWorkspace?: boolean
  directoryResetKey?: number
  onSelectTask?(taskId: Uuid): void
  onProvision(envelope: unknown): Promise<ProvisionAgentResponse>
}

type AgentEditorPreferences = {
  name: string
  avatar: string
  avatarColor: string
  avatarColumnColor: TaskListColumnColor
  avatarIcon?: TaskListIcon
  systemPrompt: string
  capabilities: string[]
}

const agentEditorPreferencesKey = (agentKey: string) =>
  `sprout.agent-editor.${agentKey}`

const readAgentEditorPreferences = (agentKey: string) => {
  try {
    const raw = window.localStorage.getItem(agentEditorPreferencesKey(agentKey))
    if (!raw) return undefined
    return JSON.parse(raw) as AgentEditorPreferences
  } catch {
    return undefined
  }
}

const runnerLabel = (
  state: AgentDirectoryItemDto['runner_state'],
): string => {
  if (state === 'active') return 'Runner connesso'
  if (state === 'pending_key') return 'In attesa della chiave'
  return 'Runner revocato'
}

const resizeChatComposer = (textarea: HTMLTextAreaElement) => {
  textarea.style.height = 'auto'
  textarea.style.height = `${Math.min(textarea.scrollHeight, 288)}px`
}


const validateProvisioningEnvelope = (value: string): unknown => {
  let parsed: unknown
  try {
    parsed = JSON.parse(value)
  } catch {
    throw new Error('Il provisioning non contiene JSON valido.')
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('L’envelope di provisioning deve essere un oggetto JSON.')
  }
  const record = parsed as Record<string, unknown>
  const required = [
    'id',
    'principal_identity_id',
    'controller_identity_id',
    'identity_handle',
    'encrypted_profile',
    'profile_resource_node_id',
    'key_epoch',
    'availability',
    'runner_id',
    'runner_device_id',
    'encrypted_runner_label',
    'initial_local_goal',
    'final_prompt_approval',
  ]
  const missing = required.filter((key) => record[key] == null)
  if (missing.length > 0) {
    throw new Error(`Campi obbligatori mancanti: ${missing.join(', ')}.`)
  }
  return parsed
}

const activityMeta: Record<
  AgentActivity,
  { label: string; description: string }
> = {
  working: { label: 'Working', description: 'Agenti al lavoro' },
  done: { label: 'Done', description: 'Obiettivi conclusi' },
  rest: { label: 'Rest', description: 'In attesa o non connessi' },
}

const AGENT_ICON_COLOR_VALUES: Record<TaskListColumnColor, string> = {
  'column-white': '#f5f5f5',
  'column-slate': '#64748b',
  'column-blue': '#0b8dca',
  'column-sand': '#d6a54c',
  'column-emerald': '#22a06b',
  'column-violet': '#7c5ce5',
  'column-peach': '#e78a62',
  'column-mauve': '#b461a7',
  'column-rose': '#df5e7a',
}

const AgentStageTile = ({
  agent,
  activity,
  onSelect,
}: {
  agent: AgentDirectoryItemDto
  activity: AgentActivity
  onSelect(agentId: Uuid): void
}) => (
  <button
    type="button"
    className="agent-stage-tile"
    onClick={() => onSelect(agent.id)}
    aria-label={`${agent.identity_handle}, ${activityMeta[activity].label}`}
  >
    <span className={`agent-stage-avatar agent-stage-avatar--${activity}`}>
      {agent.identity_handle.slice(0, 1).toUpperCase()}
    </span>
    <strong>{agent.identity_handle}</strong>
    <span>{runnerLabel(agent.runner_state)}</span>
  </button>
)

const DemoAgentTile = ({
  activity,
  onSelect,
}: {
  activity: AgentActivity
  onSelect(): void
}) => (
  <button
    type="button"
    className="agent-stage-tile agent-stage-tile--demo"
    onClick={onSelect}
    aria-label={`Atlas, agente di esempio, ${activityMeta[activity].label}`}
  >
    <span className={`agent-stage-avatar agent-stage-avatar--${activity}`}>
      🦉
    </span>
    <strong>Atlas</strong>
  </button>
)

export const AgentManagementPanel = ({
  agents,
  searchQuery = '',
  activityFilter = 'all',
  selectedAgentId,
  tasks = [],
  onSelectAgent,
  onWorkspaceChange,
  restoreDemoWorkspace = false,
  directoryResetKey,
  onSelectTask,
  onProvision,
}: AgentManagementPanelProps) => {
  const [createOpen, setCreateOpen] = useState(false)
  const [envelope, setEnvelope] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string>()
  const [bootstrap, setBootstrap] = useState<ProvisionAgentResponse>()
  const [copied, setCopied] = useState(false)
  const [demoWorkspaceOpen, setDemoWorkspaceOpen] = useState(false)
  const [chatDraft, setChatDraft] = useState('')
  const [tasksCollapsed, setTasksCollapsed] = useState(false)
  const [tasksOpening, setTasksOpening] = useState(false)
  const [agentEditorOpen, setAgentEditorOpen] = useState(false)
  const [agentDisplayName, setAgentDisplayName] = useState('')
  const [agentDisplayAvatar, setAgentDisplayAvatar] = useState('')
  const [agentSystemPrompt, setAgentSystemPrompt] = useState('')
  const [agentCapabilities, setAgentCapabilities] = useState<string[]>([])
  const [agentCapabilityMenuOpen, setAgentCapabilityMenuOpen] = useState(false)
  const [agentIconPickerOpen, setAgentIconPickerOpen] = useState(false)
  const [agentAvatarColor, setAgentAvatarColor] = useState('#0b8dca')
  const [agentAvatarColumnColor, setAgentAvatarColumnColor] =
    useState<TaskListColumnColor>('column-blue')
  const [agentAvatarIcon, setAgentAvatarIcon] = useState<TaskListIcon>()
  const [agentIconPickerAnchorRect, setAgentIconPickerAnchorRect] =
    useState<DOMRect | null>(null)

  const selectedAgent = agents.find((agent) => agent.id === selectedAgentId)
  const workspaceAgentName = selectedAgent?.identity_handle ?? (demoWorkspaceOpen ? 'Atlas' : undefined)
  const workspaceAgentInitial = workspaceAgentName?.slice(0, 1).toUpperCase()
  const displayedAgentName = agentDisplayName || workspaceAgentName
  const displayedAgentAvatar = agentDisplayAvatar || (selectedAgent ? workspaceAgentInitial : '🦉')
  const agentPreferenceKey = selectedAgent?.id ?? (workspaceAgentName ? 'demo-atlas' : undefined)

  useEffect(() => {
    onWorkspaceChange?.(
      workspaceAgentName
        ? {
            ...(selectedAgent ? { agentId: selectedAgent.id } : {}),
            name: displayedAgentName ?? workspaceAgentName,
            avatar: displayedAgentAvatar ?? workspaceAgentInitial ?? '',
          }
        : undefined,
    )
  }, [
    displayedAgentAvatar,
    displayedAgentName,
    onWorkspaceChange,
    workspaceAgentInitial,
    workspaceAgentName,
  ])

  useEffect(() => {
    if (directoryResetKey === undefined) return
    setDemoWorkspaceOpen(false)
  }, [directoryResetKey])

  useEffect(() => {
    if (!restoreDemoWorkspace || selectedAgentId) return
    setDemoWorkspaceOpen(true)
  }, [restoreDemoWorkspace, selectedAgentId])

  useEffect(() => {
    const saved = agentPreferenceKey
      ? readAgentEditorPreferences(agentPreferenceKey)
      : undefined
    setAgentEditorOpen(false)
    setAgentDisplayName(saved?.name ?? workspaceAgentName ?? '')
    setAgentDisplayAvatar(
      saved?.avatar ??
        (selectedAgent ? workspaceAgentInitial ?? '' : workspaceAgentName ? '🦉' : ''),
    )
    setAgentSystemPrompt(saved?.systemPrompt ?? '')
    setAgentCapabilities(saved?.capabilities ?? [])
    setAgentCapabilityMenuOpen(false)
    setAgentIconPickerOpen(false)
    setAgentAvatarColor(saved?.avatarColor ?? '#0b8dca')
    setAgentAvatarColumnColor(saved?.avatarColumnColor ?? 'column-blue')
    setAgentAvatarIcon(saved?.avatarIcon)
    setAgentIconPickerAnchorRect(null)
  }, [agentPreferenceKey, selectedAgent, workspaceAgentInitial, workspaceAgentName])

  useEffect(() => {
    setTasksCollapsed(false)
    setTasksOpening(false)
  }, [workspaceAgentName])

  useEffect(() => {
    if (!tasksOpening) return
    const timeoutId = window.setTimeout(() => setTasksOpening(false), 280)
    return () => window.clearTimeout(timeoutId)
  }, [tasksOpening])

  const toggleTasksPanel = () => {
    if (tasksCollapsed) {
      setTasksOpening(true)
      setTasksCollapsed(false)
      return
    }
    setTasksCollapsed(true)
  }

  const saveAgentEditor = () => {
    if (agentPreferenceKey) {
      const preferences: AgentEditorPreferences = {
        name: agentDisplayName.trim() || workspaceAgentName || 'Agente',
        avatar: agentDisplayAvatar,
        avatarColor: agentAvatarColor,
        avatarColumnColor: agentAvatarColumnColor,
        ...(agentAvatarIcon ? { avatarIcon: agentAvatarIcon } : {}),
        systemPrompt: agentSystemPrompt,
        capabilities: agentCapabilities,
      }
      try {
        window.localStorage.setItem(
          agentEditorPreferencesKey(agentPreferenceKey),
          JSON.stringify(preferences),
        )
      } catch {
        // The visual editor remains usable even if browser persistence is disabled.
      }
    }
    setAgentEditorOpen(false)
  }
  const agentTasks = selectedAgent
    ? tasks.filter(
        (task) =>
          task.wire.active_assignee_identity_id ===
          selectedAgent.principal_identity_id,
      )
    : []
  const openAgentTasks = agentTasks.filter((task) => task.wire.state.state === 'open')
  const completedAgentTasks = agentTasks.filter(
    (task) => task.wire.state.state === 'completed',
  )
  const normalizedQuery = searchQuery.trim().toLocaleLowerCase()
  const demoAgentActivity: AgentActivity = 'rest'
  const showDemoAgent =
    (activityFilter === 'all' || activityFilter === demoAgentActivity) &&
    (!normalizedQuery ||
      `atlas ${activityMeta[demoAgentActivity].label}`
        .toLocaleLowerCase()
        .includes(normalizedQuery))
  const activityFilteredAgents =
    activityFilter === 'all'
      ? agents
      : agents.filter((agent) => activityForAgent(agent) === activityFilter)
  const filteredAgents = normalizedQuery
    ? activityFilteredAgents.filter((agent) => {
        const activity = activityMeta[activityForAgent(agent)].label
        return `${agent.identity_handle} ${agent.state} ${agent.runner_state} ${activity}`
          .toLocaleLowerCase()
          .includes(normalizedQuery)
      })
    : activityFilteredAgents
  const groupedAgents: Record<AgentActivity, AgentDirectoryItemDto[]> = {
    working: filteredAgents.filter(
      (agent) => activityForAgent(agent) === 'working',
    ),
    done: filteredAgents.filter((agent) => activityForAgent(agent) === 'done'),
    rest: filteredAgents.filter((agent) => activityForAgent(agent) === 'rest'),
  }
  const visibleActivities = (['working', 'done', 'rest'] as const).filter(
    (activity) =>
      groupedAgents[activity].length > 0 ||
      (activity === demoAgentActivity && showDemoAgent),
  )

  useEffect(() => {
    if (!createOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !submitting && !bootstrap) {
        setCreateOpen(false)
        setError(undefined)
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [bootstrap, createOpen, submitting])

  const closeProvisioning = () => {
    if (submitting) return
    setCreateOpen(false)
    setEnvelope('')
    setError(undefined)
    setBootstrap(undefined)
    setCopied(false)
  }

  const submitProvisioning = async (event: FormEvent) => {
    event.preventDefault()
    setError(undefined)
    let parsed: unknown
    try {
      parsed = validateProvisioningEnvelope(envelope)
    } catch (validationError) {
      setError(
        validationError instanceof Error
          ? validationError.message
          : 'Provisioning non valido.',
      )
      return
    }
    setSubmitting(true)
    try {
      setBootstrap(await onProvision(parsed))
      setEnvelope('')
    } catch (requestError) {
      setError(
        requestError instanceof Error
          ? requestError.message
          : 'Il backend ha rifiutato il provisioning.',
      )
    } finally {
      setSubmitting(false)
    }
  }

  const copyBootstrapToken = async () => {
    if (!bootstrap) return
    try {
      await navigator.clipboard.writeText(bootstrap.bootstrap_token)
      setCopied(true)
    } catch {
      setError('Copia automatica non disponibile: seleziona il token manualmente.')
    }
  }

  return (
    <section className="agent-management" aria-labelledby="agent-management-title">
      <article className="agent-stage-panel">
        <h1 id="agent-management-title" className="visually-hidden">
          {selectedAgent ? selectedAgent.identity_handle : 'Agenti'}
        </h1>
        {workspaceAgentName ? (
          <div className={`agent-workspace${tasksCollapsed ? ' agent-workspace--tasks-collapsed' : ''}${tasksOpening ? ' agent-workspace--tasks-opening' : ''}`}>
            <section className="agent-workspace-conversation" aria-label={`Chat con ${workspaceAgentName}`}>
              <div className="agent-workspace-agent-identity">
                <span className="agent-stage-avatar agent-stage-avatar--working">
                  {displayedAgentAvatar}
                </span>
                <h2>{displayedAgentName}</h2>
                <button
                  type="button"
                  className="agent-workspace-agent-options"
                  aria-label="Modifica nome e icona agente"
                  aria-expanded={agentEditorOpen}
                  onClick={() => {
                    setAgentIconPickerOpen(false)
                    setAgentIconPickerAnchorRect(null)
                    setAgentEditorOpen(true)
                  }}
                >
                  <span aria-hidden>•••</span>
                </button>
              </div>
            </section>

            <aside className="agent-workspace-tasks" aria-label={`Task di ${workspaceAgentName}`}>
              <div className="agent-workspace-tasks-heading">
                <button
                  type="button"
                  className={`agent-workspace-tasks-toggle${tasksCollapsed ? ' is-collapsed' : ''}`}
                  aria-label={tasksCollapsed ? 'Mostra task agente' : 'Nascondi task agente'}
                  aria-expanded={!tasksCollapsed}
                  onClick={toggleTasksPanel}
                >
                  <svg viewBox="0 0 24 24" fill="none" aria-hidden>
                    <circle cx="5" cy="7" r="1.5" />
                    <path d="M10 7h9M10 17h9" />
                    <circle cx="5" cy="17" r="1.5" />
                  </svg>
                </button>
              </div>
              {!tasksCollapsed && (
                <>
                <label
                  className="agent-workspace-add-task"
                >
                  <span aria-hidden />
                  <input type="text" placeholder="Aggiungi task" aria-label="Aggiungi task" />
                </label>
                  <div className="agent-workspace-task-list">
                    {openAgentTasks.map((task) => (
                      <button
                        type="button"
                        className="agent-workspace-task"
                        key={task.wire.id}
                        onClick={() => onSelectTask?.(task.wire.id)}
                      >
                        <span aria-hidden />
                        {task.document.title}
                      </button>
                    ))}
                    {completedAgentTasks.map((task) => (
                      <button
                        type="button"
                        className="agent-workspace-task agent-workspace-task--complete"
                        key={task.wire.id}
                        onClick={() => onSelectTask?.(task.wire.id)}
                      >
                        <span aria-hidden />
                        {task.document.title}
                      </button>
                    ))}
                  </div>
                </>
              )}
            </aside>
            <label className="agent-chat-composer">
              <textarea
                value={chatDraft}
                onChange={(event) => {
                  setChatDraft(event.target.value)
                  resizeChatComposer(event.currentTarget)
                }}
                placeholder="Ask everything"
                aria-label={`Messaggio per ${workspaceAgentName}`}
                rows={1}
              />
              <button
                type="button"
                className="agent-chat-attach"
                aria-label="Aggiungi contesto"
                title="Aggiungi contesto"
              >
                <PlusIcon aria-hidden />
              </button>
              <button
                type="button"
                className="agent-chat-model"
                aria-label="Seleziona modello: Sprout 1"
                aria-haspopup="listbox"
                title="Seleziona modello"
              >
                Sprout 1
                <ChevronDownIcon aria-hidden />
              </button>
              <button
                type="button"
                className="agent-chat-send"
                disabled={!chatDraft.trim()}
                aria-label="Invia messaggio"
                title="La consegna E2EE sarà disponibile con l'attivazione della chat sicura"
              >
                <svg viewBox="0 0 24 24" fill="none" aria-hidden>
                  <path d="M12 19V5m0 0-6 6m6-6 6 6" />
                </svg>
              </button>
            </label>
          </div>
        ) : (
          <div className="agent-stage" aria-label="Agenti per stato operativo">
            {visibleActivities.map((activity) => (
              <section className="agent-stage-group" key={activity} aria-labelledby={`agent-group-${activity}`}>
                <header>
                  <span className={`agent-stage-dot agent-stage-dot--${activity}`} aria-hidden />
                  <div>
                    <h2 id={`agent-group-${activity}`}>{activityMeta[activity].label}</h2>
                    <p>{activityMeta[activity].description}</p>
                  </div>
                </header>
                <div className="agent-stage-row">
                  {activity === demoAgentActivity && (
                    <button
                      type="button"
                      className="agent-stage-create"
                      onClick={() => setCreateOpen(true)}
                      aria-label="Crea nuovo agente"
                    >
                      <span aria-hidden>
                        <PlusIcon />
                      </span>
                      <strong>New</strong>
                    </button>
                  )}
                  {activity === demoAgentActivity && showDemoAgent && (
                    <DemoAgentTile
                      activity={demoAgentActivity}
                      onSelect={() => setDemoWorkspaceOpen(true)}
                    />
                  )}
                  {groupedAgents[activity].map((agent) => (
                    <AgentStageTile
                      key={agent.id}
                      agent={agent}
                      activity={activity}
                      onSelect={(agentId) => {
                        setDemoWorkspaceOpen(false)
                        onSelectAgent(agentId)
                      }}
                    />
                  ))}
                </div>
              </section>
            ))}
          </div>
        )}
      </article>

      {agentEditorOpen &&
        createPortal(
          <div
            className="agent-editor-overlay"
            role="presentation"
            onMouseDown={() => {
              setAgentIconPickerOpen(false)
              setAgentIconPickerAnchorRect(null)
              setAgentEditorOpen(false)
            }}
          >
            <section
              className="agent-editor-dialog"
              role="dialog"
              aria-modal="true"
              aria-label="Modifica agente"
              onMouseDown={(event) => event.stopPropagation()}
            >
              <header>
                <div>
                  <button
                    type="button"
                    className="agent-editor-avatar-trigger"
                    style={{ backgroundColor: agentAvatarColor }}
                    onClick={(event) => {
                      setAgentIconPickerAnchorRect(
                        event.currentTarget.getBoundingClientRect(),
                      )
                      setAgentIconPickerOpen(true)
                    }}
                    aria-label="Cambia icona e colore agente"
                  >
                    <TaskListAvatarContent
                      icon={agentAvatarIcon}
                      fallbackInitial={agentDisplayAvatar || workspaceAgentInitial || null}
                    />
                  </button>
                  <input
                    className="agent-editor-name"
                    value={agentDisplayName}
                    onChange={(event) => setAgentDisplayName(event.target.value)}
                    aria-label="Nome agente"
                  />
                </div>
                <button
                  type="button"
                  className="agent-dialog-close"
                  onClick={() => {
                    setAgentIconPickerOpen(false)
                    setAgentIconPickerAnchorRect(null)
                    setAgentEditorOpen(false)
                  }}
                  aria-label="Chiudi"
                >
                  <XIcon aria-hidden />
                </button>
              </header>
              <div className="agent-editor-fields">
                <label className="agent-editor-prompt">
                  <textarea
                    value={agentSystemPrompt}
                    onChange={(event) => setAgentSystemPrompt(event.target.value)}
                    placeholder="Definisci personalità, obiettivi e modo di lavorare del tuo agente"
                    aria-label="Istruzioni agente"
                  />
                </label>
              </div>
              <footer>
                <div className="agent-editor-footer-tools">
                  <button
                    type="button"
                    className="agent-editor-add-capability"
                    aria-label="Aggiungi tool o skill"
                    aria-expanded={agentCapabilityMenuOpen}
                    onClick={() => setAgentCapabilityMenuOpen((open) => !open)}
                  >
                    <PlusIcon aria-hidden />
                    <span>Aggiungi</span>
                  </button>
                  {agentCapabilityMenuOpen && (
                    <div className="agent-editor-capability-menu">
                      <button
                        type="button"
                        onClick={() => {
                          setAgentCapabilities((items) => [...items, 'Nuovo tool'])
                          setAgentCapabilityMenuOpen(false)
                        }}
                      >
                        Aggiungi tool
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setAgentCapabilities((items) => [...items, 'Nuova skill'])
                          setAgentCapabilityMenuOpen(false)
                        }}
                      >
                        Aggiungi skill
                      </button>
                    </div>
                  )}
                  {agentCapabilities.length > 0 && (
                    <div className="agent-editor-capability-list" aria-label="Tool e skill">
                      {agentCapabilities.map((capability, index) => (
                        <span key={`${capability}-${index}`}>{capability}</span>
                      ))}
                    </div>
                  )}
                </div>
                <button
                  type="button"
                  className="primary-button"
                  onClick={saveAgentEditor}
                >
                  Salva
                </button>
              </footer>
              {agentIconPickerOpen && agentIconPickerAnchorRect && (
                <TaskListIconPanel
                  anchorRect={agentIconPickerAnchorRect}
                  listName={agentDisplayName || workspaceAgentName || 'Agente'}
                  value={agentAvatarIcon}
                  color={agentAvatarColumnColor}
                  onChange={setAgentAvatarIcon}
                  onColorChange={(color) => {
                    setAgentAvatarColumnColor(color)
                    setAgentAvatarColor(AGENT_ICON_COLOR_VALUES[color])
                  }}
                  onClose={() => {
                    setAgentIconPickerOpen(false)
                    setAgentIconPickerAnchorRect(null)
                  }}
                />
              )}
            </section>
          </div>,
          document.body,
        )}

      {createOpen && (
        <div className="agent-provisioning-backdrop" role="presentation">
          <section
            className="agent-provisioning-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="agent-provisioning-title"
          >
            <header>
              <div>
                <p className="eyebrow">Provisioning governato</p>
                <h2 id="agent-provisioning-title">
                  {bootstrap ? 'Salva il bootstrap token' : 'Nuovo agente AI'}
                </h2>
              </div>
              <button
                type="button"
                className="agent-dialog-close"
                onClick={closeProvisioning}
                disabled={submitting}
                aria-label="Chiudi"
              >
                <XIcon aria-hidden />
              </button>
            </header>

            {bootstrap ? (
              <div className="agent-bootstrap-result" role="status">
                <div className="agent-bootstrap-warning">
                  <strong>Visibile una sola volta</strong>
                  <p>
                    Il token non viene salvato nel browser né nei log. Copialo ora
                    nel runner dell’agente.
                  </p>
                </div>
                <label>
                  Bootstrap token
                  <input readOnly value={bootstrap.bootstrap_token} onFocus={(event) => event.currentTarget.select()} />
                </label>
                <p>Scade il {new Intl.DateTimeFormat('it-IT', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(bootstrap.bootstrap_expires_at))}.</p>
                <div className="agent-dialog-actions">
                  <button type="button" className="secondary-button" onClick={() => void copyBootstrapToken()}>
                    {copied ? 'Copiato' : 'Copia token'}
                  </button>
                  <button type="button" className="primary-button" onClick={closeProvisioning}>
                    Ho salvato il token
                  </button>
                </div>
              </div>
            ) : (
              <form onSubmit={(event) => void submitProvisioning(event)}>
                <p className="agent-provisioning-intro">
                  Sprout crea un agente soltanto da un envelope completo: identità,
                  profilo cifrato, LocalGoal compilato, firme e final approval.
                </p>
                <label className="agent-envelope-field">
                  Envelope di provisioning firmato
                  <textarea
                    required
                    autoFocus
                    spellCheck={false}
                    value={envelope}
                    onChange={(event) => setEnvelope(event.target.value)}
                    placeholder={'{\n  "id": "…",\n  "principal_identity_id": "…",\n  "initial_local_goal": { … }\n}'}
                  />
                </label>
                <p className="agent-provisioning-note">
                  Le chiavi private e il testo in chiaro non devono essere inclusi.
                  Certificati e firme vengono verificati dal backend.
                </p>
                {error && <p className="agent-provisioning-error" role="alert">{error}</p>}
                <div className="agent-dialog-actions">
                  <button type="button" className="secondary-button" onClick={closeProvisioning} disabled={submitting}>
                    Annulla
                  </button>
                  <button type="submit" className="primary-button" disabled={submitting || !envelope.trim()}>
                    {submitting ? 'Verifica e crea…' : 'Verifica e crea agente'}
                  </button>
                </div>
              </form>
            )}
            {bootstrap && error && <p className="agent-provisioning-error" role="alert">{error}</p>}
          </section>
        </div>
      )}
    </section>
  )
}
