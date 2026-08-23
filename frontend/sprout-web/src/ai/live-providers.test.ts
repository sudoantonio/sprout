import { describe, expect, it } from 'vitest'
import type { EncryptedPayloadDto } from '../api/contracts'
import type {
  ClaimedLanguageInvocation,
  DeviceObservationSigner,
  ProviderGenerationRequest,
} from './contracts'
import { runOneClientOwnedInvocation, type AgentLanguageTransport } from './edge-runtime'
import { Ds4Provider, OllamaProvider, OpenAiCompatibleProvider } from './providers'

const environment =
  (globalThis as unknown as { process?: { env?: Record<string, string | undefined> } })
    .process?.env ?? {}

const liveRequest = (model: string): ProviderGenerationRequest => ({
  task: 'answer_from_authorized_context',
  model,
  instructions: 'Return exactly one JSON object matching the supplied schema. Keep the answer short.',
  sources: [
    {
      descriptor: { kind: 'resource_body', resource_id: '00000000-0000-4000-8000-000000000001' },
      plaintext: 'The authorized answer is green.',
    },
  ],
  input: { question: 'What is the authorized answer?' },
  outputSchema: {
    type: 'object',
    additionalProperties: false,
    required: ['answer'],
    properties: { answer: { type: 'string' } },
  },
  preferences: { timeoutMs: 60_000, maxOutputTokens: 64, maxAttempts: 1, temperature: 0 },
})

const liveEncrypted = (seed: string): EncryptedPayloadDto => ({
  version: 1,
  algorithm: 'xchacha20poly1305',
  key_id: '00000000-0000-4000-8000-000000000001',
  nonce_b64: btoa(`${seed}-nonce`),
  ciphertext_b64: btoa(`${seed}-ciphertext`),
})

const liveClaim = (
  kind: 'answer_from_authorized_context' | 'interpret_proxy_request',
): ClaimedLanguageInvocation => ({
  id: crypto.randomUUID(),
  dispatch_id: crypto.randomUUID(),
  lease_id: crypto.randomUUID(),
  lease_expires_at: new Date(Date.now() + 60_000).toISOString(),
  attempt: 1,
  language_task: {
    id: crypto.randomUUID(),
    kind,
    input_item_count: 1,
    max_input_items: 1,
    max_output_items: 1,
    max_nesting_depth: 2,
    max_attempts: 2,
    closed_output_schema: true,
    grounded_identifiers_only: true,
    requires_formal_proof: false,
    requires_permission_decision: false,
    requires_exact_semantic_equivalence: false,
    requires_exhaustive_world_knowledge: false,
    allowed_resource_ids: ['00000000-0000-4000-8000-000000000001'],
    allowed_principal_ids: ['00000000-0000-4000-8000-000000000005'],
    allowed_tools: [],
  },
  authority_envelope: {},
  sources: [{ kind: 'resource_body', resource_id: '00000000-0000-4000-8000-000000000001' }],
  encrypted_input: liveEncrypted('input'),
  context_principal_identity_id: '00000000-0000-4000-8000-000000000005',
  request_commitment_hex: '11'.repeat(32),
  context_commitment_hex: '22'.repeat(32),
  transport_commitment_hex: '33'.repeat(32),
  runtime_kind: 'client_provider_v1',
})

const liveSigner: DeviceObservationSigner = {
  identityId: '00000000-0000-4000-8000-000000000005',
  deviceId: '00000000-0000-4000-8000-000000000006',
  keyVersion: 1,
  sign: async () => ({
    classicalSignature: new Uint8Array(64).fill(1),
    postQuantumSignature: new Uint8Array(128).fill(2),
  }),
}

