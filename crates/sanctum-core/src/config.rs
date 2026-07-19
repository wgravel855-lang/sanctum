//! Configuration, schedule, and lock-state types + the lock invariants
//! that make "Cold Turkey mode" honest: while locked, protection can only
//! get *stronger*, never weaker, and never permanent.

use crate::error::{Error, Result};
use chrono::{DateTime, Datelike, Duration, Local, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

pub const CONFIG_SCHEMA: u32 = 1;

/// Hard ceiling on any lock's duration. The clamp is applied at EVERY write
/// to the unlock time, so no bug, fat-finger, or tampered clock can ever
/// create an effectively permanent lock. The timer is always a guaranteed,
/// secret-free exit. (ADR-001 §8.)
pub const MAX_LOCK_DURATION_DAYS: i64 = 90;

/// The furthest-future instant a lock may be set to, given `now`.
pub fn max_unlock_at(now: DateTime<Utc>) -> DateTime<Utc> {
    now + Duration::days(MAX_LOCK_DURATION_DAYS)
}

/// A daily protection window, in minutes since local midnight. If
/// `start_min > end_min`, the window wraps past midnight (e.g. 21:00–06:00).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TimeWindow {
    pub start_min: u16,
    pub end_min: u16,
    /// Days the window applies to (empty = every day).
    #[serde(default)]
    pub days: Vec<u8>, // 0 = Mon .. 6 = Sun
}

impl TimeWindow {
    fn applies_on(&self, weekday: Weekday) -> bool {
        if self.days.is_empty() {
            return true;
        }
        self.days.contains(&(weekday.num_days_from_monday() as u8))
    }

    fn contains_minute(&self, m: u16) -> bool {
        if self.start_min == self.end_min {
            false // zero-length window
        } else if self.start_min < self.end_min {
            m >= self.start_min && m < self.end_min
        } else {
            // Overnight wrap.
            m >= self.start_min || m < self.end_min
        }
    }
}

/// When protection is enforced.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Schedule {
    /// Enforced 24/7.
    AlwaysOn,
    /// Never enforced by schedule (protection can still be manually on).
    Off,
    /// Enforced during the given daily windows.
    Windows { windows: Vec<TimeWindow> },
    /// A one-off focus session that ends at a fixed instant.
    Focus { ends_at: DateTime<Utc> },
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule::AlwaysOn
    }
}

impl Schedule {
    /// Whether the schedule calls for protection at local time `now`.
    pub fn is_active_at(&self, now: DateTime<Local>) -> bool {
        match self {
            Schedule::AlwaysOn => true,
            Schedule::Off => false,
            Schedule::Focus { ends_at } => now.with_timezone(&Utc) < *ends_at,
            Schedule::Windows { windows } => {
                let m = (now.hour() * 60 + now.minute()) as u16;
                let wd = now.weekday();
                windows
                    .iter()
                    .any(|w| w.applies_on(wd) && w.contains_minute(m))
            }
        }
    }
}

/// Persisted application configuration.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AppConfig {
    pub schema: u32,
    /// Master switch. Even when true, `schedule` decides active windows.
    pub protection_enabled: bool,
    pub schedule: Schedule,
    pub sink_ipv4: String,
    pub sink_ipv6: String,
    pub enforce_safesearch: bool,
    pub block_doh: bool,
    /// Block bypass tools (DoH resolvers, VPN/proxy services, Tor) at the DNS
    /// layer. On by default; turning it off is a weakening op (password-gated,
    /// frozen while locked), so a locked session also seals the escape hatches.
    #[serde(default = "default_true")]
    pub block_bypass: bool,
    /// Block known DoH provider IPs on :443 via Windows Firewall (safe; on).
    #[serde(default = "default_true")]
    pub block_doh_ips: bool,
    /// Block outbound plaintext DNS (:53/:853) to non-Sanctum resolvers via a
    /// WFP dynamic session. OFF by default in v0.1: it is kernel-level packet
    /// filtering that must be verified on a real machine first (a wrong permit
    /// filter can break the service's own forwarding). See README.
    #[serde(default)]
    pub block_plaintext_dns: bool,
    /// Cooldown before an *unlocked* uninstall is permitted.
    pub uninstall_cooldown_hours: u32,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema: CONFIG_SCHEMA,
            protection_enabled: true,
            schedule: Schedule::AlwaysOn,
            sink_ipv4: crate::SINK_IPV4.to_string(),
            sink_ipv6: crate::SINK_IPV6.to_string(),
            enforce_safesearch: true,
            block_doh: true,
            block_bypass: true,
            block_doh_ips: true,
            block_plaintext_dns: false,
            uninstall_cooldown_hours: 24,
        }
    }
}

/// Lock ("Cold Turkey") state. A real lock always has an expiry — Sanctum
/// never claims to be permanent or unremovable.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LockState {
    pub locked: bool,
    /// When the lock expires. `None` while unlocked. A locked state with no
    /// expiry is treated as *inactive* on purpose, so a bug can never brick
    /// the machine.
    pub locked_until: Option<DateTime<Utc>>,
}

