import { ChevronIcon } from './icons'

export interface BoardTimelineNavProps {
  rangeLabel: string
  onPan(direction: -1 | 1): void
}

export const BoardTimelineNav = ({
  rangeLabel,
  onPan,
}: BoardTimelineNavProps) => {
  return (
    <div className="board-timeline-nav" aria-label="Navigazione timeline">
      <button
        type="button"
        className="board-timeline-nav-btn"
        aria-label="Scorri indietro"
        onClick={() => onPan(-1)}
      >
        <ChevronIcon className="board-timeline-nav-icon board-timeline-nav-icon--prev" />
      </button>
      <p className="board-timeline-range">{rangeLabel || '—'}</p>
      <button
        type="button"
        className="board-timeline-nav-btn"
        aria-label="Scorri avanti"
        onClick={() => onPan(1)}
      >
        <ChevronIcon className="board-timeline-nav-icon" />
      </button>
    </div>
  )
}
