import type { Uuid } from '../api/contracts'
import type {
  DecryptedTask,
  TaskCreationInput,
  TaskDocument,
  TaskFilter,
  TaskSelectedValueDocument,
} from './models'

export interface BoardListLike {
  wire: { id: Uuid; topic_id: Uuid }
  document?: { name: string }
}

export interface BoardTopicLike {
  wire: { id: Uuid }
  document?: { name: string }
}

export interface BuiltTaskCreation {
  taskKind: DecryptedTask['wire']['task_kind']
  document: TaskDocument
  selectedValue: TaskSelectedValueDocument
}

const validDate = (value: string): boolean =>
  value.length > 0 && Number.isFinite(new Date(value).getTime())

const withNotes = (
  document: TaskDocument,
  notes: string | undefined,
): TaskDocument => {
  const trimmed = notes?.trim()
  return trimmed ? { ...document, notes: trimmed } : document
}

export const buildTaskCreation = (
  input: TaskCreationInput,
): BuiltTaskCreation => {
  const title = input.title.trim()
  if (!title) throw new Error('Task title is required')

  if (input.taskKind === 'priority') {
    return {
      taskKind: 'priority',
      document: withNotes(
        { schema: 1, title, priority: input.priority },
        input.notes,
      ),
      selectedValue: { schema: 1, priority: input.priority },
    }
  }

  if (!validDate(input.dueAt)) {
    throw new Error('A valid date is required for this task type')
  }

  if (input.taskKind === 'deadline') {
    return {
      taskKind: 'deadline',
      document: withNotes(
        { schema: 1, title, due_at: input.dueAt },
        input.notes,
      ),
      selectedValue: { schema: 1, due_at: input.dueAt },
    }
  }

  if (!Number.isSafeInteger(input.interval) || input.interval < 1) {
    throw new Error('Recurrence interval must be a positive integer')
  }
  const recurrence = {
    frequency: input.frequency,
    interval: input.interval,
  }
  return {
    taskKind: 'recurring',
    document: withNotes(
      {
        schema: 1,
        title,
        due_at: input.dueAt,
        recurrence,
      },
      input.notes,
    ),
    selectedValue: {
      schema: 1,
      due_at: input.dueAt,
      recurrence,
    },
  }
}

const startOfDay = (value: Date): Date => {
  const date = new Date(value)
  date.setHours(0, 0, 0, 0)
  return date
}

const endOfDay = (value: Date): Date => {
  const date = new Date(value)
  date.setHours(23, 59, 59, 999)
  return date
}

export const isTaskOverdue = (
  task: DecryptedTask,
  now = new Date(),
): boolean =>
  task.wire.state.state === 'open' &&
  Boolean(task.document.due_at) &&
  new Date(task.document.due_at as string).getTime() < now.getTime()

export const isRecurringTaskOverdue = (
  task: DecryptedTask,
  now = new Date(),
): boolean =>
  Boolean(task.document.recurrence) && isTaskOverdue(task, now)

export type TaskStatusIndicatorVariant =
  | 'completed'
  | 'overdue'
  | 'due-today'
  | 'due-soon'
  | 'scheduled'
  | 'recurring'
  | 'priority-high'
  | 'priority-normal'
  | 'priority-low'
  | 'default'

export interface TaskStatusIndicator {
  variant: TaskStatusIndicatorVariant
  label: string
  /** 0 = start of due window, 1 = due or overdue. Omitted when not applicable. */
  dueProgress?: number
}

const MS_PER_DAY = 86_400_000
export const TASK_DUE_PROGRESS_WINDOW_MS = 7 * MS_PER_DAY
/** Completed tasks stay visible in open/date filters for this long. */
export const TASK_RECENTLY_COMPLETED_WINDOW_MS = MS_PER_DAY

