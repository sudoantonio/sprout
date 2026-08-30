import { describe, expect, it, vi } from 'vitest'
import type { EncryptedPayloadDto } from '../api/contracts'
import { ProviderFailure, resultFromRaw } from './provider-core'
import { runOneClientOwnedInvocation, type AgentLanguageTransport } from './edge-runtime'
import type {
  ClaimedLanguageInvocation,
  DeviceObservationSigner,
  ProviderAdapter,
} from './contracts'

const resourceId = '00000000-0000-4000-8000-000000000001'
const invocationId = '00000000-0000-4000-8000-000000000002'
const projectId = '00000000-0000-4000-8000-000000000003'
const agentId = '00000000-0000-4000-8000-000000000004'
const identityId = '00000000-0000-4000-8000-000000000005'
const sessionId = '00000000-0000-4000-8000-000000000006'

const encrypted = (seed: string): EncryptedPayloadDto => ({
  version: 1,
  algorithm: 'xchacha20poly1305',
  key_id: resourceId,
  nonce_b64: btoa(`${seed}-nonce`),
  ciphertext_b64: btoa(`${seed}-ciphertext`),
})

const claim = (): ClaimedLanguageInvocation => ({
  id: invocationId,
  dispatch_id: '00000000-0000-4000-8000-000000000007',
  lease_id: '00000000-0000-4000-8000-000000000008',
  lease_expires_at: new Date(Date.now() + 60_000).toISOString(),
  attempt: 1,
  language_task: {
    id: '00000000-0000-4000-8000-000000000009',
    kind: 'answer_from_authorized_context',
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
    allowed_resource_ids: [resourceId],
    allowed_principal_ids: [identityId],
    allowed_tools: [],
  },
  authority_envelope: {},
  sources: [{ kind: 'resource_body', resource_id: resourceId }],
  encrypted_input: encrypted('input'),
  context_principal_identity_id: identityId,
  request_commitment_hex: '11'.repeat(32),
  context_commitment_hex: '22'.repeat(32),
  transport_commitment_hex: '33'.repeat(32),
  runtime_kind: 'client_provider_v1',
})

const signer: DeviceObservationSigner = {
  identityId,
  deviceId: '00000000-0000-4000-8000-000000000010',
  keyVersion: 1,
  sign: vi.fn(async () => ({
    classicalSignature: new Uint8Array(64).fill(1),
    postQuantumSignature: new Uint8Array(128).fill(2),
  })),
}

const cryptoBoundary = {
  decryptInvocationInput: async () => ({
    kind: 'answer_from_authorized_context' as const,
    session_id: sessionId,
    question: 'What is grounded?',
    instructions: 'Return the answer as strict JSON.',
  }),
  resolveAuthorizedSources: async () => [
    {
      descriptor: { kind: 'resource_body' as const, resource_id: resourceId },
      plaintext: 'Only this current authorized source.',
    },
  ],
  encryptOutput: async (plaintext: string) => encrypted(String(plaintext.length)),
}

