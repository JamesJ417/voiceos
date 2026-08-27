use chrono::{Duration, Utc};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{ConversationStore, StoreError, TaskReviewClaim, TaskReviewRun, TaskReviewSnapshot};

impl ConversationStore {
    pub fn claim_next_task_review(
        &self,
        owner_id: &str,
        lease_seconds: i64,
    ) -> Result<Option<TaskReviewClaim>, StoreError> {
        if owner_id.trim().is_empty() || !(1..=3600).contains(&lease_seconds) {
            return Err(StoreError::InvalidInput("invalid review claim".into()));
        }
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let lease = (now + Duration::seconds(lease_seconds)).to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE task_review_runs SET status='expired', completed_at=?2 WHERE owner_id=?1 AND status='running' AND lease_expires_at<=?2",
            params![owner_id.trim(), now_text],
        )?;
        transaction.execute(
            "UPDATE task_review_state SET active_review_id=NULL, updated_at=?2 WHERE owner_id=?1 AND active_review_id IN (SELECT review_id FROM task_review_runs WHERE owner_id=?1 AND status!='running')",
            params![owner_id.trim(), now_text],
        )?;
        let active_id: Option<String> = transaction
            .query_row(
                "SELECT review_id FROM task_review_runs WHERE owner_id=?1 AND status='running' AND lease_expires_at>?2 ORDER BY started_at, review_id LIMIT 1",
                params![owner_id.trim(), now_text],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(active_id) = active_id {
            drop(transaction);
            drop(connection);
            let review = self
                .task_review_run(owner_id, &active_id)?
                .expect("active review exists");
            let task = self
                .task_detail(owner_id, &review.task_id)?
                .expect("active task exists");
            return Ok(Some(TaskReviewClaim { review, task }));
        }
        let cursor: Option<String> = transaction
            .query_row(
                "SELECT cursor_task_id FROM task_review_state WHERE owner_id=?1",
                params![owner_id.trim()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let mut tasks = transaction.prepare(
            "SELECT task_id FROM tasks WHERE owner_id=?1 AND status IN ('proposed','ready','active') ORDER BY created_at, task_id",
        )?;
        let task_ids = tasks
            .query_map(params![owner_id.trim()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let Some(task_id) = task_ids
            .iter()
            .position(|id| cursor.as_ref() == Some(id))
            .map(|i| task_ids[(i + 1) % task_ids.len()].clone())
            .or_else(|| task_ids.first().cloned())
        else {
            return Ok(None);
        };
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO task_review_runs(review_id,owner_id,task_id,status,lease_expires_at,started_at) VALUES(?1,?2,?3,'running',?4,?5)",
            params![id, owner_id.trim(), task_id, lease, now_text],
        )?;
        transaction.execute(
            "INSERT INTO task_review_state(owner_id,cursor_task_id,active_review_id,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(owner_id) DO UPDATE SET cursor_task_id=excluded.cursor_task_id,active_review_id=excluded.active_review_id,updated_at=excluded.updated_at",
            params![owner_id.trim(), task_id, id, now_text],
        )?;
        drop(tasks);
        transaction.commit()?;
        drop(connection);
        self.append_execution_event(
            owner_id,
            &task_id,
            "task.review.claimed",
            "review-loop",
            serde_json::json!({"review_id": id}),
        )?;
        let review = self
            .task_review_run(owner_id, &id)?
            .expect("claimed review exists");
        let task = self
            .task_detail(owner_id, &task_id)?
            .expect("claimed task exists");
        Ok(Some(TaskReviewClaim { review, task }))
    }

    pub fn complete_task_review(
        &self,
        owner_id: &str,
        review_id: &str,
        summary: &str,
        safe_actions: Vec<String>,
        blockers: Vec<String>,
        ideas: Vec<String>,
    ) -> Result<(), StoreError> {
        validate_findings(summary, &safe_actions)?;
        validate_findings(summary, &blockers)?;
        validate_findings(summary, &ideas)?;
        self.finish_task_review(
            owner_id,
            review_id,
            "completed",
            Some(summary),
            Some((safe_actions, blockers, ideas)),
            None,
        )
    }
    pub fn fail_task_review(
        &self,
        owner_id: &str,
        review_id: &str,
        error_code: &str,
    ) -> Result<(), StoreError> {
        if error_code.trim().is_empty() || error_code.len() > 120 {
            return Err(StoreError::InvalidInput("invalid review error".into()));
        }
        self.finish_task_review(owner_id, review_id, "failed", None, None, Some(error_code))
    }
    fn finish_task_review(
        &self,
        owner_id: &str,
        review_id: &str,
        status: &str,
        summary: Option<&str>,
        findings: Option<(Vec<String>, Vec<String>, Vec<String>)>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task_id: Option<String> = transaction.query_row("SELECT task_id FROM task_review_runs WHERE owner_id=?1 AND review_id=?2 AND status='running'", params![owner_id.trim(), review_id.trim()], |row| row.get(0)).optional()?;
        let Some(task_id) = task_id else {
            return Err(StoreError::InvalidInput("review is not running".into()));
        };
        let (safe, blockers, ideas) = findings.unwrap_or_default();
        transaction.execute("UPDATE task_review_runs SET status=?3,safe_actions_json=?4,blockers_json=?5,ideas_json=?6,summary=?7,error_code=?8,completed_at=?9 WHERE owner_id=?1 AND review_id=?2", params![owner_id.trim(), review_id.trim(), status, serde_json::to_string(&safe)?, serde_json::to_string(&blockers)?, serde_json::to_string(&ideas)?, summary.map(str::trim), error, now])?;
        transaction.execute("UPDATE task_review_state SET active_review_id=NULL,updated_at=?2 WHERE owner_id=?1 AND active_review_id=?3", params![owner_id.trim(), now, review_id.trim()])?;
        transaction.commit()?;
        drop(connection);
        self.append_execution_event(
            owner_id,
            &task_id,
            if status == "completed" {
                "task.review.completed"
            } else {
                "task.review.failed"
            },
            "review-loop",
            serde_json::json!({"review_id": review_id, "summary": summary, "error_code": error}),
        )?;
        Ok(())
    }
    pub fn task_review_snapshot(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<TaskReviewSnapshot, StoreError> {
        let connection = self.connection()?;
        let cursor_task_id: Option<String> = connection
            .query_row(
                "SELECT cursor_task_id FROM task_review_state WHERE owner_id=?1",
                params![owner_id.trim()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let mut statement = connection.prepare("SELECT review_id,task_id,status,lease_expires_at,safe_actions_json,blockers_json,ideas_json,summary,error_code,started_at,completed_at FROM task_review_runs WHERE owner_id=?1 ORDER BY started_at DESC LIMIT ?2")?;
        let reviews = statement
            .query_map(params![owner_id.trim(), limit.clamp(1, 50)], review_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TaskReviewSnapshot {
            active_review: reviews.iter().find(|r| r.status == "running").cloned(),
            last_completed_review: reviews.iter().find(|r| r.status == "completed").cloned(),
            recent_reviews: reviews,
            cursor_task_id,
        })
    }
    fn task_review_run(
        &self,
        owner_id: &str,
        review_id: &str,
    ) -> Result<Option<TaskReviewRun>, StoreError> {
        let connection = self.connection()?;
        connection.query_row("SELECT review_id,task_id,status,lease_expires_at,safe_actions_json,blockers_json,ideas_json,summary,error_code,started_at,completed_at FROM task_review_runs WHERE owner_id=?1 AND review_id=?2", params![owner_id.trim(), review_id.trim()], review_row).optional().map_err(StoreError::from)
    }
}
fn validate_findings(summary: &str, values: &[String]) -> Result<(), StoreError> {
    if summary.trim().is_empty()
        || summary.chars().count() > 1000
        || values.len() > 3
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.chars().count() > 500)
    {
        Err(StoreError::InvalidInput("invalid review findings".into()))
    } else {
        Ok(())
    }
}
fn review_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskReviewRun> {
    let decode = |index| {
        let text: String = row.get(index)?;
        serde_json::from_str::<Vec<String>>(&text).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                text.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    };
    Ok(TaskReviewRun {
        id: row.get(0)?,
        task_id: row.get(1)?,
        status: row.get(2)?,
        lease_expires_at: row.get(3)?,
        safe_actions: decode(4)?,
        blockers: decode(5)?,
        ideas: decode(6)?,
        summary: row.get(7)?,
        error_code: row.get(8)?,
        started_at: row.get(9)?,
        completed_at: row.get(10)?,
    })
}
