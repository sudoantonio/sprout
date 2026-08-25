import { canonicalGovernanceJson } from './canonical'
import type { LocalAiProfile } from './contracts'

const PROFILE_COMMITMENT_CONTEXT = 'sprout-client-provider-execution-profile-v1'

const bytesToHex = (bytes: Uint8Array): string =>
  Array.from(bytes, (value) => value.toString(16).padStart(2, '0')).join('')

const importHmacKey = (secret: Uint8Array): Promise<CryptoKey> =>
  crypto.subtle.importKey(
    'raw',
    Uint8Array.from(secret).buffer,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign'],
  )

export const newExecutionProfileSecret = (): Uint8Array => crypto.getRandomValues(new Uint8Array(32))

/**
 * Hiding commitment over the local execution profile. The random device-only
 * secret prevents offline enumeration of provider/model/endpoint tuples.
 */
export const executionProfileCommitment = async (
  profile: LocalAiProfile,
  profileRevision: string,
  deviceSecret: Uint8Array,
): Promise<string> => {
  if (deviceSecret.byteLength !== 32 || !profileRevision) {
    throw new Error('Invalid execution-profile commitment material')
  }
  const key = await importHmacKey(deviceSecret)
  const message = canonicalGovernanceJson({
    context: PROFILE_COMMITMENT_CONTEXT,
    profile_revision: profileRevision,
    profile,
  })
  return bytesToHex(
    new Uint8Array(await crypto.subtle.sign('HMAC', key, Uint8Array.from(message).buffer)),
  )
}
