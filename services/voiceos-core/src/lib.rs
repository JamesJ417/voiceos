mod agent_store;
mod calendar_secret_store;
mod conversation_area;
mod conversation_store;
pub mod engine;
pub mod execution;
mod fieldy;
mod floor;
mod focus;
mod google_calendar_oauth;
mod governance;
mod integrity;
mod model;
mod outreach;
mod personal_support;
mod proactive;
mod provider;
mod schema;
mod skill_proposal;
mod sleep_cycle;
mod store;
mod task_initiative;
mod task_progress;
mod task_review;

pub use calendar_secret_store::{
    CalendarSecretReference, CalendarSecretStore, CalendarSecretStoreError,
    InMemoryCalendarSecretStore, SecretServiceBackend, SecretServiceBackendError,
    SecretToolCalendarSecretStore, UnavailableCalendarSecretStore,
};
pub use conversation_area::{
    GENERAL_TALK_AREA_ID, built_in_conversation_areas, is_valid_conversation_area,
};
pub use engine::{
    ConversationEngine, EngineConfig, EngineError, ExplicitMemoryExtractor, HeuristicSummarizer,
    MemoryExtractor, OwnerTurnInput, Summarizer,
};
pub use fieldy::{
    DEFAULT_FIELDY_RETENTION_DAYS, FieldyTranscriptEvent, FieldyTranscriptIntake,
    FieldyWebhookError, FieldyWebhookStore, MAX_FIELDY_BODY_BYTES, verify_fieldy_signature,
};
pub use google_calendar_oauth::{
    ConsumedGoogleCalendarAuthorizationSession, GOOGLE_CALENDAR_AUTHORIZATION_SESSION_TTL,
    GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI, GoogleCalendarAuthorizationSession,
    GoogleCalendarAuthorizationSessionError, GoogleCalendarAuthorizationSessions,
    GoogleCalendarOAuthConfiguration, GoogleCalendarOAuthConfigurationError,
    ValidatedGoogleCalendarOAuthConfiguration,
};
pub use governance::{
    AuditEvent, AuthorizationContext, CapabilityGrant, Identity, IdentityError, PublicAuditEvent,
    ReleaseManifest,
};
pub use integrity::{
    ContextClaim, ContextSource, IntegrityReport, QuarantinedClaim, validate_context,
};
pub use model::{
    ArtifactRecord, AttachmentRecord, AutomationProposal, CaptureProposal, CaptureSource,
    ChatMessage, ConversationArea, ConversationContext, ConversationDay, ConversationExport,
    ConversationExportMessage, ConversationFloor, ConversationMessage, ConversationRecord,
    ConversationSyncPayload, ConversationSyncRecord, DailyFocusReset, DocumentRecord,
    ExecutionEvent, FocusPriority, FocusSessionRecord, FocusSnapshot, GoalRecord,
    GoogleCalendarConnection, JobRecord, LiveMemoryChange, Memory, NewCaptureProposal,
    NewOutreachDelivery, NewOutreachProposal, NewPersonalCapture, NewProactiveCandidate,
    NewProactiveFeedback, NewProactiveSubscription, OutreachDelivery, OutreachPolicy,
    OutreachProposal, OutreachRecord, PersonalCapture, PersonalExtractionContract,
    PersonalExtractionInput, PersonalFocusReset, PersonalReviewRecord, ProactiveCandidate,
    ProactiveDraftingContract, ProactiveDraftingInput, ProactiveFeedback, ProactiveSubscription,
    ProjectRecord, ProviderCompletion, ProviderImageAttachment, ProviderRequest, ProviderRunMetric,
    QuarantineRecord, ReviewDecision, Role, SkillProposal, SkillUsage, SleepCycleChange,
    SleepCycleRecord, SleepCycleReport, TaskApprovalStatus, TaskArtifactRecord, TaskBlockerRecord,
    TaskDetail, TaskHandoffRecord, TaskInitiative, TaskProgress, TaskRecord, TaskReviewClaim,
    TaskReviewRun, TaskReviewSnapshot, TaskStepRecord, ToolCall, ToolDefinition, Usage,
};
pub use provider::{
    CodexBridgeProvider, MockProvider, OllamaProvider, Provider, ProviderError, ProviderRouter,
    RoutingPolicy,
};
pub use store::{CalendarSecretIntegrationError, ConversationStore, StoreError};
pub use task_initiative::begin_task_initiative;
