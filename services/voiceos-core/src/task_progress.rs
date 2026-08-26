use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ConversationStore, StoreError, TaskArtifactRecord, TaskBlockerRecord, TaskDetail,
    TaskHandoffRecord, TaskProgress, TaskStepRecord,
};

impl ConversationStore {
    pub(crate) fn backfill_task_progress(&self) -> Result<(), StoreError> {
        let tasks = {
            let connection = self.connection()?;
            let mut statement = connection.prepare(
                "SELECT task_id, owner_id, title, observable_outcome FROM tasks t WHERE status NOT IN ('completed', 'cancelled') AND NOT EXISTS (SELECT 1 FROM task_steps s WHERE s.owner_id=t.owner_id AND s.task_id=t.task_id)",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for (task_id, owner_id, title, outcome) in tasks {
            self.create_task_step(
                &owner_id,
                &task_id,
                &format!("VIC prepares the smallest useful next step for {title}"),
                "vic",
                "voiceos-migration",
            )?;
            self.create_task_step(
                &owner_id,
                &task_id,
                &format!("Confirm the completion target: {outcome}"),
                "user",
                "voiceos-migration",
            )?;
            self.create_task_step(
                &owner_id,
                &task_id,
                "Resolve identified blockers and dependencies",
                "shared",
                "voiceos-migration",
            )?;
        }
        Ok(())
    }

    pub fn create_task_step(
        &self,
        owner_id: &str,
        task_id: &str,
        title: &str,
        assigned_owner: &str,
        actor: &str,
    ) -> Result<TaskStepRecord, StoreError> {
        validate_text("step title", title)?;
        validate_party(assigned_owner, true)?;
        self.require_task(owner_id, task_id)?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        let position: u32 = connection.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM task_steps WHERE owner_id=?1 AND task_id=?2",
            params![owner_id.trim(), task_id.trim()],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO task_steps(step_id, owner_id, task_id, title, assigned_owner, status, evidence_json, position, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, 'pending', '{}', ?6, ?7, ?7)",
            params![id, owner_id.trim(), task_id.trim(), title.trim(), assigned_owner, position, now],
        )?;
        drop(connection);
        let step = self
            .task_step(owner_id, task_id, &id)?
            .expect("inserted step");
        self.append_execution_event(
            owner_id,
            task_id,
            "task.step.created",
            actor,
            serde_json::to_value(&step)?,
        )?;
        Ok(step)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_task_step(
        &self,
        owner_id: &str,
        task_id: &str,
        step_id: &str,
        status: &str,
        assigned_owner: Option<&str>,
        evidence: Value,
        actor: &str,
    ) -> Result<Option<TaskStepRecord>, StoreError> {
        validate_step_status(status)?;
        if let Some(owner) = assigned_owner {
            validate_party(owner, true)?;
        }
        if !evidence.is_object() {
            return Err(StoreError::InvalidInput(
                "step evidence must be an object".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE task_steps SET status=?4, assigned_owner=COALESCE(?5, assigned_owner), evidence_json=?6, updated_at=?7 WHERE owner_id=?1 AND task_id=?2 AND step_id=?3",
            params![owner_id.trim(), task_id.trim(), step_id.trim(), status, assigned_owner, evidence.to_string(), now],
        )?;
        drop(connection);
        if changed == 0 {
            return Ok(None);
        }
        let step = self
            .task_step(owner_id, task_id, step_id)?
            .expect("updated step");
        self.append_execution_event(
            owner_id,
            task_id,
            "task.step.updated",
            actor,
            serde_json::to_value(&step)?,
        )?;
        Ok(Some(step))
    }

    /// Completes one stage, closes the handoff that delivered it, activates the next
    /// unfinished stage, and creates the next owner handoff as one transaction.
    pub fn advance_task_step(
        &self,
        owner_id: &str,
        task_id: &str,
        step_id: &str,
        summary: &str,
        evidence: Value,
        actor: &str,
    ) -> Result<Option<TaskDetail>, StoreError> {
        validate_text("stage completion summary", summary)?;
        if !evidence.is_object() {
            return Err(StoreError::InvalidInput(
                "step evidence must be an object".to_owned(),
            ));
        }
        let actor_owner = if actor.starts_with("provider:") || actor == "vic" {
            "vic"
        } else {
            "user"
        };
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current: Option<(String, String, u32)> = transaction
            .query_row(
                "SELECT title, status, position FROM task_steps WHERE owner_id=?1 AND task_id=?2 AND step_id=?3",
                params![owner_id.trim(), task_id.trim(), step_id.trim()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((current_title, current_status, current_position)) = current else {
            return Ok(None);
        };
        let unaccepted_handoff: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_handoffs WHERE owner_id=?1 AND task_id=?2 AND to_owner=?3 AND status='pending')",
            params![owner_id.trim(), task_id.trim(), actor_owner],
            |row| row.get(0),
        )?;
        if unaccepted_handoff {
            return Err(StoreError::InvalidInput(
                "handoff must be accepted before the stage can advance".to_owned(),
            ));
        }
        if current_status != "completed" {
            transaction.execute(
                "UPDATE task_steps SET status='completed', evidence_json=?4, updated_at=?5 WHERE owner_id=?1 AND task_id=?2 AND step_id=?3",
                params![owner_id.trim(), task_id.trim(), step_id.trim(), evidence.to_string(), now],
            )?;
        }
        transaction.execute(
            "UPDATE task_handoffs SET status='completed', completed_at=?4 WHERE owner_id=?1 AND task_id=?2 AND to_owner=?3 AND status IN ('pending','accepted')",
            params![owner_id.trim(), task_id.trim(), actor_owner, now],
        )?;
        let next: Option<(String, String, String, String)> = transaction
            .query_row(
                "SELECT step_id, title, assigned_owner, status FROM task_steps WHERE owner_id=?1 AND task_id=?2 AND status NOT IN ('completed','cancelled') ORDER BY position, created_at LIMIT 1",
                params![owner_id.trim(), task_id.trim()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let mut handoff_id = None;
        if let Some((next_id, next_title, next_owner, next_status)) = &next {
            if next_status == "pending" {
                transaction.execute(
                    "UPDATE task_steps SET status='active', updated_at=?4 WHERE owner_id=?1 AND task_id=?2 AND step_id=?3",
                    params![owner_id.trim(), task_id.trim(), next_id, now],
                )?;
            }
            transaction.execute(
                "UPDATE tasks SET status='active', updated_at=?3 WHERE owner_id=?1 AND task_id=?2",
                params![owner_id.trim(), task_id.trim(), now],
            )?;
            if matches!(next_owner.as_str(), "user" | "vic") && next_owner != actor_owner {
                let existing: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM task_handoffs WHERE owner_id=?1 AND task_id=?2 AND to_owner=?3 AND status IN ('pending','accepted'))",
                    params![owner_id.trim(), task_id.trim(), next_owner],
                    |row| row.get(0),
                )?;
                if !existing {
                    let id = Uuid::new_v4().to_string();
                    transaction.execute(
                        "INSERT INTO task_handoffs(handoff_id, owner_id, task_id, from_owner, to_owner, kind, summary, status, created_at) VALUES(?1, ?2, ?3, ?4, ?5, 'handoff', ?6, 'pending', ?7)",
                        params![id, owner_id.trim(), task_id.trim(), actor_owner, next_owner, format!("Stage ready: {next_title}"), now],
                    )?;
                    handoff_id = Some(id);
                }
            }
        } else {
            transaction.execute(
                "UPDATE tasks SET status='completed', updated_at=?3 WHERE owner_id=?1 AND task_id=?2",
                params![owner_id.trim(), task_id.trim(), now],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.append_execution_event(
            owner_id,
            task_id,
            "task.step.advanced",
            actor,
            json!({
                "step_id": step_id,
                "step_title": current_title,
                "step_position": current_position,
                "summary": summary.trim(),
                "next_step_id": next.as_ref().map(|item| item.0.as_str()),
                "next_owner": next.as_ref().map(|item| item.2.as_str()),
                "handoff_id": handoff_id,
                "task_completed": next.is_none(),
            }),
        )?;
        self.task_detail(owner_id, task_id)
    }

    pub fn create_task_blocker(
        &self,
        owner_id: &str,
        task_id: &str,
        description: &str,
        assigned_owner: &str,
        actor: &str,
    ) -> Result<TaskBlockerRecord, StoreError> {
        validate_text("blocker description", description)?;
        validate_party(assigned_owner, true)?;
        self.require_task(owner_id, task_id)?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO task_blockers(blocker_id, owner_id, task_id, description, assigned_owner, status, created_at) VALUES(?1, ?2, ?3, ?4, ?5, 'open', ?6)",
            params![id, owner_id.trim(), task_id.trim(), description.trim(), assigned_owner, now],
        )?;
        drop(connection);
        let blocker = self
            .task_blockers(owner_id, task_id)?
            .into_iter()
            .find(|item| item.id == id)
            .expect("inserted blocker");
        self.append_execution_event(
            owner_id,
            task_id,
            "task.blocker.created",
            actor,
            serde_json::to_value(&blocker)?,
        )?;
        Ok(blocker)
    }

    pub fn resolve_task_blocker(
        &self,
        owner_id: &str,
        task_id: &str,
        blocker_id: &str,
        actor: &str,
    ) -> Result<Option<TaskBlockerRecord>, StoreError> {
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE task_blockers SET status='resolved', resolved_at=?4 WHERE owner_id=?1 AND task_id=?2 AND blocker_id=?3 AND status='open'",
            params![owner_id.trim(), task_id.trim(), blocker_id.trim(), now],
        )?;
        drop(connection);
        if changed == 0 {
            return Ok(None);
        }
        let blocker = self
            .task_blockers(owner_id, task_id)?
            .into_iter()
            .find(|item| item.id == blocker_id)
            .expect("resolved blocker");
        self.append_execution_event(
            owner_id,
            task_id,
            "task.blocker.resolved",
            actor,
            serde_json::to_value(&blocker)?,
        )?;
        Ok(Some(blocker))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_task_handoff(
        &self,
        owner_id: &str,
        task_id: &str,
        from_owner: &str,
        to_owner: &str,
        kind: &str,
        summary: &str,
        actor: &str,
    ) -> Result<TaskHandoffRecord, StoreError> {
        validate_party(from_owner, false)?;
        validate_party(to_owner, false)?;
        if from_owner == to_owner {
            return Err(StoreError::InvalidInput(
                "handoff parties must differ".to_owned(),
            ));
        }
        if !matches!(kind, "handoff" | "review" | "approval") {
            return Err(StoreError::InvalidInput("invalid handoff kind".to_owned()));
        }
        validate_text("handoff summary", summary)?;
        self.require_task(owner_id, task_id)?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO task_handoffs(handoff_id, owner_id, task_id, from_owner, to_owner, kind, summary, status, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8)",
            params![id, owner_id.trim(), task_id.trim(), from_owner, to_owner, kind, summary.trim(), now],
        )?;
        drop(connection);
        let handoff = self
            .task_handoffs(owner_id, task_id)?
            .into_iter()
            .find(|item| item.id == id)
            .expect("inserted handoff");
        self.append_execution_event(
            owner_id,
            task_id,
            "task.handoff.created",
            actor,
            serde_json::to_value(&handoff)?,
        )?;
        Ok(handoff)
    }

    pub fn update_task_handoff(
        &self,
        owner_id: &str,
        task_id: &str,
        handoff_id: &str,
        status: &str,
        actor: &str,
    ) -> Result<Option<TaskHandoffRecord>, StoreError> {
        if !matches!(status, "accepted" | "completed" | "cancelled") {
            return Err(StoreError::InvalidInput(
                "invalid task handoff status".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let to_owner: Option<String> = transaction
            .query_row(
                "SELECT to_owner FROM task_handoffs WHERE owner_id=?1 AND task_id=?2 AND handoff_id=?3",
                params![owner_id.trim(), task_id.trim(), handoff_id.trim()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(to_owner) = to_owner else {
            return Ok(None);
        };
        let actor_owner = if actor.starts_with("provider:") || actor == "vic" {
            "vic"
        } else {
            "user"
        };
        if status == "accepted" && actor_owner != to_owner {
            return Err(StoreError::InvalidInput(
                "only the receiving owner can accept a handoff".to_owned(),
            ));
        }
        transaction.execute(
            "UPDATE task_handoffs SET status=?4, completed_at=CASE WHEN ?4 IN ('completed','cancelled') THEN ?5 ELSE NULL END WHERE owner_id=?1 AND task_id=?2 AND handoff_id=?3",
            params![owner_id.trim(), task_id.trim(), handoff_id.trim(), status, now],
        )?;
        if status == "accepted" {
            transaction.execute(
                "UPDATE task_steps SET status='active', updated_at=?4 WHERE owner_id=?1 AND task_id=?2 AND assigned_owner=?3 AND status='pending' AND position=(SELECT MIN(position) FROM task_steps WHERE owner_id=?1 AND task_id=?2 AND assigned_owner=?3 AND status='pending')",
                params![owner_id.trim(), task_id.trim(), to_owner, now],
            )?;
            transaction.execute(
                "UPDATE tasks SET status='active', updated_at=?3 WHERE owner_id=?1 AND task_id=?2 AND status IN ('proposed','ready','blocked')",
                params![owner_id.trim(), task_id.trim(), now],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        let handoff = self
            .task_handoffs(owner_id, task_id)?
            .into_iter()
            .find(|item| item.id == handoff_id)
            .expect("updated handoff");
        self.append_execution_event(
            owner_id,
            task_id,
            "task.handoff.updated",
            actor,
            serde_json::to_value(&handoff)?,
        )?;
        Ok(Some(handoff))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn attach_task_artifact(
        &self,
        owner_id: &str,
        task_id: &str,
        kind: &str,
        uri: &str,
        description: &str,
        created_by: &str,
        actor: &str,
    ) -> Result<TaskArtifactRecord, StoreError> {
        for (label, value) in [
            ("artifact kind", kind),
            ("artifact uri", uri),
            ("artifact description", description),
        ] {
            validate_text(label, value)?;
        }
        validate_party(created_by, false)?;
        self.require_task(owner_id, task_id)?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO task_artifacts(task_artifact_id, owner_id, task_id, kind, uri, description, created_by, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, owner_id.trim(), task_id.trim(), kind.trim(), uri.trim(), description.trim(), created_by, now],
        )?;
        drop(connection);
        let artifact = self
            .task_artifacts(owner_id, task_id)?
            .into_iter()
            .find(|item| item.id == id)
            .expect("inserted artifact");
        self.append_execution_event(
            owner_id,
            task_id,
            "task.artifact.attached",
            actor,
            serde_json::to_value(&artifact)?,
        )?;
        Ok(artifact)
    }

    pub fn record_task_progress(
        &self,
        owner_id: &str,
        task_id: &str,
        summary: &str,
        evidence: Value,
        actor: &str,
    ) -> Result<(), StoreError> {
        validate_text("progress summary", summary)?;
        if !evidence.is_object() {
            return Err(StoreError::InvalidInput(
                "progress evidence must be an object".to_owned(),
            ));
        }
        self.require_task(owner_id, task_id)?;
        self.append_execution_event(
            owner_id,
            task_id,
            "task.progress.recorded",
            actor,
            json!({"summary": summary.trim(), "evidence": evidence}),
        )?;
        Ok(())
    }

    pub fn task_detail(
        &self,
        owner_id: &str,
        task_id: &str,
    ) -> Result<Option<TaskDetail>, StoreError> {
        let Some(task) = self.task(owner_id, task_id)? else {
            return Ok(None);
        };
        let steps = self.task_steps(owner_id, task_id)?;
        let blockers = self.task_blockers(owner_id, task_id)?;
        let handoffs = self.task_handoffs(owner_id, task_id)?;
        let artifacts = self.task_artifacts(owner_id, task_id)?;
        let initiative = self.initiative_job_for_task(owner_id, task_id)?;
        let activity = self.execution_events(owner_id, task_id, 500)?;
        let mut approvals = Vec::new();
        for event in &activity {
            if event.event_type == "task.approval.requested" {
                approvals.push(event.payload.clone());
            }
            if let Some(event_approvals) = event
                .payload
                .get("approvals")
                .and_then(|value| value.as_array())
            {
                approvals.extend(event_approvals.iter().cloned());
            }
        }
        let completed_steps = steps
            .iter()
            .filter(|step| step.status == "completed")
            .count();
        let open_blockers = blockers
            .iter()
            .filter(|blocker| blocker.status == "open")
            .count();
        let pending_handoff = handoffs
            .iter()
            .rev()
            .find(|handoff| handoff.status == "pending");
        let lane = if pending_handoff
            .is_some_and(|handoff| handoff.to_owner == "user" && handoff.kind == "review")
        {
            "review"
        } else if initiative
            .as_ref()
            .is_some_and(|job| matches!(job.status.as_str(), "approved" | "running"))
            || steps
                .iter()
                .any(|step| step.status == "active" && step.owner == "vic")
        {
            "vic_working"
        } else if pending_handoff.is_some_and(|handoff| handoff.to_owner == "user")
            || blockers
                .iter()
                .any(|blocker| blocker.status == "open" && blocker.owner == "user")
            || steps
                .iter()
                .any(|step| step.status != "completed" && step.owner == "user")
        {
            "needs_me"
        } else if steps
            .iter()
            .any(|step| step.status != "completed" && step.owner == "vic")
        {
            "vic_working"
        } else {
            "shared"
        };
        let vic_status = match initiative.as_ref().map(|job| job.status.as_str()) {
            Some("running") => "working",
            Some("approved") | Some("proposed") => "queued",
            Some("completed") => "finished_portion",
            Some("failed") => "failed",
            Some("paused") => "waiting",
            _ => "not_analyzed",
        }
        .to_owned();
        let next_user_action = next_action(&steps, "user").or_else(|| {
            pending_handoff
                .filter(|handoff| handoff.to_owner == "user")
                .map(|handoff| handoff.summary.clone())
        });
        let next_vic_action = next_action(&steps, "vic").or_else(|| {
            pending_handoff
                .filter(|handoff| handoff.to_owner == "vic")
                .map(|handoff| handoff.summary.clone())
        });
        Ok(Some(TaskDetail {
            task,
            initiative,
            progress: TaskProgress {
                completed_steps,
                total_steps: steps.len(),
                open_blockers,
                lane: lane.to_owned(),
                vic_status,
                next_user_action,
                next_vic_action,
            },
            steps,
            blockers,
            handoffs,
            artifacts,
            approvals,
            activity,
        }))
    }

    fn require_task(&self, owner_id: &str, task_id: &str) -> Result<(), StoreError> {
        if self.task(owner_id, task_id)?.is_some() {
            Ok(())
        } else {
            Err(StoreError::InvalidInput(
                "task does not belong to owner".to_owned(),
            ))
        }
    }

    fn task_step(
        &self,
        owner_id: &str,
        task_id: &str,
        step_id: &str,
    ) -> Result<Option<TaskStepRecord>, StoreError> {
        let connection = self.connection()?;
        connection.query_row(
            "SELECT step_id, task_id, title, assigned_owner, status, evidence_json, position, created_at, updated_at FROM task_steps WHERE owner_id=?1 AND task_id=?2 AND step_id=?3",
            params![owner_id, task_id, step_id], step_row,
        ).optional().map_err(StoreError::from)
    }

    fn task_steps(&self, owner_id: &str, task_id: &str) -> Result<Vec<TaskStepRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT step_id, task_id, title, assigned_owner, status, evidence_json, position, created_at, updated_at FROM task_steps WHERE owner_id=?1 AND task_id=?2 ORDER BY position, created_at")?;
        statement
            .query_map(params![owner_id, task_id], step_row)?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    fn task_blockers(
        &self,
        owner_id: &str,
        task_id: &str,
    ) -> Result<Vec<TaskBlockerRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT blocker_id, task_id, description, assigned_owner, status, created_at, resolved_at FROM task_blockers WHERE owner_id=?1 AND task_id=?2 ORDER BY created_at")?;
        statement
            .query_map(params![owner_id, task_id], |row| {
                Ok(TaskBlockerRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    description: row.get(2)?,
                    owner: row.get(3)?,
                    status: row.get(4)?,
                    created_at: row.get(5)?,
                    resolved_at: row.get(6)?,
                })
            })?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    fn task_handoffs(
        &self,
        owner_id: &str,
        task_id: &str,
    ) -> Result<Vec<TaskHandoffRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT handoff_id, task_id, from_owner, to_owner, kind, summary, status, created_at, completed_at FROM task_handoffs WHERE owner_id=?1 AND task_id=?2 ORDER BY created_at")?;
        statement
            .query_map(params![owner_id, task_id], |row| {
                Ok(TaskHandoffRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    from_owner: row.get(2)?,
                    to_owner: row.get(3)?,
                    kind: row.get(4)?,
                    summary: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                    completed_at: row.get(8)?,
                })
            })?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    fn task_artifacts(
        &self,
        owner_id: &str,
        task_id: &str,
    ) -> Result<Vec<TaskArtifactRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT task_artifact_id, task_id, kind, uri, description, created_by, created_at FROM task_artifacts WHERE owner_id=?1 AND task_id=?2 ORDER BY created_at")?;
        statement
            .query_map(params![owner_id, task_id], |row| {
                Ok(TaskArtifactRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    kind: row.get(2)?,
                    uri: row.get(3)?,
                    description: row.get(4)?,
                    created_by: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }
}

fn step_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskStepRecord> {
    let raw: String = row.get(5)?;
    Ok(TaskStepRecord {
        id: row.get(0)?,
        task_id: row.get(1)?,
        title: row.get(2)?,
        owner: row.get(3)?,
        status: row.get(4)?,
        evidence: serde_json::from_str(&raw).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                raw.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        position: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn next_action(steps: &[TaskStepRecord], owner: &str) -> Option<String> {
    steps
        .iter()
        .find(|step| {
            step.owner == owner && matches!(step.status.as_str(), "active" | "pending" | "blocked")
        })
        .map(|step| step.title.clone())
}

fn validate_party(value: &str, shared: bool) -> Result<(), StoreError> {
    if matches!(value, "user" | "vic") || (shared && value == "shared") {
        Ok(())
    } else {
        Err(StoreError::InvalidInput(
            "owner must be user, vic, or shared".to_owned(),
        ))
    }
}

fn validate_step_status(value: &str) -> Result<(), StoreError> {
    if matches!(
        value,
        "pending" | "active" | "blocked" | "completed" | "cancelled"
    ) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput(
            "invalid task step status".to_owned(),
        ))
    }
}

fn validate_text(label: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        Err(StoreError::InvalidInput(format!("{label} is required")))
    } else {
        Ok(())
    }
}
