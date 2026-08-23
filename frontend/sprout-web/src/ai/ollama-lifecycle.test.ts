import { describe, expect, it, vi } from 'vitest'
import {
  OLLAMA_CHECKPOINT_MODEL,
  installOllamaWithConsent,
  pullOllamaModelWithSeparateConsent,
  type OllamaLifecycle,
} from './ollama-lifecycle'

describe('Ollama local installation lifecycle', () => {
  it('never installs without explicit consent and does not uninstall after use', async () => {
    const detect = vi
      .fn()
      .mockResolvedValueOnce({ installed: false, models: [] })
      .mockResolvedValueOnce({ installed: false, models: [] })
      .mockResolvedValueOnce({ installed: true, version: 'test', models: [] })
    const lifecycle: OllamaLifecycle = {
      detect,
      installOfficialDistribution: vi.fn(async () => ({ installed: true as const, version: 'test' })),
      pullModel: vi.fn(),
      removeModel: vi.fn(),
    }
    expect(await installOllamaWithConsent(lifecycle, false)).toBe(false)
    expect(lifecycle.installOfficialDistribution).not.toHaveBeenCalled()
    expect(await installOllamaWithConsent(lifecycle, true)).toBe(true)
    expect(lifecycle.installOfficialDistribution).toHaveBeenCalledOnce()
    expect(detect).toHaveBeenCalledTimes(3)
    expect(lifecycle.removeModel).not.toHaveBeenCalled()
  })

  it('requires separate model-pull consent and uses the checkpoint model exactly', async () => {
    const lifecycle: OllamaLifecycle = {
      detect: async () => ({ installed: true, models: [] }),
      installOfficialDistribution: vi.fn(async () => ({ installed: true as const, version: 'test' })),
      pullModel: vi.fn(),
      removeModel: vi.fn(),
    }
    expect(
      await pullOllamaModelWithSeparateConsent(lifecycle, OLLAMA_CHECKPOINT_MODEL, false),
    ).toBe(false)
    expect(lifecycle.pullModel).not.toHaveBeenCalled()
    expect(
      await pullOllamaModelWithSeparateConsent(lifecycle, OLLAMA_CHECKPOINT_MODEL, true),
    ).toBe(true)
    expect(lifecycle.pullModel).toHaveBeenCalledWith('qwen2.5:0.5b-instruct')
  })
})
