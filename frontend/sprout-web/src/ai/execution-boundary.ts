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
 * Browser CORS is never inferred from a successful Node fetch. Credentialed
 * cloud and LAN calls execute in the user-owned native edge unless an adapter
 * has independently established a browser-safe origin contract.
 */
export const browserDirectInferenceAllowed = (_profile: LocalAiProfile): false => false

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
