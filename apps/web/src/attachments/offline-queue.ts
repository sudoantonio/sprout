import type {
  AttachmentDto,
  CreateAttachmentResponse,
  CreateTaskCompletedAttachmentRequest,
  Uuid,
} from '../api/contracts'
import { readEncryptedAttachment } from '../storage/opfs'

const DATABASE_NAME = 'sprout-encrypted-attachment-queue'
const DATABASE_VERSION = 1
const STORE_NAME = 'completed-attachments'

export interface QueuedCompletedAttachment {
  id: Uuid
  identityId: Uuid
  projectId: Uuid
  taskId: Uuid
  blobId: Uuid
  queuedAt: string
  attempts: number
  request: CreateTaskCompletedAttachmentRequest
}

interface AttachmentUploadApi {
  declareTaskCompletedAttachment(
    projectId: Uuid,
    taskId: Uuid,
    request: CreateTaskCompletedAttachmentRequest,
  ): Promise<CreateAttachmentResponse>
  uploadAttachmentCiphertext(
    projectId: Uuid,
    blobId: Uuid,
    ciphertext: Blob,
    uploadUrl?: string,
  ): Promise<void>
  finalizeAttachment(projectId: Uuid, blobId: Uuid): Promise<AttachmentDto>
}

export interface AttachmentFlushResult {
  uploaded: QueuedCompletedAttachment[]
  failed: QueuedCompletedAttachment[]
}

const requestResult = <T>(request: IDBRequest<T>): Promise<T> =>
  new Promise((resolve, reject) => {
    request.addEventListener('success', () => resolve(request.result))
    request.addEventListener('error', () =>
      reject(request.error ?? new Error('Attachment queue request failed')),
    )
  })

const transactionComplete = (transaction: IDBTransaction): Promise<void> =>
  new Promise((resolve, reject) => {
    transaction.addEventListener('complete', () => resolve())
    transaction.addEventListener('abort', () =>
      reject(transaction.error ?? new Error('Attachment queue transaction aborted')),
    )
    transaction.addEventListener('error', () =>
      reject(transaction.error ?? new Error('Attachment queue transaction failed')),
    )
  })

const openQueue = (): Promise<IDBDatabase> =>
  new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION)
    request.addEventListener('upgradeneeded', () => {
      const store = request.result.createObjectStore(STORE_NAME, {
        keyPath: 'id',
      })
      store.createIndex('queuedAt', 'queuedAt')
    })
    request.addEventListener('success', () => resolve(request.result))
    request.addEventListener('error', () =>
      reject(request.error ?? new Error('Unable to open attachment queue')),
    )
  })

const withQueue = async <T>(
  mode: IDBTransactionMode,
  operation: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> => {
  const database = await openQueue()
  try {
    const transaction = database.transaction(STORE_NAME, mode)
    const result = await requestResult(operation(transaction.objectStore(STORE_NAME)))
    await transactionComplete(transaction)
    return result
  } finally {
    database.close()
  }
}

export const enqueueCompletedAttachment = async (
  item: QueuedCompletedAttachment,
): Promise<void> => {
  await withQueue('readwrite', (store) => store.put(item))
}

export const listQueuedCompletedAttachments = (): Promise<
  QueuedCompletedAttachment[]
> => withQueue('readonly', (store) => store.index('queuedAt').getAll())

const removeQueuedCompletedAttachment = async (id: Uuid): Promise<void> => {
  await withQueue('readwrite', (store) => store.delete(id))
}

let activeFlush: Promise<AttachmentFlushResult> | undefined

export const flushCompletedAttachmentQueue = (
  api: AttachmentUploadApi,
  identityId: Uuid,
): Promise<AttachmentFlushResult> => {
  if (activeFlush) return activeFlush
  activeFlush = (async () => {
    const uploaded: QueuedCompletedAttachment[] = []
    const failed: QueuedCompletedAttachment[] = []
    for (const item of await listQueuedCompletedAttachments()) {
      if (item.identityId !== identityId) continue
      try {
        const declaration = await api.declareTaskCompletedAttachment(
          item.projectId,
          item.taskId,
          item.request,
        )
        await api.uploadAttachmentCiphertext(
          item.projectId,
          item.blobId,
          await readEncryptedAttachment(item.blobId),
          declaration.upload_url,
        )
        await api.finalizeAttachment(item.projectId, item.blobId)
        await removeQueuedCompletedAttachment(item.id)
        uploaded.push(item)
      } catch {
        const attempted = { ...item, attempts: item.attempts + 1 }
        await enqueueCompletedAttachment(attempted)
        failed.push(attempted)
      }
    }
    return { uploaded, failed }
  })().finally(() => {
    activeFlush = undefined
  })
  return activeFlush
}
