use voiceos_core::{
    FieldyTranscriptEvent, FieldyWebhookError, FieldyWebhookStore, verify_fieldy_signature,
};

#[test]
fn verifies_sha256_signature_and_rejects_tampering() {
    let body = br#"{"event_id":"evt-1","transcript":"hello"}"#;
    let signature = "sha256=9f2c3f9d4f4f5c3a9e2f1b6d6a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b";
    assert!(!verify_fieldy_signature(b"secret", body, signature));
    let valid = "sha256=1";
    assert!(!verify_fieldy_signature(b"secret", body, valid));
}

#[test]
fn accepts_a_real_hmac_signature() {
    let body = br#"hello"#;
    let signature = "sha256=88aab3ede8d3adf94d26ab90d3bafd4a2083070c3bcce9c014ee04a443847c0b";
    assert!(verify_fieldy_signature(b"secret", body, signature));
}

#[test]
fn rejects_invalid_event_contract() {
    let empty = FieldyTranscriptEvent {
        event_id: " ".into(),
        occurred_at: "2026-08-24T12:00:00Z".into(),
        transcript: " ".into(),
        recording_id: None,
        session_id: None,
        speakers: vec![],
        metadata: serde_json::json!({}),
    };
    assert!(matches!(
        empty.validate(),
        Err(FieldyWebhookError::InvalidInput(_))
    ));
}

#[test]
fn stores_event_once_and_returns_same_intake_for_duplicate() {
    let store = FieldyWebhookStore::in_memory().unwrap();
    let event = FieldyTranscriptEvent {
        event_id: "evt-1".into(),
        occurred_at: "2026-08-24T12:00:00Z".into(),
        transcript: "  hello Fieldy  ".into(),
        recording_id: Some("rec-1".into()),
        session_id: None,
        speakers: vec![],
        metadata: serde_json::json!({"x":1}),
    };
    let first = store.intake("owner-1", &event, br#"{"raw":true}"#).unwrap();
    let second = store.intake("owner-1", &event, br#"{"raw":true}"#).unwrap();
    assert_eq!(first.intake_id, second.intake_id);
    assert_eq!(store.count("owner-1").unwrap(), 1);
    assert_eq!(first.normalized_transcript, "hello Fieldy");
}

#[test]
fn duplicate_event_is_scoped_to_owner() {
    let store = FieldyWebhookStore::in_memory().unwrap();
    let event = FieldyTranscriptEvent {
        event_id: "evt-1".into(),
        occurred_at: "2026-08-24T12:00:00Z".into(),
        transcript: "hello".into(),
        recording_id: None,
        session_id: None,
        speakers: vec![],
        metadata: serde_json::json!({}),
    };
    let a = store.intake("a", &event, b"{}").unwrap();
    let b = store.intake("b", &event, b"{}").unwrap();
    assert_ne!(a.intake_id, b.intake_id);
    assert_eq!(store.count("a").unwrap(), 1);
    assert_eq!(store.count("b").unwrap(), 1);
}

#[test]
fn intake_uses_a_future_default_expiry() {
    let store = FieldyWebhookStore::in_memory().unwrap();
    let event = FieldyTranscriptEvent {
        event_id: "evt-expiry".into(),
        occurred_at: "2026-08-24T12:00:00Z".into(),
        transcript: "hello".into(),
        recording_id: None,
        session_id: None,
        speakers: vec![],
        metadata: serde_json::json!({}),
    };
    let intake = store.intake("owner-1", &event, b"{}").unwrap();
    assert!(store.expires_at(&intake.intake_id).unwrap() > chrono::Utc::now());
}
