import type {
  ProjectDeviceKeyPackage,
  ResourceEpochInputDto,
  ResourceEpochRotationDto,
  ResourceKeyEnvelopeDto,
  ResourceKeyEnvelopeViewDto,
  Uuid,
} from '../api/contracts'
import type { KeyVault } from '../security/key-vault'
import {
  base64ToBytes,
  bytesToBase64,
  signDual,
  unwrapResourceKeyForRecipient,
  verifyDualSignature,
  wrapResourceKeyForRecipient,
  zeroBytes,
} from '../security/wasm'

const encoder = new TextEncoder()
const ENVELOPE_VERSION = 1
const RESOURCE_EPOCH = 1
const SIGNATURE_CONTEXT = 'sprout-resource-key-envelope-v2'

interface RecipientDevice {
  identityId: Uuid
  deviceId: Uuid
  keyVersion: number
  x25519PublicKey: Uint8Array
  mlKem768PublicKey: Uint8Array
}

export interface InitialResourceEpochBundle {
  epoch: ResourceEpochInputDto
  envelopes: ResourceKeyEnvelopeDto[]
}

export interface BuiltResourceEpochRotation {
  rotation: ResourceEpochRotationDto
  resourceKey: Uint8Array
  headerKey?: Uint8Array
}

const asArrayBuffer = (value: Uint8Array): ArrayBuffer =>
  value.buffer.slice(
    value.byteOffset,
    value.byteOffset + value.byteLength,
  ) as ArrayBuffer

const sha256 = async (value: Uint8Array): Promise<Uint8Array> =>
  new Uint8Array(await crypto.subtle.digest('SHA-256', asArrayBuffer(value)))

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

const uuidBytes = (value: Uuid): Uint8Array => {
  const normalized = value.replaceAll('-', '')
  if (!/^[0-9a-fA-F]{32}$/.test(normalized)) {
    throw new Error('Invalid UUID in resource envelope')
  }
  return Uint8Array.from(
    normalized.match(/.{2}/g) as string[],
    (byte) => Number.parseInt(byte, 16),
  )
}

const integerBytes = (value: number, size: 2 | 4): Uint8Array => {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error('Invalid resource envelope integer')
  }
  const bytes = new Uint8Array(size)
  const view = new DataView(bytes.buffer)
  if (size === 2) view.setUint16(0, value, false)
  else view.setUint32(0, value, false)
  return bytes
}

const equalBytes = (left: Uint8Array, right: Uint8Array): boolean => {
  if (left.byteLength !== right.byteLength) return false
  let difference = 0
  for (let index = 0; index < left.byteLength; index += 1) {
    difference |= left[index] ^ right[index]
  }
  return difference === 0
}

const recipientDevices = async (
  packages: ProjectDeviceKeyPackage[],
  identityId: Uuid,
): Promise<RecipientDevice[]> => {
  const recipients: RecipientDevice[] = []
  for (const item of packages) {
    if (item.identity_id !== identityId) continue
    const packageBytes = base64ToBytes(item.package_b64)
    const expectedHash = base64ToBytes(item.package_hash_b64)
    let actualHash: Uint8Array | undefined
    try {
      actualHash = await sha256(packageBytes)
      if (!equalBytes(actualHash, expectedHash)) {
        throw new Error('Device package transparency digest mismatch')
      }
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
      recipients.push({
        identityId,
        deviceId: item.device_id,
        keyVersion: item.key_version,
        x25519PublicKey: Uint8Array.from(x25519.public_key),
        mlKem768PublicKey: Uint8Array.from(mlKem768.public_key),
      })
    } finally {
      zeroBytes(packageBytes, expectedHash, actualHash)
    }
  }
  if (recipients.length === 0) {
    throw new Error('Permission recipient has no verified active device package')
  }
  return recipients
}

export const envelopeSigningMessage = async (
  projectId: Uuid,
  envelope: Omit<
    ResourceKeyEnvelopeDto,
    | 'encrypted_key_b64'
    | 'sender_signature_b64'
    | 'sender_post_quantum_signature_b64'
  >,
  encryptedKey: Uint8Array,
): Promise<Uint8Array> => {
  const encryptedHash = await sha256(encryptedKey)
  const legacyMessage = concat(
    encoder.encode(SIGNATURE_CONTEXT),
    uuidBytes(projectId),
    integerBytes(envelope.version, 2),
    uuidBytes(envelope.resource_id),
    integerBytes(envelope.epoch, 4),
    uuidBytes(envelope.recipient_identity_id),
    uuidBytes(envelope.recipient_device_id),
    integerBytes(envelope.recipient_device_key_version, 4),
    integerBytes(envelope.sender_device_key_version, 4),
    encryptedHash,
  )
  return envelope.key_purpose === 'header'
    ? concat(legacyMessage, encoder.encode('header'))
    : legacyMessage
}

