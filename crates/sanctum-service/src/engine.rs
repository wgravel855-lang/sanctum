//! The enforcement engine: composes the resolver, the HOSTS floor, and
//! adapter DNS into the desired filtering state, and applies/reconciles/
//! tears it down. All privileged effects funnel through here so the service
//! host and the (future) reconcile loop share one code path.
//!
//! Startup ordering is load-bearing (ADR-001 §3.2): capture prior DNS →
//! build + bind the resolver → if bind FAILS, stay HOSTS-only and NEVER
//! repoint adapters (the anti-brick gate) → apply floor → repoint adapters.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sanctum_core::{paths, Blocklist, Db};

use crate::dns::{FilterState, Resolver};
use crate::{hostsfile, lists, netcfg};

/// Public DNS fallbacks used when a machine's prior DNS was DHCP-only and we
/// can't recover a specific upstream (v0.1 simplification; documented).
const FALLBACK_UPSTREAMS: [&str; 2] = ["1.1.1.1:53", "9.9.9.9:53"];

pub struct EnforcementEngine {
    db_path: PathBuf,
    hosts_path: PathBuf,
}

impl Default for EnforcementEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl EnforcementEngine {
    pub fn new() -> Self {
        Self {
            db_path: paths::db_path(),
            hosts_path: hostsfile::hosts_path(),
        }
    }

    fn db(&self) -> anyhow::Result<Db> {
        Ok(Db::open(&self.db_path)?)
    }

    /// Capture every manageable adapter's prior DNS and persist the restore
    /// journal BEFORE anything is changed. Returns the journal.
    pub fn capture_and_journal(&self) -> anyhow::Result<Vec<netcfg::AdapterRestore>> {
        let adapters = netcfg::enumerate()?;
        let journal: Vec<_> = adapters
            .iter()
            .filter(|a| a.is_manageable())
            .map(netcfg::capture_restore)
            .collect();
        self.db()?
            .set_kv("dns_restore", &serde_json::to_string(&journal)?)?;
        Ok(journal)
    }

