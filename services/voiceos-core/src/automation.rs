use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{AutomationFrequencyLimit, AutomationRule, ConversationStore, StoreError};

impl ConversationStore {
    #[allow(clippy::too_many_arguments)]
    pub fn create_automation_rule(
        &self,
        owner_id: &str,
        name: &str,
        description: &str,
        trigger: Value,
        conditions: Value,
        permitted_actions: Vec<String>,
        frequency_limit: AutomationFrequencyLimit,
        evidence: Value,
        enabled: bool,
    ) -> Result<AutomationRule, StoreError> {
        validate_rule(
            name,
            description,
            &trigger,
            &conditions,
            &permitted_actions,
            &frequency_limit,
            &evidence,
        )?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id.trim(), now],
        )?;
        connection.execute(
            "INSERT INTO automation_rules(automation_id,owner_id,name,description,enabled,trigger_json,conditions_json,permitted_actions_json,frequency_max_runs,frequency_window_minutes,evidence_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
            params![id, owner_id.trim(), name.trim(), description.trim(), enabled, trigger.to_string(), conditions.to_string(), serde_json::to_string(&permitted_actions)?, frequency_limit.max_runs, frequency_limit.window_minutes, evidence.to_string(), now],
        )?;
        drop(connection);
        self.automation_rule(owner_id, &id)?
            .ok_or_else(|| StoreError::InvalidInput("automation rule was not persisted".to_owned()))
    }

    pub fn automation_rules(
        &self,
        owner_id: &str,
        include_disabled: bool,
        limit: usize,
    ) -> Result<Vec<AutomationRule>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT automation_id,owner_id,name,description,enabled,trigger_json,conditions_json,permitted_actions_json,frequency_max_runs,frequency_window_minutes,evidence_json,created_at,updated_at FROM automation_rules WHERE owner_id=?1 AND (?2 OR enabled=1) ORDER BY name LIMIT ?3",
        )?;
        statement
            .query_map(
                params![owner_id.trim(), include_disabled, limit.clamp(1, 500)],
                automation_row,
            )?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    pub fn automation_rule(
        &self,
        owner_id: &str,
        automation_id: &str,
    ) -> Result<Option<AutomationRule>, StoreError> {
        self.connection()?
            .query_row(
                "SELECT automation_id,owner_id,name,description,enabled,trigger_json,conditions_json,permitted_actions_json,frequency_max_runs,frequency_window_minutes,evidence_json,created_at,updated_at FROM automation_rules WHERE owner_id=?1 AND automation_id=?2",
                params![owner_id.trim(), automation_id.trim()],
                automation_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn set_automation_rule_enabled(
        &self,
        owner_id: &str,
        automation_id: &str,
        enabled: bool,
    ) -> Result<Option<AutomationRule>, StoreError> {
        let changed = self.connection()?.execute(
            "UPDATE automation_rules SET enabled=?3,updated_at=?4 WHERE owner_id=?1 AND automation_id=?2",
            params![owner_id.trim(), automation_id.trim(), enabled, Utc::now().to_rfc3339()],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.automation_rule(owner_id, automation_id)
    }

    pub fn ensure_default_attention_automations(
        &self,
        owner_id: &str,
    ) -> Result<Vec<AutomationRule>, StoreError> {
        let defaults = [
            (
                "scheduled-review",
                "Changed-only attention review loop",
                "review",
                vec!["review.scan"],
                96,
            ),
            (
                "task-attention",
                "Task, job, blocker, and deadline attention",
                "tasks",
                vec!["notify.needs_you", "digest.add"],
                100,
            ),
            (
                "system-attention",
                "Deterministic system-condition attention",
                "system",
                vec!["notify.needs_you", "digest.add"],
                24,
            ),
            (
                "approval-attention",
                "Pending approval attention",
                "approval",
                vec!["notify.needs_you"],
                24,
            ),
            (
                "email-attention",
                "New email signal attention",
                "email",
                vec!["notify.needs_you", "digest.add", "model.classify"],
                100,
            ),
            (
                "calendar-attention",
                "Deadline and calendar-event attention",
                "calendar",
                vec!["notify.needs_you", "digest.add", "model.classify"],
                100,
            ),
            (
                "communication-attention",
                "Missed call and message attention",
                "message",
                vec!["notify.needs_you", "digest.add", "model.classify"],
                100,
            ),
            (
                "document-attention",
                "New private or VIC-created file attention",
                "document",
                vec!["digest.add"],
                500,
            ),
            (
                "question-attention",
                "Unanswered VIC question attention",
                "question",
                vec!["digest.add"],
                100,
            ),
            (
                "daily-planning",
                "Twelve-question daily planning prompt",
                "planning",
                vec!["planning.prompt"],
                1,
            ),
            (
                "routine-digest",
                "Morning and evening routine digest",
                "digest",
                vec!["digest.deliver"],
                2,
            ),
        ];
        let mut rules = Vec::new();
        for (name, description, source, actions, max_runs) in defaults {
            let existing = {
                let connection = self.connection()?;
                connection
                    .query_row(
                        "SELECT automation_id FROM automation_rules WHERE owner_id=?1 AND name=?2",
                        params![owner_id.trim(), name],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
            };
            if let Some(existing) = existing {
                if let Some(rule) = self.automation_rule(owner_id, &existing)? {
                    rules.push(rule);
                }
                continue;
            }
            rules.push(self.create_automation_rule(
                owner_id,
                name,
                description,
                json!({"kind": "change_or_schedule", "source": source}),
                json!({"respect_attention_policy": true}),
                actions.into_iter().map(str::to_owned).collect(),
                AutomationFrequencyLimit {
                    max_runs,
                    window_minutes: 1_440,
                },
                json!({"origin": "voiceos-default", "reviewed": true}),
                true,
            )?);
        }
        Ok(rules)
    }
}

fn automation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRule> {
    let trigger: String = row.get(5)?;
    let conditions: String = row.get(6)?;
    let actions: String = row.get(7)?;
    let evidence: String = row.get(10)?;
    Ok(AutomationRule {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        enabled: row.get(4)?,
        trigger: parse_json(trigger)?,
        conditions: parse_json(conditions)?,
        permitted_actions: parse_json(actions)?,
        frequency_limit: AutomationFrequencyLimit {
            max_runs: row.get(8)?,
            window_minutes: row.get(9)?,
        },
        evidence: parse_json(evidence)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
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

fn validate_rule(
    name: &str,
    description: &str,
    trigger: &Value,
    conditions: &Value,
    actions: &[String],
    frequency: &AutomationFrequencyLimit,
    evidence: &Value,
) -> Result<(), StoreError> {
    if name.trim().is_empty() || description.trim().is_empty() || name.len() > 120 {
        return Err(StoreError::InvalidInput(
            "automation name and description are required".to_owned(),
        ));
    }
    if !trigger.is_object()
        || !conditions.is_object()
        || !(evidence.is_object() || evidence.is_array())
        || evidence.as_object().is_some_and(|value| value.is_empty())
        || evidence.as_array().is_some_and(|value| value.is_empty())
    {
        return Err(StoreError::InvalidInput(
            "automation trigger, conditions, and evidence must be structured".to_owned(),
        ));
    }
    let kind = trigger
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source = trigger
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !["event", "schedule", "change", "change_or_schedule"].contains(&kind)
        || source.trim().is_empty()
    {
        return Err(StoreError::InvalidInput(
            "automation trigger kind and source are required".to_owned(),
        ));
    }
    let allowed = [
        "notify.needs_you",
        "digest.add",
        "digest.deliver",
        "model.classify",
        "planning.prompt",
        "review.scan",
    ];
    if actions.is_empty()
        || actions
            .iter()
            .any(|action| !allowed.contains(&action.as_str()))
    {
        return Err(StoreError::InvalidInput(
            "automation action is not permitted".to_owned(),
        ));
    }
    if !(1..=1_000).contains(&frequency.max_runs)
        || !(1..=43_200).contains(&frequency.window_minutes)
    {
        return Err(StoreError::InvalidInput(
            "automation frequency limit is invalid".to_owned(),
        ));
    }
    Ok(())
}
