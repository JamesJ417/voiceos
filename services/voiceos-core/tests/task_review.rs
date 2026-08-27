use voiceos_core::ConversationStore;

fn task(store: &ConversationStore, title: &str) -> voiceos_core::TaskRecord {
    store
        .create_task("owner", None, None, title, "Verified outcome", 10)
        .unwrap()
}

#[test]
fn durable_reviews_rotate_through_open_tasks_after_failures_and_wrap() {
    let store = ConversationStore::in_memory().unwrap();
    let first = task(&store, "First");
    let second = task(&store, "Second");
    let third = task(&store, "Third");

    let review_one = store.claim_next_task_review("owner", 600).unwrap().unwrap();
    assert_eq!(review_one.task.task.id, first.id);
    store
        .fail_task_review("owner", &review_one.review.id, "provider_failed")
        .unwrap();

    let review_two = store.claim_next_task_review("owner", 600).unwrap().unwrap();
    assert_eq!(review_two.task.task.id, second.id);
    store
        .complete_task_review(
            "owner",
            &review_two.review.id,
            "Checked dependencies",
            vec!["Prepare the next safe draft".to_owned()],
            vec![],
            vec![],
        )
        .unwrap();

    let review_three = store.claim_next_task_review("owner", 600).unwrap().unwrap();
    assert_eq!(review_three.task.task.id, third.id);
    store
        .complete_task_review(
            "owner",
            &review_three.review.id,
            "Checked dependencies",
            vec![],
            vec![],
            vec![],
        )
        .unwrap();

    let wrapped = store.claim_next_task_review("owner", 600).unwrap().unwrap();
    assert_eq!(wrapped.task.task.id, first.id);
}

#[test]
fn durable_reviews_allow_only_one_active_run_and_keep_findings_bounded() {
    let store = ConversationStore::in_memory().unwrap();
    task(&store, "Only");
    let review = store.claim_next_task_review("owner", 600).unwrap().unwrap();
    let repeated = store.claim_next_task_review("owner", 600).unwrap().unwrap();
    assert_eq!(repeated.review.id, review.review.id);
    assert_eq!(repeated.task.task.id, review.task.task.id);
    assert!(
        store
            .complete_task_review(
                "owner",
                &review.review.id,
                "x",
                vec!["a".to_owned(); 4],
                vec![],
                vec![]
            )
            .is_err()
    );
    assert_eq!(
        store
            .task_review_snapshot("owner", 10)
            .unwrap()
            .active_review
            .unwrap()
            .id,
        review.review.id
    );
    store
        .fail_task_review("owner", &review.review.id, "invalid_result")
        .unwrap();
    assert!(
        store
            .task_review_snapshot("owner", 10)
            .unwrap()
            .active_review
            .is_none()
    );
}
