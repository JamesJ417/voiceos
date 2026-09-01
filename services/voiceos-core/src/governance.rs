use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub tenant_id: String,
    pub user_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityError(&'static str);
impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for IdentityError {}

impl Identity {
    pub fn new(
        tenant_id: impl Into<String>,
        user_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> Result<Self, IdentityError> {
        let identity = Self {
            tenant_id: tenant_id.into(),
            user_id: user_id.into(),
            device_id: device_id.into(),
        };
        for value in [&identity.tenant_id, &identity.user_id, &identity.device_id] {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
            {
                return Err(IdentityError(
                    "identity components must be non-empty and normalized",
                ));
            }
        }
        Ok(identity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub capability: String,
    pub version: u16,
}
impl CapabilityGrant {
    pub fn new(capability: impl Into<String>, version: u16) -> Self {
        Self {
            capability: capability.into(),
            version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationContext {
    pub subject: Identity,
    pub grants: Vec<CapabilityGrant>,
}
impl AuthorizationContext {
    pub fn new(subject: Identity, grants: Vec<CapabilityGrant>) -> Self {
        Self { subject, grants }
    }
    pub fn allows(&self, capability: &str) -> bool {
        self.grants
            .iter()
            .any(|g| g.capability == capability && g.version == 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub product: String,
    pub version: String,
    pub artifact_url: String,
    pub signature: String,
    pub key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub occurred_at: DateTime<Utc>,
    pub actor: Identity,
    pub action: String,
    detail: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicAuditEvent {
    pub occurred_at: DateTime<Utc>,
    pub tenant_id: String,
    pub action: String,
    pub detail: Option<String>,
}
impl AuditEvent {
    pub fn new(actor: Identity, action: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            occurred_at: Utc::now(),
            actor,
            action: action.into(),
            detail: detail.into(),
        }
    }
    pub fn public_projection(&self) -> PublicAuditEvent {
        PublicAuditEvent {
            occurred_at: self.occurred_at,
            tenant_id: self.actor.tenant_id.clone(),
            action: self.action.clone(),
            detail: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_validate_and_round_trip() {
        let identity = Identity::new("tenant-a", "user-1", "device-9").unwrap();
        assert_eq!(
            serde_json::to_string(&identity).unwrap(),
            r#"{"tenant_id":"tenant-a","user_id":"user-1","device_id":"device-9"}"#
        );
        assert_eq!(
            serde_json::from_str::<Identity>(&serde_json::to_string(&identity).unwrap()).unwrap(),
            identity
        );
        assert!(Identity::new("tenant/a", "user", "device").is_err());
    }
    #[test]
    fn unknown_capability_versions_are_denied() {
        let context = AuthorizationContext::new(
            Identity::new("t", "u", "d").unwrap(),
            vec![CapabilityGrant::new("sleep.read", 99)],
        );
        assert!(!context.allows("sleep.read"));
    }
    #[test]
    fn audit_projection_redacts_details() {
        let event = AuditEvent::new(
            Identity::new("t", "u", "d").unwrap(),
            "secret.internal",
            "token=abc",
        );
        assert!(event.public_projection().detail.is_none());
    }
}
