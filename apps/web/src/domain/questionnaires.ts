import type {
  EncryptedPayloadDto,
  QuestionnaireAnswerDto,
  QuestionnaireQuestionDto,
  QuestionnaireVersionDto,
  TaskDto,
  UpsertQuestionnaireDraftRequest,
  Uuid,
} from '../api/contracts'
import type { KeyVault } from '../security/key-vault'
import {
  bytesToBase64,
  decryptDocument,
  encryptDocument,
  zeroBytes,
} from '../security/wasm'
import type {
  DecryptedQuestionnaireVersion,
  QuestionnaireOptionDocument,
  QuestionnaireQuestionDocument,
  QuestionnaireVersionDocument,
} from './models'

export interface QuestionnaireEditorQuestion {
  id?: Uuid
  prompt: string
  questionKind: QuestionnaireQuestionDto['question_kind']
  required: boolean
  options: Array<{ id?: Uuid; label: string }>
}

export type QuestionnaireAnswerValue = string | string[] | boolean

export const validateQuestionnaireAnswers = (
  version: DecryptedQuestionnaireVersion,
  values: Record<Uuid, QuestionnaireAnswerValue>,
): Array<{
  question: DecryptedQuestionnaireVersion['questions'][number]
  value: QuestionnaireAnswerValue
}> => {
  const knownQuestionIds = new Set(
    version.questions.map((question) => question.id),
  )
  if (Object.keys(values).some((id) => !knownQuestionIds.has(id))) {
    throw new Error('Questionnaire answers contain an unknown question')
  }
  return version.questions.flatMap((question) => {
    const value = values[question.id]
    const optionIds = new Set(question.options.map((option) => option.id))
    const answered =
      typeof value === 'boolean' ||
      (typeof value === 'string' && value.trim().length > 0) ||
      (Array.isArray(value) && value.length > 0)
    if (!answered) {
      if (question.required) {
        throw new Error(`Answer “${question.prompt}” before submitting`)
      }
      return []
    }
    if (question.questionKind === 'open' && typeof value !== 'string') {
      throw new Error(`Answer “${question.prompt}” with text`)
    }
    if (question.questionKind === 'boolean' && typeof value !== 'boolean') {
      throw new Error(`Answer “${question.prompt}” with yes or no`)
    }
    if (
      question.questionKind === 'single_choice' &&
      (typeof value !== 'string' || !optionIds.has(value))
    ) {
      throw new Error(`Choose one valid option for “${question.prompt}”`)
    }
    if (question.questionKind === 'multiple_choice') {
      if (
        !Array.isArray(value) ||
        new Set(value).size !== value.length ||
        value.some((id) => !optionIds.has(id))
      ) {
        throw new Error(`Choose valid unique options for “${question.prompt}”`)
      }
    }
    return [{ question, value }]
  })
}

const asArrayBuffer = (value: Uint8Array): ArrayBuffer =>
  value.buffer.slice(
    value.byteOffset,
    value.byteOffset + value.byteLength,
  ) as ArrayBuffer

const requireQuestionnaireKey = (
  vault: KeyVault,
  questionnaireId: Uuid,
): Uint8Array => {
  const key = vault.getResourceKey(questionnaireId)
  if (!key) {
    throw new Error('This questionnaire key is not available on this device')
  }
  return key
}

const encryptVersionDocument = <T>(
  vault: KeyVault,
  projectId: Uuid,
  questionnaireId: Uuid,
  versionId: Uuid,
  document: T,
): Promise<EncryptedPayloadDto> =>
  encryptDocument(document, {
    projectId,
    resourceId: versionId,
    keyId: crypto.randomUUID(),
    kind: 'questionnaire',
    aggregateVersion: 0,
    keyEpoch: 1,
    resourceKey: requireQuestionnaireKey(vault, questionnaireId),
  })

const decryptVersionDocument = <T>(
  vault: KeyVault,
  projectId: Uuid,
  questionnaireId: Uuid,
  versionId: Uuid,
  payload: EncryptedPayloadDto,
): Promise<T> =>
  decryptDocument<T>(payload, {
    projectId,
    resourceId: versionId,
    kind: 'questionnaire',
    aggregateVersion: 0,
    keyEpoch: 1,
    resourceKey: requireQuestionnaireKey(vault, questionnaireId),
  })

