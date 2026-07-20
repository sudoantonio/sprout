import type { IsoDateTime, Uuid } from '../api/contracts'
import { base64ToBytes, bytesToBase64, zeroBytes } from './wasm'

export interface DeviceTransparencyEntry {
  log_sequence: number
  key_version: number
  generation: number
  event_kind: string
  package_hash_b64: string
  previous_entry_hash_b64: string | null
  entry_hash_b64: string
  recorded_at: IsoDateTime
}

const encoder = new TextEncoder()
const DOMAIN = encoder.encode('sprout-device-key-transparency-v1')

const uuidBytes = (value: Uuid): Uint8Array => {
  const normalized = value.replaceAll('-', '')
  if (!/^[0-9a-f]{32}$/i.test(normalized)) {
    throw new Error('Invalid transparency UUID')
  }
  return Uint8Array.from(
    normalized.match(/.{2}/g)!.map((byte) => Number.parseInt(byte, 16)),
  )
}

const i32Bytes = (value: number): Uint8Array => {
  const bytes = new Uint8Array(4)
  new DataView(bytes.buffer).setInt32(0, value, false)
  return bytes
}

const i64Bytes = (value: number): Uint8Array => {
  if (!Number.isSafeInteger(value)) {
    throw new Error('Transparency generation exceeds safe integer range')
  }
  const bytes = new Uint8Array(8)
  new DataView(bytes.buffer).setBigInt64(0, BigInt(value), false)
  return bytes
}

export const verifyDeviceTransparency = async (
  identityId: Uuid,
  deviceId: Uuid,
  entries: DeviceTransparencyEntry[],
): Promise<void> => {
  let previous: Uint8Array | undefined
  let expectedSequence = 1
  try {
    for (const entry of entries) {
      if (entry.log_sequence !== expectedSequence) {
        throw new Error('Device transparency log has a sequence gap')
      }
      const declaredPrevious = entry.previous_entry_hash_b64
        ? base64ToBytes(entry.previous_entry_hash_b64)
        : undefined
      if (
        (previous === undefined) !== (declaredPrevious === undefined) ||
        (previous &&
          declaredPrevious &&
          bytesToBase64(previous) !== bytesToBase64(declaredPrevious))
      ) {
        zeroBytes(declaredPrevious)
        throw new Error('Device transparency log has a broken previous hash')
      }
      const identity = uuidBytes(identityId)
      const device = uuidBytes(deviceId)
      const keyVersion = i32Bytes(entry.key_version)
      const generation = i64Bytes(entry.generation)
      const kind = encoder.encode(entry.event_kind)
      const packageHash = base64ToBytes(entry.package_hash_b64)
      const input = new Uint8Array(
        DOMAIN.length +
          identity.length +
          device.length +
          keyVersion.length +
          generation.length +
          kind.length +
          packageHash.length +
          (previous?.length ?? 0),
      )
      let offset = 0
      for (const part of [
        DOMAIN,
        identity,
        device,
        keyVersion,
        generation,
        kind,
        packageHash,
        ...(previous ? [previous] : []),
      ]) {
        input.set(part, offset)
        offset += part.length
      }
      const calculated = new Uint8Array(
        await crypto.subtle.digest('SHA-256', input),
      )
      const expected = base64ToBytes(entry.entry_hash_b64)
      zeroBytes(input, expected, declaredPrevious, packageHash, previous)
      if (bytesToBase64(calculated) !== entry.entry_hash_b64) {
        zeroBytes(calculated)
        throw new Error('Device transparency entry hash is invalid')
      }
      previous = calculated
      expectedSequence += 1
    }
  } catch (error) {
    zeroBytes(previous)
    throw error
  }
  zeroBytes(previous)
}
