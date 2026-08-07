use rusqlite::Connection;

pub(crate) fn apply_current_schema(connection: &Connection) -> rusqlite::Result<()> {
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
            due_at TEXT,
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
            do_not_disturb INTEGER NOT NULL DEFAULT 0,
            current_location TEXT NOT NULL DEFAULT 'unknown',
            daily_planning_time TEXT NOT NULL DEFAULT '08:30',
            morning_digest_time TEXT NOT NULL DEFAULT '08:00',
            evening_digest_time TEXT NOT NULL DEFAULT '18:00',
            scan_interval_minutes INTEGER NOT NULL DEFAULT 20,
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
        CREATE TABLE IF NOT EXISTS automation_rules (
            automation_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            trigger_json TEXT NOT NULL,
            conditions_json TEXT NOT NULL,
            permitted_actions_json TEXT NOT NULL,
            frequency_max_runs INTEGER NOT NULL,
            frequency_window_minutes INTEGER NOT NULL,
            evidence_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            UNIQUE(owner_id, name)
        );
        CREATE INDEX IF NOT EXISTS automation_rules_owner_enabled_idx
            ON automation_rules(owner_id, enabled, updated_at DESC);
        CREATE TABLE IF NOT EXISTS attention_items (
            attention_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            category TEXT NOT NULL CHECK(category IN ('email','calendar','question','approval','document','system','message','agent_work')),
            source_id TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            urgency TEXT NOT NULL CHECK(urgency IN ('routine','important','urgent')),
            status TEXT NOT NULL CHECK(status IN ('open','snoozed','resolved','dismissed')),
            task_id TEXT,
            occurred_at TEXT NOT NULL,
            due_at TEXT,
            approval_required INTEGER NOT NULL DEFAULT 0,
            available_actions_json TEXT NOT NULL DEFAULT '[]',
            evidence_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id),
            UNIQUE(owner_id, category, source_id)
        );
        CREATE INDEX IF NOT EXISTS attention_items_owner_status_idx
            ON attention_items(owner_id, status, urgency, occurred_at DESC);
        CREATE TABLE IF NOT EXISTS task_schedules (
            task_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            earliest_start_at TEXT,
            recurrence_rule TEXT,
            location TEXT,
            preparation_minutes INTEGER NOT NULL DEFAULT 0 CHECK(preparation_minutes BETWEEN 0 AND 1440),
            travel_minutes INTEGER NOT NULL DEFAULT 0 CHECK(travel_minutes BETWEEN 0 AND 1440),
            preferred_time TEXT,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS calendar_events (
            calendar_event_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            title TEXT NOT NULL,
            start_at TEXT NOT NULL,
            end_at TEXT NOT NULL,
            location TEXT,
            status TEXT NOT NULL CHECK(status IN ('confirmed','tentative','cancelled')),
            response_status TEXT NOT NULL CHECK(response_status IN ('none','needs_action','accepted','declined','tentative')),
            task_id TEXT,
            preparation_minutes INTEGER NOT NULL DEFAULT 0,
            travel_minutes INTEGER NOT NULL DEFAULT 0,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id),
            UNIQUE(owner_id, source_id)
        );
        CREATE INDEX IF NOT EXISTS calendar_events_owner_time_idx
            ON calendar_events(owner_id, start_at, end_at);
        CREATE TABLE IF NOT EXISTS update_proposals (
            update_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            component TEXT NOT NULL,
            current_version TEXT NOT NULL,
            proposed_version TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('discovered','approved','rejected','candidate_ready','deploying','deployed','failed','rolled_back')),
            release_notes TEXT NOT NULL,
            dependency_changes_json TEXT NOT NULL,
            api_changes_json TEXT NOT NULL,
            configuration_changes_json TEXT NOT NULL,
            skill_changes_json TEXT NOT NULL,
            security_changes_json TEXT NOT NULL,
            affected_components_json TEXT NOT NULL,
            rollback_version TEXT NOT NULL,
            candidate_path TEXT,
            evidence_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            UNIQUE(owner_id, component, proposed_version)
        );
        CREATE INDEX IF NOT EXISTS update_proposals_owner_status_idx
            ON update_proposals(owner_id,status,updated_at DESC);
        CREATE TABLE IF NOT EXISTS artifacts (
            artifact_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            job_id TEXT,
            task_id TEXT,
            parent_artifact_id TEXT,
            kind TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            filename TEXT NOT NULL DEFAULT '',
            media_type TEXT NOT NULL DEFAULT 'application/octet-stream',
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'ready',
            progress_percent INTEGER NOT NULL DEFAULT 100,
            storage_key TEXT,
            uri TEXT NOT NULL,
            sha256 TEXT,
            byte_size INTEGER,
            version INTEGER NOT NULL DEFAULT 1,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            error TEXT,
            created_by TEXT NOT NULL DEFAULT 'vic',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT '',
            completed_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(job_id) REFERENCES jobs(job_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id),
            FOREIGN KEY(parent_artifact_id) REFERENCES artifacts(artifact_id)
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
        "#,
    )?;
    add_column(connection, "conversations", "owner_id", "TEXT")?;
    add_column(connection, "messages", "origin_device_id", "TEXT")?;
    add_column(connection, "messages", "request_id", "TEXT")?;
    add_column(connection, "memories", "owner_id", "TEXT")?;
    add_column(connection, "documents", "owner_id", "TEXT")?;
    add_column(connection, "artifacts", "task_id", "TEXT")?;
    add_column(connection, "artifacts", "parent_artifact_id", "TEXT")?;
    add_column(connection, "artifacts", "title", "TEXT NOT NULL DEFAULT ''")?;
    add_column(
        connection,
        "artifacts",
        "filename",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        connection,
        "artifacts",
        "media_type",
        "TEXT NOT NULL DEFAULT 'application/octet-stream'",
    )?;
    add_column(
        connection,
        "artifacts",
        "description",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        connection,
        "artifacts",
        "status",
        "TEXT NOT NULL DEFAULT 'ready'",
    )?;
    add_column(
        connection,
        "artifacts",
        "progress_percent",
        "INTEGER NOT NULL DEFAULT 100",
    )?;
    add_column(connection, "artifacts", "storage_key", "TEXT")?;
    add_column(connection, "artifacts", "byte_size", "INTEGER")?;
    add_column(
        connection,
        "artifacts",
        "version",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column(
        connection,
        "artifacts",
        "metadata_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    add_column(connection, "artifacts", "error", "TEXT")?;
    add_column(
        connection,
        "artifacts",
        "created_by",
        "TEXT NOT NULL DEFAULT 'vic'",
    )?;
    add_column(
        connection,
        "artifacts",
        "updated_at",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(connection, "artifacts", "completed_at", "TEXT")?;
    add_column(connection, "tasks", "due_at", "TEXT")?;
    add_column(
        connection,
        "outreach_policies",
        "do_not_disturb",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        connection,
        "outreach_policies",
        "current_location",
        "TEXT NOT NULL DEFAULT 'unknown'",
    )?;
    add_column(
        connection,
        "outreach_policies",
        "daily_planning_time",
        "TEXT NOT NULL DEFAULT '08:30'",
    )?;
    add_column(
        connection,
        "outreach_policies",
        "morning_digest_time",
        "TEXT NOT NULL DEFAULT '08:00'",
    )?;
    add_column(
        connection,
        "outreach_policies",
        "evening_digest_time",
        "TEXT NOT NULL DEFAULT '18:00'",
    )?;
    add_column(
        connection,
        "outreach_policies",
        "scan_interval_minutes",
        "INTEGER NOT NULL DEFAULT 20",
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
        CREATE INDEX IF NOT EXISTS documents_owner_idx ON documents(owner_id, created_at);
        CREATE INDEX IF NOT EXISTS artifacts_owner_idx ON artifacts(owner_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS artifacts_task_idx ON artifacts(owner_id, task_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS artifacts_parent_idx ON artifacts(owner_id, parent_artifact_id, version);

        CREATE TABLE IF NOT EXISTS raw_memory_events (
            event_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            source_ref TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            content_sha256 TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            UNIQUE(owner_id, source_kind, source_ref)
        );
        CREATE INDEX IF NOT EXISTS raw_memory_events_owner_time_idx
            ON raw_memory_events(owner_id, occurred_at, event_id);
        CREATE TRIGGER IF NOT EXISTS raw_memory_events_no_update
            BEFORE UPDATE ON raw_memory_events BEGIN
                SELECT RAISE(ABORT, 'raw memory events are immutable');
            END;
        CREATE TRIGGER IF NOT EXISTS raw_memory_events_no_delete
            BEFORE DELETE ON raw_memory_events BEGIN
                SELECT RAISE(ABORT, 'raw memory events are immutable');
            END;

        CREATE TABLE IF NOT EXISTS sleep_cycles (
            cycle_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('running','staged','paused','completed','failed','cancelled','rolled_back')),
            phase TEXT NOT NULL CHECK(phase IN ('preparing','snapshotting','selecting_events','replaying','extracting_memories','forming_connections','detecting_contradictions','dreaming','validating','staging','committing','reporting','completed','failed','rolled_back')),
            mode TEXT NOT NULL CHECK(mode IN ('dry_run','commit')),
            trigger_kind TEXT NOT NULL CHECK(trigger_kind IN ('manual','scheduled','resume','test')),
            config_json TEXT NOT NULL,
            snapshot_sha256 TEXT,
            model_budget_used INTEGER NOT NULL DEFAULT 0,
            metrics_json TEXT NOT NULL DEFAULT '{}',
            error TEXT,
            started_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            completed_at TEXT,
            rolled_back_at TEXT,
            rollback_reason TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
        CREATE INDEX IF NOT EXISTS sleep_cycles_owner_idx
            ON sleep_cycles(owner_id, started_at DESC);
        CREATE UNIQUE INDEX IF NOT EXISTS sleep_cycles_one_active_owner_idx
            ON sleep_cycles(owner_id)
            WHERE status IN ('running','staged','paused');
        CREATE TABLE IF NOT EXISTS sleep_cycle_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            cycle_id TEXT NOT NULL,
            phase TEXT NOT NULL,
            status TEXT NOT NULL,
            metrics_json TEXT NOT NULL DEFAULT '{}',
            occurred_at TEXT NOT NULL,
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id)
        );
        CREATE INDEX IF NOT EXISTS sleep_cycle_events_cycle_idx
            ON sleep_cycle_events(cycle_id, sequence);
        CREATE TRIGGER IF NOT EXISTS sleep_cycle_events_no_update
            BEFORE UPDATE ON sleep_cycle_events BEGIN
                SELECT RAISE(ABORT, 'sleep cycle events are append-only');
            END;
        CREATE TRIGGER IF NOT EXISTS sleep_cycle_events_no_delete
            BEFORE DELETE ON sleep_cycle_events BEGIN
                SELECT RAISE(ABORT, 'sleep cycle events are append-only');
            END;

        CREATE TABLE IF NOT EXISTS sleep_event_selection (
            cycle_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            selected INTEGER NOT NULL CHECK(selected IN (0,1)),
            salience_score REAL NOT NULL,
            score_components_json TEXT NOT NULL,
            reason TEXT NOT NULL,
            PRIMARY KEY(cycle_id, event_id),
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id),
            FOREIGN KEY(event_id) REFERENCES raw_memory_events(event_id)
        );
        CREATE TABLE IF NOT EXISTS sleep_snapshots (
            cycle_id TEXT PRIMARY KEY,
            active_memory_ids_json TEXT NOT NULL,
            active_view_sha256 TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id)
        );

        CREATE TABLE IF NOT EXISTS memory_proposals (
            proposal_id TEXT PRIMARY KEY,
            cycle_id TEXT NOT NULL,
            owner_id TEXT NOT NULL,
            proposal_kind TEXT NOT NULL CHECK(proposal_kind IN ('memory','connection','contradiction','skill')),
            memory_kind TEXT,
            cognitive_status TEXT NOT NULL CHECK(cognitive_status IN ('verified_fact','supported_inference','working_hypothesis','dream_association','disputed','superseded','rejected')),
            content TEXT NOT NULL,
            normalized_content TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            provider TEXT NOT NULL,
            model_version TEXT,
            operation_version TEXT NOT NULL,
            confidence REAL NOT NULL CHECK(confidence BETWEEN 0.0 AND 1.0),
            protected INTEGER NOT NULL DEFAULT 0 CHECK(protected IN (0,1)),
            approval_required INTEGER NOT NULL DEFAULT 0 CHECK(approval_required IN (0,1)),
            approval_status TEXT NOT NULL CHECK(approval_status IN ('not_required','pending','approved','rejected')),
            validation_status TEXT NOT NULL CHECK(validation_status IN ('pending','valid','invalid')),
            validation_errors_json TEXT NOT NULL DEFAULT '[]',
            dedupe_key TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id),
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            UNIQUE(cycle_id, dedupe_key)
        );
        CREATE INDEX IF NOT EXISTS memory_proposals_cycle_idx
            ON memory_proposals(cycle_id, validation_status, approval_status);

        CREATE TABLE IF NOT EXISTS cognitive_memories (
            memory_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            cycle_id TEXT NOT NULL,
            proposal_id TEXT NOT NULL UNIQUE,
            memory_kind TEXT NOT NULL CHECK(memory_kind IN ('working','episodic','semantic','procedural','identity_doctrine','dream_association')),
            cognitive_status TEXT NOT NULL CHECK(cognitive_status IN ('verified_fact','supported_inference','working_hypothesis','dream_association','disputed','superseded','rejected')),
            content TEXT NOT NULL,
            normalized_content TEXT NOT NULL,
            confidence REAL NOT NULL CHECK(confidence BETWEEN 0.0 AND 1.0),
            active INTEGER NOT NULL CHECK(active IN (0,1)),
            quarantined INTEGER NOT NULL CHECK(quarantined IN (0,1)),
            protected INTEGER NOT NULL CHECK(protected IN (0,1)),
            provider TEXT NOT NULL,
            model_version TEXT,
            operation_version TEXT NOT NULL,
            revision_of TEXT,
            expires_at TEXT,
            invalidation_conditions_json TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            committed_at TEXT,
            deactivated_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id),
            FOREIGN KEY(proposal_id) REFERENCES memory_proposals(proposal_id),
            FOREIGN KEY(revision_of) REFERENCES cognitive_memories(memory_id)
        );
        CREATE INDEX IF NOT EXISTS cognitive_memories_retrieval_idx
            ON cognitive_memories(owner_id, active, quarantined, created_at DESC);
        CREATE UNIQUE INDEX IF NOT EXISTS cognitive_memories_active_dedupe_idx
            ON cognitive_memories(owner_id, normalized_content, memory_kind)
            WHERE active=1;
        CREATE TRIGGER IF NOT EXISTS cognitive_memories_dream_transition_guard
            BEFORE UPDATE OF cognitive_status ON cognitive_memories
            WHEN OLD.cognitive_status='dream_association'
                 AND NEW.cognitive_status NOT IN ('dream_association','working_hypothesis')
            BEGIN
                SELECT RAISE(ABORT, 'dream associations may only promote to working hypotheses');
            END;
        CREATE TRIGGER IF NOT EXISTS cognitive_memories_dream_promotion_shape_guard
            BEFORE UPDATE ON cognitive_memories
            WHEN OLD.cognitive_status='dream_association'
                 AND NEW.cognitive_status='working_hypothesis'
                 AND (NEW.memory_kind<>'semantic' OR NEW.active<>1 OR NEW.quarantined<>0)
            BEGIN
                SELECT RAISE(ABORT, 'dream promotion must produce an active semantic working hypothesis');
            END;
        CREATE TRIGGER IF NOT EXISTS cognitive_memories_dream_origin_advancement_guard
            BEFORE UPDATE OF cognitive_status ON cognitive_memories
            WHEN OLD.cognitive_status='working_hypothesis'
                 AND NEW.cognitive_status IN ('supported_inference','verified_fact')
                 AND EXISTS(
                     SELECT 1 FROM memory_proposals p
                     WHERE p.proposal_id=OLD.proposal_id
                       AND p.memory_kind='dream_association'
                 )
            BEGIN
                SELECT RAISE(ABORT, 'dream-origin hypotheses require a separate evidence-validation workflow');
            END;

        CREATE TABLE IF NOT EXISTS memory_provenance (
            memory_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            evidence_role TEXT NOT NULL CHECK(evidence_role IN ('supports','contradicts','derived_from')),
            confidence REAL NOT NULL CHECK(confidence BETWEEN 0.0 AND 1.0),
            created_at TEXT NOT NULL,
            PRIMARY KEY(memory_id, event_id, evidence_role),
            FOREIGN KEY(memory_id) REFERENCES cognitive_memories(memory_id),
            FOREIGN KEY(event_id) REFERENCES raw_memory_events(event_id)
        );
        CREATE TABLE IF NOT EXISTS memory_links (
            link_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            cycle_id TEXT NOT NULL,
            source_memory_id TEXT NOT NULL,
            target_memory_id TEXT NOT NULL,
            relation TEXT NOT NULL CHECK(relation IN ('supports','contradicts','caused','preceded','follows','related_to','part_of','derived_from','applies_to','exception_to','supersedes','duplicates','unresolved_with','predicts','outcome_of')),
            confidence REAL NOT NULL CHECK(confidence BETWEEN 0.0 AND 1.0),
            evidence_json TEXT NOT NULL,
            cognitive_status TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
            created_at TEXT NOT NULL,
            deactivated_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id),
            FOREIGN KEY(source_memory_id) REFERENCES cognitive_memories(memory_id),
            FOREIGN KEY(target_memory_id) REFERENCES cognitive_memories(memory_id)
        );
        CREATE TABLE IF NOT EXISTS memory_contradictions (
            contradiction_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            cycle_id TEXT NOT NULL,
            existing_memory_id TEXT,
            proposal_id TEXT NOT NULL,
            conflict_kind TEXT NOT NULL CHECK(conflict_kind IN ('factual','temporal','preference','interpretive','correction','unknown')),
            summary TEXT NOT NULL,
            evidence_json TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('open','resolved','dismissed')),
            resolution TEXT,
            requires_human_review INTEGER NOT NULL CHECK(requires_human_review IN (0,1)),
            created_at TEXT NOT NULL,
            resolved_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id),
            FOREIGN KEY(existing_memory_id) REFERENCES cognitive_memories(memory_id),
            FOREIGN KEY(proposal_id) REFERENCES memory_proposals(proposal_id)
        );
        CREATE INDEX IF NOT EXISTS memory_contradictions_owner_idx
            ON memory_contradictions(owner_id, status, created_at DESC);

        CREATE TABLE IF NOT EXISTS retrieval_quality_results (
            result_id TEXT PRIMARY KEY,
            cycle_id TEXT NOT NULL,
            query TEXT NOT NULL,
            baseline_ids_json TEXT NOT NULL,
            staged_ids_json TEXT NOT NULL,
            passed INTEGER NOT NULL CHECK(passed IN (0,1)),
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id)
        );
        CREATE TABLE IF NOT EXISTS morning_reports (
            report_id TEXT PRIMARY KEY,
            cycle_id TEXT NOT NULL UNIQUE,
            owner_id TEXT NOT NULL,
            report_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(cycle_id) REFERENCES sleep_cycles(cycle_id),
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );

        CREATE TABLE IF NOT EXISTS doctrine_source_profiles (
            profile_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            internal_name TEXT NOT NULL,
            visible_to_conversation INTEGER NOT NULL DEFAULT 0 CHECK(visible_to_conversation=0),
            approved INTEGER NOT NULL CHECK(approved IN (0,1)),
            allow_direct_quotes INTEGER NOT NULL DEFAULT 0 CHECK(allow_direct_quotes=0),
            allow_voice_imitation INTEGER NOT NULL DEFAULT 0 CHECK(allow_voice_imitation=0),
            allow_style_imitation INTEGER NOT NULL DEFAULT 0 CHECK(allow_style_imitation=0),
            allow_identity_simulation INTEGER NOT NULL DEFAULT 0 CHECK(allow_identity_simulation=0),
            permitted_uses_json TEXT NOT NULL,
            prohibited_uses_json TEXT NOT NULL,
            domains_json TEXT NOT NULL,
            corpus_locations_json TEXT NOT NULL DEFAULT '[]',
            authorization_status TEXT NOT NULL CHECK(authorization_status IN ('approved','pending','revoked')),
            authorization_basis TEXT NOT NULL,
            ingestion_status TEXT NOT NULL DEFAULT 'empty' CHECK(ingestion_status IN ('empty','pending','processing','processed','blocked','revoked')),
            extraction_version TEXT NOT NULL,
            last_processed_at TEXT,
            source_count INTEGER NOT NULL DEFAULT 0,
            source_types_json TEXT NOT NULL DEFAULT '[]',
            review_status TEXT NOT NULL DEFAULT 'approved' CHECK(review_status IN ('pending','approved','rejected')),
            notes TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            UNIQUE(owner_id, internal_name)
        );
        CREATE INDEX IF NOT EXISTS doctrine_profiles_owner_idx ON doctrine_source_profiles(owner_id, approved, review_status);

        CREATE TABLE IF NOT EXISTS doctrine_source_records (
            record_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            profile_id TEXT NOT NULL,
            source_type TEXT NOT NULL CHECK(source_type IN ('user_note','licensed_excerpt','public_domain','authorized_transcript','authorized_document')),
            title TEXT NOT NULL,
            private_origin TEXT NOT NULL,
            publication_date TEXT,
            ingested_at TEXT NOT NULL,
            authorization_status TEXT NOT NULL CHECK(authorization_status IN ('approved','pending','revoked')),
            authorization_basis TEXT NOT NULL,
            content_sha256 TEXT NOT NULL,
            storage_location TEXT NOT NULL,
            source_content BLOB NOT NULL,
            extraction_status TEXT NOT NULL CHECK(extraction_status IN ('pending','processing','processed','failed','revoked')),
            source_quality REAL NOT NULL CHECK(source_quality BETWEEN 0.0 AND 1.0),
            duplicate_of TEXT,
            active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
            revoked_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(profile_id) REFERENCES doctrine_source_profiles(profile_id),
            FOREIGN KEY(duplicate_of) REFERENCES doctrine_source_records(record_id),
            UNIQUE(owner_id, content_sha256)
        );
        CREATE INDEX IF NOT EXISTS doctrine_records_owner_idx ON doctrine_source_records(owner_id, extraction_status, active);
        CREATE TABLE IF NOT EXISTS doctrine_source_passages (
            passage_id TEXT PRIMARY KEY,
            record_id TEXT NOT NULL,
            passage_index INTEGER NOT NULL,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            content TEXT NOT NULL,
            content_sha256 TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(record_id) REFERENCES doctrine_source_records(record_id),
            UNIQUE(record_id, passage_index)
        );
        CREATE TRIGGER IF NOT EXISTS doctrine_passages_no_update BEFORE UPDATE ON doctrine_source_passages
        BEGIN SELECT RAISE(ABORT, 'doctrine source passages are immutable'); END;
        CREATE TRIGGER IF NOT EXISTS doctrine_passages_no_delete BEFORE DELETE ON doctrine_source_passages
        BEGIN SELECT RAISE(ABORT, 'doctrine source passages are immutable'); END;

        CREATE TABLE IF NOT EXISTS doctrine_candidates (
            candidate_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            normalized_proposition TEXT NOT NULL,
            normalized_key TEXT NOT NULL,
            domain TEXT NOT NULL,
            principle_type TEXT NOT NULL,
            decision_rule TEXT NOT NULL,
            rationale TEXT NOT NULL,
            applicable_conditions_json TEXT NOT NULL,
            exceptions_json TEXT NOT NULL,
            counterexamples_json TEXT NOT NULL,
            risk_posture TEXT NOT NULL,
            time_horizon TEXT NOT NULL,
            ethical_constraints_json TEXT NOT NULL,
            source_profile_diversity INTEGER NOT NULL DEFAULT 0,
            extraction_model TEXT NOT NULL,
            extraction_prompt_version TEXT NOT NULL,
            confidence REAL NOT NULL CHECK(confidence BETWEEN 0.0 AND 1.0),
            abstraction_score REAL NOT NULL CHECK(abstraction_score BETWEEN 0.0 AND 1.0),
            style_contamination_score REAL NOT NULL CHECK(style_contamination_score BETWEEN 0.0 AND 1.0),
            identity_contamination_score REAL NOT NULL CHECK(identity_contamination_score BETWEEN 0.0 AND 1.0),
            status TEXT NOT NULL CHECK(status IN ('extracted','decontamination_failed','normalized','disputed','awaiting_review','approved','active','superseded','rejected','archived')),
            review_requirement TEXT NOT NULL CHECK(review_requirement IN ('explicit','protected')),
            protected INTEGER NOT NULL DEFAULT 1 CHECK(protected=1),
            created_cycle_id TEXT,
            revision_of TEXT,
            version INTEGER NOT NULL DEFAULT 1,
            validation_errors_json TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            activated_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(created_cycle_id) REFERENCES sleep_cycles(cycle_id),
            FOREIGN KEY(revision_of) REFERENCES doctrine_candidates(candidate_id),
            UNIQUE(owner_id, normalized_key, version)
        );
        CREATE INDEX IF NOT EXISTS doctrine_candidates_owner_idx ON doctrine_candidates(owner_id, status, domain, updated_at DESC);
        CREATE TABLE IF NOT EXISTS doctrine_candidate_sources (
            candidate_id TEXT NOT NULL,
            passage_id TEXT NOT NULL,
            evidence_role TEXT NOT NULL CHECK(evidence_role IN ('supports','contradicts')),
            directness REAL NOT NULL CHECK(directness BETWEEN 0.0 AND 1.0),
            created_at TEXT NOT NULL,
            PRIMARY KEY(candidate_id, passage_id, evidence_role),
            FOREIGN KEY(candidate_id) REFERENCES doctrine_candidates(candidate_id),
            FOREIGN KEY(passage_id) REFERENCES doctrine_source_passages(passage_id)
        );
        CREATE TABLE IF NOT EXISTS doctrine_contradictions (
            contradiction_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            left_candidate_id TEXT NOT NULL,
            right_candidate_id TEXT NOT NULL,
            tension_kind TEXT NOT NULL,
            summary TEXT NOT NULL,
            conditions_json TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL CHECK(status IN ('open','resolved','dismissed')),
            resolution TEXT,
            created_at TEXT NOT NULL,
            resolved_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(left_candidate_id) REFERENCES doctrine_candidates(candidate_id),
            FOREIGN KEY(right_candidate_id) REFERENCES doctrine_candidates(candidate_id)
        );
        CREATE TABLE IF NOT EXISTS doctrine_lenses (
            lens_id TEXT PRIMARY KEY,
            public_name TEXT NOT NULL UNIQUE,
            domains_json TEXT NOT NULL,
            description TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1))
        );
        CREATE TABLE IF NOT EXISTS doctrine_runs (
            run_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            record_id TEXT,
            status TEXT NOT NULL CHECK(status IN ('running','completed','failed')),
            extraction_model TEXT,
            critique_model TEXT,
            metrics_json TEXT NOT NULL DEFAULT '{}',
            error TEXT,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(record_id) REFERENCES doctrine_source_records(record_id)
        );
        CREATE TABLE IF NOT EXISTS doctrine_evaluations (
            evaluation_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            evaluation_kind TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('passed','failed','needs_review')),
            input_fingerprint TEXT NOT NULL,
            evidence_json TEXT NOT NULL,
            model TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id)
        );
        CREATE TRIGGER IF NOT EXISTS doctrine_no_automatic_activation
            BEFORE UPDATE OF status ON doctrine_candidates
            WHEN NEW.status='active' AND OLD.status<>'approved'
            BEGIN SELECT RAISE(ABORT, 'doctrine activation requires prior approval'); END;
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
