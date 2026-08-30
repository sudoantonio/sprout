import { describe, expect, it, vi } from 'vitest'
import type { KeyVault } from '../security/key-vault'
import { deleteLocalConnectorProfile, loadLocalConnectorProfile, rejectExternalSend, saveLocalConnectorProfile, type LocalConnectorProfile, type ReadOnlyConnectorAdapter } from './connector-profiles'

function localVault() {
  const settings = new Map<string, string>()
  return {
    vault: {
      getLocalSetting: (key: string) => settings.get(key),
      putLocalSetting: vi.fn(async (key: string, value: string) => { settings.set(key, value); return true }),
      deleteLocalSetting: vi.fn(async (key: string) => { settings.delete(key); return true }),
      exportDevSnapshot: () => ({ version: 1, identityId: 'local-only' }),
    } as unknown as KeyVault,
    settings,
  }
}

describe('device-local connector profiles', () => {
  it('stores and deletes only in the encrypted device vault with no Sprout sync', async () => {
    const { vault, settings } = localVault()
    const profile: LocalConnectorProfile = { version: 1, kind: 'mail.receive', opaqueProfileId: 'mail-1', encryptedConfiguration: 'ciphertext-only' }
    await saveLocalConnectorProfile(vault, profile)
    expect(loadLocalConnectorProfile(vault, 'mail.receive', 'mail-1')).toEqual(profile)
    expect(JSON.stringify(vault.exportDevSnapshot())).not.toContain('ciphertext-only')
    expect([...settings.keys()]).toEqual(['device:external-tool-connector:mail.receive:mail-1'])
    await deleteLocalConnectorProfile(vault, 'mail.receive', 'mail-1')
    expect(loadLocalConnectorProfile(vault, 'mail.receive', 'mail-1')).toBeUndefined()
  })

  it('supports read-only fake connector discovery/execution without enabling send', async () => {
    const profile: LocalConnectorProfile = { version: 1, kind: 'telegram.receive', opaqueProfileId: 'tg-1', encryptedConfiguration: 'ciphertext-only' }
    const adapter: ReadOnlyConnectorAdapter = {
      kind: 'telegram.receive',
      discoverCapabilities: vi.fn(async () => ['receive']),
      receiveStructured: vi.fn(async () => ({ messages: [] })),
    }
    await expect(adapter.discoverCapabilities(profile)).resolves.toEqual(['receive'])
    await expect(adapter.receiveStructured(profile, {}, new AbortController().signal)).resolves.toEqual({ messages: [] })
    expect(() => rejectExternalSend('mail.send')).toThrow('fail_closed_external_disclosure_sink_missing')
    expect(() => rejectExternalSend('telegram.send')).toThrow('fail_closed_external_disclosure_sink_missing')
  })
})
