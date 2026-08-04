use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    ArgumentKind, ArgumentSpec, CanonicalRequest, Confidence, EntityDefinition, EntityKind,
    EntityRef, IntentDefinition, IntentId, ResolutionSource, Unit, contains_phrase,
};

pub const CATALOG_VERSION: u32 = 2;
pub const MINIMUM_COMPATIBLE_VERSION: u32 = 1;

const ENTITY_KINDS: [EntityKind; 13] = [
    EntityKind::Device,
    EntityKind::Provider,
    EntityKind::Service,
    EntityKind::Document,
    EntityKind::Artifact,
    EntityKind::Task,
    EntityKind::Person,
    EntityKind::Project,
    EntityKind::Skill,
    EntityKind::Email,
    EntityKind::Location,
    EntityKind::Memory,
    EntityKind::Decision,
];

#[derive(Clone, Debug)]
pub struct Catalog {
    intents: BTreeMap<IntentId, IntentDefinition>,
    entities: Vec<EntityDefinition>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self::seeded()
    }
}

impl Catalog {
    pub fn seeded() -> Self {
        let intents = vec![
            intent(
                "voice.playback_speed.set",
                "Set spoken reply playback speed.",
                vec![number("rate", true, Some(Unit::Multiplier), 1.0, 2.0)],
                false,
            ),
            intent(
                "voice.playback_speed.adjust",
                "Increase or decrease spoken reply playback speed.",
                vec![enumeration("direction", true, &["increase", "decrease"])],
                false,
            ),
            intent(
                "provider.select",
                "Select a reasoning provider for a turn.",
                vec![entity("provider", EntityKind::Provider, true)],
                false,
            ),
            intent(
                "memory.remember",
                "Store an explicit durable memory.",
                vec![string("content", true)],
                false,
            ),
            intent(
                "memory.forget",
                "Remove a durable memory.",
                vec![string("content", true)],
                true,
            ),
            intent("memory.list", "List durable memories.", vec![], false),
            intent(
                "knowledge.document.add",
                "Add a private knowledge document used as context.",
                vec![],
                false,
            ),
            intent(
                "knowledge.document.list",
                "List private knowledge documents.",
                vec![],
                false,
            ),
            intent(
                "knowledge.document.delete",
                "Delete a private knowledge document.",
                vec![string("document", true)],
                true,
            ),
            intent(
                "artifact.pdf.create",
                "Create a VIC-managed PDF.",
                pdf_arguments(false),
                false,
            ),
            intent(
                "artifact.pdf.revise",
                "Create an immutable revision of a VIC-managed PDF.",
                pdf_arguments(true),
                false,
            ),
            intent(
                "artifact.search",
                "Search VIC-created files.",
                vec![string("query", false)],
                false,
            ),
            intent(
                "artifact.attach",
                "Attach a ready managed artifact to a task.",
                vec![
                    string("artifact_id", true),
                    string("task_id", true),
                    string("description", true),
                ],
                false,
            ),
            intent("system.health.check", "Check system health.", vec![], false),
            intent("system.disk.check", "Check free disk space.", vec![], false),
            intent(
                "system.network.check",
                "Check network status.",
                vec![],
                false,
            ),
            intent(
                "system.service.check",
                "Check an allowlisted service.",
                vec![entity("service", EntityKind::Service, true)],
                false,
            ),
            intent(
                "system.admin.execute",
                "Execute an exact capability-brokered administrative command.",
                vec![
                    array("argv", true),
                    string("cwd", true),
                    number("timeout_seconds", false, Some(Unit::Seconds), 1.0, 300.0),
                    string("rollback", true),
                ],
                true,
            ),
            intent(
                "device.revoke",
                "Revoke one enrolled device credential after explicit approval.",
                vec![
                    string("device_id", true),
                    string("requesting_device_id", true),
                ],
                true,
            ),
            intent(
                "project.tests.run",
                "Run the fixed VoiceOS gateway test suite.",
                vec![enumeration("suite", false, &["gateway"])],
                true,
            ),
            intent(
                "approval.decide",
                "Approve or deny the currently pending action.",
                vec![enumeration("decision", true, &["approve", "deny"])],
                false,
            ),
            intent(
                "task.create",
                "Create a task on the shared VoiceOS task board.",
                vec![
                    string("title", true),
                    string("observable_outcome", false),
                    number(
                        "estimated_minutes",
                        false,
                        Some(Unit::Minutes),
                        1.0,
                        1_440.0,
                    ),
                ],
                false,
            ),
            intent(
                "task.list",
                "List open tasks on the shared task board.",
                vec![],
                false,
            ),
            intent(
                "task.review",
                "Review open tasks and recommend what to work on next.",
                vec![],
                false,
            ),
            intent(
                "task.assist",
                "Explain concrete ways VoiceOS can help with the shared task board.",
                vec![],
                false,
            ),
            intent(
                "task.start",
                "Start a task on the shared task board.",
                vec![string("reference", false)],
                false,
            ),
            intent(
                "task.complete",
                "Complete a task on the shared task board.",
                vec![string("reference", false)],
                false,
            ),
            intent(
                "task.step.create",
                "Create and assign a task step.",
                vec![
                    string("task_id", true),
                    string("title", true),
                    enumeration("owner", true, &["user", "vic", "shared"]),
                ],
                false,
            ),
            intent(
                "task.step.update",
                "Update a task step with evidence.",
                vec![
                    string("task_id", true),
                    string("step_id", true),
                    enumeration(
                        "status",
                        true,
                        &["pending", "active", "blocked", "completed", "cancelled"],
                    ),
                    enumeration("owner", false, &["user", "vic", "shared"]),
                    object("evidence", false),
                ],
                false,
            ),
            intent(
                "task.blocker.create",
                "Record a task blocker.",
                vec![
                    string("task_id", true),
                    string("description", true),
                    enumeration("owner", true, &["user", "vic", "shared"]),
                ],
                false,
            ),
            intent(
                "task.blocker.resolve",
                "Resolve a task blocker.",
                vec![string("task_id", true), string("blocker_id", true)],
                false,
            ),
            intent(
                "task.handoff.create",
                "Hand responsibility between VIC and the user.",
                vec![
                    string("task_id", true),
                    enumeration("from_owner", true, &["user", "vic"]),
                    enumeration("to_owner", true, &["user", "vic"]),
                    enumeration("kind", true, &["handoff", "review", "approval"]),
                    string("summary", true),
                ],
                false,
            ),
            intent(
                "task.progress.record",
                "Record evidence-backed progress on a task.",
                vec![
                    string("task_id", true),
                    string("summary", true),
                    object("evidence", false),
                ],
                false,
            ),
            intent(
                "task.review.request",
                "Request review of VIC's task work.",
                vec![string("task_id", true), string("summary", true)],
                false,
            ),
            intent(
                "outreach.create",
                "Queue a policy-governed VIC outreach event.",
                vec![
                    enumeration(
                        "kind",
                        true,
                        &[
                            "status_update",
                            "check_in",
                            "question",
                            "blocker",
                            "review",
                            "digest",
                        ],
                    ),
                    enumeration("priority", true, &["quiet", "check_in", "needs_you"]),
                    string("title", true),
                    string("body", true),
                    string("reason", true),
                    string("task_id", false),
                    string("dedupe_key", false),
                ],
                false,
            ),
        ];
        let intents = intents
            .into_iter()
            .map(|definition| (definition.id.clone(), definition))
            .collect();

        let entities = vec![
            entity_definition(
                EntityKind::Device,
                "gpu-rig",
                &[
                    "gpu rig",
                    "mining rig",
                    "model rig",
                    "rtx rig",
                    "inference rig",
                ],
            ),
            entity_definition(
                EntityKind::Device,
                "hp-wall-terminal",
                &[
                    "hp",
                    "hp elite desk",
                    "wall computer",
                    "wall terminal",
                    "touchscreen",
                ],
            ),
            entity_definition(
                EntityKind::Device,
                "pixel",
                &["pixel", "phone", "android phone", "my phone"],
            ),
            entity_definition(
                EntityKind::Provider,
                "gemma",
                &["gemma", "fast model", "local model", "normal model"],
            ),
            entity_definition(
                EntityKind::Provider,
                "gpt-oss",
                &["gpt oss", "gpt-oss", "deep model", "reasoning model"],
            ),
            entity_definition(
                EntityKind::Provider,
                "codex-sol",
                &["codex", "sol", "codex sol", "highest confidence"],
            ),
            entity_definition(
                EntityKind::Service,
                "tailscale",
                &["tailscale", "tailscaled"],
            ),
            entity_definition(
                EntityKind::Service,
                "voiceos",
                &["voice os", "voiceos", "gateway", "voice os gateway"],
            ),
            entity_definition(EntityKind::Service, "ollama", &["ollama", "model server"]),
            entity_definition(
                EntityKind::Artifact,
                "vic-created-file",
                &["vic file", "created file", "generated file"],
            ),
            entity_definition(EntityKind::Task, "shared-task", &["task", "to do", "todo"]),
            entity_definition(
                EntityKind::Person,
                "primary-owner",
                &["me", "myself", "owner"],
            ),
            entity_definition(
                EntityKind::Project,
                "voiceos",
                &["voice os project", "voiceos project"],
            ),
            entity_definition(
                EntityKind::Skill,
                "hermes-skill",
                &["hermes skill", "vic skill"],
            ),
            entity_definition(
                EntityKind::Email,
                "inbox-message",
                &["email", "message", "inbox item"],
            ),
            entity_definition(EntityKind::Location, "home", &["home", "house"]),
            entity_definition(
                EntityKind::Location,
                "business",
                &["business", "office", "work"],
            ),
        ];
        Self { intents, entities }
    }

