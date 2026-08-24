use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;
use voiceos_core::{
    ConversationStore, NewOutreachDelivery, NewOutreachProposal, NewProactiveCandidate,
    NewProactiveFeedback, NewProactiveSubscription,
};

fn subscription(store: &ConversationStore, owner: &str) -> voiceos_core::ProactiveSubscription {
    store
        .create_proactive_subscription(NewProactiveSubscription {
            owner_id: owner.into(),
            topic: "project-health".into(),
            project_id: None,
            source_type: "local_project".into(),
            cadence: "daily".into(),
            quiet_hours: Some("22:00-08:00".into()),
            status: "active".into(),
            provenance: "test://subscription".into(),
        })
        .unwrap()
}

fn candidate(
    store: &ConversationStore,
    owner: &str,
    subscription_id: &str,
    deduplication_key: &str,
) -> NewProactiveCandidate {
    let project = store
        .create_project(owner, None, "Evidence project")
        .unwrap();
    let task = store
        .create_task(
            owner,
            Some(&project.id),
            None,
            "Evidence task",
            "Authoritative task evidence",
            10,
        )
        .unwrap();
    NewProactiveCandidate {
        owner_id: owner.into(),
        subscription_id: Some(subscription_id.into()),
        project_id: Some(project.id),
        reason: "Project has had no recent progress".into(),
        evidence: json!([{"source_type":"task", "source_id":task.id}]),
        priority: "normal".into(),
        confidence: 0.8,
        expires_at: "2099-09-01T00:00:00Z".into(),
        deduplication_key: deduplication_key.into(),
        provenance: "detector://stale-project".into(),
    }
}

