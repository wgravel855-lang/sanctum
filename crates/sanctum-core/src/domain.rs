//! Domain normalization and suffix matching.
//!
//! Matching rule: a query host is blocked if the host itself, or any of
//! its parent domains, is present in the block set. So blocking
//! `example.com` also blocks `www.example.com` and `cdn.a.example.com`.

/// Normalize a user- or list-supplied domain string into a canonical
/// lowercase host with no scheme, path, port, wildcard, or trailing dot.
///
/// Returns `None` if the input is empty, a comment, or not a plausible
/// hostname. This is intentionally strict-ish but tolerant of pasted URLs.
pub fn normalize(input: &str) -> Option<String> {
    let mut s = input.trim();
    if s.is_empty() || s.starts_with('#') {
        return None;
    }

    // Strip an inline comment (e.g. "example.com  # note").
    if let Some(idx) = s.find('#') {
        s = s[..idx].trim();
    }

    let mut host = s.to_ascii_lowercase();

    // Strip scheme.
    if let Some(idx) = host.find("://") {
        host = host[idx + 3..].to_string();
    }
    // Strip userinfo.
    if let Some(idx) = host.find('@') {
        host = host[idx + 1..].to_string();
    }
    // Strip path/query/fragment.
    for sep in ['/', '?', '#'] {
        if let Some(idx) = host.find(sep) {
            host.truncate(idx);
        }
    }
    // Strip a trailing port (":8080"). IPv6 literals aren't valid domains here.
    if let Some(idx) = host.rfind(':') {
        if host[idx + 1..].chars().all(|c| c.is_ascii_digit()) && !host[idx + 1..].is_empty() {
            host.truncate(idx);
        }
    }
    // Strip a leading wildcard and stray leading/trailing dots.
    host = host
        .trim_start_matches("*.")
        .trim_matches('.')
        .to_string();

    if is_valid_host(&host) {
        Some(host)
    } else {
        None
    }
}

/// Loose hostname validation: ASCII letters/digits/hyphen labels, at least
/// two labels, each label 1..=63 chars, no leading/trailing hyphen.
pub fn is_valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for label in &labels {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return false;
        }
    }
    // The TLD label must be non-numeric (rejects bare IPv4).
    let tld = labels.last().unwrap();
    if tld.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

/// Yield `host` and each of its parent suffixes, longest first.
/// `a.b.example.com` -> `a.b.example.com`, `b.example.com`, `example.com`, `com`.
pub fn parent_suffixes(host: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(host), |h| {
        h.find('.').map(|idx| &h[idx + 1..])
    })
}

/// True if `host` (or any parent suffix) is contained in `set`.
pub fn is_blocked_by(host: &str, set: &std::collections::HashSet<String>) -> bool {
    let host = host.trim_matches('.').to_ascii_lowercase();
    let blocked = parent_suffixes(&host).any(|suffix| set.contains(suffix));
    blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn normalizes_urls_and_wildcards() {
        assert_eq!(normalize("Example.COM"), Some("example.com".into()));
        assert_eq!(
            normalize("https://www.Example.com/path?x=1"),
            Some("www.example.com".into())
        );
        assert_eq!(normalize("*.ads.example.com"), Some("ads.example.com".into()));
        assert_eq!(normalize("example.com:8443"), Some("example.com".into()));
        assert_eq!(normalize("  example.com.  "), Some("example.com".into()));
        assert_eq!(normalize("user@example.com"), Some("example.com".into()));
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("# comment"), None);
        assert_eq!(normalize("localhost"), None); // single label
        assert_eq!(normalize("1.2.3.4"), None); // bare ipv4
        assert_eq!(normalize("-bad.example.com"), None);
    }

    #[test]
    fn suffix_matching() {
        let mut set = HashSet::new();
        set.insert("example.com".to_string());
        assert!(is_blocked_by("example.com", &set));
        assert!(is_blocked_by("www.example.com", &set));
        assert!(is_blocked_by("a.b.example.com", &set));
        assert!(is_blocked_by("Example.Com.", &set));
        assert!(!is_blocked_by("notexample.com", &set));
        assert!(!is_blocked_by("example.com.evil.com", &set));
    }
}
