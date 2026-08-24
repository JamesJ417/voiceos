use crate::{
    error::ConsoleError,
    model::{AppConfig, AppSettings},
};
use std::{fs, path::Path};

pub fn load_config(path: &Path) -> Result<AppConfig, ConsoleError> {
    read_or_default(path)
}
pub fn load_settings(path: &Path) -> Result<AppSettings, ConsoleError> {
    read_or_default(path)
}

fn read_or_default<T>(path: &Path) -> Result<T, ConsoleError>
where
    T: serde::de::DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).map_err(|e| ConsoleError::Persistence(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| ConsoleError::Persistence(e.to_string()))
}

pub fn save_settings(path: &Path, settings: &AppSettings) -> Result<(), ConsoleError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ConsoleError::Persistence(e.to_string()))?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| ConsoleError::Persistence(e.to_string()))?;
    fs::write(path, bytes).map_err(|e| ConsoleError::Persistence(e.to_string()))
}
