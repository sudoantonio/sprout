import type { EncryptedPayloadDto, Uuid } from '../api/contracts'
import type { ResourceKind } from '../domain/models'
import type { RecoverySecretResult } from '../../public/wasm/sprout_crypto'

export type GeneratedSproutCryptoModule =
  typeof import('../../public/wasm/sprout_crypto')
export type SproutCryptoModule = GeneratedSproutCryptoModule

export interface DeviceSecrets {
  keyVersion: number
  suiteVersion: number
  publicPackage: Uint8Array
  x25519PrivateKey: Uint8Array
  mlKem768PrivateKey: Uint8Array
  ed25519PrivateKey: Uint8Array
  mlDsa65PrivateKey: Uint8Array
}

interface ResourceKeyCipherBundle {
  wrapping_mode: 'resource_key_aes_gcm_v1'
  payload_b64: string
  wrapped_dek_b64: string
  wrap_nonce_b64: string
}

interface HybridCipherBundle {
  wrapping_mode: 'hybrid_device_v1'
  payload_b64: string
  wrapped_dek_b64: string
  wrapped_key_suite_version: number
  wrapped_key_audit_status: typeof PRODUCTION_AUDIT_REQUIRED
}

type CipherBundle = ResourceKeyCipherBundle | HybridCipherBundle

export const PRODUCTION_AUDIT_REQUIRED =
  'production_audit_required' as const

export interface HybridKeyMetadata {
  resourceId: Uuid
  recipientDeviceId: Uuid
  resourceEpoch: number | bigint
  previousEpochHash: Uint8Array
  context: string | Uint8Array
}

export interface HybridRecipientPublicKeys {
  x25519PublicKey: Uint8Array
  mlKem768PublicKey: Uint8Array
}

export interface HybridRecipientPrivateKeys {
  x25519PrivateKey: Uint8Array
  mlKem768PrivateKey: Uint8Array
}

export interface HybridDocumentEncryptionRecipient
  extends HybridRecipientPublicKeys {
  deviceId: Uuid
}

export interface HybridDocumentDecryptionRecipient
  extends HybridRecipientPrivateKeys {
  deviceId: Uuid
}

export interface HybridWrappedResourceKey {
  envelope: Uint8Array
  suiteVersion: number
  auditStatus: typeof PRODUCTION_AUDIT_REQUIRED
}

export interface HybridUnwrappedResourceKey {
  resourceKey: Uint8Array
  auditStatus: typeof PRODUCTION_AUDIT_REQUIRED
}

const encoder = new TextEncoder()
const decoder = new TextDecoder()
const DEFAULT_RECOVERY_CONTEXT = 'sprout/recovery/n-of-n/v1'
const AES_GCM_NONCE_BYTES = 12

const asArrayBuffer = (value: Uint8Array): ArrayBuffer =>
  value.buffer.slice(
    value.byteOffset,
    value.byteOffset + value.byteLength,
  ) as ArrayBuffer

const importResourceWrappingKey = (resourceKey: Uint8Array): Promise<CryptoKey> => {
  if (resourceKey.byteLength !== 32) {
    throw new Error('Resource wrapping keys must contain 256 bits')
  }
  return crypto.subtle.importKey(
    'raw',
    asArrayBuffer(resourceKey),
    { name: 'AES-GCM' },
    false,
    ['encrypt', 'decrypt'],
  )
}

const copyContext = (context: string | Uint8Array): Uint8Array =>
  typeof context === 'string' ? encoder.encode(context) : context.slice()

const requireAuditStatus = (
  status: string,
): typeof PRODUCTION_AUDIT_REQUIRED => {
  if (status !== PRODUCTION_AUDIT_REQUIRED) {
    throw new Error(`Unsupported hybrid suite audit status: ${status}`)
  }
  return status
}

const requiredExports: Array<keyof SproutCryptoModule> = [
  'initialize',
  'hash',
  'canonicalHeader',
  'encrypt',
  'decrypt',
  'generateDevicePackage',
  'signDual',
  'verifyDual',
  'wrapResourceKey',
  'unwrapResourceKey',
  'splitRecoverySecretNOfN',
  'combineRecoverySecretNOfN',
  'combineRecoverySecretBundleNOfN',
  'RecoveryShareSet',
]

let modulePromise: Promise<SproutCryptoModule> | undefined
let testModule: SproutCryptoModule | undefined

export const zeroBytes = (...values: Array<Uint8Array | undefined>): void => {
  for (const value of values) {
    value?.fill(0)
  }
}

