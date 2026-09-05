import type { EncryptedPayloadDto, Uuid } from '../api/contracts'

export const LOCAL_AI_PROFILE_NOTICE =
  'Valida soltanto su questo dispositivo — non sincronizzata con Sprout'

export type AiMode =
  | 'commercial_api'
  | 'lan_inference'
  | 'private_remote'
  | 'commercial_privacy'

export type CommercialProvider =
  | 'openai'
  | 'anthropic'
  | 'xai'
  | 'deepseek'
  | 'openai_compatible'
  | 'anthropic_compatible'

export type SelfHostedEngine = 'ds4' | 'ollama'
export type SupportedLanguageTask =
  | 'answer_from_authorized_context'
  | 'interpret_proxy_request'

export interface GenerationPreferences {
  timeoutMs: number
  maxOutputTokens: number
  maxAttempts: number
  temperature?: number
}

interface BaseProfile {
  mode: AiMode
  model: string
  preferences: GenerationPreferences
}

export interface CommercialProfile extends BaseProfile {
  mode: 'commercial_api'
  provider: CommercialProvider
  credential: string
  baseUrl?: string
}

export interface LanProfile extends BaseProfile {
  mode: 'lan_inference'
  engine: SelfHostedEngine
  baseUrl: string
  token?: string
  tlsPinSha256?: string
  /** Explicit development exception. Only loopback or a LAN test fixture may use it. */
  allowInsecureDevelopmentHttp?: boolean
}

export interface PrivateRemoteProfile extends BaseProfile {
  mode: 'private_remote'
  engine: SelfHostedEngine
  destination: string
  baseUrl: string
  token?: string
  tlsPinSha256: string
  transportProfileId?: string
  validatedTransport: false
}

export interface CommercialPrivacyProfile extends BaseProfile {
  mode: 'commercial_privacy'
  provider: CommercialProvider
  credential: string
  baseUrl?: string
  companionUrl: string
  companionProtocolVersion: 'sprout-local-privacy-v1'
  privacyModel: 'gpt-oss-safeguard-20b'
  companionInstalled: boolean
  modelInstalled: boolean
}

export type LocalAiProfile =
  | CommercialProfile
  | LanProfile
  | PrivateRemoteProfile
  | CommercialPrivacyProfile

export interface ProviderCapabilities {
  modelDiscovery: boolean
  structuredOutput: boolean
  cancellation: boolean
  maxContextItems?: number
}

export interface InferenceSource {
  descriptor: InformationSource
  plaintext: string
}

export interface ProviderGenerationRequest {
  task: SupportedLanguageTask
  model: string
  instructions: string
  sources: InferenceSource[]
  input: unknown
  outputSchema: JsonSchema
  preferences: GenerationPreferences
}

export interface ProviderGenerationResult {
  value: unknown
  attemptCount: number
  sanitizedStatus: string
  wireWitness: ProviderWireRequestWitness
  actualRequestCommitmentHex: string
  actualOutputCommitmentHex: string
}

export type ProviderWireProtocol =
  | 'openai_responses_v1'
  | 'openai_chat_completions_v1'
  | 'deepseek_chat_v4'
  | 'anthropic_messages_v1'
  | 'ollama_chat_v1'
  | 'ds4_openai_chat_v1'

/**
 * Non-secret semantic headers fixed by each wire protocol identity. Adapters
 * cannot vary these independently of the committed protocol. Authentication
 * headers are deliberately excluded from both this manifest and the witness.
 */
export const PROVIDER_PROTOCOL_SEMANTIC_HEADERS: Record<
  ProviderWireProtocol,
  Readonly<Record<string, string>>
> = {
  openai_responses_v1: { Accept: 'application/json', 'Content-Type': 'application/json' },
  openai_chat_completions_v1: { Accept: 'application/json', 'Content-Type': 'application/json' },
  deepseek_chat_v4: { Accept: 'application/json', 'Content-Type': 'application/json' },
  anthropic_messages_v1: {
    Accept: 'application/json',
    'Content-Type': 'application/json',
    'anthropic-version': '2023-06-01',
  },
  ollama_chat_v1: { Accept: 'application/json', 'Content-Type': 'application/json' },
  ds4_openai_chat_v1: { Accept: 'application/json', 'Content-Type': 'application/json' },
}

/**
 * Exact semantic request passed to fetch: protocol-fixed non-secret headers,
 * method/path and exact body. Authorization headers are excluded.
 */
export interface ProviderWireRequestWitness {
  protocol: ProviderWireProtocol
  method: 'POST'
  path: string
  selectedModel: string
  /** Exact JSON bytes passed as RequestInit.body, represented as UTF-8 text. */
  body: string
}

export interface ProviderModel {
  id: string
  label?: string
  /** Provider-declared model parameters. Informational; never grants capability. */
  supportedParameters?: string[]
}

export interface ProviderAdapter {
  readonly capabilities: ProviderCapabilities
  discoverModels(signal?: AbortSignal): Promise<ProviderModel[]>
  generateStructured(
    request: ProviderGenerationRequest,
    signal?: AbortSignal,
  ): Promise<ProviderGenerationResult>
}

export type JsonSchema = {
  type: 'object'
  additionalProperties: false
  required: string[]
  properties: Record<string, unknown>
}

export type InformationSource =
  | { kind: 'resource_body'; resource_id: Uuid }
  | { kind: 'comment'; resource_id: Uuid; comment_id: Uuid }
  | { kind: 'info_document'; resource_id: Uuid; document_id: Uuid }
  | { kind: 'info_file'; resource_id: Uuid; file_id: Uuid }
  | { kind: 'tool_output'; call_id: Uuid }
  | { kind: 'proxy_transcript'; thread_id: Uuid }
  | { kind: 'event_history'; event_id: Uuid }
  | { kind: 'provenance'; provenance_id: Uuid }

export interface StructuredLanguageTaskEnvelopeDto {
  id: Uuid
  kind: SupportedLanguageTask
  input_item_count: number
  max_input_items: number
  max_output_items: number
  max_nesting_depth: number
  max_attempts: number
  closed_output_schema: boolean
  grounded_identifiers_only: boolean
  requires_formal_proof: boolean
  requires_permission_decision: boolean
  requires_exact_semantic_equivalence: boolean
  requires_exhaustive_world_knowledge: boolean
  allowed_resource_ids: Uuid[]
  allowed_principal_ids: Uuid[]
  allowed_tools: string[]
}

export interface ClaimedLanguageInvocation {
  id: Uuid
  dispatch_id: Uuid
  lease_id: Uuid
  lease_expires_at: string
  attempt: number
  language_task: StructuredLanguageTaskEnvelopeDto
  authority_envelope: unknown
  sources: InformationSource[]
  encrypted_input: EncryptedPayloadDto
  context_principal_identity_id: Uuid
  request_commitment_hex: string
  context_commitment_hex: string
  transport_commitment_hex: string
  runtime_kind: 'client_provider_v1'
}

export interface DeviceObservationSigner {
  identityId: Uuid
  deviceId: Uuid
  keyVersion: number
  sign(message: Uint8Array, context: string): Promise<{
    classicalSignature: Uint8Array
    postQuantumSignature: Uint8Array
  }>
}

export const MODEL_OBSERVATION_SIGNATURE_CONTEXT =
  'sprout-model-runtime-observation-v1'
