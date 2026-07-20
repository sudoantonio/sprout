import type {
  ProjectDeviceKeyPackage,
  ProjectRecoveryShareInputDto,
  ProvisionProjectRecoveryRequest,
  Uuid,
} from '../api/contracts'
import type { KeyVault } from '../security/key-vault'
import {
  base64ToBytes,
  bytesToBase64,
  splitRecoverySecretWithCommitment,
  unwrapResourceKeyForRecipient,
  wrapResourceKeyForRecipient,
  zeroBytes,
} from '../security/wasm'

const encoder = new TextEncoder()
const CONTEXT_HASH_OFFSET = 11
const SHARE_COMMITMENT_OFFSET = 75

export const recoveryUnprovisionedMessage =
  'Owner recovery is unprovisioned. Provision n-of-n shares before a device can be recovered.'

export const ownerOnlyRecoveryWarning =
  'This project has no eligible non-owner participants. Unanimous owner recovery is impossible until at least one participant device can hold a share.'

export const unreachableParticipantWarning =
  'One unreachable participant or missing share makes unanimous recovery impossible by design.'

const asArrayBuffer = (value: Uint8Array): ArrayBuffer =>
  value.buffer.slice(
    value.byteOffset,
    value.byteOffset + value.byteLength,
  ) as ArrayBuffer

const uuidBytes = (value: Uuid): Uint8Array => {
  const normalized = value.replaceAll('-', '')
  if (!/^[0-9a-fA-F]{32}$/.test(normalized)) {
    throw new Error('Invalid UUID in recovery context')
  }
  return Uint8Array.from(
    normalized.match(/.{2}/g) as string[],
    (byte) => Number.parseInt(byte, 16),
  )
}

const i64Be = (value: number): Uint8Array => {
  if (!Number.isSafeInteger(value)) {
    throw new Error('Invalid recovery epoch integer')
  }
  const bytes = new Uint8Array(8)
  new DataView(bytes.buffer).setBigInt64(0, BigInt(value), false)
  return bytes
}

const concat = (...values: Uint8Array[]): Uint8Array => {
  const output = new Uint8Array(
    values.reduce((total, value) => total + value.byteLength, 0),
  )
  let offset = 0
  for (const value of values) {
    output.set(value, offset)
    offset += value.byteLength
  }
  return output
}

/** Stable provision context bound to project + recovery/membership epochs. */
export const buildProvisionContext = (
  projectId: Uuid,
  recoveryEpoch: number,
  membershipEpoch: number,
): Uint8Array =>
  concat(
    encoder.encode('sprout/project-recovery/provision/v1'),
    uuidBytes(projectId),
    i64Be(recoveryEpoch),
    i64Be(membershipEpoch),
  )

export const shareCommitmentFromEncodedShare = (share: Uint8Array): Uint8Array => {
  if (share.byteLength !== 171) {
    throw new Error('Encoded recovery share has unexpected length')
  }
  return share.slice(SHARE_COMMITMENT_OFFSET, SHARE_COMMITMENT_OFFSET + 32)
}

export const contextHashFromEncodedShare = (share: Uint8Array): Uint8Array => {
  if (share.byteLength !== 171) {
    throw new Error('Encoded recovery share has unexpected length')
  }
  return share.slice(CONTEXT_HASH_OFFSET, CONTEXT_HASH_OFFSET + 32)
}

const aesGcmEncrypt = async (
  keyBytes: Uint8Array,
  plaintext: Uint8Array,
  aad: Uint8Array,
): Promise<Uint8Array> => {
  const nonce = crypto.getRandomValues(new Uint8Array(12))
  const key = await crypto.subtle.importKey(
    'raw',
    asArrayBuffer(keyBytes),
    'AES-GCM',
    false,
    ['encrypt'],
  )
  const ciphertext = new Uint8Array(
    await crypto.subtle.encrypt(
      {
        name: 'AES-GCM',
        iv: asArrayBuffer(nonce),
        additionalData: asArrayBuffer(aad),
        tagLength: 128,
      },
      key,
      asArrayBuffer(plaintext),
    ),
  )
  return concat(nonce, ciphertext)
}

const aesGcmDecrypt = async (
  keyBytes: Uint8Array,
  envelope: Uint8Array,
  aad: Uint8Array,
): Promise<Uint8Array> => {
  if (envelope.byteLength <= 12) {
    throw new Error('Encrypted recovery payload is truncated')
  }
  const nonce = envelope.slice(0, 12)
  const ciphertext = envelope.slice(12)
  const key = await crypto.subtle.importKey(
    'raw',
    asArrayBuffer(keyBytes),
    'AES-GCM',
    false,
    ['decrypt'],
  )
  return new Uint8Array(
    await crypto.subtle.decrypt(
      {
        name: 'AES-GCM',
        iv: asArrayBuffer(nonce),
        additionalData: asArrayBuffer(aad),
        tagLength: 128,
      },
      key,
      asArrayBuffer(ciphertext),
    ),
  )
}