export const encryptQuestionnaireVersion = async (
  vault: KeyVault,
  input: {
    projectId: Uuid
    questionnaireId: Uuid
    versionId: Uuid
    description?: string
    questions: QuestionnaireEditorQuestion[]
    preserveIds: boolean
  },
): Promise<{
  schema: EncryptedPayloadDto
  questions: QuestionnaireQuestionDto[]
  contentHashB64: string
}> => {
  if (input.questions.length === 0) {
    throw new Error('A questionnaire version needs at least one question')
  }
  const schema = await encryptVersionDocument(
    vault,
    input.projectId,
    input.questionnaireId,
    input.versionId,
    {
      schema: 1,
      description: input.description || undefined,
    } satisfies QuestionnaireVersionDocument,
  )
  const questions = await Promise.all(
    input.questions.map(async (question, ordinal) => {
      const id =
        input.preserveIds && question.id ? question.id : crypto.randomUUID()
      const requiresOptions =
        question.questionKind === 'single_choice' ||
        question.questionKind === 'multiple_choice'
      const optionInputs = requiresOptions
        ? question.options.filter((option) => option.label.trim())
        : []
      if (requiresOptions && optionInputs.length === 0) {
        throw new Error('Choice questions need at least one option')
      }
      return {
        id,
        question_kind: question.questionKind,
        ordinal,
        required: question.required,
        payload: await encryptVersionDocument(
          vault,
          input.projectId,
          input.questionnaireId,
          input.versionId,
          {
            schema: 1,
            prompt: question.prompt,
          } satisfies QuestionnaireQuestionDocument,
        ),
        options: await Promise.all(
          optionInputs.map(async (option, optionOrdinal) => ({
            id:
              input.preserveIds && option.id
                ? option.id
                : crypto.randomUUID(),
            ordinal: optionOrdinal,
            payload: await encryptVersionDocument(
              vault,
              input.projectId,
              input.questionnaireId,
              input.versionId,
              {
                schema: 1,
                label: option.label,
              } satisfies QuestionnaireOptionDocument,
            ),
          })),
        ),
      } satisfies QuestionnaireQuestionDto
    }),
  )
  const encoded = new TextEncoder().encode(JSON.stringify({ schema, questions }))
  let digest: ArrayBuffer | undefined
  try {
    digest = await crypto.subtle.digest('SHA-256', asArrayBuffer(encoded))
    return {
      schema,
      questions,
      contentHashB64: bytesToBase64(new Uint8Array(digest)),
    }
  } finally {
    zeroBytes(encoded, digest ? new Uint8Array(digest) : undefined)
  }
}

export const decryptQuestionnaireVersion = async (
  vault: KeyVault,
  version: QuestionnaireVersionDto,
): Promise<DecryptedQuestionnaireVersion> => ({
  wire: version,
  document: await decryptVersionDocument<QuestionnaireVersionDocument>(
    vault,
    version.project_id,
    version.questionnaire_id,
    version.id,
    version.schema,
  ),
  questions: await Promise.all(
    version.questions.map(async (question) => ({
      id: question.id,
      questionKind: question.question_kind,
      ordinal: question.ordinal,
      required: question.required,
      prompt: (
        await decryptVersionDocument<QuestionnaireQuestionDocument>(
          vault,
          version.project_id,
          version.questionnaire_id,
          version.id,
          question.payload,
        )
      ).prompt,
      options: await Promise.all(
        question.options.map(async (option) => ({
          id: option.id,
          ordinal: option.ordinal,
          label: (
            await decryptVersionDocument<QuestionnaireOptionDocument>(
              vault,
              version.project_id,
              version.questionnaire_id,
              version.id,
              option.payload,
            )
          ).label,
        })),
      ),
    })),
  ),
})

export const selectImmutableQuestionnaireVersion = (
  versions: QuestionnaireVersionDto[],
  pinnedVersionId: Uuid,
): QuestionnaireVersionDto => {
  const pinned = versions.find((version) => version.id === pinnedVersionId)
  if (!pinned || pinned.state !== 'published') {
    throw new Error('The task-pinned published questionnaire version is unavailable')
  }
  return pinned
}

export const buildAssigneeSubmissionRequest = (input: {
  task: TaskDto
  identityId: Uuid
  submissionId: Uuid
  expectedRevision: number | null
  encryptedPayload: EncryptedPayloadDto
  answers: QuestionnaireAnswerDto[]
  idempotencyKey: Uuid
}): UpsertQuestionnaireDraftRequest => {
  if (
    !input.task.active_assignment_id ||
    input.task.active_assignee_identity_id !== input.identityId
  ) {
    throw new Error('Only the active assignee can submit this questionnaire')
  }
  if (!input.task.questionnaire_version_id) {
    throw new Error('This task does not pin a questionnaire version')
  }
  return {
    submission_id: input.submissionId,
    assignment_id: input.task.active_assignment_id,
    questionnaire_version_id: input.task.questionnaire_version_id,
    expected_revision: input.expectedRevision,
    encrypted_payload: input.encryptedPayload,
    answers: input.answers,
    idempotency_key: input.idempotencyKey,
  }
}

export const questionnaireSubmissionSigningMessage = (input: {
  projectId: Uuid
  taskId: Uuid
  submissionId: Uuid
  expectedRevision: number
}): Uint8Array =>
  new TextEncoder().encode(
    `sprout/questionnaire-submission/v1/${input.projectId}/${input.taskId}/${input.submissionId}/${input.expectedRevision}`,
  )

export const QUESTIONNAIRE_SUBMISSION_SIGNATURE_CONTEXT =
  'sprout-questionnaire-submission-v1'
