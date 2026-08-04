use serde_json::json;

use crate::{ConversationStore, StoreError, TaskInitiative, TaskRecord};

/// Creates the one durable, safe-by-default kickoff job for a newly captured task.
pub fn begin_task_initiative(
    store: &ConversationStore,
    owner_id: &str,
    task: &TaskRecord,
    actor: &str,
) -> Result<TaskInitiative, StoreError> {
    let capabilities = infer_capabilities(task);
    let next_actions = next_actions(task);
    let job = store.create_job(
        owner_id,
        Some(&task.id),
        &format!("task-initiative:{}", task.id),
        json!(capabilities),
    )?;
    let job = store
        .transition_job_status(owner_id, &job.id, "proposed", "approved")?
        .unwrap_or(job);
    let initiative = TaskInitiative {
        task_id: task.id.clone(),
        job_id: job.id,
        status: "queued".to_owned(),
        summary: format!(
            "VIC analyzed {} and queued safe work to move it forward.",
            task.title
        ),
        capabilities,
        started_actions: vec![
            "Scanned the task and its observable outcome".to_owned(),
            "Identified useful work VIC can perform".to_owned(),
            "Queued a permissioned Hermes run".to_owned(),
        ],
        next_actions,
        approval_boundary: "VIC may analyze, research, draft, and organize automatically. External communication, purchases, destructive changes, credentials, and administrative actions still require explicit approval.".to_owned(),
    };
    store.append_execution_event(
        owner_id,
        &task.id,
        "task.initiative.queued",
        actor,
        serde_json::to_value(&initiative)?,
    )?;
    Ok(initiative)
}

fn infer_capabilities(task: &TaskRecord) -> Vec<String> {
    let text = format!("{} {}", task.title, task.observable_outcome).to_lowercase();
    let mut capabilities = vec![
        "task.analysis".to_owned(),
        "task.next_actions".to_owned(),
        "task.blocker_detection".to_owned(),
    ];
    if contains_any(
        &text,
        &["research", "find", "compare", "look up", "investigate"],
    ) {
        capabilities.push("research.prepare_evidence".to_owned());
    }
    if contains_any(
        &text,
        &["write", "draft", "email", "message", "document", "recipe"],
    ) {
        capabilities.push("content.prepare_draft".to_owned());
    }
    if contains_any(
        &text,
        &["build", "fix", "code", "implement", "test", "refactor"],
    ) {
        capabilities.push("project.inspect_and_plan".to_owned());
    }
    if contains_any(
        &text,
        &["print", "laminate", "buy", "call", "schedule", "install"],
    ) {
        capabilities.push("task.prepare_checklist".to_owned());
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn next_actions(task: &TaskRecord) -> Vec<String> {
    vec![
        format!("Confirm the completion target: {}", task.observable_outcome),
        format!("Prepare the smallest useful next step for {}", task.title),
        "Identify missing information, dependencies, and approval-gated actions".to_owned(),
    ]
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use crate::ConversationStore;

    use super::begin_task_initiative;

    #[test]
    fn queues_one_safe_kickoff_job_with_audited_capabilities() {
        let store = ConversationStore::in_memory().unwrap();
        let task = store
            .create_task(
                "owner",
                None,
                None,
                "Print and laminate recipe cards",
                "All recipe cards are printed and laminated",
                60,
            )
            .unwrap();
        let initiative = begin_task_initiative(&store, "owner", &task, "device:pixel").unwrap();
        assert_eq!(initiative.status, "queued");
        assert!(
            initiative
                .capabilities
                .contains(&"task.prepare_checklist".to_owned())
        );
        let job = store
            .initiative_job_for_task("owner", &task.id)
            .unwrap()
            .unwrap();
        assert_eq!(job.status, "approved");
        assert!(
            store
                .transition_job_status("owner", &job.id, "approved", "running")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .transition_job_status("owner", &job.id, "approved", "running")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.execution_events("owner", &task.id, 20).unwrap().len(),
            1
        );
    }
}
