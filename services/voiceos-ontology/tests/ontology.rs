use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use voiceos_ontology::{
    AliasInput, CanonicalRequest, Catalog, Confidence, DecisionStatus, EntityKind, IntentId,
    Interpreter, ModelCandidate, ModelFallback, OntologyStore, ResolutionSource,
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
        ("show my files", "document.list"),
        ("how much disk space is free", "system.disk.check"),
        ("is ollama running", "system.service.check"),
        ("approve", "approval.decide"),
    ];
    for (phrase, intent) in cases {
        let decision = interpreter.interpret("owner", phrase).unwrap();
        assert_eq!(decision.interpretation.unwrap().intent.0, intent);
    }
}

#[test]
fn console_phrases_resolve_to_narrow_local_display_commands() {
    let interpreter = interpreter();
    let cases = [
        ("Show the weather", "console.show_weather"),
        (
            "Refresh the VIC Console dashboard",
            "console.refresh_dashboard",
        ),
    ];
    for (phrase, intent) in cases {
        let decision = interpreter.interpret("owner", phrase).unwrap();
        assert_eq!(decision.status, DecisionStatus::Resolved);
        assert_eq!(decision.interpretation.unwrap().intent.0, intent);
    }

    let question = interpreter
        .interpret("owner", "What is the weather tomorrow?")
        .unwrap();
    assert_eq!(question.status, DecisionStatus::Unrecognized);
}

#[test]
fn adhd_focus_phrases_resolve_without_claiming_task_completion() {
    let interpreter = interpreter();
    let cases = [
        ("What should I do now?", "focus.next"),
        ("I'm overwhelmed", "focus.next"),
        ("Start a five minute focus session", "focus.start"),
        ("I got interrupted", "focus.interrupt"),
        ("Help me restart", "focus.restart"),
        ("I'm done for now", "focus.complete"),
    ];
    for (phrase, intent) in cases {
        let decision = interpreter.interpret("owner", phrase).unwrap();
        assert_eq!(decision.status, DecisionStatus::Resolved, "{phrase}");
        assert_eq!(
            decision.interpretation.unwrap().intent.0,
            intent,
            "{phrase}"
        );
    }
    let start = interpreter
        .interpret("owner", "Start a five minute focus session")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(start.arguments["minutes"], json!(5));
    let overwhelmed = interpreter
        .interpret("owner", "I'm overwhelmed")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(overwhelmed.arguments["mode"], json!("low_energy"));

    let capture = interpreter
        .interpret("owner", "Park this idea build a mobile greenhouse")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(capture.intent.0, "focus.capture");
    assert_eq!(
        capture.arguments["title"],
        json!("build a mobile greenhouse")
    );

    let switch = interpreter
        .interpret("owner", "Work on the tax return instead")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(switch.intent.0, "focus.switch");
    assert_eq!(switch.arguments["reference"], json!("the tax return"));
}

#[test]
fn personal_support_phrases_resolve_to_narrow_canonical_requests() {
    let interpreter = interpreter();
    let cases = [
        ("Capture this buy milk", "personal.capture"),
        ("What should I do next?", "personal.next"),
        ("Help me get unstuck", "personal.unstuck"),
        ("I'm interrupted", "personal.interrupt"),
        ("Show my captures", "personal.inbox"),
        ("Review that", "personal.review"),
        ("Discard that", "personal.discard"),
    ];

    for (phrase, intent) in cases {
        let decision = interpreter.interpret("owner", phrase).unwrap();
        assert_eq!(decision.status, DecisionStatus::Resolved, "{phrase}");
        assert_eq!(
            decision.interpretation.unwrap().intent.0,
            intent,
            "{phrase}"
        );
    }
    let capture = interpreter
        .interpret("owner", "Capture this buy milk")
        .unwrap()
        .interpretation
        .unwrap();
    assert_eq!(
        capture.arguments,
        BTreeMap::from([("content".to_owned(), json!("buy milk"))])
    );
}

#[test]
fn personal_capture_requires_an_explicit_capture_command() {
    let interpreter = interpreter();
    for phrase in [
        "Buy milk after work",
        "I was thinking about picking up milk",
        "Can you tell me about captures?",
        "My next meeting is at noon",
    ] {
        let decision = interpreter.interpret("owner", phrase).unwrap();
        assert_eq!(decision.status, DecisionStatus::Unrecognized, "{phrase}");
    }
}

#[test]
fn model_fallback_cannot_turn_ordinary_conversation_into_personal_capture() {
    let candidate = ModelCandidate {
        intent: IntentId("personal.capture".to_owned()),
        entities: vec![],
        arguments: BTreeMap::from([("content".to_owned(), json!("buy milk"))]),
        confidence: 0.99,
    };
    let interpreter = interpreter().with_fallback(Arc::new(FixedFallback(candidate)));
    let decision = interpreter
        .interpret("owner", "I was thinking about milk")
        .unwrap();
    assert_eq!(decision.status, DecisionStatus::Unrecognized);
    assert!(decision.interpretation.is_none());
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
