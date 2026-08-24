use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::{FocusSessionRecord, FocusSnapshot};
use voiceos_ontology::{CanonicalRequest, DecisionStatus, normalize_phrase};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct FocusQuery {
    mode: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartFocusRequest {
    task_id: Option<String>,
    mode: Option<String>,
    planned_minutes: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FocusActionRequest {
    action: String,
    note: Option<String>,
    restart_action: Option<String>,
    reflection: Option<String>,
    planned_minutes: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SwitchFocusRequest {
    task_id: String,
    planned_minutes: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttentionCaptureRequest {
    title: String,
    details: Option<String>,
    estimated_minutes: Option<u32>,
    due_at: Option<String>,
    importance: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct VoiceFocusCommandRequest {
    device_id: String,
    text: String,
}

pub(crate) async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FocusQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let mode = query.mode.as_deref().unwrap_or("normal");
    let snapshot = state
        .store
        .focus_snapshot(&state.primary_owner_id, mode)
        .map_err(store_error)?;
    Ok(Json(json!({"focus": snapshot})))
}

pub(crate) async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StartFocusRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let mode = request.mode.as_deref().unwrap_or("normal");
    let minutes = request
        .planned_minutes
        .unwrap_or(if mode == "five_minute" { 5 } else { 20 });
    let task_id = match request
        .task_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        Some(task_id) => task_id.to_owned(),
        None => recommended_task_id(&state, mode)?,
    };
    let session = state
        .store
        .start_focus_session(
            &state.primary_owner_id,
            &task_id,
            mode,
            minutes,
            &format!("device:{device_id}"),
        )
        .map_err(store_error)?;
    let snapshot = state
        .store
        .focus_snapshot(&state.primary_owner_id, mode)
        .map_err(store_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"session": session, "focus": snapshot})),
    ))
}

pub(crate) async fn act(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<FocusActionRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let actor = format!("device:{device_id}");
    let session = run_action(&state, &session_id, request, &actor)?;
    let snapshot = state
        .store
        .focus_snapshot(&state.primary_owner_id, "normal")
        .map_err(store_error)?;
    Ok(Json(json!({"session": session, "focus": snapshot})))
}

pub(crate) async fn switch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SwitchFocusRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    Ok(Json(switch_to_task(
        &state,
        &request.task_id,
        request.planned_minutes.unwrap_or(5),
        &format!("device:{device_id}"),
    )?))
}

pub(crate) async fn capture(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AttentionCaptureRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let result = capture_idea(
        &state,
        &request.title,
        request.details.as_deref(),
        request.estimated_minutes.unwrap_or(10),
        request.due_at.as_deref(),
        request.importance.as_deref().unwrap_or("normal"),
        &format!("device:{device_id}"),
    )?;
    Ok((StatusCode::CREATED, Json(result)))
}

pub(crate) async fn voice_command(
    State(state): State<AppState>,
    Json(request): Json<VoiceFocusCommandRequest>,
) -> ApiResult<Json<Value>> {
    let phrase = request.text.trim().to_owned();
    let device_id = request.device_id.trim().to_owned();
    if phrase.is_empty() || device_id.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "device_id_and_text_required",
        ));
    }
    if !looks_like_focus_command(&phrase) {
        return Ok(Json(json!({"handled": false})));
    }
    let ontology = state.ontology.clone();
    let ontology_owner = device_id.clone();
    let decision =
        tokio::task::spawn_blocking(move || ontology.interpret(&ontology_owner, &phrase))
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let Some(command) = decision.interpretation else {
        return Ok(Json(json!({"handled": false})));
    };
    if decision.status != DecisionStatus::Resolved || !command.intent.0.starts_with("focus.") {
        return Ok(Json(json!({"handled": false})));
    }
    execute_voice_command(&state, &device_id, &command)
}

