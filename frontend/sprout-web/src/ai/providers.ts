import type {
  CommercialProfile,
  LanProfile,
  ProviderAdapter,
  ProviderCapabilities,
  ProviderGenerationRequest,
  ProviderGenerationResult,
  ProviderModel,
  ProviderWireProtocol,
  ProviderWireRequestWitness,
} from './contracts'
import { PROVIDER_PROTOCOL_SEMANTIC_HEADERS } from './contracts'
import {
  ProviderFailure,
  requestPayload,
  preserveWireWitness,
  resultFromRaw,
  sanitizedProviderStatus,
  withTimeout,
} from './provider-core'

const JSON_HEADERS = { 'Content-Type': 'application/json', Accept: 'application/json' }

const protocolHeaders = (protocol: ProviderWireProtocol): Record<string, string> => ({
  ...PROVIDER_PROTOCOL_SEMANTIC_HEADERS[protocol],
})

const cleanBaseUrl = (baseUrl: string): string => baseUrl.replace(/\/+$/, '')

// OpenAI-compatible paths below include the version prefix. Accept either an
// origin-style base or the commonly configured `/v1` API base without ever
// producing `/v1/v1/...`.
const normalizeOpenAiCompatibleBaseUrl = (baseUrl: string): string =>
  cleanBaseUrl(baseUrl).replace(/\/v1$/i, '')

const ensureHttpSuccess = async (response: Response): Promise<Response> => {
  if (response.ok) return response
  const retryable = response.status === 408 || response.status === 429 || response.status >= 500
  throw new ProviderFailure(
    response.status === 429 ? 'rate_limited' : 'unavailable',
    `Provider request failed (${response.status})`,
    retryable,
  )
}

const readJson = async (response: Response): Promise<Record<string, unknown>> => {
  let value: unknown
  try {
    value = await response.json()
  } catch {
    throw new ProviderFailure('invalid_output', 'Provider returned invalid JSON', true)
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new ProviderFailure('invalid_output', 'Provider response is not an object', true)
  }
  return value as Record<string, unknown>
}

const bearerHeaders = (credential?: string): Record<string, string> =>
  credential ? { Authorization: `Bearer ${credential}` } : {}

const modelList = (value: Record<string, unknown>): ProviderModel[] => {
  const candidates = Array.isArray(value.data)
    ? value.data
    : Array.isArray(value.models)
      ? value.models
      : []
  return candidates.flatMap((entry): ProviderModel[] => {
    if (typeof entry === 'string') return [{ id: entry }]
    if (!entry || typeof entry !== 'object') return []
    const model = entry as Record<string, unknown>
    const id = typeof model.id === 'string' ? model.id : typeof model.name === 'string' ? model.name : undefined
    const supportedParameters = Array.isArray(model.supported_parameters)
      ? model.supported_parameters.filter((entry): entry is string => typeof entry === 'string')
      : undefined
    return id
      ? [{
          id,
          label: typeof model.name === 'string' ? model.name : undefined,
          ...(supportedParameters ? { supportedParameters } : {}),
        }]
      : []
  })
}

const sourceText = (request: ProviderGenerationRequest): string =>
  JSON.stringify(requestPayload(request))

const extractOpenAiChatText = (body: Record<string, unknown>): string => {
  const choices = body.choices
  if (!Array.isArray(choices) || choices.length !== 1) {
    throw new ProviderFailure('invalid_output', 'Provider returned no unique choice', true)
  }
  const choice = choices[0] as Record<string, unknown>
  const message = choice?.message as Record<string, unknown> | undefined
  if (typeof message?.content !== 'string') {
    throw new ProviderFailure('invalid_output', 'Provider returned no text content', true)
  }
  return message.content
}

const extractOpenAiResponseText = (body: Record<string, unknown>): string => {
  if (typeof body.output_text === 'string') return body.output_text
  if (!Array.isArray(body.output)) {
    throw new ProviderFailure('invalid_output', 'OpenAI response has no output', true)
  }
  const texts = body.output.flatMap((item): string[] => {
    if (!item || typeof item !== 'object') return []
    const content = (item as Record<string, unknown>).content
    if (!Array.isArray(content)) return []
    return content.flatMap((part): string[] => {
      if (!part || typeof part !== 'object') return []
      const text = (part as Record<string, unknown>).text
      return typeof text === 'string' ? [text] : []
    })
  })
  if (texts.length !== 1) {
    throw new ProviderFailure('invalid_output', 'OpenAI response has no unique text output', true)
  }
  return texts[0]
}

