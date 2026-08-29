import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { createPortal } from 'react-dom'
import type { Uuid } from '../api/contracts'
import {
  formatDueDate,
  formatTaskCardDueDate,
  getTaskCompletedAt,
  getTaskHistoryDay,
  getTaskStatusIndicator,
  groupTasksByHistoryDay,
  type TaskStatusIndicator,
} from '../domain/tasks'
import {
  memberAvatarColor,
  resolveTaskListIconColorFromStored,
  type DecryptedTask,
  type TaskListColumnColor,
  type TaskListIcon,
  type DecryptedInfoDocument,
  type InfoDocumentContent,
  type InfoFileBlock,
} from '../domain/models'
import type { BoardMember, TaskListItem } from '../store/app-store'
import { TaskListAvatarContent } from './TaskListAvatarContent'
import { TaskListInfoPanel } from './TaskListInfoPanel'

const HIDE_TOOLTIP_DELAY_MS = 120

const initialFor = (label: string): string => {
  const trimmed = label.trim()
  if (!trimmed) return '?'
  return trimmed[0].toUpperCase()
}

const columnAvatarColorClass = (color: TaskListColumnColor): string =>
  `board-avatar column board-avatar--${color}`

const clampToViewport = (
  anchorRect: DOMRect,
  width: number,
  height: number,
  gap = 8,
): { left: number; top: number } => {
  const margin = 8
  let top = anchorRect.top - height - gap
  if (top < margin) top = anchorRect.bottom + gap
  let left = anchorRect.left + anchorRect.width / 2 - width / 2
  left = Math.min(Math.max(margin, left), window.innerWidth - width - margin)
  top = Math.min(Math.max(margin, top), window.innerHeight - height - margin)
  return { left, top }
}

const TaskHistoryTooltip = ({
  task,
  status,
  assignee,
  anchorEl,
  tooltipId,
  onPointerEnter,
  onPointerLeave,
}: {
  task: DecryptedTask
  status: TaskStatusIndicator
  assignee?: BoardMember
  anchorEl: HTMLElement
  tooltipId: string
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
    const next = clampToViewport(
      anchorEl.getBoundingClientRect(),
      node.getBoundingClientRect().width,
      node.getBoundingClientRect().height,
    )
    setPosition({ left: next.left, top: next.top })
  }, [anchorEl, task.wire.id])

  const completedAt = getTaskCompletedAt(task)
  const dueAt = task.document.due_at
  const dueTone =
    status.variant === 'overdue' || status.variant === 'priority-high'
      ? 'overdue'
      : status.variant === 'due-today' ||
          status.variant === 'due-soon' ||
          status.variant === 'priority-normal'
        ? 'soon'
        : status.variant === 'completed'
          ? 'completed'
          : undefined

  return createPortal(
    <div
      ref={popoverRef}
      id={tooltipId}
      className={`tasklist-history-tooltip tasklist-history-tooltip--${status.variant}`}
      role="tooltip"
      style={
        {
          ...position,
          position: 'fixed',
          ...(status.dueProgress === undefined
            ? {}
            : { '--task-due-progress': status.dueProgress }),
        } as CSSProperties
      }
      onMouseEnter={onPointerEnter}
      onMouseLeave={onPointerLeave}
    >
      <p className="tasklist-history-tooltip-title">{task.document.title}</p>
      <p
        className={`tasklist-history-tooltip-status tasklist-history-tooltip-status--${status.variant}`}
      >
        <span
          className={`board-task-check board-task-check--${status.variant}`}
          aria-hidden
        >
          <span className="board-task-check-dot" />
        </span>
        {status.label}
      </p>
      {task.document.notes?.trim() && (
        <p className="tasklist-history-tooltip-notes">{task.document.notes}</p>
      )}
      <dl className="tasklist-history-tooltip-meta">
        {dueAt && (
          <>
            <dt>Scadenza</dt>
            <dd
              className={
                dueTone
                  ? `tasklist-history-tooltip-due tasklist-history-tooltip-due--${dueTone}`
                  : undefined
              }
            >
              {formatDueDate(dueAt)}
            </dd>
          </>
        )}
        {completedAt && (
          <>
            <dt>Completata</dt>
            <dd className="tasklist-history-tooltip-due tasklist-history-tooltip-due--completed">
              {formatTaskCardDueDate(completedAt.toISOString())}
            </dd>
          </>
        )}
        {assignee && (
          <>
            <dt>Assegnata</dt>
            <dd className="tasklist-history-tooltip-assignee">
              <span
                className={`board-avatar member board-avatar--${memberAvatarColor(assignee.identityId)}`}
                aria-hidden
              >
                {initialFor(assignee.label)}
              </span>
              {assignee.label}
            </dd>
          </>
        )}
      </dl>
    </div>,
    document.body,
  )
}

