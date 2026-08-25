use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rusqlite::{OptionalExtension, Row, params};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ConversationStore, FocusPriority, FocusSessionRecord, FocusSnapshot, PersonalFocusReset,
    StoreError, TaskDetail,
};

const FOCUS_MODES: &[&str] = &["normal", "five_minute", "low_energy", "restart"];

impl ConversationStore {
    pub fn focus_snapshot(&self, owner_id: &str, mode: &str) -> Result<FocusSnapshot, StoreError> {
        validate_mode(mode)?;
        let active_session = self.active_focus_session(owner_id)?;
        let last_interrupted_session = self.last_interrupted_focus_session(owner_id)?;
        let context = self.focus_project_context(owner_id)?;
        let mut priorities = Vec::new();
        let mut parked = Vec::new();
        for task in self.tasks(owner_id, false, 100)? {
            if !matches!(task.status.as_str(), "active" | "ready" | "proposed") {
                continue;
            }
            if let Some(detail) = self.task_detail(owner_id, &task.id)? {
                let is_parked = task.status == "proposed";
                if !is_parked && detail.progress.lane == "vic_working" {
                    continue;
                }
                let next_action = recommended_action(&detail);
                let (project_title, goal_title) = task
                    .project_id
                    .as_ref()
                    .and_then(|project_id| context.get(project_id))
                    .cloned()
                    .unwrap_or((None, None));
                let priority = FocusPriority {
                    task_id: task.id,
                    title: task.title,
                    observable_outcome: task.observable_outcome,
                    estimated_minutes: task.estimated_minutes,
                    due_at: task.due_at.clone(),
                    importance: task.importance,
                    urgency: urgency_label(task.due_at.as_deref()),
                    status: task.status,
                    next_action,
                    project_title,
                    goal_title,
                };
                if is_parked {
                    parked.push(priority);
                } else {
                    priorities.push(priority);
                }
            }
        }

        priorities.sort_by(|left, right| focus_order(left, right, mode));
        if let Some(session) = &active_session {
            if let Some(position) = priorities
                .iter()
                .position(|priority| priority.task_id == session.task_id)
            {
                priorities.swap(0, position);
                priorities[0].next_action = session.next_action.clone();
            }
        } else if let Some(session) = &last_interrupted_session {
            if let Some(position) = priorities
                .iter()
                .position(|priority| priority.task_id == session.task_id)
            {
                priorities.swap(0, position);
                priorities[0].next_action = session
                    .restart_action
                    .clone()
                    .unwrap_or_else(|| session.next_action.clone());
            }
        }
        priorities.truncate(3);
        let recommendation = priorities.first().cloned();
        Ok(FocusSnapshot {
            mode: mode.to_owned(),
            active_session,
            priorities,
            recommendation,
            last_interrupted_session,
            parked,
        })
    }

    pub fn personal_focus_reset(
        &self,
        owner_id: &str,
        mode: &str,
    ) -> Result<PersonalFocusReset, StoreError> {
        let mut snapshot = self.focus_snapshot(owner_id, mode)?;
        if snapshot.active_session.is_none() {
            if let Some(interrupted) = &snapshot.last_interrupted_session {
                if let Some(position) = snapshot
                    .priorities
                    .iter()
                    .position(|priority| priority.task_id == interrupted.task_id)
                {
                    snapshot.priorities.swap(0, position);
                    snapshot.recommendation = snapshot.priorities.first().cloned();
                }
            }
        }

        let restart_action = snapshot
            .last_interrupted_session
            .as_ref()
            .filter(|_| snapshot.active_session.is_none())
            .and_then(|session| session.restart_action.clone())
            .or_else(|| {
                snapshot
                    .recommendation
                    .as_ref()
                    .map(|task| task.next_action.clone())
            });
        let has_recommendation = snapshot.recommendation.is_some();
        let message = if snapshot.active_session.is_some() {
            "You already have a small focus session in progress. You can return to its next step."
        } else if snapshot.last_interrupted_session.is_some() {
            "You were interrupted; your restart point is saved. You can begin there."
        } else if has_recommendation {
            "You do not need to solve everything right now. Start with this one small step."
        } else {
            "There is nothing you need to catch up on right now. We can start with one small capture."
        };
        let first_physical_action = restart_action.or_else(|| {
            (!has_recommendation).then(|| "Capture the next thing pulling at you.".to_owned())
        });
        let five_minute_version = first_physical_action
            .as_ref()
            .map(|action| format!("Spend five minutes: {action}"));

        Ok(PersonalFocusReset {
            active_session: snapshot.active_session,
            interrupted_session: snapshot.last_interrupted_session,
            priorities: snapshot.priorities,
            recommendation: snapshot.recommendation,
            first_physical_action,
            five_minute_version,
            optional_question: None,
            message: message.to_owned(),
        })
    }

