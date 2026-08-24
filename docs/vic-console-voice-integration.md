# VIC Console voice integration boundary

VIC Console works without VoiceOS. It does not listen to microphones or infer voice commands.

The Tauri backend exposes `dispatch_console_command` and emits `vic-console-command`. Current commands are `show_weather` and `refresh_dashboard`. The React shell selects Weather and optionally refreshes it.

VoiceOS registers VIC Console and its `show_weather` and `refresh_dashboard`
commands in `contracts/component-registry.json`. Every authenticated client
discovers that registration through `GET /v1/client/bootstrap`, which Python
proxies to the Rust control plane during migration. Touch displays the live
registration in its System view.

The Console owns an allowlisted version-1 Unix-socket listener at
`$XDG_RUNTIME_DIR/voiceos/vic-console.sock`. The containing directory is mode
`0700`, the socket is mode `0600`, and `VOICEOS_CONSOLE_SOCKET` may override the
path only with an absolute path. Startup refuses to replace a regular file or
an active listener. Messages are bounded JSON records; unknown commands,
unknown fields, oversized bodies, and unsupported protocol versions are
denied. No IPC message can inject JavaScript, synthesize a click, or carry
arbitrary executable text.

The authenticated public command route is `POST /v1/console/commands`. Python
proxies it to the Rust control plane while the migration ingress remains in
place. Rust validates the typed command, delivers it over the owner-only
socket, requires a matching acknowledgement, and appends either
`console.command.completed` or `console.command.failed` to the authoritative
execution log. A failed or stopped Console produces `503`; VoiceOS never claims
delivery without the acknowledgement.

VIC can also resolve “Show the weather” and “Refresh the VIC Console dashboard”
through the canonical ontology before the reasoning model runs. The same two
commands are exposed as `console.show_weather` and
`console.refresh_dashboard` provider tools, with empty argument schemas. They
need no confirmation because they change only the local information display;
they cannot alter settings, contact anyone, launch arbitrary programs, or
perform an external commitment.

The adapter remains outside the weather service, so VIC Console still works as
a standalone weather application when VoiceOS is stopped.
