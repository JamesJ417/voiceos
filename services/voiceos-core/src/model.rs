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
    pub due_at: Option<String>,
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
pub struct OutreachRecord {
    pub id: String,
    pub owner_id: String,
    pub kind: String,
    pub priority: String,
    pub title: String,
    pub body: String,
    pub reason: String,
    pub status: String,
    pub task_id: Option<String>,
    pub conversation_id: Option<String>,
    pub dedupe_key: Option<String>,
    pub actions: Vec<String>,
    pub scheduled_for: String,
    pub created_at: String,
    pub delivered_at: Option<String>,
    pub responded_at: Option<String>,
    pub snoozed_until: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutreachPolicy {
    pub owner_id: String,
    pub enabled: bool,
    pub quiet_hours_start: String,
    pub quiet_hours_end: String,
    pub timezone: String,
    pub max_checkins_per_day: u32,
    pub cooldown_minutes: u32,
    pub driving_mode: bool,
    pub spoken_headphones_only: bool,
    pub daily_digest_enabled: bool,
    pub do_not_disturb: bool,
    pub current_location: String,
    pub daily_planning_time: String,
    pub morning_digest_time: String,
    pub evening_digest_time: String,
    pub scan_interval_minutes: u32,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutreachPolicyUpdate {
    pub enabled: Option<bool>,
    pub quiet_hours_start: Option<String>,
    pub quiet_hours_end: Option<String>,
    pub timezone: Option<String>,
    pub max_checkins_per_day: Option<u32>,
    pub cooldown_minutes: Option<u32>,
    pub driving_mode: Option<bool>,
    pub spoken_headphones_only: Option<bool>,
    pub daily_digest_enabled: Option<bool>,
    pub do_not_disturb: Option<bool>,
    pub current_location: Option<String>,
    pub daily_planning_time: Option<String>,
    pub morning_digest_time: Option<String>,
    pub evening_digest_time: Option<String>,
    pub scan_interval_minutes: Option<u32>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationFloor {
    pub owner_id: String,
    pub conversation_id: String,
    pub lease_id: Option<String>,
    pub holder_device_id: Option<String>,
    pub holder_display_name: Option<String>,
    pub phase: String,
    pub partial_transcript: Option<String>,
    pub response_text: Option<String>,
    pub revision: i64,
    pub acquired_at: Option<String>,
    pub updated_at: String,
    pub expires_at_unix: i64,
    pub active: bool,
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
pub struct SkillUsage {
    pub id: String,
    pub owner_id: String,
    pub skill_id: String,
    pub skill_name: String,
    pub skill_version: u32,
    pub conversation_id: Option<String>,
    pub request_id: Option<String>,
    pub tool_calls: serde_json::Value,
    pub result: serde_json::Value,
    pub outcome: String,
    pub feedback: Option<String>,
    pub feedback_note: Option<String>,
    pub used_at: String,
    pub reviewed_at: Option<String>,
    pub reviewed_by: Option<String>,
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
pub struct AutomationFrequencyLimit {
    pub max_runs: u32,
    pub window_minutes: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationRule {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger: serde_json::Value,
    pub conditions: serde_json::Value,
    pub permitted_actions: Vec<String>,
    pub frequency_limit: AutomationFrequencyLimit,
    pub evidence: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttentionItem {
    pub id: String,
    pub owner_id: String,
    pub category: String,
    pub source_id: String,
    pub title: String,
    pub summary: String,
    pub urgency: String,
    pub status: String,
    pub task_id: Option<String>,
    pub occurred_at: String,
    pub due_at: Option<String>,
    pub approval_required: bool,
    pub available_actions: Vec<String>,
    pub evidence: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSchedule {
    pub task_id: String,
    pub owner_id: String,
    pub earliest_start_at: Option<String>,
    pub recurrence_rule: Option<String>,
    pub location: Option<String>,
    pub preparation_minutes: u32,
    pub travel_minutes: u32,
    pub preferred_time: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub owner_id: String,
    pub source_id: String,
    pub title: String,
    pub start_at: String,
    pub end_at: String,
    pub location: Option<String>,
    pub status: String,
    pub response_status: String,
    pub task_id: Option<String>,
    pub preparation_minutes: u32,
    pub travel_minutes: u32,
    pub metadata: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedWorkBlock {
    pub task_id: String,
    pub title: String,
    pub start_at: String,
    pub end_at: String,
    pub location: Option<String>,
    pub preparation_minutes: u32,
    pub travel_minutes: u32,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyWorkPlan {
    pub owner_id: String,
    pub date: String,
    pub generated_at: String,
    pub current_location: String,
    pub blocks: Vec<PlannedWorkBlock>,
    pub unscheduled_task_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateProposal {
    pub id: String,
    pub owner_id: String,
    pub component: String,
    pub current_version: String,
    pub proposed_version: String,
    pub status: String,
    pub release_notes: String,
    pub dependency_changes: serde_json::Value,
    pub api_changes: serde_json::Value,
    pub configuration_changes: serde_json::Value,
    pub skill_changes: serde_json::Value,
    pub security_changes: serde_json::Value,
    pub affected_components: serde_json::Value,
    pub rollback_version: String,
    pub candidate_path: Option<String>,
    pub evidence: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id: String,
    pub owner_id: String,
    pub job_id: Option<String>,
    pub task_id: Option<String>,
    pub parent_artifact_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub filename: String,
    pub media_type: String,
    pub description: String,
    pub status: String,
    pub progress_percent: u32,
    pub storage_key: Option<String>,
    pub uri: String,
    pub sha256: Option<String>,
    pub byte_size: Option<u64>,
    pub version: u32,
    pub metadata: serde_json::Value,
    pub error: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
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
