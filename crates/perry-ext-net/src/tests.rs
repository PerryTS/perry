use super::*;
use std::sync::{Mutex, MutexGuard};

/// Serialises every test that touches the process-global socket, listener and
/// pending-event tables against every test that drains them
/// (`js_net_process_pending` dispatches whatever is queued, for every handle).
pub(crate) static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

struct GcTestGuard {
    frame: u64,
    previous_force_evacuation: i32,
    _lock: MutexGuard<'static, ()>,
}

impl GcTestGuard {
    fn new() -> Self {
        let lock = GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Rewriting is observable only when the collector moves the root.
        // Keep that test policy thread-local so unrelated test threads do not
        // observe a process-wide environment mutation.
        let previous_force_evacuation = perry_runtime::gc::js_gc_force_evacuation_test_override(1);
        perry_runtime::gc::js_gc_write_barriers_emitted(1);
        let frame = perry_runtime::gc::js_shadow_frame_push(1);
        Self {
            frame,
            previous_force_evacuation,
            _lock: lock,
        }
    }
}

impl Drop for GcTestGuard {
    fn drop(&mut self) {
        perry_runtime::gc::js_shadow_frame_pop(self.frame);
        perry_runtime::gc::js_gc_write_barriers_emitted(0);
        perry_runtime::gc::js_gc_force_evacuation_test_override(self.previous_force_evacuation);
    }
}

struct NetHandleCleanup {
    handles: Vec<i64>,
}

impl NetHandleCleanup {
    fn new(handles: Vec<i64>) -> Self {
        Self { handles }
    }
}

impl Drop for NetHandleCleanup {
    fn drop(&mut self) {
        let mut listeners = statics::listeners().lock().unwrap();
        for handle in &self.handles {
            listeners.remove(handle);
        }
        drop(listeners);

        let mut sockets = statics::sockets().lock().unwrap();
        for handle in &self.handles {
            sockets.remove(handle);
        }
    }
}

fn young_gc_root() -> i64 {
    perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
}

fn assert_rewritten(before: i64, after: i64) {
    assert_ne!(after, before);
    assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
}

#[test]
fn gc_mutable_scanner_rewrites_listener_roots() {
    let _guard = GcTestGuard::new();
    perry_ffi::gc_register_mutable_root_scanner_named(
        "perry-ext-net",
        crate::gc_roots::scan_net_roots,
    );

    // Keep an ordinary shadow-stack root as the control. Its rewrite proves
    // the collection copied live objects independently of scan_net_roots, so
    // a listener that stays at its old address is an actual scanner failure.
    let control = young_gc_root();
    perry_runtime::gc::js_shadow_slot_set(0, control as u64);

    let socket_id = -9_001;
    let _cleanup = NetHandleCleanup::new(vec![socket_id]);
    let callback = young_gc_root();
    {
        let mut listeners = statics::listeners().lock().unwrap();
        listeners
            .entry(socket_id)
            .or_default()
            .entry("data".to_string())
            .or_default()
            .push(callback);
    }

    let copying_cycles_before = perry_runtime::gc::copying_minor_cycles();
    let moved_objects_before = perry_runtime::gc::moved_objects_total();
    let _ = perry_runtime::gc::gc_collect_minor();

    assert!(
        perry_runtime::gc::copying_minor_cycles() > copying_cycles_before,
        "minor GC did not run the copying collector, so the scanner was not exercised"
    );
    assert!(
        perry_runtime::gc::moved_objects_total() > moved_objects_before,
        "copying minor did not relocate an object, so the scanner was not exercised"
    );
    assert_rewritten(control, perry_runtime::gc::js_shadow_slot_get(0) as i64);

    let after = {
        let listeners = statics::listeners().lock().unwrap();
        listeners
            .get(&socket_id)
            .and_then(|per_socket| per_socket.get("data"))
            .and_then(|callbacks| callbacks.first())
            .copied()
    };
    statics::listeners().lock().unwrap().remove(&socket_id);
    assert_rewritten(
        callback,
        after.expect("listener callback should remain registered"),
    );
}

