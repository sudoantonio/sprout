import { afterEach, describe, expect, it, vi } from 'vitest'
import type { LocalEdgeInferenceBridge } from './execution-boundary'
import { LocalAiProfileStore } from './profile'
import {
  buildWorkspaceSources,
  WorkspaceChatService,
  type WorkspaceSnapshot,
} from './workspace-chat'

class MemoryVault {
  readonly values = new Map<string, string>()
  getLocalSetting(key: string) { return this.values.get(key) }
  async putLocalSetting(key: string, value: string) { this.values.set(key, value); return true }
  async deleteLocalSetting(key: string) { this.values.delete(key); return true }
}

const projectId = crypto.randomUUID()
const rootId = crypto.randomUUID()
const topicId = crypto.randomUUID()
const topicResourceId = crypto.randomUUID()
const listId = crypto.randomUUID()
const listResourceId = crypto.randomUUID()
const taskResourceId = crypto.randomUUID()

const snapshot = (): WorkspaceSnapshot => ({
  project: {
    wire: {
      id: projectId,
      root_resource_id: rootId,
      owner_identity_id: crypto.randomUUID(),
      encrypted_metadata_b64: 'project-secret-ciphertext',
      key_epoch: 1,
      status: 'active',
      created_at: '2026-09-05T09:00:00Z',
      updated_at: '2026-09-05T09:00:00Z',
    },
    document: { schema: 1, name: 'Lancio autunnale' },
  },
  topics: [
    {
      wire: {
        id: topicId,
        project_id: projectId,
        resource_node_id: topicResourceId,
        payload: null,
        key_epoch: 1,
        payload_version: 1,
        created_at: '2026-09-05T09:00:00Z',
        deleted_at: null,
      },
      document: { schema: 1, name: 'Marketing' },
    },
    {
      wire: {
        id: crypto.randomUUID(),
        project_id: projectId,
        resource_node_id: crypto.randomUUID(),
        payload: null,
        key_epoch: 1,
        payload_version: 1,
        created_at: '2026-09-05T09:00:00Z',
        deleted_at: null,
      },
      lockedReason: 'missing key',
    },
  ],
  taskLists: [{
    wire: {
      id: listId,
      project_id: projectId,
      topic_id: topicId,
      resource_node_id: listResourceId,
      payload: null,
      payload_version: 1,
      key_epoch: 1,
      created_at: '2026-09-05T09:00:00Z',
      archived_at: null,
    },
    document: { schema: 1, name: 'Campagna email' },
  }],
  tasks: [{
    wire: {
      id: crypto.randomUUID(),
      project_id: projectId,
      list_id: listId,
      resource_node_id: taskResourceId,
      task_kind: 'deadline',
      payload: null,
      selected_value_snapshot: null,
      key_epoch: 1,
      state: { state: 'open' },
      source_pretask_id: null,
      preset_assignment_id: null,
      copied_from_task_id: null,
      questionnaire_version_id: null,
      recurrence_series_id: null,
      occurrence_number: null,
      active_assignment_id: null,
      active_assignee_identity_id: null,
      created_at: '2026-09-05T09:00:00Z',
      payload_version: 1,
    },
    document: {
      schema: 1,
      title: 'Preparare newsletter',
      notes: 'Usare il nuovo catalogo',
      due_at: '2026-09-12T10:00:00Z',
    },
  }],
})

const bridge = () => ({
  protocolVersion: 'sprout-client-inference-edge-v1' as const,
  discoverModels: vi.fn().mockResolvedValue([]),
  generateStructured: vi.fn().mockResolvedValue({
    value: {
      answer: 'La newsletter scade il 12 settembre.',
      action_type: 'none',
      target_id: '',
      title: '',
      notes: '',
      priority: '',
      assignee_identity_id: '',
      name: '',
      email: '',
      role: '',
    },
    attemptCount: 1,
    sanitizedStatus: 'succeeded',
    wireWitness: {
      protocol: 'openai_responses_v1' as const,
      method: 'POST' as const,
      path: '/v1/responses',
      selectedModel: 'external-model',
      body: '{}',
    },
    actualRequestCommitmentHex: 'request',
    actualOutputCommitmentHex: 'output',
  }),
  detectOllama: vi.fn().mockResolvedValue({ installed: false, models: [] }),
  installOfficialOllama: vi.fn().mockResolvedValue({ installed: true as const, version: '1' }),
  pullOllamaModel: vi.fn().mockResolvedValue(undefined),
}) satisfies LocalEdgeInferenceBridge

