use crate::model::{
    CaptureProposal, CaptureSource, DailyFocusReset, NewCaptureProposal, NewPersonalCapture,
    PersonalCapture, PersonalExtractionContract, PersonalExtractionInput, PersonalReviewRecord,
    ReviewDecision, TaskApprovalStatus, TaskRecord,
};
use crate::{ConversationStore, StoreError};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use uuid::Uuid;

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidInput("timestamp must be RFC3339".into()))
}

fn validate_active_record(
    owner_id: &str,
    text: &str,
    created_at: &str,
    expires_at: &str,
) -> Result<(), StoreError> {
    if owner_id.trim().is_empty() || text.trim().is_empty() {
        return Err(StoreError::InvalidInput(
            "owner and text are required".into(),
        ));
    }
    let created_at = parse_timestamp(created_at)?;
    let expires_at = parse_timestamp(expires_at)?;
    if created_at >= expires_at || expires_at <= Utc::now() {
        return Err(StoreError::InvalidInput(
            "record must not be expired".into(),
        ));
    }
    Ok(())
}

fn validate_proposal(proposal: &NewCaptureProposal) -> Result<(), StoreError> {
    if proposal.owner_id.trim().is_empty()
        || proposal.capture_id.trim().is_empty()
        || proposal.title.trim().is_empty()
        || proposal.rationale.trim().is_empty()
        || proposal.audit_id.trim().is_empty()
        || !matches!(
            proposal.category.as_str(),
            "task" | "appointment" | "worry" | "idea" | "note"
        )
    {
        return Err(StoreError::InvalidInput("invalid proposal".into()));
    }
    if parse_timestamp(&proposal.expires_at)? <= Utc::now() {
        return Err(StoreError::InvalidInput(
            "record must not be expired".into(),
        ));
    }
    Ok(())
}

fn ensure_owner(
    connection: &rusqlite::Connection,
    owner_id: &str,
    at: &str,
) -> Result<(), StoreError> {
    connection.execute(
        "INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) \
         ON CONFLICT(owner_id) DO NOTHING",
        params![owner_id, at],
    )?;
    Ok(())
}

fn audit(
    connection: &rusqlite::Connection,
    owner_id: &str,
    audit_id: &str,
    event_type: &str,
    record_id: &str,
    at: &str,
) -> Result<(), StoreError> {
    if audit_id.trim().is_empty() {
        return Err(StoreError::InvalidInput("audit id is required".into()));
    }
    connection.execute(
        "INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) \
         VALUES(?1,?2,?3,'voiceos-core',?4,?5)",
        params![owner_id, audit_id, event_type, serde_json::json!({"record_id": record_id}).to_string(), at],
    )?;
    Ok(())
}

fn close_capture_after_review(
    connection: &rusqlite::Connection,
    owner_id: &str,
    capture_id: &str,
    now: &str,
) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE personal_captures SET status='approved' \
         WHERE capture_id=?1 AND owner_id=?2 AND status='reviewing' \
         AND NOT EXISTS(SELECT 1 FROM capture_proposals \
             WHERE capture_id=?1 AND owner_id=?2 AND status='reviewing' AND expires_at>?3)",
        params![capture_id, owner_id, now],
    )?;
    Ok(())
}