    pub fn start_focus_session(
        &self,
        owner_id: &str,
        task_id: &str,
        mode: &str,
        planned_minutes: u32,
        actor: &str,
    ) -> Result<FocusSessionRecord, StoreError> {
        validate_mode(mode)?;
        validate_minutes(planned_minutes)?;
        if self.active_focus_session(owner_id)?.is_some() {
            return Err(StoreError::InvalidInput(
                "finish or interrupt the active focus session first".to_owned(),
            ));
        }
        let detail = self
            .task_detail(owner_id, task_id)?
            .ok_or_else(|| StoreError::InvalidInput("focus task was not found".to_owned()))?;
        if !matches!(detail.task.status.as_str(), "active" | "ready") {
            return Err(StoreError::InvalidInput(
                "focus task must be active or ready".to_owned(),
            ));
        }
        let next_action = recommended_action(&detail);
        let step_id = next_step_id(&detail);
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO focus_sessions(focus_session_id, owner_id, task_id, step_id, mode, planned_minutes, status, next_action, started_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?8)",
            params![id, owner_id.trim(), task_id.trim(), step_id, mode, planned_minutes, next_action, now],
        )?;
        drop(connection);
        let session = self
            .focus_session(owner_id, &id)?
            .expect("inserted focus session");
        self.append_execution_event(
            owner_id,
            task_id,
            "focus.started",
            actor,
            json!({"session_id": id, "mode": mode, "planned_minutes": planned_minutes, "next_action": session.next_action}),
        )?;
        Ok(session)
    }

    pub fn interrupt_focus_session(
        &self,
        owner_id: &str,
        session_id: &str,
        note: &str,
        restart_action: Option<&str>,
        actor: &str,
    ) -> Result<FocusSessionRecord, StoreError> {
        let session = self.require_focus_session(owner_id, session_id)?;
        if session.status != "active" {
            return Err(StoreError::InvalidInput(
                "only an active focus session can be interrupted".to_owned(),
            ));
        }
        let note = clean_optional_text(note, 1_000)?.unwrap_or_else(|| "Interrupted".to_owned());
        let restart_action =
            clean_optional_text(restart_action.unwrap_or(&session.next_action), 1_000)?
                .unwrap_or_else(|| session.next_action.clone());
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "UPDATE focus_sessions SET status='interrupted', interruption_note=?3, restart_action=?4, ended_at=?5, updated_at=?5 WHERE owner_id=?1 AND focus_session_id=?2 AND status='active'",
            params![owner_id.trim(), session_id.trim(), note, restart_action, now],
        )?;
        drop(connection);
        let updated = self.require_focus_session(owner_id, session_id)?;
        self.append_execution_event(
            owner_id,
            &updated.task_id,
            "focus.interrupted",
            actor,
            json!({"session_id": session_id, "note": updated.interruption_note, "restart_action": updated.restart_action}),
        )?;
        Ok(updated)
    }

    pub fn resume_focus_session(
        &self,
        owner_id: &str,
        session_id: &str,
        planned_minutes: u32,
        actor: &str,
    ) -> Result<FocusSessionRecord, StoreError> {
        validate_minutes(planned_minutes)?;
        if self.active_focus_session(owner_id)?.is_some() {
            return Err(StoreError::InvalidInput(
                "a focus session is already active".to_owned(),
            ));
        }
        let session = self.require_focus_session(owner_id, session_id)?;
        if session.status != "interrupted" {
            return Err(StoreError::InvalidInput(
                "only an interrupted focus session can be restarted".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let next_action = session
            .restart_action
            .clone()
            .unwrap_or_else(|| session.next_action.clone());
        let connection = self.connection()?;
        connection.execute(
            "UPDATE focus_sessions SET status='active', mode='restart', planned_minutes=?3, next_action=?4, ended_at=NULL, updated_at=?5 WHERE owner_id=?1 AND focus_session_id=?2 AND status='interrupted'",
            params![owner_id.trim(), session_id.trim(), planned_minutes, next_action, now],
        )?;
        drop(connection);
        let updated = self.require_focus_session(owner_id, session_id)?;
        self.append_execution_event(
            owner_id,
            &updated.task_id,
            "focus.restarted",
            actor,
            json!({"session_id": session_id, "planned_minutes": planned_minutes, "next_action": updated.next_action}),
        )?;
        Ok(updated)
    }

    pub fn complete_focus_session(
        &self,
        owner_id: &str,
        session_id: &str,
        reflection: Option<&str>,
        restart_action: Option<&str>,
        actor: &str,
    ) -> Result<FocusSessionRecord, StoreError> {
        let session = self.require_focus_session(owner_id, session_id)?;
        if session.status != "active" {
            return Err(StoreError::InvalidInput(
                "only an active focus session can be completed".to_owned(),
            ));
        }
        let reflection = clean_optional_text(reflection.unwrap_or(""), 2_000)?;
        let restart_action = clean_optional_text(restart_action.unwrap_or(""), 1_000)?;
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "UPDATE focus_sessions SET status='completed', reflection=?3, restart_action=?4, ended_at=?5, updated_at=?5 WHERE owner_id=?1 AND focus_session_id=?2 AND status='active'",
            params![owner_id.trim(), session_id.trim(), reflection, restart_action, now],
        )?;
        drop(connection);
        let updated = self.require_focus_session(owner_id, session_id)?;
        self.append_execution_event(
            owner_id,
            &updated.task_id,
            "focus.completed",
            actor,
            json!({"session_id": session_id, "reflection": updated.reflection, "restart_action": updated.restart_action}),
        )?;
        Ok(updated)
    }

    pub fn active_focus_session(
        &self,
        owner_id: &str,
    ) -> Result<Option<FocusSessionRecord>, StoreError> {
        self.focus_session_by_status(owner_id, "active")
    }

    pub fn last_interrupted_focus_session(
        &self,
        owner_id: &str,
    ) -> Result<Option<FocusSessionRecord>, StoreError> {
        self.focus_session_by_status(owner_id, "interrupted")
    }

    fn focus_session_by_status(
        &self,
        owner_id: &str,
        status: &str,
    ) -> Result<Option<FocusSessionRecord>, StoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT focus_session_id, owner_id, task_id, step_id, mode, planned_minutes, status, next_action, interruption_note, restart_action, reflection, started_at, updated_at, ended_at FROM focus_sessions WHERE owner_id=?1 AND status=?2 ORDER BY updated_at DESC LIMIT 1",
                params![owner_id.trim(), status],
                focus_session_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    fn focus_session(
        &self,
        owner_id: &str,
        session_id: &str,
    ) -> Result<Option<FocusSessionRecord>, StoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT focus_session_id, owner_id, task_id, step_id, mode, planned_minutes, status, next_action, interruption_note, restart_action, reflection, started_at, updated_at, ended_at FROM focus_sessions WHERE owner_id=?1 AND focus_session_id=?2",
                params![owner_id.trim(), session_id.trim()],
                focus_session_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    fn require_focus_session(
        &self,
        owner_id: &str,
        session_id: &str,
    ) -> Result<FocusSessionRecord, StoreError> {
        self.focus_session(owner_id, session_id)?
            .ok_or_else(|| StoreError::InvalidInput("focus session was not found".to_owned()))
    }

    fn focus_project_context(
        &self,
        owner_id: &str,
    ) -> Result<HashMap<String, (Option<String>, Option<String>)>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT p.project_id, p.title, g.title FROM projects p LEFT JOIN goals g ON g.goal_id=p.goal_id AND g.owner_id=p.owner_id WHERE p.owner_id=?1",
        )?;
        let rows = statement.query_map([owner_id.trim()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ),
            ))
        })?;
        Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
    }
}

