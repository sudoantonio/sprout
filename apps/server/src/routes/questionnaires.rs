use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{
    Json,
    extract::{Path, Query, State},
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sprout_api_contract::{
    CreateQuestionnaireRequest, CreateQuestionnaireVersionRequest,
    FinalizeQuestionnaireSubmissionRequest, ListQuestionnaireVersionsResponse,
    ListQuestionnairesResponse, QuestionKindDto, QuestionnaireAnswerDto, QuestionnaireDto,
    QuestionnaireOptionDto, QuestionnaireQuestionDto, QuestionnaireResponse,
    QuestionnaireSubmissionDto, QuestionnaireSubmissionResponse, QuestionnaireSubmissionStateDto,
    QuestionnaireVersionDto, QuestionnaireVersionResponse, QuestionnaireVersionStateDto,
    UpdateQuestionnaireDraftRequest, UpsertQuestionnaireDraftRequest,
};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        AuthSession, ProjectAccess, ResourceAccess, require_project_access,
        require_resource_access, set_database_context,
    },
    error::AppError,
};

use super::pagination::{finish_page, parse_page};

pub async fn create(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateQuestionnaireRequest>,
) -> Result<Json<QuestionnaireResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let payload = opaque_payload(&request.payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, QuestionnaireRow>(
        r#"
        INSERT INTO questionnaires (
            id, project_id, encrypted_metadata, created_by_identity_id
        )
        VALUES ($1, $2, $3, $4)
        RETURNING id, project_id, encrypted_metadata, created_at, NULL::timestamptz AS archived_at
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(payload)
    .bind(actor.identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireResponse {
        questionnaire: row.into_dto(0)?,
    }))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path(project_id): Path<Uuid>,
    Query(query): Query<sprout_api_contract::CollectionPageQuery>,
) -> Result<Json<ListQuestionnairesResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let page = parse_page(query)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let mut rows = sqlx::query_as::<_, QuestionnaireCollectionRow>(
        r#"
        SELECT
            questionnaire.id,
            questionnaire.project_id,
            questionnaire.encrypted_metadata,
            questionnaire.created_at,
            CASE
                WHEN questionnaire.state = 'archived' THEN questionnaire.updated_at
                ELSE NULL
            END AS archived_at,
            (
                SELECT COALESCE(max(version.version_number), 0)
                FROM questionnaire_versions version
                WHERE version.project_id = questionnaire.project_id
                  AND version.questionnaire_id = questionnaire.id
            ) AS latest_version
        FROM questionnaires questionnaire
        WHERE questionnaire.project_id = $1
          AND questionnaire.state <> 'archived'
          AND (
              $2::timestamptz IS NULL
              OR (questionnaire.created_at, questionnaire.id)
                   > ($2::timestamptz, $3::uuid)
          )
        ORDER BY questionnaire.created_at, questionnaire.id
        LIMIT $4
        "#,
    )
    .bind(project_id)
    .bind(page.after_created_at)
    .bind(page.after_id)
    .bind(page.sql_limit()?)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let next_cursor = finish_page(&mut rows, page, |row| (row.created_at, row.id))?;
    Ok(Json(ListQuestionnairesResponse {
        questionnaires: rows
            .into_iter()
            .map(QuestionnaireCollectionRow::into_dto)
            .collect::<Result<_, _>>()?,
        next_cursor,
    }))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, questionnaire_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<QuestionnaireResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = questionnaire_row(&mut transaction, project_id, questionnaire_id).await?;
    let latest = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(max(version_number), 0)
         FROM questionnaire_versions
         WHERE project_id = $1 AND questionnaire_id = $2",
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireResponse {
        questionnaire: row.into_dto(to_u32(latest)?)?,
    }))
}

