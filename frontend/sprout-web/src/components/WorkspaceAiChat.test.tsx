import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type {
  WorkspaceChatService,
  WorkspaceChatTurn,
  WorkspaceSnapshot,
} from '../ai/workspace-chat'
import { WorkspaceAiChat } from './WorkspaceAiChat'

const snapshot = {
  project: { wire: { id: crypto.randomUUID() } },
  topics: [],
  taskLists: [],
  tasks: [],
} as unknown as WorkspaceSnapshot

const successfulTurns: WorkspaceChatTurn[] = [
  {
    id: crypto.randomUUID(),
    role: 'user',
    content: 'Prima domanda',
    createdAt: '2026-09-05T18:00:00Z',
  },
  {
    id: crypto.randomUUID(),
    role: 'assistant',
    content: 'Risposta',
    createdAt: '2026-09-05T18:00:01Z',
  },
]

describe('WorkspaceAiChat composer', () => {
  it('executes only the exact proposed action after one-shot confirmation', async () => {
    const user = userEvent.setup()
    const proposal = {
      id: crypto.randomUUID(),
      requestId: successfulTurns[0].id,
      kind: 'create_task' as const,
      targetId: crypto.randomUUID(),
      title: 'Analizzare i certificati API',
      notes: '',
      priority: 'normal' as const,
      assigneeIdentityId: '' as const,
      name: '',
      email: '',
      role: '' as const,
      summary: 'Crea il task “Analizzare i certificati API” nella tasklist “Infrastruttura” con priorità normal.',
      status: 'pending' as const,
    }
    const actionTurns: WorkspaceChatTurn[] = [
      successfulTurns[0],
      { ...successfulTurns[1], proposal },
    ]
    const executedTurns: WorkspaceChatTurn[] = [
      successfulTurns[0],
      { ...successfulTurns[1], proposal: { ...proposal, status: 'executed' } },
    ]
    const onExecuteAction = vi.fn().mockResolvedValue(undefined)
    const service = {
      history: vi.fn().mockReturnValue(actionTurns),
      availability: vi.fn().mockReturnValue({
        profileConfigured: true,
        runtimeConnected: true,
        model: 'deepseek-v4-flash',
      }),
      ask: vi.fn(),
      clear: vi.fn(),
      updateProposalStatus: vi.fn().mockResolvedValue(executedTurns),
    } as unknown as WorkspaceChatService

    render(
      <WorkspaceAiChat
        snapshot={snapshot}
        service={service}
        onExecuteAction={onExecuteAction}
        onOpenAiSettings={vi.fn()}
        onClose={vi.fn()}
      />,
    )

    expect(onExecuteAction).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Conferma ed esegui' }))

    expect(onExecuteAction).toHaveBeenCalledWith(proposal)
    expect(await screen.findByText('Azione eseguita')).toBeVisible()
    expect(service.updateProposalStatus).toHaveBeenCalledWith(
      snapshot.project.wire.id,
      successfulTurns[1].id,
      'executed',
    )
  })

  it('reenables submission and restores focus after a provider failure', async () => {
    const user = userEvent.setup()
    const ask = vi.fn()
      .mockRejectedValueOnce(new Error('Richiesta rifiutata dal provider (400)'))
      .mockResolvedValueOnce(successfulTurns)
    const service = {
      history: vi.fn().mockReturnValue([]),
      availability: vi.fn().mockReturnValue({
        profileConfigured: true,
        runtimeConnected: true,
        model: 'deepseek-v4-flash',
      }),
      ask,
      clear: vi.fn(),
    } as unknown as WorkspaceChatService

    render(
      <WorkspaceAiChat
        snapshot={snapshot}
        service={service}
        onOpenAiSettings={vi.fn()}
        onClose={vi.fn()}
      />,
    )
    const input = screen.getByRole('textbox', { name: 'Messaggio per Ask to AI' })
    const send = screen.getByRole('button', { name: 'Invia messaggio' })
    await user.type(input, 'Prima domanda')
    await user.click(send)

    expect(await screen.findByRole('alert')).toHaveTextContent('400')
    expect(send).toBeEnabled()
    expect(input).toHaveFocus()

    await user.click(send)
    expect(await screen.findByText('Risposta')).toBeVisible()
    expect(ask).toHaveBeenCalledTimes(2)
  })

  it('keeps the textarea writable and preserves a next draft while a request is pending', async () => {
    const user = userEvent.setup()
    let resolveRequest!: (turns: WorkspaceChatTurn[]) => void
    const pending = new Promise<WorkspaceChatTurn[]>((resolve) => { resolveRequest = resolve })
    const service = {
      history: vi.fn().mockReturnValue([]),
      availability: vi.fn().mockReturnValue({
        profileConfigured: true,
        runtimeConnected: true,
        model: 'deepseek-v4-flash',
      }),
      ask: vi.fn().mockReturnValue(pending),
      clear: vi.fn(),
    } as unknown as WorkspaceChatService

    render(
      <WorkspaceAiChat
        snapshot={snapshot}
        service={service}
        onOpenAiSettings={vi.fn()}
        onClose={vi.fn()}
      />,
    )
    const input = screen.getByRole('textbox', { name: 'Messaggio per Ask to AI' })
    await user.type(input, 'Prima domanda')
    await user.click(screen.getByRole('button', { name: 'Invia messaggio' }))
    expect(input).toBeEnabled()
    expect(screen.getByRole('status')).toHaveTextContent('Invio in corso…')

    await user.type(input, ' e seconda bozza')
    resolveRequest(successfulTurns)

    expect(await screen.findByText('Risposta')).toBeVisible()
    expect(input).toHaveValue('Prima domanda e seconda bozza')
  })

  it('submits with Enter through the same send path', async () => {
    const user = userEvent.setup()
    const ask = vi.fn().mockResolvedValue(successfulTurns)
    const service = {
      history: vi.fn().mockReturnValue([]),
      availability: vi.fn().mockReturnValue({
        profileConfigured: true,
        runtimeConnected: true,
        model: 'deepseek-v4-flash',
      }),
      ask,
      clear: vi.fn(),
    } as unknown as WorkspaceChatService

    render(
      <WorkspaceAiChat
        snapshot={snapshot}
        service={service}
        onOpenAiSettings={vi.fn()}
        onClose={vi.fn()}
      />,
    )
    const input = screen.getByRole('textbox', { name: 'Messaggio per Ask to AI' })
    await user.type(input, 'Prima domanda{Enter}')

    expect(await screen.findByText('Risposta')).toBeVisible()
    expect(ask).toHaveBeenCalledOnce()
  })

  it('opens AI settings when submission has no configured provider', async () => {
    const user = userEvent.setup()
    const onOpenAiSettings = vi.fn()
    const service = {
      history: vi.fn().mockReturnValue([]),
      availability: vi.fn().mockReturnValue({
        profileConfigured: false,
        runtimeConnected: false,
      }),
      ask: vi.fn(),
      clear: vi.fn(),
    } as unknown as WorkspaceChatService

    render(
      <WorkspaceAiChat
        snapshot={snapshot}
        service={service}
        onOpenAiSettings={onOpenAiSettings}
        onClose={vi.fn()}
      />,
    )
    await user.type(
      screen.getByRole('textbox', { name: 'Messaggio per Ask to AI' }),
      'Prima domanda',
    )
    await user.click(screen.getByRole('button', { name: 'Invia messaggio' }))

    expect(onOpenAiSettings).toHaveBeenCalledOnce()
    expect(service.ask).not.toHaveBeenCalled()
  })
})
