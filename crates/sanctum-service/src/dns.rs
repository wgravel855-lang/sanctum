//! The DNS sinkhole resolver (ADR-001 §3).
//!
//! Raw tokio UDP+TCP loops over `hickory-proto` messages — no
//! `RequestHandler` trait surface, full control over the per-query
//! pipeline. The resolver is transport-agnostic and binds arbitrary
//! addresses, so its logic is unit/integration-testable without admin
//! rights or touching port 53.
//!
//! The mutable filter lists live behind an `RwLock` so the IPC layer and
//! the reconcile loop can hot-reload them (a blocklist add must take effect
//! immediately, even mid locked-session) without rebinding sockets.
//!
//! Per-query priority (normalized lowercase FQDN):
//!   1. health canary        -> fixed answer (watchdog liveness)
//!   2. DoH-disable canary    -> NXDOMAIN (`use-application-dns.net`)
//!   3. allowlist             -> forward
//!   4. blocklist             -> sink 0.0.0.0 / ::
//!   5. DoH endpoint sink     -> sink 0.0.0.0 / ::
//!   6. SafeSearch            -> CNAME (chained with the target's A/AAAA)
//!   7. else                  -> forward upstream

use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA, CNAME};
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc;

use sanctum_core::{domain, Blocklist, SafeSearchMap};

/// Short TTL so un-blocks / list edits propagate to clients quickly.
const TTL: u32 = 5;
/// The watchdog liveness canary name and its fixed answer.
pub const HEALTH_CANARY: &str = "health.sanctum.invalid";
const HEALTH_ANSWER: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
/// Firefox's auto-DoH probe. Answering NXDOMAIN disables its DoH upgrade.
const DOH_CANARY: &str = "use-application-dns.net";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Decision {
    HealthCanary,
    DohDisableCanary,
    Forward,
    Sink(SinkKind),
    SafeSearch(String),
}

/// Which list produced a sink. Only adult-block hits are treated as "urges"
/// for the intervention system; DoH-endpoint sinks are plumbing, not intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SinkKind {
    Blocked,
    Doh,
}

/// The hot-reloadable filter lists + toggles.
#[derive(Debug, Clone)]
pub struct FilterState {
    pub blocklist: Blocklist,
    /// Suffix-matched allowlist (explicit exceptions).
    pub allowlist: HashSet<String>,
    pub safesearch: SafeSearchMap,
    /// DoH provider hostnames, matched by suffix like the blocklist.
    pub doh: Blocklist,
    pub enforce_safesearch: bool,
    pub block_doh: bool,
}

impl FilterState {
    pub fn new(blocklist: Blocklist, safesearch: SafeSearchMap, doh: Blocklist) -> Self {
        Self {
            blocklist,
            allowlist: HashSet::new(),
            safesearch,
            doh,
            enforce_safesearch: true,
            block_doh: true,
        }
    }

    fn is_allowed(&self, host: &str) -> bool {
        !self.allowlist.is_empty() && domain::is_blocked_by(host, &self.allowlist)
    }

    fn classify(&self, host: &str) -> Decision {
        if host == HEALTH_CANARY {
            Decision::HealthCanary
        } else if host == DOH_CANARY {
            Decision::DohDisableCanary
        } else if self.is_allowed(host) {
            Decision::Forward
        } else if self.blocklist.is_blocked(host) {
            Decision::Sink(SinkKind::Blocked)
        } else if self.block_doh && self.doh.is_blocked(host) {
            Decision::Sink(SinkKind::Doh)
        } else if self.enforce_safesearch {
            match self.safesearch.lookup(host) {
                Some(target) => Decision::SafeSearch(target.to_string()),
                None => Decision::Forward,
            }
        } else {
            Decision::Forward
        }
    }
}

