# VIC Console

VIC Console is a Tauri 2 desktop information application for Omarchy/Linux. It renders structured information in its own frameless, resizable application window; it does not embed external websites or launch a visible browser.

The first panel is a real ten-day forecast for Newberry, Florida using Open-Meteo. The Rust backend owns network access, response validation, local caching, configuration, and offline fallback. Tasks, News, and Business are reserved panel boundaries and are explicitly marked as coming soon.

## Prerequisites

- Rust and Cargo
- Node.js 22 or newer and npm
- Tauri Linux dependencies: WebKitGTK 4.1, GTK3, librsvg, OpenSSL, and standard build tools

On Arch/Omarchy, the relevant packages are normally `webkit2gtk-4.1`, `gtk3`, `librsvg`, `openssl`, `base-devel`, and `patchelf`.

## Development

```bash
cd apps/vic-console
npm install
npm run tauri dev
```

`tauri dev` opens only the native VIC Console window. Vite runs as its internal asset server; no browser tab is launched.

Frontend checks:

```bash
npm run typecheck
npm run lint
npm test
npm run build
```

Backend checks:

```bash
cargo fmt --all -- --check
cargo test -p vic-console
cargo check -p vic-console
```

## Production build and launch

```bash
cd apps/vic-console
npm ci
npm run tauri build
```

The native binary is written to the workspace's `target/release/vic-console`. Installable `.deb` and `.rpm` packages are written under the workspace's `target/release/bundle/`. From this directory, launch an unpackaged build with `../../target/release/vic-console`.

The repository includes `vic-console.desktop` as a desktop-entry template. For an unpackaged binary, copy it to `~/.local/share/applications/` and adjust `Exec` to an absolute path.

While the native application is running it accepts exactly two local VoiceOS
commands—`show_weather` and `refresh_dashboard`—through the owner-only Unix
socket at `$XDG_RUNTIME_DIR/voiceos/vic-console.sock`. Set
`VOICEOS_CONSOLE_SOCKET` to the same absolute path in both processes only when
an alternate runtime location is required. See the
[voice integration boundary](../../docs/vic-console-voice-integration.md) for
the protocol and audit guarantees.

## Optional autostart

Autostart is intentionally not installed automatically. To opt in, copy the adjusted desktop entry to `~/.config/autostart/vic-console.desktop`. Remove that file to disable autostart.

## Configuration and data

Tauri resolves the Linux application-data directory for `org.omarchy.vic-console`. The backend uses `config.json`, `settings.json`, and `weather-cache.json`. Defaults are Newberry, Florida (`29.6464`, `-82.6065`), Fahrenheit, a 30-minute refresh interval, and Open-Meteo. No API credential is required. Cache writes are atomic and a failed request falls back to saved data with a visible stale warning.

## Troubleshooting

- `cargo: command not found`: install Rust through rustup or Omarchy's development setup.
- `npm: command not found`: install Node.js 22+ and npm.
- Missing `webkit2gtk-4.1.pc`: install WebKitGTK 4.1 development packages.
- Blank window: run `npm run build` and inspect the terminal for CSP or frontend errors.
- Cached/stale weather: verify access to `api.open-meteo.com`; source details show the last fetch.
- No cached forecast: use **Refresh weather** once while online.
- `vic_console_unavailable`: start VIC Console and confirm both processes use the same `XDG_RUNTIME_DIR` or `VOICEOS_CONSOLE_SOCKET`.

See [architecture](../../docs/vic-console-architecture.md) and the [voice integration boundary](../../docs/vic-console-voice-integration.md).
