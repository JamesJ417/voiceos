use std::sync::Arc;

use crate::{
    ChatMessage, ContextClaim, ContextSource, ConversationStore, Provider, ProviderCompletion,
    ProviderError, ProviderRequest, Role, StoreError, ToolDefinition, validate_context,
};

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub recent_message_limit: usize,
    pub summary_trigger_messages: usize,
    pub memory_limit: usize,
    pub system_prompt: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            recent_message_limit: 12,
            summary_trigger_messages: 18,
            memory_limit: 24,
            system_prompt: include_str!("../../../contracts/master-system-prompt.md")
                .trim()
                .to_owned(),
        }
    }
}

pub trait Summarizer: Send + Sync {
    fn summarize(&self, previous: Option<&str>, messages: &[ChatMessage]) -> String;
}

pub struct HeuristicSummarizer;

impl Summarizer for HeuristicSummarizer {
    fn summarize(&self, previous: Option<&str>, messages: &[ChatMessage]) -> String {
        let mut parts = Vec::new();
        if let Some(previous) = previous.filter(|value| !value.trim().is_empty()) {
            parts.push(format!("Earlier summary: {}", bounded(previous, 2_000)));
        }
        for message in messages {
            let role = match message.role {
                Role::User => "User",
                Role::Assistant => "VoiceOS",
                Role::Tool => "Tool",
                Role::System => "System",
            };
            parts.push(format!("{role}: {}", bounded(&message.content, 400)));
        }
        bounded(&parts.join("\n"), 6_000)
    }
}

pub trait MemoryExtractor: Send + Sync {
    fn extract(&self, user_text: &str) -> Vec<String>;
}

pub struct ExplicitMemoryExtractor;

impl MemoryExtractor for ExplicitMemoryExtractor {
    fn extract(&self, user_text: &str) -> Vec<String> {
        let trimmed = user_text.trim();
        let lower = trimmed.to_lowercase();
        for prefix in [
            "remember that ",
            "please remember that ",
            "please remember ",
        ] {
            if lower.starts_with(prefix) {
                let memory = trimmed[prefix.len()..]
                    .trim()
                    .trim_end_matches(['.', '!', '?']);
                if !memory.is_empty() {
                    return vec![memory.to_owned()];
                }
            }
        }
        let durable_prefixes = [
            "my name is ",
            "my preferred name is ",
            "my favorite ",
            "i prefer ",
            "i am allergic to ",
            "i'm allergic to ",
            "my timezone is ",
            "i live in ",
            "i work at ",
            "my email is ",
        ];
        if durable_prefixes
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        {
            let memory = trimmed.trim_end_matches(['.', '!', '?']).trim();
            if memory.len() >= 4 && memory.len() <= 500 {
                return vec![memory.to_owned()];
            }
        }
        Vec::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("context integrity rejected {count} claim(s)")]
    Integrity { count: usize },
}

pub struct OwnerTurnInput<'a> {
    pub owner_id: &'a str,
    pub device_id: &'a str,
    pub client_session_id: Option<&'a str>,
    pub user_text: &'a str,
    pub tools: Vec<ToolDefinition>,
    pub request_id: Option<&'a str>,
    pub attachment_ids: Vec<String>,
}

pub struct ConversationEngine {
    store: Arc<ConversationStore>,
    summarizer: Arc<dyn Summarizer>,
    memory_extractor: Arc<dyn MemoryExtractor>,
    config: EngineConfig,
}

impl ConversationEngine {
    pub fn new(store: Arc<ConversationStore>) -> Self {
        Self {
            store,
            summarizer: Arc::new(HeuristicSummarizer),
            memory_extractor: Arc::new(ExplicitMemoryExtractor),
            config: EngineConfig::default(),
        }
    }

    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    pub fn run_turn(
        &self,
        device_id: &str,
        client_session_id: Option<&str>,
        user_text: &str,
        tools: Vec<ToolDefinition>,
        provider: &dyn Provider,
    ) -> Result<(String, ProviderCompletion), EngineError> {
        self.run_owner_turn(
            device_id,
            device_id,
            client_session_id,
            user_text,
            tools,
            provider,
        )
    }

    pub fn run_owner_turn(
        &self,
        owner_id: &str,
        device_id: &str,
        client_session_id: Option<&str>,
        user_text: &str,
        tools: Vec<ToolDefinition>,
        provider: &dyn Provider,
    ) -> Result<(String, ProviderCompletion), EngineError> {
        self.run_owner_turn_idempotent(
            OwnerTurnInput {
                owner_id,
                device_id,
                client_session_id,
                user_text,
                tools,
                request_id: None,
                attachment_ids: vec![],
            },
            provider,
        )
    }

