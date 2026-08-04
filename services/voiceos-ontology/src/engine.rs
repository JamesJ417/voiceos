use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    Alias, AliasInput, CanonicalRequest, Catalog, Confidence, DecisionStatus,
    DeterministicResolver, InterpretationDecision, ModelCandidate, OntologyStore, RegressionCase,
    ResolutionSource, StoreError, ValidationIssue, ValidatorDisposition, ValidatorResult,
    normalize_phrase, validator_result,
};

pub trait ModelFallback: Send + Sync {
    fn resolve(&self, phrase: &str, catalog: &Catalog) -> Result<Option<ModelCandidate>, String>;
}

#[derive(Debug, Error)]
pub enum InterpreterError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("alias phrase cannot be empty")]
    EmptyAlias,
    #[error("alias entity is not in the ontology")]
    UnknownAliasEntity,
    #[error("corrected interpretation did not pass validation")]
    InvalidCorrection,
}

pub struct Interpreter {
    catalog: Catalog,
    resolver: DeterministicResolver,
    store: Arc<OntologyStore>,
    fallback: Option<Arc<dyn ModelFallback>>,
}

impl Interpreter {
    pub fn new(store: Arc<OntologyStore>) -> Self {
        Self {
            catalog: Catalog::seeded(),
            resolver: DeterministicResolver,
            store,
            fallback: None,
        }
    }

