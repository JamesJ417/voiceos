use chrono::{DateTime, NaiveTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

use crate::{AttentionItem, CalendarEvent, ConversationStore, StoreError, TaskSchedule};

impl ConversationStore {
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_attention_item(
        &self,
        owner_id: &str,
        category: &str,
        source_id: &str,
        title: &str,
        summary: &str,
        urgency: &str,
        task_id: Option<&str>,
        occurred_at: &str,
        due_at: Option<&str>,
        approval_required: bool,
        available_actions: Vec<String>,
        evidence: Value,
    ) -> Result<AttentionItem, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("source_id", source_id)?;
        require_text("title", title)?;
        require_text("summary", summary)?;
        if ![
            "email",
            "calendar",
            "question",
            "approval",
            "document",
            "system",
            "message",
            "agent_work",
        ]
        .contains(&category)
        {
            return invalid("unsupported attention category");
        }
        if !["routine", "important", "urgent"].contains(&urgency) {
            return invalid("unsupported attention urgency");
        }
        parse_time("occurred_at", occurred_at)?;
        if let Some(value) = due_at {
            parse_time("due_at", value)?;
        }
        if !evidence.is_object() || available_actions.is_empty() {
            return invalid("attention evidence and available actions are required");
        }
        let allowed = [
            "summarize",
            "create_task",
            "prepare_reply",
            "request_send_approval",
            "request_invitation_approval",
            "review",
            "resolve",
            "dismiss",
            "snooze",
        ];
        if available_actions
            .iter()
            .any(|action| !allowed.contains(&action.as_str()))
        {
            return invalid("unsupported attention action");
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id.trim(), now],
        )?;
        connection.execute(
            "INSERT INTO attention_items(attention_id,owner_id,category,source_id,title,summary,urgency,status,task_id,occurred_at,due_at,approval_required,available_actions_json,evidence_json,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,'open',?8,?9,?10,?11,?12,?13,?14,?14)
             ON CONFLICT(owner_id,category,source_id) DO UPDATE SET title=excluded.title,summary=excluded.summary,urgency=excluded.urgency,task_id=COALESCE(excluded.task_id,attention_items.task_id),occurred_at=excluded.occurred_at,due_at=excluded.due_at,approval_required=excluded.approval_required,available_actions_json=excluded.available_actions_json,evidence_json=excluded.evidence_json,updated_at=excluded.updated_at",
            params![id, owner_id.trim(), category, source_id.trim(), title.trim(), summary.trim(), urgency, task_id, occurred_at, due_at, approval_required, serde_json::to_string(&available_actions)?, evidence.to_string(), now],
        )?;
        drop(connection);
        self.attention_item_by_source(owner_id, category, source_id)?
            .ok_or_else(|| StoreError::InvalidInput("attention item was not persisted".to_owned()))
    }

    pub fn attention_items(
        &self,
        owner_id: &str,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AttentionItem>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT attention_id,owner_id,category,source_id,title,summary,urgency,status,task_id,occurred_at,due_at,approval_required,available_actions_json,evidence_json,created_at,updated_at FROM attention_items WHERE owner_id=?1 AND (?2 IS NULL OR status=?2) ORDER BY CASE urgency WHEN 'urgent' THEN 0 WHEN 'important' THEN 1 ELSE 2 END, COALESCE(due_at,occurred_at), occurred_at DESC LIMIT ?3",
        )?;
        statement
            .query_map(
                params![owner_id.trim(), status, limit.clamp(1, 500)],
                attention_row,
            )?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    pub fn attention_item(
        &self,
        owner_id: &str,
        attention_id: &str,
    ) -> Result<Option<AttentionItem>, StoreError> {
        self.connection()?.query_row(
            "SELECT attention_id,owner_id,category,source_id,title,summary,urgency,status,task_id,occurred_at,due_at,approval_required,available_actions_json,evidence_json,created_at,updated_at FROM attention_items WHERE owner_id=?1 AND attention_id=?2",
            params![owner_id.trim(), attention_id.trim()], attention_row,
        ).optional().map_err(StoreError::from)
    }

    pub fn set_attention_status(
        &self,
        owner_id: &str,
        attention_id: &str,
        status: &str,
    ) -> Result<Option<AttentionItem>, StoreError> {
        if !["open", "snoozed", "resolved", "dismissed"].contains(&status) {
            return invalid("unsupported attention status");
        }
        let changed = self.connection()?.execute(
            "UPDATE attention_items SET status=?3,updated_at=?4 WHERE owner_id=?1 AND attention_id=?2",
            params![owner_id.trim(), attention_id.trim(), status, Utc::now().to_rfc3339()],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.attention_item(owner_id, attention_id)
    }

    fn attention_item_by_source(
        &self,
        owner_id: &str,
        category: &str,
        source_id: &str,
    ) -> Result<Option<AttentionItem>, StoreError> {
        self.connection()?.query_row(
            "SELECT attention_id,owner_id,category,source_id,title,summary,urgency,status,task_id,occurred_at,due_at,approval_required,available_actions_json,evidence_json,created_at,updated_at FROM attention_items WHERE owner_id=?1 AND category=?2 AND source_id=?3",
            params![owner_id.trim(), category, source_id.trim()], attention_row,
        ).optional().map_err(StoreError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_task_schedule(
        &self,
        owner_id: &str,
        task_id: &str,
        earliest_start_at: Option<&str>,
        recurrence_rule: Option<&str>,
        location: Option<&str>,
        preparation_minutes: u32,
        travel_minutes: u32,
        preferred_time: Option<&str>,
    ) -> Result<TaskSchedule, StoreError> {
        if let Some(value) = earliest_start_at {
            parse_time("earliest_start_at", value)?;
        }
        if preparation_minutes > 1_440 || travel_minutes > 1_440 {
            return invalid("preparation and travel must not exceed 1440 minutes");
        }
        if let Some(value) = preferred_time
            && NaiveTime::parse_from_str(value, "%H:%M").is_err()
        {
            return invalid("preferred_time must use HH:MM");
        }
        if let Some(value) = recurrence_rule
            && !value.starts_with("FREQ=")
        {
            return invalid("recurrence_rule must be an RFC5545-style FREQ rule");
        }
        if self.task(owner_id, task_id)?.is_none() {
            return invalid("task was not found");
        }
        let now = Utc::now().to_rfc3339();
        self.connection()?.execute(
            "INSERT INTO task_schedules(task_id,owner_id,earliest_start_at,recurrence_rule,location,preparation_minutes,travel_minutes,preferred_time,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(task_id) DO UPDATE SET earliest_start_at=excluded.earliest_start_at,recurrence_rule=excluded.recurrence_rule,location=excluded.location,preparation_minutes=excluded.preparation_minutes,travel_minutes=excluded.travel_minutes,preferred_time=excluded.preferred_time,updated_at=excluded.updated_at",
            params![task_id.trim(), owner_id.trim(), earliest_start_at, recurrence_rule, location, preparation_minutes, travel_minutes, preferred_time, now],
        )?;
        self.task_schedule(owner_id, task_id)?
            .ok_or_else(|| StoreError::InvalidInput("task schedule was not persisted".to_owned()))
    }

    pub fn task_schedule(
        &self,
        owner_id: &str,
        task_id: &str,
    ) -> Result<Option<TaskSchedule>, StoreError> {
        self.connection()?.query_row(
            "SELECT task_id,owner_id,earliest_start_at,recurrence_rule,location,preparation_minutes,travel_minutes,preferred_time,updated_at FROM task_schedules WHERE owner_id=?1 AND task_id=?2",
            params![owner_id.trim(), task_id.trim()], schedule_row,
        ).optional().map_err(StoreError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_calendar_event(
        &self,
        owner_id: &str,
        source_id: &str,
        title: &str,
        start_at: &str,
        end_at: &str,
        location: Option<&str>,
        status: &str,
        response_status: &str,
        task_id: Option<&str>,
        preparation_minutes: u32,
        travel_minutes: u32,
        metadata: Value,
    ) -> Result<CalendarEvent, StoreError> {
        require_text("calendar source_id", source_id)?;
        require_text("calendar title", title)?;
        let start = parse_time("start_at", start_at)?;
        let end = parse_time("end_at", end_at)?;
        if end <= start {
            return invalid("calendar end_at must be after start_at");
        }
        if !["confirmed", "tentative", "cancelled"].contains(&status)
            || !["none", "needs_action", "accepted", "declined", "tentative"]
                .contains(&response_status)
        {
            return invalid("unsupported calendar status");
        }
        if !metadata.is_object() {
            return invalid("calendar metadata must be an object");
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute("INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at", params![owner_id.trim(), now])?;
        connection.execute(
            "INSERT INTO calendar_events(calendar_event_id,owner_id,source_id,title,start_at,end_at,location,status,response_status,task_id,preparation_minutes,travel_minutes,metadata_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14) ON CONFLICT(owner_id,source_id) DO UPDATE SET title=excluded.title,start_at=excluded.start_at,end_at=excluded.end_at,location=excluded.location,status=excluded.status,response_status=excluded.response_status,task_id=excluded.task_id,preparation_minutes=excluded.preparation_minutes,travel_minutes=excluded.travel_minutes,metadata_json=excluded.metadata_json,updated_at=excluded.updated_at",
            params![id, owner_id.trim(), source_id.trim(), title.trim(), start_at, end_at, location, status, response_status, task_id, preparation_minutes, travel_minutes, metadata.to_string(), now],
        )?;
        drop(connection);
        self.calendar_event_by_source(owner_id, source_id)?
            .ok_or_else(|| StoreError::InvalidInput("calendar event was not persisted".to_owned()))
    }

    pub fn calendar_events(
        &self,
        owner_id: &str,
        start_at: &str,
        end_at: &str,
    ) -> Result<Vec<CalendarEvent>, StoreError> {
        parse_time("start_at", start_at)?;
        parse_time("end_at", end_at)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT calendar_event_id,owner_id,source_id,title,start_at,end_at,location,status,response_status,task_id,preparation_minutes,travel_minutes,metadata_json,created_at,updated_at FROM calendar_events WHERE owner_id=?1 AND status<>'cancelled' AND end_at>?2 AND start_at<?3 ORDER BY start_at")?;
        statement
            .query_map(params![owner_id.trim(), start_at, end_at], calendar_row)?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    fn calendar_event_by_source(
        &self,
        owner_id: &str,
        source_id: &str,
    ) -> Result<Option<CalendarEvent>, StoreError> {
        self.connection()?.query_row("SELECT calendar_event_id,owner_id,source_id,title,start_at,end_at,location,status,response_status,task_id,preparation_minutes,travel_minutes,metadata_json,created_at,updated_at FROM calendar_events WHERE owner_id=?1 AND source_id=?2", params![owner_id.trim(), source_id.trim()], calendar_row).optional().map_err(StoreError::from)
    }
}

fn attention_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttentionItem> {
    Ok(AttentionItem {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        category: row.get(2)?,
        source_id: row.get(3)?,
        title: row.get(4)?,
        summary: row.get(5)?,
        urgency: row.get(6)?,
        status: row.get(7)?,
        task_id: row.get(8)?,
        occurred_at: row.get(9)?,
        due_at: row.get(10)?,
        approval_required: row.get(11)?,
        available_actions: parse_json(row.get(12)?)?,
        evidence: parse_json(row.get(13)?)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}
fn schedule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskSchedule> {
    Ok(TaskSchedule {
        task_id: row.get(0)?,
        owner_id: row.get(1)?,
        earliest_start_at: row.get(2)?,
        recurrence_rule: row.get(3)?,
        location: row.get(4)?,
        preparation_minutes: row.get(5)?,
        travel_minutes: row.get(6)?,
        preferred_time: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
fn calendar_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CalendarEvent> {
    Ok(CalendarEvent {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        source_id: row.get(2)?,
        title: row.get(3)?,
        start_at: row.get(4)?,
        end_at: row.get(5)?,
        location: row.get(6)?,
        status: row.get(7)?,
        response_status: row.get(8)?,
        task_id: row.get(9)?,
        preparation_minutes: row.get(10)?,
        travel_minutes: row.get(11)?,
        metadata: parse_json(row.get(12)?)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}
fn parse_json<T: serde::de::DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            value.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
fn parse_time(name: &str, value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidInput(format!("{name} must be an RFC3339 timestamp")))
}
fn require_text(name: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        invalid(&format!("{name} is required"))
    } else {
        Ok(())
    }
}
fn invalid<T>(message: &str) -> Result<T, StoreError> {
    Err(StoreError::InvalidInput(message.to_owned()))
}
