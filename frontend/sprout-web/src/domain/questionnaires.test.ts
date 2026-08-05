import { describe, expect, it } from 'vitest'
import type {
  EncryptedPayloadDto,
  QuestionnaireVersionDto,
  TaskDto,
} from '../api/contracts'
import type { DecryptedQuestionnaireVersion } from './models'
import {
  buildAssigneeSubmissionRequest,
  selectImmutableQuestionnaireVersion,
  validateQuestionnaireAnswers,
  type QuestionnaireAnswerValue,
} from './questionnaires'

const encrypted: EncryptedPayloadDto = {
  version: 1,
  algorithm: 'sprout-protocol-v1',
  key_id: crypto.randomUUID(),
  nonce_b64: 'bm9uY2U=',
  ciphertext_b64: 'Y2lwaGVydGV4dA==',
}

const version = (
  id: string,
  number: number,
  state: 'draft' | 'published',
): QuestionnaireVersionDto => ({
  id,
  questionnaire_id: crypto.randomUUID(),
  project_id: crypto.randomUUID(),
  number,
  source_version_id: null,
  schema: encrypted,
  questions: [],
  revision: 0,
  state,
  created_at: new Date().toISOString(),
  published_at: state === 'published' ? new Date().toISOString() : null,
})

describe('questionnaire task pinning', () => {
  it('selects the immutable task pin instead of a newer version', () => {
    const pinnedId = crypto.randomUUID()
    const pinned = version(pinnedId, 1, 'published')
    const newer = version(crypto.randomUUID(), 2, 'published')

    expect(
      selectImmutableQuestionnaireVersion([newer, pinned], pinnedId),
    ).toBe(pinned)
  })

  it('rejects a draft even when its ID matches the task pin', () => {
    const pinnedId = crypto.randomUUID()
    expect(() =>
      selectImmutableQuestionnaireVersion(
        [version(pinnedId, 2, 'draft')],
        pinnedId,
      ),
    ).toThrow(/published/)
  })
})

describe('assignee submission request construction', () => {
  it('takes assignment and version IDs only from the active task', () => {
    const identityId = crypto.randomUUID()
    const assignmentId = crypto.randomUUID()
    const questionnaireVersionId = crypto.randomUUID()
    const task = {
      id: crypto.randomUUID(),
      project_id: crypto.randomUUID(),
      list_id: crypto.randomUUID(),
      resource_node_id: crypto.randomUUID(),
      task_kind: 'deadline',
      payload: encrypted,
      selected_value_snapshot: encrypted,
      state: { state: 'open' },
      source_pretask_id: null,
      preset_assignment_id: null,
      copied_from_task_id: null,
      questionnaire_version_id: questionnaireVersionId,
      recurrence_series_id: null,
      occurrence_number: null,
      active_assignment_id: assignmentId,
      active_assignee_identity_id: identityId,
      created_at: new Date().toISOString(),
      payload_version: 1,
      key_epoch: 1,
    } satisfies TaskDto

    const request = buildAssigneeSubmissionRequest({
      task,
      identityId,
      submissionId: crypto.randomUUID(),
      expectedRevision: null,
      encryptedPayload: encrypted,
      answers: [],
      idempotencyKey: crypto.randomUUID(),
    })

    expect(request.assignment_id).toBe(assignmentId)
    expect(request.questionnaire_version_id).toBe(questionnaireVersionId)
  })

  it('rejects a caller who is not the active assignee', () => {
    const task = {
      active_assignment_id: crypto.randomUUID(),
      active_assignee_identity_id: crypto.randomUUID(),
      questionnaire_version_id: crypto.randomUUID(),
    } as TaskDto
    expect(() =>
      buildAssigneeSubmissionRequest({
        task,
        identityId: crypto.randomUUID(),
        submissionId: crypto.randomUUID(),
        expectedRevision: null,
        encryptedPayload: encrypted,
        answers: [],
        idempotencyKey: crypto.randomUUID(),
      }),
    ).toThrow(/active assignee/)
  })
})

describe('question type validation (T-LLR-04.2)', () => {
  const optionA = crypto.randomUUID()
  const optionB = crypto.randomUUID()
  const questions: DecryptedQuestionnaireVersion['questions'] = [
    {
      id: crypto.randomUUID(),
      questionKind: 'open',
      ordinal: 0,
      required: true,
      prompt: 'Explain',
      options: [],
    },
    {
      id: crypto.randomUUID(),
      questionKind: 'single_choice',
      ordinal: 1,
      required: true,
      prompt: 'Choose one',
      options: [
        { id: optionA, ordinal: 0, label: 'A' },
        { id: optionB, ordinal: 1, label: 'B' },
      ],
    },
    {
      id: crypto.randomUUID(),
      questionKind: 'multiple_choice',
      ordinal: 2,
      required: true,
      prompt: 'Choose many',
      options: [
        { id: optionA, ordinal: 0, label: 'A' },
        { id: optionB, ordinal: 1, label: 'B' },
      ],
    },
    {
      id: crypto.randomUUID(),
      questionKind: 'boolean',
      ordinal: 3,
      required: true,
      prompt: 'Confirm',
      options: [],
    },
  ]
  const questionnaireVersion = {
    wire: version(crypto.randomUUID(), 1, 'published'),
    document: { schema: 1 },
    questions,
  } satisfies DecryptedQuestionnaireVersion

  it('accepts valid open, single, multiple, and boolean answers', () => {
    expect(
      validateQuestionnaireAnswers(questionnaireVersion, {
        [questions[0].id]: 'Details',
        [questions[1].id]: optionA,
        [questions[2].id]: [optionA, optionB],
        [questions[3].id]: false,
      }),
    ).toHaveLength(4)
  })

  const invalidAnswerCases: Array<
    [Record<string, QuestionnaireAnswerValue>, RegExp]
  > = [
    [{}, /before submitting/],
    [
      {
        [questions[0].id]: 'Details',
        [questions[1].id]: crypto.randomUUID(),
        [questions[2].id]: [optionA],
        [questions[3].id]: true,
      },
      /valid option/,
    ],
    [
      {
        [questions[0].id]: 'Details',
        [questions[1].id]: optionA,
        [questions[2].id]: [optionA, optionA],
        [questions[3].id]: true,
      },
      /valid unique options/,
    ],
  ]

  it.each(invalidAnswerCases)('rejects incompatible answer sets', (answers, message) => {
    expect(() =>
      validateQuestionnaireAnswers(questionnaireVersion, answers),
    ).toThrow(message)
  })
})
