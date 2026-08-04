use chrono::{Duration, Utc};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{ConversationStore, OutreachPolicy, OutreachRecord, StoreError};

impl ConversationStore {
    #[allow(clippy::too_many_arguments)]
    pub fn create_outreach(
        &self,
        owner_id: &str,
        kind: &str,
        priority: &str,
        title: &str,
        body: &str,
        reason: &str,
        task_id: Option<&str>,
        conversation_id: Option<&str>,
        dedupe_key: Option<&str>,
        actions: &[String],
        scheduled_for: Option<&str>,
    ) -> Result<OutreachRecord, StoreError> {
        validate_choice(
            kind,
            &[
                "status_update",
                "check_in",
                "question",
                "blocker",
                "review",
                "digest",
            ],
            "kind",
        )?;
        validate_choice(priority, &["quiet", "check_in", "needs_you"], "priority")?;
        for (label, value) in [("title", title), ("body", body), ("reason", reason)] {
            if value.trim().is_empty() || value.len() > 2_000 {
                return Err(StoreError::InvalidInput(format!(
                    "invalid outreach {label}"
                )));
            }
        }
        let allowed_actions = [
            "talk_now",
            "show_progress",
            "hear_update",
            "later",
            "dismiss",
        ];
        if actions.is_empty()
            || actions
                .iter()
                .any(|action| !allowed_actions.contains(&action.as_str()))
        {
            return Err(StoreError::InvalidInput(
                "invalid outreach actions".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let scheduled = scheduled_for.unwrap_or(&now).to_owned();
        let owner = owner_id.trim();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO owners(owner_id, display_name, created_at, updated_at) VALUES(?1, NULL, ?2, ?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner, now],
        )?;
        if let Some(key) = dedupe_key.filter(|key| !key.trim().is_empty()) {
            if let Some(existing) = connection.query_row(
                "SELECT outreach_id, owner_id, kind, priority, title, body, reason, status, task_id, conversation_id, dedupe_key, actions_json, scheduled_for, created_at, delivered_at, responded_at, snoozed_until FROM outreach_events WHERE owner_id=?1 AND dedupe_key=?2 AND status IN ('queued','delivered','snoozed') ORDER BY created_at DESC LIMIT 1",
                params![owner, key.trim()], outreach_row,
            ).optional()? {
                return Ok(existing);
            }
        }
        let id = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO outreach_events(outreach_id, owner_id, kind, priority, title, body, reason, status, task_id, conversation_id, dedupe_key, actions_json, scheduled_for, created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'queued',?8,?9,?10,?11,?12,?13)",
            params![id, owner, kind, priority, title.trim(), body.trim(), reason.trim(), task_id, conversation_id, dedupe_key.map(str::trim), serde_json::to_string(actions)?, scheduled, now],
        )?;
        connection.query_row(
            "SELECT outreach_id, owner_id, kind, priority, title, body, reason, status, task_id, conversation_id, dedupe_key, actions_json, scheduled_for, created_at, delivered_at, responded_at, snoozed_until FROM outreach_events WHERE outreach_id=?1",
            params![id], outreach_row,
        ).map_err(StoreError::from)
    }

    pub fn outreaches(
        &self,
        owner_id: &str,
        include_closed: bool,
        limit: usize,
    ) -> Result<Vec<OutreachRecord>, StoreError> {
        let connection = self.connection()?;
        let sql = if include_closed {
            "SELECT outreach_id, owner_id, kind, priority, title, body, reason, status, task_id, conversation_id, dedupe_key, actions_json, scheduled_for, created_at, delivered_at, responded_at, snoozed_until FROM outreach_events WHERE owner_id=?1 ORDER BY created_at DESC LIMIT ?2"
        } else {
            "SELECT outreach_id, owner_id, kind, priority, title, body, reason, status, task_id, conversation_id, dedupe_key, actions_json, scheduled_for, created_at, delivered_at, responded_at, snoozed_until FROM outreach_events WHERE owner_id=?1 AND status IN ('queued','delivered','snoozed') ORDER BY created_at DESC LIMIT ?2"
        };
        let mut statement = connection.prepare(sql)?;
        statement
            .query_map(params![owner_id.trim(), limit.clamp(1, 200)], outreach_row)?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    pub fn act_on_outreach(
        &self,
        owner_id: &str,
        outreach_id: &str,
        action: &str,
        snooze_minutes: Option<u32>,
    ) -> Result<Option<OutreachRecord>, StoreError> {
        validate_choice(
            action,
            &[
                "delivered",
                "talk_now",
                "show_progress",
                "hear_update",
                "later",
                "dismiss",
            ],
            "action",
        )?;
        let now = Utc::now();
        let (status, delivered_at, responded_at, snoozed_until) = match action {
            "delivered" => ("delivered", Some(now.to_rfc3339()), None, None),
            "later" => {
                let minutes = snooze_minutes.unwrap_or(30).clamp(5, 1_440);
                (
                    "snoozed",
                    None,
                    Some(now.to_rfc3339()),
                    Some((now + Duration::minutes(i64::from(minutes))).to_rfc3339()),
                )
            }
            "dismiss" => ("dismissed", None, Some(now.to_rfc3339()), None),
            _ => ("responded", None, Some(now.to_rfc3339()), None),
        };
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE outreach_events SET status=?3, delivered_at=COALESCE(?4,delivered_at), responded_at=COALESCE(?5,responded_at), snoozed_until=?6 WHERE owner_id=?1 AND outreach_id=?2",
            params![owner_id.trim(), outreach_id.trim(), status, delivered_at, responded_at, snoozed_until],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        connection.query_row(
            "SELECT outreach_id, owner_id, kind, priority, title, body, reason, status, task_id, conversation_id, dedupe_key, actions_json, scheduled_for, created_at, delivered_at, responded_at, snoozed_until FROM outreach_events WHERE outreach_id=?1",
            params![outreach_id.trim()], outreach_row,
        ).optional().map_err(StoreError::from)
    }

    pub fn outreach_policy(&self, owner_id: &str) -> Result<OutreachPolicy, StoreError> {
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO owners(owner_id, display_name, created_at, updated_at) VALUES(?1,NULL,?2,?2) ON CONFLICT(owner_id) DO NOTHING",
            params![owner_id.trim(), now],
        )?;
        connection.execute(
            "INSERT INTO outreach_policies(owner_id, updated_at) VALUES(?1,?2) ON CONFLICT(owner_id) DO NOTHING",
            params![owner_id.trim(), now],
        )?;
        connection.query_row(
            "SELECT owner_id, enabled, quiet_hours_start, quiet_hours_end, timezone, max_checkins_per_day, cooldown_minutes, driving_mode, spoken_headphones_only, daily_digest_enabled, updated_at FROM outreach_policies WHERE owner_id=?1",
            params![owner_id.trim()], policy_row,
        ).map_err(StoreError::from)
    }
}

fn outreach_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutreachRecord> {
    let actions: String = row.get(11)?;
    Ok(OutreachRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        kind: row.get(2)?,
        priority: row.get(3)?,
        title: row.get(4)?,
        body: row.get(5)?,
        reason: row.get(6)?,
        status: row.get(7)?,
        task_id: row.get(8)?,
        conversation_id: row.get(9)?,
        dedupe_key: row.get(10)?,
        actions: serde_json::from_str(&actions).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                actions.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        scheduled_for: row.get(12)?,
        created_at: row.get(13)?,
        delivered_at: row.get(14)?,
        responded_at: row.get(15)?,
        snoozed_until: row.get(16)?,
    })
}

fn policy_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutreachPolicy> {
    Ok(OutreachPolicy {
        owner_id: row.get(0)?,
        enabled: row.get(1)?,
        quiet_hours_start: row.get(2)?,
        quiet_hours_end: row.get(3)?,
        timezone: row.get(4)?,
        max_checkins_per_day: row.get(5)?,
        cooldown_minutes: row.get(6)?,
        driving_mode: row.get(7)?,
        spoken_headphones_only: row.get(8)?,
        daily_digest_enabled: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn validate_choice(value: &str, allowed: &[&str], label: &str) -> Result<(), StoreError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput(format!(
            "invalid outreach {label}"
        )))
    }
}
