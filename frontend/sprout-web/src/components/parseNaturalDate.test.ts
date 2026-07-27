/** @vitest-environment node */
import { describe, expect, it } from 'vitest'
import {
  DEFAULT_HOUR,
  DEFAULT_MINUTE,
  formatNaturalDateLabel,
  formatNaturalDateRelativeHint,
  parseNaturalDateInput,
} from './parseNaturalDate'

const referenceDate = new Date(2026, 6, 25, 12, 0, 0)

describe('parseNaturalDateInput', () => {
  it('parses Italian month names with day and year', () => {
    const parsed = parseNaturalDateInput('27 luglio 2029', { referenceDate })
    expect(parsed).not.toBeNull()
    expect(parsed?.getFullYear()).toBe(2029)
    expect(parsed?.getMonth()).toBe(6)
    expect(parsed?.getDate()).toBe(27)
    expect(parsed?.getHours()).toBe(DEFAULT_HOUR)
    expect(parsed?.getMinutes()).toBe(DEFAULT_MINUTE)
  })

  it('parses numeric Italian dates', () => {
    const parsed = parseNaturalDateInput('15/08/2026', { referenceDate })
    expect(parsed).not.toBeNull()
    expect(parsed?.getFullYear()).toBe(2026)
    expect(parsed?.getMonth()).toBe(7)
    expect(parsed?.getDate()).toBe(15)
  })

  it('parses relative Italian phrases', () => {
    const tomorrow = parseNaturalDateInput('domani', { referenceDate })
    expect(tomorrow).not.toBeNull()
    expect(tomorrow?.getFullYear()).toBe(2026)
    expect(tomorrow?.getMonth()).toBe(6)
    expect(tomorrow?.getDate()).toBe(26)

    const inTwoWeeks = parseNaturalDateInput('tra 2 settimane', { referenceDate })
    expect(inTwoWeeks).not.toBeNull()
    expect(inTwoWeeks?.getFullYear()).toBe(2026)
    expect(inTwoWeeks?.getMonth()).toBe(7)
    expect(inTwoWeeks?.getDate()).toBe(8)
  })

  it('returns null for empty or unparseable input', () => {
    expect(parseNaturalDateInput('', { referenceDate })).toBeNull()
    expect(parseNaturalDateInput('   ', { referenceDate })).toBeNull()
    expect(parseNaturalDateInput('xyz non una data', { referenceDate })).toBeNull()
  })

  it('preserves fallback time when only the date is specified', () => {
    const parsed = parseNaturalDateInput('27 luglio 2029', {
      referenceDate,
      fallbackHour: 9,
      fallbackMinute: 30,
    })
    expect(parsed?.getHours()).toBe(9)
    expect(parsed?.getMinutes()).toBe(30)
  })
})

describe('formatNaturalDateLabel', () => {
  it('formats a date with weekday, day, month, and year', () => {
    const label = formatNaturalDateLabel(new Date(2029, 6, 27, 17, 0, 0))
    expect(label).toMatch(/27/)
    expect(label).toMatch(/2029/)
    expect(label.toLowerCase()).toMatch(/lug/)
  })
})

describe('formatNaturalDateRelativeHint', () => {
  it('returns domani for the next calendar day', () => {
    expect(
      formatNaturalDateRelativeHint(new Date(2026, 6, 26, 17, 0, 0), referenceDate),
    ).toBe('domani')
  })

  it('returns a year-based relative hint for far-future dates', () => {
    const hint = formatNaturalDateRelativeHint(
      new Date(2029, 6, 27, 17, 0, 0),
      referenceDate,
    )
    expect(hint).toMatch(/3 anni/)
  })
})