export const bytesToBase64 = (bytes: Uint8Array): string => {
  let value = ''
  for (const byte of bytes) {
    value += String.fromCharCode(byte)
  }
  return btoa(value)
}

export const base64ToBytes = (value: string): Uint8Array => {
  const decoded = atob(value)
  return Uint8Array.from(decoded, (character) => character.charCodeAt(0))
}

export const uuidToBytes = (uuid: Uuid): Uint8Array => {
  const normalized = uuid.replaceAll('-', '')
  if (!/^[0-9a-f]{32}$/i.test(normalized)) {
    throw new Error('Expected a UUID identifier')
  }
  return Uint8Array.from(
    normalized.match(/.{2}/g) ?? [],
    (pair) => Number.parseInt(pair, 16),
  )
}

const assertExports = (module: SproutCryptoModule): SproutCryptoModule => {
  const missing = requiredExports.filter(
    (name) => typeof module[name] !== 'function',
  )
  if (missing.length > 0) {
    throw new Error(`Crypto WASM is missing exports: ${missing.join(', ')}`)
  }
  return module
}

const acceptGeneratedAndLegacyVerifyOrder = (
  module: SproutCryptoModule,
): SproutCryptoModule => ({
  ...module,
  verifyDual: (
    ed25519PublicKey,
    second,
    third,
    fourth,
    fifth,
    sixth,
  ) => {
    const legacyCall =
      second.byteLength === 1_952 && fifth.byteLength === 64
    return legacyCall
      ? module.verifyDual(
          ed25519PublicKey,
          fifth,
          second,
          sixth,
          third,
          fourth,
        )
      : module.verifyDual(
          ed25519PublicKey,
          second,
          third,
          fourth,
          fifth,
          sixth,
        )
  },
})

export const loadCrypto = async (): Promise<SproutCryptoModule> => {
  if (testModule) {
    return testModule
  }
  modulePromise ??= (async () => {
    // Vite 8 refuses root-absolute dynamic imports that resolve into public/.
    // An origin-qualified URL is treated as an external module URL instead.
    const origin =
      typeof globalThis.location?.origin === 'string' &&
      globalThis.location.origin !== 'null'
        ? globalThis.location.origin
        : ''
    const moduleUrl = `${origin}/wasm/sprout_crypto.js`
    const imported = (await import(
      /* @vite-ignore */ moduleUrl
    )) as SproutCryptoModule
    await imported.default?.({
      module_or_path: `${origin}/wasm/sprout_crypto_bg.wasm`,
    })
    const module = acceptGeneratedAndLegacyVerifyOrder(
      assertExports(imported),
    )
    module.initialize()
    return module
  })()
  return modulePromise
}

export const configureCryptoModuleForTests = (
  module?: SproutCryptoModule,
): void => {
  testModule = module
  modulePromise = undefined
}

const RESOURCE_PAYLOAD_CONTENT_KIND = 1

// Builds before the content-kind boundary was corrected wrote the application
// resource kind into this byte. Values 2-4 happened to be accepted as other
// protocol content kinds, so keep a read-only fallback for those payloads.
const legacyContentKind = (kind: ResourceKind): number | undefined =>
  (
    {
      'agent-chat': undefined,
      project: undefined,
      topic: 2,
      'task-list': 3,
      task: 4,
      preset: undefined,
      recurrence: undefined,
      questionnaire: undefined,
      attachment: undefined,
    } satisfies Record<ResourceKind, number | undefined>
  )[kind]

export const resourceAad = (
  projectId: Uuid,
  resourceId: Uuid,
  kind: ResourceKind,
  aggregateVersion: number,
  keyEpoch: number,
): Uint8Array =>
  encoder.encode(
    `sprout/v1/${projectId}/${resourceId}/${kind}/${aggregateVersion}/${keyEpoch}`,
  )

const canonicalHeader = (
  module: SproutCryptoModule,
  resourceId: Uuid,
  keyId: Uuid,
  contentKind: number,
  aggregateVersion: number,
  previousHash: Uint8Array,
  aad: Uint8Array,
): Uint8Array =>
  module.canonicalHeader(
    1,
    1,
    contentKind,
    uuidToBytes(resourceId),
    uuidToBytes(keyId),
    BigInt(
      previousHash.every((byte) => byte === 0)
        ? 0
        : aggregateVersion,
    ),
    previousHash,
    aad,
  )

