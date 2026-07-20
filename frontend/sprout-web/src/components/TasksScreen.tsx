import { useMemo, useState, type FormEvent } from 'react'
import type { TaskDto, Uuid } from '../api/contracts'
import {
  filterTasks,
  formatDueDate,
  isRecurringTaskOverdue,
  isTaskOverdue,
} from '../domain/tasks'
import type {
  DecryptedTask,
  TaskCreationInput,
  TaskFilter,
} from '../domain/models'
import type {
  ProjectItem,
  TaskListItem,
  TopicItem,
} from '../store/app-store'
import { ChevronIcon, LockIcon, PlusIcon, RepeatIcon } from './icons'

interface TasksScreenProps {
  project?: ProjectItem
  topics: TopicItem[]
  taskLists: TaskListItem[]
  tasks: ReturnType<typeof filterTasks>
  lockedTasks: TaskDto[]
  selectedTopicId?: Uuid
  selectedListId?: Uuid
  selectedTaskId?: Uuid
  publishedQuestionnaireVersions: Array<{
    id: Uuid
    label: string
  }>
  filter: TaskFilter
  loading: boolean
  onSelectTopic(id: Uuid): void
  onSelectList(id: Uuid): void
  onSelectTask(id: Uuid): void
  onFilter(filter: TaskFilter): void
  onCreateTopic(name: string): Promise<void>
  onCreateList(name: string): Promise<void>
  onCreateTask(input: TaskCreationInput): Promise<void>
  onUpdateTask(
    task: DecryptedTask,
    input: { title: string; notes?: string },
  ): Promise<void>
  onCompleteTask(task: DecryptedTask): Promise<void>
  onCopyTask(task: DecryptedTask): Promise<void>
}

const EditTaskForm = ({
  task,
  onUpdate,
}: {
  task: DecryptedTask
  onUpdate(
    task: DecryptedTask,
    input: { title: string; notes?: string },
  ): Promise<void>
}) => {
  const [title, setTitle] = useState(task.document.title)
  const [notes, setNotes] = useState(task.document.notes ?? '')
  return (
    <form
      className="detail-edit-form"
      onSubmit={(event) => {
        event.preventDefault()
        void onUpdate(task, { title, notes: notes || undefined })
      }}
    >
      <label>
        Title
        <input
          required
          value={title}
          onChange={(event) => setTitle(event.target.value)}
        />
      </label>
      <label>
        Notes
        <textarea
          value={notes}
          onChange={(event) => setNotes(event.target.value)}
        />
      </label>
      <button type="submit" className="secondary-button">
        Save encrypted update
      </button>
    </form>
  )
}

const filters: Array<{ value: TaskFilter; label: string }> = [
  { value: 'open', label: 'Open' },
  { value: 'today', label: 'Today' },
  { value: 'upcoming', label: 'Upcoming' },
  { value: 'completed', label: 'Completed' },
]

