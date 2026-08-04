mod activity;
mod artifacts;
mod attention;
mod auth;
mod automations;
mod conversations;
mod documents;
mod error;
mod floor;
mod health;
mod ontology;
mod outreach;
mod planning;
mod skills;
mod tasks;
mod turns;
mod updates;

use axum::Router;
use axum::routing::{delete, get, post};

use crate::state::AppState;

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health::health))
        .route("/v1/activity", get(activity::list))
        .route("/v1/updates", get(updates::list))
        .route("/v1/updates/{update_id}/decision", post(updates::decide))
        .route("/v1/updates/{update_id}/actions", post(updates::action))
        .route(
            "/v1/attention",
            get(attention::list).post(attention::upsert),
        )
        .route("/v1/attention/{attention_id}/actions", post(attention::act))
        .route(
            "/v1/calendar/events",
            get(planning::list_calendar).post(planning::upsert_calendar),
        )
        .route("/v1/plans/daily/work", post(planning::daily_plan))
        .route(
            "/v1/automations",
            get(automations::list).post(automations::create),
        )
        .route(
            "/v1/automations/{automation_id}/enabled",
            post(automations::set_enabled),
        )
        .route("/v1/artifacts", get(artifacts::list))
        .route("/v1/artifacts/pdfs", post(artifacts::create_pdf))
        .route("/v1/artifacts/events", get(artifacts::events))
        .route("/v1/artifacts/{artifact_id}", get(artifacts::get))
        .route(
            "/v1/artifacts/{artifact_id}/preview",
            get(artifacts::preview),
        )
        .route(
            "/v1/artifacts/{artifact_id}/download",
            get(artifacts::download),
        )
        .route(
            "/v1/artifacts/{artifact_id}/revisions",
            post(artifacts::revise_pdf),
        )
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
        .route(
            "/v1/tasks/{task_id}/schedule",
            post(planning::set_task_schedule),
        )
        .route("/v1/outreach", get(outreach::list).post(outreach::create))
        .route(
            "/v1/outreach/policy",
            get(outreach::policy).post(outreach::update_policy),
        )
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
            "/internal/v1/artifacts/tools",
            post(artifacts::internal_tool),
        )
        .route(
            "/internal/v1/ontology/interpret",
            post(ontology::interpret_deterministic),
        )
        .route(
            "/internal/v1/ontology/tools/validate",
            post(ontology::validate_tool),
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
        .route("/internal/v1/updates/discover", post(updates::discover))
        .with_state(state)
}
