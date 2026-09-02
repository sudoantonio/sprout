import type {
  EncryptedPayloadDto,
  InfoDocumentDto,
  PresetDto,
  ProjectView,
  TaskDto,
  TaskListDto,
  TopicDto,
  Uuid,
} from '../api/contracts'
import type { KeyVault } from '../security/key-vault'
import { recoverDevResourceKeyFromBackup } from '../security/dev-resource-keys'
import {
  base64ToBytes,
  bytesToBase64,
  decryptDocument,
  encryptDocument,
  zeroBytes,
} from '../security/wasm'
import type {
  DecryptedTask,
  DecryptedInfoDocument,
  DecryptedPreset,
  InfoDocumentContent,
  ProjectDocument,
  PresetDocument,
  ResourceKind,
  TaskDocument,
  TaskListDocument,
  TopicDocument,
} from './models'

const decoder = new TextDecoder()
const encoder = new TextEncoder()
export const INITIAL_PAYLOAD_VERSION = 1

export const encodePayloadContainer = (
  payload: EncryptedPayloadDto,
): string => bytesToBase64(encoder.encode(JSON.stringify(payload)))

export const decodePayloadContainer = (
  value: string,
): EncryptedPayloadDto => {
  const bytes = base64ToBytes(value)
  try {
    return JSON.parse(decoder.decode(bytes)) as EncryptedPayloadDto
  } finally {
    zeroBytes(bytes)
  }
}

/**
 * Prefer wire epoch; fall back to any restored/backed-up body epoch.
 * Mirrors resolveHierarchyHeaderKey for body (payload) keys.
 */
export const resolveActiveResourceKey = (
  vault: {
    getResourceKey: (resourceId: Uuid, epoch?: number) => Uint8Array | undefined
    getLatestResourceKey: (
      resourceId: Uuid,
    ) => { epoch: number; key: Uint8Array } | undefined
  },
  resourceId: Uuid,
  preferredEpoch: number,
): { epoch: number; key: Uint8Array } | undefined => {
  const exact = vault.getResourceKey(resourceId, preferredEpoch)
  if (exact) return { epoch: preferredEpoch, key: exact }
  const latest = vault.getLatestResourceKey(resourceId)
  if (latest) return latest
  if (preferredEpoch !== 1) {
    const genesis = vault.getResourceKey(resourceId, 1)
    if (genesis) return { epoch: 1, key: genesis }
  }
  return undefined
}

/**
 * Project metadata and the project's root resource are created from the same
 * body key. Keep both vault aliases available so project Info documents never
 * fall back to a newly minted, incompatible root key after a reload.
 */
export const synchronizeProjectRootKey = async (
  vault: KeyVault,
  project: ProjectView,
): Promise<boolean> => {
  const rootKey = vault.getResourceKey(
    project.root_resource_id,
    project.key_epoch,
  )
  if (rootKey) {
    try {
      if (!vault.getResourceKey(project.id, project.key_epoch)) {
        await vault.putResourceKey(
          project.id,
          rootKey,
          project.key_epoch,
          'body',
        )
      }
      return true
    } finally {
      zeroBytes(rootKey)
    }
  }

  const projectKey = vault.getResourceKey(project.id, project.key_epoch)
  if (!projectKey) return false
  try {
    await vault.putResourceKey(
      project.root_resource_id,
      projectKey,
      project.key_epoch,
      'body',
    )
    return true
  } finally {
    zeroBytes(projectKey)
  }
}

const requireKey = (
  vault: KeyVault,
  resourceId: Uuid,
  keyEpoch: number,
): Uint8Array => {
  // Exact epoch only: keyEpoch is bound into ciphertext AAD. Callers must
  // resolve via resolveActiveResourceKey and pass the matching epoch.
  const key = vault.getResourceKey(resourceId, keyEpoch)
  if (!key) {
    throw new Error('This resource key is not available on this device')
  }
  return key
}

/**
 * DEV recovery for ciphertext encrypted with an all-zero key when
 * createEncryptedResource used to `return encryptDocument(...)` without await,
 * so `finally { zeroBytes(key) }` ran mid-encrypt.
 */
const decryptWithDevZeroKeyFallback = async <T>(
  ciphertext: EncryptedPayloadDto,
  options: {
    projectId: Uuid
    resourceId: Uuid
    kind: ResourceKind
    aggregateVersion: number
    keyEpoch: number
    resourceKey?: Uint8Array
  },
  primaryError?: unknown,
): Promise<T> => {
  if (!import.meta.env.DEV) {
    if (primaryError instanceof Error) throw primaryError
    throw new Error('This resource key is not available on this device')
  }
  const zeroKey = new Uint8Array(32)
  try {
    return await decryptDocument<T>(ciphertext, {
      ...options,
      resourceKey: zeroKey,
    })
  } catch {
    if (primaryError instanceof Error) throw primaryError
    throw new Error('This resource key is not available on this device')
  } finally {
    zeroBytes(zeroKey)
  }
}

