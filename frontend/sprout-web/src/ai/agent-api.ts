import { ApiClient } from '../api/client'
import type { EncryptedPayloadDto, Uuid } from '../api/contracts'
import { base64ToBytes, bytesToBase64 } from '../security/wasm'
import type { InformationSource, StructuredLanguageTaskEnvelopeDto } from './contracts'

/** Governed APIs serialize the domain payload as byte arrays, unlike product DTOs. */
export interface AgentEncryptedPayload {
  version: number
  algorithm: string
  key_id: string
  nonce: number[]
  ciphertext: number[]
}

export const toAgentPayload = (payload: EncryptedPayloadDto): AgentEncryptedPayload => ({
  version: payload.version, algorithm: payload.algorithm, key_id: payload.key_id,
  nonce: Array.from(base64ToBytes(payload.nonce_b64)),
  ciphertext: Array.from(base64ToBytes(payload.ciphertext_b64)),
})

export const fromAgentPayload = (payload: AgentEncryptedPayload): EncryptedPayloadDto => ({
  version: payload.version, algorithm: payload.algorithm, key_id: payload.key_id,
  nonce_b64: bytesToBase64(new Uint8Array(payload.nonce)),
  ciphertext_b64: bytesToBase64(new Uint8Array(payload.ciphertext)),
})

export interface InvocationStatus {
  id: Uuid
  status: 'pending' | 'leased' | 'succeeded' | 'failed' | 'cancelled'
  attempt: number
  max_attempts: number
}
export interface ChatMessageWire {
  id: Uuid
  transcript_resource_node_id: Uuid
  key_epoch: number
  encrypted_transcript: AgentEncryptedPayload
  encrypted_answer: AgentEncryptedPayload | null
  created_at: string
  answered_at: string | null
  invocation: InvocationStatus | null
}
export interface ChatHistoryPage {
  messages: ChatMessageWire[]
  next_cursor: Uuid | null
}
export interface RecordChatRequest {
  id: Uuid
  transcript_resource_node_id: Uuid
  key_epoch: number
  encrypted_transcript: AgentEncryptedPayload
  causal_delta: {
    resource_effects: never[]; tool_invocations: never[]; prompt_revisions: never[]
    local_goal_revisions: never[]; created_work: never[]; activated_obligations: never[]
    assigned_tasks: never[]
  }
}
export interface QueueChatRequest {
  id: Uuid
  language_task: StructuredLanguageTaskEnvelopeDto
  authority_envelope: { resource_authority: never[]; tool_authority: never[] }
  sources: InformationSource[]
  encrypted_input: AgentEncryptedPayload
  surface: 'interrogation'
  interrogation_id: Uuid
}

export class AgentChatApi {
  constructor(private readonly api: ApiClient) {}
  private path(projectId: Uuid, agentId: Uuid) {
    return `/v1/projects/${encodeURIComponent(projectId)}/agents/${encodeURIComponent(agentId)}`
  }
  history(projectId: Uuid, agentId: Uuid, before?: Uuid, signal?: AbortSignal): Promise<ChatHistoryPage> {
    return this.api.request(`${this.path(projectId, agentId)}/interrogations${before ? `?before=${encodeURIComponent(before)}` : ''}`, { signal })
  }
  get(projectId: Uuid, agentId: Uuid, id: Uuid): Promise<Omit<ChatMessageWire, 'invocation'>> {
    return this.api.request(`${this.path(projectId, agentId)}/interrogations/${encodeURIComponent(id)}`)
  }
  record(projectId: Uuid, agentId: Uuid, body: RecordChatRequest): Promise<unknown> {
    return this.api.request(`${this.path(projectId, agentId)}/interrogations`, { method: 'POST', body })
  }
  invocation(projectId: Uuid, agentId: Uuid, id: Uuid): Promise<InvocationStatus> {
    return this.api.request(`${this.path(projectId, agentId)}/invocations/${encodeURIComponent(id)}`)
  }
  queue(projectId: Uuid, agentId: Uuid, body: QueueChatRequest): Promise<unknown> {
    return this.api.request(`${this.path(projectId, agentId)}/invocations/client-provider`, { method: 'POST', body })
  }
}
