use std::path::Path;
use std::sync::Mutex;

use crate::{CaptureSource, ConversationStore, PersonalCapture, StoreError};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
pub const MAX_FIELDY_BODY_BYTES: usize = 1_048_576;
pub const DEFAULT_FIELDY_RETENTION_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldyTranscriptEvent {
    pub event_id: String,
    pub occurred_at: String,
    pub transcript: String,
    pub recording_id: Option<String>,
    pub session_id: Option<String>,
    pub speakers: Vec<serde_json::Value>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum FieldyWebhookError {
    #[error("invalid Fieldy event: {0}")]
    InvalidInput(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("database lock is poisoned")]
    LockPoisoned,
}

impl FieldyTranscriptEvent {
    pub fn validate(&self) -> Result<(), FieldyWebhookError> {
        if self.event_id.trim().is_empty() {
            return Err(FieldyWebhookError::InvalidInput(
                "event_id is required".into(),
            ));
        }
        if self.transcript.trim().is_empty() {
            return Err(FieldyWebhookError::InvalidInput(
                "transcript is required".into(),
            ));
        }
        if self.transcript.chars().count() > 100_000 {
            return Err(FieldyWebhookError::InvalidInput(
                "transcript is too large".into(),
            ));
        }
        if chrono::DateTime::parse_from_rfc3339(&self.occurred_at).is_err() {
            return Err(FieldyWebhookError::InvalidInput(
                "occurred_at must be RFC3339".into(),
            ));
        }
        Ok(())
    }
}

impl ConversationStore {
    pub fn capture_fieldy_event(
        &self,
        owner_id: &str,
        event: &FieldyTranscriptEvent,
        retention: Duration,
    ) -> Result<PersonalCapture, StoreError> {
        event
            .validate()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        let occurred_at = chrono::DateTime::parse_from_rfc3339(&event.occurred_at)
            .map_err(|_| StoreError::InvalidInput("occurred_at must be RFC3339".into()))?
            .with_timezone(&Utc);
        self.capture_personal_input(
            owner_id,
            CaptureSource::fieldy(event.event_id.trim()),
            &event.transcript,
            occurred_at,
            retention,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldyTranscriptIntake {
    pub intake_id: String,
    pub owner_id: String,
    pub event_id: String,
    pub normalized_transcript: String,
    pub status: String,
}

pub fn verify_fieldy_signature(secret: &[u8], body: &[u8], header: &str) -> bool {
    let Some(encoded) = header.strip_prefix("sha256=") else {
        return false;
    };
    if encoded.len() != 64 || !encoded.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let Ok(expected) = hex::decode(encoded) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

pub struct FieldyWebhookStore {
    connection: Mutex<Connection>,
}

impl FieldyWebhookStore {
    pub fn in_memory() -> Result<Self, FieldyWebhookError> {
        Self::open_connection(Connection::open_in_memory()?)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FieldyWebhookError> {
        Self::open_connection(Connection::open(path)?)
    }
    fn open_connection(connection: Connection) -> Result<Self, FieldyWebhookError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        crate::schema::migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    pub fn intake(
        &self,
        owner_id: &str,
        event: &FieldyTranscriptEvent,
        raw_payload: &[u8],
    ) -> Result<FieldyTranscriptIntake, FieldyWebhookError> {
        if owner_id.trim().is_empty() {
            return Err(FieldyWebhookError::InvalidInput(
                "owner_id is required".into(),
            ));
        }
        if raw_payload.len() > MAX_FIELDY_BODY_BYTES {
            return Err(FieldyWebhookError::InvalidInput(
                "request body is too large".into(),
            ));
        }
        event.validate()?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| FieldyWebhookError::LockPoisoned)?;
        if let Some(found) = connection.query_row("SELECT intake_id, normalized_transcript, status FROM fieldy_transcript_intake WHERE owner_id=?1 AND source='fieldy' AND event_id=?2", params![owner_id.trim(), event.event_id.trim()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).optional()? {
            return Ok(FieldyTranscriptIntake { intake_id: found.0, owner_id: owner_id.trim().into(), event_id: event.event_id.trim().into(), normalized_transcript: found.1, status: found.2 });
        }
        let id = Uuid::new_v4().to_string();
        let received_at = Utc::now();
        let expires_at = received_at + chrono::Duration::days(DEFAULT_FIELDY_RETENTION_DAYS);
        connection.execute("INSERT INTO fieldy_transcript_intake (intake_id, owner_id, source, event_id, occurred_at, received_at, raw_payload_json, normalized_transcript, status, expires_at) VALUES (?1,?2,'fieldy',?3,?4,?5,?6,?7,'received',?8)", params![id, owner_id.trim(), event.event_id.trim(), event.occurred_at, received_at.to_rfc3339(), String::from_utf8_lossy(raw_payload).as_ref(), event.transcript.trim(), expires_at.to_rfc3339()])?;
        Ok(FieldyTranscriptIntake {
            intake_id: id,
            owner_id: owner_id.trim().into(),
            event_id: event.event_id.trim().into(),
            normalized_transcript: event.transcript.trim().into(),
            status: "received".into(),
        })
    }
    pub fn expires_at(&self, intake_id: &str) -> Result<chrono::DateTime<Utc>, FieldyWebhookError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| FieldyWebhookError::LockPoisoned)?;
        let value: String = connection.query_row(
            "SELECT expires_at FROM fieldy_transcript_intake WHERE intake_id=?1",
            [intake_id],
            |row| row.get(0),
        )?;
        chrono::DateTime::parse_from_rfc3339(&value)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| FieldyWebhookError::InvalidInput("stored expiry is invalid".into()))
    }
    pub fn count(&self, owner_id: &str) -> Result<usize, FieldyWebhookError> {
        let c = self
            .connection
            .lock()
            .map_err(|_| FieldyWebhookError::LockPoisoned)?;
        Ok(c.query_row(
            "SELECT COUNT(*) FROM fieldy_transcript_intake WHERE owner_id=?1",
            [owner_id],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }
}
