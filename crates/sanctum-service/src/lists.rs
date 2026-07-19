//! The curated lists, embedded at compile time so the service enforces even
//! before any on-disk copy exists. At runtime these are unioned with the
//! user's custom list from the database.

use sanctum_core::{Blocklist, SafeSearchMap};
use std::sync::OnceLock;

/// Small curated worst-offender list; seeds the HOSTS floor.
const ADULT: &str = include_str!("../../../blocklist/adult-domains.txt");
/// Bulk compiled list (StevenBlack porn extension, MIT; see THIRD_PARTY.md).
const ADULT_FULL: &str = include_str!("../../../blocklist/adult-domains-full.txt");
/// Bypass-tool list: DoH/VPN/proxy/Tor (hagezi, GPL-3.0; see THIRD_PARTY.md).
const BYPASS: &str = include_str!("../../../blocklist/bypass-domains.txt");
/// Curated bypass additions the upstream misses (e.g. Opera's VPN proxy).
const BYPASS_EXTRA: &str = include_str!("../../../blocklist/bypass-extra.txt");
/// Strict-mode list: mainstream suggestive-content gateways (opt-in).
const STRICT: &str = include_str!("../../../blocklist/strict-domains.txt");
const DOH: &str = include_str!("../../../blocklist/doh-endpoints.txt");
const SAFESEARCH: &str = include_str!("../../../blocklist/safesearch.map");

/// Default number of domains promoted to the always-on HOSTS floor.
pub const FLOOR_SIZE: usize = 24;

/// The full embedded block set (curated ∪ bulk), parsed once per process —
/// the bulk list is ~48k entries and reload runs every reconcile tick.
pub fn starter_blocklist() -> Blocklist {
    static FULL: OnceLock<Blocklist> = OnceLock::new();
    FULL.get_or_init(|| {
        let mut b = Blocklist::parse(ADULT_FULL).0;
        b.merge(&Blocklist::parse(ADULT).0);
        b
    })
    .clone()
}

/// The small curated list on its own (floor seeding + tests).
pub fn curated_blocklist() -> Blocklist {
    Blocklist::parse(ADULT).0
}

pub fn doh_list() -> Blocklist {
    Blocklist::parse(DOH).0
}

/// Strict-mode block set (mainstream suggestive-content gateways), parsed once.
pub fn strict_blocklist() -> Blocklist {
    static STRICT_SET: OnceLock<Blocklist> = OnceLock::new();
    STRICT_SET.get_or_init(|| Blocklist::parse(STRICT).0).clone()
}

/// Bypass-tool block set (DoH resolvers, VPN/proxy services, Tor). ~17k
/// entries, parsed once per process behind a `OnceLock`.
pub fn bypass_blocklist() -> Blocklist {
    static BYPASS_SET: OnceLock<Blocklist> = OnceLock::new();
    BYPASS_SET
        .get_or_init(|| {
            let mut b = Blocklist::parse(BYPASS).0;
            b.merge(&Blocklist::parse(BYPASS_EXTRA).0);
            b
        })
        .clone()
}

pub fn safesearch_map() -> SafeSearchMap {
    SafeSearchMap::parse(SAFESEARCH).0
}

/// The HOSTS floor (ADR-001 §1): a small curated subset that keeps blocking
/// the worst domains with **no running process** — through resolver death,
/// the boot window, and SAFE_FALLBACK. The full list lives only in the
/// resolver: HOSTS must stay tiny (the Windows stub resolver scans it
/// linearly), so the floor is drawn from the curated list, filtered by the
/// live block set so an empty set (enforcement off) yields an empty floor.
pub fn floor_domains(block: &Blocklist, n: usize) -> Vec<String> {
    static CURATED_SORTED: OnceLock<Vec<String>> = OnceLock::new();
    CURATED_SORTED
        .get_or_init(|| curated_blocklist().sorted())
        .iter()
        .filter(|d| block.is_blocked(d))
        .take(n)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_lists_parse_and_are_nonempty() {
        assert!(curated_blocklist().len() >= 40);
        assert!(
            starter_blocklist().len() >= 40_000,
            "bulk list missing or truncated: {}",
            starter_blocklist().len()
        );
        assert!(doh_list().len() >= 10);
        assert!(
            bypass_blocklist().len() >= 10_000,
            "bypass list missing or truncated: {}",
            bypass_blocklist().len()
        );
        assert!(!safesearch_map().is_empty());
    }

    #[test]
    fn strict_list_covers_named_gateways() {
        let s = strict_blocklist();
        assert!(s.len() >= 40, "strict list too small: {}", s.len());
        for d in ["instagram.com", "pinterest.com", "reddit.com", "tiktok.com", "tinder.com"] {
            assert!(s.is_blocked(d), "strict should block {d}");
            assert!(s.is_blocked(&format!("www.{d}")), "strict should block www.{d}");
        }
        // Overtly adult / excluded gateways must NOT be in the strict tier.
        assert!(!s.is_blocked("discord.com"));
    }

    #[test]
    fn bypass_covers_opera_vpn_and_common_tools() {
        let b = bypass_blocklist();
        // Opera's modern built-in VPN proxy (curated supplement, suffix match).
        assert!(b.is_blocked("de0.opera-proxy.net"));
        assert!(b.is_blocked("us1.opera-proxy.net"));
        // Opera's older SurfEasy backend (from the upstream list).
        assert!(b.is_blocked("api.surfeasy.com"));
    }

    #[test]
    fn full_list_covers_the_curated_floor() {
        let full = starter_blocklist();
        for d in curated_blocklist().sorted() {
            assert!(full.is_blocked(&d), "curated entry missing from full: {d}");
        }
    }

    #[test]
    fn floor_is_bounded_curated_and_blocked() {
        let block = starter_blocklist();
        let floor = floor_domains(&block, FLOOR_SIZE);
        assert!(!floor.is_empty());
        assert!(floor.len() <= FLOOR_SIZE);
        let curated = curated_blocklist();
        // The floor is drawn from the curated list and everything in it is
        // genuinely blocked by the live set.
        assert!(floor.iter().all(|d| curated.contains_exact(d)));
        assert!(floor.iter().all(|d| block.is_blocked(d)));
    }

    #[test]
    fn empty_block_set_yields_empty_floor() {
        let floor = floor_domains(&Blocklist::new(), FLOOR_SIZE);
        assert!(floor.is_empty());
    }
}
