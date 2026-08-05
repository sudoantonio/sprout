import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { PresetScreen } from './PresetScreen'

describe('preset template attachments', () => {
  it('passes each optional pretask file into materialization', async () => {
    const user = userEvent.setup()
    const onMaterialize = vi.fn().mockResolvedValue(undefined)
    const priority = new File(['priority'], 'priority.txt')
    const deadline = new File(['deadline'], 'deadline.txt')
    const recurring = new File(['recurring'], 'recurring.txt')
    render(
      <PresetScreen
        destinationReady
        onMaterialize={onMaterialize}
      />,
    )

    await user.upload(
      screen.getByLabelText('Priority template attachment (optional)'),
      priority,
    )
    await user.upload(
      screen.getByLabelText('Deadline template attachment (optional)'),
      deadline,
    )
    await user.upload(
      screen.getByLabelText('Recurring template attachment (optional)'),
      recurring,
    )
    await user.click(
      screen.getByRole('button', {
        name: 'Create, assign and materialize',
      }),
    )

    expect(onMaterialize).toHaveBeenCalledWith(
      expect.objectContaining({
        templateAttachments: { priority, deadline, recurring },
      }),
    )
  })
})
