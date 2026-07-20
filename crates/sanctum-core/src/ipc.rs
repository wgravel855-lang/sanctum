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
    /// Toggle bypass-tool blocking (proxies/VPN/Tor/DoH). Enabling is always
    /// allowed (strengthens); disabling is password-gated and frozen while
    /// locked.
    SetBypassBlocking { enabled: bool, password: String },
    /// Toggle Strict mode (blocks mainstream suggestive-content gateways).
    /// Enabling is always allowed; disabling is password-gated and frozen while
    /// locked.
    SetStrictMode { enabled: bool, password: String },
    /// Toggle keyword blocking (domain-NAME matching, never page content).
    /// Enabling is always allowed; disabling is password-gated and frozen while
    /// locked, exactly like Strict mode.
    SetKeywordBlocking { enabled: bool, password: String },
    /// The user's own keyword rules (the built-in set is not returned).
    ListKeywords,
    /// Add a keyword. Always allowed (grow-only, strengthens protection).
    AddKeyword { word: String },
    /// Remove a user keyword. Refused while locked; else password-gated.
    RemoveKeyword { word: String, password: String },
    /// Set the opt-in uninstall cooldown, in hours (0 = off). GROW-ONLY: once
    /// set above zero it can only be increased, never reduced or disabled. Only
    /// ever strengthens, so no password is required.
    SetUninstallCooldown { hours: u32 },
    /// Set/change/remove the accountability webhook (empty = remove). Adding one
    /// (from none) is always allowed; changing or removing an existing one is
    /// password-gated and frozen while locked, and alerts the current partner.
    SetAccountability { webhook: String, password: String },
    /// Set/change/remove Twilio SMS accountability (all-empty = remove). Same
    /// gating as the webhook: adding is allowed; changing or removing an existing
    /// one is password-gated, frozen while locked, and alerts the partner.
    SetAccountabilitySms {
        sid: String,
        token: String,
        from: String,
        to: String,
        password: String,
    },
    /// Send a test notification to every configured accountability channel.
    TestAccountability,
    /// Toggle the weekly "still protected" heartbeat to the partner. Enabling is
    /// always allowed (more oversight); disabling reduces oversight, so it is
    /// password-gated, frozen while locked, and alerts the partner.
    SetHeartbeat { enabled: bool, password: String },
    /// Turn partner-approved unblocking on/off. Enabling adds a gate (allowed,
    /// but needs a partner channel configured); disabling is a weakening op
    /// (password-gated, frozen while locked, alerts the partner).
    SetPartnerApproval { enabled: bool, password: String },
    /// Ask to unblock `domain` (only when partner approval is required). Mints a
    /// one-time code, sends it to the partner, and stores the pending request.
    RequestUnblock { domain: String },
    /// Submit the partner's one-time code to complete the pending unblock.
    ApproveUnblock { code: String },
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
    /// Fetch the user's "letter to self" (shown during the block-moment pause).
    GetLetter,
    /// Save the letter to self. Always allowed: it can only strengthen resolve,
    /// never weakens protection, so it is not frozen during a locked session.
    SetLetter { text: String },
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
    /// The user's own keyword rules.
    Keywords(Vec<String>),
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
    /// The user's letter to self (`None` if never written).
    Letter(Option<String>),
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
    /// Lifetime count of urges resisted (intervention windows closed without a
    /// bypass). Preserved across history wipes — the honest, positive number.
    #[serde(default)]
    pub urges_resisted: u64,
    pub protected_days: u64,
    pub streak: u64,
    pub locked: bool,
    pub locked_until: Option<DateTime<Utc>>,
    pub schedule: Schedule,
    /// Size of the effective block set (built-in baseline ∪ the user's list).
    /// Not shown as "your block list" — the baseline isn't user-managed.
    pub blocklist_count: usize,
    /// How many sites the user has personally added. This is what the Block
    /// List screen manages and counts.
    #[serde(default)]
    pub custom_block_count: usize,
    /// Whether bypass-tool blocking (proxies/VPN/Tor/DoH) is on.
    #[serde(default)]
    pub block_bypass: bool,
    /// Whether Strict mode (suggestive-content gateways) is on.
    #[serde(default)]
    pub block_strict: bool,
    /// Whether keyword blocking (domain-name matching) is on.
    #[serde(default)]
    pub block_keywords: bool,
    /// How many keyword rules the user has added of their own.
    #[serde(default)]
    pub custom_keyword_count: usize,
    /// The opt-in uninstall cooldown in hours (0 = off). Grow-only once set.
    #[serde(default)]
    pub uninstall_cooldown_hours: u32,
    /// Whether an accountability webhook is configured.
    #[serde(default)]
    pub accountability_on: bool,
    /// Whether Twilio SMS accountability is configured.
    #[serde(default)]
    pub accountability_sms_on: bool,
    /// If the accountability webhook is an ntfy.sh topic, the topic name (so the
    /// UI can re-show the partner's QR). `None` for a custom webhook or when off.
    #[serde(default)]
    pub accountability_ntfy_topic: Option<String>,
    /// Whether the weekly "still protected" heartbeat/digest is enabled. Only
    /// sends when a partner channel is configured.
    #[serde(default)]
    pub heartbeat_on: bool,
    /// Whether unblocking a site requires the partner's one-time code.
    #[serde(default)]
    pub require_partner_approval: bool,
    /// The domain of a pending unblock request awaiting the partner's code
    /// (`None` if there is no request in flight).
    #[serde(default)]
    pub pending_unblock: Option<String>,
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
    /// The user's letter to self, attached when `pending` so the block-moment
    /// window can show it without a second round-trip. `None` if unwritten.
    #[serde(default)]
    pub letter: Option<String>,
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