const TaskHistoryDot = ({
  task,
  boardMembers,
  selected,
  onSelect,
}: {
  task: DecryptedTask
  boardMembers: BoardMember[]
  selected: boolean
  onSelect(): void
}) => {
  const triggerRef = useRef<HTMLButtonElement>(null)
  const tooltipId = useId()
  const [open, setOpen] = useState(false)
  const hideTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  )
  const status = getTaskStatusIndicator(task)
  const assignee = boardMembers.find(
    (member) => member.identityId === task.wire.active_assignee_identity_id,
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
    hideTimeoutRef.current = setTimeout(() => setOpen(false), HIDE_TOOLTIP_DELAY_MS)
  }

  useEffect(() => () => clearHideTimeout(), [])

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={
          selected
            ? `tasklist-history-dot board-task-check board-task-check--${status.variant} selected`
            : `tasklist-history-dot board-task-check board-task-check--${status.variant}`
        }
        style={
          status.dueProgress === undefined
            ? undefined
            : ({ '--task-due-progress': status.dueProgress } as CSSProperties)
        }
        aria-label={`${task.document.title}: ${status.label}`}
        aria-describedby={open ? tooltipId : undefined}
        onMouseEnter={show}
        onMouseLeave={scheduleHide}
        onFocus={show}
        onBlur={scheduleHide}
        onClick={onSelect}
      >
        <span className="board-task-check-dot" aria-hidden />
      </button>
      {open && triggerRef.current && (
        <TaskHistoryTooltip
          task={task}
          status={status}
          assignee={assignee}
          anchorEl={triggerRef.current}
          tooltipId={tooltipId}
          onPointerEnter={show}
          onPointerLeave={scheduleHide}
        />
      )}
    </>
  )
}

const TaskHistoryRow = ({
  task,
  boardMembers,
  selected,
  onSelect,
}: {
  task: DecryptedTask
  boardMembers: BoardMember[]
  selected: boolean
  onSelect(): void
}) => {
  const status = getTaskStatusIndicator(task)
  const historyDate = getTaskHistoryDay(task)
  const assignee = boardMembers.find(
    (member) => member.identityId === task.wire.active_assignee_identity_id,
  )

  return (
    <button
      type="button"
      className={selected ? 'tasklist-history-row selected' : 'tasklist-history-row'}
      onClick={onSelect}
    >
      <span className={`board-task-check board-task-check--${status.variant}`} aria-hidden>
        <span className="board-task-check-dot" />
      </span>
      <span className="tasklist-history-row-content">
        <strong>{task.document.title}</strong>
        {task.document.notes?.trim() && <span>{task.document.notes}</span>}
      </span>
      <time dateTime={historyDate.toISOString()}>
        {formatTaskCardDueDate(historyDate.toISOString())}
      </time>
      {assignee && (
        <span
          className={`board-avatar member board-avatar--${memberAvatarColor(assignee.identityId)} tasklist-history-row-assignee`}
          title={assignee.label}
          aria-label={assignee.label}
        >
          {initialFor(assignee.label)}
        </span>
      )}
    </button>
  )
}

