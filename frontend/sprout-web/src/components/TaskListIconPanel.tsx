import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { createPortal } from 'react-dom'
import type { TaskListColumnColor } from '../domain/models'
import type { TaskListIcon } from '../domain/task-list-icon'
import {
  filterTaskListEmojis,
  groupTaskListEmojis,
  loadTaskListEmojis,
  type TaskListEmojiEntry,
  type TaskListEmojiGroup,
} from './task-list-emoji-data'
import { filterTaskListGlyphs, TaskListGlyphIcon } from './task-list-glyphs'

type PickerTab = 'glyph' | 'emoji' | 'letter'

const PICKER_TABS = [
  ['glyph', 'Icone'],
  ['emoji', 'Emoji'],
  ['letter', 'Lettere'],
] as const satisfies ReadonlyArray<readonly [PickerTab, string]>

const COLUMN_COLOR_OPTIONS = [
  ['column-white', 'Bianco'],
  ['column-slate', 'Ardesia'],
  ['column-blue', 'Azzurro'],
  ['column-sand', 'Sabbia'],
  ['column-emerald', 'Salvia'],
  ['column-violet', 'Lavanda'],
  ['column-peach', 'Pesca'],
  ['column-mauve', 'Malva'],
  ['column-rose', 'Rosa'],
] as const satisfies ReadonlyArray<[TaskListColumnColor, string]>

const initialTabForIcon = (icon: TaskListIcon | undefined): PickerTab => {
  if (icon?.kind === 'emoji') return 'emoji'
  if (icon?.kind === 'glyph') return 'glyph'
  return 'letter'
}

const clampMenuPosition = (
  x: number,
  y: number,
  width: number,
  height: number,
): { left: number; top: number } => {
  const margin = 8
  const maxLeft = Math.max(margin, window.innerWidth - width - margin)
  const maxTop = Math.max(margin, window.innerHeight - height - margin)
  return {
    left: Math.min(Math.max(x, margin), maxLeft),
    top: Math.min(Math.max(y, margin), maxTop),
  }
}

const positionIconPicker = (
  anchorRect: DOMRect,
  width: number,
  height: number,
  gap = 8,
): { left: number; top: number } => {
  let top = anchorRect.bottom + gap
  if (top + height > window.innerHeight - gap) {
    top = anchorRect.top - height - gap
  }
  return clampMenuPosition(anchorRect.left, top, width, height)
}

