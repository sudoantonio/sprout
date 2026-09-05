import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import type {
  DecryptedInfoDocument,
  DecryptedPreset,
  DecryptedTask,
  InfoDocumentContent,
} from '../domain/models'
import { startOfWeek } from '../domain/timeline'
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

const timelineDueAt = (dayOffset: number, hour: number): string => {
  const dueAt = startOfWeek(new Date())
  dueAt.setDate(dueAt.getDate() + dayOffset)
  dueAt.setHours(hour, 0, 0, 0)
  return dueAt.toISOString()
}

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

const infoRoot: DecryptedInfoDocument = {
  wire: {
    id: crypto.randomUUID(),
    project_id: projectId,
    topic_id: null,
    task_list_id: listId,
    parent_document_id: null,
    resource_node_id: taskList.wire.resource_node_id,
    payload: taskList.wire.payload!,
    key_epoch: 1,
    payload_version: 1,
    created_at: '2026-07-18T12:00:00.000Z',
    updated_at: '2026-07-18T12:00:00.000Z',
  },
  document: {
    schema: 1,
    blocks: [
      {
        id: crypto.randomUUID(),
        type: 'text',
        markdown: '# Informazioni\nhttps://sprout.test',
      },
    ],
  },
}

const members: BoardMember[] = [
  { identityId: memberId, label: 'Elena Russo' },
  { identityId: otherMemberId, label: 'Lucia Bianchi' },
]

