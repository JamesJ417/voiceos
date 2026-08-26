use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::state::AppState;

use super::auth::{authenticate, header_text};
use super::error::{ApiResult, api_error};
use super::image_contract::detected_image_type;

const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;

pub(crate) async fn upload_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if body.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "empty_attachment"));
    }
    if body.len() > MAX_ATTACHMENT_BYTES {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "attachment_too_large",
        ));
    }
    let device_id = authenticate(&state, &headers)?;
    let filename = header_text(&headers, "x-voiceos-file-name")
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "attachment_name_required"))
        .and_then(|value| {
            urlencoding::decode(value)
                .map(|value| value.into_owned())
                .map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_attachment_name"))
        })?;
    let declared_type = header_text(&headers, "content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    let detected_type = detected_image_type(&body).ok_or_else(|| {
        api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_image_content",
        )
    })?;
    if declared_type != detected_type || !supported_image_name(&filename, detected_type) {
        return Err(api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "attachment_media_type_mismatch",
        ));
    }
    let attachment = state
        .store
        .ingest_attachment_for_owner(
            &state.primary_owner_id,
            &device_id,
            &filename,
            detected_type,
            &body,
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"attachment": attachment}))))
}

pub(crate) async fn attachment_content(
    State(state): State<AppState>,
    Path(attachment_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    authenticate(&state, &headers)?;
    state
        .store
        .cleanup_expired_attachments()
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let Some((attachment, bytes)) = state
        .store
        .attachment_content_for_owner(&state.primary_owner_id, &attachment_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    else {
        return Err(api_error(StatusCode::NOT_FOUND, "attachment_not_found"));
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, attachment.media_type),
            (header::CACHE_CONTROL, "private, max-age=300".to_owned()),
        ],
        bytes,
    )
        .into_response())
}

fn supported_image_name(filename: &str, media_type: &str) -> bool {
    let extension = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        (extension.as_str(), media_type),
        ("jpg" | "jpeg", "image/jpeg") | ("png", "image/png") | ("webp", "image/webp")
    )
}
