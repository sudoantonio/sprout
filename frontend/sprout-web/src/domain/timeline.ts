import type { DecryptedTask } from './models'

export type TimelineZoomLevel = 'triDay' | 'day' | 'hour'

export const MS_PER_MINUTE = 60 * 1000
export const MS_PER_HOUR = 60 * MS_PER_MINUTE
export const MS_PER_DAY = 24 * MS_PER_HOUR

/** Pixels representing one day on the continuous axis. */
export const TIMELINE_SCALE_MIN = 28
export const TIMELINE_SCALE_MAX = 1100
export const TIMELINE_SCALE_DEFAULT = 100
export const TIMELINE_SCALE_STEP = 1.5
/** Wheel zoom sensitivity (higher = faster). */
export const TIMELINE_WHEEL_ZOOM_FACTOR = 0.012

export const DEFAULT_TIMELINE_ZOOM: TimelineZoomLevel = 'day'

export const TIMELINE_CARD_HEIGHT = 32
export const TIMELINE_CARD_GAP = 8
export const TIMELINE_LANE_PADDING_Y = 12
/** Reserved height at the top of each lane for the hover "+" create bar. */
export const TIMELINE_LANE_CREATE_ZONE_HEIGHT = 28
export const TIMELINE_MIN_BAR_WIDTH = 72

export const timelineBarTop = (stackIndex: number): number =>
  TIMELINE_LANE_CREATE_ZONE_HEIGHT +
  TIMELINE_LANE_PADDING_Y +
  stackIndex * (TIMELINE_CARD_HEIGHT + TIMELINE_CARD_GAP)

export interface TimelineTick {
  key: string
  at: Date
  x: number
  label: string
  secondary?: string
  isToday: boolean
  isWeekend: boolean
  /** Unlabeled header marks (e.g. half-hours). */
  kind?: 'major' | 'minor'
}

export interface TimelineBand {
  key: string
  x: number
  width: number
  /** `divider` is legacy and not emitted; views ignore it if present. */
  kind: 'weekend' | 'alt' | 'divider'
}

export interface TimelineWindow {
  start: Date
  end: Date
  startMs: number
  endMs: number
  widthPx: number
  pxPerDay: number
  level: TimelineZoomLevel
}

export interface LaidOutTimelineTask {
  task: DecryptedTask
  startMs: number
  endMs: number
  left: number
  width: number
  stackIndex: number
}

export interface TimelineLaneLayout {
  listId: string
  tasks: LaidOutTimelineTask[]
  stackCount: number
  height: number
}

export const startOfDay = (value: Date): Date => {
  const date = new Date(value)
  date.setHours(0, 0, 0, 0)
  return date
}

/** Monday as week start (it-IT convention). */
export const startOfWeek = (value: Date): Date => {
  const date = startOfDay(value)
  const weekday = date.getDay()
  const diff = weekday === 0 ? -6 : 1 - weekday
  date.setDate(date.getDate() + diff)
  return date
}