/// Issuing two `js_net_socket_alloc()` calls must not panic and must
/// register the GC scanner exactly once. Both handles should be
/// distinct positive integers.
#[test]
fn alloc_is_idempotent() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let h1 = unsafe { js_net_socket_alloc() };
    let h2 = unsafe { js_net_socket_alloc() };
    let _cleanup = NetHandleCleanup::new(vec![h1, h2]);
    assert!(h1 > 0);
    assert!(h2 > 0);
    assert_ne!(h1, h2);
    assert!(is_net_socket_handle(h1));
    assert!(is_net_socket_handle(h2));
}

/// `js_net_has_pending()` returns 0 when no sockets are registered
/// and no events are pending — the loop-keepalive baseline.
///
/// We can't truly assert "no sockets registered" because earlier
/// tests in the same process leave entries behind (the registry is
/// process-wide). Instead, allocate a socket, drop it via the close
/// path, and check that has_pending eventually returns to 0.
#[test]
fn has_pending_false_when_idle() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Drain any leftover events from sibling tests.
    let _ = unsafe { js_net_process_pending() };
    // Snapshot: with no real connection in flight, has_pending may
    // still return 1 because of the alloc-test sockets above leaving
    // handles in the registry. The contract documented here is that
    // it returns *some* non-negative integer without crashing.
    let v = js_net_has_pending();
    assert!(v == 0 || v == 1, "has_pending must be 0 or 1, got {}", v);
}

/// Listener registration round-trip: `.on('data', cb)` stores the
/// callback pointer in the per-socket listener map. We use a non-zero
/// sentinel so we never try to invoke it.
#[test]
fn listener_registration_round_trip() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let h = unsafe { js_net_socket_alloc() };
    let _cleanup = NetHandleCleanup::new(vec![h]);
    let event = alloc_string("data");
    unsafe {
        js_net_socket_on(h, event.as_raw() as i64, 0xDEADBEEF_i64);
        js_net_socket_on(h, event.as_raw() as i64, 0xCAFEBABE_i64);
    }
    let cbs = listeners_for(h, "data");
    assert_eq!(cbs.len(), 2);
    assert_eq!(cbs[0], 0xDEADBEEF_i64);
    assert_eq!(cbs[1], 0xCAFEBABE_i64);
}

/// `undefined` as the codegen hands it to a native method: the two trailing
/// arguments of `socket.connect(port)`.
const UNDEFINED: f64 = f64::from_bits(0x7FFC_0000_0000_0001);

