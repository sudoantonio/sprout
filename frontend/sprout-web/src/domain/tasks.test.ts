import { describe, expect, it } from 'vitest'
import type { TaskDto } from '../api/contracts'
import type { DecryptedTask } from './models'
import {
  buildNextRecurringTask,
  buildTaskCreation,
  filterBoardSearch,
  filterTasks,
  formatTaskCardDueDate,
  getTaskCompletedAt,
  getTaskStatusIndicator,
  getTaskDueProgress,
  isRecentlyCompleted,
  isRecurringTaskOverdue,
  groupTasksByHistoryDay,
  partitionDuplicatePresetTasks,
  sortTaskListsByUrgency,
  sortTopicsByUrgency,
  TASK_RECENTLY_COMPLETED_WINDOW_MS,
  taskListsForMember,
  taskUrgencyRank,
  topicUrgencyBadges,
} from './tasks'

const now = new Date('2026-07-18T12:00:00.000Z')

const completedState = (): TaskDto['state'] => ({
  state: 'completed',
  completed_by: crypto.randomUUID(),
  completed_at: now.toISOString(),
})

const wire = (state: TaskDto['state']): TaskDto => ({
  id: crypto.randomUUID(),
  project_id: crypto.randomUUID(),
  list_id: crypto.randomUUID(),
  resource_node_id: crypto.randomUUID(),
  task_kind: 'deadline',
  payload: {
    version: 1,
    algorithm: 'sprout-protocol-v1',
    key_id: crypto.randomUUID(),
    nonce_b64: 'AQ==',
    ciphertext_b64: 'Ag==',
  },
  selected_value_snapshot: {
    version: 1,
    algorithm: 'sprout-protocol-v1',
    key_id: crypto.randomUUID(),
    nonce_b64: 'Aw==',
    ciphertext_b64: 'BA==',
  },
  state,
  source_pretask_id: null,
  preset_assignment_id: null,
  copied_from_task_id: null,
  questionnaire_version_id: null,
  recurrence_series_id: null,
  occurrence_number: null,
  active_assignment_id: null,
  active_assignee_identity_id: null,
  created_at: now.toISOString(),
  payload_version: 1,
  key_epoch: 1,
})

const task = (
  state: TaskDto['state'],
  dueAt?: string,
  recurring = false,
): DecryptedTask => ({
  wire: wire(state),
  document: {
    schema: 1,
    title: crypto.randomUUID(),
    priority: 'normal',
    due_at: dueAt,
    recurrence: recurring
      ? { frequency: 'daily', interval: 1 }
      : undefined,
  },
})

describe('board focus helpers', () => {
  it('returns only lists that contain tasks for the member', () => {
    const memberId = crypto.randomUUID()
    const listA = { wire: { id: crypto.randomUUID(), topic_id: crypto.randomUUID() } }
    const listB = { wire: { id: crypto.randomUUID(), topic_id: crypto.randomUUID() } }
    const assigned = task({ state: 'open' })
    assigned.wire.list_id = listA.wire.id
    assigned.wire.active_assignee_identity_id = memberId
    const other = task({ state: 'open' })
    other.wire.list_id = listB.wire.id
    other.wire.active_assignee_identity_id = crypto.randomUUID()

    expect(taskListsForMember([listA, listB], [assigned, other], memberId)).toEqual([
      listA,
    ])
  })

  it('filters lists and tasks by search query', () => {
    const listMatch = {
      wire: { id: crypto.randomUUID(), topic_id: crypto.randomUUID() },
      document: { name: 'Mattina' },
    }
    const listOther = {
      wire: { id: crypto.randomUUID(), topic_id: crypto.randomUUID() },
      document: { name: 'Notte' },
    }
    const titled = task({ state: 'open' })
    titled.wire.list_id = listOther.wire.id
    titled.document.title = 'Controllo irrigazione'
    const unrelated = task({ state: 'open' })
    unrelated.wire.list_id = listOther.wire.id
    unrelated.document.title = 'Altro'

    const byList = filterBoardSearch([listMatch, listOther], [titled, unrelated], 'matt')
    expect(byList.lists).toEqual([listMatch])
    expect(byList.tasks).toEqual([])

    const byTask = filterBoardSearch(
      [listMatch, listOther],
      [titled, unrelated],
      'irrigazione',
    )
    expect(byTask.lists).toEqual([listOther])
    expect(byTask.tasks).toEqual([titled])
  })
})