    pub fn version(&self) -> u32 {
        CATALOG_VERSION
    }
    pub fn minimum_compatible_version(&self) -> u32 {
        MINIMUM_COMPATIBLE_VERSION
    }
    pub fn supports_version(&self, version: u32) -> bool {
        (MINIMUM_COMPATIBLE_VERSION..=CATALOG_VERSION).contains(&version)
    }
    pub fn entity_kinds(&self) -> &'static [EntityKind] {
        &ENTITY_KINDS
    }
    pub fn intent(&self, id: &IntentId) -> Option<&IntentDefinition> {
        self.intents.get(id)
    }
    pub fn intents(&self) -> impl Iterator<Item = &IntentDefinition> {
        self.intents.values()
    }
    pub fn entities(&self) -> impl Iterator<Item = &EntityDefinition> {
        self.entities.iter()
    }
    pub fn entity(&self, kind: &EntityKind, id: &str) -> Option<&EntityDefinition> {
        self.entities
            .iter()
            .find(|entity| &entity.kind == kind && entity.id == id)
    }

    pub fn canonical_intent(&self, id: &IntentId) -> IntentId {
        match id.0.as_str() {
            "document.add" => IntentId::from("knowledge.document.add"),
            "document.list" => IntentId::from("knowledge.document.list"),
            "document.delete" => IntentId::from("knowledge.document.delete"),
            "artifact.find" => IntentId::from("artifact.search"),
            _ => id.clone(),
        }
    }

    pub fn migrate_request(&self, request: &mut CanonicalRequest) {
        request.intent = self.canonical_intent(&request.intent);
    }

    pub fn request_for_tool(
        &self,
        tool: &str,
        arguments: BTreeMap<String, Value>,
        confidence: f32,
        source: ResolutionSource,
    ) -> Option<CanonicalRequest> {
        let intent = match tool {
            "system.health" => "system.health.check",
            "disk.space" => "system.disk.check",
            "network.status" => "system.network.check",
            "service.status" => "system.service.check",
            "project.tests" => "project.tests.run",
            "rig.root_command" => "system.admin.execute",
            "device.revoke" => "device.revoke",
            "artifact.pdf.create" => "artifact.pdf.create",
            "artifact.pdf.revise" => "artifact.pdf.revise",
            "artifact.find" => "artifact.search",
            "artifact.attach" => "artifact.attach",
            "task.step.create" => "task.step.create",
            "task.step.update" => "task.step.update",
            "task.blocker.create" => "task.blocker.create",
            "task.blocker.resolve" => "task.blocker.resolve",
            "task.handoff.create" => "task.handoff.create",
            "task.progress.record" => "task.progress.record",
            "task.review.request" => "task.review.request",
            "outreach.create" => "outreach.create",
            _ => return None,
        };
        Some(CanonicalRequest {
            intent: IntentId::from(intent),
            entities: Vec::new(),
            arguments,
            confidence: Confidence::new(confidence),
            source,
        })
    }

    pub fn resolve_builtin_entity(&self, phrase: &str, kind: &EntityKind) -> Option<EntityRef> {
        self.entities
            .iter()
            .filter(|entity| &entity.kind == kind)
            .flat_map(|entity| {
                entity
                    .aliases
                    .iter()
                    .map(move |alias| (entity, alias.as_str()))
            })
            .filter(|(_, alias)| contains_phrase(phrase, alias))
            .max_by_key(|(_, alias)| alias.len())
            .map(|(entity, surface)| EntityRef {
                kind: entity.kind,
                id: entity.id.clone(),
                surface: Some(surface.to_owned()),
            })
    }
}

