import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { expect, test } from '@playwright/test'

interface Hlt05Evidence {
  project_id: string
  task_id: string
  resource_id: string
  assignment_id: string
  preset_version_id: string
  pretask_id: string
  alice_identity_id: string
  alice_session: string
  second_device_id: string
  second_device_session: string
  resource_key_b64: string
  key_epoch: number
  encrypted_metadata: {
    version: number
    algorithm: string
    key_id: string
    nonce_b64: string
    ciphertext_b64: string
  }
}

const evidencePath =
  process.env.HLT05_EVIDENCE_PATH ?? '/evidence/hlt05.json'
const evidencePathWasConfigured = Boolean(process.env.HLT05_EVIDENCE_PATH)

test('HLT-05 real backend preserves provenance across offline sync and second-device read', async ({
  browser,
  browserName,
}) => {
  test.skip(browserName !== 'chromium', 'OPFS staging is exercised in Chromium')
  test.skip(
    !evidencePathWasConfigured && !existsSync(evidencePath),
    'Requires backend-generated HLT-05 evidence',
  )
  const evidence = JSON.parse(
    await readFile(evidencePath, 'utf8'),
  ) as Hlt05Evidence
  const first = await browser.newContext()
  const second = await browser.newContext()
  const firstPage = await first.newPage()
  const secondPage = await second.newPage()
  const templateAttachmentId = crypto.randomUUID()
  const templateBlobId = crypto.randomUUID()
  const requiredAttachmentId = crypto.randomUUID()
  const requiredBlobId = crypto.randomUUID()
  const completedAttachmentId = crypto.randomUUID()
  const completedBlobId = crypto.randomUUID()

  try {
    await firstPage.goto('/')
    await secondPage.goto('/')
    await firstPage.evaluate(async () => {
      await Promise.all([
        import('/src/api/client.ts'),
        import('/src/attachments/crypto.ts'),
        import('/src/attachments/offline-queue.ts'),
        import('/src/storage/opfs.ts'),
      ])
    })

    const provenance = await firstPage.evaluate(
      async ({
        evidence,
        templateAttachmentId,
        templateBlobId,
        requiredAttachmentId,
        requiredBlobId,
      }) => {
        const { ApiClient } = await import('/src/api/client.ts')
        const attachmentCrypto = await import('/src/attachments/crypto.ts')
        const api = new ApiClient()
        api.setSession(evidence.alice_session)
        const placeholder = new Blob([new Uint8Array([1, 2, 3, 4])])
        const digest =
          await attachmentCrypto.attachmentCiphertextSha256(placeholder)
        const blob = (blobId: string) => ({
          blob_id: blobId,
          resource_node_id: evidence.resource_id,
          ciphertext_size: placeholder.size,
          ciphertext_sha256: digest,
          key_epoch: evidence.key_epoch,
          encrypted_blob_metadata: evidence.encrypted_metadata,
          encrypted_attachment_metadata: evidence.encrypted_metadata,
        })
        await api.declarePretaskTemplateAttachment(
          evidence.project_id,
          evidence.preset_version_id,
          evidence.pretask_id,
          {
            id: templateAttachmentId,
            blob: blob(templateBlobId),
            idempotency_key: templateAttachmentId,
          },
        )
        await api.declareTaskRequiredAttachment(
          evidence.project_id,
          evidence.task_id,
          {
            id: requiredAttachmentId,
            source_template_attachment_id: templateAttachmentId,
            blob: blob(requiredBlobId),
            idempotency_key: requiredAttachmentId,
          },
        )
        const required = await api.listTaskRequiredAttachments(
          evidence.project_id,
          evidence.task_id,
        )
        return required.attachments.find(
          (attachment: { id: string }) =>
            attachment.id === requiredAttachmentId,
        )
      },
      {
        evidence,
        templateAttachmentId,
        templateBlobId,
        requiredAttachmentId,
        requiredBlobId,
      },
    )
    expect(provenance).toMatchObject({
      id: requiredAttachmentId,
      source_attachment_id: templateAttachmentId,
      attachment_kind: 'task_required',
    })

    await first.setOffline(true)
    const staged = await firstPage.evaluate(
      async ({
        evidence,
        requiredAttachmentId,
        completedAttachmentId,
        completedBlobId,
      }) => {
        const attachmentCrypto = await import('/src/attachments/crypto.ts')
        const opfs = await import('/src/storage/opfs.ts')
        const queue = await import('/src/attachments/offline-queue.ts')
        const key = Uint8Array.from(
          atob(evidence.resource_key_b64),
          (character) => character.charCodeAt(0),
        )
        const ciphertext = await attachmentCrypto.encryptAttachment(
          new Blob(['real-backend-offline-completion']),
          key,
          {
            projectId: evidence.project_id,
            resourceId: evidence.resource_id,
            blobId: completedBlobId,
            keyEpoch: evidence.key_epoch,
          },
        )
        await opfs.writeEncryptedAttachment(completedBlobId, ciphertext)
        await queue.enqueueCompletedAttachment({
          id: completedAttachmentId,
          identityId: evidence.alice_identity_id,
          projectId: evidence.project_id,
          taskId: evidence.task_id,
          blobId: completedBlobId,
          queuedAt: new Date().toISOString(),
          attempts: 0,
          request: {
            id: completedAttachmentId,
            assignment_id: evidence.assignment_id,
            required_attachment_id: requiredAttachmentId,
            blob: {
              blob_id: completedBlobId,
              resource_node_id: evidence.resource_id,
              ciphertext_size: ciphertext.size,
              ciphertext_sha256:
                await attachmentCrypto.attachmentCiphertextSha256(ciphertext),
              key_epoch: evidence.key_epoch,
              encrypted_blob_metadata: evidence.encrypted_metadata,
              encrypted_attachment_metadata: evidence.encrypted_metadata,
            },
            idempotency_key: completedAttachmentId,
          },
        })
        return (await queue.listQueuedCompletedAttachments()).length
      },
      {
        evidence,
        requiredAttachmentId,
        completedAttachmentId,
        completedBlobId,
      },
    )
    expect(staged).toBe(1)

    await first.setOffline(false)
    const synchronized = await firstPage.evaluate(async (evidence) => {
      const { ApiClient } = await import('/src/api/client.ts')
      const queue = await import('/src/attachments/offline-queue.ts')
      const api = new ApiClient()
      api.setSession(evidence.alice_session)
      const result = await queue.flushCompletedAttachmentQueue(
        api,
        evidence.alice_identity_id,
      )
      return {
        uploaded: result.uploaded.length,
        failed: result.failed.length,
        remaining: (await queue.listQueuedCompletedAttachments()).length,
      }
    }, evidence)
    expect(synchronized).toEqual({ uploaded: 1, failed: 0, remaining: 0 })

    const secondDeviceRead = await secondPage.evaluate(
      async ({ evidence, completedBlobId }) => {
        const { ApiClient } = await import('/src/api/client.ts')
        const attachmentCrypto = await import('/src/attachments/crypto.ts')
        const api = new ApiClient()
        api.setSession(evidence.second_device_session)
        const metadata = await api.getAttachment(
          evidence.project_id,
          completedBlobId,
        )
        const downloaded = await api.downloadCiphertext(
          `/v1/projects/${evidence.project_id}/files/${completedBlobId}/content`,
        )
        const key = Uint8Array.from(
          atob(evidence.resource_key_b64),
          (character) => character.charCodeAt(0),
        )
        const plaintext = await attachmentCrypto.decryptAttachment(
          downloaded,
          key,
          {
            projectId: evidence.project_id,
            resourceId: evidence.resource_id,
            blobId: completedBlobId,
            keyEpoch: evidence.key_epoch,
          },
        )
        return {
          deviceId: evidence.second_device_id,
          state: metadata.state.state,
          plaintext: new TextDecoder().decode(plaintext),
        }
      },
      { evidence, completedBlobId },
    )
    expect(secondDeviceRead).toEqual({
      deviceId: evidence.second_device_id,
      state: 'available',
      plaintext: 'real-backend-offline-completion',
    })

    const completed = await firstPage.evaluate(
      async ({ evidence, completedAttachmentId }) => {
        const { ApiClient } = await import('/src/api/client.ts')
        const api = new ApiClient()
        api.setSession(evidence.alice_session)
        const response = await api.listTaskCompletedAttachments(
          evidence.project_id,
          evidence.task_id,
        )
        return response.attachments.find(
          (attachment: { id: string }) =>
            attachment.id === completedAttachmentId,
        )
      },
      { evidence, completedAttachmentId },
    )
    expect(completed).toMatchObject({
      id: completedAttachmentId,
      source_attachment_id: requiredAttachmentId,
      attachment_kind: 'task_completed',
    })
  } finally {
    await first.close()
    await second.close()
  }
})
