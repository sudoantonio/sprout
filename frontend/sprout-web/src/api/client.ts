import type {
  AssignmentDto,
  AgentDirectoryItemDto,
  AttachmentDto,
  CreateAttachmentResponse,
  CreateInfoDocumentFileRequest,
  CreateInfoDocumentRequest,
  CreatePretaskTemplateAttachmentRequest,
  CreateQuestionnaireVersionRequest,
  CreateTaskCompletedAttachmentRequest,
  CreateTaskRequiredAttachmentRequest,
  ListAttachmentsResponse,
  ListAgentsResponse,
  ListInfoDocumentsResponse,
  DeviceKeyPackageView,
  DeviceSessionRequest,
  EmailStartResponse,
  EncryptedPayloadDto,
  FinalizeQuestionnaireSubmissionRequest,
  ListQuestionnaireVersionsResponse,
  ListResourceKeyEnvelopesResponse,
  ListQuestionnairesResponse,
  ListPresetsResponse,
  MaterializationChoiceDto,
  MaterializePresetResponse,
  InfoDocumentResponse,
  ListRetentionArchivesResponse,
  ListRetentionWarningsResponse,
  ListTaskListsResponse,
  ListTasksResponse,
  ListTopicsResponse,
  ProjectRecoveryProvisionStatus,
  ProjectRecoveryShareView,
  ProjectRecoveryStatus,
  ProjectDeviceKeyPackage,
  ProvisionProjectRecoveryRequest,
  ProvisionAgentResponse,
  ProjectInvitationDto,
  ProjectMemberDto,
  ProjectView,
  ParticipantSuggestionDto,
  PermissionGrantDto,
  ResourceEpochRotationDto,
  ResourceEpochInputDto,
  ResourceEnvelopePlanResponse,
  ResourceKeyEnvelopeDto,
  ResourceRotationPlanResponse,
  PresetResponse,
  PresetAssignmentResponse,
  PresetVersionResponse,
  PretaskDto,
  PretaskSelectionDto,
  PullSyncResponse,
  PushSyncRequest,
  PushSyncResponse,
  QuestionnaireResponse,
  QuestionnaireSubmissionResponse,
  QuestionnaireVersionResponse,
  RetentionPreferenceResponse,
  SessionResponse,
  TaskDto,
  TaskListDto,
  TopicDto,
  Uuid,
  UpdateQuestionnaireDraftRequest,
  UpdateInfoDocumentRequest,
  UpsertQuestionnaireDraftRequest,
  WebAuthnChallenge,
} from './contracts'

export class ApiError extends Error {
  readonly status: number
  readonly details: unknown

  constructor(status: number, message: string, details?: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.details = details
  }
}

interface RequestOptions<TBody> {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  body?: TBody
  signal?: AbortSignal
  authenticated?: boolean
}

export class ApiClient {
  readonly #baseUrl: string
  #token?: string

  constructor(baseUrl = '') {
    this.#baseUrl = baseUrl.replace(/\/$/, '')
  }

  setSession(token?: string): void {
    this.#token = token
  }

