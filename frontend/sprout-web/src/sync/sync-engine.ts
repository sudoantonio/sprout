import { ApiClient, ApiError } from '../api/client'
import type {
  EncryptedPayloadDto,
  PushSyncRequest,
  SyncEventDto,
  SyncWakeNotification,
  Uuid,
} from '../api/contracts'
import type {
  EncryptedLocalRecord,
  LocalTombstone,
  ResourceKind,
  SignedQueueItem,
  SyncConflict,
} from '../domain/models'
import type { KeyVault } from '../security/key-vault'
import {
  base64ToBytes,
  bytesToBase64,
  loadCrypto,
  signDual,
  uuidToBytes,
  zeroBytes,
} from '../security/wasm'
import { EncryptedDatabase } from '../storage/encrypted-db'

const encoder = new TextEncoder()
const SYNC_SIGNATURE_CONTEXT = 'sprout-sync-event-v2'

/** Map a durable local projection into conflict remote fields (T-LLR-07.4). */
export const remoteFieldsFromRecord = (
  remote?: EncryptedLocalRecord,
): Pick<SyncConflict, 'remoteVersion' | 'remotePayloadB64'> => {
  if (!remote) return {}
  return {
    remoteVersion: remote.aggregateVersion,
    remotePayloadB64: bytesToBase64(
      encoder.encode(JSON.stringify(remote.payload)),
    ),
  }
}

/** True when a wake socket open should immediately REST catch-up (T-LLR-07.2). */
export const shouldCatchUpAfterWakeOpen = (reconnectAttempt: number): boolean =>
  reconnectAttempt > 0

export const hasExpectedPackageDigest = async (
  packageBytes: Uint8Array,
  expectedBase64: string,
): Promise<boolean> => {
  const digest = new Uint8Array(
    await crypto.subtle.digest(
      'SHA-256',
      packageBytes.buffer.slice(
        packageBytes.byteOffset,
        packageBytes.byteOffset + packageBytes.byteLength,
      ) as ArrayBuffer,
    ),
  )
  try {
    return bytesToBase64(digest) === expectedBase64
  } finally {
    zeroBytes(digest)
  }
}

const i32 = (value: number): Uint8Array => {
  const bytes = new Uint8Array(4)
  new DataView(bytes.buffer).setInt32(0, value)
  return bytes
}

const i64 = (value: number): Uint8Array => {
  const bytes = new Uint8Array(8)
  new DataView(bytes.buffer).setBigInt64(0, BigInt(value))
  return bytes
}

const u64 = (value: number): Uint8Array => {
  const bytes = new Uint8Array(8)
  new DataView(bytes.buffer).setBigUint64(0, BigInt(value))
  return bytes
}

const concatenate = (...parts: Uint8Array[]): Uint8Array => {
  const result = new Uint8Array(
    parts.reduce((length, part) => length + part.length, 0),
  )
  let offset = 0
  for (const part of parts) {
    result.set(part, offset)
    offset += part.length
  }
  return result
}

const eventHash = async (input: {
  projectId: Uuid
  resourceId: Uuid
  identityId: Uuid
  deviceId: Uuid
  keyVersion: number
  deviceSequence: number
  baseVersion: number
  aggregateVersion: number
  clientEventId: Uuid
  eventKind: string
  mutation: 'upsert' | 'tombstone'
  keyEpoch: number
  encryptedPayload: Uint8Array
  previousHash?: Uint8Array
}): Promise<Uint8Array> => {
  const module = await loadCrypto()
  const payloadHash = module.hash(input.encryptedPayload)
  const kind = encoder.encode(input.eventKind)
  const mutation = encoder.encode(input.mutation)
  const bytes = concatenate(
    encoder.encode(SYNC_SIGNATURE_CONTEXT),
    uuidToBytes(input.projectId),
    uuidToBytes(input.resourceId),
    uuidToBytes(input.identityId),
    uuidToBytes(input.deviceId),
    i32(input.keyVersion),
    i64(input.deviceSequence),
    i64(input.baseVersion),
    i64(input.aggregateVersion),
    uuidToBytes(input.clientEventId),
    u64(kind.length),
    kind,
    mutation,
    i32(input.keyEpoch),
    payloadHash,
    ...(input.previousHash ? [input.previousHash] : []),
  )
  try {
    return module.hash(bytes)
  } finally {
    zeroBytes(payloadHash, bytes, kind, mutation)
  }
}

