import { useReducer } from 'react'
import type {
  ProjectRecoveryStatus,
  ProjectView,
  SessionResponse,
  TaskDto,
  TaskListDto,
  TopicDto,
  Uuid,
} from '../api/contracts'
import type {
  DecryptedTask,
  ProjectDocument,
  SyncConflict,
  TaskFilter,
  TaskListDocument,
  TopicDocument,
} from '../domain/models'
import type { VaultPersistence } from '../security/key-vault'
import type { WakeStatus } from '../sync/sync-engine'

export type AppScreen =
  | 'tasks'
  | 'people'
  | 'presets'
  | 'questionnaires'
  | 'attachments'
  | 'recovery'
  | 'retention'
  | 'security'
  | 'conflicts'

export interface ProjectItem {
  wire: ProjectView
  document?: ProjectDocument
  lockedReason?: string
}

export interface TopicItem {
  wire: TopicDto
  document?: TopicDocument
  lockedReason?: string
}

export interface TaskListItem {
  wire: TaskListDto
  document?: TaskListDocument
  lockedReason?: string
}

export interface AppState {
  session?: SessionResponse
  localAccess?: {
    deviceId: Uuid
    identityId?: Uuid
  }
  phase:
    | 'signed-out'
    | 'authenticating'
    | 'locked'
    | 'local-ready'
    | 'ready'
  screen: AppScreen
  projects: ProjectItem[]
  topics: TopicItem[]
  taskLists: TaskListItem[]
  tasks: DecryptedTask[]
  lockedTasks: TaskDto[]
  selectedProjectId?: Uuid
  selectedTopicId?: Uuid
  selectedListId?: Uuid
  selectedTaskId?: Uuid
  taskFilter: TaskFilter
  loading: boolean
  error?: string
  notice?: string
  online: boolean
  queueCount: number
  conflicts: SyncConflict[]
  wakeStatus: WakeStatus
  storagePersistence: 'unknown' | 'granted' | 'not-granted'
  vaultPersistence: VaultPersistence
  recoveryStatus?: ProjectRecoveryStatus
}

export type AppAction =
  | { type: 'auth-started' }
  | {
      type: 'session-ready'
      session: SessionResponse
      vaultPersistence: VaultPersistence
    }
  | { type: 'vault-locked'; session: SessionResponse; message: string }
  | {
      type: 'local-vault-ready'
      deviceId: Uuid
      identityId?: Uuid
    }
  | { type: 'logout' }
  | { type: 'set-screen'; screen: AppScreen }
  | { type: 'set-loading'; value: boolean }
  | { type: 'set-error'; message?: string }
  | { type: 'set-notice'; message?: string }
  | { type: 'set-online'; value: boolean }
  | { type: 'set-queue-count'; count: number }
  | { type: 'set-conflicts'; conflicts: SyncConflict[] }
  | { type: 'set-wake-status'; status: WakeStatus }
  | {
      type: 'set-storage-persistence'
      value: AppState['storagePersistence']
    }
  | { type: 'set-vault-persistence'; value: VaultPersistence }
  | { type: 'set-projects'; projects: ProjectItem[] }
  | { type: 'select-project'; projectId: Uuid }
  | { type: 'set-topics'; topics: TopicItem[] }
  | { type: 'select-topic'; topicId: Uuid }
  | { type: 'set-task-lists'; taskLists: TaskListItem[] }
  | { type: 'select-list'; listId: Uuid }
  | {
      type: 'set-tasks'
      tasks: DecryptedTask[]
      lockedTasks: TaskDto[]
    }
  | { type: 'select-task'; taskId?: Uuid }
  | { type: 'set-task-filter'; filter: TaskFilter }
  | { type: 'set-recovery'; status?: ProjectRecoveryStatus }

