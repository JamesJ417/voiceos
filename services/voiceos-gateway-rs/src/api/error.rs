use axum::Json;
use axum::http::StatusCode;
use serde_json::{Value, json};

pub(crate) type ApiError = (StatusCode, Json<Value>);
pub(crate) type ApiResult<T> = Result<T, ApiError>;

pub(crate) fn api_error(status: StatusCode, error: impl AsRef<str>) -> ApiError {
    (status, Json(json!({"error": error.as_ref()})))
}
