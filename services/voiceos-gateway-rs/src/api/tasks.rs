use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::{TaskRecord, begin_task_initiative};
use voiceos_ontology::{CanonicalRequest, DecisionStatus, normalize_phrase};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct TaskQuery {
    include_completed: Option<bool>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct CreateTaskRequest {
    title: String,
    observable_outcome: String,
    estimated_minutes: u32,
    project_id: Option<String>,
    parent_task_id: Option<String>,
    due_at: Option<String>,
    importance: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct UpdateTaskStatusRequest {
    status: String,
}

#[derive(Deserialize)]
pub(crate) struct AssignTaskProjectRequest {
    project_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetTaskAttentionRequest {
    due_at: Option<String>,
    importance: String,
}

#[derive(Deserialize)]
pub(crate) struct TaskActionRequest {
    action: String,
    #[serde(default)]
    task_id: String,
    step_id: Option<String>,
    blocker_id: Option<String>,
    handoff_id: Option<String>,
    title: Option<String>,
    owner: Option<String>,
    status: Option<String>,
    from_owner: Option<String>,
    to_owner: Option<String>,
    kind: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    uri: Option<String>,
    #[serde(default)]
    evidence: Value,
}

#[derive(Deserialize)]
pub(crate) struct VoiceTaskCommandRequest {
    device_id: String,
    text: String,
}

#[derive(Deserialize)]
pub(crate) struct InitiativeResultRequest {
    job_id: String,
    status: String,
    response_text: String,
    provider: String,
    #[serde(default)]
    approvals: Vec<Value>,
    #[serde(default)]
    results: Vec<Value>,
    #[serde(default)]
    errors: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubagentTaskRequest {
    worker_id: String,
    status: String,
    session_id: Option<String>,
    title: Option<String>,
    observable_outcome: Option<String>,
    estimated_minutes: Option<u32>,
    importance: Option<String>,
    summary: Option<String>,
}

pub(crate) async fn list_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TaskQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let tasks = state
        .store
        .tasks(
            &state.primary_owner_id,
            query.include_completed.unwrap_or(false),
            query.limit.unwrap_or(50),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let details = tasks
        .iter()
        .filter_map(|task| {
            state
                .store
                .task_detail(&state.primary_owner_id, &task.id)
                .ok()
                .flatten()
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"tasks": tasks, "details": details})))
}

pub(crate) async fn task_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    state
        .store
        .task_detail(&state.primary_owner_id, &task_id)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .map(|detail| Json(json!({"detail": detail})))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))
}

pub(crate) async fn create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateTaskRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let mut task = state
        .store
        .create_task(
            &state.primary_owner_id,
            request.project_id.as_deref(),
            request.parent_task_id.as_deref(),
            &request.title,
            &request.observable_outcome,
            request.estimated_minutes,
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    if request.due_at.is_some() || request.importance.is_some() {
        task = state
            .store
            .set_task_attention_as(
                &state.primary_owner_id,
                &task.id,
                request.due_at.as_deref(),
                request.importance.as_deref().unwrap_or("normal"),
                &format!("device:{device_id}"),
            )
            .map_err(store_error)?
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))?;
    }
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            &task.id,
            "task.created",
            &format!("device:{device_id}"),
            json!({"title": task.title, "estimated_minutes": task.estimated_minutes}),
        )
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let initiative = begin_task_initiative(
        state.store.as_ref(),
        &state.primary_owner_id,
        &task,
        &format!("device:{device_id}"),
    )
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"task": task, "initiative": initiative})),
    ))
}

