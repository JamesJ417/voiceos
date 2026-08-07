use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

struct Migration {
    version: i64,
    name: &'static str,
    checksum: &'static str,
    apply: fn(&Connection) -> rusqlite::Result<()>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "voiceos_authority_baseline",
        checksum: "voiceos-authority-baseline-v1-2026-08-06",
        apply: crate::schema::apply_current_schema,
    },
    Migration {
        version: 2,
        name: "codex_agent_runs",
        checksum: "codex-agent-runs-v2-2026-08-06",
        apply: apply_codex_agent_runs,
    },
];

fn apply_codex_agent_runs(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_runs (
            run_id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            task_id TEXT,
            parent_run_id TEXT,
            idempotency_key TEXT NOT NULL,
            role TEXT NOT NULL,
            objective TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('queued','starting','running','waiting_approval','waiting_input','completed','failed','cancelled')),
            provider TEXT NOT NULL DEFAULT 'codex',
            model TEXT NOT NULL,
            reasoning_effort TEXT NOT NULL,
            sandbox TEXT NOT NULL CHECK(sandbox IN ('read-only','workspace-write')),
            capability_scope_json TEXT NOT NULL,
            codex_thread_id TEXT,
            current_activity TEXT,
            result_summary TEXT,
            error TEXT,
            requested_by TEXT NOT NULL,
            created_at TEXT NOT NULL,
            started_at TEXT,
            updated_at TEXT NOT NULL,
            completed_at TEXT,
            FOREIGN KEY(owner_id) REFERENCES owners(owner_id),
            FOREIGN KEY(task_id) REFERENCES tasks(task_id),
            FOREIGN KEY(parent_run_id) REFERENCES agent_runs(run_id),
            UNIQUE(owner_id,idempotency_key)
        );
        CREATE INDEX IF NOT EXISTS agent_runs_owner_status_idx
            ON agent_runs(owner_id,status,updated_at DESC);
        CREATE INDEX IF NOT EXISTS agent_runs_task_idx
            ON agent_runs(owner_id,task_id,updated_at DESC);
        CREATE INDEX IF NOT EXISTS agent_runs_parent_idx
            ON agent_runs(owner_id,parent_run_id,created_at);",
    )
}

pub(crate) fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    run_migrations(connection, MIGRATIONS)
}

fn run_migrations(connection: &Connection, migrations: &[Migration]) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            checksum TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;

    let mut previous = 0i64;
    for migration in migrations {
        if migration.version <= previous {
            return Err(rusqlite::Error::InvalidQuery);
        }
        previous = migration.version;
        let applied: Option<(String, String)> = connection
            .query_row(
                "SELECT name,checksum FROM schema_migrations WHERE version=?1",
                [migration.version],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((name, checksum)) = applied {
            if name != migration.name || checksum != migration.checksum {
                return Err(rusqlite::Error::InvalidQuery);
            }
            continue;
        }

        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (migration.apply)(connection).and_then(|_| {
            connection.execute(
                "INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(?1,?2,?3,?4)",
                params![
                    migration.version,
                    migration.name,
                    migration.checksum,
                    Utc::now().to_rfc3339()
                ],
            )?;
            Ok(())
        });
        match result {
            Ok(()) => connection.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_exists(connection: &Connection, kind: &str, name: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type=?1 AND name=?2)",
                params![kind, name],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn fresh_database_applies_baseline_once() {
        let connection = Connection::open_in_memory().unwrap();
        migrate(&connection).unwrap();
        assert!(object_exists(&connection, "table", "conversations"));
        assert!(object_exists(&connection, "table", "sleep_cycles"));
        assert!(object_exists(&connection, "table", "doctrine_candidates"));
        let applied: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(applied, 2);

        migrate(&connection).unwrap();
        let still_applied: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(still_applied, 2);
    }

    #[test]
    fn migration_ledger_survives_database_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("voiceos.sqlite3");
        {
            let connection = Connection::open(&path).unwrap();
            migrate(&connection).unwrap();
        }
        let reopened = Connection::open(&path).unwrap();
        migrate(&reopened).unwrap();
        let applied: i64 = reopened
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(applied, 2);
        assert!(object_exists(&reopened, "table", "doctrine_candidates"));
        assert!(object_exists(&reopened, "table", "agent_runs"));
    }

    #[test]
    fn legacy_database_is_upgraded_without_losing_rows() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE devices (
                    device_id TEXT PRIMARY KEY,
                    display_name TEXT,
                    created_at TEXT NOT NULL,
                    last_seen_at TEXT NOT NULL
                );
                INSERT INTO devices(device_id,display_name,created_at,last_seen_at)
                VALUES('legacy-device','Legacy','2026-01-01','2026-01-01');",
            )
            .unwrap();
        migrate(&connection).unwrap();
        let display_name: String = connection
            .query_row(
                "SELECT display_name FROM devices WHERE device_id='legacy-device'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(display_name, "Legacy");
        assert!(object_exists(&connection, "table", "owner_devices"));
    }

    fn fail_after_writing(connection: &Connection) -> rusqlite::Result<()> {
        connection.execute_batch("CREATE TABLE partial_migration(value TEXT);")?;
        Err(rusqlite::Error::InvalidQuery)
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_ledger_entry() {
        let connection = Connection::open_in_memory().unwrap();
        let failing = [Migration {
            version: 99,
            name: "forced_failure",
            checksum: "forced-failure-v1",
            apply: fail_after_writing,
        }];
        assert!(run_migrations(&connection, &failing).is_err());
        assert!(!object_exists(&connection, "table", "partial_migration"));
        let applied: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(applied, 0);
    }

    #[test]
    fn checksum_drift_fails_closed() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    checksum TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations VALUES(1,'voiceos_authority_baseline','tampered','now');",
            )
            .unwrap();
        assert!(migrate(&connection).is_err());
    }
}
