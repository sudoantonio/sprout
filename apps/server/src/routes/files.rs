use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{
        HeaderValue, Response, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE,
            X_CONTENT_TYPE_OPTIONS,
        },
    },
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sprout_api_contract::{
    AttachmentCollectionItemDto, AttachmentDto, AttachmentKindDto, AttachmentStateDto,
    CreateAttachmentResponse, CreateInfoDocumentFileRequest,
    CreatePretaskTemplateAttachmentRequest, CreateTaskCompletedAttachmentRequest,
    CreateTaskRequiredAttachmentRequest, EncryptedBlobDeclarationDto, EncryptedPayloadDto,
    ListAttachmentsResponse, OpaqueDigestDto, SensitiveUrlDto,
};
use sqlx::{FromRow, Postgres, Transaction};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        AuthSession, ProjectAccess, ResourceAccess, completed_attachment_upload_allowed,
        require_project_access, require_resource_access, set_database_context,
    },
    error::AppError,
};

use super::pagination::{finish_page, parse_page};

const ORPHAN_GRACE: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy)]
enum AttachmentInsert {
    Template {
        preset_version_id: Uuid,
        pretask_id: Uuid,
    },
    Required {
        task_id: Uuid,
        source_template_attachment_id: Option<Uuid>,
    },
    Completed {
        task_id: Uuid,
        assignment_id: Uuid,
        required_attachment_id: Option<Uuid>,
    },
    InfoDocument,
}

pub async fn list_template(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, version_id, pretask_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<sprout_api_contract::CollectionPageQuery>,
) -> Result<Json<ListAttachmentsResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let page = parse_page(query)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let mut rows = sqlx::query_as::<_, AttachmentCollectionRow>(
        r#"
        SELECT
            attachment.id,
            attachment.project_id,
            attachment.resource_node_id,
            attachment.key_epoch,
            'pretask_template'::text AS attachment_kind,
            attachment.blob_id,
            NULL::uuid AS task_id,
            attachment.pretask_id,
            NULL::uuid AS source_attachment_id,
            NULL::uuid AS assignment_id,
            CASE
                WHEN project.owner_identity_id = $4
                  OR membership.role = 'admin'
                  OR resource.created_by_identity_id = $4
                  OR permission.access_scope = 'full'
                THEN attachment.encrypted_metadata
                ELSE NULL
            END AS encrypted_metadata,
            blob.upload_state,
            blob.available_at,
            attachment.created_at
        FROM pretask_template_attachments attachment
        JOIN file_blobs blob
          ON blob.project_id = attachment.project_id
         AND blob.id = attachment.blob_id
        JOIN resource_nodes resource
          ON resource.project_id = attachment.project_id
         AND resource.id = attachment.resource_node_id
         AND resource.deleted_at IS NULL
        JOIN projects project
          ON project.id = attachment.project_id
         AND project.deleted_at IS NULL
        JOIN project_memberships membership
          ON membership.project_id = attachment.project_id
         AND membership.identity_id = $4
         AND membership.state = 'active'
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            attachment.project_id, attachment.resource_node_id, $4
        ) permission ON true
        WHERE attachment.project_id = $1
          AND attachment.preset_version_id = $2
          AND attachment.pretask_id = $3
          AND blob.upload_state <> 'deleted'
          AND (
              project.owner_identity_id = $4
              OR membership.role = 'admin'
              OR resource.created_by_identity_id = $4
              OR permission.access_level IS NOT NULL
          )
          AND (
              $5::timestamptz IS NULL
              OR (attachment.created_at, attachment.id)
                   > ($5::timestamptz, $6::uuid)
          )
        ORDER BY attachment.created_at, attachment.id
        LIMIT $7
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(pretask_id)
    .bind(actor.identity_id)
    .bind(page.after_created_at)
    .bind(page.after_id)
    .bind(page.sql_limit()?)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    attachment_page(&mut rows, page)
}

