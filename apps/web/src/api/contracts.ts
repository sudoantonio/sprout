export type Uuid = string
export type IsoDateTime = string

export interface EncryptedPayloadDto {
  version: number
  algorithm: string
  key_id: string
  nonce_b64: string
  ciphertext_b64: string
}

export interface DeviceSessionRequest {
  device_id: Uuid
  device_kind: 'web'
  encrypted_device_label_b64: string
}

export interface SessionResponse {
  token: string
  expires_at: IsoDateTime
  identity_id: Uuid
  device_id: Uuid
}

export interface EmailStartResponse {
  accepted: boolean
}

export interface WebAuthnChallenge<TOptions> {
  challenge_id: Uuid
  options: TOptions
}

export interface ProjectView {
  id: Uuid
  root_resource_id: Uuid
  owner_identity_id: Uuid
  encrypted_metadata_b64: string
  key_epoch: number
  status: string
  created_at: IsoDateTime
  updated_at: IsoDateTime
}

export interface ProjectInvitationDto {
  id: Uuid
  role: 'admin' | 'member' | 'guest'
  state: string
  accepted_by_identity_id: Uuid | null
  keys_shared: boolean
  created_at: IsoDateTime
  expires_at: IsoDateTime
}

export interface ParticipantSuggestionDto {
  identity_id: Uuid
  identity_handle: string
  shared_project_count: number
  most_recent_shared_project_at: IsoDateTime
}

export interface ProjectDeviceKeyPackage {
  identity_id: Uuid
  device_id: Uuid
  key_version: number
  generation: number
  package_b64: string
  package_hash_b64: string
  suite_status: string
}

export interface DeviceKeyPackageView {
  device_id: Uuid
  key_version: number
  generation: number
  package_b64: string
  package_hash_b64: string
  created_at: IsoDateTime
  revoked_at: IsoDateTime | null
  suite_status: string
}

export interface ResourceKeyEnvelopeDto {
  version: number
  resource_id: Uuid
  epoch: number
  key_purpose?: 'body' | 'header'
  recipient_identity_id: Uuid
  recipient_device_id: Uuid
  recipient_device_key_version: number
  sender_device_key_version: number
  encrypted_key_b64: string
  sender_signature_b64: string
  sender_post_quantum_signature_b64: string
}

export interface ResourceKeyEnvelopeViewDto extends ResourceKeyEnvelopeDto {
  sender_identity_id: Uuid
  sender_device_id: Uuid
  previous_epoch_hash_b64: string | null
}

export interface ListResourceKeyEnvelopesResponse {
  envelopes: ResourceKeyEnvelopeViewDto[]
}

export interface ResourceEpochInputDto {
  id: Uuid
  epoch: number
  creator_device_key_version: number
  key_commitment_b64: string
  header_key_commitment_b64?: string | null
}

export interface ResourceEnvelopePlanResponse {
  resources: Array<{
    resource_id: Uuid
    epoch_id: Uuid
    epoch: number
    key_commitment_b64: string
    previous_epoch_hash_b64: string | null
  }>
}

export interface ResourceRotationPlanResponse {
  revoked_identity_id: Uuid
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
}

export interface ResourceEpochRotationDto {
  epoch_id: Uuid
  resource_id: Uuid
  previous_epoch_id: Uuid
  new_epoch: number
  creator_device_key_version: number
  key_commitment_b64: string
  header_key_commitment_b64?: string | null
  envelopes: ResourceKeyEnvelopeDto[]
}

export interface PermissionGrantDto {
  id: Uuid
  root_grant_id: Uuid
  user_id: Uuid
  resource_id: Uuid
  access_level: 'view' | 'comment' | 'edit' | 'manage'
  access_scope: 'full' | 'container_only'
  origin:
    | { type: 'direct' }
    | { type: 'assignment'; assignment_id: Uuid }
    | {
        type: 'inherited'
        root_grant_id: Uuid
        root_resource_id: Uuid
      }
  granted_at: IsoDateTime
  revoked_at: IsoDateTime | null
}

