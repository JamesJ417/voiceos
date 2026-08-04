use chrono::Utc;
use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ArtifactRecord, ConversationStore, StoreError};

impl ConversationStore {
    #[allow(clippy::too_many_arguments)]
    pub fn create_artifact(
        &self,
        owner_id: &str,
        job_id: Option<&str>,
        task_id: Option<&str>,
        parent_artifact_id: Option<&str>,
        kind: &str,
        title: &str,
        filename: &str,
        media_type: &str,
        description: &str,
        created_by: &str,
        metadata: Value,
    ) -> Result<ArtifactRecord, StoreError> {
        for (label, value) in [
            ("owner_id", owner_id),
            ("artifact kind", kind),
            ("artifact title", title),
            ("artifact filename", filename),
            ("artifact media_type", media_type),
            ("artifact created_by", created_by),
        ] {
            if value.trim().is_empty() {
                return Err(StoreError::InvalidInput(format!("{label} is required")));
            }
        }
        if filename.contains(['/', '\\']) || filename == "." || filename == ".." {
            return Err(StoreError::InvalidInput(
                "artifact filename must be a basename".to_owned(),
            ));
        }
        if !metadata.is_object() {
            return Err(StoreError::InvalidInput(
                "artifact metadata must be an object".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1, ?2, ?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id.trim(), now],
        )?;
        for (table, column, value) in [
            ("jobs", "job_id", job_id),
            ("tasks", "task_id", task_id),
            ("artifacts", "artifact_id", parent_artifact_id),
        ] {
            if let Some(value) = value {
                let sql = format!(
                    "SELECT EXISTS(SELECT 1 FROM {table} WHERE {column}=?1 AND owner_id=?2)"
                );
                let exists: bool =
                    connection.query_row(&sql, params![value, owner_id], |row| row.get(0))?;
                if !exists {
                    return Err(StoreError::InvalidInput(format!(
                        "{column} is not owned by owner"
                    )));
                }
            }
        }
        let version = if let Some(parent) = parent_artifact_id {
            connection.query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM artifacts WHERE owner_id=?1 AND (artifact_id=?2 OR parent_artifact_id=?2)",
                params![owner_id, parent], |row| row.get::<_, u32>(0),
            )?
        } else {
            1
        };
        let uri = format!("voiceos://artifacts/{id}");
        connection.execute(
            "INSERT INTO artifacts(artifact_id, owner_id, job_id, task_id, parent_artifact_id, kind, title, filename, media_type, description, status, progress_percent, uri, version, metadata_json, created_by, created_at, updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'queued',0,?11,?12,?13,?14,?15,?15)",
            params![id, owner_id.trim(), job_id, task_id, parent_artifact_id, kind.trim(), title.trim(), filename.trim(), media_type.trim(), description.trim(), uri, version, metadata.to_string(), created_by.trim(), now],
        )?;
        drop(connection);
        let artifact = self.artifact(owner_id, &id)?.expect("inserted artifact");
        self.append_execution_event(
            owner_id,
            &id,
            "artifact.queued",
            created_by,
            json!({"artifact": artifact}),
        )?;
        self.artifact(owner_id, &id)?
            .ok_or_else(|| StoreError::InvalidInput("artifact disappeared".to_owned()))
    }