pub async fn list_required(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<sprout_api_contract::CollectionPageQuery>,
) -> Result<Json<ListAttachmentsResponse>, AppError> {
    let task_resource_id = task_resource(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        task_resource_id,
        ResourceAccess::ViewHeader,
    )
    .await?;
    let page = parse_page(query)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let mut rows = sqlx::query_as::<_, AttachmentCollectionRow>(
        r#"
        SELECT
            attachment.id,
            attachment.project_id,
            attachment.resource_node_id,
            attachment.key_epoch,
            'task_required'::text AS attachment_kind,
            attachment.blob_id,
            attachment.task_id,
            NULL::uuid AS pretask_id,
            attachment.source_template_attachment_id AS source_attachment_id,
            NULL::uuid AS assignment_id,
            CASE
                WHEN project.owner_identity_id = $3
                  OR membership.role = 'admin'
                  OR resource.created_by_identity_id = $3
                  OR permission.access_scope = 'full'
                THEN attachment.encrypted_snapshot
                ELSE NULL
            END AS encrypted_metadata,
            blob.upload_state,
            blob.available_at,
            attachment.created_at
        FROM task_required_attachments attachment
        JOIN file_blobs blob
          ON blob.project_id = attachment.project_id
         AND blob.id = attachment.blob_id
        JOIN resource_nodes resource
          ON resource.project_id = attachment.project_id
         AND resource.id = attachment.resource_node_id
         AND resource.deleted_at IS NULL
        JOIN projects project
          ON project.id = attachment.project_id
         AND project.deleted_at IS NULL
        JOIN project_memberships membership
          ON membership.project_id = attachment.project_id
         AND membership.identity_id = $3
         AND membership.state = 'active'
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            attachment.project_id, attachment.resource_node_id, $3
        ) permission ON true
        WHERE attachment.project_id = $1
          AND attachment.task_id = $2
          AND blob.upload_state <> 'deleted'
          AND (
              project.owner_identity_id = $3
              OR membership.role = 'admin'
              OR resource.created_by_identity_id = $3
              OR permission.access_level IS NOT NULL
          )
          AND (
              $4::timestamptz IS NULL
              OR (attachment.created_at, attachment.id)
                   > ($4::timestamptz, $5::uuid)
          )
        ORDER BY attachment.created_at, attachment.id
        LIMIT $6
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .bind(actor.identity_id)
    .bind(page.after_created_at)
    .bind(page.after_id)
    .bind(page.sql_limit()?)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    attachment_page(&mut rows, page)
}

pub async fn list_completed(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<sprout_api_contract::CollectionPageQuery>,
) -> Result<Json<ListAttachmentsResponse>, AppError> {
    let task_resource_id = task_resource(&state, actor, project_id, task_id).await?;
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        task_resource_id,
        ResourceAccess::ViewHeader,
    )
    .await?;
    let page = parse_page(query)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let mut rows = sqlx::query_as::<_, AttachmentCollectionRow>(
        r#"
        SELECT
            attachment.id,
            attachment.project_id,
            attachment.resource_node_id,
            attachment.key_epoch,
            'task_completed'::text AS attachment_kind,
            attachment.blob_id,
            attachment.task_id,
            NULL::uuid AS pretask_id,
            attachment.required_attachment_id AS source_attachment_id,
            attachment.assignment_id,
            CASE
                WHEN project.owner_identity_id = $3
                  OR membership.role = 'admin'
                  OR resource.created_by_identity_id = $3
                  OR permission.access_scope = 'full'
                THEN attachment.encrypted_metadata
                ELSE NULL
            END AS encrypted_metadata,
            blob.upload_state,
            blob.available_at,
            attachment.created_at
        FROM task_completed_attachments attachment
        JOIN file_blobs blob
          ON blob.project_id = attachment.project_id
         AND blob.id = attachment.blob_id
        JOIN resource_nodes resource
          ON resource.project_id = attachment.project_id
         AND resource.id = attachment.resource_node_id
         AND resource.deleted_at IS NULL
        JOIN projects project
          ON project.id = attachment.project_id
         AND project.deleted_at IS NULL
        JOIN project_memberships membership
          ON membership.project_id = attachment.project_id
         AND membership.identity_id = $3
         AND membership.state = 'active'
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            attachment.project_id, attachment.resource_node_id, $3
        ) permission ON true
        WHERE attachment.project_id = $1
          AND attachment.task_id = $2
          AND blob.upload_state <> 'deleted'
          AND (
              project.owner_identity_id = $3
              OR membership.role = 'admin'
              OR resource.created_by_identity_id = $3
              OR permission.access_level IS NOT NULL
          )
          AND (
              $4::timestamptz IS NULL
              OR (attachment.created_at, attachment.id)
                   > ($4::timestamptz, $5::uuid)
          )
        ORDER BY attachment.created_at, attachment.id
        LIMIT $6
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .bind(actor.identity_id)
    .bind(page.after_created_at)
    .bind(page.after_id)
    .bind(page.sql_limit()?)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    attachment_page(&mut rows, page)
}

pub async fn create_template(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, version_id, pretask_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<CreatePretaskTemplateAttachmentRequest>,
) -> Result<Json<CreateAttachmentResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let valid = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM preset_pretasks
            WHERE project_id = $1 AND preset_version_id = $2 AND id = $3
        )",
    )
    .bind(project_id)
    .bind(version_id)
    .bind(pretask_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if !valid {
        return Err(AppError::NotFound);
    }
    create_attachment(
        &state,
        actor,
        project_id,
        request.id,
        &request.blob,
        AttachmentInsert::Template {
            preset_version_id: version_id,
            pretask_id,
        },
    )
    .await
}

