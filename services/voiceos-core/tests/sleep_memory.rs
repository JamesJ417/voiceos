use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;
use voiceos_core::{
    CognitiveStatus, ConversationStore, FixtureSleepProposalGenerator, MemoryKind, ProposedMemory,
    Provider, ProviderCompletion, ProviderError, ProviderRequest, ProviderRouter, Role,
    RoutedSleepProposalGenerator, RoutingPolicy, SLEEP_OPERATION_VERSION, SleepConfig, SleepError,
    SleepMemoryAuthority, SleepProposalBatch, SleepProposalGenerator, ToolCall, Usage,
};

const OWNER: &str = "owner-test";

fn seeded(content: &str) -> (Arc<ConversationStore>, SleepMemoryAuthority) {
    let store = Arc::new(ConversationStore::in_memory().unwrap());
    store.migrate_devices_to_owner(OWNER).unwrap();
    let conversation = store
        .resolve_owner_conversation(OWNER, "pixel", None)
        .unwrap();
    store
        .append_message(&conversation, Role::User, content, None)
        .unwrap();
    let authority = SleepMemoryAuthority::new(store.clone());
    (store, authority)
}

fn config() -> SleepConfig {
    SleepConfig {
        minimum_salience: 0.0,
        ..SleepConfig::default()
    }
}

#[derive(Clone, Copy)]
enum Behavior {
    Semantic,
    SemanticTwo,
    Dream,
    Protected,
    Invalid,
    Duplicate,
    Contradiction,
    Skill,
    Capability,
    DreamMismatch,
    Fail,
}

struct Generator(Behavior);

impl SleepProposalGenerator for Generator {
    fn generate(
        &self,
        events: &[voiceos_core::RawMemoryEvent],
    ) -> Result<SleepProposalBatch, SleepError> {
        if matches!(self.0, Behavior::Fail) {
            return Err(SleepError::InvalidProposal("fixture interruption".into()));
        }
        let event = events.first().expect("selected event");
        let (proposal_kind, kind, status, content, protected, capabilities) = match self.0 {
            Behavior::Semantic => (
                "memory",
                Some(MemoryKind::Semantic),
                CognitiveStatus::SupportedInference,
                "The user prefers teal interfaces.",
                false,
                vec![],
            ),
            Behavior::SemanticTwo => (
                "memory",
                Some(MemoryKind::Semantic),
                CognitiveStatus::SupportedInference,
                "The user prefers concise morning reports.",
                false,
                vec![],
            ),
            Behavior::Dream => (
                "memory",
                Some(MemoryKind::DreamAssociation),
                CognitiveStatus::DreamAssociation,
                "Teal interfaces may improve planning momentum.",
                false,
                vec![],
            ),
            Behavior::Protected => (
                "memory",
                Some(MemoryKind::IdentityDoctrine),
                CognitiveStatus::SupportedInference,
                "VIC must change its governing doctrine.",
                true,
                vec![],
            ),
            Behavior::Invalid => (
                "memory",
                Some(MemoryKind::Semantic),
                CognitiveStatus::SupportedInference,
                "Missing provenance.",
                false,
                vec![],
            ),
            Behavior::Duplicate => (
                "memory",
                Some(MemoryKind::Semantic),
                CognitiveStatus::SupportedInference,
                "Duplicate staged memory.",
                false,
                vec![],
            ),
            Behavior::Contradiction => (
                "contradiction",
                None,
                CognitiveStatus::Disputed,
                "A new statement conflicts with an existing preference.",
                false,
                vec![],
            ),
            Behavior::Skill => (
                "skill",
                None,
                CognitiveStatus::WorkingHypothesis,
                "Candidate skill: summarize the morning report.",
                false,
                vec![],
            ),
            Behavior::Capability => (
                "memory",
                Some(MemoryKind::Semantic),
                CognitiveStatus::SupportedInference,
                "Unsafe proposal.",
                false,
                vec!["filesystem.write".to_owned()],
            ),
            Behavior::DreamMismatch => (
                "memory",
                Some(MemoryKind::DreamAssociation),
                CognitiveStatus::VerifiedFact,
                "Invalid dream typing.",
                false,
                vec![],
            ),
            Behavior::Fail => unreachable!(),
        };
        let source = if matches!(self.0, Behavior::Invalid) {
            vec![]
        } else {
            vec![event.id.clone()]
        };
        let proposal = ProposedMemory {
            proposal_kind: proposal_kind.into(),
            memory_kind: kind,
            cognitive_status: status,
            content: content.into(),
            confidence: 0.91,
            source_event_ids: source,
            supporting_event_ids: vec![event.id.clone()],
            contradicting_event_ids: vec![],
            payload: json!({"conflict_kind":"preference","invalidation_conditions":["user correction"]}),
            provider: "fixture".into(),
            model_version: Some("v1".into()),
            operation_version: SLEEP_OPERATION_VERSION.into(),
            protected,
            requested_capabilities: capabilities,
        };
        let proposals = if matches!(self.0, Behavior::Duplicate) {
            vec![proposal.clone(), proposal]
        } else {
            vec![proposal]
        };
        Ok(SleepProposalBatch {
            proposals,
            provider_calls: vec![],
        })
    }
}

