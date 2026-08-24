use chrono::Utc;
use rusqlite::{OptionalExtension, Row, params};
use serde_json::Value;
use uuid::Uuid;

use crate::{ConversationStore, SleepCycleChange, SleepCycleRecord, SleepCycleReport, StoreError};

impl ConversationStore {
    /// Captures a stable, inspectable nightly report without mutating memories.
    /// Reusing an owner/idempotency-key pair returns the original report.
    pub fn create_dry_run_sleep_cycle(
        &self,
        owner_id: &str,
        idempotency_key: &str,
    ) -> Result<SleepCycleRecord, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("idempotency key", idempotency_key)?;
        let owner_id = owner_id.trim();
        let idempotency_key = idempotency_key.trim();
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1, ?2, ?2) ON CONFLICT(owner_id) DO NOTHING",
            params![owner_id, now],
        )?;

        if let Some(existing) = transaction
            .query_row(
                &sleep_cycle_select("owner_id=?1 AND idempotency_key=?2"),
                params![owner_id, idempotency_key],
                map_cycle,
            )
            .optional()?
        {
            transaction.commit()?;
            return Ok(existing);
        }

        let previous = transaction
            .query_row(
                &sleep_cycle_select(
                    "owner_id=?1 AND status='completed' ORDER BY created_at DESC LIMIT 1",
                ),
                [owner_id],
                map_cycle,
            )
            .optional()?;
        let previous_event_watermark = previous
            .as_ref()
            .map(|cycle| cycle.event_watermark)
            .unwrap_or(0);
        let previous_message_watermark = previous
            .as_ref()
            .map(|cycle| cycle.message_watermark)
            .unwrap_or(0);
        let event_watermark: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(event_id), 0) FROM execution_events WHERE owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let message_watermark: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(m.message_id), 0) FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let events_inspected: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM execution_events WHERE owner_id=?1 AND event_id>?2 AND event_id<=?3",
            params![owner_id, previous_event_watermark, event_watermark],
            |row| row.get(0),
        )?;
        let messages_inspected: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1 AND m.message_id>?2 AND m.message_id<=?3",
            params![owner_id, previous_message_watermark, message_watermark],
            |row| row.get(0),
        )?;
        let memories: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let id = Uuid::new_v4().to_string();
        let summary = format!(
            "Inspected {messages_inspected} new messages and {events_inspected} new events. Proposed 0 memory changes and committed 0; durable memories remain at {memories}."
        );
        transaction.execute(
            "INSERT INTO sleep_cycles(
                sleep_cycle_id, owner_id, idempotency_key, mode, status,
                previous_cycle_id, event_watermark, message_watermark,
                events_inspected, messages_inspected, memories_before, memories_after,
                proposed_changes, committed_changes, summary, created_at, completed_at
             ) VALUES(?1,?2,?3,'dry_run','completed',?4,?5,?6,?7,?8,?9,?9,0,0,?10,?11,?11)",
            params![
                id,
                owner_id,
                idempotency_key,
                previous.as_ref().map(|cycle| cycle.id.as_str()),
                event_watermark,
                message_watermark,
                events_inspected,
                messages_inspected,
                memories,
                summary,
                now,
            ],
        )?;
        let record =
            transaction.query_row(&sleep_cycle_select("sleep_cycle_id=?1"), [&id], map_cycle)?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn sleep_cycle_report(
        &self,
        owner_id: &str,
        sleep_cycle_id: &str,
    ) -> Result<Option<SleepCycleReport>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("sleep_cycle_id", sleep_cycle_id)?;
        let connection = self.connection()?;
        let cycle = connection
            .query_row(
                &sleep_cycle_select("owner_id=?1 AND sleep_cycle_id=?2"),
                params![owner_id.trim(), sleep_cycle_id.trim()],
                map_cycle,
            )
            .optional()?;
        cycle
            .map(|cycle| build_report(&connection, cycle))
            .transpose()
    }

    pub fn sleep_cycle_reports(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<SleepCycleReport>, StoreError> {
        require_text("owner_id", owner_id)?;
        let connection = self.connection()?;
        let sql = format!(
            "{} ORDER BY created_at DESC LIMIT ?2",
            sleep_cycle_select("owner_id=?1")
        );
        let mut statement = connection.prepare(&sql)?;
        let cycles = statement
            .query_map(params![owner_id.trim(), limit.clamp(1, 100)], map_cycle)?
            .collect::<Result<Vec<_>, _>>()?;
        cycles
            .into_iter()
            .map(|cycle| build_report(&connection, cycle))
            .collect()
    }
}

fn build_report(
    connection: &rusqlite::Connection,
    cycle: SleepCycleRecord,
) -> Result<SleepCycleReport, StoreError> {
    let mut statement = connection.prepare(
        "SELECT change_id, sleep_cycle_id, operation, memory_kind, title, detail, status, confidence, evidence_json, created_at
         FROM sleep_cycle_changes WHERE sleep_cycle_id=?1 ORDER BY created_at, change_id",
    )?;
    let changes = statement
        .query_map([&cycle.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?
        .map(|row| {
            let row = row?;
            Ok(SleepCycleChange {
                id: row.0,
                sleep_cycle_id: row.1,
                operation: row.2,
                memory_kind: row.3,
                title: row.4,
                detail: row.5,
                status: row.6,
                confidence: row.7,
                evidence: serde_json::from_str::<Value>(&row.8)?,
                created_at: row.9,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let previous_proposed = cycle
        .previous_cycle_id
        .as_ref()
        .map(|id| {
            connection.query_row(
                "SELECT proposed_changes FROM sleep_cycles WHERE sleep_cycle_id=?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
        })
        .transpose()?
        .unwrap_or(0);
    Ok(SleepCycleReport {
        new_evidence_count: cycle.events_inspected + cycle.messages_inspected,
        durable_memory_delta: cycle.memories_after as i64 - cycle.memories_before as i64,
        proposed_change_delta: cycle.proposed_changes as i64 - previous_proposed,
        cycle,
        changes,
    })
}

fn sleep_cycle_select(filter: &str) -> String {
    format!(
        "SELECT sleep_cycle_id, owner_id, idempotency_key, mode, status,
                previous_cycle_id, event_watermark, message_watermark,
                events_inspected, messages_inspected, memories_before, memories_after,
                proposed_changes, committed_changes, summary, created_at, completed_at
         FROM sleep_cycles WHERE {filter}"
    )
}

fn map_cycle(row: &Row<'_>) -> rusqlite::Result<SleepCycleRecord> {
    let mode: String = row.get(3)?;
    Ok(SleepCycleRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        idempotency_key: row.get(2)?,
        dry_run: mode == "dry_run",
        mode,
        status: row.get(4)?,
        previous_cycle_id: row.get(5)?,
        event_watermark: row.get(6)?,
        message_watermark: row.get(7)?,
        events_inspected: nonnegative(row, 8)?,
        messages_inspected: nonnegative(row, 9)?,
        memories_before: nonnegative(row, 10)?,
        memories_after: nonnegative(row, 11)?,
        proposed_changes: nonnegative(row, 12)?,
        committed_changes: nonnegative(row, 13)?,
        summary: row.get(14)?,
        created_at: row.get(15)?,
        completed_at: row.get(16)?,
    })
}

fn nonnegative(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    Ok(row.get::<_, i64>(index)?.max(0) as u64)
}

fn require_text(label: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        Err(StoreError::InvalidInput(format!("{label} is required")))
    } else {
        Ok(())
    }
}
