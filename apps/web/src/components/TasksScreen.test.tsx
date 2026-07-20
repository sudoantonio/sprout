import {
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { ProjectItem, TaskListItem } from '../store/app-store'
import { TasksScreen } from './TasksScreen'

const projectId = crypto.randomUUID()
const listId = crypto.randomUUID()

const project: ProjectItem = {
  wire: {
    id: projectId,
    root_resource_id: crypto.randomUUID(),
    owner_identity_id: crypto.randomUUID(),
    encrypted_metadata_b64: 'ciphertext',
    key_epoch: 1,
    status: 'active',
    created_at: '2026-07-18T12:00:00.000Z',
    updated_at: '2026-07-18T12:00:00.000Z',
  },
  document: { schema: 1, name: 'Project' },
}

const taskList: TaskListItem = {
  wire: {
    id: listId,
    project_id: projectId,
    topic_id: crypto.randomUUID(),
    resource_node_id: crypto.randomUUID(),
    payload: {
      version: 1,
      algorithm: 'sprout-protocol-v1',
      key_id: crypto.randomUUID(),
      nonce_b64: 'AQ==',
      ciphertext_b64: 'Ag==',
    },
    payload_version: 1,
    key_epoch: 1,
    created_at: '2026-07-18T12:00:00.000Z',
    archived_at: null,
  },
  document: { schema: 1, name: 'Tasks' },
}

describe('task creation form', () => {
  it('submits exactly the selected semantic task type', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    render(
      <TasksScreen
        project={project}
        topics={[]}
        taskLists={[taskList]}
        tasks={[]}
        lockedTasks={[]}
        selectedListId={listId}
        publishedQuestionnaireVersions={[]}
        filter="open"
        loading={false}
        onSelectTopic={vi.fn()}
        onSelectList={vi.fn()}
        onSelectTask={vi.fn()}
        onFilter={vi.fn()}
        onCreateTopic={vi.fn()}
        onCreateList={vi.fn()}
        onCreateTask={onCreateTask}
        onUpdateTask={vi.fn()}
        onCompleteTask={vi.fn()}
        onCopyTask={vi.fn()}
      />,
    )

    await user.click(screen.getByText('Add encrypted task'))
    const title = screen.getByLabelText('Title')
    const form = title.closest('form')
    expect(form).not.toBeNull()
    await user.type(title, 'Release')
    await user.selectOptions(screen.getByLabelText('Task type'), 'deadline')
    expect(screen.queryByLabelText('Priority')).not.toBeInTheDocument()

    const localDueAt = '2026-07-20T09:30'
    fireEvent.change(screen.getByLabelText('Due'), {
      target: { value: localDueAt },
    })
    await user.click(
      within(form as HTMLFormElement).getByRole('button', {
        name: 'Encrypt and create',
      }),
    )

    expect(onCreateTask).toHaveBeenCalledWith({
      title: 'Release',
      questionnaireVersionId: undefined,
      taskKind: 'deadline',
      dueAt: new Date(localDueAt).toISOString(),
    })
  })
})
