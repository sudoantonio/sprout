import { ProviderFailure } from './provider-core'

export const PRIVACY_PROTOCOL_VERSION = 'sprout-local-privacy-v1' as const
export const PRIVACY_MODEL_CANDIDATE = 'gpt-oss-safeguard-20b' as const

export interface SensitiveSpan {
  start: number
  end: number
  kind: 'PERSON' | 'EMAIL' | 'PHONE' | 'ADDRESS' | 'SECRET' | 'OTHER'
}

export interface PrivacyCompanion {
  status(): Promise<{ runtimeInstalled: boolean; modelInstalled: boolean }>
  classify(input: string, signal?: AbortSignal): Promise<SensitiveSpan[]>
  requestRuntimeInstallConsent(): Promise<boolean>
  requestModelDownloadConsent(): Promise<boolean>
  removeModel(): Promise<void>
  uninstallRuntime(): Promise<void>
}

export interface PrivacyMapping {
  transformed: string
  reconstruct(output: string): string
  purge(): void
  get purged(): boolean
}

const validateSpans = (input: string, spans: SensitiveSpan[]): SensitiveSpan[] => {
  const ordered = [...spans].sort((left, right) => left.start - right.start || left.end - right.end)
  let previousEnd = 0
  for (const span of ordered) {
    if (
      !Number.isSafeInteger(span.start) ||
      !Number.isSafeInteger(span.end) ||
      span.start < previousEnd ||
      span.end <= span.start ||
      span.end > input.length
    ) {
      throw new ProviderFailure('invalid_output', 'Privacy companion returned invalid spans', false)
    }
    previousEnd = span.end
  }
  return ordered
}

export const pseudonymize = (input: string, spans: SensitiveSpan[]): PrivacyMapping => {
  const ordered = validateSpans(input, spans)
  const counters = new Map<SensitiveSpan['kind'], number>()
  const mapping = new Map<string, string>()
  let cursor = 0
  let transformed = ''
  for (const span of ordered) {
    transformed += input.slice(cursor, span.start)
    const sequence = (counters.get(span.kind) ?? 0) + 1
    counters.set(span.kind, sequence)
    const placeholder = `[[${span.kind}_${String(sequence).padStart(4, '0')}]]`
    mapping.set(placeholder, input.slice(span.start, span.end))
    transformed += placeholder
    cursor = span.end
  }
  transformed += input.slice(cursor)
  let purged = false
  return {
    transformed,
    reconstruct(output: string): string {
      if (purged) throw new Error('Privacy mapping has been purged')
      const placeholders = output.match(/\[\[[A-Z]+_\d{4}\]\]/g) ?? []
      for (const placeholder of placeholders) {
        if (!mapping.has(placeholder)) {
          throw new ProviderFailure('invalid_output', 'Unknown privacy placeholder', false)
        }
      }
      let reconstructed = output
      for (const [placeholder, original] of mapping) {
        reconstructed = reconstructed.split(placeholder).join(original)
      }
      return reconstructed
    },
    purge(): void {
      mapping.clear()
      purged = true
    },
    get purged(): boolean {
      return purged
    },
  }
}

/**
 * Isolated experimental mode D. It deliberately does not implement the
 * ProviderAdapter interface and therefore cannot enable an R5.41 model surface
 * in checkpoint 0032. There is no fallback to the commercial core.
 */
export const preparePrivacyInput = async (
  companion: PrivacyCompanion,
  input: string,
  signal?: AbortSignal,
): Promise<PrivacyMapping> => {
  const status = await companion.status()
  if (!status.runtimeInstalled || !status.modelInstalled) {
    throw new ProviderFailure(
      'privacy_companion_unavailable',
      status.runtimeInstalled
        ? 'Local privacy model is not installed'
        : 'Local privacy runtime is not installed',
      false,
    )
  }
  return pseudonymize(input, await companion.classify(input, signal))
}
