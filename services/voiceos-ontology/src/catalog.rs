use std::collections::BTreeMap;

use crate::{
    ArgumentKind, ArgumentSpec, EntityDefinition, EntityKind, EntityRef, IntentDefinition,
    IntentId, Unit,
};

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
        let intents = [
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
                "document.add",
                "Add a private knowledge document.",
                vec![],
                false,
            ),
            intent(
                "document.list",
                "List private knowledge documents.",
                vec![],
                false,
            ),
            intent(
                "document.delete",
                "Delete a private knowledge document.",
                vec![string("document", true)],
                true,
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
        ]
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
        ];

        Self { intents, entities }
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
            .filter(|(_, alias)| phrase.contains(alias))
            .max_by_key(|(_, alias)| alias.len())
            .map(|(entity, surface)| EntityRef {
                kind: entity.kind.clone(),
                id: entity.id.clone(),
                surface: Some(surface.to_owned()),
            })
    }
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
    ArgumentSpec {
        name: name.to_owned(),
        kind: ArgumentKind::String,
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