#[test]
fn raw_events_are_append_only() {
    let directory = tempdir().unwrap();
    let db = directory.path().join("memory.sqlite3");
    let store = Arc::new(ConversationStore::open(&db).unwrap());
    store.migrate_devices_to_owner(OWNER).unwrap();
    let conversation = store
        .resolve_owner_conversation(OWNER, "pixel", None)
        .unwrap();
    store
        .append_message(
            &conversation,
            Role::User,
            "Remember this immutable statement",
            None,
        )
        .unwrap();
    let authority = SleepMemoryAuthority::new(store);
    authority.ingest_conversation_events(OWNER, 10).unwrap();
    let id = authority.raw_events(OWNER, 10).unwrap()[0].id.clone();
    let connection = Connection::open(db).unwrap();
    assert!(
        connection
            .execute(
                "UPDATE raw_memory_events SET source_kind='changed' WHERE event_id=?1",
                [&id]
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM raw_memory_events WHERE event_id=?1", [&id])
            .is_err()
    );
}

#[test]
fn failed_cycle_commits_nothing() {
    let (_, authority) = seeded("Remember the teal preference");
    assert!(
        authority
            .run_cycle(
                OWNER,
                "commit",
                "test",
                config(),
                &Generator(Behavior::Fail)
            )
            .is_err()
    );
    assert_eq!(
        authority.latest_cycle(OWNER).unwrap().unwrap().status,
        "failed"
    );
    assert!(
        authority
            .search(OWNER, "teal", false, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn interrupted_cycle_can_resume() {
    let (_, authority) = seeded("Remember the interface preference");
    let _ = authority.run_cycle(
        OWNER,
        "commit",
        "test",
        config(),
        &Generator(Behavior::Fail),
    );
    let cycle = authority.latest_cycle(OWNER).unwrap().unwrap();
    let (resumed, _) = authority
        .resume_cycle(&cycle.id, &FixtureSleepProposalGenerator)
        .unwrap();
    assert_eq!(resumed.status, "completed");
}

#[test]
fn malformed_proposal_is_rejected() {
    let (_, authority) = seeded("Remember a preference");
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Invalid),
        )
        .unwrap();
    assert_eq!(report.proposals_rejected, 1);
    assert!(
        authority
            .search(OWNER, "missing", false, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn dream_type_and_status_must_match() {
    let (_, authority) = seeded("Consider an analogy");
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::DreamMismatch),
        )
        .unwrap();
    assert_eq!(report.proposals_rejected, 1);
}

#[test]
fn dreams_are_quarantined_from_ordinary_retrieval() {
    let (_, authority) = seeded("Consider planning and teal interfaces");
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Dream),
        )
        .unwrap();
    assert_eq!(report.dream_associations, 1);
    assert!(
        authority
            .search(OWNER, "momentum", false, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        authority.search(OWNER, "momentum", true, 10).unwrap().len(),
        1
    );
}

#[test]
fn automatic_cycle_cannot_change_identity_doctrine() {
    let (_, authority) = seeded("Remember VIC identity rules");
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "scheduled",
            config(),
            &Generator(Behavior::Protected),
        )
        .unwrap();
    assert_eq!(report.protected_changes_awaiting_approval, 1);
    assert!(
        authority
            .search(OWNER, "doctrine", false, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn dry_run_does_not_mutate_active_memory() {
    let (_, authority) = seeded("Remember teal interfaces");
    let (cycle, report) = authority
        .run_cycle(
            OWNER,
            "dry_run",
            "test",
            config(),
            &Generator(Behavior::Semantic),
        )
        .unwrap();
    assert_eq!(report.memories_committed, 0);
    assert_eq!(cycle.mode, "dry_run");
    assert!(
        authority
            .search(OWNER, "teal", false, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn approved_dry_run_can_commit_without_model_replay() {
    let (_, authority) = seeded("Remember teal interfaces");
    let (cycle, _) = authority
        .run_cycle(
            OWNER,
            "dry_run",
            "test",
            config(),
            &Generator(Behavior::Semantic),
        )
        .unwrap();
    let (_, report) = authority.commit_staged_cycle(&cycle.id).unwrap();
    assert_eq!(report.memories_committed, 1);
}

#[test]
fn duplicate_active_memories_are_not_created() {
    let (_, authority) = seeded("Remember teal interfaces");
    authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Semantic),
        )
        .unwrap();
    let (_, second) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Semantic),
        )
        .unwrap();
    assert_eq!(second.memories_committed, 0);
    assert_eq!(authority.search(OWNER, "teal", false, 10).unwrap().len(), 1);
}

#[test]
fn staged_duplicates_are_collapsed_before_retrieval() {
    let (_, authority) = seeded("Remember a duplicate test");
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Duplicate),
        )
        .unwrap();
    assert!(report.retrieval_quality_passed);
    assert_eq!(
        authority
            .search(OWNER, "duplicate", false, 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn contradictions_are_recorded_without_overwrite() {
    let (_, authority) = seeded("Remember teal interfaces");
    authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Semantic),
        )
        .unwrap();
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Contradiction),
        )
        .unwrap();
    assert_eq!(report.contradictions_detected, 1);
    assert_eq!(authority.search(OWNER, "teal", false, 10).unwrap().len(), 1);
}