export const getTaskCompletedAt = (task: DecryptedTask): Date | undefined => {
  const { state } = task.wire
  if (state.state !== 'completed') return undefined
  const completedAt = new Date(state.completed_at)
  return Number.isFinite(completedAt.getTime()) ? completedAt : undefined
}

/** True when the task was completed within the last 24 hours. */
export const isRecentlyCompleted = (
  task: DecryptedTask,
  now = new Date(),
): boolean => {
  const completedAt = getTaskCompletedAt(task)
  if (!completedAt) return false
  return (
    now.getTime() - completedAt.getTime() < TASK_RECENTLY_COMPLETED_WINDOW_MS
  )
}

/** Linear fill for the status ring between window start and due moment. */
export const getTaskDueProgress = (
  task: DecryptedTask,
  now = new Date(),
): number | undefined => {
  if (task.wire.state.state === 'completed') return undefined

  const dueAt = task.document.due_at
  if (!dueAt) return undefined

  const dueMs = new Date(dueAt).getTime()
  if (!Number.isFinite(dueMs)) return undefined

  const nowMs = now.getTime()
  if (nowMs >= dueMs) return 1

  const createdMs = new Date(task.wire.created_at).getTime()
  const windowStart = Number.isFinite(createdMs)
    ? Math.max(dueMs - TASK_DUE_PROGRESS_WINDOW_MS, createdMs)
    : dueMs - TASK_DUE_PROGRESS_WINDOW_MS

  if (nowMs <= windowStart) return 0

  const windowDuration = dueMs - windowStart
  if (windowDuration <= 0) return 1

  return Math.min(1, Math.max(0, (nowMs - windowStart) / windowDuration))
}

const withDueProgress = (
  indicator: Omit<TaskStatusIndicator, 'dueProgress'>,
  dueProgress: number | undefined,
): TaskStatusIndicator =>
  dueProgress === undefined ? indicator : { ...indicator, dueProgress }

/** Linear-style task dot: priority kind first, then deadline urgency. */
export const getTaskStatusIndicator = (
  task: DecryptedTask,
  now = new Date(),
): TaskStatusIndicator => {
  if (task.wire.state.state === 'completed') {
    return { variant: 'completed', label: 'Completata' }
  }

  const dueProgress = getTaskDueProgress(task, now)
  const dueAt = task.document.due_at
  const dueMs = dueAt ? new Date(dueAt).getTime() : undefined
  const todayStart = startOfDay(now).getTime()
  const todayEnd = endOfDay(now).getTime()
  const tomorrow = new Date(now)
  tomorrow.setDate(tomorrow.getDate() + 1)
  const tomorrowStart = startOfDay(tomorrow).getTime()
  const tomorrowEnd = endOfDay(tomorrow).getTime()

  if (task.wire.task_kind === 'priority') {
    const priority = task.document.priority ?? 'normal'
    if (priority === 'high') {
      return { variant: 'priority-high', label: 'Priorità alta' }
    }
    if (priority === 'low') {
      return { variant: 'priority-low', label: 'Priorità bassa' }
    }
    return { variant: 'priority-normal', label: 'Priorità normale' }
  }

  if (dueMs !== undefined && Number.isFinite(dueMs)) {
    if (dueMs < now.getTime()) {
      return withDueProgress({ variant: 'overdue', label: 'Scaduta' }, dueProgress)
    }
    if (dueMs >= todayStart && dueMs <= todayEnd) {
      return withDueProgress(
        { variant: 'due-today', label: 'In scadenza oggi' },
        dueProgress,
      )
    }
    if (dueMs >= tomorrowStart && dueMs <= tomorrowEnd) {
      return withDueProgress(
        { variant: 'due-soon', label: 'In scadenza domani' },
        dueProgress,
      )
    }
    return withDueProgress({ variant: 'scheduled', label: 'Con scadenza' }, dueProgress)
  }

  if (task.wire.task_kind === 'recurring' || task.document.recurrence) {
    return { variant: 'recurring', label: 'Ricorrente' }
  }

  return { variant: 'default', label: 'Aperta' }
}