export const createInitialAppState = (): AppState => ({
  phase: 'signed-out',
  screen: 'tasks',
  projects: [],
  topics: [],
  taskLists: [],
  tasks: [],
  lockedTasks: [],
  taskFilter: 'open',
  loading: false,
  online: typeof navigator === 'undefined' ? true : navigator.onLine,
  queueCount: 0,
  conflicts: [],
  wakeStatus: 'disconnected',
  storagePersistence: 'unknown',
  vaultPersistence: 'locked',
})

export const appReducer = (state: AppState, action: AppAction): AppState => {
  switch (action.type) {
    case 'auth-started':
      return { ...state, phase: 'authenticating', error: undefined }
    case 'session-ready':
      return {
        ...state,
        session: action.session,
        localAccess: undefined,
        phase: 'ready',
        vaultPersistence: action.vaultPersistence,
        error: undefined,
      }
    case 'vault-locked':
      return {
        ...state,
        session: action.session,
        phase: 'locked',
        vaultPersistence: 'locked',
        error: action.message,
      }
    case 'local-vault-ready':
      return {
        ...state,
        session: undefined,
        localAccess: {
          deviceId: action.deviceId,
          identityId: action.identityId,
        },
        phase: 'local-ready',
        vaultPersistence: 'prf-wrapped',
        error: undefined,
        notice:
          'Local vault unlocked. Server sync remains paused until you sign in again.',
      }
    case 'logout':
      return createInitialAppState()
    case 'set-screen':
      return { ...state, screen: action.screen, error: undefined }
    case 'set-loading':
      return { ...state, loading: action.value }
    case 'set-error':
      return {
        ...state,
        error: action.message,
        loading: false,
        phase:
          state.phase === 'authenticating' ? 'signed-out' : state.phase,
      }
    case 'set-notice':
      return { ...state, notice: action.message }
    case 'set-online':
      return { ...state, online: action.value }
    case 'set-queue-count':
      return { ...state, queueCount: Math.max(0, action.count) }
    case 'set-conflicts':
      return { ...state, conflicts: action.conflicts }
    case 'set-wake-status':
      return { ...state, wakeStatus: action.status }
    case 'set-storage-persistence':
      return { ...state, storagePersistence: action.value }
    case 'set-vault-persistence':
      return { ...state, vaultPersistence: action.value }
    case 'set-projects':
      return {
        ...state,
        projects: action.projects,
        selectedProjectId:
          state.selectedProjectId ?? action.projects[0]?.wire.id,
        loading: false,
      }
    case 'select-project':
      return {
        ...state,
        selectedProjectId: action.projectId,
        selectedTopicId: undefined,
        selectedListId: undefined,
        selectedTaskId: undefined,
        topics: [],
        taskLists: [],
        tasks: [],
        lockedTasks: [],
      }
    case 'set-topics':
      return {
        ...state,
        topics: action.topics,
        selectedTopicId: action.topics[0]?.wire.id,
        loading: false,
      }
    case 'select-topic':
      return {
        ...state,
        selectedTopicId: action.topicId,
        selectedListId: undefined,
        selectedTaskId: undefined,
        taskLists: [],
        tasks: [],
        lockedTasks: [],
      }
    case 'set-task-lists':
      return {
        ...state,
        taskLists: action.taskLists,
        selectedListId: action.taskLists[0]?.wire.id,
        loading: false,
      }
    case 'select-list':
      return {
        ...state,
        selectedListId: action.listId,
        selectedTaskId: undefined,
        tasks: [],
        lockedTasks: [],
      }
    case 'set-tasks':
      return {
        ...state,
        tasks: action.tasks,
        lockedTasks: action.lockedTasks,
        selectedTaskId:
          state.selectedTaskId ?? action.tasks[0]?.wire.id,
        loading: false,
      }
    case 'select-task':
      return { ...state, selectedTaskId: action.taskId }
    case 'set-task-filter':
      return { ...state, taskFilter: action.filter }
    case 'set-recovery':
      return { ...state, recoveryStatus: action.status }
  }
}

export const useAppStore = () =>
  useReducer(appReducer, undefined, createInitialAppState)