export const TaskListIconPanel = ({
  anchorRect,
  listName,
  value,
  color,
  onChange,
  onColorChange,
  onClose,
}: {
  anchorRect: DOMRect
  listName: string
  value: TaskListIcon | undefined
  color: TaskListColumnColor
  onChange(icon: TaskListIcon | undefined): void
  onColorChange(color: TaskListColumnColor): void
  onClose(): void
}) => {
  const searchId = useId()
  const panelRef = useRef<HTMLElement>(null)
  const nameInitial = listName.trim().charAt(0).toUpperCase() || '?'
  const [tab, setTab] = useState<PickerTab>(() => initialTabForIcon(value))
  const [query, setQuery] = useState('')
  const [emojis, setEmojis] = useState<TaskListEmojiEntry[]>([])
  const [emojiGroups, setEmojiGroups] = useState<TaskListEmojiGroup[]>([])
  const [emojisLoading, setEmojisLoading] = useState(false)
  const [position, setPosition] = useState<CSSProperties>({
    left: anchorRect.left,
    top: anchorRect.bottom + 8,
  })

  useLayoutEffect(() => {
    const node = panelRef.current
    if (!node) return
    const rect = node.getBoundingClientRect()
    const next = positionIconPicker(anchorRect, rect.width, rect.height)
    setPosition({ left: next.left, top: next.top })
  }, [anchorRect, tab, query, emojisLoading, value, color])

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as HTMLElement
      if (panelRef.current?.contains(target)) return
      if (target.closest('.board-column-icon-trigger')) return
      onClose()
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        onClose()
      }
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [onClose])

  useEffect(() => {
    if (tab !== 'emoji' || emojis.length > 0 || emojisLoading) return
    setEmojisLoading(true)
    void loadTaskListEmojis()
      .then(setEmojis)
      .finally(() => setEmojisLoading(false))
  }, [tab, emojis.length, emojisLoading])

  const filteredGlyphs = useMemo(
    () => filterTaskListGlyphs(query),
    [query],
  )

  const filteredEmojis = useMemo(
    () => filterTaskListEmojis(emojis, query),
    [emojis, query],
  )

  useEffect(() => {
    if (tab !== 'emoji') {
      setEmojiGroups([])
      return
    }
    if (filteredEmojis.length === 0) {
      setEmojiGroups([])
      return
    }
    let cancelled = false
    void groupTaskListEmojis(filteredEmojis).then((groups) => {
      if (!cancelled) setEmojiGroups(groups)
    })
    return () => {
      cancelled = true
    }
  }, [tab, filteredEmojis])

  const searchPlaceholder =
    tab === 'glyph' ? 'Cerca icone…' : 'Cerca emoji…'

  const selectTab = (nextTab: PickerTab) => {
    setTab(nextTab)
    setQuery('')
    if (nextTab === 'letter') onChange(undefined)
  }

  const panelStyle: CSSProperties = {
    ...position,
    position: 'fixed',
    ['--task-list-icon-accent' as string]: `var(--avatar-${color}-icon-bg)`,
  }

  return createPortal(
    <section
      ref={panelRef}
      className="task-list-icon-panel"
      style={panelStyle}
      role="dialog"
      aria-label={`Scegli icona per ${listName}`}
      onClick={(event) => event.stopPropagation()}
    >
      <div className="task-list-icon-panel-body">
        <div
          className="task-list-icon-panel-tabs"
          role="tablist"
          aria-label="Tipo icona"
        >
          {PICKER_TABS.map(([tabValue, label]) => (
            <button
              key={tabValue}
              type="button"
              role="tab"
              aria-selected={tab === tabValue}
              className={
                tab === tabValue
                  ? 'task-list-icon-panel-tab selected'
                  : 'task-list-icon-panel-tab'
              }
              onClick={() => selectTab(tabValue)}
            >
              {label}
            </button>
          ))}
        </div>

        <div
          className="task-list-icon-panel-colors"
          role="listbox"
          aria-label="Colore task list"
        >
          {COLUMN_COLOR_OPTIONS.map(([colorValue, label]) => (
            <button
              type="button"
              key={colorValue}
              role="option"
              aria-selected={color === colorValue}
              aria-label={label}
              className={
                color === colorValue
                  ? `board-column-color-option board-column-color-option--${colorValue} selected`
                  : `board-column-color-option board-column-color-option--${colorValue}`
              }
              onClick={() => onColorChange(colorValue)}
            />
          ))}
        </div>

        {tab !== 'letter' && (
          <label className="task-list-icon-panel-search" htmlFor={searchId}>
            <span className="sr-only">{searchPlaceholder}</span>
            <input
              id={searchId}
              type="search"
              placeholder={searchPlaceholder}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              aria-label={searchPlaceholder}
            />
          </label>
        )}

        {tab === 'glyph' ? (
          <div
            className="task-list-icon-panel-grid"
            role="listbox"
            aria-label="Icone disponibili"
          >
            {filteredGlyphs.map((glyph) => {
              const selected =
                value?.kind === 'glyph' && value.id === glyph.id
              return (
                <button
                  key={glyph.id}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  aria-label={glyph.label}
                  className={
                    selected
                      ? 'task-list-icon-panel-option selected'
                      : 'task-list-icon-panel-option'
                  }
                  onClick={() => onChange({ kind: 'glyph', id: glyph.id })}
                >
                  <TaskListGlyphIcon glyphId={glyph.id} />
                </button>
              )
            })}
          </div>
        ) : tab === 'emoji' ? (
          <div className="task-list-icon-panel-emoji-scroll">
            {emojisLoading ? (
              <p className="task-list-icon-panel-status">Caricamento emoji…</p>
            ) : emojiGroups.length === 0 ? (
              <p className="task-list-icon-panel-status">Nessuna emoji trovata</p>
            ) : (
              emojiGroups.map((group) => (
                <section
                  key={group.group}
                  className="task-list-icon-panel-emoji-group"
                >
                  <h3>{group.label}</h3>
                  <div className="task-list-icon-panel-emoji-grid">
                    {group.entries.map((entry) => {
                      const selected =
                        value?.kind === 'emoji' && value.value === entry.emoji
                      return (
                        <button
                          key={entry.hexcode}
                          type="button"
                          aria-label={entry.label}
                          className={
                            selected
                              ? 'task-list-icon-panel-emoji selected'
                              : 'task-list-icon-panel-emoji'
                          }
                          onClick={() =>
                            onChange({ kind: 'emoji', value: entry.emoji })
                          }
                        >
                          {entry.emoji}
                        </button>
                      )
                    })}
                  </div>
                </section>
              ))
            )}
          </div>
        ) : (
          <div className="task-list-icon-panel-letter-preview">
            <span className="task-list-icon-panel-letter-mark" aria-hidden>
              {nameInitial}
            </span>
            <p>Viene usata la prima lettera del titolo.</p>
          </div>
        )}
      </div>
    </section>,
    document.body,
  )
}
