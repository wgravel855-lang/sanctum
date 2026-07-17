//! Management of Sanctum's owned region of the Windows hosts file.
//!
//! Hard rule: we only ever read/replace the bytes *between and including*
//! our two markers. Everything outside is preserved byte-for-byte,
//! including the user's line endings. We never parse or rewrite the rest
//! of the hosts file.

use crate::error::{Error, Result};
use crate::{HOSTS_END, HOSTS_START};

/// Marker for the machinery in this module. Zero-sized; groups the fns.
pub struct HostsSection;

fn err(msg: &str) -> Error {
    Error::Hosts(msg.to_string())
}

/// Detect the dominant newline style used by `content`.
fn newline_of(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else if content.contains('\n') {
        "\n"
    } else {
        // Empty or single-line: default to Windows-native.
        "\r\n"
    }
}

fn is_marker(line: &str, marker: &str) -> bool {
    line.trim_end_matches(|c| c == '\n' || c == '\r').trim() == marker
}

/// Locate the byte range `[start, end)` covering the entire Sanctum
/// section, including both marker lines and the end marker's trailing
/// newline.
///
/// * `Ok(None)`   — no section present (zero markers of either kind).
/// * `Ok(Some(_))`— exactly one well-formed section.
/// * `Err(_)`     — markers are unbalanced (this is the failure the
///   integrity check guards against).
pub fn section_byte_range(content: &str) -> Result<Option<(usize, usize)>> {
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;
    let mut starts = 0usize;
    let mut ends = 0usize;
    let mut offset = 0usize;

    for line in content.split_inclusive('\n') {
        if is_marker(line, HOSTS_START) {
            starts += 1;
            if start.is_none() {
                start = Some(offset);
            }
        } else if is_marker(line, HOSTS_END) {
            ends += 1;
            end = Some(offset + line.len());
        }
        offset += line.len();
    }

    match (starts, ends) {
        (0, 0) => Ok(None),
        (1, 1) => {
            let (s, e) = (start.unwrap(), end.unwrap());
            if s >= e {
                Err(err("hosts END marker appears before START marker"))
            } else {
                Ok(Some((s, e)))
            }
        }
        _ => Err(err(&format!(
            "hosts markers unbalanced: {starts} START / {ends} END (expected 1 each)"
        ))),
    }
}

/// True if the section markers are balanced (0 or exactly 1 well-formed
/// section). Used by the integrity check.
pub fn markers_balanced(content: &str) -> bool {
    section_byte_range(content).is_ok()
}

/// Render the full Sanctum section (both markers included) for the given
/// sinkhole domains, using `newline` between lines. No trailing newline.
pub fn render_section(domains: &[String], sink_v4: &str, sink_v6: &str, newline: &str) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(domains.len() * 4 + 4);
    lines.push(HOSTS_START.to_string());
    lines.push("# Managed by Sanctum — do not edit between these markers.".to_string());
    lines.push("# Editing here does nothing while protection is active;".to_string());
    lines.push("# the Sanctum service rewrites this section.".to_string());
    for d in domains {
        lines.push(format!("{sink_v4} {d}"));
        lines.push(format!("{sink_v6} {d}"));
        // Also cover the ubiquitous "www." host explicitly (hosts files
        // have no wildcard; the DNS layer covers deeper subdomains).
        lines.push(format!("{sink_v4} www.{d}"));
        lines.push(format!("{sink_v6} www.{d}"));
    }
    lines.push(HOSTS_END.to_string());
    lines.join(newline)
}

/// Insert or replace the Sanctum section in `content`, leaving all other
/// bytes untouched. Returns the new file body.
pub fn upsert_section(
    content: &str,
    domains: &[String],
    sink_v4: &str,
    sink_v6: &str,
) -> Result<String> {
    let nl = newline_of(content);
    let rendered = render_section(domains, sink_v4, sink_v6, nl);

    if let Some((s, e)) = section_byte_range(content)? {
        // Replace exactly the old section's byte range. `e` already
        // consumed the end marker's trailing newline (if any), so we add
        // one back after the rendered block to keep separation.
        let mut out = String::with_capacity(content.len() + rendered.len());
        out.push_str(&content[..s]);
        out.push_str(&rendered);
        out.push_str(nl);
        out.push_str(&content[e..]);
        Ok(out)
    } else {
        let mut out = String::with_capacity(content.len() + rendered.len() + 8);
        out.push_str(content);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(nl);
        }
        if !out.is_empty() {
            out.push_str(nl); // blank separator line
        }
        out.push_str(&rendered);
        out.push_str(nl);
        Ok(out)
    }
}

/// Remove the Sanctum section entirely (used on uninstall / disable).
/// Bytes outside the section are preserved exactly.
pub fn remove_section(content: &str) -> Result<String> {
    if let Some((s, e)) = section_byte_range(content)? {
        let mut out = String::with_capacity(content.len());
        out.push_str(&content[..s]);
        out.push_str(&content[e..]);
        Ok(out)
    } else {
        Ok(content.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V4: &str = "0.0.0.0";
    const V6: &str = "::";

    #[test]
    fn inserts_and_preserves_surrounding_bytes() {
        let original = "127.0.0.1 localhost\r\n::1 localhost\r\n";
        let domains = vec!["bad.com".to_string()];
        let out = upsert_section(original, &domains, V4, V6).unwrap();
        assert!(out.starts_with("127.0.0.1 localhost\r\n::1 localhost\r\n"));
        assert!(out.contains("# >>> SANCTUM START"));
        assert!(out.contains("0.0.0.0 bad.com"));
        assert!(out.contains(":: bad.com"));
        assert!(out.contains("0.0.0.0 www.bad.com"));
        assert!(out.contains("# <<< SANCTUM END"));
    }

    #[test]
    fn upsert_is_idempotent_in_count() {
        let original = "127.0.0.1 localhost\n";
        let out1 = upsert_section(original, &["a.com".into()], V4, V6).unwrap();
        let out2 = upsert_section(&out1, &["a.com".into(), "b.com".into()], V4, V6).unwrap();
        assert_eq!(out2.matches("# >>> SANCTUM START").count(), 1);
        assert_eq!(out2.matches("# <<< SANCTUM END").count(), 1);
        assert!(out2.contains("0.0.0.0 b.com"));
    }

    #[test]
    fn remove_restores_original() {
        let original = "127.0.0.1 localhost\r\nsome other line\r\n";
        let with = upsert_section(original, &["x.com".into()], V4, V6).unwrap();
        let without = remove_section(&with).unwrap();
        // Surrounding lines are intact; our section is gone.
        assert!(without.contains("127.0.0.1 localhost"));
        assert!(without.contains("some other line"));
        assert!(!without.contains("SANCTUM"));
    }

    #[test]
    fn detects_unbalanced_markers() {
        let bad = "# >>> SANCTUM START\n0.0.0.0 a.com\n# >>> SANCTUM START\n# <<< SANCTUM END\n";
        assert!(!markers_balanced(bad));
        assert!(section_byte_range(bad).is_err());

        let end_before_start = "# <<< SANCTUM END\n# >>> SANCTUM START\n";
        assert!(!markers_balanced(end_before_start));

        let clean = "127.0.0.1 localhost\n";
        assert!(markers_balanced(clean));
        assert!(section_byte_range(clean).unwrap().is_none());
    }
}