const decryptBodyOrHeader = async <T>(
  vault: KeyVault,
  input: {
    payload: EncryptedPayloadDto | null
    header: EncryptedPayloadDto | null
    projectId: Uuid
    resourceId: Uuid
    kind: ResourceKind
    aggregateVersion: number
    keyEpoch: number
  },
): Promise<T> => {
  if (!input.payload && !input.header) {
    throw new Error('No encrypted resource content was returned')
  }

  const attempt = async (
    ciphertext: EncryptedPayloadDto,
    purpose: 'body' | 'header',
  ): Promise<T> => {
    const resolved =
      purpose === 'body'
        ? resolveActiveResourceKey(vault, input.resourceId, input.keyEpoch)
        : (() => {
            const exact = vault.getHeaderKey(input.resourceId, input.keyEpoch)
            if (exact) return { epoch: input.keyEpoch, key: exact }
            const latest = vault.getLatestHeaderKey(input.resourceId)
            if (latest) return latest
            if (input.keyEpoch !== 1) {
              const genesis = vault.getHeaderKey(input.resourceId, 1)
              if (genesis) return { epoch: 1, key: genesis }
            }
            return undefined
          })()
    const options = {
      projectId: input.projectId,
      resourceId: input.resourceId,
      kind: input.kind,
      aggregateVersion: input.aggregateVersion,
      keyEpoch: resolved?.epoch ?? input.keyEpoch,
    }
    let primaryError: unknown
    if (resolved) {
      try {
        return await decryptDocument<T>(ciphertext, {
          ...options,
          resourceKey: resolved.key,
        })
      } catch (error) {
        primaryError = error
      }
    }
    const recovered = await recoverDevResourceKeyFromBackup<T>(vault, {
      ciphertext,
      projectId: input.projectId,
      resourceId: input.resourceId,
      kind: input.kind,
      aggregateVersion: input.aggregateVersion,
      keyEpoch: input.keyEpoch,
      purpose,
    })
    if (recovered !== undefined) return recovered
    try {
      return await decryptWithDevZeroKeyFallback<T>(ciphertext, {
        ...options,
        keyEpoch: input.keyEpoch,
      }, primaryError)
    } catch (error) {
      if (primaryError instanceof Error) throw primaryError
      throw error
    }
  }

  let bodyError: unknown
  if (input.payload) {
    try {
      return await attempt(input.payload, 'body')
    } catch (error) {
      bodyError = error
    }
  }
  if (input.header) {
    try {
      return await attempt(input.header, 'header')
    } catch (error) {
      if (!bodyError) bodyError = error
    }
  }
  if (bodyError instanceof Error) throw bodyError
  throw new Error('This resource key is not available on this device')
}

export const decryptProject = async (
  project: ProjectView,
  vault: KeyVault,
): Promise<ProjectDocument> => {
  const ciphertext = decodePayloadContainer(project.encrypted_metadata_b64)
  const options = {
    projectId: project.id,
    resourceId: project.id,
    kind: 'project' as const,
    aggregateVersion: 0,
    keyEpoch: project.key_epoch,
  }
  const key = vault.getResourceKey(project.id, project.key_epoch)
  let primaryError: unknown
  if (key) {
    try {
      return await decryptDocument<ProjectDocument>(ciphertext, {
        ...options,
        resourceKey: key,
      })
    } catch (error) {
      primaryError = error
    }
  }
  const recovered = await recoverDevResourceKeyFromBackup<ProjectDocument>(
    vault,
    {
      ciphertext,
      projectId: project.id,
      resourceId: project.id,
      kind: 'project',
      aggregateVersion: 0,
      keyEpoch: project.key_epoch,
      purpose: 'body',
    },
  )
  if (recovered !== undefined) return recovered
  return decryptWithDevZeroKeyFallback<ProjectDocument>(
    ciphertext,
    options,
    primaryError,
  )
}

export const decryptTopic = (
  topic: TopicDto,
  vault: KeyVault,
): Promise<TopicDocument> =>
  decryptBodyOrHeader<TopicDocument>(vault, {
    payload: topic.payload,
    header: topic.header ?? null,
    projectId: topic.project_id,
    resourceId: topic.resource_node_id,
    kind: 'topic',
    aggregateVersion: topic.payload_version,
    keyEpoch: topic.key_epoch,
  })

export const decryptTaskList = (
  list: TaskListDto,
  vault: KeyVault,
): Promise<TaskListDocument> =>
  decryptBodyOrHeader<TaskListDocument>(vault, {
    payload: list.payload,
    header: list.header ?? null,
    projectId: list.project_id,
    resourceId: list.resource_node_id,
    kind: 'task-list',
    aggregateVersion: list.payload_version,
    keyEpoch: list.key_epoch,
  })

export const decryptTask = async (
  task: TaskDto,
  vault: KeyVault,
): Promise<DecryptedTask> => ({
  wire: task,
  document: await decryptBodyOrHeader<TaskDocument>(vault, {
    payload: task.payload,
    header: task.header ?? null,
    projectId: task.project_id,
    resourceId: task.resource_node_id,
    kind: 'task',
    aggregateVersion: task.payload_version,
    keyEpoch: task.key_epoch,
  }),
})

