// @vitest-environment node

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { EncryptedDatabase } from '../storage/encrypted-db'
import { KeyVault } from '../security/key-vault'
import type { DeviceSecrets } from '../security/wasm'
import type { EncryptedPayloadDto, ProjectView } from '../api/contracts'

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
    vi.unstubAllGlobals()
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

  it('binds info ciphertext to its document id while reusing the container key', async () => {
    let captured:
      | {
          resourceId: string
          kind: string
          resourceKey: Uint8Array
        }
      | undefined
    vi.doMock('../security/wasm', async () => {
      const actual =
        await vi.importActual<typeof import('../security/wasm')>(
          '../security/wasm',
        )
      return {
        ...actual,
        encryptDocument: async (
          _document: unknown,
          options: {
            resourceId: string
            kind: string
            resourceKey: Uint8Array
          },
        ) => {
          captured = options
          return {
            version: 1,
            key_id: 'k',
            algorithm: 'test',
            nonce_b64: 'n',
            ciphertext_b64: 'c',
          }
        },
      }
    })

    const { encryptInfoDocument } = await import('./resources')
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const containerResourceId = crypto.randomUUID()
    const documentId = crypto.randomUUID()
    const key = crypto.getRandomValues(new Uint8Array(32))
    vault.setSessionSecrets(crypto.randomUUID(), secrets(), crypto.randomUUID())
    await vault.putResourceKey(containerResourceId, key)

    await encryptInfoDocument(vault, {
      projectId: crypto.randomUUID(),
      documentId,
      containerResourceId,
      aggregateVersion: 1,
      keyEpoch: 1,
      kind: 'task-list',
      document: { schema: 1, blocks: [] },
    })

    expect(captured?.resourceId).toBe(documentId)
    expect(captured?.kind).toBe('task-list')
    expect(captured?.resourceKey).toEqual(key)
  })

  it('mirrors the project metadata key to the project root resource', async () => {
    const { synchronizeProjectRootKey } = await import('./resources')
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const projectId = crypto.randomUUID()
    const rootResourceId = crypto.randomUUID()
    const key = crypto.getRandomValues(new Uint8Array(32))
    const project: ProjectView = {
      id: projectId,
      root_resource_id: rootResourceId,
      owner_identity_id: crypto.randomUUID(),
      encrypted_metadata_b64: 'opaque',
      key_epoch: 1,
      status: 'active',
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    }
    vault.setSessionSecrets(crypto.randomUUID(), secrets(), crypto.randomUUID())
    await vault.putResourceKey(projectId, key)

    await expect(synchronizeProjectRootKey(vault, project)).resolves.toBe(true)
    expect(vault.getResourceKey(rootResourceId)).toEqual(key)
  })

  it('does not invent a project root key when neither alias is available', async () => {
    const { synchronizeProjectRootKey } = await import('./resources')
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const project: ProjectView = {
      id: crypto.randomUUID(),
      root_resource_id: crypto.randomUUID(),
      owner_identity_id: crypto.randomUUID(),
      encrypted_metadata_b64: 'opaque',
      key_epoch: 1,
      status: 'active',
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    }
    vault.setSessionSecrets(crypto.randomUUID(), secrets(), crypto.randomUUID())

    await expect(synchronizeProjectRootKey(vault, project)).resolves.toBe(false)
    expect(vault.getResourceKey(project.root_resource_id)).toBeUndefined()
  })

  it('rebinds a legacy backup slot only after ciphertext authentication', async () => {
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    })
    const key = crypto.getRandomValues(new Uint8Array(32))
    const wireResourceId = crypto.randomUUID()
    const document = { schema: 1 as const, name: 'Recuperata' }
    vi.doMock('../security/wasm', async () => {
      const actual =
        await vi.importActual<typeof import('../security/wasm')>(
          '../security/wasm',
        )
      return {
        ...actual,
        decryptDocument: async (
          _ciphertext: EncryptedPayloadDto,
          options: { resourceId: string; resourceKey: Uint8Array },
        ) => {
          if (
            options.resourceId !== wireResourceId ||
            options.resourceKey.length !== key.length ||
            options.resourceKey.some((byte, index) => byte !== key[index])
          ) {
            throw new Error('authentication failed')
          }
          return document
        },
      }
    })

    const { backupDevResourceKeys, recoverDevResourceKeyFromBackup } =
      await import('../security/dev-resource-keys')
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const identityId = crypto.randomUUID()
    vault.setSessionSecrets(crypto.randomUUID(), secrets(), identityId)
    await vault.putResourceKey(crypto.randomUUID(), key, 1)
    backupDevResourceKeys(identityId, vault)

    const recovered = await recoverDevResourceKeyFromBackup<typeof document>(
      vault,
      {
        ciphertext: {
          version: 1,
          key_id: crypto.randomUUID(),
          algorithm: 'test',
          nonce_b64: 'n',
          ciphertext_b64: 'c',
        },
        projectId: crypto.randomUUID(),
        resourceId: wireResourceId,
        kind: 'topic',
        aggregateVersion: 1,
        keyEpoch: 1,
        purpose: 'body',
      },
    )

    expect(recovered).toEqual(document)
    expect(vault.getResourceKey(wireResourceId, 1)).toEqual(key)
  })
})
