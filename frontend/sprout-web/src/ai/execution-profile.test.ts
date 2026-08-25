import { describe, expect, it } from 'vitest'
import type { CommercialProfile } from './contracts'
import { executionProfileCommitment } from './execution-profile'

const profile = (): CommercialProfile => ({
  mode: 'commercial_api',
  provider: 'deepseek',
  credential: 'device-only-secret',
  model: 'deepseek-v4-flash',
  preferences: { timeoutMs: 30_000, maxOutputTokens: 64, maxAttempts: 2 },
})

describe('opaque execution-profile commitment', () => {
  it('is stable for one local secret/revision and changes with the profile', async () => {
    const secret = new Uint8Array(32).fill(7)
    const exact = await executionProfileCommitment(profile(), 'revision-1', secret)
    const replay = await executionProfileCommitment(profile(), 'revision-1', secret)
    const otherModel = await executionProfileCommitment(
      { ...profile(), model: 'deepseek-v4-pro' },
      'revision-2',
      secret,
    )
    expect(exact).toBe(replay)
    expect(exact).not.toBe(otherModel)
    expect(exact).not.toContain('deepseek')
    expect(exact).toMatch(/^[a-f0-9]{64}$/)
  })

  it('is hiding against enumeration without the device-only secret', async () => {
    const first = await executionProfileCommitment(profile(), 'revision-1', new Uint8Array(32).fill(1))
    const second = await executionProfileCommitment(profile(), 'revision-1', new Uint8Array(32).fill(2))
    expect(first).not.toBe(second)
  })
})
