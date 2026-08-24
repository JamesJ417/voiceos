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
pub(crate) struct AttachmentReference {
    attachment_id: String,
    #[serde(default = "input_image_purpose")]
    purpose: String,
}

fn input_image_purpose() -> String {
    "input_image".to_owned()
}

#[derive(Deserialize)]
pub(crate) struct TurnRequest {
    session_id: Option<String>,
    text: String,
    provider: Option<String>,
    request_id: Option<String>,
    attachments: Option<Vec<AttachmentReference>>,
    attachment_ids: Option<Vec<String>>,
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
    if request.attachments.is_some() && request.attachment_ids.is_some() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "attachments_and_attachment_ids_are_mutually_exclusive",
        ));
    }
    let attachment_ids = match request.attachments {
        Some(attachments) => {
            let mut ids = Vec::with_capacity(attachments.len());
            for attachment in attachments {
                if attachment.purpose != "input_image" {
                    return Err(api_error(
                        StatusCode::BAD_REQUEST,
                        "unsupported_attachment_purpose",
                    ));
                }
                ids.push(attachment.attachment_id);
            }
            ids
        }
        None => request.attachment_ids.unwrap_or_default(),
    };

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
    if !attachment_ids.is_empty() && !provider.supports_vision() {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "vision_not_supported",
        ));
    }
    let engine = state.engine.clone();
    let owner_id = state.primary_owner_id.clone();
    let client_session = request.session_id;
    let original_text = text.clone();
    let request_id = request.request_id;
    let started = Instant::now();
    let (conversation_id, completion) = tokio::task::spawn_blocking(move || {
        let input = voiceos_core::OwnerTurnInput {
            owner_id: &owner_id,
            device_id: &device_id,
            client_session_id: client_session.as_deref(),
            user_text: &original_text,
            tools: vec![],
            request_id: request_id.as_deref(),
        };
        if attachment_ids.is_empty() {
            engine.run_owner_turn_idempotent(input, provider.as_ref())
        } else {
            engine.run_owner_turn_idempotent_with_attachments(
                input,
                provider.as_ref(),
                &attachment_ids,
            )
        }
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
