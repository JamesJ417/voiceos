use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tempfile::tempdir;
use voiceos_core::{
    ConversationEngine, ConversationStore, EngineConfig, Provider, ProviderCompletion,
    ProviderError, ProviderRequest, Role, StoreError, Usage,
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
fn finalized_owner_attachment_is_claimed_once_by_an_idempotent_user_turn() {
    let store = Arc::new(ConversationStore::in_memory().unwrap());
    store.migrate_devices_to_owner("owner-1").unwrap();
    let engine = ConversationEngine::new(store.clone());
    {
        let connection = store.connection().unwrap();
        connection
            .execute(
                "INSERT INTO attachments(attachment_id, upload_id, owner_id, filename, media_type, byte_size, sha256, bytes, status, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    "attachment-ready",
                    "upload-1",
                    "owner-1",
                    "photo.png",
                    "image/png",
                    3,
                    "abc",
                    b"png".as_slice(),
                    "ready",
                    "2026-08-23T00:00:00Z",
                ],
            )
            .unwrap();
    }

    let (conversation_id, _) = engine
        .prepare_owner_turn_with_attachments(
            "owner-1",
            "pixel",
            Some("phone-session"),
            "What is in this image?",
            Some("request-with-image"),
            &["attachment-ready".to_owned()],
        )
        .unwrap();
    engine
        .prepare_owner_turn_with_attachments(
            "owner-1",
            "pixel",
            Some("phone-session"),
            "What is in this image?",
            Some("request-with-image"),
            &["attachment-ready".to_owned()],
        )
        .unwrap();

    let connection = store.connection().unwrap();
    let user_message_id: i64 = connection
        .query_row(
            "SELECT message_id FROM messages WHERE conversation_id=?1 AND request_id=?2",
            rusqlite::params![&conversation_id, "request-with-image"],
            |row| row.get(0),
        )
        .unwrap();
    let claims: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM message_attachments WHERE message_id=?1 AND attachment_id=?2",
            rusqlite::params![user_message_id, "attachment-ready"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(claims, 1);
    drop(connection);
    assert_eq!(store.message_count(&conversation_id).unwrap(), 1);
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
            .import_legacy_audit(&legacy_path, "legacy-device", "legacy-device")
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .import_legacy_audit(&legacy_path, "legacy-device", "legacy-device")
            .unwrap(),
        0
    );
    let conversation = store
        .resolve_conversation("legacy-device", Some("new-session"))
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
fn conflicting_owner_for_an_active_device_returns_an_explicit_error() {
    let store = ConversationStore::in_memory().unwrap();
    store
        .resolve_owner_conversation("primary-owner", "legacy-audit", None)
        .unwrap();

    let error = store
        .resolve_owner_conversation("different-owner", "legacy-audit", Some("new-session"))
        .unwrap_err();

    assert!(
        matches!(error, StoreError::InvalidInput(message) if message.contains("active device belongs to a different owner"))
    );
}

#[test]
fn importing_legacy_turns_reuses_the_primary_owner_conversation() {
    let directory = tempdir().unwrap();
    let legacy_path = directory.path().join("audit.sqlite3");
    let legacy = Connection::open(&legacy_path).unwrap();
    legacy
        .execute_batch(
            "CREATE TABLE turns(id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, transcript TEXT NOT NULL, response_text TEXT NOT NULL, provider TEXT NOT NULL, created_at TEXT NOT NULL);",
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO turns VALUES(1, 'legacy-session', 'remember this', 'recorded', 'mock', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    drop(legacy);

    let store = ConversationStore::in_memory().unwrap();
    store.migrate_devices_to_owner("primary-owner").unwrap();
    let conversation_id = store
        .resolve_owner_conversation("primary-owner", "legacy-audit", None)
        .unwrap();

    assert_eq!(
        store
            .import_legacy_audit(&legacy_path, "primary-owner", "legacy-audit")
            .unwrap(),
        1
    );
    let connection = store.connection().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT conversation_id FROM conversation_aliases WHERE device_id='legacy-audit' AND client_session_id='legacy:legacy-session'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        conversation_id,
    );
    let imported_messages: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id=?1",
            [&conversation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(imported_messages, 2);
    let violations: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}
