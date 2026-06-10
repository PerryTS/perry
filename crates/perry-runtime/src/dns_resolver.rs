//! Synchronous DNS client backing the real `node:dns` resolve*/reverse
//! family (#4911).
//!
//! Builds queries with `hickory-proto`'s wire-format encoder and sends them
//! over a blocking `std::net::UdpSocket` (falling back to TCP when the server
//! sets the truncation bit), reading the system nameserver list from
//! `/etc/resolv.conf`. Deliberately tokio-free — this mirrors `net.rs`'s
//! blocking-I/O style and keeps the core runtime off the async runtime.
//!
//! The JS-facing value construction lives in `dns.rs`; this module only
//! returns plain Rust [`Answer`] values plus a [`DnsError`] that `dns.rs`
//! maps onto Node's c-ares error codes.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use hickory_proto::op::{Message, Query, ResponseCode};
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};

const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const UDP_RECV_CAP: usize = 4096;

/// The record families Perry's `dns.resolve*` surface understands. Mirrors
/// `dns.rs`'s `RecordKind`; kept separate so the hickory dependency stays
/// contained in this module.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    A,
    Aaaa,
    Any,
    Caa,
    Cname,
    Mx,
    Naptr,
    Ns,
    Ptr,
    Soa,
    Srv,
    Tlsa,
    Txt,
}

impl QueryType {
    fn record_type(self) -> RecordType {
        match self {
            QueryType::A => RecordType::A,
            QueryType::Aaaa => RecordType::AAAA,
            QueryType::Any => RecordType::ANY,
            QueryType::Caa => RecordType::CAA,
            QueryType::Cname => RecordType::CNAME,
            QueryType::Mx => RecordType::MX,
            QueryType::Naptr => RecordType::NAPTR,
            QueryType::Ns => RecordType::NS,
            QueryType::Ptr => RecordType::PTR,
            QueryType::Soa => RecordType::SOA,
            QueryType::Srv => RecordType::SRV,
            QueryType::Tlsa => RecordType::TLSA,
            QueryType::Txt => RecordType::TXT,
        }
    }
}

/// Failure modes surfaced to `dns.rs`, which translates them into the
/// matching Node c-ares error (`ENOTFOUND`, `ENODATA`, `ESERVFAIL`, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsError {
    /// NXDOMAIN — the name does not exist.
    NotFound,
    /// NOERROR but no records of the requested type.
    NoData,
    /// SERVFAIL from the upstream resolver.
    ServFail,
    /// REFUSED from the upstream resolver.
    Refused,
    /// No response within [`QUERY_TIMEOUT`] from any configured server.
    Timeout,
    /// Malformed response / could not encode the query / socket error.
    BadResp,
    /// The query name was not a valid DNS name.
    BadName,
}

pub struct MxAnswer {
    pub priority: u16,
    pub exchange: String,
}

pub struct SoaAnswer {
    pub nsname: String,
    pub hostmaster: String,
    pub serial: u32,
    pub refresh: i64,
    pub retry: i64,
    pub expire: i64,
    pub minttl: u32,
}

pub struct SrvAnswer {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub name: String,
}

pub struct NaptrAnswer {
    pub order: u16,
    pub preference: u16,
    pub flags: String,
    pub service: String,
    pub regexp: String,
    pub replacement: String,
}

pub struct TlsaAnswer {
    pub usage: u8,
    pub selector: u8,
    pub matching_type: u8,
    pub certificate: Vec<u8>,
}

pub struct CaaAnswer {
    pub critical: u8,
    /// Property tag — `"issue"`, `"issuewild"`, `"iodef"`, `"contactemail"`,
    /// `"contactphone"`, … — which becomes the object key in `dns.rs`.
    pub field: String,
    pub value: String,
}

/// A single resolved record, type-tagged so `resolveAny` can build Node's
/// mixed `{ type, … }` objects from the same data the typed resolvers use.
pub struct ResolvedRecord {
    pub rtype: &'static str,
    pub ttl: u32,
    pub data: Answer,
}

pub enum Answer {
    /// A / AAAA address.
    Addr(IpAddr),
    /// CNAME / NS / PTR target (trailing dot stripped).
    Name(String),
    Mx(MxAnswer),
    /// One TXT record's character-strings.
    Txt(Vec<String>),
    Soa(SoaAnswer),
    Srv(SrvAnswer),
    Naptr(NaptrAnswer),
    Tlsa(TlsaAnswer),
    Caa(CaaAnswer),
}

fn name_to_string(name: &Name) -> String {
    name.to_utf8().trim_end_matches('.').to_string()
}

fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The system nameserver list (`/etc/resolv.conf` on unix), with a public
/// resolver fallback when the file is missing, empty, or unparsable (e.g.
/// on Windows, which has no resolv.conf).
pub fn system_nameservers() -> Vec<SocketAddr> {
    #[cfg(unix)]
    {
        if let Ok(data) = std::fs::read("/etc/resolv.conf") {
            if let Ok(cfg) = resolv_conf::Config::parse(&data) {
                let servers: Vec<SocketAddr> = cfg
                    .nameservers
                    .iter()
                    .map(|scoped| SocketAddr::new(IpAddr::from(scoped), 53))
                    .collect();
                if !servers.is_empty() {
                    return servers;
                }
            }
        }
    }
    vec![
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53),
    ]
}

