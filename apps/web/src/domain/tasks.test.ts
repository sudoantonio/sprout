import { describe, expect, it } from 'vitest'
import type { TaskDto } from '../api/contracts'
import type { DecryptedTask } from './models'
import {
  buildNextRecurringTask,
  buildTaskCreation,
  filterTasks,
  isRecurringTaskOverdue,
} from './tasks'

const now = new Date('2026-07-18T12:00:00.000Z')

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
    expect(filterTasks([open, completed], 'open', now)).toEqual([open])
    expect(filterTasks([open, completed], 'completed', now)).toEqual([
      completed,
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