pub(crate) async fn sync_subagent_task(
    State(state): State<AppState>,
    Json(request): Json<SubagentTaskRequest>,
) -> ApiResult<Json<Value>> {
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let worker_id = request.worker_id.trim().to_owned();
    if worker_id.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "worker_id_required"));
    }
    let response = tokio::task::spawn_blocking(move || match request.status.as_str() {
        "running" => {
            let title = request
                .title
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("VIC delegated background work");
            let outcome = request
                .observable_outcome
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("A verified subagent report is returned to VIC and the originating conversation.");
            let (task, job, created) = store.start_subagent_task(
                &owner,
                &worker_id,
                request.session_id.as_deref(),
                title,
                outcome,
                request.estimated_minutes.unwrap_or(30),
                request.importance.as_deref().unwrap_or("normal"),
                "provider:hermes",
            )?;
            let detail = store.task_detail(&owner, &task.id)?.expect("subagent task exists");
            Ok::<_, voiceos_core::StoreError>(json!({
                "created": created,
                "task": task,
                "job": job,
                "detail": detail,
            }))
        }
        "completed" | "failed" => {
            let summary = request
                .summary
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(if request.status == "completed" {
                    "Hermes returned its delegated report to VIC."
                } else {
                    "Hermes could not complete the delegated work."
                });
            let Some((task, job)) = store.finish_subagent_task(
                &owner,
                &worker_id,
                &request.status,
                summary,
                "provider:hermes",
            )? else {
                return Ok(json!({"found": false}));
            };
            let detail = store.task_detail(&owner, &task.id)?.expect("subagent task exists");
            Ok(json!({
                "found": true,
                "task": task,
                "job": job,
                "detail": detail,
            }))
        }
        _ => Err(voiceos_core::StoreError::InvalidInput(
            "subagent status must be running, completed, or failed".to_owned(),
        )),
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(store_error)?;
    if response.get("found") == Some(&Value::Bool(false)) {
        return Err(api_error(StatusCode::NOT_FOUND, "subagent_task_not_found"));
    }
    Ok(Json(response))
}

pub(crate) async fn update_task_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<UpdateTaskStatusRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let task = state
        .store
        .update_task_status_as(
            &state.primary_owner_id,
            &task_id,
            &request.status,
            &format!("device:{device_id}"),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))?;
    Ok(Json(json!({"task": task})))
}

pub(crate) async fn assign_task_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<AssignTaskProjectRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let task = state
        .store
        .assign_task_project_as(
            &state.primary_owner_id,
            &task_id,
            request.project_id.as_deref(),
            &format!("device:{device_id}"),
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))?;
    Ok(Json(json!({"task": task})))
}

pub(crate) async fn set_task_attention(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<SetTaskAttentionRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let task = state
        .store
        .set_task_attention_as(
            &state.primary_owner_id,
            &task_id,
            request.due_at.as_deref(),
            &request.importance,
            &format!("device:{device_id}"),
        )
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))?;
    Ok(Json(json!({"task": task})))
}

pub(crate) async fn task_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(mut request): Json<TaskActionRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    request.task_id = task_id;
    run_task_action(&state, request, &format!("device:{device_id}"))
}

pub(crate) async fn internal_task_action(
    State(state): State<AppState>,
    Json(request): Json<TaskActionRequest>,
) -> ApiResult<Json<Value>> {
    run_task_action(&state, request, "provider:vic")
}

fn run_task_action(
    state: &AppState,
    request: TaskActionRequest,
    actor: &str,
) -> ApiResult<Json<Value>> {
    if request.task_id.trim().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "task_id_required"));
    }
    let owner_id = &state.primary_owner_id;
    let task_id = request.task_id.trim();
    let value = match request.action.as_str() {
        "step.create" => json!({"step": state.store.create_task_step(
            owner_id, task_id, required(&request.title, "title")?,
            request.owner.as_deref().unwrap_or("shared"), actor,
        ).map_err(store_error)?}),
        "step.update" => json!({"step": state.store.update_task_step(
            owner_id, task_id, required(&request.step_id, "step_id")?,
            required(&request.status, "status")?, request.owner.as_deref(),
            object_or_empty(request.evidence), actor,
        ).map_err(store_error)?.ok_or_else(|| api_error(StatusCode::NOT_FOUND, "step_not_found"))?}),
        "step.advance" => {
            let detail = state
                .store
                .advance_task_step(
                    owner_id,
                    task_id,
                    required(&request.step_id, "step_id")?,
                    request
                        .summary
                        .as_deref()
                        .unwrap_or("Stage completed from the task board"),
                    object_or_empty(request.evidence),
                    actor,
                )
                .map_err(store_error)?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "step_not_found"))?;
            return Ok(Json(
                json!({"action": request.action, "result": {"advanced": true}, "detail": detail}),
            ));
        }
        "blocker.create" => json!({"blocker": state.store.create_task_blocker(
            owner_id, task_id, required(&request.description, "description")?,
            request.owner.as_deref().unwrap_or("shared"), actor,
        ).map_err(store_error)?}),
        "blocker.resolve" => json!({"blocker": state.store.resolve_task_blocker(
            owner_id, task_id, required(&request.blocker_id, "blocker_id")?, actor,
        ).map_err(store_error)?.ok_or_else(|| api_error(StatusCode::NOT_FOUND, "blocker_not_found"))?}),
        "handoff.create" | "review.request" => json!({"handoff": state.store.create_task_handoff(
            owner_id, task_id,
            request.from_owner.as_deref().unwrap_or("vic"),
            request.to_owner.as_deref().unwrap_or("user"),
            if request.action == "review.request" { "review" } else { request.kind.as_deref().unwrap_or("handoff") },
            required(&request.summary, "summary")?, actor,
        ).map_err(store_error)?}),
        "handoff.update" => json!({"handoff": state.store.update_task_handoff(
            owner_id, task_id, required(&request.handoff_id, "handoff_id")?,
            required(&request.status, "status")?, actor,
        ).map_err(store_error)?.ok_or_else(|| api_error(StatusCode::NOT_FOUND, "handoff_not_found"))?}),
        "artifact.attach" => json!({"artifact": state.store.attach_task_artifact(
            owner_id, task_id, request.kind.as_deref().unwrap_or("reference"),
            required(&request.uri, "uri")?, required(&request.description, "description")?,
            request.owner.as_deref().unwrap_or("vic"), actor,
        ).map_err(store_error)?}),
        "progress.record" => {
            state
                .store
                .record_task_progress(
                    owner_id,
                    task_id,
                    required(&request.summary, "summary")?,
                    object_or_empty(request.evidence),
                    actor,
                )
                .map_err(store_error)?;
            json!({"recorded": true})
        }
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_task_action",
            ));
        }
    };
    let detail = state
        .store
        .task_detail(owner_id, task_id)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))?;
    Ok(Json(
        json!({"action": request.action, "result": value, "detail": detail}),
    ))
}