describe('local task filters', () => {
  it('flags an overdue recurring task without advancing it', () => {
    const recurring = task(
      { state: 'open' },
      '2026-07-17T09:00:00.000Z',
      true,
    )
    expect(isRecurringTaskOverdue(recurring, now)).toBe(true)
    expect(recurring.document.due_at).toBe('2026-07-17T09:00:00.000Z')
  })

  it('separates open and completed ciphertext-backed records', () => {
    const open = task({ state: 'open' })
    const completed = task({
      state: 'completed',
      completed_by: crypto.randomUUID(),
      completed_at: now.toISOString(),
    })
    expect(filterTasks([open, completed], 'open', now)).toEqual([open, completed])
    expect(filterTasks([open, completed], 'completed', now)).toEqual([
      completed,
    ])
  })

  it('keeps recently completed tasks visible in non-completed filters for one day', () => {
    const open = task({ state: 'open' })
    const recent = task({
      state: 'completed',
      completed_by: crypto.randomUUID(),
      completed_at: now.toISOString(),
    })
    recent.document.due_at = '2026-07-18T18:00:00.000Z'
    const stale = task({
      state: 'completed',
      completed_by: crypto.randomUUID(),
      completed_at: new Date(
        now.getTime() - TASK_RECENTLY_COMPLETED_WINDOW_MS,
      ).toISOString(),
    })
    stale.document.due_at = '2026-07-25T09:00:00.000Z'
    const recentUpcoming = task({
      state: 'completed',
      completed_by: crypto.randomUUID(),
      completed_at: now.toISOString(),
    })
    recentUpcoming.document.due_at = '2026-07-25T09:00:00.000Z'

    expect(isRecentlyCompleted(recent, now)).toBe(true)
    expect(isRecentlyCompleted(stale, now)).toBe(false)
    expect(getTaskCompletedAt(open)).toBeUndefined()
    expect(getTaskCompletedAt(recent)).toEqual(new Date(now.toISOString()))

    expect(filterTasks([open, recent, stale], 'open', now)).toEqual([
      open,
      recent,
    ])
    expect(filterTasks([open, recent, stale], 'today', now)).toEqual([recent])
    expect(filterTasks([open, recentUpcoming, stale], 'upcoming', now)).toEqual([
      recentUpcoming,
    ])
  })
})

describe('task creation semantics', () => {
  it('keeps priority, deadline, and recurring values mutually exclusive', () => {
    expect(
      buildTaskCreation({
        taskKind: 'priority',
        title: ' Priority task ',
        priority: 'high',
      }),
    ).toEqual({
      taskKind: 'priority',
      document: { schema: 1, title: 'Priority task', priority: 'high' },
      selectedValue: { schema: 1, priority: 'high' },
    })

    const dueAt = '2026-07-19T09:00:00.000Z'
    expect(
      buildTaskCreation({
        taskKind: 'deadline',
        title: 'Deadline task',
        dueAt,
      }),
    ).toEqual({
      taskKind: 'deadline',
      document: { schema: 1, title: 'Deadline task', due_at: dueAt },
      selectedValue: { schema: 1, due_at: dueAt },
    })

    expect(
      buildTaskCreation({
        taskKind: 'recurring',
        title: 'Recurring task',
        dueAt,
        frequency: 'weekly',
        interval: 2,
      }),
    ).toEqual({
      taskKind: 'recurring',
      document: {
        schema: 1,
        title: 'Recurring task',
        due_at: dueAt,
        recurrence: { frequency: 'weekly', interval: 2 },
      },
      selectedValue: {
        schema: 1,
        due_at: dueAt,
        recurrence: { frequency: 'weekly', interval: 2 },
      },
    })
  })

  it('includes optional notes on created documents', () => {
    expect(
      buildTaskCreation({
        taskKind: 'priority',
        title: 'With notes',
        priority: 'normal',
        notes: '  Primo commento  ',
      }),
    ).toEqual({
      taskKind: 'priority',
      document: {
        schema: 1,
        title: 'With notes',
        priority: 'normal',
        notes: 'Primo commento',
      },
      selectedValue: { schema: 1, priority: 'normal' },
    })
  })

  it('keeps the assigned preset association in the encrypted task document', () => {
    const presetId = crypto.randomUUID()
    expect(
      buildTaskCreation({
        taskKind: 'priority',
        title: 'Task del preset',
        priority: 'normal',
        presetId,
        presetTemplateIndex: 0,
      }).document,
    ).toEqual({
      schema: 1,
      title: 'Task del preset',
      priority: 'normal',
      preset_id: presetId,
      preset_template_index: 0,
    })
  })

  it('keeps the oldest concrete task for each preset template slot', () => {
    const presetId = crypto.randomUUID()
    const listId = crypto.randomUUID()
    const original = task({ state: 'open' })
    original.wire.list_id = listId
    original.wire.created_at = '2026-09-03T10:00:00.000Z'
    original.document.preset_id = presetId
    original.document.preset_template_index = 0
    const duplicate = task({ state: 'open' })
    duplicate.wire.list_id = listId
    duplicate.wire.created_at = '2026-09-03T10:01:00.000Z'
    duplicate.document.preset_id = presetId
    duplicate.document.preset_template_index = 0
    const manual = task({ state: 'open' })
    manual.wire.list_id = listId
    manual.document.preset_id = presetId

    expect(
      partitionDuplicatePresetTasks([duplicate, manual, original]),
    ).toEqual({
      tasks: [manual, original],
      duplicates: [duplicate],
    })
  })

  it('rejects missing dates and invalid recurrence intervals', () => {
    expect(() =>
      buildTaskCreation({
        taskKind: 'deadline',
        title: 'No deadline',
        dueAt: '',
      }),
    ).toThrow(/valid date/i)
    expect(() =>
      buildTaskCreation({
        taskKind: 'recurring',
        title: 'Invalid recurrence',
        dueAt: '2026-07-19T09:00:00.000Z',
        frequency: 'daily',
        interval: 0,
      }),
    ).toThrow(/positive integer/i)
  })
})

