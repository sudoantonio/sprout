import type { KeyVault } from '../security/key-vault'

const PREFIX = 'device:external-tool-connector:'

export type ReadOnlyConnectorKind = 'mail.receive' | 'telegram.receive'

export interface LocalConnectorProfile {
  version: 1
  kind: ReadOnlyConnectorKind
  opaqueProfileId: string
  encryptedConfiguration: string
}

export interface ReadOnlyConnectorAdapter {
  readonly kind: ReadOnlyConnectorKind
  discoverCapabilities(profile: LocalConnectorProfile): Promise<readonly string[]>
  receiveStructured(profile: LocalConnectorProfile, canonicalInput: unknown, signal: AbortSignal): Promise<unknown>
}

export async function saveLocalConnectorProfile(vault: KeyVault, profile: LocalConnectorProfile): Promise<boolean> {
  validateProfile(profile)
  return vault.putLocalSetting(`${PREFIX}${profile.kind}:${profile.opaqueProfileId}`, JSON.stringify(profile))
}

export function loadLocalConnectorProfile(vault: KeyVault, kind: ReadOnlyConnectorKind, opaqueProfileId: string): LocalConnectorProfile | undefined {
  const encoded = vault.getLocalSetting(`${PREFIX}${kind}:${opaqueProfileId}`)
  if (!encoded) return undefined
  const value: unknown = JSON.parse(encoded)
  validateProfile(value)
  return value
}

export async function deleteLocalConnectorProfile(vault: KeyVault, kind: ReadOnlyConnectorKind, opaqueProfileId: string): Promise<boolean> {
  return vault.deleteLocalSetting(`${PREFIX}${kind}:${opaqueProfileId}`)
}

export function rejectExternalSend(kind: 'mail.send' | 'telegram.send'): never {
  throw new Error(`${kind}:fail_closed_external_disclosure_sink_missing`)
}

function validateProfile(value: unknown): asserts value is LocalConnectorProfile {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) throw new Error('Invalid local connector profile')
  const profile = value as Partial<LocalConnectorProfile>
  if (profile.version !== 1 || !['mail.receive', 'telegram.receive'].includes(profile.kind ?? '') || !profile.opaqueProfileId || !profile.encryptedConfiguration) {
    throw new Error('Invalid local connector profile')
  }
}
