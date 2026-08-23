use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::ExecutionEvent;

use crate::state::AppState;

use super::auth::{authenticate, header_text};
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct EventQuery {
    after: Option<i64>,
}

fn public_event_type(event_type: &str) -> Option<&str> {
    match event_type {
        "conversation.floor.changed" => Some("conversation.floor.changed"),
        "conversation.turn" => Some("conversation.turn"),
        "approval.proposed" => Some("approval.proposed"),
        "approval.decided" => Some("approval.decided"),
        "daily_plan.proposed" => Some("daily_plan.proposed"),
        "status.changed" => Some("status.changed"),
        "vic.outreach.created" => Some("vic.outreach.created"),
        "vic.outreach.updated" => Some("vic.outreach.updated"),
        value if value.starts_with("task.initiative.") => Some("task.initiative.updated"),
        "task.progress.recorded" => Some("task.progress.updated"),
        value if value.starts_with("task.") => Some("task.changed"),
        // Internal provider, skill, and job evidence remains private until a
        // dedicated public contract explicitly defines its redacted shape.
        _ => None,
    }
}

fn client_event(event: ExecutionEvent) -> Option<Value> {
    let event_type = public_event_type(&event.event_type)?;
    Some(json!({
        "id": event.id,
        "type": event_type,
        "payload": event.payload,
        "created_at": event.occurred_at,
    }))
}

pub(crate) async fn recovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let after = query.after.unwrap_or(0).max(0);
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let events =
        tokio::task::spawn_blocking(move || store.execution_events_after(&owner_id, after, 200))
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let latest_event_id = events.last().map(|event| event.id).unwrap_or(after);
    let events = events
        .into_iter()
        .filter_map(client_event)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "after": after,
        "latest_event_id": latest_event_id,
        "events": events,
    })))
}

pub(crate) async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
) -> ApiResult<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>> {
    authenticate(&state, &headers)?;
    let header_cursor =
        header_text(&headers, "last-event-id").and_then(|value| value.parse::<i64>().ok());
    let mut cursor = query.after.or(header_cursor).unwrap_or(0).max(0);
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let event_stream = async_stream::stream! {
        loop {
            let read_store = store.clone();
            let read_owner = owner_id.clone();
            match tokio::task::spawn_blocking(move || {
                read_store.execution_events_after(&read_owner, cursor, 100)
            }).await {
                Ok(Ok(events)) => {
                    if events.is_empty() {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                    for stored in events {
                        cursor = stored.id;
                        let Some(event_type) = public_event_type(&stored.event_type) else {
                            continue;
                        };
                        let event_type = event_type.to_owned();
                        let data = client_event(stored).expect("mapped public event").to_string();
                        yield Ok(Event::default()
                            .id(cursor.to_string())
                            .event(event_type)
                            .data(data));
                    }
                }
                _ => {
                    yield Ok(Event::default()
                        .event("status.changed")
                        .data("{\"type\":\"status.changed\",\"payload\":{\"error\":\"event_stream_unavailable\"}}"));
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    };
    Ok(Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}