const genesisHash = (projectId: Uuid, resourceId: Uuid): Promise<Uint8Array> =>
  sha256(
    encoder.encode(
      `sprout-resource-key-genesis-v1/${projectId}/${resourceId}`,
    ),
  )

const keyCommitment = (
  projectId: Uuid,
  resourceId: Uuid,
  resourceKey: Uint8Array,
): Promise<Uint8Array> =>
  sha256(
    concat(
      encoder.encode('sprout-resource-key-commitment-v1'),
      uuidBytes(projectId),
      uuidBytes(resourceId),
      resourceKey,
    ),
  )

const envelopeContext = (
  projectId: Uuid,
  resourceId: Uuid,
  identityId: Uuid,
  deviceId: Uuid,
  purpose: 'body' | 'header',
): string =>
  `sprout/resource-envelope/v2/${projectId}/${resourceId}/${identityId}/${deviceId}` +
  (purpose === 'header' ? '/header' : '')

export const buildResourceKeyEnvelopes = async (
  vault: KeyVault,
  input: {
    projectId: Uuid
    resourceId: Uuid
    resourceKey: Uint8Array
    keyPurpose?: 'body' | 'header'
    recipientIdentityId: Uuid
    packages: ProjectDeviceKeyPackage[]
    epoch?: number
    previousEpochHash?: Uint8Array
  },
): Promise<ResourceKeyEnvelopeDto[]> => {
  const recipients = await recipientDevices(
    input.packages,
    input.recipientIdentityId,
  )
  const epoch = input.epoch ?? RESOURCE_EPOCH
  const keyPurpose = input.keyPurpose ?? 'body'
  const previousEpochHash = input.previousEpochHash
    ? input.previousEpochHash.slice()
    : epoch === RESOURCE_EPOCH
      ? await genesisHash(input.projectId, input.resourceId)
      : (() => {
          throw new Error('Rotated envelopes require the previous epoch hash')
        })()
  const envelopes: ResourceKeyEnvelopeDto[] = []
  try {
    for (const recipient of recipients) {
      let encryptedKey: Uint8Array | undefined
      let signingMessage: Uint8Array | undefined
      try {
        const wrapped = await wrapResourceKeyForRecipient(
          input.resourceKey,
          recipient,
          {
            resourceId: input.resourceId,
            recipientDeviceId: recipient.deviceId,
            resourceEpoch: epoch,
            previousEpochHash,
            context: envelopeContext(
              input.projectId,
              input.resourceId,
              recipient.identityId,
              recipient.deviceId,
              keyPurpose,
            ),
          },
        )
        encryptedKey = wrapped.envelope
        const unsigned = {
          version: ENVELOPE_VERSION,
          resource_id: input.resourceId,
          epoch,
          key_purpose: keyPurpose,
          recipient_identity_id: recipient.identityId,
          recipient_device_id: recipient.deviceId,
          recipient_device_key_version: recipient.keyVersion,
          sender_device_key_version: vault.deviceSecrets.keyVersion,
        }
        signingMessage = await envelopeSigningMessage(
          input.projectId,
          unsigned,
          encryptedKey,
        )
        const signatures = await signDual(
          vault.deviceSecrets,
          signingMessage,
          SIGNATURE_CONTEXT,
        )
        try {
          envelopes.push({
            ...unsigned,
            encrypted_key_b64: bytesToBase64(encryptedKey),
            sender_signature_b64: bytesToBase64(
              signatures.classicalSignature,
            ),
            sender_post_quantum_signature_b64: bytesToBase64(
              signatures.postQuantumSignature,
            ),
          })
        } finally {
          zeroBytes(
            signatures.classicalSignature,
            signatures.postQuantumSignature,
          )
        }
      } finally {
        zeroBytes(encryptedKey, signingMessage)
      }
    }
    return envelopes
  } finally {
    zeroBytes(
      previousEpochHash,
      ...recipients.flatMap((recipient) => [
        recipient.x25519PublicKey,
        recipient.mlKem768PublicKey,
      ]),
    )
  }
}

