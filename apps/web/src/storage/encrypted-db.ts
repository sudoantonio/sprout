import type { Uuid } from '../api/contracts'
import type {
  EncryptedLocalRecord,
  LocalTombstone,
  SignedQueueItem,
  SyncConflict,
} from '../domain/models'

const DATABASE_NAME = 'sprout-encrypted-workspace'
const DATABASE_VERSION = 2
const RECORD_STORE = 'encrypted-records'
const QUEUE_STORE = 'encrypted-sync-queue'
const SYNC_STORE = 'sync-metadata'
const VAULT_STORE = 'encrypted-key-vault'
const TOMBSTONE_STORE = 'sync-tombstones'
const CONFLICT_STORE = 'encrypted-conflicts'

export interface VaultCipherRecord {
  id: string
  deviceId: Uuid
  credentialId: string
  saltB64: string
  nonceB64: string
  ciphertextB64: string
  createdAt: string
}

export interface SyncMetadata {
  projectId: Uuid
  cursor: number
  deviceSequence: number
  lastEventHashB64?: string
}

const requestResult = <T>(request: IDBRequest<T>): Promise<T> =>
  new Promise((resolve, reject) => {
    request.addEventListener('success', () => resolve(request.result))
    request.addEventListener('error', () =>
      reject(request.error ?? new Error('IndexedDB request failed')),
    )
  })

const transactionComplete = (transaction: IDBTransaction): Promise<void> =>
  new Promise((resolve, reject) => {
    transaction.addEventListener('complete', () => resolve())
    transaction.addEventListener('abort', () =>
      reject(transaction.error ?? new Error('IndexedDB transaction aborted')),
    )
    transaction.addEventListener('error', () =>
      reject(transaction.error ?? new Error('IndexedDB transaction failed')),
    )
  })

const isNonEmptyString = (value: unknown): value is string =>
  typeof value === 'string' && value.length > 0

export const isRecoverableSignedQueueItem = (
  value: unknown,
): value is SignedQueueItem => {
  if (!value || typeof value !== 'object') return false
  const item = value as Partial<SignedQueueItem>
  const request = item.request as
    | Partial<SignedQueueItem['request']>
    | undefined
  return (
    isNonEmptyString(item.id) &&
    isNonEmptyString(item.queuedAt) &&
    Number.isSafeInteger(item.attempts) &&
    item.attempts! >= 0 &&
    Boolean(request) &&
    isNonEmptyString(request?.project_id) &&
    isNonEmptyString(request?.resource_node_id) &&
    isNonEmptyString(request?.client_event_id) &&
    isNonEmptyString(request?.idempotency_key) &&
    isNonEmptyString(request?.event_kind) &&
    (request?.mutation === 'upsert' || request?.mutation === 'tombstone') &&
    isNonEmptyString(request?.encrypted_payload_b64) &&
    isNonEmptyString(request?.event_hash_b64) &&
    isNonEmptyString(request?.classical_signature_b64) &&
    isNonEmptyString(request?.post_quantum_signature_b64) &&
    Number.isSafeInteger(request?.device_sequence) &&
    Number.isSafeInteger(request?.aggregate_version) &&
    Number.isSafeInteger(request?.key_epoch)
  )
}

const createEncryptedStores = (
  database: IDBDatabase,
  recoveredQueue: SignedQueueItem[] = [],
): void => {
  const records = database.createObjectStore(RECORD_STORE, {
    keyPath: 'id',
  })
  records.createIndex('projectId', 'projectId', { unique: false })
  records.createIndex('kind', 'kind', { unique: false })
  records.createIndex('updatedAt', 'updatedAt', { unique: false })
  const queue = database.createObjectStore(QUEUE_STORE, { keyPath: 'id' })
  queue.createIndex('queuedAt', 'queuedAt', { unique: false })
  queue.createIndex('projectId', 'request.project_id', { unique: false })
  for (const item of recoveredQueue) queue.put(item)
  database.createObjectStore(SYNC_STORE, { keyPath: 'projectId' })
  database.createObjectStore(VAULT_STORE, { keyPath: 'id' })
  const tombstones = database.createObjectStore(TOMBSTONE_STORE, {
    keyPath: 'resourceId',
  })
  tombstones.createIndex('projectId', 'projectId', { unique: false })
  const conflicts = database.createObjectStore(CONFLICT_STORE, {
    keyPath: 'id',
  })
  conflicts.createIndex('projectId', 'projectId', { unique: false })
}