fn required<'a>(value: &'a Option<String>, name: &str) -> ApiResult<&'a str> {
    value
        .as_deref()
        .filter(|item| !item.trim().is_empty())
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("{name}_required")))
}

fn object_or_empty(value: Value) -> Value {
    if value.is_object() { value } else { json!({}) }
}

fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    api_error(StatusCode::BAD_REQUEST, error.to_string())
}

pub(crate) async fn voice_command(
    State(state): State<AppState>,
    Json(request): Json<VoiceTaskCommandRequest>,
) -> ApiResult<Json<Value>> {
    let phrase = request.text.trim().to_owned();
    let device_id = request.device_id.trim().to_owned();
    if phrase.is_empty() || device_id.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "device_id_and_text_required",
        ));
    }
    if !looks_like_task_command(&phrase) {
        return Ok(Json(json!({"handled": false})));
    }

    let ontology = state.ontology.clone();
    let ontology_owner = device_id.clone();
    let ontology_phrase = phrase.clone();
    let decision =
        tokio::task::spawn_blocking(move || ontology.interpret(&ontology_owner, &ontology_phrase))
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;

    let Some(command) = decision.interpretation else {
        return Ok(Json(model_fallback(None)));
    };
    if !command.intent.0.starts_with("task.") {
        return Ok(Json(json!({"handled": false})));
    }
    if decision.status != DecisionStatus::Resolved {
        return Ok(Json(model_fallback(Some(&command))));
    }

    let owner = state.primary_owner_id.clone();
    let actor = format!("voice-device:{device_id}");
    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        execute_voice_task_command(store.as_ref(), &owner, &actor, &command)
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    Ok(Json(result))
}

