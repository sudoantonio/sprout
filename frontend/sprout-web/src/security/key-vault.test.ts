// @vitest-environment node

import { describe, expect, it, vi } from 'vitest'
import type { EncryptedDatabase, VaultCipherRecord } from '../storage/encrypted-db'
import { KeyVault } from './key-vault'
import { bytesToBase64, type DeviceSecrets } from './wasm'

describe('local key vault', () => {
  it('keeps separate keys for every resource epoch', async () => {
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const resourceId = crypto.randomUUID()
    const epochOne = crypto.getRandomValues(new Uint8Array(32))
    const epochTwo = crypto.getRandomValues(new Uint8Array(32))

    await vault.putResourceKey(resourceId, epochOne, 1)
    await vault.putResourceKey(resourceId, epochTwo, 2)

    expect(vault.getResourceKey(resourceId, 1)).toEqual(epochOne)
    expect(vault.getResourceKey(resourceId, 2)).toEqual(epochTwo)
    expect(vault.getResourceKey(resourceId, 3)).toBeUndefined()
  })

  it('keeps body and header keys in separate slots', async () => {
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const resourceId = crypto.randomUUID()
    const bodyKey = crypto.getRandomValues(new Uint8Array(32))
    const headerKey = crypto.getRandomValues(new Uint8Array(32))

    await vault.putResourceKey(resourceId, bodyKey, 1, 'body')
    await vault.putResourceKey(resourceId, headerKey, 1, 'header')

    expect(vault.getResourceKey(resourceId, 1)).toEqual(bodyKey)
    expect(vault.getHeaderKey(resourceId, 1)).toEqual(headerKey)
    expect(vault.getLatestResourceKey(resourceId)?.key).toEqual(bodyKey)
    expect(vault.getLatestHeaderKey(resourceId)?.key).toEqual(headerKey)

    // Callers must not be able to wipe vault slots by zeroing returned keys.
    vault.getHeaderKey(resourceId, 1)!.fill(0)
    expect(vault.getHeaderKey(resourceId, 1)).toEqual(headerKey)
  })

  it('getLatestResourceKey peeks DEV backup after clearMemory', async () => {
    vi.stubEnv('DEV', true)
    const storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value)
      },
      removeItem: (key: string) => {
        storage.delete(key)
      },
    })
    const deviceSecrets = (): DeviceSecrets => ({
      keyVersion: 1,
      suiteVersion: 0x8001,
      publicPackage: crypto.getRandomValues(new Uint8Array(64)),
      x25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
      mlKem768PrivateKey: crypto.getRandomValues(new Uint8Array(48)),
      ed25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
      mlDsa65PrivateKey: crypto.getRandomValues(new Uint8Array(64)),
    })
    try {
      const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
      const vault = new KeyVault(database)
      const identityId = crypto.randomUUID()
      const resourceId = crypto.randomUUID()
      const bodyKey = crypto.getRandomValues(new Uint8Array(32))
      const headerKey = crypto.getRandomValues(new Uint8Array(32))

      vault.setSessionSecrets(crypto.randomUUID(), deviceSecrets(), identityId)
      await vault.putResourceKey(resourceId, bodyKey, 2, 'body')
      await vault.putResourceKey(resourceId, headerKey, 2, 'header')
      storage.set(
        'sprout-dev-resource-keys',
        JSON.stringify({
          [identityId]: {
            [`body:${resourceId}:2`]: bytesToBase64(bodyKey),
            [`header:${resourceId}:2`]: bytesToBase64(headerKey),
          },
        }),
      )

      vault.clearMemory()
      vault.setSessionSecrets(crypto.randomUUID(), deviceSecrets(), identityId)
      expect(vault.getLatestResourceKey(resourceId)).toEqual({
        epoch: 2,
        key: bodyKey,
      })
      expect(vault.getHeaderKey(resourceId, 2)).toEqual(headerKey)
      expect(vault.getLatestHeaderKey(resourceId)).toEqual({
        epoch: 2,
        key: headerKey,
      })
    } finally {
      vi.unstubAllGlobals()
      vi.unstubAllEnvs()
    }
  })

  it('exports and restores dev snapshots without touching PRF persistence', async () => {
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const deviceId = crypto.randomUUID()
    const resourceId = crypto.randomUUID()
    const resourceKey = crypto.getRandomValues(new Uint8Array(32))
    const secrets: DeviceSecrets = {
      keyVersion: 2,
      suiteVersion: 0x8001,
      publicPackage: crypto.getRandomValues(new Uint8Array(64)),
      x25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
      mlKem768PrivateKey: crypto.getRandomValues(new Uint8Array(48)),
      ed25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
      mlDsa65PrivateKey: crypto.getRandomValues(new Uint8Array(64)),
    }

    vault.setSessionSecrets(deviceId, secrets, crypto.randomUUID())
    await vault.putResourceKey(resourceId, resourceKey, 1)

    const snapshot = vault.exportDevSnapshot()
    expect(snapshot?.deviceId).toBe(deviceId)

    vault.clearMemory()
    expect(vault.isUnlocked).toBe(false)

    vault.restoreDevSnapshot(snapshot!)
    expect(vault.isUnlocked).toBe(true)
    expect(vault.persistence).toBe('session-only')
    expect(vault.getResourceKey(resourceId, 1)).toEqual(resourceKey)
    expect(vault.localDeviceId).toBe(deviceId)

    const identityId = crypto.randomUUID()
    vault.clearMemory()
    vault.restoreDevSnapshot({
      ...snapshot!,
      identityId: undefined,
      resourceKeys: snapshot!.resourceKeys,
    })
    vault.ensureIdentityId(identityId)
    expect(vault.localIdentityId).toBe(identityId)
  })

  it('never exports device-local AI settings and deletes them explicitly', async () => {
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const secrets: DeviceSecrets = {
      keyVersion: 1,
      suiteVersion: 0x8001,
      publicPackage: crypto.getRandomValues(new Uint8Array(64)),
      x25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
      mlKem768PrivateKey: crypto.getRandomValues(new Uint8Array(48)),
      ed25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
      mlDsa65PrivateKey: crypto.getRandomValues(new Uint8Array(64)),
    }
    vault.setSessionSecrets(crypto.randomUUID(), secrets, crypto.randomUUID())
    const profile = '{"credential":"never-export-ai-secret"}'
    expect(await vault.putLocalSetting('device:ai-generation-profile-v1', profile)).toBe(false)
    expect(vault.getLocalSetting('device:ai-generation-profile-v1')).toBe(profile)
    expect(JSON.stringify(vault.exportDevSnapshot() ?? null)).not.toContain(
      'never-export-ai-secret',
    )

    expect(await vault.deleteLocalSetting('device:ai-generation-profile-v1')).toBe(false)
    expect(vault.getLocalSetting('device:ai-generation-profile-v1')).toBeUndefined()
    await vault.putLocalSetting('device:ai-generation-profile-v1', profile)
    vault.clearMemory()
    expect(vault.getLocalSetting('device:ai-generation-profile-v1')).toBeUndefined()
    expect(JSON.stringify(vault.exportDevSnapshot() ?? null)).not.toContain(
      'never-export-ai-secret',
    )
  })

  it('persists only PRF-wrapped key material and clears memory', async () => {
    let stored: VaultCipherRecord | undefined
    const database = {
      putVault: vi.fn(async (record: VaultCipherRecord) => {
        stored = record
      }),
    } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const secretMarker = crypto.getRandomValues(new Uint8Array(32))
    const secrets: DeviceSecrets = {
      keyVersion: 1,
      suiteVersion: 0x8001,
      publicPackage: crypto.getRandomValues(new Uint8Array(64)),
      x25519PrivateKey: secretMarker.slice(),
      mlKem768PrivateKey: crypto.getRandomValues(new Uint8Array(48)),
      ed25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
      mlDsa65PrivateKey: crypto.getRandomValues(new Uint8Array(64)),
    }
    const encodedMarker = bytesToBase64(secretMarker)

    vault.setSessionSecrets(crypto.randomUUID(), secrets)
    await vault.enablePrfPersistence(
      crypto.getRandomValues(new Uint8Array(32)),
      'test-passkey-credential',
    )

    expect(stored).toBeDefined()
    expect(JSON.stringify(stored)).not.toContain(encodedMarker)
    expect(vault.persistence).toBe('prf-wrapped')

    vault.clearMemory()
    expect(secrets.x25519PrivateKey.every((byte) => byte === 0)).toBe(true)
    expect(vault.persistence).toBe('locked')
  })
})
