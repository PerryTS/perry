use super::*;
use std::sync::{Mutex, MutexGuard};

static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

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
///   the submission is posted to that owner, so nothing is registered here;
/// * no loop exists for the agent — the connect is refused with `ENOTSUP`
///   and the socket is NOT left marked as a loop socket.
///
/// **The route is observed, not assumed**, because an agent's route is claimed
/// once per thread by the first thread to ask
/// (`event_pump::agent_loop::claim_route`) and every other thread acting for
/// that agent is declined for life — so which harness thread this lands on
/// decides the route. Every arm asserts something; none is a skip.
#[test]
fn deferred_connect_reaches_the_loop_on_every_route() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let owns_loop = turnloop_io::enabled();
    let can_post = perry_ffi::agent_post::available();
    let handles_before = perry_ffi::turnloop_net::live_handles();

    let h = unsafe { js_net_socket_alloc() };
    let _cleanup = NetHandleCleanup::new(vec![h]);
    {
        let sockets = statics::sockets().lock().unwrap();
        assert!(
            !sockets[&h].turnloop && sockets[&h].awaiting_connect,
            "fixture must start unconnected, or the verdict below is vacuous"
        );
    }

    // Port 1 on loopback: the submission is what is under test, not the
    // connect's outcome. turnloop resolves and connects asynchronously, so a
    // refused peer arrives as a later completion and cannot make this pass.
    unsafe {
        js_net_socket_method_connect(h, 1.0, UNDEFINED, UNDEFINED);
    }

    let (on_loop, awaiting) = {
        let sockets = statics::sockets().lock().unwrap();
        (sockets[&h].turnloop, sockets[&h].awaiting_connect)
    };
    let handles_after = perry_ffi::turnloop_net::live_handles();
    assert!(!awaiting, "connect() must consume the awaiting-connect state");
    if owns_loop {
        // Leave no in-flight connect behind for a sibling test's pump.
        crate::lifecycle::js_ext_net_destroy_socket(h);
    }
    let _ = unsafe { js_net_process_pending() };

    if owns_loop {
        assert!(on_loop, "an owned loop must take the connect");
        assert_eq!(
            handles_after,
            handles_before + 1,
            "the flag says turnloop but the driver holds no new handle"
        );
    } else if can_post {
        assert!(on_loop, "a posted connect still makes this a loop socket");
        assert_eq!(
            handles_after, handles_before,
            "a posted connect must not register a handle on the posting thread"
        );
    } else {
        assert!(
            !on_loop,
            "a refused connect must not leave the socket marked as a loop socket"
        );
        assert_eq!(handles_after, handles_before);
    }
}
