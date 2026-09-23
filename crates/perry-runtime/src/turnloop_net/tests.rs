//! P1 acceptance: real bytes over real sockets, on the real driver.
//!
//! Every test here asserts its *subject* ran, not merely that nothing threw
//! (DESIGN §11, and the "four ways a gate can be unable to fail" rule in
//! CLAUDE.md): a byte assertion is paired with a completion-kind assertion, and
//! the loopback tests check `live_handles()` so a run in which no socket was
//! ever created cannot pass.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::sink::{
    NetCompletion, NET_ACCEPT, NET_CLOSED, NET_CONNECT, NET_DATA, NET_EOF, NET_ERROR, NET_SHUTDOWN,
    NET_TIMER, NET_WROTE,
};
use super::*;

/// One recorded completion, owned (the ABI struct borrows its payload).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    kind: i32,
    id: i64,
    conn: i64,
    user: u64,
    len: usize,
    queued: usize,
    errno: i32,
    data: Vec<u8>,
    code: Option<String>,
    syscall: Option<String>,
}

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(1000) };
}

extern "C" fn test_sink(completion: *const NetCompletion) {
    // SAFETY: `dispatch` passes a live completion for the duration of the call.
    let c = unsafe { &*completion };
    // SAFETY: same call, and the payload pointers are valid for it.
    let (data, code, syscall) = unsafe {
        (
            c.bytes().to_vec(),
            c.code_str().map(str::to_string),
            c.syscall_str().map(str::to_string),
        )
    };
    EVENTS.with(|events| {
        events.borrow_mut().push(Event {
            kind: c.kind,
            id: c.id,
            conn: c.conn,
            user: c.user,
            len: c.len,
            queued: c.queued,
            errno: c.errno,
            data,
            code,
            syscall,
        })
    });
}

extern "C" fn test_alloc_id() -> i64 {
    NEXT_ID.with(|n| {
        let id = n.get() + 1;
        n.set(id);
        id
    })
}

const SUBSYSTEM: u8 = 3;

struct Fixture;

impl Fixture {
    fn start() -> Self {
        assert!(
            crate::event_pump::install_net_loop_for_test(),
            "the host must provide a turnloop loop for the P1 tests"
        );
        assert!(
            super::register_sink(SUBSYSTEM, test_sink, test_alloc_id),
            "sink registration must succeed, or every assertion below is vacuous"
        );
        EVENTS.with(|events| events.borrow_mut().clear());
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

/// Pump until `want` is satisfied or the budget runs out. Returns whether it
/// was satisfied, so a test can assert on it rather than time out silently.
fn pump_until(want: impl Fn(&[Event]) -> bool) -> bool {
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

fn events() -> Vec<Event> {
    EVENTS.with(|events| events.borrow().clone())
}

fn count(kind: i32, id: i64) -> usize {
    events()
        .iter()
        .filter(|e| e.kind == kind && e.id == id)
        .count()
}

fn payload(kind: i32, id: i64) -> Vec<u8> {
    events()
        .iter()
        .filter(|e| e.kind == kind && e.id == id)
        .flat_map(|e| e.data.clone())
        .collect()
}

fn accepted_id(server: i64) -> Option<i64> {
    events()
        .iter()
        .find(|e| e.kind == NET_ACCEPT && e.id == server)
        .map(|e| e.conn)
}

fn listen_local() -> (i64, SocketAddr) {
    let server = 1;
    let local = super::tcp_listen(
        server,
        SUBSYSTEM,
        "127.0.0.1:0".parse().unwrap(),
        128,
        false,
        true,
    )
    .expect("bind an ephemeral loopback port");
    assert_ne!(local.port(), 0, "listen(0) must report its real port");
    super::accept_start(server).expect("multishot accept");
    (server, local)
}

#[test]
fn tokens_round_trip_operation_class_and_id() {
    for id in [1i64, 2, 4095, (1i64 << 40) - 1, (1i64 << 56) - 1] {
        for op in [
            OP_ACCEPT,
            OP_READ,
            OP_WRITE,
            OP_SHUTDOWN,
            OP_CONNECT,
            OP_CLOSE,
        ] {
            let (back_op, back_id) = token_parts(token(op, id));
            assert_eq!((back_op, back_id), (op, id), "token({op}, {id})");
        }
    }
}

#[test]
fn a_full_loopback_exchange_moves_real_bytes_both_ways() {
    let _fixture = Fixture::start();
    let (server, local) = listen_local();
    assert_eq!(super::live_handles(), 1, "the listener is on the loop");

    let client = 2;
    super::tcp_connect(client, SUBSYSTEM, local, true).expect("connect");
    assert!(
        pump_until(
            |e| e.iter().any(|e| e.kind == NET_CONNECT && e.id == client)
                && e.iter().any(|e| e.kind == NET_ACCEPT && e.id == server)
        ),
        "connect and accept must both complete: {:?}",
        events()
    );
    let conn = accepted_id(server).expect("accept reported a connection id");
    assert_eq!(
        super::live_handles(),
        3,
        "listener + client + accepted connection"
    );
    // `socket.localAddress` / `remoteAddress` on an accepted connection read
    // these, and a binding that got `None` here would report `undefined` for
    // every accepted socket — which is exactly what the first draft did.
    let conn_local = super::local_addr(conn).expect("accepted socket has a local endpoint");
    let conn_peer = super::peer_addr(conn).expect("accepted socket has a peer endpoint");
    assert_eq!(
        conn_local.port(),
        local.port(),
        "accepted on the bound port"
    );
    assert!(conn_peer.ip().is_loopback(), "peer is the loopback client");
    assert_eq!(
        super::peer_addr(client).map(|a| a.port()),
        Some(local.port()),
        "the client's peer is the listener"
    );

    super::read_start(client).expect("client read");
    super::read_start(conn).expect("server read");

    // Client → server.
    let queued = super::write(client, b"ping".to_vec(), 7).expect("client write");
    assert_eq!(queued, 4, "the write is queued until the driver reports it");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_DATA && e.id == conn)),
        "the server side must receive the bytes: {:?}",
        events()
    );
    assert_eq!(payload(NET_DATA, conn), b"ping");