    pub fn run_owner_turn_idempotent(
        &self,
        input: OwnerTurnInput<'_>,
        provider: &dyn Provider,
    ) -> Result<(String, ProviderCompletion), EngineError> {
        let (conversation_id, context) = self.prepare_owner_turn_with_attachments(
            input.owner_id,
            input.device_id,
            input.client_session_id,
            input.user_text,
            input.request_id,
            &input.attachment_ids,
        )?;
        let messages = self.provider_messages(context)?;
        let completion = provider.complete(&ProviderRequest {
            conversation_id: conversation_id.clone(),
            messages,
            tools: input.tools,
        })?;
        self.record_assistant_from(
            &conversation_id,
            &completion.text,
            &completion.provider,
            input.device_id,
            input.request_id,
        )?;
        Ok((conversation_id, completion))
    }

    pub fn run_owner_turn_idempotent_with_attachments(
        &self,
        input: OwnerTurnInput<'_>,
        provider: &dyn Provider,
        attachment_ids: &[String],
    ) -> Result<(String, ProviderCompletion), EngineError> {
        let (conversation_id, context) = self.prepare_owner_turn_with_attachments(
            input.owner_id,
            input.device_id,
            input.client_session_id,
            input.user_text,
            input.request_id,
            attachment_ids,
        )?;
        let messages = self.provider_messages(context)?;
        let completion = provider.complete(&ProviderRequest {
            conversation_id: conversation_id.clone(),
            messages,
            tools: input.tools,
        })?;
        self.record_assistant_from(
            &conversation_id,
            &completion.text,
            &completion.provider,
            input.device_id,
            input.request_id,
        )?;
        Ok((conversation_id, completion))
    }

    pub fn prepare_turn(
        &self,
        device_id: &str,
        client_session_id: Option<&str>,
        user_text: &str,
    ) -> Result<(String, crate::ConversationContext), StoreError> {
        self.prepare_owner_turn(device_id, device_id, client_session_id, user_text, None)
    }

    pub fn prepare_owner_turn(
        &self,
        owner_id: &str,
        device_id: &str,
        client_session_id: Option<&str>,
        user_text: &str,
        request_id: Option<&str>,
    ) -> Result<(String, crate::ConversationContext), StoreError> {
        self.prepare_owner_turn_with_attachments(
            owner_id,
            device_id,
            client_session_id,
            user_text,
            request_id,
            &[],
        )
    }

    pub fn prepare_owner_turn_with_attachments(
        &self,
        owner_id: &str,
        device_id: &str,
        client_session_id: Option<&str>,
        user_text: &str,
        request_id: Option<&str>,
        attachment_ids: &[String],
    ) -> Result<(String, crate::ConversationContext), StoreError> {
        let conversation_id =
            self.store
                .resolve_owner_conversation(owner_id, device_id, client_session_id)?;
        for memory in self.memory_extractor.extract(user_text) {
            let lower = user_text.trim().to_lowercase();
            let source = if lower.starts_with("remember ") || lower.starts_with("please remember ")
            {
                "explicit-user-request"
            } else {
                "automatic-user-statement"
            };
            self.store.remember_for_owner_in_conversation(
                owner_id,
                device_id,
                &conversation_id,
                &memory,
                source,
            )?;
        }
        self.store.claim_attachments_for_owner_turn(
            owner_id,
            device_id,
            &conversation_id,
            user_text,
            request_id,
            attachment_ids,
        )?;
        self.roll_summary(owner_id, &conversation_id)?;
        let context = self.store.context_for_owner(
            owner_id,
            &conversation_id,
            user_text,
            self.config.recent_message_limit,
            self.config.memory_limit,
        )?;
        Ok((conversation_id, context))
    }

    pub fn prepare_owner_turn_with_attachments(
        &self,
        owner_id: &str,
        device_id: &str,
        client_session_id: Option<&str>,
        user_text: &str,
        request_id: Option<&str>,
        attachment_ids: &[String],
    ) -> Result<(String, crate::ConversationContext), StoreError> {
        for memory in self.memory_extractor.extract(user_text) {
            self.store
                .remember_for_owner(owner_id, device_id, &memory, "explicit-user-request")?;
        }
        let conversation_id = self.store.append_owner_user_message_with_attachments(
            owner_id,
            device_id,
            client_session_id,
            user_text,
            request_id,
            attachment_ids,
        )?;
        self.roll_summary(&conversation_id)?;
        let context = self.store.context_for_owner(
            owner_id,
            &conversation_id,
            user_text,
            self.config.recent_message_limit,
            self.config.memory_limit,
        )?;
        Ok((conversation_id, context))
    }