const BASE_CAPABILITIES: ProviderCapabilities = {
  modelDiscovery: true,
  structuredOutput: true,
  cancellation: true,
}

abstract class HttpProvider implements ProviderAdapter {
  readonly capabilities = BASE_CAPABILITIES

  constructor(
    protected readonly baseUrl: string,
    protected readonly credential?: string,
  ) {}

  abstract discoverModels(signal?: AbortSignal): Promise<ProviderModel[]>
  abstract generateStructured(
    request: ProviderGenerationRequest,
    signal?: AbortSignal,
  ): Promise<ProviderGenerationResult>

  protected async fetch(
    path: string,
    init: RequestInit,
    timeoutMs: number,
    signal?: AbortSignal,
  ): Promise<Response> {
    return withTimeout(timeoutMs, signal, async (boundedSignal) => {
      try {
        return await ensureHttpSuccess(
          await fetch(`${cleanBaseUrl(this.baseUrl)}${path}`, {
            ...init,
            cache: 'no-store',
            credentials: 'omit',
            referrerPolicy: 'no-referrer',
            signal: boundedSignal,
          }),
        )
      } catch (error) {
        if (error instanceof ProviderFailure) throw error
        throw new ProviderFailure('unavailable', 'Provider could not be reached', true)
      }
    })
  }
}

export class OpenAiCompatibleProvider extends HttpProvider {
  constructor(
    baseUrl: string,
    credential: string | undefined,
    private readonly useResponses: boolean,
    private readonly protocol: ProviderWireRequestWitness['protocol'] = useResponses
      ? 'openai_responses_v1'
      : 'openai_chat_completions_v1',
    private readonly chatJsonMode: 'json_schema' | 'json_object' | 'prompt_json_only' = 'json_schema',
    private readonly disableThinking = false,
    private readonly reasoningEffort?: 'none',
  ) {
    super(normalizeOpenAiCompatibleBaseUrl(baseUrl), credential)
  }

  async discoverModels(signal?: AbortSignal): Promise<ProviderModel[]> {
    const response = await this.fetch(
      '/v1/models',
      { headers: { ...JSON_HEADERS, ...bearerHeaders(this.credential) } },
      10_000,
      signal,
    )
    return modelList(await readJson(response))
  }

  async generateStructured(
    request: ProviderGenerationRequest,
    signal?: AbortSignal,
  ): Promise<ProviderGenerationResult> {
    const path = this.useResponses ? '/v1/responses' : '/v1/chat/completions'
    const wireBody = JSON.stringify(
      this.useResponses
        ? {
            model: request.model,
            instructions: request.instructions,
            input: sourceText(request),
            max_output_tokens: request.preferences.maxOutputTokens,
            text: {
              format: {
                type: 'json_schema',
                name: 'sprout_structured_language_artifact',
                strict: true,
                schema: request.outputSchema,
              },
            },
          }
        : {
            model: request.model,
            messages: [
              {
                role: 'system',
                content:
                  this.chatJsonMode === 'prompt_json_only'
                    ? `${request.instructions}\n\nReturn exactly one JSON object matching output_schema. Do not include Markdown fences, commentary, or trailing prose.`
                    : request.instructions,
              },
              { role: 'user', content: sourceText(request) },
            ],
            max_tokens: request.preferences.maxOutputTokens,
            temperature: request.preferences.temperature,
            ...(this.disableThinking ? { thinking: { type: 'disabled' } } : {}),
            ...(this.reasoningEffort ? { reasoning_effort: this.reasoningEffort } : {}),
            ...(this.chatJsonMode === 'prompt_json_only'
              ? {}
              : {
                  response_format:
                    this.chatJsonMode === 'json_object'
                      ? { type: 'json_object' }
                      : {
                          type: 'json_schema',
                          json_schema: {
                            name: 'sprout_structured_language_artifact',
                            strict: true,
                            schema: request.outputSchema,
                          },
                        },
                }),
          },
    )
    const witness: ProviderWireRequestWitness = {
      protocol: this.protocol,
      method: 'POST',
      path,
      selectedModel: request.model,
      body: wireBody,
    }
    try {
      const response = await this.fetch(
        path,
        {
          method: 'POST',
          headers: { ...protocolHeaders(this.protocol), ...bearerHeaders(this.credential) },
          body: wireBody,
        },
        request.preferences.timeoutMs,
        signal,
      )
      const body = await readJson(response)
      const text = this.useResponses
        ? extractOpenAiResponseText(body)
        : extractOpenAiChatText(body)
      return resultFromRaw(request, text, 1, 'succeeded', witness)
    } catch (error) {
      return preserveWireWitness(error, witness)
    }
  }
}

