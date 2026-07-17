//! Password hashing for the settings gate (Argon2id, never plaintext).

use crate::error::{Error, Result};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use password_hash::{rand_core::OsRng, PasswordHash, SaltString};

/// Hash a password with Argon2id and return a PHC string (safe to store).
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default(); // Argon2id, sensible defaults
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| Error::Password(e.to_string()))?;
    Ok(hash.to_string())
}

/// Verify a password against a stored PHC hash. Returns `Ok(false)` on a
/// wrong password, `Err` only if the stored hash is malformed.
pub fn verify_password(password: &str, phc: &str) -> Result<bool> {
    let parsed = PasswordHash::new(phc).map_err(|e| Error::Password(e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify() {
        let phc = hash_password("correct horse battery staple").unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery staple", &phc).unwrap());
        assert!(!verify_password("wrong", &phc).unwrap());
    }

    #[test]
    fn malformed_hash_errors() {
        assert!(verify_password("x", "not-a-phc-string").is_err());
    }
}
