import type {
  PrivateRemoteProfile,
  ProviderAdapter,
  ProviderGenerationRequest,
  ProviderGenerationResult,
  ProviderModel,
} from './contracts'
import { ProviderFailure } from './provider-core'

export const validatePrivateDestination = (destination: string): boolean => {
  const separator = destination.lastIndexOf('/')
  if (separator < 1) return false
  const address = destination.slice(0, separator)
  const prefix = destination.slice(separator + 1)
  if (prefix === '32') {
    const octets = address.split('.')
    return (
      octets.length === 4 &&
      octets.every(
        (part) => /^\d{1,3}$/.test(part) && Number(part) >= 0 && Number(part) <= 255,
      )
    )
  }
  if (prefix !== '128' || !address.includes(':') || address.includes('%')) return false
  try {
    return new URL(`http://[${address}]/`).hostname.startsWith('[')
  } catch {
    return false
  }
}

/**
 * Mode C intentionally has no network implementation in 0032. It records the
 * per-destination contract locally and fails closed until a separately
 * validated private transport is supplied by the edge environment.
 */
export class UnvalidatedPrivateRemoteProvider implements ProviderAdapter {
  readonly capabilities = {
    modelDiscovery: false,
    structuredOutput: true,
    cancellation: true,
  }

  constructor(readonly profile: PrivateRemoteProfile) {
    if (!validatePrivateDestination(profile.destination)) {
      throw new ProviderFailure(
        'remote_transport_unvalidated',
        'Remote destination must be an exact /32 or /128 address',
        false,
      )
    }
  }

  discoverModels(_signal?: AbortSignal): Promise<ProviderModel[]> {
    return Promise.reject(
      new ProviderFailure(
        'remote_transport_unvalidated',
        'Private remote transport has not been live-validated',
        false,
      ),
    )
  }

  generateStructured(
    _request: ProviderGenerationRequest,
    _signal?: AbortSignal,
  ): Promise<ProviderGenerationResult> {
    return Promise.reject(
      new ProviderFailure(
        'remote_transport_unvalidated',
        'Private remote transport has not been live-validated',
        false,
      ),
    )
  }
}
