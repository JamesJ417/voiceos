use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::AppState;

use super::auth::{authenticate, header_text};
use super::error::{ApiResult, api_error};

const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Deserialize)]
pub(crate) struct DocumentContextRequest {
    #[serde(rename = "device_id")]
    _device_id: String,
    query: String,
}

pub(crate) async fn upload_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if body.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "empty_file"));
    }
    if body.len() > MAX_FILE_BYTES {
        return Err(api_error(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large"));
    }
    let device_id = authenticate(&state, &headers)?;
    let encoded_filename = header_text(&headers, "x-voiceos-file-name")
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "file_name_required"))?;
    let filename = urlencoding::decode(encoded_filename)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_file_name"))?
        .into_owned();
    let media_type = header_text(&headers, "content-type")
        .unwrap_or("application/octet-stream")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if !supported_text_file(&filename, media_type) {
        return Err(api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_file_type",
        ));
    }
    let mode = header_text(&headers, "x-voiceos-document-mode").unwrap_or("reference");
    let document = state
        .store
        .ingest_text_document_for_owner(
            &state.primary_owner_id,
            &device_id,
            &filename,
            media_type,
            mode,
            &body,
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"document": document}))))
}

pub(crate) async fn list_files(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let documents = state
        .store
        .list_documents_for_owner(&state.primary_owner_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"documents": documents})))
}

pub(crate) async fn delete_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let deleted = state
        .store
        .delete_document_for_owner(&state.primary_owner_id, &document_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !deleted {
        return Err(api_error(StatusCode::NOT_FOUND, "document_not_found"));
    }
    Ok(Json(
        json!({"document_id": document_id, "status": "deleted"}),
    ))
}

pub(crate) async fn document_context(
    State(state): State<AppState>,
    Json(request): Json<DocumentContextRequest>,
) -> ApiResult<Json<Value>> {
    let context = state
        .store
        .relevant_document_context_for_owner(&state.primary_owner_id, &request.query, 6, 8_000)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"context": context})))
}

fn supported_text_file(filename: &str, media_type: &str) -> bool {
    let extension = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(
        extension.as_str(),
        "txt" | "md" | "markdown" | "json" | "csv"
    ) && matches!(
        media_type,
        "text/plain"
            | "text/markdown"
            | "text/csv"
            | "application/csv"
            | "application/json"
            | "application/octet-stream"
    )
}

#[cfg(test)]
mod tests {
    use super::supported_text_file;

    #[test]
    fn requires_both_supported_extension_and_media_type() {
        assert!(supported_text_file("profile.md", "text/markdown"));
        assert!(!supported_text_file("payload.exe", "text/plain"));
        assert!(!supported_text_file("profile.md", "application/pdf"));
    }
}
