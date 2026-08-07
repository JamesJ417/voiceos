use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::{
    FixtureSleepProposalGenerator, RoutedDoctrineExtractor, RoutedSleepProposalGenerator,
    SleepConfig, SleepError, SleepProposalGenerator,
};

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_trigger")]
    trigger_kind: String,
    #[serde(default)]
    config: SleepConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionRequest {
    action: String,
    proposal_id: Option<String>,
    memory_id: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchQuery {
    q: String,
    limit: Option<usize>,
    include_dreams: Option<bool>,
}

fn default_mode() -> String {
    "dry_run".to_owned()
}

fn default_trigger() -> String {
    "manual".to_owned()
}

pub(crate) async fn current_cycle(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let cycle = state
        .sleep_memory
        .latest_cycle(&state.primary_owner_id)
        .map_err(sleep_error)?;
    Ok(Json(
        json!({"enabled":state.sleep_memory_enabled,"cycle":cycle}),
    ))
}

pub(crate) async fn get_cycle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(cycle_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let cycle = state
        .sleep_memory
        .cycle(&cycle_id)
        .map_err(sleep_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "sleep_cycle_not_found"))?;
    let report = state
        .sleep_memory
        .morning_report(&state.primary_owner_id, Some(&cycle_id))
        .map_err(sleep_error)?;
    let events = state
        .sleep_memory
        .cycle_events(&cycle_id)
        .map_err(sleep_error)?;
    Ok(Json(json!({"cycle":cycle,"report":report,"events":events})))
}

pub(crate) async fn start_cycle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RunRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device = authenticate(&state, &headers)?;
    run(state, request, format!("device:{device}")).await
}

pub(crate) async fn internal_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut request): Json<RunRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authorize_internal(state.internal_token.as_deref(), &headers)?;
    request.trigger_kind = "scheduled".to_owned();
    run(state, request, "scheduler".to_owned()).await
}

fn authorize_internal(expected: Option<&str>, headers: &HeaderMap) -> ApiResult<()> {
    let expected = expected.ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "internal_scheduler_auth_not_configured",
        )
    })?;
    let supplied = headers
        .get("x-voiceos-internal-token")
        .and_then(|value| value.to_str().ok());
    if supplied != Some(expected) {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "invalid_internal_scheduler_token",
        ));
    }
    Ok(())
}

async fn run(
    state: AppState,
    request: RunRequest,
    actor: String,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if !state.sleep_memory_enabled {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sleep_memory_feature_disabled",
        ));
    }
    if request.mode == "commit" && request.trigger_kind == "scheduled" {
        // Scheduled commits remain bounded by the Rust validation and protected-memory gates.
    }
    let authority = state.sleep_memory.clone();
    let owner = state.primary_owner_id.clone();
    let router = state.router.clone();
    let model_mode = state.sleep_model_mode.clone();
    let mode = request.mode.clone();
    let doctrine_mode = request.mode.clone();
    let trigger = request.trigger_kind.clone();
    let result = tokio::task::spawn_blocking(move || {
        let generator: Box<dyn SleepProposalGenerator> = if model_mode == "routed" {
            Box::new(RoutedSleepProposalGenerator::new(router))
        } else {
            Box::new(FixtureSleepProposalGenerator)
        };
        authority.run_cycle(&owner, &mode, &trigger, request.config, generator.as_ref())
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(sleep_error)?;
    let doctrine_report = if state.doctrine_flags.enabled
        && state.doctrine_flags.extraction
        && state.doctrine_flags.sleep_integration
        && doctrine_mode == "commit"
    {
        let doctrine = state.doctrine.clone();
        let owner = state.primary_owner_id.clone();
        let router = state.router.clone();
        tokio::task::spawn_blocking(move || {
            let pending = doctrine
                .source_records(&owner, 100)
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|record| {
                    record.active
                        && record.authorization_status == "approved"
                        && matches!(record.extraction_status.as_str(), "pending" | "failed")
                })
                .take(5)
                .collect::<Vec<_>>();
            let mut records_processed = 0usize;
            let mut candidates_staged = 0usize;
            let mut contamination_failures = 0usize;
            let mut failures = 0usize;
            let extractor = RoutedDoctrineExtractor::new(router);
            for record in pending {
                match doctrine.process_record(&owner, &record.id, &extractor) {
                    Ok(candidates) => {
                        records_processed += 1;
                        candidates_staged += candidates.len();
                        contamination_failures += candidates
                            .iter()
                            .filter(|candidate| candidate.status == "decontamination_failed")
                            .count();
                    }
                    Err(_) => failures += 1,
                }
            }
            Ok::<Value, String>(json!({
                "records_processed": records_processed,
                "candidates_staged": candidates_staged,
                "contamination_failures": contamination_failures,
                "processing_failures": failures,
                "automatic_approvals": 0,
                "automatic_activations": 0
            }))
        })
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?
    } else {
        json!({"status":"not_run"})
    };
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &result.0.id,
            "memory.sleep.completed",
            &actor,
            json!({"cycle":&result.0,"report":&result.1,"doctrine":&doctrine_report}),
        )
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"cycle":result.0,"report":result.1,"doctrine":doctrine_report})),
    ))
}