const makeTask = (
  title: string,
  list: string,
  assignee: string | null,
  options?: {
    kind?: 'priority' | 'deadline' | 'recurring'
    dueAt?: string
    presetId?: string
    presetTemplateIndex?: number
    createdAt?: string
  },
): DecryptedTask => ({
  wire: {
    id: crypto.randomUUID(),
    project_id: projectId,
    list_id: list,
    resource_node_id: crypto.randomUUID(),
    task_kind: options?.kind ?? 'priority',
    payload: null,
    selected_value_snapshot: null,
    key_epoch: 1,
    state: { state: 'open' },
    source_pretask_id: null,
    preset_assignment_id: null,
    copied_from_task_id: null,
    questionnaire_version_id: null,
    recurrence_series_id:
      options?.kind === 'recurring' ? crypto.randomUUID() : null,
    occurrence_number: options?.kind === 'recurring' ? 1 : null,
    active_assignment_id: assignee ? crypto.randomUUID() : null,
    active_assignee_identity_id: assignee,
    created_at: options?.createdAt ?? '2026-07-18T12:00:00.000Z',
    payload_version: 1,
  },
  document: {
    schema: 1,
    title,
    priority: 'normal',
    notes: `${title} notes`,
    preset_id: options?.presetId,
    preset_template_index: options?.presetTemplateIndex,
    due_at: options?.dueAt,
    recurrence:
      options?.kind === 'recurring'
        ? { frequency: 'daily', interval: 1 }
        : undefined,
  },
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
  agents: [],
  agentsLoading: false,
  boardFocus: { type: 'generali' as const },
  boardViewMode: 'board' as const,
  selectedTopicId: topicId,
  selectedListId: listId,
  publishedQuestionnaireVersions: [],
  currentUserLabel: 'Admin Minerva',
  filter: 'open' as const,
  loading: false,
  onSelectFocus: vi.fn(),
  onBoardViewModeChange: vi.fn(),
  onSelectList: vi.fn(),
  onSelectTask: vi.fn(),
  onFilter: vi.fn(),
  onCreateTopic: vi.fn().mockResolvedValue(undefined),
  onRenameTopic: vi.fn().mockResolvedValue(undefined),
  onToggleTopicFavorite: vi.fn().mockResolvedValue(undefined),
  onDeleteTopic: vi.fn().mockResolvedValue(undefined),
  onCreateList: vi.fn().mockResolvedValue(undefined),
  onUpdateTaskList: vi.fn().mockResolvedValue(undefined),
  onLoadProjectInfo: vi.fn().mockResolvedValue([infoRoot]),
  onCreateProjectInfoDocument: vi.fn().mockResolvedValue(infoRoot),
  onLoadTopicInfo: vi.fn().mockResolvedValue([infoRoot]),
  onCreateTopicInfoDocument: vi.fn().mockResolvedValue(infoRoot),
  onLoadTaskListInfo: vi.fn().mockResolvedValue([infoRoot]),
  onCreateTaskListInfoDocument: vi.fn().mockResolvedValue(infoRoot),
  onUpdateInfoDocument: vi.fn().mockResolvedValue(infoRoot),
  onUploadInfoDocumentFile: vi.fn().mockResolvedValue({
    id: crypto.randomUUID(),
    type: 'file',
    blob_id: crypto.randomUUID(),
    file_name: 'documento.pdf',
    content_type: 'application/pdf',
    plaintext_size: 10,
  }),
  onReadInfoDocumentFile: vi.fn().mockResolvedValue(new Blob(['file'])),
  onDownloadInfoDocumentFile: vi.fn().mockResolvedValue(undefined),
  onCreateTask: vi.fn().mockResolvedValue(undefined),
  onUpdateTask: vi.fn().mockResolvedValue(undefined),
  onAssignTask: vi.fn().mockResolvedValue(undefined),
  onCompleteTask: vi.fn().mockResolvedValue(undefined),
  onCopyTask: vi.fn().mockResolvedValue(undefined),
  onInviteMember: vi.fn().mockResolvedValue(undefined),
  onUpdateMemberResponsibilities: vi.fn().mockResolvedValue(undefined),
  onRefreshAgents: vi.fn(),
  onProvisionAgent: vi.fn().mockResolvedValue({
    agent_id: crypto.randomUUID(),
    principal_identity_id: crypto.randomUUID(),
    runner_id: crypto.randomUUID(),
    runner_device_id: crypto.randomUUID(),
    bootstrap_token: 'bootstrap-test-token',
    bootstrap_expires_at: '2026-08-26T03:00:00.000Z',
    runner_state: 'pending_key' as const,
  }),
  taskAttachments: [],
  taskAttachmentLabels: {},
  onRefreshTaskAttachments: vi.fn().mockResolvedValue(undefined),
  onDownloadTaskAttachment: vi.fn().mockResolvedValue(undefined),
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

const openSlashMenu = (editor: HTMLElement) => {
  const emptyLine = document.createElement('p')
  emptyLine.innerHTML = '<br>'
  editor.append(emptyLine)

  const range = document.createRange()
  range.selectNodeContents(emptyLine)
  range.collapse(true)
  const selection = window.getSelection()
  selection?.removeAllRanges()
  selection?.addRange(range)

  fireEvent.keyDown(editor, { key: '/' })
}

const openSlashMenuBefore = (editor: HTMLElement, before: Element) => {
  const emptyLine = document.createElement('p')
  emptyLine.innerHTML = '<br>'
  editor.insertBefore(emptyLine, before)

  const range = document.createRange()
  range.selectNodeContents(emptyLine)
  range.collapse(true)
  const selection = window.getSelection()
  selection?.removeAllRanges()
  selection?.addRange(range)

  fireEvent.keyDown(editor, { key: '/' })
}

describe('board shell', () => {
  const sidebar = () =>
    within(screen.getByRole('complementary', { name: 'Board navigation' }))

  it('creates the first project from the empty state', async () => {
    const user = userEvent.setup()
    const onCreateProject = vi.fn()
    const ProjectCreationHarness = () => {
      const [projectName, setProjectName] = useState('')
      return (
        <TasksScreen
          {...baseProps}
          project={undefined}
          userMenu={{
            ...baseProps.userMenu,
            projects: [],
            selectedProjectId: undefined,
            projectName,
            onProjectNameChange: setProjectName,
            onCreateProject,
          }}
        />
      )
    }
    render(<ProjectCreationHarness />)

    await user.click(
      screen.getByRole('button', { name: /Progetto: Seleziona progetto/i }),
    )
    await user.click(screen.getByRole('menuitem', { name: /Nuovo progetto/i }))
    await user.type(screen.getByLabelText('Nome nuovo progetto'), 'Primo')
    await user.click(screen.getByRole('button', { name: /^Crea$/i }))

    expect(onCreateProject).toHaveBeenCalled()
  })

  it('shows generali, members, topics, and creates a topic', async () => {
    const user = userEvent.setup()
    const onCreateTopic = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onCreateTopic={onCreateTopic} />)

    expect(sidebar().getByRole('button', { name: /Generali/i })).toBeTruthy()
    expect(sidebar().getByRole('button', { name: /Elena Russo/i })).toBeTruthy()
    expect(sidebar().queryByText('Elena Russo')).toBeNull()
    expect(sidebar().getByRole('button', { name: /Impianti/i })).toBeTruthy()
    expect(screen.getByText('Admin Minerva')).toBeTruthy()
    expect(screen.getByRole('button', { name: /Progetto: Project/i })).toBeTruthy()

    await user.click(
      sidebar().getByRole('button', { name: /Nuova categoria/i }),
    )
    const topicInput = sidebar().getByLabelText('Topic name')
    expect(sidebar().queryByRole('button', { name: /^Crea$/i })).toBeNull()
    expect(sidebar().queryByRole('button', { name: /Annulla/i })).toBeNull()
    await user.type(topicInput, 'Ospiti')
    await user.click(
      sidebar().getByRole('button', { name: 'Conferma categoria' }),
    )
    expect(onCreateTopic).toHaveBeenCalledWith('Ospiti')
  })

  it('switches projects from the sidebar toolbar switcher', async () => {
    const user = userEvent.setup()
    const otherProjectId = crypto.randomUUID()
    const otherProject: ProjectItem = {
      wire: {
        id: otherProjectId,
        root_resource_id: crypto.randomUUID(),
        owner_identity_id: crypto.randomUUID(),
        encrypted_metadata_b64: 'ciphertext',
        key_epoch: 1,
        status: 'active',
        created_at: '2026-07-18T12:00:00.000Z',
        updated_at: '2026-07-18T12:00:00.000Z',
      },
      document: { schema: 1, name: 'Second Project' },
    }
    const onSelectProject = vi.fn()
    render(
      <TasksScreen
        {...baseProps}
        userMenu={{
          ...baseProps.userMenu,
          projects: [project, otherProject],
          onSelectProject,
        }}
      />,
    )

    await user.click(screen.getByRole('button', { name: /Progetto: Project/i }))
    await user.click(screen.getByRole('menuitemradio', { name: /Second Project/i }))
    expect(onSelectProject).toHaveBeenCalledWith(otherProjectId)
  })

  it('expands the Ask to AI composer with multiline content', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} />)

    await user.click(screen.getByRole('button', { name: 'Ask to AI' }))
    const composer = screen.getByLabelText('Messaggio per Ask to AI')
    Object.defineProperty(composer, 'scrollHeight', {
      configurable: true,
      value: 132,
    })

    fireEvent.change(composer, { target: { value: 'Testo su più righe' } })

    expect(composer).toHaveStyle({ height: '132px', overflowY: 'hidden' })
  })

  it('renders note URLs as links only in the open task detail', () => {
    const url = 'https://docs.google.com/spreadsheets/d/example-id-without-spaces'
    const linkedTask = makeTask('Riferimento', listId, memberId)
    linkedTask.document.notes = url
    const { rerender } = render(
      <TasksScreen
        {...baseProps}
        tasks={[linkedTask]}
      />,
    )

    expect(screen.queryByRole('link', { name: url })).toBeNull()

    rerender(
      <TasksScreen
        {...baseProps}
        tasks={[linkedTask]}
        selectedTaskId={linkedTask.wire.id}
      />,
    )

    const detail = screen.getByRole('dialog', { name: 'Task detail' })
    const link = within(detail).getByRole('link', { name: url })
    expect(link).toHaveAttribute('href', url)
    expect(link).toHaveAttribute('target', '_blank')
    expect(within(detail).getAllByText(url)).toHaveLength(1)
    expect(within(detail).queryByLabelText('Commento')).toBeNull()

    fireEvent.click(within(detail).getByRole('button', { name: 'Modifica note' }))
    expect(within(detail).getByLabelText('Commento')).toHaveValue(url)
  })

  it('creates a project from the sidebar toolbar switcher', async () => {
    const user = userEvent.setup()
    const onCreateProject = vi.fn()
    const ProjectCreationHarness = () => {
      const [projectName, setProjectName] = useState('')
      return (
        <TasksScreen
          {...baseProps}
          userMenu={{
            ...baseProps.userMenu,
            projectName,
            onProjectNameChange: setProjectName,
            onCreateProject,
          }}
        />
      )
    }
    render(<ProjectCreationHarness />)

    await user.click(screen.getByRole('button', { name: /Progetto: Project/i }))
    await user.click(screen.getByRole('menuitem', { name: /Nuovo progetto/i }))
    await user.type(screen.getByLabelText('Nome nuovo progetto'), 'Nuovo')
    await user.click(screen.getByRole('button', { name: /^Crea$/i }))
    expect(onCreateProject).toHaveBeenCalled()
  })

  it('invites a member from the members overview', async () => {
    const user = userEvent.setup()
    const onInviteMember = vi.fn().mockResolvedValue(undefined)
    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'members' }}
        boardViewMode="overview"
        onInviteMember={onInviteMember}
      />,
    )

    await user.click(screen.getByRole('button', { name: /Invita nuovo membro/i }))
    await user.type(screen.getByLabelText('Email membro'), 'lucia@example.com')
    await user.type(screen.getByLabelText('Nome membro'), 'Lucia Bianchi')
    await user.click(screen.getByRole('button', { name: /^Invita$/i }))
    expect(onInviteMember).toHaveBeenCalledWith({
      email: 'lucia@example.com',
      name: 'Lucia Bianchi',
      role: 'member',
    })
  })

  it('opens a minimal member detail with role and assigned tasks', async () => {
    const user = userEvent.setup()
    const onSelectTask = vi.fn()
    const assignedTask = makeTask('Controlla impianto', listId, memberId)
    const completedTask = makeTask('Rapporto concluso', listId, memberId)
    completedTask.wire.state = {
      state: 'completed',
      completed_by: memberId,
      completed_at: '2026-09-03T12:00:00.000Z',
    }
    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'members' }}
        boardViewMode="overview"
        boardMembers={[{
          identityId: memberId,
          label: 'Elena Russo',
          email: 'elena@example.test',
          role: 'admin',
          joinedAt: '2026-09-01T10:00:00.000Z',
        }]}
        tasks={[assignedTask, completedTask]}
        onSelectTask={onSelectTask}
      />,
    )

    await user.click(
      screen.getByRole('button', { name: 'Apri dettagli di Elena Russo' }),
    )

    const detail = screen.getByLabelText('Dettagli di Elena Russo')
    expect(screen.queryByRole('group', { name: 'Vista board' })).toBeNull()
    expect(
      screen.getByRole('button', { name: 'Torna alla board' }),
    ).toBeTruthy()
    expect(within(detail).getByText('elena@example.test')).toBeTruthy()
    expect(within(detail).getByText('Ruolo')).toBeTruthy()
    expect(within(detail).getByText('Amministratore')).toBeTruthy()
    expect(within(detail).queryByLabelText(/Responsabilità di/)).toBeNull()
    expect(within(detail).queryByText('Controlla impianto')).toBeNull()

    const memberNavigation = screen.getByLabelText('Vista membro')
    expect(within(memberNavigation).getByRole('button', { name: 'Torna alla board' })).toBeTruthy()
    expect(within(memberNavigation).getByRole('tab', { name: 'Info' })).toBeTruthy()
    await user.click(within(memberNavigation).getByRole('tab', { name: 'History' }))
    expect(within(detail).queryByText('1 aperti · 1 completati')).toBeNull()

    await user.click(
      within(detail).getByRole('button', { name: /Controlla impianto/i }),
    )
    expect(onSelectTask).toHaveBeenCalledWith(assignedTask.wire.id)

    await user.click(
      within(detail).getByRole('button', { name: 'Filtra task membro' }),
    )
    expect(within(detail).getByText('Board')).toBeTruthy()
    expect(within(detail).getByText('Tipologia')).toBeTruthy()
    expect(within(detail).getByText('Stato')).toBeTruthy()
    expect(within(detail).getByText('Data')).toBeTruthy()
    expect(within(detail).queryByText('Membro')).toBeNull()
    const boardCategory = within(detail).getByRole('button', { name: 'Board' })
    const dateCategory = within(detail).getByRole('button', { name: 'Data' })
    expect(boardCategory.getAttribute('aria-pressed')).toBe('false')
    expect(dateCategory.getAttribute('aria-pressed')).toBe('true')
    expect(boardCategory.querySelector('.board-filter-selection-circle')).toBeTruthy()
    expect(dateCategory.querySelector('.board-filter-selection-circle.selected')).toBeTruthy()
    await user.click(boardCategory)
    expect(boardCategory.getAttribute('aria-pressed')).toBe('true')
    expect(dateCategory.getAttribute('aria-pressed')).toBe('false')
    expect(
      detail.querySelector('.tasklist-history-day-label--violet'),
    ).toBeTruthy()
    await user.click(
      within(detail).getByRole('button', { name: 'Apri filtri Stato' }),
    )
    await user.click(
      within(detail).getByRole('menuitemcheckbox', { name: 'Completati' }),
    )
    expect(within(detail).queryByText('Controlla impianto')).toBeNull()
    expect(within(detail).getByText('Rapporto concluso')).toBeTruthy()

    await user.click(screen.getByRole('button', { name: 'Torna alla board' }))
    expect(screen.getByRole('heading', { name: 'Membri' })).toBeTruthy()
  })

  it('edits responsibilities for a non-administrator member', async () => {
    const user = userEvent.setup()
    const onUpdateMemberResponsibilities = vi.fn().mockResolvedValue(undefined)
    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'members' }}
        boardViewMode="overview"
        boardMembers={[{
          identityId: memberId,
          label: 'Elena Russo',
          email: 'elena@example.test',
          role: 'member',
          responsibilities: 'Controllo qualità',
        }]}
        onUpdateMemberResponsibilities={onUpdateMemberResponsibilities}
      />,
    )

    await user.click(
      screen.getByRole('button', { name: 'Apri dettagli di Elena Russo' }),
    )
    const input = screen.getByRole('textbox', {
      name: 'Responsabilità di Elena Russo',
    })
    expect(input).toHaveValue('Controllo qualità')
    await user.clear(input)
    await user.type(input, 'Verifica impianti{Enter}')

    await waitFor(() => {
      expect(onUpdateMemberResponsibilities).toHaveBeenCalledWith(
        memberId,
        'Verifica impianti',
      )
    })
  })

  it('shows at most three member icons and an overflow control', () => {
    const manyMembers = [
      { identityId: crypto.randomUUID(), label: 'Elena Russo' },
      { identityId: crypto.randomUUID(), label: 'Lucia Bianchi' },
      { identityId: crypto.randomUUID(), label: 'Marco Verdi' },
      { identityId: crypto.randomUUID(), label: 'Sara Neri' },
    ]
    render(<TasksScreen {...baseProps} boardMembers={manyMembers} />)

    expect(sidebar().getByRole('button', { name: 'Elena Russo' })).toBeTruthy()
    expect(sidebar().getByRole('button', { name: 'Lucia Bianchi' })).toBeTruthy()
    expect(sidebar().getByRole('button', { name: 'Marco Verdi' })).toBeTruthy()
    expect(sidebar().queryByRole('button', { name: 'Sara Neri' })).toBeNull()
    expect(
      sidebar().getByRole('button', { name: /Altri 1 membri/i }),
    ).toBeTruthy()
  })

  it('shows fixed category views in the toolbar', () => {
    render(<TasksScreen {...baseProps} />)

    expect(
      sidebar()
        .getByRole('button', { name: 'Generali' })
        .getAttribute('aria-current'),
    ).toBe('page')
    const viewSwitch = within(screen.getByRole('group', { name: 'Vista board' }))
    expect(
      screen
        .getByRole('group', { name: 'Vista board' })
        .closest('.board-view-navigation'),
    ).toBeTruthy()
    expect(viewSwitch.getByRole('button', { name: /Overview/i })).toBeTruthy()
    expect(viewSwitch.getByRole('button', { name: /Board/i })).toBeTruthy()
    expect(viewSwitch.getByRole('button', { name: /Timeline/i })).toBeTruthy()
    expect(viewSwitch.getByRole('button', { name: /History/i })).toBeTruthy()
  })

  it('loads the encrypted topic document in category Overview', async () => {
    const onLoadTopicInfo = vi.fn().mockResolvedValue([infoRoot])
    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'topic', topicId }}
        boardViewMode="overview"
        onLoadTopicInfo={onLoadTopicInfo}
      />,
    )

    expect(document.querySelector('.board-overview-scroll')).toBeTruthy()
    await waitFor(() => expect(onLoadTopicInfo).toHaveBeenCalledWith(topic))
    expect(screen.getByRole('textbox', { name: 'Testo info in Markdown' })).toBeTruthy()
  })

  it('edits and saves the encrypted project document in Generali Overview', async () => {
    const user = userEvent.setup()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
    }
    const onLoadProjectInfo = vi.fn().mockResolvedValue([projectRoot])
    const onUpdateInfoDocument = vi.fn(
      async (
        _current: DecryptedInfoDocument,
        document: InfoDocumentContent,
      ) => ({
        ...projectRoot,
        wire: { ...projectRoot.wire, payload_version: 2 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={onLoadProjectInfo}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    await waitFor(() => expect(onLoadProjectInfo).toHaveBeenCalledWith(project))
    const editor = screen.getByRole('textbox', {
      name: 'Testo info in Markdown',
    })
    editor.innerHTML = '<h1>Scopo del progetto</h1>'
    fireEvent.input(editor)
    fireEvent.blur(editor)

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalled())
    expect(onUpdateInfoDocument.mock.calls[0]?.[1]).toEqual(
      expect.objectContaining({
        blocks: expect.arrayContaining([
          expect.objectContaining({
            type: 'text',
            markdown: '# Scopo del progetto',
          }),
        ]),
      }),
    )

    const sourceHeading = editor.querySelector('h1')
    openSlashMenu(editor)
    await user.click(await screen.findByRole('menuitem', { name: /Titolo medio/ }))
    const insertedHeading = editor.querySelector('h2')
    expect(insertedHeading).toBeTruthy()
    expect(insertedHeading?.textContent).toBe('')
    expect(editor.querySelector('h1')).toBe(sourceHeading)
    expect(document.activeElement).toBe(editor)
    expect(insertedHeading?.contains(window.getSelection()?.anchorNode ?? null)).toBe(true)
  })

  it('keeps exactly one empty trailing prompt while writing consecutive lines', async () => {
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{ id: crypto.randomUUID(), type: 'text', markdown: 'Prima' }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    const writeIntoPrompt = (text: string, outsideHelperSpan = false) => {
      const prompt = editor.querySelector<HTMLElement>(':scope > [data-overview-prompt]')!
      const content = prompt.querySelector<HTMLElement>('[data-task-text]') ?? prompt
      if (outsideHelperSpan) prompt.append(document.createTextNode(text))
      else content.textContent = text
      fireEvent.input(editor)
    }

    writeIntoPrompt('Seconda', true)
    expect(editor.querySelectorAll(':scope > [data-overview-prompt]')).toHaveLength(1)
    expect(editor.querySelector('[data-overview-prompt]')?.textContent).toBe('')
    writeIntoPrompt('Terza')
    expect(editor.querySelectorAll(':scope > [data-overview-prompt]')).toHaveLength(1)
    expect(editor.querySelector('[data-overview-prompt]')?.textContent).toBe('')

    fireEvent.blur(editor)
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalled())
    expect(onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks).toEqual([
      expect.objectContaining({
        type: 'text',
        markdown: 'Prima\nSeconda\nTerza',
      }),
    ])
  })

  it('adds and edits a resizable Markdown table in Overview', async () => {
    const user = userEvent.setup()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{ id: crypto.randomUUID(), type: 'text', markdown: '' }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (_current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...projectRoot,
        wire: { ...projectRoot.wire, payload_version: projectRoot.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    openSlashMenu(editor)
    await user.click(await screen.findByRole('menuitem', { name: 'Tabella' }))

    const table = editor.querySelector('table')!
    expect(table.querySelectorAll('thead th')).toHaveLength(2)
    expect(table.querySelectorAll('tbody tr')).toHaveLength(1)
    expect(table.closest('.tasklist-info-overview-table-scroll')).toBeTruthy()

    await user.click(screen.getByRole('button', { name: 'Aggiungi riga' }))
    expect(table.querySelectorAll('tbody tr')).toHaveLength(2)
    await user.click(screen.getByRole('button', { name: 'Aggiungi colonna' }))
    expect(table.querySelectorAll('thead th')).toHaveLength(3)
    expect(table.querySelectorAll('tbody tr')[0]?.querySelectorAll('td')).toHaveLength(3)

    const firstCell = table.querySelector<HTMLElement>('[data-table-cell]')!
    firstCell.textContent = 'Attività'
    fireEvent.input(firstCell)
    fireEvent.keyDown(firstCell, { key: 'a', ctrlKey: true })
    expect(window.getSelection()?.toString()).toBe('Attività')
    fireEvent.paste(firstCell, {
      clipboardData: { getData: () => 'Nuovo\nvalore' },
    })
    expect(firstCell).toHaveTextContent('Nuovo valore')
    const firstResize = screen.getByRole('separator', { name: 'Ridimensiona colonna 1' })
    fireEvent.keyDown(firstResize, { key: 'ArrowRight' })

    await waitFor(() => {
      const markdown = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks
        .find((block) => block.type === 'text')?.markdown
      expect(markdown).toContain('<!-- sprout-table-widths:196,180,180 -->')
      expect(markdown).toContain('| Nuovo valore | Colonna 2 | Colonna 3 |')
      expect(markdown?.match(/^\|  \|  \|  \|$/gm)).toHaveLength(2)
    })

    const bodyCell = table.querySelector<HTMLElement>('tbody [data-table-cell]')!
    fireEvent.contextMenu(bodyCell, { clientX: 120, clientY: 140 })
    const contextMenu = screen.getByRole('menu', { name: 'Azioni tabella' })
    expect(within(contextMenu).getByRole('menuitem', { name: 'Elimina riga' })).toBeEnabled()
    expect(within(contextMenu).getByRole('menuitem', { name: 'Elimina colonna' })).toBeEnabled()
    await user.click(
      within(contextMenu).getByRole('menuitem', { name: 'Aggiungi riga sotto' }),
    )
    expect(table.querySelectorAll('tbody tr')).toHaveLength(3)

    fireEvent.contextMenu(bodyCell, { clientX: 120, clientY: 140 })
    await user.click(screen.getByRole('menuitem', { name: 'Elimina riga' }))
    expect(table.querySelectorAll('tbody tr')).toHaveLength(2)

    const thirdHeader = table.querySelectorAll<HTMLElement>('thead [data-table-cell]')[2]!
    fireEvent.contextMenu(thirdHeader, { clientX: 160, clientY: 140 })
    await user.click(screen.getByRole('menuitem', { name: 'Elimina colonna' }))
    expect(table.querySelectorAll('thead th')).toHaveLength(2)

    await waitFor(() => {
      const markdown = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks
        .find((block) => block.type === 'text')?.markdown
      expect(markdown).toContain('<!-- sprout-table-widths:196,180 -->')
      expect(markdown).toContain('| Nuovo valore | Colonna 2 |')
      expect(markdown?.match(/^\|  \|  \|$/gm)).toHaveLength(2)
    })
  })

  it('renders and preserves inline Markdown formatting inside table cells', async () => {
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{
          id: crypto.randomUUID(),
          type: 'text',
          markdown: [
            '<!-- sprout-table-widths:180 -->',
            '| **Grassetto** *corsivo* ~~barrato~~ <u>sottolineato</u> `codice` [link](https://example.com) |',
            '| --- |',
            '| contenuto |',
          ].join('\n'),
        }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (_current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...projectRoot,
        wire: { ...projectRoot.wire, payload_version: projectRoot.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    const cell = editor.querySelector<HTMLElement>('[data-table-cell]')!
    expect(cell.querySelector('strong')).toHaveTextContent('Grassetto')
    expect(cell.querySelector('em')).toHaveTextContent('corsivo')
    expect(cell.querySelector('s')).toHaveTextContent('barrato')
    expect(cell.querySelector('u')).toHaveTextContent('sottolineato')
    expect(cell.querySelector('code')).toHaveTextContent('codice')
    expect(cell.querySelector('a')).toHaveAttribute('href', 'https://example.com')

    const text = cell.querySelector('strong')?.firstChild
    expect(text).toBeTruthy()
    const range = document.createRange()
    range.selectNodeContents(text!)
    Object.defineProperty(range, 'getBoundingClientRect', {
      configurable: true,
      value: () => ({
        x: 320, y: 240, top: 240, right: 400, bottom: 264, left: 320,
        width: 80, height: 24, toJSON: () => ({}),
      }),
    })
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(range)
    fireEvent.mouseUp(cell)

    const toolbar = await screen.findByRole('toolbar', { name: 'Formattazione testo' })
    expect(within(toolbar).getByTitle('Grassetto')).toBeVisible()
    expect(within(toolbar).getByTitle('Corsivo')).toBeVisible()
    expect(within(toolbar).getByTitle('Barrato')).toBeVisible()
    expect(within(toolbar).getByTitle('Sottolineato')).toBeVisible()
    expect(within(toolbar).getByTitle('Link')).toBeVisible()
    expect(within(toolbar).getByTitle('Codice')).toBeVisible()
    expect(within(toolbar).queryByTitle('Titolo piccolo')).not.toBeInTheDocument()
    expect(within(toolbar).queryByTitle('Citazione')).not.toBeInTheDocument()

    fireEvent.input(cell)
    fireEvent.blur(cell)
    await waitFor(() => {
      const markdown = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks
        .find((block) => block.type === 'text')?.markdown
      expect(markdown).toContain(
        '| **Grassetto** *corsivo* ~~barrato~~ <u>sottolineato</u> `codice` [link](https://example.com) |',
      )
    })
  })

  it('moves a Markdown table with its external drag handle', async () => {
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{
          id: crypto.randomUUID(),
          type: 'text',
          markdown: [
            'Prima',
            '<!-- sprout-table-widths:180 -->',
            '| Colonna |',
            '| --- |',
            '| Valore |',
            'Dopo',
          ].join('\n'),
        }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (_current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...projectRoot,
        wire: { ...projectRoot.wire, payload_version: projectRoot.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    const handle = screen.getByRole('button', { name: 'Sposta tabella' })
    expect(handle).toHaveAttribute('draggable', 'true')
    expect(handle).toHaveClass('tasklist-info-overview-table-drag-handle')

    const canvas = editor.parentElement!
    const rect = (top: number, bottom: number) => ({
      x: 0,
      y: top,
      top,
      right: 600,
      bottom,
      left: 0,
      width: 600,
      height: bottom - top,
      toJSON: () => ({}),
    })
    vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue(rect(80, 260))
    Object.defineProperty(canvas, 'offsetHeight', { configurable: true, value: 180 })
    vi.spyOn(editor.children[0]!, 'getBoundingClientRect').mockReturnValue(rect(100, 120))
    vi.spyOn(editor.children[1]!, 'getBoundingClientRect').mockReturnValue(rect(130, 190))
    vi.spyOn(editor.children[2]!, 'getBoundingClientRect').mockReturnValue(rect(200, 220))
    vi.spyOn(editor.children[3]!, 'getBoundingClientRect').mockReturnValue(rect(230, 250))

    const transferred: Record<string, string> = {}
    const dataTransfer = {
      effectAllowed: 'none',
      setData: vi.fn((type: string, value: string) => { transferred[type] = value }),
      getData: vi.fn((type: string) => transferred[type] ?? ''),
    }
    fireEvent.dragStart(handle, { dataTransfer })
    const dragOver = new Event('dragover', { bubbles: true, cancelable: true })
    Object.defineProperties(dragOver, {
      clientY: { value: 96 },
      dataTransfer: { value: dataTransfer },
    })
    fireEvent(editor, dragOver)
    const drop = new Event('drop', { bubbles: true, cancelable: true })
    Object.defineProperties(drop, {
      clientY: { value: 96 },
      dataTransfer: { value: dataTransfer },
    })
    fireEvent(editor, drop)

    await waitFor(() => {
      const markdown = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks
        .find((block) => block.type === 'text')?.markdown
      expect(markdown).toMatch(/^<!-- sprout-table-widths:180 -->/)
      expect(markdown).toContain('| Valore |\nPrima\nDopo')
    })
  })

  it('renders the text formatting toolbar above the document clipping layers', async () => {
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{ id: crypto.randomUUID(), type: 'text', markdown: 'Testo selezionato' }],
      },
    }

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    const text = editor.querySelector('[data-task-text]')?.firstChild
    expect(text).toBeTruthy()
    const range = document.createRange()
    range.selectNodeContents(text!)
    Object.defineProperty(range, 'getBoundingClientRect', {
      configurable: true,
      value: () => ({
        x: 320, y: 240, top: 240, right: 440, bottom: 264, left: 320,
        width: 120, height: 24, toJSON: () => ({}),
      }),
    })
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(range)
    fireEvent.mouseUp(editor)

    const toolbar = await screen.findByRole('toolbar', { name: 'Formattazione testo' })
    expect(toolbar).toHaveClass('tasklist-info-text-format-toolbar--portal')
    expect(screen.getByTestId('overview-block-flow')).not.toContainElement(toolbar)
    expect(document.body).toContainElement(toolbar)
  })

  it('shows an editable writing prompt after a final attachment', async () => {
    const firstTextId = crypto.randomUUID()
    const pageId = crypto.randomUUID()
    const imageId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [
          { id: firstTextId, type: 'text', markdown: 'Prima del documento' },
          { id: pageId, type: 'document', document_id: crypto.randomUUID(), title: 'Pagina' },
          { id: imageId, type: 'file', blob_id: crypto.randomUUID(), file_name: 'foto.png', content_type: 'image/png', plaintext_size: 10 },
        ],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const flow = await screen.findByTestId('overview-block-flow')
    expect(
      await screen.findByRole('button', { name: 'Scarica immagine' }),
    ).toHaveClass('tasklist-info-image-download')
    const prompts = flow.querySelectorAll('[data-overview-prompt]')
    expect(prompts).toHaveLength(1)
    const trailingTextBlock = prompts[0]?.closest<HTMLElement>('[data-block-id]')
    expect(trailingTextBlock).toBeTruthy()
    expect(prompts[0]?.getAttribute('data-placeholder')).toBe('Scrivi una nota o usa / per aggiungere contenuti')
    expect(flow.querySelector(`[data-block-id="${firstTextId}"] [data-overview-prompt]`)).toBeNull()
    expect(flow.querySelector(`[data-block-id="${firstTextId}"] [data-show-prompt]`)?.getAttribute('data-show-prompt')).toBe('false')
    expect(Array.from(flow.children).map((element) => element.getAttribute('data-block-id'))).toEqual([
      firstTextId,
      pageId,
      imageId,
      trailingTextBlock?.getAttribute('data-block-id') ?? null,
    ])

    const editor = trailingTextBlock?.querySelector<HTMLElement>('[role="textbox"]')
    const promptText = prompts[0]?.querySelector<HTMLElement>('[data-task-text]')
    expect(editor).toBeTruthy()
    expect(promptText).toBeTruthy()

    const imageTrigger = await screen.findByRole('button', { name: 'Seleziona foto.png' })
    fireEvent.click(imageTrigger)
    const imageBlock = flow.querySelector<HTMLElement>(`[data-block-id="${imageId}"]`)
    await waitFor(() => expect(imageBlock).toHaveClass('is-selected'))
    expect(document.activeElement).toBe(imageBlock)
    fireEvent.keyDown(imageBlock!, { key: 'Enter' })
    await waitFor(() => expect(document.activeElement).toBe(editor))

    promptText!.textContent = 'Dopo immagine'
    fireEvent.input(editor!)
    fireEvent.blur(editor!)

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalled())
    expect(onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks.at(-1)).toMatchObject({
      type: 'text',
      markdown: 'Dopo immagine',
    })

    fireEvent.click(await screen.findByRole('button', { name: 'Seleziona foto.png' }))
    await waitFor(() => expect(imageBlock).toHaveClass('is-selected'))
    fireEvent.keyDown(imageBlock!, { key: 'Delete' })
    await waitFor(() => {
      const blocks = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks ?? []
      expect(blocks.some((block) => block.id === imageId)).toBe(false)
    })
  })

  it('groups an unreadable encrypted image with its delete action', async () => {
    const imageId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{
          id: imageId,
          type: 'file',
          blob_id: crypto.randomUUID(),
          file_name: 'cifrata.png',
          content_type: 'image/png',
          plaintext_size: 10,
        }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onReadInfoDocumentFile={vi.fn().mockRejectedValue(new Error('missing key'))}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const placeholder = await screen.findByText('Immagine cifrata')
    const frame = placeholder.closest<HTMLElement>('.tasklist-info-image-frame')
    const remove = screen.getByRole('button', { name: 'Elimina cifrata.png' })
    expect(frame).toHaveClass('is-encrypted')
    expect(frame).toContainElement(remove)
    expect(screen.queryByRole('separator', { name: 'Ridimensiona cifrata.png' })).toBeNull()

    fireEvent.click(remove)
    await waitFor(() => {
      const blocks = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks ?? []
      expect(blocks.some((block) => block.id === imageId)).toBe(false)
    })
  })

  it('hides the trailing writing prompt inside a closed collapse', async () => {
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [
          { id: crypto.randomUUID(), type: 'text', markdown: ':::collapse[closed] Capitolo chiuso' },
          { id: crypto.randomUUID(), type: 'file', blob_id: crypto.randomUUID(), file_name: 'nascosta.png', content_type: 'image/png', plaintext_size: 10 },
        ],
      },
    }

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
      />,
    )

    const flow = await screen.findByTestId('overview-block-flow')
    const prompt = flow.querySelector<HTMLElement>('[data-overview-prompt]')
    expect(prompt).toBeTruthy()
    expect(prompt?.closest('[data-block-id]')).toHaveAttribute('hidden')
    expect(screen.queryByRole('button', { name: 'Seleziona nascosta.png' })).toBeNull()
  })

  it('inserts a new writing row immediately after a selected image', async () => {
    const beforeId = crypto.randomUUID()
    const imageId = crypto.randomUUID()
    const afterId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [
          { id: beforeId, type: 'text', markdown: 'Prima' },
          { id: imageId, type: 'file', blob_id: crypto.randomUUID(), file_name: 'centrale.png', content_type: 'image/png', plaintext_size: 10 },
          { id: afterId, type: 'text', markdown: 'Testo già presente dopo immagine' },
        ],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const flow = await screen.findByTestId('overview-block-flow')
    fireEvent.click(await screen.findByRole('button', { name: 'Seleziona centrale.png' }))
    const imageBlock = flow.querySelector<HTMLElement>(`[data-block-id="${imageId}"]`)!
    fireEvent.keyDown(imageBlock, { key: 'Enter' })

    await waitFor(() => expect(flow.children).toHaveLength(4))
    const insertedBlock = flow.children[2] as HTMLElement
    expect(insertedBlock.getAttribute('data-block-id')).not.toBe(afterId)
    const insertedEditor = insertedBlock.querySelector<HTMLElement>('[role="textbox"]')!
    await waitFor(() => expect(document.activeElement).toBe(insertedEditor))

    insertedEditor.innerHTML = '<p><span data-task-text>Riga subito sotto</span></p>'
    fireEvent.input(insertedEditor)
    fireEvent.blur(insertedEditor)

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalled())
    expect(onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks.map((block) => (
      block.type === 'text' ? block.markdown : block.id
    ))).toEqual([
      'Prima',
      imageId,
      'Riga subito sotto',
      'Testo già presente dopo immagine',
    ])
  })

  it('rolls back the prepared text split when an image upload fails', async () => {
    const user = userEvent.setup()
    const textId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{ id: textId, type: 'text', markdown: 'Prima\nDopo' }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
        onUploadInfoDocumentFile={vi.fn().mockRejectedValue(new Error('upload failed'))}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    openSlashMenuBefore(editor, editor.children[1]!)
    await user.click(await screen.findByRole('menuitem', { name: 'Immagine' }))
    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]')!
    fireEvent.change(fileInput, {
      target: { files: [new File(['test'], 'broken.png', { type: 'image/png' })] },
    })

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(2))
    expect(onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks).toEqual([
      { id: textId, type: 'text', markdown: 'Prima\nDopo' },
    ])
    await waitFor(() => {
      expect(screen.getAllByRole('textbox', { name: /Testo info in Markdown/ })).toHaveLength(1)
    })
  })

  it('rolls back the prepared text split when the file picker is cancelled', async () => {
    const user = userEvent.setup()
    const textId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{ id: textId, type: 'text', markdown: 'Prima\nDopo' }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )
    const onUploadInfoDocumentFile = vi.fn()

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
        onUploadInfoDocumentFile={onUploadInfoDocumentFile}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    openSlashMenuBefore(editor, editor.children[1]!)
    await user.click(await screen.findByRole('menuitem', { name: 'File' }))
    await waitFor(() => {
      expect(screen.getAllByRole('textbox', { name: /Testo info in Markdown/ })).toHaveLength(2)
    })
    const splitEditors = screen.getAllByRole('textbox', { name: /Testo info in Markdown/ })
    splitEditors[0]!.innerHTML = '<p>Prima modificata</p>'
    fireEvent.input(splitEditors[0]!)
    const currentSplitEditors = screen.getAllByRole('textbox', { name: /Testo info in Markdown/ })
    currentSplitEditors[1]!.innerHTML = '<p>Dopo modificato</p>'
    fireEvent.input(currentSplitEditors[1]!)
    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]')!
    fireEvent(fileInput, new Event('cancel', { bubbles: true }))

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(2))
    expect(onUploadInfoDocumentFile).not.toHaveBeenCalled()
    expect(onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks).toEqual([
      { id: textId, type: 'text', markdown: 'Prima modificata\nDopo modificato' },
    ])
  })

  it('inserts an image at a focused row without duplicating the following text', async () => {
    const user = userEvent.setup()
    const textId = crypto.randomUUID()
    const imageId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{ id: textId, type: 'text', markdown: 'Sopra\nSotto' }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )
    const onUploadInfoDocumentFile = vi.fn().mockResolvedValue({
      id: imageId,
      type: 'file',
      blob_id: crypto.randomUUID(),
      file_name: 'in-mezzo.png',
      content_type: 'image/png',
      plaintext_size: 4,
    })

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
        onUploadInfoDocumentFile={onUploadInfoDocumentFile}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    editor.focus()
    expect(document.activeElement).toBe(editor)
    openSlashMenuBefore(editor, editor.children[1]!)
    await user.click(await screen.findByRole('menuitem', { name: 'Immagine' }))

    await waitFor(() => {
      expect(editor).toHaveTextContent('Sopra')
      expect(editor).not.toHaveTextContent('Sotto')
    })

    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]')!
    fireEvent.change(fileInput, {
      target: { files: [new File(['image'], 'in-mezzo.png', { type: 'image/png' })] },
    })
    await waitFor(() => expect(onUploadInfoDocumentFile).toHaveBeenCalled())
    await screen.findByRole('button', { name: 'Sposta in-mezzo.png' })
    fireEvent.blur(editor)

    await waitFor(() => {
      const blocks = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks ?? []
      expect(blocks.map((block) => block.type === 'text' ? block.markdown : block.id)).toEqual([
        'Sopra',
        imageId,
        'Sotto',
      ])
      expect(blocks.filter((block) => (
        block.type === 'text' && block.markdown.includes('Sotto')
      ))).toHaveLength(1)
    })
  })

  it('moves an image below bullet and collapse row borders without duplicating text', async () => {
    const textId = crypto.randomUUID()
    const imageId = crypto.randomUUID()
    const trailingTextId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [
          { id: textId, type: 'text', markdown: 'Uno\n- Due\n:::collapse Tre' },
          {
            id: imageId,
            type: 'file',
            blob_id: crypto.randomUUID(),
            file_name: 'da-spostare.png',
            content_type: 'image/png',
            plaintext_size: 4,
          },
          { id: trailingTextId, type: 'text', markdown: 'Quattro' },
        ],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    const canvas = editor.parentElement!
    const rect = (top: number, bottom: number) => ({
      x: 0,
      y: top,
      top,
      right: 600,
      bottom,
      left: 0,
      width: 600,
      height: bottom - top,
      toJSON: () => ({}),
    })
    vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue(rect(80, 220))
    Object.defineProperty(canvas, 'offsetHeight', { configurable: true, value: 175 })
    vi.spyOn(editor, 'getBoundingClientRect').mockReturnValue(rect(96, 204))
    vi.spyOn(editor.children[0]!, 'getBoundingClientRect').mockReturnValue(rect(100, 120))
    vi.spyOn(editor.children[1]!, 'getBoundingClientRect').mockReturnValue(rect(140, 160))
    vi.spyOn(editor.children[2]!, 'getBoundingClientRect').mockReturnValue(rect(180, 200))

    editor.focus()
    const transferred: Record<string, string> = {}
    const dataTransfer = {
      effectAllowed: 'none',
      setData: vi.fn((type: string, value: string) => { transferred[type] = value }),
      getData: vi.fn((type: string) => transferred[type] ?? ''),
    }
    fireEvent.dragStart(screen.getByRole('button', { name: 'Sposta da-spostare.png' }), {
      dataTransfer,
    })

    // The event deliberately targets editor whitespace just below the bullet row.
    const dragOver = new Event('dragover', { bubbles: true, cancelable: true })
    Object.defineProperties(dragOver, {
      clientY: { value: 165 },
      dataTransfer: { value: dataTransfer },
    })
    fireEvent(editor, dragOver)
    expect(editor).toHaveClass('is-drop-target')
    expect(canvas.querySelector('.tasklist-info-text-drop-line')).toHaveStyle({ top: '100px' })
    const drop = new Event('drop', { bubbles: true, cancelable: true })
    Object.defineProperties(drop, {
      clientY: { value: 165 },
      dataTransfer: { value: dataTransfer },
    })
    fireEvent(editor, drop)
    expect(editor).not.toHaveClass('is-drop-target')

    await waitFor(() => {
      expect(editor).toHaveTextContent('Uno')
      expect(editor).toHaveTextContent('Due')
      expect(editor).not.toHaveTextContent('Tre')
    })
    fireEvent.blur(editor)

    await waitFor(() => {
      const blocks = onUpdateInfoDocument.mock.calls.at(-1)?.[1].blocks ?? []
      expect(blocks.map((block) => block.type === 'text' ? block.markdown : block.id)).toEqual([
        'Uno\n- Due',
        imageId,
        ':::collapse Tre\nQuattro',
      ])
      expect(blocks.filter((block) => block.id === imageId)).toHaveLength(1)
    })
  })

  it('supports interactive Overview blocks and separate attachment commands', async () => {
    const user = userEvent.setup()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{
          id: crypto.randomUUID(),
          type: 'text',
          markdown: '- Verifica allegati',
        }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (_current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...projectRoot,
        document,
      }),
    )
    const onUploadInfoDocumentFile = vi.fn().mockResolvedValue({
      id: crypto.randomUUID(),
      type: 'file',
      blob_id: crypto.randomUUID(),
      file_name: 'schema.png',
      content_type: 'image/png',
      plaintext_size: 4,
    })

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
        onUploadInfoDocumentFile={onUploadInfoDocumentFile}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    expect(editor).toHaveTextContent('Verifica allegati')

    openSlashMenu(editor)
    expect(await screen.findByRole('menuitem', { name: 'Immagine' })).toBeTruthy()
    expect(screen.getByRole('menuitem', { name: 'File' })).toBeTruthy()
    await user.click(screen.getByRole('menuitem', { name: 'Immagine' }))
    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]')
    expect(fileInput?.accept).toBe('image/*')
    fireEvent.change(fileInput!, {
      target: { files: [new File(['test'], 'schema.png', { type: 'image/png' })] },
    })
    await waitFor(() => expect(onUploadInfoDocumentFile).toHaveBeenCalled())
    const resizeHandle = await screen.findByRole('separator', { name: 'Ridimensiona schema.png' })
    const imageFrame = resizeHandle.parentElement!
    const imageFigure = imageFrame.parentElement!
    Object.defineProperty(imageFrame, 'offsetWidth', { configurable: true, value: 400 })
    Object.defineProperty(imageFigure, 'clientWidth', { configurable: true, value: 800 })
    fireEvent.keyDown(resizeHandle, { key: 'ArrowRight' })
    await waitFor(() => {
      const payloads = onUpdateInfoDocument.mock.calls.map((call) => call[1])
      expect(payloads.some((payload) => payload.blocks.some(
        (block) => block.type === 'file' && block.display_width === 424,
      ))).toBe(true)
    })
    expect(imageFrame).toHaveStyle({ width: '424px' })

    const trailingEditor = (await screen.findAllByRole('textbox', {
      name: /Testo info in Markdown/,
    }))[1]!
    trailingEditor.innerHTML = '<p>Nota dopo immagine</p>'
    fireEvent.input(trailingEditor)
    fireEvent.blur(trailingEditor)
    await waitFor(() => {
      const payloads = onUpdateInfoDocument.mock.calls.map((call) => call[1])
      expect(payloads.some((payload) => payload.blocks.some(
        (block) => block.type === 'text' && block.markdown === 'Nota dopo immagine',
      ))).toBe(true)
    })

    openSlashMenu(trailingEditor)
    await user.click(await screen.findByRole('menuitem', { name: 'Pagina' }))
    expect(screen.getByRole('textbox', { name: 'Nome sottopagina' })).toBeTruthy()
  })

  it('resizes an Overview image with pointer capture and persists on pointer up', async () => {
    const imageId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [
          { id: crypto.randomUUID(), type: 'text', markdown: 'Prima' },
          {
            id: imageId,
            type: 'file',
            blob_id: crypto.randomUUID(),
            file_name: 'schema.png',
            content_type: 'image/png',
            plaintext_size: 4,
            display_width: 400,
          },
          { id: crypto.randomUUID(), type: 'text', markdown: 'Dopo' },
        ],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const resizeHandle = await screen.findByRole('separator', { name: 'Ridimensiona schema.png' })
    const imageFrame = resizeHandle.parentElement!
    const imageFigure = imageFrame.parentElement!
    Object.defineProperty(imageFrame, 'offsetWidth', { configurable: true, value: 400 })
    Object.defineProperty(imageFigure, 'clientWidth', { configurable: true, value: 800 })
    vi.spyOn(imageFrame, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      top: 0,
      right: 400,
      bottom: 300,
      left: 0,
      width: 400,
      height: 300,
      toJSON: () => ({}),
    })
    let capturedPointer: number | undefined
    Object.defineProperties(resizeHandle, {
      setPointerCapture: {
        configurable: true,
        value: vi.fn((pointerId: number) => { capturedPointer = pointerId }),
      },
      hasPointerCapture: {
        configurable: true,
        value: vi.fn((pointerId: number) => capturedPointer === pointerId),
      },
      releasePointerCapture: {
        configurable: true,
        value: vi.fn((pointerId: number) => {
          if (capturedPointer === pointerId) capturedPointer = undefined
        }),
      },
    })

    fireEvent.pointerDown(resizeHandle, {
      pointerId: 7,
      clientX: 400,
      buttons: 1,
    })
    expect(resizeHandle.setPointerCapture).toHaveBeenCalledWith(7)
    expect(document.body.style.userSelect).toBe('none')
    expect(document.body.style.cursor).toBe('nwse-resize')
    fireEvent.pointerMove(resizeHandle, {
      pointerId: 7,
      clientX: 520,
      buttons: 1,
    })
    await waitFor(() => expect(imageFrame).toHaveStyle({ width: '520px' }))
    expect(onUpdateInfoDocument).not.toHaveBeenCalled()

    fireEvent.pointerUp(resizeHandle, {
      pointerId: 7,
      clientX: 520,
      buttons: 0,
    })
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(1))
    expect(onUpdateInfoDocument.mock.calls[0]?.[1].blocks.find(
      (block) => block.id === imageId,
    )).toMatchObject({
      type: 'file',
      display_width: 520,
    })
    expect(resizeHandle.releasePointerCapture).toHaveBeenCalledWith(7)
    expect(document.body.style.userSelect).toBe('')
    expect(document.body.style.cursor).toBe('')
  })

  it('renders text, files and pages in their exact persisted block order', async () => {
    const textBeforeId = crypto.randomUUID()
    const fileId = crypto.randomUUID()
    const textMiddleId = crypto.randomUUID()
    const pageId = crypto.randomUUID()
    const textAfterId = crypto.randomUUID()
    const childId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [
          { id: textBeforeId, type: 'text', markdown: 'Prima' },
          {
            id: fileId,
            type: 'file',
            blob_id: crypto.randomUUID(),
            file_name: 'intermedio.pdf',
            content_type: 'application/pdf',
            plaintext_size: 4,
          },
          { id: textMiddleId, type: 'text', markdown: 'In mezzo' },
          { id: pageId, type: 'document', document_id: childId, title: 'Pagina interna' },
          { id: textAfterId, type: 'text', markdown: 'Dopo' },
        ],
      },
    }
    const child: DecryptedInfoDocument = {
      ...projectRoot,
      wire: { ...projectRoot.wire, id: childId, parent_document_id: projectRoot.wire.id },
      document: { schema: 1, title: 'Pagina interna', blocks: [] },
    }

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot, child])}
      />,
    )

    const flow = await screen.findByTestId('overview-block-flow')
    expect(Array.from(flow.children).map((element) => (
      (element as HTMLElement).dataset.blockId
    ))).toEqual([textBeforeId, fileId, textMiddleId, pageId, textAfterId])
    expect(screen.getByRole('button', { name: 'Sposta Pagina interna' })).toHaveAttribute('draggable', 'true')
  })

  it('writes inside an open Collapse and restores its persisted closed state', async () => {
    const textId = crypto.randomUUID()
    let persisted: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{
          id: textId,
          type: 'text',
          markdown: ':::collapse Sezione\nContenuto',
        }],
      },
    }
    const onLoadProjectInfo = vi.fn(async () => [persisted])
    let delayClosedSave = false
    let releaseClosedSave: (() => void) | undefined
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, content: InfoDocumentContent) => {
        const closesChapter = content.blocks.some((block) => (
          block.type === 'text' && block.markdown.includes(':::collapse[closed] Sezione')
        ))
        if (delayClosedSave && closesChapter) {
          await new Promise<void>((resolve) => { releaseClosedSave = resolve })
        }
        persisted = {
          ...current,
          wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
          document: content,
        }
        return persisted
      },
    )
    const props = {
      ...baseProps,
      boardFocus: { type: 'generali' as const },
      onLoadProjectInfo,
      onUpdateInfoDocument,
    }
    const view = render(<TasksScreen {...props} boardViewMode="overview" />)

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    const headingText = editor.querySelector<HTMLElement>(
      '[data-md-kind="collapse"] [data-task-text]',
    )
    const textNode = headingText?.firstChild
    expect(textNode?.nodeType).toBe(Node.TEXT_NODE)
    const range = document.createRange()
    range.setStart(textNode!, textNode!.textContent?.length ?? 0)
    range.collapse(true)
    window.getSelection()?.removeAllRanges()
    window.getSelection()?.addRange(range)

    fireEvent.keyDown(editor, { key: 'Enter' })

    expect(editor.querySelectorAll('[data-md-kind="collapse"]')).toHaveLength(1)
    expect(editor.querySelector('[data-md-kind="collapse"]')?.nextElementSibling?.tagName).toBe('P')
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalled())

    delayClosedSave = true
    fireEvent.click(screen.getByRole('button', { name: 'Comprimi capitolo' }))
    await waitFor(() => {
      expect(onUpdateInfoDocument.mock.calls.some((call) => call[1].blocks.some(
        (block) => block.type === 'text' && block.markdown.includes(':::collapse[closed] Sezione'),
      ))).toBe(true)
    })

    window.sessionStorage.clear()
    view.rerender(<TasksScreen {...props} boardViewMode="board" />)
    view.rerender(<TasksScreen {...props} boardViewMode="overview" />)

    expect(await screen.findByRole('button', { name: 'Espandi capitolo' })).toBeTruthy()
    releaseClosedSave?.()
    await waitFor(() => {
      const text = persisted.document.blocks.find(
        (block) => block.id === textId && block.type === 'text',
      )
      expect(text).toMatchObject({
        type: 'text',
        markdown: expect.stringContaining(':::collapse[closed] Sezione'),
      })
    })
  })

  it('creates a page at the selected line instead of appending it after the document', async () => {
    const user = userEvent.setup()
    const textId = crypto.randomUUID()
    const childId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [{ id: textId, type: 'text', markdown: 'Prima\nDopo' }],
      },
    }
    const child: DecryptedInfoDocument = {
      ...projectRoot,
      wire: {
        ...projectRoot.wire,
        id: childId,
        parent_document_id: projectRoot.wire.id,
      },
      document: { schema: 1, title: 'Pagina centrale', blocks: [] },
    }
    let version = projectRoot.wire.payload_version
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: ++version },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onCreateProjectInfoDocument={vi.fn().mockResolvedValue(child)}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    expect(editor.children.length).toBeGreaterThanOrEqual(2)
    openSlashMenuBefore(editor, editor.children[1]!)
    await user.click(await screen.findByRole('menuitem', { name: 'Pagina' }))
    await user.type(screen.getByRole('textbox', { name: 'Nome sottopagina' }), 'Pagina centrale')
    await user.click(screen.getByRole('button', { name: 'Crea' }))

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(2))
    const lastContent = onUpdateInfoDocument.mock.calls.at(-1)?.[1]
    expect(lastContent?.blocks.map((block) => block.type)).toEqual([
      'text', 'document', 'text',
    ])
    expect(lastContent?.blocks[0]).toMatchObject({ id: textId, markdown: 'Prima' })
    expect(lastContent?.blocks[1]).toMatchObject({
      type: 'document',
      document_id: childId,
      title: 'Pagina centrale',
    })
    expect(lastContent?.blocks[2]).toMatchObject({ type: 'text', markdown: 'Dopo' })
  })

  it('keeps edits from multiple text blocks around a page in the same saved snapshot', async () => {
    const firstId = crypto.randomUUID()
    const pageId = crypto.randomUUID()
    const secondId = crypto.randomUUID()
    const childId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: {
        schema: 1,
        blocks: [
          { id: firstId, type: 'text', markdown: 'Prima' },
          { id: pageId, type: 'document', document_id: childId, title: 'Pagina' },
          { id: secondId, type: 'text', markdown: 'Dopo' },
        ],
      },
    }
    const child: DecryptedInfoDocument = {
      ...projectRoot,
      wire: { ...projectRoot.wire, id: childId, parent_document_id: projectRoot.wire.id },
      document: { schema: 1, title: 'Pagina', blocks: [] },
    }
    const onUpdateInfoDocument = vi.fn(
      async (current: DecryptedInfoDocument, document: InfoDocumentContent) => ({
        ...current,
        wire: { ...current.wire, payload_version: current.wire.payload_version + 1 },
        document,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot, child])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editors = await screen.findAllByRole('textbox', { name: /Testo info in Markdown/ })
    editors[0]!.innerHTML = '<p>Prima modificata</p>'
    fireEvent.input(editors[0]!)
    fireEvent.blur(editors[0]!)
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(1))

    const currentEditors = screen.getAllByRole('textbox', { name: /Testo info in Markdown/ })
    currentEditors[1]!.innerHTML = '<p>Dopo modificato</p>'
    fireEvent.input(currentEditors[1]!)
    fireEvent.blur(currentEditors[1]!)

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(2))
    expect(onUpdateInfoDocument.mock.calls[1]?.[1].blocks).toEqual([
      { id: firstId, type: 'text', markdown: 'Prima modificata' },
      { id: pageId, type: 'document', document_id: childId, title: 'Pagina' },
      { id: secondId, type: 'text', markdown: 'Dopo modificato' },
    ])
  })

  it('chains simultaneous Overview saves from the latest returned payload version', async () => {
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
        payload_version: 1,
      },
      document: {
        schema: 1,
        title: 'Titolo iniziale',
        blocks: [{ id: crypto.randomUUID(), type: 'text', markdown: 'Prima' }],
      },
    }
    const releases: Array<() => void> = []
    const onUpdateInfoDocument = vi.fn(
      (current: DecryptedInfoDocument, document: InfoDocumentContent) => (
        new Promise<DecryptedInfoDocument>((resolve) => {
          releases.push(() => resolve({
            ...current,
            wire: {
              ...current.wire,
              payload_version: current.wire.payload_version + 1,
            },
            document,
          }))
        })
      ),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    editor.innerHTML = '<p>Autosave</p>'
    fireEvent.input(editor)
    fireEvent.blur(editor)
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(1))

    const title = screen.getByRole('textbox', { name: 'Titolo Overview' })
    fireEvent.change(title, { target: { value: 'Titolo simultaneo' } })
    fireEvent.blur(title)
    await new Promise((resolve) => window.setTimeout(resolve, 0))
    expect(onUpdateInfoDocument).toHaveBeenCalledTimes(1)

    releases[0]!()
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(2))
    expect(onUpdateInfoDocument.mock.calls[1]?.[0].wire.payload_version).toBe(2)
    releases[1]!()
    await waitFor(() => {
      expect(onUpdateInfoDocument.mock.calls[1]?.[1]).toEqual(expect.objectContaining({
        title: 'Titolo simultaneo',
      }))
    })
  })

  it('preserves the latest content through queued autosave, upload link and resize', async () => {
    const uploadedFileId = crypto.randomUUID()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
        payload_version: 1,
      },
      document: {
        schema: 1,
        blocks: [{ id: crypto.randomUUID(), type: 'text', markdown: 'Prima' }],
      },
    }
    const releases: Array<() => void> = []
    const onUpdateInfoDocument = vi.fn(
      (current: DecryptedInfoDocument, document: InfoDocumentContent) => (
        new Promise<DecryptedInfoDocument>((resolve) => {
          releases.push(() => resolve({
            ...current,
            wire: {
              ...current.wire,
              payload_version: current.wire.payload_version + 1,
            },
            document,
          }))
        })
      ),
    )
    const onUploadInfoDocumentFile = vi.fn().mockResolvedValue({
      id: uploadedFileId,
      type: 'file',
      blob_id: crypto.randomUUID(),
      file_name: 'schema.png',
      content_type: 'image/png',
      plaintext_size: 4,
    })

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onUpdateInfoDocument={onUpdateInfoDocument}
        onUploadInfoDocumentFile={onUploadInfoDocumentFile}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    editor.innerHTML = '<p>Autosave più recente</p>'
    fireEvent.input(editor)
    fireEvent.blur(editor)
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(1))

    openSlashMenu(editor)
    fireEvent.pointerDown(await screen.findByRole('menuitem', { name: 'Immagine' }))
    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]')
    fireEvent.change(fileInput!, {
      target: { files: [new File(['test'], 'schema.png', { type: 'image/png' })] },
    })
    await waitFor(() => expect(onUploadInfoDocumentFile).toHaveBeenCalledTimes(1))

    const resizeHandle = await screen.findByRole('separator', { name: 'Ridimensiona schema.png' })
    const imageFrame = resizeHandle.parentElement!
    const imageFigure = imageFrame.parentElement!
    Object.defineProperty(imageFrame, 'offsetWidth', { configurable: true, value: 400 })
    Object.defineProperty(imageFigure, 'clientWidth', { configurable: true, value: 800 })
    fireEvent.mouseEnter(imageFrame)
    expect(screen.getByRole('separator', { name: 'Ridimensiona schema.png' })).toBe(resizeHandle)
    fireEvent.keyDown(resizeHandle, { key: 'ArrowRight' })
    expect(imageFrame).toHaveStyle({ width: '424px' })

    for (let callIndex = 0; callIndex < 4; callIndex += 1) {
      await waitFor(() => expect(releases.length).toBe(callIndex + 1))
      releases[callIndex]!()
    }
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalledTimes(4))

    expect(onUpdateInfoDocument.mock.calls.map((call) => call[0].wire.payload_version))
      .toEqual([1, 2, 3, 4])
    const lastContent = onUpdateInfoDocument.mock.calls.at(-1)?.[1]
    expect(lastContent?.blocks.some((block) => (
      block.type === 'text' && block.markdown === 'Autosave più recente'
    ))).toBe(true)
    expect(lastContent?.blocks.find((block) => block.id === uploadedFileId)).toMatchObject({
      type: 'file',
      file_name: 'schema.png',
      display_width: 424,
    })
  })

  it('uses the document path to return from an Overview child document', async () => {
    const user = userEvent.setup()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
      document: { schema: 1, blocks: [{ id: crypto.randomUUID(), type: 'text', markdown: '' }] },
    }
    const child: DecryptedInfoDocument = {
      ...projectRoot,
      wire: {
        ...projectRoot.wire,
        id: crypto.randomUUID(),
        parent_document_id: projectRoot.wire.id,
      },
      document: {
        schema: 1,
        title: 'Specifica',
        blocks: [{ id: crypto.randomUUID(), type: 'text', markdown: '' }],
      },
    }
    const onUpdateInfoDocument = vi.fn(
      async (document: DecryptedInfoDocument, content: InfoDocumentContent) => ({
        ...document,
        document: content,
      }),
    )

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={vi.fn().mockResolvedValue([projectRoot])}
        onCreateProjectInfoDocument={vi.fn().mockResolvedValue(child)}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const editor = await screen.findByRole('textbox', { name: 'Testo info in Markdown' })
    openSlashMenu(editor)
    await user.click(await screen.findByRole('menuitem', { name: 'Pagina' }))
    await user.type(screen.getByRole('textbox', { name: 'Nome sottopagina' }), 'Specifica')
    await user.click(screen.getByRole('button', { name: 'Crea' }))
    await user.click(await screen.findByRole('button', { name: 'Specifica' }))

    const path = await screen.findByRole('navigation', { name: 'Percorso documenti' })
    expect(within(path).getByRole('button', { name: 'Specifica' })).toBeDisabled()
    await user.click(within(path).getByRole('button', { name: 'Project' }))
    expect(screen.getByRole('textbox', { name: 'Titolo Overview' })).toHaveValue('Project')
  })

  it('retries loading Generali after a project document decryption error', async () => {
    const user = userEvent.setup()
    const projectRoot: DecryptedInfoDocument = {
      ...infoRoot,
      wire: {
        ...infoRoot.wire,
        topic_id: null,
        task_list_id: null,
        resource_node_id: project.wire.root_resource_id,
      },
    }
    const onLoadProjectInfo = vi
      .fn()
      .mockRejectedValueOnce(new DOMException('decrypt failed', 'OperationError'))
      .mockResolvedValueOnce([projectRoot])

    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'generali' }}
        boardViewMode="overview"
        onLoadProjectInfo={onLoadProjectInfo}
      />,
    )

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Il documento non può essere decifrato con le chiavi di questo dispositivo.',
    )
    await user.click(screen.getByRole('button', { name: 'Riprova' }))

    await waitFor(() => expect(onLoadProjectInfo).toHaveBeenCalledTimes(2))
    expect(screen.getByRole('textbox', { name: 'Testo info in Markdown' })).toBeTruthy()
  })

  it('shows completed tasks in category History', () => {
    const completed = makeTask('Task completato', listId, memberId)
    completed.wire.state = {
      state: 'completed',
      completed_by: memberId,
      completed_at: '2026-07-19T09:00:00.000Z',
    }
    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'topic', topicId }}
        boardViewMode="history"
        tasks={[completed]}
      />,
    )

    expect(screen.getByRole('region', { name: 'History Impianti' })).toBeTruthy()
    expect(
      screen.getByRole('button', { name: /Task completato: Completata/i }),
    ).toBeTruthy()
  })

  it('shows task due progress in History', () => {
    const dueAt = new Date(Date.now() + 2 * 86_400_000).toISOString()
    const task = makeTask('Task in avanzamento', listId, memberId, {
      kind: 'deadline',
      dueAt,
    })
    render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'topic', topicId }}
        boardViewMode="history"
        tasks={[task]}
      />,
    )

    const row = screen.getByRole('button', { name: /Task in avanzamento/i })
    const indicator = row.querySelector<HTMLElement>('.board-task-check')
    expect(indicator?.style.getPropertyValue('--task-due-progress')).not.toBe('')
  })

  it('switches to timeline view and hides kanban columns', async () => {
    const user = userEvent.setup()
    const onBoardViewModeChange = vi.fn()
    const deadlineTask = makeTask('Scadenza', listId, memberId, {
      kind: 'deadline',
      dueAt: timelineDueAt(2, 14),
    })
    const { rerender } = render(
      <TasksScreen
        {...baseProps}
        tasks={[deadlineTask]}
        onBoardViewModeChange={onBoardViewModeChange}
      />,
    )

    await user.click(
      within(screen.getByRole('group', { name: 'Vista board' })).getByRole(
        'button',
        { name: /^Timeline$/i },
      ),
    )
    expect(onBoardViewModeChange).toHaveBeenCalledWith('timeline')

    rerender(
      <TasksScreen
        {...baseProps}
        tasks={[deadlineTask]}
        boardViewMode="timeline"
      />,
    )

    const timelineRegion = screen.getByRole('region', {
      name: 'Timeline giornaliera',
    })
    expect(timelineRegion).toBeTruthy()
    expect(timelineRegion.querySelector('.board-timeline-board-grid')).toBeTruthy()
    expect(timelineRegion.querySelector('.board-timeline-list-gutter')).toBeTruthy()
    expect(screen.getByText('Scadenza')).toBeTruthy()
    expect(screen.getByText('Elena Russo')).toBeTruthy()
    expect(screen.queryByRole('listitem', { name: 'Nuova task list' })).toBeNull()
  })

  it('excludes priority tasks from timeline view', () => {
    const priorityTask = makeTask('Priorità', listId, memberId)
    const deadlineTask = makeTask('Scadenza', listId, memberId, {
      kind: 'deadline',
      dueAt: timelineDueAt(2, 14),
    })

    render(
      <TasksScreen
        {...baseProps}
        tasks={[priorityTask, deadlineTask]}
        boardViewMode="timeline"
      />,
    )

    expect(screen.getByText('Scadenza')).toBeTruthy()
    expect(screen.queryByText('Priorità')).toBeNull()
  })

  it('keeps timeline tasks scoped to member focus', () => {
    const memberTask = makeTask('Member due', listId, memberId, {
      kind: 'deadline',
      dueAt: timelineDueAt(2, 14),
    })
    const otherTask = makeTask('Other due', otherList.wire.id, otherMemberId, {
      kind: 'deadline',
      dueAt: timelineDueAt(2, 15),
    })

    render(
      <TasksScreen
        {...baseProps}
        tasks={[memberTask, otherTask]}
        boardFocus={{ type: 'member', identityId: memberId }}
        boardViewMode="timeline"
      />,
    )

    expect(screen.getByText('Member due')).toBeTruthy()
    expect(screen.queryByText('Other due')).toBeNull()
  })

  it('collapses and expands the sidebar', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} />)

    expect(
      screen.getByRole('complementary', { name: 'Board navigation' }),
    ).toHaveAttribute('aria-expanded', 'true')
    expect(sidebar().getByRole('button', { name: 'Nuova categoria' })).toBeTruthy()

    await user.click(screen.getByRole('button', { name: /Riduci sidebar/i }))
    expect(
      screen.getByRole('complementary', { name: 'Board navigation' }),
    ).toHaveAttribute('aria-expanded', 'false')
    expect(sidebar().queryByRole('button', { name: 'Nuova categoria' })).toBeNull()
    expect(sidebar().getByRole('button', { name: /Generali/i })).toBeTruthy()
    expect(
      screen.getByRole('button', { name: /Espandi sidebar/i }),
    ).toBeTruthy()

    await user.click(screen.getByRole('button', { name: /Espandi sidebar/i }))
    expect(
      screen.getByRole('complementary', { name: 'Board navigation' }),
    ).toHaveAttribute('aria-expanded', 'true')
    expect(sidebar().getByRole('button', { name: 'Nuova categoria' })).toBeTruthy()
  })

  it('shows member columns when selecting a member', () => {
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
        boardViewMode="board"
      />,
    )
    expect(screen.getByRole('listitem', { name: 'Elena Russo' })).toBeTruthy()
    expect(screen.queryByRole('listitem', { name: 'Lucia Bianchi' })).toBeNull()
    expect(screen.queryByRole('listitem', { name: 'Mattina' })).toBeNull()
    expect(screen.queryByRole('listitem', { name: 'Nuovo utente' })).toBeNull()
    expect(screen.queryByRole('listitem', { name: 'Nuova task list' })).toBeNull()
    expect(screen.getByText('Color test')).toBeTruthy()
  })

  it('shows all member columns when selecting Membri', () => {
    const onSelectFocus = vi.fn()
    const { rerender } = render(
      <TasksScreen {...baseProps} onSelectFocus={onSelectFocus} />,
    )

    fireEvent.click(sidebar().getByRole('button', { name: /^Membri$/i }))
    expect(onSelectFocus).toHaveBeenCalledWith({ type: 'members' })

    rerender(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'members' }}
        boardViewMode="board"
      />,
    )
    expect(screen.getByRole('listitem', { name: 'Elena Russo' })).toBeTruthy()
    expect(screen.getByRole('listitem', { name: 'Lucia Bianchi' })).toBeTruthy()
    expect(screen.queryByRole('listitem', { name: 'Nuovo utente' })).toBeNull()
    expect(screen.queryByRole('listitem', { name: 'Mattina' })).toBeNull()
    expect(screen.queryByRole('listitem', { name: 'Nuova task list' })).toBeNull()
    expect(screen.getByText('Color test')).toBeTruthy()
    expect(screen.getByText('Hidden task')).toBeTruthy()
  })

  it('hides unavailable tabs and toolbar filters in member and agent views', () => {
    const { rerender } = render(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'members' }}
        boardViewMode="board"
      />,
    )

    const memberViewSwitch = within(
      screen.getByRole('group', { name: 'Vista board' }),
    )
    expect(memberViewSwitch.queryByRole('button', { name: 'Timeline' })).toBeNull()
    expect(memberViewSwitch.queryByRole('button', { name: 'History' })).toBeNull()
    expect(screen.queryByRole('button', { name: /^Filtra task/ })).toBeNull()

    rerender(
      <TasksScreen
        {...baseProps}
        boardFocus={{ type: 'agents' }}
        boardViewMode="overview"
      />,
    )

    expect(screen.queryByRole('button', { name: 'Filtra agenti' })).toBeNull()
    expect(screen.queryByRole('button', { name: /^Filtra task/ })).toBeNull()
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
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))

    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(dialog).getByLabelText('Titolo'), 'New board task')
    await user.click(within(dialog).getByRole('button', { name: 'Priorità' }))
    const kindMenu = await screen.findByRole('dialog', {
      name: 'Priorità e scadenza',
    })
    expect(
      within(kindMenu).getByRole('menuitem', { name: 'Scadenza' }),
    ).toBeTruthy()
    expect(
      within(kindMenu).getByRole('menuitem', { name: 'Ricorrente' }),
    ).toBeTruthy()
    await user.click(within(kindMenu).getByRole('menuitemradio', { name: 'Alta' }))
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

  it('opens the unified add menu and creates a preset with internal tasks', async () => {
    const user = userEvent.setup()
    const createdPreset: DecryptedPreset = {
      wire: {
        id: crypto.randomUUID(),
        project_id: projectId,
        payload: {
          version: 1,
          algorithm: 'sprout-protocol-v1',
          key_id: crypto.randomUUID(),
          nonce_b64: 'AQ==',
          ciphertext_b64: 'Ag==',
        },
        created_at: '2026-08-31T12:00:00.000Z',
        deleted_at: null,
      },
      document: {
        schema: 1,
        name: 'Apertura locale',
        tasks: [
          {
            taskKind: 'priority',
            title: 'Controlla sala',
            priority: 'normal',
          },
        ],
      },
    }
    const onCreatePreset = vi.fn().mockResolvedValue(createdPreset)
    const onApplyPreset = vi.fn().mockResolvedValue(undefined)
    render(
      <TasksScreen
        {...baseProps}
        onCreatePreset={onCreatePreset}
        onApplyPreset={onApplyPreset}
      />,
    )

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    expect(screen.getByRole('menuitem', { name: /Task/i })).toBeTruthy()
    await user.click(screen.getByRole('menuitem', { name: /Preset/i }))

    const library = screen.getByRole('dialog', { name: 'Scegli preset' })
    await user.click(within(library).getByRole('button', { name: /Crea preset/i }))
    const editor = screen.getByRole('dialog', { name: 'Crea preset' })
    await user.type(within(editor).getByLabelText('Nome preset'), 'Apertura locale')
    const taskEditor = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(taskEditor).getByLabelText('Titolo'), 'Controlla sala')
    const addTaskButton = taskEditor.querySelector<HTMLButtonElement>(
      '.task-create-submit',
    )
    expect(addTaskButton?.textContent).toMatch(/^Aggiungi$/i)
    await user.click(addTaskButton!)
    const taskDot = within(editor).getByRole('listitem')
    expect(taskDot).toBeTruthy()
    const taskDotButton = within(taskDot).getByRole('button', {
      name: /Controlla sala/i,
    })
    await user.hover(taskDotButton)
    expect(within(taskDot).getByRole('tooltip')).toHaveTextContent(
      'Controlla sala',
    )
    await user.click(taskDotButton)
    const reopenedTaskEditor = screen.getByRole('dialog', {
      name: 'Nuovo task',
    })
    expect(within(reopenedTaskEditor).getByLabelText('Titolo')).toHaveValue(
      'Controlla sala',
    )
    expect(within(reopenedTaskEditor).getByRole('button', { name: /^Elimina$/i })).toBeTruthy()
    await user.type(
      within(reopenedTaskEditor).getByLabelText('Commento'),
      ' aggiornata',
    )
    expect(within(reopenedTaskEditor).getByRole('button', { name: /^Salva$/i })).toBeTruthy()
    await user.click(within(editor).getByRole('button', { name: /^Crea preset$/i }))

    await waitFor(() => {
      expect(onCreatePreset).toHaveBeenCalledWith(
        'Apertura locale',
        [expect.objectContaining({ title: 'Controlla sala', taskKind: 'priority' })],
      )
      expect(onApplyPreset).toHaveBeenCalledWith(createdPreset, listId)
    })
  })

  it('opens a linked preset as an internal tasklist page', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    const presetId = crypto.randomUUID()
    const linkedPreset: DecryptedPreset = {
      wire: {
        id: presetId,
        project_id: projectId,
        payload: {
          version: 1,
          algorithm: 'sprout-protocol-v1',
          key_id: crypto.randomUUID(),
          nonce_b64: 'AQ==',
          ciphertext_b64: 'Ag==',
        },
        created_at: '2026-09-02T12:00:00.000Z',
        deleted_at: null,
      },
      document: {
        schema: 1,
        name: 'Apertura locale',
        tasks: [
          {
            taskKind: 'priority',
            title: 'Controlla sala',
            priority: 'normal',
          },
        ],
      },
    }
    const linkedList = {
      ...taskList,
      document: { ...taskList.document!, presetIds: [presetId] },
    }
    render(
      <TasksScreen
        {...baseProps}
        taskLists={[linkedList, otherList]}
        tasks={[
          ...baseProps.tasks,
          makeTask('Task aggiunta al preset', listId, memberId, { presetId }),
        ]}
        presets={[linkedPreset]}
        onCreateTask={onCreateTask}
      />,
    )

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    expect(within(column).queryByText('Task aggiunta al preset')).toBeNull()
    await user.click(
      within(column).getByRole('button', { name: 'Apri preset Apertura locale' }),
    )

    expect(within(column).getByRole('region', { name: 'Preset Apertura locale' })).toBeTruthy()
    expect(within(column).getByText('Controlla sala')).toBeTruthy()
    expect(within(column).getByText('Task aggiunta al preset')).toBeTruthy()
    expect(within(column).getByRole('heading', { name: 'Apertura locale' })).toBeTruthy()
    expect(within(column).queryByRole('heading', { name: 'Elena Russo' })).toBeNull()

    await user.click(
      within(column).getByRole('button', {
        name: 'Apri dettaglio task Controlla sala',
      }),
    )
    await waitFor(() => {
      expect(onCreateTask).toHaveBeenCalledWith(
        expect.objectContaining({
          title: 'Controlla sala',
          presetId,
          presetTemplateIndex: 0,
        }),
        listId,
      )
    })

    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    const taskEditor = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(taskEditor).getByLabelText('Titolo'), 'Task locale')
    await user.click(within(taskEditor).getByRole('button', { name: /^Crea$/i }))
    await waitFor(() => {
      expect(onCreateTask).toHaveBeenCalledWith(
        expect.objectContaining({
          title: 'Task locale',
          presetId,
        }),
        listId,
      )
    })

    await user.click(
      within(column).getByRole('button', { name: 'Torna alle task di Elena Russo' }),
    )
    expect(within(column).queryByRole('region', { name: 'Preset Apertura locale' })).toBeNull()

    await user.click(screen.getByRole('button', { name: /Filtra task/ }))
    await user.click(screen.getByRole('button', { name: 'Apri filtri Tipologia' }))
    await user.click(screen.getByRole('menuitemcheckbox', { name: 'Preset' }))

    expect(
      within(column).getByText('Preset', {
        selector: '.board-card-group-heading span',
      }),
    ).toBeTruthy()
    expect(within(column).getByRole('button', { name: 'Apri preset Apertura locale' })).toBeTruthy()
    expect(within(column).queryByText('Color test')).toBeNull()
  })

  it('shows one category and one canonical task for legacy preset duplicates', async () => {
    const user = userEvent.setup()
    const olderPresetId = crypto.randomUUID()
    const newerPresetId = crypto.randomUUID()
    const presetDocument = {
      schema: 1 as const,
      name: 'Apertura locale',
      tasks: [
        {
          taskKind: 'priority' as const,
          title: 'Controlla sala',
          priority: 'normal' as const,
        },
      ],
    }
    const olderPreset: DecryptedPreset = {
      wire: {
        id: olderPresetId,
        project_id: projectId,
        payload: null,
        created_at: '2026-09-02T12:00:00.000Z',
        deleted_at: null,
      },
      document: presetDocument,
    }
    const newerPreset: DecryptedPreset = {
      wire: {
        ...olderPreset.wire,
        id: newerPresetId,
        created_at: '2026-09-03T12:00:00.000Z',
      },
      document: presetDocument,
    }
    const linkedList = {
      ...taskList,
      document: {
        ...taskList.document!,
        presetIds: [olderPresetId, olderPresetId, newerPresetId],
      },
    }
    const originalTask = makeTask(
      'Task materializzata',
      listId,
      memberId,
      {
        presetId: newerPresetId,
        presetTemplateIndex: 0,
        createdAt: '2026-09-03T12:01:00.000Z',
      },
    )
    const duplicateTask = makeTask(
      'Task materializzata',
      listId,
      memberId,
      {
        presetId: newerPresetId,
        presetTemplateIndex: 0,
        createdAt: '2026-09-03T12:02:00.000Z',
      },
    )

    render(
      <TasksScreen
        {...baseProps}
        taskLists={[linkedList, otherList]}
        tasks={[...baseProps.tasks, duplicateTask, originalTask]}
        presets={[olderPreset, newerPreset]}
      />,
    )

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    const categoryButtons = within(column).getAllByRole('button', {
      name: 'Apri preset Apertura locale',
    })
    expect(categoryButtons).toHaveLength(1)

    await user.click(categoryButtons[0])
    const presetPage = within(column).getByRole('region', {
      name: 'Preset Apertura locale',
    })
    expect(within(presetPage).getAllByText('Task materializzata')).toHaveLength(
      1,
    )
    expect(within(presetPage).queryByText('Controlla sala')).toBeNull()
  })

  it('opens an existing preset in the editor from the vertical menu button', async () => {
    const user = userEvent.setup()
    const existingPreset: DecryptedPreset = {
      wire: {
        id: crypto.randomUUID(),
        project_id: projectId,
        payload: {
          version: 1,
          algorithm: 'sprout-protocol-v1',
          key_id: crypto.randomUUID(),
          nonce_b64: 'AQ==',
          ciphertext_b64: 'Ag==',
        },
        created_at: '2026-09-02T12:00:00.000Z',
        deleted_at: null,
      },
      document: {
        schema: 1,
        name: 'Preset modificabile',
        tasks: [
          {
            taskKind: 'priority',
            title: 'Task esistente',
            priority: 'high',
          },
        ],
      },
    }
    render(<TasksScreen {...baseProps} presets={[existingPreset]} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Preset/i }))
    const library = screen.getByRole('dialog', { name: 'Scegli preset' })
    await user.click(
      within(library).getByRole('button', { name: 'Modifica Preset modificabile' }),
    )

    const editor = screen.getByRole('dialog', { name: 'Modifica preset' })
    expect(within(editor).getByLabelText('Nome preset')).toHaveValue('Preset modificabile')
    expect(within(editor).getByRole('button', { name: '1. Task esistente' })).toBeTruthy()
    expect(within(editor).getByRole('button', { name: 'Salva preset' })).toBeTruthy()
  })

  it('shows recurrence controls without a date field for recurring tasks', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} onCreateTask={vi.fn()} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))

    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.click(within(dialog).getByRole('button', { name: 'Priorità' }))
    const kindMenu = await screen.findByRole('dialog', {
      name: 'Priorità e scadenza',
    })
    await user.click(within(kindMenu).getByRole('menuitem', { name: 'Ricorrente' }))

    expect(within(kindMenu).queryByLabelText('Scadenza')).toBeNull()
    expect(within(kindMenu).queryByLabelText('Prima occorrenza')).toBeNull()
    expect(
      within(kindMenu).getByRole('spinbutton', { name: 'Intervallo ricorrenza' }),
    ).toBeTruthy()
    expect(within(kindMenu).getByRole('radio', { name: 'Minuti' })).toBeTruthy()
    expect(within(kindMenu).getByRole('radio', { name: 'Giorno' })).toBeTruthy()
    expect(within(kindMenu).getByRole('radio', { name: 'Mese' })).toBeTruthy()
    expect(within(kindMenu).queryByRole('radio', { name: 'Settimana' })).toBeNull()
  })

  it('passes optional comment notes when creating a task', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onCreateTask={onCreateTask} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))

    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(dialog).getByLabelText('Titolo'), 'Task con commento')
    await user.type(
      within(dialog).getByLabelText('Commento'),
      'Dettagli sul lavoro da fare',
    )
    await user.click(within(dialog).getByRole('button', { name: /^Crea$/i }))

    expect(onCreateTask).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Task con commento',
        notes: 'Dettagli sul lavoro da fare',
      }),
      listId,
    )
  })

  it('passes questionnaire and attachments when creating a task', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    const questionnaireVersionId = crypto.randomUUID()
    const file = new File(['hello'], 'brief.pdf', { type: 'application/pdf' })
    render(
      <TasksScreen
        {...baseProps}
        publishedQuestionnaireVersions={[
          { id: questionnaireVersionId, label: 'Checklist · v1' },
        ]}
        onCreateTask={onCreateTask}
      />,
    )

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))

    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(dialog).getByLabelText('Titolo'), 'Task con extra')
    await user.click(within(dialog).getByRole('button', { name: 'Aggiungi' }))
    await user.click(
      within(dialog).getByRole('menuitemradio', { name: 'Checklist · v1' }),
    )
    const attachmentInput = dialog.querySelector<HTMLInputElement>(
      'input[type="file"]',
    )
    expect(attachmentInput).not.toBeNull()
    await user.upload(attachmentInput!, file)
    await user.click(within(dialog).getByRole('button', { name: /^Crea$/i }))

    expect(onCreateTask).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Task con extra',
        questionnaireVersionId,
        requiredAttachments: [file],
      }),
      listId,
    )
  })

  it('closes the create task modal on cancel', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))
    expect(screen.getByRole('dialog', { name: 'Nuovo task' })).toBeTruthy()

    await user.click(
      within(screen.getByRole('dialog', { name: 'Nuovo task' })).getByRole(
        'button',
        { name: /^Chiudi$/i },
      ),
    )
    expect(screen.queryByRole('dialog', { name: 'Nuovo task' })).toBeNull()
  })

  it('does not show a permanent task detail column', () => {
    render(<TasksScreen {...baseProps} />)
    expect(screen.queryByText('Nessun task selezionato')).toBeNull()
    expect(screen.queryByRole('dialog', { name: 'Task detail' })).toBeNull()
  })

  it('shows saved attachments in task detail and refreshes them on open', async () => {
    const onRefreshTaskAttachments = vi.fn().mockResolvedValue(undefined)
    const onDownloadTaskAttachment = vi.fn().mockResolvedValue(undefined)
    const task = baseProps.tasks[0]
    const attachmentId = crypto.randomUUID()
    render(
      <TasksScreen
        {...baseProps}
        selectedTaskId={task.wire.id}
        onRefreshTaskAttachments={onRefreshTaskAttachments}
        onDownloadTaskAttachment={onDownloadTaskAttachment}
        taskAttachments={[
          {
            id: attachmentId,
            project_id: projectId,
            resource_node_id: task.wire.resource_node_id,
            key_epoch: 1,
            attachment_kind: 'task_required',
            blob_id: crypto.randomUUID(),
            task_id: task.wire.id,
            pretask_id: null,
            source_attachment_id: null,
            assignment_id: null,
            encrypted_metadata: null,
            state: {
              state: 'available',
              uploaded_at: '2026-07-18T12:00:00.000Z',
            },
            created_at: '2026-07-18T12:00:00.000Z',
          },
        ]}
        taskAttachmentLabels={{ [attachmentId]: 'brief.pdf' }}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    expect(onRefreshTaskAttachments).toHaveBeenCalledWith(task.wire.id)
    expect(
      within(drawer).getByRole('button', { name: 'brief.pdf' }),
    ).toBeTruthy()
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
    expect(
      within(drawer).getByRole('textbox', { name: 'Titolo' }),
    ).toHaveValue('Color test')
    expect(
      within(drawer).getByRole('textbox', { name: 'Commento' }),
    ).toHaveValue('Color test notes')
    expect(within(drawer).getByText('18 lug')).toBeTruthy()
    expect(
      within(drawer).getByLabelText('Assegnato a Elena Russo', {
        selector: 'span',
      }),
    ).toBeTruthy()
  })

  it('shows creation date but hides assignment metadata when the task is unassigned', () => {
    const unassignedTask = makeTask('Da fare', listId, null)
    render(
      <TasksScreen
        {...baseProps}
        tasks={[unassignedTask]}
        selectedTaskId={unassignedTask.wire.id}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    expect(within(drawer).getByText('18 lug')).toBeTruthy()
    expect(
      within(drawer).queryByLabelText(/Assegnato a/i, { selector: 'span' }),
    ).toBeNull()
    expect(
      within(drawer).getByRole('button', { name: 'Assegna' }),
    ).toBeTruthy()
  })

  it('lets unassigned tasks pick a board member from Assegna', async () => {
    const user = userEvent.setup()
    const onAssignTask = vi.fn().mockResolvedValue(undefined)
    const unassignedTask = makeTask('Da fare', listId, null)
    render(
      <TasksScreen
        {...baseProps}
        tasks={[unassignedTask]}
        selectedTaskId={unassignedTask.wire.id}
        onAssignTask={onAssignTask}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    await user.click(within(drawer).getByRole('button', { name: 'Assegna' }))
    await user.click(
      within(drawer).getByRole('menuitemradio', { name: 'Lucia Bianchi' }),
    )

    expect(onAssignTask).toHaveBeenCalledWith(
      unassignedTask,
      otherMemberId,
    )
  })

  it('keeps attached files when the chip name is clicked and only removes via ×', async () => {
    const user = userEvent.setup()
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(null)
    const file = new File(['hello'], 'brief.pdf', { type: 'application/pdf' })
    render(<TasksScreen {...baseProps} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))

    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    const attachmentInput = dialog.querySelector<HTMLInputElement>(
      'input[type="file"]',
    )
    expect(attachmentInput).not.toBeNull()
    await user.upload(attachmentInput!, file)
    expect(within(dialog).getByRole('button', { name: 'brief.pdf' })).toBeTruthy()

    await user.click(within(dialog).getByRole('button', { name: 'brief.pdf' }))
    expect(within(dialog).getByRole('button', { name: 'brief.pdf' })).toBeTruthy()

    await user.click(
      within(dialog).getByRole('button', { name: 'Rimuovi brief.pdf' }),
    )
    expect(
      within(dialog).queryByRole('button', { name: 'brief.pdf' }),
    ).toBeNull()
    openSpy.mockRestore()
  })

  it('passes selected assignee when creating a task', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onCreateTask={onCreateTask} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))

    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(dialog).getByLabelText('Titolo'), 'Task assegnato')
    await user.click(within(dialog).getByRole('button', { name: 'Assegna' }))
    await user.click(
      within(dialog).getByRole('menuitemradio', { name: 'Lucia Bianchi' }),
    )
    expect(
      within(dialog).getByRole('button', { name: 'Assegna' }),
    ).toBeTruthy()
    expect(
      within(dialog).getByLabelText('Assegnato a Lucia Bianchi', {
        selector: 'span',
      }),
    ).toBeTruthy()
    await user.click(within(dialog).getByRole('button', { name: /^Crea$/i }))

    expect(onCreateTask).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Task assegnato',
        assigneeIdentityId: otherMemberId,
      }),
      listId,
    )
  })

  it('combines deadline kind and due date in task detail footer', () => {
    const deadlineTask: DecryptedTask = {
      ...makeTask('Pagare bolletta', listId, memberId),
      wire: {
        ...makeTask('Pagare bolletta', listId, memberId).wire,
        task_kind: 'deadline',
      },
      document: {
        schema: 1,
        title: 'Pagare bolletta',
        due_at: '2026-08-22T10:23:00.000Z',
      },
    }
    render(
      <TasksScreen
        {...baseProps}
        tasks={[deadlineTask]}
        selectedTaskId={deadlineTask.wire.id}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    const tools = drawer.querySelector('.task-create-tools')
    expect(tools).toBeTruthy()
    const deadlinePill = within(tools as HTMLElement).getByRole('button', {
      name: 'Priorità',
    })
    expect(deadlinePill.textContent).toMatch(/Scad ·/)
    expect(
      within(tools as HTMLElement).queryAllByText(/^Scad$/),
    ).toHaveLength(0)
  })

  it('shows create-panel footer tools in task detail', () => {
    const task = baseProps.tasks[0]
    render(
      <TasksScreen
        {...baseProps}
        selectedTaskId={task.wire.id}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    expect(within(drawer).getByRole('button', { name: 'Aggiungi' })).toBeTruthy()
    expect(within(drawer).getByLabelText('Allega file')).toBeTruthy()
    expect(within(drawer).getByRole('button', { name: 'Priorità' })).toBeTruthy()
    expect(
      within(drawer).getByRole('button', { name: 'Assegna' }),
    ).toBeTruthy()
    expect(within(drawer).getByText('Aperto')).toBeTruthy()
    expect(
      within(drawer).getByLabelText('Assegnato a Elena Russo', {
        selector: 'span',
      }),
    ).toBeTruthy()
  })

  it('keeps Assegna available on assigned tasks for reassignment', async () => {
    const user = userEvent.setup()
    const onAssignTask = vi.fn().mockResolvedValue(undefined)
    const task = baseProps.tasks[0]
    render(
      <TasksScreen
        {...baseProps}
        selectedTaskId={task.wire.id}
        onAssignTask={onAssignTask}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    await user.click(within(drawer).getByRole('button', { name: 'Assegna' }))
    await user.click(
      within(drawer).getByRole('menuitemradio', { name: 'Lucia Bianchi' }),
    )

    expect(onAssignTask).toHaveBeenCalledWith(task, otherMemberId)
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

  it('starts inline rename from the topic overview menu', async () => {
    const user = userEvent.setup()
    const onRenameTopic = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onRenameTopic={onRenameTopic} />)

    fireEvent.contextMenu(sidebar().getByRole('button', { name: /Impianti/i }))
    const menu = screen.getByRole('menu', { name: /Azioni per Impianti/i })
    await user.click(within(menu).getByRole('menuitem', { name: 'Rinomina' }))

    const input = screen.getByLabelText('Rinomina categoria')
    expect(input).toHaveValue('Impianti')
    await user.clear(input)
    await user.type(input, 'Manutenzione{Enter}')

    await waitFor(() => {
      expect(onRenameTopic).toHaveBeenCalledWith(topic, 'Manutenzione')
    })
  })

  it('cancels inline rename when pressing Escape', async () => {
    const user = userEvent.setup()
    const onRenameTopic = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onRenameTopic={onRenameTopic} />)

    fireEvent.contextMenu(sidebar().getByRole('button', { name: /Impianti/i }))
    await user.click(
      screen.getByRole('menuitem', { name: 'Rinomina' }),
    )

    const input = screen.getByLabelText('Rinomina categoria')
    await user.clear(input)
    await user.type(input, 'Annullato{Escape}')

    expect(onRenameTopic).not.toHaveBeenCalled()
    expect(sidebar().getByRole('button', { name: /Impianti/i })).toBeTruthy()
  })

  it('sorts favorite topics to the top of the sidebar', () => {
    const favoriteTopic: TopicItem = {
      wire: {
        ...topic.wire,
        id: crypto.randomUUID(),
        created_at: '2026-07-19T12:00:00.000Z',
      },
      document: { schema: 1, name: 'Zona VIP', favorite: true },
    }
    render(
      <TasksScreen
        {...baseProps}
        topics={[topic, favoriteTopic]}
      />,
    )

    const labels = sidebar()
      .getAllByRole('button', { name: /Impianti|Zona VIP/i })
      .map((button) => button.textContent?.trim())
    expect(labels[0]).toContain('Zona VIP')
  })

  it('changes task filter from the pill dropdown', async () => {
    const user = userEvent.setup()
    const onFilter = vi.fn()
    render(<TasksScreen {...baseProps} onFilter={onFilter} />)

    const filterTrigger = screen.getByRole('button', {
      name: 'Filtra task: Aperti',
    })
    expect(filterTrigger).toBeTruthy()
    expect(filterTrigger.textContent).toBe('')

    await user.click(filterTrigger)
    await user.click(screen.getByRole('menuitemradio', { name: 'Oggi' }))

    expect(onFilter).toHaveBeenCalledWith('today')
  })

  it('renders task cards with notes, due date, and assignee avatar', () => {
    const taskWithDue: DecryptedTask = {
      ...makeTask('Comprare il latte', listId, memberId),
      document: {
        schema: 1,
        title: 'Comprare il latte',
        priority: 'normal',
        notes: 'Latte intero, due litri',
        due_at: '2026-12-15T09:00:00.000Z',
      },
    }
    render(
      <TasksScreen
        {...baseProps}
        tasks={[taskWithDue, baseProps.tasks[1]]}
      />,
    )

    const card = screen.getByRole('button', { name: /Comprare il latte/i })
    expect(within(card).getByText('Latte intero, due litri')).toBeTruthy()
    const cardArticle = card.closest('article')
    expect(cardArticle).toBeTruthy()
    expect(within(cardArticle!).getByText('15 dic')).toBeTruthy()
    expect(
      within(cardArticle!).getByLabelText('Assegnato a Elena Russo', {
        selector: 'span',
      }),
    ).toBeTruthy()
  })

  it('shows assignee overview banner on avatar hover', async () => {
    const taskWithDue: DecryptedTask = {
      ...makeTask('Comprare il latte', listId, memberId),
      document: {
        schema: 1,
        title: 'Comprare il latte',
        priority: 'normal',
        notes: 'Latte intero, due litri',
        due_at: '2026-12-15T09:00:00.000Z',
      },
    }
    render(
      <TasksScreen
        {...baseProps}
        tasks={[taskWithDue, baseProps.tasks[1]]}
      />,
    )

    const assignee = screen.getByLabelText('Assegnato a Elena Russo', {
      selector: 'span',
    })
    fireEvent.mouseEnter(assignee)

    await waitFor(() => {
      expect(screen.getByRole('tooltip')).toBeTruthy()
    })
    const tooltip = screen.getByRole('tooltip')
    expect(within(tooltip).getByText('Assegnato a')).toBeTruthy()
    expect(within(tooltip).getByText('Elena Russo')).toBeTruthy()
    expect(assignee.getAttribute('aria-describedby')).toBeTruthy()
  })

  it('shows a pencil on task list hover and enters inline edit mode', async () => {
    const user = userEvent.setup()
    const onUpdateTaskList = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onUpdateTaskList={onUpdateTaskList} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(
      within(column).getByRole('button', {
        name: 'Apri dettaglio di Elena Russo',
      }),
    )
    const history = screen.getByRole('region', { name: 'Storico Elena Russo' })
    await user.click(
      within(history).getByRole('button', { name: 'Modifica Elena Russo' }),
    )

    const input = screen.getByLabelText('Modifica nome task list')
    expect(input).toHaveValue('Elena Russo')
    expect(
      within(history).getByRole('button', { name: 'Conferma modifiche task list' }),
    ).toBeTruthy()
  })

  it('saves task list name and color on confirm', async () => {
    const user = userEvent.setup()
    const onUpdateTaskList = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onUpdateTaskList={onUpdateTaskList} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(
      within(column).getByRole('button', {
        name: 'Apri dettaglio di Elena Russo',
      }),
    )
    const history = screen.getByRole('region', { name: 'Storico Elena Russo' })
    await user.click(
      within(history).getByRole('button', { name: 'Modifica Elena Russo' }),
    )

    const input = screen.getByLabelText('Modifica nome task list')
    await user.clear(input)
    await user.type(input, 'Pomeriggio')

    await user.click(
      within(history).getByRole('button', { name: 'Scegli icona task list' }),
    )
    await user.click(screen.getByRole('option', { name: 'Rosa' }))

    await user.click(
      within(history).getByRole('button', { name: 'Conferma modifiche task list' }),
    )

    await waitFor(() => {
      expect(onUpdateTaskList).toHaveBeenCalledWith(taskList, {
        name: 'Pomeriggio',
        color: 'column-rose',
      })
    })
  })

  it('cancels task list edit when pressing Escape', async () => {
    const user = userEvent.setup()
    const onUpdateTaskList = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onUpdateTaskList={onUpdateTaskList} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(
      within(column).getByRole('button', {
        name: 'Apri dettaglio di Elena Russo',
      }),
    )
    const history = screen.getByRole('region', { name: 'Storico Elena Russo' })
    await user.click(
      within(history).getByRole('button', { name: 'Modifica Elena Russo' }),
    )

    const input = screen.getByLabelText('Modifica nome task list')
    await user.clear(input)
    await user.type(input, 'Annullato{Escape}')

    expect(onUpdateTaskList).not.toHaveBeenCalled()
    expect(within(history).getByRole('heading', { name: 'Elena Russo' })).toBeTruthy()
    expect(
      within(history).getByRole('button', { name: 'Modifica Elena Russo' }),
    ).toBeTruthy()
  })

  it('switches from task history to encrypted task-list info', async () => {
    const user = userEvent.setup()
    const onLoadTaskListInfo = vi.fn().mockResolvedValue([infoRoot])
    const onUpdateInfoDocument = vi.fn().mockImplementation(
      async (document: DecryptedInfoDocument, content) => ({
        ...document,
        document: content,
      }),
    )
    render(
      <TasksScreen
        {...baseProps}
        onLoadTaskListInfo={onLoadTaskListInfo}
        onUpdateInfoDocument={onUpdateInfoDocument}
      />,
    )

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(
      within(column).getByRole('button', {
        name: 'Apri dettaglio di Elena Russo',
      }),
    )
    await user.click(screen.getByRole('tab', { name: 'Info' }))

    await waitFor(() => {
      expect(onLoadTaskListInfo).toHaveBeenCalledWith(taskList)
    })
    expect(
      await screen.findByRole('link', { name: 'https://sprout.test' }),
    ).toHaveAttribute('href', 'https://sprout.test')

    await user.click(screen.getByRole('button', { name: /Testo/i }))
    await user.type(screen.getByLabelText('Testo info in Markdown'), '\nDettagli')
    await user.click(screen.getByRole('button', { name: 'Salva' }))

    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalled())
  })

  it('keeps every selected property when creating a richly configured task', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    const questionnaireVersionId = crypto.randomUUID()
    const file = new File(['brief'], 'specifiche.pdf', {
      type: 'application/pdf',
    })
    render(
      <TasksScreen
        {...baseProps}
        publishedQuestionnaireVersions={[
          { id: questionnaireVersionId, label: 'Verifica qualità · v3' },
        ]}
        onCreateTask={onCreateTask}
      />,
    )

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))
    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })

    await user.type(within(dialog).getByLabelText('Titolo'), 'Controllo completo')
    await user.type(within(dialog).getByLabelText('Commento'), 'Note combinate')
    await user.click(within(dialog).getByRole('button', { name: 'Aggiungi' }))
    await user.click(
      within(dialog).getByRole('menuitemradio', {
        name: 'Verifica qualità · v3',
      }),
    )
    await user.click(within(dialog).getByRole('button', { name: 'Assegna' }))
    await user.click(
      within(dialog).getByRole('menuitemradio', { name: 'Lucia Bianchi' }),
    )
    await user.upload(
      dialog.querySelector<HTMLInputElement>('input[type="file"]')!,
      file,
    )
    await user.click(within(dialog).getByRole('button', { name: 'Priorità' }))
    const kindMenu = await screen.findByRole('dialog', {
      name: 'Priorità e scadenza',
    })
    await user.click(within(kindMenu).getByRole('menuitem', { name: 'Priorità' }))
    await user.click(within(kindMenu).getByRole('menuitemradio', { name: 'Alta' }))
    await user.click(within(dialog).getByRole('button', { name: /^Crea$/i }))

    expect(onCreateTask).toHaveBeenCalledWith(
      {
        taskKind: 'priority',
        title: 'Controllo completo',
        notes: 'Note combinate',
        priority: 'high',
        questionnaireVersionId,
        requiredAttachments: [file],
        assigneeIdentityId: otherMemberId,
      },
      listId,
    )
  })

  it('submits only recurrence properties after changing task kind repeatedly', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn().mockResolvedValue(undefined)
    render(<TasksScreen {...baseProps} onCreateTask={onCreateTask} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))
    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(dialog).getByLabelText('Titolo'), 'Cambio proprietà')

    await user.click(within(dialog).getByRole('button', { name: 'Priorità' }))
    let kindMenu = await screen.findByRole('dialog', {
      name: 'Priorità e scadenza',
    })
    await user.click(within(kindMenu).getByRole('menuitemradio', { name: 'Alta' }))

    await user.click(within(dialog).getByRole('button', { name: 'Priorità' }))
    kindMenu = await screen.findByRole('dialog', {
      name: 'Priorità e scadenza',
    })
    await user.click(within(kindMenu).getByRole('menuitem', { name: 'Ricorrente' }))
    const interval = within(kindMenu).getByRole('spinbutton', {
      name: 'Intervallo ricorrenza',
    })
    await user.clear(interval)
    await user.type(interval, '3')
    await user.click(within(kindMenu).getByRole('radio', { name: 'Mese' }))
    fireEvent.mouseDown(within(dialog).getByLabelText('Titolo'))
    await user.click(within(dialog).getByRole('button', { name: /^Crea$/i }))

    expect(onCreateTask).toHaveBeenCalledWith(
      expect.objectContaining({
        taskKind: 'recurring',
        frequency: 'monthly',
        interval: 3,
      }),
      listId,
    )
    const submitted = onCreateTask.mock.calls[0][0]
    expect(submitted).not.toHaveProperty('priority')
  })

  it('persists the selected task kind when saving task details', async () => {
    const user = userEvent.setup()
    const onUpdateTask = vi.fn().mockResolvedValue(undefined)
    const task = makeTask('Converti tipo', listId, memberId)
    render(
      <TasksScreen
        {...baseProps}
        tasks={[task]}
        selectedTaskId={task.wire.id}
        onUpdateTask={onUpdateTask}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    await user.click(within(drawer).getByRole('button', { name: 'Priorità' }))
    const kindMenu = await screen.findByRole('dialog', {
      name: 'Priorità e scadenza',
    })
    await user.click(within(kindMenu).getByRole('menuitem', { name: 'Ricorrente' }))
    const interval = within(kindMenu).getByRole('spinbutton', {
      name: 'Intervallo ricorrenza',
    })
    await user.clear(interval)
    await user.type(interval, '2')
    fireEvent.mouseDown(within(drawer).getByLabelText('Titolo'))
    await user.click(within(drawer).getByRole('button', { name: 'Salva' }))

    expect(onUpdateTask).toHaveBeenCalledWith(
      task,
      expect.objectContaining({
        taskKind: 'recurring',
        recurrence: { frequency: 'daily', interval: 2 },
      }),
    )
  })

  it('persists questionnaire and new attachments when saving task details', async () => {
    const user = userEvent.setup()
    const onUpdateTask = vi.fn().mockResolvedValue(undefined)
    const questionnaireVersionId = crypto.randomUUID()
    const task = makeTask('Extra dettaglio', listId, memberId)
    const file = new File(['foto'], 'evidenza.png', { type: 'image/png' })
    render(
      <TasksScreen
        {...baseProps}
        tasks={[task]}
        selectedTaskId={task.wire.id}
        publishedQuestionnaireVersions={[
          { id: questionnaireVersionId, label: 'Checklist finale · v2' },
        ]}
        onUpdateTask={onUpdateTask}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    await user.click(within(drawer).getByRole('button', { name: 'Aggiungi' }))
    await user.click(
      within(drawer).getByRole('menuitemradio', {
        name: 'Checklist finale · v2',
      }),
    )
    await user.upload(
      drawer.querySelector<HTMLInputElement>('input[type="file"]')!,
      file,
    )
    await user.click(within(drawer).getByRole('button', { name: 'Salva' }))

    expect(onUpdateTask).toHaveBeenCalledWith(
      task,
      expect.objectContaining({
        questionnaireVersionId,
        attachmentFiles: [file],
      }),
    )
  })

  it('clears deadline and recurrence fields when task detail changes to priority', async () => {
    const user = userEvent.setup()
    const onUpdateTask = vi.fn().mockResolvedValue(undefined)
    const task: DecryptedTask = {
      ...makeTask('Rimuovi scadenza', listId, memberId, {
        kind: 'recurring',
        dueAt: '2026-09-15T10:00:00.000Z',
      }),
      document: {
        schema: 1,
        title: 'Rimuovi scadenza',
        due_at: '2026-09-15T10:00:00.000Z',
        recurrence: { frequency: 'monthly', interval: 2 },
      },
    }
    render(
      <TasksScreen
        {...baseProps}
        tasks={[task]}
        selectedTaskId={task.wire.id}
        onUpdateTask={onUpdateTask}
      />,
    )

    const drawer = screen.getByRole('dialog', { name: 'Task detail' })
    await user.click(within(drawer).getByRole('button', { name: 'Priorità' }))
    const kindMenu = await screen.findByRole('dialog', {
      name: 'Priorità e scadenza',
    })
    await user.click(
      within(kindMenu).getByRole('menuitem', { name: 'Priorità' }),
    )
    await user.click(within(kindMenu).getByRole('menuitemradio', { name: 'Alta' }))
    await user.click(within(drawer).getByRole('button', { name: 'Salva' }))

    expect(onUpdateTask).toHaveBeenCalledWith(
      task,
      expect.objectContaining({
        taskKind: 'priority',
        priority: 'high',
      }),
    )
    const submitted = onUpdateTask.mock.calls[0][1]
    expect(submitted).not.toHaveProperty('dueAt')
    expect(submitted).not.toHaveProperty('recurrence')
  })

  it('prevents duplicate create submissions while the first request is pending', async () => {
    const user = userEvent.setup()
    let resolveCreate!: () => void
    const onCreateTask = vi.fn(
      () => new Promise<void>((resolve) => {
        resolveCreate = resolve
      }),
    )
    render(<TasksScreen {...baseProps} onCreateTask={onCreateTask} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))
    await user.click(screen.getByRole('menuitem', { name: /Task/i }))
    const dialog = screen.getByRole('dialog', { name: 'Nuovo task' })
    await user.type(within(dialog).getByLabelText('Titolo'), 'Una volta sola')
    const submit = within(dialog).getByRole('button', { name: /^Crea$/i })

    fireEvent.click(submit)
    fireEvent.click(submit)
    expect(onCreateTask).toHaveBeenCalledTimes(1)
    expect(submit).toBeDisabled()

    resolveCreate()
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Nuovo task' })).toBeNull()
    })
  })

  it('resets search and advanced filters when the selected project changes', async () => {
    const user = userEvent.setup()
    const { rerender } = render(<TasksScreen {...baseProps} />)

    expect(screen.getByRole('button', { name: 'Filtra task' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Da completare' })).toBeNull()

    const search = screen.getByLabelText('Cerca task e tasklist')
    await user.type(search, 'Color')
    await user.click(screen.getByRole('button', { name: /^Filtra task/ }))
    await user.click(screen.getByRole('button', { name: 'Apri filtri Stato' }))
    await user.click(screen.getByRole('menuitemcheckbox', { name: 'Completati' }))
    await user.click(screen.getByRole('menuitemcheckbox', { name: 'Da completare' }))
    expect(screen.queryByText('Color test')).toBeNull()

    const nextProjectId = crypto.randomUUID()
    const nextListId = crypto.randomUUID()
    const nextProject: ProjectItem = {
      ...project,
      wire: {
        ...project.wire,
        id: nextProjectId,
        root_resource_id: crypto.randomUUID(),
      },
      document: { schema: 1, name: 'Second project' },
    }
    const nextList: TaskListItem = {
      ...taskList,
      wire: {
        ...taskList.wire,
        id: nextListId,
        project_id: nextProjectId,
        resource_node_id: crypto.randomUUID(),
      },
      document: { schema: 1, name: 'Nuova board' },
    }
    const nextTask = {
      ...makeTask('Visibile nel nuovo progetto', nextListId, memberId),
      wire: {
        ...makeTask('Visibile nel nuovo progetto', nextListId, memberId).wire,
        project_id: nextProjectId,
        list_id: nextListId,
      },
    }
    rerender(
      <TasksScreen
        {...baseProps}
        project={nextProject}
        taskLists={[nextList]}
        tasks={[nextTask]}
        selectedListId={nextListId}
      />,
    )

    expect(screen.getByLabelText('Cerca task e tasklist')).toHaveValue('')
    expect(screen.getByRole('button', { name: 'Filtra task' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Da completare' })).toBeNull()
    expect(screen.getByText('Visibile nel nuovo progetto')).toBeTruthy()
  })

  it('hides tasklists with no content matching an explicit filter', async () => {
    const user = userEvent.setup()
    const completedTask = makeTask('Già completata', listId, memberId)
    completedTask.wire.state = {
      state: 'completed',
      completed_by: memberId,
      completed_at: '2026-09-03T10:00:00.000Z',
    }
    const openTask = makeTask(
      'Ancora aperta',
      otherList.wire.id,
      otherMemberId,
    )

    render(
      <TasksScreen
        {...baseProps}
        tasks={[completedTask, openTask]}
      />,
    )

    // The implicit "open" default must not remove otherwise empty tasklists.
    expect(screen.getByRole('listitem', { name: 'Elena Russo' })).toBeTruthy()
    expect(screen.getByRole('listitem', { name: 'Mattina' })).toBeTruthy()

    await user.click(screen.getByRole('button', { name: /^Filtra task/ }))
    await user.click(screen.getByRole('button', { name: 'Apri filtri Stato' }))
    await user.click(screen.getByRole('menuitemcheckbox', { name: 'Completati' }))
    await user.click(screen.getByRole('menuitemcheckbox', { name: 'Da completare' }))

    expect(screen.getByRole('listitem', { name: 'Elena Russo' })).toBeTruthy()
    expect(screen.queryByRole('listitem', { name: 'Mattina' })).toBeNull()
  })
})
