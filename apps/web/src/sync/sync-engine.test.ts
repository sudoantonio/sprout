import { describe, expect, it } from 'vitest'
import {
  hasExpectedPackageDigest,
  isStaleAfterTombstone,
  remoteFieldsFromRecord,
  shouldCatchUpAfterWakeOpen,
} from './sync-engine'

describe('stale tombstone protection', () => {
  it('rejects resurrection at or before the durable tombstone version', () => {
    expect(
      isStaleAfterTombstone(
        { mutation: 'upsert', aggregate_version: 7 },
        { aggregateVersion: 7 },
      ),
    ).toBe(true)
    expect(
      isStaleAfterTombstone(
        { mutation: 'upsert', aggregate_version: 8 },
        { aggregateVersion: 7 },
      ),
    ).toBe(false)
  })

  it('does not reject a tombstone as resurrection', () => {
    expect(
      isStaleAfterTombstone(
        { mutation: 'tombstone', aggregate_version: 7 },
        { aggregateVersion: 7 },
      ),
    ).toBe(false)
  })
})

describe('device package substitution protection', () => {
  it('rejects a package whose bytes do not match the server digest', async () => {
    const bytes = new TextEncoder().encode('{}')
    await expect(
      hasExpectedPackageDigest(
        bytes,
        'RBNvo1WzZ4oRRq0W9+hknpT7T8If536DEMBg9hyq/4o=',
      ),
    ).resolves.toBe(true)
    bytes[0] ^= 1
    await expect(
      hasExpectedPackageDigest(
        bytes,
        'RBNvo1WzZ4oRRq0W9+hknpT7T8If536DEMBg9hyq/4o=',
      ),
    ).resolves.toBe(false)
  })
})

describe('stale conflict remote fields (T-LLR-07.4)', () => {
  it('copies authoritative remote version and payload after catch-up', () => {
    const fields = remoteFieldsFromRecord({
      id: '11111111-1111-4111-8111-111111111111',
      projectId: '22222222-2222-4222-8222-222222222222',
      resourceId: '11111111-1111-4111-8111-111111111111',
      kind: 'task',
      aggregateVersion: 9,
      keyEpoch: 1,
      payload: {
        version: 1,
        algorithm: 'aes-256-gcm',
        key_id: '33333333-3333-4333-8333-333333333333',
        nonce_b64: 'AAAA',
        ciphertext_b64: 'BBBB',
      },
      updatedAt: new Date().toISOString(),
    })
    expect(fields.remoteVersion).toBe(9)
    expect(fields.remotePayloadB64).toBeTruthy()
    expect(remoteFieldsFromRecord(undefined)).toEqual({})
  })
})

describe('wake reconnect catch-up (T-LLR-07.2)', () => {
  it('requires REST catch-up only after a reconnect attempt', () => {
    expect(shouldCatchUpAfterWakeOpen(0)).toBe(false)
    expect(shouldCatchUpAfterWakeOpen(1)).toBe(true)
    expect(shouldCatchUpAfterWakeOpen(3)).toBe(true)
  })
})
