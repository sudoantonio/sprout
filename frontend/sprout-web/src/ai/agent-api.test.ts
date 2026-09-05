import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiClient } from '../api/client'
import { AgentChatApi, toAgentPayload } from './agent-api'
import { ApiAgentLanguageTransport } from './edge-runtime'

const payload = { version: 1, algorithm: 'test', key_id: 'key', nonce_b64: 'AQI=', ciphertext_b64: 'AwQ=' }
afterEach(() => vi.unstubAllGlobals())

describe('agent HTTP wire contract', () => {
  it('uses authenticated scoped endpoints, encoded cursors and native payloads', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response('{"messages":[],"next_cursor":null}', { headers: { 'content-type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    const api = new ApiClient(); api.setSession('session')
    await new AgentChatApi(api).history('project', 'agent', 'cursor+value')
    expect(fetch.mock.calls[0][0]).toBe('/v1/projects/project/agents/agent/interrogations?before=cursor%2Bvalue')
    expect(fetch.mock.calls[0][1].headers.get('Authorization')).toBe('Bearer session')
  })

  it('converts domain ciphertext on a runner claim before local decryption', async () => {
    const request = vi.fn().mockResolvedValue({ id: 'invocation', encrypted_input: toAgentPayload(payload) })
    const transport = new ApiAgentLanguageTransport({ request } as unknown as ApiClient)
    const claim = await transport.claim('project', 'agent', '11'.repeat(32))
    expect(claim?.encrypted_input).toEqual(payload)
    expect(request).toHaveBeenCalledWith('/v1/projects/project/agents/agent/runner/client-provider/claim', {
      method: 'POST', body: { execution_profile_commitment_hex: '11'.repeat(32) }, signal: undefined,
    })
    request.mockResolvedValueOnce(null)
    await expect(transport.claim('project', 'agent', '11'.repeat(32))).resolves.toBeNull()
  })
})