    /// Upstreams to forward to: the captured prior servers, then public
    /// fallbacks, de-duplicated and never loopback.
    pub fn compute_upstreams(&self, journal: &[netcfg::AdapterRestore]) -> Vec<SocketAddr> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut push = |ip_str: &str| {
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                if ip.is_loopback() {
                    return;
                }
                let sa = SocketAddr::new(ip, 53);
                if seen.insert(sa) {
                    out.push(sa);
                }
            }
        };
        for r in journal {
            for s in r.v4.iter().chain(r.v6.iter()) {
                push(s);
            }
        }
        for f in FALLBACK_UPSTREAMS {
            push(f.trim_end_matches(":53"));
        }
        out
    }

    /// Build the effective resolver from the database.
    pub fn build_resolver(
        &self,
        upstreams: Vec<SocketAddr>,
    ) -> anyhow::Result<(Arc<Resolver>, Blocklist)> {
        let db = self.db()?;
        let cfg = db.load_config()?;
        let (state, block) = filter_state_from_db(&db)?;
        let sink_v4 = cfg.sink_ipv4.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
        let sink_v6 = cfg.sink_ipv6.parse().unwrap_or(Ipv6Addr::UNSPECIFIED);
        Ok((
            Arc::new(Resolver::from_state(state, upstreams, sink_v4, sink_v6)),
            block,
        ))
    }

    /// Rebuild filter state from the database and hot-reload the running
    /// resolver (called after an IPC change and on each reconcile tick).
    /// Returns the fresh blocklist for the HOSTS-floor refresh.
    pub fn reload(&self, resolver: &Arc<Resolver>) -> anyhow::Result<Blocklist> {
        reload_resolver(&self.db()?, resolver)
    }

    /// Write the always-on HOSTS floor.
    pub fn apply_floor(&self, block: &Blocklist) -> anyhow::Result<()> {
        let floor = lists::floor_domains(block, lists::FLOOR_SIZE);
        hostsfile::apply(&self.hosts_path, &floor, "0.0.0.0", "::")
    }

    /// Re-assert loopback DNS on all manageable adapters (bring-up + each
    /// reconcile tick). Does NOT capture — capture happened once up front.
    pub fn reassert_loopback(&self) -> anyhow::Result<()> {
        for a in netcfg::enumerate()?.iter().filter(|a| a.is_manageable()) {
            if let Err(e) = netcfg::set_loopback(&a.name) {
                tracing::warn!(adapter = %a.name, error = %e, "failed to set loopback DNS");
            }
        }
        netcfg::flush_dns_cache()?;
        Ok(())
    }

    /// Restore adapters from the journal (authorized, unlocked stop only).
    pub fn restore_adapters(&self) -> anyhow::Result<()> {
        if let Some(json) = self.db()?.get_kv("dns_restore")? {
            let journal: Vec<netcfg::AdapterRestore> = serde_json::from_str(&json)?;
            for r in &journal {
                // Restoring DNS is the anti-brick path, so retry a transient
                // netsh failure a few times rather than leaving an adapter
                // stranded on the (now-dead) 127.0.0.1 resolver.
                let mut attempt = 0;
                loop {
                    match netcfg::restore(r) {
                        Ok(()) => break,
                        Err(e) => {
                            attempt += 1;
                            if attempt >= 3 {
                                tracing::warn!(adapter = %r.name, error = %e, "failed to restore DNS after retries");
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                    }
                }
            }
            netcfg::flush_dns_cache()?;
        }
        Ok(())
    }

    pub fn remove_hosts_floor(&self) -> anyhow::Result<()> {
        hostsfile::remove(&self.hosts_path)
    }

    /// Bind the resolver on both loopback stacks. Returns `true` if the
    /// critical IPv4 UDP listener bound. The v4 bind is retried briefly — a
    /// service restart can race the previous process releasing the port.
    pub async fn bind(&self, resolver: &Arc<Resolver>) -> bool {
        let v4: SocketAddr = (Ipv4Addr::LOCALHOST, 53).into();
        let v6: SocketAddr = (Ipv6Addr::LOCALHOST, 53).into();

        let mut bound = false;
        for attempt in 0..6 {
            match resolver.clone().spawn_udp(v4).await {
                Ok(_) => {
                    bound = true;
                    break;
                }
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "127.0.0.1:53 bind failed; retrying");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        if !bound {
            tracing::error!("could not bind 127.0.0.1:53 after retries — staying HOSTS-only");
        }
        let _ = resolver.clone().spawn_tcp(v4).await;
        if resolver.clone().spawn_udp(v6).await.is_err() {
            tracing::warn!("IPv6 loopback resolver unavailable ([::1]:53)");
        }
        let _ = resolver.clone().spawn_tcp(v6).await;
        bound
    }

    /// End-to-end proof that the resolver is actually answering on
    /// `127.0.0.1:53` — query the health canary and require the fixed answer.
    /// This is the real gate for repointing adapters: a bound socket that
    /// isn't serving must never be trusted (that is what bricked DNS).
    pub async fn self_verify(&self) -> bool {
        canary_ok((Ipv4Addr::LOCALHOST, 53).into()).await
    }
}

/// Query a loopback resolver for the health canary; true iff it returns the
/// fixed `127.0.0.2` answer within the timeout.
async fn canary_ok(addr: SocketAddr) -> bool {
    use tokio::net::UdpSocket;
    let Ok(sock) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await else {
        return false;
    };
    if sock.send_to(&build_canary_query(), addr).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_millis(1500), sock.recv(&mut buf)).await {
        Ok(Ok(n)) => buf[..n].windows(4).any(|w| w == [127, 0, 0, 2]),
        _ => false,
    }
}

/// A minimal DNS A query for `health.sanctum.invalid`.
fn build_canary_query() -> Vec<u8> {
    let mut q = vec![
        0x57, 0x41, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    for label in ["health", "sanctum", "invalid"] {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
    q
}

/// Build the effective filter state from a database handle: embedded starter
/// list ∪ DB custom list, with the allowlist, SafeSearch, and DoH settings
/// from config. Free function so the IPC handler can reuse it with any DB.
pub fn filter_state_from_db(db: &Db) -> anyhow::Result<(FilterState, Blocklist)> {
    let cfg = db.load_config()?;
    let mut block = lists::starter_blocklist();
    for d in db.list_custom_block()? {
        block.add(&d);
    }
    let allowlist: HashSet<String> = db.list_allow()?.into_iter().map(|(d, _)| d).collect();

    let mut state = FilterState::new(block.clone(), lists::safesearch_map(), lists::doh_list());
    state.allowlist = allowlist;
    state.enforce_safesearch = cfg.enforce_safesearch;
    state.block_doh = cfg.block_doh;
    Ok((state, block))
}

/// Hot-reload a running resolver from a database handle.
pub fn reload_resolver(db: &Db, resolver: &Arc<Resolver>) -> anyhow::Result<Blocklist> {
    let (state, block) = filter_state_from_db(db)?;
    resolver.update(state);
    Ok(block)
}
