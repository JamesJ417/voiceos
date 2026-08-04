use std::path::PathBuf;
use std::sync::Arc;

use voiceos_core::{ConversationEngine, ConversationStore, ProviderRouter};
use voiceos_ontology::Interpreter;

use crate::artifact_worker::{ArtifactStorage, PdfWorker};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Arc<ConversationStore>,
    pub(crate) engine: Arc<ConversationEngine>,
    pub(crate) router: Arc<ProviderRouter>,
    pub(crate) ontology: Arc<Interpreter>,
    pub(crate) legacy_audit_path: PathBuf,
    pub(crate) require_device_auth: bool,
    pub(crate) primary_owner_id: String,
    pub(crate) artifact_storage: ArtifactStorage,
    pub(crate) pdf_worker: PdfWorker,
}