export const toDateKey = (value: Date): string => {
  const year = value.getFullYear()
  const month = String(value.getMonth() + 1).padStart(2, '0')
  const day = String(value.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

export const isTimelineTask = (task: DecryptedTask): boolean => {
  if (task.wire.task_kind === 'priority') return false
  if (!task.document.due_at) return false
  return (
    task.wire.task_kind === 'deadline' || task.wire.task_kind === 'recurring'
  )
}

export const filterTimelineTasks = (tasks: DecryptedTask[]): DecryptedTask[] =>
  tasks.filter(isTimelineTask)

export const clampTimelineScale = (scale: number): number =>
  Math.min(TIMELINE_SCALE_MAX, Math.max(TIMELINE_SCALE_MIN, scale))

export const nudgeTimelineScale = (
  scale: number,
  direction: 1 | -1,
): number => {
  const next =
    direction > 0 ? scale * TIMELINE_SCALE_STEP : scale / TIMELINE_SCALE_STEP
  return clampTimelineScale(next)
}

export const canZoomScaleIn = (scale: number): boolean =>
  scale < TIMELINE_SCALE_MAX - 0.5

export const canZoomScaleOut = (scale: number): boolean =>
  scale > TIMELINE_SCALE_MIN + 0.5

export const zoomLevelFromScale = (pxPerDay: number): TimelineZoomLevel => {
  if (pxPerDay < 56) return 'triDay'
  if (pxPerDay < 300) return 'day'
  return 'hour'
}

export const pxPerMsFromScale = (pxPerDay: number): number =>
  clampTimelineScale(pxPerDay) / MS_PER_DAY

export const timeToX = (
  timeMs: number,
  viewStartMs: number,
  pxPerDay: number,
): number => (timeMs - viewStartMs) * pxPerMsFromScale(pxPerDay)

export const xToTime = (
  x: number,
  viewStartMs: number,
  pxPerDay: number,
): Date => new Date(viewStartMs + x / pxPerMsFromScale(pxPerDay))

/** Hour-zoom bar duration ending at due_at. Day/triDay use full due-day columns. */
export const visualDurationMsForLevel = (level: TimelineZoomLevel): number => {
  switch (level) {
    case 'triDay':
      return 3 * MS_PER_DAY
    case 'day':
      return MS_PER_DAY
    case 'hour':
      return 3 * MS_PER_HOUR
  }
}

/** Snap step while resizing at hour zoom. */
export const TIMELINE_RESIZE_SNAP_MS = 15 * MS_PER_MINUTE
export const TIMELINE_MIN_DURATION_MS_HOUR = 15 * MS_PER_MINUTE
export const TIMELINE_MIN_DURATION_MS_DAY = MS_PER_DAY

export const snapTimelineTime = (
  timeMs: number,
  level: TimelineZoomLevel,
): number => {
  if (level === 'hour') {
    return Math.round(timeMs / TIMELINE_RESIZE_SNAP_MS) * TIMELINE_RESIZE_SNAP_MS
  }
  return startOfDay(new Date(timeMs)).getTime()
}

export const minTimelineDurationMs = (level: TimelineZoomLevel): number =>
  level === 'hour' ? TIMELINE_MIN_DURATION_MS_HOUR : TIMELINE_MIN_DURATION_MS_DAY

/** Resolve a live resize drag into a valid [start, end) range. */
export const resolveTimelineResizeRange = (
  edge: 'start' | 'end',
  pointerMs: number,
  currentStartMs: number,
  currentEndMs: number,
  level: TimelineZoomLevel,
): { startMs: number; endMs: number } => {
  const minDuration = minTimelineDurationMs(level)
  const snapped = snapTimelineTime(pointerMs, level)

  if (edge === 'start') {
    const maxStart = currentEndMs - minDuration
    let startMs = Math.min(snapped, maxStart)
    if (level !== 'hour') {
      startMs = startOfDay(new Date(startMs)).getTime()
    }
    return { startMs, endMs: currentEndMs }
  }

  let endMs = Math.max(snapped, currentStartMs + minDuration)
  if (level !== 'hour') {
    // Day bars use exclusive end-of-day: pointer day → next midnight.
    const dayStart = startOfDay(new Date(snapped))
    endMs = dayStart.getTime() + MS_PER_DAY
    if (endMs < currentStartMs + minDuration) {
      endMs = currentStartMs + minDuration
    }
  }
  return { startMs: currentStartMs, endMs }
}

/**
 * Resolve a body-drag move into a valid [start, end) range.
 * Keeps the visual duration; snaps the start edge to the zoom grid.
 */
export const resolveTimelineMoveRange = (
  pointerMs: number,
  grabOffsetMs: number,
  originStartMs: number,
  originEndMs: number,
  level: TimelineZoomLevel,
): { startMs: number; endMs: number } => {
  const duration = Math.max(
    minTimelineDurationMs(level),
    originEndMs - originStartMs,
  )
  let startMs = snapTimelineTime(pointerMs - grabOffsetMs, level)
  if (level !== 'hour') {
    startMs = startOfDay(new Date(startMs)).getTime()
  }
  return { startMs, endMs: startMs + duration }
}

const applyTimeOfDay = (dayStart: Date, timeSource: Date): Date => {
  const next = new Date(dayStart)
  next.setHours(
    timeSource.getHours(),
    timeSource.getMinutes(),
    timeSource.getSeconds(),
    timeSource.getMilliseconds(),
  )
  return next
}

/**
 * Convert a visual [start, end) range back to persisted start_at / due_at.
 * Preserves clock time from the previous values when snapping by day.
 */
export const timelineRangeToTaskTimes = (
  startMs: number,
  endMs: number,
  level: TimelineZoomLevel,
  previous: { startAt?: string; dueAt: string },
): { start_at: string; due_at: string } => {
  const previousDue = new Date(previous.dueAt)
  const previousStart = previous.startAt
    ? new Date(previous.startAt)
    : null

  if (level === 'hour') {
    return {
      start_at: new Date(startMs).toISOString(),
      due_at: new Date(endMs).toISOString(),
    }
  }

  const startDay = startOfDay(new Date(startMs))
  const dueDay = startOfDay(new Date(endMs - 1))
  const start_at = (
    previousStart
      ? applyTimeOfDay(startDay, previousStart)
      : startDay
  ).toISOString()
  const due_at = applyTimeOfDay(dueDay, previousDue).toISOString()
  return { start_at, due_at }
}

export const buildTimelineWindow = (
  focus: Date,
  pxPerDay: number,
): TimelineWindow => {
  const scale = clampTimelineScale(pxPerDay)
  const level = zoomLevelFromScale(scale)
  const daySpan =
    level === 'hour' ? 7 : level === 'day' ? 42 : 90
  const focusDay = startOfDay(focus)
  const before = Math.floor(daySpan / 3)
  const start = new Date(focusDay)
  start.setDate(focusDay.getDate() - before)
  const end = new Date(start)
  end.setDate(start.getDate() + daySpan)
  const startMs = start.getTime()
  const endMs = end.getTime()
  return {
    start,
    end,
    startMs,
    endMs,
    widthPx: (endMs - startMs) * pxPerMsFromScale(scale),
    pxPerDay: scale,
    level,
  }
}

const isWeekend = (date: Date): boolean => {
  const day = date.getDay()
  return day === 0 || day === 6
}

const MS_PER_HALF_HOUR = MS_PER_MINUTE * 30

/**
 * Keep a readable amount of space between hour labels as the timeline zooms.
 * The grid becomes progressively denser, rather than switching straight to a
 * label for every hour as soon as hour mode opens.
 */
const hourTickPlan = (
  pxPerDay: number,
): { majorStepMs: number; minorStepMs: number | null } => {
  const pxPerHour = pxPerDay / 24
  if (pxPerHour >= 42) {
    return { majorStepMs: MS_PER_HOUR, minorStepMs: MS_PER_HALF_HOUR }
  }
  if (pxPerHour >= 30) {
    return { majorStepMs: 2 * MS_PER_HOUR, minorStepMs: MS_PER_HOUR }
  }
  if (pxPerHour >= 22) {
    return { majorStepMs: 3 * MS_PER_HOUR, minorStepMs: MS_PER_HOUR }
  }
  if (pxPerHour >= 16) {
    return { majorStepMs: 4 * MS_PER_HOUR, minorStepMs: 2 * MS_PER_HOUR }
  }
  return { majorStepMs: 6 * MS_PER_HOUR, minorStepMs: 3 * MS_PER_HOUR }
}

export const buildTimelineTicks = (
  window: TimelineWindow,
  now = new Date(),
  locale = 'it-IT',
): TimelineTick[] => {
  const ticks: TimelineTick[] = []
  const todayKey = toDateKey(now)
  const hourFormatter = new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
  })
  const dayFormatter = new Intl.DateTimeFormat(locale, { day: 'numeric' })
  const monthFormatter = new Intl.DateTimeFormat(locale, { month: 'short' })

  if (window.level === 'hour') {
    const { majorStepMs, minorStepMs } = hourTickPlan(window.pxPerDay)
    const majorTimes = new Set<number>()

    const alignHourCursor = (value: Date, stepMs: number): Date => {
      const aligned = new Date(value)
      const stepMinutes = stepMs / MS_PER_MINUTE
      const minuteOfDay = aligned.getHours() * 60 + aligned.getMinutes()
      const alignedMinutes = Math.floor(minuteOfDay / stepMinutes) * stepMinutes
      aligned.setHours(0, alignedMinutes, 0, 0)
      return aligned
    }

    const majorCursor = alignHourCursor(window.start, majorStepMs)
    while (majorCursor.getTime() < window.endMs) {
      const at = new Date(majorCursor)
      const stamp = at.getTime()
      majorTimes.add(stamp)
      ticks.push({
        key: `${toDateKey(at)}T${String(at.getHours()).padStart(2, '0')}`,
        at,
        x: timeToX(stamp, window.startMs, window.pxPerDay),
        label: hourFormatter.format(at),
        secondary: at.getHours() === 0 ? dayFormatter.format(at) : undefined,
        kind: 'major',
        isToday:
          toDateKey(at) === todayKey && at.getHours() === now.getHours(),
        isWeekend: isWeekend(at),
      })
      majorCursor.setTime(majorCursor.getTime() + majorStepMs)
    }

    if (minorStepMs) {
      const minorCursor = alignHourCursor(window.start, minorStepMs)
      while (minorCursor.getTime() < window.endMs) {
        const stamp = minorCursor.getTime()
        if (!majorTimes.has(stamp)) {
          const at = new Date(minorCursor)
          ticks.push({
            key: `${toDateKey(at)}T${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`,
            at,
            x: timeToX(stamp, window.startMs, window.pxPerDay),
            label: '',
            kind: 'minor',
            isToday:
              toDateKey(at) === todayKey &&
              at.getHours() === now.getHours() &&
              at.getMinutes() === now.getMinutes(),
            isWeekend: isWeekend(at),
          })
        }
        minorCursor.setTime(minorCursor.getTime() + minorStepMs)
      }
    }

    return ticks.sort((left, right) => left.at.getTime() - right.at.getTime())
  }

  /** Day / tri-day labels sit at mid-day for clearer column centering. */
  const dayColumnMidX = (dayStart: Date): number => {
    const next = new Date(dayStart)
    next.setDate(dayStart.getDate() + 1)
    const left = timeToX(dayStart.getTime(), window.startMs, window.pxPerDay)
    const right = timeToX(next.getTime(), window.startMs, window.pxPerDay)
    return left + (right - left) / 2
  }

  if (window.level === 'triDay') {
    const cursor = startOfDay(window.start)
    let lastLabeledMonth = -1
    while (cursor.getTime() < window.endMs) {
      const at = new Date(cursor)
      const month = at.getMonth()
      const showMonth = month !== lastLabeledMonth
      lastLabeledMonth = month
      ticks.push({
        key: toDateKey(at),
        at,
        x: dayColumnMidX(at),
        label: dayFormatter.format(at),
        secondary: showMonth ? monthFormatter.format(at) : undefined,
        kind: 'major',
        isToday: toDateKey(at) === todayKey,
        isWeekend: isWeekend(at),
      })
      cursor.setDate(cursor.getDate() + 3)
    }
    return ticks
  }

  let lastMonth = -1
  const cursor = startOfDay(window.start)
  while (cursor.getTime() < window.endMs) {
    const at = new Date(cursor)
    const month = at.getMonth()
    const showMonth = month !== lastMonth
    lastMonth = month
    ticks.push({
      key: toDateKey(at),
      at,
      x: dayColumnMidX(at),
      label: dayFormatter.format(at),
      secondary: showMonth ? monthFormatter.format(at) : undefined,
      kind: 'major',
      isToday: toDateKey(at) === todayKey,
      isWeekend: isWeekend(at),
    })
    cursor.setDate(cursor.getDate() + 1)
  }
  return ticks
}

