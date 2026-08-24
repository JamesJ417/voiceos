use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;
use voiceos_core::{
    ConversationStore, NewProactiveCandidate, ProactiveDraftingContract, ProactiveDraftingInput,
};

const FUTURE: &str = "2099-09-01T12:00:00Z";

struct StaticDrafter(String);

impl ProactiveDraftingContract for StaticDrafter {
    fn draft(&self, _input: &ProactiveDraftingInput) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

fn candidate(store: &ConversationStore, owner: &str) -> voiceos_core::ProactiveCandidate {
    candidate_with_expiry(store, owner, FUTURE)
}

fn candidate_with_expiry(
    store: &ConversationStore,
    owner: &str,
    expires_at: &str,
) -> voiceos_core::ProactiveCandidate {
    let project = store.create_project(owner, None, "Approved scope").unwrap();
    let task = store
        .create_task(
            owner,
            Some(&project.id),
            None,
            "Approved evidence",
            "Task used as authoritative evidence",
            10,
        )
        .unwrap();
    store
        .create_proactive_candidate(NewProactiveCandidate {
            owner_id: owner.into(),
            subscription_id: None,
            project_id: Some(project.id),
            reason: "Project has had no recent progress".into(),
            evidence: json!([{"source_type":"task", "source_id":task.id}]),
            priority: "normal".into(),
            confidence: 0.8,
            expires_at: expires_at.into(),
            deduplication_key: format!("candidate-{owner}"),
            provenance: "detector://test".into(),
        })
        .unwrap()
}

fn evidence_id(candidate: &voiceos_core::ProactiveCandidate) -> &str {
    candidate.evidence[0]["source_id"].as_str().unwrap()
}

fn valid_draft(candidate: &voiceos_core::ProactiveCandidate) -> String {
    let evidence_id = evidence_id(candidate);
    json!({
        "owner_id": candidate.owner_id,
        "project_id": candidate.project_id,
        "confidence": 0.91,
        "rationale": "The approved local evidence shows no recent project progress.",
        "message": "Would you like to review the project status?",
        "reason_category": "stale_project",
        "urgency": "normal",
        "risk_class": "low",
        "approval_state": "pending_review",
        "evidence_ids": [evidence_id],
        "expires_at": FUTURE
    })
    .to_string()
}

#[test]
fn well_formed_local_draft_creates_pending_review_proposal_and_preserves_draft() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("voiceos.db");
    let store = ConversationStore::open(&database_path).unwrap();
    let candidate = candidate(&store, "owner-a");
    let draft = valid_draft(&candidate);
    let tasks_before = store.tasks("owner-a", true, 20).unwrap();
    let memories_before = store.memories_for_owner("owner-a", 20).unwrap();
    let outreaches_before = store.outreaches("owner-a", true, 20).unwrap();
    let deliveries_before: i64 = Connection::open(&database_path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM outreach_deliveries", [], |row| {
            row.get(0)
        })
        .unwrap();

    let proposal = store
        .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(draft.clone()))
        .unwrap();

    assert_eq!(proposal.owner_id, "owner-a");
    assert_eq!(proposal.candidate_id, candidate.id);
    assert_eq!(proposal.original_draft, draft);
    assert_eq!(
        proposal.editable_draft,
        "Would you like to review the project status?"
    );
    assert_eq!(proposal.approval_state, "pending_review");
    assert_eq!(proposal.risk_class, "low");
    assert_eq!(proposal.channel, "internal_queue");
    assert_eq!(store.tasks("owner-a", true, 20).unwrap(), tasks_before);
    assert_eq!(
        store.memories_for_owner("owner-a", 20).unwrap(),
        memories_before
    );
    assert_eq!(
        store.outreaches("owner-a", true, 20).unwrap(),
        outreaches_before
    );
    let deliveries_after: i64 = Connection::open(&database_path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM outreach_deliveries", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(deliveries_after, deliveries_before);
    assert!(
        store
            .outreach_delivery("owner-a", &proposal.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn malformed_structured_draft_is_rejected_without_a_proposal() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate(&store, "owner-a");

    assert!(
        store
            .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter("not json".into()))
            .is_err()
    );
    assert!(store.outreaches("owner-a", true, 20).unwrap().is_empty());
}

#[test]
fn draft_with_unsupported_evidence_is_rejected() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate(&store, "owner-a");
    let draft = valid_draft(&candidate).replace(evidence_id(&candidate), "other-owner-event");

    assert!(
        store
            .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(draft))
            .is_err()
    );
    assert!(store.outreaches("owner-a", true, 20).unwrap().is_empty());
}

