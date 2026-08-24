use crate::model::{
    CaptureProposal, CaptureSource, DailyFocusReset, NewCaptureProposal, NewPersonalCapture,
    PersonalCapture, ReviewDecision,
};
use crate::{ConversationStore, StoreError};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::{OptionalExtension, params};
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

fn normalize_display_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

impl ConversationStore {
    pub fn capture_personal_input(
        &self,
        owner_id: &str,
        source: CaptureSource,
        text: &str,
        occurred_at: DateTime<Utc>,
        retention: Duration,
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
            audit_id: format!("personal-capture:{}", Uuid::new_v4()),
        })
    }

    pub fn personal_inbox(&self, owner_id: &str) -> Result<Vec<PersonalCapture>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT capture_id,owner_id,source,source_id,raw_content,display_text,structured_content_json,status,created_at,expires_at,audit_id \
             FROM personal_captures WHERE owner_id=?1 AND expires_at>?2 ORDER BY created_at DESC, capture_id DESC",
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
                proposal_id,owner_id,capture_id,title,category,rationale,status,created_at,expires_at,audit_id\
             ) VALUES(?1,?2,?3,?4,?5,?6,'reviewing',?7,?8,?9)",
            params![
                id,
                proposal.owner_id,
                proposal.capture_id,
                proposal.title,
                proposal.category,
                proposal.rationale,
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
            capture_id: proposal.capture_id,
            title: proposal.title,
            category: proposal.category,
            rationale: proposal.rationale,
            status: "reviewing".into(),
            created_at,
            expires_at: proposal.expires_at,
            audit_id: proposal.audit_id,
        })
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
            || !matches!(status, "approved" | "rejected" | "snoozed" | "discarded")
        {
            return Err(StoreError::InvalidInput("invalid review decision".into()));
        }
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
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
