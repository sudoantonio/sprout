use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use sprout_server::{AppState, build_router, config::Config};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn app() -> axum::Router {
    let config = Config::for_test();
    let pool = PgPoolOptions::new()
        .connect_lazy(config.database_url.expose())
        .expect("test database URL is valid");
    build_router(Arc::new(
        AppState::new(config, pool).expect("test application state"),
    ))
    .expect("test router")
}

#[tokio::test]
async fn liveness_is_available_without_database_io() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_require_the_dedicated_bearer_token() {
    for authorization in [None, Some("Bearer wrong-token")] {
        let mut request = Request::builder().uri("/internal/metrics");
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        let response = app()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn project_collection_requires_a_session() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn hlr01_routes_have_concrete_handlers() {
    for path in [
        "/v1/auth/email/verification/start",
        "/v1/auth/email/verification/finish",
        "/v1/auth/email/recovery/start",
        "/v1/auth/email/recovery/finish",
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
    }

    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects/00000000-0000-0000-0000-000000000001/invitations/accept")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn hybrid_key_and_recovery_routes_require_authentication() {
    for path in [
        "/v1/devices/00000000-0000-0000-0000-000000000001/key-packages",
        "/v1/devices/00000000-0000-0000-0000-000000000001/key-transparency",
    ] {
        let response = app()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects/00000000-0000-0000-0000-000000000001/recovery-requests")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn hlr02_permission_and_assignment_routes_are_wired() {
    for (method, path) in [
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/resource-key-envelopes",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/member-resource-keys",
        ),
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/resources/00000000-0000-0000-0000-000000000002/envelope-plan",
        ),
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/resources/00000000-0000-0000-0000-000000000002/permissions",
        ),
        (
            "DELETE",
            "/v1/projects/00000000-0000-0000-0000-000000000001/resources/00000000-0000-0000-0000-000000000002/permissions/00000000-0000-0000-0000-000000000003",
        ),
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/resources/00000000-0000-0000-0000-000000000002/permissions/00000000-0000-0000-0000-000000000003/rotation-plan",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/assignments",
        ),
        (
            "DELETE",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/assignments/00000000-0000-0000-0000-000000000003",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/complete-assignment",
        ),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn hlt03_task_lifecycle_routes_are_wired() {
    // T-LLR-12.1: every task CRUD mutation derives its actor from a valid
    // bearer session before parsing or applying the client command.
    for (method, path) in [
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/topics",
        ),
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/task-lists/00000000-0000-0000-0000-000000000002/tasks",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks",
        ),
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002",
        ),
        (
            "PUT",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002",
        ),
        (
            "DELETE",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/complete",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/copy",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/presets/00000000-0000-0000-0000-000000000002/versions",
        ),
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/presets?limit=25",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/preset-assignments/00000000-0000-0000-0000-000000000002/materialize",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/recurrence-series",
        ),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn hlt04_questionnaire_version_and_submission_routes_are_wired() {
    for (method, path) in [
        (
            "GET",
            "/v1/projects/00000000-0000-0000-0000-000000000001/questionnaires?limit=25",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/questionnaires/00000000-0000-0000-0000-000000000002/versions",
        ),
        (
            "PUT",
            "/v1/projects/00000000-0000-0000-0000-000000000001/questionnaires/00000000-0000-0000-0000-000000000002/versions/00000000-0000-0000-0000-000000000003",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/questionnaires/00000000-0000-0000-0000-000000000002/versions/00000000-0000-0000-0000-000000000003/publish",
        ),
        (
            "PUT",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/questionnaire-submission",
        ),
        (
            "POST",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/questionnaire-submission/submit",
        ),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn hlt05_distinct_attachment_routes_are_wired() {
    for method in ["GET", "POST"] {
        for path in [
            "/v1/projects/00000000-0000-0000-0000-000000000001/preset-versions/00000000-0000-0000-0000-000000000002/pretasks/00000000-0000-0000-0000-000000000003/attachments?limit=25",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/required-attachments?limit=25",
            "/v1/projects/00000000-0000-0000-0000-000000000001/tasks/00000000-0000-0000-0000-000000000002/completed-attachments?limit=25",
        ] {
            let response = app()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }
}

#[tokio::test]
async fn hlt08_retention_archive_routes_require_authentication() {
    for (method, path) in [
        ("GET", "/v1/retention/preferences"),
        ("PUT", "/v1/retention/preferences"),
        ("GET", "/v1/retention/archives"),
        (
            "GET",
            "/v1/retention/archives/00000000-0000-0000-0000-000000000001/download",
        ),
        (
            "POST",
            "/v1/retention/archives/00000000-0000-0000-0000-000000000001/receipt",
        ),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}
