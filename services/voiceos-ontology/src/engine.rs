use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    Alias, AliasInput, CanonicalRequest, Catalog, Confidence, DecisionStatus,
    DeterministicResolver, InterpretationDecision, ModelCandidate, OntologyStore, ResolutionSource,
    StoreError, ValidationIssue, normalize_phrase, validate_request,
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
                Ok(Some(candidate)) if candidate.intent.0 == "personal.capture" => {}
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

        let mut issues = interpretation
            .as_ref()
            .map(|request| validate_request(request, &self.catalog))
            .unwrap_or_default();
        if let Some(issue) = fallback_issue {
            issues.push(issue);
        }
        let status = match &interpretation {
            None => DecisionStatus::Unrecognized,
            Some(_) if !issues.is_empty() => DecisionStatus::Rejected,
            Some(request) if request.confidence.score < 0.8 => DecisionStatus::NeedsConfirmation,
            Some(_) => DecisionStatus::Resolved,
        };
        let now = Utc::now().to_rfc3339();
        let decision = InterpretationDecision {
            id: Uuid::new_v4().to_string(),
            owner_id: owner_id.to_owned(),
            original_phrase: phrase.to_owned(),
            normalized_phrase: normalize_phrase(phrase),
            interpretation,
            status: status.clone(),
            validation_issues: issues,
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
            input.entity_kind.clone(),
            &input.entity_id,
        )?)
    }

    pub fn aliases(&self, owner_id: &str) -> Result<Vec<Alias>, InterpreterError> {
        Ok(self.store.aliases(owner_id)?)
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
        if !validate_request(&request, &self.catalog).is_empty() {
            return Err(InterpreterError::InvalidCorrection);
        }
        Ok(self.store.add_correction(
            owner_id,
            interpretation_id,
            request,
            note,
            DecisionStatus::Resolved,
        )?)
    }

    pub fn get(
        &self,
        owner_id: &str,
        interpretation_id: &str,
    ) -> Result<Option<InterpretationDecision>, InterpreterError> {
        Ok(self.store.get(owner_id, interpretation_id)?)
    }
}
