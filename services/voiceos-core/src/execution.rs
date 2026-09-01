use crate::{ConversationStore, StoreError};
use chrono::Utc;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionCheckpoint {
    pub id: String,
    pub job_id: String,
    pub owner_id: String,
    pub sequence: i64,
    pub state: Value,
    pub rollback: Value,
    pub created_at: String,
}

impl ConversationStore {
    pub fn acquire_capability_lease(
        &self,
        owner_id: &str,
        job_id: &str,
        capabilities: Value,
        ttl_seconds: i64,
    ) -> Result<String, StoreError> {
        require_execution_id("owner_id", owner_id)?;
        require_execution_id("job_id", job_id)?;
        if !capabilities.is_array() || !(1..=3600).contains(&ttl_seconds) {
            return Err(StoreError::InvalidInput("invalid capability lease".into()));
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let expires = (now + chrono::Duration::seconds(ttl_seconds)).to_rfc3339();
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE jobs SET lease_id=?3, lease_capabilities_json=?4, lease_expires_at=?5, updated_at=?6 WHERE owner_id=?1 AND job_id=?2 AND status IN ('approved','running','paused')",
            params![owner_id.trim(), job_id.trim(), id, capabilities.to_string(), expires, now_text],
        )?;
        if changed == 0 {
            return Err(StoreError::InvalidInput(
                "job is not executable or not owned".into(),
            ));
        }
        Ok(id)
    }

    pub fn checkpoint_execution(
        &self,
        owner_id: &str,
        job_id: &str,
        state: Value,
        rollback: Value,
    ) -> Result<ExecutionCheckpoint, StoreError> {
        require_execution_id("owner_id", owner_id)?;
        require_execution_id("job_id", job_id)?;
        if !state.is_object() || !rollback.is_object() {
            return Err(StoreError::InvalidInput(
                "checkpoint state and rollback must be objects".into(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let executable = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE owner_id=?1 AND job_id=?2 AND status IN ('approved','running','paused'))",
            params![owner_id.trim(), job_id.trim()],
            |row| row.get::<_, bool>(0),
        )?;
        if !executable {
            return Err(StoreError::InvalidInput(
                "job is not executable or not owned".into(),
            ));
        }
        let sequence: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(sequence),0)+1 FROM execution_checkpoints WHERE owner_id=?1 AND job_id=?2",
            params![owner_id.trim(), job_id.trim()],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO execution_checkpoints(checkpoint_id,owner_id,job_id,sequence,state_json,rollback_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![id, owner_id.trim(), job_id.trim(), sequence, state.to_string(), rollback.to_string(), now],
        )?;
        transaction.execute(
            "UPDATE jobs SET checkpoint_sequence=?3, updated_at=?4 WHERE owner_id=?1 AND job_id=?2",
            params![owner_id.trim(), job_id.trim(), sequence, now],
        )?;
        transaction.commit()?;
        Ok(ExecutionCheckpoint {
            id,
            job_id: job_id.trim().into(),
            owner_id: owner_id.trim().into(),
            sequence,
            state,
            rollback,
            created_at: now,
        })
    }

    /// Transitions an owned non-terminal job to cancelled. Repeated cancellation
    /// requests are successful and preserve the original cancellation reason.
    pub fn cancel_execution(
        &self,
        owner_id: &str,
        job_id: &str,
        reason: &str,
    ) -> Result<bool, StoreError> {
        require_execution_id("owner_id", owner_id)?;
        require_execution_id("job_id", job_id)?;
        require_execution_id("cancellation reason", reason)?;
        let now = Utc::now().to_rfc3339();
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE jobs SET status='cancelled', cancellation_reason=?3, updated_at=?4 WHERE owner_id=?1 AND job_id=?2 AND status NOT IN ('completed','failed','cancelled')",
            params![owner_id.trim(), job_id.trim(), reason.trim(), now],
        )?;
        if changed > 0 {
            return Ok(true);
        }
        connection
            .query_row(
                "SELECT status='cancelled' FROM jobs WHERE owner_id=?1 AND job_id=?2",
                params![owner_id.trim(), job_id.trim()],
                |row| row.get(0),
            )
            .optional()
            .map(|cancelled| cancelled.unwrap_or(false))
            .map_err(StoreError::from)
    }

    pub fn latest_execution_checkpoint(
        &self,
        owner_id: &str,
        job_id: &str,
    ) -> Result<Option<ExecutionCheckpoint>, StoreError> {
        require_execution_id("owner_id", owner_id)?;
        require_execution_id("job_id", job_id)?;
        let connection = self.connection()?;
        let raw: Option<(String, String, String, i64, String, String, String)> = connection
            .query_row(
                "SELECT checkpoint_id,job_id,owner_id,sequence,state_json,rollback_json,created_at FROM execution_checkpoints WHERE owner_id=?1 AND job_id=?2 ORDER BY sequence DESC LIMIT 1",
                params![owner_id.trim(), job_id.trim()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            )
            .optional()?;
        raw.map(checkpoint_from_row).transpose()
    }

    pub fn resume_execution(
        &self,
        owner_id: &str,
        job_id: &str,
    ) -> Result<Option<ExecutionCheckpoint>, StoreError> {
        require_execution_id("owner_id", owner_id)?;
        require_execution_id("job_id", job_id)?;
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resumable = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE owner_id=?1 AND job_id=?2 AND status IN ('paused','running'))",
            params![owner_id.trim(), job_id.trim()],
            |row| row.get::<_, bool>(0),
        )?;
        if !resumable {
            return Err(StoreError::InvalidInput(
                "job is not resumable or not owned".into(),
            ));
        }
        transaction.execute(
            "UPDATE jobs SET status='running', updated_at=?3 WHERE owner_id=?1 AND job_id=?2",
            params![owner_id.trim(), job_id.trim(), now],
        )?;
        let raw: Option<(String, String, String, i64, String, String, String)> = transaction
            .query_row(
                "SELECT checkpoint_id,job_id,owner_id,sequence,state_json,rollback_json,created_at FROM execution_checkpoints WHERE owner_id=?1 AND job_id=?2 ORDER BY sequence DESC LIMIT 1",
                params![owner_id.trim(), job_id.trim()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            )
            .optional()?;
        transaction.commit()?;
        raw.map(checkpoint_from_row).transpose()
    }
}

fn checkpoint_from_row(
    (id, job_id, owner_id, sequence, state, rollback, created_at): (
        String,
        String,
        String,
        i64,
        String,
        String,
        String,
    ),
) -> Result<ExecutionCheckpoint, StoreError> {
    Ok(ExecutionCheckpoint {
        id,
        job_id,
        owner_id,
        sequence,
        state: serde_json::from_str(&state)?,
        rollback: serde_json::from_str(&rollback)?,
        created_at,
    })
}

fn require_execution_id(label: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        return Err(StoreError::InvalidInput(format!("{label} is required")));
    }
    Ok(())
}
