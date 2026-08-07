use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::ConversationStore;
use crate::doctrine::{
    DoctrineCandidate, DoctrineError, DoctrineLens, DoctrineSourceProfile, DoctrineSourceRecord,
    DoctrineStatus,
};

pub(crate) struct DoctrineRepository {
    store: Arc<ConversationStore>,
}

impl DoctrineRepository {
    pub(crate) fn new(store: Arc<ConversationStore>) -> Self {
        Self { store }
    }

    pub(crate) fn source_profiles(
        &self,
        owner_id: &str,
    ) -> Result<Vec<DoctrineSourceProfile>, DoctrineError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT profile_id,internal_name,approved,visible_to_conversation,permitted_uses_json,prohibited_uses_json,domains_json,authorization_status,authorization_basis,ingestion_status,source_count,review_status,last_processed_at FROM doctrine_source_profiles WHERE owner_id=?1 ORDER BY profile_id")?;
        let rows = statement.query_map([owner_id], |row| {
            Ok(DoctrineSourceProfile {
                id: row.get(0)?,
                internal_name: row.get(1)?,
                approved: row.get::<_, i64>(2)? != 0,
                visible_to_conversation: row.get::<_, i64>(3)? != 0,
                permitted_uses: parse_vec(row.get(4)?),
                prohibited_uses: parse_vec(row.get(5)?),
                domains: parse_vec(row.get(6)?),
                authorization_status: row.get(7)?,
                authorization_basis: row.get(8)?,
                ingestion_status: row.get(9)?,
                source_count: row.get(10)?,
                review_status: row.get(11)?,
                last_processed_at: row.get(12)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn source_records(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<DoctrineSourceRecord>, DoctrineError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT record_id,profile_id,source_type,title,private_origin,publication_date,authorization_status,authorization_basis,content_sha256,storage_location,extraction_status,source_quality,duplicate_of,active,ingested_at FROM doctrine_source_records WHERE owner_id=?1 ORDER BY ingested_at DESC LIMIT ?2")?;
        Ok(statement
            .query_map(params![owner_id, limit.clamp(1, 200)], source_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn candidates(
        &self,
        owner_id: &str,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DoctrineCandidate>, DoctrineError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT candidate_id,normalized_proposition,domain,principle_type,decision_rule,rationale,applicable_conditions_json,exceptions_json,counterexamples_json,risk_posture,time_horizon,ethical_constraints_json,source_profile_diversity,extraction_model,extraction_prompt_version,confidence,abstraction_score,style_contamination_score,identity_contamination_score,status,review_requirement,protected,version,validation_errors_json,created_at,updated_at FROM doctrine_candidates WHERE owner_id=?1 AND (?2 IS NULL OR status=?2) ORDER BY updated_at DESC LIMIT ?3")?;
        let rows = statement.query_map(
            params![owner_id, status, limit.clamp(1, 200)],
            candidate_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn active_doctrine(
        &self,
        owner_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<DoctrineCandidate>, DoctrineError> {
        let terms = terms(query);
        let mut candidates = self.candidates(owner_id, Some("active"), 200)?;
        candidates.retain(|candidate| {
            terms.is_empty()
                || terms.iter().any(|term| {
                    candidate.domain.contains(term)
                        || candidate
                            .normalized_proposition
                            .to_lowercase()
                            .contains(term)
                })
        });
        candidates.truncate(limit.clamp(1, 50));
        Ok(candidates)
    }

    pub(crate) fn reasoning_lenses(&self, query: &str) -> Result<Vec<DoctrineLens>, DoctrineError> {
        let query_terms = terms(query);
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT lens_id,public_name,domains_json,description FROM doctrine_lenses WHERE active=1 ORDER BY lens_id")?;
        let mut lenses = statement
            .query_map([], |row| {
                let domains = parse_vec(row.get(2)?);
                let weight = domains
                    .iter()
                    .filter(|domain| {
                        query_terms
                            .iter()
                            .any(|term| domain.contains(term) || term.contains(domain.as_str()))
                    })
                    .count() as f64;
                Ok(DoctrineLens {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    domains,
                    description: row.get(3)?,
                    weight,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        lenses.sort_by(|a, b| b.weight.total_cmp(&a.weight).then_with(|| a.id.cmp(&b.id)));
        Ok(lenses
            .into_iter()
            .filter(|lens| lens.weight > 0.0 || query_terms.is_empty())
            .take(5)
            .collect())
    }

    pub(crate) fn status(&self, owner_id: &str) -> Result<DoctrineStatus, DoctrineError> {
        let connection = self.store.connection()?;
        let count = |sql: &str| -> Result<usize, rusqlite::Error> {
            connection.query_row(sql, [owner_id], |row| row.get(0))
        };
        Ok(DoctrineStatus {
            source_profiles: count("SELECT COUNT(*) FROM doctrine_source_profiles WHERE owner_id=?1")?,
            source_records: count("SELECT COUNT(*) FROM doctrine_source_records WHERE owner_id=?1")?,
            processed_records: count("SELECT COUNT(*) FROM doctrine_source_records WHERE owner_id=?1 AND extraction_status='processed'")?,
            authorization_warnings: count("SELECT COUNT(*) FROM doctrine_source_records WHERE owner_id=?1 AND (authorization_status<>'approved' OR active=0)")?,
            candidates_awaiting_review: count("SELECT COUNT(*) FROM doctrine_candidates WHERE owner_id=?1 AND status='awaiting_review'")?,
            active_doctrine: count("SELECT COUNT(*) FROM doctrine_candidates WHERE owner_id=?1 AND status='active'")?,
            contamination_failures: count("SELECT COUNT(*) FROM doctrine_candidates WHERE owner_id=?1 AND status='decontamination_failed'")?,
            open_contradictions: count("SELECT COUNT(*) FROM doctrine_contradictions WHERE owner_id=?1 AND status='open'")?,
            last_run_at: connection.query_row("SELECT completed_at FROM doctrine_runs WHERE owner_id=?1 ORDER BY completed_at DESC LIMIT 1",[owner_id],|row|row.get(0)).optional()?,
            latest_evaluation_status: connection.query_row("SELECT status FROM doctrine_evaluations WHERE owner_id=?1 ORDER BY created_at DESC LIMIT 1",[owner_id],|row|row.get(0)).optional()?,
        })
    }

    pub(crate) fn candidate_provenance(
        &self,
        owner_id: &str,
        candidate_id: &str,
    ) -> Result<Value, DoctrineError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT r.record_id,r.profile_id,r.title,r.private_origin,r.content_sha256,p.passage_id,s.evidence_role,s.directness FROM doctrine_candidate_sources s JOIN doctrine_source_passages p ON p.passage_id=s.passage_id JOIN doctrine_source_records r ON r.record_id=p.record_id JOIN doctrine_candidates c ON c.candidate_id=s.candidate_id WHERE c.owner_id=?1 AND c.candidate_id=?2 ORDER BY s.evidence_role,r.record_id,p.passage_index")?;
        let rows = statement.query_map(params![owner_id,candidate_id], |row| Ok(json!({"record_id":row.get::<_,String>(0)?,"profile_id":row.get::<_,String>(1)?,"title":row.get::<_,String>(2)?,"private_origin":row.get::<_,String>(3)?,"content_sha256":row.get::<_,String>(4)?,"passage_id":row.get::<_,String>(5)?,"role":row.get::<_,String>(6)?,"directness":row.get::<_,f64>(7)?})))?;
        Ok(Value::Array(rows.collect::<Result<Vec<_>, _>>()?))
    }

    pub(crate) fn contradictions(&self, owner_id: &str) -> Result<Value, DoctrineError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT contradiction_id,left_candidate_id,right_candidate_id,tension_kind,summary,conditions_json,status,resolution,created_at FROM doctrine_contradictions WHERE owner_id=?1 ORDER BY created_at DESC")?;
        let rows = statement.query_map([owner_id], |row| {
            let conditions: String = row.get(5)?;
            Ok(json!({
                "id":row.get::<_,String>(0)?,"left_candidate_id":row.get::<_,String>(1)?,
                "right_candidate_id":row.get::<_,String>(2)?,"tension_kind":row.get::<_,String>(3)?,
                "summary":row.get::<_,String>(4)?,
                "conditions":serde_json::from_str::<Value>(&conditions).unwrap_or(json!([])),
                "status":row.get::<_,String>(6)?,"resolution":row.get::<_,Option<String>>(7)?,
                "created_at":row.get::<_,String>(8)?
            }))
        })?;
        Ok(Value::Array(rows.collect::<Result<Vec<_>, _>>()?))
    }

    pub(crate) fn candidate(
        &self,
        owner_id: &str,
        id: &str,
    ) -> Result<Option<DoctrineCandidate>, DoctrineError> {
        let connection = self.store.connection()?;
        connection.query_row("SELECT candidate_id,normalized_proposition,domain,principle_type,decision_rule,rationale,applicable_conditions_json,exceptions_json,counterexamples_json,risk_posture,time_horizon,ethical_constraints_json,source_profile_diversity,extraction_model,extraction_prompt_version,confidence,abstraction_score,style_contamination_score,identity_contamination_score,status,review_requirement,protected,version,validation_errors_json,created_at,updated_at FROM doctrine_candidates WHERE owner_id=?1 AND candidate_id=?2",params![owner_id,id],candidate_row).optional().map_err(DoctrineError::from)
    }

    pub(crate) fn source_by_hash(
        &self,
        owner_id: &str,
        digest: &str,
    ) -> Result<Option<DoctrineSourceRecord>, DoctrineError> {
        let connection = self.store.connection()?;
        connection.query_row("SELECT record_id,profile_id,source_type,title,private_origin,publication_date,authorization_status,authorization_basis,content_sha256,storage_location,extraction_status,source_quality,duplicate_of,active,ingested_at FROM doctrine_source_records WHERE owner_id=?1 AND content_sha256=?2",params![owner_id,digest],source_row).optional().map_err(DoctrineError::from)
    }

    pub(crate) fn source_record(
        &self,
        owner_id: &str,
        id: &str,
    ) -> Result<Option<DoctrineSourceRecord>, DoctrineError> {
        let connection = self.store.connection()?;
        connection.query_row("SELECT record_id,profile_id,source_type,title,private_origin,publication_date,authorization_status,authorization_basis,content_sha256,storage_location,extraction_status,source_quality,duplicate_of,active,ingested_at FROM doctrine_source_records WHERE owner_id=?1 AND record_id=?2",params![owner_id,id],source_row).optional().map_err(DoctrineError::from)
    }

    pub(crate) fn passages(&self, record_id: &str) -> Result<Vec<(String, String)>, DoctrineError> {
        let connection = self.store.connection()?;
        let mut statement = connection.prepare("SELECT passage_id,content FROM doctrine_source_passages WHERE record_id=?1 ORDER BY passage_index")?;
        Ok(statement
            .query_map([record_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?)
    }
}

fn candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DoctrineCandidate> {
    Ok(DoctrineCandidate {
        id: row.get(0)?,
        normalized_proposition: row.get(1)?,
        domain: row.get(2)?,
        principle_type: row.get(3)?,
        decision_rule: row.get(4)?,
        rationale: row.get(5)?,
        applicable_conditions: parse_vec(row.get(6)?),
        exceptions: parse_vec(row.get(7)?),
        counterexamples: parse_vec(row.get(8)?),
        risk_posture: row.get(9)?,
        time_horizon: row.get(10)?,
        ethical_constraints: parse_vec(row.get(11)?),
        source_profile_diversity: row.get(12)?,
        extraction_model: row.get(13)?,
        extraction_prompt_version: row.get(14)?,
        confidence: row.get(15)?,
        abstraction_score: row.get(16)?,
        style_contamination_score: row.get(17)?,
        identity_contamination_score: row.get(18)?,
        status: row.get(19)?,
        review_requirement: row.get(20)?,
        protected: row.get::<_, i64>(21)? != 0,
        version: row.get(22)?,
        validation_errors: parse_vec(row.get(23)?),
        created_at: row.get(24)?,
        updated_at: row.get(25)?,
    })
}

fn source_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DoctrineSourceRecord> {
    Ok(DoctrineSourceRecord {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        source_type: row.get(2)?,
        title: row.get(3)?,
        private_origin: row.get(4)?,
        publication_date: row.get(5)?,
        authorization_status: row.get(6)?,
        authorization_basis: row.get(7)?,
        content_sha256: row.get(8)?,
        storage_location: row.get(9)?,
        extraction_status: row.get(10)?,
        source_quality: row.get(11)?,
        duplicate_of: row.get(12)?,
        active: row.get::<_, i64>(13)? != 0,
        ingested_at: row.get(14)?,
    })
}

fn parse_vec(value: String) -> Vec<String> {
    serde_json::from_str(&value).unwrap_or_default()
}

fn terms(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|value| value.len() > 2)
        .map(str::to_owned)
        .collect()
}
