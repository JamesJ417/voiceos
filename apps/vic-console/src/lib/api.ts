import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, AppSettings, WeatherSnapshot } from "../shared/types";
export const consoleApi = {
  weather: (forceRefresh=false) => invoke<WeatherSnapshot>("get_weather", { forceRefresh }),
  config: () => invoke<AppConfig>("get_config"),
  settings: () => invoke<AppSettings>("get_settings"),
  updateSettings: (settings:AppSettings) => invoke<void>("update_settings", { settings }),
};
