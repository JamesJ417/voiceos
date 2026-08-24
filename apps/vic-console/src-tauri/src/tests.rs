use crate::{
    cache,
    error::ConsoleError,
    model::{AppConfig, AppSettings, CacheEnvelope, DataFreshness, TemperatureUnit},
    service::WeatherService,
    weather::{self, WeatherFetcher},
};
use chrono::{Duration, Utc};
use std::{fs, sync::Arc};
use tempfile::tempdir;

struct StaticFetcher(Result<String, String>);
impl WeatherFetcher for StaticFetcher {
    fn fetch(&self, _: &str) -> Result<String, ConsoleError> {
        self.0.clone().map_err(ConsoleError::Network)
    }
}

fn payload() -> String {
    let dates = (1..=10)
        .map(|day| format!("2026-08-{day:02}"))
        .collect::<Vec<_>>();
    serde_json::json!({
        "timezone":"America/New_York",
        "current":{"temperature_2m":84.0,"apparent_temperature":91.0,"weather_code":2,"wind_speed_10m":7.0,"wind_direction_10m":120,"is_day":1},
        "current_units":{"temperature_2m":"°F","wind_speed_10m":"mp/h"},
        "daily_units":{"temperature_2m_max":"°F","wind_speed_10m_max":"mp/h"},
        "daily":{"time":dates,"weather_code":[2,3,61,80,1,0,2,3,61,80],"temperature_2m_max":[90,91,88,87,92,93,91,89,88,90],"temperature_2m_min":[72,73,71,70,74,75,73,72,71,72],"precipitation_probability_max":[10,20,60,70,5,0,15,25,65,55],"wind_speed_10m_max":[8,9,10,11,7,6,8,9,12,10],"wind_direction_10m_dominant":[120,130,140,150,100,90,110,120,160,150],"sunrise":["2026-08-01T06:48","2026-08-02T06:49","2026-08-03T06:49","2026-08-04T06:50","2026-08-05T06:51","2026-08-06T06:51","2026-08-07T06:52","2026-08-08T06:52","2026-08-09T06:53","2026-08-10T06:54"],"sunset":["2026-08-01T20:20","2026-08-02T20:19","2026-08-03T20:18","2026-08-04T20:17","2026-08-05T20:16","2026-08-06T20:15","2026-08-07T20:14","2026-08-08T20:13","2026-08-09T20:12","2026-08-10T20:11"]}
    }).to_string()
}

#[test]
fn parses_and_transforms_ten_days() {
    let data = weather::parse(&payload(), &AppConfig::default()).unwrap();
    assert_eq!(data.forecast.len(), 10);
    assert_eq!(data.forecast[2].precipitation_probability, Some(60));
    assert_eq!(data.current.unwrap().temperature, 84.0);
}

#[test]
fn temperature_units_change_request_and_display_symbol() {
    let config = AppConfig {
        temperature_unit: TemperatureUnit::Celsius,
        ..AppConfig::default()
    };
    assert!(weather::endpoint(&config).contains("temperature_unit=celsius"));
    assert_eq!(
        weather::parse(&payload(), &config)
            .unwrap()
            .temperature_symbol,
        "°C"
    );
}

#[test]
fn rejects_incomplete_or_malformed_responses() {
    assert!(weather::parse("not-json", &AppConfig::default()).is_err());
    let mut short: serde_json::Value = serde_json::from_str(&payload()).unwrap();
    short["daily"]["time"].as_array_mut().unwrap().truncate(9);
    assert!(weather::parse(&short.to_string(), &AppConfig::default()).is_err());
}

#[test]
fn cache_round_trip_and_stale_age_work() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let data = weather::parse(&payload(), &AppConfig::default()).unwrap();
    cache::write(&path, &data).unwrap();
    assert_eq!(cache::read(&path).unwrap().data.forecast.len(), 10);
    let envelope = CacheEnvelope {
        saved_at: (Utc::now() - Duration::minutes(90)).to_rfc3339(),
        data,
    };
    fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert!(cache::age_minutes(&cache::read(&path).unwrap()).unwrap() >= 89);
}

#[test]
fn network_failure_falls_back_to_stale_cache() {
    let dir = tempdir().unwrap();
    let good = WeatherService::new(
        dir.path().to_owned(),
        Arc::new(StaticFetcher(Ok(payload()))),
    );
    assert_eq!(good.refresh().unwrap().freshness, DataFreshness::Fresh);
    let offline = WeatherService::new(
        dir.path().to_owned(),
        Arc::new(StaticFetcher(Err("offline".into()))),
    );
    let fallback = offline.refresh().unwrap();
    assert_eq!(fallback.freshness, DataFreshness::Stale);
    assert!(fallback.source_error.unwrap().contains("offline"));
}

#[test]
fn stale_startup_cache_triggers_a_refresh() {
    let dir = tempdir().unwrap();
    let data = weather::parse(&payload(), &AppConfig::default()).unwrap();
    let stale = CacheEnvelope {
        saved_at: (Utc::now() - Duration::hours(2)).to_rfc3339(),
        data,
    };
    fs::create_dir_all(dir.path()).unwrap();
    fs::write(
        dir.path().join("weather-cache.json"),
        serde_json::to_vec(&stale).unwrap(),
    )
    .unwrap();
    let service = WeatherService::new(
        dir.path().to_owned(),
        Arc::new(StaticFetcher(Ok(payload()))),
    );
    assert_eq!(service.initial().unwrap().freshness, DataFreshness::Fresh);
}

#[test]
fn network_failure_without_cache_is_an_error() {
    let dir = tempdir().unwrap();
    let offline = WeatherService::new(
        dir.path().to_owned(),
        Arc::new(StaticFetcher(Err("offline".into()))),
    );
    assert!(matches!(offline.refresh(), Err(ConsoleError::Network(_))));
}

#[test]
fn settings_persist() {
    let dir = tempdir().unwrap();
    let service = WeatherService::new(
        dir.path().to_owned(),
        Arc::new(StaticFetcher(Ok(payload()))),
    );
    service
        .save_settings(&AppSettings {
            temperature_unit: TemperatureUnit::Celsius,
            selected_panel: "news".into(),
        })
        .unwrap();
    assert_eq!(
        service.settings().unwrap().temperature_unit,
        TemperatureUnit::Celsius
    );
}
