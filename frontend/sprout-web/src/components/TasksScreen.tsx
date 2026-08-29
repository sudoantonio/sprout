import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactElement,
} from 'react'
import { createPortal } from 'react-dom'
import type {
  AgentDirectoryItemDto,
  AttachmentCollectionItemDto,
  ProvisionAgentResponse,
  TaskDto,
  Uuid,
} from '../api/contracts'
import {
  filterBoardSearch,
  formatDueDate,
  formatTaskCardDueDate,
  getTaskStatusIndicator,
  isTaskOverdue,
  sortItemsByTaskUrgency,
  sortTaskListsByUrgency,
  sortTopicsByUrgency,
  taskListsForTopic,
  topicUrgencyBadges,
  type TopicUrgencyBadge,
} from '../domain/tasks'
import {
  TIMELINE_SCALE_DEFAULT,
  defaultTimelineDueDatetimeLocal,
  filterTimelineTasks,
  startOfWeek,
} from '../domain/timeline'
import {
  memberAvatarColor,
  normalizeTaskListColumnColor,
  resolveTaskListColumnTint,
  resolveTaskListIconColorFromStored,
  TASK_LIST_ICON_COLORS,
  type DecryptedTask,
  type DecryptedInfoDocument,
  type InfoDocumentContent,
  type InfoFileBlock,
  type TaskCreationInput,
  type TaskDocument,
  type TaskFilter,
  type TaskListColumnColor,
  type TaskListIcon,
  isSameTaskListIcon,
} from '../domain/models'
import { TaskListIconPanel } from './TaskListIconPanel'
import { TaskListAvatarContent } from './TaskListAvatarContent'
import { BoardTimelineView } from './BoardTimelineView'
import { BoardProjectSwitcher } from './BoardProjectSwitcher'
import {
  TaskHistoryRows,
  TaskListHistoryPanel,
} from './TaskListHistoryPanel'
import { InfoDocumentPanel } from './TaskListInfoPanel'
import type {
  BoardFocus,
  BoardMember,
  BoardViewMode,
  ProjectItem,
  TaskListItem,
  TopicItem,
} from '../store/app-store'
import {
  AgentIcon,
  CalendarIcon,
  CheckIcon,
  ChevronIcon,
  ChevronDownIcon,
  CircleIcon,
  ClockIcon,
  ExpandDetailIcon,
  FilterIcon,
  FlagIcon,
  LayoutGridIcon,
  LockIcon,
  SidebarHomeIcon,
  SidebarUserIcon,
  FolderIcon,
  PaperclipIcon,
  PlusIcon,
  RepeatIcon,
  UsersIcon,
  SearchIcon,
  SidebarAgentIcon,
  StarIcon,
  TimeHistoryIcon,
  XIcon,
} from './icons'
import {
  WorkspaceUserMenu,
  type WorkspaceUserMenuProps,
} from './WorkspaceUserMenu'
import { NaturalLanguageDateField } from './NaturalLanguageDateField'
import { AgentManagementPanel } from './AgentManagementPanel'

type AgentActivityFilter = 'all' | 'working' | 'done' | 'rest'

const AGENT_FILTER_OPTIONS: Array<[AgentActivityFilter, string]> = [
  ['all', 'Tutti gli agenti'],
  ['working', 'Working'],
  ['done', 'Done'],
  ['rest', 'Rest'],
]

export interface TasksScreenProps {
  project?: ProjectItem
  topics: TopicItem[]
  taskLists: TaskListItem[]
  tasks: DecryptedTask[]
  lockedTasks: TaskDto[]
  boardMembers: BoardMember[]
  agents: AgentDirectoryItemDto[]
  boardFocus: BoardFocus
  boardViewMode: BoardViewMode
  selectedTopicId?: Uuid
  selectedListId?: Uuid
  selectedTaskId?: Uuid
  currentUserLabel: string
  publishedQuestionnaireVersions: Array<{
    id: Uuid
    label: string
  }>
  filter: TaskFilter
  loading: boolean
  onSelectFocus(focus: BoardFocus): void
  onBoardViewModeChange(mode: BoardViewMode): void
  onSelectList(id: Uuid): void
  onSelectTask(id: Uuid | undefined): void
  onFilter(filter: TaskFilter): void
  onCreateTopic(name: string): Promise<void>
  onRenameTopic(topic: TopicItem, name: string): Promise<void>
  onToggleTopicFavorite(topic: TopicItem): Promise<void>
  onDeleteTopic(topic: TopicItem): Promise<void>
  onCreateList(name: string, topicId: Uuid): Promise<void>
  onUpdateTaskList(
    list: TaskListItem,
    input: {
      name: string
      color?: TaskListColumnColor
      icon?: TaskListIcon
    },
  ): Promise<void>
  onLoadProjectInfo(project: ProjectItem): Promise<DecryptedInfoDocument[]>
  onCreateProjectInfoDocument(
    project: ProjectItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onLoadTopicInfo(topic: TopicItem): Promise<DecryptedInfoDocument[]>
  onCreateTopicInfoDocument(
    topic: TopicItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onLoadTaskListInfo(list: TaskListItem): Promise<DecryptedInfoDocument[]>
  onCreateTaskListInfoDocument(
    list: TaskListItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUpdateInfoDocument(
    document: DecryptedInfoDocument,
    content: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUploadInfoDocumentFile(
    document: DecryptedInfoDocument,
    file: File,
  ): Promise<InfoFileBlock>
  onReadInfoDocumentFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<Blob>
  onDownloadInfoDocumentFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<void>
  onCreateTask(input: TaskCreationInput, listId: Uuid): Promise<void>
  onUpdateTask(
    task: DecryptedTask,
    input: TaskUpdateInput,
  ): Promise<void>
  onAssignTask(task: DecryptedTask, assigneeIdentityId: Uuid): Promise<void>
  onCompleteTask(task: DecryptedTask): Promise<void>
  onCopyTask(task: DecryptedTask): Promise<void>
  onInviteMember(input: {
    email: string
    name: string
    role?: 'admin' | 'member' | 'guest'
  }): Promise<void>
  onProvisionAgent(envelope: unknown): Promise<ProvisionAgentResponse>
  taskAttachments: AttachmentCollectionItemDto[]
  taskAttachmentLabels: Record<string, string>
  onRefreshTaskAttachments(taskId: Uuid): Promise<void>
  onDownloadTaskAttachment(
    attachment: AttachmentCollectionItemDto,
  ): Promise<void>
  userMenu: Omit<WorkspaceUserMenuProps, 'variant'>
}

const SIDEBAR_MEMBER_VISIBLE_MAX = 3

const initialFor = (label: string): string => {
  const trimmed = label.trim()
  if (!trimmed) return '?'
  return trimmed[0].toUpperCase()
}

const initialsFor = (label: string): string => {
  const parts = label.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) {
    return parts[0].slice(0, 2).toUpperCase()
  }
  return `${parts[0][0] ?? ''}${parts[parts.length - 1]?.[0] ?? ''}`.toUpperCase()
}

const isMemberBoardFocus = (
  focus: BoardFocus,
): focus is { type: 'members' } | { type: 'member'; identityId: Uuid } =>
  focus.type === 'members' || focus.type === 'member'

const isAgentBoardFocus = (
  focus: BoardFocus,
): focus is { type: 'agents' } | { type: 'agent'; agentId: Uuid } =>
  focus.type === 'agents' || focus.type === 'agent'

const BoardTaskCard = ({
  task,
  selected,
  boardMemberById,
  hideAssignee,
  onSelect,
  onComplete,
}: {
  task: DecryptedTask
  selected: boolean
  boardMemberById: Map<Uuid, BoardMember>
  hideAssignee?: boolean
  onSelect(): void
  onComplete(): void
}) => {
  const open = task.wire.state.state === 'open'
  const status = getTaskStatusIndicator(task)
  const overdue = open && isTaskOverdue(task)
  const associatedUsers = hideAssignee
    ? []
    : taskAssociatedUsers(task, boardMemberById)
  const cardClass = [
    'board-card',
    !open ? 'is-completed' : '',
    selected ? 'selected' : '',
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <article className={cardClass}>
      <div className="board-card-top">
        <label
          className={`board-task-check board-task-check--${status.variant}`}
          title={status.label}
        >
          <input
            type="checkbox"
            checked={!open}
            disabled={!open || !task.wire.active_assignment_id}
            aria-label={`${status.label}: ${task.document.title}`}
            onChange={() => {
              if (open) onComplete()
            }}
          />
          <span
            className="board-task-check-dot"
            style={
              status.dueProgress !== undefined
                ? ({
                    '--task-due-progress': status.dueProgress,
                  } as CSSProperties)
                : undefined
            }
            aria-hidden
          />
        </label>
        <div className="board-card-content">
          <button
            type="button"
            className="board-card-body"
            onClick={onSelect}
          >
            <div className="board-card-header">
              <strong>{task.document.title}</strong>
            </div>
            {task.document.notes && (
              <span className="board-card-notes">{task.document.notes}</span>
            )}
          </button>
          {(task.document.due_at || associatedUsers.length > 0) && (
            <div
              className="board-card-footer"
              onClick={onSelect}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault()
                  onSelect()
                }
              }}
              role="presentation"
            >
              <div className="board-card-footer-start">
                {task.document.due_at && (
                  <span
                    className={
                      overdue ? 'board-card-due is-overdue' : 'board-card-due'
                    }
                  >
                    <ClockIcon className="board-card-due-icon" aria-hidden />
                    {formatTaskCardDueDate(task.document.due_at)}
                  </span>
                )}
              </div>
              {associatedUsers.length > 0 && (
                <BoardCardAssignee users={associatedUsers} />
              )}
            </div>
          )}
        </div>
      </div>
    </article>
  )
}

const TOPIC_ICON_COLORS = TASK_LIST_ICON_COLORS

const memberAvatarColorClass = (identityId: Uuid): string =>
  `board-avatar--${memberAvatarColor(identityId)}`

const memberAvatarClassName = (identityId: Uuid, extra = ''): string =>
  ['board-avatar', 'member', memberAvatarColorClass(identityId), extra]
    .filter(Boolean)
    .join(' ')

const MembersOverviewPanel = ({
  members,
  onInviteMember,
}: {
  members: BoardMember[]
  onInviteMember(input: {
    email: string
    name: string
    role?: 'admin' | 'member' | 'guest'
  }): Promise<void>
}) => {
  const [createOpen, setCreateOpen] = useState(false)
  const [email, setEmail] = useState('')
  const [name, setName] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setSubmitting(true)
    try {
      await onInviteMember({ email: email.trim(), name: name.trim(), role: 'member' })
      setEmail('')
      setName('')
      setCreateOpen(false)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <section className="agent-management member-management" aria-label="Panoramica membri">
      <article className="agent-stage-panel">
        <div className="agent-stage member-stage">
          <section className="agent-stage-group" aria-labelledby="members-overview-title">
            <header>
              <div>
                <h2 id="members-overview-title">Membri</h2>
              </div>
            </header>
            <div className="agent-stage-row">
              <button
                type="button"
                className="agent-stage-create"
                onClick={() => setCreateOpen(true)}
                aria-label="Invita nuovo membro"
              >
                <span aria-hidden>
                  <PlusIcon />
                </span>
                <strong>New</strong>
              </button>
              {members.map((member) => (
                <div
                  className="agent-stage-tile agent-stage-tile--demo member-stage-tile"
                  key={member.identityId}
                >
                  <span className={`agent-stage-avatar member-stage-avatar ${memberAvatarClassName(member.identityId)}`} aria-hidden>
                    {initialsFor(member.label)}
                  </span>
                  <strong>{member.label}</strong>
                </div>
              ))}
            </div>
          </section>
        </div>
        {createOpen && (
          <form className="member-overview-invite" onSubmit={(event) => void submit(event)}>
            <input
              required
              autoFocus
              type="email"
              placeholder="Email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              aria-label="Email membro"
            />
            <input
              required
              placeholder="Nome"
              value={name}
              onChange={(event) => setName(event.target.value)}
              aria-label="Nome membro"
            />
            <button type="button" onClick={() => setCreateOpen(false)}>Annulla</button>
            <button type="submit" disabled={submitting}>Invita</button>
          </form>
        )}
      </article>
    </section>
  )
}

const topicAvatarClass = (index: number): string => {
  const color = TOPIC_ICON_COLORS[index % TOPIC_ICON_COLORS.length]
  return `column board-avatar--${color}`
}

const COLUMN_AVATAR_COLORS = TASK_LIST_ICON_COLORS

const columnAvatarClass = (
  index: number,
): (typeof COLUMN_AVATAR_COLORS)[number] =>
  COLUMN_AVATAR_COLORS[index % COLUMN_AVATAR_COLORS.length]

const resolveListColumnColor = (
  list: TaskListItem,
  index: number,
): TaskListColumnColor => {
  if (!list.document?.color) return 'column-white'
  return normalizeTaskListColumnColor(list.document.color, columnAvatarClass(index))
}

const columnAvatarColorClass = (color: TaskListColumnColor): string =>
  `board-avatar column board-avatar--${color}`

const columnTintClass = (color: TaskListColumnColor): string =>
  `board-column-tint-${color}`

const BoardColumnHeader = ({
  list,
  isEditing,
  editName,
  editColor,
  editIcon,
  iconPickerOpen,
  onEditNameChange,
  onCancelEdit,
  onCommitEdit,
  onToggleIconPicker,
  onOpenHistory,
  onAddTask,
}: {
  list: TaskListItem
  isEditing: boolean
  editName: string
  editColor: TaskListColumnColor
  editIcon: TaskListIcon | undefined
  iconPickerOpen: boolean
  onEditNameChange(name: string): void
  onCancelEdit(): void
  onCommitEdit(): void
  onToggleIconPicker(): void
  onOpenHistory(): void
  onAddTask(): void
}) => {
  const renameInputRef = useRef<HTMLInputElement>(null)
  const listNameLabel = list.document?.name ?? 'Locked list'
  const displayName = isEditing ? editName || list.document?.name : list.document?.name
  const avatarInitial = list.document && displayName
    ? initialFor(displayName)
    : null
  const avatarColor = resolveTaskListIconColorFromStored(
    isEditing ? editColor : list.document?.color,
    list.wire.id,
  )
  const displayIcon = isEditing ? editIcon : list.document?.icon

  useEffect(() => {
    if (!isEditing) return
    renameInputRef.current?.focus()
    renameInputRef.current?.select()
  }, [isEditing])

  return (
    <header
      className={
        isEditing
          ? 'board-column-header board-column-header--editing'
          : 'board-column-header'
      }
    >
      <div className="board-column-identity">
        {isEditing && list.document ? (
          <div className="board-column-icon-trigger-wrap">
            <button
              type="button"
              className={`${columnAvatarColorClass(avatarColor)} board-column-icon-trigger`}
              aria-label="Scegli icona task list"
              aria-expanded={iconPickerOpen}
              onMouseDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.stopPropagation()
                onToggleIconPicker()
              }}
            >
              <TaskListAvatarContent
                icon={displayIcon}
                fallbackInitial={avatarInitial}
              />
            </button>
          </div>
        ) : (
          <span className={columnAvatarColorClass(avatarColor)} aria-hidden>
            {list.document ? (
              <TaskListAvatarContent
                icon={displayIcon}
                fallbackInitial={avatarInitial}
              />
            ) : (
              <LockIcon />
            )}
          </span>
        )}
        <div className="board-column-title-row">
          {isEditing && list.document ? (
            <input
              ref={renameInputRef}
              className="board-column-rename-input"
              value={editName}
              aria-label="Modifica nome task list"
              onChange={(event) => onEditNameChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  onCommitEdit()
                }
                if (event.key === 'Escape') {
                  event.preventDefault()
                  onCancelEdit()
                }
              }}
            />
          ) : (
            <h3>{listNameLabel}</h3>
          )}
          {list.document && !isEditing && (
            <button
              type="button"
              className="board-column-detail-trigger"
              aria-label={`Apri dettaglio di ${listNameLabel}`}
              title="Apri dettaglio"
              onClick={(event) => {
                event.stopPropagation()
                onOpenHistory()
              }}
            >
              <ExpandDetailIcon className="board-column-action-icon" />
            </button>
          )}
          {isEditing && list.document && (
            <div className="board-column-edit-actions">
              <button
                type="button"
                className="board-column-edit-confirm"
                aria-label="Conferma modifiche task list"
                onClick={(event) => {
                  event.stopPropagation()
                  onCommitEdit()
                }}
              >
                <CheckIcon className="board-column-action-icon" />
              </button>
            </div>
          )}
        </div>
      </div>
      <div className="board-column-actions">
        <button
          type="button"
          className="board-add-task"
          onClick={onAddTask}
        >
          <PlusIcon />
          Aggiungi
        </button>
      </div>
    </header>
  )
}

const MemberColumnHeader = ({
  member,
}: {
  member: BoardMember
}) => {
  return (
    <header className="board-column-header board-column-header--member">
      <div className="board-column-identity">
        <span
          className={`board-avatar column member ${memberAvatarColorClass(member.identityId)}`}
          aria-hidden
        >
          {initialsFor(member.label)}
        </span>
        <div className="board-column-title-row">
          <h3>{member.label}</h3>
        </div>
      </div>
    </header>
  )
}

const AgentColumnHeader = ({
  agent,
  onAddTask,
}: {
  agent: AgentDirectoryItemDto
  onAddTask(): void
}) => (
  <header className="board-column-header board-column-header--member">
    <div className="board-column-identity">
      <span
        className={`board-avatar column member ${memberAvatarColorClass(agent.principal_identity_id)}`}
        aria-hidden
      >
        {initialsFor(agent.identity_handle)}
      </span>
      <div className="board-column-title-row">
        <h3>{agent.identity_handle}</h3>
      </div>
    </div>
    <div className="board-column-actions">
      <button type="button" className="board-add-task" onClick={onAddTask}>
        <PlusIcon />
        Aggiungi
      </button>
    </div>
  </header>
)

const AgentTaskListHeader = ({ onAddTask }: { onAddTask(): void }) => (
  <header className="board-column-header board-column-header--member">
    <div className="board-column-identity">
      <span className="board-avatar column member column-sky" aria-hidden>
        A
      </span>
      <div className="board-column-title-row">
        <h3>Tasklist agenti</h3>
      </div>
    </div>
    <div className="board-column-actions">
      <button type="button" className="board-add-task" onClick={onAddTask}>
        <PlusIcon />
        Aggiungi
      </button>
    </div>
  </header>
)

type TaskTypeFilter = 'priority' | 'deadline' | 'recurring'
type TaskStateFilter = 'open' | 'completed'
type TaskDateFilter = 'overdue' | 'today' | 'upcoming' | 'none'

interface AdvancedTaskFilters {
  listIds: Uuid[]
  types: TaskTypeFilter[]
  memberIds: Uuid[]
  states: TaskStateFilter[]
  dates: TaskDateFilter[]
}

type TaskFilterGroup = keyof AdvancedTaskFilters

const taskTypeFor = (task: DecryptedTask): TaskTypeFilter =>
  task.document.recurrence
    ? 'recurring'
    : task.document.due_at
      ? 'deadline'
      : 'priority'

const boardTaskGroupMeta = (
  task: DecryptedTask,
  group: TaskFilterGroup,
  members: BoardMember[],
  now = new Date(),
): { key: string; label: string; tone: string; rank: number; member?: BoardMember } => {
  if (group === 'types') {
    const type = taskTypeFor(task)
    if (type === 'priority') {
      const priority = task.document.priority ?? 'normal'
      return priority === 'high'
        ? { key: 'priority-high', label: 'Priorità alta', tone: 'danger', rank: 0 }
        : priority === 'low'
          ? { key: 'priority-low', label: 'Priorità bassa', tone: 'info', rank: 2 }
          : { key: 'priority-normal', label: 'Priorità media', tone: 'warning', rank: 1 }
    }
    if (type === 'deadline') {
      const due = new Date(task.document.due_at as string)
      const todayStart = new Date(now)
      todayStart.setHours(0, 0, 0, 0)
      const todayEnd = new Date(now)
      todayEnd.setHours(23, 59, 59, 999)
      return due.getTime() < todayStart.getTime()
        ? { key: 'deadline-overdue', label: 'Scadenza · Scadute', tone: 'danger', rank: 3 }
        : due.getTime() <= todayEnd.getTime()
          ? { key: 'deadline-today', label: 'Scadenza · Oggi', tone: 'orange', rank: 4 }
          : { key: 'deadline-upcoming', label: 'Scadenza · Prossime', tone: 'orange', rank: 5 }
    }

    const recurrence = task.document.recurrence
    const frequency = recurrence?.frequency ?? 'daily'
    const interval = recurrence?.interval ?? 1
    const singularLabels: Record<typeof frequency, string> = {
      minutes: 'Minuti',
      daily: 'Giornaliera',
      weekly: 'Settimanale',
      monthly: 'Mensile',
    }
    const pluralLabels: Record<typeof frequency, string> = {
      minutes: 'minuti',
      daily: 'giorni',
      weekly: 'settimane',
      monthly: 'mesi',
    }
    return {
      key: `recurring-${frequency}-${interval}`,
      label:
        interval === 1
          ? `Ricorsività · ${singularLabels[frequency]}`
          : `Ricorsività · Ogni ${interval} ${pluralLabels[frequency]}`,
      tone: 'violet',
      rank: 10 + ['minutes', 'daily', 'weekly', 'monthly'].indexOf(frequency),
    }
  }

  if (group === 'memberIds') {
    const identityId = task.wire.active_assignee_identity_id
    const member = members.find((item) => item.identityId === identityId)
    return member
      ? { key: member.identityId, label: member.label, tone: 'member', rank: 0, member }
      : { key: 'unassigned', label: 'Non assegnato', tone: 'neutral', rank: 1 }
  }

  if (group === 'states') {
    const completed = task.wire.state.state === 'completed'
    return completed
      ? { key: 'completed', label: 'Completati', tone: 'success', rank: 1 }
      : { key: 'open', label: 'Da completare', tone: 'info', rank: 0 }
  }

  const dueAt = task.document.due_at
  if (!dueAt) return { key: 'none', label: 'Senza data', tone: 'neutral', rank: 3 }
  const due = new Date(dueAt)
  const todayStart = new Date(now)
  todayStart.setHours(0, 0, 0, 0)
  const todayEnd = new Date(now)
  todayEnd.setHours(23, 59, 59, 999)
  if (due.getTime() < todayStart.getTime()) {
    return { key: 'overdue', label: 'Scaduti', tone: 'danger', rank: 0 }
  }
  if (due.getTime() <= todayEnd.getTime()) {
    return { key: 'today', label: 'Oggi', tone: 'mauve', rank: 1 }
  }
  return { key: 'upcoming', label: 'Prossimi', tone: 'orange', rank: 2 }
}

