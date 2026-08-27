use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine as _;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI: &str =
    "http://127.0.0.1:53682/v1/integrations/google-calendar/callback";
pub const GOOGLE_CALENDAR_AUTHORIZATION_SESSION_TTL: Duration = Duration::minutes(5);

/// Private Google OAuth configuration. It deliberately implements neither Debug nor serde traits.
pub struct GoogleCalendarOAuthConfiguration {
    client_id: Option<String>,
    redirect_uri: Option<String>,
}

impl GoogleCalendarOAuthConfiguration {
    pub fn new(client_id: Option<String>, redirect_uri: Option<String>) -> Self {
        Self {
            client_id,
            redirect_uri,
        }
    }

    pub fn validate(
        self,
    ) -> Result<ValidatedGoogleCalendarOAuthConfiguration, GoogleCalendarOAuthConfigurationError>
    {
        let client_id = self
            .client_id
            .filter(|value| !value.trim().is_empty())
            .ok_or(GoogleCalendarOAuthConfigurationError::MissingClientId)?;
        let redirect_uri = self
            .redirect_uri
            .filter(|value| !value.trim().is_empty())
            .ok_or(GoogleCalendarOAuthConfigurationError::MissingRedirectUri)?;
        if redirect_uri != GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI {
            return Err(GoogleCalendarOAuthConfigurationError::RedirectUriNotAllowed);
        }
        Ok(ValidatedGoogleCalendarOAuthConfiguration {
            client_id,
            redirect_uri,
        })
    }
}

/// Validated private OAuth configuration. It deliberately implements neither Debug nor serde traits.
pub struct ValidatedGoogleCalendarOAuthConfiguration {
    client_id: String,
    redirect_uri: String,
}

impl ValidatedGoogleCalendarOAuthConfiguration {
    #[allow(dead_code)]
    pub(crate) fn client_id(&self) -> &str {
        &self.client_id
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum GoogleCalendarOAuthConfigurationError {
    #[error("Google Calendar OAuth client ID is not configured")]
    MissingClientId,
    #[error("Google Calendar OAuth redirect URI is not configured")]
    MissingRedirectUri,
    #[error("Google Calendar OAuth redirect URI is not allowlisted")]
    RedirectUriNotAllowed,
}

impl GoogleCalendarOAuthConfigurationError {
    pub fn code(self) -> &'static str {
        match self {
            Self::MissingClientId => "google_calendar_oauth_client_id_missing",
            Self::MissingRedirectUri => "google_calendar_oauth_redirect_uri_missing",
            Self::RedirectUriNotAllowed => "google_calendar_oauth_redirect_uri_not_allowed",
        }
    }
}

/// Process-local, non-persistent authorization-session registry.
pub struct GoogleCalendarAuthorizationSessions {
    configuration: ValidatedGoogleCalendarOAuthConfiguration,
    sessions: Mutex<HashMap<String, PendingGoogleCalendarAuthorizationSession>>,
}

impl GoogleCalendarAuthorizationSessions {
    pub fn new(configuration: ValidatedGoogleCalendarOAuthConfiguration) -> Self {
        Self {
            configuration,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn begin(
        &self,
        owner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<GoogleCalendarAuthorizationSession, GoogleCalendarAuthorizationSessionError> {
        if owner_id.is_empty() {
            return Err(GoogleCalendarAuthorizationSessionError::InvalidOwner);
        }
        let state = random_value();
        let verifier = format!("{}{}", random_value(), random_value());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        let expires_at = now + GOOGLE_CALENDAR_AUTHORIZATION_SESSION_TTL;
        let pending = PendingGoogleCalendarAuthorizationSession {
            owner_id: owner_id.to_owned(),
            redirect_uri: self.configuration.redirect_uri.clone(),
            verifier,
            expires_at,
        };
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| GoogleCalendarAuthorizationSessionError::Unavailable)?;
        sessions.retain(|_, session| session.expires_at > now);
        sessions.insert(state.clone(), pending);
        Ok(GoogleCalendarAuthorizationSession {
            state,
            code_challenge: challenge,
            expires_at,
        })
    }

    pub fn consume(
        &self,
        owner_id: &str,
        state: &str,
        callback_redirect_uri: &str,
        now: DateTime<Utc>,
    ) -> Result<ConsumedGoogleCalendarAuthorizationSession, GoogleCalendarAuthorizationSessionError>
    {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| GoogleCalendarAuthorizationSessionError::Unavailable)?;
        let Some(pending) = sessions.get(state) else {
            return Err(GoogleCalendarAuthorizationSessionError::InvalidOrExpiredState);
        };
        if pending.expires_at <= now {
            sessions.remove(state);
            return Err(GoogleCalendarAuthorizationSessionError::InvalidOrExpiredState);
        }
        if pending.owner_id != owner_id || pending.redirect_uri != callback_redirect_uri {
            return Err(GoogleCalendarAuthorizationSessionError::InvalidOrExpiredState);
        }
        let pending = sessions
            .remove(state)
            .expect("pending session was present while mutex was held");
        Ok(ConsumedGoogleCalendarAuthorizationSession {
            verifier: pending.verifier,
        })
    }
}

fn random_value() -> String {
    Uuid::new_v4().simple().to_string()
}

struct PendingGoogleCalendarAuthorizationSession {
    owner_id: String,
    redirect_uri: String,
    verifier: String,
    expires_at: DateTime<Utc>,
}

/// Safe-to-return authorization start details; it contains no verifier, code, token, or client secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoogleCalendarAuthorizationSession {
    state: String,
    code_challenge: String,
    expires_at: DateTime<Utc>,
}

impl GoogleCalendarAuthorizationSession {
    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn code_challenge(&self) -> &str {
        &self.code_challenge
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

/// One-time consumed private session material. It deliberately implements neither Debug nor serde traits.
pub struct ConsumedGoogleCalendarAuthorizationSession {
    verifier: String,
}

impl ConsumedGoogleCalendarAuthorizationSession {
    pub fn with_pkce_verifier<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        operation(&self.verifier)
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum GoogleCalendarAuthorizationSessionError {
    #[error("Google Calendar OAuth owner is invalid")]
    InvalidOwner,
    #[error("Google Calendar OAuth state is invalid or expired")]
    InvalidOrExpiredState,
    #[error("Google Calendar OAuth session storage is unavailable")]
    Unavailable,
}

impl GoogleCalendarAuthorizationSessionError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidOwner => "google_calendar_oauth_owner_invalid",
            Self::InvalidOrExpiredState => "google_calendar_oauth_state_invalid_or_expired",
            Self::Unavailable => "google_calendar_oauth_session_unavailable",
        }
    }
}