export const TaskHistoryRows = ({
  tasks,
  boardMembers,
  taskLists = [],
  groupModes = ['date'],
  selectedTaskId,
  emptyMessage,
  onSelectTask,
}: {
  tasks: DecryptedTask[]
  boardMembers: BoardMember[]
  taskLists?: TaskListItem[]
  groupModes?: Array<'tasklist' | 'type' | 'member' | 'state' | 'date'>
  selectedTaskId?: Uuid
  emptyMessage: string
  onSelectTask(id: Uuid): void
}) => {
  const groups = useMemo(() => {
    const listNames = new Map(
      taskLists.map((list) => [list.wire.id, list.document?.name ?? 'Tasklist']),
    )
    const listColors = new Map(
      taskLists.map((list) => [
        list.wire.id,
        resolveTaskListIconColorFromStored(list.document?.color, list.wire.id),
      ]),
    )
    const memberNames = new Map(
      boardMembers.map((member) => [member.identityId, member.label]),
    )
    const historyDays = new Map<string, { key: string; label: string; rank: number }>()
    groupTasksByHistoryDay(tasks).forEach((group, index) => {
      group.tasks.forEach((task) => {
        historyDays.set(task.wire.id, {
          key: `date-${group.key}`,
          label: group.label,
          rank: index,
        })
      })
    })
    type GroupPart = {
      key: string
      label: string
      rank: number
      color?: TaskListColumnColor
      memberId?: Uuid
    }
    const grouped = new Map<
      string,
      { key: string; labels: GroupPart[]; tasks: DecryptedTask[] }
    >()

    const partFor = (
      task: DecryptedTask,
      groupMode: 'tasklist' | 'type' | 'member' | 'state' | 'date',
    ): GroupPart => {
      if (groupMode === 'date') {
        return historyDays.get(task.wire.id) ?? {
          key: 'date-none',
          label: 'Senza data',
          rank: 999,
        }
      }
      if (groupMode === 'tasklist') {
        const label = listNames.get(task.wire.list_id) ?? 'Tasklist'
        return {
          key: `list-${task.wire.list_id}`,
          label,
          rank: 0,
          color: listColors.get(task.wire.list_id),
        }
      }
      if (groupMode === 'member') {
        const memberId = task.wire.active_assignee_identity_id
        const label = memberId ? memberNames.get(memberId) ?? 'Membro' : 'Senza membro'
        return {
          key: memberId ? `member-${memberId}` : 'member-none',
          label,
          rank: 0,
          memberId: memberId ?? undefined,
        }
      }
      if (groupMode === 'state') {
        const completed = task.wire.state.state === 'completed'
        return {
          key: completed ? 'state-completed' : 'state-open',
          label: completed ? 'Completati' : 'Da completare',
          rank: completed ? 1 : 0,
        }
      }
      if (task.document.recurrence) {
        return { key: 'type-recurring', label: 'Ricorsività', rank: 4 }
      }
      if (task.document.due_at) {
        return { key: 'type-deadline', label: 'Scadenza', rank: 3 }
      }
      const priority = task.document.priority ?? 'normal'
      return {
        key: `type-priority-${priority}`,
        label:
          priority === 'high'
            ? 'Priorità alta'
            : priority === 'low'
              ? 'Priorità bassa'
              : 'Priorità media',
        rank: priority === 'high' ? 0 : priority === 'normal' ? 1 : 2,
      }
    }

    for (const task of tasks) {
      const labels = groupModes.map((mode) => partFor(task, mode))
      const key = labels.map((label) => label.key).join('|')
      const existing = grouped.get(key)
      if (existing) existing.tasks.push(task)
      else grouped.set(key, { key, labels, tasks: [task] })
    }

    return [...grouped.values()].sort((left, right) => {
      for (let index = 0; index < left.labels.length; index += 1) {
        const leftPart = left.labels[index]
        const rightPart = right.labels[index]
        if (leftPart.rank !== rightPart.rank) return leftPart.rank - rightPart.rank
        const labelOrder = leftPart.label.localeCompare(rightPart.label, 'it', {
          sensitivity: 'base',
        })
        if (labelOrder !== 0) return labelOrder
      }
      return 0
    })
  }, [boardMembers, groupModes, taskLists, tasks])

  const sections = groups.reduce<
    Array<{
      key: string
      label: (typeof groups)[number]['labels'][number]
      groups: typeof groups
    }>
  >((result, group) => {
    const primaryLabel = group.labels[0]
    const existing = result.find((section) => section.key === primaryLabel.key)
    if (existing) existing.groups.push(group)
    else result.push({ key: primaryLabel.key, label: primaryLabel, groups: [group] })
    return result
  }, [])

  const toneFor = (key: string): string =>
    key.includes('priority-high')
      ? 'danger'
      : key.includes('priority-normal')
        ? 'warning'
        : key.includes('priority-low') || key.includes('state-open')
          ? 'info'
          : key.includes('deadline')
            ? 'orange'
            : key.includes('recurring')
              ? 'violet'
              : key.includes('completed')
                ? 'success'
                : key.startsWith('member-')
                  ? 'mauve'
                  : key.startsWith('list-')
                    ? 'cyan'
                    : 'neutral'

  const renderGroupLabel = (
    label: (typeof groups)[number]['labels'][number],
    primary = false,
  ) =>
    label.memberId ? (
      <span
        key={label.key}
        className={`board-avatar member board-avatar--${memberAvatarColor(label.memberId)} tasklist-history-member-avatar${primary ? ' tasklist-history-primary-member' : ''}`}
        title={label.label}
        aria-label={label.label}
      >
        {initialFor(label.label)}
      </span>
    ) : (
      <span
        key={label.key}
        className={`tasklist-history-day-label${primary ? ' tasklist-history-primary-label' : ''} tasklist-history-day-label--${toneFor(label.key)}${label.color ? ` tasklist-history-day-label--${label.color}` : ''}`}
      >
        {label.label}
      </span>
    )

  if (groups.length === 0) {
    return <p className="tasklist-history-empty">{emptyMessage}</p>
  }

  return (
    <ul className="tasklist-history-days">
      {sections.map((section) => (
        <li key={section.key} className="tasklist-history-day tasklist-history-section">
          <div className="tasklist-history-subgroups">
            {section.groups.map((group, groupIndex) => (
              <div key={group.key} className="tasklist-history-subgroup">
                {(groupIndex === 0 || group.labels.length > 1) && (
                  <div className="tasklist-history-group-labels">
                    {[
                      ...(groupIndex === 0 ? [section.label] : []),
                      ...group.labels.slice(1),
                    ].map((label, labelIndex) =>
                      renderGroupLabel(
                        label,
                        groupIndex === 0 && labelIndex === 0,
                      ),
                    )}
                  </div>
                )}
                <div className="tasklist-history-rows" role="list">
                  {group.tasks.map((task) => (
                    <TaskHistoryRow
                      key={task.wire.id}
                      task={task}
                      boardMembers={boardMembers}
                      selected={selectedTaskId === task.wire.id}
                      onSelect={() => onSelectTask(task.wire.id)}
                    />
                  ))}
                </div>
              </div>
            ))}
          </div>
        </li>
      ))}
    </ul>
  )
}

