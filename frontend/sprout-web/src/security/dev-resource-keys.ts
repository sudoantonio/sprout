import type { EncryptedPayloadDto, Uuid } from '../api/contracts'
import type { ResourceKind } from '../domain/models'
import type { DevVaultSnapshot, KeyVault } from './key-vault'
import type { SessionResponse } from '../api/contracts'
import { saveDevSession, loadDevSession } from './dev-session'
import { base64ToBytes, decryptDocument, zeroBytes } from './wasm'

const STORAGE_KEY = 'sprout-dev-resource-keys'

type KeyBackup = Record<string, Record<string, string>>

const isZeroKeyB64 = (value: string): boolean => {
  try {
    const bytes = base64ToBytes(value)
    return bytes.length === 0 || bytes.every((byte) => byte === 0)
  } catch {
    return true
  }
}

const filterLiveKeys = (
  keys: Record<string, string>,
): Record<string, string> => {
  const live: Record<string, string> = {}
  for (const [slot, value] of Object.entries(keys)) {
    if (!isZeroKeyB64(value)) live[slot] = value
  }
  return live
}

/** Later sources win, but a zero key never overwrites a live key. */
const mergeKeysPreferLive = (
  ...sources: Array<Record<string, string> | undefined>
): Record<string, string> => {
  const merged: Record<string, string> = {}
  for (const source of sources) {
    if (!source) continue
    for (const [slot, value] of Object.entries(source)) {
      if (isZeroKeyB64(value)) continue
      merged[slot] = value
    }
  }
  return merged
}

const readBackup = (): KeyBackup => {
  if (!import.meta.env.DEV) return {}
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null) return {}
    return parsed as KeyBackup
  } catch {
    return {}
  }
}

const writeBackup = (backup: KeyBackup): void => {
  if (!import.meta.env.DEV) return
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(backup))
  } catch {
    // Ignore quota / private mode failures
  }
}

export const hasDevResourceKeyBackup = (identityId: Uuid): boolean =>
  countDevResourceKeyBackup(identityId) > 0

export const countDevResourceKeyBackup = (identityId: Uuid): number => {
  if (!import.meta.env.DEV) return 0
  return Object.keys(filterLiveKeys(readBackup()[identityId] ?? {})).length
}

export const countDevResourceKeyBackupAll = (): number => {
  if (!import.meta.env.DEV) return 0
  let total = 0
  for (const slots of Object.values(readBackup())) {
    total += Object.keys(filterLiveKeys(slots)).length
  }
  return total
}

/** Merge identity backup slots into a vault snapshot (sync, no vault I/O). */
export const mergeDevResourceKeysIntoSnapshot = (
  identityId: Uuid,
  snapshot: DevVaultSnapshot,
): DevVaultSnapshot => ({
  ...snapshot,
  resourceKeys: mergeKeysPreferLive(
    readBackup()[identityId],
    snapshot.resourceKeys,
  ),
})

/** Persist all resource-key slots currently in the vault for this identity. */
export const backupDevResourceKeys = (
  identityId: Uuid,
  vault: KeyVault,
): void => {
  if (!import.meta.env.DEV) return
  const snapshot = vault.exportDevSnapshot()
  if (!snapshot) return
  const liveFromVault = filterLiveKeys(snapshot.resourceKeys)
  // Cleared/zeroed vault must not erase the backup.
  if (Object.keys(liveFromVault).length === 0) return
  const backup = readBackup()
  backup[identityId] = mergeKeysPreferLive(backup[identityId], liveFromVault)
  writeBackup(backup)
}

/**
 * Save session + vault without ever replacing live keys with zeroed keys
 * (StrictMode clearMemory → persist was corrupting the backup).
 */
export const persistDevVault = (
  session: SessionResponse,
  vault: KeyVault,
): void => {
  if (!import.meta.env.DEV) return
  const snapshot = vault.exportDevSnapshot()
  if (!snapshot) return

  const liveFromVault = filterLiveKeys(snapshot.resourceKeys)
  // If memory was cleared/zeroed, keep whatever is already stored.
  if (Object.keys(liveFromVault).length === 0) return

  backupDevResourceKeys(session.identity_id, vault)

  const existing = loadDevSession()
  const mergedKeys = mergeKeysPreferLive(
    existing?.session.identity_id === session.identity_id
      ? existing?.vault?.resourceKeys
      : undefined,
    readBackup()[session.identity_id],
    liveFromVault,
  )

  const mergedSnapshot: DevVaultSnapshot = {
    ...snapshot,
    identityId: snapshot.identityId ?? session.identity_id,
    resourceKeys: mergedKeys,
  }

  saveDevSession(session, mergedSnapshot)
  const backup = readBackup()
  backup[session.identity_id] = mergeKeysPreferLive(
    backup[session.identity_id],
    mergedKeys,
  )
  writeBackup(backup)
}

