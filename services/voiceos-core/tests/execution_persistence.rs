use serde_json::json;
use voiceos_core::ConversationStore;

fn approved_job(store: &ConversationStore, owner: &str) -> String {
    let job = store
        .create_job(
            owner,
            None,
            &format!("job-{owner}"),
            json!(["filesystem.read"]),
        )
        .unwrap();
    store
        .transition_job_status(owner, &job.id, "proposed", "approved")
        .unwrap()
        .unwrap();
    job.id
}

#[test]
fn lease_is_owner_scoped_bounded_and_limited_to_executable_states() {
    let store = ConversationStore::in_memory().unwrap();
    let job_id = approved_job(&store, "owner-a");
    store
        .create_job("owner-b", None, "job-owner-b", json!([]))
        .unwrap();

    assert!(
        store
            .acquire_capability_lease("owner-a", &job_id, json!(["filesystem.read"]), 60)
            .is_ok()
    );
    assert!(
        store
            .acquire_capability_lease("owner-a", &job_id, json!([]), 3_601)
            .is_err()
    );
    assert!(
        store
            .acquire_capability_lease("owner-b", &job_id, json!([]), 60)
            .is_err()
    );
    assert!(
        store
            .acquire_capability_lease("owner-a", &job_id, json!({}), 60)
            .is_err()
    );
    assert!(
        store
            .transition_job_status("owner-a", &job_id, "approved", "completed")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .acquire_capability_lease("owner-a", &job_id, json!([]), 60)
            .is_err()
    );
}

#[test]
fn checkpoints_are_monotonic_persist_rollback_and_cannot_cross_owners() {
    let store = ConversationStore::in_memory().unwrap();
    let job_id = approved_job(&store, "owner-a");
    store
        .create_job("owner-b", None, "job-owner-b", json!([]))
        .unwrap();

    let first = store
        .checkpoint_execution(
            "owner-a",
            &job_id,
            json!({"cursor": 1}),
            json!({"undo": "step-1"}),
        )
        .unwrap();
    let second = store
        .checkpoint_execution(
            "owner-a",
            &job_id,
            json!({"cursor": 2}),
            json!({"undo": "step-2"}),
        )
        .unwrap();
    assert_eq!((first.sequence, second.sequence), (1, 2));
    store
        .transition_job_status("owner-a", &job_id, "approved", "running")
        .unwrap()
        .unwrap();
    assert_eq!(
        store.resume_execution("owner-a", &job_id).unwrap().unwrap(),
        second
    );
    assert!(
        store
            .checkpoint_execution("owner-b", &job_id, json!({"cursor": 3}), json!({}))
            .is_err()
    );
    assert_eq!(
        store.resume_execution("owner-a", &job_id).unwrap().unwrap(),
        second
    );
}

#[test]
fn cancellation_is_durable_idempotent_and_blocks_resume() {
    let store = ConversationStore::in_memory().unwrap();
    let job_id = approved_job(&store, "owner-a");
    store
        .transition_job_status("owner-a", &job_id, "approved", "running")
        .unwrap()
        .unwrap();
    store
        .checkpoint_execution("owner-a", &job_id, json!({"cursor": 1}), json!({"undo": 1}))
        .unwrap();

    assert!(
        store
            .cancel_execution("owner-a", &job_id, "user_requested")
            .unwrap()
    );
    assert!(
        store
            .cancel_execution("owner-a", &job_id, "different_reason")
            .unwrap()
    );
    assert_eq!(
        store.job("owner-a", &job_id).unwrap().unwrap().status,
        "cancelled"
    );
    let connection = store.connection().unwrap();
    let reason: String = connection
        .query_row(
            "SELECT cancellation_reason FROM jobs WHERE owner_id=?1 AND job_id=?2",
            ["owner-a", job_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reason, "user_requested");
    drop(connection);
    assert!(store.resume_execution("owner-a", &job_id).is_err());
}

#[test]
fn resume_returns_latest_checkpoint_only_from_paused_or_running_job() {
    let store = ConversationStore::in_memory().unwrap();
    let job_id = approved_job(&store, "owner-a");
    assert!(store.resume_execution("owner-a", &job_id).is_err());
    store
        .transition_job_status("owner-a", &job_id, "approved", "running")
        .unwrap()
        .unwrap();
    let checkpoint = store
        .checkpoint_execution("owner-a", &job_id, json!({"cursor": 1}), json!({}))
        .unwrap();
    store
        .transition_job_status("owner-a", &job_id, "running", "paused")
        .unwrap()
        .unwrap();
    assert_eq!(
        store.resume_execution("owner-a", &job_id).unwrap().unwrap(),
        checkpoint
    );
    assert_eq!(
        store.job("owner-a", &job_id).unwrap().unwrap().status,
        "running"
    );
}
