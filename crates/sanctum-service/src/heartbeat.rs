//! The "still protected" heartbeat (accountability digest).
//!
//! Real-time alerts tell a partner the moment protection is *weakened*. But the
//! most honest signal is a positive one whose ABSENCE is the tell: a short
//! periodic "still protected" note. If Sanctum is uninstalled, disabled, or the
//! machine simply stops running it, the weekly note stops arriving, and the
//! partner notices silence rather than having to trust that no news is good
//! news. The message carries only counts and protection state, never anything
//! about what was browsed.
//!
//! This module is pure and unit-tested; the service's reconcile loop owns the
//! timer and the actual sending (see `service.rs`).

use chrono::{DateTime, Duration, Utc};

/// How long between heartbeats (weekly).
pub const INTERVAL_SECS: i64 = 7 * 24 * 60 * 60;

/// The heartbeat interval as a `Duration`.
pub fn interval() -> Duration {
    Duration::seconds(INTERVAL_SECS)
}

/// Whether a heartbeat is due. The first one (never sent) is always due, so the
/// partner gets an immediate note that confirms the channel and teaches them
/// what the ongoing silence-or-signal means.
pub fn due(last_sent: Option<DateTime<Utc>>, now: DateTime<Utc>, interval: Duration) -> bool {
    match last_sent {
        None => true,
        Some(t) => now.signed_duration_since(t) >= interval,
    }
}

/// Build the digest text. Honest, no shame, no browsing data. `protection_on` is
/// the master switch; `urges_past_week` and `streak_days` come from the local
/// activity log.
pub fn message(protection_on: bool, urges_past_week: u64, streak_days: u64) -> String {
    let mut s = String::new();

    if protection_on {
        s.push_str("Weekly check-in: still protected.");
    } else {
        s.push_str("Weekly check-in: heads up, protection is currently turned off.");
    }

    s.push(' ');
    match urges_past_week {
        0 => s.push_str("No urges needed resisting this week."),
        1 => s.push_str("1 urge resisted this week."),
        n => s.push_str(&format!("{n} urges resisted this week.")),
    }

    if streak_days >= 2 {
        s.push_str(&format!(" {streak_days}-day protected streak."));
    }

    s.push_str(" If these check-ins stop arriving, protection may have been removed.");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_is_always_due_then_weekly() {
        let now = Utc::now();
        assert!(due(None, now, interval()));
        // 6 days: not yet.
        assert!(!due(Some(now - Duration::days(6)), now, interval()));
        // Exactly 7 days: due.
        assert!(due(Some(now - Duration::days(7)), now, interval()));
        // 8 days: due.
        assert!(due(Some(now - Duration::days(8)), now, interval()));
    }

    #[test]
    fn message_reflects_state_and_pluralizes() {
        let on = message(true, 3, 12);
        assert!(on.contains("still protected"));
        assert!(on.contains("3 urges resisted"));
        assert!(on.contains("12-day protected streak"));
        assert!(on.contains("If these check-ins stop"));
        assert!(!on.contains('—')); // no em dashes anywhere

        assert!(message(true, 0, 0).contains("No urges needed resisting"));
        assert!(message(true, 1, 0).contains("1 urge resisted"));
        // A short streak (<2) is omitted rather than reading "1-day streak".
        assert!(!message(true, 1, 1).contains("streak"));

        // Off flips to an honest heads-up.
        assert!(message(false, 0, 5).contains("protection is currently turned off"));
    }
}
