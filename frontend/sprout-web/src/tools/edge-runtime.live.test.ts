// @vitest-environment node
import { afterEach, describe, expect, it } from 'vitest'
import { createServer, type Server } from 'node:http'
import { mkdtemp, readFile, realpath, rm, symlink, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { GovernedLocalEdgeToolRuntime, type NativeDocumentTransport, type NativePinnedHttpTransport, type PinnedHttpRequest } from './edge-runtime'

const session = () => ({
  authenticatedSessionId: 'development-edge-session',
  expectedOrigin: 'https://sprout.test',
  actualOrigin: 'https://sprout.test',
  attempt: 1,
  signal: new AbortController().signal,
  developmentAllowLocalTargets: true,
})

class DevelopmentLoopbackTransport implements NativePinnedHttpTransport {
  async resolve(): Promise<readonly string[]> { return ['127.0.0.1'] }
  async executePinned(request: PinnedHttpRequest) {
    expect(request.approvedAddresses).toEqual(['127.0.0.1'])
    const response = await fetch(request.url, {
      method: request.method,
      headers: request.headers,
      redirect: 'manual',
      signal: request.signal,
    })
    return {
      status: response.status,
      headers: Object.fromEntries(response.headers.entries()),
      body: new Uint8Array(await response.arrayBuffer()),
    }
  }
}

describe('controlled development local-edge feature tests', () => {
  let server: Server | undefined
  let directory: string | undefined
  afterEach(async () => {
    if (server?.listening) await new Promise<void>((resolve, reject) => server?.close((error) => error ? reject(error) : resolve()))
    if (directory) await rm(directory, { recursive: true, force: true })
    server = undefined
    directory = undefined
  })

  it('performs a real bounded loopback web.read without ambient credentials', async () => {
    let receivedCookie: string | undefined
    let receivedAuthorization: string | undefined
    server = createServer((request, response) => {
      receivedCookie = request.headers.cookie
      receivedAuthorization = request.headers.authorization
      response.writeHead(200, { 'content-type': 'text/html' })
      response.end('<title>Controlled</title><script>never()</script><p>Passive body</p>')
    })
    await new Promise<void>((resolve) => server?.listen(0, '127.0.0.1', resolve))
    const address = server.address()
    if (!address || typeof address === 'string') throw new Error('missing controlled listener')
    const documents: NativeDocumentTransport = { readCapability: async () => ({ bytes: new Uint8Array(), mimeType: 'text/plain', versionHash: 'unused' }) }
    const runtime = new GovernedLocalEdgeToolRuntime(new DevelopmentLoopbackTransport(), documents)
    await expect(runtime.execute('web.read', { url: `http://127.0.0.1:${address.port}/page` }, session())).resolves.toEqual({
      final_url: `http://127.0.0.1:${address.port}/page`,
      content_type: 'text/html',
      text: 'Controlled Passive body',
      title: 'Controlled',
    })
    expect(receivedCookie).toBeUndefined()
    expect(receivedAuthorization).toBeUndefined()
  })

  it('reads a real text file only through an opaque capability and rejects a symlink escape', async () => {
    directory = await mkdtemp(join(tmpdir(), 'sprout-tool-edge-'))
    const allowed = join(directory, 'allowed.md')
    const outside = join(tmpdir(), `sprout-tool-outside-${process.pid}.txt`)
    const escaped = join(directory, 'escaped.md')
    await writeFile(allowed, '# bounded markdown')
    await writeFile(outside, 'outside')
    await symlink(outside, escaped)
    const capabilities = new Map([['allowed-capability', allowed], ['escaped-capability', escaped]])
    const documents: NativeDocumentTransport = {
      readCapability: async (capabilityId) => {
        const candidate = capabilities.get(capabilityId)
        if (!candidate) throw new Error('unknown capability')
        const root = `${await realpath(directory as string)}/`
        const exact = await realpath(candidate)
        if (!exact.startsWith(root)) throw new Error('symlink escape')
        return { bytes: new Uint8Array(await readFile(exact)), mimeType: 'text/markdown', versionHash: 'exact-v1' }
      },
    }
    const runtime = new GovernedLocalEdgeToolRuntime(new DevelopmentLoopbackTransport(), documents)
    await expect(runtime.execute('document.local.read', { document_capability_id: 'allowed-capability' }, session())).resolves.toEqual({ content: '# bounded markdown', version_hash: 'exact-v1' })
    await expect(runtime.execute('document.local.read', { document_capability_id: 'escaped-capability' }, session())).rejects.toThrow('symlink escape')
    await rm(outside, { force: true })
  })
})
