use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    ActiveTurn,
    Conversation,
    ConversationSummary,
    ExplicitMemory,
    Document,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextClaim {
    pub id: String,
    pub conversation_id: String,
    pub source: ContextSource,
    pub provenance: String,
    pub confidence: f32,
    pub relevance: f32,
    pub content: String,
}

impl ContextClaim {
    pub fn new(
        id: impl Into<String>,
        conversation_id: impl Into<String>,
        source: ContextSource,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            conversation_id: conversation_id.into(),
            source,
            provenance: "runtime".to_owned(),
            confidence: 1.0,
            relevance: 1.0,
            content: content.into(),
        }
    }

    pub fn with_metadata(
        mut self,
        provenance: impl Into<String>,
        confidence: f32,
        relevance: f32,
    ) -> Self {
        self.provenance = provenance.into();
        self.confidence = confidence.clamp(0.0, 1.0);
        self.relevance = relevance.clamp(0.0, 1.0);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuarantinedClaim {
    pub claim: ContextClaim,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub accepted: Vec<ContextClaim>,
    pub quarantined: Vec<QuarantinedClaim>,
}

pub fn validate_context(
    active_conversation_id: &str,
    claims: impl IntoIterator<Item = ContextClaim>,
) -> IntegrityReport {
    let mut report = IntegrityReport::default();
    for claim in claims {
        let reason = if claim.conversation_id.trim().is_empty() {
            Some("missing conversation scope".to_owned())
        } else if claim.conversation_id != active_conversation_id {
            Some("conversation scope does not match the active conversation".to_owned())
        } else if claim.content.trim().is_empty() {
            Some("empty context content".to_owned())
        } else if !(0.0..=1.0).contains(&claim.confidence) {
            Some("confidence is outside the allowed range".to_owned())
        } else if !(0.0..=1.0).contains(&claim.relevance) {
            Some("relevance is outside the allowed range".to_owned())
        } else {
            None
        };

        if let Some(reason) = reason {
            report.quarantined.push(QuarantinedClaim { claim, reason });
        } else {
            report.accepted.push(claim);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_claims_from_another_conversation() {
        let report = validate_context(
            "current",
            [
                ContextClaim::new("good", "current", ContextSource::Conversation, "James"),
                ContextClaim::new("bad", "old", ContextSource::Conversation, "Dave Johnson"),
            ],
        );

        assert_eq!(report.accepted.len(), 1);
        assert_eq!(report.quarantined.len(), 1);
        assert_eq!(report.quarantined[0].claim.id, "bad");
        assert!(report.quarantined[0].reason.contains("does not match"));
    }

    #[test]
    fn records_provenance_and_bounds_confidence_and_relevance() {
        let claim = ContextClaim::new("memory", "current", ContextSource::ExplicitMemory, "fact")
            .with_metadata("memory://owner/fact", 0.8, 0.6);
        assert_eq!(claim.provenance, "memory://owner/fact");
        assert_eq!(claim.confidence, 0.8);
        assert_eq!(claim.relevance, 0.6);

        let report = validate_context(
            "current",
            [ContextClaim {
                confidence: 1.1,
                ..claim
            }],
        );
        assert_eq!(report.quarantined.len(), 1);
        assert!(report.quarantined[0].reason.contains("confidence"));
    }
    #[test]
    fn rejects_unscoped_or_empty_claims() {
        let report = validate_context(
            "current",
            [
                ContextClaim::new("unscoped", "", ContextSource::ExplicitMemory, "x"),
                ContextClaim::new("empty", "current", ContextSource::Document, "  "),
            ],
        );

        assert!(report.accepted.is_empty());
        assert_eq!(report.quarantined.len(), 2);
    }
}
