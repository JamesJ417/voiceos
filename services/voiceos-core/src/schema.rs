use rusqlite::{Connection, OptionalExtension};

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
        CREATE TABLE IF NOT EXISTS context_quarantine (
            quarantine_id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            claim_id TEXT NOT NULL,
            source TEXT NOT NULL,
            provenance TEXT NOT NULL,
            confidence REAL NOT NULL,
            relevance REAL NOT NULL,
            content TEXT NOT NULL,
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(conversation_id) REFERENCES conversations(conversation_id)
        );
        CREATE INDEX IF NOT EXISTS context_quarantine_conversation_idx
            ON context_quarantine(conversation_id, created_at);
        CREATE TABLE IF NOT EXISTS memories (
            memory_id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            normalized_content TEXT NOT NULL,
            content TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
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
        CREATE TABLE IF NOT EXISTS attachments (
            attachment_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            device_id TEXT NOT NULL,
            filename TEXT NOT NULL,
            media_type TEXT NOT NULL,
            byte_size INTEGER NOT NULL,
            sha256 TEXT NOT NULL,
            source_bytes BLOB NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('uploaded', 'attached')),
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            FOREIGN KEY(device_id) REFERENCES devices(device_id)
        );
        CREATE INDEX IF NOT EXISTS attachments_owner_status_idx ON attachments(owner_id, status, created_at);
        CREATE TABLE IF NOT EXISTS upload_sessions (
            upload_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            device_id TEXT NOT NULL,
            filename TEXT NOT NULL,
            media_type TEXT NOT NULL,
            byte_size INTEGER NOT NULL,
            sha256 TEXT NOT NULL,
            received_bytes INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'created' CHECK(status IN ('created', 'uploading', 'finalized')),
            attachment_id TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY(device_id) REFERENCES devices(device_id),
            FOREIGN KEY(attachment_id) REFERENCES attachments(attachment_id)
        );
        CREATE INDEX IF NOT EXISTS upload_sessions_owner_idx ON upload_sessions(owner_id, created_at);
        CREATE TABLE IF NOT EXISTS upload_chunks (
            upload_id TEXT NOT NULL,
            offset INTEGER NOT NULL,
            bytes BLOB NOT NULL,
            PRIMARY KEY(upload_id, offset),
            FOREIGN KEY(upload_id) REFERENCES upload_sessions(upload_id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS message_attachments (
            message_id INTEGER NOT NULL,
            attachment_id TEXT NOT NULL,
            PRIMARY KEY(message_id, attachment_id),
            FOREIGN KEY(message_id) REFERENCES messages(message_id) ON DELETE CASCADE,
            FOREIGN KEY(attachment_id) REFERENCES attachments(attachment_id)
        );
        CREATE TABLE IF NOT EXISTS fieldy_transcript_intake (
            intake_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            source TEXT NOT NULL CHECK(source = 'fieldy'),
            event_id TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            received_at TEXT NOT NULL,
            raw_payload_json TEXT NOT NULL,
            normalized_transcript TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('received', 'reviewing', 'approved', 'discarded', 'expired', 'failed')),
            expires_at TEXT NOT NULL,
            processing_error TEXT,
            review_metadata_json TEXT,
            UNIQUE(owner_id, source, event_id)
        );
        CREATE INDEX IF NOT EXISTS fieldy_intake_owner_status_idx ON fieldy_transcript_intake(owner_id, status, received_at);

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
            due_at TEXT,
            importance TEXT NOT NULL DEFAULT 'normal' CHECK(importance IN ('low', 'normal', 'high', 'critical')),
            status TEXT NOT NULL CHECK(status IN ('proposed', 'ready', 'active', 'blocked', 'completed', 'cancelled')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(project_id) REFERENCES projects(project_id),
            FOREIGN KEY(parent_task_id) REFERENCES tasks(task_id)
        );
        CREATE INDEX IF NOT EXISTS tasks_owner_status_idx ON tasks(owner_id, status, updated_at);
        CREATE TABLE IF NOT EXISTS focus_sessions (
            focus_session_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            step_id TEXT,
            mode TEXT NOT NULL CHECK(mode IN ('normal', 'five_minute', 'low_energy', 'restart')),
            planned_minutes INTEGER NOT NULL CHECK(planned_minutes BETWEEN 1 AND 120),
            status TEXT NOT NULL CHECK(status IN ('active', 'interrupted', 'completed', 'cancelled')),
            next_action TEXT NOT NULL,
            interruption_note TEXT,
            restart_action TEXT,
            reflection TEXT,
            started_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            ended_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id),
            FOREIGN KEY(step_id) REFERENCES task_steps(step_id)
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_active_focus_session_per_owner
            ON focus_sessions(owner_id) WHERE status='active';
        CREATE INDEX IF NOT EXISTS focus_sessions_owner_updated_idx
            ON focus_sessions(owner_id, updated_at DESC);
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
        CREATE TABLE IF NOT EXISTS task_review_state (
            owner_id TEXT PRIMARY KEY,
            cursor_task_id TEXT,
            active_review_id TEXT,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(cursor_task_id) REFERENCES tasks(task_id)
        );
        CREATE TABLE IF NOT EXISTS task_review_runs (
            review_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('running','completed','failed','expired')),
            lease_expires_at TEXT NOT NULL,
            safe_actions_json TEXT NOT NULL DEFAULT '[]',
            blockers_json TEXT NOT NULL DEFAULT '[]',
            ideas_json TEXT NOT NULL DEFAULT '[]',
            summary TEXT,
            error_code TEXT,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id)
        );
        CREATE INDEX IF NOT EXISTS task_review_runs_owner_status_lease_idx ON task_review_runs(owner_id, status, lease_expires_at);
        CREATE INDEX IF NOT EXISTS task_review_runs_owner_task_started_idx ON task_review_runs(owner_id, task_id, started_at DESC);
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
        CREATE TABLE IF NOT EXISTS outreach_events (
            outreach_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('status_update', 'check_in', 'question', 'blocker', 'review', 'digest')),
            priority TEXT NOT NULL CHECK(priority IN ('quiet', 'check_in', 'needs_you')),
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            reason TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('queued', 'delivered', 'responded', 'snoozed', 'dismissed', 'expired')),
            task_id TEXT,
            conversation_id TEXT,
            dedupe_key TEXT,
            actions_json TEXT NOT NULL,
            scheduled_for TEXT NOT NULL,
            created_at TEXT NOT NULL,
            delivered_at TEXT,
            responded_at TEXT,
            snoozed_until TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id)
        );
        CREATE INDEX IF NOT EXISTS outreach_owner_status_idx
            ON outreach_events(owner_id, status, scheduled_for, created_at);
        CREATE UNIQUE INDEX IF NOT EXISTS outreach_active_dedupe_idx
            ON outreach_events(owner_id, dedupe_key)
            WHERE dedupe_key IS NOT NULL AND status IN ('queued', 'delivered', 'snoozed');
        CREATE TABLE IF NOT EXISTS outreach_policies (
            owner_id TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 1,
            quiet_hours_start TEXT NOT NULL DEFAULT '22:00',
            quiet_hours_end TEXT NOT NULL DEFAULT '08:00',
            timezone TEXT NOT NULL DEFAULT 'America/New_York',
            max_checkins_per_day INTEGER NOT NULL DEFAULT 6,
            cooldown_minutes INTEGER NOT NULL DEFAULT 30,
            driving_mode INTEGER NOT NULL DEFAULT 0,
            spoken_headphones_only INTEGER NOT NULL DEFAULT 1,
            daily_digest_enabled INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
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
        CREATE TABLE IF NOT EXISTS skill_usages (
            usage_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            skill_id TEXT NOT NULL,
            conversation_id TEXT,
            request_id TEXT,
            tool_calls_json TEXT NOT NULL,
            result_json TEXT NOT NULL,
            outcome TEXT NOT NULL CHECK(outcome IN ('completed', 'failed')),
            feedback TEXT CHECK(feedback IN ('correct', 'incorrect')),
            feedback_note TEXT,
            used_at TEXT NOT NULL,
            reviewed_at TEXT,
            reviewed_by TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(skill_id) REFERENCES skills(skill_id)
        );
        CREATE INDEX IF NOT EXISTS skill_usages_owner_idx
            ON skill_usages(owner_id, used_at DESC);
        CREATE INDEX IF NOT EXISTS skill_usages_skill_idx
            ON skill_usages(owner_id, skill_id, used_at DESC);
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
        CREATE TABLE IF NOT EXISTS conversation_floors (
            owner_id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            lease_id TEXT,
            holder_device_id TEXT,
            holder_display_name TEXT,
            phase TEXT NOT NULL CHECK(phase IN ('idle', 'listening', 'processing', 'speaking')),
            partial_transcript TEXT,
            response_text TEXT,
            revision INTEGER NOT NULL DEFAULT 0,
            acquired_at TEXT,
            updated_at TEXT NOT NULL,
            expires_at_unix INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(conversation_id) REFERENCES conversations(conversation_id),
            FOREIGN KEY(holder_device_id) REFERENCES devices(device_id)
        );
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
        CREATE TABLE IF NOT EXISTS sleep_cycles (
            sleep_cycle_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            mode TEXT NOT NULL DEFAULT 'dry_run' CHECK(mode IN ('dry_run', 'commit')),
            status TEXT NOT NULL DEFAULT 'completed' CHECK(status IN ('running', 'completed', 'failed')),
            previous_cycle_id TEXT,
            event_watermark INTEGER NOT NULL DEFAULT 0,
            message_watermark INTEGER NOT NULL DEFAULT 0,
            events_inspected INTEGER NOT NULL DEFAULT 0,
            messages_inspected INTEGER NOT NULL DEFAULT 0,
            memories_before INTEGER NOT NULL DEFAULT 0,
            memories_after INTEGER NOT NULL DEFAULT 0,
            proposed_changes INTEGER NOT NULL DEFAULT 0,
            committed_changes INTEGER NOT NULL DEFAULT 0,
            summary TEXT NOT NULL DEFAULT '',
            input_digest TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            completed_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(previous_cycle_id) REFERENCES sleep_cycles(sleep_cycle_id),
            UNIQUE(owner_id, idempotency_key)
        );
        CREATE INDEX IF NOT EXISTS sleep_cycles_owner_created_idx
            ON sleep_cycles(owner_id, created_at DESC);
        CREATE TABLE IF NOT EXISTS sleep_cycle_changes (
            change_id TEXT PRIMARY KEY,
            sleep_cycle_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK(operation IN ('add', 'reinforce', 'link', 'supersede', 'dispute', 'expire', 'noop')),
            memory_kind TEXT NOT NULL,
            title TEXT NOT NULL,
            detail TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('proposed', 'verified', 'committed', 'rejected')),
            confidence REAL,
            evidence_json TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            FOREIGN KEY(sleep_cycle_id) REFERENCES sleep_cycles(sleep_cycle_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS sleep_cycle_changes_cycle_idx
            ON sleep_cycle_changes(sleep_cycle_id, created_at, change_id);
        "#,
    )?;
    add_column(connection, "conversations", "owner_id", "TEXT")?;
    add_column(connection, "messages", "origin_device_id", "TEXT")?;
    add_column(connection, "messages", "request_id", "TEXT")?;
    add_column(connection, "conversation_summaries", "owner_id", "TEXT")?;
    add_column(connection, "conversation_summaries", "provenance", "TEXT")?;
    add_column(connection, "tasks", "due_at", "TEXT")?;
    add_column(
        connection,
        "tasks",
        "importance",
        "TEXT NOT NULL DEFAULT 'normal' CHECK(importance IN ('low', 'normal', 'high', 'critical'))",
    )?;
    add_column(connection, "memories", "owner_id", "TEXT")?;
    add_column(connection, "memories", "conversation_id", "TEXT")?;
    add_column(
        connection,
        "memories",
        "category",
        "TEXT NOT NULL DEFAULT 'general'",
    )?;
    add_column(
        connection,
        "memories",
        "status",
        "TEXT NOT NULL DEFAULT 'active'",
    )?;
    add_column(
        connection,
        "memories",
        "confidence",
        "REAL NOT NULL DEFAULT 1.0",
    )?;
    add_column(
        connection,
        "memories",
        "provenance",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(connection, "memories", "supersedes_memory_id", "TEXT")?;
    migrate_memory_lifecycle_constraint(connection)?;
    add_column(connection, "documents", "owner_id", "TEXT")?;
    add_column(
        connection,
        "sleep_cycles",
        "status",
        "TEXT NOT NULL DEFAULT 'completed'",
    )?;
    add_column(connection, "sleep_cycles", "previous_cycle_id", "TEXT")?;
    add_column(
        connection,
        "sleep_cycles",
        "event_watermark",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "message_watermark",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "events_inspected",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "messages_inspected",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "memories_before",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "memories_after",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "proposed_changes",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "committed_changes",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "sleep_cycles",
        "summary",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(connection, "sleep_cycles", "completed_at", "TEXT")?;
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS personal_captures (
            capture_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, source TEXT NOT NULL, source_id TEXT NOT NULL,
            raw_content TEXT NOT NULL, display_text TEXT NOT NULL DEFAULT '', structured_content_json TEXT,
            status TEXT NOT NULL CHECK(status IN ('received','reviewing','approved','rejected','snoozed','discarded','expired')),
            created_at TEXT NOT NULL, expires_at TEXT NOT NULL, audit_id TEXT NOT NULL,
            UNIQUE(owner_id, source, source_id), FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
        CREATE INDEX IF NOT EXISTS personal_captures_owner_status_created_idx ON personal_captures(owner_id,status,created_at);
        CREATE TABLE IF NOT EXISTS fieldy_conversation_assemblies (
            assembly_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, capture_id TEXT NOT NULL UNIQUE,
            status TEXT NOT NULL CHECK(status IN ('assembling','analyzed')),
            started_at TEXT NOT NULL, last_event_at TEXT NOT NULL, last_received_at TEXT NOT NULL,
            chunk_count INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(capture_id) REFERENCES personal_captures(capture_id)
        );
        CREATE INDEX IF NOT EXISTS fieldy_assemblies_owner_status_received_idx
            ON fieldy_conversation_assemblies(owner_id,status,last_received_at);
        CREATE TABLE IF NOT EXISTS fieldy_conversation_chunks (
            owner_id TEXT NOT NULL, event_id TEXT NOT NULL, assembly_id TEXT NOT NULL,
            occurred_at TEXT NOT NULL, received_at TEXT NOT NULL, transcript TEXT NOT NULL,
            recording_id TEXT, session_id TEXT,
            speakers_json TEXT NOT NULL DEFAULT '[]', metadata_json TEXT NOT NULL DEFAULT '{}',
            PRIMARY KEY(owner_id,event_id),
            FOREIGN KEY(assembly_id) REFERENCES fieldy_conversation_assemblies(assembly_id)
        );
        CREATE INDEX IF NOT EXISTS fieldy_chunks_assembly_occurred_idx
            ON fieldy_conversation_chunks(assembly_id,occurred_at,event_id);
        CREATE TABLE IF NOT EXISTS capture_proposals (
            proposal_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, capture_id TEXT NOT NULL, title TEXT NOT NULL,
            category TEXT NOT NULL CHECK(category IN ('task','appointment','worry','idea','note')), confidence REAL NOT NULL DEFAULT 0.0,
            details TEXT, suggested_next_action TEXT NOT NULL DEFAULT '', rationale TEXT NOT NULL,
            evidence_capture_ids_json TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL CHECK(status IN ('reviewing','approved','rejected','snoozed','discarded','expired')),
            created_at TEXT NOT NULL, expires_at TEXT NOT NULL, audit_id TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id), FOREIGN KEY(capture_id) REFERENCES personal_captures(capture_id)
        );
        CREATE INDEX IF NOT EXISTS capture_proposals_owner_status_created_idx ON capture_proposals(owner_id,status,created_at);
        CREATE TABLE IF NOT EXISTS personal_review_records (
            record_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, proposal_id TEXT NOT NULL UNIQUE,
            capture_id TEXT NOT NULL, category TEXT NOT NULL CHECK(category IN ('appointment','worry','idea','note')),
            title TEXT NOT NULL, details TEXT, suggested_next_action TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL CHECK(status='reviewable'), created_at TEXT NOT NULL, audit_id TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(proposal_id) REFERENCES capture_proposals(proposal_id),
            FOREIGN KEY(capture_id) REFERENCES personal_captures(capture_id)
        );
        CREATE INDEX IF NOT EXISTS personal_review_records_owner_category_created_idx ON personal_review_records(owner_id,category,created_at);
        CREATE TABLE IF NOT EXISTS daily_focus_resets (
            reset_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, reset_date TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('received','reviewing','approved','rejected','snoozed','discarded','expired')),
            created_at TEXT NOT NULL, expires_at TEXT NOT NULL, audit_id TEXT NOT NULL,
            UNIQUE(owner_id,reset_date), FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
        CREATE INDEX IF NOT EXISTS daily_focus_resets_owner_status_created_idx ON daily_focus_resets(owner_id,status,created_at);
        "#,
    )?;
    add_column(
        connection,
        "personal_captures",
        "display_text",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        connection,
        "fieldy_conversation_chunks",
        "recording_id",
        "TEXT",
    )?;
    add_column(
        connection,
        "fieldy_conversation_chunks",
        "session_id",
        "TEXT",
    )?;
    add_column(
        connection,
        "capture_proposals",
        "confidence",
        "REAL NOT NULL DEFAULT 0.0",
    )?;
    add_column(connection, "capture_proposals", "details", "TEXT")?;
    add_column(
        connection,
        "capture_proposals",
        "suggested_next_action",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        connection,
        "capture_proposals",
        "evidence_capture_ids_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    add_column(connection, "capture_proposals", "project_id", "TEXT")?;
    add_column(
        connection,
        "capture_proposals",
        "dedupe_key",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        connection,
        "capture_proposals",
        "occurrence_count",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column(
        connection,
        "capture_proposals",
        "last_seen_at",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(connection, "personal_review_records", "project_id", "TEXT")?;
    connection.execute(
        "UPDATE capture_proposals SET last_seen_at=created_at WHERE last_seen_at=''",
        [],
    )?;
    connection.execute(
        "UPDATE capture_proposals \
         SET dedupe_key=lower(category || ':' || COALESCE(project_id,'unassigned') || ':' || trim(title)) \
         WHERE dedupe_key='' OR dedupe_key=lower(category || ':' || title)",
        [],
    )?;
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS capture_proposals_owner_dedupe_status_idx \
         ON capture_proposals(owner_id,dedupe_key,status,last_seen_at);",
    )?;
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
        CREATE INDEX IF NOT EXISTS memories_owner_status_idx ON memories(owner_id, status, updated_at);
        CREATE UNIQUE INDEX IF NOT EXISTS active_memory_owner_content_idx ON memories(owner_id, normalized_content) WHERE owner_id IS NOT NULL AND status='active';
        CREATE UNIQUE INDEX IF NOT EXISTS active_memory_device_content_idx ON memories(device_id, normalized_content) WHERE owner_id IS NULL AND status='active';
        CREATE INDEX IF NOT EXISTS documents_owner_idx ON documents(owner_id, created_at);
        "#,
    )?;
    migrate_sleep_cycle_mode_constraint(connection)?;
    add_column(
        connection,
        "sleep_cycles",
        "input_digest",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    connection.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS proactive_subscriptions (subscription_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, topic TEXT NOT NULL, project_id TEXT, source_type TEXT NOT NULL, cadence TEXT NOT NULL, quiet_hours TEXT, status TEXT NOT NULL, provenance TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, FOREIGN KEY(owner_id) REFERENCES owners(owner_id));
        CREATE TABLE IF NOT EXISTS proactive_candidates (candidate_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, subscription_id TEXT, project_id TEXT, reason TEXT NOT NULL, evidence_json TEXT NOT NULL, priority TEXT NOT NULL, confidence REAL NOT NULL, expires_at TEXT NOT NULL, deduplication_key TEXT NOT NULL, provenance TEXT NOT NULL, created_at TEXT NOT NULL, FOREIGN KEY(owner_id) REFERENCES owners(owner_id), FOREIGN KEY(subscription_id) REFERENCES proactive_subscriptions(subscription_id), UNIQUE(owner_id, deduplication_key));
        CREATE TABLE IF NOT EXISTS outreach_proposals (proposal_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, candidate_id TEXT NOT NULL, original_draft TEXT NOT NULL, editable_draft TEXT NOT NULL, channel TEXT NOT NULL, approval_state TEXT NOT NULL, risk_class TEXT NOT NULL, delivery_deadline TEXT, provenance TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, FOREIGN KEY(owner_id) REFERENCES owners(owner_id), FOREIGN KEY(candidate_id) REFERENCES proactive_candidates(candidate_id));
        CREATE TABLE IF NOT EXISTS outreach_deliveries (delivery_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, proposal_id TEXT NOT NULL, provider TEXT NOT NULL, channel TEXT NOT NULL, result TEXT NOT NULL, idempotency_key TEXT NOT NULL, response_link TEXT, provenance TEXT NOT NULL, created_at TEXT NOT NULL, FOREIGN KEY(owner_id) REFERENCES owners(owner_id), FOREIGN KEY(proposal_id) REFERENCES outreach_proposals(proposal_id), UNIQUE(owner_id, idempotency_key));
        CREATE TABLE IF NOT EXISTS proactive_feedback (feedback_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, proposal_id TEXT, action TEXT NOT NULL, note TEXT, provenance TEXT NOT NULL, created_at TEXT NOT NULL, FOREIGN KEY(owner_id) REFERENCES owners(owner_id), FOREIGN KEY(proposal_id) REFERENCES outreach_proposals(proposal_id));
        CREATE TRIGGER IF NOT EXISTS proactive_subscription_audit AFTER INSERT ON proactive_subscriptions BEGIN INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(NEW.owner_id,NEW.subscription_id,'proactive.subscription_created','voiceos-core','{}',NEW.created_at); END;
        CREATE TRIGGER IF NOT EXISTS proactive_candidate_audit AFTER INSERT ON proactive_candidates BEGIN INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(NEW.owner_id,NEW.candidate_id,'proactive.candidate_created','voiceos-core','{}',NEW.created_at); END;
        CREATE TRIGGER IF NOT EXISTS outreach_proposal_audit AFTER INSERT ON outreach_proposals BEGIN INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(NEW.owner_id,NEW.proposal_id,'proactive.proposal_created','voiceos-core','{}',NEW.created_at); END;
        CREATE TRIGGER IF NOT EXISTS outreach_delivery_audit AFTER INSERT ON outreach_deliveries BEGIN INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(NEW.owner_id,NEW.delivery_id,'proactive.delivery_recorded','voiceos-core','{}',NEW.created_at); END;
        CREATE TRIGGER IF NOT EXISTS proactive_feedback_audit AFTER INSERT ON proactive_feedback BEGIN INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(NEW.owner_id,NEW.feedback_id,'proactive.feedback_recorded','voiceos-core','{}',NEW.created_at); END;
    "#)?;
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS google_calendar_connections (
            owner_id TEXT PRIMARY KEY, provider TEXT NOT NULL, account_email TEXT NOT NULL,
            provider_account_id TEXT NOT NULL, secret_reference TEXT, connected_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
    "#,
    )?;
    add_column(
        connection,
        "google_calendar_connections",
        "secret_reference",
        "TEXT",
    )?;
    Ok(())
}

fn migrate_sleep_cycle_mode_constraint(connection: &Connection) -> rusqlite::Result<()> {
    let sql: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='sleep_cycles'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if !sql
        .as_deref()
        .is_some_and(|value| value.contains("mode = 'dry_run'"))
    {
        return Ok(());
    }
    rebuild_table_without_foreign_keys(
        connection,
        "BEGIN;
         CREATE TABLE sleep_cycles_rebuilt (
           sleep_cycle_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, idempotency_key TEXT NOT NULL,
           mode TEXT NOT NULL DEFAULT 'dry_run' CHECK(mode IN ('dry_run', 'commit')),
           status TEXT NOT NULL DEFAULT 'completed' CHECK(status IN ('running', 'completed', 'failed')),
           previous_cycle_id TEXT, event_watermark INTEGER NOT NULL DEFAULT 0,
           message_watermark INTEGER NOT NULL DEFAULT 0, events_inspected INTEGER NOT NULL DEFAULT 0,
           messages_inspected INTEGER NOT NULL DEFAULT 0, memories_before INTEGER NOT NULL DEFAULT 0,
           memories_after INTEGER NOT NULL DEFAULT 0, proposed_changes INTEGER NOT NULL DEFAULT 0,
           committed_changes INTEGER NOT NULL DEFAULT 0, summary TEXT NOT NULL DEFAULT '',
           created_at TEXT NOT NULL, completed_at TEXT,
           FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
           FOREIGN KEY(previous_cycle_id) REFERENCES sleep_cycles(sleep_cycle_id),
           UNIQUE(owner_id, idempotency_key)
         );
         INSERT INTO sleep_cycles_rebuilt SELECT sleep_cycle_id, owner_id, idempotency_key, mode, status, previous_cycle_id, event_watermark, message_watermark, events_inspected, messages_inspected, memories_before, memories_after, proposed_changes, committed_changes, summary, created_at, completed_at FROM sleep_cycles;
         DROP TABLE sleep_cycles;
         ALTER TABLE sleep_cycles_rebuilt RENAME TO sleep_cycles;
         CREATE INDEX IF NOT EXISTS sleep_cycles_owner_created_idx ON sleep_cycles(owner_id, created_at DESC);
         COMMIT;",
    )
}

fn migrate_memory_lifecycle_constraint(connection: &Connection) -> rusqlite::Result<()> {
    let sql: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='memories'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if !sql
        .as_deref()
        .is_some_and(|value| value.contains("UNIQUE(device_id, normalized_content)"))
    {
        return Ok(());
    }
    rebuild_table_without_foreign_keys(
        connection,
        "BEGIN;
         CREATE TABLE memories_rebuilt (
           memory_id TEXT PRIMARY KEY, device_id TEXT NOT NULL, normalized_content TEXT NOT NULL,
           content TEXT NOT NULL, source TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
           owner_id TEXT, conversation_id TEXT, category TEXT NOT NULL DEFAULT 'general',
           status TEXT NOT NULL DEFAULT 'active', confidence REAL NOT NULL DEFAULT 1.0,
           provenance TEXT NOT NULL DEFAULT '', supersedes_memory_id TEXT,
           FOREIGN KEY(device_id) REFERENCES devices(device_id));
         INSERT INTO memories_rebuilt(memory_id,device_id,normalized_content,content,source,created_at,updated_at,owner_id,conversation_id,category,status,confidence,provenance,supersedes_memory_id)
           SELECT memory_id,device_id,normalized_content,content,source,created_at,updated_at,owner_id,conversation_id,category,status,confidence,provenance,supersedes_memory_id FROM memories;
         DROP TABLE memories;
         ALTER TABLE memories_rebuilt RENAME TO memories;
         COMMIT;",
    )?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS google_calendar_connections (\
            owner_id TEXT PRIMARY KEY,\
            provider TEXT NOT NULL,\
            account_email TEXT NOT NULL,\
            provider_account_id TEXT NOT NULL,\
            secret_reference TEXT,\
            connected_at TEXT NOT NULL,\
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)\
        );",
    )
}

fn rebuild_table_without_foreign_keys(
    connection: &Connection,
    statements: &str,
) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "OFF")?;
    let rebuild = connection.execute_batch(statements);
    if rebuild.is_err() {
        let _ = connection.execute_batch("ROLLBACK;");
    }
    let restore = connection.pragma_update(None, "foreign_keys", "ON");
    rebuild?;
    restore
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
