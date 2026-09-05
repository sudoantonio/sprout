import type { AgentDirectoryItemDto, Uuid } from '../api/contracts'
import type { KeyVault } from '../security/key-vault'
import { fromAgentPayload, toAgentPayload } from './agent-api'
import { chatCrypto, type ChatDocument } from './chat'
import type { EdgeLanguageCrypto } from './edge-runtime'

/** One crypto boundary per runOneClientOwnedInvocation call, in the native runner.
 * The host resolves current authorized sources and provides its own agent session,
 * device signatures and provider. The browser never impersonates this runner.
 */
export const createAgentChatCrypto = (
  vault: Pick<KeyVault, 'getResourceKey'>,
  projectId: Uuid,
  agent: Pick<AgentDirectoryItemDto, 'profile_resource_node_id' | 'key_epoch'>,
  resolveAuthorizedSources: EdgeLanguageCrypto['resolveAuthorizedSources'],
): EdgeLanguageCrypto => {
  let active: ReturnType<typeof chatCrypto> | undefined
  return {
    async decryptInvocationInput(payload) {
      active = undefined
      const codec = chatCrypto(vault, projectId, agent.profile_resource_node_id, agent.key_epoch, payload.key_id)
      const input = await codec.decrypt<ChatDocument>(toAgentPayload(payload))
      if (input.kind !== 'answer_from_authorized_context' || input.session_id !== payload.key_id ||
          typeof input.question !== 'string' || typeof input.instructions !== 'string') {
        throw new Error('Invalid encrypted chat input')
      }
      active = codec
      return input
    },
    resolveAuthorizedSources,
    async encryptOutput(plaintext) {
      if (!active) throw new Error('No authenticated chat input')
      return fromAgentPayload(await active.encrypt(plaintext))
    },
  }
}
