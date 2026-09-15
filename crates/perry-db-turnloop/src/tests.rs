//! Unit tests for the shared database transport.
//!
//! The dev-dependency turns on `perry-ffi`'s `runtime-link` feature, so these
//! run against the **real** `turnloop_net` ABI rather than its no-runtime
//! stubs: registration really goes through the layout digest check, and a
//! connect really reaches the driver. What they cannot do is *turn* the loop —
//! only perry-runtime can — so nothing here observes a completion.
//!
//! The end-to-end coverage (a real socket, a real server, real protocol events)
//! is in `scripts/turnloop/apps/`, because a database core needs a database to
//! say anything.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

/// A core that records what the driver did to it.
#[derive(Default)]
struct Log {
    connected: usize,
    received: Vec<Vec<u8>>,
    drained: usize,
    failed: Vec<String>,
    consumed: usize,
    timeouts: usize,
}

struct FakeCore {
    log: Rc<RefCell<Log>>,
    out: Vec<u8>,
    finish_after: Option<usize>,
    pending: bool,
    timeout_ms: Option<u64>,
    drain_err: Option<String>,
}

impl FakeCore {
    fn new(log: Rc<RefCell<Log>>) -> Self {
        Self {
            log,
            out: Vec::new(),
            finish_after: None,
            pending: false,
            timeout_ms: None,
            drain_err: None,
        }
    }
}

impl DbCore for FakeCore {
    fn transport_connected(&mut self) -> Result<(), String> {
        self.log.borrow_mut().connected += 1;
        self.out.extend_from_slice(b"HELLO");
        Ok(())
    }
    fn receive(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.log.borrow_mut().received.push(bytes.to_vec());
        Ok(())
    }
    fn drain(&mut self) -> Result<bool, String> {
        let mut log = self.log.borrow_mut();
        log.drained += 1;
        if let Some(message) = self.drain_err.take() {
            return Err(message);
        }
        Ok(match self.finish_after {
            Some(n) => log.drained >= n,
            None => false,
        })
    }
    fn output(&self) -> &[u8] {
        &self.out
    }
    fn consume_output(&mut self, n: usize) {
        self.log.borrow_mut().consumed += n;
        self.out.drain(..n.min(self.out.len()));
    }
    fn next_timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }
    fn handle_timeout(&mut self) {
        self.log.borrow_mut().timeouts += 1;
    }
    fn fail(&mut self, reason: &str) {
        self.log.borrow_mut().failed.push(reason.to_string());
        self.pending = false;
    }
    fn has_pending_work(&self) -> bool {
        self.pending
    }
}

extern "C" fn unused_sink(_: *const NetCompletion) {}

fn registry() -> Registry<FakeCore> {
    Registry::new(subsystem::REDIS)
}

#[test]
fn the_four_subsystem_slots_are_distinct_and_clear_of_p1_and_p5() {
    // P1 took 0, P5 took 1, and the runtime's own `turnloop_net` tests take 3.
    // A binding that picked an occupied slot would replace another binding's
    // sink and route its completions into the wrong crate — silently, because
    // `register_sink` only refuses an out-of-range slot.
    let slots = [
        subsystem::PG,
        subsystem::MYSQL,
        subsystem::REDIS,
        subsystem::MONGODB,
    ];
    for (i, a) in slots.iter().enumerate() {
        assert_ne!(*a, 0, "slot 0 belongs to perry-ext-net");
        assert_ne!(*a, 1, "slot 1 belongs to perry-ext-http");
        assert_ne!(*a, 3, "slot 3 is the runtime's turnloop_net test slot");
        for b in slots.iter().skip(i + 1) {
            assert_ne!(a, b, "two bindings claimed the same sink slot");
        }
    }
}

#[test]
fn id_bands_do_not_overlap_between_bindings() {
    // Ids are handed out from one process-wide counter, so two bindings can
    // never collide; the bands exist so an id names its owner on sight. A band
    // that started below another binding's base would make that guarantee a
    // coincidence of allocation order rather than a property.
    let mut bases = [
        id_base(subsystem::PG),
        id_base(subsystem::MYSQL),
        id_base(subsystem::REDIS),
        id_base(subsystem::MONGODB),
    ];
    bases.sort_unstable();
    for pair in bases.windows(2) {
        assert!(
            pair[1] - pair[0] >= 1 << 40,
            "bands are closer than a process could ever exhaust"
        );
    }
    assert!(
        bases[0] > 0,
        "ids must stay positive: 0 refuses a connection"
    );
}

#[test]
fn registration_passes_the_abi_layout_check_and_claims_only_its_own_slot() {
    // `register_sink` refuses outright when perry-ffi's `NetCompletion` layout
    // digest does not match the runtime's, which leaves `available` false and
    // keeps every connection on the legacy transport. A silent drift between
    // the two declarations is the failure mode P1 built that check for — this
    // asserts it still passes, and that registering one binding installs a sink
    // for its own slot and for no other.
    //
    // Deliberately `register` and not `enabled`: whether the *running thread*
    // owns a loop is not a property of the build, and `cargo test` puts each
    // test on its own thread.
    let reg = registry();
    assert!(
        reg.register(unused_sink),
        "a false here is an ABI layout mismatch between perry-ffi and perry-runtime"
    );
    assert!(perry_ffi::turnloop_net::sink_installed(subsystem::REDIS));
    for other in [subsystem::PG, subsystem::MYSQL, subsystem::MONGODB] {
        assert!(
            !perry_ffi::turnloop_net::sink_installed(other),
            "registering one binding must not install a sink for slot {other}"
        );
    }
}

