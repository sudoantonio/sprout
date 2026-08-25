import type { LocalAiProfile } from './contracts'
import {
  executionProfileCommitment,
  newExecutionProfileSecret,
} from './execution-profile'

const PROFILE_KEY = 'device:ai-generation-profile-v1'
const PROFILE_COMMITMENT_SECRET_KEY = 'device:ai-generation-profile-commitment-secret-v1'
const PROFILE_REVISION_KEY = 'device:ai-generation-profile-revision-v1'

const bytesToBase64 = (bytes: Uint8Array): string =>
  btoa(String.fromCharCode(...bytes))

const base64ToBytes = (value: string): Uint8Array =>
  Uint8Array.from(atob(value), (character) => character.charCodeAt(0))

const isRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value) && typeof value === 'object' && !Array.isArray(value)

export const validateLocalAiProfile = (value: unknown): LocalAiProfile => {
  if (!isRecord(value) || typeof value.mode !== 'string' || typeof value.model !== 'string') {
    throw new Error('Invalid local AI profile')
  }
  if (!isRecord(value.preferences)) throw new Error('Invalid generation preferences')
  const { timeoutMs, maxOutputTokens, maxAttempts, temperature } = value.preferences
  if (
    !Number.isSafeInteger(timeoutMs) ||
    Number(timeoutMs) < 250 ||
    Number(timeoutMs) > 300_000 ||
    !Number.isSafeInteger(maxOutputTokens) ||
    Number(maxOutputTokens) < 1 ||
    Number(maxOutputTokens) > 16_384 ||
    !Number.isSafeInteger(maxAttempts) ||
    Number(maxAttempts) < 1 ||
    Number(maxAttempts) > 16 ||
    (temperature !== undefined &&
      (typeof temperature !== 'number' || temperature < 0 || temperature > 2))
  ) {
    throw new Error('Generation preferences exceed the local bounds')
  }
  if (!value.model.trim()) throw new Error('A model must be selected explicitly')
  if (value.mode === 'commercial_api') {
    if (typeof value.provider !== 'string' || typeof value.credential !== 'string') {
      throw new Error('Commercial provider and credential are required')
    }
  } else if (value.mode === 'lan_inference') {
    if (
      (value.engine !== 'ds4' && value.engine !== 'ollama') ||
      typeof value.baseUrl !== 'string'
    ) {
      throw new Error('LAN engine and endpoint are required')
    }
  } else if (value.mode === 'private_remote') {
    if (value.validatedTransport !== false || typeof value.tlsPinSha256 !== 'string') {
      throw new Error('Remote transport must remain unvalidated in checkpoint 0032')
    }
  } else if (value.mode === 'commercial_privacy') {
    if (
      value.companionProtocolVersion !== 'sprout-local-privacy-v1' ||
      value.privacyModel !== 'gpt-oss-safeguard-20b'
    ) {
      throw new Error('Unsupported local privacy companion')
    }
  } else {
    throw new Error('Unsupported AI mode')
  }
  return value as unknown as LocalAiProfile
}

export class LocalAiProfileStore {
  constructor(
    private readonly vault: {
      getLocalSetting(key: string): string | undefined
      putLocalSetting(key: string, value: string): Promise<boolean>
      deleteLocalSetting(key: string): Promise<boolean>
    },
  ) {}

  load(): LocalAiProfile | undefined {
    const encoded = this.vault.getLocalSetting(PROFILE_KEY)
    return encoded ? validateLocalAiProfile(JSON.parse(encoded)) : undefined
  }

  async save(profile: LocalAiProfile): Promise<'persisted' | 'session_only'> {
    const validated = validateLocalAiProfile(profile)
    const encoded = JSON.stringify(validated)
    const changed = this.vault.getLocalSetting(PROFILE_KEY) !== encoded
    const writes = [await this.vault.putLocalSetting(PROFILE_KEY, encoded)]
    if (!this.vault.getLocalSetting(PROFILE_COMMITMENT_SECRET_KEY)) {
      writes.push(
        await this.vault.putLocalSetting(
          PROFILE_COMMITMENT_SECRET_KEY,
          bytesToBase64(newExecutionProfileSecret()),
        ),
      )
    }
    if (changed || !this.vault.getLocalSetting(PROFILE_REVISION_KEY)) {
      writes.push(
        await this.vault.putLocalSetting(PROFILE_REVISION_KEY, crypto.randomUUID()),
      )
    }
    return writes.every(Boolean) ? 'persisted' : 'session_only'
  }

  async delete(): Promise<'persisted' | 'session_only'> {
    const deleted = await Promise.all([
      this.vault.deleteLocalSetting(PROFILE_KEY),
      this.vault.deleteLocalSetting(PROFILE_COMMITMENT_SECRET_KEY),
      this.vault.deleteLocalSetting(PROFILE_REVISION_KEY),
    ])
    return deleted.every(Boolean) ? 'persisted' : 'session_only'
  }

  async executionProfileCommitment(): Promise<string> {
    const profile = this.load()
    const secret = this.vault.getLocalSetting(PROFILE_COMMITMENT_SECRET_KEY)
    const revision = this.vault.getLocalSetting(PROFILE_REVISION_KEY)
    if (!profile || !secret || !revision) throw new Error('Local AI profile is not committed')
    return executionProfileCommitment(profile, revision, base64ToBytes(secret))
  }
}

export const redactAiSecrets = (profile: LocalAiProfile): Record<string, unknown> => {
  switch (profile.mode) {
    case 'commercial_api':
      return { mode: profile.mode }
    case 'lan_inference':
      return { mode: profile.mode, engine: profile.engine }
    case 'private_remote':
      return { mode: profile.mode, engine: profile.engine, validatedTransport: false }
    case 'commercial_privacy':
      return {
        mode: profile.mode,
        companionProtocolVersion: profile.companionProtocolVersion,
        formallyEnabled: false,
      }
  }
}
