use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

const ADMIN_SESSION_TTL_HOURS: i64 = 12;

type SessionMac = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdminSessionClaims {
    pub session_id: String,
    pub issued_at: DateTime<Utc>,
    pub password_version: u64,
}

impl AdminSessionClaims {
    pub fn new(
        session_id: impl Into<String>,
        issued_at: DateTime<Utc>,
        password_version: u64,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            issued_at,
            password_version,
        }
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.issued_at + admin_session_ttl()
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("admin session secret must not be empty")]
    EmptySecret,
    #[error("session cookie is malformed")]
    MalformedCookie,
    #[error("session cookie signature is invalid")]
    InvalidSignature,
    #[error("session has expired")]
    Expired,
    #[error("session password version does not match the current password")]
    PasswordVersionMismatch,
    #[error("session payload could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn sign_session_cookie(
    secret: &str,
    claims: &AdminSessionClaims,
) -> Result<String, SessionError> {
    let payload = serde_json::to_vec(claims)?;
    let signature = sign_payload(secret, &payload)?;
    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

pub fn verify_session_cookie(
    secret: &str,
    cookie_value: &str,
    now: DateTime<Utc>,
    current_password_version: u64,
) -> Result<AdminSessionClaims, SessionError> {
    let (encoded_payload, encoded_signature) = cookie_value
        .split_once('.')
        .ok_or(SessionError::MalformedCookie)?;
    let payload = URL_SAFE_NO_PAD
        .decode(encoded_payload)
        .map_err(|_| SessionError::MalformedCookie)?;
    let signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|_| SessionError::MalformedCookie)?;

    let mut mac = session_mac(secret)?;
    mac.update(&payload);
    mac.verify_slice(&signature)
        .map_err(|_| SessionError::InvalidSignature)?;

    let claims = serde_json::from_slice::<AdminSessionClaims>(&payload)?;
    if claims.expires_at() <= now {
        return Err(SessionError::Expired);
    }
    if claims.password_version != current_password_version {
        return Err(SessionError::PasswordVersionMismatch);
    }

    Ok(claims)
}

pub fn admin_session_ttl() -> Duration {
    Duration::hours(ADMIN_SESSION_TTL_HOURS)
}

fn sign_payload(secret: &str, payload: &[u8]) -> Result<Vec<u8>, SessionError> {
    let mut mac = session_mac(secret)?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn session_mac(secret: &str) -> Result<SessionMac, SessionError> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err(SessionError::EmptySecret);
    }

    SessionMac::new_from_slice(trimmed.as_bytes()).map_err(|_| SessionError::EmptySecret)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::{
        sign_session_cookie, verify_session_cookie, AdminSessionClaims, SessionError,
    };

    #[test]
    fn signed_session_cookie_is_accepted() {
        let now = Utc::now();
        let claims = AdminSessionClaims::new("session-1", now, 3);

        let cookie = sign_session_cookie("secret", &claims).expect("sign session");
        let verified = verify_session_cookie("secret", &cookie, now, 3).expect("verify session");

        assert_eq!(verified.session_id, "session-1");
        assert_eq!(verified.password_version, 3);
    }

    #[test]
    fn expired_session_is_rejected() {
        let now = Utc::now();
        let claims = AdminSessionClaims::new("session-1", now - Duration::hours(13), 0);

        let cookie = sign_session_cookie("secret", &claims).expect("sign session");
        let error =
            verify_session_cookie("secret", &cookie, now, 0).expect_err("expired session");

        assert!(matches!(error, SessionError::Expired));
    }

    #[test]
    fn password_change_invalidates_prior_sessions() {
        let now = Utc::now();
        let claims = AdminSessionClaims::new("session-1", now, 1);

        let cookie = sign_session_cookie("secret", &claims).expect("sign session");
        let error =
            verify_session_cookie("secret", &cookie, now, 2).expect_err("stale session");

        assert!(matches!(error, SessionError::PasswordVersionMismatch));
    }
}
