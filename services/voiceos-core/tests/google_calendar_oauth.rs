use base64::Engine as _;
use sha2::{Digest, Sha256};
use voiceos_core::{
    GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI, GoogleCalendarAuthorizationSessions,
    GoogleCalendarOAuthConfiguration, GoogleCalendarOAuthConfigurationError,
};

#[test]
fn oauth_readiness_reports_missing_client_id_without_exposing_configuration_values() {
    let error = GoogleCalendarOAuthConfiguration::new(
        None,
        Some("http://127.0.0.1:53682/v1/integrations/google-calendar/callback".into()),
    )
    .validate()
    .err()
    .unwrap();

    assert_eq!(
        error,
        GoogleCalendarOAuthConfigurationError::MissingClientId
    );
    assert_eq!(error.code(), "google_calendar_oauth_client_id_missing");
    assert!(!error.to_string().contains("127.0.0.1"));
}

#[test]
fn oauth_readiness_reports_missing_redirect_and_rejects_non_allowlisted_redirects() {
    let missing_redirect =
        GoogleCalendarOAuthConfiguration::new(Some("private-client-id".into()), None)
            .validate()
            .err()
            .unwrap();
    let invalid_redirect = GoogleCalendarOAuthConfiguration::new(
        Some("private-client-id".into()),
        Some("http://localhost:53682/v1/integrations/google-calendar/callback".into()),
    )
    .validate()
    .err()
    .unwrap();

    assert_eq!(
        missing_redirect,
        GoogleCalendarOAuthConfigurationError::MissingRedirectUri
    );
    assert_eq!(
        missing_redirect.code(),
        "google_calendar_oauth_redirect_uri_missing"
    );
    assert_eq!(
        invalid_redirect,
        GoogleCalendarOAuthConfigurationError::RedirectUriNotAllowed
    );
    assert_eq!(
        invalid_redirect.code(),
        "google_calendar_oauth_redirect_uri_not_allowed"
    );
}

#[test]
fn authorization_session_binds_random_state_to_owner_and_uses_pkce_s256() {
    let configuration = GoogleCalendarOAuthConfiguration::new(
        Some("private-client-id".into()),
        Some(GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI.into()),
    )
    .validate()
    .unwrap();
    let sessions = GoogleCalendarAuthorizationSessions::new(configuration);
    let now = chrono::Utc::now();

    let first = sessions.begin("owner-a", now).unwrap();
    let second = sessions.begin("owner-a", now).unwrap();

    assert_ne!(first.state(), second.state());
    assert_eq!(first.expires_at(), now + chrono::Duration::minutes(5));
    let consumed = sessions
        .consume(
            "owner-a",
            first.state(),
            GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI,
            now,
        )
        .unwrap();
    consumed.with_pkce_verifier(|verifier| {
        let challenge =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier));
        assert_eq!(challenge, first.code_challenge());
    });
}

#[test]
fn callback_consumption_is_owner_bound_exact_redirect_and_replay_safe() {
    let configuration = GoogleCalendarOAuthConfiguration::new(
        Some("private-client-id".into()),
        Some(GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI.into()),
    )
    .validate()
    .unwrap();
    let sessions = GoogleCalendarAuthorizationSessions::new(configuration);
    let now = chrono::Utc::now();
    let session = sessions.begin("owner-a", now).unwrap();

    for (owner, redirect) in [
        ("owner-b", GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI),
        ("owner-a", "http://127.0.0.1:53682/unexpected"),
    ] {
        let error = sessions
            .consume(owner, session.state(), redirect, now)
            .err()
            .unwrap();
        assert_eq!(
            error.code(),
            "google_calendar_oauth_state_invalid_or_expired"
        );
    }
    sessions
        .consume(
            "owner-a",
            session.state(),
            GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI,
            now,
        )
        .unwrap();
    let replay = sessions
        .consume(
            "owner-a",
            session.state(),
            GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI,
            now,
        )
        .err()
        .unwrap();
    assert_eq!(
        replay.code(),
        "google_calendar_oauth_state_invalid_or_expired"
    );
}

#[test]
fn expired_callback_session_is_removed_and_fails_closed() {
    let configuration = GoogleCalendarOAuthConfiguration::new(
        Some("private-client-id".into()),
        Some(GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI.into()),
    )
    .validate()
    .unwrap();
    let sessions = GoogleCalendarAuthorizationSessions::new(configuration);
    let now = chrono::Utc::now();
    let session = sessions.begin("owner-a", now).unwrap();
    let error = sessions
        .consume(
            "owner-a",
            session.state(),
            GOOGLE_CALENDAR_LOOPBACK_REDIRECT_URI,
            session.expires_at(),
        )
        .err()
        .unwrap();

    assert_eq!(
        error.code(),
        "google_calendar_oauth_state_invalid_or_expired"
    );
}
