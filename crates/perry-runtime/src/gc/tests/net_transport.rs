//! NET-TRANSPORT-DESIGN P0 witnesses: a turnloop transport owned by a native
//! payload, on the real driver, under real collections. Each test asserts its
//! subject ran (a listener bound, a completion dispatched, a move happened),
//! and `every_net_transport_sabotage_makes_its_witness_red` runs each one
//! against the defect it exists to catch.
use super::super::*;
use super::support::*;
use crate::native_payload::{self as np, CloseOutcome, NativePayloadFamily, OwnerLink};
use crate::turnloop_net::transport::{self, TransportCore, TransportPayload};
use crate::turnloop_net::{
    NetCompletion, NET_ACCEPT, NET_CLOSED, NET_CONNECT, NET_DATA, NET_FLAG_LINK,
};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static DROPS: AtomicUsize = AtomicUsize::new(0);

/// The family's own fields; its drop is counted.
struct Ext;
impl Drop for Ext {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}
type Payload = TransportPayload<Ext>;

fn install(_: &mut np::PayloadPrototype) {}
static FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::CRYPTO_HASH,
    name: "TransportProbe",
    constructor_export: None,
    constructor_length: 0,
    links_owner: true,
    install_prototype: install,
};

/// An unclaimed sink slot (the id-route tests hold 3).
const ROUTE: u8 = 14;

#[derive(Clone, Debug)]
struct Event {
    kind: i32,
    link: usize,
    /// `link_event_owner` as the sink saw it.
    owner: Option<u64>,
    bytes: Vec<u8>,
    rerouted: bool,
}

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    /// When set, the sink installs an accepted connection into a fresh
    /// payload instead of refusing it.
    static INSTALL_ACCEPTED: Cell<bool> = const { Cell::new(false) };
    static REROUTED: Cell<bool> = const { Cell::new(false) };
    static CHILD: Cell<usize> = const { Cell::new(0) };
}

extern "C" fn link_sink(completion: *const NetCompletion) {
    // SAFETY: dispatch passes a live completion for the duration of the call.
    let c = unsafe { &*completion };
    assert_ne!(
        c.flags & NET_FLAG_LINK,
        0,
        "a link route marks its completions"
    );
    let link = OwnerLink(c.id as usize);
    // SAFETY: the dispatched completion's ref keeps the cell alive.
    let owner = unsafe { np::link_event_owner(link) }.map(f64::to_bits);
    if c.kind == NET_ACCEPT && INSTALL_ACCEPTED.with(Cell::get) {
        let scope = RuntimeHandleScope::new();
        let child = scope.root_nanbox_f64(fresh());
        let child_link = np::owner_link(child.get_nanbox_f64(), &FAMILY).unwrap();
        // SAFETY: the child is OPEN on this thread; the completion is ours.
        unsafe {
            transport::install_accepted(core(child.get_nanbox_f64()), child_link, completion)
                .expect("install the accepted connection");
        }
        CHILD.with(|slot| slot.set(child_link.0));
    }
    EVENTS.with(|events| {
        events.borrow_mut().push(Event {
            kind: c.kind,
            link: link.0,
            owner,
            bytes: if c.kind == NET_DATA && c.len != 0 {
                unsafe { std::slice::from_raw_parts(c.data, c.len) }.to_vec()
            } else {
                Vec::new()
            },
            rerouted: REROUTED.with(Cell::get),
        })
    });
}

extern "C" fn alternate_sink(completion: *const NetCompletion) {
    REROUTED.with(|flag| flag.set(true));
    link_sink(completion);
    REROUTED.with(|flag| flag.set(false));
}

fn fresh() -> f64 {
    np::alloc(
        &FAMILY,
        Payload {
            core: TransportCore::new(ROUTE),
            ext: Ext,
        },
        0,
        &[],
    )
}

/// The payload's core: offset 0 of the payload (`repr(C)`, core first).
fn core(value: f64) -> *mut TransportCore {
    let payload = unsafe { np::payload_mut::<Payload>(value, &FAMILY) }.expect("an OPEN payload");
    &mut payload.core as *mut TransportCore
}

fn cell(link: OwnerLink) -> *mut crate::native_handle::NativeHandleHeader {
    link.0 as *mut crate::native_handle::NativeHandleHeader
}

fn refs(link: OwnerLink) -> u32 {
    unsafe { (*cell(link)).refs }
}