/** Merge previously backed-up resource keys into the in-memory vault. */
export const restoreDevResourceKeys = async (
  identityId: Uuid,
  vault: KeyVault,
): Promise<number> => {
  if (!import.meta.env.DEV || !vault.isUnlocked) return 0
  const slots = filterLiveKeys(readBackup()[identityId] ?? {})
  let restored = 0
  for (const [slot, value] of Object.entries(slots)) {
    const match = /^(body|header):([^:]+):(\d+)$/.exec(slot)
    if (!match) continue
    const purpose = match[1] as 'body' | 'header'
    const resourceId = match[2] as Uuid
    const epoch = Number(match[3])
    if (!Number.isSafeInteger(epoch) || epoch < 1) continue
    const bytes = base64ToBytes(value)
    try {
      await vault.putResourceKey(resourceId, bytes, epoch, purpose)
      restored += 1
    } finally {
      zeroBytes(bytes)
    }
  }
  return restored
}

/** Load every identity's backed-up keys into the vault (DEV recovery). */
export const restoreAllDevResourceKeys = async (
  vault: KeyVault,
): Promise<number> => {
  if (!import.meta.env.DEV || !vault.isUnlocked) return 0
  let total = 0
  for (const identityId of Object.keys(readBackup())) {
    total += await restoreDevResourceKeys(identityId as Uuid, vault)
  }
  return total
}

/**
 * Recover a DEV key whose historical backup slot no longer matches the wire
 * resource id. AEAD authentication is the mapping oracle: a candidate is only
 * rebound after it successfully decrypts the exact resource ciphertext/AAD.
 */
export const recoverDevResourceKeyFromBackup = async <T>(
  vault: KeyVault,
  input: {
    ciphertext: EncryptedPayloadDto
    projectId: Uuid
    resourceId: Uuid
    kind: ResourceKind
    aggregateVersion: number
    keyEpoch: number
    purpose: 'body' | 'header'
  },
): Promise<T | undefined> => {
  if (!import.meta.env.DEV || !vault.isUnlocked) return undefined
  const candidates = new Set<string>()
  for (const slots of Object.values(readBackup())) {
    for (const value of Object.values(filterLiveKeys(slots))) {
      candidates.add(value)
    }
  }

  for (const value of candidates) {
    const bytes = base64ToBytes(value)
    try {
      const document = await decryptDocument<T>(input.ciphertext, {
        projectId: input.projectId,
        resourceId: input.resourceId,
        kind: input.kind,
        aggregateVersion: input.aggregateVersion,
        keyEpoch: input.keyEpoch,
        resourceKey: bytes,
      })
      await vault.putResourceKey(
        input.resourceId,
        bytes,
        input.keyEpoch,
        input.purpose,
      )
      return document
    } catch {
      // Authentication failure means this candidate belongs to another
      // resource. Continue without ever persisting the incorrect mapping.
    } finally {
      zeroBytes(bytes)
    }
  }
  return undefined
}

/** Purge zeroed/corrupt keys left by earlier clearMemory races. */
export const purgeZeroDevResourceKeys = (): number => {
  if (!import.meta.env.DEV) return 0
  const backup = readBackup()
  let removed = 0
  for (const [identityId, slots] of Object.entries(backup)) {
    const live = filterLiveKeys(slots)
    removed += Object.keys(slots).length - Object.keys(live).length
    if (Object.keys(live).length === 0) delete backup[identityId]
    else backup[identityId] = live
  }
  writeBackup(backup)

  const session = loadDevSession()
  if (session?.vault) {
    const live = filterLiveKeys(session.vault.resourceKeys)
    removed +=
      Object.keys(session.vault.resourceKeys).length - Object.keys(live).length
    saveDevSession(session.session, {
      ...session.vault,
      resourceKeys: live,
    })
  }
  return removed
}

/** Exact purpose+epoch coverage for decrypt (not “any epoch” false hits). */
export const countBackupHitsForResources = (
  resources: Array<{
    resourceId: Uuid
    epoch: number
    needsBody: boolean
  }>,
): {
  hits: number
  missing: Uuid[]
  epochMiss: number
  purposeMiss: number
  zeroHits: number
} => {
  const tree = readBackup()
  const liveSlots = new Set<string>()
  const allSlots = new Set<string>()
  for (const slots of Object.values(tree)) {
    for (const [slot, value] of Object.entries(slots)) {
      allSlots.add(slot)
      if (!isZeroKeyB64(value)) liveSlots.add(slot)
    }
  }
  const missing: Uuid[] = []
  let hits = 0
  let epochMiss = 0
  let purposeMiss = 0
  let zeroHits = 0
  for (const resource of resources) {
    const purpose = resource.needsBody ? 'body' : 'header'
    const exact = `${purpose}:${resource.resourceId}:${resource.epoch}`
    if (liveSlots.has(exact)) {
      hits += 1
      continue
    }
    missing.push(resource.resourceId)
    const otherPurpose = purpose === 'body' ? 'header' : 'body'
    const hasExactOtherPurpose = liveSlots.has(
      `${otherPurpose}:${resource.resourceId}:${resource.epoch}`,
    )
    const hasAnyEpoch = [...liveSlots].some((slot) =>
      slot.startsWith(`${purpose}:${resource.resourceId}:`),
    )
    const hasZeroExact = allSlots.has(exact) && !liveSlots.has(exact)
    if (hasZeroExact) zeroHits += 1
    else if (hasExactOtherPurpose) purposeMiss += 1
    else if (hasAnyEpoch) epochMiss += 1
  }
  return { hits, missing, epochMiss, purposeMiss, zeroHits }
}
