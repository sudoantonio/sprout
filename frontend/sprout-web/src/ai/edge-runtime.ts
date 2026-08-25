import type { ApiClient } from '../api/client'
import type { EncryptedPayloadDto, Uuid } from '../api/contracts'
import { canonicalGovernanceJson, sha256Hex } from './canonical'
import type {
  ClaimedLanguageInvocation,
  DeviceObservationSigner,
  InformationSource,
  JsonSchema,
  ProviderAdapter,
  ProviderGenerationRequest,
  ProviderGenerationResult,
} from './contracts'
import { MODEL_OBSERVATION_SIGNATURE_CONTEXT } from './contracts'
import {
  ProviderFailure,
  assertExactWireWitness,
  assertClosedObject,
  providerWireRequestCommitment,
} from './provider-core'

type ResourceOperation =
  | 'read'
  | 'replace_body'
  | 'append_comment'
  | 'post_comment'
  | 'assign_task'
  | 'manage'

interface GroundedOutputItem {
  resource_id: Uuid | null
  principal_id: Uuid | null
  tool: string | null
  action: null
}

interface StructuredLanguageOutput {
  items: GroundedOutputItem[]
  max_observed_nesting_depth: number
}

interface ProxyPlanningEnvelope {
  language_task: ClaimedLanguageInvocation['language_task']
  request_id: Uuid
  user: Uuid
  candidate_resources: Uuid[]
  candidate_operations: ResourceOperation[]
  available_tools: string[]
  max_plan_steps: number
}

interface DecryptedInterrogationInput {
  kind: 'answer_from_authorized_context'
  session_id: Uuid
  question: string
  instructions: string
}

interface DecryptedProxyInput {
  kind: 'interpret_proxy_request'
  thread_id: Uuid
  instructions: string
  envelope: ProxyPlanningEnvelope
}

export type DecryptedLanguageInput = DecryptedInterrogationInput | DecryptedProxyInput

export interface EdgeLanguageCrypto {
  decryptInvocationInput(payload: EncryptedPayloadDto): Promise<DecryptedLanguageInput>
  resolveAuthorizedSources(sources: InformationSource[]): Promise<Array<{
    descriptor: InformationSource
    plaintext: string
  }>>
  encryptOutput(plaintext: string): Promise<EncryptedPayloadDto>
}

export interface AgentLanguageTransport {
  claim(
    projectId: Uuid,
    agentId: Uuid,
    executionProfileCommitmentHex: string,
    signal?: AbortSignal,
  ): Promise<ClaimedLanguageInvocation | null>
  submit(
    projectId: Uuid,
    agentId: Uuid,
    invocationId: Uuid,
    request: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<unknown>
  fail(
    projectId: Uuid,
    agentId: Uuid,
    invocationId: Uuid,
    request: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<unknown>
}

export class ApiAgentLanguageTransport implements AgentLanguageTransport {
  constructor(private readonly api: ApiClient) {}

  claim(
    projectId: Uuid,
    agentId: Uuid,
    executionProfileCommitmentHex: string,
    signal?: AbortSignal,
  ): Promise<ClaimedLanguageInvocation | null> {
    return this.api.request(`/v1/projects/${projectId}/agents/${agentId}/runner/client-provider/claim`, {
      method: 'POST',
      body: { execution_profile_commitment_hex: executionProfileCommitmentHex },
      signal,
    })
  }

  submit(
    projectId: Uuid,
    agentId: Uuid,
    invocationId: Uuid,
    request: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<unknown> {
    return this.api.request(
      `/v1/projects/${projectId}/agents/${agentId}/invocations/${invocationId}/submit`,
      { method: 'POST', body: request, signal },
    )
  }

  fail(
    projectId: Uuid,
    agentId: Uuid,
    invocationId: Uuid,
    request: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<unknown> {
    return this.api.request(
      `/v1/projects/${projectId}/agents/${agentId}/invocations/${invocationId}/fail`,
      { method: 'POST', body: request, signal },
    )
  }
}

const ANSWER_SCHEMA: JsonSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['answer'],
  properties: { answer: { type: 'string', minLength: 1, maxLength: 32768 } },
}

const PROXY_SCHEMA: JsonSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['explanation', 'resource_effects', 'tool_invocations'],
  properties: {
    explanation: { type: 'string', maxLength: 32768 },
    resource_effects: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['operation', 'resource_id'],
        properties: {
          operation: { type: 'string' },
          resource_id: { type: 'string', format: 'uuid' },
        },
      },
    },
    tool_invocations: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['input_digest', 'required_effects', 'tool'],
        properties: {
          input_digest: { type: 'string' },
          required_effects: { type: 'array' },
          tool: { type: 'string' },
        },
      },
    },
  },
}

