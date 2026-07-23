import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from 'react'
import type { TaskDto, Uuid } from '../api/contracts'
import {
  filterBoardSearch,
  filterTasks,
  formatDueDate,
  taskListsForMember,
  taskListsForTopic,
} from '../domain/tasks'
import type {
  DecryptedTask,
  TaskCreationInput,
  TaskFilter,
} from '../domain/models'
import type {
  BoardFocus,
  BoardMember,
  ProjectItem,
  TaskListItem,
  TopicItem,
} from '../store/app-store'
import {
  CalendarIcon,
  ChevronDownIcon,
  LockIcon,
  FolderIcon,
  PlusIcon,
  SearchIcon,
  SidebarCollapseIcon,
  SidebarExpandIcon,
} from './icons'
import {
  WorkspaceUserMenu,
  type WorkspaceUserMenuProps,
} from './WorkspaceUserMenu'

export interface TasksScreenProps {
  project?: ProjectItem
  topics: TopicItem[]
  taskLists: TaskListItem[]
  tasks: DecryptedTask[]
  lockedTasks: TaskDto[]
  boardMembers: BoardMember[]
  boardFocus: BoardFocus
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
  onSelectList(id: Uuid): void
  onSelectTask(id: Uuid | undefined): void
  onFilter(filter: TaskFilter): void
  onCreateTopic(name: string): Promise<void>
  onCreateList(name: string, topicId: Uuid): Promise<void>
  onCreateTask(input: TaskCreationInput, listId: Uuid): Promise<void>
  onUpdateTask(
    task: DecryptedTask,
    input: { title: string; notes?: string },
  ): Promise<void>
  onCompleteTask(task: DecryptedTask): Promise<void>
  onCopyTask(task: DecryptedTask): Promise<void>
  userMenu: Omit<WorkspaceUserMenuProps, 'variant'>
}

const initialsFor = (label: string): string => {
  const parts = label.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return `${parts[0][0] ?? ''}${parts[1][0] ?? ''}`.toUpperCase()
}

const TOPIC_AVATAR_COLORS = [
  'topic-green',
  'topic-amber',
  'topic-teal',
  'topic-pink',
] as const

const topicAvatarClass = (index: number): (typeof TOPIC_AVATAR_COLORS)[number] =>
  TOPIC_AVATAR_COLORS[index % TOPIC_AVATAR_COLORS.length]

const COLUMN_AVATAR_COLORS = [
  'column-blue',
  'column-violet',
  'column-rose',
  'column-emerald',
] as const

const columnAvatarClass = (
  index: number,
): (typeof COLUMN_AVATAR_COLORS)[number] =>
  COLUMN_AVATAR_COLORS[index % COLUMN_AVATAR_COLORS.length]

const BOARD_FILTER_OPTIONS = [
  ['open', 'Aperti'],
  ['today', 'Oggi'],
  ['upcoming', 'Prossimi'],
  ['completed', 'Completati'],
] as const satisfies ReadonlyArray<[TaskFilter, string]>

const TASK_KIND_OPTIONS = [
  ['priority', 'Priorità'],
  ['deadline', 'Scadenza'],
  ['recurring', 'Ricorrente'],
] as const satisfies ReadonlyArray<
  ['priority' | 'deadline' | 'recurring', string]
>

const TASK_PRIORITY_OPTIONS = [
  ['low', 'Bassa'],
  ['normal', 'Normale'],
  ['high', 'Alta'],
] as const satisfies ReadonlyArray<['low' | 'normal' | 'high', string]>

const RECURRENCE_FREQUENCY_OPTIONS = [
  ['daily', 'Giorno'],
  ['weekly', 'Settimana'],
  ['monthly', 'Mese'],
] as const satisfies ReadonlyArray<
  ['daily' | 'weekly' | 'monthly', string]
>

const boardFilterLabel = (filter: TaskFilter): string =>
  BOARD_FILTER_OPTIONS.find(([value]) => value === filter)?.[1] ?? 'Aperti'

const SIDEBAR_COLLAPSED_KEY = 'sprout-board-sidebar-collapsed'

