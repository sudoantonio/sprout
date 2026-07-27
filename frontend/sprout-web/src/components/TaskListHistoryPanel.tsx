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
} from '../domain/models'
import type { BoardMember, TaskListItem } from '../store/app-store'
import { CheckIcon, PencilIcon, XIcon } from './icons'
import { TaskListAvatarContent } from './TaskListAvatarContent'

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
  const hideTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>()
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

export const TaskListHistoryPanel = ({
  list,
  tasks,
  boardMembers,
  selectedTaskId,
  isEditing,
  editName,
  editColor,
  editIcon,
  iconPickerOpen,
  tintClassName,
  onEditNameChange,
  onStartEdit,
  onCancelEdit,
  onCommitEdit,
  onToggleIconPicker,
  onSelectTask,
  onClose,
}: {
  list: TaskListItem
  tasks: DecryptedTask[]
  boardMembers: BoardMember[]
  selectedTaskId?: Uuid
  isEditing: boolean
  editName: string
  editColor: TaskListColumnColor
  editIcon: TaskListIcon | undefined
  iconPickerOpen: boolean
  tintClassName?: string
  onEditNameChange(name: string): void
  onStartEdit(): void
  onCancelEdit(): void
  onCommitEdit(): void
  onToggleIconPicker(): void
  onSelectTask(id: Uuid): void
  onClose(): void
}) => {
  const renameInputRef = useRef<HTMLInputElement>(null)
  const listTasks = useMemo(
    () => tasks.filter((task) => task.wire.list_id === list.wire.id),
    [list.wire.id, tasks],
  )
  const dayGroups = useMemo(
    () => groupTasksByHistoryDay(listTasks),
    [listTasks],
  )
  const listName = list.document?.name ?? 'Locked list'
  const displayName = isEditing ? editName || list.document?.name : list.document?.name
  const avatarInitial = list.document && displayName ? initialFor(displayName) : null
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

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        if (isEditing) onCancelEdit()
        else onClose()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [isEditing, onCancelEdit, onClose])

  return (
    <section
      className={['tasklist-history-panel', 'board-column', tintClassName]
        .filter(Boolean)
        .join(' ')}
      role="region"
      aria-label={`Storico ${listName}`}
    >
      <div className="tasklist-history-panel-inner">
        <header className="board-detail-header tasklist-history-header">
          <div className="tasklist-history-identity">
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
                <input
                  ref={renameInputRef}
                  className="board-column-rename-input tasklist-history-rename-input"
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
                <h2>{listName}</h2>
              )}
            </div>
          </div>
          <div className="tasklist-history-header-actions">
            {list.document && !isEditing && (
              <button
                type="button"
                className="board-column-edit-trigger"
                aria-label={`Modifica ${listName}`}
                title="Modifica tasklist"
                onClick={onStartEdit}
              >
                <PencilIcon className="board-column-action-icon" />
              </button>
            )}
            {isEditing && list.document && (
              <button
                type="button"
                className="board-column-edit-confirm"
                aria-label="Conferma modifiche task list"
                onClick={onCommitEdit}
              >
                <CheckIcon className="board-column-action-icon" />
              </button>
            )}
            <button
              type="button"
              className="board-detail-close"
              aria-label="Chiudi storico tasklist"
              onClick={onClose}
            >
              <XIcon aria-hidden />
            </button>
          </div>
        </header>

        {dayGroups.length === 0 ? (
          <p className="tasklist-history-empty">
            Non ci sono ancora task assegnati a questa tasklist.
          </p>
        ) : (
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
        )}
      </div>
    </section>
  )
}