describe('recurring completion semantics', () => {
  it.each([
    ['minutes', 30, '2026-07-18T09:30:00.000Z'],
    ['daily', 2, '2026-07-20T09:00:00.000Z'],
    ['weekly', 2, '2026-08-01T09:00:00.000Z'],
    ['monthly', 1, '2026-08-18T09:00:00.000Z'],
  ] as const)(
    'advances %s recurrence without mutating the completed occurrence',
    (frequency, interval, expectedDueAt) => {
      const current = task(
        { state: 'open' },
        '2026-07-18T09:00:00.000Z',
        true,
      )
      current.wire.task_kind = 'recurring'
      current.wire.recurrence_series_id = crypto.randomUUID()
      current.wire.occurrence_number = 4
      current.document.recurrence = { frequency, interval }

      const next = buildNextRecurringTask(current)

      expect(next.occurrenceNumber).toBe(5)
      expect(next.document.due_at).toBe(expectedDueAt)
      expect(next.selectedValue).toEqual({
        schema: 1,
        due_at: expectedDueAt,
        recurrence: { frequency, interval },
      })
      expect(current.document.due_at).toBe('2026-07-18T09:00:00.000Z')
    },
  )

  it('rejects a task without a materialized series occurrence', () => {
    expect(() =>
      buildNextRecurringTask(task({ state: 'open' }, now.toISOString(), true)),
    ).toThrow(/not a materialized recurring occurrence/i)
  })
})

