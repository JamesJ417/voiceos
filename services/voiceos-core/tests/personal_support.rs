use chrono::{Duration, Utc};
use voiceos_core::{
    CaptureSource, ConversationStore, DailyFocusReset, NewCaptureProposal, NewPersonalCapture,
    PersonalCapture, PersonalExtractionContract, PersonalExtractionInput, ReviewDecision,
    TaskApprovalStatus,
};

struct StaticExtractor(String);

impl PersonalExtractionContract for StaticExtractor {
    fn extract(&self, _input: &PersonalExtractionInput) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

#[test]
fn personal_focus_reset_is_owner_scoped_and_has_no_outbound_side_effects() {
    let store = ConversationStore::in_memory().unwrap();
    let project = store.create_project("owner-a", None, "Errands").unwrap();
    let task = store
        .create_task(
            "owner-a",
            Some(&project.id),
            None,
            "Buy milk",
            "Milk is in the refrigerator",
            10,
        )
        .unwrap();
    store
        .create_task_step(
            "owner-a",
            &task.id,
            "Open the shopping list",
            "user",
            "test",
        )
        .unwrap();
    let counts_before = store.downstream_record_counts("owner-a").unwrap();

    let other_owner = store.personal_focus_reset("owner-b", "normal").unwrap();
    let owner_reset = store.personal_focus_reset("owner-a", "normal").unwrap();

    assert!(other_owner.priorities.is_empty());
    assert_eq!(
        owner_reset
            .recommendation
            .as_ref()
            .map(|task| task.task_id.as_str()),
        Some(task.id.as_str())
    );
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        counts_before
    );
}

fn capture(owner_id: &str) -> NewPersonalCapture {
    NewPersonalCapture {
        owner_id: owner_id.into(),
        source: "voice".into(),
        source_id: "utterance-1".into(),
        raw_content: "  buy milk after work  ".into(),
        structured_content: Some(serde_json::json!({"text": "buy milk after work"})),
        created_at: "2026-08-24T12:00:00Z".into(),
        expires_at: "2026-08-25T12:00:00Z".into(),
        audit_id: "audit-capture-1".into(),
    }
}

#[test]
fn expired_captures_are_hidden_from_the_owner_inbox() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("short-lived"),
            "temporary thought",
            Utc::now(),
            Duration::milliseconds(1),
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));

    assert_eq!(store.personal_inbox("owner-a").unwrap(), Vec::new());
    assert_eq!(
        store.personal_capture("owner-a", &capture.id).unwrap(),
        None
    );
}

#[test]
fn only_the_owner_can_decide_a_capture_and_decisions_only_update_intake() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("utterance-1"),
            "buy milk",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();

    assert!(
        store
            .decide_personal_capture("owner-b", &capture.id, "approved", "audit-b")
            .is_err()
    );
    let decision = store
        .decide_personal_capture("owner-a", &capture.id, "approved", "audit-a")
        .unwrap();
    assert_eq!(decision.status, "approved");
    assert_eq!(store.personal_inbox("owner-a").unwrap(), Vec::new());
    assert_eq!(store.personal_inbox("owner-b").unwrap(), Vec::new());
    assert_eq!(
        store
            .personal_capture("owner-a", &capture.id)
            .unwrap()
            .unwrap()
            .status,
        "approved"
    );
    assert!(store.audit_event_exists("owner-a", "audit-a").unwrap());
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        (0, 0, 0, 0, 0)
    );
}

#[test]
fn decided_capture_is_hidden_and_cannot_be_extracted() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("discard-before-extract"),
            "Maybe reorganize the garage",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    store
        .decide_personal_capture(
            "owner-a",
            &capture.id,
            "discarded",
            "discard-before-extract-audit",
        )
        .unwrap();
    let output = serde_json::json!({
        "owner_id": "owner-a",
        "capture_id": capture.id,
        "candidates": []
    })
    .to_string();

    assert!(
        store
            .extract_personal_capture("owner-a", &capture.id, &StaticExtractor(output))
            .is_err()
    );
    assert!(store.personal_inbox("owner-a").unwrap().is_empty());
}

