import type { Uuid } from '../api/contracts'
import {
  EncryptedDatabase,
  type VaultCipherRecord,
} from '../storage/encrypted-db'
import {
  base64ToBytes,
  bytesToBase64,
  type DeviceSecrets,
  zeroBytes,
} from './wasm'

const DEV_RESOURCE_KEYS_STORAGE = 'sprout-dev-resource-keys'

const isAllZeroKey = (key: Uint8Array): boolean =>
  key.length === 0 || key.every((byte) => byte === 0)

const readDevBackupTree = (): Record<string, Record<string, string>> => {
  if (!import.meta.env.DEV) return {}
  try {
    const raw = localStorage.getItem(DEV_RESOURCE_KEYS_STORAGE)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null) return {}
    return parsed as Record<string, Record<string, string>>
  } catch {
    return {}
  }
}

/**
 * DEV decrypt fallback. Exact purpose/resource/epoch only (wrong epoch breaks
 * AEAD). Skips all-zero keys left by clearMemory corruption. Searches the
 * preferred identity first, then any other identity backup.
 */
const peekDevBackupKey = (
  identityId: Uuid | undefined,
  resourceId: Uuid,
  epoch: number,
  purpose: ResourceKeyPurpose,
): Uint8Array | undefined => {
  if (!import.meta.env.DEV) return undefined
  const tree = readDevBackupTree()
  const exactSlot = `${purpose}:${resourceId}:${epoch}`
  const identityOrder = identityId
    ? [identityId, ...Object.keys(tree).filter((id) => id !== identityId)]
    : Object.keys(tree)

  for (const id of identityOrder) {
    const value = tree[id]?.[exactSlot]
    if (!value) continue
    try {
      const bytes = base64ToBytes(value)
      if (bytes.length === 0 || bytes.every((byte) => byte === 0)) continue
      return bytes
    } catch {
      // try next identity
    }
  }
  return undefined
}

export interface SerializedDeviceSecrets {
  keyVersion: number
  suiteVersion: number
  publicPackageB64: string
  x25519PrivateKeyB64: string
  mlKem768PrivateKeyB64: string
  ed25519PrivateKeyB64: string
  mlDsa65PrivateKeyB64: string
}

/** Dev-only vault snapshot for localStorage session restore after reload. */
export interface DevVaultSnapshot {
  deviceId: Uuid
  identityId?: Uuid
  device: SerializedDeviceSecrets
  resourceKeys: Record<string, string>
}

interface VaultPlaintext {
  version: 1 | 2 | 3 | 4
  identityId?: Uuid
  device: SerializedDeviceSecrets
  resourceKeys: Record<string, string>
}

export type VaultPersistence = 'locked' | 'session-only' | 'prf-wrapped'

const encoder = new TextEncoder()
const decoder = new TextDecoder()
const VAULT_INFO = encoder.encode('sprout-key-vault-v1')
export type ResourceKeyPurpose = 'body' | 'header'
const resourceKeySlot = (
  resourceId: Uuid,
  epoch: number,
  purpose: ResourceKeyPurpose = 'body',
): string => `${purpose}:${resourceId}:${epoch}`
const asArrayBuffer = (value: Uint8Array): ArrayBuffer =>
  value.buffer.slice(
    value.byteOffset,
    value.byteOffset + value.byteLength,
  ) as ArrayBuffer

const deriveVaultKey = async (
  prfOutput: Uint8Array,
  salt: Uint8Array,
  deviceId: Uuid,
): Promise<CryptoKey> => {
  const input = await crypto.subtle.importKey(
    'raw',
    asArrayBuffer(prfOutput),
    'HKDF',
    false,
    ['deriveKey'],
  )
  return crypto.subtle.deriveKey(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: asArrayBuffer(salt),
      info: asArrayBuffer(
        new Uint8Array([...VAULT_INFO, ...encoder.encode(deviceId)]),
      ),
    },
    input,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt'],
  )
}

