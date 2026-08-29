import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react'
import type { Uuid } from '../api/contracts'
import type {
  DecryptedInfoDocument,
  InfoDocumentContent,
  InfoFileBlock,
} from '../domain/models'
import type { TaskListItem } from '../store/app-store'
import {
  FileIcon,
  FolderIcon,
  ImageIcon,
  ListIcon,
  PaperclipIcon,
  PencilIcon,
} from './icons'
import { InfoMarkdown } from './InfoMarkdown'

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

const markdownToOverviewHtml = (markdown: string): string =>
  markdown.split('\n').map((line) => {
    const escaped = escapeHtml(line)
    if (line.startsWith('### ')) return `<h3>${escapeHtml(line.slice(4)) || '<br>'}</h3>`
    if (line.startsWith('## ')) return `<h2>${escapeHtml(line.slice(3)) || '<br>'}</h2>`
    if (line.startsWith('# ')) return `<h1>${escapeHtml(line.slice(2)) || '<br>'}</h1>`
    if (line.startsWith('- ')) return `<div class="tasklist-info-overview-bullet" data-md-kind="bullet">${escapeHtml(line.slice(2)) || '<br>'}</div>`
    return `<p>${escaped || '<br>'}</p>`
  }).join('')

const overviewEditorToMarkdown = (editor: HTMLElement): string =>
  (Array.from(editor.children) as HTMLElement[]).map((element) => {
    const text = element.querySelector<HTMLElement>('[data-task-text]')?.textContent?.trimEnd()
      ?? element.textContent?.trimEnd()
      ?? ''
    if (element.tagName === 'H1') return `# ${text}`
    if (element.tagName === 'H2') return `## ${text}`
    if (element.tagName === 'H3') return `### ${text}`
    if (element.dataset.mdKind === 'bullet') return `- ${text}`
    return text
  }).join('\n')

const ensureOverviewTrailingPromptLine = (editor: HTMLElement): void => {
  const last = editor.lastElementChild as HTMLElement | null
  const hasEmptyTrailingParagraph =
    last?.tagName === 'P' && (last.textContent ?? '').trim() === ''
  if (hasEmptyTrailingParagraph) return
  const promptLine = window.document.createElement('p')
  promptLine.innerHTML = '<br>'
  editor.append(promptLine)
}

