use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    Alias, CanonicalRequest, Catalog, Confidence, EntityKind, EntityRef, IntentId, ResolutionSource,
};

#[derive(Clone, Debug, Default)]
pub struct DeterministicResolver;

impl DeterministicResolver {
    pub fn resolve(
        &self,
        phrase: &str,
        catalog: &Catalog,
        aliases: &[Alias],
    ) -> Option<CanonicalRequest> {
        let normalized = normalize_phrase(phrase);
        if normalized.is_empty() {
            return None;
        }
        let learned_entities = resolve_learned_entities(&normalized, aliases);
        let source = if learned_entities.is_empty() {
            ResolutionSource::Deterministic
        } else {
            ResolutionSource::ApprovedAlias
        };

        if let Some(decision) = approval_decision(&normalized) {
            return Some(request(
                "approval.decide",
                [("decision", json!(decision))],
                vec![],
                0.99,
                source,
            ));
        }

        if has_any(&normalized, &["refresh", "update", "reload"])
            && has_any(&normalized, &["console", "dashboard", "weather"])
        {
            return Some(request(
                "console.refresh_dashboard",
                [],
                vec![],
                0.99,
                source,
            ));
        }
        if has_any(&normalized, &["show", "open", "display", "switch"])
            && normalized.contains("weather")
        {
            return Some(request("console.show_weather", [], vec![], 0.99, source));
        }

        if let Some(title) = strip_after(
            &normalized,
            &[
                "park this idea ",
                "park this ",
                "capture this without switching ",
                "capture this ",
                "put this in the parking lot ",
                "don t let me forget ",
            ],
        ) && !title.is_empty()
        {
            return Some(request(
                "focus.capture",
                [("title", json!(title))],
                vec![],
                0.99,
                source,
            ));
        }
        if let Some(reference) = strip_after(
            &normalized,
            &[
                "switch focus to ",
                "switch my focus to ",
                "change focus to ",
                "work on ",
            ],
        ) {
            let reference = reference
                .strip_suffix(" instead")
                .unwrap_or(reference)
                .trim();
            if !reference.is_empty()
                && (normalized.contains("switch") || normalized.ends_with(" instead"))
            {
                return Some(request(
                    "focus.switch",
                    [("reference", json!(reference))],
                    vec![],
                    0.99,
                    source,
                ));
            }
        }

        if has_any(
            &normalized,
            &[
                "i got interrupted",
                "i was interrupted",
                "save my restart point",
                "lost my focus",
            ],
        ) {
            return Some(request("focus.interrupt", [], vec![], 0.99, source));
        }
        if has_any(
            &normalized,
            &[
                "help me restart",
                "where was i",
                "resume focus",
                "restart my focus",
                "pick up where i left off",
            ],
        ) {
            return Some(request("focus.restart", [], vec![], 0.99, source));
        }
        if has_any(
            &normalized,
            &["done for now", "end focus session", "finish focus session"],
        ) {
            return Some(request("focus.complete", [], vec![], 0.99, source));
        }
        if let Some(minutes) = focus_minutes(&normalized) {
            return Some(request(
                "focus.start",
                [("minutes", json!(minutes))],
                vec![],
                0.99,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "i am overwhelmed",
                "i m overwhelmed",
                "low energy",
                "make it easier",
            ],
        ) {
            return Some(request(
                "focus.next",
                [("mode", json!("low_energy"))],
                vec![],
                0.99,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "five minute version",
                "5 minute version",
                "give me five minutes",
            ],
        ) {
            return Some(request(
                "focus.next",
                [("mode", json!("five_minute"))],
                vec![],
                0.99,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "what should i do now",
                "what is the next thing",
                "give me one thing",
                "help me focus",
                "next action",
            ],
        ) {
            return Some(request(
                "focus.next",
                [("mode", json!("normal"))],
                vec![],
                0.99,
                source,
            ));
        }

        if is_task_assistance_request(&normalized) {
            return Some(request("task.assist", [], vec![], 0.99, source));
        }

        if is_task_review_request(&normalized) {
            return Some(request("task.review", [], vec![], 0.99, source));
        }

        if is_task_list_request(&normalized) {
            return Some(request("task.list", [], vec![], 0.99, source));
        }
        if let Some((title, estimated_minutes)) = task_to_create(&normalized) {
            let mut arguments = BTreeMap::from([
                ("title".to_owned(), json!(title)),
                (
                    "observable_outcome".to_owned(),
                    json!(format!("Complete: {title}")),
                ),
            ]);
            if let Some(minutes) = estimated_minutes {
                arguments.insert("estimated_minutes".to_owned(), json!(minutes));
            }
            return Some(CanonicalRequest {
                intent: IntentId::from("task.create"),
                entities: vec![],
                arguments,
                confidence: Confidence::new(0.98),
                source,
            });
        }
        if let Some(reference) = task_reference(
            &normalized,
            &[
                "mark task ",
                "mark the task ",
                "complete task ",
                "complete the task ",
                "finish task ",
                "finish the task ",
            ],
            &[" done", " complete", " completed", " finished"],
        ) {
            return Some(request(
                "task.complete",
                [("reference", json!(reference))],
                vec![],
                0.97,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "mark that done",
                "mark it done",
                "complete that task",
                "complete the task",
                "finish that task",
                "finish the current task",
            ],
        ) {
            return Some(request(
                "task.complete",
                [("reference", json!("current"))],
                vec![],
                0.96,
                source,
            ));
        }
        if let Some(reference) = strip_after(
            &normalized,
            &[
                "start task ",
                "start the task ",
                "begin task ",
                "work on task ",
            ],
        ) && !reference.is_empty()
        {
            return Some(request(
                "task.start",
                [("reference", json!(reference))],
                vec![],
                0.97,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "start the next task",
                "start my next task",
                "begin the next task",
                "work on the next task",
                "start that task",
            ],
        ) {
            return Some(request(
                "task.start",
                [("reference", json!("next"))],
                vec![],
                0.98,
                source,
            ));
        }

        if let Some(rate) = playback_rate(&normalized) {
            return Some(request(
                "voice.playback_speed.set",
                [("rate", json!(rate))],
                vec![],
                0.98,
                source,
            ));
        }
        if has_any(
            &normalized,
            &["speak faster", "talk faster", "read faster", "speed up"],
        ) {
            return Some(request(
                "voice.playback_speed.adjust",
                [("direction", json!("increase"))],
                vec![],
                0.97,
                source,
            ));
        }
        if has_any(
            &normalized,
            &["speak slower", "talk slower", "read slower", "slow down"],
        ) {
            return Some(request(
                "voice.playback_speed.adjust",
                [("direction", json!("decrease"))],
                vec![],
                0.97,
                source,
            ));
        }

        if let Some(content) = strip_after(&normalized, &["remember that ", "remember "])
            && !content.is_empty()
        {
            return Some(request(
                "memory.remember",
                [("content", json!(content))],
                vec![],
                0.98,
                source,
            ));
        }
        if let Some(content) = strip_after(
            &normalized,
            &["forget that ", "forget the memory ", "forget "],
        ) && !content.is_empty()
        {
            return Some(request(
                "memory.forget",
                [("content", json!(content))],
                vec![],
                0.94,
                source,
            ));
        }
        if has_any(
            &normalized,
            &["list memories", "what do you remember", "show memories"],
        ) {
            return Some(request("memory.list", [], vec![], 0.96, source));
        }

        if has_any(
            &normalized,
            &[
                "add a file",
                "upload a file",
                "add document",
                "upload document",
            ],
        ) {
            return Some(request("document.add", [], vec![], 0.95, source));
        }
        if has_any(
            &normalized,
            &[
                "list files",
                "list documents",
                "show my files",
                "show documents",
            ],
        ) {
            return Some(request("document.list", [], vec![], 0.96, source));
        }
        if let Some(document) = strip_after(
            &normalized,
            &[
                "delete document ",
                "delete file ",
                "remove document ",
                "remove file ",
            ],
        ) && !document.is_empty()
        {
            return Some(request(
                "document.delete",
                [("document", json!(document))],
                vec![],
                0.92,
                source,
            ));
        }

        if has_any(
            &normalized,
            &["disk space", "free space", "storage space", "disk usage"],
        ) {
            return Some(request(
                "system.disk.check",
                [],
                learned_entities,
                0.98,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "network status",
                "check network",
                "is the network",
                "internet connection",
            ],
        ) {
            return Some(request(
                "system.network.check",
                [],
                learned_entities,
                0.95,
                source,
            ));
        }

        let service = resolve_entity(&normalized, EntityKind::Service, catalog, aliases);
        if let Some(entity) = service
            && has_any(
                &normalized,
                &["status", "running", "healthy", "check", "working"],
            )
        {
            return Some(request(
                "system.service.check",
                [("service", json!(entity.id))],
                vec![entity],
                0.97,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "system health",
                "health report",
                "gateway health",
                "is the system healthy",
            ],
        ) {
            return Some(request(
                "system.health.check",
                [],
                learned_entities,
                0.98,
                source,
            ));
        }
        if has_any(
            &normalized,
            &[
                "run tests",
                "run the tests",
                "test the project",
                "gateway tests",
            ],
        ) {
            return Some(request(
                "project.tests.run",
                [("suite", json!("gateway"))],
                vec![],
                0.96,
                source,
            ));
        }

        let provider = resolve_entity(&normalized, EntityKind::Provider, catalog, aliases);
        if let Some(entity) = provider.filter(|_| {
            has_any(
                &normalized,
                &["use ", "ask ", "switch to", "route to", "with the"],
            )
        }) {
            return Some(request(
                "provider.select",
                [("provider", json!(entity.id))],
                vec![entity],
                0.98,
                source,
            ));
        }
        None
    }
}

pub fn normalize_phrase(phrase: &str) -> String {
    let mut normalized = String::with_capacity(phrase.len());
    let mut previous_space = true;
    for character in phrase.to_lowercase().chars() {
        if character.is_alphanumeric() || character == '.' || character == '-' {
            normalized.push(character);
            previous_space = false;
        } else if !previous_space {
            normalized.push(' ');
            previous_space = true;
        }
    }
    normalized.trim().to_owned()
}

fn request<const N: usize>(
    intent: &str,
    arguments: [(&str, Value); N],
    entities: Vec<EntityRef>,
    confidence: f32,
    source: ResolutionSource,
) -> CanonicalRequest {
    CanonicalRequest {
        intent: IntentId::from(intent),
        entities,
        arguments: arguments
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<BTreeMap<_, _>>(),
        confidence: Confidence::new(confidence),
        source,
    }
}

fn resolve_entity(
    phrase: &str,
    kind: EntityKind,
    catalog: &Catalog,
    aliases: &[Alias],
) -> Option<EntityRef> {
    aliases
        .iter()
        .filter(|alias| alias.entity.kind == kind && phrase.contains(&alias.phrase))
        .max_by_key(|alias| alias.phrase.len())
        .map(|alias| EntityRef {
            surface: Some(alias.phrase.clone()),
            ..alias.entity.clone()
        })
        .or_else(|| catalog.resolve_builtin_entity(phrase, &kind))
}

fn resolve_learned_entities(phrase: &str, aliases: &[Alias]) -> Vec<EntityRef> {
    aliases
        .iter()
        .filter(|alias| phrase.contains(&alias.phrase))
        .map(|alias| EntityRef {
            surface: Some(alias.phrase.clone()),
            ..alias.entity.clone()
        })
        .collect()
}

fn approval_decision(phrase: &str) -> Option<&'static str> {
    match phrase {
        "approve" | "approved" | "yes approve" | "confirm" | "yes confirm" => Some("approve"),
        "deny" | "denied" | "no deny" | "reject" | "cancel" => Some("deny"),
        _ => None,
    }
}

fn playback_rate(phrase: &str) -> Option<f64> {
    if has_any(
        phrase,
        &["double speed", "two times", "2x", "2 x", "speed to 2"],
    ) {
        Some(2.0)
    } else if has_any(phrase, &["one point seven five", "1.75"]) {
        Some(1.75)
    } else if has_any(phrase, &["one point five", "one and a half", "1.5"]) {
        Some(1.5)
    } else if has_any(phrase, &["one point two five", "1.25"]) {
        Some(1.25)
    } else if has_any(
        phrase,
        &[
            "normal speed",
            "regular speed",
            "reset speech speed",
            "speak normally",
        ],
    ) {
        Some(1.0)
    } else {
        None
    }
}

fn focus_minutes(phrase: &str) -> Option<u32> {
    let focus_language = has_any(phrase, &["focus", "session", "timer"]);
    if !focus_language || !has_any(phrase, &["start", "begin", "give me", "focus for"]) {
        return None;
    }
    if has_any(phrase, &["five minute", "5 minute", "5-minute"]) {
        Some(5)
    } else if has_any(phrase, &["ten minute", "10 minute", "10-minute"]) {
        Some(10)
    } else if has_any(phrase, &["twenty minute", "20 minute", "20-minute"]) {
        Some(20)
    } else {
        None
    }
}

fn has_any(phrase: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| phrase.contains(candidate))
}

fn is_task_assistance_request(phrase: &str) -> bool {
    if task_to_create(phrase).is_some() {
        return false;
    }
    let mentions_tasks = has_any(phrase, &["task", "to do", "todo"]);
    let asks_for_help = has_any(
        phrase,
        &[
            "how can you help",
            "how could you help",
            "what can you help",
            "what could you help",
            "help me with my task",
            "help me with the task",
            "help with my task",
            "help with the task",
        ],
    );
    mentions_tasks && asks_for_help
}

fn is_task_review_request(phrase: &str) -> bool {
    if task_to_create(phrase).is_some() {
        return false;
    }
    if has_any(
        phrase,
        &[
            "review my tasks",
            "review the tasks",
            "review my task list",
            "review the task list",
            "look at my tasks",
            "look at the tasks",
            "look at my task list",
            "look at the task list",
            "what should i work on",
            "what should we work on",
            "what do i need to work on",
            "what do we need to work on",
            "what needs to get done",
            "tell me my priorities",
            "tell me what to work on",
            "tell me what we need to work on",
            "prioritize my tasks",
            "prioritize the task list",
        ],
    ) {
        return true;
    }

    let mentions_tasks = has_any(phrase, &["task", "to do", "todo"]);
    let asks_for_direction = has_any(
        phrase,
        &[
            "need to work on",
            "needs to work on",
            "need work on",
            "work on next",
            "should work on",
            "focus on",
            "priorities",
            "priority",
            "recommend",
            "needs to be done",
            "need to be done",
            "need to get done",
        ],
    );
    mentions_tasks && asks_for_direction
}

fn is_task_list_request(phrase: &str) -> bool {
    if task_to_create(phrase).is_some() {
        return false;
    }
    if has_any(
        phrase,
        &[
            "what are my tasks",
            "what tasks do i have",
            "list my tasks",
            "list tasks",
            "show my tasks",
            "read my tasks",
            "what is next on my list",
            "what s next on my list",
        ],
    ) {
        return true;
    }

    let mentions_tasks = has_any(phrase, &["task", "to do", "todo"]);
    let asks_for_list = has_any(
        phrase,
        &[
            "list of",
            "give me a list",
            "show me",
            "read me",
            "tell me the",
            "which tasks",
            "what tasks",
        ],
    );
    mentions_tasks && asks_for_list
}

fn strip_after<'a>(phrase: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|prefix| phrase.strip_prefix(prefix).map(str::trim))
}

fn task_to_create(phrase: &str) -> Option<(&str, Option<u32>)> {
    let mut title = strip_after(
        phrase,
        &[
            "create a task to ",
            "create task to ",
            "add a task to ",
            "add task to ",
            "make a task to ",
            "remind me that we need to ",
            "remind me that i need to ",
            "remind me that i should ",
            "remind me we need to ",
            "remind me i need to ",
            "remind me i should ",
            "remind me to ",
            "remind me about ",
            "i need to ",
        ],
    )?;
    title = title.trim_end_matches(['.', '!', '?']);
    for suffix in [
        " as a task",
        " as a todo",
        " as a to do",
        " on my task list",
        " to my task list",
    ] {
        if let Some(candidate) = title.strip_suffix(suffix) {
            title = candidate.trim();
            break;
        }
    }
    if title.is_empty() {
        return None;
    }
    let mut minutes = None;
    for marker in [" for ", " in "] {
        if let Some((candidate, suffix)) = title.rsplit_once(marker)
            && let Some(value) = parse_minute_suffix(suffix)
        {
            title = candidate.trim();
            minutes = Some(value);
            break;
        }
    }
    (!title.is_empty()).then_some((title, minutes))
}

fn parse_minute_suffix(value: &str) -> Option<u32> {
    let number = value
        .strip_suffix(" minutes")
        .or_else(|| value.strip_suffix(" minute"))?
        .trim()
        .parse::<u32>()
        .ok()?;
    (1..=1_440).contains(&number).then_some(number)
}

fn task_reference<'a>(phrase: &'a str, prefixes: &[&str], suffixes: &[&str]) -> Option<&'a str> {
    let body = strip_after(phrase, prefixes)?;
    let reference = suffixes
        .iter()
        .find_map(|suffix| body.strip_suffix(suffix).map(str::trim))
        .unwrap_or(body)
        .trim();
    (!reference.is_empty()).then_some(reference)
}
