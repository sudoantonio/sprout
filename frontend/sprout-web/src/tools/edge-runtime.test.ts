import { describe, expect, it, vi } from 'vitest'
import { EDGE_TOOL_MANIFESTS, EdgeToolFailure, GovernedLocalEdgeToolRuntime, executeContractOnlyDocumentEdit, type NativeDocumentTransport, type NativePinnedHttpTransport, type PinnedHttpResponse } from './edge-runtime'

const encoder = new TextEncoder()
const context = (overrides = {}) => ({ authenticatedSessionId: 'session', expectedOrigin: 'https://sprout.test', actualOrigin: 'https://sprout.test', attempt: 1, signal: new AbortController().signal, ...overrides })
const documents: NativeDocumentTransport = { readCapability: vi.fn(async () => ({ bytes: encoder.encode('hello'), mimeType: 'text/markdown', versionHash: 'v1' })) }

function transport(address = '203.0.113.10', response = { status: 200, headers: { 'content-type': 'text/plain' }, body: encoder.encode('ok') }): NativePinnedHttpTransport {
  return { resolve: vi.fn(async () => [address]), executePinned: vi.fn(async () => response) }
}

describe('governed local edge tool runtime', () => {
  it('keeps native Sprout surfaces and external sends absent or fail closed', async () => {
    expect(EDGE_TOOL_MANIFESTS.some(({ id }) => /task|topic|info|comment/u.test(id))).toBe(false)
    const runtime = new GovernedLocalEdgeToolRuntime(transport(), documents)
    await expect(runtime.execute('mail.send', {}, context())).rejects.toMatchObject({ code: 'fail_closed' })
    await expect(runtime.execute('telegram.send', {}, context())).rejects.toMatchObject({ code: 'fail_closed' })
  })

  it('requires an authenticated origin-bound local bridge', async () => {
    const runtime = new GovernedLocalEdgeToolRuntime(transport(), documents)
    await expect(runtime.execute('web.read', { url: 'https://example.test' }, context({ authenticatedSessionId: '' }))).rejects.toEqual(new EdgeToolFailure('unauthenticated'))
    await expect(runtime.execute('web.read', { url: 'https://example.test' }, context({ actualOrigin: 'https://evil.test' }))).rejects.toEqual(new EdgeToolFailure('origin_mismatch'))
  })

  it('performs one pinned GET with no ambient cookie or authorization', async () => {
    const http = transport()
    const runtime = new GovernedLocalEdgeToolRuntime(http, documents)
    await expect(runtime.execute('web.read', { url: 'https://example.test/page' }, context())).resolves.toMatchObject({ text: 'ok' })
    expect(http.executePinned).toHaveBeenCalledTimes(1)
    expect(vi.mocked(http.executePinned).mock.calls[0][0]).toMatchObject({ method: 'GET', approvedAddresses: ['203.0.113.10'], headers: { accept: expect.any(String) } })
    expect(vi.mocked(http.executePinned).mock.calls[0][0].headers).not.toHaveProperty('cookie')
    expect(vi.mocked(http.executePinned).mock.calls[0][0].headers).not.toHaveProperty('authorization')

    const head = transport()
    await expect(new GovernedLocalEdgeToolRuntime(head, documents).execute('web.read', { url: 'https://example.test/page', method: 'HEAD' }, context())).resolves.toMatchObject({ text: 'ok' })
    expect(vi.mocked(head.executePinned).mock.calls[0][0].method).toBe('HEAD')
  })

  it('rejects POST, credentials, private targets, DNS rebinding and private redirects', async () => {
    const runtime = new GovernedLocalEdgeToolRuntime(transport(), documents)
    await expect(runtime.execute('web.read', { url: 'https://example.test', method: 'POST' }, context())).rejects.toMatchObject({ code: 'invalid_input' })
    await expect(runtime.execute('web.read', { url: 'https://user:secret@example.test' }, context())).rejects.toMatchObject({ code: 'invalid_input' })
    await expect(new GovernedLocalEdgeToolRuntime(transport('127.0.0.1'), documents).execute('web.read', { url: 'http://local.test' }, context())).rejects.toMatchObject({ code: 'ssrf_denied' })
    const redirect: NativePinnedHttpTransport = {
      resolve: vi.fn(async (host) => host === 'public.test' ? ['203.0.113.1'] : ['169.254.169.254']),
      executePinned: vi.fn(async () => ({ status: 302, headers: { location: 'http://metadata.test/latest', 'content-type': 'text/plain' }, body: new Uint8Array() })),
    }
    await expect(new GovernedLocalEdgeToolRuntime(redirect, documents).execute('web.read', { url: 'https://public.test' }, context())).rejects.toMatchObject({ code: 'ssrf_denied' })
  })

  it('rejects oversized and active content while extracting passive HTML', async () => {
    const oversized = transport('203.0.113.10', { status: 200, headers: { 'content-type': 'text/plain' }, body: new Uint8Array(1_048_577) })
    await expect(new GovernedLocalEdgeToolRuntime(oversized, documents).execute('web.read', { url: 'https://example.test' }, context())).rejects.toMatchObject({ code: 'oversized' })
    const active = transport('203.0.113.10', { status: 200, headers: { 'content-type': 'application/javascript' }, body: encoder.encode('alert(1)') })
    await expect(new GovernedLocalEdgeToolRuntime(active, documents).execute('web.read', { url: 'https://example.test' }, context())).rejects.toMatchObject({ code: 'unsupported_content' })
    const html = transport('203.0.113.10', { status: 200, headers: { 'content-type': 'text/html' }, body: encoder.encode('<title>T</title><script>secret()</script><p>Hello</p>') })
    await expect(new GovernedLocalEdgeToolRuntime(html, documents).execute('web.read', { url: 'https://example.test' }, context())).resolves.toMatchObject({ title: 'T', text: 'T Hello' })
  })

  it('reads only an opaque user-granted text/Markdown document capability', async () => {
    const runtime = new GovernedLocalEdgeToolRuntime(transport(), documents)
    await expect(runtime.execute('document.local.read', { document_capability_id: 'opaque-handle' }, context())).resolves.toEqual({ content: 'hello', version_hash: 'v1' })
    expect(documents.readCapability).toHaveBeenCalledWith('opaque-handle', expect.any(AbortSignal))
    await expect(runtime.execute('document.local.read', { document_capability_id: '../secret', path: '/private' }, context())).rejects.toMatchObject({ code: 'invalid_input' })

    const unsupported: NativeDocumentTransport = { readCapability: vi.fn(async () => ({ bytes: encoder.encode('%PDF'), mimeType: 'application/pdf', versionHash: 'v1' })) }
    await expect(new GovernedLocalEdgeToolRuntime(transport(), unsupported).execute('document.local.read', { document_capability_id: 'opaque' }, context())).rejects.toMatchObject({ code: 'unsupported_content' })
    const oversized: NativeDocumentTransport = { readCapability: vi.fn(async () => ({ bytes: new Uint8Array(1_048_577), mimeType: 'text/plain', versionHash: 'v1' })) }
    await expect(new GovernedLocalEdgeToolRuntime(transport(), oversized).execute('document.local.read', { document_capability_id: 'opaque' }, context())).rejects.toMatchObject({ code: 'oversized' })
  })

  it('maps caller cancellation without starting a hidden second execution', async () => {
    const controller = new AbortController()
    const http: NativePinnedHttpTransport = {
      resolve: vi.fn(async () => ['203.0.113.10']),
      executePinned: vi.fn(async ({ signal }) => await new Promise<PinnedHttpResponse>((_, reject) => {
        signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), { once: true })
      })),
    }
    const pending = new GovernedLocalEdgeToolRuntime(http, documents).execute('web.read', { url: 'https://example.test' }, context({ signal: controller.signal }))
    await vi.waitFor(() => expect(http.executePinned).toHaveBeenCalledTimes(1))
    controller.abort()
    await expect(pending).rejects.toMatchObject({ code: 'cancelled' })
    expect(http.executePinned).toHaveBeenCalledTimes(1)
  })

  it('keeps document edit contract-only with consent, optimistic version and idempotency', async () => {
    let version = 'v1'
    let content = 'old'
    const replay = new Map<string, { versionHash: string; replayed: boolean }>()
    const transport: NativeDocumentTransport = {
      ...documents,
      editCapability: vi.fn(async ({ expectedVersionHash, replacement, idempotencyKey }) => {
        const prior = replay.get(idempotencyKey)
        if (prior) return { ...prior, replayed: true }
        if (expectedVersionHash !== version) throw new Error('stale version')
        const candidate = new TextDecoder().decode(replacement)
        if (candidate === 'fail-before-atomic-rename') throw new Error('atomic rollback')
        content = candidate
        version = `v${Number(version.slice(1)) + 1}`
        const result = { versionHash: version, replayed: false }
        replay.set(idempotencyKey, result)
        return result
      }),
    }
    const signal = new AbortController().signal
    const input = { document_capability_id: 'opaque', expected_version_hash: 'v1', replacement: 'new', idempotency_key: 'once', one_shot_consent: true as const }
    await expect(executeContractOnlyDocumentEdit(transport, input, signal)).resolves.toEqual({ version_hash: 'v2', replayed: false })
    await expect(executeContractOnlyDocumentEdit(transport, input, signal)).resolves.toEqual({ version_hash: 'v2', replayed: true })
    await expect(executeContractOnlyDocumentEdit(transport, { ...input, idempotency_key: 'stale' }, signal)).rejects.toThrow('stale version')
    await expect(executeContractOnlyDocumentEdit(transport, { ...input, expected_version_hash: 'v2', replacement: 'fail-before-atomic-rename', idempotency_key: 'rollback' }, signal)).rejects.toThrow('atomic rollback')
    expect(content).toBe('new')
    expect(version).toBe('v2')
  })
})