const BoardGroupedTaskCards = ({
  tasks,
  groups,
  members,
  selectedTaskId,
  boardMemberById,
  onSelectTask,
  onCompleteTask,
  level = 0,
}: {
  tasks: DecryptedTask[]
  groups: TaskFilterGroup[]
  members: BoardMember[]
  selectedTaskId: Uuid | null | undefined
  boardMemberById: Map<Uuid, BoardMember>
  onSelectTask(taskId: Uuid): void
  onCompleteTask(task: DecryptedTask): void
  level?: number
}) => {
  if (groups.length === 0) {
    return (
      <ul className={`board-cards${level > 0 ? ' board-cards--grouped' : ''}`}>
        {tasks.map((task) => (
          <li key={task.wire.id}>
            <BoardTaskCard
              task={task}
              selected={selectedTaskId === task.wire.id}
              boardMemberById={boardMemberById}
              onSelect={() => onSelectTask(task.wire.id)}
              onComplete={() => onCompleteTask(task)}
            />
          </li>
        ))}
      </ul>
    )
  }

  const [group, ...remainingGroups] = groups
  const buckets = new Map<
    string,
    { meta: ReturnType<typeof boardTaskGroupMeta>; tasks: DecryptedTask[] }
  >()
  tasks.forEach((task) => {
    const meta = boardTaskGroupMeta(task, group, members)
    const bucket = buckets.get(meta.key)
    if (bucket) bucket.tasks.push(task)
    else buckets.set(meta.key, { meta, tasks: [task] })
  })

  return (
    <div className={`board-card-groups board-card-groups--level-${level}`}>
      {[...buckets.values()]
        .sort((left, right) => left.meta.rank - right.meta.rank)
        .map(({ meta, tasks: groupedTasks }) => (
        <section className="board-card-group" key={`${group}-${meta.key}`}>
          <div className="board-card-group-heading">
            {meta.member ? (
              <>
                <span
                  className={`board-avatar member ${memberAvatarColorClass(meta.member.identityId)}`}
                  aria-hidden
                >
                  {initialFor(meta.member.label)}
                </span>
                <span className="board-card-group-member-name">{meta.member.label}</span>
              </>
            ) : (
              <span className={`tasklist-history-day-label tasklist-history-day-label--${meta.tone}`}>
                {meta.label}
              </span>
            )}
          </div>
          <BoardGroupedTaskCards
            tasks={groupedTasks}
            groups={remainingGroups}
            members={members}
            selectedTaskId={selectedTaskId}
            boardMemberById={boardMemberById}
            onSelectTask={onSelectTask}
            onCompleteTask={onCompleteTask}
            level={level + 1}
          />
        </section>
      ))}
    </div>
  )
}

const BoardColumnFilterBadges = ({
  filters,
  members,
  onRemove,
}: {
  filters: AdvancedTaskFilters
  members: BoardMember[]
  onRemove(key: keyof AdvancedTaskFilters, value: string): void
}) => {
  const hasBadges =
    filters.types.length > 0 ||
    filters.memberIds.length > 0 ||
    filters.states.length > 0 ||
    filters.dates.length > 0

  if (!hasBadges) return null

  const typeMeta: Record<TaskTypeFilter, { label: string; tone: string }> = {
    priority: { label: 'Priorità', tone: 'warning' },
    deadline: { label: 'Scadenza', tone: 'orange' },
    recurring: { label: 'Ricorsività', tone: 'violet' },
  }
  const stateMeta: Record<TaskStateFilter, { label: string; tone: string }> = {
    open: { label: 'Da completare', tone: 'info' },
    completed: { label: 'Completati', tone: 'success' },
  }
  const dateMeta: Record<TaskDateFilter, { label: string; tone: string }> = {
    overdue: { label: 'Scaduti', tone: 'danger' },
    today: { label: 'Oggi', tone: 'mauve' },
    upcoming: { label: 'Prossimi', tone: 'orange' },
    none: { label: 'Senza data', tone: 'neutral' },
  }

  return (
    <div className="board-column-filter-badges" aria-label="Filtri attivi nella tasklist">
      {filters.types.map((value) => {
        const meta = typeMeta[value]
        return (
          <button
            type="button"
            key={`type-${value}`}
            className={`tasklist-history-day-label tasklist-history-day-label--${meta.tone} board-column-filter-badge`}
            title={`Rimuovi filtro ${meta.label}`}
            onClick={() => onRemove('types', value)}
          >
            {meta.label}
          </button>
        )
      })}
      {filters.memberIds.map((identityId) => {
        const member = members.find((item) => item.identityId === identityId)
        if (!member) return null
        return (
          <button
            type="button"
            key={`member-${identityId}`}
            className={`board-avatar member ${memberAvatarColorClass(identityId)} board-column-filter-member`}
            title={`Rimuovi filtro ${member.label}`}
            aria-label={`Rimuovi filtro ${member.label}`}
            onClick={() => onRemove('memberIds', identityId)}
          >
            {initialFor(member.label)}
          </button>
        )
      })}
      {filters.states.map((value) => {
        const meta = stateMeta[value]
        return (
          <button
            type="button"
            key={`state-${value}`}
            className={`tasklist-history-day-label tasklist-history-day-label--${meta.tone} board-column-filter-badge`}
            title={`Rimuovi filtro ${meta.label}`}
            onClick={() => onRemove('states', value)}
          >
            {meta.label}
          </button>
        )
      })}
      {filters.dates.map((value) => {
        const meta = dateMeta[value]
        return (
          <button
            type="button"
            key={`date-${value}`}
            className={`tasklist-history-day-label tasklist-history-day-label--${meta.tone} board-column-filter-badge`}
            title={`Rimuovi filtro ${meta.label}`}
            onClick={() => onRemove('dates', value)}
          >
            {meta.label}
          </button>
        )
      })}
    </div>
  )
}

const initialAdvancedTaskFilters = (filter: TaskFilter): AdvancedTaskFilters => ({
  listIds: [],
  types: [],
  memberIds: [],
  states: filter === 'completed' ? ['completed'] : ['open'],
  dates:
    filter === 'today'
      ? ['today']
      : filter === 'upcoming'
        ? ['upcoming']
        : [],
})

const applyAdvancedTaskFilters = (
  tasks: DecryptedTask[],
  filters: AdvancedTaskFilters,
  now = new Date(),
): DecryptedTask[] => {
  const todayStart = new Date(now)
  todayStart.setHours(0, 0, 0, 0)
  const todayEnd = new Date(now)
  todayEnd.setHours(23, 59, 59, 999)

  return tasks.filter((task) => {
    if (filters.listIds.length > 0 && !filters.listIds.includes(task.wire.list_id)) return false

    const taskType = taskTypeFor(task)
    if (filters.types.length > 0 && !filters.types.includes(taskType)) return false

    const assigneeId = task.wire.active_assignee_identity_id
    if (filters.memberIds.length > 0 && (!assigneeId || !filters.memberIds.includes(assigneeId))) {
      return false
    }

    const state: TaskStateFilter = task.wire.state.state === 'completed' ? 'completed' : 'open'
    if (filters.states.length > 0 && !filters.states.includes(state)) return false

    if (filters.dates.length > 0) {
      const dueAt = task.document.due_at
      const matchesDate = filters.dates.some((dateFilter) => {
        if (dateFilter === 'none') return !dueAt
        if (!dueAt) return false
        const due = new Date(dueAt).getTime()
        if (!Number.isFinite(due)) return false
        if (dateFilter === 'overdue') return due < todayStart.getTime()
        if (dateFilter === 'today') return due >= todayStart.getTime() && due <= todayEnd.getTime()
        return due > todayEnd.getTime()
      })
      if (!matchesDate) return false
    }

    return true
  })
}

const sortTopicsForSidebar = (topics: TopicItem[]): TopicItem[] =>
  [...topics].sort((left, right) => {
    const leftFavorite = left.document?.favorite ? 1 : 0
    const rightFavorite = right.document?.favorite ? 1 : 0
    if (leftFavorite !== rightFavorite) return rightFavorite - leftFavorite
    const leftName = left.document?.name ?? left.wire.id
    const rightName = right.document?.name ?? right.wire.id
    if (leftName !== rightName) {
      return leftName.localeCompare(rightName, 'it', { sensitivity: 'base' })
    }
    return left.wire.created_at.localeCompare(right.wire.created_at)
  })

type TopicOverviewAnchor = {
  topic: TopicItem
  x: number
  y: number
}

const clampMenuPosition = (
  x: number,
  y: number,
  width: number,
  height: number,
): { left: number; top: number } => {
  const margin = 8
  const maxLeft = Math.max(margin, window.innerWidth - width - margin)
  const maxTop = Math.max(margin, window.innerHeight - height - margin)
  return {
    left: Math.min(Math.max(x, margin), maxLeft),
    top: Math.min(Math.max(y, margin), maxTop),
  }
}

const HIDE_OVERVIEW_DELAY_MS = 120
const OVERLAY_FILE_DIALOG_GUARD_MS = 750

const openLocalAttachmentPreview = (file: File) => {
  const url = URL.createObjectURL(file)
  const opened = window.open(url, '_blank', 'noopener,noreferrer')
  if (!opened) {
    const link = document.createElement('a')
    link.href = url
    link.download = file.name
    link.rel = 'noopener'
    link.click()
  }
  window.setTimeout(() => URL.revokeObjectURL(url), 60_000)
}

const taskAssociatedUsers = (
  task: DecryptedTask,
  boardMemberById: Map<Uuid, BoardMember>,
): BoardMember[] => {
  const assigneeId = task.wire.active_assignee_identity_id
  if (!assigneeId) return []
  const member = boardMemberById.get(assigneeId)
  return member ? [member] : []
}

const clampPopoverToViewport = (
  anchorRect: DOMRect,
  width: number,
  height: number,
  gap = 6,
): { left: number; top: number } => {
  const margin = 8
  let top = anchorRect.top - height - gap
  if (top < margin) {
    top = anchorRect.bottom + gap
  }
  let left = anchorRect.right - width
  const clamped = clampMenuPosition(left, top, width, height)
  return clamped
}

const TaskAssigneeOverviewPopover = ({
  anchorEl,
  users,
  overviewId,
  onPointerEnter,
  onPointerLeave,
}: {
  anchorEl: HTMLElement
  users: BoardMember[]
  overviewId: string
  onPointerEnter(): void
  onPointerLeave(): void
}) => {
  const popoverRef = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState<CSSProperties>({
    left: anchorEl.getBoundingClientRect().left,
    top: anchorEl.getBoundingClientRect().top,
  })

  useLayoutEffect(() => {
    const node = popoverRef.current
    if (!node) return
    const anchorRect = anchorEl.getBoundingClientRect()
    const rect = node.getBoundingClientRect()
    const next = clampPopoverToViewport(anchorRect, rect.width, rect.height)
    setPosition({ left: next.left, top: next.top })
  }, [anchorEl, users])

  const heading =
    users.length === 0
      ? 'Assegnatario'
      : users.length === 1
        ? 'Assegnato a'
        : 'Utenti associati'

  return createPortal(
    <div
      ref={popoverRef}
      id={overviewId}
      className="board-task-user-overview"
      role="tooltip"
      style={{ ...position, position: 'fixed' }}
      onMouseEnter={onPointerEnter}
      onMouseLeave={onPointerLeave}
    >
      <p className="board-task-user-overview-heading">{heading}</p>
      {users.length === 0 ? (
        <p className="board-task-user-overview-empty">Nessun assegnatario</p>
      ) : (
        <ul className="board-task-user-overview-list">
          {users.map((user) => (
            <li key={user.identityId} className="board-task-user-overview-item">
              <span
                className={`board-task-user-overview-avatar ${memberAvatarClassName(user.identityId)}`}
                aria-hidden
              >
                {initialFor(user.label)}
              </span>
              <span className="board-task-user-overview-name">{user.label}</span>
            </li>
          ))}
        </ul>
      )}
    </div>,
    document.body,
  )
}

const BoardCardAssignee = ({ users }: { users: BoardMember[] }) => {
  const triggerRef = useRef<HTMLSpanElement>(null)
  const overviewId = useId()
  const [open, setOpen] = useState(false)
  const hideTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  )

  const clearHideTimeout = () => {
    if (hideTimeoutRef.current !== undefined) {
      clearTimeout(hideTimeoutRef.current)
      hideTimeoutRef.current = undefined
    }
  }

  const show = () => {
    clearHideTimeout()
    setOpen(true)
  }

  const scheduleHide = () => {
    clearHideTimeout()
    hideTimeoutRef.current = setTimeout(
      () => setOpen(false),
      HIDE_OVERVIEW_DELAY_MS,
    )
  }

  useEffect(() => () => clearHideTimeout(), [])

  const primary = users[0]
  const ariaLabel =
    users.length === 1
      ? `Assegnato a ${primary.label}`
      : `${users.length} utenti associati`

  return (
    <>
      <span
        ref={triggerRef}
        className={`board-card-assignee board-card-assignee-trigger ${memberAvatarClassName(primary.identityId)}`}
        tabIndex={0}
        aria-label={ariaLabel}
        aria-describedby={open ? overviewId : undefined}
        onMouseEnter={show}
        onMouseLeave={scheduleHide}
        onFocus={show}
        onBlur={scheduleHide}
        onClick={(event) => event.stopPropagation()}
      >
        {initialFor(primary.label)}
      </span>
      {open && triggerRef.current && (
        <TaskAssigneeOverviewPopover
          anchorEl={triggerRef.current}
          users={users}
          overviewId={overviewId}
          onPointerEnter={show}
          onPointerLeave={scheduleHide}
        />
      )}
    </>
  )
}

const TopicOverviewMenu = ({
  anchor,
  onClose,
  onStartRename,
  onToggleFavorite,
  onDelete,
}: {
  anchor: TopicOverviewAnchor
  onClose(): void
  onStartRename(): void
  onToggleFavorite(): Promise<void>
  onDelete(): Promise<void>
}) => {
  const menuId = useId()
  const menuRef = useRef<HTMLDivElement>(null)
  const [mode, setMode] = useState<'actions' | 'confirm-delete'>('actions')
  const [busy, setBusy] = useState(false)
  const [position, setPosition] = useState<CSSProperties>({
    left: anchor.x,
    top: anchor.y,
  })

  const topicName = anchor.topic.document?.name ?? 'Categoria'
  const isFavorite = Boolean(anchor.topic.document?.favorite)
  const locked = !anchor.topic.document

  useLayoutEffect(() => {
    const node = menuRef.current
    if (!node) return
    const rect = node.getBoundingClientRect()
    const next = clampMenuPosition(anchor.x, anchor.y, rect.width, rect.height)
    setPosition({ left: next.left, top: next.top })
  }, [anchor.x, anchor.y, mode])

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) onClose()
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [onClose])

  const run = async (action: () => Promise<void>) => {
    setBusy(true)
    try {
      await action()
      onClose()
    } finally {
      setBusy(false)
    }
  }

  return createPortal(
    <div
      ref={menuRef}
      id={menuId}
      className="board-topic-overview"
      role="menu"
      aria-label={`Azioni per ${topicName}`}
      style={{ ...position, position: 'fixed' }}
      onContextMenu={(event) => event.preventDefault()}
    >
      <p className="board-topic-overview-title">{topicName}</p>
      {mode === 'actions' && (
        <>
          <button
            type="button"
            role="menuitem"
            className="board-topic-overview-item"
            disabled={locked || busy}
            onClick={() => {
              onStartRename()
              onClose()
            }}
          >
            Rinomina
          </button>
          <button
            type="button"
            role="menuitem"
            className="board-topic-overview-item"
            disabled={locked || busy}
            onClick={() => void run(onToggleFavorite)}
          >
            {isFavorite ? 'Rimuovi dai preferiti' : 'Aggiungi ai preferiti'}
          </button>
          <button
            type="button"
            role="menuitem"
            className="board-topic-overview-item board-topic-overview-item--danger"
            disabled={busy}
            onClick={() => setMode('confirm-delete')}
          >
            Elimina
          </button>
        </>
      )}
      {mode === 'confirm-delete' && (
        <div className="board-topic-overview-confirm">
          <p>Eliminare «{topicName}» e le sue task list?</p>
          <div className="board-topic-overview-actions">
            <button
              type="button"
              className="text-button"
              disabled={busy}
              onClick={() => setMode('actions')}
            >
              Annulla
            </button>
            <button
              type="button"
              className="primary-button board-topic-overview-delete"
              disabled={busy}
              onClick={() => void run(onDelete)}
            >
              Elimina
            </button>
          </div>
        </div>
      )}
    </div>,
    document.body,
  )
}

type TaskKind = 'priority' | 'deadline' | 'recurring'

type TaskKindOption = readonly [
  TaskKind,
  string,
  (props: React.SVGProps<SVGSVGElement>) => ReactElement,
]

const TASK_KIND_OPTIONS = [
  ['priority', 'Priorità', FlagIcon],
  ['deadline', 'Scadenza', CalendarIcon],
  ['recurring', 'Ricorrente', RepeatIcon],
] as const satisfies ReadonlyArray<TaskKindOption>

const taskKindIcon = (kind: TaskKind) =>
  TASK_KIND_OPTIONS.find(([value]) => value === kind)?.[2] ?? FlagIcon

const TASK_PRIORITY_OPTIONS = [
  ['low', 'Bassa'],
  ['normal', 'Normale'],
  ['high', 'Alta'],
] as const satisfies ReadonlyArray<['low' | 'normal' | 'high', string]>

type TaskPriorityLevel = (typeof TASK_PRIORITY_OPTIONS)[number][0]

const taskPriorityPillClass = (priority: TaskPriorityLevel): string =>
  `task-create-pill-btn--priority-${priority}`

const taskPrioritySegmentClass = (priority: TaskPriorityLevel): string =>
  `task-create-segment-btn--priority-${priority}`

const taskPriorityPillLabel = (priority: TaskPriorityLevel): string =>
  priority === 'normal'
    ? 'Priorità'
    : (TASK_PRIORITY_OPTIONS.find(([value]) => value === priority)?.[1] ??
      'Priorità')

type RecurrenceFrequency = NonNullable<
  TaskDocument['recurrence']
>['frequency']

const RECURRENCE_FREQUENCY_OPTIONS = [
  ['minutes', 'Minuti'],
  ['daily', 'Giorno'],
  ['monthly', 'Mese'],
] as const satisfies ReadonlyArray<[RecurrenceFrequency, string]>

const urgencyBadgeLabel = (badge: TopicUrgencyBadge): string => {
  if (badge.count === 1) {
    return badge.hasOverdue
      ? '1 task scaduto o in scadenza'
      : '1 task in scadenza'
  }
  return badge.hasOverdue
    ? `${badge.count} task scaduti o in scadenza`
    : `${badge.count} task in scadenza`
}

const BoardNavUrgencyBadge = ({
  badge,
}: {
  badge: TopicUrgencyBadge | undefined
}) => {
  if (!badge || badge.count <= 0) return null
  const label = urgencyBadgeLabel(badge)
  return (
    <span
      className={
        badge.hasOverdue
          ? 'board-nav-urgency board-nav-urgency--overdue'
          : 'board-nav-urgency board-nav-urgency--due-soon'
      }
      aria-label={label}
      title={label}
    >
      {badge.count > 99 ? '99+' : badge.count}
    </span>
  )
}

const BoardMobileStoryRing = ({
  badge,
  className,
  children,
}: {
  badge: TopicUrgencyBadge | undefined
  className: string
  children: React.ReactNode
}) => {
  const urgent = badge != null && badge.count > 0
  const urgencyClass = urgent
    ? badge.hasOverdue
      ? 'board-mobile-story-ring--overdue'
      : 'board-mobile-story-ring--due-soon'
    : ''
  const label = urgent ? urgencyBadgeLabel(badge) : undefined

  return (
    <span
      className={[className, 'board-mobile-story-ring', urgencyClass]
        .filter(Boolean)
        .join(' ')}
      aria-hidden={urgent ? undefined : true}
      aria-label={label}
      title={label}
    >
      {children}
    </span>
  )
}

type EditableBoardViewMode = 'board' | 'timeline' | 'history'

const BOARD_HIDDEN_VIEW_MODES_KEY = 'sprout.board.hidden-view-modes'
const EDITABLE_BOARD_VIEWS: ReadonlyArray<
  readonly [EditableBoardViewMode, string]
> = [
  ['board', 'Board'],
  ['timeline', 'Timeline'],
  ['history', 'History'],
]

