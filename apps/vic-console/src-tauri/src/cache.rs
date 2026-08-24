use crate::{
    error::ConsoleError,
    model::{CacheEnvelope, WeatherData},
};
use chrono::{DateTime, Utc};
use std::{fs, path::Path};

pub fn write(path: &Path, data: &WeatherData) -> Result<(), ConsoleError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ConsoleError::Persistence(e.to_string()))?;
    }
    let envelope = CacheEnvelope {
        saved_at: Utc::now().to_rfc3339(),
        data: data.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&envelope)
        .map_err(|e| ConsoleError::Persistence(e.to_string()))?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).map_err(|e| ConsoleError::Persistence(e.to_string()))?;
    fs::rename(&temporary, path).map_err(|e| ConsoleError::Persistence(e.to_string()))
}

pub fn read(path: &Path) -> Result<CacheEnvelope, ConsoleError> {
    let bytes = fs::read(path).map_err(|_| ConsoleError::NoCache)?;
    serde_json::from_slice(&bytes).map_err(|e| ConsoleError::Persistence(e.to_string()))
}

pub fn age_minutes(envelope: &CacheEnvelope) -> Option<i64> {
    DateTime::parse_from_rfc3339(&envelope.saved_at)
        .ok()
        .map(|saved| {
            Utc::now()
                .signed_duration_since(saved.with_timezone(&Utc))
                .num_minutes()
                .max(0)
        })
}
