use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Response, Sse};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::artifact_worker::{PdfSpec, recipe_card_spec};
use crate::state::AppState;

use super::auth::{authenticate, header_text};
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct ArtifactQuery {
    query: Option<String>,
    limit: Option<usize>,
    after: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct CreatePdfRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    metadata: Value,
    spec: Option<PdfSpec>,
    #[serde(default)]
    template: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct InternalToolRequest {
    tool: String,
    #[serde(default)]
    arguments: Value,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArtifactQuery>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let artifacts = tokio::task::spawn_blocking(move || {
        store.list_artifacts(&owner, query.query.as_deref(), query.limit.unwrap_or(100))
    })
    .await
    .map_err(internal)?
    .map_err(store_error)?;
    Ok(Json(json!({"artifacts": artifacts})))
}

pub(crate) async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let artifact = state
        .store
        .artifact(&state.primary_owner_id, &id)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "artifact_not_found"))?;
    Ok(Json(json!({"artifact": artifact})))
}

pub(crate) async fn create_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreatePdfRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    queue_pdf(&state, request, None, &device_id)
}

pub(crate) async fn revise_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CreatePdfRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let device_id = authenticate(&state, &headers)?;
    let parent = state
        .store
        .artifact(&state.primary_owner_id, &id)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "artifact_not_found"))?;
    if parent.kind != "pdf" {
        return Err(api_error(StatusCode::BAD_REQUEST, "artifact_is_not_pdf"));
    }
    queue_pdf(&state, request, Some(&id), &device_id)
}

fn queue_pdf(
    state: &AppState,
    request: CreatePdfRequest,
    parent: Option<&str>,
    device_id: &str,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let mut spec = match request.template.as_deref() {
        Some("recipe-card") => recipe_card_spec(),
        _ => request
            .spec
            .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "pdf_spec_required"))?,
    };
    if !request.title.trim().is_empty() {
        spec.title = request.title.trim().to_owned();
    }
    let filename = request
        .filename
        .unwrap_or_else(|| format!("{}.pdf", slug(&spec.title)));
    if !filename.to_ascii_lowercase().ends_with(".pdf") {
        return Err(api_error(StatusCode::BAD_REQUEST, "pdf_filename_required"));
    }
    let metadata = if request.metadata.is_object() {
        request.metadata
    } else {
        json!({})
    };
    let artifact = state
        .store
        .create_artifact(
            &state.primary_owner_id,
            request.job_id.as_deref(),
            request.task_id.as_deref(),
            parent,
            "pdf",
            &spec.title,
            &filename,
            "application/pdf",
            &request.description,
            device_id,
            metadata,
        )
        .map_err(store_error)?;
    state
        .pdf_worker
        .enqueue(
            state.primary_owner_id.clone(),
            artifact.id.clone(),
            request.task_id,
            request.description,
            spec,
        )
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error))?;
    Ok((StatusCode::ACCEPTED, Json(json!({"artifact": artifact}))))
}

pub(crate) async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    file_response(&state, &headers, &id, false)
}

pub(crate) async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    file_response(&state, &headers, &id, true)
}

