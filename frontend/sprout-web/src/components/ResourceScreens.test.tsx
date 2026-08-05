import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { AttachmentCollectionItemDto } from '../api/contracts'
import type { DecryptedTask } from '../domain/models'
import { AttachmentScreen, RetentionScreen } from './ResourceScreens'

describe('required attachment completion', () => {
  it('links the completed upload to the selected required snapshot', async () => {
    const user = userEvent.setup()
    const taskId = crypto.randomUUID()
    const requiredId = crypto.randomUUID()
    const task = {
      wire: { id: taskId },
      document: { schema: 1, title: 'Assigned task' },
    } as unknown as DecryptedTask
    const required = {
      id: requiredId,
      task_id: taskId,
      attachment_kind: 'task_required',
      blob_id: crypto.randomUUID(),
      state: { state: 'available', uploaded_at: new Date().toISOString() },
    } as unknown as AttachmentCollectionItemDto
    const onUpload = vi.fn().mockResolvedValue(undefined)
    render(
      <AttachmentScreen
        assigneeTasks={[task]}
        attachments={[required]}
        onRefresh={vi.fn().mockResolvedValue(undefined)}
        onUpload={onUpload}
        onResume={vi.fn()}
        onDownload={vi.fn()}
      />,
    )

    await user.selectOptions(
      screen.getByLabelText('Active assignment'),
      taskId,
    )
    await user.selectOptions(
      screen.getByLabelText('Required attachment being completed'),
      requiredId,
    )
    const file = new File(['completed'], 'completed.txt')
    await user.upload(screen.getByLabelText('Local file'), file)
    const submit = screen.getByRole('button', {
      name: 'Encrypt, persist, and upload',
    })
    await waitFor(() => expect(submit).toBeEnabled())
    fireEvent.submit(submit.closest('form') as HTMLFormElement)

    await waitFor(() =>
      expect(onUpload).toHaveBeenCalledWith(task, file, requiredId),
    )
  })
})

describe('retention delivery', () => {
  it('renders next-login warnings and available archives', () => {
    render(
      <RetentionScreen
        autoExport
        warnings={[
          {
            id: crypto.randomUUID(),
            project_id: crypto.randomUUID(),
            state: 'delivered',
            scheduled_at: new Date().toISOString(),
            created_at: new Date().toISOString(),
          },
        ]}
        archives={[
          {
            id: crypto.randomUUID(),
            project_id: crypto.randomUUID(),
            source_kind: 'task_completed',
            source_id: crypto.randomUUID(),
            state: 'succeeded',
            created_at: new Date().toISOString(),
            completed_at: new Date().toISOString(),
            source_purged_at: null,
            expires_at: null,
            downloaded_at: null,
          },
        ]}
        onToggle={vi.fn()}
        onRefresh={vi.fn()}
        onDownload={vi.fn()}
      />,
    )

    expect(screen.getByRole('status')).toHaveTextContent(
      '1 retained resource reached a deletion warning window',
    )
    expect(
      screen.getByRole('button', { name: 'Download ciphertext' }),
    ).toBeEnabled()
  })
})