fn pdf_arguments(revision: bool) -> Vec<ArgumentSpec> {
    let mut values = Vec::new();
    if revision {
        values.push(string("artifact_id", true));
    }
    values.extend([
        string("title", true),
        string("description", false),
        string("filename", false),
        string("task_id", false),
        enumeration("template", false, &["recipe-card"]),
        object("spec", false),
    ]);
    values
}

fn intent(
    id: &str,
    description: &str,
    arguments: Vec<ArgumentSpec>,
    approval_required: bool,
) -> IntentDefinition {
    IntentDefinition {
        id: id.into(),
        description: description.to_owned(),
        arguments,
        approval_required,
    }
}
fn entity_definition(kind: EntityKind, id: &str, aliases: &[&str]) -> EntityDefinition {
    EntityDefinition {
        kind,
        id: id.to_owned(),
        aliases: aliases.iter().map(|value| (*value).to_owned()).collect(),
    }
}
fn string(name: &str, required: bool) -> ArgumentSpec {
    argument(name, ArgumentKind::String, required)
}
fn object(name: &str, required: bool) -> ArgumentSpec {
    argument(name, ArgumentKind::Object, required)
}
fn array(name: &str, required: bool) -> ArgumentSpec {
    argument(name, ArgumentKind::Array, required)
}
fn argument(name: &str, kind: ArgumentKind, required: bool) -> ArgumentSpec {
    ArgumentSpec {
        name: name.to_owned(),
        kind,
        required,
        unit: None,
        minimum: None,
        maximum: None,
        allowed_values: vec![],
    }
}
fn number(
    name: &str,
    required: bool,
    unit: Option<Unit>,
    minimum: f64,
    maximum: f64,
) -> ArgumentSpec {
    ArgumentSpec {
        name: name.to_owned(),
        kind: ArgumentKind::Number,
        required,
        unit,
        minimum: Some(minimum),
        maximum: Some(maximum),
        allowed_values: vec![],
    }
}
fn enumeration(name: &str, required: bool, allowed_values: &[&str]) -> ArgumentSpec {
    ArgumentSpec {
        name: name.to_owned(),
        kind: ArgumentKind::String,
        required,
        unit: None,
        minimum: None,
        maximum: None,
        allowed_values: allowed_values
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
    }
}
fn entity(name: &str, kind: EntityKind, required: bool) -> ArgumentSpec {
    ArgumentSpec {
        name: name.to_owned(),
        kind: ArgumentKind::Entity(kind),
        required,
        unit: None,
        minimum: None,
        maximum: None,
        allowed_values: vec![],
    }
}