fn execute_voice_task_command(
    store: &voiceos_core::ConversationStore,
    owner: &str,
    actor: &str,
    command: &CanonicalRequest,
) -> Result<Value, String> {
    match command.intent.0.as_str() {
        "task.create" => {
            let raw_title = string_argument(command, "title").unwrap_or_default();
            if raw_title.trim().is_empty() {
                return Ok(clarification(
                    "What task would you like me to add?",
                    Some(command),
                ));
            }
            let title = sentence_case(raw_title.trim());
            let minutes = command
                .arguments
                .get("estimated_minutes")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(20)
                .clamp(1, 1_440);
            let outcome = string_argument(command, "observable_outcome")
                .filter(|value| !value.trim().is_empty())
                .map(|value| sentence_case(value.trim()))
                .unwrap_or_else(|| format!("Complete: {title}"));
            let open = store
                .tasks(owner, false, 200)
                .map_err(|error| error.to_string())?;
            if let Some(existing) = open
                .iter()
                .find(|task| normalize_phrase(&task.title) == normalize_phrase(&title))
            {
                return Ok(success(
                    command,
                    format!("That task is already on your list: {}.", existing.title),
                    false,
                    json!({"task": existing, "duplicate": true}),
                ));
            }
            let task = store
                .create_task(owner, None, None, &title, &outcome, minutes)
                .map_err(|error| error.to_string())?;
            store
                .append_execution_event(
                    owner,
                    &task.id,
                    "task.created",
                    actor,
                    json!({"source": "voice", "title": task.title, "estimated_minutes": task.estimated_minutes}),
                )
                .map_err(|error| error.to_string())?;
            let initiative = begin_task_initiative(store, owner, &task, actor)
                .map_err(|error| error.to_string())?;
            Ok(success(
                command,
                format!(
                    "Added {} to your task list as a {} minute task. VIC analyzed it and started safe preparation work.",
                    task.title, task.estimated_minutes
                ),
                true,
                json!({"task": task, "initiative": initiative}),
            ))
        }
        "task.list" => {
            let tasks = store
                .tasks(owner, false, 50)
                .map_err(|error| error.to_string())?;
            Ok(success(
                command,
                describe_tasks(&tasks),
                false,
                json!({"tasks": tasks}),
            ))
        }
        "task.review" => {
            let tasks = store
                .tasks(owner, false, 50)
                .map_err(|error| error.to_string())?;
            let recommended_task_ids = tasks
                .iter()
                .filter(|task| task.status != "blocked")
                .take(3)
                .map(|task| task.id.clone())
                .collect::<Vec<_>>();
            Ok(success(
                command,
                review_tasks(&tasks),
                false,
                json!({
                    "tasks": tasks,
                    "recommended_task_ids": recommended_task_ids,
                }),
            ))
        }
        "task.assist" => {
            let tasks = store
                .tasks(owner, false, 50)
                .map_err(|error| error.to_string())?;
            Ok(success(
                command,
                describe_task_assistance(&tasks),
                false,
                json!({
                    "tasks": tasks,
                    "supported_actions": [
                        "prioritize",
                        "plan_next_actions",
                        "identify_blockers",
                        "research_and_draft",
                        "track_status",
                    ],
                }),
            ))
        }
        "task.start" | "task.complete" => {
            let tasks = store
                .tasks(owner, false, 200)
                .map_err(|error| error.to_string())?;
            let reference = string_argument(command, "reference").unwrap_or("next");
            let task = match resolve_task_reference(&tasks, reference) {
                Ok(task) => task,
                Err(message) => return Ok(clarification(&message, Some(command))),
            };
            let (status, verb) = if command.intent.0 == "task.start" {
                ("active", "Started")
            } else {
                ("completed", "Completed")
            };
            let updated = store
                .update_task_status_as(owner, &task.id, status, actor)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "task_not_found".to_owned())?;
            Ok(success(
                command,
                format!("{verb} {}.", updated.title),
                true,
                json!({"task": updated}),
            ))
        }
        _ => Ok(json!({"handled": false})),
    }
}

