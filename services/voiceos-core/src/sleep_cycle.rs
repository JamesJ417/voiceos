use chrono::Utc;
use rusqlite::{OptionalExtension, Row, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    ConversationStore, LiveMemoryChange, SleepCycleChange, SleepCycleRecord, SleepCycleReport,
    StoreError,
};

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
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1 AND status='active'",
            [owner_id],
            |row| row.get(0),
        )?;
        let mut statement = transaction.prepare("SELECT m.content,m.message_id FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1 AND m.role='user' AND m.message_id>?2 AND m.message_id<=?3 ORDER BY m.message_id LIMIT 1000")?;
        let candidates = statement
            .query_map(
                params![owner_id, previous_message_watermark, message_watermark],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut proposals = Vec::new();
        for (content, message_id) in candidates {
            let Some(memory) = explicit_memory_candidate(&content) else {
                continue;
            };
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM memories WHERE owner_id=?1 AND normalized_content=?2 AND status='active')",
                params![owner_id, normalize_memory(&memory)], |row| row.get(0),
            )?;
            if !exists {
                proposals.push((memory, message_id));
            }
        }
        let id = Uuid::new_v4().to_string();
        let summary = format!(
            "Inspected {messages_inspected} new messages and {events_inspected} new events. Proposed {} memory changes and committed 0; durable memories remain at {memories}.",
            proposals.len()
        );
        transaction.execute(
            "INSERT INTO sleep_cycles(
                sleep_cycle_id, owner_id, idempotency_key, mode, status,
                previous_cycle_id, event_watermark, message_watermark,
                events_inspected, messages_inspected, memories_before, memories_after,
                proposed_changes, committed_changes, summary, created_at, completed_at
             ) VALUES(?1,?2,?3,'dry_run','completed',?4,?5,?6,?7,?8,?9,?9,?10,0,?11,?12,?12)",
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
                proposals.len() as i64,
                summary,
                now,
            ],
        )?;
        for (memory, message_id) in proposals {
            transaction.execute(
                "INSERT INTO sleep_cycle_changes(change_id,sleep_cycle_id,operation,memory_kind,title,detail,status,confidence,evidence_json,created_at) VALUES(?1,?2,'add','general',?3,'explicit user memory request','proposed',1.0,?4,?5)",
                params![Uuid::new_v4().to_string(), id, memory, serde_json::json!([{"role":"user","message_id":message_id}]).to_string(), now],
            )?;
        }
        let record =
            transaction.query_row(&sleep_cycle_select("sleep_cycle_id=?1"), [&id], map_cycle)?;
        transaction.commit()?;
        Ok(record)
    }

    /// Commits only caller-supplied additions for a controlled live run.
    /// This is deliberately not an evidence extractor or automatic deletion mechanism.
    pub fn commit_live_sleep_cycle(
        &self,
        owner_id: &str,
        idempotency_key: &str,
        device_id: &str,
        changes: &[LiveMemoryChange],
    ) -> Result<SleepCycleRecord, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("idempotency key", idempotency_key)?;
        require_text("device_id", device_id)?;
        if changes.is_empty() {
            return Err(StoreError::InvalidInput(
                "at least one live memory change is required".to_owned(),
            ));
        }
        let owner_id = owner_id.trim();
        let idempotency_key = idempotency_key.trim();
        let device_id = device_id.trim();
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let belongs_to_owner: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM owner_devices WHERE owner_id=?1 AND device_id=?2 AND revoked_at IS NULL)",
            params![owner_id, device_id],
            |row| row.get(0),
        )?;
        if !belongs_to_owner {
            return Err(StoreError::InvalidInput(
                "device does not belong to owner".to_owned(),
            ));
        }
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
        let memories_before: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO sleep_cycles(
                sleep_cycle_id, owner_id, idempotency_key, mode, status,
                event_watermark, message_watermark, memories_before, memories_after,
                proposed_changes, committed_changes, summary, created_at, completed_at
             ) VALUES(?1,?2,?3,'commit','running',0,0,?4,?4,?5,0,'',?6,NULL)",
            params![
                id,
                owner_id,
                idempotency_key,
                memories_before,
                changes.len() as i64,
                now
            ],
        )?;
        let mut committed = 0_i64;
        for change in changes {
            require_text("live memory content", &change.content)?;
            require_text("live memory source", &change.source)?;
            let normalized = normalize_memory(&change.content);
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM memories WHERE owner_id=?1 AND normalized_content=?2 AND status='active')",
                params![owner_id, normalized],
                |row| row.get(0),
            )?;
            let status = if exists { "rejected" } else { "committed" };
            if !exists {
                if change.evidence.to_string().len() > 16 * 1024 {
                    return Err(StoreError::InvalidInput(
                        "proposal evidence exceeds 16 KiB".to_owned(),
                    ));
                }
                ConversationStore::insert_structured_memory(
                    &transaction,
                    owner_id,
                    device_id,
                    &change.content,
                    "general",
                    &change.source,
                    1.0,
                    &format!("sleep-cycle://{id}"),
                )?;
                committed += 1;
            }
            transaction.execute(
                "INSERT INTO sleep_cycle_changes(change_id, sleep_cycle_id, operation, memory_kind, title, detail, status, confidence, evidence_json, created_at)
                 VALUES(?1,?2,'add','durable_memory',?3,?4,?5,1.0,?6,?7)",
                params![Uuid::new_v4().to_string(), id, change.content.trim(), change.source.trim(), status, change.evidence.to_string(), now],
            )?;
        }
        let memories_after: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let summary = format!(
            "Committed {committed} explicit memory additions from {} supplied candidates; durable memories changed from {memories_before} to {memories_after}.",
            changes.len()
        );
        transaction.execute(
            "UPDATE sleep_cycles SET status='completed', memories_after=?2, committed_changes=?3, summary=?4, completed_at=?5 WHERE sleep_cycle_id=?1",
            params![id, memories_after, committed, summary, now],
        )?;
        let record =
            transaction.query_row(&sleep_cycle_select("sleep_cycle_id=?1"), [&id], map_cycle)?;
        transaction.commit()?;
        Ok(record)
    }

    /// Commits only explicit `Remember ...` statements authored by the owner.
    pub fn create_commit_sleep_cycle(
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
        transaction.execute("INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO NOTHING", params![owner_id, now])?;
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
            "SELECT COALESCE(MAX(event_id),0) FROM execution_events WHERE owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let message_watermark: i64 = transaction.query_row("SELECT COALESCE(MAX(m.message_id),0) FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1", [owner_id], |row| row.get(0))?;
        let events_inspected: i64 = transaction.query_row("SELECT COUNT(*) FROM execution_events WHERE owner_id=?1 AND event_id>?2 AND event_id<=?3", params![owner_id, previous_event_watermark, event_watermark], |row| row.get(0))?;
        let messages_inspected: i64 = transaction.query_row("SELECT COUNT(*) FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1 AND m.message_id>?2 AND m.message_id<=?3", params![owner_id, previous_message_watermark, message_watermark], |row| row.get(0))?;
        let memories_before: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let device_id: Option<String> = transaction.query_row("SELECT device_id FROM owner_devices WHERE owner_id=?1 AND revoked_at IS NULL ORDER BY enrolled_at LIMIT 1", [owner_id], |row| row.get(0)).optional()?;
        let mut statement = transaction.prepare("SELECT m.content, m.message_id FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1 AND m.role='user' AND m.message_id>?2 AND m.message_id<=?3 ORDER BY m.message_id")?;
        let candidates = statement
            .query_map(
                params![owner_id, previous_message_watermark, message_watermark],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let id = Uuid::new_v4().to_string();
        transaction.execute("INSERT INTO sleep_cycles(sleep_cycle_id, owner_id, idempotency_key, mode, status, previous_cycle_id, event_watermark, message_watermark, events_inspected, messages_inspected, memories_before, memories_after, proposed_changes, committed_changes, summary, created_at) VALUES(?1,?2,?3,'commit','running',?4,?5,?6,?7,?8,?9,?9,0,0,'',?10)", params![id, owner_id, idempotency_key, previous.as_ref().map(|cycle| cycle.id.as_str()), event_watermark, message_watermark, events_inspected, messages_inspected, memories_before, now])?;
        let mut proposed = 0_i64;
        let mut committed = 0_i64;
        for (content, message_id) in candidates {
            let Some(memory) = explicit_memory_candidate(&content) else {
                continue;
            };
            proposed += 1;
            let normalized = normalize_memory(&memory);
            let exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM memories WHERE owner_id=?1 AND normalized_content=?2 AND status='active')", params![owner_id, normalized], |row| row.get(0))?;
            let status = if exists || device_id.is_none() {
                "rejected"
            } else {
                "committed"
            };
            if status == "committed" {
                ConversationStore::insert_structured_memory(
                    &transaction,
                    owner_id,
                    device_id.as_deref().unwrap_or(""),
                    &memory,
                    "general",
                    "sleep-cycle-explicit-user-request",
                    1.0,
                    &format!("message://{message_id}"),
                )?;
                committed += 1;
            }
            transaction.execute("INSERT INTO sleep_cycle_changes(change_id, sleep_cycle_id, operation, memory_kind, title, detail, status, confidence, evidence_json, created_at) VALUES(?1,?2,'add','durable_memory',?3,'explicit user memory request',?4,1.0,?5,?6)", params![Uuid::new_v4().to_string(), id, memory, status, serde_json::json!([{ "role": "user", "message_id": message_id }]).to_string(), now])?;
        }
        let memories_after: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1",
            [owner_id],
            |row| row.get(0),
        )?;
        let summary = format!(
            "Inspected {messages_inspected} new messages and {events_inspected} new events. Proposed {proposed} explicit memory changes and committed {committed}; durable memories changed from {memories_before} to {memories_after}."
        );
        transaction.execute("UPDATE sleep_cycles SET status='completed', memories_after=?2, proposed_changes=?3, committed_changes=?4, summary=?5, completed_at=?6 WHERE sleep_cycle_id=?1", params![id, memories_after, proposed, committed, summary, now])?;
        let record =
            transaction.query_row(&sleep_cycle_select("sleep_cycle_id=?1"), [&id], map_cycle)?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn commit_sleep_cycle_proposals(
        &self,
        owner_id: &str,
        source_cycle_id: &str,
        idempotency_key: &str,
        device_id: &str,
        change_ids: &[String],
    ) -> Result<SleepCycleRecord, StoreError> {
        for (label, value) in [
            ("owner_id", owner_id),
            ("source cycle", source_cycle_id),
            ("idempotency key", idempotency_key),
            ("device_id", device_id),
        ] {
            require_text(label, value)?;
        }
        if change_ids.is_empty() || change_ids.len() > 100 {
            return Err(StoreError::InvalidInput(
                "select between 1 and 100 proposals".to_owned(),
            ));
        }
        let mut selected = change_ids
            .iter()
            .map(|id| id.trim().to_owned())
            .collect::<Vec<_>>();
        if selected.iter().any(|id| id.is_empty()) {
            return Err(StoreError::InvalidInput(
                "proposal id is required".to_owned(),
            ));
        }
        selected.sort();
        selected.dedup();
        let digest = format!("{:x}", Sha256::digest(selected.join("\n").as_bytes()));
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let belongs: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM owner_devices WHERE owner_id=?1 AND device_id=?2 AND revoked_at IS NULL)", params![owner_id,device_id], |row| row.get(0))?;
        if !belongs {
            return Err(StoreError::InvalidInput(
                "device does not belong to owner".to_owned(),
            ));
        }
        if let Some(existing) = transaction
            .query_row(
                &sleep_cycle_select("owner_id=?1 AND idempotency_key=?2"),
                params![owner_id, idempotency_key],
                map_cycle,
            )
            .optional()?
        {
            let existing_digest: String = transaction.query_row(
                "SELECT input_digest FROM sleep_cycles WHERE sleep_cycle_id=?1",
                [&existing.id],
                |row| row.get(0),
            )?;
            if existing_digest != digest {
                return Err(StoreError::InvalidInput(
                    "idempotency key reused with different proposals".to_owned(),
                ));
            }
            transaction.commit()?;
            return Ok(existing);
        }
        let source = transaction
            .query_row(
                &sleep_cycle_select("owner_id=?1 AND sleep_cycle_id=?2 AND mode='dry_run'"),
                params![owner_id, source_cycle_id],
                map_cycle,
            )
            .optional()?
            .ok_or_else(|| StoreError::InvalidInput("proposal cycle not found".to_owned()))?;
        let before: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1 AND status='active'",
            [owner_id],
            |row| row.get(0),
        )?;
        let cycle_id = Uuid::new_v4().to_string();
        transaction.execute("INSERT INTO sleep_cycles(sleep_cycle_id,owner_id,idempotency_key,mode,status,previous_cycle_id,event_watermark,message_watermark,events_inspected,messages_inspected,memories_before,memories_after,proposed_changes,committed_changes,summary,created_at,input_digest) VALUES(?1,?2,?3,'commit','running',?4,?5,?6,0,0,?7,?7,?8,0,'',?9,?10)", params![cycle_id,owner_id,idempotency_key,source_cycle_id,source.event_watermark,source.message_watermark,before,selected.len() as i64,now,digest])?;
        let mut committed = 0_i64;
        for change_id in &selected {
            let proposal: Option<(String,String,f64,String)> = transaction.query_row(
                "SELECT title,memory_kind,COALESCE(confidence,0),evidence_json FROM sleep_cycle_changes WHERE sleep_cycle_id=?1 AND change_id=?2 AND status='proposed' AND operation='add'",
                params![source_cycle_id,change_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).optional()?;
            let (content, category, confidence, evidence) = proposal.ok_or_else(|| {
                StoreError::InvalidInput(format!("proposal not available: {change_id}"))
            })?;
            if evidence.len() > 16 * 1024 {
                return Err(StoreError::InvalidInput(
                    "proposal evidence exceeds 16 KiB".to_owned(),
                ));
            }
            let duplicate: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM memories WHERE owner_id=?1 AND normalized_content=?2 AND status='active')", params![owner_id,normalize_memory(&content)], |row| row.get(0))?;
            let status = if duplicate { "rejected" } else { "committed" };
            if !duplicate {
                ConversationStore::insert_structured_memory(
                    &transaction,
                    owner_id,
                    device_id,
                    &content,
                    &category,
                    "sleep-cycle-approved",
                    confidence,
                    &format!("sleep-cycle://{source_cycle_id}/{change_id}"),
                )?;
                committed += 1;
            }
            transaction.execute(
                "UPDATE sleep_cycle_changes SET status=?1 WHERE change_id=?2 AND sleep_cycle_id=?3",
                params![status, change_id, source_cycle_id],
            )?;
            transaction.execute("INSERT INTO sleep_cycle_changes(change_id,sleep_cycle_id,operation,memory_kind,title,detail,status,confidence,evidence_json,created_at) VALUES(?1,?2,'add',?3,?4,'approved sleep-cycle proposal',?5,?6,?7,?8)", params![Uuid::new_v4().to_string(),cycle_id,category,content,status,confidence,evidence,now])?;
        }
        let after: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id=?1 AND status='active'",
            [owner_id],
            |row| row.get(0),
        )?;
        let summary = format!(
            "Approved {} proposals and committed {committed}; active durable memories changed from {before} to {after}.",
            selected.len()
        );
        transaction.execute("UPDATE sleep_cycles SET status='completed',memories_after=?2,committed_changes=?3,summary=?4,completed_at=?5 WHERE sleep_cycle_id=?1", params![cycle_id,after,committed,summary,now])?;
        let record = transaction.query_row(
            &sleep_cycle_select("sleep_cycle_id=?1"),
            [&cycle_id],
            map_cycle,
        )?;
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

fn explicit_memory_candidate(content: &str) -> Option<String> {
    let trimmed = content.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let prefix = if lowered.starts_with("remember that ") {
        "remember that "
    } else if lowered.starts_with("remember ") {
        "remember "
    } else {
        return None;
    };
    let candidate = trimmed[prefix.len()..]
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim();
    (!candidate.is_empty()).then(|| candidate.to_owned())
}

fn normalize_memory(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
