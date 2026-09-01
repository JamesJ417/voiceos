use serde::{Deserialize, Serialize};

use crate::CalendarSecretReference;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleCalendarConnection {
    pub owner_id: String,
    pub provider: String,
    pub account_email: String,
    pub provider_account_id: String,
    #[serde(skip)]
    pub secret_reference: Option<CalendarSecretReference>,
}

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
    pub category: String,
    pub status: String,
    pub confidence: f64,
    pub provenance: String,
    pub supersedes_memory_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleepCycleRecord {
    pub id: String,
    pub owner_id: String,
    pub idempotency_key: String,
    pub mode: String,
    pub dry_run: bool,
    pub status: String,
    pub previous_cycle_id: Option<String>,
    pub event_watermark: i64,
    pub message_watermark: i64,
    pub events_inspected: u64,
    pub messages_inspected: u64,
    pub memories_before: u64,
    pub memories_after: u64,
    pub proposed_changes: u64,
    pub committed_changes: u64,
    pub summary: String,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveMemoryChange {
    pub content: String,
    pub source: String,
    pub evidence: serde_json::Value,
}

impl LiveMemoryChange {
    pub fn add(content: impl Into<String>, source: impl Into<String>) -> Self {
        let source = source.into();
        Self {
            content: content.into(),
            evidence: serde_json::json!([{ "source": source.clone() }]),
            source,
        }
    }

    pub fn with_evidence(mut self, evidence: serde_json::Value) -> Self {
        self.evidence = evidence;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SleepCycleChange {
    pub id: String,
    pub sleep_cycle_id: String,
    pub operation: String,
    pub memory_kind: String,
    pub title: String,
    pub detail: String,
    pub status: String,
    pub confidence: Option<f64>,
    pub evidence: serde_json::Value,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SleepCycleReport {
    pub cycle: SleepCycleRecord,
    pub changes: Vec<SleepCycleChange>,
    pub new_evidence_count: u64,
    pub durable_memory_delta: i64,
    pub proposed_change_delta: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuarantineRecord {
    pub quarantine_id: String,
    pub conversation_id: String,
    pub claim_id: String,
    pub source: String,
    pub provenance: String,
    pub confidence: f32,
    pub relevance: f32,
    pub content: String,
    pub reason: String,
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
pub struct AttachmentRecord {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConversationContext {
    pub conversation_id: String,
    pub area_id: String,
    pub summary: Option<String>,
    pub memories: Vec<Memory>,
    pub document_context: Option<String>,
    pub recent_messages: Vec<ChatMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub sequence: i64,
    pub conversation_id: String,
    pub area_id: String,
    pub role: Role,
    pub content: String,
    pub provider: Option<String>,
    pub origin_device_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub attachments: Vec<AttachmentRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationArea {
    pub id: String,
    pub display_name: String,
    pub position: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: String,
    pub owner_id: String,
    pub area_id: String,
    pub title: String,
    pub status: String,
    pub message_count: u64,
    pub last_message_preview: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub area_updated_at: String,
    pub area_updated_by_device: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationDay {
    pub date: String,
    pub conversations: Vec<ConversationRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationExport {
    pub version: u8,
    pub export_id: String,
    pub source_conversation_id: String,
    pub area_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<ConversationExportMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationExportMessage {
    pub role: Role,
    pub content: String,
    pub provider: Option<String>,
    pub origin_device_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSyncRecord {
    pub conversation_id: String,
    pub area_id: String,
    pub area_updated_at: String,
    pub area_updated_by_device: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSyncPayload {
    pub cursor: i64,
    pub selected_area_id: String,
    pub active_conversation_id: Option<String>,
    pub conversations: Vec<ConversationSyncRecord>,
    pub messages: Vec<ConversationMessage>,
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
    #[serde(default)]
    pub image_attachments: Vec<ProviderImageAttachment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderImageAttachment {
    pub attachment_id: String,
    pub filename: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
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
    pub importance: String,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReviewRun {
    pub id: String,
    pub task_id: String,
    pub status: String,
    pub lease_expires_at: String,
    pub safe_actions: Vec<String>,
    pub blockers: Vec<String>,
    pub ideas: Vec<String>,
    pub summary: Option<String>,
    pub error_code: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskReviewClaim {
    pub review: TaskReviewRun,
    pub task: TaskDetail,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskReviewSnapshot {
    pub active_review: Option<TaskReviewRun>,
    pub last_completed_review: Option<TaskReviewRun>,
    pub recent_reviews: Vec<TaskReviewRun>,
    pub cursor_task_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusSessionRecord {
    pub id: String,
    pub owner_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    pub mode: String,
    pub planned_minutes: u32,
    pub status: String,
    pub next_action: String,
    pub interruption_note: Option<String>,
    pub restart_action: Option<String>,
    pub reflection: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusPriority {
    pub task_id: String,
    pub title: String,
    pub observable_outcome: String,
    pub estimated_minutes: u32,
    pub due_at: Option<String>,
    pub importance: String,
    pub urgency: String,
    pub status: String,
    pub next_action: String,
    pub project_title: Option<String>,
    pub goal_title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusSnapshot {
    pub mode: String,
    pub active_session: Option<FocusSessionRecord>,
    pub priorities: Vec<FocusPriority>,
    pub recommendation: Option<FocusPriority>,
    pub last_interrupted_session: Option<FocusSessionRecord>,
    pub parked: Vec<FocusPriority>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalFocusReset {
    pub active_session: Option<FocusSessionRecord>,
    pub interrupted_session: Option<FocusSessionRecord>,
    pub priorities: Vec<FocusPriority>,
    pub recommendation: Option<FocusPriority>,
    pub first_physical_action: Option<String>,
    pub five_minute_version: Option<String>,
    pub optional_question: Option<String>,
    pub message: String,
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
    pub updated_at: String,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProactiveSubscription {
    pub id: String,
    pub owner_id: String,
    pub topic: String,
    pub project_id: Option<String>,
    pub source_type: String,
    pub cadence: String,
    pub quiet_hours: Option<String>,
    pub status: String,
    pub provenance: String,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProactiveCandidate {
    pub id: String,
    pub owner_id: String,
    pub subscription_id: Option<String>,
    pub project_id: Option<String>,
    pub reason: String,
    pub evidence: serde_json::Value,
    pub priority: String,
    pub confidence: f64,
    pub expires_at: String,
    pub deduplication_key: String,
    pub provenance: String,
    pub created_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutreachProposal {
    pub id: String,
    pub owner_id: String,
    pub candidate_id: String,
    pub original_draft: String,
    pub editable_draft: String,
    pub channel: String,
    pub approval_state: String,
    pub risk_class: String,
    pub delivery_deadline: Option<String>,
    pub provenance: String,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutreachDelivery {
    pub id: String,
    pub owner_id: String,
    pub proposal_id: String,
    pub provider: String,
    pub channel: String,
    pub result: String,
    pub idempotency_key: String,
    pub response_link: Option<String>,
    pub provenance: String,
    pub created_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProactiveFeedback {
    pub id: String,
    pub owner_id: String,
    pub proposal_id: Option<String>,
    pub action: String,
    pub note: Option<String>,
    pub provenance: String,
    pub created_at: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProactiveDraftingInput {
    pub owner_id: String,
    pub candidate_id: String,
    pub project_id: Option<String>,
    pub reason: String,
    pub priority: String,
    pub candidate_confidence: f64,
    pub evidence_ids: Vec<String>,
    pub candidate_expires_at: String,
}

pub trait ProactiveDraftingContract {
    /// Returns a local structured draft. Implementations receive no tools or store access.
    fn draft(&self, input: &ProactiveDraftingInput) -> Result<String, String>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProactiveSubscription {
    pub owner_id: String,
    pub topic: String,
    pub project_id: Option<String>,
    pub source_type: String,
    pub cadence: String,
    pub quiet_hours: Option<String>,
    pub status: String,
    pub provenance: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewProactiveCandidate {
    pub owner_id: String,
    pub subscription_id: Option<String>,
    pub project_id: Option<String>,
    pub reason: String,
    pub evidence: serde_json::Value,
    pub priority: String,
    pub confidence: f64,
    pub expires_at: String,
    pub deduplication_key: String,
    pub provenance: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewOutreachProposal {
    pub owner_id: String,
    pub candidate_id: String,
    pub original_draft: String,
    pub editable_draft: String,
    pub channel: String,
    pub approval_state: String,
    pub risk_class: String,
    pub delivery_deadline: Option<String>,
    pub provenance: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewOutreachDelivery {
    pub owner_id: String,
    pub proposal_id: String,
    pub provider: String,
    pub channel: String,
    pub result: String,
    pub idempotency_key: String,
    pub response_link: Option<String>,
    pub provenance: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProactiveFeedback {
    pub owner_id: String,
    pub proposal_id: Option<String>,
    pub action: String,
    pub note: Option<String>,
    pub provenance: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSource {
    pub kind: String,
    pub id: String,
}

impl CaptureSource {
    pub fn voice(id: impl Into<String>) -> Self {
        Self {
            kind: "voice".into(),
            id: id.into(),
        }
    }

    pub fn fieldy(event_id: impl Into<String>) -> Self {
        Self {
            kind: "fieldy".into(),
            id: event_id.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewPersonalCapture {
    pub owner_id: String,
    pub source: String,
    pub source_id: String,
    pub raw_content: String,
    pub structured_content: Option<serde_json::Value>,
    pub created_at: String,
    pub expires_at: String,
    pub audit_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersonalCapture {
    pub id: String,
    pub owner_id: String,
    pub source: String,
    pub source_id: String,
    pub raw_content: String,
    pub display_text: String,
    pub structured_content: Option<serde_json::Value>,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub audit_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewCaptureProposal {
    pub owner_id: String,
    pub capture_id: String,
    pub title: String,
    pub category: String,
    pub rationale: String,
    pub expires_at: String,
    pub audit_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureProposal {
    pub id: String,
    pub owner_id: String,
    pub capture_id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub category: String,
    pub confidence: f64,
    pub details: Option<String>,
    pub suggested_next_action: String,
    pub rationale: String,
    pub evidence_capture_ids: Vec<String>,
    pub dedupe_key: String,
    pub occurrence_count: u32,
    pub last_seen_at: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub audit_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalExtractionInput {
    pub owner_id: String,
    pub capture_id: String,
    pub raw_content: String,
    pub display_text: String,
    pub capture_expires_at: String,
}

pub trait PersonalExtractionContract {
    /// Returns structured proposals only; implementations receive no store or provider access.
    fn extract(&self, input: &PersonalExtractionInput) -> Result<String, String>;
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalReviewRecord {
    pub id: String,
    pub owner_id: String,
    pub proposal_id: String,
    pub capture_id: String,
    pub project_id: Option<String>,
    pub category: String,
    pub title: String,
    pub details: Option<String>,
    pub suggested_next_action: String,
    pub status: String,
    pub created_at: String,
    pub audit_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskApprovalStatus {
    Proposed,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDecision {
    pub id: String,
    pub owner_id: String,
    pub status: String,
    pub audit_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyFocusReset {
    pub id: String,
    pub owner_id: String,
    pub reset_date: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub audit_id: String,
}
