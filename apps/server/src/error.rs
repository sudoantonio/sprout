use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("authentication required")]
    Unauthorized,
    #[error("access denied")]
    Forbidden,
    #[error("resource not found")]
    NotFound,
    #[error("request conflicts with existing state")]
    Conflict,
    #[error("project recovery is not provisioned")]
    RecoveryUnprovisioned,
    #[error("request exceeds configured storage limits")]
    PayloadTooLarge,
    #[error("invalid request: {0}")]
    BadRequest(&'static str),
    #[error("service is unavailable")]
    Unavailable,
    #[error("internal server error")]
    Internal,
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: &'static str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required",
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "access denied"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", "resource not found"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "request conflicts with existing state",
            ),
            Self::RecoveryUnprovisioned => (
                StatusCode::CONFLICT,
                "recovery_unprovisioned",
                "project recovery requires an active provisioned share set",
            ),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request exceeds configured storage limits",
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "service is unavailable",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error",
            ),
        };
        (
            status,
            Json(ErrorBody {
                error: ErrorDetail { code, message },
            }),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(database) if database.is_unique_violation() => Self::Conflict,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("40001") => {
                Self::Conflict
            }
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000") => {
                Self::Conflict
            }
            sqlx::Error::Database(database) if database.code().as_deref() == Some("42501") => {
                Self::Forbidden
            }
            sqlx::Error::Database(database) if database.code().as_deref() == Some("23514") => {
                Self::BadRequest("request violates a domain invariant")
            }
            _ => {
                tracing::error!(error = %redact_database_error(&error), "database operation failed");
                Self::Internal
            }
        }
    }
}

impl From<sprout_storage_postgres::StorageError> for AppError {
    fn from(error: sprout_storage_postgres::StorageError) -> Self {
        match error {
            sprout_storage_postgres::StorageError::IdempotencyConflict => Self::Conflict,
            sprout_storage_postgres::StorageError::InvalidInput(_) => {
                Self::BadRequest("invalid storage request")
            }
            sprout_storage_postgres::StorageError::Database(database) => database.into(),
            sprout_storage_postgres::StorageError::Migration(_) => Self::Internal,
        }
    }
}

fn redact_database_error(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(database) => format!(
            "database code={} constraint={}",
            database.code().as_deref().unwrap_or("unknown"),
            database.constraint().unwrap_or("unknown")
        ),
        sqlx::Error::ColumnDecode { index, source } => {
            format!("database column decode failed index={index} source={source}")
        }
        sqlx::Error::ColumnNotFound(column) => {
            format!("database result column not found column={column}")
        }
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "database pool unavailable".into(),
        _ => "database query failed".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_errors_do_not_include_sensitive_context() {
        let response = AppError::Internal.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!format!("{response:?}").contains("credential"));
    }
}