export const buildTimelineBands = (window: TimelineWindow): TimelineBand[] => {
  const bands: TimelineBand[] = []

  /** Match triDay tick labels: alternating 3-day column groups. */
  if (window.level === 'triDay') {
    const cursor = startOfDay(window.start)
    let groupIndex = 0
    while (cursor.getTime() < window.endMs) {
      const at = new Date(cursor)
      const next = new Date(cursor)
      next.setDate(cursor.getDate() + 3)
      const x = timeToX(at.getTime(), window.startMs, window.pxPerDay)
      const width = Math.max(
        0,
        timeToX(next.getTime(), window.startMs, window.pxPerDay) - x,
      )
      if (width > 0 && groupIndex % 2 === 1) {
        bands.push({
          key: `alt-${toDateKey(at)}`,
          x,
          width,
          kind: 'alt',
        })
      }
      groupIndex += 1
      cursor.setDate(cursor.getDate() + 3)
    }
    return bands
  }

  /** Day / hour: per calendar day, with weekend highlight + weekday alt. */
  const cursor = startOfDay(window.start)
  let dayIndex = 0
  while (cursor.getTime() < window.endMs) {
    const at = new Date(cursor)
    const next = new Date(cursor)
    next.setDate(cursor.getDate() + 1)
    const x = timeToX(at.getTime(), window.startMs, window.pxPerDay)
    const width = Math.max(
      0,
      timeToX(next.getTime(), window.startMs, window.pxPerDay) - x,
    )
    if (width > 0) {
      if (isWeekend(at)) {
        bands.push({
          key: `weekend-${toDateKey(at)}`,
          x,
          width,
          kind: 'weekend',
        })
      } else if (dayIndex % 2 === 1) {
        bands.push({
          key: `alt-${toDateKey(at)}`,
          x,
          width,
          kind: 'alt',
        })
      }
    }
    dayIndex += 1
    cursor.setDate(cursor.getDate() + 1)
  }
  return bands
}

