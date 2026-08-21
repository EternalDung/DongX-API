use axum::response::{IntoResponse, Response};
use axum::http::StatusCode;
use serde::Serialize;
use thiserror::Error;

/// Application error type.
///
/// Serves both Axum (data plane) and Tauri (management plane):
/// - For Axum: implements IntoResponse -> JSON error body with HTTP status
/// - For Tauri: implements Serialize -> string error returned to frontend invoke()
///
/// Java comparison: like a custom RuntimeException that carries both
/// an HTTP status code (for REST) and a serializable message (for RPC).
#[derive(Error, Debug, Serialize)]
#[serde(tag = "code", content = "message")]
pub enum AppError {
    #[error("Database error: {0}")]
    #[serde(serialize_with = "serialize_to_string")]
    Database(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Proxy error: {0}")]
    #[serde(serialize_with = "serialize_to_string")]
    Proxy(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Internal error: {0}")]
    #[serde(serialize_with = "serialize_to_string")]
    Internal(String),
}

/// Serialize a string-wrapped error as a plain string (not an object)
fn serialize_to_string<S>(value: &String, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(value)
}

// --- From impls for ergonomic ? operator ---

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Database(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Proxy(e.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

// --- Axum integration ---

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    message: String,
    code: String,
}

impl AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Validation(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &str {
        match self {
            AppError::Database(_) => "database_error",
            AppError::NotFound(_) => "not_found",
            AppError::Validation(_) => "validation_error",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Proxy(_) => "proxy_error",
            AppError::Crypto(_) => "crypto_error",
            AppError::Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorResponse {
            error: ErrorDetail {
                message: self.to_string(),
                code: self.code().to_string(),
            },
        };
        (status, axum::Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
