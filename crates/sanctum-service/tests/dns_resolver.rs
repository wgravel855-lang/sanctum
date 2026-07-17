//! Hermetic end-to-end test of the DNS sinkhole (no real internet).
//!
//! A mock upstream answers every A with 1.2.3.4 and every AAAA with a
//! fixed v6 address, so we can prove the resolver's decisions: blocked
//! domains are sinkholed, allowed ones are forwarded, SafeSearch is
//! CNAME-chained, and the canaries behave — all against loopback ports.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA};
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
use tokio::net::UdpSocket;
use tokio::time::timeout;

use sanctum_core::{Blocklist, SafeSearchMap};
use sanctum_service::dns::Resolver;

const MOCK_V4: Ipv4Addr = Ipv4Addr::new(1, 2, 3, 4);
const MOCK_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1);

/// A tiny upstream that answers A -> 1.2.3.4, AAAA -> 2001:db8::1.
async fn spawn_mock_upstream() -> SocketAddr {
    let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = sock.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            let (n, peer) = match sock.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(_) => break,
            };
            let req = Message::from_vec(&buf[..n]).unwrap();
            let q = req.queries()[0].clone();
            let mut resp = Message::new();
            resp.set_id(req.id());
            resp.set_message_type(MessageType::Response);
            resp.set_op_code(OpCode::Query);
            resp.set_recursion_available(true);
            resp.add_query(q.clone());
            resp.set_response_code(ResponseCode::NoError);
            match q.query_type() {
                RecordType::A => {
                    resp.add_answer(Record::from_rdata(q.name().clone(), 60, RData::A(A(MOCK_V4))));
                }
                RecordType::AAAA => {
                    resp.add_answer(Record::from_rdata(
                        q.name().clone(),
                        60,
                        RData::AAAA(AAAA(MOCK_V6)),
                    ));
                }
                _ => {}
            }
            let _ = sock.send_to(&resp.to_vec().unwrap(), peer).await;
        }
    });
    addr
}

async fn query(resolver_addr: SocketAddr, name: &str, rtype: RecordType) -> Message {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.connect(resolver_addr).await.unwrap();
    let mut msg = Message::new();
    msg.set_id(0x1234);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    let mut q = Query::new();
    q.set_name(Name::from_ascii(format!("{name}.")).unwrap());
    q.set_query_type(rtype);
    q.set_query_class(DNSClass::IN);
    msg.add_query(q);
    sock.send(&msg.to_vec().unwrap()).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(3), sock.recv(&mut buf))
        .await
        .expect("resolver timed out")
        .unwrap();
    Message::from_vec(&buf[..n]).unwrap()
}

fn a_answers(msg: &Message) -> Vec<Ipv4Addr> {
    msg.answers()
        .iter()
        .filter_map(|r| match r.data() {
            RData::A(A(ip)) => Some(*ip),
            _ => None,
        })
        .collect()
}

fn aaaa_answers(msg: &Message) -> Vec<Ipv6Addr> {
    msg.answers()
        .iter()
        .filter_map(|r| match r.data() {
            RData::AAAA(AAAA(ip)) => Some(*ip),
            _ => None,
        })
        .collect()
}

fn has_cname_to(msg: &Message, target: &str) -> bool {
    msg.answers().iter().any(|r| match r.data() {
        RData::CNAME(c) => c.0.to_string().trim_end_matches('.') == target,
        _ => false,
    })
}

async fn make_resolver() -> SocketAddr {
    let upstream = spawn_mock_upstream().await;
    let (block, _) = Blocklist::parse("bad.com\n");
    let (ss, _) = SafeSearchMap::parse("google.com forcesafesearch.google.com\n");
    let (doh, _) = Blocklist::parse("dns.google\n");
    let resolver = Arc::new(Resolver::new(block, ss, doh, vec![upstream]));
    resolver
        .spawn_udp("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn blocked_domain_is_sinkholed_v4_and_v6() {
    let addr = make_resolver().await;

    let resp = query(addr, "bad.com", RecordType::A).await;
    assert_eq!(a_answers(&resp), vec![Ipv4Addr::UNSPECIFIED]);

    // Subdomains are covered by suffix match.
    let resp = query(addr, "cdn.bad.com", RecordType::A).await;
    assert_eq!(a_answers(&resp), vec![Ipv4Addr::UNSPECIFIED]);

    // IPv6 must not leak past the block.
    let resp = query(addr, "bad.com", RecordType::AAAA).await;
    assert_eq!(aaaa_answers(&resp), vec![Ipv6Addr::UNSPECIFIED]);
}

#[tokio::test]
async fn allowed_domain_is_forwarded() {
    let addr = make_resolver().await;
    let resp = query(addr, "example.org", RecordType::A).await;
    assert_eq!(a_answers(&resp), vec![MOCK_V4]);
}

#[tokio::test]
async fn doh_endpoint_is_sinkholed() {
    let addr = make_resolver().await;
    let resp = query(addr, "dns.google", RecordType::A).await;
    assert_eq!(a_answers(&resp), vec![Ipv4Addr::UNSPECIFIED]);
}

#[tokio::test]
async fn safesearch_is_cname_chained() {
    let addr = make_resolver().await;
    let resp = query(addr, "google.com", RecordType::A).await;
    assert!(
        has_cname_to(&resp, "forcesafesearch.google.com"),
        "expected CNAME to the safe host"
    );
    // The chained A (resolved from the target via the mock upstream).
    assert!(
        a_answers(&resp).contains(&MOCK_V4),
        "expected the target's A record chained in"
    );
}

#[tokio::test]
async fn canaries_behave() {
    let addr = make_resolver().await;

    let resp = query(addr, "health.sanctum.invalid", RecordType::A).await;
    assert_eq!(a_answers(&resp), vec![Ipv4Addr::new(127, 0, 0, 2)]);

    let resp = query(addr, "use-application-dns.net", RecordType::A).await;
    assert_eq!(resp.response_code(), ResponseCode::NXDomain);
}
