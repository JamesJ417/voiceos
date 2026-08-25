mod attachments;
mod auth;
mod client;
mod console;
mod conversations;
mod documents;
mod error;
mod events;
mod floor;
mod focus;
mod health;
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

use axum::Router;
use axum::routing::{delete, get, post, put};

use crate::state::AppState;

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health::health))
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
            post(personal_support::fieldy_intake),
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
            "/internal/v1/tasks/{task_id}/initiative/claim",
            post(tasks::claim_initiative),
        )
        .route(
            "/internal/v1/tasks/{task_id}/initiative/result",
            post(tasks::complete_initiative),
        )
        .route("/internal/v1/skills/import", post(skills::import_proposal))
        .route("/internal/v1/skills/usages", post(skills::record_usage))
        .with_state(state)
}

#[cfg(test)]
mod integration_tests {
    use std::sync::Arc;

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
    use voiceos_core::{ConversationEngine, ConversationStore, ProviderRouter, RoutingPolicy};
    use voiceos_ontology::{Interpreter, OntologyStore};

    use super::router;
    use crate::state::AppState;

    const OWNER: &str = "owner-a";
    const TOKEN: &str = "test-device-token";

    fn authenticated_router() -> (Router, Arc<ConversationStore>, std::path::PathBuf) {
        let store = Arc::new(ConversationStore::in_memory().unwrap());
        store.migrate_devices_to_owner(OWNER).unwrap();
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
        let state = AppState {
            engine: Arc::new(ConversationEngine::new(store.clone())),
            router: Arc::new(ProviderRouter::new(RoutingPolicy::default())),
            ontology,
            store: store.clone(),
            legacy_audit_path: legacy_audit_path.clone(),
            require_device_auth: true,
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
                .header("content-type", "application/json")
                .body(Body::from(event.to_string()))
                .unwrap()
        };

        let (first_status, first) = response(app.clone(), request()).await;
        let (second_status, second) = response(app, request()).await;

        assert_eq!(first_status, StatusCode::CREATED);
        assert_eq!(second_status, StatusCode::CREATED);
        assert_eq!(first["capture"]["id"], second["capture"]["id"]);
        assert_eq!(first["capture"]["source"], "fieldy");
        assert_eq!(store.personal_inbox(OWNER).unwrap().len(), 1);
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
}
