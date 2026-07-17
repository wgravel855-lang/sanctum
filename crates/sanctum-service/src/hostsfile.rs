//! Atomic, marker-scoped writes to the Windows hosts file (ADR-001 §9).
//!
//! The string-level section logic lives in `sanctum_core::hosts` (byte-exact,
//! never touches outside the markers). This module adds the Windows file I/O:
//! resolve the path from the registry `DataBasePath`, write a same-directory
//! temp file, and swap it in atomically with `ReplaceFileW` (preserves ACLs
//! and attributes). All functions take an explicit path so they are testable
//! against a temp file rather than the live system hosts.

use std::path::{Path, PathBuf};

use sanctum_core::hosts;

/// Resolve the hosts directory from `HKLM\...\Tcpip\Parameters\DataBasePath`
/// (a `REG_EXPAND_SZ` like `%SystemRoot%\System32\drivers\etc`), expanding
/// environment references. Falls back to the conventional path.
pub fn hosts_path() -> PathBuf {
    hosts_dir_from_registry()
        .map(|d| d.join("hosts"))
        .unwrap_or_else(sanctum_core::paths::hosts_path)
}

fn hosts_dir_from_registry() -> Option<PathBuf> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters")
        .ok()?;
    let raw: String = key.get_value("DataBasePath").ok()?;
    Some(PathBuf::from(expand_env(&raw)))
}

/// Expand `%VAR%` references using the process environment.
pub fn expand_env(input: &str) -> String {
    let mut out = input.to_string();
    let mut search_from = 0;
    while let Some(rel) = out[search_from..].find('%') {
        let start = search_from + rel;
        let Some(end_rel) = out[start + 1..].find('%') else {
            break;
        };
        let end = start + 1 + end_rel;
        let var = &out[start + 1..end];
        let val = std::env::var(var).unwrap_or_default();
        out.replace_range(start..=end, &val);
        search_from = start + val.len();
    }
    out
}

/// Insert/replace the Sanctum section, sinkholing `domains`.
pub fn apply(path: &Path, domains: &[String], sink_v4: &str, sink_v6: &str) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let updated = hosts::upsert_section(&content, domains, sink_v4, sink_v6)?;
    atomic_write(path, &updated)
}

/// Remove the Sanctum section entirely (uninstall / authorized disable).
pub fn remove(path: &Path) -> anyhow::Result<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // nothing to remove
    };
    let updated = hosts::remove_section(&content)?;
    atomic_write(path, &updated)
}

fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("hosts path has no parent directory"))?;
    let tmp = dir.join("hosts.sanctum.tmp");

    clear_readonly(path);
    // ASCII/UTF-8, no BOM, newlines already normalized by the core writer.
    std::fs::write(&tmp, content.as_bytes())?;

    if path.exists() {
        if let Err(e) = replace_file(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    } else {
        std::fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// Atomically replace `dst` with `src` via `ReplaceFileW` (preserves the
/// destination's ACLs/attributes — important for the system hosts file).
fn replace_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS};

    let dst_w = to_wide(dst);
    let src_w = to_wide(src);
    unsafe {
        ReplaceFileW(
            PCWSTR(dst_w.as_ptr()),
            PCWSTR(src_w.as_ptr()),
            PCWSTR::null(),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )?;
    }
    Ok(())
}

fn to_wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn clear_readonly(path: &Path) {
    if let Ok(md) = std::fs::metadata(path) {
        let mut perm = md.permissions();
        if perm.readonly() {
            perm.set_readonly(false);
            let _ = std::fs::set_permissions(path, perm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_environment_refs() {
        std::env::set_var("SANCTUM_TEST_ROOT", r"C:\Win");
        assert_eq!(
            expand_env(r"%SANCTUM_TEST_ROOT%\System32\drivers\etc"),
            r"C:\Win\System32\drivers\etc"
        );
        // Unmatched percent is left as-is.
        assert_eq!(expand_env("100% done"), "100% done");
    }

    #[test]
    fn atomic_apply_then_remove_preserves_other_lines() {
        let dir = tempfile::tempdir().unwrap();
        let hosts = dir.path().join("hosts");
        std::fs::write(&hosts, "127.0.0.1 localhost\r\n::1 localhost\r\n").unwrap();

        apply(&hosts, &["bad.com".into()], "0.0.0.0", "::").unwrap();
        let c = std::fs::read_to_string(&hosts).unwrap();
        assert!(c.contains("127.0.0.1 localhost"));
        assert!(c.contains("0.0.0.0 bad.com"));
        assert!(c.contains(":: bad.com"));
        assert!(c.contains("# >>> SANCTUM START"));
        assert!(!dir.path().join("hosts.sanctum.tmp").exists()); // temp cleaned up

        remove(&hosts).unwrap();
        let c = std::fs::read_to_string(&hosts).unwrap();
        assert!(c.contains("127.0.0.1 localhost"));
        assert!(c.contains("::1 localhost"));
        assert!(!c.contains("SANCTUM"));
    }
}