const openDatabase = (): Promise<IDBDatabase> =>
  new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION)

    request.addEventListener('upgradeneeded', (event) => {
      const database = request.result
      if (event.oldVersion < 2) {
        const rebuild = (legacyItems: unknown[]) => {
          // ADR-0005: v1 data has no current integrity guarantee. Preserve only
          // complete signed queue operations; all other legacy stores and
          // malformed/unsigned entries are intentionally destroyed.
          const recoveredQueue = legacyItems.filter(
            isRecoverableSignedQueueItem,
          )
          for (const storeName of Array.from(database.objectStoreNames)) {
            database.deleteObjectStore(storeName)
          }
          createEncryptedStores(database, recoveredQueue)
        }
        if (database.objectStoreNames.contains(QUEUE_STORE)) {
          const legacyQueue = request.transaction!
            .objectStore(QUEUE_STORE)
            .getAll()
          legacyQueue.addEventListener('success', () =>
            rebuild(legacyQueue.result),
          )
          legacyQueue.addEventListener('error', () => rebuild([]))
        } else {
          rebuild([])
        }
        return
      }
    })

    request.addEventListener('success', () => resolve(request.result))
    request.addEventListener('error', () =>
      reject(request.error ?? new Error('Unable to open encrypted storage')),
    )
  })

export class EncryptedDatabase {
  readonly #database: IDBDatabase

  private constructor(database: IDBDatabase) {
    this.#database = database
  }

  static async open(): Promise<EncryptedDatabase> {
    if (!('indexedDB' in globalThis)) {
      throw new Error('Encrypted offline storage is unavailable')
    }
    return new EncryptedDatabase(await openDatabase())
  }

  close(): void {
    this.#database.close()
  }

  async putRecord(record: EncryptedLocalRecord): Promise<void> {
    const transaction = this.#database.transaction(RECORD_STORE, 'readwrite')
    transaction.objectStore(RECORD_STORE).put(record)
    await transactionComplete(transaction)
  }

  async getRecord(id: Uuid): Promise<EncryptedLocalRecord | undefined> {
    const transaction = this.#database.transaction(RECORD_STORE, 'readonly')
    const result = await requestResult<EncryptedLocalRecord | undefined>(
      transaction.objectStore(RECORD_STORE).get(id),
    )
    await transactionComplete(transaction)
    return result
  }

  async listRecords(projectId?: Uuid): Promise<EncryptedLocalRecord[]> {
    const transaction = this.#database.transaction(RECORD_STORE, 'readonly')
    const store = transaction.objectStore(RECORD_STORE)
    const request = projectId
      ? store.index('projectId').getAll(projectId)
      : store.getAll()
    const result = await requestResult<EncryptedLocalRecord[]>(
      request,
    )
    await transactionComplete(transaction)
    return result
  }

  async deleteRecord(id: Uuid): Promise<void> {
    const transaction = this.#database.transaction(RECORD_STORE, 'readwrite')
    transaction.objectStore(RECORD_STORE).delete(id)
    await transactionComplete(transaction)
  }

  async enqueue(item: SignedQueueItem): Promise<void> {
    const transaction = this.#database.transaction(QUEUE_STORE, 'readwrite')
    transaction.objectStore(QUEUE_STORE).put(item)
    await transactionComplete(transaction)
  }