#[test]
fn proactive_records_are_owner_scoped_and_candidate_creation_is_idempotent() {
    let store = ConversationStore::in_memory().unwrap();
    let owner_a_subscription = subscription(&store, "owner-a");
    let owner_b_subscription = subscription(&store, "owner-b");

    let first = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &owner_a_subscription.id,
            "stale-project-1",
        ))
        .unwrap();
    let retry = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &owner_a_subscription.id,
            "stale-project-1",
        ))
        .unwrap();
    let other_owner = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-b",
            &owner_b_subscription.id,
            "stale-project-1",
        ))
        .unwrap();

    assert_eq!(first.id, retry.id);
    assert_ne!(first.id, other_owner.id);
    assert_eq!(
        store.proactive_candidates("owner-a", 10).unwrap(),
        vec![first]
    );
    assert_eq!(
        store.proactive_candidates("owner-b", 10).unwrap(),
        vec![other_owner]
    );
    assert!(
        store
            .proactive_candidate("owner-b", &retry.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn candidate_creation_rejects_malformed_or_non_future_expiry() {
    let store = ConversationStore::in_memory().unwrap();
    let subscription = subscription(&store, "owner-a");

    for (expires_at, deduplication_key) in [
        ("not-an-rfc3339-time", "malformed-expiry"),
        ("2000-01-01T00:00:00Z", "expired-expiry"),
    ] {
        assert!(
            store
                .create_proactive_candidate(NewProactiveCandidate {
                    expires_at: expires_at.into(),
                    ..candidate(&store, "owner-a", &subscription.id, deduplication_key)
                })
                .is_err(),
            "candidate expiration must be RFC3339 and strictly future: {expires_at}"
        );
    }
}

#[test]
fn candidate_creation_rejects_forged_or_cross_owner_evidence() {
    let store = ConversationStore::in_memory().unwrap();
    let subscription = subscription(&store, "owner-a");
    let owner_b_project = store
        .create_project("owner-b", None, "Private project")
        .unwrap();

    for (evidence, deduplication_key) in [
        (
            json!([{"source_type":"task", "source_id":"forged-task"}]),
            "forged-evidence",
        ),
        (
            json!([{"source_type":"project", "source_id":owner_b_project.id}]),
            "cross-owner-evidence",
        ),
    ] {
        assert!(
            store
                .create_proactive_candidate(NewProactiveCandidate {
                    evidence,
                    ..candidate(&store, "owner-a", &subscription.id, deduplication_key)
                })
                .is_err(),
            "evidence must resolve to an owner-scoped authoritative record"
        );
    }
}

#[test]
fn project_scoped_records_reject_projects_owned_by_another_owner() {
    let store = ConversationStore::in_memory().unwrap();
    let owner_b_project = store
        .create_project("owner-b", None, "Private project")
        .unwrap();

    assert!(
        store
            .create_proactive_subscription(NewProactiveSubscription {
                owner_id: "owner-a".into(),
                topic: "project-health".into(),
                project_id: Some(owner_b_project.id.clone()),
                source_type: "local_project".into(),
                cadence: "daily".into(),
                quiet_hours: None,
                status: "active".into(),
                provenance: "test://subscription".into(),
            })
            .is_err()
    );

    let owner_a_subscription = subscription(&store, "owner-a");
    assert!(
        store
            .create_proactive_candidate(NewProactiveCandidate {
                project_id: Some(owner_b_project.id),
                ..candidate(
                    &store,
                    "owner-a",
                    &owner_a_subscription.id,
                    "cross-owner-project"
                )
            })
            .is_err()
    );
}

#[test]
fn proposal_preserves_original_draft_and_links_its_owner_scoped_candidate() {
    let store = ConversationStore::in_memory().unwrap();
    let owner_a_subscription = subscription(&store, "owner-a");
    let owner_b_subscription = subscription(&store, "owner-b");
    let owner_a_candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &owner_a_subscription.id,
            "candidate-a",
        ))
        .unwrap();
    let owner_b_candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-b",
            &owner_b_subscription.id,
            "candidate-b",
        ))
        .unwrap();

    let proposal = store
        .create_outreach_proposal(NewOutreachProposal {
            owner_id: "owner-a".into(),
            candidate_id: owner_a_candidate.id.clone(),
            original_draft: "Would you like to review Project 1?".into(),
            editable_draft: "Would you like to review Project 1 today?".into(),
            channel: "internal_queue".into(),
            approval_state: "pending_review".into(),
            risk_class: "normal".into(),
            delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
            provenance: "draft://deterministic".into(),
        })
        .unwrap();

    assert_eq!(proposal.candidate_id, owner_a_candidate.id);
    assert_eq!(
        proposal.original_draft,
        "Would you like to review Project 1?"
    );
    assert_eq!(
        proposal.editable_draft,
        "Would you like to review Project 1 today?"
    );
    assert_eq!(proposal.approval_state, "pending_review");
    assert!(
        store
            .outreach_proposal("owner-b", &proposal.id)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .create_outreach_proposal(NewOutreachProposal {
                owner_id: "owner-a".into(),
                candidate_id: owner_b_candidate.id,
                original_draft: "Would you like to review the project status?".into(),
                editable_draft: "Would you like to review the project status?".into(),
                channel: "internal_queue".into(),
                approval_state: "pending_review".into(),
                risk_class: "normal".into(),
                delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
                provenance: "draft://deterministic".into(),
            })
            .is_err()
    );
}

#[test]
fn direct_proposal_creation_rejects_action_text_in_each_draft() {
    let store = ConversationStore::in_memory().unwrap();
    let subscription = subscription(&store, "owner-a");
    let candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &subscription.id,
            "direct-action-text",
        ))
        .unwrap();

    for (original_draft, editable_draft) in [
        (
            "Email the user now.",
            "Would you like to review the project status?",
        ),
        (
            "Would you like to review the project status?",
            "Delete the project.",
        ),
    ] {
        assert!(
            store
                .create_outreach_proposal(NewOutreachProposal {
                    owner_id: "owner-a".into(),
                    candidate_id: candidate.id.clone(),
                    original_draft: original_draft.into(),
                    editable_draft: editable_draft.into(),
                    channel: "internal_queue".into(),
                    approval_state: "pending_review".into(),
                    risk_class: "normal".into(),
                    delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
                    provenance: "draft://direct-test".into(),
                })
                .is_err(),
            "action text must be rejected: {original_draft} / {editable_draft}"
        );
    }
}

