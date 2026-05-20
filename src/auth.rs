use crate::config::ServerConfig;
use argon2::{
    password_hash::{self, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use subtle::ConstantTimeEq;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("admin password must not be empty")]
    EmptyPassword,
    #[error("password hash error: {0}")]
    PasswordHash(String),
}

pub fn bearer_token_matches(header_value: Option<&str>, expected_token: &str) -> bool {
    header_value
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| constant_time_token_eq(token.as_bytes(), expected_token.as_bytes()))
}

pub fn hash_admin_password(password: &str) -> Result<String, AuthError> {
    if password.is_empty() {
        return Err(AuthError::EmptyPassword);
    }

    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(map_password_hash_error)?
        .to_string())
}

pub fn verify_admin_password(hash: &str, password: &str) -> Result<bool, AuthError> {
    if password.is_empty() {
        return Ok(false);
    }

    let parsed_hash = PasswordHash::new(hash).map_err(map_password_hash_error)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(error) => Err(map_password_hash_error(error)),
    }
}

pub fn bootstrap_required(config: &ServerConfig) -> bool {
    config.admin_password_hash.is_none()
}

fn map_password_hash_error(error: password_hash::Error) -> AuthError {
    AuthError::PasswordHash(error.to_string())
}

fn constant_time_token_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.ct_eq(right).into()
}

#[cfg(test)]
mod tests {
    use super::{
        bearer_token_matches, bootstrap_required, hash_admin_password, verify_admin_password,
    };
    use crate::config::{load_or_create_config, ConfigOverrides};

    #[test]
    fn accepts_only_exact_bearer_token() {
        assert!(bearer_token_matches(Some("Bearer secret"), "secret"));
        assert!(!bearer_token_matches(Some("Bearer other"), "secret"));
        assert!(!bearer_token_matches(Some("secret"), "secret"));
        assert!(!bearer_token_matches(None, "secret"));
    }

    #[test]
    fn hashes_and_verifies_admin_passwords() {
        let password = "correct horse battery staple";

        let hash = hash_admin_password(password).expect("hash password");

        assert_ne!(hash, password);
        assert!(
            verify_admin_password(&hash, password).expect("verify matching password")
        );
        assert!(
            !verify_admin_password(&hash, "wrong password").expect("verify wrong password")
        );
    }

    #[test]
    fn admin_password_verification_keeps_whitespace_significant() {
        let hash = hash_admin_password(" secret ").expect("hash password");

        assert!(verify_admin_password(&hash, " secret ").expect("verify matching password"));
        assert!(!verify_admin_password(&hash, "secret").expect("verify trimmed password"));
    }

    #[test]
    fn bootstrap_is_required_when_admin_password_hash_is_missing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load config");

        assert!(bootstrap_required(&config));
    }
}
