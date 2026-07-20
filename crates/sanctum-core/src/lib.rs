//! sanctum-core — shared logic for the Sanctum content blocker.
//!
//! This crate is deliberately platform-light: it holds the pure,
//! testable pieces (domain matching, blocklist parsing, hosts-section
//! management, SafeSearch map, storage, password hashing) that both the
//! Windows service and the Tauri UI rely on. All privileged Windows
//! operations (running the DNS resolver, editing the real hosts file,
//! setting adapter DNS, ACLs, the service itself) live in the
//! `sanctum-service` crate.
//!
//! Design principle: **honesty and fail-safety over cleverness.** Nothing
//! here hides itself or makes the machine unusable. See docs/ADR-001.

pub mod approval;
pub mod blocklist;
pub mod config;
pub mod domain;
pub mod error;
pub mod hosts;
pub mod ipc;
pub mod keyword;
pub mod password;
pub mod paths;
pub mod safesearch;
pub mod storage;

pub use blocklist::Blocklist;
pub use config::{AppConfig, LockState, Schedule, TimeWindow};
pub use error::{Error, Result};
pub use hosts::HostsSection;
pub use safesearch::SafeSearchMap;
pub use storage::{Db, Event};

/// Default IPv4 sinkhole address for blocked domains.
pub const SINK_IPV4: &str = "0.0.0.0";
/// Default IPv6 sinkhole address for blocked domains.
pub const SINK_IPV6: &str = "::";

/// Markers that delimit Sanctum's owned region of the hosts file.
/// These are load-bearing: the integrity check verifies they are balanced.
pub const HOSTS_START: &str = "# >>> SANCTUM START";
pub const HOSTS_END: &str = "# <<< SANCTUM END";
