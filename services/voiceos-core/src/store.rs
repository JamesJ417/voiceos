use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{ChatMessage, ConversationContext, ConversationMessage, DocumentRecord, Memory, Role};

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

pub struct ConversationStore {
    connection: Mutex<Connection>,
}

impl ConversationStore {
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

    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let connection = self.connection()?;
        crate::migrations::migrate(&connection)?;
        drop(connection);
        self.backfill_task_progress()?;
        Ok(())
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
        transaction.execute(
            "INSERT OR IGNORE INTO conversations(conversation_id, device_id, status, created_at, updated_at, owner_id) VALUES(?1, ?2, 'active', ?3, ?3, ?4)",
            params![conversation_id, device_id, now, owner_id.trim()],
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
            "SELECT m.message_id, m.conversation_id, m.role, m.content, m.provider, m.origin_device_id, m.created_at
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
                    role: parse_role(row.get::<_, String>(2)?),
                    content: row.get(3)?,
                    provider: row.get(4)?,
                    origin_device_id: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn recent_conversation_messages(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<ConversationMessage>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT message_id, conversation_id, role, content, provider, origin_device_id, created_at
             FROM (
                SELECT m.message_id, m.conversation_id, m.role, m.content, m.provider, m.origin_device_id, m.created_at
                FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id
                WHERE c.owner_id=?1 AND c.status='active'
                ORDER BY m.message_id DESC LIMIT ?2
             ) ORDER BY message_id",
        )?;
        let rows = statement.query_map(params![owner_id, limit.clamp(1, 500)], |row| {
            Ok(ConversationMessage {
                sequence: row.get(0)?,
                conversation_id: row.get(1)?,
                role: parse_role(row.get::<_, String>(2)?),
                content: row.get(3)?,
                provider: row.get(4)?,
                origin_device_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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
        self.connection()?.execute(
            "INSERT INTO memories(memory_id, device_id, normalized_content, content, source, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6) ON CONFLICT(device_id, normalized_content) DO UPDATE SET content=excluded.content, source=excluded.source, updated_at=excluded.updated_at",
            params![Uuid::new_v4().to_string(), device_id, normalized, content.trim(), source, now],
        )?;
        Ok(())
    }

    pub fn memories(&self, device_id: &str, limit: usize) -> Result<Vec<Memory>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT memory_id, content, source, created_at FROM memories WHERE device_id=?1 ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![device_id, limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                created_at: row.get(3)?,
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

    pub fn memories_for_owner(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<Memory>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT memory_id, content, source, created_at FROM memories WHERE owner_id=?1 ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![owner_id, limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                created_at: row.get(3)?,
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
        Ok(ConversationContext {
            conversation_id: conversation_id.to_owned(),
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
        Ok(ConversationContext {
            conversation_id: conversation_id.to_owned(),
            summary: self.summary(conversation_id)?.map(|value| value.0),
            memories: self.memories_for_owner(owner_id, memory_limit)?,
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
            let conversation_id =
                self.resolve_conversation(device_id, Some(&format!("legacy:{session_id}")))?;
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
