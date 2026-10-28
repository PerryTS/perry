//! The `perry_net` imports: outbound TCP and name lookup for `PERRY_WASM_NET=1`
//! modules (docs/WASI_P1_OUTPUT.md).
//!
//! `tcp_connect` never blocks. A name is resolved on a worker thread that
//! writes its answer to a pipe, so the guest's descriptor can sit in
//! `poll_oneoff` next to everything else. The descriptor then walks
//! Resolving -> Connecting -> Connected or Failed: the resolved addresses are
//! tried in order with non-blocking connects, and `poll_oneoff` reports the
//! descriptor only once it is Connected or Failed. Closing it drops the pipe
//! and the socket, which cancels whatever was under way.
//!
//! The lookup is a table from `--dns`, so a test decides the answer and how long
//! it takes. A literal address resolves to itself, `localhost` to 127.0.0.1,
//! and any other name not in the table is not found; the host never asks the
//! network.

use crate::fdtable::Fd;
use crate::preview1::{errno, map_os, put_u32, span, split, Res, Stats};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::time::Duration;
use wasmtime::Caller;

/// What `sock_error` reports for a lookup that failed, whatever the reason.
pub const LOOKUP_FAILED: u16 = 2000;

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_TEMPORARY: u8 = 2;
const RECORD: usize = 17;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Addresses(Vec<IpAddr>),
    NotFound,
    Temporary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub answer: Answer,
    pub delay: Duration,
}

/// The names the host can resolve, from `--dns`.
#[derive(Default)]
pub struct Dns {
    entries: HashMap<String, Entry>,
}

impl Dns {
    /// `NAME=RESULT[,delay=MS]`, where RESULT is `notfound`, `temp` or one or
    /// more addresses joined by `+` (`127.0.0.2+127.0.0.1`).
    pub fn add(&mut self, spec: &str) -> Result<(), String> {
        let (name, rest) = spec.split_once('=').ok_or("--dns wants NAME=RESULT")?;
        let mut parts = rest.split(',');
        let result = parts.next().unwrap_or_default();
        let answer = match result {
            "notfound" => Answer::NotFound,
            "temp" => Answer::Temporary,
            _ => Answer::Addresses(
                result
                    .split('+')
                    .map(|a| a.parse().map_err(|_| format!("bad address `{a}` in --dns")))
                    .collect::<Result<_, _>>()?,
            ),
        };
        let mut delay = Duration::ZERO;
        for option in parts {
            let ms = option
                .strip_prefix("delay=")
                .ok_or_else(|| format!("unknown --dns option `{option}`"))?;
            delay = Duration::from_millis(ms.parse().map_err(|_| "bad --dns delay")?);
        }
        if name.is_empty() {
            return Err("--dns wants a name".into());
        }
        self.entries
            .insert(name.to_ascii_lowercase(), Entry { answer, delay });
        Ok(())
    }

    fn lookup(&self, name: &str) -> Entry {
        let name = name.to_ascii_lowercase();
        if let Some(entry) = self.entries.get(&name) {
            return entry.clone();
        }
        let answer = if let Ok(ip) = name.parse::<IpAddr>() {
            Answer::Addresses(vec![ip])
        } else if name == "localhost" {
            Answer::Addresses(vec!["127.0.0.1".parse().unwrap()])
        } else {
            Answer::NotFound
        };
        Entry {
            answer,
            delay: Duration::ZERO,
        }
    }
}

/// The bytes of a `resolve` stream for `entry`, filtered to `family` (0, 4, 6).
fn reply(entry: &Entry, family: u32) -> Vec<u8> {
    let addresses: Vec<&IpAddr> = match &entry.answer {
        Answer::Addresses(all) => all
            .iter()
            .filter(|ip| match family {
                4 => ip.is_ipv4(),
                6 => ip.is_ipv6(),
                _ => true,
            })
            .collect(),
        _ => vec![],
    };
    let status = match &entry.answer {
        Answer::Addresses(_) if addresses.is_empty() => STATUS_NOT_FOUND,
        Answer::Addresses(_) => STATUS_OK,
        Answer::NotFound => STATUS_NOT_FOUND,
        Answer::Temporary => STATUS_TEMPORARY,
    };
    let mut bytes = vec![status];
    for ip in addresses {
        let mut record = [0u8; RECORD];
        match ip {
            IpAddr::V4(v4) => {
                record[0] = 4;
                record[1..5].copy_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                record[0] = 6;
                record[1..].copy_from_slice(&v6.octets());
            }
        }
        bytes.extend_from_slice(&record);
    }
    bytes
}