fn finalized() -> usize {
    crate::native_handle::PAYLOAD_FINALIZED.load(Ordering::SeqCst)
}

fn full() {
    let before = crate::gc::block_persist_force_mark_count();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
    assert_eq!(crate::gc::block_persist_force_mark_count(), before);
}

fn count(kind: i32, link: OwnerLink) -> usize {
    EVENTS.with(|events| {
        events
            .borrow()
            .iter()
            .filter(|e| e.kind == kind && e.link == link.0)
            .count()
    })
}

fn last(kind: i32, link: OwnerLink) -> Option<Event> {
    EVENTS.with(|events| {
        events
            .borrow()
            .iter()
            .rev()
            .find(|e| e.kind == kind && e.link == link.0)
            .cloned()
    })
}

fn pump_until(want: impl Fn() -> bool) -> bool {
    let limit = Instant::now() + Duration::from_secs(5);
    loop {
        if want() {
            return true;
        }
        if Instant::now() >= limit {
            return false;
        }
        crate::event_pump::pump_net_for_test(Duration::from_millis(5));
    }
}

/// Root scanners, counters and the loop, as the payload tests set them up.
struct Fixture;
impl Fixture {
    fn start() -> Self {
        np::reset_payload_prototypes_for_tests();
        gc_register_mutable_root_scanner(np::scan_payload_prototype_roots_mut);
        register_runtime_handle_root_scanner_for_tests();
        gc_register_named_mutable_root_scanner(
            "shape_table",
            crate::object::shapes::scan_shape_table_rekey_mut,
        );
        gc_register_named_mutable_root_scanner(
            "pinned",
            crate::gc::pin::scan_pinned_object_roots_mut,
        );
        DROPS.store(0, Ordering::SeqCst);
        EVENTS.with(|events| events.borrow_mut().clear());
        INSTALL_ACCEPTED.with(|f| f.set(false));
        CHILD.with(|c| c.set(0));
        assert!(
            crate::event_pump::install_net_loop_for_test(),
            "the host must provide a turnloop loop"
        );
        assert!(
            crate::turnloop_net::register_link_sink(ROUTE, link_sink),
            "link sink registration must succeed, or every assertion is vacuous"
        );
        Fixture
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        crate::event_pump::reset_net_loop_for_test();
        EVENTS.with(|events| events.borrow_mut().clear());
        np::reset_payload_prototypes_for_tests();
    }
}

/// Close the transport and release the payload (rule 3); the `Closed` stays
/// owed. Returns whether a `NET_CLOSED` will follow.
fn destroy(link: OwnerLink) -> bool {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(unsafe { np::link_event_owner(link) }.unwrap());
    let closed = unsafe { transport::close(core(value.get_nanbox_f64()), link) }.unwrap();
    assert_eq!(
        np::close(value.get_nanbox_f64(), &FAMILY),
        CloseOutcome::Closed
    );
    closed
}

