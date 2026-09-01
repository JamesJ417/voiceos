use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
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

#[derive(Deserialize)]
pub(crate) struct ConversationListQuery {
    area_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct HistoryQuery {
    area_id: Option<String>,
    timezone_offset_minutes: Option<i32>,
    limit_days: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct CreateConversationRequest {
    area_id: String,
    title: Option<String>,
    request_id: String,
}

#[derive(Deserialize)]
pub(crate) struct SelectRequest {
    request_id: String,
}

#[derive(Deserialize)]
pub(crate) struct MoveRequest {
    source_area_id: String,
    destination_area_id: String,
    confirmed: bool,
    request_id: String,
}

#[derive(Deserialize)]
pub(crate) struct ImportRequest {
    import_id: String,
    conversation: voiceos_core::ConversationExport,
}

#[derive(Deserialize)]
pub(crate) struct SyncRequest {
    #[serde(default)]
    conversations: Vec<voiceos_core::ConversationSyncRecord>,
}

pub(crate) async fn areas(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let (selected_area_id, active_conversation) = tokio::task::spawn_blocking(move || {
        Ok::<_, voiceos_core::StoreError>((
            store.selected_area(&owner_id)?,
            store.active_conversation_record(&owner_id)?,
        ))
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(json!({
        "areas": state.store.conversation_areas(),
        "selected_area_id": selected_area_id,
        "active_conversation": active_conversation,
    })))
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ConversationListQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let area_id = query.area_id;
    let conversations = tokio::task::spawn_blocking(move || {
        store.conversations_for_owner(&owner_id, area_id.as_deref(), query.limit.unwrap_or(100))
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(json!({"conversations": conversations})))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateConversationRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let conversation = tokio::task::spawn_blocking(move || {
        store.create_conversation_in_area(
            &owner_id,
            &device_id,
            &request.area_id,
            request.title.as_deref(),
            &request.request_id,
        )
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"conversation": conversation})),
    ))
}

pub(crate) async fn select_area(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(area_id): Path<String>,
    Json(request): Json<SelectRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let selected_area_id = area_id.clone();
    let conversation = tokio::task::spawn_blocking(move || {
        store.select_area_for_owner(&owner_id, &device_id, &area_id, &request.request_id)
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(
        json!({"selected_area_id":selected_area_id,"conversation":conversation}),
    ))
}

pub(crate) async fn select(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<SelectRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let conversation = tokio::task::spawn_blocking(move || {
        store.select_conversation_for_owner(
            &owner_id,
            &device_id,
            &conversation_id,
            &request.request_id,
        )
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(json!({"conversation":conversation})))
}

pub(crate) async fn move_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<MoveRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let conversation = tokio::task::spawn_blocking(move || {
        store.move_conversation_for_owner(
            &owner_id,
            &device_id,
            &conversation_id,
            &request.source_area_id,
            &request.destination_area_id,
            request.confirmed,
            &request.request_id,
        )
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(json!({"conversation":conversation})))
}

pub(crate) async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let days = tokio::task::spawn_blocking(move || {
        store.conversation_history_days(
            &owner_id,
            query.area_id.as_deref(),
            query.timezone_offset_minutes.unwrap_or(0),
            query.limit_days.unwrap_or(30),
        )
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(json!({"days":days})))
}

pub(crate) async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let exported =
        tokio::task::spawn_blocking(move || store.export_conversation(&owner_id, &conversation_id))
            .await
            .map_err(internal_join_error)?
            .map_err(store_error)?;
    Ok(Json(json!({"conversation":exported})))
}

pub(crate) async fn conversation_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Query(query): Query<MessageQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let after = query.after.unwrap_or(0);
    let messages = tokio::task::spawn_blocking(move || {
        store.messages_for_owner_conversation(
            &owner_id,
            &conversation_id,
            after,
            query.limit.unwrap_or(200),
        )
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    let latest_sequence = messages
        .last()
        .map(|message| message.sequence)
        .unwrap_or(after);
    Ok(Json(
        json!({"after":after,"latest_sequence":latest_sequence,"messages":messages}),
    ))
}

pub(crate) async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ImportRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let conversation = tokio::task::spawn_blocking(move || {
        store.import_conversation(
            &owner_id,
            &device_id,
            &request.import_id,
            &request.conversation,
        )
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"conversation":conversation})),
    ))
}

pub(crate) async fn sync_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MessageQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let payload = tokio::task::spawn_blocking(move || {
        store.conversation_sync_payload(
            &owner_id,
            query.after.unwrap_or(0),
            query.limit.unwrap_or(500),
        )
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({})),
    ))
}

pub(crate) async fn sync_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SyncRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let applied = tokio::task::spawn_blocking(move || {
        store.apply_conversation_sync(&owner_id, &device_id, &request.conversations)
    })
    .await
    .map_err(internal_join_error)?
    .map_err(store_error)?;
    Ok(Json(json!({"applied":applied})))
}

pub(crate) async fn active(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conversation = store.active_conversation_record(&owner_id)?;
        let messages = store.recent_conversation_messages(&owner_id, 200)?;
        Ok::<_, voiceos_core::StoreError>((conversation, messages))
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let latest_sequence = result.1.last().map(|message| message.sequence).unwrap_or(0);
    Ok(Json(json!({
        "conversation_id": result.0.as_ref().map(|conversation| &conversation.id),
        "area_id": result.0.as_ref().map(|conversation| &conversation.area_id),
        "conversation": result.0,
        "latest_sequence": latest_sequence,
        "messages": result.1,
    })))
}

fn internal_join_error(error: tokio::task::JoinError) -> super::error::ApiError {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    match error {
        voiceos_core::StoreError::InvalidInput(_) => {
            api_error(StatusCode::BAD_REQUEST, "invalid_conversation_request")
        }
        error => api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
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