#[test]
fn skill_candidates_are_not_activated_as_memory() {
    let (store, authority) = seeded("We repeated a useful workflow");
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Skill),
        )
        .unwrap();
    assert_eq!(report.skill_candidates, 1);
    assert_eq!(
        store
            .skill_proposals(OWNER, Some("proposed"), 10)
            .unwrap()
            .len(),
        1
    );
    assert!(
        authority
            .search(OWNER, "candidate", false, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn model_proposals_cannot_request_capabilities() {
    let (_, authority) = seeded("Try an unsafe proposal");
    let (_, report) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Capability),
        )
        .unwrap();
    assert_eq!(report.proposals_rejected, 1);
}

#[test]
fn rollback_deactivates_only_cycle_derived_memory_and_preserves_raw_events() {
    let (_, authority) = seeded("Remember teal interfaces and concise reports");
    let (_, _) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::Semantic),
        )
        .unwrap();
    let (second, _) = authority
        .run_cycle(
            OWNER,
            "commit",
            "test",
            config(),
            &Generator(Behavior::SemanticTwo),
        )
        .unwrap();
    let raw_before = authority.raw_events(OWNER, 100).unwrap();
    authority
        .rollback_cycle(&second.id, "test rollback")
        .unwrap();
    assert_eq!(authority.search(OWNER, "teal", false, 10).unwrap().len(), 1);
    assert!(
        authority
            .search(OWNER, "concise", false, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(raw_before, authority.raw_events(OWNER, 100).unwrap());
}

struct RecordingProvider {
    name: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
    tools: bool,
}
impl Provider for RecordingProvider {
    fn name(&self) -> &str {
        self.name
    }
    fn complete(&self, _: &ProviderRequest) -> Result<ProviderCompletion, ProviderError> {
        self.calls.lock().unwrap().push(self.name.into());
        Ok(ProviderCompletion {
            text: "[]".into(),
            provider: self.name.into(),
            tool_calls: if self.tools {
                vec![ToolCall {
                    name: "forbidden".into(),
                    arguments: json!({}),
                }]
            } else {
                vec![]
            },
            usage: Usage::default(),
        })
    }
}

#[test]
fn routed_generator_uses_gemma_and_gpt_oss_without_tools() {
    let calls = Arc::new(Mutex::new(vec![]));
    let mut router = ProviderRouter::new(RoutingPolicy::default());
    router.register(Arc::new(RecordingProvider {
        name: "gemma",
        calls: calls.clone(),
        tools: false,
    }));
    router.register(Arc::new(RecordingProvider {
        name: "gpt-oss",
        calls: calls.clone(),
        tools: false,
    }));
    let (_, authority) = seeded("Remember provider routing");
    authority
        .run_cycle(
            OWNER,
            "dry_run",
            "test",
            config(),
            &RoutedSleepProposalGenerator::new(Arc::new(router)),
        )
        .unwrap();
    assert_eq!(&*calls.lock().unwrap(), &["gemma", "gpt-oss"]);
}

#[test]
fn routed_generator_rejects_any_tool_call() {
    let calls = Arc::new(Mutex::new(vec![]));
    let mut router = ProviderRouter::new(RoutingPolicy::default());
    router.register(Arc::new(RecordingProvider {
        name: "gemma",
        calls: calls.clone(),
        tools: true,
    }));
    router.register(Arc::new(RecordingProvider {
        name: "gpt-oss",
        calls,
        tools: false,
    }));
    let (_, authority) = seeded("Remember tool isolation");
    assert!(
        authority
            .run_cycle(
                OWNER,
                "dry_run",
                "test",
                config(),
                &RoutedSleepProposalGenerator::new(Arc::new(router))
            )
            .is_err()
    );
}
