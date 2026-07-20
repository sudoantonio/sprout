import { describe, expect, it } from 'vitest'
import {
  buildProvisionContext,
  ownerOnlyRecoveryWarning,
  recoveryUnprovisionedMessage,
  unreachableParticipantWarning,
} from './recovery'

describe('recovery provision helpers', () => {
  it('surfaces unprovisioned and owner-only availability warnings', () => {
    expect(recoveryUnprovisionedMessage).toMatch(/unprovisioned/i)
    expect(ownerOnlyRecoveryWarning).toMatch(/impossible/i)
    expect(unreachableParticipantWarning).toMatch(/unreachable/i)
  })

  it('builds a stable epoch-bound provision context', () => {
    const projectId = '30000000-0000-0000-0000-000000000001'
    const first = buildProvisionContext(projectId, 2, 5)
    const second = buildProvisionContext(projectId, 2, 5)
    const differentEpoch = buildProvisionContext(projectId, 3, 5)
    expect(first).toEqual(second)
    expect(first).not.toEqual(differentEpoch)
    expect(new TextDecoder().decode(first.slice(0, 36))).toBe(
      'sprout/project-recovery/provision/v1',
    )
  })
})
