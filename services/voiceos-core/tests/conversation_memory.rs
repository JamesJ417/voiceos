use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tempfile::tempdir;
use voiceos_core::{
    ContextClaim, ContextSource, ConversationEngine, ConversationStore, EngineConfig, Provider,
    ProviderCompletion, ProviderError, ProviderRequest, QuarantinedClaim, Role, Usage,
};

#[derive(Default)]
struct RecordingProvider {
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for RecordingProvider {
    fn name(&self) -> &str {
        "recording"
    }

    fn complete(&self, request: &ProviderRequest) -> Result<ProviderCompletion, ProviderError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(ProviderCompletion {
            text: "Recorded response".to_owned(),
            provider: self.name().to_owned(),
            tool_calls: vec![],
            usage: Usage::default(),
        })
    }
}

#[test]
fn device_owns_one_conversation_across_client_sessions() {
    let store = ConversationStore::in_memory().unwrap();
    let first = store
        .resolve_conversation("pixel-1", Some("android-process-a"))
        .unwrap();
    let second = store
        .resolve_conversation("pixel-1", Some("android-process-b"))
        .unwrap();
    let other = store
        .resolve_conversation("kiosk-1", Some("same-alias"))
        .unwrap();
    assert_eq!(first, second);
    assert_ne!(first, other);
}

#[test]
fn owner_shares_one_ordered_conversation_across_devices_and_retries() {
    let store = Arc::new(ConversationStore::in_memory().unwrap());
    store.migrate_devices_to_owner("owner-1").unwrap();
    let engine = ConversationEngine::new(store.clone());
    let (pixel_conversation, _) = engine
        .prepare_owner_turn(
            "owner-1",
            "pixel",
            Some("phone-session"),
            "We are testing shared continuity",
            Some("request-1"),
        )
        .unwrap();
    let (retry_conversation, _) = engine
        .prepare_owner_turn(
            "owner-1",
            "pixel",
            Some("phone-session"),
            "We are testing shared continuity",
            Some("request-1"),
        )
        .unwrap();
    let (kiosk_conversation, kiosk_context) = engine
        .prepare_owner_turn(
            "owner-1",
            "hp-kiosk",
            Some("wall-session"),
            "What are we testing?",
            Some("request-2"),
        )
        .unwrap();
    assert_eq!(pixel_conversation, retry_conversation);
    assert_eq!(pixel_conversation, kiosk_conversation);
    assert_eq!(store.message_count(&pixel_conversation).unwrap(), 2);
    engine
        .record_assistant_from(
            &pixel_conversation,
            "Shared continuity is working.",
            "test-provider",
            "hp-kiosk",
            Some("request-2"),
        )
        .unwrap();
    engine
        .record_assistant_from(
            &pixel_conversation,
            "Shared continuity is working.",
            "test-provider",
            "hp-kiosk",
            Some("request-2"),
        )
        .unwrap();
    assert_eq!(store.message_count(&pixel_conversation).unwrap(), 3);
    assert!(kiosk_context.recent_messages.iter().any(|message| {
        message.role == Role::User && message.content == "We are testing shared continuity"
    }));
    let messages = store.conversation_messages("owner-1", 0, 10).unwrap();
    assert_eq!(messages[0].origin_device_id.as_deref(), Some("pixel"));
    assert_eq!(messages[1].origin_device_id.as_deref(), Some("hp-kiosk"));
    assert_eq!(messages[2].role, Role::Assistant);
}

#[test]
fn provider_receives_recent_context_across_model_independent_turns() {
    let store = Arc::new(ConversationStore::in_memory().unwrap());
    let engine = ConversationEngine::new(store);
    let provider = RecordingProvider::default();
    engine
        .run_turn(
            "pixel",
            Some("one"),
            "My project is VoiceOS",
            vec![],
            &provider,
        )
        .unwrap();
    engine
        .run_turn(
            "pixel",
            Some("two"),
            "What is my project?",
            vec![],
            &provider,
        )
        .unwrap();
    let requests = provider.requests.lock().unwrap();
    let second = &requests[1].messages;
    assert!(second.iter().any(|message| message.role == Role::User && message.content == "My project is VoiceOS"));
    assert!(
        second.iter().any(
            |message| message.role == Role::Assistant && message.content == "Recorded response"
        )
    );
}