fn focus_order(left: &FocusPriority, right: &FocusPriority, mode: &str) -> std::cmp::Ordering {
    let deadline =
        deadline_rank(left.due_at.as_deref()).cmp(&deadline_rank(right.due_at.as_deref()));
    if deadline != std::cmp::Ordering::Equal {
        return deadline;
    }
    let importance = importance_rank(&left.importance).cmp(&importance_rank(&right.importance));
    if importance != std::cmp::Ordering::Equal {
        return importance;
    }
    if matches!(mode, "five_minute" | "low_energy") {
        return left.estimated_minutes.cmp(&right.estimated_minutes);
    }
    status_rank(&left.status).cmp(&status_rank(&right.status))
}

fn deadline_rank(value: Option<&str>) -> u8 {
    let Some(due_at) = value.and_then(parse_due_at) else {
        return 5;
    };
    let remaining = due_at - Utc::now();
    if remaining <= Duration::zero() {
        0
    } else if remaining <= Duration::hours(24) {
        1
    } else if remaining <= Duration::days(3) {
        2
    } else if remaining <= Duration::days(7) {
        3
    } else {
        4
    }
}

fn urgency_label(value: Option<&str>) -> String {
    match deadline_rank(value) {
        0 => "overdue",
        1 => "due_today",
        2 => "due_soon",
        3 => "due_this_week",
        4 => "scheduled",
        _ => "unscheduled",
    }
    .to_owned()
}

