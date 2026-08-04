# HP Carbon Command kiosk

This package boots the HP directly into the private VoiceOS web interface under
the unprivileged `voiceos-kiosk` account. The account has USB audio, display,
touch, and input-device access, but no sudo rights or model credentials.

On Ubuntu 24.04 LTS, install `cage`, Chromium, and Tailscale. Copy this directory
to the HP and run:

```bash
sudo ./install.sh
sudoedit /etc/voiceos/kiosk.env
sudo systemctl start voiceos-kiosk
```

Set `VOICEOS_KIOSK_URL` to the Carbon Command URL reachable over Tailscale.
Open Settings on the kiosk, enter a one-time VoiceOS enrollment code, and grant
microphone permission. Chromium keeps that site permission in
`/var/lib/voiceos-kiosk/browser` across reboots.

Use `systemctl status voiceos-kiosk` and `journalctl -u voiceos-kiosk` for health
checks. To return tty1 to a normal login, disable the unit and re-enable
`getty@tty1.service`.
