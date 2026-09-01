use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LeaseRequest {
    capabilities: Value,
    ttl_seconds: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckpointRequest {
    state: Value,
    rollback: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelRequest {
    reason: String,
}

pub(crate) async fn live(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        let job = store.job(&owner, &job_id)?;
        let latest_checkpoint = match job {
            Some(_) => store.latest_execution_checkpoint(&owner, &job_id)?,
            None => return Ok::<_, voiceos_core::StoreError>(None),
        };
        Ok(Some((
            store.job(&owner, &job_id)?.expect("checked job exists"),
            latest_checkpoint,
        )))
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    match result {
        Some((job, latest_checkpoint)) => Ok(Json(json!({
            "job": job,
            "latest_checkpoint": latest_checkpoint,
        }))),
        None => Err(api_error(StatusCode::NOT_FOUND, "execution_not_found")),
    }
}

pub(crate) async fn acquire_lease(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
    Json(request): Json<LeaseRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let lease_id = tokio::task::spawn_blocking(move || {
        store.acquire_capability_lease(&owner, &job_id, request.capabilities, request.ttl_seconds)
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"lease_id": lease_id}))))
}

pub(crate) async fn checkpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
    Json(request): Json<CheckpointRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let checkpoint = tokio::task::spawn_blocking(move || {
        store.checkpoint_execution(&owner, &job_id, request.state, request.rollback)
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"checkpoint": checkpoint}))))
}

pub(crate) async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
    Json(request): Json<CancelRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let cancelled = tokio::task::spawn_blocking(move || {
        store.cancel_execution(&owner, &job_id, &request.reason)
    })
    .await
    .map_err(internal_join)?
    .map_err(store_error)?;
    Ok(Json(json!({"cancelled": cancelled})))
}

pub(crate) async fn resume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let checkpoint = tokio::task::spawn_blocking(move || store.resume_execution(&owner, &job_id))
        .await
        .map_err(internal_join)?
        .map_err(store_error)?;
    Ok(Json(json!({"latest_checkpoint": checkpoint})))
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
