import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
} from 'react'
import { createPortal } from 'react-dom'
import type { Uuid } from '../api/contracts'
import type {
  DecryptedInfoDocument,
  InfoDocumentContent,
  InfoDocumentBlock,
  InfoFileBlock,
} from '../domain/models'
import type { TaskListItem } from '../store/app-store'
import {
  FileIcon,
  FolderIcon,
  GripIcon,
  ImageIcon,
  ListIcon,
  DownloadIcon,
  PaperclipIcon,
  PencilIcon,
  XIcon,
} from './icons'
import { InfoMarkdown } from './InfoMarkdown'
import {
  flowCollapseStates,
  moveFlowBlockToTextBoundary,
  removeFlowBlock,
  splitTextBlockWith,
  updateTextBlock,
} from './infoDocumentFlow'
import { createSerialMutationQueue } from './serialMutationQueue'

const emptyDocument = (title?: string): InfoDocumentContent => ({
  schema: 1,
  ...(title ? { title } : {}),
  blocks: [
    {
      id: crypto.randomUUID(),
      type: 'text',
      markdown: '',
    },
  ],
})

const markdownFor = (document: DecryptedInfoDocument): string =>
  document.document.blocks.find((block) => block.type === 'text')?.markdown ?? ''

const trailingMarkdownFor = (document: DecryptedInfoDocument): string => {
  const textBlocks = document.document.blocks.filter((block) => block.type === 'text')
  return textBlocks.length > 1 ? textBlocks.at(-1)?.markdown ?? '' : ''
}

const infoErrorMessage = (reason: unknown, fallback: string): string => {
  if (reason instanceof DOMException && reason.name === 'OperationError') {
    return 'Il documento non può essere decifrato con le chiavi di questo dispositivo.'
  }
  if (reason instanceof Error) return reason.message
  if (
    typeof reason === 'object' &&
    reason !== null &&
    'message' in reason &&
    typeof reason.message === 'string'
  ) {
    return reason.message
  }
  return fallback
}

const withMarkdown = (
  document: InfoDocumentContent,
  markdown: string,
): InfoDocumentContent => {
  const textIndex = document.blocks.findIndex((block) => block.type === 'text')
  if (textIndex < 0) {
    return {
      ...document,
      blocks: [
        { id: crypto.randomUUID(), type: 'text', markdown },
        ...document.blocks,
      ],
    }
  }
  return {
    ...document,
    blocks: document.blocks.map((block, index) =>
      index === textIndex && block.type === 'text'
        ? { ...block, markdown }
        : block,
    ),
  }
}

const withTrailingMarkdown = (
  document: InfoDocumentContent,
  markdown: string,
): InfoDocumentContent => {
  const textIndexes = document.blocks
    .map((block, index) => block.type === 'text' ? index : -1)
    .filter((index) => index >= 0)
  const trailingIndex = textIndexes.length > 1 ? textIndexes.at(-1) : undefined
  if (trailingIndex !== undefined) {
    return {
      ...document,
      blocks: document.blocks.map((block, index) =>
        index === trailingIndex && block.type === 'text'
          ? { ...block, markdown }
          : block,
      ),
    }
  }
  return markdown
    ? {
        ...document,
        blocks: [...document.blocks, {
          id: crypto.randomUUID(),
          type: 'text',
          markdown,
        }],
      }
    : document
}

const escapeHtml = (value: string): string =>
  value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')

const inlineMarkdownToHtml = (value: string): string => {
  let html = escapeHtml(value)
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>')
  html = html.replace(/\[([^\]]+)]\(((?:https?:\/\/|mailto:)[^)]+)\)/g, '<a href="$2">$1</a>')
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  html = html.replace(/~~([^~]+)~~/g, '<s>$1</s>')
  html = html.replace(/(^|[^*])\*([^*]+)\*/g, '$1<em>$2</em>')
  html = html.replace(/&lt;u&gt;([\s\S]*?)&lt;\/u&gt;/g, '<u>$1</u>')
  return html
}

const overviewInlineToMarkdown = (root: Node): string =>
  Array.from(root.childNodes).map((node) => {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? ''
    if (!(node instanceof HTMLElement)) return ''
    const content = overviewInlineToMarkdown(node)
    const tag = node.tagName
    if (tag === 'STRONG' || tag === 'B') return `**${content}**`
    if (tag === 'EM' || tag === 'I') return `*${content}*`
    if (tag === 'S' || tag === 'STRIKE') return `~~${content}~~`
    if (tag === 'U') return `<u>${content}</u>`
    if (tag === 'CODE') return `\`${content}\``
    if (tag === 'A') return `[${content}](${node.getAttribute('href') ?? ''})`
    if (tag === 'BR') return ''
    return content
  }).join('')

const overviewLineHtml = (
  tag: 'p' | 'h1' | 'h2' | 'h3' | 'div' | 'blockquote',
  content: string,
  attributes = '',
): string =>
  `<${tag}${attributes}><span data-task-text>${content || '<br>'}</span></${tag}>`

const openCollapsePrefix = ':::collapse '
const closedCollapsePrefix = ':::collapse[closed] '
const overviewDocumentSessionKey = (containerId: Uuid) =>
  `sprout.overview.current-document:${containerId}`
const overviewCollapseSessionKey = (documentId: Uuid, textBlockId: Uuid) =>
  `sprout.overview.collapses:${documentId}:${textBlockId}`
const optimisticOverviewCollapseState = new Map<string, boolean>()
const overviewCollapseStateKey = (
  documentId: Uuid,
  textBlockId: Uuid,
  index: number,
) => `${documentId}:${textBlockId}:${index}`

const rememberedOverviewDocumentId = (containerId: Uuid): Uuid | undefined => {
  try {
    return window.sessionStorage.getItem(overviewDocumentSessionKey(containerId)) || undefined
  } catch {
    return undefined
  }
}

const rememberOverviewDocumentId = (containerId: Uuid, documentId: Uuid): void => {
  try {
    window.sessionStorage.setItem(overviewDocumentSessionKey(containerId), documentId)
  } catch {
    // Session persistence is a convenience; the encrypted document remains authoritative.
  }
}

const rememberedOverviewCollapses = (
  documentId: Uuid,
  textBlockId: Uuid,
): Record<string, boolean> => {
  try {
    const value = window.sessionStorage.getItem(
      overviewCollapseSessionKey(documentId, textBlockId),
    )
    return value ? JSON.parse(value) as Record<string, boolean> : {}
  } catch {
    return {}
  }
}

const rememberOverviewCollapse = (
  documentId: Uuid,
  textBlockId: Uuid,
  index: number,
  collapsed: boolean,
): void => {
  optimisticOverviewCollapseState.set(
    overviewCollapseStateKey(documentId, textBlockId, index),
    collapsed,
  )
  try {
    const values = rememberedOverviewCollapses(documentId, textBlockId)
    values[String(index)] = collapsed
    window.sessionStorage.setItem(
      overviewCollapseSessionKey(documentId, textBlockId),
      JSON.stringify(values),
    )
  } catch {
    // The encrypted Markdown remains authoritative when session storage is unavailable.
  }
}

const applyRememberedOverviewCollapses = (
  editor: HTMLElement,
  documentId: Uuid,
  textBlockId: Uuid,
): void => {
  const remembered = rememberedOverviewCollapses(documentId, textBlockId)
  const headings = Array.from(
    editor.querySelectorAll<HTMLElement>('[data-md-kind="collapse"]'),
  )
  headings.forEach((heading, index) => {
    const collapsed = optimisticOverviewCollapseState.get(
      overviewCollapseStateKey(documentId, textBlockId, index),
    ) ?? remembered[String(index)]
    if (collapsed === undefined) return
    heading.dataset.collapsed = String(collapsed)
    const toggle = heading.querySelector<HTMLElement>('[data-overview-collapse-toggle]')
    if (!toggle) return
    toggle.textContent = collapsed ? '▶' : '▼'
    toggle.setAttribute('aria-expanded', String(!collapsed))
    toggle.setAttribute('aria-label', collapsed ? 'Espandi capitolo' : 'Comprimi capitolo')
  })
}

const markdownWithRememberedOverviewCollapses = (
  markdown: string,
  documentId: Uuid,
  textBlockId: Uuid,
): string => {
  const remembered = rememberedOverviewCollapses(documentId, textBlockId)
  let headingIndex = 0
  return markdown.split('\n').map((line) => {
    const isClosed = line.startsWith(closedCollapsePrefix)
    const isOpen = line.startsWith(openCollapsePrefix)
    if (!isClosed && !isOpen) return line
    const collapsed = optimisticOverviewCollapseState.get(
      overviewCollapseStateKey(documentId, textBlockId, headingIndex),
    ) ?? remembered[String(headingIndex)]
    headingIndex += 1
    if (collapsed === undefined) return line
    const title = line.slice((isClosed ? closedCollapsePrefix : openCollapsePrefix).length)
    return `${collapsed ? closedCollapsePrefix : openCollapsePrefix}${title}`
  }).join('\n')
}

const imageFullWidthSnapThreshold = 12

const markdownToOverviewHtml = (markdown: string): string =>
  markdown.split('\n').map((line) => {
    const isClosedCollapse = line.startsWith(closedCollapsePrefix)
    if (isClosedCollapse || line.startsWith(openCollapsePrefix)) {
      const prefix = isClosedCollapse ? closedCollapsePrefix : openCollapsePrefix
      return overviewLineHtml(
        'div',
        inlineMarkdownToHtml(line.slice(prefix.length)),
        ` class="tasklist-info-overview-collapse" data-md-kind="collapse" data-collapsed="${String(isClosedCollapse)}"><button type="button" class="tasklist-info-overview-collapse-toggle" contenteditable="false" data-overview-collapse-toggle aria-label="${isClosedCollapse ? 'Espandi' : 'Comprimi'} capitolo" aria-expanded="${String(!isClosedCollapse)}">${isClosedCollapse ? '▶' : '▼'}</button`,
      )
    }
    if (line.startsWith('### ')) return overviewLineHtml('h3', inlineMarkdownToHtml(line.slice(4)))
    if (line.startsWith('## ')) return overviewLineHtml('h2', inlineMarkdownToHtml(line.slice(3)))
    if (line.startsWith('# ')) return overviewLineHtml('h1', inlineMarkdownToHtml(line.slice(2)))
    if (line.startsWith('> ')) return overviewLineHtml('blockquote', inlineMarkdownToHtml(line.slice(2)))
    if (line.startsWith('- ')) return overviewLineHtml('div', inlineMarkdownToHtml(line.slice(2)), ' class="tasklist-info-overview-bullet" data-md-kind="bullet"')
    return overviewLineHtml('p', inlineMarkdownToHtml(line))
  }).join('')

const overviewEditorToMarkdown = (editor: HTMLElement): string =>
  (Array.from(editor.children) as HTMLElement[]).map((element): string | null => {
    // Browsers may place typed text next to the helper span in an empty line.
    // Read the complete line except for collapse headings, whose button must not
    // become part of the Markdown.
    const textRoot = element.dataset.mdKind === 'collapse'
      ? element.querySelector<HTMLElement>('[data-task-text]') ?? element
      : element
    const text = overviewInlineToMarkdown(textRoot).trimEnd()
    if (element.dataset.overviewPrompt === 'true' && text === '') return null
    if (element.tagName === 'H1') return `# ${text}`
    if (element.tagName === 'H2') return `## ${text}`
    if (element.tagName === 'H3') return `### ${text}`
    if (element.tagName === 'BLOCKQUOTE') return `> ${text}`
    if (element.tagName === 'PRE') return `\`${text}\``
    if (element.dataset.mdKind === 'collapse') {
      const prefix = element.dataset.collapsed === 'true'
        ? closedCollapsePrefix
        : openCollapsePrefix
      return `${prefix}${text}`
    }
    if (element.dataset.mdKind === 'bullet') return `- ${text}`
    return text
  }).filter((line): line is string => line !== null).join('\n')

const syncOverviewCollapseVisibility = (
  editor: HTMLElement,
  inheritedCollapsed = false,
): void => {
  let collapsed = inheritedCollapsed
  for (const element of Array.from(editor.children) as HTMLElement[]) {
    if (element.dataset.overviewPrompt === 'true') {
      element.classList.toggle('is-collapsed-by-chapter', collapsed)
      continue
    }
    if (element.dataset.mdKind === 'collapse') {
      collapsed = element.dataset.collapsed === 'true'
      element.classList.remove('is-collapsed-by-chapter')
      continue
    }
    element.classList.toggle('is-collapsed-by-chapter', collapsed)
  }
}

const overviewElementsToMarkdown = (elements: HTMLElement[]): string => {
  const editor = window.document.createElement('div')
  elements.forEach((element) => editor.append(element.cloneNode(true)))
  return overviewEditorToMarkdown(editor)
}

type PendingBlockInsertion = {
  target: 'main' | 'trailing'
  before: string
  after: string
}

const insertBlockAtPendingPosition = (
  content: InfoDocumentContent,
  block: InfoDocumentBlock,
  pending: PendingBlockInsertion | undefined,
): InfoDocumentContent => {
  if (!pending) {
    return {
      ...content,
      blocks: [...content.blocks, block],
    }
  }
  const textIndexes = content.blocks
    .map((value, index) => value.type === 'text' ? index : -1)
    .filter((index) => index >= 0)
  const textIndex = pending.target === 'main'
    ? textIndexes[0]
    : textIndexes.at(-1)
  if (textIndex === undefined) {
    return {
      ...content,
      blocks: [...content.blocks, block],
    }
  }
  const textBlock = content.blocks[textIndex]
  if (textBlock.type !== 'text') return content
  const replacement: InfoDocumentBlock[] = [
    ...(pending.before
      ? [{ ...textBlock, markdown: pending.before }]
      : []),
    block,
    {
      id: crypto.randomUUID(),
      type: 'text',
      markdown: pending.after,
    },
  ]
  return {
    ...content,
    blocks: [
      ...content.blocks.slice(0, textIndex),
      ...replacement,
      ...content.blocks.slice(textIndex + 1),
    ],
  }
}

const ensureOverviewTrailingPromptLine = (
  editor: HTMLElement,
  showPrompt = true,
): void => {
  editor.dataset.showPrompt = String(showPrompt)
  const promptLines = Array.from(
    editor.querySelectorAll<HTMLElement>(':scope > [data-overview-prompt]'),
  )
  for (const promptLine of promptLines) {
    const text = (promptLine.textContent ?? '').trim()
    if (text !== '') delete promptLine.dataset.overviewPrompt
  }

  if (!showPrompt) {
    for (const promptLine of Array.from(
      editor.querySelectorAll<HTMLElement>(':scope > [data-overview-prompt]'),
    )) promptLine.remove()
    return
  }

  let last = editor.lastElementChild as HTMLElement | null
  const lastIsEmptyParagraph =
    last?.tagName === 'P' &&
    (last.textContent ?? '').trim() === ''
  if (!lastIsEmptyParagraph) {
    const promptLine = window.document.createElement('p')
    promptLine.innerHTML = '<span data-task-text><br></span>'
    editor.append(promptLine)
    last = promptLine
  }
  if (!last) return
  last.dataset.overviewPrompt = 'true'
  last.dataset.placeholder = editor.dataset.placeholder ?? 'Scrivi una nota o usa / per aggiungere contenuti'

  for (const promptLine of Array.from(
    editor.querySelectorAll<HTMLElement>(':scope > [data-overview-prompt]'),
  )) {
    if (promptLine !== last) promptLine.remove()
  }
}