fn parse_due_at(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

fn importance_rank(value: &str) -> u8 {
    match value {
        "critical" => 0,
        "high" => 1,
        "normal" => 2,
        _ => 3,
    }
}

fn status_rank(value: &str) -> u8 {
    if value == "active" { 0 } else { 1 }
}

fn recommended_action(detail: &TaskDetail) -> String {
    detail
        .progress
        .next_user_action
        .clone()
        .or_else(|| {
            detail
                .steps
                .iter()
                .find(|step| {
                    step.status != "completed" && matches!(step.owner.as_str(), "user" | "shared")
                })
                .map(|step| step.title.clone())
        })
        .unwrap_or_else(|| detail.task.observable_outcome.clone())
}

fn next_step_id(detail: &TaskDetail) -> Option<String> {
    detail
        .steps
        .iter()
        .find(|step| step.status != "completed" && matches!(step.owner.as_str(), "user" | "shared"))
        .map(|step| step.id.clone())
}

fn validate_mode(mode: &str) -> Result<(), StoreError> {
    if FOCUS_MODES.contains(&mode) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput("invalid focus mode".to_owned()))
    }
}

fn validate_minutes(minutes: u32) -> Result<(), StoreError> {
    if (1..=120).contains(&minutes) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput(
            "focus session must be between 1 and 120 minutes".to_owned(),
        ))
    }
}

fn clean_optional_text(value: &str, maximum: usize) -> Result<Option<String>, StoreError> {
    let value = value.trim();
    if value.chars().count() > maximum {
        return Err(StoreError::InvalidInput(
            "focus note is too long".to_owned(),
        ));
    }
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

fn focus_session_row(row: &Row<'_>) -> rusqlite::Result<FocusSessionRecord> {
    Ok(FocusSessionRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        task_id: row.get(2)?,
        step_id: row.get(3)?,
        mode: row.get(4)?,
        planned_minutes: row.get(5)?,
        status: row.get(6)?,
        next_action: row.get(7)?,
        interruption_note: row.get(8)?,
        restart_action: row.get(9)?,
        reflection: row.get(10)?,
        started_at: row.get(11)?,
        updated_at: row.get(12)?,
        ended_at: row.get(13)?,
    })
}
