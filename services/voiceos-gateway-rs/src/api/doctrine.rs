use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::{DoctrineError, NewDoctrineSource, RoutedDoctrineExtractor};

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    status: Option<String>,
    limit: Option<usize>,
    q: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionRequest {
    decision: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusRequest {
    active: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvaluationRequest {
    kind: String,
    input: String,
}

fn require_enabled(state: &AppState) -> ApiResult<()> {
    if !state.doctrine_flags.enabled {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "vic_doctrine_disabled",
        ));
    }
    Ok(())
}
fn require_audit(state: &AppState) -> ApiResult<()> {
    require_enabled(state)?;
    if !state.doctrine_flags.source_audit {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "vic_doctrine_source_audit_disabled",
        ));
    }
    Ok(())
}

pub(crate) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let summary = state
        .doctrine
        .status(&state.primary_owner_id)
        .map_err(map_error)?;
    Ok(Json(
        json!({"enabled":state.doctrine_flags.enabled,"extraction_enabled":state.doctrine_flags.extraction,"sleep_integration_enabled":state.doctrine_flags.sleep_integration,"runtime_enabled":state.doctrine_flags.runtime,"source_audit_enabled":state.doctrine_flags.source_audit,"status":summary}),
    ))
}
pub(crate) async fn sources(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_audit(&state)?;
    Ok(Json(
        json!({"profiles":state.doctrine.source_profiles(&state.primary_owner_id).map_err(map_error)?}),
    ))
}
pub(crate) async fn source_records(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_audit(&state)?;
    Ok(Json(
        json!({"records":state.doctrine.source_records(&state.primary_owner_id,query.limit.unwrap_or(100)).map_err(map_error)?}),
    ))
}
pub(crate) async fn register_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<NewDoctrineSource>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    require_audit(&state)?;
    let record = state
        .doctrine
        .register_source(&state.primary_owner_id, request)
        .map_err(map_error)?;
    Ok((StatusCode::CREATED, Json(json!({"record":record}))))
}
pub(crate) async fn process_record(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(record_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    // Manual processing exposes and acts on a private source record identifier, so it uses
    // the same stronger audit gate as source registration and provenance inspection.
    require_audit(&state)?;
    if !state.doctrine_flags.extraction {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "vic_doctrine_extraction_disabled",
        ));
    }
    let authority = state.doctrine.clone();
    let owner = state.primary_owner_id.clone();
    let router = state.router.clone();
    let candidates = tokio::task::spawn_blocking(move || {
        authority.process_record(&owner, &record_id, &RoutedDoctrineExtractor::new(router))
    })
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(map_error)?;
    Ok(Json(json!({"candidates":candidates})))
}
pub(crate) async fn revoke_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(record_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_audit(&state)?;
    Ok(Json(
        json!({"affected_active_doctrine":state.doctrine.revoke_source(&state.primary_owner_id,&record_id).map_err(map_error)?}),
    ))
}
pub(crate) async fn candidates(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_enabled(&state)?;
    Ok(Json(
        json!({"candidates":state.doctrine.candidates(&state.primary_owner_id,query.status.as_deref(),query.limit.unwrap_or(100)).map_err(map_error)?}),
    ))
}
pub(crate) async fn decide(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(candidate_id): Path<String>,
    Json(request): Json<DecisionRequest>,
) -> ApiResult<Json<Value>> {
    let device = authenticate(&state, &headers)?;
    require_enabled(&state)?;
    let candidate = state
        .doctrine
        .decide_candidate(&state.primary_owner_id, &candidate_id, &request.decision)
        .map_err(map_error)?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &candidate_id,
            "doctrine.candidate.decided",
            &format!("device:{device}"),
            json!({"decision":request.decision,"candidate_id":candidate_id}),
        )
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({"candidate":candidate})))
}
pub(crate) async fn set_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(candidate_id): Path<String>,
    Json(request): Json<StatusRequest>,
) -> ApiResult<Json<Value>> {
    let device = authenticate(&state, &headers)?;
    require_enabled(&state)?;
    let candidate = state
        .doctrine
        .set_active(&state.primary_owner_id, &candidate_id, request.active)
        .map_err(map_error)?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &candidate_id,
            "doctrine.status.changed",
            &format!("device:{device}"),
            json!({"active":request.active,"candidate_id":candidate_id}),
        )
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({"candidate":candidate})))
}
pub(crate) async fn provenance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(candidate_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_audit(&state)?;
    Ok(Json(
        json!({"provenance":state.doctrine.candidate_provenance(&state.primary_owner_id,&candidate_id).map_err(map_error)?}),
    ))
}
pub(crate) async fn active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_enabled(&state)?;
    if !state.doctrine_flags.runtime {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "vic_doctrine_runtime_disabled",
        ));
    }
    Ok(Json(
        json!({"doctrine":state.doctrine.active_doctrine(&state.primary_owner_id,query.q.as_deref().unwrap_or(""),query.limit.unwrap_or(20)).map_err(map_error)?}),
    ))
}
pub(crate) async fn lenses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_enabled(&state)?;
    Ok(Json(
        json!({"lenses":state.doctrine.reasoning_lenses(query.q.as_deref().unwrap_or("")).map_err(map_error)?}),
    ))
}
pub(crate) async fn contradictions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_enabled(&state)?;
    Ok(Json(
        json!({"contradictions":state.doctrine.contradictions(&state.primary_owner_id).map_err(map_error)?}),
    ))
}
pub(crate) async fn evaluate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EvaluationRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    require_enabled(&state)?;
    Ok(Json(
        json!({"evaluation":state.doctrine.run_evaluation(&state.primary_owner_id,&request.kind,&request.input).map_err(map_error)?}),
    ))
}

fn map_error(error: DoctrineError) -> super::error::ApiError {
    match error {
        DoctrineError::NotFound => api_error(StatusCode::NOT_FOUND, error.to_string()),
        DoctrineError::Invalid(_) | DoctrineError::InvalidState => {
            api_error(StatusCode::CONFLICT, error.to_string())
        }
        _ => api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}
