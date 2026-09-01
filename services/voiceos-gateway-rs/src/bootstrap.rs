use std::env;
use std::path::Path;
use std::sync::Arc;

use voiceos_core::{
    CodexBridgeProvider, ConversationEngine, ConversationStore, GoogleCalendarOAuthConfiguration,
    MockProvider, OllamaProvider, ProviderRouter, RoutingPolicy, SecretToolCalendarSecretStore,
};
use voiceos_ontology::{Interpreter, OntologyStore};

use crate::ontology_fallback::GatewayModelFallback;
use crate::state::AppState;

pub(crate) fn build_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir =
        env::var("VOICEOS_RUST_DATA_DIR").unwrap_or_else(|_| "work/voiceos-core".to_owned());
    let store = Arc::new(ConversationStore::open(
        Path::new(&data_dir).join("memory.sqlite3"),
    )?);
    store.cleanup_expired_attachments()?;
    let primary_owner_id =
        env::var("VOICEOS_PRIMARY_OWNER_ID").unwrap_or_else(|_| "voiceos-primary-owner".to_owned());
    store.migrate_devices_to_owner(&primary_owner_id)?;
    let legacy_audit_path: std::path::PathBuf = env::var("VOICEOS_LEGACY_AUDIT_PATH")
        .unwrap_or_else(|_| "work/gateway-data/audit.sqlite3".to_owned())
        .into();

    if let Ok(legacy_path) = env::var("VOICEOS_IMPORT_AUDIT") {
        let imported =
            store.import_legacy_audit(&legacy_path, &primary_owner_id, "legacy-audit")?;
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
    let google_calendar_oauth_configuration_error = GoogleCalendarOAuthConfiguration::new(
        env::var("VOICEOS_GOOGLE_CALENDAR_CLIENT_ID").ok(),
        env::var("VOICEOS_GOOGLE_CALENDAR_REDIRECT_URI").ok(),
    )
    .validate()
    .err();
    Ok(AppState {
        store,
        calendar_secret_store: Arc::new(SecretToolCalendarSecretStore::new()),
        google_calendar_oauth_configuration_error,
        engine,
        router,
        ontology: Arc::new(ontology),
        legacy_audit_path,
        // Authentication is fail-closed unless local development explicitly opts out.
        require_device_auth: env::var("VOICEOS_REQUIRE_DEVICE_AUTH").as_deref() != Ok("0"),
        gateway_service_token: env::var("VOICEOS_RUST_SERVICE_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty()),
        primary_owner_id,
        pending_capture_devices: Arc::default(),
    })
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