export class AnthropicCompatibleProvider extends HttpProvider {
  async discoverModels(signal?: AbortSignal): Promise<ProviderModel[]> {
    const response = await this.fetch(
      '/v1/models',
      {
        headers: {
          ...JSON_HEADERS,
          'x-api-key': this.credential ?? '',
          'anthropic-version': '2023-06-01',
        },
      },
      10_000,
      signal,
    )
    return modelList(await readJson(response))
  }

  async generateStructured(
    request: ProviderGenerationRequest,
    signal?: AbortSignal,
  ): Promise<ProviderGenerationResult> {
    const path = '/v1/messages'
    const wireBody = JSON.stringify({
      model: request.model,
      system: request.instructions,
      messages: [{ role: 'user', content: sourceText(request) }],
      max_tokens: request.preferences.maxOutputTokens,
      temperature: request.preferences.temperature,
    })
    const witness: ProviderWireRequestWitness = {
      protocol: 'anthropic_messages_v1',
      method: 'POST',
      path,
      selectedModel: request.model,
      body: wireBody,
    }
    try {
      const response = await this.fetch(
        path,
        {
          method: 'POST',
          headers: {
            ...protocolHeaders('anthropic_messages_v1'),
            'x-api-key': this.credential ?? '',
          },
          body: wireBody,
        },
        request.preferences.timeoutMs,
        signal,
      )
      const body = await readJson(response)
      const content = body.content
      if (!Array.isArray(content)) {
        throw new ProviderFailure('invalid_output', 'Anthropic response has no content', true)
      }
      const textParts = content.flatMap((entry): string[] => {
        if (!entry || typeof entry !== 'object') return []
        const item = entry as Record<string, unknown>
        return item.type === 'text' && typeof item.text === 'string' ? [item.text] : []
      })
      if (textParts.length !== 1) {
        throw new ProviderFailure('invalid_output', 'Anthropic response has no unique text', true)
      }
      return resultFromRaw(request, textParts[0], 1, 'succeeded', witness)
    } catch (error) {
      return preserveWireWitness(error, witness)
    }
  }
}

export class OllamaProvider extends HttpProvider {
  constructor(baseUrl: string, token?: string) {
    super(baseUrl, token)
  }

  async discoverModels(signal?: AbortSignal): Promise<ProviderModel[]> {
    const response = await this.fetch(
      '/api/tags',
      { headers: { ...JSON_HEADERS, ...bearerHeaders(this.credential) } },
      10_000,
      signal,
    )
    return modelList(await readJson(response))
  }

  async generateStructured(
    request: ProviderGenerationRequest,
    signal?: AbortSignal,
  ): Promise<ProviderGenerationResult> {
    const path = '/api/chat'
    const wireBody = JSON.stringify({
      model: request.model,
      stream: false,
      keep_alive: 0,
      messages: [
        { role: 'system', content: request.instructions },
        { role: 'user', content: sourceText(request) },
      ],
      format: request.outputSchema,
      options: {
        num_predict: request.preferences.maxOutputTokens,
        temperature: request.preferences.temperature,
      },
    })
    const witness: ProviderWireRequestWitness = {
      protocol: 'ollama_chat_v1',
      method: 'POST',
      path,
      selectedModel: request.model,
      body: wireBody,
    }
    try {
      const response = await this.fetch(
        path,
        {
          method: 'POST',
          headers: { ...protocolHeaders('ollama_chat_v1'), ...bearerHeaders(this.credential) },
          body: wireBody,
        },
        request.preferences.timeoutMs,
        signal,
      )
      const body = await readJson(response)
      const message = body.message as Record<string, unknown> | undefined
      if (typeof message?.content !== 'string') {
        throw new ProviderFailure('invalid_output', 'Ollama response has no text content', true)
      }
      return resultFromRaw(request, message.content, 1, 'succeeded', witness)
    } catch (error) {
      return preserveWireWitness(error, witness)
    }
  }
}

