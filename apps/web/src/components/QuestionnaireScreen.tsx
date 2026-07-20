import { useEffect, useMemo, useState, type FormEvent } from 'react'
import type {
  QuestionnaireDto,
  QuestionnaireSubmissionDto,
  Uuid,
} from '../api/contracts'
import type {
  DecryptedQuestionnaireVersion,
  DecryptedTask,
  QuestionnaireDocument,
} from '../domain/models'
import type { QuestionnaireEditorQuestion } from '../domain/questionnaires'
import { LockIcon, PlusIcon, ShieldIcon } from './icons'

interface QuestionnaireItem {
  wire: QuestionnaireDto
  document?: QuestionnaireDocument
  lockedReason?: string
}

export type QuestionnaireAnswerValue = string | string[] | boolean

interface QuestionnaireScreenProps {
  questionnaires: QuestionnaireItem[]
  versions: DecryptedQuestionnaireVersion[]
  selectedQuestionnaireId?: Uuid
  assigneeTasks: DecryptedTask[]
  taskVersion?: DecryptedQuestionnaireVersion
  submission?: QuestionnaireSubmissionDto
  submissionAnswers: Record<Uuid, QuestionnaireAnswerValue>
  onRefresh(): Promise<void>
  onCreate(title: string): Promise<void>
  onSelect(questionnaireId: Uuid): Promise<void>
  onSaveVersion(input: {
    draft?: DecryptedQuestionnaireVersion
    sourceVersionId?: Uuid
    description?: string
    questions: QuestionnaireEditorQuestion[]
  }): Promise<void>
  onPublish(version: DecryptedQuestionnaireVersion): Promise<void>
  onLoadTask(taskId: Uuid): Promise<void>
  onSubmitTask(
    task: DecryptedTask,
    version: DecryptedQuestionnaireVersion,
    answers: Record<Uuid, QuestionnaireAnswerValue>,
  ): Promise<void>
}

const blankQuestion = (): QuestionnaireEditorQuestion => ({
  prompt: '',
  questionKind: 'open',
  required: false,
  options: [],
})

const editorQuestions = (
  source?: DecryptedQuestionnaireVersion,
): QuestionnaireEditorQuestion[] =>
  source?.questions.map((question) => ({
    id: question.id,
    prompt: question.prompt,
    questionKind: question.questionKind,
    required: question.required,
    options: question.options.map((option) => ({
      id: option.id,
      label: option.label,
    })),
  })) ?? [blankQuestion()]

const VersionEditor = ({
  draft,
  source,
  onSave,
}: {
  draft?: DecryptedQuestionnaireVersion
  source?: DecryptedQuestionnaireVersion
  onSave(input: {
    draft?: DecryptedQuestionnaireVersion
    sourceVersionId?: Uuid
    description?: string
    questions: QuestionnaireEditorQuestion[]
  }): Promise<void>
}) => {
  const initial = draft ?? source
  const [description, setDescription] = useState(
    initial?.document.description ?? '',
  )
  const [questions, setQuestions] = useState(() => editorQuestions(initial))

  const updateQuestion = (
    index: number,
    update: Partial<QuestionnaireEditorQuestion>,
  ) =>
    setQuestions((current) =>
      current.map((question, position) =>
        position === index ? { ...question, ...update } : question,
      ),
    )

  return (
    <form
      className="panel-form questionnaire-editor"
      onSubmit={(event) => {
        event.preventDefault()
        void onSave({
          draft,
          sourceVersionId: draft ? undefined : source?.wire.id,
          description: description || undefined,
          questions,
        })
      }}
    >
      <h3>{draft ? `Edit draft v${draft.wire.number}` : 'Create version draft'}</h3>
      <p>
        Prompts, option labels, and the schema are encrypted on this device.
        Published versions can only be used as a source for a new draft.
      </p>
      <label>
        Private description
        <textarea
          value={description}
          onChange={(event) => setDescription(event.target.value)}
        />
      </label>
      {questions.map((question, index) => (
        <fieldset className="question-editor-row" key={question.id ?? index}>
          <legend>Question {index + 1}</legend>
          <label>
            Prompt
            <input
              required
              value={question.prompt}
              onChange={(event) =>
                updateQuestion(index, { prompt: event.target.value })
              }
            />
          </label>
          <label>
            Type
            <select
              value={question.questionKind}
              onChange={(event) =>
                updateQuestion(index, {
                  questionKind: event.target
                    .value as QuestionnaireEditorQuestion['questionKind'],
                  options: [],
                })
              }
            >
              <option value="open">Open response</option>
              <option value="single_choice">Single choice</option>
              <option value="multiple_choice">Multiple choice</option>
              <option value="boolean">Yes / no</option>
            </select>
          </label>
          {(question.questionKind === 'single_choice' ||
            question.questionKind === 'multiple_choice') && (
            <label>
              Options, one per line
              <textarea
                required
                value={question.options.map((option) => option.label).join('\n')}
                onChange={(event) => {
                  const prior = question.options
                  updateQuestion(index, {
                    options: event.target.value.split('\n').map((label, optionIndex) => ({
                      id: prior[optionIndex]?.id,
                      label,
                    })),
                  })
                }}
              />
            </label>
          )}
          <label className="check-label">
            <input
              type="checkbox"
              checked={question.required}
              onChange={(event) =>
                updateQuestion(index, { required: event.target.checked })
              }
            />
            Required
          </label>
          <button
            className="text-button"
            type="button"
            disabled={questions.length === 1}
            onClick={() =>
              setQuestions((current) =>
                current.filter((_, position) => position !== index),
              )
            }
          >
            Remove question
          </button>
        </fieldset>
      ))}
      <button
        className="secondary-button"
        type="button"
        onClick={() => setQuestions((current) => [...current, blankQuestion()])}
      >
        <PlusIcon />
        Add question
      </button>
      <button className="primary-button" type="submit">
        Encrypt and save draft
      </button>
    </form>
  )
}