export interface TimelineDayColumn {
  key: string
  start: Date
  x: number
  width: number
  dayIndex: number
  isToday: boolean
}

export const buildTimelineDayColumns = (
  window: TimelineWindow,
  now = new Date(),
): TimelineDayColumn[] => {
  const columns: TimelineDayColumn[] = []
  const todayKey = toDateKey(now)
  const cursor = startOfDay(window.start)
  let dayIndex = 0
  while (cursor.getTime() < window.endMs) {
    const at = new Date(cursor)
    const next = new Date(cursor)
    next.setDate(cursor.getDate() + 1)
    const x = timeToX(at.getTime(), window.startMs, window.pxPerDay)
    const width = Math.max(
      0,
      timeToX(next.getTime(), window.startMs, window.pxPerDay) - x,
    )
    if (width > 0) {
      columns.push({
        key: toDateKey(at),
        start: at,
        x,
        width,
        dayIndex,
        isToday: toDateKey(at) === todayKey,
      })
    }
    dayIndex += 1
    cursor.setDate(cursor.getDate() + 1)
  }
  return columns
}

export const taskVisualRange = (
  task: DecryptedTask,
  level: TimelineZoomLevel,
): { startMs: number; endMs: number } | null => {
  const dueAt = task.document.due_at
  if (!dueAt) return null
  const due = new Date(dueAt)
  const startAt = task.document.start_at
    ? new Date(task.document.start_at)
    : null
  const hasStart =
    startAt !== null && !Number.isNaN(startAt.getTime())

  // Day / tri-day: full calendar days from start day through due day.
  if (level !== 'hour') {
    const endDay = startOfDay(due)
    const dayEnd = new Date(endDay)
    dayEnd.setDate(endDay.getDate() + 1)
    if (hasStart) {
      const dayStart = startOfDay(startAt)
      const startMs = Math.min(dayStart.getTime(), endDay.getTime())
      return { startMs, endMs: dayEnd.getTime() }
    }
    return { startMs: endDay.getTime(), endMs: dayEnd.getTime() }
  }

  // Hour zoom: explicit interval, or default duration ending at due_at.
  const endMs = due.getTime()
  if (hasStart) {
    const startMs = Math.min(startAt.getTime(), endMs - TIMELINE_MIN_DURATION_MS_HOUR)
    return { startMs, endMs }
  }
  return { startMs: endMs - visualDurationMsForLevel(level), endMs }
}