fn file_response(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    attachment: bool,
) -> ApiResult<Response> {
    authenticate(state, headers)?;
    let artifact = state
        .store
        .artifact(&state.primary_owner_id, id)
        .map_err(store_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "artifact_not_found"))?;
    if artifact.status != "ready" {
        return Err(api_error(StatusCode::CONFLICT, "artifact_not_ready"));
    }
    let bytes = state
        .artifact_storage
        .read_validated(&artifact)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let disposition = if attachment { "attachment" } else { "inline" };
    Ok((
        [
            (header::CONTENT_TYPE, artifact.media_type),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "{disposition}; filename=\"{}\"",
                    safe_header_filename(&artifact.filename)
                ),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_owned()),
            (
                header::ETAG,
                format!("\"{}\"", artifact.sha256.unwrap_or_default()),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub(crate) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArtifactQuery>,
) -> ApiResult<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>> {
    authenticate(&state, &headers)?;
    let header_cursor =
        header_text(&headers, "last-event-id").and_then(|value| value.parse::<i64>().ok());
    let mut cursor = query.after.or(header_cursor).unwrap_or(0).max(0);
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let stream = async_stream::stream! {
        loop {
            let read_store = store.clone(); let read_owner = owner.clone();
            match tokio::task::spawn_blocking(move || read_store.artifact_events_after(&read_owner, cursor, 100)).await {
                Ok(Ok(events)) if !events.is_empty() => for event in events {
                    cursor = event.id;
                    let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_owned());
                    yield Ok(Event::default().event(event.event_type).id(cursor.to_string()).data(data));
                },
                Ok(Ok(_)) => tokio::time::sleep(Duration::from_millis(400)).await,
                _ => { yield Ok(Event::default().event("artifact.error").data("{\"error\":\"artifact_stream_unavailable\"}")); tokio::time::sleep(Duration::from_secs(1)).await; }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

pub(crate) async fn internal_tool(
    State(state): State<AppState>,
    Json(request): Json<InternalToolRequest>,
) -> ApiResult<Json<Value>> {
    let arguments = request
        .arguments
        .as_object()
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "artifact_arguments_required"))?;
    let owner = state.primary_owner_id.clone();
    match request.tool.as_str() {
        "artifact.pdf.create" | "artifact.pdf.revise" => {
            let title = required_argument(arguments, "title")?.to_owned();
            let parent = if request.tool.ends_with("revise") {
                Some(required_argument(arguments, "artifact_id")?)
            } else {
                None
            };
            let create = CreatePdfRequest {
                title,
                description: arguments
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                filename: arguments
                    .get("filename")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                task_id: arguments
                    .get("task_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                job_id: None,
                metadata: arguments
                    .get("metadata")
                    .cloned()
                    .unwrap_or_else(|| json!({"source":"vic"})),
                spec: arguments
                    .get("spec")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?,
                template: arguments
                    .get("template")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            };
            let (_, Json(value)) = queue_pdf(&state, create, parent, "vic")?;
            Ok(Json(value))
        }
        "artifact.find" => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let artifacts = state
                .store
                .list_artifacts(&owner, query, 25)
                .map_err(store_error)?;
            Ok(Json(json!({"artifacts": artifacts})))
        }
        "artifact.attach" => {
            let artifact_id = required_argument(arguments, "artifact_id")?;
            let task_id = required_argument(arguments, "task_id")?;
            let description = required_argument(arguments, "description")?;
            let artifact = state
                .store
                .artifact(&owner, artifact_id)
                .map_err(store_error)?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "artifact_not_found"))?;
            if artifact.status != "ready" {
                return Err(api_error(StatusCode::CONFLICT, "artifact_not_ready"));
            }
            let attachment = state
                .store
                .attach_task_artifact(
                    &owner,
                    task_id,
                    &artifact.kind,
                    &format!("artifact:{}", artifact.id),
                    description,
                    "vic",
                    "vic",
                )
                .map_err(store_error)?;
            Ok(Json(
                json!({"artifact": artifact, "attachment": attachment}),
            ))
        }
        _ => Err(api_error(StatusCode::NOT_FOUND, "artifact_tool_not_found")),
    }
}

fn required_argument<'a>(
    arguments: &'a serde_json::Map<String, Value>,
    key: &str,
) -> ApiResult<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("{key}_required")))
}

fn slug(value: &str) -> String {
    let value = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if value.is_empty() {
        "vic-document".to_owned()
    } else {
        value.chars().take(80).collect()
    }
}

fn safe_header_filename(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        .collect()
}
fn internal(error: tokio::task::JoinError) -> super::error::ApiError {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
fn store_error(error: voiceos_core::StoreError) -> super::error::ApiError {
    api_error(StatusCode::BAD_REQUEST, error.to_string())
}
