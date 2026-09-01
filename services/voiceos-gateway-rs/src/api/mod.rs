mod attachments;
mod auth;
mod client;
mod console;
mod conversations;
mod documents;
mod error;
mod events;
mod executions;
mod floor;
mod focus;
mod google_calendar;
mod health;
mod image_contract;
mod memories;
mod ontology;
mod outreach;
mod personal_support;
mod projects;
mod skills;
mod sleep_cycles;
mod tasks;
mod turns;
mod uploads;

use axum::routing::{delete, get, post, put};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header::HeaderName},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::state::AppState;

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health::health))
        .route(
            "/v1/integrations/google-calendar/status",
            get(google_calendar::status),
        )
        .route(
            "/v1/integrations/google-calendar/disconnect",
            post(google_calendar::disconnect),
        )
        .route("/v1/attachments", post(attachments::upload_attachment))
        .route(
            "/v1/attachments/{attachment_id}",
            get(attachments::attachment_content),
        )
        .route("/v1/client/bootstrap", get(client::bootstrap))
        .route("/v1/console/commands", post(console::execute))
        .route("/v1/events", get(events::stream))
        .route("/v1/events/recovery", get(events::recovery))
        .route("/v1/focus", get(focus::snapshot))
        .route("/v1/focus/captures", post(focus::capture))
        .route("/v1/personal/captures", post(personal_support::capture))
        .route("/v1/personal/inbox", get(personal_support::inbox))
        .route(
            "/v1/personal/captures/{capture_id}/decision",
            post(personal_support::capture_decision),
        )
        .route(
            "/v1/personal/captures/{capture_id}/extract",
            post(personal_support::extract),
        )
        .route("/v1/personal/proposals", get(personal_support::proposals))
        .route(
            "/v1/personal/proposals/{proposal_id}/approve",
            post(personal_support::approve),
        )
        .route(
            "/v1/personal/proposals/{proposal_id}/decision",
            post(personal_support::proposal_decision),
        )
        .route("/v1/personal/reviews", get(personal_support::reviews))
        .route(
            "/v1/personal/focus-reset",
            get(personal_support::focus_reset),
        )
        .route("/v1/personal/daily-reset", post(personal_support::reset))
        .route("/v1/focus/switch", post(focus::switch))
        .route("/v1/focus/sessions", post(focus::start))
        .route("/v1/focus/sessions/{session_id}/actions", post(focus::act))
        .route("/v1/turns/text", post(turns::turn))
        .route("/v1/conversation-areas", get(conversations::areas))
        .route(
            "/v1/conversation-areas/{area_id}/select",
            post(conversations::select_area),
        )
        .route(
            "/v1/conversations",
            get(conversations::list).post(conversations::create),
        )
        .route("/v1/conversations/history", get(conversations::history))
        .route("/v1/conversations/import", post(conversations::import))
        .route(
            "/v1/conversations/sync",
            get(conversations::sync_get).post(conversations::sync_apply),
        )
        .route(
            "/v1/conversations/{conversation_id}/select",
            post(conversations::select),
        )
        .route(
            "/v1/conversations/{conversation_id}/move",
            post(conversations::move_conversation),
        )
        .route(
            "/v1/conversations/{conversation_id}/export",
            get(conversations::export),
        )
        .route(
            "/v1/conversations/{conversation_id}/messages",
            get(conversations::conversation_messages),
        )
        .route("/v1/conversations/active", get(conversations::active))
        .route("/v1/memories", get(memories::list).post(memories::create))
        .route("/v1/memories/{memory_id}", delete(memories::forget))
        .route("/v1/memories/{memory_id}/correct", post(memories::correct))
        .route(
            "/v1/memory/sleep-cycles",
            get(sleep_cycles::list).post(sleep_cycles::start),
        )
        .route(
            "/v1/memory/sleep-cycles/{sleep_cycle_id}",
            get(sleep_cycles::detail),
        )
        .route(
            "/v1/memory/sleep-cycles/{sleep_cycle_id}/commit",
            post(sleep_cycles::commit),
        )
        .route(
            "/v1/conversations/active/messages",
            get(conversations::messages),
        )
        .route(
            "/v1/conversations/active/events",
            get(conversations::events),
        )
        .route(
            "/v1/conversations/active/floor",
            get(floor::get_floor).post(floor::change_floor),
        )
        .route("/v1/skills/proposals", get(skills::list_proposals))
        .route("/v1/skills", get(skills::list_skills))
        .route("/v1/skills/usages", get(skills::list_usages))
        .route("/v1/uploads", post(uploads::create))
        .route(
            "/v1/uploads/{upload_id}/chunks/{offset}",
            put(uploads::chunk),
        )
        .route("/v1/uploads/{upload_id}/finalize", post(uploads::finalize))
        .route(
            "/v1/skills/usages/{usage_id}/feedback",
            post(skills::review_usage),
        )
        .route("/v1/skills/{skill_id}/status", post(skills::set_status))
        .route("/v1/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route("/v1/executions/{job_id}", get(executions::live))
        .route(
            "/v1/executions/{job_id}/lease",
            post(executions::acquire_lease),
        )
        .route(
            "/v1/executions/{job_id}/checkpoints",
            post(executions::checkpoint),
        )
        .route("/v1/executions/{job_id}/cancel", post(executions::cancel))
        .route("/v1/executions/{job_id}/resume", post(executions::resume))
        .route("/v1/tasks/review/claim", post(tasks::claim_task_review))
        .route("/v1/projects", get(projects::list).post(projects::create))
        .route("/v1/tasks/{task_id}", get(tasks::task_detail))
        .route(
            "/v1/tasks/{task_id}/project",
            post(tasks::assign_task_project),
        )
        .route(
            "/v1/tasks/{task_id}/attention",
            post(tasks::set_task_attention),
        )
        .route(
            "/v1/tasks/{task_id}/status",
            post(tasks::update_task_status),
        )
        .route("/v1/tasks/{task_id}/actions", post(tasks::task_action))
        .route(
            "/v1/fieldy/intake",
            get(personal_support::list_fieldy_intake),
        )
        .route(
            "/v1/fieldy/intake/{capture_id}",
            get(personal_support::fieldy_intake_detail)
                .delete(personal_support::discard_fieldy_intake),
        )
        .route("/v1/outreach", get(outreach::list).post(outreach::create))
        .route("/v1/outreach/policy", get(outreach::policy))
        .route("/v1/outreach/{outreach_id}/actions", post(outreach::act))
        .route(
            "/v1/skills/proposals/{skill_id}/decision",
            post(skills::decide_proposal),
        )
        .route("/v1/ontology/catalog", get(ontology::catalog))
        .route("/v1/ontology/interpret", post(ontology::interpret))
        .route(
            "/v1/ontology/aliases",
            get(ontology::list_aliases).post(ontology::approve_alias),
        )
        .route(
            "/v1/ontology/interpretations/{interpretation_id}/correct",
            post(ontology::correct),
        )
        .route(
            "/v1/files",
            get(documents::list_files).post(documents::upload_file),
        )
        .route("/v1/files/{document_id}", delete(documents::delete_file))
        .route(
            "/internal/v1/documents/context",
            post(documents::document_context),
        )
        .route(
            "/internal/v1/ontology/interpret",
            post(ontology::interpret_deterministic),
        )
        .route(
            "/internal/v1/conversations/prepare",
            post(conversations::prepare),
        )
        .route(
            "/internal/v1/conversations/commit",
            post(conversations::commit),
        )
        .route("/internal/v1/tasks/command", post(tasks::voice_command))
        .route("/internal/v1/focus/command", post(focus::voice_command))
        .route(
            "/internal/v1/personal/command",
            post(personal_support::voice_command),
        )
        .route(
            "/internal/v1/personal/fieldy",
            post(personal_support::fieldy_webhook_intake),
        )
        .route(
            "/internal/v1/personal/fieldy/pending",
            get(personal_support::pending_fieldy),
        )
        .route(
            "/internal/v1/personal/fieldy/context/{capture_id}",
            get(personal_support::fieldy_context),
        )
        .route(
            "/internal/v1/personal/captures/{capture_id}/extract",
            post(personal_support::internal_extract),
        )
        .route(
            "/internal/v1/console/commands",
            post(console::internal_execute),
        )
        .route("/internal/v1/console/command", post(console::voice_command))
        .route(
            "/internal/v1/tasks/actions",
            post(tasks::internal_task_action),
        )
        .route(
            "/internal/v1/tasks/subagents",
            post(tasks::sync_subagent_task),
        )
        .route(
            "/internal/v1/tasks/{task_id}/initiative/claim",
            post(tasks::claim_initiative),
        )
        .route(
            "/internal/v1/tasks/{task_id}/initiative/result",
            post(tasks::complete_initiative),
        )
        .route("/internal/v1/skills/import", post(skills::import_proposal))
        .route("/internal/v1/skills/usages", post(skills::record_usage))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state,
            require_gateway_service,
        ))
}

