// @vitest-environment node

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SessionResponse } from '../api/contracts'
import type { DevVaultSnapshot } from './key-vault'

const session = (): SessionResponse => ({
  token: 'dev-token',
  identity_id: crypto.randomUUID(),
  device_id: crypto.randomUUID(),
  expires_at: new Date(Date.now() + 60_000).toISOString(),
})

const vault = (deviceId: string): DevVaultSnapshot => ({
  deviceId,
  identityId: crypto.randomUUID(),
  device: {
    keyVersion: 1,
    suiteVersion: 0x8001,
    publicPackageB64: 'cHVibGlj',
    x25519PrivateKeyB64: 'cHJpdmF0ZQ==',
    mlKem768PrivateKeyB64: 'a2Vt',
    ed25519PrivateKeyB64: 'ZWQ=',
    mlDsa65PrivateKeyB64: 'bWxkc2E=',
  },
  resourceKeys: {
    [`body:${crypto.randomUUID()}:1`]: 'cmVzb3VyY2U=',
  },
})

describe('dev session persistence', () => {
  let storage = new Map<string, string>()

  beforeEach(async () => {
    storage = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value)
      },
      removeItem: (key: string) => {
        storage.delete(key)
      },
    })
    vi.resetModules()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  const loadModule = async () => {
    vi.stubEnv('DEV', true)
    return import('./dev-session')
  }

  it('stores and restores session with vault snapshot', async () => {
    const { saveDevSession, loadDevSession } = await loadModule()
    const current = session()
    const snapshot = vault(current.device_id)
    saveDevSession(current, snapshot)
    expect(loadDevSession()).toEqual({
      session: current,
      vault: snapshot,
    })
  })

  it('restores legacy session-only payloads', async () => {
    const { loadDevSession } = await loadModule()
    const current = session()
    storage.set('sprout-dev-session', JSON.stringify(current))
    expect(loadDevSession()).toEqual({ session: current })
  })

  it('drops expired sessions', async () => {
    const { saveDevSession, loadDevSession } = await loadModule()
    const current = session()
    current.expires_at = new Date(Date.now() - 1_000).toISOString()
    saveDevSession(current)
    expect(loadDevSession()).toBeUndefined()
    expect(storage.has('sprout-dev-session')).toBe(false)
  })

  it('ignores vault snapshots for a different identity', async () => {
    const { saveDevSession, loadDevSession } = await loadModule()
    const current = session()
    // vault() generates its own random identityId ≠ session.identity_id
    saveDevSession(current, vault(crypto.randomUUID()))
    expect(loadDevSession()).toEqual({ session: current })
  })

  it('keeps vault when device drifts but identity matches', async () => {
    const { saveDevSession, loadDevSession } = await loadModule()
    const current = session()
    const snapshot = vault(crypto.randomUUID())
    snapshot.identityId = current.identity_id
    saveDevSession(current, snapshot)
    expect(loadDevSession()).toEqual({
      session: current,
      vault: snapshot,
    })
  })
})
