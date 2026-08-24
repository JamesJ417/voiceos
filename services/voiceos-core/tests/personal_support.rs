use chrono::{Duration, Utc};
use voiceos_core::{
    CaptureSource, ConversationStore, DailyFocusReset, NewCaptureProposal, NewPersonalCapture,
    PersonalCapture, ReviewDecision,
};

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
            expires_at: "2026-08-25T00:00:00Z".into(),
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
