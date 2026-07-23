import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { DecryptedTask } from '../domain/models'
import type {
  BoardMember,
  ProjectItem,
  TaskListItem,
  TopicItem,
} from '../store/app-store'
import { TasksScreen } from './TasksScreen'

const projectId = crypto.randomUUID()
const topicId = crypto.randomUUID()
const listId = crypto.randomUUID()
const memberId = crypto.randomUUID()
const otherMemberId = crypto.randomUUID()

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

const topic: TopicItem = {
  wire: {
    id: topicId,
    project_id: projectId,
    resource_node_id: crypto.randomUUID(),
    payload: null,
    key_epoch: 1,
    payload_version: 1,
    created_at: '2026-07-18T12:00:00.000Z',
    deleted_at: null,
  },
  document: { schema: 1, name: 'Impianti' },
}

const taskList: TaskListItem = {
  wire: {
    id: listId,
    project_id: projectId,
    topic_id: topicId,
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
  document: { schema: 1, name: 'Elena Russo' },
}

const otherList: TaskListItem = {
  wire: {
    ...taskList.wire,
    id: crypto.randomUUID(),
    topic_id: topicId,
  },
  document: { schema: 1, name: 'Mattina' },
}

const members: BoardMember[] = [
  { identityId: memberId, label: 'Elena Russo' },
  { identityId: otherMemberId, label: 'Lucia Bianchi' },
]

const makeTask = (
  title: string,
  list: string,
  assignee: string | null,
): DecryptedTask => ({
  wire: {
    id: crypto.randomUUID(),
    project_id: projectId,
    list_id: list,
    resource_node_id: crypto.randomUUID(),
    task_kind: 'priority',
    payload: null,
    selected_value_snapshot: null,
    key_epoch: 1,
    state: { state: 'open' },
    source_pretask_id: null,
    preset_assignment_id: null,
    copied_from_task_id: null,
    questionnaire_version_id: null,
    recurrence_series_id: null,
    occurrence_number: null,
    active_assignment_id: assignee ? crypto.randomUUID() : null,
    active_assignee_identity_id: assignee,
    created_at: '2026-07-18T12:00:00.000Z',
    payload_version: 1,
  },
  document: { schema: 1, title, priority: 'normal', notes: `${title} notes` },
})

const baseProps = {
  project,
  topics: [topic],
  taskLists: [taskList, otherList],
  tasks: [
    makeTask('Color test', listId, memberId),
    makeTask('Hidden task', otherList.wire.id, otherMemberId),
  ],
  lockedTasks: [],
  boardMembers: members,
  boardFocus: { type: 'generali' as const },
  selectedTopicId: topicId,
  selectedListId: listId,
  publishedQuestionnaireVersions: [],
  currentUserLabel: 'Admin Minerva',
  filter: 'open' as const,
  loading: false,
  onSelectFocus: vi.fn(),
  onSelectList: vi.fn(),
  onSelectTask: vi.fn(),
  onFilter: vi.fn(),
  onCreateTopic: vi.fn().mockResolvedValue(undefined),
  onCreateList: vi.fn().mockResolvedValue(undefined),
  onCreateTask: vi.fn().mockResolvedValue(undefined),
  onUpdateTask: vi.fn().mockResolvedValue(undefined),
  onCompleteTask: vi.fn().mockResolvedValue(undefined),
  onCopyTask: vi.fn().mockResolvedValue(undefined),
  userMenu: {
    userLabel: 'Admin Minerva',
    projects: [project],
    selectedProjectId: projectId,
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
  },
}

describe('board shell', () => {
  const sidebar = () =>
    within(screen.getByRole('complementary', { name: 'Board navigation' }))

  it('shows generali, members, topics, and creates a topic', async () => {
    const user = userEvent.setup()
    const onCreateTopic = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onCreateTopic={onCreateTopic} />)

    expect(sidebar().getByRole('button', { name: /Generali/i })).toBeTruthy()
    expect(sidebar().getByRole('button', { name: /Elena Russo/i })).toBeTruthy()
    expect(sidebar().getByRole('button', { name: /Impianti/i })).toBeTruthy()
    expect(screen.getByText('Admin Minerva')).toBeTruthy()

    await user.click(screen.getByRole('button', { name: /Nuova categoria/i }))
    await user.type(screen.getByLabelText('Topic name'), 'Ospiti')
    await user.click(screen.getByRole('button', { name: /^Crea$/i }))
    expect(onCreateTopic).toHaveBeenCalledWith('Ospiti')
  })

  it('collapses and expands the sidebar', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} />)

    expect(
      screen.getByRole('complementary', { name: 'Board navigation' }),
    ).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('Nuova categoria')).toHaveAttribute(
      'aria-hidden',
      'false',
    )

    await user.click(screen.getByRole('button', { name: /Riduci sidebar/i }))
    expect(
      screen.getByRole('complementary', { name: 'Board navigation' }),
    ).toHaveAttribute('aria-expanded', 'false')
    await waitFor(() => {
      expect(screen.getByText('Nuova categoria')).toHaveAttribute(
        'aria-hidden',
        'true',
      )
    })
    expect(sidebar().getByRole('button', { name: /Generali/i })).toBeTruthy()
    expect(
      sidebar().queryByRole('button', { name: /Espandi sidebar/i }),
    ).toBeNull()
    const board = screen.getByRole('region', { name: 'Board' })
    expect(
      within(board).getByRole('button', { name: /Espandi sidebar/i }),
    ).toBeTruthy()

    await user.click(
      within(board).getByRole('button', { name: /Espandi sidebar/i }),
    )
    expect(
      screen.getByRole('complementary', { name: 'Board navigation' }),
    ).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('Nuova categoria')).toHaveAttribute(
      'aria-hidden',
      'false',
    )
  })

  it('filters columns when selecting a member', () => {
    const onSelectFocus = vi.fn()
    const { rerender } = render(
      <TasksScreen {...baseProps} onSelectFocus={onSelectFocus} />,
    )

    fireEvent.click(sidebar().getByRole('button', { name: /Elena Russo/i }))
    expect(onSelectFocus).toHaveBeenCalledWith({
      type: 'member',
      identityId: memberId,
    })

    rerender(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'member', identityId: memberId }}
      />,
    )
    expect(screen.getByRole('listitem', { name: 'Elena Russo' })).toBeTruthy()
    expect(screen.queryByRole('listitem', { name: 'Mattina' })).toBeNull()
  })

  it('search hides non-matching task lists and tasks', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} />)

    expect(screen.getByText('Color test')).toBeTruthy()
    expect(screen.getByText('Hidden task')).toBeTruthy()

    await user.type(screen.getByLabelText('Cerca task e tasklist'), 'Color')
    expect(screen.getByText('Color test')).toBeTruthy()
    expect(screen.queryByText('Hidden task')).toBeNull()
  })

  it('creates a task list from the add column tab', async () => {
    const user = userEvent.setup()
    const onCreateList = vi.fn().mockResolvedValue(undefined)
    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'topic', topicId }}
        onCreateList={onCreateList}
      />,
    )

    const addColumn = screen.getByRole('listitem', { name: 'Nuova task list' })
    await user.click(
      within(addColumn).getByRole('button', { name: /Nuova task list/i }),
    )
    await user.type(screen.getByLabelText('Task list name'), 'Pomeriggio')
    await user.click(screen.getByRole('button', { name: /^Crea$/i }))

    expect(onCreateList).toHaveBeenCalledWith('Pomeriggio', topicId)
  })

  it('creates a task from a column add control', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onCreateTask={onCreateTask} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))

    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(dialog).getByLabelText('Titolo'), 'New board task')
    await user.click(
      within(dialog).getByRole('radio', { name: 'Alta' }),
    )
    await user.click(within(dialog).getByRole('button', { name: /^Crea$/i }))

    expect(onCreateTask).toHaveBeenCalledWith(
      expect.objectContaining({
        taskKind: 'priority',
        title: 'New board task',
        priority: 'high',
      }),
      listId,
    )
  })

  it('closes the create task modal on cancel', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    expect(screen.getByRole('dialog', { name: 'Nuovo task' })).toBeTruthy()

    await user.click(
      within(screen.getByRole('dialog', { name: 'Nuovo task' })).getByRole(
        'button',
        { name: /^Annulla$/i },
      ),
    )
    expect(screen.queryByRole('dialog', { name: 'Nuovo task' })).toBeNull()
  })

  it('does not show a permanent task detail column', () => {
    render(<TasksScreen {...baseProps} />)
    expect(screen.queryByText('Nessun task selezionato')).toBeNull()
    expect(screen.queryByRole('dialog', { name: 'Task detail' })).toBeNull()
  })

  it('opens task details in a drawer when a card is clicked', async () => {
    const user = userEvent.setup()
    const task = baseProps.tasks[0]
    const onSelectTask = vi.fn()
    const { rerender } = render(
      <TasksScreen {...baseProps} onSelectTask={onSelectTask} />,
    )

    await user.click(screen.getByRole('button', { name: /Color test/i }))
    expect(onSelectTask).toHaveBeenCalledWith(task.wire.id)

    rerender(
      <TasksScreen
        {...baseProps}
        selectedTaskId={task.wire.id}
        onSelectTask={onSelectTask}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    expect(within(drawer).getByRole('heading', { name: 'Color test' })).toBeTruthy()
    const notesSection = within(drawer)
      .getByRole('heading', { name: 'Notes' })
      .closest('section')
    expect(within(notesSection!).getByText('Color test notes')).toBeTruthy()
  })

  it('closes the task detail drawer from the close button', async () => {
    const user = userEvent.setup()
    const task = baseProps.tasks[0]
    const onSelectTask = vi.fn()
    render(
      <TasksScreen
        {...baseProps}
        selectedTaskId={task.wire.id}
        onSelectTask={onSelectTask}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Close task detail' }))
    expect(onSelectTask).toHaveBeenCalledWith(undefined)
  })

  it('changes task filter from the pill dropdown', async () => {
    const user = userEvent.setup()
    const onFilter = vi.fn()
    render(<TasksScreen {...baseProps} onFilter={onFilter} />)

    expect(
      screen.getByRole('button', { name: 'Filtra task: Aperti' }),
    ).toBeTruthy()

    await user.click(screen.getByRole('button', { name: 'Filtra task: Aperti' }))
    await user.click(screen.getByRole('menuitemradio', { name: 'Oggi' }))

    expect(onFilter).toHaveBeenCalledWith('today')
  })
})
