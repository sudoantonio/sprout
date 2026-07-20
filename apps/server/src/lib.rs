mod archive_store;
pub mod auth;
pub mod config;
pub mod error;
pub mod observability;
pub mod routes;
pub mod worker;

use std::sync::Arc;

use axum::{
    Router,
    extract::{Request, State},
    http::{
        HeaderValue, Method, StatusCode,
        header::{
            AUTHORIZATION, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE,
            COOKIE, HeaderName, SET_COOKIE, STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS,
            X_FRAME_OPTIONS,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use config::Config;
use error::AppError;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing::info_span;
use webauthn_rs::{Webauthn, WebauthnBuilder};

#[derive(Clone, serde::Serialize)]
pub struct SyncWake {
    pub project_id: uuid::Uuid,
    pub cursor: i64,
}

pub struct AppState {
    pub config: Config,
    pub pool: PgPool,
    pub webauthn: Webauthn,
    pub sync_wake_tx: broadcast::Sender<SyncWake>,
    pub metrics: observability::Metrics,
    pub rate_limiter: observability::RateLimiter,
}

impl AppState {
    pub fn new(config: Config, pool: PgPool) -> Result<Self, AppError> {
        let webauthn = WebauthnBuilder::new(&config.webauthn_rp_id, &config.base_url)
            .map_err(|_| AppError::Internal)?
            .rp_name(&config.webauthn_rp_name)
            .build()
            .map_err(|_| AppError::Internal)?;
        let (sync_wake_tx, _) = broadcast::channel(1024);
        let metrics = observability::Metrics::default();
        metrics.start();
        Ok(Self {
            config,
            pool,
            webauthn,
            sync_wake_tx,
            metrics,
            rate_limiter: observability::RateLimiter::default(),
        })
    }
}

pub fn build_router(state: Arc<AppState>) -> Result<Router, AppError> {
    let request_id_header = HeaderName::from_static("x-request-id");
    let cors = cors_layer(&state.config)?;
    let body_limit = state.config.body_limit_bytes;

    Ok(routes::router()
        .fallback(routes::not_found)
        .layer(SetResponseHeaderLayer::if_not_present(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'; base-uri 'none'"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(SetSensitiveResponseHeadersLayer::new(std::iter::once(
            SET_COOKIE,
        )))
        .layer(SetSensitiveRequestHeadersLayer::new([
            AUTHORIZATION,
            COOKIE,
            HeaderName::from_static("sec-websocket-protocol"),
        ]))
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request| {
                info_span!(
                    "http.request",
                    method = %request.method(),
                    path = request.uri().path(),
                    request_id = tracing::field::Empty
                )
            }),
        )
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(cors)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            observability::enforce_rate_limits,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            reject_oversized_content_length,
        ))
        .layer(RequestBodyLimitLayer::new(body_limit))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            observability::observe_requests,
        ))
        .with_state(state))
}

async fn reject_oversized_content_length(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let too_large = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > state.config.body_limit_bytes);
    if too_large {
        StatusCode::PAYLOAD_TOO_LARGE.into_response()
    } else {
        next.run(request).await
    }
}

fn cors_layer(config: &Config) -> Result<CorsLayer, AppError> {
    let origins = config
        .cors_origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(&origin.origin().ascii_serialization())
                .map_err(|_| AppError::Internal)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("idempotency-key"),
            HeaderName::from_static("x-request-id"),
        ]))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    fn test_app() -> Router {
        test_app_with_config(Config::for_test())
    }

    fn test_app_with_config(config: Config) -> Router {
        let pool = PgPoolOptions::new()
            .connect_lazy(config.database_url.expose())
            .expect("test database URL is valid");
        let state = Arc::new(AppState::new(config, pool).expect("test state"));
        build_router(state).expect("test router")
    }

    #[tokio::test]
    async fn liveness_does_not_depend_on_postgres() {
        let response = test_app()
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
    async fn protected_routes_reject_missing_authentication() {
        let response = test_app()
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
    async fn request_body_limit_is_enforced() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/passkeys/authenticate/start")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(vec![b'x'; 2048]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn security_headers_and_request_id_are_present() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert!(response.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn unauthenticated_auth_rate_limit_fails_closed() {
        let mut config = Config::for_test();
        config.auth_rate_limit_per_minute = 2;
        let app = test_app_with_config(config);
        for expected in [
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::TOO_MANY_REQUESTS,
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/auth/passkeys/authenticate/start")
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
    }
}
