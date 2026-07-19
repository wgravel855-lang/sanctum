//! Honest accountability notifier.
//!
//! This is the ONLY code in Sanctum that makes an outbound HTTP request, and it
//! runs ONLY when the user has configured an accountability webhook. It sends
//! short, human-readable TEXT signals ("protection was turned off at ...") to a
//! channel the user owns (Discord / Slack / ntfy / any HTTPS endpoint). It never
//! sends browsing history, visited domains, the block list, or any content —
//! Sanctum keeps no account and runs no server of its own.

use std::time::Duration;

/// Fire-and-forget: post `text` to the user's `webhook`, best-effort. Spawns a
/// short-lived thread so callers never block the enforcement/IPC path, and a
/// failure (offline, bad URL) is silently tolerated.
pub fn notify(webhook: &str, text: &str) {
    if webhook.trim().is_empty() {
        return;
    }
    let url = webhook.trim().to_string();
    let msg = text.to_string();
    std::thread::spawn(move || post(&url, &msg));
}

/// Synchronous variant for callers that exit the process immediately after
/// (e.g. the uninstall path) — waits for the POST so the signal actually leaves
/// before the process dies. Otherwise identical to [`notify`].
pub fn notify_blocking(webhook: &str, text: &str) {
    let w = webhook.trim();
    if !w.is_empty() {
        post(w, text);
    }
}

/// Send an SMS via the user's own Twilio account. Fire-and-forget.
pub fn send_sms(sid: &str, token: &str, from: &str, to: &str, text: &str) {
    let (s, t, f, o, m) = (
        sid.trim().to_string(),
        token.trim().to_string(),
        from.trim().to_string(),
        to.trim().to_string(),
        text.to_string(),
    );
    std::thread::spawn(move || post_sms(&s, &t, &f, &o, &m));
}

/// Blocking Twilio SMS, for callers that exit immediately after (uninstall).
pub fn send_sms_blocking(sid: &str, token: &str, from: &str, to: &str, text: &str) {
    post_sms(sid.trim(), token.trim(), from.trim(), to.trim(), text);
}

fn post_sms(sid: &str, token: &str, from: &str, to: &str, text: &str) {
    if sid.is_empty() || token.is_empty() || from.is_empty() || to.is_empty() {
        return;
    }
    let url = format!("https://api.twilio.com/2010-04-01/Accounts/{sid}/Messages.json");
    let auth = base64(&format!("{sid}:{token}"));
    let body = format!(
        "From={}&To={}&Body={}",
        pct(from),
        pct(to),
        pct(text),
    );
    let req = ureq::post(&url)
        .timeout(Duration::from_secs(12))
        .set("Authorization", &format!("Basic {auth}"))
        .set("Content-Type", "application/x-www-form-urlencoded")
        .set("User-Agent", "Sanctum");
    match req.send_string(&body) {
        Ok(_) => tracing::info!("accountability SMS sent"),
        Err(e) => tracing::warn!(error = %e, "accountability SMS failed"),
    }
}

/// Standard base64 (for the Twilio HTTP Basic auth header). Hand-rolled so this
/// module pulls no extra crates.
fn base64(input: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = input.as_bytes();
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        out.push(T[(n[0] >> 2) as usize] as char);
        out.push(T[(((n[0] & 0x03) << 4) | (n[1] >> 4)) as usize] as char);
        out.push(if c.len() > 1 { T[(((n[1] & 0x0f) << 2) | (n[2] >> 6)) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { T[(n[2] & 0x3f) as usize] as char } else { '=' });
    }
    out
}

/// Percent-encode a form value (application/x-www-form-urlencoded), encoding
/// everything but the RFC 3986 unreserved set.
fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn post(url: &str, msg: &str) {
    let (body, ctype) = build_body(url, msg);
    let req = ureq::post(url)
        .timeout(Duration::from_secs(12))
        .set("Content-Type", ctype)
        .set("User-Agent", "Sanctum");
    match req.send_string(&body) {
        Ok(_) => tracing::info!("accountability notification sent"),
        Err(e) => tracing::warn!(error = %e, "accountability notification failed"),
    }
}

/// Format the payload for the detected provider. Discord and Slack take a JSON
/// object with a known field; ntfy takes a plain-text body; everything else
/// gets a small generic JSON object with a `text` field.
fn build_body(url: &str, text: &str) -> (String, &'static str) {
    let lower = url.to_ascii_lowercase();
    if lower.contains("discord.com") || lower.contains("discordapp.com") {
        (format!(r#"{{"content":{}}}"#, json_string(text)), "application/json")
    } else if lower.contains("hooks.slack.com") {
        (format!(r#"{{"text":{}}}"#, json_string(text)), "application/json")
    } else if lower.contains("ntfy.sh") || lower.contains("/ntfy") {
        (text.to_string(), "text/plain; charset=utf-8")
    } else {
        (
            format!(r#"{{"text":{},"source":"sanctum"}}"#, json_string(text)),
            "application/json",
        )
    }
}

/// Minimal JSON string encoder (value in quotes), so we don't pull serde_json
/// into this tiny module and can guarantee no structured data leaks.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_per_provider() {
        let (b, c) = build_body("https://discord.com/api/webhooks/1/x", "hi \"there\"");
        assert_eq!(b, r#"{"content":"hi \"there\""}"#);
        assert_eq!(c, "application/json");

        let (b, _) = build_body("https://hooks.slack.com/services/x", "off");
        assert_eq!(b, r#"{"text":"off"}"#);

        let (b, c) = build_body("https://ntfy.sh/my-topic", "off");
        assert_eq!(b, "off");
        assert_eq!(c, "text/plain; charset=utf-8");

        let (b, _) = build_body("https://example.com/hook", "off");
        assert_eq!(b, r#"{"text":"off","source":"sanctum"}"#);
    }

    #[test]
    fn json_string_escapes() {
        assert_eq!(json_string("a\nb\"c"), r#""a\nb\"c""#);
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64("AC:tok"), "QUM6dG9r");
        assert_eq!(base64("Man"), "TWFu");
        assert_eq!(base64("Ma"), "TWE=");
        assert_eq!(base64("M"), "TQ==");
    }

    #[test]
    fn pct_encodes_form_values() {
        assert_eq!(pct("+15551234567"), "%2B15551234567");
        assert_eq!(pct("Protection was OFF."), "Protection%20was%20OFF.");
        assert_eq!(pct("a-b_c.d~e"), "a-b_c.d~e");
    }
}