export interface QueueEventInput {
  projectId: Uuid
  resourceId: Uuid
  identityId: Uuid
  deviceId: Uuid
  deviceKeyVersion: number
  baseVersion: number
  keyEpoch: number
  eventKind: string
  mutation: 'upsert' | 'tombstone'
  encryptedPayload: EncryptedPayloadDto
  restMutation?: SignedQueueItem['restMutation']
}

export const createSignedQueueItem = async (
  database: EncryptedDatabase,
  vault: KeyVault,
  input: QueueEventInput,
): Promise<SignedQueueItem> => {
  const metadata = await database.getSyncMetadata(input.projectId)
  const deviceSequence = metadata.deviceSequence + 1
  const clientEventId = crypto.randomUUID()
  const encryptedPayload = encoder.encode(
    JSON.stringify(input.encryptedPayload),
  )
  const previousHash = metadata.lastEventHashB64
    ? base64ToBytes(metadata.lastEventHashB64)
    : undefined
  const hash = await eventHash({
    projectId: input.projectId,
    resourceId: input.resourceId,
    identityId: input.identityId,
    deviceId: input.deviceId,
    keyVersion: input.deviceKeyVersion,
    deviceSequence,
    baseVersion: input.baseVersion,
    aggregateVersion: input.baseVersion + 1,
    clientEventId,
    eventKind: input.eventKind,
    mutation: input.mutation,
    keyEpoch: input.keyEpoch,
    encryptedPayload,
    previousHash,
  })
  const signatures = await signDual(
    vault.deviceSecrets,
    hash,
    SYNC_SIGNATURE_CONTEXT,
  )
  const request: PushSyncRequest = {
    project_id: input.projectId,
    resource_node_id: input.resourceId,
    base_version: input.baseVersion,
    aggregate_version: input.baseVersion + 1,
    actor_device_key_version: input.deviceKeyVersion,
    device_sequence: deviceSequence,
    client_event_id: clientEventId,
    event_kind: input.eventKind,
    mutation: input.mutation,
    key_epoch: input.keyEpoch,
    encrypted_payload_b64: bytesToBase64(encryptedPayload),
    previous_hash_b64: previousHash
      ? bytesToBase64(previousHash)
      : null,
    event_hash_b64: bytesToBase64(hash),
    classical_signature_b64: bytesToBase64(
      signatures.classicalSignature,
    ),
    post_quantum_signature_b64: bytesToBase64(
      signatures.postQuantumSignature,
    ),
    client_created_at: new Date().toISOString(),
    idempotency_key: crypto.randomUUID(),
  }
  const item: SignedQueueItem = {
    id: crypto.randomUUID(),
    request,
    restMutation: input.restMutation,
    queuedAt: new Date().toISOString(),
    attempts: 0,
  }
  await database.enqueue(item)
  await database.putSyncMetadata({
    ...metadata,
    deviceSequence,
    lastEventHashB64: request.event_hash_b64,
  })
  zeroBytes(
    encryptedPayload,
    previousHash,
    hash,
    signatures.classicalSignature,
    signatures.postQuantumSignature,
  )
  return item
}

const inferKind = (eventKind: string): ResourceKind => {
  if (eventKind.includes('task_list')) return 'task-list'
  if (eventKind.includes('questionnaire')) return 'questionnaire'
  if (eventKind.includes('attachment')) return 'attachment'
  if (eventKind.includes('preset')) return 'preset'
  if (eventKind.includes('topic')) return 'topic'
  if (eventKind.includes('project')) return 'project'
  return 'task'
}

export interface SyncSummary {
  uploaded: number
  downloaded: number
  pending: number
  conflicts: number
  offline: boolean
}

export const isStaleAfterTombstone = (
  event: Pick<SyncEventDto, 'mutation' | 'aggregate_version'>,
  tombstone?: Pick<LocalTombstone, 'aggregateVersion'>,
): boolean =>
  event.mutation === 'upsert' &&
  Boolean(
    tombstone &&
      event.aggregate_version <= tombstone.aggregateVersion,
  )

