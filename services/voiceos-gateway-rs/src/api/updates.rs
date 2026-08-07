use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct UpdateQuery {
    status: Option<String>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
pub(crate) struct DecisionRequest {
    decision: String,
}
#[derive(Deserialize)]
pub(crate) struct ActionRequest {
    action: String,
}
#[derive(Deserialize)]
pub(crate) struct DiscoverRequest {
    component: String,
    current_version: String,
    proposed_version: String,
    release_notes: String,
    dependency_changes: Value,
    api_changes: Value,
    configuration_changes: Value,
    skill_changes: Value,
    security_changes: Value,
    affected_components: Value,
    rollback_version: String,
    candidate_path: Option<String>,
    evidence: Value,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UpdateQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let proposals = state
        .store
        .update_proposals(
            &state.primary_owner_id,
            query.status.as_deref(),
            query.limit.unwrap_or(50),
        )
        .map_err(store_error)?;
    Ok(Json(json!({"proposals":proposals})))
}

pub(crate) async fn discover(
    State(state): State<AppState>,
    Json(request): Json<DiscoverRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let proposal = state
        .store
        .upsert_update_proposal(
            &state.primary_owner_id,
            &request.component,
            &request.current_version,
            &request.proposed_version,
            &request.release_notes,
            request.dependency_changes,
            request.api_changes,
            request.configuration_changes,
            request.skill_changes,
            request.security_changes,
            request.affected_components,
            &request.rollback_version,
            request.candidate_path.as_deref(),
            request.evidence,
        )
        .map_err(store_error)?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &proposal.id,
            "update.discovered",
            "vic:update-monitor",
            json!({"proposal":&proposal,"production_changed":false}),
        )
        .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"proposal":proposal}))))
}

pub(crate) async fn decide(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<DecisionRequest>,
) -> ApiResult<Json<Value>> {
    let device = authenticate(&state, &headers)?;
    let status = match request.decision.as_str() {
        "approve" => "approved",
        "reject" => "rejected",
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "decision_must_be_approve_or_reject",
            ));
        }
    };
    let proposal = state
        .store
        .set_update_status(&state.primary_owner_id, &id, status, None)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "update_not_found"))?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &id,
            "update.decision",
            &format!("device:{device}"),
            json!({"status":status}),
        )
        .map_err(store_error)?;
    Ok(Json(json!({"proposal":proposal})))
}

pub(crate) async fn action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<ActionRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let proposal = state
        .store
        .update_proposal(&state.primary_owner_id, &id)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "update_not_found"))?;
    if proposal.status != "approved" && !matches!(request.action.as_str(), "health_check") {
        return Err(api_error(StatusCode::CONFLICT, "update_must_be_approved"));
    }
    let candidate = proposal
        .candidate_path
        .as_deref()
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "candidate_path_missing"))?;
    if !candidate.starts_with("/var/lib/voiceos/update-candidates/hermes/") {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "candidate_path_outside_managed_root",
        ));
    }
    let (argv, rollback) = match request.action.as_str() {
        "stage" => (
            vec![
                "/opt/voiceos/ops/agents/stage-hermes-candidate.sh".to_owned(),
                format!("{candidate}/proposal.json"),
            ],
            "Remove the isolated candidate directory; production is unchanged.",
        ),
        "deploy" => (
            vec![
                "/opt/voiceos/ops/agents/deploy-hermes-candidate.sh".to_owned(),
                "deploy".to_owned(),
                candidate.to_owned(),
            ],
            "Run the rollback action to restore the pre-deployment snapshot.",
        ),
        "health_check" => (
            vec![
                "/opt/voiceos/ops/agents/deploy-hermes-candidate.sh".to_owned(),
                "health-check".to_owned(),
                candidate.to_owned(),
            ],
            "Read-only health check; no rollback required.",
        ),
        "rollback" => (
            vec![
                "/opt/voiceos/ops/agents/deploy-hermes-candidate.sh".to_owned(),
                "rollback".to_owned(),
                candidate.to_owned(),
            ],
            "The failed release is retained for investigation; redeploy only after a new approval.",
        ),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_update_action",
            ));
        }
    };
    Ok(Json(
        json!({"status":"approval_required","approval":{"tool":"rig.root_command","arguments":{"argv":argv,"cwd":"/opt/voiceos","timeout_seconds":900,"rollback":rollback},"single_use":true,"update_id":id,"exact_effect":request.action}}),
    ))
}

fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    match error {
        voiceos_core::StoreError::InvalidInput(message) => {
            api_error(StatusCode::BAD_REQUEST, message)
        }
        other => api_error(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}