pub async fn create_required(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateTaskRequiredAttachmentRequest>,
) -> Result<Json<CreateAttachmentResponse>, AppError> {
    let task_resource_id = task_resource(&state, actor, project_id, task_id).await?;
    if task_resource_id != request.blob.resource_node_id {
        return Err(AppError::BadRequest(
            "required attachment must use the task resource",
        ));
    }
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        task_resource_id,
        ResourceAccess::Write,
    )
    .await?;
    create_attachment(
        &state,
        actor,
        project_id,
        request.id,
        &request.blob,
        AttachmentInsert::Required {
            task_id,
            source_template_attachment_id: request.source_template_attachment_id,
        },
    )
    .await
}

pub async fn create_completed(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateTaskCompletedAttachmentRequest>,
) -> Result<Json<CreateAttachmentResponse>, AppError> {
    let task_resource_id = task_resource(&state, actor, project_id, task_id).await?;
    if task_resource_id != request.blob.resource_node_id {
        return Err(AppError::BadRequest(
            "completed attachment must use the task resource",
        ));
    }
    require_exact_active_assignee(&state, actor, project_id, task_id, request.assignment_id)
        .await?;
    create_attachment(
        &state,
        actor,
        project_id,
        request.id,
        &request.blob,
        AttachmentInsert::Completed {
            task_id,
            assignment_id: request.assignment_id,
            required_attachment_id: request.required_attachment_id,
        },
    )
    .await
}

