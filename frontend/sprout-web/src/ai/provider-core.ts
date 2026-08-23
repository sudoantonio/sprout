import {
  canonicalGovernanceJson,
  sha256Hex,
} from './canonical'
import type {
  ProviderAdapter,
  ProviderGenerationRequest,
  ProviderGenerationResult,
  ProviderWireRequestWitness,
} from './contracts'

export class ProviderFailure extends Error {
  constructor(
    readonly code:
      | 'cancelled'
      | 'timeout'
      | 'rate_limited'
      | 'unavailable'
      | 'invalid_output'
      | 'remote_transport_unvalidated'
      | 'privacy_companion_unavailable',
    message: string,
    readonly retryable: boolean,
    readonly wireWitness?: ProviderWireRequestWitness,
  ) {
    super(message)
  }

  withWireWitness(wireWitness: ProviderWireRequestWitness): ProviderFailure {
    return new ProviderFailure(this.code, this.message, this.retryable, wireWitness)
  }
}

export const sanitizedProviderStatus = (status: string): string =>
  status.replace(/[^a-z0-9_-]/gi, '_').slice(0, 128) || 'provider_error'

const abortReason = (signal: AbortSignal): ProviderFailure =>
  signal.reason instanceof ProviderFailure
    ? signal.reason
    : new ProviderFailure('cancelled', 'Generation was cancelled', false)

export const withTimeout = async <T>(
  timeoutMs: number,
  signal: AbortSignal | undefined,
  operation: (signal: AbortSignal) => Promise<T>,
): Promise<T> => {
  if (signal?.aborted) throw abortReason(signal)
  const controller = new AbortController()
  const timeoutFailure = new ProviderFailure(
    'timeout',
    'The local provider request timed out',
    true,
  )
  const timeout = setTimeout(() => controller.abort(timeoutFailure), timeoutMs)
  const cancel = () => controller.abort(abortReason(signal!))
  signal?.addEventListener('abort', cancel, { once: true })
  try {
    return await operation(controller.signal)
  } catch (error) {
    if (controller.signal.aborted) throw abortReason(controller.signal)
    throw error
  } finally {
    clearTimeout(timeout)
    signal?.removeEventListener('abort', cancel)
  }
}

export const strictJsonObject = (raw: string): Record<string, unknown> => {
  if (!raw.trim()) {
    throw new ProviderFailure('invalid_output', 'Provider returned an empty response', true)
  }
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch {
    throw new ProviderFailure(
      'invalid_output',
      'Provider response is not a single JSON value',
      true,
    )
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new ProviderFailure('invalid_output', 'Provider output must be an object', true)
  }
  return value as Record<string, unknown>
}

export const assertClosedObject = (
  value: Record<string, unknown>,
  required: readonly string[],
): void => {
  const keys = Object.keys(value).sort()
  const expected = [...required].sort()
  if (keys.length !== expected.length || keys.some((key, index) => key !== expected[index])) {
    throw new ProviderFailure(
      'invalid_output',
      'Provider output does not match the closed schema',
      true,
    )
  }
}

export const requestPayload = (request: ProviderGenerationRequest): unknown => ({
  task: request.task,
  instructions: request.instructions,
  sources: request.sources,
  input: request.input,
  output_schema: request.outputSchema,
})

export const resultFromRaw = async (
  request: ProviderGenerationRequest,
  raw: string,
  attemptCount: number,
  status: string,
  wireWitness: ProviderWireRequestWitness,
): Promise<ProviderGenerationResult> => {
  const value = strictJsonObject(raw)
  assertExactWireWitness(request, wireWitness)
  return {
    value,
    attemptCount,
    sanitizedStatus: sanitizedProviderStatus(status),
    wireWitness,
    actualRequestCommitmentHex: await providerWireRequestCommitment(wireWitness),
    actualOutputCommitmentHex: await sha256Hex(canonicalGovernanceJson(value)),
  }
}

export const providerWireRequestCommitment = (
  witness: ProviderWireRequestWitness,
): Promise<string> => sha256Hex(canonicalGovernanceJson(witness))

export const preserveWireWitness = (error: unknown, witness: ProviderWireRequestWitness): never => {
  if (error instanceof ProviderFailure) throw error.withWireWitness(witness)
  throw new ProviderFailure('unavailable', 'Provider request failed', true, witness)
}

export const assertExactWireWitness = (
  request: ProviderGenerationRequest,
  witness: ProviderWireRequestWitness,
): void => {
  if (witness.selectedModel !== request.model || witness.method !== 'POST') {
    throw new ProviderFailure(
      'invalid_output',
      'Provider wire witness does not bind the selected model',
      false,
    )
  }
  let body: unknown
  try {
    body = JSON.parse(witness.body)
  } catch {
    throw new ProviderFailure('invalid_output', 'Provider wire body is not exact JSON', false)
  }
  if (
    !body ||
    typeof body !== 'object' ||
    Array.isArray(body) ||
    (body as Record<string, unknown>).model !== request.model
  ) {
    throw new ProviderFailure(
      'invalid_output',
      'Provider wire body changed the selected model',
      false,
    )
  }
}

export const generateWithBoundedRetry = async (
  adapter: Pick<ProviderAdapter, 'generateStructured'>,
  request: ProviderGenerationRequest,
  signal?: AbortSignal,
): Promise<ProviderGenerationResult> => {
  const attempts = Math.min(request.preferences.maxAttempts, 16)
  let lastFailure: ProviderFailure | undefined
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await adapter.generateStructured(
        { ...request, preferences: { ...request.preferences, maxAttempts: 1 } },
        signal,
      )
    } catch (error) {
      const failure =
        error instanceof ProviderFailure
          ? error
          : new ProviderFailure('unavailable', 'Provider request failed', true)
      lastFailure = failure
      if (!failure.retryable || attempt === attempts) throw failure
    }
  }
  throw lastFailure ?? new ProviderFailure('unavailable', 'Provider request failed', false)
}