export const buildNextRecurringTask = (
  task: DecryptedTask,
): {
  document: TaskDocument
  selectedValue: TaskSelectedValueDocument
  occurrenceNumber: number
} => {
  const recurrence = task.document.recurrence
  const dueAt = task.document.due_at
  const currentOccurrence = task.wire.occurrence_number
  if (
    task.wire.task_kind !== 'recurring' ||
    !task.wire.recurrence_series_id ||
    !recurrence ||
    !dueAt ||
    currentOccurrence === null
  ) {
    throw new Error('Task is not a materialized recurring occurrence')
  }
  if (!Number.isSafeInteger(recurrence.interval) || recurrence.interval < 1) {
    throw new Error('Recurring interval must be a positive integer')
  }
  const nextDueAt = new Date(dueAt)
  if (!Number.isFinite(nextDueAt.getTime())) {
    throw new Error('Recurring due date is invalid')
  }
  if (recurrence.frequency === 'minutes') {
    nextDueAt.setUTCMinutes(nextDueAt.getUTCMinutes() + recurrence.interval)
  } else if (recurrence.frequency === 'daily') {
    nextDueAt.setUTCDate(nextDueAt.getUTCDate() + recurrence.interval)
  } else if (recurrence.frequency === 'weekly') {
    nextDueAt.setUTCDate(nextDueAt.getUTCDate() + recurrence.interval * 7)
  } else {
    nextDueAt.setUTCMonth(nextDueAt.getUTCMonth() + recurrence.interval)
  }
  const nextDocument: TaskDocument = {
    ...task.document,
    due_at: nextDueAt.toISOString(),
    recurrence: { ...recurrence },
  }
  return {
    document: nextDocument,
    selectedValue: {
      schema: 1,
      due_at: nextDocument.due_at,
      recurrence: nextDocument.recurrence,
    },
    occurrenceNumber: currentOccurrence + 1,
  }
}

export const filterTasks = (
  tasks: DecryptedTask[],
  filter: TaskFilter,
  now = new Date(),
): DecryptedTask[] => {
  const todayStart = startOfDay(now).getTime()
  const todayEnd = endOfDay(now).getTime()

  return tasks.filter((task) => {
    const completed = task.wire.state.state === 'completed'

    if (filter === 'completed') {
      return completed
    }

    if (completed) {
      if (!isRecentlyCompleted(task, now)) {
        return false
      }
      if (filter === 'open') {
        return true
      }
      if (!task.document.due_at) {
        return false
      }
      const due = new Date(task.document.due_at).getTime()
      if (filter === 'today') {
        return due >= todayStart && due <= todayEnd
      }
      return due > todayEnd
    }

    if (filter === 'open') {
      return true
    }

    if (!task.document.due_at) {
      return false
    }

    const due = new Date(task.document.due_at).getTime()
    if (filter === 'today') {
      return due >= todayStart && due <= todayEnd
    }

    return due > todayEnd
  })
}

/** Task lists that contain at least one task assigned to the given identity. */
export const taskListsForMember = <T extends BoardListLike>(
  lists: T[],
  tasks: DecryptedTask[],
  identityId: Uuid,
): T[] => {
  const listIds = new Set(
    tasks
      .filter((task) => task.wire.active_assignee_identity_id === identityId)
      .map((task) => task.wire.list_id),
  )
  return lists.filter((list) => listIds.has(list.wire.id))
}

export const taskListsForTopic = <T extends BoardListLike>(
  lists: T[],
  topicId: Uuid,
): T[] => lists.filter((list) => list.wire.topic_id === topicId)