pub async fn create_info_document_file(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, document_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateInfoDocumentFileRequest>,
) -> Result<Json<CreateAttachmentResponse>, AppError> {
    let mut transaction = begin(&state, actor, project_id).await?;
    let resource_node_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM info_documents
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(document_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    if resource_node_id != request.blob.resource_node_id {
        return Err(AppError::BadRequest(
            "info file must use the document container resource",
        ));
    }
    require_resource_access(
        &state.pool,
        actor,
        project_id,
        resource_node_id,
        ResourceAccess::Write,
    )
    .await?;
    create_attachment(
        &state,
        actor,
        project_id,
        request.id,
        &request.blob,
        AttachmentInsert::InfoDocument,
    )
    .await
}

async fn create_attachment(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    attachment_id: Uuid,
    blob: &EncryptedBlobDeclarationDto,
    kind: AttachmentInsert,
) -> Result<Json<CreateAttachmentResponse>, AppError> {
    validate_declaration(state, blob)?;
    cleanup_project_storage(state, actor, project_id).await?;
    let ciphertext_hash = decode(&blob.ciphertext_sha256.0)?;
    let encrypted_blob_metadata = opaque_payload(&blob.encrypted_blob_metadata)?;
    let encrypted_attachment_metadata = opaque_payload(&blob.encrypted_attachment_metadata)?;
    let ciphertext_size =
        i64::try_from(blob.ciphertext_size).map_err(|_| AppError::PayloadTooLarge)?;
    let key_epoch =
        i32::try_from(blob.key_epoch).map_err(|_| AppError::BadRequest("invalid key epoch"))?;
    let storage_key = format!("{}.blob", Uuid::new_v4().simple());
    validate_storage_key(&storage_key)?;
    let link_id = match kind {
        AttachmentInsert::InfoDocument => attachment_id,
        _ => Uuid::new_v4(),
    };
    let link_kind = match kind {
        AttachmentInsert::InfoDocument => "inline",
        _ => "attachment",
    };
    let mut transaction = begin(state, actor, project_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 19))")
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    let epoch_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM resource_epochs
            WHERE project_id = $1 AND resource_node_id = $2
              AND epoch = $3 AND retired_at IS NULL
        )",
    )
    .bind(project_id)
    .bind(blob.resource_node_id)
    .bind(key_epoch)
    .fetch_one(&mut *transaction)
    .await?;
    if !epoch_active {
        return Err(AppError::BadRequest(
            "attachment must use the active resource key epoch",
        ));
    }
    let reserved = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(sum(ciphertext_size), 0)::bigint
         FROM file_blobs
         WHERE project_id = $1 AND upload_state IN ('pending', 'available')",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await?;
    let quota =
        i64::try_from(state.config.blob_project_quota_bytes).map_err(|_| AppError::Internal)?;
    if reserved
        .checked_add(ciphertext_size)
        .is_none_or(|total| total > quota)
    {
        return Err(AppError::PayloadTooLarge);
    }
    sqlx::query(
        r#"
        INSERT INTO file_blobs (
            id, project_id, storage_provider, storage_key, ciphertext_size,
            ciphertext_hash, key_epoch, encrypted_metadata,
            created_by_identity_id, resource_node_id
        )
        VALUES ($1, $2, 'filesystem', $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(blob.blob_id)
    .bind(project_id)
    .bind(&storage_key)
    .bind(ciphertext_size)
    .bind(&ciphertext_hash)
    .bind(key_epoch)
    .bind(&encrypted_blob_metadata)
    .bind(actor.identity_id)
    .bind(blob.resource_node_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO file_links (
            id, project_id, blob_id, resource_node_id, link_kind,
            encrypted_metadata, created_by_identity_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(link_id)
    .bind(project_id)
    .bind(blob.blob_id)
    .bind(blob.resource_node_id)
    .bind(link_kind)
    .bind(&encrypted_attachment_metadata)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    match kind {
        AttachmentInsert::Template {
            preset_version_id,
            pretask_id,
        } => {
            sqlx::query(
                r#"
                INSERT INTO pretask_template_attachments (
                    id, project_id, preset_version_id, pretask_id,
                    blob_id, resource_node_id, key_epoch,
                    encrypted_metadata, created_by_identity_id
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(attachment_id)
            .bind(project_id)
            .bind(preset_version_id)
            .bind(pretask_id)
            .bind(blob.blob_id)
            .bind(blob.resource_node_id)
            .bind(key_epoch)
            .bind(&encrypted_attachment_metadata)
            .bind(actor.identity_id)
            .execute(&mut *transaction)
            .await?;
        }
        AttachmentInsert::Required {
            task_id,
            source_template_attachment_id,
        } => {
            sqlx::query(
                r#"
                INSERT INTO task_required_attachments (
                    id, project_id, task_id, source_template_attachment_id,
                    blob_id, resource_node_id, key_epoch,
                    encrypted_snapshot, materialized_by_identity_id
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(attachment_id)
            .bind(project_id)
            .bind(task_id)
            .bind(source_template_attachment_id)
            .bind(blob.blob_id)
            .bind(blob.resource_node_id)
            .bind(key_epoch)
            .bind(&encrypted_attachment_metadata)
            .bind(actor.identity_id)
            .execute(&mut *transaction)
            .await?;
        }
        AttachmentInsert::Completed {
            task_id,
            assignment_id,
            required_attachment_id,
        } => {
            sqlx::query(
                r#"
                INSERT INTO task_completed_attachments (
                    id, project_id, task_id, assignment_id,
                    required_attachment_id, blob_id, resource_node_id,
                    key_epoch, encrypted_metadata, uploaded_by_identity_id
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                "#,
            )
            .bind(attachment_id)
            .bind(project_id)
            .bind(task_id)
            .bind(assignment_id)
            .bind(required_attachment_id)
            .bind(blob.blob_id)
            .bind(blob.resource_node_id)
            .bind(key_epoch)
            .bind(&encrypted_attachment_metadata)
            .bind(actor.identity_id)
            .execute(&mut *transaction)
            .await?;
        }
        AttachmentInsert::InfoDocument => {}
    }
    transaction.commit().await?;
    let attachment = load_attachment(state, actor, project_id, blob.blob_id).await?;
    Ok(Json(CreateAttachmentResponse {
        attachment,
        upload_url: SensitiveUrlDto(format!(
            "/v1/projects/{project_id}/files/{}/content",
            blob.blob_id
        )),
    }))
}

pub async fn get_metadata(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, blob_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<AttachmentDto>, AppError> {
    Ok(Json(
        load_attachment(&state, actor, project_id, blob_id).await?,
    ))
}

pub async fn upload(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, blob_id)): Path<(Uuid, Uuid)>,
    bytes: Bytes,
) -> Result<StatusCode, AppError> {
    let authorized = load_authorized_file(
        &state,
        actor,
        project_id,
        blob_id,
        FileAuthorization::Upload,
    )
    .await?;
    if authorized.upload_state != "pending" {
        return Err(AppError::Conflict);
    }
    if bytes.len() as i64 != authorized.ciphertext_size {
        return Err(AppError::BadRequest(
            "encrypted file size does not match declaration",
        ));
    }
    let actual_hash = Sha256::digest(&bytes);
    if actual_hash.as_slice() != authorized.ciphertext_hash {
        return Err(AppError::BadRequest(
            "encrypted file digest does not match declaration",
        ));
    }
    cleanup_project_storage(&state, actor, project_id).await?;
    let project_dir = project_blob_dir(&state, project_id);
    persist_ciphertext(
        &project_dir,
        &authorized.storage_key,
        &bytes,
        PersistFault::None,
    )
    .await
    .map_err(io_error)?;

    let final_path = safe_blob_path(&project_dir, &authorized.storage_key)?;
    let update = async {
        let mut transaction = begin(&state, actor, project_id).await?;
        let changed = sqlx::query(
            r#"
            UPDATE file_blobs
            SET upload_state = 'available', available_at = clock_timestamp()
            WHERE project_id = $1 AND id = $2 AND upload_state = 'pending'
            "#,
        )
        .bind(project_id)
        .bind(blob_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(AppError::Conflict);
        }
        transaction.commit().await?;
        Ok::<_, AppError>(())
    }
    .await;
    if let Err(error) = update {
        let _ = tokio::fs::remove_file(&final_path).await;
        let _ = sync_directory(&project_dir).await;
        return Err(error);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn download(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, blob_id)): Path<(Uuid, Uuid)>,
) -> Result<Response<Body>, AppError> {
    let authorized =
        load_authorized_file(&state, actor, project_id, blob_id, FileAuthorization::Read).await?;
    if authorized.upload_state != "available" {
        return Err(AppError::NotFound);
    }
    let project_dir = project_blob_dir(&state, project_id);
    let path = safe_blob_path(&project_dir, &authorized.storage_key)?;
    ensure_regular_single_link(&path).await.map_err(io_error)?;
    let file = tokio::fs::File::open(&path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AppError::NotFound
        } else {
            io_error(error)
        }
    })?;
    ensure_open_file_single_link(&file)
        .await
        .map_err(io_error)?;
    let bytes = tokio::fs::read(&path).await.map_err(io_error)?;
    if bytes.len() as i64 != authorized.ciphertext_size
        || Sha256::digest(&bytes).as_slice() != authorized.ciphertext_hash
    {
        tracing::error!(
            project_id = %project_id,
            blob_id = %blob_id,
            "encrypted blob integrity verification failed"
        );
        return Err(AppError::Internal);
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        )
        .header(
            CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!(
                "attachment; filename=\"encrypted-{}.bin\"",
                blob_id.simple()
            ))
            .map_err(|_| AppError::Internal)?,
        )
        .header(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"))
        .header(CACHE_CONTROL, HeaderValue::from_static("private, no-store"))
        .header(CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .map_err(|_| AppError::Internal)
}

#[derive(Clone, Copy)]
enum FileAuthorization {
    Read,
    Upload,
}

async fn load_authorized_file(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    blob_id: Uuid,
    access: FileAuthorization,
) -> Result<FileRow, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let row = sqlx::query_as::<_, FileRow>(FILE_SELECT)
        .bind(project_id)
        .bind(blob_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    match access {
        FileAuthorization::Read => {
            require_resource_access(
                &state.pool,
                actor,
                project_id,
                row.resource_node_id,
                ResourceAccess::Read,
            )
            .await?;
        }
        FileAuthorization::Upload if row.attachment_kind == "task_completed" => {
            let task_id = row.task_id.ok_or(AppError::Internal)?;
            let assignment_id = row.assignment_id.ok_or(AppError::Internal)?;
            require_exact_active_assignee(state, actor, project_id, task_id, assignment_id).await?;
        }
        FileAuthorization::Upload => {
            require_resource_access(
                &state.pool,
                actor,
                project_id,
                row.resource_node_id,
                ResourceAccess::Write,
            )
            .await?;
        }
    }
    Ok(row)
}

async fn load_attachment(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    blob_id: Uuid,
) -> Result<AttachmentDto, AppError> {
    let row =
        load_authorized_file(state, actor, project_id, blob_id, FileAuthorization::Read).await?;
    let attachment_kind = parse_attachment_kind(&row.attachment_kind)?;
    let state = attachment_state(&row.upload_state, row.available_at)?;
    Ok(AttachmentDto {
        id: row.attachment_id,
        project_id: row.project_id,
        resource_node_id: row.resource_node_id,
        attachment_kind,
        blob_id: row.id,
        task_id: row.task_id,
        pretask_id: row.pretask_id,
        source_attachment_id: row.source_attachment_id,
        assignment_id: row.assignment_id,
        uploaded_by_identity_id: row.created_by_identity_id,
        ciphertext_size: u64::try_from(row.ciphertext_size).map_err(|_| AppError::Internal)?,
        ciphertext_sha256: OpaqueDigestDto(encode(&row.ciphertext_hash)),
        key_epoch: u32::try_from(row.key_epoch).map_err(|_| AppError::Internal)?,
        encrypted_metadata: payload_from_bytes(&row.encrypted_metadata)?,
        state,
        created_at: row.created_at,
    })
}

fn attachment_page(
    rows: &mut Vec<AttachmentCollectionRow>,
    page: super::pagination::CollectionPage,
) -> Result<Json<ListAttachmentsResponse>, AppError> {
    let next_cursor = finish_page(rows, page, |row| (row.created_at, row.id))?;
    let attachments = std::mem::take(rows)
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<_, _>>()?;
    Ok(Json(ListAttachmentsResponse {
        attachments,
        next_cursor,
    }))
}

fn attachment_state(
    upload_state: &str,
    available_at: Option<DateTime<Utc>>,
) -> Result<AttachmentStateDto, AppError> {
    match (upload_state, available_at) {
        ("pending", None) => Ok(AttachmentStateDto::PendingUpload),
        ("available", Some(uploaded_at)) => Ok(AttachmentStateDto::Available { uploaded_at }),
        _ => Err(AppError::Internal),
    }
}

async fn task_resource(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<Uuid, AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let resource = sqlx::query_scalar::<_, Uuid>(
        "SELECT resource_node_id FROM tasks
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    transaction.commit().await?;
    Ok(resource)
}

async fn require_exact_active_assignee(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
    task_id: Uuid,
    assignment_id: Uuid,
) -> Result<(), AppError> {
    let mut transaction = begin(state, actor, project_id).await?;
    let allowed = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1
            FROM task_assignments assignment
            JOIN tasks task
              ON task.project_id = assignment.project_id
             AND task.id = assignment.task_id
             AND task.deleted_at IS NULL
            WHERE assignment.project_id = $1
              AND assignment.task_id = $2
              AND assignment.id = $3
              AND assignment.assignee_identity_id = $4
              AND assignment.revoked_at IS NULL
        )",
    )
    .bind(project_id)
    .bind(task_id)
    .bind(assignment_id)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if completed_attachment_upload_allowed(allowed) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn begin(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
) -> Result<Transaction<'_, Postgres>, AppError> {
    let mut transaction = state.pool.begin().await?;
    set_database_context(
        &mut transaction,
        actor.identity_id,
        Some(actor.device_id),
        Some(project_id),
    )
    .await?;
    Ok(transaction)
}

fn validate_declaration(
    state: &AppState,
    blob: &EncryptedBlobDeclarationDto,
) -> Result<(), AppError> {
    if blob.ciphertext_size == 0
        || blob.ciphertext_size > state.config.blob_max_file_bytes
        || blob.ciphertext_size > state.config.body_limit_bytes as u64
        || blob.key_epoch == 0
    {
        return Err(AppError::PayloadTooLarge);
    }
    let digest = decode(&blob.ciphertext_sha256.0)?;
    if digest.len() != 32 {
        return Err(AppError::BadRequest(
            "ciphertext SHA-256 must contain 32 bytes",
        ));
    }
    opaque_payload(&blob.encrypted_blob_metadata)?;
    opaque_payload(&blob.encrypted_attachment_metadata)?;
    Ok(())
}

fn project_blob_dir(state: &AppState, project_id: Uuid) -> PathBuf {
    state.config.blob_dir.join(project_id.simple().to_string())
}

fn validate_storage_key(storage_key: &str) -> Result<(), AppError> {
    let Some(stem) = storage_key.strip_suffix(".blob") else {
        return Err(AppError::Internal);
    };
    if stem.len() != 32
        || !stem
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AppError::Internal);
    }
    Ok(())
}

fn safe_blob_path(root: &FsPath, storage_key: &str) -> Result<PathBuf, AppError> {
    validate_storage_key(storage_key)?;
    let path = root.join(storage_key);
    if path.parent() != Some(root) {
        return Err(AppError::Internal);
    }
    Ok(path)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PersistFault {
    None,
    #[cfg(test)]
    DiskFull,
    #[cfg(test)]
    AfterWrite,
    #[cfg(test)]
    BeforeRename,
    #[cfg(test)]
    AfterRename,
}

async fn persist_ciphertext(
    project_dir: &FsPath,
    storage_key: &str,
    bytes: &[u8],
    fault: PersistFault,
) -> std::io::Result<()> {
    let _ = fault;
    prepare_project_dir(project_dir).await?;
    let final_path = project_dir.join(storage_key);
    if tokio::fs::symlink_metadata(&final_path).await.is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "blob destination already exists",
        ));
    }
    let temporary_path = project_dir.join(format!(".tmp-{}", Uuid::new_v4().simple()));
    let mut renamed = false;
    let result = async {
        let mut temporary = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .await?;
        #[cfg(test)]
        if fault == PersistFault::DiskFull {
            return Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "simulated disk full",
            ));
        }
        temporary.write_all(bytes).await?;
        temporary.sync_all().await?;
        ensure_open_file_single_link(&temporary).await?;
        #[cfg(test)]
        if fault == PersistFault::AfterWrite {
            return Err(std::io::Error::other("fault after write"));
        }
        drop(temporary);
        #[cfg(test)]
        if fault == PersistFault::BeforeRename {
            return Err(std::io::Error::other("fault before rename"));
        }
        tokio::fs::rename(&temporary_path, &final_path).await?;
        renamed = true;
        ensure_regular_single_link(&final_path).await?;
        sync_directory(project_dir).await?;
        #[cfg(test)]
        if fault == PersistFault::AfterRename {
            return Err(std::io::Error::other("fault after rename"));
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        if renamed {
            let _ = tokio::fs::remove_file(&final_path).await;
        }
        let _ = sync_directory(project_dir).await;
    }
    result
}

