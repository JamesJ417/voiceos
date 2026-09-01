use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AttachmentRecord, CalendarSecretReference, CalendarSecretStore, CalendarSecretStoreError,
    ChatMessage, ConversationContext, ConversationMessage, DocumentRecord, GENERAL_TALK_AREA_ID,
    GoogleCalendarConnection, Memory, QuarantineRecord, QuarantinedClaim, Role,
};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("conversation database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("conversation database lock is poisoned")]
    LockPoisoned,
    #[error("invalid agent record: {0}")]
    InvalidInput(String),
    #[error("invalid stored JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum CalendarSecretIntegrationError {
    #[error("calendar connection metadata error")]
    Store(#[from] StoreError),
    #[error("calendar secret storage error")]
    SecretStore(#[from] CalendarSecretStoreError),
}

pub struct ConversationStore {
    connection: Mutex<Connection>,
}

impl ConversationStore {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_structured_memory(
        transaction: &Transaction<'_>,
        owner_id: &str,
        device_id: &str,
        content: &str,
        category: &str,
        source: &str,
        confidence: f64,
        provenance: &str,
    ) -> Result<String, StoreError> {
        let content = content.trim();
        if content.is_empty() || content.chars().count() > 500 {
            return Err(StoreError::InvalidInput(
                "memory content must contain 1 to 500 characters".to_owned(),
            ));
        }
        if !matches!(
            category,
            "general" | "identity" | "preference" | "person" | "project" | "routine" | "sensitive"
        ) {
            return Err(StoreError::InvalidInput(
                "invalid memory category".to_owned(),
            ));
        }
        if !(0.0..=1.0).contains(&confidence) || provenance.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "memory confidence or provenance is invalid".to_owned(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "INSERT INTO memories(memory_id,device_id,normalized_content,content,source,created_at,updated_at,owner_id,category,status,confidence,provenance) VALUES(?1,?2,?3,?4,?5,?6,?6,?7,?8,'active',?9,?10)",
            params![id, device_id, normalize(content), content, source.trim(), now, owner_id, category, confidence, provenance.trim()],
        )?;
        Ok(id)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                StoreError::Sql(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }

    pub fn quarantine_claims(
        &self,
        conversation_id: &str,
        claims: &[QuarantinedClaim],
    ) -> Result<(), StoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for quarantined in claims {
            let claim = &quarantined.claim;
            transaction.execute(
                "INSERT INTO context_quarantine (quarantine_id, conversation_id, claim_id, source, provenance, confidence, relevance, content, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    Uuid::new_v4().to_string(), conversation_id, claim.id,
                    serde_json::to_string(&claim.source)?, claim.provenance,
                    claim.confidence, claim.relevance, claim.content,
                    quarantined.reason, Utc::now().to_rfc3339()
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn quarantined_claims_for_owner(
        &self,
        owner_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<QuarantineRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT q.quarantine_id, q.conversation_id, q.claim_id, q.source, q.provenance, q.confidence, q.relevance, q.content, q.reason, q.created_at FROM context_quarantine q JOIN conversations c ON c.conversation_id=q.conversation_id WHERE q.conversation_id=?1 AND c.owner_id=?2 ORDER BY q.created_at")?;
        let rows = statement.query_map(params![conversation_id, owner_id], |row| {
            Ok(QuarantineRecord {
                quarantine_id: row.get(0)?,
                conversation_id: row.get(1)?,
                claim_id: row.get(2)?,
                source: row.get(3)?,
                provenance: row.get(4)?,
                confidence: row.get(5)?,
                relevance: row.get(6)?,
                content: row.get(7)?,
                reason: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let connection = self.connection()?;
        crate::schema::migrate(&connection)?;
        drop(connection);
        self.backfill_task_progress()?;
        Ok(())
    }

    pub fn google_calendar_connection_for_owner(
        &self,
        owner_id: &str,
    ) -> Result<Option<GoogleCalendarConnection>, StoreError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT owner_id, provider, account_email, provider_account_id, secret_reference FROM google_calendar_connections WHERE owner_id=?1",
                [owner_id],
                |row| {
                    let secret_reference = row
                        .get::<_, Option<String>>(4)?
                        .map(CalendarSecretReference::try_from)
                        .transpose()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(GoogleCalendarConnection {
                        owner_id: row.get(0)?,
                        provider: row.get(1)?,
                        account_email: row.get(2)?,
                        provider_account_id: row.get(3)?,
                        secret_reference,
                    })
                },
            )
            .optional()?)
    }

    pub fn upsert_google_calendar_connection(
        &self,
        owner_id: &str,
        provider: &str,
        account_email: &str,
        provider_account_id: &str,
    ) -> Result<(), StoreError> {
        let connection = self.connection()?;
        connection.execute("INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at", params![owner_id, Utc::now().to_rfc3339()])?;
        connection.execute("INSERT INTO google_calendar_connections(owner_id,provider,account_email,provider_account_id,connected_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner_id) DO UPDATE SET provider=excluded.provider,account_email=excluded.account_email,provider_account_id=excluded.provider_account_id,connected_at=excluded.connected_at", params![owner_id,provider,account_email,provider_account_id,Utc::now().to_rfc3339()])?;
        Ok(())
    }

    pub fn disconnect_google_calendar(&self, owner_id: &str) -> Result<bool, StoreError> {
        Ok(self.connection()?.execute(
            "DELETE FROM google_calendar_connections WHERE owner_id=?1",
            [owner_id],
        )? > 0)
    }

    pub fn set_google_calendar_secret_reference(
        &self,
        owner_id: &str,
        reference: &CalendarSecretReference,
    ) -> Result<(), StoreError> {
        if self.connection()?.execute(
            "UPDATE google_calendar_connections SET secret_reference=?1 WHERE owner_id=?2",
            params![reference.as_str(), owner_id],
        )? == 0
        {
            return Err(StoreError::InvalidInput(
                "calendar connection does not exist for owner".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn disconnect_google_calendar_with_secret_store(
        &self,
        owner_id: &str,
        secret_store: &dyn CalendarSecretStore,
    ) -> Result<bool, CalendarSecretIntegrationError> {
        let reference = self
            .google_calendar_connection_for_owner(owner_id)?
            .and_then(|connection| connection.secret_reference);
        if let Some(reference) = reference {
            secret_store.delete(owner_id, &reference)?;
        }
        Ok(self.disconnect_google_calendar(owner_id)?)
    }

    pub fn resolve_conversation(
        &self,
        device_id: &str,
        client_session_id: Option<&str>,
    ) -> Result<String, StoreError> {
        self.resolve_owner_conversation(device_id, device_id, client_session_id)
    }

    pub fn migrate_devices_to_owner(&self, owner_id: &str) -> Result<(), StoreError> {
        if owner_id.trim().is_empty() {
            return Err(StoreError::InvalidInput("owner_id is required".to_owned()));
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let already_reconciled: bool = connection.query_row(
            "SELECT
                NOT EXISTS(SELECT 1 FROM devices d LEFT JOIN owner_devices od ON od.device_id=d.device_id AND od.owner_id=?1 AND od.revoked_at IS NULL WHERE od.device_id IS NULL)
                AND NOT EXISTS(SELECT 1 FROM conversations WHERE owner_id IS NULL OR owner_id<>?1)
                AND NOT EXISTS(SELECT 1 FROM memories WHERE owner_id IS NULL OR owner_id<>?1)
                AND NOT EXISTS(SELECT 1 FROM documents WHERE owner_id IS NULL OR owner_id<>?1)
                AND (SELECT COUNT(*) FROM conversations WHERE status='active') <= 1",
            [owner_id.trim()],
            |row| row.get(0),
        )?;
        if already_reconciled {
            return Ok(());
        }
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1, ?2, ?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id.trim(), now],
        )?;
        transaction.execute(
            "INSERT INTO owner_devices(owner_id, device_id, enrolled_at) SELECT ?1, device_id, ?2 FROM devices WHERE true ON CONFLICT(device_id) DO UPDATE SET owner_id=excluded.owner_id, revoked_at=NULL",
            params![owner_id.trim(), now],
        )?;
        let canonical: Option<String> = transaction
            .query_row(
                "SELECT conversation_id FROM conversations WHERE status='active' ORDER BY updated_at DESC, created_at LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(canonical) = canonical {
            transaction.execute(
                "UPDATE messages SET conversation_id=?1 WHERE conversation_id IN (SELECT conversation_id FROM conversations WHERE status='active' AND conversation_id<>?1)",
                [&canonical],
            )?;
            transaction.execute(
                "UPDATE conversation_aliases SET conversation_id=?1 WHERE conversation_id IN (SELECT conversation_id FROM conversations WHERE status='active' AND conversation_id<>?1)",
                [&canonical],
            )?;
            transaction.execute(
                "DELETE FROM conversation_summaries WHERE conversation_id IN (SELECT conversation_id FROM conversations WHERE status='active' AND conversation_id<>?1)",
                [&canonical],
            )?;
            transaction.execute(
                "UPDATE conversations SET status='archived', owner_id=?2 WHERE status='active' AND conversation_id<>?1",
                params![canonical, owner_id.trim()],
            )?;
        }
        transaction.execute("UPDATE conversations SET owner_id=?1", [owner_id.trim()])?;
        transaction.execute("UPDATE memories SET owner_id=?1", [owner_id.trim()])?;
        transaction.execute("UPDATE documents SET owner_id=?1", [owner_id.trim()])?;
        transaction.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS one_active_conversation_per_owner ON conversations(owner_id) WHERE status='active' AND owner_id IS NOT NULL;",
        )?;
        transaction.execute(
            "INSERT INTO owner_area_selections(owner_id,area_id,conversation_id,updated_at,updated_by_device) \
             SELECT ?1,COALESCE(c.area_id,'general-talk'),c.conversation_id,?2,COALESCE(c.device_id,'migration') \
             FROM (SELECT 1) seed LEFT JOIN conversations c ON c.owner_id=?1 AND c.status='active' \
             ON CONFLICT(owner_id) DO NOTHING",
            params![owner_id.trim(), now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn ensure_owner_device(&self, owner_id: &str, device_id: &str) -> Result<(), StoreError> {
        if owner_id.trim().is_empty() || device_id.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "owner_id and device_id are required".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1, ?2, ?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id.trim(), now],
        )?;
        transaction.execute(
            "INSERT INTO devices(device_id, created_at, last_seen_at) VALUES(?1, ?2, ?2) ON CONFLICT(device_id) DO UPDATE SET last_seen_at=excluded.last_seen_at",
            params![device_id.trim(), now],
        )?;
        transaction.execute(
            "INSERT INTO owner_devices(owner_id, device_id, enrolled_at) VALUES(?1, ?2, ?3) ON CONFLICT(device_id) DO UPDATE SET owner_id=excluded.owner_id, revoked_at=NULL",
            params![owner_id.trim(), device_id.trim(), now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn resolve_owner_conversation(
        &self,
        owner_id: &str,
        device_id: &str,
        client_session_id: Option<&str>,
    ) -> Result<String, StoreError> {
        if owner_id.trim().is_empty() || device_id.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "owner_id and device_id are required".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1, ?2, ?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id.trim(), now],
        )?;
        transaction.execute(
            "INSERT INTO devices(device_id, created_at, last_seen_at) VALUES(?1, ?2, ?2) ON CONFLICT(device_id) DO UPDATE SET last_seen_at=excluded.last_seen_at",
            params![device_id, now],
        )?;
        transaction.execute(
            "INSERT INTO owner_devices(owner_id, device_id, enrolled_at) VALUES(?1, ?2, ?3) ON CONFLICT(device_id) DO UPDATE SET owner_id=excluded.owner_id, revoked_at=NULL",
            params![owner_id.trim(), device_id, now],
        )?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT conversation_id FROM conversations WHERE owner_id=?1 AND status='active'",
                [owner_id.trim()],
                |row| row.get(0),
            )
            .optional()?;
        let conversation_id = existing.unwrap_or_else(|| Uuid::new_v4().to_string());
        let area_id: String = transaction
            .query_row(
                "SELECT area_id FROM owner_area_selections WHERE owner_id=?1",
                [owner_id.trim()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_else(|| GENERAL_TALK_AREA_ID.to_owned());
        transaction.execute(
            "INSERT OR IGNORE INTO conversations(conversation_id, device_id, status, created_at, updated_at, owner_id, area_id, title, area_updated_at, area_updated_by_device) VALUES(?1, ?2, 'active', ?3, ?3, ?4, ?5, 'New conversation', ?3, ?2)",
            params![conversation_id, device_id, now, owner_id.trim(), area_id],
        )?;
        transaction.execute(
            "INSERT INTO owner_area_selections(owner_id,area_id,conversation_id,updated_at,updated_by_device) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner_id) DO UPDATE SET area_id=excluded.area_id,conversation_id=excluded.conversation_id,updated_at=excluded.updated_at,updated_by_device=excluded.updated_by_device",
            params![owner_id.trim(), area_id, conversation_id, now, device_id],
        )?;
        if let Some(alias) = client_session_id.filter(|alias| !alias.trim().is_empty()) {
            transaction.execute(
                "INSERT INTO conversation_aliases(device_id, client_session_id, conversation_id, first_seen_at) VALUES(?1, ?2, ?3, ?4) ON CONFLICT(device_id, client_session_id) DO UPDATE SET conversation_id=excluded.conversation_id",
                params![device_id, alias.trim(), conversation_id, now],
            )?;
        }
        transaction.commit()?;
        Ok(conversation_id)
    }

    pub fn append_message(
        &self,
        conversation_id: &str,
        role: Role,
        content: &str,
        provider: Option<&str>,
    ) -> Result<i64, StoreError> {
        self.append_message_from(conversation_id, role, content, provider, None, None)
    }

    pub fn append_message_from(
        &self,
        conversation_id: &str,
        role: Role,
        content: &str,
        provider: Option<&str>,
        origin_device_id: Option<&str>,
        request_id: Option<&str>,
    ) -> Result<i64, StoreError> {
        let now = Utc::now().to_rfc3339();
        let role = role_name(&role);
        let connection = self.connection()?;
        connection.execute(
            "INSERT OR IGNORE INTO messages(conversation_id, role, content, provider, created_at, origin_device_id, request_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![conversation_id, role, content, provider, now, origin_device_id, request_id],
        )?;
        connection.execute(
            "UPDATE conversations SET updated_at=?2 WHERE conversation_id=?1",
            params![conversation_id, now],
        )?;
        if role == "user" {
            let title = content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(120)
                .collect::<String>();
            connection.execute(
                "UPDATE conversations SET title=?2 WHERE conversation_id=?1 AND (title='' OR title='New conversation')",
                params![conversation_id, title],
            )?;
        }
        if let Some(request_id) = request_id {
            return connection
                .query_row(
                    "SELECT message_id FROM messages WHERE conversation_id=?1 AND request_id=?2",
                    params![conversation_id, request_id],
                    |row| row.get(0),
                )
                .map_err(StoreError::from);
        }
        Ok(connection.last_insert_rowid())
    }

    pub fn active_conversation(&self, owner_id: &str) -> Result<Option<String>, StoreError> {
        self.connection()?
            .query_row(
                "SELECT conversation_id FROM conversations WHERE owner_id=?1 AND status='active'",
                [owner_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn conversation_messages(
        &self,
        owner_id: &str,
        after_sequence: i64,
        limit: usize,
    ) -> Result<Vec<ConversationMessage>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT m.message_id, m.conversation_id, c.area_id, m.role, m.content, m.provider, m.origin_device_id, m.created_at
             FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id
             WHERE c.owner_id=?1 AND c.status='active' AND m.message_id>?2
             ORDER BY m.message_id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![owner_id, after_sequence.max(0), limit.clamp(1, 500)],
            |row| {
                Ok(ConversationMessage {
                    sequence: row.get(0)?,
                    conversation_id: row.get(1)?,
                    area_id: row.get(2)?,
                    role: parse_role(row.get::<_, String>(3)?),
                    content: row.get(4)?,
                    provider: row.get(5)?,
                    origin_device_id: row.get(6)?,
                    created_at: row.get(7)?,
                    attachments: Vec::new(),
                })
            },
        )?;
        let mut messages = rows.collect::<Result<Vec<_>, _>>()?;
        for message in &mut messages {
            message.attachments =
                attachments_for_message_connection(&connection, message.sequence)?;
        }
        Ok(messages)
    }

    pub fn recent_conversation_messages(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<ConversationMessage>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT message_id, conversation_id, area_id, role, content, provider, origin_device_id, created_at
             FROM (
                SELECT m.message_id, m.conversation_id, c.area_id, m.role, m.content, m.provider, m.origin_device_id, m.created_at
                FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id
                WHERE c.owner_id=?1 AND c.status='active'
                ORDER BY m.message_id DESC LIMIT ?2
             ) ORDER BY message_id",
        )?;
        let rows = statement.query_map(params![owner_id, limit.clamp(1, 500)], |row| {
            Ok(ConversationMessage {
                sequence: row.get(0)?,
                conversation_id: row.get(1)?,
                area_id: row.get(2)?,
                role: parse_role(row.get::<_, String>(3)?),
                content: row.get(4)?,
                provider: row.get(5)?,
                origin_device_id: row.get(6)?,
                created_at: row.get(7)?,
                attachments: Vec::new(),
            })
        })?;
        let mut messages = rows.collect::<Result<Vec<_>, _>>()?;
        for message in &mut messages {
            message.attachments =
                attachments_for_message_connection(&connection, message.sequence)?;
        }
        Ok(messages)
    }

    pub fn message_count(&self, conversation_id: &str) -> Result<usize, StoreError> {
        let count: i64 = self.connection()?.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id=?1",
            [conversation_id],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn messages_through(
        &self,
        conversation_id: &str,
        maximum_id: i64,
    ) -> Result<Vec<(i64, ChatMessage)>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT message_id, role, content FROM messages WHERE conversation_id=?1 AND message_id<=?2 ORDER BY message_id",
        )?;
        let rows = statement.query_map(params![conversation_id, maximum_id], |row| {
            Ok((
                row.get(0)?,
                ChatMessage::new(
                    parse_role(row.get::<_, String>(1)?),
                    row.get::<_, String>(2)?,
                ),
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn recent_messages(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<ChatMessage>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT role, content FROM (SELECT message_id, role, content FROM messages WHERE conversation_id=?1 ORDER BY message_id DESC LIMIT ?2) ORDER BY message_id",
        )?;
        let rows = statement.query_map(params![conversation_id, limit as i64], |row| {
            Ok(ChatMessage::new(
                parse_role(row.get::<_, String>(0)?),
                row.get::<_, String>(1)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn message_id_before_recent(
        &self,
        conversation_id: &str,
        keep_recent: usize,
    ) -> Result<Option<i64>, StoreError> {
        self.connection()?
            .query_row(
                "SELECT message_id FROM messages WHERE conversation_id=?1 ORDER BY message_id DESC LIMIT 1 OFFSET ?2",
                params![conversation_id, keep_recent as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn summary(&self, conversation_id: &str) -> Result<Option<(String, i64)>, StoreError> {
        self.connection()?
            .query_row(
                "SELECT content, through_message_id FROM conversation_summaries WHERE conversation_id=?1",
                [conversation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn summary_for_owner(
        &self,
        owner_id: &str,
        conversation_id: &str,
    ) -> Result<Option<(String, i64)>, StoreError> {
        self.connection()?
            .query_row(
                "SELECT s.content, s.through_message_id FROM conversation_summaries s JOIN conversations c ON c.conversation_id=s.conversation_id WHERE s.conversation_id=?1 AND c.owner_id=?2 AND c.status='active' AND s.owner_id=?2 AND s.provenance<>''",
                params![conversation_id, owner_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn save_summary_for_owner(
        &self,
        owner_id: &str,
        conversation_id: &str,
        content: &str,
        through_message_id: i64,
    ) -> Result<(), StoreError> {
        let eligible: bool = self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE conversation_id=?1 AND owner_id=?2 AND status='active')",
            params![conversation_id, owner_id],
            |row| row.get(0),
        )?;
        if !eligible {
            return Err(StoreError::InvalidInput(
                "summary requires an active conversation owned by the requested owner".to_owned(),
            ));
        }
        self.connection()?.execute(
            "INSERT INTO conversation_summaries(conversation_id, content, through_message_id, updated_at, owner_id, provenance) VALUES(?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(conversation_id) DO UPDATE SET content=excluded.content, through_message_id=excluded.through_message_id, updated_at=excluded.updated_at, owner_id=excluded.owner_id, provenance=excluded.provenance",
            params![
                conversation_id,
                content,
                through_message_id,
                Utc::now().to_rfc3339(),
                owner_id,
                format!("conversation-summary://{conversation_id}"),
            ],
        )?;
        Ok(())
    }

    pub fn save_summary(
        &self,
        conversation_id: &str,
        content: &str,
        through_message_id: i64,
    ) -> Result<(), StoreError> {
        self.connection()?.execute(
            "INSERT INTO conversation_summaries(conversation_id, content, through_message_id, updated_at) VALUES(?1, ?2, ?3, ?4) ON CONFLICT(conversation_id) DO UPDATE SET content=excluded.content, through_message_id=excluded.through_message_id, updated_at=excluded.updated_at",
            params![conversation_id, content, through_message_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn remember(&self, device_id: &str, content: &str, source: &str) -> Result<(), StoreError> {
        let normalized = normalize(content);
        if normalized.is_empty() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let confidence = if source == "explicit-user-request" {
            1.0
        } else {
            0.85
        };
        let provenance = format!("user://{device_id}");
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE memories SET content=?1,source=?2,confidence=?3,provenance=?4,updated_at=?5 WHERE device_id=?6 AND normalized_content=?7 AND status='active'",
            params![content.trim(), source, confidence, provenance, now, device_id, normalized],
        )?;
        if changed == 0 {
            transaction.execute(
                "INSERT INTO memories(memory_id,device_id,normalized_content,content,source,created_at,updated_at,category,status,confidence,provenance) VALUES(?1,?2,?3,?4,?5,?6,?6,'general','active',?7,?8)",
                params![Uuid::new_v4().to_string(), device_id, normalized, content.trim(), source, now, confidence, provenance],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn memories(&self, device_id: &str, limit: usize) -> Result<Vec<Memory>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT memory_id, content, source, created_at, updated_at, category, status, confidence, provenance, supersedes_memory_id FROM memories WHERE device_id=?1 AND status='active' ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![device_id, limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                category: row.get(5)?,
                status: row.get(6)?,
                confidence: row.get(7)?,
                provenance: row.get(8)?,
                supersedes_memory_id: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn remember_for_owner(
        &self,
        owner_id: &str,
        device_id: &str,
        content: &str,
        source: &str,
    ) -> Result<(), StoreError> {
        self.remember(device_id, content, source)?;
        self.connection()?.execute(
            "UPDATE memories SET owner_id=?1 WHERE device_id=?2 AND normalized_content=?3",
            params![owner_id, device_id, normalize(content)],
        )?;
        Ok(())
    }

    pub fn remember_for_owner_in_conversation(
        &self,
        owner_id: &str,
        device_id: &str,
        conversation_id: &str,
        content: &str,
        source: &str,
    ) -> Result<(), StoreError> {
        self.remember_for_owner(owner_id, device_id, content, source)?;
        self.connection()?.execute("UPDATE memories SET conversation_id=?1,area_id=(SELECT area_id FROM conversations WHERE conversation_id=?1 AND owner_id=?2) WHERE owner_id=?2 AND device_id=?3 AND normalized_content=?4", params![conversation_id, owner_id, device_id, normalize(content)])?;
        Ok(())
    }

    pub fn memories_for_owner(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<Memory>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT memory_id, content, source, created_at, updated_at, category, status, confidence, provenance, supersedes_memory_id FROM memories WHERE owner_id=?1 AND status='active' ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![owner_id, limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                category: row.get(5)?,
                status: row.get(6)?,
                confidence: row.get(7)?,
                provenance: row.get(8)?,
                supersedes_memory_id: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn search_memories_for_owner(
        &self,
        owner_id: &str,
        query: Option<&str>,
        include_inactive: bool,
        limit: usize,
    ) -> Result<Vec<Memory>, StoreError> {
        let connection = self.connection()?;
        let pattern = format!("%{}%", query.unwrap_or("").trim());
        let mut statement = connection.prepare(
            "SELECT memory_id, content, source, created_at, updated_at, category, status, confidence, provenance, supersedes_memory_id FROM memories WHERE owner_id=?1 AND (?2 OR status='active') AND content LIKE ?3 ORDER BY updated_at DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![
                owner_id,
                include_inactive,
                pattern,
                limit.clamp(1, 500) as i64
            ],
            memory_from_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_structured_memory(
        &self,
        owner_id: &str,
        device_id: &str,
        content: &str,
        category: &str,
        source: &str,
        confidence: f64,
        provenance: &str,
        supersedes_memory_id: Option<&str>,
    ) -> Result<Memory, StoreError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO owners(owner_id, created_at, updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at",
            params![owner_id, now],
        )?;
        transaction.execute(
            "INSERT INTO devices(device_id, created_at, last_seen_at) VALUES(?1,?2,?2) ON CONFLICT(device_id) DO UPDATE SET last_seen_at=excluded.last_seen_at",
            params![device_id, now],
        )?;
        transaction.execute(
            "INSERT INTO owner_devices(owner_id, device_id, enrolled_at) VALUES(?1,?2,?3) ON CONFLICT(device_id) DO UPDATE SET owner_id=excluded.owner_id, revoked_at=NULL",
            params![owner_id, device_id, now],
        )?;
        if let Some(previous) = supersedes_memory_id {
            transaction.execute(
                "UPDATE memories SET status='superseded', updated_at=?1 WHERE memory_id=?2 AND owner_id=?3 AND status='active'",
                params![now, previous, owner_id],
            )?;
        }
        transaction.execute(
            "INSERT INTO memories(memory_id, device_id, normalized_content, content, source, created_at, updated_at, owner_id, category, status, confidence, provenance, supersedes_memory_id) VALUES(?1,?2,?3,?4,?5,?6,?6,?7,?8,'active',?9,?10,?11)",
            params![id, device_id, normalize(content), content.trim(), source, now, owner_id, category.trim(), confidence.clamp(0.0, 1.0), provenance, supersedes_memory_id],
        )?;
        transaction.commit()?;
        Ok(Memory {
            id,
            content: content.trim().to_owned(),
            source: source.to_owned(),
            created_at: now.clone(),
            updated_at: now,
            category: category.trim().to_owned(),
            status: "active".to_owned(),
            confidence: confidence.clamp(0.0, 1.0),
            provenance: provenance.to_owned(),
            supersedes_memory_id: supersedes_memory_id.map(str::to_owned),
        })
    }

    pub fn forget_memory_for_owner(
        &self,
        owner_id: &str,
        memory_id: &str,
    ) -> Result<bool, StoreError> {
        let changed = self.connection()?.execute(
            "UPDATE memories SET status='forgotten', updated_at=?1 WHERE owner_id=?2 AND memory_id=?3 AND status!='forgotten'",
            params![Utc::now().to_rfc3339(), owner_id, memory_id],
        )?;
        Ok(changed == 1)
    }

    pub fn memories_for_owner_conversation(
        &self,
        owner_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<Memory>, StoreError> {
        let connection = self.connection()?;
        let area_id: Option<String> = connection
            .query_row(
                "SELECT area_id FROM conversations WHERE owner_id=?1 AND conversation_id=?2",
                params![owner_id, conversation_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(area_id) = area_id else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare("SELECT memory_id, content, source, created_at, updated_at, category, status, confidence, provenance, supersedes_memory_id FROM memories WHERE owner_id=?1 AND area_id=?2 AND status='active' ORDER BY updated_at DESC LIMIT ?3")?;
        let rows = statement.query_map(params![owner_id, area_id, limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                category: row.get(5)?,
                status: row.get(6)?,
                confidence: row.get(7)?,
                provenance: row.get(8)?,
                supersedes_memory_id: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn context(
        &self,
        device_id: &str,
        conversation_id: &str,
        query: &str,
        recent_limit: usize,
        memory_limit: usize,
    ) -> Result<ConversationContext, StoreError> {
        let area_id = self
            .connection()?
            .query_row(
                "SELECT area_id FROM conversations WHERE conversation_id=?1 AND device_id=?2",
                params![conversation_id, device_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_else(|| GENERAL_TALK_AREA_ID.to_owned());
        Ok(ConversationContext {
            conversation_id: conversation_id.to_owned(),
            area_id,
            summary: self.summary(conversation_id)?.map(|value| value.0),
            memories: self.memories(device_id, memory_limit)?,
            document_context: self.relevant_document_context(device_id, query, 6, 8_000)?,
            recent_messages: self.recent_messages(conversation_id, recent_limit)?,
        })
    }

    pub fn context_for_owner(
        &self,
        owner_id: &str,
        conversation_id: &str,
        query: &str,
        recent_limit: usize,
        memory_limit: usize,
    ) -> Result<ConversationContext, StoreError> {
        let eligible: bool = self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE conversation_id=?1 AND owner_id=?2 AND status='active')",
            params![conversation_id, owner_id],
            |row| row.get(0),
        )?;
        if !eligible {
            return Err(StoreError::InvalidInput(
                "conversation is not an active conversation owned by the requested owner"
                    .to_owned(),
            ));
        }
        let area_id: String = self.connection()?.query_row(
            "SELECT area_id FROM conversations WHERE owner_id=?1 AND conversation_id=?2",
            params![owner_id, conversation_id],
            |row| row.get(0),
        )?;
        Ok(ConversationContext {
            conversation_id: conversation_id.to_owned(),
            area_id,
            summary: self
                .summary_for_owner(owner_id, conversation_id)?
                .map(|value| value.0),
            memories: self.memories_for_owner_conversation(
                owner_id,
                conversation_id,
                memory_limit,
            )?,
            document_context: self
                .relevant_document_context_for_owner(owner_id, query, 6, 8_000)?,
            recent_messages: self.recent_messages(conversation_id, recent_limit)?,
        })
    }

    pub fn ingest_text_document(
        &self,
        device_id: &str,
        filename: &str,
        media_type: &str,
        mode: &str,
        source: &[u8],
    ) -> Result<DocumentRecord, StoreError> {
        let safe_mode = if mode == "profile" {
            "profile"
        } else {
            "reference"
        };
        let content = std::str::from_utf8(source).map_err(|error| {
            StoreError::Sql(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
        })?;
        let content = content.trim_start_matches('\u{feff}').trim();
        let chunks = chunk_text(content, 1_200, 160);
        let now = Utc::now().to_rfc3339();
        let sha256 = format!("{:x}", Sha256::digest(source));
        let document_id = Uuid::new_v4().to_string();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO devices(device_id, created_at, last_seen_at) VALUES(?1, ?2, ?2) ON CONFLICT(device_id) DO UPDATE SET last_seen_at=excluded.last_seen_at",
            params![device_id, now],
        )?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT document_id FROM documents WHERE device_id=?1 AND sha256=?2 AND mode=?3",
                params![device_id, sha256, safe_mode],
                |row| row.get(0),
            )
            .optional()?;
        let final_id = existing.unwrap_or_else(|| document_id.clone());
        transaction.execute(
            "INSERT OR IGNORE INTO documents(document_id, device_id, filename, media_type, mode, byte_size, sha256, source_bytes, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![final_id, device_id, sanitize_filename(filename), media_type, safe_mode, source.len() as i64, sha256, source, now],
        )?;
        if final_id == document_id {
            for (ordinal, chunk) in chunks.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO document_chunks(document_id, ordinal, content) VALUES(?1, ?2, ?3)",
                    params![final_id, ordinal as i64, chunk],
                )?;
            }
        }
        transaction.commit()?;
        drop(connection);
        self.document(device_id, &final_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows.into())
    }

    pub fn ingest_text_document_for_owner(
        &self,
        owner_id: &str,
        device_id: &str,
        filename: &str,
        media_type: &str,
        mode: &str,
        source: &[u8],
    ) -> Result<DocumentRecord, StoreError> {
        let document = self.ingest_text_document(device_id, filename, media_type, mode, source)?;
        self.connection()?.execute(
            "UPDATE documents SET owner_id=?1 WHERE document_id=?2",
            params![owner_id, document.id],
        )?;
        Ok(document)
    }

    pub fn ingest_attachment_for_owner(
        &self,
        owner_id: &str,
        device_id: &str,
        filename: &str,
        media_type: &str,
        source: &[u8],
    ) -> Result<AttachmentRecord, StoreError> {
        if owner_id.trim().is_empty() || device_id.trim().is_empty() || source.is_empty() {
            return Err(StoreError::InvalidInput(
                "attachment owner, device, and bytes are required".to_owned(),
            ));
        }
        self.cleanup_expired_attachments()?;
        let now = Utc::now().to_rfc3339();
        self.connection()?.execute(
            "INSERT INTO devices(device_id, created_at, last_seen_at) VALUES(?1, ?2, ?2) ON CONFLICT(device_id) DO UPDATE SET last_seen_at=excluded.last_seen_at",
            params![device_id.trim(), now],
        )?;
        let attachment = AttachmentRecord {
            id: Uuid::new_v4().to_string(),
            filename: sanitize_filename(filename),
            media_type: media_type.to_owned(),
            byte_size: source.len() as u64,
            sha256: format!("{:x}", Sha256::digest(source)),
            status: "uploaded".to_owned(),
            created_at: now.clone(),
        };
        self.connection()?.execute(
            "INSERT INTO attachments(attachment_id, owner_id, device_id, filename, media_type, byte_size, sha256, source_bytes, status, created_at, expires_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![attachment.id, owner_id.trim(), device_id.trim(), attachment.filename, attachment.media_type, attachment.byte_size as i64, attachment.sha256, source, attachment.status, attachment.created_at, (Utc::now() + chrono::Duration::days(7)).to_rfc3339()],
        )?;
        Ok(attachment)
    }

    pub fn claim_attachments_for_owner_turn(
        &self,
        owner_id: &str,
        device_id: &str,
        conversation_id: &str,
        content: &str,
        request_id: Option<&str>,
        attachment_ids: &[String],
    ) -> Result<i64, StoreError> {
        if request_id.is_none() && !attachment_ids.is_empty() {
            return Err(StoreError::InvalidInput(
                "request_id is required when claiming attachments".to_owned(),
            ));
        }
        if attachment_ids.iter().any(|id| id.trim().is_empty())
            || attachment_ids
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != attachment_ids.len()
        {
            return Err(StoreError::InvalidInput(
                "attachment_ids must be unique and non-empty".to_owned(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO messages(conversation_id, role, content, created_at, origin_device_id, request_id) VALUES(?1, 'user', ?2, ?3, ?4, ?5)",
            params![conversation_id, content, Utc::now().to_rfc3339(), device_id, request_id],
        )?;
        let message_id = if let Some(request_id) = request_id {
            transaction.query_row(
                "SELECT message_id FROM messages WHERE conversation_id=?1 AND request_id=?2",
                params![conversation_id, request_id],
                |row| row.get(0),
            )?
        } else {
            transaction.last_insert_rowid()
        };
        if inserted > 0 {
            for attachment_id in attachment_ids {
                if transaction.execute("UPDATE attachments SET status='attached' WHERE attachment_id=?1 AND owner_id=?2 AND device_id=?3 AND status='uploaded'", params![attachment_id, owner_id, device_id])? != 1 {
                    return Err(StoreError::InvalidInput("attachment missing, unavailable, or not owned by device".to_owned()));
                }
                transaction.execute(
                    "INSERT INTO message_attachments(message_id, attachment_id) VALUES(?1, ?2)",
                    params![message_id, attachment_id],
                )?;
            }
        } else {
            let existing = attachments_for_message_connection(&transaction, message_id)?;
            let existing_ids = existing
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>();
            let requested_ids = attachment_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            if existing_ids != requested_ids {
                return Err(StoreError::InvalidInput(
                    "request_id was already used with different attachments".to_owned(),
                ));
            }
        }
        transaction.commit()?;
        Ok(message_id)
    }

    pub fn attachments_for_message(
        &self,
        message_id: i64,
    ) -> Result<Vec<AttachmentRecord>, StoreError> {
        let connection = self.connection()?;
        attachments_for_message_connection(&connection, message_id)
    }

    pub fn attachment_content_for_owner(
        &self,
        owner_id: &str,
        attachment_id: &str,
    ) -> Result<Option<(AttachmentRecord, Vec<u8>)>, StoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT attachment_id, filename, media_type, byte_size, sha256, status, created_at, source_bytes FROM attachments WHERE attachment_id=?1 AND owner_id=?2",
                params![attachment_id, owner_id],
                |row| Ok((attachment_row(row)?, row.get(7)?)),
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn cleanup_expired_attachments(&self) -> Result<usize, StoreError> {
        let deleted = self.connection()?.execute(
            "DELETE FROM attachments WHERE status='uploaded' AND expires_at < ?1",
            [Utc::now().to_rfc3339()],
        )?;
        Ok(deleted)
    }

    pub fn list_documents(&self, device_id: &str) -> Result<Vec<DocumentRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.document_id, d.filename, d.media_type, d.mode, d.byte_size, d.sha256, COUNT(c.chunk_id), d.created_at FROM documents d LEFT JOIN document_chunks c ON c.document_id=d.document_id WHERE d.device_id=?1 GROUP BY d.document_id ORDER BY d.created_at DESC",
        )?;
        let rows = statement.query_map([device_id], document_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_documents_for_owner(
        &self,
        owner_id: &str,
    ) -> Result<Vec<DocumentRecord>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.document_id, d.filename, d.media_type, d.mode, d.byte_size, d.sha256, COUNT(c.chunk_id), d.created_at FROM documents d LEFT JOIN document_chunks c ON c.document_id=d.document_id WHERE d.owner_id=?1 GROUP BY d.document_id ORDER BY d.created_at DESC",
        )?;
        let rows = statement.query_map([owner_id], document_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn delete_document(&self, device_id: &str, document_id: &str) -> Result<bool, StoreError> {
        let changed = self.connection()?.execute(
            "DELETE FROM documents WHERE device_id=?1 AND document_id=?2",
            params![device_id, document_id],
        )?;
        Ok(changed == 1)
    }

    pub fn delete_document_for_owner(
        &self,
        owner_id: &str,
        document_id: &str,
    ) -> Result<bool, StoreError> {
        let changed = self.connection()?.execute(
            "DELETE FROM documents WHERE owner_id=?1 AND document_id=?2",
            params![owner_id, document_id],
        )?;
        Ok(changed == 1)
    }

    pub fn relevant_document_context(
        &self,
        device_id: &str,
        query: &str,
        max_chunks: usize,
        max_chars: usize,
    ) -> Result<Option<String>, StoreError> {
        let query_terms = search_terms(query);
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.filename, d.mode, c.ordinal, c.content FROM documents d JOIN document_chunks c ON c.document_id=d.document_id WHERE d.device_id=?1 ORDER BY d.created_at DESC, c.ordinal",
        )?;
        let rows = statement.query_map([device_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut candidates = rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(filename, mode, ordinal, content)| {
                let terms = search_terms(&content);
                let overlap = query_terms
                    .iter()
                    .filter(|term| terms.contains(*term))
                    .count() as i32;
                let score = if mode == "profile" {
                    10_000 - ordinal as i32
                } else {
                    overlap * 100 - ordinal as i32
                };
                (score, filename, mode, ordinal, content)
            })
            .filter(|(score, _, mode, _, _)| mode == "profile" || *score > 0)
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let mut output = String::new();
        for (_, filename, mode, ordinal, content) in candidates.into_iter().take(max_chunks) {
            let section = format!(
                "[Source: {filename}; mode: {mode}; passage: {}]\n{}\n",
                ordinal + 1,
                content.trim()
            );
            if output.chars().count() + section.chars().count() > max_chars {
                break;
            }
            output.push_str(&section);
        }
        Ok((!output.is_empty()).then(|| output.trim().to_owned()))
    }

    pub fn relevant_document_context_for_owner(
        &self,
        owner_id: &str,
        query: &str,
        max_chunks: usize,
        max_chars: usize,
    ) -> Result<Option<String>, StoreError> {
        let query_terms = search_terms(query);
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.filename, d.mode, c.ordinal, c.content FROM documents d JOIN document_chunks c ON c.document_id=d.document_id WHERE d.owner_id=?1 ORDER BY d.created_at DESC, c.ordinal",
        )?;
        let rows = statement.query_map([owner_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut candidates = rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(filename, mode, ordinal, content)| {
                let terms = search_terms(&content);
                let overlap = query_terms
                    .iter()
                    .filter(|term| terms.contains(*term))
                    .count() as i32;
                let score = if mode == "profile" {
                    10_000 - ordinal as i32
                } else {
                    overlap * 100 - ordinal as i32
                };
                (score, filename, mode, ordinal, content)
            })
            .filter(|(score, _, mode, _, _)| mode == "profile" || *score > 0)
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let mut output = String::new();
        for (_, filename, mode, ordinal, content) in candidates.into_iter().take(max_chunks) {
            let section = format!(
                "[Source: {filename}; mode: {mode}; passage: {}]\n{}\n",
                ordinal + 1,
                content.trim()
            );
            if output.chars().count() + section.chars().count() > max_chars {
                break;
            }
            output.push_str(&section);
        }
        Ok((!output.is_empty()).then(|| output.trim().to_owned()))
    }

    fn document(
        &self,
        device_id: &str,
        document_id: &str,
    ) -> Result<Option<DocumentRecord>, StoreError> {
        self.connection()?.query_row(
            "SELECT d.document_id, d.filename, d.media_type, d.mode, d.byte_size, d.sha256, COUNT(c.chunk_id), d.created_at FROM documents d LEFT JOIN document_chunks c ON c.document_id=d.document_id WHERE d.device_id=?1 AND d.document_id=?2 GROUP BY d.document_id",
            params![device_id, document_id],
            document_row,
        ).optional().map_err(StoreError::from)
    }

    pub fn import_legacy_audit(
        &self,
        legacy_path: impl AsRef<Path>,
        owner_id: &str,
        device_id: &str,
    ) -> Result<usize, StoreError> {
        let source_path = legacy_path.as_ref().to_string_lossy().to_string();
        let source_key = format!("{source_path}#device={device_id}");
        let legacy =
            Connection::open_with_flags(legacy_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut statement = legacy.prepare(
            "SELECT id, session_id, transcript, response_text, provider, created_at FROM turns ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut imported = 0;
        for row in rows {
            let (legacy_id, session_id, transcript, response, provider, created_at) = row?;
            let already_imported: bool = self.connection()?.query_row(
                "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE source_path=?1 AND legacy_turn_id=?2)",
                params![source_key, legacy_id],
                |row| row.get(0),
            )?;
            if already_imported {
                continue;
            }
            let conversation_id = self.resolve_owner_conversation(
                owner_id,
                device_id,
                Some(&format!("legacy:{session_id}")),
            )?;
            let connection = self.connection()?;
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO messages(conversation_id, role, content, legacy_turn_id, created_at) VALUES(?1, 'user', ?2, ?3, ?4)",
                params![conversation_id, transcript, legacy_id, created_at],
            )?;
            transaction.execute(
                "INSERT INTO messages(conversation_id, role, content, provider, legacy_turn_id, created_at) VALUES(?1, 'assistant', ?2, ?3, ?4, ?5)",
                params![conversation_id, response, provider, legacy_id, created_at],
            )?;
            transaction.execute(
                "INSERT INTO legacy_imports(source_path, legacy_turn_id, imported_at) VALUES(?1, ?2, ?3)",
                params![source_key, legacy_id, Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
            imported += 1;
        }
        Ok(imported)
    }
}

fn role_name(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn parse_role(value: String) -> Role {
    match value.as_str() {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn attachment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRecord> {
    Ok(AttachmentRecord {
        id: row.get(0)?,
        filename: row.get(1)?,
        media_type: row.get(2)?,
        byte_size: row.get::<_, i64>(3)?.max(0) as u64,
        sha256: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn attachments_for_message_connection(
    connection: &rusqlite::Connection,
    message_id: i64,
) -> Result<Vec<AttachmentRecord>, StoreError> {
    let mut statement = connection.prepare("SELECT a.attachment_id, a.filename, a.media_type, a.byte_size, a.sha256, a.status, a.created_at FROM attachments a JOIN message_attachments ma ON ma.attachment_id=a.attachment_id WHERE ma.message_id=?1 ORDER BY ma.rowid")?;
    let rows = statement.query_map([message_id], attachment_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn document_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentRecord> {
    Ok(DocumentRecord {
        id: row.get(0)?,
        filename: row.get(1)?,
        media_type: row.get(2)?,
        mode: row.get(3)?,
        byte_size: row.get::<_, i64>(4)?.max(0) as u64,
        sha256: row.get(5)?,
        chunk_count: row.get::<_, i64>(6)?.max(0) as usize,
        created_at: row.get(7)?,
    })
}

fn sanitize_filename(filename: &str) -> String {
    let name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("document.txt")
        .trim();
    if name.is_empty() {
        "document.txt".to_owned()
    } else {
        name.chars().take(180).collect()
    }
}

fn chunk_text(content: &str, chunk_chars: usize, overlap_chars: usize) -> Vec<String> {
    let characters = content.chars().collect::<Vec<_>>();
    if characters.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < characters.len() {
        let end = (start + chunk_chars).min(characters.len());
        chunks.push(characters[start..end].iter().collect::<String>());
        if end == characters.len() {
            break;
        }
        start = end.saturating_sub(overlap_chars.min(chunk_chars.saturating_sub(1)));
    }
    chunks
}

fn search_terms(value: &str) -> std::collections::HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "the", "and", "that", "this", "with", "from", "your", "what", "when", "where", "about",
        "have", "into", "for", "are", "was", "you",
    ];
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.len() >= 3 && !STOP_WORDS.contains(term))
        .map(str::to_owned)
        .collect()
}

fn memory_from_row(row: &Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: row.get(0)?,
        content: row.get(1)?,
        source: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        category: row.get(5)?,
        status: row.get(6)?,
        confidence: row.get(7)?,
        provenance: row.get(8)?,
        supersedes_memory_id: row.get(9)?,
    })
}