/// N1 (runtime half) + rule 2: a payload holding a driver handle, with no JS
/// reference, survives full and moving collections until the handle's
/// terminal `Closed` is dispatched; the accept on the way reaches the same
/// owner and installs into a child that is itself kept by its handle.
#[test]
fn n1_a_handle_holding_payload_lives_until_its_closed_is_dispatched() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _fixture = Fixture::start();
    let _no_stack = ConservativeScanDisabledGuard::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(fresh());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let addr = unsafe {
        let c = core(value.get_nanbox_f64());
        let addr = transport::tcp_listen(c, link, "127.0.0.1:0".parse().unwrap(), 16, false, true)
            .expect("listen");
        transport::accept_start(c, link).expect("accept");
        addr
    };
    assert!(crate::turnloop_net::live_handles() >= 1);
    drop(scope);
    // Prove marking reaches the unreferenced owner through the pinned cell:
    // after a sweep, a stale-but-not-reused owner would otherwise look alive.
    let owner_addr =
        (unsafe { np::link_event_owner(link) }.unwrap().to_bits() & POINTER_MASK) as usize;
    clear_marks();
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid_ptrs);
    drain_incremental_mark_barrier_seeds(&valid_ptrs);
    assert_ne!(
        unsafe { (*header_from_user_ptr(owner_addr as *const u8)).gc_flags } & GC_FLAG_MARKED,
        0,
        "the pinned cell must mark its otherwise unreferenced owner"
    );
    clear_marks();
    clear_mark_seeds();
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    full();
    full();
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        0,
        "a payload holding a driver handle must survive every collection"
    );
    assert_eq!(refs(link), 1, "the installed handle holds exactly one ref");
    let before_finalized = finalized();

    INSTALL_ACCEPTED.with(|f| f.set(true));
    let _client = std::net::TcpStream::connect(addr).expect("connect to the listener");
    assert!(
        pump_until(|| count(NET_ACCEPT, link) == 1),
        "the accept must reach the unreferenced listener"
    );
    let accept = last(NET_ACCEPT, link).unwrap();
    assert_eq!(
        accept.owner,
        unsafe { np::link_event_owner(link) }.map(f64::to_bits)
    );
    let child = OwnerLink(CHILD.with(Cell::get));
    assert_ne!(child.0, 0, "the sink installed the connection");
    assert_eq!(refs(child), 1, "the accepted handle pins its own payload");
    full();
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);

    // Rule 3: close moves each handle into the driver's close, and the
    // payload is released at once; the cell stays while the Closed is owed.
    assert!(destroy(link));
    assert!(destroy(child));
    assert_eq!(DROPS.load(Ordering::SeqCst), 2, "release drops T now");
    assert_eq!(refs(link), 1, "the owed Closed keeps its ref after release");
    full();
    assert_eq!(finalized(), before_finalized, "no cell dies while owed");
    assert!(
        pump_until(|| count(NET_CLOSED, link) == 1 && count(NET_CLOSED, child) == 1),
        "both Closed completions must be dispatched"
    );
    assert!(last(NET_CLOSED, link).unwrap().owner.is_some());
    assert_eq!(refs(link), 0);
    assert_eq!(refs(child), 0);
    full();
    assert_eq!(
        finalized(),
        before_finalized + 2,
        "after the last terminal completion the cycles are garbage"
    );
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        2,
        "the drop ran exactly once each"
    );
}

/// The cb-net-2 blocker witness (N5): `close(); listen()` reopens the same
/// object; the first listener's `Closed`, dispatched after the reopen, is
/// delivered once as that close and leaves the new listener untouched.
#[test]
fn n5_a_stale_closed_after_reopen_leaves_the_new_listener_alone() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _fixture = Fixture::start();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(fresh());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    unsafe {
        let c = core(value.get_nanbox_f64());
        transport::tcp_listen(c, link, "127.0.0.1:0".parse().unwrap(), 16, false, true).unwrap();
        transport::accept_start(c, link).unwrap();
    }
    assert!(destroy(link));
    let reopened = np::attach(
        value.get_nanbox_f64(),
        &FAMILY,
        Payload {
            core: TransportCore::new(ROUTE),
            ext: Ext,
        },
        0,
    );
    assert_eq!(reopened, Ok(()));
    let addr = unsafe {
        let c = core(value.get_nanbox_f64());
        let addr = transport::tcp_listen(c, link, "127.0.0.1:0".parse().unwrap(), 16, false, true)
            .unwrap();
        transport::accept_start(c, link).unwrap();
        addr
    };
    assert_eq!(refs(link), 2, "the owed Closed and the new handle");
    let handle = unsafe { (*core(value.get_nanbox_f64())).handle() };
    assert!(handle.is_some());
    assert!(
        pump_until(|| count(NET_CLOSED, link) == 1),
        "the first listener's Closed must be dispatched"
    );
    assert_eq!(
        unsafe { (*core(value.get_nanbox_f64())).handle() },
        handle,
        "a stale Closed must not touch the reopened listener"
    );
    assert_eq!(refs(link), 1);
    let _client = std::net::TcpStream::connect(addr).unwrap();
    assert!(
        pump_until(|| count(NET_ACCEPT, link) == 1),
        "the reopened listener still accepts"
    );
    assert!(destroy(link));
    assert!(pump_until(|| count(NET_CLOSED, link) == 2));
    assert_eq!(refs(link), 0);
}