impl LockState {
    pub fn unlocked() -> Self {
        Self::default()
    }

    pub fn locked_until(until: DateTime<Utc>) -> Self {
        Self {
            locked: true,
            locked_until: Some(until),
        }
    }

    /// Whether the lock is currently in force.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.locked && self.locked_until.map_or(false, |u| now < u)
    }

    pub fn remaining(&self, now: DateTime<Utc>) -> Option<Duration> {
        match self.locked_until {
            Some(u) if self.locked && now < u => Some(u - now),
            _ => None,
        }
    }
}

/// Guard a proposed config change against an active lock. While locked,
/// every setting is frozen; protection cannot be turned off.
pub fn guard_config_change(
    current: &AppConfig,
    proposed: &AppConfig,
    lock: &LockState,
    now: DateTime<Utc>,
) -> Result<()> {
    if !lock.is_active(now) {
        return Ok(());
    }
    if current != proposed {
        return Err(Error::Locked(
            "settings are frozen until the locked session ends".into(),
        ));
    }
    Ok(())
}

/// Guard a proposed new lock state. While locked you may only *extend* the
/// timer, never shorten it or unlock early.
pub fn guard_lock_change(
    current: &LockState,
    proposed: &LockState,
    now: DateTime<Utc>,
) -> Result<()> {
    // The clamp applies to EVERY write, locked or not: a lock can never be
    // set further out than now + MAX_LOCK_DURATION.
    if let Some(until) = proposed.locked_until {
        if proposed.locked && until > max_unlock_at(now) {
            return Err(Error::Locked(format!(
                "lock duration exceeds the {MAX_LOCK_DURATION_DAYS}-day maximum"
            )));
        }
    }
    if !current.is_active(now) {
        return Ok(()); // not locked: any (clamped) change, including starting a lock, is fine
    }
    match (current.locked_until, proposed.locked_until) {
        // Extending (or keeping) the timer is allowed; it must still be locked.
        (Some(cur), Some(new)) if proposed.locked && new >= cur => Ok(()),
        _ => Err(Error::Locked(
            "a locked session can only be extended, not shortened or ended early".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local(h: u32, m: u32) -> DateTime<Local> {
        // Use a fixed date (a Wednesday) for weekday-independent tests.
        Local.with_ymd_and_hms(2026, 7, 15, h, m, 0).unwrap()
    }

    #[test]
    fn overnight_window() {
        let s = Schedule::Windows {
            windows: vec![TimeWindow {
                start_min: 21 * 60,
                end_min: 6 * 60,
                days: vec![],
            }],
        };
        assert!(s.is_active_at(local(22, 0)));
        assert!(s.is_active_at(local(2, 0)));
        assert!(!s.is_active_at(local(12, 0)));
    }

    #[test]
    fn always_on_and_off() {
        assert!(Schedule::AlwaysOn.is_active_at(local(3, 0)));
        assert!(!Schedule::Off.is_active_at(local(3, 0)));
    }

    #[test]
    fn lock_expiry_and_extend_rules() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();
        let future = now + Duration::hours(5);
        let sooner = now + Duration::hours(1);

        let locked = LockState::locked_until(future);
        assert!(locked.is_active(now));

        // Shortening is refused.
        assert!(guard_lock_change(&locked, &LockState::locked_until(sooner), now).is_err());
        // Unlocking early is refused.
        assert!(guard_lock_change(&locked, &LockState::unlocked(), now).is_err());
        // Extending is allowed.
        let longer = LockState::locked_until(now + Duration::hours(10));
        assert!(guard_lock_change(&locked, &longer, now).is_ok());

        // An expired lock imposes no restriction.
        let expired = LockState::locked_until(now - Duration::hours(1));
        assert!(!expired.is_active(now));
        assert!(guard_lock_change(&expired, &LockState::unlocked(), now).is_ok());
    }

    #[test]
    fn rejects_locks_beyond_max() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();
        let cur = LockState::unlocked();
        // 91 days > 90-day ceiling: refused even from unlocked.
        let too_long = LockState::locked_until(now + Duration::days(91));
        assert!(guard_lock_change(&cur, &too_long, now).is_err());
        // Exactly at the ceiling is allowed.
        let ok = LockState::locked_until(now + Duration::days(90));
        assert!(guard_lock_change(&cur, &ok, now).is_ok());
    }

    #[test]
    fn config_frozen_while_locked() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();
        let lock = LockState::locked_until(now + Duration::hours(5));
        let cur = AppConfig::default();
        let mut proposed = cur.clone();
        proposed.protection_enabled = false;
        assert!(guard_config_change(&cur, &proposed, &lock, now).is_err());
        // No change is always fine.
        assert!(guard_config_change(&cur, &cur, &lock, now).is_ok());
    }
}
