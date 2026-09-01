use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde_json::{Value, json};
use voiceos_core::{ConversationArea, ConversationRecord};

use crate::state::AppState;

use super::auth::authenticate;
use super::error::ApiResult;

/// Returns the stable, server-owned contract a native client needs after it
/// has obtained a device credential through the production enrollment gateway.
pub(crate) async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    let owner_id = state.primary_owner_id.clone();
    let store = state.store.clone();
    let (selected_area_id, active_conversation) = tokio::task::spawn_blocking(move || {
        Ok::<_, voiceos_core::StoreError>((
            store.selected_area(&owner_id)?,
            store.active_conversation_record(&owner_id)?,
        ))
    })
    .await
    .map_err(|error| {
        super::error::api_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        )
    })?
    .map_err(|error| {
        super::error::api_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        )
    })?;
    Ok(Json(bootstrap_payload(
        &device_id,
        &selected_area_id,
        active_conversation.as_ref(),
        &state.store.conversation_areas(),
    )))
}

fn bootstrap_payload(
    device_id: &str,
    selected_area_id: &str,
    active_conversation: Option<&ConversationRecord>,
    areas: &[ConversationArea],
) -> Value {
    json!({
        "contract_version": 2,
        "device_id": device_id,
        "conversation_areas": areas,
        "selected_area_id": selected_area_id,
        "active_conversation": active_conversation,
        "authentication": {"scheme": "bearer"},
        "component_registry": component_registry(),
        "endpoints": {
            "bootstrap": "/v1/client/bootstrap",
            "conversation": "/v1/conversations/active",
            "conversation_events": "/v1/conversations/active/events",
            "conversation_areas": "/v1/conversation-areas",
            "conversations": "/v1/conversations",
            "conversation_history": "/v1/conversations/history",
            "conversation_sync": "/v1/conversations/sync",
            "turn": "/v1/turns/text"
        },
        "transport": {"private_network_required": true, "tls_required": true}
    })
}

fn component_registry() -> Value {
    serde_json::from_str(include_str!(
        "../../../../contracts/component-registry.json"
    ))
    .expect("component registry must be valid JSON")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use voiceos_core::built_in_conversation_areas;

    use super::bootstrap_payload;

    #[test]
    fn bootstrap_describes_the_windows_client_contract() {
        let payload = bootstrap_payload(
            "windows-laptop",
            "general-talk",
            None,
            &built_in_conversation_areas(),
        );
        assert_eq!(payload["contract_version"], json!(2));
        assert_eq!(payload["device_id"], json!("windows-laptop"));
        assert_eq!(payload["authentication"], json!({"scheme": "bearer"}));
        assert_eq!(
            payload["endpoints"],
            json!({
                "bootstrap": "/v1/client/bootstrap",
                "conversation": "/v1/conversations/active",
                "conversation_events": "/v1/conversations/active/events",
                "conversation_areas": "/v1/conversation-areas",
                "conversations": "/v1/conversations",
                "conversation_history": "/v1/conversations/history",
                "conversation_sync": "/v1/conversations/sync",
                "turn": "/v1/turns/text"
            })
        );
        assert_eq!(payload["selected_area_id"], json!("general-talk"));
        assert_eq!(payload["conversation_areas"].as_array().unwrap().len(), 6);
        assert_eq!(
            payload["component_registry"]["roles"]["backend_control_plane"],
            "voiceos"
        );
        assert_eq!(
            payload["component_registry"]["roles"]["voice_interface_controller"],
            "vic"
        );
        assert_eq!(
            payload["component_registry"]["roles"]["touchscreen_system_interface"],
            "touch"
        );
        assert_eq!(
            payload["transport"],
            json!({"private_network_required": true, "tls_required": true})
        );
    }
}