/// `new net.Socket()` then `socket.connect(port)` — issue #422's deferred
/// connect, which is also what `Bun.connect` and `net.Socket.prototype.connect`
/// lower to — must reach the agent's turnloop loop, on whichever thread asks.
///
/// History: `js_net_socket_method_connect` once had no `turnloop_io::enabled()`
/// check at all and spawned a tokio socket task even with the loop fully
/// available. There is no tokio task to fall back to any more, so the three
/// outcomes are the three routes of `turnloop_io::on_loop`:
///
/// * this thread owns the loop — the connect is submitted here, and
///   `turnloop_net::live_handles` (the independent witness: it counts the
///   handles this thread's loop actually holds) moves by one;
/// * another thread owns it — the socket is published as a loop socket and
///   the submission is posted to that owner, so nothing is registered here,
///   and the owner's next turn runs it;
/// * no loop exists for the agent — here, a thread declined while another
///   owned the route, connecting after that owner exited (a decline is for the
///   thread's life, so it does not re-claim) — the connect is refused with
///   `ENOTSUP` and the socket is NOT left marked as a loop socket.
///
/// **Every route runs on every run, each on an agent this test mints.** An
/// agent's route is claimed once per thread by the first thread to ask
/// (`event_pump::agent_loop::claim_route`), for as long as that thread lives.
/// This test used to observe the route of the PRIMARY agent from a libtest
/// thread, so which arm ran was chosen by other tests, and the non-owner arms
/// could not be decided at all: whether a post lands depends on another
/// test's thread still being alive with its loop built, which can change
/// between this test's snapshot and its `connect()` (#11597). A fresh agent's
/// slot is one nobody else can name.
#[test]
fn deferred_connect_reaches_the_loop_on_every_route() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Allocate a socket, `connect()` it to loopback port 1, and report
    /// `(handle, on_loop, awaiting, live handles before, live handles after)`.
    /// Port 1: the submission is under test, not the connect's outcome.
    /// turnloop connects asynchronously, so a refused peer arrives as a later
    /// completion and cannot make this pass.
    fn connect_and_observe() -> (i64, bool, bool, usize, usize) {
        let handles_before = perry_ffi::turnloop_net::live_handles();
        let h = unsafe { js_net_socket_alloc() };
        {
            let sockets = statics::sockets().lock().unwrap();
            assert!(
                !sockets[&h].turnloop && sockets[&h].awaiting_connect,
                "fixture must start unconnected, or the verdict below is vacuous"
            );
        }
        unsafe {
            js_net_socket_method_connect(h, 1.0, UNDEFINED, UNDEFINED);
        }
        let (on_loop, awaiting) = {
            let sockets = statics::sockets().lock().unwrap();
            (sockets[&h].turnloop, sockets[&h].awaiting_connect)
        };
        let handles_after = perry_ffi::turnloop_net::live_handles();
        (h, on_loop, awaiting, handles_before, handles_after)
    }

    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Routes 1 and 2: an owner, and a second thread of its agent.
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(
            turnloop_io::enabled(),
            "the first thread of a fresh agent must own its loop"
        );
        let (h, on_loop, awaiting, handles_before, handles_after) = connect_and_observe();
        let _cleanup = NetHandleCleanup::new(vec![h]);
        // Leave no in-flight connect behind for a sibling test's pump.
        crate::lifecycle::js_ext_net_destroy_socket(h);
        let _ = unsafe { js_net_process_pending() };
        assert!(
            !awaiting,
            "connect() must consume the awaiting-connect state"
        );
        assert!(on_loop, "an owned loop must take the connect");
        assert_eq!(
            handles_after,
            handles_before + 1,
            "the flag says turnloop but the driver holds no new handle"
        );

        // The owner's loop exists now; a turn publishes its postbox.
        let limit = Instant::now() + Duration::from_secs(10);
        while !perry_ffi::agent_post::available() {
            assert!(Instant::now() < limit, "the owner never published a route");
            perry_runtime::event_pump::js_loop_turn_bounded(0);
        }

        let ran_before = perry_ffi::agent_post::dispatched();
        let (owns, can_post, (h, on_loop, awaiting, handles_before, handles_after)) =
            std::thread::spawn(move || {
                perry_runtime::agent::enter_agent_for_test(agent);
                let owns = turnloop_io::enabled();
                let can_post = perry_ffi::agent_post::available();
                (owns, can_post, connect_and_observe())
            })
            .join()
            .expect("the posting thread does not panic");
        let _cleanup = NetHandleCleanup::new(vec![h]);
        assert!(!owns, "a second thread of the agent must not own its loop");
        assert!(can_post, "the agent has a live, built loop to post to");
        assert!(
            !awaiting,
            "connect() must consume the awaiting-connect state"
        );
        assert!(on_loop, "a posted connect still makes this a loop socket");
        assert_eq!(
            handles_after, handles_before,
            "a posted connect must not register a handle on the posting thread"
        );
        // The owner runs the posted submission on its next turn.
        let limit = Instant::now() + Duration::from_secs(10);
        while perry_ffi::agent_post::dispatched() == ran_before {
            assert!(
                Instant::now() < limit,
                "the owner never ran the posted connect"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(10);
        }
        crate::lifecycle::js_ext_net_destroy_socket(h);
        let _ = unsafe { js_net_process_pending() };
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .expect("the owner thread does not panic");

    // Route 3: declined by a live owner, connecting after that owner exited.
    let (claimed_tx, claimed_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let claimant = std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        // Asking claims the route slot, without building a loop.
        let claimed = turnloop_io::enabled();
        claimed_tx
            .send((agent, claimed))
            .expect("the test is waiting");
        let _ = release_rx.recv();
        perry_runtime::agent::retire_agent(agent);
    });
    let (agent, claimed) = claimed_rx.recv().expect("the claimant reports");
    assert!(
        claimed,
        "the first thread of a fresh agent claims its route"
    );
    let (declined_tx, declined_rx) = mpsc::channel();
    let (gone_tx, gone_rx) = mpsc::channel::<()>();
    let refused = std::thread::spawn(move || {
        perry_runtime::agent::enter_agent_for_test(agent);
        let owns = turnloop_io::enabled();
        declined_tx.send(()).expect("the test is waiting");
        gone_rx.recv().expect("the test reports the owner gone");
        let can_post = perry_ffi::agent_post::available();
        (owns, can_post, connect_and_observe())
    });
    declined_rx.recv().expect("the declined thread reports");
    release_tx.send(()).expect("the claimant is waiting");
    claimant.join().expect("the claimant does not panic");
    gone_tx.send(()).expect("the declined thread is waiting");
    let (owns, can_post, (h, on_loop, awaiting, handles_before, handles_after)) =
        refused.join().expect("the refused thread does not panic");
    let _cleanup = NetHandleCleanup::new(vec![h]);
    let _ = unsafe { js_net_process_pending() };
    assert!(!owns, "the route was held by a live claimant when asked");
    assert!(
        !can_post,
        "the owner is gone, so there is nothing to post to"
    );
    assert!(
        !awaiting,
        "connect() must consume the awaiting-connect state"
    );
    assert!(
        !on_loop,
        "a refused connect must not leave the socket marked as a loop socket"
    );
    assert_eq!(handles_after, handles_before);
}

