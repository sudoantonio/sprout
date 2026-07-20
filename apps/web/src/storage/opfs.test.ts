import { afterEach, describe, expect, it, vi } from 'vitest'
import { asAttachmentCiphertext } from '../attachments/crypto'
import {
  readEncryptedAttachment,
  removeEncryptedAttachment,
  writeEncryptedAttachment,
} from './opfs'

describe('encrypted attachment OPFS storage', () => {
  const files = new Map<string, Blob>()

  afterEach(() => {
    files.clear()
    vi.restoreAllMocks()
  })

  const installOpfs = () => {
    const directory = {
      getFileHandle: vi.fn(async (name: string) => ({
        createWritable: async () => ({
          write: async (blob: Blob) => files.set(name, blob),
          close: async () => undefined,
        }),
        getFile: async () => files.get(name) as File,
      })),
      removeEntry: vi.fn(async (name: string) => {
        files.delete(name)
      }),
    }
    Object.defineProperty(navigator, 'storage', {
      configurable: true,
      value: {
        getDirectory: async () => ({
          getDirectoryHandle: async () => directory,
        }),
      },
    })
    return directory
  }

  it('uses only opaque IDs and preserves ciphertext bytes', async () => {
    const directory = installOpfs()
    const ciphertext = asAttachmentCiphertext(
      new Blob([new Uint8Array([83, 80, 82, 79, 85, 84, 1, 2, 3])]),
    )

    await writeEncryptedAttachment('opaque_blob_123', ciphertext)
    expect(directory.getFileHandle).toHaveBeenCalledWith('opaque_blob_123', {
      create: true,
    })
    expect(
      new Uint8Array(
        await (await readEncryptedAttachment('opaque_blob_123')).arrayBuffer(),
      ),
    ).toEqual(new Uint8Array([83, 80, 82, 79, 85, 84, 1, 2, 3]))

    await removeEncryptedAttachment('opaque_blob_123')
    expect(files.has('opaque_blob_123')).toBe(false)
  })

  it.each(['../secret.txt', '/tmp/plaintext', 'file name.txt'])(
    'rejects a local path instead of synchronizing it: %s',
    async (path) => {
      installOpfs()
      await expect(
        writeEncryptedAttachment(
          path,
          asAttachmentCiphertext(new Blob(['ciphertext'])),
        ),
      ).rejects.toThrow(/opaque identifiers/)
    },
  )
})
