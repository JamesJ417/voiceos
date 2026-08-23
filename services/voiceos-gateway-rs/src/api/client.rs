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
        "endpoints": {
            "conversation": "/v1/conversations/active",
            "conversation_events": "/v1/conversations/active/events",
            "turn": "/v1/turns/text"
        },
        "transport": {"private_network_required": true, "tls_required": true}
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::bootstrap_payload;

    #[test]
    fn bootstrap_describes_the_windows_client_contract() {
        assert_eq!(
            bootstrap_payload("windows-laptop"),
            json!({
                "contract_version": 1,
                "device_id": "windows-laptop",
                "authentication": {"scheme": "bearer"},
                "endpoints": {
                    "conversation": "/v1/conversations/active",
                    "conversation_events": "/v1/conversations/active/events",
                    "turn": "/v1/turns/text"
                },
                "transport": {"private_network_required": true, "tls_required": true}
            })
        );
    }
}