const noDuplicates = <T>(items: T[]): boolean => new Set(items.map((item) => JSON.stringify(item))).size === items.length

const validateInvocationEnvelope = (claim: ClaimedLanguageInvocation): void => {
  const task = claim.language_task
  if (
    claim.runtime_kind !== 'client_provider_v1' ||
    !['answer_from_authorized_context', 'interpret_proxy_request'].includes(task.kind) ||
    task.input_item_count > task.max_input_items ||
    task.max_output_items < 1 ||
    task.max_nesting_depth < 1 ||
    task.max_attempts < 1 ||
    !task.closed_output_schema ||
    !task.grounded_identifiers_only ||
    task.requires_formal_proof ||
    task.requires_permission_decision ||
    task.requires_exact_semantic_equivalence ||
    task.requires_exhaustive_world_knowledge ||
    !noDuplicates(claim.sources)
  ) {
    throw new ProviderFailure('invalid_output', 'Invocation envelope is not supported', false)
  }
}

const providerRequest = (
  claim: ClaimedLanguageInvocation,
  input: DecryptedLanguageInput,
  sources: Array<{ descriptor: InformationSource; plaintext: string }>,
  model: string,
  timeoutMs: number,
  maxOutputTokens: number,
): ProviderGenerationRequest => ({
  task: claim.language_task.kind,
  model,
  instructions: input.instructions,
  sources,
  input,
  outputSchema: input.kind === 'answer_from_authorized_context' ? ANSWER_SCHEMA : PROXY_SCHEMA,
  preferences: {
    timeoutMs,
    maxOutputTokens,
    maxAttempts: claim.language_task.max_attempts,
  },
})

const expectString = (value: unknown, name: string): string => {
  if (typeof value !== 'string' || !value.trim()) {
    throw new ProviderFailure('invalid_output', `${name} must be a non-empty string`, true)
  }
  return value
}

const buildArtifact = async (
  claim: ClaimedLanguageInvocation,
  input: DecryptedLanguageInput,
  result: ProviderGenerationResult,
  cryptoBoundary: EdgeLanguageCrypto,
): Promise<{
  artifact: Record<string, unknown>
  structuredOutput: StructuredLanguageOutput
  encryptedOutput: EncryptedPayloadDto
}> => {
  const candidate = result.value as Record<string, unknown>
  const encryptedOutput = await cryptoBoundary.encryptOutput(JSON.stringify(candidate))
  if (input.kind === 'answer_from_authorized_context') {
    assertClosedObject(candidate, ['answer'])
    const answer = expectString(candidate.answer, 'answer')
    return {
      encryptedOutput,
      structuredOutput: { items: [], max_observed_nesting_depth: 1 },
      artifact: {
        kind: 'interrogation_answer',
        session_id: input.session_id,
        encrypted_answer: await cryptoBoundary.encryptOutput(answer),
        context_sources: claim.sources,
      },
    }
  }

  assertClosedObject(candidate, ['explanation', 'resource_effects', 'tool_invocations'])
  const effects = candidate.resource_effects
  const tools = candidate.tool_invocations
  if (!Array.isArray(effects) || !Array.isArray(tools) || typeof candidate.explanation !== 'string') {
    throw new ProviderFailure('invalid_output', 'Invalid proxy plan shape', true)
  }
  if (effects.length + tools.length > input.envelope.max_plan_steps) {
    throw new ProviderFailure('invalid_output', 'Proxy plan exceeds its bound', true)
  }
  const groundedItems: GroundedOutputItem[] = []
  for (const effect of effects) {
    if (!effect || typeof effect !== 'object' || Array.isArray(effect)) {
      throw new ProviderFailure('invalid_output', 'Invalid resource effect', true)
    }
    const item = effect as Record<string, unknown>
    assertClosedObject(item, ['operation', 'resource_id'])
    if (
      typeof item.resource_id !== 'string' ||
      typeof item.operation !== 'string' ||
      !input.envelope.candidate_resources.includes(item.resource_id) ||
      !input.envelope.candidate_operations.includes(item.operation as ResourceOperation)
    ) {
      throw new ProviderFailure('invalid_output', 'Proxy effect is outside the candidate envelope', true)
    }
    groundedItems.push({ resource_id: item.resource_id, principal_id: null, tool: null, action: null })
  }
  for (const tool of tools) {
    if (!tool || typeof tool !== 'object' || Array.isArray(tool)) {
      throw new ProviderFailure('invalid_output', 'Invalid tool invocation', true)
    }
    const item = tool as Record<string, unknown>
    assertClosedObject(item, ['input_digest', 'required_effects', 'tool'])
    if (typeof item.tool !== 'string' || !input.envelope.available_tools.includes(item.tool)) {
      throw new ProviderFailure('invalid_output', 'Proxy tool is outside the candidate envelope', true)
    }
    groundedItems.push({ resource_id: null, principal_id: null, tool: item.tool, action: null })
  }
  if (groundedItems.length > claim.language_task.max_output_items) {
    throw new ProviderFailure('invalid_output', 'Proxy output exceeds language task bounds', true)
  }
  return {
    encryptedOutput,
    structuredOutput: { items: groundedItems, max_observed_nesting_depth: 1 },
    artifact: {
      kind: 'user_proxy_plan',
      envelope: input.envelope,
      plan: {
        request_id: input.envelope.request_id,
        thread_id: input.thread_id,
        user: input.envelope.user,
        intent_id: crypto.randomUUID(),
        resource_effects: effects,
        tool_invocations: tools,
        encrypted_explanation: await cryptoBoundary.encryptOutput(candidate.explanation),
      },
    },
  }
}

