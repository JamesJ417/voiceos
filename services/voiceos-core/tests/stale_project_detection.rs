use chrono::{DateTime, Duration};
use rusqlite::{Connection, params};
use serde_json::json;
use tempfile::tempdir;
use voiceos_core::ConversationStore;

const AS_OF: &str = "2026-07-10T12:00:00Z";
const STALE_AFTER_SECONDS: i64 = 7 * 24 * 60 * 60;

fn set_project_timestamp(connection: &Connection, project_id: &str, timestamp: &str) {
    connection
        .execute(
            "UPDATE projects SET created_at=?2, updated_at=?2 WHERE project_id=?1",
            params![project_id, timestamp],
        )
        .unwrap();
}

fn set_task_timestamp(connection: &Connection, task_id: &str, timestamp: &str) {
    connection
        .execute(
            "UPDATE tasks SET created_at=?2, updated_at=?2 WHERE task_id=?1",
            params![task_id, timestamp],
        )
        .unwrap();
}

#[test]
fn stale_project_detection_is_owner_scoped_deterministic_and_side_effect_free() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("voiceos.db");
    let store = ConversationStore::open(&database_path).unwrap();
    let stale = store
        .create_project("owner-a", None, "Stale project")
        .unwrap();
    let recent = store
        .create_project("owner-a", None, "Recent project")
        .unwrap();
    let other_owner = store
        .create_project("owner-b", None, "Private stale project")
        .unwrap();
    let completed = store
        .create_project("owner-a", None, "Completed project")
        .unwrap();
    let archived = store
        .create_project("owner-a", None, "Archived project")
        .unwrap();
    let stale_task = store
        .create_task(
            "owner-a",
            Some(&stale.id),
            None,
            "Old task",
            "Old task is complete",
            15,
        )
        .unwrap();
    let recent_event = store
        .append_execution_event(
            "owner-a",
            &recent.id,
            "project.progressed",
            "test",
            json!({"transcript_body": "must not enter evidence"}),
        )
        .unwrap();

    let connection = Connection::open(&database_path).unwrap();
    set_project_timestamp(&connection, &stale.id, "2026-06-20T12:00:00Z");
    set_task_timestamp(&connection, &stale_task.id, "2026-06-21T12:00:00Z");
    set_project_timestamp(&connection, &other_owner.id, "2026-06-20T12:00:00Z");
    set_project_timestamp(&connection, &completed.id, "2026-06-20T12:00:00Z");
    set_project_timestamp(&connection, &archived.id, "2026-06-20T12:00:00Z");
    connection
        .execute(
            "UPDATE projects SET status='completed' WHERE project_id=?1",
            [&completed.id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE projects SET status='archived' WHERE project_id=?1",
            [&archived.id],
        )
        .unwrap();
    drop(connection);

    let tasks_before = store.tasks("owner-a", true, 20).unwrap();
    let memories_before = store.memories_for_owner("owner-a", 20).unwrap();
    let outreaches_before = store.outreaches("owner-a", true, 20).unwrap();

    let as_of = recent_event.occurred_at.as_str();
    let first = store
        .detect_stale_project_candidates("owner-a", as_of, STALE_AFTER_SECONDS)
        .unwrap();
    let retry = store
        .detect_stale_project_candidates("owner-a", as_of, STALE_AFTER_SECONDS)
        .unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first, retry);
    assert_eq!(first[0].project_id.as_deref(), Some(stale.id.as_str()));
    assert!(
        DateTime::parse_from_rfc3339(&first[0].expires_at).unwrap()
            > DateTime::parse_from_rfc3339(as_of).unwrap()
    );
    assert_eq!(
        first[0].evidence,
        json!([{
            "source_type": "task",
            "source_id": stale_task.id,
            "occurred_at": "2026-06-21T12:00:00Z"
        }])
    );
    assert!(!first[0].evidence.to_string().contains("transcript_body"));
    assert_eq!(store.proactive_candidates("owner-a", 20).unwrap(), first);

    let after_expiry = DateTime::parse_from_rfc3339(as_of)
        .unwrap()
        .checked_add_signed(Duration::seconds(STALE_AFTER_SECONDS + 1))
        .unwrap()
        .to_rfc3339();
    let refreshed = store
        .detect_stale_project_candidates("owner-a", &after_expiry, STALE_AFTER_SECONDS)
        .unwrap();
    let refreshed_stale = refreshed
        .iter()
        .find(|candidate| candidate.project_id.as_deref() == Some(stale.id.as_str()))
        .unwrap();
    assert_ne!(refreshed_stale.id, first[0].id);
    assert!(
        DateTime::parse_from_rfc3339(&refreshed_stale.expires_at).unwrap()
            > DateTime::parse_from_rfc3339(&after_expiry).unwrap()
    );

    let event_as_of = DateTime::parse_from_rfc3339(as_of)
        .unwrap()
        .checked_add_signed(Duration::days(8))
        .unwrap()
        .to_rfc3339();
    let event_candidates = store
        .detect_stale_project_candidates("owner-a", &event_as_of, STALE_AFTER_SECONDS)
        .unwrap();
    let event_candidate = event_candidates
        .iter()
        .find(|candidate| candidate.project_id.as_deref() == Some(recent.id.as_str()))
        .unwrap();
    assert_eq!(
        event_candidate.evidence,
        json!([{
            "source_type": "execution_event",
            "source_id": recent_event.id.to_string(),
            "occurred_at": recent_event.occurred_at,
        }])
    );
    assert!(
        !event_candidate
            .evidence
            .to_string()
            .contains("transcript_body")
    );

    assert!(
        store
            .proactive_candidates("owner-b", 20)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.tasks("owner-a", true, 20).unwrap(), tasks_before);
    assert_eq!(
        store.memories_for_owner("owner-a", 20).unwrap(),
        memories_before
    );
    assert_eq!(
        store.outreaches("owner-a", true, 20).unwrap(),
        outreaches_before
    );
}

#[test]
fn stale_project_detection_rejects_non_positive_intervals_and_invalid_as_of_time() {
    let store = ConversationStore::in_memory().unwrap();

    assert!(
        store
            .detect_stale_project_candidates("owner-a", AS_OF, 0)
            .is_err()
    );
    assert!(
        store
            .detect_stale_project_candidates("owner-a", AS_OF, -1)
            .is_err()
    );
    assert!(
        store
            .detect_stale_project_candidates("owner-a", "not-a-time", STALE_AFTER_SECONDS)
            .is_err()
    );
}
