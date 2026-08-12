import { useEffect, useMemo, useState } from 'react'
import { CalendarIcon } from './icons'
import {
  DEFAULT_HOUR,
  DEFAULT_MINUTE,
  formatNaturalDateLabel,
  formatNaturalDateRelativeHint,
  parseNaturalDateInput,
} from './parseNaturalDate'

const pad = (value: number) => String(value).padStart(2, '0')

const parseDatetimeLocal = (value: string): { hour: number; minute: number } | null => {
  if (!value) return null
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value)
  if (!match) return null
  return { hour: Number(match[4]), minute: Number(match[5]) }
}

const buildDatetimeLocal = (date: Date): string =>
  `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`

export const NaturalLanguageDateField = ({
  label,
  value,
  onChange,
  required = false,
  showCommittedPreview = false,
}: {
  label: string
  value: string
  onChange(value: string): void
  required?: boolean
  showCommittedPreview?: boolean
}) => {
  const [inputText, setInputText] = useState('')
  const fallbackTime = parseDatetimeLocal(value) ?? {
    hour: DEFAULT_HOUR,
    minute: DEFAULT_MINUTE,
  }

  useEffect(() => {
    setInputText('')
  }, [value])

  const previewDate = useMemo(() => {
    const trimmed = inputText.trim()
    if (trimmed) {
      return parseNaturalDateInput(trimmed, {
        fallbackHour: fallbackTime.hour,
        fallbackMinute: fallbackTime.minute,
      })
    }
    if (!showCommittedPreview || !value) return null
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? null : date
  }, [
    fallbackTime.hour,
    fallbackTime.minute,
    inputText,
    showCommittedPreview,
    value,
  ])

  const showInvalidHint = inputText.trim().length > 0 && previewDate === null

  const applyPreview = () => {
    if (!previewDate || !inputText.trim()) return
    onChange(buildDatetimeLocal(previewDate))
    setInputText('')
  }

  return (
    <div className="natural-language-date-field">
      <div className="natural-language-date-body">
        <input
          required={required && !value}
          className="natural-language-date-input"
          type="text"
          value={inputText}
          placeholder="scrivi data"
          aria-label={label}
          onChange={(event) => setInputText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              applyPreview()
            }
          }}
          onBlur={applyPreview}
        />
        {previewDate && (
          <div
            className={`natural-language-date-preview${
              inputText.trim() ? ' natural-language-date-preview--active' : ''
            }`}
            aria-live="polite"
          >
            <CalendarIcon
              className="natural-language-date-preview-icon"
              aria-hidden
            />
            <span className="natural-language-date-preview-label">
              {formatNaturalDateLabel(previewDate)}
            </span>
            <span className="natural-language-date-preview-relative">
              {formatNaturalDateRelativeHint(previewDate)}
            </span>
          </div>
        )}
        {showInvalidHint && (
          <p className="natural-language-date-invalid-hint">
            Data non riconosciuta — prova un formato come 15/08/2026 o domani
          </p>
        )}
      </div>
    </div>
  )
}
