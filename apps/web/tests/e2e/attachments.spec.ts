import { expect, test } from '@playwright/test'

test('T-LLR-05.5 keeps plaintext and local paths out of OPFS and transport', async ({
  browserName,
  page,
}) => {
  test.skip(browserName !== 'chromium', 'OPFS is exercised in Chromium')
  const requests: Array<{ url: string; body: Buffer | null }> = []
  await page.route('**/v1/projects/**/files/**/content', async (route) => {
    requests.push({
      url: route.request().url(),
      body: route.request().postDataBuffer(),
    })
    await route.fulfill({ status: 204 })
  })
  await page.goto('/')

  const result = await page.evaluate(async () => {
    const cryptoPath = '/src/attachments/crypto.ts'
    const opfsPath = '/src/storage/opfs.ts'
    const apiPath = '/src/api/client.ts'
    const attachmentCrypto = await import(/* @vite-ignore */ cryptoPath)
    const opfs = await import(/* @vite-ignore */ opfsPath)
    const { ApiClient } = await import(/* @vite-ignore */ apiPath)
    const canary = 'classified-browser-attachment-05'
    const localPath = '/Users/alice/Documents/classified-browser-attachment-05.txt'
    const context = {
      projectId: '11111111-1111-4111-8111-111111111111',
      resourceId: '22222222-2222-4222-8222-222222222222',
      blobId: '33333333-3333-4333-8333-333333333333',
      keyEpoch: 1,
    }
    const key = crypto.getRandomValues(new Uint8Array(32))
    const ciphertext = await attachmentCrypto.encryptAttachment(
      new Blob([canary], { type: 'text/plain' }),
      key,
      context,
    )
    await opfs.writeEncryptedAttachment(context.blobId, ciphertext)
    const stored = await opfs.readEncryptedAttachment(context.blobId)
    const storedText = new TextDecoder().decode(await stored.arrayBuffer())
    let localPathRejected = false
    try {
      await opfs.writeEncryptedAttachment(localPath, ciphertext)
    } catch {
      localPathRejected = true
    }
    const client = new ApiClient()
    client.setSession('opaque-session')
    await client.uploadAttachmentCiphertext(
      context.projectId,
      context.blobId,
      stored,
    )
    const plaintext = new TextDecoder().decode(
      await attachmentCrypto.decryptAttachment(stored, key, context),
    )
    await opfs.removeEncryptedAttachment(context.blobId)
    return {
      canary,
      localPath,
      localPathRejected,
      plaintext,
      storedText,
      ciphertextType: ciphertext.type,
    }
  })

  expect(result.localPathRejected).toBe(true)
  expect(result.plaintext).toBe(result.canary)
  expect(result.storedText).not.toContain(result.canary)
  expect(result.storedText).not.toContain(result.localPath)
  expect(result.ciphertextType).toBe('application/octet-stream')
  expect(requests).toHaveLength(1)
  expect(requests[0].url).not.toContain('file:')
  expect(requests[0].url).not.toContain(encodeURIComponent(result.localPath))
  expect(requests[0].body?.toString('utf8')).not.toContain(result.canary)
  expect(requests[0].body?.toString('utf8')).not.toContain(result.localPath)
})

test('T-LLR-05.6 forces hostile content through an opaque download', async ({
  page,
}) => {
  await page.goto('/')
  const result = await page.evaluate(async () => {
    const modulePath = '/src/downloads/download.ts'
    const downloads = await import(/* @vite-ignore */ modulePath)
    const hostile = new Blob(
      ['<svg onload="document.body.dataset.executed=1"></svg>'],
      { type: 'image/svg+xml' },
    )
    const safe = downloads.asSafeDownloadBlob(hostile)
    const created: string[] = []
    const originalCreate = URL.createObjectURL
    const originalClick = HTMLAnchorElement.prototype.click
    URL.createObjectURL = (blob: Blob) => {
      created.push(blob.type)
      return 'blob:https://sprout.test/opaque'
    }
    HTMLAnchorElement.prototype.click = function () {
      created.push(`${this.download}|${this.rel}`)
    }
    try {
      downloads.standardDownload(hostile, '../../hostile.svg')
    } finally {
      URL.createObjectURL = originalCreate
      HTMLAnchorElement.prototype.click = originalClick
    }
    return {
      bodyExecuted: document.body.dataset.executed ?? null,
      created,
      safeName: downloads.safeDownloadFileName('../../hostile.svg'),
      safeType: safe.type,
    }
  })

  expect(result.bodyExecuted).toBeNull()
  expect(result.safeType).toBe('application/octet-stream')
  expect(result.safeName).toBe('hostile.svg')
  expect(result.created).toEqual([
    'application/octet-stream',
    'hostile.svg|noopener',
  ])
})

