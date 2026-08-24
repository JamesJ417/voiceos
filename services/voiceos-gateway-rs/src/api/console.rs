use std::path::PathBuf;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use uuid::Uuid;
use voiceos_ontology::DecisionStatus;

use crate::state::AppState;

use super::auth::authenticate;
use super::error::{ApiResult, api_error};

const MAX_RESPONSE_BYTES: u64 = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConsoleCommand {
    ShowWeather,
    RefreshDashboard,
}

impl ConsoleCommand {
    fn intent(self) -> &'static str {
        match self {
            Self::ShowWeather => "console.show_weather",
            Self::RefreshDashboard => "console.refresh_dashboard",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConsoleCommandRequest {
    command: ConsoleCommand,
}

#[derive(Deserialize)]
pub(crate) struct VoiceConsoleCommandRequest {
    device_id: String,
    text: String,
}

#[derive(Serialize)]
struct IpcCommandRequest {
    version: u8,
    request_id: String,
    command: ConsoleCommand,
}

#[derive(Deserialize)]
struct IpcCommandResponse {
    version: u8,
    request_id: String,
    status: String,
    command: Option<ConsoleCommand>,
    error: Option<String>,
}

pub(crate) async fn execute(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ConsoleCommandRequest>,
) -> ApiResult<Json<Value>> {
    let device_id = authenticate(&state, &headers)?;
    execute_as(&state, request.command, &format!("device:{device_id}")).await
}

pub(crate) async fn internal_execute(
    State(state): State<AppState>,
    Json(request): Json<ConsoleCommandRequest>,
) -> ApiResult<Json<Value>> {
    execute_as(&state, request.command, "provider:vic").await
}

pub(crate) async fn voice_command(
    State(state): State<AppState>,
    Json(request): Json<VoiceConsoleCommandRequest>,
) -> ApiResult<Json<Value>> {
    let phrase = request.text.trim().to_owned();
    let device_id = request.device_id.trim().to_owned();
    if phrase.is_empty() || device_id.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "device_id_and_text_required",
        ));
    }
    if !looks_like_console_command(&phrase) {
        return Ok(Json(json!({"handled": false})));
    }
    let ontology = state.ontology.clone();
    let ontology_owner = device_id.clone();
    let decision =
        tokio::task::spawn_blocking(move || ontology.interpret(&ontology_owner, &phrase))
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let Some(request) = decision.interpretation else {
        return Ok(Json(json!({"handled": false})));
    };
    if decision.status != DecisionStatus::Resolved {
        return Ok(Json(json!({"handled": false})));
    }
    let command = match request.intent.0.as_str() {
        "console.show_weather" => ConsoleCommand::ShowWeather,
        "console.refresh_dashboard" => ConsoleCommand::RefreshDashboard,
        _ => return Ok(Json(json!({"handled": false}))),
    };
    execute_as(&state, command, &format!("voice-device:{device_id}")).await
}

async fn execute_as(
    state: &AppState,
    command: ConsoleCommand,
    actor: &str,
) -> ApiResult<Json<Value>> {
    let request_id = Uuid::new_v4().to_string();
    let delivery = deliver(command, &request_id).await;
    let (event_type, status, error) = match &delivery {
        Ok(()) => ("console.command.completed", "completed", None),
        Err(error) => ("console.command.failed", "error", Some(error.as_str())),
    };
    state
        .store
        .append_execution_event(
            &state.primary_owner_id,
            "vic-console",
            event_type,
            actor,
            json!({
                "request_id": request_id,
                "intent": command.intent(),
                "status": status,
                "error": error,
            }),
        )
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if let Err(error) = delivery {
        return Err(api_error(StatusCode::SERVICE_UNAVAILABLE, error));
    }
    Ok(Json(json!({
        "handled": true,
        "response_text": match command {
            ConsoleCommand::ShowWeather => "Showing weather on VIC Console.",
            ConsoleCommand::RefreshDashboard => "Refreshing the VIC Console weather dashboard.",
        },
        "provider": "deterministic-console",
        "tool_calls": [{"name": command.intent(), "status": "completed"}],
        "approvals": [],
        "results": [{"request_id": request_id, "command": command, "status": "completed"}],
        "errors": [],
        "evidence": {"console_command_delivered": true, "command": command},
    })))
}

async fn deliver(command: ConsoleCommand, request_id: &str) -> Result<(), String> {
    let path = console_socket_path()?;
    let request = serde_json::to_vec(&IpcCommandRequest {
        version: 1,
        request_id: request_id.to_owned(),
        command,
    })
    .map_err(|_| "console_command_serialization_failed".to_owned())?;
    tokio::time::timeout(Duration::from_secs(2), async move {
        let mut stream = UnixStream::connect(path)
            .await
            .map_err(|_| "vic_console_unavailable".to_owned())?;
        stream
            .write_all(&request)
            .await
            .map_err(|_| "console_command_write_failed".to_owned())?;
        stream
            .shutdown()
            .await
            .map_err(|_| "console_command_write_failed".to_owned())?;
        let mut response_bytes = Vec::new();
        (&mut stream)
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut response_bytes)
            .await
            .map_err(|_| "console_response_read_failed".to_owned())?;
        if response_bytes.len() > MAX_RESPONSE_BYTES as usize {
            return Err("console_response_too_large".to_owned());
        }
        let response: IpcCommandResponse = serde_json::from_slice(&response_bytes)
            .map_err(|_| "invalid_console_response".to_owned())?;
        if response.version != 1
            || response.request_id != request_id
            || response.status != "completed"
            || response.command != Some(command)
        {
            return Err(response
                .error
                .unwrap_or_else(|| "console_command_rejected".to_owned()));
        }
        Ok(())
    })
    .await
    .map_err(|_| "vic_console_timeout".to_owned())?
}

fn console_socket_path() -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os("VOICEOS_CONSOLE_SOCKET") {
        let path = PathBuf::from(configured);
        return path
            .is_absolute()
            .then_some(path)
            .ok_or_else(|| "console_socket_path_must_be_absolute".to_owned());
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|path| path.join("voiceos/vic-console.sock"))
        .ok_or_else(|| "console_runtime_directory_unavailable".to_owned())
}

fn looks_like_console_command(phrase: &str) -> bool {
    let phrase = voiceos_ontology::normalize_phrase(phrase);
    let surface = phrase.contains("console") || phrase.contains("dashboard");
    let weather = phrase.contains("weather")
        && [
            "show", "open", "display", "switch", "refresh", "update", "reload",
        ]
        .iter()
        .any(|verb| phrase.contains(verb));
    surface || weather
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_prefilter_is_narrow() {
        assert!(looks_like_console_command("Show the weather"));
        assert!(looks_like_console_command(
            "Refresh the VIC Console dashboard"
        ));
        assert!(!looks_like_console_command("What is the weather tomorrow?"));
        assert!(!looks_like_console_command("Run a shell command"));
    }
}
