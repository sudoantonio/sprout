import { describe, expect, it } from 'vitest'
import type { DecryptedTask } from './models'
import {
  buildTimelineBands,
  buildTimelineDayColumns,
  buildTimelineLanes,
  buildTimelineTicks,
  buildTimelineWindow,
  clampTimelineScale,
  defaultTimelineDueDatetimeLocal,
  filterTimelineTasks,
  getWeekDays,
  isTimelineTask,
  layoutTimelineLane,
  nudgeTimelineScale,
  packStackIndexes,
  startOfDay,
  startOfWeek,
  resolveTimelineMoveRange,
  resolveTimelineResizeRange,
  taskVisualRange,
  TIMELINE_MIN_BAR_WIDTH,
  TIMELINE_MIN_DURATION_MS_HOUR,
  TIMELINE_SCALE_DEFAULT,
  timeToX,
  timelineRangeToTaskTimes,
  toDateKey,
  todayLineX,
  visualDurationMsForLevel,
  xToTime,
  zoomLevelFromScale,
  MS_PER_DAY,
  MS_PER_HOUR,
  MS_PER_MINUTE,
} from './timeline'

const makeTask = (
  kind: 'priority' | 'deadline' | 'recurring',
  dueAt?: string,
): DecryptedTask => ({
  wire: {
    id: crypto.randomUUID(),
    project_id: crypto.randomUUID(),
    list_id: crypto.randomUUID(),
    resource_node_id: crypto.randomUUID(),
    task_kind: kind,
    payload: null,
    selected_value_snapshot: null,
    key_epoch: 1,
    state: { state: 'open' },
    source_pretask_id: null,
    preset_assignment_id: null,
    copied_from_task_id: null,
    questionnaire_version_id: null,
    recurrence_series_id: kind === 'recurring' ? crypto.randomUUID() : null,
    occurrence_number: kind === 'recurring' ? 1 : null,
    active_assignment_id: null,
    active_assignee_identity_id: null,
    created_at: '2026-07-18T12:00:00.000Z',
    payload_version: 1,
  },
  document: {
    schema: 1,
    title: kind,
    due_at: dueAt,
    recurrence:
      kind === 'recurring' ? { frequency: 'daily', interval: 1 } : undefined,
  },
})

describe('timeline task filter', () => {
  it('includes deadline and recurring tasks with due dates', () => {
    expect(isTimelineTask(makeTask('deadline', '2026-07-20T10:00:00.000Z'))).toBe(
      true,
    )
    expect(
      isTimelineTask(makeTask('recurring', '2026-07-20T10:00:00.000Z')),
    ).toBe(true)
  })

  it('excludes priority tasks even with a due date', () => {
    const priority = makeTask('priority', '2026-07-20T10:00:00.000Z')
    expect(isTimelineTask(priority)).toBe(false)
    expect(filterTimelineTasks([priority])).toEqual([])
  })
})

describe('week helpers', () => {
  it('starts weeks on Monday', () => {
    const wednesday = new Date('2026-07-22T15:00:00.000Z')
    const monday = startOfWeek(wednesday)
    expect(monday.getDay()).toBe(1)
    expect(toDateKey(monday)).toBe('2026-07-20')
    expect(getWeekDays(wednesday)).toHaveLength(7)
  })
})