pub(crate) async fn claim_initiative(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let claimed = tokio::task::spawn_blocking(move || {
        let task = store.task(&owner, &task_id)?;
        let job = store.initiative_job_for_task(&owner, &task_id)?;
        let claimed = match job {
            Some(job) if job.status == "approved" => {
                store.transition_job_status(&owner, &job.id, "approved", "running")?
            }
            _ => None,
        };
        Ok::<_, voiceos_core::StoreError>((task, claimed))
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    match claimed {
        (Some(task), Some(job)) => Ok(Json(json!({"claimed": true, "task": task, "job": job}))),
        (None, _) => Err(api_error(StatusCode::NOT_FOUND, "task_not_found")),
        _ => Ok(Json(json!({"claimed": false}))),
    }
}

pub(crate) async fn complete_initiative(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(request): Json<InitiativeResultRequest>,
) -> ApiResult<Json<Value>> {
    let next_status = match request.status.as_str() {
        "completed" | "paused" | "failed" => request.status.clone(),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "invalid_initiative_status",
            ));
        }
    };
    if request.response_text.trim().is_empty() || request.provider.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "response_text_and_provider_required",
        ));
    }
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let completed = tokio::task::spawn_blocking(move || {
        let existing = store.job(&owner, &request.job_id)?;
        if existing.as_ref().and_then(|job| job.task_id.as_deref()) != Some(task_id.as_str()) {
            return Ok::<_, voiceos_core::StoreError>(None);
        }
        let job = store.transition_job_status(&owner, &request.job_id, "running", &next_status)?;
        if job.is_none() {
            return Ok::<_, voiceos_core::StoreError>(None);
        }
        store.append_execution_event(
            &owner,
            &task_id,
            &format!("task.initiative.{next_status}"),
            &format!("provider:{}", request.provider),
            json!({
                "job_id": request.job_id,
                "response_text": request.response_text,
                "provider": request.provider,
                "approvals": request.approvals,
                "results": request.results,
                "errors": request.errors,
            }),
        )?;
        if next_status == "completed"
            && let Some(detail) = store.task_detail(&owner, &task_id)?
        {
            if let Some(step) = detail
                .steps
                .iter()
                .find(|step| step.owner == "vic" && step.status != "completed")
            {
                store.update_task_step(
                    &owner,
                    &task_id,
                    &step.id,
                    "completed",
                    None,
                    json!({
                        "provider": request.provider,
                        "summary": request.response_text,
                        "job_id": request.job_id,
                    }),
                    &format!("provider:{}", request.provider),
                )?;
            }
            store.create_task_handoff(
                &owner,
                &task_id,
                "vic",
                "user",
                "review",
                &request.response_text,
                &format!("provider:{}", request.provider),
            )?;
        }
        Ok(job)
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    completed
        .map(|job| Json(json!({"recorded": true, "job": job})))
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "initiative_not_running"))
}

fn looks_like_task_command(phrase: &str) -> bool {
    let normalized = normalize_phrase(phrase);
    normalized.contains("task")
        || normalized.contains("to do")
        || normalized.contains("todo")
        || normalized.starts_with("remind me to ")
        || normalized.starts_with("i need to ")
        || normalized.contains("what should i work on")
        || normalized.contains("what should we work on")
        || normalized.contains("what do i need to work on")
        || normalized.contains("what do we need to work on")
        || normalized.contains("what needs to get done")
        || normalized.contains("tell me what to work on")
        || normalized.contains("tell me what we need to work on")
        || normalized.contains("tell me my priorities")
        || matches!(
            normalized.as_str(),
            "mark that done"
                | "mark it done"
                | "what is next on my list"
                | "what s next on my list"
        )
}

fn string_argument<'a>(command: &'a CanonicalRequest, name: &str) -> Option<&'a str> {
    command.arguments.get(name).and_then(Value::as_str)
}

fn sentence_case(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_uppercase().collect::<String>() + characters.as_str()
}

