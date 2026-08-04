use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct FloorRequest {
    action: String,
    phase: Option<String>,
    partial_transcript: Option<String>,
    response_text: Option<String>,
    display_name: Option<String>,
    ttl_seconds: Option<i64>,
}

pub(crate) async fn get_floor(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let floor = tokio::task::spawn_blocking(move || store.conversation_floor(&owner_id))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"floor": floor})))
}

pub(crate) async fn change_floor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<FloorRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conversation_id = store.resolve_owner_conversation(&owner_id, &device_id, None)?;
        store.change_conversation_floor(
            &owner_id,
            &conversation_id,
            &device_id,
            request.display_name.as_deref(),
            &request.action,
            request.phase.as_deref(),
            request.partial_transcript.as_deref(),
            request.response_text.as_deref(),
            request.ttl_seconds.unwrap_or(45),
        )
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    match result {
        Ok(floor) => Ok(Json(json!({"floor": floor}))),
        Err(voiceos_core::StoreError::InvalidInput(message))
            if message == "conversation_floor_not_owned" =>
        {
            Err(api_error(StatusCode::CONFLICT, message))
        }
        Err(voiceos_core::StoreError::InvalidInput(message)) => {
            Err(api_error(StatusCode::BAD_REQUEST, message))
        }
        Err(error) => Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        )),
    }
}