#[test]
fn split_gateway_turn_prepares_context_and_records_assistant() {
    let store = Arc::new(ConversationStore::in_memory().unwrap());
    let engine = ConversationEngine::new(store);
    let (conversation_id, first_context) = engine
        .prepare_turn("pixel", Some("phone-session"), "We are building VoiceOS")
        .unwrap();
    assert_eq!(
        first_context.recent_messages.last().unwrap().role,
        Role::User
    );
    engine
        .record_assistant(&conversation_id, "I will remember that.", "gemma")
        .unwrap();
    let (same_conversation, second_context) = engine
        .prepare_turn("pixel", Some("new-phone-session"), "What are we building?")
        .unwrap();
    assert_eq!(conversation_id, same_conversation);
    assert!(
        second_context
            .recent_messages
            .iter()
            .any(|message| message.content == "We are building VoiceOS")
    );
    assert!(
        second_context
            .recent_messages
            .iter()
            .any(|message| message.content == "I will remember that.")
    );
}

#[test]
fn explicit_memories_and_rolling_summaries_are_durable() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("memory.sqlite3");
    {
        let store = Arc::new(ConversationStore::open(&path).unwrap());
        let engine = ConversationEngine::new(store.clone()).with_config(EngineConfig {
            recent_message_limit: 2,
            summary_trigger_messages: 4,
            ..EngineConfig::default()
        });
        let provider = RecordingProvider::default();
        engine
            .run_turn(
                "pixel",
                None,
                "Remember that the HP will host the memory database.",
                vec![],
                &provider,
            )
            .unwrap();
        engine
            .run_turn("pixel", None, "Second turn", vec![], &provider)
            .unwrap();
        engine
            .run_turn("pixel", None, "Third turn", vec![], &provider)
            .unwrap();
        let conversation = store.resolve_conversation("pixel", None).unwrap();
        assert!(store.summary(&conversation).unwrap().is_some());
    }
    let reopened = ConversationStore::open(&path).unwrap();
    let conversation = reopened
        .resolve_conversation("pixel", Some("new-app-session"))
        .unwrap();
    assert!(reopened.summary(&conversation).unwrap().is_some());
    assert_eq!(
        reopened.memories("pixel", 10).unwrap()[0].content,
        "the HP will host the memory database"
    );
}

#[test]
fn legacy_python_audit_can_be_replayed_idempotently() {
    let directory = tempdir().unwrap();
    let legacy_path = directory.path().join("audit.sqlite3");
    let legacy = Connection::open(&legacy_path).unwrap();
    legacy.execute_batch(
        "CREATE TABLE turns(id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, transcript TEXT NOT NULL, response_text TEXT NOT NULL, provider TEXT NOT NULL, created_at TEXT NOT NULL);",
    ).unwrap();
    legacy.execute(
        "INSERT INTO turns VALUES(1, 'old-session', 'What is VoiceOS?', 'A voice-first system.', 'ollama', '2026-08-01T00:00:00Z')",
        [],
    ).unwrap();
    drop(legacy);

    let store = ConversationStore::in_memory().unwrap();
    assert_eq!(
        store
            .import_legacy_audit(&legacy_path, "legacy-owner", "legacy-device")
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .import_legacy_audit(&legacy_path, "legacy-owner", "legacy-device")
            .unwrap(),
        0
    );
    let conversation = store
        .resolve_owner_conversation("legacy-owner", "legacy-device", Some("new-session"))
        .unwrap();
    let messages = store.recent_messages(&conversation, 10).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "What is VoiceOS?");
    assert_eq!(messages[1].content, "A voice-first system.");
}

