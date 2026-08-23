import { describe, expect, it } from 'vitest'
import { canonicalGovernanceJson, sha256Hex } from './canonical'

const decoder = new TextDecoder()

describe('Sprout integer-only canonical JSON', () => {
  it('matches a literal cross-language golden vector and digest', async () => {
    const value = {
      z: '"\\\n\u0001',
      a: [0, -1, 9_007_199_254_740_991],
      nested: { '😀': 'unicode', Z: true, 'é': null },
    }
    const expected =
      '{"a":[0,-1,9007199254740991],"nested":{"Z":true,"é":null,"😀":"unicode"},"z":"\\\"\\\\\\n\\u0001"}'
    const actual = canonicalGovernanceJson(value)
    expect(decoder.decode(actual)).toBe(expected)
    expect(await sha256Hex(actual)).toBe(
      'cb3858ee12d1adeb58942f68d845db7e2accc97d84fcebda6af8fc704dda1c6a',
    )
  })

  it('is independent of object declaration order and rejects floating point', () => {
    expect(canonicalGovernanceJson({ z: 1, a: 2 })).toEqual(
      canonicalGovernanceJson({ a: 2, z: 1 }),
    )
    expect(() => canonicalGovernanceJson({ value: 1.5 })).toThrow('safe integers')
  })
})
