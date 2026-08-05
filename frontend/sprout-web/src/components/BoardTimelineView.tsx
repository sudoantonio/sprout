import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import {
  buildTimelineBands,
  buildTimelineDayColumns,
  buildTimelineLanes,
  buildTimelineTicks,
  buildTimelineWindow,
  canZoomScaleIn,
  canZoomScaleOut,
  clampTimelineScale,
  filterTimelineTasks,
  formatTimelineRangeFromMs,
  formatTimelineTime,
  resolveTimelineMoveRange,
  resolveTimelineResizeRange,
  timeToX,
  TIMELINE_CARD_HEIGHT,
  TIMELINE_MIN_BAR_WIDTH,
  TIMELINE_WHEEL_ZOOM_FACTOR,
  timelineBarTop,
  timelineRangeToTaskTimes,
  todayLineX,
  xToTime,
  zoomLevelFromScale,
  type TimelineDayColumn,
  type TimelineWindow,
} from '../domain/timeline'
import { getTaskStatusIndicator } from '../domain/tasks'
import { resolveTaskListIconColorFromStored } from '../domain/models'
import type { DecryptedTask } from '../domain/models'
import type { TaskListItem } from '../store/app-store'
import { LockIcon, PlusIcon } from './icons'
import { BoardTimelineNav } from './BoardTimelineNav'
import { BoardTimelineScaleControls } from './BoardTimelineScaleControls'
import { TaskListAvatarContent } from './TaskListAvatarContent'

const LIST_COL_WIDTH = 200

export interface BoardTimelineViewProps {
  taskLists: TaskListItem[]
  tasks: DecryptedTask[]
  weekAnchor: Date
  scale: number
  onScaleChange(scale: number): void
  onWeekAnchorChange(anchor: Date): void
  onScrollToToday(): void
  scrollToFocusRequest?: number
  selectedTaskId?: string
  onSelectTask(id: string): void
  onCompleteTask(task: DecryptedTask): void
  onResizeTask?(
    task: DecryptedTask,
    range: { start_at: string; due_at: string },
  ): void
  onCreateTaskInDay?(
    listId: string,
    day: Date,
    anchorEl: HTMLElement,
  ): void
}

const initialFor = (label: string): string =>
  label.trim().charAt(0).toUpperCase() || '?'