/** Greedy interval packing: assign non-overlapping stack rows. */
export const packStackIndexes = (
  intervals: Array<{ id: string; startMs: number; endMs: number }>,
): Map<string, number> => {
  const sorted = [...intervals].sort((left, right) => {
    if (left.startMs !== right.startMs) return left.startMs - right.startMs
    return left.endMs - right.endMs
  })
  const laneEnds: number[] = []
  const result = new Map<string, number>()

  for (const interval of sorted) {
    let stackIndex = laneEnds.findIndex((end) => end <= interval.startMs)
    if (stackIndex === -1) {
      stackIndex = laneEnds.length
      laneEnds.push(interval.endMs)
    } else {
      laneEnds[stackIndex] = interval.endMs
    }
    result.set(interval.id, stackIndex)
  }

  return result
}

export const layoutTimelineLane = (
  listId: string,
  tasks: DecryptedTask[],
  window: TimelineWindow,
): TimelineLaneLayout => {
  const ranged = tasks
    .map((task) => {
      const range = taskVisualRange(task, window.level)
      if (!range) return null
      if (range.endMs <= window.startMs || range.startMs >= window.endMs) {
        return null
      }
      return { task, ...range }
    })
    .filter((value): value is NonNullable<typeof value> => value !== null)

  const stacks = packStackIndexes(
    ranged.map((item) => ({
      id: item.task.wire.id,
      startMs: item.startMs,
      endMs: item.endMs,
    })),
  )

  const laidOut: LaidOutTimelineTask[] = ranged
    .map((item) => {
      // Expand min width to the right only so bars stay anchored at range start.
      const rawLeft = timeToX(item.startMs, window.startMs, window.pxPerDay)
      const rawRight = timeToX(item.endMs, window.startMs, window.pxPerDay)
      const naturalWidth = Math.max(0, rawRight - rawLeft)
      let left = rawLeft
      let width = Math.max(TIMELINE_MIN_BAR_WIDTH, naturalWidth)
      // Clip to the axis so partially-offscreen bars never paint under the sticky list.
      if (left < 0) {
        width += left
        left = 0
      }
      if (left + width > window.widthPx) {
        width = window.widthPx - left
      }
      if (width <= 0) return null
      return {
        task: item.task,
        startMs: item.startMs,
        endMs: item.endMs,
        left,
        width,
        stackIndex: stacks.get(item.task.wire.id) ?? 0,
      }
    })
    .filter((value): value is LaidOutTimelineTask => value !== null)

  const stackCount =
    laidOut.reduce((max, item) => Math.max(max, item.stackIndex + 1), 0) || 1
  const height =
    TIMELINE_LANE_CREATE_ZONE_HEIGHT +
    TIMELINE_LANE_PADDING_Y * 2 +
    stackCount * TIMELINE_CARD_HEIGHT +
    Math.max(0, stackCount - 1) * TIMELINE_CARD_GAP

  return { listId, tasks: laidOut, stackCount, height }
}

