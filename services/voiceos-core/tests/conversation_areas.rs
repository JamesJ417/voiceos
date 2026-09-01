use chrono::{Duration, Utc};
use rusqlite::Connection;
use voiceos_core::{
    ConversationStore, ConversationSyncRecord, GENERAL_TALK_AREA_ID, Role, StoreError,
};

const OWNER: &str = "owner-a";
const DEVICE: &str = "device-a";

#[test]
fn file_backed_legacy_upgrade_preserves_sleep_cycle_data_across_two_opens() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy-sleep-cycle-upgrade.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE owners(owner_id TEXT PRIMARY KEY,display_name TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
                         CREATE TABLE devices(device_id TEXT PRIMARY KEY,display_name TEXT,created_at TEXT NOT NULL,last_seen_at TEXT NOT NULL);
                         CREATE TABLE conversations(conversation_id TEXT PRIMARY KEY,device_id TEXT NOT NULL,status TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
                         CREATE TABLE messages(message_id INTEGER PRIMARY KEY AUTOINCREMENT,conversation_id TEXT NOT NULL,role TEXT NOT NULL,content TEXT NOT NULL,provider TEXT,legacy_turn_id INTEGER,created_at TEXT NOT NULL);
                         CREATE TABLE memories(memory_id TEXT PRIMARY KEY,device_id TEXT NOT NULL,conversation_id TEXT,normalized_content TEXT NOT NULL,content TEXT NOT NULL,source TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
             CREATE TABLE jobs(job_id TEXT PRIMARY KEY,owner_id TEXT NOT NULL,task_id TEXT,status TEXT NOT NULL,idempotency_key TEXT NOT NULL,capability_scope_json TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
             INSERT INTO owners VALUES('owner-a','Owner A','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO devices VALUES('device-a','Device A','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO conversations VALUES('legacy-conversation','device-a','active','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO messages(conversation_id,role,content,provider,created_at) VALUES('legacy-conversation','user','preserve this message','legacy-provider','2026-08-01T00:00:00Z');
             INSERT INTO memories(memory_id,device_id,conversation_id,normalized_content,content,source,created_at,updated_at) VALUES('legacy-memory','device-a','legacy-conversation','preserve this memory','preserve this memory','legacy','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO jobs VALUES('legacy-job','owner-a',NULL,'paused','legacy-job-key','[\"filesystem.read\"]','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');",
        )
        .unwrap();
    drop(connection);

    for (iteration, _) in (0..2).enumerate() {
        let store = ConversationStore::open(&path).unwrap();
        store.migrate_devices_to_owner(OWNER).unwrap();
        let conversation = store
            .conversation_for_owner(OWNER, "legacy-conversation")
            .unwrap()
            .unwrap();
        assert_eq!(conversation.area_id, "general-talk");
        assert_eq!(conversation.message_count, 1);
        assert_eq!(store.conversation_messages(OWNER, 0, 10).unwrap().len(), 1);
        assert_eq!(
            store.job(OWNER, "legacy-job").unwrap().unwrap().status,
            if iteration == 0 { "paused" } else { "running" }
        );
        let checkpoint = store
            .checkpoint_execution(
                OWNER,
                "legacy-job",
                serde_json::json!({"cursor": 1}),
                serde_json::json!({"undo": "none"}),
            )
            .unwrap();
        assert_eq!(checkpoint.sequence, iteration as i64 + 1);
        assert_eq!(
            store
                .resume_execution(OWNER, "legacy-job")
                .unwrap()
                .unwrap()
                .sequence,
            iteration as i64 + 1
        );
        assert_eq!(
            store.job(OWNER, "legacy-job").unwrap().unwrap().status,
            "running"
        );
    }

    let connection = Connection::open(&path).unwrap();
    let counts: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM conversations),(SELECT COUNT(*) FROM messages),(SELECT COUNT(*) FROM memories),(SELECT COUNT(*) FROM execution_checkpoints)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 1, 1, 2));
    let memory_area: Option<String> = connection
        .query_row(
            "SELECT area_id FROM memories WHERE memory_id='legacy-memory'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(memory_area.as_deref(), Some("general-talk"));
    drop(connection);
}

#[test]
fn seeds_six_areas_and_migrates_legacy_conversations_idempotently() {
    let (_directory, path) = temporary_database("area-migration");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE devices(device_id TEXT PRIMARY KEY,display_name TEXT,created_at TEXT NOT NULL,last_seen_at TEXT NOT NULL);
             CREATE TABLE conversations(conversation_id TEXT PRIMARY KEY,device_id TEXT NOT NULL,status TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
             CREATE TABLE messages(message_id INTEGER PRIMARY KEY AUTOINCREMENT,conversation_id TEXT NOT NULL,role TEXT NOT NULL,content TEXT NOT NULL,provider TEXT,legacy_turn_id INTEGER,created_at TEXT NOT NULL);
             INSERT INTO devices VALUES('legacy-device',NULL,'2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO conversations VALUES('legacy-conversation','legacy-device','active','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO messages(conversation_id,role,content,created_at) VALUES('legacy-conversation','user','do not lose me','2026-08-01T00:00:00Z');",
        )
        .unwrap();
    drop(connection);

    for _ in 0..2 {
        let store = ConversationStore::open(&path).unwrap();
        store.migrate_devices_to_owner(OWNER).unwrap();
        let record = store
            .conversation_for_owner(OWNER, "legacy-conversation")
            .unwrap()
            .unwrap();
        assert_eq!(record.area_id, GENERAL_TALK_AREA_ID);
        assert_eq!(record.message_count, 1);
        assert_eq!(store.conversation_areas().len(), 6);
    }
}

#[test]
fn upgrading_legacy_memories_preserves_content_and_leaves_unlinked_records_unassigned() {
    let (_directory, path) = temporary_database("legacy-memory-area-migration");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE devices(device_id TEXT PRIMARY KEY,display_name TEXT,created_at TEXT NOT NULL,last_seen_at TEXT NOT NULL);
             CREATE TABLE conversations(conversation_id TEXT PRIMARY KEY,device_id TEXT NOT NULL,status TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
             CREATE TABLE memories(memory_id TEXT PRIMARY KEY,device_id TEXT NOT NULL,normalized_content TEXT NOT NULL,content TEXT NOT NULL,source TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,FOREIGN KEY(device_id) REFERENCES devices(device_id));
             INSERT INTO devices VALUES('legacy-device',NULL,'2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO conversations VALUES('legacy-conversation','legacy-device','active','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');
             INSERT INTO memories VALUES('legacy-memory','legacy-device','remember this','remember this','legacy','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z');",
        )
        .unwrap();
    drop(connection);

    for _ in 0..2 {
        let store = ConversationStore::open(&path).unwrap();
        drop(store);
    }

    let connection = Connection::open(&path).unwrap();
    let area_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('memories') WHERE name = 'area_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(area_column_count, 1);
    let (content, area_id): (String, Option<String>) = connection
        .query_row(
            "SELECT content, area_id FROM memories WHERE memory_id = 'legacy-memory'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(content, "remember this");
    assert_eq!(area_id, None);
    let duplicate = connection.execute(
        "INSERT INTO memories(memory_id,device_id,normalized_content,content,source,created_at,updated_at) VALUES('duplicate','legacy-device','remember this','duplicate','test','2026-08-01T00:00:00Z','2026-08-01T00:00:00Z')",
        [],
    );
    assert!(duplicate.is_err());
    drop(connection);
}

#[test]
fn create_select_and_confirmed_move_are_idempotent_and_audited() {
    let store = ConversationStore::in_memory().unwrap();
    let created = store
        .create_conversation_in_area(
            OWNER,
            DEVICE,
            "brick-copper",
            Some("Opening checklist"),
            "create-1",
        )
        .unwrap();
    let duplicate = store
        .create_conversation_in_area(
            OWNER,
            DEVICE,
            "brick-copper",
            Some("ignored duplicate"),
            "create-1",
        )
        .unwrap();
    assert_eq!(created.id, duplicate.id);

    let unconfirmed = store.move_conversation_for_owner(
        OWNER,
        DEVICE,
        &created.id,
        "brick-copper",
        "personal",
        false,
        "move-1",
    );
    assert!(matches!(unconfirmed, Err(StoreError::InvalidInput(_))));
    assert_eq!(
        store
            .conversation_for_owner(OWNER, &created.id)
            .unwrap()
            .unwrap()
            .area_id,
        "brick-copper"
    );

    let moved = store
        .move_conversation_for_owner(
            OWNER,
            DEVICE,
            &created.id,
            "brick-copper",
            "personal",
            true,
            "move-1",
        )
        .unwrap();
    let duplicate_move = store
        .move_conversation_for_owner(
            OWNER,
            DEVICE,
            &created.id,
            "brick-copper",
            "personal",
            true,
            "move-1",
        )
        .unwrap();
    assert_eq!(moved, duplicate_move);
    assert_eq!(moved.area_id, "personal");
    let events: i64 = store
        .connection()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM execution_events WHERE event_type='conversation.moved' AND stream_id=?1",
            [&created.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 1);
}

#[test]
fn area_context_does_not_retrieve_memory_from_another_area() {
    let store = ConversationStore::in_memory().unwrap();
    let personal = store
        .create_conversation_in_area(OWNER, DEVICE, "personal", None, "personal-create")
        .unwrap();
    store
        .remember_for_owner_in_conversation(
            OWNER,
            DEVICE,
            &personal.id,
            "My private restart action is open the notebook",
            "explicit-user-request",
        )
        .unwrap();
    let personal_second = store
        .create_conversation_in_area(OWNER, DEVICE, "personal", None, "personal-second")
        .unwrap();
    let personal_context = store
        .context_for_owner(OWNER, &personal_second.id, "restart", 10, 10)
        .unwrap();
    assert_eq!(personal_context.memories.len(), 1);

    let business = store
        .create_conversation_in_area(OWNER, DEVICE, "brick-copper", None, "business-create")
        .unwrap();
    let business_context = store
        .context_for_owner(OWNER, &business.id, "restart", 10, 10)
        .unwrap();
    assert!(business_context.memories.is_empty());
}

#[test]
fn export_import_round_trip_is_validated_and_idempotent() {
    let store = ConversationStore::in_memory().unwrap();
    let original = store
        .create_conversation_in_area(
            OWNER,
            DEVICE,
            "religious-biblical",
            Some("Psalm notes"),
            "create-export",
        )
        .unwrap();
    store
        .append_message(&original.id, Role::User, "Read Psalm 23", None)
        .unwrap();
    store
        .append_message(
            &original.id,
            Role::Assistant,
            "The psalm opens with trust.",
            Some("test"),
        )
        .unwrap();
    let export = store.export_conversation(OWNER, &original.id).unwrap();
    assert!(uuid::Uuid::parse_str(&export.export_id).is_ok());
    let imported = store
        .import_conversation(OWNER, DEVICE, "import-1", &export)
        .unwrap();
    let duplicate = store
        .import_conversation(OWNER, DEVICE, "import-1", &export)
        .unwrap();
    assert_eq!(imported.id, duplicate.id);
    assert_ne!(imported.id, original.id);
    assert_eq!(imported.area_id, "religious-biblical");
    assert_eq!(imported.message_count, 2);

    let mut invalid = export;
    invalid.area_id = "made-up".to_owned();
    assert!(matches!(
        store.import_conversation(OWNER, DEVICE, "import-invalid", &invalid),
        Err(StoreError::InvalidInput(_))
    ));
}

#[test]
fn synchronization_uses_timestamp_then_device_as_a_deterministic_tiebreaker() {
    let store = ConversationStore::in_memory().unwrap();
    let conversation = store
        .create_conversation_in_area(OWNER, DEVICE, "personal", None, "sync-create")
        .unwrap();
    let newer = (Utc::now() + Duration::seconds(60)).to_rfc3339();
    let first = ConversationSyncRecord {
        conversation_id: conversation.id.clone(),
        area_id: "general-talk".to_owned(),
        area_updated_at: newer.clone(),
        area_updated_by_device: "device-b".to_owned(),
    };
    assert_eq!(
        store
            .apply_conversation_sync(OWNER, DEVICE, &[first])
            .unwrap(),
        1
    );
    let losing_tie = ConversationSyncRecord {
        conversation_id: conversation.id.clone(),
        area_id: "brick-copper".to_owned(),
        area_updated_at: newer.clone(),
        area_updated_by_device: "device-a".to_owned(),
    };
    assert_eq!(
        store
            .apply_conversation_sync(OWNER, DEVICE, &[losing_tie])
            .unwrap(),
        0
    );
    let winning_tie = ConversationSyncRecord {
        conversation_id: conversation.id.clone(),
        area_id: "vine-branch-deli".to_owned(),
        area_updated_at: newer,
        area_updated_by_device: "device-z".to_owned(),
    };
    assert_eq!(
        store
            .apply_conversation_sync(OWNER, DEVICE, &[winning_tie])
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .conversation_for_owner(OWNER, &conversation.id)
            .unwrap()
            .unwrap()
            .area_id,
        "vine-branch-deli"
    );
}

#[test]
fn calendar_history_is_cross_area_with_an_optional_area_filter() {
    let store = ConversationStore::in_memory().unwrap();
    let general = store
        .create_conversation_in_area(OWNER, DEVICE, "general-talk", None, "history-general")
        .unwrap();
    store
        .append_message(&general.id, Role::User, "hello", None)
        .unwrap();
    let personal = store
        .create_conversation_in_area(OWNER, DEVICE, "personal", None, "history-personal")
        .unwrap();
    store
        .append_message(&personal.id, Role::User, "journal", None)
        .unwrap();

    let all = store.conversation_history_days(OWNER, None, 0, 30).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].conversations.len(), 2);
    let personal_only = store
        .conversation_history_days(OWNER, Some("personal"), 0, 30)
        .unwrap();
    assert_eq!(personal_only[0].conversations.len(), 1);
    assert_eq!(personal_only[0].conversations[0].area_id, "personal");
}

#[test]
fn calendar_history_uses_the_requested_timezone_across_midnight_without_duplicates() {
    let store = ConversationStore::in_memory().unwrap();
    let conversation = store
        .create_conversation_in_area(OWNER, DEVICE, "general-talk", None, "midnight-create")
        .unwrap();
    let first = store
        .append_message(&conversation.id, Role::User, "before midnight", None)
        .unwrap();
    let second = store
        .append_message(&conversation.id, Role::Assistant, "after midnight", None)
        .unwrap();
    let connection = store.connection().unwrap();
    connection
        .execute(
            "UPDATE messages SET created_at='2026-08-30T23:59:59Z' WHERE message_id=?1",
            [first],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE messages SET created_at='2026-08-31T00:00:01Z' WHERE message_id=?1",
            [second],
        )
        .unwrap();
    drop(connection);

    let utc = store.conversation_history_days(OWNER, None, 0, 30).unwrap();
    assert_eq!(
        utc.iter().map(|day| day.date.as_str()).collect::<Vec<_>>(),
        vec!["2026-08-31", "2026-08-30"]
    );
    assert!(utc.iter().all(|day| day.conversations.len() == 1));
    let eastern = store
        .conversation_history_days(OWNER, None, -240, 30)
        .unwrap();
    assert_eq!(eastern.len(), 1);
    assert_eq!(eastern[0].date, "2026-08-30");
    assert_eq!(eastern[0].conversations.len(), 1);
}

#[test]
fn idempotency_request_id_cannot_be_reused_for_another_operation() {
    let store = ConversationStore::in_memory().unwrap();
    let conversation = store
        .create_conversation_in_area(OWNER, DEVICE, "personal", None, "reused-request")
        .unwrap();

    let result = store.move_conversation_for_owner(
        OWNER,
        DEVICE,
        &conversation.id,
        "personal",
        "brick-copper",
        true,
        "reused-request",
    );

    assert!(
        matches!(result, Err(StoreError::InvalidInput(message)) if message == "request ID was already used for a different operation")
    );
}

fn temporary_database(label: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join(format!("voiceos-{label}-{}.sqlite", uuid::Uuid::new_v4()));
    (directory, path)
}
