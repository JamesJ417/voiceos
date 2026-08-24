use std::time::Instant;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct TurnRequest {
    session_id: Option<String>,
    text: String,
    provider: Option<String>,
    request_id: Option<String>,
    #[serde(default)]
    attachment_ids: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct TurnResponse {
    session_id: String,
    transcript: String,
    response_text: String,
    processing_ms: u128,
    provider: String,
    tool_calls: Vec<Value>,
    approvals: Vec<Value>,
    results: Vec<Value>,
    errors: Vec<Value>,
    evidence: Option<Value>,
    usage: Value,
    reply_audio_url: Option<String>,
}

pub(crate) async fn turn(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<TurnRequest>,
) -> ApiResult<Json<TurnResponse>> {
    let text = request.text.trim().to_owned();
    if text.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "text_required"));
    }
    if text.chars().count() > 8_000 {
        return Err(api_error(StatusCode::PAYLOAD_TOO_LARGE, "text_too_long"));
    }
    if request.attachment_ids.len() > 10
        || request.attachment_ids.iter().any(|id| id.trim().is_empty())
        || request
            .attachment_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != request.attachment_ids.len()
    {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_attachment_ids"));
    }
    if !request.attachment_ids.is_empty() && request.request_id.is_none() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "request_id_required_for_attachments",
        ));
    }

    let device_id = authenticate(&state, &headers)?;
    let ontology = state.ontology.clone();
    let ontology_owner = device_id.clone();
    let ontology_text = text.clone();
    // Shadow interpretation is deterministic and fail-open until ontology-driven
    // dispatch reaches parity with the existing router and permission broker.
    let _ = tokio::task::spawn_blocking(move || {
        ontology.interpret_deterministic(&ontology_owner, &ontology_text)
    })
    .await;
    let provider = state
        .router
        .select(&text, request.provider.as_deref())
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    let engine = state.engine.clone();
    let owner_id = state.primary_owner_id.clone();
    let client_session = request.session_id;
    let original_text = text.clone();
    let started = Instant::now();
    let (conversation_id, completion) = tokio::task::spawn_blocking(move || {
        engine.run_owner_turn_idempotent(
            voiceos_core::OwnerTurnInput {
                owner_id: &owner_id,
                device_id: &device_id,
                client_session_id: client_session.as_deref(),
                user_text: &original_text,
                tools: vec![],
                request_id: request.request_id.as_deref(),
                attachment_ids: request.attachment_ids,
            },
            provider.as_ref(),
        )
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;

    let tool_calls = completion
        .tool_calls
        .iter()
        .map(|call| json!({"name": call.name, "arguments": call.arguments}))
        .collect();
    Ok(Json(TurnResponse {
        session_id: conversation_id,
        transcript: text,
        response_text: completion.text,
        processing_ms: started.elapsed().as_millis().max(1),
        provider: completion.provider,
        tool_calls,
        approvals: vec![],
        results: vec![],
        errors: vec![],
        evidence: None,
        usage: serde_json::to_value(completion.usage).unwrap_or_else(|_| json!({})),
        reply_audio_url: None,
    }))
}
