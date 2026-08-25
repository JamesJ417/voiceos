use voiceos_core::ConversationStore;

fn task(
    store: &ConversationStore,
    owner: &str,
    project_id: &str,
    title: &str,
    minutes: u32,
) -> String {
    let task = store
        .create_task(
            owner,
            Some(project_id),
            None,
            title,
            &format!("{title} is visibly complete"),
            minutes,
        )
        .unwrap();
    store
        .create_task_step(
            owner,
            &task.id,
            &format!("Open the materials for {title}"),
            "user",
            "test",
        )
        .unwrap();
    task.id
}

#[test]
fn focus_snapshot_limits_choices_and_keeps_goal_context() {
    let store = ConversationStore::in_memory().unwrap();
    let goal = store
        .create_goal(
            "owner",
            "Open the restaurant",
            "Restaurant serves its first guest",
        )
        .unwrap();
    let project = store
        .create_project("owner", Some(&goal.id), "Opening checklist")
        .unwrap();
    task(&store, "owner", &project.id, "Print menus", 30);
    task(&store, "owner", &project.id, "Call electrician", 10);
    task(&store, "owner", &project.id, "Order aprons", 20);
    task(&store, "owner", &project.id, "Paint office", 120);

    let snapshot = store.focus_snapshot("owner", "normal").unwrap();
    assert_eq!(snapshot.priorities.len(), 3);
    assert_eq!(
        snapshot
            .recommendation
            .as_ref()
            .unwrap()
            .goal_title
            .as_deref(),
        Some("Open the restaurant")
    );
    let low_energy = store.focus_snapshot("owner", "low_energy").unwrap();
    assert_eq!(low_energy.recommendation.unwrap().estimated_minutes, 10);
}

#[test]
fn personal_focus_reset_recommends_one_concrete_first_action() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store.create_project("owner", None, "Website").unwrap();
    task(&store, "owner", &project.id, "Update home page", 20);

    let reset = store.personal_focus_reset("owner", "normal").unwrap();

    assert_eq!(reset.priorities.len(), 1);
    assert_eq!(
        reset
            .recommendation
            .as_ref()
            .map(|task| task.title.as_str()),
        Some("Update home page")
    );
    assert_eq!(
        reset.first_physical_action.as_deref(),
        Some("Open the materials for Update home page")
    );
    assert_eq!(
        reset.five_minute_version.as_deref(),
        Some("Spend five minutes: Open the materials for Update home page")
    );
    assert_eq!(reset.optional_question, None);
}

#[test]
fn personal_focus_reset_uses_low_energy_ordering() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store.create_project("owner", None, "Website").unwrap();
    task(&store, "owner", &project.id, "Write release notes", 30);
    task(&store, "owner", &project.id, "Open analytics", 5);

    let reset = store.personal_focus_reset("owner", "low_energy").unwrap();

    assert_eq!(reset.recommendation.unwrap().title, "Open analytics");
}

#[test]
fn personal_focus_reset_limits_more_than_three_tasks_to_three_priorities() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store.create_project("owner", None, "Website").unwrap();
    for (title, minutes) in [
        ("Write release notes", 30),
        ("Open analytics", 5),
        ("Update home page", 20),
        ("Review feedback", 15),
    ] {
        task(&store, "owner", &project.id, title, minutes);
    }

    let reset = store.personal_focus_reset("owner", "normal").unwrap();

    assert_eq!(reset.priorities.len(), 3);
    assert!(reset.recommendation.is_some());
}

#[test]
fn personal_focus_reset_with_an_empty_board_invites_a_tiny_capture() {
    let store = ConversationStore::in_memory().unwrap();

    let reset = store.personal_focus_reset("owner", "normal").unwrap();

    assert!(reset.priorities.is_empty());
    assert_eq!(reset.recommendation, None);
    assert_eq!(
        reset.first_physical_action.as_deref(),
        Some("Capture the next thing pulling at you.")
    );
    assert_eq!(
        reset.five_minute_version.as_deref(),
        Some("Spend five minutes: Capture the next thing pulling at you.")
    );
    assert_eq!(reset.optional_question, None);
    assert!(reset.message.contains("nothing you need to catch up on"));
}