const TaskQuestionnaire = ({
  tasks,
  version,
  submission,
  initialAnswers,
  onLoad,
  onSubmit,
}: {
  tasks: DecryptedTask[]
  version?: DecryptedQuestionnaireVersion
  submission?: QuestionnaireSubmissionDto
  initialAnswers: Record<Uuid, QuestionnaireAnswerValue>
  onLoad(taskId: Uuid): Promise<void>
  onSubmit(
    task: DecryptedTask,
    version: DecryptedQuestionnaireVersion,
    answers: Record<Uuid, QuestionnaireAnswerValue>,
  ): Promise<void>
}) => {
  const [taskId, setTaskId] = useState('')
  const [answers, setAnswers] = useState<
    Record<Uuid, QuestionnaireAnswerValue>
  >({})
  const task = tasks.find((candidate) => candidate.wire.id === taskId)

  useEffect(() => {
    setAnswers(initialAnswers)
  }, [initialAnswers])

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!task || !version) return
    await onSubmit(task, version, answers)
  }

  return (
    <form className="panel-form task-questionnaire" onSubmit={(event) => void submit(event)}>
      <h3>Complete an assigned task questionnaire</h3>
      <p>
        The exact published version pinned by the task is loaded. A newer
        questionnaire version is never substituted.
      </p>
      <label>
        Active assignment
        <select
          value={taskId}
          onChange={(event) => {
            const nextTaskId = event.target.value
            setTaskId(nextTaskId)
            setAnswers({})
            if (nextTaskId) void onLoad(nextTaskId)
          }}
        >
          <option value="">Choose an assigned task</option>
          {tasks.map((candidate) => (
            <option key={candidate.wire.id} value={candidate.wire.id}>
              {candidate.document.title}
            </option>
          ))}
        </select>
      </label>
      {version?.questions.map((question) => (
        <fieldset
          key={question.id}
          className="answer-field"
          disabled={submission?.state === 'submitted'}
        >
          <legend>
            {question.prompt}
            {question.required ? ' *' : ''}
          </legend>
          {question.questionKind === 'open' && (
            <textarea
              required={question.required}
              value={(answers[question.id] as string | undefined) ?? ''}
              onChange={(event) =>
                setAnswers((current) => ({
                  ...current,
                  [question.id]: event.target.value,
                }))
              }
            />
          )}
          {question.questionKind === 'boolean' && (
            <select
              required={question.required}
              value={
                answers[question.id] === undefined
                  ? ''
                  : String(answers[question.id])
              }
              onChange={(event) =>
                setAnswers((current) => ({
                  ...current,
                  [question.id]: event.target.value === 'true',
                }))
              }
            >
              <option value="">Choose</option>
              <option value="true">Yes</option>
              <option value="false">No</option>
            </select>
          )}
          {question.questionKind === 'single_choice' &&
            question.options.map((option) => (
              <label className="check-label" key={option.id}>
                <input
                  required={question.required}
                  type="radio"
                  name={question.id}
                  checked={answers[question.id] === option.id}
                  onChange={() =>
                    setAnswers((current) => ({
                      ...current,
                      [question.id]: option.id,
                    }))
                  }
                />
                {option.label}
              </label>
            ))}
          {question.questionKind === 'multiple_choice' &&
            question.options.map((option) => {
              const selected =
                (answers[question.id] as string[] | undefined) ?? []
              return (
                <label className="check-label" key={option.id}>
                  <input
                    type="checkbox"
                    checked={selected.includes(option.id)}
                    onChange={(event) =>
                      setAnswers((current) => ({
                        ...current,
                        [question.id]: event.target.checked
                          ? [...selected, option.id]
                          : selected.filter((id) => id !== option.id),
                      }))
                    }
                  />
                  {option.label}
                </label>
              )
            })}
        </fieldset>
      ))}
      {submission && (
        <p role="status">
          Submission {submission.state} · revision {submission.revision}
        </p>
      )}
      <button
        className="primary-button"
        type="submit"
        disabled={!task || !version || submission?.state === 'submitted'}
      >
        Encrypt, sign, and submit
      </button>
    </form>
  )
}