/// Runtime half of N8: the original multishot read token survives a route
/// store, and bytes already sent on that read reach the new sink in order.
#[test]
fn n8_a_route_store_keeps_the_same_multishot_read() {
    use std::io::Write;
    let _guard = CopyingNurseryTestGuard::new(0);
    let _fixture = Fixture::start();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    assert!(crate::turnloop_net::register_link_sink(
        ROUTE + 1,
        alternate_sink
    ));
    let scope = RuntimeHandleScope::new();
    let server = scope.root_nanbox_f64(fresh());
    let link = np::owner_link(server.get_nanbox_f64(), &FAMILY).unwrap();
    let addr = unsafe {
        let c = core(server.get_nanbox_f64());
        let addr = transport::tcp_listen(c, link, "127.0.0.1:0".parse().unwrap(), 16, false, true)
            .unwrap();
        transport::accept_start(c, link).unwrap();
        addr
    };
    INSTALL_ACCEPTED.with(|flag| flag.set(true));
    let mut peer = std::net::TcpStream::connect(addr).unwrap();
    assert!(pump_until(|| CHILD.with(Cell::get) != 0));
    let child = OwnerLink(CHILD.with(Cell::get));
    let owner = scope.root_nanbox_f64(unsafe { np::link_event_owner(child) }.unwrap());
    unsafe {
        let c = core(owner.get_nanbox_f64());
        transport::read_start(c, child).unwrap();
        let read = (*c).read_operation();
        assert!(read.is_some(), "subject must have a multishot read");
        peer.write_all(b"head+same-packet").unwrap();
        transport::set_route(c, child, ROUTE + 1).unwrap();
        assert_eq!(
            (*c).read_operation(),
            read,
            "upgrade must not replace its read"
        );
    }
    assert!(pump_until(|| EVENTS.with(|events| events
        .borrow()
        .iter()
        .filter(|e| e.link == child.0)
        .map(|e| e.bytes.len())
        .sum::<usize>()
        >= 16)));
    EVENTS.with(|events| {
        let events = events.borrow();
        let reads: Vec<_> = events
            .iter()
            .filter(|e| e.link == child.0 && e.kind == NET_DATA)
            .collect();
        assert!(
            reads.iter().all(|e| e.rerouted),
            "old read tokens dispatch through the current route"
        );
        assert_eq!(
            reads
                .iter()
                .flat_map(|e| e.bytes.iter().copied())
                .collect::<Vec<_>>(),
            b"head+same-packet"
        );
    });
    assert!(destroy(child));
    assert!(destroy(link));
    assert!(pump_until(|| refs(child) == 0 && refs(link) == 0));
}

/// N9: a cancelled resolver cannot clear the reopened socket's plan, and
/// pre-connect writes survive localhost's v6-to-v4 fallback.
#[test]
fn n9_a_stale_resolve_leaves_the_new_plan_and_backlog_alone() {
    use std::io::Read;
    let _guard = CopyingNurseryTestGuard::new(0);
    let _fixture = Fixture::start();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let scope = RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(fresh());
    let link = np::owner_link(socket.get_nanbox_f64(), &FAMILY).unwrap();
    unsafe {
        transport::tcp_connect(core(socket.get_nanbox_f64()), link, "localhost", port, true)
            .unwrap();
    }
    assert_eq!(refs(link), 1, "the old resolver is owed before any turn");
    assert!(!destroy(link), "resolving socket has no handle yet");
    np::attach(
        socket.get_nanbox_f64(),
        &FAMILY,
        Payload {
            core: TransportCore::new(ROUTE),
            ext: Ext,
        },
        0,
    )
    .unwrap();
    unsafe {
        let c = core(socket.get_nanbox_f64());
        transport::tcp_connect(c, link, "localhost", port, true).unwrap();
        transport::write(c, link, b"pre-connect".to_vec(), 1).unwrap();
    }
    assert!(
        pump_until(|| count(NET_CONNECT, link) == 1),
        "new resolver must connect despite the old terminal completion"
    );
    assert!(
        pump_until(|| count(crate::turnloop_net::NET_WROTE, link) == 1),
        "pre-connect write must leave the backlog"
    );
    let (mut peer, _) = listener
        .accept()
        .expect("only the new plan reaches the v4 listener");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut bytes = [0; 11];
    peer.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"pre-connect");
    assert!(destroy(link));
    assert!(pump_until(|| refs(link) == 0));
}