export const encryptDocument = async <T>(
  document: T,
  options: {
    projectId: Uuid
    resourceId: Uuid
    keyId: Uuid
    kind: ResourceKind
    aggregateVersion: number
    keyEpoch: number
    previousHash?: Uint8Array
    resourceKey: Uint8Array
    hybridRecipient?: HybridDocumentEncryptionRecipient
  },
): Promise<EncryptedPayloadDto> => {
  const module = await loadCrypto()
  const previousHash = options.previousHash ?? new Uint8Array(32)
  const aad = resourceAad(
    options.projectId,
    options.resourceId,
    options.kind,
    options.aggregateVersion,
    options.keyEpoch,
  )
  const header = canonicalHeader(
    module,
    options.resourceId,
    options.keyId,
    RESOURCE_PAYLOAD_CONTENT_KIND,
    options.aggregateVersion,
    previousHash,
    aad,
  )
  const plaintext = encoder.encode(JSON.stringify(document))
  const result = module.encrypt(header, plaintext)
  const dek = result.dek
  const payload = result.payload
  let wrappedDek: Uint8Array | undefined
  let wrapNonce: Uint8Array | undefined
  try {
    let bundle: CipherBundle
    if (options.hybridRecipient) {
      const wrapped = await wrapResourceKeyForRecipient(
        dek,
        options.hybridRecipient,
        {
          resourceId: options.resourceId,
          recipientDeviceId: options.hybridRecipient.deviceId,
          resourceEpoch: options.keyEpoch,
          previousEpochHash: previousHash,
          context: aad,
        },
      )
      wrappedDek = wrapped.envelope
      bundle = {
        wrapping_mode: 'hybrid_device_v1',
        payload_b64: bytesToBase64(payload),
        wrapped_dek_b64: bytesToBase64(wrappedDek),
        wrapped_key_suite_version: wrapped.suiteVersion,
        wrapped_key_audit_status: wrapped.auditStatus,
      }
    } else {
      wrapNonce = crypto.getRandomValues(
        new Uint8Array(AES_GCM_NONCE_BYTES),
      )
      wrappedDek = new Uint8Array(
        await crypto.subtle.encrypt(
          {
            name: 'AES-GCM',
            iv: asArrayBuffer(wrapNonce),
            additionalData: asArrayBuffer(header),
            tagLength: 128,
          },
          await importResourceWrappingKey(options.resourceKey),
          asArrayBuffer(dek),
        ),
      )
      bundle = {
        wrapping_mode: 'resource_key_aes_gcm_v1',
        payload_b64: bytesToBase64(payload),
        wrapped_dek_b64: bytesToBase64(wrappedDek),
        wrap_nonce_b64: bytesToBase64(wrapNonce),
      }
    }
    const nonceMarker = module.hash(payload).slice(0, 12)
    try {
      return {
        version: 1,
        algorithm: 'sprout-protocol-v1',
        key_id: options.keyId,
        nonce_b64: bytesToBase64(nonceMarker),
        ciphertext_b64: bytesToBase64(
          encoder.encode(JSON.stringify(bundle)),
        ),
      }
    } finally {
      zeroBytes(nonceMarker)
    }
  } finally {
    result.destroy()
    zeroBytes(
      plaintext,
      dek,
      payload,
      wrappedDek,
      wrapNonce,
      header,
      aad,
    )
  }
}

type DocumentDecryptionOptions = {
  projectId: Uuid
  resourceId: Uuid
  kind: ResourceKind
  aggregateVersion: number
  keyEpoch: number
  previousHash?: Uint8Array
  resourceKey: Uint8Array
  hybridRecipient?: HybridDocumentDecryptionRecipient
}

