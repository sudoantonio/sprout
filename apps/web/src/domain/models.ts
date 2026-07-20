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
}

export interface TaskListDocument {
  schema: 1
  name: string
}

export interface TaskDocument {
  schema: 1
  title: string
  notes?: string
  due_at?: string
  priority?: 'low' | 'normal' | 'high'
  recurrence?: {
    frequency: 'daily' | 'weekly' | 'monthly'
    interval: number
  }
}

interface TaskCreationBase {
  title: string
  questionnaireVersionId?: Uuid
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
      frequency: 'daily' | 'weekly' | 'monthly'
      interval: number
    })

export interface TaskSelectedValueDocument {
  schema: 1
  priority?: TaskDocument['priority']
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
