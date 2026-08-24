use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const PROTOCOL_VERSION: u8 = 1;
const MAX_REQUEST_BYTES: u64 = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConsoleCommand {
    ShowWeather,
    RefreshDashboard,
}

impl ConsoleCommand {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ShowWeather => "show_weather",
            Self::RefreshDashboard => "refresh_dashboard",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    version: u8,
    request_id: String,
    command: ConsoleCommand,
}

#[derive(Serialize)]
struct CommandResponse {
    version: u8,
    request_id: String,
    status: String,
    command: Option<String>,
    error: Option<String>,
}

pub(crate) fn socket_path() -> io::Result<PathBuf> {
    if let Some(configured) = std::env::var_os("VOICEOS_CONSOLE_SOCKET") {
        let path = PathBuf::from(configured);
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "VOICEOS_CONSOLE_SOCKET must be absolute",
            ));
        }
        return Ok(path);
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "XDG_RUNTIME_DIR is required for VIC Console IPC",
        )
    })?;
    Ok(PathBuf::from(runtime).join("voiceos/vic-console.sock"))
}

pub(crate) fn start(app: AppHandle) -> io::Result<PathBuf> {
    let path = socket_path()?;
    prepare_socket_parent(&path)?;
    remove_stale_socket(&path)?;
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let thread_path = path.clone();
    std::thread::Builder::new()
        .name("vic-console-ipc".into())
        .spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => handle_connection(&app, stream),
                    Err(error) => eprintln!("VIC Console IPC accept failed: {error}"),
                }
            }
            let _ = fs::remove_file(thread_path);
        })?;
    Ok(path)
}

fn prepare_socket_parent(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "console socket needs a parent")
    })?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
}

fn remove_stale_socket(path: &Path) -> io::Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !metadata.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to replace a non-socket VIC Console IPC path",
        ));
    }
    if UnixStream::connect(path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "another VIC Console IPC listener is active",
        ));
    }
    fs::remove_file(path)
}

fn handle_connection(app: &AppHandle, mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut body = String::new();
    let read = Read::by_ref(&mut stream)
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_string(&mut body);
    let parsed = match read {
        Ok(size) if size <= MAX_REQUEST_BYTES as usize => parse_request(&body),
        Ok(_) => Err("request_too_large"),
        Err(_) => Err("request_read_failed"),
    };
    let response = match parsed {
        Ok(request) => match app.emit("vic-console-command", request.command.as_str()) {
            Ok(()) => CommandResponse {
                version: PROTOCOL_VERSION,
                request_id: request.request_id,
                status: "completed".into(),
                command: Some(request.command.as_str().into()),
                error: None,
            },
            Err(_) => CommandResponse {
                version: PROTOCOL_VERSION,
                request_id: request.request_id,
                status: "error".into(),
                command: None,
                error: Some("event_dispatch_failed".into()),
            },
        },
        Err(error) => CommandResponse {
            version: PROTOCOL_VERSION,
            request_id: String::new(),
            status: "denied".into(),
            command: None,
            error: Some(error.into()),
        },
    };
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = stream.write_all(&bytes);
    }
}

fn parse_request(body: &str) -> Result<CommandRequest, &'static str> {
    let request: CommandRequest =
        serde_json::from_str(body).map_err(|_| "invalid_command_request")?;
    if request.version != PROTOCOL_VERSION {
        return Err("unsupported_protocol_version");
    }
    if request.request_id.trim().is_empty() || request.request_id.len() > 128 {
        return Err("invalid_request_id");
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_versioned_allowlisted_commands() {
        let request =
            parse_request(r#"{"version":1,"request_id":"request-1","command":"show_weather"}"#)
                .unwrap();
        assert_eq!(request.command, ConsoleCommand::ShowWeather);
        assert_eq!(request.command.as_str(), "show_weather");
    }

    #[test]
    fn rejects_unknown_commands_and_fields() {
        assert_eq!(
            parse_request(r#"{"version":1,"request_id":"request-1","command":"run_shell"}"#)
                .unwrap_err(),
            "invalid_command_request"
        );
        assert!(parse_request(
            r#"{"version":1,"request_id":"request-1","command":"show_weather","script":"alert(1)"}"#
        )
        .is_err());
    }
}
