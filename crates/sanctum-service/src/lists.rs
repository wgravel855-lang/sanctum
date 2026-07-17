//! The curated lists, embedded at compile time so the service enforces even
//! before any on-disk copy exists. At runtime these are unioned with the
//! user's custom list from the database.

use sanctum_core::{Blocklist, SafeSearchMap};

const ADULT: &str = include_str!("../../../blocklist/adult-domains.txt");
const DOH: &str = include_str!("../../../blocklist/doh-endpoints.txt");
const SAFESEARCH: &str = include_str!("../../../blocklist/safesearch.map");

/// Default number of domains promoted to the always-on HOSTS floor.
pub const FLOOR_SIZE: usize = 24;

pub fn starter_blocklist() -> Blocklist {
    Blocklist::parse(ADULT).0
}

pub fn doh_list() -> Blocklist {
    Blocklist::parse(DOH).0
}

pub fn safesearch_map() -> SafeSearchMap {
    SafeSearchMap::parse(SAFESEARCH).0
}

/// The HOSTS floor (ADR-001 §1): a small curated subset that keeps blocking
/// the worst domains with **no running process** — through resolver death,
/// the boot window, and SAFE_FALLBACK. The full list lives in the resolver.
pub fn floor_domains(block: &Blocklist, n: usize) -> Vec<String> {
    block.sorted().into_iter().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_lists_parse_and_are_nonempty() {
        assert!(starter_blocklist().len() >= 40);
        assert!(doh_list().len() >= 10);
        assert!(!safesearch_map().is_empty());
    }

    #[test]
    fn floor_is_bounded_and_blocked() {
        let block = starter_blocklist();
        let floor = floor_domains(&block, FLOOR_SIZE);
        assert!(floor.len() <= FLOOR_SIZE);
        // Everything in the floor is genuinely in the block set.
        assert!(floor.iter().all(|d| block.is_blocked(d)));
    }
}
