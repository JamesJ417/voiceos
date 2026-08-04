# VoiceOS Carbon Command kiosk

This responsive interface is the browser and HP touchscreen client for VoiceOS.
It connects to the existing gateway and shared server-owned conversation. The
Command surface supports browser speech recognition, typed turns, spoken
responses, playback speed, repeat, copy, private document upload, and approval
decisions. History and System load live audit, provider, and host-health data.

Run `npm run dev` for a local preview and `npm run build` for the production
bundle. Open Connection settings, save the HTTPS Tailscale gateway address, and
enter a one-time enrollment code. Browser credentials are scoped to that
browser and can be forgotten from the same settings panel.

The Python gateway permits local browser origins by default. For a deployed web
client, configure the exact origin before restarting the gateway:

```text
VOICEOS_WEB_ORIGINS=https://your-private-web-client.example
```

Multiple exact origins may be separated with commas. Wildcards are not
accepted. Keep the gateway on HTTPS when the web client is served over HTTPS.