export const buildTimelineLanes = (
  listIds: string[],
  tasks: DecryptedTask[],
  window: TimelineWindow,
): TimelineLaneLayout[] => {
  const tasksByList = new Map<string, DecryptedTask[]>(
    listIds.map((listId) => [listId, []]),
  )
  for (const task of tasks) {
    const listId = task.wire.list_id
    if (!tasksByList.has(listId)) tasksByList.set(listId, [])
    tasksByList.get(listId)?.push(task)
  }
  return listIds.map((listId) =>
    layoutTimelineLane(listId, tasksByList.get(listId) ?? [], window),
  )
}

export const formatTimelineRangeFromMs = (
  startMs: number,
  endMs: number,
  level: TimelineZoomLevel,
  locale = 'it-IT',
): string => {
  const start = new Date(startMs)
  const end = new Date(Math.max(startMs, endMs - 1))

  if (level === 'hour' && toDateKey(start) === toDateKey(end)) {
    return new Intl.DateTimeFormat(locale, {
      weekday: 'short',
      day: 'numeric',
      month: 'short',
      year: 'numeric',
    }).format(start)
  }

  const dayFormatter = new Intl.DateTimeFormat(locale, { day: 'numeric' })
  const monthFormatter = new Intl.DateTimeFormat(locale, { month: 'short' })
  const yearFormatter = new Intl.DateTimeFormat(locale, { year: 'numeric' })
  const sameMonth = start.getMonth() === end.getMonth()
  const sameYear = start.getFullYear() === end.getFullYear()

  if (sameMonth && sameYear) {
    return `${dayFormatter.format(start)} – ${dayFormatter.format(end)} ${monthFormatter.format(start)} ${yearFormatter.format(start)}`
  }
  if (sameYear) {
  return `${dayFormatter.format(start)} ${monthFormatter.format(start)} – ${dayFormatter.format(end)} ${monthFormatter.format(end)} ${yearFormatter.format(end)}`
  }
  return `${dayFormatter.format(start)} ${monthFormatter.format(start)} ${yearFormatter.format(start)} – ${dayFormatter.format(end)} ${monthFormatter.format(end)} ${yearFormatter.format(end)}`
}