export const decryptPreset = async (
  preset: PresetDto,
  vault: KeyVault,
): Promise<DecryptedPreset> => {
  const resolved = resolveActiveResourceKey(vault, preset.id, 1)
  if (!resolved) {
    throw new Error('This preset key is not available on this device')
  }
  return {
    wire: preset,
    document: await decryptDocument<PresetDocument>(preset.payload, {
      projectId: preset.project_id,
      resourceId: preset.id,
      kind: 'preset',
      aggregateVersion: 0,
      keyEpoch: resolved.epoch,
      resourceKey: resolved.key,
    }),
  }
}

export const decryptInfoDocument = async (
  document: InfoDocumentDto,
  vault: KeyVault,
): Promise<DecryptedInfoDocument> => {
  const resourceKey = requireKey(
    vault,
    document.resource_node_id,
    document.key_epoch,
  )
  return {
    wire: document,
    document: await decryptDocument<InfoDocumentContent>(document.payload, {
      projectId: document.project_id,
      resourceId: document.id,
      kind: document.task_list_id
        ? 'task-list'
        : document.topic_id
          ? 'topic'
          : 'project',
      aggregateVersion: document.payload_version,
      keyEpoch: document.key_epoch,
      resourceKey,
    }),
  }
}

export const encryptInfoDocument = (
  vault: KeyVault,
  input: {
    projectId: Uuid
    documentId: Uuid
    containerResourceId: Uuid
    aggregateVersion: number
    keyEpoch: number
    kind: 'project' | 'task-list' | 'topic'
    document: InfoDocumentContent
  },
): Promise<EncryptedPayloadDto> =>
  encryptDocument(input.document, {
    projectId: input.projectId,
    resourceId: input.documentId,
    keyId: crypto.randomUUID(),
    kind: input.kind,
    aggregateVersion: input.aggregateVersion,
    keyEpoch: input.keyEpoch,
    resourceKey: requireKey(
      vault,
      input.containerResourceId,
      input.keyEpoch,
    ),
  })

export const createEncryptedResource = async <T>(
  vault: KeyVault,
  input: {
    projectId: Uuid
    resourceId: Uuid
    kind: ResourceKind
    aggregateVersion: number
    document: T
  },
): Promise<EncryptedPayloadDto> => {
  const key = crypto.getRandomValues(new Uint8Array(32))
  // Encrypt with a dedicated copy so finally/zeroBytes cannot touch the vault
  // slot (putResourceKey also stores its own copy).
  const encryptKey = key.slice()
  try {
    await vault.putResourceKey(input.resourceId, key)
    // Must await: a bare `return encryptDocument(...)` runs finally/zeroBytes
    // before encryption finishes, producing all-zero-key ciphertext.
    return await encryptDocument(input.document, {
      projectId: input.projectId,
      resourceId: input.resourceId,
      keyId: crypto.randomUUID(),
      kind: input.kind,
      aggregateVersion: input.aggregateVersion,
      keyEpoch: 1,
      resourceKey: encryptKey,
    })
  } finally {
    zeroBytes(key, encryptKey)
  }
}

export const createEncryptedResourceHeader = async <T>(
  vault: KeyVault,
  input: {
    projectId: Uuid
    resourceId: Uuid
    kind: ResourceKind
    aggregateVersion: number
    document: T
  },
): Promise<EncryptedPayloadDto> => {
  const key = crypto.getRandomValues(new Uint8Array(32))
  // Encrypt with a dedicated copy so finally/zeroBytes cannot touch the vault
  // slot (putResourceKey also stores its own copy).
  const encryptKey = key.slice()
  try {
    await vault.putResourceKey(input.resourceId, key, 1, 'header')
    // Must await: same zeroBytes race as createEncryptedResource (header
    // keys are required later for hierarchy task assignment).
    return await encryptDocument(input.document, {
      projectId: input.projectId,
      resourceId: input.resourceId,
      keyId: crypto.randomUUID(),
      kind: input.kind,
      aggregateVersion: input.aggregateVersion,
      keyEpoch: 1,
      resourceKey: encryptKey,
    })
  } finally {
    zeroBytes(key, encryptKey)
  }
}

export const encryptExistingResource = <T>(
  vault: KeyVault,
  input: {
    projectId: Uuid
    resourceId: Uuid
    kind: ResourceKind
    aggregateVersion: number
    keyEpoch?: number
    document: T
  },
): Promise<EncryptedPayloadDto> =>
  encryptDocument(input.document, {
    projectId: input.projectId,
    resourceId: input.resourceId,
    keyId: crypto.randomUUID(),
    kind: input.kind,
    aggregateVersion: input.aggregateVersion,
    keyEpoch: input.keyEpoch ?? 1,
    resourceKey: requireKey(
      vault,
      input.resourceId,
      input.keyEpoch ?? 1,
    ),
  })
