import { describe, expect, it, vi } from 'vitest'
import { preparePrivacyInput, pseudonymize, type PrivacyCompanion } from './privacy-companion'

describe('isolated local privacy companion', () => {
  it('pseudonymizes deterministically, reconstructs, rejects unknown placeholders and purges mapping', () => {
    const input = 'Mario Rossi scrive a mario@example.com'
    const mapping = pseudonymize(input, [
      { start: 0, end: 11, kind: 'PERSON' },
      { start: input.indexOf('mario@'), end: input.length, kind: 'EMAIL' },
    ])
    expect(mapping.transformed).toBe('[[PERSON_0001]] scrive a [[EMAIL_0001]]')
    expect(mapping.reconstruct('Ciao [[PERSON_0001]]')).toBe('Ciao Mario Rossi')
    expect(() => mapping.reconstruct('[[PERSON_9999]]')).toThrow('Unknown privacy placeholder')
    mapping.purge()
    expect(mapping.purged).toBe(true)
    expect(() => mapping.reconstruct('Ciao')).toThrow('purged')
  })

  it.each([
    { runtimeInstalled: false, modelInstalled: false },
    { runtimeInstalled: true, modelInstalled: false },
  ])('fails explicitly when runtime/model is absent: %j', async (status) => {
    const companion: PrivacyCompanion = {
      status: async () => status,
      classify: vi.fn(),
      requestRuntimeInstallConsent: async () => false,
      requestModelDownloadConsent: async () => false,
      removeModel: async () => undefined,
      uninstallRuntime: async () => undefined,
    }
    await expect(preparePrivacyInput(companion, 'plaintext')).rejects.toMatchObject({
      code: 'privacy_companion_unavailable',
    })
    expect(companion.classify).not.toHaveBeenCalled()
  })

  it('requires separate installer and model-download consent in the companion contract', async () => {
    const install = vi.fn(async () => true)
    const download = vi.fn(async () => false)
    const companion: PrivacyCompanion = {
      status: async () => ({ runtimeInstalled: false, modelInstalled: false }),
      classify: async () => [],
      requestRuntimeInstallConsent: install,
      requestModelDownloadConsent: download,
      removeModel: async () => undefined,
      uninstallRuntime: async () => undefined,
    }
    expect(await companion.requestRuntimeInstallConsent()).toBe(true)
    expect(await companion.requestModelDownloadConsent()).toBe(false)
    expect(install).toHaveBeenCalledOnce()
    expect(download).toHaveBeenCalledOnce()
  })
})
