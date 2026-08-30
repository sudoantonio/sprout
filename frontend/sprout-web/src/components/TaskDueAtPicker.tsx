import { useEffect, useMemo, useState } from 'react'
import { CalendarIcon, ChevronIcon } from './icons'
import {
  DEFAULT_HOUR,
  DEFAULT_MINUTE,
  formatNaturalDateLabel,
  formatNaturalDateRelativeHint,
  parseNaturalDateInput,
} from './parseNaturalDate'

const WEEKDAY_LABELS = ['lun', 'mar', 'mer', 'gio', 'ven', 'sab', 'dom'] as const

const pad = (value: number) => String(value).padStart(2, '0')

type DateParts = {
  year: number
  month: number
  day: number
  hour: number
  minute: number
}

const defaultDateParts = (): DateParts => {
  const now = new Date()
  return {
    year: now.getFullYear(),
    month: now.getMonth(),
    day: now.getDate(),
    hour: DEFAULT_HOUR,
    minute: DEFAULT_MINUTE,
  }
}

const parseDatetimeLocal = (value: string): DateParts | null => {
  if (!value) return null
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value)
  if (match) {
    return {
      year: Number(match[1]),
      month: Number(match[2]) - 1,
      day: Number(match[3]),
      hour: Number(match[4]),
      minute: Number(match[5]),
    }
  }
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return null
  return {
    year: date.getFullYear(),
    month: date.getMonth(),
    day: date.getDate(),
    hour: date.getHours(),
    minute: date.getMinutes(),
  }
}

const toDateFromParts = ({
  year,
  month,
  day,
  hour,
  minute,
}: DateParts): Date => new Date(year, month, day, hour, minute)

const buildDatetimeLocal = ({
  year,
  month,
  day,
  hour,
  minute,
}: DateParts): string =>
  `${year}-${pad(month + 1)}-${pad(day)}T${pad(hour)}:${pad(minute)}`

const sameDay = (left: Date, right: Date) =>
  left.getFullYear() === right.getFullYear() &&
  left.getMonth() === right.getMonth() &&
  left.getDate() === right.getDate()

const formatDueAtPickerSummary = (value: string): string => {
  if (!value) return 'Scegli data e ora'
  const parts = parseDatetimeLocal(value)
  if (!parts) return 'Scegli data e ora'

  const due = toDateFromParts(parts)
  const now = new Date()
  const startOfDay = (date: Date) =>
    new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  const dayDifference = Math.round(
    (startOfDay(due) - startOfDay(now)) / 86_400_000,
  )
  const time = new Intl.DateTimeFormat('it-IT', {
    hour: '2-digit',
    minute: '2-digit',
  }).format(due)

  if (dayDifference === 0) return `Oggi, ${time}`
  if (dayDifference === 1) return `Domani, ${time}`

  const datePart = new Intl.DateTimeFormat('it-IT', {
    weekday: 'short',
    day: 'numeric',
    month: 'short',
  }).format(due)
  return `${datePart}, ${time}`
}

const capitalize = (value: string) =>
  value ? `${value.charAt(0).toUpperCase()}${value.slice(1)}` : value

const buildMonthGrid = (year: number, month: number) => {
  const firstWeekday = (new Date(year, month, 1).getDay() + 6) % 7
  const daysInMonth = new Date(year, month + 1, 0).getDate()
  const cells: Array<{ date: Date; inMonth: boolean }> = []

  for (let index = 0; index < firstWeekday; index += 1) {
    cells.push({
      date: new Date(year, month, index - firstWeekday + 1),
      inMonth: false,
    })
  }

  for (let day = 1; day <= daysInMonth; day += 1) {
    cells.push({ date: new Date(year, month, day), inMonth: true })
  }

  while (cells.length % 7 !== 0) {
    const trailingDay = cells.length - firstWeekday - daysInMonth + 1
    cells.push({
      date: new Date(year, month + 1, trailingDay),
      inMonth: false,
    })
  }

  return cells
}