fn normalize_display_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractionOutput {
    owner_id: String,
    capture_id: String,
    candidates: Vec<ExtractionCandidate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractionCandidate {
    category: String,
    confidence: f64,
    title: String,
    details: Option<String>,
    suggested_next_action: String,
    rationale: String,
    evidence_capture_ids: Vec<String>,
    expires_at: String,
}

fn contains_forbidden_instruction(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("http://")
        || value.contains("https://")
        || value.contains("www.")
        || [
            "email ",
            "send ",
            "post ",
            "publish ",
            "delete ",
            "destroy ",
            "execute ",
            "deploy ",
            "transfer ",
            "invite ",
            "approve ",
            "schedule ",
            "book ",
            "create ",
            "mutate ",
            "update ",
        ]
        .iter()
        .any(|phrase| value.contains(phrase))
}

fn validate_extraction(
    capture: &PersonalCapture,
    output: &ExtractionOutput,
) -> Result<(), StoreError> {
    if output.owner_id != capture.owner_id || output.capture_id != capture.id {
        return Err(StoreError::InvalidInput(
            "extraction output is not owner and capture scoped".into(),
        ));
    }
    if output.candidates.len() > 8 {
        return Err(StoreError::InvalidInput(
            "extraction may contain at most eight candidates".into(),
        ));
    }
    let capture_expiry = parse_timestamp(&capture.expires_at)?;
    if capture_expiry <= Utc::now() {
        return Err(StoreError::InvalidInput("capture is expired".into()));
    }
    for candidate in &output.candidates {
        if !matches!(
            candidate.category.as_str(),
            "task" | "appointment" | "worry" | "idea" | "note"
        ) || !(0.0..=1.0).contains(&candidate.confidence)
            || candidate.title.trim().is_empty()
            || candidate.suggested_next_action.trim().is_empty()
            || candidate.rationale.trim().is_empty()
            || candidate.evidence_capture_ids != [capture.id.clone()]
        {
            return Err(StoreError::InvalidInput(
                "invalid extraction candidate".into(),
            ));
        }
        let expiry = parse_timestamp(&candidate.expires_at)?;
        if expiry <= Utc::now() || expiry > capture_expiry {
            return Err(StoreError::InvalidInput(
                "candidate expiry must be active and bounded by capture expiry".into(),
            ));
        }
        let text = format!(
            "{} {} {} {}",
            candidate.title,
            candidate.details.as_deref().unwrap_or_default(),
            candidate.suggested_next_action,
            candidate.rationale
        );
        if contains_forbidden_instruction(&text) {
            return Err(StoreError::InvalidInput(
                "extraction contains unsafe instruction or URL".into(),
            ));
        }
    }
    Ok(())
}

fn personal_capture_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersonalCapture> {
    let structured_content = row
        .get::<_, Option<String>>(6)?
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(PersonalCapture {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        source: row.get(2)?,
        source_id: row.get(3)?,
        raw_content: row.get(4)?,
        display_text: row.get(5)?,
        structured_content,
        status: row.get(7)?,
        created_at: row.get(8)?,
        expires_at: row.get(9)?,
        audit_id: row.get(10)?,
    })
}

fn capture_proposal_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CaptureProposal> {
    let evidence_capture_ids =
        serde_json::from_str(&row.get::<_, String>(9)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(CaptureProposal {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        capture_id: row.get(2)?,
        title: row.get(3)?,
        category: row.get(4)?,
        confidence: row.get(5)?,
        details: row.get(6)?,
        suggested_next_action: row.get(7)?,
        rationale: row.get(8)?,
        evidence_capture_ids,
        status: row.get(10)?,
        created_at: row.get(11)?,
        expires_at: row.get(12)?,
        audit_id: row.get(13)?,
    })
}

impl ConversationStore {
    pub fn capture_personal_input(
        &self,
        owner_id: &str,
        source: CaptureSource,
        text: &str,
        occurred_at: DateTime<Utc>,
        retention: Duration,
    ) -> Result<PersonalCapture, StoreError> {
        self.capture_personal_input_with_audit(
            owner_id,
            source,
            text,
            occurred_at,
            retention,
            format!("personal-capture:{}", Uuid::new_v4()),
        )
    }

    pub fn capture_personal_input_as(
        &self,
        owner_id: &str,
        source: CaptureSource,
        text: &str,
        occurred_at: DateTime<Utc>,
        retention: Duration,
        actor: &str,
    ) -> Result<PersonalCapture, StoreError> {
        if actor.trim().is_empty() {
            return Err(StoreError::InvalidInput("capture actor is required".into()));
        }
        self.capture_personal_input_with_audit(
            owner_id,
            source,
            text,
            occurred_at,
            retention,
            format!(
                "device:{}:personal-capture:{}",
                actor.trim(),
                Uuid::new_v4()
            ),
        )
    }

    fn capture_personal_input_with_audit(
        &self,
        owner_id: &str,
        source: CaptureSource,
        text: &str,
        occurred_at: DateTime<Utc>,
        retention: Duration,
        audit_id: String,
    ) -> Result<PersonalCapture, StoreError> {
        if source.kind.trim().is_empty()
            || source.id.trim().is_empty()
            || retention <= Duration::zero()
        {
            return Err(StoreError::InvalidInput(
                "capture source and positive retention are required".into(),
            ));
        }
        let now = Utc::now();
        let created_at = occurred_at.to_rfc3339();
        let expires_at = (now + retention).to_rfc3339();
        if let Some(existing) =
            self.personal_capture_by_source(owner_id, &source.kind, &source.id)?
        {
            return Ok(existing);
        }
        self.create_personal_capture(NewPersonalCapture {
            owner_id: owner_id.into(),
            source: source.kind,
            source_id: source.id,
            raw_content: text.into(),
            structured_content: None,
            created_at,
            expires_at,
            audit_id,
        })
    }

    pub fn personal_inbox(&self, owner_id: &str) -> Result<Vec<PersonalCapture>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT capture_id,owner_id,source,source_id,raw_content,display_text,structured_content_json,status,created_at,expires_at,audit_id \
             FROM personal_captures WHERE owner_id=?1 AND status IN ('received','reviewing') \
             AND expires_at>?2 ORDER BY created_at DESC, capture_id DESC",
        )?;
        statement
            .query_map(
                params![owner_id, Utc::now().to_rfc3339()],
                personal_capture_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn downstream_record_counts(
        &self,
        owner_id: &str,
    ) -> Result<(usize, usize, usize, usize, usize), StoreError> {
        let connection = self.connection()?;
        let count = |table: &str| {
            connection.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE owner_id=?1"),
                [owner_id],
                |row| row.get::<_, i64>(0),
            )
        };
        Ok((
            count("tasks")? as usize,
            count("memories")? as usize,
            count("focus_sessions")? as usize,
            count("outreach_deliveries")? as usize,
            count("outreach_events")? as usize,
        ))
    }

    fn personal_capture_by_source(
        &self,
        owner_id: &str,
        source: &str,
        source_id: &str,
    ) -> Result<Option<PersonalCapture>, StoreError> {
        let connection = self.connection()?;
        connection.query_row(
            "SELECT capture_id,owner_id,source,source_id,raw_content,display_text,structured_content_json,status,created_at,expires_at,audit_id \
             FROM personal_captures WHERE owner_id=?1 AND source=?2 AND source_id=?3 AND expires_at>?4",
            params![owner_id, source, source_id, Utc::now().to_rfc3339()],
            personal_capture_from_row,
        ).optional().map_err(StoreError::from)
    }

    pub fn create_personal_capture(
        &self,
        capture: NewPersonalCapture,
    ) -> Result<PersonalCapture, StoreError> {
        validate_active_record(
            &capture.owner_id,
            &capture.raw_content,
            &capture.created_at,
            &capture.expires_at,
        )?;
        if capture.source.trim().is_empty()
            || capture.source_id.trim().is_empty()
            || capture.audit_id.trim().is_empty()
        {
            return Err(StoreError::InvalidInput(
                "capture metadata is required".into(),
            ));
        }

        let connection = self.connection()?;
        ensure_owner(&connection, &capture.owner_id, &capture.created_at)?;
        let id = Uuid::new_v4().to_string();
        let structured_content = capture
            .structured_content
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let display_text = normalize_display_text(&capture.raw_content);
        connection.execute(
            "INSERT INTO personal_captures(\
                capture_id,owner_id,source,source_id,raw_content,display_text,structured_content_json,status,created_at,expires_at,audit_id\
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,'received',?8,?9,?10)",
            params![
                id,
                capture.owner_id,
                capture.source,
                capture.source_id,
                capture.raw_content,
                display_text,
                structured_content,
                capture.created_at,
                capture.expires_at,
                capture.audit_id,
            ],
        )?;
        audit(
            &connection,
            &capture.owner_id,
            &capture.audit_id,
            "personal.capture_received",
            &id,
            &capture.created_at,
        )?;

        Ok(PersonalCapture {
            id,
            owner_id: capture.owner_id,
            source: capture.source,
            source_id: capture.source_id,
            raw_content: capture.raw_content,
            display_text,
            structured_content: capture.structured_content,
            status: "received".into(),
            created_at: capture.created_at,
            expires_at: capture.expires_at,
            audit_id: capture.audit_id,
        })
    }

    pub fn personal_capture(
        &self,
        owner_id: &str,
        capture_id: &str,
    ) -> Result<Option<PersonalCapture>, StoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT capture_id,owner_id,source,source_id,raw_content,display_text,structured_content_json,status,created_at,expires_at,audit_id \
                 FROM personal_captures WHERE owner_id=?1 AND capture_id=?2 AND expires_at>?3",
                params![owner_id, capture_id, Utc::now().to_rfc3339()],
                personal_capture_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn decide_personal_capture(
        &self,
        owner_id: &str,
        capture_id: &str,
        status: &str,
        audit_id: &str,
    ) -> Result<ReviewDecision, StoreError> {
        if owner_id.trim().is_empty()
            || capture_id.trim().is_empty()
            || audit_id.trim().is_empty()
            || !matches!(status, "approved" | "rejected" | "discarded")
        {
            return Err(StoreError::InvalidInput("invalid capture decision".into()));
        }
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
        if connection.execute(
            "UPDATE personal_captures SET status=?1 WHERE capture_id=?2 AND owner_id=?3 \
             AND status IN ('received','reviewing') AND expires_at>?4",
            params![status, capture_id, owner_id, now],
        )? != 1
        {
            return Err(StoreError::InvalidInput(
                "capture missing, cross-owner, expired, or already decided".into(),
            ));
        }
        audit(
            &connection,
            owner_id,
            audit_id,
            "personal.capture_decided",
            capture_id,
            &now,
        )?;
        Ok(ReviewDecision {
            id: capture_id.into(),
            owner_id: owner_id.into(),
            status: status.into(),
            audit_id: audit_id.into(),
        })
    }

    pub fn extract_personal_capture(
        &self,
        owner_id: &str,
        capture_id: &str,
        extractor: &dyn PersonalExtractionContract,
    ) -> Result<Vec<CaptureProposal>, StoreError> {
        let capture = self
            .personal_capture(owner_id, capture_id)?
            .ok_or_else(|| {
                StoreError::InvalidInput("capture is not owner-scoped or is expired".into())
            })?;
        if !matches!(capture.status.as_str(), "received" | "reviewing") {
            return Err(StoreError::InvalidInput(
                "capture is already decided and cannot be extracted".into(),
            ));
        }
        let input = PersonalExtractionInput {
            owner_id: capture.owner_id.clone(),
            capture_id: capture.id.clone(),
            raw_content: capture.raw_content.clone(),
            display_text: capture.display_text.clone(),
            capture_expires_at: capture.expires_at.clone(),
        };
        let original_output = extractor
            .extract(&input)
            .map_err(StoreError::InvalidInput)?;
        let output: ExtractionOutput = serde_json::from_str(&original_output).map_err(|_| {
            StoreError::InvalidInput("extraction output must be valid structured JSON".into())
        })?;
        validate_extraction(&capture, &output)?;

        let connection = self.connection()?;
        let created_at = Utc::now().to_rfc3339();
        let transaction = connection.unchecked_transaction()?;
        let mut proposals = Vec::with_capacity(output.candidates.len());
        for candidate in output.candidates {
            let id = Uuid::new_v4().to_string();
            let audit_id = format!("personal-extraction:{id}");
            transaction.execute(
                "INSERT INTO capture_proposals(\
                    proposal_id,owner_id,capture_id,title,category,confidence,details,suggested_next_action,\
                    rationale,evidence_capture_ids_json,status,created_at,expires_at,audit_id\
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'reviewing',?11,?12,?13)",
                params![
                    id,
                    capture.owner_id,
                    capture.id,
                    candidate.title,
                    candidate.category,
                    candidate.confidence,
                    candidate.details,
                    candidate.suggested_next_action,
                    candidate.rationale,
                    serde_json::to_string(&candidate.evidence_capture_ids)?,
                    created_at,
                    candidate.expires_at,
                    audit_id,
                ],
            )?;
            audit(
                &transaction,
                &capture.owner_id,
                &audit_id,
                "personal.extraction_proposed",
                &id,
                &created_at,
            )?;
            proposals.push(CaptureProposal {
                id,
                owner_id: capture.owner_id.clone(),
                capture_id: capture.id.clone(),
                title: candidate.title,
                category: candidate.category,
                confidence: candidate.confidence,
                details: candidate.details,
                suggested_next_action: candidate.suggested_next_action,
                rationale: candidate.rationale,
                evidence_capture_ids: candidate.evidence_capture_ids,
                status: "reviewing".into(),
                created_at: created_at.clone(),
                expires_at: candidate.expires_at,
                audit_id,
            });
        }
        transaction.execute(
            "UPDATE personal_captures SET status='reviewing' \
             WHERE capture_id=?1 AND owner_id=?2 AND status IN ('received','reviewing')",
            params![capture.id, capture.owner_id],
        )?;
        transaction.commit()?;
        Ok(proposals)
    }

    pub fn create_capture_proposal(
        &self,
        proposal: NewCaptureProposal,
    ) -> Result<CaptureProposal, StoreError> {
        validate_proposal(&proposal)?;
        let connection = self.connection()?;
        let capture_owner = connection
            .query_row(
                "SELECT owner_id FROM personal_captures WHERE capture_id=?1 AND expires_at>?2",
                params![proposal.capture_id, Utc::now().to_rfc3339()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if capture_owner.as_deref() != Some(proposal.owner_id.as_str()) {
            return Err(StoreError::InvalidInput(
                "capture missing, cross-owner, or expired".into(),
            ));
        }

        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        connection.execute(
            "INSERT INTO capture_proposals(\
                proposal_id,owner_id,capture_id,title,category,confidence,details,suggested_next_action,\
                rationale,evidence_capture_ids_json,status,created_at,expires_at,audit_id\
             ) VALUES(?1,?2,?3,?4,?5,0.0,NULL,'',?6,?7,'reviewing',?8,?9,?10)",
            params![
                id,
                proposal.owner_id,
                proposal.capture_id,
                proposal.title,
                proposal.category,
                proposal.rationale,
                serde_json::to_string(&vec![proposal.capture_id.clone()])?,
                created_at,
                proposal.expires_at,
                proposal.audit_id,
            ],
        )?;
        audit(
            &connection,
            &proposal.owner_id,
            &proposal.audit_id,
            "personal.proposal_created",
            &id,
            &created_at,
        )?;

        Ok(CaptureProposal {
            id,
            owner_id: proposal.owner_id,
            capture_id: proposal.capture_id.clone(),
            title: proposal.title,
            category: proposal.category,
            confidence: 0.0,
            details: None,
            suggested_next_action: String::new(),
            rationale: proposal.rationale,
            evidence_capture_ids: vec![proposal.capture_id.clone()],
            status: "reviewing".into(),
            created_at,
            expires_at: proposal.expires_at,
            audit_id: proposal.audit_id,
        })
    }

    pub fn capture_proposals(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<CaptureProposal>, StoreError> {
        if owner_id.trim().is_empty() {
            return Err(StoreError::InvalidInput("owner is required".into()));
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT proposal_id,owner_id,capture_id,title,category,confidence,details,suggested_next_action,\
             rationale,evidence_capture_ids_json,status,created_at,expires_at,audit_id \
             FROM capture_proposals WHERE owner_id=?1 AND status='reviewing' AND expires_at>?2 \
             ORDER BY created_at DESC, proposal_id DESC LIMIT ?3",
        )?;
        statement
            .query_map(
                params![owner_id, Utc::now().to_rfc3339(), limit.clamp(1, 200)],
                capture_proposal_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn approve_task_proposal(
        &self,
        owner_id: &str,
        proposal_id: &str,
        status: TaskApprovalStatus,
        estimated_minutes: u32,
        audit_id: &str,
    ) -> Result<TaskRecord, StoreError> {
        if owner_id.trim().is_empty()
            || proposal_id.trim().is_empty()
            || audit_id.trim().is_empty()
            || !(1..=1_440).contains(&estimated_minutes)
        {
            return Err(StoreError::InvalidInput("invalid task approval".into()));
        }
        let task_status = match status {
            TaskApprovalStatus::Proposed => "proposed",
            TaskApprovalStatus::Ready => "ready",
        };
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let transaction = connection.unchecked_transaction()?;
        let proposal = transaction
            .query_row(
                "SELECT proposal_id,owner_id,capture_id,title,category,confidence,details,suggested_next_action,\
                 rationale,evidence_capture_ids_json,status,created_at,expires_at,audit_id \
                 FROM capture_proposals WHERE proposal_id=?1 AND owner_id=?2 AND category='task' \
                 AND status='reviewing' AND expires_at>?3",
                params![proposal_id, owner_id, now],
                capture_proposal_from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::InvalidInput("task proposal missing, cross-owner, expired, or already decided".into()))?;
        let task_id = Uuid::new_v4().to_string();
        let observable_outcome = proposal
            .details
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&proposal.suggested_next_action);
        transaction.execute(
            "INSERT INTO tasks(task_id,owner_id,project_id,parent_task_id,title,observable_outcome,estimated_minutes,due_at,importance,status,created_at,updated_at) \
             VALUES(?1,?2,NULL,NULL,?3,?4,?5,NULL,'normal',?6,?7,?7)",
            params![task_id, owner_id, proposal.title, observable_outcome, estimated_minutes, task_status, now],
        )?;
        if transaction.execute(
            "UPDATE capture_proposals SET status='approved' WHERE proposal_id=?1 AND owner_id=?2 AND status='reviewing' AND expires_at>?3",
            params![proposal_id, owner_id, now],
        )? != 1 {
            return Err(StoreError::InvalidInput("task proposal is no longer reviewable".into()));
        }
        close_capture_after_review(&transaction, owner_id, &proposal.capture_id, &now)?;
        transaction.execute(
            "INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(?1,?2,'personal.proposal_task_approved','voiceos-core',?3,?4)",
            params![owner_id, audit_id, serde_json::json!({"proposal_id": proposal_id, "capture_id": proposal.capture_id, "task_id": task_id}).to_string(), now],
        )?;
        transaction.commit()?;
        Ok(TaskRecord {
            id: task_id,
            owner_id: owner_id.into(),
            project_id: None,
            parent_task_id: None,
            title: proposal.title,
            observable_outcome: observable_outcome.into(),
            estimated_minutes,
            due_at: None,
            importance: "normal".into(),
            status: task_status.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn approve_non_task_proposal(
        &self,
        owner_id: &str,
        proposal_id: &str,
        audit_id: &str,
    ) -> Result<PersonalReviewRecord, StoreError> {
        if owner_id.trim().is_empty() || proposal_id.trim().is_empty() || audit_id.trim().is_empty()
        {
            return Err(StoreError::InvalidInput("invalid non-task approval".into()));
        }
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let transaction = connection.unchecked_transaction()?;
        let proposal = transaction
            .query_row(
                "SELECT proposal_id,owner_id,capture_id,title,category,confidence,details,suggested_next_action,\
                 rationale,evidence_capture_ids_json,status,created_at,expires_at,audit_id \
                 FROM capture_proposals WHERE proposal_id=?1 AND owner_id=?2 \
                 AND category IN ('appointment','worry','idea','note') AND status='reviewing' AND expires_at>?3",
                params![proposal_id, owner_id, now],
                capture_proposal_from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::InvalidInput("non-task proposal missing, cross-owner, expired, or already decided".into()))?;
        let record_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO personal_review_records(record_id,owner_id,proposal_id,capture_id,category,title,details,suggested_next_action,status,created_at,audit_id) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'reviewable',?9,?10)",
            params![record_id, owner_id, proposal_id, proposal.capture_id, proposal.category, proposal.title, proposal.details, proposal.suggested_next_action, now, audit_id],
        )?;
        if transaction.execute(
            "UPDATE capture_proposals SET status='approved' WHERE proposal_id=?1 AND owner_id=?2 AND status='reviewing' AND expires_at>?3",
            params![proposal_id, owner_id, now],
        )? != 1 {
            return Err(StoreError::InvalidInput("non-task proposal is no longer reviewable".into()));
        }
        close_capture_after_review(&transaction, owner_id, &proposal.capture_id, &now)?;
        transaction.execute(
            "INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(?1,?2,'personal.proposal_review_record_approved','voiceos-core',?3,?4)",
            params![owner_id, audit_id, serde_json::json!({"proposal_id": proposal_id, "capture_id": proposal.capture_id, "record_id": record_id, "category": proposal.category}).to_string(), now],
        )?;
        transaction.commit()?;
        Ok(PersonalReviewRecord {
            id: record_id,
            owner_id: owner_id.into(),
            proposal_id: proposal_id.into(),
            capture_id: proposal.capture_id,
            category: proposal.category,
            title: proposal.title,
            details: proposal.details,
            suggested_next_action: proposal.suggested_next_action,
            status: "reviewable".into(),
            created_at: now,
            audit_id: audit_id.into(),
        })
    }

    pub fn personal_review_records(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<PersonalReviewRecord>, StoreError> {
        if owner_id.trim().is_empty() {
            return Err(StoreError::InvalidInput("owner is required".into()));
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT record_id,owner_id,proposal_id,capture_id,category,title,details,suggested_next_action,status,created_at,audit_id \
             FROM personal_review_records WHERE owner_id=?1 ORDER BY created_at DESC, record_id DESC LIMIT ?2",
        )?;
        statement
            .query_map(params![owner_id, limit.clamp(1, 200)], |row| {
                Ok(PersonalReviewRecord {
                    id: row.get(0)?,
                    owner_id: row.get(1)?,
                    proposal_id: row.get(2)?,
                    capture_id: row.get(3)?,
                    category: row.get(4)?,
                    title: row.get(5)?,
                    details: row.get(6)?,
                    suggested_next_action: row.get(7)?,
                    status: row.get(8)?,
                    created_at: row.get(9)?,
                    audit_id: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn decide_capture_proposal(
        &self,
        owner_id: &str,
        proposal_id: &str,
        status: &str,
        audit_id: &str,
    ) -> Result<ReviewDecision, StoreError> {
        if owner_id.trim().is_empty()
            || proposal_id.trim().is_empty()
            || audit_id.trim().is_empty()
            || !matches!(status, "rejected" | "snoozed" | "discarded")
        {
            return Err(StoreError::InvalidInput("invalid review decision".into()));
        }
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
        let capture_id: String = connection
            .query_row(
                "SELECT capture_id FROM capture_proposals WHERE proposal_id=?1 AND owner_id=?2 \
                 AND status='reviewing' AND expires_at>?3",
                params![proposal_id, owner_id, now],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::InvalidInput(
                    "proposal missing, cross-owner, expired, or already decided".into(),
                )
            })?;
        if connection.execute(
            "UPDATE capture_proposals SET status=?1 WHERE proposal_id=?2 AND owner_id=?3 \
             AND status='reviewing' AND expires_at>?4",
            params![status, proposal_id, owner_id, now],
        )? != 1
        {
            return Err(StoreError::InvalidInput(
                "proposal missing, cross-owner, expired, or already decided".into(),
            ));
        }
        close_capture_after_review(&connection, owner_id, &capture_id, &now)?;
        audit(
            &connection,
            owner_id,
            audit_id,
            "personal.proposal_decided",
            proposal_id,
            &now,
        )?;
        Ok(ReviewDecision {
            id: proposal_id.into(),
            owner_id: owner_id.into(),
            status: status.into(),
            audit_id: audit_id.into(),
        })
    }

    pub fn create_daily_focus_reset(
        &self,
        owner_id: &str,
        reset_date: &str,
        audit_id: &str,
    ) -> Result<DailyFocusReset, StoreError> {
        if owner_id.trim().is_empty() || audit_id.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "owner and audit id are required".into(),
            ));
        }
        NaiveDate::parse_from_str(reset_date, "%Y-%m-%d")
            .map_err(|_| StoreError::InvalidInput("date must be ISO date".into()))?;
        let connection = self.connection()?;
        let created_at = Utc::now().to_rfc3339();
        ensure_owner(&connection, owner_id, &created_at)?;
        let id = Uuid::new_v4().to_string();
        let expires_at = (Utc::now() + Duration::days(2)).to_rfc3339();
        connection.execute(
            "INSERT INTO daily_focus_resets(\
                reset_id,owner_id,reset_date,status,created_at,expires_at,audit_id\
             ) VALUES(?1,?2,?3,'received',?4,?5,?6)",
            params![id, owner_id, reset_date, created_at, expires_at, audit_id],
        )?;
        audit(
            &connection,
            owner_id,
            audit_id,
            "personal.daily_reset_created",
            &id,
            &created_at,
        )?;
        Ok(DailyFocusReset {
            id,
            owner_id: owner_id.into(),
            reset_date: reset_date.into(),
            status: "received".into(),
            created_at,
            expires_at,
            audit_id: audit_id.into(),
        })
    }

    pub fn audit_event_exists(&self, owner_id: &str, audit_id: &str) -> Result<bool, StoreError> {
        self.connection()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM execution_events WHERE owner_id=?1 AND stream_id=?2)",
                params![owner_id, audit_id],
                |row| row.get(0),
            )
            .map_err(StoreError::from)
    }
}
