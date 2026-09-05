import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '../api/client'
import type { AgentDirectoryItemDto, EncryptedPayloadDto } from '../api/contracts'
import type { KeyVault } from '../security/key-vault'
import { encryptDocument } from '../security/wasm'
import { AgentChatApi, fromAgentPayload, toAgentPayload, type ChatMessageWire } from './agent-api'
import { chatCrypto, createChatService } from './chat'

const payload: EncryptedPayloadDto = { version: 1, algorithm: 'test', key_id: 'key', nonce_b64: 'AQI=', ciphertext_b64: 'AwQ=' }
vi.mock('../security/wasm', async (original) => ({
  ...await original<typeof import('../security/wasm')>(),
  encryptDocument: vi.fn(async () => payload),
  decryptDocument: vi.fn(async () => ({ kind: 'answer_from_authorized_context', session_id: 'turn', question: 'Restored question' })),
}))
const agent = {
  id: 'agent', controller_identity_id: 'owner', state: 'active', runner_state: 'active',
  profile_resource_node_id: 'profile', key_epoch: 3,
} as AgentDirectoryItemDto
const notFound = () => new ApiError(404, 'Not found')
const fixture = () => {
  const settings = new Map<string, string>()
  const vault = {
    getResourceKey: vi.fn(() => new Uint8Array(32).fill(7)),
    getLocalSetting: (key: string) => settings.get(key),
    putLocalSetting: vi.fn(async (key: string, value: string) => { settings.set(key, value); return true }),
    deleteLocalSetting: vi.fn(async (key: string) => settings.delete(key)),
  } as unknown as KeyVault
  const api = {
    get: vi.fn().mockRejectedValue(notFound()), record: vi.fn().mockResolvedValue({}),
    invocation: vi.fn().mockRejectedValue(notFound()), queue: vi.fn().mockResolvedValue({}),
    history: vi.fn().mockResolvedValue({ messages: [], next_cursor: null }),
  }
  return { api, vault, settings, service: createChatService(api as unknown as AgentChatApi, vault, 'project', 'owner') }
}

beforeEach(() => vi.clearAllMocks())
describe('governed browser chat', () => {
  it('uses native ciphertext bytes, empty authority and an exact profile source', async () => {
    const { api, service, settings } = fixture()
    await service.send(agent, 'PRIVATE CANARY')
    expect(api.record).toHaveBeenCalledOnce()
    expect(api.queue).toHaveBeenCalledOnce()
    const record = api.record.mock.calls[0][2]
    const queue = api.queue.mock.calls[0][2]
    expect(record.encrypted_transcript).toEqual(toAgentPayload(payload))
    expect(queue.encrypted_input).toEqual(record.encrypted_transcript)
    expect(queue.interrogation_id).toBe(record.id)
    expect(queue.sources).toEqual([{ kind: 'resource_body', resource_id: 'profile' }])
    expect(queue.authority_envelope).toEqual({ resource_authority: [], tool_authority: [] })
    expect(Object.values(record.causal_delta).every((value) => Array.isArray(value) && value.length === 0)).toBe(true)
    expect(JSON.stringify([record, queue])).not.toContain('PRIVATE CANARY')
    expect(settings.size).toBe(0)
  })

  it('keeps the same ciphertext and IDs after queue failure and service recreation', async () => {
    const { api, service, vault } = fixture()
    api.queue.mockRejectedValueOnce(new ApiError(0, 'Disconnected'))
    await expect(service.send(agent, 'Question')).rejects.toThrow('Disconnected')
    const original = api.queue.mock.calls[0][2]
    expect(service.hasPending(agent)).toBe(true)
    api.get.mockResolvedValue({})
    const reloaded = createChatService(api as unknown as AgentChatApi, vault, 'project', 'owner')
    await reloaded.retry(agent)
    expect(api.record).toHaveBeenCalledOnce()
    expect(api.queue.mock.calls[1][2]).toEqual(original)
    expect(encryptDocument).toHaveBeenCalledOnce()
    expect(reloaded.hasPending(agent)).toBe(false)
  })

  it('recovers a lost successful response without duplicating the invocation', async () => {
    const { api, service } = fixture()
    api.queue.mockRejectedValueOnce(new ApiError(0, 'Lost response'))
    api.invocation.mockRejectedValueOnce(notFound()).mockResolvedValue({ id: 'existing', status: 'pending' })
    await service.send(agent, 'Question')
    expect(api.queue).toHaveBeenCalledOnce()
    expect(service.hasPending(agent)).toBe(false)
  })

  it('does not interpret forbidden or offline reads as a missing record', async () => {
    const { api, service } = fixture()
    api.get.mockRejectedValue(new ApiError(403, 'Forbidden'))
    await expect(service.send(agent, 'Question')).rejects.toThrow('Forbidden')
    expect(api.record).not.toHaveBeenCalled()
    expect(api.queue).not.toHaveBeenCalled()
  })

  it('rejects other controllers and inactive runners before encryption or network calls', async () => {
    const { api, service } = fixture()
    await expect(service.send({ ...agent, controller_identity_id: 'someone' }, 'Q')).rejects.toThrow('controller')
    await expect(service.send({ ...agent, runner_state: 'pending_key' }, 'Q')).rejects.toThrow('runner')
    expect(encryptDocument).not.toHaveBeenCalled()
    expect(api.record).not.toHaveBeenCalled()
  })

  it('isolates pending envelopes by account and project', async () => {
    const { api, service, vault } = fixture()
    api.queue.mockRejectedValue(new ApiError(0, 'Disconnected'))
    await expect(service.send(agent, 'Question')).rejects.toThrow()
    expect(createChatService(api as unknown as AgentChatApi, vault, 'other', 'owner').hasPending(agent)).toBe(false)
    expect(createChatService(api as unknown as AgentChatApi, vault, 'project', 'other').hasPending(agent)).toBe(false)
  })

  it('binds encryption to the turn and exact epoch and wipes the temporary key', async () => {
    const { vault } = fixture()
    await chatCrypto(vault, 'project', 'profile', 3, 'turn').encrypt({ question: 'Q' })
    expect(vault.getResourceKey).toHaveBeenCalledWith('profile', 3)
    const options = vi.mocked(encryptDocument).mock.calls[0][1]
    expect(options).toMatchObject({ resourceId: 'turn', keyId: 'turn', kind: 'agent-chat', keyEpoch: 3 })
    expect([...options.resourceKey].every((byte) => byte === 0)).toBe(true)
    vi.mocked(vault.getResourceKey).mockReturnValue(undefined)
    await expect(chatCrypto(vault, 'project', 'profile', 4, 'turn').encrypt('Q')).rejects.toThrow('chiave')
  })

  it('loads encrypted history and marks legacy or mismatched sessions as locked', async () => {
    const { api, service } = fixture()
    const wire: ChatMessageWire = { id: 'turn', transcript_resource_node_id: 'profile', key_epoch: 3, encrypted_transcript: toAgentPayload(payload), encrypted_answer: null, created_at: '2026-09-01', answered_at: null, invocation: null }
    api.history.mockResolvedValue({ messages: [wire, { ...wire, id: 'wrong' }], next_cursor: 'older' })
    const result = await service.history(agent)
    expect(result.messages[0].question).toBe('Restored question')
    expect(result.messages[1].locked).toBe(true)
    expect(result.next_cursor).toBe('older')
    expect(fromAgentPayload(toAgentPayload(payload))).toEqual(payload)
  })
})