fn rcode_to_error(code: ResponseCode) -> Option<DnsError> {
    match code {
        ResponseCode::NoError => None,
        ResponseCode::NXDomain => Some(DnsError::NotFound),
        ResponseCode::ServFail => Some(DnsError::ServFail),
        ResponseCode::Refused => Some(DnsError::Refused),
        _ => Some(DnsError::BadResp),
    }
}

fn build_query(name: Name, rtype: RecordType) -> Result<(Vec<u8>, u16), DnsError> {
    let mut message = Message::query();
    message.metadata.recursion_desired = true;
    let id = message.metadata.id;
    let mut query = Query::query(name, rtype);
    query.set_query_class(DNSClass::IN);
    message.add_query(query);
    let bytes = message.to_vec().map_err(|_| DnsError::BadResp)?;
    Ok((bytes, id))
}

/// Map a `std::io` error from a socket op onto our error enum — a read
/// timeout (the common "no route / dropped" case) becomes [`DnsError::Timeout`].
fn io_error(err: &std::io::Error) -> DnsError {
    match err.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => DnsError::Timeout,
        _ => DnsError::BadResp,
    }
}

fn query_udp(server: SocketAddr, request: &[u8], id: u16) -> Result<Message, DnsError> {
    let bind_addr: SocketAddr = if server.is_ipv4() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    } else {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
    };
    let socket = UdpSocket::bind(bind_addr).map_err(|e| io_error(&e))?;
    socket
        .set_read_timeout(Some(QUERY_TIMEOUT))
        .map_err(|e| io_error(&e))?;
    socket.send_to(request, server).map_err(|e| io_error(&e))?;

    let mut buf = [0u8; UDP_RECV_CAP];
    loop {
        let (n, from) = socket.recv_from(&mut buf).map_err(|e| io_error(&e))?;
        // Ignore stray datagrams from other peers (spoofing guard).
        if from.ip() != server.ip() {
            continue;
        }
        let message = Message::from_vec(&buf[..n]).map_err(|_| DnsError::BadResp)?;
        if message.metadata.id != id {
            continue;
        }
        if message.metadata.truncation {
            return query_tcp(server, request, id);
        }
        return Ok(message);
    }
}

fn query_tcp(server: SocketAddr, request: &[u8], id: u16) -> Result<Message, DnsError> {
    use std::io::{Read, Write};

    let mut stream =
        TcpStream::connect_timeout(&server, QUERY_TIMEOUT).map_err(|e| io_error(&e))?;
    stream
        .set_read_timeout(Some(QUERY_TIMEOUT))
        .map_err(|e| io_error(&e))?;
    // DNS-over-TCP frames the message with a 2-byte big-endian length prefix.
    let len = u16::try_from(request.len()).map_err(|_| DnsError::BadResp)?;
    stream
        .write_all(&len.to_be_bytes())
        .map_err(|e| io_error(&e))?;
    stream.write_all(request).map_err(|e| io_error(&e))?;

    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).map_err(|e| io_error(&e))?;
    let resp_len = u16::from_be_bytes(len_buf) as usize;
    let mut resp = vec![0u8; resp_len];
    stream.read_exact(&mut resp).map_err(|e| io_error(&e))?;

    let message = Message::from_vec(&resp).map_err(|_| DnsError::BadResp)?;
    if message.metadata.id != id {
        return Err(DnsError::BadResp);
    }
    Ok(message)
}

/// Send `name`/`rtype` to each server in turn. NXDOMAIN / REFUSED are
/// authoritative answers (returned immediately); SERVFAIL and timeouts fall
/// through to the next server.
fn query(servers: &[SocketAddr], name: Name, rtype: RecordType) -> Result<Message, DnsError> {
    let (request, id) = build_query(name, rtype)?;
    let mut last_error = DnsError::Timeout;
    for &server in servers {
        match query_udp(server, &request, id) {
            Ok(message) => match rcode_to_error(message.metadata.response_code) {
                None => return Ok(message),
                Some(err @ (DnsError::NotFound | DnsError::Refused)) => return Err(err),
                Some(err) => last_error = err,
            },
            Err(err @ (DnsError::NotFound | DnsError::Refused)) => return Err(err),
            Err(err) => last_error = err,
        }
    }
    Err(last_error)
}

fn map_caa(caa: &hickory_proto::rr::rdata::CAA) -> CaaAnswer {
    // Node keys each CAA object by its property tag (`issue`/`issuewild`/
    // `iodef`/`contactemail`/…) with the raw property value as the string.
    CaaAnswer {
        critical: caa.flags(),
        field: caa.tag.clone(),
        value: bytes_to_string(&caa.value),
    }
}