/// The resolver. Cheap to wrap in an `Arc` and share across accept loops.
pub struct Resolver {
    state: RwLock<FilterState>,
    pub upstreams: Vec<SocketAddr>,
    pub sink_v4: Ipv4Addr,
    pub sink_v6: Ipv6Addr,
    pub upstream_timeout: Duration,
    /// Optional sink for sinkholed adult-block hosts (v0.1.5 §A). Set once at
    /// bring-up; `None` in tests and until wired.
    block_tx: OnceLock<mpsc::UnboundedSender<String>>,
}

impl Resolver {
    pub fn from_state(
        state: FilterState,
        upstreams: Vec<SocketAddr>,
        sink_v4: Ipv4Addr,
        sink_v6: Ipv6Addr,
    ) -> Self {
        Self {
            state: RwLock::new(state),
            upstreams,
            sink_v4,
            sink_v6,
            upstream_timeout: Duration::from_secs(2),
            block_tx: OnceLock::new(),
        }
    }

    /// Wire the block-event sink (v0.1.5 §A). Idempotent; the first caller wins.
    pub fn set_block_sink(&self, tx: mpsc::UnboundedSender<String>) {
        let _ = self.block_tx.set(tx);
    }

    /// Convenience constructor with default sinks and toggles.
    pub fn new(
        blocklist: Blocklist,
        safesearch: SafeSearchMap,
        doh: Blocklist,
        upstreams: Vec<SocketAddr>,
    ) -> Self {
        Self::from_state(
            FilterState::new(blocklist, safesearch, doh),
            upstreams,
            Ipv4Addr::UNSPECIFIED,
            Ipv6Addr::UNSPECIFIED,
        )
    }

    /// Hot-reload the filter lists (called by IPC changes + reconcile).
    pub fn update(&self, state: FilterState) {
        *self.state.write().unwrap() = state;
    }

    /// Number of blocked registrable domains currently loaded.
    pub fn blocklist_len(&self) -> usize {
        self.state.read().unwrap().blocklist.len()
    }

    fn classify(&self, host: &str) -> Decision {
        self.state.read().unwrap().classify(host)
    }

    /// Handle one raw DNS query and produce the raw response bytes.
    /// Returns `None` if the query is undecodable or forwarding failed.
    pub async fn handle_packet(&self, query: &[u8]) -> Option<Vec<u8>> {
        let req = Message::from_vec(query).ok()?;
        let Some(q) = req.queries().first().cloned() else {
            return self.send_upstream(query).await;
        };

        let host = q.name().to_string();
        let host = host.trim_end_matches('.').to_ascii_lowercase();

        // Classify under a short read lock; never held across an await.
        let response = match self.classify(&host) {
            Decision::Forward => return self.send_upstream(query).await,
            Decision::HealthCanary => self.canary_response(&req, &q),
            Decision::DohDisableCanary => {
                let mut resp = base_response(&req);
                resp.set_response_code(ResponseCode::NXDomain);
                resp
            }
            Decision::Sink(SinkKind::Blocked) => {
                // A real adult-block hit: feed the intervention debouncer.
                if let Some(tx) = self.block_tx.get() {
                    let _ = tx.send(host.clone());
                }
                self.sink_response(&req, &q)
            }
            Decision::Sink(SinkKind::Doh) => self.sink_response(&req, &q),
            Decision::SafeSearch(target) => self.safesearch_response(&req, &q, &target).await,
        };
        response.to_vec().ok()
    }

    fn canary_response(&self, req: &Message, q: &Query) -> Message {
        let mut resp = base_response(req);
        resp.set_response_code(ResponseCode::NoError);
        if q.query_type() == RecordType::A {
            resp.add_answer(Record::from_rdata(
                q.name().clone(),
                TTL,
                RData::A(A(HEALTH_ANSWER)),
            ));
        }
        resp
    }