const outputCommitment = async (
  structuredOutput: StructuredLanguageOutput,
  encryptedOutput: EncryptedPayloadDto,
): Promise<string> =>
  sha256Hex(
    JSON.stringify({
      structured_output: structuredOutput,
      encrypted_output: encryptedOutput,
      effects: [],
    }),
  )

const signatureJson = (bytes: Uint8Array): number[] => Array.from(bytes)

const signedObservation = async (
  claim: ClaimedLanguageInvocation,
  signer: DeviceObservationSigner,
  providerStatus: string,
  endpointRequestCommitmentHex: string | undefined,
  outputCommitmentHex: string | undefined,
  artifactCommitmentHex: string | undefined,
  executionProfileCommitmentHex: string,
): Promise<Record<string, unknown>> => {
  const statement = {
    observation_id: crypto.randomUUID(),
    dispatch_id: claim.dispatch_id,
    invocation_id: claim.id,
    attempt: claim.attempt,
    lease_id: claim.lease_id,
    principal_identity_id: claim.context_principal_identity_id,
    exposed_sources: claim.sources,
    request_commitment_hex: claim.request_commitment_hex,
    context_commitment_hex: claim.context_commitment_hex,
    transport_commitment_hex: claim.transport_commitment_hex,
    endpoint_request_commitment_hex: endpointRequestCommitmentHex,
    endpoint_request_exact: endpointRequestCommitmentHex !== undefined,
    runtime_kind: 'client_provider_v1',
    execution_profile_commitment_hex: executionProfileCommitmentHex,
    output_commitment_hex: outputCommitmentHex,
    artifact_commitment_hex: artifactCommitmentHex,
    provider_status: providerStatus,
    hidden_persistent_model_memory_available: false,
    idempotency_key: crypto.randomUUID(),
    observed_at: new Date().toISOString(),
  }
  const signatures = await signer.sign(
    canonicalGovernanceJson(statement),
    MODEL_OBSERVATION_SIGNATURE_CONTEXT,
  )
  return {
    statement,
    signatures: {
      signer_identity_id: signer.identityId,
      signer_device_id: signer.deviceId,
      signer_device_key_version: signer.keyVersion,
      classical_signature: signatureJson(signatures.classicalSignature),
      post_quantum_signature: signatureJson(signatures.postQuantumSignature),
    },
  }
}

export interface RunEdgeInvocationOptions {
  projectId: Uuid
  agentId: Uuid
  model: string
  timeoutMs: number
  maxOutputTokens: number
  transport: AgentLanguageTransport
  provider: ProviderAdapter
  crypto: EdgeLanguageCrypto
  signer: DeviceObservationSigner
  /** Hiding, device-generated commitment. Provider/model/URL never leave the edge. */
  executionProfileCommitmentHex: string
  signal?: AbortSignal
}

