import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { AgentDirectoryItemDto } from '../api/contracts'
import type { DecryptedTask } from '../domain/models'
import type {
  WorkspaceChatService,
  WorkspaceChatTurn,
  WorkspaceSnapshot,
} from '../ai/workspace-chat'

const resizeComposer = (textarea: HTMLTextAreaElement) => {
  textarea.style.height = 'auto'
  const nextHeight = Math.min(textarea.scrollHeight, 288)
  textarea.style.height = `${nextHeight}px`
  textarea.style.overflowY = textarea.scrollHeight > nextHeight ? 'auto' : 'hidden'
}

export const AgentConversation = ({
  agent,
  snapshot,
  service,
  onPostComment,
  onOpenAiSettings,
}: {
  agent: AgentDirectoryItemDto
  snapshot?: WorkspaceSnapshot
  service?: WorkspaceChatService
  onPostComment?(task: DecryptedTask, markdown: string): Promise<void>
  onOpenAiSettings(): void
}) => {
  const projectId = snapshot?.project.wire.id
  const channel = `agent:${agent.id}`
  const [messages, setMessages] = useState<WorkspaceChatTurn[]>(() =>
    projectId && service ? service.history(projectId, channel) : [],
  )
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [commentOpen, setCommentOpen] = useState(false)
  const [commentTaskId, setCommentTaskId] = useState('')
  const [commentDraft, setCommentDraft] = useState('')
  const [commentBusy, setCommentBusy] = useState(false)
  const [commentNotice, setCommentNotice] = useState('')
  const log = useRef<HTMLDivElement>(null)
  const input = useRef<HTMLTextAreaElement>(null)
  const availability = service?.availability()
  const observedTasks = snapshot?.tasks.filter(
    (task) => task.wire.active_assignee_identity_id === agent.principal_identity_id,
  ) ?? []

  useEffect(() => {
    setMessages(projectId && service ? service.history(projectId, channel) : [])
    setDraft('')
    setError('')
    setCommentOpen(false)
    setCommentTaskId('')
    setCommentDraft('')
    setCommentNotice('')
    input.current?.focus()
  }, [channel, projectId, service])

  useLayoutEffect(() => {
    if (log.current) log.current.scrollTop = log.current.scrollHeight
  }, [messages])

  const unavailable = !snapshot || !service
    ? 'Seleziona un progetto per usare l’agente personale.'
    : !availability?.profileConfigured
      ? 'Configura un provider nelle impostazioni AI per ricevere risposte.'
      : !availability.runtimeConnected
        ? 'Questa modalità richiede lo Sprout Local Edge Runtime.'
        : undefined

  const send = async () => {
    if (!snapshot || !service || busy || !draft.trim()) return
    if (unavailable) {
      setError(unavailable)
      onOpenAiSettings()
      return
    }
    const submitted = draft
    setBusy(true)
    setError('')
    try {
      setMessages(await service.askAboutAgent(snapshot, {
        agentId: agent.id,
        principalIdentityId: agent.principal_identity_id,
        identityHandle: agent.identity_handle,
      }, submitted))
      setDraft((current) => current === submitted ? '' : current)
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Invio non riuscito.')
    } finally {
      setBusy(false)
      input.current?.focus()
    }
  }

  const postComment = async () => {
    const task = observedTasks.find((candidate) => candidate.wire.id === commentTaskId)
    if (!task || !onPostComment || commentBusy || !commentDraft.trim()) return
    setCommentBusy(true)
    setError('')
    setCommentNotice('')
    try {
      await onPostComment(task, commentDraft)
      setCommentDraft('')
      setCommentOpen(false)
      setCommentNotice(`Commento pubblicato su “${task.document.title}”.`)
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Pubblicazione non riuscita.')
    } finally {
      setCommentBusy(false)
    }
  }

  return <div className="agent-conversation">
    <p className="agent-conversation-notice">
      Risponde il tuo agente personale usando il lavoro visibile di {agent.identity_handle}.
      L’interrogazione non comanda né richiede il runner dell’agente osservato.
    </p>
    {onPostComment && observedTasks.length > 0 && <div className="agent-conversation-actions">
      <button
        type="button"
        className="secondary-button"
        aria-expanded={commentOpen}
        onClick={() => {
          setCommentOpen((open) => !open)
          setCommentTaskId((current) => current || observedTasks[0].wire.id)
          setCommentNotice('')
        }}
      >
        Commenta un task
      </button>
      <span>La pubblicazione usa i tuoi permessi effettivi.</span>
    </div>}
    {commentOpen && <form
      className="agent-conversation-comment"
      onSubmit={(event) => { event.preventDefault(); void postComment() }}
    >
      <label>
        Task
        <select
          aria-label="Task da commentare"
          value={commentTaskId}
          onChange={(event) => setCommentTaskId(event.target.value)}
        >
          {observedTasks.map((task) => <option key={task.wire.id} value={task.wire.id}>
            {task.document.title}
          </option>)}
        </select>
      </label>
      <label>
        Commento
        <textarea
          aria-label="Commento da pubblicare"
          value={commentDraft}
          maxLength={4_000}
          onChange={(event) => setCommentDraft(event.target.value)}
        />
      </label>
      <div>
        <button type="button" className="secondary-button" onClick={() => setCommentOpen(false)}>
          Annulla
        </button>
        <button
          type="submit"
          className="primary-button"
          disabled={commentBusy || !commentDraft.trim() || !commentTaskId}
        >
          {commentBusy ? 'Pubblicazione…' : 'Conferma e pubblica'}
        </button>
      </div>
    </form>}
    {commentNotice && <p className="agent-conversation-status" role="status">{commentNotice}</p>}
    <div
      ref={log}
      className="agent-conversation-messages"
      role="log"
      aria-label="Messaggi della conversazione"
      aria-live="polite"
    >
      {messages.length === 0 && <p>
        Nessun messaggio. Chiedi informazioni sul lavoro svolto o assegnato all’agente.
      </p>}
      {messages.map((message) => <article className="agent-conversation-turn" key={message.id}>
        <div className={message.role === 'user' ? 'agent-conversation-question' : 'agent-conversation-answer'}>
          <strong>{message.role === 'user'
            ? 'Tu'
            : `Agente personale · contesto ${agent.identity_handle}`}</strong>
          <p>{message.content}</p>
        </div>
      </article>)}
    </div>
    {(unavailable || error || busy) && <p
      className="agent-conversation-status"
      role={error ? 'alert' : 'status'}
    >
      {error || unavailable || 'Invio in corso…'}
    </p>}
    <form
      className="agent-chat-composer agent-conversation-composer"
      onSubmit={(event) => { event.preventDefault(); void send() }}
    >
      <textarea
        ref={input}
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value)
          resizeComposer(event.currentTarget)
        }}
        placeholder={`Chiedi del lavoro di ${agent.identity_handle}`}
        aria-label={`Messaggio su ${agent.identity_handle}`}
        aria-busy={busy}
        maxLength={4000}
        rows={2}
        disabled={!snapshot || !service}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault()
            if (draft.trim() && !busy) void send()
          }
        }}
      />
      <span className="agent-conversation-runtime">
        Agente personale · permessi dell’utente
      </span>
      <button
        type="submit"
        className="agent-chat-send"
        aria-label="Invia messaggio"
        disabled={busy || !draft.trim() || !snapshot || !service}
      >
        {busy ? '…' : <svg viewBox="0 0 24 24" fill="none" aria-hidden>
          <path d="M12 19V5m0 0-6 6m6-6 6 6" />
        </svg>}
      </button>
    </form>
  </div>
}
