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
fn projects_are_owner_scoped_and_tasks_can_be_reorganized() {
    let store = ConversationStore::in_memory().unwrap();
    let first = store
        .create_project("owner", None, "VIC touch panel")
        .unwrap();
    let second = store.create_project("owner", None, "SMB Sentinel").unwrap();
    store
        .create_project("another-owner", None, "Private project")
        .unwrap();
    let task = store
        .create_task(
            "owner",
            None,
            None,
            "Group current work",
            "Every task has an intentional home",
            20,
        )
        .unwrap();

    let projects = store.projects("owner", 20).unwrap();
    assert_eq!(2, projects.len());
    assert!(projects.iter().all(|project| project.owner_id == "owner"));

    let assigned = store
        .assign_task_project_as("owner", &task.id, Some(&first.id), "device:panel")
        .unwrap()
        .unwrap();
    assert_eq!(Some(first.id.as_str()), assigned.project_id.as_deref());
    let moved = store
        .assign_task_project_as("owner", &task.id, Some(&second.id), "device:panel")
        .unwrap()
        .unwrap();
    assert_eq!(Some(second.id.as_str()), moved.project_id.as_deref());
    let unassigned = store
        .assign_task_project_as("owner", &task.id, None, "device:panel")
        .unwrap()
        .unwrap();
    assert_eq!(None, unassigned.project_id);

    let private_project = store.projects("another-owner", 20).unwrap().remove(0);
    assert!(
        store
            .assign_task_project_as("owner", &task.id, Some(&private_project.id), "device:panel",)
            .is_err()
    );
    assert!(
        store
            .assign_task_project_as("another-owner", &task.id, None, "device:other")
            .unwrap()
            .is_none()
    );
    let events = store.execution_events("owner", &task.id, 10).unwrap();
    assert_eq!(3, events.len());
    assert_eq!("task.project_changed", events[0].event_type);
    assert_eq!(Some(first.id.as_str()), events[0].payload["to"].as_str());
    assert!(events[2].payload["to"].is_null());
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
    let usages = store
        .record_matching_skill_usages(
            "owner",
            Some("conversation"),
            Some("request"),
            &json!([{"name": "system.health", "arguments": {}}]),
            &json!({"results": [{"status": "healthy"}], "errors": []}),
            "completed",
        )
        .unwrap();
    assert_eq!(1, usages.len());
    assert_eq!(proposal.id, usages[0].skill_id);
    let reviewed = store
        .review_skill_usage_as(
            "owner",
            &usages[0].id,
            "correct",
            Some("The health summary matched the evidence."),
            "device:pixel",
        )
        .unwrap()
        .unwrap();
    assert_eq!(Some("correct"), reviewed.feedback.as_deref());
    assert_eq!(1, store.skill_usages("owner", 20).unwrap().len());
    let automation = store
        .create_automation_proposal("owner", &proposal.id, json!({"schedule": "daily"}))
        .unwrap();
    assert_eq!("proposed", automation.status);
    let disabled = store
        .set_skill_status_as("owner", &proposal.id, "disabled", "device:pixel")
        .unwrap()
        .unwrap();
    assert_eq!("disabled", disabled.status);
    assert!(
        store
            .record_matching_skill_usages(
                "owner",
                None,
                None,
                &json!([{"name": "system.health"}]),
                &json!({}),
                "completed",
            )
            .unwrap()
            .is_empty()
    );
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
fn advancing_stages_closes_and_creates_owner_handoffs_until_task_completion() {
    let store = ConversationStore::in_memory().unwrap();
    let task = store
        .create_task(
            "owner",
            None,
            None,
            "Ship the handoff loop",
            "Every stage moves cleanly between VIC and the user",
            30,
        )
        .unwrap();
    let plan = store
        .create_task_step("owner", &task.id, "Plan", "vic", "provider:vic")
        .unwrap();
    let review = store
        .create_task_step("owner", &task.id, "Review", "user", "provider:vic")
        .unwrap();
    let finish = store
        .create_task_step("owner", &task.id, "Finish", "vic", "provider:vic")
        .unwrap();

    let after_plan = store
        .advance_task_step(
            "owner",
            &task.id,
            &plan.id,
            "Plan is ready",
            json!({"source": "test"}),
            "provider:vic",
        )
        .unwrap()
        .unwrap();
    assert_eq!(after_plan.steps[0].status, "completed");
    assert_eq!(after_plan.steps[1].status, "active");
    let user_handoff = after_plan
        .handoffs
        .iter()
        .find(|handoff| handoff.to_owner == "user" && handoff.status == "pending")
        .unwrap();
    assert!(
        store
            .advance_task_step(
                "owner",
                &task.id,
                &review.id,
                "Review approved too early",
                json!({"source": "test"}),
                "device:pixel",
            )
            .is_err()
    );
    store
        .update_task_handoff(
            "owner",
            &task.id,
            &user_handoff.id,
            "accepted",
            "device:pixel",
        )
        .unwrap()
        .unwrap();

    let after_review = store
        .advance_task_step(
            "owner",
            &task.id,
            &review.id,
            "Review approved",
            json!({"source": "test"}),
            "device:pixel",
        )
        .unwrap()
        .unwrap();
    assert_eq!(after_review.steps[2].status, "active");
    assert!(after_review.handoffs.iter().any(|handoff| {
        handoff.from_owner == "user" && handoff.to_owner == "vic" && handoff.status == "pending"
    }));
    let vic_handoff = after_review
        .handoffs
        .iter()
        .find(|handoff| handoff.to_owner == "vic" && handoff.status == "pending")
        .unwrap();
    store
        .update_task_handoff(
            "owner",
            &task.id,
            &vic_handoff.id,
            "accepted",
            "provider:vic",
        )
        .unwrap()
        .unwrap();

    let completed = store
        .advance_task_step(
            "owner",
            &task.id,
            &finish.id,
            "Finished",
            json!({"source": "test"}),
            "provider:vic",
        )
        .unwrap()
        .unwrap();
    assert_eq!(completed.task.status, "completed");
    assert_eq!(completed.progress.completed_steps, 3);
    assert!(
        completed
            .handoffs
            .iter()
            .all(|handoff| handoff.status == "completed")
    );
}

#[test]
fn vic_outreach_is_durable_deduplicated_and_actionable() {
    let store = ConversationStore::in_memory().unwrap();
    let actions = vec![
        "talk_now".to_owned(),
        "show_progress".to_owned(),
        "later".to_owned(),
    ];
    let first = store
        .create_outreach(
            "owner",
            "status_update",
            "check_in",
            "VIC wants to talk",
            "I finished the first useful step.",
            "Task progress changed",
            None,
            None,
            Some("task-1-progress"),
            &actions,
            None,
        )
        .unwrap();
    let duplicate = store
        .create_outreach(
            "owner",
            "status_update",
            "check_in",
            "Duplicate",
            "This should resolve to the active event.",
            "Same reason",
            None,
            None,
            Some("task-1-progress"),
            &actions,
            None,
        )
        .unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(store.outreaches("owner", false, 20).unwrap().len(), 1);

    let delivered = store
        .act_on_outreach("owner", &first.id, "delivered", None)
        .unwrap()
        .unwrap();
    assert_eq!(delivered.status, "delivered");
    assert!(delivered.delivered_at.is_some());
    let snoozed = store
        .act_on_outreach("owner", &first.id, "later", Some(20))
        .unwrap()
        .unwrap();
    assert_eq!(snoozed.status, "snoozed");
    assert!(snoozed.snoozed_until.is_some());

    let policy = store.outreach_policy("owner").unwrap();
    assert!(policy.enabled);
    assert_eq!(policy.max_checkins_per_day, 6);
}
#[test]
fn subagent_work_is_a_durable_idempotent_task_from_start_to_finish() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store
        .create_project("owner", None, "Launch VoiceOS")
        .unwrap();
    let parent = store
        .create_task(
            "owner",
            Some(&project.id),
            None,
            "Prepare the launch",
            "VoiceOS is ready to launch.",
            60,
        )
        .unwrap();
    let task_session = format!("task:{}", parent.id);
    let (task, job, created) = store
        .start_subagent_task(
            "owner",
            "run_123",
            Some(&task_session),
            "Research the launch plan",
            "A verified launch report is returned to VIC.",
            30,
            "normal",
            "provider:hermes",
        )
        .unwrap();
    assert!(created);
    assert_eq!("active", task.status);
    assert_eq!(Some(project.id), task.project_id);
    assert_eq!(Some(parent.id), task.parent_task_id);
    assert!(task.due_at.is_some());
    assert_eq!("running", job.status);

    let (same_task, same_job, created_again) = store
        .start_subagent_task(
            "owner",
            "run_123",
            Some(&task_session),
            "A duplicate title that must not replace the task",
            "A duplicate outcome",
            45,
            "high",
            "provider:hermes",
        )
        .unwrap();
    assert!(!created_again);
    assert_eq!(task.id, same_task.id);
    assert_eq!(job.id, same_job.id);

    let (finished_task, finished_job) = store
        .finish_subagent_task(
            "owner",
            "run_123",
            "completed",
            "Verified report returned.",
            "provider:hermes",
        )
        .unwrap()
        .unwrap();
    assert_eq!("completed", finished_task.status);
    assert_eq!("completed", finished_job.status);
    let detail = store.task_detail("owner", &task.id).unwrap().unwrap();
    assert_eq!(3, detail.progress.completed_steps);
    assert_eq!(3, detail.progress.total_steps);
    assert_eq!(1, detail.handoffs.len());
    assert_eq!("review", detail.progress.lane);
}

#[test]
fn failed_subagent_work_stays_visible_as_a_blocked_task() {
    let store = ConversationStore::in_memory().unwrap();
    let (task, _, _) = store
        .start_subagent_task(
            "owner",
            "run_failed",
            None,
            "Inspect the unavailable service",
            "A verified service report is returned to VIC.",
            15,
            "high",
            "provider:hermes",
        )
        .unwrap();
    let (failed_task, failed_job) = store
        .finish_subagent_task(
            "owner",
            "run_failed",
            "failed",
            "The provider timed out.",
            "provider:hermes",
        )
        .unwrap()
        .unwrap();
    assert_eq!("blocked", failed_task.status);
    assert_eq!("failed", failed_job.status);
    let detail = store.task_detail("owner", &task.id).unwrap().unwrap();
    assert_eq!(
        3,
        detail.progress.open_blockers
            + detail
                .steps
                .iter()
                .filter(|step| step.status == "blocked")
                .count()
    );
}