    fn sink_response(&self, req: &Message, q: &Query) -> Message {
        let mut resp = base_response(req);
        resp.set_response_code(ResponseCode::NoError);
        match q.query_type() {
            RecordType::A => {
                resp.add_answer(Record::from_rdata(
                    q.name().clone(),
                    TTL,
                    RData::A(A(self.sink_v4)),
                ));
            }
            RecordType::AAAA => {
                resp.add_answer(Record::from_rdata(
                    q.name().clone(),
                    TTL,
                    RData::AAAA(AAAA(self.sink_v6)),
                ));
            }
            // For HTTPS/SVCB/MX/etc., an empty NOERROR is enough: the
            // client still needs A/AAAA (both sinkholed) to connect.
            _ => {}
        }
        resp
    }

    /// SafeSearch: CNAME to the safe target + the target's upstream-resolved
    /// A/AAAA (a bare CNAME reads as incomplete to the Windows stub resolver
    /// and silently fails). ADR-001 §3.4.
    async fn safesearch_response(&self, req: &Message, q: &Query, target: &str) -> Message {
        let mut resp = base_response(req);
        resp.set_response_code(ResponseCode::NoError);

        let target_name = match Name::from_ascii(format!("{target}.")) {
            Ok(n) => n,
            Err(_) => return self.sink_response(req, q),
        };
        resp.add_answer(Record::from_rdata(
            q.name().clone(),
            TTL,
            RData::CNAME(CNAME(target_name.clone())),
        ));

        if matches!(q.query_type(), RecordType::A | RecordType::AAAA) {
            if let Some(upstream) = self.resolve_upstream(&target_name, q.query_type()).await {
                for ans in upstream.answers() {
                    resp.add_answer(ans.clone());
                }
            }
        }
        resp
    }

    async fn resolve_upstream(&self, name: &Name, rtype: RecordType) -> Option<Message> {
        let mut msg = Message::new();
        msg.set_id(0x5A57);
        msg.set_message_type(MessageType::Query);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        let mut query = Query::new();
        query.set_name(name.clone());
        query.set_query_type(rtype);
        query.set_query_class(DNSClass::IN);
        msg.add_query(query);

        let bytes = msg.to_vec().ok()?;
        let reply = self.send_upstream(&bytes).await?;
        Message::from_vec(&reply).ok()
    }

    /// Forward raw query bytes to the first responsive upstream; return the
    /// raw reply. Loop-guarded against forwarding to a loopback :53.
    async fn send_upstream(&self, query: &[u8]) -> Option<Vec<u8>> {
        for up in &self.upstreams {
            if up.ip().is_loopback() && up.port() == 53 {
                continue;
            }
            let bind: SocketAddr = if up.is_ipv4() {
                (Ipv4Addr::UNSPECIFIED, 0).into()
            } else {
                (Ipv6Addr::UNSPECIFIED, 0).into()
            };
            let Ok(sock) = UdpSocket::bind(bind).await else {
                continue;
            };
            if sock.connect(up).await.is_err() || sock.send(query).await.is_err() {
                continue;
            }
            let mut buf = vec![0u8; 4096];
            match tokio::time::timeout(self.upstream_timeout, sock.recv(&mut buf)).await {
                Ok(Ok(n)) => {
                    buf.truncate(n);
                    return Some(buf);
                }
                _ => continue,
            }
        }
        None
    }

