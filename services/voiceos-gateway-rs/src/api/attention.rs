use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_core::begin_task_initiative;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct AttentionQuery {
    status: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct UpsertAttentionRequest {
    category: String,
    source_id: String,
    title: String,
    summary: String,
    #[serde(default = "routine")]
    urgency: String,
    task_id: Option<String>,
    occurred_at: Option<String>,
    due_at: Option<String>,
    #[serde(default)]
    approval_required: bool,
    available_actions: Vec<String>,
    #[serde(default = "empty_object")]
    evidence: Value,
}

#[derive(Deserialize)]
pub(crate) struct AttentionActionRequest {
    action: String,
    task_title: Option<String>,
    observable_outcome: Option<String>,
    estimated_minutes: Option<u32>,
    draft: Option<String>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AttentionQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let items = state
        .store
        .attention_items(
            &state.primary_owner_id,
            query.status.as_deref(),
            query.limit.unwrap_or(100),
        )
        .map_err(store_error)?;
    let summary = json!({
        "total": items.len(),
        "urgent": items.iter().filter(|item| item.urgency == "urgent").count(),
        "approval_required": items.iter().filter(|item| item.approval_required).count(),
        "by_category": items.iter().fold(std::collections::BTreeMap::<String, usize>::new(), |mut counts, item| { *counts.entry(item.category.clone()).or_default() += 1; counts }),
    });
    Ok(Json(json!({"items": items, "summary": summary})))
}

pub(crate) async fn upsert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpsertAttentionRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    authenticate(&state, &headers)?;
    let occurred_at = request
        .occurred_at
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let item = state
        .store
        .upsert_attention_item(
            &state.primary_owner_id,
            &request.category,
            &request.source_id,
            &request.title,
            &request.summary,
            &request.urgency,
            request.task_id.as_deref(),
            &occurred_at,
            request.due_at.as_deref(),
            request.approval_required,
            request.available_actions,
            request.evidence,
        )
        .map_err(store_error)?;
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            "attention",
            "attention.changed",
            "vic:attention-engine",
            json!({"item": &item}),
        )
        .map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"item": item}))))
}

pub(crate) async fn act(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(attention_id): Path<String>,
    Json(request): Json<AttentionActionRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let item = state
        .store
        .attention_item(&state.primary_owner_id, &attention_id)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "attention_item_not_found"))?;
    if !item.available_actions.contains(&request.action) {
        return Err(api_error(StatusCode::BAD_REQUEST, "action_not_available"));
    }
    let actor = format!("device:{device_id}");
    let result = match request.action.as_str() {
        "resolve" | "dismiss" | "snooze" => {
            let status = if request.action == "resolve" {
                "resolved"
            } else if request.action == "dismiss" {
                "dismissed"
            } else {
                "snoozed"
            };
            json!({"item": state.store.set_attention_status(&state.primary_owner_id, &attention_id, status).map_err(store_error)?})
        }
        "create_task" => {
            let title = request.task_title.as_deref().unwrap_or(&item.title);
            let outcome = request
                .observable_outcome
                .as_deref()
                .unwrap_or(&item.summary);
            let task = state
                .store
                .create_task(
                    &state.primary_owner_id,
                    None,
                    None,
                    title,
                    outcome,
                    request.estimated_minutes.unwrap_or(20),
                )
                .map_err(store_error)?;
            let initiative =
                begin_task_initiative(state.store.as_ref(), &state.primary_owner_id, &task, &actor)
                    .map_err(store_error)?;
            state
                .store
                .set_attention_status(&state.primary_owner_id, &attention_id, "resolved")
                .map_err(store_error)?;
            json!({"task": task, "initiative": initiative})
        }
        "prepare_reply" => {
            json!({"status": "draft_prepared", "draft": request.draft.unwrap_or_else(|| format!("Draft a reply regarding: {}", item.title)), "sending_allowed": false})
        }
        "request_send_approval" | "request_invitation_approval" => json!({
            "status": "approval_required",
            "approval": {
                "kind": if request.action == "request_send_approval" { "send_email" } else { "respond_to_calendar_invitation" },
                "attention_id": item.id,
                "exact_effect": if request.action == "request_send_approval" { "Send the reviewed email draft" } else { "Accept, decline, or tentatively accept the invitation" },
                "single_use": true
            }
        }),
        "summarize" | "review" => json!({"item": item}),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "unsupported_attention_action",
            ));
        }
    };
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            "attention",
            "attention.actioned",
            &actor,
            json!({"attention_id": attention_id, "action": request.action, "result": result}),
        )
        .map_err(store_error)?;
    Ok(Json(result))
}

fn routine() -> String {
    "routine".to_owned()
}
fn empty_object() -> Value {
    json!({})
}
fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    match error {
        voiceos_core::StoreError::InvalidInput(message) => {
            api_error(StatusCode::BAD_REQUEST, message)
        }
        other => api_error(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}