    pub fn update_artifact_progress(
        &self,
        owner_id: &str,
        artifact_id: &str,
        status: &str,
        progress: u32,
    ) -> Result<ArtifactRecord, StoreError> {
        if !matches!(
            status,
            "queued" | "generating" | "validating" | "ready" | "failed"
        ) {
            return Err(StoreError::InvalidInput(
                "invalid artifact status".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE artifacts SET status=?3, progress_percent=?4, updated_at=?5 WHERE owner_id=?1 AND artifact_id=?2",
            params![owner_id, artifact_id, status, progress.min(100), now],
        )?;
        drop(connection);
        if changed == 0 {
            return Err(StoreError::InvalidInput("artifact not found".to_owned()));
        }
        let artifact = self
            .artifact(owner_id, artifact_id)?
            .expect("updated artifact");
        self.append_execution_event(
            owner_id,
            artifact_id,
            "artifact.progress",
            "pdf-worker",
            json!({"artifact": artifact}),
        )?;
        self.artifact(owner_id, artifact_id)?
            .ok_or_else(|| StoreError::InvalidInput("artifact disappeared".to_owned()))
    }

    pub fn complete_artifact(
        &self,
        owner_id: &str,
        artifact_id: &str,
        storage_key: &str,
        sha256: &str,
        byte_size: u64,
    ) -> Result<ArtifactRecord, StoreError> {
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE artifacts SET status='ready', progress_percent=100, storage_key=?3, sha256=?4, byte_size=?5, error=NULL, updated_at=?6, completed_at=?6 WHERE owner_id=?1 AND artifact_id=?2",
            params![owner_id, artifact_id, storage_key, sha256, byte_size, now],
        )?;
        drop(connection);
        if changed == 0 {
            return Err(StoreError::InvalidInput("artifact not found".to_owned()));
        }
        let artifact = self
            .artifact(owner_id, artifact_id)?
            .expect("completed artifact");
        self.append_execution_event(
            owner_id,
            artifact_id,
            "artifact.ready",
            "pdf-worker",
            json!({"artifact": artifact}),
        )?;
        self.artifact(owner_id, artifact_id)?
            .ok_or_else(|| StoreError::InvalidInput("artifact disappeared".to_owned()))
    }

    pub fn fail_artifact(
        &self,
        owner_id: &str,
        artifact_id: &str,
        error: &str,
    ) -> Result<ArtifactRecord, StoreError> {
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        connection.execute(
            "UPDATE artifacts SET status='failed', error=?3, updated_at=?4 WHERE owner_id=?1 AND artifact_id=?2",
            params![owner_id, artifact_id, error, now],
        )?;
        drop(connection);
        let artifact = self
            .artifact(owner_id, artifact_id)?
            .ok_or_else(|| StoreError::InvalidInput("artifact not found".to_owned()))?;
        self.append_execution_event(
            owner_id,
            artifact_id,
            "artifact.failed",
            "pdf-worker",
            json!({"artifact": artifact}),
        )?;
        self.artifact(owner_id, artifact_id)?
            .ok_or_else(|| StoreError::InvalidInput("artifact disappeared".to_owned()))
    }

    pub fn artifact(
        &self,
        owner_id: &str,
        artifact_id: &str,
    ) -> Result<Option<ArtifactRecord>, StoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!(
                    "SELECT {} FROM artifacts WHERE owner_id=?1 AND artifact_id=?2",
                    artifact_columns()
                ),
                params![owner_id, artifact_id],
                artifact_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn list_artifacts(
        &self,
        owner_id: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ArtifactRecord>, StoreError> {
        let connection = self.connection()?;
        let needle = format!("%{}%", query.unwrap_or("").trim());
        let sql = format!(
            "SELECT {} FROM artifacts WHERE owner_id=?1 AND (?2='%%' OR title LIKE ?2 OR filename LIKE ?2 OR description LIKE ?2) ORDER BY created_at DESC LIMIT ?3",
            artifact_columns()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![owner_id, needle, limit.clamp(1, 500)],
            artifact_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn artifact_events_after(
        &self,
        owner_id: &str,
        after: i64,
        limit: usize,
    ) -> Result<Vec<crate::ExecutionEvent>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT event_id, owner_id, stream_id, event_type, actor, payload_json, occurred_at FROM execution_events WHERE owner_id=?1 AND event_id>?2 AND event_type LIKE 'artifact.%' ORDER BY event_id LIMIT ?3")?;
        let rows = statement.query_map(params![owner_id, after, limit.clamp(1, 500)], |row| {
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
            Ok(crate::ExecutionEvent {
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
}

fn artifact_columns() -> &'static str {
    "artifact_id, owner_id, job_id, task_id, parent_artifact_id, kind, COALESCE(NULLIF(title,''),kind), COALESCE(NULLIF(filename,''), artifact_id || '.bin'), media_type, description, status, progress_percent, storage_key, uri, sha256, byte_size, version, metadata_json, error, created_by, created_at, COALESCE(NULLIF(updated_at,''),created_at), completed_at"
}

fn artifact_from_row(row: &Row<'_>) -> rusqlite::Result<ArtifactRecord> {
    let metadata: String = row.get(17)?;
    Ok(ArtifactRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        job_id: row.get(2)?,
        task_id: row.get(3)?,
        parent_artifact_id: row.get(4)?,
        kind: row.get(5)?,
        title: row.get(6)?,
        filename: row.get(7)?,
        media_type: row.get(8)?,
        description: row.get(9)?,
        status: row.get(10)?,
        progress_percent: row.get(11)?,
        storage_key: row.get(12)?,
        uri: row.get(13)?,
        sha256: row.get(14)?,
        byte_size: row.get(15)?,
        version: row.get(16)?,
        metadata: serde_json::from_str(&metadata).unwrap_or_else(|_| json!({})),
        error: row.get(18)?,
        created_by: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
        completed_at: row.get(22)?,
    })
}
