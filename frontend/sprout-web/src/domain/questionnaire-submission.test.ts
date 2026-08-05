import { describe, expect, it, vi } from 'vitest'
import { ApiError } from '../api/client'
import type {
  FinalizeQuestionnaireSubmissionRequest,
  QuestionnaireSubmissionResponse,
} from '../api/contracts'
import { submitQuestionnaireRecoveringLostResponse } from './questionnaire-submission'

const request = {
  expected_revision: 1,
  signer_device_key_version: 1,
  classical_signature_b64: 'Y2xhc3NpY2Fs',
  post_quantum_signature_b64: 'cG9zdC1xdWFudHVt',
  idempotency_key: crypto.randomUUID(),
} satisfies FinalizeQuestionnaireSubmissionRequest

const response = (
  id: string,
  state: 'draft' | 'submitted',
): QuestionnaireSubmissionResponse =>
  ({
    submission: { id, state },
    answers: [],
  }) as unknown as QuestionnaireSubmissionResponse

describe('questionnaire response-loss recovery', () => {
  it('accepts the authoritative submitted result after transport loss', async () => {
    const projectId = crypto.randomUUID()
    const taskId = crypto.randomUUID()
    const submissionId = crypto.randomUUID()
    const authoritative = response(submissionId, 'submitted')
    const api = {
      submitQuestionnaire: vi
        .fn()
        .mockRejectedValue(new ApiError(0, 'connection reset')),
      getQuestionnaireSubmission: vi.fn().mockResolvedValue(authoritative),
    }

    await expect(
      submitQuestionnaireRecoveringLostResponse(
        api,
        projectId,
        taskId,
        submissionId,
        request,
      ),
    ).resolves.toBe(authoritative)
    expect(api.getQuestionnaireSubmission).toHaveBeenCalledWith(
      projectId,
      taskId,
    )
  })

  it('preserves ambiguity when the authoritative draft does not match', async () => {
    const transport = new ApiError(0, 'connection reset')
    const api = {
      submitQuestionnaire: vi.fn().mockRejectedValue(transport),
      getQuestionnaireSubmission: vi
        .fn()
        .mockResolvedValue(response(crypto.randomUUID(), 'draft')),
    }

    await expect(
      submitQuestionnaireRecoveringLostResponse(
        api,
        crypto.randomUUID(),
        crypto.randomUUID(),
        crypto.randomUUID(),
        request,
      ),
    ).rejects.toBe(transport)
  })
})
