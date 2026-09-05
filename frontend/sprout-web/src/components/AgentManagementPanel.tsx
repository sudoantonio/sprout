import { AgentConversation } from './AgentConversation'
import { createPortal } from 'react-dom'
import { useEffect, useState, type FormEvent } from 'react'
import type {
  AgentDirectoryItemDto,
  ProvisionAgentResponse,
  Uuid,
} from '../api/contracts'
import type { DecryptedTask, TaskListColumnColor } from '../domain/models'
import type { TaskListIcon } from '../domain/task-list-icon'
import type { WorkspaceChatService, WorkspaceSnapshot } from '../ai/workspace-chat'
import {
  AGENT_ACTIONS,
  AGENT_ACTION_LABELS,
  compileAgentProvisioningPreview,
  type AgentActionClass,
  type AgentAvailability,
  type AgentProvisioningDraft,
  type AgentProvisioningPreview,
} from '../domain/agent-provisioning'
import {
  activityForAgent,
  type AgentActivity,
} from '../domain/agents'
import {
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
  onProvision(draft: AgentProvisioningDraft): Promise<ProvisionAgentResponse>
  workspaceAiService?: WorkspaceChatService
  workspaceSnapshot?: WorkspaceSnapshot
  onPostPersonalAgentComment?(
    agent: AgentDirectoryItemDto,
    task: DecryptedTask,
    markdown: string,
  ): Promise<void>
  onOpenAiSettings(): void
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
  workspaceAiService,
  workspaceSnapshot,
  onPostPersonalAgentComment,
  onOpenAiSettings,
}: AgentManagementPanelProps) => {
  const [createOpen, setCreateOpen] = useState(false)
  const [provisioningHandle, setProvisioningHandle] = useState('')
  const [provisioningPrompt, setProvisioningPrompt] = useState('')
  const [provisioningAvailability, setProvisioningAvailability] =
    useState<AgentAvailability>('controller_private')
  const [provisioningActions, setProvisioningActions] =
    useState<AgentActionClass[]>(['post_comment'])
  const [provisioningPreview, setProvisioningPreview] =
    useState<AgentProvisioningPreview>()
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string>()
  const [bootstrap, setBootstrap] = useState<ProvisionAgentResponse>()
  const [copied, setCopied] = useState(false)
  const [demoWorkspaceOpen, setDemoWorkspaceOpen] = useState(false)
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
  const selectedWorkspaceAgentId = selectedAgent?.id
  const workspaceAgentName = selectedAgent?.identity_handle ?? (demoWorkspaceOpen ? 'Atlas' : undefined)
  const workspaceAgentInitial = workspaceAgentName?.slice(0, 1).toUpperCase()
  const displayedAgentName = agentDisplayName || workspaceAgentName
  const displayedAgentAvatar = agentDisplayAvatar || (selectedAgent ? workspaceAgentInitial : '🦉')
  const agentPreferenceKey = selectedAgent?.id ?? (workspaceAgentName ? 'demo-atlas' : undefined)

  useEffect(() => {
    onWorkspaceChange?.(
      workspaceAgentName
        ? {
            ...(selectedWorkspaceAgentId ? { agentId: selectedWorkspaceAgentId } : {}),
            name: displayedAgentName ?? workspaceAgentName,
            avatar: displayedAgentAvatar ?? workspaceAgentInitial ?? '',
          }
        : undefined,
    )
  }, [
    displayedAgentAvatar,
    displayedAgentName,
    onWorkspaceChange,
    selectedWorkspaceAgentId,
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
    setProvisioningHandle('')
    setProvisioningPrompt('')
    setProvisioningAvailability('controller_private')
    setProvisioningActions(['post_comment'])
    setProvisioningPreview(undefined)
    setError(undefined)
    setBootstrap(undefined)
    setCopied(false)
  }

  const reviewProvisioning = (event: FormEvent) => {
    event.preventDefault()
    setError(undefined)
    try {
      setProvisioningPreview(compileAgentProvisioningPreview({
        identityHandle: provisioningHandle,
        systemPrompt: provisioningPrompt,
        availability: provisioningAvailability,
        actions: provisioningActions,
      }))
    } catch (validationError) {
      setError(
        validationError instanceof Error
          ? validationError.message
          : 'Provisioning non valido.',
      )
    }
  }

  const submitProvisioning = async () => {
    if (!provisioningPreview) return
    setError(undefined)
    setSubmitting(true)
    try {
      setBootstrap(await onProvision({
        identityHandle: provisioningPreview.identityHandle,
        systemPrompt: provisioningPreview.systemPrompt,
        availability: provisioningPreview.availability,
        actions: provisioningPreview.actions,
      }))
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
              {selectedAgent ? <AgentConversation
                agent={selectedAgent}
                snapshot={workspaceSnapshot}
                service={workspaceAiService}
                onPostComment={onPostPersonalAgentComment
                  ? (task, markdown) => onPostPersonalAgentComment(selectedAgent, task, markdown)
                  : undefined}
                onOpenAiSettings={onOpenAiSettings}
              /> : <p role="status">Atlas è un esempio. Crea un agente per consultare il suo lavoro tramite il tuo agente personale.</p>}
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
              {selectedAgent && <p className="agent-conversation-notice">
                Qui puoi personalizzare nome e icona. Una modifica alle istruzioni operative
                deve passare dall’agente personale e da una nuova revisione compilata e approvata;
                non dipende dal runner dell’agente osservato.
              </p>}
              {!selectedAgent && <div className="agent-editor-fields">
                <label className="agent-editor-prompt">
                  <textarea
                    value={agentSystemPrompt}
                    onChange={(event) => setAgentSystemPrompt(event.target.value)}
                    placeholder="Definisci personalità, obiettivi e modo di lavorare del tuo agente"
                    aria-label="Istruzioni agente"
                  />
                </label>
              </div>}
              <footer>
                {!selectedAgent && <div className="agent-editor-footer-tools">
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
                </div>}
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
            ) : provisioningPreview ? (
              <div className="agent-provisioning-review">
                <p className="agent-provisioning-intro">
                  Controlla la proposta compilata. L’approvazione firma il prompt
                  esatto e crea insieme identità, LocalGoal e runner.
                </p>
                <dl>
                  <div>
                    <dt>Nome tecnico</dt>
                    <dd>{provisioningPreview.identityHandle}</dd>
                  </div>
                  <div>
                    <dt>Visibilità</dt>
                    <dd>{provisioningPreview.availability === 'controller_private'
                      ? 'Privato al controller'
                      : 'Delegabile nel progetto'}</dd>
                  </div>
                </dl>
                <section className="agent-provisioning-prompt-review" aria-labelledby="agent-prompt-review-title">
                  <h3 id="agent-prompt-review-title">System prompt</h3>
                  <p>{provisioningPreview.systemPrompt}</p>
                </section>
                <section className="agent-provisioning-capabilities" aria-labelledby="agent-capabilities-title">
                  <h3 id="agent-capabilities-title">Azioni richieste dal contratto</h3>
                  <ul>
                    {provisioningPreview.actions.map((action) => (
                      <li key={action}>{AGENT_ACTION_LABELS[action]}</li>
                    ))}
                  </ul>
                  <p>Nessun tool esterno viene autorizzato durante la creazione.</p>
                </section>
                {error && <p className="agent-provisioning-error" role="alert">{error}</p>}
                <div className="agent-dialog-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    disabled={submitting}
                    onClick={() => {
                      setProvisioningPreview(undefined)
                      setError(undefined)
                    }}
                  >
                    Modifica
                  </button>
                  <button
                    type="button"
                    className="primary-button"
                    disabled={submitting}
                    onClick={() => void submitProvisioning()}
                  >
                    {submitting ? 'Firma e crea…' : 'Approva e crea agente'}
                  </button>
                </div>
              </div>
            ) : (
              <form onSubmit={reviewProvisioning}>
                <p className="agent-provisioning-intro">
                  Descrivi in linguaggio naturale identità, obiettivi e modo di
                  lavorare. Sprout compilerà una proposta di provisioning da
                  controllare prima della firma.
                </p>
                <label className="agent-provisioning-field">
                  Nome tecnico
                  <input
                    required
                    autoFocus
                    minLength={3}
                    maxLength={128}
                    value={provisioningHandle}
                    onChange={(event) => setProvisioningHandle(event.target.value)}
                    placeholder="es. assistente-progetto"
                    autoComplete="off"
                  />
                </label>
                <label className="agent-provisioning-field">
                  System prompt
                  <textarea
                    required
                    maxLength={12_000}
                    value={provisioningPrompt}
                    onChange={(event) => setProvisioningPrompt(event.target.value)}
                    placeholder="Descrivi cosa deve fare l’agente, come deve comportarsi e quali limiti deve rispettare."
                  />
                </label>
                <label className="agent-provisioning-field">
                  Visibilità
                  <select
                    value={provisioningAvailability}
                    onChange={(event) => setProvisioningAvailability(event.target.value as AgentAvailability)}
                  >
                    <option value="controller_private">Privato al controller</option>
                    <option value="project_delegable">Delegabile nel progetto</option>
                  </select>
                </label>
                <fieldset className="agent-provisioning-action-picker">
                  <legend>Capacità operative</legend>
                  <p>
                    Scegli esplicitamente cosa potrà fare l’agente. Il system prompt
                    non può concedere permessi da solo.
                  </p>
                  <div>
                    {AGENT_ACTIONS.map((action) => (
                      <label key={action}>
                        <input
                          type="checkbox"
                          checked={provisioningActions.includes(action)}
                          onChange={(event) => setProvisioningActions((current) =>
                            event.target.checked
                              ? [...current, action]
                              : current.filter((candidate) => candidate !== action))}
                        />
                        <span>{AGENT_ACTION_LABELS[action]}</span>
                      </label>
                    ))}
                  </div>
                </fieldset>
                <p className="agent-provisioning-note">
                  Il prompt viene cifrato sul dispositivo. Il backend riceve soltanto
                  ciphertext, impegni crittografici, contratto compilato e firme.
                </p>
                {error && <p className="agent-provisioning-error" role="alert">{error}</p>}
                <div className="agent-dialog-actions">
                  <button type="button" className="secondary-button" onClick={closeProvisioning}>
                    Annulla
                  </button>
                  <button
                    type="submit"
                    className="primary-button"
                    disabled={!provisioningHandle.trim() || !provisioningPrompt.trim() || provisioningActions.length === 0}
                  >
                    Struttura proposta
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
