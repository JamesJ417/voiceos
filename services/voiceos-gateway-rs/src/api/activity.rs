use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub(crate) struct ActivityQuery {
    after: Option<i64>,
    limit: Option<usize>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ActivityQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let events = state
        .store
        .activity_events(
            &state.primary_owner_id,
            query.after.unwrap_or(0),
            query.limit.unwrap_or(200),
        )
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let items=events.into_iter().map(|event|{
        let payload=&event.payload;
        json!({
            "id":event.id,"occurred_at":event.occurred_at,"stream_id":event.stream_id,"type":event.event_type,"actor":event.actor,
            "noticed":payload.get("noticed").or_else(||payload.get("reason")),
            "decision":payload.get("decision").or_else(||payload.get("status")),
            "model":payload.get("model").or_else(||payload.get("provider")),
            "attempted":payload.get("action").or_else(||payload.get("tool_calls")),
            "changed":payload.get("result").or_else(||payload.get("to")).or_else(||payload.get("proposal")),
            "evidence":payload.get("evidence").unwrap_or(payload),
            "files":payload.get("artifacts").or_else(||payload.get("files")),
            "needs_you":event.event_type.contains("approval")||payload.get("approval_required").and_then(Value::as_bool).unwrap_or(false),
            "rollback":payload.get("rollback").or_else(||payload.pointer("/arguments/rollback")),
            "raw":payload,
        })
    }).collect::<Vec<_>>();
    Ok(Json(json!({"items":items})))
}
