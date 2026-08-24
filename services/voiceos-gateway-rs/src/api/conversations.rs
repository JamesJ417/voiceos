use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::Role;

use crate::state::AppState;

use super::auth::{authenticate, header_text};
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct PrepareRequest {
    device_id: String,
    session_id: Option<String>,
    text: String,
    request_id: Option<String>,
    #[serde(default)]
    attachment_ids: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct MessageQuery {
    after: Option<i64>,
    limit: Option<usize>,
}

pub(crate) async fn active(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conversation_id = store.active_conversation(&owner_id)?;
        let messages = store.recent_conversation_messages(&owner_id, 200)?;
        Ok::<_, voiceos_core::StoreError>((conversation_id, messages))
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let latest_sequence = result.1.last().map(|message| message.sequence).unwrap_or(0);
    Ok(Json(json!({
        "conversation_id": result.0,
        "latest_sequence": latest_sequence,
        "messages": result.1,
    })))
}

pub(crate) async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MessageQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let after = query.after.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(200).clamp(1, 500);
    let messages =
        tokio::task::spawn_blocking(move || store.conversation_messages(&owner_id, after, limit))
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let latest_sequence = messages
        .last()
        .map(|message| message.sequence)
        .unwrap_or(after);
    Ok(Json(
        json!({"after": after, "latest_sequence": latest_sequence, "messages": messages}),
    ))
}

pub(crate) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MessageQuery>,
) -> ApiResult<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>> {
    authenticate(&state, &headers)?;
    let header_cursor =
        header_text(&headers, "last-event-id").and_then(|value| value.parse::<i64>().ok());
    let mut cursor = query.after.or(header_cursor).unwrap_or(0).max(0);
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let stream = async_stream::stream! {
        loop {
            let read_store = store.clone();
            let read_owner = owner_id.clone();
            match tokio::task::spawn_blocking(move || {
                read_store.conversation_messages(&read_owner, cursor, 100)
            }).await {
                Ok(Ok(messages)) => {
                    if messages.is_empty() {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                    for message in messages {
                        cursor = message.sequence;
                        let data = serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_owned());
                        yield Ok(Event::default()
                            .event("conversation.message")
                            .id(cursor.to_string())
                            .data(data));
                    }
                }
                _ => {
                    yield Ok(Event::default().event("conversation.error").data("{\"error\":\"message_stream_unavailable\"}"));
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

#[derive(Deserialize)]
pub(crate) struct CommitRequest {
    conversation_id: String,
    response_text: String,
    provider: String,
    device_id: Option<String>,
    request_id: Option<String>,
}

pub(crate) async fn prepare(
    State(state): State<AppState>,
    Json(request): Json<PrepareRequest>,
) -> ApiResult<Json<Value>> {
    if request.device_id.trim().is_empty() || request.text.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "device_id_and_text_required",
        ));
    }
    let engine = state.engine.clone();
    let owner_id = state.primary_owner_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let (conversation_id, mut context) = engine.prepare_owner_turn_with_attachments(
            &owner_id,
            &request.device_id,
            request.session_id.as_deref(),
            &request.text,
            request.request_id.as_deref(),
            &request.attachment_ids,
        )?;
        if context.recent_messages.last().is_some_and(|message| {
            message.role == Role::User && message.content == request.text.trim()
        }) {
            context.recent_messages.pop();
        }
        Ok::<_, voiceos_core::StoreError>((conversation_id, context))
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({
        "conversation_id": result.0,
        "context": result.1,
    })))
}

pub(crate) async fn commit(
    State(state): State<AppState>,
    Json(request): Json<CommitRequest>,
) -> ApiResult<Json<Value>> {
    if request.conversation_id.trim().is_empty()
        || request.response_text.trim().is_empty()
        || request.provider.trim().is_empty()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "conversation_id_response_text_and_provider_required",
        ));
    }
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(device_id) = request.device_id.as_deref() {
            engine.record_assistant_from(
                &request.conversation_id,
                &request.response_text,
                &request.provider,
                device_id,
                request.request_id.as_deref(),
            )
        } else {
            engine.record_assistant(
                &request.conversation_id,
                &request.response_text,
                &request.provider,
            )
        }
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"status": "committed"})))
}
