import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from 'react'
import './App.css'
import { ApiClient, ApiError } from './api/client'
import { authErrorMessage } from './security/auth-errors'
import type {
  AgentDirectoryItemDto,
  AttachmentCollectionItemDto,
  EncryptedPayloadDto,
  ParticipantSuggestionDto,
  PermissionGrantDto,
  ProjectInvitationDto,
  ProjectView,
  ProvisionAgentResponse,
  QuestionnaireDto,
  QuestionnaireSubmissionDto,
  QuestionnaireVersionDto,
  RetentionArchiveDto,
  RetentionWarningDto,
  SessionResponse,
  TaskDto,
  TaskListDto,
  TopicDto,
  Uuid,
} from './api/contracts'
import {
  asAttachmentCiphertext,
  attachmentCiphertextSha256,
  decryptAttachment,
  encryptAttachment,
} from './attachments/crypto'
import {
  enqueueCompletedAttachment,
  flushCompletedAttachmentQueue,
} from './attachments/offline-queue'
import { AuthScreen } from './components/AuthScreen'
import {
  AlertTriangleIcon,
  DownloadIcon,
  KeyIcon,
  ShieldIcon,
  WifiOffIcon,
} from './components/icons'
import {
  AttachmentScreen,
  ConflictScreen,
  ProjectPeopleScreen,
  RecoveryScreen,
  RetentionScreen,
  SecurityScreen,
} from './components/ResourceScreens'
import {
  QuestionnaireScreen,
  type QuestionnaireAnswerValue,
} from './components/QuestionnaireScreen'
import { TasksScreen } from './components/TasksScreen'
import { WorkspaceUserMenu } from './components/WorkspaceUserMenu'
import { AiGenerationScreen } from './components/AiGenerationScreen'
import { useTheme } from './hooks/useTheme'
import {
  PresetScreen,
  type PresetMaterializationInput,
} from './components/PresetScreen'
import {
  createEncryptedResource,
  createEncryptedResourceHeader,
  decodePayloadContainer,
  decryptProject,
  decryptInfoDocument,
  decryptTask,
  decryptTaskList,
  decryptTopic,
  encodePayloadContainer,
  encryptExistingResource,
  encryptInfoDocument,
  INITIAL_PAYLOAD_VERSION,
  resolveActiveResourceKey,
  synchronizeProjectRootKey,
} from './domain/resources'
import type {
  DecryptedTask,
  DecryptedInfoDocument,
  EncryptedLocalRecord,
  PresetDocument,
  ProjectDocument,
  QuestionnaireDocument,
  AttachmentDocument,
  DecryptedQuestionnaireVersion,
  QuestionnaireAnswerDocument,
  SyncConflict,
  TaskCreationInput,
  TaskDocument,
  TaskSelectedValueDocument,
  TaskListDocument,
  InfoDocumentContent,
  InfoFileBlock,
  TopicDocument,
} from './domain/models'
import { isSameTaskListIcon } from './domain/models'
import {
  buildAssigneeSubmissionRequest,
  decryptQuestionnaireVersion,
  encryptQuestionnaireVersion,
  QUESTIONNAIRE_SUBMISSION_SIGNATURE_CONTEXT,
  questionnaireSubmissionSigningMessage,
  selectImmutableQuestionnaireVersion,
  validateQuestionnaireAnswers,
  type QuestionnaireEditorQuestion,
} from './domain/questionnaires'
import { submitQuestionnaireRecoveringLostResponse } from './domain/questionnaire-submission'
import {
  buildInitialResourceEpoch,
  buildResourceEpochRotation,
  buildResourceKeyEnvelopes,
  importResourceKeyEnvelopes,
} from './domain/envelopes'
import { buildNextRecurringTask, buildTaskCreation } from './domain/tasks'
import { availableRetentionArchiveCount } from './domain/retention'
import {
  buildThreePretaskPreset,
} from './domain/presets'
import { saveWithDownloadFallback } from './downloads/download'
import { requestPersistentStorage } from './pwa'
import { AuthController } from './security/auth-controller'
import {
  clearDevSession,
  loadDevSession,
} from './security/dev-session'
import {
  countBackupHitsForResources,
  countDevResourceKeyBackup,
  hasDevResourceKeyBackup,
  mergeDevResourceKeysIntoSnapshot,
  persistDevVault,
  purgeZeroDevResourceKeys,
  restoreAllDevResourceKeys,
  restoreDevResourceKeys,
} from './security/dev-resource-keys'
import {
  base64ToBytes,
  bytesToBase64,
  combineRecoverySecret,
  decryptDocument,
  loadCrypto,
  signDual,
  zeroBytes,
} from './security/wasm'
import { getOrCreateDeviceId } from './storage/device'
import { EncryptedDatabase } from './storage/encrypted-db'
import {
  readEncryptedAttachment,
  removeEncryptedAttachment,
  writeEncryptedAttachment,
} from './storage/opfs'
import {
  createSignedQueueItem,
  SyncEngine,
  SyncWakeClient,
} from './sync/sync-engine'
import {
  type AppScreen,
  type BoardFocus,
  type BoardViewMode,
  type BoardMember,
  type ProjectItem,
  type TaskListItem,
  type TopicItem,
  useAppStore,
} from './store/app-store'

interface Services {
  database: EncryptedDatabase
  auth: AuthController
  sync: SyncEngine
  wake: SyncWakeClient
}

interface QuestionnaireItem {
  wire: QuestionnaireDto
  document?: QuestionnaireDocument
  lockedReason?: string
}

interface AppProps {
  apiClient?: ApiClient
  initialSession?: SessionResponse
}

const errorMessage = (error: unknown): string => {
  if (error instanceof ApiError && error.status === 429) {
    return 'Troppe richieste, riprova tra qualche secondo'
  }
  return error instanceof Error ? error.message : 'An unexpected error occurred'
}

const screenTitles: Record<AppScreen, string> = {
  tasks: 'Board',
  people: 'Persone',
  presets: 'Preset',
  questionnaires: 'Questionari',
  attachments: 'Allegati',
  recovery: 'Recovery',
  retention: 'Retention',
  conflicts: 'Conflitti',
  security: 'Sicurezza',
  ai: 'AI / Generazione testo',
}

const asLockedReason = (error: unknown): string =>
  error instanceof Error && error.message.includes('not available')
    ? 'Missing resource key on this device'
    : 'Ciphertext could not be authenticated'

const projectRecord = (
  project: ProjectView,
  payload: EncryptedPayloadDto,
) => ({
  id: project.id,
  projectId: project.id,
  resourceId: project.id,
  kind: 'project' as const,
  aggregateVersion: 0,
  keyEpoch: project.key_epoch,
  payload,
  wire: project,
  updatedAt: project.updated_at,
})

/** Reload DEV backups + server envelopes so body/header slots are available. */
const recoverProjectResourceKeys = async (
  api: ApiClient,
  services: Services,
  projectId: Uuid,
  identityId?: Uuid,
): Promise<number> => {
  if (import.meta.env.DEV) {
    if (identityId) {
      await restoreDevResourceKeys(identityId, services.auth.vault)
    }
    await restoreAllDevResourceKeys(services.auth.vault)
  }
  const [envelopeResponse, packages] = await Promise.all([
    api.listResourceKeyEnvelopes(projectId),
    api.listProjectDevicePackages(projectId),
  ])
  return importResourceKeyEnvelopes(services.auth.vault, {
    projectId,
    envelopes: envelopeResponse.envelopes,
    packages,
  })
}

/** Prefer wire epoch; fall back to any restored/backed-up header epoch. */
const resolveHierarchyHeaderKey = (
  vault: {
    getHeaderKey: (resourceId: Uuid, epoch?: number) => Uint8Array | undefined
    getLatestHeaderKey: (
      resourceId: Uuid,
    ) => { epoch: number; key: Uint8Array } | undefined
  },
  resourceId: Uuid,
  preferredEpoch: number,
): { epoch: number; key: Uint8Array } | undefined => {
  const exact = vault.getHeaderKey(resourceId, preferredEpoch)
  if (exact) return { epoch: preferredEpoch, key: exact }
  const latest = vault.getLatestHeaderKey(resourceId)
  if (latest) return latest
  if (preferredEpoch !== 1) {
    const genesis = vault.getHeaderKey(resourceId, 1)
    if (genesis) return { epoch: 1, key: genesis }
  }
  return undefined
}

/**
 * Resolve a body key for encrypt/update: vault → envelope/DEV restore →
 * (DEV) mint a replacement when plaintext is already in memory but the body
 * slot was lost/zero-purged (header-only display still works in that state).
 */
const ensureActiveResourceKey = async (
  api: ApiClient,
  services: Services,
  session: SessionResponse,
  projectId: Uuid,
  resourceId: Uuid,
  preferredEpoch: number,
  missingMessage: string,
  allowDevelopmentMint = true,
): Promise<{ epoch: number; key: Uint8Array }> => {
  const vault = services.auth.vault
  let resolved = resolveActiveResourceKey(vault, resourceId, preferredEpoch)
  if (!resolved) {
    try {
      await recoverProjectResourceKeys(
        api,
        services,
        projectId,
        session.identity_id,
      )
      if (import.meta.env.DEV) {
        persistDevVault(session, vault)
      }
    } catch {
      // Envelope restore may be offline; DEV mint below can still unblock edits.
    }
    resolved = resolveActiveResourceKey(vault, resourceId, preferredEpoch)
  }
  if (!resolved && import.meta.env.DEV && allowDevelopmentMint) {
    // Body keys are often missing after zero-key purge while header keys
    // remain (board still decrypts via header / DEV zero-key fallback).
    // Mint a replacement at the wire epoch so this device can rewrite payload.
    const minted = crypto.getRandomValues(new Uint8Array(32))
    try {
      await vault.putResourceKey(resourceId, minted, preferredEpoch, 'body')
      persistDevVault(session, vault)
      resolved = { epoch: preferredEpoch, key: minted.slice() }
    } finally {
      zeroBytes(minted)
    }
  }
  if (!resolved) throw new Error(missingMessage)
  return resolved
}

const hydrateServerProject = async (
  api: ApiClient,
  services: Services,
  project: ProjectView,
  identityId?: Uuid,
): Promise<ProjectItem> => {
  const payload = decodePayloadContainer(project.encrypted_metadata_b64)
  await putRestRecord(
    services.database,
    projectRecord(project, payload),
  )
  try {
    const imported = await recoverProjectResourceKeys(
      api,
      services,
      project.id,
      identityId,
    )
    const hasProjectRootKey = await synchronizeProjectRootKey(
      services.auth.vault,
      project,
    )
    if (
      !hasProjectRootKey &&
      imported === 0 &&
      !services.auth.vault.getResourceKey(project.id, project.key_epoch)
    ) {
      throw new Error(
        'No project keys on this device (no matching key envelopes). Sign in with the original device keys or use Recovery.',
      )
    }
    const document = await decryptProject(project, services.auth.vault)
    return {
      wire: project,
      document,
    }
  } catch (error) {
    return { wire: project, lockedReason: errorMessage(error) }
  }
}

const availableCiphertext = (
  body: EncryptedPayloadDto | null,
  header?: EncryptedPayloadDto | null,
): EncryptedPayloadDto => {
  const payload = body ?? header
  if (!payload) throw new Error('Resource response contains no ciphertext')
  return payload
}

const topicRecord = (topic: TopicDto) => ({
  id: topic.resource_node_id,
  projectId: topic.project_id,
  resourceId: topic.resource_node_id,
  kind: 'topic' as const,
  aggregateVersion: 0,
  keyEpoch: topic.key_epoch,
  payload: availableCiphertext(topic.payload, topic.header),
  wire: topic,
  updatedAt: topic.created_at,
})

const listRecord = (list: TaskListDto) => ({
  id: list.resource_node_id,
  projectId: list.project_id,
  resourceId: list.resource_node_id,
  parentId: list.topic_id,
  kind: 'task-list' as const,
  aggregateVersion: 0,
  keyEpoch: list.key_epoch,
  payload: availableCiphertext(list.payload, list.header),
  wire: list,
  updatedAt: list.created_at,
})

const taskRecord = (task: TaskDto) => ({
  id: task.resource_node_id,
  projectId: task.project_id,
  resourceId: task.resource_node_id,
  parentId: task.list_id,
  kind: 'task' as const,
  aggregateVersion: 0,
  keyEpoch: task.key_epoch,
  payload: availableCiphertext(task.payload, task.header),
  wire: task,
  updatedAt: task.created_at,
})

const putRestRecord = async (
  database: EncryptedDatabase,
  record: EncryptedLocalRecord,
): Promise<number> => {
  const current = await database.getRecord(record.id)
  const aggregateVersion = current?.aggregateVersion ?? 0
  await database.putRecord({ ...record, aggregateVersion })
  return aggregateVersion
}

