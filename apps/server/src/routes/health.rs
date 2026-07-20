use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{AppState, error::AppError};

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
}

pub async fn live() -> Json<Health> {
    Json(Health { status: "ok" })
}

pub async fn ready(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(1) => (StatusCode::OK, Json(Health { status: "ready" })),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Health {
                status: "unavailable",
            }),
        ),
    }
}

#[derive(Serialize)]
pub struct TraceHealth {
    status: &'static str,
    request_id: Option<String>,
}

pub async fn trace(headers: HeaderMap) -> Json<TraceHealth> {
    Json(TraceHealth {
        status: "ok",
        request_id: headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned),
    })
}

pub async fn metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let expected = state
        .config
        .metrics_token
        .as_ref()
        .ok_or(AppError::NotFound)?;
    let supplied = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;
    if !constant_time_token_matches(expected.expose(), supplied) {
        return Err(AppError::Unauthorized);
    }
    let worker_lag_seconds = sqlx::query_scalar::<_, f64>(
        "SELECT value FROM operational_metrics WHERE name = 'worker_lag_seconds'",
    )
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or(0.0);
    let mut response = state.metrics.render(worker_lag_seconds).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    Ok(response)
}

fn constant_time_token_matches(expected: &str, supplied: &str) -> bool {
    let expected_digest = Sha256::digest(expected.as_bytes());
    let supplied_digest = Sha256::digest(supplied.as_bytes());
    expected_digest
        .iter()
        .zip(supplied_digest)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_token_comparison_does_not_accept_prefixes() {
        assert!(constant_time_token_matches("full-secret", "full-secret"));
        assert!(!constant_time_token_matches("full-secret", "full"));
        assert!(!constant_time_token_matches(
            "full-secret",
            "full-secret-extra"
        ));
    }
}
