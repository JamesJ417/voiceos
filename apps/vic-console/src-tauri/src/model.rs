use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureUnit {
    Fahrenheit,
    Celsius,
}

impl TemperatureUnit {
    pub fn api_value(&self) -> &'static str {
        match self {
            Self::Fahrenheit => "fahrenheit",
            Self::Celsius => "celsius",
        }
    }
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Fahrenheit => "°F",
            Self::Celsius => "°C",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AppConfig {
    pub location_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub temperature_unit: TemperatureUnit,
    pub refresh_interval_minutes: u64,
    pub api_endpoint: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            location_name: "Newberry, Florida".into(),
            latitude: 29.6464,
            longitude: -82.6065,
            temperature_unit: TemperatureUnit::Fahrenheit,
            refresh_interval_minutes: 30,
            api_endpoint: "https://api.open-meteo.com/v1/forecast".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CurrentConditions {
    pub temperature: f64,
    pub apparent_temperature: Option<f64>,
    pub weather_code: i32,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<i32>,
    pub is_day: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ForecastDay {
    pub date: String,
    pub weather_code: i32,
    pub high: f64,
    pub low: f64,
    pub precipitation_probability: Option<i32>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<i32>,
    pub sunrise: Option<String>,
    pub sunset: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct WeatherData {
    pub location_name: String,
    pub timezone: String,
    pub temperature_unit: TemperatureUnit,
    pub temperature_symbol: String,
    pub wind_unit: String,
    pub current: Option<CurrentConditions>,
    pub forecast: Vec<ForecastDay>,
    pub fetched_at: String,
    pub source_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DataFreshness {
    Fresh,
    Stale,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct WeatherSnapshot {
    pub data: WeatherData,
    pub freshness: DataFreshness,
    pub source_error: Option<String>,
    pub cache_age_minutes: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AppSettings {
    pub temperature_unit: TemperatureUnit,
    pub selected_panel: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            temperature_unit: TemperatureUnit::Fahrenheit,
            selected_panel: "weather".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CacheEnvelope {
    pub saved_at: String,
    pub data: WeatherData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenMeteoResponse {
    pub timezone: String,
    pub current: Option<OpenMeteoCurrent>,
    pub daily: OpenMeteoDaily,
    pub current_units: Option<OpenMeteoCurrentUnits>,
    pub daily_units: Option<OpenMeteoDailyUnits>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenMeteoCurrent {
    pub temperature_2m: f64,
    pub apparent_temperature: Option<f64>,
    pub weather_code: i32,
    pub wind_speed_10m: Option<f64>,
    pub wind_direction_10m: Option<i32>,
    pub is_day: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenMeteoCurrentUnits {
    pub temperature_2m: Option<String>,
    pub wind_speed_10m: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenMeteoDailyUnits {
    pub temperature_2m_max: Option<String>,
    pub wind_speed_10m_max: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenMeteoDaily {
    pub time: Vec<String>,
    pub weather_code: Vec<i32>,
    pub temperature_2m_max: Vec<f64>,
    pub temperature_2m_min: Vec<f64>,
    pub precipitation_probability_max: Option<Vec<Option<i32>>>,
    pub wind_speed_10m_max: Option<Vec<Option<f64>>>,
    pub wind_direction_10m_dominant: Option<Vec<Option<i32>>>,
    pub sunrise: Option<Vec<Option<String>>>,
    pub sunset: Option<Vec<Option<String>>>,
}