    let wrote = events()
        .into_iter()
        .find(|e| e.kind == NET_WROTE && e.id == client)
        .expect("the write completed");
    assert_eq!(
        wrote.user, 7,
        "the caller's completion token is echoed back"
    );
    assert_eq!(wrote.len, 4);
    assert_eq!(wrote.queued, 0, "the write drained");
    assert_eq!(super::queued_bytes(client), 0);

    // Server → client.
    super::write(conn, b"pong".to_vec(), 0).expect("server write");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_DATA && e.id == client)),
        "the client must receive the reply: {:?}",
        events()
    );
    assert_eq!(payload(NET_DATA, client), b"pong");

    super::close(client).expect("close client");
    super::close(conn).expect("close connection");
    super::close(server).expect("close listener");
    assert!(
        pump_until(|e| e.iter().filter(|e| e.kind == NET_CLOSED).count() == 3),
        "every handle must report its terminal Closed: {:?}",
        events()
    );
    assert_eq!(
        super::live_handles(),
        0,
        "the entry is released on Closed, not before"
    );
}

#[test]
fn half_close_ends_the_write_side_and_the_peer_sees_eof() {
    let _fixture = Fixture::start();
    let (server, local) = listen_local();
    let client = 2;
    super::tcp_connect(client, SUBSYSTEM, local, true).expect("connect");
    assert!(pump_until(|e| e.iter().any(|e| e.kind == NET_ACCEPT)));
    let conn = accepted_id(server).expect("connection id");
    super::read_start(conn).expect("server read");
    super::read_start(client).expect("client read");

    // A queued write followed by end(): turnloop orders the shutdown after the
    // write, so the peer must see the bytes AND THEN the EOF, never a
    // truncated stream.
    super::write(client, b"last".to_vec(), 0).expect("write");
    super::shutdown(client, 42).expect("end");

    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_EOF && e.id == conn)),
        "the peer must observe the half-close: {:?}",
        events()
    );
    assert_eq!(
        payload(NET_DATA, conn),
        b"last",
        "the queued write must precede the FIN"
    );
    let shutdown = events()
        .into_iter()
        .find(|e| e.kind == NET_SHUTDOWN && e.id == client)
        .expect("the shutdown completed");
    assert_eq!(shutdown.user, 42, "end()'s callback token is echoed back");

    // Half-close is half: the read side of the client still works.
    super::write(conn, b"reply".to_vec(), 0).expect("server can still write");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_DATA && e.id == client)),
        "the client's read side survives its own half-close: {:?}",
        events()
    );
    assert_eq!(payload(NET_DATA, client), b"reply");

    for id in [client, conn, server] {
        let _ = super::close(id);
    }
    pump_until(|e| e.iter().filter(|e| e.kind == NET_CLOSED).count() == 3);
}

