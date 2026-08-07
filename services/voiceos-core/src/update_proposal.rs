use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

use crate::{ConversationStore, StoreError, UpdateProposal};

impl ConversationStore {
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_update_proposal(
        &self,
        owner_id: &str,
        component: &str,
        current_version: &str,
        proposed_version: &str,
        release_notes: &str,
        dependency_changes: Value,
        api_changes: Value,
        configuration_changes: Value,
        skill_changes: Value,
        security_changes: Value,
        affected_components: Value,
        rollback_version: &str,
        candidate_path: Option<&str>,
        evidence: Value,
    ) -> Result<UpdateProposal, StoreError> {
        for (name, value) in [
            ("owner_id", owner_id),
            ("component", component),
            ("current_version", current_version),
            ("proposed_version", proposed_version),
            ("rollback_version", rollback_version),
        ] {
            if value.trim().is_empty() {
                return Err(StoreError::InvalidInput(format!("{name} is required")));
            }
        }
        for value in [
            &dependency_changes,
            &api_changes,
            &configuration_changes,
            &skill_changes,
            &security_changes,
            &affected_components,
            &evidence,
        ] {
            if !(value.is_array() || value.is_object()) {
                return Err(StoreError::InvalidInput(
                    "update change evidence must be structured".to_owned(),
                ));
            }
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        connection.execute("INSERT INTO owners(owner_id,created_at,updated_at) VALUES(?1,?2,?2) ON CONFLICT(owner_id) DO UPDATE SET updated_at=excluded.updated_at", params![owner_id.trim(), now])?;
        connection.execute(
            "INSERT INTO update_proposals(update_id,owner_id,component,current_version,proposed_version,status,release_notes,dependency_changes_json,api_changes_json,configuration_changes_json,skill_changes_json,security_changes_json,affected_components_json,rollback_version,candidate_path,evidence_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'discovered',?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?16) ON CONFLICT(owner_id,component,proposed_version) DO UPDATE SET release_notes=excluded.release_notes,dependency_changes_json=excluded.dependency_changes_json,api_changes_json=excluded.api_changes_json,configuration_changes_json=excluded.configuration_changes_json,skill_changes_json=excluded.skill_changes_json,security_changes_json=excluded.security_changes_json,affected_components_json=excluded.affected_components_json,candidate_path=COALESCE(excluded.candidate_path,update_proposals.candidate_path),evidence_json=excluded.evidence_json,updated_at=excluded.updated_at",
            params![id,owner_id.trim(),component,current_version,proposed_version,release_notes,dependency_changes.to_string(),api_changes.to_string(),configuration_changes.to_string(),skill_changes.to_string(),security_changes.to_string(),affected_components.to_string(),rollback_version,candidate_path,evidence.to_string(),now],
        )?;
        drop(connection);
        self.update_proposal_by_version(owner_id, component, proposed_version)?
            .ok_or_else(|| StoreError::InvalidInput("update proposal was not persisted".to_owned()))
    }

    pub fn update_proposals(
        &self,
        owner_id: &str,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<UpdateProposal>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT update_id,owner_id,component,current_version,proposed_version,status,release_notes,dependency_changes_json,api_changes_json,configuration_changes_json,skill_changes_json,security_changes_json,affected_components_json,rollback_version,candidate_path,evidence_json,created_at,updated_at FROM update_proposals WHERE owner_id=?1 AND (?2 IS NULL OR status=?2) ORDER BY updated_at DESC LIMIT ?3")?;
        statement
            .query_map(
                params![owner_id.trim(), status, limit.clamp(1, 200)],
                update_row,
            )?
            .map(|row| row.map_err(StoreError::from))
            .collect()
    }

    pub fn update_proposal(
        &self,
        owner_id: &str,
        id: &str,
    ) -> Result<Option<UpdateProposal>, StoreError> {
        self.connection()?.query_row("SELECT update_id,owner_id,component,current_version,proposed_version,status,release_notes,dependency_changes_json,api_changes_json,configuration_changes_json,skill_changes_json,security_changes_json,affected_components_json,rollback_version,candidate_path,evidence_json,created_at,updated_at FROM update_proposals WHERE owner_id=?1 AND update_id=?2",params![owner_id.trim(),id.trim()],update_row).optional().map_err(StoreError::from)
    }

    pub fn set_update_status(
        &self,
        owner_id: &str,
        id: &str,
        status: &str,
        candidate_path: Option<&str>,
    ) -> Result<Option<UpdateProposal>, StoreError> {
        if ![
            "discovered",
            "approved",
            "rejected",
            "candidate_ready",
            "deploying",
            "deployed",
            "failed",
            "rolled_back",
        ]
        .contains(&status)
        {
            return Err(StoreError::InvalidInput("invalid update status".to_owned()));
        }
        let changed = self.connection()?.execute("UPDATE update_proposals SET status=?3,candidate_path=COALESCE(?4,candidate_path),updated_at=?5 WHERE owner_id=?1 AND update_id=?2",params![owner_id.trim(),id.trim(),status,candidate_path,Utc::now().to_rfc3339()])?;
        if changed == 0 {
            return Ok(None);
        }
        self.update_proposal(owner_id, id)
    }

    fn update_proposal_by_version(
        &self,
        owner_id: &str,
        component: &str,
        version: &str,
    ) -> Result<Option<UpdateProposal>, StoreError> {
        self.connection()?.query_row("SELECT update_id,owner_id,component,current_version,proposed_version,status,release_notes,dependency_changes_json,api_changes_json,configuration_changes_json,skill_changes_json,security_changes_json,affected_components_json,rollback_version,candidate_path,evidence_json,created_at,updated_at FROM update_proposals WHERE owner_id=?1 AND component=?2 AND proposed_version=?3",params![owner_id.trim(),component,version],update_row).optional().map_err(StoreError::from)
    }
}

fn update_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UpdateProposal> {
    Ok(UpdateProposal {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        component: row.get(2)?,
        current_version: row.get(3)?,
        proposed_version: row.get(4)?,
        status: row.get(5)?,
        release_notes: row.get(6)?,
        dependency_changes: parse(row.get(7)?)?,
        api_changes: parse(row.get(8)?)?,
        configuration_changes: parse(row.get(9)?)?,
        skill_changes: parse(row.get(10)?)?,
        security_changes: parse(row.get(11)?)?,
        affected_components: parse(row.get(12)?)?,
        rollback_version: row.get(13)?,
        candidate_path: row.get(14)?,
        evidence: parse(row.get(15)?)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}
fn parse(value: String) -> rusqlite::Result<Value> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            value.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
