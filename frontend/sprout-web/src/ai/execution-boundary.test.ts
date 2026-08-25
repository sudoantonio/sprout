import { describe, expect, it } from 'vitest'
import { browserDirectInferenceAllowed, inferenceExecutionEnvironment } from './execution-boundary'

describe('browser/edge inference boundary', () => {
  it('treats the web app as control plane and never infers CORS from Node fetch', () => {
    expect(inferenceExecutionEnvironment()).toBe('browser_control_plane')
    expect(
      browserDirectInferenceAllowed({
        mode: 'commercial_api',
        provider: 'deepseek',
        credential: 'local-only',
        model: 'deepseek-v4-flash',
        preferences: { timeoutMs: 1000, maxOutputTokens: 32, maxAttempts: 1 },
      }),
    ).toBe(false)
  })
})
