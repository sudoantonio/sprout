import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { AgentDirectoryItemDto } from '../api/contracts'
import { AgentManagementPanel } from './AgentManagementPanel'

const agent: AgentDirectoryItemDto = {
  profile_resource_node_id: '00000000-0000-4000-8000-000000000107',
  key_epoch: 1,
  id: '00000000-0000-4000-8000-000000000101',
  principal_identity_id: '00000000-0000-4000-8000-000000000102',
  identity_handle: 'minerva-agent',
  controller_identity_id: '00000000-0000-4000-8000-000000000103',
  availability: 'controller_private',
  state: 'active',
  created_at: '2026-08-26T00:00:00.000Z',
  runner_id: '00000000-0000-4000-8000-000000000104',
  runner_device_id: '00000000-0000-4000-8000-000000000105',
  runner_state: 'pending_key',
  runner_last_seen_at: null,
  local_goal_id: '00000000-0000-4000-8000-000000000106',
  local_goal_revision: 1,
  local_goal_state: 'active',
}

const baseProps = {
  agents: [agent],
  onSelectAgent: vi.fn(),
  onShowDirectory: vi.fn(),
  onOpenAiSettings: vi.fn(),
  onProvision: vi.fn().mockResolvedValue({
    agent_id: agent.id,
    principal_identity_id: agent.principal_identity_id,
    runner_id: agent.runner_id,
    runner_device_id: agent.runner_device_id,
    bootstrap_token: 'one-shot-bootstrap-token',
    bootstrap_expires_at: '2026-08-26T03:00:00.000Z',
    runner_state: 'pending_key' as const,
  }),
}

describe('AgentManagementPanel', () => {
  it('shows the server-derived directory and opens an agent detail', async () => {
    const user = userEvent.setup()
    const onSelectAgent = vi.fn()
    render(<AgentManagementPanel {...baseProps} onSelectAgent={onSelectAgent} />)

    expect(screen.getByRole('heading', { name: 'Agenti' })).toBeVisible()
    expect(screen.getByLabelText('Atlas, agente di esempio, Rest')).toBeVisible()
    expect(screen.queryByRole('heading', { name: 'Working' })).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Done' })).not.toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Rest' })).toBeVisible()
    await user.click(screen.getByRole('button', { name: /minerva-agent/i }))
    expect(onSelectAgent).toHaveBeenCalledWith(agent.id)
  })

  it('validates the natural-language proposal before the request', async () => {
    const user = userEvent.setup()
    const onProvision = vi.fn()
    render(<AgentManagementPanel {...baseProps} onProvision={onProvision} />)

    await user.click(screen.getByRole('button', { name: 'Crea nuovo agente' }))
    await user.type(screen.getByLabelText('Nome tecnico'), 'Nome non valido')
    await user.type(screen.getByLabelText('System prompt'), 'Rispondi ai commenti del progetto.')
    await user.click(screen.getByRole('button', { name: 'Struttura proposta' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'senza spazi',
    )
    expect(onProvision).not.toHaveBeenCalled()
  })

  it('reviews the compiled proposal before provisioning and presents the one-shot token', async () => {
    const user = userEvent.setup()
    const onProvision = vi.fn().mockResolvedValue({
      agent_id: agent.id,
      principal_identity_id: agent.principal_identity_id,
      runner_id: agent.runner_id,
      runner_device_id: agent.runner_device_id,
      bootstrap_token: 'one-shot-bootstrap-token',
      bootstrap_expires_at: '2026-08-26T03:00:00.000Z',
      runner_state: 'pending_key' as const,
    })
    render(<AgentManagementPanel {...baseProps} onProvision={onProvision} />)

    await user.click(screen.getByRole('button', { name: 'Crea nuovo agente' }))
    await user.type(screen.getByLabelText('Nome tecnico'), 'project-helper')
    await user.type(
      screen.getByLabelText('System prompt'),
      'Crea task e pubblica commenti sintetici sul progetto.',
    )
    await user.click(screen.getByRole('checkbox', { name: 'Creare task' }))
    await user.selectOptions(screen.getByLabelText('Visibilità'), 'project_delegable')
    await user.click(screen.getByRole('button', { name: 'Struttura proposta' }))

    expect(screen.getByRole('heading', { name: 'System prompt' })).toBeVisible()
    expect(screen.getByText('Creare task')).toBeVisible()
    expect(screen.getByText('Pubblicare commenti')).toBeVisible()
    expect(onProvision).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Approva e crea agente' }))

    expect(onProvision).toHaveBeenCalledWith({
      identityHandle: 'project-helper',
      systemPrompt: 'Crea task e pubblica commenti sintetici sul progetto.',
      availability: 'project_delegable',
      actions: ['post_comment', 'create_task'],
    })
    expect(await screen.findByDisplayValue('one-shot-bootstrap-token')).toBeVisible()
    expect(screen.getByText('Visibile una sola volta')).toBeVisible()
  })
})
