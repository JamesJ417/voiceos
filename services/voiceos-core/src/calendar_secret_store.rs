use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use thiserror::Error;
use uuid::Uuid;

const GOOGLE_CALENDAR_INTEGRATION_KEY: &str = "google-calendar";

/// An opaque identifier for a calendar credential held outside SQLite.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CalendarSecretReference(String);

impl CalendarSecretReference {
    fn new() -> Self {
        Self(format!("gcal_secret_{}", Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CalendarSecretReference {
    type Error = CalendarSecretStoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.starts_with("gcal_secret_") && value.len() > "gcal_secret_".len() {
            Ok(Self(value))
        } else {
            Err(CalendarSecretStoreError::InvalidReference)
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CalendarSecretStoreError {
    #[error("calendar secret storage is unavailable")]
    Unavailable,
    #[error("calendar secret reference is invalid")]
    InvalidReference,
    #[error("calendar secret storage operation failed")]
    OperationFailed,
}

/// Generic, value-free failures returned by an OS Secret Service backend.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum SecretServiceBackendError {
    #[error("secret service is unavailable")]
    Unavailable,
    #[error("secret service operation failed")]
    OperationFailed,
}

/// Injection boundary for an OS-managed encrypted secret service.
///
/// Implementations must not serialize credentials or return backend diagnostics containing secret
/// material. On Linux, [`SecretToolCalendarSecretStore`] uses `secret-tool`, which talks to the
/// session Secret Service/keyring rather than the VoiceOS SQLite database.
pub trait SecretServiceBackend: Send + Sync {
    fn is_available(&self) -> bool;
    fn store(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
        secret: &[u8],
    ) -> Result<(), SecretServiceBackendError>;
    fn lookup(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, SecretServiceBackendError>;
    fn delete(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
    ) -> Result<bool, SecretServiceBackendError>;
}

/// Boundary for encrypted, owner-scoped calendar credential storage.
///
/// Production must provide an OS-backed implementation. SQLite stores only the returned opaque
/// reference and never receives the credential bytes.
pub trait CalendarSecretStore: Send + Sync {
    /// Whether the configured secret-store capability has been locally verified as available.
    fn is_available(&self) -> bool;
    fn put(
        &self,
        owner_id: &str,
        secret: &[u8],
    ) -> Result<CalendarSecretReference, CalendarSecretStoreError>;
    fn get(
        &self,
        owner_id: &str,
        reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, CalendarSecretStoreError>;
    fn delete(
        &self,
        owner_id: &str,
        reference: &CalendarSecretReference,
    ) -> Result<bool, CalendarSecretStoreError>;
}

/// Explicit production default until an approved OS-backed encrypted adapter is available.
pub struct UnavailableCalendarSecretStore;

impl CalendarSecretStore for UnavailableCalendarSecretStore {
    fn is_available(&self) -> bool {
        false
    }

    fn put(
        &self,
        _owner_id: &str,
        _secret: &[u8],
    ) -> Result<CalendarSecretReference, CalendarSecretStoreError> {
        Err(CalendarSecretStoreError::Unavailable)
    }

    fn get(
        &self,
        _owner_id: &str,
        _reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, CalendarSecretStoreError> {
        Err(CalendarSecretStoreError::Unavailable)
    }

    fn delete(
        &self,
        _owner_id: &str,
        _reference: &CalendarSecretReference,
    ) -> Result<bool, CalendarSecretStoreError> {
        Err(CalendarSecretStoreError::Unavailable)
    }
}

/// Local Linux adapter for the OS Secret Service via the standard `secret-tool` utility.
///
/// The underlying keyring owns encryption and persistence. This adapter passes credentials only
/// over the child process's stdin/stdout and suppresses child output; it never writes credential
/// bytes to SQLite, logs, or error values. Every operation fails closed when the backend is not
/// locally available.
pub struct SecretToolCalendarSecretStore {
    backend: Box<dyn SecretServiceBackend>,
}

impl SecretToolCalendarSecretStore {
    pub fn new() -> Self {
        Self::with_program("secret-tool".into())
    }

    pub fn with_program(program: PathBuf) -> Self {
        Self::with_backend(SecretToolBackend { program })
    }

    pub fn with_backend(backend: impl SecretServiceBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    fn ensure_available(&self) -> Result<(), CalendarSecretStoreError> {
        self.backend
            .is_available()
            .then_some(())
            .ok_or(CalendarSecretStoreError::Unavailable)
    }
}

impl Default for SecretToolCalendarSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CalendarSecretStore for SecretToolCalendarSecretStore {
    fn is_available(&self) -> bool {
        self.backend.is_available()
    }

    fn put(
        &self,
        owner_id: &str,
        secret: &[u8],
    ) -> Result<CalendarSecretReference, CalendarSecretStoreError> {
        self.ensure_available()?;
        let reference = CalendarSecretReference::new();
        self.backend
            .store(
                owner_id,
                GOOGLE_CALENDAR_INTEGRATION_KEY,
                &reference,
                secret,
            )
            .map_err(map_backend_error)?;
        Ok(reference)
    }

    fn get(
        &self,
        owner_id: &str,
        reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, CalendarSecretStoreError> {
        self.ensure_available()?;
        self.backend
            .lookup(owner_id, GOOGLE_CALENDAR_INTEGRATION_KEY, reference)
            .map_err(map_backend_error)
    }

    fn delete(
        &self,
        owner_id: &str,
        reference: &CalendarSecretReference,
    ) -> Result<bool, CalendarSecretStoreError> {
        self.ensure_available()?;
        self.backend
            .delete(owner_id, GOOGLE_CALENDAR_INTEGRATION_KEY, reference)
            .map_err(map_backend_error)
    }
}

fn map_backend_error(error: SecretServiceBackendError) -> CalendarSecretStoreError {
    match error {
        SecretServiceBackendError::Unavailable => CalendarSecretStoreError::Unavailable,
        SecretServiceBackendError::OperationFailed => CalendarSecretStoreError::OperationFailed,
    }
}

struct SecretToolBackend {
    program: PathBuf,
}

impl SecretToolBackend {
    fn command(
        &self,
        operation: &str,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
    ) -> Command {
        let mut command = Command::new(&self.program);
        command.args([
            operation,
            "voiceos.application",
            "voiceos",
            "voiceos.integration",
            integration_key,
            "voiceos.owner",
            owner_id,
            "voiceos.reference",
            reference.as_str(),
        ]);
        command
    }

    fn result(
        output: std::io::Result<std::process::Output>,
    ) -> Result<std::process::Output, SecretServiceBackendError> {
        let output = output.map_err(|_| SecretServiceBackendError::Unavailable)?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(SecretServiceBackendError::OperationFailed)
        }
    }
}

impl SecretServiceBackend for SecretToolBackend {
    fn is_available(&self) -> bool {
        // A read-only query for a deliberately nonexistent attribute verifies that the session
        // Secret Service can be reached without creating, reading, or disclosing a credential.
        Command::new(&self.program)
            .args([
                "search",
                "--all",
                "voiceos.capability_probe",
                "unconfigured",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn store(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
        secret: &[u8],
    ) -> Result<(), SecretServiceBackendError> {
        let mut command = self.command("store", owner_id, integration_key, reference);
        command
            .arg("--label=VoiceOS Calendar Credential")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|_| SecretServiceBackendError::Unavailable)?;
        child
            .stdin
            .take()
            .ok_or(SecretServiceBackendError::OperationFailed)?
            .write_all(secret)
            .map_err(|_| SecretServiceBackendError::OperationFailed)?;
        Self::result(child.wait_with_output())?;
        Ok(())
    }

    fn lookup(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, SecretServiceBackendError> {
        let mut command = self.command("lookup", owner_id, integration_key, reference);
        command.stdin(Stdio::null()).stderr(Stdio::null());
        Ok(Some(Self::result(command.output())?.stdout))
    }

    fn delete(
        &self,
        owner_id: &str,
        integration_key: &str,
        reference: &CalendarSecretReference,
    ) -> Result<bool, SecretServiceBackendError> {
        let mut command = self.command("clear", owner_id, integration_key, reference);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Self::result(command.output())?;
        Ok(true)
    }
}

/// Process-local adapter for tests only. It is intentionally not a production backend.
#[derive(Default)]
pub struct InMemoryCalendarSecretStore {
    entries: Mutex<HashMap<(String, CalendarSecretReference), Vec<u8>>>,
}

impl InMemoryCalendarSecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CalendarSecretStore for InMemoryCalendarSecretStore {
    fn is_available(&self) -> bool {
        true
    }

    fn put(
        &self,
        owner_id: &str,
        secret: &[u8],
    ) -> Result<CalendarSecretReference, CalendarSecretStoreError> {
        let reference = CalendarSecretReference::new();
        self.entries
            .lock()
            .map_err(|_| CalendarSecretStoreError::OperationFailed)?
            .insert((owner_id.to_owned(), reference.clone()), secret.to_vec());
        Ok(reference)
    }

    fn get(
        &self,
        owner_id: &str,
        reference: &CalendarSecretReference,
    ) -> Result<Option<Vec<u8>>, CalendarSecretStoreError> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| CalendarSecretStoreError::OperationFailed)?
            .get(&(owner_id.to_owned(), reference.clone()))
            .cloned())
    }

    fn delete(
        &self,
        owner_id: &str,
        reference: &CalendarSecretReference,
    ) -> Result<bool, CalendarSecretStoreError> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| CalendarSecretStoreError::OperationFailed)?
            .remove(&(owner_id.to_owned(), reference.clone()))
            .is_some())
    }
}
