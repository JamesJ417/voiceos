use std::time::{Duration as StdDuration, Instant};

use super::{
    auth::authenticate,
    error::{ApiResult, api_error},
};
use crate::state::AppState;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use voiceos_core::{
    CaptureSource, DEFAULT_FIELDY_RETENTION_DAYS, FieldyTranscriptEvent, MAX_FIELDY_BODY_BYTES,
    PersonalExtractionContract, PersonalExtractionInput, TaskApprovalStatus,
};
use voiceos_ontology::{CanonicalRequest, DecisionStatus};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureRequest {
    source: String,
    source_id: String,
    text: String,
    retention_hours: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LimitQuery {
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionRequest {
    status: String,
    audit_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtractRequest {
    output: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApproveRequest {
    status: Option<TaskApprovalStatus>,
    estimated_minutes: Option<u32>,
    audit_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResetRequest {
    reset_date: String,
    audit_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FocusResetQuery {
    mode: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoicePersonalCommandRequest {
    device_id: String,
    text: String,
}

struct JsonExtractor(Value);
impl PersonalExtractionContract for JsonExtractor {
    fn extract(&self, _input: &PersonalExtractionInput) -> Result<String, String> {
        Ok(self.0.to_string())
    }
}
fn err(e: voiceos_core::StoreError) -> super::error::ApiError {
    api_error(StatusCode::BAD_REQUEST, e.to_string())
}

pub(crate) async fn voice_command(
    State(state): State<AppState>,
    Json(request): Json<VoicePersonalCommandRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = request.device_id.trim().to_owned();
    let text = request.text.trim().to_owned();
    if device_id.is_empty() || text.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "device_id_and_text_required",
        ));
    }
    let awaiting_capture = state
        .pending_capture_devices
        .lock()
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture_prompt_unavailable",
            )
        })?
        .remove(&device_id)
        .is_some_and(|created_at| created_at.elapsed() <= StdDuration::from_secs(300));
    if awaiting_capture {
        return capture_voice_text(&state, &device_id, &text);
    }

    let ontology = state.ontology.clone();
    let owner_id = state.primary_owner_id.clone();
    let phrase = text.clone();
    let decision =
        tokio::task::spawn_blocking(move || ontology.interpret_deterministic(&owner_id, &phrase))
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let Some(command) = decision.interpretation else {
        return Ok(Json(json!({"handled": false})));
    };
    if decision.status != DecisionStatus::Resolved || !command.intent.0.starts_with("personal.") {
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
    match command.intent.0.as_str() {
        "personal.capture" => match command.arguments.get("content").and_then(Value::as_str) {
            Some(content) if !content.trim().is_empty() => {
                capture_voice_text(state, device_id, content)
            }
            _ => {
                state
                    .pending_capture_devices
                    .lock()
                    .map_err(|_| {
                        api_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "capture_prompt_unavailable",
                        )
                    })?
                    .insert(device_id.to_owned(), Instant::now());
                Ok(Json(json!({
                    "handled": true,
                    "intent": "personal.capture",
                    "capture_prompt": true,
                    "response_text": "What would you like me to capture?"
                })))
            }
        },
        "personal.next" | "personal.unstuck" => {
            let focus_reset = state
                .store
                .personal_focus_reset(owner, "normal")
                .map_err(err)?;
            let response_text = focus_reset.message.clone();
            Ok(personal_result(
                &command.intent.0,
                response_text,
                json!({"focus_reset": focus_reset}),
                false,
            ))
        }
        "personal.interrupt" => {
            let session = match state.store.active_focus_session(owner).map_err(err)? {
                Some(active) => Some(
                    state
                        .store
                        .interrupt_focus_session(
                            owner,
                            &active.id,
                            "Voice interruption",
                            None,
                            &format!("voice-device:{device_id}"),
                        )
                        .map_err(err)?,
                ),
                None => None,
            };
            let response_text = if session.is_some() {
                "Your place is saved. We can return to it when you are ready."
            } else {
                "There is no active focus session to save right now."
            };
            Ok(personal_result(
                &command.intent.0,
                response_text.into(),
                json!({"session": session}),
                session.is_some(),
            ))
        }
        "personal.inbox" => {
            let captures = state.store.personal_inbox(owner).map_err(err)?;
            let response_text = match captures.len() {
                0 => "Your capture inbox is clear.".to_owned(),
                1 => "You have one capture waiting for review.".to_owned(),
                count => format!("You have {count} captures waiting for review."),
            };
            Ok(personal_result(
                &command.intent.0,
                response_text,
                json!({"captures": captures}),
                false,
            ))
        }
        "personal.review" => {
            let capture = state
                .store
                .personal_inbox(owner)
                .map_err(err)?
                .into_iter()
                .next();
            let response_text = capture
                .as_ref()
                .map(|capture| format!("The newest capture is: {}", capture.display_text))
                .unwrap_or_else(|| "There is no capture waiting for review.".to_owned());
            Ok(personal_result(
                &command.intent.0,
                response_text,
                json!({"capture": capture}),
                false,
            ))
        }
        "personal.discard" => Ok(personal_result(
            &command.intent.0,
            "Tell me which capture you want to discard.".into(),
            json!({"decision": Value::Null}),
            false,
        )),
        _ => Ok(Json(json!({"handled": false}))),
    }
}

fn capture_voice_text(state: &AppState, device_id: &str, text: &str) -> ApiResult<Json<Value>> {
    let capture = state
        .store
        .capture_personal_input_as(
            &state.primary_owner_id,
            CaptureSource::voice(format!("voice-{}", Uuid::new_v4())),
            text,
            Utc::now(),
            Duration::hours(48),
            device_id,
        )
        .map_err(err)?;
    Ok(Json(json!({
        "handled": true,
        "intent": "personal.capture",
        "capture": capture,
        "response_text": "Captured. It is safely parked for review.",
        "provider": "deterministic-personal",
        "tool_calls": [{"name": "personal.capture", "status": "completed"}],
        "approvals": [],
        "results": [{"capture": capture}],
        "errors": [],
        "evidence": {"personal_state_changed": true}
    })))
}

fn personal_result(
    intent: &str,
    response_text: String,
    result: Value,
    changed: bool,
) -> Json<Value> {
    let mut payload = result.as_object().cloned().unwrap_or_default();
    payload.insert("handled".into(), Value::Bool(true));
    payload.insert("intent".into(), Value::String(intent.to_owned()));
    payload.insert("response_text".into(), Value::String(response_text));
    payload.insert(
        "provider".into(),
        Value::String("deterministic-personal".into()),
    );
    payload.insert(
        "tool_calls".into(),
        json!([{"name": intent, "status": "completed"}]),
    );
    payload.insert("approvals".into(), json!([]));
    payload.insert("results".into(), Value::Array(vec![result]));
    payload.insert("errors".into(), json!([]));
    payload.insert(
        "evidence".into(),
        json!({"personal_state_changed": changed}),
    );
    Json(Value::Object(payload))
}

pub(crate) async fn fieldy_intake(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if body.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "fieldy_body_required"));
    }
    if body.len() > MAX_FIELDY_BODY_BYTES {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "fieldy_body_too_large",
        ));
    }
    let event: FieldyTranscriptEvent = serde_json::from_slice(&body)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_fieldy_event"))?;
    let capture = state
        .store
        .capture_fieldy_event(
            &state.primary_owner_id,
            &event,
            Duration::days(DEFAULT_FIELDY_RETENTION_DAYS),
        )
        .map_err(err)?;
    Ok((StatusCode::CREATED, Json(json!({"capture": capture}))))
}

