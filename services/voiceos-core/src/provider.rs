use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ChatMessage, ProviderCompletion, ProviderRequest, Role, ToolCall, Usage};

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider is unavailable: {0}")]
    Unavailable(String),
    #[error("provider returned an invalid response: {0}")]
    InvalidResponse(String),
}

pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn complete(&self, request: &ProviderRequest) -> Result<ProviderCompletion, ProviderError>;
}

pub struct MockProvider;

impl Provider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }
    fn complete(&self, request: &ProviderRequest) -> Result<ProviderCompletion, ProviderError> {
        let text = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.content.as_str())
            .unwrap_or("");
        Ok(ProviderCompletion {
            text: format!("I heard: {text}."),
            provider: self.name().to_owned(),
            tool_calls: vec![],
            usage: Usage::default(),
        })
    }
}

pub struct OllamaProvider {
    name: String,
    base_url: String,
    model: String,
    think: bool,
    keep_alive: serde_json::Value,
    client: reqwest::blocking::Client,
}

impl OllamaProvider {
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
        think: bool,
    ) -> Result<Self, ProviderError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(if think { 300 } else { 120 }))
            .build()
            .map_err(|error| ProviderError::Unavailable(error.to_string()))?;
        Ok(Self {
            name: name.into(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            model: model.into(),
            think,
            keep_alive: if think {
                serde_json::json!("5m")
            } else {
                serde_json::json!(-1)
            },
            client,
        })
    }
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    think: bool,
    keep_alive: &'a serde_json::Value,
    options: serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    prompt_eval_count: Option<u64>,
    eval_count: Option<u64>,
}

#[derive(Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Deserialize)]
struct OllamaToolCall {
    function: OllamaFunction,
}

#[derive(Deserialize)]
struct OllamaFunction {
    name: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn complete(&self, request: &ProviderRequest) -> Result<ProviderCompletion, ProviderError> {
        if self.model.trim().is_empty() {
            return Err(ProviderError::Unavailable(format!(
                "{} model is not configured",
                self.name
            )));
        }
        let tools = request.tools.iter().map(|tool| serde_json::json!({"type":"function","function":{"name":tool.name,"description":tool.description,"parameters":tool.parameters}})).collect();
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&OllamaRequest {
                model: &self.model,
                messages: &request.messages,
                stream: false,
                think: self.think,
                keep_alive: &self.keep_alive,
                options: serde_json::json!({"temperature": if self.think { 0.2 } else { 0.0 }}),
                tools,
            })
            .send()
            .map_err(|error| ProviderError::Unavailable(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ProviderError::Unavailable(format!(
                "Ollama returned HTTP {}",
                response.status()
            )));
        }
        let body: OllamaResponse = response
            .json()
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;
        let tool_calls: Vec<ToolCall> = body
            .message
            .tool_calls
            .into_iter()
            .map(|call| ToolCall {
                name: call.function.name,
                arguments: call.function.arguments,
            })
            .collect();
        if body.message.content.trim().is_empty() && tool_calls.is_empty() {
            return Err(ProviderError::InvalidResponse(
                "empty assistant message".to_owned(),
            ));
        }
        Ok(ProviderCompletion {
            text: body.message.content.trim().to_owned(),
            provider: self.name.clone(),
            tool_calls,
            usage: Usage {
                input_tokens: body.prompt_eval_count,
                output_tokens: body.eval_count,
                cost_usd: Some(0.0),
            },
        })
    }
}

pub struct CodexBridgeProvider {
    socket_path: String,
}

impl CodexBridgeProvider {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }
}

impl Provider for CodexBridgeProvider {
    fn name(&self) -> &str {
        "codex-sol"
    }
    fn complete(&self, request: &ProviderRequest) -> Result<ProviderCompletion, ProviderError> {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;
            let mut stream = UnixStream::connect(&self.socket_path)
                .map_err(|error| ProviderError::Unavailable(error.to_string()))?;
            let prompt = request
                .messages
                .iter()
                .map(render_message)
                .collect::<Vec<_>>()
                .join("\n\n");
            let payload = serde_json::json!({"text": prompt}).to_string();
            if payload.len() > 65_535 {
                return Err(ProviderError::Unavailable(
                    "Codex bridge request exceeds 64 KiB".to_owned(),
                ));
            }
            stream
                .write_all(payload.as_bytes())
                .and_then(|_| stream.write_all(b"\n"))
                .map_err(|error| ProviderError::Unavailable(error.to_string()))?;
            let mut response = String::new();
            BufReader::new(stream)
                .read_line(&mut response)
                .map_err(|error| ProviderError::Unavailable(error.to_string()))?;
            let value: serde_json::Value = serde_json::from_str(&response)
                .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;
            if value.get("ok").and_then(|value| value.as_bool()) != Some(true) {
                return Err(ProviderError::Unavailable(
                    value
                        .get("error")
                        .and_then(|value| value.as_str())
                        .unwrap_or("bridge rejected request")
                        .to_owned(),
                ));
            }
            let text = value
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_owned();
            if text.is_empty() {
                return Err(ProviderError::InvalidResponse(
                    "empty Codex response".to_owned(),
                ));
            }
            Ok(ProviderCompletion {
                text,
                provider: self.name().to_owned(),
                tool_calls: vec![],
                usage: Usage::default(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            Err(ProviderError::Unavailable(format!(
                "Codex bridge {} requires Unix-domain sockets",
                self.socket_path
            )))
        }
    }
}

#[derive(Clone, Debug)]
pub struct RoutingPolicy {
    pub default: String,
    pub deep: String,
    pub codex: String,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            default: "gemma".to_owned(),
            deep: "gpt-oss".to_owned(),
            codex: "codex-sol".to_owned(),
        }
    }
}

pub struct ProviderRouter {
    providers: HashMap<String, Arc<dyn Provider>>,
    policy: RoutingPolicy,
}

impl ProviderRouter {
    pub fn new(policy: RoutingPolicy) -> Self {
        Self {
            providers: HashMap::new(),
            policy,
        }
    }
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.insert(provider.name().to_owned(), provider);
    }
    pub fn select(
        &self,
        text: &str,
        explicit: Option<&str>,
    ) -> Result<Arc<dyn Provider>, ProviderError> {
        let selected = explicit.map(str::to_owned).unwrap_or_else(|| {
            let normalized = text.to_lowercase();
            if [
                "ask codex",
                "use codex",
                "use sol",
                "final verification",
                "highest confidence",
            ]
            .iter()
            .any(|phrase| normalized.contains(phrase))
            {
                self.policy.codex.clone()
            } else if [
                "think deeply",
                "deep reasoning",
                "deep analysis",
                "use gpt-oss",
                "architecture review",
                "security review",
                "threat model",
            ]
            .iter()
            .any(|phrase| normalized.contains(phrase))
                || normalized.len() >= 600
            {
                self.policy.deep.clone()
            } else {
                self.policy.default.clone()
            }
        });
        self.providers.get(&selected).cloned().ok_or_else(|| {
            ProviderError::Unavailable(format!("provider {selected} is not configured"))
        })
    }
}

#[cfg(unix)]
fn render_message(message: &ChatMessage) -> String {
    let role = match message.role {
        Role::System => "SYSTEM",
        Role::User => "USER",
        Role::Assistant => "ASSISTANT",
        Role::Tool => "TOOL",
    };
    format!("{role}: {}", message.content)
}
