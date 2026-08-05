// @vitest-environment node

import { describe, expect, it } from 'vitest'
import { resolveActiveResourceKey } from './resources'
import type { Uuid } from '../api/contracts'

describe('resolveActiveResourceKey', () => {
  const resourceId = '11111111-1111-1111-1111-111111111111' as Uuid
  const keyAt = (epoch: number) => new Uint8Array(32).fill(epoch)

  it('returns the preferred epoch when present', () => {
    const vault = {
      getResourceKey: (id: Uuid, epoch = 1) =>
        id === resourceId && epoch === 2 ? keyAt(2) : undefined,
      getLatestResourceKey: () => undefined,
    }
    expect(resolveActiveResourceKey(vault, resourceId, 2)).toEqual({
      epoch: 2,
      key: keyAt(2),
    })
  })

  it('falls back to the latest body key when preferred epoch is missing', () => {
    const vault = {
      getResourceKey: () => undefined,
      getLatestResourceKey: (id: Uuid) =>
        id === resourceId ? { epoch: 3, key: keyAt(3) } : undefined,
    }
    expect(resolveActiveResourceKey(vault, resourceId, 2)).toEqual({
      epoch: 3,
      key: keyAt(3),
    })
  })

  it('falls back to genesis epoch 1 when preferred is not 1', () => {
    const vault = {
      getResourceKey: (id: Uuid, epoch = 1) =>
        id === resourceId && epoch === 1 ? keyAt(1) : undefined,
      getLatestResourceKey: () => undefined,
    }
    expect(resolveActiveResourceKey(vault, resourceId, 4)).toEqual({
      epoch: 1,
      key: keyAt(1),
    })
  })

  it('returns undefined when no body key exists', () => {
    const vault = {
      getResourceKey: () => undefined,
      getLatestResourceKey: () => undefined,
    }
    expect(resolveActiveResourceKey(vault, resourceId, 1)).toBeUndefined()
  })
})