describe('workspace AI chat', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('projects only decrypted resources from the selected workspace', () => {
    const sources = buildWorkspaceSources(snapshot())
    const plaintext = sources.map((source) => source.plaintext).join('\n')

    expect(sources.map((source) => source.descriptor)).toEqual([
      { kind: 'resource_body', resource_id: rootId },
      { kind: 'resource_body', resource_id: topicResourceId },
      { kind: 'resource_body', resource_id: listResourceId },
      { kind: 'resource_body', resource_id: taskResourceId },
    ])
    expect(plaintext).toContain('Lancio autunnale')
    expect(plaintext).toContain('Preparare newsletter')
    expect(plaintext).not.toContain('project-secret-ciphertext')
    expect(plaintext).not.toContain('missing key')
  })

  it('uses the external runtime without an agent and isolates history by project', async () => {
    const vault = new MemoryVault()
    await new LocalAiProfileStore(vault).save({
      mode: 'commercial_api',
      provider: 'openai',
      credential: 'device-only-key',
      model: 'external-model',
      preferences: { timeoutMs: 30_000, maxOutputTokens: 512, maxAttempts: 2 },
    })
    const runtime = bridge()
    const service = new WorkspaceChatService(vault, () => runtime)

    const turns = await service.ask(snapshot(), 'Quando scade la newsletter?')

    expect(turns.map(({ role, content }) => ({ role, content }))).toEqual([
      { role: 'user', content: 'Quando scade la newsletter?' },
      { role: 'assistant', content: 'La newsletter scade il 12 settembre.' },
    ])
    expect(runtime.generateStructured).toHaveBeenCalledOnce()
    const [profile, request] = runtime.generateStructured.mock.calls[0]
    expect(profile).toMatchObject({ model: 'external-model' })
    expect(request.input).toMatchObject({ project_id: projectId, question: 'Quando scade la newsletter?' })
    expect(request.sources).toHaveLength(4)
    expect(JSON.stringify(request)).not.toContain('agent_id')
    expect(service.history(crypto.randomUUID())).toEqual([])
  })

  it('turns an imperative request into one grounded action plan', async () => {
    const vault = new MemoryVault()
    await new LocalAiProfileStore(vault).save({
      mode: 'commercial_api',
      provider: 'openai',
      credential: 'device-only-key',
      model: 'external-model',
      preferences: { timeoutMs: 30_000, maxOutputTokens: 512, maxAttempts: 2 },
    })
    const runtime = bridge()
    runtime.generateStructured.mockResolvedValueOnce({
      value: {
        answer: 'Ho preparato la creazione del task.',
        action_type: 'create_task',
        target_id: listId,
        title: 'Analizzare i certificati API',
        notes: '',
        priority: 'normal',
        assignee_identity_id: '',
        name: '',
        email: '',
        role: '',
      },
      attemptCount: 1,
      sanitizedStatus: 'succeeded',
      wireWitness: {
        protocol: 'openai_responses_v1',
        method: 'POST',
        path: '/v1/responses',
        selectedModel: 'external-model',
        body: '{}',
      },
      actualRequestCommitmentHex: 'request',
      actualOutputCommitmentHex: 'output',
    })
    const service = new WorkspaceChatService(vault, () => runtime)

    const turns = await service.ask(
      snapshot(),
      'Crea task dentro Campagna email: Analizzare i certificati API',
    )

    expect(turns.at(-1)?.proposal).toMatchObject({
      kind: 'create_task',
      targetId: listId,
      title: 'Analizzare i certificati API',
      status: 'pending',
    })
    const request = runtime.generateStructured.mock.calls[0][1]
    expect(request.task).toBe('interpret_proxy_request')
    expect(request.input).toMatchObject({
      max_plan_steps: 1,
      candidate_task_list_ids: [listId],
    })
  })

  it('rejects a model-selected target outside the workspace envelope', async () => {
    const vault = new MemoryVault()
    await new LocalAiProfileStore(vault).save({
      mode: 'commercial_api',
      provider: 'openai',
      credential: 'device-only-key',
      model: 'external-model',
      preferences: { timeoutMs: 30_000, maxOutputTokens: 512, maxAttempts: 2 },
    })
    const runtime = bridge()
    runtime.generateStructured.mockResolvedValueOnce({
      value: {
        answer: 'Creo il task.',
        action_type: 'create_task',
        target_id: crypto.randomUUID(),
        title: 'Task fuori contesto',
        notes: '',
        priority: 'normal',
        assignee_identity_id: '',
        name: '',
        email: '',
        role: '',
      },
      attemptCount: 1,
      sanitizedStatus: 'succeeded',
      wireWitness: {
        protocol: 'openai_responses_v1',
        method: 'POST',
        path: '/v1/responses',
        selectedModel: 'external-model',
        body: '{}',
      },
      actualRequestCommitmentHex: 'request',
      actualOutputCommitmentHex: 'output',
    })
    const service = new WorkspaceChatService(vault, () => runtime)

    await expect(service.ask(snapshot(), 'Crea un task fuori contesto')).rejects.toThrow(
      'non appartiene al progetto aperto',
    )
  })

  it('answers as the personal agent from only the observed agent work', async () => {
    const vault = new MemoryVault()
    await new LocalAiProfileStore(vault).save({
      mode: 'commercial_api',
      provider: 'openai',
      credential: 'device-only-key',
      model: 'external-model',
      preferences: { timeoutMs: 30_000, maxOutputTokens: 512, maxAttempts: 2 },
    })
    const runtime = bridge()
    const service = new WorkspaceChatService(vault, () => runtime)
    const agentPrincipal = crypto.randomUUID()
    const agentSnapshot = snapshot()
    agentSnapshot.tasks[0].wire.active_assignee_identity_id = agentPrincipal

    await service.askAboutAgent(agentSnapshot, {
      agentId: crypto.randomUUID(),
      principalIdentityId: agentPrincipal,
      identityHandle: 'minerva-agent',
    }, 'Cosa sta facendo?')

    const request = runtime.generateStructured.mock.calls[0][1]
    expect(request.instructions).toContain('Sei l’agente personale dell’utente')
    expect(request.instructions).toContain('answer-only')
    expect(request.sources.some((source: { plaintext: string }) =>
      source.plaintext.includes('Preparare newsletter'))).toBe(true)
    expect(service.history(projectId, `agent:${request.input.observed_agent.agentId}`)).toHaveLength(2)
    expect(service.history(projectId)).toEqual([])
  })

  it('sends through the configured provider when the native bridge is absent', async () => {
    const vault = new MemoryVault()
    await new LocalAiProfileStore(vault).save({
      mode: 'commercial_api',
      provider: 'openai',
      credential: 'device-only-key',
      model: 'gpt-project',
      preferences: { timeoutMs: 30_000, maxOutputTokens: 512, maxAttempts: 1 },
    })
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({
      output: [{ content: [{ text: JSON.stringify({
        answer: 'Risposta dal provider esterno.',
        action_type: 'none',
        target_id: '',
        title: '',
        notes: '',
        priority: '',
        assignee_identity_id: '',
        name: '',
        email: '',
        role: '',
      }) }] }],
    }), { status: 200, headers: { 'Content-Type': 'application/json' } })))
    const service = new WorkspaceChatService(vault, () => undefined)

    expect(service.availability()).toMatchObject({
      profileConfigured: true,
      runtimeConnected: true,
      model: 'gpt-project',
    })
    const turns = await service.ask(snapshot(), 'Riassumi il progetto')

    expect(turns.at(-1)?.content).toBe('Risposta dal provider esterno.')
    expect(fetch).toHaveBeenCalledWith(
      'https://api.openai.com/v1/responses',
      expect.objectContaining({ method: 'POST', credentials: 'omit' }),
    )
  })
})