export const buildResourceEpochRotation = async (
  vault: KeyVault,
  input: {
    projectId: Uuid
    resourceId: Uuid
    previousEpochId: Uuid
    currentEpoch: number
    previousKeyCommitment: Uint8Array
    previousHeaderKeyCommitment?: Uint8Array
    recipientIdentityIds: Uuid[]
    bodyRecipientIdentityIds?: Uuid[]
    headerRecipientIdentityIds?: Uuid[]
    packages: ProjectDeviceKeyPackage[]
  },
): Promise<BuiltResourceEpochRotation> => {
  const newEpoch = input.currentEpoch + 1
  const resourceKey = crypto.getRandomValues(new Uint8Array(32))
  const headerKey = input.previousHeaderKeyCommitment
    ? crypto.getRandomValues(new Uint8Array(32))
    : undefined
  let commitment: Uint8Array | undefined
  let headerCommitment: Uint8Array | undefined
  try {
    commitment = await keyCommitment(
      input.projectId,
      input.resourceId,
      resourceKey,
    )
    headerCommitment = headerKey
      ? await keyCommitment(input.projectId, input.resourceId, headerKey)
      : undefined
    const bodyEnvelopes = (
      await Promise.all(
        (input.bodyRecipientIdentityIds ?? input.recipientIdentityIds).map((recipientIdentityId) =>
          buildResourceKeyEnvelopes(vault, {
            projectId: input.projectId,
            resourceId: input.resourceId,
            resourceKey,
            recipientIdentityId,
            packages: input.packages,
            epoch: newEpoch,
            previousEpochHash: input.previousKeyCommitment,
          }),
        ),
      )
    ).flat()
    const headerEnvelopes = headerKey
      ? (
          await Promise.all(
            (input.headerRecipientIdentityIds ?? input.recipientIdentityIds).map((recipientIdentityId) =>
              buildResourceKeyEnvelopes(vault, {
                projectId: input.projectId,
                resourceId: input.resourceId,
                resourceKey: headerKey,
                keyPurpose: 'header',
                recipientIdentityId,
                packages: input.packages,
                epoch: newEpoch,
                previousEpochHash: input.previousHeaderKeyCommitment,
              }),
            ),
          )
        ).flat()
      : []
    return {
      resourceKey,
      headerKey,
      rotation: {
        epoch_id: crypto.randomUUID(),
        resource_id: input.resourceId,
        previous_epoch_id: input.previousEpochId,
        new_epoch: newEpoch,
        creator_device_key_version: vault.deviceSecrets.keyVersion,
        key_commitment_b64: bytesToBase64(commitment),
        header_key_commitment_b64: headerCommitment
          ? bytesToBase64(headerCommitment)
          : null,
        envelopes: [...bodyEnvelopes, ...headerEnvelopes],
      },
    }
  } catch (error) {
    zeroBytes(resourceKey, headerKey)
    throw error
  } finally {
    zeroBytes(commitment, headerCommitment)
  }
}

export const buildInitialResourceEpoch = async (
  vault: KeyVault,
  input: {
    projectId: Uuid
    resourceId: Uuid
    resourceKey: Uint8Array
    headerKey?: Uint8Array
    recipientIdentityId: Uuid
    packages: ProjectDeviceKeyPackage[]
  },
): Promise<InitialResourceEpochBundle> => {
  const commitment = await keyCommitment(
    input.projectId,
    input.resourceId,
    input.resourceKey,
  )
  const headerCommitment = input.headerKey
    ? await keyCommitment(input.projectId, input.resourceId, input.headerKey)
    : undefined
  try {
    const bodyEnvelopes = await buildResourceKeyEnvelopes(vault, input)
    const headerEnvelopes = input.headerKey
      ? await buildResourceKeyEnvelopes(vault, {
          ...input,
          resourceKey: input.headerKey,
          keyPurpose: 'header',
        })
      : []
    return {
      epoch: {
        id: crypto.randomUUID(),
        epoch: RESOURCE_EPOCH,
        creator_device_key_version: vault.deviceSecrets.keyVersion,
        key_commitment_b64: bytesToBase64(commitment),
        header_key_commitment_b64: headerCommitment
          ? bytesToBase64(headerCommitment)
          : null,
      },
      envelopes: [...bodyEnvelopes, ...headerEnvelopes],
    }
  } finally {
    zeroBytes(commitment, headerCommitment)
  }
}

