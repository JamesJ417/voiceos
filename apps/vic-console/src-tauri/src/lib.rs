mod cache;
mod config;
mod error;
#[cfg(unix)]
mod ipc;
mod model;
mod service;
#[cfg(test)]
mod tests;
mod weather;

use model::{AppConfig, AppSettings, WeatherSnapshot};
use service::WeatherService;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

#[cfg(unix)]
use ipc::ConsoleCommand;

struct AppState(Arc<WeatherService>);

#[tauri::command]
async fn get_weather(
    state: State<'_, AppState>,
    force_refresh: bool,
) -> Result<WeatherSnapshot, error::ConsoleError> {
    let service = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if force_refresh {
            service.refresh()
        } else {
            service.initial()
        }
    })
    .await
    .map_err(|e| error::ConsoleError::Persistence(e.to_string()))?
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Result<AppConfig, error::ConsoleError> {
    state.0.config()
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, error::ConsoleError> {
    state.0.settings()
}

#[tauri::command]
fn update_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), error::ConsoleError> {
    state.0.save_settings(&settings)
}

#[tauri::command]
fn dispatch_console_command(app: tauri::AppHandle, command: ConsoleCommand) -> Result<(), String> {
    app.emit("vic-console-command", command.as_str())
        .map_err(|e| e.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            let fetcher = weather::fetcher().map_err(|e| std::io::Error::other(e.to_string()))?;
            app.manage(AppState(Arc::new(WeatherService::new(directory, fetcher))));
            #[cfg(unix)]
            {
                let socket = ipc::start(app.handle().clone())?;
                println!(
                    "VIC Console command socket listening at {}",
                    socket.display()
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_weather,
            get_config,
            get_settings,
            update_settings,
            dispatch_console_command
        ])
        .run(tauri::generate_context!())
        .expect("run VIC Console");
}
