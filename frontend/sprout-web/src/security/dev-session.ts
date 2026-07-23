import type { SessionResponse } from '../api/contracts'
import type { DevVaultSnapshot } from './key-vault'

const STORAGE_KEY = 'sprout-dev-session'

interface StoredDevSession {
  session: SessionResponse
  vault?: DevVaultSnapshot
}

export interface DevSessionBundle {
  session: SessionResponse
  vault?: DevVaultSnapshot
}

const isSessionResponse = (value: unknown): value is SessionResponse =>
  typeof value === 'object' &&
  value !== null &&
  'token' in value &&
  'device_id' in value &&
  'expires_at' in value

const isExpired = (session: SessionResponse): boolean =>
  new Date(session.expires_at) <= new Date()

export const saveDevSession = (
  session: SessionResponse,
  vault?: DevVaultSnapshot,
): void => {
  if (!import.meta.env.DEV) return
  const payload: StoredDevSession = vault ? { session, vault } : { session }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(payload))
}

export const loadDevSession = (): DevSessionBundle | undefined => {
  if (!import.meta.env.DEV) return undefined
  const raw = localStorage.getItem(STORAGE_KEY)
  if (!raw) return undefined
  try {
    const parsed: unknown = JSON.parse(raw)
    if (isSessionResponse(parsed)) {
      if (isExpired(parsed)) {
        localStorage.removeItem(STORAGE_KEY)
        return undefined
      }
      return { session: parsed }
    }
    if (
      typeof parsed === 'object' &&
      parsed !== null &&
      isSessionResponse((parsed as StoredDevSession).session)
    ) {
      const bundle = parsed as StoredDevSession
      if (isExpired(bundle.session)) {
        localStorage.removeItem(STORAGE_KEY)
        return undefined
      }
      // Keep the vault even when device ids drift: resource keys are what
      // decrypt needs. Dropping the vault here was wiping keys on every reload
      // after a device-id mismatch (passkey/dev login rotation).
      if (
        bundle.vault &&
        bundle.vault.deviceId !== bundle.session.device_id &&
        bundle.vault.identityId &&
        bundle.vault.identityId !== bundle.session.identity_id
      ) {
        return { session: bundle.session }
      }
      return bundle
    }
    localStorage.removeItem(STORAGE_KEY)
    return undefined
  } catch {
    localStorage.removeItem(STORAGE_KEY)
    return undefined
  }
}

export const clearDevSession = (): void => {
  localStorage.removeItem(STORAGE_KEY)
}