interface HolderDevice {
  identityId: Uuid
  deviceId: Uuid
  keyVersion: number
  x25519PublicKey: Uint8Array
  mlKem768PublicKey: Uint8Array
}

const parseHolderDevices = async (
  packages: ProjectDeviceKeyPackage[],
  holderIdentityIds: Uuid[],
): Promise<HolderDevice[]> => {
  const wanted = new Set(holderIdentityIds)
  const holders: HolderDevice[] = []
  for (const item of packages) {
    if (!wanted.has(item.identity_id)) continue
    const packageBytes = base64ToBytes(item.package_b64)
    try {
      const parsed = JSON.parse(new TextDecoder().decode(packageBytes)) as {
        encryption_keys?: Array<{
          algorithm: string
          public_key: number[]
        }>
      }
      const x25519 = parsed.encryption_keys?.find(
        (key) => key.algorithm === 'x25519',
      )
      const mlKem768 = parsed.encryption_keys?.find(
        (key) => key.algorithm === 'ml_kem768_experimental',
      )
      if (!x25519 || !mlKem768) {
        throw new Error('Device package is missing hybrid encryption keys')
      }
      holders.push({
        identityId: item.identity_id,
        deviceId: item.device_id,
        keyVersion: item.key_version,
        x25519PublicKey: Uint8Array.from(x25519.public_key),
        mlKem768PublicKey: Uint8Array.from(mlKem768.public_key),
      })
    } finally {
      zeroBytes(packageBytes)
    }
  }
  for (const identityId of holderIdentityIds) {
    if (!holders.some((holder) => holder.identityId === identityId)) {
      throw new Error(
        `Participant ${identityId} has no trusted device package for recovery provisioning`,
      )
    }
  }
  return holders
}

const wrapShareForHolder = async (
  share: Uint8Array,
  holder: HolderDevice,
  recoverySetId: Uuid,
  recoveryEpoch: number,
  provisionContext: Uint8Array,
): Promise<Uint8Array> => {
  const wrapKey = crypto.getRandomValues(new Uint8Array(32))
  let sealedShare: Uint8Array | undefined
  let hybridEnvelope: Uint8Array | undefined
  try {
    sealedShare = await aesGcmEncrypt(wrapKey, share, provisionContext)
    const hybrid = await wrapResourceKeyForRecipient(
      wrapKey,
      {
        x25519PublicKey: holder.x25519PublicKey,
        mlKem768PublicKey: holder.mlKem768PublicKey,
      },
      {
        resourceId: recoverySetId,
        recipientDeviceId: holder.deviceId,
        resourceEpoch: recoveryEpoch,
        previousEpochHash: new Uint8Array(32),
        context: provisionContext,
      },
    )
    hybridEnvelope = hybrid.envelope
    const hybridLength = new Uint8Array(4)
    new DataView(hybridLength.buffer).setUint32(
      0,
      hybridEnvelope.byteLength,
      false,
    )
    return concat(hybridLength, hybridEnvelope, sealedShare)
  } finally {
    zeroBytes(wrapKey, sealedShare, hybridEnvelope)
  }
}

export const unwrapShareEnvelope = async (
  vault: KeyVault,
  envelope: Uint8Array,
  recoverySetId: Uuid,
  recoveryEpoch: number,
  provisionContext: Uint8Array,
): Promise<Uint8Array> => {
  const deviceId = vault.localDeviceId
  if (!deviceId) {
    throw new Error('Device identity is required to unwrap a recovery share')
  }
  if (envelope.byteLength < 4) {
    throw new Error('Recovery share envelope is truncated')
  }
  const hybridLength = new DataView(
    envelope.buffer,
    envelope.byteOffset,
    4,
  ).getUint32(0, false)
  const hybrid = envelope.slice(4, 4 + hybridLength)
  const sealedShare = envelope.slice(4 + hybridLength)
  const unwrapped = await unwrapResourceKeyForRecipient(
    hybrid,
    {
      x25519PrivateKey: vault.deviceSecrets.x25519PrivateKey,
      mlKem768PrivateKey: vault.deviceSecrets.mlKem768PrivateKey,
    },
    {
      resourceId: recoverySetId,
      recipientDeviceId: deviceId,
      resourceEpoch: recoveryEpoch,
      previousEpochHash: new Uint8Array(32),
      context: provisionContext,
    },
  )
  try {
    return await aesGcmDecrypt(
      unwrapped.resourceKey,
      sealedShare,
      provisionContext,
    )
  } finally {
    zeroBytes(unwrapped.resourceKey)
  }
}

export interface BuiltRecoveryProvision {
  request: ProvisionProjectRecoveryRequest
  secret: Uint8Array
}