/// The addresses in a complete answer, or why there are none.
fn parse_reply(bytes: &[u8]) -> Result<Vec<IpAddr>, ()> {
    match bytes.split_first() {
        Some((&STATUS_OK, records)) => Ok(records
            .chunks_exact(RECORD)
            .map(|r| {
                if r[0] == 4 {
                    IpAddr::from(<[u8; 4]>::try_from(&r[1..5]).unwrap())
                } else {
                    IpAddr::from(<[u8; 16]>::try_from(&r[1..]).unwrap())
                }
            })
            .collect()),
        _ => Err(()),
    }
}

/// Start the lookup on a worker thread and return the read end of the pipe the
/// answer arrives on: non-blocking, and at end of file once the answer is in.
fn spawn_resolver(dns: &Arc<Dns>, name: String, family: u32) -> Res<OwnedFd> {
    let mut ends = [0i32; 2];
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        return Err(errno::IO);
    }
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };
    set_nonblock(read.as_raw_fd())?;
    let dns = dns.clone();
    std::thread::spawn(move || {
        let entry = dns.lookup(&name);
        std::thread::sleep(entry.delay);
        // The guest may have closed its end already; nobody is waiting then.
        let _ = std::fs::File::from(write).write_all(&reply(&entry, family));
    });
    Ok(read)
}

fn set_nonblock(fd: RawFd) -> Res {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(errno::IO);
        }
    }
    Ok(())
}

/// A connection that has not been decided yet.
pub enum Pending {
    /// Waiting for the resolver's answer on `pipe`.
    Resolving {
        pipe: OwnedFd,
        answer: Vec<u8>,
        port: u16,
    },
    /// A non-blocking connect to one address is in flight; `rest` are the
    /// addresses to try if it fails, and `last` the errno of the last failure.
    Connecting {
        sock: OwnedFd,
        rest: VecDeque<SocketAddr>,
        last: u16,
    },
}

impl Pending {
    pub fn os(&self) -> RawFd {
        match self {
            Pending::Resolving { pipe, .. } => pipe.as_raw_fd(),
            Pending::Connecting { sock, .. } => sock.as_raw_fd(),
        }
    }

    /// What to poll the descriptor for while it waits.
    pub fn poll_events(&self) -> i16 {
        match self {
            Pending::Resolving { .. } => libc::POLLIN,
            Pending::Connecting { .. } => libc::POLLOUT,
        }
    }
}

enum Started {
    Connected(OwnedFd),
    InProgress(OwnedFd),
}

fn start_connect(addr: SocketAddr, stats: &mut Stats) -> Result<Started, u16> {
    let family = if addr.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    let raw = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
    if raw < 0 {
        return Err(map_os(last_os()));
    }
    let sock = unsafe { OwnedFd::from_raw_fd(raw) };
    set_nonblock(raw)?;
    stats.connect_attempts += 1;
    let result = match addr {
        SocketAddr::V4(a) => {
            let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sa.sin_family = libc::AF_INET as _;
            sa.sin_port = a.port().to_be();
            sa.sin_addr.s_addr = u32::from_ne_bytes(a.ip().octets());
            unsafe {
                libc::connect(
                    raw,
                    (&sa as *const libc::sockaddr_in).cast(),
                    std::mem::size_of_val(&sa) as libc::socklen_t,
                )
            }
        }
        SocketAddr::V6(a) => {
            let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sa.sin6_family = libc::AF_INET6 as _;
            sa.sin6_port = a.port().to_be();
            sa.sin6_addr.s6_addr = a.ip().octets();
            unsafe {
                libc::connect(
                    raw,
                    (&sa as *const libc::sockaddr_in6).cast(),
                    std::mem::size_of_val(&sa) as libc::socklen_t,
                )
            }
        }
    };
    if result == 0 {
        return Ok(Started::Connected(sock));
    }
    match last_os() {
        libc::EINPROGRESS => Ok(Started::InProgress(sock)),
        other => Err(map_os(other)),
    }
}

fn last_os() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Try `rest` in order until one connects or is in flight.
fn connect_each(mut rest: VecDeque<SocketAddr>, mut last: u16, stats: &mut Stats) -> Fd {
    while let Some(addr) = rest.pop_front() {
        match start_connect(addr, stats) {
            Ok(Started::Connected(sock)) => return Fd::Stream(sock),
            Ok(Started::InProgress(sock)) => {
                return Fd::Pending(Pending::Connecting { sock, rest, last })
            }
            Err(e) => last = e,
        }
    }
    Fd::Failed(if last == 0 { errno::HOSTUNREACH } else { last })
}