export interface AssignmentDto {
  id: Uuid
  project_id: Uuid
  task_id: Uuid
  assignee_identity_id: Uuid
  assigned_by_identity_id: Uuid
  permission_root_grant_id: Uuid
  assigned_at: IsoDateTime
  revoked_at: IsoDateTime | null
}

export interface TopicDto {
  id: Uuid
  project_id: Uuid
  resource_node_id: Uuid
  payload: EncryptedPayloadDto | null
  header?: EncryptedPayloadDto | null
  key_epoch: number
  payload_version: number
  created_at: IsoDateTime
  deleted_at: IsoDateTime | null
}

export interface TaskListDto {
  id: Uuid
  project_id: Uuid
  topic_id: Uuid
  resource_node_id: Uuid
  payload: EncryptedPayloadDto | null
  header?: EncryptedPayloadDto | null
  key_epoch: number
  payload_version: number
  created_at: IsoDateTime
  archived_at: IsoDateTime | null
}

export type TaskStateDto =
  | { state: 'open' }
  | {
      state: 'completed'
      completed_by: Uuid
      completed_at: IsoDateTime
    }

export interface TaskDto {
  id: Uuid
  project_id: Uuid
  list_id: Uuid
  resource_node_id: Uuid
  task_kind: 'priority' | 'deadline' | 'recurring'
  payload: EncryptedPayloadDto | null
  header?: EncryptedPayloadDto | null
  selected_value_snapshot: EncryptedPayloadDto | null
  key_epoch: number
  state: TaskStateDto
  source_pretask_id: Uuid | null
  preset_assignment_id: Uuid | null
  copied_from_task_id: Uuid | null
  questionnaire_version_id: Uuid | null
  recurrence_series_id: Uuid | null
  occurrence_number: number | null
  active_assignment_id: Uuid | null
  active_assignee_identity_id: Uuid | null
  created_at: IsoDateTime
  payload_version: number
}

export interface ListTopicsResponse {
  topics: TopicDto[]
}

export interface ListTaskListsResponse {
  task_lists: TaskListDto[]
}

export interface ListTasksResponse {
  tasks: TaskDto[]
}

export interface PresetDto {
  id: Uuid
  project_id: Uuid
  payload: EncryptedPayloadDto
  created_at: IsoDateTime
  deleted_at: IsoDateTime | null
}

export interface PresetResponse {
  preset: PresetDto
}

export interface ListPresetsResponse {
  presets: PresetDto[]
  next_cursor: string | null
}

export type TaskKindDto = 'priority' | 'deadline' | 'recurring'

export interface PretaskDto {
  id: Uuid
  task_kind: TaskKindDto
  payload: EncryptedPayloadDto
}

export interface PresetVersionDto {
  id: Uuid
  preset_id: Uuid
  project_id: Uuid
  version_number: number
  payload: EncryptedPayloadDto
  pretasks: PretaskDto[]
  created_at: IsoDateTime
}

export interface PresetVersionResponse {
  version: PresetVersionDto
}

export interface PretaskSelectionDto {
  pretask_id: Uuid
  task_kind: TaskKindDto
  selected_value: EncryptedPayloadDto
}

export interface PresetAssignmentDto {
  id: Uuid
  preset_version_id: Uuid
  destination_list_id: Uuid
  assigned_to_identity_id: Uuid
  assigned_by_identity_id: Uuid
  payload_version: number
  state: string
  created_at: IsoDateTime
  materialized_at: IsoDateTime | null
}

export interface PresetAssignmentResponse {
  assignment: PresetAssignmentDto
}

