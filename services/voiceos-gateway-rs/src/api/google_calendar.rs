use super::auth::authenticate;
use super::error::{ApiResult, api_error};
use crate::state::AppState;
use axum::http::{HeaderMap, StatusCode};
use axum::{Json, extract::State};
use serde_json::{Value, json};

pub(crate) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let secret_storage_available = state.calendar_secret_store.is_available();
    if let Some(error) = state.google_calendar_oauth_configuration_error {
        return Ok(Json(json!({
            "connected": false,
            "authorization_ready": false,
            "secret_storage_available": secret_storage_available,
            "error": error.code(),
            "next_step": "configure_google_calendar_oauth"
        })));
    }
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let connection =
        tokio::task::spawn_blocking(move || store.google_calendar_connection_for_owner(&owner))
            .await
            .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "store_task_failed"))?
            .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(match connection {
        Some(connection) => json!({
            "connected": true,
            "provider": connection.provider,
            "account_email": connection.account_email,
            "provider_account_id": connection.provider_account_id,
            "authorization_ready": false,
            "secret_storage_available": secret_storage_available,
            "error": "google_calendar_secret_store_unavailable",
            "next_step": "configure_secret_store"
        }),
        None => json!({
            "connected": false,
            "authorization_ready": false,
            "secret_storage_available": secret_storage_available,
            "error": "google_calendar_secret_store_unavailable",
            "next_step": "configure_secret_store"
        }),
    }))
}

pub(crate) async fn disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authenticate(&state, &headers)?;
    let owner = state.primary_owner_id.clone();
    let store = state.store.clone();
    let secret_store = state.calendar_secret_store.clone();
    let disconnected = tokio::task::spawn_blocking(move || {
        store.disconnect_google_calendar_with_secret_store(&owner, secret_store.as_ref())
    })
    .await
    .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "store_task_failed"))?
    .map_err(|_| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "google_calendar_secret_store_unavailable",
        )
    })?;
    Ok(Json(json!({"disconnected": disconnected})))
}