/// Advance a pending descriptor as far as it can go without waiting.
pub fn settle(slot: &mut Fd, stats: &mut Stats) {
    while matches!(slot, Fd::Pending(_)) {
        let Fd::Pending(pending) = std::mem::replace(slot, Fd::Failed(0)) else {
            unreachable!()
        };
        let (next, progressed) = step(pending, stats);
        *slot = next;
        if !progressed {
            return;
        }
    }
}

fn step(pending: Pending, stats: &mut Stats) -> (Fd, bool) {
    match pending {
        Pending::Resolving {
            pipe,
            mut answer,
            port,
        } => {
            let mut done = false;
            loop {
                let mut chunk = [0u8; 256];
                let n =
                    unsafe { libc::read(pipe.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
                if n > 0 {
                    answer.extend_from_slice(&chunk[..n as usize]);
                    continue;
                }
                // End of file, or an error that will not clear.
                match (n, last_os()) {
                    (n, libc::EINTR) if n < 0 => continue,
                    (n, libc::EAGAIN) if n < 0 => {}
                    _ => done = true,
                }
                break;
            }
            if !done {
                return (
                    Fd::Pending(Pending::Resolving { pipe, answer, port }),
                    false,
                );
            }
            let next = match parse_reply(&answer) {
                Ok(ips) if !ips.is_empty() => connect_each(
                    ips.into_iter()
                        .map(|ip| SocketAddr::new(ip, port))
                        .collect(),
                    0,
                    stats,
                ),
                _ => Fd::Failed(LOOKUP_FAILED),
            };
            (next, true)
        }
        Pending::Connecting { sock, rest, last } => {
            let mut p = libc::pollfd {
                fd: sock.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            if unsafe { libc::poll(&mut p, 1, 0) } <= 0 || p.revents == 0 {
                return (Fd::Pending(Pending::Connecting { sock, rest, last }), false);
            }
            let mut error = 0i32;
            let mut len = std::mem::size_of::<i32>() as libc::socklen_t;
            let got = unsafe {
                libc::getsockopt(
                    sock.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut error as *mut i32).cast(),
                    &mut len,
                )
            };
            if got < 0 {
                error = last_os();
            }
            if error == 0 {
                (Fd::Stream(sock), true)
            } else {
                (connect_each(rest, map_os(error), stats), true)
            }
        }
    }
}

/// `host:port`, split at the last colon; an IPv6 literal is bracketed.
fn parse_target(text: &str) -> Res<(Target, u16)> {
    let (host, port) = text.rsplit_once(':').ok_or(errno::INVAL)?;
    let port = port.parse().map_err(|_| errno::INVAL)?;
    if host.is_empty() {
        return Err(errno::INVAL);
    }
    let target = match host.strip_prefix('[') {
        Some(inner) => Target::Ip(IpAddr::V6(
            inner
                .strip_suffix(']')
                .and_then(|v6| v6.parse().ok())
                .ok_or(errno::INVAL)?,
        )),
        None => match host.parse::<std::net::Ipv4Addr>() {
            Ok(v4) => Target::Ip(IpAddr::V4(v4)),
            Err(_) => Target::Name(host.to_string()),
        },
    };
    Ok((target, port))
}

enum Target {
    Ip(IpAddr),
    Name(String),
}

pub fn tcp_connect(
    caller: &mut Caller<'_, crate::preview1::Host>,
    addr: u32,
    len: u32,
    out: u32,
) -> Res {
    let (mem, host) = split(caller)?;
    let range = span(mem, addr, len as usize)?;
    let text = std::str::from_utf8(&mem[range]).map_err(|_| errno::INVAL)?;
    let (target, port) = parse_target(text)?;
    // Check the out-parameter before any work: a descriptor that cannot be
    // reported would leak.
    span(mem, out, 4)?;
    let fd = match target {
        Target::Ip(ip) => connect_each(
            VecDeque::from([SocketAddr::new(ip, port)]),
            0,
            &mut host.stats,
        ),
        Target::Name(name) => Fd::Pending(Pending::Resolving {
            pipe: spawn_resolver(&host.dns, name, 0)?,
            answer: Vec::new(),
            port,
        }),
    };
    let number = host.table.insert(fd);
    host.stats.connects.push(number);
    put_u32(mem, out, number)
}

pub fn sock_error(caller: &mut Caller<'_, crate::preview1::Host>, fd: u32, out: u32) -> Res {
    let (mem, host) = split(caller)?;
    host.stats.sock_errors += 1;
    let slot = host.table.get_mut(fd).ok_or(errno::BADF)?;
    settle(slot, &mut host.stats);
    // The call itself answers EINPROGRESS while the connection is undecided.
    let value = match slot {
        Fd::Pending(_) => return Err(errno::INPROGRESS),
        Fd::Failed(e) => *e,
        Fd::Stream(_) => errno::SUCCESS,
        _ => return Err(errno::NOTSOCK),
    };
    put_u32(mem, out, value as u32)
}

pub fn resolve(
    caller: &mut Caller<'_, crate::preview1::Host>,
    name: u32,
    len: u32,
    family: u32,
    out: u32,
) -> Res {
    let (mem, host) = split(caller)?;
    if !matches!(family, 0 | 4 | 6) {
        return Err(errno::INVAL);
    }
    let range = span(mem, name, len as usize)?;
    let name = std::str::from_utf8(&mem[range]).map_err(|_| errno::INVAL)?;
    span(mem, out, 4)?;
    let pipe = spawn_resolver(&host.dns, name.to_string(), family)?;
    let number = host.table.insert(Fd::Resolve(pipe));
    put_u32(mem, out, number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_splits_at_the_last_colon() {
        let ip = |t| match parse_target(t).unwrap() {
            (Target::Ip(ip), port) => (ip.to_string(), port),
            _ => panic!("{t} is a name"),
        };
        assert_eq!(ip("127.0.0.1:80"), ("127.0.0.1".into(), 80));
        assert_eq!(ip("[::1]:8080"), ("::1".into(), 8080));
        assert_eq!(ip("[fe80::1]:1"), ("fe80::1".into(), 1));
        assert!(matches!(
            parse_target("db.internal:5432").unwrap(),
            (Target::Name(n), 5432) if n == "db.internal"
        ));
        for bad in [
            "nocolon",
            ":80",
            "host:",
            "host:99999",
            "[::1:80",
            "[nope]:80",
        ] {
            assert!(parse_target(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_dns_knob_parses_answers_and_delays() {
        let mut dns = Dns::default();
        dns.add("slow.test=127.0.0.1,delay=400").unwrap();
        dns.add("Multi.Test=127.0.0.2+::1").unwrap();
        dns.add("gone.test=notfound,delay=30").unwrap();
        dns.add("flaky.test=temp").unwrap();
        assert_eq!(dns.lookup("slow.test").delay, Duration::from_millis(400));
        assert_eq!(
            dns.lookup("MULTI.test").answer,
            Answer::Addresses(vec!["127.0.0.2".parse().unwrap(), "::1".parse().unwrap()])
        );
        assert_eq!(dns.lookup("gone.test").answer, Answer::NotFound);
        assert_eq!(dns.lookup("flaky.test").answer, Answer::Temporary);
        // Not in the table: a literal is itself, localhost is loopback, the rest is unknown.
        assert_eq!(
            dns.lookup("10.1.2.3").answer,
            Answer::Addresses(vec!["10.1.2.3".parse().unwrap()])
        );
        assert_eq!(dns.lookup("elsewhere.test").answer, Answer::NotFound);
        for bad in [
            "noequals",
            "x=",
            "x=1.2.3.4,delay=soon",
            "x=1.2.3.4,wait=1",
            "=1.2.3.4",
        ] {
            assert!(Dns::default().add(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_answer_is_a_status_byte_and_seventeen_byte_records() {
        let both = Entry {
            answer: Answer::Addresses(vec!["1.2.3.4".parse().unwrap(), "::1".parse().unwrap()]),
            delay: Duration::ZERO,
        };
        let any = reply(&both, 0);
        assert_eq!(any.len(), 1 + 2 * RECORD);
        assert_eq!(&any[..6], [0, 4, 1, 2, 3, 4]);
        assert!(any[6..18].iter().all(|b| *b == 0));
        assert_eq!(any[18], 6);
        assert_eq!(any[18 + 16], 1);
        assert_eq!(reply(&both, 6).len(), 1 + RECORD);
        assert_eq!(parse_reply(&any).unwrap().len(), 2);
        // Nothing of the requested family is not a found name.
        let v4 = Entry {
            answer: Answer::Addresses(vec!["1.2.3.4".parse().unwrap()]),
            delay: Duration::ZERO,
        };
        assert_eq!(reply(&v4, 6), [STATUS_NOT_FOUND]);
        assert!(parse_reply(&reply(&v4, 6)).is_err());
        let temp = Entry {
            answer: Answer::Temporary,
            delay: Duration::ZERO,
        };
        assert_eq!(reply(&temp, 0), [STATUS_TEMPORARY]);
    }
}