pub async fn create_version(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, questionnaire_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateQuestionnaireVersionRequest>,
) -> Result<Json<QuestionnaireVersionResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    validate_questions(&request.questions)?;
    let schema = opaque_payload(&request.schema)?;
    let content_hash = decode(&request.content_hash_b64)?;
    if content_hash.len() < 16 {
        return Err(AppError::BadRequest(
            "questionnaire content hash is too short",
        ));
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended($1::uuid::text || ':' || $2::uuid::text, 18)
        )",
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .execute(&mut *transaction)
    .await?;
    questionnaire_row(&mut transaction, project_id, questionnaire_id).await?;
    if let Some(source_version_id) = request.source_version_id {
        let source_is_published = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM questionnaire_versions
                WHERE project_id = $1 AND questionnaire_id = $2
                  AND id = $3 AND published_at IS NOT NULL
            )",
        )
        .bind(project_id)
        .bind(questionnaire_id)
        .bind(source_version_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !source_is_published {
            return Err(AppError::BadRequest(
                "source questionnaire version is not published",
            ));
        }
    }
    let version_number = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(max(version_number), 0) + 1
         FROM questionnaire_versions
         WHERE project_id = $1 AND questionnaire_id = $2",
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO questionnaire_versions (
            id, project_id, questionnaire_id, version_number,
            source_version_id, encrypted_payload, content_hash,
            created_by_identity_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(request.id)
    .bind(project_id)
    .bind(questionnaire_id)
    .bind(version_number)
    .bind(request.source_version_id)
    .bind(schema)
    .bind(content_hash)
    .bind(actor.identity_id)
    .execute(&mut *transaction)
    .await?;
    insert_questions(&mut transaction, project_id, request.id, &request.questions).await?;
    let version = load_version(&mut transaction, project_id, questionnaire_id, request.id).await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireVersionResponse { version }))
}

pub async fn list_versions(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, questionnaire_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListQuestionnaireVersionsResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    questionnaire_row(&mut transaction, project_id, questionnaire_id).await?;
    let ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM questionnaire_versions
         WHERE project_id = $1 AND questionnaire_id = $2
         ORDER BY version_number",
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .fetch_all(&mut *transaction)
    .await?;
    let mut versions = Vec::with_capacity(ids.len());
    for id in ids {
        versions.push(load_version(&mut transaction, project_id, questionnaire_id, id).await?);
    }
    transaction.commit().await?;
    Ok(Json(ListQuestionnaireVersionsResponse { versions }))
}

pub async fn get_version(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, questionnaire_id, version_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<QuestionnaireVersionResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Member).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let version = load_version(&mut transaction, project_id, questionnaire_id, version_id).await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireVersionResponse { version }))
}

pub async fn update_draft(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, questionnaire_id, version_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<UpdateQuestionnaireDraftRequest>,
) -> Result<Json<QuestionnaireVersionResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    validate_questions(&request.questions)?;
    let schema = opaque_payload(&request.schema)?;
    let content_hash = decode(&request.content_hash_b64)?;
    if content_hash.len() < 16 {
        return Err(AppError::BadRequest(
            "questionnaire content hash is too short",
        ));
    }
    let mut transaction = begin(&state, actor, project_id).await?;
    let current = sqlx::query_as::<_, VersionRow>(&format!(
        "{VERSION_SELECT}
             WHERE project_id = $1 AND questionnaire_id = $2 AND id = $3
             FOR UPDATE"
    ))
    .bind(project_id)
    .bind(questionnaire_id)
    .bind(version_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if current.published_at.is_some() || current.revision != to_i64(request.expected_revision)? {
        return Err(AppError::Conflict);
    }
    sqlx::query(
        "DELETE FROM questionnaire_options
         WHERE project_id = $1 AND question_id IN (
             SELECT id FROM questionnaire_questions
             WHERE project_id = $1 AND questionnaire_version_id = $2
         )",
    )
    .bind(project_id)
    .bind(version_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "DELETE FROM questionnaire_questions
         WHERE project_id = $1 AND questionnaire_version_id = $2",
    )
    .bind(project_id)
    .bind(version_id)
    .execute(&mut *transaction)
    .await?;
    insert_questions(&mut transaction, project_id, version_id, &request.questions).await?;
    let updated = sqlx::query(
        "UPDATE questionnaire_versions
         SET encrypted_payload = $4, content_hash = $5, revision = revision + 1
         WHERE project_id = $1 AND questionnaire_id = $2 AND id = $3
           AND published_at IS NULL AND revision = $6",
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .bind(version_id)
    .bind(schema)
    .bind(content_hash)
    .bind(to_i64(request.expected_revision)?)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let version = load_version(&mut transaction, project_id, questionnaire_id, version_id).await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireVersionResponse { version }))
}

