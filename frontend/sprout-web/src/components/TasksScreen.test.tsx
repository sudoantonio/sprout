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
    created_at: '2026-07-18T12:00:00.000Z',
    payload_version: 1,
  },
  document: {
    schema: 1,
    title,
    priority: 'normal',
    notes: `${title} notes`,
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

  it('supports interactive Overview tasks and separate attachment commands', async () => {
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
          markdown: '- [ ] Verifica allegati\n',
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
    const task = screen.getByRole('checkbox', { name: 'Completa task' })
    await user.click(task)
    fireEvent.blur(editor)
    await waitFor(() => expect(onUpdateInfoDocument).toHaveBeenCalled())
    expect(onUpdateInfoDocument.mock.calls.at(-1)?.[1]).toEqual(
      expect.objectContaining({
        blocks: expect.arrayContaining([
          expect.objectContaining({ markdown: '- [x] Verifica allegati\n' }),
        ]),
      }),
    )

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
    vi.spyOn(imageFrame, 'getBoundingClientRect').mockReturnValue({
      width: 400,
    } as DOMRect)
    vi.spyOn(imageFigure, 'getBoundingClientRect').mockReturnValue({
      width: 800,
    } as DOMRect)
    fireEvent.keyDown(resizeHandle, { key: 'ArrowRight' })
    await waitFor(() => {
      const payloads = onUpdateInfoDocument.mock.calls.map((call) => call[1])
      expect(payloads.some((payload) => payload.blocks.some(
        (block) => block.type === 'file' && block.display_width === 424,
      ))).toBe(true)
    })
    expect(imageFrame).toHaveStyle({ width: '424px' })

    const trailingEditor = await screen.findByRole('textbox', { name: 'Testo dopo gli allegati' })
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
    await user.click(await screen.findByRole('menuitem', { name: 'Documento' }))
    expect(screen.getByRole('textbox', { name: 'Nome sotto-documento' })).toBeTruthy()
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
    await user.click(await screen.findByRole('menuitem', { name: 'Documento' }))
    await user.type(screen.getByRole('textbox', { name: 'Nome sotto-documento' }), 'Specifica')
    await user.click(screen.getByRole('button', { name: 'Crea' }))

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

  it('shows recurrence controls without a date field for recurring tasks', async () => {
    const user = userEvent.setup()
    render(<TasksScreen {...baseProps} onCreateTask={vi.fn()} />)

    const column = screen.getByRole('listitem', { name: 'Elena Russo' })
    await user.click(within(column).getByRole('button', { name: /Aggiungi/i }))

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
})
