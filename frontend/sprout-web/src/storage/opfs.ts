import type { AttachmentCiphertext } from '../attachments/crypto'

interface OpfsStorageManager {
  getDirectory?: () => Promise<FileSystemDirectoryHandle>
}

const mapOpfsFailure = (error: unknown): Error => {
  const message = error instanceof Error ? error.message : String(error)
  if (
    /unknown transient reason|out of memory/i.test(message) ||
    (error instanceof DOMException && error.name === 'UnknownError')
  ) {
    return new Error(
      'Encrypted attachment storage (OPFS) is unavailable. In Safari this usually means a Private window: reopen http://localhost:4173 in a normal window and sign in again.',
    )
  }
  if (error instanceof Error) {
    return error
  }
  return new Error(message || 'Encrypted attachment storage failed')
}

const getOpfsRoot = async (): Promise<FileSystemDirectoryHandle> => {
  const storage = navigator.storage as OpfsStorageManager
  if (!storage.getDirectory) {
    throw new Error('Origin private file storage is unavailable')
  }
  try {
    return await storage.getDirectory()
  } catch (error) {
    throw mapOpfsFailure(error)
  }
}

const assertOpaqueFileId = (fileId: string): void => {
  if (!/^[a-zA-Z0-9_-]+$/.test(fileId)) {
    throw new Error('Attachment storage keys must be opaque identifiers')
  }
}

const attachmentDirectory = async (
  create: boolean,
): Promise<FileSystemDirectoryHandle> => {
  const root = await getOpfsRoot()
  return root.getDirectoryHandle('encrypted-attachments', { create })
}

export const isOpfsAvailable = (): boolean =>
  typeof navigator !== 'undefined' &&
  typeof (navigator.storage as OpfsStorageManager | undefined)?.getDirectory ===
    'function'

export const writeEncryptedAttachment = async (
  fileId: string,
  ciphertext: AttachmentCiphertext,
): Promise<void> => {
  assertOpaqueFileId(fileId)
  try {
    const directory = await attachmentDirectory(true)
    const handle = await directory.getFileHandle(fileId, { create: true })
    const writable = await handle.createWritable()
    try {
      await writable.write(ciphertext)
    } finally {
      await writable.close()
    }
  } catch (error) {
    throw mapOpfsFailure(error)
  }
}

export const readEncryptedAttachment = async (
  fileId: string,
): Promise<AttachmentCiphertext> => {
  assertOpaqueFileId(fileId)
  const directory = await attachmentDirectory(false)
  const handle = await directory.getFileHandle(fileId)
  return (await handle.getFile()) as unknown as AttachmentCiphertext
}

export const removeEncryptedAttachment = async (
  fileId: string,
): Promise<void> => {
  assertOpaqueFileId(fileId)
  const directory = await attachmentDirectory(false)
  await directory.removeEntry(fileId)
}
