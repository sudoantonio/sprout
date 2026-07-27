import {
  canZoomScaleIn,
  canZoomScaleOut,
  nudgeTimelineScale,
  startOfDay,
  startOfWeek,
  zoomLevelFromScale,
} from '../domain/timeline'
import { MinusIcon, PlusIcon } from './icons'

export interface BoardTimelineScaleControlsProps {
  scale: number
  onWeekAnchorChange(anchor: Date): void
  onScaleChange(scale: number): void
  onScrollToToday(): void
}

export const BoardTimelineScaleControls = ({
  scale,
  onWeekAnchorChange,
  onScaleChange,
  onScrollToToday,
}: BoardTimelineScaleControlsProps) => {
  const handleZoomIn = () => {
    if (!canZoomScaleIn(scale)) return
    onScaleChange(nudgeTimelineScale(scale, 1))
  }

  const handleZoomOut = () => {
    if (!canZoomScaleOut(scale)) return
    onScaleChange(nudgeTimelineScale(scale, -1))
  }

  const goToday = () => {
    const now = new Date()
    const level = zoomLevelFromScale(scale)
    onWeekAnchorChange(level === 'day' ? startOfWeek(now) : startOfDay(now))
    onScrollToToday()
  }

  return (
    <div
      className="board-timeline-scale-controls"
      role="group"
      aria-label="Zoom e posizione timeline"
    >
      <button
        type="button"
        className="board-timeline-nav-btn"
        aria-label="Riduci zoom"
        disabled={!canZoomScaleOut(scale)}
        onClick={handleZoomOut}
      >
        <MinusIcon className="board-timeline-nav-icon" />
      </button>
      <button type="button" className="board-timeline-today" onClick={goToday}>
        Oggi
      </button>
      <button
        type="button"
        className="board-timeline-nav-btn"
        aria-label="Aumenta zoom"
        disabled={!canZoomScaleIn(scale)}
        onClick={handleZoomIn}
      >
        <PlusIcon className="board-timeline-nav-icon" />
      </button>
    </div>
  )
}