    pub fn record_assistant(
        &self,
        conversation_id: &str,
        response_text: &str,
        provider: &str,
    ) -> Result<(), StoreError> {
        self.store.append_message(
            conversation_id,
            Role::Assistant,
            response_text,
            Some(provider),
        )?;
        Ok(())
    }

    pub fn record_assistant_from(
        &self,
        conversation_id: &str,
        response_text: &str,
        provider: &str,
        origin_device_id: &str,
        request_id: Option<&str>,
    ) -> Result<(), StoreError> {
        let assistant_request_id = request_id.map(|value| format!("{value}:assistant"));
        self.store.append_message_from(
            conversation_id,
            Role::Assistant,
            response_text,
            Some(provider),
            Some(origin_device_id),
            assistant_request_id.as_deref(),
        )?;
        Ok(())
    }

    fn provider_messages(
        &self,
        context: crate::ConversationContext,
    ) -> Result<Vec<ChatMessage>, EngineError> {
        let mut claims = Vec::new();
        if let Some(summary) = context.summary.as_ref() {
            claims.push(ContextClaim::new(
                "conversation-summary",
                &context.conversation_id,
                ContextSource::ConversationSummary,
                summary,
            ));
        }
        claims.extend(context.memories.iter().map(|memory| {
            ContextClaim::new(
                memory.id.clone(),
                &context.conversation_id,
                ContextSource::ExplicitMemory,
                memory.content.clone(),
            )
        }));
        if let Some(document_context) = context.document_context.as_ref() {
            claims.push(ContextClaim::new(
                "document-context",
                &context.conversation_id,
                ContextSource::Document,
                document_context,
            ));
        }
        claims.extend(
            context
                .recent_messages
                .iter()
                .enumerate()
                .map(|(index, message)| {
                    ContextClaim::new(
                        format!("message-{index}"),
                        &context.conversation_id,
                        ContextSource::Conversation,
                        message.content.clone(),
                    )
                }),
        );
        let integrity = validate_context(&context.conversation_id, claims);
        if !integrity.quarantined.is_empty() {
            self.store
                .quarantine_claims(&context.conversation_id, &integrity.quarantined)?;
            return Err(EngineError::Integrity {
                count: integrity.quarantined.len(),
            });
        }

        let mut messages = vec![ChatMessage::new(Role::System, &self.config.system_prompt)];
        if !context.memories.is_empty() {
            messages.push(ChatMessage::new(
                Role::System,
                format!(
                    "Durable user memories:\n{}",
                    context
                        .memories
                        .iter()
                        .map(|memory| format!("- {}", memory.content))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            ));
        }
        if let Some(document_context) = context.document_context {
            messages.push(ChatMessage::new(
                Role::System,
                format!("Private uploaded document context. Use it only as reference and cite the source filename when it materially informs the answer:\n{document_context}"),
            ));
        }
        if let Some(summary) = context.summary {
            messages.push(ChatMessage::new(
                Role::System,
                format!("Rolling conversation summary:\n{summary}"),
            ));
        }
        messages.extend(context.recent_messages);
        Ok(messages)
    }

    fn roll_summary(&self, owner_id: &str, conversation_id: &str) -> Result<(), StoreError> {
        if self.store.message_count(conversation_id)? < self.config.summary_trigger_messages {
            return Ok(());
        }
        let Some(through_id) = self
            .store
            .message_id_before_recent(conversation_id, self.config.recent_message_limit)?
        else {
            return Ok(());
        };
        let existing = self.store.summary_for_owner(owner_id, conversation_id)?;
        if existing
            .as_ref()
            .is_some_and(|(_, existing_id)| *existing_id >= through_id)
        {
            return Ok(());
        }
        let after_id = existing.as_ref().map(|(_, id)| *id).unwrap_or(0);
        let messages = self
            .store
            .messages_through(conversation_id, through_id)?
            .into_iter()
            .filter(|(id, _)| *id > after_id)
            .map(|(_, message)| message)
            .collect::<Vec<_>>();
        if messages.is_empty() {
            return Ok(());
        }
        let summary = self
            .summarizer
            .summarize(existing.as_ref().map(|value| value.0.as_str()), &messages);
        self.store
            .save_summary_for_owner(owner_id, conversation_id, &summary, through_id)
    }
}

fn bounded(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let result = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}