pub(crate) fn resolve_task_reference<'a>(
    tasks: &'a [TaskRecord],
    reference: &str,
) -> Result<&'a TaskRecord, String> {
    if tasks.is_empty() {
        return Err("You do not have any open tasks.".to_owned());
    }
    let normalized = normalize_phrase(reference);
    if normalized.is_empty() || matches!(normalized.as_str(), "next" | "that" | "it" | "current") {
        return tasks
            .iter()
            .find(|task| task.status == "active")
            .or_else(|| tasks.iter().find(|task| task.status == "ready"))
            .or_else(|| tasks.first())
            .ok_or_else(|| "You do not have any open tasks.".to_owned());
    }
    let matches = tasks
        .iter()
        .filter(|task| {
            let title = normalize_phrase(&task.title);
            title.contains(&normalized) || normalized.contains(&title)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [task] => Ok(*task),
        [] => Err(format!(
            "I could not find an open task matching {reference}. Ask me to list your tasks if you want to hear them."
        )),
        many => Err(format!(
            "I found more than one matching task: {}. Please say which one.",
            many.iter()
                .take(3)
                .map(|task| task.title.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn describe_tasks(tasks: &[TaskRecord]) -> String {
    if tasks.is_empty() {
        return "You do not have any open tasks.".to_owned();
    }
    let listed = tasks
        .iter()
        .take(5)
        .enumerate()
        .map(|(index, task)| format!("{}. {}", index + 1, task.title))
        .collect::<Vec<_>>()
        .join("; ");
    let remainder = tasks.len().saturating_sub(5);
    if remainder == 0 {
        let noun = if tasks.len() == 1 { "task" } else { "tasks" };
        format!("You have {} open {noun}: {listed}.", tasks.len())
    } else {
        format!(
            "You have {} open tasks. The first five are: {listed}; plus {remainder} more.",
            tasks.len()
        )
    }
}

fn review_tasks(tasks: &[TaskRecord]) -> String {
    if tasks.is_empty() {
        return "You do not have any open tasks, so there is nothing waiting for you right now."
            .to_owned();
    }

    let recommended = tasks
        .iter()
        .filter(|task| task.status != "blocked")
        .take(3)
        .collect::<Vec<_>>();
    let blocked = tasks.iter().filter(|task| task.status == "blocked").count();

    if recommended.is_empty() {
        return format!(
            "You have {} open tasks, but all of them are blocked. We should review those blockers first.",
            tasks.len()
        );
    }

    let priorities = recommended
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let action = if index == 0 && task.status == "active" {
                "continue"
            } else if index == 0 {
                "start"
            } else {
                "then"
            };
            format!(
                "{action} {} for about {} minutes",
                task.title, task.estimated_minutes
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let blocked_note = if blocked == 0 {
        String::new()
    } else {
        format!(" You also have {blocked} blocked tasks that need their blockers resolved.")
    };

    format!(
        "You have {} open tasks. Based on their current status, I recommend: {priorities}.{blocked_note}",
        tasks.len()
    )
}

fn describe_task_assistance(tasks: &[TaskRecord]) -> String {
    if tasks.is_empty() {
        return "Your task board is empty. I can capture new tasks, turn them into short next actions, and help you organize what to do first."
            .to_owned();
    }

    let first = tasks
        .iter()
        .find(|task| task.status == "active")
        .or_else(|| tasks.iter().find(|task| task.status == "ready"))
        .unwrap_or(&tasks[0]);
    let blocked = tasks.iter().filter(|task| task.status == "blocked").count();
    let blocked_note = if blocked == 0 {
        String::new()
    } else {
        format!(" I can also help identify what is blocking your {blocked} blocked tasks.")
    };

    format!(
        "I can help with your {} open tasks by prioritizing them, breaking them into twenty-minute next actions, identifying blockers, researching or drafting supporting material, and keeping their status current. The best place to begin is {}. It is estimated at {} minutes.{blocked_note} Say, start the next task, when you want me to mark it active.",
        tasks.len(),
        first.title,
        first.estimated_minutes
    )
}

fn success(
    command: &CanonicalRequest,
    response_text: String,
    tasks_changed: bool,
    result: Value,
) -> Value {
    json!({
        "handled": true,
        "response_text": response_text,
        "provider": "deterministic-task",
        "tool_calls": [{
            "name": command.intent.0,
            "arguments": command.arguments,
            "status": "completed"
        }],
        "approvals": [],
        "results": [result],
        "errors": [],
        "evidence": {"tasks_changed": tasks_changed}
    })
}

fn clarification(message: &str, command: Option<&CanonicalRequest>) -> Value {
    let tool_calls = command
        .map(|request| {
            vec![json!({
                "name": request.intent.0,
                "arguments": request.arguments,
                "status": "needs_confirmation"
            })]
        })
        .unwrap_or_default();
    json!({
        "handled": true,
        "response_text": message,
        "provider": "deterministic-task",
        "tool_calls": tool_calls,
        "approvals": [],
        "results": [],
        "errors": [],
        "evidence": {"tasks_changed": false, "needs_clarification": true}
    })
}

fn model_fallback(command: Option<&CanonicalRequest>) -> Value {
    json!({
        "handled": false,
        "task_candidate": true,
        "canonical_candidate": command.map(|request| json!({
            "intent": request.intent.0,
            "arguments": request.arguments,
            "confidence": request.confidence.score,
        })),
        "evidence": {
            "reason": "task_interpretation_unresolved",
            "fallback": "reasoning_provider"
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use voiceos_core::ConversationStore;
    use voiceos_ontology::{Confidence, IntentId, ResolutionSource};

    use super::{execute_voice_task_command, looks_like_task_command, model_fallback};

    fn command(
        intent: &str,
        arguments: BTreeMap<String, serde_json::Value>,
    ) -> super::CanonicalRequest {
        super::CanonicalRequest {
            intent: IntentId::from(intent),
            entities: vec![],
            arguments,
            confidence: Confidence::new(0.99),
            source: ResolutionSource::Deterministic,
        }
    }

    #[test]
    fn creates_deduplicates_starts_and_completes_voice_tasks() {
        let store = ConversationStore::in_memory().unwrap();
        let create = command(
            "task.create",
            BTreeMap::from([
                ("title".to_owned(), json!("call the dentist")),
                (
                    "observable_outcome".to_owned(),
                    json!("Complete: call the dentist"),
                ),
                ("estimated_minutes".to_owned(), json!(15)),
            ]),
        );
        let created =
            execute_voice_task_command(&store, "owner", "voice-device:pixel", &create).unwrap();
        assert_eq!(created["handled"], json!(true));
        assert_eq!(created["evidence"]["tasks_changed"], json!(true));
        assert_eq!(store.tasks("owner", false, 20).unwrap().len(), 1);

        let duplicate =
            execute_voice_task_command(&store, "owner", "voice-device:pixel", &create).unwrap();
        assert_eq!(duplicate["results"][0]["duplicate"], json!(true));
        assert_eq!(store.tasks("owner", false, 20).unwrap().len(), 1);

        let start = command(
            "task.start",
            BTreeMap::from([("reference".to_owned(), json!("next"))]),
        );
        execute_voice_task_command(&store, "owner", "voice-device:pixel", &start).unwrap();
        assert_eq!(store.tasks("owner", false, 20).unwrap()[0].status, "active");

        let complete = command(
            "task.complete",
            BTreeMap::from([("reference".to_owned(), json!("current"))]),
        );
        execute_voice_task_command(&store, "owner", "voice-device:pixel", &complete).unwrap();
        assert!(store.tasks("owner", false, 20).unwrap().is_empty());
    }

    #[test]
    fn identifies_task_language_without_intercepting_ordinary_conversation() {
        assert!(looks_like_task_command("Remind me to call the dentist"));
        assert!(looks_like_task_command(
            "Remind me that we need to work on printing all the recipe cards and laminating them as a task."
        ));
        assert!(looks_like_task_command("Mark that done"));
        assert!(looks_like_task_command(
            "Look at the task list and tell me what we need to work on"
        ));
        assert!(looks_like_task_command("What should we work on next?"));
        assert!(looks_like_task_command(
            "How can you help me with my task list?"
        ));
        assert!(!looks_like_task_command("Tell me about black holes"));
    }

    #[test]
    fn unresolved_task_language_falls_through_to_the_reasoning_provider() {
        let candidate = command(
            "task.assist",
            BTreeMap::from([("reference".to_owned(), json!("my list"))]),
        );
        let result = model_fallback(Some(&candidate));

        assert_eq!(result["handled"], json!(false));
        assert_eq!(result["task_candidate"], json!(true));
        assert_eq!(result["evidence"]["fallback"], json!("reasoning_provider"));
        assert_eq!(
            result["canonical_candidate"]["intent"],
            json!("task.assist")
        );
    }

    #[test]
    fn reviews_open_tasks_and_recommends_active_work_first() {
        let store = ConversationStore::in_memory().unwrap();
        let active = store
            .create_task(
                "owner",
                None,
                None,
                "Print recipe cards",
                "Recipe cards are printed",
                20,
            )
            .unwrap();
        store
            .create_task(
                "owner",
                None,
                None,
                "Laminate recipe cards",
                "Recipe cards are laminated",
                30,
            )
            .unwrap();
        store
            .update_task_status_as("owner", &active.id, "active", "test")
            .unwrap();

        let review = command("task.review", BTreeMap::new());
        let result =
            execute_voice_task_command(&store, "owner", "voice-device:pixel", &review).unwrap();

        assert_eq!(result["handled"], json!(true));
        assert_eq!(result["evidence"]["tasks_changed"], json!(false));
        assert!(
            result["response_text"]
                .as_str()
                .unwrap()
                .contains("continue Print recipe cards for about 20 minutes")
        );
        assert_eq!(
            result["results"][0]["recommended_task_ids"][0],
            json!(active.id)
        );

        let assistance = command("task.assist", BTreeMap::new());
        let assistance =
            execute_voice_task_command(&store, "owner", "voice-device:pixel", &assistance).unwrap();
        assert_eq!(assistance["handled"], json!(true));
        assert_eq!(assistance["evidence"]["tasks_changed"], json!(false));
        assert!(
            assistance["response_text"]
                .as_str()
                .unwrap()
                .contains("The best place to begin is Print recipe cards")
        );
    }
}
