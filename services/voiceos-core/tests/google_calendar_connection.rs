use voiceos_core::{ConversationStore, GoogleCalendarConnection};

#[test]
fn stores_owner_scoped_calendar_metadata_without_refresh_token() {
    let store = ConversationStore::in_memory().unwrap();
    store
        .upsert_google_calendar_connection("owner-a", "google", "account@example.com", "acct-123")
        .unwrap();

    let connection = store
        .google_calendar_connection_for_owner("owner-a")
        .unwrap()
        .unwrap();
    assert_eq!(
        connection,
        GoogleCalendarConnection {
            owner_id: "owner-a".into(),
            provider: "google".into(),
            account_email: "account@example.com".into(),
            provider_account_id: "acct-123".into(),
            secret_reference: None,
        }
    );
    assert!(
        store
            .google_calendar_connection_for_owner("owner-b")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE sql LIKE '%refresh_token%'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap()
            == 0
    );

    assert!(store.disconnect_google_calendar("owner-a").unwrap());
    assert!(
        store
            .google_calendar_connection_for_owner("owner-a")
            .unwrap()
            .is_none()
    );
}