export interface MaterializationChoiceDto {
  pretask_id: Uuid
  task_kind: TaskKindDto
  task_id: Uuid
  task_resource_node_id: Uuid
  assignment_id: Uuid
  permission_grant_id: Uuid
  encrypted_assignment: EncryptedPayloadDto
  selected_value_snapshot: EncryptedPayloadDto
  task_snapshot: EncryptedPayloadDto
  header: EncryptedPayloadDto
  recurrence_series_id: Uuid | null
  occurrence_number: number | null
  epoch: ResourceEpochInputDto
  envelopes: ResourceKeyEnvelopeDto[]
}

export interface MaterializePresetResponse {
  tasks: TaskDto[]
}

export interface RecurrenceSeriesDto {
  id: Uuid
  project_id: Uuid
  list_id: Uuid
  encrypted_rule: EncryptedPayloadDto
  state: { state: 'active' } | { state: 'archived'; archived_at: IsoDateTime }
  payload_version: number
  created_at: IsoDateTime
}

export interface QuestionnaireDto {
  id: Uuid
  project_id: Uuid
  payload: EncryptedPayloadDto
  latest_version: number
  created_at: IsoDateTime
  archived_at: IsoDateTime | null
}

export interface QuestionnaireResponse {
  questionnaire: QuestionnaireDto
}

export interface ListQuestionnairesResponse {
  questionnaires: QuestionnaireDto[]
  next_cursor: string | null
}

export type QuestionKindDto =
  | 'open'
  | 'single_choice'
  | 'multiple_choice'
  | 'boolean'

export interface QuestionnaireOptionDto {
  id: Uuid
  ordinal: number
  payload: EncryptedPayloadDto
}

export interface QuestionnaireQuestionDto {
  id: Uuid
  question_kind: QuestionKindDto
  ordinal: number
  required: boolean
  payload: EncryptedPayloadDto
  options: QuestionnaireOptionDto[]
}

export interface QuestionnaireVersionDto {
  id: Uuid
  questionnaire_id: Uuid
  project_id: Uuid
  number: number
  source_version_id: Uuid | null
  schema: EncryptedPayloadDto
  questions: QuestionnaireQuestionDto[]
  revision: number
  state: 'draft' | 'published'
  created_at: IsoDateTime
  published_at: IsoDateTime | null
}

export interface QuestionnaireVersionResponse {
  version: QuestionnaireVersionDto
}

export interface ListQuestionnaireVersionsResponse {
  versions: QuestionnaireVersionDto[]
}

export interface CreateQuestionnaireVersionRequest {
  id: Uuid
  source_version_id: Uuid | null
  schema: EncryptedPayloadDto
  content_hash_b64: string
  questions: QuestionnaireQuestionDto[]
  idempotency_key: Uuid
}

export interface UpdateQuestionnaireDraftRequest {
  expected_revision: number
  schema: EncryptedPayloadDto
  content_hash_b64: string
  questions: QuestionnaireQuestionDto[]
  idempotency_key: Uuid
}

export interface PublishQuestionnaireVersionRequest {
  expected_revision: number
  idempotency_key: Uuid
}

export interface QuestionnaireAnswerDto {
  id: Uuid
  question_id: Uuid
  selected_option_ids: Uuid[]
  payload: EncryptedPayloadDto
}

export interface QuestionnaireSubmissionDto {
  id: Uuid
  task_id: Uuid
  assignment_id: Uuid
  questionnaire_version_id: Uuid
  submitted_by_identity_id: Uuid
  encrypted_payload: EncryptedPayloadDto
  answers: QuestionnaireAnswerDto[]
  state: 'draft' | 'submitted'
  revision: number
  signer_device_id: Uuid | null
  signer_device_key_version: number | null
  created_at: IsoDateTime
  updated_at: IsoDateTime
  submitted_at: IsoDateTime | null
}

export interface UpsertQuestionnaireDraftRequest {
  submission_id: Uuid
  assignment_id: Uuid
  questionnaire_version_id: Uuid
  expected_revision: number | null
  encrypted_payload: EncryptedPayloadDto
  answers: QuestionnaireAnswerDto[]
  idempotency_key: Uuid
}

