// @vitest-environment node

import { describe, expect, it } from 'vitest'
import { shouldProvisionDevice } from './auth-controller'

describe('device key provisioning policy', () => {
  it('never replaces an active package when the local vault is missing', () => {
    expect(shouldProvisionDevice([{ revoked_at: null }])).toBe(false)
  })

  it('allows initial provisioning when no active package exists', () => {
    expect(shouldProvisionDevice([])).toBe(true)
    expect(
      shouldProvisionDevice([
        { revoked_at: new Date().toISOString() },
      ]),
    ).toBe(true)
  })
})
