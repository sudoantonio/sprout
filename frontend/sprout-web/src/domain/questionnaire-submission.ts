import { ApiError } from '../api/client'
import type {
  FinalizeQuestionnaireSubmissionRequest,
  QuestionnaireSubmissionResponse,
  Uuid,
} from '../api/contracts'

interface QuestionnaireSubmissionApi {
  submitQuestionnaire(
    projectId: Uuid,
    taskId: Uuid,
    request: FinalizeQuestionnaireSubmissionRequest,
  ): Promise<QuestionnaireSubmissionResponse>
  getQuestionnaireSubmission(
    projectId: Uuid,
    taskId: Uuid,
  ): Promise<QuestionnaireSubmissionResponse>
}

/**
 * A successful submission response can be lost after the server commits it.
 * Only a transport failure is ambiguous: resolve it with an authoritative read
 * and accept the result solely when it is the exact submitted draft.
 */
export const submitQuestionnaireRecoveringLostResponse = async (
  api: QuestionnaireSubmissionApi,
  projectId: Uuid,
  taskId: Uuid,
  submissionId: Uuid,
  request: FinalizeQuestionnaireSubmissionRequest,
): Promise<QuestionnaireSubmissionResponse> => {
  try {
    return await api.submitQuestionnaire(projectId, taskId, request)
  } catch (error) {
    if (!(error instanceof ApiError && error.status === 0)) {
      throw error
    }
    try {
      const authoritative = await api.getQuestionnaireSubmission(
        projectId,
        taskId,
      )
      if (
        authoritative.submission.id === submissionId &&
        authoritative.submission.state === 'submitted'
      ) {
        return authoritative
      }
    } catch {
      // Preserve the original ambiguous transport failure. The caller can
      // safely retry with the stable submission-scoped idempotency key.
    }
    throw error
  }
}
