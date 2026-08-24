use serde_json::json;
use voiceos_core::{ConversationStore, Role};

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