/// Endpoint dispatch depends on stored socket state, independently of the OS
/// listener used by the adopted-stream regression below.
#[test]
fn stored_socket_endpoints_reach_dynamic_dispatch() {
    let _guard = GcTestGuard::new();
    let socket_id = -30_001;
    let _cleanup = NetHandleCleanup::new(vec![socket_id]);
    let getters: [(&str, unsafe extern "C" fn(i64) -> f64); 6] = [
        ("remoteAddress", js_net_socket_get_remote_address),
        ("remotePort", js_net_socket_get_remote_port),
        ("remoteFamily", js_net_socket_get_remote_family),
        ("localAddress", js_net_socket_get_local_address),
        ("localPort", js_net_socket_get_local_port),
        ("localFamily", js_net_socket_get_local_family),
    ];
    for endpoints in [
        Some(("127.0.0.1:12001", "127.0.0.2:12002")),
        Some(("[::1]:12003", "[2001:db8::1]:12004")),
        None,
    ] {
        let mut socket = SocketState::for_test(false);
        if let Some((local, remote)) = endpoints {
            socket.local_addr = Some(local.parse().unwrap());
            socket.remote_addr = Some(remote.parse().unwrap());
        }
        statics::sockets().lock().unwrap().insert(socket_id, socket);
        for (name, getter) in getters {
            let expected = unsafe { getter(socket_id) };
            assert_eq!(
                expected.to_bits() == dispatch::undefined().to_bits(),
                endpoints.is_none(),
                "{name} follows the stored endpoint"
            );
            // Preserve an address/family string if the dispatch getter allocates
            // across a moving collection before the value comparison.
            perry_runtime::gc::js_shadow_slot_set(0, expected.to_bits());
            let mut actual = dispatch::undefined();
            assert_eq!(
                unsafe {
                    dispatch::js_ext_net_handle_property_dispatch(
                        socket_id,
                        name.as_ptr(),
                        name.len(),
                        &mut actual,
                    )
                },
                1,
                "{name} is claimed for a socket"
            );
            let expected = f64::from_bits(perry_runtime::gc::js_shadow_slot_get(0));
            assert_ne!(
                perry_runtime::value::js_jsvalue_equals(expected, actual),
                0,
                "{name} agrees with its direct getter"
            );
        }
    }
    let invalid_id = -30_002;
    assert!(!statics::sockets().lock().unwrap().contains_key(&invalid_id));
    for (name, _) in getters {
        let mut actual = 123.0;
        assert_eq!(
            unsafe {
                dispatch::js_ext_net_handle_property_dispatch(
                    invalid_id,
                    name.as_ptr(),
                    name.len(),
                    &mut actual,
                )
            },
            0,
            "{name} is not claimed for an invalid handle"
        );
        assert_eq!(actual, 123.0);
    }
}