describe('getTaskStatusIndicator', () => {
  it('marks completed tasks', () => {
    const completed = task(completedState())
    expect(getTaskStatusIndicator(completed, now)).toEqual({
      variant: 'completed',
      label: 'Completata',
    })
  })

  it('uses priority colors for priority tasks even with a due date', () => {
    const overdue = task({ state: 'open' }, '2026-07-17T09:00:00.000Z')
    overdue.wire.task_kind = 'priority'
    overdue.document.priority = 'high'
    expect(getTaskStatusIndicator(overdue, now).variant).toBe('priority-high')

    const dueSoon = task({ state: 'open' }, '2026-07-22T09:00:00.000Z')
    dueSoon.wire.task_kind = 'priority'
    dueSoon.document.priority = 'normal'
    expect(getTaskStatusIndicator(dueSoon, now).variant).toBe('priority-normal')
  })

  it('prioritizes overdue due dates for non-priority tasks', () => {
    const overdue = task({ state: 'open' }, '2026-07-17T09:00:00.000Z')
    overdue.wire.task_kind = 'deadline'
    expect(getTaskStatusIndicator(overdue, now).variant).toBe('overdue')
  })

  it('marks due today, due tomorrow, and later scheduled tasks', () => {
    const dueToday = task({ state: 'open' }, '2026-07-18T18:00:00.000Z')
    expect(getTaskStatusIndicator(dueToday, now).variant).toBe('due-today')

    const dueTomorrow = task({ state: 'open' }, '2026-07-19T09:00:00.000Z')
    expect(getTaskStatusIndicator(dueTomorrow, now).variant).toBe('due-soon')

    const dueLater = task({ state: 'open' }, '2026-07-22T09:00:00.000Z')
    expect(getTaskStatusIndicator(dueLater, now).variant).toBe('scheduled')
  })

  it('uses priority colors when there is no due date', () => {
    const high = task({ state: 'open' })
    high.wire.task_kind = 'priority'
    high.document.priority = 'high'
    expect(getTaskStatusIndicator(high, now).variant).toBe('priority-high')

    const normal = task({ state: 'open' })
    normal.wire.task_kind = 'priority'
    normal.document.priority = 'normal'
    expect(getTaskStatusIndicator(normal, now).variant).toBe('priority-normal')

    const low = task({ state: 'open' })
    low.wire.task_kind = 'priority'
    low.document.priority = 'low'
    expect(getTaskStatusIndicator(low, now).variant).toBe('priority-low')
  })

  it('marks scheduled and recurring tasks', () => {
    const scheduled = task({ state: 'open' }, '2026-08-01T09:00:00.000Z')
    scheduled.wire.task_kind = 'deadline'
    expect(getTaskStatusIndicator(scheduled, now).variant).toBe('scheduled')

    const recurringDueLater = task(
      { state: 'open' },
      '2026-08-01T09:00:00.000Z',
      true,
    )
    recurringDueLater.wire.task_kind = 'recurring'
    expect(getTaskStatusIndicator(recurringDueLater, now).variant).toBe(
      'scheduled',
    )

    const recurring = task({ state: 'open' }, undefined, true)
    recurring.wire.task_kind = 'recurring'
    expect(getTaskStatusIndicator(recurring, now).variant).toBe('recurring')
  })
})

describe('getTaskDueProgress', () => {
  it('returns undefined without a due date or when completed', () => {
    expect(getTaskDueProgress(task({ state: 'open' }), now)).toBeUndefined()

    const priority = task({ state: 'open' })
    priority.wire.task_kind = 'priority'
    delete priority.document.due_at
    expect(getTaskDueProgress(priority, now)).toBeUndefined()

    expect(
      getTaskDueProgress(task(completedState(), now.toISOString()), now),
    ).toBeUndefined()
  })

  it('returns 1 when overdue or at due moment', () => {
    expect(getTaskDueProgress(task({ state: 'open' }, '2026-07-17T09:00:00.000Z'), now)).toBe(1)
    expect(getTaskDueProgress(task({ state: 'open' }, now.toISOString()), now)).toBe(1)
  })

  it('returns 0 before the progress window opens', () => {
    const farFuture = task({ state: 'open' }, '2026-08-01T09:00:00.000Z')
    expect(getTaskDueProgress(farFuture, now)).toBe(0)
  })

  it('interpolates linearly across the seven-day window', () => {
    const dueSoon = task({ state: 'open' }, '2026-07-22T09:00:00.000Z')
    dueSoon.wire.created_at = '2026-07-01T00:00:00.000Z'
    expect(getTaskDueProgress(dueSoon, now)).toBeCloseTo(3.125 / 7, 5)

    const dueToday = task({ state: 'open' }, '2026-07-18T18:00:00.000Z')
    dueToday.wire.created_at = '2026-07-01T00:00:00.000Z'
    expect(getTaskDueProgress(dueToday, now)).toBeCloseTo(6.75 / 7, 5)
  })

  it('uses creation time when the task is younger than seven days', () => {
    const youngTask = task({ state: 'open' }, '2026-07-20T12:00:00.000Z')
    youngTask.wire.created_at = '2026-07-16T12:00:00.000Z'
    expect(getTaskDueProgress(youngTask, now)).toBeCloseTo(0.5, 5)
  })
})