    pub fn with_fallback(mut self, fallback: Arc<dyn ModelFallback>) -> Self {
        self.fallback = Some(fallback);
        self
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn interpret(
        &self,
        owner_id: &str,
        phrase: &str,
    ) -> Result<InterpretationDecision, InterpreterError> {
        self.interpret_with_fallback(owner_id, phrase, true)
    }

    pub fn interpret_deterministic(
        &self,
        owner_id: &str,
        phrase: &str,
    ) -> Result<InterpretationDecision, InterpreterError> {
        self.interpret_with_fallback(owner_id, phrase, false)
    }

    fn interpret_with_fallback(
        &self,
        owner_id: &str,
        phrase: &str,
        allow_fallback: bool,
    ) -> Result<InterpretationDecision, InterpreterError> {
        let aliases = self.store.aliases(owner_id)?;
        let mut fallback_issue = None;
        let mut interpretation = self.resolver.resolve(phrase, &self.catalog, &aliases);
        if allow_fallback
            && interpretation.is_none()
            && let Some(fallback) = &self.fallback
        {
            match fallback.resolve(phrase, &self.catalog) {
                Ok(Some(candidate)) => {
                    let confidence_valid = candidate.confidence.is_finite()
                        && (0.0..=1.0).contains(&candidate.confidence);
                    interpretation = Some(CanonicalRequest {
                        intent: candidate.intent,
                        entities: candidate.entities,
                        arguments: candidate.arguments,
                        confidence: Confidence::new(candidate.confidence),
                        source: ResolutionSource::ModelFallback,
                    });
                    if !confidence_valid {
                        fallback_issue = Some(ValidationIssue {
                            field: "confidence".to_owned(),
                            code: "confidence_out_of_range".to_owned(),
                            message: "Model confidence must be between zero and one.".to_owned(),
                        });
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    fallback_issue = Some(ValidationIssue {
                        field: "fallback".to_owned(),
                        code: "model_fallback_unavailable".to_owned(),
                        message: error,
                    });
                }
            }
        }

        if let Some(request) = &mut interpretation {
            self.catalog.migrate_request(request);
        }

        let mut validator = validator_result(interpretation.as_ref(), &self.catalog);
        if let Some(issue) = fallback_issue {
            validator.issues.push(issue);
            validator.disposition = ValidatorDisposition::Reject;
            validator.reason = "model_fallback_validation_failed".to_owned();
        }
        let status = match validator.disposition {
            ValidatorDisposition::Execute => DecisionStatus::Resolved,
            ValidatorDisposition::AskForConfirmation => DecisionStatus::NeedsConfirmation,
            ValidatorDisposition::AskClarifyingQuestion => DecisionStatus::Unrecognized,
            ValidatorDisposition::Reject => DecisionStatus::Rejected,
        };
        let now = Utc::now().to_rfc3339();
        let decision = InterpretationDecision {
            id: Uuid::new_v4().to_string(),
            owner_id: owner_id.to_owned(),
            original_phrase: phrase.to_owned(),
            normalized_phrase: normalize_phrase(phrase),
            catalog_version: self.catalog.version(),
            interpretation,
            status: status.clone(),
            validation_issues: validator.issues.clone(),
            validator,
            corrections: vec![],
            final_decision: status,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.record(&decision)?;
        Ok(decision)
    }

    pub fn approve_alias(
        &self,
        owner_id: &str,
        input: &AliasInput,
    ) -> Result<Alias, InterpreterError> {
        if normalize_phrase(&input.phrase).is_empty() {
            return Err(InterpreterError::EmptyAlias);
        }
        if self
            .catalog
            .entity(&input.entity_kind, &input.entity_id)
            .is_none()
        {
            return Err(InterpreterError::UnknownAliasEntity);
        }
        Ok(self.store.approve_alias(
            owner_id,
            &input.phrase,
            input.entity_kind,
            &input.entity_id,
        )?)
    }

    pub fn aliases(&self, owner_id: &str) -> Result<Vec<Alias>, InterpreterError> {
        Ok(self.store.aliases(owner_id)?)
    }

    pub fn validate_tool(
        &self,
        owner_id: &str,
        tool: &str,
        arguments: BTreeMap<String, Value>,
        confidence: f32,
        source: ResolutionSource,
    ) -> Result<InterpretationDecision, InterpreterError> {
        let interpretation = self
            .catalog
            .request_for_tool(tool, arguments, confidence, source);
        let validator = interpretation.as_ref().map_or_else(
            || ValidatorResult {
                disposition: ValidatorDisposition::Reject,
                reason: "structured_tool_not_in_catalog".to_owned(),
                issues: vec![ValidationIssue {
                    field: "tool".to_owned(),
                    code: "unknown_tool".to_owned(),
                    message: format!("structured tool `{tool}` has no canonical ontology intent"),
                }],
            },
            |request| validator_result(Some(request), &self.catalog),
        );
        let status = match validator.disposition {
            ValidatorDisposition::Execute => DecisionStatus::Resolved,
            ValidatorDisposition::AskForConfirmation => DecisionStatus::NeedsConfirmation,
            ValidatorDisposition::AskClarifyingQuestion => DecisionStatus::Unrecognized,
            ValidatorDisposition::Reject => DecisionStatus::Rejected,
        };
        let now = Utc::now().to_rfc3339();
        let decision = InterpretationDecision {
            id: Uuid::new_v4().to_string(),
            owner_id: owner_id.to_owned(),
            original_phrase: format!("structured-tool:{tool}"),
            normalized_phrase: format!("structured-tool:{tool}"),
            catalog_version: self.catalog.version(),
            interpretation,
            status: status.clone(),
            validation_issues: validator.issues.clone(),
            validator,
            corrections: Vec::new(),
            final_decision: status,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.record(&decision)?;
        Ok(decision)
    }

    pub fn correct(
        &self,
        owner_id: &str,
        interpretation_id: &str,
        mut request: CanonicalRequest,
        note: &str,
    ) -> Result<InterpretationDecision, InterpreterError> {
        request.source = ResolutionSource::UserCorrection;
        request.confidence = Confidence::new(1.0);
        self.catalog.migrate_request(&mut request);
        let validator = validator_result(Some(&request), &self.catalog);
        if validator.disposition != ValidatorDisposition::Execute {
            return Err(InterpreterError::InvalidCorrection);
        }
        Ok(self.store.add_correction(
            owner_id,
            interpretation_id,
            request,
            note,
            self.catalog.version(),
            validator,
        )?)
    }

    pub fn get(
        &self,
        owner_id: &str,
        interpretation_id: &str,
    ) -> Result<Option<InterpretationDecision>, InterpreterError> {
        let Some(mut decision) = self.store.get(owner_id, interpretation_id)? else {
            return Ok(None);
        };
        if self.catalog.supports_version(decision.catalog_version)
            && decision.catalog_version < self.catalog.version()
        {
            if let Some(request) = &mut decision.interpretation {
                self.catalog.migrate_request(request);
            }
            for correction in &mut decision.corrections {
                self.catalog.migrate_request(&mut correction.request);
            }
            decision.catalog_version = self.catalog.version();
            decision.validator = validator_result(decision.interpretation.as_ref(), &self.catalog);
            decision.validation_issues = decision.validator.issues.clone();
        }
        Ok(Some(decision))
    }

    pub fn correction_regression_corpus(
        &self,
        owner_id: &str,
    ) -> Result<Vec<RegressionCase>, InterpreterError> {
        Ok(self.store.correction_regression_corpus(owner_id)?)
    }
}