fn execute_voice_command(
    state: &AppState,
    device_id: &str,
    command: &CanonicalRequest,
) -> ApiResult<Json<Value>> {
    let owner = &state.primary_owner_id;
    let actor = format!("voice-device:{device_id}");
    match command.intent.0.as_str() {
        "focus.next" => {
            let mode = string_argument(command, "mode").unwrap_or("normal");
            let snapshot = state
                .store
                .focus_snapshot(owner, mode)
                .map_err(store_error)?;
            let response = recommendation_response(&snapshot);
            Ok(voice_result(
                &command.intent.0,
                response,
                json!({"focus": snapshot}),
            ))
        }
        "focus.start" => {
            let minutes = number_argument(command, "minutes")
                .unwrap_or(20)
                .clamp(1, 120);
            let mode = if minutes <= 5 {
                "five_minute"
            } else {
                "normal"
            };
            let task_id = recommended_task_id(state, mode)?;
            let session = state
                .store
                .start_focus_session(owner, &task_id, mode, minutes, &actor)
                .map_err(store_error)?;
            Ok(voice_result(
                &command.intent.0,
                format!(
                    "For the next {minutes} minutes, only do this: {}",
                    session.next_action
                ),
                json!({"session": session}),
            ))
        }
        "focus.interrupt" => {
            let Some(active) = state
                .store
                .active_focus_session(owner)
                .map_err(store_error)?
            else {
                return Ok(voice_result(
                    &command.intent.0,
                    "There is no active focus session. Nothing was lost.".to_owned(),
                    json!({"session": null}),
                ));
            };
            let session = state
                .store
                .interrupt_focus_session(owner, &active.id, "Voice interruption", None, &actor)
                .map_err(store_error)?;
            let restart = session
                .restart_action
                .clone()
                .unwrap_or_else(|| session.next_action.clone());
            Ok(voice_result(
                &command.intent.0,
                format!("Saved your place. When you return: {restart}"),
                json!({"session": session}),
            ))
        }
        "focus.restart" => {
            let Some(interrupted) = state
                .store
                .last_interrupted_focus_session(owner)
                .map_err(store_error)?
            else {
                return Ok(voice_result(
                    &command.intent.0,
                    "There is no saved interruption to restart. Ask what to do now for one next action."
                        .to_owned(),
                    json!({"session": null}),
                ));
            };
            let session = state
                .store
                .resume_focus_session(owner, &interrupted.id, 5, &actor)
                .map_err(store_error)?;
            Ok(voice_result(
                &command.intent.0,
                format!("Welcome back. Only do this: {}", session.next_action),
                json!({"session": session}),
            ))
        }
        "focus.complete" => {
            let Some(active) = state
                .store
                .active_focus_session(owner)
                .map_err(store_error)?
            else {
                return Ok(voice_result(
                    &command.intent.0,
                    "There is no active focus session to end.".to_owned(),
                    json!({"session": null}),
                ));
            };
            let session = state
                .store
                .complete_focus_session(owner, &active.id, Some("Ended by voice"), None, &actor)
                .map_err(store_error)?;
            Ok(voice_result(
                &command.intent.0,
                "Focus session saved. I did not mark the whole task complete.".to_owned(),
                json!({"session": session}),
            ))
        }
        "focus.capture" => {
            let title = string_argument(command, "title").unwrap_or_default();
            if title.trim().is_empty() {
                return Ok(voice_result(
                    &command.intent.0,
                    "What idea should I park without changing your focus?".to_owned(),
                    json!({"captured": false}),
                ));
            }
            let result = capture_idea(state, title, None, 10, None, "normal", &actor)?;
            Ok(voice_result(
                &command.intent.0,
                format!("Parked {title}. Your current focus did not change."),
                result,
            ))
        }
        "focus.switch" => {
            let reference = string_argument(command, "reference").unwrap_or_default();
            let tasks = state
                .store
                .tasks(owner, false, 200)
                .map_err(store_error)?
                .into_iter()
                .filter(|task| matches!(task.status.as_str(), "active" | "ready"))
                .collect::<Vec<_>>();
            let task = super::tasks::resolve_task_reference(&tasks, reference)
                .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
            let switched = switch_to_task(state, &task.id, 5, &actor)?;
            let response = switched
                .get("response_text")
                .and_then(Value::as_str)
                .unwrap_or("Focus switched and the previous restart point was saved.")
                .to_owned();
            Ok(voice_result(&command.intent.0, response, switched))
        }
        _ => Ok(Json(json!({"handled": false}))),
    }
}

fn switch_to_task(
    state: &AppState,
    task_id: &str,
    planned_minutes: u32,
    actor: &str,
) -> ApiResult<Value> {
    let owner = &state.primary_owner_id;
    let mut previous_saved = false;
    if let Some(active) = state
        .store
        .active_focus_session(owner)
        .map_err(store_error)?
    {
        if active.task_id == task_id {
            let snapshot = state
                .store
                .focus_snapshot(owner, "normal")
                .map_err(store_error)?;
            return Ok(json!({
                "response_text": format!("You are already focused here: {}", active.next_action),
                "session": active,
                "focus": snapshot,
                "previous_saved": false
            }));
        }
        state
            .store
            .interrupt_focus_session(
                owner,
                &active.id,
                "Deliberate focus switch",
                Some(&active.next_action),
                actor,
            )
            .map_err(store_error)?;
        previous_saved = true;
    }
    let session = state
        .store
        .start_focus_session(
            owner,
            task_id,
            "normal",
            planned_minutes.clamp(1, 120),
            actor,
        )
        .map_err(store_error)?;
    let snapshot = state
        .store
        .focus_snapshot(owner, "normal")
        .map_err(store_error)?;
    let response_text = if previous_saved {
        format!(
            "Saved the old restart point. Only do this now: {}",
            session.next_action
        )
    } else {
        format!("Only do this now: {}", session.next_action)
    };
    Ok(json!({
        "response_text": response_text,
        "session": session,
        "focus": snapshot,
        "previous_saved": previous_saved
    }))
}

