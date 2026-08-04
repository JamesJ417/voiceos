use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct CalendarQuery {
    start_at: String,
    end_at: String,
}

#[derive(Deserialize)]
pub(crate) struct CalendarEventRequest {
    source_id: String,
    title: String,
    start_at: String,
    end_at: String,
    location: Option<String>,
    #[serde(default = "confirmed")]
    status: String,
    #[serde(default = "none_status")]
    response_status: String,
    task_id: Option<String>,
    #[serde(default)]
    preparation_minutes: u32,
    #[serde(default)]
    travel_minutes: u32,
    #[serde(default = "empty_object")]
    metadata: Value,
}

#[derive(Deserialize)]
pub(crate) struct TaskScheduleRequest {
    earliest_start_at: Option<String>,
    recurrence_rule: Option<String>,
    location: Option<String>,
    #[serde(default)]
    preparation_minutes: u32,
    #[serde(default)]
    travel_minutes: u32,
    preferred_time: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PlanRequest {
    day_start: String,
    day_end: String,
    #[serde(default = "unknown")]
    current_location: String,
}

pub(crate) async fn list_calendar(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CalendarQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let events = state
        .store
        .calendar_events(&state.primary_owner_id, &query.start_at, &query.end_at)
        .map_err(store_error)?;
    Ok(Json(json!({"events": events})))
}

pub(crate) async fn upsert_calendar(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CalendarEventRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    let event = state
        .store
        .upsert_calendar_event(
            &state.primary_owner_id,
            &request.source_id,
            &request.title,
            &request.start_at,
            &request.end_at,
            request.location.as_deref(),
            &request.status,
            &request.response_status,
            request.task_id.as_deref(),
            request.preparation_minutes,
            request.travel_minutes,
            request.metadata,
        )
        .map_err(store_error)?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            "calendar",
            "calendar.changed",
            "vic:calendar-adapter",
            json!({"event": &event}),
        )
        .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"event": event}))))
}

pub(crate) async fn set_task_schedule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<TaskScheduleRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let schedule = state
        .store
        .set_task_schedule(
            &state.primary_owner_id,
            &task_id,
            request.earliest_start_at.as_deref(),
            request.recurrence_rule.as_deref(),
            request.location.as_deref(),
            request.preparation_minutes,
            request.travel_minutes,
            request.preferred_time.as_deref(),
        )
        .map_err(store_error)?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &task_id,
            "task.schedule.changed",
            &format!("device:{device_id}"),
            json!({"schedule": &schedule}),
        )
        .map_err(store_error)?;
    Ok(Json(json!({"schedule": schedule})))
}

pub(crate) async fn daily_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PlanRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let plan = state
        .store
        .build_daily_work_plan(
            &state.primary_owner_id,
            &request.day_start,
            &request.day_end,
            &request.current_location,
        )
        .map_err(store_error)?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            "daily-plan",
            "daily_plan.proposed",
            "vic:planner",
            json!({"plan": &plan}),
        )
        .map_err(store_error)?;
    Ok(Json(json!({"plan": plan})))
}

fn confirmed() -> String {
    "confirmed".to_owned()
}
fn none_status() -> String {
    "none".to_owned()
}
fn unknown() -> String {
    "unknown".to_owned()
}
fn empty_object() -> Value {
    json!({})
}
fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    match error {
        voiceos_core::StoreError::InvalidInput(message) => {
            api_error(StatusCode::BAD_REQUEST, message)
        }
        other => api_error(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}