/// N11: the pipe server path. `close()`, a synchronous unlink (the binding's
/// job), then `listen(samePath)`: the old `Closed` neither disturbs the new
/// listener nor its socket file.
#[cfg(unix)]
#[test]
fn n11_a_pipe_server_relistens_on_its_path_across_a_stale_closed() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _fixture = Fixture::start();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let dir = std::env::temp_dir().join(format!("perry-netp0-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("n11.sock");
    let _ = std::fs::remove_file(&path);
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(fresh());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    unsafe {
        let c = core(value.get_nanbox_f64());
        transport::pipe_listen(c, link, &path, 16).unwrap();
        transport::accept_start(c, link).unwrap();
    }
    assert!(destroy(link));
    std::fs::remove_file(&path).expect("the binding unlinks at close, synchronously");
    np::attach(
        value.get_nanbox_f64(),
        &FAMILY,
        Payload {
            core: TransportCore::new(ROUTE),
            ext: Ext,
        },
        0,
    )
    .unwrap();
    unsafe {
        let c = core(value.get_nanbox_f64());
        transport::pipe_listen(c, link, &path, 16).unwrap();
        transport::accept_start(c, link).unwrap();
    }
    assert!(pump_until(|| count(NET_CLOSED, link) == 1));
    assert!(
        path.exists(),
        "the old Closed must not remove the new socket"
    );
    assert!(unsafe { (*core(value.get_nanbox_f64())).handle() }.is_some());
    let _client = std::os::unix::net::UnixStream::connect(&path).expect("connect to the new path");
    assert!(pump_until(|| count(NET_ACCEPT, link) == 1));
    assert!(destroy(link));
    assert!(pump_until(|| count(NET_CLOSED, link) == 2));
    let _ = std::fs::remove_dir_all(&dir);
}

/// N7: dropping a payload never reaches the loop, from the sweep or from an
/// explicit release (the cb-net blocker's "record Drop releases listener"
/// witness becomes this guard).
#[test]
fn n7_dropping_a_transport_payload_never_reaches_the_driver() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _fixture = Fixture::start();
    let _no_stack = ConservativeScanDisabledGuard::new();
    let before = finalized();
    {
        let scope = RuntimeHandleScope::new();
        let _never_connected = scope.root_nanbox_f64(fresh());
        let released = scope.root_nanbox_f64(fresh());
        let _forbid = transport::ForbidDriver::new();
        assert_eq!(
            np::close(released.get_nanbox_f64(), &FAMILY),
            CloseOutcome::Closed
        );
    }
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    {
        let _forbid = transport::ForbidDriver::new();
        full();
    }
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        2,
        "the sweep dropped the other"
    );
    assert_eq!(finalized(), before + 2);
}

/// N10: a moving collection between submission and completion moves the
/// owner; the token still names the cell, and the event reaches the moved
/// object (the pinned cell's owner edge is rewritten).
#[test]
fn n10_a_moved_owner_still_receives_its_events() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _fixture = Fixture::start();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(fresh());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let addr = unsafe {
        let c = core(value.get_nanbox_f64());
        let addr = transport::tcp_listen(c, link, "127.0.0.1:0".parse().unwrap(), 16, false, true)
            .unwrap();
        transport::accept_start(c, link).unwrap();
        addr
    };
    let before = value.get_nanbox_f64().to_bits();
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    let moved = value.get_nanbox_f64().to_bits();
    assert_ne!(before, moved, "the fixture must move its owner");
    let _client = std::net::TcpStream::connect(addr).unwrap();
    assert!(pump_until(|| count(NET_ACCEPT, link) == 1));
    assert_eq!(
        last(NET_ACCEPT, link).unwrap().owner,
        Some(value.get_nanbox_f64().to_bits()),
        "the event must reach the moved owner"
    );
    assert!(destroy(link));
    assert!(pump_until(|| count(NET_CLOSED, link) == 1));
}

/// N6 / L8 (runtime half): a worker agent going away with a `Closed` still
/// owed runs no JS for it and dereferences no token; its heap teardown then
/// finalizes the cell despite the ref.
#[test]
fn n6_teardown_discards_owed_completions_without_dispatch() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let before = finalized();
    let (dispatched, discarded) = std::thread::spawn(|| {
        let _fixture = Fixture::start();
        let scope = RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(fresh());
        let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
        unsafe {
            let c = core(value.get_nanbox_f64());
            transport::tcp_listen(c, link, "127.0.0.1:0".parse().unwrap(), 16, false, true)
                .unwrap();
        }
        assert!(destroy(link));
        drop(scope);
        // The agent's teardown sets the discard rule before its last turns.
        transport::begin_teardown_for_test();
        assert!(
            pump_until(|| transport::discarded_for_test() + count(NET_CLOSED, link) >= 1),
            "the owed Closed must arrive during teardown, or the witness is vacuous"
        );
        crate::event_pump::shutdown_agent_loop();
        (count(NET_CLOSED, link), transport::discarded_for_test())
    })
    .join()
    .unwrap();
    assert_eq!(dispatched, 0, "teardown must discard, never dispatch");
    assert!(discarded >= 1);
    assert_eq!(
        finalized(),
        before + 1,
        "the worker heap teardown finalizes the pinned cell"
    );
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
}

