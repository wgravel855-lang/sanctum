//! SafeSearch / Restricted-Mode CNAME map.
//!
//! The DNS resolver answers each mapped host with a CNAME to the "safe"
//! target; the client then re-queries the target, which is forwarded
//! upstream normally. Matching is by exact host (we only rewrite the
//! specific search hostnames, never their whole domains).

use crate::domain;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default, Clone)]
pub struct SafeSearchMap {
    map: HashMap<String, String>,
}

impl SafeSearchMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a `safesearch.map` body: `"<host> <cname-target>"` per line.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut m = SafeSearchMap::new();
        let mut warnings = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (Some(host), Some(target)) = (parts.next(), parts.next()) else {
                warnings.push(format!("line {}: expected '<host> <target>'", i + 1));
                continue;
            };
            match (domain::normalize(host), domain::normalize(target)) {
                (Some(h), Some(t)) => {
                    m.map.insert(h, t);
                }
                _ => warnings.push(format!("line {}: invalid host or target", i + 1)),
            }
        }
        (m, warnings)
    }

    /// The safe CNAME target for `host`, if it is a mapped search host.
    pub fn lookup(&self, host: &str) -> Option<&str> {
        let h = host.trim_matches('.').to_ascii_lowercase();
        self.map.get(&h).map(|s| s.as_str())
    }

    /// All CNAME targets (the "safe" hosts). The resolver must never
    /// sinkhole or rewrite these, so they resolve normally.
    pub fn targets(&self) -> HashSet<String> {
        self.map.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_looks_up() {
        let text = "# c\ngoogle.com  forcesafesearch.google.com\nwww.youtube.com restrict.youtube.com\nbad line\n";
        let (m, warnings) = SafeSearchMap::parse(text);
        assert_eq!(m.len(), 2);
        assert_eq!(m.lookup("Google.com"), Some("forcesafesearch.google.com"));
        assert_eq!(m.lookup("www.youtube.com"), Some("restrict.youtube.com"));
        assert_eq!(m.lookup("example.com"), None);
        assert!(m.targets().contains("restrict.youtube.com"));
        assert_eq!(warnings.len(), 1);
    }
}
