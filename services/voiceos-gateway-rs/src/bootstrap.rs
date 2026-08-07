use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use voiceos_core::{
    CodexBridgeProvider, ConversationEngine, ConversationStore, DoctrineAuthority, MockProvider,
    OllamaProvider, ProviderRouter, RoutingPolicy, SleepMemoryAuthority,
};
use voiceos_ontology::{Interpreter, OntologyStore};

use crate::artifact_worker::{ArtifactStorage, PdfWorker};
use crate::ontology_fallback::GatewayModelFallback;
use crate::state::{AppState, DoctrineFlags};

pub(crate) fn build_state() -> Result<AppState, Box<dyn std::error::Error>> {
    if env::var("VOICEOS_SLEEP_MEMORY_ENABLED").as_deref() == Ok("1")
        && env::var("VOICEOS_SLEEP_MODEL_MODE")
            .as_deref()
            .unwrap_or("routed")
            == "routed"
    {
        let url =
            env::var("VOICEOS_OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_owned());
        if !is_loopback_http_url(&url) {
            return Err("sleep-memory routed models must use a loopback Ollama URL".into());
        }
    }
    let data_dir =
        env::var("VOICEOS_RUST_DATA_DIR").unwrap_or_else(|_| "work/voiceos-core".to_owned());
    let store = Arc::new(ConversationStore::open(
        Path::new(&data_dir).join("memory.sqlite3"),
    )?);
    let primary_owner_id =
        env::var("VOICEOS_PRIMARY_OWNER_ID").unwrap_or_else(|_| "voiceos-primary-owner".to_owned());
    store.migrate_devices_to_owner(&primary_owner_id)?;
    store.ensure_default_attention_automations(&primary_owner_id)?;
    let legacy_audit_path: std::path::PathBuf = env::var("VOICEOS_LEGACY_AUDIT_PATH")
        .unwrap_or_else(|_| "work/gateway-data/audit.sqlite3".to_owned())
        .into();

    if let Ok(legacy_path) = env::var("VOICEOS_IMPORT_AUDIT") {
        let imported = store.import_legacy_audit(&legacy_path, "legacy-audit")?;
        eprintln!("Imported {imported} legacy audit turns");
    }
    if env::var("VOICEOS_PROPOSE_SKILLS_FROM_AUDIT").as_deref() != Ok("0")
        && legacy_audit_path.is_file()
    {
        match store.propose_skills_from_legacy_audit(&legacy_audit_path, &primary_owner_id, 2) {
            Ok(proposals) if !proposals.is_empty() => {
                eprintln!(
                    "Proposed {} reviewed skills from audit evidence",
                    proposals.len()
                );
            }
            Ok(_) => {}
            Err(error) => eprintln!("Skill proposal replay skipped: {error}"),
        }
    }

    let engine = Arc::new(ConversationEngine::new(store.clone()));
    let router = Arc::new(build_provider_router()?);
    let sleep_memory = Arc::new(SleepMemoryAuthority::new(store.clone()));
    let doctrine = Arc::new(DoctrineAuthority::new(store.clone()));
    doctrine.seed_registry(&primary_owner_id)?;
    let ontology_store = Arc::new(OntologyStore::open(
        Path::new(&data_dir).join("ontology.sqlite3"),
    )?);
    let ontology = Interpreter::new(ontology_store);
    let ontology = if env::var("VOICEOS_ONTOLOGY_MODEL_FALLBACK").as_deref() == Ok("1") {
        ontology.with_fallback(Arc::new(GatewayModelFallback::new(
            router.clone(),
            env::var("VOICEOS_ONTOLOGY_MODEL").unwrap_or_else(|_| "gemma".to_owned()),
        )))
    } else {
        ontology
    };
    let artifact_dir = env::var("VOICEOS_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(&data_dir).join("artifacts"));
    let artifact_storage = ArtifactStorage::new(artifact_dir)?;
    let pdf_worker = PdfWorker::start(store.clone(), artifact_storage.clone());
    Ok(AppState {
        store,
        engine,
        router,
        sleep_memory,
        sleep_memory_enabled: env::var("VOICEOS_SLEEP_MEMORY_ENABLED").as_deref() == Ok("1"),
        sleep_model_mode: env::var("VOICEOS_SLEEP_MODEL_MODE")
            .unwrap_or_else(|_| "routed".to_owned()),
        doctrine,
        doctrine_flags: DoctrineFlags {
            enabled: env::var("VOICEOS_VIC_DOCTRINE_ENABLED").as_deref() == Ok("1"),
            extraction: env::var("VOICEOS_VIC_DOCTRINE_EXTRACTION_ENABLED").as_deref() == Ok("1"),
            sleep_integration: env::var("VOICEOS_VIC_DOCTRINE_SLEEP_INTEGRATION_ENABLED")
                .as_deref()
                == Ok("1"),
            runtime: env::var("VOICEOS_VIC_DOCTRINE_RUNTIME_ENABLED").as_deref() == Ok("1"),
            source_audit: env::var("VOICEOS_VIC_DOCTRINE_SOURCE_AUDIT_ENABLED").as_deref()
                == Ok("1"),
        },
        internal_token: env::var("VOICEOS_INTERNAL_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        ontology: Arc::new(ontology),
        legacy_audit_path,
        require_device_auth: env::var("VOICEOS_REQUIRE_DEVICE_AUTH").as_deref() == Ok("1"),
        primary_owner_id,
        artifact_storage,
        pdf_worker,
    })
}

fn is_loopback_http_url(value: &str) -> bool {
    ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

fn build_provider_router() -> Result<ProviderRouter, Box<dyn std::error::Error>> {
    let mut router = ProviderRouter::new(RoutingPolicy::default());
    router.register(Arc::new(MockProvider));

    let ollama_url =
        env::var("VOICEOS_OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_owned());
    if let Ok(model) = env::var("VOICEOS_GEMMA_MODEL").or_else(|_| env::var("VOICEOS_OLLAMA_MODEL"))
    {
        router.register(Arc::new(OllamaProvider::new(
            "gemma",
            &ollama_url,
            model,
            false,
        )?));
    }
    if let Ok(model) =
        env::var("VOICEOS_GPT_OSS_MODEL").or_else(|_| env::var("VOICEOS_OLLAMA_DEEP_MODEL"))
    {
        router.register(Arc::new(OllamaProvider::new(
            "gpt-oss",
            &ollama_url,
            model,
            true,
        )?));
    }
    if env::var("VOICEOS_CODEX_ENABLED").as_deref() == Ok("1") {
        router.register(Arc::new(CodexBridgeProvider::new(
            env::var("VOICEOS_CODEX_SOCKET")
                .unwrap_or_else(|_| "/run/voiceos-codex/codex.sock".to_owned()),
        )));
    }
    Ok(router)
}
