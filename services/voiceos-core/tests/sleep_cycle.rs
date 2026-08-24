use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;
use voiceos_core::{ConversationStore, LiveMemoryChange, Role};

#[test]
fn dry_run_sleep_cycle_is_owner_scoped_idempotent_and_leaves_memories_untouched() {
    let store = ConversationStore::in_memory().unwrap();
    store
        .resolve_owner_conversation("owner-a", "pixel-a", None)
        .unwrap();
    store
        .remember_for_owner("owner-a", "pixel-a", "keep this memory", "test")
        .unwrap();
    let memories_before = store.memories_for_owner("owner-a", 10).unwrap();

    let first = store
        .create_dry_run_sleep_cycle("owner-a", "good-night-2026-08-23")
        .unwrap();
    let retry = store
        .create_dry_run_sleep_cycle("owner-a", "good-night-2026-08-23")
        .unwrap();
    let other_owner = store
        .create_dry_run_sleep_cycle("owner-b", "good-night-2026-08-23")
        .unwrap();

    assert_eq!(first.id, retry.id);
    assert_eq!(first.owner_id, "owner-a");
    assert_eq!(first.idempotency_key, "good-night-2026-08-23");
    assert!(first.dry_run);
    assert_eq!(first.status, "completed");
    assert_eq!(first.memories_before, 1);
    assert_eq!(first.memories_after, 1);
    assert_eq!(first.committed_changes, 0);
    assert_ne!(first.id, other_owner.id);
    assert_eq!(
        memories_before,
        store.memories_for_owner("owner-a", 10).unwrap()
    );
}

#[test]
fn reports_only_new_evidence_and_links_each_night_to_the_previous_cycle() {
    let store = ConversationStore::in_memory().unwrap();
    let conversation = store
        .resolve_owner_conversation("owner-a", "pixel-a", None)
        .unwrap();
    store
        .append_message(&conversation, Role::User, "first day", None)
        .unwrap();
    store
        .append_execution_event(
            "owner-a",
            "task-a",
            "task.changed",
            "vic",
            json!({"day": 1}),
        )
        .unwrap();

    let first = store
        .create_dry_run_sleep_cycle("owner-a", "2026-08-23")
        .unwrap();
    assert_eq!(first.messages_inspected, 1);
    assert_eq!(first.events_inspected, 1);

    store
        .append_message(&conversation, Role::Assistant, "second day", Some("test"))
        .unwrap();
    let second = store
        .create_dry_run_sleep_cycle("owner-a", "2026-08-24")
        .unwrap();
    assert_eq!(second.previous_cycle_id.as_deref(), Some(first.id.as_str()));
    assert_eq!(second.messages_inspected, 1);
    assert_eq!(second.events_inspected, 0);

    let reports = store.sleep_cycle_reports("owner-a", 30).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].cycle.id, second.id);
    assert_eq!(reports[0].new_evidence_count, 1);
    assert_eq!(reports[0].durable_memory_delta, 0);
    assert!(reports[0].changes.is_empty());
}

#[test]
fn report_lookup_does_not_leak_across_owners() {
    let store = ConversationStore::in_memory().unwrap();
    let cycle = store
        .create_dry_run_sleep_cycle("owner-a", "2026-08-23")
        .unwrap();
    assert!(
        store
            .sleep_cycle_report("owner-b", &cycle.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn live_sleep_cycle_commits_explicit_additions_once_and_preserves_other_owner_memories() {
    let store = ConversationStore::in_memory().unwrap();
    store
        .resolve_owner_conversation("owner-a", "device-a", None)
        .unwrap();
    store
        .resolve_owner_conversation("owner-b", "device-b", None)
        .unwrap();
    store
        .remember_for_owner("owner-b", "device-b", "private memory", "test")
        .unwrap();

    let first = store
        .commit_live_sleep_cycle(
            "owner-a",
            "controlled-live-test",
            "device-a",
            &[LiveMemoryChange::add(
                "durable test memory",
                "controlled-test",
            )],
        )
        .unwrap();
    let retry = store
        .commit_live_sleep_cycle(
            "owner-a",
            "controlled-live-test",
            "device-a",
            &[LiveMemoryChange::add(
                "different retry input",
                "controlled-test",
            )],
        )
        .unwrap();

    assert!(!first.dry_run);
    assert_eq!(first.mode, "commit");
    assert_eq!(first.committed_changes, 1);
    assert_eq!(first.memories_before, 0);
    assert_eq!(first.memories_after, 1);
    assert_eq!(first.id, retry.id);
    assert_eq!(
        store.memories_for_owner("owner-a", 10).unwrap()[0].content,
        "durable test memory"
    );
    assert_eq!(
        store.memories_for_owner("owner-b", 10).unwrap()[0].content,
        "private memory"
    );
    let report = store
        .sleep_cycle_report("owner-a", &first.id)
        .unwrap()
        .unwrap();
    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].operation, "add");
    assert_eq!(report.changes[0].status, "committed");
}

#[test]
fn opening_a_legacy_dry_run_database_migrates_the_mode_constraint_for_commit_cycles() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("legacy.db");
    Connection::open(&database_path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE sleep_cycles (
                sleep_cycle_id TEXT PRIMARY KEY,
                owner_id TEXT NOT NULL,
                idempotency_key TEXT NOT NULL,
                mode TEXT NOT NULL DEFAULT 'dry_run' CHECK(mode = 'dry_run'),
                status TEXT NOT NULL DEFAULT 'completed',
                created_at TEXT NOT NULL,
                UNIQUE(owner_id, idempotency_key)
            );",
        )
        .unwrap();

    let store = ConversationStore::open(&database_path).unwrap();
    let conversation = store
        .resolve_owner_conversation("owner-a", "device-a", None)
        .unwrap();
    store
        .append_message(
            &conversation,
            Role::User,
            "Remember that I prefer a 7 AM report.",
            None,
        )
        .unwrap();

    let cycle = store
        .create_commit_sleep_cycle("owner-a", "legacy-commit-test")
        .unwrap();
    assert_eq!(cycle.mode, "commit");
}