const decryptDocumentWithContentKind = async <T>(
  encrypted: EncryptedPayloadDto,
  options: DocumentDecryptionOptions,
  contentKind: number,
): Promise<T> => {
  if (
    encrypted.version !== 1 ||
    encrypted.algorithm !== 'sprout-protocol-v1'
  ) {
    throw new Error('Unsupported encrypted document format')
  }
  const module = await loadCrypto()
  const previousHash = options.previousHash ?? new Uint8Array(32)
  const aad = resourceAad(
    options.projectId,
    options.resourceId,
    options.kind,
    options.aggregateVersion,
    options.keyEpoch,
  )
  const header = canonicalHeader(
    module,
    options.resourceId,
    encrypted.key_id,
    contentKind,
    options.aggregateVersion,
    previousHash,
    aad,
  )
  const bundleBytes = base64ToBytes(encrypted.ciphertext_b64)
  const bundle = JSON.parse(decoder.decode(bundleBytes)) as CipherBundle
  const wrappedDek = base64ToBytes(bundle.wrapped_dek_b64)
  const payload = base64ToBytes(bundle.payload_b64)
  let dek: Uint8Array | undefined
  let plaintext: Uint8Array | undefined
  let wrapNonce: Uint8Array | undefined
  try {
    if (bundle.wrapping_mode === 'hybrid_device_v1') {
      if (
        !options.hybridRecipient ||
        bundle.wrapped_key_suite_version !== 0x8001 ||
        bundle.wrapped_key_audit_status !== PRODUCTION_AUDIT_REQUIRED
      ) {
        throw new Error('Unsupported wrapped document key suite')
      }
      const unwrapped = await unwrapResourceKeyForRecipient(
        wrappedDek,
        options.hybridRecipient,
        {
          resourceId: options.resourceId,
          recipientDeviceId: options.hybridRecipient.deviceId,
          resourceEpoch: options.keyEpoch,
          previousEpochHash: previousHash,
          context: aad,
        },
      )
      dek = unwrapped.resourceKey
    } else if (bundle.wrapping_mode === 'resource_key_aes_gcm_v1') {
      wrapNonce = base64ToBytes(bundle.wrap_nonce_b64)
      if (wrapNonce.byteLength !== AES_GCM_NONCE_BYTES) {
        throw new Error('Invalid resource-key wrapping nonce')
      }
      dek = new Uint8Array(
        await crypto.subtle.decrypt(
          {
            name: 'AES-GCM',
            iv: asArrayBuffer(wrapNonce),
            additionalData: asArrayBuffer(header),
            tagLength: 128,
          },
          await importResourceWrappingKey(options.resourceKey),
          asArrayBuffer(wrappedDek),
        ),
      )
    } else {
      throw new Error('Unsupported document key wrapping mode')
    }
    plaintext = module.decrypt(dek, payload, header)
    return JSON.parse(decoder.decode(plaintext)) as T
  } finally {
    zeroBytes(
      bundleBytes,
      wrappedDek,
      payload,
      dek,
      plaintext,
      wrapNonce,
      header,
      aad,
    )
  }
}

export const decryptDocument = async <T>(
  encrypted: EncryptedPayloadDto,
  options: DocumentDecryptionOptions,
): Promise<T> => {
  try {
    return await decryptDocumentWithContentKind<T>(
      encrypted,
      options,
      RESOURCE_PAYLOAD_CONTENT_KIND,
    )
  } catch (error) {
    const legacy = legacyContentKind(options.kind)
    if (legacy === undefined) throw error
    try {
      return await decryptDocumentWithContentKind<T>(
        encrypted,
        options,
        legacy,
      )
    } catch {
      throw error
    }
  }
}

export const generateDeviceSecrets = async (
  deviceId: Uuid,
): Promise<DeviceSecrets> => {
  const module = await loadCrypto()
  const result = module.generateDevicePackage(
    uuidToBytes(deviceId),
    uuidToBytes(crypto.randomUUID()),
    uuidToBytes(crypto.randomUUID()),
    uuidToBytes(crypto.randomUUID()),
    uuidToBytes(crypto.randomUUID()),
  )
  try {
    return {
      keyVersion: 1,
      suiteVersion: result.suiteVersion,
      publicPackage: result.publicPackage,
      x25519PrivateKey: result.x25519PrivateKey,
      mlKem768PrivateKey: result.mlKem768PrivateKey,
      ed25519PrivateKey: result.ed25519PrivateKey,
      mlDsa65PrivateKey: result.mlDsa65PrivateKey,
    }
  } finally {
    result.destroy()
  }
}

export const signDual = async (
  secrets: Pick<
    DeviceSecrets,
    'ed25519PrivateKey' | 'mlDsa65PrivateKey'
  >,
  message: Uint8Array,
  context: string,
): Promise<{
  classicalSignature: Uint8Array
  postQuantumSignature: Uint8Array
}> => {
  const module = await loadCrypto()
  const contextBytes = encoder.encode(context)
  const result = module.signDual(
    secrets.ed25519PrivateKey,
    secrets.mlDsa65PrivateKey,
    message,
    contextBytes,
  )
  try {
    return {
      classicalSignature: result.ed25519,
      postQuantumSignature: result.mlDsa65,
    }
  } finally {
    result.destroy()
    zeroBytes(contextBytes)
  }
}