#[test]
fn capture_deduplicates_voice_capture_ids_and_fieldy_event_ids() {
    let store = ConversationStore::in_memory().unwrap();
    let occurred_at = Utc::now();
    let first = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("utterance-1"),
            "first transcript",
            occurred_at,
            Duration::days(1),
        )
        .unwrap();
    let repeated = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("utterance-1"),
            "changed retry transcript",
            occurred_at,
            Duration::days(1),
        )
        .unwrap();
    assert_eq!(first.id, repeated.id);

    let event = voiceos_core::FieldyTranscriptEvent {
        event_id: "fieldy-event-1".into(),
        occurred_at: occurred_at.to_rfc3339(),
        transcript: "Fieldy transcript".into(),
        recording_id: None,
        session_id: None,
        speakers: vec![],
        metadata: serde_json::json!({}),
    };
    let fieldy = store
        .capture_fieldy_event("owner-a", &event, Duration::days(1))
        .unwrap();
    let fieldy_retry = store
        .capture_fieldy_event("owner-a", &event, Duration::days(1))
        .unwrap();
    assert_eq!(fieldy.id, fieldy_retry.id);
    assert_eq!(store.personal_inbox("owner-a").unwrap().len(), 2);
}

#[test]
fn capture_personal_input_preserves_raw_text_normalizes_display_and_has_no_downstream_effects() {
    let store = ConversationStore::in_memory().unwrap();
    let raw = "  Buy   milk\nwhen I leave work.  ";

    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("utterance-1"),
            raw,
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();

    assert_eq!(capture.raw_content, raw);
    assert_eq!(capture.display_text, "Buy milk when I leave work.");
    assert_eq!(
        store.personal_inbox("owner-a").unwrap(),
        vec![capture.clone()]
    );
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        (0, 0, 0, 0, 0)
    );
}

#[test]
fn personal_capture_is_typed_owner_scoped_and_audited() {
    let store = ConversationStore::in_memory().unwrap();
    let record: PersonalCapture = store.create_personal_capture(capture("owner-a")).unwrap();
    assert_eq!(record.owner_id, "owner-a");
    assert_eq!(record.status, "received");
    assert_eq!(record.raw_content, "  buy milk after work  ");
    assert_eq!(store.personal_capture("owner-b", &record.id).unwrap(), None);
    assert!(
        store
            .personal_capture("owner-a", &record.id)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .audit_event_exists("owner-a", &record.audit_id)
            .unwrap()
    );
}

#[test]
fn personal_records_reject_empty_malformed_cross_owner_and_expired_inputs() {
    let store = ConversationStore::in_memory().unwrap();
    for bad in [
        NewPersonalCapture {
            raw_content: " ".into(),
            source_id: "x".into(),
            ..capture("owner-a")
        },
        NewPersonalCapture {
            created_at: "nope".into(),
            source_id: "y".into(),
            ..capture("owner-a")
        },
        NewPersonalCapture {
            expires_at: "2026-08-24T11:00:00Z".into(),
            source_id: "z".into(),
            ..capture("owner-a")
        },
    ] {
        assert!(store.create_personal_capture(bad).is_err());
    }
    let created = store.create_personal_capture(capture("owner-a")).unwrap();
    assert!(
        store
            .create_capture_proposal(NewCaptureProposal {
                owner_id: "owner-b".into(),
                capture_id: created.id,
                title: "x".into(),
                category: "task".into(),
                rationale: "r".into(),
                expires_at: "2026-08-25T00:00:00Z".into(),
                audit_id: "a".into(),
            })
            .is_err()
    );
}

#[test]
fn proposal_and_daily_reset_use_explicit_states_and_review_decisions() {
    let store = ConversationStore::in_memory().unwrap();
    let cap = store.create_personal_capture(capture("owner-a")).unwrap();
    let proposal = store
        .create_capture_proposal(NewCaptureProposal {
            owner_id: "owner-a".into(),
            capture_id: cap.id.clone(),
            title: "Buy milk".into(),
            category: "task".into(),
            rationale: "stated intent".into(),
            expires_at: (Utc::now() + Duration::days(1)).to_rfc3339(),
            audit_id: "audit-proposal".into(),
        })
        .unwrap();
    assert_eq!(proposal.status, "reviewing");
    let decision: ReviewDecision = store
        .decide_capture_proposal("owner-a", &proposal.id, "snoozed", "audit-decision")
        .unwrap();
    assert_eq!(decision.status, "snoozed");
    let reset: DailyFocusReset = store
        .create_daily_focus_reset("owner-a", "2026-08-24", "audit-reset")
        .unwrap();
    assert_eq!(reset.status, "received");
}

