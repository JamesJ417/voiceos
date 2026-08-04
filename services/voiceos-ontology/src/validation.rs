use std::collections::HashSet;

use crate::{
    ArgumentKind, CanonicalRequest, Catalog, EntityKind, ValidationIssue, ValidatorDisposition,
    ValidatorResult,
};

pub fn validator_result(request: Option<&CanonicalRequest>, catalog: &Catalog) -> ValidatorResult {
    let Some(request) = request else {
        return ValidatorResult {
            disposition: ValidatorDisposition::AskClarifyingQuestion,
            reason: "intent_not_recognized".to_owned(),
            issues: Vec::new(),
        };
    };
    let issues = validate_request(request, catalog);
    if !issues.is_empty() {
        let only_missing = issues
            .iter()
            .all(|issue| issue.code == "required_argument_missing");
        return ValidatorResult {
            disposition: if only_missing {
                ValidatorDisposition::AskClarifyingQuestion
            } else {
                ValidatorDisposition::Reject
            },
            reason: if only_missing {
                "required_information_missing"
            } else {
                "ontology_validation_failed"
            }
            .to_owned(),
            issues,
        };
    }
    let definition = catalog
        .intent(&request.intent)
        .expect("validated intent must exist in catalog");
    if definition.approval_required || request.confidence.score < 0.8 {
        return ValidatorResult {
            disposition: ValidatorDisposition::AskForConfirmation,
            reason: if definition.approval_required {
                "intent_requires_confirmation"
            } else {
                "confidence_requires_confirmation"
            }
            .to_owned(),
            issues,
        };
    }
    ValidatorResult {
        disposition: ValidatorDisposition::Execute,
        reason: "validated_for_execution".to_owned(),
        issues,
    }
}

pub fn validate_request(request: &CanonicalRequest, catalog: &Catalog) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let Some(definition) = catalog.intent(&request.intent) else {
        issues.push(issue(
            "intent",
            "intent_not_allowlisted",
            "The intent is not in the VoiceOS ontology.",
        ));
        return issues;
    };

    if !request.confidence.score.is_finite() || !(0.0..=1.0).contains(&request.confidence.score) {
        issues.push(issue(
            "confidence",
            "confidence_out_of_range",
            "Confidence must be between zero and one.",
        ));
    }

    let allowed: HashSet<_> = definition
        .arguments
        .iter()
        .map(|argument| argument.name.as_str())
        .collect();
    for name in request.arguments.keys() {
        if !allowed.contains(name.as_str()) {
            issues.push(issue(
                name,
                "argument_not_allowlisted",
                "The argument is not valid for this intent.",
            ));
        }
    }

    for spec in &definition.arguments {
        let value = request.arguments.get(&spec.name);
        if value.is_none() && spec.required {
            issues.push(issue(
                &spec.name,
                "required_argument_missing",
                "A required argument is missing.",
            ));
            continue;
        }
        let Some(value) = value else { continue };
        match &spec.kind {
            ArgumentKind::String => match value.as_str() {
                None => issues.push(issue(
                    &spec.name,
                    "argument_type_invalid",
                    "The argument must be a string.",
                )),
                Some(text) if text.trim().is_empty() => issues.push(issue(
                    &spec.name,
                    "argument_empty",
                    "The argument cannot be empty.",
                )),
                Some(text)
                    if !spec.allowed_values.is_empty()
                        && !spec.allowed_values.iter().any(|allowed| allowed == text) =>
                {
                    issues.push(issue(
                        &spec.name,
                        "argument_value_not_allowlisted",
                        "The argument value is not allowlisted.",
                    ));
                }
                Some(_) => {}
            },
            ArgumentKind::Number => match value.as_f64() {
                None => issues.push(issue(
                    &spec.name,
                    "argument_type_invalid",
                    "The argument must be a number.",
                )),
                Some(number) if !number.is_finite() => issues.push(issue(
                    &spec.name,
                    "argument_number_invalid",
                    "The number must be finite.",
                )),
                Some(number) => {
                    if spec.minimum.is_some_and(|minimum| number < minimum)
                        || spec.maximum.is_some_and(|maximum| number > maximum)
                    {
                        issues.push(issue(
                            &spec.name,
                            "argument_out_of_range",
                            "The number is outside the permitted range.",
                        ));
                    }
                }
            },
            ArgumentKind::Boolean => {
                if !value.is_boolean() {
                    issues.push(issue(
                        &spec.name,
                        "argument_type_invalid",
                        "The argument must be true or false.",
                    ));
                }
            }
            ArgumentKind::Object => {
                if !value.is_object() {
                    issues.push(issue(
                        &spec.name,
                        "argument_type_invalid",
                        "The argument must be an object.",
                    ));
                }
            }
            ArgumentKind::Array => {
                if !value.is_array() {
                    issues.push(issue(
                        &spec.name,
                        "argument_type_invalid",
                        "The argument must be an array.",
                    ));
                }
            }
            ArgumentKind::Entity(kind) => match value.as_str() {
                None => issues.push(issue(
                    &spec.name,
                    "argument_type_invalid",
                    "The argument must contain a canonical entity identifier.",
                )),
                Some(id) if catalog.entity(kind, id).is_none() => issues.push(issue(
                    &spec.name,
                    "entity_not_allowlisted",
                    "The entity is not in the VoiceOS ontology.",
                )),
                Some(_) => {}
            },
        }
    }

    for entity in &request.entities {
        if matches!(
            entity.kind,
            EntityKind::Device | EntityKind::Provider | EntityKind::Service
        ) && catalog.entity(&entity.kind, &entity.id).is_none()
        {
            issues.push(issue(
                "entities",
                "entity_not_allowlisted",
                "A referenced entity is not in the VoiceOS ontology.",
            ));
        }
    }
    issues
}

fn issue(field: &str, code: &str, message: &str) -> ValidationIssue {
    ValidationIssue {
        field: field.to_owned(),
        code: code.to_owned(),
        message: message.to_owned(),
    }
}