export const verifyDualSignature = async (
  publicKeys: {
    ed25519PublicKey: Uint8Array
    mlDsa65PublicKey: Uint8Array
  },
  signatures: {
    classicalSignature: Uint8Array
    postQuantumSignature: Uint8Array
  },
  message: Uint8Array,
  context: string,
): Promise<boolean> => {
  const module = await loadCrypto()
  const contextBytes = encoder.encode(context)
  try {
    return module.verifyDual(
      publicKeys.ed25519PublicKey,
      signatures.classicalSignature,
      publicKeys.mlDsa65PublicKey,
      signatures.postQuantumSignature,
      message,
      contextBytes,
    )
  } finally {
    zeroBytes(contextBytes)
  }
}

export const wrapResourceKeyForRecipient = async (
  resourceKey: Uint8Array,
  recipient: HybridRecipientPublicKeys,
  metadata: HybridKeyMetadata,
): Promise<HybridWrappedResourceKey> => {
  const module = await loadCrypto()
  const resourceId = uuidToBytes(metadata.resourceId)
  const recipientDeviceId = uuidToBytes(metadata.recipientDeviceId)
  const previousEpochHash = metadata.previousEpochHash.slice()
  const context = copyContext(metadata.context)
  const result = module.wrapResourceKey(
    resourceKey,
    recipient.x25519PublicKey,
    recipient.mlKem768PublicKey,
    resourceId,
    recipientDeviceId,
    BigInt(metadata.resourceEpoch),
    previousEpochHash,
    context,
  )
  try {
    return {
      envelope: result.envelope,
      suiteVersion: result.suiteVersion,
      auditStatus: requireAuditStatus(result.auditStatus),
    }
  } finally {
    result.destroy()
    zeroBytes(
      resourceId,
      recipientDeviceId,
      previousEpochHash,
      context,
    )
  }
}

export const unwrapResourceKeyForRecipient = async (
  envelope: Uint8Array,
  recipient: HybridRecipientPrivateKeys,
  metadata: HybridKeyMetadata,
): Promise<HybridUnwrappedResourceKey> => {
  const module = await loadCrypto()
  const resourceId = uuidToBytes(metadata.resourceId)
  const recipientDeviceId = uuidToBytes(metadata.recipientDeviceId)
  const previousEpochHash = metadata.previousEpochHash.slice()
  const context = copyContext(metadata.context)
  const result = module.unwrapResourceKey(
    envelope,
    recipient.x25519PrivateKey,
    recipient.mlKem768PrivateKey,
    resourceId,
    recipientDeviceId,
    BigInt(metadata.resourceEpoch),
    previousEpochHash,
    context,
  )
  try {
    return {
      resourceKey: result.resourceKey,
      auditStatus: requireAuditStatus(result.auditStatus),
    }
  } finally {
    result.destroy()
    zeroBytes(
      resourceId,
      recipientDeviceId,
      previousEpochHash,
      context,
    )
  }
}

export const splitRecoverySecretWithCommitment = async (
  secret: Uint8Array,
  participantCount: number,
  recoveryContext: string | Uint8Array = DEFAULT_RECOVERY_CONTEXT,
): Promise<{ shares: Uint8Array[]; commitment: Uint8Array }> => {
  const module = await loadCrypto()
  const context = copyContext(recoveryContext)
  const result = module.splitRecoverySecretNOfN(
    secret,
    participantCount,
    context,
  )
  try {
    return {
      shares: Array.from(
        { length: result.shareCount },
        (_, position) => result.share(position),
      ),
      commitment: result.commitment,
    }
  } finally {
    result.destroy()
    zeroBytes(context)
  }
}

export const splitRecoverySecret = async (
  secret: Uint8Array,
  participantCount: number,
  recoveryContext: string | Uint8Array = DEFAULT_RECOVERY_CONTEXT,
): Promise<Uint8Array[]> => {
  const { shares } = await splitRecoverySecretWithCommitment(
    secret,
    participantCount,
    recoveryContext,
  )
  return shares
}

export const combineRecoverySecret = async (
  shares: Uint8Array[],
  recoveryContext: string | Uint8Array = DEFAULT_RECOVERY_CONTEXT,
): Promise<Uint8Array> => {
  const module = await loadCrypto()
  const context = copyContext(recoveryContext)
  const shareSet = new module.RecoveryShareSet()
  let result: RecoverySecretResult | undefined
  try {
    for (const share of shares) {
      shareSet.addShare(share)
    }
    result = module.combineRecoverySecretNOfN(shareSet, context)
    return result.secret
  } finally {
    result?.destroy()
    shareSet.destroy()
    zeroBytes(context)
  }
}