const serializeDevice = (secrets: DeviceSecrets): SerializedDeviceSecrets => ({
  keyVersion: secrets.keyVersion,
  suiteVersion: secrets.suiteVersion,
  publicPackageB64: bytesToBase64(secrets.publicPackage),
  x25519PrivateKeyB64: bytesToBase64(secrets.x25519PrivateKey),
  mlKem768PrivateKeyB64: bytesToBase64(secrets.mlKem768PrivateKey),
  ed25519PrivateKeyB64: bytesToBase64(secrets.ed25519PrivateKey),
  mlDsa65PrivateKeyB64: bytesToBase64(secrets.mlDsa65PrivateKey),
})

const deserializeDevice = (
  serialized: SerializedDeviceSecrets,
): DeviceSecrets => ({
  keyVersion: serialized.keyVersion,
  suiteVersion: serialized.suiteVersion,
  publicPackage: base64ToBytes(serialized.publicPackageB64),
  x25519PrivateKey: base64ToBytes(serialized.x25519PrivateKeyB64),
  mlKem768PrivateKey: base64ToBytes(serialized.mlKem768PrivateKeyB64),
  ed25519PrivateKey: base64ToBytes(serialized.ed25519PrivateKeyB64),
  mlDsa65PrivateKey: base64ToBytes(serialized.mlDsa65PrivateKeyB64),
})

export class KeyVault {
  readonly #database: EncryptedDatabase
  #deviceId?: Uuid
  #identityId?: Uuid
  #credentialId?: string
  #deviceSecrets?: DeviceSecrets
  #resourceKeys = new Map<string, Uint8Array>()
  #wrappingKey?: CryptoKey
  #salt?: Uint8Array
  #persistence: VaultPersistence = 'locked'
  #devMutationListener?: () => void

  constructor(database: EncryptedDatabase) {
    this.#database = database
  }

  /** DEV-only hook: fired after resource keys change so App can persist them. */
  setDevMutationListener(listener?: () => void): void {
    this.#devMutationListener = listener
  }

  get persistence(): VaultPersistence {
    return this.#persistence
  }

  get isUnlocked(): boolean {
    return Boolean(this.#deviceSecrets)
  }

  get deviceSecrets(): DeviceSecrets {
    if (!this.#deviceSecrets) {
      throw new Error('This device key vault is locked')
    }
    return this.#deviceSecrets
  }

  get localDeviceId(): Uuid | undefined {
    return this.#deviceId
  }

  get localIdentityId(): Uuid | undefined {
    return this.#identityId
  }

  setSessionSecrets(
    deviceId: Uuid,
    secrets: DeviceSecrets,
    identityId?: Uuid,
  ): void {
    this.clearMemory()
    this.#deviceId = deviceId
    this.#identityId = identityId
    this.#deviceSecrets = secrets
    this.#persistence = 'session-only'
  }

  exportDevSnapshot(): DevVaultSnapshot | undefined {
    if (!this.#deviceId || !this.#deviceSecrets) {
      return undefined
    }
    return {
      deviceId: this.#deviceId,
      identityId: this.#identityId,
      device: serializeDevice(this.#deviceSecrets),
      resourceKeys: Object.fromEntries(
        [...this.#resourceKeys].map(([id, key]) => [
          id,
          bytesToBase64(key),
        ]),
      ),
    }
  }

  restoreDevSnapshot(snapshot: DevVaultSnapshot): void {
    this.clearMemory()
    this.#deviceId = snapshot.deviceId
    this.#identityId = snapshot.identityId
    this.#deviceSecrets = deserializeDevice(snapshot.device)
    this.#resourceKeys = new Map()
    for (const [id, value] of Object.entries(snapshot.resourceKeys)) {
      try {
        const bytes = base64ToBytes(value)
        if (isAllZeroKey(bytes)) continue
        this.#resourceKeys.set(id, bytes)
      } catch {
        // skip corrupt slots
      }
    }
    this.#persistence = 'session-only'
  }

  /** Fill missing identity after DEV restore from a session payload. */
  ensureIdentityId(identityId: Uuid): void {
    if (!this.#identityId) {
      this.#identityId = identityId
    }
  }

  async enablePrfPersistence(
    prfOutput: Uint8Array,
    credentialId: string,
  ): Promise<void> {
    if (!this.#deviceId || !this.#deviceSecrets) {
      throw new Error('Create device keys before enabling persistent vault')
    }
    if (!credentialId) {
      throw new Error('A passkey credential is required for local persistence')
    }
    const salt = crypto.getRandomValues(new Uint8Array(32))
    try {
      this.#wrappingKey = await deriveVaultKey(
        prfOutput,
        salt,
        this.#deviceId,
      )
      this.#salt = salt.slice()
      this.#credentialId = credentialId
      this.#persistence = 'prf-wrapped'
      await this.persist()
    } finally {
      zeroBytes(prfOutput, salt)
    }
  }

