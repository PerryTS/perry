//! P2 acceptance: real descriptors, real bytes, on the real driver.
//!
//! Every test asserts its *subject* ran rather than that nothing threw (DESIGN
//! §11, and CLAUDE.md's "four ways a gate can be unable to fail"): each byte
//! assertion is paired with an event-kind assertion, and every fixture checks
//! [`live_handles`] so a run in which no descriptor was ever adopted cannot
//! pass. Nothing here is mocked — the pipes come from `pipe(2)`, the sockets
//! are bound UDP sockets on loopback, and the completions come out of
//! `Loop::turn`.

use std::cell::RefCell;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use super::*;

/// One recorded event, owned.
#[derive(Clone, Debug)]
pub(super) enum Rec {
    Data(u64, Vec<u8>),
    Datagram(u64, Vec<u8>, SocketAddr),
    Eof(u64),
    Wrote(u64, u64, usize),
    Closed(u64),
    Error(u64, u64, &'static str),
}

thread_local! {
    static EVENTS: RefCell<Vec<Rec>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn record(id: u64, event: StreamEvent) {
    EVENTS.with(|events| {
        events.borrow_mut().push(match event {
            StreamEvent::Data(bytes) => Rec::Data(id, bytes),
            StreamEvent::Datagram { bytes, from } => Rec::Datagram(id, bytes, from),
            StreamEvent::Eof => Rec::Eof(id),
            StreamEvent::Wrote { user, len } => Rec::Wrote(id, user, len),
            StreamEvent::Closed => Rec::Closed(id),
            StreamEvent::Error { user, error, .. } => Rec::Error(id, user, error.code),
            StreamEvent::Signal => Rec::Eof(id),
        })
    });
}

struct Fixture;

impl Fixture {
    fn start() -> Self {
        assert!(
            crate::event_pump::install_net_loop_for_test(),
            "the host must provide a turnloop loop, or every assertion below is vacuous"
        );
        EVENTS.with(|events| events.borrow_mut().clear());
        super::reset_for_test();
        Fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        crate::event_pump::reset_net_loop_for_test();
        EVENTS.with(|events| events.borrow_mut().clear());
    }
}

fn pump() {
    crate::event_pump::pump_net_for_test(Duration::from_millis(5));
}

/// Pump until `want` holds or the budget runs out. Returns whether it held, so
/// a test asserts on it instead of timing out silently.
fn pump_until(want: impl Fn(&[Rec]) -> bool) -> bool {
    let limit = Instant::now() + Duration::from_secs(5);
    loop {
        if EVENTS.with(|events| want(&events.borrow())) {
            return true;
        }
        if Instant::now() >= limit {
            return false;
        }
        pump();
    }
}

fn events() -> Vec<Rec> {
    EVENTS.with(|events| events.borrow().clone())
}

fn data_for(id: u64) -> Vec<u8> {
    events()
        .iter()
        .filter_map(|e| match e {
            Rec::Data(at, bytes) if *at == id => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn datagrams(id: u64) -> Vec<(Vec<u8>, SocketAddr)> {
    events()
        .iter()
        .filter_map(|e| match e {
            Rec::Datagram(at, bytes, from) if *at == id => Some((bytes.clone(), *from)),
            _ => None,
        })
        .collect()
}

fn writes(id: u64) -> Vec<(u64, usize)> {
    events()
        .iter()
        .filter_map(|e| match e {
            Rec::Wrote(at, user, len) if *at == id => Some((*user, *len)),
            _ => None,
        })
        .collect()
}

fn closes(id: u64) -> usize {
    events()
        .iter()
        .filter(|e| matches!(e, Rec::Closed(at) if *at == id))
        .count()
}

// ── Adoption helpers ────────────────────────────────────────────────────────

#[cfg(unix)]
fn os_pipe() -> (std::os::fd::OwnedFd, std::os::fd::OwnedFd) {
    use std::os::fd::FromRawFd;
    let mut fds = [0i32; 2];
    // SAFETY: `fds` is writable output storage of the required length.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe(2)");
    // SAFETY: both descriptors are fresh and owned by this call.
    unsafe {
        (
            std::os::fd::OwnedFd::from_raw_fd(fds[0]),
            std::os::fd::OwnedFd::from_raw_fd(fds[1]),
        )
    }
}

#[cfg(unix)]
fn adopt_fd(fd: std::os::fd::OwnedFd) -> u64 {
    super::adopt_stream(super::adopt::Transport::Fd(fd), Owner::Test)
        .expect("adopting a live descriptor must succeed")
}

#[cfg(unix)]
fn adopt_udp(udp: &UdpSocket) -> u64 {
    use std::os::fd::AsFd;
    let transport =
        super::adopt::duplicate_fd(udp.as_fd()).expect("duplicating a bound socket must succeed");
    super::adopt_stream(transport, Owner::Test).expect("adopting a bound UDP socket must succeed")
}

// ── The token space ─────────────────────────────────────────────────────────

#[test]
fn the_two_token_spaces_do_not_overlap() {
    // P1's classes are 1..=7; a completion carrying one of those must never be
    // routed here, and vice versa. This is the entire routing contract, so it
    // is tested directly rather than inferred from a passing workload.
    for p1_class in 1u64..=7 {
        let t = Token((p1_class << ID_BITS) | 42);
        assert!(!owns(t), "class {p1_class} belongs to turnloop_net");
    }
    for class in [OP_READ, OP_CLOSE, OP_RECV, OP_SEND, OP_SIGNAL] {
        let t = token(class, 42);
        assert!(owns(t), "class {class:#x} belongs to turnloop_proc");
        assert_eq!(
            token_parts(t),
            (class, 42),
            "the id survives the round trip"
        );
    }
}

#[test]
fn a_submission_for_an_unknown_id_is_refused() {
    let _fixture = Fixture::start();
    let err = super::send_to(9_999_999, b"x".to_vec(), None, 0).expect_err("no such descriptor");
    assert_eq!(err.code, "ENOENT");
    assert_eq!(err.syscall, "send");
    assert_eq!(super::live_handles(), 0, "a refusal registers nothing");
}

// ── Streams ─────────────────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn a_pipe_streams_its_bytes_and_then_its_eof() {
    use std::io::Write;
    let _fixture = Fixture::start();
    let (read_end, write_end) = os_pipe();
    let id = adopt_fd(read_end);
    assert_eq!(
        super::live_handles(),
        1,
        "the descriptor really was adopted"
    );
    super::read_start(id).expect("read_start");

    let mut writer = std::fs::File::from(write_end);
    writer.write_all(b"hello from the child").expect("write");
    assert!(
        pump_until(|events| events
            .iter()
            .any(|e| matches!(e, Rec::Data(at, _) if *at == id))),
        "the driver must deliver the bytes"
    );
    assert_eq!(data_for(id), b"hello from the child");

    // Closing the writer is the child exiting: the read must end in EOF, which
    // is what the deleted reader thread's `Ok(0)` arm produced.
    drop(writer);
    assert!(
        pump_until(|events| events
            .iter()
            .any(|e| matches!(e, Rec::Eof(at) if *at == id))),
        "closing the write end must produce EOF"
    );
}

#[cfg(unix)]
#[test]
fn close_releases_the_entry_exactly_once() {
    let _fixture = Fixture::start();
    let (read_end, _write_end) = os_pipe();
    let id = adopt_fd(read_end);
    super::read_start(id).expect("read_start");
    assert_eq!(super::live_handles(), 1);

    super::close(id);
    // A second close must not produce a second release (DESIGN D4).
    super::close(id);
    assert!(pump_until(|_| closes(id) >= 1), "close must be reported");
    for _ in 0..10 {
        pump();
    }
    assert_eq!(closes(id), 1, "exactly one Closed, never two");
    assert_eq!(super::live_handles(), 0, "the entry is gone after Closed");
}

// ── Datagrams ───────────────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn a_udp_round_trip_carries_bytes_and_the_source_endpoint() {
    let _fixture = Fixture::start();
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind receiver");
    let sender = UdpSocket::bind("127.0.0.1:0").expect("bind sender");
    let receiver_addr = receiver.local_addr().expect("local_addr");
    let sender_addr = sender.local_addr().expect("local_addr");

    let rx = adopt_udp(&receiver);
    let tx = adopt_udp(&sender);
    assert_eq!(super::live_handles(), 2, "both sockets really were adopted");
    super::recv_start(rx).expect("recv_start");

    assert_eq!(
        super::send_to(tx, b"ping".to_vec(), Some(receiver_addr), 7).expect("send_to"),
        4
    );
    assert!(
        pump_until(|_| !datagrams(rx).is_empty()),
        "the datagram must be delivered"
    );
    let got = datagrams(rx);
    assert_eq!(got[0].0, b"ping", "the payload survives the pooled lease");
    assert_eq!(
        got[0].1, sender_addr,
        "rinfo's address/port come from the completion, not from a guess"
    );
    assert!(
        writes(tx).iter().any(|w| w.0 == 7),
        "the send's own completion must carry the caller's token back"
    );
}

#[cfg(unix)]
#[test]
fn a_datagram_receive_rearms_itself_until_it_is_closed() {
    let _fixture = Fixture::start();
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind receiver");
    let sender = UdpSocket::bind("127.0.0.1:0").expect("bind sender");
    let receiver_addr = receiver.local_addr().expect("local_addr");

    let rx = adopt_udp(&receiver);
    super::recv_start(rx).expect("recv_start");

    // turnloop's UDP receive is single-shot, so "keeps receiving" is a
    // property of this module's rearm, not of the driver. Three datagrams is
    // the smallest count that can distinguish "rearmed" from "delivered the
    // first one and stopped".
    for n in 0..3u8 {
        sender
            .send_to(&[b'a' + n], receiver_addr)
            .expect("send from a plain socket");
    }
    assert!(
        pump_until(|_| datagrams(rx).len() >= 3),
        "every datagram must arrive, which requires the receive to rearm"
    );
    let payloads: Vec<u8> = datagrams(rx).iter().map(|d| d.0[0]).collect();
    assert_eq!(payloads, vec![b'a', b'b', b'c'], "in order");
}

#[cfg(unix)]
#[test]
fn the_retained_duplicate_names_the_same_socket_as_the_adopted_one() {
    // The whole dgram design rests on this: `setsockopt`/`getsockname` through
    // the copy Perry keeps must affect and describe the socket the driver
    // received on. If `dup` ever stopped sharing the open file description,
    // every multicast option would silently apply to nothing.
    let _fixture = Fixture::start();
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
    let bound = socket.local_addr().expect("local_addr");
    let id = adopt_udp(&socket);
    assert_eq!(super::live_handles(), 1);

    socket.set_broadcast(true).expect("setsockopt via the copy");
    assert!(socket.broadcast().expect("getsockopt via the copy"));
    assert_eq!(
        socket.local_addr().expect("getsockname via the copy"),
        bound,
        "the retained copy still names the same binding"
    );

    super::recv_start(id).expect("recv_start");
    let sender = UdpSocket::bind("127.0.0.1:0").expect("bind sender");
    sender.send_to(b"z", bound).expect("send");
    assert!(
        pump_until(|_| !datagrams(id).is_empty()),
        "and the driver is receiving on that same binding"
    );
}

#[cfg(unix)]
#[test]
fn queued_sends_complete_in_order_and_drain_the_queued_count() {
    // Three datagrams submitted before a single turn. The ordering and the
    // running queued-byte count are this module's bookkeeping, not the
    // driver's, and `send()`'s Node-visible completion order depends on both.
    let _fixture = Fixture::start();
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind receiver");
    let sender = UdpSocket::bind("127.0.0.1:0").expect("bind sender");
    let to = receiver.local_addr().expect("local_addr");
    let tx = adopt_udp(&sender);
    let rx = adopt_udp(&receiver);
    assert_eq!(super::live_handles(), 2);
    super::recv_start(rx).expect("recv_start");

    assert_eq!(
        super::send_to(tx, b"one".to_vec(), Some(to), 11).expect("send"),
        3,
        "queued bytes accumulate across submissions"
    );
    assert_eq!(
        super::send_to(tx, b"two".to_vec(), Some(to), 22).expect("send"),
        6
    );
    assert_eq!(
        super::send_to(tx, b"three".to_vec(), Some(to), 33).expect("send"),
        11
    );

    assert!(pump_until(|_| writes(tx).len() == 3), "all three complete");
    assert_eq!(
        writes(tx).iter().map(|w| w.0).collect::<Vec<_>>(),
        vec![11, 22, 33],
        "completions arrive in submission order, carrying each caller token"
    );
    assert!(
        pump_until(|_| datagrams(rx).len() == 3),
        "and every datagram reached the peer"
    );
    let payloads: Vec<Vec<u8>> = datagrams(rx).into_iter().map(|d| d.0).collect();
    assert_eq!(
        payloads,
        vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()]
    );
}

#[cfg(unix)]
#[test]
fn a_failed_send_names_the_caller_token_and_nodes_error_code() {
    // The reporting contract `socket.send(msg, cb)` depends on: a failure has
    // to reach the *right* callback, which means the completion must carry the
    // submitting token rather than merely saying something broke. A datagram
    // past the maximum UDP payload is the failure every host agrees on.
    let _fixture = Fixture::start();
    let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind receiver");
    let to = receiver.local_addr().expect("local_addr");
    let id = adopt_udp(&sender);
    assert_eq!(super::live_handles(), 1);

    super::send_to(id, vec![0u8; 70_000], Some(to), 4242)
        .expect("submission succeeds; the failure arrives as a completion");
    assert!(
        pump_until(|events| events
            .iter()
            .any(|e| matches!(e, Rec::Error(at, _, _) if *at == id))),
        "an oversized datagram must fail"
    );
    let failure = events()
        .into_iter()
        .find_map(|e| match e {
            Rec::Error(at, user, code) if at == id => Some((user, code)),
            _ => None,
        })
        .expect("one error");
    assert_eq!(failure.0, 4242, "the caller's token comes back");
    assert_eq!(
        failure.1, "EMSGSIZE",
        "with Node's code for an oversized datagram"
    );
}

/// turnloop P9: two agents' ids are disjoint by construction.
///
/// The entry tables are thread-local, so before P9 — one loop, one minter — a
/// per-thread counter from 1 was enough. Now every JS agent can own a loop,
/// and two agents counting from 1 would both own an id `1`: same-thread
/// lookups would each find their own entry, and a lookup that crossed agents
/// would find the *wrong* entry rather than none. Banding the id by agent
/// turns that silent misroute into a guaranteed miss, which the caller sees as
/// an error.
///
/// Asserted rather than commented, because the property is invisible in normal
/// operation — it only shows up the one time something crosses.
#[test]
fn agent_id_bands_do_not_overlap() {
    fn mint_three() -> Vec<u64> {
        PROC.with(|state| {
            let mut state = state.borrow_mut();
            (0..3).map(|_| mint_id(&mut state)).collect()
        })
    }

    // The primary agent keeps the band it always had, so nothing about a
    // single-agent program moves.
    let primary = std::thread::spawn(mint_three).join().unwrap();
    assert_eq!(primary, vec![1, 2, 3], "the primary agent's ids moved");

    let (a, b) = (
        std::thread::spawn(|| {
            let id = crate::agent::enter_worker_agent();
            let ids = mint_three();
            crate::agent::retire_agent(id);
            ids
        })
        .join()
        .unwrap(),
        std::thread::spawn(|| {
            let id = crate::agent::enter_worker_agent();
            let ids = mint_three();
            crate::agent::retire_agent(id);
            ids
        })
        .join()
        .unwrap(),
    );

    for ids in [&a, &b] {
        for id in ids {
            assert!(*id > 0 && *id <= ID_MASK, "id {id} left the token field");
        }
    }
    for id in &a {
        assert!(!b.contains(id), "id {id} was minted by two agents");
        assert!(
            !primary.contains(id),
            "id {id} collides with the primary agent"
        );
    }
}
