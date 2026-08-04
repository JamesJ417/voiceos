use axum::Json;
use serde_json::{Value, json};

pub(crate) async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "gateway": "ok",
        "speech_to_text": "android-on-device",
        "language_model": "rust-provider-router",
        "text_to_speech": "android-device",
        "memory": "voiceos-core-sqlite",
        "transport": "tailscale-https"
    }))
}
