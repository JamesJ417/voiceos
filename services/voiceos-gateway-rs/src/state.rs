use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use voiceos_core::{
    CalendarSecretStore, ConversationEngine, ConversationStore,
    GoogleCalendarOAuthConfigurationError, ProviderRouter,
};
use voiceos_ontology::Interpreter;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Arc<ConversationStore>,
    pub(crate) calendar_secret_store: Arc<dyn CalendarSecretStore>,
    pub(crate) google_calendar_oauth_configuration_error:
        Option<GoogleCalendarOAuthConfigurationError>,
    pub(crate) engine: Arc<ConversationEngine>,
    pub(crate) router: Arc<ProviderRouter>,
    pub(crate) ontology: Arc<Interpreter>,
    pub(crate) legacy_audit_path: PathBuf,
    pub(crate) require_device_auth: bool,
    pub(crate) gateway_service_token: Option<String>,
    pub(crate) primary_owner_id: String,
    pub(crate) pending_capture_devices: Arc<Mutex<HashMap<String, Instant>>>,
}