const App = ({ apiClient, initialSession }: AppProps) => {
  const api = useMemo(() => apiClient ?? new ApiClient(), [apiClient])
  const { appearance, setAppearance } = useTheme()
  const [state, dispatch] = useAppStore()
  const [services, setServices] = useState<Services>()
  const [offlineVaultAvailable, setOfflineVaultAvailable] = useState(false)
  const [projectName, setProjectName] = useState('')
  const [presetResult, setPresetResult] = useState<{
    id: Uuid
    name?: string
    locked?: boolean
    detail?: string
  }>()
  const [questionnaires, setQuestionnaires] = useState<QuestionnaireItem[]>([])
  const [questionnaireVersions, setQuestionnaireVersions] = useState<
    DecryptedQuestionnaireVersion[]
  >([])
  const [selectedQuestionnaireId, setSelectedQuestionnaireId] =
    useState<Uuid>()
  const [taskQuestionnaireVersion, setTaskQuestionnaireVersion] =
    useState<DecryptedQuestionnaireVersion>()
  const [questionnaireSubmission, setQuestionnaireSubmission] =
    useState<QuestionnaireSubmissionDto>()
  const [questionnaireSubmissionAnswers, setQuestionnaireSubmissionAnswers] =
    useState<Record<Uuid, QuestionnaireAnswerValue>>({})
  const [attachments, setAttachments] = useState<
    AttachmentCollectionItemDto[]
  >([])
  const [attachmentLabels, setAttachmentLabels] = useState<
    Record<string, string>
  >({})
  const [invitations, setInvitations] = useState<ProjectInvitationDto[]>([])
  const [managedResourceGrants, setManagedResourceGrants] = useState<
    Array<{
      topicName: string
      resourceId: Uuid
      grant: PermissionGrantDto
    }>
  >([])
  const [participantSuggestions, setParticipantSuggestions] = useState<
    ParticipantSuggestionDto[]
  >([])
  const [autoExport, setAutoExport] = useState<boolean>()
  const [archives, setArchives] = useState<RetentionArchiveDto[]>([])
  const [retentionWarnings, setRetentionWarnings] = useState<
    RetentionWarningDto[]
  >([])
  const [agents, setAgents] = useState<AgentDirectoryItemDto[]>([])
  const [agentDirectoryRefreshToken, setAgentDirectoryRefreshToken] =
    useState(0)
  const deviceId = useMemo(getOrCreateDeviceId, [])
  const sessionRef = useRef(state.session)
  sessionRef.current = state.session
  const attachmentRefreshInFlight = useRef(new Map<Uuid, Promise<void>>())
  const [boardReloadToken, setBoardReloadToken] = useState(0)

  const selectedProject = state.projects.find(
    (project) => project.wire.id === state.selectedProjectId,
  )
  const selectedQuestionnaireVersions = useMemo(
    () =>
      questionnaireVersions.filter(
        (version) =>
          version.wire.questionnaire_id === selectedQuestionnaireId,
      ),
    [questionnaireVersions, selectedQuestionnaireId],
  )
  const publishedQuestionnaireVersions = useMemo(
    () =>
      questionnaireVersions
        .filter((version) => version.wire.state === 'published')
        .map((version) => ({
          id: version.wire.id,
          label: `${
            questionnaires.find(
              (questionnaire) =>
                questionnaire.wire.id === version.wire.questionnaire_id,
            )?.document?.title ?? 'Questionnaire'
          } · v${version.wire.number}`,
        })),
    [questionnaireVersions, questionnaires],
  )
  const activeAssigneeTasks = useMemo(
    () =>
      state.tasks.filter(
        (task) =>
          task.wire.active_assignment_id &&
          task.wire.active_assignee_identity_id === state.session?.identity_id,
      ),
    [state.session?.identity_id, state.tasks],
  )
  const questionnaireAssigneeTasks = useMemo(
    () =>
      activeAssigneeTasks.filter(
        (task) => task.wire.questionnaire_version_id,
      ),
    [activeAssigneeTasks],
  )

  useEffect(() => {
    let active = true
    let opened: EncryptedDatabase | undefined
    let initialized: Services | undefined
    void EncryptedDatabase.open()
      .then((database) => {
        if (!active) {
          database.close()
          return
        }
        opened = database
        const auth = new AuthController(api, database)
        const sync = new SyncEngine(database, api)
        const wake = new SyncWakeClient(api)
        initialized = { database, auth, sync, wake }
        setServices(initialized)
        void auth.hasLocalVault(deviceId).then((available) => {
          if (active) setOfflineVaultAvailable(available)
        })
        if (initialSession) {
          api.setSession(initialSession.token)
          dispatch({
            type: 'session-ready',
            session: initialSession,
            vaultPersistence: 'locked',
          })
        } else if (import.meta.env.DEV) {
          const saved = loadDevSession()
          if (saved?.vault) {
            // Synchronous merge+restore — no async gap for StrictMode to
            // overwrite localStorage with an empty vault mid-restore.
            api.setSession(saved.session.token)
            purgeZeroDevResourceKeys()
            const merged = mergeDevResourceKeysIntoSnapshot(
              saved.session.identity_id,
              saved.vault,
            )
            auth.vault.restoreDevSnapshot(merged)
            auth.vault.ensureIdentityId(saved.session.identity_id)
            persistDevVault(saved.session, auth.vault)
            dispatch({
              type: 'session-ready',
              session: saved.session,
              vaultPersistence: auth.vault.persistence,
            })
          } else if (saved) {
            // Session without device keys cannot decrypt anything — drop it and
            // the stale device id so the next login can provision a new vault
            // instead of showing a Locked board.
            clearDevSession()
            localStorage.removeItem('sprout.device-id')
          }
        }
      })
      .catch((error: unknown) =>
        dispatch({ type: 'set-error', message: errorMessage(error) }),
      )
    return () => {
      active = false
      // Persist DEV vault BEFORE tearing down. Do NOT call auth.logout() here:
      // it clears the shared ApiClient session token, and React StrictMode
      // remounts would leave the next mount briefly unauthenticated / keyless.
      // persistDevVault never replaces a richer key set with an emptier one.
      if (import.meta.env.DEV && initialized?.auth.vault.isUnlocked) {
        const saved = loadDevSession()
        if (saved?.session) {
          persistDevVault(saved.session, initialized.auth.vault)
        }
      }
      initialized?.wake.stop()
      initialized?.sync.clearMemory()
      initialized?.auth.vault.clearMemory()
      opened?.close()
    }
  }, [api, deviceId, dispatch, initialSession])

  useEffect(() => {
    if (!import.meta.env.DEV || !services) return
    services.auth.vault.setDevMutationListener(() => {
      const session = sessionRef.current
      if (!session || !services.auth.vault.isUnlocked) return
      services.auth.vault.ensureIdentityId(session.identity_id)
      persistDevVault(session, services.auth.vault)
    })
    return () => services.auth.vault.setDevMutationListener(undefined)
  }, [services])

  useEffect(() => {
    if (!services || !state.session) return
    services.auth.vault.ensureIdentityId(state.session.identity_id)
  }, [services, state.session])

  useEffect(() => {
    if (!import.meta.env.DEV || !services || !state.session) {
      return
    }
    const onPersist = () => {
      persistDevVault(state.session!, services.auth.vault)
    }
    // beforeunload alone is unreliable (Safari/Chrome often skip localStorage
    // writes). Also persist on pagehide / tab hide and right after mount.
    onPersist()
    window.addEventListener('pagehide', onPersist)
    window.addEventListener('beforeunload', onPersist)
    document.addEventListener('visibilitychange', onPersist)
    return () => {
      onPersist()
      window.removeEventListener('pagehide', onPersist)
      window.removeEventListener('beforeunload', onPersist)
      document.removeEventListener('visibilitychange', onPersist)
    }
  }, [services, state.session])

  useEffect(() => {
    const online = () => dispatch({ type: 'set-online', value: true })
    const offline = () => dispatch({ type: 'set-online', value: false })
    window.addEventListener('online', online)
    window.addEventListener('offline', offline)
    return () => {
      window.removeEventListener('online', online)
      window.removeEventListener('offline', offline)
    }
  }, [dispatch])

  useEffect(() => {
    if (
      state.storagePersistence !== 'unknown' ||
      (!state.session && state.phase !== 'local-ready')
    ) {
      return
    }
    let active = true
    void requestPersistentStorage()
      .then((granted) => {
        if (active) {
          dispatch({
            type: 'set-storage-persistence',
            value: granted ? 'granted' : 'not-granted',
          })
        }
      })
      .catch(() => {
        if (active) {
          dispatch({
            type: 'set-storage-persistence',
            value: 'not-granted',
          })
        }
      })
    return () => {
      active = false
    }
  }, [
    dispatch,
    state.phase,
    state.session,
    state.storagePersistence,
  ])

  useEffect(() => {
    if (!services || !state.session) return
    let active = true
    dispatch({ type: 'set-loading', value: true })
    void api
      .listProjects()
      .then(async (projects) => {
        const identityId = state.session?.identity_id
        if (import.meta.env.DEV && identityId) {
          await restoreDevResourceKeys(identityId, services.auth.vault)
        }
        const items = await Promise.all(
          projects.map((project) =>
            hydrateServerProject(api, services, project, identityId),
          ),
        )
        if (!active) return
        // After envelopes are imported, persist DEV vault so a reload can
        // decrypt without waiting for beforeunload.
        if (import.meta.env.DEV && state.session) {
          persistDevVault(state.session, services.auth.vault)
        }
        dispatch({ type: 'set-projects', projects: items })
      })
      .catch((error: unknown) => {
        if (!active) return
        if (error instanceof ApiError && error.status === 401) {
          clearDevSession()
          services.wake.stop()
          services.sync.clearMemory()
          services.auth.logout()
          dispatch({ type: 'logout' })
          dispatch({
            type: 'set-notice',
            message: 'Sessione server scaduta. Accedi di nuovo per ricaricare tutti i progetti.',
          })
          return
        }
        dispatch({ type: 'set-error', message: errorMessage(error) })
      })
    void services.database
      .queueCount()
      .then((count) => {
        if (active) dispatch({ type: 'set-queue-count', count })
      })
      .catch(() => {
        // The IndexedDB connection can close while HMR or logout tears down
        // the current app instance. The next mount reloads this value.
      })
    void services.database
      .listConflicts()
      .then((conflicts) => {
        if (active) dispatch({ type: 'set-conflicts', conflicts })
      })
      .catch(() => {
        // See queueCount above: teardown races are expected and recoverable.
      })
    return () => {
      active = false
    }
  }, [api, dispatch, services, state.session])

  useEffect(() => {
    if (!services || state.phase !== 'local-ready') return
    let active = true
    dispatch({ type: 'set-loading', value: true })
    void services.database
      .listRecords()
      .then(async (records) => {
        const projects: ProjectItem[] = []
        for (const record of records) {
          if (record.kind !== 'project' || !record.wire) continue
          const wire = record.wire as ProjectView
          try {
            projects.push({
              wire,
              document: await decryptProject(wire, services.auth.vault),
            })
          } catch (error) {
            projects.push({ wire, lockedReason: asLockedReason(error) })
          }
        }
        if (active) {
          dispatch({ type: 'set-projects', projects })
          dispatch({
            type: 'set-queue-count',
            count: await services.database.queueCount(),
          })
          dispatch({
            type: 'set-conflicts',
            conflicts: await services.database.listConflicts(),
          })
        }
      })
      .catch((error: unknown) => {
        if (active) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      })
    return () => {
      active = false
    }
  }, [dispatch, services, state.phase])

  useEffect(() => {
    if (
      !services ||
      state.phase !== 'local-ready' ||
      !state.selectedProjectId
    ) {
      return
    }
    let active = true
    void services.database
      .listRecords(state.selectedProjectId)
      .then(async (records) => {
        const topics: TopicItem[] = []
        for (const record of records) {
          if (record.kind !== 'topic' || !record.wire) continue
          const wire = record.wire as TopicDto
          try {
            topics.push({
              wire,
              document: await decryptTopic(wire, services.auth.vault),
            })
          } catch (error) {
            topics.push({ wire, lockedReason: asLockedReason(error) })
          }
        }
        if (active) dispatch({ type: 'set-topics', topics })
      })
      .catch((error: unknown) => {
        if (active) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      })
    return () => {
      active = false
    }
  }, [dispatch, services, state.phase, state.selectedProjectId])

  useEffect(() => {
    if (
      !services ||
      state.phase !== 'local-ready' ||
      !state.selectedProjectId
    ) {
      return
    }
    let active = true
    void services.database
      .listRecords(state.selectedProjectId)
      .then(async (records) => {
        const taskLists: TaskListItem[] = []
        const tasks: DecryptedTask[] = []
        const lockedTasks: TaskDto[] = []
        for (const record of records) {
          if (record.kind === 'task-list' && record.wire) {
            const wire = record.wire as TaskListDto
            try {
              taskLists.push({
                wire,
                document: await decryptTaskList(wire, services.auth.vault),
              })
            } catch (error) {
              taskLists.push({ wire, lockedReason: asLockedReason(error) })
            }
            continue
          }
          if (record.kind === 'task' && record.wire) {
            const wire = record.wire as TaskDto
            try {
              tasks.push(await decryptTask(wire, services.auth.vault))
            } catch {
              lockedTasks.push(wire)
            }
          }
        }
        if (active) {
          dispatch({ type: 'set-task-lists', taskLists })
          dispatch({ type: 'set-tasks', tasks, lockedTasks })
        }
      })
      .catch((error: unknown) => {
        if (active) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      })
    return () => {
      active = false
    }
  }, [dispatch, services, state.phase, state.selectedProjectId])

  useEffect(() => {
    if (!services || !state.selectedProjectId || !state.session) return
    let active = true
    dispatch({ type: 'set-loading', value: true })
    void api
      .listTopics(state.selectedProjectId)
      .then(async ({ topics }) => {
        const items = await Promise.all(
          topics.map(async (topic): Promise<TopicItem> => {
            await putRestRecord(services.database, topicRecord(topic))
            try {
              return {
                wire: topic,
                document: await decryptTopic(topic, services.auth.vault),
              }
            } catch (error) {
              return { wire: topic, lockedReason: asLockedReason(error) }
            }
          }),
        )
        if (active) dispatch({ type: 'set-topics', topics: items })
      })
      .catch((error: unknown) => {
        if (active) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      })
    return () => {
      active = false
    }
  }, [
    api,
    boardReloadToken,
    dispatch,
    services,
    state.selectedProjectId,
    state.session,
  ])

  useEffect(() => {
    if (!services || !state.selectedProjectId || !state.session) {
      return
    }
    if (state.topics.length === 0) {
      dispatch({ type: 'set-task-lists', taskLists: [] })
      return
    }
    let active = true
    const projectId = state.selectedProjectId
    dispatch({ type: 'set-loading', value: true })
    void Promise.all(
      state.topics.map((topic) => api.listTaskLists(projectId, topic.wire.id)),
    )
      .then(async (responses) => {
        const items: TaskListItem[] = []
        for (const { task_lists: lists } of responses) {
          for (const list of lists) {
            await putRestRecord(services.database, listRecord(list))
            try {
              items.push({
                wire: list,
                document: await decryptTaskList(list, services.auth.vault),
              })
            } catch (error) {
              items.push({ wire: list, lockedReason: asLockedReason(error) })
            }
          }
        }
        if (active) dispatch({ type: 'set-task-lists', taskLists: items })
      })
      .catch((error: unknown) => {
        if (active) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      })
    return () => {
      active = false
    }
  }, [
    api,
    dispatch,
    services,
    state.selectedProjectId,
    state.session,
    state.topics,
  ])

  useEffect(() => {
    if (!services || !state.selectedProjectId || !state.session) {
      return
    }
    if (state.taskLists.length === 0) {
      dispatch({ type: 'set-tasks', tasks: [], lockedTasks: [] })
      return
    }
    let active = true
    const projectId = state.selectedProjectId
    dispatch({ type: 'set-loading', value: true })
    void Promise.all(
      state.taskLists.map((list) => api.listTasks(projectId, list.wire.id)),
    )
      .then(async (responses) => {
        const decrypted: DecryptedTask[] = []
        const locked: TaskDto[] = []
        for (const { tasks } of responses) {
          for (const task of tasks) {
            await putRestRecord(services.database, taskRecord(task))
            try {
              decrypted.push(await decryptTask(task, services.auth.vault))
            } catch {
              locked.push(task)
            }
          }
        }
        if (active) {
          dispatch({
            type: 'set-tasks',
            tasks: decrypted,
            lockedTasks: locked,
          })
        }
      })
      .catch((error: unknown) => {
        if (active) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      })
    return () => {
      active = false
    }
  }, [
    api,
    dispatch,
    services,
    state.selectedProjectId,
    state.session,
    state.taskLists,
  ])

  useEffect(() => {
    if (!services || !state.selectedProjectId || !state.session) {
      return
    }
    let active = true
    const projectId = state.selectedProjectId
    const project = state.projects.find(
      (candidate) => candidate.wire.id === projectId,
    )
    void (async () => {
      try {
        const [packages, invites] = await Promise.all([
          api.listProjectDevicePackages(projectId),
          api.listProjectInvitations(projectId),
        ])
        const identityIds = new Set<Uuid>()
        if (project) identityIds.add(project.wire.owner_identity_id)
        for (const devicePackage of packages) {
          identityIds.add(devicePackage.identity_id)
        }
        for (const invitation of invites) {
          if (invitation.accepted_by_identity_id) {
            identityIds.add(invitation.accepted_by_identity_id)
          }
        }
        const handleById = new Map<Uuid, string>()
        try {
          const suggestions = await api.suggestProjectParticipants(
            projectId,
            '',
          )
          for (const suggestion of suggestions) {
            handleById.set(suggestion.identity_id, suggestion.identity_handle)
          }
        } catch {
          // Handles are best-effort; fall back to truncated identity IDs.
        }
        const members: BoardMember[] = [...identityIds].map((identityId) => ({
          identityId,
          label:
            handleById.get(identityId) ?? `User ${identityId.slice(0, 8)}`,
        }))
        if (active) {
          dispatch({ type: 'set-board-members', members })
        }
      } catch (error: unknown) {
        if (active) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      }
    })()
    return () => {
      active = false
    }
  }, [
    api,
    dispatch,
    services,
    state.projects,
    state.selectedProjectId,
    state.session,
  ])

  useEffect(() => {
    if (!state.selectedProjectId || !state.session) {
      setAgents([])
      return
    }
    let active = true
    void api
      .listAgents(state.selectedProjectId)
      .then((nextAgents) => {
        if (active) setAgents(nextAgents)
      })
      .catch((error: unknown) => {
        if (active) {
          setAgents([])
          dispatch({ type: 'set-error', message: errorMessage(error) })
        }
      })
    return () => {
      active = false
    }
  }, [
    agentDirectoryRefreshToken,
    api,
    dispatch,
    state.selectedProjectId,
    state.session,
  ])

  useEffect(() => {
    if (!state.selectedProjectId || !state.session) return
    const refreshInterval = window.setInterval(() => {
      setAgentDirectoryRefreshToken((value) => value + 1)
    }, 30_000)
    return () => window.clearInterval(refreshInterval)
  }, [state.selectedProjectId, state.session])

  const provisionAgent = useCallback(
    async (envelope: unknown): Promise<ProvisionAgentResponse> => {
      if (!state.selectedProjectId) {
        throw new Error('Seleziona un progetto prima di creare un agente.')
      }
      const response = await api.provisionAgent(
        state.selectedProjectId,
        envelope,
      )
      setAgentDirectoryRefreshToken((value) => value + 1)
      return response
    },
    [api, state.selectedProjectId],
  )

  useEffect(() => {
    if (
      !services ||
      !state.online ||
      !state.session ||
      !state.selectedProjectId
    ) {
      services?.wake.stop()
      return
    }
    services.wake.start(
      state.selectedProjectId,
      () => {
        void services.sync
          .pull(state.selectedProjectId as Uuid)
          .then(async () => {
            dispatch({
              type: 'set-conflicts',
              conflicts: await services.database.listConflicts(
                state.selectedProjectId,
              ),
            })
          })
          .catch((error: unknown) =>
            dispatch({
              type: 'set-notice',
              message: `Wake received, but cursor sync failed: ${errorMessage(error)}`,
            }),
          )
      },
      (status) => dispatch({ type: 'set-wake-status', status }),
    )
    return () => services.wake.stop()
  }, [
    dispatch,
    services,
    state.online,
    state.selectedProjectId,
    state.session,
  ])

  useEffect(() => {
    if (
      !services ||
      !state.online ||
      !state.session ||
      !state.selectedProjectId
    ) {
      return
    }
    void services.sync
      .flush(state.selectedProjectId)
      .then(async (summary) => {
        dispatch({ type: 'set-queue-count', count: summary.pending })
        dispatch({
          type: 'set-conflicts',
          conflicts: await services.database.listConflicts(
            state.selectedProjectId,
          ),
        })
      })
      .catch(() => undefined)
  }, [
    dispatch,
    services,
    state.online,
    state.selectedProjectId,
    state.session,
  ])

  useEffect(() => {
    if (!state.session) return
    void Promise.all([
      api.getRetentionPreference(),
      api.listRetentionArchives(),
      api.listRetentionWarnings(),
    ])
      .then(([preference, archiveList, warningList]) => {
        setAutoExport(preference.preference.auto_export_enabled)
        setArchives(archiveList.archives)
        setRetentionWarnings(warningList.warnings)
        const available = availableRetentionArchiveCount(archiveList.archives)
        if (available > 0) {
          dispatch({
            type: 'set-notice',
            message: `${available} encrypted retention archive${available === 1 ? ' is' : 's are'} available for download.`,
          })
        }
      })
      .catch((error: unknown) =>
        dispatch({ type: 'set-error', message: errorMessage(error) }),
      )
  }, [api, dispatch, state.session])

  const requireServices = useCallback((): Services => {
    if (!services) throw new Error('Encrypted local storage is still opening')
    return services
  }, [services])

  const runAuth = async (
    operation: () => Promise<{
      session: SessionResponse
      requiresAuthorizedDevice: boolean
    }>,
    phase: 'signin' | 'signup' | 'verify' | 'recover' = 'signin',
    successNotice?: string,
  ) => {
    dispatch({ type: 'auth-started' })
    try {
      const outcome = await operation()
      const current = requireServices()
      // Prefer the live vault: older clients reported requiresAuthorizedDevice
      // even after a successful device provision, which skipped DEV key save
      // and left every resource Locked after reload.
      const vaultReady = current.auth.vault.isUnlocked
      if (outcome.requiresAuthorizedDevice && !vaultReady) {
        // Last chance in DEV: a previous snapshot may still hold this device's keys.
        if (import.meta.env.DEV) {
          const saved = loadDevSession()
          if (
            saved?.vault &&
            saved.vault.deviceId === outcome.session.device_id
          ) {
            current.auth.vault.restoreDevSnapshot(saved.vault)
            current.auth.vault.ensureIdentityId(outcome.session.identity_id)
          }
        }
      }
      if (outcome.requiresAuthorizedDevice && !current.auth.vault.isUnlocked) {
        dispatch({
          type: 'vault-locked',
          session: outcome.session,
          message:
            'Authentication succeeded, but this device has no wrapped project keys. Use another authorized device or unanimous recovery.',
        })
      } else {
        dispatch({
          type: 'session-ready',
          session: outcome.session,
          vaultPersistence: current.auth.vault.persistence,
        })
        if (import.meta.env.DEV) {
          await restoreDevResourceKeys(
            outcome.session.identity_id,
            current.auth.vault,
          )
          persistDevVault(outcome.session, current.auth.vault)
        }
        if (successNotice) {
          dispatch({ type: 'set-notice', message: successNotice })
        } else if (outcome.requiresAuthorizedDevice && vaultReady) {
          dispatch({
            type: 'set-notice',
            message:
              'Sessione ripristinata con le chiavi locali di questo device.',
          })
        }
      }
    } catch (error) {
      dispatch({ type: 'set-error', message: authErrorMessage(error, phase) })
    }
  }

  const resetLocalDeviceKeys = () => {
    // Keep sprout-dev-resource-keys: raw resource keys are enough to decrypt
    // even after minting a new device package.
    clearDevSession()
    localStorage.removeItem('sprout.device-id')
    if (services) {
      services.wake.stop()
      services.sync.clearMemory()
      services.auth.logout()
    }
    setPresetResult(undefined)
    setQuestionnaires([])
    setQuestionnaireVersions([])
    setSelectedQuestionnaireId(undefined)
    setTaskQuestionnaireVersion(undefined)
    setQuestionnaireSubmission(undefined)
    setAttachments([])
    setAttachmentLabels({})
    dispatch({ type: 'logout' })
    dispatch({
      type: 'set-notice',
      message:
        'Device locale resettato. Usa Dev login: se esiste un backup chiavi per questo account, i progetti torneranno leggibili.',
    })
  }

  const retryDevKeyRestore = async () => {
    if (!services || !state.session) return
    try {
      const identityId = state.session.identity_id
      services.auth.vault.ensureIdentityId(identityId)
      const purged = purgeZeroDevResourceKeys()
      const restored = await restoreAllDevResourceKeys(services.auth.vault)
      persistDevVault(state.session, services.auth.vault)
      const projects = await api.listProjects()
      const hydrated = await Promise.all(
        projects.map((project) =>
          hydrateServerProject(api, services, project, identityId),
        ),
      )
      dispatch({ type: 'set-projects', projects: hydrated })

      // Decrypt topics immediately with the restored keys (don't wait on effects).
      if (state.selectedProjectId) {
        const { topics } = await api.listTopics(state.selectedProjectId)
        const topicItems: TopicItem[] = []
        for (const topic of topics) {
          await putRestRecord(services.database, topicRecord(topic))
          try {
            topicItems.push({
              wire: topic,
              document: await decryptTopic(topic, services.auth.vault),
            })
          } catch (error) {
            topicItems.push({ wire: topic, lockedReason: asLockedReason(error) })
          }
        }
        dispatch({ type: 'set-topics', topics: topicItems })
        const coverage = countBackupHitsForResources(
          topicItems.map((topic) => ({
            resourceId: topic.wire.resource_node_id,
            epoch: topic.wire.key_epoch,
            needsBody: topic.wire.payload != null,
          })),
        )
        const stillLocked = topicItems.filter((topic) => !topic.document).length
        const firstReason = topicItems.find((topic) => topic.lockedReason)
          ?.lockedReason
        dispatch({
          type: 'set-notice',
          message:
            `Purged=${purged}, restored=${restored}, exact-hits=${coverage.hits}/${topicItems.length} (epochMiss=${coverage.epochMiss}, purposeMiss=${coverage.purposeMiss}), locked=${stillLocked}.` +
            (firstReason ? ` Errore: ${firstReason}` : '') +
            (stillLocked === 0
              ? ' Board sbloccata: gli slot recuperati sono stati salvati automaticamente.'
              : ' Nessuna chiave locale disponibile ha autenticato questi ciphertext; serve il vault del device originale o una recovery envelope valida.'),
        })
      } else {
        dispatch({
          type: 'set-notice',
          message: hasDevResourceKeyBackup(identityId)
            ? `Purged=${purged}, ripristinate ${restored} chiavi vive. Nessun progetto selezionato.`
            : `Purged=${purged}. Nessun backup chiavi vive in questo browser.`,
        })
      }
      setBoardReloadToken((token) => token + 1)
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        clearDevSession()
        services.wake.stop()
        services.sync.clearMemory()
        services.auth.logout()
        dispatch({ type: 'logout' })
        dispatch({
          type: 'set-notice',
          message: 'Sessione server scaduta. Accedi di nuovo prima di ripristinare le chiavi.',
        })
        return
      }
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const lockedBoardReason = useMemo(() => {
    if (!state.session || state.phase === 'locked') return undefined
    const lockedTopics = state.topics.filter((topic) => !topic.document)
    const lockedLists = state.taskLists.filter((list) => !list.document)
    if (
      state.topics.length === 0 &&
      state.taskLists.length === 0 &&
      state.projects.every((project) => project.document)
    ) {
      return undefined
    }
    if (
      state.topics.length > 0 &&
      lockedTopics.length === state.topics.length
    ) {
      return (
        lockedTopics[0]?.lockedReason ??
        state.projects.find((project) => project.lockedReason)?.lockedReason ??
        'Resource keys missing on this device'
      )
    }
    if (
      state.taskLists.length > 0 &&
      lockedLists.length === state.taskLists.length
    ) {
      return (
        lockedLists[0]?.lockedReason ??
        'Resource keys missing on this device'
      )
    }
    return undefined
  }, [
    state.phase,
    state.projects,
    state.session,
    state.taskLists,
    state.topics,
  ])

  const unlockLocalVault = async () => {
    dispatch({ type: 'auth-started' })
    try {
      const outcome = await requireServices().auth.unlockLocalVault(deviceId)
      dispatch({
        type: 'local-vault-ready',
        deviceId: outcome.deviceId,
        identityId: outcome.identityId,
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const startSignup = async (input: {
    email: string
    identityHandle: string
  }) => {
    dispatch({ type: 'auth-started' })
    try {
      const response = await requireServices().auth.startSignup({ ...input, deviceId })
      if (
        import.meta.env.DEV &&
        response.identity_id &&
        !response.dev_verification_token
      ) {
        dispatch({ type: 'set-error' })
        await runAuth(
          () =>
            requireServices().auth.devLogin({
              email: input.email,
              identityHandle: input.identityHandle,
              deviceId,
            }),
          'signup',
        )
        return response
      }
      dispatch({ type: 'set-error' })
      dispatch({
        type: 'set-notice',
        message: response.dev_verification_token
          ? 'Verifica pronta: identity ID e token sono stati compilati automaticamente.'
          : 'Verifica richiesta. Inserisci identity ID e token dall’outbox di sviluppo.',
      })
      return response
    } catch (error) {
      if (
        import.meta.env.DEV &&
        error instanceof ApiError &&
        error.status === 409
      ) {
        dispatch({ type: 'set-error' })
        await runAuth(
          () =>
            requireServices().auth.devLogin({
              email: input.email,
              identityHandle: input.identityHandle,
              deviceId,
            }),
          'signup',
        )
        return { accepted: true, identity_id: undefined }
      }
      dispatch({ type: 'set-error', message: authErrorMessage(error, 'signup') })
      throw error
    }
  }

  const devLogin = (input: { email: string; identityHandle: string }) =>
    runAuth(
      () =>
        requireServices().auth.devLogin({
          email: input.email,
          identityHandle: input.identityHandle,
          deviceId,
        }),
      'signin',
    )

  const startRecovery = async (email: string) => {
    dispatch({ type: 'auth-started' })
    try {
      await requireServices().auth.startRecovery(email)
      dispatch({ type: 'set-error' })
      dispatch({
        type: 'set-notice',
        message:
          'Se l’account esiste, un messaggio di recupero è in coda.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
      throw error
    }
  }

  const createProject = async (event: FormEvent) => {
    event.preventDefault()
    if (!state.online || !state.session) {
      dispatch({
        type: 'set-error',
        message:
          'Sign in to the server before creating a project. Local-only unlock cannot authorize new resources.',
      })
      return
    }
    try {
      const current = requireServices()
      const id = crypto.randomUUID()
      const document: ProjectDocument = { schema: 1, name: projectName }
      const payload = await createEncryptedResource(current.auth.vault, {
        projectId: id,
        resourceId: id,
        kind: 'project',
        aggregateVersion: 0,
        document,
      })
      const project = await api.createProject({
        id,
        encrypted_metadata_b64: encodePayloadContainer(payload),
      })
      const projectKey = current.auth.vault.getResourceKey(id)
      if (!projectKey) throw new Error('New project key is unavailable')
      const packages = await api.listProjectDevicePackages(id)
      const rootEpoch = await buildInitialResourceEpoch(
        current.auth.vault,
        {
          projectId: id,
          resourceId: project.root_resource_id,
          resourceKey: projectKey,
          recipientIdentityId: state.session.identity_id,
          packages,
        },
      )
      await api.initializeResourceEpoch(
        id,
        project.root_resource_id,
        rootEpoch,
      )
      await current.auth.vault.putResourceKey(
        project.root_resource_id,
        projectKey,
      )
      if (import.meta.env.DEV) {
        persistDevVault(state.session, current.auth.vault)
      }
      await putRestRecord(current.database, projectRecord(project, payload))
      const serverProjects = await api.listProjects()
      const hydratedProjects = await Promise.all(
        serverProjects.map((serverProject) =>
          hydrateServerProject(
            api,
            current,
            serverProject,
            state.session?.identity_id,
          ),
        ),
      )
      dispatch({
        type: 'set-projects',
        projects: hydratedProjects,
      })
      dispatch({ type: 'select-project', projectId: id })
      setProjectName('')
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const refreshProjectPeople = useCallback(async () => {
    if (!state.selectedProjectId) {
      setInvitations([])
      setManagedResourceGrants([])
      return
    }
    try {
      const [projectInvitations, topicGrants] = await Promise.all([
        api.listProjectInvitations(state.selectedProjectId),
        Promise.all(
          state.topics
            .filter(
              (topic) =>
                topic.wire.project_id === state.selectedProjectId,
            )
            .map(async (topic) => {
              try {
                const response = await api.listResourcePermissions(
                  state.selectedProjectId as Uuid,
                  topic.wire.resource_node_id,
                )
                return response.grants.map((grant) => ({
                  topicName: topic.document?.name ?? 'Encrypted topic',
                  resourceId: topic.wire.resource_node_id,
                  grant,
                }))
              } catch (error) {
                if (error instanceof ApiError && error.status === 403) {
                  return []
                }
                throw error
              }
            }),
        ),
      ])
      setInvitations(projectInvitations)
      setManagedResourceGrants(
        topicGrants
          .flat()
          .filter((item) => item.grant.revoked_at === null),
      )
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }, [api, dispatch, state.selectedProjectId, state.topics])

  useEffect(() => {
    if (state.screen !== 'people' || !state.session) return
    setInvitations([])
    setManagedResourceGrants([])
    setParticipantSuggestions([])
    void refreshProjectPeople()
  }, [
    refreshProjectPeople,
    state.screen,
    state.selectedProjectId,
    state.session,
  ])

  const inviteProjectParticipant = async (input: {
    email: string
    name: string
    phone?: string
    role: 'admin' | 'member' | 'guest'
  }) => {
    if (!state.selectedProjectId) {
      throw new Error('Select a project before inviting participants')
    }
    const current = requireServices()
    const payload = await encryptExistingResource(current.auth.vault, {
      projectId: state.selectedProjectId,
      resourceId: state.selectedProjectId,
      kind: 'project',
      aggregateVersion: 0,
      document: {
        schema: 1,
        kind: 'project-invitation',
        name: input.name,
        phone: input.phone,
      },
    })
    await api.createProjectInvitation(state.selectedProjectId, {
      invitee_email: input.email,
      encrypted_payload_b64: encodePayloadContainer(payload),
      role: input.role,
      expires_in_seconds: 7 * 24 * 60 * 60,
    })
    await refreshProjectPeople()
    dispatch({
      type: 'set-notice',
      message: 'Encrypted project invitation queued for delivery.',
    })
  }

  const acceptProjectInvitation = async (input: {
    projectId: Uuid
    invitationId: Uuid
    token: string
  }) => {
    await api.acceptProjectInvitation(
      input.projectId,
      input.invitationId,
      input.token,
    )
    const current = requireServices()
    const projects = await api.listProjects()
    const identityId = state.session?.identity_id
    if (import.meta.env.DEV && identityId) {
      await restoreDevResourceKeys(identityId, current.auth.vault)
    }
    const hydrated = await Promise.all(
      projects.map((project) =>
        hydrateServerProject(api, current, project, identityId),
      ),
    )
    if (import.meta.env.DEV && identityId && state.session) {
      persistDevVault(state.session, current.auth.vault)
    }
    dispatch({ type: 'set-projects', projects: hydrated })
    dispatch({
      type: 'set-notice',
      message:
        'Invitation accepted. Available resource envelopes were verified and imported.',
    })
  }

  const shareProjectWithParticipant = async (
    recipientIdentityId: Uuid,
  ) => {
    if (!state.selectedProjectId || !state.session) {
      throw new Error('Select a project before sharing encrypted access')
    }
    const current = requireServices()
    const project = state.projects.find(
      (candidate) => candidate.wire.id === state.selectedProjectId,
    )
    if (!project) throw new Error('Selected project is unavailable')
    const packages = await api.listProjectDevicePackages(
      state.selectedProjectId,
    )
    const { topics } = await api.listTopics(state.selectedProjectId)
    for (const topic of topics) {
      const plan = await api.getFullResourceEnvelopePlan(
        state.selectedProjectId,
        topic.resource_node_id,
      )
      const envelopes = (
        await Promise.all(
          plan.resources.map(async (resource) => {
            const resourceKey = current.auth.vault.getResourceKey(
              resource.resource_id,
              resource.epoch,
            )
            if (!resourceKey) {
              throw new Error(
                'This owner device is missing a resource key required for sharing',
              )
            }
            const bodyEnvelopes = await buildResourceKeyEnvelopes(current.auth.vault, {
              projectId: state.selectedProjectId as Uuid,
              resourceId: resource.resource_id,
              resourceKey,
              recipientIdentityId,
              packages,
              epoch: resource.epoch,
              previousEpochHash:
                resource.previous_epoch_hash_b64 === null
                  ? undefined
                  : base64ToBytes(resource.previous_epoch_hash_b64),
            })
            const headerKey = current.auth.vault.getHeaderKey(
              resource.resource_id,
              resource.epoch,
            )
            if (!headerKey) return bodyEnvelopes
            return [
              ...bodyEnvelopes,
              ...(await buildResourceKeyEnvelopes(current.auth.vault, {
                projectId: state.selectedProjectId as Uuid,
                resourceId: resource.resource_id,
                resourceKey: headerKey,
                keyPurpose: 'header',
                recipientIdentityId,
                packages,
                epoch: resource.epoch,
                previousEpochHash:
                  resource.previous_epoch_hash_b64 === null
                    ? undefined
                    : base64ToBytes(resource.previous_epoch_hash_b64),
              })),
            ]
          }),
        )
      ).flat()
      await api.grantResourcePermission(
        state.selectedProjectId,
        topic.resource_node_id,
        {
          grant_id: crypto.randomUUID(),
          user_id: recipientIdentityId,
          resource_id: topic.resource_node_id,
          access_level: 'view',
          access_scope: 'full',
          visibility: 'restricted',
          envelopes,
          idempotency_key: crypto.randomUUID(),
        },
      )
    }
    const rootKey = current.auth.vault.getResourceKey(
      project.wire.root_resource_id,
      project.wire.key_epoch,
    )
    if (!rootKey) {
      throw new Error('This owner device is missing the project root key')
    }
    await api.shareMemberResourceKeys(state.selectedProjectId, {
      recipient_identity_id: recipientIdentityId,
      resource_ids: [project.wire.root_resource_id],
      envelopes: await buildResourceKeyEnvelopes(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId: project.wire.root_resource_id,
        resourceKey: rootKey,
        recipientIdentityId,
        packages,
      }),
    })
    await refreshProjectPeople()
    dispatch({
      type: 'set-notice',
      message:
        'Signed resource keys and view permissions were shared with the participant. Reprovision recovery shares so the new membership epoch remains recoverable.',
    })
    await provisionSelectedProjectRecovery()
  }

  const revokeProjectResourceGrant = async (input: {
    resourceId: Uuid
    grantId: Uuid
    userId: Uuid
  }) => {
    const projectId = state.selectedProjectId
    if (!projectId) {
      throw new Error('Select a project before revoking access')
    }
    const current = requireServices()
    const [plan, packages] = await Promise.all([
      api.getResourceRotationPlan(
        projectId,
        input.resourceId,
        input.grantId,
      ),
      api.listProjectDevicePackages(projectId),
    ])
    if (plan.revoked_identity_id !== input.userId) {
      throw new Error('Permission rotation plan identity mismatch')
    }
    const builtRotations: Awaited<
      ReturnType<typeof buildResourceEpochRotation>
    >[] = []
    try {
      for (const resource of plan.resources) {
        const previousKeyCommitment = base64ToBytes(
          resource.previous_key_commitment_b64,
        )
        const previousHeaderKeyCommitment =
          resource.previous_header_key_commitment_b64 === null
            ? undefined
            : base64ToBytes(resource.previous_header_key_commitment_b64)
        try {
          builtRotations.push(
            await buildResourceEpochRotation(current.auth.vault, {
              projectId,
              resourceId: resource.resource_id,
              previousEpochId: resource.previous_epoch_id,
              currentEpoch: resource.current_epoch,
              previousKeyCommitment,
              previousHeaderKeyCommitment,
              recipientIdentityIds: resource.recipient_identity_ids,
              bodyRecipientIdentityIds:
                resource.body_recipient_identity_ids,
              headerRecipientIdentityIds:
                resource.header_recipient_identity_ids,
              packages,
            }),
          )
        } finally {
          zeroBytes(previousKeyCommitment, previousHeaderKeyCommitment)
        }
      }
      await api.revokeResourcePermission(
        projectId,
        input.resourceId,
        input.grantId,
        {
          user_id: input.userId,
          rotations: builtRotations.map((item) => item.rotation),
          encrypted_admin_notification_b64: null,
          idempotency_key: crypto.randomUUID(),
        },
      )
      await Promise.all(
        builtRotations.flatMap((item) => [
          current.auth.vault.putResourceKey(
            item.rotation.resource_id,
            item.resourceKey,
            item.rotation.new_epoch,
          ),
          ...(item.headerKey
            ? [
                current.auth.vault.putResourceKey(
                  item.rotation.resource_id,
                  item.headerKey,
                  item.rotation.new_epoch,
                  'header',
                ),
              ]
            : []),
        ]),
      )
      await refreshProjectPeople()
      dispatch({
        type: 'set-notice',
        message:
          'Access revoked and all affected resource keys rotated atomically.',
      })
    } finally {
      zeroBytes(
        ...builtRotations.flatMap((item) => [item.resourceKey, item.headerKey]),
      )
    }
  }

  const suggestProjectParticipants = async (prefix: string) => {
    if (!state.selectedProjectId) {
      throw new Error('Select a project before searching participants')
    }
    setParticipantSuggestions(
      await api.suggestProjectParticipants(state.selectedProjectId, prefix),
    )
  }

  const createTopic = async (name: string) => {
    if (!state.selectedProjectId || !state.session) {
      dispatch({
        type: 'set-error',
        message: 'Server sign-in is required to create a topic.',
      })
      return
    }
    try {
      const current = requireServices()
      const project = state.projects.find(
        (candidate) => candidate.wire.id === state.selectedProjectId,
      )
      if (!project) throw new Error('Selected project is unavailable')
      const id = crypto.randomUUID()
      const resourceId = crypto.randomUUID()
      const document: TopicDocument = { schema: 1, name }
      const payload = await createEncryptedResource(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        kind: 'topic',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document,
      })
      const header = await createEncryptedResourceHeader(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        kind: 'topic',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document,
      })
      const resourceKey = current.auth.vault.getResourceKey(resourceId)
      const headerKey = current.auth.vault.getHeaderKey(resourceId)
      if (!resourceKey) throw new Error('New topic key is unavailable')
      if (!headerKey) throw new Error('New topic header key is unavailable')
      const epoch = await buildInitialResourceEpoch(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        resourceKey,
        headerKey,
        recipientIdentityId: state.session.identity_id,
        packages: await api.listProjectDevicePackages(
          state.selectedProjectId,
        ),
      })
      const { topic } = await api.createTopic(state.selectedProjectId, {
        id,
        resource_node_id: resourceId,
        parent_resource_node_id: project.wire.root_resource_id,
        payload,
        header,
        ...epoch,
        idempotency_key: crypto.randomUUID(),
      })
      if (import.meta.env.DEV) {
        persistDevVault(state.session, current.auth.vault)
      }
      await putRestRecord(current.database, topicRecord(topic))
      dispatch({
        type: 'set-topics',
        topics: [...state.topics, { wire: topic, document }],
      })
      dispatch({ type: 'select-topic', topicId: topic.id })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const updateTopicDocument = async (
    topic: TopicItem,
    document: TopicDocument,
  ) => {
    if (!state.selectedProjectId || !state.session) {
      dispatch({
        type: 'set-error',
        message: 'Server sign-in is required to update a topic.',
      })
      return
    }
    if (!topic.document) {
      throw new Error('This topic cannot be edited on this device')
    }
    const current = requireServices()
    const active = await ensureActiveResourceKey(
      api,
      current,
      state.session,
      state.selectedProjectId,
      topic.wire.resource_node_id,
      topic.wire.key_epoch,
      'Missing active topic resource key',
    )
    const payload = await encryptExistingResource(current.auth.vault, {
      projectId: state.selectedProjectId,
      resourceId: topic.wire.resource_node_id,
      kind: 'topic',
      aggregateVersion: topic.wire.payload_version + 1,
      keyEpoch: active.epoch,
      document,
    })
    const body = {
      expected_payload_version: topic.wire.payload_version,
      key_epoch: active.epoch,
      payload,
      idempotency_key: crypto.randomUUID(),
    }
    const { topic: wire } = await api.updateTopic(
      state.selectedProjectId,
      topic.wire.id,
      body,
    )
    const mergedWire: typeof wire = {
      ...topic.wire,
      ...wire,
      header: wire.header ?? topic.wire.header,
      payload: wire.payload ?? payload,
    }
    await putRestRecord(current.database, topicRecord(mergedWire))
    dispatch({
      type: 'set-topics',
      topics: state.topics.map((item) =>
        item.wire.id === topic.wire.id
          ? { wire: mergedWire, document }
          : item,
      ),
    })
  }

  const renameTopic = async (topic: TopicItem, name: string) => {
    const trimmed = name.trim()
    if (!trimmed) return
    try {
      await updateTopicDocument(topic, {
        ...topic.document!,
        name: trimmed,
      })
      dispatch({
        type: 'set-notice',
        message: 'Categoria rinominata.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const updateTaskListDocument = async (
    list: TaskListItem,
    document: TaskListDocument,
  ) => {
    if (!state.selectedProjectId || !state.session) {
      dispatch({
        type: 'set-error',
        message: 'Server sign-in is required to update a task list.',
      })
      return
    }
    if (!list.document) {
      throw new Error('This task list cannot be edited on this device')
    }
    const current = requireServices()
    const active = await ensureActiveResourceKey(
      api,
      current,
      state.session,
      state.selectedProjectId,
      list.wire.resource_node_id,
      list.wire.key_epoch,
      'Missing active task-list resource key',
    )
    const payload = await encryptExistingResource(current.auth.vault, {
      projectId: state.selectedProjectId,
      resourceId: list.wire.resource_node_id,
      kind: 'task-list',
      aggregateVersion: list.wire.payload_version + 1,
      keyEpoch: active.epoch,
      document,
    })
    const body = {
      expected_payload_version: list.wire.payload_version,
      key_epoch: active.epoch,
      payload,
      idempotency_key: crypto.randomUUID(),
    }
    const { task_list: wire } = await api.updateTaskList(
      state.selectedProjectId,
      list.wire.id,
      body,
    )
    const mergedWire: typeof wire = {
      ...list.wire,
      ...wire,
      header: wire.header ?? list.wire.header,
      payload: wire.payload ?? payload,
    }
    await putRestRecord(current.database, listRecord(mergedWire))
    dispatch({
      type: 'set-task-lists',
      taskLists: state.taskLists.map((item) =>
        item.wire.id === list.wire.id ? { wire: mergedWire, document } : item,
      ),
    })
  }

  const updateTaskList = async (
    list: TaskListItem,
    input: {
      name: string
      color?: TaskListDocument['color']
      icon?: TaskListDocument['icon']
    },
  ) => {
    if (!list.document) return
    const trimmed = input.name.trim()
    if (!trimmed) return
    const previousName = list.document.name
    const previousColor = list.document.color
    const previousIcon = list.document.icon
    const unchanged =
      trimmed === previousName &&
      input.color === previousColor &&
      isSameTaskListIcon(input.icon, previousIcon)
    if (unchanged) return
    try {
      await updateTaskListDocument(list, {
        ...list.document,
        name: trimmed,
        color: input.color,
        icon: input.icon,
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const loadTaskListInfoDocuments = async (
    list: TaskListItem,
  ): Promise<DecryptedInfoDocument[]> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to load task-list info')
    }
    const current = requireServices()
    const response = await api.listTaskListInfoDocuments(
      state.selectedProjectId,
      list.wire.id,
    )
    return Promise.all(
      response.documents.map((document) =>
        decryptInfoDocument(document, current.auth.vault),
      ),
    )
  }

  const loadProjectInfoDocuments = async (
    project: ProjectItem,
  ): Promise<DecryptedInfoDocument[]> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to load project info')
    }
    const current = requireServices()
    const response = await api.listProjectInfoDocuments(project.wire.id)
    return Promise.all(
      response.documents.map((document) =>
        decryptInfoDocument(document, current.auth.vault),
      ),
    )
  }

  const createProjectInfoDocument = async (
    project: ProjectItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to create project info')
    }
    const current = requireServices()
    const hasProjectRootKey = await synchronizeProjectRootKey(
      current.auth.vault,
      project.wire,
    )
    if (hasProjectRootKey && import.meta.env.DEV) {
      persistDevVault(state.session, current.auth.vault)
    }
    const active = await ensureActiveResourceKey(
      api,
      current,
      state.session,
      project.wire.id,
      project.wire.root_resource_id,
      project.wire.key_epoch,
      'Missing active project resource key',
      false,
    )
    const documentId = crypto.randomUUID()
    const payload = await encryptInfoDocument(current.auth.vault, {
      projectId: project.wire.id,
      documentId,
      containerResourceId: project.wire.root_resource_id,
      aggregateVersion: INITIAL_PAYLOAD_VERSION,
      keyEpoch: active.epoch,
      kind: 'project',
      document,
    })
    const { document: wire } = await api.createProjectInfoDocument(
      project.wire.id,
      {
        id: documentId,
        parent_document_id: parentDocumentId ?? null,
        resource_node_id: project.wire.root_resource_id,
        key_epoch: active.epoch,
        payload,
        idempotency_key: crypto.randomUUID(),
      },
    )
    return { wire, document }
  }

  const createTaskListInfoDocument = async (
    list: TaskListItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to create task-list info')
    }
    const current = requireServices()
    const active = await ensureActiveResourceKey(
      api,
      current,
      state.session,
      state.selectedProjectId,
      list.wire.resource_node_id,
      list.wire.key_epoch,
      'Missing active task-list resource key',
    )
    const documentId = crypto.randomUUID()
    const payload = await encryptInfoDocument(current.auth.vault, {
      projectId: state.selectedProjectId,
      documentId,
      containerResourceId: list.wire.resource_node_id,
      aggregateVersion: INITIAL_PAYLOAD_VERSION,
      keyEpoch: active.epoch,
      kind: 'task-list',
      document,
    })
    const { document: wire } = await api.createTaskListInfoDocument(
      state.selectedProjectId,
      list.wire.id,
      {
        id: documentId,
        parent_document_id: parentDocumentId ?? null,
        resource_node_id: list.wire.resource_node_id,
        key_epoch: active.epoch,
        payload,
        idempotency_key: crypto.randomUUID(),
      },
    )
    return { wire, document }
  }

  const loadTopicInfoDocuments = async (
    topic: TopicItem,
  ): Promise<DecryptedInfoDocument[]> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to load topic info')
    }
    const current = requireServices()
    const response = await api.listTopicInfoDocuments(
      state.selectedProjectId,
      topic.wire.id,
    )
    return Promise.all(
      response.documents.map((document) =>
        decryptInfoDocument(document, current.auth.vault),
      ),
    )
  }

  const createTopicInfoDocument = async (
    topic: TopicItem,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to create topic info')
    }
    const current = requireServices()
    const active = await ensureActiveResourceKey(
      api,
      current,
      state.session,
      state.selectedProjectId,
      topic.wire.resource_node_id,
      topic.wire.key_epoch,
      'Missing active topic resource key',
    )
    const documentId = crypto.randomUUID()
    const payload = await encryptInfoDocument(current.auth.vault, {
      projectId: state.selectedProjectId,
      documentId,
      containerResourceId: topic.wire.resource_node_id,
      aggregateVersion: INITIAL_PAYLOAD_VERSION,
      keyEpoch: active.epoch,
      kind: 'topic',
      document,
    })
    const { document: wire } = await api.createTopicInfoDocument(
      state.selectedProjectId,
      topic.wire.id,
      {
        id: documentId,
        parent_document_id: parentDocumentId ?? null,
        resource_node_id: topic.wire.resource_node_id,
        key_epoch: active.epoch,
        payload,
        idempotency_key: crypto.randomUUID(),
      },
    )
    return { wire, document }
  }

  const updateInfoDocument = async (
    value: DecryptedInfoDocument,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to update info documents')
    }
    const current = requireServices()
    const active = await ensureActiveResourceKey(
      api,
      current,
      state.session,
      state.selectedProjectId,
      value.wire.resource_node_id,
      value.wire.key_epoch,
      'Missing active info-document resource key',
    )
    const payload = await encryptInfoDocument(current.auth.vault, {
      projectId: state.selectedProjectId,
      documentId: value.wire.id,
      containerResourceId: value.wire.resource_node_id,
      aggregateVersion: value.wire.payload_version + 1,
      keyEpoch: active.epoch,
      kind: value.wire.task_list_id
        ? 'task-list'
        : value.wire.topic_id
          ? 'topic'
          : 'project',
      document,
    })
    const { document: wire } = await api.updateInfoDocument(
      state.selectedProjectId,
      value.wire.id,
      {
        expected_payload_version: value.wire.payload_version,
        key_epoch: active.epoch,
        payload,
        idempotency_key: crypto.randomUUID(),
      },
    )
    return { wire, document }
  }

  const uploadInfoDocumentFile = async (
    document: DecryptedInfoDocument,
    file: File,
  ): Promise<InfoFileBlock> => {
    if (!state.session || !state.selectedProjectId) {
      throw new Error('Server sign-in is required to attach a file')
    }
    const current = requireServices()
    const active = await ensureActiveResourceKey(
      api,
      current,
      state.session,
      state.selectedProjectId,
      document.wire.resource_node_id,
      document.wire.key_epoch,
      'Missing active info-document resource key',
    )
    const resourceKey = current.auth.vault.getResourceKey(
      document.wire.resource_node_id,
      active.epoch,
    )
    if (!resourceKey) {
      throw new Error('This device cannot decrypt the info-document resource')
    }
    const blobId = crypto.randomUUID()
    const blockId = crypto.randomUUID()
    const context = {
      projectId: state.selectedProjectId,
      resourceId: document.wire.resource_node_id,
      blobId,
      keyEpoch: active.epoch,
    }
    const ciphertext = await encryptAttachment(file, resourceKey, context)
    try {
      await writeEncryptedAttachment(blobId, ciphertext)
      const [ciphertextSha256, encryptedBlobMetadata, encryptedMetadata] =
        await Promise.all([
          attachmentCiphertextSha256(ciphertext),
          encryptExistingResource(current.auth.vault, {
            projectId: state.selectedProjectId,
            resourceId: document.wire.resource_node_id,
            kind: 'attachment',
            aggregateVersion: 0,
            keyEpoch: active.epoch,
            document: {
              schema: 1,
              format: 'sprout-attachment-v1',
              plaintext_size: file.size,
            },
          }),
          encryptExistingResource(current.auth.vault, {
            projectId: state.selectedProjectId,
            resourceId: document.wire.resource_node_id,
            kind: 'attachment',
            aggregateVersion: 0,
            keyEpoch: active.epoch,
            document: {
              schema: 1,
              file_name: file.name,
              content_type: file.type || 'application/octet-stream',
            } satisfies AttachmentDocument,
          }),
        ])
      const declaration = await api.declareInfoDocumentFile(
        state.selectedProjectId,
        document.wire.id,
        {
          id: blockId,
          blob: {
            blob_id: blobId,
            resource_node_id: document.wire.resource_node_id,
            ciphertext_size: ciphertext.size,
            ciphertext_sha256: ciphertextSha256,
            key_epoch: active.epoch,
            encrypted_blob_metadata: encryptedBlobMetadata,
            encrypted_attachment_metadata: encryptedMetadata,
          },
          idempotency_key: crypto.randomUUID(),
        },
      )
      await api.uploadAttachmentCiphertext(
        state.selectedProjectId,
        blobId,
        ciphertext,
        declaration.upload_url,
      )
      await api.finalizeAttachment(state.selectedProjectId, blobId)
      return {
        id: blockId,
        type: 'file',
        blob_id: blobId,
        file_name: file.name,
        content_type: file.type || 'application/octet-stream',
        plaintext_size: file.size,
      }
    } catch (error) {
      await removeEncryptedAttachment(blobId).catch(() => undefined)
      throw error
    }
  }

  const readInfoDocumentFile = async (
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<Blob> => {
    const current = requireServices()
    const resourceKey = current.auth.vault.getResourceKey(
      document.wire.resource_node_id,
      document.wire.key_epoch,
    )
    if (!resourceKey) {
      throw new Error('This device cannot decrypt the info-document resource')
    }
    const attachment = await api.getAttachment(
      document.wire.project_id,
      file.blob_id,
    )
    const ciphertext = asAttachmentCiphertext(
      await api.downloadCiphertext(
        `/v1/projects/${document.wire.project_id}/files/${file.blob_id}/content`,
      ),
    )
    if (
      ciphertext.size !== attachment.ciphertext_size ||
      (await attachmentCiphertextSha256(ciphertext)) !==
        attachment.ciphertext_sha256
    ) {
      throw new Error('Downloaded info file failed ciphertext integrity checks')
    }
    let plaintext: Uint8Array | undefined
    try {
      plaintext = await decryptAttachment(ciphertext, resourceKey, {
        projectId: document.wire.project_id,
        resourceId: document.wire.resource_node_id,
        blobId: file.blob_id,
        keyEpoch: document.wire.key_epoch,
      })
      return new Blob(
        [
          plaintext.buffer.slice(
            plaintext.byteOffset,
            plaintext.byteOffset + plaintext.byteLength,
          ) as ArrayBuffer,
        ],
        { type: file.content_type },
      )
    } finally {
      zeroBytes(plaintext)
    }
  }

  const downloadInfoDocumentFile = async (
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<void> => {
    await saveWithDownloadFallback(
      await readInfoDocumentFile(document, file),
      file.file_name,
    )
  }

  const toggleTopicFavorite = async (topic: TopicItem) => {
    if (!topic.document) return
    try {
      const favorite = !topic.document.favorite
      await updateTopicDocument(topic, {
        ...topic.document,
        favorite: favorite || undefined,
      })
      dispatch({
        type: 'set-notice',
        message: favorite
          ? 'Categoria aggiunta ai preferiti.'
          : 'Categoria rimossa dai preferiti.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const deleteTopic = async (topic: TopicItem) => {
    if (!state.selectedProjectId || !state.session) {
      dispatch({
        type: 'set-error',
        message: 'Server sign-in is required to delete a topic.',
      })
      return
    }
    try {
      const current = requireServices()
      await api.deleteTopic(state.selectedProjectId, topic.wire.id)
      const listsToRemove = state.taskLists.filter(
        (list) => list.wire.topic_id === topic.wire.id,
      )
      const listIds = new Set(listsToRemove.map((list) => list.wire.id))
      await current.database.deleteRecord(topic.wire.resource_node_id)
      await Promise.all(
        listsToRemove.map((list) =>
          current.database.deleteRecord(list.wire.resource_node_id),
        ),
      )
      await Promise.all(
        state.tasks
          .filter((task) => listIds.has(task.wire.list_id))
          .map((task) =>
            current.database.deleteRecord(task.wire.resource_node_id),
          ),
      )
      if (
        state.boardFocus.type === 'topic' &&
        state.boardFocus.topicId === topic.wire.id
      ) {
        dispatch({ type: 'set-board-focus', focus: { type: 'generali' } })
      }
      dispatch({
        type: 'set-topics',
        topics: state.topics.filter((item) => item.wire.id !== topic.wire.id),
      })
      dispatch({
        type: 'set-task-lists',
        taskLists: state.taskLists.filter(
          (list) => list.wire.topic_id !== topic.wire.id,
        ),
      })
      dispatch({
        type: 'set-tasks',
        tasks: state.tasks.filter((task) => !listIds.has(task.wire.list_id)),
        lockedTasks: state.lockedTasks.filter(
          (task) => !listIds.has(task.list_id),
        ),
      })
      dispatch({
        type: 'set-notice',
        message: 'Categoria eliminata.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const createTaskList = async (name: string, topicId: Uuid) => {
    if (!state.session || !state.selectedProjectId || !topicId) {
      dispatch({
        type: 'set-error',
        message: 'Server sign-in is required to create a task list.',
      })
      return
    }
    try {
      const current = requireServices()
      const id = crypto.randomUUID()
      const resourceId = crypto.randomUUID()
      const document: TaskListDocument = { schema: 1, name }
      const payload = await createEncryptedResource(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        kind: 'task-list',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document,
      })
      const header = await createEncryptedResourceHeader(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        kind: 'task-list',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document,
      })
      const resourceKey = current.auth.vault.getResourceKey(resourceId)
      const headerKey = current.auth.vault.getHeaderKey(resourceId)
      if (!resourceKey) throw new Error('New task-list key is unavailable')
      if (!headerKey) throw new Error('New task-list header key is unavailable')
      const epoch = await buildInitialResourceEpoch(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        resourceKey,
        headerKey,
        recipientIdentityId: state.session.identity_id,
        packages: await api.listProjectDevicePackages(
          state.selectedProjectId,
        ),
      })
      const { task_list: list } = await api.createTaskList(
        state.selectedProjectId,
        topicId,
        {
          id,
          topic_id: topicId,
          resource_node_id: resourceId,
          payload,
          header,
          ...epoch,
          idempotency_key: crypto.randomUUID(),
        },
      )
      if (import.meta.env.DEV) {
        persistDevVault(state.session, current.auth.vault)
      }
      await putRestRecord(current.database, listRecord(list))
      dispatch({
        type: 'set-task-lists',
        taskLists: [...state.taskLists, { wire: list, document }],
      })
      dispatch({ type: 'select-list', listId: list.id })
      if (state.boardFocus.type !== 'topic') {
        dispatch({
          type: 'set-board-focus',
          focus: { type: 'topic', topicId },
        })
      }
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const createTask = async (input: TaskCreationInput, listId: Uuid) => {
    if (!state.session || !state.selectedProjectId || !listId) {
      dispatch({
        type: 'set-error',
        message: 'Server sign-in is required to create a task.',
      })
      return
    }
    try {
      const current = requireServices()
      const list = state.taskLists.find(
        (candidate) => candidate.wire.id === listId,
      )
      if (!list) throw new Error('Selected task list is unavailable')
      const topic = state.topics.find(
        (candidate) => candidate.wire.id === list.wire.topic_id,
      )
      if (!topic) throw new Error('Task list topic is unavailable')
      const creation = buildTaskCreation(input)
      const id = crypto.randomUUID()
      const resourceId = crypto.randomUUID()
      const document: TaskDocument = creation.document
      const payload = await createEncryptedResource(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        kind: 'task',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document,
      })
      const headerDocument: TaskDocument = {
        schema: 1,
        title: document.title,
      }
      const header = await createEncryptedResourceHeader(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        kind: 'task',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document: headerDocument,
      })
      const selectedValue = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: state.selectedProjectId,
          resourceId,
          kind: 'task',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: creation.selectedValue,
        },
      )
      const resourceKey = current.auth.vault.getResourceKey(resourceId)
      const headerKey = current.auth.vault.getHeaderKey(resourceId)
      if (!resourceKey) throw new Error('New task key is unavailable')
      if (!headerKey) throw new Error('New task header key is unavailable')
      const packages = await api.listProjectDevicePackages(
        state.selectedProjectId,
      )
      const epoch = await buildInitialResourceEpoch(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId,
        resourceKey,
        headerKey,
        recipientIdentityId: state.session.identity_id,
        packages,
      })
      let recurrenceSeriesId: Uuid | null = null
      if (creation.taskKind === 'recurring') {
        recurrenceSeriesId = crypto.randomUUID()
        const encryptedRule = await createEncryptedResource(
          current.auth.vault,
          {
            projectId: state.selectedProjectId,
            resourceId: recurrenceSeriesId,
            kind: 'recurrence',
            aggregateVersion: INITIAL_PAYLOAD_VERSION,
            document: {
              schema: 1,
              starts_at: creation.document.due_at,
              ...creation.document.recurrence,
            },
          },
        )
        await api.createRecurrence(state.selectedProjectId, {
          id: recurrenceSeriesId,
          list_id: listId,
          encrypted_rule: encryptedRule,
          idempotency_key: crypto.randomUUID(),
        })
      }
      const { task: createdTask } = await api.createTask(
        state.selectedProjectId,
        {
        id,
        list_id: listId,
        resource_node_id: resourceId,
        task_kind: creation.taskKind,
        payload,
        header,
        selected_value_snapshot: selectedValue,
        questionnaire_version_id: input.questionnaireVersionId ?? null,
        recurrence_series_id: recurrenceSeriesId,
        occurrence_number: creation.taskKind === 'recurring' ? 1 : null,
        ...epoch,
        idempotency_key: crypto.randomUUID(),
        },
      )
      const project = state.projects.find(
        (candidate) => candidate.wire.id === state.selectedProjectId,
      )
      if (!project) {
        throw new Error('Task hierarchy is unavailable for assignment')
      }
      dispatch({ type: 'select-list', listId })
      const assigneeIdentityId =
        input.assigneeIdentityId ?? state.session.identity_id
      const assignmentId = crypto.randomUUID()
      const hierarchyResources = [
        {
          resourceId: topic.wire.resource_node_id,
          epoch: topic.wire.key_epoch,
          body: false,
        },
        {
          resourceId: list.wire.resource_node_id,
          epoch: list.wire.key_epoch,
          body: false,
        },
        { resourceId, epoch: 1, body: true },
      ]
      const missingHierarchyHeader = hierarchyResources.some(
        (item) =>
          !resolveHierarchyHeaderKey(
            current.auth.vault,
            item.resourceId,
            item.epoch,
          ),
      )
      if (missingHierarchyHeader) {
        await recoverProjectResourceKeys(
          api,
          current,
          state.selectedProjectId,
          state.session.identity_id,
        )
        if (import.meta.env.DEV) {
          persistDevVault(state.session, current.auth.vault)
        }
      }
      const assignmentEnvelopes = (
        await Promise.all(
          hierarchyResources.map(async (hierarchyResource) => {
            const resolvedHeader = resolveHierarchyHeaderKey(
              current.auth.vault,
              hierarchyResource.resourceId,
              hierarchyResource.epoch,
            )
            if (!resolvedHeader) {
              throw new Error(
                'A hierarchy header key is unavailable for task assignment',
              )
            }
            const headerEnvelopes = await buildResourceKeyEnvelopes(
              current.auth.vault,
              {
                projectId: state.selectedProjectId as Uuid,
                resourceId: hierarchyResource.resourceId,
                resourceKey: resolvedHeader.key,
                keyPurpose: 'header',
                recipientIdentityId: assigneeIdentityId,
                packages,
                epoch: resolvedHeader.epoch,
              },
            )
            if (!hierarchyResource.body) return headerEnvelopes
            const exactBody = current.auth.vault.getResourceKey(
              hierarchyResource.resourceId,
              hierarchyResource.epoch,
            )
            const resolvedBody = exactBody
              ? { epoch: hierarchyResource.epoch, key: exactBody }
              : current.auth.vault.getLatestResourceKey(
                  hierarchyResource.resourceId,
                )
            if (!resolvedBody) {
              throw new Error('The assigned task body key is unavailable')
            }
            return [
              ...headerEnvelopes,
              ...(await buildResourceKeyEnvelopes(current.auth.vault, {
                projectId: state.selectedProjectId as Uuid,
                resourceId: hierarchyResource.resourceId,
                resourceKey: resolvedBody.key,
                recipientIdentityId: assigneeIdentityId,
                packages,
                epoch: resolvedBody.epoch,
              })),
            ]
          }),
        )
      ).flat()
      const encryptedAssignment = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: state.selectedProjectId,
          resourceId,
          kind: 'task',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: {
            schema: 1,
            assignment_id: assignmentId,
            assignee_identity_id: assigneeIdentityId,
          },
        },
      )
      const { assignment } = await api.assignTask(
        state.selectedProjectId,
        createdTask.id,
        {
          assignment_id: assignmentId,
          permission_grant_id: crypto.randomUUID(),
          assignee_identity_id: assigneeIdentityId,
          encrypted_payload_b64: encodePayloadContainer(
            encryptedAssignment,
          ),
          envelopes: assignmentEnvelopes,
          idempotency_key: crypto.randomUUID(),
        },
      )
      const task: TaskDto = {
        ...createdTask,
        active_assignment_id: assignment.id,
        active_assignee_identity_id: assignment.assignee_identity_id,
      }
      await putRestRecord(current.database, taskRecord(task))
      dispatch({
        type: 'set-tasks',
        tasks: [...state.tasks, { wire: task, document }],
        lockedTasks: state.lockedTasks,
      })
      dispatch({ type: 'select-task', taskId: task.id })
      if (input.requiredAttachments?.length) {
        const attachmentResourceKey = current.auth.vault.getResourceKey(
          task.resource_node_id,
          task.key_epoch,
        )
        if (!attachmentResourceKey) {
          throw new Error('The new task key is unavailable for attachments')
        }
        for (const file of input.requiredAttachments) {
          const attachmentId = crypto.randomUUID()
          const blobId = crypto.randomUUID()
          const ciphertext = await encryptAttachment(
            file,
            attachmentResourceKey,
            {
              projectId: state.selectedProjectId,
              resourceId: task.resource_node_id,
              blobId,
              keyEpoch: task.key_epoch,
            },
          )
          await writeEncryptedAttachment(blobId, ciphertext)
          const [ciphertextSha256, encryptedBlobMetadata, encryptedMetadata] =
            await Promise.all([
              attachmentCiphertextSha256(ciphertext),
              encryptExistingResource(current.auth.vault, {
                projectId: state.selectedProjectId,
                resourceId: task.resource_node_id,
                kind: 'attachment',
                aggregateVersion: 0,
                keyEpoch: task.key_epoch,
                document: {
                  schema: 1,
                  format: 'sprout-attachment-v1',
                  plaintext_size: file.size,
                },
              }),
              encryptExistingResource(current.auth.vault, {
                projectId: state.selectedProjectId,
                resourceId: task.resource_node_id,
                kind: 'attachment',
                aggregateVersion: 0,
                keyEpoch: task.key_epoch,
                document: {
                  schema: 1,
                  file_name: file.name,
                  content_type: file.type || 'application/octet-stream',
                } satisfies AttachmentDocument,
              }),
            ])
          const declaration = await api.declareTaskRequiredAttachment(
            state.selectedProjectId,
            task.id,
            {
              id: attachmentId,
              source_template_attachment_id: null,
              blob: {
                blob_id: blobId,
                resource_node_id: task.resource_node_id,
                ciphertext_size: ciphertext.size,
                ciphertext_sha256: ciphertextSha256,
                key_epoch: task.key_epoch,
                encrypted_blob_metadata: encryptedBlobMetadata,
                encrypted_attachment_metadata: encryptedMetadata,
              },
              idempotency_key: crypto.randomUUID(),
            },
          )
          await api.uploadAttachmentCiphertext(
            state.selectedProjectId,
            blobId,
            await readEncryptedAttachment(blobId),
            declaration.upload_url,
          )
          await api.finalizeAttachment(state.selectedProjectId, blobId)
        }
        await refreshTaskAttachments(task.id)
        dispatch({
          type: 'set-notice',
          message:
            input.requiredAttachments.length === 1
              ? 'Task creato con 1 allegato.'
              : `Task creato con ${input.requiredAttachments.length} allegati.`,
        })
      }
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const updateTask = async (
    task: DecryptedTask,
    input: {
      title: string
      notes?: string
      priority?: TaskDocument['priority']
      start_at?: string
      due_at?: string
      recurrence?: TaskDocument['recurrence']
    },
  ) => {
    const actorIdentityId =
      state.session?.identity_id ?? state.localAccess?.identityId
    const actorDeviceId =
      state.session?.device_id ?? state.localAccess?.deviceId
    if (!actorIdentityId || !actorDeviceId) {
      dispatch({
        type: 'set-error',
        message:
          'This local vault predates offline signing metadata; reauthenticate or recover it on an authorized device.',
      })
      return
    }
    try {
      const current = requireServices()
      const document: TaskDocument = {
        ...task.document,
        title: input.title,
        notes: input.notes,
        ...(input.priority !== undefined ? { priority: input.priority } : {}),
        ...(input.start_at !== undefined ? { start_at: input.start_at } : {}),
        ...(input.due_at !== undefined ? { due_at: input.due_at } : {}),
        ...(input.recurrence !== undefined
          ? { recurrence: input.recurrence }
          : {}),
      }
      if (document.due_at === undefined) {
        delete document.start_at
      }
      const activeKey = state.session
        ? await ensureActiveResourceKey(
            api,
            current,
            state.session,
            task.wire.project_id,
            task.wire.resource_node_id,
            task.wire.key_epoch,
            'Missing active task resource key',
          )
        : resolveActiveResourceKey(
            current.auth.vault,
            task.wire.resource_node_id,
            task.wire.key_epoch,
          )
      if (!activeKey) throw new Error('Missing active task resource key')
      const payload = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: task.wire.project_id,
          resourceId: task.wire.resource_node_id,
          kind: 'task',
          aggregateVersion: task.wire.payload_version + 1,
          keyEpoch: activeKey.epoch,
          document,
        },
      )
      const selectedValueSnapshot = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: task.wire.project_id,
          resourceId: task.wire.resource_node_id,
          kind: 'task',
          aggregateVersion: task.wire.payload_version + 1,
          keyEpoch: activeKey.epoch,
          document: {
            schema: 1,
            priority: document.priority,
            start_at: document.start_at,
            due_at: document.due_at,
            recurrence: document.recurrence,
          } satisfies TaskSelectedValueDocument,
        },
      )
      const body = {
        expected_payload_version: task.wire.payload_version,
        key_epoch: activeKey.epoch,
        payload,
        selected_value_snapshot: selectedValueSnapshot,
        idempotency_key: crypto.randomUUID(),
      }
      const [localRecord, queued] = await Promise.all([
        current.database.getRecord(task.wire.resource_node_id),
        current.database.listQueue(task.wire.project_id),
      ])
      const syncBaseVersion = queued
        .filter(
          (item) =>
            item.request.resource_node_id === task.wire.resource_node_id,
        )
        .reduce(
          (version, item) =>
            Math.max(version, item.request.aggregate_version),
          localRecord?.aggregateVersion ?? 0,
        )
      let nextSyncVersion = syncBaseVersion
      let wire: TaskDto
      if (state.online && state.session) {
        wire = (
          await api.updateTask(
            task.wire.project_id,
            task.wire.id,
            body,
          )
        ).task
      } else {
        wire = {
          ...task.wire,
          payload,
          payload_version: task.wire.payload_version + 1,
        }
        await createSignedQueueItem(current.database, current.auth.vault, {
          projectId: task.wire.project_id,
          resourceId: task.wire.resource_node_id,
          identityId: actorIdentityId,
          deviceId: actorDeviceId,
          deviceKeyVersion: current.auth.vault.deviceSecrets.keyVersion,
          baseVersion: syncBaseVersion,
          keyEpoch: activeKey.epoch,
          eventKind: 'task.updated',
          mutation: 'upsert',
          encryptedPayload: payload,
          restMutation: {
            path: `/v1/projects/${task.wire.project_id}/tasks/${task.wire.id}`,
            method: 'PUT',
            body,
          },
        })
        nextSyncVersion += 1
      }
      await current.database.putRecord({
        ...taskRecord(wire),
        aggregateVersion: nextSyncVersion,
      })
      dispatch({
        type: 'set-tasks',
        tasks: state.tasks.map((item) =>
          item.wire.id === task.wire.id
            ? { wire, document }
            : item,
        ),
        lockedTasks: state.lockedTasks,
      })
      dispatch({
        type: 'set-queue-count',
        count: await current.database.queueCount(),
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const assignTask = async (
    task: DecryptedTask,
    assigneeIdentityId: Uuid,
  ) => {
    if (!state.session || !state.selectedProjectId) {
      dispatch({
        type: 'set-error',
        message: 'Server sign-in is required to assign a task.',
      })
      return
    }
    if (task.wire.active_assignee_identity_id === assigneeIdentityId) {
      return
    }
    try {
      const current = requireServices()
      const projectId = state.selectedProjectId
      let workingTask = task

      if (workingTask.wire.active_assignment_id) {
        const listed = await api.listTaskAssignments(
          projectId,
          workingTask.wire.id,
        )
        const activeAssignment = listed.assignments.find(
          (item) =>
            item.id === listed.active_assignment_id &&
            item.revoked_at == null,
        )
        if (!activeAssignment) {
          throw new Error('Active task assignment is unavailable')
        }
        const [plan, revokePackages] = await Promise.all([
          api.getResourceRotationPlan(
            projectId,
            workingTask.wire.resource_node_id,
            activeAssignment.permission_root_grant_id,
          ),
          api.listProjectDevicePackages(projectId),
        ])
        if (plan.revoked_identity_id !== activeAssignment.assignee_identity_id) {
          throw new Error('Assignment rotation plan identity mismatch')
        }
        const builtRotations: Awaited<
          ReturnType<typeof buildResourceEpochRotation>
        >[] = []
        try {
          for (const resource of plan.resources) {
            const previousKeyCommitment = base64ToBytes(
              resource.previous_key_commitment_b64,
            )
            const previousHeaderKeyCommitment =
              resource.previous_header_key_commitment_b64 === null
                ? undefined
                : base64ToBytes(resource.previous_header_key_commitment_b64)
            try {
              builtRotations.push(
                await buildResourceEpochRotation(current.auth.vault, {
                  projectId,
                  resourceId: resource.resource_id,
                  previousEpochId: resource.previous_epoch_id,
                  currentEpoch: resource.current_epoch,
                  previousKeyCommitment,
                  previousHeaderKeyCommitment,
                  recipientIdentityIds: resource.recipient_identity_ids,
                  bodyRecipientIdentityIds:
                    resource.body_recipient_identity_ids,
                  headerRecipientIdentityIds:
                    resource.header_recipient_identity_ids,
                  packages: revokePackages,
                }),
              )
            } finally {
              zeroBytes(previousKeyCommitment, previousHeaderKeyCommitment)
            }
          }
          await api.revokeTaskAssignment(
            projectId,
            workingTask.wire.id,
            activeAssignment.id,
            {
              rotations: builtRotations.map((item) => item.rotation),
              idempotency_key: crypto.randomUUID(),
            },
          )
          await Promise.all(
            builtRotations.flatMap((item) => [
              current.auth.vault.putResourceKey(
                item.rotation.resource_id,
                item.resourceKey,
                item.rotation.new_epoch,
              ),
              ...(item.headerKey
                ? [
                    current.auth.vault.putResourceKey(
                      item.rotation.resource_id,
                      item.headerKey,
                      item.rotation.new_epoch,
                      'header',
                    ),
                  ]
                : []),
            ]),
          )
          const rotatedTask = builtRotations.find(
            (item) =>
              item.rotation.resource_id === workingTask.wire.resource_node_id,
          )
          workingTask = {
            ...workingTask,
            wire: {
              ...workingTask.wire,
              active_assignment_id: null,
              active_assignee_identity_id: null,
              key_epoch:
                rotatedTask?.rotation.new_epoch ?? workingTask.wire.key_epoch,
            },
          }
        } finally {
          zeroBytes(
            ...builtRotations.flatMap((item) => [
              item.resourceKey,
              item.headerKey,
            ]),
          )
        }
      }

      const list = state.taskLists.find(
        (candidate) => candidate.wire.id === workingTask.wire.list_id,
      )
      if (!list) throw new Error('Task list is unavailable for assignment')
      const topic = state.topics.find(
        (candidate) => candidate.wire.id === list.wire.topic_id,
      )
      if (!topic) throw new Error('Task topic is unavailable for assignment')
      const packages = await api.listProjectDevicePackages(projectId)
      const assignmentId = crypto.randomUUID()
      const latestTaskBody = current.auth.vault.getLatestResourceKey(
        workingTask.wire.resource_node_id,
      )
      const taskBodyEpoch =
        latestTaskBody?.epoch ?? workingTask.wire.key_epoch
      const hierarchyResources = [
        {
          resourceId: topic.wire.resource_node_id,
          epoch: topic.wire.key_epoch,
          body: false,
        },
        {
          resourceId: list.wire.resource_node_id,
          epoch: list.wire.key_epoch,
          body: false,
        },
        {
          resourceId: workingTask.wire.resource_node_id,
          epoch: taskBodyEpoch,
          body: true,
        },
      ]
      const missingHierarchyHeader = hierarchyResources.some(
        (item) =>
          !resolveHierarchyHeaderKey(
            current.auth.vault,
            item.resourceId,
            item.epoch,
          ),
      )
      if (missingHierarchyHeader) {
        await recoverProjectResourceKeys(
          api,
          current,
          projectId,
          state.session.identity_id,
        )
        if (import.meta.env.DEV) {
          persistDevVault(state.session, current.auth.vault)
        }
      }
      const assignmentEnvelopes = (
        await Promise.all(
          hierarchyResources.map(async (hierarchyResource) => {
            const resolvedHeader = resolveHierarchyHeaderKey(
              current.auth.vault,
              hierarchyResource.resourceId,
              hierarchyResource.epoch,
            )
            if (!resolvedHeader) {
              throw new Error(
                'A hierarchy header key is unavailable for task assignment',
              )
            }
            const headerEnvelopes = await buildResourceKeyEnvelopes(
              current.auth.vault,
              {
                projectId,
                resourceId: hierarchyResource.resourceId,
                resourceKey: resolvedHeader.key,
                keyPurpose: 'header',
                recipientIdentityId: assigneeIdentityId,
                packages,
                epoch: resolvedHeader.epoch,
              },
            )
            if (!hierarchyResource.body) return headerEnvelopes
            const exactBody = current.auth.vault.getResourceKey(
              hierarchyResource.resourceId,
              hierarchyResource.epoch,
            )
            const resolvedBody = exactBody
              ? { epoch: hierarchyResource.epoch, key: exactBody }
              : current.auth.vault.getLatestResourceKey(
                  hierarchyResource.resourceId,
                )
            if (!resolvedBody) {
              throw new Error('The assigned task body key is unavailable')
            }
            return [
              ...headerEnvelopes,
              ...(await buildResourceKeyEnvelopes(current.auth.vault, {
                projectId,
                resourceId: hierarchyResource.resourceId,
                resourceKey: resolvedBody.key,
                recipientIdentityId: assigneeIdentityId,
                packages,
                epoch: resolvedBody.epoch,
              })),
            ]
          }),
        )
      ).flat()
      const encryptedAssignment = await encryptExistingResource(
        current.auth.vault,
        {
          projectId,
          resourceId: workingTask.wire.resource_node_id,
          kind: 'task',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: {
            schema: 1,
            assignment_id: assignmentId,
            assignee_identity_id: assigneeIdentityId,
          },
        },
      )
      const { assignment } = await api.assignTask(
        projectId,
        workingTask.wire.id,
        {
          assignment_id: assignmentId,
          permission_grant_id: crypto.randomUUID(),
          assignee_identity_id: assigneeIdentityId,
          encrypted_payload_b64: encodePayloadContainer(encryptedAssignment),
          envelopes: assignmentEnvelopes,
          idempotency_key: crypto.randomUUID(),
        },
      )
      const updatedTask: TaskDto = {
        ...workingTask.wire,
        active_assignment_id: assignment.id,
        active_assignee_identity_id: assignment.assignee_identity_id,
      }
      await putRestRecord(current.database, taskRecord(updatedTask))
      dispatch({
        type: 'set-tasks',
        tasks: state.tasks.map((item) =>
          item.wire.id === workingTask.wire.id
            ? { ...item, wire: updatedTask }
            : item,
        ),
        lockedTasks: state.lockedTasks,
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const completeTask = async (task: DecryptedTask) => {
    if (!state.session) {
      dispatch({
        type: 'set-error',
        message: 'Reconnect and authenticate before completing a task.',
      })
      return
    }
    if (!task.wire.active_assignment_id) {
      dispatch({
        type: 'set-error',
        message: 'Only the active assignee can complete this task.',
      })
      return
    }
    try {
      const current = requireServices()
      const completedAt = new Date().toISOString()
      const encryptedCompletion = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: task.wire.project_id,
          resourceId: task.wire.resource_node_id,
          kind: 'task',
          aggregateVersion: task.wire.payload_version,
          document: {
            schema: 1,
            completed_at: completedAt,
            assignment_id: task.wire.active_assignment_id,
          },
        },
      )
      let nextOccurrence:
        | {
            id: Uuid
            resource_node_id: Uuid
            assignment_id: Uuid
            permission_grant_id: Uuid
            encrypted_assignment: EncryptedPayloadDto
            recurrence_series_id: Uuid
            occurrence_number: number
            payload: EncryptedPayloadDto
            header: EncryptedPayloadDto
            selected_value_snapshot: EncryptedPayloadDto
            epoch: {
              id: Uuid
              epoch: number
              creator_device_key_version: number
              key_commitment_b64: string
              header_key_commitment_b64?: string | null
            }
            envelopes: Awaited<
              ReturnType<typeof buildInitialResourceEpoch>
            >['envelopes']
          }
        | undefined
      let nextDocument: TaskDocument | undefined
      if (task.wire.task_kind === 'recurring') {
        const next = buildNextRecurringTask(task)
        const resourceId = crypto.randomUUID()
        const taskId = crypto.randomUUID()
        const assignmentId = crypto.randomUUID()
        const payload = await createEncryptedResource(current.auth.vault, {
          projectId: task.wire.project_id,
          resourceId,
          kind: 'task',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: next.document,
        })
        const header = await createEncryptedResourceHeader(
          current.auth.vault,
          {
            projectId: task.wire.project_id,
            resourceId,
            kind: 'task',
            aggregateVersion: INITIAL_PAYLOAD_VERSION,
            document: { schema: 1, title: next.document.title },
          },
        )
        const selectedValueSnapshot = await encryptExistingResource(
          current.auth.vault,
          {
            projectId: task.wire.project_id,
            resourceId,
            kind: 'task',
            aggregateVersion: INITIAL_PAYLOAD_VERSION,
            document: next.selectedValue,
          },
        )
        const encryptedAssignment = await encryptExistingResource(
          current.auth.vault,
          {
            projectId: task.wire.project_id,
            resourceId,
            kind: 'task',
            aggregateVersion: INITIAL_PAYLOAD_VERSION,
            document: {
              schema: 1,
              assignment_id: assignmentId,
              assignee_identity_id: state.session.identity_id,
            },
          },
        )
        const resourceKey = current.auth.vault.getResourceKey(resourceId)
        const headerKey = current.auth.vault.getHeaderKey(resourceId)
        if (!resourceKey || !headerKey) {
          throw new Error('Next occurrence keys are unavailable')
        }
        const epoch = await buildInitialResourceEpoch(current.auth.vault, {
          projectId: task.wire.project_id,
          resourceId,
          resourceKey,
          headerKey,
          recipientIdentityId: state.session.identity_id,
          packages: await api.listProjectDevicePackages(task.wire.project_id),
        })
        nextOccurrence = {
          id: taskId,
          resource_node_id: resourceId,
          assignment_id: assignmentId,
          permission_grant_id: crypto.randomUUID(),
          encrypted_assignment: encryptedAssignment,
          recurrence_series_id: task.wire.recurrence_series_id as Uuid,
          occurrence_number: next.occurrenceNumber,
          payload,
          header,
          selected_value_snapshot: selectedValueSnapshot,
          ...epoch,
        }
        nextDocument = next.document
      }
      const response = await api.completeTask(
        task.wire.project_id,
        task.wire.id,
        {
          completion_id: crypto.randomUUID(),
          assignment_id: task.wire.active_assignment_id,
          expected_payload_version: task.wire.payload_version,
          encrypted_completion: encryptedCompletion,
          completed_at: completedAt,
          recurrence_series_id: nextOccurrence?.recurrence_series_id ?? null,
          occurrence_number: nextOccurrence?.occurrence_number ?? null,
          next_occurrence: nextOccurrence ?? null,
          idempotency_key: crypto.randomUUID(),
        },
      )
      const completed = response.completed_task
      await putRestRecord(current.database, taskRecord(completed))
      if (response.next_task) {
        await putRestRecord(current.database, taskRecord(response.next_task))
      }
      dispatch({
        type: 'set-tasks',
        tasks: [
          ...state.tasks.map((item) =>
            item.wire.id === completed.id
              ? { ...item, wire: completed }
              : item,
          ),
          ...(response.next_task && nextDocument
            ? [{ wire: response.next_task, document: nextDocument }]
            : []),
        ],
        lockedTasks: state.lockedTasks,
      })
      dispatch({
        type: 'set-notice',
        message: response.next_task
          ? 'Task completed and next recurring occurrence created atomically.'
          : 'Task completed.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const copyTask = async (task: DecryptedTask) => {
    if (!state.session || !state.selectedListId) {
      dispatch({
        type: 'set-error',
        message: 'Select a destination list and authenticate before copying.',
      })
      return
    }
    try {
      const current = requireServices()
      const newTaskId = crypto.randomUUID()
      const newResourceId = crypto.randomUUID()
      const payload = await createEncryptedResource(current.auth.vault, {
        projectId: task.wire.project_id,
        resourceId: newResourceId,
        kind: 'task',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document: task.document,
      })
      const header = await createEncryptedResourceHeader(current.auth.vault, {
        projectId: task.wire.project_id,
        resourceId: newResourceId,
        kind: 'task',
        aggregateVersion: INITIAL_PAYLOAD_VERSION,
        document: { schema: 1, title: task.document.title },
      })
      const selectedValueSnapshot = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: task.wire.project_id,
          resourceId: newResourceId,
          kind: 'task',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: {
            schema: 1,
            due_at: task.document.due_at,
            priority: task.document.priority,
            recurrence: task.document.recurrence,
          },
        },
      )
      const resourceKey = current.auth.vault.getResourceKey(newResourceId)
      const headerKey = current.auth.vault.getHeaderKey(newResourceId)
      if (!resourceKey || !headerKey) {
        throw new Error('Copied task keys are unavailable')
      }
      const epoch = await buildInitialResourceEpoch(current.auth.vault, {
        projectId: task.wire.project_id,
        resourceId: newResourceId,
        resourceKey,
        headerKey,
        recipientIdentityId: state.session.identity_id,
        packages: await api.listProjectDevicePackages(task.wire.project_id),
      })
      const assignmentId = crypto.randomUUID()
      const encryptedAssignment = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: task.wire.project_id,
          resourceId: newResourceId,
          kind: 'task',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: {
            schema: 1,
            assignment_id: assignmentId,
            assignee_identity_id: state.session.identity_id,
          },
        },
      )
      const { task: copied } = await api.copyTask(
        task.wire.project_id,
        task.wire.id,
        {
          destination_list_id: state.selectedListId,
          new_task_id: newTaskId,
          new_resource_node_id: newResourceId,
          assignment_id: assignmentId,
          permission_grant_id: crypto.randomUUID(),
          encrypted_assignment: encryptedAssignment,
          payload,
          header,
          selected_value_snapshot: selectedValueSnapshot,
          ...epoch,
          recurrence_series_id: null,
          occurrence_number: null,
          idempotency_key: crypto.randomUUID(),
        },
      )
      await putRestRecord(current.database, taskRecord(copied))
      dispatch({
        type: 'set-tasks',
        tasks: [...state.tasks, { wire: copied, document: task.document }],
        lockedTasks: state.lockedTasks,
      })
      dispatch({ type: 'select-task', taskId: copied.id })
      dispatch({ type: 'set-notice', message: 'Task copied with a new identity.' })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const materializePresetJourney = async (
    input: PresetMaterializationInput,
  ) => {
    if (
      !state.session ||
      !state.selectedProjectId ||
      !state.selectedListId
    ) {
      throw new Error('Select a task list and authenticate first')
    }
    const current = requireServices()
    const destinationList = state.taskLists.find(
      (list) => list.wire.id === state.selectedListId,
    )
    if (!destinationList) {
      throw new Error('The selected task list is unavailable')
    }
    const built = buildThreePretaskPreset(input)
    const presetId = crypto.randomUUID()
    const versionId = crypto.randomUUID()
    const assignmentId = crypto.randomUUID()
    try {
      const presetPayload = await createEncryptedResource(
        current.auth.vault,
        {
          projectId: state.selectedProjectId,
          resourceId: presetId,
          kind: 'preset',
          aggregateVersion: 0,
          document: {
            schema: 1,
            name: built.name,
          } satisfies PresetDocument,
        },
      )
      await api.createPreset(
        state.selectedProjectId,
        presetId,
        presetPayload,
      )
      const versionPayload = await createEncryptedResource(
        current.auth.vault,
        {
          projectId: state.selectedProjectId,
          resourceId: versionId,
          kind: 'preset',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: { schema: 1, name: built.name },
        },
      )
      const pretaskIds = built.pretasks.map(() => crypto.randomUUID())
      const pretasks = await Promise.all(
        built.pretasks.map(async (pretask, index) => ({
          id: pretaskIds[index] as Uuid,
          task_kind: pretask.taskKind,
          payload: await createEncryptedResource(current.auth.vault, {
            projectId: state.selectedProjectId as Uuid,
            resourceId: pretaskIds[index] as Uuid,
            kind: 'preset',
            aggregateVersion: INITIAL_PAYLOAD_VERSION,
            document: pretask.template,
          }),
        })),
      )
      const contentBytes = new TextEncoder().encode(JSON.stringify(pretasks))
      const contentHash = (await loadCrypto()).hash(contentBytes)
      try {
        await api.createPresetVersion(
          state.selectedProjectId,
          presetId,
          {
            id: versionId,
            payload: versionPayload,
            content_hash_b64: bytesToBase64(contentHash),
            pretasks,
            idempotency_key: crypto.randomUUID(),
          },
        )
      } finally {
        zeroBytes(contentBytes, contentHash)
      }
      const templateAttachments = new Map<
        Uuid,
        { attachmentId: Uuid; file: File }
      >()
      for (const [index, pretask] of built.pretasks.entries()) {
        const file = input.templateAttachments[pretask.taskKind]
        if (!file) continue
        const pretaskId = pretaskIds[index] as Uuid
        const attachmentId = crypto.randomUUID()
        const blobId = crypto.randomUUID()
        const resourceId = destinationList.wire.resource_node_id
        const keyEpoch = destinationList.wire.key_epoch
        const resourceKey = current.auth.vault.getResourceKey(
          resourceId,
          keyEpoch,
        )
        if (!resourceKey) {
          throw new Error('The destination list key is unavailable')
        }
        const ciphertext = await encryptAttachment(file, resourceKey, {
          projectId: state.selectedProjectId,
          resourceId,
          blobId,
          keyEpoch,
        })
        await writeEncryptedAttachment(blobId, ciphertext)
        const [ciphertextSha256, encryptedBlobMetadata, encryptedMetadata] =
          await Promise.all([
            attachmentCiphertextSha256(ciphertext),
            encryptExistingResource(current.auth.vault, {
              projectId: state.selectedProjectId,
              resourceId,
              kind: 'attachment',
              aggregateVersion: 0,
              keyEpoch,
              document: {
                schema: 1,
                format: 'sprout-attachment-v1',
                plaintext_size: file.size,
              },
            }),
            encryptExistingResource(current.auth.vault, {
              projectId: state.selectedProjectId,
              resourceId,
              kind: 'attachment',
              aggregateVersion: 0,
              keyEpoch,
              document: {
                schema: 1,
                file_name: file.name,
                content_type: file.type || 'application/octet-stream',
              } satisfies AttachmentDocument,
            }),
          ])
        const declaration = await api.declarePretaskTemplateAttachment(
          state.selectedProjectId,
          versionId,
          pretaskId,
          {
            id: attachmentId,
            blob: {
              blob_id: blobId,
              resource_node_id: resourceId,
              ciphertext_size: ciphertext.size,
              ciphertext_sha256: ciphertextSha256,
              key_epoch: keyEpoch,
              encrypted_blob_metadata: encryptedBlobMetadata,
              encrypted_attachment_metadata: encryptedMetadata,
            },
            idempotency_key: crypto.randomUUID(),
          },
        )
        await api.uploadAttachmentCiphertext(
          state.selectedProjectId,
          blobId,
          await readEncryptedAttachment(blobId),
          declaration.upload_url,
        )
        await api.finalizeAttachment(state.selectedProjectId, blobId)
        templateAttachments.set(pretaskId, { attachmentId, file })
      }
      const selections = await Promise.all(
        built.pretasks.map(async (pretask, index) => ({
          pretask_id: pretaskIds[index] as Uuid,
          task_kind: pretask.taskKind,
          selected_value: await encryptExistingResource(
            current.auth.vault,
            {
              projectId: state.selectedProjectId as Uuid,
              resourceId: pretaskIds[index] as Uuid,
              kind: 'preset',
              aggregateVersion: INITIAL_PAYLOAD_VERSION,
              document: pretask.selectedValue,
            },
          ),
        })),
      )
      const assignmentPayload = await createEncryptedResource(
        current.auth.vault,
        {
          projectId: state.selectedProjectId,
          resourceId: assignmentId,
          kind: 'preset',
          aggregateVersion: INITIAL_PAYLOAD_VERSION,
          document: {
            schema: 1,
            assigned_at: new Date().toISOString(),
          },
        },
      )
      const { assignment: presetAssignment } =
        await api.createPresetAssignment(state.selectedProjectId, {
        id: assignmentId,
        preset_version_id: versionId,
        destination_list_id: state.selectedListId,
        assigned_to_identity_id: state.session.identity_id,
        payload: assignmentPayload,
        selections,
        idempotency_key: crypto.randomUUID(),
      })
      const packages = await api.listProjectDevicePackages(
        state.selectedProjectId,
      )
      const documents = new Map<Uuid, TaskDocument>()
      const choices = await Promise.all(
        built.pretasks.map(async (pretask, index) => {
          const taskId = crypto.randomUUID()
          const resourceId = crypto.randomUUID()
          const concreteAssignmentId = crypto.randomUUID()
          let recurrenceSeriesId: Uuid | null = null
          if (pretask.taskKind === 'recurring') {
            recurrenceSeriesId = crypto.randomUUID()
            const encryptedRule = await createEncryptedResource(
              current.auth.vault,
              {
                projectId: state.selectedProjectId as Uuid,
                resourceId: recurrenceSeriesId,
                kind: 'recurrence',
                aggregateVersion: INITIAL_PAYLOAD_VERSION,
                document: {
                  schema: 1,
                  starts_at: pretask.task.due_at,
                  ...pretask.task.recurrence,
                },
              },
            )
            await api.createRecurrence(state.selectedProjectId as Uuid, {
              id: recurrenceSeriesId,
              list_id: state.selectedListId,
              encrypted_rule: encryptedRule,
              idempotency_key: crypto.randomUUID(),
            })
          }
          const taskSnapshot = await createEncryptedResource(
            current.auth.vault,
            {
              projectId: state.selectedProjectId as Uuid,
              resourceId,
              kind: 'task',
              aggregateVersion: INITIAL_PAYLOAD_VERSION,
              document: pretask.task,
            },
          )
          const header = await createEncryptedResourceHeader(
            current.auth.vault,
            {
              projectId: state.selectedProjectId as Uuid,
              resourceId,
              kind: 'task',
              aggregateVersion: INITIAL_PAYLOAD_VERSION,
              document: { schema: 1, title: pretask.task.title },
            },
          )
          const selectedValueSnapshot = await encryptExistingResource(
            current.auth.vault,
            {
              projectId: state.selectedProjectId as Uuid,
              resourceId,
              kind: 'task',
              aggregateVersion: INITIAL_PAYLOAD_VERSION,
              document: pretask.selectedValue,
            },
          )
          const encryptedAssignment = await encryptExistingResource(
            current.auth.vault,
            {
              projectId: state.selectedProjectId as Uuid,
              resourceId,
              kind: 'task',
              aggregateVersion: INITIAL_PAYLOAD_VERSION,
              document: {
                schema: 1,
                assignment_id: concreteAssignmentId,
                assignee_identity_id: state.session?.identity_id,
              },
            },
          )
          const resourceKey = current.auth.vault.getResourceKey(resourceId)
          const headerKey = current.auth.vault.getHeaderKey(resourceId)
          if (!resourceKey || !headerKey) {
            throw new Error('Materialized task keys are unavailable')
          }
          const epoch = await buildInitialResourceEpoch(
            current.auth.vault,
            {
              projectId: state.selectedProjectId as Uuid,
              resourceId,
              resourceKey,
              headerKey,
              recipientIdentityId: state.session?.identity_id as Uuid,
              packages,
            },
          )
          documents.set(taskId, pretask.task)
          return {
            pretask_id: pretaskIds[index] as Uuid,
            task_kind: pretask.taskKind,
            task_id: taskId,
            task_resource_node_id: resourceId,
            assignment_id: concreteAssignmentId,
            permission_grant_id: crypto.randomUUID(),
            encrypted_assignment: encryptedAssignment,
            selected_value_snapshot: selectedValueSnapshot,
            task_snapshot: taskSnapshot,
            header,
            recurrence_series_id: recurrenceSeriesId,
            occurrence_number:
              pretask.taskKind === 'recurring' ? 1 : null,
            ...epoch,
          }
        }),
      )
      const response = await api.materializePresetAssignment(
        state.selectedProjectId,
        assignmentId,
        {
          expected_assignment_version: presetAssignment.payload_version,
          choices,
          idempotency_key: crypto.randomUUID(),
        },
      )
      for (const [pretaskId, template] of templateAttachments) {
        const task = response.tasks.find(
          (candidate) => candidate.source_pretask_id === pretaskId,
        )
        if (!task) {
          throw new Error('The template attachment task was not materialized')
        }
        const resourceKey = current.auth.vault.getResourceKey(
          task.resource_node_id,
          task.key_epoch,
        )
        if (!resourceKey) {
          throw new Error('The materialized task key is unavailable')
        }
        const attachmentId = crypto.randomUUID()
        const blobId = crypto.randomUUID()
        const ciphertext = await encryptAttachment(
          template.file,
          resourceKey,
          {
            projectId: state.selectedProjectId,
            resourceId: task.resource_node_id,
            blobId,
            keyEpoch: task.key_epoch,
          },
        )
        await writeEncryptedAttachment(blobId, ciphertext)
        const [ciphertextSha256, encryptedBlobMetadata, encryptedMetadata] =
          await Promise.all([
            attachmentCiphertextSha256(ciphertext),
            encryptExistingResource(current.auth.vault, {
              projectId: state.selectedProjectId,
              resourceId: task.resource_node_id,
              kind: 'attachment',
              aggregateVersion: 0,
              keyEpoch: task.key_epoch,
              document: {
                schema: 1,
                format: 'sprout-attachment-v1',
                plaintext_size: template.file.size,
              },
            }),
            encryptExistingResource(current.auth.vault, {
              projectId: state.selectedProjectId,
              resourceId: task.resource_node_id,
              kind: 'attachment',
              aggregateVersion: 0,
              keyEpoch: task.key_epoch,
              document: {
                schema: 1,
                file_name: template.file.name,
                content_type:
                  template.file.type || 'application/octet-stream',
              } satisfies AttachmentDocument,
            }),
          ])
        const declaration = await api.declareTaskRequiredAttachment(
          state.selectedProjectId,
          task.id,
          {
            id: attachmentId,
            source_template_attachment_id: template.attachmentId,
            blob: {
              blob_id: blobId,
              resource_node_id: task.resource_node_id,
              ciphertext_size: ciphertext.size,
              ciphertext_sha256: ciphertextSha256,
              key_epoch: task.key_epoch,
              encrypted_blob_metadata: encryptedBlobMetadata,
              encrypted_attachment_metadata: encryptedMetadata,
            },
            idempotency_key: crypto.randomUUID(),
          },
        )
        await api.uploadAttachmentCiphertext(
          state.selectedProjectId,
          blobId,
          await readEncryptedAttachment(blobId),
          declaration.upload_url,
        )
        await api.finalizeAttachment(state.selectedProjectId, blobId)
      }
      await Promise.all(
        response.tasks.map((task) =>
          putRestRecord(current.database, taskRecord(task)),
        ),
      )
      dispatch({
        type: 'set-tasks',
        tasks: [
          ...state.tasks,
          ...response.tasks.map((task) => ({
            wire: task,
            document: documents.get(task.id) as TaskDocument,
          })),
        ],
        lockedTasks: state.lockedTasks,
      })
      setPresetResult({
        id: presetId,
        name: built.name,
        detail:
          `${response.tasks.length} immutable task snapshots materialized` +
          (templateAttachments.size > 0
            ? ` with ${templateAttachments.size} required attachment snapshots`
            : ''),
      })
      dispatch({
        type: 'set-notice',
        message:
          'Preset version assigned; task and required attachment snapshots materialized.',
      })
    } catch (error) {
      setPresetResult({
        id: presetId,
        locked: true,
        detail: errorMessage(error),
      })
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const fetchQuestionnaireCatalog = useCallback(async (projectId: Uuid) => {
    const wires: QuestionnaireDto[] = []
    let cursor: string | undefined
    do {
      const page = await api.listQuestionnaires(projectId, cursor)
      wires.push(...page.questionnaires)
      cursor = page.next_cursor ?? undefined
    } while (cursor)
    const versionsByQuestionnaire = await Promise.all(
      wires.map(async (questionnaire) => ({
        questionnaire,
        versions: (
          await api.listQuestionnaireVersions(projectId, questionnaire.id)
        ).versions,
      })),
    )
    return { wires, versionsByQuestionnaire }
  }, [api])

  const refreshQuestionnaires = useCallback(async () => {
    if (!state.selectedProjectId) {
      setQuestionnaires([])
      setQuestionnaireVersions([])
      return
    }
    if (!services) return
    try {
      const current = services
      const catalog = await fetchQuestionnaireCatalog(
        state.selectedProjectId,
      )
      const items = await Promise.all(
        catalog.wires.map(async (wire): Promise<QuestionnaireItem> => {
          try {
            const key = current.auth.vault.getResourceKey(wire.id)
            if (!key) throw new Error('Missing questionnaire key')
            return {
              wire,
              document: await decryptDocument<QuestionnaireDocument>(
                wire.payload,
                {
                  projectId: wire.project_id,
                  resourceId: wire.id,
                  kind: 'questionnaire',
                  aggregateVersion: 0,
                  keyEpoch: 1,
                  resourceKey: key,
                },
              ),
            }
          } catch (error) {
            return { wire, lockedReason: errorMessage(error) }
          }
        }),
      )
      const decryptedVersions: DecryptedQuestionnaireVersion[] = []
      for (const entry of catalog.versionsByQuestionnaire) {
        for (const version of entry.versions) {
          try {
            decryptedVersions.push(
              await decryptQuestionnaireVersion(current.auth.vault, version),
            )
          } catch {
            // Keep locked questionnaire content out of application state.
          }
        }
      }
      setQuestionnaires(items)
      setQuestionnaireVersions(decryptedVersions)
      setSelectedQuestionnaireId((selected) =>
        selected && items.some((item) => item.wire.id === selected)
          ? selected
          : items[0]?.wire.id,
      )
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }, [
    dispatch,
    fetchQuestionnaireCatalog,
    services,
    state.selectedProjectId,
  ])

  const createQuestionnaire = async (title: string) => {
    if (!state.selectedProjectId || !state.session) {
      throw new Error('Select a project and sign in before creating a questionnaire')
    }
    try {
      const current = requireServices()
      const id = crypto.randomUUID()
      const payload = await createEncryptedResource(current.auth.vault, {
        projectId: state.selectedProjectId,
        resourceId: id,
        kind: 'questionnaire',
        aggregateVersion: 0,
        document: {
          schema: 1,
          title,
        } satisfies QuestionnaireDocument,
      })
      await api.createQuestionnaire(state.selectedProjectId, id, payload)
      setSelectedQuestionnaireId(id)
      await refreshQuestionnaires()
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
      throw error
    }
  }

  const saveQuestionnaireVersion = async (input: {
    draft?: DecryptedQuestionnaireVersion
    sourceVersionId?: Uuid
    description?: string
    questions: QuestionnaireEditorQuestion[]
  }) => {
    if (!state.selectedProjectId || !selectedQuestionnaireId) {
      throw new Error('Select a questionnaire before creating a version')
    }
    try {
      const versionId = input.draft?.wire.id ?? crypto.randomUUID()
      const encrypted = await encryptQuestionnaireVersion(
        requireServices().auth.vault,
        {
          projectId: state.selectedProjectId,
          questionnaireId: selectedQuestionnaireId,
          versionId,
          description: input.description,
          questions: input.questions,
          preserveIds: Boolean(input.draft),
        },
      )
      if (input.draft) {
        await api.updateQuestionnaireDraft(
          state.selectedProjectId,
          selectedQuestionnaireId,
          versionId,
          {
            expected_revision: input.draft.wire.revision,
            schema: encrypted.schema,
            content_hash_b64: encrypted.contentHashB64,
            questions: encrypted.questions,
            idempotency_key: crypto.randomUUID(),
          },
        )
      } else {
        const source = input.sourceVersionId
          ? questionnaireVersions.find(
              (version) => version.wire.id === input.sourceVersionId,
            )
          : undefined
        if (source && source.wire.state !== 'published') {
          throw new Error('Only a published version can seed a new draft')
        }
        await api.createQuestionnaireVersion(
          state.selectedProjectId,
          selectedQuestionnaireId,
          {
            id: versionId,
            source_version_id: source?.wire.id ?? null,
            schema: encrypted.schema,
            content_hash_b64: encrypted.contentHashB64,
            questions: encrypted.questions,
            idempotency_key: crypto.randomUUID(),
          },
        )
      }
      await refreshQuestionnaires()
      dispatch({
        type: 'set-notice',
        message: 'Encrypted questionnaire draft saved.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
      throw error
    }
  }

  const publishQuestionnaireVersion = async (
    version: DecryptedQuestionnaireVersion,
  ) => {
    try {
      await api.publishQuestionnaireVersion(
        version.wire.project_id,
        version.wire.questionnaire_id,
        version.wire.id,
        version.wire.revision,
      )
      await refreshQuestionnaires()
      dispatch({
        type: 'set-notice',
        message:
          'Questionnaire version published. Further edits create a new version.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
      throw error
    }
  }

  const loadTaskQuestionnaire = async (taskId: Uuid) => {
    const task = state.tasks.find((candidate) => candidate.wire.id === taskId)
    if (
      !task ||
      !task.wire.questionnaire_version_id ||
      !state.selectedProjectId
    ) {
      throw new Error('The selected task has no pinned questionnaire')
    }
    try {
      let decrypted = questionnaireVersions.find(
        (version) => version.wire.id === task.wire.questionnaire_version_id,
      )
      if (!decrypted) {
        const catalog = await fetchQuestionnaireCatalog(
          state.selectedProjectId,
        )
        let pinned: QuestionnaireVersionDto | undefined
        for (const entry of catalog.versionsByQuestionnaire) {
          try {
            pinned = selectImmutableQuestionnaireVersion(
              entry.versions,
              task.wire.questionnaire_version_id,
            )
            break
          } catch {
            // Continue until the exact task pin is found.
          }
        }
        if (!pinned) {
          throw new Error('The task-pinned questionnaire version is unavailable')
        }
        decrypted = await decryptQuestionnaireVersion(
          requireServices().auth.vault,
          pinned,
        )
      }
      if (decrypted.wire.id !== task.wire.questionnaire_version_id) {
        throw new Error('Questionnaire version selection changed unexpectedly')
      }
      setTaskQuestionnaireVersion(decrypted)
      try {
        const response = await api.getQuestionnaireSubmission(
          task.wire.project_id,
          task.wire.id,
        )
        setQuestionnaireSubmission(response.submission)
        const resourceKey = requireServices().auth.vault.getResourceKey(
          task.wire.resource_node_id,
          task.wire.key_epoch,
        )
        if (!resourceKey) {
          throw new Error('This questionnaire task key is unavailable')
        }
        const hydrated = await Promise.all(
          response.submission.answers.map(async (answer) => {
            const question = decrypted.questions.find(
              (candidate) => candidate.id === answer.question_id,
            )
            if (!question) {
              throw new Error('A saved answer references an unknown question')
            }
            const document = await decryptDocument<QuestionnaireAnswerDocument>(
              answer.payload,
              {
                projectId: task.wire.project_id,
                resourceId: task.wire.resource_node_id,
                kind: 'questionnaire',
                aggregateVersion: 0,
                keyEpoch: task.wire.key_epoch,
                resourceKey,
              },
            )
            const value: QuestionnaireAnswerValue =
              question.questionKind === 'single_choice'
                ? (answer.selected_option_ids[0] ?? '')
                : question.questionKind === 'multiple_choice'
                  ? answer.selected_option_ids
                  : (document.value ?? '')
            return [question.id, value] as const
          }),
        )
        setQuestionnaireSubmissionAnswers(Object.fromEntries(hydrated))
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) {
          setQuestionnaireSubmission(undefined)
          setQuestionnaireSubmissionAnswers({})
        } else {
          throw error
        }
      }
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
      throw error
    }
  }

  const submitTaskQuestionnaire = async (
    task: DecryptedTask,
    version: DecryptedQuestionnaireVersion,
    values: Record<Uuid, QuestionnaireAnswerValue>,
  ) => {
    if (!state.session) {
      throw new Error('Sign in before submitting a questionnaire')
    }
    if (version.wire.id !== task.wire.questionnaire_version_id) {
      throw new Error('The loaded questionnaire does not match the task pin')
    }
    let signingMessage: Uint8Array | undefined
    try {
      try {
        const currentSubmission = await api.getQuestionnaireSubmission(
          task.wire.project_id,
          task.wire.id,
        )
        if (currentSubmission.submission.state === 'submitted') {
          setQuestionnaireSubmission(currentSubmission.submission)
          dispatch({
            type: 'set-notice',
            message: 'This questionnaire was already submitted.',
          })
          return
        }
      } catch (error) {
        if (!(error instanceof ApiError && error.status === 404)) {
          throw error
        }
      }
      const current = requireServices()
      const answerInputs = validateQuestionnaireAnswers(version, values)
      const answers = await Promise.all(
        answerInputs.map(async ({ question, value }) => ({
          id: crypto.randomUUID(),
          question_id: question.id,
          selected_option_ids:
            question.questionKind === 'single_choice'
              ? [value as string]
              : question.questionKind === 'multiple_choice'
                ? (value as string[])
                : [],
          payload: await encryptExistingResource(current.auth.vault, {
            projectId: task.wire.project_id,
            resourceId: task.wire.resource_node_id,
            kind: 'questionnaire',
            aggregateVersion: 0,
            document: {
              schema: 1,
              value:
                question.questionKind === 'open' ||
                question.questionKind === 'boolean'
                  ? (value as string | boolean)
                  : null,
            } satisfies QuestionnaireAnswerDocument,
          }),
        })),
      )
      const submissionId =
        questionnaireSubmission?.task_id === task.wire.id &&
        questionnaireSubmission.state === 'draft'
          ? questionnaireSubmission.id
          : crypto.randomUUID()
      const encryptedPayload = await encryptExistingResource(
        current.auth.vault,
        {
          projectId: task.wire.project_id,
          resourceId: task.wire.resource_node_id,
          kind: 'questionnaire',
          aggregateVersion: 0,
          document: {
            schema: 1,
            questionnaire_version_id: version.wire.id,
          },
        },
      )
      const draftResponse = await api.upsertQuestionnaireSubmissionDraft(
        task.wire.project_id,
        task.wire.id,
        buildAssigneeSubmissionRequest({
          task: task.wire,
          identityId: state.session.identity_id,
          submissionId,
          expectedRevision:
            questionnaireSubmission?.state === 'draft'
              ? questionnaireSubmission.revision
              : null,
          encryptedPayload,
          answers,
          idempotencyKey: crypto.randomUUID(),
        }),
      )
      signingMessage = questionnaireSubmissionSigningMessage({
        projectId: task.wire.project_id,
        taskId: task.wire.id,
        submissionId: draftResponse.submission.id,
        expectedRevision: draftResponse.submission.revision,
      })
      const signatures = await signDual(
        current.auth.vault.deviceSecrets,
        signingMessage,
        QUESTIONNAIRE_SUBMISSION_SIGNATURE_CONTEXT,
      )
      try {
        const submitted = await submitQuestionnaireRecoveringLostResponse(
          api,
          task.wire.project_id,
          task.wire.id,
          draftResponse.submission.id,
          {
            expected_revision: draftResponse.submission.revision,
            signer_device_key_version:
              current.auth.vault.deviceSecrets.keyVersion,
            classical_signature_b64: bytesToBase64(
              signatures.classicalSignature,
            ),
            post_quantum_signature_b64: bytesToBase64(
              signatures.postQuantumSignature,
            ),
            idempotency_key: draftResponse.submission.id,
          },
        )
        setQuestionnaireSubmission(submitted.submission)
        dispatch({
          type: 'set-notice',
          message: 'Questionnaire encrypted, signed, and submitted.',
        })
      } finally {
        zeroBytes(
          signatures.classicalSignature,
          signatures.postQuantumSignature,
        )
      }
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
      throw error
    } finally {
      zeroBytes(signingMessage)
    }
  }

  const refreshTaskAttachments = useCallback(
    async (taskId: Uuid) => {
      if (!state.selectedProjectId) return
      const inFlight = attachmentRefreshInFlight.current.get(taskId)
      if (inFlight) return inFlight

      const refresh = (async () => {
        try {
          const projectId = state.selectedProjectId as Uuid
          const loadAll = async (
            loadPage: (
              cursor?: string,
            ) => Promise<{
              attachments: AttachmentCollectionItemDto[]
              next_cursor: string | null
            }>,
          ) => {
            const values: AttachmentCollectionItemDto[] = []
            let cursor: string | undefined
            do {
              const page = await loadPage(cursor)
              values.push(...page.attachments)
              cursor = page.next_cursor ?? undefined
            } while (cursor)
            return values
          }
          const [required, completed] = await Promise.all([
            loadAll((cursor) =>
              api.listTaskRequiredAttachments(projectId, taskId, cursor),
            ),
            loadAll((cursor) =>
              api.listTaskCompletedAttachments(projectId, taskId, cursor),
            ),
          ])
          const nextAttachments = [...required, ...completed]
          setAttachments(nextAttachments)

          let current: Services | undefined
          try {
            current = requireServices()
          } catch {
            current = undefined
          }
          if (!current) return

          const labels = await Promise.all(
            nextAttachments.map(async (item) => {
              if (!item.encrypted_metadata) {
                return [item.id, 'Allegato'] as const
              }
              try {
                const resourceKey = current.auth.vault.getResourceKey(
                  item.resource_node_id,
                  item.key_epoch,
                )
                if (!resourceKey) {
                  return [item.id, 'Allegato'] as const
                }
                const metadata = await decryptDocument<AttachmentDocument>(
                  item.encrypted_metadata,
                  {
                    projectId: item.project_id,
                    resourceId: item.resource_node_id,
                    kind: 'attachment',
                    aggregateVersion: 0,
                    keyEpoch: item.key_epoch,
                    resourceKey,
                  },
                )
                return [item.id, metadata.file_name] as const
              } catch {
                return [item.id, 'Allegato'] as const
              }
            }),
          )
          setAttachmentLabels((previous) => {
            const next = { ...previous }
            for (const [id, label] of labels) {
              next[id] = label
            }
            return next
          })
        } catch (error) {
          dispatch({ type: 'set-error', message: errorMessage(error) })
        } finally {
          attachmentRefreshInFlight.current.delete(taskId)
        }
      })()

      attachmentRefreshInFlight.current.set(taskId, refresh)
      return refresh
    },
    [api, dispatch, requireServices, state.selectedProjectId],
  )

  const uploadCompletedAttachment = async (
    task: DecryptedTask,
    file: File,
    requiredAttachmentId?: Uuid,
  ) => {
    if (!state.session || !task.wire.active_assignment_id) {
      throw new Error('Only the active assignee can upload a completed file')
    }
    const current = requireServices()
    const resourceKey = current.auth.vault.getResourceKey(
      task.wire.resource_node_id,
      task.wire.key_epoch,
    )
    if (!resourceKey) {
      throw new Error('This device is not authorized to decrypt the task resource')
    }
    const blobId = crypto.randomUUID()
    const attachmentId = crypto.randomUUID()
    const context = {
      projectId: task.wire.project_id,
      resourceId: task.wire.resource_node_id,
      blobId,
      keyEpoch: task.wire.key_epoch,
    }
    const ciphertext = await encryptAttachment(file, resourceKey, context)
    let queued = false
    try {
      await writeEncryptedAttachment(blobId, ciphertext)
      const [ciphertextSha256, encryptedBlobMetadata, encryptedAttachmentMetadata] =
        await Promise.all([
          attachmentCiphertextSha256(ciphertext),
          encryptExistingResource(current.auth.vault, {
            projectId: task.wire.project_id,
            resourceId: task.wire.resource_node_id,
            kind: 'attachment',
            aggregateVersion: 0,
            keyEpoch: task.wire.key_epoch,
            document: {
              schema: 1,
              format: 'sprout-attachment-v1',
              plaintext_size: file.size,
            },
          }),
          encryptExistingResource(current.auth.vault, {
            projectId: task.wire.project_id,
            resourceId: task.wire.resource_node_id,
            kind: 'attachment',
            aggregateVersion: 0,
            keyEpoch: task.wire.key_epoch,
            document: {
              schema: 1,
              file_name: file.name,
              content_type: file.type || 'application/octet-stream',
            } satisfies AttachmentDocument,
          }),
        ])
      await enqueueCompletedAttachment({
        id: attachmentId,
        identityId: state.session.identity_id,
        projectId: task.wire.project_id,
        taskId: task.wire.id,
        blobId,
        queuedAt: new Date().toISOString(),
        attempts: 0,
        request: {
          id: attachmentId,
          assignment_id: task.wire.active_assignment_id,
          required_attachment_id: requiredAttachmentId ?? null,
          blob: {
            blob_id: blobId,
            resource_node_id: task.wire.resource_node_id,
            ciphertext_size: ciphertext.size,
            ciphertext_sha256: ciphertextSha256,
            key_epoch: task.wire.key_epoch,
            encrypted_blob_metadata: encryptedBlobMetadata,
            encrypted_attachment_metadata: encryptedAttachmentMetadata,
          },
          idempotency_key: crypto.randomUUID(),
        },
      })
      queued = true
      if (!state.online) {
        dispatch({
          type: 'set-notice',
          message:
            'Encrypted attachment staged offline and will upload after reconnection.',
        })
        return
      }
      const result = await flushCompletedAttachmentQueue(
        api,
        state.session.identity_id,
      )
      if (!result.uploaded.some((item) => item.id === attachmentId)) {
        throw new Error(
          'Encrypted attachment remains staged and will retry after reconnection',
        )
      }
      await refreshTaskAttachments(task.wire.id)
      dispatch({
        type: 'set-notice',
        message: 'Ciphertext persisted in OPFS and uploaded atomically.',
      })
    } catch (error) {
      if (!queued) {
        await removeEncryptedAttachment(blobId).catch(() => undefined)
      }
      dispatch({ type: 'set-error', message: errorMessage(error) })
      throw error
    }
  }

  const resumeAttachmentUpload = async (
    attachment: AttachmentCollectionItemDto,
  ) => {
    if (!attachment.task_id) return
    try {
      await api.uploadAttachmentCiphertext(
        attachment.project_id,
        attachment.blob_id,
        await readEncryptedAttachment(attachment.blob_id),
      )
      await api.finalizeAttachment(
        attachment.project_id,
        attachment.blob_id,
      )
      await refreshTaskAttachments(attachment.task_id)
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  useEffect(() => {
    if (
      !state.online ||
      !state.session ||
      typeof indexedDB === 'undefined'
    ) {
      return
    }
    let active = true
    void flushCompletedAttachmentQueue(api, state.session.identity_id).then(
      ({ uploaded, failed }) => {
      if (!active || uploaded.length === 0) return
      dispatch({
        type: 'set-notice',
        message:
          failed.length === 0
            ? `${uploaded.length} encrypted offline attachment${uploaded.length === 1 ? '' : 's'} synchronized.`
            : `${uploaded.length} encrypted offline attachment${uploaded.length === 1 ? '' : 's'} synchronized; ${failed.length} remain queued.`,
      })
      },
    )
    return () => {
      active = false
    }
  }, [api, dispatch, state.online, state.session])

  const downloadAttachment = async (
    value: AttachmentCollectionItemDto,
  ) => {
    let plaintext: Uint8Array | undefined
    try {
      const current = requireServices()
      const resourceKey = current.auth.vault.getResourceKey(
        value.resource_node_id,
        value.key_epoch,
      )
      if (!resourceKey) {
        throw new Error(
          'This authorized session has no attachment key on this device',
        )
      }
      const attachment = await api.getAttachment(
        value.project_id,
        value.blob_id,
      )
      const downloaded = asAttachmentCiphertext(
        await api.downloadCiphertext(
          `/v1/projects/${value.project_id}/files/${value.blob_id}/content`,
        ),
      )
      if (
        downloaded.size !== attachment.ciphertext_size ||
        (await attachmentCiphertextSha256(downloaded)) !==
          attachment.ciphertext_sha256
      ) {
        throw new Error('Downloaded attachment ciphertext failed integrity checks')
      }
      await writeEncryptedAttachment(value.blob_id, downloaded)
      const metadata = await decryptDocument<AttachmentDocument>(
        attachment.encrypted_metadata,
        {
          projectId: attachment.project_id,
          resourceId: attachment.resource_node_id,
          kind: 'attachment',
          aggregateVersion: 0,
          keyEpoch: attachment.key_epoch,
          resourceKey,
        },
      )
      plaintext = await decryptAttachment(downloaded, resourceKey, {
        projectId: attachment.project_id,
        resourceId: attachment.resource_node_id,
        blobId: attachment.blob_id,
        keyEpoch: attachment.key_epoch,
      })
      const method = await saveWithDownloadFallback(
        new Blob(
          [
            plaintext.buffer.slice(
              plaintext.byteOffset,
              plaintext.byteOffset + plaintext.byteLength,
            ) as ArrayBuffer,
          ],
          { type: 'application/octet-stream' },
        ),
        metadata.file_name,
      )
      dispatch({
        type: 'set-notice',
        message:
          method === 'file-picker'
            ? 'Decrypted file saved after an authorized-device key check.'
            : 'Safe attachment download started after local decryption.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    } finally {
      zeroBytes(plaintext)
    }
  }

  useEffect(() => {
    if (
      !services ||
      !state.session ||
      !state.selectedProjectId ||
      (state.screen !== 'tasks' && state.screen !== 'questionnaires')
    ) {
      return
    }
    void refreshQuestionnaires()
  }, [
    services,
    state.screen,
    state.selectedProjectId,
    state.session,
    refreshQuestionnaires,
  ])

  const refreshRetention = async () => {
    try {
      const [preference, archiveList, warningList] = await Promise.all([
        api.getRetentionPreference(),
        api.listRetentionArchives(),
        api.listRetentionWarnings(),
      ])
      setAutoExport(preference.preference.auto_export_enabled)
      setArchives(archiveList.archives)
      setRetentionWarnings(warningList.warnings)
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const downloadArchive = async (archive: RetentionArchiveDto) => {
    let bytes: Uint8Array | undefined
    let digest: Uint8Array | undefined
    try {
      const blob = await api.downloadCiphertext(
        `/v1/retention/archives/${archive.id}/download`,
      )
      bytes = new Uint8Array(await blob.arrayBuffer())
      digest = (await loadCrypto()).hash(bytes)
      await saveWithDownloadFallback(
        blob,
        `sprout-retention-${archive.id}.archive`,
      )
      await api.recordArchiveReceipt(archive.id, bytesToBase64(digest))
      dispatch({
        type: 'set-notice',
        message:
          'Encrypted archive download started and its ciphertext receipt was recorded.',
      })
      await refreshRetention()
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    } finally {
      zeroBytes(bytes, digest)
    }
  }

  const provisionSelectedProjectRecovery = async () => {
    if (!state.selectedProjectId || !state.session) return
    const project = state.projects.find(
      (item) => item.wire.id === state.selectedProjectId,
    )
    if (!project) return
    let escrowPlaintext: Uint8Array | undefined
    let secret: Uint8Array | undefined
    try {
      const current = requireServices()
      const [provision, packages] = await Promise.all([
        api.getProjectRecoveryProvision(state.selectedProjectId),
        api.listProjectDevicePackages(state.selectedProjectId),
      ])
      if (!provision.recoverable) {
        throw new Error(
          'This project has no eligible non-owner participants. Unanimous owner recovery is impossible until at least one participant device can hold a share.',
        )
      }
      const holderIdentityIds = [
        ...new Set(
          packages
            .map((item) => item.identity_id)
            .filter((identityId) => identityId !== project.wire.owner_identity_id),
        ),
      ]
      if (holderIdentityIds.length === 0) {
        throw new Error(
          'This project has no eligible non-owner participants. Unanimous owner recovery is impossible until at least one participant device can hold a share.',
        )
      }
      const { buildRecoveryProvisionBundle, encodeOwnerEscrowPlaintext } =
        await import('./domain/recovery')
      const rootKey = current.auth.vault.getResourceKey(
        project.wire.root_resource_id,
      )
      if (!rootKey) {
        throw new Error('Project root key is unavailable for recovery escrow')
      }
      escrowPlaintext = encodeOwnerEscrowPlaintext([
        {
          resourceId: project.wire.root_resource_id,
          epoch: 1,
          purpose: 'body',
          key: rootKey,
        },
      ])
      const bundle = await buildRecoveryProvisionBundle({
        projectId: state.selectedProjectId,
        recoveryEpoch: provision.recovery_epoch,
        membershipEpoch: provision.membership_epoch,
        ownerEscrowPlaintext: escrowPlaintext,
        holderIdentityIds,
        packages,
      })
      secret = bundle.secret
      try {
        await api.provisionProjectRecovery(
          state.selectedProjectId,
          bundle.request,
        )
        await api.activateProjectRecovery(
          state.selectedProjectId,
          bundle.request.recovery_set_id,
        )
      } finally {
        zeroBytes(secret)
        secret = undefined
      }
      dispatch({
        type: 'set-notice',
        message:
          'Recovery shares provisioned and activated for the current membership epoch. One unreachable participant makes recovery impossible.',
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    } finally {
      zeroBytes(escrowPlaintext, secret)
    }
  }

  const startProjectRecovery = async (
    kind: 'participant_device' | 'lost_owner',
  ) => {
    if (!state.selectedProjectId) return
    let challenge: Uint8Array | undefined
    let context: Uint8Array | undefined
    try {
      const current = requireServices()
      const requestId = crypto.randomUUID()
      challenge = crypto.getRandomValues(new Uint8Array(32))
      context = (await loadCrypto()).hash(
        new TextEncoder().encode(
          `sprout-recovery/${state.selectedProjectId}/${requestId}/${kind}`,
        ),
      )
      const status = await api.startProjectRecovery(
        state.selectedProjectId,
        {
          request_id: requestId,
          request_kind: kind,
          challenge_b64: bytesToBase64(challenge),
          context_hash_b64: bytesToBase64(context),
          expires_in_seconds: 3600,
          requester_device_key_version:
            current.auth.vault.deviceSecrets.keyVersion,
        },
      )
      dispatch({ type: 'set-recovery', status })
      dispatch({
        type: 'set-notice',
        message:
          'Recovery started against the active provisioned share set. Approvers must deliver rewrapped shares; finalize after unanimous approval.',
      })
    } catch (error) {
      const message = errorMessage(error)
      dispatch({
        type: 'set-error',
        message: /recovery_unprovisioned|unprovisioned/i.test(message)
          ? 'Owner recovery is unprovisioned. Provision n-of-n shares before starting recovery.'
          : message,
      })
    } finally {
      zeroBytes(challenge, context)
    }
  }

  const approveRecovery = async (input: {
    requestId: Uuid
    encryptedShareB64: string
    keyVersion: number
  }) => {
    if (!state.selectedProjectId) return
    let encryptedShare: Uint8Array | undefined
    let prefix: Uint8Array | undefined
    let shareHash: Uint8Array | undefined
    let message: Uint8Array | undefined
    let signatureContextBytes: Uint8Array | undefined
    let provisionContext: Uint8Array | undefined
    try {
      const current = requireServices()
      const status = state.recoveryStatus
      if (
        status?.request_id !== input.requestId ||
        !status.canonical_approval_prefix_b64
      ) {
        throw new Error(
          'Load the authenticated recovery status as an eligible approver first',
        )
      }
      if (input.encryptedShareB64.trim()) {
        encryptedShare = base64ToBytes(input.encryptedShareB64)
      } else {
        const { buildProvisionContext, unwrapShareEnvelope } = await import(
          './domain/recovery'
        )
        const mine = await api.listMyRecoveryShares(state.selectedProjectId)
        const held = mine.shares.find(
          (share) => share.recovery_set_id === status.recovery_set_id,
        )
        if (!held) {
          throw new Error(
            'No provisioned recovery share is available for this device',
          )
        }
        provisionContext = buildProvisionContext(
          state.selectedProjectId,
          held.recovery_epoch,
          held.membership_epoch,
        )
        const envelope = base64ToBytes(held.encrypted_share_b64)
        try {
          encryptedShare = await unwrapShareEnvelope(
            current.auth.vault,
            envelope,
            held.recovery_set_id,
            held.recovery_epoch,
            provisionContext,
          )
        } finally {
          zeroBytes(envelope)
        }
      }
      if (!encryptedShare) {
        throw new Error('Recovery share material is unavailable')
      }
      prefix = base64ToBytes(status.canonical_approval_prefix_b64)
      shareHash = (await loadCrypto()).hash(encryptedShare)
      message = new Uint8Array(prefix.length + shareHash.length)
      message.set(prefix)
      message.set(shareHash, prefix.length)
      signatureContextBytes = base64ToBytes(
        status.approval_signature_context_b64,
      )
      const signatureContext = new TextDecoder().decode(signatureContextBytes)
      const signatures = await signDual(
        current.auth.vault.deviceSecrets,
        message,
        signatureContext,
      )
      try {
        const status = await api.approveProjectRecovery(
          state.selectedProjectId,
          input.requestId,
          {
            approver_device_key_version: input.keyVersion,
            encrypted_share_b64: bytesToBase64(encryptedShare),
            classical_signature_b64: bytesToBase64(
              signatures.classicalSignature,
            ),
            post_quantum_signature_b64: bytesToBase64(
              signatures.postQuantumSignature,
            ),
          },
        )
        dispatch({ type: 'set-recovery', status })
      } finally {
        zeroBytes(
          signatures.classicalSignature,
          signatures.postQuantumSignature,
        )
      }
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    } finally {
      zeroBytes(
        encryptedShare,
        prefix,
        shareHash,
        message,
        signatureContextBytes,
        provisionContext,
      )
    }
  }

  const combineShares = async (values: string[]) => {
    const manualShares = values.map(base64ToBytes)
    let combined: Uint8Array | undefined
    let escrowPlaintext: Uint8Array | undefined
    let provisionContext: Uint8Array | undefined
    let replacementSecret: Uint8Array | undefined
    try {
      const current = requireServices()
      const status = state.recoveryStatus
      const project = state.projects.find(
        (item) => item.wire.id === state.selectedProjectId,
      )
      if (!state.selectedProjectId || !status || !project) {
        throw new Error('Load an authenticated recovery request first')
      }
      const required = status.required_approver_ids.length
      if (required === 0) {
        throw new Error('Recovery electorate is empty')
      }
      if (
        status.approved_approver_ids.length !== required ||
        !status.delivery_available
      ) {
        throw new Error('Finalize requires unanimous approvals first')
      }
      const deliveries = status.deliveries ?? []
      if (deliveries.length !== required && manualShares.length !== required) {
        throw new Error(`Exactly ${required} delivered shares are required`)
      }
      const shareBytes =
        deliveries.length === required
          ? deliveries.map((delivery) =>
              base64ToBytes(delivery.encrypted_share_b64),
            )
          : manualShares
      const {
        buildProvisionContext,
        buildRecoveryProvisionBundle,
        decodeOwnerEscrowPlaintext,
        encodeOwnerEscrowPlaintext,
        openOwnerEscrow,
      } = await import('./domain/recovery')
      provisionContext = buildProvisionContext(
        state.selectedProjectId,
        status.recovery_epoch,
        status.membership_epoch,
      )
      combined = await combineRecoverySecret(shareBytes, provisionContext)
      if (!status.encrypted_owner_key_escrow_b64) {
        throw new Error('Requester-only owner escrow was not delivered')
      }
      const escrow = base64ToBytes(status.encrypted_owner_key_escrow_b64)
      try {
        escrowPlaintext = await openOwnerEscrow(
          combined,
          escrow,
          state.selectedProjectId,
          status.recovery_epoch,
          status.membership_epoch,
        )
        const recoveredKeys = decodeOwnerEscrowPlaintext(escrowPlaintext)
        await Promise.all(
          recoveredKeys.map((entry) =>
            current.auth.vault.putResourceKey(
              entry.resourceId,
              entry.key,
              entry.epoch,
              entry.purpose,
            ),
          ),
        )
        for (const entry of recoveredKeys) zeroBytes(entry.key)
      } finally {
        zeroBytes(escrow)
      }
      const [packages, plan] = await Promise.all([
        api.listProjectDevicePackages(state.selectedProjectId),
        api.getProjectRecoveryRotationPlan(state.selectedProjectId),
      ])
      const holderIdentityIds = [
        ...new Set(
          packages
            .map((item) => item.identity_id)
            .filter(
              (identityId) => identityId !== project.wire.owner_identity_id,
            ),
        ),
      ]
      const builtRotations: Awaited<
        ReturnType<typeof buildResourceEpochRotation>
      >[] = []
      try {
        for (const resource of plan.resources) {
          const previousKeyCommitment = base64ToBytes(
            resource.previous_key_commitment_b64,
          )
          const previousHeaderKeyCommitment =
            resource.previous_header_key_commitment_b64 === null
              ? undefined
              : base64ToBytes(resource.previous_header_key_commitment_b64)
          try {
            builtRotations.push(
              await buildResourceEpochRotation(current.auth.vault, {
                projectId: state.selectedProjectId,
                resourceId: resource.resource_id,
                previousEpochId: resource.previous_epoch_id,
                currentEpoch: resource.current_epoch,
                previousKeyCommitment,
                previousHeaderKeyCommitment,
                recipientIdentityIds: resource.recipient_identity_ids,
                bodyRecipientIdentityIds: resource.body_recipient_identity_ids,
                headerRecipientIdentityIds:
                  resource.header_recipient_identity_ids,
                packages,
              }),
            )
          } finally {
            zeroBytes(previousKeyCommitment, previousHeaderKeyCommitment)
          }
        }
        const nextEscrow = encodeOwnerEscrowPlaintext(
          builtRotations.flatMap((item) => [
            {
              resourceId: item.rotation.resource_id,
              epoch: item.rotation.new_epoch,
              purpose: 'body' as const,
              key: item.resourceKey,
            },
            ...(item.headerKey
              ? [
                  {
                    resourceId: item.rotation.resource_id,
                    epoch: item.rotation.new_epoch,
                    purpose: 'header' as const,
                    key: item.headerKey,
                  },
                ]
              : []),
          ]),
        )
        const replacement = await buildRecoveryProvisionBundle({
          projectId: state.selectedProjectId,
          recoveryEpoch: status.recovery_epoch + 1,
          membershipEpoch: status.membership_epoch,
          ownerEscrowPlaintext: nextEscrow,
          holderIdentityIds,
          packages,
        })
        replacementSecret = replacement.secret
        zeroBytes(nextEscrow)
        try {
          const finalized = await api.finalizeProjectRecovery(
            state.selectedProjectId,
            status.request_id,
            {
              new_device_key_version:
                current.auth.vault.deviceSecrets.keyVersion,
              rotations: builtRotations.map((item) => item.rotation),
              replacement_recovery: replacement.request,
            },
          )
          await Promise.all(
            builtRotations.flatMap((item) => [
              current.auth.vault.putResourceKey(
                item.rotation.resource_id,
                item.resourceKey,
                item.rotation.new_epoch,
              ),
              ...(item.headerKey
                ? [
                    current.auth.vault.putResourceKey(
                      item.rotation.resource_id,
                      item.headerKey,
                      item.rotation.new_epoch,
                      'header',
                    ),
                  ]
                : []),
            ]),
          )
          dispatch({
            type: 'set-notice',
            message: `Recovery finalized (epoch ${finalized.recovery_epoch}). New resource keys are imported on this device.`,
          })
        } finally {
          zeroBytes(replacementSecret)
          replacementSecret = undefined
        }
      } finally {
        zeroBytes(
          ...builtRotations.flatMap((item) => [
            item.resourceKey,
            item.headerKey,
          ]),
        )
      }
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    } finally {
      for (const share of manualShares) zeroBytes(share)
      zeroBytes(combined, escrowPlaintext, provisionContext, replacementSecret)
    }
  }

  const discardConflict = async (conflict: SyncConflict) => {
    const current = requireServices()
    await current.database.removeConflict(conflict.id)
    dispatch({
      type: 'set-conflicts',
      conflicts: await current.database.listConflicts(
        state.selectedProjectId,
      ),
    })
  }

  const retryConflict = async (conflict: SyncConflict) => {
    if (
      !state.session ||
      conflict.remoteVersion === undefined ||
      !conflict.local.restMutation
    ) {
      return
    }
    try {
      const current = requireServices()
      const bytes = base64ToBytes(
        conflict.local.request.encrypted_payload_b64,
      )
      try {
        const payload = JSON.parse(
          new TextDecoder().decode(bytes),
        ) as EncryptedPayloadDto
        const body =
          typeof conflict.local.restMutation.body === 'object' &&
          conflict.local.restMutation.body !== null
            ? {
                ...conflict.local.restMutation.body,
                expected_payload_version: conflict.remoteVersion,
                idempotency_key: crypto.randomUUID(),
              }
            : conflict.local.restMutation.body
        await createSignedQueueItem(current.database, current.auth.vault, {
          projectId: conflict.projectId,
          resourceId: conflict.resourceId,
          identityId: state.session.identity_id,
          deviceId: state.session.device_id,
          deviceKeyVersion: current.auth.vault.deviceSecrets.keyVersion,
          baseVersion: conflict.remoteVersion,
          keyEpoch: conflict.local.request.key_epoch,
          eventKind: 'conflict.resolved',
          mutation: 'upsert',
          encryptedPayload: payload,
          restMutation: {
            ...conflict.local.restMutation,
            body,
          },
        })
      } finally {
        zeroBytes(bytes)
      }
      await discardConflict(conflict)
      dispatch({
        type: 'set-queue-count',
        count: await current.database.queueCount(),
      })
    } catch (error) {
      dispatch({ type: 'set-error', message: errorMessage(error) })
    }
  }

  const logout = () => {
    if (services) {
      services.wake.stop()
      services.sync.clearMemory()
      services.auth.logout()
    }
    clearDevSession()
    setPresetResult(undefined)
    setQuestionnaires([])
    setQuestionnaireVersions([])
    setSelectedQuestionnaireId(undefined)
    setTaskQuestionnaireVersion(undefined)
    setQuestionnaireSubmission(undefined)
    setAttachments([])
    setAttachmentLabels({})
    dispatch({ type: 'logout' })
  }

  const userLabel = useMemo(() => {
    if (!state.session) return 'Utente'
    const member = state.boardMembers.find(
      (item) => item.identityId === state.session?.identity_id,
    )
    return member?.label ?? `User ${state.session.identity_id.slice(0, 8)}`
  }, [state.boardMembers, state.session])

  const userMenuProps = {
    userLabel,
    projects: state.projects,
    selectedProjectId: state.selectedProjectId,
    currentScreen: state.screen,
    conflictCount: state.conflicts.length,
    projectName,
    onProjectNameChange: setProjectName,
    onSelectProject: (projectId: string) =>
      dispatch({ type: 'select-project', projectId }),
    onCreateProject: (event: FormEvent) => void createProject(event),
    onNavigate: (screen: AppScreen) => dispatch({ type: 'set-screen', screen }),
    onLogout: logout,
    appearance,
    onAppearanceChange: setAppearance,
  }

  if (!state.session) {
    return (
      <AuthScreen
        online={state.online}
        busy={state.phase === 'authenticating' || !services}
        error={state.error}
        notice={state.notice}
        deviceId={deviceId}
        offlineVaultAvailable={offlineVaultAvailable}
        onOfflineUnlock={unlockLocalVault}
        onSignIn={(input) =>
          runAuth(
            () =>
              requireServices().auth.authenticatePasskey({
                ...input,
                deviceId,
              }),
            'signin',
          )
        }
        onSignup={startSignup}
        onVerify={(input) =>
          runAuth(
            () =>
              requireServices().auth.finishSignup({
                ...input,
                deviceId,
              }),
            'verify',
            'Account attivato. Vai in Security e registra una passkey prima di uscire, così potrai rientrare con Accedi.',
          )
        }
        onRecoveryStart={startRecovery}
        onRecoveryFinish={(input) =>
          runAuth(
            () =>
              requireServices().auth.finishRecovery({
                ...input,
                deviceId,
              }),
            'recover',
          )
        }
        onDevLogin={import.meta.env.DEV ? devLogin : undefined}
      />
    )
  }

  return (
    <div className="app-shell">
      <main className="main-content">
        <div className="app-notifications" aria-live="polite">
        {state.phase === 'locked' && (
          <div className="app-banner warning-banner" role="alert">
            <KeyIcon />
            <div>
              <strong>Project keys unavailable</strong>
              <p>
                The passkey authenticated this session but did not reveal
                encryption keys. Use another authorized device or Recovery.
              </p>
              {import.meta.env.DEV && (
                <p>
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={resetLocalDeviceKeys}
                  >
                    DEV: reset local device keys
                  </button>
                </p>
              )}
            </div>
          </div>
        )}
        {lockedBoardReason && (
          <div className="app-banner warning-banner" role="alert">
            <KeyIcon />
            <div>
              <strong>Board locked: missing decryption keys</strong>
              <p>{lockedBoardReason}</p>
              <p>
                Sei loggato, ma questo browser non ha le resource key per
                decifrare topic/list/task. Il messaggio verde sopra è solo la
                coda recovery email, non sblocca i dati.
              </p>
              {import.meta.env.DEV && state.session && (
                <>
                  <p style={{ fontSize: '0.8rem', opacity: 0.85 }}>
                    DEV diag: vault{' '}
                    {services?.auth.vault.isUnlocked ? 'unlocked' : 'locked'},
                    backup keys=
                    {countDevResourceKeyBackup(state.session.identity_id)},
                    identity=
                    {services?.auth.vault.localIdentityId?.slice(0, 8) ??
                      'none'}
                    …, device session={state.session.device_id.slice(0, 8)}…
                    {services?.auth.vault.localDeviceId
                      ? `, vault=${services.auth.vault.localDeviceId.slice(0, 8)}…`
                      : ', vault=none'}
                  </p>
                  <p style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={() => void retryDevKeyRestore()}
                    >
                      DEV: retry key restore
                    </button>
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={resetLocalDeviceKeys}
                    >
                      DEV: reset device + re-login
                    </button>
                  </p>
                </>
              )}
            </div>
          </div>
        )}
        {state.error && (
          <div className="app-banner error-banner" role="alert">
            <strong>Request failed</strong>
            <span>{state.error}</span>
            <button
              type="button"
              onClick={() => dispatch({ type: 'set-error' })}
              aria-label="Dismiss error"
            >
              Dismiss
            </button>
          </div>
        )}
        {state.notice && (
          <div className="app-banner" role="status">
            <ShieldIcon />
            <span>{state.notice}</span>
            <button
              type="button"
              onClick={() => dispatch({ type: 'set-notice' })}
              aria-label="Dismiss notice"
            >
              Dismiss
            </button>
          </div>
        )}
        </div>

        {state.screen === 'tasks' && (
          <TasksScreen
            project={selectedProject}
            topics={state.topics}
            taskLists={state.taskLists}
            tasks={state.tasks}
            lockedTasks={state.lockedTasks}
            boardMembers={state.boardMembers}
            agents={agents}
            boardFocus={state.boardFocus}
            boardViewMode={state.boardViewMode}
            selectedTopicId={state.selectedTopicId}
            selectedListId={state.selectedListId}
            selectedTaskId={state.selectedTaskId}
            currentUserLabel={userLabel}
            publishedQuestionnaireVersions={publishedQuestionnaireVersions}
            filter={state.taskFilter}
            loading={state.loading}
            userMenu={userMenuProps}
            onSelectFocus={(focus: BoardFocus) =>
              dispatch({ type: 'set-board-focus', focus })
            }
            onBoardViewModeChange={(mode: BoardViewMode) =>
              dispatch({ type: 'set-board-view-mode', mode })
            }
            onSelectList={(listId) =>
              dispatch({ type: 'select-list', listId })
            }
            onSelectTask={(taskId) =>
              dispatch({ type: 'select-task', taskId })
            }
            onFilter={(filter) =>
              dispatch({ type: 'set-task-filter', filter })
            }
            onCreateTopic={createTopic}
            onRenameTopic={renameTopic}
            onToggleTopicFavorite={toggleTopicFavorite}
            onDeleteTopic={deleteTopic}
            onCreateList={createTaskList}
            onUpdateTaskList={updateTaskList}
            onLoadProjectInfo={loadProjectInfoDocuments}
            onCreateProjectInfoDocument={createProjectInfoDocument}
            onLoadTopicInfo={loadTopicInfoDocuments}
            onCreateTopicInfoDocument={createTopicInfoDocument}
            onLoadTaskListInfo={loadTaskListInfoDocuments}
            onCreateTaskListInfoDocument={createTaskListInfoDocument}
            onUpdateInfoDocument={updateInfoDocument}
            onUploadInfoDocumentFile={uploadInfoDocumentFile}
            onReadInfoDocumentFile={readInfoDocumentFile}
            onDownloadInfoDocumentFile={downloadInfoDocumentFile}
            onCreateTask={createTask}
            onUpdateTask={updateTask}
            onAssignTask={assignTask}
            onCompleteTask={completeTask}
            onCopyTask={copyTask}
            onInviteMember={inviteProjectParticipant}
            onProvisionAgent={provisionAgent}
            taskAttachments={attachments}
            taskAttachmentLabels={attachmentLabels}
            onRefreshTaskAttachments={refreshTaskAttachments}
            onDownloadTaskAttachment={downloadAttachment}
          />
        )}
        {state.screen !== 'tasks' && (
          <div className="settings-layout">
            <aside className="settings-sidebar" aria-label="Workspace">
              <WorkspaceUserMenu {...userMenuProps} variant="overview" />
            </aside>
            <div className="settings-main">
              <header className="settings-toolbar">
                <h1>{screenTitles[state.screen]}</h1>
              </header>
              {state.screen === 'people' && (
                <ProjectPeopleScreen
                  invitations={invitations}
                  suggestions={participantSuggestions}
                  onRefresh={refreshProjectPeople}
                  onInvite={inviteProjectParticipant}
                  onAccept={acceptProjectInvitation}
                  onShare={shareProjectWithParticipant}
                  managedGrants={managedResourceGrants}
                  onRevoke={revokeProjectResourceGrant}
                  onSuggest={suggestProjectParticipants}
                />
              )}
              {state.screen === 'presets' && (
                <PresetScreen
                  destinationReady={Boolean(
                    state.session &&
                      state.selectedProjectId &&
                      state.selectedListId,
                  )}
                  result={presetResult}
                  onMaterialize={materializePresetJourney}
                />
              )}
              {state.screen === 'questionnaires' && (
                <QuestionnaireScreen
                  questionnaires={questionnaires}
                  versions={selectedQuestionnaireVersions}
                  selectedQuestionnaireId={selectedQuestionnaireId}
                  assigneeTasks={questionnaireAssigneeTasks}
                  taskVersion={taskQuestionnaireVersion}
                  submission={questionnaireSubmission}
                  submissionAnswers={questionnaireSubmissionAnswers}
                  onRefresh={refreshQuestionnaires}
                  onCreate={createQuestionnaire}
                  onSelect={async (questionnaireId) =>
                    setSelectedQuestionnaireId(questionnaireId)
                  }
                  onSaveVersion={saveQuestionnaireVersion}
                  onPublish={publishQuestionnaireVersion}
                  onLoadTask={loadTaskQuestionnaire}
                  onSubmitTask={submitTaskQuestionnaire}
                />
              )}
              {state.screen === 'attachments' && (
                <AttachmentScreen
                  assigneeTasks={activeAssigneeTasks}
                  attachments={attachments}
                  onRefresh={refreshTaskAttachments}
                  onUpload={uploadCompletedAttachment}
                  onResume={resumeAttachmentUpload}
                  onDownload={downloadAttachment}
                />
              )}
              {state.screen === 'retention' && (
                <RetentionScreen
                  autoExport={autoExport}
                  archives={archives}
                  warnings={retentionWarnings}
                  onRefresh={refreshRetention}
                  onToggle={async (value) => {
                    const response = await api.updateRetentionPreference(value)
                    setAutoExport(response.preference.auto_export_enabled)
                  }}
                  onDownload={downloadArchive}
                />
              )}
              {state.screen === 'recovery' && (
                <RecoveryScreen
                  projectId={state.selectedProjectId}
                  status={state.recoveryStatus}
                  onProvision={provisionSelectedProjectRecovery}
                  onStart={startProjectRecovery}
                  onLoad={async (requestId) => {
                    if (!state.selectedProjectId) return
                    dispatch({
                      type: 'set-recovery',
                      status: await api.getProjectRecovery(
                        state.selectedProjectId,
                        requestId,
                      ),
                    })
                  }}
                  onApprove={approveRecovery}
                  onCombine={combineShares}
                />
              )}
              {state.screen === 'security' && (
                <SecurityScreen
                  vaultPersistence={state.vaultPersistence}
                  storagePersistence={state.storagePersistence}
                  onRegisterPasskey={async () => {
                    const current = requireServices()
                    const result = await current.auth.registerPasskey(
                      state.session?.device_id as Uuid,
                    )
                    dispatch({
                      type: 'set-vault-persistence',
                      value: current.auth.vault.persistence,
                    })
                    dispatch({
                      type: 'set-notice',
                      message: result.prfSupported
                        ? 'Passkey registered and the local vault was wrapped with PRF-derived input.'
                        : 'Passkey registered. PRF output was unavailable, so keys remain session-only.',
                    })
                  }}
                  onPersistStorage={async () =>
                    dispatch({
                      type: 'set-storage-persistence',
                      value: (await requestPersistentStorage())
                        ? 'granted'
                        : 'not-granted',
                    })
                  }
                />
              )}
              {state.screen === 'ai' && services && (
                <AiGenerationScreen vault={services.auth.vault} />
              )}
              {state.screen === 'conflicts' && (
                <ConflictScreen
                  conflicts={state.conflicts}
                  onDiscard={discardConflict}
                  onRetry={retryConflict}
                />
              )}
            </div>
            <button
              type="button"
              className="settings-report-issues"
              aria-label="Report issues"
            >
              <AlertTriangleIcon aria-hidden />
              <span>Report issues</span>
            </button>
          </div>
        )}
      </main>

      <footer className="mobile-status">
        <span>
          {state.online ? <ShieldIcon /> : <WifiOffIcon />}
          {state.online ? 'Encrypted session' : 'Offline queue active'}
        </span>
        <button
          type="button"
          onClick={() => dispatch({ type: 'set-screen', screen: 'retention' })}
        >
          <DownloadIcon />
          Exports
        </button>
      </footer>
    </div>
  )
}

export default App
