use std::path::PathBuf;
use std::sync::Arc;

use voiceos_core::{
    ConversationEngine, ConversationStore, DoctrineAuthority, ProviderRouter, SleepMemoryAuthority,
};
use voiceos_ontology::Interpreter;

use crate::artifact_worker::{ArtifactStorage, PdfWorker};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Arc<ConversationStore>,
    pub(crate) engine: Arc<ConversationEngine>,
    pub(crate) router: Arc<ProviderRouter>,
    pub(crate) sleep_memory: Arc<SleepMemoryAuthority>,
    pub(crate) sleep_memory_enabled: bool,
    pub(crate) sleep_model_mode: String,
    pub(crate) doctrine: Arc<DoctrineAuthority>,
    pub(crate) doctrine_flags: DoctrineFlags,
    pub(crate) internal_token: Option<String>,
    pub(crate) ontology: Arc<Interpreter>,
    pub(crate) legacy_audit_path: PathBuf,
    pub(crate) require_device_auth: bool,
    pub(crate) primary_owner_id: String,
    pub(crate) artifact_storage: ArtifactStorage,
    pub(crate) pdf_worker: PdfWorker,
}

#[derive(Clone, Default)]
pub(crate) struct DoctrineFlags {
    pub(crate) enabled: bool,
    pub(crate) extraction: bool,
    pub(crate) sleep_integration: bool,
    pub(crate) runtime: bool,
    pub(crate) source_audit: bool,
}
