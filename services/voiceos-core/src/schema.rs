use rusqlite::Connection;

pub(crate) fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS devices (
            device_id TEXT PRIMARY KEY,
            display_name TEXT,
            created_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS conversations (
            conversation_id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('active', 'archived')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(device_id) REFERENCES devices(device_id)
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_active_conversation_per_device
            ON conversations(device_id) WHERE status = 'active';
        CREATE TABLE IF NOT EXISTS conversation_aliases (
            device_id TEXT NOT NULL,
            client_session_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            first_seen_at TEXT NOT NULL,
            PRIMARY KEY(device_id, client_session_id),
            FOREIGN KEY(conversation_id) REFERENCES conversations(conversation_id)
        );
        CREATE TABLE IF NOT EXISTS messages (
            message_id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            provider TEXT,
            legacy_turn_id INTEGER,
            created_at TEXT NOT NULL,
            FOREIGN KEY(conversation_id) REFERENCES conversations(conversation_id)
        );
        CREATE INDEX IF NOT EXISTS messages_conversation_idx
            ON messages(conversation_id, message_id);
        CREATE TABLE IF NOT EXISTS conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            through_message_id INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(conversation_id) REFERENCES conversations(conversation_id)
        );
        CREATE TABLE IF NOT EXISTS memories (
            memory_id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            normalized_content TEXT NOT NULL,
            content TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(device_id, normalized_content),
            FOREIGN KEY(device_id) REFERENCES devices(device_id)
        );
        CREATE TABLE IF NOT EXISTS legacy_imports (
            source_path TEXT NOT NULL,
            legacy_turn_id INTEGER NOT NULL,
            imported_at TEXT NOT NULL,
            PRIMARY KEY(source_path, legacy_turn_id)
        );
        CREATE TABLE IF NOT EXISTS documents (
            document_id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            filename TEXT NOT NULL,
            media_type TEXT NOT NULL,
            mode TEXT NOT NULL CHECK(mode IN ('profile', 'reference')),
            byte_size INTEGER NOT NULL,
            sha256 TEXT NOT NULL,
            source_bytes BLOB NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(device_id) REFERENCES devices(device_id),
            UNIQUE(device_id, sha256, mode)
        );
        CREATE TABLE IF NOT EXISTS document_chunks (
            chunk_id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            content TEXT NOT NULL,
            FOREIGN KEY(document_id) REFERENCES documents(document_id) ON DELETE CASCADE,
            UNIQUE(document_id, ordinal)
        );
        CREATE INDEX IF NOT EXISTS documents_device_idx ON documents(device_id, created_at);
        CREATE INDEX IF NOT EXISTS document_chunks_document_idx ON document_chunks(document_id, ordinal);

        CREATE TABLE IF NOT EXISTS owners (
            owner_id TEXT PRIMARY KEY,
            display_name TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS goals (
            goal_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            title TEXT NOT NULL,
            desired_outcome TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('active', 'completed', 'cancelled', 'archived')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
        CREATE TABLE IF NOT EXISTS projects (
            project_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            goal_id TEXT,
            title TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('active', 'completed', 'cancelled', 'archived')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(goal_id) REFERENCES goals(goal_id)
        );
        CREATE TABLE IF NOT EXISTS tasks (
            task_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            project_id TEXT,
            parent_task_id TEXT,
            title TEXT NOT NULL,
            observable_outcome TEXT NOT NULL,
            estimated_minutes INTEGER NOT NULL CHECK(estimated_minutes BETWEEN 1 AND 1440),
            status TEXT NOT NULL CHECK(status IN ('proposed', 'ready', 'active', 'blocked', 'completed', 'cancelled')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(project_id) REFERENCES projects(project_id),
            FOREIGN KEY(parent_task_id) REFERENCES tasks(task_id)
        );
        CREATE INDEX IF NOT EXISTS tasks_owner_status_idx ON tasks(owner_id, status, updated_at);
        CREATE TABLE IF NOT EXISTS task_steps (
            step_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            title TEXT NOT NULL,
            assigned_owner TEXT NOT NULL CHECK(assigned_owner IN ('user', 'vic', 'shared')),
            status TEXT NOT NULL CHECK(status IN ('pending', 'active', 'blocked', 'completed', 'cancelled')),
            evidence_json TEXT NOT NULL DEFAULT '{}',
            position INTEGER NOT NULL CHECK(position >= 0),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS task_steps_task_idx ON task_steps(owner_id, task_id, position);
        CREATE TABLE IF NOT EXISTS task_blockers (
            blocker_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            description TEXT NOT NULL,
            assigned_owner TEXT NOT NULL CHECK(assigned_owner IN ('user', 'vic', 'shared')),
            status TEXT NOT NULL CHECK(status IN ('open', 'resolved')),
            created_at TEXT NOT NULL,
            resolved_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS task_blockers_task_idx ON task_blockers(owner_id, task_id, status);
        CREATE TABLE IF NOT EXISTS task_handoffs (
            handoff_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            from_owner TEXT NOT NULL CHECK(from_owner IN ('user', 'vic')),
            to_owner TEXT NOT NULL CHECK(to_owner IN ('user', 'vic')),
            kind TEXT NOT NULL CHECK(kind IN ('handoff', 'review', 'approval')),
            summary TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('pending', 'accepted', 'completed', 'cancelled')),
            created_at TEXT NOT NULL,
            completed_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS task_handoffs_task_idx ON task_handoffs(owner_id, task_id, status);
        CREATE TABLE IF NOT EXISTS task_artifacts (
            task_artifact_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            uri TEXT NOT NULL,
            description TEXT NOT NULL,
            created_by TEXT NOT NULL CHECK(created_by IN ('user', 'vic')),
            created_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS task_artifacts_task_idx ON task_artifacts(owner_id, task_id, created_at);
        CREATE TABLE IF NOT EXISTS jobs (
            job_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT,
            status TEXT NOT NULL CHECK(status IN ('proposed', 'approved', 'running', 'paused', 'completed', 'failed', 'cancelled')),
            idempotency_key TEXT NOT NULL,
            capability_scope_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id),
            UNIQUE(owner_id, idempotency_key)
        );
        CREATE TABLE IF NOT EXISTS skills (
            skill_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            name TEXT NOT NULL,
            version INTEGER NOT NULL CHECK(version > 0),
            status TEXT NOT NULL CHECK(status IN ('proposed', 'approved', 'rejected', 'disabled')),
            content TEXT NOT NULL,
            required_capabilities_json TEXT NOT NULL,
            evidence_json TEXT NOT NULL,
            evidence_sha256 TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            UNIQUE(owner_id, name, version),
            UNIQUE(owner_id, name, evidence_sha256)
        );
        CREATE INDEX IF NOT EXISTS skills_owner_status_idx
            ON skills(owner_id, status, updated_at DESC);
        CREATE TABLE IF NOT EXISTS automation_proposals (
            automation_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            skill_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('proposed', 'approved', 'rejected', 'disabled')),
            trigger_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(skill_id) REFERENCES skills(skill_id)
        );
        CREATE TABLE IF NOT EXISTS artifacts (
            artifact_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            job_id TEXT,
            kind TEXT NOT NULL,
            uri TEXT NOT NULL,
            sha256 TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(job_id) REFERENCES jobs(job_id)
        );
        CREATE TABLE IF NOT EXISTS execution_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_id TEXT NOT NULL,
            stream_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            actor TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
        CREATE INDEX IF NOT EXISTS execution_events_stream_idx
            ON execution_events(owner_id, stream_id, event_id);
        CREATE TRIGGER IF NOT EXISTS execution_events_no_update
            BEFORE UPDATE ON execution_events BEGIN
                SELECT RAISE(ABORT, 'execution_events are append-only');
            END;
        CREATE TRIGGER IF NOT EXISTS execution_events_no_delete
            BEFORE DELETE ON execution_events BEGIN
                SELECT RAISE(ABORT, 'execution_events are append-only');
            END;
        CREATE TABLE IF NOT EXISTS provider_runs (
            provider_run_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_id TEXT NOT NULL,
            job_id TEXT,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER,
            output_tokens INTEGER,
            duration_ms INTEGER NOT NULL CHECK(duration_ms > 0),
            output_tokens_per_second REAL,
            cost_usd REAL,
            status TEXT NOT NULL CHECK(status IN ('completed', 'failed', 'cancelled')),
            created_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(job_id) REFERENCES jobs(job_id)
        );
        CREATE INDEX IF NOT EXISTS provider_runs_owner_idx
            ON provider_runs(owner_id, provider_run_id DESC);
        "#,
    )?;
    add_column(connection, "conversations", "owner_id", "TEXT")?;
    add_column(connection, "messages", "origin_device_id", "TEXT")?;
    add_column(connection, "messages", "request_id", "TEXT")?;
    add_column(connection, "memories", "owner_id", "TEXT")?;
    add_column(connection, "documents", "owner_id", "TEXT")?;
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS owner_devices (
            owner_id TEXT NOT NULL,
            device_id TEXT NOT NULL UNIQUE,
            role TEXT NOT NULL DEFAULT 'personal',
            display_name TEXT,
            enrolled_at TEXT NOT NULL,
            revoked_at TEXT,
            PRIMARY KEY(owner_id, device_id),
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(device_id) REFERENCES devices(device_id)
        );
        CREATE INDEX IF NOT EXISTS conversations_owner_status_idx
            ON conversations(owner_id, status, updated_at);
        CREATE UNIQUE INDEX IF NOT EXISTS messages_conversation_request_idx
            ON messages(conversation_id, request_id) WHERE request_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS memories_owner_idx ON memories(owner_id, updated_at);
        CREATE INDEX IF NOT EXISTS documents_owner_idx ON documents(owner_id, created_at);
        "#,
    )?;
    Ok(())
}

fn add_column(
    connection: &Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> rusqlite::Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let present = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    if !present {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {declaration};"
        ))?;
    }
    Ok(())
}