async fn require_gateway_service(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !request.uri().path().starts_with("/internal/") {
        return next.run(request).await;
    }
    let Some(expected) = state.gateway_service_token.as_deref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"gateway_service_authentication_unconfigured"})),
        )
            .into_response();
    };
    let supplied = request
        .headers()
        .get(HeaderName::from_static("x-voiceos-gateway-service-token"))
        .and_then(|value| value.to_str().ok());
    if supplied.is_none_or(|value| !constant_time_eq(value, expected)) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"gateway_service_authentication_required"})),
        )
            .into_response();
    }
    next.run(request).await
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod integration_tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use rusqlite::Connection;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;
    use voiceos_core::{
        ConversationEngine, ConversationStore, MockProvider, Provider, ProviderCompletion,
        ProviderRequest, ProviderRouter, RoutingPolicy, Usage,
    };
    use voiceos_ontology::{Interpreter, OntologyStore};

    use super::router;
    use crate::state::AppState;

    const OWNER: &str = "owner-a";
    const TOKEN: &str = "test-device-token";

    struct VisionRecordingProvider {
        requests: Arc<Mutex<Vec<ProviderRequest>>>,
    }

    impl Provider for VisionRecordingProvider {
        fn name(&self) -> &str {
            "vision-recording"
        }

        fn supports_vision(&self) -> bool {
            true
        }

        fn complete(
            &self,
            request: &ProviderRequest,
        ) -> Result<ProviderCompletion, voiceos_core::ProviderError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(ProviderCompletion {
                text: "image received".to_owned(),
                provider: self.name().to_owned(),
                tool_calls: vec![],
                usage: Usage::default(),
            })
        }
    }

    fn authenticated_router() -> (Router, Arc<ConversationStore>, std::path::PathBuf) {
        authenticated_router_with_provider(Arc::new(MockProvider))
    }

    fn authenticated_router_with_provider(
        provider: Arc<dyn Provider>,
    ) -> (Router, Arc<ConversationStore>, std::path::PathBuf) {
        let store = Arc::new(ConversationStore::in_memory().unwrap());
        store.ensure_owner_device(OWNER, "device-a").unwrap();
        let legacy_audit_path = std::env::temp_dir().join(format!(
            "voiceos-gateway-test-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let connection = Connection::open(&legacy_audit_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE devices(device_id TEXT, token_hash TEXT, disabled_at TEXT)",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO devices(device_id, token_hash, disabled_at) VALUES (?1, ?2, NULL)",
                [
                    "device-a",
                    &format!("{:x}", Sha256::digest(TOKEN.as_bytes())),
                ],
            )
            .unwrap();
        drop(connection);
        let ontology = Arc::new(Interpreter::new(Arc::new(
            OntologyStore::in_memory().unwrap(),
        )));
        let mut provider_router = ProviderRouter::new(RoutingPolicy::default());
        provider_router.register(provider);
        let state = AppState {
            engine: Arc::new(ConversationEngine::new(store.clone())),
            router: Arc::new(provider_router),
            ontology,
            store: store.clone(),
            calendar_secret_store: Arc::new(voiceos_core::UnavailableCalendarSecretStore),
            google_calendar_oauth_configuration_error: Some(
                voiceos_core::GoogleCalendarOAuthConfigurationError::MissingClientId,
            ),
            legacy_audit_path: legacy_audit_path.clone(),
            require_device_auth: true,
            gateway_service_token: Some("test-gateway-service-token".to_owned()),
            primary_owner_id: OWNER.into(),
            pending_capture_devices: Arc::default(),
        };
        (router(state), store, legacy_audit_path)
    }

    async fn response(app: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn internal_routes_reject_missing_and_invalid_service_credentials_but_accept_valid_credentials()
     {
        let (app, _store, path) = authenticated_router();
        let request = Request::builder()
            .method("POST")
            .uri("/internal/v1/personal/command")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"device_id":"spoofed-device","text":"capture this private note"})
                    .to_string(),
            ))
            .unwrap();

        let (status, body) = response(app.clone(), request).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "gateway_service_authentication_required");

        let request = Request::builder()
            .method("POST")
            .uri("/internal/v1/personal/command")
            .header("content-type", "application/json")
            .header(
                "x-voiceos-gateway-service-token",
                "wrong-gateway-service-token",
            )
            .body(Body::from(
                json!({"device_id":"spoofed-device","text":"capture this private note"})
                    .to_string(),
            ))
            .unwrap();
        let (status, body) = response(app.clone(), request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "gateway_service_authentication_required");

        let request = Request::builder()
            .method("POST")
            .uri("/internal/v1/personal/command")
            .header("content-type", "application/json")
            .header(
                "x-voiceos-gateway-service-token",
                "test-gateway-service-token",
            )
            .body(Body::from(
                json!({"device_id":"gateway-device","text":"capture this private note"})
                    .to_string(),
            ))
            .unwrap();
        let (status, _body) = response(app, request).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn google_calendar_status_reports_a_machine_readable_missing_client_id() {
        let (app, _store, path) = authenticated_router();
        let request = Request::builder()
            .uri("/v1/integrations/google-calendar/status")
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = response(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["connected"], false);
        assert_eq!(body["authorization_ready"], false);
        assert_eq!(body["secret_storage_available"], false);
        assert_eq!(body["error"], "google_calendar_oauth_client_id_missing");
        assert_eq!(body["next_step"], "configure_google_calendar_oauth");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn google_calendar_disconnect_fails_closed_when_secret_store_is_unavailable() {
        let (app, store, path) = authenticated_router();
        let references = voiceos_core::InMemoryCalendarSecretStore::new();
        let reference = voiceos_core::CalendarSecretStore::put(&references, OWNER, &[]).unwrap();
        store
            .upsert_google_calendar_connection(OWNER, "google", "account@example.com", "acct-123")
            .unwrap();
        store
            .set_google_calendar_secret_reference(OWNER, &reference)
            .unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/v1/integrations/google-calendar/disconnect")
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = response(app, request).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "google_calendar_secret_store_unavailable");
        assert!(
            store
                .google_calendar_connection_for_owner(OWNER)
                .unwrap()
                .is_some()
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn image_turn_returns_typed_error_without_claiming_the_attachment() {
        let (app, store, path) = authenticated_router();
        let attachment = store
            .ingest_attachment_for_owner(
                OWNER,
                "device-a",
                "kitchen.jpg",
                "image/jpeg",
                b"\xff\xd8\xfftest-image",
            )
            .unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/v1/turns/text")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "text": "What is in this photo?",
                    "provider": "mock",
                    "request_id": "image-turn-1",
                    "attachment_ids": [attachment.id],
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = response(app, request).await;

        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["error"], "vision_not_supported");
        assert!(
            store
                .recent_conversation_messages(OWNER, 10)
                .unwrap()
                .is_empty()
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn upload_http_contract_rejects_malformed_metadata_without_creating_a_session() {
        let (app, store, path) = authenticated_router();
        let request = Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-voiceos-file-name", "photo.png")
            .header("content-type", "image/png")
            .header("x-voiceos-upload-length", "not-a-number")
            .header("x-voiceos-upload-sha256", "invalid")
            .body(Body::empty())
            .unwrap();

        let (status, body) = response(app, request).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_upload_length");
        let session_count: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM upload_sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(session_count, 0);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn upload_http_contract_rejects_malformed_sha256_metadata() {
        let (app, store, path) = authenticated_router();
        let request = Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-voiceos-file-name", "photo.png")
            .header("content-type", "image/png")
            .header("x-voiceos-upload-length", "8")
            .header("x-voiceos-upload-sha256", "ABC")
            .body(Body::empty())
            .unwrap();

        let (status, body) = response(app, request).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_upload_sha256");
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn upload_http_contract_rejects_unsupported_media_and_oversized_metadata_without_sessions()
     {
        let (app, store, path) = authenticated_router();
        let valid_sha256 = "a".repeat(64);
        let unsupported_media = Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-voiceos-file-name", "photo.gif")
            .header("content-type", "image/gif")
            .header("x-voiceos-upload-length", "8")
            .header("x-voiceos-upload-sha256", &valid_sha256)
            .body(Body::empty())
            .unwrap();
        let (status, body) = response(app.clone(), unsupported_media).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body["error"], "unsupported_media_type");

        let oversized = Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-voiceos-file-name", "photo.png")
            .header("content-type", "image/png")
            .header("x-voiceos-upload-length", "26214401")
            .header("x-voiceos-upload-sha256", &valid_sha256)
            .body(Body::empty())
            .unwrap();
        let (status, body) = response(app, oversized).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body["error"], "upload_too_large");
        let session_count: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM upload_sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(session_count, 0);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn upload_http_contract_retries_an_identical_chunk_and_finalizes_once() {
        let (app, store, path) = authenticated_router();
        let bytes = b"\x89PNG\r\n\x1a\n";
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let create = Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-voiceos-file-name", "photo.png")
            .header("content-type", "image/png")
            .header("x-voiceos-upload-length", bytes.len())
            .header("x-voiceos-upload-sha256", &sha256)
            .body(Body::empty())
            .unwrap();
        let (status, created) = response(app.clone(), create).await;
        assert_eq!(status, StatusCode::CREATED, "create response: {created}");
        let upload_id = created["upload"]["upload_id"].as_str().unwrap();

        let chunk = || {
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/uploads/{upload_id}/chunks/0"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/octet-stream")
                .body(Body::from(bytes.as_slice()))
                .unwrap()
        };
        let (status, first_chunk) = response(app.clone(), chunk()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first_chunk["next_offset"], json!(bytes.len()));
        let (status, retried_chunk) = response(app.clone(), chunk()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(retried_chunk["next_offset"], json!(bytes.len()));

        let finalize = || {
            Request::builder()
                .method("POST")
                .uri(format!("/v1/uploads/{upload_id}/finalize"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap()
        };
        let (status, finalized) = response(app.clone(), finalize()).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(finalized["attachment"]["sha256"], sha256);
        let (status, repeated_finalization) = response(app, finalize()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            repeated_finalization["attachment"]["attachment_id"],
            finalized["attachment"]["attachment_id"]
        );
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn uploaded_image_turn_forwards_typed_bytes_to_a_vision_provider() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(VisionRecordingProvider {
            requests: requests.clone(),
        });
        let (app, store, path) = authenticated_router_with_provider(provider);
        let bytes = b"\x89PNG\r\n\x1a\nvoiceos-image";
        let sha256 = format!("{:x}", Sha256::digest(bytes));

        let create = Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-voiceos-file-name", "kitchen.png")
            .header("content-type", "image/png")
            .header("x-voiceos-upload-length", bytes.len())
            .header("x-voiceos-upload-sha256", &sha256)
            .body(Body::empty())
            .unwrap();
        let (status, created) = response(app.clone(), create).await;
        assert_eq!(status, StatusCode::CREATED, "create response: {created}");
        let upload_id = created["upload"]["upload_id"].as_str().unwrap().to_owned();

        let chunk = Request::builder()
            .method("PUT")
            .uri(format!("/v1/uploads/{upload_id}/chunks/0"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::from(bytes.as_slice()))
            .unwrap();
        let (status, chunked) = response(app.clone(), chunk).await;
        assert_eq!(status, StatusCode::OK, "chunk response: {chunked}");

        let finalize = Request::builder()
            .method("POST")
            .uri(format!("/v1/uploads/{upload_id}/finalize"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let (status, finalized) = response(app.clone(), finalize).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "finalize response: {finalized}"
        );
        let attachment_id = finalized["attachment"]["attachment_id"]
            .as_str()
            .unwrap()
            .to_owned();

        let turn = Request::builder()
            .method("POST")
            .uri("/v1/turns/text")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "text": "What is in this image?",
                    "provider": "vision-recording",
                    "request_id": "vision-upload-turn-1",
                    "attachment_ids": [attachment_id],
                })
                .to_string(),
            ))
            .unwrap();
        let (status, turn) = response(app, turn).await;
        assert_eq!(status, StatusCode::OK, "turn response: {turn}");
        assert_eq!(turn["response_text"], "image received");

        let provider_requests = requests.lock().unwrap();
        assert_eq!(provider_requests.len(), 1);
        assert_eq!(
            provider_requests[0].image_attachments,
            vec![voiceos_core::ProviderImageAttachment {
                attachment_id,
                filename: "kitchen.png".to_owned(),
                media_type: "image/png".to_owned(),
                bytes: bytes.to_vec(),
            }]
        );
        drop(provider_requests);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn upload_http_contract_rejects_conflicting_retry_without_advancing_offset() {
        let (app, store, path) = authenticated_router();
        let bytes = b"\\x89PNG\\r\\n\\x1a\\n";
        let conflicting = b"\\x89PNG\\r\\n\\x1aX";
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let create = Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-voiceos-file-name", "photo.png")
            .header("content-type", "image/png")
            .header("x-voiceos-upload-length", bytes.len())
            .header("x-voiceos-upload-sha256", &sha256)
            .body(Body::empty())
            .unwrap();
        let (status, created) = response(app.clone(), create).await;
        assert_eq!(status, StatusCode::CREATED);
        let upload_id = created["upload"]["upload_id"].as_str().unwrap();

        let first = Request::builder()
            .method("PUT")
            .uri(format!("/v1/uploads/{upload_id}/chunks/0"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::from(bytes.as_slice()))
            .unwrap();
        let (status, body) = response(app.clone(), first).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["received_bytes"], json!(bytes.len()));

        let retry = Request::builder()
            .method("PUT")
            .uri(format!("/v1/uploads/{upload_id}/chunks/0"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::from(conflicting.as_slice()))
            .unwrap();
        let (status, body) = response(app.clone(), retry).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "offset_conflict");

        let received: i64 = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT received_bytes FROM upload_sessions WHERE upload_id=?",
                [upload_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(received, bytes.len() as i64);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn personal_capture_rejects_unknown_json_without_creating_a_record() {
        let (app, store, path) = authenticated_router();
        let request = Request::builder()
            .method("POST")
            .uri("/v1/personal/captures")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({
                "source": "voice", "source_id": "utterance-1", "text": "buy milk", "untyped": true
            }).to_string()))
            .unwrap();

        let (status, body) = response(app, request).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body.is_null());
        assert!(store.personal_inbox(OWNER).unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn signed_gateway_fieldy_intake_reaches_the_personal_inbox_idempotently() {
        let (app, store, path) = authenticated_router();
        let event = json!({
            "event_id": "fieldy-event-1",
            "occurred_at": chrono::Utc::now().to_rfc3339(),
            "transcript": "Remember to confirm the portrait display mount",
            "recording_id": "recording-1",
            "session_id": "session-1",
            "speakers": [],
            "metadata": {"source": "fieldy"}
        });
        let request = || {
            Request::builder()
                .method("POST")
                .uri("/internal/v1/personal/fieldy")
                .header(
                    "x-voiceos-gateway-service-token",
                    "test-gateway-service-token",
                )
                .header("content-type", "application/json")
                .body(Body::from(event.to_string()))
                .unwrap()
        };

        let (first_status, first) = response(app.clone(), request()).await;
        let (second_status, second) = response(app.clone(), request()).await;

        assert_eq!(first_status, StatusCode::CREATED);
        assert_eq!(second_status, StatusCode::CREATED);
        assert_eq!(first["capture"]["id"], second["capture"]["id"]);
        assert_eq!(first["capture"]["source"], "fieldy");
        assert_eq!(store.personal_inbox(OWNER).unwrap().len(), 1);

        let pending_request = || {
            Request::builder()
                .method("GET")
                .uri("/internal/v1/personal/fieldy/pending?quiet_seconds=0")
                .header(
                    "x-voiceos-gateway-service-token",
                    "test-gateway-service-token",
                )
                .body(Body::empty())
                .unwrap()
        };
        let (pending_status, pending) = response(app.clone(), pending_request()).await;
        assert_eq!(pending_status, StatusCode::OK);
        assert_eq!(pending["captures"].as_array().unwrap().len(), 1);

        let capture = &first["capture"];
        let context_request = Request::builder()
            .method("GET")
            .uri(format!(
                "/internal/v1/personal/fieldy/context/{}",
                capture["id"].as_str().unwrap()
            ))
            .header(
                "x-voiceos-gateway-service-token",
                "test-gateway-service-token",
            )
            .body(Body::empty())
            .unwrap();
        let (context_status, context) = response(app.clone(), context_request).await;
        assert_eq!(context_status, StatusCode::OK);
        assert_eq!(context["capture_id"], capture["id"]);
        assert!(context["projects"].is_array());
        assert!(context["tasks"].is_array());
        assert!(context["memories"].is_array());
        assert!(context["reviewing_proposals"].is_array());

        let extract_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/internal/v1/personal/captures/{}/extract",
                capture["id"].as_str().unwrap()
            ))
            .header(
                "x-voiceos-gateway-service-token",
                "test-gateway-service-token",
            )
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"output": {
                    "owner_id": OWNER,
                    "capture_id": capture["id"],
                    "candidates": [{
                        "project_id": null,
                        "category": "note",
                        "confidence": 0.88,
                        "title": "Portrait display mount",
                        "details": "The mount measurements need confirmation.",
                        "suggested_next_action": "Review the mount measurements.",
                        "rationale": "The conversation explicitly mentions the display mount.",
                        "evidence_capture_ids": [capture["id"]],
                        "expires_at": capture["expires_at"]
                    }]
                }})
                .to_string(),
            ))
            .unwrap();
        let (extract_status, extracted) = response(app.clone(), extract_request).await;
        assert_eq!(extract_status, StatusCode::OK);
        assert_eq!(extracted["proposals"].as_array().unwrap().len(), 1);

        let (_, pending_after) = response(app, pending_request()).await;
        assert!(pending_after["captures"].as_array().unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn personal_capture_requires_a_device_and_extraction_stays_review_only() {
        let (app, store, path) = authenticated_router();
        let unauthenticated = Request::builder()
            .method("GET")
            .uri("/v1/personal/inbox")
            .body(Body::empty())
            .unwrap();
        let (status, body) = response(app.clone(), unauthenticated).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "device_authentication_required");

        let capture_request = Request::builder()
            .method("POST")
            .uri("/v1/personal/captures")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "source": "voice", "source_id": "brain-dump-1", "text": "Need milk after work"
                })
                .to_string(),
            ))
            .unwrap();
        let (status, body) = response(app.clone(), capture_request).await;
        assert_eq!(status, StatusCode::CREATED);
        let capture = body["capture"].clone();
        assert_eq!(capture["owner_id"], OWNER);
        assert!(
            capture["audit_id"]
                .as_str()
                .unwrap()
                .starts_with("device:device-a:")
        );
        assert_eq!(
            store.downstream_record_counts(OWNER).unwrap(),
            (0, 0, 0, 0, 0)
        );

        let extraction = json!({
            "owner_id": OWNER,
            "capture_id": capture["id"],
            "candidates": [{
                "category": "task", "confidence": 0.9, "title": "Buy milk",
                "details": "Pick up milk after work.",
                "suggested_next_action": "Add milk to your shopping list for review.",
                "rationale": "The capture explicitly says you need milk.",
                "evidence_capture_ids": [capture["id"]], "expires_at": capture["expires_at"]
            }]
        });
        let extract_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/v1/personal/captures/{}/extract",
                capture["id"].as_str().unwrap()
            ))
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"output": extraction}).to_string()))
            .unwrap();
        let (status, body) = response(app, extract_request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["proposals"].as_array().unwrap().len(), 1);
        assert_eq!(
            store.downstream_record_counts(OWNER).unwrap(),
            (0, 0, 0, 0, 0)
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn authenticated_personal_focus_reset_returns_one_owner_scoped_next_action() {
        let (app, store, path) = authenticated_router();
        let project = store.create_project(OWNER, None, "Errands").unwrap();
        let task = store
            .create_task(
                OWNER,
                Some(&project.id),
                None,
                "Buy milk",
                "Milk is in the refrigerator",
                10,
            )
            .unwrap();
        store
            .create_task_step(OWNER, &task.id, "Open the shopping list", "user", "test")
            .unwrap();
        let request = Request::builder()
            .method("GET")
            .uri("/v1/personal/focus-reset")
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = response(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["focus_reset"]["recommendation"]["task_id"], task.id);
        assert_eq!(
            body["focus_reset"]["priorities"].as_array().unwrap().len(),
            1
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cross_owner_capture_ids_have_the_same_safe_error_as_missing_ids() {
        let (app, store, path) = authenticated_router();
        let other_capture = store
            .capture_personal_input(
                "owner-b",
                voiceos_core::CaptureSource::voice("other-owner-capture"),
                "private thought",
                chrono::Utc::now(),
                chrono::Duration::days(1),
            )
            .unwrap();
        let extract = |id: &str| {
            Request::builder()
                .method("POST")
                .uri(format!("/v1/personal/captures/{id}/extract"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"output": {"owner_id": OWNER, "capture_id": id, "candidates": []}})
                        .to_string(),
                ))
                .unwrap()
        };

        let (cross_owner_status, cross_owner_body) =
            response(app.clone(), extract(&other_capture.id)).await;
        let (missing_status, missing_body) = response(app, extract("not-a-real-capture")).await;

        assert_eq!(cross_owner_status, StatusCode::BAD_REQUEST);
        assert_eq!(cross_owner_status, missing_status);
        assert_eq!(cross_owner_body, missing_body);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn personal_voice_command_captures_only_directed_input_and_preserves_context() {
        let (app, store, path) = authenticated_router();
        let command = |text: &str| {
            Request::builder()
                .method("POST")
                .uri("/internal/v1/personal/command")
                .header(
                    "x-voiceos-gateway-service-token",
                    "test-gateway-service-token",
                )
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"device_id":"voice-device","text":text}).to_string(),
                ))
                .unwrap()
        };

        let (status, body) = response(app.clone(), command("Capture this buy milk")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["handled"], true);
        assert_eq!(body["capture"]["owner_id"], OWNER);
        assert!(
            body["capture"]["audit_id"]
                .as_str()
                .unwrap()
                .starts_with("device:voice-device:")
        );
        assert_eq!(store.personal_inbox(OWNER).unwrap().len(), 1);

        let (status, body) = response(app.clone(), command("I need to call the dentist")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({"handled": false}));
        assert_eq!(store.personal_inbox(OWNER).unwrap().len(), 1);

        let (status, body) = response(app.clone(), command("Capture this")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["capture_prompt"], true);
        let (status, body) = response(app, command("Call the dentist tomorrow")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["capture"]["raw_content"], "Call the dentist tomorrow");
        assert_eq!(body["capture"]["owner_id"], OWNER);
        assert!(
            body["capture"]["audit_id"]
                .as_str()
                .unwrap()
                .starts_with("device:voice-device:")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn ambiguous_voice_discard_never_deletes_a_capture() {
        let (app, store, path) = authenticated_router();
        let capture = store
            .capture_personal_input(
                OWNER,
                voiceos_core::CaptureSource::voice("voice-capture"),
                "keep this thought",
                chrono::Utc::now(),
                chrono::Duration::hours(24),
            )
            .unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/internal/v1/personal/command")
            .header(
                "x-voiceos-gateway-service-token",
                "test-gateway-service-token",
            )
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"device_id":"voice-device","text":"discard that"}).to_string(),
            ))
            .unwrap();

        let (status, body) = response(app, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["handled"], true);
        assert_eq!(body["decision"], serde_json::Value::Null);
        assert_eq!(
            store
                .personal_capture(OWNER, &capture.id)
                .unwrap()
                .unwrap()
                .status,
            "received"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn authenticated_five_minute_session_can_interrupt_resume_and_complete() {
        let (app, store, path) = authenticated_router();
        let project = store.create_project(OWNER, None, "Errands").unwrap();
        let task = store
            .create_task(
                OWNER,
                Some(&project.id),
                None,
                "Buy milk",
                "Milk is home",
                10,
            )
            .unwrap();
        store
            .create_task_step(OWNER, &task.id, "Open the shopping list", "user", "test")
            .unwrap();
        let start = Request::builder()
            .method("POST")
            .uri("/v1/focus/sessions")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"task_id": task.id, "mode": "five_minute"}).to_string(),
            ))
            .unwrap();
        let (status, body) = response(app.clone(), start).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["session"]["mode"], "five_minute");
        assert_eq!(body["session"]["planned_minutes"], 5);
        let session_id = body["session"]["id"].as_str().unwrap();
        for action in [
            json!({"action": "interrupt", "note": "phone", "restart_action": "Open the list"}),
            json!({"action": "resume", "planned_minutes": 5}),
            json!({"action": "complete", "reflection": "done for now"}),
        ] {
            let request = Request::builder()
                .method("POST")
                .uri(format!("/v1/focus/sessions/{session_id}/actions"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(action.to_string()))
                .unwrap();
            let (status, _) = response(app.clone(), request).await;
            assert_eq!(status, StatusCode::OK);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn internal_subagent_lifecycle_populates_and_finishes_one_task() {
        let (app, store, path) = authenticated_router();
        let start = Request::builder()
            .method("POST")
            .uri("/internal/v1/tasks/subagents")
            .header(
                "x-voiceos-gateway-service-token",
                "test-gateway-service-token",
            )
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "worker_id": "run_test_1",
                    "status": "running",
                    "session_id": "conversation-1",
                    "title": "VIC delegated: research launch options",
                    "observable_outcome": "A verified report returns to VIC.",
                    "estimated_minutes": 30,
                    "importance": "normal"
                })
                .to_string(),
            ))
            .unwrap();
        let (status, started) = response(app.clone(), start).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(started["created"], true);
        assert_eq!(started["task"]["status"], "active");
        assert_eq!(started["detail"]["progress"]["total_steps"], 3);
        let task_id = started["task"]["id"].as_str().unwrap().to_owned();

        let duplicate = Request::builder()
            .method("POST")
            .uri("/internal/v1/tasks/subagents")
            .header(
                "x-voiceos-gateway-service-token",
                "test-gateway-service-token",
            )
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"worker_id": "run_test_1", "status": "running"}).to_string(),
            ))
            .unwrap();
        let (status, duplicate) = response(app.clone(), duplicate).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(duplicate["created"], false);
        assert_eq!(duplicate["task"]["id"], task_id);

        let finish = Request::builder()
            .method("POST")
            .uri("/internal/v1/tasks/subagents")
            .header(
                "x-voiceos-gateway-service-token",
                "test-gateway-service-token",
            )
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "worker_id": "run_test_1",
                    "status": "completed",
                    "summary": "Verified launch report returned."
                })
                .to_string(),
            ))
            .unwrap();
        let (status, finished) = response(app, finish).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(finished["task"]["status"], "completed");
        assert_eq!(finished["detail"]["progress"]["completed_steps"], 3);
        assert_eq!(store.tasks(OWNER, true, 20).unwrap().len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn fieldy_operator_can_list_inspect_and_discard_intake() {
        let (app, store, path) = authenticated_router();
        let capture = store
            .capture_personal_input(
                OWNER,
                voiceos_core::CaptureSource::fieldy("fieldy-event-operator"),
                "Review the screen mount measurements",
                chrono::Utc::now(),
                chrono::Duration::hours(24),
            )
            .unwrap();
        let request = |method: &str, uri: String, body: Body| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(body)
                .unwrap()
        };
        let (status, body) = response(
            app.clone(),
            request("GET", "/v1/fieldy/intake".into(), Body::empty()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["intake"][0]["id"], capture.id);
        let (status, body) = response(
            app.clone(),
            request(
                "GET",
                format!("/v1/fieldy/intake/{}", capture.id),
                Body::empty(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["intake"]["raw_content"],
            "Review the screen mount measurements"
        );
        let (status, _) = response(
            app,
            request(
                "DELETE",
                format!("/v1/fieldy/intake/{}", capture.id),
                Body::from(json!({"audit_id":"operator-discard"}).to_string()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            store
                .personal_capture(OWNER, &capture.id)
                .unwrap()
                .unwrap()
                .status,
            "discarded"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn conversation_http_routes_preserve_area_isolation_and_selection() {
        let (app, store, path) = authenticated_router();
        let authenticated = |method: &str, uri: &str, body: Value| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap()
        };

        let (status, first) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversations",
                json!({"area_id":"brick-copper","title":"First","request_id":"http-isolation-1"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let first_id = first["conversation"]["id"].as_str().unwrap().to_owned();

        let (status, second) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversations",
                json!({"area_id":"personal","title":"Second","request_id":"http-isolation-2"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let second_id = second["conversation"]["id"].as_str().unwrap().to_owned();
        store
            .append_message(&first_id, voiceos_core::Role::User, "first message", None)
            .unwrap();
        store
            .append_message(&second_id, voiceos_core::Role::User, "second message", None)
            .unwrap();

        let (status, brick) = response(
            app.clone(),
            authenticated("GET", "/v1/conversations?area_id=brick-copper", Value::Null),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(brick["conversations"].as_array().unwrap().len(), 1);
        assert_eq!(brick["conversations"][0]["id"], first_id);

        let (status, selected) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversation-areas/personal/select",
                json!({"request_id":"http-isolation-select"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(selected["selected_area_id"], "personal");
        assert_eq!(selected["conversation"]["id"], second_id);

        let (status, personal_history) = response(
            app,
            authenticated(
                "GET",
                "/v1/conversations/history?area_id=personal",
                Value::Null,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            personal_history["days"][0]["conversations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            personal_history["days"][0]["conversations"][0]["id"],
            second_id
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn conversation_http_export_import_round_trip_preserves_metadata_and_messages() {
        let (app, store, path) = authenticated_router();
        let authenticated = |method: &str, uri: &str, body: Value| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap()
        };

        let (status, created) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversations",
                json!({"area_id":"brick-copper","title":"Export me","request_id":"http-export-create"}),
            ),
        ).await;
        assert_eq!(status, StatusCode::CREATED);
        let source_id = created["conversation"]["id"].as_str().unwrap().to_owned();
        store
            .append_message(&source_id, voiceos_core::Role::User, "keep this", None)
            .unwrap();
        store
            .append_message(
                &source_id,
                voiceos_core::Role::Assistant,
                "and this",
                Some("test-provider"),
            )
            .unwrap();

        let (status, exported) = response(
            app.clone(),
            authenticated(
                "GET",
                &format!("/v1/conversations/{source_id}/export"),
                Value::Null,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            exported["conversation"]["source_conversation_id"],
            source_id
        );
        assert_eq!(
            exported["conversation"]["messages"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let (status, imported) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversations/import",
                json!({"import_id":"http-export-import-1","conversation":exported["conversation"]}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let imported_id = imported["conversation"]["id"].as_str().unwrap().to_owned();
        assert_ne!(imported_id, source_id);
        assert_eq!(imported["conversation"]["area_id"], "brick-copper");
        assert_eq!(imported["conversation"]["title"], "Export me");

        let (status, messages) = response(
            app.clone(),
            authenticated(
                "GET",
                &format!("/v1/conversations/{imported_id}/messages"),
                Value::Null,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(messages["messages"][0]["content"], "keep this");
        assert_eq!(messages["messages"][1]["content"], "and this");

        let (_, duplicate) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversations/import",
                json!({"import_id":"http-export-import-1","conversation":exported["conversation"]}),
            ),
        )
        .await;
        assert_eq!(duplicate["conversation"]["id"], imported_id);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn conversation_http_sync_applies_newer_area_and_rejects_invalid_records() {
        let (app, _store, path) = authenticated_router();
        let authenticated = |method: &str, uri: &str, body: Value| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap()
        };
        let (_, created) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversations",
                json!({"area_id":"brick-copper","title":"Sync me","request_id":"http-sync-create"}),
            ),
        )
        .await;
        let conversation_id = created["conversation"]["id"].as_str().unwrap().to_owned();
        let newer = json!({"conversation_id":conversation_id,"area_id":"personal","area_updated_at":"2099-01-01T00:00:00Z","area_updated_by_device":"phone"});
        let (status, applied) = response(
            app.clone(),
            authenticated(
                "POST",
                "/v1/conversations/sync",
                json!({"conversations":[newer]}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(applied["applied"], 1);
        let (status, listed) = response(
            app.clone(),
            authenticated("GET", "/v1/conversations?area_id=personal", Value::Null),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["conversations"][0]["id"], conversation_id);

        let (status, stale) = response(app.clone(), authenticated("POST", "/v1/conversations/sync", json!({"conversations":[{"conversation_id":conversation_id,"area_id":"general-talk","area_updated_at":"2020-01-01T00:00:00Z","area_updated_by_device":"old-device"}]}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(stale["applied"], 0);
        let (status, invalid) = response(app.clone(), authenticated("POST", "/v1/conversations/sync", json!({"conversations":[{"conversation_id":conversation_id,"area_id":"not-an-area","area_updated_at":"2099-01-01T00:00:00Z","area_updated_by_device":"phone"}]}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(invalid["error"], "invalid_conversation_request");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn conversation_area_flow_is_authenticated_idempotent_and_visible_in_bootstrap() {
        let (app, store, path) = authenticated_router();
        let unauthorized = Request::builder()
            .uri("/v1/conversation-areas")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            response(app.clone(), unauthorized).await.0,
            StatusCode::UNAUTHORIZED
        );

        let authenticated = |method: &str, uri: &str, body: Value| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap()
        };
        let (status, bootstrap) = response(
            app.clone(),
            authenticated("GET", "/v1/client/bootstrap", Value::Null),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bootstrap["contract_version"], 2);
        assert_eq!(bootstrap["conversation_areas"].as_array().unwrap().len(), 6);

        let create_body = json!({
            "area_id":"brick-copper",
            "title":"Opening plan",
            "request_id":"gateway-create-1"
        });
        let (status, created) = response(
            app.clone(),
            authenticated("POST", "/v1/conversations", create_body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let conversation_id = created["conversation"]["id"].as_str().unwrap().to_owned();
        let (_, duplicate) = response(
            app.clone(),
            authenticated("POST", "/v1/conversations", create_body),
        )
        .await;
        assert_eq!(duplicate["conversation"]["id"], conversation_id);

        let (status, rejected) = response(
            app.clone(),
            authenticated(
                "POST",
                &format!("/v1/conversations/{conversation_id}/move"),
                json!({
                    "source_area_id":"brick-copper",
                    "destination_area_id":"personal",
                    "confirmed":false,
                    "request_id":"gateway-move-1"
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(rejected["error"], "invalid_conversation_request");

        let (status, moved) = response(
            app.clone(),
            authenticated(
                "POST",
                &format!("/v1/conversations/{conversation_id}/move"),
                json!({
                    "source_area_id":"brick-copper",
                    "destination_area_id":"personal",
                    "confirmed":true,
                    "request_id":"gateway-move-1"
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["conversation"]["area_id"], "personal");

        store
            .append_message(&conversation_id, voiceos_core::Role::User, "hello", None)
            .unwrap();
        let (status, history) = response(
            app.clone(),
            authenticated(
                "GET",
                "/v1/conversations/history?timezone_offset_minutes=0",
                Value::Null,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            history["days"][0]["conversations"][0]["area_id"],
            "personal"
        );

        let (status, sync) = response(
            app,
            authenticated("GET", "/v1/conversations/sync?after=0", Value::Null),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(sync["selected_area_id"], "personal");
        assert_eq!(sync["messages"][0]["area_id"], "personal");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn execution_routes_require_auth_and_expose_only_owned_persisted_state() {
        let (app, store, path) = authenticated_router();
        let job = store
            .create_job(OWNER, None, "gateway-execution", json!(["filesystem.read"]))
            .unwrap();
        store
            .transition_job_status(OWNER, &job.id, "proposed", "approved")
            .unwrap()
            .unwrap();

        let unauthorized = Request::builder()
            .method("GET")
            .uri(format!("/v1/executions/{}", job.id))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            response(app.clone(), unauthorized).await.0,
            StatusCode::UNAUTHORIZED
        );

        let request = |method: &str, suffix: &str, body: Value| {
            Request::builder()
                .method(method)
                .uri(format!("/v1/executions/{}{suffix}", job.id))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap()
        };
        let (status, lease) = response(
            app.clone(),
            request(
                "POST",
                "/lease",
                json!({"capabilities":["filesystem.read"],"ttl_seconds":60}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(lease["lease_id"].as_str().is_some());

        let (status, checkpoint) = response(
            app.clone(),
            request(
                "POST",
                "/checkpoints",
                json!({"state":{"cursor":1},"rollback":{"undo":"step"}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(checkpoint["checkpoint"]["sequence"], 1);

        let (status, live) = response(app.clone(), request("GET", "", Value::Null)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(live["job"]["id"], job.id);
        assert_eq!(
            live["latest_checkpoint"]["rollback"],
            json!({"undo":"step"})
        );
        assert!(live.get("provider_telemetry").is_none());
        let other_owner_job = store
            .create_job("owner-b", None, "other-owner-execution", json!([]))
            .unwrap();
        let (status, missing) = response(
            app.clone(),
            Request::builder()
                .uri(format!("/v1/executions/{}", other_owner_job.id))
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(missing["error"], "execution_not_found");

        let (status, cancelled) = response(
            app.clone(),
            request("POST", "/cancel", json!({"reason":"user_requested"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cancelled["cancelled"], true);
        let (status, retry) = response(
            app.clone(),
            request("POST", "/cancel", json!({"reason":"retry"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(retry["cancelled"], true);
        assert_eq!(
            response(app, request("POST", "/resume", json!({}))).await.0,
            StatusCode::BAD_REQUEST
        );
        std::fs::remove_file(path).unwrap();
    }
}
