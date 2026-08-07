use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use voiceos_core::AgentRunProgressUpdate;

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct AgentRunQuery {
    task_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct AgentEventQuery {
    after: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateAgentRunRequest {
    task_id: Option<String>,
    objective: String,
    role: Option<String>,
    sandbox: Option<String>,
    idempotency_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProgressRequest {
    event_kind: String,
    activity: String,
    #[serde(default)]
    evidence: Value,
    codex_thread_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResultRequest {
    status: String,
    result_summary: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChildRequest {
    idempotency_key: String,
    role: String,
    objective: String,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AgentRunQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let runs = state
        .store
        .agent_runs(
            &state.primary_owner_id,
            query.task_id.as_deref(),
            query.limit.unwrap_or(100),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"runs": runs})))
}

pub(crate) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AgentEventQuery>,
) -> ApiResult<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>> {
    authenticate(&state, &headers)?;
    let header_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let mut cursor = query.after.or(header_cursor).unwrap_or(0).max(0);
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let stream = async_stream::stream! {
        loop {
            let read_store = store.clone();
            let read_owner = owner.clone();
            match tokio::task::spawn_blocking(move || read_store.agent_events_after(&read_owner, cursor, 100)).await {
                Ok(Ok(events)) if !events.is_empty() => for event in events {
                    cursor = event.id;
                    let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_owned());
                    yield Ok(Event::default().event(event.event_type).id(cursor.to_string()).data(data));
                },
                Ok(Ok(_)) => tokio::time::sleep(Duration::from_millis(400)).await,
                _ => {
                    yield Ok(Event::default().event("agent.error").data("{\"error\":\"agent_stream_unavailable\"}"));
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateAgentRunRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let role = request.role.as_deref().unwrap_or("coordinator");
    let sandbox = request.sandbox.as_deref().unwrap_or("read-only");
    let capabilities = match sandbox {
        "read-only" => json!(["repo.read", "tests.inspect"]),
        "workspace-write" => json!(["repo.read", "repo.write", "tests.run"]),
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "invalid_agent_sandbox")),
    };
    let idempotency_key = request
        .idempotency_key
        .unwrap_or_else(|| format!("agent-request:{}", Uuid::new_v4()));
    let run = state
        .store
        .create_agent_run(
            &state.primary_owner_id,
            request.task_id.as_deref(),
            None,
            &idempotency_key,
            role,
            &request.objective,
            "gpt-5.6-sol",
            "high",
            sandbox,
            capabilities,
            &format!("device:{device_id}"),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok((StatusCode::ACCEPTED, Json(json!({"run": run}))))
}

pub(crate) async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let run = state
        .store
        .agent_run(&state.primary_owner_id, &run_id)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "agent_run_not_found"))?;
    let events = state
        .store
        .execution_events(&state.primary_owner_id, &run_id, 500)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let children = state
        .store
        .agent_runs(&state.primary_owner_id, run.task_id.as_deref(), 500)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .into_iter()
        .filter(|candidate| candidate.parent_run_id.as_deref() == Some(run_id.as_str()))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"run": run, "children": children, "events": events}),
    ))
}

pub(crate) async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let current = state
        .store
        .agent_run(&state.primary_owner_id, &run_id)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "agent_run_not_found"))?;
    if matches!(
        current.status.as_str(),
        "completed" | "failed" | "cancelled"
    ) {
        return Err(api_error(StatusCode::CONFLICT, "agent_run_is_terminal"));
    }
    let run = state
        .store
        .transition_agent_run(
            &state.primary_owner_id,
            &run_id,
            &current.status,
            "cancelled",
            &format!("device:{device_id}"),
            Some("Cancellation requested by user"),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "agent_run_changed"))?;
    Ok(Json(json!({"run": run})))
}

pub(crate) async fn claim(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authorize_supervisor(&state, &headers)?;
    let run = state
        .store
        .claim_next_agent_run(&state.primary_owner_id, "codex-supervisor")
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"claimed":run.is_some(),"run":run})))
}

pub(crate) async fn progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(request): Json<ProgressRequest>,
) -> ApiResult<Json<Value>> {
    authorize_supervisor(&state, &headers)?;
    if request.event_kind == "agent.run.running" {
        let current = state
            .store
            .agent_run(&state.primary_owner_id, &run_id)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "agent_run_not_found"))?;
        if current.status == "queued" {
            state
                .store
                .transition_agent_run(
                    &state.primary_owner_id,
                    &run_id,
                    "queued",
                    "starting",
                    "codex-supervisor",
                    Some("Codex subagent observed"),
                )
                .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::CONFLICT, "agent_run_changed"))?;
        } else if current.status == "running" {
            let run = state
                .store
                .update_agent_run_progress(
                    &state.primary_owner_id,
                    &run_id,
                    "codex-supervisor",
                    AgentRunProgressUpdate {
                        event_kind: request.event_kind,
                        activity: request.activity,
                        evidence: request.evidence,
                        codex_thread_id: request.codex_thread_id,
                    },
                )
                .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
            return Ok(Json(json!({"run":run})));
        }
        let run = state
            .store
            .transition_agent_run(
                &state.primary_owner_id,
                &run_id,
                "starting",
                "running",
                "codex-supervisor",
                Some(&request.activity),
            )
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
            .ok_or_else(|| api_error(StatusCode::CONFLICT, "agent_run_not_starting"))?;
        return Ok(Json(json!({"run":run})));
    }
    let run = state
        .store
        .update_agent_run_progress(
            &state.primary_owner_id,
            &run_id,
            "codex-supervisor",
            AgentRunProgressUpdate {
                event_kind: request.event_kind,
                activity: request.activity,
                evidence: request.evidence,
                codex_thread_id: request.codex_thread_id,
            },
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"run":run})))
}

pub(crate) async fn result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(request): Json<ResultRequest>,
) -> ApiResult<Json<Value>> {
    authorize_supervisor(&state, &headers)?;
    let run = state
        .store
        .finish_agent_run(
            &state.primary_owner_id,
            &run_id,
            "codex-supervisor",
            &request.status,
            request.result_summary.as_deref(),
            request.error.as_deref(),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "agent_run_not_active"))?;
    Ok(Json(json!({"run":run})))
}

pub(crate) async fn create_child(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(request): Json<ChildRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authorize_supervisor(&state, &headers)?;
    let parent = state
        .store
        .agent_run(&state.primary_owner_id, &run_id)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "parent_agent_run_not_found"))?;
    let run = state
        .store
        .create_agent_run(
            &state.primary_owner_id,
            parent.task_id.as_deref(),
            Some(&parent.id),
            &request.idempotency_key,
            &request.role,
            &request.objective,
            &parent.model,
            &parent.reasoning_effort,
            &parent.sandbox,
            parent.capability_scope.clone(),
            "codex-supervisor",
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"run":run}))))
}

fn authorize_supervisor(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let expected = state.internal_token.as_deref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "internal_auth_not_configured",
        )
    })?;
    let supplied = headers
        .get("x-voiceos-internal-token")
        .and_then(|value| value.to_str().ok());
    if supplied != Some(expected) {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "invalid_internal_token",
        ));
    }
    Ok(())
}