pub(crate) async fn capture(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(r): Json<CaptureRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&s, &h)?;
    let c = s
        .store
        .capture_personal_input_as(
            &s.primary_owner_id,
            CaptureSource {
                kind: r.source,
                id: r.source_id,
            },
            &r.text,
            Utc::now(),
            Duration::hours(r.retention_hours.unwrap_or(48)),
            &device_id,
        )
        .map_err(err)?;
    Ok((StatusCode::CREATED, Json(json!({"capture":c}))))
}
pub(crate) async fn inbox(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    Ok(Json(
        json!({"captures":s.store.personal_inbox(&s.primary_owner_id).map_err(err)?}),
    ))
}
pub(crate) async fn capture_decision(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(r): Json<DecisionRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    Ok(Json(
        json!({"decision":s.store.decide_personal_capture(&s.primary_owner_id,&id,&r.status,&r.audit_id).map_err(err)?}),
    ))
}
pub(crate) async fn extract(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(r): Json<ExtractRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    Ok(Json(
        json!({"proposals":s.store.extract_personal_capture(&s.primary_owner_id,&id,&JsonExtractor(r.output)).map_err(err)?}),
    ))
}
pub(crate) async fn proposals(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    Ok(Json(
        json!({"proposals":s.store.capture_proposals(&s.primary_owner_id,q.limit.unwrap_or(50)).map_err(err)?}),
    ))
}
pub(crate) async fn approve(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(r): Json<ApproveRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    let audit = r.audit_id;
    if let Some(status) = r.status {
        let t = s
            .store
            .approve_task_proposal(
                &s.primary_owner_id,
                &id,
                status,
                r.estimated_minutes.unwrap_or(30),
                &audit,
            )
            .map_err(err)?;
        return Ok(Json(json!({"task":t})));
    }
    let record = s
        .store
        .approve_non_task_proposal(&s.primary_owner_id, &id, &audit)
        .map_err(err)?;
    Ok(Json(json!({"review":record})))
}
pub(crate) async fn proposal_decision(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(r): Json<DecisionRequest>,
) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    Ok(Json(
        json!({"decision":s.store.decide_capture_proposal(&s.primary_owner_id,&id,&r.status,&r.audit_id).map_err(err)?}),
    ))
}
pub(crate) async fn reviews(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    Ok(Json(
        json!({"reviews":s.store.personal_review_records(&s.primary_owner_id,q.limit.unwrap_or(50)).map_err(err)?}),
    ))
}
pub(crate) async fn focus_reset(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<FocusResetQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&s, &h)?;
    let reset = s
        .store
        .personal_focus_reset(&s.primary_owner_id, q.mode.as_deref().unwrap_or("normal"))
        .map_err(err)?;
    Ok(Json(json!({"focus_reset": reset})))
}
pub(crate) async fn reset(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(r): Json<ResetRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&s, &h)?;
    let x = s
        .store
        .create_daily_focus_reset(&s.primary_owner_id, &r.reset_date, &r.audit_id)
        .map_err(err)?;
    Ok((StatusCode::CREATED, Json(json!({"reset":x}))))
}