    /// Bind a UDP listener at `addr`, spawn its serve loop, and return the
    /// bound local address (useful when `addr` uses port 0 in tests).
    pub async fn spawn_udp(self: Arc<Self>, addr: SocketAddr) -> std::io::Result<SocketAddr> {
        let sock = Arc::new(UdpSocket::bind(addr).await?);
        let local = sock.local_addr()?;
        let me = self.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match sock.recv_from(&mut buf).await {
                    Ok((n, peer)) => {
                        let query = buf[..n].to_vec();
                        let me2 = me.clone();
                        let s = sock.clone();
                        tokio::spawn(async move {
                            if let Some(resp) = me2.handle_packet(&query).await {
                                let _ = s.send_to(&resp, peer).await;
                            }
                        });
                    }
                    // A recv error must NEVER kill the listener. On Windows,
                    // WSAECONNRESET (10054) is delivered on the recv *after* a
                    // reply to a since-closed client port triggers an ICMP Port
                    // Unreachable — benign and frequent. Breaking here dropped the
                    // whole resolver, silently stopping all blocking. Keep serving.
                    Err(_) => continue,
                }
            }
        });
        Ok(local)
    }

    /// Bind a TCP listener at `addr`, spawn its serve loop (length-prefixed
    /// DNS), and return the bound local address.
    pub async fn spawn_tcp(self: Arc<Self>, addr: SocketAddr) -> std::io::Result<SocketAddr> {
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let me2 = me.clone();
                tokio::spawn(async move {
                    loop {
                        let mut len_buf = [0u8; 2];
                        if stream.read_exact(&mut len_buf).await.is_err() {
                            break;
                        }
                        let len = u16::from_be_bytes(len_buf) as usize;
                        let mut msg = vec![0u8; len];
                        if stream.read_exact(&mut msg).await.is_err() {
                            break;
                        }
                        match me2.handle_packet(&msg).await {
                            Some(resp) => {
                                let rlen = (resp.len() as u16).to_be_bytes();
                                if stream.write_all(&rlen).await.is_err()
                                    || stream.write_all(&resp).await.is_err()
                                {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                });
            }
        });
        Ok(local)
    }
}

/// A response message pre-populated from the request (id, flags, question).
fn base_response(req: &Message) -> Message {
    let mut resp = Message::new();
    resp.set_id(req.id());
    resp.set_message_type(MessageType::Response);
    resp.set_op_code(req.op_code());
    resp.set_recursion_desired(req.recursion_desired());
    resp.set_recursion_available(true);
    resp.set_authoritative(false);
    for q in req.queries() {
        resp.add_query(q.clone());
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> Resolver {
        let (mut block, _) = Blocklist::parse("bad.com\n");
        block.add("ads.example.com");
        let (ss, _) = SafeSearchMap::parse("google.com forcesafesearch.google.com\n");
        let (doh, _) = Blocklist::parse("dns.google\n");
        Resolver::new(block, ss, doh, vec!["1.1.1.1:53".parse().unwrap()])
    }

    #[test]
    fn classify_pipeline() {
        let r = resolver();
        assert_eq!(r.classify("health.sanctum.invalid"), Decision::HealthCanary);
        assert_eq!(r.classify("use-application-dns.net"), Decision::DohDisableCanary);
        assert_eq!(r.classify("bad.com"), Decision::Sink(SinkKind::Blocked));
        assert_eq!(r.classify("www.bad.com"), Decision::Sink(SinkKind::Blocked));
        assert_eq!(r.classify("dns.google"), Decision::Sink(SinkKind::Doh));
        assert_eq!(
            r.classify("google.com"),
            Decision::SafeSearch("forcesafesearch.google.com".into())
        );
        assert_eq!(r.classify("example.org"), Decision::Forward);
    }

    #[test]
    fn hot_reload_updates_and_allowlist_overrides() {
        let r = resolver();
        assert_eq!(r.classify("ads.example.com"), Decision::Sink(SinkKind::Blocked));

        // Reload with an allowlist entry -> now forwarded.
        let mut st = r.state.read().unwrap().clone();
        st.allowlist.insert("ads.example.com".to_string());
        r.update(st);
        assert_eq!(r.classify("ads.example.com"), Decision::Forward);

        // Reload with a new blocked domain -> now sinkholed.
        let mut st = r.state.read().unwrap().clone();
        st.blocklist.add("newbad.com");
        r.update(st);
        assert_eq!(r.classify("sub.newbad.com"), Decision::Sink(SinkKind::Blocked));
    }
}
