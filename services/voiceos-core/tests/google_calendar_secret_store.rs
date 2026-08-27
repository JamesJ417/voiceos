use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use voiceos_core::{
    CalendarSecretReference, CalendarSecretStore, ConversationStore, InMemoryCalendarSecretStore,
    SecretServiceBackend, SecretServiceBackendError, SecretToolCalendarSecretStore,
    UnavailableCalendarSecretStore,
};

#[derive(Default)]
struct FakeSecretService {
    entries: Mutex<HashMap<(String, String, String), Vec<u8>>>,
}

impl SecretServiceBackend for FakeSecretService {
    fn is_available(&self) -> bool {
        true
    }

    fn store(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
        secret: &[u8],
    ) -> Result<(), SecretServiceBackendError> {
        self.entries.lock().unwrap().insert(
            (
                owner_id.to_owned(),
                integration_key.to_owned(),
                reference.as_str().to_owned(),
            ),
            secret.to_vec(),
        );
        Ok(())
    }

    fn lookup(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, SecretServiceBackendError> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .get(&(
                owner_id.to_owned(),
                integration_key.to_owned(),
                reference.as_str().to_owned(),
            ))
            .cloned())
    }

    fn delete(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
    ) -> Result<bool, SecretServiceBackendError> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .remove(&(
                owner_id.to_owned(),
                integration_key.to_owned(),
                reference.as_str().to_owned(),
            ))
            .is_some())
    }
}

#[test]
fn os_secret_service_adapter_writes_reads_and_deletes_opaque_credential() {
    let secrets = SecretToolCalendarSecretStore::with_backend(FakeSecretService::default());
    let credential = b"test-only-calendar-credential";

    let reference = secrets.put("owner-a", credential).unwrap();
    assert_eq!(
        secrets.get("owner-a", &reference).unwrap(),
        Some(credential.to_vec())
    );
    assert!(secrets.delete("owner-a", &reference).unwrap());
    assert_eq!(secrets.get("owner-a", &reference).unwrap(), None);
}

#[test]
fn os_secret_service_adapter_returns_missing_for_another_owner() {
    let secrets = SecretToolCalendarSecretStore::with_backend(FakeSecretService::default());
    let reference = secrets
        .put("owner-a", b"test-only-calendar-credential")
        .unwrap();

    assert_eq!(secrets.get("owner-b", &reference).unwrap(), None);
    assert!(!secrets.delete("owner-b", &reference).unwrap());
    assert!(secrets.get("owner-a", &reference).unwrap().is_some());
}

struct FailingSecretService;

impl SecretServiceBackend for FailingSecretService {
    fn is_available(&self) -> bool {
        true
    }

    fn store(
        &self,
        _owner_id: &str,
        _integration_key: &str,
        _reference: &CalendarSecretReference,
        _secret: &[u8],
    ) -> Result<(), SecretServiceBackendError> {
        Err(SecretServiceBackendError::OperationFailed)
    }