export class SyncEngine {
  readonly #database: EncryptedDatabase
  readonly #api: ApiClient
  readonly #signingKeys = new Map<
    string,
    { ed25519: Uint8Array; mlDsa65: Uint8Array }
  >()

  constructor(database: EncryptedDatabase, api: ApiClient) {
    this.#database = database
    this.#api = api
  }

  async flush(projectId?: Uuid, signal?: AbortSignal): Promise<SyncSummary> {
    const queue = await this.#database.listQueue(projectId)
    let uploaded = 0
    let conflicts = 0
    let offline = false
    for (const item of queue) {
      if (signal?.aborted) break
      try {
        const outcome = await this.#api.pushSync(item.request)
        if (
          outcome.projection.resource_node_id !==
            item.request.resource_node_id ||
          outcome.projection.aggregate_version !==
            item.request.aggregate_version
        ) {
          throw new Error('Sync projection did not atomically advance')
        }
        await this.#database.removeQueueItem(item.id)
        uploaded += 1
      } catch (error) {
        if (error instanceof ApiError && error.status === 409) {
          // Authoritative catch-up before recording the conflict so retry has
          // a concrete remote version/payload (T-LLR-07.4).
          try {
            await this.pull(item.request.project_id, signal)
          } catch {
            // Pull failure still records the conflict; remote fields may be empty.
          }
          const remote = await this.#database.getRecord(
            item.request.resource_node_id,
          )
          const conflict: SyncConflict = {
            id: crypto.randomUUID(),
            projectId: item.request.project_id,
            resourceId: item.request.resource_node_id,
            local: {
              ...item,
              attempts: item.attempts + 1,
              lastError: error.message,
            },
            ...remoteFieldsFromRecord(remote),
            reason: 'stale-version',
            createdAt: new Date().toISOString(),
          }
          await this.#database.putConflict(conflict)
          await this.#database.removeQueueItem(item.id)
          conflicts += 1
          continue
        }
        offline = true
        await this.#database.enqueue({
          ...item,
          attempts: item.attempts + 1,
          lastError:
            error instanceof Error ? error.message : 'Sync transport failed',
        })
        break
      }
    }
    return {
      uploaded,
      downloaded: 0,
      pending: await this.#database.queueCount(),
      conflicts,
      offline,
    }
  }

  clearMemory(): void {
    for (const keys of this.#signingKeys.values()) {
      zeroBytes(keys.ed25519, keys.mlDsa65)
    }
    this.#signingKeys.clear()
  }

  async pull(projectId: Uuid, signal?: AbortSignal): Promise<SyncSummary> {
    let metadata = await this.#database.getSyncMetadata(projectId)
    let downloaded = 0
    let conflicts = 0
    let hasMore = true

    while (hasMore && !signal?.aborted) {
      const response = await this.#api.pullSync(
        projectId,
        metadata.cursor,
        100,
      )
      for (const event of response.events) {
        if (!(await this.#verifyEvent(event))) {
          await this.#putIncomingConflict(event, 'chain-mismatch')
          conflicts += 1
          continue
        }
        const outcome = await this.#applyEvent(event)
        if (outcome === 'applied') downloaded += 1
        if (outcome === 'conflict') conflicts += 1
      }
      metadata = {
        ...metadata,
        cursor: response.next_sequence,
      }
      await this.#database.putSyncMetadata(metadata)
      hasMore = response.has_more
    }

    return {
      uploaded: 0,
      downloaded,
      pending: await this.#database.queueCount(),
      conflicts,
      offline: false,
    }
  }

  async #applyEvent(
    event: SyncEventDto,
  ): Promise<'applied' | 'ignored' | 'conflict'> {
    const tombstone = await this.#database.getTombstone(
      event.resource_node_id,
    )
    if (isStaleAfterTombstone(event, tombstone)) {
      const conflict: SyncConflict = {
        id: crypto.randomUUID(),
        projectId: event.project_id,
        resourceId: event.resource_node_id,
        local: {
          id: crypto.randomUUID(),
          request: {
            ...event,
            idempotency_key: crypto.randomUUID(),
            post_quantum_signature_b64:
              event.post_quantum_signature_b64 ?? '',
          },
          queuedAt: event.received_at,
          attempts: 0,
        },
        remotePayloadB64: event.encrypted_payload_b64,
        remoteVersion: event.aggregate_version,
        reason: 'stale-tombstone',
        createdAt: new Date().toISOString(),
      }
      await this.#database.putConflict(conflict)
      return 'conflict'
    }

    if (event.mutation === 'tombstone') {
      await this.#database.putTombstone({
        resourceId: event.resource_node_id,
        projectId: event.project_id,
        aggregateVersion: event.aggregate_version,
        eventSequence: event.event_sequence,
        recordedAt: event.received_at,
      })
      await this.#database.deleteRecord(event.resource_node_id)
      return 'applied'
    }

    const payloadBytes = base64ToBytes(event.encrypted_payload_b64)
    try {
      const payload = JSON.parse(
        new TextDecoder().decode(payloadBytes),
      ) as EncryptedPayloadDto
      const record: EncryptedLocalRecord = {
        id: event.resource_node_id,
        projectId: event.project_id,
        resourceId: event.resource_node_id,
        kind: inferKind(event.event_kind),
        aggregateVersion: event.aggregate_version,
        keyEpoch: event.key_epoch,
        payload,
        updatedAt: event.received_at,
      }
      await this.#database.putRecord(record)
      return 'applied'
    } catch {
      return 'ignored'
    } finally {
      zeroBytes(payloadBytes)
    }
  }

  async #verifyEvent(event: SyncEventDto): Promise<boolean> {
    const encryptedPayload = base64ToBytes(event.encrypted_payload_b64)
    const previousHash = event.previous_hash_b64
      ? base64ToBytes(event.previous_hash_b64)
      : undefined
    const suppliedHash = base64ToBytes(event.event_hash_b64)
    const classical = base64ToBytes(event.classical_signature_b64)
    const postQuantum = event.post_quantum_signature_b64
      ? base64ToBytes(event.post_quantum_signature_b64)
      : undefined
    let expectedHash: Uint8Array | undefined
    try {
      if (!postQuantum) return false
      expectedHash = await eventHash({
        projectId: event.project_id,
        resourceId: event.resource_node_id,
        identityId: event.actor_identity_id,
        deviceId: event.actor_device_id,
        keyVersion: event.actor_device_key_version,
        deviceSequence: event.device_sequence,
        baseVersion: event.base_version,
        aggregateVersion: event.aggregate_version,
        clientEventId: event.client_event_id,
        eventKind: event.event_kind,
        mutation: event.mutation,
        keyEpoch: event.key_epoch,
        encryptedPayload,
        previousHash,
      })
      if (
        expectedHash.length !== suppliedHash.length ||
        !expectedHash.every((byte, index) => byte === suppliedHash[index])
      ) {
        return false
      }
      const keys = await this.#keysFor(event)
      if (!keys) return false
      const module = await loadCrypto()
      const context = encoder.encode(SYNC_SIGNATURE_CONTEXT)
      try {
        return module.verifyDual(
          keys.ed25519,
          keys.mlDsa65,
          suppliedHash,
          context,
          classical,
          postQuantum,
        )
      } finally {
        zeroBytes(context)
      }
    } finally {
      zeroBytes(
        encryptedPayload,
        previousHash,
        suppliedHash,
        classical,
        postQuantum,
        expectedHash,
      )
    }
  }

  async #keysFor(
    event: SyncEventDto,
  ): Promise<{ ed25519: Uint8Array; mlDsa65: Uint8Array } | undefined> {
    const cacheKey = `${event.project_id}:${event.actor_device_id}:${event.actor_device_key_version}`
    const cached = this.#signingKeys.get(cacheKey)
    if (cached) return cached

    const packages = await this.#api.listProjectDevicePackages(
      event.project_id,
    )
    const match = packages.find(
      (item) =>
        item.device_id === event.actor_device_id &&
        item.key_version === event.actor_device_key_version,
    )
    if (!match) return undefined
    const bytes = base64ToBytes(match.package_b64)
    try {
      if (!(await hasExpectedPackageDigest(bytes, match.package_hash_b64))) {
        return undefined
      }
      const parsed = JSON.parse(new TextDecoder().decode(bytes)) as {
        signing_keys?: Array<{
          algorithm: string
          public_key: number[]
        }>
      }
      const ed25519 = parsed.signing_keys?.find(
        (key) => key.algorithm === 'ed25519',
      )
      const mlDsa65 = parsed.signing_keys?.find(
        (key) => key.algorithm === 'ml_dsa65_experimental',
      )
      if (!ed25519 || !mlDsa65) return undefined
      const keys = {
        ed25519: Uint8Array.from(ed25519.public_key),
        mlDsa65: Uint8Array.from(mlDsa65.public_key),
      }
      this.#signingKeys.set(cacheKey, keys)
      return keys
    } finally {
      zeroBytes(bytes)
    }
  }

  async #putIncomingConflict(
    event: SyncEventDto,
    reason: SyncConflict['reason'],
  ): Promise<void> {
    await this.#database.putConflict({
      id: crypto.randomUUID(),
      projectId: event.project_id,
      resourceId: event.resource_node_id,
      local: {
        id: crypto.randomUUID(),
        request: {
          project_id: event.project_id,
          resource_node_id: event.resource_node_id,
          base_version: event.base_version,
          aggregate_version: event.aggregate_version,
          actor_device_key_version: event.actor_device_key_version,
          device_sequence: event.device_sequence,
          client_event_id: event.client_event_id,
          event_kind: event.event_kind,
          mutation: event.mutation,
          key_epoch: event.key_epoch,
          encrypted_payload_b64: event.encrypted_payload_b64,
          previous_hash_b64: event.previous_hash_b64,
          event_hash_b64: event.event_hash_b64,
          classical_signature_b64: event.classical_signature_b64,
          post_quantum_signature_b64:
            event.post_quantum_signature_b64 ?? '',
          client_created_at: event.client_created_at,
          idempotency_key: crypto.randomUUID(),
        },
        queuedAt: event.received_at,
        attempts: 0,
      },
      remotePayloadB64: event.encrypted_payload_b64,
      remoteVersion: event.aggregate_version,
      reason,
      createdAt: new Date().toISOString(),
    })
  }
}