describe('continuous scale and window', () => {
  it('maps scale to zoom levels', () => {
    expect(zoomLevelFromScale(40)).toBe('triDay')
    expect(zoomLevelFromScale(TIMELINE_SCALE_DEFAULT)).toBe('day')
    expect(zoomLevelFromScale(480)).toBe('hour')
  })

  it('builds a continuous window with pixel width', () => {
    const focus = new Date(2026, 6, 25)
    const window = buildTimelineWindow(focus, TIMELINE_SCALE_DEFAULT)
    expect(window.level).toBe('day')
    expect(window.widthPx).toBeGreaterThan(0)
    expect(window.endMs).toBeGreaterThan(window.startMs)
  })

  it('converts time to x and back', () => {
    const window = buildTimelineWindow(new Date(2026, 6, 25), 100)
    const mid = window.startMs + MS_PER_DAY * 3
    const x = timeToX(mid, window.startMs, window.pxPerDay)
    const back = xToTime(x, window.startMs, window.pxPerDay)
    expect(Math.abs(back.getTime() - mid)).toBeLessThan(2)
  })

  it('builds day ticks with today marked at mid-day', () => {
    const focus = new Date(2026, 6, 25, 12)
    const window = buildTimelineWindow(focus, TIMELINE_SCALE_DEFAULT)
    const ticks = buildTimelineTicks(window, focus)
    const today = ticks.find((tick) => tick.isToday)
    expect(today?.label).toBeTruthy()
    expect(ticks.some((tick) => tick.label === '25')).toBe(true)
    expect(ticks.every((tick) => tick.kind === 'major')).toBe(true)

    const day25 = startOfDay(new Date(2026, 6, 25))
    const day26 = new Date(day25)
    day26.setDate(day25.getDate() + 1)
    const dayStartX = timeToX(day25.getTime(), window.startMs, window.pxPerDay)
    const dayEndX = timeToX(day26.getTime(), window.startMs, window.pxPerDay)
    const midX = dayStartX + (dayEndX - dayStartX) / 2
    const tick25 = ticks.find((tick) => toDateKey(tick.at) === '2026-07-25')
    expect(tick25?.x).toBeCloseTo(midX, 5)

    expect(tick25?.x).not.toBeCloseTo(dayStartX, 0)
  })

  it('places today line at start of today for day zoom', () => {
    const now = new Date(2026, 6, 25, 17, 30)
    const window = buildTimelineWindow(new Date(2026, 6, 25), TIMELINE_SCALE_DEFAULT)
    const lineX = todayLineX(window, now)
    const todayStartX = timeToX(
      startOfDay(now).getTime(),
      window.startMs,
      window.pxPerDay,
    )
    expect(lineX).toBeCloseTo(todayStartX, 5)
  })

  it('places today line at exact now for hour zoom', () => {
    const now = new Date(2026, 6, 25, 17, 30)
    const window = buildTimelineWindow(new Date(2026, 6, 25), 480)
    expect(window.level).toBe('hour')
    const lineX = todayLineX(window, now)
    expect(lineX).toBeCloseTo(
      timeToX(now.getTime(), window.startMs, window.pxPerDay),
      5,
    )
  })

  it('builds tri-day ticks every three days only', () => {
    const focus = new Date(2026, 6, 25, 12)
    const window = buildTimelineWindow(focus, 40)
    expect(window.level).toBe('triDay')
    const ticks = buildTimelineTicks(window, focus)
    expect(ticks.length).toBeGreaterThan(1)
    expect(ticks.every((tick) => tick.label)).toBe(true)
    for (let index = 1; index < ticks.length; index += 1) {
      const prev = ticks[index - 1]?.at.getTime() ?? 0
      const next = ticks[index]?.at.getTime() ?? 0
      expect(next - prev).toBe(3 * MS_PER_DAY)
    }
  })

  it('adds half-hour minor ticks when zoomed in to hours', () => {
    const focus = new Date(2026, 6, 25, 12)
    const window = buildTimelineWindow(focus, 900)
    expect(window.level).toBe('hour')
    const ticks = buildTimelineTicks(window, focus)
    expect(ticks.some((tick) => tick.kind === 'minor')).toBe(true)
    expect(ticks.some((tick) => tick.kind === 'major' && tick.label.includes(':'))).toBe(
      true,
    )
  })

  it('builds full-day weekend and alternating weekday bands without dividers', () => {
    const window = buildTimelineWindow(new Date(2026, 6, 25), TIMELINE_SCALE_DEFAULT)
    expect(window.level).toBe('day')
    const bands = buildTimelineBands(window)
    expect(bands.some((band) => band.kind === 'alt')).toBe(true)
    expect(bands.some((band) => band.kind === 'weekend')).toBe(true)
    expect(bands.every((band) => band.kind === 'alt' || band.kind === 'weekend')).toBe(
      true,
    )
    expect(bands.every((band) => band.width > 1)).toBe(true)

    const columns = buildTimelineDayColumns(window)
    for (const column of columns) {
      const band = bands.find((entry) => entry.key.endsWith(column.key))
      if (column.start.getDay() === 0 || column.start.getDay() === 6) {
        expect(band?.kind).toBe('weekend')
        expect(band?.width).toBeCloseTo(column.width, 5)
      } else if (column.dayIndex % 2 === 1) {
        expect(band?.kind).toBe('alt')
        expect(band?.width).toBeCloseTo(column.width, 5)
      } else {
        expect(band).toBeUndefined()
      }
    }
  })

  it('bands by calendar day at hour zoom', () => {
    const window = buildTimelineWindow(new Date(2026, 6, 25, 12), 900)
    expect(window.level).toBe('hour')
    const bands = buildTimelineBands(window)
    const columns = buildTimelineDayColumns(window)
    expect(bands.some((band) => band.kind === 'weekend')).toBe(true)
    for (const column of columns) {
      const band = bands.find((entry) => entry.key.endsWith(column.key))
      if (column.start.getDay() === 0 || column.start.getDay() === 6) {
        expect(band?.kind).toBe('weekend')
        expect(band?.width).toBeCloseTo(column.width, 5)
      } else if (column.dayIndex % 2 === 1) {
        expect(band?.kind).toBe('alt')
        expect(band?.width).toBeCloseTo(column.width, 5)
      } else {
        expect(band).toBeUndefined()
      }
    }
  })

  it('groups alternating bands into three-day blocks at triDay zoom', () => {
    const focus = new Date(2026, 6, 25, 12)
    const window = buildTimelineWindow(focus, 40)
    expect(window.level).toBe('triDay')
    const bands = buildTimelineBands(window)
    expect(bands.every((band) => band.kind === 'alt')).toBe(true)
    expect(bands.every((band) => band.width > 1)).toBe(true)

    const threeDayWidth =
      timeToX(
        startOfDay(window.start).getTime() + 3 * MS_PER_DAY,
        window.startMs,
        window.pxPerDay,
      ) -
      timeToX(startOfDay(window.start).getTime(), window.startMs, window.pxPerDay)

    for (const band of bands) {
      expect(band.width).toBeCloseTo(threeDayWidth, 5)
    }

    const ticks = buildTimelineTicks(window, focus)
    const altTickKeys = ticks
      .filter((_, index) => index % 2 === 1)
      .map((tick) => tick.key)
    expect(bands.map((band) => band.key.replace(/^alt-/, ''))).toEqual(altTickKeys)
  })

  it('nudges scale within bounds', () => {
    expect(nudgeTimelineScale(100, 1)).toBeGreaterThan(100)
    expect(clampTimelineScale(10_000)).toBeLessThanOrEqual(1100)
  })
})