  async unlockWithPrf(
    deviceId: Uuid,
    prfOutput: Uint8Array,
  ): Promise<boolean> {
    const record = await this.#database.getVault(deviceId)
    if (!record) {
      zeroBytes(prfOutput)
      return false
    }

    const salt = base64ToBytes(record.saltB64)
    const nonce = base64ToBytes(record.nonceB64)
    const ciphertext = base64ToBytes(record.ciphertextB64)
    let plaintext: ArrayBuffer | undefined
    try {
      const wrappingKey = await deriveVaultKey(prfOutput, salt, deviceId)
      plaintext = await crypto.subtle.decrypt(
        {
          name: 'AES-GCM',
          iv: asArrayBuffer(nonce),
          additionalData: asArrayBuffer(
            encoder.encode(`sprout-vault:${deviceId}`),
          ),
        },
        wrappingKey,
        asArrayBuffer(ciphertext),
      )
      const decoded = JSON.parse(
        decoder.decode(plaintext),
      ) as VaultPlaintext
      if (
        decoded.version !== 1 &&
        decoded.version !== 2 &&
        decoded.version !== 3 &&
        decoded.version !== 4
      ) {
        throw new Error('Unsupported local vault version')
      }
      if (!record.credentialId) {
        throw new Error('Local vault has no bound passkey credential')
      }
      this.clearMemory()
      this.#deviceId = deviceId
      this.#identityId = decoded.identityId
      this.#credentialId = record.credentialId
      this.#deviceSecrets = deserializeDevice(decoded.device)
      this.#resourceKeys = new Map(
        Object.entries(decoded.resourceKeys).map(([id, value]) => [
          decoded.version === 4
            ? id
            : decoded.version === 3
              ? `body:${id}`
              : resourceKeySlot(id, 1),
          base64ToBytes(value),
        ]),
      )
      this.#wrappingKey = wrappingKey
      this.#salt = salt.slice()
      this.#persistence = 'prf-wrapped'
      return true
    } finally {
      zeroBytes(
        prfOutput,
        salt,
        nonce,
        ciphertext,
        plaintext ? new Uint8Array(plaintext) : undefined,
      )
    }
  }

  getResourceKey(resourceId: Uuid, epoch = 1): Uint8Array | undefined {
    const slot = resourceKeySlot(resourceId, epoch, 'body')
    const existing = this.#resourceKeys.get(slot)
    // All-zero entries are clearMemory leftovers — treat as missing so DEV
    // backup (or a later restore) can supply a live key.
    if (existing && !isAllZeroKey(existing)) return existing
    if (existing) {
      this.#resourceKeys.delete(slot)
    }
    const fromBackup = peekDevBackupKey(
      this.#identityId,
      resourceId,
      epoch,
      'body',
    )
    if (fromBackup) {
      this.#resourceKeys.set(slot, fromBackup)
      return fromBackup
    }
    return undefined
  }

  getHeaderKey(resourceId: Uuid, epoch = 1): Uint8Array | undefined {
    const slot = resourceKeySlot(resourceId, epoch, 'header')
    const existing = this.#resourceKeys.get(slot)
    if (existing && !isAllZeroKey(existing)) return existing
    if (existing) {
      this.#resourceKeys.delete(slot)
    }
    const fromBackup = peekDevBackupKey(
      this.#identityId,
      resourceId,
      epoch,
      'header',
    )
    if (fromBackup) {
      this.#resourceKeys.set(slot, fromBackup)
      return fromBackup
    }
    return undefined
  }

  getLatestResourceKey(
    resourceId: Uuid,
  ): { epoch: number; key: Uint8Array } | undefined {
    const prefix = `body:${resourceId}:`
    let latest: { epoch: number; key: Uint8Array } | undefined
    for (const [slot, key] of this.#resourceKeys) {
      if (!slot.startsWith(prefix)) continue
      const epoch = Number(slot.slice(prefix.length))
      if (
        Number.isSafeInteger(epoch) &&
        epoch > 0 &&
        (!latest || epoch > latest.epoch)
      ) {
        latest = { epoch, key }
      }
    }
    return latest
  }

  async putResourceKey(
    resourceId: Uuid,
    key: Uint8Array,
    epoch = 1,
    purpose: ResourceKeyPurpose = 'body',
  ): Promise<void> {
    if (isAllZeroKey(key)) {
      throw new Error('Refusing to store an all-zero resource key')
    }
    const slot = resourceKeySlot(resourceId, epoch, purpose)
    const existing = this.#resourceKeys.get(slot)
    zeroBytes(existing)
    this.#resourceKeys.set(slot, key.slice())
    await this.persist()
    this.#devMutationListener?.()
  }

  async persist(): Promise<boolean> {
    if (
      !this.#wrappingKey ||
      !this.#salt ||
      !this.#credentialId ||
      !this.#deviceId ||
      !this.#deviceSecrets
    ) {
      return false
    }
    const value: VaultPlaintext = {
      version: 4,
      identityId: this.#identityId,
      device: serializeDevice(this.#deviceSecrets),
      resourceKeys: Object.fromEntries(
        [...this.#resourceKeys].map(([id, key]) => [
          id,
          bytesToBase64(key),
        ]),
      ),
    }
    const encoded = encoder.encode(JSON.stringify(value))
    const nonce = crypto.getRandomValues(new Uint8Array(12))
    try {
      const encrypted = await crypto.subtle.encrypt(
        {
          name: 'AES-GCM',
          iv: asArrayBuffer(nonce),
          additionalData: asArrayBuffer(
            encoder.encode(`sprout-vault:${this.#deviceId}`),
          ),
        },
        this.#wrappingKey,
        asArrayBuffer(encoded),
      )
      const record: VaultCipherRecord = {
        id: `device:${this.#deviceId}`,
        deviceId: this.#deviceId,
        credentialId: this.#credentialId,
        saltB64: bytesToBase64(this.#salt),
        nonceB64: bytesToBase64(nonce),
        ciphertextB64: bytesToBase64(new Uint8Array(encrypted)),
        createdAt: new Date().toISOString(),
      }
      await this.#database.putVault(record)
      return true
    } finally {
      zeroBytes(encoded, nonce)
    }
  }

  clearMemory(): void {
    if (this.#deviceSecrets) {
      zeroBytes(
        this.#deviceSecrets.publicPackage,
        this.#deviceSecrets.x25519PrivateKey,
        this.#deviceSecrets.mlKem768PrivateKey,
        this.#deviceSecrets.ed25519PrivateKey,
        this.#deviceSecrets.mlDsa65PrivateKey,
      )
    }
    for (const key of this.#resourceKeys.values()) {
      zeroBytes(key)
    }
    zeroBytes(this.#salt)
    this.#resourceKeys.clear()
    this.#deviceSecrets = undefined
    this.#wrappingKey = undefined
    this.#salt = undefined
    this.#identityId = undefined
    this.#credentialId = undefined
    this.#deviceId = undefined
    this.#persistence = 'locked'
  }
}