export const TaskHistoryDots = ({
  tasks,
  boardMembers,
  selectedTaskId,
  emptyMessage,
  onSelectTask,
}: {
  tasks: DecryptedTask[]
  boardMembers: BoardMember[]
  selectedTaskId?: Uuid
  emptyMessage: string
  onSelectTask(id: Uuid): void
}) => {
  const dayGroups = useMemo(() => groupTasksByHistoryDay(tasks), [tasks])

  if (dayGroups.length === 0) {
    return <p className="tasklist-history-empty">{emptyMessage}</p>
  }

  return (
    <ul className="tasklist-history-days">
      {dayGroups.map((group) => (
        <li key={group.key} className="tasklist-history-day">
          <span className="tasklist-history-day-label">{group.label}</span>
          <div className="tasklist-history-dots" role="list">
            {group.tasks.map((task) => (
              <TaskHistoryDot
                key={task.wire.id}
                task={task}
                boardMembers={boardMembers}
                selected={selectedTaskId === task.wire.id}
                onSelect={() => onSelectTask(task.wire.id)}
              />
            ))}
          </div>
        </li>
      ))}
    </ul>
  )
}

export const TaskListHistoryPanel = ({
  list,
  isEditing,
  editName,
  editColor,
  editIcon,
  iconPickerOpen,
  tintClassName,
  onEditNameChange,
  onStartEdit,
  onAutoSave,
  onCommitEdit,
  onToggleIconPicker,
  onLoadInfo,
  onCreateInfoDocument,
  onUpdateInfoDocument,
  onUploadInfoFile,
  onReadInfoFile,
  onDownloadInfoFile,
  onClose,
}: {
  list: TaskListItem
  isEditing: boolean
  editName: string
  editColor: TaskListColumnColor
  editIcon: TaskListIcon | undefined
  iconPickerOpen: boolean
  tintClassName?: string
  onEditNameChange(name: string): void
  onStartEdit(): void
  onAutoSave(): void
  onCommitEdit(): void
  onToggleIconPicker(): void
  onLoadInfo(list: TaskListItem): Promise<DecryptedInfoDocument[]>
  onCreateInfoDocument(
    list: TaskListItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUpdateInfoDocument(
    document: DecryptedInfoDocument,
    content: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUploadInfoFile(
    document: DecryptedInfoDocument,
    file: File,
  ): Promise<InfoFileBlock>
  onReadInfoFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<Blob>
  onDownloadInfoFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<void>
  onClose(): void
}) => {
  const renameInputRef = useRef<HTMLInputElement>(null)
  const editRegionRef = useRef<HTMLDivElement>(null)
  const autoSaveRef = useRef(onAutoSave)
  const listName = list.document?.name ?? 'Locked list'
  const displayName = isEditing ? editName || list.document?.name : list.document?.name
  const avatarInitial = list.document && displayName ? initialFor(displayName) : null
  const avatarColor = resolveTaskListIconColorFromStored(
    isEditing ? editColor : list.document?.color,
    list.wire.id,
  )
  const displayIcon = isEditing ? editIcon : list.document?.icon

  useEffect(() => {
    autoSaveRef.current = onAutoSave
  }, [onAutoSave])

  useEffect(() => {
    if (!isEditing) return
    renameInputRef.current?.focus()
    renameInputRef.current?.select()
  }, [isEditing])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        if (isEditing) onCommitEdit()
        else onClose()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [isEditing, onClose, onCommitEdit])

  useEffect(() => {
    if (!isEditing) return
    const timeoutId = window.setTimeout(() => autoSaveRef.current(), 450)
    return () => window.clearTimeout(timeoutId)
  }, [editColor, editIcon, editName, isEditing])

  useEffect(() => {
    if (!isEditing) return
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target
      if (!(target instanceof Node)) return
      if (editRegionRef.current?.contains(target)) return
      if (
        target instanceof Element &&
        target.closest('.task-list-icon-panel')
      ) {
        return
      }
      onCommitEdit()
    }
    document.addEventListener('pointerdown', handlePointerDown, true)
    return () =>
      document.removeEventListener('pointerdown', handlePointerDown, true)
  }, [isEditing, onCommitEdit])

  return (
    <section
      className={['board-overview', 'tasklist-history-panel', tintClassName]
        .filter(Boolean)
        .join(' ')}
      role="region"
      aria-label={`Info ${listName}`}
    >
      <div className="board-overview-document tasklist-history-panel-inner">
        <header className="board-detail-header tasklist-history-header">
          <div ref={editRegionRef} className="tasklist-history-identity">
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
                  '?'
                )}
              </span>
            )}
            <div className="tasklist-history-heading">
              {isEditing && list.document ? (
                <>
                  <input
                    ref={renameInputRef}
                    className="board-column-rename-input tasklist-history-rename-input"
                    value={editName}
                    aria-label="Modifica nome task list"
                    onChange={(event) => onEditNameChange(event.target.value)}
                  />
                </>
              ) : (
                <>
                  <h2>{listName}</h2>
                  {list.document && (
                    <button
                      type="button"
                      className="board-column-edit-trigger tasklist-history-more-trigger"
                      aria-label={`Modifica ${listName}`}
                      title="Modifica tasklist"
                      onClick={onStartEdit}
                    >
                      <svg viewBox="0 0 24 24" aria-hidden>
                        <circle cx="12" cy="5" r="2" />
                        <circle cx="12" cy="12" r="2" />
                        <circle cx="12" cy="19" r="2" />
                      </svg>
                    </button>
                  )}
                </>
              )}
            </div>
          </div>
        </header>

        <div className="board-overview-scroll tasklist-history-info-overview">
          <TaskListInfoPanel
            list={list}
            presentation="overview"
            overviewTitle=""
            showOverviewTitle={false}
            onLoad={onLoadInfo}
            onCreateDocument={onCreateInfoDocument}
            onUpdateDocument={onUpdateInfoDocument}
            onUploadFile={onUploadInfoFile}
            onReadFile={onReadInfoFile}
            onDownloadFile={onDownloadInfoFile}
          />
        </div>
      </div>
    </section>
  )
}
