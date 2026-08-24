use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde_json::{Value, json};

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
    Ok(Json(bootstrap_payload(&device_id)))
}

fn bootstrap_payload(device_id: &str) -> Value {
    json!({
        "contract_version": 1,
        "device_id": device_id,
        "authentication": {"scheme": "bearer"},
        "component_registry": component_registry(),
        "endpoints": {
            "bootstrap": "/v1/client/bootstrap",
            "conversation": "/v1/conversations/active",
            "conversation_events": "/v1/conversations/active/events",
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

    use super::bootstrap_payload;

    #[test]
    fn bootstrap_describes_the_windows_client_contract() {
        let payload = bootstrap_payload("windows-laptop");
        assert_eq!(payload["contract_version"], json!(1));
        assert_eq!(payload["device_id"], json!("windows-laptop"));
        assert_eq!(payload["authentication"], json!({"scheme": "bearer"}));
        assert_eq!(
            payload["endpoints"],
            json!({
                "bootstrap": "/v1/client/bootstrap",
                "conversation": "/v1/conversations/active",
                "conversation_events": "/v1/conversations/active/events",
                "turn": "/v1/turns/text"
            })
        );
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
