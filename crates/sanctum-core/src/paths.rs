//! Well-known filesystem locations. All privileged state lives under
//! `%ProgramData%\Sanctum`, which the service ACL-locks so a standard
//! user cannot hand-edit it during a locked session.

use std::path::PathBuf;

/// `%ProgramData%` (e.g. `C:\ProgramData`), falling back sensibly.
fn program_data() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

/// `%SystemRoot%` (e.g. `C:\Windows`).
fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

/// `%ProgramData%\Sanctum` — the ACL-locked data root.
pub fn data_dir() -> PathBuf {
    program_data().join("Sanctum")
}

/// The SQLite database file.
pub fn db_path() -> PathBuf {
    data_dir().join("sanctum.db")
}

/// Directory holding the effective blocklists the service loads at runtime.
pub fn blocklist_dir() -> PathBuf {
    data_dir().join("blocklist")
}

/// The named-pipe address the service listens on for the UI.
pub const PIPE_NAME: &str = r"\\.\pipe\sanctum-service";

/// The Windows hosts file.
pub fn hosts_path() -> PathBuf {
    system_root().join(r"System32\drivers\etc\hosts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_under_program_data() {
        assert!(db_path().starts_with(data_dir()));
        assert!(hosts_path().ends_with(r"drivers\etc\hosts"));
    }
}
