import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type {
  WorkspaceActionProposal,
  WorkspaceChatService,
  WorkspaceChatTurn,
  WorkspaceSnapshot,
} from '../ai/workspace-chat'
import { ChevronDownIcon, PlusIcon, TimeHistoryIcon, XIcon } from './icons'

const resizeComposer = (textarea: HTMLTextAreaElement) => {
  textarea.style.height = 'auto'
  const nextHeight = Math.min(textarea.scrollHeight, 288)
  textarea.style.height = `${nextHeight}px`
  textarea.style.overflowY = textarea.scrollHeight > nextHeight ? 'auto' : 'hidden'
}

export const WorkspaceAiChat = ({
  snapshot,
  service,
  onExecuteAction,
  onOpenAiSettings,
  onClose,
}: {
  snapshot?: WorkspaceSnapshot
  service?: WorkspaceChatService
  onExecuteAction?(proposal: WorkspaceActionProposal): Promise<void>
  onOpenAiSettings(): void
  onClose(): void
}) => {
  const projectId = snapshot?.project.wire.id
  const [messages, setMessages] = useState<WorkspaceChatTurn[]>(() =>
    projectId && service ? service.history(projectId) : [],
  )
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [historyOpen, setHistoryOpen] = useState(false)
  const log = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const availability = service?.availability()

  useLayoutEffect(() => {
    if (log.current) log.current.scrollTop = log.current.scrollHeight
  }, [messages])
  useEffect(() => {
    inputRef.current?.focus()
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    document.addEventListener('keydown', keydown)
    return () => document.removeEventListener('keydown', keydown)
  }, [onClose])

  const unavailable = !snapshot || !service
    ? 'Seleziona un progetto per iniziare la chat.'
    : !availability?.profileConfigured
      ? 'Configura un provider esterno nelle impostazioni AI per iniziare.'
      : !availability.runtimeConnected
        ? 'Collega lo Sprout Local Edge Runtime per usare il provider esterno configurato.'
        : undefined

  const send = async () => {
    if (!snapshot || !service || busy || !draft.trim()) return
    if (unavailable) {
      setError(`Invio non eseguito: ${unavailable}`)
      onOpenAiSettings()
      return
    }
    const submittedDraft = draft
    setBusy(true)
    setError('')
    try {
      setMessages(await service.ask(snapshot, submittedDraft))
      setDraft((current) => current === submittedDraft ? '' : current)
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Invio non riuscito.')
    } finally {
      setBusy(false)
      inputRef.current?.focus()
    }
  }

  const clear = async () => {
    if (!projectId || !service || busy) return
    await service.clear(projectId)
    setMessages([])
    setError('')
  }

  const setProposalStatus = (
    turnId: WorkspaceChatTurn['id'],
    status: WorkspaceActionProposal['status'],
    proposalError?: string,
  ) => {
    setMessages((current) => current.map((turn) =>
      turn.id === turnId && turn.proposal
        ? {
            ...turn,
            proposal: {
              ...turn.proposal,
              status,
              ...(proposalError ? { error: proposalError } : { error: undefined }),
            },
          }
        : turn))
  }

  const confirmProposal = async (turn: WorkspaceChatTurn) => {
    if (!projectId || !service || !turn.proposal || !onExecuteAction) return
    const acceptedPlan = turn.proposal
    setProposalStatus(turn.id, 'executing')
    setError('')
    try {
      await onExecuteAction(acceptedPlan)
      setMessages(await service.updateProposalStatus(projectId, turn.id, 'executed'))
    } catch (failure) {
      const message = failure instanceof Error ? failure.message : 'Azione non eseguita.'
      setMessages(await service.updateProposalStatus(projectId, turn.id, 'failed', message))
    }
  }

  const cancelProposal = async (turn: WorkspaceChatTurn) => {
    if (!projectId || !service || !turn.proposal) return
    setMessages(await service.updateProposalStatus(projectId, turn.id, 'cancelled'))
  }

  return createPortal(
    <section className={`board-ai-badge${messages.length > 0 ? ' board-ai-badge--conversation' : ''}`} role="dialog" aria-label="New chat">
      <header className="board-ai-badge-header">
        <div className="board-ai-badge-title">
          <button
            type="button"
            className="board-ai-badge-history"
            onClick={() => setHistoryOpen((open) => !open)}
            aria-label="Mostra chat passate"
            aria-expanded={historyOpen}
            title="Chat passate"
          >
            <TimeHistoryIcon aria-hidden />
          </button>
          <span>New chat</span>
        </div>
        <button type="button" aria-label="Chiudi New chat" onClick={onClose}><XIcon aria-hidden /></button>
      </header>
      {historyOpen && <div className="board-ai-badge-history-menu" role="status">
        <span>{messages.length > 0 ? `${messages.length} messaggi nel progetto` : 'Nessuna chat precedente'}</span>
        {messages.length > 0 && <button type="button" onClick={() => void clear()}>Nuova chat</button>}
      </div>}
      <div className="board-ai-badge-body">
        {!historyOpen && messages.length === 0 && <img className="board-ai-badge-empty-logo" src="/sprout-ai-logo.png" alt="" />}
        {messages.length > 0 && <div ref={log} className="workspace-ai-messages" role="log" aria-label="Messaggi Ask to AI" aria-live="polite">
          {messages.map((message) => <article className="agent-conversation-turn" key={message.id}>
            <div className={message.role === 'user' ? 'agent-conversation-question' : 'agent-conversation-answer'}>
              <strong>{message.role === 'user' ? 'Tu' : 'AI'}</strong>
              <p>{message.content}</p>
              {message.proposal && <div className="workspace-ai-action" aria-label="Piano azione Ask to AI">
                <span className="workspace-ai-action-label">Azione proposta</span>
                <p>{message.proposal.summary}</p>
                {message.proposal.error && <p className="workspace-ai-action-error" role="alert">{message.proposal.error}</p>}
                <div className="workspace-ai-action-controls">
                  {(message.proposal.status === 'pending' || message.proposal.status === 'failed') && <>
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={() => void cancelProposal(message)}
                    >Annulla</button>
                    <button
                      type="button"
                      className="primary-button"
                      disabled={!onExecuteAction}
                      onClick={() => void confirmProposal(message)}
                    >{message.proposal.status === 'failed' ? 'Riprova e conferma' : 'Conferma ed esegui'}</button>
                  </>}
                  {message.proposal.status === 'executing' && <span role="status">Esecuzione…</span>}
                  {message.proposal.status === 'executed' && <span className="workspace-ai-action-success">Azione eseguita</span>}
                  {message.proposal.status === 'cancelled' && <span>Azione annullata</span>}
                </div>
              </div>}
            </div>
          </article>)}
        </div>}
        {(unavailable || error || busy) && <p className="board-ai-badge-context-status" role={error ? 'alert' : 'status'}>{error || unavailable || 'Invio in corso…'}</p>}
        <form className="agent-chat-composer board-ai-badge-composer" onSubmit={(event) => { event.preventDefault(); void send() }}>
          <textarea
            ref={inputRef}
            value={draft}
            onChange={(event) => { setDraft(event.target.value); resizeComposer(event.currentTarget) }}
            placeholder="Ask everything"
            aria-label="Messaggio per Ask to AI"
            aria-busy={busy}
            maxLength={4000}
            rows={1}
            disabled={!snapshot || !service}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
                event.preventDefault()
                if (draft.trim() && !busy) void send()
              }
            }}
          />
          <button type="button" className="agent-chat-attach" aria-label="Contesto del progetto incluso" title="Contesto del progetto incluso">
            <PlusIcon aria-hidden />
          </button>
          <button type="button" className="agent-chat-model" onClick={onOpenAiSettings} aria-label="Apri impostazioni AI" title="Impostazioni AI">
            {availability?.model || 'Configura AI'}
            <ChevronDownIcon aria-hidden />
          </button>
          <button type="button" className="agent-chat-send" aria-label="Invia messaggio" disabled={busy || !draft.trim() || !snapshot || !service} onClick={() => void send()}>
            {busy ? '…' : <svg viewBox="0 0 24 24" fill="none" aria-hidden><path d="M12 19V5m0 0-6 6m6-6 6 6" /></svg>}
          </button>
        </form>
      </div>
    </section>,
    document.body,
  )
}