#[test]
fn direct_proposal_creation_rejects_review_question_action_clause_bypasses() {
    let store = ConversationStore::in_memory().unwrap();
    let subscription = subscription(&store, "owner-a");
    let candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &subscription.id,
            "direct-review-question-action-clause",
        ))
        .unwrap();

    for draft in [
        "Would you like to review the project and email the user?",
        "Would you like to review the project status; Email the user?",
        "Would you like to review the project status\nDelete the project?",
    ] {
        assert!(
            store
                .create_outreach_proposal(NewOutreachProposal {
                    owner_id: "owner-a".into(),
                    candidate_id: candidate.id.clone(),
                    original_draft: draft.into(),
                    editable_draft: draft.into(),
                    channel: "internal_queue".into(),
                    approval_state: "pending_review".into(),
                    risk_class: "normal".into(),
                    delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
                    provenance: "draft://direct-test".into(),
                })
                .is_err(),
            "review-question action clause must be rejected: {draft}"
        );
    }
}

#[test]
fn direct_proposal_creation_rejects_invalid_or_unbounded_delivery_deadlines() {
    let store = ConversationStore::in_memory().unwrap();
    let subscription = subscription(&store, "owner-a");
    let candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &subscription.id,
            "direct-deadline",
        ))
        .unwrap();

    for delivery_deadline in [
        "not-an-rfc3339-time",
        "2000-01-01T00:00:00Z",
        "2099-09-01T00:00:01Z",
    ] {
        assert!(
            store
                .create_outreach_proposal(NewOutreachProposal {
                    owner_id: "owner-a".into(),
                    candidate_id: candidate.id.clone(),
                    original_draft: "Would you like to review the project status?".into(),
                    editable_draft: "Would you like to review the project status?".into(),
                    channel: "internal_queue".into(),
                    approval_state: "pending_review".into(),
                    risk_class: "normal".into(),
                    delivery_deadline: Some(delivery_deadline.into()),
                    provenance: "draft://direct-test".into(),
                })
                .is_err(),
            "delivery deadline must be RFC3339, future, and bounded by candidate: {delivery_deadline}"
        );
    }
}

#[test]
fn direct_proposal_creation_requires_internal_pending_review() {
    let store = ConversationStore::in_memory().unwrap();
    let subscription = subscription(&store, "owner-a");
    let candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &subscription.id,
            "direct-review-state",
        ))
        .unwrap();

    for (channel, approval_state) in [("email", "pending_review"), ("internal_queue", "approved")] {
        assert!(
            store
                .create_outreach_proposal(NewOutreachProposal {
                    owner_id: "owner-a".into(),
                    candidate_id: candidate.id.clone(),
                    original_draft: "Would you like to review the project status?".into(),
                    editable_draft: "Would you like to review the project status?".into(),
                    channel: channel.into(),
                    approval_state: approval_state.into(),
                    risk_class: "normal".into(),
                    delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
                    provenance: "draft://direct-test".into(),
                })
                .is_err(),
            "direct proposals must stay in the internal review queue: {channel} / {approval_state}"
        );
    }
}

#[test]
fn direct_proposal_creation_rejects_expired_candidate() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("voiceos.db");
    let store = ConversationStore::open(&database_path).unwrap();
    let subscription = subscription(&store, "owner-a");
    let candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &subscription.id,
            "direct-expired-candidate",
        ))
        .unwrap();
    Connection::open(&database_path)
        .unwrap()
        .execute(
            "UPDATE proactive_candidates SET expires_at=?1 WHERE candidate_id=?2",
            ["2000-01-01T00:00:00Z", candidate.id.as_str()],
        )
        .unwrap();

    let error = store
        .create_outreach_proposal(NewOutreachProposal {
            owner_id: "owner-a".into(),
            candidate_id: candidate.id,
            original_draft: "Would you like to review the project status?".into(),
            editable_draft: "Would you like to review the project status?".into(),
            channel: "internal_queue".into(),
            approval_state: "pending_review".into(),
            risk_class: "normal".into(),
            delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
            provenance: "draft://direct-test".into(),
        })
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("candidate expiration must be in the future")
    );
}

