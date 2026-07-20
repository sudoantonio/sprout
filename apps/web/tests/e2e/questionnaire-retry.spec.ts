import { expect, test } from '@playwright/test'

test('T-LLR-04.4 recovers a committed submission after response loss', async ({
  page,
}) => {
  const projectId = '04400000-0000-4000-8000-000000000001'
  const taskId = '04400000-0000-4000-8000-000000000002'
  const submissionId = '04400000-0000-4000-8000-000000000003'
  let committed = false
  let submitCount = 0
  const authorization: string[] = []

  await page.route('**/v1/projects/**', async (route) => {
    const request = route.request()
    authorization.push(request.headers().authorization ?? '')
    if (
      request.method() === 'POST' &&
      new URL(request.url()).pathname.endsWith('/submit')
    ) {
      submitCount += 1
      const body = request.postDataJSON() as { idempotency_key: string }
      expect(body.idempotency_key).toBe(submissionId)
      committed = true
      await route.abort('connectionreset')
      return
    }
    if (request.method() === 'GET' && committed) {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          submission: {
            id: submissionId,
            project_id: projectId,
            task_id: taskId,
            state: 'submitted',
            revision: 2,
          },
          answers: [],
        }),
      })
      return
    }
    await route.fulfill({ status: 404 })
  })

  await page.goto('/')
  const recovered = await page.evaluate(
    async ({ projectId, taskId, submissionId }) => {
      const apiPath = '/src/api/client.ts'
      const recoveryPath = '/src/domain/questionnaire-submission.ts'
      const { ApiClient } = await import(/* @vite-ignore */ apiPath)
      const { submitQuestionnaireRecoveringLostResponse } = await import(
        /* @vite-ignore */ recoveryPath
      )
      const api = new ApiClient()
      api.setSession('authenticated-assignee-session')
      const result = await submitQuestionnaireRecoveringLostResponse(
        api,
        projectId,
        taskId,
        submissionId,
        {
          expected_revision: 1,
          signer_device_key_version: 1,
          classical_signature_b64: 'Y2xhc3NpY2Fs',
          post_quantum_signature_b64: 'cG9zdC1xdWFudHVt',
          idempotency_key: submissionId,
        },
      )
      return result.submission
    },
    { projectId, taskId, submissionId },
  )

  expect(recovered).toMatchObject({
    id: submissionId,
    state: 'submitted',
    revision: 2,
  })
  expect(submitCount).toBe(1)
  expect(authorization).toEqual([
    'Bearer authenticated-assignee-session',
    'Bearer authenticated-assignee-session',
  ])
})
