import type {
  LocalAiProfile,
  ProviderGenerationRequest,
  ProviderGenerationResult,
  ProviderModel,
} from './contracts'

export type InferenceExecutionEnvironment = 'browser_control_plane' | 'native_edge_runtime'

export const inferenceExecutionEnvironment = (): InferenceExecutionEnvironment =>
  typeof document === 'undefined' ? 'native_edge_runtime' : 'browser_control_plane'

/**
 * Commercial API and LAN profiles are explicit, device-local user choices.
 * They may execute through fetch when no native bridge is installed; the
 * provider remains responsible for accepting the browser origin.
 */
export const browserDirectInferenceAllowed = (profile: LocalAiProfile): boolean =>
  profile.mode === 'commercial_api' || profile.mode === 'lan_inference'

export interface LocalEdgeInferenceBridge {
  readonly protocolVersion: 'sprout-client-inference-edge-v1'
  discoverModels(profile: LocalAiProfile, signal?: AbortSignal): Promise<ProviderModel[]>
  generateStructured(
    profile: LocalAiProfile,
    request: ProviderGenerationRequest,
    signal?: AbortSignal,
  ): Promise<ProviderGenerationResult>
  detectOllama(): Promise<{ installed: boolean; version?: string; models: string[] }>
  installOfficialOllama(): Promise<{ installed: true; version: string }>
  pullOllamaModel(model: string): Promise<void>
}

/** Native desktop hosts inject this bridge; ordinary web origins leave it absent. */
export const resolveLocalEdgeInferenceBridge = (): LocalEdgeInferenceBridge | undefined => {
  const candidate = (globalThis as typeof globalThis & {
    sproutLocalEdge?: LocalEdgeInferenceBridge
  }).sproutLocalEdge
  return candidate?.protocolVersion === 'sprout-client-inference-edge-v1'
    ? candidate
    : undefined
}