test('HLT-05 stages provenance-bound completion offline and reads it on an authorized device', async ({
  browser,
  browserName,
}) => {
  test.skip(browserName !== 'chromium', 'OPFS offline staging is exercised in Chromium')
  const projectId = '05000000-0000-4000-8000-000000000001'
  const resourceId = '05000000-0000-4000-8000-000000000002'
  const taskId = '05000000-0000-4000-8000-000000000003'
  const templateId = '05000000-0000-4000-8000-000000000004'
  const requiredId = '05000000-0000-4000-8000-000000000005'
  const completedId = '05000000-0000-4000-8000-000000000006'
  const blobId = '05000000-0000-4000-8000-000000000007'
  const assignmentId = '05000000-0000-4000-8000-000000000008'
  const aliceIdentityId = '05000000-0000-4000-8000-000000000013'
  let templateDeclared = false
  let requiredProvenance = false
  let completedRequired = false
  let ciphertext: Buffer | null = null

  const alice = await browser.newContext()
  const bob = await browser.newContext()
  const alicePage = await alice.newPage()
  const bobPage = await bob.newPage()
  const installBackend = async (
    page: typeof alicePage,
    token: string,
  ) => {
    await page.route('**/v1/projects/**', async (route) => {
      const request = route.request()
      const path = new URL(request.url()).pathname
      if (request.headers().authorization !== `Bearer ${token}`) {
        await route.fulfill({ status: 401 })
        return
      }
      if (request.method() === 'POST' && path.includes('/pretasks/')) {
        const body = request.postDataJSON() as { id: string }
        templateDeclared = body.id === templateId
        await route.fulfill({
          contentType: 'application/json',
          body: JSON.stringify({
            attachment: { id: templateId },
            upload_url: `/v1/projects/${projectId}/files/template/content`,
          }),
        })
        return
      }
      if (request.method() === 'POST' && path.endsWith('/required-attachments')) {
        const body = request.postDataJSON() as {
          id: string
          source_template_attachment_id: string
        }
        requiredProvenance =
          templateDeclared &&
          body.id === requiredId &&
          body.source_template_attachment_id === templateId
        await route.fulfill({
          contentType: 'application/json',
          body: JSON.stringify({
            attachment: { id: requiredId },
            upload_url: `/v1/projects/${projectId}/files/required/content`,
          }),
        })
        return
      }
      if (request.method() === 'POST' && path.endsWith('/completed-attachments')) {
        const body = request.postDataJSON() as {
          id: string
          required_attachment_id: string
        }
        completedRequired =
          requiredProvenance &&
          body.id === completedId &&
          body.required_attachment_id === requiredId
        await route.fulfill({
          contentType: 'application/json',
          body: JSON.stringify({
            attachment: { id: completedId },
            upload_url: `/v1/projects/${projectId}/files/${blobId}/content`,
          }),
        })
        return
      }
      if (
        request.method() === 'PUT' &&
        path.endsWith(`/files/${blobId}/content`)
      ) {
        ciphertext = request.postDataBuffer()
        await route.fulfill({ status: 204 })
        return
      }
      if (request.method() === 'GET' && path.endsWith(`/files/${blobId}`)) {
        await route.fulfill({
          contentType: 'application/json',
          body: JSON.stringify({
            id: blobId,
            project_id: projectId,
            resource_node_id: resourceId,
            ciphertext_size: ciphertext?.byteLength ?? 0,
            ciphertext_sha256: 'unused-by-this-ceremony',
            key_epoch: 1,
            encrypted_metadata: {},
            state: { state: 'available', uploaded_at: new Date().toISOString() },
          }),
        })
        return
      }
      if (
        request.method() === 'GET' &&
        path.endsWith(`/files/${blobId}/content`) &&
        ciphertext
      ) {
        await route.fulfill({
          contentType: 'application/octet-stream',
          body: ciphertext,
        })
        return
      }
      await route.fulfill({ status: 404 })
    })
  }

  try {
    await installBackend(alicePage, 'alice-authenticated-device')
    await installBackend(bobPage, 'bob-authorized-device')
    await alicePage.goto('/')
    await bobPage.goto('/')

    const declared = await alicePage.evaluate(
      async ({ projectId, taskId, templateId, requiredId, resourceId }) => {
        const apiPath = '/src/api/client.ts'
        const { ApiClient } = await import(/* @vite-ignore */ apiPath)
        const api = new ApiClient()
        api.setSession('alice-authenticated-device')
        const encrypted = {
          version: 1,
          algorithm: 'xchacha20poly1305',
          key_id: 'resource:1',
          ciphertext_b64: 'Y2lwaGVydGV4dA==',
          nonce_b64: 'MDEyMzQ1Njc4OWFi',
        }
        await api.declarePretaskTemplateAttachment(
          projectId,
          '05000000-0000-4000-8000-000000000009',
          '05000000-0000-4000-8000-000000000010',
          {
            id: templateId,
            blob: {
              blob_id: '05000000-0000-4000-8000-000000000011',
              resource_node_id: resourceId,
              ciphertext_size: 32,
              ciphertext_sha256: 'dGVtcGxhdGU=',
              key_epoch: 1,
              encrypted_blob_metadata: encrypted,
              encrypted_attachment_metadata: encrypted,
            },
            idempotency_key: crypto.randomUUID(),
          },
        )
        await api.declareTaskRequiredAttachment(projectId, taskId, {
          id: requiredId,
          source_template_attachment_id: templateId,
          blob: {
            blob_id: '05000000-0000-4000-8000-000000000012',
            resource_node_id: resourceId,
            ciphertext_size: 32,
            ciphertext_sha256: 'cmVxdWlyZWQ=',
            key_epoch: 1,
            encrypted_blob_metadata: encrypted,
            encrypted_attachment_metadata: encrypted,
          },
          idempotency_key: crypto.randomUUID(),
        })
        return true
      },
      { projectId, taskId, templateId, requiredId, resourceId },
    )
    expect(declared).toBe(true)

    await alice.setOffline(true)
    const staged = await alicePage.evaluate(
      async ({
        projectId,
        resourceId,
        taskId,
        requiredId,
        completedId,
        blobId,
        assignmentId,
        aliceIdentityId,
      }) => {
        const cryptoPath = '/src/attachments/crypto.ts'
        const opfsPath = '/src/storage/opfs.ts'
        const queuePath = '/src/attachments/offline-queue.ts'
        const attachmentCrypto = await import(/* @vite-ignore */ cryptoPath)
        const opfs = await import(/* @vite-ignore */ opfsPath)
        const queue = await import(/* @vite-ignore */ queuePath)
        const key = new Uint8Array(32).fill(7)
        const encrypted = await attachmentCrypto.encryptAttachment(
          new Blob(['offline-completed-attachment']),
          key,
          { projectId, resourceId, blobId, keyEpoch: 1 },
        )
        await opfs.writeEncryptedAttachment(blobId, encrypted)
        const metadata = {
          version: 1,
          algorithm: 'xchacha20poly1305',
          key_id: 'resource:1',
          ciphertext_b64: 'bWV0YWRhdGE=',
          nonce_b64: 'MDEyMzQ1Njc4OWFi',
        }
        await queue.enqueueCompletedAttachment({
          id: completedId,
          identityId: aliceIdentityId,
          projectId,
          taskId,
          blobId,
          queuedAt: new Date().toISOString(),
          attempts: 0,
          request: {
            id: completedId,
            assignment_id: assignmentId,
            required_attachment_id: requiredId,
            blob: {
              blob_id: blobId,
              resource_node_id: resourceId,
              ciphertext_size: encrypted.size,
              ciphertext_sha256:
                await attachmentCrypto.attachmentCiphertextSha256(encrypted),
              key_epoch: 1,
              encrypted_blob_metadata: metadata,
              encrypted_attachment_metadata: metadata,
            },
            idempotency_key: completedId,
          },
        })
        return (await queue.listQueuedCompletedAttachments()).length
      },
      {
        projectId,
        resourceId,
        taskId,
        requiredId,
        completedId,
        blobId,
        assignmentId,
        aliceIdentityId,
      },
    )
    expect(staged).toBe(1)

    await alice.setOffline(false)
    const synchronized = await alicePage.evaluate(async (aliceIdentityId) => {
      const apiPath = '/src/api/client.ts'
      const queuePath = '/src/attachments/offline-queue.ts'
      const { ApiClient } = await import(/* @vite-ignore */ apiPath)
      const queue = await import(/* @vite-ignore */ queuePath)
      const api = new ApiClient()
      api.setSession('alice-authenticated-device')
      const result = await queue.flushCompletedAttachmentQueue(
        api,
        aliceIdentityId,
      )
      return {
        uploaded: result.uploaded.length,
        failed: result.failed.length,
        remaining: (await queue.listQueuedCompletedAttachments()).length,
      }
    }, aliceIdentityId)
    expect(synchronized).toEqual({ uploaded: 1, failed: 0, remaining: 0 })
    expect(completedRequired).toBe(true)

    const bobPlaintext = await bobPage.evaluate(
      async ({ projectId, resourceId, blobId }) => {
        const apiPath = '/src/api/client.ts'
        const cryptoPath = '/src/attachments/crypto.ts'
        const { ApiClient } = await import(/* @vite-ignore */ apiPath)
        const attachmentCrypto = await import(/* @vite-ignore */ cryptoPath)
        const api = new ApiClient()
        api.setSession('bob-authorized-device')
        const downloaded = await api.downloadCiphertext(
          `/v1/projects/${projectId}/files/${blobId}/content`,
        )
        const plaintext = await attachmentCrypto.decryptAttachment(
          downloaded,
          new Uint8Array(32).fill(7),
          { projectId, resourceId, blobId, keyEpoch: 1 },
        )
        return new TextDecoder().decode(plaintext)
      },
      { projectId, resourceId, blobId },
    )
    expect(bobPlaintext).toBe('offline-completed-attachment')
  } finally {
    await alice.close()
    await bob.close()
  }
})
