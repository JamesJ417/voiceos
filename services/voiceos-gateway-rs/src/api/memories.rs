use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct MemoryQuery {
    query: Option<String>,
    include_inactive: Option<bool>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct CreateMemory {
    content: String,
    category: Option<String>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let store = state.store.clone();
    let owner = state.primary_owner_id.clone();
    let memories = tokio::task::spawn_blocking(move || {
        store.search_memories_for_owner(
            &owner,
            query.query.as_deref(),
            query.include_inactive.unwrap_or(false),
            query.limit.unwrap_or(100),
        )
    })
    .await
    .map_err(internal)?
    .map_err(store_error)?;
    Ok(Json(json!({"memories": memories})))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateMemory>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device = authenticate(&state, &headers)?;
    validate(&request.content, request.category.as_deref())?;
    let store = state.store.clone();
    let owner = state.primary_owner_id.clone();
    let memory = tokio::task::spawn_blocking(move || {
        store.create_structured_memory(
            &owner,
            &device,
            &request.content,
            request.category.as_deref().unwrap_or("general"),
            "explicit-panel-entry",
            1.0,
            &format!("user://{device}"),
            None,
        )
    })
    .await
    .map_err(internal)?
    .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"memory": memory}))))
}

pub(crate) async fn correct(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CreateMemory>,
) -> ApiResult<Json<Value>> {
    let device = authenticate(&state, &headers)?;
    validate(&request.content, request.category.as_deref())?;
    let store = state.store.clone();
    let owner = state.primary_owner_id.clone();
    let memory = tokio::task::spawn_blocking(move || {
        store.create_structured_memory(
            &owner,
            &device,
            &request.content,
            request.category.as_deref().unwrap_or("general"),
            "user-correction",
            1.0,
            &format!("user://{device}"),
            Some(&memory_id),
        )
    })
    .await
    .map_err(internal)?
    .map_err(store_error)?;
    Ok(Json(json!({"memory": memory})))
}

pub(crate) async fn forget(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let store = state.store.clone();
    let owner = state.primary_owner_id.clone();
    let forgotten =
        tokio::task::spawn_blocking(move || store.forget_memory_for_owner(&owner, &memory_id))
            .await
            .map_err(internal)?
            .map_err(store_error)?;
    if !forgotten {
        return Err(api_error(StatusCode::NOT_FOUND, "memory_not_found"));
    }
    Ok(Json(json!({"forgotten": true})))
}

fn validate(content: &str, category: Option<&str>) -> ApiResult<()> {
    if content.trim().is_empty() || content.chars().count() > 500 {
        return Err(api_error(StatusCode::BAD_REQUEST, "memory_content_invalid"));
    }
    let category = category.unwrap_or("general");
    if !matches!(
        category,
        "general" | "identity" | "preference" | "person" | "project" | "routine" | "sensitive"
    ) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "memory_category_invalid",
        ));
    }
    Ok(())
}

fn internal(error: tokio::task::JoinError) -> (StatusCode, Json<Value>) {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
fn store_error(error: voiceos_core::StoreError) -> (StatusCode, Json<Value>) {
    api_error(StatusCode::BAD_REQUEST, error.to_string())
}