  async listQueue(projectId?: Uuid): Promise<SignedQueueItem[]> {
    const transaction = this.#database.transaction(QUEUE_STORE, 'readonly')
    const store = transaction.objectStore(QUEUE_STORE)
    const request = projectId
      ? store.index('projectId').getAll(projectId)
      : store.index('queuedAt').getAll()
    const items = await requestResult<SignedQueueItem[]>(
      request,
    )
    await transactionComplete(transaction)
    return items
  }

  async removeQueueItem(id: Uuid): Promise<void> {
    const transaction = this.#database.transaction(QUEUE_STORE, 'readwrite')
    transaction.objectStore(QUEUE_STORE).delete(id)
    await transactionComplete(transaction)
  }

  async queueCount(): Promise<number> {
    const transaction = this.#database.transaction(QUEUE_STORE, 'readonly')
    const count = await requestResult(
      transaction.objectStore(QUEUE_STORE).count(),
    )
    await transactionComplete(transaction)
    return count
  }

  async putSyncMetadata(metadata: SyncMetadata): Promise<void> {
    const transaction = this.#database.transaction(SYNC_STORE, 'readwrite')
    transaction.objectStore(SYNC_STORE).put(metadata)
    await transactionComplete(transaction)
  }

  async getSyncMetadata(projectId: Uuid): Promise<SyncMetadata> {
    const transaction = this.#database.transaction(SYNC_STORE, 'readonly')
    const metadata = await requestResult<SyncMetadata | undefined>(
      transaction.objectStore(SYNC_STORE).get(projectId),
    )
    await transactionComplete(transaction)
    return (
      metadata ?? {
        projectId,
        cursor: 0,
        deviceSequence: 0,
      }
    )
  }

  async putVault(record: VaultCipherRecord): Promise<void> {
    const transaction = this.#database.transaction(VAULT_STORE, 'readwrite')
    transaction.objectStore(VAULT_STORE).put(record)
    await transactionComplete(transaction)
  }

  async getVault(deviceId: Uuid): Promise<VaultCipherRecord | undefined> {
    const transaction = this.#database.transaction(VAULT_STORE, 'readonly')
    const record = await requestResult<VaultCipherRecord | undefined>(
      transaction.objectStore(VAULT_STORE).get(`device:${deviceId}`),
    )
    await transactionComplete(transaction)
    return record
  }

  async putTombstone(tombstone: LocalTombstone): Promise<void> {
    const transaction = this.#database.transaction(
      TOMBSTONE_STORE,
      'readwrite',
    )
    transaction.objectStore(TOMBSTONE_STORE).put(tombstone)
    await transactionComplete(transaction)
  }

  async getTombstone(resourceId: Uuid): Promise<LocalTombstone | undefined> {
    const transaction = this.#database.transaction(TOMBSTONE_STORE, 'readonly')
    const tombstone = await requestResult<LocalTombstone | undefined>(
      transaction.objectStore(TOMBSTONE_STORE).get(resourceId),
    )
    await transactionComplete(transaction)
    return tombstone
  }

  async putConflict(conflict: SyncConflict): Promise<void> {
    const transaction = this.#database.transaction(
      CONFLICT_STORE,
      'readwrite',
    )
    transaction.objectStore(CONFLICT_STORE).put(conflict)
    await transactionComplete(transaction)
  }

  async listConflicts(projectId?: Uuid): Promise<SyncConflict[]> {
    const transaction = this.#database.transaction(
      CONFLICT_STORE,
      'readonly',
    )
    const store = transaction.objectStore(CONFLICT_STORE)
    const request = projectId
      ? store.index('projectId').getAll(projectId)
      : store.getAll()
    const conflicts = await requestResult<SyncConflict[]>(request)
    await transactionComplete(transaction)
    return conflicts
  }

  async removeConflict(id: Uuid): Promise<void> {
    const transaction = this.#database.transaction(
      CONFLICT_STORE,
      'readwrite',
    )
    transaction.objectStore(CONFLICT_STORE).delete(id)
    await transactionComplete(transaction)
  }
}
