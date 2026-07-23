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
