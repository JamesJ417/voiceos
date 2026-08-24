export type PanelId = "weather" | "tasks" | "news" | "business";
export type TemperatureUnit = "fahrenheit" | "celsius";
export type ForecastDay = { date:string; weather_code:number; high:number; low:number; precipitation_probability:number|null; wind_speed:number|null; wind_direction:number|null; sunrise:string|null; sunset:string|null };
export type WeatherData = { location_name:string; timezone:string; temperature_unit:TemperatureUnit; temperature_symbol:string; wind_unit:string; current:{temperature:number; apparent_temperature:number|null; weather_code:number; wind_speed:number|null; wind_direction:number|null; is_day:boolean}|null; forecast:ForecastDay[]; fetched_at:string; source_name:string };
export type WeatherSnapshot = { data:WeatherData; freshness:"fresh"|"stale"; source_error:string|null; cache_age_minutes:number|null };
export type AppConfig = { location_name:string; latitude:number; longitude:number; temperature_unit:TemperatureUnit; refresh_interval_minutes:number; api_endpoint:string };
export type AppSettings = { temperature_unit:TemperatureUnit; selected_panel:string };
export type LoadState = { phase:"loading"|"refreshing"|"success"|"stale"|"error"; snapshot:WeatherSnapshot|null; error:string|null };
