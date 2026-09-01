use std::path::Path;

use axum::http::{HeaderMap, StatusCode};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::state::AppState;

use super::error::{ApiResult, api_error};

pub(crate) fn authenticate(state: &AppState, headers: &HeaderMap) -> ApiResult<String> {
    if !state.require_device_auth {
        // Explicit local-development mode has one fixed identity. Never trust a
        // caller-controlled device header as an authorization identity.
        return Ok("development-device".to_owned());
    }

    let token = header_text(headers, "authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "device_authentication_required"))?;
    authenticate_legacy_token(&state.legacy_audit_path, token)
        .map_err(|_| api_error(StatusCode::UNAUTHORIZED, "invalid_device_credential"))?
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "invalid_device_credential"))
}

pub(crate) fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
}

fn authenticate_legacy_token(path: &Path, token: &str) -> rusqlite::Result<Option<String>> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    connection
        .query_row(
            "SELECT device_id FROM devices WHERE token_hash=?1 AND disabled_at IS NULL",
            [token_hash],
            |row| row.get(0),
        )
        .optional()
}