/// Map one wire record onto a typed [`ResolvedRecord`], or `None` for record
/// types Perry's `dns` surface does not expose.
fn map_record(record: &Record) -> Option<ResolvedRecord> {
    let ttl = record.ttl;
    let (rtype, data) = match &record.data {
        RData::A(a) => ("A", Answer::Addr(IpAddr::V4(a.0))),
        RData::AAAA(aaaa) => ("AAAA", Answer::Addr(IpAddr::V6(aaaa.0))),
        RData::CNAME(cname) => ("CNAME", Answer::Name(name_to_string(&cname.0))),
        RData::NS(ns) => ("NS", Answer::Name(name_to_string(&ns.0))),
        RData::PTR(ptr) => ("PTR", Answer::Name(name_to_string(&ptr.0))),
        RData::MX(mx) => (
            "MX",
            Answer::Mx(MxAnswer {
                priority: mx.preference,
                exchange: name_to_string(&mx.exchange),
            }),
        ),
        RData::TXT(txt) => (
            "TXT",
            Answer::Txt(
                txt.txt_data
                    .iter()
                    .map(|chunk| bytes_to_string(chunk))
                    .collect(),
            ),
        ),
        RData::SOA(soa) => (
            "SOA",
            Answer::Soa(SoaAnswer {
                nsname: name_to_string(&soa.mname),
                hostmaster: name_to_string(&soa.rname),
                serial: soa.serial,
                refresh: soa.refresh as i64,
                retry: soa.retry as i64,
                expire: soa.expire as i64,
                minttl: soa.minimum,
            }),
        ),
        RData::SRV(srv) => (
            "SRV",
            Answer::Srv(SrvAnswer {
                priority: srv.priority,
                weight: srv.weight,
                port: srv.port,
                name: name_to_string(&srv.target),
            }),
        ),
        RData::NAPTR(naptr) => (
            "NAPTR",
            Answer::Naptr(NaptrAnswer {
                order: naptr.order,
                preference: naptr.preference,
                flags: bytes_to_string(&naptr.flags),
                service: bytes_to_string(&naptr.services),
                regexp: bytes_to_string(&naptr.regexp),
                replacement: name_to_string(&naptr.replacement),
            }),
        ),
        RData::TLSA(tlsa) => (
            "TLSA",
            Answer::Tlsa(TlsaAnswer {
                usage: u8::from(tlsa.cert_usage),
                selector: u8::from(tlsa.selector),
                matching_type: u8::from(tlsa.matching),
                certificate: tlsa.cert_data.clone(),
            }),
        ),
        RData::CAA(caa) => ("CAA", Answer::Caa(map_caa(caa))),
        _ => return None,
    };
    Some(ResolvedRecord { rtype, ttl, data })
}

/// Resolve `name` for `qtype` against `servers` (empty → system resolvers).
/// Returns the matching answer records, or [`DnsError::NoData`] when the name
/// exists but has no records of that type.
pub fn resolve(
    name: &str,
    qtype: QueryType,
    servers: &[SocketAddr],
) -> Result<Vec<ResolvedRecord>, DnsError> {
    let fqdn = Name::from_utf8(name).map_err(|_| DnsError::BadName)?;
    let owned_servers;
    let servers = if servers.is_empty() {
        owned_servers = system_nameservers();
        owned_servers.as_slice()
    } else {
        servers
    };
    if servers.is_empty() {
        return Err(DnsError::ServFail);
    }

    let want = qtype.record_type();
    let message = query(servers, fqdn, want)?;

    let mut out = Vec::new();
    for record in &message.answers {
        let Some(resolved) = map_record(record) else {
            continue;
        };
        // For a typed query keep only matching records (the answer section
        // can also carry the CNAME chain that led to them); ANY keeps all.
        if qtype == QueryType::Any || record.record_type() == want {
            out.push(resolved);
        }
    }

    if out.is_empty() {
        return Err(DnsError::NoData);
    }
    Ok(out)
}

fn reverse_name(ip: IpAddr) -> Name {
    let label = match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.{}.in-addr.arpa.", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v6) => {
            let mut s = String::with_capacity(72);
            for octet in v6.octets().iter().rev() {
                s.push_str(&format!("{:x}.{:x}.", octet & 0x0f, octet >> 4));
            }
            s.push_str("ip6.arpa.");
            s
        }
    };
    // The constructed reverse name is always valid; fall back to root on the
    // impossible parse error rather than panicking.
    Name::from_ascii(&label).unwrap_or_else(|_| Name::root())
}

/// `dns.reverse(ip)` — PTR lookup of the reverse zone, returning every PTR
/// target. Empty result is reported as [`DnsError::NotFound`], matching
/// Node's `ENOTFOUND` for an address with no PTR record.
pub fn reverse(ip: IpAddr, servers: &[SocketAddr]) -> Result<Vec<String>, DnsError> {
    let owned_servers;
    let servers = if servers.is_empty() {
        owned_servers = system_nameservers();
        owned_servers.as_slice()
    } else {
        servers
    };
    if servers.is_empty() {
        return Err(DnsError::ServFail);
    }

    let message = query(servers, reverse_name(ip), RecordType::PTR)?;
    let names: Vec<String> = message
        .answers
        .iter()
        .filter_map(|record| match &record.data {
            RData::PTR(ptr) => Some(name_to_string(&ptr.0)),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        return Err(DnsError::NotFound);
    }
    Ok(names)
}
