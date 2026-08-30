import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ProviderGenerationRequest } from './contracts'
import { PROVIDER_PROTOCOL_SEMANTIC_HEADERS } from './contracts'
import {
  AnthropicCompatibleProvider,
  Ds4Provider,
  OllamaProvider,
  OpenAiCompatibleProvider,
} from './providers'
import {
  ProviderFailure,
  assertExactWireWitness,
  generateWithBoundedRetry,
  providerWireRequestCommitment,
  resultFromRaw,
  strictJsonObject,
} from './provider-core'

const request: ProviderGenerationRequest = {
  task: 'answer_from_authorized_context',
  model: 'selected-model',
  instructions: 'Return one answer.',
  sources: [
    {
      descriptor: { kind: 'resource_body', resource_id: '00000000-0000-4000-8000-000000000001' },
      plaintext: 'Authorized source',
    },
  ],
  input: { question: 'Question?' },
  outputSchema: {
    type: 'object',
    additionalProperties: false,
    required: ['answer'],
    properties: { answer: { type: 'string' } },
  },
  preferences: { timeoutMs: 1000, maxOutputTokens: 64, maxAttempts: 3 },
}

afterEach(() => vi.unstubAllGlobals())

describe('client-owned provider contracts', () => {
  it('fixes every non-secret semantic header through the committed protocol identity', () => {
    expect(PROVIDER_PROTOCOL_SEMANTIC_HEADERS).toEqual({
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
    })
  })

  it('discovers models directly and uses the exact selected model without session memory', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ data: [{ id: 'model-a' }, { id: 'selected-model' }] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({ choices: [{ message: { content: '{"answer":"ok"}' } }] }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
    vi.stubGlobal('fetch', fetchMock)
    const adapter = new OpenAiCompatibleProvider('https://provider.example', 'secret', false)
    expect(await adapter.discoverModels()).toEqual([{ id: 'model-a' }, { id: 'selected-model' }])
    await adapter.generateStructured(request)
    const generation = JSON.parse(String(fetchMock.mock.calls[1][1]?.body)) as Record<string, unknown>
    expect(generation.model).toBe('selected-model')
    expect(generation).not.toHaveProperty('previous_response_id')
    expect(generation).not.toHaveProperty('conversation')
    expect(generation).not.toHaveProperty('session_id')
  })

  it('uses separate Anthropic and Ollama protocols without leaking credentials into bodies', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ content: [{ type: 'text', text: '{"answer":"yes"}' }] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ message: { content: '{"answer":"local"}' } }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      )
    vi.stubGlobal('fetch', fetchMock)
    await new AnthropicCompatibleProvider('https://anthropic.example', 'anthropic-secret')
      .generateStructured(request)
    await new OllamaProvider('http://127.0.0.1:11434', 'local-token')
      .generateStructured(request)
    const anthropicBody = String(fetchMock.mock.calls[0][1]?.body)
    const ollamaBody = JSON.parse(String(fetchMock.mock.calls[1][1]?.body)) as Record<string, unknown>
    expect(anthropicBody).not.toContain('anthropic-secret')
    expect(JSON.stringify(ollamaBody)).not.toContain('local-token')
    expect(ollamaBody.keep_alive).toBe(0)
    expect(ollamaBody).not.toHaveProperty('session')
  })

  it.each([
    'http://host:8000',
    'http://host:8000/',
    'http://host:8000/v1',
    'http://host:8000/v1/',
  ])('normalizes DS4 OpenAI-compatible base %s without duplicating /v1', async (baseUrl) => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          data: [{
            id: 'selected-model',
            supported_parameters: ['max_tokens', 'reasoning_effort', 'temperature'],
          }],
        }), { status: 200, headers: { 'content-type': 'application/json' } }),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({ choices: [{ message: { content: '{"answer":"ok"}' } }] }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
    vi.stubGlobal('fetch', fetchMock)
    const adapter = new Ds4Provider(baseUrl)
    expect(await adapter.discoverModels()).toEqual([{
      id: 'selected-model',
      supportedParameters: ['max_tokens', 'reasoning_effort', 'temperature'],
    }])
    const result = await adapter.generateStructured(request)
    expect(String(fetchMock.mock.calls[0][0])).toBe('http://host:8000/v1/models')
    expect(String(fetchMock.mock.calls[1][0])).toBe('http://host:8000/v1/chat/completions')
    expect(String(fetchMock.mock.calls[0][0])).not.toContain('/v1/v1/')
    expect(String(fetchMock.mock.calls[1][0])).not.toContain('/v1/v1/')
    expect(result.wireWitness).toMatchObject({
      protocol: 'ds4_openai_chat_v1',
      path: '/v1/chat/completions',
      selectedModel: request.model,
    })
    const body = JSON.parse(result.wireWitness.body) as Record<string, unknown>
    expect(body).not.toHaveProperty('response_format')
    expect(body.reasoning_effort).toBe('none')
    expect(JSON.stringify(body.messages)).toContain('Return exactly one JSON object')
  })

  it('fails closed when DS4 discovery does not prove the exact request capabilities', async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValueOnce(
      new Response(JSON.stringify({
        data: [{ id: 'selected-model', supported_parameters: ['max_tokens'] }],
      }), { status: 200, headers: { 'content-type': 'application/json' } }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const adapter = new Ds4Provider('http://host:8000/v1')
    await adapter.discoverModels()
    await expect(adapter.generateStructured(request)).rejects.toMatchObject({
      code: 'unavailable',
      retryable: false,
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it.each(['', 'not-json', '{"answer":"ok"} trailing', '[]'])(
    'strictly rejects malformed or non-object output %j',
    (raw) => expect(() => strictJsonObject(raw)).toThrow(ProviderFailure),
  )

  it('bounds retry and persists no silent text fallback', async () => {
    let attempts = 0
    const adapter = {
      generateStructured: async () => {
        attempts += 1
        throw new ProviderFailure('invalid_output', 'bad schema', true)
      },
    }
    await expect(generateWithBoundedRetry(adapter, request)).rejects.toMatchObject({
      code: 'invalid_output',
    })
    expect(attempts).toBe(3)
  })

  it('maps abort and timeout without exposing provider error payloads', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn((_url: RequestInfo | URL, init?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => reject(init.signal?.reason))
        }),
      ),
    )
    const adapter = new OpenAiCompatibleProvider('https://provider.example', 'never-log-this', false)
    const failure = await adapter
      .generateStructured({ ...request, preferences: { ...request.preferences, timeoutMs: 10 } })
      .catch((error: unknown) => error as ProviderFailure)
    expect(failure).toMatchObject({ code: 'timeout' })
    expect(failure.wireWitness).toMatchObject({ selectedModel: request.model })
    expect(await providerWireRequestCommitment(failure.wireWitness!)).toMatch(/^[a-f0-9]{64}$/)
  })

  it('fails before transport when the caller signal is already cancelled', async () => {
    const fetchMock = vi.fn<typeof fetch>()
    vi.stubGlobal('fetch', fetchMock)
    const controller = new AbortController()
    controller.abort()
    const adapter = new OllamaProvider('http://127.0.0.1:11434')
    await expect(adapter.generateStructured(request, controller.signal)).rejects.toMatchObject({
      code: 'cancelled',
    })
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('preserves the exact request witness for malformed response and non-retryable auth failure', async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(new Response('not-json', { status: 200 }))
      .mockResolvedValueOnce(new Response('', { status: 401 }))
    vi.stubGlobal('fetch', fetchMock)
    const adapter = new OpenAiCompatibleProvider('https://provider.example', 'never-log-this', false)
    const malformed = await adapter.generateStructured(request).catch((error: unknown) => error as ProviderFailure)
    const auth = await adapter.generateStructured(request).catch((error: unknown) => error as ProviderFailure)
    expect(malformed).toMatchObject({ code: 'invalid_output', retryable: true })
    expect(auth).toMatchObject({ code: 'unavailable', retryable: false })
    expect(malformed.wireWitness?.body).toContain('selected-model')
    expect(auth.wireWitness?.body).toBe(malformed.wireWitness?.body)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('makes request and output commitments over the actual structured bytes', async () => {
    const body = JSON.stringify({ model: request.model, response_format: request.outputSchema })
    const result = await resultFromRaw(request, '{"answer":"ok"}', 1, 'provider 200', {
      protocol: 'openai_chat_completions_v1',
      method: 'POST',
      path: '/v1/chat/completions',
      selectedModel: request.model,
      body,
    })
    expect(result.actualRequestCommitmentHex).toMatch(/^[a-f0-9]{64}$/)
    expect(result.actualOutputCommitmentHex).toMatch(/^[a-f0-9]{64}$/)
    expect(result.sanitizedStatus).toBe('provider_200')
  })

  it('changes commitment when the exact wire body or selected model changes', async () => {
    const exact = {
      protocol: 'openai_chat_completions_v1' as const,
      method: 'POST' as const,
      path: '/v1/chat/completions',
      selectedModel: request.model,
      body: JSON.stringify({ model: request.model, messages: [] }),
    }
    const alteredBody = { ...exact, body: JSON.stringify({ model: request.model, messages: ['extra'] }) }
    const alteredModel = {
      ...exact,
      selectedModel: 'substituted-model',
      body: JSON.stringify({ model: 'substituted-model', messages: [] }),
    }
    expect(await providerWireRequestCommitment(alteredBody)).not.toBe(
      await providerWireRequestCommitment(exact),
    )
    expect(await providerWireRequestCommitment(alteredModel)).not.toBe(
      await providerWireRequestCommitment(exact),
    )
    expect(() => assertExactWireWitness(request, alteredModel)).toThrow(
      'does not bind the selected model',
    )
  })
})
