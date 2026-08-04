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
