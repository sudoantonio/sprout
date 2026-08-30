import type { TaskListIcon } from '../domain/task-list-icon'
import { TaskListGlyphIcon } from './TaskListGlyphIcon'

export const TaskListAvatarContent = ({
  icon,
  fallbackInitial,
}: {
  icon: TaskListIcon | undefined
  fallbackInitial: string | null
}) => {
  if (icon?.kind === 'emoji') {
    return <span className="board-avatar-emoji">{icon.value}</span>
  }
  if (icon?.kind === 'glyph') {
    return <TaskListGlyphIcon glyphId={icon.id} className="board-avatar-glyph" />
  }
  if (icon?.kind === 'letter') {
    return icon.value
  }
  return fallbackInitial
}