export interface FinalizeQuestionnaireSubmissionRequest {
  expected_revision: number
  signer_device_key_version: number
  classical_signature_b64: string
  post_quantum_signature_b64: string
  idempotency_key: Uuid
}

export interface QuestionnaireSubmissionResponse {
  submission: QuestionnaireSubmissionDto
  replayed: boolean
}

export type AttachmentStateDto =
  | { state: 'pending_upload' }
  | { state: 'available'; uploaded_at: IsoDateTime }
  | {
      state: 'pending_deletion'
      deleted_at: IsoDateTime
      purge_at: IsoDateTime
    }

export interface AttachmentDto {
  id: Uuid
  project_id: Uuid
  resource_node_id: Uuid
  attachment_kind: 'pretask_template' | 'task_required' | 'task_completed'
  blob_id: Uuid
  task_id: Uuid | null
  pretask_id: Uuid | null
  source_attachment_id: Uuid | null
  assignment_id: Uuid | null
  uploaded_by_identity_id: Uuid
  ciphertext_size: number
  ciphertext_sha256: string
  key_epoch: number
  encrypted_metadata: EncryptedPayloadDto
  state: AttachmentStateDto
  created_at: IsoDateTime
}

export interface AttachmentCollectionItemDto {
  id: Uuid
  project_id: Uuid
  resource_node_id: Uuid
  key_epoch: number
  attachment_kind: 'pretask_template' | 'task_required' | 'task_completed'
  blob_id: Uuid
  task_id: Uuid | null
  pretask_id: Uuid | null
  source_attachment_id: Uuid | null
  assignment_id: Uuid | null
  encrypted_metadata: EncryptedPayloadDto | null
  state: AttachmentDto['state']
  created_at: IsoDateTime
}

export interface ListAttachmentsResponse {
  attachments: AttachmentCollectionItemDto[]
  next_cursor: string | null
}

export interface EncryptedBlobDeclarationDto {
  blob_id: Uuid
  resource_node_id: Uuid
  ciphertext_size: number
  ciphertext_sha256: string
  key_epoch: number
  encrypted_blob_metadata: EncryptedPayloadDto
  encrypted_attachment_metadata: EncryptedPayloadDto
}

export interface CreatePretaskTemplateAttachmentRequest {
  id: Uuid
  blob: EncryptedBlobDeclarationDto
  idempotency_key: Uuid
}

export interface CreateTaskRequiredAttachmentRequest {
  id: Uuid
  source_template_attachment_id: Uuid | null
  blob: EncryptedBlobDeclarationDto
  idempotency_key: Uuid
}

export interface CreateTaskCompletedAttachmentRequest {
  id: Uuid
  assignment_id: Uuid
  required_attachment_id: Uuid | null
  blob: EncryptedBlobDeclarationDto
  idempotency_key: Uuid
}

export interface CreateAttachmentResponse {
  attachment: AttachmentDto
  upload_url: string
}

export interface RetentionPreferenceResponse {
  preference: {
    auto_export_enabled: boolean
    updated_at: IsoDateTime
  }
}

export interface RetentionArchiveDto {
  id: Uuid
  project_id: Uuid
  source_kind: string
  source_id: Uuid
  state: 'pending' | 'running' | 'succeeded' | 'failed'
  created_at: IsoDateTime
  completed_at: IsoDateTime | null
  source_purged_at: IsoDateTime | null
  expires_at: IsoDateTime | null
  downloaded_at: IsoDateTime | null
}

export interface ListRetentionArchivesResponse {
  archives: RetentionArchiveDto[]
}

export interface RetentionWarningDto {
  id: Uuid
  project_id: Uuid
  state: string
  scheduled_at: IsoDateTime
  created_at: IsoDateTime
}

export interface ListRetentionWarningsResponse {
  warnings: RetentionWarningDto[]
}

export interface ProjectRecoveryShareInputDto {
  share_id: Uuid
  holder_identity_id: Uuid
  holder_device_id: Uuid
  holder_device_key_version: number
  share_index: number
  encrypted_share_b64: string
  share_commitment_b64: string
}