#[test]
fn queued_writes_report_backpressure_and_drain_in_order() {
    let _fixture = Fixture::start();
    let (server, local) = listen_local();
    let client = 2;
    super::tcp_connect(client, SUBSYSTEM, local, true).expect("connect");
    // Wait for the SERVER's accept, not just the client's connect: they are two
    // independent completions with nothing ordering them, and the next line
    // needs the accepted id. Waiting on the connect alone made this test fail
    // intermittently on a loaded machine — the accept simply landed a turn
    // later and `accepted_id` returned None. Same shape as the UDS test below.
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_ACCEPT && e.id == server)
            && e.iter().any(|e| e.kind == NET_CONNECT && e.id == client)),
        "the connection must establish on both ends: {:?}",
        events()
    );
    let conn = accepted_id(server).expect("connection id");
    super::read_start(conn).expect("server read");

    // Submit several writes before turning once, so they are genuinely queued
    // rather than completing one at a time.
    let mut queued = 0;
    for (i, chunk) in [b"aaa".as_slice(), b"bb", b"c"].iter().enumerate() {
        queued = super::write(client, chunk.to_vec(), i as u64 + 1).expect("write");
    }
    assert_eq!(queued, 6, "every queued byte counts toward writableLength");
    assert_eq!(super::queued_bytes(client), 6);

    assert!(
        pump_until(|e| e.iter().filter(|e| e.kind == NET_WROTE).count() == 3),
        "all three writes must complete: {:?}",
        events()
    );
    let tokens: Vec<u64> = events()
        .iter()
        .filter(|e| e.kind == NET_WROTE)
        .map(|e| e.user)
        .collect();
    assert_eq!(
        tokens,
        vec![1, 2, 3],
        "write completions are reported in submission order"
    );
    assert_eq!(super::queued_bytes(client), 0, "the queue drained");

    assert!(pump_until(|e| e
        .iter()
        .any(|e| e.kind == NET_DATA && e.id == conn)));
    let received = payload(NET_DATA, conn);
    assert_eq!(
        received, b"aaabbc",
        "ordered writes arrive as one ordered stream"
    );

    for id in [client, conn, server] {
        let _ = super::close(id);
    }
    pump_until(|e| e.iter().filter(|e| e.kind == NET_CLOSED).count() == 3);
}

#[test]
fn a_refused_connect_reports_nodes_code_errno_and_syscall() {
    let _fixture = Fixture::start();
    // Bind and immediately close, so the port is almost certainly unused.
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe bind");
    let dead: SocketAddr = probe.local_addr().expect("probe addr");
    drop(probe);

    let client = 5;
    super::tcp_connect(client, SUBSYSTEM, dead, false).expect("submit connect");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_ERROR && e.id == client)),
        "a connect to a dead port must fail: {:?}",
        events()
    );
    let err = events()
        .into_iter()
        .find(|e| e.kind == NET_ERROR && e.id == client)
        .expect("the error completion");
    assert_eq!(err.syscall.as_deref(), Some("connect"));
    assert_eq!(
        err.code.as_deref(),
        Some("ECONNREFUSED"),
        "Node reports the OS code, not a portable category"
    );
    assert!(err.errno < 0, "libuv reports errno negated: {}", err.errno);
    let _ = super::close(client);
    pump();
}

