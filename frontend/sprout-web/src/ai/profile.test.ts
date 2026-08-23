import { afterEach, describe, expect, it, vi } from 'vitest'
import type { LocalAiProfile } from './contracts'
import { LocalAiProfileStore, redactAiSecrets } from './profile'
import { UnvalidatedPrivateRemoteProvider } from './remote-private'

const profile: LocalAiProfile = {
  mode: 'commercial_api',
  provider: 'deepseek',
  credential: 'super-secret',
  model: 'deepseek-v4-flash',
  preferences: { timeoutMs: 1000, maxOutputTokens: 32, maxAttempts: 1 },
}

describe('device-local AI profile', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('uses only the encrypted vault setting surface and never localStorage', async () => {
    const settings = new Map<string, string>()
    const vault = {
      getLocalSetting: (key: string) => settings.get(key),
      putLocalSetting: async (key: string, value: string) => {
        settings.set(key, value)
        return true
      },
      deleteLocalSetting: async (key: string) => {
        settings.delete(key)
        return true
      },
    }
    const store = new LocalAiProfileStore(vault)
    expect(await store.save(profile)).toBe('persisted')
    expect(store.load()).toEqual(profile)
    const committedBeforeRestart = await store.executionProfileCommitment()
    const restartedStore = new LocalAiProfileStore(vault)
    expect(await restartedStore.executionProfileCommitment()).toBe(committedBeforeRestart)
    await restartedStore.save({ ...profile, model: 'deepseek-v4-pro' })
    expect(await restartedStore.executionProfileCommitment()).not.toBe(committedBeforeRestart)
    expect(localStorage.length).toBe(0)
    expect(await store.delete()).toBe('persisted')
    expect(store.load()).toBeUndefined()
  })

  it('save/delete makes zero Sprout synchronization requests and removes local credential bytes', async () => {
    const settings = new Map<string, string>()
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
    const store = new LocalAiProfileStore({
      getLocalSetting: (key) => settings.get(key),
      putLocalSetting: async (key, value) => {
        settings.set(key, value)
        return true
      },
      deleteLocalSetting: async (key) => {
        settings.delete(key)
        return true
      },
    })
    await store.save(profile)
    expect([...settings.values()].join('')).toContain('super-secret')
    await store.delete()
    expect([...settings.values()].join('')).not.toContain('super-secret')
    expect(fetchSpy).not.toHaveBeenCalled()
  })

  it('redacts credentials, model and endpoint from diagnostics', () => {
    const redacted = JSON.stringify(redactAiSecrets(profile))
    expect(redacted).not.toContain('super-secret')
    expect(redacted).not.toContain('deepseek-v4-flash')
    expect(redacted).not.toContain('api.deepseek.com')
  })

  it('keeps private remote DS4/Ollama fail-closed without validated transport', async () => {
    const adapter = new UnvalidatedPrivateRemoteProvider({
      mode: 'private_remote',
      engine: 'ollama',
      destination: '203.0.113.8/32',
      baseUrl: 'https://203.0.113.8',
      tlsPinSha256: '00'.repeat(32),
      validatedTransport: false,
      model: 'model',
      preferences: { timeoutMs: 1000, maxOutputTokens: 8, maxAttempts: 1 },
    })
    await expect(adapter.discoverModels()).rejects.toMatchObject({
      code: 'remote_transport_unvalidated',
    })
  })
})