async fn prepare_project_dir(project_dir: &FsPath) -> std::io::Result<()> {
    if let Some(root) = project_dir.parent() {
        tokio::fs::create_dir_all(root).await?;
        let root_metadata = tokio::fs::symlink_metadata(root).await?;
        if !root_metadata.file_type().is_dir() || root_metadata.file_type().is_symlink() {
            return Err(std::io::Error::other("blob root is not a real directory"));
        }
    }
    match tokio::fs::symlink_metadata(project_dir).await {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Ok(_) => Err(std::io::Error::other(
            "project blob path is not a real directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(project_dir).await?;
            sync_directory(
                project_dir
                    .parent()
                    .ok_or_else(|| std::io::Error::other("project blob directory has no parent"))?,
            )
            .await
        }
        Err(error) => Err(error),
    }
}

async fn ensure_regular_single_link(path: &FsPath) -> std::io::Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other("blob path is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(std::io::Error::other("blob has multiple hard links"));
        }
    }
    Ok(())
}

async fn ensure_open_file_single_link(file: &tokio::fs::File) -> std::io::Result<()> {
    let metadata = file.metadata().await?;
    if !metadata.is_file() {
        return Err(std::io::Error::other("blob handle is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(std::io::Error::other("blob has multiple hard links"));
        }
    }
    Ok(())
}

async fn sync_directory(path: &FsPath) -> std::io::Result<()> {
    tokio::fs::File::open(path).await?.sync_all().await
}

async fn cleanup_project_storage(
    state: &AppState,
    actor: AuthSession,
    project_id: Uuid,
) -> Result<(), AppError> {
    let project_dir = project_blob_dir(state, project_id);
    match tokio::fs::symlink_metadata(&project_dir).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(AppError::Internal),
    }
    let cutoff = SystemTime::now()
        .checked_sub(ORPHAN_GRACE)
        .ok_or(AppError::Internal)?;
    let mut entries = tokio::fs::read_dir(&project_dir).await.map_err(io_error)?;
    while let Some(entry) = entries.next_entry().await.map_err(io_error)? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let metadata = entry.metadata().await.map_err(io_error)?;
        if metadata
            .modified()
            .map_or(true, |modified| modified > cutoff)
        {
            continue;
        }
        if name.starts_with(".tmp-") {
            let _ = tokio::fs::remove_file(entry.path()).await;
            continue;
        }
        if validate_storage_key(name).is_err() {
            continue;
        }
        let mut transaction = begin(state, actor, project_id).await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM file_blobs
                WHERE project_id = $1 AND storage_provider = 'filesystem'
                  AND storage_key = $2 AND upload_state <> 'deleted'
            )",
        )
        .bind(project_id)
        .bind(name)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        if !exists {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
    let _ = sync_directory(&project_dir).await;
    Ok(())
}

