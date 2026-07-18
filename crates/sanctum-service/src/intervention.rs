//! Block-moment intervention detection (v0.1.5 §A).
//!
//! A single blocked DNS lookup is NOT user intent: browsers speculatively
//! prefetch DNS for links merely visible on a page. So we debounce a stream of
//! sinkholed adult-block hits into calm, deliberate-looking intervention
//! triggers, and never fire more than once per cooldown.
//!
//! [`Detector`] is a pure state machine (time is passed in) so the debounce
//! rules are exhaustively unit-tested. [`InterventionCenter`] wraps it with a
//! real monotonic clock and the shared state the IPC layer polls.

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use sanctum_core::ipc::InterventionDto;

/// ≥ this many hits for one domain inside the window → intervene.
const BURST_SAME: usize = 3;
/// ≥ this many *distinct* blocked domains inside the window → intervene.
const DISTINCT_DOMAINS: usize = 2;
/// Sliding debounce window.
const WINDOW_MS: u64 = 10_000;
/// At most one intervention per this cooldown.
const COOLDOWN_MS: u64 = 180_000;
/// The UI counts as "present" if it polled within this long.
const UI_ACTIVE_MS: u64 = 10_000;
/// A pending intervention older than this is stale and won't pop a window.
const PENDING_TTL_MS: u64 = 15_000;

/// Fold `www.pornhub.com` and `cdn.pornhub.com` into one urge. Registrable
/// domain = last two dot-labels, which is exact for the single-label TLDs the
/// starter blocklist uses (`.com`, `.net`, …). Multi-part TLDs (`.co.uk`) fold
/// one label too coarsely, which only makes the debounce *slightly* stricter.
pub fn group_key(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    let n = labels.len();
    if n >= 2 {
        format!("{}.{}", labels[n - 2], labels[n - 1])
    } else {
        host
    }
}

/// Pure debounce + rate-limit state machine. Caller supplies monotonic
/// milliseconds so this is fully deterministic under test.
pub struct Detector {
    recent: VecDeque<(u64, String)>, // (time_ms, group_key)
    last_trigger_ms: Option<u64>,
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}

impl Detector {
    pub fn new() -> Self {
        Self {
            recent: VecDeque::new(),
            last_trigger_ms: None,
        }
    }

    /// Record one sinkholed adult-block hit at `now_ms`. Returns `true` iff this
    /// hit should open a *new* intervention window right now. Hits during the
    /// cooldown still count toward the window but never re-trigger.
    pub fn record(&mut self, now_ms: u64, host: &str) -> bool {
        let key = group_key(host);

        // Drop events that have aged out of the sliding window.
        while let Some((t, _)) = self.recent.front() {
            if now_ms.saturating_sub(*t) > WINDOW_MS {
                self.recent.pop_front();
            } else {
                break;
            }
        }
        self.recent.push_back((now_ms, key.clone()));

        // Still counted above, but no fresh trigger during cooldown.
        if let Some(last) = self.last_trigger_ms {
            if now_ms.saturating_sub(last) < COOLDOWN_MS {
                return false;
            }
        }

        let same = self.recent.iter().filter(|(_, k)| *k == key).count();
        let distinct: HashSet<&String> = self.recent.iter().map(|(_, k)| k).collect();

        if same >= BURST_SAME || distinct.len() >= DISTINCT_DOMAINS {
            self.last_trigger_ms = Some(now_ms);
            // Consume the window so the same burst can't immediately re-arm once
            // the cooldown lapses.
            self.recent.clear();
            true
        } else {
            false
        }
    }
}

struct CenterState {
    detector: Detector,
    /// (armed_at_ms, domain) awaiting the UI to pick it up.
    pending: Option<(u64, String)>,
    urges_while_away: u32,
    last_poll_ms: Option<u64>,
}

/// Shared, thread-safe home for the debounce state. The block-event consumer
/// task feeds it; the IPC handler polls it.
pub struct InterventionCenter {
    start: Instant,
    inner: Mutex<CenterState>,
}

