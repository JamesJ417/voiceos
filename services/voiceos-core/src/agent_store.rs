use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    ArtifactRecord, AutomationProposal, ConversationStore, ExecutionEvent, GoalRecord, JobRecord,
    ProjectRecord, ProviderRunMetric, SkillProposal, SkillUsage, StoreError, TaskRecord,
};

impl ConversationStore {
    pub fn create_goal(
        &self,
        owner_id: &str,
        title: &str,
        desired_outcome: &str,
    ) -> Result<GoalRecord, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("goal title", title)?;
        require_text("desired outcome", desired_outcome)?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        connection.execute(
            "INSERT INTO goals(goal_id, owner_id, title, desired_outcome, status, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, 'active', ?5, ?5)",
            params![id, owner_id.trim(), title.trim(), desired_outcome.trim(), now],
        )?;
        Ok(GoalRecord {
            id,
            owner_id: owner_id.trim().to_owned(),
            title: title.trim().to_owned(),
            desired_outcome: desired_outcome.trim().to_owned(),
            status: "active".to_owned(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn create_project(
        &self,
        owner_id: &str,
        goal_id: Option<&str>,
        title: &str,
    ) -> Result<ProjectRecord, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("project title", title)?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        if let Some(goal_id) = goal_id {
            require_owned(&connection, "goals", "goal_id", goal_id, owner_id)?;
        }
        connection.execute(
            "INSERT INTO projects(project_id, owner_id, goal_id, title, status, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, 'active', ?5, ?5)",
            params![id, owner_id.trim(), goal_id, title.trim(), now],
        )?;
        Ok(ProjectRecord {
            id,
            owner_id: owner_id.trim().to_owned(),
            goal_id: goal_id.map(str::to_owned),
            title: title.trim().to_owned(),
            status: "active".to_owned(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn create_task(
        &self,
        owner_id: &str,
        project_id: Option<&str>,
        parent_task_id: Option<&str>,
        title: &str,
        observable_outcome: &str,
        estimated_minutes: u32,
    ) -> Result<TaskRecord, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("task title", title)?;
        require_text("observable outcome", observable_outcome)?;
        if !(1..=1_440).contains(&estimated_minutes) {
            return Err(StoreError::InvalidInput(
                "estimated_minutes must be between 1 and 1440".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        if let Some(project_id) = project_id {
            require_owned(&connection, "projects", "project_id", project_id, owner_id)?;
        }
        if let Some(parent_task_id) = parent_task_id {
            require_owned(&connection, "tasks", "task_id", parent_task_id, owner_id)?;
        }
        connection.execute(
            "INSERT INTO tasks(task_id, owner_id, project_id, parent_task_id, title, observable_outcome, estimated_minutes, status, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ready', ?8, ?8)",
            params![id, owner_id.trim(), project_id, parent_task_id, title.trim(), observable_outcome.trim(), estimated_minutes, now],
        )?;
        Ok(TaskRecord {
            id,
            owner_id: owner_id.trim().to_owned(),
            project_id: project_id.map(str::to_owned),
            parent_task_id: parent_task_id.map(str::to_owned),
            title: title.trim().to_owned(),
            observable_outcome: observable_outcome.trim().to_owned(),
            estimated_minutes,
            status: "ready".to_owned(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn tasks(
        &self,
        owner_id: &str,
        include_completed: bool,
        limit: usize,
    ) -> Result<Vec<TaskRecord>, StoreError> {
        require_text("owner_id", owner_id)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task_id, owner_id, project_id, parent_task_id, title, observable_outcome, estimated_minutes, status, created_at, updated_at
             FROM tasks
             WHERE owner_id=?1 AND (?2 OR status NOT IN ('completed', 'cancelled'))
             ORDER BY CASE status WHEN 'active' THEN 0 WHEN 'ready' THEN 1 WHEN 'blocked' THEN 2 WHEN 'proposed' THEN 3 ELSE 4 END,
                      updated_at DESC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![owner_id.trim(), include_completed, limit.clamp(1, 200)],
            task_row,
        )?;
        rows.map(|row| row.map_err(StoreError::from)).collect()
    }

    pub fn task(&self, owner_id: &str, task_id: &str) -> Result<Option<TaskRecord>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("task_id", task_id)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task_id, owner_id, project_id, parent_task_id, title, observable_outcome, estimated_minutes, status, created_at, updated_at
             FROM tasks WHERE owner_id=?1 AND task_id=?2",
        )?;
        let mut rows = statement.query(params![owner_id.trim(), task_id.trim()])?;
        rows.next()?
            .map(task_row)
            .transpose()
            .map_err(StoreError::from)
    }

    pub fn update_task_status_as(
        &self,
        owner_id: &str,
        task_id: &str,
        status: &str,
        actor: &str,
    ) -> Result<Option<TaskRecord>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("task_id", task_id)?;
        require_text("actor", actor)?;
        validate_task_status(status)?;
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let previous_status: Option<String> = transaction
            .query_row(
                "SELECT status FROM tasks WHERE owner_id=?1 AND task_id=?2",
                params![owner_id.trim(), task_id.trim()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(previous_status) = previous_status else {
            return Ok(None);
        };
        transaction.execute(
            "UPDATE tasks SET status=?3, updated_at=?4 WHERE owner_id=?1 AND task_id=?2",
            params![owner_id.trim(), task_id.trim(), status, now],
        )?;
        transaction.execute(
            "INSERT INTO execution_events(owner_id, stream_id, event_type, actor, payload_json, occurred_at)
             VALUES(?1, ?2, 'task.status_changed', ?3, ?4, ?5)",
            params![
                owner_id.trim(),
                task_id.trim(),
                actor.trim(),
                serde_json::json!({"from": previous_status, "to": status}).to_string(),
                now,
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.task(owner_id, task_id)
    }

    pub fn create_job(
        &self,
        owner_id: &str,
        task_id: Option<&str>,
        idempotency_key: &str,
        capability_scope: Value,
    ) -> Result<JobRecord, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("idempotency key", idempotency_key)?;
        if !capability_scope.is_array() {
            return Err(StoreError::InvalidInput(
                "capability_scope must be a JSON array".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        if let Some(task_id) = task_id {
            require_owned(&connection, "tasks", "task_id", task_id, owner_id)?;
        }
        connection.execute(
            "INSERT INTO jobs(job_id, owner_id, task_id, status, idempotency_key, capability_scope_json, created_at, updated_at) VALUES(?1, ?2, ?3, 'proposed', ?4, ?5, ?6, ?6)",
            params![id, owner_id.trim(), task_id, idempotency_key.trim(), capability_scope.to_string(), now],
        )?;
        Ok(JobRecord {
            id,
            owner_id: owner_id.trim().to_owned(),
            task_id: task_id.map(str::to_owned),
            status: "proposed".to_owned(),
            idempotency_key: idempotency_key.trim().to_owned(),
            capability_scope,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn job(&self, owner_id: &str, job_id: &str) -> Result<Option<JobRecord>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("job_id", job_id)?;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT job_id, owner_id, task_id, status, idempotency_key, capability_scope_json, created_at, updated_at FROM jobs WHERE owner_id=?1 AND job_id=?2",
                params![owner_id.trim(), job_id.trim()],
                job_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn initiative_job_for_task(
        &self,
        owner_id: &str,
        task_id: &str,
    ) -> Result<Option<JobRecord>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("task_id", task_id)?;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT job_id, owner_id, task_id, status, idempotency_key, capability_scope_json, created_at, updated_at FROM jobs WHERE owner_id=?1 AND task_id=?2 AND idempotency_key=?3 ORDER BY created_at DESC LIMIT 1",
                params![owner_id.trim(), task_id.trim(), format!("task-initiative:{}", task_id.trim())],
                job_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn transition_job_status(
        &self,
        owner_id: &str,
        job_id: &str,
        expected_status: &str,
        next_status: &str,
    ) -> Result<Option<JobRecord>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("job_id", job_id)?;
        validate_job_status(expected_status)?;
        validate_job_status(next_status)?;
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE jobs SET status=?4, updated_at=?5 WHERE owner_id=?1 AND job_id=?2 AND status=?3",
            params![owner_id.trim(), job_id.trim(), expected_status, next_status, now],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        drop(connection);
        self.job(owner_id, job_id)
    }

    pub fn append_execution_event(
        &self,
        owner_id: &str,
        stream_id: &str,
        event_type: &str,
        actor: &str,
        payload: Value,
    ) -> Result<ExecutionEvent, StoreError> {
        for (label, value) in [
            ("owner_id", owner_id),
            ("stream_id", stream_id),
            ("event_type", event_type),
            ("actor", actor),
        ] {
            require_text(label, value)?;
        }
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        connection.execute(
            "INSERT INTO execution_events(owner_id, stream_id, event_type, actor, payload_json, occurred_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![owner_id.trim(), stream_id.trim(), event_type.trim(), actor.trim(), payload.to_string(), now],
        )?;
        Ok(ExecutionEvent {
            id: connection.last_insert_rowid(),
            owner_id: owner_id.trim().to_owned(),
            stream_id: stream_id.trim().to_owned(),
            event_type: event_type.trim().to_owned(),
            actor: actor.trim().to_owned(),
            payload,
            occurred_at: now,
        })
    }

    pub fn execution_events(
        &self,
        owner_id: &str,
        stream_id: &str,
        limit: usize,
    ) -> Result<Vec<ExecutionEvent>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT event_id, owner_id, stream_id, event_type, actor, payload_json, occurred_at FROM execution_events WHERE owner_id=?1 AND stream_id=?2 ORDER BY event_id LIMIT ?3",
        )?;
        let rows =
            statement.query_map(params![owner_id, stream_id, limit.clamp(1, 1_000)], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?;
        rows.map(|row| {
            let row = row?;
            Ok(ExecutionEvent {
                id: row.0,
                owner_id: row.1,
                stream_id: row.2,
                event_type: row.3,
                actor: row.4,
                payload: serde_json::from_str(&row.5)?,
                occurred_at: row.6,
            })
        })
        .collect()
    }

    pub fn propose_skill(
        &self,
        owner_id: &str,
        name: &str,
        content: &str,
        required_capabilities: Value,
        evidence: Value,
    ) -> Result<SkillProposal, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("skill name", name)?;
        require_text("skill content", content)?;
        if !required_capabilities.is_array() || !evidence.is_array() {
            return Err(StoreError::InvalidInput(
                "skill capabilities and evidence must be JSON arrays".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        let evidence_sha256 = evidence_fingerprint(&evidence);
        let version: u32 = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM skills WHERE owner_id=?1 AND name=?2",
            params![owner_id.trim(), name.trim()],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO skills(skill_id, owner_id, name, version, status, content, required_capabilities_json, evidence_json, evidence_sha256, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, 'proposed', ?5, ?6, ?7, ?8, ?9, ?9)",
            params![id, owner_id.trim(), name.trim(), version, content.trim(), required_capabilities.to_string(), evidence.to_string(), evidence_sha256, now],
        )?;
        Ok(SkillProposal {
            id,
            owner_id: owner_id.trim().to_owned(),
            name: name.trim().to_owned(),
            version,
            status: "proposed".to_owned(),
            content: content.trim().to_owned(),
            required_capabilities,
            evidence,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub(crate) fn has_skill_evidence(
        &self,
        owner_id: &str,
        name: &str,
        evidence: &Value,
    ) -> Result<bool, StoreError> {
        let fingerprint = evidence_fingerprint(evidence);
        self.connection()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM skills WHERE owner_id=?1 AND name=?2 AND evidence_sha256=?3)",
                params![owner_id, name, fingerprint],
                |row| row.get(0),
            )
            .map_err(StoreError::from)
    }

    pub fn skill_proposals(
        &self,
        owner_id: &str,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SkillProposal>, StoreError> {
        require_text("owner_id", owner_id)?;
        if let Some(status) = status {
            validate_skill_status(status)?;
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT skill_id, owner_id, name, version, status, content, required_capabilities_json, evidence_json, created_at, updated_at FROM skills WHERE owner_id=?1 AND (?2 IS NULL OR status=?2) ORDER BY updated_at DESC, version DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![owner_id.trim(), status, limit.clamp(1, 200)],
            skill_row,
        )?;
        rows.map(|row| row.map_err(StoreError::from)?.into_proposal())
            .collect()
    }

    pub fn skill_proposal(
        &self,
        owner_id: &str,
        skill_id: &str,
    ) -> Result<Option<SkillProposal>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("skill_id", skill_id)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT skill_id, owner_id, name, version, status, content, required_capabilities_json, evidence_json, created_at, updated_at FROM skills WHERE owner_id=?1 AND skill_id=?2",
        )?;
        let mut rows = statement.query(params![owner_id.trim(), skill_id.trim()])?;
        rows.next()?
            .map(skill_row)
            .transpose()?
            .map(SkillRow::into_proposal)
            .transpose()
    }

    pub fn decide_skill_proposal(
        &self,
        owner_id: &str,
        skill_id: &str,
        approve: bool,
    ) -> Result<bool, StoreError> {
        Ok(self
            .decide_skill_proposal_as(owner_id, skill_id, approve, "voiceos-core")?
            .is_some())
    }

    pub fn decide_skill_proposal_as(
        &self,
        owner_id: &str,
        skill_id: &str,
        approve: bool,
        actor: &str,
    ) -> Result<Option<SkillProposal>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("skill_id", skill_id)?;
        require_text("actor", actor)?;
        let now = Utc::now().to_rfc3339();
        let decision = if approve { "approved" } else { "rejected" };
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE skills SET status=?3, updated_at=?4 WHERE owner_id=?1 AND skill_id=?2 AND status='proposed'",
            params![owner_id.trim(), skill_id.trim(), decision, now],
        )?;
        if changed == 1 {
            transaction.execute(
                "INSERT INTO execution_events(owner_id, stream_id, event_type, actor, payload_json, occurred_at) VALUES(?1, ?2, 'skill.decided', ?3, ?4, ?5)",
                params![
                    owner_id.trim(),
                    skill_id.trim(),
                    actor.trim(),
                    serde_json::json!({"decision": decision, "execution_enabled": false}).to_string(),
                    now,
                ],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        if changed == 1 {
            self.skill_proposal(owner_id, skill_id)
        } else {
            Ok(None)
        }
    }

    pub fn set_skill_status_as(
        &self,
        owner_id: &str,
        skill_id: &str,
        status: &str,
        actor: &str,
    ) -> Result<Option<SkillProposal>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("skill_id", skill_id)?;
        require_text("actor", actor)?;
        if status != "disabled" {
            return Err(StoreError::InvalidInput(
                "skill lifecycle status must be disabled".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE skills SET status=?3, updated_at=?4 WHERE owner_id=?1 AND skill_id=?2 AND status='approved'",
            params![owner_id.trim(), skill_id.trim(), status, now],
        )?;
        if changed == 1 {
            transaction.execute(
                "INSERT INTO execution_events(owner_id, stream_id, event_type, actor, payload_json, occurred_at) VALUES(?1, ?2, 'skill.status.changed', ?3, ?4, ?5)",
                params![owner_id.trim(), skill_id.trim(), actor.trim(), serde_json::json!({"status": status}).to_string(), now],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        if changed == 1 {
            self.skill_proposal(owner_id, skill_id)
        } else {
            Ok(None)
        }
    }

    pub fn record_matching_skill_usages(
        &self,
        owner_id: &str,
        conversation_id: Option<&str>,
        request_id: Option<&str>,
        tool_calls: &Value,
        result: &Value,
        outcome: &str,
    ) -> Result<Vec<SkillUsage>, StoreError> {
        require_text("owner_id", owner_id)?;
        if !matches!(outcome, "completed" | "failed") {
            return Err(StoreError::InvalidInput(
                "invalid skill usage outcome".to_owned(),
            ));
        }
        let names = tool_calls
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|call| call.get("name").and_then(Value::as_str))
            .collect::<std::collections::HashSet<_>>();
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let approved = {
            let mut statement = connection.prepare(
                "SELECT skill_id, name, version, required_capabilities_json
                 FROM skills s
                 WHERE owner_id=?1 AND status='approved'
                   AND version=(SELECT MAX(version) FROM skills newest WHERE newest.owner_id=s.owner_id AND newest.name=s.name AND newest.status='approved')",
            )?;
            let rows = statement.query_map([owner_id.trim()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let transaction = connection.transaction()?;
        let mut recorded = Vec::new();
        for (skill_id, skill_name, skill_version, capabilities_json) in approved {
            let capabilities: Value = serde_json::from_str(&capabilities_json)?;
            let matched = capabilities.as_array().is_some_and(|items| {
                !items.is_empty()
                    && items
                        .iter()
                        .all(|item| item.as_str().is_some_and(|name| names.contains(name)))
            });
            if !matched {
                continue;
            }
            let usage_id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO skill_usages(usage_id,owner_id,skill_id,conversation_id,request_id,tool_calls_json,result_json,outcome,used_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![usage_id, owner_id.trim(), skill_id, conversation_id, request_id, tool_calls.to_string(), result.to_string(), outcome, now],
            )?;
            transaction.execute(
                "INSERT INTO execution_events(owner_id, stream_id, event_type, actor, payload_json, occurred_at) VALUES(?1, ?2, 'skill.used', 'vic', ?3, ?4)",
                params![owner_id.trim(), skill_id, serde_json::json!({"usage_id": usage_id, "skill_name": skill_name, "skill_version": skill_version, "outcome": outcome}).to_string(), now],
            )?;
            recorded.push(SkillUsage {
                id: usage_id,
                owner_id: owner_id.trim().to_owned(),
                skill_id,
                skill_name,
                skill_version,
                conversation_id: conversation_id.map(str::to_owned),
                request_id: request_id.map(str::to_owned),
                tool_calls: tool_calls.clone(),
                result: result.clone(),
                outcome: outcome.to_owned(),
                feedback: None,
                feedback_note: None,
                used_at: now.clone(),
                reviewed_at: None,
                reviewed_by: None,
            });
        }
        transaction.commit()?;
        Ok(recorded)
    }

    pub fn skill_usages(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<SkillUsage>, StoreError> {
        require_text("owner_id", owner_id)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT u.usage_id,u.owner_id,u.skill_id,s.name,s.version,u.conversation_id,u.request_id,u.tool_calls_json,u.result_json,u.outcome,u.feedback,u.feedback_note,u.used_at,u.reviewed_at,u.reviewed_by
             FROM skill_usages u JOIN skills s ON s.skill_id=u.skill_id
             WHERE u.owner_id=?1 ORDER BY u.used_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![owner_id.trim(), limit.clamp(1, 200)],
            skill_usage_row,
        )?;
        rows.map(|row| row.map_err(StoreError::from)).collect()
    }

    pub fn review_skill_usage_as(
        &self,
        owner_id: &str,
        usage_id: &str,
        feedback: &str,
        note: Option<&str>,
        actor: &str,
    ) -> Result<Option<SkillUsage>, StoreError> {
        require_text("owner_id", owner_id)?;
        require_text("usage_id", usage_id)?;
        require_text("actor", actor)?;
        if !matches!(feedback, "correct" | "incorrect") {
            return Err(StoreError::InvalidInput(
                "feedback must be correct or incorrect".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let skill_id: Option<String> = transaction
            .query_row(
                "SELECT skill_id FROM skill_usages WHERE owner_id=?1 AND usage_id=?2",
                params![owner_id.trim(), usage_id.trim()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(skill_id) = skill_id else {
            return Ok(None);
        };
        transaction.execute(
            "UPDATE skill_usages SET feedback=?3,feedback_note=?4,reviewed_at=?5,reviewed_by=?6 WHERE owner_id=?1 AND usage_id=?2",
            params![owner_id.trim(), usage_id.trim(), feedback, note.map(str::trim).filter(|value| !value.is_empty()), now, actor.trim()],
        )?;
        transaction.execute(
            "INSERT INTO execution_events(owner_id, stream_id, event_type, actor, payload_json, occurred_at) VALUES(?1, ?2, 'skill.feedback.recorded', ?3, ?4, ?5)",
            params![owner_id.trim(), skill_id, actor.trim(), serde_json::json!({"usage_id": usage_id.trim(), "feedback": feedback, "note": note}).to_string(), now],
        )?;
        transaction.commit()?;
        drop(connection);
        Ok(self
            .skill_usages(owner_id, 200)?
            .into_iter()
            .find(|usage| usage.id == usage_id))
    }

    pub fn create_automation_proposal(
        &self,
        owner_id: &str,
        skill_id: &str,
        trigger: Value,
    ) -> Result<AutomationProposal, StoreError> {
        if !trigger.is_object() {
            return Err(StoreError::InvalidInput(
                "automation trigger must be a JSON object".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        let approved: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM skills WHERE skill_id=?1 AND owner_id=?2 AND status='approved')",
            params![skill_id, owner_id],
            |row| row.get(0),
        )?;
        if !approved {
            return Err(StoreError::InvalidInput(
                "automations may only be proposed for approved skills".to_owned(),
            ));
        }
        connection.execute(
            "INSERT INTO automation_proposals(automation_id, owner_id, skill_id, status, trigger_json, created_at, updated_at) VALUES(?1, ?2, ?3, 'proposed', ?4, ?5, ?5)",
            params![id, owner_id, skill_id, trigger.to_string(), now],
        )?;
        Ok(AutomationProposal {
            id,
            owner_id: owner_id.to_owned(),
            skill_id: skill_id.to_owned(),
            status: "proposed".to_owned(),
            trigger,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn record_artifact(
        &self,
        owner_id: &str,
        job_id: Option<&str>,
        kind: &str,
        uri: &str,
        sha256: Option<&str>,
    ) -> Result<ArtifactRecord, StoreError> {
        require_text("artifact kind", kind)?;
        require_text("artifact uri", uri)?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        if let Some(job_id) = job_id {
            require_owned(&connection, "jobs", "job_id", job_id, owner_id)?;
        }
        connection.execute(
            "INSERT INTO artifacts(artifact_id, owner_id, job_id, kind, uri, sha256, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, owner_id, job_id, kind.trim(), uri.trim(), sha256, now],
        )?;
        Ok(ArtifactRecord {
            id,
            owner_id: owner_id.to_owned(),
            job_id: job_id.map(str::to_owned),
            kind: kind.trim().to_owned(),
            uri: uri.trim().to_owned(),
            sha256: sha256.map(str::to_owned),
            created_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_provider_run(
        &self,
        owner_id: &str,
        job_id: Option<&str>,
        provider: &str,
        model: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        duration_ms: u64,
        cost_usd: Option<f64>,
        status: &str,
    ) -> Result<ProviderRunMetric, StoreError> {
        require_text("provider", provider)?;
        require_text("model", model)?;
        if duration_ms == 0 {
            return Err(StoreError::InvalidInput(
                "provider duration_ms must be greater than zero".to_owned(),
            ));
        }
        if !matches!(status, "completed" | "failed" | "cancelled") {
            return Err(StoreError::InvalidInput(
                "invalid provider run status".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        ensure_owner(&connection, owner_id, &now)?;
        if let Some(job_id) = job_id {
            require_owned(&connection, "jobs", "job_id", job_id, owner_id)?;
        }
        let output_tokens_per_second =
            output_tokens.map(|tokens| tokens as f64 * 1_000.0 / duration_ms as f64);
        connection.execute(
            "INSERT INTO provider_runs(owner_id, job_id, provider, model, input_tokens, output_tokens, duration_ms, output_tokens_per_second, cost_usd, status, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![owner_id, job_id, provider.trim(), model.trim(), input_tokens, output_tokens, duration_ms, output_tokens_per_second, cost_usd, status, now],
        )?;
        let metric = ProviderRunMetric {
            id: connection.last_insert_rowid(),
            owner_id: owner_id.to_owned(),
            job_id: job_id.map(str::to_owned),
            provider: provider.trim().to_owned(),
            model: model.trim().to_owned(),
            input_tokens,
            output_tokens,
            duration_ms,
            output_tokens_per_second,
            cost_usd,
            status: status.to_owned(),
            created_at: now,
        };
        drop(connection);
        self.append_execution_event(
            owner_id,
            job_id.unwrap_or("provider-telemetry"),
            "provider.run.recorded",
            "voiceos-core",
            serde_json::to_value(&metric)?,
        )?;
        Ok(metric)
    }
}

fn ensure_owner(connection: &Connection, owner_id: &str, now: &str) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1, ?2, ?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
        params![owner_id.trim(), now],
    )?;
    Ok(())
}

fn require_owned(
    connection: &Connection,
    table: &str,
    id_column: &str,
    id: &str,
    owner_id: &str,
) -> Result<(), StoreError> {
    let sql = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {id_column}=?1 AND owner_id=?2)");
    let exists: bool = connection.query_row(&sql, params![id, owner_id], |row| row.get(0))?;
    if exists {
        Ok(())
    } else {
        Err(StoreError::InvalidInput(format!(
            "{id_column} does not belong to owner"
        )))
    }
}

fn require_text(label: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        Err(StoreError::InvalidInput(format!("{label} is required")))
    } else {
        Ok(())
    }
}

fn evidence_fingerprint(evidence: &Value) -> String {
    format!("{:x}", Sha256::digest(evidence.to_string().as_bytes()))
}

fn validate_skill_status(status: &str) -> Result<(), StoreError> {
    if matches!(status, "proposed" | "approved" | "rejected" | "disabled") {
        Ok(())
    } else {
        Err(StoreError::InvalidInput("invalid skill status".to_owned()))
    }
}

fn validate_task_status(status: &str) -> Result<(), StoreError> {
    if matches!(
        status,
        "proposed" | "ready" | "active" | "blocked" | "completed" | "cancelled"
    ) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput("invalid task status".to_owned()))
    }
}

fn validate_job_status(status: &str) -> Result<(), StoreError> {
    if matches!(
        status,
        "proposed" | "approved" | "running" | "paused" | "completed" | "failed" | "cancelled"
    ) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput("invalid job status".to_owned()))
    }
}

fn job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRecord> {
    let capability_scope: String = row.get(5)?;
    Ok(JobRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        task_id: row.get(2)?,
        status: row.get(3)?,
        idempotency_key: row.get(4)?,
        capability_scope: serde_json::from_str(&capability_scope).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                capability_scope.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        project_id: row.get(2)?,
        parent_task_id: row.get(3)?,
        title: row.get(4)?,
        observable_outcome: row.get(5)?,
        estimated_minutes: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

struct SkillRow {
    id: String,
    owner_id: String,
    name: String,
    version: u32,
    status: String,
    content: String,
    required_capabilities_json: String,
    evidence_json: String,
    created_at: String,
    updated_at: String,
}

impl SkillRow {
    fn into_proposal(self) -> Result<SkillProposal, StoreError> {
        Ok(SkillProposal {
            id: self.id,
            owner_id: self.owner_id,
            name: self.name,
            version: self.version,
            status: self.status,
            content: self.content,
            required_capabilities: serde_json::from_str(&self.required_capabilities_json)?,
            evidence: serde_json::from_str(&self.evidence_json)?,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn skill_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillRow> {
    Ok(SkillRow {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        name: row.get(2)?,
        version: row.get(3)?,
        status: row.get(4)?,
        content: row.get(5)?,
        required_capabilities_json: row.get(6)?,
        evidence_json: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn skill_usage_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillUsage> {
    let tool_calls_json: String = row.get(7)?;
    let result_json: String = row.get(8)?;
    let decode = |value: &str| {
        serde_json::from_str(value).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                value.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    };
    Ok(SkillUsage {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        skill_id: row.get(2)?,
        skill_name: row.get(3)?,
        skill_version: row.get(4)?,
        conversation_id: row.get(5)?,
        request_id: row.get(6)?,
        tool_calls: decode(&tool_calls_json)?,
        result: decode(&result_json)?,
        outcome: row.get(9)?,
        feedback: row.get(10)?,
        feedback_note: row.get(11)?,
        used_at: row.get(12)?,
        reviewed_at: row.get(13)?,
        reviewed_by: row.get(14)?,
    })
}