pub(crate) async fn cycle_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(cycle_id): Path<String>,
    Json(request): Json<ActionRequest>,
) -> ApiResult<Json<Value>> {
    let device = authenticate(&state, &headers)?;
    if !state.sleep_memory_enabled && !matches!(request.action.as_str(), "rollback" | "cancel") {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sleep_memory_feature_disabled",
        ));
    }
    if request.action == "resume" {
        let authority = state.sleep_memory.clone();
        let router = state.router.clone();
        let model_mode = state.sleep_model_mode.clone();
        let resume_id = cycle_id.clone();
        let (cycle, report) = tokio::task::spawn_blocking(move || {
            let generator: Box<dyn SleepProposalGenerator> = if model_mode == "routed" {
                Box::new(RoutedSleepProposalGenerator::new(router))
            } else {
                Box::new(FixtureSleepProposalGenerator)
            };
            authority.resume_cycle(&resume_id, generator.as_ref())
        })
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(sleep_error)?;
        let result = json!({"cycle":cycle,"report":report});
        state
            .store
            .append_execution_event(
                &state.primary_owner_id,
                &cycle_id,
                "memory.sleep.action",
                &format!("device:{device}"),
                json!({"action":"resume","result":&result}),
            )
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        return Ok(Json(result));
    }
    let result = match request.action.as_str() {
        "commit" => {
            let (cycle, report) = state
                .sleep_memory
                .commit_staged_cycle(&cycle_id)
                .map_err(sleep_error)?;
            json!({"cycle":cycle,"report":report})
        }
        "rollback" => json!({"cycle":state.sleep_memory.rollback_cycle(
            &cycle_id,
            request.reason.as_deref().unwrap_or("operator requested rollback")
        ).map_err(sleep_error)?}),
        "cancel" => {
            json!({"cancelled":state.sleep_memory.cancel_cycle(&cycle_id).map_err(sleep_error)?})
        }
        "pause" => {
            json!({"paused":state.sleep_memory.pause_cycle(&cycle_id).map_err(sleep_error)?})
        }
        "approve_proposal" | "reject_proposal" => {
            let proposal_id = request
                .proposal_id
                .as_deref()
                .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "proposal_id_required"))?;
            json!({"updated":state.sleep_memory.approve_proposal(&cycle_id,proposal_id,request.action == "approve_proposal").map_err(sleep_error)?})
        }
        "promote_dream" => {
            let memory_id = request
                .memory_id
                .as_deref()
                .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "memory_id_required"))?;
            json!({"promoted":state.sleep_memory.promote_dream(&state.primary_owner_id,memory_id).map_err(sleep_error)?})
        }
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_sleep_action",
            ));
        }
    };
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &cycle_id,
            "memory.sleep.action",
            &format!("device:{device}"),
            json!({"action":request.action,"result":&result}),
        )
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(result))
}

pub(crate) async fn morning_report(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let report = state
        .sleep_memory
        .morning_report(&state.primary_owner_id, None)
        .map_err(sleep_error)?;
    let doctrine = if state.doctrine_flags.enabled {
        Some(
            state
                .doctrine
                .status(&state.primary_owner_id)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
        )
    } else {
        None
    };
    Ok(Json(json!({"report":report,"doctrine":doctrine})))
}

pub(crate) async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let memories = state
        .sleep_memory
        .search(
            &state.primary_owner_id,
            &query.q,
            query.include_dreams.unwrap_or(false),
            query.limit.unwrap_or(20),
        )
        .map_err(sleep_error)?;
    Ok(Json(json!({"memories":memories})))
}

fn sleep_error(error: SleepError) -> super::error::ApiError {
    match error {
        SleepError::CycleNotFound => api_error(StatusCode::NOT_FOUND, "sleep_cycle_not_found"),
        SleepError::InvalidState | SleepError::InvalidProposal(_) => {
            api_error(StatusCode::CONFLICT, error.to_string())
        }
        other => api_error(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_scheduler_route_requires_exact_token() {
        let mut headers = HeaderMap::new();
        assert!(authorize_internal(Some("secret"), &headers).is_err());
        assert!(authorize_internal(None, &headers).is_err());
        headers.insert("x-voiceos-internal-token", "wrong".parse().unwrap());
        assert!(authorize_internal(Some("secret"), &headers).is_err());
        headers.insert("x-voiceos-internal-token", "secret".parse().unwrap());
        assert!(authorize_internal(Some("secret"), &headers).is_ok());
    }

    #[test]
    fn api_requests_reject_unknown_fields() {
        assert!(
            serde_json::from_str::<RunRequest>(r#"{"mode":"dry_run","extra":"smuggle"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<ActionRequest>(r#"{"action":"commit","tool":"shell"}"#).is_err()
        );
    }
}
