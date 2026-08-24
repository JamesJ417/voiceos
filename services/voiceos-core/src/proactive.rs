use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use rusqlite::params;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    ConversationStore, NewOutreachDelivery, NewOutreachProposal, NewProactiveCandidate,
    NewProactiveFeedback, NewProactiveSubscription, OutreachDelivery, OutreachProposal,
    ProactiveCandidate, ProactiveDraftingContract, ProactiveDraftingInput, ProactiveFeedback,
    ProactiveSubscription, StoreError,
};

impl ConversationStore {
    pub fn create_proactive_subscription(
        &self,
        input: NewProactiveSubscription,
    ) -> Result<ProactiveSubscription, StoreError> {
        validate(&[
            (&input.owner_id, "owner_id"),
            (&input.topic, "topic"),
            (&input.source_type, "source_type"),
            (&input.cadence, "cadence"),
            (&input.status, "status"),
            (&input.provenance, "provenance"),
        ])?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, &input.owner_id, &now)?;
        require_owned_optional(
            &connection,
            "projects",
            "project_id",
            input.project_id.as_deref(),
            input.owner_id.trim(),
            "project",
        )?;
        connection.execute("INSERT INTO proactive_subscriptions(subscription_id,owner_id,topic,project_id,source_type,cadence,quiet_hours,status,provenance,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)", params![id, input.owner_id.trim(), input.topic.trim(), input.project_id.as_deref().map(str::trim), input.source_type.trim(), input.cadence.trim(), input.quiet_hours.as_deref().map(str::trim), input.status.trim(), input.provenance.trim(), now])?;
        subscription(&connection, &id).map_err(Into::into)
    }

    pub fn create_proactive_candidate(
        &self,
        input: NewProactiveCandidate,
    ) -> Result<ProactiveCandidate, StoreError> {
        validate(&[
            (&input.owner_id, "owner_id"),
            (&input.reason, "reason"),
            (&input.priority, "priority"),
            (&input.expires_at, "expires_at"),
            (&input.deduplication_key, "deduplication_key"),
            (&input.provenance, "provenance"),
        ])?;
        if !(0.0..=1.0).contains(&input.confidence)
            || !input.evidence.is_array()
            || input.evidence.as_array().is_some_and(Vec::is_empty)
        {
            return Err(StoreError::InvalidInput(
                "candidate evidence or confidence is invalid".into(),
            ));
        }
        let expires_at = DateTime::parse_from_rfc3339(input.expires_at.trim())
            .map_err(|_| {
                StoreError::InvalidInput("expires_at must be an RFC 3339 date-time".into())
            })?
            .with_timezone(&Utc);
        let now = Utc::now();
        if expires_at <= now {
            return Err(StoreError::InvalidInput(
                "expires_at must be strictly in the future".into(),
            ));
        }
        let now = now.to_rfc3339();
        let connection = self.connection()?;
        ensure_owner(&connection, &input.owner_id, &now)?;
        require_owned_optional(
            &connection,
            "projects",
            "project_id",
            input.project_id.as_deref(),
            input.owner_id.trim(),
            "project",
        )?;
        require_owned_optional(
            &connection,
            "proactive_subscriptions",
            "subscription_id",
            input.subscription_id.as_deref(),
            input.owner_id.trim(),
            "subscription",
        )?;
        validate_candidate_evidence(
            &connection,
            input.owner_id.trim(),
            input.project_id.as_deref().map(str::trim),
            &input.evidence,
        )?;
        let id = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO proactive_candidates(candidate_id,owner_id,subscription_id,project_id,reason,evidence_json,priority,confidence,expires_at,deduplication_key,provenance,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(owner_id,deduplication_key) DO NOTHING",
            params![id, input.owner_id.trim(), input.subscription_id.as_deref().map(str::trim), input.project_id.as_deref().map(str::trim), input.reason.trim(), serde_json::to_string(&input.evidence)?, input.priority.trim(), input.confidence, input.expires_at.trim(), input.deduplication_key.trim(), input.provenance.trim(), now],
        )?;
        let canonical_id: String = connection.query_row(
            "SELECT candidate_id FROM proactive_candidates WHERE owner_id=?1 AND deduplication_key=?2",
            params![input.owner_id.trim(), input.deduplication_key.trim()],
            |row| row.get(0),
        )?;
        candidate(&connection, &canonical_id).map_err(Into::into)
    }

    /// Detect active owner-scoped projects whose latest local progress is older
    /// than `stale_after_seconds` at the caller-provided RFC 3339 `as_of` time.
    /// This only creates canonical candidate records; it never drafts, delivers,
    /// or otherwise contacts anyone.
    pub fn detect_stale_project_candidates(
        &self,
        owner_id: &str,
        as_of: &str,
        stale_after_seconds: i64,
    ) -> Result<Vec<ProactiveCandidate>, StoreError> {
        validate(&[(owner_id, "owner_id")])?;
        if stale_after_seconds <= 0 {
            return Err(StoreError::InvalidInput(
                "stale_after_seconds must be positive".into(),
            ));
        }
        let as_of = DateTime::parse_from_rfc3339(as_of)
            .map_err(|_| StoreError::InvalidInput("as_of must be an RFC 3339 date-time".into()))?
            .with_timezone(&Utc);
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT project_id, source_type, source_id, occurred_at FROM (
                 SELECT project_id, 'project' AS source_type, project_id AS source_id, updated_at AS occurred_at
                 FROM projects WHERE owner_id=?1 AND status='active'
                 UNION ALL
                 SELECT p.project_id, 'task', t.task_id, t.updated_at
                 FROM projects p JOIN tasks t ON t.owner_id=p.owner_id AND t.project_id=p.project_id
                 WHERE p.owner_id=?1 AND p.status='active'
                 UNION ALL
                 SELECT p.project_id, 'execution_event', CAST(e.event_id AS TEXT), e.occurred_at
                 FROM projects p JOIN execution_events e ON e.owner_id=p.owner_id
                 WHERE p.owner_id=?1 AND p.status='active'
                   AND (e.stream_id=p.project_id OR EXISTS (
                       SELECT 1 FROM tasks t WHERE t.owner_id=p.owner_id
                       AND t.project_id=p.project_id AND t.task_id=e.stream_id
                   ))
             ) ORDER BY project_id, source_type, source_id",
        )?;
        let rows = statement.query_map([owner_id.trim()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut latest_by_project = BTreeMap::new();
        for row in rows {
            let (project_id, source_type, source_id, occurred_at) = row?;
            let timestamp = DateTime::parse_from_rfc3339(&occurred_at)
                .map_err(|_| {
                    StoreError::InvalidInput("stored progress timestamp is invalid".into())
                })?
                .with_timezone(&Utc);
            let replace = latest_by_project.get(&project_id).is_none_or(
                |current: &(DateTime<Utc>, String, String, String)| {
                    (timestamp, &source_type, &source_id) > (current.0, &current.1, &current.2)
                },
            );
            if replace {
                latest_by_project
                    .insert(project_id, (timestamp, source_type, source_id, occurred_at));
            }
        }
        drop(statement);
        drop(connection);

        let stale_after = Duration::seconds(stale_after_seconds);
        latest_by_project
            .into_iter()
            .filter(|(_, (latest, _, _, _))| as_of.signed_duration_since(*latest) >= stale_after)
            .map(
                |(project_id, (latest, source_type, source_id, occurred_at))| {
                    let stale_window =
                        as_of.signed_duration_since(latest).num_seconds() / stale_after_seconds;
                    self.create_proactive_candidate(NewProactiveCandidate {
                        owner_id: owner_id.trim().into(),
                        subscription_id: None,
                        project_id: Some(project_id.clone()),
                        reason: "Project has had no recent progress".into(),
                        evidence: serde_json::json!([{
                            "source_type": source_type,
                            "source_id": source_id,
                            "occurred_at": occurred_at,
                        }]),
                        priority: "normal".into(),
                        confidence: 0.8,
                        expires_at: as_of
                            .checked_add_signed(stale_after)
                            .ok_or_else(|| {
                                StoreError::InvalidInput(
                                    "candidate expiration is out of range".into(),
                                )
                            })?
                            .to_rfc3339(),
                        deduplication_key: format!("stale-project:{project_id}:{stale_window}"),
                        provenance: "detector://stale-project/v1".into(),
                    })
                },
            )
            .collect()
    }

    pub fn proactive_candidates(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<ProactiveCandidate>, StoreError> {
        let connection = self.connection()?;
        let mut statement=connection.prepare("SELECT candidate_id FROM proactive_candidates WHERE owner_id=?1 ORDER BY created_at DESC LIMIT ?2")?;
        statement
            .query_map(params![owner_id.trim(), limit.clamp(1, 200)], |row| {
                row.get::<_, String>(0)
            })?
            .map(|id| candidate(&connection, &id?).map_err(Into::into))
            .collect()
    }
    pub fn proactive_candidate(
        &self,
        owner_id: &str,
        id: &str,
    ) -> Result<Option<ProactiveCandidate>, StoreError> {
        let connection = self.connection()?;
        if !owned(
            &connection,
            "proactive_candidates",
            "candidate_id",
            owner_id.trim(),
            id.trim(),
        )? {
            return Ok(None);
        }
        Ok(Some(candidate(&connection, id)?))
    }

    /// Drafts a review-only proposal through an injected local boundary. This path
    /// never invokes a provider itself and never creates a delivery.
    pub fn draft_proactive_proposal(
        &self,
        owner_id: &str,
        candidate_id: &str,
        drafter: &dyn ProactiveDraftingContract,
    ) -> Result<OutreachProposal, StoreError> {
        let candidate = self
            .proactive_candidate(owner_id, candidate_id)?
            .ok_or_else(|| StoreError::InvalidInput("candidate is not owner-scoped".into()))?;
        let input = ProactiveDraftingInput {
            owner_id: candidate.owner_id.clone(),
            candidate_id: candidate.id.clone(),
            project_id: candidate.project_id.clone(),
            reason: candidate.reason.clone(),
            priority: candidate.priority.clone(),
            candidate_confidence: candidate.confidence,
            evidence_ids: evidence_ids(&candidate.evidence),
            candidate_expires_at: candidate.expires_at.clone(),
        };
        let original_draft = drafter.draft(&input).map_err(StoreError::InvalidInput)?;
        let output: BoundedDraftOutput = serde_json::from_str(&original_draft).map_err(|_| {
            StoreError::InvalidInput("draft output must be valid structured JSON".into())
        })?;
        validate_draft_output(&candidate, &output)?;
        self.create_outreach_proposal(NewOutreachProposal {
            owner_id: candidate.owner_id,
            candidate_id: candidate.id,
            original_draft,
            editable_draft: output.message,
            channel: "internal_queue".into(),
            approval_state: "pending_review".into(),
            risk_class: output.risk_class,
            delivery_deadline: Some(output.expires_at),
            provenance: "draft://bounded-local/v1".into(),
        })
    }

    pub fn create_outreach_proposal(
        &self,
        input: NewOutreachProposal,
    ) -> Result<OutreachProposal, StoreError> {
        validate(&[
            (&input.owner_id, "owner_id"),
            (&input.candidate_id, "candidate_id"),
            (&input.original_draft, "original_draft"),
            (&input.editable_draft, "editable_draft"),
            (&input.channel, "channel"),
            (&input.approval_state, "approval_state"),
            (&input.risk_class, "risk_class"),
            (&input.provenance, "provenance"),
        ])?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, &input.owner_id, &now)?;
        if !owned(
            &connection,
            "proactive_candidates",
            "candidate_id",
            input.owner_id.trim(),
            input.candidate_id.trim(),
        )? {
            return Err(StoreError::InvalidInput(
                "candidate is not owner-scoped".into(),
            ));
        }
        let candidate = candidate(&connection, input.candidate_id.trim())?;
        validate_proposal_input(&candidate, &input)?;
        connection.execute("INSERT INTO outreach_proposals(proposal_id,owner_id,candidate_id,original_draft,editable_draft,channel,approval_state,risk_class,delivery_deadline,provenance,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",params![id,input.owner_id.trim(),input.candidate_id.trim(),input.original_draft.trim(),input.editable_draft.trim(),input.channel.trim(),input.approval_state.trim(),input.risk_class.trim(),input.delivery_deadline.as_deref().map(str::trim),input.provenance.trim(),now])?;
        proposal(&connection, &id).map_err(Into::into)
    }
    pub fn outreach_proposal(
        &self,
        owner_id: &str,
        id: &str,
    ) -> Result<Option<OutreachProposal>, StoreError> {
        let connection = self.connection()?;
        if !owned(
            &connection,
            "outreach_proposals",
            "proposal_id",
            owner_id.trim(),
            id.trim(),
        )? {
            return Ok(None);
        }
        Ok(Some(proposal(&connection, id)?))
    }

    pub fn create_outreach_delivery(
        &self,
        input: NewOutreachDelivery,
    ) -> Result<OutreachDelivery, StoreError> {
        validate(&[
            (&input.owner_id, "owner_id"),
            (&input.proposal_id, "proposal_id"),
            (&input.provider, "provider"),
            (&input.channel, "channel"),
            (&input.result, "result"),
            (&input.idempotency_key, "idempotency_key"),
            (&input.provenance, "provenance"),
        ])?;
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        ensure_owner(&connection, &input.owner_id, &now)?;
        if !owned(
            &connection,
            "outreach_proposals",
            "proposal_id",
            input.owner_id.trim(),
            input.proposal_id.trim(),
        )? {
            return Err(StoreError::InvalidInput(
                "proposal is not owner-scoped".into(),
            ));
        }
        let state: String = connection.query_row(
            "SELECT approval_state FROM outreach_proposals WHERE proposal_id=?1",
            [input.proposal_id.trim()],
            |r| r.get(0),
        )?;
        if state != "approved" {
            return Err(StoreError::InvalidInput(
                "proposal must be approved before a delivery record".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO outreach_deliveries(delivery_id,owner_id,proposal_id,provider,channel,result,idempotency_key,response_link,provenance,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(owner_id,idempotency_key) DO NOTHING",
            params![id,input.owner_id.trim(),input.proposal_id.trim(),input.provider.trim(),input.channel.trim(),input.result.trim(),input.idempotency_key.trim(),input.response_link.as_deref().map(str::trim),input.provenance.trim(),now],
        )?;
        let canonical_id: String = connection.query_row(
            "SELECT delivery_id FROM outreach_deliveries WHERE owner_id=?1 AND idempotency_key=?2",
            params![input.owner_id.trim(), input.idempotency_key.trim()],
            |row| row.get(0),
        )?;
        delivery(&connection, &canonical_id).map_err(Into::into)
    }
    pub fn outreach_delivery(
        &self,
        owner_id: &str,
        id: &str,
    ) -> Result<Option<OutreachDelivery>, StoreError> {
        let connection = self.connection()?;
        if !owned(
            &connection,
            "outreach_deliveries",
            "delivery_id",
            owner_id.trim(),
            id.trim(),
        )? {
            return Ok(None);
        }
        Ok(Some(delivery(&connection, id)?))
    }

    pub fn create_proactive_feedback(
        &self,
        input: NewProactiveFeedback,
    ) -> Result<ProactiveFeedback, StoreError> {
        validate(&[
            (&input.owner_id, "owner_id"),
            (&input.action, "action"),
            (&input.provenance, "provenance"),
        ])?;
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_owner(&connection, &input.owner_id, &now)?;
        require_owned_optional(
            &connection,
            "outreach_proposals",
            "proposal_id",
            input.proposal_id.as_deref(),
            input.owner_id.trim(),
            "proposal",
        )?;
        connection.execute("INSERT INTO proactive_feedback(feedback_id,owner_id,proposal_id,action,note,provenance,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,input.owner_id.trim(),input.proposal_id.as_deref().map(str::trim),input.action.trim(),input.note.as_deref().map(str::trim),input.provenance.trim(),now])?;
        feedback(&connection, &id).map_err(Into::into)
    }
    pub fn proactive_feedback(
        &self,
        owner_id: &str,
        id: &str,
    ) -> Result<Option<ProactiveFeedback>, StoreError> {
        let connection = self.connection()?;
        if !owned(
            &connection,
            "proactive_feedback",
            "feedback_id",
            owner_id.trim(),
            id.trim(),
        )? {
            return Ok(None);
        }
        Ok(Some(feedback(&connection, id)?))
    }
}

fn validate_proposal_input(
    candidate: &ProactiveCandidate,
    input: &NewOutreachProposal,
) -> Result<(), StoreError> {
    if input.channel.trim() != "internal_queue" || input.approval_state.trim() != "pending_review" {
        return Err(StoreError::InvalidInput(
            "proposal must use the internal pending review queue".into(),
        ));
    }
    let original_is_safe = is_owner_review_question(&input.original_draft)
        || serde_json::from_str::<BoundedDraftOutput>(&input.original_draft)
            .ok()
            .is_some_and(|output| validate_draft_output(candidate, &output).is_ok());
    if !original_is_safe || !is_owner_review_question(&input.editable_draft) {
        return Err(StoreError::InvalidInput(
            "proposal drafts must be owner review questions".into(),
        ));
    }
    let now = Utc::now();
    let candidate_expiry = DateTime::parse_from_rfc3339(&candidate.expires_at)
        .map_err(|_| StoreError::InvalidInput("candidate expiration must be RFC 3339".into()))?
        .with_timezone(&Utc);
    if candidate_expiry <= now {
        return Err(StoreError::InvalidInput(
            "candidate expiration must be in the future".into(),
        ));
    }
    let deadline = input
        .delivery_deadline
        .as_deref()
        .ok_or_else(|| StoreError::InvalidInput("delivery deadline is required".into()))?;
    let deadline = DateTime::parse_from_rfc3339(deadline.trim())
        .map_err(|_| StoreError::InvalidInput("delivery deadline must be RFC 3339".into()))?
        .with_timezone(&Utc);
    if deadline <= now {
        return Err(StoreError::InvalidInput(
            "delivery deadline must be in the future".into(),
        ));
    }
    if deadline > candidate_expiry {
        return Err(StoreError::InvalidInput(
            "delivery deadline must not exceed candidate expiration".into(),
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundedDraftOutput {
    owner_id: String,
    project_id: Option<String>,
    confidence: f64,
    rationale: String,
    message: String,
    reason_category: String,
    urgency: String,
    risk_class: String,
    approval_state: String,
    evidence_ids: Vec<String>,
    expires_at: String,
}

const MAX_DRAFT_FIELD_CHARS: usize = 1_000;

fn validate_draft_output(
    candidate: &ProactiveCandidate,
    output: &BoundedDraftOutput,
) -> Result<(), StoreError> {
    if output.owner_id != candidate.owner_id || output.project_id != candidate.project_id {
        return Err(StoreError::InvalidInput(
            "draft scope must match its candidate".into(),
        ));
    }
    if !output.confidence.is_finite() || !(0.0..=1.0).contains(&output.confidence) {
        return Err(StoreError::InvalidInput(
            "draft confidence is invalid".into(),
        ));
    }
    if output.rationale.trim().is_empty()
        || output.message.trim().is_empty()
        || output.rationale.chars().count() > MAX_DRAFT_FIELD_CHARS
        || output.message.chars().count() > MAX_DRAFT_FIELD_CHARS
        || !is_review_rationale(&output.rationale)
        || !is_owner_review_question(&output.message)
    {
        return Err(StoreError::InvalidInput(
            "draft text must be a bounded evidence rationale and owner review question".into(),
        ));
    }
    if !matches!(output.reason_category.as_str(), "stale_project")
        || !matches!(output.urgency.as_str(), "low" | "normal" | "high")
        || !matches!(output.risk_class.as_str(), "low" | "normal" | "high")
        || output.approval_state != "pending_review"
    {
        return Err(StoreError::InvalidInput(
            "draft category or review state is invalid".into(),
        ));
    }
    let allowed_ids = evidence_ids(&candidate.evidence);
    if output.evidence_ids.is_empty()
        || output.evidence_ids.iter().any(|id| id.trim().is_empty())
        || output
            .evidence_ids
            .iter()
            .any(|id| !allowed_ids.contains(id))
    {
        return Err(StoreError::InvalidInput(
            "draft evidence is not candidate-scoped".into(),
        ));
    }
    let now = Utc::now();
    let candidate_expiry = DateTime::parse_from_rfc3339(&candidate.expires_at)
        .map_err(|_| StoreError::InvalidInput("candidate expiration must be RFC 3339".into()))?
        .with_timezone(&Utc);
    if candidate_expiry <= now {
        return Err(StoreError::InvalidInput(
            "candidate expiration must be in the future".into(),
        ));
    }
    let expiry = DateTime::parse_from_rfc3339(&output.expires_at)
        .map_err(|_| StoreError::InvalidInput("draft expiration must be RFC 3339".into()))?
        .with_timezone(&Utc);
    if expiry <= now {
        return Err(StoreError::InvalidInput(
            "draft expiration must be in the future".into(),
        ));
    }
    if expiry > candidate_expiry {
        return Err(StoreError::InvalidInput(
            "draft expiration must not exceed candidate expiration".into(),
        ));
    }
    Ok(())
}

fn evidence_ids(evidence: &serde_json::Value) -> Vec<String> {
    evidence
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_object())
        .filter_map(|item| {
            ["evidence_id", "event_id", "source_id"]
                .iter()
                .find_map(|key| item.get(*key).and_then(serde_json::Value::as_str))
        })
        .map(str::to_owned)
        .collect()
}

/// Review drafts are deliberately a small language, rather than a blacklist of
/// dangerous verbs. This makes newly phrased side effects fail closed.
fn is_review_rationale(value: &str) -> bool {
    let rationale = value.trim();
    rationale.strip_suffix('.').is_some_and(|prefix| {
        prefix
            .to_ascii_lowercase()
            .starts_with("the approved local evidence ")
    })
}

/// Review drafts use a closed, declarative question grammar.  In particular,
/// the subject is a positive vocabulary of project-review terms, rather than a
/// free-form clause that tries to blacklist potentially dangerous verbs.
fn is_owner_review_question(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    let Some(subject) = ["would you like to review ", "do you want to review "]
        .iter()
        .find_map(|template| normalized.strip_prefix(template))
        .and_then(|subject| subject.strip_suffix('?'))
    else {
        return false;
    };

    is_safe_review_subject(subject)
}

fn is_safe_review_subject(subject: &str) -> bool {
    if subject.is_empty()
        || subject.starts_with(' ')
        || subject.ends_with(' ')
        || subject.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, ' ' | '-' | '\'')
        })
    {
        return false;
    }

    const SUBJECT_TERMS: &[&str] = &[
        "active",
        "approved",
        "backlog",
        "current",
        "local",
        "my",
        "open",
        "plan",
        "priorities",
        "priority",
        "project",
        "status",
        "task",
        "tasks",
        "the",
        "this",
        "timeline",
        "today",
    ];
    subject.split(' ').all(|term| {
        !term.is_empty()
            && (term.chars().all(|character| character.is_ascii_digit())
                || SUBJECT_TERMS.contains(&term))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateEvidenceRecord {
    source_type: String,
    source_id: Option<String>,
    event_id: Option<i64>,
    occurred_at: Option<String>,
}

fn validate_candidate_evidence(
    connection: &rusqlite::Connection,
    owner_id: &str,
    candidate_project_id: Option<&str>,
    evidence: &serde_json::Value,
) -> Result<(), StoreError> {
    let records: Vec<CandidateEvidenceRecord> =
        serde_json::from_value(evidence.clone()).map_err(|_| {
            StoreError::InvalidInput(
                "candidate evidence must contain only typed source references".into(),
            )
        })?;
    for record in records {
        let source_id = match (record.source_id.as_deref(), record.event_id) {
            (Some(id), None) if !id.trim().is_empty() => id.trim().to_owned(),
            (None, Some(event_id)) if record.source_type == "execution_event" && event_id > 0 => {
                event_id.to_string()
            }
            _ => {
                return Err(StoreError::InvalidInput(
                    "candidate evidence requires exactly one source identifier".into(),
                ));
            }
        };
        let exists = match record.source_type.as_str() {
            "project" => {
                candidate_project_id.is_none_or(|project_id| project_id == source_id)
                    && owned(connection, "projects", "project_id", owner_id, &source_id)?
            }
            "task" => {
                if let Some(project_id) = candidate_project_id {
                    connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM tasks WHERE task_id=?1 AND owner_id=?2 AND project_id=?3)",
                        params![source_id, owner_id, project_id],
                        |row| row.get(0),
                    )?
                } else {
                    owned(connection, "tasks", "task_id", owner_id, &source_id)?
                }
            }
            "execution_event" => {
                if let Some(project_id) = candidate_project_id {
                    connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM execution_events e WHERE e.event_id=?1 AND e.owner_id=?2 AND (e.stream_id=?3 OR EXISTS(SELECT 1 FROM tasks t WHERE t.task_id=e.stream_id AND t.owner_id=e.owner_id AND t.project_id=?3)))",
                        params![source_id, owner_id, project_id],
                        |row| row.get(0),
                    )?
                } else {
                    owned(
                        connection,
                        "execution_events",
                        "event_id",
                        owner_id,
                        &source_id,
                    )?
                }
            }
            _ => false,
        };
        if !exists {
            return Err(StoreError::InvalidInput(
                "candidate evidence is not an owner-scoped authoritative source".into(),
            ));
        }
        let _ = record.occurred_at;
    }
    Ok(())
}

fn validate(values: &[(&str, &str)]) -> Result<(), StoreError> {
    for (value, label) in values {
        if value.trim().is_empty() {
            return Err(StoreError::InvalidInput(format!("{label} is required")));
        }
    }
    Ok(())
}
fn ensure_owner(c: &rusqlite::Connection, owner: &str, now: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO NOTHING",params![owner.trim(),now])?;
    Ok(())
}
fn owned(
    c: &rusqlite::Connection,
    table: &str,
    id_column: &str,
    owner: &str,
    id: &str,
) -> rusqlite::Result<bool> {
    c.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {id_column}=?1 AND owner_id=?2)"),
        params![id, owner],
        |r| r.get(0),
    )
}
fn require_owned_optional(
    c: &rusqlite::Connection,
    table: &str,
    id_column: &str,
    id: Option<&str>,
    owner: &str,
    label: &str,
) -> Result<(), StoreError> {
    if let Some(id) = id {
        if !owned(c, table, id_column, owner, id)? {
            return Err(StoreError::InvalidInput(format!(
                "{label} is not owner-scoped"
            )));
        }
    }
    Ok(())
}
fn subscription(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<ProactiveSubscription> {
    c.query_row("SELECT subscription_id,owner_id,topic,project_id,source_type,cadence,quiet_hours,status,provenance,created_at,updated_at FROM proactive_subscriptions WHERE subscription_id=?1",[id],|r|Ok(ProactiveSubscription{id:r.get(0)?,owner_id:r.get(1)?,topic:r.get(2)?,project_id:r.get(3)?,source_type:r.get(4)?,cadence:r.get(5)?,quiet_hours:r.get(6)?,status:r.get(7)?,provenance:r.get(8)?,created_at:r.get(9)?,updated_at:r.get(10)?}))
}
fn candidate(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<ProactiveCandidate> {
    c.query_row("SELECT candidate_id,owner_id,subscription_id,project_id,reason,evidence_json,priority,confidence,expires_at,deduplication_key,provenance,created_at FROM proactive_candidates WHERE candidate_id=?1",[id],|r|{let e:String=r.get(5)?;Ok(ProactiveCandidate{id:r.get(0)?,owner_id:r.get(1)?,subscription_id:r.get(2)?,project_id:r.get(3)?,reason:r.get(4)?,evidence:serde_json::from_str(&e).map_err(|x|rusqlite::Error::FromSqlConversionFailure(5,rusqlite::types::Type::Text,Box::new(x)))?,priority:r.get(6)?,confidence:r.get(7)?,expires_at:r.get(8)?,deduplication_key:r.get(9)?,provenance:r.get(10)?,created_at:r.get(11)?})})
}
fn proposal(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<OutreachProposal> {
    c.query_row("SELECT proposal_id,owner_id,candidate_id,original_draft,editable_draft,channel,approval_state,risk_class,delivery_deadline,provenance,created_at,updated_at FROM outreach_proposals WHERE proposal_id=?1",[id],|r|Ok(OutreachProposal{id:r.get(0)?,owner_id:r.get(1)?,candidate_id:r.get(2)?,original_draft:r.get(3)?,editable_draft:r.get(4)?,channel:r.get(5)?,approval_state:r.get(6)?,risk_class:r.get(7)?,delivery_deadline:r.get(8)?,provenance:r.get(9)?,created_at:r.get(10)?,updated_at:r.get(11)?}))
}
fn delivery(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<OutreachDelivery> {
    c.query_row("SELECT delivery_id,owner_id,proposal_id,provider,channel,result,idempotency_key,response_link,provenance,created_at FROM outreach_deliveries WHERE delivery_id=?1",[id],|r|Ok(OutreachDelivery{id:r.get(0)?,owner_id:r.get(1)?,proposal_id:r.get(2)?,provider:r.get(3)?,channel:r.get(4)?,result:r.get(5)?,idempotency_key:r.get(6)?,response_link:r.get(7)?,provenance:r.get(8)?,created_at:r.get(9)?}))
}
fn feedback(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<ProactiveFeedback> {
    c.query_row("SELECT feedback_id,owner_id,proposal_id,action,note,provenance,created_at FROM proactive_feedback WHERE feedback_id=?1",[id],|r|Ok(ProactiveFeedback{id:r.get(0)?,owner_id:r.get(1)?,proposal_id:r.get(2)?,action:r.get(3)?,note:r.get(4)?,provenance:r.get(5)?,created_at:r.get(6)?}))
}
