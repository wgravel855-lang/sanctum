//! Browser DoH lockdown via enterprise policy keys (owner-approved 2026-07-18).
//!
//! While the service runs, every adapter's DNS points at Sanctum's resolver —
//! but Chromium and Firefox can tunnel lookups around it with built-in
//! DNS-over-HTTPS ("secure DNS"). The clean, supported way to keep browser
//! lookups on system DNS is the browsers' own enterprise policies under
//! `HKLM\SOFTWARE\Policies`. Honest and visible: the browsers display
//! "managed by your organization" and the keys are plainly inspectable.
//!
//! Anything we change is journaled first (exact prior values, and whether we
//! created the key), so removal restores precisely the pre-Sanctum state and
//! never clobbers a real organization's policy. Removal happens on authorized
//! teardown and in `sanctum-recover`.

use serde::{Deserialize, Serialize};
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
use winreg::{RegKey, RegValue};

/// One registry value we enforce.
#[derive(Clone, Debug)]
enum Desired {
    Sz(&'static str),
    Dword(u32),
}

/// A browser policy target: a key path (relative to the hive root) plus the
/// values that disable DoH for that browser.
struct Target {
    key: &'static str,
    values: &'static [(&'static str, Desired)],
}

const CHROMIUM_OFF: &[(&str, Desired)] = &[("DnsOverHttpsMode", Desired::Sz("off"))];
const FIREFOX_OFF: &[(&str, Desired)] = &[
    ("Enabled", Desired::Dword(0)),
    ("Locked", Desired::Dword(1)),
];

fn targets() -> Vec<Target> {
    vec![
        Target {
            key: r"SOFTWARE\Policies\Google\Chrome",
            values: CHROMIUM_OFF,
        },
        Target {
            key: r"SOFTWARE\Policies\Microsoft\Edge",
            values: CHROMIUM_OFF,
        },
        Target {
            key: r"SOFTWARE\Policies\BraveSoftware\Brave",
            values: CHROMIUM_OFF,
        },
        // Vivaldi honors the Chromium enterprise policy schema under its own key.
        Target {
            key: r"SOFTWARE\Policies\Vivaldi",
            values: CHROMIUM_OFF,
        },
        Target {
            key: r"SOFTWARE\Policies\Mozilla\Firefox\DNSOverHTTPS",
            values: FIREFOX_OFF,
        },
    ]
}

/// Journal of what a single value looked like before we touched it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PriorValue {
    pub key: String,
    pub name: String,
    /// Raw bytes + type of the pre-existing value, or None if it didn't exist.
    pub prior: Option<(u32, Vec<u8>)>,
}

/// Journal for a full apply: every value we set, plus every key we created
/// (deepest last, so removal can delete them in reverse when left empty).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Journal {
    pub values: Vec<PriorValue>,
    pub created_keys: Vec<String>,
}