pub async fn publish(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, questionnaire_id, version_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<sprout_api_contract::PublishQuestionnaireVersionRequest>,
) -> Result<Json<QuestionnaireVersionResponse>, AppError> {
    require_project_access(&state.pool, actor, project_id, ProjectAccess::Manage).await?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let changed = sqlx::query(
        "UPDATE questionnaire_versions
         SET published_at = clock_timestamp(), revision = revision + 1
         WHERE project_id = $1 AND questionnaire_id = $2 AND id = $3
           AND published_at IS NULL AND revision = $4",
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .bind(version_id)
    .bind(to_i64(request.expected_revision)?)
    .execute(&mut *transaction)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    sqlx::query(
        "UPDATE questionnaires SET state = 'published'
         WHERE project_id = $1 AND id = $2 AND state = 'draft'",
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .execute(&mut *transaction)
    .await?;
    let version = load_version(&mut transaction, project_id, questionnaire_id, version_id).await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireVersionResponse { version }))
}

pub async fn upsert_submission_draft(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpsertQuestionnaireDraftRequest>,
) -> Result<Json<QuestionnaireSubmissionResponse>, AppError> {
    let encrypted_payload = opaque_payload(&request.encrypted_payload)?;
    let mut transaction = begin(&state, actor, project_id).await?;
    let task = load_submission_task(&mut transaction, project_id, task_id).await?;
    if task.questionnaire_version_id != Some(request.questionnaire_version_id) {
        return Err(AppError::BadRequest(
            "submission version does not match the task pin",
        ));
    }
    require_active_assignee(
        &mut transaction,
        project_id,
        task_id,
        request.assignment_id,
        actor.identity_id,
    )
    .await?;
    validate_answers(
        &mut transaction,
        project_id,
        request.questionnaire_version_id,
        &request.answers,
    )
    .await?;

    let existing = sqlx::query_as::<_, SubmissionRow>(&format!(
        "{SUBMISSION_SELECT}
         WHERE project_id = $1 AND task_id = $2
         FOR UPDATE"
    ))
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let submission_id = if let Some(existing) = existing {
        if existing.id != request.submission_id
            || existing.state != "draft"
            || existing.submitted_by_identity_id != actor.identity_id
            || request.expected_revision.map(to_i64).transpose()? != Some(existing.revision)
        {
            return Err(AppError::Conflict);
        }
        sqlx::query(
            "DELETE FROM questionnaire_answer_options
             WHERE project_id = $1 AND submission_id = $2",
        )
        .bind(project_id)
        .bind(existing.id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "DELETE FROM questionnaire_answers
             WHERE project_id = $1 AND submission_id = $2",
        )
        .bind(project_id)
        .bind(existing.id)
        .execute(&mut *transaction)
        .await?;
        let updated = sqlx::query(
            "UPDATE questionnaire_submissions
             SET encrypted_payload = $3, revision = revision + 1
             WHERE project_id = $1 AND id = $2
               AND state = 'draft' AND revision = $4",
        )
        .bind(project_id)
        .bind(existing.id)
        .bind(encrypted_payload)
        .bind(existing.revision)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AppError::Conflict);
        }
        existing.id
    } else {
        if request.expected_revision.is_some() {
            return Err(AppError::Conflict);
        }
        sqlx::query(
            r#"
            INSERT INTO questionnaire_submissions (
                id, project_id, questionnaire_version_id,
                submitted_by_identity_id, client_submission_id,
                encrypted_payload, state, task_id, assignment_id
            )
            VALUES ($1, $2, $3, $4, $1, $5, 'draft', $6, $7)
            "#,
        )
        .bind(request.submission_id)
        .bind(project_id)
        .bind(request.questionnaire_version_id)
        .bind(actor.identity_id)
        .bind(encrypted_payload)
        .bind(task_id)
        .bind(request.assignment_id)
        .execute(&mut *transaction)
        .await?;
        request.submission_id
    };
    insert_answers(
        &mut transaction,
        project_id,
        request.questionnaire_version_id,
        submission_id,
        &request.answers,
    )
    .await?;
    let submission = load_submission(&mut transaction, project_id, submission_id).await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireSubmissionResponse {
        submission,
        replayed: false,
    }))
}

