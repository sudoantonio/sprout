import { useEffect, useMemo, useRef, useState } from 'react'
import type { Uuid } from '../api/contracts'
import { linkifyInfoText, parseInfoMarkdown } from '../domain/info-documents'
import type {
  DecryptedInfoDocument,
  InfoDocumentContent,
  InfoFileBlock,
} from '../domain/models'
import type { TaskListItem } from '../store/app-store'
import { FolderIcon, PaperclipIcon, PencilIcon } from './icons'

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

const infoErrorMessage = (reason: unknown, fallback: string): string => {
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

const LinkifiedText = ({ value }: { value: string }) => (
  <>
    {linkifyInfoText(value).map((segment, index) =>
      segment.type === 'link' ? (
        <a
          key={`${index}-${segment.href}`}
          href={segment.href}
          target="_blank"
          rel="noreferrer noopener"
        >
          {segment.value}
        </a>
      ) : (
        <span key={`${index}-${segment.value}`}>{segment.value}</span>
      ),
    )}
  </>
)

const MarkdownPreview = ({ value }: { value: string }) => {
  if (!value.trim()) {
    return (
      <p className="tasklist-info-placeholder">
        Inserisci testo, link e informazioni relative a questa task list.
      </p>
    )
  }
  return (
    <div className="tasklist-info-markdown">
      {parseInfoMarkdown(value).map((line) => {
        if (line.type === 'blank') {
          return <div key={line.key} className="tasklist-info-blank" aria-hidden />
        }
        if (line.type === 'heading') {
          const Heading = `h${line.level}` as 'h1' | 'h2' | 'h3'
          return (
            <Heading key={line.key}>
              <LinkifiedText value={line.value} />
            </Heading>
          )
        }
        if (line.type === 'list-item') {
          return (
            <p key={line.key} className="tasklist-info-list-item">
              <span aria-hidden>•</span>
              <LinkifiedText value={line.value} />
            </p>
          )
        }
        return (
          <p key={line.key}>
            <LinkifiedText value={line.value} />
          </p>
        )
      })}
    </div>
  )
}

const InfoImageBlock = ({
  document,
  file,
  onRead,
  onDownload,
}: {
  document: DecryptedInfoDocument
  file: InfoFileBlock
  onRead(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<Blob>
  onDownload(document: DecryptedInfoDocument, file: InfoFileBlock): Promise<void>
}) => {
  const [source, setSource] = useState<string>()

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
    <button
      type="button"
      className="tasklist-info-image"
      onClick={() => void onDownload(document, file)}
    >
      {source ? (
        <img src={source} alt={file.file_name} />
      ) : (
        <span className="tasklist-info-image-placeholder">Immagine cifrata</span>
      )}
      <span>{file.file_name}</span>
    </button>
  )
}

export const TaskListInfoPanel = ({
  list,
  onLoad,
  onCreateDocument,
  onUpdateDocument,
  onUploadFile,
  onReadFile,
  onDownloadFile,
}: {
  list: TaskListItem
  onLoad(list: TaskListItem): Promise<DecryptedInfoDocument[]>
  onCreateDocument(
    list: TaskListItem,
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
}) => {
  const fileInputRef = useRef<HTMLInputElement>(null)
  const loadRef = useRef(onLoad)
  const createRef = useRef(onCreateDocument)
  const listRef = useRef(list)
  loadRef.current = onLoad
  createRef.current = onCreateDocument
  listRef.current = list
  const [documents, setDocuments] = useState<DecryptedInfoDocument[]>([])
  const [currentId, setCurrentId] = useState<Uuid>()
  const [markdown, setMarkdown] = useState('')
  const [editing, setEditing] = useState(false)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const [showDocumentForm, setShowDocumentForm] = useState(false)
  const [newDocumentTitle, setNewDocumentTitle] = useState('')

  useEffect(() => {
    let active = true
    setLoading(true)
    setError(undefined)
    void (async () => {
      try {
        let loaded = await loadRef.current(listRef.current)
        let root = loaded.find((document) => !document.wire.parent_document_id)
        if (!root) {
          try {
            root = await createRef.current(
              listRef.current,
              undefined,
              emptyDocument(),
            )
            loaded = [...loaded, root]
          } catch {
            loaded = await loadRef.current(listRef.current)
            root = loaded.find((document) => !document.wire.parent_document_id)
            if (!root) throw new Error('Impossibile inizializzare il documento info')
          }
        }
        if (!active) return
        setDocuments(loaded)
        setCurrentId(root.wire.id)
        setMarkdown(markdownFor(root))
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
  }, [list.wire.id])

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
        content ?? withMarkdown(current.document, markdown),
      )
      replaceDocument(next)
      setMarkdown(markdownFor(next))
      setEditing(false)
      return next
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Salvataggio non riuscito'))
      return undefined
    } finally {
      setBusy(false)
    }
  }

  const addFile = async (file: File) => {
    if (!current) return
    setBusy(true)
    setError(undefined)
    try {
      const block = await onUploadFile(current, file)
      const nextContent = withMarkdown(current.document, markdown)
      const next = await onUpdateDocument(current, {
        ...nextContent,
        blocks: [...nextContent.blocks, block],
      })
      replaceDocument(next)
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Upload non riuscito'))
    } finally {
      setBusy(false)
      if (fileInputRef.current) fileInputRef.current.value = ''
    }
  }

  const addDocument = async () => {
    if (!current || !newDocumentTitle.trim()) return
    setBusy(true)
    setError(undefined)
    try {
      const child = await onCreateDocument(
        list,
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
      setCurrentId(child.wire.id)
      setMarkdown('')
      setNewDocumentTitle('')
      setShowDocumentForm(false)
      setEditing(true)
    } catch (reason) {
      setError(infoErrorMessage(reason, 'Creazione non riuscita'))
    } finally {
      setBusy(false)
    }
  }

  if (loading) return <p className="tasklist-history-empty">Caricamento info…</p>
  if (!current) {
    return (
      <p className="tasklist-history-empty" role="alert">
        {error ?? 'Documento info non disponibile.'}
      </p>
    )
  }

  const files = current.document.blocks.filter(
    (block): block is InfoFileBlock => block.type === 'file',
  )
  const childBlocks = current.document.blocks.filter(
    (block) => block.type === 'document',
  )

  return (
    <div className="tasklist-info-panel">
      {breadcrumbs.length > 1 && (
        <nav className="tasklist-info-breadcrumbs" aria-label="Documenti info">
          {breadcrumbs.map((document, index) => (
            <button
              key={document.wire.id}
              type="button"
              disabled={document.wire.id === current.wire.id}
              onClick={() => {
                setCurrentId(document.wire.id)
                setMarkdown(markdownFor(document))
                setEditing(false)
              }}
            >
              {index === 0 ? 'Info' : document.document.title || 'Documento'}
            </button>
          ))}
        </nav>
      )}

      {current.document.title && <h1>{current.document.title}</h1>}

      <div className="tasklist-info-toolbar" aria-label="Azioni documento info">
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
        <input
          ref={fileInputRef}
          type="file"
          hidden
          onChange={(event) => {
            const file = event.target.files?.[0]
            if (file) void addFile(file)
          }}
        />
      </div>

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
          <textarea
            autoFocus
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
        </div>
      ) : (
        <div className="tasklist-info-preview">
          <MarkdownPreview value={markdown} />
        </div>
      )}

      {files.length > 0 && (
        <div className="tasklist-info-files">
          {files.map((file) => (
            file.content_type.startsWith('image/') ? (
              <InfoImageBlock
                key={file.id}
                document={current}
                file={file}
                onRead={onReadFile}
                onDownload={onDownloadFile}
              />
            ) : (
              <button
                key={file.id}
                type="button"
                className="tasklist-info-file"
                onClick={() => void onDownloadFile(current, file)}
              >
                <PaperclipIcon aria-hidden />
                <span>{file.file_name}</span>
                <small>File</small>
              </button>
            )
          ))}
        </div>
      )}

      {childBlocks.length > 0 && (
        <div className="tasklist-info-documents">
          {childBlocks.map((block) => (
            <button
              key={block.id}
              type="button"
              onClick={() => {
                const child = documents.find(
                  (document) => document.wire.id === block.document_id,
                )
                if (!child) return
                setCurrentId(child.wire.id)
                setMarkdown(markdownFor(child))
                setEditing(false)
              }}
            >
              <FolderIcon aria-hidden />
              <span>{block.title}</span>
            </button>
          ))}
        </div>
      )}

      {error && <p className="tasklist-info-error" role="alert">{error}</p>}
    </div>
  )
}