/// `net.connect(port, 'localhost')` against an IPv4-only listener.
///
/// On a dual-stack host `localhost` resolves to `::1` *and* `127.0.0.1`, and
/// the resolver usually returns the v6 address first — so this only passes if
/// a refused first attempt falls through to the next address (Node's
/// `autoSelectFamily`). Written as a regression: the single-address version
/// of this code failed here with ECONNREFUSED, and it is the single most
/// common connect form in the net corpus.
#[test]
fn a_hostname_connect_resolves_and_falls_through_to_a_reachable_family() {
    let _fixture = Fixture::start();
    let (server, local) = listen_local();
    let client = 2;
    // "localhost" is not an IP literal, so this goes through Loop::resolve.
    super::tcp_connect_host(client, SUBSYSTEM, "localhost", local.port(), false)
        .expect("submit resolve+connect");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_CONNECT && e.id == client)),
        "a name must resolve and connect: {:?}",
        events()
    );
    assert!(
        super::peer_addr(client).is_some(),
        "the resolved peer is recorded"
    );
    assert_eq!(
        events().iter().filter(|e| e.kind == NET_ERROR).count(),
        0,
        "an abandoned address attempt must not reach the binding: {:?}",
        events()
    );
    assert_eq!(
        events().iter().filter(|e| e.kind == NET_CLOSED).count(),
        0,
        "nor must the close that retires it: {:?}",
        events()
    );
    for id in [client, server] {
        let _ = super::close(id);
    }
    pump_until(|e| e.iter().filter(|e| e.kind == NET_CLOSED).count() == 2);
}

#[test]
fn an_unresolvable_hostname_reports_enotfound_on_getaddrinfo() {
    let _fixture = Fixture::start();
    let client = 9;
    super::tcp_connect_host(client, SUBSYSTEM, "perry-turnloop-p1.invalid", 80, false)
        .expect("submit resolve");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_ERROR && e.id == client)),
        "a bogus name must fail: {:?}",
        events()
    );
    let err = events()
        .into_iter()
        .find(|e| e.kind == NET_ERROR && e.id == client)
        .expect("the error completion");
    assert_eq!(err.code.as_deref(), Some("ENOTFOUND"));
    assert_eq!(err.syscall.as_deref(), Some("getaddrinfo"));
    assert_eq!(
        super::live_handles(),
        0,
        "a failed resolve leaves no pending connect behind"
    );
}

#[cfg(unix)]
#[test]
fn unix_domain_sockets_carry_the_same_lifecycle() {
    let _fixture = Fixture::start();
    let dir = std::env::temp_dir().join(format!(
        "perry-p1-uds-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("s.sock");
    let _ = std::fs::remove_file(&path);

    let server = 1;
    super::pipe_listen(server, SUBSYSTEM, &path, 128).expect("bind the socket path");
    super::accept_start(server).expect("accept");

    let client = 2;
    super::pipe_connect(client, SUBSYSTEM, &path).expect("connect");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_ACCEPT && e.id == server)
            && e.iter().any(|e| e.kind == NET_CONNECT && e.id == client)),
        "a UDS connection must establish: {:?}",
        events()
    );
    let conn = accepted_id(server).expect("connection id");
    super::read_start(conn).expect("read");
    super::write(client, b"unix".to_vec(), 0).expect("write");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_DATA && e.id == conn)),
        "bytes must cross the UDS: {:?}",
        events()
    );
    assert_eq!(payload(NET_DATA, conn), b"unix");

    for id in [client, conn, server] {
        let _ = super::close(id);
    }
    pump_until(|e| e.iter().filter(|e| e.kind == NET_CLOSED).count() == 3);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn closing_a_socket_with_queued_writes_clears_its_write_accounting() {
    let _fixture = Fixture::start();
    let (server, local) = listen_local();
    let client = 2;
    super::tcp_connect(client, SUBSYSTEM, local, true).expect("connect");
    assert!(pump_until(|e| e.iter().any(|e| e.kind == NET_CONNECT)));

    super::write(client, vec![0u8; 8], 1).expect("write");
    super::close(client).expect("close");
    // The close cancels the write; the entry must still disappear exactly once,
    // on Closed, with no leaked queued-byte accounting behind it.
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_CLOSED && e.id == client)),
        "close must terminate: {:?}",
        events()
    );
    assert_eq!(count(NET_CLOSED, client), 1, "Closed arrives exactly once");
    assert_eq!(super::queued_bytes(client), 0);
    assert!(!super::is_live(client));

    let conn = accepted_id(server);
    for id in conn.into_iter().chain([server]) {
        let _ = super::close(id);
    }
    pump();
}

