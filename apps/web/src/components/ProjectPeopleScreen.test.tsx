import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ProjectPeopleScreen } from './ResourceScreens'

describe('project participants screen', () => {
  it('submits private participant attributes through the encrypted invite flow', async () => {
    const user = userEvent.setup()
    const onInvite = vi.fn().mockResolvedValue(undefined)
    render(
      <ProjectPeopleScreen
        invitations={[]}
        suggestions={[]}
        onRefresh={vi.fn()}
        onInvite={onInvite}
        onAccept={vi.fn()}
        onShare={vi.fn()}
        managedGrants={[]}
        onRevoke={vi.fn()}
        onSuggest={vi.fn()}
      />,
    )

    await user.type(screen.getByLabelText('Email'), 'member@example.test')
    await user.type(screen.getByLabelText('Name'), 'Private Name')
    await user.type(screen.getByLabelText('Phone'), '+3900000000')
    await user.selectOptions(screen.getByLabelText('Role'), 'guest')
    await user.click(
      screen.getByRole('button', { name: 'Encrypt and invite' }),
    )

    expect(onInvite).toHaveBeenCalledWith({
      email: 'member@example.test',
      name: 'Private Name',
      phone: '+3900000000',
      role: 'guest',
    })
  })

  it('requests ranked suggestions without exposing profile fields', async () => {
    const user = userEvent.setup()
    const onSuggest = vi.fn().mockResolvedValue(undefined)
    render(
      <ProjectPeopleScreen
        invitations={[]}
        suggestions={[
          {
            identity_id: crypto.randomUUID(),
            identity_handle: 'known-person',
            shared_project_count: 3,
            most_recent_shared_project_at: '2026-07-18T12:00:00.000Z',
          },
        ]}
        onRefresh={vi.fn()}
        onInvite={vi.fn()}
        onAccept={vi.fn()}
        onShare={vi.fn()}
        managedGrants={[]}
        onRevoke={vi.fn()}
        onSuggest={onSuggest}
      />,
    )

    await user.type(
      screen.getByLabelText('Identity handle prefix'),
      'known',
    )
    await user.click(
      screen.getByRole('button', { name: 'Rank shared participants' }),
    )

    expect(onSuggest).toHaveBeenCalledWith('known')
    expect(screen.getByText('known-person')).toBeInTheDocument()
    expect(screen.getByText('3 shared projects')).toBeInTheDocument()
    expect(screen.queryByText(/example\.test/)).not.toBeInTheDocument()
  })

  it('shares keys only for an accepted invitation without prior coverage', async () => {
    const user = userEvent.setup()
    const identityId = crypto.randomUUID()
    const onShare = vi.fn().mockResolvedValue(undefined)
    render(
      <ProjectPeopleScreen
        invitations={[
          {
            id: crypto.randomUUID(),
            role: 'member',
            state: 'accepted',
            accepted_by_identity_id: identityId,
            keys_shared: false,
            created_at: '2026-07-18T12:00:00.000Z',
            expires_at: '2026-07-19T12:00:00.000Z',
          },
        ]}
        suggestions={[]}
        onRefresh={vi.fn()}
        onInvite={vi.fn()}
        onAccept={vi.fn()}
        onShare={onShare}
        managedGrants={[]}
        onRevoke={vi.fn()}
        onSuggest={vi.fn()}
      />,
    )

    await user.click(
      screen.getByRole('button', { name: 'Share encrypted project' }),
    )
    expect(onShare).toHaveBeenCalledWith(identityId)
  })

  it('requests atomic key rotation when revoking a managed grant', async () => {
    const user = userEvent.setup()
    const resourceId = crypto.randomUUID()
    const grantId = crypto.randomUUID()
    const userId = crypto.randomUUID()
    const onRevoke = vi.fn().mockResolvedValue(undefined)
    render(
      <ProjectPeopleScreen
        invitations={[]}
        suggestions={[]}
        onRefresh={vi.fn()}
        onInvite={vi.fn()}
        onAccept={vi.fn()}
        onShare={vi.fn()}
        managedGrants={[
          {
            topicName: 'Private topic',
            resourceId,
            grant: {
              id: grantId,
              root_grant_id: grantId,
              user_id: userId,
              resource_id: resourceId,
              access_level: 'view',
              access_scope: 'full',
              origin: { type: 'direct' },
              granted_at: '2026-07-18T12:00:00.000Z',
              revoked_at: null,
            },
          },
        ]}
        onRevoke={onRevoke}
        onSuggest={vi.fn()}
      />,
    )

    await user.click(
      screen.getByRole('button', { name: 'Revoke and rotate keys' }),
    )
    expect(onRevoke).toHaveBeenCalledWith({
      resourceId,
      grantId,
      userId,
    })
  })
})