#[test]
fn proactive_records_require_owner_and_provenance_and_do_not_mutate_tasks_memories_or_outreach() {
    let store = ConversationStore::in_memory().unwrap();

    assert!(
        store
            .create_proactive_subscription(NewProactiveSubscription {
                owner_id: " ".into(),
                topic: "topic".into(),
                project_id: None,
                source_type: "local_project".into(),
                cadence: "daily".into(),
                quiet_hours: None,
                status: "active".into(),
                provenance: "test://subscription".into(),
            })
            .is_err()
    );
    assert!(
        store
            .create_proactive_subscription(NewProactiveSubscription {
                owner_id: "owner-a".into(),
                topic: "topic".into(),
                project_id: None,
                source_type: "local_project".into(),
                cadence: "daily".into(),
                quiet_hours: None,
                status: "active".into(),
                provenance: " ".into(),
            })
            .is_err()
    );

    let active_subscription = subscription(&store, "owner-a");
    assert!(
        store
            .create_proactive_candidate(NewProactiveCandidate {
                provenance: " ".into(),
                ..candidate(
                    &store,
                    "owner-a",
                    &active_subscription.id,
                    "missing-provenance"
                )
            })
            .is_err()
    );

    let candidate_input = candidate(
        &store,
        "owner-a",
        &active_subscription.id,
        "no-side-effects",
    );
    let tasks_before = store.tasks("owner-a", true, 10).unwrap();
    let memories_before = store.memories_for_owner("owner-a", 10).unwrap();
    let outreach_before = store.outreaches("owner-a", true, 10).unwrap();
    let created = store.create_proactive_candidate(candidate_input).unwrap();
    store
        .create_outreach_proposal(NewOutreachProposal {
            owner_id: "owner-a".into(),
            candidate_id: created.id,
            original_draft: "Would you like to review the project status?".into(),
            editable_draft: "Would you like to review the project status?".into(),
            channel: "internal_queue".into(),
            approval_state: "pending_review".into(),
            risk_class: "normal".into(),
            delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
            provenance: "draft://test".into(),
        })
        .unwrap();

    assert_eq!(store.tasks("owner-a", true, 10).unwrap(), tasks_before);
    assert_eq!(
        store.memories_for_owner("owner-a", 10).unwrap(),
        memories_before
    );
    assert_eq!(
        store.outreaches("owner-a", true, 10).unwrap(),
        outreach_before
    );
}

#[test]
fn delivery_attempts_and_feedback_are_owner_scoped_audited_records_without_contact() {
    let store = ConversationStore::in_memory().unwrap();
    let subscription = subscription(&store, "owner-a");
    let candidate = store
        .create_proactive_candidate(candidate(
            &store,
            "owner-a",
            &subscription.id,
            "delivery-candidate",
        ))
        .unwrap();
    let proposal = store
        .create_outreach_proposal(NewOutreachProposal {
            owner_id: "owner-a".into(),
            candidate_id: candidate.id,
            original_draft: "Would you like to review this project?".into(),
            editable_draft: "Would you like to review this project?".into(),
            channel: "internal_queue".into(),
            approval_state: "pending_review".into(),
            risk_class: "normal".into(),
            delivery_deadline: Some("2099-08-31T12:00:00Z".into()),
            provenance: "approval://test".into(),
        })
        .unwrap();
    assert!(
        store
            .create_outreach_delivery(NewOutreachDelivery {
                owner_id: "owner-a".into(),
                proposal_id: proposal.id.clone(),
                provider: "none".into(),
                channel: "internal_queue".into(),
                result: "dry_run".into(),
                idempotency_key: "delivery-1".into(),
                response_link: None,
                provenance: "delivery://dry-run".into(),
            })
            .is_err()
    );
    let feedback = store
        .create_proactive_feedback(NewProactiveFeedback {
            owner_id: "owner-a".into(),
            proposal_id: Some(proposal.id.clone()),
            action: "useful".into(),
            note: Some("Good timing".into()),
            provenance: "feedback://owner".into(),
        })
        .unwrap();

    assert_eq!(feedback.proposal_id.as_deref(), Some(proposal.id.as_str()));
    assert!(
        store
            .proactive_feedback("owner-b", &feedback.id)
            .unwrap()
            .is_none()
    );
}
