//! Wire contracts shared with TypeScript clients.
//!
//! User-authored content is represented only by [`EncryptedPayloadDto`].

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct EncryptedPayloadDto {
    pub version: u16,
    pub algorithm: String,
    pub key_id: String,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

impl fmt::Debug for EncryptedPayloadDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedPayloadDto")
            .field("version", &self.version)
            .field("algorithm", &self.algorithm)
            .field("key_id", &"[REDACTED]")
            .field("nonce_b64", &"[REDACTED]")
            .field("ciphertext_b64", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct IdempotencyKeyDto(pub String);

impl fmt::Debug for IdempotencyKeyDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IdempotencyKeyDto([REDACTED])")
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct OpaqueDigestDto(pub String);

impl fmt::Debug for OpaqueDigestDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaqueDigestDto([REDACTED])")
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct SensitiveUrlDto(pub String);

impl fmt::Debug for SensitiveUrlDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveUrlDto([REDACTED])")
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CollectionPageQuery {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UserDto {
    pub id: Uuid,
    pub profile: EncryptedPayloadDto,
    pub state: UserStateDto,
    pub created_at: DateTime<Utc>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UserStateDto {
    Active,
    PendingDeletion {
        requested_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpdateUserProfileRequest {
    pub profile: EncryptedPayloadDto,
    pub expected_revision: u64,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UserResponse {
    pub user: UserDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProjectDto {
    pub id: Uuid,
    pub root_resource_id: Uuid,
    pub owner_id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub state: ProjectStateDto,
    pub created_at: DateTime<Utc>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProjectStateDto {
    Active,
    PendingDeletion {
        requested_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateProjectRequest {
    pub id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateProjectResponse {
    pub project: ProjectDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ScheduleDeletionRequest {
    pub expected_revision: u64,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKindDto {
    Project,
    Topic,
    TaskList,
    Task,
    Preset,
    RecurrenceSeries,
    Questionnaire,
    QuestionnaireVersion,
    Attachment,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: ResourceKindDto,
    pub parent_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccessScopeDto {
    Full,
    ContainerOnly,
}

pub type PermissionLevelDto = AccessScopeDto;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKeyPurposeDto {
    #[default]
    Body,
    Header,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAccessLevelDto {
    View,
    Comment,
    Edit,
    Manage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GrantOriginDto {
    Direct,
    Assignment {
        assignment_id: Uuid,
    },
    Inherited {
        root_grant_id: Uuid,
        root_resource_id: Uuid,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PermissionGrantDto {
    pub id: Uuid,
    pub root_grant_id: Uuid,
    pub user_id: Uuid,
    pub resource_id: Uuid,
    pub access_level: PermissionAccessLevelDto,
    pub access_scope: AccessScopeDto,
    pub origin: GrantOriginDto,
    pub granted_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceKeyEnvelopeDto {
    pub version: u16,
    pub resource_id: Uuid,
    pub epoch: u32,
    #[serde(default)]
    pub key_purpose: ResourceKeyPurposeDto,
    pub recipient_identity_id: Uuid,
    pub recipient_device_id: Uuid,
    pub recipient_device_key_version: u32,
    pub sender_device_key_version: u32,
    pub encrypted_key_b64: String,
    pub sender_signature_b64: String,
    pub sender_post_quantum_signature_b64: String,
}

impl fmt::Debug for ResourceKeyEnvelopeDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceKeyEnvelopeDto")
            .field("version", &self.version)
            .field("resource_id", &self.resource_id)
            .field("epoch", &self.epoch)
            .field("recipient_identity_id", &self.recipient_identity_id)
            .field("recipient_device_id", &self.recipient_device_id)
            .field(
                "recipient_device_key_version",
                &self.recipient_device_key_version,
            )
            .field("sender_device_key_version", &self.sender_device_key_version)
            .field("encrypted_key_b64", &"[REDACTED]")
            .field("sender_signature_b64", &"[REDACTED]")
            .field("sender_post_quantum_signature_b64", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceKeyEnvelopeViewDto {
    #[serde(flatten)]
    pub envelope: ResourceKeyEnvelopeDto,
    pub sender_identity_id: Uuid,
    pub sender_device_id: Uuid,
    pub previous_epoch_hash_b64: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListResourceKeyEnvelopesResponse {
    pub envelopes: Vec<ResourceKeyEnvelopeViewDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceEnvelopePlanItemDto {
    pub resource_id: Uuid,
    pub epoch_id: Uuid,
    pub epoch: u32,
    pub key_commitment_b64: String,
    pub previous_epoch_hash_b64: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceEnvelopePlanResponse {
    pub resources: Vec<ResourceEnvelopePlanItemDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceRotationPlanItemDto {
    pub resource_id: Uuid,
    pub previous_epoch_id: Uuid,
    pub current_epoch: u32,
    pub previous_key_commitment_b64: String,
    pub previous_header_key_commitment_b64: Option<String>,
    /// All identities retaining at least container-header access.
    pub recipient_identity_ids: Vec<Uuid>,
    /// Identities retaining full body access after this revocation.
    pub body_recipient_identity_ids: Vec<Uuid>,
    /// Identities requiring a separated header key.
    pub header_recipient_identity_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceRotationPlanResponse {
    pub revoked_identity_id: Uuid,
    pub resources: Vec<ResourceRotationPlanItemDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ShareMemberResourceKeysRequest {
    pub recipient_identity_id: Uuid,
    pub resource_ids: Vec<Uuid>,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceEpochRotationDto {
    pub epoch_id: Uuid,
    pub resource_id: Uuid,
    pub previous_epoch_id: Uuid,
    pub new_epoch: u32,
    pub creator_device_key_version: u32,
    pub key_commitment_b64: String,
    #[serde(default)]
    pub header_key_commitment_b64: Option<String>,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct GrantPermissionRequest {
    pub grant_id: Uuid,
    pub user_id: Uuid,
    pub resource_id: Uuid,
    pub access_level: PermissionAccessLevelDto,
    pub access_scope: AccessScopeDto,
    pub visibility: String,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct GrantPermissionResponse {
    pub grant: PermissionGrantDto,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RevokePermissionRequest {
    pub user_id: Uuid,
    pub rotations: Vec<ResourceEpochRotationDto>,
    pub encrypted_admin_notification_b64: Option<String>,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListPermissionsResponse {
    pub grants: Vec<PermissionGrantDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct AssignmentDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub task_id: Uuid,
    pub assignee_identity_id: Uuid,
    pub assigned_by_identity_id: Uuid,
    pub permission_root_grant_id: Uuid,
    pub assigned_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct AssignTaskRequest {
    pub assignment_id: Uuid,
    pub permission_grant_id: Uuid,
    pub assignee_identity_id: Uuid,
    pub encrypted_payload_b64: String,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct AssignmentResponse {
    pub assignment: AssignmentDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListAssignmentsResponse {
    pub assignments: Vec<AssignmentDto>,
    pub active_assignment_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RevokeAssignmentRequest {
    pub rotations: Vec<ResourceEpochRotationDto>,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskKindDto {
    Priority,
    Deadline,
    Recurring,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct TaskListDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub topic_id: Uuid,
    pub resource_node_id: Uuid,
    pub payload: Option<EncryptedPayloadDto>,
    pub header: Option<EncryptedPayloadDto>,
    pub key_epoch: u32,
    pub payload_version: u64,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct TopicDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub resource_node_id: Uuid,
    pub payload: Option<EncryptedPayloadDto>,
    pub header: Option<EncryptedPayloadDto>,
    pub key_epoch: u32,
    pub payload_version: u64,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskStateDto {
    Open,
    Completed {
        completed_by: Uuid,
        completed_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct TaskDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub list_id: Uuid,
    pub resource_node_id: Uuid,
    pub task_kind: TaskKindDto,
    pub payload: Option<EncryptedPayloadDto>,
    pub header: Option<EncryptedPayloadDto>,
    pub selected_value_snapshot: Option<EncryptedPayloadDto>,
    pub key_epoch: u32,
    pub state: TaskStateDto,
    pub source_pretask_id: Option<Uuid>,
    pub preset_assignment_id: Option<Uuid>,
    pub copied_from_task_id: Option<Uuid>,
    pub questionnaire_version_id: Option<Uuid>,
    pub recurrence_series_id: Option<Uuid>,
    pub occurrence_number: Option<u64>,
    pub active_assignment_id: Option<Uuid>,
    pub active_assignee_identity_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub payload_version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateTopicRequest {
    pub id: Uuid,
    pub resource_node_id: Uuid,
    pub parent_resource_node_id: Uuid,
    pub payload: EncryptedPayloadDto,
    #[serde(default)]
    pub header: Option<EncryptedPayloadDto>,
    pub epoch: ResourceEpochInputDto,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct TopicResponse {
    pub topic: TopicDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpdateEncryptedResourceRequest {
    pub expected_payload_version: u64,
    pub key_epoch: u32,
    pub payload: EncryptedPayloadDto,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListTopicsResponse {
    /// No ordering contract is implied by this collection.
    pub topics: Vec<TopicDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateTaskListRequest {
    pub id: Uuid,
    pub topic_id: Uuid,
    pub resource_node_id: Uuid,
    pub payload: EncryptedPayloadDto,
    #[serde(default)]
    pub header: Option<EncryptedPayloadDto>,
    pub epoch: ResourceEpochInputDto,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct TaskListResponse {
    pub task_list: TaskListDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListTaskListsResponse {
    /// No ordering contract is implied by this collection.
    pub task_lists: Vec<TaskListDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct InfoDocumentDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub topic_id: Option<Uuid>,
    pub task_list_id: Option<Uuid>,
    pub parent_document_id: Option<Uuid>,
    pub resource_node_id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub key_epoch: u32,
    pub payload_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateInfoDocumentRequest {
    pub id: Uuid,
    pub parent_document_id: Option<Uuid>,
    pub resource_node_id: Uuid,
    pub key_epoch: u32,
    pub payload: EncryptedPayloadDto,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpdateInfoDocumentRequest {
    pub expected_payload_version: u64,
    pub key_epoch: u32,
    pub payload: EncryptedPayloadDto,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct InfoDocumentResponse {
    pub document: InfoDocumentDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListInfoDocumentsResponse {
    /// Parent/child ordering is carried only inside encrypted document payloads.
    pub documents: Vec<InfoDocumentDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateTaskRequest {
    pub id: Uuid,
    pub list_id: Uuid,
    pub resource_node_id: Uuid,
    pub task_kind: TaskKindDto,
    pub payload: EncryptedPayloadDto,
    #[serde(default)]
    pub header: Option<EncryptedPayloadDto>,
    pub selected_value_snapshot: EncryptedPayloadDto,
    pub questionnaire_version_id: Option<Uuid>,
    pub recurrence_series_id: Option<Uuid>,
    pub occurrence_number: Option<u64>,
    pub epoch: ResourceEpochInputDto,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpdateTaskRequest {
    pub expected_payload_version: u64,
    pub key_epoch: u32,
    #[serde(default)]
    pub update_task_metadata: bool,
    #[serde(default)]
    pub task_kind: Option<TaskKindDto>,
    pub questionnaire_version_id: Option<Uuid>,
    pub recurrence_series_id: Option<Uuid>,
    pub occurrence_number: Option<u64>,
    pub payload: EncryptedPayloadDto,
    pub selected_value_snapshot: EncryptedPayloadDto,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct NextRecurringTaskDto {
    pub id: Uuid,
    pub resource_node_id: Uuid,
    pub assignment_id: Uuid,
    pub permission_grant_id: Uuid,
    pub encrypted_assignment: EncryptedPayloadDto,
    pub recurrence_series_id: Uuid,
    pub occurrence_number: u64,
    pub payload: EncryptedPayloadDto,
    pub header: EncryptedPayloadDto,
    pub selected_value_snapshot: EncryptedPayloadDto,
    pub epoch: ResourceEpochInputDto,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ResourceEpochInputDto {
    pub id: Uuid,
    pub epoch: u32,
    pub creator_device_key_version: u32,
    pub key_commitment_b64: String,
    #[serde(default)]
    pub header_key_commitment_b64: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct InitializeResourceEpochRequest {
    pub epoch: ResourceEpochInputDto,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
}

impl fmt::Debug for ResourceEpochInputDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceEpochInputDto")
            .field("id", &self.id)
            .field("epoch", &self.epoch)
            .field(
                "creator_device_key_version",
                &self.creator_device_key_version,
            )
            .field("key_commitment_b64", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CompleteTaskRequest {
    pub completion_id: Uuid,
    pub assignment_id: Uuid,
    pub expected_payload_version: u64,
    pub encrypted_completion: EncryptedPayloadDto,
    pub completed_at: DateTime<Utc>,
    pub recurrence_series_id: Option<Uuid>,
    pub occurrence_number: Option<u64>,
    pub next_occurrence: Option<NextRecurringTaskDto>,
    pub idempotency_key: Uuid,
}

pub type CompleteAssignedTaskRequest = CompleteTaskRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CompleteTaskResponse {
    pub completed_task: TaskDto,
    pub next_task: Option<TaskDto>,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CopyCompletedTaskRequest {
    pub destination_list_id: Uuid,
    pub new_task_id: Uuid,
    pub new_resource_node_id: Uuid,
    pub assignment_id: Uuid,
    pub permission_grant_id: Uuid,
    pub encrypted_assignment: EncryptedPayloadDto,
    pub payload: EncryptedPayloadDto,
    pub header: EncryptedPayloadDto,
    pub selected_value_snapshot: EncryptedPayloadDto,
    pub epoch: ResourceEpochInputDto,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
    pub recurrence_series_id: Option<Uuid>,
    pub occurrence_number: Option<u64>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct TaskResponse {
    pub task: TaskDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListTasksResponse {
    /// No ordering contract is implied by this collection.
    pub tasks: Vec<TaskDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PretaskDto {
    pub id: Uuid,
    pub task_kind: TaskKindDto,
    pub payload: EncryptedPayloadDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PresetDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreatePresetRequest {
    pub id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpdatePresetRequest {
    pub payload: EncryptedPayloadDto,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PresetResponse {
    pub preset: PresetDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListPresetsResponse {
    pub presets: Vec<PresetDto>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PresetVersionDto {
    pub id: Uuid,
    pub preset_id: Uuid,
    pub project_id: Uuid,
    pub version_number: u32,
    pub payload: EncryptedPayloadDto,
    pub pretasks: Vec<PretaskDto>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreatePresetVersionRequest {
    pub id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub content_hash_b64: String,
    pub pretasks: Vec<PretaskDto>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PresetVersionResponse {
    pub version: PresetVersionDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PretaskSelectionDto {
    pub pretask_id: Uuid,
    pub task_kind: TaskKindDto,
    pub selected_value: EncryptedPayloadDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreatePresetAssignmentRequest {
    pub id: Uuid,
    pub preset_version_id: Uuid,
    pub destination_list_id: Uuid,
    pub assigned_to_identity_id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub selections: Vec<PretaskSelectionDto>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PresetAssignmentDto {
    pub id: Uuid,
    pub preset_version_id: Uuid,
    pub destination_list_id: Uuid,
    pub assigned_to_identity_id: Uuid,
    pub assigned_by_identity_id: Uuid,
    pub payload_version: u64,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub materialized_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PresetAssignmentResponse {
    pub assignment: PresetAssignmentDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct MaterializationChoiceDto {
    pub pretask_id: Uuid,
    pub task_kind: TaskKindDto,
    pub task_id: Uuid,
    pub task_resource_node_id: Uuid,
    pub assignment_id: Uuid,
    pub permission_grant_id: Uuid,
    pub encrypted_assignment: EncryptedPayloadDto,
    pub selected_value_snapshot: EncryptedPayloadDto,
    pub task_snapshot: EncryptedPayloadDto,
    pub header: EncryptedPayloadDto,
    pub recurrence_series_id: Option<Uuid>,
    pub occurrence_number: Option<u64>,
    pub epoch: ResourceEpochInputDto,
    pub envelopes: Vec<ResourceKeyEnvelopeDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct MaterializePresetRequest {
    pub expected_assignment_version: u64,
    pub choices: Vec<MaterializationChoiceDto>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct MaterializePresetResponse {
    pub tasks: Vec<TaskDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RecurrenceStateDto {
    Active,
    Archived { archived_at: DateTime<Utc> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RecurrenceSeriesDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub list_id: Uuid,
    pub encrypted_rule: EncryptedPayloadDto,
    pub state: RecurrenceStateDto,
    pub payload_version: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateRecurrenceSeriesRequest {
    pub id: Uuid,
    pub list_id: Uuid,
    pub encrypted_rule: EncryptedPayloadDto,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RecurrenceSeriesResponse {
    pub series: RecurrenceSeriesDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ArchiveRecurrenceSeriesRequest {
    pub expected_payload_version: u64,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub latest_version: u32,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateQuestionnaireRequest {
    pub id: Uuid,
    pub payload: EncryptedPayloadDto,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireResponse {
    pub questionnaire: QuestionnaireDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListQuestionnairesResponse {
    pub questionnaires: Vec<QuestionnaireDto>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKindDto {
    Open,
    SingleChoice,
    MultipleChoice,
    Boolean,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireOptionDto {
    pub id: Uuid,
    pub ordinal: u32,
    pub payload: EncryptedPayloadDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireQuestionDto {
    pub id: Uuid,
    pub question_kind: QuestionKindDto,
    pub ordinal: u32,
    pub required: bool,
    pub payload: EncryptedPayloadDto,
    pub options: Vec<QuestionnaireOptionDto>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionnaireVersionStateDto {
    Draft,
    Published,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireVersionDto {
    pub id: Uuid,
    pub questionnaire_id: Uuid,
    pub project_id: Uuid,
    pub number: u32,
    pub source_version_id: Option<Uuid>,
    pub schema: EncryptedPayloadDto,
    pub questions: Vec<QuestionnaireQuestionDto>,
    pub revision: u64,
    pub state: QuestionnaireVersionStateDto,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateQuestionnaireVersionRequest {
    pub id: Uuid,
    pub source_version_id: Option<Uuid>,
    pub schema: EncryptedPayloadDto,
    pub content_hash_b64: String,
    pub questions: Vec<QuestionnaireQuestionDto>,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpdateQuestionnaireDraftRequest {
    pub expected_revision: u64,
    pub schema: EncryptedPayloadDto,
    pub content_hash_b64: String,
    pub questions: Vec<QuestionnaireQuestionDto>,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PublishQuestionnaireVersionRequest {
    pub expected_revision: u64,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireVersionResponse {
    pub version: QuestionnaireVersionDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListQuestionnaireVersionsResponse {
    pub versions: Vec<QuestionnaireVersionDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireAnswerDto {
    pub id: Uuid,
    pub question_id: Uuid,
    pub selected_option_ids: Vec<Uuid>,
    pub payload: EncryptedPayloadDto,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionnaireSubmissionStateDto {
    Draft,
    Submitted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireSubmissionDto {
    pub id: Uuid,
    pub task_id: Uuid,
    pub assignment_id: Uuid,
    pub questionnaire_version_id: Uuid,
    pub submitted_by_identity_id: Uuid,
    pub encrypted_payload: EncryptedPayloadDto,
    pub answers: Vec<QuestionnaireAnswerDto>,
    pub state: QuestionnaireSubmissionStateDto,
    pub revision: u64,
    pub signer_device_id: Option<Uuid>,
    pub signer_device_key_version: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub submitted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpsertQuestionnaireDraftRequest {
    pub submission_id: Uuid,
    pub assignment_id: Uuid,
    pub questionnaire_version_id: Uuid,
    pub expected_revision: Option<u64>,
    pub encrypted_payload: EncryptedPayloadDto,
    pub answers: Vec<QuestionnaireAnswerDto>,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct FinalizeQuestionnaireSubmissionRequest {
    pub expected_revision: u64,
    pub signer_device_key_version: u32,
    pub classical_signature_b64: String,
    pub post_quantum_signature_b64: String,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QuestionnaireSubmissionResponse {
    pub submission: QuestionnaireSubmissionDto,
    pub replayed: bool,
}

pub type PublishQuestionnaireVersionResponse = QuestionnaireVersionResponse;
pub type SubmitQuestionnaireRequest = FinalizeQuestionnaireSubmissionRequest;
pub type SubmitQuestionnaireResponse = QuestionnaireSubmissionResponse;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AttachmentStateDto {
    PendingUpload,
    Available {
        uploaded_at: DateTime<Utc>,
    },
    PendingDeletion {
        deleted_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKindDto {
    PretaskTemplate,
    TaskRequired,
    TaskCompleted,
    InfoDocument,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct AttachmentDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub resource_node_id: Uuid,
    pub attachment_kind: AttachmentKindDto,
    pub blob_id: Uuid,
    pub task_id: Option<Uuid>,
    pub pretask_id: Option<Uuid>,
    pub source_attachment_id: Option<Uuid>,
    pub assignment_id: Option<Uuid>,
    pub uploaded_by_identity_id: Uuid,
    pub ciphertext_size: u64,
    pub ciphertext_sha256: OpaqueDigestDto,
    pub key_epoch: u32,
    pub encrypted_metadata: EncryptedPayloadDto,
    pub state: AttachmentStateDto,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct AttachmentCollectionItemDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub resource_node_id: Uuid,
    pub key_epoch: u32,
    pub attachment_kind: AttachmentKindDto,
    pub blob_id: Uuid,
    pub task_id: Option<Uuid>,
    pub pretask_id: Option<Uuid>,
    pub source_attachment_id: Option<Uuid>,
    pub assignment_id: Option<Uuid>,
    /// Present only when the caller has full body access. A `container_only`
    /// grant can expose the collection header, but never this ciphertext.
    pub encrypted_metadata: Option<EncryptedPayloadDto>,
    pub state: AttachmentStateDto,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListAttachmentsResponse {
    pub attachments: Vec<AttachmentCollectionItemDto>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct EncryptedBlobDeclarationDto {
    pub blob_id: Uuid,
    pub resource_node_id: Uuid,
    pub ciphertext_size: u64,
    pub ciphertext_sha256: OpaqueDigestDto,
    pub key_epoch: u32,
    pub encrypted_blob_metadata: EncryptedPayloadDto,
    pub encrypted_attachment_metadata: EncryptedPayloadDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreatePretaskTemplateAttachmentRequest {
    pub id: Uuid,
    pub blob: EncryptedBlobDeclarationDto,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateTaskRequiredAttachmentRequest {
    pub id: Uuid,
    pub source_template_attachment_id: Option<Uuid>,
    pub blob: EncryptedBlobDeclarationDto,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateTaskCompletedAttachmentRequest {
    pub id: Uuid,
    pub assignment_id: Uuid,
    pub required_attachment_id: Option<Uuid>,
    pub blob: EncryptedBlobDeclarationDto,
    pub idempotency_key: IdempotencyKeyDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateInfoDocumentFileRequest {
    pub id: Uuid,
    pub blob: EncryptedBlobDeclarationDto,
    pub idempotency_key: IdempotencyKeyDto,
}

pub type CreateAttachmentRequest = CreateTaskCompletedAttachmentRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreateAttachmentResponse {
    pub attachment: AttachmentDto,
    pub upload_url: SensitiveUrlDto,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RegisterDeviceKeyPackageRequest {
    pub package_b64: String,
    pub previous_classical_signature_b64: Option<String>,
    pub previous_post_quantum_signature_b64: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct DeviceKeyPackageStatusDto {
    pub device_id: Uuid,
    pub key_version: i32,
    pub generation: i64,
    pub package_hash_b64: String,
    pub status: String,
    pub suite_status: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRecoveryKindDto {
    ParticipantDevice,
    LostOwner,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct StartProjectRecoveryRequest {
    pub request_id: Uuid,
    pub request_kind: ProjectRecoveryKindDto,
    pub challenge_b64: String,
    pub context_hash_b64: String,
    pub expires_in_seconds: u32,
    pub requester_device_key_version: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProjectRecoveryShareInputDto {
    pub share_id: Uuid,
    pub holder_identity_id: Uuid,
    pub holder_device_id: Uuid,
    pub holder_device_key_version: i32,
    pub share_index: u16,
    pub encrypted_share_b64: String,
    pub share_commitment_b64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProvisionProjectRecoveryRequest {
    pub recovery_set_id: Uuid,
    pub recovery_epoch: i64,
    pub membership_epoch: i64,
    pub secret_commitment_b64: String,
    pub context_hash_b64: String,
    pub encrypted_owner_key_escrow_b64: String,
    pub shares: Vec<ProjectRecoveryShareInputDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ActivateProjectRecoveryRequest {
    pub recovery_set_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProjectRecoveryProvisionStatusDto {
    pub recovery_set_id: Option<Uuid>,
    pub recovery_epoch: i64,
    pub membership_epoch: i64,
    pub share_count: u16,
    pub state: String,
    pub secret_commitment_b64: Option<String>,
    pub holder_identity_ids: Vec<Uuid>,
    pub provisioned: bool,
    pub recoverable: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProjectRecoveryShareViewDto {
    pub share_id: Uuid,
    pub recovery_set_id: Uuid,
    pub recovery_epoch: i64,
    pub membership_epoch: i64,
    pub share_index: u16,
    pub encrypted_share_b64: String,
    pub share_commitment_b64: String,
    pub holder_device_id: Uuid,
    pub holder_device_key_version: i32,
    pub context_hash_b64: String,
    pub secret_commitment_b64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListMyProjectRecoverySharesResponse {
    pub shares: Vec<ProjectRecoveryShareViewDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RecoveryApprovalDeliveryDto {
    pub approver_identity_id: Uuid,
    pub encrypted_share_b64: String,
    pub classical_signature_b64: String,
    pub post_quantum_signature_b64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProjectRecoveryStatusDto {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub requester_identity_id: Uuid,
    pub request_kind: ProjectRecoveryKindDto,
    pub status: String,
    pub membership_epoch: i64,
    pub recovery_epoch: i64,
    pub recovery_set_id: Uuid,
    pub challenge_b64: String,
    pub context_hash_b64: String,
    pub approval_signature_context_b64: String,
    pub canonical_approval_prefix_b64: Option<String>,
    pub required_approver_ids: Vec<Uuid>,
    pub approved_approver_ids: Vec<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub delivery_available: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deliveries: Vec<RecoveryApprovalDeliveryDto>,
    pub encrypted_owner_key_escrow_b64: Option<String>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProjectRecoveryApprovalRequest {
    pub approver_device_key_version: i32,
    pub encrypted_share_b64: String,
    pub classical_signature_b64: String,
    pub post_quantum_signature_b64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct FinalizeProjectRecoveryRequest {
    pub new_device_key_version: i32,
    pub rotations: Vec<ResourceEpochRotationDto>,
    pub replacement_recovery: ProvisionProjectRecoveryRequest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProjectRecoveryFinalizedDto {
    pub request_id: Uuid,
    pub status: String,
    pub owner_epoch: i64,
    pub device_epoch: i64,
    pub device_generation: i64,
    pub recovery_epoch: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RecoveryRotationPlanResponse {
    pub resources: Vec<ResourceRotationPlanItemDto>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SyncMutationDto {
    Upsert,
    Tombstone,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct SyncEventDto {
    pub id: Uuid,
    pub event_sequence: i64,
    pub project_id: Uuid,
    pub resource_node_id: Uuid,
    pub base_version: i64,
    pub aggregate_version: i64,
    pub mutation: SyncMutationDto,
    pub actor_identity_id: Uuid,
    pub actor_device_id: Uuid,
    pub actor_device_key_version: i32,
    pub device_sequence: i64,
    pub client_event_id: Uuid,
    pub event_kind: String,
    pub key_epoch: i32,
    pub encrypted_payload_b64: String,
    pub previous_hash_b64: Option<String>,
    pub event_hash_b64: String,
    pub classical_signature_b64: String,
    pub post_quantum_signature_b64: Option<String>,
    pub client_created_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PullSyncRequest {
    pub project_id: Uuid,
    pub after_sequence: i64,
    pub limit: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PullSyncResponse {
    pub project_id: Uuid,
    pub from_sequence: i64,
    pub next_sequence: i64,
    pub events: Vec<SyncEventDto>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PushSyncRequest {
    pub project_id: Uuid,
    pub resource_node_id: Uuid,
    pub base_version: i64,
    pub aggregate_version: i64,
    pub actor_device_key_version: i32,
    pub device_sequence: i64,
    pub client_event_id: Uuid,
    pub event_kind: String,
    pub mutation: SyncMutationDto,
    pub key_epoch: i32,
    pub encrypted_payload_b64: String,
    pub previous_hash_b64: Option<String>,
    pub event_hash_b64: String,
    pub classical_signature_b64: String,
    pub post_quantum_signature_b64: String,
    pub client_created_at: DateTime<Utc>,
    pub idempotency_key: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct PushSyncResponse {
    pub event: SyncEventDto,
    pub projection: SyncProjectionDto,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct SyncProjectionDto {
    pub project_id: Uuid,
    pub resource_node_id: Uuid,
    pub aggregate_version: i64,
    pub mutation: SyncMutationDto,
    pub key_epoch: i32,
    pub encrypted_payload_b64: String,
    pub event_id: Uuid,
    pub event_hash_b64: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct SyncWakeNotificationDto {
    pub project_id: Uuid,
    pub cursor: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClassDto {
    DeletedOrObsolete,
    Completed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RetentionDeadlineDto {
    pub class: RetentionClassDto,
    pub event_at: DateTime<Utc>,
    pub warning_at: DateTime<Utc>,
    pub purge_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RetentionPreferenceDto {
    pub auto_export_enabled: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpdateRetentionPreferenceRequest {
    pub auto_export_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RetentionPreferenceResponse {
    pub preference: RetentionPreferenceDto,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RetentionArchiveStateDto {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RetentionArchiveDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_kind: String,
    pub source_id: Uuid,
    pub state: RetentionArchiveStateDto,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub source_purged_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub downloaded_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListRetentionArchivesResponse {
    pub archives: Vec<RetentionArchiveDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RetentionWarningDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub state: String,
    pub scheduled_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ListRetentionWarningsResponse {
    pub warnings: Vec<RetentionWarningDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RecordArchiveReceiptRequest {
    pub ciphertext_sha256: OpaqueDigestDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ArchiveReceiptDto {
    pub archive_id: Uuid,
    pub received_at: DateTime<Utc>,
    pub ciphertext_sha256: OpaqueDigestDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ArchiveReceiptResponse {
    pub receipt: ArchiveReceiptDto,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encrypted() -> EncryptedPayloadDto {
        EncryptedPayloadDto {
            version: 1,
            algorithm: "xchacha20poly1305".into(),
            key_id: "secret-key-id".into(),
            nonce_b64: "secret-nonce".into(),
            ciphertext_b64: "secret-ciphertext".into(),
        }
    }

    #[test]
    fn debug_output_redacts_wire_secrets_recursively() {
        let request = CreateTaskRequest {
            id: Uuid::nil(),
            list_id: Uuid::nil(),
            resource_node_id: Uuid::nil(),
            task_kind: TaskKindDto::Priority,
            payload: encrypted(),
            header: None,
            selected_value_snapshot: encrypted(),
            questionnaire_version_id: None,
            recurrence_series_id: None,
            occurrence_number: None,
            epoch: ResourceEpochInputDto {
                id: Uuid::nil(),
                epoch: 1,
                creator_device_key_version: 1,
                key_commitment_b64: "secret-commitment".into(),
                header_key_commitment_b64: None,
            },
            envelopes: Vec::new(),
            idempotency_key: Uuid::nil(),
        };
        let debug = format!("{request:?}");
        for secret in ["secret-key-id", "secret-nonce", "secret-ciphertext"] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("REDACTED"));

        let signed_url = SensitiveUrlDto("https://upload.test/?signature=secret".into());
        assert!(!format!("{signed_url:?}").contains("signature=secret"));

        let envelope = ResourceKeyEnvelopeDto {
            version: 1,
            resource_id: Uuid::nil(),
            epoch: 1,
            key_purpose: ResourceKeyPurposeDto::Body,
            recipient_identity_id: Uuid::nil(),
            recipient_device_id: Uuid::nil(),
            recipient_device_key_version: 1,
            sender_device_key_version: 1,
            encrypted_key_b64: "secret-encrypted-key".into(),
            sender_signature_b64: "secret-signature".into(),
            sender_post_quantum_signature_b64: "secret-pq-signature".into(),
        };
        let envelope_debug = format!("{envelope:?}");
        assert!(!envelope_debug.contains("secret-encrypted-key"));
        assert!(!envelope_debug.contains("secret-signature"));
        assert!(!envelope_debug.contains("secret-pq-signature"));
    }

    #[test]
    fn representative_types_are_typescript_exportable() {
        let config = ts_rs::Config::default();
        assert!(ProjectDto::decl(&config).contains("root_resource_id"));
        assert!(TaskDto::decl(&config).contains("active_assignment_id"));
        assert!(ListAssignmentsResponse::decl(&config).contains("active_assignment_id"));
        assert!(CreateTaskRequest::decl(&config).contains("CreateTaskRequest"));
        assert!(MaterializePresetRequest::decl(&config).contains("MaterializePresetRequest"));
        assert!(PullSyncResponse::decl(&config).contains("PullSyncResponse"));
        assert!(GrantPermissionRequest::decl(&config).contains("GrantPermissionRequest"));
        assert!(AssignTaskRequest::decl(&config).contains("AssignTaskRequest"));
        assert!(
            UpsertQuestionnaireDraftRequest::decl(&config)
                .contains("UpsertQuestionnaireDraftRequest")
        );
        assert!(
            CreateTaskCompletedAttachmentRequest::decl(&config)
                .contains("CreateTaskCompletedAttachmentRequest")
        );
        assert!(ListPresetsResponse::decl(&config).contains("ListPresetsResponse"));
        assert!(ListQuestionnairesResponse::decl(&config).contains("ListQuestionnairesResponse"));
        assert!(ListAttachmentsResponse::decl(&config).contains("ListAttachmentsResponse"));
        assert!(
            ListRetentionArchivesResponse::decl(&config).contains("ListRetentionArchivesResponse")
        );
    }

    #[test]
    fn llr_05_5_attachment_wire_contract_never_serializes_local_paths() {
        let request = CreateTaskCompletedAttachmentRequest {
            id: Uuid::new_v4(),
            assignment_id: Uuid::new_v4(),
            required_attachment_id: None,
            blob: EncryptedBlobDeclarationDto {
                blob_id: Uuid::new_v4(),
                resource_node_id: Uuid::new_v4(),
                ciphertext_size: 32,
                ciphertext_sha256: OpaqueDigestDto("opaque-digest".into()),
                key_epoch: 1,
                encrypted_blob_metadata: encrypted(),
                encrypted_attachment_metadata: encrypted(),
            },
            idempotency_key: IdempotencyKeyDto(Uuid::new_v4().to_string()),
        };
        let declaration = CreateTaskCompletedAttachmentRequest::decl(&ts_rs::Config::default());
        let collection_declaration = AttachmentCollectionItemDto::decl(&ts_rs::Config::default());
        let debug = format!("{:?}", request.id);
        for forbidden in [
            "client_path",
            "file_path",
            "filename",
            "mime_type",
            "media_type",
        ] {
            assert!(!declaration.contains(forbidden));
            assert!(!collection_declaration.contains(forbidden));
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn container_only_attachment_header_has_no_body_ciphertext() {
        let item = AttachmentCollectionItemDto {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            resource_node_id: Uuid::new_v4(),
            key_epoch: 1,
            attachment_kind: AttachmentKindDto::TaskRequired,
            blob_id: Uuid::new_v4(),
            task_id: Some(Uuid::new_v4()),
            pretask_id: None,
            source_attachment_id: None,
            assignment_id: None,
            encrypted_metadata: None,
            state: AttachmentStateDto::PendingUpload,
            created_at: Utc::now(),
        };
        assert!(item.encrypted_metadata.is_none());
        let declaration = AttachmentCollectionItemDto::decl(&ts_rs::Config::default());
        assert!(declaration.contains("encrypted_metadata"));
        for forbidden in ["path", "filename", "mime"] {
            assert!(!declaration.contains(forbidden));
        }
    }
}