fn parse_attachment_kind(value: &str) -> Result<AttachmentKindDto, AppError> {
    match value {
        "pretask_template" => Ok(AttachmentKindDto::PretaskTemplate),
        "task_required" => Ok(AttachmentKindDto::TaskRequired),
        "task_completed" => Ok(AttachmentKindDto::TaskCompleted),
        "info_document" => Ok(AttachmentKindDto::InfoDocument),
        _ => Err(AppError::Internal),
    }
}

fn opaque_payload(payload: &EncryptedPayloadDto) -> Result<Vec<u8>, AppError> {
    if payload.version == 0
        || payload.algorithm.trim().is_empty()
        || payload.key_id.trim().is_empty()
        || decode(&payload.nonce_b64)?.is_empty()
        || decode(&payload.ciphertext_b64)?.is_empty()
    {
        return Err(AppError::BadRequest("encrypted payload is incomplete"));
    }
    serde_json::to_vec(payload).map_err(|_| AppError::BadRequest("invalid encrypted payload"))
}

fn payload_from_bytes(bytes: &[u8]) -> Result<EncryptedPayloadDto, AppError> {
    serde_json::from_slice(bytes).map_err(|_| AppError::Internal)
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 payload"))
}

fn encode(value: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn io_error(error: std::io::Error) -> AppError {
    tracing::error!(kind = ?error.kind(), "encrypted blob filesystem operation failed");
    AppError::Internal
}

const FILE_SELECT: &str = r#"
SELECT
    blob.id,
    blob.project_id,
    blob.storage_key,
    blob.ciphertext_size,
    blob.ciphertext_hash,
    blob.key_epoch,
    blob.resource_node_id,
    blob.encrypted_metadata,
    blob.upload_state,
    blob.created_by_identity_id,
    blob.created_at,
    blob.available_at,
    COALESCE(template.id, required.id, completed.id, inline_link.id) AS attachment_id,
    CASE
        WHEN template.id IS NOT NULL THEN 'pretask_template'
        WHEN required.id IS NOT NULL THEN 'task_required'
        WHEN completed.id IS NOT NULL THEN 'task_completed'
        WHEN inline_link.id IS NOT NULL THEN 'info_document'
    END AS attachment_kind,
    COALESCE(required.task_id, completed.task_id) AS task_id,
    template.pretask_id,
    COALESCE(
        required.source_template_attachment_id,
        completed.required_attachment_id
    ) AS source_attachment_id,
    completed.assignment_id
FROM file_blobs blob
LEFT JOIN pretask_template_attachments template
  ON template.project_id = blob.project_id
 AND template.blob_id = blob.id
LEFT JOIN task_required_attachments required
  ON required.project_id = blob.project_id
 AND required.blob_id = blob.id
LEFT JOIN task_completed_attachments completed
  ON completed.project_id = blob.project_id
 AND completed.blob_id = blob.id
LEFT JOIN file_links inline_link
  ON inline_link.project_id = blob.project_id
 AND inline_link.blob_id = blob.id
 AND inline_link.link_kind = 'inline'
 AND inline_link.removed_at IS NULL
WHERE blob.project_id = $1
  AND blob.id = $2
  AND blob.upload_state <> 'deleted'
  AND (
      template.id IS NOT NULL
      OR required.id IS NOT NULL
      OR completed.id IS NOT NULL
      OR inline_link.id IS NOT NULL
  )
"#;

#[derive(FromRow)]
struct AttachmentCollectionRow {
    id: Uuid,
    project_id: Uuid,
    resource_node_id: Uuid,
    key_epoch: i32,
    attachment_kind: String,
    blob_id: Uuid,
    task_id: Option<Uuid>,
    pretask_id: Option<Uuid>,
    source_attachment_id: Option<Uuid>,
    assignment_id: Option<Uuid>,
    encrypted_metadata: Option<Vec<u8>>,
    upload_state: String,
    available_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<AttachmentCollectionRow> for AttachmentCollectionItemDto {
    type Error = AppError;

    fn try_from(row: AttachmentCollectionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            resource_node_id: row.resource_node_id,
            key_epoch: u32::try_from(row.key_epoch).map_err(|_| AppError::Internal)?,
            attachment_kind: parse_attachment_kind(&row.attachment_kind)?,
            blob_id: row.blob_id,
            task_id: row.task_id,
            pretask_id: row.pretask_id,
            source_attachment_id: row.source_attachment_id,
            assignment_id: row.assignment_id,
            encrypted_metadata: row
                .encrypted_metadata
                .as_deref()
                .map(payload_from_bytes)
                .transpose()?,
            state: attachment_state(&row.upload_state, row.available_at)?,
            created_at: row.created_at,
        })
    }
}