export const formatTimelineTime = (dueAt: string, locale = 'it-IT'): string =>
  new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(dueAt))

/**
 * Default due time when creating from a point on the continuous axis.
 * Day / tri-day use 17:00; hour keeps the clicked time.
 */
export const defaultTimelineDueDatetimeLocal = (
  value: Date,
  scaleOrLevel: number | TimelineZoomLevel = 'day',
): string => {
  const level =
    typeof scaleOrLevel === 'number'
      ? zoomLevelFromScale(scaleOrLevel)
      : scaleOrLevel
  const date = new Date(value)
  if (level !== 'hour') {
  date.setHours(17, 0, 0, 0)
  }
  const pad = (part: number) => String(part).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}

/**
 * Today marker x on the shared axis.
 * Day / tri-day: start of today.
 * Hour: exact now.
 */
export const todayLineX = (
  window: TimelineWindow,
  now = new Date(),
): number | null => {
  const markerMs =
    window.level === 'hour' ? now.getTime() : startOfDay(now).getTime()
  if (markerMs < window.startMs || markerMs > window.endMs) return null
  return timeToX(markerMs, window.startMs, window.pxPerDay)
}

// --- Legacy week helpers kept for existing unit tests / callers ---

export const getWeekDays = (anchorDate: Date): Date[] => {
  const weekStart = startOfWeek(anchorDate)
  return Array.from({ length: 7 }, (_, index) => {
    const day = new Date(weekStart)
    day.setDate(weekStart.getDate() + index)
    return day
  })
}

export interface TimelineColumn {
  key: string
  start: Date
  end: Date
}

export const getTimelineColumns = (
  anchorDate: Date,
  zoom: TimelineZoomLevel,
): TimelineColumn[] => {
  if (zoom === 'day') {
    return getWeekDays(anchorDate).map((day) => {
      const start = startOfDay(day)
      const end = new Date(start)
      end.setDate(start.getDate() + 1)
      return { key: toDateKey(start), start, end }
    })
  }
  if (zoom === 'triDay') {
    const start = startOfDay(anchorDate)
    return Array.from({ length: 7 }, (_, index) => {
      const colStart = new Date(start)
      colStart.setDate(start.getDate() + index * 3)
      const colEnd = new Date(colStart)
      colEnd.setDate(colStart.getDate() + 3)
      const lastVisible = new Date(colEnd)
      lastVisible.setDate(colEnd.getDate() - 1)
      return {
        key: `${toDateKey(colStart)}_${toDateKey(lastVisible)}`,
        start: colStart,
        end: colEnd,
      }
    })
  }
  const day = startOfDay(anchorDate)
  return Array.from({ length: 24 }, (_, hour) => {
    const start = new Date(day)
    start.setHours(hour, 0, 0, 0)
    const end = new Date(day)
    end.setHours(hour + 1, 0, 0, 0)
    return {
      key: `${toDateKey(day)}T${String(hour).padStart(2, '0')}`,
      start,
      end,
    }
  })
}