impl Journal {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

fn desired_matches(existing: &RegValue, want: &Desired) -> bool {
    match want {
        Desired::Sz(s) => {
            existing.vtype == winreg::enums::RegType::REG_SZ
                && String::from_utf16_lossy(
                    &existing
                        .bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect::<Vec<_>>(),
                )
                .trim_end_matches('\0')
                    == *s
        }
        Desired::Dword(d) => {
            existing.vtype == winreg::enums::RegType::REG_DWORD
                && existing.bytes.len() == 4
                && u32::from_le_bytes([
                    existing.bytes[0],
                    existing.bytes[1],
                    existing.bytes[2],
                    existing.bytes[3],
                ]) == *d
        }
    }
}

/// Apply the DoH-off policies under `root` (HKLM in production, a scratch
/// HKCU key in tests). Idempotent: values already exactly ours are not
/// re-journaled, so a re-apply never overwrites the original journal entry.
/// Returns the journal of prior state for everything newly changed.
pub fn apply_under(root: &RegKey, prefix: &str, mut journal: Journal) -> anyhow::Result<Journal> {
    for t in targets() {
        let path = join(prefix, t.key);

        // Track which ancestor keys we create so removal can prune them.
        let mut created_here: Vec<String> = Vec::new();
        let mut probe = String::new();
        for part in path.split('\\') {
            if !probe.is_empty() {
                probe.push('\\');
            }
            probe.push_str(part);
            if root.open_subkey(&probe).is_err() && !journal.created_keys.contains(&probe) {
                created_here.push(probe.clone());
            }
        }

        let (key, _) = root.create_subkey(&path)?;
        journal.created_keys.extend(created_here);

        for (name, want) in t.values {
            let existing = key.get_raw_value(name).ok();
            let already_ours = existing.as_ref().is_some_and(|v| desired_matches(v, want));
            let already_journaled = journal
                .values
                .iter()
                .any(|p| p.key == path && p.name == *name);
            if !already_ours && !already_journaled {
                journal.values.push(PriorValue {
                    key: path.clone(),
                    name: (*name).to_string(),
                    prior: existing.map(|v| (v.vtype as u32, v.bytes.into_owned())),
                });
            }
            match want {
                Desired::Sz(s) => key.set_value(name, s)?,
                Desired::Dword(d) => key.set_value(name, d)?,
            }
        }
    }
    Ok(journal)
}

/// Restore the exact prior state recorded in `journal`: put back overwritten
/// values, delete values we created, and prune keys we created (deepest
/// first) if they are now empty.
pub fn remove_under(root: &RegKey, journal: &Journal) {
    for pv in &journal.values {
        let Ok(key) = root.open_subkey_with_flags(&pv.key, KEY_READ | KEY_WRITE) else {
            continue;
        };
        match &pv.prior {
            Some((vtype, bytes)) => {
                let raw = RegValue {
                    vtype: reg_type_from(*vtype),
                    bytes: std::borrow::Cow::Owned(bytes.clone()),
                };
                let _ = key.set_raw_value(&pv.name, &raw);
            }
            None => {
                let _ = key.delete_value(&pv.name);
            }
        }
    }
    // Deepest-first so children go before parents.
    let mut keys = journal.created_keys.clone();
    keys.sort_by_key(|k| std::cmp::Reverse(k.matches('\\').count()));
    for k in keys {
        if let Ok(sub) = root.open_subkey(&k) {
            let empty = sub.enum_keys().next().is_none() && sub.enum_values().next().is_none();
            drop(sub);
            if empty {
                let _ = root.delete_subkey(&k);
            }
        }
    }
}

fn reg_type_from(v: u32) -> winreg::enums::RegType {
    use winreg::enums::RegType::*;
    match v {
        1 => REG_SZ,
        2 => REG_EXPAND_SZ,
        3 => REG_BINARY,
        4 => REG_DWORD,
        7 => REG_MULTI_SZ,
        11 => REG_QWORD,
        _ => REG_BINARY,
    }
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}\\{key}")
    }
}

const KV_JOURNAL: &str = "browser_policy_restore";

/// Production apply: journal to the DB first, then set policies under HKLM.
/// Reuses (and extends) any existing journal so repeated applies across
/// service restarts keep the original pre-Sanctum state.
pub fn apply(db: &sanctum_core::Db) {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let prior = db
        .get_kv(KV_JOURNAL)
        .ok()
        .flatten()
        .and_then(|s| Journal::from_json(&s))
        .unwrap_or_default();
    match apply_under(&hklm, "", prior) {
        Ok(journal) => {
            if let Err(e) = db.set_kv(KV_JOURNAL, &journal.to_json()) {
                tracing::warn!(error = %e, "could not persist browser-policy journal");
            }
            tracing::info!("browser DoH policies applied (Chrome/Edge/Brave/Firefox)");
        }
        Err(e) => tracing::warn!(error = %e, "could not apply browser DoH policies"),
    }
}

/// Production removal: restore prior state from the DB journal and clear it.
pub fn remove(db: &sanctum_core::Db) {
    let Some(journal) = db
        .get_kv(KV_JOURNAL)
        .ok()
        .flatten()
        .and_then(|s| Journal::from_json(&s))
    else {
        return;
    };
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    remove_under(&hklm, &journal);
    let _ = db.set_kv(KV_JOURNAL, "");
    tracing::info!("browser DoH policies removed (prior state restored)");
}

