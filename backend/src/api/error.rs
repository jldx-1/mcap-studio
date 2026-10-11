//! HTTP error serialization and stable status mapping.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
pub(super) type ApiResult<T> = Result<T, ApiError>;
pub struct ApiError(pub(super) anyhow::Error);
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = format!("{:#}", self.0);
        let lower = message.to_lowercase();
        let status = if lower.contains("unauthenticated") {
            StatusCode::UNAUTHORIZED
        } else if lower.contains("forbidden") {
            StatusCode::FORBIDDEN
        } else if lower.contains("too many login") {
            StatusCode::TOO_MANY_REQUESTS
        } else if lower.contains("not found") {
            StatusCode::NOT_FOUND
        } else if lower.contains("sha-256")
            || lower.contains("already exists")
            || lower.contains("does not match")
            || lower.contains("changed; refresh")
        {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_REQUEST
        };
        (status, Json(json!({"error":message}))).into_response()
    }
}