  get hasSession(): boolean {
    return Boolean(this.#token)
  }

  async request<TResponse, TBody = unknown>(
    path: string,
    options: RequestOptions<TBody> = {},
  ): Promise<TResponse> {
    const headers = new Headers({ Accept: 'application/json' })
    if (options.body !== undefined) {
      headers.set('Content-Type', 'application/json')
    }
    if (options.authenticated !== false) {
      if (!this.#token) {
        throw new ApiError(401, 'Authentication is required')
      }
      headers.set('Authorization', `Bearer ${this.#token}`)
    }

    let response: Response
    try {
      response = await fetch(`${this.#baseUrl}${path}`, {
        method: options.method ?? 'GET',
        headers,
        cache: 'no-store',
        credentials: 'same-origin',
        body:
          options.body === undefined
            ? undefined
            : JSON.stringify(options.body),
        signal: options.signal,
      })
    } catch (error) {
      throw new ApiError(
        0,
        navigator.onLine
          ? 'The API could not be reached'
          : 'You are offline; encrypted changes remain queued',
        error,
      )
    }

    const isJson = response.headers
      .get('Content-Type')
      ?.includes('application/json')
    const payload: unknown = isJson ? await response.json() : undefined
    if (!response.ok) {
      let serverMessage: string | undefined
      if (typeof payload === 'object' && payload !== null && 'error' in payload) {
        const detail = (payload as { error: unknown }).error
        if (typeof detail === 'string') {
          serverMessage = detail
        } else if (
          typeof detail === 'object' &&
          detail !== null &&
          'message' in detail &&
          typeof (detail as { message: unknown }).message === 'string'
        ) {
          const code =
            'code' in detail && typeof (detail as { code: unknown }).code === 'string'
              ? (detail as { code: string }).code
              : undefined
          const message = (detail as { message: string }).message
          serverMessage = code ? `${code}: ${message}` : message
        }
      }
      throw new ApiError(
        response.status,
        serverMessage ??
          (response.status === 429
            ? 'Troppe richieste, riprova tra qualche secondo'
            : `Request failed with status ${response.status}`),
        payload,
      )
    }
    return payload as TResponse
  }

  startEmailVerification(input: {
    email: string
    identity_handle: string
    encrypted_profile_b64: string
  }): Promise<EmailStartResponse> {
    return this.request('/v1/auth/email/verification/start', {
      method: 'POST',
      authenticated: false,
      body: input,
    })
  }

  finishEmailVerification(input: {
    identity_id: Uuid
    token: string
  } & DeviceSessionRequest): Promise<SessionResponse> {
    return this.request('/v1/auth/email/verification/finish', {
      method: 'POST',
      authenticated: false,
      body: input,
    })
  }

  startEmailRecovery(email: string): Promise<EmailStartResponse> {
    return this.request('/v1/auth/email/recovery/start', {
      method: 'POST',
      authenticated: false,
      body: { email },
    })
  }

  finishEmailRecovery(input: {
    identity_id: Uuid
    token: string
  } & DeviceSessionRequest): Promise<SessionResponse> {
    return this.request('/v1/auth/email/recovery/finish', {
      method: 'POST',
      authenticated: false,
      body: input,
    })
  }

  devLogin(input: {
    email?: string
    identity_handle?: string
  } & DeviceSessionRequest): Promise<SessionResponse> {
    return this.request('/v1/auth/dev/login', {
      method: 'POST',
      authenticated: false,
      body: input,
    })
  }

  startPasskeyRegistration(): Promise<WebAuthnChallenge<unknown>> {
    return this.request('/v1/auth/passkeys/register/start', {
      method: 'POST',
    })
  }

  finishPasskeyRegistration(
    challengeId: Uuid,
    credential: unknown,
  ): Promise<{ passkey_id: Uuid }> {
    return this.request('/v1/auth/passkeys/register/finish', {
      method: 'POST',
      body: { challenge_id: challengeId, credential },
    })
  }

  startPasskeyAuthentication(input: {
    identity_id: Uuid
    identity_handle: string
  }): Promise<WebAuthnChallenge<unknown>> {
    return this.request('/v1/auth/passkeys/authenticate/start', {
      method: 'POST',
      authenticated: false,
      body: input,
    })
  }

  finishPasskeyAuthentication(input: {
    identity_id: Uuid
    challenge_id: Uuid
    credential: unknown
  } & DeviceSessionRequest): Promise<SessionResponse> {
    return this.request('/v1/auth/passkeys/authenticate/finish', {
      method: 'POST',
      authenticated: false,
      body: input,
    })
  }

  registerDevicePackage(
    deviceId: Uuid,
    packageB64: string,
  ): Promise<{
    device_id: Uuid
    key_version: number
    generation: number
    package_hash_b64: string
    status: string
    suite_status: string
  }> {
    return this.request(`/v1/devices/${encodeURIComponent(deviceId)}/key-packages`, {
      method: 'POST',
      body: {
        package_b64: packageB64,
        previous_classical_signature_b64: null,
        previous_post_quantum_signature_b64: null,
      },
    })
  }

  listDevicePackages(deviceId: Uuid): Promise<DeviceKeyPackageView[]> {
    return this.request(
      `/v1/devices/${encodeURIComponent(deviceId)}/key-packages`,
    )
  }

  listProjects(signal?: AbortSignal): Promise<ProjectView[]> {
    return this.request('/v1/projects', { signal })
  }

  listProjectDevicePackages(
    projectId: Uuid,
  ): Promise<ProjectDeviceKeyPackage[]> {
    return this.request(
      `/v1/projects/${projectId}/device-key-packages`,
    )
  }

  listResourceKeyEnvelopes(
    projectId: Uuid,
  ): Promise<ListResourceKeyEnvelopesResponse> {
    return this.request(
      `/v1/projects/${projectId}/resource-key-envelopes`,
    )
  }

  createProject(input: {
    id: Uuid
    encrypted_metadata_b64: string
  }): Promise<ProjectView> {
    return this.request('/v1/projects', {
      method: 'POST',
      body: input,
    })
  }

  initializeResourceEpoch(
    projectId: Uuid,
    resourceId: Uuid,
    input: {
      epoch: ResourceEpochInputDto
      envelopes: ResourceKeyEnvelopeDto[]
    },
  ): Promise<void> {
    return this.request(
      `/v1/projects/${projectId}/resources/${resourceId}/epochs`,
      { method: 'POST', body: input },
    )
  }

  getFullResourceEnvelopePlan(
    projectId: Uuid,
    resourceId: Uuid,
  ): Promise<ResourceEnvelopePlanResponse> {
    return this.request(
      `/v1/projects/${projectId}/resources/${resourceId}/envelope-plan`,
    )
  }

  grantResourcePermission(
    projectId: Uuid,
    resourceId: Uuid,
    input: {
      grant_id: Uuid
      user_id: Uuid
      resource_id: Uuid
      access_level: 'view' | 'comment' | 'edit' | 'manage'
      access_scope: 'full' | 'container_only'
      visibility: 'restricted'
      envelopes: ResourceKeyEnvelopeDto[]
      idempotency_key: string
    },
  ): Promise<void> {
    return this.request(
      `/v1/projects/${projectId}/resources/${resourceId}/permissions`,
      { method: 'POST', body: input },
    )
  }

  listResourcePermissions(
    projectId: Uuid,
    resourceId: Uuid,
  ): Promise<{ grants: PermissionGrantDto[] }> {
    return this.request(
      `/v1/projects/${projectId}/resources/${resourceId}/permissions`,
    )
  }

  getResourceRotationPlan(
    projectId: Uuid,
    resourceId: Uuid,
    grantId: Uuid,
  ): Promise<ResourceRotationPlanResponse> {
    return this.request(
      `/v1/projects/${projectId}/resources/${resourceId}/permissions/${grantId}/rotation-plan`,
    )
  }

  revokeResourcePermission(
    projectId: Uuid,
    resourceId: Uuid,
    grantId: Uuid,
    input: {
      user_id: Uuid
      rotations: ResourceEpochRotationDto[]
      encrypted_admin_notification_b64: string | null
      idempotency_key: Uuid
    },
  ): Promise<PermissionGrantDto> {
    return this.request(
      `/v1/projects/${projectId}/resources/${resourceId}/permissions/${grantId}`,
      { method: 'DELETE', body: input },
    )
  }

  shareMemberResourceKeys(
    projectId: Uuid,
    input: {
      recipient_identity_id: Uuid
      resource_ids: Uuid[]
      envelopes: ResourceKeyEnvelopeDto[]
    },
  ): Promise<void> {
    return this.request(
      `/v1/projects/${projectId}/member-resource-keys`,
      { method: 'POST', body: input },
    )
  }

  listProjectInvitations(projectId: Uuid): Promise<ProjectInvitationDto[]> {
    return this.request(`/v1/projects/${projectId}/invitations`)
  }

  listProjectMembers(projectId: Uuid): Promise<ProjectMemberDto[]> {
    return this.request(`/v1/projects/${projectId}/members`)
  }

  updateProjectMemberResponsibilities(
    projectId: Uuid,
    memberIdentityId: Uuid,
    responsibilities: string,
  ): Promise<{ responsibilities: string | null }> {
    return this.request(
      `/v1/projects/${projectId}/members/${memberIdentityId}`,
      { method: 'PATCH', body: { responsibilities } },
    )
  }

  createProjectInvitation(
    projectId: Uuid,
    input: {
      invitee_email: string
      encrypted_payload_b64: string
      role: 'admin' | 'member' | 'guest'
      expires_in_seconds: number
    },
  ): Promise<{ id: Uuid; expires_at: string }> {
    return this.request(`/v1/projects/${projectId}/invitations`, {
      method: 'POST',
      body: input,
    })
  }

  acceptProjectInvitation(
    projectId: Uuid,
    invitationId: Uuid,
    token: string,
  ): Promise<{ accepted: boolean }> {
    return this.request(`/v1/projects/${projectId}/invitations/accept`, {
      method: 'POST',
      body: { invitation_id: invitationId, token },
    })
  }

  suggestProjectParticipants(
    projectId: Uuid,
    prefix: string,
    limit = 20,
  ): Promise<ParticipantSuggestionDto[]> {
    return this.request(
      `/v1/projects/${projectId}/participant-suggestions`,
      {
        method: 'POST',
        body: { prefix, limit },
      },
    )
  }

  listAgents(projectId: Uuid): Promise<AgentDirectoryItemDto[]> {
    return this.request<ListAgentsResponse>(
      `/v1/projects/${encodeURIComponent(projectId)}/agents`,
    ).then((response) => response.agents)
  }

  provisionAgent(
    projectId: Uuid,
    input: unknown,
  ): Promise<ProvisionAgentResponse> {
    return this.request(
      `/v1/projects/${encodeURIComponent(projectId)}/agents`,
      { method: 'POST', body: input },
    )
  }

  listTopics(projectId: Uuid): Promise<ListTopicsResponse> {
    return this.request(`/v1/projects/${projectId}/topics`)
  }

  createTopic(projectId: Uuid, body: unknown): Promise<{ topic: TopicDto }> {
    return this.request(`/v1/projects/${projectId}/topics`, {
      method: 'POST',
      body,
    })
  }

  updateTopic(
    projectId: Uuid,
    topicId: Uuid,
    body: unknown,
  ): Promise<{ topic: TopicDto }> {
    return this.request(`/v1/projects/${projectId}/topics/${topicId}`, {
      method: 'PUT',
      body,
    })
  }

  deleteTopic(projectId: Uuid, topicId: Uuid): Promise<void> {
    return this.request(`/v1/projects/${projectId}/topics/${topicId}`, {
      method: 'DELETE',
    })
  }

  listTaskLists(
    projectId: Uuid,
    topicId: Uuid,
  ): Promise<ListTaskListsResponse> {
    return this.request(
      `/v1/projects/${projectId}/topics/${topicId}/task-lists`,
    )
  }

  createTaskList(
    projectId: Uuid,
    topicId: Uuid,
    body: unknown,
  ): Promise<{ task_list: TaskListDto }> {
    return this.request(
      `/v1/projects/${projectId}/topics/${topicId}/task-lists`,
      { method: 'POST', body },
    )
  }

  updateTaskList(
    projectId: Uuid,
    listId: Uuid,
    body: unknown,
  ): Promise<{ task_list: TaskListDto }> {
    return this.request(`/v1/projects/${projectId}/task-lists/${listId}`, {
      method: 'PUT',
      body,
    })
  }

  listTaskListInfoDocuments(
    projectId: Uuid,
    listId: Uuid,
  ): Promise<ListInfoDocumentsResponse> {
    return this.request(
      `/v1/projects/${projectId}/task-lists/${listId}/info-documents`,
    )
  }

  listProjectInfoDocuments(
    projectId: Uuid,
  ): Promise<ListInfoDocumentsResponse> {
    return this.request(`/v1/projects/${projectId}/info-documents`)
  }

  listTopicInfoDocuments(
    projectId: Uuid,
    topicId: Uuid,
  ): Promise<ListInfoDocumentsResponse> {
    return this.request(
      `/v1/projects/${projectId}/topics/${topicId}/info-documents`,
    )
  }

  createTaskListInfoDocument(
    projectId: Uuid,
    listId: Uuid,
    body: CreateInfoDocumentRequest,
  ): Promise<InfoDocumentResponse> {
    return this.request(
      `/v1/projects/${projectId}/task-lists/${listId}/info-documents`,
      { method: 'POST', body },
    )
  }

  createProjectInfoDocument(
    projectId: Uuid,
    body: CreateInfoDocumentRequest,
  ): Promise<InfoDocumentResponse> {
    return this.request(`/v1/projects/${projectId}/info-documents`, {
      method: 'POST',
      body,
    })
  }

  createTopicInfoDocument(
    projectId: Uuid,
    topicId: Uuid,
    body: CreateInfoDocumentRequest,
  ): Promise<InfoDocumentResponse> {
    return this.request(
      `/v1/projects/${projectId}/topics/${topicId}/info-documents`,
      { method: 'POST', body },
    )
  }

  updateInfoDocument(
    projectId: Uuid,
    documentId: Uuid,
    body: UpdateInfoDocumentRequest,
  ): Promise<InfoDocumentResponse> {
    return this.request(
      `/v1/projects/${projectId}/info-documents/${documentId}`,
      { method: 'PUT', body },
    )
  }

  deleteInfoDocument(projectId: Uuid, documentId: Uuid): Promise<void> {
    return this.request(
      `/v1/projects/${projectId}/info-documents/${documentId}`,
      { method: 'DELETE' },
    )
  }

  listTasks(projectId: Uuid, listId: Uuid): Promise<ListTasksResponse> {
    return this.request(
      `/v1/projects/${projectId}/task-lists/${listId}/tasks`,
    )
  }

  createTask(projectId: Uuid, body: unknown): Promise<{ task: TaskDto }> {
    return this.request(`/v1/projects/${projectId}/tasks`, {
      method: 'POST',
      body,
    })
  }

  listTaskAssignments(
    projectId: Uuid,
    taskId: Uuid,
  ): Promise<{
    assignments: AssignmentDto[]
    active_assignment_id: Uuid | null
  }> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/assignments`,
    )
  }

  assignTask(
    projectId: Uuid,
    taskId: Uuid,
    input: {
      assignment_id: Uuid
      permission_grant_id: Uuid
      assignee_identity_id: Uuid
      encrypted_payload_b64: string
      envelopes: ResourceKeyEnvelopeDto[]
      idempotency_key: Uuid
    },
  ): Promise<{ assignment: AssignmentDto }> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/assignments`,
      { method: 'POST', body: input },
    )
  }

  revokeTaskAssignment(
    projectId: Uuid,
    taskId: Uuid,
    assignmentId: Uuid,
    input: {
      rotations: ResourceEpochRotationDto[]
      idempotency_key: Uuid
    },
  ): Promise<{ assignment: AssignmentDto }> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/assignments/${assignmentId}`,
      { method: 'DELETE', body: input },
    )
  }

  createRecurrence(projectId: Uuid, body: unknown): Promise<unknown> {
    return this.request(
      `/v1/projects/${projectId}/recurrence-series`,
      { method: 'POST', body },
    )
  }

  updateTask(
    projectId: Uuid,
    taskId: Uuid,
    body: unknown,
  ): Promise<{ task: TaskDto }> {
    return this.request(`/v1/projects/${projectId}/tasks/${taskId}`, {
      method: 'PUT',
      body,
    })
  }

  deleteTask(projectId: Uuid, taskId: Uuid): Promise<void> {
    return this.request(`/v1/projects/${projectId}/tasks/${taskId}`, {
      method: 'DELETE',
    })
  }

  completeTask(
    projectId: Uuid,
    taskId: Uuid,
    body: unknown,
  ): Promise<{
    completed_task: TaskDto
    next_task: TaskDto | null
    replayed: boolean
  }> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/complete`,
      { method: 'POST', body },
    )
  }

  copyTask(
    projectId: Uuid,
    taskId: Uuid,
    body: unknown,
  ): Promise<{ task: TaskDto }> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/copy`,
      { method: 'POST', body },
    )
  }

  getPreset(projectId: Uuid, presetId: Uuid): Promise<PresetResponse> {
    return this.request(`/v1/projects/${projectId}/presets/${presetId}`)
  }

  listPresets(
    projectId: Uuid,
    cursor?: string,
    limit = 100,
  ): Promise<ListPresetsResponse> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (cursor) query.set('cursor', cursor)
    return this.request(`/v1/projects/${projectId}/presets?${query}`)
  }

  createPreset(
    projectId: Uuid,
    id: Uuid,
    payload: EncryptedPayloadDto,
  ): Promise<PresetResponse> {
    return this.request(`/v1/projects/${projectId}/presets`, {
      method: 'POST',
      body: { id, payload, idempotency_key: crypto.randomUUID() },
    })
  }

  updatePreset(
    projectId: Uuid,
    presetId: Uuid,
    payload: EncryptedPayloadDto,
  ): Promise<PresetResponse> {
    return this.request(`/v1/projects/${projectId}/presets/${presetId}`, {
      method: 'PUT',
      body: { payload, idempotency_key: crypto.randomUUID() },
    })
  }

  deletePreset(projectId: Uuid, presetId: Uuid): Promise<void> {
    return this.request(`/v1/projects/${projectId}/presets/${presetId}`, {
      method: 'DELETE',
    })
  }

  createPresetVersion(
    projectId: Uuid,
    presetId: Uuid,
    input: {
      id: Uuid
      payload: EncryptedPayloadDto
      content_hash_b64: string
      pretasks: PretaskDto[]
      idempotency_key: Uuid
    },
  ): Promise<PresetVersionResponse> {
    return this.request(
      `/v1/projects/${projectId}/presets/${presetId}/versions`,
      { method: 'POST', body: input },
    )
  }

  getPresetVersion(
    projectId: Uuid,
    presetId: Uuid,
    versionId: Uuid,
  ): Promise<PresetVersionResponse> {
    return this.request(
      `/v1/projects/${projectId}/presets/${presetId}/versions/${versionId}`,
    )
  }

  createPresetAssignment(
    projectId: Uuid,
    input: {
      id: Uuid
      preset_version_id: Uuid
      destination_list_id: Uuid
      assigned_to_identity_id: Uuid
      payload: EncryptedPayloadDto
      selections: PretaskSelectionDto[]
      idempotency_key: Uuid
    },
  ): Promise<PresetAssignmentResponse> {
    return this.request(`/v1/projects/${projectId}/preset-assignments`, {
      method: 'POST',
      body: input,
    })
  }

  materializePresetAssignment(
    projectId: Uuid,
    assignmentId: Uuid,
    input: {
      expected_assignment_version: number
      choices: MaterializationChoiceDto[]
      idempotency_key: Uuid
    },
  ): Promise<MaterializePresetResponse> {
    return this.request(
      `/v1/projects/${projectId}/preset-assignments/${assignmentId}/materialize`,
      { method: 'POST', body: input },
    )
  }

  getQuestionnaire(
    projectId: Uuid,
    questionnaireId: Uuid,
  ): Promise<QuestionnaireResponse> {
    return this.request(
      `/v1/projects/${projectId}/questionnaires/${questionnaireId}`,
    )
  }

  listQuestionnaires(
    projectId: Uuid,
    cursor?: string,
    limit = 100,
  ): Promise<ListQuestionnairesResponse> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (cursor) query.set('cursor', cursor)
    return this.request(
      `/v1/projects/${projectId}/questionnaires?${query}`,
    )
  }

  createQuestionnaire(
    projectId: Uuid,
    id: Uuid,
    payload: EncryptedPayloadDto,
  ): Promise<QuestionnaireResponse> {
    return this.request(`/v1/projects/${projectId}/questionnaires`, {
      method: 'POST',
      body: {
        id,
        payload,
        idempotency_key: crypto.randomUUID(),
      },
    })
  }

  listQuestionnaireVersions(
    projectId: Uuid,
    questionnaireId: Uuid,
  ): Promise<ListQuestionnaireVersionsResponse> {
    return this.request(
      `/v1/projects/${projectId}/questionnaires/${questionnaireId}/versions`,
    )
  }

  getQuestionnaireVersion(
    projectId: Uuid,
    questionnaireId: Uuid,
    versionId: Uuid,
  ): Promise<QuestionnaireVersionResponse> {
    return this.request(
      `/v1/projects/${projectId}/questionnaires/${questionnaireId}/versions/${versionId}`,
    )
  }

  createQuestionnaireVersion(
    projectId: Uuid,
    questionnaireId: Uuid,
    body: CreateQuestionnaireVersionRequest,
  ): Promise<QuestionnaireVersionResponse> {
    return this.request(
      `/v1/projects/${projectId}/questionnaires/${questionnaireId}/versions`,
      { method: 'POST', body },
    )
  }

  updateQuestionnaireDraft(
    projectId: Uuid,
    questionnaireId: Uuid,
    versionId: Uuid,
    body: UpdateQuestionnaireDraftRequest,
  ): Promise<QuestionnaireVersionResponse> {
    return this.request(
      `/v1/projects/${projectId}/questionnaires/${questionnaireId}/versions/${versionId}`,
      { method: 'PUT', body },
    )
  }

  publishQuestionnaireVersion(
    projectId: Uuid,
    questionnaireId: Uuid,
    versionId: Uuid,
    expectedRevision: number,
  ): Promise<QuestionnaireVersionResponse> {
    return this.request(
      `/v1/projects/${projectId}/questionnaires/${questionnaireId}/versions/${versionId}/publish`,
      {
        method: 'POST',
        body: {
          expected_revision: expectedRevision,
          idempotency_key: crypto.randomUUID(),
        },
      },
    )
  }

  getQuestionnaireSubmission(
    projectId: Uuid,
    taskId: Uuid,
  ): Promise<QuestionnaireSubmissionResponse> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/questionnaire-submission`,
    )
  }

  upsertQuestionnaireSubmissionDraft(
    projectId: Uuid,
    taskId: Uuid,
    body: UpsertQuestionnaireDraftRequest,
  ): Promise<QuestionnaireSubmissionResponse> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/questionnaire-submission`,
      { method: 'PUT', body },
    )
  }

  submitQuestionnaire(
    projectId: Uuid,
    taskId: Uuid,
    body: FinalizeQuestionnaireSubmissionRequest,
  ): Promise<QuestionnaireSubmissionResponse> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/questionnaire-submission/submit`,
      { method: 'POST', body },
    )
  }

  getAttachment(projectId: Uuid, blobId: Uuid): Promise<AttachmentDto> {
    return this.request(`/v1/projects/${projectId}/files/${blobId}`)
  }

  declarePretaskTemplateAttachment(
    projectId: Uuid,
    versionId: Uuid,
    pretaskId: Uuid,
    body: CreatePretaskTemplateAttachmentRequest,
  ): Promise<CreateAttachmentResponse> {
    return this.request(
      `/v1/projects/${projectId}/preset-versions/${versionId}/pretasks/${pretaskId}/attachments`,
      { method: 'POST', body },
    )
  }

  declareTaskRequiredAttachment(
    projectId: Uuid,
    taskId: Uuid,
    body: CreateTaskRequiredAttachmentRequest,
  ): Promise<CreateAttachmentResponse> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/required-attachments`,
      { method: 'POST', body },
    )
  }

  declareTaskCompletedAttachment(
    projectId: Uuid,
    taskId: Uuid,
    body: CreateTaskCompletedAttachmentRequest,
  ): Promise<CreateAttachmentResponse> {
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/completed-attachments`,
      { method: 'POST', body },
    )
  }

  declareInfoDocumentFile(
    projectId: Uuid,
    documentId: Uuid,
    body: CreateInfoDocumentFileRequest,
  ): Promise<CreateAttachmentResponse> {
    return this.request(
      `/v1/projects/${projectId}/info-documents/${documentId}/files`,
      { method: 'POST', body },
    )
  }

  listPretaskAttachments(
    projectId: Uuid,
    versionId: Uuid,
    pretaskId: Uuid,
    cursor?: string,
    limit = 100,
  ): Promise<ListAttachmentsResponse> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (cursor) query.set('cursor', cursor)
    return this.request(
      `/v1/projects/${projectId}/preset-versions/${versionId}/pretasks/${pretaskId}/attachments?${query}`,
    )
  }

  listTaskRequiredAttachments(
    projectId: Uuid,
    taskId: Uuid,
    cursor?: string,
    limit = 100,
  ): Promise<ListAttachmentsResponse> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (cursor) query.set('cursor', cursor)
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/required-attachments?${query}`,
    )
  }

  listTaskCompletedAttachments(
    projectId: Uuid,
    taskId: Uuid,
    cursor?: string,
    limit = 100,
  ): Promise<ListAttachmentsResponse> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (cursor) query.set('cursor', cursor)
    return this.request(
      `/v1/projects/${projectId}/tasks/${taskId}/completed-attachments?${query}`,
    )
  }

  async uploadAttachmentCiphertext(
    projectId: Uuid,
    blobId: Uuid,
    ciphertext: Blob,
    uploadUrl = `/v1/projects/${projectId}/files/${blobId}/content`,
  ): Promise<void> {
    const expectedPath = `/v1/projects/${projectId}/files/${blobId}/content`
    if (uploadUrl !== expectedPath) {
      throw new ApiError(0, 'The attachment upload URL was not the expected same-origin route')
    }
    if (!this.#token) {
      throw new ApiError(401, 'Authentication is required')
    }
    const response = await fetch(`${this.#baseUrl}${expectedPath}`, {
      method: 'PUT',
      headers: {
        Authorization: `Bearer ${this.#token}`,
        'Content-Type': 'application/octet-stream',
      },
      cache: 'no-store',
      credentials: 'same-origin',
      body: ciphertext,
    })
    if (!response.ok) {
      throw new ApiError(response.status, 'Encrypted upload failed')
    }
  }

  async finalizeAttachment(
    projectId: Uuid,
    blobId: Uuid,
  ): Promise<AttachmentDto> {
    const attachment = await this.getAttachment(projectId, blobId)
    if (attachment.state.state !== 'available') {
      throw new ApiError(409, 'Encrypted upload is not available yet')
    }
    return attachment
  }

  async downloadCiphertext(path: string): Promise<Blob> {
    if (!this.#token) {
      throw new ApiError(401, 'Authentication is required')
    }
    const response = await fetch(`${this.#baseUrl}${path}`, {
      headers: { Authorization: `Bearer ${this.#token}` },
      cache: 'no-store',
      credentials: 'same-origin',
    })
    if (!response.ok) {
      throw new ApiError(response.status, 'Encrypted download failed')
    }
    return new Blob([await response.arrayBuffer()], {
      type: 'application/octet-stream',
    })
  }

  getRetentionPreference(): Promise<RetentionPreferenceResponse> {
    return this.request('/v1/retention/preferences')
  }

  updateRetentionPreference(
    autoExportEnabled: boolean,
  ): Promise<RetentionPreferenceResponse> {
    return this.request('/v1/retention/preferences', {
      method: 'PUT',
      body: { auto_export_enabled: autoExportEnabled },
    })
  }

  listRetentionArchives(): Promise<ListRetentionArchivesResponse> {
    return this.request('/v1/retention/archives')
  }

  listRetentionWarnings(): Promise<ListRetentionWarningsResponse> {
    return this.request('/v1/retention/warnings')
  }

  recordArchiveReceipt(
    archiveId: Uuid,
    ciphertextSha256B64: string,
  ): Promise<unknown> {
    return this.request(`/v1/retention/archives/${archiveId}/receipt`, {
      method: 'POST',
      body: { ciphertext_sha256: ciphertextSha256B64 },
    })
  }

  getProjectRecoveryProvision(
    projectId: Uuid,
  ): Promise<ProjectRecoveryProvisionStatus> {
    return this.request(`/v1/projects/${projectId}/recovery-provision`)
  }

  provisionProjectRecovery(
    projectId: Uuid,
    body: ProvisionProjectRecoveryRequest,
  ): Promise<ProjectRecoveryProvisionStatus> {
    return this.request(`/v1/projects/${projectId}/recovery-provision`, {
      method: 'PUT',
      body,
    })
  }

  activateProjectRecovery(
    projectId: Uuid,
    recoverySetId: Uuid,
  ): Promise<ProjectRecoveryProvisionStatus> {
    return this.request(
      `/v1/projects/${projectId}/recovery-provision/activate`,
      { method: 'POST', body: { recovery_set_id: recoverySetId } },
    )
  }

  listMyRecoveryShares(projectId: Uuid): Promise<{
    shares: ProjectRecoveryShareView[]
  }> {
    return this.request(
      `/v1/projects/${projectId}/recovery-provision/shares/me`,
    )
  }

  getProjectRecoveryRotationPlan(projectId: Uuid): Promise<{
    resources: Array<{
      resource_id: Uuid
      previous_epoch_id: Uuid
      current_epoch: number
      previous_key_commitment_b64: string
      previous_header_key_commitment_b64: string | null
      recipient_identity_ids: Uuid[]
      body_recipient_identity_ids: Uuid[]
      header_recipient_identity_ids: Uuid[]
    }>
  }> {
    return this.request(`/v1/projects/${projectId}/recovery-rotation-plan`)
  }

  startProjectRecovery(
    projectId: Uuid,
    body: unknown,
  ): Promise<ProjectRecoveryStatus> {
    return this.request(`/v1/projects/${projectId}/recovery-requests`, {
      method: 'POST',
      body,
    })
  }

  getProjectRecovery(
    projectId: Uuid,
    requestId: Uuid,
  ): Promise<ProjectRecoveryStatus> {
    return this.request(
      `/v1/projects/${projectId}/recovery-requests/${requestId}`,
    )
  }

  approveProjectRecovery(
    projectId: Uuid,
    requestId: Uuid,
    body: unknown,
  ): Promise<ProjectRecoveryStatus> {
    return this.request(
      `/v1/projects/${projectId}/recovery-requests/${requestId}/approvals`,
      { method: 'POST', body },
    )
  }

  finalizeProjectRecovery(
    projectId: Uuid,
    requestId: Uuid,
    body: {
      new_device_key_version: number
      rotations: ResourceEpochRotationDto[]
      replacement_recovery: ProvisionProjectRecoveryRequest
    },
  ): Promise<{
    request_id: Uuid
    status: string
    owner_epoch: number
    device_epoch: number
    device_generation: number
    recovery_epoch: number
  }> {
    return this.request(
      `/v1/projects/${projectId}/recovery-requests/${requestId}/finalize`,
      { method: 'POST', body },
    )
  }

  pullSync(
    projectId: Uuid,
    afterSequence: number,
    limit = 100,
  ): Promise<PullSyncResponse> {
    return this.request('/v1/sync/pull', {
      method: 'POST',
      body: {
        project_id: projectId,
        after_sequence: afterSequence,
        limit,
      },
    })
  }

  pushSync(request: PushSyncRequest): Promise<PushSyncResponse> {
    return this.request('/v1/sync/push', {
      method: 'POST',
      body: request,
    })
  }

  openSyncWake(projectId: Uuid): WebSocket {
    if (!this.#token) {
      throw new ApiError(401, 'Authentication is required')
    }
    const base = new URL(this.#baseUrl || '/', window.location.href)
    base.protocol = base.protocol === 'https:' ? 'wss:' : 'ws:'
    base.pathname = '/v1/sync/wake'
    base.search = new URLSearchParams({ project_id: projectId }).toString()
    return new WebSocket(base, [`sprout-auth.${this.#token}`])
  }
}
