use rusqlite::{Connection, params};
use serde_json::json;
use tempfile::tempdir;
use voiceos_core::ConversationStore;

#[test]
fn canonical_goal_task_job_artifact_and_event_records_are_owner_scoped() {
    let store = ConversationStore::in_memory().unwrap();
    let goal = store
        .create_goal("owner-1", "Build VoiceOS", "A dependable whole-home agent")
        .unwrap();
    let project = store
        .create_project("owner-1", Some(&goal.id), "Trusted agent kernel")
        .unwrap();
    let task = store
        .create_task(
            "owner-1",
            Some(&project.id),
            None,
            "Add execution records",
            "Schema and contract tests pass",
            20,
        )
        .unwrap();
    let job = store
        .create_job(
            "owner-1",
            Some(&task.id),
            "agent-kernel-schema-v1",
            json!(["project.tests"]),
        )
        .unwrap();
    let artifact = store
        .record_artifact(
            "owner-1",
            Some(&job.id),
            "test-report",
            "voiceos://artifacts/test-report-1",
            Some("abc123"),
        )
        .unwrap();
    let first = store
        .append_execution_event(
            "owner-1",
            &job.id,
            "job.proposed",
            "voiceos-core",
            json!({"capabilities": ["project.tests"]}),
        )
        .unwrap();
    let second = store
        .append_execution_event(
            "owner-1",
            &job.id,
            "artifact.recorded",
            "voiceos-core",
            json!({"artifact_id": artifact.id}),
        )
        .unwrap();
    let provider_run = store
        .record_provider_run(
            "owner-1",
            Some(&job.id),
            "ollama",
            "gemma",
            Some(120),
            Some(40),
            2_000,
            Some(0.0),
            "completed",
        )
        .unwrap();

    assert_eq!(20, task.estimated_minutes);
    assert_eq!("proposed", job.status);
    assert_eq!(Some(20.0), provider_run.output_tokens_per_second);
    assert!(first.id < second.id);
    let events = store.execution_events("owner-1", &job.id, 20).unwrap();
    assert_eq!(
        vec!["job.proposed", "artifact.recorded", "provider.run.recorded"],
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        store
            .create_project("another-owner", Some(&goal.id), "Cross-owner project")
            .is_err()
    );
}

#[test]
fn jobs_require_typed_capability_arrays_and_idempotency_keys() {
    let store = ConversationStore::in_memory().unwrap();
    assert!(
        store
            .create_job("owner", None, "job-1", json!({"shell": "anything"}))
            .is_err()
    );
    store
        .create_job("owner", None, "job-1", json!(["system.health"]))
        .unwrap();
    assert!(
        store
            .create_job("owner", None, "job-1", json!(["system.health"]))
            .is_err()
    );
}

#[test]
fn tasks_can_be_listed_and_status_changes_are_audited() {
    let store = ConversationStore::in_memory().unwrap();
    let ready = store
        .create_task(
            "owner",
            None,
            None,
            "Wire the widget",
            "Widget shows tasks",
            20,
        )
        .unwrap();
    let active = store
        .create_task(
            "owner",
            None,
            None,
            "Test the phone",
            "Pixel test passes",
            15,
        )
        .unwrap();
    store
        .update_task_status_as("owner", &active.id, "active", "device:pixel")
        .unwrap();

    let tasks = store.tasks("owner", false, 20).unwrap();
    assert_eq!(
        vec![active.id.as_str(), ready.id.as_str()],
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>()
    );
    let completed = store
        .update_task_status_as("owner", &active.id, "completed", "device:pixel")
        .unwrap()
        .unwrap();
    assert_eq!("completed", completed.status);
    assert_eq!(1, store.tasks("owner", false, 20).unwrap().len());
    assert_eq!(2, store.tasks("owner", true, 20).unwrap().len());
    let events = store.execution_events("owner", &active.id, 10).unwrap();
    assert_eq!(2, events.len());
    assert_eq!("device:pixel", events[1].actor);
    assert_eq!("completed", events[1].payload["to"]);
    assert!(
        store
            .update_task_status_as("other", &ready.id, "completed", "device:other")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .update_task_status_as("owner", &ready.id, "invalid", "device:pixel")
            .is_err()
    );
}

