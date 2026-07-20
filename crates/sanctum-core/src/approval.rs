//! Partner-approved unblock, done without a server.
//!
//! Sanctum has no inbound channel, so a literal "partner taps Approve" is
//! impossible without standing up infrastructure (which would break the
//! zero-server, zero-account promise). Instead, approval is a RELAY: when the
//! user asks to unblock a site, Sanctum sends the partner a one-time code tied
//! to that exact request. The partner approves by reading the code back (a call,
//! a text); the user enters it and the site unblocks.
//!
//! Every request mints a fresh, high-entropy code, so nothing can be memorized
//! and reused, and the partner always sees precisely which site is being asked
//! for. The code is stored only as an Argon2 hash, tries are capped, and the
//! request expires — so the ACL-locked store never holds a reusable secret.

use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use password_hash::rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

/// Length of a one-time approval code.
pub const CODE_LEN: usize = 6;
/// Wrong-code attempts before the pending request is discarded (forcing a new
/// request, which re-notifies the partner). Caps online guessing.
pub const MAX_ATTEMPTS: u32 = 5;
/// How long a pending request stays valid.
pub const REQUEST_TTL_MINS: i64 = 30;

/// Alphabet for codes: unambiguous when read aloud (no 0/O/1/I/L) and no vowels
/// (so codes never spell words). 27 symbols -> 27^6 ~= 387 million.
const ALPHABET: &[u8] = b"BCDFGHJKMNPQRSTVWXZ23456789";

/// What an approved request will do. Removing a user-added block or adding an
/// allowlist exception both *weaken* protection, which is exactly why they route
/// through the partner when approval is required.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnblockAction {
    /// Remove a domain the user previously added to their own block list.
    RemoveBlock,
    /// Add an allowlist exception overriding the built-in list.
    AddAllow,
}

/// A pending unblock request awaiting the partner's code. At most one is active;
/// a new request replaces any previous one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PendingUnblock {
    pub domain: String,
    pub action: UnblockAction,
    /// Argon2 PHC hash of the one-time code (never stored in the clear).
    pub code_hash: String,
    pub created_at: DateTime<Utc>,
    pub attempts: u32,
}

impl PendingUnblock {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now.signed_duration_since(self.created_at) > Duration::minutes(REQUEST_TTL_MINS)
    }
}

/// The result of checking a submitted code against a pending request.
#[derive(Debug, PartialEq, Eq)]
pub enum ApprovalOutcome {
    /// Code matched — the caller should apply the action and clear the request.
    Approved,
    /// Wrong code; `attempts_left` tries remain before the request is discarded.
    Wrong { attempts_left: u32 },
    /// The request is older than the TTL — discard it, ask for a new one.
    Expired,
    /// Too many wrong tries — discard it, ask for a new one.
    TooManyAttempts,
}

/// Generate a fresh, human-readable one-time code from the OS CSPRNG.
pub fn generate_code() -> String {
    let mut buf = [0u8; CODE_LEN];
    OsRng.fill_bytes(&mut buf);
    buf.iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

/// Check a submitted code against a pending request. Pure: the caller persists
/// the incremented attempt count, applies the action, or clears the request
/// based on the outcome.
pub fn check_code(
    pending: &PendingUnblock,
    submitted: &str,
    now: DateTime<Utc>,
) -> Result<ApprovalOutcome> {
    if pending.is_expired(now) {
        return Ok(ApprovalOutcome::Expired);
    }
    if pending.attempts >= MAX_ATTEMPTS {
        return Ok(ApprovalOutcome::TooManyAttempts);
    }
    if crate::password::verify_password(submitted.trim(), &pending.code_hash)? {
        Ok(ApprovalOutcome::Approved)
    } else {
        let attempts_left = MAX_ATTEMPTS.saturating_sub(pending.attempts + 1);
        Ok(ApprovalOutcome::Wrong { attempts_left })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::password::hash_password;

    fn pending(code: &str, created: DateTime<Utc>, attempts: u32) -> PendingUnblock {
        PendingUnblock {
            domain: "example.com".into(),
            action: UnblockAction::AddAllow,
            code_hash: hash_password(code).unwrap(),
            created_at: created,
            attempts,
        }
    }

    #[test]
    fn code_is_readable_and_random() {
        let a = generate_code();
        let b = generate_code();
        assert_eq!(a.len(), CODE_LEN);
        assert!(a.bytes().all(|c| ALPHABET.contains(&c)));
        assert_ne!(a, b, "two codes should essentially never collide");
    }

    #[test]
    fn correct_code_approves() {
        let now = Utc::now();
        let p = pending("BCD234", now, 0);
        assert_eq!(check_code(&p, "BCD234", now).unwrap(), ApprovalOutcome::Approved);
        // Case/space tolerant only via trim; exact match required otherwise.
        assert_eq!(
            check_code(&p, " BCD234 ", now).unwrap(),
            ApprovalOutcome::Approved
        );
    }

    #[test]
    fn wrong_code_counts_down() {
        let now = Utc::now();
        let p = pending("BCD234", now, 0);
        assert_eq!(
            check_code(&p, "WRONG1", now).unwrap(),
            ApprovalOutcome::Wrong { attempts_left: 4 }
        );
        let p4 = pending("BCD234", now, 4);
        assert_eq!(
            check_code(&p4, "WRONG1", now).unwrap(),
            ApprovalOutcome::Wrong { attempts_left: 0 }
        );
    }

    #[test]
    fn expired_and_exhausted_are_rejected() {
        let now = Utc::now();
        let old = now - Duration::minutes(REQUEST_TTL_MINS + 1);
        assert_eq!(
            check_code(&pending("BCD234", old, 0), "BCD234", now).unwrap(),
            ApprovalOutcome::Expired
        );
        assert_eq!(
            check_code(&pending("BCD234", now, MAX_ATTEMPTS), "BCD234", now).unwrap(),
            ApprovalOutcome::TooManyAttempts
        );
    }
}
