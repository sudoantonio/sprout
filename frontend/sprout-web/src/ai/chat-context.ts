import { createContext } from 'react'
import type { Uuid } from '../api/contracts'
import type { ChatService } from './chat'

export const AgentChatContext = createContext<{
  service?: ChatService
  identityId?: Uuid
  scope: string
}>({ scope: '' })