#[test]
fn repeated_successful_audit_workflows_become_inert_review_proposals() {
    let directory = tempdir().unwrap();
    let audit_path = directory.path().join("audit.sqlite3");
    let legacy = Connection::open(&audit_path).unwrap();
    legacy
        .execute_batch(
            "CREATE TABLE turns(
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool_requests_json TEXT NOT NULL,
                results_json TEXT NOT NULL,
                errors_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );",
        )
        .unwrap();
    for turn_id in 1..=2 {
        legacy
            .execute(
                "INSERT INTO turns VALUES(?1, 'session', ?2, ?3, '[]', '2026-08-02T00:00:00Z')",
                params![
                    turn_id,
                    json!([{"tool": "system.health", "arguments": {}}]).to_string(),
                    json!([{"status": "healthy"}]).to_string()
                ],
            )
            .unwrap();
    }
    legacy
        .execute(
            "INSERT INTO turns VALUES(3, 'session', ?1, '[]', ?2, '2026-08-02T00:00:00Z')",
            params![
                json!([{"tool": "system.health", "arguments": {}}]).to_string(),
                json!([{"error": "timeout"}]).to_string()
            ],
        )
        .unwrap();
    drop(legacy);

    let store = ConversationStore::in_memory().unwrap();
    let proposals = store
        .propose_skills_from_legacy_audit(&audit_path, "owner", 2)
        .unwrap();
    assert_eq!(1, proposals.len());
    let proposal = &proposals[0];
    assert_eq!("proposed", proposal.status);
    assert_eq!(2, proposal.evidence.as_array().unwrap().len());
    assert!(
        proposal
            .content
            .contains("typed `system.health` capability")
    );
    assert!(proposal.content.contains("Never execute shell text"));
    assert_eq!(
        vec![proposal.clone()],
        store
            .skill_proposals("owner", Some("proposed"), 20)
            .unwrap()
    );
    assert!(
        store
            .skill_proposals("another-owner", Some("proposed"), 20)
            .unwrap()
            .is_empty()
    );
    let events = store.execution_events("owner", &proposal.id, 10).unwrap();
    assert_eq!("skill.proposed", events[0].event_type);
    assert_eq!(
        Some(false),
        events[0].payload["execution_enabled"].as_bool()
    );
    assert!(
        store
            .propose_skills_from_legacy_audit(&audit_path, "owner", 2)
            .unwrap()
            .is_empty()
    );

    assert!(
        store
            .create_automation_proposal("owner", &proposal.id, json!({"schedule": "daily"}))
            .is_err()
    );
    let approved = store
        .decide_skill_proposal_as("owner", &proposal.id, true, "pixel-device")
        .unwrap()
        .unwrap();
    assert_eq!("approved", approved.status);
    assert!(
        store
            .decide_skill_proposal_as("owner", &proposal.id, false, "web-device")
            .unwrap()
            .is_none()
    );
    let events = store.execution_events("owner", &proposal.id, 10).unwrap();
    assert_eq!(
        vec!["skill.proposed", "skill.decided"],
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!("pixel-device", events[1].actor);
    assert_eq!("approved", events[1].payload["decision"]);
    let automation = store
        .create_automation_proposal("owner", &proposal.id, json!({"schedule": "daily"}))
        .unwrap();
    assert_eq!("proposed", automation.status);
}

#[test]
fn task_detail_projects_owned_steps_blockers_handoffs_artifacts_and_activity() {
    let store = ConversationStore::in_memory().unwrap();
    let task = store
        .create_task(
            "owner",
            None,
            None,
            "Print recipe cards",
            "Cards are printed and laminated",
            40,
        )
        .unwrap();
    let vic_step = store
        .create_task_step(
            "owner",
            &task.id,
            "Prepare print layout",
            "vic",
            "provider:vic",
        )
        .unwrap();
    store
        .update_task_step(
            "owner",
            &task.id,
            &vic_step.id,
            "completed",
            None,
            json!({"file": "recipe-cards.pdf"}),
            "provider:vic",
        )
        .unwrap();
    store
        .create_task_step(
            "owner",
            &task.id,
            "Confirm card size",
            "user",
            "provider:vic",
        )
        .unwrap();
    let blocker = store
        .create_task_blocker(
            "owner",
            &task.id,
            "Card size is unknown",
            "user",
            "provider:vic",
        )
        .unwrap();
    store
        .attach_task_artifact(
            "owner",
            &task.id,
            "pdf",
            "voiceos://artifacts/recipe-cards.pdf",
            "Draft print layout",
            "vic",
            "provider:vic",
        )
        .unwrap();
    store
        .create_task_handoff(
            "owner",
            &task.id,
            "vic",
            "user",
            "review",
            "Review the card layout and confirm dimensions",
            "provider:vic",
        )
        .unwrap();

    let detail = store.task_detail("owner", &task.id).unwrap().unwrap();
    assert_eq!(detail.progress.completed_steps, 1);
    assert_eq!(detail.progress.total_steps, 2);
    assert_eq!(detail.progress.open_blockers, 1);
    assert_eq!(detail.progress.lane, "review");
    assert_eq!(
        detail.progress.next_user_action.as_deref(),
        Some("Confirm card size")
    );
    assert_eq!(detail.artifacts.len(), 1);
    assert!(detail.activity.len() >= 6);

    store
        .resolve_task_blocker("owner", &task.id, &blocker.id, "device:pixel")
        .unwrap();
    assert_eq!(
        store
            .task_detail("owner", &task.id)
            .unwrap()
            .unwrap()
            .progress
            .open_blockers,
        0
    );
}

#[test]
fn vic_outreach_is_durable_deduplicated_and_actionable() {
    let store = ConversationStore::in_memory().unwrap();
    let actions = vec!["talk_now".to_owned(), "show_progress".to_owned(), "later".to_owned()];
    let first = store.create_outreach(
        "owner", "status_update", "check_in", "VIC wants to talk",
        "I finished the first useful step.", "Task progress changed", None, None,
        Some("task-1-progress"), &actions, None,
    ).unwrap();
    let duplicate = store.create_outreach(
        "owner", "status_update", "check_in", "Duplicate",
        "This should resolve to the active event.", "Same reason", None, None,
        Some("task-1-progress"), &actions, None,
    ).unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(store.outreaches("owner", false, 20).unwrap().len(), 1);

    let delivered = store.act_on_outreach("owner", &first.id, "delivered", None).unwrap().unwrap();
    assert_eq!(delivered.status, "delivered");
    assert!(delivered.delivered_at.is_some());
    let snoozed = store.act_on_outreach("owner", &first.id, "later", Some(20)).unwrap().unwrap();
    assert_eq!(snoozed.status, "snoozed");
    assert!(snoozed.snoozed_until.is_some());

    let policy = store.outreach_policy("owner").unwrap();
    assert!(policy.enabled);
    assert_eq!(policy.max_checkins_per_day, 6);
}
