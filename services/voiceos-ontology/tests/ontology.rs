use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use voiceos_ontology::{
    AliasInput, CanonicalRequest, Catalog, Confidence, DecisionStatus, EntityKind, IntentId,
    Interpreter, ModelCandidate, ModelFallback, OntologyStore, ResolutionSource,
    ValidatorDisposition,
};

struct FixedFallback(ModelCandidate);

impl ModelFallback for FixedFallback {
    fn resolve(&self, _phrase: &str, _catalog: &Catalog) -> Result<Option<ModelCandidate>, String> {
        Ok(Some(self.0.clone()))
    }
}

fn interpreter() -> Interpreter {
    Interpreter::new(Arc::new(OntologyStore::in_memory().unwrap()))
}

#[test]
fn equivalent_speed_phrases_resolve_to_canonical_requests() {
    let interpreter = interpreter();
    let first = interpreter
        .interpret("owner", "Set speech to two times speed")
        .unwrap();
    let second = interpreter.interpret("owner", "Double speed").unwrap();
    assert_eq!(first.status, DecisionStatus::Resolved);
    assert_eq!(
        first.interpretation.unwrap().intent.0,
        "voice.playback_speed.set"
    );
    assert_eq!(second.interpretation.unwrap().arguments["rate"], json!(2.0));
}

#[test]
fn seeded_terms_cover_provider_memory_documents_health_services_and_approval() {
    let interpreter = interpreter();
    let cases = [
        ("use gpt oss", "provider.select"),
        (
            "remember that my favorite color is amber",
            "memory.remember",
        ),
        ("show my files", "artifact.search"),
        ("show my uploaded documents", "knowledge.document.list"),
        ("how much disk space is free", "system.disk.check"),
        ("is ollama running", "system.service.check"),
        ("approve", "approval.decide"),
    ];
    for (phrase, intent) in cases {
        let decision = interpreter.interpret("owner", phrase).unwrap();
        assert_eq!(
            decision.interpretation.unwrap().intent.0,
            intent,
            "phrase: {phrase}"
        );
    }
}

#[test]
fn catalog_v2_adds_operational_entity_kinds_and_migrates_v1_intents() {
    let catalog = Catalog::seeded();
    assert_eq!(catalog.version(), 2);
    assert!(catalog.supports_version(1));
    for kind in [
        EntityKind::Artifact,
        EntityKind::Task,
        EntityKind::Person,
        EntityKind::Project,
        EntityKind::Skill,
        EntityKind::Email,
        EntityKind::Location,
    ] {
        assert!(catalog.entity_kinds().contains(&kind));
    }
    assert_eq!(
        catalog.canonical_intent(&IntentId::from("document.list")).0,
        "knowledge.document.list"
    );
}

#[test]
fn legacy_ontology_database_is_migrated_without_losing_compatibility() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy-ontology.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE ontology_interpretations (
                interpretation_id TEXT PRIMARY KEY,
                owner_id TEXT NOT NULL,
                original_phrase TEXT NOT NULL,
                normalized_phrase TEXT NOT NULL,
                interpretation_json TEXT NOT NULL,
                status TEXT NOT NULL,
                validation_json TEXT NOT NULL,
                corrections_json TEXT NOT NULL,
                final_decision TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
    drop(connection);

    OntologyStore::open(&path).unwrap();

    let connection = rusqlite::Connection::open(&path).unwrap();
    let columns = connection
        .prepare("PRAGMA table_info(ontology_interpretations)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(columns.contains(&"catalog_version".to_owned()));
    assert!(columns.contains(&"validator_json".to_owned()));
}

#[test]
fn alias_matching_respects_token_boundaries() {
    let interpreter = interpreter();
    let decision = interpreter
        .interpret_deterministic("owner", "use resolution for this")
        .unwrap();
    assert!(decision.interpretation.is_none());
    assert_eq!(
        decision.validator.disposition,
        ValidatorDisposition::AskClarifyingQuestion
    );
}

#[test]
fn structured_tools_must_receive_an_executable_ontology_decision() {
    let interpreter = interpreter();
    let valid = interpreter
        .validate_tool(
            "owner",
            "artifact.pdf.create",
            std::collections::BTreeMap::from([("title".to_owned(), json!("Recipe cards"))]),
            1.0,
            ResolutionSource::Deterministic,
        )
        .unwrap();
    assert_eq!(valid.catalog_version, 2);
    assert_eq!(valid.validator.disposition, ValidatorDisposition::Execute);

    let incomplete = interpreter
        .validate_tool(
            "owner",
            "artifact.pdf.create",
            std::collections::BTreeMap::new(),
            1.0,
            ResolutionSource::Deterministic,
        )
        .unwrap();
    assert_eq!(
        incomplete.validator.disposition,
        ValidatorDisposition::AskClarifyingQuestion
    );

    let unknown = interpreter
        .validate_tool(
            "owner",
            "unregistered.tool",
            std::collections::BTreeMap::new(),
            1.0,
            ResolutionSource::Deterministic,
        )
        .unwrap();
    assert_eq!(unknown.validator.disposition, ValidatorDisposition::Reject);
}

#[test]
fn actual_vic_transcripts_form_a_regression_corpus() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/vic_transcripts.json")).unwrap();
    let interpreter = interpreter();
    for case in cases {
        let phrase = case["phrase"].as_str().unwrap();
        let decision = interpreter.interpret("owner", phrase).unwrap();
        assert_eq!(
            decision.interpretation.as_ref().unwrap().intent.0,
            case["expected_intent"].as_str().unwrap(),
            "phrase: {phrase}"
        );
        assert_eq!(
            serde_json::to_value(decision.validator.disposition).unwrap(),
            case["expected_disposition"],
            "phrase: {phrase}"
        );
    }
}