/**
 * DS4 checkpoint contract. The adapter enables generation only after the
 * endpoint proves the OpenAI-compatible /v1/models surface. No protocol is
 * inferred from a CasaOS page or from temporal/network proximity.
 */
export class Ds4Provider extends OpenAiCompatibleProvider {
  private readonly supportedParametersByModel = new Map<string, ReadonlySet<string>>()

  constructor(baseUrl: string, token?: string) {
    // `ds4_openai_chat_v1` deliberately never sends response_format. The
    // configured DS4 must return JSON under a deterministic scaffold; Sprout's
    // strict closed-schema/grounding validator remains authoritative.
    super(baseUrl, token, false, 'ds4_openai_chat_v1', 'prompt_json_only', false, 'none')
  }

  override async discoverModels(signal?: AbortSignal): Promise<ProviderModel[]> {
    const models = await super.discoverModels(signal)
    this.supportedParametersByModel.clear()
    for (const model of models) {
      this.supportedParametersByModel.set(model.id, new Set(model.supportedParameters ?? []))
    }
    return models
  }

  override async generateStructured(
    request: ProviderGenerationRequest,
    signal?: AbortSignal,
  ): Promise<ProviderGenerationResult> {
    const supported = this.supportedParametersByModel.get(request.model)
    const required = [
      'max_tokens',
      'reasoning_effort',
      ...(request.preferences.temperature === undefined ? [] : ['temperature']),
    ]
    if (!supported || required.some((parameter) => !supported.has(parameter))) {
      throw new ProviderFailure(
        'unavailable',
        'DS4 model capabilities were not discovered or do not support the exact request',
        false,
      )
    }
    return super.generateStructured(request, signal)
  }
}

export const commercialProvider = (profile: CommercialProfile): ProviderAdapter => {
  const defaultBase = {
    openai: 'https://api.openai.com',
    anthropic: 'https://api.anthropic.com',
    xai: 'https://api.x.ai',
    deepseek: 'https://api.deepseek.com',
    openai_compatible: '',
    anthropic_compatible: '',
  }[profile.provider]
  const baseUrl = profile.baseUrl || defaultBase
  if (!baseUrl || !baseUrl.startsWith('https://')) {
    throw new ProviderFailure('unavailable', 'Commercial provider URL must use HTTPS', false)
  }
  return profile.provider === 'anthropic' || profile.provider === 'anthropic_compatible'
    ? new AnthropicCompatibleProvider(baseUrl, profile.credential)
    : new OpenAiCompatibleProvider(
        baseUrl,
        profile.credential,
        profile.provider === 'openai',
        profile.provider === 'deepseek'
          ? 'deepseek_chat_v4'
          : profile.provider === 'openai'
            ? 'openai_responses_v1'
            : 'openai_chat_completions_v1',
        profile.provider === 'deepseek' ? 'json_object' : 'json_schema',
        profile.provider === 'deepseek',
      )
}

const isLoopback = (url: URL): boolean =>
  url.hostname === '127.0.0.1' || url.hostname === 'localhost' || url.hostname === '::1'

const isPrivateIpv4 = (hostname: string): boolean => {
  const octets = hostname.split('.').map(Number)
  if (octets.length !== 4 || octets.some((part) => !Number.isInteger(part) || part < 0 || part > 255)) {
    return false
  }
  return (
    octets[0] === 10 ||
    (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31) ||
    (octets[0] === 192 && octets[1] === 168)
  )
}

const isLocalDevelopmentTarget = (url: URL): boolean =>
  isLoopback(url) || isPrivateIpv4(url.hostname) || url.hostname.endsWith('.local')

export const lanProvider = (profile: LanProfile): ProviderAdapter => {
  const url = new URL(profile.baseUrl)
  if (
    url.protocol !== 'https:' &&
    !(url.protocol === 'http:' && isLoopback(url)) &&
    !(
      url.protocol === 'http:' &&
      profile.allowInsecureDevelopmentHttp &&
      isLocalDevelopmentTarget(url)
    )
  ) {
    throw new ProviderFailure(
      'unavailable',
      'LAN inference requires verified HTTPS outside loopback',
      false,
    )
  }
  return profile.engine === 'ollama'
    ? new OllamaProvider(profile.baseUrl, profile.token)
    : new Ds4Provider(profile.baseUrl, profile.token)
}

export const providerFailureStatus = (error: unknown): string =>
  error instanceof ProviderFailure
    ? sanitizedProviderStatus(error.code)
    : 'local_execution_failed'
