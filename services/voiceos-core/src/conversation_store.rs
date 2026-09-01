use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, FixedOffset, Utc};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{
    ConversationArea, ConversationDay, ConversationExport, ConversationExportMessage,
    ConversationMessage, ConversationRecord, ConversationStore, ConversationSyncPayload,
    ConversationSyncRecord, GENERAL_TALK_AREA_ID, Role, StoreError, built_in_conversation_areas,
    is_valid_conversation_area,
};

impl ConversationStore {
    pub fn conversation_areas(&self) -> Vec<ConversationArea> {
        built_in_conversation_areas()
    }

    pub fn selected_area(&self, owner_id: &str) -> Result<String, StoreError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT area_id FROM owner_area_selections WHERE owner_id=?1",
                [owner_id],
                |row| row.get(0),
            )
            .optional()?
            .filter(|area_id: &String| is_valid_conversation_area(area_id))
            .unwrap_or_else(|| GENERAL_TALK_AREA_ID.to_owned()))
    }

    pub fn conversation_for_owner(
        &self,
        owner_id: &str,
        conversation_id: &str,
    ) -> Result<Option<ConversationRecord>, StoreError> {
        self.connection()?
            .query_row(
                &conversation_record_sql("c.owner_id=?1 AND c.conversation_id=?2"),
                params![owner_id, conversation_id],
                conversation_record_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn active_conversation_record(
        &self,
        owner_id: &str,
    ) -> Result<Option<ConversationRecord>, StoreError> {
        self.connection()?
            .query_row(
                &conversation_record_sql("c.owner_id=?1 AND c.status='active'"),
                [owner_id],
                conversation_record_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn conversations_for_owner(
        &self,
        owner_id: &str,
        area_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ConversationRecord>, StoreError> {
        if area_id.is_some_and(|id| !is_valid_conversation_area(id)) {
            return Err(StoreError::InvalidInput(
                "invalid conversation area".to_owned(),
            ));
        }
        let connection = self.connection()?;
        let sql = conversation_record_sql(
            "c.owner_id=?1 AND (?2 IS NULL OR c.area_id=?2) ORDER BY c.updated_at DESC,c.conversation_id LIMIT ?3",
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![owner_id, area_id, limit.clamp(1, 500) as i64],
            conversation_record_from_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_conversation_in_area(
        &self,
        owner_id: &str,
        device_id: &str,
        area_id: &str,
        title: Option<&str>,
        request_id: &str,
    ) -> Result<ConversationRecord, StoreError> {
        validate_mutation(owner_id, device_id, area_id, request_id)?;
        if let Some(record) = self.idempotent_record(owner_id, request_id, "create")? {
            return Ok(record);
        }
        self.ensure_owner_device(owner_id, device_id)?;
        let now = Utc::now().to_rfc3339();
        let conversation_id = Uuid::new_v4().to_string();
        let title = normalized_title(title.unwrap_or("New conversation"));
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE conversations SET status='archived' WHERE owner_id=?1 AND status='active'",
            [owner_id],
        )?;
        transaction.execute(
            "INSERT INTO conversations(conversation_id,device_id,status,created_at,updated_at,owner_id,area_id,title,area_updated_at,area_updated_by_device) VALUES(?1,?2,'active',?3,?3,?4,?5,?6,?3,?2)",
            params![conversation_id, device_id, now, owner_id, area_id, title],
        )?;
        transaction.execute(
            "INSERT INTO owner_area_selections(owner_id,area_id,conversation_id,updated_at,updated_by_device) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner_id) DO UPDATE SET area_id=excluded.area_id,conversation_id=excluded.conversation_id,updated_at=excluded.updated_at,updated_by_device=excluded.updated_by_device",
            params![owner_id, area_id, conversation_id, now, device_id],
        )?;
        audit(
            &transaction,
            owner_id,
            &conversation_id,
            "conversation.created",
            device_id,
            serde_json::json!({"area_id":area_id,"request_id":request_id}),
            &now,
        )?;
        let record = transaction.query_row(
            &conversation_record_sql("c.conversation_id=?1 AND c.owner_id=?2"),
            params![conversation_id, owner_id],
            conversation_record_from_row,
        )?;
        save_mutation(&transaction, owner_id, request_id, "create", &record, &now)?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn select_area_for_owner(
        &self,
        owner_id: &str,
        device_id: &str,
        area_id: &str,
        request_id: &str,
    ) -> Result<Option<ConversationRecord>, StoreError> {
        validate_mutation(owner_id, device_id, area_id, request_id)?;
        if let Some(response) =
            self.idempotent_optional_record(owner_id, request_id, "select_area")?
        {
            return Ok(response);
        }
        self.ensure_owner_device(owner_id, device_id)?;
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let selected: Option<String> = transaction
            .query_row(
                "SELECT conversation_id FROM conversations WHERE owner_id=?1 AND area_id=?2 ORDER BY updated_at DESC,conversation_id LIMIT 1",
                params![owner_id, area_id],
                |row| row.get(0),
            )
            .optional()?;
        transaction.execute(
            "UPDATE conversations SET status='archived' WHERE owner_id=?1 AND status='active'",
            [owner_id],
        )?;
        if let Some(conversation_id) = selected.as_deref() {
            transaction.execute(
                "UPDATE conversations SET status='active',updated_at=?1 WHERE owner_id=?2 AND conversation_id=?3",
                params![now, owner_id, conversation_id],
            )?;
        }
        transaction.execute(
            "INSERT INTO owner_area_selections(owner_id,area_id,conversation_id,updated_at,updated_by_device) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner_id) DO UPDATE SET area_id=excluded.area_id,conversation_id=excluded.conversation_id,updated_at=excluded.updated_at,updated_by_device=excluded.updated_by_device",
            params![owner_id, area_id, selected, now, device_id],
        )?;
        audit(
            &transaction,
            owner_id,
            selected.as_deref().unwrap_or(area_id),
            "conversation.area_selected",
            device_id,
            serde_json::json!({"area_id":area_id,"conversation_id":selected,"request_id":request_id}),
            &now,
        )?;
        let record = selected
            .as_deref()
            .map(|conversation_id| {
                transaction.query_row(
                    &conversation_record_sql("c.conversation_id=?1 AND c.owner_id=?2"),
                    params![conversation_id, owner_id],
                    conversation_record_from_row,
                )
            })
            .transpose()?;
        save_mutation(
            &transaction,
            owner_id,
            request_id,
            "select_area",
            &record,
            &now,
        )?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn select_conversation_for_owner(
        &self,
        owner_id: &str,
        device_id: &str,
        conversation_id: &str,
        request_id: &str,
    ) -> Result<ConversationRecord, StoreError> {
        if owner_id.trim().is_empty()
            || device_id.trim().is_empty()
            || conversation_id.trim().is_empty()
            || request_id.trim().is_empty()
        {
            return Err(StoreError::InvalidInput(
                "owner, device, conversation, and request IDs are required".to_owned(),
            ));
        }
        if let Some(record) = self.idempotent_record(owner_id, request_id, "select")? {
            return Ok(record);
        }
        let existing = self
            .conversation_for_owner(owner_id, conversation_id)?
            .ok_or_else(|| {
                StoreError::InvalidInput("conversation not found for owner".to_owned())
            })?;
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE conversations SET status='archived' WHERE owner_id=?1 AND status='active'",
            [owner_id],
        )?;
        transaction.execute(
            "UPDATE conversations SET status='active',updated_at=?1 WHERE owner_id=?2 AND conversation_id=?3",
            params![now, owner_id, conversation_id],
        )?;
        transaction.execute(
            "INSERT INTO owner_area_selections(owner_id,area_id,conversation_id,updated_at,updated_by_device) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner_id) DO UPDATE SET area_id=excluded.area_id,conversation_id=excluded.conversation_id,updated_at=excluded.updated_at,updated_by_device=excluded.updated_by_device",
            params![owner_id, existing.area_id, conversation_id, now, device_id],
        )?;
        audit(
            &transaction,
            owner_id,
            conversation_id,
            "conversation.selected",
            device_id,
            serde_json::json!({"area_id":existing.area_id,"request_id":request_id}),
            &now,
        )?;
        let record = transaction.query_row(
            &conversation_record_sql("c.conversation_id=?1 AND c.owner_id=?2"),
            params![conversation_id, owner_id],
            conversation_record_from_row,
        )?;
        save_mutation(&transaction, owner_id, request_id, "select", &record, &now)?;
        transaction.commit()?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn move_conversation_for_owner(
        &self,
        owner_id: &str,
        device_id: &str,
        conversation_id: &str,
        source_area_id: &str,
        destination_area_id: &str,
        confirmed: bool,
        request_id: &str,
    ) -> Result<ConversationRecord, StoreError> {
        validate_mutation(owner_id, device_id, destination_area_id, request_id)?;
        if !is_valid_conversation_area(source_area_id) || !confirmed {
            return Err(StoreError::InvalidInput(
                "a valid source area and explicit move confirmation are required".to_owned(),
            ));
        }
        if source_area_id == destination_area_id {
            return Err(StoreError::InvalidInput(
                "source and destination areas must differ".to_owned(),
            ));
        }
        if let Some(record) = self.idempotent_record(owner_id, request_id, "move")? {
            return Ok(record);
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE conversations SET area_id=?1,area_updated_at=?2,area_updated_by_device=?3,updated_at=?2 WHERE owner_id=?4 AND conversation_id=?5 AND area_id=?6",
            params![destination_area_id, now, device_id, owner_id, conversation_id, source_area_id],
        )?;
        if changed != 1 {
            return Err(StoreError::InvalidInput(
                "conversation is missing, foreign, or no longer in the confirmed source area"
                    .to_owned(),
            ));
        }
        transaction.execute(
            "UPDATE memories SET area_id=?1 WHERE owner_id=?2 AND conversation_id=?3",
            params![destination_area_id, owner_id, conversation_id],
        )?;
        transaction.execute(
            "UPDATE owner_area_selections SET area_id=?1,updated_at=?2,updated_by_device=?3 WHERE owner_id=?4 AND conversation_id=?5",
            params![destination_area_id, now, device_id, owner_id, conversation_id],
        )?;
        audit(
            &transaction,
            owner_id,
            conversation_id,
            "conversation.moved",
            device_id,
            serde_json::json!({"source_area_id":source_area_id,"destination_area_id":destination_area_id,"confirmed":true,"request_id":request_id}),
            &now,
        )?;
        let record = transaction.query_row(
            &conversation_record_sql("c.conversation_id=?1 AND c.owner_id=?2"),
            params![conversation_id, owner_id],
            conversation_record_from_row,
        )?;
        save_mutation(&transaction, owner_id, request_id, "move", &record, &now)?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn conversation_history_days(
        &self,
        owner_id: &str,
        area_id: Option<&str>,
        timezone_offset_minutes: i32,
        limit_days: usize,
    ) -> Result<Vec<ConversationDay>, StoreError> {
        if area_id.is_some_and(|id| !is_valid_conversation_area(id)) {
            return Err(StoreError::InvalidInput(
                "invalid conversation area".to_owned(),
            ));
        }
        let offset = FixedOffset::east_opt(timezone_offset_minutes.clamp(-1_080, 1_080) * 60)
            .ok_or_else(|| StoreError::InvalidInput("invalid timezone offset".to_owned()))?;
        let records = self.conversations_for_owner(owner_id, area_id, 500)?;
        let by_id = records
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect::<HashMap<_, _>>();
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT m.conversation_id,m.created_at,m.message_id FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1 AND (?2 IS NULL OR c.area_id=?2) ORDER BY m.message_id",
        )?;
        let rows = statement.query_map(params![owner_id, area_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut days: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
        for row in rows {
            let (conversation_id, created_at, sequence) = row?;
            let parsed = DateTime::parse_from_rfc3339(&created_at).map_err(|_| {
                StoreError::InvalidInput("stored message has an invalid timestamp".to_owned())
            })?;
            let date = parsed.with_timezone(&offset).date_naive().to_string();
            let entries = days.entry(date).or_default();
            if !entries.iter().any(|(_, id)| id == &conversation_id) {
                entries.push((sequence, conversation_id));
            }
        }
        Ok(days
            .into_iter()
            .rev()
            .take(limit_days.clamp(1, 366))
            .map(|(date, mut entries)| {
                entries.sort_by_key(|(sequence, _)| *sequence);
                ConversationDay {
                    date,
                    conversations: entries
                        .into_iter()
                        .filter_map(|(_, id)| by_id.get(&id).cloned())
                        .collect(),
                }
            })
            .collect())
    }

    pub fn export_conversation(
        &self,
        owner_id: &str,
        conversation_id: &str,
    ) -> Result<ConversationExport, StoreError> {
        let record = self
            .conversation_for_owner(owner_id, conversation_id)?
            .ok_or_else(|| {
                StoreError::InvalidInput("conversation not found for owner".to_owned())
            })?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT role,content,provider,origin_device_id,created_at FROM messages WHERE conversation_id=?1 ORDER BY message_id",
        )?;
        let rows = statement.query_map([conversation_id], |row| {
            Ok(ConversationExportMessage {
                role: parse_role(&row.get::<_, String>(0)?),
                content: row.get(1)?,
                provider: row.get(2)?,
                origin_device_id: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        Ok(ConversationExport {
            version: 1,
            export_id: Uuid::new_v4().to_string(),
            source_conversation_id: record.id,
            area_id: record.area_id,
            title: record.title,
            created_at: record.created_at,
            updated_at: record.updated_at,
            messages: rows.collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub fn import_conversation(
        &self,
        owner_id: &str,
        device_id: &str,
        import_id: &str,
        export: &ConversationExport,
    ) -> Result<ConversationRecord, StoreError> {
        validate_export(import_id, export)?;
        self.ensure_owner_device(owner_id, device_id)?;
        let existing_import = {
            self.connection()?.query_row(
                "SELECT conversation_id FROM conversation_imports WHERE owner_id=?1 AND import_id=?2",
                params![owner_id, import_id],
                |row| row.get::<_, String>(0),
            ).optional()?
        };
        if let Some(conversation_id) = existing_import {
            return self
                .conversation_for_owner(owner_id, &conversation_id)?
                .ok_or_else(|| {
                    StoreError::InvalidInput("idempotent import target is missing".to_owned())
                });
        }
        let now = Utc::now().to_rfc3339();
        let conversation_id = Uuid::new_v4().to_string();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO conversations(conversation_id,device_id,status,created_at,updated_at,owner_id,area_id,title,area_updated_at,area_updated_by_device) VALUES(?1,?2,'archived',?3,?4,?5,?6,?7,?4,?2)",
            params![conversation_id, device_id, export.created_at, export.updated_at, owner_id, export.area_id, normalized_title(&export.title)],
        )?;
        for message in &export.messages {
            transaction.execute(
                "INSERT INTO messages(conversation_id,role,content,provider,origin_device_id,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![conversation_id, role_name(&message.role), message.content.trim(), message.provider, message.origin_device_id, message.created_at],
            )?;
        }
        transaction.execute(
            "INSERT INTO conversation_imports(owner_id,import_id,source_conversation_id,conversation_id,imported_at) VALUES(?1,?2,?3,?4,?5)",
            params![owner_id, import_id, export.source_conversation_id, conversation_id, now],
        )?;
        audit(
            &transaction,
            owner_id,
            &conversation_id,
            "conversation.imported",
            device_id,
            serde_json::json!({"import_id":import_id,"source_conversation_id":export.source_conversation_id,"area_id":export.area_id}),
            &now,
        )?;
        let record = transaction.query_row(
            &conversation_record_sql("c.conversation_id=?1 AND c.owner_id=?2"),
            params![conversation_id, owner_id],
            conversation_record_from_row,
        )?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn conversation_sync_payload(
        &self,
        owner_id: &str,
        after_sequence: i64,
        limit: usize,
    ) -> Result<ConversationSyncPayload, StoreError> {
        let records = self.conversations_for_owner(owner_id, None, 500)?;
        let conversations = records
            .iter()
            .map(|record| ConversationSyncRecord {
                conversation_id: record.id.clone(),
                area_id: record.area_id.clone(),
                area_updated_at: record.area_updated_at.clone(),
                area_updated_by_device: record.area_updated_by_device.clone(),
            })
            .collect();
        let messages = self.owner_messages(owner_id, after_sequence, limit)?;
        let cursor = messages
            .last()
            .map(|message| message.sequence)
            .unwrap_or(after_sequence.max(0));
        Ok(ConversationSyncPayload {
            cursor,
            selected_area_id: self.selected_area(owner_id)?,
            active_conversation_id: records
                .iter()
                .find(|record| record.status == "active")
                .map(|record| record.id.clone()),
            conversations,
            messages,
        })
    }

    pub fn messages_for_owner_conversation(
        &self,
        owner_id: &str,
        conversation_id: &str,
        after_sequence: i64,
        limit: usize,
    ) -> Result<Vec<ConversationMessage>, StoreError> {
        if self
            .conversation_for_owner(owner_id, conversation_id)?
            .is_none()
        {
            return Err(StoreError::InvalidInput(
                "conversation not found for owner".to_owned(),
            ));
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT m.message_id,m.conversation_id,c.area_id,m.role,m.content,m.provider,m.origin_device_id,m.created_at FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1 AND c.conversation_id=?2 AND m.message_id>?3 ORDER BY m.message_id LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![
                owner_id,
                conversation_id,
                after_sequence.max(0),
                limit.clamp(1, 500) as i64
            ],
            |row| {
                Ok(ConversationMessage {
                    sequence: row.get(0)?,
                    conversation_id: row.get(1)?,
                    area_id: row.get(2)?,
                    role: parse_role(&row.get::<_, String>(3)?),
                    content: row.get(4)?,
                    provider: row.get(5)?,
                    origin_device_id: row.get(6)?,
                    created_at: row.get(7)?,
                    attachments: Vec::new(),
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn apply_conversation_sync(
        &self,
        owner_id: &str,
        device_id: &str,
        records: &[ConversationSyncRecord],
    ) -> Result<usize, StoreError> {
        self.ensure_owner_device(owner_id, device_id)?;
        if records.len() > 500 {
            return Err(StoreError::InvalidInput("too many sync records".to_owned()));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut applied = 0;
        for incoming in records {
            if !is_valid_conversation_area(&incoming.area_id)
                || incoming.area_updated_by_device.trim().is_empty()
                || DateTime::parse_from_rfc3339(&incoming.area_updated_at).is_err()
            {
                return Err(StoreError::InvalidInput("invalid sync record".to_owned()));
            }
            let local: Option<(String, String, String)> = transaction
                .query_row(
                    "SELECT area_id,area_updated_at,area_updated_by_device FROM conversations WHERE owner_id=?1 AND conversation_id=?2",
                    params![owner_id, incoming.conversation_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let Some((local_area, local_at, local_device)) = local else {
                continue;
            };
            if (
                incoming.area_updated_at.as_str(),
                incoming.area_updated_by_device.as_str(),
            ) > (local_at.as_str(), local_device.as_str())
            {
                transaction.execute(
                    "UPDATE conversations SET area_id=?1,area_updated_at=?2,area_updated_by_device=?3,updated_at=max(updated_at,?2) WHERE owner_id=?4 AND conversation_id=?5",
                    params![incoming.area_id, incoming.area_updated_at, incoming.area_updated_by_device, owner_id, incoming.conversation_id],
                )?;
                transaction.execute(
                    "UPDATE memories SET area_id=?1 WHERE owner_id=?2 AND conversation_id=?3",
                    params![incoming.area_id, owner_id, incoming.conversation_id],
                )?;
                transaction.execute(
                    "UPDATE owner_area_selections SET area_id=?1,updated_at=?2,updated_by_device=?3 WHERE owner_id=?4 AND conversation_id=?5",
                    params![incoming.area_id, incoming.area_updated_at, incoming.area_updated_by_device, owner_id, incoming.conversation_id],
                )?;
                if incoming.area_id != local_area {
                    audit(
                        &transaction,
                        owner_id,
                        &incoming.conversation_id,
                        "conversation.area_synced",
                        device_id,
                        serde_json::json!({
                            "source_area_id": local_area,
                            "destination_area_id": incoming.area_id,
                            "source_device_id": incoming.area_updated_by_device,
                            "conflict_order": "area_updated_at_then_device_id",
                        }),
                        &Utc::now().to_rfc3339(),
                    )?;
                }
                applied += 1;
            }
        }
        transaction.commit()?;
        Ok(applied)
    }

    fn owner_messages(
        &self,
        owner_id: &str,
        after_sequence: i64,
        limit: usize,
    ) -> Result<Vec<ConversationMessage>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT m.message_id,m.conversation_id,c.area_id,m.role,m.content,m.provider,m.origin_device_id,m.created_at FROM messages m JOIN conversations c ON c.conversation_id=m.conversation_id WHERE c.owner_id=?1 AND m.message_id>?2 ORDER BY m.message_id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                owner_id,
                after_sequence.max(0),
                limit.clamp(1, 1_000) as i64
            ],
            |row| {
                Ok(ConversationMessage {
                    sequence: row.get(0)?,
                    conversation_id: row.get(1)?,
                    area_id: row.get(2)?,
                    role: parse_role(&row.get::<_, String>(3)?),
                    content: row.get(4)?,
                    provider: row.get(5)?,
                    origin_device_id: row.get(6)?,
                    created_at: row.get(7)?,
                    attachments: Vec::new(),
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn idempotent_record(
        &self,
        owner_id: &str,
        request_id: &str,
        operation: &str,
    ) -> Result<Option<ConversationRecord>, StoreError> {
        self.idempotent_json(owner_id, request_id, operation)?
            .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
            .transpose()
    }

    fn idempotent_optional_record(
        &self,
        owner_id: &str,
        request_id: &str,
        operation: &str,
    ) -> Result<Option<Option<ConversationRecord>>, StoreError> {
        self.idempotent_json(owner_id, request_id, operation)?
            .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
            .transpose()
    }

    fn idempotent_json(
        &self,
        owner_id: &str,
        request_id: &str,
        operation: &str,
    ) -> Result<Option<String>, StoreError> {
        let existing: Option<(String, String)> = self
            .connection()?
            .query_row(
                "SELECT operation,response_json FROM conversation_mutations WHERE owner_id=?1 AND request_id=?2",
                params![owner_id, request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((stored_operation, _)) = existing.as_ref()
            && stored_operation != operation
        {
            return Err(StoreError::InvalidInput(
                "request ID was already used for a different operation".to_owned(),
            ));
        }
        Ok(existing.map(|(_, json)| json))
    }
}

fn conversation_record_sql(predicate: &str) -> String {
    format!(
        "SELECT c.conversation_id,c.owner_id,c.area_id,c.title,c.status,\
         (SELECT COUNT(*) FROM messages m WHERE m.conversation_id=c.conversation_id),\
         (SELECT substr(m.content,1,160) FROM messages m WHERE m.conversation_id=c.conversation_id ORDER BY m.message_id DESC LIMIT 1),\
         c.created_at,c.updated_at,c.area_updated_at,c.area_updated_by_device FROM conversations c WHERE {predicate}"
    )
}

fn conversation_record_from_row(row: &Row<'_>) -> rusqlite::Result<ConversationRecord> {
    Ok(ConversationRecord {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        area_id: row.get(2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        message_count: row.get::<_, i64>(5)?.max(0) as u64,
        last_message_preview: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        area_updated_at: row.get(9)?,
        area_updated_by_device: row.get(10)?,
    })
}

fn validate_mutation(
    owner_id: &str,
    device_id: &str,
    area_id: &str,
    request_id: &str,
) -> Result<(), StoreError> {
    if owner_id.trim().is_empty() || device_id.trim().is_empty() || request_id.trim().is_empty() {
        return Err(StoreError::InvalidInput(
            "owner, device, and request IDs are required".to_owned(),
        ));
    }
    if !is_valid_conversation_area(area_id) {
        return Err(StoreError::InvalidInput(
            "invalid conversation area".to_owned(),
        ));
    }
    Ok(())
}

fn normalized_title(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        "New conversation".to_owned()
    } else {
        title.chars().take(120).collect()
    }
}

fn save_mutation<T: serde::Serialize>(
    transaction: &rusqlite::Transaction<'_>,
    owner_id: &str,
    request_id: &str,
    operation: &str,
    response: &T,
    now: &str,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO conversation_mutations(owner_id,request_id,operation,response_json,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![owner_id, request_id, operation, serde_json::to_string(response)?, now],
    )?;
    Ok(())
}

fn audit(
    transaction: &rusqlite::Transaction<'_>,
    owner_id: &str,
    stream_id: &str,
    event_type: &str,
    actor: &str,
    payload: serde_json::Value,
    occurred_at: &str,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO execution_events(owner_id,stream_id,event_type,actor,payload_json,occurred_at) VALUES(?1,?2,?3,?4,?5,?6)",
        params![owner_id, stream_id, event_type, actor, serde_json::to_string(&payload)?, occurred_at],
    )?;
    Ok(())
}

fn validate_export(import_id: &str, export: &ConversationExport) -> Result<(), StoreError> {
    if import_id.trim().is_empty()
        || export.version != 1
        || export.export_id.trim().is_empty()
        || export.source_conversation_id.trim().is_empty()
        || !is_valid_conversation_area(&export.area_id)
        || export.messages.len() > 10_000
        || DateTime::parse_from_rfc3339(&export.created_at).is_err()
        || DateTime::parse_from_rfc3339(&export.updated_at).is_err()
    {
        return Err(StoreError::InvalidInput(
            "conversation import envelope is invalid".to_owned(),
        ));
    }
    for message in &export.messages {
        if message.content.trim().is_empty()
            || message.content.chars().count() > 32_000
            || DateTime::parse_from_rfc3339(&message.created_at).is_err()
        {
            return Err(StoreError::InvalidInput(
                "conversation import contains an invalid message".to_owned(),
            ));
        }
    }
    Ok(())
}

fn role_name(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn parse_role(value: &str) -> Role {
    match value {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}
