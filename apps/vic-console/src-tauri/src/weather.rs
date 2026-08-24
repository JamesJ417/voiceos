use crate::{error::ConsoleError, model::*};
use chrono::Utc;
use std::{sync::Arc, time::Duration};

pub trait WeatherFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<String, ConsoleError>;
}

pub struct HttpWeatherFetcher {
    client: reqwest::blocking::Client,
}
impl HttpWeatherFetcher {
    pub fn new() -> Result<Self, ConsoleError> {
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(12))
                .user_agent("VIC-Console/0.1")
                .build()
                .map_err(|e| ConsoleError::Network(e.to_string()))?,
        })
    }
}
impl WeatherFetcher for HttpWeatherFetcher {
    fn fetch(&self, url: &str) -> Result<String, ConsoleError> {
        self.client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| ConsoleError::Network(e.to_string()))
    }
}

pub fn endpoint(config: &AppConfig) -> String {
    format!(
        "{}?latitude={}&longitude={}&timezone=America%2FNew_York&forecast_days=10&temperature_unit={}&wind_speed_unit=mph&current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,wind_direction_10m,is_day&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,wind_speed_10m_max,wind_direction_10m_dominant,sunrise,sunset",
        config.api_endpoint,
        config.latitude,
        config.longitude,
        config.temperature_unit.api_value()
    )
}

pub fn parse(payload: &str, config: &AppConfig) -> Result<WeatherData, ConsoleError> {
    let raw: OpenMeteoResponse =
        serde_json::from_str(payload).map_err(|e| ConsoleError::InvalidData(e.to_string()))?;
    let lengths = [
        raw.daily.time.len(),
        raw.daily.weather_code.len(),
        raw.daily.temperature_2m_max.len(),
        raw.daily.temperature_2m_min.len(),
    ];
    let count = *lengths.iter().min().unwrap_or(&0);
    if count < 10 {
        return Err(ConsoleError::InvalidData(format!(
            "expected 10 forecast days, received {count}"
        )));
    }
    let forecast = (0..10)
        .map(|i| ForecastDay {
            date: raw.daily.time[i].clone(),
            weather_code: raw.daily.weather_code[i],
            high: raw.daily.temperature_2m_max[i],
            low: raw.daily.temperature_2m_min[i],
            precipitation_probability: nested(&raw.daily.precipitation_probability_max, i),
            wind_speed: nested(&raw.daily.wind_speed_10m_max, i),
            wind_direction: nested(&raw.daily.wind_direction_10m_dominant, i),
            sunrise: nested_clone(&raw.daily.sunrise, i),
            sunset: nested_clone(&raw.daily.sunset, i),
        })
        .collect();
    let current = raw.current.map(|value| CurrentConditions {
        temperature: value.temperature_2m,
        apparent_temperature: value.apparent_temperature,
        weather_code: value.weather_code,
        wind_speed: value.wind_speed_10m,
        wind_direction: value.wind_direction_10m,
        is_day: value.is_day.unwrap_or(1) == 1,
    });
    Ok(WeatherData {
        location_name: config.location_name.clone(),
        timezone: raw.timezone,
        temperature_unit: config.temperature_unit.clone(),
        temperature_symbol: config.temperature_unit.symbol().into(),
        wind_unit: raw
            .current_units
            .and_then(|u| u.wind_speed_10m)
            .or_else(|| raw.daily_units.and_then(|u| u.wind_speed_10m_max))
            .unwrap_or_else(|| "mph".into()),
        current,
        forecast,
        fetched_at: Utc::now().to_rfc3339(),
        source_name: "Open-Meteo".into(),
    })
}

fn nested<T: Copy>(values: &Option<Vec<Option<T>>>, index: usize) -> Option<T> {
    values
        .as_ref()
        .and_then(|v| v.get(index))
        .copied()
        .flatten()
}
fn nested_clone<T: Clone>(values: &Option<Vec<Option<T>>>, index: usize) -> Option<T> {
    values
        .as_ref()
        .and_then(|v| v.get(index))
        .cloned()
        .flatten()
}

pub fn fetcher() -> Result<Arc<dyn WeatherFetcher>, ConsoleError> {
    Ok(Arc::new(HttpWeatherFetcher::new()?))
}
