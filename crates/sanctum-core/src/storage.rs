//! Local SQLite storage. Everything is on-device; nothing leaves the machine.
//!
//! The service is the only privileged writer. Lock-invariant enforcement
//! (blocklist may only grow, timer may only extend) is applied by the
//! caller via `config::guard_*`; this layer provides the raw operations
//! and the private streak/activity data.

use crate::config::{AppConfig, LockState};
use crate::error::Result;
use crate::password;
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

/// One entry in the activity log.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Event {
    pub id: i64,
    pub ts: DateTime<Utc>,
    pub kind: String,
    pub detail: String,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (creating if needed) and migrate the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open a private in-memory database (used by tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS kv (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS custom_block (
                domain   TEXT PRIMARY KEY,
                added_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS allowlist (
                domain   TEXT PRIMARY KEY,
                added_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id     INTEGER PRIMARY KEY AUTOINCREMENT,
                ts     INTEGER NOT NULL,
                kind   TEXT NOT NULL,
                detail TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
            CREATE TABLE IF NOT EXISTS protected_days (
                day TEXT PRIMARY KEY
            );
            "#,
        )?;
        self.set_kv("schema_version", &SCHEMA_VERSION.to_string())?;
        Ok(())
    }

    // ---- key/value ----

    pub fn set_kv(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO kv(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_kv(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
    }

    // ---- config ----

    pub fn load_config(&self) -> Result<AppConfig> {
        match self.get_kv("config")? {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => {
                let cfg = AppConfig::default();
                self.save_config(&cfg)?;
                Ok(cfg)
            }
        }
    }

    pub fn save_config(&self, cfg: &AppConfig) -> Result<()> {
        self.set_kv("config", &serde_json::to_string(cfg)?)
    }

    // ---- lock ----

    pub fn load_lock(&self) -> Result<LockState> {
        match self.get_kv("lock")? {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => Ok(LockState::unlocked()),
        }
    }

    pub fn save_lock(&self, lock: &LockState) -> Result<()> {
        self.set_kv("lock", &serde_json::to_string(lock)?)
    }

    // ---- password ----

    pub fn set_password(&self, plaintext: &str) -> Result<()> {
        let phc = password::hash_password(plaintext)?;
        self.set_kv("password_hash", &phc)
    }

    pub fn has_password(&self) -> Result<bool> {
        Ok(self.get_kv("password_hash")?.is_some())
    }

    pub fn verify_password(&self, plaintext: &str) -> Result<bool> {
        match self.get_kv("password_hash")? {
            Some(phc) => password::verify_password(plaintext, &phc),
            None => Ok(false),
        }
    }

    // ---- custom blocklist / allowlist ----

    pub fn add_custom_block(&self, domain: &str, now: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO custom_block(domain, added_at) VALUES(?1, ?2)",
            params![domain, now.timestamp()],
        )?;
        Ok(())
    }

    pub fn remove_custom_block(&self, domain: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM custom_block WHERE domain = ?1", params![domain])?;
        Ok(())
    }

    pub fn list_custom_block(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT domain FROM custom_block ORDER BY domain")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn add_allow(&self, domain: &str, now: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO allowlist(domain, added_at) VALUES(?1, ?2)",
            params![domain, now.timestamp()],
        )?;
        Ok(())
    }

    pub fn remove_allow(&self, domain: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM allowlist WHERE domain = ?1", params![domain])?;
        Ok(())
    }

    /// Allowlist entries as `(domain, added_at)`. The service ignores any
    /// entry added *after* a lock started (can't whitelist your way out).
    pub fn list_allow(&self) -> Result<Vec<(String, DateTime<Utc>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT domain, added_at FROM allowlist ORDER BY domain")?;
        let rows = stmt
            .query_map([], |r| {
                let d: String = r.get(0)?;
                let ts: i64 = r.get(1)?;
                Ok((d, ts))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(d, ts)| (d, Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now)))
            .collect())
    }

    // ---- activity log + block counter ----

    /// Record a blocked lookup: bumps the lifetime counter and logs an event.
    pub fn record_block(&self, domain: &str, layer: &str, now: DateTime<Utc>) -> Result<()> {
        let total = self.total_blocks()? + 1;
        self.set_kv("total_blocks", &total.to_string())?;
        self.record_event("block", &format!("{domain} [{layer}]"), now)
    }

    pub fn total_blocks(&self) -> Result<u64> {
        Ok(self
            .get_kv("total_blocks")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }

    pub fn record_event(&self, kind: &str, detail: &str, now: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events(ts, kind, detail) VALUES(?1, ?2, ?3)",
            params![now.timestamp(), kind, detail],
        )?;
        Ok(())
    }

    pub fn recent_events(&self, limit: u32) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, kind, detail FROM events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                let id: i64 = r.get(0)?;
                let ts: i64 = r.get(1)?;
                let kind: String = r.get(2)?;
                let detail: String = r.get(3)?;
                Ok((id, ts, kind, detail))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(id, ts, kind, detail)| Event {
                id,
                ts: Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now),
                kind,
                detail,
            })
            .collect())
    }

    /// Wipe the activity log. Returns the number of rows deleted. Lifetime
    /// aggregate counters (total blocks, protected days) are preserved, as
    /// they carry no record of *what* was visited.
    pub fn delete_all_history(&self) -> Result<usize> {
        let n = self.conn.execute("DELETE FROM events", [])?;
        // Reclaim space so the wipe is real on disk.
        self.conn.execute_batch("VACUUM;")?;
        Ok(n)
    }

    pub fn event_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    // ---- protected-days streak ----

    pub fn mark_protected_today(&self) -> Result<()> {
        let today = Local::now().date_naive().to_string();
        self.mark_protected_day(&today)
    }

    pub fn mark_protected_day(&self, day: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO protected_days(day) VALUES(?1)",
            params![day],
        )?;
        Ok(())
    }

    pub fn total_protected_days(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM protected_days", [], |r| {
                r.get::<_, i64>(0)
            })? as u64)
    }

    /// Current consecutive-day streak ending today (or yesterday if today
    /// isn't yet marked).
    pub fn current_streak(&self) -> Result<u64> {
        let mut stmt = self
            .conn
            .prepare("SELECT day FROM protected_days ORDER BY day DESC")?;
        let days: Vec<NaiveDate> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|s| s.parse::<NaiveDate>().ok())
            .collect();
        if days.is_empty() {
            return Ok(0);
        }
        let today = Local::now().date_naive();
        let yesterday = today.pred_opt().unwrap_or(today);
        let mut expected = if days[0] == today {
            today
        } else if days[0] == yesterday {
            yesterday
        } else {
            return Ok(0);
        };
        let mut streak = 0u64;
        for d in days {
            if d == expected {
                streak += 1;
                expected = expected.pred_opt().unwrap_or(expected);
            } else if d < expected {
                break;
            }
        }
        Ok(streak)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_lock_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let mut cfg = db.load_config().unwrap();
        assert!(cfg.protection_enabled);
        cfg.block_doh = false;
        db.save_config(&cfg).unwrap();
        assert!(!db.load_config().unwrap().block_doh);

        assert!(!db.load_lock().unwrap().locked);
    }

    #[test]
    fn password_gate() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.has_password().unwrap());
        db.set_password("hunter2").unwrap();
        assert!(db.has_password().unwrap());
        assert!(db.verify_password("hunter2").unwrap());
        assert!(!db.verify_password("nope").unwrap());
    }

    #[test]
    fn blocks_counter_and_history_wipe() {
        let db = Db::open_in_memory().unwrap();
        let now = Utc::now();
        db.record_block("bad.com", "dns", now).unwrap();
        db.record_block("bad2.com", "hosts", now).unwrap();
        assert_eq!(db.total_blocks().unwrap(), 2);
        assert_eq!(db.event_count().unwrap(), 2);

        let deleted = db.delete_all_history().unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(db.event_count().unwrap(), 0);
        // Lifetime counter is intentionally preserved.
        assert_eq!(db.total_blocks().unwrap(), 2);
    }

    #[test]
    fn streak_counts_consecutive_days() {
        let db = Db::open_in_memory().unwrap();
        let today = Local::now().date_naive();
        db.mark_protected_day(&today.to_string()).unwrap();
        db.mark_protected_day(&today.pred_opt().unwrap().to_string())
            .unwrap();
        db.mark_protected_day(
            &today
                .pred_opt()
                .unwrap()
                .pred_opt()
                .unwrap()
                .to_string(),
        )
        .unwrap();
        assert_eq!(db.current_streak().unwrap(), 3);
        assert_eq!(db.total_protected_days().unwrap(), 3);
    }
}
