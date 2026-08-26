use std::path::Path;
use std::sync::Mutex;

use crate::{ConversationStore, PersonalCapture, StoreError};
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
pub const FIELDY_CONVERSATION_GAP_SECONDS: i64 = 300;

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
        if owner_id.trim().is_empty() {
            return Err(StoreError::InvalidInput("owner is required".into()));
        }
        let occurred_at = chrono::DateTime::parse_from_rfc3339(&event.occurred_at)
            .map_err(|_| StoreError::InvalidInput("occurred_at must be RFC3339".into()))?
            .with_timezone(&Utc);
        let received_at = Utc::now();
        let expires_at = received_at + retention;
        let event_id = event.event_id.trim();
        let connection = self.connection()?;

        if let Some(capture_id) = connection
            .query_row(
                "SELECT a.capture_id FROM fieldy_conversation_chunks c \
                 JOIN fieldy_conversation_assemblies a ON a.assembly_id=c.assembly_id \
                 WHERE c.owner_id=?1 AND c.event_id=?2",
                params![owner_id.trim(), event_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            drop(connection);
            return self
                .personal_capture(owner_id, &capture_id)?
                .ok_or_else(|| StoreError::InvalidInput("assembled capture is missing".into()));
        }

        let event_threshold =
            (occurred_at - Duration::seconds(FIELDY_CONVERSATION_GAP_SECONDS)).to_rfc3339();
        let received_threshold =
            (received_at - Duration::seconds(FIELDY_CONVERSATION_GAP_SECONDS)).to_rfc3339();
        let active = connection
            .query_row(
                "SELECT assembly_id,capture_id,started_at FROM fieldy_conversation_assemblies \
                 WHERE owner_id=?1 AND status='assembling' AND last_event_at>=?2 \
                 AND last_received_at>=?3 AND (\
                    (?4 IS NOT NULL AND EXISTS(SELECT 1 FROM fieldy_conversation_chunks c \
                        WHERE c.assembly_id=fieldy_conversation_assemblies.assembly_id AND c.session_id=?4)) \
                    OR (?4 IS NULL AND ?5 IS NOT NULL AND EXISTS(SELECT 1 FROM fieldy_conversation_chunks c \
                        WHERE c.assembly_id=fieldy_conversation_assemblies.assembly_id AND c.recording_id=?5)) \
                    OR (?4 IS NULL AND ?5 IS NULL)\
                 ) ORDER BY last_event_at DESC LIMIT 1",
                params![
                    owner_id.trim(),
                    event_threshold,
                    received_threshold,
                    event.session_id,
                    event.recording_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let transaction = connection.unchecked_transaction()?;
        let (assembly_id, capture_id, started_at, is_new) = if let Some(active) = active {
            (active.0, active.1, active.2, false)
        } else {
            let assembly_id = Uuid::new_v4().to_string();
            let capture_id = Uuid::new_v4().to_string();
            let started_at = occurred_at.to_rfc3339();
            transaction.execute(
                "INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) \
                 ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
                params![owner_id.trim(), received_at.to_rfc3339()],
            )?;
            transaction.execute(
                "INSERT INTO personal_captures(\
                    capture_id,owner_id,source,source_id,raw_content,display_text,structured_content_json,\
                    status,created_at,expires_at,audit_id\
                 ) VALUES(?1,?2,'fieldy',?3,'','',NULL,'received',?4,?5,?6)",
                params![
                    capture_id,
                    owner_id.trim(),
                    event_id,
                    started_at,
                    expires_at.to_rfc3339(),
                    format!("fieldy-assembly:{assembly_id}"),
                ],
            )?;
            transaction.execute(
                "INSERT INTO fieldy_conversation_assemblies(\
                    assembly_id,owner_id,capture_id,status,started_at,last_event_at,last_received_at,\
                    chunk_count,created_at,updated_at\
                 ) VALUES(?1,?2,?3,'assembling',?4,?4,?5,0,?5,?5)",
                params![
                    assembly_id,
                    owner_id.trim(),
                    capture_id,
                    started_at,
                    received_at.to_rfc3339(),
                ],
            )?;
            (assembly_id, capture_id, started_at, true)
        };

        transaction.execute(
            "INSERT INTO fieldy_conversation_chunks(\
                owner_id,event_id,assembly_id,occurred_at,received_at,transcript,recording_id,session_id,speakers_json,metadata_json\
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                owner_id.trim(),
                event_id,
                assembly_id,
                occurred_at.to_rfc3339(),
                received_at.to_rfc3339(),
                event.transcript.trim(),
                event.recording_id,
                event.session_id,
                serde_json::to_string(&event.speakers)?,
                serde_json::to_string(&event.metadata)?,
            ],
        )?;
        let (assembled, structured, chunk_count, last_event_at) =
            assembled_fieldy_content(&transaction, &assembly_id, &started_at)?;
        transaction.execute(
            "UPDATE personal_captures SET raw_content=?1,display_text=?1,structured_content_json=?2,\
             expires_at=?3 WHERE capture_id=?4 AND owner_id=?5 AND status='received'",
            params![
                assembled,
                structured.to_string(),
                expires_at.to_rfc3339(),
                capture_id,
                owner_id.trim(),
            ],
        )?;
        transaction.execute(
            "UPDATE fieldy_conversation_assemblies SET last_event_at=?1,last_received_at=?2,\
             chunk_count=?3,updated_at=?2 WHERE assembly_id=?4",
            params![
                last_event_at,
                received_at.to_rfc3339(),
                chunk_count,
                assembly_id,
            ],
        )?;
        transaction.execute(
            "INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) \
             VALUES(?1,?2,?3,'voiceos-core',?4,?5)",
            params![
                owner_id.trim(),
                format!("fieldy-assembly:{assembly_id}"),
                if is_new {
                    "personal.fieldy_assembly_started"
                } else {
                    "personal.fieldy_chunk_appended"
                },
                serde_json::json!({
                    "capture_id": capture_id,
                    "assembly_id": assembly_id,
                    "event_id": event_id,
                    "chunk_count": chunk_count,
                })
                .to_string(),
                received_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.personal_capture(owner_id, &capture_id)?
            .ok_or_else(|| StoreError::InvalidInput("assembled capture is missing".into()))
    }
}

fn assembled_fieldy_content(
    connection: &Connection,
    assembly_id: &str,
    started_at: &str,
) -> Result<(String, serde_json::Value, usize, String), StoreError> {
    let mut statement = connection.prepare(
        "SELECT event_id,occurred_at,transcript,recording_id,session_id,speakers_json,metadata_json \
         FROM fieldy_conversation_chunks WHERE assembly_id=?1 \
         ORDER BY occurred_at,event_id",
    )?;
    let rows = statement.query_map([assembly_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    let mut assembled = String::new();
    let mut chunks = Vec::new();
    let mut last_event_at = started_at.to_owned();
    for row in rows {
        let (event_id, occurred_at, transcript, recording_id, session_id, speakers, metadata) =
            row?;
        assembled = merge_fieldy_transcript(&assembled, &transcript);
        let speakers: serde_json::Value = serde_json::from_str(&speakers)?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
        chunks.push(serde_json::json!({
            "event_id": event_id,
            "occurred_at": occurred_at,
            "transcript": transcript,
            "recording_id": recording_id,
            "session_id": session_id,
            "speakers": speakers,
            "metadata": metadata,
        }));
        last_event_at = occurred_at;
    }
    let chunk_count = chunks.len();
    let structured = serde_json::json!({
        "kind": "fieldy_conversation",
        "assembly_id": assembly_id,
        "started_at": started_at,
        "last_event_at": last_event_at,
        "chunk_count": chunk_count,
        "chunks": chunks,
    });
    Ok((assembled, structured, chunk_count, last_event_at))
}

fn merge_fieldy_transcript(existing: &str, incoming: &str) -> String {
    let existing = existing.split_whitespace().collect::<Vec<_>>();
    let incoming = incoming.split_whitespace().collect::<Vec<_>>();
    if existing.is_empty() {
        return incoming.join(" ");
    }
    if incoming.is_empty() {
        return existing.join(" ");
    }
    if incoming == existing
        || existing
            .windows(incoming.len())
            .any(|window| window == incoming)
    {
        return existing.join(" ");
    }
    if incoming
        .windows(existing.len())
        .any(|window| window == existing)
    {
        return incoming.join(" ");
    }
    let overlap = (1..=existing.len().min(incoming.len()))
        .rev()
        .find(|size| existing[existing.len() - size..] == incoming[..*size])
        .unwrap_or(0);
    existing
        .into_iter()
        .chain(incoming.into_iter().skip(overlap))
        .collect::<Vec<_>>()
        .join(" ")
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