#[test]
fn extraction_of_a_messy_brain_dump_persists_only_reviewable_proposals() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("brain-dump-1"),
            "Need milk, worried I forgot rent, and maybe learn pottery.",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let output = serde_json::json!({
        "owner_id": "owner-a",
        "capture_id": capture.id,
        "candidates": [{
            "category": "task",
            "confidence": 0.9,
            "title": "Buy milk",
            "details": "Pick up milk after work.",
            "suggested_next_action": "Add milk to your shopping list for review.",
            "rationale": "The capture explicitly says you need milk.",
            "evidence_capture_ids": [capture.id],
            "expires_at": capture.expires_at
        }]
    })
    .to_string();

    let proposals = store
        .extract_personal_capture("owner-a", &capture.id, &StaticExtractor(output))
        .unwrap();

    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].category, "task");
    assert_eq!(proposals[0].confidence, 0.9);
    assert_eq!(
        proposals[0].details.as_deref(),
        Some("Pick up milk after work.")
    );
    assert_eq!(
        proposals[0].suggested_next_action,
        "Add milk to your shopping list for review."
    );
    assert_eq!(proposals[0].evidence_capture_ids, vec![capture.id]);
    assert_eq!(proposals[0].status, "reviewing");
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        (0, 0, 0, 0, 0)
    );
}

#[test]
fn extraction_persists_multiple_finite_category_candidates() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("brain-dump-2"),
            "Milk, rent worry, pottery idea.",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let candidate = |category: &str, title: &str| {
        serde_json::json!({
            "category": category, "confidence": 0.8, "title": title, "details": null,
            "suggested_next_action": "Review this suggestion.",
            "rationale": "This was stated in the capture.",
            "evidence_capture_ids": [capture.id], "expires_at": capture.expires_at
        })
    };
    let output = serde_json::json!({
        "owner_id": "owner-a", "capture_id": capture.id,
        "candidates": [candidate("task", "Buy milk"), candidate("worry", "Rent concern"), candidate("idea", "Learn pottery")]
    })
    .to_string();

    let proposals = store
        .extract_personal_capture("owner-a", &capture.id, &StaticExtractor(output))
        .unwrap();

    assert_eq!(proposals.len(), 3);
    assert_eq!(
        proposals
            .iter()
            .map(|proposal| proposal.category.as_str())
            .collect::<Vec<_>>(),
        vec!["task", "worry", "idea"]
    );
}

#[test]
fn extraction_rejects_hidden_direct_mutation_language_without_persisting() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("unsafe-output"),
            "Call the dentist.",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let output = serde_json::json!({
        "owner_id": "owner-a", "capture_id": capture.id,
        "candidates": [{
            "category": "appointment", "confidence": 0.9, "title": "Dentist appointment",
            "details": "Create a note directly in durable storage.",
            "suggested_next_action": "Review this suggestion.",
            "rationale": "The capture mentions a dentist.",
            "evidence_capture_ids": [capture.id], "expires_at": capture.expires_at
        }]
    })
    .to_string();

    assert!(
        store
            .extract_personal_capture("owner-a", &capture.id, &StaticExtractor(output))
            .is_err()
    );
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        (0, 0, 0, 0, 0)
    );
}

#[test]
fn extraction_rejects_malformed_unknown_and_unscoped_output() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("bad-output"),
            "A thought.",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    for output in [
        "not json".to_owned(),
        serde_json::json!({"owner_id":"owner-a","capture_id":capture.id,"candidates":[],"unexpected":true}).to_string(),
        serde_json::json!({"owner_id":"owner-b","capture_id":capture.id,"candidates":[]}).to_string(),
        serde_json::json!({"owner_id":"owner-a","capture_id":capture.id,"candidates":[{
            "category":"other","confidence":0.5,"title":"x","details":null,"suggested_next_action":"Review.","rationale":"x","evidence_capture_ids":["https://untrusted.example"],"expires_at":capture.expires_at
        }]}).to_string(),
    ] {
        assert!(store
            .extract_personal_capture("owner-a", &capture.id, &StaticExtractor(output))
            .is_err());
    }
}