#[test]
fn submissions_for_an_unknown_id_are_rejected_not_ignored() {
    let _fixture = Fixture::start();
    let err = super::write(4242, b"x".to_vec(), 0).expect_err("no such socket");
    assert_eq!(err.code, "ENOENT");
    assert_eq!(err.syscall, "write");
    assert!(super::read_start(4242).is_err());
    assert!(super::close(4242).is_err());
}

/// The bug this pins: `perry-ext-http` passed `server.noDelay` into
/// `tcp_listen`'s `reuse_port` position. `noDelay` defaults to true, so every
/// turnloop HTTP and HTTPS listener bound with `SO_REUSEPORT` — a duplicate
/// `listen()` silently succeeded where Node answers EADDRINUSE — and
/// `TCP_NODELAY` was never applied to an accepted connection, leaving the
/// turnloop transport as the only one serving HTTP with Nagle on.
///
/// Neither symptom is visible at the call site, and no existing test could see
/// either, because both live in options handed to the OS. This asserts the one
/// thing that was actually wrong: which argument lands in which field.
#[test]
fn listen_opts_put_each_argument_in_its_own_field() {
    let server = super::listen_opts(511, false, true);
    assert_eq!(
        server.reuse_port,
        turnloop::ReusePort::No,
        "a plain server listener must not set SO_REUSEPORT"
    );
    assert!(
        server.accept_defaults.nodelay,
        "noDelay must reach TCP_NODELAY on every accepted connection"
    );
    assert_eq!(server.backlog, 511);

    // The two are independent, in both directions.
    let cluster = super::listen_opts(128, true, false);
    // Share, NOT Distribute. turnloop 0.1.0-alpha.6 split the old bool into
    // three, and Distribute — permit the duplicate bind AND spread connections
    // across listeners — is `Unsupported` on macOS, the BSDs, Windows, WASI and
    // the web. `Share` is what this bool has always meant and what
    // perry-ext-http's cluster_bind.rs sets by hand, so it is what preserves
    // behaviour; a caller that wants kernel balancing must ask for it
    // deliberately, on a platform that has it.
    assert_eq!(cluster.reuse_port, turnloop::ReusePort::Share);
    assert_ne!(
        cluster.reuse_port,
        turnloop::ReusePort::Distribute,
        "the bool must not silently become the kernel-balanced variant"
    );
    assert!(!cluster.accept_defaults.nodelay);
    assert_eq!(cluster.backlog, 128);

    // Nothing else is turned on behind the caller's back.
    assert!(
        server.accept_defaults.keep_alive.is_none(),
        "keep-alive is not wired on either transport; do not invent a default"
    );
}

/// A second binding that lands on an occupied slot must be REFUSED, not
/// silently swapped in.
///
/// Until this check existed, `register_sink` stored and returned `true` to
/// both. Both bindings then believed they were registered — `available()` was
/// true for each — while every completion for that slot went to whichever
/// registered last, which reads the token's low bits as one of its OWN
/// connection ids. That is exactly what three colliding slot pairs did
/// (`perry-ext-fastify` with `perry-ext-mysql2` on 4, and two more), reachable
/// by any program linking both bindings.
///
/// Refusing makes `available()` false for the loser, so it keeps its fallback
/// transport instead of corrupting the winner's table.
#[test]
fn a_second_sink_on_an_occupied_slot_is_refused_not_swapped_in() {
    extern "C" fn other_sink(_completion: *const super::NetCompletion) {
        unreachable!("the refused sink must never be routed to");
    }
    extern "C" fn other_alloc() -> i64 {
        unreachable!("the refused allocator must never be called");
    }

    // A slot no other test uses, so this cannot race the shared fixture.
    const SLOT: u8 = 13;
    assert!(
        super::register_sink(SLOT, test_sink, test_alloc_id),
        "the first registration must succeed, or the rest is vacuous"
    );
    assert!(
        !super::register_sink(SLOT, other_sink, other_alloc),
        "a DIFFERENT sink on an occupied slot must be refused"
    );
    // Re-registering the SAME sink stays idempotent: that is the documented
    // contract, and a binding whose module initialises twice relies on it.
    assert!(
        super::register_sink(SLOT, test_sink, test_alloc_id),
        "re-registering the same sink must remain idempotent"
    );
    // Out of range is still refused, rather than writing past the array.
    assert!(
        !super::register_sink(super::MAX_SUBSYSTEMS as u8, test_sink, test_alloc_id),
        "a slot at the ceiling must be refused"
    );
}