#[test]
fn draft_with_unsupported_urgency_or_hidden_action_content_is_rejected() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate(&store, "owner-a");
    let unsupported_urgency = valid_draft(&candidate).replace("\"normal\"", "\"immediate\"");
    let hidden_action = valid_draft(&candidate).replace(
        "Would you like to review the project status?",
        "Create a task and send this message now.",
    );

    assert!(
        store
            .draft_proactive_proposal(
                "owner-a",
                &candidate.id,
                &StaticDrafter(unsupported_urgency)
            )
            .is_err()
    );
    assert!(
        store
            .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(hidden_action))
            .is_err()
    );
    assert!(store.outreaches("owner-a", true, 20).unwrap().is_empty());
}

#[test]
fn cross_owner_candidate_is_not_drafted() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate(&store, "owner-b");
    let draft = valid_draft(&candidate);

    assert!(
        store
            .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(draft.clone()))
            .is_err()
    );
    assert!(store.outreaches("owner-a", true, 20).unwrap().is_empty());
    assert!(store.outreaches("owner-b", true, 20).unwrap().is_empty());
}

#[test]
fn draft_rejects_unrelated_output_fields() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate(&store, "owner-a");
    let draft = valid_draft(&candidate).replace('}', ",\"unrelated_context\":\"private note\"}");

    assert!(
        store
            .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(draft))
            .is_err()
    );
    assert!(store.outreaches("owner-a", true, 20).unwrap().is_empty());
}

#[test]
fn draft_rejects_case_insensitive_outbound_and_destructive_action_language() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate(&store, "owner-a");

    for message in [
        "Email the user now.",
        "POST this to Slack.",
        "Delete the project.",
        "EXECUTE the deployment.",
        "Deploy the release now.",
        "Transfer funds to the vendor.",
        "Invite the contractor.",
        "Approve the purchase order.",
        "Schedule a meeting.",
    ] {
        let draft = valid_draft(&candidate)
            .replace("Would you like to review the project status?", message);
        assert!(
            store
                .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(draft))
                .is_err(),
            "action language must fail closed: {message}"
        );
    }
    assert!(store.outreaches("owner-a", true, 20).unwrap().is_empty());
}

#[test]
fn drafter_rejects_review_question_action_clause_bypasses() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate(&store, "owner-a");

    for message in [
        "Would you like to review the project and email the user?",
        "Would you like to review the project status; Email the user?",
        "Would you like to review the project status\nDelete the project?",
    ] {
        let draft = valid_draft(&candidate)
            .replace("Would you like to review the project status?", message);
        assert!(
            store
                .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(draft))
                .is_err(),
            "review-question action clause must be rejected: {message}"
        );
    }
}

#[test]
fn draft_rejects_output_expiry_after_candidate_expiry() {
    let store = ConversationStore::in_memory().unwrap();
    let candidate = candidate_with_expiry(&store, "owner-a", "2099-09-01T12:00:00Z");
    let draft = valid_draft(&candidate).replace(FUTURE, "2099-09-01T12:00:01Z");

    assert!(
        store
            .draft_proactive_proposal("owner-a", &candidate.id, &StaticDrafter(draft))
            .is_err()
    );
    assert!(store.outreaches("owner-a", true, 20).unwrap().is_empty());
}
