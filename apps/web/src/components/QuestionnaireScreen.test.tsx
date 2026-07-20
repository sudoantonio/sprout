import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type {
  QuestionnaireSubmissionDto,
  Uuid,
} from '../api/contracts'
import type {
  DecryptedQuestionnaireVersion,
  DecryptedTask,
} from '../domain/models'
import { QuestionnaireScreen } from './QuestionnaireScreen'

describe('historical questionnaire submission', () => {
  it('hydrates encrypted-answer results into a read-only submitted form', () => {
    const questionId = crypto.randomUUID()
    const versionId = crypto.randomUUID()
    const taskId = crypto.randomUUID()
    const version = {
      wire: { id: versionId },
      document: { schema: 1 },
      questions: [
        {
          id: questionId,
          questionKind: 'open',
          ordinal: 0,
          required: true,
          prompt: 'Historical answer',
          options: [],
        },
      ],
    } as unknown as DecryptedQuestionnaireVersion
    const task = {
      wire: {
        id: taskId,
        questionnaire_version_id: versionId,
      },
      document: { schema: 1, title: 'Pinned questionnaire' },
    } as unknown as DecryptedTask
    const submission = {
      task_id: taskId,
      state: 'submitted',
      revision: 2,
    } as QuestionnaireSubmissionDto

    render(
      <QuestionnaireScreen
        questionnaires={[]}
        versions={[]}
        assigneeTasks={[task]}
        taskVersion={version}
        submission={submission}
        submissionAnswers={{
          [questionId as Uuid]: 'Faithfully restored',
        }}
        onRefresh={vi.fn()}
        onCreate={vi.fn()}
        onSelect={vi.fn()}
        onSaveVersion={vi.fn()}
        onPublish={vi.fn()}
        onLoadTask={vi.fn()}
        onSubmitTask={vi.fn()}
      />,
    )

    const answer = screen.getByDisplayValue('Faithfully restored')
    expect(answer).toBeDisabled()
    expect(
      screen.getByRole('button', { name: 'Encrypt, sign, and submit' }),
    ).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent(
      'Submission submitted · revision 2',
    )
  })
})