const InfoImageBlock = ({
  document,
  file,
  onRead,
  onDownload,
  onResize,
}: {
  document: DecryptedInfoDocument
  file: InfoFileBlock
  onRead(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<Blob>
  onDownload(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<void>
  onResize(file: InfoFileBlock, width: number): Promise<void>
}) => {
  const [source, setSource] = useState<string>()
  const [imageWidth, setImageWidth] = useState<number | undefined>(
    file.display_width,
  )
  const resizeStartRef = useRef<{ x: number; width: number } | null>(null)
  const currentWidthRef = useRef<number | undefined>(file.display_width)

  useEffect(() => {
    setImageWidth(file.display_width)
    currentWidthRef.current = file.display_width
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

  return (
    <figure
      className="tasklist-info-image"
    >
      <div
        className="tasklist-info-image-frame"
        style={{ width: imageWidth ? `${imageWidth}px` : '100%' }}
      >
        {source ? (
          <img src={source} alt={file.file_name} />
        ) : (
          <span className="tasklist-info-image-placeholder">Immagine cifrata</span>
        )}
        <span
          className="tasklist-info-image-resize-handle"
          role="separator"
          aria-label={`Ridimensiona ${file.file_name}`}
          aria-orientation="vertical"
          tabIndex={0}
          onPointerDown={(event) => {
            const frame = event.currentTarget.parentElement
            if (!frame) return
            resizeStartRef.current = { x: event.clientX, width: frame.getBoundingClientRect().width }
            event.currentTarget.setPointerCapture(event.pointerId)
            event.preventDefault()
          }}
          onPointerMove={(event) => {
            const start = resizeStartRef.current
            const parentWidth = event.currentTarget.parentElement?.parentElement?.getBoundingClientRect().width
            if (!start || !parentWidth) return
            const nextWidth = Math.round(
              Math.min(parentWidth, Math.max(160, start.width + event.clientX - start.x)),
            )
            currentWidthRef.current = nextWidth
            setImageWidth(nextWidth)
          }}
          onPointerUp={(event) => {
            resizeStartRef.current = null
            event.currentTarget.releasePointerCapture(event.pointerId)
            if (currentWidthRef.current) {
              void onResize(file, currentWidthRef.current)
            }
          }}
          onKeyDown={(event) => {
            if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
            const frame = event.currentTarget.parentElement
            const parentWidth = frame?.parentElement?.getBoundingClientRect().width
            if (!frame || !parentWidth) return
            const currentWidth = frame.getBoundingClientRect().width
            const nextWidth = Math.round(
              Math.min(parentWidth, Math.max(160, currentWidth + (event.key === 'ArrowRight' ? 24 : -24))),
            )
            currentWidthRef.current = nextWidth
            setImageWidth(nextWidth)
            void onResize(file, nextWidth)
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
    </figure>
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
  const titleRef = useRef<HTMLTextAreaElement>(null)
  const activeEmptyBlockRef = useRef<HTMLElement | null>(null)
  const slashMenuTargetRef = useRef<'main' | 'trailing' | null>(null)
  const mainInserterTargetRef = useRef<HTMLElement | null>(null)
  const trailingInserterTargetRef = useRef<HTMLElement | null>(null)
  const loadRef = useRef(onLoad)
  const createRef = useRef(onCreateDocument)
  const containerRef = useRef(container)
  loadRef.current = onLoad
  createRef.current = onCreateDocument
  containerRef.current = container
  const [documents, setDocuments] = useState<DecryptedInfoDocument[]>([])
  const [currentId, setCurrentId] = useState<Uuid>()
  const [markdown, setMarkdown] = useState('')
  const [trailingMarkdown, setTrailingMarkdown] = useState('')
  const [editing, setEditing] = useState(false)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const [showDocumentForm, setShowDocumentForm] = useState(false)
  const [newDocumentTitle, setNewDocumentTitle] = useState('')
  const [reloadToken, setReloadToken] = useState(0)
  const [dirty, setDirty] = useState(false)
  const [title, setTitle] = useState('')
  const [blockMenuOpen, setBlockMenuOpen] = useState(false)
  const [trailingBlockMenuOpen, setTrailingBlockMenuOpen] = useState(false)
  const [blockInserterTop, setBlockInserterTop] = useState(0)
  const [trailingBlockInserterTop, setTrailingBlockInserterTop] = useState(0)
  const saveRef = useRef<(content?: InfoDocumentContent) => Promise<DecryptedInfoDocument | undefined>>(
    async () => undefined,
  )

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
        setDocuments(loaded)
        setCurrentId(root.wire.id)
        setMarkdown(markdownFor(root))
        setTrailingMarkdown(trailingMarkdownFor(root))
        setEditing(presentation === 'overview')
        setTitle(root.document.title || overviewTitle || '')
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
    setDocuments((values) =>
      values.map((value) => (value.wire.id === next.wire.id ? next : value)),
    )
  }

  const save = async (content?: InfoDocumentContent) => {
    if (!current) return undefined
    setBusy(true)
    setError(undefined)
    try {
      const next = await onUpdateDocument(
        current,
        content ?? {
          ...withMarkdown(current.document, markdown),
          ...(presentation === 'overview' ? { title } : {}),
        },
      )
      replaceDocument(next)
      setMarkdown(markdownFor(next))
      setTrailingMarkdown(trailingMarkdownFor(next))
      setEditing(presentation === 'overview')
      setDirty(false)
      return next
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Salvataggio non riuscito'))
      return undefined
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
      busy ||
      window.document.activeElement === overviewEditorRef.current ||
      window.document.activeElement === trailingEditorRef.current
    ) return
    const timeout = window.setTimeout(() => {
      void saveRef.current()
    }, 900)
    return () => window.clearTimeout(timeout)
  }, [busy, current, dirty, markdown, presentation, title, trailingMarkdown])

  useEffect(() => {
    const editor = overviewEditorRef.current
    if (
      !editor ||
      presentation !== 'overview' ||
      window.document.activeElement === editor
    ) return
    editor.innerHTML = markdownToOverviewHtml(markdown)
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
      const caretRect = selectionRange
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
    activeEmptyBlockRef.current = block
    slashMenuTargetRef.current = target
    if (target === 'main') {
      setBlockMenuOpen(true)
    } else {
      setTrailingBlockMenuOpen(true)
    }
    positionBlockInserter(target, block)
  }

  const insertBlock = (
    kind: 'h1' | 'h2' | 'h3' | 'bullet',
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
    }
    const sourceBlock = (target === 'main'
      ? mainInserterTargetRef.current
      : trailingInserterTargetRef.current) ?? activeEmptyBlockRef.current
    const rawSourceText = sourceBlock?.querySelector<HTMLElement>('[data-task-text]')?.textContent
      ?? sourceBlock?.textContent
      ?? ''
    const sourceText = rawSourceText.trim() === '/' ? '' : rawSourceText
    next.textContent = sourceText
    if (sourceText.trim() === '') next.innerHTML = '<br>'
    if (sourceBlock?.parentElement === editor) {
      sourceBlock.replaceWith(next)
    } else {
      editor.append(next)
    }
    if (!next.nextElementSibling) {
      const promptLine = window.document.createElement('p')
      promptLine.innerHTML = '<br>'
      next.after(promptLine)
    }
    activeEmptyBlockRef.current = null
    slashMenuTargetRef.current = null
    editor.focus()
    const selection = window.getSelection()
    const range = window.document.createRange()
    range.selectNodeContents(next)
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

  const openFilePicker = (accept: string) => {
    const input = fileInputRef.current
    if (!input) return
    input.accept = accept
    input.click()
    slashMenuTargetRef.current = null
    setBlockMenuOpen(false)
    setTrailingBlockMenuOpen(false)
  }

  const addFile = async (file: File) => {
    if (!current) return
    setBusy(true)
    setError(undefined)
    try {
      const block = await onUploadFile(current, file)
      let nextContent = withTrailingMarkdown(
        withMarkdown(current.document, markdown),
        trailingMarkdown,
      )
      let trailingIndex = -1
      nextContent.blocks.forEach((value, index) => {
        if (value.type === 'text' && index > 0) trailingIndex = index
      })
      const blocks = trailingIndex >= 0
        ? [
            ...nextContent.blocks.slice(0, trailingIndex),
            block,
            ...nextContent.blocks.slice(trailingIndex),
          ]
        : [
            ...nextContent.blocks,
            block,
            { id: crypto.randomUUID(), type: 'text' as const, markdown: '' },
          ]
      nextContent = { ...nextContent, blocks }
      const next = await onUpdateDocument(current, {
        ...nextContent,
        blocks,
      })
      replaceDocument(next)
      setTrailingMarkdown(trailingMarkdownFor(next))
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Upload non riuscito'))
    } finally {
      setBusy(false)
      if (fileInputRef.current) fileInputRef.current.value = ''
    }
  }

  const resizeImage = async (file: InfoFileBlock, width: number) => {
    if (!current) return
    const normalizedWidth = Math.max(160, Math.round(width))
    const base = withTrailingMarkdown(
      withMarkdown(current.document, markdown),
      trailingMarkdown,
    )
    const content: InfoDocumentContent = {
      ...base,
      ...(presentation === 'overview' ? { title } : {}),
      blocks: base.blocks.map((block) =>
        block.type === 'file' && block.id === file.id
          ? { ...block, display_width: normalizedWidth }
          : block,
      ),
    }
    const previous = current
    replaceDocument({ ...current, document: content })
    try {
      replaceDocument(await onUpdateDocument(current, content))
    } catch (reason) {
      replaceDocument(previous)
      setError(infoErrorMessage(reason, 'Salvataggio dimensione immagine non riuscito'))
    }
  }

  const addDocument = async () => {
    if (!current || !newDocumentTitle.trim()) return
    setBusy(true)
    setError(undefined)
    try {
      const child = await onCreateDocument(
        container,
        current.wire.id,
        emptyDocument(newDocumentTitle.trim()),
      )
      const parentContent = withMarkdown(current.document, markdown)
      const parent = await onUpdateDocument(current, {
        ...parentContent,
        blocks: [
          ...parentContent.blocks,
          {
            id: crypto.randomUUID(),
            type: 'document',
            document_id: child.wire.id,
            title: newDocumentTitle.trim(),
          },
        ],
      })
      replaceDocument(parent)
      setDocuments((values) => [...values, child])
      selectDocument(child)
      setNewDocumentTitle('')
      setShowDocumentForm(false)
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Creazione non riuscita'))
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

  const files = current.document.blocks.filter(
    (block): block is InfoFileBlock => block.type === 'file',
  )
  const childBlocks = current.document.blocks.filter(
    (block) => block.type === 'document',
  )

  const renderBlockMenu = (target: 'main' | 'trailing') => (
    <div className="tasklist-info-block-menu" role="menu" aria-label="Tipi di blocco">
      {(['h1', 'h2', 'h3'] as const).map((kind, index) => (
        <button key={kind} type="button" role="menuitem" onMouseDown={(event) => event.preventDefault()} onClick={() => insertBlock(kind, target)}>
          <span className={`tasklist-info-block-icon tasklist-info-block-icon--${kind}`}>{kind.toUpperCase()}</span>
          <span>{['Titolo grande', 'Titolo medio', 'Titolo piccolo'][index]}</span>
        </button>
      ))}
      <button type="button" role="menuitem" onMouseDown={(event) => event.preventDefault()} onClick={() => insertBlock('bullet', target)}>
        <span className="tasklist-info-block-icon"><ListIcon aria-hidden /></span>
        <span>Elenco puntato</span>
      </button>
      <button type="button" role="menuitem" onMouseDown={(event) => event.preventDefault()} onClick={() => openFilePicker('image/*')}>
        <span className="tasklist-info-block-icon"><ImageIcon aria-hidden /></span>
        <span>Immagine</span>
      </button>
      <button type="button" role="menuitem" onMouseDown={(event) => event.preventDefault()} onClick={() => openFilePicker('*/*')}>
        <span className="tasklist-info-block-icon"><PaperclipIcon aria-hidden /></span>
        <span>File</span>
      </button>
      <button type="button" role="menuitem" onMouseDown={(event) => event.preventDefault()} onClick={() => {
        setBlockMenuOpen(false)
        setTrailingBlockMenuOpen(false)
        setShowDocumentForm(true)
      }}>
        <span className="tasklist-info-block-icon"><FolderIcon aria-hidden /></span>
        <span>Documento</span>
      </button>
    </div>
  )

  return (
    <div className={presentation === 'overview' ? 'tasklist-info-panel tasklist-info-panel--overview' : 'tasklist-info-panel'}>
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
                : document.document.title || 'Documento'}
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
          onClick={() => fileInputRef.current?.click()}
          disabled={busy}
        >
          <PaperclipIcon aria-hidden />
          File o immagine
        </button>
        <button
          type="button"
          onClick={() => setShowDocumentForm((visible) => !visible)}
          disabled={busy}
        >
          <FolderIcon aria-hidden />
          Documento
        </button>
      </div>}
      <input
        ref={fileInputRef}
        type="file"
        hidden
        onChange={(event) => {
          const file = event.target.files?.[0]
          if (file) void addFile(file)
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
            placeholder="Nome documento"
            aria-label="Nome sotto-documento"
            onChange={(event) => setNewDocumentTitle(event.target.value)}
          />
          <button type="submit" disabled={busy}>Crea</button>
          <button type="button" onClick={() => setShowDocumentForm(false)}>
            Annulla
          </button>
        </form>
      )}

      {editing ? (
        <div className="tasklist-info-editor">
          {presentation === 'overview' ? (
            <div className="tasklist-info-overview-canvas">
              <div
                ref={overviewEditorRef}
                className="tasklist-info-overview-editor"
                contentEditable
                suppressContentEditableWarning
                role="textbox"
                aria-multiline="true"
                data-placeholder="Scrivi una nota o usa / per aggiungere contenuti"
                aria-label="Testo info in Markdown"
                onKeyDown={(event) => openBlockMenuFromSlash('main', event)}
                onInput={(event) => {
                  ensureOverviewTrailingPromptLine(event.currentTarget)
                  setMarkdown(overviewEditorToMarkdown(event.currentTarget))
                  setDirty(true)
                  if (slashMenuTargetRef.current !== 'main') {
                    setBlockMenuOpen(false)
                  }
                  positionBlockInserter('main')
                }}
                onKeyUp={() => positionBlockInserter('main')}
                onClick={(event) => {
                  if (
                    event.target instanceof HTMLInputElement &&
                    event.target.type === 'checkbox'
                  ) {
                    setMarkdown(overviewEditorToMarkdown(event.currentTarget))
                    setDirty(true)
                  }
                  positionBlockInserter(
                    'main',
                    directEditorBlock(event.currentTarget, event.target),
                  )
                }}
                onBlur={(event) => {
                  if (
                    event.relatedTarget instanceof Node &&
                    (event.currentTarget.contains(event.relatedTarget) ||
                      event.relatedTarget.parentElement?.closest('.tasklist-info-block-menu'))
                  ) return
                  setBlockMenuOpen(false)
                  slashMenuTargetRef.current = null
                  const next = overviewEditorToMarkdown(event.currentTarget)
                  event.currentTarget.innerHTML = markdownToOverviewHtml(next)
                  ensureOverviewTrailingPromptLine(event.currentTarget)
                  setMarkdown(next)
                  setDirty(true)
                  if (current) {
                    void save({
                      ...withMarkdown(current.document, next),
                      title,
                    })
                  }
                }}
              />
              {blockMenuOpen && <div
                className="tasklist-info-block-inserter is-visible"
                style={{ top: `${blockInserterTop}px` }}
              >
                {renderBlockMenu('main')}
              </div>}
            </div>
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

      {files.length > 0 && (
        <div className={`tasklist-info-files${presentation === 'overview' ? ' tasklist-info-files--overview' : ''}`}>
          {files.map((file) => (
            file.content_type.startsWith('image/') ? (
              <InfoImageBlock
                key={file.id}
                document={current}
                file={file}
                onRead={onReadFile}
                onDownload={onDownloadFile}
                onResize={resizeImage}
              />
            ) : (
              <div
                key={file.id}
                className="tasklist-info-file"
              >
                <FileIcon aria-hidden />
                <button
                  type="button"
                  className="tasklist-info-attachment-link"
                  onClick={() => void onDownloadFile(current, file)}
                >
                  {file.file_name}
                </button>
              </div>
            )
          ))}
        </div>
      )}

      {childBlocks.length > 0 && (
        <div className={`tasklist-info-documents${presentation === 'overview' ? ' tasklist-info-documents--overview' : ''}`}>
          {childBlocks.map((block) => (
            <button
              key={block.id}
              type="button"
              onClick={() => {
                const child = documents.find(
                  (document) => document.wire.id === block.document_id,
                )
                if (!child) return
                selectDocument(child)
              }}
            >
              <FolderIcon aria-hidden />
              <span>{block.title}</span>
            </button>
          ))}
        </div>
      )}

      {presentation === 'overview' && (files.length > 0 || childBlocks.length > 0) && (
        <div
          className="tasklist-info-overview-canvas tasklist-info-trailing-canvas"
          onClick={(event) => {
            if (event.target !== event.currentTarget) return
            const editor = trailingEditorRef.current
            if (!editor) return
            editor.focus()
            const range = window.document.createRange()
            range.selectNodeContents(editor)
            range.collapse(false)
            const selection = window.getSelection()
            selection?.removeAllRanges()
            selection?.addRange(range)
          }}
        >
          <div
            ref={trailingEditorRef}
            className="tasklist-info-overview-editor tasklist-info-trailing-editor"
            contentEditable
            suppressContentEditableWarning
            role="textbox"
            aria-multiline="true"
            data-placeholder="Scrivi una nota o usa / per aggiungere contenuti"
            aria-label="Testo dopo gli allegati"
            onKeyDown={(event) => openBlockMenuFromSlash('trailing', event)}
            onInput={(event) => {
              ensureOverviewTrailingPromptLine(event.currentTarget)
              setTrailingMarkdown(overviewEditorToMarkdown(event.currentTarget))
              setDirty(true)
              if (slashMenuTargetRef.current !== 'trailing') {
                setTrailingBlockMenuOpen(false)
              }
              positionBlockInserter('trailing')
            }}
            onKeyUp={() => positionBlockInserter('trailing')}
            onClick={(event) => {
              if (
                event.target instanceof HTMLInputElement &&
                event.target.type === 'checkbox'
              ) {
                setTrailingMarkdown(overviewEditorToMarkdown(event.currentTarget))
                setDirty(true)
              }
              positionBlockInserter(
                'trailing',
                directEditorBlock(event.currentTarget, event.target),
              )
            }}
            onBlur={(event) => {
              if (
                event.relatedTarget instanceof Node &&
                (event.currentTarget.contains(event.relatedTarget) ||
                  event.relatedTarget.parentElement?.closest('.tasklist-info-block-menu'))
              ) return
              setTrailingBlockMenuOpen(false)
              slashMenuTargetRef.current = null
              const next = overviewEditorToMarkdown(event.currentTarget)
              event.currentTarget.innerHTML = markdownToOverviewHtml(next)
              ensureOverviewTrailingPromptLine(event.currentTarget)
              setTrailingMarkdown(next)
              setDirty(true)
              void save({
                ...withTrailingMarkdown(
                  withMarkdown(current.document, markdown),
                  next,
                ),
                title,
              })
            }}
          />
          {trailingBlockMenuOpen && <div
            className="tasklist-info-block-inserter tasklist-info-block-inserter--trailing is-visible"
            style={{ top: `${trailingBlockInserterTop}px` }}
          >
            {renderBlockMenu('trailing')}
          </div>}
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