const InfoImageBlock = ({
  document,
  file,
  onRead,
  onDownload,
  onResize,
  onRemove,
  onDragStart,
  onDragEnd,
  selected,
  onSelect,
}: {
  document: DecryptedInfoDocument
  file: InfoFileBlock
  onRead(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<Blob>
  onDownload(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<void>
  onResize(file: InfoFileBlock, width: number | undefined): Promise<void>
  onRemove(file: InfoFileBlock): void
  onDragStart?(): void
  onDragEnd?(): void
  selected?: boolean
  onSelect?(target: HTMLElement): void
}) => {
  const [source, setSource] = useState<string>()
  const [previewOpen, setPreviewOpen] = useState(false)
  const [imageWidth, setImageWidth] = useState<number | undefined>(
    file.display_width,
  )
  const figureRef = useRef<HTMLElement>(null)
  const resizeStartRef = useRef<{
    pointerId: number
    x: number
    width: number
    scale: number
  } | null>(null)
  const resizeCleanupRef = useRef<(() => void) | undefined>(undefined)
  const currentWidthRef = useRef<number | undefined>(file.display_width)

  useEffect(() => () => resizeCleanupRef.current?.(), [])

  useLayoutEffect(() => {
    const availableWidth = figureRef.current?.clientWidth
    const fillsAvailableWidth =
      availableWidth !== undefined && availableWidth > 0 &&
      (file.display_width === undefined || file.display_width >= availableWidth - imageFullWidthSnapThreshold)
    setImageWidth(fillsAvailableWidth ? undefined : file.display_width)
    currentWidthRef.current = fillsAvailableWidth ? availableWidth : file.display_width
  }, [file.display_width])

  useLayoutEffect(() => {
    const figure = figureRef.current
    if (!figure || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(([entry]) => {
      const availableWidth = entry?.contentRect.width
      if (!availableWidth) return
      if (
        file.display_width === undefined ||
        (currentWidthRef.current !== undefined && currentWidthRef.current >= availableWidth - imageFullWidthSnapThreshold)
      ) {
        currentWidthRef.current = availableWidth
        setImageWidth(undefined)
      }
    })
    observer.observe(figure)
    return () => observer.disconnect()
  }, [file.display_width])

  useEffect(() => {
    let active = true
    let objectUrl: string | undefined
    void onRead(document, file)
      .then((blob) => {
        if (!active) return
        objectUrl = URL.createObjectURL(blob)
        setSource(objectUrl)
      })
      .catch(() => undefined)
    return () => {
      active = false
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [document, file, onRead])

  useEffect(() => {
    if (!previewOpen) return
    const previousOverflow = window.document.body.style.overflow
    window.document.body.style.overflow = 'hidden'
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') setPreviewOpen(false)
    }
    window.addEventListener('keydown', closeOnEscape)
    return () => {
      window.document.body.style.overflow = previousOverflow
      window.removeEventListener('keydown', closeOnEscape)
    }
  }, [previewOpen])

  return (
    <figure
      ref={figureRef}
      className={`tasklist-info-image${selected ? ' is-selected' : ''}`}
    >
      <button
        type="button"
        draggable
        className="tasklist-info-block-drag-handle tasklist-info-image-drag-handle"
        aria-label={`Sposta ${file.file_name}`}
        title="Trascina per spostare"
        onDragStart={(event) => {
          event.dataTransfer.effectAllowed = 'move'
          event.dataTransfer.setData('text/plain', file.id)
          onDragStart?.()
        }}
        onDragEnd={() => onDragEnd?.()}
      >
        <GripIcon aria-hidden />
      </button>
      <div
        className="tasklist-info-image-frame"
        style={{ width: imageWidth ? `${imageWidth}px` : '100%' }}
      >
        {source ? (
          <button
            type="button"
            className="tasklist-info-image-preview-trigger"
            aria-label={`Seleziona ${file.file_name}`}
            title="Doppio click per aprire a schermo intero"
            onClick={(event) => {
              if (onSelect) onSelect(event.currentTarget)
              else setPreviewOpen(true)
            }}
            onDoubleClick={() => setPreviewOpen(true)}
          >
            <img src={source} alt={file.file_name} />
          </button>
        ) : (
          <span className="tasklist-info-image-placeholder">Immagine cifrata</span>
        )}
        <button
          type="button"
          className="tasklist-info-block-remove tasklist-info-image-remove"
          aria-label={`Elimina ${file.file_name}`}
          title="Elimina immagine"
          onClick={() => onRemove(file)}
        >
          <XIcon aria-hidden />
        </button>
        <span
          className="tasklist-info-image-resize-handle"
          role="separator"
          aria-label={`Ridimensiona ${file.file_name}`}
          aria-orientation="vertical"
          tabIndex={0}
          onPointerDown={(event) => {
            const frame = event.currentTarget.parentElement
            if (!frame) return
            resizeCleanupRef.current?.()
            const renderedWidth = frame.getBoundingClientRect().width
            const layoutWidth = frame.offsetWidth
            const handle = event.currentTarget
            resizeStartRef.current = {
              pointerId: event.pointerId,
              x: event.clientX,
              width: layoutWidth,
              scale: layoutWidth > 0 ? renderedWidth / layoutWidth : 1,
            }
            currentWidthRef.current = layoutWidth
            const previousUserSelect = window.document.body.style.userSelect
            const previousCursor = window.document.body.style.cursor
            window.document.body.style.userSelect = 'none'
            window.document.body.style.cursor = 'nwse-resize'
            let pendingClientX = event.clientX
            let resizeFrame = 0

            const applyPendingMove = () => {
              resizeFrame = 0
              const start = resizeStartRef.current
              const parentWidth = figureRef.current?.clientWidth
              if (!start || !parentWidth) return
              const nextWidth = Math.round(
                Math.min(
                  parentWidth,
                  Math.max(160, start.width + (pendingClientX - start.x) / Math.max(0.01, start.scale)),
                ),
              )
              currentWidthRef.current = nextWidth
              frame.style.width = `${nextWidth}px`
            }

            const move = (pointerEvent: PointerEvent) => {
              const start = resizeStartRef.current
              if (!start || start.pointerId !== pointerEvent.pointerId) return
              pendingClientX = pointerEvent.clientX
              if (!resizeFrame) resizeFrame = window.requestAnimationFrame(applyPendingMove)
              pointerEvent.preventDefault()
            }

            const cleanup = () => {
              window.removeEventListener('pointermove', move)
              window.removeEventListener('pointerup', finish)
              window.removeEventListener('pointercancel', cancel)
              window.removeEventListener('blur', finishFromBlur)
              if (resizeFrame) window.cancelAnimationFrame(resizeFrame)
              resizeFrame = 0
              window.document.body.style.userSelect = previousUserSelect
              window.document.body.style.cursor = previousCursor
              try {
                if (handle.hasPointerCapture(event.pointerId)) {
                  handle.releasePointerCapture(event.pointerId)
                }
              } catch {
                // The browser may release capture before pointerup reaches window.
              }
              if (resizeCleanupRef.current === cleanup) resizeCleanupRef.current = undefined
            }

            const persist = () => {
              const parentWidth = figureRef.current?.clientWidth
              const draggedWidth = currentWidthRef.current
              const finalWidth =
                parentWidth && draggedWidth && draggedWidth >= parentWidth - imageFullWidthSnapThreshold
                  ? undefined
                  : draggedWidth
              resizeStartRef.current = null
              if (finalWidth !== undefined || parentWidth) {
                setImageWidth(finalWidth)
                void onResize(file, finalWidth)
              }
            }

            function finish(pointerEvent: PointerEvent) {
              if (resizeStartRef.current?.pointerId !== pointerEvent.pointerId) return
              pendingClientX = pointerEvent.clientX
              if (resizeFrame) window.cancelAnimationFrame(resizeFrame)
              applyPendingMove()
              cleanup()
              persist()
            }

            function cancel(pointerEvent: PointerEvent) {
              if (resizeStartRef.current?.pointerId !== pointerEvent.pointerId) return
              resizeStartRef.current = null
              cleanup()
              frame!.style.width = imageWidth ? `${imageWidth}px` : '100%'
            }

            function finishFromBlur() {
              if (!resizeStartRef.current) return
              if (resizeFrame) window.cancelAnimationFrame(resizeFrame)
              applyPendingMove()
              cleanup()
              persist()
            }

            resizeCleanupRef.current = cleanup
            window.addEventListener('pointermove', move, { passive: false })
            window.addEventListener('pointerup', finish)
            window.addEventListener('pointercancel', cancel)
            window.addEventListener('blur', finishFromBlur)
            try {
              handle.setPointerCapture(event.pointerId)
            } catch {
              // Window listeners still keep the resize active without capture.
            }
            event.preventDefault()
            event.stopPropagation()
          }}
          onKeyDown={(event) => {
            if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
            const frame = event.currentTarget.parentElement
            const parentWidth = frame?.parentElement?.clientWidth
            if (!frame || !parentWidth) return
            const currentWidth = currentWidthRef.current && currentWidthRef.current > 0
              ? currentWidthRef.current
              : frame.offsetWidth
            const nextWidth = Math.round(
              Math.min(parentWidth, Math.max(160, currentWidth + (event.key === 'ArrowRight' ? 24 : -24))),
            )
            currentWidthRef.current = nextWidth
            setImageWidth(nextWidth >= parentWidth ? undefined : nextWidth)
            void onResize(file, nextWidth >= parentWidth ? undefined : nextWidth)
            event.preventDefault()
          }}
        />
      </div>
      <figcaption>
        <button
          type="button"
          className="tasklist-info-attachment-link"
          onClick={() => void onDownload(document, file)}
        >
          {file.file_name}
        </button>
      </figcaption>
      {previewOpen && source && createPortal(
        <div
          className="tasklist-info-image-lightbox"
          role="dialog"
          aria-modal="true"
          aria-label={`Anteprima ${file.file_name}`}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setPreviewOpen(false)
          }}
        >
          <div className="tasklist-info-image-lightbox-toolbar">
            <button
              type="button"
              onClick={() => void onDownload(document, file)}
            >
              <DownloadIcon aria-hidden />
              <span>Scarica</span>
            </button>
            <button
              type="button"
              aria-label="Chiudi anteprima"
              title="Chiudi"
              onClick={() => setPreviewOpen(false)}
            >
              <XIcon aria-hidden />
            </button>
          </div>
          <img src={source} alt={file.file_name} />
        </div>,
        window.document.body,
      )}
    </figure>
  )
}

type FlowInsertionAnchor = {
  documentId: Uuid
  textBlockId: Uuid
  beforeMarkdown: string
  afterMarkdown: string
  afterTextBlockId?: Uuid
}

const OverviewFlowEditor = ({
  document,
  documents,
  busy,
  onDraft,
  onCommit,
  onRequestFile,
  onRequestPage,
  onReadFile,
  onDownloadFile,
  onResizeFile,
  onRemoveFile,
  onSelectDocument,
  onRenameDocument,
  onRemoveDocument,
}: {
  document: DecryptedInfoDocument
  documents: DecryptedInfoDocument[]
  busy: boolean
  onDraft(content: InfoDocumentContent): void
  onCommit(content: InfoDocumentContent, failureMessage?: string): Promise<DecryptedInfoDocument | undefined>
  onRequestFile(anchor: FlowInsertionAnchor, accept: string): void
  onRequestPage(anchor: FlowInsertionAnchor): void
  onReadFile(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<Blob>
  onDownloadFile(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<void>
  onResizeFile(file: InfoFileBlock, width: number | undefined): void
  onRemoveFile(file: InfoFileBlock): void
  onSelectDocument(document: DecryptedInfoDocument): void
  onRenameDocument(block: Extract<InfoDocumentBlock, { type: 'document' }>): void
  onRemoveDocument(block: Extract<InfoDocumentBlock, { type: 'document' }>): void
}) => {
  const contentRef = useRef(document.document)
  const editorRefs = useRef(new Map<Uuid, HTMLDivElement>())
  const activeEmptyLineRef = useRef<{ blockId: Uuid; line: HTMLElement } | undefined>(undefined)
  const selectedRangeRef = useRef<Range | undefined>(undefined)
  const draggedBlockIdRef = useRef<Uuid | undefined>(undefined)
  const [menu, setMenu] = useState<{ blockId: Uuid; top: number }>()
  const [toolbar, setToolbar] = useState<{ blockId: Uuid; top: number; left: number }>()
  const [dropLine, setDropLine] = useState<{ blockId: Uuid; index: number; position: 'before' | 'after'; top: number }>()
  const [renamingId, setRenamingId] = useState<Uuid>()
  const [renameDraft, setRenameDraft] = useState('')
  const [selectedBlockId, setSelectedBlockId] = useState<Uuid>()
  const trailingDraftTextIdsRef = useRef(new Map<Uuid, Uuid>())
  let trailingDraftTextId = trailingDraftTextIdsRef.current.get(document.wire.id)
  if (!trailingDraftTextId) {
    trailingDraftTextId = crypto.randomUUID()
    trailingDraftTextIdsRef.current.set(document.wire.id, trailingDraftTextId)
  }
  contentRef.current = document.document
  const lastPersistedBlock = document.document.blocks.at(-1)
  const flowBlocks: InfoDocumentBlock[] = lastPersistedBlock?.type === 'text'
    ? document.document.blocks
    : [
        ...document.document.blocks,
        { id: trailingDraftTextId, type: 'text', markdown: '' },
      ]
  const collapseStates = flowCollapseStates(flowBlocks.map((block) => (
    block.type === 'text'
      ? {
          ...block,
          markdown: markdownWithRememberedOverviewCollapses(
            block.markdown,
            document.wire.id,
            block.id,
          ),
        }
      : block
  )))

  const showsTrailingPrompt = (blockId: Uuid): boolean => {
    const lastBlock = contentRef.current.blocks.at(-1)
    return lastBlock?.type === 'text'
      ? lastBlock.id === blockId
      : blockId === trailingDraftTextId
  }

  const setContent = (content: InfoDocumentContent) => {
    contentRef.current = content
    onDraft(content)
  }

  const editorContent = (blockId: Uuid, markdown: string): InfoDocumentContent => {
    const blockExists = contentRef.current.blocks.some((block) => block.id === blockId)
    return {
      ...contentRef.current,
      blocks: blockExists
        ? updateTextBlock(contentRef.current.blocks, blockId, markdown)
        : [...contentRef.current.blocks, { id: blockId, type: 'text', markdown }],
    }
  }

  const focusTextBlock = (blockId: Uuid) => {
    window.requestAnimationFrame(() => {
      const editor = editorRefs.current.get(blockId)
      if (!editor) return
      editor.focus()
      const target = editor.querySelector<HTMLElement>('[data-overview-prompt] [data-task-text]')
        ?? editor.firstElementChild?.querySelector<HTMLElement>('[data-task-text]')
        ?? editor
      const range = window.document.createRange()
      range.selectNodeContents(target)
      range.collapse(true)
      const selection = window.getSelection()
      selection?.removeAllRanges()
      selection?.addRange(range)
    })
  }

  const selectImageBlock = (blockId: Uuid, target: HTMLElement) => {
    target.closest<HTMLElement>('[data-block-id]')?.focus({ preventScroll: true })
    setSelectedBlockId(blockId)
  }

  const insertLineAfterImage = (blockId: Uuid) => {
    const flowIndex = flowBlocks.findIndex((block) => block.id === blockId)
    const following = flowBlocks[flowIndex + 1]
    if (following?.type === 'text' && following.markdown.trim() === '') {
      focusTextBlock(following.id)
      setSelectedBlockId(undefined)
      return
    }

    const sourceIndex = contentRef.current.blocks.findIndex((block) => block.id === blockId)
    if (sourceIndex < 0) return
    const textId = crypto.randomUUID()
    const content = {
      ...contentRef.current,
      blocks: [
        ...contentRef.current.blocks.slice(0, sourceIndex + 1),
        { id: textId, type: 'text' as const, markdown: '' },
        ...contentRef.current.blocks.slice(sourceIndex + 1),
      ],
    }
    setContent(content)
    setSelectedBlockId(undefined)
    focusTextBlock(textId)
  }

  const syncEditor = (
    blockId: Uuid,
    editor: HTMLDivElement | null,
    markdown: string,
    inheritedCollapsed: boolean,
    showPrompt: boolean,
  ) => {
    if (!editor) {
      editorRefs.current.delete(blockId)
      return
    }
    editorRefs.current.set(blockId, editor)
    if (
      window.document.activeElement !== editor &&
      editor.dataset.sourceMarkdown !== markdown
    ) {
      editor.innerHTML = markdownToOverviewHtml(markdown)
      applyRememberedOverviewCollapses(editor, document.wire.id, blockId)
      editor.dataset.sourceMarkdown = markdown
    }
    ensureOverviewTrailingPromptLine(editor, showPrompt)
    syncOverviewCollapseVisibility(editor, inheritedCollapsed)
  }

  const lineForTarget = (editor: HTMLElement, target: EventTarget | null) => {
    let line = target instanceof HTMLElement
      ? target
      : target instanceof Node
        ? target.parentElement
        : null
    if (line === editor) return editor.lastElementChild as HTMLElement | null
    while (line?.parentElement && line.parentElement !== editor) line = line.parentElement
    return line?.parentElement === editor ? line : null
  }

  const serializeEditor = (blockId: Uuid, editor: HTMLElement) => {
    const markdown = overviewEditorToMarkdown(editor)
    editor.dataset.sourceMarkdown = markdown
    const content = editorContent(blockId, markdown)
    setContent(content)
    return content
  }

  const insertionAnchor = (blockId: Uuid, line: HTMLElement): FlowInsertionAnchor | undefined => {
    const editor = editorRefs.current.get(blockId)
    if (!editor || line.parentElement !== editor) return undefined
    const lines = Array.from(editor.children) as HTMLElement[]
    const index = lines.indexOf(line)
    if (index < 0) return undefined
    return {
      documentId: document.wire.id,
      textBlockId: blockId,
      beforeMarkdown: overviewElementsToMarkdown(lines.slice(0, index)),
      afterMarkdown: overviewElementsToMarkdown(lines.slice(index + 1)),
    }
  }

  const positionMenu = (blockId: Uuid, editor: HTMLElement, line: HTMLElement) => {
    const editorBounds = editor.getBoundingClientRect()
    const lineBounds = line.getBoundingClientRect()
    setMenu({ blockId, top: Math.max(0, lineBounds.top - editorBounds.top) })
  }

  const updateToolbar = (blockId: Uuid) => {
    window.requestAnimationFrame(() => {
      const editor = editorRefs.current.get(blockId)
      const selection = window.getSelection()
      if (
        !editor || !selection || selection.rangeCount === 0 || selection.isCollapsed ||
        !selection.anchorNode || !selection.focusNode ||
        !editor.contains(selection.anchorNode) || !editor.contains(selection.focusNode)
      ) {
        selectedRangeRef.current = undefined
        setToolbar(undefined)
        return
      }
      const range = selection.getRangeAt(0)
      const startLine = lineForTarget(editor, range.startContainer)
      const endLine = lineForTarget(editor, range.endContainer)
      if (!startLine || startLine !== endLine || selection.toString().length === 0) {
        selectedRangeRef.current = undefined
        setToolbar(undefined)
        return
      }
      const bounds = range.getBoundingClientRect()
      const toolbarHalfWidth = Math.min(270, Math.max(0, window.innerWidth / 2 - 12))
      const minimumLeft = 12 + toolbarHalfWidth
      const maximumLeft = Math.max(minimumLeft, window.innerWidth - 12 - toolbarHalfWidth)
      selectedRangeRef.current = range.cloneRange()
      setToolbar({
        blockId,
        top: bounds.top >= 56 ? bounds.top - 52 : bounds.bottom + 8,
        left: Math.max(minimumLeft, Math.min(maximumLeft, bounds.left + bounds.width / 2)),
      })
    })
  }

  const applyFormat = (blockId: Uuid, command: string, value?: string) => {
    const editor = editorRefs.current.get(blockId)
    const range = selectedRangeRef.current
    if (!editor || !range) return
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(range)
    editor.focus()
    window.document.execCommand(command, false, value)
    if (selection?.rangeCount) selectedRangeRef.current = selection.getRangeAt(0).cloneRange()
    serializeEditor(blockId, editor)
    updateToolbar(blockId)
  }

  const toggleInlineCode = (blockId: Uuid) => {
    const editor = editorRefs.current.get(blockId)
    const range = selectedRangeRef.current
    if (!editor || !range) return
    const node = range.commonAncestorContainer instanceof HTMLElement
      ? range.commonAncestorContainer
      : range.commonAncestorContainer.parentElement
    const active = node?.closest<HTMLElement>('code')
    const selection = window.getSelection()
    const nextRange = window.document.createRange()
    if (active && editor.contains(active)) {
      const first = active.firstChild
      const last = active.lastChild
      const parent = active.parentNode
      if (!first || !last || !parent) return
      while (active.firstChild) parent.insertBefore(active.firstChild, active)
      active.remove()
      nextRange.setStartBefore(first)
      nextRange.setEndAfter(last)
    } else {
      const code = window.document.createElement('code')
      code.append(range.extractContents())
      range.insertNode(code)
      nextRange.selectNodeContents(code)
    }
    selection?.removeAllRanges()
    selection?.addRange(nextRange)
    selectedRangeRef.current = nextRange.cloneRange()
    serializeEditor(blockId, editor)
    updateToolbar(blockId)
  }

  const insertMarkdownLine = (
    blockId: Uuid,
    kind: 'h1' | 'h2' | 'h3' | 'bullet' | 'collapse',
  ) => {
    const editor = editorRefs.current.get(blockId)
    if (!editor) return
    const source = activeEmptyLineRef.current?.blockId === blockId
      ? activeEmptyLineRef.current.line
      : editor.lastElementChild as HTMLElement | null
    const next = window.document.createElement(
      kind === 'h1' ? 'h1' : kind === 'h2' ? 'h2' : kind === 'h3' ? 'h3' : 'div',
    )
    if (kind === 'bullet') {
      next.dataset.mdKind = 'bullet'
      next.className = 'tasklist-info-overview-bullet'
    } else if (kind === 'collapse') {
      next.dataset.mdKind = 'collapse'
      next.dataset.collapsed = 'false'
      next.className = 'tasklist-info-overview-collapse'
    }
    next.innerHTML = kind === 'collapse'
      ? '<button type="button" class="tasklist-info-overview-collapse-toggle" contenteditable="false" data-overview-collapse-toggle aria-label="Comprimi capitolo" aria-expanded="true">▼</button><span data-task-text><br></span>'
      : '<span data-task-text><br></span>'
    if (source?.parentElement === editor) source.replaceWith(next)
    else editor.append(next)
    syncOverviewCollapseVisibility(editor)
    ensureOverviewTrailingPromptLine(editor, showsTrailingPrompt(blockId))
    activeEmptyLineRef.current = undefined
    setMenu(undefined)
    const text = next.querySelector<HTMLElement>('[data-task-text]') ?? next
    const range = window.document.createRange()
    range.selectNodeContents(text)
    range.collapse(true)
    editor.focus()
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(range)
    serializeEditor(blockId, editor)
  }

  const handleKeyDown = (blockId: Uuid, event: KeyboardEvent<HTMLDivElement>) => {
    const editor = event.currentTarget
    const selection = window.getSelection()
    const line = lineForTarget(editor, selection?.anchorNode ?? null)
      ?? editor.lastElementChild as HTMLElement | null
    if (
      event.key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey &&
      line && (line.querySelector<HTMLElement>('[data-task-text]')?.textContent ?? line.textContent ?? '').trim() === ''
    ) {
      event.preventDefault()
      activeEmptyLineRef.current = { blockId, line }
      positionMenu(blockId, editor, line)
      return
    }
    if (
      event.key !== 'Enter' || event.shiftKey || event.metaKey || event.ctrlKey ||
      event.altKey || event.nativeEvent.isComposing || line?.dataset.mdKind !== 'collapse'
    ) return
    event.preventDefault()
    const createsChapter = line.dataset.collapsed === 'true'
    const next = window.document.createElement(createsChapter ? 'div' : 'p')
    if (createsChapter) {
      next.dataset.mdKind = 'collapse'
      next.dataset.collapsed = 'false'
      next.className = 'tasklist-info-overview-collapse'
      next.innerHTML = '<button type="button" class="tasklist-info-overview-collapse-toggle" contenteditable="false" data-overview-collapse-toggle aria-label="Comprimi capitolo" aria-expanded="true">▼</button><span data-task-text><br></span>'
      let boundary = line.nextElementSibling as HTMLElement | null
      while (boundary && boundary.dataset.mdKind !== 'collapse' && boundary.dataset.overviewPrompt !== 'true') {
        boundary = boundary.nextElementSibling as HTMLElement | null
      }
      if (boundary) boundary.before(next)
      else editor.append(next)
    } else {
      next.innerHTML = '<span data-task-text><br></span>'
      line.after(next)
    }
    syncOverviewCollapseVisibility(editor)
    ensureOverviewTrailingPromptLine(editor, showsTrailingPrompt(blockId))
    const text = next.querySelector<HTMLElement>('[data-task-text]') ?? next
    const range = window.document.createRange()
    range.selectNodeContents(text)
    range.collapse(true)
    editor.focus()
    selection?.removeAllRanges()
    selection?.addRange(range)
    serializeEditor(blockId, editor)
  }

  const toggleCollapse = (blockId: Uuid, editor: HTMLElement, heading: HTMLElement) => {
    const collapsed = heading.dataset.collapsed !== 'true'
    heading.dataset.collapsed = String(collapsed)
    const headingIndex = Array.from(
      editor.querySelectorAll<HTMLElement>('[data-md-kind="collapse"]'),
    ).indexOf(heading)
    if (headingIndex >= 0) {
      rememberOverviewCollapse(document.wire.id, blockId, headingIndex, collapsed)
    }
    const toggle = heading.querySelector<HTMLElement>('[data-overview-collapse-toggle]')
    if (toggle) {
      toggle.textContent = collapsed ? '▶' : '▼'
      toggle.setAttribute('aria-expanded', String(!collapsed))
      toggle.setAttribute('aria-label', collapsed ? 'Espandi capitolo' : 'Comprimi capitolo')
    }
    syncOverviewCollapseVisibility(editor)
    const content = serializeEditor(blockId, editor)
    void onCommit(content, 'Salvataggio capitolo non riuscito')
  }

  const dropForEvent = (blockId: Uuid, event: DragEvent<HTMLDivElement>) => {
    const line = lineForTarget(event.currentTarget, event.target)
    if (!line) return undefined
    const lines = Array.from(event.currentTarget.children) as HTMLElement[]
    const index = lines.indexOf(line)
    if (index < 0) return undefined
    const bounds = line.getBoundingClientRect()
    const canvas = event.currentTarget.parentElement?.getBoundingClientRect()
    if (!canvas) return undefined
    const position = event.clientY < bounds.top + bounds.height / 2 ? 'before' as const : 'after' as const
    return { blockId, index, position, top: (position === 'before' ? bounds.top : bounds.bottom) - canvas.top }
  }

  const finishDrop = (blockId: Uuid, event: DragEvent<HTMLDivElement>) => {
    event.preventDefault()
    const drop = dropForEvent(blockId, event) ?? dropLine
    const sourceId = draggedBlockIdRef.current || event.dataTransfer.getData('text/plain')
    if (!drop || drop.blockId !== blockId || !sourceId) return
    const editor = event.currentTarget
    const lines = Array.from(editor.children) as HTMLElement[]
    const cut = drop.index + (drop.position === 'after' ? 1 : 0)
    const before = overviewElementsToMarkdown(lines.slice(0, cut))
    const after = overviewElementsToMarkdown(lines.slice(cut))
    const blocks = moveFlowBlockToTextBoundary(
      contentRef.current.blocks,
      sourceId,
      blockId,
      before,
      after,
    )
    const content = { ...contentRef.current, blocks }
    setContent(content)
    void onCommit(content, 'Spostamento blocco non riuscito')
    draggedBlockIdRef.current = undefined
    setDropLine(undefined)
  }

  const renderToolbar = (blockId: Uuid) => toolbar?.blockId === blockId ? createPortal((
    <div
      className="tasklist-info-text-format-toolbar tasklist-info-text-format-toolbar--portal"
      style={{ top: `${toolbar.top}px`, left: `${toolbar.left}px` }}
      role="toolbar"
      aria-label="Formattazione testo"
      onMouseDown={(event) => event.preventDefault()}
    >
      <button type="button" onClick={() => applyFormat(blockId, 'formatBlock', 'h3')} title="Titolo piccolo">H3⌄</button>
      <button type="button" onClick={() => applyFormat(blockId, 'bold')} title="Grassetto"><strong>B</strong></button>
      <button type="button" onClick={() => applyFormat(blockId, 'italic')} title="Corsivo"><em>I</em></button>
      <button type="button" onClick={() => applyFormat(blockId, 'strikeThrough')} title="Barrato"><s>S</s></button>
      <button type="button" onClick={() => applyFormat(blockId, 'underline')} title="Sottolineato"><u>U</u></button>
      <button type="button" onClick={() => {
        const href = window.prompt('Inserisci il link')?.trim()
        if (href) applyFormat(blockId, 'createLink', href)
      }} title="Link">🔗</button>
      <button type="button" onClick={() => applyFormat(blockId, 'formatBlock', 'blockquote')} title="Citazione">❞</button>
      <button type="button" onClick={() => {
        applyFormat(blockId, 'removeFormat')
        applyFormat(blockId, 'unlink')
      }} title="Rimuovi formattazione">×</button>
      <button type="button" onClick={() => toggleInlineCode(blockId)} title="Codice">‹/›</button>
    </div>
  ), window.document.body) : null

  const renderMenu = (blockId: Uuid) => menu?.blockId === blockId ? (
    <div
      className={`tasklist-info-block-inserter is-visible${menu.top < 280 ? ' is-below' : ''}`}
      style={{ top: `${menu.top}px` }}
    >
      <div className="tasklist-info-block-menu" role="menu" aria-label="Tipi di blocco">
        {(['h1', 'h2', 'h3'] as const).map((kind, index) => (
          <button key={kind} type="button" role="menuitem" onPointerDown={(event) => {
            event.preventDefault()
            insertMarkdownLine(blockId, kind)
          }}>
            <span className={`tasklist-info-block-icon tasklist-info-block-icon--${kind}`}>{kind.toUpperCase()}</span>
            <span>{['Titolo grande', 'Titolo medio', 'Titolo piccolo'][index]}</span>
          </button>
        ))}
        <button type="button" role="menuitem" onPointerDown={(event) => { event.preventDefault(); insertMarkdownLine(blockId, 'bullet') }}>
          <ListIcon aria-hidden /><span>Elenco puntato</span>
        </button>
        <button type="button" role="menuitem" onPointerDown={(event) => { event.preventDefault(); insertMarkdownLine(blockId, 'collapse') }}>
          <span className="tasklist-info-block-icon">▸</span><span>Collapse</span>
        </button>
        <button type="button" role="menuitem" onPointerDown={(event) => {
          event.preventDefault()
          const line = activeEmptyLineRef.current?.blockId === blockId
            ? activeEmptyLineRef.current.line
            : undefined
          const anchor = line ? insertionAnchor(blockId, line) : undefined
          if (anchor) onRequestFile(anchor, 'image/*')
          setMenu(undefined)
        }}><ImageIcon aria-hidden /><span>Immagine</span></button>
        <button type="button" role="menuitem" onPointerDown={(event) => {
          event.preventDefault()
          const line = activeEmptyLineRef.current?.blockId === blockId
            ? activeEmptyLineRef.current.line
            : undefined
          const anchor = line ? insertionAnchor(blockId, line) : undefined
          if (anchor) onRequestFile(anchor, '')
          setMenu(undefined)
        }}><PaperclipIcon aria-hidden /><span>File</span></button>
        <button type="button" role="menuitem" onPointerDown={(event) => {
          event.preventDefault()
          const line = activeEmptyLineRef.current?.blockId === blockId
            ? activeEmptyLineRef.current.line
            : undefined
          const anchor = line ? insertionAnchor(blockId, line) : undefined
          if (anchor) onRequestPage(anchor)
          setMenu(undefined)
        }}><FolderIcon aria-hidden /><span>Pagina</span></button>
      </div>
    </div>
  ) : null

  const firstTextBlockId = flowBlocks.find(
    (block) => block.type === 'text',
  )?.id

  return (
    <div className="tasklist-info-flow" data-testid="overview-block-flow">
      {flowBlocks.map((block, blockIndex) => {
        const collapseState = collapseStates.get(block.id)
        if (block.type === 'text') return (
          <div
            className="tasklist-info-overview-canvas tasklist-info-flow-text"
            key={block.id}
            data-block-id={block.id}
            hidden={collapseState?.hiddenByCollapse}
          >
            <div
              ref={(editor) => syncEditor(
                block.id,
                editor,
                block.markdown,
                collapseState?.inheritedCollapsed ?? false,
                blockIndex === flowBlocks.length - 1,
              )}
              className="tasklist-info-overview-editor"
              contentEditable
              suppressContentEditableWarning
              role="textbox"
              aria-multiline="true"
              aria-label={block.id === firstTextBlockId
                ? 'Testo info in Markdown'
                : `Testo info in Markdown ${blockIndex + 1}`}
              data-placeholder="Scrivi una nota o usa / per aggiungere contenuti"
              onPointerDown={(event) => {
                setSelectedBlockId(undefined)
                const prompt = event.target instanceof Element
                  ? event.target.closest<HTMLElement>('[data-overview-prompt]')
                  : null
                if (!prompt || !event.currentTarget.contains(prompt)) return
                event.preventDefault()
                const target = prompt.querySelector<HTMLElement>('[data-task-text]') ?? prompt
                const range = window.document.createRange()
                range.selectNodeContents(target)
                range.collapse(true)
                event.currentTarget.focus()
                const selection = window.getSelection()
                selection?.removeAllRanges()
                selection?.addRange(range)
              }}
              onKeyDown={(event) => handleKeyDown(block.id, event)}
              onInput={(event) => {
                ensureOverviewTrailingPromptLine(
                  event.currentTarget,
                  showsTrailingPrompt(block.id),
                )
                serializeEditor(block.id, event.currentTarget)
              }}
              onKeyUp={() => updateToolbar(block.id)}
              onMouseUp={() => updateToolbar(block.id)}
              onClick={(event) => {
                const toggle = event.target instanceof Element
                  ? event.target.closest('[data-overview-collapse-toggle]')
                  : null
                if (toggle) {
                  const heading = lineForTarget(event.currentTarget, toggle)
                  if (heading) toggleCollapse(block.id, event.currentTarget, heading)
                }
              }}
              onBlur={(event) => {
                if (event.relatedTarget instanceof Node && event.relatedTarget.closest?.('.tasklist-info-block-menu')) return
                setMenu(undefined)
                const content = serializeEditor(block.id, event.currentTarget)
                event.currentTarget.innerHTML = markdownToOverviewHtml(
                  (content.blocks.find((value) => value.id === block.id && value.type === 'text') as { markdown?: string } | undefined)?.markdown ?? '',
                )
                syncOverviewCollapseVisibility(
                  event.currentTarget,
                  collapseState?.inheritedCollapsed ?? false,
                )
                ensureOverviewTrailingPromptLine(
                  event.currentTarget,
                  showsTrailingPrompt(block.id),
                )
                void onCommit(content)
              }}
              onDragOver={(event) => {
                if (!draggedBlockIdRef.current) return
                const drop = dropForEvent(block.id, event)
                if (!drop) return
                event.preventDefault()
                setDropLine(drop)
                const host = event.currentTarget.closest<HTMLElement>('.board-overview-scroll')
                const bounds = host?.getBoundingClientRect()
                if (host && bounds) {
                  if (event.clientY > bounds.bottom - 80) host.scrollTop += 18
                  else if (event.clientY < bounds.top + 80) host.scrollTop -= 18
                }
              }}
              onDrop={(event) => finishDrop(block.id, event)}
            />
            {renderToolbar(block.id)}
            {dropLine?.blockId === block.id && <span className="tasklist-info-text-drop-line" style={{ top: `${dropLine.top}px` }} aria-hidden />}
            {renderMenu(block.id)}
          </div>
        )
        if (block.type === 'file') return (
          <div
            className={`tasklist-info-content-block${selectedBlockId === block.id ? ' is-selected' : ''}`}
            key={block.id}
            data-block-id={block.id}
            hidden={collapseState?.hiddenByCollapse}
            tabIndex={block.content_type.startsWith('image/') ? -1 : undefined}
            onKeyDown={block.content_type.startsWith('image/') ? (event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                insertLineAfterImage(block.id)
              } else if (event.key === 'Delete' || event.key === 'Backspace') {
                event.preventDefault()
                setSelectedBlockId(undefined)
                onRemoveFile(block)
              } else if (event.key === 'Escape') {
                setSelectedBlockId(undefined)
                event.currentTarget.blur()
              }
            } : undefined}
          >
            {block.content_type.startsWith('image/') ? (
              <InfoImageBlock
                document={document}
                file={block}
                onRead={onReadFile}
                onDownload={onDownloadFile}
                onResize={(file, width) => { onResizeFile(file, width); return Promise.resolve() }}
                onRemove={onRemoveFile}
                onDragStart={() => { draggedBlockIdRef.current = block.id }}
                onDragEnd={() => { draggedBlockIdRef.current = undefined; setDropLine(undefined) }}
                selected={selectedBlockId === block.id}
                onSelect={(target) => selectImageBlock(block.id, target)}
              />
            ) : (
              <div className="tasklist-info-file">
                <button type="button" draggable className="tasklist-info-block-drag-handle" aria-label={`Sposta ${block.file_name}`} onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = 'move'
                  event.dataTransfer.setData('text/plain', block.id)
                  draggedBlockIdRef.current = block.id
                }} onDragEnd={() => { draggedBlockIdRef.current = undefined; setDropLine(undefined) }}><GripIcon aria-hidden /></button>
                <FileIcon aria-hidden />
                <button type="button" className="tasklist-info-attachment-link" onClick={() => void onDownloadFile(document, block)}>{block.file_name}</button>
                <button type="button" className="tasklist-info-block-remove tasklist-info-file-remove" aria-label={`Elimina ${block.file_name}`} onClick={() => onRemoveFile(block)}><XIcon aria-hidden /></button>
              </div>
            )}
          </div>
        )
        return (
          <div
            className="tasklist-info-content-block"
            key={block.id}
            data-block-id={block.id}
            hidden={collapseState?.hiddenByCollapse}
          >
            <div className="tasklist-info-document-row">
              <button
                type="button"
                draggable
                className="tasklist-info-block-drag-handle tasklist-info-document-drag-handle"
                aria-label={`Sposta ${block.title}`}
                title="Trascina per spostare"
                onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = 'move'
                  event.dataTransfer.setData('text/plain', block.id)
                  draggedBlockIdRef.current = block.id
                }}
                onDragEnd={() => {
                  draggedBlockIdRef.current = undefined
                  setDropLine(undefined)
                }}
              >
                <GripIcon aria-hidden />
              </button>
              {renamingId === block.id ? (
                <form className="tasklist-info-document-rename" onSubmit={(event) => { event.preventDefault(); onRenameDocument({ ...block, title: renameDraft.trim() || block.title }); setRenamingId(undefined) }}>
                  <FolderIcon aria-hidden />
                  <input autoFocus value={renameDraft} aria-label="Nome pagina" onChange={(event) => setRenameDraft(event.target.value)} onKeyDown={(event) => {
                    if (event.key === 'Escape') { setRenamingId(undefined); setRenameDraft('') }
                  }} />
                </form>
              ) : (
                <button type="button" className="tasklist-info-document-link" onClick={() => {
                  const child = documents.find((value) => value.wire.id === block.document_id)
                  if (child) onSelectDocument(child)
                }}><FolderIcon aria-hidden /><span>{block.title}</span></button>
              )}
              <div className="tasklist-info-document-actions">
                <button type="button" className="tasklist-info-document-action" aria-label={`Rinomina ${block.title}`} disabled={busy} onClick={() => { setRenamingId(block.id); setRenameDraft(block.title) }}><PencilIcon aria-hidden /></button>
                <button type="button" className="tasklist-info-block-remove tasklist-info-document-remove" aria-label={`Elimina ${block.title}`} disabled={busy} onClick={() => onRemoveDocument(block)}><XIcon aria-hidden /></button>
              </div>
            </div>
          </div>
        )
      })}
    </div>
  )
}