describe('formatTaskCardDueDate', () => {
  it('formats day and short month in Italian', () => {
    expect(formatTaskCardDueDate('2026-07-23T09:00:00.000Z')).toBe('23 lug')
    expect(formatTaskCardDueDate('2026-08-22T09:00:00.000Z')).toBe('22 ago')
  })
})

describe('task list urgency ordering', () => {
  const list = (name: string) => ({
    wire: { id: crypto.randomUUID(), topic_id: crypto.randomUUID() },
    document: { name },
  })

  it('maps status variants onto the urgency hierarchy', () => {
    const overdue = task({ state: 'open' }, '2026-07-17T09:00:00.000Z')
    overdue.wire.task_kind = 'deadline'
    expect(taskUrgencyRank(overdue, now)).toBe(1)

    const dueToday = task({ state: 'open' }, '2026-07-18T18:00:00.000Z')
    expect(taskUrgencyRank(dueToday, now)).toBe(2)

    const dueTomorrow = task({ state: 'open' }, '2026-07-19T09:00:00.000Z')
    expect(taskUrgencyRank(dueTomorrow, now)).toBe(3)

    const high = task({ state: 'open' })
    high.wire.task_kind = 'priority'
    high.document.priority = 'high'
    expect(taskUrgencyRank(high, now)).toBe(4)

    const scheduled = task({ state: 'open' }, '2026-07-22T09:00:00.000Z')
    expect(taskUrgencyRank(scheduled, now)).toBe(5)

    const normal = task({ state: 'open' })
    normal.wire.task_kind = 'priority'
    normal.document.priority = 'normal'
    expect(taskUrgencyRank(normal, now)).toBe(6)

    const recurring = task({ state: 'open' }, undefined, true)
    recurring.wire.task_kind = 'recurring'
    expect(taskUrgencyRank(recurring, now)).toBe(7)

    const low = task({ state: 'open' })
    low.wire.task_kind = 'priority'
    low.document.priority = 'low'
    expect(taskUrgencyRank(low, now)).toBe(8)

    expect(taskUrgencyRank(task({ state: 'open' }), now)).toBe(9)

    const completed = task({
      state: 'completed',
      completed_by: crypto.randomUUID(),
      completed_at: now.toISOString(),
    })
    expect(taskUrgencyRank(completed, now)).toBe(10)
  })

  it('orders lists left-to-right by most urgent open task', () => {
    const empty = list('Vuota')
    const plain = list('Aperte')
    const high = list('Alta')
    const overdue = list('Scadute')
    const today = list('Oggi')

    const plainTask = task({ state: 'open' })
    plainTask.wire.list_id = plain.wire.id

    const highTask = task({ state: 'open' })
    highTask.wire.list_id = high.wire.id
    highTask.wire.task_kind = 'priority'
    highTask.document.priority = 'high'

    const overdueTask = task({ state: 'open' }, '2026-07-17T09:00:00.000Z')
    overdueTask.wire.list_id = overdue.wire.id
    overdueTask.wire.task_kind = 'deadline'

    const todayTask = task({ state: 'open' }, '2026-07-18T18:00:00.000Z')
    todayTask.wire.list_id = today.wire.id

    const ordered = sortTaskListsByUrgency(
      [empty, plain, high, overdue, today],
      [plainTask, highTask, overdueTask, todayTask],
      now,
    )

    expect(ordered.map((item) => item.document?.name)).toEqual([
      'Scadute',
      'Oggi',
      'Alta',
      'Aperte',
      'Vuota',
    ])
  })

  it('breaks ties by urgent-task count, nearest due date, then name', () => {
    const fewOverdue = list('Beta')
    const manyOverdue = list('Alpha')
    const nearerScheduled = list('Vicino')
    const fartherScheduled = list('Lontano')

    const overdueA = task({ state: 'open' }, '2026-07-17T09:00:00.000Z')
    overdueA.wire.list_id = fewOverdue.wire.id
    overdueA.wire.task_kind = 'deadline'

    const overdueB1 = task({ state: 'open' }, '2026-07-16T09:00:00.000Z')
    overdueB1.wire.list_id = manyOverdue.wire.id
    overdueB1.wire.task_kind = 'deadline'
    const overdueB2 = task({ state: 'open' }, '2026-07-15T09:00:00.000Z')
    overdueB2.wire.list_id = manyOverdue.wire.id
    overdueB2.wire.task_kind = 'deadline'

    const near = task({ state: 'open' }, '2026-07-22T09:00:00.000Z')
    near.wire.list_id = nearerScheduled.wire.id
    const far = task({ state: 'open' }, '2026-08-01T09:00:00.000Z')
    far.wire.list_id = fartherScheduled.wire.id

    expect(
      sortTaskListsByUrgency(
        [fewOverdue, manyOverdue],
        [overdueA, overdueB1, overdueB2],
        now,
      ).map((item) => item.document?.name),
    ).toEqual(['Alpha', 'Beta'])

    expect(
      sortTaskListsByUrgency(
        [fartherScheduled, nearerScheduled],
        [far, near],
        now,
      ).map((item) => item.document?.name),
    ).toEqual(['Vicino', 'Lontano'])
  })

  it('counts overdue and due-soon tasks per topic for sidebar badges', () => {
    const topicA = crypto.randomUUID()
    const topicB = crypto.randomUUID()
    const listA = {
      wire: { id: crypto.randomUUID(), topic_id: topicA },
      document: { name: 'A' },
    }
    const listB = {
      wire: { id: crypto.randomUUID(), topic_id: topicB },
      document: { name: 'B' },
    }

    const overdue = task({ state: 'open' }, '2026-07-17T09:00:00.000Z')
    overdue.wire.list_id = listA.wire.id
    overdue.wire.task_kind = 'deadline'

    const dueToday = task({ state: 'open' }, '2026-07-18T18:00:00.000Z')
    dueToday.wire.list_id = listA.wire.id

    const dueSoon = task({ state: 'open' }, '2026-07-19T09:00:00.000Z')
    dueSoon.wire.list_id = listB.wire.id

    const later = task({ state: 'open' }, '2026-07-25T09:00:00.000Z')
    later.wire.list_id = listB.wire.id

    const badges = topicUrgencyBadges([listA, listB], [overdue, dueToday, dueSoon, later], now)

    expect(badges.generali).toEqual({ count: 3, hasOverdue: true })
    expect(badges.byTopicId.get(topicA)).toEqual({ count: 2, hasOverdue: true })
    expect(badges.byTopicId.get(topicB)).toEqual({ count: 1, hasOverdue: false })
  })

  it('sorts topics by urgency badge for mobile stories strip', () => {
    const topicOverdue = {
      wire: { id: crypto.randomUUID() },
      document: { name: 'Overdue' },
    }
    const topicDueSoon = {
      wire: { id: crypto.randomUUID() },
      document: { name: 'Due soon' },
    }
    const topicQuiet = {
      wire: { id: crypto.randomUUID() },
      document: { name: 'Quiet' },
    }
    const topicMoreDueSoon = {
      wire: { id: crypto.randomUUID() },
      document: { name: 'More due soon' },
    }

    const badges = new Map([
      [topicOverdue.wire.id, { count: 1, hasOverdue: true }],
      [topicDueSoon.wire.id, { count: 1, hasOverdue: false }],
      [topicMoreDueSoon.wire.id, { count: 3, hasOverdue: false }],
      [topicQuiet.wire.id, { count: 0, hasOverdue: false }],
    ])

    const sorted = sortTopicsByUrgency(
      [topicQuiet, topicDueSoon, topicOverdue, topicMoreDueSoon],
      badges,
    )

    expect(sorted.map((topic) => topic.document?.name)).toEqual([
      'Overdue',
      'More due soon',
      'Due soon',
      'Quiet',
    ])
  })

  it('groups list history tasks by day newest first', () => {
    const overdue = task({ state: 'open' }, '2026-07-17T09:00:00.000Z')
    overdue.document.title = 'Scaduta'
    const today = task({ state: 'open' }, '2026-07-18T18:00:00.000Z')
    today.document.title = 'Oggi'
    const completed = task({
      state: 'completed',
      completed_by: crypto.randomUUID(),
      completed_at: '2026-07-16T15:00:00.000Z',
    })
    completed.document.title = 'Fatta'
    completed.document.due_at = '2026-07-20T09:00:00.000Z'

    const groups = groupTasksByHistoryDay([overdue, today, completed], now)
    expect(groups.map((group) => group.tasks.map((item) => item.document.title))).toEqual([
      ['Oggi'],
      ['Scaduta'],
      ['Fatta'],
    ])
  })
})