const runLiveEdgeTask = async (
  adapter: OpenAiCompatibleProvider,
  model: string,
  kind: 'answer_from_authorized_context' | 'interpret_proxy_request',
): Promise<Record<string, unknown>> => {
  const claim = liveClaim(kind)
  let claimed = false
  let submitted: Record<string, unknown> | undefined
  const transport: AgentLanguageTransport = {
    claim: async () => {
      if (claimed) return null
      claimed = true
      return claim
    },
    submit: async (_project, _agent, _invocation, request) => {
      submitted = request
    },
    fail: async (_project, _agent, _invocation, request) => {
      throw new Error(`Live edge task failed: ${JSON.stringify(request)}`)
    },
  }
  const requestId = crypto.randomUUID()
  const threadId = crypto.randomUUID()
  await expect(
    runOneClientOwnedInvocation({
      projectId: '00000000-0000-4000-8000-000000000003',
      agentId: '00000000-0000-4000-8000-000000000004',
      model,
      timeoutMs: 60_000,
      maxOutputTokens: 128,
      transport,
      provider: adapter,
      signer: liveSigner,
      executionProfileCommitmentHex: '66'.repeat(32),
      crypto: {
        decryptInvocationInput: async () =>
          kind === 'answer_from_authorized_context'
            ? {
                kind,
                session_id: crypto.randomUUID(),
                question: 'What is the authorized answer?',
                instructions: 'Return JSON only: {"answer":"green"}.',
              }
            : {
                kind,
                thread_id: threadId,
                instructions:
                  'Return JSON only with explanation, one read resource_effect, and empty tool_invocations.',
                envelope: {
                  language_task: claim.language_task,
                  request_id: requestId,
                  user: liveSigner.identityId,
                  candidate_resources: ['00000000-0000-4000-8000-000000000001'],
                  candidate_operations: ['read'],
                  available_tools: [],
                  max_plan_steps: 1,
                },
              },
        resolveAuthorizedSources: async () => [{
          descriptor: claim.sources[0],
          plaintext: 'The only authorized answer and resource is green.',
        }],
        encryptOutput: async (plaintext) => liveEncrypted(String(plaintext.length)),
      },
    }),
  ).resolves.toBe('succeeded')
  expect(submitted).toBeDefined()
  expect(submitted?.runtime_kind).toBe('client_provider_v1')
  expect(submitted?.endpoint_request_exact).toBe(true)
  expect(submitted?.endpoint_request_commitment_hex).toMatch(/^[a-f0-9]{64}$/)
  return submitted!
}

describe.runIf(environment.DEEPSEEK_LIVE_TESTS === '1')('DeepSeek primary live cloud', () => {
  it('discovers and uses the exact DeepSeek V4 Flash model with structured output', async () => {
    const key = environment.DEEPSEEK_API_KEY
    const model = environment.DEEPSEEK_TEST_MODEL
    if (!key || !model) throw new Error('DeepSeek live variables are incomplete')
    const adapter = new OpenAiCompatibleProvider(
      'https://api.deepseek.com',
      key,
      false,
      'deepseek_chat_v4',
      'json_object',
      true,
    )
    const models = await adapter.discoverModels()
    expect(models.map((entry) => entry.id)).toContain(model)
    const result = await adapter.generateStructured(liveRequest(model))
    expect(result.value).toEqual(expect.objectContaining({ answer: expect.any(String) }))
    expect(result.actualRequestCommitmentHex).toMatch(/^[a-f0-9]{64}$/)
  })

  it('executes both certified language tasks through the live edge boundary', async () => {
    const key = environment.DEEPSEEK_API_KEY
    const model = environment.DEEPSEEK_TEST_MODEL
    if (!key || !model) throw new Error('DeepSeek live variables are incomplete')
    const adapter = new OpenAiCompatibleProvider(
      'https://api.deepseek.com',
      key,
      false,
      'deepseek_chat_v4',
      'json_object',
      true,
    )
    const answer = await runLiveEdgeTask(adapter, model, 'answer_from_authorized_context')
    const proxy = await runLiveEdgeTask(adapter, model, 'interpret_proxy_request')
    expect((answer.artifact as Record<string, unknown>).kind).toBe('interrogation_answer')
    expect((proxy.artifact as Record<string, unknown>).kind).toBe('user_proxy_plan')
  })
})