#[test]
fn personal_focus_reset_uses_the_saved_restart_action_after_an_interruption() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store.create_project("owner", None, "Website").unwrap();
    let interrupted_task = task(&store, "owner", &project.id, "Update home page", 20);
    for title in ["Pay hosting bill", "Reply to customer", "Fix login bug"] {
        let urgent_task = task(&store, "owner", &project.id, title, 10);
        store
            .set_task_attention_as(
                "owner",
                &urgent_task,
                Some("2000-01-01T12:00:00Z"),
                "critical",
                "test",
            )
            .unwrap();
    }
    let session = store
        .start_focus_session("owner", &interrupted_task, "normal", 20, "device:test")
        .unwrap();
    store
        .interrupt_focus_session(
            "owner",
            &session.id,
            "A call came in",
            Some("Reopen the home page draft"),
            "device:test",
        )
        .unwrap();

    let reset = store.personal_focus_reset("owner", "normal").unwrap();

    assert_eq!(
        reset
            .recommendation
            .as_ref()
            .map(|task| task.task_id.as_str()),
        Some(interrupted_task.as_str())
    );
    assert_eq!(
        reset
            .interrupted_session
            .as_ref()
            .map(|session| session.id.as_str()),
        Some(session.id.as_str())
    );
    assert_eq!(
        reset.first_physical_action.as_deref(),
        Some("Reopen the home page draft")
    );
    assert_eq!(
        reset.five_minute_version.as_deref(),
        Some("Spend five minutes: Reopen the home page draft")
    );
}

#[test]
fn interruption_preserves_a_restart_point_without_marking_the_task_done() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store.create_project("owner", None, "Website").unwrap();
    let task_id = task(&store, "owner", &project.id, "Update home page", 20);
    let started = store
        .start_focus_session("owner", &task_id, "five_minute", 5, "device:test")
        .unwrap();
    assert_eq!(started.status, "active");
    assert!(started.next_action.starts_with("Open the materials"));

    let interrupted = store
        .interrupt_focus_session(
            "owner",
            &started.id,
            "Phone call",
            Some("Reopen the home page draft"),
            "device:test",
        )
        .unwrap();
    assert_eq!(interrupted.status, "interrupted");
    assert_eq!(
        interrupted.restart_action.as_deref(),
        Some("Reopen the home page draft")
    );

    let restarted = store
        .resume_focus_session("owner", &started.id, 5, "device:test")
        .unwrap();
    assert_eq!(restarted.mode, "restart");
    assert_eq!(restarted.next_action, "Reopen the home page draft");
    let completed = store
        .complete_focus_session(
            "owner",
            &started.id,
            Some("Draft reopened"),
            Some("Write the first headline"),
            "device:test",
        )
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(
        store.task("owner", &task_id).unwrap().unwrap().status,
        "ready"
    );
    let events = store.execution_events("owner", &task_id, 50).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "focus.started")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "focus.interrupted")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "focus.restarted")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "focus.completed")
    );
}

#[test]
fn deadlines_rank_ready_work_and_proposed_ideas_stay_parked() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store
        .create_project("owner", None, "Attention system")
        .unwrap();
    let later = task(&store, "owner", &project.id, "Interesting future idea", 5);
    let overdue = task(
        &store,
        "owner",
        &project.id,
        "Pay the time-sensitive bill",
        30,
    );
    let parked = task(
        &store,
        "owner",
        &project.id,
        "Redesign the whole office",
        120,
    );

    store
        .set_task_attention_as(
            "owner",
            &later,
            Some("2100-01-01T12:00:00Z"),
            "critical",
            "test",
        )
        .unwrap();
    let updated = store
        .set_task_attention_as(
            "owner",
            &overdue,
            Some("2000-01-01T12:00:00Z"),
            "normal",
            "test",
        )
        .unwrap()
        .unwrap();
    assert_eq!(updated.importance, "normal");
    assert_eq!(updated.due_at.as_deref(), Some("2000-01-01T12:00:00+00:00"));
    store
        .update_task_status_as("owner", &parked, "proposed", "test")
        .unwrap();

    let snapshot = store.focus_snapshot("owner", "normal").unwrap();
    assert_eq!(snapshot.recommendation.unwrap().task_id, overdue);
    assert_eq!(snapshot.parked.len(), 1);
    assert_eq!(snapshot.parked[0].task_id, parked);
    assert!(
        !snapshot
            .priorities
            .iter()
            .any(|item| item.task_id == parked)
    );
    assert_eq!(snapshot.priorities[0].urgency, "overdue");

    let events = store.execution_events("owner", &overdue, 20).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "task.attention_changed")
    );
}