export const QuestionnaireScreen = ({
  questionnaires,
  versions,
  selectedQuestionnaireId,
  assigneeTasks,
  taskVersion,
  submission,
  submissionAnswers,
  onRefresh,
  onCreate,
  onSelect,
  onSaveVersion,
  onPublish,
  onLoadTask,
  onSubmitTask,
}: QuestionnaireScreenProps) => {
  const [title, setTitle] = useState('')
  const [editorVersionId, setEditorVersionId] = useState<Uuid>()
  const [sourceVersionId, setSourceVersionId] = useState<Uuid>()
  const draft = versions.find(
    (version) =>
      version.wire.id === editorVersionId && version.wire.state === 'draft',
  )
  const source = versions.find(
    (version) =>
      version.wire.id === sourceVersionId &&
      version.wire.state === 'published',
  )
  const editorKey = draft?.wire.id ?? source?.wire.id ?? 'new'
  const selectedTitle = useMemo(
    () =>
      questionnaires.find(
        (questionnaire) => questionnaire.wire.id === selectedQuestionnaireId,
      )?.document?.title,
    [questionnaires, selectedQuestionnaireId],
  )

  return (
    <section className="content-screen" aria-labelledby="questionnaires-title">
      <div className="screen-heading">
        <div>
          <p className="eyebrow">Immutable encrypted forms</p>
          <h2 id="questionnaires-title">Questionnaires</h2>
        </div>
        <button
          className="secondary-button inline-button"
          type="button"
          onClick={() => void onRefresh()}
        >
          Refresh
        </button>
      </div>
      <div className="resource-grid questionnaire-layout">
        <div className="panel-form">
          <h3>Questionnaire library</h3>
          <form
            className="inline-create-form"
            onSubmit={(event) => {
              event.preventDefault()
              void onCreate(title).then(() => setTitle(''))
            }}
          >
            <label>
              Private title
              <input
                required
                value={title}
                onChange={(event) => setTitle(event.target.value)}
              />
            </label>
            <button className="primary-button" type="submit">
              Encrypt and create
            </button>
          </form>
          <ul className="archive-list questionnaire-list">
            {questionnaires.map((questionnaire) => (
              <li key={questionnaire.wire.id}>
                <div>
                  <strong>
                    {questionnaire.document?.title ?? 'Locked questionnaire'}
                  </strong>
                  <small>
                    {questionnaire.wire.latest_version} version(s)
                  </small>
                </div>
                {questionnaire.document ? <ShieldIcon /> : <LockIcon />}
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => void onSelect(questionnaire.wire.id)}
                >
                  Manage
                </button>
              </li>
            ))}
          </ul>
        </div>
        <TaskQuestionnaire
          tasks={assigneeTasks}
          version={taskVersion}
          submission={submission}
          initialAnswers={submissionAnswers}
          onLoad={onLoadTask}
          onSubmit={onSubmitTask}
        />
      </div>

      {selectedQuestionnaireId && (
        <div className="questionnaire-version-workspace">
          <div className="screen-heading">
            <div>
              <p className="eyebrow">Version history</p>
              <h2>{selectedTitle ?? 'Locked questionnaire'}</h2>
            </div>
          </div>
          <ul className="archive-list version-list">
            {versions.map((version) => (
              <li key={version.wire.id}>
                <div>
                  <strong>Version {version.wire.number}</strong>
                  <small>
                    {version.questions.length} question(s) · revision{' '}
                    {version.wire.revision}
                  </small>
                </div>
                <span>{version.wire.state}</span>
                <div className="version-actions">
                  {version.wire.state === 'draft' ? (
                    <>
                      <button
                        className="secondary-button"
                        type="button"
                        onClick={() => {
                          setSourceVersionId(undefined)
                          setEditorVersionId(version.wire.id)
                        }}
                      >
                        Edit draft
                      </button>
                      <button
                        className="primary-button"
                        type="button"
                        onClick={() => void onPublish(version)}
                      >
                        Publish
                      </button>
                    </>
                  ) : (
                    <button
                      className="secondary-button"
                      type="button"
                      onClick={() => {
                        setEditorVersionId(undefined)
                        setSourceVersionId(version.wire.id)
                      }}
                    >
                      New draft from this version
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
          <VersionEditor
            key={editorKey}
            draft={draft}
            source={source}
            onSave={onSaveVersion}
          />
        </div>
      )}
    </section>
  )
}
