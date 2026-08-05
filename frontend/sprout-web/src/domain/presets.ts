import type {
  PretaskDocument,
  TaskCreationInput,
  TaskDocument,
  TaskSelectedValueDocument,
} from './models'
import { buildTaskCreation } from './tasks'

export interface ThreePretaskPresetInput {
  name: string
  priorityTitle: string
  priority: 'low' | 'normal' | 'high'
  deadlineTitle: string
  deadlineDueAt: string
  recurringTitle: string
  recurringDueAt: string
  frequency: 'minutes' | 'daily' | 'weekly' | 'monthly'
  interval: number
}

export interface BuiltPretask {
  taskKind: 'priority' | 'deadline' | 'recurring'
  template: PretaskDocument
  task: TaskDocument
  selectedValue: TaskSelectedValueDocument
}

export const buildThreePretaskPreset = (
  input: ThreePretaskPresetInput,
): { name: string; pretasks: BuiltPretask[] } => {
  const name = input.name.trim()
  if (!name) throw new Error('Preset name is required')
  const creations: TaskCreationInput[] = [
    {
      taskKind: 'priority',
      title: input.priorityTitle,
      priority: input.priority,
    },
    {
      taskKind: 'deadline',
      title: input.deadlineTitle,
      dueAt: input.deadlineDueAt,
    },
    {
      taskKind: 'recurring',
      title: input.recurringTitle,
      dueAt: input.recurringDueAt,
      frequency: input.frequency,
      interval: input.interval,
    },
  ]
  return {
    name,
    pretasks: creations.map((creation) => {
      const built = buildTaskCreation(creation)
      return {
        taskKind: built.taskKind,
        template: { schema: 1, title: built.document.title },
        task: built.document,
        selectedValue: built.selectedValue,
      }
    }),
  }
}
