mod auth;
mod conversations;
mod documents;
mod error;
mod events;
mod floor;
mod health;
mod ontology;
mod outreach;
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
        .route("/v1/events", get(events::stream))
        .route("/v1/events/recovery", get(events::recovery))
        .route("/v1/turns/text", post(turns::turn))
        .route("/v1/conversations/active", get(conversations::active))
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
            "/v1/memory/sleep-cycles",
            get(sleep_cycles::list).post(sleep_cycles::start),
        )
        .route(
            "/v1/memory/sleep-cycles/{sleep_cycle_id}",
            get(sleep_cycles::detail),
        )
        .route(
            "/v1/skills/usages/{usage_id}/feedback",
            post(skills::review_usage),
        )
        .route("/v1/skills/{skill_id}/status", post(skills::set_status))
        .route("/v1/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route("/v1/tasks/{task_id}", get(tasks::task_detail))
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
