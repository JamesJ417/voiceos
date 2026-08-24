# VIC Console architecture

`apps/vic-console` is deliberately independent from Touch. It shares the Rust workspace and visual values but not conversation state or browser runtime.

- **Desktop shell:** Tauri 2 creates one frameless, resizable native window. The custom title bar has only minimize, toggle-maximize, and close permissions.
- **Backend:** Rust builds the Open-Meteo URL from validated configuration, applies a 12-second timeout, parses typed responses, and transforms exactly ten daily records.
- **Persistence:** Tauri's application-data directory stores configuration, settings, and an atomic JSON cache. Source failure returns cached data as stale; malformed data is never cached.
- **Presentation:** React components render navigation and loading, refreshing, success, stale, and terminal-error states. No component fetches weather directly.
- **State:** A reducer makes refresh transitions explicit. Selected panel and temperature unit are persisted through backend commands.

```text
WeatherDashboard -> typed Tauri command -> WeatherService
  -> validated AppConfig -> Open-Meteo with timeout
  -> typed transformation -> atomic cache
  -> fresh/stale WeatherSnapshot -> reducer -> presentation
```

The source URL stays in configuration and is visible only through source details. No website is embedded and no backend path launches a browser.

To add a panel, add a `PanelId`, navigation descriptor, and panel component. Backend panels should follow the same typed command, isolated service, validation, cache policy, explicit state, and test pattern. Tasks should reuse VoiceOS's authenticated task APIs; News and Business need their own source and trust policies.

Security properties include HTTPS-only configuration, no credentials or browser cookies, bounded requests, rejection of incomplete forecasts, cache updates only after validation, serializable failures, and structured data rather than source HTML.
