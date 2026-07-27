import * as chronoEn from 'chrono-node/en'
import * as chronoIt from 'chrono-node/it'
import type { ParsedResult } from 'chrono-node'

export const DEFAULT_HOUR = 17
export const DEFAULT_MINUTE = 0

const LOCALE = 'it-IT'

const startOfDay = (date: Date) =>
  new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()

const capitalize = (value: string) =>
  value ? `${value.charAt(0).toUpperCase()}${value.slice(1)}` : value

const compareParsedResults = (left: ParsedResult, right: ParsedResult, ref: Date) => {
  const lengthDiff = right.text.length - left.text.length
  if (lengthDiff !== 0) return lengthDiff

  const refStart = startOfDay(ref)
  const leftStart = startOfDay(left.start.date())
  const rightStart = startOfDay(right.start.date())
  const leftFuture = leftStart >= refStart ? 1 : 0
  const rightFuture = rightStart >= refStart ? 1 : 0
  if (rightFuture !== leftFuture) return rightFuture - leftFuture

  return (
    Math.abs(leftStart - refStart) - Math.abs(rightStart - refStart) ||
    left.index - right.index
  )
}

const pickBestResult = (text: string, ref: Date): ParsedResult | null => {
  const trimmed = text.trim()
  if (!trimmed) return null

  const options = { forwardDate: true } as const
  const results = [
    ...chronoIt.parse(trimmed, ref, options),
    ...chronoEn.parse(trimmed, ref, options),
  ]

  if (results.length === 0) return null

  return results.sort((left, right) => compareParsedResults(left, right, ref))[0]
}

export type NaturalDateParseOptions = {
  referenceDate?: Date
  fallbackHour?: number
  fallbackMinute?: number
}

export const parseNaturalDateInput = (
  text: string,
  {
    referenceDate = new Date(),
    fallbackHour = DEFAULT_HOUR,
    fallbackMinute = DEFAULT_MINUTE,
  }: NaturalDateParseOptions = {},
): Date | null => {
  const best = pickBestResult(text, referenceDate)
  if (!best) return null

  const parsed = best.start.date()
  const hour = best.start.isCertain('hour') ? parsed.getHours() : fallbackHour
  const minute = best.start.isCertain('minute') ? parsed.getMinutes() : fallbackMinute

  return new Date(
    parsed.getFullYear(),
    parsed.getMonth(),
    parsed.getDate(),
    hour,
    minute,
  )
}

export const formatNaturalDateLabel = (date: Date): string => {
  const formatted = new Intl.DateTimeFormat(LOCALE, {
    weekday: 'short',
    day: 'numeric',
    month: 'short',
    year: 'numeric',
  }).format(date)

  return capitalize(formatted.replace(/\./g, ''))
}

export const formatNaturalDateRelativeHint = (
  date: Date,
  referenceDate = new Date(),
): string => {
  const refStart = startOfDay(referenceDate)
  const targetStart = startOfDay(date)
  const dayDifference = Math.round((targetStart - refStart) / 86_400_000)

  if (dayDifference === 0) return 'oggi'
  if (dayDifference === 1) return 'domani'
  if (dayDifference === -1) return 'ieri'

  const rtf = new Intl.RelativeTimeFormat(LOCALE, { numeric: 'auto' })
  const millisecondDifference = date.getTime() - referenceDate.getTime()
  const absoluteMilliseconds = Math.abs(millisecondDifference)
  const sign = millisecondDifference >= 0 ? 1 : -1

  const minute = 60_000
  const hour = 60 * minute
  const day = 86_400_000
  const week = 7 * day
  const month = 30 * day
  const year = 365 * day

  if (absoluteMilliseconds < hour) {
    return rtf.format(sign * Math.max(1, Math.round(millisecondDifference / minute)), 'minute')
  }
  if (absoluteMilliseconds < day) {
    return rtf.format(sign * Math.max(1, Math.round(millisecondDifference / hour)), 'hour')
  }
  if (absoluteMilliseconds < week * 2) {
    return rtf.format(sign * dayDifference, 'day')
  }
  if (absoluteMilliseconds < month * 2) {
    return rtf.format(sign * Math.round(millisecondDifference / week), 'week')
  }
  if (absoluteMilliseconds < year) {
    return rtf.format(sign * Math.round(millisecondDifference / month), 'month')
  }

  return rtf.format(sign * Math.round(millisecondDifference / year), 'year')
}