const readSidebarCollapsed = (): boolean => {
  try {
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

const BoardFilterDropdown = ({
  filter,
  onFilter,
}: {
  filter: TaskFilter
  onFilter(filter: TaskFilter): void
}) => {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const menuId = useId()
  const filterLabel = boardFilterLabel(filter)

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

  const select = (value: TaskFilter) => {
    onFilter(value)
    setOpen(false)
  }

  return (
    <div className="board-filter-dropdown" ref={rootRef}>
      <button
        type="button"
        className="board-filter-trigger"
        aria-expanded={open}
        aria-haspopup="menu"
        aria-controls={menuId}
        aria-label={`Filtra task: ${filterLabel}`}
        onClick={() => setOpen((value) => !value)}
      >
        <CalendarIcon />
        <span>{filterLabel}</span>
        <ChevronDownIcon className="board-filter-chevron" />
      </button>
      {open && (
        <div
          id={menuId}
          className="board-filter-menu"
          role="menu"
          aria-label="Filtra task"
        >
          {BOARD_FILTER_OPTIONS.map(([value, label]) => (
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
              onClick={() => select(value)}
            >
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

const EditTaskForm = ({
  task,
  onUpdate,
}: {
  task: DecryptedTask
  onUpdate(
    task: DecryptedTask,
    input: { title: string; notes?: string },
  ): Promise<void>
}) => {
  const [title, setTitle] = useState(task.document.title)
  const [notes, setNotes] = useState(task.document.notes ?? '')
  return (
    <form
      className="detail-edit-form"
      onSubmit={(event) => {
        event.preventDefault()
        void onUpdate(task, { title, notes: notes || undefined })
      }}
    >
      <label>
        Title
        <input
          required
          value={title}
          onChange={(event) => setTitle(event.target.value)}
        />
      </label>
      <label>
        Notes
        <textarea
          value={notes}
          onChange={(event) => setNotes(event.target.value)}
        />
      </label>
      <button type="submit" className="secondary-button">
        Save
      </button>
    </form>
  )
}

const CreateTaskModal = ({
  listId,
  publishedQuestionnaireVersions,
  onCreateTask,
  onCancel,
}: {
  listId: Uuid
  publishedQuestionnaireVersions: Array<{ id: Uuid; label: string }>
  onCreateTask(input: TaskCreationInput, listId: Uuid): Promise<void>
  onCancel(): void
}) => {
  const [taskTitle, setTaskTitle] = useState('')
  const [taskDueAt, setTaskDueAt] = useState('')
  const [taskKind, setTaskKind] = useState<
    'priority' | 'deadline' | 'recurring'
  >('priority')
  const [taskPriority, setTaskPriority] = useState<'low' | 'normal' | 'high'>(
    'normal',
  )
  const [recurrenceFrequency, setRecurrenceFrequency] = useState<
    'daily' | 'weekly' | 'monthly'
  >('daily')
  const [recurrenceInterval, setRecurrenceInterval] = useState('1')
  const [questionnaireVersionId, setQuestionnaireVersionId] = useState('')

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
      questionnaireVersionId: questionnaireVersionId || undefined,
    }
    const dueAt = taskDueAt ? new Date(taskDueAt).toISOString() : ''
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
    await onCreateTask(input, listId)
    onCancel()
  }

  return (
    <div
      className="task-create-overlay"
      onClick={onCancel}
      aria-hidden={false}
    >
      <div
        className="task-create-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Nuovo task"
        onClick={(event) => event.stopPropagation()}
      >
        <form
          className="task-create-form"
          onSubmit={(event) => void submit(event)}
        >
          <input
            className="task-create-title"
            required
            autoFocus
            placeholder="Titolo del task"
            value={taskTitle}
            onChange={(event) => setTaskTitle(event.target.value)}
            aria-label="Titolo"
          />
          <div className="task-create-fields">
            <fieldset className="task-create-field task-create-field--choices">
              <legend className="task-create-field-label">Tipo</legend>
              <div
                className="task-create-choice-group task-create-choice-group--horizontal"
                role="radiogroup"
                aria-label="Tipo"
              >
                {TASK_KIND_OPTIONS.map(([value, label]) => (
                  <button
                    type="button"
                    key={value}
                    role="radio"
                    aria-checked={taskKind === value}
                    className={
                      taskKind === value
                        ? 'task-create-choice task-create-choice--kind selected'
                        : 'task-create-choice task-create-choice--kind'
                    }
                    onClick={() => setTaskKind(value)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </fieldset>
            {taskKind === 'priority' ? (
              <fieldset className="task-create-field task-create-field--choices">
                <legend className="task-create-field-label">Priorità</legend>
                <div
                  className="task-create-choice-group task-create-choice-group--horizontal"
                  role="radiogroup"
                  aria-label="Priorità"
                >
                  {TASK_PRIORITY_OPTIONS.map(([value, label]) => (
                    <button
                      type="button"
                      key={value}
                      role="radio"
                      aria-checked={taskPriority === value}
                      className={
                        taskPriority === value
                          ? `task-create-choice task-create-choice--priority-${value} selected`
                          : `task-create-choice task-create-choice--priority-${value}`
                      }
                      onClick={() => setTaskPriority(value)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </fieldset>
            ) : (
              <label className="task-create-field">
                <span className="task-create-field-label">
                  {taskKind === 'recurring' ? 'Prima occorrenza' : 'Scadenza'}
                </span>
                <input
                  required
                  type="datetime-local"
                  value={taskDueAt}
                  aria-label={
                    taskKind === 'recurring' ? 'Prima occorrenza' : 'Scadenza'
                  }
                  onChange={(event) => setTaskDueAt(event.target.value)}
                />
              </label>
            )}
            {taskKind === 'recurring' && (
              <>
                <label className="task-create-field">
                  <span className="task-create-field-label">Ogni</span>
                  <input
                    required
                    type="number"
                    min="1"
                    step="1"
                    value={recurrenceInterval}
                    aria-label="Intervallo ricorrenza"
                    onChange={(event) =>
                      setRecurrenceInterval(event.target.value)
                    }
                  />
                </label>
                <fieldset className="task-create-field task-create-field--choices">
                  <legend className="task-create-field-label">Unità</legend>
                  <div
                    className="task-create-choice-group"
                    role="radiogroup"
                    aria-label="Unità ricorrenza"
                  >
                    {RECURRENCE_FREQUENCY_OPTIONS.map(([value, label]) => (
                      <button
                        type="button"
                        key={value}
                        role="radio"
                        aria-checked={recurrenceFrequency === value}
                        className={
                          recurrenceFrequency === value
                            ? 'task-create-choice task-create-choice--kind selected'
                            : 'task-create-choice task-create-choice--kind'
                        }
                        onClick={() => setRecurrenceFrequency(value)}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                </fieldset>
              </>
            )}
            {publishedQuestionnaireVersions.length > 0 && (
              <fieldset className="task-create-field task-create-field--choices">
                <legend className="task-create-field-label">Questionario</legend>
                <div
                  className="task-create-choice-group"
                  role="radiogroup"
                  aria-label="Questionario"
                >
                  <button
                    type="button"
                    role="radio"
                    aria-checked={questionnaireVersionId === ''}
                    className={
                      questionnaireVersionId === ''
                        ? 'task-create-choice task-create-choice--kind selected'
                        : 'task-create-choice task-create-choice--kind'
                    }
                    onClick={() => setQuestionnaireVersionId('')}
                  >
                    Nessuno
                  </button>
                  {publishedQuestionnaireVersions.map((version) => (
                    <button
                      type="button"
                      key={version.id}
                      role="radio"
                      aria-checked={questionnaireVersionId === version.id}
                      className={
                        questionnaireVersionId === version.id
                          ? 'task-create-choice task-create-choice--kind selected'
                          : 'task-create-choice task-create-choice--kind'
                      }
                      onClick={() => setQuestionnaireVersionId(version.id)}
                    >
                      {version.label}
                    </button>
                  ))}
                </div>
              </fieldset>
            )}
          </div>
          <div className="task-create-actions">
            <button
              type="button"
              className="task-create-cancel"
              onClick={onCancel}
            >
              Annulla
            </button>
            <button type="submit" className="task-create-submit">
              Crea
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

export const TasksScreen = ({
  project,
  topics,
  taskLists,
  tasks,
  lockedTasks,
  boardMembers,
  boardFocus,
  selectedTopicId,
  selectedTaskId,
  currentUserLabel,
  publishedQuestionnaireVersions,
  filter,
  loading,
  onSelectFocus,
  onSelectList,
  onSelectTask,
  onFilter,
  onCreateTopic,
  onCreateList,
  onCreateTask,
  onUpdateTask,
  onCompleteTask,
  onCopyTask,
  userMenu,
}: TasksScreenProps) => {
  const [topicName, setTopicName] = useState('')
  const [showNewTopic, setShowNewTopic] = useState(false)
  const [listName, setListName] = useState('')
  const [listTopicId, setListTopicId] = useState('')
  const [showNewList, setShowNewList] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [creatingInListId, setCreatingInListId] = useState<Uuid | undefined>()
  const [sidebarCollapsed, setSidebarCollapsed] = useState(readSidebarCollapsed)

  const toggleSidebarCollapsed = () => {
    setSidebarCollapsed((value) => {
      const next = !value
      persistSidebarCollapsed(next)
      return next
    })
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

  const focusLists = useMemo(() => {
    if (boardFocus.type === 'topic') {
      return taskListsForTopic(taskLists, boardFocus.topicId)
    }
    if (boardFocus.type === 'member') {
      return taskListsForMember(taskLists, tasks, boardFocus.identityId)
    }
    return taskLists
  }, [boardFocus, taskLists, tasks])

  const focusTasks = useMemo(() => {
    if (boardFocus.type === 'member') {
      return tasks.filter(
        (task) =>
          task.wire.active_assignee_identity_id === boardFocus.identityId,
      )
    }
    if (boardFocus.type === 'topic') {
      const listIds = new Set(focusLists.map((list) => list.wire.id))
      return tasks.filter((task) => listIds.has(task.wire.list_id))
    }
    return tasks
  }, [boardFocus, focusLists, tasks])

  const searched = useMemo(
    () => filterBoardSearch(focusLists, focusTasks, searchQuery),
    [focusLists, focusTasks, searchQuery],
  )

  const visibleTasks = useMemo(
    () => filterTasks(searched.tasks, filter),
    [filter, searched.tasks],
  )

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

  useEffect(() => {
    if (!selectedTask) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeTaskDetail()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [selectedTask, onSelectTask])

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
    return (
      <section className="screen-empty">
        <h2>No project selected</h2>
        <p>Create or select an encrypted project to load its resources.</p>
      </section>
    )
  }

  return (
    <div
      className={
        sidebarCollapsed
          ? 'board-layout board-layout--sidebar-collapsed'
          : 'board-layout'
      }
    >
      <aside
        className={
          sidebarCollapsed
            ? 'board-sidebar board-sidebar--collapsed'
            : 'board-sidebar'
        }
        aria-label="Board navigation"
        aria-expanded={!sidebarCollapsed}
      >
        <div className="board-sidebar-top">
          {showNewTopic ? (
            <form
              className="board-new-topic-form"
              onSubmit={(event) => void createTopic(event)}
            >
              <input
                required
                autoFocus
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
          ) : (
            <div className="board-sidebar-top-row">
              <button
                type="button"
                className={
                  sidebarCollapsed
                    ? 'board-new-category board-new-category--icon'
                    : 'board-new-category'
                }
                onClick={openNewTopic}
                aria-label="Nuova categoria"
              >
                <PlusIcon />
                <span
                  className="board-new-category-label"
                  aria-hidden={sidebarCollapsed}
                >
                  Nuova categoria
                </span>
              </button>
              <button
                type="button"
                className="board-sidebar-toggle board-sidebar-toggle--in-sidebar"
                onClick={toggleSidebarCollapsed}
                aria-label="Riduci sidebar"
                aria-hidden={sidebarCollapsed}
                tabIndex={sidebarCollapsed ? -1 : undefined}
              >
                <SidebarCollapseIcon />
              </button>
            </div>
          )}
        </div>

        <nav className="board-nav">
          <div className="board-nav-section">
            <p className="board-nav-heading">Spazio</p>
            <ul className="board-nav-list">
              <li>
                <button
                  type="button"
                  className={
                    boardFocus.type === 'generali'
                      ? 'board-nav-item active'
                      : 'board-nav-item'
                  }
                  onClick={() => onSelectFocus({ type: 'generali' })}
                >
                  <span className="board-avatar generali" aria-hidden>
                    G
                  </span>
                  <span className="board-nav-label">Generali</span>
                </button>
              </li>
            </ul>
          </div>

          {boardMembers.length > 0 && (
            <div className="board-nav-section">
              <p className="board-nav-heading">Membri</p>
              <ul className="board-nav-list board-nav-list-nested">
                {boardMembers.map((member) => (
                  <li key={member.identityId}>
                    <button
                      type="button"
                      className={
                        boardFocus.type === 'member' &&
                        boardFocus.identityId === member.identityId
                          ? 'board-nav-item active'
                          : 'board-nav-item'
                      }
                      onClick={() =>
                        onSelectFocus({
                          type: 'member',
                          identityId: member.identityId,
                        })
                      }
                    >
                      <span className="board-avatar member" aria-hidden>
                        {initialsFor(member.label)}
                      </span>
                      <span className="board-nav-label">{member.label}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div className="board-nav-section">
            <p className="board-nav-heading">Categorie</p>
            <ul className="board-nav-list">
              {topics.map((topic, topicIndex) => (
                <li key={topic.wire.id}>
                  <button
                    type="button"
                    className={
                      boardFocus.type === 'topic' &&
                      boardFocus.topicId === topic.wire.id
                        ? 'board-nav-item active'
                        : 'board-nav-item'
                    }
                    onClick={() =>
                      onSelectFocus({
                        type: 'topic',
                        topicId: topic.wire.id,
                      })
                    }
                  >
                    {topic.document ? (
                      <>
                        <span
                          className={`board-avatar ${topicAvatarClass(topicIndex)}`}
                          aria-hidden
                        >
                          {initialsFor(topic.document.name)}
                        </span>
                        <span className="board-nav-label">
                          {topic.document.name}
                        </span>
                      </>
                    ) : (
                      <>
                        <span className="board-avatar locked" aria-hidden>
                          <LockIcon />
                        </span>
                        <span className="board-nav-label">Locked topic</span>
                      </>
                    )}
                  </button>
                </li>
              ))}
            </ul>
            {topics.length === 0 && !loading && (
              <p className="inline-empty">Nessuna categoria.</p>
            )}
          </div>
        </nav>

        <WorkspaceUserMenu {...userMenu} variant="sidebar" />
      </aside>

      <section className="board-main" aria-label="Board">
        <header className="board-toolbar">
          <div className="board-toolbar-start">
            {sidebarCollapsed && (
              <button
                type="button"
                className="board-sidebar-toggle"
                onClick={toggleSidebarCollapsed}
                aria-label="Espandi sidebar"
              >
                <SidebarExpandIcon />
              </button>
            )}
            <BoardFilterDropdown filter={filter} onFilter={onFilter} />
          </div>
          <label className="board-search">
            <SearchIcon />
            <input
              type="search"
              placeholder="Cerca task e tasklist"
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              aria-label="Cerca task e tasklist"
            />
          </label>
        </header>

        {loading && <div className="loading-state">Caricamento…</div>}

        <div className="board-columns" role="list">
          {searched.lists.map((list, listIndex) => {
            const listTasks = visibleTasks.filter(
              (task) => task.wire.list_id === list.wire.id,
            )
            const listLocked = lockedTasks.filter(
              (task) => task.list_id === list.wire.id,
            )
            const listNameLabel = list.document?.name ?? 'Locked list'
            return (
              <section
                key={list.wire.id}
                className="board-column"
                role="listitem"
                aria-label={listNameLabel}
              >
                <header className="board-column-header">
                  <div className="board-column-identity">
                    <span
                      className={`board-avatar column ${columnAvatarClass(listIndex)}`}
                      aria-hidden
                    >
                      {list.document ? (
                        initialsFor(list.document.name)
                      ) : (
                        <LockIcon />
                      )}
                    </span>
                    <h3>{listNameLabel}</h3>
                  </div>
                  <div className="board-column-actions">
                    <button
                      type="button"
                      className="board-add-task"
                      onClick={() => {
                        onSelectList(list.wire.id)
                        setCreatingInListId(list.wire.id)
                      }}
                    >
                      <PlusIcon />
                      Aggiungi
                    </button>
                  </div>
                </header>

                <ul className="board-cards">
                  {listTasks.map((task) => {
                    const open = task.wire.state.state === 'open'
                    return (
                      <li key={task.wire.id}>
                        <article
                          className={
                            selectedTaskId === task.wire.id
                              ? 'board-card selected'
                              : 'board-card'
                          }
                        >
                          <div className="board-card-top">
                            <input
                              type="checkbox"
                              checked={!open}
                              disabled={!open || !task.wire.active_assignment_id}
                              aria-label={`Complete ${task.document.title}`}
                              onChange={() => {
                                if (open) void onCompleteTask(task)
                              }}
                            />
                            <button
                              type="button"
                              className="board-card-body"
                              onClick={() => onSelectTask(task.wire.id)}
                            >
                              <strong>{task.document.title}</strong>
                              {task.document.notes && (
                                <span className="board-card-notes">
                                  {task.document.notes}
                                </span>
                              )}
                              {task.document.due_at && (
                                <span className="board-card-due">
                                  {formatDueDate(task.document.due_at)}
                                </span>
                              )}
                            </button>
                          </div>
                        </article>
                      </li>
                    )
                  })}
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
                        {initialsFor(newListTopicLabel)}
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
                          {initialsFor(newListTopicLabel)}
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
        </div>
      </section>

      {creatingInListId && (
        <CreateTaskModal
          listId={creatingInListId}
          publishedQuestionnaireVersions={publishedQuestionnaireVersions}
          onCreateTask={onCreateTask}
          onCancel={() => setCreatingInListId(undefined)}
        />
      )}

      {selectedTask && (
        <div
          className="board-detail-overlay"
          onClick={closeTaskDetail}
          aria-hidden={false}
        >
          <aside
            className="board-detail-drawer"
            role="dialog"
            aria-modal="true"
            aria-label="Task detail"
            onClick={(event) => event.stopPropagation()}
          >
            <header className="board-detail-header">
              <h2>{selectedTask.document.title}</h2>
              <button
                type="button"
                className="board-detail-close"
                aria-label="Close task detail"
                onClick={closeTaskDetail}
              >
                ×
              </button>
            </header>
            <dl className="task-metadata">
              <div>
                <dt>State</dt>
                <dd>{selectedTask.wire.state.state}</dd>
              </div>
              <div>
                <dt>Type</dt>
                <dd>{selectedTask.wire.task_kind}</dd>
              </div>
              {selectedTask.document.due_at && (
                <div>
                  <dt>Due</dt>
                  <dd>{formatDueDate(selectedTask.document.due_at)}</dd>
                </div>
              )}
            </dl>
            <section className="detail-section">
              <h3>Notes</h3>
              <p>{selectedTask.document.notes ?? 'No notes.'}</p>
            </section>
            <section className="detail-section">
              <h3>Edit</h3>
              <EditTaskForm
                key={selectedTask.wire.id}
                task={selectedTask}
                onUpdate={onUpdateTask}
              />
            </section>
            <section className="detail-section">
              <h3>Lifecycle</h3>
              {selectedTask.wire.state.state === 'open' ? (
                <button
                  type="button"
                  className="primary-button"
                  disabled={!selectedTask.wire.active_assignment_id}
                  onClick={() => void onCompleteTask(selectedTask)}
                >
                  Complete as assignee
                </button>
              ) : (
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => void onCopyTask(selectedTask)}
                >
                  Copy as a new open task
                </button>
              )}
            </section>
          </aside>
        </div>
      )}
    </div>
  )
}