#[test]
fn a_connection_that_fails_settles_its_core_rather_than_stranding_it() {
    // The transport owes every outstanding operation an answer. `abort` is the
    // path a refused connect, a read error and an EOF all funnel through, and
    // the core's `fail` is where a binding rejects its promises — a connection
    // torn down without it leaves them pending forever, which is the one
    // outcome a caller cannot recover from.
    let log = Rc::new(RefCell::new(Log::default()));
    let reg = registry();
    let id = 4242;
    reg.insert_detached(id, FakeCore::new(log.clone()), 7);
    assert_eq!(reg.live_connections(), 1);
    assert_eq!(reg.counters().0, 1, "the connect counter must move");
    assert_eq!(reg.tag(id), Some(7), "the binding's tag must survive");
    assert!(reg.is_live(id));

    reg.abort(id, "ECONNREFUSED connect -111");
    assert_eq!(
        log.borrow().failed,
        vec!["ECONNREFUSED connect -111".to_string()],
        "the core must be told why, in the driver's own words"
    );
    assert!(
        log.borrow().drained >= 1,
        "abort must drain once so the core's terminal events settle"
    );
    // A second abort must not fail the core twice: a binding that rejected its
    // promises once would reject them again, and a `JsPromise` settles once.
    reg.abort(id, "a second time");
    assert_eq!(log.borrow().failed.len(), 1);
}

#[test]
fn retiring_a_connection_settles_what_its_core_still_owes() {
    // Both retirement paths — the driver's terminal `NET_CLOSED` and the
    // `close`-already-gone branch in `finish` — drop the entry, and with it the
    // core and every promise the core still owes. Dropping without settling
    // leaves those promises pending forever. Two reviewers found this hole
    // independently in the first version of this driver, which is why it has a
    // test rather than a comment.
    let log = Rc::new(RefCell::new(Log::default()));
    let reg = registry();
    let id = 7373;
    let mut core = FakeCore::new(log.clone());
    core.pending = true; // the core owes an answer
    reg.insert_detached(id, core, 0);

    let closed = NetCompletion {
        kind: tl::NET_CLOSED,
        errno: 0,
        terminal: 1,
        _reserved: 0,
        id,
        conn: 0,
        user: 0,
        len: 0,
        queued: 0,
        data: std::ptr::null(),
        code: std::ptr::null(),
        code_len: 0,
        syscall: std::ptr::null(),
        syscall_len: 0,
    };
    reg.dispatch(&closed);
    assert_eq!(
        log.borrow().failed,
        vec!["Connection closed".to_string()],
        "a retired core with work outstanding must be failed before it is dropped"
    );
    assert_eq!(reg.live_connections(), 0, "the entry must not leak");

    // A core that owes nothing is not failed — a spurious rejection on a clean
    // close would be just as wrong in the other direction.
    let quiet = Rc::new(RefCell::new(Log::default()));
    let id2 = 7474;
    reg.insert_detached(id2, FakeCore::new(quiet.clone()), 0);
    let mut closed2 = closed;
    closed2.id = id2;
    reg.dispatch(&closed2);
    assert!(
        quiet.borrow().failed.is_empty(),
        "a clean close must not invent a rejection"
    );
    assert_eq!(reg.live_connections(), 0);
}

#[test]
fn dispatch_for_an_unknown_id_is_dropped_rather_than_panicking() {
    // A completion can outlive its entry: `close` cancels outstanding
    // operations but a completion already staged for dispatch still arrives.
    // P1's routing drops such a token; so must this.
    let reg = registry();
    let c = NetCompletion {
        kind: tl::NET_DATA,
        errno: 0,
        terminal: 0,
        _reserved: 0,
        id: 999_999,
        conn: 0,
        user: 0,
        len: 0,
        queued: 0,
        data: std::ptr::null(),
        code: std::ptr::null(),
        code_len: 0,
        syscall: std::ptr::null(),
        syscall_len: 0,
    };
    reg.dispatch(&c);
    assert_eq!(reg.live_connections(), 0);
}

#[test]
fn a_completion_message_names_the_code_and_syscall() {
    let code = b"ECONNREFUSED";
    let syscall = b"connect";
    let c = NetCompletion {
        kind: tl::NET_ERROR,
        errno: -111,
        terminal: 1,
        _reserved: 0,
        id: 1,
        conn: 0,
        user: 0,
        len: 0,
        queued: 0,
        data: std::ptr::null(),
        code: code.as_ptr(),
        code_len: code.len(),
        syscall: syscall.as_ptr(),
        syscall_len: syscall.len(),
    };
    let message = completion_message(&c);
    assert!(message.contains("ECONNREFUSED"), "{message}");
    assert!(message.contains("connect"), "{message}");
    assert!(message.contains("-111"), "{message}");
}

#[test]
fn a_completion_with_neither_code_nor_syscall_still_has_a_message() {
    // A rejected promise whose reason is the empty string is worse than a
    // vague one: the caller sees `Error: ` and cannot tell it from a bug.
    let c = NetCompletion {
        kind: tl::NET_ERROR,
        errno: 0,
        terminal: 1,
        _reserved: 0,
        id: 1,
        conn: 0,
        user: 0,
        len: 0,
        queued: 0,
        data: std::ptr::null(),
        code: std::ptr::null(),
        code_len: 0,
        syscall: std::ptr::null(),
        syscall_len: 0,
    };
    assert!(!completion_message(&c).is_empty());
}

#[test]
fn with_core_on_an_unknown_id_reports_it_rather_than_pretending() {
    // A binding calls this to submit a command; `None` is how it learns the
    // connection is gone and must reject rather than leave a pending promise.
    let reg = registry();
    assert!(reg.with_core(42, |_: &mut FakeCore| ()).is_none());
    assert!(reg.inspect(42, |_: &FakeCore| ()).is_none());
    assert!(reg.tag(42).is_none());
    assert!(!reg.is_live(42));
}
