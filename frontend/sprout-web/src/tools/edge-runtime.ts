export const EXTERNAL_TOOL_IDS = [
  'web.read',
  'document.local.read',
  'document.local.edit',
  'mail.receive',
  'mail.send',
  'telegram.receive',
  'telegram.send',
] as const

export type ExternalToolId = (typeof EXTERNAL_TOOL_IDS)[number]

export type EdgeToolAvailability = 'executable' | 'contract_only' | 'fail_closed'

export interface EdgeToolManifest {
  id: ExternalToolId
  version: 1
  availability: EdgeToolAvailability
  maxInputBytes: number
  maxOutputBytes: number
  maxRedirects: number
  maxTimeoutMs: number
}

export const EDGE_TOOL_MANIFESTS: readonly EdgeToolManifest[] = [
  { id: 'web.read', version: 1, availability: 'executable', maxInputBytes: 16_384, maxOutputBytes: 1_048_576, maxRedirects: 4, maxTimeoutMs: 60_000 },
  { id: 'document.local.read', version: 1, availability: 'executable', maxInputBytes: 16_384, maxOutputBytes: 1_048_576, maxRedirects: 0, maxTimeoutMs: 60_000 },
  { id: 'document.local.edit', version: 1, availability: 'contract_only', maxInputBytes: 1_048_576, maxOutputBytes: 16_384, maxRedirects: 0, maxTimeoutMs: 60_000 },
  { id: 'mail.receive', version: 1, availability: 'contract_only', maxInputBytes: 16_384, maxOutputBytes: 1_048_576, maxRedirects: 0, maxTimeoutMs: 60_000 },
  { id: 'mail.send', version: 1, availability: 'fail_closed', maxInputBytes: 16_384, maxOutputBytes: 16_384, maxRedirects: 0, maxTimeoutMs: 60_000 },
  { id: 'telegram.receive', version: 1, availability: 'contract_only', maxInputBytes: 16_384, maxOutputBytes: 1_048_576, maxRedirects: 0, maxTimeoutMs: 60_000 },
  { id: 'telegram.send', version: 1, availability: 'fail_closed', maxInputBytes: 16_384, maxOutputBytes: 16_384, maxRedirects: 0, maxTimeoutMs: 60_000 },
] as const

export interface EdgeExecutionContext {
  authenticatedSessionId: string
  expectedOrigin: string
  actualOrigin: string
  attempt: number
  signal: AbortSignal
  developmentAllowLocalTargets?: boolean
}

export interface PinnedHttpRequest {
  url: URL
  method: 'GET' | 'HEAD'
  approvedAddresses: readonly string[]
  headers: Readonly<Record<string, string>>
  signal: AbortSignal
}

export interface PinnedHttpResponse {
  status: number
  headers: Readonly<Record<string, string>>
  body: Uint8Array
}

/** Implemented by the user-owned native edge. It must connect only to one of
 * `approvedAddresses` while preserving TLS hostname verification for `url`.
 */
export interface NativePinnedHttpTransport {
  resolve(hostname: string): Promise<readonly string[]>
  executePinned(request: PinnedHttpRequest): Promise<PinnedHttpResponse>
}

export interface NativeDocumentTransport {
  readCapability(capabilityId: string, signal: AbortSignal): Promise<{
    bytes: Uint8Array
    mimeType: string
    versionHash: string
  }>
  /** Optional native-companion contract. The companion owns path allowlists,
   * symlink/TOCTOU checks, temp-file fsync and atomic rename. */
  editCapability?(request: {
    capabilityId: string
    expectedVersionHash: string
    replacement: Uint8Array
    idempotencyKey: string
    signal: AbortSignal
  }): Promise<{ versionHash: string; replayed: boolean }>
}

export interface WebReadInput { url: string; method?: 'GET' | 'HEAD' }
export interface WebReadOutput { final_url: string; content_type: string; text: string; title: string | null }
export interface DocumentReadInput { document_capability_id: string }
export interface DocumentReadOutput { content: string; version_hash: string }
export interface DocumentEditInput { document_capability_id: string; expected_version_hash: string; replacement: string; idempotency_key: string; one_shot_consent: true }

export class EdgeToolFailure extends Error {
  constructor(readonly code: 'unauthenticated' | 'origin_mismatch' | 'invalid_input' | 'ssrf_denied' | 'timeout' | 'cancelled' | 'unsupported_content' | 'oversized' | 'fail_closed') {
    super(code)
  }
}

export class GovernedLocalEdgeToolRuntime {
  constructor(
    private readonly http: NativePinnedHttpTransport,
    private readonly documents: NativeDocumentTransport,
  ) {}

  async execute(tool: ExternalToolId, input: unknown, context: EdgeExecutionContext): Promise<unknown> {
    this.requireBoundSession(context)
    if (context.attempt < 1) throw new EdgeToolFailure('invalid_input')
    const manifest = EDGE_TOOL_MANIFESTS.find((candidate) => candidate.id === tool)
    if (!manifest || manifest.availability !== 'executable') throw new EdgeToolFailure('fail_closed')
    const timeout = AbortSignal.timeout(manifest.maxTimeoutMs)
    const signal = AbortSignal.any([context.signal, timeout])
    try {
      if (tool === 'web.read') return await this.webRead(input, context, signal, manifest)
      if (tool === 'document.local.read') return await this.documentRead(input, signal, manifest)
      throw new EdgeToolFailure('fail_closed')
    } catch (error) {
      if (error instanceof EdgeToolFailure) throw error
      if (context.signal.aborted) throw new EdgeToolFailure('cancelled')
      if (timeout.aborted) throw new EdgeToolFailure('timeout')
      throw error
    }
  }