/// Last-resort removal for `sanctum-recover` when no journal is available:
/// delete a policy value only if it is EXACTLY the value Sanctum sets (so a
/// real organization's differing policy is never touched), then prune any
/// now-empty policy keys along our known paths.
pub fn force_remove_ours() {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for t in targets() {
        if let Ok(key) = hklm.open_subkey_with_flags(t.key, KEY_READ | KEY_WRITE) {
            for (name, want) in t.values {
                if key
                    .get_raw_value(name)
                    .ok()
                    .is_some_and(|v| desired_matches(&v, want))
                {
                    let _ = key.delete_value(name);
                }
            }
        }
        // Prune empty keys from the deepest path upward, stopping at
        // SOFTWARE\Policies itself.
        let mut path = t.key.to_string();
        while path.to_ascii_lowercase() != r"software\policies" {
            let Ok(sub) = hklm.open_subkey(&path) else { break };
            let empty = sub.enum_keys().next().is_none() && sub.enum_values().next().is_none();
            drop(sub);
            if !empty {
                break;
            }
            let _ = hklm.delete_subkey(&path);
            match path.rfind('\\') {
                Some(idx) => path.truncate(idx),
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;

    fn scratch(name: &str) -> (RegKey, String) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(r"Software\SanctumTest\{name}");
        // Clean slate.
        let _ = hkcu.delete_subkey_all(&path);
        (hkcu, path)
    }

    #[test]
    fn apply_sets_all_policies_and_remove_restores_cleanly() {
        let (root, prefix) = scratch("fresh");
        let journal = apply_under(&root, &prefix, Journal::default()).unwrap();

        // All four browser keys set.
        let chrome = root
            .open_subkey(format!(r"{prefix}\SOFTWARE\Policies\Google\Chrome"))
            .unwrap();
        let mode: String = chrome.get_value("DnsOverHttpsMode").unwrap();
        assert_eq!(mode, "off");
        let ff = root
            .open_subkey(format!(
                r"{prefix}\SOFTWARE\Policies\Mozilla\Firefox\DNSOverHTTPS"
            ))
            .unwrap();
        let enabled: u32 = ff.get_value("Enabled").unwrap();
        let locked: u32 = ff.get_value("Locked").unwrap();
        assert_eq!((enabled, locked), (0, 1));

        // We created everything, so removal must leave no trace.
        remove_under(&root, &journal);
        assert!(root
            .open_subkey(format!(r"{prefix}\SOFTWARE\Policies\Google\Chrome"))
            .is_err());
        assert!(root
            .open_subkey(format!(r"{prefix}\SOFTWARE\Policies\Mozilla"))
            .is_err());

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(r"Software\SanctumTest\fresh");
    }

    #[test]
    fn preexisting_policy_is_journaled_and_restored_not_deleted() {
        let (root, prefix) = scratch("preexisting");
        // Simulate a real org policy that already sets DoH to automatic.
        let (chrome, _) = root
            .create_subkey(format!(r"{prefix}\SOFTWARE\Policies\Google\Chrome"))
            .unwrap();
        chrome.set_value("DnsOverHttpsMode", &"automatic").unwrap();

        let journal = apply_under(&root, &prefix, Journal::default()).unwrap();
        let mode: String = chrome.get_value("DnsOverHttpsMode").unwrap();
        assert_eq!(mode, "off");

        remove_under(&root, &journal);
        // The org's value is back and the key survives.
        let mode: String = chrome.get_value("DnsOverHttpsMode").unwrap();
        assert_eq!(mode, "automatic");

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(r"Software\SanctumTest\preexisting");
    }

    #[test]
    fn reapply_is_idempotent_and_preserves_original_journal() {
        let (root, prefix) = scratch("idempotent");
        let j1 = apply_under(&root, &prefix, Journal::default()).unwrap();
        // Re-apply with the persisted journal (as a service restart would).
        let j2 = apply_under(&root, &prefix, j1.clone()).unwrap();
        assert_eq!(j1, j2, "re-apply must not grow or rewrite the journal");

        remove_under(&root, &j2);
        assert!(root
            .open_subkey(format!(r"{prefix}\SOFTWARE\Policies"))
            .is_err());

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(r"Software\SanctumTest\idempotent");
    }
}
