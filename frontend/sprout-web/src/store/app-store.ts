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
  | 'ai'
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

export interface BoardMember {
  identityId: Uuid
  label: string
}

export type BoardFocus =
  | { type: 'generali' }
  | { type: 'members' }
  | { type: 'member'; identityId: Uuid }
  | { type: 'agents' }
  | { type: 'agent'; agentId: Uuid }
  | { type: 'topic'; topicId: Uuid }

export type BoardViewMode = 'overview' | 'board' | 'timeline' | 'history'

const BOARD_VIEW_MODE_KEY = 'sprout-board-view-mode'

export const readBoardViewMode = (): BoardViewMode => {
  try {
    const value = localStorage.getItem(BOARD_VIEW_MODE_KEY)
    return value === 'overview' ||
      value === 'board' ||
      value === 'timeline' ||
      value === 'history'
      ? value
      : 'overview'
  } catch {
    return 'overview'
  }
}

export const persistBoardViewMode = (mode: BoardViewMode): void => {
  try {
    localStorage.setItem(BOARD_VIEW_MODE_KEY, mode)
  } catch {
    // ignore storage failures
  }
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
  boardMembers: BoardMember[]
  boardFocus: BoardFocus
  boardViewMode: BoardViewMode
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
  | { type: 'set-board-focus'; focus: BoardFocus }
  | { type: 'set-board-view-mode'; mode: BoardViewMode }
  | { type: 'set-board-members'; members: BoardMember[] }
  | { type: 'set-task-lists'; taskLists: TaskListItem[] }
  | { type: 'select-list'; listId: Uuid }
  | {
      type: 'set-tasks'
      tasks: DecryptedTask[]
      lockedTasks: TaskDto[]
    }
  | { type: 'upsert-task'; task: DecryptedTask }
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
  boardMembers: [],
  boardFocus: { type: 'generali' },
  boardViewMode: readBoardViewMode(),
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
        boardFocus: { type: 'generali' },
        boardViewMode: 'overview',
        boardMembers: [],
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
        selectedTopicId:
          state.boardFocus.type === 'topic'
            ? state.boardFocus.topicId
            : (state.selectedTopicId ?? action.topics[0]?.wire.id),
        loading: false,
      }
    case 'select-topic':
      return {
        ...state,
        boardFocus: { type: 'topic', topicId: action.topicId },
        selectedTopicId: action.topicId,
        selectedListId: undefined,
        selectedTaskId: undefined,
      }
    case 'set-board-focus': {
      const selectedTopicId =
        action.focus.type === 'topic'
          ? action.focus.topicId
          : state.selectedTopicId
      return {
        ...state,
        boardFocus: action.focus,
        selectedTopicId,
        selectedListId: undefined,
        selectedTaskId: undefined,
      }
    }
    case 'set-board-view-mode':
      persistBoardViewMode(action.mode)
      return { ...state, boardViewMode: action.mode }
    case 'set-board-members':
      return { ...state, boardMembers: action.members }
    case 'set-task-lists':
      return {
        ...state,
        taskLists: action.taskLists,
        selectedListId:
          state.selectedListId &&
          action.taskLists.some(
            (list) => list.wire.id === state.selectedListId,
          )
            ? state.selectedListId
            : action.taskLists[0]?.wire.id,
        loading: false,
      }
    case 'select-list':
      return {
        ...state,
        selectedListId: action.listId,
        selectedTaskId: undefined,
      }
    case 'set-tasks':
      return {
        ...state,
        tasks: action.tasks,
        lockedTasks: action.lockedTasks,
        selectedTaskId:
          state.selectedTaskId &&
          action.tasks.some((task) => task.wire.id === state.selectedTaskId)
            ? state.selectedTaskId
            : undefined,
        loading: false,
      }
    case 'upsert-task': {
      const exists = state.tasks.some(
        (task) => task.wire.id === action.task.wire.id,
      )
      return {
        ...state,
        tasks: exists
          ? state.tasks.map((task) =>
              task.wire.id === action.task.wire.id ? action.task : task,
            )
          : [...state.tasks, action.task],
      }
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