export type WakeStatus = 'connecting' | 'connected' | 'disconnected'

export class SyncWakeClient {
  readonly #api: ApiClient
  #socket?: WebSocket
  #timer?: number
  #attempt = 0
  #stopped = true

  constructor(api: ApiClient) {
    this.#api = api
  }

  start(
    projectId: Uuid,
    onWake: (notification: SyncWakeNotification) => void,
    onStatus: (status: WakeStatus) => void,
  ): void {
    this.stop()
    this.#stopped = false
    const connect = () => {
      if (this.#stopped || !navigator.onLine) {
        onStatus('disconnected')
        return
      }
      onStatus('connecting')
      const socket = this.#api.openSyncWake(projectId)
      this.#socket = socket
      socket.addEventListener('open', () => {
        const reconnectAttempt = this.#attempt
        this.#attempt = 0
        onStatus('connected')
        // T-LLR-07.2: after wake loss, reconnect must REST catch-up immediately.
        if (shouldCatchUpAfterWakeOpen(reconnectAttempt)) {
          onWake({ project_id: projectId, cursor: 0 })
        }
      })
      socket.addEventListener('message', (event) => {
        try {
          const wake = JSON.parse(String(event.data)) as SyncWakeNotification
          if (wake.project_id === projectId) onWake(wake)
        } catch {
          socket.close(1003, 'invalid wake payload')
        }
      })
      socket.addEventListener('close', () => {
        if (this.#stopped) return
        onStatus('disconnected')
        const delay = Math.min(30_000, 1_000 * 2 ** this.#attempt++)
        this.#timer = window.setTimeout(connect, delay)
      })
      socket.addEventListener('error', () => socket.close())
    }
    connect()
  }

  stop(): void {
    this.#stopped = true
    if (this.#timer) window.clearTimeout(this.#timer)
    this.#socket?.close(1000, 'client stopped')
    this.#socket = undefined
    this.#timer = undefined
  }
}
