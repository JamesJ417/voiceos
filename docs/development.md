# Development setup

## Windows laptop

Required tools:

- Git
- Python 3.11+
- JDK 17
- Android SDK Platform 36
- Android SDK Build Tools 36.0.0
- Android SDK Platform Tools

Android Studio is optional for command-line builds but recommended for device debugging and UI inspection.

## Pixel connection

For the first USB deployment:

1. Enable Android developer options on the Pixel.
2. Enable USB debugging.
3. Connect the phone and approve this laptop's debugging key.
4. Run `adb devices` and verify that the phone is listed as `device`.
5. Build and install the debug APK with the gateway URL supplied as `AIOS_SERVER_URL`.

The final app will not require developer mode or USB debugging.

## Network stages

1. Emulator to laptop: `http://10.0.2.2:8787`.
2. Pixel to laptop over LAN: laptop IPv4 address and Windows Firewall rule for port 8787.
3. Pixel to laptop or mining rig: Tailscale Serve with an ACL allowing only the Pixel and development laptop.
4. Production: TLS plus application-layer device authentication, even inside Tailscale.

Do not expose port 8787 through the home router.

## Persistent gateway enrollment

The Android app accepts a private enrollment link:

```text
voiceos://enroll?gateway=https%3A%2F%2Fvoiceos-rig.example.ts.net
```

Only HTTPS gateway addresses without embedded user credentials are accepted. The selected address is persisted in private app preferences and health-checked when the app opens. A one-time enrollment QR adds a `code` parameter. The Pixel exchanges it for a device credential and encrypts that credential using Android Keystore.

Transient health and turn requests retry automatically with short exponential delays. Deliberate HTTP failures, including 401 responses, are not treated as reconnectable network errors.

For the current development laptop, Tailscale Serve proxies the loopback gateway:

```powershell
tailscale serve --bg --yes 8787
tailscale serve status
```

Disable it with `tailscale serve --https=443 off`.