/** Build a ciphertext-only recovery provision bundle for upload + activate. */
export const buildRecoveryProvisionBundle = async (input: {
  projectId: Uuid
  recoveryEpoch: number
  membershipEpoch: number
  ownerEscrowPlaintext: Uint8Array
  holderIdentityIds: Uuid[]
  packages: ProjectDeviceKeyPackage[]
}): Promise<BuiltRecoveryProvision> => {
  if (input.holderIdentityIds.length === 0) {
    throw new Error('Recovery provisioning requires at least one holder')
  }
  const recoverySetId = crypto.randomUUID()
  const provisionContext = buildProvisionContext(
    input.projectId,
    input.recoveryEpoch,
    input.membershipEpoch,
  )
  const secret = crypto.getRandomValues(new Uint8Array(32))
  let shares: Uint8Array[] = []
  let commitment: Uint8Array | undefined
  let escrow: Uint8Array | undefined
  let contextHash: Uint8Array | undefined
  try {
    const devices = await parseHolderDevices(
      input.packages,
      input.holderIdentityIds,
    )
    const primaryByIdentity = new Map<Uuid, HolderDevice>()
    for (const device of devices) {
      if (!primaryByIdentity.has(device.identityId)) {
        primaryByIdentity.set(device.identityId, device)
      }
    }
    const orderedHolders = input.holderIdentityIds.map((identityId) => {
      const holder = primaryByIdentity.get(identityId)
      if (!holder) {
        throw new Error(`Missing holder device for ${identityId}`)
      }
      return holder
    })
    const split = await splitRecoverySecretWithCommitment(
      secret,
      orderedHolders.length,
      provisionContext,
    )
    shares = split.shares
    commitment = split.commitment
    contextHash = contextHashFromEncodedShare(shares[0])
    escrow = await aesGcmEncrypt(
      secret,
      input.ownerEscrowPlaintext,
      provisionContext,
    )
    const shareInputs: ProjectRecoveryShareInputDto[] = []
    for (const [index, holder] of orderedHolders.entries()) {
      const share = shares[index]
      const shareCommitment = shareCommitmentFromEncodedShare(share)
      const encrypted = await wrapShareForHolder(
        share,
        holder,
        recoverySetId,
        input.recoveryEpoch,
        provisionContext,
      )
      try {
        shareInputs.push({
          share_id: crypto.randomUUID(),
          holder_identity_id: holder.identityId,
          holder_device_id: holder.deviceId,
          holder_device_key_version: holder.keyVersion,
          share_index: index + 1,
          encrypted_share_b64: bytesToBase64(encrypted),
          share_commitment_b64: bytesToBase64(shareCommitment),
        })
      } finally {
        zeroBytes(encrypted, shareCommitment)
      }
    }
    return {
      secret,
      request: {
        recovery_set_id: recoverySetId,
        recovery_epoch: input.recoveryEpoch,
        membership_epoch: input.membershipEpoch,
        secret_commitment_b64: bytesToBase64(commitment),
        context_hash_b64: bytesToBase64(contextHash),
        encrypted_owner_key_escrow_b64: bytesToBase64(escrow),
        shares: shareInputs,
      },
    }
  } catch (error) {
    zeroBytes(secret)
    throw error
  } finally {
    for (const share of shares) zeroBytes(share)
    zeroBytes(commitment, escrow, contextHash, provisionContext)
  }
}

export const openOwnerEscrow = async (
  secret: Uint8Array,
  encryptedOwnerEscrow: Uint8Array,
  projectId: Uuid,
  recoveryEpoch: number,
  membershipEpoch: number,
): Promise<Uint8Array> => {
  const context = buildProvisionContext(projectId, recoveryEpoch, membershipEpoch)
  try {
    return await aesGcmDecrypt(secret, encryptedOwnerEscrow, context)
  } finally {
    zeroBytes(context)
  }
}

export const encodeOwnerEscrowPlaintext = (
  resourceKeys: Array<{
    resourceId: Uuid
    epoch: number
    purpose: 'body' | 'header'
    key: Uint8Array
  }>,
): Uint8Array => {
  const document = {
    version: 1 as const,
    keys: resourceKeys.map((entry) => ({
      resource_id: entry.resourceId,
      epoch: entry.epoch,
      purpose: entry.purpose,
      key_b64: bytesToBase64(entry.key),
    })),
  }
  return encoder.encode(JSON.stringify(document))
}

export const decodeOwnerEscrowPlaintext = (
  plaintext: Uint8Array,
): Array<{
  resourceId: Uuid
  epoch: number
  purpose: 'body' | 'header'
  key: Uint8Array
}> => {
  const document = JSON.parse(new TextDecoder().decode(plaintext)) as {
    version: number
    keys: Array<{
      resource_id: Uuid
      epoch: number
      purpose: 'body' | 'header'
      key_b64: string
    }>
  }
  if (document.version !== 1 || !Array.isArray(document.keys)) {
    throw new Error('Unsupported owner escrow document')
  }
  return document.keys.map((entry) => ({
    resourceId: entry.resource_id,
    epoch: entry.epoch,
    purpose: entry.purpose,
    key: base64ToBytes(entry.key_b64),
  }))
}
