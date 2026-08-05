import { describe, expect, it } from 'vitest'
import { buildThreePretaskPreset } from './presets'

describe('preset assignment choices', () => {
  it('builds one compatible selected value for every pretask kind', () => {
    const result = buildThreePretaskPreset({
      name: ' Launch ',
      priorityTitle: 'Review',
      priority: 'high',
      deadlineTitle: 'Ship',
      deadlineDueAt: '2026-07-20T09:00:00.000Z',
      recurringTitle: 'Report',
      recurringDueAt: '2026-07-21T09:00:00.000Z',
      frequency: 'weekly',
      interval: 2,
    })

    expect(result.name).toBe('Launch')
    expect(result.pretasks.map((item) => item.taskKind)).toEqual([
      'priority',
      'deadline',
      'recurring',
    ])
    expect(result.pretasks[0]?.selectedValue).toEqual({
      schema: 1,
      priority: 'high',
    })
    expect(result.pretasks[1]?.selectedValue).toEqual({
      schema: 1,
      due_at: '2026-07-20T09:00:00.000Z',
    })
    expect(result.pretasks[2]?.selectedValue).toEqual({
      schema: 1,
      due_at: '2026-07-21T09:00:00.000Z',
      recurrence: { frequency: 'weekly', interval: 2 },
    })
  })

  it('rejects an incomplete or incompatible selected value', () => {
    expect(() =>
      buildThreePretaskPreset({
        name: 'Invalid',
        priorityTitle: 'Review',
        priority: 'normal',
        deadlineTitle: 'Ship',
        deadlineDueAt: '',
        recurringTitle: 'Report',
        recurringDueAt: '2026-07-21T09:00:00.000Z',
        frequency: 'daily',
        interval: 1,
      }),
    ).toThrow(/valid date/i)
  })
})