#[test]
fn extraction_allows_an_explicit_empty_candidate_result() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("empty-output"),
            "Just thinking.",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let output = serde_json::json!({"owner_id":"owner-a","capture_id":capture.id,"candidates":[]})
        .to_string();

    assert!(
        store
            .extract_personal_capture("owner-a", &capture.id, &StaticExtractor(output))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        (0, 0, 0, 0, 0)
    );
}

#[test]
fn owner_can_list_and_approve_a_reviewing_task_proposal_into_the_chosen_task_state() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("task-review-1"),
            "Buy milk",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let proposal = store
        .create_capture_proposal(NewCaptureProposal {
            owner_id: "owner-a".into(),
            capture_id: capture.id.clone(),
            title: "Buy milk".into(),
            category: "task".into(),
            rationale: "The capture asks for milk.".into(),
            expires_at: capture.expires_at.clone(),
            audit_id: "task-proposal".into(),
        })
        .unwrap();

    assert_eq!(
        store.capture_proposals("owner-a", 10).unwrap(),
        vec![proposal.clone()]
    );
    assert_eq!(store.capture_proposals("owner-b", 10).unwrap(), Vec::new());
    let task = store
        .approve_task_proposal(
            "owner-a",
            &proposal.id,
            TaskApprovalStatus::Proposed,
            15,
            "task-approval",
        )
        .unwrap();

    assert_eq!(task.status, "proposed");
    assert_eq!(task.title, "Buy milk");
    assert_eq!(store.tasks("owner-a", false, 10).unwrap(), vec![task]);
    assert!(
        store
            .audit_event_exists("owner-a", "task-approval")
            .unwrap()
    );

    let ready_capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("task-review-ready"),
            "Buy bread",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let ready_proposal = store
        .create_capture_proposal(NewCaptureProposal {
            owner_id: "owner-a".into(),
            capture_id: ready_capture.id,
            title: "Buy bread".into(),
            category: "task".into(),
            rationale: "The capture asks for bread.".into(),
            expires_at: ready_capture.expires_at,
            audit_id: "ready-task-proposal".into(),
        })
        .unwrap();
    assert_eq!(
        store
            .approve_task_proposal(
                "owner-a",
                &ready_proposal.id,
                TaskApprovalStatus::Ready,
                10,
                "ready-task-approval",
            )
            .unwrap()
            .status,
        "ready"
    );
}

#[test]
fn non_task_approval_is_owner_scoped_reviewable_and_never_creates_memory_or_outbound_actions() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("appointment-review-1"),
            "Call the dentist and remember to ask about the crown.",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let appointment = store
        .create_capture_proposal(NewCaptureProposal {
            owner_id: "owner-a".into(),
            capture_id: capture.id.clone(),
            title: "Dentist appointment".into(),
            category: "appointment".into(),
            rationale: "The capture mentions a dentist.".into(),
            expires_at: capture.expires_at.clone(),
            audit_id: "appointment-proposal".into(),
        })
        .unwrap();

    assert!(
        store
            .approve_non_task_proposal("owner-b", &appointment.id, "appointment-approval-b")
            .is_err()
    );
    let record = store
        .approve_non_task_proposal("owner-a", &appointment.id, "appointment-approval")
        .unwrap();

    assert_eq!(record.category, "appointment");
    assert_eq!(record.status, "reviewable");
    assert_eq!(record.proposal_id, appointment.id);
    assert_eq!(record.capture_id, capture.id);
    assert_eq!(
        store.personal_review_records("owner-a", 10).unwrap(),
        vec![record]
    );
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        (0, 0, 0, 0, 0)
    );
    assert!(
        store
            .audit_event_exists("owner-a", "appointment-approval")
            .unwrap()
    );
}