describe('client-owned exact invocation runner', () => {
  it('submits a signed exact projection without provider/config/secret/plaintext server fields', async () => {
    const submitted: Record<string, unknown>[] = []
    const transport: AgentLanguageTransport = {
      claim: async () => claim(),
      submit: async (_project, _agent, _invocation, request) => {
        submitted.push(request)
      },
      fail: vi.fn(),
    }
    const provider: ProviderAdapter = {
      capabilities: { modelDiscovery: true, structuredOutput: true, cancellation: true },
      discoverModels: async () => [{ id: 'private-model' }],
      generateStructured: async (request) => {
        const body = JSON.stringify({ model: request.model, input: request.input })
        return resultFromRaw(request, '{"answer":"grounded"}', 1, 'succeeded', {
          protocol: 'openai_chat_completions_v1',
          method: 'POST',
          path: '/v1/chat/completions',
          selectedModel: request.model,
          body,
        })
      },
    }

    await expect(
      runOneClientOwnedInvocation({
        projectId,
        agentId,
        model: 'private-model',
        timeoutMs: 1000,
        maxOutputTokens: 64,
        transport,
        provider,
        crypto: cryptoBoundary,
        signer,
        executionProfileCommitmentHex: '66'.repeat(32),
      }),
    ).resolves.toBe('succeeded')
    expect(submitted).toHaveLength(1)
    const serialized = JSON.stringify(submitted[0])
    expect(serialized).not.toContain('private-model')
    expect(serialized).not.toContain('Only this current authorized source')
    expect(serialized).not.toContain('grounded')
    expect(serialized).not.toContain('api_key')
    expect(serialized).not.toContain('base_url')
    const observation = submitted[0].observation as Record<string, unknown>
    const statement = observation.statement as Record<string, unknown>
    expect(statement.endpoint_request_commitment_hex).toMatch(/^[a-f0-9]{64}$/)
    expect(statement.exposed_sources).toEqual(claim().sources)
    expect(statement.hidden_persistent_model_memory_available).toBe(false)
    expect(statement.runtime_kind).toBe('client_provider_v1')
    expect(statement.execution_profile_commitment_hex).toBe('66'.repeat(32))
    expect(signer.sign).toHaveBeenCalledOnce()
  })

  it('fails closed when source resolution changes order or membership', async () => {
    const fail = vi.fn(async () => undefined)
    const transport: AgentLanguageTransport = {
      claim: async () => claim(),
      submit: vi.fn(),
      fail,
    }
    await expect(
      runOneClientOwnedInvocation({
        projectId,
        agentId,
        model: 'model',
        timeoutMs: 1000,
        maxOutputTokens: 64,
        transport,
        provider: {
          capabilities: { modelDiscovery: false, structuredOutput: true, cancellation: true },
          discoverModels: async () => [],
          generateStructured: vi.fn(),
        },
        crypto: { ...cryptoBoundary, resolveAuthorizedSources: async () => [] },
        signer,
        executionProfileCommitmentHex: '66'.repeat(32),
      }),
    ).resolves.toBe('failed')
    expect(fail).toHaveBeenCalledOnce()
    const preRequestFailure = (fail.mock.calls as unknown[][])[0][3] as Record<string, unknown>
    expect(preRequestFailure.failure_code).toBe('local_execution_failed')
    expect(preRequestFailure.endpoint_request_exact).toBe(false)
    expect(preRequestFailure.endpoint_request_commitment_hex).toBeUndefined()
  })

  it('rejects actual/provider projection mismatch instead of accepting output', async () => {
    const submit = vi.fn()
    const fail = vi.fn(async () => undefined)
    const transport: AgentLanguageTransport = {
      claim: async () => claim(),
      submit,
      fail,
    }
    await expect(
      runOneClientOwnedInvocation({
        projectId,
        agentId,
        model: 'model',
        timeoutMs: 1000,
        maxOutputTokens: 64,
        transport,
        provider: {
          capabilities: { modelDiscovery: false, structuredOutput: true, cancellation: true },
          discoverModels: async () => [],
          generateStructured: async () => ({
            value: { answer: 'forged' },
            attemptCount: 1,
            sanitizedStatus: 'succeeded',
            wireWitness: {
              protocol: 'openai_chat_completions_v1',
              method: 'POST',
              path: '/v1/chat/completions',
              selectedModel: 'model',
              body: '{"model":"model","input":"forged"}',
            },
            actualRequestCommitmentHex: 'ff'.repeat(32),
            actualOutputCommitmentHex: 'ee'.repeat(32),
          }),
        },
        crypto: cryptoBoundary,
        signer,
        executionProfileCommitmentHex: '66'.repeat(32),
      }),
    ).resolves.toBe('failed')
    expect(submit).not.toHaveBeenCalled()
    expect(fail).toHaveBeenCalledOnce()
  })

  it('persists each real provider retry as a distinct Sprout attempt and keeps prior witnesses', async () => {
    let claimOrdinal = 0
    let providerCalls = 0
    const failures: Record<string, unknown>[] = []
    const submissions: Record<string, unknown>[] = []
    const transport: AgentLanguageTransport = {
      claim: async () => {
        claimOrdinal += 1
        if (claimOrdinal > 2) return null
        return {
          ...claim(),
          attempt: claimOrdinal,
          dispatch_id: `00000000-0000-4000-8000-00000000000${claimOrdinal + 6}`,
          lease_id: `00000000-0000-4000-8000-00000000001${claimOrdinal}`,
        }
      },
      submit: async (_project, _agent, _invocation, request) => submissions.push(request),
      fail: async (_project, _agent, _invocation, request) => failures.push(request),
    }
    const provider: ProviderAdapter = {
      capabilities: { modelDiscovery: false, structuredOutput: true, cancellation: true },
      discoverModels: async () => [],
      generateStructured: async (request) => {
        providerCalls += 1
        const witness = {
          protocol: 'openai_chat_completions_v1' as const,
          method: 'POST' as const,
          path: '/v1/chat/completions',
          selectedModel: request.model,
          body: JSON.stringify({ model: request.model, attempt: providerCalls }),
        }
        if (providerCalls === 1) {
          throw new ProviderFailure('timeout', 'timeout', true, witness)
        }
        return resultFromRaw(request, '{"answer":"after retry"}', 1, 'succeeded', witness)
      },
    }
    const options = {
      projectId,
      agentId,
      model: 'model',
      timeoutMs: 1000,
      maxOutputTokens: 64,
      transport,
      provider,
      crypto: cryptoBoundary,
      signer,
      executionProfileCommitmentHex: '66'.repeat(32),
    }
    await expect(runOneClientOwnedInvocation(options)).resolves.toBe('failed')
    await expect(runOneClientOwnedInvocation(options)).resolves.toBe('succeeded')
    expect(providerCalls).toBe(2)
    expect(failures).toHaveLength(1)
    expect(submissions).toHaveLength(1)
    const failedObservation = failures[0].observation as Record<string, unknown>
    const successObservation = submissions[0].observation as Record<string, unknown>
    expect((failedObservation.statement as Record<string, unknown>).attempt).toBe(1)
    expect((successObservation.statement as Record<string, unknown>).attempt).toBe(2)
    expect((failedObservation.statement as Record<string, unknown>).endpoint_request_commitment_hex)
      .toMatch(/^[a-f0-9]{64}$/)
    expect((successObservation.statement as Record<string, unknown>).endpoint_request_commitment_hex)
      .toMatch(/^[a-f0-9]{64}$/)
  })

  it('does not call the provider again when exact replay has no new claim', async () => {
    let claimed = false
    const provider = {
      capabilities: { modelDiscovery: false, structuredOutput: true, cancellation: true },
      discoverModels: async () => [],
      generateStructured: vi.fn(async (request) => {
        const body = JSON.stringify({ model: request.model })
        return resultFromRaw(request, '{"answer":"once"}', 1, 'succeeded', {
          protocol: 'openai_chat_completions_v1',
          method: 'POST',
          path: '/v1/chat/completions',
          selectedModel: request.model,
          body,
        })
      }),
    } satisfies ProviderAdapter
    const transport: AgentLanguageTransport = {
      claim: async () => {
        if (claimed) return null
        claimed = true
        return claim()
      },
      submit: async () => undefined,
      fail: async () => undefined,
    }
    const options = {
      projectId,
      agentId,
      model: 'model',
      timeoutMs: 1000,
      maxOutputTokens: 64,
      transport,
      provider,
      crypto: cryptoBoundary,
      signer,
      executionProfileCommitmentHex: '66'.repeat(32),
    }
    await expect(runOneClientOwnedInvocation(options)).resolves.toBe('succeeded')
    await expect(runOneClientOwnedInvocation(options)).resolves.toBe('idle')
    expect(provider.generateStructured).toHaveBeenCalledOnce()
  })
})
