import type {
  AttachmentCollectionItemDto,
  EncryptedPayloadDto,
  PresetDto,
  ProjectView,
  PushSyncRequest,
  QuestionnaireDto,
  QuestionnaireVersionDto,
  TaskDto,
  TaskListDto,
  TopicDto,
  Uuid,
} from '../api/contracts'
import type { TaskListIcon } from './task-list-icon'

export type { TaskListIcon } from './task-list-icon'
export { isSameTaskListIcon, taskListIconLabel } from './task-list-icon'

export type TaskFilter = 'open' | 'today' | 'upcoming' | 'completed'
export type ResourceKind =
  | 'project'
  | 'topic'
  | 'task-list'
  | 'task'
  | 'preset'
  | 'recurrence'
  | 'questionnaire'
  | 'attachment'

export interface ProjectDocument {
  schema: 1
  name: string
}

export interface TopicDocument {
  schema: 1
  name: string
  favorite?: boolean
}

export type TaskListColumnColor =
  | 'column-white'
  | 'column-blue'
  | 'column-violet'
  | 'column-rose'
  | 'column-emerald'
  | 'column-sand'
  | 'column-slate'
  | 'column-peach'
  | 'column-mauve'

export const TASK_LIST_ICON_COLORS = [
  'column-slate',
  'column-blue',
  'column-sand',
  'column-emerald',
  'column-violet',
  'column-peach',
  'column-mauve',
  'column-rose',
] as const satisfies readonly TaskListColumnColor[]

export const TASK_LIST_COLUMN_COLORS = [
  'column-white',
  ...TASK_LIST_ICON_COLORS,
  'column-slate',
] as const satisfies readonly TaskListColumnColor[]

const LEGACY_TASK_LIST_COLUMN_COLOR_MAP: Record<string, TaskListColumnColor> = {
  blue: 'column-blue',
  azzurro: 'column-blue',
  'column-sky': 'column-blue',
  'column-cyan': 'column-blue',
  peach: 'column-peach',
  pesca: 'column-peach',
  orange: 'column-peach',
  'column-orange': 'column-peach',
  violet: 'column-violet',
  purple: 'column-violet',
  lavanda: 'column-violet',
  lavender: 'column-violet',
  'column-purple': 'column-violet',
  'column-lavender': 'column-violet',
  rose: 'column-rose',
  pink: 'column-rose',
  rosa: 'column-rose',
  'column-pink': 'column-rose',
  emerald: 'column-emerald',
  green: 'column-emerald',
  sage: 'column-emerald',
  salvia: 'column-emerald',
  'column-green': 'column-emerald',
  'column-sage': 'column-emerald',
  sand: 'column-sand',
  sabbia: 'column-sand',
  beige: 'column-sand',
  slate: 'column-slate',
  grey: 'column-slate',
  gray: 'column-slate',
  ardesia: 'column-slate',
  'column-grey': 'column-slate',
  'column-gray': 'column-slate',
  mauve: 'column-mauve',
  malva: 'column-mauve',
  'column-mauve-rose': 'column-mauve',
  white: 'column-white',
  bianco: 'column-white',
  default: 'column-white',
  neutral: 'column-white',
  none: 'column-white',
}

export function normalizeTaskListColumnColor(
  color: string | undefined,
  fallback: TaskListColumnColor = 'column-blue',
): TaskListColumnColor {
  if (!color) return fallback
  if ((TASK_LIST_COLUMN_COLORS as readonly string[]).includes(color)) {
    return color as TaskListColumnColor
  }
  return LEGACY_TASK_LIST_COLUMN_COLOR_MAP[color.toLowerCase()] ?? fallback
}

function hashStringToPaletteIndex(value: string, paletteLength: number): number {
  let hash = 0
  for (let index = 0; index < value.length; index++) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0
  }
  return hash % paletteLength
}

export function defaultTaskListColumnColor(listId: string): TaskListColumnColor {
  return TASK_LIST_ICON_COLORS[
    hashStringToPaletteIndex(listId, TASK_LIST_ICON_COLORS.length)
  ]
}

export function memberAvatarColor(identityId: string): TaskListColumnColor {
  return TASK_LIST_ICON_COLORS[
    hashStringToPaletteIndex(identityId, TASK_LIST_ICON_COLORS.length)
  ]
}

export function resolveTaskListColumnTint(
  color: TaskListColumnColor | undefined,
): TaskListColumnColor | undefined {
  if (!color || color === 'column-white') return undefined
  return color
}

export function resolveTaskListIconColorFromStored(
  storedColor: TaskListColumnColor | undefined,
  listId: string,
): TaskListColumnColor {
  if (storedColor && storedColor !== 'column-white') {
    return normalizeTaskListColumnColor(storedColor)
  }
  return defaultTaskListColumnColor(listId)
}

export function resolveTaskListDisplayColor(list: {
  document?: { color?: TaskListColumnColor }
  wire: { id: string }
}): TaskListColumnColor {
  return resolveTaskListIconColorFromStored(list.document?.color, list.wire.id)
}

