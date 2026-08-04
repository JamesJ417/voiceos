mod catalog;
mod engine;
mod model;
mod resolver;
mod store;
mod validation;

pub use catalog::Catalog;
pub use engine::{Interpreter, InterpreterError, ModelFallback};
pub use model::{
    Alias, AliasInput, ArgumentKind, ArgumentSpec, CanonicalRequest, Confidence, ConfidenceLevel,
    Correction, DecisionStatus, EntityDefinition, EntityKind, EntityRef, IntentDefinition,
    IntentId, InterpretationDecision, ModelCandidate, ResolutionSource, Unit, ValidationIssue,
};
pub use resolver::{DeterministicResolver, normalize_phrase};
pub use store::{OntologyStore, StoreError};
pub use validation::validate_request;
