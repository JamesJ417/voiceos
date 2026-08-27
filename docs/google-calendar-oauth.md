# Google Calendar OAuth boundary

Status: authorization intentionally blocked before browser launch until VoiceOS has an approved encrypted secret-store adapter. This slice makes that condition machine-readable; it does not contact Google, generate credentials, exchange an authorization code, or persist token material.

## Current UI contract

Authenticated `GET /v1/integrations/google-calendar/status` remains owner-scoped through the gateway's primary-owner state and returns the existing connection metadata only. Until a secret store exists, it also returns:

```json
{
  "authorization_ready": false,
  "error": "google_calendar_secret_store_unavailable",
  "next_step": "configure_secret_store"
}
```

The UI must render configuration guidance rather than a Connect button. A stored metadata row is not evidence of usable authorization: `authorization_ready` remains `false` until encrypted token read-back has succeeded.

## Required secure next slice

The repository has no established encrypted server-side secret store today; SQLite persistence is metadata-only and must never gain plaintext `access_token`, `refresh_token`, authorization `code`, PKCE verifier, or client secret columns. Do not put these values in committed `.env` files, manifests, logs, API responses, errors, or audit payloads.

Before enabling browser authorization, introduce and test an approved local OS-backed secret-store adapter (for example, the platform credential manager) with opaque per-owner secret references. The SQLite connection record may contain only the opaque reference plus non-secret Google account metadata. `disconnect` must delete both the owner-scoped metadata and its secret-store entry.

Once that adapter is present, the desktop/local OAuth flow must be:

1. The authenticated owner requests begin; the gateway validates a configured Google desktop OAuth client ID and exact loopback redirect URI, then creates a cryptographically random state and an RFC 7636 PKCE verifier/challenge. Keep the state and verifier in process memory (or encrypted storage) only, bound to the authenticated owner, redirect URI, and short expiry.
2. The UI opens the returned Google authorization URL. Request only the calendar scopes approved for the product; use `response_type=code`, exact redirect URI, `state`, `code_challenge`, and `code_challenge_method=S256`. Do not make a provider call from the begin endpoint.
3. The loopback callback accepts only the exact configured redirect URI. It must reject missing, expired, unknown, replayed, or owner-mismatched state before reading/using a code. It consumes state exactly once and performs the token exchange only over TLS with the original PKCE verifier.
4. Validate the token response and account identity, write token material only through the approved encrypted secret-store adapter, then persist owner-scoped non-secret metadata. Return no token material to the UI. On any secret-store or exchange failure, remove the pending state and fail closed.

## Operator prerequisites before the screen authorization

1. Approve and configure a local encrypted secret-store adapter for the gateway process.
2. Create a Google OAuth desktop/loopback client in Google Cloud Console and configure the exact local redirect URI; keep any client secret in the approved secret store, not source control or a committed environment file.
3. Configure the gateway privately with the client identifier and redirect URI; enable the begin/callback routes only after startup verifies secret-store read/write/delete capability.
4. Have the owner physically approve the Google consent screen. Verify success by reading the owner-scoped status endpoint, then test disconnect and verify the encrypted secret is removed.