export const filterBoardSearch = <T extends BoardListLike>(
  lists: T[],
  tasks: DecryptedTask[],
  query: string,
): { lists: T[]; tasks: DecryptedTask[] } => {
  const normalized = query.trim().toLowerCase()
  if (!normalized) {
    return { lists, tasks }
  }

  const matchingTasks = tasks.filter((task) => {
    const title = task.document.title.toLowerCase()
    const notes = (task.document.notes ?? '').toLowerCase()
    return title.includes(normalized) || notes.includes(normalized)
  })
  const matchingTaskListIds = new Set(
    matchingTasks.map((task) => task.wire.list_id),
  )
  const matchingLists = lists.filter((list) => {
    const name = (list.document?.name ?? '').toLowerCase()
    return name.includes(normalized) || matchingTaskListIds.has(list.wire.id)
  })
  const visibleListIds = new Set(matchingLists.map((list) => list.wire.id))
  return {
    lists: matchingLists,
    tasks: matchingTasks.filter((task) => visibleListIds.has(task.wire.list_id)),
  }
}

/** Lower rank = more urgent. Empty / completed-only lists use 10. */
export const TASK_URGENCY_RANK: Record<TaskStatusIndicatorVariant, number> = {
  overdue: 1,
  'due-today': 2,
  'due-soon': 3,
  'priority-high': 4,
  scheduled: 5,
  'priority-normal': 6,
  recurring: 7,
  'priority-low': 8,
  default: 9,
  completed: 10,
}

export const taskUrgencyRank = (
  task: DecryptedTask,
  now = new Date(),
): number => TASK_URGENCY_RANK[getTaskStatusIndicator(task, now).variant]

/** Overdue, due today, or due tomorrow. */
export const isTaskDueUrgent = (
  task: DecryptedTask,
  now = new Date(),
): boolean => {
  if (task.wire.state.state !== 'open') return false
  const rank = taskUrgencyRank(task, now)
  return rank >= 1 && rank <= 3
}

export interface TopicUrgencyBadge {
  count: number
  hasOverdue: boolean
}

const emptyUrgencyBadge = (): TopicUrgencyBadge => ({
  count: 0,
  hasOverdue: false,
})

/** Urgent open-task counts for Generali and each topic. */
export const topicUrgencyBadges = (
  lists: BoardListLike[],
  tasks: DecryptedTask[],
  now = new Date(),
): { generali: TopicUrgencyBadge; byTopicId: Map<Uuid, TopicUrgencyBadge> } => {
  const listTopicById = new Map<Uuid, Uuid>()
  for (const list of lists) {
    listTopicById.set(list.wire.id, list.wire.topic_id)
  }

  const generali = emptyUrgencyBadge()
  const byTopicId = new Map<Uuid, TopicUrgencyBadge>()

  for (const task of tasks) {
    if (!isTaskDueUrgent(task, now)) continue
    const topicId = listTopicById.get(task.wire.list_id)
    if (!topicId) continue

    const overdue = taskUrgencyRank(task, now) === 1
    generali.count += 1
    if (overdue) generali.hasOverdue = true

    const current = byTopicId.get(topicId) ?? emptyUrgencyBadge()
    current.count += 1
    if (overdue) current.hasOverdue = true
    byTopicId.set(topicId, current)
  }

  return { generali, byTopicId }
}

/** Most urgent first: overdue beats due-soon; higher count wins within a tier. */
export const compareTopicUrgencyBadges = (
  left: TopicUrgencyBadge | undefined,
  right: TopicUrgencyBadge | undefined,
): number => {
  const leftBadge = left ?? emptyUrgencyBadge()
  const rightBadge = right ?? emptyUrgencyBadge()

  const leftUrgent = leftBadge.count > 0
  const rightUrgent = rightBadge.count > 0
  if (leftUrgent !== rightUrgent) {
    return leftUrgent ? -1 : 1
  }
  if (!leftUrgent) return 0

  if (leftBadge.hasOverdue !== rightBadge.hasOverdue) {
    return leftBadge.hasOverdue ? -1 : 1
  }

  if (leftBadge.count !== rightBadge.count) {
    return rightBadge.count - leftBadge.count
  }

  return 0
}

