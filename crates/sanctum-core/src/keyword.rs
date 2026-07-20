//! Keyword rules matched against the DOMAIN NAME.
//!
//! Sanctum filters at the DNS layer, so the only text it ever sees is the
//! hostname being resolved. That is enough to catch the long tail of sites whose
//! names describe themselves, which a fixed blocklist can never keep up with.
//! It is NOT page-content filtering: HTTPS encrypts the URL path and the page,
//! and Sanctum refuses to install a MITM root certificate to see them. The UI
//! and the site say so plainly.
//!
//! # Avoiding the Scunthorpe problem
//!
//! Naive substring matching is actively harmful: `sex` would block
//! `essex.ac.uk`, and `anal` would block `analytics.google.com`, breaking huge
//! parts of the web. So every keyword carries a match mode:
//!
//! * [`MatchMode::Substring`] — for long, distinctive terms that are safe
//!   anywhere inside a label (`porn` in `freeporn`).
//! * [`MatchMode::Token`] — for short or ambiguous terms, which must appear as a
//!   COMPLETE token. `sex` then matches `sex.com` but not `essex`, and `cunt`
//!   matches nothing in `scunthorpe`.
//!
//! Labels are split into tokens on any non-letter (hyphens, digits, underscore),
//! so `free-porn-4-you` yields `free`, `porn`, `you`.

use serde::{Deserialize, Serialize};

/// User-added keywords at least this long are safe enough to match as a
/// substring; anything shorter is restricted to whole-token matching.
pub const MIN_USER_SUBSTRING_LEN: usize = 5;

/// How a keyword must appear in a domain label to count as a match.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// Must be a complete token within a label (safe for short/ambiguous words).
    Token,
    /// May appear anywhere inside a label (only for distinctive words).
    Substring,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Keyword {
    pub word: String,
    pub mode: MatchMode,
}

impl Keyword {
    pub fn token(word: &str) -> Self {
        Self { word: word.trim().to_lowercase(), mode: MatchMode::Token }
    }

    pub fn substring(word: &str) -> Self {
        Self { word: word.trim().to_lowercase(), mode: MatchMode::Substring }
    }

    /// A keyword the user typed. Short words are pinned to token matching so a
    /// three-letter entry can't quietly take out half the web.
    pub fn user(word: &str) -> Self {
        let w = word.trim().to_lowercase();
        if w.chars().count() >= MIN_USER_SUBSTRING_LEN {
            Self { word: w, mode: MatchMode::Substring }
        } else {
            Self { word: w, mode: MatchMode::Token }
        }
    }
}

/// A compiled set of keyword rules.
#[derive(Clone, Debug, Default)]
pub struct KeywordSet {
    keywords: Vec<Keyword>,
}

impl KeywordSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.keywords.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty()
    }

    pub fn insert(&mut self, k: Keyword) {
        if k.word.is_empty() || self.keywords.iter().any(|e| e.word == k.word) {
            return;
        }
        self.keywords.push(k);
    }

    pub fn extend<I: IntoIterator<Item = Keyword>>(&mut self, it: I) {
        for k in it {
            self.insert(k);
        }
    }

    /// Parse the built-in rule file. `~word` is a substring rule, a bare `word`
    /// is a token rule; `#` starts a comment and blank lines are ignored.
    pub fn parse(text: &str) -> Self {
        let mut set = Self::new();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            set.insert(match line.strip_prefix('~') {
                Some(rest) => Keyword::substring(rest),
                None => Keyword::token(line),
            });
        }
        set
    }

    /// The keyword that matches `domain`, if any. Case-insensitive.
    pub fn match_of(&self, domain: &str) -> Option<&Keyword> {
        if self.keywords.is_empty() {
            return None;
        }
        let host = domain.trim().trim_end_matches('.').to_lowercase();
        if host.is_empty() {
            return None;
        }
        for label in host.split('.') {
            if label.is_empty() {
                continue;
            }
            for k in &self.keywords {
                let hit = match k.mode {
                    MatchMode::Substring => label.contains(k.word.as_str()),
                    MatchMode::Token => tokens(label).any(|t| t == k.word),
                };
                if hit {
                    return Some(k);
                }
            }
        }
        None
    }

    pub fn is_blocked(&self, domain: &str) -> bool {
        self.match_of(domain).is_some()
    }
}

