// @vitest-environment node

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { EncryptedDatabase } from '../storage/encrypted-db'
import { KeyVault } from '../security/key-vault'
import type { DeviceSecrets } from '../security/wasm'

const secrets = (): DeviceSecrets => ({
  keyVersion: 1,
  suiteVersion: 0x8001,
  publicPackage: crypto.getRandomValues(new Uint8Array(64)),
  x25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
  mlKem768PrivateKey: crypto.getRandomValues(new Uint8Array(48)),
  ed25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
  mlDsa65PrivateKey: crypto.getRandomValues(new Uint8Array(64)),
})

describe('createEncryptedResource key lifetime', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.stubEnv('DEV', true)
  })

  afterEach(() => {
    vi.unstubAllEnvs()
    vi.doUnmock('../security/wasm')
  })

  it('awaits encrypt before finally zeroBytes (no mid-encrypt wipe)', async () => {
    let keySeenDuringEncrypt: number[] | undefined
    vi.doMock('../security/wasm', async () => {
      const actual =
        await vi.importActual<typeof import('../security/wasm')>(
          '../security/wasm',
        )
      return {
        ...actual,
        encryptDocument: async (
          _document: unknown,
          options: { resourceKey: Uint8Array },
        ) => {
          await Promise.resolve()
          keySeenDuringEncrypt = [...options.resourceKey]
          return {
            key_id: 'k',
            algorithm: 'test',
            nonce_b64: 'n',
            ciphertext_b64: 'c',
          }
        },
      }
    })

    const { createEncryptedResource } = await import('./resources')
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    vault.setSessionSecrets(crypto.randomUUID(), secrets(), crypto.randomUUID())

    await createEncryptedResource(vault, {
      projectId: crypto.randomUUID(),
      resourceId: crypto.randomUUID(),
      kind: 'topic',
      aggregateVersion: 1,
      document: { schema: 1, name: 'x' },
    })

    expect(keySeenDuringEncrypt).toBeDefined()
    expect(keySeenDuringEncrypt!.some((byte) => byte !== 0)).toBe(true)
  })

  it('awaits header encrypt before finally zeroBytes', async () => {
    let keySeenDuringEncrypt: number[] | undefined
    vi.doMock('../security/wasm', async () => {
      const actual =
        await vi.importActual<typeof import('../security/wasm')>(
          '../security/wasm',
        )
      return {
        ...actual,
        encryptDocument: async (
          _document: unknown,
          options: { resourceKey: Uint8Array },
        ) => {
          await Promise.resolve()
          keySeenDuringEncrypt = [...options.resourceKey]
          return {
            key_id: 'k',
            algorithm: 'test',
            nonce_b64: 'n',
            ciphertext_b64: 'c',
          }
        },
      }
    })

    const { createEncryptedResourceHeader } = await import('./resources')
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const resourceId = crypto.randomUUID()
    vault.setSessionSecrets(crypto.randomUUID(), secrets(), crypto.randomUUID())

    await createEncryptedResourceHeader(vault, {
      projectId: crypto.randomUUID(),
      resourceId,
      kind: 'topic',
      aggregateVersion: 1,
      document: { schema: 1, name: 'x' },
    })

    expect(keySeenDuringEncrypt).toBeDefined()
    expect(keySeenDuringEncrypt!.some((byte) => byte !== 0)).toBe(true)
    expect(vault.getHeaderKey(resourceId)?.some((byte) => byte !== 0)).toBe(
      true,
    )
  })
})