#[test]
fn parking_a_deadline_keeps_its_handle_and_cancelling_destroys_it() {
    let _fixture = Fixture::start();
    let id = 4242;
    let before = super::live_handles();

    // Arm once: the deadline now owns a handle, which is the quantity that
    // discriminates a park from a cancel. Without this the assertions below
    // would pass against a fixture that never armed anything.
    super::timer_arm(id, SUBSYSTEM, 60_000).expect("arm");
    assert_eq!(
        super::live_handles(),
        before + 1,
        "the subject must exist: arming a deadline takes a handle"
    );

    // Park: disarmed, but the handle survives, so the re-arm below is a
    // deadline move rather than a fresh handle — and turnloop answers a move
    // with no completion at all, where it answers a close with two.
    super::timer_park(id).expect("park");
    assert_eq!(
        super::live_handles(),
        before + 1,
        "parking keeps the handle so the next arm moves it in place"
    );

    // The parked handle is still usable: re-arm it short and let it fire.
    super::timer_arm(id, SUBSYSTEM, 1).expect("re-arm");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_TIMER && e.id == id)),
        "a re-armed parked deadline still expires: {:?}",
        events()
    );
    assert_eq!(
        super::live_handles(),
        before,
        "a fired one-shot retires its handle"
    );

    // Cancelling is the other half of the contract and must still destroy.
    super::timer_arm(id, SUBSYSTEM, 60_000).expect("arm again");
    assert_eq!(super::live_handles(), before + 1);
    super::timer_cancel(id).expect("cancel");
    assert_eq!(
        super::live_handles(),
        before,
        "cancelling releases the handle"
    );

    // Both are idempotent on an id with no deadline.
    super::timer_park(id).expect("park is idempotent");
    super::timer_cancel(id).expect("cancel is idempotent");
    assert_eq!(super::live_handles(), before);
}

/// A stream some *other* transport connected becomes an ordinary socket on the
/// loop: its endpoints are reported, its bytes arrive as `NET_DATA` under the
/// caller's id, its writes reach the peer, and it closes like any other handle.
/// This is the path an HTTP `'upgrade'` takes into `node:net`.
#[cfg(any(unix, windows))]
#[test]
fn an_adopted_stream_is_an_ordinary_socket_on_the_loop() {
    use std::io::{Read, Write};

    let _fixture = Fixture::start();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let listen_addr = listener.local_addr().unwrap();
    let mut peer = std::net::TcpStream::connect(listen_addr).expect("connect");
    let (adopted, _) = listener.accept().expect("accept");
    let before = super::live_handles();

    let id = 77;
    super::adopt_stream(id, SUBSYSTEM, adopted.into()).expect("adopt the stream");
    assert_eq!(
        super::live_handles(),
        before + 1,
        "the adopted stream must be a handle this loop holds"
    );
    assert_eq!(
        super::local_addr(id).map(|a| a.port()),
        Some(listen_addr.port()),
        "the endpoints are read off the descriptor before it is handed over"
    );
    assert_eq!(
        super::peer_addr(id),
        Some(peer.local_addr().unwrap()),
        "the peer endpoint too"
    );

    super::read_start(id).expect("read the adopted stream");
    peer.write_all(b"hello").unwrap();
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_DATA && e.id == id)),
        "bytes the peer sends must arrive under the caller's id: {:?}",
        events()
    );
    assert_eq!(payload(NET_DATA, id), b"hello");

    super::write(id, b"world".to_vec(), 0).expect("write to the peer");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_WROTE && e.id == id)),
        "the write must complete: {:?}",
        events()
    );
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reply = [0u8; 5];
    peer.read_exact(&mut reply)
        .expect("the peer reads the reply");
    assert_eq!(&reply, b"world");

    super::close(id).expect("close the adopted stream");
    assert!(
        pump_until(|e| e.iter().any(|e| e.kind == NET_CLOSED && e.id == id)),
        "the adopted stream reports its terminal Closed: {:?}",
        events()
    );
    assert_eq!(super::live_handles(), before);
}