#[test]
fn uploaded_documents_are_private_searchable_and_deletable() {
    let store = ConversationStore::in_memory().unwrap();
    let profile = store
        .ingest_text_document(
            "pixel",
            "about-me.md",
            "text/markdown",
            "profile",
            b"I prefer concise spoken answers. My workshop project is called VoiceOS.",
        )
        .unwrap();
    let reference = store
        .ingest_text_document(
            "pixel",
            "equipment.txt",
            "text/plain",
            "reference",
            b"The mining rig uses an RTX 5060 Ti with 16 GB of VRAM.",
        )
        .unwrap();
    store
        .ingest_text_document(
            "other-device",
            "private.txt",
            "text/plain",
            "profile",
            b"This must never appear for the Pixel.",
        )
        .unwrap();

    let context = store
        .relevant_document_context("pixel", "Which GPU is in the mining rig?", 6, 8_000)
        .unwrap()
        .unwrap();
    assert!(context.contains("about-me.md"));
    assert!(context.contains("RTX 5060 Ti"));
    assert!(!context.contains("never appear"));
    assert_eq!(store.list_documents("pixel").unwrap().len(), 2);
    assert!(store.delete_document("pixel", &reference.id).unwrap());
    assert!(!store.delete_document("pixel", &reference.id).unwrap());
    assert_eq!(store.list_documents("pixel").unwrap(), vec![profile]);
}

#[test]
fn rejected_claims_are_durable_and_retrievable_with_provenance_reason_and_timestamp() {
    let store = ConversationStore::in_memory().unwrap();
    store.migrate_devices_to_owner("owner-1").unwrap();
    let conversation = store
        .resolve_owner_conversation("owner-1", "pixel", None)
        .unwrap();
    let claim = ContextClaim::new(
        "bad",
        &conversation,
        ContextSource::ConversationSummary,
        "old summary",
    )
    .with_metadata("summary://source-1", 0.7, 0.4);
    store
        .quarantine_claims(
            &conversation,
            &[QuarantinedClaim {
                claim,
                reason: "stale summary".into(),
            }],
        )
        .unwrap();
    let records = store
        .quarantined_claims_for_owner("owner-1", &conversation)
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].claim_id, "bad");
    assert_eq!(records[0].provenance, "summary://source-1");
    assert_eq!(records[0].reason, "stale summary");
    assert!(!records[0].created_at.is_empty());
}

