export type TaskListIcon =
  | { kind: 'glyph'; id: string }
  | { kind: 'emoji'; value: string }
  | { kind: 'letter'; value: string }

export function isSameTaskListIcon(
  left: TaskListIcon | undefined,
  right: TaskListIcon | undefined,
): boolean {
  if (!left && !right) return true
  if (!left || !right) return false
  if (left.kind !== right.kind) return false
  if (left.kind === 'emoji') {
    return left.value === (right as Extract<TaskListIcon, { kind: 'emoji' }>).value
  }
  if (left.kind === 'letter') {
    return left.value === (right as Extract<TaskListIcon, { kind: 'letter' }>).value
  }
  return left.id === (right as Extract<TaskListIcon, { kind: 'glyph' }>).id
}

export function taskListIconLabel(icon: TaskListIcon | undefined): string {
  if (!icon) return 'Iniziale'
  if (icon.kind === 'emoji') return icon.value
  if (icon.kind === 'letter') return icon.value
  return icon.id
}
