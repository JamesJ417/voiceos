use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub source: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRecord {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub mode: String,
    pub byte_size: u64,
    pub sha256: String,
    pub chunk_count: usize,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationContext {
    pub conversation_id: String,
    pub summary: Option<String>,
    pub memories: Vec<Memory>,
    pub document_context: Option<String>,
    pub recent_messages: Vec<ChatMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub sequence: i64,
    pub conversation_id: String,
    pub role: Role,
    pub content: String,
    pub provider: Option<String>,
    pub origin_device_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub conversation_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderCompletion {
    pub text: String,
    pub provider: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalRecord {
    pub id: String,
    pub owner_id: String,
    pub title: String,
    pub desired_outcome: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: String,
    pub owner_id: String,
    pub goal_id: Option<String>,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub owner_id: String,
    pub project_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub observable_outcome: String,
    pub estimated_minutes: u32,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskStepRecord {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub owner: String,
    pub status: String,
    pub evidence: serde_json::Value,
    pub position: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBlockerRecord {
    pub id: String,
    pub task_id: String,
    pub description: String,
    pub owner: String,
    pub status: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskHandoffRecord {
    pub id: String,
    pub task_id: String,
    pub from_owner: String,
    pub to_owner: String,
    pub kind: String,
    pub summary: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskArtifactRecord {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub uri: String,
    pub description: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskProgress {
    pub completed_steps: usize,
    pub total_steps: usize,
    pub open_blockers: usize,
    pub lane: String,
    pub vic_status: String,
    pub next_user_action: Option<String>,
    pub next_vic_action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskDetail {
    pub task: TaskRecord,
    pub initiative: Option<JobRecord>,
    pub progress: TaskProgress,
    pub steps: Vec<TaskStepRecord>,
    pub blockers: Vec<TaskBlockerRecord>,
    pub handoffs: Vec<TaskHandoffRecord>,
    pub artifacts: Vec<TaskArtifactRecord>,
    pub approvals: Vec<serde_json::Value>,
    pub activity: Vec<ExecutionEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub owner_id: String,
    pub task_id: Option<String>,
    pub status: String,
    pub idempotency_key: String,
    pub capability_scope: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskInitiative {
    pub task_id: String,
    pub job_id: String,
    pub status: String,
    pub summary: String,
    pub capabilities: Vec<String>,
    pub started_actions: Vec<String>,
    pub next_actions: Vec<String>,
    pub approval_boundary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub id: i64,
    pub owner_id: String,
    pub stream_id: String,
    pub event_type: String,
    pub actor: String,
    pub payload: serde_json::Value,
    pub occurred_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillProposal {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub version: u32,
    pub status: String,
    pub content: String,
    pub required_capabilities: serde_json::Value,
    pub evidence: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationProposal {
    pub id: String,
    pub owner_id: String,
    pub skill_id: String,
    pub status: String,
    pub trigger: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id: String,
    pub owner_id: String,
    pub job_id: Option<String>,
    pub kind: String,
    pub uri: String,
    pub sha256: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRunMetric {
    pub id: i64,
    pub owner_id: String,
    pub job_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub duration_ms: u64,
    pub output_tokens_per_second: Option<f64>,
    pub cost_usd: Option<f64>,
    pub status: String,
    pub created_at: String,
}
