import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiClient } from './client'

describe('attachment upload transport (T-LLR-05.5)', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('sends ciphertext only to the fixed same-origin route', async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(null, { status: 204 }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const client = new ApiClient('https://sprout.test')
    client.setSession('session-token')
    const projectId = '11111111-1111-4111-8111-111111111111'
    const blobId = '22222222-2222-4222-8222-222222222222'
    const ciphertext = new Blob(['opaque-ciphertext'], {
      type: 'application/octet-stream',
    })

    await client.uploadAttachmentCiphertext(projectId, blobId, ciphertext)

    const [url, request] = fetchMock.mock.calls[0]
    expect(request).toBeDefined()
    expect(url).toBe(
      `https://sprout.test/v1/projects/${projectId}/files/${blobId}/content`,
    )
    expect(request?.body).toBe(ciphertext)
    expect(JSON.stringify(request)).not.toContain('/Users/')
    expect(new Headers(request?.headers).get('Content-Type')).toBe(
      'application/octet-stream',
    )
  })

  it('rejects a caller-supplied local or cross-origin upload path', async () => {
    const client = new ApiClient()
    client.setSession('session-token')
    await expect(
      client.uploadAttachmentCiphertext(
        '11111111-1111-4111-8111-111111111111',
        '22222222-2222-4222-8222-222222222222',
        new Blob(['ciphertext']),
        'file:///Users/alice/private.txt',
      ),
    ).rejects.toThrow(/same-origin route/)
  })
})