export interface InfoDocumentContainer {
  wire: { id: Uuid }
  document?: { name: string }
}

export interface InfoDocumentPanelProps<T extends InfoDocumentContainer> {
  container: T
  presentation?: 'default' | 'overview'
  overviewTitle?: string
  showOverviewTitle?: boolean
  onLoad(container: T): Promise<DecryptedInfoDocument[]>
  onCreateDocument(
    container: T,
    parentDocumentId: Uuid | undefined,
    document: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUpdateDocument(
    document: DecryptedInfoDocument,
    content: InfoDocumentContent,
  ): Promise<DecryptedInfoDocument>
  onUploadFile(
    document: DecryptedInfoDocument,
    file: File,
  ): Promise<InfoFileBlock>
  onReadFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<Blob>
  onDownloadFile(
    document: DecryptedInfoDocument,
    file: InfoFileBlock,
  ): Promise<void>
}

export function InfoDocumentPanel<T extends InfoDocumentContainer>({
  container,
  presentation = 'default',
  overviewTitle,
  showOverviewTitle = true,
  onLoad,
  onCreateDocument,
  onUpdateDocument,
  onUploadFile,
  onReadFile,
  onDownloadFile,
}: InfoDocumentPanelProps<T>) {
  const fileInputRef = useRef<HTMLInputElement>(null)
  const overviewEditorRef = useRef<HTMLDivElement>(null)
  const trailingEditorRef = useRef<HTMLDivElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const titleRef = useRef<HTMLTextAreaElement>(null)
  const activeEmptyBlockRef = useRef<HTMLElement | null>(null)
  const slashMenuTargetRef = useRef<'main' | 'trailing' | null>(null)
  const pendingBlockInsertionRef = useRef<PendingBlockInsertion | undefined>(undefined)
  const pendingFlowInsertionRef = useRef<FlowInsertionAnchor | undefined>(undefined)
  const mainInserterTargetRef = useRef<HTMLElement | null>(null)
  const trailingInserterTargetRef = useRef<HTMLElement | null>(null)
  const selectedTextRangeRef = useRef<Range | null>(null)
  const textFormatToolbarActiveRef = useRef(false)
  const draggedBlockIdRef = useRef<Uuid | undefined>(undefined)
  const dragAutoScrollFrameRef = useRef<number | undefined>(undefined)
  const dragAutoScrollHostRef = useRef<HTMLElement | undefined>(undefined)
  const dragAutoScrollVelocityRef = useRef(0)
  const textDropLineRef = useRef<{
    target: 'main' | 'trailing'
    index: number
    position: 'before' | 'after'
    top: number
  } | undefined>(undefined)
  const loadRef = useRef(onLoad)
  const createRef = useRef(onCreateDocument)
  const containerRef = useRef(container)
  loadRef.current = onLoad
  createRef.current = onCreateDocument
  containerRef.current = container
  const [documents, setDocuments] = useState<DecryptedInfoDocument[]>([])
  const documentsRef = useRef<DecryptedInfoDocument[]>([])
  const authoritativeDocumentsRef = useRef<Map<Uuid, DecryptedInfoDocument>>(new Map())
  const documentMutationQueueRef = useRef(createSerialMutationQueue())
  const documentRevisionRef = useRef<Map<Uuid, number>>(new Map())
  const [currentId, setCurrentId] = useState<Uuid>()
  const [markdown, setMarkdown] = useState('')
  const [trailingMarkdown, setTrailingMarkdown] = useState('')
  const [editing, setEditing] = useState(false)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const [showDocumentForm, setShowDocumentForm] = useState(false)
  const [newDocumentTitle, setNewDocumentTitle] = useState('')
  const [editingDocumentBlockId, setEditingDocumentBlockId] = useState<Uuid>()
  const [documentTitleDraft, setDocumentTitleDraft] = useState('')
  const [reloadToken, setReloadToken] = useState(0)
  const [dirty, setDirty] = useState(false)
  const [overviewDraftRevision, setOverviewDraftRevision] = useState(0)
  const [title, setTitle] = useState('')
  const [blockMenuOpen, setBlockMenuOpen] = useState(false)
  const [trailingBlockMenuOpen, setTrailingBlockMenuOpen] = useState(false)
  const [blockInserterTop, setBlockInserterTop] = useState(0)
  const [trailingBlockInserterTop, setTrailingBlockInserterTop] = useState(0)
  const [textDropLine, setTextDropLine] = useState<{
    target: 'main' | 'trailing'
    index: number
    position: 'before' | 'after'
    top: number
  }>()
  const [textFormatToolbar, setTextFormatToolbar] = useState<{
    target: 'main' | 'trailing'
    top: number
    left: number
  }>()
  const [activeOverviewCheckpoint, setActiveOverviewCheckpoint] = useState('title')
  const [overviewCheckpointHost, setOverviewCheckpointHost] = useState<HTMLElement>()
  const [overviewCollapseCheckpoints, setOverviewCollapseCheckpoints] = useState<Array<{
    id: string
    label: string
    target: 'flow'
    index: number
  }>>([])
  const saveRef = useRef<(content?: InfoDocumentContent) => Promise<DecryptedInfoDocument | undefined>>(
    async () => undefined,
  )

  useLayoutEffect(() => {
    if (presentation !== 'overview' || loading) {
      setOverviewCheckpointHost(undefined)
      return
    }
    const panel = panelRef.current
    setOverviewCheckpointHost(
      panel?.closest<HTMLElement>('.board-secondary-view-panel') ??
      panel?.closest<HTMLElement>('.board-overview-document') ??
      undefined,
    )
  }, [currentId, loading, presentation])

  useEffect(() => {
    let active = true
    setLoading(true)
    setError(undefined)
    void (async () => {
      try {
        let loaded = await loadRef.current(containerRef.current)
        let root = loaded.find((document) => !document.wire.parent_document_id)
        if (!root) {
          try {
            root = await createRef.current(
              containerRef.current,
              undefined,
              emptyDocument(),
            )
            loaded = [...loaded, root]
          } catch {
            loaded = await loadRef.current(containerRef.current)
            root = loaded.find((document) => !document.wire.parent_document_id)
            if (!root) throw new Error('Impossibile inizializzare il documento info')
          }
        }
        if (!active) return
        const rememberedId = presentation === 'overview'
          ? rememberedOverviewDocumentId(containerRef.current.wire.id)
          : undefined
        const initialDocument = loaded.find((document) => document.wire.id === rememberedId) ?? root
        documentsRef.current = loaded
        authoritativeDocumentsRef.current = new Map([
          ...authoritativeDocumentsRef.current,
          ...loaded.map((document) => [document.wire.id, document] as const),
        ])
        setDocuments(loaded)
        setCurrentId(initialDocument.wire.id)
        setMarkdown(markdownFor(initialDocument))
        setTrailingMarkdown(trailingMarkdownFor(initialDocument))
        setEditing(presentation === 'overview')
        setTitle(initialDocument.document.title || overviewTitle || '')
        if (presentation === 'overview') {
          rememberOverviewDocumentId(containerRef.current.wire.id, initialDocument.wire.id)
        }
        setDirty(false)
      } catch (reason) {
        if (active) {
          setError(infoErrorMessage(reason, 'Errore caricamento info'))
        }
      } finally {
        if (active) setLoading(false)
      }
    })()
    return () => {
      active = false
    }
  }, [container.wire.id, overviewTitle, presentation, reloadToken])

  const current = documents.find((document) => document.wire.id === currentId)
  const breadcrumbs = useMemo(() => {
    if (!current) return []
    const values: DecryptedInfoDocument[] = []
    let cursor: DecryptedInfoDocument | undefined = current
    while (cursor) {
      values.unshift(cursor)
      const parentId: Uuid | null = cursor.wire.parent_document_id
      cursor = parentId
        ? documents.find((document) => document.wire.id === parentId)
        : undefined
    }
    return values
  }, [current, documents])

  const selectDocument = (document: DecryptedInfoDocument) => {
    pendingBlockInsertionRef.current = undefined
    if (presentation === 'overview') {
      rememberOverviewDocumentId(container.wire.id, document.wire.id)
    }
    setCurrentId(document.wire.id)
    setMarkdown(markdownFor(document))
    setTrailingMarkdown(trailingMarkdownFor(document))
    setTitle(document.document.title || overviewTitle || '')
    setEditing(presentation === 'overview')
    setDirty(false)
    setBlockMenuOpen(false)
    setTrailingBlockMenuOpen(false)
  }

  const replaceDocument = (next: DecryptedInfoDocument) => {
    const nextValues = documentsRef.current.map((value) => (
      value.wire.id === next.wire.id ? next : value
    ))
    documentsRef.current = nextValues
    setDocuments(nextValues)
  }

  const enqueueDocumentUpdate = (
    documentId: Uuid,
    content: InfoDocumentContent,
    failureMessage = 'Salvataggio non riuscito',
  ): Promise<DecryptedInfoDocument | undefined> => {
    const revision = (documentRevisionRef.current.get(documentId) ?? 0) + 1
    documentRevisionRef.current.set(documentId, revision)

    const currentDraft = documentsRef.current.find(
      (document) => document.wire.id === documentId,
    )
    if (currentDraft) replaceDocument({ ...currentDraft, document: content })

    const run = async () => {
      const base = authoritativeDocumentsRef.current.get(documentId)
        ?? documentsRef.current.find((document) => document.wire.id === documentId)
      if (!base) {
        setError(failureMessage)
        return undefined
      }
      try {
        const next = await onUpdateDocument(base, content)
        authoritativeDocumentsRef.current.set(documentId, next)
        const hasNewerDraft = (documentRevisionRef.current.get(documentId) ?? 0) > revision
        const visibleDocument = documentsRef.current.find(
          (document) => document.wire.id === documentId,
        )
        replaceDocument(
          hasNewerDraft && visibleDocument
            ? { ...next, document: visibleDocument.document }
            : next,
        )
        return next
      } catch (reason) {
        setError(infoErrorMessage(reason, failureMessage))
        return undefined
      }
    }
    return documentMutationQueueRef.current(run)
  }

  const save = async (content?: InfoDocumentContent) => {
    if (!current) return undefined
    setBusy(true)
    setError(undefined)
    const latest = documentsRef.current.find(
      (document) => document.wire.id === current.wire.id,
    ) ?? current
    const snapshot = content ?? (presentation === 'overview'
      ? { ...latest.document, title }
      : { ...withMarkdown(latest.document, markdown) })
    try {
      const next = await enqueueDocumentUpdate(current.wire.id, snapshot)
      if (!next) return undefined
      if (presentation !== 'overview') {
        setMarkdown(markdownFor(next))
        setTrailingMarkdown(trailingMarkdownFor(next))
      }
      setEditing(presentation === 'overview')
      const visible = documentsRef.current.find(
        (document) => document.wire.id === current.wire.id,
      )
      if (visible?.document === snapshot || visible?.document === next.document) {
        setDirty(false)
      }
      return next
    } finally {
      setBusy(false)
    }
  }
  saveRef.current = save

  useEffect(() => {
    if (
      presentation !== 'overview' ||
      !dirty ||
      !current ||
      busy
    ) return
    const timeout = window.setTimeout(() => {
      void saveRef.current()
    }, 900)
    return () => window.clearTimeout(timeout)
  }, [busy, current, dirty, markdown, overviewDraftRevision, presentation, title, trailingMarkdown])

  useEffect(() => {
    const editor = overviewEditorRef.current
    if (
      !editor ||
      presentation !== 'overview' ||
      window.document.activeElement === editor
    ) return
    editor.innerHTML = markdownToOverviewHtml(markdown)
    syncOverviewCollapseVisibility(editor)
    ensureOverviewTrailingPromptLine(editor)
    const last = editor.lastElementChild as HTMLElement | null
    mainInserterTargetRef.current = last
    setBlockInserterTop(
      last ? last.offsetTop + Math.max(0, last.offsetHeight - 36) : 0,
    )
  }, [currentId, markdown, presentation])

  useEffect(() => {
    const editor = trailingEditorRef.current
    if (!editor || window.document.activeElement === editor) return
    editor.innerHTML = markdownToOverviewHtml(trailingMarkdown)
    syncOverviewCollapseVisibility(editor)
    ensureOverviewTrailingPromptLine(editor)
    const last = editor.lastElementChild as HTMLElement | null
    trailingInserterTargetRef.current = last
    setTrailingBlockInserterTop(
      last ? last.offsetTop + Math.max(0, last.offsetHeight - 36) : 0,
    )
  }, [currentId, trailingMarkdown])

  useLayoutEffect(() => {
    const textarea = titleRef.current
    if (!textarea || presentation !== 'overview') return
    textarea.style.minHeight = '0'
    textarea.style.height = 'auto'
    textarea.style.height = `${textarea.scrollHeight}px`
  }, [presentation, title])

  useEffect(() => {
    if (presentation !== 'overview') return
    const frame = window.requestAnimationFrame(() => {
      const checkpoints = Array.from(
        panelRef.current?.querySelectorAll<HTMLElement>(
          '.tasklist-info-flow [data-md-kind="collapse"]',
        ) ?? [],
      ).map((element, index) => ({
          id: `flow-${index}`,
          label:
            element.querySelector<HTMLElement>('[data-task-text]')?.textContent?.trim() ||
            'Capitolo',
          target: 'flow' as const,
          index,
        }))
      setOverviewCollapseCheckpoints(checkpoints)
    })
    return () => window.cancelAnimationFrame(frame)
  }, [currentId, documents, presentation])

  useEffect(() => {
    if (presentation !== 'overview') return
    const titleElement = titleRef.current
    const scrollHost = titleElement?.closest<HTMLElement>('.board-overview-scroll')
    if (!titleElement || !scrollHost) return

    const updateActiveCheckpoint = () => {
      const anchors = [
        { id: 'title', element: titleElement },
        ...Array.from(
          panelRef.current?.querySelectorAll<HTMLElement>(
            '.tasklist-info-flow [data-md-kind="collapse"]',
          ) ?? [],
        ).map((element, index) => ({ id: `flow-${index}`, element })),
      ]
      const hostBounds = scrollHost.getBoundingClientRect()
      const readingLine = hostBounds.top + Math.min(180, hostBounds.height * 0.35)
      let activeId = anchors[0].id
      for (const anchor of anchors) {
        if (anchor.element.getBoundingClientRect().top > readingLine) break
        activeId = anchor.id
      }
      setActiveOverviewCheckpoint((previous) => previous === activeId ? previous : activeId)
    }

    const frame = window.requestAnimationFrame(updateActiveCheckpoint)
    scrollHost.addEventListener('scroll', updateActiveCheckpoint, { passive: true })
    window.addEventListener('resize', updateActiveCheckpoint)
    return () => {
      window.cancelAnimationFrame(frame)
      scrollHost.removeEventListener('scroll', updateActiveCheckpoint)
      window.removeEventListener('resize', updateActiveCheckpoint)
    }
  }, [currentId, overviewCollapseCheckpoints.length, presentation])

  const directEditorBlock = (
    editor: HTMLElement,
    eventTarget: EventTarget | null,
  ): HTMLElement | null => {
    let block = eventTarget instanceof HTMLElement ? eventTarget : null
    if (block === editor) return editor.lastElementChild as HTMLElement | null
    while (block?.parentElement && block.parentElement !== editor) {
      block = block.parentElement
    }
    return block?.parentElement === editor ? block : null
  }

  const toggleOverviewCollapse = (
    editor: HTMLElement,
    heading: HTMLElement,
  ) => {
    const collapsed = heading.dataset.collapsed !== 'true'
    heading.dataset.collapsed = String(collapsed)
    const toggle = heading.querySelector<HTMLElement>('[data-overview-collapse-toggle]')
    if (toggle) {
      toggle.textContent = collapsed ? '▶' : '▼'
      toggle.setAttribute('aria-expanded', String(!collapsed))
      toggle.setAttribute('aria-label', collapsed ? 'Espandi capitolo' : 'Comprimi capitolo')
    }
    let sibling = heading.nextElementSibling as HTMLElement | null
    while (sibling && sibling.parentElement === editor) {
      if (sibling.dataset.mdKind === 'collapse') break
      sibling.classList.toggle('is-collapsed-by-chapter', collapsed)
      sibling = sibling.nextElementSibling as HTMLElement | null
    }
    const text = heading.querySelector<HTMLElement>('[data-task-text]')
    if (text) {
      editor.focus({ preventScroll: true })
      const selection = window.getSelection()
      const range = window.document.createRange()
      range.selectNodeContents(text)
      range.collapse(false)
      selection?.removeAllRanges()
      selection?.addRange(range)
    }
    const nextMarkdown = overviewEditorToMarkdown(editor)
    if (editor === overviewEditorRef.current) setMarkdown(nextMarkdown)
    else if (editor === trailingEditorRef.current) setTrailingMarkdown(nextMarkdown)
    setDirty(true)
    if (current) {
      const content = editor === overviewEditorRef.current
        ? withTrailingMarkdown(withMarkdown(current.document, nextMarkdown), trailingMarkdown)
        : withTrailingMarkdown(withMarkdown(current.document, markdown), nextMarkdown)
      void saveRef.current({
        ...content,
        ...(presentation === 'overview' ? { title } : {}),
      })
    }
  }

  const updateTextFormatToolbar = useCallback((target: 'main' | 'trailing') => {
    window.requestAnimationFrame(() => {
      const editor = target === 'main'
        ? overviewEditorRef.current
        : trailingEditorRef.current
      const selection = window.getSelection()
      if (
        !editor ||
        !selection ||
        selection.rangeCount === 0 ||
        selection.isCollapsed ||
        !selection.anchorNode ||
        !selection.focusNode ||
        !editor.contains(selection.anchorNode) ||
        !editor.contains(selection.focusNode)
      ) {
        selectedTextRangeRef.current = null
        setTextFormatToolbar(undefined)
        return
      }
      const range = selection.getRangeAt(0)
      const blockForNode = (node: Node): HTMLElement | null => {
        let block = node instanceof HTMLElement ? node : node.parentElement
        while (block?.parentElement && block.parentElement !== editor) {
          block = block.parentElement
        }
        return block?.parentElement === editor ? block : null
      }
      if (blockForNode(range.startContainer) !== blockForNode(range.endContainer)) {
        selectedTextRangeRef.current = null
        setTextFormatToolbar(undefined)
        return
      }
      const bounds = range.getBoundingClientRect()
      const canvasBounds = editor.parentElement?.getBoundingClientRect()
      if (!canvasBounds || selection.toString().length === 0) {
        selectedTextRangeRef.current = null
        setTextFormatToolbar(undefined)
        return
      }
      selectedTextRangeRef.current = range.cloneRange()
      setTextFormatToolbar({
        target,
        top: Math.max(0, bounds.top - canvasBounds.top - 48),
        left: Math.max(144, Math.min(canvasBounds.width - 144, bounds.left + bounds.width / 2 - canvasBounds.left)),
      })
    })
  }, [])

  useEffect(() => {
    let frame = 0
    const handleSelectionChange = () => {
      if (textFormatToolbarActiveRef.current) return
      window.cancelAnimationFrame(frame)
      frame = window.requestAnimationFrame(() => {
        const selection = window.getSelection()
        const anchor = selection?.anchorNode
        const focus = selection?.focusNode
        if (
          anchor &&
          focus &&
          overviewEditorRef.current?.contains(anchor) &&
          overviewEditorRef.current.contains(focus)
        ) {
          updateTextFormatToolbar('main')
          return
        }
        if (
          anchor &&
          focus &&
          trailingEditorRef.current?.contains(anchor) &&
          trailingEditorRef.current.contains(focus)
        ) {
          updateTextFormatToolbar('trailing')
          return
        }
        selectedTextRangeRef.current = null
        setTextFormatToolbar(undefined)
      })
    }
    window.document.addEventListener('selectionchange', handleSelectionChange)
    return () => {
      window.cancelAnimationFrame(frame)
      window.document.removeEventListener('selectionchange', handleSelectionChange)
    }
  }, [updateTextFormatToolbar])

  const applyTextFormats = (
    commands: Array<{ command: string; value?: string }>,
    target: 'main' | 'trailing',
  ) => {
    const editor = target === 'main'
      ? overviewEditorRef.current
      : trailingEditorRef.current
    const range = selectedTextRangeRef.current
    if (!editor || !range) return
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(range)
    editor.focus()
    commands.forEach(({ command, value }) => {
      window.document.execCommand(command, false, value)
      if (selection?.rangeCount) {
        selectedTextRangeRef.current = selection.getRangeAt(0).cloneRange()
      }
    })
    let removedEmptyWrapper = true
    while (removedEmptyWrapper) {
      removedEmptyWrapper = false
      editor.querySelectorAll<HTMLElement>(
        'strong:empty, b:empty, em:empty, i:empty, s:empty, strike:empty, u:empty, a:empty, code:empty',
      ).forEach((element) => {
        element.remove()
        removedEmptyWrapper = true
      })
    }
    textFormatToolbarActiveRef.current = false
    const next = overviewEditorToMarkdown(editor)
    if (target === 'main') setMarkdown(next)
    else setTrailingMarkdown(next)
    setDirty(true)
    updateTextFormatToolbar(target)
  }

  const applyTextFormat = (
    command: string,
    value: string | undefined,
    target: 'main' | 'trailing',
  ) => applyTextFormats([{ command, value }], target)

  const selectedTextBlock = (target: 'main' | 'trailing'): HTMLElement | null => {
    const editor = target === 'main'
      ? overviewEditorRef.current
      : trailingEditorRef.current
    const range = selectedTextRangeRef.current
    if (!editor || !range) return null
    let block = range.startContainer instanceof HTMLElement
      ? range.startContainer
      : range.startContainer.parentElement
    while (block?.parentElement && block.parentElement !== editor) {
      block = block.parentElement
    }
    return block?.parentElement === editor ? block : null
  }

  const selectedFormatElement = (
    target: 'main' | 'trailing',
    selector: string,
  ): HTMLElement | null => {
    const editor = target === 'main'
      ? overviewEditorRef.current
      : trailingEditorRef.current
    const range = selectedTextRangeRef.current
    if (!editor || !range) return null
    const start = range.startContainer instanceof HTMLElement
      ? range.startContainer
      : range.startContainer.parentElement
    const end = range.endContainer instanceof HTMLElement
      ? range.endContainer
      : range.endContainer.parentElement
    const startMatch = start?.closest<HTMLElement>(selector) ?? null
    const endMatch = end?.closest<HTMLElement>(selector) ?? null
    if (startMatch && startMatch === endMatch && editor.contains(startMatch)) return startMatch
    const common = range.commonAncestorContainer
    const element = common instanceof HTMLElement ? common : common.parentElement
    const containedMatch = element?.querySelector<HTMLElement>(selector) ?? null
    return containedMatch && editor.contains(containedMatch) && range.intersectsNode(containedMatch)
      ? containedMatch
      : null
  }

  const toggleBlockFormat = (
    target: 'main' | 'trailing',
    tag: 'h3' | 'blockquote',
  ) => {
    if (selectedTextBlock(target)?.dataset.mdKind) return
    const active = selectedFormatElement(target, tag)
    applyTextFormat('formatBlock', active ? 'p' : tag, target)
  }

  const toggleCodeFormat = (target: 'main' | 'trailing') => {
    const editor = target === 'main'
      ? overviewEditorRef.current
      : trailingEditorRef.current
    const savedRange = selectedTextRangeRef.current
    if (!editor || !savedRange || selectedTextBlock(target)?.dataset.mdKind) return
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(savedRange)
    const activeCode = selectedFormatElement(target, 'code, pre')
    const nextRange = window.document.createRange()
    if (activeCode?.tagName === 'CODE') {
      const first = activeCode.firstChild
      const last = activeCode.lastChild
      const parent = activeCode.parentNode
      if (!first || !last || !parent) return
      while (activeCode.firstChild) parent.insertBefore(activeCode.firstChild, activeCode)
      activeCode.remove()
      nextRange.setStartBefore(first)
      nextRange.setEndAfter(last)
    } else {
      const code = window.document.createElement('code')
      code.append(savedRange.extractContents())
      savedRange.insertNode(code)
      nextRange.selectNodeContents(code)
    }
    selection?.removeAllRanges()
    selection?.addRange(nextRange)
    selectedTextRangeRef.current = nextRange.cloneRange()
    textFormatToolbarActiveRef.current = false
    const next = overviewEditorToMarkdown(editor)
    if (target === 'main') setMarkdown(next)
    else setTrailingMarkdown(next)
    setDirty(true)
    updateTextFormatToolbar(target)
  }

  const renderTextFormatToolbar = (target: 'main' | 'trailing') => {
    if (textFormatToolbar?.target !== target) return null
    const run = (command: string, value?: string) =>
      applyTextFormat(command, value, target)
    return (
      <div
        className="tasklist-info-text-format-toolbar"
        style={{
          top: `${textFormatToolbar.top}px`,
          left: `${textFormatToolbar.left}px`,
        }}
        role="toolbar"
        aria-label="Formattazione testo"
        onMouseDown={(event) => {
          textFormatToolbarActiveRef.current = true
          event.preventDefault()
        }}
      >
        <button type="button" onClick={() => toggleBlockFormat(target, 'h3')} title="Titolo piccolo">H3⌄</button>
        <button type="button" onClick={() => run('bold')} title="Grassetto"><strong>B</strong></button>
        <button type="button" onClick={() => run('italic')} title="Corsivo"><em>I</em></button>
        <button type="button" onClick={() => run('strikeThrough')} title="Barrato"><s>S</s></button>
        <button type="button" onClick={() => run('underline')} title="Sottolineato"><u>U</u></button>
        <button type="button" onClick={() => {
          if (selectedFormatElement(target, 'a')) {
            run('unlink')
          } else {
            const href = window.prompt('Inserisci il link')?.trim()
            if (href) run('createLink', href)
            else textFormatToolbarActiveRef.current = false
          }
        }} title="Link">🔗</button>
        <button type="button" onClick={() => toggleBlockFormat(target, 'blockquote')} title="Citazione">❞</button>
        <button type="button" onClick={() => {
          applyTextFormats([
            { command: 'removeFormat' },
            { command: 'unlink' },
          ], target)
        }} title="Rimuovi formattazione">×</button>
        <button type="button" onClick={() => toggleCodeFormat(target)} title="Codice">‹/›</button>
      </div>
    )
  }

  const positionBlockInserter = (
    target: 'main' | 'trailing' = 'main',
    explicitBlock?: HTMLElement | null,
  ) => {
    window.requestAnimationFrame(() => {
      const editor = target === 'main'
        ? overviewEditorRef.current
        : trailingEditorRef.current
      if (!editor) return
      const selection = window.getSelection()
      let block = explicitBlock ?? (
        selection?.anchorNode instanceof HTMLElement
          ? selection.anchorNode
          : selection?.anchorNode?.parentElement
      )
      while (block?.parentElement && block.parentElement !== editor) {
        block = block.parentElement
      }
      if (block?.parentElement !== editor) block = undefined
      const positionedBlock = block ?? editor.lastElementChild as HTMLElement | null
      const blockText = positionedBlock?.querySelector<HTMLElement>('[data-task-text]')?.textContent
        ?? positionedBlock?.textContent
        ?? ''
      activeEmptyBlockRef.current = blockText.trim() === '' ? positionedBlock : null
      const editorRect = editor.getBoundingClientRect()
      const selectionRange = selection?.rangeCount
        ? selection.getRangeAt(0).cloneRange()
        : undefined
      const caretRect = selectionRange && typeof selectionRange.getClientRects === 'function'
        ? Array.from(selectionRange.getClientRects())[0]
        : undefined
      const selectionInsideEditor = selection?.anchorNode
        ? editor.contains(selection.anchorNode)
        : false
      const top = selectionInsideEditor && caretRect
        ? Math.max(0, caretRect.top - editorRect.top)
        : positionedBlock?.offsetTop ?? 0
      if (target === 'main') {
        mainInserterTargetRef.current = positionedBlock
        setBlockInserterTop(top)
      } else {
        trailingInserterTargetRef.current = positionedBlock
        setTrailingBlockInserterTop(top)
      }
    })
  }

  const openBlockMenuFromSlash = (
    target: 'main' | 'trailing',
    event: KeyboardEvent<HTMLDivElement>,
  ) => {
    const menuOpen = target === 'main' ? blockMenuOpen : trailingBlockMenuOpen
    if (menuOpen || slashMenuTargetRef.current === target) {
      if (target === 'main') setBlockMenuOpen(false)
      else setTrailingBlockMenuOpen(false)
      slashMenuTargetRef.current = null
    }
    if (event.key !== '/' || event.metaKey || event.ctrlKey || event.altKey) return
    const editor = target === 'main' ? overviewEditorRef.current : trailingEditorRef.current
    if (!editor) return
    const selection = window.getSelection()
    let block = selection?.anchorNode instanceof HTMLElement
      ? selection.anchorNode
      : selection?.anchorNode?.parentElement
    while (block?.parentElement && block.parentElement !== editor) {
      block = block.parentElement
    }
    if (block?.parentElement !== editor) {
      block = editor.lastElementChild as HTMLElement | null
    }
    if (!block) {
      block = window.document.createElement('p')
      block.innerHTML = '<br>'
      editor.append(block)
    }
    const text = block.querySelector<HTMLElement>('[data-task-text]')?.textContent
      ?? block.textContent
      ?? ''
    if (text.trim() !== '') return
    event.preventDefault()
    activeEmptyBlockRef.current = block
    slashMenuTargetRef.current = target
    if (target === 'main') {
      setBlockMenuOpen(true)
    } else {
      setTrailingBlockMenuOpen(true)
    }
    positionBlockInserter(target, block)
  }

  const handleOverviewEditorKeyDown = (
    target: 'main' | 'trailing',
    event: KeyboardEvent<HTMLDivElement>,
  ) => {
    openBlockMenuFromSlash(target, event)
    if (
      event.key !== 'Enter' ||
      event.shiftKey ||
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      event.nativeEvent.isComposing
    ) return

    const editor = target === 'main' ? overviewEditorRef.current : trailingEditorRef.current
    if (!editor) return
    const selection = window.getSelection()
    const selectionTarget = selection?.anchorNode instanceof HTMLElement
      ? selection.anchorNode
      : selection?.anchorNode?.parentElement ?? null
    const activeBlock = directEditorBlock(editor, selectionTarget)
    if (activeBlock?.dataset.mdKind !== 'collapse') return

    event.preventDefault()
    const collapseToggle = activeBlock.querySelector<HTMLElement>(
      '[data-overview-collapse-toggle]',
    )
    const createsChapter =
      activeBlock.dataset.collapsed === 'true' ||
      collapseToggle?.getAttribute('aria-expanded') === 'false' ||
      collapseToggle?.textContent?.trim() === '▶'
    const nextBlock = window.document.createElement(createsChapter ? 'div' : 'p')
    if (createsChapter) {
      nextBlock.dataset.mdKind = 'collapse'
      nextBlock.dataset.collapsed = 'false'
      nextBlock.className = 'tasklist-info-overview-collapse'
      nextBlock.innerHTML = '<button type="button" class="tasklist-info-overview-collapse-toggle" contenteditable="false" data-overview-collapse-toggle aria-label="Comprimi capitolo" aria-expanded="true">▼</button><span data-task-text><br></span>'
    } else {
      nextBlock.innerHTML = '<span data-task-text><br></span>'
    }
    if (createsChapter) {
      let sectionBoundary = activeBlock.nextElementSibling as HTMLElement | null
      while (sectionBoundary && sectionBoundary.dataset.mdKind !== 'collapse') {
        const boundaryText = sectionBoundary
          .querySelector<HTMLElement>('[data-task-text]')
          ?.textContent
          ?.trim() ?? ''
        const isTrailingPrompt = !sectionBoundary.nextElementSibling && boundaryText === ''
        if (isTrailingPrompt) break
        sectionBoundary = sectionBoundary.nextElementSibling as HTMLElement | null
      }
      if (sectionBoundary) sectionBoundary.before(nextBlock)
      else editor.append(nextBlock)
    } else {
      activeBlock.after(nextBlock)
    }

    const text = nextBlock.querySelector<HTMLElement>('[data-task-text]')
    if (text) {
      const range = window.document.createRange()
      range.selectNodeContents(text)
      range.collapse(true)
      selection?.removeAllRanges()
      selection?.addRange(range)
    }

    syncOverviewCollapseVisibility(editor)
    const nextMarkdown = overviewEditorToMarkdown(editor)
    if (target === 'main') setMarkdown(nextMarkdown)
    else setTrailingMarkdown(nextMarkdown)
    setDirty(true)
    positionBlockInserter(target, nextBlock)
  }

  const insertBlock = (
    kind: 'h1' | 'h2' | 'h3' | 'bullet' | 'collapse',
    target: 'main' | 'trailing' = 'main',
  ) => {
    const editor = target === 'main' ? overviewEditorRef.current : trailingEditorRef.current
    if (!editor) return
    const next = window.document.createElement(
      kind === 'h1' ? 'h1' : kind === 'h2' ? 'h2' : kind === 'h3' ? 'h3' : 'div',
    )
    if (kind === 'bullet') {
      next.dataset.mdKind = 'bullet'
      next.className = 'tasklist-info-overview-bullet'
    } else if (kind === 'collapse') {
      next.dataset.mdKind = 'collapse'
      next.dataset.collapsed = 'false'
      next.className = 'tasklist-info-overview-collapse'
    }
    const activeEmptyBlock = activeEmptyBlockRef.current?.parentElement === editor
      ? activeEmptyBlockRef.current
      : null
    const sourceBlock = activeEmptyBlock ?? (target === 'main'
      ? mainInserterTargetRef.current
      : trailingInserterTargetRef.current)
    const rawSourceText = sourceBlock?.querySelector<HTMLElement>('[data-task-text]')?.textContent
      ?? sourceBlock?.textContent
      ?? ''
    const sourceText = rawSourceText.trim() === '/' ? '' : rawSourceText
    next.innerHTML = kind === 'collapse'
      ? `<button type="button" class="tasklist-info-overview-collapse-toggle" contenteditable="false" data-overview-collapse-toggle aria-label="Comprimi capitolo" aria-expanded="true">▼</button><span data-task-text>${escapeHtml(sourceText) || '<br>'}</span>`
      : `<span data-task-text>${escapeHtml(sourceText) || '<br>'}</span>`
    if (sourceBlock?.parentElement === editor) {
      sourceBlock.replaceWith(next)
    } else {
      editor.append(next)
    }
    if (!next.nextElementSibling) {
      const promptLine = window.document.createElement('p')
      promptLine.innerHTML = '<span data-task-text><br></span>'
      next.after(promptLine)
    }
    syncOverviewCollapseVisibility(editor)
    activeEmptyBlockRef.current = null
    slashMenuTargetRef.current = null
    if (target === 'main') mainInserterTargetRef.current = next
    else trailingInserterTargetRef.current = next
    editor.focus()
    const selection = window.getSelection()
    const range = window.document.createRange()
    const textTarget = next.querySelector<HTMLElement>('[data-task-text]') ?? next
    range.selectNodeContents(textTarget)
    range.collapse(true)
    selection?.removeAllRanges()
    selection?.addRange(range)
    const nextMarkdown = overviewEditorToMarkdown(editor)
    if (target === 'main') setMarkdown(nextMarkdown)
    else setTrailingMarkdown(nextMarkdown)
    setDirty(true)
    if (target === 'main') setBlockMenuOpen(false)
    else setTrailingBlockMenuOpen(false)
    if (target === 'main') positionBlockInserter()
  }

  const capturePendingInsertion = (
    target: 'main' | 'trailing',
  ): PendingBlockInsertion | undefined => {
    const editor = target === 'main'
      ? overviewEditorRef.current
      : trailingEditorRef.current
    const activeEmptyBlock = activeEmptyBlockRef.current?.parentElement === editor
      ? activeEmptyBlockRef.current
      : null
    const sourceBlock = activeEmptyBlock ?? (target === 'main'
      ? mainInserterTargetRef.current
      : trailingInserterTargetRef.current)
    if (!editor || !sourceBlock || sourceBlock.parentElement !== editor) {
      return undefined
    }
    const elements = Array.from(editor.children) as HTMLElement[]
    const index = elements.indexOf(sourceBlock)
    if (index < 0) return undefined
    return {
      target,
      before: overviewElementsToMarkdown(elements.slice(0, index)),
      after: overviewElementsToMarkdown(elements.slice(index + 1)),
    }
  }

  const openFilePicker = (
    accept: string,
    target?: 'main' | 'trailing',
  ) => {
    const input = fileInputRef.current
    if (!input) return
    pendingBlockInsertionRef.current = target
      ? capturePendingInsertion(target)
      : undefined
    input.accept = accept
    try {
      input.showPicker()
    } catch {
      input.click()
    }
    slashMenuTargetRef.current = null
    setBlockMenuOpen(false)
    setTrailingBlockMenuOpen(false)
  }

  const prepareFlowInsertion = (anchor: FlowInsertionAnchor): FlowInsertionAnchor => {
    const draft = documentsRef.current.find(
      (document) => document.wire.id === anchor.documentId,
    )
    if (!draft) return anchor
    const textIndex = draft.document.blocks.findIndex((block) => (
      block.id === anchor.textBlockId && block.type === 'text'
    ))
    if (textIndex < 0) return anchor
    const source = draft.document.blocks[textIndex]
    if (source.type !== 'text') return anchor
    const afterTextBlockId = crypto.randomUUID()
    const content: InfoDocumentContent = {
      ...draft.document,
      blocks: [
        ...draft.document.blocks.slice(0, textIndex),
        { ...source, markdown: anchor.beforeMarkdown },
        { id: afterTextBlockId, type: 'text', markdown: anchor.afterMarkdown },
        ...draft.document.blocks.slice(textIndex + 1),
      ],
    }
    replaceDocument({ ...draft, document: content })
    void enqueueDocumentUpdate(
      draft.wire.id,
      content,
      'Preparazione inserimento non riuscita',
    )
    return { ...anchor, afterTextBlockId }
  }

  const rollbackFlowInsertion = async (
    anchor = pendingFlowInsertionRef.current,
  ): Promise<void> => {
    if (!anchor?.afterTextBlockId) return
    const draft = documentsRef.current.find(
      (document) => document.wire.id === anchor.documentId,
    )
    if (!draft) return
    const beforeIndex = draft.document.blocks.findIndex(
      (block) => block.id === anchor.textBlockId && block.type === 'text',
    )
    const afterIndex = draft.document.blocks.findIndex(
      (block) => block.id === anchor.afterTextBlockId && block.type === 'text',
    )
    const before = draft.document.blocks[beforeIndex]
    const after = draft.document.blocks[afterIndex]
    const canRollback =
      beforeIndex >= 0 &&
      afterIndex === beforeIndex + 1 &&
      before?.type === 'text' &&
      after?.type === 'text'
    if (!canRollback || before?.type !== 'text' || after?.type !== 'text') return

    const content: InfoDocumentContent = {
      ...draft.document,
      blocks: [
        ...draft.document.blocks.slice(0, beforeIndex),
        {
          ...before,
          markdown: [before.markdown, after.markdown]
            .filter((value) => value.length > 0)
            .join('\n'),
        },
        ...draft.document.blocks.slice(afterIndex + 1),
      ],
    }
    await enqueueDocumentUpdate(
      draft.wire.id,
      content,
      'Ripristino inserimento non riuscito',
    )
  }

  useEffect(() => {
    const input = fileInputRef.current
    if (!input) return undefined
    const rollbackCancelledPicker = () => {
      void rollbackFlowInsertion().finally(() => {
        pendingFlowInsertionRef.current = undefined
      })
    }
    input.addEventListener('cancel', rollbackCancelledPicker)
    return () => input.removeEventListener('cancel', rollbackCancelledPicker)
  })

  const addFile = async (file: File) => {
    if (!current) return
    const documentId = current.wire.id
    const flowAnchor = pendingFlowInsertionRef.current
    const legacyPending = pendingBlockInsertionRef.current
    setBusy(true)
    setError(undefined)
    try {
      const uploadBase = authoritativeDocumentsRef.current.get(documentId) ?? current
      const block = await onUploadFile(uploadBase, file)
      const latest = documentsRef.current.find((document) => document.wire.id === documentId)
      if (!latest) throw new Error('Documento non più disponibile')
      let nextContent = latest.document
      if (presentation === 'overview' && flowAnchor?.documentId === documentId) {
        const afterIndex = flowAnchor.afterTextBlockId
          ? nextContent.blocks.findIndex((value) => value.id === flowAnchor.afterTextBlockId)
          : -1
        nextContent = afterIndex >= 0
          ? {
              ...nextContent,
              blocks: [
                ...nextContent.blocks.slice(0, afterIndex),
                block,
                ...nextContent.blocks.slice(afterIndex),
              ],
            }
          : {
              ...nextContent,
              blocks: splitTextBlockWith(
                nextContent.blocks,
                flowAnchor.textBlockId,
                flowAnchor.beforeMarkdown,
                block,
                flowAnchor.afterMarkdown,
              ),
            }
      } else if (legacyPending) {
        nextContent = insertBlockAtPendingPosition(nextContent, block, legacyPending)
      } else {
        nextContent = {
          ...nextContent,
          blocks: [...nextContent.blocks, block, {
            id: crypto.randomUUID(), type: 'text' as const, markdown: '',
          }],
        }
      }
      const next = await enqueueDocumentUpdate(documentId, nextContent, 'Collegamento upload non riuscito')
      if (next && presentation !== 'overview') {
        setMarkdown(markdownFor(next))
        setTrailingMarkdown(trailingMarkdownFor(next))
      }
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Upload non riuscito'))
      await rollbackFlowInsertion(flowAnchor)
    } finally {
      setBusy(false)
      pendingBlockInsertionRef.current = undefined
      pendingFlowInsertionRef.current = undefined
      if (fileInputRef.current) fileInputRef.current.value = ''
    }
  }

  const resizeImage = async (file: InfoFileBlock, width: number | undefined) => {
    if (!current) return
    const normalizedWidth = width === undefined ? undefined : Math.max(160, Math.round(width))
    const latest = documentsRef.current.find((document) => document.wire.id === current.wire.id)
      ?? current
    const base = presentation === 'overview'
      ? latest.document
      : withTrailingMarkdown(withMarkdown(latest.document, markdown), trailingMarkdown)
    const content: InfoDocumentContent = {
      ...base,
      ...(presentation === 'overview' ? { title } : {}),
      blocks: base.blocks.map((block) =>
        block.type === 'file' && block.id === file.id
          ? normalizedWidth === undefined
            ? (() => {
                const { display_width: _displayWidth, ...responsiveBlock } = block
                return responsiveBlock
              })()
            : { ...block, display_width: normalizedWidth }
          : block,
      ),
    }
    await enqueueDocumentUpdate(
      current.wire.id,
      content,
      'Salvataggio dimensione immagine non riuscito',
    )
  }

  const removeFile = async (file: InfoFileBlock) => {
    if (!current) return
    const latest = documentsRef.current.find((document) => document.wire.id === current.wire.id)
      ?? current
    const base = presentation === 'overview'
      ? latest.document
      : withTrailingMarkdown(withMarkdown(latest.document, markdown), trailingMarkdown)
    const content: InfoDocumentContent = {
      ...base,
      ...(presentation === 'overview' ? { title } : {}),
      blocks: presentation === 'overview'
        ? removeFlowBlock(base.blocks, file.id)
        : base.blocks.filter((block) => block.type !== 'file' || block.id !== file.id),
    }
    setBusy(true)
    setError(undefined)
    try {
      const next = await enqueueDocumentUpdate(
        current.wire.id,
        content,
        'Eliminazione file non riuscita',
      )
      if (next && presentation !== 'overview') setTrailingMarkdown(trailingMarkdownFor(next))
    } finally {
      setBusy(false)
    }
  }

  const moveContentBlockToTextLine = async (
    sourceId: Uuid,
    target: 'main' | 'trailing',
    cutIndex: number,
  ) => {
    if (!current || busy) return
    const editor = target === 'main'
      ? overviewEditorRef.current
      : trailingEditorRef.current
    if (!editor) return
    const elements = Array.from(editor.children) as HTMLElement[]
    const safeCutIndex = Math.max(0, Math.min(elements.length, cutIndex))
    const before = overviewElementsToMarkdown(elements.slice(0, safeCutIndex))
    const after = overviewElementsToMarkdown(elements.slice(safeCutIndex))
    const base = withTrailingMarkdown(
      withMarkdown(current.document, markdown),
      trailingMarkdown,
    )
    const sourceIndex = base.blocks.findIndex((block) => block.id === sourceId)
    if (sourceIndex < 0) return
    const [source] = base.blocks.splice(sourceIndex, 1)
    if (!source || source.type === 'text') return
    const textIndexes = base.blocks
      .map((block, index) => block.type === 'text' ? index : -1)
      .filter((index) => index >= 0)
    const textIndex = target === 'main' ? textIndexes[0] : textIndexes.at(-1)
    if (textIndex === undefined) return
    const textBlock = base.blocks[textIndex]
    if (textBlock.type !== 'text') return
    const blocks = [
      ...base.blocks.slice(0, textIndex),
      { ...textBlock, markdown: before },
      source,
      { id: crypto.randomUUID(), type: 'text' as const, markdown: after },
      ...base.blocks.slice(textIndex + 1),
    ]
    const content: InfoDocumentContent = {
      ...base,
      ...(presentation === 'overview' ? { title } : {}),
      blocks,
    }
    const previous = current
    setBusy(true)
    replaceDocument({ ...current, document: content })
    try {
      const next = await onUpdateDocument(current, content)
      replaceDocument(next)
      setMarkdown(markdownFor(next))
      setTrailingMarkdown(trailingMarkdownFor(next))
    } catch (reason) {
      replaceDocument(previous)
      setError(infoErrorMessage(reason, 'Spostamento blocco non riuscito'))
    } finally {
      setBusy(false)
    }
  }

  const textDropLineForEvent = (
    event: DragEvent<HTMLDivElement>,
    target: 'main' | 'trailing',
  ) => {
    const block = directEditorBlock(event.currentTarget, event.target)
    if (!block) return
    const elements = Array.from(event.currentTarget.children) as HTMLElement[]
    const index = elements.indexOf(block)
    if (index < 0) return
    const blockBounds = block.getBoundingClientRect()
    const canvasBounds = event.currentTarget.parentElement?.getBoundingClientRect()
    if (!canvasBounds) return
    const position = event.clientY < blockBounds.top + blockBounds.height / 2
      ? 'before'
      : 'after'
    const edge = position === 'before' ? blockBounds.top : blockBounds.bottom
    return {
      target,
      index,
      position,
      top: edge - canvasBounds.top + (position === 'before' ? -3 : 3),
    } as const
  }

  const stopDragAutoScroll = () => {
    dragAutoScrollVelocityRef.current = 0
    dragAutoScrollHostRef.current = undefined
    if (dragAutoScrollFrameRef.current !== undefined) {
      window.cancelAnimationFrame(dragAutoScrollFrameRef.current)
      dragAutoScrollFrameRef.current = undefined
    }
  }

  const updateDragAutoScroll = (event: DragEvent<HTMLElement>) => {
    if (!draggedBlockIdRef.current) return
    const host = event.currentTarget.closest<HTMLElement>('.board-overview-scroll')
    if (!host) return
    const bounds = host.getBoundingClientRect()
    const threshold = Math.min(120, Math.max(72, bounds.height * 0.16))
    let velocity = 0
    if (event.clientY < bounds.top + threshold) {
      const strength = Math.min(1, (bounds.top + threshold - event.clientY) / threshold)
      velocity = -Math.max(3, Math.round(22 * strength))
    } else if (event.clientY > bounds.bottom - threshold) {
      const strength = Math.min(1, (event.clientY - (bounds.bottom - threshold)) / threshold)
      velocity = Math.max(3, Math.round(22 * strength))
    }
    dragAutoScrollHostRef.current = host
    dragAutoScrollVelocityRef.current = velocity
    if (velocity === 0 || dragAutoScrollFrameRef.current !== undefined) return
    const scroll = () => {
      const scrollHost = dragAutoScrollHostRef.current
      const speed = dragAutoScrollVelocityRef.current
      if (!scrollHost || speed === 0) {
        dragAutoScrollFrameRef.current = undefined
        return
      }
      scrollHost.scrollTop += speed
      dragAutoScrollFrameRef.current = window.requestAnimationFrame(scroll)
    }
    dragAutoScrollFrameRef.current = window.requestAnimationFrame(scroll)
  }

  const updateTextDropLine = (
    event: DragEvent<HTMLDivElement>,
    target: 'main' | 'trailing',
  ) => {
    updateDragAutoScroll(event)
    if (!draggedBlockIdRef.current) return
    const drop = textDropLineForEvent(event, target)
    if (!drop) return
    event.preventDefault()
    textDropLineRef.current = drop
    setTextDropLine(drop)
  }

  const finishTextDrop = (
    event: DragEvent<HTMLDivElement>,
    target: 'main' | 'trailing',
  ) => {
    event.preventDefault()
    const drop = textDropLineForEvent(event, target) ?? textDropLineRef.current
    const sourceId = draggedBlockIdRef.current || event.dataTransfer.getData('text/plain')
    if (!drop || drop.target !== target || !sourceId) return
    const cutIndex = drop.index + (drop.position === 'after' ? 1 : 0)
    void moveContentBlockToTextLine(sourceId, target, cutIndex)
    draggedBlockIdRef.current = undefined
    textDropLineRef.current = undefined
    stopDragAutoScroll()
    setTextDropLine(undefined)
  }

  const addDocument = async () => {
    if (!current || !newDocumentTitle.trim()) return
    const documentId = current.wire.id
    const flowAnchor = pendingFlowInsertionRef.current
    setBusy(true)
    setError(undefined)
    try {
      const child = await onCreateDocument(
        container,
        current.wire.id,
        emptyDocument(newDocumentTitle.trim()),
      )
      const pending = pendingBlockInsertionRef.current
      const latest = documentsRef.current.find((document) => document.wire.id === documentId)
        ?? current
      const parentContent = presentation === 'overview'
        ? latest.document
        : withTrailingMarkdown(withMarkdown(latest.document, markdown), trailingMarkdown)
      const documentBlock: InfoDocumentBlock = {
        id: crypto.randomUUID(),
        type: 'document',
        document_id: child.wire.id,
        title: newDocumentTitle.trim(),
      }
      let nextContent: InfoDocumentContent
      if (presentation === 'overview' && flowAnchor?.documentId === documentId) {
        const afterIndex = flowAnchor.afterTextBlockId
          ? parentContent.blocks.findIndex((value) => value.id === flowAnchor.afterTextBlockId)
          : -1
        nextContent = afterIndex >= 0
          ? {
              ...parentContent,
              blocks: [
                ...parentContent.blocks.slice(0, afterIndex),
                documentBlock,
                ...parentContent.blocks.slice(afterIndex),
              ],
            }
          : {
              ...parentContent,
              blocks: splitTextBlockWith(
                parentContent.blocks,
                flowAnchor.textBlockId,
                flowAnchor.beforeMarkdown,
                documentBlock,
                flowAnchor.afterMarkdown,
              ),
            }
      } else {
        nextContent = insertBlockAtPendingPosition(parentContent, documentBlock, pending)
      }
      authoritativeDocumentsRef.current.set(child.wire.id, child)
      documentsRef.current = [...documentsRef.current, child]
      setDocuments(documentsRef.current)
      const parent = await enqueueDocumentUpdate(documentId, nextContent, 'Collegamento pagina non riuscito')
      if (parent && presentation !== 'overview') {
        setMarkdown(markdownFor(parent))
        setTrailingMarkdown(trailingMarkdownFor(parent))
      }
      setNewDocumentTitle('')
      setShowDocumentForm(false)
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Creazione non riuscita'))
    } finally {
      setBusy(false)
      pendingBlockInsertionRef.current = undefined
      pendingFlowInsertionRef.current = undefined
    }
  }

  const renameDocumentBlock = async (
    block: Extract<InfoDocumentBlock, { type: 'document' }>,
  ) => {
    const nextTitle = (
      editingDocumentBlockId === block.id ? documentTitleDraft : block.title
    ).trim()
    if (!current || !nextTitle) return
    const latest = documentsRef.current.find((document) => document.wire.id === current.wire.id)
      ?? current
    const base = presentation === 'overview'
      ? latest.document
      : withTrailingMarkdown(withMarkdown(latest.document, markdown), trailingMarkdown)
    const parentContent: InfoDocumentContent = {
      ...base,
      ...(presentation === 'overview' ? { title } : {}),
      blocks: base.blocks.map((value) =>
        value.type === 'document' && value.id === block.id
          ? { ...value, title: nextTitle }
          : value,
      ),
    }
    const child = documentsRef.current.find((document) => document.wire.id === block.document_id)
    setBusy(true)
    setError(undefined)
    try {
      await enqueueDocumentUpdate(current.wire.id, parentContent, 'Rinomina pagina non riuscita')
      if (child) {
        await enqueueDocumentUpdate(
          child.wire.id,
          { ...child.document, title: nextTitle },
          'Rinomina contenuto pagina non riuscita',
        )
      }
      setEditingDocumentBlockId(undefined)
      setDocumentTitleDraft('')
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Rinomina pagina non riuscita'))
    } finally {
      setBusy(false)
    }
  }

  const removeDocumentBlock = async (
    block: Extract<InfoDocumentBlock, { type: 'document' }>,
  ) => {
    if (!current) return
    const latest = documentsRef.current.find((document) => document.wire.id === current.wire.id)
      ?? current
    const base = presentation === 'overview'
      ? latest.document
      : withTrailingMarkdown(withMarkdown(latest.document, markdown), trailingMarkdown)
    const content: InfoDocumentContent = {
      ...base,
      ...(presentation === 'overview' ? { title } : {}),
      blocks: presentation === 'overview'
        ? removeFlowBlock(base.blocks, block.id)
        : base.blocks.filter((value) => value.id !== block.id),
    }
    setBusy(true)
    setError(undefined)
    try {
      const next = await enqueueDocumentUpdate(
        current.wire.id,
        content,
        'Eliminazione pagina non riuscita',
      )
      if (!next) return
      const nextDocuments = documentsRef.current.filter(
        (document) => document.wire.id !== block.document_id,
      )
      documentsRef.current = nextDocuments
      setDocuments(nextDocuments)
    } finally {
      setBusy(false)
    }
  }

  if (loading) return <p className="tasklist-history-empty">Caricamento info…</p>
  if (!current) {
    return (
      <div className="tasklist-info-recovery">
        <p className="tasklist-history-empty" role="alert">
          {error ?? 'Documento info non disponibile.'}
        </p>
        <button type="button" onClick={() => setReloadToken((value) => value + 1)}>
          Riprova
        </button>
      </div>
    )
  }

  const contentBlocks = current.document.blocks.filter(
    (block) => block.type === 'file' || block.type === 'document',
  )
  const overviewCheckpoints = [
    { id: 'title', label: title || 'Inizio pagina', target: 'title' as const, index: 0 },
    ...overviewCollapseCheckpoints,
  ]

  const scrollToOverviewCheckpoint = (
    checkpoint: (typeof overviewCheckpoints)[number],
  ) => {
    const element = checkpoint.target === 'title'
      ? titleRef.current
      : panelRef.current?.querySelectorAll<HTMLElement>(
          '.tasklist-info-flow [data-md-kind="collapse"]',
        )[checkpoint.index]
    if (!element) return
    setActiveOverviewCheckpoint(checkpoint.id)
    element.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  const renderBlockMenu = (target: 'main' | 'trailing') => (
    <div className="tasklist-info-block-menu" role="menu" aria-label="Tipi di blocco">
      {(['h1', 'h2', 'h3'] as const).map((kind, index) => (
        <button
          key={kind}
          type="button"
          role="menuitem"
          onPointerDown={(event) => {
            if (event.button !== 0) return
            event.preventDefault()
            insertBlock(kind, target)
          }}
          onClick={(event) => {
            if (event.detail === 0) insertBlock(kind, target)
          }}
        >
          <span className={`tasklist-info-block-icon tasklist-info-block-icon--${kind}`}>{kind.toUpperCase()}</span>
          <span>{['Titolo grande', 'Titolo medio', 'Titolo piccolo'][index]}</span>
        </button>
      ))}
      <button type="button" role="menuitem" onPointerDown={(event) => {
        if (event.button !== 0) return
        event.preventDefault()
        insertBlock('bullet', target)
      }} onClick={(event) => {
        if (event.detail === 0) insertBlock('bullet', target)
      }}>
        <span className="tasklist-info-block-icon"><ListIcon aria-hidden /></span>
        <span>Elenco puntato</span>
      </button>
      <button type="button" role="menuitem" onPointerDown={(event) => {
        if (event.button !== 0) return
        event.preventDefault()
        insertBlock('collapse', target)
      }} onClick={(event) => {
        if (event.detail === 0) insertBlock('collapse', target)
      }}>
        <span className="tasklist-info-block-icon tasklist-info-block-icon--collapse" aria-hidden>−</span>
        <span>Collapse</span>
      </button>
      <button type="button" role="menuitem" onPointerDown={(event) => {
        if (event.button !== 0) return
        event.preventDefault()
        openFilePicker('image/*', target)
      }} onClick={(event) => {
        if (event.detail === 0) openFilePicker('image/*', target)
      }}>
        <span className="tasklist-info-block-icon"><ImageIcon aria-hidden /></span>
        <span>Immagine</span>
      </button>
      <button type="button" role="menuitem" onPointerDown={(event) => {
        if (event.button !== 0) return
        event.preventDefault()
        openFilePicker('*/*', target)
      }} onClick={(event) => {
        if (event.detail === 0) openFilePicker('*/*', target)
      }}>
        <span className="tasklist-info-block-icon"><PaperclipIcon aria-hidden /></span>
        <span>File</span>
      </button>
      <button type="button" role="menuitem" onPointerDown={(event) => {
        if (event.button !== 0) return
        event.preventDefault()
        pendingBlockInsertionRef.current = capturePendingInsertion(target)
        setBlockMenuOpen(false)
        setTrailingBlockMenuOpen(false)
        setShowDocumentForm(true)
      }} onClick={(event) => {
        if (event.detail !== 0) return
        pendingBlockInsertionRef.current = capturePendingInsertion(target)
        setBlockMenuOpen(false)
        setTrailingBlockMenuOpen(false)
        setShowDocumentForm(true)
      }}>
        <span className="tasklist-info-block-icon"><FolderIcon aria-hidden /></span>
        <span>Pagina</span>
      </button>
    </div>
  )

  // The default Markdown editor still owns these compatibility helpers. The
  // Overview renderer no longer invokes them, but keeping them together here
  // avoids changing the non-Overview editor in the same migration.
  void [
    blockInserterTop,
    trailingBlockInserterTop,
    textDropLine,
    toggleOverviewCollapse,
    renderTextFormatToolbar,
    handleOverviewEditorKeyDown,
    updateTextDropLine,
    finishTextDrop,
    renderBlockMenu,
  ]

  return (
    <div
      ref={panelRef}
      className={presentation === 'overview' ? 'tasklist-info-panel tasklist-info-panel--overview' : 'tasklist-info-panel'}
      onDragOverCapture={updateDragAutoScroll}
    >
      {presentation === 'overview' && overviewCheckpointHost && createPortal(
        <nav className="tasklist-info-overview-checkpoints" aria-label="Checkpoint documento">
          <div>
            {overviewCheckpoints.map((checkpoint) => (
              <button
                key={checkpoint.id}
                type="button"
                className={activeOverviewCheckpoint === checkpoint.id ? 'is-active' : ''}
                data-label={checkpoint.label}
                aria-label={`Vai a ${checkpoint.label}`}
                title={checkpoint.label}
                onClick={() => scrollToOverviewCheckpoint(checkpoint)}
              />
            ))}
          </div>
        </nav>,
        overviewCheckpointHost,
      )}
      {breadcrumbs.length > 1 && (
        <nav
          className={`tasklist-info-breadcrumbs${presentation === 'overview' ? ' tasklist-info-breadcrumbs--overview' : ''}`}
          aria-label="Percorso documenti"
        >
          {breadcrumbs.map((document, index) => (
            <button
              key={document.wire.id}
              type="button"
              disabled={document.wire.id === current.wire.id}
              onClick={() => selectDocument(document)}
            >
              {index === 0
                ? document.document.title || overviewTitle || 'Overview'
                : document.document.title || 'Pagina'}
            </button>
          ))}
        </nav>
      )}
      {presentation === 'overview' && showOverviewTitle && (
        <textarea
          ref={titleRef}
          className="tasklist-info-overview-title"
          value={title}
          rows={1}
          placeholder="Titolo senza nome"
          aria-label="Titolo Overview"
          onChange={(event) => {
            setTitle(event.target.value)
            setDirty(true)
          }}
          onBlur={() => {
            if (dirty) void save()
          }}
        />
      )}

      {presentation !== 'overview' && current.document.title && <h1>{current.document.title}</h1>}

      {presentation !== 'overview' && <div className="tasklist-info-toolbar" aria-label="Azioni documento info">
        <button type="button" onClick={() => setEditing(true)} disabled={busy}>
          <PencilIcon aria-hidden />
          Testo
        </button>
        <button
          type="button"
          onClick={() => {
            pendingBlockInsertionRef.current = undefined
            fileInputRef.current?.click()
          }}
          disabled={busy}
        >
          <PaperclipIcon aria-hidden />
          File o immagine
        </button>
        <button
          type="button"
          onClick={() => {
            pendingBlockInsertionRef.current = undefined
            setShowDocumentForm((visible) => !visible)
          }}
          disabled={busy}
        >
          <FolderIcon aria-hidden />
          Pagina
        </button>
      </div>}
      <input
        ref={fileInputRef}
        type="file"
        hidden
        onChange={(event) => {
          const file = event.target.files?.[0]
          if (file) {
            void addFile(file)
            return
          }
          void rollbackFlowInsertion().finally(() => {
            pendingFlowInsertionRef.current = undefined
          })
        }}
      />

      {showDocumentForm && (
        <form
          className="tasklist-info-document-form"
          onSubmit={(event) => {
            event.preventDefault()
            void addDocument()
          }}
        >
          <input
            autoFocus
            required
            value={newDocumentTitle}
            placeholder="Nome pagina"
            aria-label="Nome sottopagina"
            onChange={(event) => setNewDocumentTitle(event.target.value)}
          />
          <button type="submit" disabled={busy}>Crea</button>
          <button type="button" onClick={() => {
            pendingBlockInsertionRef.current = undefined
            void rollbackFlowInsertion().finally(() => {
              pendingFlowInsertionRef.current = undefined
            })
            setShowDocumentForm(false)
          }}>
            Annulla
          </button>
        </form>
      )}

      {editing ? (
        <div className="tasklist-info-editor">
          {presentation === 'overview' ? (
            <OverviewFlowEditor
              document={current}
              documents={documents}
              busy={busy}
              onDraft={(content) => {
                const draft = documentsRef.current.find(
                  (document) => document.wire.id === current.wire.id,
                ) ?? current
                replaceDocument({ ...draft, document: content })
                setDirty(true)
                setOverviewDraftRevision((revision) => revision + 1)
              }}
              onCommit={async (content, failureMessage) => {
                setDirty(true)
                const next = await enqueueDocumentUpdate(
                  current.wire.id,
                  { ...content, title },
                  failureMessage,
                )
                const visible = documentsRef.current.find(
                  (document) => document.wire.id === current.wire.id,
                )
                if (next && (visible?.document === content || visible?.document === next.document)) {
                  setDirty(false)
                }
                return next
              }}
              onRequestFile={(anchor, accept) => {
                pendingFlowInsertionRef.current = prepareFlowInsertion(anchor)
                const input = fileInputRef.current
                if (!input) return
                input.accept = accept
                input.click()
              }}
              onRequestPage={(anchor) => {
                pendingFlowInsertionRef.current = prepareFlowInsertion(anchor)
                setShowDocumentForm(true)
              }}
              onReadFile={onReadFile}
              onDownloadFile={onDownloadFile}
              onResizeFile={(file, width) => { void resizeImage(file, width) }}
              onRemoveFile={(file) => { void removeFile(file) }}
              onSelectDocument={selectDocument}
              onRenameDocument={(block) => { void renameDocumentBlock(block) }}
              onRemoveDocument={(block) => { void removeDocumentBlock(block) }}
            />
          ) : <>
            <textarea
              value={markdown}
              placeholder="Scrivi in Markdown…"
              aria-label="Testo info in Markdown"
              onChange={(event) => setMarkdown(event.target.value)}
            />
            <div className="tasklist-info-editor-actions">
            <button type="button" onClick={() => void save()} disabled={busy}>
              {busy ? 'Salvataggio…' : 'Salva'}
            </button>
            <button
              type="button"
              onClick={() => {
                setMarkdown(markdownFor(current))
                setEditing(false)
              }}
              disabled={busy}
            >
              Annulla
            </button>
            </div>
          </>}
        </div>
      ) : (
        <div className="tasklist-info-preview">
          <InfoMarkdown>{markdown}</InfoMarkdown>
        </div>
      )}

      {presentation !== 'overview' && contentBlocks.length > 0 && (
        <div className="tasklist-info-files tasklist-info-content-blocks">
          {contentBlocks.map((block) => {
            return (
              <div
                key={block.id}
                className="tasklist-info-content-block"
              >
                {block.type === 'file' ? (
                  block.content_type.startsWith('image/') ? (
                    <InfoImageBlock
                      document={current}
                      file={block}
                      onRead={onReadFile}
                      onDownload={onDownloadFile}
                      onResize={resizeImage}
                      onRemove={(file) => void removeFile(file)}
                      onDragStart={() => {
                        draggedBlockIdRef.current = block.id
                      }}
                      onDragEnd={() => {
                        draggedBlockIdRef.current = undefined
                        textDropLineRef.current = undefined
                        stopDragAutoScroll()
                        setTextDropLine(undefined)
                      }}
                    />
                  ) : (
                    <div className="tasklist-info-file">
                      <button
                        type="button"
                        draggable
                        className="tasklist-info-block-drag-handle"
                        aria-label={`Sposta ${block.file_name}`}
                        title="Trascina per spostare"
                        onDragStart={(event) => {
                          event.dataTransfer.effectAllowed = 'move'
                          event.dataTransfer.setData('text/plain', block.id)
                          draggedBlockIdRef.current = block.id
                        }}
                        onDragEnd={() => {
                          draggedBlockIdRef.current = undefined
                          textDropLineRef.current = undefined
                          stopDragAutoScroll()
                          setTextDropLine(undefined)
                        }}
                      >
                        <GripIcon aria-hidden />
                      </button>
                      <FileIcon aria-hidden />
                      <button
                        type="button"
                        className="tasklist-info-attachment-link"
                        onClick={() => void onDownloadFile(current, block)}
                      >
                        {block.file_name}
                      </button>
                      <button
                        type="button"
                        className="tasklist-info-block-remove tasklist-info-file-remove"
                        aria-label={`Elimina ${block.file_name}`}
                        title="Elimina file"
                        onClick={() => void removeFile(block)}
                      >
                        <XIcon aria-hidden />
                      </button>
                    </div>
                  )
                ) : (
                  <div className="tasklist-info-document-row">
                    {editingDocumentBlockId === block.id ? (
                      <form
                        className="tasklist-info-document-rename"
                        onSubmit={(event) => {
                          event.preventDefault()
                          void renameDocumentBlock(block)
                        }}
                      >
                        <FolderIcon aria-hidden />
                        <input
                          autoFocus
                          value={documentTitleDraft}
                          aria-label="Nome pagina"
                          onChange={(event) => setDocumentTitleDraft(event.target.value)}
                          onBlur={() => void renameDocumentBlock(block)}
                          onKeyDown={(event) => {
                            if (event.key !== 'Escape') return
                            setEditingDocumentBlockId(undefined)
                            setDocumentTitleDraft('')
                          }}
                        />
                      </form>
                    ) : (
                      <button
                        type="button"
                        className="tasklist-info-document-link"
                        onClick={() => {
                          const child = documents.find(
                            (document) => document.wire.id === block.document_id,
                          )
                          if (child) selectDocument(child)
                        }}
                      >
                        <FolderIcon aria-hidden />
                        <span>{block.title}</span>
                      </button>
                    )}
                    <div className="tasklist-info-document-actions">
                      <button
                        type="button"
                        className="tasklist-info-document-action"
                        aria-label={`Rinomina ${block.title}`}
                        title="Rinomina pagina"
                        onClick={() => {
                          setEditingDocumentBlockId(block.id)
                          setDocumentTitleDraft(block.title)
                        }}
                      >
                        <PencilIcon aria-hidden />
                      </button>
                      <button
                        type="button"
                        className="tasklist-info-block-remove tasklist-info-document-remove"
                        aria-label={`Elimina ${block.title}`}
                        title="Elimina pagina"
                        onClick={() => void removeDocumentBlock(block)}
                      >
                        <XIcon aria-hidden />
                      </button>
                    </div>
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}


      {error && <p className="tasklist-info-error" role="alert">{error}</p>}
    </div>
  )
}

type TaskListInfoPanelProps = Omit<
  InfoDocumentPanelProps<TaskListItem>,
  'container'
> & { list: TaskListItem }

export const TaskListInfoPanel = ({
  list,
  ...props
}: TaskListInfoPanelProps) => (
  <InfoDocumentPanel container={list} {...props} />
)