impl Default for InterventionCenter {
    fn default() -> Self {
        Self::new()
    }
}

impl InterventionCenter {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            inner: Mutex::new(CenterState {
                detector: Detector::new(),
                pending: None,
                urges_while_away: 0,
                last_poll_ms: None,
            }),
        }
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Feed one sinkholed adult-block host. Runs the debounce; if it fires,
    /// either arm a poppable intervention (UI present) or bump the
    /// while-you-were-away counter (UI absent).
    pub fn record_block(&self, host: &str) {
        let now = self.now_ms();
        let mut s = self.inner.lock().unwrap();
        if s.detector.record(now, host) {
            let ui_active = s
                .last_poll_ms
                .is_some_and(|lp| now.saturating_sub(lp) < UI_ACTIVE_MS);
            if ui_active {
                s.pending = Some((now, host.to_string()));
            } else {
                s.urges_while_away = s.urges_while_away.saturating_add(1);
            }
        }
    }

    /// Arm an intervention unconditionally ("I need help now" / panic hotkey).
    pub fn trigger_manual(&self) {
        let now = self.now_ms();
        let mut s = self.inner.lock().unwrap();
        s.pending = Some((now, String::new()));
    }

    /// The UI's short-interval poll. Marks the UI present, returns (and clears)
    /// any fresh pending intervention, and drains the away counter.
    pub fn poll(&self) -> InterventionDto {
        let now = self.now_ms();
        let mut s = self.inner.lock().unwrap();
        s.last_poll_ms = Some(now);
        let fresh = s
            .pending
            .take()
            .filter(|(t, _)| now.saturating_sub(*t) < PENDING_TTL_MS);
        let away = std::mem::take(&mut s.urges_while_away);
        InterventionDto {
            pending: fresh.is_some(),
            domain: fresh
                .map(|(_, d)| d)
                .filter(|d| !d.is_empty()),
            urges_while_away: away,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_key_folds_subdomains() {
        assert_eq!(group_key("www.pornhub.com"), "pornhub.com");
        assert_eq!(group_key("cdn.a.b.pornhub.com"), "pornhub.com");
        assert_eq!(group_key("pornhub.com."), "pornhub.com");
        assert_eq!(group_key("localhost"), "localhost");
    }

    #[test]
    fn single_prefetch_does_not_trigger() {
        let mut d = Detector::new();
        assert!(!d.record(0, "bad.com"));
        assert!(!d.record(100, "bad.com")); // two isn't enough
    }

    #[test]
    fn three_hits_same_domain_within_window_trigger() {
        let mut d = Detector::new();
        assert!(!d.record(0, "www.bad.com"));
        assert!(!d.record(1_000, "bad.com"));
        assert!(d.record(2_000, "cdn.bad.com")); // 3rd of the same registrable domain
    }

    #[test]
    fn two_distinct_domains_within_window_trigger() {
        let mut d = Detector::new();
        assert!(!d.record(0, "bad.com"));
        assert!(d.record(500, "evil.com")); // 2 distinct blocked domains
    }

    #[test]
    fn hits_outside_window_do_not_accumulate() {
        let mut d = Detector::new();
        assert!(!d.record(0, "bad.com"));
        assert!(!d.record(1_000, "bad.com"));
        // 3rd arrives after the first two aged out (>10s window).
        assert!(!d.record(12_000, "bad.com"));
    }

    #[test]
    fn cooldown_blocks_a_second_trigger() {
        let mut d = Detector::new();
        assert!(!d.record(0, "bad.com"));
        assert!(d.record(1_000, "evil.com")); // first trigger at t=1s
        // A fresh burst well within the 3-minute cooldown must not re-trigger.
        assert!(!d.record(20_000, "bad.com"));
        assert!(!d.record(20_500, "evil.com"));
        assert!(!d.record(21_000, "worse.com"));
        // After the cooldown lapses, it can trigger again.
        assert!(!d.record(190_000, "bad.com"));
        assert!(d.record(190_500, "evil.com"));
    }
}
