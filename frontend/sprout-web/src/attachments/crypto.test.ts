import { describe, expect, it } from 'vitest'
import {
  attachmentCiphertextSha256,
  decryptAttachment,
  encryptAttachment,
} from './crypto'

const context = {
  projectId: '11111111-1111-4111-8111-111111111111',
  resourceId: '22222222-2222-4222-8222-222222222222',
  blobId: '33333333-3333-4333-8333-333333333333',
  keyEpoch: 1,
}

describe('encrypted attachment container', () => {
  it('stores opaque ciphertext and decrypts only in the bound context', async () => {
    const plaintext = new TextEncoder().encode(
      'classified-attachment-canary-05',
    )
    const key = crypto.getRandomValues(new Uint8Array(32))
    const ciphertext = await encryptAttachment(
      new Blob([plaintext], { type: 'text/plain' }),
      key,
      context,
    )

    expect(ciphertext.type).toBe('application/octet-stream')
    const stored = new Uint8Array(await ciphertext.arrayBuffer())
    expect(new TextDecoder().decode(stored)).not.toContain(
      'classified-attachment-canary-05',
    )
    expect(await attachmentCiphertextSha256(ciphertext)).toMatch(
      /^[A-Za-z0-9+/]+={0,2}$/,
    )
    await expect(
      decryptAttachment(ciphertext, key, {
        ...context,
        blobId: '44444444-4444-4444-8444-444444444444',
      }),
    ).rejects.toThrow()
    expect(
      Array.from(await decryptAttachment(ciphertext, key, context)),
    ).toEqual(Array.from(plaintext))
  })
})
