use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use voiceos_ontology::{AliasInput, CanonicalRequest, ResolutionSource};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

#[derive(Deserialize)]
pub(crate) struct InterpretRequest {
    phrase: String,
}

#[derive(Deserialize)]
pub(crate) struct InternalInterpretRequest {
    owner_id: String,
    phrase: String,
}

#[derive(Deserialize)]
pub(crate) struct ValidateToolRequest {
    owner_id: String,
    tool: String,
    #[serde(default)]
    arguments: BTreeMap<String, Value>,
    #[serde(default = "default_confidence")]
    confidence: f32,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CorrectionRequest {
    request: CanonicalRequest,
    #[serde(default)]
    note: String,
}

pub(crate) async fn catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    Ok(Json(json!({
        "version": state.ontology.catalog().version(),
        "minimum_compatible_version": state.ontology.catalog().minimum_compatible_version(),
        "entity_kinds": state.ontology.catalog().entity_kinds(),
        "intents": state.ontology.catalog().intents().collect::<Vec<_>>(),
        "entities": state.ontology.catalog().entities().collect::<Vec<_>>(),
    })))
}

pub(crate) async fn validate_tool(
    State(state): State<AppState>,
    Json(request): Json<ValidateToolRequest>,
) -> ApiResult<Json<Value>> {
    if request.owner_id.trim().is_empty() || request.tool.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "owner_id_and_tool_required",
        ));
    }
    let source = match request.source.as_deref() {
        Some("deterministic") => ResolutionSource::Deterministic,
        Some("approved_alias") => ResolutionSource::ApprovedAlias,
        Some("user_correction") => ResolutionSource::UserCorrection,
        _ => ResolutionSource::ModelFallback,
    };
    let ontology = state.ontology.clone();
    let decision = tokio::task::spawn_blocking(move || {
        ontology.validate_tool(
            &request.owner_id,
            &request.tool,
            request.arguments,
            request.confidence,
            source,
        )
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"decision": decision})))
}

fn default_confidence() -> f32 {
    1.0
}

pub(crate) async fn interpret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<InterpretRequest>,
) -> ApiResult<Json<Value>> {
    if request.phrase.trim().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "phrase_required"));
    }
    let owner_id = authenticate(&state, &headers)?;
    let ontology = state.ontology.clone();
    let decision =
        tokio::task::spawn_blocking(move || ontology.interpret(&owner_id, &request.phrase))
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"decision": decision})))
}

pub(crate) async fn interpret_deterministic(
    State(state): State<AppState>,
    Json(request): Json<InternalInterpretRequest>,
) -> ApiResult<Json<Value>> {
    if request.owner_id.trim().is_empty() || request.phrase.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "owner_id_and_phrase_required",
        ));
    }
    let ontology = state.ontology.clone();
    let decision = tokio::task::spawn_blocking(move || {
        ontology.interpret_deterministic(&request.owner_id, &request.phrase)
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"decision": decision})))
}

pub(crate) async fn list_aliases(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let owner_id = authenticate(&state, &headers)?;
    let aliases = state
        .ontology
        .aliases(&owner_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({"aliases": aliases})))
}

pub(crate) async fn approve_alias(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AliasInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let owner_id = authenticate(&state, &headers)?;
    let alias = state
        .ontology
        .approve_alias(&owner_id, &input)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"alias": alias}))))
}

pub(crate) async fn correct(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(interpretation_id): Path<String>,
    Json(correction): Json<CorrectionRequest>,
) -> ApiResult<Json<Value>> {
    let owner_id = authenticate(&state, &headers)?;
    let decision = state
        .ontology
        .correct(
            &owner_id,
            &interpretation_id,
            correction.request,
            &correction.note,
        )
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"decision": decision})))
}
