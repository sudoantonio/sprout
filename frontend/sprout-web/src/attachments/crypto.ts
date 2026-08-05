import type { Uuid } from '../api/contracts'
import { bytesToBase64, zeroBytes } from '../security/wasm'

export type AttachmentCiphertext = Blob & {
  readonly __sproutAttachmentCiphertext: true
}

export interface AttachmentCipherContext {
  projectId: Uuid
  resourceId: Uuid
  blobId: Uuid
  keyEpoch: number
}

const MAGIC = new TextEncoder().encode('SPROUTA1')
const IV_BYTES = 12

const asArrayBuffer = (value: Uint8Array): ArrayBuffer =>
  value.buffer.slice(
    value.byteOffset,
    value.byteOffset + value.byteLength,
  ) as ArrayBuffer

const attachmentAad = (context: AttachmentCipherContext): Uint8Array =>
  new TextEncoder().encode(
    `sprout/attachment/v1/${context.projectId}/${context.resourceId}/${context.blobId}/${context.keyEpoch}`,
  )

const importAttachmentKey = (resourceKey: Uint8Array): Promise<CryptoKey> => {
  if (resourceKey.byteLength !== 32) {
    throw new Error('Attachment encryption requires a 256-bit resource key')
  }
  return crypto.subtle.importKey(
    'raw',
    asArrayBuffer(resourceKey),
    { name: 'AES-GCM' },
    false,
    ['encrypt', 'decrypt'],
  )
}

export const asAttachmentCiphertext = (blob: Blob): AttachmentCiphertext =>
  new Blob([blob], {
    type: 'application/octet-stream',
  }) as AttachmentCiphertext

export const encryptAttachment = async (
  plaintext: Blob,
  resourceKey: Uint8Array,
  context: AttachmentCipherContext,
): Promise<AttachmentCiphertext> => {
  const source = new Uint8Array(await plaintext.arrayBuffer())
  const iv = crypto.getRandomValues(new Uint8Array(IV_BYTES))
  const aad = attachmentAad(context)
  let encrypted: ArrayBuffer | undefined
  let container: Uint8Array | undefined
  try {
    encrypted = await crypto.subtle.encrypt(
      {
        name: 'AES-GCM',
        iv: asArrayBuffer(iv),
        additionalData: asArrayBuffer(aad),
        tagLength: 128,
      },
      await importAttachmentKey(resourceKey),
      asArrayBuffer(source),
    )
    container = new Uint8Array(
      MAGIC.byteLength + IV_BYTES + encrypted.byteLength,
    )
    container.set(MAGIC)
    container.set(iv, MAGIC.byteLength)
    container.set(new Uint8Array(encrypted), MAGIC.byteLength + IV_BYTES)
    return new Blob([asArrayBuffer(container)], {
      type: 'application/octet-stream',
    }) as AttachmentCiphertext
  } finally {
    zeroBytes(
      source,
      iv,
      aad,
      encrypted ? new Uint8Array(encrypted) : undefined,
      container,
    )
  }
}

export const decryptAttachment = async (
  ciphertext: Blob,
  resourceKey: Uint8Array,
  context: AttachmentCipherContext,
): Promise<Uint8Array> => {
  const container = new Uint8Array(await ciphertext.arrayBuffer())
  const prefixLength = MAGIC.byteLength + IV_BYTES
  if (
    container.byteLength <= prefixLength ||
    !MAGIC.every((byte, index) => container[index] === byte)
  ) {
    zeroBytes(container)
    throw new Error('Encrypted attachment has an invalid container')
  }
  const iv = container.slice(MAGIC.byteLength, prefixLength)
  const payload = container.slice(prefixLength)
  const aad = attachmentAad(context)
  try {
    const plaintext = await crypto.subtle.decrypt(
      {
        name: 'AES-GCM',
        iv: asArrayBuffer(iv),
        additionalData: asArrayBuffer(aad),
        tagLength: 128,
      },
      await importAttachmentKey(resourceKey),
      asArrayBuffer(payload),
    )
    return new Uint8Array(plaintext)
  } finally {
    zeroBytes(container, iv, payload, aad)
  }
}

export const attachmentCiphertextSha256 = async (
  ciphertext: Blob,
): Promise<string> => {
  const bytes = new Uint8Array(await ciphertext.arrayBuffer())
  let digest: ArrayBuffer | undefined
  try {
    digest = await crypto.subtle.digest('SHA-256', asArrayBuffer(bytes))
    return bytesToBase64(new Uint8Array(digest))
  } finally {
    zeroBytes(bytes, digest ? new Uint8Array(digest) : undefined)
  }
}