export const BoardTimelineView = ({
  taskLists,
  tasks,
  weekAnchor,
  scale,
  onScaleChange,
  onWeekAnchorChange,
  onScrollToToday,
  scrollToFocusRequest,
  selectedTaskId,
  onSelectTask,
  onCompleteTask,
  onResizeTask,
  onCreateTaskInDay,
}: BoardTimelineViewProps) => {
  const timelineTasks = useMemo(() => filterTimelineTasks(tasks), [tasks])
  const window = useMemo(
    () => buildTimelineWindow(weekAnchor, scale),
    [weekAnchor, scale],
  )
  const ticks = useMemo(() => buildTimelineTicks(window), [window])
  const bands = useMemo(() => buildTimelineBands(window), [window])
  const dayColumns = useMemo(() => buildTimelineDayColumns(window), [window])
  const listIds = useMemo(
    () => taskLists.map((list) => list.wire.id),
    [taskLists],
  )
  const lanes = useMemo(
    () => buildTimelineLanes(listIds, timelineTasks, window),
    [listIds, timelineTasks, window],
  )
  const listById = useMemo(
    () => new Map(taskLists.map((list) => [list.wire.id, list])),
    [taskLists],
  )
  const nowX = useMemo(() => todayLineX(window), [window])

  const [rangeLabel, setRangeLabel] = useState('')
  const scrollRef = useRef<HTMLDivElement>(null)
  const scaleRef = useRef(scale)
  const windowRef = useRef(window)
  const centerTimeRef = useRef<Date>(weekAnchor)
  const anchorTokenRef = useRef(weekAnchor.getTime())
  const suppressScrollRef = useRef(false)

  scaleRef.current = scale
  windowRef.current = window

  const publishVisibleRange = () => {
    const node = scrollRef.current
    if (!node) return
    const win = windowRef.current
    const axisWidth = Math.max(0, node.clientWidth - LIST_COL_WIDTH)
    const startMs =
      win.startMs +
      (node.scrollLeft / Math.max(win.widthPx, 1)) * (win.endMs - win.startMs)
    const endMs =
      win.startMs +
      ((node.scrollLeft + axisWidth) / Math.max(win.widthPx, 1)) *
        (win.endMs - win.startMs)
    setRangeLabel(formatTimelineRangeFromMs(startMs, endMs, win.level))
  }

  const scrollLeftForTime = (time: Date, node: HTMLElement): number => {
    const win = windowRef.current
    const axisWidth = Math.max(0, node.clientWidth - LIST_COL_WIDTH)
    const x =
      ((time.getTime() - win.startMs) / Math.max(win.endMs - win.startMs, 1)) *
      win.widthPx
    return Math.max(0, x - axisWidth / 2)
  }

  useLayoutEffect(() => {
    const node = scrollRef.current
    if (!node) return

    const anchorChanged = anchorTokenRef.current !== weekAnchor.getTime()
    if (anchorChanged) {
      anchorTokenRef.current = weekAnchor.getTime()
      centerTimeRef.current = weekAnchor
    }

    suppressScrollRef.current = true
    node.scrollLeft = scrollLeftForTime(centerTimeRef.current, node)
    suppressScrollRef.current = false
    publishVisibleRange()
  }, [window.startMs, window.endMs, window.widthPx, weekAnchor, scale])

  useEffect(() => {
    const node = scrollRef.current
    if (!node) return

    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return
      event.preventDefault()
      const direction: 1 | -1 = event.deltaY < 0 ? 1 : -1
      if (direction > 0 && !canZoomScaleIn(scaleRef.current)) return
      if (direction < 0 && !canZoomScaleOut(scaleRef.current)) return

      const win = windowRef.current
      const axisWidth = Math.max(0, node.clientWidth - LIST_COL_WIDTH)
      const centerX = node.scrollLeft + axisWidth / 2
      centerTimeRef.current = xToTime(centerX, win.startMs, win.pxPerDay)

      const nextScale = clampTimelineScale(
        scaleRef.current * Math.exp(-event.deltaY * TIMELINE_WHEEL_ZOOM_FACTOR),
      )
      if (Math.abs(nextScale - scaleRef.current) < 0.01) return

      const prevLevel = zoomLevelFromScale(scaleRef.current)
      const nextLevel = zoomLevelFromScale(nextScale)
      onScaleChange(nextScale)
      if (nextLevel !== prevLevel) {
        onWeekAnchorChange(centerTimeRef.current)
      }
    }

    const onScroll = () => {
      if (suppressScrollRef.current) return
      const win = windowRef.current
      const axisWidth = Math.max(0, node.clientWidth - LIST_COL_WIDTH)
      const centerX = node.scrollLeft + axisWidth / 2
      centerTimeRef.current = xToTime(centerX, win.startMs, win.pxPerDay)
      publishVisibleRange()
    }

    node.addEventListener('wheel', onWheel, { passive: false })
    node.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      node.removeEventListener('wheel', onWheel)
      node.removeEventListener('scroll', onScroll)
    }
  }, [onScaleChange, onWeekAnchorChange])

  const handlePan = (direction: -1 | 1) => {
    const node = scrollRef.current
    if (!node) return
    const delta =
      direction * Math.max(240, (node.clientWidth - LIST_COL_WIDTH) * 0.85)
    node.scrollBy({ left: delta, behavior: 'smooth' })
  }

  useEffect(() => {
    if (scrollToFocusRequest === undefined) return
    const node = scrollRef.current
    if (!node) return
    centerTimeRef.current = weekAnchor
    node.scrollTo({
      left: scrollLeftForTime(weekAnchor, node),
      behavior: 'smooth',
    })
  }, [scrollToFocusRequest, weekAnchor])

  const level = window.level
  const showTaskTime = level === 'hour'

  const handleLaneCreate = (
    listId: string,
    event: ReactMouseEvent<HTMLDivElement>,
  ) => {
    if (!onCreateTaskInDay) return
    if ((event.target as HTMLElement).closest('.board-timeline-bar')) return
    const laneEl = event.currentTarget
    const rect = laneEl.getBoundingClientRect()
    const x = Math.max(0, event.clientX - rect.left)
    const at = xToTime(x, window.startMs, window.pxPerDay)
    onCreateTaskInDay(listId, at, laneEl)
  }

  return (
    <section
      className="board-timeline"
      aria-label={
        level === 'hour'
          ? 'Timeline oraria'
          : level === 'triDay'
            ? 'Timeline continua'
            : 'Timeline giornaliera'
      }
      style={
        {
          '--timeline-list-col': `${LIST_COL_WIDTH}px`,
        } as CSSProperties
      }
    >
      <div ref={scrollRef} className="board-timeline-scroll">
        {taskLists.length === 0 ? (
          <p className="board-timeline-empty-state">Nessuna task list.</p>
        ) : (
          <div
            className="board-timeline-gantt"
            style={
              {
                '--timeline-axis-width': `${window.widthPx}px`,
              } as CSSProperties
            }
          >
            <div className="board-timeline-gantt-header">
              <div
                className="board-timeline-list-label board-timeline-list-label--corner"
                aria-hidden
              />
              <div
                className="board-timeline-axis"
                style={{ width: window.widthPx }}
              >
                {bands
                  .filter((band) => band.kind !== 'divider')
                  .map((band) => (
                    <div
                      key={band.key}
                      className={`board-timeline-band board-timeline-band--${band.kind}`}
                      style={{ left: band.x, width: band.width }}
                    />
                  ))}
                {ticks.map((tick) => {
                  if (tick.kind === 'minor') {
                    return (
                      <div
                        key={tick.key}
                        className="board-timeline-tick board-timeline-tick--minor"
                        style={{ left: tick.x }}
                        aria-hidden
                      />
                    )
                  }
                  if (!tick.label || tick.x < 12) return null
                  return (
                    <div
                      key={tick.key}
                      className={[
                        'board-timeline-tick',
                        tick.isToday ? 'is-today' : '',
                        tick.isWeekend ? 'is-weekend' : '',
                        tick.x < 36 ? 'board-timeline-tick--align-start' : '',
                      ]
                        .filter(Boolean)
                        .join(' ')}
                      style={{ left: tick.x }}
                    >
                      <span className="board-timeline-tick-label">
                        {tick.label}
                      </span>
                      {tick.secondary ? (
                        <span className="board-timeline-tick-secondary">
                          {tick.secondary}
                        </span>
                      ) : null}
                    </div>
                  )
                })}
              </div>
            </div>

            {lanes.map((lane) => {
              const list = listById.get(lane.listId)
              if (!list) return null
              return (
                <div
                  key={lane.listId}
                  className="board-timeline-gantt-row"
                  style={{ minHeight: Math.max(96, lane.height) }}
                >
                  <TimelineListLabel list={list} />
                  <div
                    className="board-timeline-lane"
                    data-list-id={lane.listId}
                    style={{ width: window.widthPx, height: '100%' }}
                    onDoubleClick={(event) =>
                      handleLaneCreate(lane.listId, event)
                    }
                  >
                    {bands
                      .filter((band) => band.kind !== 'divider')
                      .map((band) => (
                        <div
                          key={`${lane.listId}-${band.key}`}
                          className={`board-timeline-band board-timeline-band--${band.kind}`}
                          style={{ left: band.x, width: band.width }}
                        />
                      ))}
                    {nowX !== null ? (
                      <div
                        className="board-timeline-today-line"
                        style={{ left: nowX }}
                        aria-hidden
                      />
                    ) : null}
                    {dayColumns.map((column) => (
                      <TimelineLaneDayCell
                        key={`${lane.listId}-${column.key}`}
                        listId={lane.listId}
                        column={column}
                        window={window}
                        onCreateTaskInDay={onCreateTaskInDay}
                      />
                    ))}
                    {lane.tasks.map((item) => (
                      <TimelineBar
                        key={item.task.wire.id}
                        task={item.task}
                        left={item.left}
                        width={item.width}
                        top={timelineBarTop(item.stackIndex)}
                        startMs={item.startMs}
                        endMs={item.endMs}
                        window={window}
                        selected={selectedTaskId === item.task.wire.id}
                        showTime={showTaskTime}
                        onSelect={() => onSelectTask(item.task.wire.id)}
                        onComplete={() => onCompleteTask(item.task)}
                        onResize={
                          onResizeTask
                            ? (range) => onResizeTask(item.task, range)
                            : undefined
                        }
                      />
                    ))}
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </div>
      <footer className="board-timeline-footer">
        <BoardTimelineNav rangeLabel={rangeLabel} onPan={handlePan} />
        <BoardTimelineScaleControls
          scale={scale}
          onScaleChange={onScaleChange}
          onWeekAnchorChange={onWeekAnchorChange}
          onScrollToToday={onScrollToToday}
        />
      </footer>
    </section>
  )
}

const TimelineLaneDayCell = ({
  listId,
  column,
  window,
  onCreateTaskInDay,
}: {
  listId: string
  column: TimelineDayColumn
  window: TimelineWindow
  onCreateTaskInDay?(
    listId: string,
    day: Date,
    anchorEl: HTMLElement,
  ): void
}) => {
  const cellRef = useRef<HTMLDivElement>(null)

  const openCreate = (clientX: number) => {
    if (!onCreateTaskInDay || !cellRef.current) return
    const laneEl = cellRef.current.parentElement
    if (!laneEl) return
    const rect = laneEl.getBoundingClientRect()
    const x = Math.max(0, Math.min(window.widthPx, clientX - rect.left))
    const at = xToTime(x, window.startMs, window.pxPerDay)
    onCreateTaskInDay(listId, at, cellRef.current)
  }

  const cellClass = [
    'board-timeline-cell',
    column.isToday ? 'is-today' : '',
  ]
    .filter(Boolean)
    .join(' ')

  if (!onCreateTaskInDay) {
    return (
      <div
        className={cellClass}
        style={{ left: column.x, width: column.width }}
        aria-hidden
      />
    )
  }

  return (
    <div
      ref={cellRef}
      className={cellClass}
      style={{ left: column.x, width: column.width }}
    >
      <button
        type="button"
        className="board-timeline-cell-add"
        aria-label="Aggiungi task"
        onClick={(event) => {
          event.stopPropagation()
          openCreate(event.clientX)
        }}
      >
        <PlusIcon aria-hidden />
      </button>
      <button
        type="button"
        className="board-timeline-cell-empty-trigger"
        aria-label="Aggiungi task in questo giorno"
        onClick={(event) => {
          event.stopPropagation()
          openCreate(event.clientX)
        }}
      />
    </div>
  )
}

const TimelineListLabel = ({ list }: { list: TaskListItem }) => {
  const listNameLabel = list.document?.name ?? 'Locked list'
  const avatarInitial = list.document
    ? initialFor(list.document.name)
    : null
  const avatarColor = resolveTaskListIconColorFromStored(
    list.document?.color,
    list.wire.id,
  )

  return (
    <div className="board-timeline-list-label">
      <span
        className={`board-avatar column board-avatar--${avatarColor}`}
        aria-hidden
      >
        {list.document ? (
          <TaskListAvatarContent
            icon={list.document.icon}
            fallbackInitial={avatarInitial}
          />
        ) : (
          <LockIcon />
        )}
      </span>
      <span className="board-timeline-list-name">{listNameLabel}</span>
    </div>
  )
}

const TimelineBar = ({
  task,
  left,
  width,
  top,
  startMs,
  endMs,
  window,
  selected,
  showTime,
  onSelect,
  onComplete,
  onResize,
}: {
  task: DecryptedTask
  left: number
  width: number
  top: number
  startMs: number
  endMs: number
  window: TimelineWindow
  selected: boolean
  showTime: boolean
  onSelect(): void
  onComplete(): void
  onResize?(range: { start_at: string; due_at: string }): void
}) => {
  const open = task.wire.state.state === 'open'
  const status = getTaskStatusIndicator(task)
  const priority = task.document.priority
  const canResize = Boolean(open && onResize && task.document.due_at)
  const canMove = Boolean(open && onResize && task.document.due_at)
  const [preview, setPreview] = useState<{
    left: number
    width: number
    startMs: number
    endMs: number
  } | null>(null)
  const dragRef = useRef<{
    mode: 'resize-start' | 'resize-end' | 'move'
    originStartMs: number
    originEndMs: number
    currentStartMs: number
    currentEndMs: number
    grabOffsetMs: number
    pointerId: number
    laneLeft: number
    originClientX: number
    originClientY: number
    active: boolean
  } | null>(null)
  /** Swallow the synthetic click that fires on the bar body after a drag. */
  const suppressSelectClickRef = useRef(false)
  const MOVE_DRAG_THRESHOLD_PX = 4

  const displayLeft = preview?.left ?? left
  const displayWidth = preview?.width ?? width
  const displayEndMs = preview?.endMs ?? endMs

  const className = [
    'board-timeline-bar',
    priority === 'high'
      ? 'priority-high'
      : priority === 'low'
        ? 'priority-low'
        : priority === 'normal'
          ? 'priority-normal'
          : 'priority-low',
    !open ? 'is-completed' : '',
    open && status.variant === 'overdue' ? 'is-overdue' : '',
    open && status.variant === 'due-soon' ? 'is-due-soon' : '',
    open && status.variant === 'due-today' ? 'is-due-today' : '',
    selected ? 'selected' : '',
    preview ? 'is-resizing' : '',
    canMove ? 'is-movable' : '',
  ]
    .filter(Boolean)
    .join(' ')
  const timeLabel =
    showTime && task.document.due_at
      ? formatTimelineTime(
          preview
            ? new Date(displayEndMs).toISOString()
            : task.document.due_at,
        )
      : null

  const geometryForRange = (nextStartMs: number, nextEndMs: number) => {
    let nextLeft = timeToX(nextStartMs, window.startMs, window.pxPerDay)
    let nextWidth = Math.max(
      TIMELINE_MIN_BAR_WIDTH,
      timeToX(nextEndMs, window.startMs, window.pxPerDay) - nextLeft,
    )
    if (nextLeft < 0) {
      nextWidth += nextLeft
      nextLeft = 0
    }
    if (nextLeft + nextWidth > window.widthPx) {
      nextWidth = window.widthPx - nextLeft
    }
    return {
      left: nextLeft,
      width: Math.max(0, nextWidth),
      startMs: nextStartMs,
      endMs: nextEndMs,
    }
  }

  const laneLeftFor = (target: HTMLElement): number | null => {
    const lane = target.closest('.board-timeline-lane')
    if (!(lane instanceof HTMLElement)) return null
    return lane.getBoundingClientRect().left
  }

  const beginDrag = (
    mode: 'resize-start' | 'resize-end' | 'move',
    event: ReactPointerEvent<HTMLElement>,
  ) => {
    if (!task.document.due_at) return
    if (mode === 'move' && !canMove) return
    if (mode !== 'move' && !canResize) return
    const immediate = mode !== 'move'
    if (immediate) {
      event.preventDefault()
      event.stopPropagation()
    }
    const laneLeft = laneLeftFor(event.currentTarget)
    if (laneLeft === null) return
    const pointerMs = xToTime(
      event.clientX - laneLeft,
      window.startMs,
      window.pxPerDay,
    ).getTime()
    if (immediate) suppressSelectClickRef.current = true
    dragRef.current = {
      mode,
      originStartMs: startMs,
      originEndMs: endMs,
      currentStartMs: startMs,
      currentEndMs: endMs,
      grabOffsetMs: pointerMs - startMs,
      pointerId: event.pointerId,
      laneLeft,
      originClientX: event.clientX,
      originClientY: event.clientY,
      active: immediate,
    }
    event.currentTarget.setPointerCapture(event.pointerId)
    if (immediate) setPreview(geometryForRange(startMs, endMs))
  }

  const onDragPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    const drag = dragRef.current
    if (!drag || event.pointerId !== drag.pointerId) return
    event.preventDefault()
    event.stopPropagation()
    if (!drag.active) {
      const dx = event.clientX - drag.originClientX
      const dy = event.clientY - drag.originClientY
      if (Math.hypot(dx, dy) < MOVE_DRAG_THRESHOLD_PX) return
      drag.active = true
      suppressSelectClickRef.current = true
      event.preventDefault()
      event.stopPropagation()
      setPreview(geometryForRange(drag.originStartMs, drag.originEndMs))
    }
    const x = event.clientX - drag.laneLeft
    const pointerMs = xToTime(x, window.startMs, window.pxPerDay).getTime()
    const next =
      drag.mode === 'move'
        ? resolveTimelineMoveRange(
            pointerMs,
            drag.grabOffsetMs,
            drag.originStartMs,
            drag.originEndMs,
            window.level,
          )
        : resolveTimelineResizeRange(
            drag.mode === 'resize-start' ? 'start' : 'end',
            pointerMs,
            drag.originStartMs,
            drag.originEndMs,
            window.level,
          )
    drag.currentStartMs = next.startMs
    drag.currentEndMs = next.endMs
    setPreview(geometryForRange(next.startMs, next.endMs))
  }

  const finishDrag = (
    event: ReactPointerEvent<HTMLElement>,
    commit: boolean,
  ) => {
    const drag = dragRef.current
    if (!drag || event.pointerId !== drag.pointerId) return
    if (drag.active) {
      event.preventDefault()
      event.stopPropagation()
    }
    dragRef.current = null
    try {
      event.currentTarget.releasePointerCapture(event.pointerId)
    } catch {
      // already released
    }
    const dueAt = task.document.due_at
    if (commit && dueAt && drag.active) {
      const timeChanged =
        drag.currentStartMs !== drag.originStartMs ||
        drag.currentEndMs !== drag.originEndMs
      if (timeChanged && onResize) {
        onResize(
          timelineRangeToTaskTimes(
            drag.currentStartMs,
            drag.currentEndMs,
            window.level,
            {
              startAt: task.document.start_at,
              dueAt,
            },
          ),
        )
      }
    }
    setPreview(null)
    // Backup clear if the trailing click never hits this bar.
    globalThis.setTimeout(() => {
      suppressSelectClickRef.current = false
    }, 100)
  }

  return (
    <div
      className={className}
      style={{
        left: displayLeft,
        width: displayWidth,
        top,
        height: TIMELINE_CARD_HEIGHT,
      }}
      onClickCapture={(event) => {
        if (!suppressSelectClickRef.current) return
        event.preventDefault()
        event.stopPropagation()
        suppressSelectClickRef.current = false
      }}
    >
      {canResize ? (
        <span
          className="board-timeline-bar-handle board-timeline-bar-handle--start"
          role="separator"
          aria-orientation="vertical"
          aria-label="Ridimensiona inizio task"
          onPointerDown={(event) => beginDrag('resize-start', event)}
          onPointerMove={onDragPointerMove}
          onPointerUp={(event) => finishDrag(event, true)}
          onPointerCancel={(event) => finishDrag(event, false)}
          onClick={(event) => event.stopPropagation()}
        />
      ) : null}
      <label
        className={`board-task-check board-timeline-bar-check board-task-check--${status.variant}`}
        title={status.label}
        onClick={(event) => event.stopPropagation()}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        <input
          type="checkbox"
          checked={!open}
          disabled={!open || !task.wire.active_assignment_id}
          aria-label={`${status.label}: ${task.document.title}`}
          onChange={(event) => {
            event.stopPropagation()
            if (open) onComplete()
          }}
          onClick={(event) => event.stopPropagation()}
        />
        <span
          className="board-task-check-dot"
          style={
            status.dueProgress !== undefined
              ? ({
                  '--task-due-progress': status.dueProgress,
                } as CSSProperties)
              : undefined
          }
          aria-hidden
        />
      </label>
      <button
        type="button"
        className="board-timeline-bar-body"
        onClick={onSelect}
        onPointerDown={
          canMove ? (event) => beginDrag('move', event) : undefined
        }
        onPointerMove={canMove ? onDragPointerMove : undefined}
        onPointerUp={canMove ? (event) => finishDrag(event, true) : undefined}
        onPointerCancel={
          canMove ? (event) => finishDrag(event, false) : undefined
        }
      >
        {timeLabel ? (
          <span className="board-timeline-bar-time">{timeLabel}</span>
        ) : null}
        <span className="board-timeline-bar-title">{task.document.title}</span>
      </button>
      {canResize ? (
        <span
          className="board-timeline-bar-handle board-timeline-bar-handle--end"
          role="separator"
          aria-orientation="vertical"
          aria-label="Ridimensiona fine task"
          onPointerDown={(event) => beginDrag('resize-end', event)}
          onPointerMove={onDragPointerMove}
          onPointerUp={(event) => finishDrag(event, true)}
          onPointerCancel={(event) => finishDrag(event, false)}
          onClick={(event) => event.stopPropagation()}
        />
      ) : null}
    </div>
  )
}