pub async fn finalize_submission(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<FinalizeQuestionnaireSubmissionRequest>,
) -> Result<Json<QuestionnaireSubmissionResponse>, AppError> {
    let classical_signature = decode(&request.classical_signature_b64)?;
    let post_quantum_signature = decode(&request.post_quantum_signature_b64)?;
    if classical_signature.len() != 64
        || post_quantum_signature.is_empty()
        || request.signer_device_key_version == 0
    {
        return Err(AppError::BadRequest(
            "questionnaire submission requires both device signatures",
        ));
    }
    let request_hash = Sha256::digest(
        serde_json::to_vec(&request)
            .map_err(|_| AppError::BadRequest("invalid submission finalization"))?,
    )
    .to_vec();
    let mut transaction = begin(&state, actor, project_id).await?;
    let row = sqlx::query_as::<_, SubmissionRow>(&format!(
        "{SUBMISSION_SELECT}
         WHERE project_id = $1 AND task_id = $2
         FOR UPDATE"
    ))
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let idempotency_key = parse_idempotency_uuid(&request.idempotency_key.0)?;
    if row.state == "submitted" {
        if row.idempotency_key == Some(idempotency_key)
            && row.request_hash.as_deref() == Some(request_hash.as_slice())
        {
            let submission = load_submission(&mut transaction, project_id, row.id).await?;
            transaction.commit().await?;
            return Ok(Json(QuestionnaireSubmissionResponse {
                submission,
                replayed: true,
            }));
        }
        return Err(AppError::Conflict);
    }
    if row.submitted_by_identity_id != actor.identity_id
        || row.revision != to_i64(request.expected_revision)?
    {
        return Err(AppError::Forbidden);
    }
    require_active_assignee(
        &mut transaction,
        project_id,
        task_id,
        row.assignment_id.ok_or(AppError::Internal)?,
        actor.identity_id,
    )
    .await?;
    let changed = sqlx::query(
        r#"
        UPDATE questionnaire_submissions
        SET
            state = 'submitted',
            submitted_at = clock_timestamp(),
            signer_device_id = $3,
            signer_device_key_version = $4,
            classical_signature = $5,
            post_quantum_signature = $6,
            idempotency_key = $7,
            request_hash = $8,
            revision = revision + 1
        WHERE project_id = $1 AND id = $2
          AND state = 'draft' AND revision = $9
        "#,
    )
    .bind(project_id)
    .bind(row.id)
    .bind(actor.device_id)
    .bind(
        i32::try_from(request.signer_device_key_version)
            .map_err(|_| AppError::BadRequest("invalid device key version"))?,
    )
    .bind(classical_signature)
    .bind(post_quantum_signature)
    .bind(idempotency_key)
    .bind(request_hash)
    .bind(row.revision)
    .execute(&mut *transaction)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::Conflict);
    }
    let submission = load_submission(&mut transaction, project_id, row.id).await?;
    transaction.commit().await?;
    Ok(Json(QuestionnaireSubmissionResponse {
        submission,
        replayed: false,
    }))
}

