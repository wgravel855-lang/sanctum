//! Parsing and evaluation of the blocked-domain set.

use crate::domain;
use std::collections::HashSet;

/// A set of blocked registrable domains. Matching is by suffix, so an
/// entry blocks the domain and all of its subdomains.
#[derive(Debug, Default, Clone)]
pub struct Blocklist {
    blocked: HashSet<String>,
}

/// A non-fatal problem encountered while parsing a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseWarning {
    pub line: usize,
    pub content: String,
    pub reason: String,
}

impl Blocklist {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a blocklist file body. Returns the list plus any per-line
    /// warnings (invalid domains are skipped, never silently accepted).
    pub fn parse(text: &str) -> (Self, Vec<ParseWarning>) {
        let mut list = Blocklist::new();
        let mut warnings = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            match domain::normalize(trimmed) {
                Some(d) => {
                    list.blocked.insert(d);
                }
                None => warnings.push(ParseWarning {
                    line: i + 1,
                    content: trimmed.to_string(),
                    reason: "not a valid domain".to_string(),
                }),
            }
        }
        (list, warnings)
    }

    /// Add a domain (already-normalized or raw). Returns true if newly added.
    pub fn add(&mut self, domain: &str) -> bool {
        match domain::normalize(domain) {
            Some(d) => self.blocked.insert(d),
            None => false,
        }
    }

    /// Remove a domain. Returns true if it was present.
    pub fn remove(&mut self, domain: &str) -> bool {
        match domain::normalize(domain) {
            Some(d) => self.blocked.remove(&d),
            None => false,
        }
    }

    /// True if `host` (or a parent domain) is blocked.
    pub fn is_blocked(&self, host: &str) -> bool {
        domain::is_blocked_by(host, &self.blocked)
    }

    /// True if this exact normalized domain is an entry (not suffix match).
    pub fn contains_exact(&self, domain: &str) -> bool {
        domain::normalize(domain)
            .map(|d| self.blocked.contains(&d))
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.blocked.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocked.is_empty()
    }

    /// Merge another list into this one (union). Used to combine the
    /// starter list + custom list + imported list.
    pub fn merge(&mut self, other: &Blocklist) {
        for d in &other.blocked {
            self.blocked.insert(d.clone());
        }
    }

    /// Sorted view, suitable for rendering the hosts section deterministically.
    pub fn sorted(&self) -> Vec<String> {
        let mut v: Vec<String> = self.blocked.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.blocked.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_dedupes() {
        let text = "# header\nexample.com\nExample.com\n\nhttps://www.foo.com/x\nnot a domain\n";
        let (list, warnings) = Blocklist::parse(text);
        assert_eq!(list.len(), 2); // example.com (deduped) + www.foo.com
        assert!(list.is_blocked("sub.example.com"));
        assert!(list.is_blocked("www.foo.com"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].content, "not a domain");
    }

    #[test]
    fn add_remove_and_merge() {
        let mut a = Blocklist::new();
        assert!(a.add("a.com"));
        assert!(!a.add("a.com"));
        let mut b = Blocklist::new();
        b.add("b.com");
        a.merge(&b);
        assert!(a.is_blocked("x.a.com"));
        assert!(a.is_blocked("b.com"));
        assert!(a.remove("a.com"));
        assert!(!a.is_blocked("a.com"));
    }
}