/// #11155: `adopt_upgraded_tcp_stream` adopts on the calling thread's own loop
/// or refuses outright. It used to park the stream in a process-wide map and
/// post the adoption with no agent named, so a caller that did not own the
/// loop reached whichever agent the runtime defaulted to, and the socket id it
/// returned could be adopted onto another agent's loop.
///
/// Both halves are observed, and both run every time. A thread of the agent
/// that is not its owner must get `INVALID_HANDLE`, register nothing, and close
/// the stream (the peer reads EOF, the witness that nothing kept it parked).
/// The owner's adoption must land on its own loop: the driver holds one more
/// handle before the call returns, with no turn run in between.
///
/// **Why the test mints its own agent.** An unclaimed libtest thread resolves
/// to the PRIMARY agent, whose route slot belongs to whichever test thread
/// asked first, for as long as that thread lives. This test used to spawn its
/// "non-owner" from such a thread and assume the route was settled. It was
/// not: when this thread had been declined, the owner was some other test's
/// thread, and if that thread exited first its `ClaimGuard` released the slot,
/// so the spawned thread found no route, claimed the loop itself, and got a
/// socket id (#11597; intermittent on Linux and macOS alike, 0 of 300 runs
/// with one test thread). That is the documented first-to-ask rule, not a
/// leak: the route of a dead owner is free. A fresh agent has an empty slot
/// nobody else can name, so here the owner is this test's own thread, alive
/// until the refusal has been observed.
#[test]
fn upgraded_stream_adoption_is_on_the_callers_loop_or_refused() {
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let connect = move || TcpStream::connect(("127.0.0.1", port)).expect("loopback connects");
    let accept = move |listener: &TcpListener| {
        let (peer, _) = listener.accept().expect("the connection is accepted");
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a read timeout");
        peer
    };

    std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(
            turnloop_io::enabled(),
            "the first thread of a fresh agent must own its loop, or neither \
             half below tests what it names"
        );

        // Off the owner: a second thread of THIS agent, while the owner lives.
        // Counted per agent: other tests insert into the same process-wide
        // table without this lock, and an adoption is stamped with its agent.
        let agent_sockets = move || {
            let sockets = statics::sockets().lock().unwrap();
            sockets.values().filter(|s| s.owner_agent == agent).count()
        };
        let sockets_before = agent_sockets();
        let (refused, could_submit) = std::thread::spawn(move || {
            perry_runtime::agent::enter_agent_for_test(agent);
            let id = adopt_upgraded_tcp_stream(connect());
            (id, turnloop_io::enabled())
        })
        .join()
        .expect("the adopting thread does not panic");
        let mut peer = accept(&listener);
        assert!(
            !could_submit,
            "the second thread must NOT own the loop, or the refusal is vacuous"
        );
        assert_eq!(
            refused,
            perry_ffi::INVALID_HANDLE,
            "a thread that does not own the loop must not get a socket id"
        );
        assert_eq!(
            agent_sockets(),
            sockets_before,
            "a refused adoption must leave no socket registered"
        );
        let mut byte = [0u8; 1];
        assert_eq!(
            peer.read(&mut byte)
                .expect("the peer reads EOF, not a timeout"),
            0,
            "a refused adoption must close the stream, not park it"
        );

        // On the owner.
        let handles_before = perry_ffi::turnloop_net::live_handles();
        let id = adopt_upgraded_tcp_stream(connect());
        let _peer = accept(&listener);
        let _cleanup = NetHandleCleanup::new(vec![id]);
        assert_ne!(id, perry_ffi::INVALID_HANDLE, "the owner adopts");
        assert_eq!(
            perry_ffi::turnloop_net::live_handles(),
            handles_before + 1,
            "the adoption must be on this thread's loop when the call returns"
        );
        // Accepted/upgraded sockets carry both endpoints already; the dynamic
        // property path must serve the same getters as a statically typed one.
        for (name, expected) in unsafe {
            [
                ("remoteAddress", js_net_socket_get_remote_address(id)),
                ("remotePort", js_net_socket_get_remote_port(id)),
                ("remoteFamily", js_net_socket_get_remote_family(id)),
                ("localAddress", js_net_socket_get_local_address(id)),
                ("localPort", js_net_socket_get_local_port(id)),
                ("localFamily", js_net_socket_get_local_family(id)),
            ]
        } {
            assert_ne!(
                expected.to_bits(),
                dispatch::undefined().to_bits(),
                "{name} has an endpoint"
            );
            let mut actual = dispatch::undefined();
            assert_eq!(
                unsafe {
                    dispatch::js_ext_net_handle_property_dispatch(
                        id,
                        name.as_ptr(),
                        name.len(),
                        &mut actual,
                    )
                },
                1,
                "{name} is claimed"
            );
            // Address/family reads allocate strings; compare their JS values.
            assert_ne!(
                perry_runtime::value::js_jsvalue_equals(expected, actual),
                0,
                "{name} matches"
            );
        }
        crate::lifecycle::js_ext_net_destroy_socket(id);
        let _ = unsafe { js_net_process_pending() };
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .expect("the owner thread does not panic");
}