#[test]
fn a_forgotten_memory_can_be_added_again_as_a_new_active_memory() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("memory-lifecycle.db");
    {
        let store = ConversationStore::open(&database_path).unwrap();
        store
            .resolve_owner_conversation("owner-a", "device-a", None)
            .unwrap();
        store
            .remember_for_owner("owner-a", "device-a", "preferred color is blue", "test")
            .unwrap();
    }

    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "UPDATE memories SET status = 'forgotten' WHERE owner_id = 'owner-a'",
            [],
        )
        .unwrap();
    drop(connection);

    let store = ConversationStore::open(&database_path).unwrap();
    store
        .remember_for_owner("owner-a", "device-a", "preferred color is blue", "test")
        .unwrap();
    let active = store.memories_for_owner("owner-a", 10).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].content, "preferred color is blue");
    drop(store);

    let connection = Connection::open(&database_path).unwrap();
    let (total, active): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END) FROM memories WHERE owner_id = 'owner-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((total, active), (2, 1));
}

#[test]
fn dry_run_proposals_survive_the_scan_watermark_and_commit_only_after_selection() {
    let store = ConversationStore::in_memory().unwrap();
    let conversation = store
        .resolve_owner_conversation("owner-a", "device-a", None)
        .unwrap();
    store
        .append_message(
            &conversation,
            Role::User,
            "Remember that my preferred report time is 7 AM.",
            None,
        )
        .unwrap();

    let scan = store
        .create_dry_run_sleep_cycle("owner-a", "scan-1")
        .unwrap();
    assert!(scan.dry_run);
    assert_eq!(scan.proposed_changes, 1);
    assert!(store.memories_for_owner("owner-a", 10).unwrap().is_empty());
    let proposal = store
        .sleep_cycle_report("owner-a", &scan.id)
        .unwrap()
        .unwrap()
        .changes[0]
        .id
        .clone();

    let committed = store
        .commit_sleep_cycle_proposals(
            "owner-a",
            &scan.id,
            "commit-1",
            "device-a",
            std::slice::from_ref(&proposal),
        )
        .unwrap();
    assert_eq!(committed.mode, "commit");
    assert_eq!(committed.committed_changes, 1);
    assert_eq!(
        store.memories_for_owner("owner-a", 10).unwrap()[0].content,
        "my preferred report time is 7 AM"
    );

    let retry = store
        .commit_sleep_cycle_proposals("owner-a", &scan.id, "commit-1", "device-a", &[proposal])
        .unwrap();
    assert_eq!(retry.id, committed.id);
}

#[test]
fn commit_idempotency_key_rejects_a_different_proposal_set() {
    let store = ConversationStore::in_memory().unwrap();
    let conversation = store
        .resolve_owner_conversation("owner-a", "device-a", None)
        .unwrap();
    store
        .append_message(
            &conversation,
            Role::User,
            "Remember that alpha is enabled.",
            None,
        )
        .unwrap();
    store
        .append_message(
            &conversation,
            Role::User,
            "Remember that beta is enabled.",
            None,
        )
        .unwrap();
    let scan = store
        .create_dry_run_sleep_cycle("owner-a", "scan-2")
        .unwrap();
    let changes = store
        .sleep_cycle_report("owner-a", &scan.id)
        .unwrap()
        .unwrap()
        .changes;
    store
        .commit_sleep_cycle_proposals(
            "owner-a",
            &scan.id,
            "commit-2",
            "device-a",
            &[changes[0].id.clone()],
        )
        .unwrap();
    let error = store
        .commit_sleep_cycle_proposals(
            "owner-a",
            &scan.id,
            "commit-2",
            "device-a",
            &[changes[1].id.clone()],
        )
        .unwrap_err();
    assert!(error.to_string().contains("different proposals"));
}