#[derive(FromRow)]
struct FileRow {
    id: Uuid,
    project_id: Uuid,
    storage_key: String,
    ciphertext_size: i64,
    ciphertext_hash: Vec<u8>,
    key_epoch: i32,
    resource_node_id: Uuid,
    encrypted_metadata: Vec<u8>,
    upload_state: String,
    created_by_identity_id: Uuid,
    created_at: DateTime<Utc>,
    available_at: Option<DateTime<Utc>>,
    attachment_id: Uuid,
    attachment_kind: String,
    task_id: Option<Uuid>,
    pretask_id: Option<Uuid>,
    source_attachment_id: Option<Uuid>,
    assignment_id: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("sprout-blob-test-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn llr_05_4_atomic_faults_leave_no_partial_or_temp_blob() {
        for fault in [
            PersistFault::DiskFull,
            PersistFault::AfterWrite,
            PersistFault::BeforeRename,
            PersistFault::AfterRename,
        ] {
            let root = temp_dir();
            let key = format!("{}.blob", Uuid::new_v4().simple());
            assert!(
                persist_ciphertext(&root, &key, b"ciphertext", fault)
                    .await
                    .is_err()
            );
            if root.exists() {
                assert!(std::fs::read_dir(&root).unwrap().next().is_none());
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn llr_05_2_filesystem_uses_opaque_names_and_ciphertext_bytes_only() {
        let root = temp_dir();
        let key = format!("{}.blob", Uuid::new_v4().simple());
        let ciphertext = [0_u8, 255, 13, 37, 99];
        persist_ciphertext(&root, &key, &ciphertext, PersistFault::None)
            .await
            .unwrap();
        let entries = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![key.clone()]);
        assert_eq!(tokio::fs::read(root.join(key)).await.unwrap(), ciphertext);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn llr_05_4_rejects_symlink_and_hardlink_hazards() {
        let root = temp_dir();
        tokio::fs::create_dir_all(&root).await.unwrap();
        let outside = root.with_extension("outside");
        tokio::fs::write(&outside, b"ciphertext").await.unwrap();
        let key = format!("{}.blob", Uuid::new_v4().simple());
        let path = root.join(&key);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, &path).unwrap();
            assert!(
                persist_ciphertext(&root, &key, b"new", PersistFault::None)
                    .await
                    .is_err()
            );
            tokio::fs::remove_file(&path).await.unwrap();
            tokio::fs::hard_link(&outside, &path).await.unwrap();
            assert!(ensure_regular_single_link(&path).await.is_err());
        }
        let _ = tokio::fs::remove_file(path).await;
        tokio::fs::remove_file(outside).await.unwrap();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn llr_05_6_hostile_names_and_mime_are_not_part_of_the_contract() {
        assert!(validate_storage_key("../../payload.html").is_err());
        assert!(validate_storage_key("0123456789abcdef0123456789abcdef.blob").is_ok());
        let disposition = format!(
            "attachment; filename=\"encrypted-{}.bin\"",
            Uuid::nil().simple()
        );
        assert!(disposition.starts_with("attachment;"));
        assert!(!disposition.contains("text/html"));
    }
}
