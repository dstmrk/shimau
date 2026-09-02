//! Password hashing. Argon2id with the crate's default parameters (spec §7.1).

use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::Argon2;

#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    #[error("could not hash the password: {0}")]
    Hash(String),
    #[error("stored password hash is unreadable: {0}")]
    Parse(String),
}

/// Shortest bootstrap password accepted. Short enough not to be annoying,
/// long enough that the rate limiter is not the only thing standing between
/// an attacker and the account.
pub const MIN_PASSWORD_LEN: usize = 12;

/// Hashes a plaintext password into a PHC string (`$argon2id$v=19$...`).
pub fn hash(password: &str) -> Result<String, PasswordError> {
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| PasswordError::Hash(e.to_string()))
}

/// Verifies a password against a stored PHC string.
///
/// A malformed stored hash is an error, never a silent `false`: it would
/// otherwise look identical to a wrong password and hide a corrupt database.
pub fn verify(password: &str, stored: &str) -> Result<bool, PasswordError> {
    let parsed = PasswordHash::new(stored).map_err(|e| PasswordError::Parse(e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_verifies_against_its_own_password() {
        let stored = hash("correct horse battery staple").unwrap();
        assert!(verify("correct horse battery staple", &stored).unwrap());
    }

    #[test]
    fn a_wrong_password_does_not_verify() {
        let stored = hash("correct horse battery staple").unwrap();
        assert!(!verify("Correct horse battery staple", &stored).unwrap());
        assert!(!verify("", &stored).unwrap());
    }

    #[test]
    fn hashes_are_argon2id_and_salted() {
        let first = hash("same password").unwrap();
        let second = hash("same password").unwrap();
        assert!(first.starts_with("$argon2id$"), "got {first}");
        assert_ne!(first, second, "each hash must carry its own salt");
    }

    #[test]
    fn the_plaintext_never_appears_in_the_hash() {
        let stored = hash("hunter2-hunter2-hunter2").unwrap();
        assert!(!stored.contains("hunter2"));
    }

    #[test]
    fn a_corrupt_stored_hash_is_an_error_not_a_false() {
        let err = verify("whatever", "not-a-phc-string").unwrap_err();
        assert!(matches!(err, PasswordError::Parse(_)));
    }
}