/** Sort topics left→right by sidebar urgency badge (stable tie-break). */
export const sortTopicsByUrgency = <T extends BoardTopicLike>(
  topics: T[],
  badgesByTopicId: Map<Uuid, TopicUrgencyBadge>,
): T[] => {
  const indexed = topics.map((topic, index) => ({
    topic,
    index,
    badge: badgesByTopicId.get(topic.wire.id),
  }))
  indexed.sort((left, right) => {
    const byUrgency = compareTopicUrgencyBadges(left.badge, right.badge)
    return byUrgency !== 0 ? byUrgency : left.index - right.index
  })
  return indexed.map((entry) => entry.topic)
}

interface ItemUrgencyStats {
  rank: number
  rankCount: number
  nearestDueMs: number
  name: string
}

const EMPTY_LIST_URGENCY_RANK = 10

const urgencyStatsForTasks = (
  tasks: DecryptedTask[],
  name: string,
  now: Date,
): ItemUrgencyStats => {
  const openTasks = tasks.filter((task) => task.wire.state.state === 'open')
  if (openTasks.length === 0) {
    return {
      rank: EMPTY_LIST_URGENCY_RANK,
      rankCount: 0,
      nearestDueMs: Number.POSITIVE_INFINITY,
      name,
    }
  }

  let rank = EMPTY_LIST_URGENCY_RANK
  for (const task of openTasks) {
    rank = Math.min(rank, taskUrgencyRank(task, now))
  }

  let rankCount = 0
  let nearestDueMs = Number.POSITIVE_INFINITY
  for (const task of openTasks) {
    if (taskUrgencyRank(task, now) === rank) {
      rankCount += 1
    }
    const dueAt = task.document.due_at
    if (!dueAt) continue
    const dueMs = new Date(dueAt).getTime()
    if (Number.isFinite(dueMs)) {
      nearestDueMs = Math.min(nearestDueMs, dueMs)
    }
  }

  return { rank, rankCount, nearestDueMs, name }
}

const compareUrgencyStats = (
  left: ItemUrgencyStats,
  right: ItemUrgencyStats,
): number => {
  if (left.rank !== right.rank) return left.rank - right.rank
  if (left.rankCount !== right.rankCount) return right.rankCount - left.rankCount
  if (left.nearestDueMs !== right.nearestDueMs) {
    return left.nearestDueMs - right.nearestDueMs
  }
  return left.name.localeCompare(right.name, 'it', { sensitivity: 'base' })
}

/** Sort board columns / items left→right by max open-task urgency. */
export const sortItemsByTaskUrgency = <T>(
  items: T[],
  tasksForItem: (item: T) => DecryptedTask[],
  itemName: (item: T) => string,
  now = new Date(),
): T[] => {
  const ranked = items.map((item, index) => ({
    item,
    index,
    stats: urgencyStatsForTasks(tasksForItem(item), itemName(item), now),
  }))
  ranked.sort((left, right) => {
    const byUrgency = compareUrgencyStats(left.stats, right.stats)
    return byUrgency !== 0 ? byUrgency : left.index - right.index
  })
  return ranked.map((entry) => entry.item)
}

/** Sort task lists by urgency of their open tasks (lowest rank first). */
export const sortTaskListsByUrgency = <T extends BoardListLike>(
  lists: T[],
  tasks: DecryptedTask[],
  now = new Date(),
): T[] => {
  const tasksByListId = new Map<Uuid, DecryptedTask[]>()
  for (const task of tasks) {
    const bucket = tasksByListId.get(task.wire.list_id)
    if (bucket) {
      bucket.push(task)
    } else {
      tasksByListId.set(task.wire.list_id, [task])
    }
  }
  return sortItemsByTaskUrgency(
    lists,
    (list) => tasksByListId.get(list.wire.id) ?? [],
    (list) => list.document?.name ?? '',
    now,
  )
}

