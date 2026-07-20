//! Library surface for the Sanctum service, so the enforcement modules are
//! integration-testable independently of the `windows-service` host binary.

pub mod browser_policy;
pub mod dns;
pub mod engine;
pub mod firewall;
pub mod heartbeat;
pub mod hostsfile;
pub mod intervention;
pub mod ipc;
pub mod lists;
pub mod netcfg;
pub mod notifier;
pub mod service;