    fn lookup(
        &self,
        _owner_id: &str,
        _integration_key: &str,
        _reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, SecretServiceBackendError> {
        Err(SecretServiceBackendError::OperationFailed)
    }

    fn delete(
        &self,
        _owner_id: &str,
        _integration_key: &str,
        _reference: &CalendarSecretReference,
    ) -> Result<bool, SecretServiceBackendError> {
        Err(SecretServiceBackendError::OperationFailed)
    }
}

#[test]
fn os_secret_service_adapter_fails_closed_with_value_free_errors() {
    let secrets = SecretToolCalendarSecretStore::with_backend(FailingSecretService);
    let credential = b"test-only-calendar-credential";
    let reference =
        CalendarSecretStore::put(&InMemoryCalendarSecretStore::new(), "owner-a", &[]).unwrap();

    for error in [
        secrets.put("owner-a", credential).unwrap_err(),
        secrets.get("owner-a", &reference).unwrap_err(),
        secrets.delete("owner-a", &reference).unwrap_err(),
    ] {
        assert_eq!(
            error,
            voiceos_core::CalendarSecretStoreError::OperationFailed
        );
        assert!(!error.to_string().contains("test-only-calendar-credential"));
        assert!(!format!("{error:?}").contains("test-only-calendar-credential"));
    }
}

#[test]
fn calendar_secret_references_are_owner_scoped() {
    let secrets = InMemoryCalendarSecretStore::new();
    let reference = secrets.put("owner-a", &[]).unwrap();

    assert_eq!(
        secrets.get("owner-a", &reference).unwrap(),
        Some(Vec::new())
    );
    assert_eq!(secrets.get("owner-b", &reference).unwrap(), None);
}

#[test]
fn calendar_metadata_persists_only_an_opaque_secret_reference() {
    let store = ConversationStore::in_memory().unwrap();
    let secrets = InMemoryCalendarSecretStore::new();
    let reference = secrets.put("owner-a", &[]).unwrap();
    store
        .upsert_google_calendar_connection("owner-a", "google", "account@example.com", "acct-123")
        .unwrap();
    store
        .set_google_calendar_secret_reference("owner-a", &reference)
        .unwrap();

    let connection = store
        .google_calendar_connection_for_owner("owner-a")
        .unwrap()
        .unwrap();
    assert_eq!(connection.secret_reference.as_ref(), Some(&reference));
    assert!(reference.as_str().starts_with("gcal_secret_"));
    assert!(
        store
            .connection()
            .unwrap()
            .query_row(
                "SELECT secret_reference FROM google_calendar_connections WHERE owner_id='owner-a'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
            .starts_with("gcal_secret_")
    );
}

#[test]
fn disconnect_removes_owner_metadata_and_secret_entry() {
    let store = ConversationStore::in_memory().unwrap();
    let secrets = InMemoryCalendarSecretStore::new();
    let reference = secrets.put("owner-a", &[]).unwrap();
    store
        .upsert_google_calendar_connection("owner-a", "google", "account@example.com", "acct-123")
        .unwrap();
    store
        .set_google_calendar_secret_reference("owner-a", &reference)
        .unwrap();

    assert!(
        store
            .disconnect_google_calendar_with_secret_store("owner-a", &secrets)
            .unwrap()
    );
    assert!(
        store
            .google_calendar_connection_for_owner("owner-a")
            .unwrap()
            .is_none()
    );
    assert_eq!(secrets.get("owner-a", &reference).unwrap(), None);
}

#[test]
fn unavailable_secret_provider_fails_closed_without_storing_metadata() {
    let store = ConversationStore::in_memory().unwrap();
    let secrets = UnavailableCalendarSecretStore;

    assert!(secrets.put("owner-a", &[]).is_err());
    assert!(
        store
            .google_calendar_connection_for_owner("owner-a")
            .unwrap()
            .is_none()
    );
}

#[test]
fn secret_tool_adapter_fails_closed_when_its_binary_is_unavailable() {
    let secrets = SecretToolCalendarSecretStore::with_program(PathBuf::from("missing-secret-tool"));

    assert!(!secrets.is_available());
    assert!(secrets.put("owner-a", &[]).is_err());
}

#[test]
fn calendar_secret_contract_never_serializes_or_schemas_token_material() {
    let store = ConversationStore::in_memory().unwrap();
    let secrets = InMemoryCalendarSecretStore::new();
    let reference = secrets.put("owner-a", &[]).unwrap();
    store
        .upsert_google_calendar_connection("owner-a", "google", "account@example.com", "acct-123")
        .unwrap();
    store
        .set_google_calendar_secret_reference("owner-a", &reference)
        .unwrap();
    let connection = store
        .google_calendar_connection_for_owner("owner-a")
        .unwrap()
        .unwrap();

    let serialized = serde_json::to_string(&connection).unwrap();
    let schema: String = store
        .connection()
        .unwrap()
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='google_calendar_connections'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!serialized.contains(reference.as_str()));
    assert!(!schema.contains("access_token"));
    assert!(!schema.contains("refresh_token"));
    assert!(!schema.contains("authorization_code"));
}