export interface ProvisionProjectRecoveryRequest {
  recovery_set_id: Uuid
  recovery_epoch: number
  membership_epoch: number
  secret_commitment_b64: string
  context_hash_b64: string
  encrypted_owner_key_escrow_b64: string
  shares: ProjectRecoveryShareInputDto[]
}

export interface ProjectRecoveryProvisionStatus {
  recovery_set_id: Uuid | null
  recovery_epoch: number
  membership_epoch: number
  share_count: number
  state: string
  secret_commitment_b64: string | null
  holder_identity_ids: Uuid[]
  provisioned: boolean
  recoverable: boolean
  warning: string | null
}

export interface ProjectRecoveryShareView {
  share_id: Uuid
  recovery_set_id: Uuid
  recovery_epoch: number
  membership_epoch: number
  share_index: number
  encrypted_share_b64: string
  share_commitment_b64: string
  holder_device_id: Uuid
  holder_device_key_version: number
  context_hash_b64: string
  secret_commitment_b64: string
}

export interface RecoveryApprovalDelivery {
  approver_identity_id: Uuid
  encrypted_share_b64: string
  classical_signature_b64: string
  post_quantum_signature_b64: string
}

export interface ProjectRecoveryStatus {
  request_id: Uuid
  project_id: Uuid
  requester_identity_id: Uuid
  request_kind: 'participant_device' | 'lost_owner'
  status: string
  membership_epoch: number
  recovery_epoch: number
  recovery_set_id: Uuid
  challenge_b64: string
  context_hash_b64: string
  approval_signature_context_b64: string
  canonical_approval_prefix_b64: string | null
  required_approver_ids: Uuid[]
  approved_approver_ids: Uuid[]
  expires_at: IsoDateTime
  delivery_available: boolean
  deliveries?: RecoveryApprovalDelivery[]
  encrypted_owner_key_escrow_b64?: string | null
}

export type SyncMutation = 'upsert' | 'tombstone'

export interface SyncEventDto {
  id: Uuid
  event_sequence: number
  project_id: Uuid
  resource_node_id: Uuid
  base_version: number
  aggregate_version: number
  mutation: SyncMutation
  actor_identity_id: Uuid
  actor_device_id: Uuid
  actor_device_key_version: number
  device_sequence: number
  client_event_id: Uuid
  event_kind: string
  key_epoch: number
  encrypted_payload_b64: string
  previous_hash_b64: string | null
  event_hash_b64: string
  classical_signature_b64: string
  post_quantum_signature_b64: string | null
  client_created_at: IsoDateTime
  received_at: IsoDateTime
}

export interface PullSyncResponse {
  project_id: Uuid
  from_sequence: number
  next_sequence: number
  has_more: boolean
  events: SyncEventDto[]
}

export interface PushSyncRequest {
  project_id: Uuid
  resource_node_id: Uuid
  base_version: number
  aggregate_version: number
  actor_device_key_version: number
  device_sequence: number
  client_event_id: Uuid
  event_kind: string
  mutation: SyncMutation
  key_epoch: number
  encrypted_payload_b64: string
  previous_hash_b64: string | null
  event_hash_b64: string
  classical_signature_b64: string
  post_quantum_signature_b64: string
  client_created_at: IsoDateTime
  idempotency_key: Uuid
}

export interface PushSyncResponse {
  event: SyncEventDto
  projection: SyncProjectionDto
  replayed: boolean
}

export interface SyncProjectionDto {
  project_id: Uuid
  resource_node_id: Uuid
  aggregate_version: number
  mutation: 'upsert' | 'tombstone'
  key_epoch: number
  encrypted_payload_b64: string
  event_id: Uuid
  event_hash_b64: string
  updated_at: IsoDateTime
}

export interface SyncWakeNotification {
  project_id: Uuid
  cursor: number
}