export const formatTaskCardDueDate = (dueAt: string): string =>
  new Intl.DateTimeFormat('it-IT', {
    day: 'numeric',
    month: 'short',
  }).format(new Date(dueAt))

/** Day used to place a task in list history (completed → due → created). */
export const getTaskHistoryDay = (
  task: DecryptedTask,
): Date => {
  const completedAt = getTaskCompletedAt(task)
  if (completedAt) return startOfDay(completedAt)

  const dueAt = task.document.due_at
  if (dueAt) {
    const due = new Date(dueAt)
    if (Number.isFinite(due.getTime())) return startOfDay(due)
  }

  const created = new Date(task.wire.created_at)
  return startOfDay(
    Number.isFinite(created.getTime()) ? created : new Date(),
  )
}

const historyDayKey = (day: Date): string => {
  const year = day.getFullYear()
  const month = String(day.getMonth() + 1).padStart(2, '0')
  const date = String(day.getDate()).padStart(2, '0')
  return `${year}-${month}-${date}`
}

export const formatTaskHistoryDayLabel = (
  day: Date,
  now = new Date(),
): string => {
  const dayStart = startOfDay(day).getTime()
  const todayStart = startOfDay(now).getTime()
  const dayDiff = Math.round((dayStart - todayStart) / MS_PER_DAY)
  if (dayDiff === 0) return 'Oggi'
  if (dayDiff === -1) return 'Ieri'
  if (dayDiff === 1) return 'Domani'
  return new Intl.DateTimeFormat('it-IT', {
    weekday: 'short',
    day: 'numeric',
    month: 'short',
    year: day.getFullYear() !== now.getFullYear() ? 'numeric' : undefined,
  }).format(day)
}

export interface TaskHistoryDayGroup {
  key: string
  day: Date
  label: string
  tasks: DecryptedTask[]
}

/** Group list tasks by history day, newest days first. */
export const groupTasksByHistoryDay = (
  tasks: DecryptedTask[],
  now = new Date(),
): TaskHistoryDayGroup[] => {
  const byKey = new Map<string, { day: Date; tasks: DecryptedTask[] }>()
  for (const task of tasks) {
    const day = getTaskHistoryDay(task)
    const key = historyDayKey(day)
    const bucket = byKey.get(key)
    if (bucket) {
      bucket.tasks.push(task)
    } else {
      byKey.set(key, { day, tasks: [task] })
    }
  }

  return [...byKey.entries()]
    .sort((left, right) => right[1].day.getTime() - left[1].day.getTime())
    .map(([key, group]) => {
      const sortedTasks = [...group.tasks].sort((left, right) => {
        const leftRank = taskUrgencyRank(left, now)
        const rightRank = taskUrgencyRank(right, now)
        if (leftRank !== rightRank) return leftRank - rightRank
        return left.document.title.localeCompare(right.document.title, 'it', {
          sensitivity: 'base',
        })
      })
      return {
        key,
        day: group.day,
        label: formatTaskHistoryDayLabel(group.day, now),
        tasks: sortedTasks,
      }
    })
}

export const formatDueDate = (dueAt: string, now = new Date()): string => {
  const due = new Date(dueAt)
  const dueStart = startOfDay(due).getTime()
  const nowStart = startOfDay(now).getTime()
  const dayDifference = Math.round((dueStart - nowStart) / 86_400_000)
  const time = new Intl.DateTimeFormat(undefined, {
    hour: 'numeric',
    minute: '2-digit',
  }).format(due)

  if (dayDifference === 0) {
    return `Today, ${time}`
  }
  if (dayDifference === 1) {
    return `Tomorrow, ${time}`
  }
  if (dayDifference === -1) {
    return `Yesterday, ${time}`
  }

  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  }).format(due)
}
