use std::collections::HashSet;
use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::ConversationStore;
use crate::sleep_memory::{
    CognitiveMemoryRecord, MorningReport, RawMemoryEvent, SleepCycle, SleepError,
};

pub(crate) struct SleepMemoryRepository {
    store: Arc<ConversationStore>,
}

impl SleepMemoryRepository {
    pub(crate) fn new(store: Arc<ConversationStore>) -> Self {
        Self { store }
    }

    pub(crate) fn raw_events(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<RawMemoryEvent>, SleepError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT event_id,owner_id,source_kind,source_ref,occurred_at,payload_json,content_sha256 FROM raw_memory_events WHERE owner_id=?1 ORDER BY occurred_at DESC,event_id DESC LIMIT ?2")?;
        let rows = statement.query_map(params![owner_id, limit.max(1)], |row| {
            let payload: String = row.get(5)?;
            Ok(RawMemoryEvent {
                id: row.get(0)?,
                owner_id: row.get(1)?,
                source_kind: row.get(2)?,
                source_ref: row.get(3)?,
                occurred_at: row.get(4)?,
                payload: serde_json::from_str(&payload).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        payload.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                content_sha256: row.get(6)?,
            })
        })?;
        let events = rows.collect::<Result<Vec<_>, _>>()?;
        for event in &events {
            let serialized = serde_json::to_string(&event.payload)?;
            if hex_digest(serialized.as_bytes()) != event.content_sha256 {
                return Err(SleepError::InvalidProposal(format!(
                    "raw event integrity check failed: {}",
                    event.id
                )));
            }
        }
        Ok(events)
    }

    pub(crate) fn search(
        &self,
        owner_id: &str,
        query: &str,
        include_dreams: bool,
        limit: usize,
    ) -> Result<Vec<CognitiveMemoryRecord>, SleepError> {
        let terms = search_terms(query);
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT memory_id,cycle_id,memory_kind,cognitive_status,content,confidence,active,quarantined,protected,provider,created_at FROM cognitive_memories WHERE owner_id=?1 AND (active=1 OR (?2=1 AND quarantined=1)) ORDER BY created_at DESC")?;
        let rows = statement
            .query_map(params![owner_id, include_dreams as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, i64>(6)? != 0,
                    row.get::<_, i64>(7)? != 0,
                    row.get::<_, i64>(8)? != 0,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut output = Vec::new();
        for (
            id,
            cycle_id,
            kind,
            status,
            content,
            confidence,
            active,
            quarantined,
            protected,
            provider,
            created_at,
        ) in rows
        {
            let overlap = if terms.is_empty() {
                1
            } else {
                let candidate = search_terms(&content);
                terms
                    .iter()
                    .filter(|term| candidate.contains(*term))
                    .count()
            };
            if overlap == 0 {
                continue;
            }
            output.push((
                overlap,
                CognitiveMemoryRecord {
                    source_event_ids: provenance_ids(&connection, &id)?,
                    id,
                    cycle_id,
                    memory_kind: kind,
                    cognitive_status: status,
                    content,
                    confidence,
                    active,
                    quarantined,
                    protected,
                    provider,
                    created_at,
                },
            ));
        }
        output.sort_by_key(|(score, memory)| {
            (
                std::cmp::Reverse(*score),
                std::cmp::Reverse(memory.created_at.clone()),
            )
        });
        Ok(output
            .into_iter()
            .take(limit.max(1))
            .map(|(_, memory)| memory)
            .collect())
    }

    pub(crate) fn cycle(&self, cycle_id: &str) -> Result<Option<SleepCycle>, SleepError> {
        let connection = self.store.connection()?;
        connection.query_row("SELECT cycle_id,owner_id,status,phase,mode,trigger_kind,config_json,metrics_json,error,started_at,updated_at,completed_at,rolled_back_at,rollback_reason FROM sleep_cycles WHERE cycle_id=?1", [cycle_id], cycle_row).optional().map_err(SleepError::from)
    }

    pub(crate) fn cycle_events(&self, cycle_id: &str) -> Result<Vec<Value>, SleepError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT sequence,phase,status,metrics_json,occurred_at FROM sleep_cycle_events WHERE cycle_id=?1 ORDER BY sequence")?;
        let rows = statement.query_map([cycle_id], |row| {
            let metrics: String = row.get(3)?;
            Ok(json!({
                "sequence": row.get::<_, i64>(0)?, "phase": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "metrics": serde_json::from_str::<Value>(&metrics).unwrap_or_else(|_| json!({})),
                "occurred_at": row.get::<_, String>(4)?,
            }))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn latest_cycle(&self, owner_id: &str) -> Result<Option<SleepCycle>, SleepError> {
        let connection = self.store.connection()?;
        connection.query_row("SELECT cycle_id,owner_id,status,phase,mode,trigger_kind,config_json,metrics_json,error,started_at,updated_at,completed_at,rolled_back_at,rollback_reason FROM sleep_cycles WHERE owner_id=?1 ORDER BY started_at DESC LIMIT 1", [owner_id], cycle_row).optional().map_err(SleepError::from)
    }

    pub(crate) fn morning_report(
        &self,
        owner_id: &str,
        cycle_id: Option<&str>,
    ) -> Result<Option<MorningReport>, SleepError> {
        let connection = self.store.connection()?;
        let payload: Option<String> = if let Some(cycle_id) = cycle_id {
            connection
                .query_row(
                    "SELECT report_json FROM morning_reports WHERE owner_id=?1 AND cycle_id=?2",
                    params![owner_id, cycle_id],
                    |row| row.get(0),
                )
                .optional()?
        } else {
            connection.query_row("SELECT report_json FROM morning_reports WHERE owner_id=?1 ORDER BY created_at DESC LIMIT 1", [owner_id], |row| row.get(0)).optional()?
        };
        payload
            .map(|value| serde_json::from_str(&value).map_err(SleepError::from))
            .transpose()
    }
}

fn cycle_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SleepCycle> {
    let config_json: String = row.get(6)?;
    let metrics_json: String = row.get(7)?;
    Ok(SleepCycle {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        status: row.get(2)?,
        phase: row.get(3)?,
        mode: row.get(4)?,
        trigger_kind: row.get(5)?,
        config: serde_json::from_str(&config_json).unwrap_or_default(),
        metrics: serde_json::from_str(&metrics_json).unwrap_or_else(|_| json!({})),
        error: row.get(8)?,
        started_at: row.get(9)?,
        updated_at: row.get(10)?,
        completed_at: row.get(11)?,
        rolled_back_at: row.get(12)?,
        rollback_reason: row.get(13)?,
    })
}

fn provenance_ids(
    connection: &rusqlite::Connection,
    memory_id: &str,
) -> Result<Vec<String>, SleepError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT event_id FROM memory_provenance WHERE memory_id=?1 ORDER BY event_id",
    )?;
    Ok(statement
        .query_map([memory_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn search_terms(value: &str) -> HashSet<String> {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|value| value.len() > 2)
        .map(str::to_owned)
        .collect()
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
