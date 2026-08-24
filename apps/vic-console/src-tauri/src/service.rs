use crate::{
    cache, config,
    error::ConsoleError,
    model::{AppConfig, AppSettings, DataFreshness, WeatherSnapshot},
    weather::{self, WeatherFetcher},
};
use std::{path::PathBuf, sync::Arc};

pub struct WeatherService {
    data_dir: PathBuf,
    fetcher: Arc<dyn WeatherFetcher>,
}
impl WeatherService {
    pub fn new(data_dir: PathBuf, fetcher: Arc<dyn WeatherFetcher>) -> Self {
        Self { data_dir, fetcher }
    }
    fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }
    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }
    fn cache_path(&self) -> PathBuf {
        self.data_dir.join("weather-cache.json")
    }
    pub fn config(&self) -> Result<AppConfig, ConsoleError> {
        let mut value = config::load_config(&self.config_path())?;
        value.temperature_unit = self.settings()?.temperature_unit;
        validate_config(&value)?;
        Ok(value)
    }
    pub fn settings(&self) -> Result<AppSettings, ConsoleError> {
        config::load_settings(&self.settings_path())
    }
    pub fn save_settings(&self, value: &AppSettings) -> Result<(), ConsoleError> {
        config::save_settings(&self.settings_path(), value)
    }
    pub fn cached(&self) -> Result<WeatherSnapshot, ConsoleError> {
        let cached = cache::read(&self.cache_path())?;
        let age = cache::age_minutes(&cached);
        let threshold = self.config()?.refresh_interval_minutes as i64 * 2;
        Ok(WeatherSnapshot {
            data: cached.data,
            freshness: if age.unwrap_or(threshold + 1) > threshold {
                DataFreshness::Stale
            } else {
                DataFreshness::Fresh
            },
            source_error: None,
            cache_age_minutes: age,
        })
    }
    pub fn initial(&self) -> Result<WeatherSnapshot, ConsoleError> {
        match self.cached() {
            Ok(snapshot) if snapshot.freshness == DataFreshness::Fresh => Ok(snapshot),
            Ok(_) | Err(_) => self.refresh(),
        }
    }
    pub fn refresh(&self) -> Result<WeatherSnapshot, ConsoleError> {
        let config = self.config()?;
        match self
            .fetcher
            .fetch(&weather::endpoint(&config))
            .and_then(|raw| weather::parse(&raw, &config))
        {
            Ok(data) => {
                cache::write(&self.cache_path(), &data)?;
                Ok(WeatherSnapshot {
                    data,
                    freshness: DataFreshness::Fresh,
                    source_error: None,
                    cache_age_minutes: Some(0),
                })
            }
            Err(source_error) => match cache::read(&self.cache_path()) {
                Ok(cached) => Ok(WeatherSnapshot {
                    cache_age_minutes: cache::age_minutes(&cached),
                    data: cached.data,
                    freshness: DataFreshness::Stale,
                    source_error: Some(source_error.to_string()),
                }),
                Err(_) => Err(source_error),
            },
        }
    }
}

fn validate_config(value: &AppConfig) -> Result<(), ConsoleError> {
    if value.location_name.trim().is_empty()
        || !(-90.0..=90.0).contains(&value.latitude)
        || !(-180.0..=180.0).contains(&value.longitude)
        || value.refresh_interval_minutes == 0
        || !value.api_endpoint.starts_with("https://")
    {
        return Err(ConsoleError::InvalidData(
            "application configuration is invalid".into(),
        ));
    }
    Ok(())
}