#[test]
fn model_fallback_is_structured_and_validated_against_the_catalog() {
    let candidate = ModelCandidate {
        intent: IntentId("voice.playback_speed.set".to_owned()),
        entities: vec![],
        arguments: BTreeMap::from([("rate".to_owned(), json!(4.0))]),
        confidence: 0.91,
    };
    let interpreter = interpreter().with_fallback(Arc::new(FixedFallback(candidate)));
    let decision = interpreter
        .interpret("owner", "make your talking extremely rapid")
        .unwrap();
    assert_eq!(decision.status, DecisionStatus::Rejected);
    assert!(
        decision
            .validation_issues
            .iter()
            .any(|issue| issue.code == "argument_out_of_range")
    );
}

#[test]
fn approved_aliases_are_owner_scoped_and_require_no_retraining() {
    let interpreter = interpreter();
    interpreter
        .approve_alias(
            "alice",
            &AliasInput {
                phrase: "the mining box".to_owned(),
                entity_kind: EntityKind::Device,
                entity_id: "gpu-rig".to_owned(),
            },
        )
        .unwrap();
    let alice = interpreter
        .interpret("alice", "check system health on the mining box")
        .unwrap();
    assert_eq!(
        alice.interpretation.as_ref().unwrap().source,
        ResolutionSource::ApprovedAlias
    );
    assert_eq!(alice.interpretation.unwrap().entities[0].id, "gpu-rig");

    let bob = interpreter
        .interpret("bob", "check system health on the mining box")
        .unwrap();
    assert!(bob.interpretation.unwrap().entities.is_empty());
}

#[test]
fn original_phrase_interpretation_and_correction_are_durable() {
    let interpreter = interpreter();
    let initial = interpreter.interpret("owner", "show my files").unwrap();
    let correction = CanonicalRequest {
        intent: IntentId("memory.list".to_owned()),
        entities: vec![],
        arguments: BTreeMap::new(),
        confidence: Confidence::new(0.1),
        source: ResolutionSource::ModelFallback,
    };
    let corrected = interpreter
        .correct("owner", &initial.id, correction, "I meant memories")
        .unwrap();
    assert_eq!(corrected.original_phrase, "show my files");
    assert_eq!(corrected.final_decision, DecisionStatus::Resolved);
    assert_eq!(corrected.corrections.len(), 1);
    assert_eq!(corrected.catalog_version, 2);
    let corpus = interpreter.correction_regression_corpus("owner").unwrap();
    assert_eq!(corpus.len(), 1);
    assert_eq!(corpus[0].phrase, "show my files");
    assert!(corpus[0].corrected);
    assert_eq!(
        interpreter.get("owner", &initial.id).unwrap().unwrap(),
        corrected
    );
}

#[test]
fn spoken_task_phrases_resolve_to_valid_canonical_requests() {
    let interpreter = interpreter();
    let created = interpreter
        .interpret("owner", "Add a task to call the dentist for 15 minutes")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(created.intent.0, "task.create");
    assert_eq!(created.arguments["title"], json!("call the dentist"));
    assert_eq!(created.arguments["estimated_minutes"], json!(15));

    let natural_reminder = interpreter
        .interpret(
            "owner",
            "Remind me that we need to work on printing all the recipe cards and laminating them as a task.",
        )
        .unwrap();
    assert_eq!(natural_reminder.final_decision, DecisionStatus::Resolved);
    let natural_reminder = natural_reminder.interpretation.unwrap();
    assert_eq!(natural_reminder.intent.0, "task.create");
    assert_eq!(
        natural_reminder.arguments["title"],
        json!("work on printing all the recipe cards and laminating them")
    );
    assert_eq!(natural_reminder.confidence.score, 0.98);

    let listed = interpreter
        .interpret("owner", "What are my tasks?")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(listed.intent.0, "task.list");

    let reviewed = interpreter
        .interpret(
            "owner",
            "Look at the task list and tell me what we need to work on.",
        )
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(reviewed.intent.0, "task.review");

    let assistance = interpreter
        .interpret("owner", "How can you help me with my task list?")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(assistance.intent.0, "task.assist");

    let reported_variant = interpreter
        .interpret("owner", "What is a list of tasks that we need to work on?")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(reported_variant.intent.0, "task.review");

    let flexible_list = interpreter
        .interpret("owner", "Give me a list of the tasks on the board.")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(flexible_list.intent.0, "task.list");

    let create_with_work_on = interpreter
        .interpret("owner", "Add a task to work on the recipe cards.")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(create_with_work_on.intent.0, "task.create");

    let started = interpreter
        .interpret("owner", "Start the next task")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(started.intent.0, "task.start");
    assert_eq!(started.arguments["reference"], json!("next"));

    let completed = interpreter
        .interpret("owner", "Mark that done")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(completed.intent.0, "task.complete");
    assert_eq!(completed.arguments["reference"], json!("current"));
}
