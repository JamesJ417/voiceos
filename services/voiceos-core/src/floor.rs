use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde_json::json;
use uuid::Uuid;

use crate::{ConversationFloor, ConversationStore, StoreError};

const MIN_TTL_SECONDS: i64 = 10;
const MAX_TTL_SECONDS: i64 = 120;

impl ConversationStore {
    pub fn conversation_floor(
        &self,
        owner_id: &str,
    ) -> Result<Option<ConversationFloor>, StoreError> {
        let now_unix = Utc::now().timestamp();
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "UPDATE conversation_floors SET lease_id=NULL, holder_device_id=NULL, holder_display_name=NULL, phase='idle', partial_transcript=NULL, response_text=NULL, revision=revision+1, updated_at=?2, expires_at_unix=0 WHERE owner_id=?1 AND holder_device_id IS NOT NULL AND expires_at_unix<=?3",
            params![owner_id.trim(), now, now_unix],
        )?;
        connection
            .query_row(
                "SELECT owner_id, conversation_id, lease_id, holder_device_id, holder_display_name, phase, partial_transcript, response_text, revision, acquired_at, updated_at, expires_at_unix FROM conversation_floors WHERE owner_id=?1",
                [owner_id.trim()],
                |row| floor_row(row, now_unix),
            )
            .optional()
            .map_err(StoreError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn change_conversation_floor(
        &self,
        owner_id: &str,
        conversation_id: &str,
        device_id: &str,
        display_name: Option<&str>,
        action: &str,
        phase: Option<&str>,
        partial_transcript: Option<&str>,
        response_text: Option<&str>,
        ttl_seconds: i64,
    ) -> Result<ConversationFloor, StoreError> {
        self.change_conversation_floor_fenced(
            owner_id,
            conversation_id,
            device_id,
            display_name,
            action,
            phase,
            partial_transcript,
            response_text,
            ttl_seconds,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn change_conversation_floor_fenced(
        &self,
        owner_id: &str,
        conversation_id: &str,
        device_id: &str,
        display_name: Option<&str>,
        action: &str,
        phase: Option<&str>,
        partial_transcript: Option<&str>,
        response_text: Option<&str>,
        ttl_seconds: i64,
        expected_lease_id: Option<&str>,
        expected_revision: Option<i64>,
    ) -> Result<ConversationFloor, StoreError> {
        if owner_id.trim().is_empty()
            || conversation_id.trim().is_empty()
            || device_id.trim().is_empty()
        {
            return Err(StoreError::InvalidInput(
                "owner_id, conversation_id, and device_id are required".to_owned(),
            ));
        }
        let action = action.trim();
        if !matches!(action, "claim" | "update" | "release") {
            return Err(StoreError::InvalidInput(
                "floor action must be claim, update, or release".to_owned(),
            ));
        }
        let phase = phase.unwrap_or("listening").trim();
        if !matches!(phase, "idle" | "listening" | "processing" | "speaking") {
            return Err(StoreError::InvalidInput(
                "invalid conversation floor phase".to_owned(),
            ));
        }

        let now_clock = Utc::now();
        let now = now_clock.to_rfc3339();
        let now_unix = now_clock.timestamp();
        let expires_at = now_unix + ttl_seconds.clamp(MIN_TTL_SECONDS, MAX_TTL_SECONDS);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let existing: Option<(Option<String>, i64, Option<String>, i64)> = transaction
            .query_row(
                "SELECT holder_device_id, expires_at_unix, lease_id, revision FROM conversation_floors WHERE owner_id=?1",
                [owner_id.trim()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let currently_owned = existing.as_ref().is_some_and(|(holder, expiry, _, _)| {
            holder.as_deref() == Some(device_id.trim()) && *expiry > now_unix
        });
        if action != "claim" {
            if expected_lease_id.is_some_and(|expected| {
                existing
                    .as_ref()
                    .and_then(|(_, _, lease, _)| lease.as_deref())
                    != Some(expected)
            }) {
                return Err(StoreError::InvalidInput(
                    "conversation_floor_lease_mismatch".to_owned(),
                ));
            }
            if expected_revision.is_some_and(|expected| {
                existing.as_ref().map(|(_, _, _, revision)| *revision) != Some(expected)
            }) {
                return Err(StoreError::InvalidInput(
                    "conversation_floor_revision_mismatch".to_owned(),
                ));
            }
        }
        if action != "claim"
            && existing.as_ref().is_some_and(|(holder, expiry, _, _)| {
                holder.is_some() && (!currently_owned && *expiry > now_unix)
            })
        {
            return Err(StoreError::InvalidInput(
                "conversation_floor_not_owned".to_owned(),
            ));
        }

        let next_lease_id = if action == "release" {
            None
        } else if action == "claim" || !currently_owned {
            Some(Uuid::new_v4().to_string())
        } else {
            transaction
                .query_row(
                    "SELECT lease_id FROM conversation_floors WHERE owner_id=?1",
                    [owner_id.trim()],
                    |row| row.get(0),
                )
                .optional()?
                .flatten()
        };
        let next_phase = if action == "release" { "idle" } else { phase };
        let next_expiry = if action == "release" { 0 } else { expires_at };
        let next_holder = (action != "release").then_some(device_id.trim());
        let next_display =
            (action != "release").then_some(display_name.unwrap_or("VoiceOS device").trim());
        let acquired_at = (action != "release").then_some(now.as_str());
        transaction.execute(
            "INSERT INTO conversation_floors(owner_id, conversation_id, lease_id, holder_device_id, holder_display_name, phase, partial_transcript, response_text, revision, acquired_at, updated_at, expires_at_unix) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10,?11) ON CONFLICT(owner_id) DO UPDATE SET conversation_id=excluded.conversation_id, lease_id=excluded.lease_id, holder_device_id=excluded.holder_device_id, holder_display_name=excluded.holder_display_name, phase=excluded.phase, partial_transcript=excluded.partial_transcript, response_text=excluded.response_text, revision=conversation_floors.revision+1, acquired_at=CASE WHEN excluded.holder_device_id IS NULL THEN NULL WHEN conversation_floors.holder_device_id=excluded.holder_device_id THEN conversation_floors.acquired_at ELSE excluded.acquired_at END, updated_at=excluded.updated_at, expires_at_unix=excluded.expires_at_unix",
            params![owner_id.trim(), conversation_id.trim(), next_lease_id, next_holder, next_display, next_phase, partial_transcript.filter(|value| !value.is_empty()), response_text.filter(|value| !value.is_empty()), acquired_at, now, next_expiry],
        )?;
        let floor = transaction.query_row(
            "SELECT owner_id, conversation_id, lease_id, holder_device_id, holder_display_name, phase, partial_transcript, response_text, revision, acquired_at, updated_at, expires_at_unix FROM conversation_floors WHERE owner_id=?1",
            [owner_id.trim()],
            |row| floor_row(row, now_unix),
        )?;
        transaction.execute(
            "INSERT INTO execution_events(owner_id, stream_id, event_type, actor, payload_json, occurred_at) VALUES(?1,?2,'conversation.floor.changed',?3,?4,?5)",
            params![owner_id.trim(), conversation_id.trim(), format!("device:{}", device_id.trim()), json!({"action": action, "floor": floor}).to_string(), now],
        )?;
        transaction.commit()?;
        Ok(floor)
    }
}

fn floor_row(row: &rusqlite::Row<'_>, now_unix: i64) -> rusqlite::Result<ConversationFloor> {
    let holder_device_id: Option<String> = row.get(3)?;
    let expires_at_unix: i64 = row.get(11)?;
    Ok(ConversationFloor {
        owner_id: row.get(0)?,
        conversation_id: row.get(1)?,
        lease_id: row.get(2)?,
        holder_device_id,
        holder_display_name: row.get(4)?,
        phase: row.get(5)?,
        partial_transcript: row.get(6)?,
        response_text: row.get(7)?,
        revision: row.get(8)?,
        acquired_at: row.get(9)?,
        updated_at: row.get(10)?,
        expires_at_unix,
        active: expires_at_unix > now_unix,
    })
}
