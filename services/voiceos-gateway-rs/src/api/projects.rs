use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct ProjectQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct CreateProjectRequest {
    title: String,
    goal_id: Option<String>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let projects = state
        .store
        .projects(&state.primary_owner_id, query.limit.unwrap_or(100))
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"projects": projects})))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateProjectRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let project = state
        .store
        .create_project(
            &state.primary_owner_id,
            request.goal_id.as_deref(),
            &request.title,
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &project.id,
            "project.created",
            &format!("device:{device_id}"),
            json!({"title": project.title}),
        )
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"project": project}))))
}
