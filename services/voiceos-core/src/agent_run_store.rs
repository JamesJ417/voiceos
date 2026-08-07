use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{AgentRunProgressUpdate, AgentRunRecord, ConversationStore, StoreError};

const MAX_OBJECTIVE_CHARS: usize = 16_384;

impl ConversationStore {
    pub fn claim_next_agent_run(
        &self,
        owner_id: &str,
        actor: &str,
    ) -> Result<Option<AgentRunRecord>, StoreError> {
        require_agent_text("owner_id", owner_id)?;
        require_agent_text("actor", actor)?;
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let run_id: Option<String> = transaction
            .query_row(
                "SELECT run_id FROM agent_runs WHERE owner_id=?1 AND status='queued' ORDER BY created_at LIMIT 1",
                [owner_id.trim()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(run_id) = run_id else {
            transaction.commit()?;
            return Ok(None);
        };
        let changed = transaction.execute(
            "UPDATE agent_runs SET status='starting',current_activity='Starting Codex',started_at=?3,updated_at=?3 WHERE owner_id=?1 AND run_id=?2 AND status='queued'",
            params![owner_id.trim(),run_id,now],
        )?;
        transaction.commit()?;
        drop(connection);
        if changed != 1 {
            return Ok(None);
        }
        let run = self
            .agent_run(owner_id, &run_id)?
            .ok_or_else(|| StoreError::InvalidInput("claimed agent run not found".to_owned()))?;
        self.append_agent_event(
            &run,
            "agent.run.starting",
            actor,
            json!({"from":"queued","to":"starting"}),
        )?;
        Ok(Some(run))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_run(
        &self,
        owner_id: &str,
        task_id: Option<&str>,
        parent_run_id: Option<&str>,
        idempotency_key: &str,
        role: &str,
        objective: &str,
        model: &str,
        reasoning_effort: &str,
        sandbox: &str,
        capability_scope: Value,
        requested_by: &str,
    ) -> Result<AgentRunRecord, StoreError> {
        for (label, value) in [
            ("owner_id", owner_id),
            ("idempotency_key", idempotency_key),
            ("role", role),
            ("objective", objective),
            ("model", model),
            ("requested_by", requested_by),
        ] {
            require_agent_text(label, value)?;
        }
        if objective.chars().count() > MAX_OBJECTIVE_CHARS {
            return Err(StoreError::InvalidInput(
                "agent objective exceeds size limit".to_owned(),
            ));
        }
        if !matches!(
            role,
            "coordinator" | "implementer" | "researcher" | "reviewer" | "tester" | "security"
        ) {
            return Err(StoreError::InvalidInput(
                "unsupported agent role".to_owned(),
            ));
        }
        if !matches!(
            reasoning_effort,
            "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
        ) {
            return Err(StoreError::InvalidInput(
                "unsupported reasoning effort".to_owned(),
            ));
        }
        if !matches!(sandbox, "read-only" | "workspace-write") {
            return Err(StoreError::InvalidInput(
                "agent sandbox must be read-only or workspace-write".to_owned(),
            ));
        }
        let capabilities = capability_scope.as_array().ok_or_else(|| {
            StoreError::InvalidInput("capability_scope must be a JSON array".to_owned())
        })?;
        if capabilities.len() > 64
            || capabilities
                .iter()
                .any(|value| value.as_str().is_none_or(|value| value.trim().is_empty()))
        {
            return Err(StoreError::InvalidInput(
                "capability_scope contains invalid entries".to_owned(),
            ));
        }

        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id.trim(), now],
        )?;
        if let Some(task_id) = task_id {
            let owned: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks WHERE owner_id=?1 AND task_id=?2)",
                params![owner_id.trim(), task_id.trim()],
                |row| row.get(0),
            )?;
            if !owned {
                return Err(StoreError::InvalidInput("task does not exist".to_owned()));
            }
        }
        if let Some(parent_run_id) = parent_run_id {
            let parent: Option<Option<String>> = connection
                .query_row(
                    "SELECT task_id FROM agent_runs WHERE owner_id=?1 AND run_id=?2",
                    params![owner_id.trim(), parent_run_id.trim()],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(parent_task_id) = parent else {
                return Err(StoreError::InvalidInput(
                    "parent agent run does not exist".to_owned(),
                ));
            };
            if parent_task_id.as_deref() != task_id {
                return Err(StoreError::InvalidInput(
                    "child agent run must inherit its parent task".to_owned(),
                ));
            }
        }
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO agent_runs(run_id,owner_id,task_id,parent_run_id,idempotency_key,role,objective,status,provider,model,reasoning_effort,sandbox,capability_scope_json,requested_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'queued','codex',?8,?9,?10,?11,?12,?13,?13)",
            params![id,owner_id.trim(),task_id,parent_run_id,idempotency_key.trim(),role,objective.trim(),model,reasoning_effort,sandbox,capability_scope.to_string(),requested_by.trim(),now],
        )?;
        drop(connection);
        let run = self
            .agent_run_by_key(owner_id, idempotency_key)?
            .ok_or_else(|| StoreError::InvalidInput("agent run was not created".to_owned()))?;
        if inserted == 1 {
            self.append_agent_event(
                &run,
                "agent.run.queued",
                requested_by,
                json!({
                    "role": run.role,
                    "model": run.model,
                    "reasoning_effort": run.reasoning_effort,
                    "sandbox": run.sandbox,
                    "capability_scope": run.capability_scope,
                    "parent_run_id": run.parent_run_id,
                }),
            )?;
        }
        Ok(run)
    }

    pub fn agent_run(
        &self,
        owner_id: &str,
        run_id: &str,
    ) -> Result<Option<AgentRunRecord>, StoreError> {
        self.connection()?
            .query_row(
                "SELECT run_id,owner_id,task_id,parent_run_id,idempotency_key,role,objective,status,provider,model,reasoning_effort,sandbox,capability_scope_json,codex_thread_id,current_activity,result_summary,error,requested_by,created_at,started_at,updated_at,completed_at FROM agent_runs WHERE owner_id=?1 AND run_id=?2",
                params![owner_id.trim(),run_id.trim()],
                agent_run_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn agent_runs(
        &self,
        owner_id: &str,
        task_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AgentRunRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT run_id,owner_id,task_id,parent_run_id,idempotency_key,role,objective,status,provider,model,reasoning_effort,sandbox,capability_scope_json,codex_thread_id,current_activity,result_summary,error,requested_by,created_at,started_at,updated_at,completed_at FROM agent_runs WHERE owner_id=?1 AND (?2 IS NULL OR task_id=?2) ORDER BY created_at DESC LIMIT ?3")?;
        Ok(statement
            .query_map(
                params![owner_id.trim(), task_id, limit.clamp(1, 500)],
                agent_run_row,
            )?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn transition_agent_run(
        &self,
        owner_id: &str,
        run_id: &str,
        expected_status: &str,
        next_status: &str,
        actor: &str,
        activity: Option<&str>,
    ) -> Result<Option<AgentRunRecord>, StoreError> {
        if !valid_agent_transition(expected_status, next_status) {
            return Err(StoreError::InvalidInput(
                "invalid agent run transition".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let completed_at =
            matches!(next_status, "completed" | "failed" | "cancelled").then_some(now.as_str());
        let started_at = (next_status == "starting").then_some(now.as_str());
        let changed = self.connection()?.execute(
            "UPDATE agent_runs SET status=?4,current_activity=COALESCE(?5,current_activity),started_at=COALESCE(started_at,?6),completed_at=?7,updated_at=?8 WHERE owner_id=?1 AND run_id=?2 AND status=?3",
            params![owner_id.trim(),run_id.trim(),expected_status,next_status,activity,started_at,completed_at,now],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        let run = self.agent_run(owner_id, run_id)?.ok_or_else(|| {
            StoreError::InvalidInput("agent run disappeared after transition".to_owned())
        })?;
        self.append_agent_event(
            &run,
            &format!("agent.run.{next_status}"),
            actor,
            json!({
                "from": expected_status, "to": next_status, "activity": activity,
            }),
        )?;
        Ok(Some(run))
    }

    pub fn update_agent_run_progress(
        &self,
        owner_id: &str,
        run_id: &str,
        actor: &str,
        update: AgentRunProgressUpdate,
    ) -> Result<AgentRunRecord, StoreError> {
        let event_kind = update.event_kind.as_str();
        let activity = update.activity.as_str();
        for (label, value) in [
            ("actor", actor),
            ("event_kind", event_kind),
            ("activity", activity),
        ] {
            require_agent_text(label, value)?;
        }
        if !event_kind.starts_with("agent.") || activity.chars().count() > 2_000 {
            return Err(StoreError::InvalidInput(
                "invalid agent progress event".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let changed = self.connection()?.execute(
            "UPDATE agent_runs SET current_activity=?3,codex_thread_id=COALESCE(?4,codex_thread_id),updated_at=?5 WHERE owner_id=?1 AND run_id=?2 AND status NOT IN ('completed','failed','cancelled')",
            params![owner_id.trim(),run_id.trim(),activity.trim(),update.codex_thread_id,now],
        )?;
        if changed != 1 {
            return Err(StoreError::InvalidInput(
                "agent run is not active".to_owned(),
            ));
        }
        let run = self
            .agent_run(owner_id, run_id)?
            .ok_or_else(|| StoreError::InvalidInput("agent run not found".to_owned()))?;
        self.append_agent_event(&run, event_kind, actor, json!({
            "activity": activity.trim(), "evidence": update.evidence, "codex_thread_id": update.codex_thread_id,
        }))?;
        Ok(run)
    }

    pub fn finish_agent_run(
        &self,
        owner_id: &str,
        run_id: &str,
        actor: &str,
        status: &str,
        result_summary: Option<&str>,
        error: Option<&str>,
    ) -> Result<Option<AgentRunRecord>, StoreError> {
        if !matches!(status, "completed" | "failed") {
            return Err(StoreError::InvalidInput(
                "invalid terminal agent status".to_owned(),
            ));
        }
        if status == "completed" {
            let active_children: i64 = self.connection()?.query_row(
                "SELECT COUNT(*) FROM agent_runs WHERE owner_id=?1 AND parent_run_id=?2 AND status NOT IN ('completed','failed','cancelled')",
                params![owner_id.trim(), run_id.trim()],
                |row| row.get(0),
            )?;
            if active_children > 0 {
                return Err(StoreError::InvalidInput(
                    "agent run has non-terminal child runs".to_owned(),
                ));
            }
        }
        let now = Utc::now().to_rfc3339();
        let changed = self.connection()?.execute(
            "UPDATE agent_runs SET status=?3,result_summary=?4,error=?5,current_activity=NULL,completed_at=?6,updated_at=?6 WHERE owner_id=?1 AND run_id=?2 AND status IN ('starting','running','waiting_approval','waiting_input')",
            params![owner_id.trim(),run_id.trim(),status,result_summary.map(|value| value.chars().take(16_384).collect::<String>()),error.map(|value| value.chars().take(2_000).collect::<String>()),now],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        let run = self
            .agent_run(owner_id, run_id)?
            .ok_or_else(|| StoreError::InvalidInput("agent run not found".to_owned()))?;
        self.append_agent_event(
            &run,
            &format!("agent.run.{status}"),
            actor,
            json!({
                "result_summary": run.result_summary, "error": run.error,
            }),
        )?;
        Ok(Some(run))
    }

    fn agent_run_by_key(
        &self,
        owner_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<AgentRunRecord>, StoreError> {
        self.connection()?.query_row(
            "SELECT run_id,owner_id,task_id,parent_run_id,idempotency_key,role,objective,status,provider,model,reasoning_effort,sandbox,capability_scope_json,codex_thread_id,current_activity,result_summary,error,requested_by,created_at,started_at,updated_at,completed_at FROM agent_runs WHERE owner_id=?1 AND idempotency_key=?2",
            params![owner_id.trim(),idempotency_key.trim()], agent_run_row,
        ).optional().map_err(StoreError::from)
    }

    fn append_agent_event(
        &self,
        run: &AgentRunRecord,
        event_type: &str,
        actor: &str,
        payload: Value,
    ) -> Result<(), StoreError> {
        self.append_execution_event(&run.owner_id, &run.id, event_type, actor, payload.clone())?;
        if let Some(task_id) = &run.task_id {
            self.append_execution_event(
                &run.owner_id,
                task_id,
                event_type,
                actor,
                json!({
                    "run_id": run.id, "parent_run_id": run.parent_run_id, "role": run.role,
                    "status": run.status, "detail": payload,
                }),
            )?;
        }
        Ok(())
    }
}

fn valid_agent_transition(current: &str, next: &str) -> bool {
    matches!(
        (current, next),
        ("queued", "starting")
            | ("queued", "cancelled")
            | ("starting", "running")
            | ("starting", "failed")
            | ("starting", "cancelled")
            | ("running", "waiting_approval")
            | ("running", "waiting_input")
            | ("running", "completed")
            | ("running", "failed")
            | ("running", "cancelled")
            | ("waiting_approval", "running")
            | ("waiting_approval", "failed")
            | ("waiting_approval", "cancelled")
            | ("waiting_input", "running")
            | ("waiting_input", "failed")
            | ("waiting_input", "cancelled")
    )
}

fn agent_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRunRecord> {
    let capability_scope: String = row.get(12)?;
    Ok(AgentRunRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        task_id: row.get(2)?,
        parent_run_id: row.get(3)?,
        idempotency_key: row.get(4)?,
        role: row.get(5)?,
        objective: row.get(6)?,
        status: row.get(7)?,
        provider: row.get(8)?,
        model: row.get(9)?,
        reasoning_effort: row.get(10)?,
        sandbox: row.get(11)?,
        capability_scope: serde_json::from_str(&capability_scope).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                capability_scope.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        codex_thread_id: row.get(13)?,
        current_activity: row.get(14)?,
        result_summary: row.get(15)?,
        error: row.get(16)?,
        requested_by: row.get(17)?,
        created_at: row.get(18)?,
        started_at: row.get(19)?,
        updated_at: row.get(20)?,
        completed_at: row.get(21)?,
    })
}

fn require_agent_text(label: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        Err(StoreError::InvalidInput(format!("{label} is required")))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::ConversationStore;

    #[test]
    fn parent_child_runs_are_durable_observable_and_task_scoped() {
        let store = ConversationStore::in_memory().unwrap();
        let task = store
            .create_task("owner", None, None, "Refactor", "Tests pass", 60)
            .unwrap();
        let parent = store
            .create_agent_run(
                "owner",
                Some(&task.id),
                None,
                "agent:parent",
                "coordinator",
                "Coordinate a bounded refactor",
                "gpt-5.6-sol",
                "high",
                "read-only",
                json!(["repo.read"]),
                "device:pixel",
            )
            .unwrap();
        let child = store
            .create_agent_run(
                "owner",
                Some(&task.id),
                Some(&parent.id),
                "agent:child",
                "reviewer",
                "Review the plan",
                "gpt-5.6-sol",
                "high",
                "read-only",
                json!(["repo.read"]),
                "agent:parent",
            )
            .unwrap();
        assert_eq!(child.parent_run_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(
            store.agent_runs("owner", Some(&task.id), 10).unwrap().len(),
            2
        );
        assert!(
            store
                .execution_events("owner", &task.id, 20)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "agent.run.queued")
        );
    }

    #[test]
    fn agent_run_lifecycle_is_compare_and_swap_and_terminal() {
        let store = ConversationStore::in_memory().unwrap();
        let run = store
            .create_agent_run(
                "owner",
                None,
                None,
                "agent:lifecycle",
                "implementer",
                "Implement one change",
                "gpt-5.6-sol",
                "high",
                "workspace-write",
                json!(["repo.read", "repo.write"]),
                "vic",
            )
            .unwrap();
        assert!(
            store
                .transition_agent_run(
                    "owner",
                    &run.id,
                    "queued",
                    "starting",
                    "codex-supervisor",
                    Some("Starting Codex")
                )
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .transition_agent_run(
                    "owner",
                    &run.id,
                    "queued",
                    "starting",
                    "codex-supervisor",
                    None
                )
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .transition_agent_run(
                    "owner",
                    &run.id,
                    "starting",
                    "running",
                    "codex-supervisor",
                    Some("Inspecting repository")
                )
                .unwrap()
                .is_some()
        );
        store
            .update_agent_run_progress(
                "owner",
                &run.id,
                "codex-supervisor",
                crate::AgentRunProgressUpdate {
                    event_kind: "agent.plan.updated".to_owned(),
                    activity: "Running tests".to_owned(),
                    evidence: json!({"step":1}),
                    codex_thread_id: Some("thr_123".to_owned()),
                },
            )
            .unwrap();
        let finished = store
            .finish_agent_run(
                "owner",
                &run.id,
                "codex-supervisor",
                "completed",
                Some("Implemented and verified"),
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(finished.status, "completed");
        assert!(
            store
                .update_agent_run_progress(
                    "owner",
                    &run.id,
                    "codex-supervisor",
                    crate::AgentRunProgressUpdate {
                        event_kind: "agent.command.started".to_owned(),
                        activity: "late".to_owned(),
                        evidence: json!({}),
                        codex_thread_id: None,
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn child_cannot_escape_parent_task_or_dangerous_sandbox() {
        let store = ConversationStore::in_memory().unwrap();
        let first = store
            .create_task("owner", None, None, "One", "One done", 10)
            .unwrap();
        let second = store
            .create_task("owner", None, None, "Two", "Two done", 10)
            .unwrap();
        let parent = store
            .create_agent_run(
                "owner",
                Some(&first.id),
                None,
                "agent:p",
                "coordinator",
                "Coordinate",
                "gpt-5.6-sol",
                "high",
                "read-only",
                json!([]),
                "vic",
            )
            .unwrap();
        assert!(
            store
                .create_agent_run(
                    "owner",
                    Some(&second.id),
                    Some(&parent.id),
                    "agent:escape",
                    "implementer",
                    "Escape",
                    "gpt-5.6-sol",
                    "high",
                    "workspace-write",
                    json!([]),
                    "vic"
                )
                .is_err()
        );
        assert!(
            store
                .create_agent_run(
                    "owner",
                    None,
                    None,
                    "agent:root",
                    "implementer",
                    "Root",
                    "gpt-5.6-sol",
                    "high",
                    "danger-full-access",
                    json!([]),
                    "vic"
                )
                .is_err()
        );
    }

    #[test]
    fn duplicate_delivery_returns_one_run_and_one_queued_event() {
        let store = ConversationStore::in_memory().unwrap();
        let first = store
            .create_agent_run(
                "owner",
                None,
                None,
                "agent:dedupe",
                "researcher",
                "Inspect the repository",
                "gpt-5.6-sol",
                "high",
                "read-only",
                json!(["repo.read"]),
                "vic",
            )
            .unwrap();
        let retry = store
            .create_agent_run(
                "owner",
                None,
                None,
                "agent:dedupe",
                "researcher",
                "Ignored retry text",
                "gpt-5.6-sol",
                "high",
                "read-only",
                json!(["repo.read"]),
                "vic",
            )
            .unwrap();
        assert_eq!(first.id, retry.id);
        let events = store.execution_events("owner", &first.id, 20).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "agent.run.queued")
                .count(),
            1
        );
    }

    #[test]
    fn parent_cannot_complete_before_its_child() {
        let store = ConversationStore::in_memory().unwrap();
        let parent = store
            .create_agent_run(
                "owner",
                None,
                None,
                "agent:p-order",
                "coordinator",
                "Coordinate",
                "gpt-5.6-sol",
                "high",
                "read-only",
                json!([]),
                "vic",
            )
            .unwrap();
        store
            .transition_agent_run(
                "owner",
                &parent.id,
                "queued",
                "starting",
                "supervisor",
                None,
            )
            .unwrap();
        store
            .transition_agent_run(
                "owner",
                &parent.id,
                "starting",
                "running",
                "supervisor",
                None,
            )
            .unwrap();
        let child = store
            .create_agent_run(
                "owner",
                None,
                Some(&parent.id),
                "agent:c-order",
                "tester",
                "Verify",
                "gpt-5.6-sol",
                "high",
                "read-only",
                json!([]),
                "supervisor",
            )
            .unwrap();
        assert!(
            store
                .finish_agent_run(
                    "owner",
                    &parent.id,
                    "supervisor",
                    "completed",
                    Some("too early"),
                    None
                )
                .is_err()
        );
        store
            .transition_agent_run("owner", &child.id, "queued", "starting", "supervisor", None)
            .unwrap();
        store
            .finish_agent_run(
                "owner",
                &child.id,
                "supervisor",
                "failed",
                None,
                Some("bounded failure"),
            )
            .unwrap();
        assert!(
            store
                .finish_agent_run(
                    "owner",
                    &parent.id,
                    "supervisor",
                    "completed",
                    Some("reviewed child result"),
                    None
                )
                .unwrap()
                .is_some()
        );
    }
}
