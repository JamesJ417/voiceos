use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct SleepCycleQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct StartSleepCycleRequest {
    idempotency_key: String,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SleepCycleQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let reports = tokio::task::spawn_blocking(move || {
        store.sleep_cycle_reports(&owner_id, query.limit.unwrap_or(30))
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    Ok(Json(json!({"sleep_cycles": reports})))
}

pub(crate) async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(sleep_cycle_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let report =
        tokio::task::spawn_blocking(move || store.sleep_cycle_report(&owner_id, &sleep_cycle_id))
            .await
            .map_err(internal_join)?
            .map_err(store_error)?;
    report
        .map(|report| Json(json!({"sleep_cycle": report})))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "sleep_cycle_not_found"))
}

pub(crate) async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StartSleepCycleRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let cycle = tokio::task::spawn_blocking(move || {
        store.create_dry_run_sleep_cycle(&owner_id, &request.idempotency_key)
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"sleep_cycle": cycle}))))
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