const senderSigningKeys = async (
  packages: ProjectDeviceKeyPackage[],
  envelope: ResourceKeyEnvelopeViewDto,
): Promise<{
  ed25519PublicKey: Uint8Array
  mlDsa65PublicKey: Uint8Array
}> => {
  const item = packages.find(
    (candidate) =>
      candidate.identity_id === envelope.sender_identity_id &&
      candidate.device_id === envelope.sender_device_id &&
      candidate.key_version === envelope.sender_device_key_version,
  )
  if (!item) {
    throw new Error('Envelope sender has no matching device package')
  }
  const packageBytes = base64ToBytes(item.package_b64)
  const expectedHash = base64ToBytes(item.package_hash_b64)
  let actualHash: Uint8Array | undefined
  try {
    actualHash = await sha256(packageBytes)
    if (!equalBytes(actualHash, expectedHash)) {
      throw new Error('Envelope sender package digest mismatch')
    }
    const parsed = JSON.parse(new TextDecoder().decode(packageBytes)) as {
      signing_keys?: Array<{
        algorithm: string
        public_key: number[]
      }>
    }
    const ed25519 = parsed.signing_keys?.find(
      (key) => key.algorithm === 'ed25519',
    )
    const mlDsa65 = parsed.signing_keys?.find(
      (key) => key.algorithm === 'ml_dsa65_experimental',
    )
    if (!ed25519 || !mlDsa65) {
      throw new Error('Envelope sender package has incomplete signing keys')
    }
    return {
      ed25519PublicKey: Uint8Array.from(ed25519.public_key),
      mlDsa65PublicKey: Uint8Array.from(mlDsa65.public_key),
    }
  } finally {
    zeroBytes(packageBytes, expectedHash, actualHash)
  }
}

export const importResourceKeyEnvelopes = async (
  vault: KeyVault,
  input: {
    projectId: Uuid
    envelopes: ResourceKeyEnvelopeViewDto[]
    packages: ProjectDeviceKeyPackage[]
  },
): Promise<number> => {
  const recipientIdentityId = vault.localIdentityId
  const recipientDeviceId = vault.localDeviceId
  if (!recipientIdentityId || !recipientDeviceId) {
    throw new Error('The local key vault has no device identity')
  }
  let imported = 0
  for (const envelope of input.envelopes) {
    if (
      envelope.recipient_identity_id !== recipientIdentityId ||
      envelope.recipient_device_id !== recipientDeviceId ||
      envelope.recipient_device_key_version !== vault.deviceSecrets.keyVersion
    ) {
      continue
    }
    const encryptedKey = base64ToBytes(envelope.encrypted_key_b64)
    const classicalSignature = base64ToBytes(
      envelope.sender_signature_b64,
    )
    const postQuantumSignature = base64ToBytes(
      envelope.sender_post_quantum_signature_b64,
    )
    const previousEpochHash =
      envelope.previous_epoch_hash_b64 === null
        ? await genesisHash(input.projectId, envelope.resource_id)
        : base64ToBytes(envelope.previous_epoch_hash_b64)
    let signingMessage: Uint8Array | undefined
    let senderKeys:
      | {
          ed25519PublicKey: Uint8Array
          mlDsa65PublicKey: Uint8Array
        }
      | undefined
    let unwrappedKey: Uint8Array | undefined
    try {
      signingMessage = await envelopeSigningMessage(
        input.projectId,
        envelope,
        encryptedKey,
      )
      senderKeys = await senderSigningKeys(input.packages, envelope)
      const valid = await verifyDualSignature(
        senderKeys,
        { classicalSignature, postQuantumSignature },
        signingMessage,
        SIGNATURE_CONTEXT,
      )
      if (!valid) {
        throw new Error('Resource-key envelope signatures are invalid')
      }
      const unwrapped = await unwrapResourceKeyForRecipient(
        encryptedKey,
        {
          x25519PrivateKey: vault.deviceSecrets.x25519PrivateKey,
          mlKem768PrivateKey: vault.deviceSecrets.mlKem768PrivateKey,
        },
        {
          resourceId: envelope.resource_id,
          recipientDeviceId,
          resourceEpoch: envelope.epoch,
          previousEpochHash,
          context: envelopeContext(
            input.projectId,
            envelope.resource_id,
            recipientIdentityId,
            recipientDeviceId,
            envelope.key_purpose ?? 'body',
          ),
        },
      )
      unwrappedKey = unwrapped.resourceKey
      await vault.putResourceKey(
        envelope.resource_id,
        unwrappedKey,
        envelope.epoch,
        envelope.key_purpose ?? 'body',
      )
      imported += 1
    } finally {
      zeroBytes(
        encryptedKey,
        classicalSignature,
        postQuantumSignature,
        previousEpochHash,
        signingMessage,
        senderKeys?.ed25519PublicKey,
        senderKeys?.mlDsa65PublicKey,
        unwrappedKey,
      )
    }
  }
  return imported
}
