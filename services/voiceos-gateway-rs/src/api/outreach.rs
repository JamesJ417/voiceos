use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct OutreachQuery {
    #[serde(default)]
    include_closed: bool,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct CreateOutreachRequest {
    kind: String,
    priority: String,
    title: String,
    body: String,
    reason: String,
    task_id: Option<String>,
    conversation_id: Option<String>,
    dedupe_key: Option<String>,
    #[serde(default = "default_actions")]
    actions: Vec<String>,
    scheduled_for: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OutreachActionRequest {
    action: String,
    snooze_minutes: Option<u32>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Query(query): Query<OutreachQuery>,
) -> ApiResult<Json<Value>> {
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let records = tokio::task::spawn_blocking(move || {
        store.outreaches(&owner, query.include_closed, query.limit.unwrap_or(50))
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    Ok(Json(json!({"outreach": records})))
}

pub(crate) async fn policy(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let policy = tokio::task::spawn_blocking(move || store.outreach_policy(&owner))
        .await
        .map_err(internal_join)?
        .map_err(store_error)?;
    Ok(Json(json!({"policy": policy})))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    Json(request): Json<CreateOutreachRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let record = tokio::task::spawn_blocking(move || {
        store.create_outreach(
            &owner,
            &request.kind,
            &request.priority,
            &request.title,
            &request.body,
            &request.reason,
            request.task_id.as_deref(),
            request.conversation_id.as_deref(),
            request.dedupe_key.as_deref(),
            &request.actions,
            request.scheduled_for.as_deref(),
        )
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"outreach": record}))))
}

pub(crate) async fn act(
    State(state): State<AppState>,
    Path(outreach_id): Path<String>,
    Json(request): Json<OutreachActionRequest>,
) -> ApiResult<Json<Value>> {
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let action = request.action;
    let stored_action = action.clone();
    let record = tokio::task::spawn_blocking(move || {
        store.act_on_outreach(&owner, &outreach_id, &stored_action, request.snooze_minutes)
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    record
        .map(|outreach| Json(json!({"outreach": outreach, "action": action})))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "outreach_not_found"))
}

fn default_actions() -> Vec<String> {
    vec![
        "talk_now".to_owned(),
        "show_progress".to_owned(),
        "later".to_owned(),
        "dismiss".to_owned(),
    ]
}

fn internal_join(error: tokio::task::JoinError) -> super::error::ApiError {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    match error {
        voiceos_core::StoreError::InvalidInput(message) => {
            api_error(StatusCode::BAD_REQUEST, message)
        }
        other => api_error(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}
