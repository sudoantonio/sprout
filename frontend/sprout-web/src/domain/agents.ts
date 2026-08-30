import type { AgentDirectoryItemDto } from '../api/contracts'

export type AgentActivity = 'working' | 'done' | 'rest'

export const activityForAgent = (
  agent: AgentDirectoryItemDto,
): AgentActivity => {
  if (
    agent.state === 'retired' ||
    agent.local_goal_state === 'completed' ||
    agent.local_goal_state === 'failed'
  ) {
    return 'done'
  }
  if (
    agent.state === 'active' &&
    agent.runner_state === 'active' &&
    agent.local_goal_state === 'active'
  ) {
    return 'working'
  }
  return 'rest'
}
