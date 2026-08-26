use std::sync::Arc;

use voiceos_core::{ChatMessage, ProviderRequest, ProviderRouter, Role};
use voiceos_ontology::{Catalog, ModelCandidate, ModelFallback};

pub(crate) struct GatewayModelFallback {
    router: Arc<ProviderRouter>,
    provider: String,
}

impl GatewayModelFallback {
    pub(crate) fn new(router: Arc<ProviderRouter>, provider: impl Into<String>) -> Self {
        Self {
            router,
            provider: provider.into(),
        }
    }
}

impl ModelFallback for GatewayModelFallback {
    fn resolve(&self, phrase: &str, catalog: &Catalog) -> Result<Option<ModelCandidate>, String> {
        let provider = self
            .router
            .select(phrase, Some(&self.provider))
            .map_err(|error| error.to_string())?;
        let ontology = serde_json::json!({
            "intents": catalog.intents().collect::<Vec<_>>(),
            "entities": catalog.entities().collect::<Vec<_>>(),
        });
        let prompt = format!(
            r#"Classify the user's meaning using the supplied VoiceOS ontology.
Return only one JSON object in this exact shape:
{{"intent":"task.review","entities":[],"arguments":{{}},"confidence":0.95}}
The entities value must always be an array. The arguments value must always be an object. Confidence must be a number from 0 to 1.
Return null only when none of the supported intents describes the user's meaning. Never invent an intent, entity, argument, or value.

Meaning examples:
- "How are you able to help me with these tasks?" -> {{"intent":"task.assist","entities":[],"arguments":{{}},"confidence":0.97}}
- "What work should we focus on?" -> {{"intent":"task.review","entities":[],"arguments":{{}},"confidence":0.94}}
- "Read everything on my to-do board" -> {{"intent":"task.list","entities":[],"arguments":{{}},"confidence":0.94}}
- "Please remember that I need to call the dentist" -> {{"intent":"task.create","entities":[],"arguments":{{"title":"call the dentist","observable_outcome":"Complete: call the dentist"}},"confidence":0.93}}
- "Tell me about black holes" -> null

Ontology: {ontology}"#
        );
        let completion = provider
            .complete(&ProviderRequest {
                conversation_id: "ontology-fallback".to_owned(),
                messages: vec![
                    ChatMessage::new(Role::System, prompt),
                    ChatMessage::new(Role::User, phrase),
                ],
                tools: vec![],
                image_attachments: vec![],
            })
            .map_err(|error| error.to_string())?;
        let content = completion.text.trim();
        if content.eq_ignore_ascii_case("null") {
            return Ok(None);
        }
        let json = extract_json_object(content)
            .ok_or_else(|| "model fallback did not return one JSON object".to_owned())?;
        parse_model_candidate(json).map(Some)
    }
}

fn parse_model_candidate(json: &str) -> Result<ModelCandidate, String> {
    let mut value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("model fallback returned invalid JSON: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "model fallback response must be a JSON object".to_owned())?;

    if object
        .get("arguments")
        .is_some_and(|arguments| arguments.as_array().is_some_and(Vec::is_empty))
    {
        object.insert("arguments".to_owned(), serde_json::json!({}));
    }
    if object
        .get("entities")
        .is_some_and(|entities| entities.as_object().is_some_and(serde_json::Map::is_empty))
    {
        object.insert("entities".to_owned(), serde_json::json!([]));
    }
    object
        .entry("arguments".to_owned())
        .or_insert_with(|| serde_json::json!({}));
    object
        .entry("entities".to_owned())
        .or_insert_with(|| serde_json::json!([]));

    serde_json::from_value(value)
        .map_err(|error| format!("model fallback returned invalid structured data: {error}"))
}

fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    (start <= end).then_some(&content[start..=end])
}

#[cfg(test)]
mod tests {
    use super::{extract_json_object, parse_model_candidate};

    #[test]
    fn extracts_a_single_json_object_from_fenced_output() {
        assert_eq!(
            extract_json_object("```json\n{\"intent\":\"memory.list\"}\n```"),
            Some("{\"intent\":\"memory.list\"}")
        );
        assert_eq!(extract_json_object("null"), None);
    }

    #[test]
    fn normalizes_empty_model_collections_before_schema_validation() {
        let candidate = parse_model_candidate(
            r#"{"intent":"task.assist","entities":{},"arguments":[],"confidence":0.96}"#,
        )
        .unwrap();
        assert_eq!(candidate.intent.0, "task.assist");
        assert!(candidate.entities.is_empty());
        assert!(candidate.arguments.is_empty());
    }
}