describe('task visual layout', () => {
  it('occupies the full due-day column at day zoom', () => {
    const due = new Date(2026, 6, 25, 17, 0, 0, 0)
    const task = makeTask('deadline', due.toISOString())
    const range = taskVisualRange(task, 'day')
    const dayStart = startOfDay(due)
    const dayEnd = new Date(dayStart)
    dayEnd.setDate(dayStart.getDate() + 1)
    expect(range?.startMs).toBe(dayStart.getTime())
    expect(range?.endMs).toBe(dayEnd.getTime())
  })

  it('occupies the full due-day column at triDay zoom', () => {
    const due = new Date(2026, 6, 25, 9, 15, 0, 0)
    const task = makeTask('deadline', due.toISOString())
    const range = taskVisualRange(task, 'triDay')
    expect(range?.startMs).toBe(startOfDay(due).getTime())
    const dayEnd = startOfDay(due)
    dayEnd.setDate(dayEnd.getDate() + 1)
    expect(range?.endMs).toBe(dayEnd.getTime())
  })

  it('ends bars at due_at with duration at hour zoom', () => {
    const due = new Date(2026, 6, 25, 17, 0, 0, 0)
    const task = makeTask('deadline', due.toISOString())
    const range = taskVisualRange(task, 'hour')
    expect(range?.endMs).toBe(due.getTime())
    expect(range?.startMs).toBe(due.getTime() - visualDurationMsForLevel('hour'))
    expect(visualDurationMsForLevel('hour')).toBe(3 * MS_PER_HOUR)
  })

  it('uses start_at through due_at when both are set', () => {
    const start = new Date(2026, 6, 24, 9, 0, 0, 0)
    const due = new Date(2026, 6, 25, 17, 0, 0, 0)
    const task = makeTask('deadline', due.toISOString())
    task.document.start_at = start.toISOString()

    const dayRange = taskVisualRange(task, 'day')
    expect(dayRange?.startMs).toBe(startOfDay(start).getTime())
    expect(dayRange?.endMs).toBe(
      startOfDay(due).getTime() + MS_PER_DAY,
    )

    const hourRange = taskVisualRange(task, 'hour')
    expect(hourRange?.startMs).toBe(start.getTime())
    expect(hourRange?.endMs).toBe(due.getTime())
  })

  it('snaps resize edges and enforces a minimum duration', () => {
    const start = new Date(2026, 6, 25, 10, 0, 0, 0).getTime()
    const end = new Date(2026, 6, 25, 13, 0, 0, 0).getTime()
    const moved = resolveTimelineResizeRange(
      'start',
      start + 10 * MS_PER_MINUTE,
      start,
      end,
      'hour',
    )
    expect(moved.startMs).toBe(start + 15 * MS_PER_MINUTE)
    expect(moved.endMs).toBe(end)

    const tooShort = resolveTimelineResizeRange(
      'end',
      start + 5 * MS_PER_MINUTE,
      start,
      end,
      'hour',
    )
    expect(tooShort.endMs).toBe(start + TIMELINE_MIN_DURATION_MS_HOUR)

    const dayStart = startOfDay(new Date(2026, 6, 25)).getTime()
    const dayEnd = dayStart + MS_PER_DAY
    const expanded = resolveTimelineResizeRange(
      'end',
      dayStart + 2 * MS_PER_DAY + MS_PER_HOUR,
      dayStart,
      dayEnd,
      'day',
    )
    expect(expanded.endMs).toBe(dayStart + 3 * MS_PER_DAY)
  })

  it('converts a resized day range back to start_at and due_at', () => {
    const previousDue = new Date(2026, 6, 25, 17, 30, 0, 0)
    const startMs = startOfDay(new Date(2026, 6, 24)).getTime()
    const endMs = startOfDay(new Date(2026, 6, 26)).getTime() + MS_PER_DAY
    const times = timelineRangeToTaskTimes(startMs, endMs, 'day', {
      dueAt: previousDue.toISOString(),
    })
    expect(new Date(times.start_at).getTime()).toBe(startMs)
    const due = new Date(times.due_at)
    expect(due.getFullYear()).toBe(2026)
    expect(due.getMonth()).toBe(6)
    expect(due.getDate()).toBe(26)
    expect(due.getHours()).toBe(17)
    expect(due.getMinutes()).toBe(30)
  })

  it('moves a bar while preserving duration and snapping', () => {
    const start = new Date(2026, 6, 25, 10, 0, 0, 0).getTime()
    const end = start + MS_PER_HOUR
    const grabOffset = 20 * MS_PER_MINUTE
    const moved = resolveTimelineMoveRange(
      start + grabOffset + 40 * MS_PER_MINUTE,
      grabOffset,
      start,
      end,
      'hour',
    )
    expect(moved.startMs).toBe(start + 45 * MS_PER_MINUTE)
    expect(moved.endMs - moved.startMs).toBe(MS_PER_HOUR)

    const dayStart = startOfDay(new Date(2026, 6, 25)).getTime()
    const dayEnd = dayStart + 2 * MS_PER_DAY
    const dayMoved = resolveTimelineMoveRange(
      dayStart + MS_PER_DAY + MS_PER_HOUR,
      MS_PER_HOUR,
      dayStart,
      dayEnd,
      'day',
    )
    expect(dayMoved.startMs).toBe(dayStart + MS_PER_DAY)
    expect(dayMoved.endMs).toBe(dayStart + 3 * MS_PER_DAY)
  })

  it('packs overlapping intervals into stacked lanes', () => {
    const stacks = packStackIndexes([
      { id: 'a', startMs: 0, endMs: 10 },
      { id: 'b', startMs: 5, endMs: 15 },
      { id: 'c', startMs: 10, endMs: 20 },
    ])
    expect(stacks.get('a')).toBe(0)
    expect(stacks.get('b')).toBe(1)
    expect(stacks.get('c')).toBe(0)
  })

  it('lays out due-day bars at day-start x matching columns', () => {
    const listId = crypto.randomUUID()
    const window = buildTimelineWindow(new Date(2026, 6, 25), TIMELINE_SCALE_DEFAULT)
    const dueDay = new Date(2026, 6, 25, 17)
    const task = makeTask('deadline', dueDay.toISOString())
    task.wire.list_id = listId
    task.document.title = 'Scheda 1'

    const lane = layoutTimelineLane(listId, [task], window)
    expect(lane.tasks).toHaveLength(1)
    const laidOut = lane.tasks[0]
    const expectedLeft = timeToX(
      startOfDay(dueDay).getTime(),
      window.startMs,
      window.pxPerDay,
    )
    expect(laidOut?.left).toBeCloseTo(expectedLeft, 5)

    const column = buildTimelineDayColumns(window).find(
      (col) => col.key === '2026-07-25',
    )
    expect(laidOut?.left).toBeCloseTo(column?.x ?? -1, 5)
    expect(laidOut?.width).toBeGreaterThanOrEqual(
      Math.min(TIMELINE_MIN_BAR_WIDTH, column?.width ?? 0),
    )
    // Natural day width when scale >= min bar; otherwise expands right from start.
    if ((column?.width ?? 0) >= TIMELINE_MIN_BAR_WIDTH) {
      expect(laidOut?.width).toBeCloseTo(column?.width ?? 0, 5)
    } else {
      expect(laidOut?.width).toBe(TIMELINE_MIN_BAR_WIDTH)
    }
    expect(lane.stackCount).toBe(1)

    const lanes = buildTimelineLanes([listId], [task], window)
    expect(lanes[0]?.tasks[0]?.task.document.title).toBe('Scheda 1')
  })

  it('expands min bar width to the right without shifting left', () => {
    const listId = crypto.randomUUID()
    // Narrow day columns so natural width < TIMELINE_MIN_BAR_WIDTH.
    const window = buildTimelineWindow(new Date(2026, 6, 25), 40)
    expect(window.level).toBe('triDay')
    const dueDay = new Date(2026, 6, 25, 17)
    const task = makeTask('deadline', dueDay.toISOString())
    task.wire.list_id = listId

    const lane = layoutTimelineLane(listId, [task], window)
    const laidOut = lane.tasks[0]
    const expectedLeft = timeToX(
      startOfDay(dueDay).getTime(),
      window.startMs,
      window.pxPerDay,
    )
    expect(laidOut?.left).toBeCloseTo(expectedLeft, 5)
    expect(laidOut?.width).toBe(TIMELINE_MIN_BAR_WIDTH)
  })

  it('clamps partially offscreen bars to non-negative left within the axis', () => {
    const listId = crypto.randomUUID()
    const focus = new Date(2026, 6, 25, 12)
    const hourWindow = buildTimelineWindow(focus, 480)
    expect(hourWindow.level).toBe('hour')
    // Visual bar starts 3h before due_at; place due 1h after window start so left is negative.
    const earlyDue = new Date(hourWindow.startMs + MS_PER_HOUR)
    const earlyTask = makeTask('deadline', earlyDue.toISOString())
    earlyTask.wire.list_id = listId

    const range = taskVisualRange(earlyTask, hourWindow.level)
    expect(range).not.toBeNull()
    expect(range!.startMs).toBeLessThan(hourWindow.startMs)

    const lane = layoutTimelineLane(listId, [earlyTask], hourWindow)
    expect(lane.tasks).toHaveLength(1)
    const laidOut = lane.tasks[0]!
    expect(laidOut.left).toBe(0)
    expect(laidOut.width).toBeGreaterThan(0)
    expect(laidOut.left + laidOut.width).toBeLessThanOrEqual(
      hourWindow.widthPx + 0.001,
    )
  })
})

describe('defaultTimelineDueDatetimeLocal', () => {
  it('returns 17:00 local on the given day for day scale', () => {
    const day = new Date(2026, 6, 22, 9, 30)
    expect(defaultTimelineDueDatetimeLocal(day, TIMELINE_SCALE_DEFAULT)).toBe(
      '2026-07-22T17:00',
    )
  })

  it('keeps the hour when zoomed to hour scale', () => {
    const hour = new Date(2026, 6, 22, 9, 15, 0, 0)
    expect(defaultTimelineDueDatetimeLocal(hour, 480)).toBe('2026-07-22T09:15')
  })
})
