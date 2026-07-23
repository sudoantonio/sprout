import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { WorkspaceUserMenu } from './WorkspaceUserMenu'

const baseProps = {
  userLabel: 'Admin Minerva',
  projects: [],
  currentScreen: 'tasks' as const,
  conflictCount: 0,
  projectName: '',
  onProjectNameChange: vi.fn(),
  onSelectProject: vi.fn(),
  onCreateProject: vi.fn(),
  onNavigate: vi.fn(),
  onLogout: vi.fn(),
  appearance: 'system' as const,
  onAppearanceChange: vi.fn(),
}

describe('WorkspaceUserMenu', () => {
  it('shows appearance controls and reports appearance changes', async () => {
    const user = userEvent.setup()
    const onAppearanceChange = vi.fn()

    render(
      <WorkspaceUserMenu
        {...baseProps}
        variant="overview"
        onAppearanceChange={onAppearanceChange}
      />,
    )

    expect(screen.getByLabelText('Appearance, System')).toBeTruthy()

    await user.click(screen.getByLabelText('Appearance, System'))

    expect(screen.getByRole('menuitemradio', { name: 'System' })).toHaveAttribute(
      'aria-checked',
      'true',
    )

    await user.click(screen.getByRole('menuitemradio', { name: 'Dark' }))
    expect(onAppearanceChange).toHaveBeenCalledWith('dark')

    await user.click(screen.getByLabelText('Appearance, System'))

    await user.click(screen.getByRole('menuitemradio', { name: 'Tactical Light' }))
    expect(onAppearanceChange).toHaveBeenCalledWith('tactical-light')
  })
})
