// @vitest-environment node

import { describe, expect, it } from 'vitest'
import {
  type DeviceTransparencyEntry,
  verifyDeviceTransparency,
} from './key-transparency'

const identityId = '11111111-1111-4111-8111-111111111111'
const deviceId = '22222222-2222-4222-8222-222222222222'

const validEntries = (): DeviceTransparencyEntry[] => [
  {
    log_sequence: 1,
    key_version: 1,
    generation: 1,
    event_kind: 'registered',
    package_hash_b64: 'BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=',
    previous_entry_hash_b64: null,
    entry_hash_b64: '+8gsqqNd1p71MtYkETHN9iFRBTxBmRmoezZuvDLMLyA=',
    recorded_at: '2026-01-01T00:00:00Z',
  },
  {
    log_sequence: 2,
    key_version: 2,
    generation: 2,
    event_kind: 'rotated',
    package_hash_b64: 'CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg=',
    previous_entry_hash_b64:
      '+8gsqqNd1p71MtYkETHN9iFRBTxBmRmoezZuvDLMLyA=',
    entry_hash_b64: 'FHaFRH6x4VavTJYJkCYlRqKh14aBdyRFNK5xZ1hVDt4=',
    recorded_at: '2026-01-02T00:00:00Z',
  },
]

describe('device key transparency', () => {
  it('accepts the server canonical hash-chain vector', async () => {
    await expect(
      verifyDeviceTransparency(identityId, deviceId, validEntries()),
    ).resolves.toBeUndefined()
  })

  it('rejects gaps, substitution, and rollback links', async () => {
    const gap = validEntries()
    gap[1]!.log_sequence = 3
    await expect(
      verifyDeviceTransparency(identityId, deviceId, gap),
    ).rejects.toThrow(/sequence gap/i)

    const substituted = validEntries()
    substituted[1]!.package_hash_b64 =
      'BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc='
    await expect(
      verifyDeviceTransparency(identityId, deviceId, substituted),
    ).rejects.toThrow(/entry hash/i)

    const rollback = validEntries()
    rollback[1]!.previous_entry_hash_b64 = null
    await expect(
      verifyDeviceTransparency(identityId, deviceId, rollback),
    ).rejects.toThrow(/previous hash/i)
  })
})