  private requireBoundSession(context: EdgeExecutionContext): void {
    if (!context.authenticatedSessionId) throw new EdgeToolFailure('unauthenticated')
    if (context.actualOrigin !== context.expectedOrigin) throw new EdgeToolFailure('origin_mismatch')
  }

  private async webRead(input: unknown, context: EdgeExecutionContext, signal: AbortSignal, manifest: EdgeToolManifest): Promise<WebReadOutput> {
    if (!isRecord(input) || typeof input.url !== 'string' || Object.keys(input).some((key) => !['url', 'method'].includes(key))) {
      throw new EdgeToolFailure('invalid_input')
    }
    const method = input.method === undefined ? 'GET' : input.method
    if (method !== 'GET' && method !== 'HEAD') throw new EdgeToolFailure('invalid_input')
    let url = parseWebUrl(input.url)
    for (let redirect = 0; redirect <= manifest.maxRedirects; redirect += 1) {
      const addresses = await this.http.resolve(url.hostname)
      if (addresses.length === 0 || addresses.some((address) => isDeniedAddress(address)) && !context.developmentAllowLocalTargets) {
        throw new EdgeToolFailure('ssrf_denied')
      }
      const response = await this.http.executePinned({
        url,
        method,
        approvedAddresses: addresses,
        headers: { accept: 'text/plain, text/markdown, text/html;q=0.9' },
        signal,
      })
      const location = header(response.headers, 'location')
      if (response.status >= 300 && response.status < 400 && location) {
        if (redirect === manifest.maxRedirects) throw new EdgeToolFailure('invalid_input')
        url = parseWebUrl(new URL(location, url).toString())
        continue
      }
      if (response.body.byteLength > manifest.maxOutputBytes) throw new EdgeToolFailure('oversized')
      const contentType = (header(response.headers, 'content-type') ?? '').split(';', 1)[0].trim().toLowerCase()
      if (!['text/plain', 'text/markdown', 'text/html'].includes(contentType)) throw new EdgeToolFailure('unsupported_content')
      const raw = new TextDecoder('utf-8', { fatal: true }).decode(response.body)
      const title = contentType === 'text/html' ? extractTitle(raw) : null
      return { final_url: url.toString(), content_type: contentType, text: passiveText(raw, contentType), title }
    }
    throw new EdgeToolFailure('invalid_input')
  }

  private async documentRead(input: unknown, signal: AbortSignal, manifest: EdgeToolManifest): Promise<DocumentReadOutput> {
    if (!isRecord(input) || typeof input.document_capability_id !== 'string' || Object.keys(input).length !== 1 || input.document_capability_id.length === 0) {
      throw new EdgeToolFailure('invalid_input')
    }
    const result = await this.documents.readCapability(input.document_capability_id, signal)
    if (result.bytes.byteLength > manifest.maxOutputBytes) throw new EdgeToolFailure('oversized')
    if (!['text/plain', 'text/markdown'].includes(result.mimeType)) throw new EdgeToolFailure('unsupported_content')
    return { content: new TextDecoder('utf-8', { fatal: true }).decode(result.bytes), version_hash: result.versionHash }
  }
}

/** Development/contract-only adapter. It is intentionally not reachable from
 * `execute`, so 0033 cannot present an external filesystem mutation as closed
 * ToolSecuritySemantics. */
export async function executeContractOnlyDocumentEdit(
  transport: NativeDocumentTransport,
  input: DocumentEditInput,
  signal: AbortSignal,
): Promise<{ version_hash: string; replayed: boolean }> {
  if (input.one_shot_consent !== true || !input.document_capability_id || !input.expected_version_hash || !input.idempotency_key || !transport.editCapability) {
    throw new EdgeToolFailure('fail_closed')
  }
  const result = await transport.editCapability({
    capabilityId: input.document_capability_id,
    expectedVersionHash: input.expected_version_hash,
    replacement: new TextEncoder().encode(input.replacement),
    idempotencyKey: input.idempotency_key,
    signal,
  })
  return { version_hash: result.versionHash, replayed: result.replayed }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function parseWebUrl(value: string): URL {
  let url: URL
  try { url = new URL(value) } catch { throw new EdgeToolFailure('invalid_input') }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new EdgeToolFailure('invalid_input')
  return url
}

function header(headers: Readonly<Record<string, string>>, name: string): string | undefined {
  const found = Object.entries(headers).find(([key]) => key.toLowerCase() === name)
  return found?.[1]
}

function isDeniedAddress(address: string): boolean {
  const normalized = address.toLowerCase()
  if (normalized === '::1' || normalized === '::' || normalized.startsWith('fe80:') || normalized.startsWith('fc') || normalized.startsWith('fd')) return true
  const octets = normalized.split('.').map(Number)
  if (octets.length !== 4 || octets.some((part) => !Number.isInteger(part) || part < 0 || part > 255)) return true
  const [a, b] = octets
  return a === 0 || a === 10 || a === 127 || a >= 224 || (a === 169 && b === 254) || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 100 && b >= 64 && b <= 127)
}

function extractTitle(html: string): string | null {
  const match = /<title(?:\s[^>]*)?>([\s\S]*?)<\/title\s*>/iu.exec(html)
  return match ? passiveText(match[1], 'text/html').slice(0, 512) : null
}

function passiveText(value: string, contentType: string): string {
  if (contentType !== 'text/html') return value
  return value
    .replace(/<(script|style|noscript|iframe|object|embed)\b[^>]*>[\s\S]*?<\/\1\s*>/giu, ' ')
    .replace(/<[^>]+>/gu, ' ')
    .replace(/\s+/gu, ' ')
    .trim()
}
