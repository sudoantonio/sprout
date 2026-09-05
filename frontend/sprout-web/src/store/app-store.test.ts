import { describe, expect, it } from 'vitest'
import type { ProjectView } from '../api/contracts'
import { appReducer, createInitialAppState, type ProjectItem } from './app-store'

const project = (id: string): ProjectItem => ({
  wire: {
    id,
    root_resource_id: `${id}-root`,
    owner_identity_id: 'owner',
    encrypted_metadata_b64: 'encrypted',
    key_epoch: 1,
    status: 'active',
    created_at: '2026-09-03T00:00:00.000Z',
    updated_at: '2026-09-03T00:00:00.000Z',
  } as ProjectView,
  deferred: true,
})

describe('project catalog selection', () => {
  it('selects the requested last project instead of the first catalog item', () => {
    const projects = [project('first'), project('last')]
    const state = appReducer(createInitialAppState(), {
      type: 'set-projects',
      projects,
      selectedProjectId: 'last',
    })

    expect(state.selectedProjectId).toBe('last')
  })

  it('falls back to the first available project when the saved one is gone', () => {
    const projects = [project('first'), project('second')]
    const state = appReducer(createInitialAppState(), {
      type: 'set-projects',
      projects,
      selectedProjectId: 'missing',
    })

    expect(state.selectedProjectId).toBe('first')
  })
})
