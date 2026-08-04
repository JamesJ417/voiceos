use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::AutomationFrequencyLimit;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct AutomationQuery {
    #[serde(default)]
    include_disabled: bool,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct CreateAutomationRequest {
    name: String,
    description: String,
    trigger: Value,
    #[serde(default = "empty_object")]
    conditions: Value,
    permitted_actions: Vec<String>,
    frequency_limit: AutomationFrequencyLimit,
    evidence: Value,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct SetEnabledRequest {
    enabled: bool,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AutomationQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let rules = state
        .store
        .automation_rules(
            &state.primary_owner_id,
            query.include_disabled,
            query.limit.unwrap_or(100),
        )
        .map_err(store_error)?;
    Ok(Json(json!({"automations": rules})))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateAutomationRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    let rule = state
        .store
        .create_automation_rule(
            &state.primary_owner_id,
            &request.name,
            &request.description,
            request.trigger,
            request.conditions,
            request.permitted_actions,
            request.frequency_limit,
            request.evidence,
            request.enabled,
        )
        .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"automation": rule}))))
}

pub(crate) async fn set_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(automation_id): Path<String>,
    Json(request): Json<SetEnabledRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    state
        .store
        .set_automation_rule_enabled(&state.primary_owner_id, &automation_id, request.enabled)
        .map_err(store_error)?
        .map(|rule| Json(json!({"automation": rule})))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "automation_not_found"))
}

fn empty_object() -> Value {
    json!({})
}

fn enabled_by_default() -> bool {
    true
}

fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    match error {
        voiceos_core::StoreError::InvalidInput(message) => {
            api_error(StatusCode::BAD_REQUEST, message)
        }
        other => api_error(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}