describe.runIf(environment.DS4_LAN_LIVE_TESTS === '1')('DS4 LAN development live feature', () => {
  it('discovers the configured exact model and performs strict structured generation', async () => {
    const baseUrl = environment.DS4_TEST_BASE_URL
    const model = environment.DS4_TEST_MODEL
    if (!baseUrl || !model) throw new Error('DS4 live variables are incomplete')
    const adapter = new Ds4Provider(baseUrl, environment.DS4_TEST_TOKEN)
    const discovered = await adapter.discoverModels()
    const exactModel = discovered.find((entry) => entry.id === model)
    expect(exactModel).toBeDefined()
    expect(exactModel?.supportedParameters).not.toContain('response_format')
    const result = await adapter.generateStructured(liveRequest(model))
    expect(result.value).toEqual(
      expect.objectContaining({ answer: expect.any(String) }),
    )
    expect(result.wireWitness).toMatchObject({
      protocol: 'ds4_openai_chat_v1',
      path: '/v1/chat/completions',
      selectedModel: model,
    })
    expect(result.wireWitness.body).not.toContain('response_format')
    expect(result.actualRequestCommitmentHex).toMatch(/^[a-f0-9]{64}$/)

    const submission = await runLiveEdgeTask(adapter, model, 'answer_from_authorized_context')
    expect(JSON.stringify(submission)).not.toContain(baseUrl)
    expect(JSON.stringify(submission)).not.toContain(model)
  })

  it('honors cancellation and preserves a wire witness for a real timeout', async () => {
    const baseUrl = environment.DS4_TEST_BASE_URL
    const model = environment.DS4_TEST_MODEL
    if (!baseUrl || !model) throw new Error('DS4 live variables are incomplete')
    const adapter = new Ds4Provider(baseUrl, environment.DS4_TEST_TOKEN)
    expect((await adapter.discoverModels()).some((entry) => entry.id === model)).toBe(true)
    const cancelled = new AbortController()
    cancelled.abort()
    await expect(adapter.generateStructured(liveRequest(model), cancelled.signal)).rejects.toMatchObject({
      code: 'cancelled',
    })
    const timeoutRequest = {
      ...liveRequest(model),
      preferences: { ...liveRequest(model).preferences, timeoutMs: 1 },
    }
    const timeout = await adapter
      .generateStructured(timeoutRequest)
      .catch((error: unknown) => error)
    expect(timeout).toMatchObject({
      code: 'timeout',
      wireWitness: {
        protocol: 'ds4_openai_chat_v1',
        path: '/v1/chat/completions',
        selectedModel: model,
      },
    })
  })
})

describe.runIf(environment.OLLAMA_LIVE_TESTS === '1')('Ollama local live feature', () => {
  it('discovers qwen2.5:0.5b-instruct and performs stateless structured generation', async () => {
    const baseUrl = environment.OLLAMA_TEST_BASE_URL
    const model = environment.OLLAMA_TEST_MODEL
    if (!baseUrl || !model) throw new Error('Ollama live variables are incomplete')
    const adapter = new OllamaProvider(baseUrl)
    expect((await adapter.discoverModels()).map((entry) => entry.id)).toContain(model)
    const result = await adapter.generateStructured(liveRequest(model))
    expect(result.value).toEqual(expect.objectContaining({ answer: expect.any(String) }))
  })

  it('honors cancellation/timeout without persisting provider session memory', async () => {
    const baseUrl = environment.OLLAMA_TEST_BASE_URL
    const model = environment.OLLAMA_TEST_MODEL
    if (!baseUrl || !model) throw new Error('Ollama live variables are incomplete')
    const adapter = new OllamaProvider(baseUrl)
    const controller = new AbortController()
    controller.abort()
    await expect(adapter.generateStructured(liveRequest(model), controller.signal)).rejects.toMatchObject({
      code: 'cancelled',
    })
  })
})