/// The ABI digest folds the opaque core block: a binding built against
/// another block size computes another digest and is refused at
/// registration (perry-ffi's `register_*_sink`). The block is tight on the
/// platforms this test runs on, so the struct and the block move together.
#[test]
fn the_net_abi_digest_folds_the_transport_core_block() {
    use crate::turnloop_net::abi::{js_perry_net_abi_layout, net_abi_layout};
    use crate::turnloop_net::TRANSPORT_CORE_WORDS;
    assert_eq!(
        js_perry_net_abi_layout(),
        net_abi_layout(TRANSPORT_CORE_WORDS)
    );
    assert_ne!(
        net_abi_layout(TRANSPORT_CORE_WORDS),
        net_abi_layout(TRANSPORT_CORE_WORDS + 1)
    );
    assert_ne!(
        net_abi_layout(TRANSPORT_CORE_WORDS),
        net_abi_layout(TRANSPORT_CORE_WORDS - 1)
    );
    let size = std::mem::size_of::<TransportCore>();
    eprintln!("TransportCore: {size} bytes, block {TRANSPORT_CORE_WORDS} words");
    #[cfg(not(windows))]
    assert_eq!(
        size.div_ceil(8),
        TRANSPORT_CORE_WORDS,
        "the reserved block must match the core exactly on this platform"
    );
    assert_eq!(std::mem::offset_of!(Payload, core), 0, "core first");
}

#[test]
fn every_net_transport_sabotage_makes_its_witness_red() {
    let exe = std::env::current_exe().unwrap();
    for (var, fault, witness) in [
        (
            "PERRY_TEST_NET_SABOTAGE",
            "skip_ref",
            "n1_a_handle_holding_payload_lives_until_its_closed_is_dispatched",
        ),
        (
            "PERRY_TEST_CALLBACK_SABOTAGE",
            "mark",
            "n1_a_handle_holding_payload_lives_until_its_closed_is_dispatched",
        ),
        (
            "PERRY_TEST_NET_SABOTAGE",
            "plan_check",
            "n9_a_stale_resolve_leaves_the_new_plan_and_backlog_alone",
        ),
        (
            "PERRY_TEST_NET_SABOTAGE",
            "route_resubmit",
            "n8_a_route_store_keeps_the_same_multishot_read",
        ),
        (
            "PERRY_TEST_NET_SABOTAGE",
            "handle_check",
            "n5_a_stale_closed_after_reopen_leaves_the_new_listener_alone",
        ),
        (
            "PERRY_TEST_NET_SABOTAGE",
            "handle_check",
            "n11_a_pipe_server_relistens_on_its_path_across_a_stale_closed",
        ),
        (
            "PERRY_TEST_NET_SABOTAGE",
            "drop_closes",
            "n7_dropping_a_transport_payload_never_reaches_the_driver",
        ),
        (
            "PERRY_TEST_CALLBACK_SABOTAGE",
            "rewrite",
            "n10_a_moved_owner_still_receives_its_events",
        ),
        (
            "PERRY_TEST_NET_SABOTAGE",
            "teardown_dispatch",
            "n6_teardown_discards_owed_completions_without_dispatch",
        ),
    ] {
        if cfg!(not(unix)) && witness.starts_with("n11") {
            continue;
        }
        let name = format!("gc::tests::net_transport::{witness}");
        let output = std::process::Command::new(&exe)
            .args(["--exact", &name, "--nocapture", "--test-threads=1"])
            .env(var, fault)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "{witness} must exist"
        );
        assert!(
            !output.status.success(),
            "{var}={fault} must make {witness} RED"
        );
        eprintln!(
            "net transport sabotage {var}={fault}: {witness} RED ({})",
            output.status
        );
    }
}