#[test]
fn invalid_or_non_approval_decisions_never_convert_a_task_proposal() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("task-review-2"),
            "Buy bread",
            Utc::now(),
            Duration::days(1),
        )
        .unwrap();
    let proposal = store
        .create_capture_proposal(NewCaptureProposal {
            owner_id: "owner-a".into(),
            capture_id: capture.id,
            title: "Buy bread".into(),
            category: "task".into(),
            rationale: "The capture asks for bread.".into(),
            expires_at: (Utc::now() + Duration::days(1)).to_rfc3339(),
            audit_id: "task-proposal-2".into(),
        })
        .unwrap();

    assert!(
        store
            .approve_task_proposal(
                "owner-b",
                &proposal.id,
                TaskApprovalStatus::Ready,
                10,
                "cross-owner"
            )
            .is_err()
    );
    assert!(
        store
            .approve_task_proposal(
                "owner-a",
                "missing",
                TaskApprovalStatus::Ready,
                10,
                "missing"
            )
            .is_err()
    );
    assert!(
        store
            .decide_capture_proposal("owner-a", &proposal.id, "approved", "bypass-approval")
            .is_err()
    );
    store
        .decide_capture_proposal("owner-a", &proposal.id, "rejected", "rejected")
        .unwrap();
    assert!(
        store
            .approve_task_proposal(
                "owner-a",
                &proposal.id,
                TaskApprovalStatus::Ready,
                10,
                "after-reject"
            )
            .is_err()
    );
    assert_eq!(store.tasks("owner-a", false, 10).unwrap(), Vec::new());

    let discarded = store
        .create_capture_proposal(NewCaptureProposal {
            owner_id: "owner-a".into(),
            capture_id: store.personal_inbox("owner-a").unwrap()[0].id.clone(),
            title: "Buy eggs".into(),
            category: "task".into(),
            rationale: "The capture implies groceries.".into(),
            expires_at: (Utc::now() + Duration::days(1)).to_rfc3339(),
            audit_id: "task-proposal-3".into(),
        })
        .unwrap();
    store
        .decide_capture_proposal("owner-a", &discarded.id, "discarded", "discarded")
        .unwrap();
    assert!(
        store
            .approve_task_proposal(
                "owner-a",
                &discarded.id,
                TaskApprovalStatus::Ready,
                10,
                "after-discard"
            )
            .is_err()
    );
    assert_eq!(store.tasks("owner-a", false, 10).unwrap(), Vec::new());
}

#[test]
fn expired_proposals_fail_closed_without_creating_a_task() {
    let store = ConversationStore::in_memory().unwrap();
    let capture = store
        .capture_personal_input(
            "owner-a",
            CaptureSource::voice("expired-task-review"),
            "Buy coffee",
            Utc::now(),
            Duration::milliseconds(25),
        )
        .unwrap();
    let proposal = store
        .create_capture_proposal(NewCaptureProposal {
            owner_id: "owner-a".into(),
            capture_id: capture.id,
            title: "Buy coffee".into(),
            category: "task".into(),
            rationale: "The capture asks for coffee.".into(),
            expires_at: capture.expires_at,
            audit_id: "expired-task-proposal".into(),
        })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    assert!(
        store
            .approve_task_proposal(
                "owner-a",
                &proposal.id,
                TaskApprovalStatus::Ready,
                10,
                "expired-task"
            )
            .is_err()
    );
    assert_eq!(store.tasks("owner-a", false, 10).unwrap(), Vec::new());
}

#[test]
fn note_idea_and_worry_approvals_stay_in_the_typed_review_store_without_memory() {
    let store = ConversationStore::in_memory().unwrap();
    for (index, category) in ["note", "idea", "worry"].iter().enumerate() {
        let capture = store
            .capture_personal_input(
                "owner-a",
                CaptureSource::voice(format!("{category}-review-{index}")),
                "A personal thought",
                Utc::now(),
                Duration::days(1),
            )
            .unwrap();
        let proposal = store
            .create_capture_proposal(NewCaptureProposal {
                owner_id: "owner-a".into(),
                capture_id: capture.id,
                title: format!("{category} title"),
                category: (*category).into(),
                rationale: "The capture states this thought.".into(),
                expires_at: capture.expires_at,
                audit_id: format!("{category}-proposal"),
            })
            .unwrap();
        let record = store
            .approve_non_task_proposal("owner-a", &proposal.id, &format!("{category}-approval"))
            .unwrap();
        assert_eq!(record.category, *category);
    }

    assert_eq!(
        store.personal_review_records("owner-a", 10).unwrap().len(),
        3
    );
    assert_eq!(
        store.downstream_record_counts("owner-a").unwrap(),
        (0, 0, 0, 0, 0)
    );
}
