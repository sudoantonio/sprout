import type {
  DecryptedTask,
  TaskCreationInput,
  TaskDocument,
  TaskFilter,
  TaskSelectedValueDocument,
} from './models'

export interface BuiltTaskCreation {
  taskKind: DecryptedTask['wire']['task_kind']
  document: TaskDocument
  selectedValue: TaskSelectedValueDocument
}

const validDate = (value: string): boolean =>
  value.length > 0 && Number.isFinite(new Date(value).getTime())

export const buildTaskCreation = (
  input: TaskCreationInput,
): BuiltTaskCreation => {
  const title = input.title.trim()
  if (!title) throw new Error('Task title is required')

  if (input.taskKind === 'priority') {
    return {
      taskKind: 'priority',
      document: { schema: 1, title, priority: input.priority },
      selectedValue: { schema: 1, priority: input.priority },
    }
  }

  if (!validDate(input.dueAt)) {
    throw new Error('A valid date is required for this task type')
  }

  if (input.taskKind === 'deadline') {
    return {
      taskKind: 'deadline',
      document: { schema: 1, title, due_at: input.dueAt },
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
    document: {
      schema: 1,
      title,
      due_at: input.dueAt,
      recurrence,
    },
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
  if (recurrence.frequency === 'daily') {
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
    if (filter === 'completed') {
      return task.wire.state.state === 'completed'
    }

    if (task.wire.state.state === 'completed') {
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
