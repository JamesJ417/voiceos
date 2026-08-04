use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::{
    Alias, CanonicalRequest, Correction, DecisionStatus, EntityKind, EntityRef,
    InterpretationDecision, RegressionCase, ValidatorDisposition, ValidatorResult,
    normalize_phrase,
};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("ontology database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("ontology database serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ontology database lock is poisoned")]
    LockPoisoned,
    #[error("interpretation was not found")]
    InterpretationNotFound,
}

pub struct OntologyStore {
    connection: Mutex<Connection>,
}

impl OntologyStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                StoreError::Sql(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        let store = Self {
            connection: Mutex::new(Connection::open_in_memory()?),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn aliases(&self, owner_id: &str) -> Result<Vec<Alias>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT phrase, entity_kind, entity_id, approved_at FROM ontology_aliases WHERE owner_id=?1 ORDER BY phrase",
        )?;
        let rows = statement.query_map([owner_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (phrase, kind, id, approved_at) = row?;
            Ok(Alias {
                owner_id: owner_id.to_owned(),
                phrase,
                entity: EntityRef {
                    kind: serde_json::from_str(&format!("\"{kind}\""))?,
                    id,
                    surface: None,
                },
                approved_at,
            })
        })
        .collect()
    }

    pub fn approve_alias(
        &self,
        owner_id: &str,
        phrase: &str,
        kind: EntityKind,
        entity_id: &str,
    ) -> Result<Alias, StoreError> {
        let phrase = normalize_phrase(phrase);
        let now = Utc::now().to_rfc3339();
        let kind_name = serde_json::to_value(kind)?
            .as_str()
            .expect("entity kind serializes as a string")
            .to_owned();
        self.connection()?.execute(
            "INSERT INTO ontology_aliases(owner_id, phrase, entity_kind, entity_id, approved_at) VALUES(?1, ?2, ?3, ?4, ?5) ON CONFLICT(owner_id, phrase) DO UPDATE SET entity_kind=excluded.entity_kind, entity_id=excluded.entity_id, approved_at=excluded.approved_at",
            params![owner_id, phrase, kind_name, entity_id, now],
        )?;
        Ok(Alias {
            owner_id: owner_id.to_owned(),
            phrase,
            entity: EntityRef {
                kind,
                id: entity_id.to_owned(),
                surface: None,
            },
            approved_at: now,
        })
    }

    pub fn record(&self, decision: &InterpretationDecision) -> Result<(), StoreError> {
        self.connection()?.execute(
            "INSERT INTO ontology_interpretations(interpretation_id, owner_id, original_phrase, normalized_phrase, catalog_version, interpretation_json, status, validation_json, validator_json, corrections_json, final_decision, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                decision.id,
                decision.owner_id,
                decision.original_phrase,
                decision.normalized_phrase,
                decision.catalog_version,
                serde_json::to_string(&decision.interpretation)?,
                status_name(&decision.status),
                serde_json::to_string(&decision.validation_issues)?,
                serde_json::to_string(&decision.validator)?,
                serde_json::to_string(&decision.corrections)?,
                status_name(&decision.final_decision),
                decision.created_at,
                decision.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn add_correction(
        &self,
        owner_id: &str,
        interpretation_id: &str,
        request: CanonicalRequest,
        note: &str,
        catalog_version: u32,
        validator: ValidatorResult,
    ) -> Result<InterpretationDecision, StoreError> {
        let mut decision = self
            .get(owner_id, interpretation_id)?
            .ok_or(StoreError::InterpretationNotFound)?;
        let now = Utc::now().to_rfc3339();
        decision.corrections.push(Correction {
            request: request.clone(),
            note: note.trim().to_owned(),
            created_at: now.clone(),
        });
        decision.interpretation = Some(request);
        decision.status = DecisionStatus::Resolved;
        decision.final_decision = DecisionStatus::Resolved;
        decision.catalog_version = catalog_version;
        decision.validation_issues = validator.issues.clone();
        decision.validator = validator;
        decision.updated_at = now;
        self.connection()?.execute(
            "UPDATE ontology_interpretations SET catalog_version=?1, interpretation_json=?2, status=?3, validation_json=?4, validator_json=?5, corrections_json=?6, final_decision=?7, updated_at=?8 WHERE interpretation_id=?9 AND owner_id=?10",
            params![
                decision.catalog_version,
                serde_json::to_string(&decision.interpretation)?,
                status_name(&decision.status),
                serde_json::to_string(&decision.validation_issues)?,
                serde_json::to_string(&decision.validator)?,
                serde_json::to_string(&decision.corrections)?,
                status_name(&decision.final_decision),
                decision.updated_at,
                interpretation_id,
                owner_id,
            ],
        )?;
        Ok(decision)
    }

    pub fn get(
        &self,
        owner_id: &str,
        interpretation_id: &str,
    ) -> Result<Option<InterpretationDecision>, StoreError> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT original_phrase, normalized_phrase, catalog_version, interpretation_json, status, validation_json, validator_json, corrections_json, final_decision, created_at, updated_at FROM ontology_interpretations WHERE interpretation_id=?1 AND owner_id=?2",
                params![interpretation_id, owner_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?, row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?, row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?, row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                },
            )
            .optional()?;
        row.map(|row| {
            Ok(InterpretationDecision {
                id: interpretation_id.to_owned(),
                owner_id: owner_id.to_owned(),
                original_phrase: row.0,
                normalized_phrase: row.1,
                catalog_version: row.2,
                interpretation: serde_json::from_str(&row.3)?,
                status: parse_status(&row.4),
                validation_issues: serde_json::from_str(&row.5)?,
                validator: serde_json::from_str(&row.6)?,
                corrections: serde_json::from_str(&row.7)?,
                final_decision: parse_status(&row.8),
                created_at: row.9,
                updated_at: row.10,
            })
        })
        .transpose()
    }

    pub fn correction_regression_corpus(
        &self,
        owner_id: &str,
    ) -> Result<Vec<RegressionCase>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT original_phrase, corrections_json FROM ontology_interpretations WHERE owner_id=?1 AND corrections_json != '[]' ORDER BY created_at",
        )?;
        let rows = statement.query_map([owner_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut corpus = Vec::new();
        for row in rows {
            let (phrase, corrections_json) = row?;
            let corrections: Vec<Correction> = serde_json::from_str(&corrections_json)?;
            corpus.extend(corrections.into_iter().map(|correction| RegressionCase {
                phrase: phrase.clone(),
                expected_intent: correction.request.intent,
                expected_disposition: ValidatorDisposition::Execute,
                corrected: true,
            }));
        }
        Ok(corpus)
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let connection = self.connection()?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS ontology_aliases (
                owner_id TEXT NOT NULL,
                phrase TEXT NOT NULL,
                entity_kind TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                approved_at TEXT NOT NULL,
                PRIMARY KEY(owner_id, phrase)
            );
            CREATE TABLE IF NOT EXISTS ontology_interpretations (
                interpretation_id TEXT PRIMARY KEY,
                owner_id TEXT NOT NULL,
                original_phrase TEXT NOT NULL,
                normalized_phrase TEXT NOT NULL,
                catalog_version INTEGER NOT NULL DEFAULT 1,
                interpretation_json TEXT NOT NULL,
                status TEXT NOT NULL,
                validation_json TEXT NOT NULL,
                validator_json TEXT NOT NULL DEFAULT '{"disposition":"ask_clarifying_question","reason":"catalog_migration_requires_revalidation","issues":[]}',
                corrections_json TEXT NOT NULL,
                final_decision TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS ontology_interpretations_owner_idx
                ON ontology_interpretations(owner_id, created_at);
            "#,
        )?;
        ensure_column(
            &connection,
            "ontology_interpretations",
            "catalog_version",
            "INTEGER NOT NULL DEFAULT 1",
        )?;
        ensure_column(
            &connection,
            "ontology_interpretations",
            "validator_json",
            "TEXT NOT NULL DEFAULT '{\"disposition\":\"ask_clarifying_question\",\"reason\":\"catalog_migration_requires_revalidation\",\"issues\":[]}'",
        )?;
        Ok(())
    }
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), StoreError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !names.iter().any(|name| name == column) {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
    Ok(())
}

fn status_name(status: &DecisionStatus) -> &'static str {
    match status {
        DecisionStatus::Resolved => "resolved",
        DecisionStatus::NeedsConfirmation => "needs_confirmation",
        DecisionStatus::Unrecognized => "unrecognized",
        DecisionStatus::Rejected => "rejected",
    }
}

fn parse_status(status: &str) -> DecisionStatus {
    match status {
        "resolved" => DecisionStatus::Resolved,
        "needs_confirmation" => DecisionStatus::NeedsConfirmation,
        "rejected" => DecisionStatus::Rejected,
        _ => DecisionStatus::Unrecognized,
    }
}
