use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct ProposalQuery {
    status: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct DecisionRequest {
    decision: String,
}

#[derive(Deserialize)]
pub(crate) struct StatusRequest {
    status: String,
}

#[derive(Deserialize)]
pub(crate) struct FeedbackRequest {
    feedback: String,
    note: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct UsageQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct RecordUsageRequest {
    conversation_id: Option<String>,
    request_id: Option<String>,
    tool_calls: Value,
    result: Value,
    outcome: String,
}

#[derive(Deserialize)]
pub(crate) struct ImportSkillRequest {
    name: String,
    content: String,
    required_capabilities: Value,
    evidence: Value,
}

pub(crate) async fn import_proposal(
    State(state): State<AppState>,
    Json(request): Json<ImportSkillRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let proposal = state
        .store
        .propose_skill(
            &state.primary_owner_id,
            &request.name,
            &request.content,
            request.required_capabilities,
            request.evidence,
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"proposal": proposal}))))
}

pub(crate) async fn list_proposals(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProposalQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let status = query.status.as_deref().or(Some("proposed"));
    let proposals = state
        .store
        .skill_proposals(&state.primary_owner_id, status, query.limit.unwrap_or(20))
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"proposals": proposals})))
}

pub(crate) async fn list_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProposalQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let mut skills = state
        .store
        .skill_proposals(
            &state.primary_owner_id,
            query.status.as_deref().or(Some("approved")),
            query.limit.unwrap_or(200),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    if query.status.as_deref().unwrap_or("approved") == "approved" {
        let mut newest = std::collections::HashMap::<String, u32>::new();
        for skill in &skills {
            newest
                .entry(skill.name.clone())
                .and_modify(|version| *version = (*version).max(skill.version))
                .or_insert(skill.version);
        }
        skills.retain(|skill| newest.get(&skill.name) == Some(&skill.version));
    }
    Ok(Json(json!({"skills": skills})))
}

pub(crate) async fn decide_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
    Json(request): Json<DecisionRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let approve = match request.decision.as_str() {
        "approve" => true,
        "reject" => false,
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "decision_must_be_approve_or_reject",
            ));
        }
    };
    let proposal = state
        .store
        .decide_skill_proposal_as(
            &state.primary_owner_id,
            &skill_id,
            approve,
            &format!("device:{device_id}"),
        )
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "skill_proposal_not_pending"))?;
    Ok(Json(json!({"proposal": proposal})))
}

pub(crate) async fn set_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
    Json(request): Json<StatusRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let skill = state
        .store
        .set_skill_status_as(
            &state.primary_owner_id,
            &skill_id,
            &request.status,
            &format!("device:{device_id}"),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "approved_or_disabled_skill_not_found",
            )
        })?;
    Ok(Json(json!({"skill": skill})))
}

pub(crate) async fn record_usage(
    State(state): State<AppState>,
    Json(request): Json<RecordUsageRequest>,
) -> ApiResult<Json<Value>> {
    let usages = state
        .store
        .record_matching_skill_usages(
            &state.primary_owner_id,
            request.conversation_id.as_deref(),
            request.request_id.as_deref(),
            &request.tool_calls,
            &request.result,
            &request.outcome,
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"usages": usages})))
}

pub(crate) async fn list_usages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let usages = state
        .store
        .skill_usages(&state.primary_owner_id, query.limit.unwrap_or(50))
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"usages": usages})))
}

pub(crate) async fn review_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(usage_id): Path<String>,
    Json(request): Json<FeedbackRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let usage = state
        .store
        .review_skill_usage_as(
            &state.primary_owner_id,
            &usage_id,
            &request.feedback,
            request.note.as_deref(),
            &format!("device:{device_id}"),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "skill_usage_not_found"))?;
    Ok(Json(json!({"usage": usage})))
}