const buildYearRange = (anchorYear: number) => {
  const currentYear = new Date().getFullYear()
  const start = Math.min(currentYear - 2, anchorYear - 5)
  const end = Math.max(currentYear + 8, anchorYear + 5)
  return Array.from({ length: end - start + 1 }, (_, index) => start + index)
}

const HOUR_ITEMS = Array.from({ length: 24 }, (_, index) => index)
const MINUTE_ITEMS = Array.from({ length: 60 }, (_, index) => index)

export const TaskDueAtPicker = ({
  label,
  value,
  onChange,
}: {
  label: string
  value: string
  onChange(value: string): void
}) => {
  const parsed = parseDatetimeLocal(value) ?? defaultDateParts()
  const [viewMonth, setViewMonth] = useState(parsed.month)
  const [viewYear, setViewYear] = useState(parsed.year)
  const [inputText, setInputText] = useState('')

  useEffect(() => {
    const next = parseDatetimeLocal(value)
    if (!next) return
    setViewMonth(next.month)
    setViewYear(next.year)
  }, [value])

  const selectedDate = useMemo(
    () => new Date(parsed.year, parsed.month, parsed.day),
    [parsed.day, parsed.month, parsed.year],
  )
  const today = useMemo(() => {
    const now = new Date()
    return new Date(now.getFullYear(), now.getMonth(), now.getDate())
  }, [])

  const monthCells = useMemo(
    () => buildMonthGrid(viewYear, viewMonth),
    [viewMonth, viewYear],
  )

  const monthLabel = capitalize(
    new Intl.DateTimeFormat('it-IT', {
      month: 'long',
      year: 'numeric',
    }).format(new Date(viewYear, viewMonth, 1)),
  )

  const yearItems = useMemo(
    () => buildYearRange(parsed.year),
    [parsed.year],
  )

  const emitChange = (next: DateParts) => {
    onChange(buildDatetimeLocal(next))
  }

  const suggestion = useMemo(() => {
    if (!inputText.trim()) return null
    return parseNaturalDateInput(inputText, {
      fallbackHour: parsed.hour,
      fallbackMinute: parsed.minute,
    })
  }, [inputText, parsed.hour, parsed.minute])

  const applySuggestion = () => {
    if (!suggestion) return
    emitChange({
      year: suggestion.getFullYear(),
      month: suggestion.getMonth(),
      day: suggestion.getDate(),
      hour: suggestion.getHours(),
      minute: suggestion.getMinutes(),
    })
    setViewYear(suggestion.getFullYear())
    setViewMonth(suggestion.getMonth())
    setInputText('')
  }

  const shiftMonth = (delta: number) => {
    const anchor = new Date(viewYear, viewMonth + delta, 1)
    setViewYear(anchor.getFullYear())
    setViewMonth(anchor.getMonth())
  }

  return (
    <div className="task-create-datetime-field">
      <div className="task-due-at-header">
        <span className="task-create-datetime-label">{label}</span>
        <span className="task-due-at-summary" aria-live="polite">
          {formatDueAtPickerSummary(value)}
        </span>
      </div>

      <div className="task-due-at-picker-surface">
        <div className="task-due-at-natural">
          <input
            className="task-due-at-natural-input"
            type="text"
            value={inputText}
            placeholder="feb 23, domani, tra 1 settimana…"
            aria-label={`${label}: scrivi una data`}
            onChange={(event) => setInputText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                applySuggestion()
              }
            }}
            onBlur={() => applySuggestion()}
          />
          {suggestion && (
            <button
              type="button"
              className="task-due-at-suggestion"
              onMouseDown={(event) => event.preventDefault()}
              onClick={applySuggestion}
            >
              <CalendarIcon
                className="task-due-at-suggestion-icon"
                aria-hidden
              />
              <span className="task-due-at-suggestion-date">
                {formatNaturalDateLabel(suggestion)}
              </span>
              <span className="task-due-at-suggestion-relative">
                {formatNaturalDateRelativeHint(suggestion)}
              </span>
            </button>
          )}
        </div>

        <div className="task-due-at-calendar" aria-label="Calendario">
          <div className="task-due-at-calendar-header">
            <button
              type="button"
              className="task-due-at-calendar-nav"
              aria-label="Mese precedente"
              onClick={() => shiftMonth(-1)}
            >
              <ChevronIcon className="task-due-at-calendar-nav-icon task-due-at-calendar-nav-icon--prev" />
            </button>
            <p className="task-due-at-calendar-title">{monthLabel}</p>
            <button
              type="button"
              className="task-due-at-calendar-nav"
              aria-label="Mese successivo"
              onClick={() => shiftMonth(1)}
            >
              <ChevronIcon className="task-due-at-calendar-nav-icon" />
            </button>
          </div>

          <div className="task-due-at-weekdays" aria-hidden>
            {WEEKDAY_LABELS.map((weekday) => (
              <span key={weekday} className="task-due-at-weekday">
                {weekday}
              </span>
            ))}
          </div>

          <div className="task-due-at-days" role="grid" aria-label={monthLabel}>
            {monthCells.map(({ date, inMonth }) => {
              const isSelected = sameDay(date, selectedDate)
              const isToday = sameDay(date, today)
              const day = date.getDate()
              return (
                <button
                  key={`${date.toISOString()}-${inMonth ? 'in' : 'out'}`}
                  type="button"
                  role="gridcell"
                  className={[
                    'task-due-at-day',
                    inMonth ? '' : 'task-due-at-day--muted',
                    isSelected ? 'task-due-at-day--selected' : '',
                    isToday ? 'task-due-at-day--today' : '',
                  ]
                    .filter(Boolean)
                    .join(' ')}
                  aria-label={new Intl.DateTimeFormat('it-IT', {
                    weekday: 'long',
                    day: 'numeric',
                    month: 'long',
                    year: 'numeric',
                  }).format(date)}
                  aria-selected={isSelected}
                  onClick={() => {
                    emitChange({
                      year: date.getFullYear(),
                      month: date.getMonth(),
                      day: date.getDate(),
                      hour: parsed.hour,
                      minute: parsed.minute,
                    })
                    setViewYear(date.getFullYear())
                    setViewMonth(date.getMonth())
                  }}
                >
                  {day}
                </button>
              )
            })}
          </div>
        </div>

        <div className="task-due-at-time-row" aria-label="Ora">
          <select
            className="task-due-at-time-select task-due-at-time-select--year"
            value={parsed.year}
            aria-label="Anno"
            onChange={(event) =>
              emitChange({ ...parsed, year: Number(event.target.value) })
            }
          >
            {yearItems.map((year) => (
              <option key={year} value={year}>
                {year}
              </option>
            ))}
          </select>

          <div className="task-due-at-time-clock">
            <select
              className="task-due-at-time-select task-due-at-time-select--hour"
              value={parsed.hour}
              aria-label="Ora"
              onChange={(event) =>
                emitChange({ ...parsed, hour: Number(event.target.value) })
              }
            >
              {HOUR_ITEMS.map((hour) => (
                <option key={hour} value={hour}>
                  {pad(hour)}
                </option>
              ))}
            </select>
            <span className="task-due-at-time-separator" aria-hidden>
              :
            </span>
            <select
              className="task-due-at-time-select task-due-at-time-select--minute"
              value={parsed.minute}
              aria-label="Minuti"
              onChange={(event) =>
                emitChange({ ...parsed, minute: Number(event.target.value) })
              }
            >
              {MINUTE_ITEMS.map((minute) => (
                <option key={minute} value={minute}>
                  {pad(minute)}
                </option>
              ))}
            </select>
          </div>
        </div>
      </div>
    </div>
  )
}
