// @vitest-environment node

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { EncryptedDatabase } from '../storage/encrypted-db'
import type { SessionResponse } from '../api/contracts'
import {
  backupDevResourceKeys,
  countDevResourceKeyBackup,
  hasDevResourceKeyBackup,
  mergeDevResourceKeysIntoSnapshot,
  persistDevVault,
  purgeZeroDevResourceKeys,
} from './dev-resource-keys'
import { loadDevSession } from './dev-session'
import { KeyVault, type DevVaultSnapshot } from './key-vault'
import { bytesToBase64, type DeviceSecrets } from './wasm'

const secrets = (): DeviceSecrets => ({
  keyVersion: 1,
  suiteVersion: 0x8001,
  publicPackage: crypto.getRandomValues(new Uint8Array(64)),
  x25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
  mlKem768PrivateKey: crypto.getRandomValues(new Uint8Array(48)),
  ed25519PrivateKey: crypto.getRandomValues(new Uint8Array(32)),
  mlDsa65PrivateKey: crypto.getRandomValues(new Uint8Array(64)),
})

describe('dev resource key backup', () => {
  let storage = new Map<string, string>()

  beforeEach(() => {
    storage = new Map()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value)
      },
      removeItem: (key: string) => {
        storage.delete(key)
      },
    })
    vi.stubEnv('DEV', true)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  it('backs up keys and serves them via getResourceKey after clear', async () => {
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const identityId = crypto.randomUUID()
    const resourceId = crypto.randomUUID()
    const key = crypto.getRandomValues(new Uint8Array(32))

    vault.setSessionSecrets(crypto.randomUUID(), secrets(), identityId)
    await vault.putResourceKey(resourceId, key, 1)
    backupDevResourceKeys(identityId, vault)
    expect(hasDevResourceKeyBackup(identityId)).toBe(true)

    vault.clearMemory()
    vault.setSessionSecrets(crypto.randomUUID(), secrets(), identityId)
    expect(vault.getResourceKey(resourceId, 1)).toEqual(key)
  })

  it('merges backup into snapshots synchronously', () => {
    const identityId = crypto.randomUUID()
    const resourceId = crypto.randomUUID()
    const slot = `body:${resourceId}:1`
    const live = bytesToBase64(crypto.getRandomValues(new Uint8Array(32)))
    storage.set(
      'sprout-dev-resource-keys',
      JSON.stringify({ [identityId]: { [slot]: live } }),
    )
    const snapshot: DevVaultSnapshot = {
      deviceId: crypto.randomUUID(),
      identityId,
      device: {
        keyVersion: 1,
        suiteVersion: 0x8001,
        publicPackageB64: 'cA==',
        x25519PrivateKeyB64: 'cA==',
        mlKem768PrivateKeyB64: 'cA==',
        ed25519PrivateKeyB64: 'cA==',
        mlDsa65PrivateKeyB64: 'cA==',
      },
      resourceKeys: {},
    }
    const merged = mergeDevResourceKeysIntoSnapshot(identityId, snapshot)
    expect(merged.resourceKeys[slot]).toBe(live)
  })

  it('does not let an empty/cleared vault overwrite live backup keys', async () => {
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const identityId = crypto.randomUUID()
    const deviceId = crypto.randomUUID()
    const resourceId = crypto.randomUUID()
    const key = crypto.getRandomValues(new Uint8Array(32))
    const session: SessionResponse = {
      token: 't',
      identity_id: identityId,
      device_id: deviceId,
      expires_at: new Date(Date.now() + 60_000).toISOString(),
    }

    vault.setSessionSecrets(deviceId, secrets(), identityId)
    await vault.putResourceKey(resourceId, key, 1)
    persistDevVault(session, vault)
    const liveB64 = bytesToBase64(key)

    // Cleared vault (no live keys) must not wipe storage.
    vault.clearMemory()
    vault.setSessionSecrets(deviceId, secrets(), identityId)
    persistDevVault(session, vault)

    expect(loadDevSession()?.vault?.resourceKeys[`body:${resourceId}:1`]).toBe(
      liveB64,
    )
    expect(countDevResourceKeyBackup(identityId)).toBe(1)
  })

  it('refuses all-zero resource keys and falls back to live backup', async () => {
    const database = { putVault: vi.fn() } as unknown as EncryptedDatabase
    const vault = new KeyVault(database)
    const identityId = crypto.randomUUID()
    const resourceId = crypto.randomUUID()
    const key = crypto.getRandomValues(new Uint8Array(32))

    vault.setSessionSecrets(crypto.randomUUID(), secrets(), identityId)
    await vault.putResourceKey(resourceId, key, 1)
    backupDevResourceKeys(identityId, vault)

    await expect(
      vault.putResourceKey(resourceId, new Uint8Array(32), 1),
    ).rejects.toThrow(/all-zero/)

    // Corrupt in-memory slot, then decrypt path must use backup.
    vault.restoreDevSnapshot({
      deviceId: crypto.randomUUID(),
      identityId,
      device: {
        keyVersion: 1,
        suiteVersion: 0x8001,
        publicPackageB64: bytesToBase64(new Uint8Array(8)),
        x25519PrivateKeyB64: bytesToBase64(new Uint8Array(32)),
        mlKem768PrivateKeyB64: bytesToBase64(new Uint8Array(48)),
        ed25519PrivateKeyB64: bytesToBase64(new Uint8Array(32)),
        mlDsa65PrivateKeyB64: bytesToBase64(new Uint8Array(64)),
      },
      resourceKeys: {
        [`body:${resourceId}:1`]: bytesToBase64(new Uint8Array(32)),
      },
    })
    expect(vault.getResourceKey(resourceId, 1)).toEqual(key)
  })

  it('purges zeroed corrupt keys from storage', () => {
    const identityId = crypto.randomUUID()
    const resourceId = crypto.randomUUID()
    const zero = bytesToBase64(new Uint8Array(32))
    const live = bytesToBase64(crypto.getRandomValues(new Uint8Array(32)))
    storage.set(
      'sprout-dev-resource-keys',
      JSON.stringify({
        [identityId]: {
          [`body:${resourceId}:1`]: zero,
          [`body:${crypto.randomUUID()}:1`]: live,
        },
      }),
    )
    expect(purgeZeroDevResourceKeys()).toBe(1)
    expect(countDevResourceKeyBackup(identityId)).toBe(1)
  })
})