pub async fn get_submission(
    State(state): State<Arc<AppState>>,
    actor: AuthSession,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<QuestionnaireSubmissionResponse>, AppError> {
    let mut transaction = begin(&state, actor, project_id).await?;
    let task = load_submission_task(&mut transaction, project_id, task_id).await?;
    let row = sqlx::query_as::<_, SubmissionRow>(&format!(
        "{SUBMISSION_SELECT} WHERE project_id = $1 AND task_id = $2"
    ))
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.state == "draft" {
        require_active_assignee(
            &mut transaction,
            project_id,
            task_id,
            row.assignment_id.ok_or(AppError::Internal)?,
            actor.identity_id,
        )
        .await?;
    }
    let submission = load_submission(&mut transaction, project_id, row.id).await?;
    transaction.commit().await?;
    if row.state == "submitted" {
        require_resource_access(
            &state.pool,
            actor,
            project_id,
            task.resource_node_id,
            ResourceAccess::Read,
        )
        .await?;
    }
    Ok(Json(QuestionnaireSubmissionResponse {
        submission,
        replayed: false,
    }))
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

async fn questionnaire_row(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    questionnaire_id: Uuid,
) -> Result<QuestionnaireRow, AppError> {
    sqlx::query_as::<_, QuestionnaireRow>(
        r#"
        SELECT
            id, project_id, encrypted_metadata, created_at,
            CASE WHEN state = 'archived' THEN updated_at ELSE NULL END AS archived_at
        FROM questionnaires
        WHERE project_id = $1 AND id = $2 AND state <> 'archived'
        "#,
    )
    .bind(project_id)
    .bind(questionnaire_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

async fn insert_questions(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    version_id: Uuid,
    questions: &[QuestionnaireQuestionDto],
) -> Result<(), AppError> {
    for question in questions {
        sqlx::query(
            r#"
            INSERT INTO questionnaire_questions (
                id, project_id, questionnaire_version_id, client_key,
                question_kind, ordinal, required, encrypted_payload
            )
            VALUES ($1, $2, $3, $1, $4, $5, $6, $7)
            "#,
        )
        .bind(question.id)
        .bind(project_id)
        .bind(version_id)
        .bind(question_kind_str(question.question_kind))
        .bind(to_i32(question.ordinal)?)
        .bind(question.required)
        .bind(opaque_payload(&question.payload)?)
        .execute(&mut **transaction)
        .await?;
        for option in &question.options {
            sqlx::query(
                r#"
                INSERT INTO questionnaire_options (
                    id, project_id, question_id, client_key,
                    ordinal, encrypted_payload
                )
                VALUES ($1, $2, $3, $1, $4, $5)
                "#,
            )
            .bind(option.id)
            .bind(project_id)
            .bind(question.id)
            .bind(to_i32(option.ordinal)?)
            .bind(opaque_payload(&option.payload)?)
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

async fn load_version(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    questionnaire_id: Uuid,
    version_id: Uuid,
) -> Result<QuestionnaireVersionDto, AppError> {
    let row = sqlx::query_as::<_, VersionRow>(&format!(
        "{VERSION_SELECT}
         WHERE project_id = $1 AND questionnaire_id = $2 AND id = $3"
    ))
    .bind(project_id)
    .bind(questionnaire_id)
    .bind(version_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let question_rows = sqlx::query_as::<_, QuestionRow>(
        r#"
        SELECT id, question_kind, ordinal, required, encrypted_payload
        FROM questionnaire_questions
        WHERE project_id = $1 AND questionnaire_version_id = $2
        ORDER BY ordinal
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut questions = Vec::with_capacity(question_rows.len());
    for question in question_rows {
        let option_rows = sqlx::query_as::<_, OptionRow>(
            r#"
            SELECT id, ordinal, encrypted_payload
            FROM questionnaire_options
            WHERE project_id = $1 AND question_id = $2
            ORDER BY ordinal
            "#,
        )
        .bind(project_id)
        .bind(question.id)
        .fetch_all(&mut **transaction)
        .await?;
        questions.push(QuestionnaireQuestionDto {
            id: question.id,
            question_kind: parse_question_kind(&question.question_kind)?,
            ordinal: to_u32(question.ordinal)?,
            required: question.required,
            payload: payload_from_bytes(&question.encrypted_payload)?,
            options: option_rows
                .into_iter()
                .map(|option| {
                    Ok(QuestionnaireOptionDto {
                        id: option.id,
                        ordinal: to_u32(option.ordinal)?,
                        payload: payload_from_bytes(&option.encrypted_payload)?,
                    })
                })
                .collect::<Result<_, AppError>>()?,
        });
    }
    let state = if row.published_at.is_some() {
        QuestionnaireVersionStateDto::Published
    } else {
        QuestionnaireVersionStateDto::Draft
    };
    Ok(QuestionnaireVersionDto {
        id: row.id,
        questionnaire_id: row.questionnaire_id,
        project_id: row.project_id,
        number: to_u32(row.version_number)?,
        source_version_id: row.source_version_id,
        schema: payload_from_bytes(&row.encrypted_payload)?,
        questions,
        revision: to_u64(row.revision)?,
        state,
        created_at: row.created_at,
        published_at: row.published_at,
    })
}

fn validate_questions(questions: &[QuestionnaireQuestionDto]) -> Result<(), AppError> {
    if questions.is_empty() {
        return Err(AppError::BadRequest(
            "questionnaire version requires at least one question",
        ));
    }
    let mut question_ids = HashSet::with_capacity(questions.len());
    let mut question_ordinals = HashSet::with_capacity(questions.len());
    let mut all_ids = HashSet::new();
    for question in questions {
        if !question_ids.insert(question.id)
            || !question_ordinals.insert(question.ordinal)
            || !all_ids.insert(question.id)
        {
            return Err(AppError::BadRequest(
                "question identifiers and ordinals must be unique",
            ));
        }
        match (question.question_kind, question.options.is_empty()) {
            (QuestionKindDto::SingleChoice | QuestionKindDto::MultipleChoice, false)
            | (QuestionKindDto::Open | QuestionKindDto::Boolean, true) => {}
            _ => {
                return Err(AppError::BadRequest(
                    "question options do not match question kind",
                ));
            }
        }
        let mut ordinals = HashSet::with_capacity(question.options.len());
        for option in &question.options {
            if !all_ids.insert(option.id) || !ordinals.insert(option.ordinal) {
                return Err(AppError::BadRequest(
                    "question option identifiers and ordinals must be unique",
                ));
            }
            opaque_payload(&option.payload)?;
        }
        opaque_payload(&question.payload)?;
    }
    Ok(())
}

async fn load_submission_task(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<SubmissionTaskRow, AppError> {
    sqlx::query_as::<_, SubmissionTaskRow>(
        "SELECT id, resource_node_id, questionnaire_version_id
         FROM tasks
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

async fn require_active_assignee(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    task_id: Uuid,
    assignment_id: Uuid,
    actor_id: Uuid,
) -> Result<(), AppError> {
    let assigned = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM task_assignments
            WHERE project_id = $1 AND task_id = $2 AND id = $3
              AND assignee_identity_id = $4 AND revoked_at IS NULL
        )",
    )
    .bind(project_id)
    .bind(task_id)
    .bind(assignment_id)
    .bind(actor_id)
    .fetch_one(&mut **transaction)
    .await?;
    if assigned {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn validate_answers(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    version_id: Uuid,
    answers: &[QuestionnaireAnswerDto],
) -> Result<(), AppError> {
    let questions = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, question_kind
         FROM questionnaire_questions
         WHERE project_id = $1 AND questionnaire_version_id = $2",
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .collect::<HashMap<_, _>>();
    let published = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM questionnaire_versions
            WHERE project_id = $1 AND id = $2 AND published_at IS NOT NULL
        )",
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !published || questions.is_empty() {
        return Err(AppError::BadRequest(
            "submission version is not a published questionnaire",
        ));
    }
    let mut answered = HashSet::with_capacity(answers.len());
    let mut answer_ids = HashSet::with_capacity(answers.len());
    for answer in answers {
        let kind = questions
            .get(&answer.question_id)
            .ok_or(AppError::BadRequest(
                "answer question does not belong to the task version",
            ))?;
        if !answered.insert(answer.question_id) || !answer_ids.insert(answer.id) {
            return Err(AppError::BadRequest("duplicate questionnaire answer"));
        }
        let mut options = HashSet::with_capacity(answer.selected_option_ids.len());
        if answer
            .selected_option_ids
            .iter()
            .any(|id| !options.insert(*id))
        {
            return Err(AppError::BadRequest("duplicate selected option"));
        }
        let option_shape_valid = match kind.as_str() {
            "open" | "boolean" => answer.selected_option_ids.is_empty(),
            "single_choice" => answer.selected_option_ids.len() == 1,
            "multiple_choice" => !answer.selected_option_ids.is_empty(),
            _ => false,
        };
        if !option_shape_valid {
            return Err(AppError::BadRequest(
                "selected options do not match question kind",
            ));
        }
        if !answer.selected_option_ids.is_empty() {
            let matching = sqlx::query_scalar::<_, i64>(
                "SELECT count(*)
                 FROM questionnaire_options
                 WHERE project_id = $1 AND question_id = $2 AND id = ANY($3)",
            )
            .bind(project_id)
            .bind(answer.question_id)
            .bind(&answer.selected_option_ids)
            .fetch_one(&mut **transaction)
            .await?;
            if matching
                != i64::try_from(answer.selected_option_ids.len())
                    .map_err(|_| AppError::BadRequest("too many selected options"))?
            {
                return Err(AppError::BadRequest(
                    "selected option does not belong to the answered question",
                ));
            }
        }
        opaque_payload(&answer.payload)?;
    }
    Ok(())
}

async fn insert_answers(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    version_id: Uuid,
    submission_id: Uuid,
    answers: &[QuestionnaireAnswerDto],
) -> Result<(), AppError> {
    for answer in answers {
        sqlx::query(
            r#"
            INSERT INTO questionnaire_answers (
                id, project_id, questionnaire_version_id,
                submission_id, question_id, encrypted_payload
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(answer.id)
        .bind(project_id)
        .bind(version_id)
        .bind(submission_id)
        .bind(answer.question_id)
        .bind(opaque_payload(&answer.payload)?)
        .execute(&mut **transaction)
        .await?;
        for option_id in &answer.selected_option_ids {
            sqlx::query(
                r#"
                INSERT INTO questionnaire_answer_options (
                    project_id, questionnaire_version_id,
                    submission_id, answer_id, question_id, option_id
                )
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(project_id)
            .bind(version_id)
            .bind(submission_id)
            .bind(answer.id)
            .bind(answer.question_id)
            .bind(option_id)
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

async fn load_submission(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    submission_id: Uuid,
) -> Result<QuestionnaireSubmissionDto, AppError> {
    let row = sqlx::query_as::<_, SubmissionRow>(&format!(
        "{SUBMISSION_SELECT} WHERE project_id = $1 AND id = $2"
    ))
    .bind(project_id)
    .bind(submission_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let answer_rows = sqlx::query_as::<_, AnswerRow>(
        "SELECT id, question_id, encrypted_payload
         FROM questionnaire_answers
         WHERE project_id = $1 AND submission_id = $2",
    )
    .bind(project_id)
    .bind(submission_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut answers = Vec::with_capacity(answer_rows.len());
    for answer in answer_rows {
        let selected_option_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT option_id
             FROM questionnaire_answer_options
             WHERE project_id = $1 AND answer_id = $2
             ORDER BY option_id",
        )
        .bind(project_id)
        .bind(answer.id)
        .fetch_all(&mut **transaction)
        .await?;
        answers.push(QuestionnaireAnswerDto {
            id: answer.id,
            question_id: answer.question_id,
            selected_option_ids,
            payload: payload_from_bytes(&answer.encrypted_payload)?,
        });
    }
    let state = match row.state.as_str() {
        "draft" => QuestionnaireSubmissionStateDto::Draft,
        "submitted" => QuestionnaireSubmissionStateDto::Submitted,
        _ => return Err(AppError::Internal),
    };
    Ok(QuestionnaireSubmissionDto {
        id: row.id,
        task_id: row.task_id.ok_or(AppError::Internal)?,
        assignment_id: row.assignment_id.ok_or(AppError::Internal)?,
        questionnaire_version_id: row.questionnaire_version_id,
        submitted_by_identity_id: row.submitted_by_identity_id,
        encrypted_payload: payload_from_bytes(&row.encrypted_payload)?,
        answers,
        state,
        revision: to_u64(row.revision)?,
        signer_device_id: row.signer_device_id,
        signer_device_key_version: row.signer_device_key_version.map(to_u32).transpose()?,
        created_at: row.created_at,
        updated_at: row.updated_at,
        submitted_at: row.submitted_at,
    })
}

fn question_kind_str(kind: QuestionKindDto) -> &'static str {
    match kind {
        QuestionKindDto::Open => "open",
        QuestionKindDto::SingleChoice => "single_choice",
        QuestionKindDto::MultipleChoice => "multiple_choice",
        QuestionKindDto::Boolean => "boolean",
    }
}

fn parse_question_kind(kind: &str) -> Result<QuestionKindDto, AppError> {
    match kind {
        "open" => Ok(QuestionKindDto::Open),
        "single_choice" => Ok(QuestionKindDto::SingleChoice),
        "multiple_choice" => Ok(QuestionKindDto::MultipleChoice),
        "boolean" => Ok(QuestionKindDto::Boolean),
        _ => Err(AppError::Internal),
    }
}

fn opaque_payload(payload: &sprout_api_contract::EncryptedPayloadDto) -> Result<Vec<u8>, AppError> {
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

fn payload_from_bytes(bytes: &[u8]) -> Result<sprout_api_contract::EncryptedPayloadDto, AppError> {
    serde_json::from_slice(bytes).map_err(|_| AppError::Internal)
}

fn decode(value: &str) -> Result<Vec<u8>, AppError> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid base64 ciphertext"))
}

fn parse_idempotency_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(|_| AppError::BadRequest("idempotency key must be a UUID"))
}

fn to_i32(value: u32) -> Result<i32, AppError> {
    i32::try_from(value).map_err(|_| AppError::BadRequest("numeric value is too large"))
}

fn to_i64(value: u64) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| AppError::BadRequest("numeric value is too large"))
}

fn to_u32(value: i32) -> Result<u32, AppError> {
    u32::try_from(value).map_err(|_| AppError::Internal)
}

fn to_u64(value: i64) -> Result<u64, AppError> {
    u64::try_from(value).map_err(|_| AppError::Internal)
}

const VERSION_SELECT: &str = r#"
SELECT
    id, project_id, questionnaire_id, version_number,
    source_version_id, encrypted_payload, revision,
    created_at, published_at
FROM questionnaire_versions
"#;

const SUBMISSION_SELECT: &str = r#"
SELECT
    id, project_id, questionnaire_version_id,
    submitted_by_identity_id, encrypted_payload, state,
    task_id, assignment_id, signer_device_id,
    signer_device_key_version, revision, idempotency_key,
    request_hash, created_at, updated_at, submitted_at
FROM questionnaire_submissions
"#;

#[derive(FromRow)]
struct QuestionnaireRow {
    id: Uuid,
    project_id: Uuid,
    encrypted_metadata: Vec<u8>,
    created_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

impl QuestionnaireRow {
    fn into_dto(self, latest_version: u32) -> Result<QuestionnaireDto, AppError> {
        Ok(QuestionnaireDto {
            id: self.id,
            project_id: self.project_id,
            payload: payload_from_bytes(&self.encrypted_metadata)?,
            latest_version,
            created_at: self.created_at,
            archived_at: self.archived_at,
        })
    }
}

#[derive(FromRow)]
struct QuestionnaireCollectionRow {
    id: Uuid,
    project_id: Uuid,
    encrypted_metadata: Vec<u8>,
    created_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
    latest_version: i32,
}

impl QuestionnaireCollectionRow {
    fn into_dto(self) -> Result<QuestionnaireDto, AppError> {
        Ok(QuestionnaireDto {
            id: self.id,
            project_id: self.project_id,
            payload: payload_from_bytes(&self.encrypted_metadata)?,
            latest_version: to_u32(self.latest_version)?,
            created_at: self.created_at,
            archived_at: self.archived_at,
        })
    }
}

#[derive(FromRow)]
struct VersionRow {
    id: Uuid,
    project_id: Uuid,
    questionnaire_id: Uuid,
    version_number: i32,
    source_version_id: Option<Uuid>,
    encrypted_payload: Vec<u8>,
    revision: i64,
    created_at: DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct QuestionRow {
    id: Uuid,
    question_kind: String,
    ordinal: i32,
    required: bool,
    encrypted_payload: Vec<u8>,
}

#[derive(FromRow)]
struct OptionRow {
    id: Uuid,
    ordinal: i32,
    encrypted_payload: Vec<u8>,
}

#[derive(FromRow)]
struct SubmissionTaskRow {
    #[allow(dead_code)]
    id: Uuid,
    resource_node_id: Uuid,
    questionnaire_version_id: Option<Uuid>,
}

#[derive(FromRow)]
struct SubmissionRow {
    id: Uuid,
    #[allow(dead_code)]
    project_id: Uuid,
    questionnaire_version_id: Uuid,
    submitted_by_identity_id: Uuid,
    encrypted_payload: Vec<u8>,
    state: String,
    task_id: Option<Uuid>,
    assignment_id: Option<Uuid>,
    signer_device_id: Option<Uuid>,
    signer_device_key_version: Option<i32>,
    revision: i64,
    idempotency_key: Option<Uuid>,
    request_hash: Option<Vec<u8>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    submitted_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct AnswerRow {
    id: Uuid,
    question_id: Uuid,
    encrypted_payload: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprout_api_contract::EncryptedPayloadDto;

    fn encrypted() -> EncryptedPayloadDto {
        EncryptedPayloadDto {
            version: 1,
            algorithm: "xchacha20poly1305".into(),
            key_id: "opaque".into(),
            nonce_b64: base64::engine::general_purpose::STANDARD.encode([1; 24]),
            ciphertext_b64: base64::engine::general_purpose::STANDARD.encode([2; 32]),
        }
    }

    #[test]
    fn llr_04_2_question_kind_metadata_is_validated_without_plaintext() {
        let question = QuestionnaireQuestionDto {
            id: Uuid::new_v4(),
            question_kind: QuestionKindDto::Open,
            ordinal: 0,
            required: true,
            payload: encrypted(),
            options: Vec::new(),
        };
        assert!(validate_questions(&[question]).is_ok());
    }

    #[test]
    fn llr_04_3_rejects_cross_question_option_shape_before_writes() {
        let invalid = QuestionnaireQuestionDto {
            id: Uuid::new_v4(),
            question_kind: QuestionKindDto::Open,
            ordinal: 0,
            required: false,
            payload: encrypted(),
            options: vec![QuestionnaireOptionDto {
                id: Uuid::new_v4(),
                ordinal: 0,
                payload: encrypted(),
            }],
        };
        assert!(validate_questions(&[invalid]).is_err());
    }
}
