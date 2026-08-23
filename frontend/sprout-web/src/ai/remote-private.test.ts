import { describe, expect, it } from 'vitest'
import { validatePrivateDestination } from './remote-private'

describe('private per-destination parsing', () => {
  it.each([
    ['192.0.2.1/32', true],
    ['999.0.2.1/32', false],
    ['192.0.2/32', false],
    ['2001:db8::1/128', true],
    ['2001:::1/128', false],
    ['example.test/32', false],
    ['2001:db8::1/64', false],
  ])('validates %s without permissive address regexes', (destination, expected) => {
    expect(validatePrivateDestination(destination)).toBe(expected)
  })
})
