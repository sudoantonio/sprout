mod assignments;
mod device_keys;
mod domain;
mod email;
mod files;
mod health;
mod pagination;
mod permissions;
mod project_recovery;
mod projects;
mod questionnaires;
mod retention;
mod sync;
mod task_flows;
mod webauthn;

use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;

pub(crate) use email::enqueue_retention_warning;

pub fn router() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .route("/health/trace", get(health::trace))
        .route("/internal/metrics", get(health::metrics))
        .route(
            "/v1/auth/passkeys/register/start",
            post(webauthn::registration_start),
        )
        .route(
            "/v1/auth/passkeys/register/finish",
            post(webauthn::registration_finish),
        )
        .route(
            "/v1/auth/passkeys/authenticate/start",
            post(webauthn::authentication_start),
        )
        .route(
            "/v1/auth/passkeys/authenticate/finish",
            post(webauthn::authentication_finish),
        )
        .route(
            "/v1/auth/email/verification/start",
            post(email::verification_start),
        )
        .route(
            "/v1/auth/email/verification/finish",
            post(email::verification_finish),
        )
        .route("/v1/auth/email/recovery/start", post(email::recovery_start))
        .route(
            "/v1/auth/email/recovery/finish",
            post(email::recovery_finish),
        )
        .route(
            "/v1/devices/{device_id}/key-packages",
            get(device_keys::list).post(device_keys::register),
        )
        .route(
            "/v1/devices/{device_id}/key-packages/{key_version}",
            axum::routing::delete(device_keys::revoke),
        )
        .route(
            "/v1/devices/{device_id}/key-transparency",
            get(device_keys::transparency),
        )
        .route("/v1/projects", get(projects::list).post(projects::create))
        .route("/v1/projects/{project_id}", get(projects::get_project))
        .route(
            "/v1/projects/{project_id}/invitations",
            get(projects::list_invitations).post(projects::create_invitation),
        )
        .route(
            "/v1/projects/{project_id}/invitations/accept",
            post(projects::accept_invitation),
        )
        .route(
            "/v1/projects/{project_id}/participant-suggestions",
            post(projects::participant_suggestions),
        )
        .route(
            "/v1/projects/{project_id}/device-key-packages",
            get(device_keys::list_project),
        )
        .route(
            "/v1/projects/{project_id}/resource-key-envelopes",
            get(permissions::list_recipient_envelopes),
        )
        .route(
            "/v1/projects/{project_id}/member-resource-keys",
            post(permissions::share_member_resource_keys),
        )
        .route(
            "/v1/projects/{project_id}/recovery-provision",
            get(project_recovery::provision_status).put(project_recovery::provision),
        )
        .route(
            "/v1/projects/{project_id}/recovery-provision/activate",
            post(project_recovery::activate),
        )
        .route(
            "/v1/projects/{project_id}/recovery-provision/shares/me",
            get(project_recovery::shares_me),
        )
        .route(
            "/v1/projects/{project_id}/recovery-rotation-plan",
            get(project_recovery::rotation_plan),
        )
        .route(
            "/v1/projects/{project_id}/recovery-requests",
            post(project_recovery::start),
        )
        .route(
            "/v1/projects/{project_id}/recovery-requests/{request_id}",
            get(project_recovery::status),
        )
        .route(
            "/v1/projects/{project_id}/recovery-requests/{request_id}/approvals",
            post(project_recovery::approve),
        )
        .route(
            "/v1/projects/{project_id}/recovery-requests/{request_id}/finalize",
            post(project_recovery::finalize),
        )
        .route(
            "/v1/projects/{project_id}/resources",
            post(domain::create_resource),
        )
        .route(
            "/v1/projects/{project_id}/resources/{resource_id}",
            get(domain::get_resource),
        )
        .route(
            "/v1/projects/{project_id}/resources/{resource_id}/epochs",
            post(permissions::initialize_epoch),
        )
        .route(
            "/v1/projects/{project_id}/resources/{resource_id}/envelope-plan",
            get(permissions::full_envelope_plan),
        )
        .route(
            "/v1/projects/{project_id}/resources/{resource_id}/permissions",
            get(permissions::list).post(permissions::grant),
        )
        .route(
            "/v1/projects/{project_id}/resources/{resource_id}/permissions/{grant_id}",
            axum::routing::delete(permissions::revoke),
        )
        .route(
            "/v1/projects/{project_id}/resources/{resource_id}/permissions/{grant_id}/rotation-plan",
            get(permissions::rotation_plan),
        )
        .route(
            "/v1/projects/{project_id}/topics",
            get(task_flows::list_topics).post(task_flows::create_topic),
        )
        .route(
            "/v1/projects/{project_id}/topics/{topic_id}",
            get(task_flows::get_topic)
                .put(task_flows::update_topic)
                .delete(task_flows::delete_topic),
        )
        .route(
            "/v1/projects/{project_id}/topics/{topic_id}/task-lists",
            get(task_flows::list_task_lists).post(task_flows::create_task_list),
        )
        .route(
            "/v1/projects/{project_id}/task-lists/{list_id}",
            get(task_flows::get_task_list)
                .put(task_flows::update_task_list)
                .delete(task_flows::delete_task_list),
        )
        .route(
            "/v1/projects/{project_id}/task-lists/{list_id}/tasks",
            get(task_flows::list_tasks),
        )
        .route(
            "/v1/projects/{project_id}/tasks",
            post(task_flows::create_task),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}",
            get(task_flows::get_task)
                .put(task_flows::update_task)
                .delete(task_flows::delete_task),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/complete",
            post(task_flows::complete_task),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/copy",
            post(task_flows::copy_task),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/assignments",
            get(assignments::list).post(assignments::assign),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/assignments/{assignment_id}",
            axum::routing::delete(assignments::revoke),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/complete-assignment",
            post(task_flows::complete_task),
        )
        .route(
            "/v1/projects/{project_id}/presets",
            get(task_flows::list_presets).post(task_flows::create_preset),
        )
        .route(
            "/v1/projects/{project_id}/presets/{preset_id}",
            get(task_flows::get_preset).delete(task_flows::delete_preset),
        )
        .route(
            "/v1/projects/{project_id}/presets/{preset_id}/versions",
            post(task_flows::create_preset_version),
        )
        .route(
            "/v1/projects/{project_id}/presets/{preset_id}/versions/{version_id}",
            get(task_flows::get_preset_version),
        )
        .route(
            "/v1/projects/{project_id}/preset-assignments",
            post(task_flows::create_preset_assignment),
        )
        .route(
            "/v1/projects/{project_id}/preset-assignments/{assignment_id}/materialize",
            post(task_flows::materialize_preset),
        )
        .route(
            "/v1/projects/{project_id}/recurrence-series",
            post(task_flows::create_recurrence),
        )
        .route(
            "/v1/projects/{project_id}/recurrence-series/{series_id}",
            get(task_flows::get_recurrence),
        )
        .route(
            "/v1/projects/{project_id}/recurrence-series/{series_id}/archive",
            post(task_flows::archive_recurrence),
        )
        .route(
            "/v1/projects/{project_id}/questionnaires",
            get(questionnaires::list).post(questionnaires::create),
        )
        .route(
            "/v1/projects/{project_id}/questionnaires/{questionnaire_id}",
            get(questionnaires::get),
        )
        .route(
            "/v1/projects/{project_id}/questionnaires/{questionnaire_id}/versions",
            get(questionnaires::list_versions).post(questionnaires::create_version),
        )
        .route(
            "/v1/projects/{project_id}/questionnaires/{questionnaire_id}/versions/{version_id}",
            get(questionnaires::get_version).put(questionnaires::update_draft),
        )
        .route(
            "/v1/projects/{project_id}/questionnaires/{questionnaire_id}/versions/{version_id}/publish",
            post(questionnaires::publish),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/questionnaire-submission",
            get(questionnaires::get_submission).put(questionnaires::upsert_submission_draft),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/questionnaire-submission/submit",
            post(questionnaires::finalize_submission),
        )
        .route("/v1/sync/push", post(sync::push))
        .route("/v1/sync/pull", post(sync::pull))
        .route("/v1/sync/wake", get(sync::websocket))
        .route(
            "/v1/retention/preferences",
            get(retention::get_preference).put(retention::update_preference),
        )
        .route(
            "/v1/retention/archives",
            get(retention::list_archives),
        )
        .route(
            "/v1/retention/warnings",
            get(retention::list_warnings),
        )
        .route(
            "/v1/retention/archives/{archive_id}/download",
            get(retention::download_archive),
        )
        .route(
            "/v1/retention/archives/{archive_id}/receipt",
            post(retention::record_receipt),
        )
        .route(
            "/v1/projects/{project_id}/preset-versions/{version_id}/pretasks/{pretask_id}/attachments",
            get(files::list_template).post(files::create_template),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/required-attachments",
            get(files::list_required).post(files::create_required),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/completed-attachments",
            get(files::list_completed).post(files::create_completed),
        )
        .route(
            "/v1/projects/{project_id}/files/{blob_id}",
            get(files::get_metadata),
        )
        .route(
            "/v1/projects/{project_id}/files/{blob_id}/content",
            get(files::download).put(files::upload),
        )
}

#[derive(Serialize)]
struct NotFoundBody {
    error: &'static str,
}

pub async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(NotFoundBody {
            error: "route_not_found",
        }),
    )
        .into_response()
}
