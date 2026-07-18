//! The IPC protocol between the unprivileged UI and the LocalSystem service
//! (ADR-001 §2). Types only — the named-pipe transport lives in the service
//! (server) and the UI (client). Wire format is length-prefixed JSON.
//!
//! The service is the sole authority: every mutating command is validated
//! server-side against the lock invariants. The client is never trusted.

use crate::config::Schedule;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Commands the UI may send. Weakening operations carry a password and are
/// refused server-side while a lock is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Current protection status for the home screen.
    GetStatus,
    /// Recent activity-log entries (most recent first).
    RecentEvents { limit: u32 },
    /// The user-added (custom) blocked domains, for the remove UI. The embedded
    /// starter list is not returned — it is the built-in baseline.
    ListCustomBlocks,
    /// Add a domain to the block list. Always allowed (grow-only).
    AddBlock { domain: String },
    /// Remove a blocked domain. Refused while locked; else password-gated.
    RemoveBlock { domain: String, password: String },
    /// Add an allowlist exception. Refused while locked (can't whitelist out).
    AddAllow { domain: String, password: String },
    /// Remove an allowlist exception. Always allowed (strengthens).
    RemoveAllow { domain: String },
    /// Change the schedule. Refused while locked; else password-gated.
    SetSchedule { schedule: Schedule, password: String },
    /// Start a locked ("Cold Turkey") session for `minutes` (clamped to max).
    StartLock { minutes: i64 },
    /// Extend the current lock by `minutes` (extend-only, clamped).
    ExtendLock { minutes: i64 },
    /// Set or change the settings password.
    SetPassword {
        new: String,
        current: Option<String>,
    },
    /// Check a password (for gating UI without mutating anything).
    VerifyPassword { password: String },
    /// Turn protection off. Refused while locked; else password-gated.
    DisableProtection { password: String },
    /// Turn protection back on.
    EnableProtection,
    /// Wipe the activity log immediately.
    DeleteHistory,
    /// Poll for a pending block-moment intervention (v0.1.5 §A). The UI calls
    /// this on a short interval; the reply also carries any "urges while you
    /// were away" count to summarise quietly.
    PollIntervention,
    /// Open the intervention window on demand ("I need help now" / panic
    /// hotkey). Always arms an intervention, even with no block event.
    TriggerIntervention,
    /// The user closed an intervention window (no bypass exists). Logs an
    /// urge-resisted event for the no-shame progress view.
    ResolveIntervention,
}

/// Responses from the service.
///
/// Adjacently tagged (`tag` + `content`) so newtype variants wrapping a
/// sequence (e.g. `Events(Vec<_>)`) serialize correctly — internal tagging
/// cannot represent those.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "resp", content = "body", rename_all = "snake_case")]
pub enum Response {
    Status(Status),
    Events(Vec<EventDto>),
    /// The user's custom (removable) blocked domains.
    CustomBlocks(Vec<String>),
    /// A history wipe report (rows deleted).
    Deleted { count: usize },
    /// The command succeeded.
    Ok,
    /// The command was refused by policy (wrong password, or locked).
    Denied { reason: String },
    /// The command failed for an operational reason.
    Error { message: String },
    /// The result of a `PollIntervention` (v0.1.5 §A).
    Intervention(InterventionDto),
}

/// Everything the home screen needs, in one round-trip.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Status {
    /// The master switch (what the Protection toggle reflects).
    pub protection_active: bool,
    /// Whether filtering is enforced *right now* — the master switch AND the
    /// schedule agreeing this is a protected moment. Drives the home display.
    pub blocking_now: bool,
    /// True when running in HOSTS-only degraded mode.
    pub degraded: bool,
    /// Lifetime count of blocked lookups ("N harmful sites blocked").
    pub total_blocked: u64,
    pub protected_days: u64,
    pub streak: u64,
    pub locked: bool,
    pub locked_until: Option<DateTime<Utc>>,
    pub schedule: Schedule,
    pub blocklist_count: usize,
    pub has_password: bool,
    /// The "All browsers protected" status line.
    pub all_browsers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventDto {
    pub ts: DateTime<Utc>,
    pub kind: String,
    pub detail: String,
}

/// A block-moment intervention poll result (v0.1.5 §A). Debounced service-side
/// so browser DNS prefetch never triggers the window on its own.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct InterventionDto {
    /// True when the intervention window should open now.
    pub pending: bool,
    /// The domain whose blocked lookups armed it (`None` for a manual/panic
    /// trigger).
    pub domain: Option<String>,
    /// Interventions that fired while the UI wasn't polling — surfaced as a
    /// quiet "urges happened while you were away" summary, then cleared.
    pub urges_while_away: u32,
}

/// Encode any protocol value to bytes (JSON).
pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec())
}

/// Decode a protocol value from bytes.
pub fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrips() {
        let c = Command::AddBlock {
            domain: "example.com".into(),
        };
        let bytes = encode(&c);
        let back: Command = decode(&bytes).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn denied_response_roundtrips() {
        let r = Response::Denied {
            reason: "locked".into(),
        };
        let back: Response = decode(&encode(&r)).unwrap();
        assert_eq!(r, back);
    }
}
