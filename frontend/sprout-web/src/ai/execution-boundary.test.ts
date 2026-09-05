import { afterEach, describe, expect, it } from 'vitest'
import {
  browserDirectInferenceAllowed,
  inferenceExecutionEnvironment,
  resolveLocalEdgeInferenceBridge,
  type LocalEdgeInferenceBridge,
} from './execution-boundary'

const edgeGlobal = globalThis as typeof globalThis & {
  sproutLocalEdge?: LocalEdgeInferenceBridge
}

afterEach(() => {
  delete edgeGlobal.sproutLocalEdge
})

describe('browser/edge inference boundary', () => {
  it('allows explicitly configured commercial and LAN profiles to run on-device', () => {
    expect(inferenceExecutionEnvironment()).toBe('browser_control_plane')
    expect(
      browserDirectInferenceAllowed({
        mode: 'commercial_api',
        provider: 'deepseek',
        credential: 'local-only',
        model: 'deepseek-v4-flash',
        preferences: { timeoutMs: 1000, maxOutputTokens: 32, maxAttempts: 1 },
      }),
    ).toBe(true)
    expect(
      browserDirectInferenceAllowed({
        mode: 'lan_inference',
        engine: 'ollama',
        baseUrl: 'http://127.0.0.1:11434',
        model: 'qwen',
        preferences: { timeoutMs: 1000, maxOutputTokens: 32, maxAttempts: 1 },
      }),
    ).toBe(true)
  })

  it('accepts only the native bridge protocol used by the workspace chat', () => {
    expect(resolveLocalEdgeInferenceBridge()).toBeUndefined()
    const bridge = {
      protocolVersion: 'sprout-client-inference-edge-v1' as const,
    } as LocalEdgeInferenceBridge
    edgeGlobal.sproutLocalEdge = bridge
    expect(resolveLocalEdgeInferenceBridge()).toBe(bridge)
  })
})