export const TasksScreen = ({
  project,
  topics,
  taskLists,
  tasks,
  lockedTasks,
  selectedTopicId,
  selectedListId,
  selectedTaskId,
  publishedQuestionnaireVersions,
  filter,
  loading,
  onSelectTopic,
  onSelectList,
  onSelectTask,
  onFilter,
  onCreateTopic,
  onCreateList,
  onCreateTask,
  onUpdateTask,
  onCompleteTask,
  onCopyTask,
}: TasksScreenProps) => {
  const [topicName, setTopicName] = useState('')
  const [listName, setListName] = useState('')
  const [taskTitle, setTaskTitle] = useState('')
  const [taskDueAt, setTaskDueAt] = useState('')
  const [taskKind, setTaskKind] = useState<
    'priority' | 'deadline' | 'recurring'
  >('priority')
  const [taskPriority, setTaskPriority] = useState<
    'low' | 'normal' | 'high'
  >('normal')
  const [recurrenceFrequency, setRecurrenceFrequency] = useState<
    'daily' | 'weekly' | 'monthly'
  >('daily')
  const [recurrenceInterval, setRecurrenceInterval] = useState('1')
  const [questionnaireVersionId, setQuestionnaireVersionId] = useState('')
  const visibleTasks = useMemo(
    () => filterTasks(tasks, filter),
    [filter, tasks],
  )
  const selectedTask = tasks.find(
    (task) => task.wire.id === selectedTaskId,
  )

  const createTopic = async (event: FormEvent) => {
    event.preventDefault()
    await onCreateTopic(topicName)
    setTopicName('')
  }

  const createList = async (event: FormEvent) => {
    event.preventDefault()
    await onCreateList(listName)
    setListName('')
  }

  const createTask = async (event: FormEvent) => {
    event.preventDefault()
    const common = {
      title: taskTitle,
      questionnaireVersionId: questionnaireVersionId || undefined,
    }
    const dueAt = taskDueAt
      ? new Date(taskDueAt).toISOString()
      : ''
    const input: TaskCreationInput =
      taskKind === 'priority'
        ? { ...common, taskKind, priority: taskPriority }
        : taskKind === 'deadline'
          ? { ...common, taskKind, dueAt }
          : {
              ...common,
              taskKind,
              dueAt,
              frequency: recurrenceFrequency,
              interval: Number(recurrenceInterval),
            }
    await onCreateTask(input)
    setTaskTitle('')
    setTaskDueAt('')
    setTaskKind('priority')
    setRecurrenceFrequency('daily')
    setRecurrenceInterval('1')
    setQuestionnaireVersionId('')
  }

  if (!project) {
    return (
      <section className="screen-empty">
        <h2>No project selected</h2>
        <p>Create or select an encrypted project to load its resources.</p>
      </section>
    )
  }

  return (
    <div className="tasks-layout">
      <aside className="resource-tree" aria-label="Project resources">
        <div className="section-heading">
          <span>Topics</span>
        </div>
        <ul className="nav-list">
          {topics.map((topic) => (
            <li key={topic.wire.id}>
              <button
                type="button"
                className={
                  selectedTopicId === topic.wire.id ? 'active' : ''
                }
                onClick={() => onSelectTopic(topic.wire.id)}
              >
                {topic.document ? (
                  <span>{topic.document.name}</span>
                ) : (
                  <>
                    <LockIcon />
                    <span>Locked topic</span>
                  </>
                )}
                <ChevronIcon />
              </button>
            </li>
          ))}
        </ul>
        {topics.length === 0 && !loading && (
          <p className="inline-empty">No topics returned by the API.</p>
        )}

        <details className="create-panel">
          <summary>
            <PlusIcon />
            New topic
          </summary>
          <form onSubmit={(event) => void createTopic(event)}>
            <label>
              Topic name
              <input
                required
                value={topicName}
                onChange={(event) => setTopicName(event.target.value)}
              />
            </label>
            <button type="submit" className="secondary-button">
              Encrypt and create
            </button>
          </form>
        </details>

        {selectedTopicId && (
          <>
            <div className="section-heading list-tree-heading">
              <span>Task lists</span>
            </div>
            <ul className="subnav-list standalone">
              {taskLists.map((list) => (
                <li key={list.wire.id}>
                  <button
                    type="button"
                    className={
                      selectedListId === list.wire.id ? 'active' : ''
                    }
                    onClick={() => onSelectList(list.wire.id)}
                  >
                    {list.document?.name ?? 'Locked task list'}
                  </button>
                </li>
              ))}
            </ul>
            <details className="create-panel">
              <summary>
                <PlusIcon />
                New task list
              </summary>
              <form onSubmit={(event) => void createList(event)}>
                <label>
                  List name
                  <input
                    required
                    value={listName}
                    onChange={(event) => setListName(event.target.value)}
                  />
                </label>
                <button type="submit" className="secondary-button">
                  Encrypt and create
                </button>
              </form>
            </details>
          </>
        )}
      </aside>

      <section className="task-workspace" aria-labelledby="tasks-heading">
        <div className="screen-heading">
          <div>
            <p className="eyebrow">Encrypted task list</p>
            <h2 id="tasks-heading">
              {taskLists.find((list) => list.wire.id === selectedListId)
                ?.document?.name ?? 'Choose a task list'}
            </h2>
          </div>
        </div>

        <div className="filter-tabs" aria-label="Filter tasks">
          {filters.map((item) => (
            <button
              type="button"
              key={item.value}
              className={filter === item.value ? 'active' : ''}
              aria-pressed={filter === item.value}
              onClick={() => onFilter(item.value)}
            >
              {item.label}
            </button>
          ))}
        </div>

        {selectedListId && (
          <details className="create-panel task-create">
            <summary>
              <PlusIcon />
              Add encrypted task
            </summary>
            <form onSubmit={(event) => void createTask(event)}>
              <label>
                Title
                <input
                  required
                  value={taskTitle}
                  onChange={(event) => setTaskTitle(event.target.value)}
                />
              </label>
              <label>
                Task type
                <select
                  value={taskKind}
                  onChange={(event) =>
                    setTaskKind(
                      event.target.value as
                        | 'priority'
                        | 'deadline'
                        | 'recurring',
                    )
                  }
                >
                  <option value="priority">Priority</option>
                  <option value="deadline">Deadline</option>
                  <option value="recurring">Recurring</option>
                </select>
              </label>
              {taskKind === 'priority' ? (
                <label>
                  Priority
                  <select
                    value={taskPriority}
                    onChange={(event) =>
                      setTaskPriority(
                        event.target.value as 'low' | 'normal' | 'high',
                      )
                    }
                  >
                    <option value="low">Low</option>
                    <option value="normal">Normal</option>
                    <option value="high">High</option>
                  </select>
                </label>
              ) : (
                <label>
                  {taskKind === 'recurring' ? 'First occurrence' : 'Due'}
                  <input
                    required
                    type="datetime-local"
                    value={taskDueAt}
                    onChange={(event) => setTaskDueAt(event.target.value)}
                  />
                </label>
              )}
              {taskKind === 'recurring' && (
                <>
                  <label>
                    Repeat every
                    <input
                      required
                      type="number"
                      min="1"
                      step="1"
                      value={recurrenceInterval}
                      onChange={(event) =>
                        setRecurrenceInterval(event.target.value)
                      }
                    />
                  </label>
                  <label>
                    Recurrence unit
                    <select
                      value={recurrenceFrequency}
                      onChange={(event) =>
                        setRecurrenceFrequency(
                          event.target.value as
                            | 'daily'
                            | 'weekly'
                            | 'monthly',
                        )
                      }
                    >
                      <option value="daily">Day</option>
                      <option value="weekly">Week</option>
                      <option value="monthly">Month</option>
                    </select>
                  </label>
                </>
              )}
              <label>
                Published questionnaire
                <select
                  value={questionnaireVersionId}
                  onChange={(event) =>
                    setQuestionnaireVersionId(event.target.value)
                  }
                >
                  <option value="">None</option>
                  {publishedQuestionnaireVersions.map((version) => (
                    <option key={version.id} value={version.id}>
                      {version.label}
                    </option>
                  ))}
                </select>
              </label>
              <button type="submit" className="secondary-button">
                Encrypt and create
              </button>
            </form>
          </details>
        )}

        {loading && <div className="loading-state">Loading ciphertext…</div>}
        {!loading && selectedListId && visibleTasks.length === 0 && (
          <div className="screen-empty compact-empty">
            <h3>No tasks in this view</h3>
            <p>Filters run locally after successful decryption.</p>
          </div>
        )}

        <ul className="task-list">
          {visibleTasks.map((task) => {
            const overdue = isTaskOverdue(task)
            const overdueRecurring = isRecurringTaskOverdue(task)
            return (
              <li
                key={task.wire.id}
                className={[
                  selectedTaskId === task.wire.id ? 'selected' : '',
                  overdueRecurring ? 'overdue-recurring' : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
              >
                <button
                  type="button"
                  className="task-row"
                  onClick={() => onSelectTask(task.wire.id)}
                >
                  <span className="task-title-row">
                    <strong>{task.document.title}</strong>
                    {task.document.recurrence && <RepeatIcon />}
                  </span>
                  <span className={overdue ? 'task-due is-overdue' : 'task-due'}>
                    {task.document.due_at
                      ? formatDueDate(task.document.due_at)
                      : 'No due date'}
                  </span>
                  {overdueRecurring && (
                    <span className="recurrence-warning">
                      Overdue recurring item; no silent advancement
                    </span>
                  )}
                </button>
              </li>
            )
          })}
          {lockedTasks.map((task) => (
            <li key={task.id} className="locked-row">
              <LockIcon />
              <span>
                Locked task
                <small>{task.id}</small>
              </span>
            </li>
          ))}
        </ul>
      </section>

      <aside className="detail-panel" aria-label="Task detail">
        {selectedTask ? (
          <>
            <p className="eyebrow">Decrypted in memory</p>
            <h2>{selectedTask.document.title}</h2>
            <dl className="task-metadata">
              <div>
                <dt>State</dt>
                <dd>{selectedTask.wire.state.state}</dd>
              </div>
              <div>
                <dt>Version</dt>
                <dd>{selectedTask.wire.payload_version}</dd>
              </div>
              <div>
                <dt>Type</dt>
                <dd>{selectedTask.wire.task_kind}</dd>
              </div>
              {selectedTask.wire.task_kind === 'priority' && (
                <div>
                  <dt>Priority</dt>
                  <dd>{selectedTask.document.priority}</dd>
                </div>
              )}
              {selectedTask.wire.task_kind !== 'priority' && (
                <div>
                  <dt>
                    {selectedTask.wire.task_kind === 'recurring'
                      ? 'Next occurrence'
                      : 'Due'}
                  </dt>
                  <dd>
                    {selectedTask.document.due_at
                      ? formatDueDate(selectedTask.document.due_at)
                      : 'Unavailable'}
                  </dd>
                </div>
              )}
            </dl>
            <section className="detail-section">
              <h3>Private notes</h3>
              <p>{selectedTask.document.notes ?? 'No notes.'}</p>
            </section>
            <section className="detail-section">
              <h3>Edit</h3>
              <EditTaskForm
                key={selectedTask.wire.id}
                task={selectedTask}
                onUpdate={onUpdateTask}
              />
            </section>
            <section className="detail-section">
              <h3>Lifecycle</h3>
              {selectedTask.wire.state.state === 'open' ? (
                <button
                  type="button"
                  className="primary-button"
                  disabled={!selectedTask.wire.active_assignment_id}
                  onClick={() => void onCompleteTask(selectedTask)}
                >
                  Complete as assignee
                </button>
              ) : (
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => void onCopyTask(selectedTask)}
                >
                  Copy as a new open task
                </button>
              )}
            </section>
            <section className="security-limitations detail-section">
              <h3>Metadata still visible</h3>
              <p>
                The service can observe resource identifiers, versions, sizes,
                timing, membership, and network metadata.
              </p>
            </section>
          </>
        ) : (
          <div className="screen-empty compact-empty">
            <h3>No task selected</h3>
            <p>Choose a decrypted task to inspect it.</p>
          </div>
        )}
      </aside>
    </div>
  )
}