/// Split a label into letter-only tokens: `free-porn-4-you` -> free, porn, you.
fn tokens(label: &str) -> impl Iterator<Item = &str> {
    label.split(|c: char| !c.is_ascii_alphabetic()).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> KeywordSet {
        KeywordSet::parse(
            "# distinctive, safe anywhere in a label\n\
             ~porn\n\
             ~hentai\n\
             ~camgirl\n\
             # short or ambiguous: whole token only\n\
             sex\n\
             anal\n\
             cunt\n\
             xxx\n",
        )
    }

    #[test]
    fn substring_rules_catch_the_long_tail() {
        let s = set();
        assert!(s.is_blocked("freeporn.com"));
        assert!(s.is_blocked("www.someporntube.net"));
        assert!(s.is_blocked("best-hentai-site.org"));
        assert!(s.is_blocked("camgirls.example"));
        // hyphens and digits split tokens but substrings still match
        assert!(s.is_blocked("free-porn-4-you.net"));
    }

    #[test]
    fn token_rules_do_not_scunthorpe() {
        let s = set();
        // The whole point: these must NOT be blocked.
        assert!(!s.is_blocked("essex.ac.uk"));
        assert!(!s.is_blocked("sussex.ac.uk"));
        assert!(!s.is_blocked("middlesex.gov.uk"));
        assert!(!s.is_blocked("analytics.google.com"));
        assert!(!s.is_blocked("google-analytics.com"));
        assert!(!s.is_blocked("scunthorpe.gov.uk"));
        assert!(!s.is_blocked("canal-boats.co.uk"));
        assert!(!s.is_blocked("sexualhealth.org"));

        // But the bare token still matches.
        assert!(s.is_blocked("sex.com"));
        assert!(s.is_blocked("www.sex.example"));
        assert!(s.is_blocked("anal.example.com"));
    }

    #[test]
    fn matches_adult_tlds_via_labels() {
        let s = set();
        assert!(s.is_blocked("example.xxx"));
        assert!(!s.is_blocked("example.com"));
    }

    #[test]
    fn user_keywords_pin_short_words_to_tokens() {
        assert_eq!(Keyword::user("sex").mode, MatchMode::Token);
        assert_eq!(Keyword::user("anal").mode, MatchMode::Token);
        assert_eq!(Keyword::user("hentai").mode, MatchMode::Substring);
        assert_eq!(Keyword::user("  PORNO ").word, "porno");

        let mut s = KeywordSet::new();
        s.extend([Keyword::user("sex"), Keyword::user("fetish")]);
        assert!(!s.is_blocked("essex.ac.uk")); // short -> token only
        assert!(s.is_blocked("myfetishsite.com")); // long -> substring
    }

    #[test]
    fn parsing_and_dedup() {
        let s = KeywordSet::parse("~porn\nporn\n# comment\n\n  sex  \n");
        assert_eq!(s.len(), 2); // duplicate word ignored regardless of mode
        assert!(s.is_blocked("freeporn.com"));
        let empty = KeywordSet::new();
        assert!(!empty.is_blocked("anything.com"));
        assert!(empty.is_empty());
    }

    #[test]
    fn reports_which_keyword_matched() {
        let s = set();
        assert_eq!(s.match_of("freeporn.com").unwrap().word, "porn");
        assert_eq!(s.match_of("sex.com").unwrap().word, "sex");
        assert!(s.match_of("example.org").is_none());
    }
}