export const runOneClientOwnedInvocation = async (
  options: RunEdgeInvocationOptions,
): Promise<'idle' | 'succeeded' | 'failed'> => {
  const claim = await options.transport.claim(
    options.projectId,
    options.agentId,
    options.executionProfileCommitmentHex,
    options.signal,
  )
  if (!claim) return 'idle'
  validateInvocationEnvelope(claim)
  let endpointCommitment: string | undefined
  try {
    const input = await options.crypto.decryptInvocationInput(claim.encrypted_input)
    if (input.kind !== claim.language_task.kind) {
      throw new ProviderFailure('invalid_output', 'Decrypted task kind does not match dispatch', false)
    }
    const sources = await options.crypto.resolveAuthorizedSources(claim.sources)
    if (
      sources.length !== claim.sources.length ||
      sources.some((source, index) => JSON.stringify(source.descriptor) !== JSON.stringify(claim.sources[index]))
    ) {
      throw new ProviderFailure('invalid_output', 'Resolved source projection is not list-exact', false)
    }
    const providerInput = providerRequest(
      claim,
      input,
      sources,
      options.model,
      options.timeoutMs,
      options.maxOutputTokens,
    )
    // One provider request per Sprout dispatch. A retry is a new persisted 0031
    // provider attempt/lease, never an internal loop hidden inside this attempt.
    const result = await options.provider.generateStructured(
      { ...providerInput, preferences: { ...providerInput.preferences, maxAttempts: 1 } },
      options.signal,
    )
    assertExactWireWitness(providerInput, result.wireWitness)
    endpointCommitment = await providerWireRequestCommitment(result.wireWitness)
    if (result.actualRequestCommitmentHex !== endpointCommitment) {
      throw new ProviderFailure('invalid_output', 'Actual provider request differs from projection', false)
    }
    const output = await buildArtifact(claim, input, result, options.crypto)
    const outputHash = await outputCommitment(output.structuredOutput, output.encryptedOutput)
    const artifactHash = await sha256Hex(canonicalGovernanceJson(output.artifact))
    const observation = await signedObservation(
      claim,
      options.signer,
      result.sanitizedStatus,
      endpointCommitment,
      outputHash,
      artifactHash,
      options.executionProfileCommitmentHex,
    )
    await options.transport.submit(
      options.projectId,
      options.agentId,
      claim.id,
      {
        lease_id: claim.lease_id,
        structured_output: output.structuredOutput,
        encrypted_output: output.encryptedOutput,
        effects: [],
        artifact: output.artifact,
        endpoint_request_commitment_hex: endpointCommitment,
        endpoint_request_exact: true,
        runtime_kind: 'client_provider_v1',
        execution_profile_commitment_hex: options.executionProfileCommitmentHex,
        observation,
      },
      options.signal,
    )
    return 'succeeded'
  } catch (error) {
    const failure = error instanceof ProviderFailure ? error : new ProviderFailure('unavailable', 'Local execution failed', false)
    if (failure.wireWitness) {
      endpointCommitment = await providerWireRequestCommitment(failure.wireWitness)
    }
    const failureCode =
      failure.code === 'timeout'
        ? 'provider_timeout'
        : failure.code === 'invalid_output' && endpointCommitment
          ? 'invalid_structured_output'
          : failure.code === 'rate_limited' && endpointCommitment
            ? 'provider_unavailable'
            : failure.code === 'unavailable' && endpointCommitment
              ? 'provider_unavailable'
              : 'local_execution_failed'
    const observation = await signedObservation(
      claim,
      options.signer,
      failureCode,
      endpointCommitment,
      undefined,
      undefined,
      options.executionProfileCommitmentHex,
    )
    await options.transport.fail(
      options.projectId,
      options.agentId,
      claim.id,
      {
        lease_id: claim.lease_id,
        failure_code: failureCode,
        retryable: failure.retryable,
        endpoint_request_commitment_hex: endpointCommitment,
        endpoint_request_exact: endpointCommitment !== undefined,
        runtime_kind: 'client_provider_v1',
        execution_profile_commitment_hex: options.executionProfileCommitmentHex,
        observation,
      },
      options.signal,
    )
    return 'failed'
  }
}
