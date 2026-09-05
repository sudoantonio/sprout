import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { AgentDirectoryItemDto } from '../api/contracts'
import type { WorkspaceChatService, WorkspaceSnapshot } from '../ai/workspace-chat'
import { AgentConversation } from './AgentConversation'

const agent = {
  id: 'agent',
  identity_handle: 'minerva-agent',
  principal_identity_id: 'agent-principal',
  controller_identity_id: 'owner',
  state: 'active',
  runner_state: 'pending_key',
} as AgentDirectoryItemDto

const snapshot = {
  project: { wire: { id: 'project' } },
  topics: [],
  taskLists: [],
  tasks: [],
} as unknown as WorkspaceSnapshot

const serviceFixture = () => ({
  availability: vi.fn().mockReturnValue({
    profileConfigured: true,
    runtimeConnected: true,
    model: 'deepseek-chat',
  }),
  history: vi.fn().mockReturnValue([]),
  askAboutAgent: vi.fn().mockResolvedValue([
    { id: 'q', role: 'user', content: 'Come procede?', createdAt: '1' },
    { id: 'a', role: 'assistant', content: 'Il lavoro procede.', createdAt: '2' },
  ]),
}) as unknown as WorkspaceChatService

describe('agent personal-proxy conversation', () => {
  it('keeps the input editable even when the observed agent runner is pending', async () => {
    const service = serviceFixture()
    const user = userEvent.setup()
    render(<AgentConversation
      agent={agent}
      snapshot={snapshot}
      service={service}
      onOpenAiSettings={vi.fn()}
    />)

    const input = screen.getByLabelText('Messaggio su minerva-agent')
    expect(input).toBeEnabled()
    await user.type(input, 'Come procede?')
    await user.keyboard('{Enter}')

    await waitFor(() => expect(service.askAboutAgent).toHaveBeenCalledWith(
      snapshot,
      {
        agentId: 'agent',
        principalIdentityId: 'agent-principal',
        identityHandle: 'minerva-agent',
      },
      'Come procede?',
    ))
    expect(await screen.findByText('Il lavoro procede.')).toBeVisible()
    expect(screen.getByText('Agente personale · contesto minerva-agent')).toBeVisible()
  })

  it('allows typing before the provider is configured and opens settings on send', async () => {
    const service = serviceFixture()
    vi.mocked(service.availability).mockReturnValue({
      profileConfigured: false,
      runtimeConnected: false,
    })
    const onOpenAiSettings = vi.fn()
    const user = userEvent.setup()
    render(<AgentConversation
      agent={agent}
      snapshot={snapshot}
      service={service}
      onOpenAiSettings={onOpenAiSettings}
    />)

    const input = screen.getByLabelText('Messaggio su minerva-agent')
    await user.type(input, 'Domanda')
    expect(input).toHaveValue('Domanda')
    await user.click(screen.getByRole('button', { name: 'Invia messaggio' }))
    expect(onOpenAiSettings).toHaveBeenCalledOnce()
  })

  it('publishes a confirmed comment through the signed-in user callback', async () => {
    const task = {
      wire: {
        id: 'task-1',
        active_assignee_identity_id: agent.principal_identity_id,
      },
      document: { title: 'Verifica consegna' },
    }
    const taskSnapshot = { ...snapshot, tasks: [task] } as unknown as WorkspaceSnapshot
    const onPostComment = vi.fn().mockResolvedValue(undefined)
    const user = userEvent.setup()
    render(<AgentConversation
      agent={agent}
      snapshot={taskSnapshot}
      service={serviceFixture()}
      onPostComment={onPostComment}
      onOpenAiSettings={vi.fn()}
    />)

    await user.click(screen.getByRole('button', { name: 'Commenta un task' }))
    await user.type(screen.getByLabelText('Commento da pubblicare'), 'Controllo completato.')
    expect(onPostComment).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Conferma e pubblica' }))

    await waitFor(() => expect(onPostComment).toHaveBeenCalledWith(task, 'Controllo completato.'))
    expect(screen.getByRole('status')).toHaveTextContent('Commento pubblicato')
  })
})
