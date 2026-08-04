use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntentId(pub String);

impl From<&str> for IntentId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Device,
    Provider,
    Service,
    Document,
    Artifact,
    Task,
    Person,
    Project,
    Skill,
    Email,
    Location,
    Memory,
    Decision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    pub kind: EntityKind,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Multiplier,
    Percent,
    Bytes,
    Seconds,
    Minutes,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Confidence {
    pub score: f32,
    pub level: ConfidenceLevel,
}

impl Confidence {
    pub fn new(score: f32) -> Self {
        let score = score.clamp(0.0, 1.0);
        let level = if score >= 0.9 {
            ConfidenceLevel::High
        } else if score >= 0.7 {
            ConfidenceLevel::Medium
        } else {
            ConfidenceLevel::Low
        };
        Self { score, level }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    Deterministic,
    ApprovedAlias,
    ModelFallback,
    UserCorrection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanonicalRequest {
    pub intent: IntentId,
    #[serde(default)]
    pub entities: Vec<EntityRef>,
    #[serde(default)]
    pub arguments: BTreeMap<String, Value>,
    pub confidence: Confidence,
    pub source: ResolutionSource,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelCandidate {
    pub intent: IntentId,
    #[serde(default)]
    pub entities: Vec<EntityRef>,
    #[serde(default)]
    pub arguments: BTreeMap<String, Value>,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentKind {
    String,
    Number,
    Boolean,
    Object,
    Array,
    Entity(EntityKind),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArgumentSpec {
    pub name: String,
    pub kind: ArgumentKind,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntentDefinition {
    pub id: IntentId,
    pub description: String,
    pub arguments: Vec<ArgumentSpec>,
    pub approval_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDefinition {
    pub kind: EntityKind,
    pub id: String,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alias {
    pub owner_id: String,
    pub phrase: String,
    pub entity: EntityRef,
    pub approved_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct AliasInput {
    pub phrase: String,
    pub entity_kind: EntityKind,
    pub entity_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub field: String,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    pub request: CanonicalRequest,
    pub note: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Resolved,
    NeedsConfirmation,
    Unrecognized,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorDisposition {
    Execute,
    AskForConfirmation,
    AskClarifyingQuestion,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorResult {
    pub disposition: ValidatorDisposition,
    pub reason: String,
    #[serde(default)]
    pub issues: Vec<ValidationIssue>,
}

impl Default for ValidatorResult {
    fn default() -> Self {
        Self {
            disposition: ValidatorDisposition::AskClarifyingQuestion,
            reason: "legacy_decision_requires_revalidation".to_owned(),
            issues: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InterpretationDecision {
    pub id: String,
    pub owner_id: String,
    pub original_phrase: String,
    pub normalized_phrase: String,
    #[serde(default = "legacy_catalog_version")]
    pub catalog_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<CanonicalRequest>,
    pub status: DecisionStatus,
    #[serde(default)]
    pub validation_issues: Vec<ValidationIssue>,
    #[serde(default)]
    pub validator: ValidatorResult,
    #[serde(default)]
    pub corrections: Vec<Correction>,
    pub final_decision: DecisionStatus,
    pub created_at: String,
    pub updated_at: String,
}

fn legacy_catalog_version() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegressionCase {
    pub phrase: String,
    pub expected_intent: IntentId,
    pub expected_disposition: ValidatorDisposition,
    #[serde(default)]
    pub corrected: bool,
}