#[test]
fn owner_context_retrieval_is_conversation_scoped_for_summaries_and_memories() {
    let store = ConversationStore::in_memory().unwrap();
    store.migrate_devices_to_owner("owner-1").unwrap();
    let conversation = store
        .resolve_owner_conversation("owner-1", "pixel", None)
        .unwrap();
    store
        .save_summary_for_owner("owner-1", &conversation, "current summary", 1)
        .unwrap();
    store
        .remember_for_owner_in_conversation(
            "owner-1",
            "pixel",
            &conversation,
            "current memory",
            "test",
        )
        .unwrap();
    let other = "other-conversation";
    assert!(store.summary_for_owner("owner-1", other).unwrap().is_none());
    assert!(
        store
            .memories_for_owner_conversation("owner-1", other, 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .context_for_owner("owner-1", other, "query", 2, 10)
            .is_err()
    );
}

#[test]
fn compression_persists_owner_scoped_summary_metadata_across_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("compression-metadata.sqlite3");
    let conversation_id;
    {
        let store = Arc::new(ConversationStore::open(&path).unwrap());
        let engine = ConversationEngine::new(store.clone()).with_config(EngineConfig {
            recent_message_limit: 1,
            summary_trigger_messages: 2,
            ..EngineConfig::default()
        });
        let provider = RecordingProvider::default();
        conversation_id = engine
            .run_owner_turn(
                "owner-1",
                "pixel",
                Some("session"),
                "First scoped turn",
                vec![],
                &provider,
            )
            .unwrap()
            .0;
        engine
            .run_owner_turn(
                "owner-1",
                "pixel",
                Some("session"),
                "Second scoped turn",
                vec![],
                &provider,
            )
            .unwrap();
    }

    let connection = Connection::open(&path).unwrap();
    let metadata: (String, String) = connection
        .query_row(
            "SELECT owner_id, provenance FROM conversation_summaries WHERE conversation_id=?1",
            [&conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(metadata.0, "owner-1");
    assert_eq!(
        metadata.1,
        format!("conversation-summary://{conversation_id}")
    );
    drop(connection);

    let reopened = ConversationStore::open(&path).unwrap();
    let context = reopened
        .context_for_owner("owner-1", &conversation_id, "resume", 1, 1)
        .unwrap();
    assert!(context.summary.unwrap().contains("First scoped turn"));
}

#[test]
fn reopened_store_rejects_recovery_of_an_archived_summary() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("recovery-isolation.sqlite3");
    let archived_conversation;
    {
        let store = ConversationStore::open(&path).unwrap();
        archived_conversation = store
            .resolve_owner_conversation("owner-1", "pixel", Some("old-session"))
            .unwrap();
        store
            .save_summary_for_owner("owner-1", &archived_conversation, "old-session summary", 1)
            .unwrap();
    }
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE conversations SET status='archived' WHERE conversation_id=?1",
            [&archived_conversation],
        )
        .unwrap();
    drop(connection);

    let reopened = ConversationStore::open(&path).unwrap();
    let active_conversation = reopened
        .resolve_owner_conversation("owner-1", "pixel", Some("new-session"))
        .unwrap();
    assert_ne!(active_conversation, archived_conversation);
    assert!(
        reopened
            .context_for_owner("owner-1", &archived_conversation, "resume", 10, 10)
            .is_err()
    );
}

#[test]
fn image_attachment_is_owner_scoped_and_claimed_once_with_an_idempotent_turn() {
    let store = Arc::new(ConversationStore::in_memory().unwrap());
    store.migrate_devices_to_owner("owner-1").unwrap();
    let attachment = store
        .ingest_attachment_for_owner(
            "owner-1",
            "pixel",
            "kitchen.jpg",
            "image/jpeg",
            b"\xff\xd8\xfftest-image",
        )
        .unwrap();
    let conversation_id = store
        .resolve_owner_conversation("owner-1", "pixel", Some("phone-session"))
        .unwrap();
    let message_id = store
        .claim_attachments_for_owner_turn(
            "owner-1",
            "pixel",
            &conversation_id,
            "What is in this photo?",
            Some("image-turn-1"),
            &[attachment.id.clone()],
        )
        .unwrap();
    store
        .claim_attachments_for_owner_turn(
            "owner-1",
            "pixel",
            &conversation_id,
            "What is in this photo?",
            Some("image-turn-1"),
            &[attachment.id.clone()],
        )
        .unwrap();

    assert_eq!(store.message_count(&conversation_id).unwrap(), 1);
    let mut expected = attachment.clone();
    expected.status = "attached".to_owned();
    assert_eq!(
        store.attachments_for_message(message_id).unwrap(),
        vec![expected.clone()]
    );
    let messages = store.recent_conversation_messages("owner-1", 10).unwrap();
    assert_eq!(messages[0].attachments, vec![expected]);
    assert!(
        store
            .claim_attachments_for_owner_turn(
                "owner-1",
                "pixel",
                &conversation_id,
                "same request, different attachments",
                Some("image-turn-1"),
                &[],
            )
            .is_err()
    );
    assert!(
        store
            .claim_attachments_for_owner_turn(
                "other-owner",
                "other-device",
                &conversation_id,
                "should fail",
                Some("cross-owner-request"),
                &[attachment.id.clone()],
            )
            .is_err()
    );
}