fn capture_idea(
    state: &AppState,
    title: &str,
    details: Option<&str>,
    estimated_minutes: u32,
    due_at: Option<&str>,
    importance: &str,
    actor: &str,
) -> ApiResult<Value> {
    let title = title.trim();
    if title.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "capture_title_required"));
    }
    let owner = &state.primary_owner_id;
    let projects = state.store.projects(owner, 200).map_err(store_error)?;
    let parking_title = "Idea Parking Lot";
    let project = match projects
        .into_iter()
        .find(|project| normalize_phrase(&project.title) == normalize_phrase(parking_title))
    {
        Some(project) => project,
        None => state
            .store
            .create_project(owner, None, parking_title)
            .map_err(store_error)?,
    };
    let open = state.store.tasks(owner, false, 200).map_err(store_error)?;
    if let Some(existing) = open
        .iter()
        .find(|task| normalize_phrase(&task.title) == normalize_phrase(title))
    {
        let snapshot = state
            .store
            .focus_snapshot(owner, "normal")
            .map_err(store_error)?;
        return Ok(
            json!({"task": existing, "project": project, "focus": snapshot, "duplicate": true}),
        );
    }
    let outcome = details
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Decide whether and when to act on: {title}"));
    let task = state
        .store
        .create_task(
            owner,
            Some(&project.id),
            None,
            title,
            &outcome,
            estimated_minutes.clamp(1, 1_440),
        )
        .map_err(store_error)?;
    let task = state
        .store
        .set_task_attention_as(owner, &task.id, due_at, importance, actor)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))?;
    let task = state
        .store
        .update_task_status_as(owner, &task.id, "proposed", actor)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "task_not_found"))?;
    state
        .store
        .append_execution_event(
            owner,
            &task.id,
            "attention.captured",
            actor,
            json!({"title": task.title, "project_id": project.id, "focus_changed": false}),
        )
        .map_err(store_error)?;
    let snapshot = state
        .store
        .focus_snapshot(owner, "normal")
        .map_err(store_error)?;
    Ok(json!({"task": task, "project": project, "focus": snapshot, "duplicate": false}))
}

fn run_action(
    state: &AppState,
    session_id: &str,
    request: FocusActionRequest,
    actor: &str,
) -> ApiResult<FocusSessionRecord> {
    match request.action.as_str() {
        "interrupt" => state.store.interrupt_focus_session(
            &state.primary_owner_id,
            session_id,
            request.note.as_deref().unwrap_or("Interrupted"),
            request.restart_action.as_deref(),
            actor,
        ),
        "resume" => state.store.resume_focus_session(
            &state.primary_owner_id,
            session_id,
            request.planned_minutes.unwrap_or(5),
            actor,
        ),
        "complete" => state.store.complete_focus_session(
            &state.primary_owner_id,
            session_id,
            request.reflection.as_deref(),
            request.restart_action.as_deref(),
            actor,
        ),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_focus_action",
            ));
        }
    }
    .map_err(store_error)
}

fn recommended_task_id(state: &AppState, mode: &str) -> ApiResult<String> {
    state
        .store
        .focus_snapshot(&state.primary_owner_id, mode)
        .map_err(store_error)?
        .recommendation
        .map(|priority| priority.task_id)
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "no_focus_action_available"))
}

fn recommendation_response(snapshot: &FocusSnapshot) -> String {
    snapshot
        .recommendation
        .as_ref()
        .map(|priority| format!("Only do this next: {}", priority.next_action))
        .unwrap_or_else(|| "You have no ready task that needs your attention right now.".to_owned())
}

fn voice_result(tool: &str, response_text: String, result: Value) -> Json<Value> {
    Json(json!({
        "handled": true,
        "response_text": response_text,
        "provider": "deterministic-focus",
        "tool_calls": [{"name": tool, "status": "completed"}],
        "approvals": [],
        "results": [result],
        "errors": [],
        "evidence": {"focus_state_changed": tool != "focus.next"},
    }))
}

fn string_argument<'a>(command: &'a CanonicalRequest, name: &str) -> Option<&'a str> {
    command.arguments.get(name).and_then(Value::as_str)
}

fn number_argument(command: &CanonicalRequest, name: &str) -> Option<u32> {
    command
        .arguments
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn looks_like_focus_command(phrase: &str) -> bool {
    let phrase = voiceos_ontology::normalize_phrase(phrase);
    (phrase.starts_with("work on ") && phrase.ends_with(" instead"))
        || [
            "focus",
            "overwhelmed",
            "next action",
            "what should i do now",
            "next thing",
            "one thing",
            "low energy",
            "five minute version",
            "5 minute version",
            "got interrupted",
            "was interrupted",
            "where was i",
            "done for now",
            "restart point",
            "pick up where i left off",
            "park this",
            "capture this",
            "parking lot",
            "don t let me forget",
            "switch focus",
        ]
        .iter()
        .any(|candidate| phrase.contains(candidate))
}

fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    api_error(StatusCode::BAD_REQUEST, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_prefilter_does_not_capture_unrelated_conversation() {
        assert!(looks_like_focus_command("I am overwhelmed"));
        assert!(looks_like_focus_command(
            "Start a five minute focus session"
        ));
        assert!(looks_like_focus_command("Where was I?"));
        assert!(looks_like_focus_command("Work on the tax return instead"));
        assert!(!looks_like_focus_command("Tell me about the weather"));
        assert!(!looks_like_focus_command("Create a task to call Sam"));
    }
}