const TASK_LIST_COLUMN_ACCENT_CSS_VARS: Record<TaskListColumnColor, string> = {
  'column-white': '--avatar-column-white-icon-bg',
  'column-blue': '--avatar-column-blue-icon-bg',
  'column-violet': '--avatar-column-violet-icon-bg',
  'column-rose': '--avatar-column-rose-icon-bg',
  'column-emerald': '--avatar-column-emerald-icon-bg',
  'column-sand': '--avatar-column-sand-icon-bg',
  'column-slate': '--avatar-column-slate-icon-bg',
  'column-peach': '--avatar-column-peach-icon-bg',
  'column-mauve': '--avatar-column-mauve-icon-bg',
}

export function taskListColumnColorAccentCssVar(
  color: TaskListColumnColor,
): string {
  return TASK_LIST_COLUMN_ACCENT_CSS_VARS[color]
}

export interface TaskListDocument {
  schema: 1
  name: string
  color?: TaskListColumnColor
  icon?: TaskListIcon
}

export interface TaskDocument {
  schema: 1
  title: string
  notes?: string
  /** Inclusive interval start; when set with due_at, timeline bars span start→due. */
  start_at?: string
  due_at?: string
  priority?: 'low' | 'normal' | 'high'
  recurrence?: {
    frequency: 'minutes' | 'daily' | 'weekly' | 'monthly'
    interval: number
  }
}

interface TaskCreationBase {
  title: string
  notes?: string
  questionnaireVersionId?: Uuid
  requiredAttachments?: File[]
  assigneeIdentityId?: Uuid
}

export type TaskCreationInput =
  | (TaskCreationBase & {
      taskKind: 'priority'
      priority: 'low' | 'normal' | 'high'
    })
  | (TaskCreationBase & {
      taskKind: 'deadline'
      dueAt: string
    })
  | (TaskCreationBase & {
      taskKind: 'recurring'
      dueAt: string
      frequency: 'minutes' | 'daily' | 'weekly' | 'monthly'
      interval: number
    })

export interface TaskSelectedValueDocument {
  schema: 1
  priority?: TaskDocument['priority']
  start_at?: string
  due_at?: string
  recurrence?: TaskDocument['recurrence']
}

export interface DecryptedTask {
  wire: TaskDto
  document: TaskDocument
}

export interface PresetDocument {
  schema: 1
  name: string
  description?: string
}

export interface PresetVersionDocument {
  schema: 1
  name: string
}

export interface PretaskDocument {
  schema: 1
  title: string
}

export interface PresetAssignmentDocument {
  schema: 1
  assigned_at: string
}

export interface QuestionnaireDocument {
  schema: 1
  title: string
  description?: string
}

export interface QuestionnaireVersionDocument {
  schema: 1
  description?: string
}

export interface QuestionnaireQuestionDocument {
  schema: 1
  prompt: string
}

export interface QuestionnaireOptionDocument {
  schema: 1
  label: string
}

export interface QuestionnaireAnswerDocument {
  schema: 1
  value: string | boolean | null
}

export interface DecryptedQuestionnaireVersion {
  wire: QuestionnaireVersionDto
  document: QuestionnaireVersionDocument
  questions: Array<{
    id: Uuid
    questionKind:
      | 'open'
      | 'single_choice'
      | 'multiple_choice'
      | 'boolean'
    ordinal: number
    required: boolean
    prompt: string
    options: Array<{
      id: Uuid
      ordinal: number
      label: string
    }>
  }>
}

export interface AttachmentDocument {
  schema: 1
  file_name: string
  content_type: string
}

export interface DecryptedResource<TDocument> {
  id: Uuid
  resourceId: Uuid
  parentId?: Uuid
  version: number
  document: TDocument
}

export interface LockedResource {
  id: Uuid
  resourceId: Uuid
  reason: 'missing-key' | 'vault-locked' | 'decrypt-failed'
}

export interface EncryptedLocalRecord {
  id: Uuid
  projectId: Uuid
  resourceId: Uuid
  parentId?: Uuid
  kind: ResourceKind
  aggregateVersion: number
  keyEpoch: number
  payload: EncryptedPayloadDto
  wire?:
    | ProjectView
    | TopicDto
    | TaskListDto
    | TaskDto
    | PresetDto
    | QuestionnaireDto
    | AttachmentCollectionItemDto
  updatedAt: string
}

export interface SignedQueueItem {
  id: Uuid
  request: PushSyncRequest
  restMutation?: {
    path: string
    method: 'POST' | 'PUT' | 'DELETE'
    body?: unknown
  }
  queuedAt: string
  attempts: number
  lastError?: string
}

export interface LocalTombstone {
  resourceId: Uuid
  projectId: Uuid
  aggregateVersion: number
  eventSequence: number
  recordedAt: string
}

export interface SyncConflict {
  id: Uuid
  projectId: Uuid
  resourceId: Uuid
  local: SignedQueueItem
  remotePayloadB64?: string
  remoteVersion?: number
  reason: 'stale-version' | 'stale-tombstone' | 'chain-mismatch'
  createdAt: string
}