const BoardViewModeSwitch = ({
  mode,
  onChange,
  scopeKey,
  compact = false,
}: {
  mode: BoardViewMode
  onChange(mode: BoardViewMode): void
  scopeKey: string
  compact?: boolean
}) => {
  const [editing, setEditing] = useState(false)
  const [addMenuOpen, setAddMenuOpen] = useState(false)
  const [hiddenModes, setHiddenModes] = useState<EditableBoardViewMode[]>(() => {
    try {
      const stored = JSON.parse(
        localStorage.getItem(`${BOARD_HIDDEN_VIEW_MODES_KEY}:${scopeKey}`) ?? '[]',
      ) as unknown
      if (!Array.isArray(stored)) return []
      return stored.filter(
        (value): value is EditableBoardViewMode =>
          value === 'board' || value === 'timeline' || value === 'history',
      )
    } catch {
      return []
    }
  })

  useEffect(() => {
    try {
      localStorage.setItem(
        `${BOARD_HIDDEN_VIEW_MODES_KEY}:${scopeKey}`,
        JSON.stringify(hiddenModes),
      )
    } catch {
      // Ignore unavailable storage.
    }
  }, [hiddenModes, scopeKey])

  const availableViews = EDITABLE_BOARD_VIEWS.filter(
    ([viewMode]) => !compact || viewMode === 'board',
  )
  const visibleViews = availableViews.filter(
    ([viewMode]) => !hiddenModes.includes(viewMode),
  )
  const hiddenViews = availableViews.filter(([viewMode]) =>
    hiddenModes.includes(viewMode),
  )

  const hideView = (viewMode: EditableBoardViewMode) => {
    const nextHidden = [...hiddenModes, viewMode]
    setHiddenModes(nextHidden)
    setAddMenuOpen(false)
    if (mode !== viewMode) return
    const fallback = availableViews.find(
      ([candidate]) => candidate !== viewMode && !nextHidden.includes(candidate),
    )?.[0]
    if (fallback) onChange(fallback)
    else onChange('overview')
  }

  const restoreView = (viewMode: EditableBoardViewMode) => {
    setHiddenModes((current) => current.filter((item) => item !== viewMode))
    setAddMenuOpen(false)
  }

  return (
    <div
      className={`board-view-mode-switch${editing ? ' is-editing' : ''}`}
      role="group"
      aria-label="Vista board"
    >
      <button
        type="button"
        className="board-view-mode-option active board-view-more-option"
        aria-label={editing ? 'Termina modifica tab' : 'Modifica tab'}
        title={editing ? 'Termina modifica' : 'Modifica tab'}
        onClick={() => {
          setEditing((value) => !value)
          setAddMenuOpen(false)
        }}
      >
        <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden>
          <circle cx="3" cy="8" r="1.25" />
          <circle cx="8" cy="8" r="1.25" />
          <circle cx="13" cy="8" r="1.25" />
        </svg>
      </button>

      <button
        type="button"
        className={
          mode === 'overview'
            ? 'board-view-mode-option active'
            : 'board-view-mode-option'
        }
        aria-pressed={mode === 'overview'}
        onClick={() => onChange('overview')}
      >
        Overview
      </button>

      {visibleViews.map(([viewMode, label]) => (
        <button
          type="button"
          key={viewMode}
          className={
            mode === viewMode
              ? 'board-view-mode-option board-view-editable-option active'
              : 'board-view-mode-option board-view-editable-option'
          }
          aria-pressed={mode === viewMode}
          onClick={() => onChange(viewMode)}
        >
          <span>{label}</span>
          {editing && (
            <span
              className="board-view-mode-remove"
              role="button"
              tabIndex={0}
              aria-label={`Nascondi ${label}`}
              onClick={(event) => {
                event.stopPropagation()
                hideView(viewMode)
              }}
              onKeyDown={(event) => {
                if (event.key !== 'Enter' && event.key !== ' ') return
                event.preventDefault()
                event.stopPropagation()
                hideView(viewMode)
              }}
            >
              <XIcon aria-hidden />
            </span>
          )}
        </button>
      ))}

      {editing && hiddenViews.length > 0 && (
        <div className="board-view-add-wrap">
          <button
            type="button"
            className="board-view-mode-option board-view-add-option"
            aria-label="Aggiungi tab"
            aria-expanded={addMenuOpen}
            onClick={() => setAddMenuOpen((value) => !value)}
          >
            <PlusIcon aria-hidden />
          </button>
          {addMenuOpen && (
            <div className="board-view-add-menu" role="menu">
              {hiddenViews.map(([viewMode, label]) => (
                <button
                  type="button"
                  key={viewMode}
                  role="menuitem"
                  onClick={() => restoreView(viewMode)}
                >
                  <PlusIcon aria-hidden />
                  {label}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

const BoardViewNavigation = ({
  mode,
  onChange,
  scopeKey,
  compact = false,
}: {
  mode: BoardViewMode
  onChange(mode: BoardViewMode): void
  scopeKey: string
  compact?: boolean
}) => (
  <div className="board-view-navigation">
    <BoardViewModeSwitch
      key={scopeKey}
      mode={mode}
      onChange={onChange}
      scopeKey={scopeKey}
      compact={compact}
    />
  </div>
)

const BoardAiBadge = ({ onClose }: { onClose(): void }) => {
  const [draft, setDraft] = useState('')
  const [historyOpen, setHistoryOpen] = useState(false)
  const inputRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  return createPortal(
    <section className="board-ai-badge" role="dialog" aria-label="New chat">
      <header className="board-ai-badge-header">
        <div className="board-ai-badge-title">
          <button
            type="button"
            className="board-ai-badge-history"
            onClick={() => setHistoryOpen((open) => !open)}
            aria-label="Mostra chat passate"
            aria-expanded={historyOpen}
            title="Chat passate"
          >
            <TimeHistoryIcon aria-hidden />
          </button>
          <span>New chat</span>
        </div>
        <button type="button" onClick={onClose} aria-label="Chiudi New chat">
          <XIcon aria-hidden />
        </button>
      </header>
      {historyOpen && (
        <div className="board-ai-badge-history-menu" role="status">
          Nessuna chat precedente
        </div>
      )}
      <div className="board-ai-badge-body">
        {!historyOpen && (
          <img
            className="board-ai-badge-empty-logo"
            src="/sprout-ai-logo.png"
            alt=""
          />
        )}
        <label className="agent-chat-composer board-ai-badge-composer">
          <textarea
            ref={inputRef}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Ask everything"
            aria-label="Messaggio per Ask to AI"
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
            title="Seleziona modello"
          >
            Sprout 1
            <ChevronDownIcon aria-hidden />
          </button>
          <button
            type="button"
            className="agent-chat-send"
            disabled={!draft.trim()}
            aria-label="Invia messaggio"
            title="Disponibile prossimamente"
          >
            <svg viewBox="0 0 24 24" fill="none" aria-hidden>
              <path d="M12 19V5m0 0-6 6m6-6 6 6" />
            </svg>
          </button>
        </label>
      </div>
    </section>,
    document.body,
  )
}

const BoardPathBadge = ({
  topics,
  onSelectFocus,
  onClose,
}: {
  topics: TopicItem[]
  onSelectFocus(focus: BoardFocus): void
  onClose(): void
}) => {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  const select = (focus: BoardFocus) => {
    onSelectFocus(focus)
    onClose()
  }

  return createPortal(
    <section className="board-path-badge" role="dialog" aria-label="Percorso file">
      <header className="board-path-badge-header">
        <span className="sr-only">Navigazione workspace</span>
        <button type="button" onClick={onClose} aria-label="Chiudi percorso file">
          <XIcon aria-hidden />
        </button>
      </header>
      <nav className="board-path-badge-nav" aria-label="Navigazione workspace">
        <button type="button" onClick={() => select({ type: 'members' })}>
          <SidebarUserIcon aria-hidden />
          Membri
        </button>
        <button type="button" onClick={() => select({ type: 'agents' })}>
          <SidebarAgentIcon aria-hidden />
          Agenti
        </button>
        <p>Spazio</p>
        <button type="button" onClick={() => select({ type: 'generali' })}>
          <SidebarHomeIcon aria-hidden />
          Generali
        </button>
        {topics.map((topic) => (
          <button
            key={topic.wire.id}
            type="button"
            onClick={() => select({ type: 'topic', topicId: topic.wire.id })}
          >
            {topic.document ? <SidebarHomeIcon aria-hidden /> : <LockIcon aria-hidden />}
            {topic.document?.name ?? 'Locked topic'}
          </button>
        ))}
      </nav>
    </section>,
    document.body,
  )
}

const AgentViewNavigation = ({
  workspace,
  onBack,
}: {
  workspace?: { name: string; avatar: string }
  onBack(): void
}) => (
  <div className="board-view-navigation" aria-label="Viste agenti">
    <div className="board-view-mode-switch">
      {workspace ? (
        <button
          type="button"
          className="board-view-mode-option active agent-view-back"
          onClick={onBack}
          aria-label="Torna alla panoramica agenti"
        >
          <svg viewBox="0 0 24 24" fill="none" aria-hidden>
            <path d="M19 12H5m6-6-6 6 6 6" />
          </svg>
          Back
        </button>
      ) : (
        <span className="board-view-mode-option active" aria-current="page">
          Overview
        </span>
      )}
    </div>
  </div>
)

const TaskListDetailViewNavigation = ({ onBack }: { onBack(): void }) => (
  <div className="board-view-navigation" aria-label="Vista tasklist">
    <div className="board-view-mode-switch">
      <button
        type="button"
        className="board-view-mode-option active agent-view-back"
        onClick={onBack}
        aria-label="Torna alla board"
      >
        <svg viewBox="0 0 24 24" fill="none" aria-hidden>
          <path d="M19 12H5m6-6-6 6 6 6" />
        </svg>
        Back
      </button>
    </div>
  </div>
)

const BoardMobileIslandViewModes = ({
  mode,
  onChange,
}: {
  mode: BoardViewMode
  onChange(mode: BoardViewMode): void
}) => (
  <>
    <button
      type="button"
      className={
        mode === 'overview'
          ? 'board-mobile-island-toggle active'
          : 'board-mobile-island-toggle'
      }
      aria-label="Overview"
      aria-current={mode === 'overview' ? 'page' : undefined}
      onClick={() => onChange('overview')}
    >
      <FolderIcon aria-hidden />
    </button>
    <button
      type="button"
      className={
        mode === 'board'
          ? 'board-mobile-island-toggle active'
          : 'board-mobile-island-toggle'
      }
      aria-label="Board"
      aria-current={mode === 'board' ? 'page' : undefined}
      onClick={() => onChange('board')}
    >
      <LayoutGridIcon aria-hidden />
    </button>
    <button
      type="button"
      className={
        mode === 'timeline'
          ? 'board-mobile-island-toggle active'
          : 'board-mobile-island-toggle'
      }
      aria-label="Timeline"
      aria-current={mode === 'timeline' ? 'page' : undefined}
      onClick={() => onChange('timeline')}
    >
      <CalendarIcon aria-hidden />
    </button>
    <button
      type="button"
      className={
        mode === 'history'
          ? 'board-mobile-island-toggle active'
          : 'board-mobile-island-toggle'
      }
      aria-label="History"
      aria-current={mode === 'history' ? 'page' : undefined}
      onClick={() => onChange('history')}
    >
      <ClockIcon aria-hidden />
    </button>
  </>
)

const BoardOverviewView = ({
  project,
  topic,
  onLoadProjectInfo,
  onCreateProjectInfoDocument,
  onLoadTopicInfo,
  onCreateTopicInfoDocument,
  onUpdateInfoDocument,
  onUploadInfoDocumentFile,
  onReadInfoDocumentFile,
  onDownloadInfoDocumentFile,
}: {
  project?: ProjectItem
  topic?: TopicItem
  onLoadProjectInfo(project: ProjectItem): Promise<DecryptedInfoDocument[]>
  onCreateProjectInfoDocument(
    project: ProjectItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onLoadTopicInfo(topic: TopicItem): Promise<DecryptedInfoDocument[]>
  onCreateTopicInfoDocument(
    topic: TopicItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUpdateInfoDocument(
    document: DecryptedInfoDocument,
    content: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUploadInfoDocumentFile(
    document: DecryptedInfoDocument,
    file: File,
  ): Promise<InfoFileBlock>
  onReadInfoDocumentFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<Blob>
  onDownloadInfoDocumentFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<void>
}) => {
  const scopeName = topic
    ? (topic.document?.name ?? 'Categoria protetta')
    : (project?.document?.name ?? 'Generali')
  return (
    <section className="board-overview" aria-label={`Overview ${scopeName}`}>
      <div className="board-overview-document">
        <div className="board-overview-scroll">
        {topic ? (
          <InfoDocumentPanel
            container={topic}
            presentation="overview"
            overviewTitle={scopeName}
            onLoad={onLoadTopicInfo}
            onCreateDocument={onCreateTopicInfoDocument}
            onUpdateDocument={onUpdateInfoDocument}
            onUploadFile={onUploadInfoDocumentFile}
            onReadFile={onReadInfoDocumentFile}
            onDownloadFile={onDownloadInfoDocumentFile}
          />
        ) : project ? (
          <InfoDocumentPanel
            container={project}
            presentation="overview"
            overviewTitle={scopeName}
            onLoad={onLoadProjectInfo}
            onCreateDocument={onCreateProjectInfoDocument}
            onUpdateDocument={onUpdateInfoDocument}
            onUploadFile={onUploadInfoDocumentFile}
            onReadFile={onReadInfoDocumentFile}
            onDownloadFile={onDownloadInfoDocumentFile}
          />
        ) : null}
        </div>
      </div>
    </section>
  )
}

const BoardHistoryView = ({
  tasks,
  boardMembers,
  taskLists,
  groupModes,
  selectedTaskId,
  scopeName,
  onSelectTask,
}: {
  tasks: DecryptedTask[]
  boardMembers: BoardMember[]
  taskLists: TaskListItem[]
  groupModes: Array<'tasklist' | 'type' | 'member' | 'state' | 'date'>
  selectedTaskId?: Uuid
  scopeName: string
  onSelectTask(id: Uuid): void
}) => {
  return (
    <section className="board-history" aria-label={`History ${scopeName}`}>
      <div
        className={
          tasks.length === 0
            ? 'board-history-scroll board-history-scroll--empty'
            : 'board-history-scroll'
        }
      >
        <TaskHistoryRows
          tasks={tasks}
          boardMembers={boardMembers}
          taskLists={taskLists}
          groupModes={groupModes}
          selectedTaskId={selectedTaskId}
          emptyMessage="Non ci sono ancora task nello storico."
          onSelectTask={onSelectTask}
        />
      </div>
    </section>
  )
}

const SIDEBAR_COLLAPSED_KEY = 'sprout-board-sidebar-collapsed'
const SIDEBAR_WIDTH_KEY = 'sprout-board-sidebar-width'
const SIDEBAR_DEFAULT_WIDTH = 268
const SIDEBAR_MIN_WIDTH = 220
const SIDEBAR_MAX_WIDTH = 420
const RECENT_SEARCHES_KEY = 'sprout-board-recent-searches'
const MAX_RECENT_SEARCHES = 4

const readRecentSearches = (): string[] => {
  try {
    const raw = localStorage.getItem(RECENT_SEARCHES_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((item): item is string => typeof item === 'string')
      .map((item) => item.trim())
      .filter(Boolean)
      .slice(0, MAX_RECENT_SEARCHES)
  } catch {
    return []
  }
}

const persistRecentSearches = (items: string[]): void => {
  try {
    localStorage.setItem(
      RECENT_SEARCHES_KEY,
      JSON.stringify(items.slice(0, MAX_RECENT_SEARCHES)),
    )
  } catch {
    // ignore storage failures
  }
}

const BoardMobileRecentSearches = ({
  items,
  onSelect,
}: {
  items: string[]
  onSelect(query: string): void
}) => (
  <div className="board-mobile-search-recent">
    <p className="board-mobile-search-recent-label">Ricerche recenti</p>
    <ul className="board-mobile-search-recent-list">
      {items.map((query) => (
        <li key={query}>
          <button
            type="button"
            className="board-mobile-search-recent-item"
            onClick={() => onSelect(query)}
          >
            <SearchIcon aria-hidden />
            <span>{query}</span>
          </button>
        </li>
      ))}
    </ul>
  </div>
)

const readSidebarCollapsed = (): boolean => {
  try {
    if (window.matchMedia('(max-width: 850px)').matches) return true
    return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === 'true'
  } catch {
    return false
  }
}

const persistSidebarCollapsed = (collapsed: boolean): void => {
  try {
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(collapsed))
  } catch {
    // ignore storage failures
  }
}

const readSidebarWidth = (): number => {
  try {
    const storedValue = localStorage.getItem(SIDEBAR_WIDTH_KEY)
    if (!storedValue) return SIDEBAR_DEFAULT_WIDTH
    const storedWidth = Number(storedValue)
    if (!Number.isFinite(storedWidth)) return SIDEBAR_DEFAULT_WIDTH
    return Math.min(
      SIDEBAR_MAX_WIDTH,
      Math.max(SIDEBAR_MIN_WIDTH, storedWidth),
    )
  } catch {
    return SIDEBAR_DEFAULT_WIDTH
  }
}

const persistSidebarWidth = (width: number): void => {
  try {
    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(Math.round(width)))
  } catch {
    // ignore storage failures
  }
}

const BoardFilterDropdown = ({
  filters,
  groupBy,
  taskLists,
  members,
  onChange,
  onGroupBy,
  onReset,
}: {
  filters: AdvancedTaskFilters
  groupBy: TaskFilterGroup[]
  taskLists: TaskListItem[]
  members: BoardMember[]
  onChange(filters: AdvancedTaskFilters): void
  onGroupBy(group: TaskFilterGroup): void
  onReset(): void
}) => {
  const [open, setOpen] = useState(false)
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false)
  const [activeSection, setActiveSection] =
    useState<keyof AdvancedTaskFilters | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const menuId = useId()
  const detailedFilterCount = Object.values(filters).reduce(
    (count, values) => count + values.length,
    0,
  )
  const groupingCount =
    groupBy.length === 1 && groupBy[0] === 'dates' ? 0 : groupBy.length
  const activeCount = detailedFilterCount + groupingCount
  const requestReset = () => {
    setOpen(false)
    setActiveSection(null)
    setResetConfirmOpen(true)
  }
  const confirmReset = () => {
    onReset()
    setResetConfirmOpen(false)
  }

  useEffect(() => {
    if (!open) return
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false)
      }
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  useEffect(() => {
    if (!resetConfirmOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setResetConfirmOpen(false)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [resetConfirmOpen])

  const toggle = (key: keyof AdvancedTaskFilters, value: string) => {
    const current = filters[key] as string[]
    const adding = !current.includes(value)
    if (adding && !groupBy.includes(key)) onGroupBy(key)
    onChange({
      ...filters,
      [key]: current.includes(value)
        ? current.filter((item) => item !== value)
        : [...current, value],
    } as AdvancedTaskFilters)
  }

  const sections: Array<{
    title: string
    key: keyof AdvancedTaskFilters
    options: ReadonlyArray<readonly [string, string]>
  }> = [
    {
      title: 'Board',
      key: 'listIds',
      options: taskLists
        .filter((list) => list.document)
        .map((list) => [list.wire.id, list.document?.name ?? 'Tasklist'] as const),
    },
    {
      title: 'Tipologia',
      key: 'types',
      options: [
        ['priority', 'Priorità'],
        ['deadline', 'Scadenza'],
        ['recurring', 'Ricorsività'],
      ],
    },
    {
      title: 'Membro',
      key: 'memberIds',
      options: members.map((member) => [member.identityId, member.label] as const),
    },
    {
      title: 'Stato',
      key: 'states',
      options: [
        ['open', 'Da completare'],
        ['completed', 'Completati'],
      ],
    },
    {
      title: 'Data',
      key: 'dates',
      options: [
        ['overdue', 'Scaduti'],
        ['today', 'Oggi'],
        ['upcoming', 'Prossimi'],
        ['none', 'Senza data'],
      ],
    },
  ]

  return (
    <>
      <div className="board-filter-dropdown" ref={rootRef}>
      <button
        type="button"
        className={`board-filter-trigger${activeCount > 0 ? ' has-active-filters' : ''}`}
        aria-expanded={open}
        aria-haspopup="menu"
        aria-controls={menuId}
        aria-label={`Filtra task${activeCount > 0 ? `: ${activeCount} filtri attivi` : ''}`}
        onClick={() => setOpen((value) => !value)}
      >
        {activeCount > 0 && (
          <span
            className="board-filter-active-count"
            role="button"
            tabIndex={0}
            aria-label="Azzera filtri"
            onClick={(event) => {
              event.stopPropagation()
              requestReset()
            }}
            onKeyDown={(event) => {
              if (event.key !== 'Enter' && event.key !== ' ') return
              event.preventDefault()
              event.stopPropagation()
              requestReset()
            }}
          >
            <span className="board-filter-active-count-value">{activeCount}</span>
            <span className="board-filter-active-count-reset" aria-hidden>×</span>
          </span>
        )}
        <FilterIcon />
      </button>
      {open && (
        <div
          id={menuId}
          className="board-filter-menu"
          role="menu"
          aria-label="Filtra task"
        >
          {sections.map((section) => {
            const sectionCount = filters[section.key].length
            const expanded = activeSection === section.key
            return (
              <div
                key={section.key}
                className="board-filter-category-wrap"
              >
                <div className={groupBy.includes(section.key) ? 'board-filter-category active' : 'board-filter-category'}>
                  <button
                    type="button"
                    className="board-filter-category-select"
                    aria-pressed={groupBy.includes(section.key)}
                    onClick={() => onGroupBy(section.key)}
                  >
                    <span
                      className={`board-filter-selection-circle${groupBy.includes(section.key) ? ' selected' : ''}`}
                      aria-hidden
                    >
                      {groupBy.includes(section.key) && <CheckIcon />}
                    </span>
                    <span
                      className={`board-filter-category-label board-filter-category-label--${section.key.toLowerCase()}`}
                    >
                      {section.title}
                    </span>
                  </button>
                  <span className="board-filter-category-meta">
                    {sectionCount > 0 && <span className="board-filter-count">{sectionCount}</span>}
                    <button
                      type="button"
                      className="board-filter-category-expand"
                      aria-label={`Apri filtri ${section.title}`}
                      aria-expanded={expanded}
                      onClick={() => setActiveSection(expanded ? null : section.key)}
                    >
                      <span className="board-filter-category-arrow" aria-hidden>›</span>
                    </button>
                  </span>
                </div>
                {expanded && (
                  <div className="board-filter-submenu" role="menu" aria-label={section.title}>
                    {section.options.length > 0 ? (
                      section.options.map(([value, label]) => {
                        const selected = (filters[section.key] as string[]).includes(value)
                        const member =
                          section.key === 'memberIds'
                            ? members.find((item) => item.identityId === value)
                            : undefined
                        const list =
                          section.key === 'listIds'
                            ? taskLists.find((item) => item.wire.id === value)
                            : undefined
                        const listColor = list
                          ? resolveTaskListIconColorFromStored(
                              list.document?.color,
                              list.wire.id,
                            )
                          : undefined
                        const optionTone =
                          section.key === 'types'
                            ? value === 'priority'
                              ? 'warning'
                              : value === 'deadline'
                                ? 'orange'
                                : 'violet'
                            : section.key === 'states'
                              ? value === 'completed'
                                ? 'success'
                                : 'info'
                              : section.key === 'dates'
                                ? 'mauve'
                                : 'neutral'
                        return (
                          <button
                            type="button"
                            key={value}
                            role="menuitemcheckbox"
                            aria-checked={selected}
                            className={selected ? 'board-filter-option active' : 'board-filter-option'}
                            onClick={() => toggle(section.key, value)}
                          >
                            <span
                              className={`board-filter-selection-circle${selected ? ' selected' : ''}`}
                              aria-hidden
                            >
                              {selected && <CheckIcon />}
                            </span>
                            {member ? (
                              <>
                                <span
                                  className={`board-avatar member ${memberAvatarColorClass(member.identityId)}`}
                                  aria-hidden
                                >
                                  {initialFor(member.label)}
                                </span>
                                <span>{member.label}</span>
                              </>
                            ) : (
                              <span
                                className={`tasklist-history-day-label tasklist-history-day-label--${optionTone}${listColor ? ` tasklist-history-day-label--${listColor}` : ''}`}
                              >
                                {label}
                              </span>
                            )}
                          </button>
                        )
                      })
                    ) : (
                      <p className="board-filter-empty">Nessuna opzione</p>
                    )}
                  </div>
                )}
              </div>
            )
          })}
          {activeCount > 0 && (
            <button
              type="button"
              className="board-filter-reset"
              onClick={requestReset}
            >
              Azzera filtri
            </button>
          )}
        </div>
      )}
      </div>
      {resetConfirmOpen &&
        createPortal(
          <div
            className="task-create-overlay filter-reset-confirm-overlay"
            onClick={() => setResetConfirmOpen(false)}
          >
            <div className="task-create-backdrop" aria-hidden="true" />
            <section
              className="filter-reset-confirm-dialog"
              role="alertdialog"
              aria-modal="true"
              aria-labelledby="filter-reset-confirm-title"
              aria-describedby="filter-reset-confirm-description"
              onClick={(event) => event.stopPropagation()}
            >
              <h2 id="filter-reset-confirm-title">Azzera filtri?</h2>
              <p id="filter-reset-confirm-description">
                Sei sicuro di voler azzerare i filtri?
              </p>
              <div className="filter-reset-confirm-actions">
                <button
                  type="button"
                  className="filter-reset-confirm-cancel"
                  onClick={() => setResetConfirmOpen(false)}
                >
                  Annulla
                </button>
                <button
                  type="button"
                  className="filter-reset-confirm-submit"
                  onClick={confirmReset}
                >
                  Conferma
                </button>
              </div>
            </section>
          </div>,
          document.body,
        )}
    </>
  )
}

const AgentFilterDropdown = ({
  filter,
  onFilter,
}: {
  filter: AgentActivityFilter
  onFilter(filter: AgentActivityFilter): void
}) => {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const menuId = useId()

  useEffect(() => {
    if (!open) return
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false)
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  return (
    <div className="board-filter-dropdown" ref={rootRef}>
      <button
        type="button"
        className="board-filter-trigger"
        aria-expanded={open}
        aria-haspopup="menu"
        aria-controls={menuId}
        aria-label="Filtra agenti"
        onClick={() => setOpen((value) => !value)}
      >
        <FilterIcon />
      </button>
      {open && (
        <div
          id={menuId}
          className="board-filter-menu"
          role="menu"
          aria-label="Filtra agenti"
        >
          {AGENT_FILTER_OPTIONS.map(([value, label]) => (
            <button
              type="button"
              key={value}
              role="menuitemradio"
              aria-checked={filter === value}
              className={
                filter === value
                  ? 'board-filter-option active'
                  : 'board-filter-option'
              }
              onClick={() => {
                onFilter(value)
                setOpen(false)
              }}
            >
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

const BoardMobileSearchResults = ({
  query,
  lists,
  tasks,
  memberColumns,
  isMemberBoard,
  boardMemberById,
  onSelectTask,
  onSelectList,
}: {
  query: string
  lists: TaskListItem[]
  tasks: DecryptedTask[]
  memberColumns: BoardMember[]
  isMemberBoard: boolean
  boardMemberById: Map<Uuid, BoardMember>
  onSelectTask(id: Uuid): void
  onSelectList(id: Uuid): void
}) => {
  const normalized = query.trim()
  if (!normalized) return null

  if (isMemberBoard) {
    const tasksByAssignee = new Map<Uuid, DecryptedTask[]>()
    for (const task of tasks) {
      const assignee = task.wire.active_assignee_identity_id
      if (!assignee) continue
      const bucket = tasksByAssignee.get(assignee)
      if (bucket) bucket.push(task)
      else tasksByAssignee.set(assignee, [task])
    }

    const visibleMembers = memberColumns.filter((member) => {
      if (member.label.toLowerCase().includes(normalized.toLowerCase())) return true
      return (tasksByAssignee.get(member.identityId)?.length ?? 0) > 0
    })

    if (visibleMembers.length === 0) {
      return (
        <p className="board-mobile-search-empty">
          Nessun risultato per &ldquo;{normalized}&rdquo;
        </p>
      )
    }

    return (
      <ul className="board-mobile-search-results-list">
        {visibleMembers.map((member) => {
          const memberTasks = tasksByAssignee.get(member.identityId) ?? []
          return (
            <li key={member.identityId} className="board-mobile-search-group">
              <div className="board-mobile-search-group-label">{member.label}</div>
              {memberTasks.length > 0 ? (
                <ul className="board-mobile-search-sublist">
                  {memberTasks.map((task) => (
                    <BoardMobileSearchResultTask
                      key={task.wire.id}
                      task={task}
                      boardMemberById={boardMemberById}
                      onSelect={() => onSelectTask(task.wire.id)}
                    />
                  ))}
                </ul>
              ) : null}
            </li>
          )
        })}
      </ul>
    )
  }

  if (lists.length === 0) {
    return (
      <p className="board-mobile-search-empty">
        Nessun risultato per &ldquo;{normalized}&rdquo;
      </p>
    )
  }

  const listNameById = new Map(
    lists.map((list) => [list.wire.id, list.document?.name ?? 'Tasklist']),
  )

  return (
    <ul className="board-mobile-search-results-list">
      {lists.map((list) => {
        const listTasks = tasks.filter((task) => task.wire.list_id === list.wire.id)
        const listName = listNameById.get(list.wire.id) ?? 'Tasklist'
        const listMatches = listName.toLowerCase().includes(normalized.toLowerCase())

        return (
          <li key={list.wire.id} className="board-mobile-search-group">
            {listMatches && list.document && (
              <button
                type="button"
                className="board-mobile-search-result-list"
                onClick={() => onSelectList(list.wire.id)}
              >
                <span
                  className={`${columnAvatarColorClass(
                    resolveTaskListIconColorFromStored(
                      list.document.color,
                      list.wire.id,
                    ),
                  )} board-mobile-search-result-list-avatar`}
                  aria-hidden
                >
                  <TaskListAvatarContent
                    icon={list.document.icon}
                    fallbackInitial={initialFor(listName)}
                  />
                </span>
                <span className="board-mobile-search-result-title">{listName}</span>
              </button>
            )}
            {listTasks.length > 0 ? (
              <ul className="board-mobile-search-sublist">
                {listTasks.map((task) => (
                  <BoardMobileSearchResultTask
                    key={task.wire.id}
                    task={task}
                    subtitle={listMatches ? undefined : listName}
                    boardMemberById={boardMemberById}
                    onSelect={() => onSelectTask(task.wire.id)}
                  />
                ))}
              </ul>
            ) : null}
          </li>
        )
      })}
    </ul>
  )
}

const BoardMobileSearchResultTask = ({
  task,
  subtitle,
  boardMemberById,
  onSelect,
}: {
  task: DecryptedTask
  subtitle?: string
  boardMemberById: Map<Uuid, BoardMember>
  onSelect(): void
}) => {
  const status = getTaskStatusIndicator(task)
  const open = task.wire.state.state === 'open'

  return (
    <li>
      <button type="button" className="board-mobile-search-result-task" onClick={onSelect}>
        <span
          className={`board-mobile-search-result-dot board-task-check board-task-check--${status.variant}`}
          aria-hidden
        >
          <span className="board-task-check-dot" />
        </span>
        <span className="board-mobile-search-result-copy">
          <span className="board-mobile-search-result-title">{task.document.title}</span>
          {subtitle ? (
            <span className="board-mobile-search-result-subtitle">{subtitle}</span>
          ) : null}
          {!open ? (
            <span className="board-mobile-search-result-meta">Completato</span>
          ) : task.document.due_at ? (
            <span className="board-mobile-search-result-meta">
              {formatTaskCardDueDate(task.document.due_at)}
            </span>
          ) : null}
        </span>
        {taskAssociatedUsers(task, boardMemberById).length > 0 ? (
          <BoardCardAssignee users={taskAssociatedUsers(task, boardMemberById)} />
        ) : null}
      </button>
    </li>
  )
}

const formatDeadlinePillLabel = (dueAt: string) =>
  `Scad · ${formatDueDate(dueAt)}`

const TASK_STATE_LABELS = {
  open: 'Aperto',
  completed: 'Completato',
} as const

export type TaskUpdateInput = {
  title: string
  notes?: string
  priority?: TaskDocument['priority']
  start_at?: string
  due_at?: string
  recurrence?: TaskDocument['recurrence']
}

const toDatetimeLocalValue = (iso?: string): string => {
  if (!iso) return ''
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}

const resizeTaskPanelComment = (
  textarea: HTMLTextAreaElement | null,
  panel: HTMLElement | null,
) => {
  if (!textarea || !panel) return

  const panelStyles = getComputedStyle(panel)
  const panelCanGrow = panel.scrollHeight <= panel.clientHeight + 1
  panel.classList.toggle('task-create-panel--comment-scroll', !panelCanGrow)

  textarea.style.height = 'auto'
  textarea.style.overflowY = 'hidden'

  const scrollHeight = textarea.scrollHeight

  const form = panel.querySelector<HTMLElement>('.task-create-form')
  const header = panel.querySelector<HTMLElement>('.task-create-header')
  const actions = panel.querySelector<HTMLElement>('.task-create-actions')
  const attachmentList = panel.querySelector<HTMLElement>(
    '.task-create-attachment-list',
  )
  const commentWrap = textarea.closest<HTMLElement>('.task-create-comment-wrap')

  const formStyles = form ? getComputedStyle(form) : null
  const formGap = formStyles
    ? parseFloat(formStyles.rowGap || formStyles.gap || '0')
    : 0
  const sectionStyles = textarea.closest('.task-create-comment-section')
    ? getComputedStyle(
        textarea.closest('.task-create-comment-section') as HTMLElement,
      )
    : null
  const sectionGap = sectionStyles
    ? parseFloat(sectionStyles.rowGap || sectionStyles.gap || '0')
    : 0
  const wrapStyles = commentWrap ? getComputedStyle(commentWrap) : null
  const wrapPadding = wrapStyles
    ? parseFloat(wrapStyles.paddingTop) + parseFloat(wrapStyles.paddingBottom)
    : 0

  const attachmentsHeight = attachmentList?.offsetHeight ?? 0
  const panelPadding =
    parseFloat(panelStyles.paddingTop) + parseFloat(panelStyles.paddingBottom)
  const reserved =
    panelPadding +
    (header?.offsetHeight ?? 0) +
    (actions?.offsetHeight ?? 0) +
    attachmentsHeight +
    formGap * 2 +
    (attachmentsHeight > 0 ? sectionGap : 0) +
    wrapPadding

  const minTextareaHeight = 72

  if (panelCanGrow) {
    textarea.style.height = `${Math.max(scrollHeight, minTextareaHeight)}px`
    textarea.style.overflowY = 'hidden'
    return
  }

  const maxTextareaHeight = Math.max(
    minTextareaHeight,
    panel.clientHeight - reserved,
  )
  const nextHeight = Math.min(scrollHeight, maxTextareaHeight)
  textarea.style.height = `${nextHeight}px`
  textarea.style.overflowY = scrollHeight > nextHeight ? 'auto' : 'hidden'
}

const CreateTaskSegmentGroup = ({
  ariaLabel,
  options,
  value,
  onChange,
  variant = 'pills',
  badgeClassForValue,
}: {
  ariaLabel: string
  options: ReadonlyArray<
    readonly [string, string] | readonly [string, string, TaskKindOption[2]]
  >
  value: string
  onChange(next: string): void
  variant?: 'pills' | 'kind' | 'badges' | 'segmented'
  badgeClassForValue?: (optionValue: string) => string | undefined
}) => (
  <div
    className={
      variant === 'kind'
        ? 'task-create-kind-tabs'
        : variant === 'segmented'
          ? 'task-create-segment-group task-create-segment-group--segmented'
          : 'task-create-segments'
    }
    role="radiogroup"
    aria-label={ariaLabel}
  >
    {options.map((option) => {
      const [optionValue, label, Icon] = option
      const selected = value === optionValue
      const badgeClass = badgeClassForValue?.(optionValue) ?? ''
      const className =
        variant === 'segmented'
          ? ['task-create-segment-btn', badgeClass, selected ? 'selected' : '']
              .filter(Boolean)
              .join(' ')
          : variant === 'badges'
            ? selected
              ? `task-create-pill-btn ${badgeClass} selected`.trim()
              : 'task-create-pill-btn task-create-pill-btn--ghost'
            : selected
              ? 'task-create-segment selected'
              : 'task-create-segment'

      return (
        <button
          type="button"
          key={optionValue}
          role="radio"
          aria-checked={selected}
          className={className}
          onClick={() => onChange(optionValue)}
        >
          {Icon ? <Icon aria-hidden /> : null}
          {label}
        </button>
      )
    })}
  </div>
)

const TaskPanelFooterTools = ({
  publishedQuestionnaireVersions,
  questionnaireVersionId,
  onQuestionnaireVersionIdChange,
  attachmentFiles,
  hasAttachments,
  onAddAttachmentFiles,
  onAttachmentPickerOpen,
  boardMembers,
  assigneeIdentityId,
  onAssigneeIdentityIdChange,
  taskKind,
  onTaskKindChange,
  taskPriority,
  onTaskPriorityChange,
  taskDueAt,
  onTaskDueAtChange,
  recurrenceFrequency,
  onRecurrenceFrequencyChange,
  recurrenceInterval,
  onRecurrenceIntervalChange,
  showAssignButton = true,
  showDueDateCommittedPreview = false,
}: {
  publishedQuestionnaireVersions: Array<{ id: Uuid; label: string }>
  questionnaireVersionId: string
  onQuestionnaireVersionIdChange(id: string): void
  attachmentFiles: File[]
  hasAttachments?: boolean
  onAddAttachmentFiles(files: FileList | File[]): void
  onAttachmentPickerOpen?(): void
  boardMembers: BoardMember[]
  assigneeIdentityId?: Uuid
  onAssigneeIdentityIdChange(identityId: Uuid): void
  taskKind: 'priority' | 'deadline' | 'recurring'
  onTaskKindChange(kind: 'priority' | 'deadline' | 'recurring'): void
  taskPriority: 'low' | 'normal' | 'high'
  onTaskPriorityChange(priority: 'low' | 'normal' | 'high'): void
  taskDueAt: string
  onTaskDueAtChange(value: string): void
  recurrenceFrequency: RecurrenceFrequency
  onRecurrenceFrequencyChange(frequency: RecurrenceFrequency): void
  recurrenceInterval: string
  onRecurrenceIntervalChange(value: string): void
  showAssignButton?: boolean
  showDueDateCommittedPreview?: boolean
}) => {
  const plusMenuRef = useRef<HTMLDivElement>(null)
  const kindMenuRef = useRef<HTMLDivElement>(null)
  const kindMenuPopoverRef = useRef<HTMLDivElement>(null)
  const kindTriggerRef = useRef<HTMLButtonElement>(null)
  const assignMenuRef = useRef<HTMLDivElement>(null)
  const attachmentInputRef = useRef<HTMLInputElement>(null)
  const [plusMenuOpen, setPlusMenuOpen] = useState(false)
  const [kindMenuOpen, setKindMenuOpen] = useState(false)
  const [activeKindSubmenu, setActiveKindSubmenu] = useState<
    'priority' | 'deadline' | 'recurring' | null
  >(null)
  const [kindMenuPosition, setKindMenuPosition] = useState<CSSProperties | null>(
    null,
  )
  const [assignMenuOpen, setAssignMenuOpen] = useState(false)
  const attachmentInputId = useId()
  const selectedAssignee = boardMembers.find(
    (member) => member.identityId === assigneeIdentityId,
  )
  const attachmentsSelected =
    hasAttachments ?? attachmentFiles.length > 0

  const kindConfigured =
    taskKind === 'recurring'
      ? recurrenceInterval !== '1' || recurrenceFrequency !== 'daily'
      : taskKind === 'deadline'
        ? taskDueAt !== ''
        : taskKind === 'priority'
          ? taskPriority !== 'normal'
          : false

  const kindTriggerLabel =
    taskKind === 'priority'
      ? taskPriorityPillLabel(taskPriority)
      : taskKind === 'deadline'
        ? taskDueAt
          ? formatDeadlinePillLabel(new Date(taskDueAt).toISOString())
          : 'Scad'
        : 'Ricorrente'

  const kindTriggerPillClass =
    taskKind === 'deadline'
      ? 'task-create-pill-btn--deadline'
      : taskKind === 'priority'
        ? taskPriorityPillClass(taskPriority)
        : ''

  const KindTriggerIcon = taskKindIcon(taskKind)

  useLayoutEffect(() => {
    if (!kindMenuOpen) {
      setKindMenuPosition(null)
      return
    }

    const trigger = kindTriggerRef.current
    const menu = kindMenuPopoverRef.current
    if (!trigger) return

    const rect = trigger.getBoundingClientRect()
    const gap = 9
    const padding = 8
    const menuHeight = menu?.offsetHeight ?? 0
    const menuWidth = menu?.offsetWidth ?? 288

    let top = rect.bottom + gap
    if (menuHeight > 0 && top + menuHeight > window.innerHeight - padding) {
      const aboveTop = rect.top - gap - menuHeight
      top =
        aboveTop >= padding
          ? aboveTop
          : Math.max(padding, window.innerHeight - menuHeight - padding)
    }

    let left = rect.left
    if (left + menuWidth > window.innerWidth - padding) {
      left = window.innerWidth - menuWidth - padding
    }
    left = Math.max(padding, left)

    setKindMenuPosition({ top, left, bottom: 'auto' })
  }, [
    kindMenuOpen,
    taskKind,
    taskPriority,
    taskDueAt,
    recurrenceFrequency,
    recurrenceInterval,
    activeKindSubmenu,
  ])

  useEffect(() => {
    if (!kindMenuOpen) {
      setActiveKindSubmenu(null)
    }
  }, [kindMenuOpen])

  useEffect(() => {
    if (!plusMenuOpen && !kindMenuOpen && !assignMenuOpen) return
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (plusMenuRef.current?.contains(target)) return
      if (kindMenuRef.current?.contains(target)) return
      if (kindMenuPopoverRef.current?.contains(target)) return
      if (assignMenuRef.current?.contains(target)) return
      setPlusMenuOpen(false)
      setKindMenuOpen(false)
      setAssignMenuOpen(false)
    }
    document.addEventListener('mousedown', onPointerDown)
    return () => document.removeEventListener('mousedown', onPointerDown)
  }, [plusMenuOpen, kindMenuOpen, assignMenuOpen])

  return (
    <>
      <div className="task-create-tool-menu-wrap" ref={plusMenuRef}>
        <button
          type="button"
          className={
            questionnaireVersionId
              ? 'task-create-icon-btn selected'
              : 'task-create-icon-btn'
          }
          aria-label="Aggiungi"
          aria-haspopup="menu"
          aria-expanded={plusMenuOpen}
          onClick={() => {
            setKindMenuOpen(false)
            setAssignMenuOpen(false)
            setPlusMenuOpen((open) => !open)
          }}
        >
          <PlusIcon />
        </button>
        {plusMenuOpen && (
          <div
            className="task-create-tool-menu"
            role="menu"
            aria-label="Aggiungi"
          >
            <p className="task-create-menu-label">Questionario</p>
            {publishedQuestionnaireVersions.length === 0 ? (
              <p className="task-create-menu-hint">
                Nessun questionario pubblicato disponibile.
              </p>
            ) : (
              <>
                <button
                  type="button"
                  role="menuitemradio"
                  aria-checked={questionnaireVersionId === ''}
                  className={
                    questionnaireVersionId === ''
                      ? 'task-create-questionnaire-option selected'
                      : 'task-create-questionnaire-option'
                  }
                  onClick={() => {
                    onQuestionnaireVersionIdChange('')
                    setPlusMenuOpen(false)
                  }}
                >
                  Nessuno
                </button>
                {publishedQuestionnaireVersions.map((version) => (
                  <button
                    type="button"
                    key={version.id}
                    role="menuitemradio"
                    aria-checked={questionnaireVersionId === version.id}
                    className={
                      questionnaireVersionId === version.id
                        ? 'task-create-questionnaire-option selected'
                        : 'task-create-questionnaire-option'
                    }
                    onClick={() => {
                      onQuestionnaireVersionIdChange(version.id)
                      setPlusMenuOpen(false)
                    }}
                  >
                    {version.label}
                  </button>
                ))}
              </>
            )}
          </div>
        )}
      </div>
      <input
        ref={attachmentInputRef}
        id={attachmentInputId}
        className="task-create-attachment-input"
        type="file"
        multiple
        onChange={(event) => {
          onAttachmentPickerOpen?.()
          if (event.target.files) {
            onAddAttachmentFiles(event.target.files)
          }
          event.target.value = ''
        }}
      />
      <button
        type="button"
        className={
          attachmentsSelected
            ? 'task-create-icon-btn selected'
            : 'task-create-icon-btn'
        }
        aria-label="Allega file"
        onClick={() => {
          setPlusMenuOpen(false)
          setKindMenuOpen(false)
          setAssignMenuOpen(false)
          onAttachmentPickerOpen?.()
          attachmentInputRef.current?.click()
        }}
      >
        <PaperclipIcon />
      </button>
      <div className="task-create-tool-menu-wrap" ref={kindMenuRef}>
        <button
          ref={kindTriggerRef}
          type="button"
          className={`task-create-pill-btn ${kindTriggerPillClass}${kindConfigured ? ' selected' : ''}`}
          aria-label="Priorità"
          aria-haspopup="dialog"
          aria-expanded={kindMenuOpen}
          onClick={() => {
            setPlusMenuOpen(false)
            setAssignMenuOpen(false)
            setKindMenuOpen((open) => {
              if (!open) {
                setActiveKindSubmenu(taskKind)
              }
              return !open
            })
          }}
        >
          <KindTriggerIcon aria-hidden />
          {kindTriggerLabel}
        </button>
        {kindMenuOpen &&
          createPortal(
            <div
              ref={kindMenuPopoverRef}
              className="task-create-tool-menu task-create-kind-menu task-create-kind-menu--floating"
              role="dialog"
              aria-label="Priorità e scadenza"
              style={
                kindMenuPosition ??
                (() => {
                  const rect = kindTriggerRef.current?.getBoundingClientRect()
                  if (!rect) return undefined
                  return {
                    top: rect.bottom + 9,
                    left: rect.left,
                    bottom: 'auto',
                  }
                })()
              }
              onClick={(event) => event.stopPropagation()}
              onMouseDown={(event) => event.stopPropagation()}
            >
              <div
                className="task-create-kind-menu-main"
                role="menu"
                aria-label="Tipo task"
              >
                {TASK_KIND_OPTIONS.map(([kindValue, label, Icon]) => {
                  const submenuOpen = activeKindSubmenu === kindValue
                  return (
                    <div
                      key={kindValue}
                      className={
                        submenuOpen
                          ? 'task-create-kind-menu-item-wrap active'
                          : 'task-create-kind-menu-item-wrap'
                      }
                      onMouseEnter={() => setActiveKindSubmenu(kindValue)}
                    >
                      <button
                        type="button"
                        role="menuitem"
                        aria-haspopup="menu"
                        aria-expanded={submenuOpen}
                        className={
                          submenuOpen || taskKind === kindValue
                            ? 'task-create-kind-menu-item selected'
                            : 'task-create-kind-menu-item'
                        }
                        onClick={() => {
                          setActiveKindSubmenu(kindValue)
                          onTaskKindChange(kindValue)
                        }}
                      >
                        <span className="task-create-kind-menu-item-label">
                          <Icon aria-hidden />
                          <span>{label}</span>
                        </span>
                        <span className="task-create-kind-menu-chevron" aria-hidden>
                          ›
                        </span>
                      </button>
                      {submenuOpen ? (
                        <div
                          className="task-create-kind-submenu"
                          role="menu"
                          aria-label={label}
                          onMouseDown={(event) => event.stopPropagation()}
                          onClick={(event) => event.stopPropagation()}
                        >
                          {kindValue === 'priority' ? (
                            TASK_PRIORITY_OPTIONS.map(([priorityValue, priorityLabel]) => (
                              <button
                                key={priorityValue}
                                type="button"
                                role="menuitemradio"
                                aria-checked={taskPriority === priorityValue}
                                className={[
                                  'task-create-kind-submenu-option',
                                  taskPrioritySegmentClass(priorityValue),
                                  taskPriority === priorityValue ? 'selected' : '',
                                ]
                                  .filter(Boolean)
                                  .join(' ')}
                                onClick={() => {
                                  onTaskKindChange('priority')
                                  onTaskPriorityChange(priorityValue)
                                  setKindMenuOpen(false)
                                }}
                              >
                                {priorityLabel}
                              </button>
                            ))
                          ) : kindValue === 'deadline' ? (
                            <NaturalLanguageDateField
                              required
                              label="Scadenza"
                              value={taskDueAt}
                              onChange={(value) => {
                                onTaskKindChange('deadline')
                                onTaskDueAtChange(value)
                              }}
                              showCommittedPreview={showDueDateCommittedPreview}
                            />
                          ) : (
                            <div className="task-create-recurrence-row">
                              <span className="task-create-recurrence-label">Ogni</span>
                              <input
                                required
                                className="task-create-recurrence-interval"
                                type="number"
                                min="1"
                                step="1"
                                value={recurrenceInterval}
                                aria-label="Intervallo ricorrenza"
                                onMouseDown={(event) => event.stopPropagation()}
                                onClick={(event) => event.stopPropagation()}
                                onChange={(event) => {
                                  onTaskKindChange('recurring')
                                  onRecurrenceIntervalChange(event.target.value)
                                }}
                              />
                              <CreateTaskSegmentGroup
                                ariaLabel="Unità ricorrenza"
                                variant="segmented"
                                options={RECURRENCE_FREQUENCY_OPTIONS}
                                value={recurrenceFrequency}
                                onChange={(next) => {
                                  onTaskKindChange('recurring')
                                  onRecurrenceFrequencyChange(
                                    next as RecurrenceFrequency,
                                  )
                                }}
                              />
                            </div>
                          )}
                        </div>
                      ) : null}
                    </div>
                  )
                })}
              </div>
            </div>,
            document.body,
          )}
      </div>
      {showAssignButton ? (
        <div className="task-create-tool-menu-wrap" ref={assignMenuRef}>
          <button
            type="button"
            className="task-create-pill-btn task-create-pill-btn--assign"
            aria-label="Assegna"
            aria-haspopup="menu"
            aria-expanded={assignMenuOpen}
            onClick={() => {
              setPlusMenuOpen(false)
              setKindMenuOpen(false)
              setAssignMenuOpen((open) => !open)
            }}
          >
            <UsersIcon />
            Assegna
          </button>
          {assignMenuOpen && (
            <div
              className="task-create-tool-menu"
              role="menu"
              aria-label="Assegna"
            >
              <p className="task-create-menu-label">Membri</p>
              {boardMembers.length === 0 ? (
                <p className="task-create-menu-hint">
                  Nessun membro disponibile.
                </p>
              ) : (
                boardMembers.map((member) => (
                  <button
                    type="button"
                    key={member.identityId}
                    role="menuitemradio"
                    aria-checked={assigneeIdentityId === member.identityId}
                    className={
                      assigneeIdentityId === member.identityId
                        ? 'task-create-questionnaire-option selected'
                        : 'task-create-questionnaire-option'
                    }
                    onClick={() => {
                      onAssigneeIdentityIdChange(member.identityId)
                      setAssignMenuOpen(false)
                    }}
                  >
                    <span className="task-create-assign-option">
                      <span
                        className={memberAvatarClassName(
                          member.identityId,
                          'board-avatar--glyph',
                        )}
                        aria-hidden="true"
                      >
                        {initialsFor(member.label)}
                      </span>
                      <span>{member.label}</span>
                    </span>
                  </button>
                ))
              )}
            </div>
          )}
        </div>
      ) : null}
      {selectedAssignee ? (
        <BoardCardAssignee users={[selectedAssignee]} />
      ) : null}
    </>
  )
}

const TaskDetailPanel = ({
  task,
  boardMembers,
  publishedQuestionnaireVersions,
  savedAttachments,
  attachmentLabels,
  onRefreshAttachments,
  onDownloadAttachment,
  onUpdate,
  onAssign,
  onComplete,
  onCopy,
  onClose,
}: {
  task: DecryptedTask
  boardMembers: BoardMember[]
  publishedQuestionnaireVersions: Array<{ id: Uuid; label: string }>
  savedAttachments: AttachmentCollectionItemDto[]
  attachmentLabels: Record<string, string>
  onRefreshAttachments(taskId: Uuid): Promise<void>
  onDownloadAttachment(attachment: AttachmentCollectionItemDto): Promise<void>
  onUpdate(task: DecryptedTask, input: TaskUpdateInput): Promise<void>
  onAssign(task: DecryptedTask, assigneeIdentityId: Uuid): Promise<void>
  onComplete(task: DecryptedTask): Promise<void>
  onCopy(task: DecryptedTask): Promise<void>
  onClose(): void
}) => {
  const panelRef = useRef<HTMLElement>(null)
  const commentInputRef = useRef<HTMLTextAreaElement>(null)
  const ignoreOverlayClickRef = useRef(false)
  const [title, setTitle] = useState(task.document.title)
  const [notes, setNotes] = useState(task.document.notes ?? '')
  const [taskKind, setTaskKind] = useState(task.wire.task_kind)
  const [taskPriority, setTaskPriority] = useState<'low' | 'normal' | 'high'>(
    task.document.priority ?? 'normal',
  )
  const [taskDueAt, setTaskDueAt] = useState(
    toDatetimeLocalValue(task.document.due_at),
  )
  const [recurrenceFrequency, setRecurrenceFrequency] = useState<RecurrenceFrequency>(
    task.document.recurrence?.frequency ?? 'daily',
  )
  const [recurrenceInterval, setRecurrenceInterval] = useState(
    String(task.document.recurrence?.interval ?? 1),
  )
  const [questionnaireVersionId, setQuestionnaireVersionId] = useState(
    task.wire.questionnaire_version_id ?? '',
  )
  const [attachmentFiles, setAttachmentFiles] = useState<File[]>([])
  const taskAttachments = useMemo(
    () =>
      savedAttachments.filter(
        (attachment) => attachment.task_id === task.wire.id,
      ),
    [savedAttachments, task.wire.id],
  )

  const savedNotes = task.document.notes ?? ''
  const savedPriority = task.document.priority ?? 'normal'
  const savedDueAt = toDatetimeLocalValue(task.document.due_at)
  const savedRecurrenceFrequency =
    task.document.recurrence?.frequency ?? 'daily'
  const savedRecurrenceInterval = String(task.document.recurrence?.interval ?? 1)
  const isDirty =
    title !== task.document.title ||
    notes !== savedNotes ||
    taskKind !== task.wire.task_kind ||
    (taskKind === 'priority' && taskPriority !== savedPriority) ||
    ((taskKind === 'deadline' || taskKind === 'recurring') &&
      taskDueAt !== savedDueAt) ||
    (taskKind === 'recurring' &&
      (recurrenceFrequency !== savedRecurrenceFrequency ||
        recurrenceInterval !== savedRecurrenceInterval)) ||
    attachmentFiles.length > 0
  const isOpen = task.wire.state.state === 'open'
  const dueAt = task.document.due_at
  const showDueDatePill = Boolean(dueAt) && task.wire.task_kind !== 'deadline'

  const guardOverlayClickAfterFileDialog = () => {
    ignoreOverlayClickRef.current = true
    window.setTimeout(() => {
      ignoreOverlayClickRef.current = false
    }, OVERLAY_FILE_DIALOG_GUARD_MS)
  }

  const addAttachmentFiles = (files: FileList | File[]) => {
    const next = Array.from(files)
    if (next.length === 0) return
    setAttachmentFiles((current) => [...current, ...next])
  }

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    const input: TaskUpdateInput = {
      title,
      notes: notes.trim() || undefined,
    }
    if (taskKind === 'priority') {
      input.priority = taskPriority
    }
    if (taskKind === 'deadline' || taskKind === 'recurring') {
      input.due_at = taskDueAt
        ? new Date(taskDueAt).toISOString()
        : undefined
    }
    if (taskKind === 'recurring') {
      input.recurrence = {
        frequency: recurrenceFrequency,
        interval: Number(recurrenceInterval),
      }
    }
    await onUpdate(task, input)
  }

  useLayoutEffect(() => {
    resizeTaskPanelComment(commentInputRef.current, panelRef.current)

    const panel = panelRef.current
    if (!panel) return

    const observer = new ResizeObserver(() => {
      resizeTaskPanelComment(commentInputRef.current, panel)
    })
    observer.observe(panel)
    return () => observer.disconnect()
  }, [notes, attachmentFiles.length, taskAttachments.length, task.wire.id])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  const onRefreshAttachmentsRef = useRef(onRefreshAttachments)
  onRefreshAttachmentsRef.current = onRefreshAttachments

  useEffect(() => {
    void onRefreshAttachmentsRef.current(task.wire.id)
  }, [task.wire.id])

  const panelStyle: CSSProperties = {
    maxHeight: 'calc(100vh - 3rem)',
  }
  const hasAnyAttachments =
    taskAttachments.length > 0 || attachmentFiles.length > 0

  return createPortal(
    <div
      className="task-create-overlay"
      onClick={() => {
        if (ignoreOverlayClickRef.current) return
        onClose()
      }}
      aria-hidden={false}
    >
      <div className="task-create-backdrop" aria-hidden="true" />
      <section
        ref={panelRef}
        className="task-create-panel"
        style={panelStyle}
        role="dialog"
        aria-modal="true"
        aria-label="Task detail"
        onClick={(event) => event.stopPropagation()}
      >
        <form className="task-create-form" onSubmit={(event) => void submit(event)}>
          <header className="task-create-header">
            <div className="task-detail-header-main">
              <div className="task-detail-header-title-row">
                <input
                  className="board-column-rename-input"
                  required
                  placeholder="Titolo del task"
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  aria-label="Titolo"
                />
                <time
                  className="task-detail-created-at"
                  dateTime={task.wire.created_at}
                >
                  {formatTaskCardDueDate(task.wire.created_at)}
                </time>
              </div>
            </div>
            <button
              type="button"
              className="task-create-close"
              aria-label="Close task detail"
              onClick={onClose}
            >
              ×
            </button>
          </header>

          <div className="task-create-body">
            <div className="task-create-comment-section">
              <div
                className="task-create-comment-wrap"
                onDragOver={(event) => {
                  event.preventDefault()
                  event.currentTarget.classList.add(
                    'task-create-comment-wrap--active',
                  )
                }}
                onDragLeave={(event) => {
                  event.currentTarget.classList.remove(
                    'task-create-comment-wrap--active',
                  )
                }}
                onDrop={(event) => {
                  event.preventDefault()
                  event.currentTarget.classList.remove(
                    'task-create-comment-wrap--active',
                  )
                  if (event.dataTransfer.files.length > 0) {
                    addAttachmentFiles(event.dataTransfer.files)
                  }
                }}
              >
                <textarea
                  ref={commentInputRef}
                  className="task-create-comment"
                  placeholder="Aggiungi note o dettagli"
                  value={notes}
                  onChange={(event) => {
                    setNotes(event.target.value)
                    resizeTaskPanelComment(event.target, panelRef.current)
                  }}
                  onInput={(event) =>
                    resizeTaskPanelComment(event.currentTarget, panelRef.current)
                  }
                  aria-label="Commento"
                  rows={5}
                />
              </div>
              {hasAnyAttachments && (
                <ul className="task-create-attachment-list">
                  {taskAttachments.map((attachment) => {
                    const label =
                      attachmentLabels[attachment.id] ?? 'Allegato'
                    const canDownload = attachment.state.state === 'available'
                    return (
                      <li key={attachment.id}>
                        <button
                          type="button"
                          className="task-create-attachment-name"
                          disabled={!canDownload}
                          onClick={() => {
                            if (!canDownload) return
                            void onDownloadAttachment(attachment)
                          }}
                        >
                          {label}
                        </button>
                      </li>
                    )
                  })}
                  {attachmentFiles.map((file, index) => (
                    <li key={`${file.name}-${file.size}-${file.lastModified}-${index}`}>
                      <button
                        type="button"
                        className="task-create-attachment-name"
                        onClick={() => openLocalAttachmentPreview(file)}
                      >
                        {file.name}
                      </button>
                      <button
                        type="button"
                        className="task-create-attachment-remove"
                        aria-label={`Rimuovi ${file.name}`}
                        onClick={() =>
                          setAttachmentFiles((current) =>
                            current.filter(
                              (_, itemIndex) => itemIndex !== index,
                            ),
                          )
                        }
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>

          <div className="task-create-actions">
            <div className="task-create-tools">
              <TaskPanelFooterTools
                publishedQuestionnaireVersions={publishedQuestionnaireVersions}
                questionnaireVersionId={questionnaireVersionId}
                onQuestionnaireVersionIdChange={setQuestionnaireVersionId}
                attachmentFiles={attachmentFiles}
                hasAttachments={hasAnyAttachments}
                onAddAttachmentFiles={addAttachmentFiles}
                onAttachmentPickerOpen={guardOverlayClickAfterFileDialog}
                boardMembers={boardMembers}
                assigneeIdentityId={
                  task.wire.active_assignee_identity_id ?? undefined
                }
                onAssigneeIdentityIdChange={(identityId) => {
                  void onAssign(task, identityId)
                }}
                taskKind={taskKind}
                onTaskKindChange={setTaskKind}
                taskPriority={taskPriority}
                onTaskPriorityChange={setTaskPriority}
                taskDueAt={taskDueAt}
                onTaskDueAtChange={setTaskDueAt}
                recurrenceFrequency={recurrenceFrequency}
                onRecurrenceFrequencyChange={setRecurrenceFrequency}
                recurrenceInterval={recurrenceInterval}
                onRecurrenceIntervalChange={setRecurrenceInterval}
                showAssignButton
                showDueDateCommittedPreview={Boolean(task.document.due_at)}
              />
              <span className="task-create-pill-btn">
                {TASK_STATE_LABELS[task.wire.state.state]}
              </span>
              {showDueDatePill && dueAt ? (
                <span className="task-create-pill-btn">
                  <CalendarIcon />
                  {formatDueDate(dueAt)}
                </span>
              ) : null}
            </div>
            {isDirty ? (
              <button type="submit" className="task-create-submit">
                Salva
              </button>
            ) : isOpen ? (
              <button
                type="button"
                className="task-create-submit"
                disabled={!task.wire.active_assignment_id}
                onClick={() => void onComplete(task)}
              >
                <CircleIcon />
                Completa
              </button>
            ) : (
              <button
                type="button"
                className="task-create-submit"
                onClick={() => void onCopy(task)}
              >
                Copia task
              </button>
            )}
          </div>
        </form>
      </section>
    </div>,
    document.body,
  )
}

const CreateTaskPanel = ({
  list,
  anchorRect: _anchorRect,
  boardMembers,
  publishedQuestionnaireVersions,
  onCreateTask,
  onCancel,
  initialTaskKind = 'priority',
  initialDueAt = '',
  initialAssigneeIdentityId,
}: {
  list: TaskListItem
  anchorRect: DOMRect
  boardMembers: BoardMember[]
  publishedQuestionnaireVersions: Array<{ id: Uuid; label: string }>
  onCreateTask(input: TaskCreationInput, listId: Uuid): Promise<void>
  onCancel(): void
  initialTaskKind?: 'priority' | 'deadline' | 'recurring'
  initialDueAt?: string
  initialAssigneeIdentityId?: Uuid
}) => {
  const panelRef = useRef<HTMLElement>(null)
  const titleInputRef = useRef<HTMLInputElement>(null)
  const commentInputRef = useRef<HTMLTextAreaElement>(null)
  const ignoreOverlayClickRef = useRef(false)
  const [taskTitle, setTaskTitle] = useState('')
  const [taskNotes, setTaskNotes] = useState('')
  const [taskDueAt, setTaskDueAt] = useState(initialDueAt)
  const [taskKind, setTaskKind] = useState<
    'priority' | 'deadline' | 'recurring'
  >(initialTaskKind)
  const [taskPriority, setTaskPriority] = useState<'low' | 'normal' | 'high'>(
    'normal',
  )
  const [recurrenceFrequency, setRecurrenceFrequency] =
    useState<RecurrenceFrequency>('daily')
  const [recurrenceInterval, setRecurrenceInterval] = useState('1')
  const [questionnaireVersionId, setQuestionnaireVersionId] = useState('')
  const [attachmentFiles, setAttachmentFiles] = useState<File[]>([])
  const [assigneeIdentityId, setAssigneeIdentityId] = useState<
    Uuid | undefined
  >(initialAssigneeIdentityId)

  const handleTaskKindChange = (
    kind: 'priority' | 'deadline' | 'recurring',
  ) => {
    setTaskKind(kind)
    if (kind !== 'deadline') {
      setTaskDueAt('')
    }
  }

  const resizeCommentInput = (textarea = commentInputRef.current) => {
    resizeTaskPanelComment(textarea, panelRef.current)
  }

  const guardOverlayClickAfterFileDialog = () => {
    ignoreOverlayClickRef.current = true
    window.setTimeout(() => {
      ignoreOverlayClickRef.current = false
    }, OVERLAY_FILE_DIALOG_GUARD_MS)
  }

  useEffect(() => {
    titleInputRef.current?.focus()
  }, [])

  useLayoutEffect(() => {
    resizeCommentInput()

    const panel = panelRef.current
    if (!panel) return

    const observer = new ResizeObserver(() => {
      resizeCommentInput()
    })
    observer.observe(panel)
    return () => observer.disconnect()
  }, [taskNotes, attachmentFiles.length])

  const addAttachmentFiles = (files: FileList | File[]) => {
    const next = Array.from(files)
    if (next.length === 0) return
    setAttachmentFiles((current) => [...current, ...next])
  }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onCancel()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onCancel])

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    const common = {
      title: taskTitle,
      notes: taskNotes.trim() || undefined,
      questionnaireVersionId: questionnaireVersionId || undefined,
      requiredAttachments:
        attachmentFiles.length > 0 ? attachmentFiles : undefined,
      assigneeIdentityId,
    }
    const dueAt =
      taskKind === 'recurring'
        ? new Date().toISOString()
        : taskDueAt
          ? new Date(taskDueAt).toISOString()
          : ''
    const input: TaskCreationInput =
      taskKind === 'priority'
        ? { ...common, taskKind, priority: taskPriority }
        : taskKind === 'deadline'
          ? { ...common, taskKind, dueAt }
          : {
              ...common,
              taskKind,
              dueAt,
              frequency: recurrenceFrequency,
              interval: Number(recurrenceInterval),
            }
    await onCreateTask(input, list.wire.id)
    onCancel()
  }

  const panelStyle: CSSProperties = {
    maxHeight: 'calc(100vh - 3rem)',
  }

  return createPortal(
    <div
      className="task-create-overlay"
      onClick={() => {
        if (ignoreOverlayClickRef.current) return
        onCancel()
      }}
      aria-hidden={false}
    >
      <div className="task-create-backdrop" aria-hidden="true" />
      <section
        ref={panelRef}
        className="task-create-panel"
        style={panelStyle}
        role="dialog"
        aria-modal="true"
        aria-label="Nuovo task"
        onClick={(event) => event.stopPropagation()}
      >
        <form className="task-create-form" onSubmit={(event) => void submit(event)}>
          <header className="task-create-header">
            <input
              ref={titleInputRef}
              className="board-column-rename-input"
              required
              placeholder="Titolo del task"
              value={taskTitle}
              onChange={(event) => setTaskTitle(event.target.value)}
              aria-label="Titolo"
            />
            <button
              type="button"
              className="task-create-close"
              aria-label="Chiudi"
              onClick={onCancel}
            >
              ×
            </button>
          </header>

          <div className="task-create-body">
            <div className="task-create-comment-section">
              <div
                className="task-create-comment-wrap"
                onDragOver={(event) => {
                  event.preventDefault()
                  event.currentTarget.classList.add(
                    'task-create-comment-wrap--active',
                  )
                }}
                onDragLeave={(event) => {
                  event.currentTarget.classList.remove(
                    'task-create-comment-wrap--active',
                  )
                }}
                onDrop={(event) => {
                  event.preventDefault()
                  event.currentTarget.classList.remove(
                    'task-create-comment-wrap--active',
                  )
                  if (event.dataTransfer.files.length > 0) {
                    addAttachmentFiles(event.dataTransfer.files)
                  }
                }}
              >
                <textarea
                  ref={commentInputRef}
                  className="task-create-comment"
                  placeholder="Aggiungi note o dettagli"
                  value={taskNotes}
                  onChange={(event) => {
                    setTaskNotes(event.target.value)
                    resizeCommentInput(event.target)
                  }}
                  onInput={(event) =>
                    resizeCommentInput(event.currentTarget)
                  }
                  aria-label="Commento"
                  rows={5}
                />
              </div>
              {attachmentFiles.length > 0 && (
                <ul className="task-create-attachment-list">
                  {attachmentFiles.map((file, index) => (
                    <li key={`${file.name}-${file.size}-${file.lastModified}-${index}`}>
                      <button
                        type="button"
                        className="task-create-attachment-name"
                        onClick={() => openLocalAttachmentPreview(file)}
                      >
                        {file.name}
                      </button>
                      <button
                        type="button"
                        className="task-create-attachment-remove"
                        aria-label={`Rimuovi ${file.name}`}
                        onClick={() =>
                          setAttachmentFiles((current) =>
                            current.filter(
                              (_, itemIndex) => itemIndex !== index,
                            ),
                          )
                        }
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
          <div className="task-create-actions">
            <div className="task-create-tools">
              <TaskPanelFooterTools
                publishedQuestionnaireVersions={publishedQuestionnaireVersions}
                questionnaireVersionId={questionnaireVersionId}
                onQuestionnaireVersionIdChange={setQuestionnaireVersionId}
                attachmentFiles={attachmentFiles}
                onAddAttachmentFiles={addAttachmentFiles}
                onAttachmentPickerOpen={guardOverlayClickAfterFileDialog}
                boardMembers={boardMembers}
                assigneeIdentityId={assigneeIdentityId}
                onAssigneeIdentityIdChange={setAssigneeIdentityId}
                taskKind={taskKind}
                onTaskKindChange={handleTaskKindChange}
                taskPriority={taskPriority}
                onTaskPriorityChange={setTaskPriority}
                taskDueAt={taskDueAt}
                onTaskDueAtChange={setTaskDueAt}
                recurrenceFrequency={recurrenceFrequency}
                onRecurrenceFrequencyChange={setRecurrenceFrequency}
                recurrenceInterval={recurrenceInterval}
                onRecurrenceIntervalChange={setRecurrenceInterval}
              />
            </div>
            <button type="submit" className="task-create-submit">
              Crea
            </button>
          </div>
        </form>
      </section>
    </div>,
    document.body,
  )
}

export const TasksScreen = ({
  project,
  topics,
  taskLists,
  tasks,
  lockedTasks,
  boardMembers,
  agents,
  boardFocus,
  boardViewMode,
  selectedTopicId,
  selectedTaskId,
  currentUserLabel: _currentUserLabel,
  publishedQuestionnaireVersions,
  filter,
  loading,
  onSelectFocus,
  onBoardViewModeChange,
  onSelectList,
  onSelectTask,
  onFilter: _onFilter,
  onCreateTopic,
  onRenameTopic,
  onToggleTopicFavorite,
  onDeleteTopic,
  onCreateList,
  onUpdateTaskList,
  onLoadProjectInfo,
  onCreateProjectInfoDocument,
  onLoadTopicInfo,
  onCreateTopicInfoDocument,
  onLoadTaskListInfo,
  onCreateTaskListInfoDocument,
  onUpdateInfoDocument,
  onUploadInfoDocumentFile,
  onReadInfoDocumentFile,
  onDownloadInfoDocumentFile,
  onCreateTask,
  onUpdateTask,
  onAssignTask,
  onCompleteTask,
  onCopyTask,
  onInviteMember,
  onProvisionAgent,
  taskAttachments,
  taskAttachmentLabels,
  onRefreshTaskAttachments,
  onDownloadTaskAttachment,
  userMenu,
}: TasksScreenProps) => {
  const [topicName, setTopicName] = useState('')
  const [showNewTopic, setShowNewTopic] = useState(false)
  const [listName, setListName] = useState('')
  const [listTopicId, setListTopicId] = useState('')
  const [showNewList, setShowNewList] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [agentActivityFilter, setAgentActivityFilter] =
    useState<AgentActivityFilter>('all')
  const [advancedTaskFilters, setAdvancedTaskFilters] =
    useState<AdvancedTaskFilters>(() => initialAdvancedTaskFilters(filter))
  const [taskFilterGroups, setTaskFilterGroups] =
    useState<TaskFilterGroup[]>(['dates'])

  const selectTaskFilterGroup = (group: TaskFilterGroup) => {
    setTaskFilterGroups((current) => {
      if (current.includes(group)) {
        const next = current.filter((item) => item !== group)
        return next.length > 0 ? next : ['dates']
      }
      if (current.length === 1 && current[0] === 'dates' && group !== 'dates') {
        return [group]
      }
      return [...current, group]
    })
  }
  const resetTaskFilters = () => {
    setAdvancedTaskFilters({
      listIds: [],
      types: [],
      memberIds: [],
      states: [],
      dates: [],
    })
    setTaskFilterGroups(['dates'])
  }
  const removeTaskFilter = (
    key: keyof AdvancedTaskFilters,
    value: string,
  ) => {
    setAdvancedTaskFilters((current) => ({
      ...current,
      [key]: (current[key] as string[]).filter((item) => item !== value),
    }) as AdvancedTaskFilters)
  }
  const [agentWorkspace, setAgentWorkspace] = useState<
    { name: string; avatar: string } | undefined
  >()
  const [agentDirectoryResetKey, setAgentDirectoryResetKey] = useState(0)
  const [listHistoryId, setListHistoryId] = useState<Uuid | undefined>()
  const [mobileSearchOpen, setMobileSearchOpen] = useState(false)
  const [mobileToolbarSearchOpen, setMobileToolbarSearchOpen] = useState(false)
  const [mobilePathPanelOpen, setMobilePathPanelOpen] = useState(false)
  const [aiBadgeOpen, setAiBadgeOpen] = useState(false)
  const [mobileSearchOverlayVisible, setMobileSearchOverlayVisible] =
    useState(false)
  const [recentSearches, setRecentSearches] = useState(readRecentSearches)
  const mobileSearchInputRef = useRef<HTMLInputElement>(null)
  const mobileToolbarSearchInputRef = useRef<HTMLInputElement>(null)
  const [creatingInListId, setCreatingInListId] = useState<Uuid | undefined>()
  const [createAnchorColumnKey, setCreateAnchorColumnKey] = useState<
    Uuid | undefined
  >()
  const [createAnchorRect, setCreateAnchorRect] = useState<DOMRect | null>(null)
  const createAnchorElRef = useRef<HTMLElement | null>(null)
  const [createInitialTaskKind, setCreateInitialTaskKind] = useState<
    'priority' | 'deadline' | 'recurring'
  >('priority')
  const [createInitialDueAt, setCreateInitialDueAt] = useState('')
  const columnRefs = useRef(new Map<Uuid, HTMLElement>())
  const [sidebarCollapsed, setSidebarCollapsed] = useState(readSidebarCollapsed)
  const [sidebarPreviewOpen, setSidebarPreviewOpen] = useState(false)
  const [sidebarWidth, setSidebarWidth] = useState(readSidebarWidth)
  const [sidebarResizing, setSidebarResizing] = useState(false)
  const sidebarResizeRef = useRef<{
    pointerId: number
    startX: number
    startWidth: number
    currentWidth: number
    scale: number
  } | null>(null)
  const [timelineWeekAnchor, setTimelineWeekAnchor] = useState(() =>
    startOfWeek(new Date()),
  )
  const [timelineScale, setTimelineScale] = useState(TIMELINE_SCALE_DEFAULT)
  const [topicOverview, setTopicOverview] = useState<
    TopicOverviewAnchor | undefined
  >()
  const [renamingTopicId, setRenamingTopicId] = useState<Uuid | undefined>()
  const [renameDraft, setRenameDraft] = useState('')
  const renameInputRef = useRef<HTMLInputElement>(null)
  const [editingListId, setEditingListId] = useState<Uuid | undefined>()
  const [listEditName, setListEditName] = useState('')
  const [listEditColor, setListEditColor] = useState<TaskListColumnColor>(
    COLUMN_AVATAR_COLORS[0],
  )
  const [listEditIcon, setListEditIcon] = useState<TaskListIcon | undefined>()
  const [listIconPickerOpen, setListIconPickerOpen] = useState(false)
  const [listIconPickerAnchorRect, setListIconPickerAnchorRect] =
    useState<DOMRect | null>(null)

  const sortedTopics = useMemo(
    () => sortTopicsForSidebar(topics),
    [topics],
  )

  const urgencyBadges = useMemo(
    () => topicUrgencyBadges(taskLists, tasks),
    [taskLists, tasks],
  )

  const mobileSortedTopics = useMemo(
    () => sortTopicsByUrgency(sortedTopics, urgencyBadges.byTopicId),
    [sortedTopics, urgencyBadges.byTopicId],
  )

  useEffect(() => {
    if (!renamingTopicId) return
    renameInputRef.current?.focus()
    renameInputRef.current?.select()
  }, [renamingTopicId])

  const startTopicRename = (topic: TopicItem) => {
    if (!topic.document) return
    setRenamingTopicId(topic.wire.id)
    setRenameDraft(topic.document.name)
  }

  const cancelTopicRename = () => {
    setRenamingTopicId(undefined)
    setRenameDraft('')
  }

  const commitTopicRename = (topic: TopicItem) => {
    const trimmed = renameDraft.trim()
    const previous = topic.document?.name ?? ''
    cancelTopicRename()
    if (!trimmed || trimmed === previous) return
    void onRenameTopic(topic, trimmed)
  }

  const startListEdit = (list: TaskListItem, listIndex: number) => {
    if (!list.document) return
    setEditingListId(list.wire.id)
    setListEditName(list.document.name)
    setListEditColor(resolveListColumnColor(list, listIndex))
    setListEditIcon(list.document.icon)
    setListIconPickerOpen(false)
    setListIconPickerAnchorRect(null)
  }

  const cancelListEdit = () => {
    setEditingListId(undefined)
    setListEditName('')
    setListEditIcon(undefined)
    setListIconPickerOpen(false)
    setListIconPickerAnchorRect(null)
  }

  const commitListEdit = (
    list: TaskListItem,
    listIndex: number,
    closeEditor = true,
  ) => {
    if (!list.document) {
      if (closeEditor) cancelListEdit()
      return
    }
    const trimmed = listEditName.trim()
    const previousName = list.document.name
    const previousColor = resolveListColumnColor(list, listIndex)
    const previousIcon = list.document.icon
    const nextColor = listEditColor
    const nextIcon = listEditIcon
    if (closeEditor) cancelListEdit()
    if (!trimmed) return
    if (
      trimmed === previousName &&
      nextColor === previousColor &&
      isSameTaskListIcon(nextIcon, previousIcon)
    ) {
      return
    }
    void onUpdateTaskList(list, {
      name: trimmed,
      color: nextColor,
      icon: nextIcon,
    })
  }

  const toggleSidebarCollapsed = () => {
    setMobilePathPanelOpen(false)
    setSidebarPreviewOpen(false)
    setSidebarCollapsed((value) => {
      const next = !value
      persistSidebarCollapsed(next)
      return next
    })
  }

  const openMobilePathPanel = () => {
    if (!window.matchMedia('(max-width: 850px)').matches) return false
    setMobilePathPanelOpen(true)
    return true
  }

  const startSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (sidebarCollapsed || event.button !== 0) return
    event.preventDefault()
    const rootZoom = Number(
      getComputedStyle(document.querySelector('#root') ?? document.body).zoom,
    )
    const remScale =
      Number.parseFloat(getComputedStyle(document.documentElement).fontSize) / 16
    const interfaceScale =
      (Number.isFinite(rootZoom) && rootZoom > 0 ? rootZoom : 1) *
      (Number.isFinite(remScale) && remScale > 0 ? remScale : 1)
    sidebarResizeRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: sidebarWidth,
      currentWidth: sidebarWidth,
      scale: interfaceScale,
    }
    event.currentTarget.setPointerCapture(event.pointerId)
    setSidebarResizing(true)
  }

  const resizeSidebar = (event: ReactPointerEvent<HTMLDivElement>) => {
    const resize = sidebarResizeRef.current
    if (!resize || resize.pointerId !== event.pointerId) return
    const nextWidth = Math.min(
      SIDEBAR_MAX_WIDTH,
      Math.max(
        SIDEBAR_MIN_WIDTH,
        resize.startWidth + (event.clientX - resize.startX) / resize.scale,
      ),
    )
    resize.currentWidth = nextWidth
    setSidebarWidth(nextWidth)
  }

  const finishSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const resize = sidebarResizeRef.current
    if (!resize || resize.pointerId !== event.pointerId) return
    persistSidebarWidth(resize.currentWidth)
    sidebarResizeRef.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    setSidebarResizing(false)
  }

  const resizeSidebarWithKeyboard = (
    event: ReactKeyboardEvent<HTMLDivElement>,
  ) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    event.preventDefault()
    setSidebarWidth((currentWidth) => {
      const direction = event.key === 'ArrowRight' ? 1 : -1
      const nextWidth = Math.min(
        SIDEBAR_MAX_WIDTH,
        Math.max(SIDEBAR_MIN_WIDTH, currentWidth + direction * 12),
      )
      persistSidebarWidth(nextWidth)
      return nextWidth
    })
  }

  const resetSidebarWidth = () => {
    setSidebarWidth(SIDEBAR_DEFAULT_WIDTH)
    persistSidebarWidth(SIDEBAR_DEFAULT_WIDTH)
  }

  const openNewTopic = () => {
    if (sidebarCollapsed) {
      setSidebarCollapsed(false)
      persistSidebarCollapsed(false)
    }
    setShowNewTopic(true)
  }

  useEffect(() => {
    if (showNewTopic && sidebarCollapsed) {
      setSidebarCollapsed(false)
      persistSidebarCollapsed(false)
    }
  }, [showNewTopic, sidebarCollapsed])

  useEffect(() => {
    if (!mobileSearchOpen) {
      setMobileSearchOverlayVisible(false)
      return
    }
    const frame = requestAnimationFrame(() => {
      setMobileSearchOverlayVisible(true)
    })
    return () => cancelAnimationFrame(frame)
  }, [mobileSearchOpen])

  useEffect(() => {
    if (!mobileSearchOpen || !mobileSearchOverlayVisible) return
    mobileSearchInputRef.current?.focus()
  }, [mobileSearchOpen, mobileSearchOverlayVisible])

  useEffect(() => {
    if (!mobileToolbarSearchOpen) return
    mobileToolbarSearchInputRef.current?.focus()
  }, [mobileToolbarSearchOpen])

  const rememberRecentSearch = useCallback((query: string) => {
    const trimmed = query.trim()
    if (!trimmed) return
    setRecentSearches((previous) => {
      const next = [
        trimmed,
        ...previous.filter((item) => item !== trimmed),
      ].slice(0, MAX_RECENT_SEARCHES)
      persistRecentSearches(next)
      return next
    })
  }, [])

  const closeMobileSearch = useCallback(
    (rememberQuery = true) => {
      if (rememberQuery) rememberRecentSearch(searchQuery)
      setMobileSearchOpen(false)
      setSearchQuery('')
    },
    [rememberRecentSearch, searchQuery],
  )

  useEffect(() => {
    if (!mobileSearchOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        closeMobileSearch(false)
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [closeMobileSearch, mobileSearchOpen])

  const isMemberBoard = isMemberBoardFocus(boardFocus)
  const isAgentBoard = isAgentBoardFocus(boardFocus)
  const boardFocusScopeId =
    boardFocus.type === 'topic'
      ? boardFocus.topicId
      : boardFocus.type === 'member'
        ? boardFocus.identityId
        : boardFocus.type === 'agent'
          ? boardFocus.agentId
          : 'all'
  const boardViewTabScopeKey = `${project?.wire.id ?? 'no-project'}:${boardFocus.type}:${boardFocusScopeId}`

  useEffect(() => {
    if (
      isAgentBoard &&
      boardViewMode !== 'overview' &&
      boardViewMode !== 'board'
    ) {
      onBoardViewModeChange('overview')
    }
  }, [boardViewMode, isAgentBoard, onBoardViewModeChange])

  const focusLists = useMemo(() => {
    if (boardFocus.type === 'topic') {
      return taskListsForTopic(taskLists, boardFocus.topicId)
    }
    if (isMemberBoard) {
      return []
    }
    return taskLists
  }, [boardFocus, isAgentBoard, isMemberBoard, taskLists])

  const focusMemberColumns = useMemo(() => {
    if (boardFocus.type === 'member') {
      return boardMembers.filter(
        (member) => member.identityId === boardFocus.identityId,
      )
    }
    if (boardFocus.type === 'members') {
      return boardMembers
    }
    return []
  }, [boardFocus, boardMembers])

  const focusTasks = useMemo(() => {
    if (boardFocus.type === 'member') {
      return tasks.filter(
        (task) =>
          task.wire.active_assignee_identity_id === boardFocus.identityId,
      )
    }
    if (boardFocus.type === 'members') {
      const memberIds = new Set(
        boardMembers.map((member) => member.identityId),
      )
      return tasks.filter((task) => {
        const assignee = task.wire.active_assignee_identity_id
        return assignee != null && memberIds.has(assignee)
      })
    }
    if (boardFocus.type === 'topic') {
      const listIds = new Set(focusLists.map((list) => list.wire.id))
      return tasks.filter((task) => listIds.has(task.wire.list_id))
    }
    return tasks
  }, [boardFocus, boardMembers, focusLists, tasks])

  const boardSearchQuery = mobileSearchOpen ? '' : searchQuery

  const searched = useMemo(
    () => filterBoardSearch(focusLists, focusTasks, boardSearchQuery),
    [focusLists, focusTasks, boardSearchQuery],
  )

  const mobileSearchData = useMemo(
    () => filterBoardSearch(focusLists, focusTasks, searchQuery),
    [focusLists, focusTasks, searchQuery],
  )

  const searchedMemberColumns = useMemo(() => {
    if (!isMemberBoard) return []
    const normalized = searchQuery.trim().toLowerCase()
    if (!normalized) return focusMemberColumns

    const matchingTaskAssignees = new Set(
      searched.tasks
        .map((task) => task.wire.active_assignee_identity_id)
        .filter((id): id is Uuid => id != null),
    )

    return focusMemberColumns.filter((member) => {
      if (member.label.toLowerCase().includes(normalized)) return true
      return matchingTaskAssignees.has(member.identityId)
    })
  }, [
    focusMemberColumns,
    isMemberBoard,
    searchQuery,
    searched.tasks,
  ])

  const creatingList = useMemo(() => {
    if (!creatingInListId) return undefined
    return (
      searched.lists.find((item) => item.wire.id === creatingInListId) ??
      taskLists.find((item) => item.wire.id === creatingInListId)
    )
  }, [creatingInListId, searched.lists, taskLists])

  const createInitialAssigneeId = useMemo(() => {
    if (!createAnchorColumnKey) return undefined
    const isMember = boardMembers.some(
      (member) => member.identityId === createAnchorColumnKey,
    )
    const isAgent = agents.some(
      (agent) => agent.principal_identity_id === createAnchorColumnKey,
    )
    return isMember || isAgent ? createAnchorColumnKey : undefined
  }, [agents, boardMembers, createAnchorColumnKey])

  const editingList = useMemo(
    () =>
      editingListId
        ? searched.lists.find((item) => item.wire.id === editingListId)
        : undefined,
    [editingListId, searched.lists],
  )

  const updateIconPickerAnchorRect = (listId: Uuid | undefined) => {
    if (!listId) {
      setListIconPickerAnchorRect(null)
      return
    }
    const historyTrigger = document.querySelector<HTMLElement>(
      '.tasklist-history-panel .board-column-icon-trigger',
    )
    if (historyTrigger) {
      setListIconPickerAnchorRect(historyTrigger.getBoundingClientRect())
      return
    }
    const column = columnRefs.current.get(listId)
    if (!column) {
      setListIconPickerAnchorRect(null)
      return
    }
    const header =
      column.querySelector<HTMLElement>('.board-column-header') ?? column
    setListIconPickerAnchorRect(header.getBoundingClientRect())
  }

  useLayoutEffect(() => {
    if (!listIconPickerOpen || !editingListId) {
      setListIconPickerAnchorRect(null)
      return
    }
    updateIconPickerAnchorRect(editingListId)
    const onLayoutChange = () => updateIconPickerAnchorRect(editingListId)
    window.addEventListener('resize', onLayoutChange)
    window.addEventListener('scroll', onLayoutChange, true)
    return () => {
      window.removeEventListener('resize', onLayoutChange)
      window.removeEventListener('scroll', onLayoutChange, true)
    }
  }, [listIconPickerOpen, editingListId, searched.lists])

  const updateCreateAnchorRect = (columnKey: Uuid | undefined) => {
    if (!columnKey) {
      setCreateAnchorRect(null)
      return
    }
    const column = columnRefs.current.get(columnKey)
    setCreateAnchorRect(column ? column.getBoundingClientRect() : null)
  }

  useLayoutEffect(() => {
    const update = () => {
      if (createAnchorElRef.current) {
        setCreateAnchorRect(createAnchorElRef.current.getBoundingClientRect())
        return
      }
      updateCreateAnchorRect(createAnchorColumnKey)
    }
    update()
    if (!createAnchorElRef.current && !createAnchorColumnKey) return

    const onLayoutChange = () => update()
    window.addEventListener('resize', onLayoutChange)
    window.addEventListener('scroll', onLayoutChange, true)
    return () => {
      window.removeEventListener('resize', onLayoutChange)
      window.removeEventListener('scroll', onLayoutChange, true)
    }
  }, [createAnchorColumnKey, searched.lists, searchedMemberColumns, creatingInListId])

  const visibleSidebarMembers = boardMembers.slice(0, SIDEBAR_MEMBER_VISIBLE_MAX)
  const overflowSidebarMembers = boardMembers.slice(SIDEBAR_MEMBER_VISIBLE_MAX)

  const closeCreateTask = () => {
    setCreatingInListId(undefined)
    setCreateAnchorColumnKey(undefined)
    setCreateAnchorRect(null)
    createAnchorElRef.current = null
    setCreateInitialTaskKind('priority')
    setCreateInitialDueAt('')
  }

  const openCreateTaskInList = (listId: Uuid, columnKey: Uuid) => {
    onSelectList(listId)
    setCreatingInListId(listId)
    createAnchorElRef.current = null
    setCreateInitialTaskKind('priority')
    setCreateInitialDueAt('')
    setCreateAnchorColumnKey(columnKey)
  }

  const openCreateTaskInTimelineDay = (
    listId: Uuid,
    day: Date,
    anchorEl: HTMLElement,
  ) => {
    onSelectList(listId)
    setCreatingInListId(listId)
    createAnchorElRef.current = anchorEl
    setCreateInitialTaskKind('deadline')
    setCreateInitialDueAt(defaultTimelineDueDatetimeLocal(day, timelineScale))
    setCreateAnchorColumnKey(undefined)
    setCreateAnchorRect(anchorEl.getBoundingClientRect())
  }

  const visibleTasks = useMemo(
    () => applyAdvancedTaskFilters(searched.tasks, advancedTaskFilters),
    [advancedTaskFilters, searched.tasks],
  )

  const mobileSearchTasks = useMemo(
    () => applyAdvancedTaskFilters(mobileSearchData.tasks, advancedTaskFilters),
    [advancedTaskFilters, mobileSearchData.tasks],
  )

  const mobileSearchLists = useMemo(
    () => sortTaskListsByUrgency(mobileSearchData.lists, mobileSearchTasks),
    [mobileSearchData.lists, mobileSearchTasks],
  )

  const mobileSearchMemberColumns = useMemo(() => {
    if (!isMemberBoard) return []
    const normalized = searchQuery.trim().toLowerCase()
    if (!normalized) return focusMemberColumns

    const matchingTaskAssignees = new Set(
      mobileSearchData.tasks
        .map((task) => task.wire.active_assignee_identity_id)
        .filter((id): id is Uuid => id != null),
    )

    return focusMemberColumns.filter((member) => {
      if (member.label.toLowerCase().includes(normalized)) return true
      return matchingTaskAssignees.has(member.identityId)
    })
  }, [
    focusMemberColumns,
    isMemberBoard,
    mobileSearchData.tasks,
    searchQuery,
  ])

  const orderedLists = useMemo(
    () => sortTaskListsByUrgency(searched.lists, visibleTasks),
    [searched.lists, visibleTasks],
  )

  const orderedMemberColumns = useMemo(() => {
    if (!isMemberBoard) return searchedMemberColumns
    const tasksByAssignee = new Map<Uuid, DecryptedTask[]>()
    for (const task of visibleTasks) {
      const assignee = task.wire.active_assignee_identity_id
      if (!assignee) continue
      const bucket = tasksByAssignee.get(assignee)
      if (bucket) {
        bucket.push(task)
      } else {
        tasksByAssignee.set(assignee, [task])
      }
    }
    return sortItemsByTaskUrgency(
      searchedMemberColumns,
      (member) => tasksByAssignee.get(member.identityId) ?? [],
      (member) => member.label,
    )
  }, [isMemberBoard, searchedMemberColumns, visibleTasks])

  const orderedAgentColumns = useMemo(() => {
    if (!isAgentBoard) return []
    const query = searchQuery.trim().toLocaleLowerCase()
    return agents.filter((agent) => {
      if (!query) return true
      if (agent.identity_handle.toLocaleLowerCase().includes(query)) return true
      return visibleTasks.some(
        (task) =>
          task.wire.active_assignee_identity_id === agent.principal_identity_id,
      )
    })
  }, [agents, isAgentBoard, searchQuery, visibleTasks])

  const timelineTasks = useMemo(
    () => filterTimelineTasks(visibleTasks),
    [visibleTasks],
  )

  const timelineLists = useMemo(() => {
    if (isMemberBoard) {
      const listIds = new Set(timelineTasks.map((task) => task.wire.list_id))
      return sortTaskListsByUrgency(
        taskLists.filter((list) => listIds.has(list.wire.id)),
        timelineTasks,
      )
    }
    return sortTaskListsByUrgency(searched.lists, timelineTasks)
  }, [isMemberBoard, searched.lists, taskLists, timelineTasks])

  const isOverviewView = boardViewMode === 'overview'
  const isTimelineView = boardViewMode === 'timeline'
  const isHistoryView = boardViewMode === 'history'
  const focusedTopic = useMemo(
    () =>
      boardFocus.type === 'topic'
        ? topics.find((topic) => topic.wire.id === boardFocus.topicId)
        : undefined,
    [boardFocus, topics],
  )

  const boardScopeName = focusedTopic
    ? (focusedTopic.document?.name ?? 'Categoria protetta')
    : ((boardFocus.type === 'member'
        ? boardMembers.find(
            (member) => member.identityId === boardFocus.identityId,
          )?.label
        : undefined) ??
      (boardFocus.type === 'members' ? 'Membri' : 'Generali'))
  const boardFooterPath = isAgentBoard ? 'Agenti' : boardScopeName

  const boardMemberById = useMemo(() => {
    const map = new Map<Uuid, BoardMember>()
    for (const member of boardMembers) {
      map.set(member.identityId, member)
    }
    return map
  }, [boardMembers])

  const selectedTask = tasks.find((task) => task.wire.id === selectedTaskId)

  const newListTopicId =
    boardFocus.type === 'topic'
      ? boardFocus.topicId
      : listTopicId || selectedTopicId || ''
  const newListTopicIndex = topics.findIndex(
    (topic) => topic.wire.id === newListTopicId,
  )
  const newListTopic =
    newListTopicIndex >= 0 ? topics[newListTopicIndex] : undefined
  const newListTopicLabel = newListTopic
    ? (newListTopic.document?.name ?? newListTopic.wire.id.slice(0, 8))
    : 'Categoria'

  const closeTaskDetail = () => onSelectTask(undefined)

  const historyList = useMemo(
    () =>
      listHistoryId
        ? taskLists.find((list) => list.wire.id === listHistoryId)
        : undefined,
    [listHistoryId, taskLists],
  )

  const openListHistory = (listId: Uuid) => {
    onSelectTask(undefined)
    setListHistoryId(listId)
  }

  const closeListHistory = () => {
    setListHistoryId(undefined)
  }

  const handleIslandViewModeChange = (mode: BoardViewMode) => {
    closeListHistory()
    onBoardViewModeChange(mode)
  }

  const handleIslandMembers = () => {
    closeListHistory()
    onSelectFocus({ type: 'members' })
  }

  const handleIslandAgents = () => {
    closeListHistory()
    onSelectFocus({ type: 'agents' })
  }

  const createTopic = async (event: FormEvent) => {
    event.preventDefault()
    await onCreateTopic(topicName)
    setTopicName('')
    setShowNewTopic(false)
  }

  const createList = async (event: FormEvent) => {
    event.preventDefault()
    const topicId =
      boardFocus.type === 'topic'
        ? boardFocus.topicId
        : (listTopicId as Uuid)
    if (!topicId) return
    await onCreateList(listName, topicId)
    setListName('')
    setListTopicId('')
    setShowNewList(false)
  }

  if (!project) {
    const hasSelectedProject = Boolean(userMenu.selectedProjectId)
    return (
      <section className="screen-empty">
        <h2>
          {hasSelectedProject
            ? 'Progetto temporaneamente non disponibile'
            : 'No project selected'}
        </h2>
        <p>
          {hasSelectedProject
            ? 'Il progetto selezionato non è stato caricato. Attendi qualche secondo e ricarica la pagina se il problema persiste.'
            : 'Create or select an encrypted project to load its resources.'}
        </p>
        {!hasSelectedProject && (
          <BoardProjectSwitcher
            projects={userMenu.projects}
            selectedProjectId={userMenu.selectedProjectId}
            projectName={userMenu.projectName}
            onProjectNameChange={userMenu.onProjectNameChange}
            onSelectProject={userMenu.onSelectProject}
            onCreateProject={userMenu.onCreateProject}
          />
        )}
      </section>
    )
  }

  const boardLayoutClass = [
    'board-layout',
    sidebarCollapsed && !sidebarPreviewOpen
      ? 'board-layout--sidebar-collapsed'
      : '',
    sidebarResizing ? 'board-layout--sidebar-resizing' : '',
    isAgentBoard ? 'board-layout--agents' : '',
    historyList ? 'board-layout--list-history' : '',
    mobileSearchOpen ? 'board-layout--mobile-search' : '',
    mobileToolbarSearchOpen ? 'board-layout--toolbar-search' : '',
  ]
    .filter(Boolean)
    .join(' ')

  const boardLayoutStyle = {
    '--board-sidebar-width': `${sidebarWidth / 16}rem`,
  } as CSSProperties

  return (
    <div className={boardLayoutClass} style={boardLayoutStyle}>
      <header className="board-mobile-top">
        <div className="board-mobile-top-row">
          <nav className="board-mobile-stories" aria-label="Categorie">
            <button
              type="button"
              className={
                boardFocus.type === 'generali'
                  ? 'board-mobile-story active'
                  : 'board-mobile-story'
              }
              onClick={() => onSelectFocus({ type: 'generali' })}
            >
              <span
                className="board-avatar generali board-avatar--glyph board-mobile-story-ring"
                aria-hidden
              >
                <SidebarHomeIcon />
              </span>
              <span className="board-mobile-story-label">Generali</span>
            </button>
            {mobileSortedTopics.map((topic, topicIndex) => {
              const isActive =
                boardFocus.type === 'topic' &&
                boardFocus.topicId === topic.wire.id
              const label = topic.document?.name ?? 'Locked'
              return (
                <button
                  key={topic.wire.id}
                  type="button"
                  className={
                    isActive
                      ? 'board-mobile-story active'
                      : 'board-mobile-story'
                  }
                  onClick={() =>
                    onSelectFocus({
                      type: 'topic',
                      topicId: topic.wire.id,
                    })
                  }
                  onContextMenu={(event) => {
                    event.preventDefault()
                    if (!topic.document) return
                    setTopicOverview({
                      topic,
                      x: event.clientX,
                      y: event.clientY,
                    })
                  }}
                >
                  {topic.document ? (
                    <BoardMobileStoryRing
                      badge={urgencyBadges.byTopicId.get(topic.wire.id)}
                      className={`board-avatar ${topicAvatarClass(topicIndex)}`}
                    >
                      {initialFor(topic.document.name)}
                    </BoardMobileStoryRing>
                  ) : (
                    <span
                      className="board-avatar locked board-mobile-story-ring"
                      aria-hidden
                    >
                      <LockIcon />
                    </span>
                  )}
                  <span className="board-mobile-story-label">{label}</span>
                </button>
              )
            })}
            <button
              type="button"
              className="board-mobile-story board-mobile-story--add"
              onClick={openNewTopic}
              aria-label="Nuova categoria"
            >
              <span className="board-mobile-story-ring board-mobile-story-add-ring" aria-hidden>
                <PlusIcon />
              </span>
              <span className="board-mobile-story-label">Nuova</span>
            </button>
          </nav>
          <button
            type="button"
            className="board-mobile-search-fab"
            aria-label="Cerca"
            aria-expanded={mobileSearchOpen}
            onClick={() => setMobileSearchOpen(true)}
          >
            <SearchIcon aria-hidden />
          </button>
        </div>
        {mobileSearchOpen && (
          <div
            className={[
              'board-mobile-search-overlay',
              mobileSearchOverlayVisible
                ? 'board-mobile-search-overlay--visible'
                : '',
            ]
              .filter(Boolean)
              .join(' ')}
            role="search"
          >
            <BoardFilterDropdown
              filters={advancedTaskFilters}
              groupBy={taskFilterGroups}
              taskLists={taskLists}
              members={boardMembers}
              onChange={setAdvancedTaskFilters}
              onGroupBy={selectTaskFilterGroup}
              onReset={resetTaskFilters}
            />
            <label className="board-search board-mobile-search-field">
              <SearchIcon />
              <input
                ref={mobileSearchInputRef}
                type="search"
                placeholder="Cerca task e tasklist"
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') rememberRecentSearch(searchQuery)
                }}
                aria-label="Cerca task e tasklist"
              />
            </label>
            <button
              type="button"
              className="board-mobile-search-close"
              aria-label="Chiudi ricerca"
              onClick={() => closeMobileSearch()}
            >
              <XIcon aria-hidden />
            </button>
          </div>
        )}
        {showNewTopic && (
          <form
            className="board-mobile-new-topic"
            onSubmit={(event) => void createTopic(event)}
          >
            <input
              required
              placeholder="Nome categoria"
              value={topicName}
              onChange={(event) => setTopicName(event.target.value)}
              aria-label="Topic name"
            />
            <div className="board-create-actions">
              <button type="submit" className="primary-button">
                Crea
              </button>
              <button
                type="button"
                className="text-button"
                onClick={() => {
                  setShowNewTopic(false)
                  setTopicName('')
                }}
              >
                Annulla
              </button>
            </div>
          </form>
        )}
      </header>

      <header className="board-toolbar">
        <div className="board-toolbar-sidebar">
          <div className="board-sidebar-top-row">
            <button
              type="button"
              className={
                sidebarCollapsed && !sidebarPreviewOpen
                  ? 'board-sidebar-toggle board-sidebar-toggle--in-sidebar'
                  : 'board-sidebar-toggle board-sidebar-toggle--in-sidebar is-expanded'
              }
              onClick={toggleSidebarCollapsed}
              aria-label={
                sidebarCollapsed && !sidebarPreviewOpen
                  ? 'Espandi sidebar'
                  : 'Riduci sidebar'
              }
            >
              <ChevronIcon aria-hidden />
            </button>
            <BoardProjectSwitcher
              projects={userMenu.projects}
              selectedProjectId={userMenu.selectedProjectId}
              currentProject={project}
              projectName={userMenu.projectName}
              onProjectNameChange={userMenu.onProjectNameChange}
              onSelectProject={userMenu.onSelectProject}
              onCreateProject={userMenu.onCreateProject}
            />
          </div>
        </div>
        <div className="board-toolbar-main">
          <div className="board-toolbar-start">
            {historyList ? (
              <TaskListDetailViewNavigation onBack={closeListHistory} />
            ) : isAgentBoard && agentWorkspace ? (
              <AgentViewNavigation
                workspace={agentWorkspace}
                onBack={() => {
                  setAgentWorkspace(undefined)
                  setAgentDirectoryResetKey((value) => value + 1)
                  onSelectFocus({ type: 'agents' })
                }}
              />
            ) : (
              <BoardViewNavigation
                mode={boardViewMode}
                onChange={onBoardViewModeChange}
                scopeKey={boardViewTabScopeKey}
                compact={isAgentBoard}
              />
            )}
          </div>
          <div className="board-toolbar-end">
            {isAgentBoard && isOverviewView ? (
              <AgentFilterDropdown
                filter={agentActivityFilter}
                onFilter={setAgentActivityFilter}
              />
            ) : (
              <BoardFilterDropdown
                filters={advancedTaskFilters}
                groupBy={taskFilterGroups}
                taskLists={taskLists}
                members={boardMembers}
                onChange={setAdvancedTaskFilters}
                onGroupBy={selectTaskFilterGroup}
                onReset={resetTaskFilters}
              />
            )}
            <label
              className={`board-search${mobileToolbarSearchOpen ? ' is-expanded' : ''}`}
              onClick={() => {
                if (window.matchMedia('(max-width: 850px)').matches) {
                  setMobileToolbarSearchOpen(true)
                }
              }}
            >
              <SearchIcon />
              <input
                ref={mobileToolbarSearchInputRef}
                type="search"
                placeholder={
                  isAgentBoard && isOverviewView
                    ? 'Cerca agenti'
                    : 'Cerca task e tasklist'
                }
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
                aria-label={
                  isAgentBoard && isOverviewView
                    ? 'Cerca agenti'
                    : 'Cerca task e tasklist'
                }
              />
            </label>
          </div>
        </div>
      </header>

      {sidebarCollapsed && !sidebarPreviewOpen && (
        <div
          className="board-sidebar-edge-trigger"
          aria-hidden
          onPointerEnter={() => setSidebarPreviewOpen(true)}
        />
      )}

      <aside
        className={
          sidebarCollapsed && !sidebarPreviewOpen
            ? 'board-sidebar board-sidebar--collapsed'
            : 'board-sidebar'
        }
        aria-label="Board navigation"
        aria-expanded={!sidebarCollapsed || sidebarPreviewOpen}
        onPointerLeave={() => {
          if (sidebarPreviewOpen) setSidebarPreviewOpen(false)
        }}
      >
        {!sidebarCollapsed && (
          <div
            className="board-sidebar-resize-handle"
            role="separator"
            aria-label="Ridimensiona sidebar"
            aria-orientation="vertical"
            aria-valuemin={SIDEBAR_MIN_WIDTH}
            aria-valuemax={SIDEBAR_MAX_WIDTH}
            aria-valuenow={Math.round(sidebarWidth)}
            tabIndex={0}
            onPointerDown={startSidebarResize}
            onPointerMove={resizeSidebar}
            onPointerUp={finishSidebarResize}
            onPointerCancel={finishSidebarResize}
            onLostPointerCapture={finishSidebarResize}
            onKeyDown={resizeSidebarWithKeyboard}
            onDoubleClick={resetSidebarWidth}
          />
        )}
        <nav className="board-nav">
          <div className="board-sidebar-primary-nav">
            <div className="board-nav-section board-nav-section--views">
              <ul className="board-nav-list">
                <li>
                  <button
                    type="button"
                    className={
                      boardFocus.type === 'members' || boardFocus.type === 'member'
                        ? 'board-nav-item board-nav-item--view active'
                        : 'board-nav-item board-nav-item--view'
                    }
                    aria-label="Membri"
                    onClick={() => onSelectFocus({ type: 'members' })}
                  >
                    <SidebarUserIcon className="board-nav-view-icon" aria-hidden />
                    <span className="board-nav-label">Membri</span>
                  </button>
                  {visibleSidebarMembers.map((member) => (
                    <button
                      key={`member-shortcut-${member.identityId}`}
                      type="button"
                      className="visually-hidden"
                      aria-label={member.label}
                      onClick={() =>
                        onSelectFocus({
                          type: 'member',
                          identityId: member.identityId,
                        })
                      }
                    />
                  ))}
                  {overflowSidebarMembers.length > 0 && (
                    <button
                      type="button"
                      className="visually-hidden"
                      aria-label={`Altri ${overflowSidebarMembers.length} membri`}
                      onClick={() => onSelectFocus({ type: 'members' })}
                    />
                  )}
                </li>
                <li>
                  <button
                    type="button"
                    className={
                      isAgentBoard
                        ? 'board-nav-item board-nav-item--view active'
                        : 'board-nav-item board-nav-item--view'
                    }
                    onClick={() => onSelectFocus({ type: 'agents' })}
                    aria-current={isAgentBoard ? 'page' : undefined}
                  >
                    <SidebarAgentIcon
                      className="board-nav-view-icon board-nav-view-icon--agent"
                      aria-hidden
                    />
                    <span className="board-nav-label">Agenti</span>
                  </button>
                </li>
              </ul>
            </div>
          </div>

          <div className="board-nav-section board-nav-section--space">
            <div className="board-space-heading">
              <p className="board-nav-heading">Spazio</p>
              {!sidebarCollapsed && !showNewTopic && (
                <button
                  type="button"
                  className="board-space-add"
                  onClick={openNewTopic}
                  aria-label="Nuova categoria"
                >
                  <PlusIcon aria-hidden />
                </button>
              )}
            </div>
            <ul className="board-nav-list">
              <li>
                <button
                  type="button"
                  className={
                    boardFocus.type === 'generali'
                      ? 'board-nav-item active'
                      : 'board-nav-item'
                  }
                  aria-current={
                    boardFocus.type === 'generali' ? 'page' : undefined
                  }
                  onClick={() => onSelectFocus({ type: 'generali' })}
                >
                  <SidebarHomeIcon
                    className="board-nav-category-file-icon"
                    aria-hidden
                  />
                  <span className="board-nav-label">Generali</span>
                  <BoardNavUrgencyBadge badge={urgencyBadges.generali} />
                </button>
              </li>
            </ul>
            <ul className="board-nav-list board-nav-list-nested board-nav-list--categories">
              {sortedTopics.map((topic) => {
                const isActive =
                  boardFocus.type === 'topic' &&
                  boardFocus.topicId === topic.wire.id
                const isRenaming = renamingTopicId === topic.wire.id
                const rowClass = isActive
                  ? 'board-nav-item active'
                  : 'board-nav-item'
                const topicBadge = urgencyBadges.byTopicId.get(topic.wire.id)

                if (isRenaming && topic.document) {
                  return (
                    <li key={topic.wire.id}>
                      <div
                        className={`${rowClass} board-nav-item--editing`}
                        onContextMenu={(event) => event.preventDefault()}
                      >
                        <SidebarHomeIcon
                          className="board-nav-category-file-icon"
                          aria-hidden
                        />
                        <input
                          ref={renameInputRef}
                          className="board-nav-rename-input"
                          value={renameDraft}
                          aria-label="Rinomina categoria"
                          onChange={(event) =>
                            setRenameDraft(event.target.value)
                          }
                          onBlur={() => commitTopicRename(topic)}
                          onKeyDown={(event) => {
                            if (event.key === 'Enter') {
                              event.preventDefault()
                              commitTopicRename(topic)
                            }
                            if (event.key === 'Escape') {
                              event.preventDefault()
                              cancelTopicRename()
                            }
                          }}
                        />
                        <BoardNavUrgencyBadge badge={topicBadge} />
                        {topic.document.favorite && (
                          <StarIcon
                            className="board-nav-category-favorite"
                            aria-label="Preferita"
                          />
                        )}
                      </div>
                    </li>
                  )
                }

                return (
                  <li key={topic.wire.id}>
                    <button
                      type="button"
                      className={rowClass}
                      aria-current={isActive ? 'page' : undefined}
                      onClick={() =>
                        onSelectFocus({
                          type: 'topic',
                          topicId: topic.wire.id,
                        })
                      }
                      onContextMenu={(event) => {
                        event.preventDefault()
                        setTopicOverview({
                          topic,
                          x: event.clientX,
                          y: event.clientY,
                        })
                      }}
                    >
                      {topic.document ? (
                        <>
                          <SidebarHomeIcon
                            className="board-nav-category-file-icon"
                            aria-hidden
                          />
                          <span className="board-nav-label">
                            {topic.document.name}
                          </span>
                          <BoardNavUrgencyBadge badge={topicBadge} />
                          {topic.document.favorite && (
                            <StarIcon
                              className="board-nav-category-favorite"
                              aria-label="Preferita"
                            />
                          )}
                        </>
                      ) : (
                        <>
                          <LockIcon
                            className="board-nav-category-file-icon"
                            aria-hidden
                          />
                          <span className="board-nav-label">Locked topic</span>
                        </>
                      )}
                    </button>
                  </li>
                )
              })}
              {!sidebarCollapsed && showNewTopic && (
                <li className="board-new-topic-inline-item">
                  <form
                    className="board-new-topic-inline"
                    onSubmit={(event) => void createTopic(event)}
                  >
                    <SidebarHomeIcon
                      className="board-nav-category-file-icon"
                      aria-hidden
                    />
                    <input
                      required
                      autoFocus
                      className="board-new-topic-inline-input"
                      placeholder="Nome categoria"
                      value={topicName}
                      onChange={(event) => setTopicName(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key !== 'Escape') return
                        event.preventDefault()
                        setShowNewTopic(false)
                        setTopicName('')
                      }}
                      aria-label="Topic name"
                    />
                    <button
                      type="submit"
                      className="board-new-topic-confirm"
                      aria-label="Conferma categoria"
                      title="Conferma categoria"
                      disabled={!topicName.trim()}
                    >
                      <CheckIcon aria-hidden />
                    </button>
                  </form>
                </li>
              )}
            </ul>
            {topics.length === 0 && !loading && !showNewTopic && (
              <p className="inline-empty board-nav-empty--categories">
                Nessuna categoria.
              </p>
            )}
          </div>
        </nav>

        {topicOverview && (
          <TopicOverviewMenu
            anchor={topicOverview}
            onClose={() => setTopicOverview(undefined)}
            onStartRename={() => startTopicRename(topicOverview.topic)}
            onToggleFavorite={() =>
              onToggleTopicFavorite(topicOverview.topic)
            }
            onDelete={() => onDeleteTopic(topicOverview.topic)}
          />
        )}

        {!sidebarCollapsed && <WorkspaceUserMenu {...userMenu} variant="sidebar" />}
      </aside>

      <section className="board-main" aria-label="Board">
        <div className="board-main-body">
        {isAgentBoard && isOverviewView ? (
          <div className="board-secondary-view-panel agent-secondary-view-panel">
            <AgentManagementPanel
              agents={agents}
              searchQuery={searchQuery}
              activityFilter={agentActivityFilter}
              selectedAgentId={
                boardFocus.type === 'agent' ? boardFocus.agentId : undefined
              }
              tasks={tasks}
              onSelectAgent={(agentId) =>
                onSelectFocus({ type: 'agent', agentId })
              }
              onWorkspaceChange={setAgentWorkspace}
              directoryResetKey={agentDirectoryResetKey}
              onSelectTask={onSelectTask}
              onProvision={onProvisionAgent}
            />
          </div>
        ) : isMemberBoard && isOverviewView ? (
          <div className="board-secondary-view-panel member-secondary-view-panel">
            <MembersOverviewPanel
              members={boardMembers}
              onInviteMember={onInviteMember}
            />
          </div>
        ) : isAgentBoard ? (
          <div className="board-board-view board-agent-board-view">
            <div className="board-columns" role="list">
              {taskLists[0] && (
                <section
                  ref={(element) => {
                    if (element) columnRefs.current.set(taskLists[0].wire.id, element)
                    else columnRefs.current.delete(taskLists[0].wire.id)
                  }}
                  className="board-column board-column--member board-column--agent"
                  role="listitem"
                  aria-label="Tasklist agenti"
                >
                  <AgentTaskListHeader
                    onAddTask={() =>
                      openCreateTaskInList(taskLists[0].wire.id, taskLists[0].wire.id)
                    }
                  />
                  <ul className="board-cards">
                    {visibleTasks
                      .filter(
                        (task) => task.wire.active_assignee_identity_id == null,
                      )
                      .map((task) => (
                        <li key={task.wire.id}>
                          <BoardTaskCard
                            task={task}
                            selected={selectedTaskId === task.wire.id}
                            boardMemberById={boardMemberById}
                            hideAssignee
                            onSelect={() => onSelectTask(task.wire.id)}
                            onComplete={() => {
                              void onCompleteTask(task)
                            }}
                          />
                        </li>
                      ))}
                  </ul>
                </section>
              )}
              {orderedAgentColumns.map((agent) => {
                const agentTasks = visibleTasks.filter(
                  (task) =>
                    task.wire.active_assignee_identity_id ===
                    agent.principal_identity_id,
                )
                const taskList = taskLists[0]
                return (
                  <section
                    key={agent.id}
                    ref={(element) => {
                      if (element) {
                        columnRefs.current.set(
                          agent.principal_identity_id,
                          element,
                        )
                      } else {
                        columnRefs.current.delete(agent.principal_identity_id)
                      }
                    }}
                    className="board-column board-column--member board-column--agent"
                    role="listitem"
                    aria-label={`Tasklist ${agent.identity_handle}`}
                  >
                    <AgentColumnHeader
                      agent={agent}
                      onAddTask={() => {
                        if (!taskList) return
                        openCreateTaskInList(
                          taskList.wire.id,
                          agent.principal_identity_id,
                        )
                      }}
                    />
                    <ul className="board-cards">
                      {agentTasks.map((task) => (
                        <li key={task.wire.id}>
                          <BoardTaskCard
                            task={task}
                            selected={selectedTaskId === task.wire.id}
                            boardMemberById={boardMemberById}
                            hideAssignee
                            onSelect={() => onSelectTask(task.wire.id)}
                            onComplete={() => {
                              void onCompleteTask(task)
                            }}
                          />
                        </li>
                      ))}
                    </ul>
                    {!taskList && (
                      <p className="inline-empty">
                        Crea prima una tasklist in una categoria per aggiungere task.
                      </p>
                    )}
                  </section>
                )
              })}
            </div>
          </div>
        ) : historyList ? (
          <div className="board-secondary-view-panel">
            <TaskListHistoryPanel
            list={historyList}
            isEditing={editingListId === historyList.wire.id}
            editName={listEditName}
            editColor={listEditColor}
            editIcon={listEditIcon}
            iconPickerOpen={
              listIconPickerOpen && editingListId === historyList.wire.id
            }
            onEditNameChange={setListEditName}
            onStartEdit={() => {
              const listIndex = taskLists.findIndex(
                (item) => item.wire.id === historyList.wire.id,
              )
              startListEdit(historyList, listIndex >= 0 ? listIndex : 0)
            }}
            onAutoSave={() => {
              const listIndex = taskLists.findIndex(
                (item) => item.wire.id === historyList.wire.id,
              )
              commitListEdit(
                historyList,
                listIndex >= 0 ? listIndex : 0,
                false,
              )
            }}
            onCommitEdit={() => {
              const listIndex = taskLists.findIndex(
                (item) => item.wire.id === historyList.wire.id,
              )
              commitListEdit(historyList, listIndex >= 0 ? listIndex : 0)
            }}
            onToggleIconPicker={() => {
              setListIconPickerOpen((open) => {
                const next = !open
                if (next) {
                  updateIconPickerAnchorRect(historyList.wire.id)
                } else {
                  setListIconPickerAnchorRect(null)
                }
                return next
              })
            }}
            onLoadInfo={onLoadTaskListInfo}
            onCreateInfoDocument={onCreateTaskListInfoDocument}
            onUpdateInfoDocument={onUpdateInfoDocument}
            onUploadInfoFile={onUploadInfoDocumentFile}
            onReadInfoFile={onReadInfoDocumentFile}
            onDownloadInfoFile={onDownloadInfoDocumentFile}
              onClose={() => {
                cancelListEdit()
                closeListHistory()
              }}
            />
          </div>
        ) : isOverviewView ? (
          <div className="board-secondary-view-panel">
            <BoardOverviewView
              project={project}
              topic={focusedTopic}
              onLoadProjectInfo={onLoadProjectInfo}
              onCreateProjectInfoDocument={onCreateProjectInfoDocument}
              onLoadTopicInfo={onLoadTopicInfo}
              onCreateTopicInfoDocument={onCreateTopicInfoDocument}
              onUpdateInfoDocument={onUpdateInfoDocument}
              onUploadInfoDocumentFile={onUploadInfoDocumentFile}
              onReadInfoDocumentFile={onReadInfoDocumentFile}
              onDownloadInfoDocumentFile={onDownloadInfoDocumentFile}
            />
          </div>
        ) : isHistoryView ? (
          <div className="board-secondary-view-panel">
            <BoardHistoryView
              tasks={visibleTasks}
              boardMembers={boardMembers}
              taskLists={taskLists}
              groupModes={taskFilterGroups.map((group) =>
                group === 'listIds'
                  ? 'tasklist'
                  : group === 'types'
                    ? 'type'
                    : group === 'memberIds'
                      ? 'member'
                      : group === 'states'
                        ? 'state'
                        : 'date',
              )}
              selectedTaskId={selectedTaskId}
              scopeName={boardScopeName}
              onSelectTask={onSelectTask}
            />
          </div>
        ) : isTimelineView ? (
          <div className="board-secondary-view-panel">
            <BoardTimelineView
              taskLists={timelineLists}
              tasks={timelineTasks}
              weekAnchor={timelineWeekAnchor}
              scale={timelineScale}
              onScaleChange={setTimelineScale}
              onWeekAnchorChange={setTimelineWeekAnchor}
              selectedTaskId={selectedTaskId}
              onSelectTask={onSelectTask}
              onCompleteTask={onCompleteTask}
              onResizeTask={(task, range) => {
                void onUpdateTask(task, {
                  title: task.document.title,
                  notes: task.document.notes,
                  start_at: range.start_at,
                  due_at: range.due_at,
                  ...(task.document.priority !== undefined
                    ? { priority: task.document.priority }
                    : {}),
                  ...(task.document.recurrence !== undefined
                    ? { recurrence: task.document.recurrence }
                    : {}),
                })
              }}
              onCreateTaskInDay={openCreateTaskInTimelineDay}
            />
          </div>
        ) : (
        <div className="board-board-view">
          <div className="board-columns" role="list">
          {orderedLists.map((list, listIndex) => {
            const listTasks = visibleTasks.filter(
              (task) => task.wire.list_id === list.wire.id,
            )
            const listLocked = lockedTasks.filter(
              (task) => task.list_id === list.wire.id,
            )
            const listNameLabel = list.document?.name ?? 'Locked list'
            const isEditingList = editingListId === list.wire.id
            const columnBackgroundColor = isEditingList
              ? listEditColor
              : list.document?.color
            const columnTint = resolveTaskListColumnTint(columnBackgroundColor)
            return (
              <section
                key={list.wire.id}
                ref={(element) => {
                  if (element) columnRefs.current.set(list.wire.id, element)
                  else columnRefs.current.delete(list.wire.id)
                }}
                className={[
                  'board-column',
                  columnTint ? columnTintClass(columnTint) : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                role="listitem"
                aria-label={listNameLabel}
              >
                <BoardColumnHeader
                  list={list}
                  isEditing={isEditingList}
                  editName={listEditName}
                  editColor={listEditColor}
                  editIcon={listEditIcon}
                  iconPickerOpen={listIconPickerOpen && isEditingList}
                  onEditNameChange={setListEditName}
                  onCancelEdit={cancelListEdit}
                  onCommitEdit={() => commitListEdit(list, listIndex)}
                  onToggleIconPicker={() => {
                    setListIconPickerOpen((open) => {
                      const next = !open
                      if (next) {
                        updateIconPickerAnchorRect(list.wire.id)
                      } else {
                        setListIconPickerAnchorRect(null)
                      }
                      return next
                    })
                  }}
                  onOpenHistory={() => openListHistory(list.wire.id)}
                  onAddTask={() => {
                    openCreateTaskInList(list.wire.id, list.wire.id)
                  }}
                />

                <BoardColumnFilterBadges
                  filters={advancedTaskFilters}
                  members={boardMembers}
                  onRemove={removeTaskFilter}
                />

                <div className="board-column-card-content">
                  <BoardGroupedTaskCards
                    tasks={listTasks}
                    groups={taskFilterGroups.filter(
                      (group) =>
                        group !== 'listIds' &&
                        !(
                          group === 'dates' &&
                          taskFilterGroups.length === 1 &&
                          advancedTaskFilters.dates.length === 0
                        ),
                    )}
                    members={boardMembers}
                    selectedTaskId={selectedTaskId}
                    boardMemberById={boardMemberById}
                    onSelectTask={onSelectTask}
                    onCompleteTask={(task) => {
                      void onCompleteTask(task)
                    }}
                  />
                </div>
                <ul className="board-cards board-cards--locked">
                  {listLocked.map((task) => (
                    <li key={task.id} className="board-card locked">
                      <LockIcon />
                      <span>Locked task</span>
                    </li>
                  ))}
                </ul>

              </section>
            )
          })}

          {orderedMemberColumns.map((member) => {
            const memberTasks = visibleTasks.filter(
              (task) =>
                task.wire.active_assignee_identity_id === member.identityId,
            )
            return (
              <section
                key={member.identityId}
                ref={(element) => {
                  if (element) {
                    columnRefs.current.set(member.identityId, element)
                  } else {
                    columnRefs.current.delete(member.identityId)
                  }
                }}
                className="board-column board-column--member"
                role="listitem"
                aria-label={member.label}
              >
                <MemberColumnHeader member={member} />
                <ul className="board-cards">
                  {memberTasks.map((task) => (
                    <li key={task.wire.id}>
                      <BoardTaskCard
                        task={task}
                        selected={selectedTaskId === task.wire.id}
                        boardMemberById={boardMemberById}
                        hideAssignee
                        onSelect={() => onSelectTask(task.wire.id)}
                        onComplete={() => {
                          void onCompleteTask(task)
                        }}
                      />
                    </li>
                  ))}
                </ul>
              </section>
            )
          })}

          {!isMemberBoard && (
          <section
            className={
              showNewList
                ? 'board-column board-column-add board-column-add--editing'
                : 'board-column board-column-add'
            }
            role="listitem"
            aria-label="Nuova task list"
          >
            {showNewList ? (
              <form
                className="board-new-list-form-inline"
                onSubmit={(event) => void createList(event)}
              >
                <input
                  required
                  autoFocus
                  className="board-new-list-name"
                  placeholder="Nome task list"
                  value={listName}
                  onChange={(event) => setListName(event.target.value)}
                  aria-label="Task list name"
                />
                {boardFocus.type === 'topic' ? (
                  newListTopic && (
                    <div className="board-new-list-topic board-new-list-topic--fixed">
                      <span
                        className={`board-avatar board-new-list-topic-avatar ${topicAvatarClass(newListTopicIndex)}`}
                        aria-hidden
                      >
                        {initialFor(newListTopicLabel)}
                      </span>
                      <span className="board-new-list-topic-label">
                        {newListTopicLabel}
                      </span>
                    </div>
                  )
                ) : (
                  <label className="board-new-list-topic board-new-list-topic-picker">
                    <span className="board-new-list-topic-display" aria-hidden>
                      {newListTopic ? (
                        <span
                          className={`board-avatar board-new-list-topic-avatar ${topicAvatarClass(newListTopicIndex)}`}
                        >
                          {initialFor(newListTopicLabel)}
                        </span>
                      ) : (
                        <span className="board-avatar board-new-list-topic-avatar board-new-list-topic-placeholder">
                          <FolderIcon />
                        </span>
                      )}
                      <span
                        className={
                          newListTopic
                            ? 'board-new-list-topic-label'
                            : 'board-new-list-topic-label board-new-list-topic-label--placeholder'
                        }
                      >
                        {newListTopicLabel}
                      </span>
                    </span>
                    <select
                      required
                      className="board-new-list-topic-select"
                      value={newListTopicId}
                      onChange={(event) => setListTopicId(event.target.value)}
                      aria-label="Topic for new list"
                    >
                      <option value="">Categoria</option>
                      {topics.map((topic) => (
                        <option key={topic.wire.id} value={topic.wire.id}>
                          {topic.document?.name ?? topic.wire.id.slice(0, 8)}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                <div className="board-create-actions board-new-list-actions">
                  <button
                    type="button"
                    className="text-button board-new-list-cancel"
                    onClick={() => {
                      setShowNewList(false)
                      setListName('')
                      setListTopicId('')
                    }}
                  >
                    Annulla
                  </button>
                  <button
                    type="submit"
                    className="primary-button board-new-list-submit"
                  >
                    Crea
                  </button>
                </div>
              </form>
            ) : (
              <button
                type="button"
                className="board-column-add-trigger"
                onClick={() => {
                  if (boardFocus.type === 'topic') {
                    setListTopicId(boardFocus.topicId)
                  }
                  setShowNewList(true)
                }}
              >
                <PlusIcon />
                Nuova task list
              </button>
            )}
          </section>
          )}
          </div>
        </div>
        )}
        <div className="board-board-footer">
            <div className="board-toolbar-secondary">
              <nav
                className="board-footer-path"
                aria-label="Percorso file"
                onClickCapture={(event) => {
                  if (!openMobilePathPanel()) return
                  event.preventDefault()
                  event.stopPropagation()
                }}
              >
                <button
                  type="button"
                  onClick={() => onSelectFocus({ type: 'generali' })}
                >
                  {project.document?.name ?? 'Progetto'}
                </button>
                <span aria-hidden>›</span>
                {focusedTopic ? (
                  <>
                    <button
                      type="button"
                      onClick={() => onSelectFocus({ type: 'generali' })}
                    >
                      Generali
                    </button>
                    <span aria-hidden>›</span>
                    <strong>{boardFooterPath}</strong>
                  </>
                ) : (
                  <strong>{boardFooterPath}</strong>
                )}
              </nav>
              <button
                type="button"
                className="board-toolbar-agent"
                onClick={() => setAiBadgeOpen(true)}
              >
                <img src="/sprout-ai-logo.png" alt="" aria-hidden />
                Ask to AI
              </button>
            </div>
        </div>
        </div>
        {mobileSearchOpen && (
          <div className="board-mobile-search-layer">
            {searchQuery.trim() ? (
              <BoardMobileSearchResults
                query={searchQuery}
                lists={mobileSearchLists}
                tasks={mobileSearchTasks}
                memberColumns={mobileSearchMemberColumns}
                isMemberBoard={isMemberBoard}
                boardMemberById={boardMemberById}
                onSelectTask={(id) => {
                  rememberRecentSearch(searchQuery)
                  closeMobileSearch(false)
                  onSelectTask(id)
                }}
                onSelectList={(id) => {
                  rememberRecentSearch(searchQuery)
                  closeMobileSearch(false)
                  openListHistory(id)
                }}
              />
            ) : recentSearches.length > 0 ? (
              <BoardMobileRecentSearches
                items={recentSearches}
                onSelect={(query) => {
                  setSearchQuery(query)
                  mobileSearchInputRef.current?.focus()
                }}
              />
            ) : (
              <p className="board-mobile-search-hint">Cerca task e tasklist</p>
            )}
          </div>
        )}
      </section>

      {listIconPickerOpen &&
        editingList?.document &&
        listIconPickerAnchorRect && (
        <TaskListIconPanel
          anchorRect={listIconPickerAnchorRect}
          listName={listEditName.trim() || editingList.document.name}
          value={listEditIcon}
          color={listEditColor}
          onChange={setListEditIcon}
          onColorChange={setListEditColor}
          onClose={() => {
            setListIconPickerOpen(false)
            setListIconPickerAnchorRect(null)
          }}
        />
      )}

      {creatingList && createAnchorRect && (
        <CreateTaskPanel
          key={`${creatingList.wire.id}-${createInitialDueAt}-${createInitialAssigneeId ?? ''}`}
          list={creatingList}
          anchorRect={createAnchorRect}
          boardMembers={boardMembers}
          initialTaskKind={createInitialTaskKind}
          initialDueAt={createInitialDueAt}
          initialAssigneeIdentityId={createInitialAssigneeId}
          publishedQuestionnaireVersions={publishedQuestionnaireVersions}
          onCreateTask={onCreateTask}
          onCancel={closeCreateTask}
        />
      )}

      {selectedTask && (
        <TaskDetailPanel
          key={selectedTask.wire.id}
          task={selectedTask}
          boardMembers={boardMembers}
          publishedQuestionnaireVersions={publishedQuestionnaireVersions}
          savedAttachments={taskAttachments}
          attachmentLabels={taskAttachmentLabels}
          onRefreshAttachments={onRefreshTaskAttachments}
          onDownloadAttachment={onDownloadTaskAttachment}
          onUpdate={onUpdateTask}
          onAssign={onAssignTask}
          onComplete={onCompleteTask}
          onCopy={onCopyTask}
          onClose={closeTaskDetail}
        />
      )}

      {aiBadgeOpen && <BoardAiBadge onClose={() => setAiBadgeOpen(false)} />}
      {mobilePathPanelOpen && (
        <BoardPathBadge
          topics={sortedTopics}
          onSelectFocus={onSelectFocus}
          onClose={() => setMobilePathPanelOpen(false)}
        />
      )}

      <nav className="board-mobile-island" aria-label="Navigazione board">
        <BoardMobileIslandViewModes
          mode={boardViewMode}
          onChange={handleIslandViewModeChange}
        />
        <button
          type="button"
          className={
            isMemberBoardFocus(boardFocus)
              ? 'board-mobile-island-members active'
              : 'board-mobile-island-members'
          }
          aria-label="Membri"
          aria-current={isMemberBoardFocus(boardFocus) ? 'page' : undefined}
          onClick={handleIslandMembers}
        >
          <UsersIcon aria-hidden />
        </button>
        <button
          type="button"
          className={
            isAgentBoard
              ? 'board-mobile-island-agent active'
              : 'board-mobile-island-agent'
          }
          aria-label="Agenti"
          aria-current={isAgentBoard ? 'page' : undefined}
          onClick={handleIslandAgents}
        >
          <AgentIcon aria-hidden />
        </button>
        <WorkspaceUserMenu {...userMenu} variant="tab" />
      </nav>
    </div>
  )
}
