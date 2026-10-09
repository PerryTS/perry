//! Real JS emitter and driver witnesses for the new binding implementation.
//! These exercise the same public Socket and Server payloads used by compiled programs.

use super::{payload_events as events, payload_io as io, payload_server as server};
use super::{payload_socket as socket, payload_transport as p};
use perry_ffi::{JsThis, RawClosureHeader, TransientRootScope};
use std::cell::Cell;

thread_local! {
    static CLOSES: Cell<usize> = const { Cell::new(0) };
    static ECHO_BYTES: Cell<usize> = const { Cell::new(0) };
}

static CODEC_DROPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct CodecProbe(Vec<u8>);
impl Drop for CodecProbe {
    fn drop(&mut self) {
        CODEC_DROPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
static CODEC_VTABLE: perry_ffi::native_stream::PayloadVTable =
    perry_ffi::native_stream::payload_vtable::<CodecProbe>(None);
static CODEC: perry_ffi::native_payload::PayloadFamily =
    perry_ffi::native_payload::PayloadFamily::new::<CodecProbe>(
        perry_ffi::native_class_ids::HTTP_PARSER,
        "HTTPParser",
        false,
        &CODEC_VTABLE,
    )
    .with_constructor_length(0);

unsafe fn close_codec(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(socket::state(owner.get()));
    let codec = scope.root_nanbox(p::own_get(state.get(), "parser"));
    perry_ffi::native_payload::close(codec.get(), &CODEC);
}

#[test]
fn explicit_socket_close_releases_its_separate_codec_immediately_and_once() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        CODEC_DROPS.store(0, std::sync::atomic::Ordering::SeqCst);
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let parser = scope.root_nanbox(unsafe {
            perry_ffi::native_payload::alloc_in(
                &CODEC,
                "",
                CodecProbe(vec![1; 16 * 1024]),
                16 * 1024 + std::mem::size_of::<CodecProbe>(),
                &[],
            )
        });
        unsafe {
            assert_eq!(
                perry_ffi::native_payload::payload_mut::<CodecProbe>(parser.get(), &CODEC)
                    .unwrap()
                    .0
                    .len(),
                16 * 1024
            );
            crate::native_transport::set_codec(owner.get(), "parser", parser.get(), close_codec);
        }
        assert_eq!(CODEC_DROPS.load(std::sync::atomic::Ordering::SeqCst), 0);
        socket::destroy(owner.get(), p::undefined());
        assert_eq!(CODEC_DROPS.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            perry_ffi::native_payload::lifecycle(parser.get(), &CODEC),
            Ok(perry_ffi::native_payload::Lifecycle::Closed)
        );
        socket::destroy(owner.get(), p::undefined());
        perry_runtime::promise::js_promise_run_microtasks();
        assert_eq!(CODEC_DROPS.load(std::sync::atomic::Ordering::SeqCst), 1);
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn saw_close(closure: *const RawClosureHeader, this: JsThis, _: f64) -> f64 {
    assert_eq!(
        this.as_f64().to_bits(),
        perry_ffi::closure_capture_f64(closure, 0).to_bits(),
        "close receives its original owner"
    );
    CLOSES.with(|closes| closes.set(closes.get() + 1));
    p::undefined()
}

fn close_listener(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(saw_close, 1; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    ) as i64);
    unsafe {
        perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 0, owner.get());
    }
    socket::once(owner.get(), "close", p::boxed_addr(callback.get()));
}

#[test]
fn idle_destroy_keeps_ordinary_identity_and_delivers_a_late_close_listener() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        CLOSES.with(|closes| closes.set(0));
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let link = socket::link(owner.get());
        p::own_set(owner.get(), "extra", 17.0);
        assert_eq!(
            socket::destroy(owner.get(), p::undefined()).to_bits(),
            owner.get().to_bits()
        );
        assert!(
            unsafe { p::socket_ptr(link) }.is_err(),
            "explicit destroy disposes native fields now"
        );
        assert_eq!(
            socket::link(owner.get()),
            link,
            "the released cell still belongs to this object"
        );
        assert_eq!(p::own_get(owner.get(), "extra"), 17.0);
        close_listener(owner.get());
        perry_runtime::promise::js_promise_run_microtasks();
        assert_eq!(
            CLOSES.with(Cell::get),
            1,
            "listener installed after destroy receives close"
        );
        perry_runtime::promise::js_promise_run_microtasks();
        assert_eq!(CLOSES.with(Cell::get), 1);
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn listener_reopens_the_same_cell_before_its_previous_closed_is_dispatched() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        CLOSES.with(|closes| closes.set(0));
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(server::new_server(
            io::ROUTE,
            p::undefined(),
            p::undefined(),
        ));
        let link = server::link(owner.get());
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        server::listen(owner.get(), 0.0, host.get(), p::undefined());
        let old = unsafe {
            perry_ffi::turnloop_net::link_snapshot_handle(&mut *p::server_core(link).unwrap(), link)
                .unwrap()
        };
        server::close(owner.get(), p::undefined());
        close_listener(owner.get());
        server::listen(owner.get(), 0.0, host.get(), p::undefined());
        assert_eq!(server::link(owner.get()), link);
        assert!(!unsafe {
            perry_ffi::turnloop_net::link_handle_matches(
                &mut *p::server_core(link).unwrap(),
                link,
                &old,
            )
        });
        let replacement = unsafe {
            perry_ffi::turnloop_net::link_snapshot_handle(&mut *p::server_core(link).unwrap(), link)
                .unwrap()
        };
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while CLOSES.with(Cell::get) == 0 {
            assert!(
                std::time::Instant::now() < limit,
                "old Closed was never dispatched"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            perry_runtime::promise::js_promise_run_microtasks();
        }
        assert_eq!(CLOSES.with(Cell::get), 1);
        assert!(
            unsafe {
                perry_ffi::turnloop_net::link_handle_matches(
                    &mut *p::server_core(link).unwrap(),
                    link,
                    &replacement,
                )
            },
            "old Closed must retain the replacement driver's handle"
        );
        assert_eq!(
            server::get(owner.get(), "listening").to_bits(),
            perry_ffi::JsValue::TRUE.bits()
        );
        server::close(owner.get(), p::undefined());
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn received_echo(_: *const RawClosureHeader, this: JsThis, chunk: f64) -> f64 {
    let owner = this.as_f64();
    assert_eq!(
        p::own_get(owner, "marker"),
        23.0,
        "the listener receives the original object"
    );
    let bytes = crate::jsvalue_to_socket_bytes(chunk).expect("data Buffer");
    assert_eq!(bytes, b"phase-A");
    ECHO_BYTES.with(|count| count.set(count.get() + bytes.len()));
    socket::destroy(owner, p::undefined());
    close_listener(owner); // Installed after explicit release, on a live TCP socket.
    p::undefined()
}

unsafe extern "C" fn churn_readable(_: *const RawClosureHeader, this: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(this.as_f64());
    assert_eq!(p::own_get(owner.get(), "churnOwner"), 11919.0);
    let value = socket::read(owner.get(), p::undefined());
    if let Some(bytes) = crate::jsvalue_to_socket_bytes(value) {
        assert!(
            bytes.iter().all(|byte| *byte == b'x'),
            "N3 echo bytes corrupted"
        );
        ECHO_BYTES.with(|count| count.set(count.get() + bytes.len()));
    }
    p::undefined()
}

#[cfg(target_os = "linux")]
fn rss_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .map(|rss| rss.split_whitespace().next().unwrap().parse().unwrap())
        })
        .unwrap()
}

/// Actual ordinary Socket owners and real loopback connections. Roots are
/// bounded by the concurrency, and all owners are released after each batch.
#[cfg(target_os = "linux")]
#[test]
fn n3_fifty_thousand_sequential_and_fifty_thousand_concurrent_sockets_release_everything() {
    use perry_runtime::native_payload::test_census;
    let whole_before = test_census::snapshot();
    let measured = std::thread::spawn(|| {
        use std::io::{Read, Write};
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            for _ in 0..124_576 {
                let (mut conn, _) = listener.accept().unwrap();
                conn.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
                let mut bytes = [0; 8192];
                conn.read_exact(&mut bytes).unwrap();
                assert!(bytes.iter().all(|byte| *byte == b'x'));
                conn.write_all(&bytes).unwrap();
                conn.shutdown(std::net::Shutdown::Write).unwrap();
                let mut tail = [0];
                assert_eq!(conn.read(&mut tail).unwrap(), 0);
            }
        });
        let scope = TransientRootScope::enter();
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        let message = scope.root_nanbox(f64::from_bits(perry_ffi::JsValue::from_object_ptr(
            perry_ffi::alloc_buffer(&vec![b'x'; 8192])).bits()));
        let readable = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(churn_readable, 0; with_flags(perry_ffi::FN_BUILTIN)), 0) as i64);
        let mut checkpoint = 0;
        let mut floor = 0;
        let mut peak = 0;
        let mut before = [0; 4];
        // Warm both allocation shapes and return to the sequential shape.
        // Each phase spans more than the collector's retained-block age
        // window, so a concurrency transition is not mistaken for a leak.
        for (total, concurrency, warmup) in [(8192, 1, true), (8192, 64, true), (8192, 1, true),
                                            (50_000, 1, false), (50_000, 64, false)] {
            if !warmup && floor == 0 {
                // The last warm-up checkpoint has already collected. A
                // second back-to-back full would cool the retained arena
                // blocks and measure rewarming instead of steady churn.
                assert_eq!(checkpoint, 0);
                floor = rss_kib(); peak = floor;
                before = test_census::snapshot();
                eprintln!("N3 warm rss_kib={floor} counts={before:?}");
            }
            let mut done = 0;
            while done < total {
                let n = concurrency.min(total - done);
                {
                    let batch = TransientRootScope::enter();
                    let closes = CLOSES.with(Cell::get);
                    let echoes = ECHO_BYTES.with(Cell::get);
                    let mut owners = Vec::with_capacity(n);
                    for _ in 0..n {
                        // This is the production exported factory, not a test identity.
                        let raw = unsafe { crate::js_net_socket_alloc() };
                        let owner = batch.root_nanbox(p::boxed_addr(raw));
                        p::own_set(owner.get(), "churnOwner", 11919.0);
                        let event = batch.root_nanbox(events::string("readable"));
                        unsafe { crate::payload_abi::js_net_socket_on(p::raw_owner(owner.get()), perry_ffi::JsValue::from_bits(event.get().to_bits()).as_string_ptr() as i64, readable.get()); }
                        assert_eq!(events::listener_count(owner.get(), "readable"), 1);
                        close_listener(owner.get());
                        socket::connect(owner.get(), port as f64, host.get(), p::undefined());
                        socket::end(owner.get(), message.get(), p::undefined(), p::undefined());
                        owners.push(owner);
                    }
                    let limit = std::time::Instant::now() + std::time::Duration::from_secs(30);
                    while CLOSES.with(Cell::get) < closes + n {
                        if std::time::Instant::now() >= limit {
                            for owner in &owners {
                                if let Ok(payload) = unsafe { p::socket_ptr(socket::link(owner.get())) } {
                                    let fields = unsafe { &(*payload).ext };
                                    eprintln!("N3 stalled done={done} total={total} connecting={} opened={} read_started={} read_ended={} read_end_emitted={} write_ended={} shutdown_done={} buffered={} bytes_read={} bytes_written={}", fields.connecting, fields.opened, fields.read_started, fields.read_ended, fields.read_end_emitted, fields.write_ended, fields.shutdown_done, fields.read_buffer.len(), fields.bytes_read, fields.bytes_written);
                                }
                            }
                            panic!("N3 connection or close stalled");
                        }
                        perry_runtime::event_pump::js_loop_turn_bounded(2);
                        perry_runtime::promise::js_promise_run_microtasks();
                    }
                    assert_eq!(ECHO_BYTES.with(Cell::get), echoes + n * 8192);
                    for owner in &owners {
                        let cell = socket::link(owner.get()).raw() as *const perry_runtime::native_handle::NativeHandleHeader;
                        unsafe {
                            assert!((*cell).resource_ptr.is_null(), "N3 explicit close retained T");
                            assert_eq!((*cell).external_bytes, 0);
                            assert_eq!((*cell).refs, 0, "N3 terminal completion retained a ref");
                        }
                    }
                    assert_eq!(perry_runtime::turnloop_net::live_handles(), 0, "N3 driver census retained a handle");
                }
                done += n; checkpoint += n;
                if checkpoint >= 1024 {
                    checkpoint = 0;
                    perry_runtime::gc::native_payload_test_collect_full();
                    if !warmup {
                        peak = peak.max(rss_kib());
                        assert!(peak.saturating_sub(floor) < 4096,
                            "N3 RSS grew by {} KiB after warm-up (total={total}, done={done}, counts={:?})", peak.saturating_sub(floor), test_census::snapshot());
                    }
                }
            }
        }
        peer.join().unwrap();
        perry_runtime::gc::native_payload_test_collect_full();
        let after = test_census::snapshot();
        assert_eq!(after[0] - before[0], 100_000);
        assert_eq!(after[2] - before[2], 100_000, "N3 not every T was dropped");
        assert_eq!(after[3], 0, "N3 total cell refs must be zero");
        eprintln!("N3 sequential=50000 concurrent=50000 width=64 rss_warm_kib={floor} rss_peak_kib={peak} delta_kib={} created=100000 drops=100000 live=0 refs=0", peak.saturating_sub(floor));
        perry_runtime::agent::retire_agent(agent);
        (before, after)
    }).join().unwrap();
    let final_counts = test_census::snapshot();
    // Thread heap teardown also collects the last conservative stack survivor.
    let created = final_counts[0] - whole_before[0];
    let finalized = final_counts[1] - whole_before[1];
    let drops = final_counts[2] - whole_before[2];
    assert_eq!(created, 124_576);
    assert_eq!(created, finalized, "N3 created cells must all be finalized");
    assert_eq!(created, drops, "N3 created cells must all drop their T");
    assert_eq!(final_counts[3], 0);
    assert_eq!(measured.1[0] - measured.0[0], 100_000);
    eprintln!("N3 after worker including warm-up: created={created} finalized={finalized} drops={drops} refs={}", final_counts[3]);
}

#[test]
fn connected_socket_echo_reopens_with_its_same_owner_and_cell() {
    std::thread::spawn(|| {
        use std::io::{Read, Write};
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        CLOSES.with(|count| count.set(0));
        ECHO_BYTES.with(|count| count.set(0));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut conn, _) = listener.accept().unwrap();
                conn.set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = [0; 7];
                conn.read_exact(&mut bytes).unwrap();
                assert_eq!(&bytes, b"phase-A");
                conn.write_all(&bytes).unwrap();
                let mut tail = [0];
                assert_eq!(
                    conn.read(&mut tail).unwrap(),
                    0,
                    "destroy closed the real peer descriptor"
                );
            }
        });
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let cell = socket::link(owner.get());
        p::own_set(owner.get(), "marker", 23.0);
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(received_echo, 1; with_flags(perry_ffi::FN_BUILTIN)),
            0,
        ) as i64);
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        let message = scope.root_nanbox(events::string("phase-A"));
        for round in 1..=2 {
            socket::once(owner.get(), "data", p::boxed_addr(callback.get()));
            socket::connect(owner.get(), port as f64, host.get(), p::undefined());
            socket::write(owner.get(), message.get(), p::undefined(), p::undefined());
            let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while CLOSES.with(Cell::get) < round {
                assert!(
                    std::time::Instant::now() < limit,
                    "echo/terminal close stalled"
                );
                perry_runtime::event_pump::js_loop_turn_bounded(2);
                perry_runtime::promise::js_promise_run_microtasks();
            }
            assert_eq!(socket::link(owner.get()), cell);
            assert!(unsafe { p::socket_ptr(cell) }.is_err());
            assert_eq!(ECHO_BYTES.with(Cell::get), 7 * round);
        }
        peer.join().unwrap();
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn n9_localhost_retry_and_pending_resolve_reopen_keep_identity_and_preconnect_writes() {
    std::thread::spawn(|| {
        use perry_runtime::turnloop_net::transport as core_test;
        use std::io::{Read, Write};
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        CLOSES.with(|count| count.set(0));
        ECHO_BYTES.with(|count| count.set(0));
        let old_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        old_listener.set_nonblocking(true).unwrap();
        let old_address = old_listener.local_addr().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut bytes = [0; 7];
            conn.read_exact(&mut bytes).unwrap();
            assert_eq!(
                &bytes, b"phase-A",
                "N9 only the reopened plan's write may arrive"
            );
            conn.write_all(&bytes).unwrap();
            let mut tail = [0];
            assert_eq!(conn.read(&mut tail).unwrap(), 0);
        });
        let scope = TransientRootScope::enter();
        let raw = unsafe { crate::js_net_socket_alloc() };
        let owner = scope.root_nanbox(p::boxed_addr(raw));
        let cell = socket::link(owner.get());
        let runtime_link = cell.raw();
        let host = scope.root_nanbox(events::string("localhost"));
        let old_message = scope.root_nanbox(events::string("oldplan"));
        let message = scope.root_nanbox(events::string("phase-A"));
        p::own_set(owner.get(), "marker", 23.0);
        socket::connect(
            owner.get(),
            old_address.port() as f64,
            host.get(),
            p::undefined(),
        );
        socket::write(
            owner.get(),
            old_message.get(),
            p::undefined(),
            p::undefined(),
        );
        let old_op = unsafe { core_test::pending_resolve_for_test(runtime_link) }
            .expect("N9 old resolve really pending");
        socket::destroy(owner.get(), p::undefined());
        close_listener(owner.get());
        socket::connect(owner.get(), port as f64, host.get(), p::undefined());
        assert_eq!(
            socket::link(owner.get()),
            cell,
            "N9 reopen retains its cell and object"
        );
        let new_op = unsafe { core_test::pending_resolve_for_test(runtime_link) }
            .expect("N9 replacement resolve really pending");
        assert_ne!(old_op, new_op, "N9 resolve incarnations differ");
        // Reproduce a delayed answer from the old operation deterministically,
        // before draining either real resolver result. No synthetic ref is owed.
        unsafe {
            core_test::replay_resolve_for_test(runtime_link, old_op, vec![old_address]);
        }
        assert_eq!(
            unsafe { core_test::pending_resolve_for_test(runtime_link) },
            Some(new_op),
            "N9 stale resolver result must not replace the new plan"
        );
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(received_echo, 1; with_flags(perry_ffi::FN_BUILTIN)),
            0,
        ) as i64);
        socket::once(owner.get(), "data", p::boxed_addr(callback.get()));
        socket::end(owner.get(), message.get(), p::undefined(), p::undefined());
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while CLOSES.with(Cell::get) < 2 {
            assert!(
                std::time::Instant::now() < limit,
                "N9 DNS retry or close stalled"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            perry_runtime::promise::js_promise_run_microtasks();
        }
        peer.join().unwrap();
        assert_eq!(ECHO_BYTES.with(Cell::get), 7);
        assert_eq!(
            old_listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "N9 the stale address must never get a connection"
        );
        assert_eq!(socket::link(owner.get()), cell);
        let header = cell.raw() as *const perry_runtime::native_handle::NativeHandleHeader;
        assert_eq!(
            unsafe { (*header).refs },
            0,
            "N9 both resolver completions and close release refs"
        );
        assert_eq!(perry_runtime::turnloop_net::live_handles(), 0);
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn n4_a_destroyed_socket_cell_cannot_be_reused_before_its_owed_closed() {
    std::thread::spawn(|| {
        use perry_runtime::native_payload::test_census;
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        CLOSES.with(|count| count.set(0));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let before = test_census::snapshot();
        let old_cell;
        {
            let scope = TransientRootScope::enter();
            let raw = unsafe { crate::js_net_socket_alloc() };
            let owner = scope.root_nanbox(p::boxed_addr(raw));
            let host = scope.root_nanbox(events::string("127.0.0.1"));
            socket::connect(owner.get(), port as f64, host.get(), p::undefined());
            old_cell = socket::link(owner.get()).raw();
            socket::destroy(owner.get(), p::undefined());
            close_listener(owner.get());
        }
        // No ordinary root remains. Only the completion's cell ref can keep
        // this owner graph alive across this collection and allocator churn.
        perry_runtime::gc::native_payload_test_collect_full();
        assert_eq!(
            test_census::snapshot()[1],
            before[1],
            "N4 an owed Closed must prevent cell finalization"
        );
        let cell = old_cell as *const perry_runtime::native_handle::NativeHandleHeader;
        assert_eq!(unsafe { (*cell).refs }, 1);
        assert!(unsafe { (*cell).resource_ptr.is_null() });
        for _ in 0..512 {
            let scope = TransientRootScope::enter();
            let raw = unsafe { crate::js_net_socket_alloc() };
            let replacement = scope.root_nanbox(p::boxed_addr(raw));
            assert_ne!(
                socket::link(replacement.get()).raw(),
                old_cell,
                "N4 a new object must not reuse an owed completion's cell"
            );
            socket::destroy(replacement.get(), p::undefined());
        }
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while CLOSES.with(Cell::get) == 0 {
            assert!(
                std::time::Instant::now() < limit,
                "N4 old owner's close stalled"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            perry_runtime::promise::js_promise_run_microtasks();
        }
        assert_eq!(
            CLOSES.with(Cell::get),
            1,
            "N4 the completion must reach its original object exactly once"
        );
        perry_runtime::gc::native_payload_test_collect_full();
        assert_eq!(perry_runtime::turnloop_net::live_handles(), 0);
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn retained_cork_capacity_is_reported_and_explicit_close_releases_it() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let link = socket::link(owner.get());
        let cell = link.raw() as *const perry_runtime::native_handle::NativeHandleHeader;
        let initial = unsafe { (*cell).external_bytes };
        assert!(initial >= std::mem::size_of::<p::SocketPayload>() as u64);
        socket::cork(owner.get(), false);
        let bytes = vec![b'x'; 16 * 1024];
        let chunk = scope.root_nanbox(f64::from_bits(
            perry_ffi::JsValue::from_object_ptr(perry_ffi::alloc_buffer(&bytes)).bits(),
        ));
        socket::write(owner.get(), chunk.get(), p::undefined(), p::undefined());
        assert!(
            unsafe { (*cell).external_bytes } >= initial + bytes.len() as u64,
            "the retained native cork buffer must contribute to GC pressure"
        );
        socket::destroy(owner.get(), p::undefined());
        assert_eq!(
            unsafe { (*cell).external_bytes },
            0,
            "explicit close releases both the buffer and its GC pressure now"
        );
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn saw_close_in_resource(
    closure: *const RawClosureHeader,
    _: JsThis,
    _: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let expected = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    assert_eq!(
        perry_runtime::async_hooks::js_async_hooks_execution_async_resource().to_bits(),
        expected.get().to_bits()
    );
    let expected_id = super::payload_provider::id(expected.get());
    assert_eq!(
        perry_runtime::async_hooks::js_async_hooks_execution_async_id(),
        expected_id as f64
    );
    CLOSES.with(|closes| closes.set(closes.get() + 1));
    p::undefined()
}

#[test]
fn idle_close_keeps_its_owned_async_resource_until_the_late_listener_runs() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        CLOSES.with(|count| count.set(0));
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let resource = scope.root_nanbox(super::payload_provider::resource(owner.get()));
        let id = super::payload_provider::id(resource.get());
        assert!(id > 0);
        assert!(perry_runtime::async_hooks::resource_tracked_for_test(id));
        socket::destroy(owner.get(), p::undefined());
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(saw_close_in_resource, 1; with_flags(perry_ffi::FN_BUILTIN)), 1) as i64);
        unsafe { perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 0, resource.get()); }
        socket::once(owner.get(), "close", p::boxed_addr(callback.get()));
        perry_runtime::promise::js_promise_run_microtasks();
        assert_eq!(CLOSES.with(Cell::get), 1);
        assert!(!perry_runtime::async_hooks::resource_tracked_for_test(id), "terminal delivery retires the existing provider metadata");
        assert_eq!(perry_runtime::async_hooks::js_async_hooks_execution_async_id(), 0.0);
        perry_runtime::agent::retire_agent(agent);
    }).join().unwrap();
}

#[test]
fn socket_and_server_initialize_the_actual_subclass_receiver() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        let scope = TransientRootScope::enter();
        for is_server in [false, true] {
            let raw = perry_runtime::object::js_object_alloc(12345, 0);
            let owner = scope.root_nanbox(perry_runtime::value::js_nanbox_pointer(raw as i64));
            let prototype = scope.root_nanbox(perry_runtime::object::js_object_get_prototype_of(
                owner.get(),
            ));
            p::own_set(owner.get(), "extra", 17.0);
            let result = if is_server {
                server::initialize_server(
                    io::ROUTE,
                    p::undefined(),
                    p::undefined(),
                    Some(owner.get()),
                )
            } else {
                socket::initialize_socket(io::ROUTE, p::undefined(), u64::MAX, Some(owner.get()))
            };
            assert_eq!(result.to_bits(), owner.get().to_bits());
            assert_eq!(
                perry_runtime::object::js_object_get_class_id(
                    p::raw_owner(owner.get()) as *const perry_runtime::ObjectHeader
                ),
                12345
            );
            assert_eq!(
                perry_runtime::object::js_object_get_prototype_of(owner.get()).to_bits(),
                prototype.get().to_bits()
            );
            assert_eq!(p::own_get(owner.get(), "extra"), 17.0);
            if is_server {
                server::close(owner.get(), p::undefined());
            } else {
                socket::destroy(owner.get(), p::undefined());
            }
            perry_runtime::promise::js_promise_run_microtasks();
            assert_eq!(p::own_get(owner.get(), "extra"), 17.0);
        }
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

thread_local! { static PIPE_BYTES: Cell<usize> = const { Cell::new(0) }; }
unsafe extern "C" fn pipe_write(closure: *const RawClosureHeader, this: JsThis, chunk: f64) -> f64 {
    assert_eq!(
        this.as_f64().to_bits(),
        perry_ffi::closure_capture_f64(closure, 0).to_bits()
    );
    let bytes = crate::jsvalue_to_socket_bytes(chunk).unwrap();
    PIPE_BYTES.with(|count| count.set(count.get() + bytes.len()));
    f64::from_bits(perry_ffi::JsValue::TRUE.bits())
}
#[test]
fn socket_pipe_forwards_data_to_an_ordinary_destination_and_unpipe_removes_it() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        PIPE_BYTES.with(|count| count.set(0));
        let scope = TransientRootScope::enter();
        let source = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let dest = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
        let write = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(pipe_write, 1; with_flags(perry_ffi::FN_BUILTIN)),
            1,
        ) as i64);
        unsafe {
            perry_ffi::set_closure_capture_f64(write.get() as *mut RawClosureHeader, 0, dest.get());
        }
        p::own_set(dest.get(), "write", p::boxed_addr(write.get()));
        assert_eq!(
            super::payload_pipe::pipe(source.get(), dest.get(), p::undefined()).to_bits(),
            dest.get().to_bits()
        );
        socket::data(source.get(), b"phase-A");
        assert_eq!(PIPE_BYTES.with(Cell::get), 7);
        super::payload_pipe::unpipe(source.get(), dest.get());
        socket::data(source.get(), b"not forwarded");
        assert_eq!(PIPE_BYTES.with(Cell::get), 7);
        socket::destroy(source.get(), p::undefined());
        perry_runtime::promise::js_promise_run_microtasks();
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn relisten_keeps_children_of_both_listener_incarnations_in_the_ownership_graph() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(server::new_server(
            io::ROUTE,
            p::undefined(),
            p::undefined(),
        ));
        let cell = server::link(owner.get());
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        let mut peers = Vec::new();
        for count in 1..=2 {
            server::listen(owner.get(), 0.0, host.get(), p::undefined());
            assert_eq!(server::link(owner.get()), cell);
            let port = unsafe {
                perry_ffi::turnloop_net::link_local_address(
                    &mut *p::server_core(cell).unwrap(),
                    cell,
                )
                .unwrap()
                .port
            };
            peers.push(std::net::TcpStream::connect(("127.0.0.1", port)).unwrap());
            let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let mut observed = 0;
                crate::native_transport::for_each_server_child(owner.get(), |_| observed += 1);
                if observed == count {
                    break;
                }
                assert!(
                    std::time::Instant::now() < limit,
                    "ownership graph must include children from the closed listener incarnation"
                );
                perry_runtime::event_pump::js_loop_turn_bounded(2);
                perry_runtime::promise::js_promise_run_microtasks();
            }
            if count == 1 {
                server::close(owner.get(), p::undefined());
            }
        }
        let mut destroyed = 0;
        crate::native_transport::for_each_server_child(owner.get(), |child| {
            socket::destroy(child.value(), p::undefined());
            destroyed += 1;
        });
        assert_eq!(destroyed, 2);
        server::close(owner.get(), p::undefined());
        drop(peers);
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn n6_socket_worker_teardown_with_closed_queued_releases_fds_without_js() {
    fn fds() -> usize {
        std::fs::read_dir("/proc/self/fd").unwrap().count()
    }
    let baseline = fds();
    std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        CLOSES.with(|count| count.set(0));
        let scope = TransientRootScope::enter();
        let raw = unsafe { crate::js_net_create_server(0, 0) };
        let owner = scope.root_nanbox(p::boxed_addr(raw));
        close_listener(owner.get());
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        server::listen(owner.get(), 0.0, host.get(), p::undefined());
        let cell = server::link(owner.get());
        let port = unsafe {
            perry_ffi::turnloop_net::link_local_address(&mut *p::server_core(cell).unwrap(), cell)
                .unwrap()
                .port
        };
        let mut clients = Vec::new();
        for _ in 0..64 {
            let raw = unsafe { crate::js_net_socket_alloc() };
            let client = scope.root_nanbox(p::boxed_addr(raw));
            close_listener(client.get());
            socket::connect(client.get(), port as f64, host.get(), p::undefined());
            clients.push(client);
        }
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut accepted = 0;
            crate::native_transport::for_each_server_child(owner.get(), |_| accepted += 1);
            if accepted == 64 {
                break;
            }
            assert!(std::time::Instant::now() < limit, "N6 accept stalled");
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            perry_runtime::promise::js_promise_run_microtasks();
        }
        let live = fds();
        assert!(
            live >= baseline + 129,
            "N6 must actually own 129 socket descriptors"
        );
        for client in &clients {
            socket::destroy(client.get(), p::undefined());
        }
        crate::native_transport::for_each_server_child(owner.get(), |child| {
            close_listener(child.value());
            socket::destroy(child.value(), p::undefined());
        });
        server::close(owner.get(), p::undefined());
        assert_eq!(CLOSES.with(Cell::get), 0, "N6 Closed really remains queued");
        perry_runtime::agent::retire_agent(agent);
        assert_eq!(CLOSES.with(Cell::get), 0, "N6 teardown may not dispatch JS");
        assert_eq!(
            fds(),
            baseline,
            "N6 worker teardown must release socket and driver descriptors before returning"
        );
        eprintln!(
            "N6 Socket owners: fd baseline={baseline} live={live} retired={} JS close callbacks=0",
            fds()
        );
    })
    .join()
    .unwrap();
    assert_eq!(
        fds(),
        baseline,
        "N6 the main thread continues without leaked descriptors"
    );
}

#[test]
fn tls_wrapper_keeps_the_parent_cell_handle_and_pending_read_and_disposes_both_payloads() {
    std::thread::spawn(|| {
        use perry_ffi::native_payload as np;
        let agent = perry_runtime::agent::enter_worker_agent();
        CLOSES.with(|count| count.set(0));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let scope = TransientRootScope::enter();
        let parent = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let parent_link = p::socket_link(parent.get()).unwrap();
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        socket::connect(
            parent.get(),
            listener.local_addr().unwrap().port() as f64,
            host.get(),
            p::undefined(),
        );
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !unsafe { (*p::socket_ptr(parent_link).unwrap()).ext.opened } {
            assert!(
                std::time::Instant::now() < limit,
                "TLS parent connect stalled"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            perry_runtime::promise::js_promise_run_microtasks();
        }
        let (peer, _) = listener.accept().unwrap();
        socket::set_paused(parent.get(), false);
        let before = unsafe {
            let core = &*(p::socket_core(parent_link).unwrap()
                as *const perry_runtime::turnloop_net::transport::TransportCore);
            (
                format!("{:?}", core.handle()),
                format!("{:?}", core.read_op()),
            )
        };
        assert_ne!(before.1, "None", "TLS wrapper must inherit an armed read");
        let wrapper = scope.root_nanbox(socket::new_tls_wrapper(parent.get()));
        let wrapper_link = p::socket_link(wrapper.get()).unwrap();
        assert_ne!(wrapper.get().to_bits(), parent.get().to_bits());
        assert_ne!(
            wrapper_link, parent_link,
            "the wrapper has its own ordinary payload owner"
        );
        assert_eq!(socket::link(wrapper.get()), parent_link);
        assert_eq!(
            unsafe { np::link_event_owner(parent_link) }
                .unwrap()
                .to_bits(),
            parent.get().to_bits()
        );
        let after = unsafe {
            let core = &*(p::socket_core(parent_link).unwrap()
                as *const perry_runtime::turnloop_net::transport::TransportCore);
            assert_eq!(core.route(), io::TLS_SUBSYSTEM);
            (
                format!("{:?}", core.handle()),
                format!("{:?}", core.read_op()),
            )
        };
        assert_eq!(
            before, after,
            "TLS wrapping changes only the parent core route"
        );
        socket::destroy(parent.get(), p::undefined());
        assert_eq!(
            np::lifecycle(parent.get(), &p::SOCKET),
            Ok(np::Lifecycle::Closed)
        );
        assert_eq!(
            np::lifecycle(wrapper.get(), &p::SOCKET),
            Ok(np::Lifecycle::Closed)
        );
        close_listener(parent.get());
        close_listener(wrapper.get());
        while CLOSES.with(Cell::get) != 2 {
            assert!(
                std::time::Instant::now() < limit,
                "TLS wrapper/parent late close stalled"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            perry_runtime::promise::js_promise_run_microtasks();
        }
        assert_eq!(
            unsafe {
                (*(parent_link.raw() as *const perry_runtime::native_handle::NativeHandleHeader))
                    .refs
            },
            0
        );
        assert_eq!(
            unsafe {
                (*(wrapper_link.raw() as *const perry_runtime::native_handle::NativeHandleHeader))
                    .refs
            },
            0
        );
        drop(peer);
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn reopen_parent_on_wrapper_close(
    closure: *const RawClosureHeader,
    _: JsThis,
    _: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let parent = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let port = perry_ffi::closure_capture_f64(closure, 1);
    let host = scope.root_nanbox(events::string("127.0.0.1"));
    socket::connect(parent.get(), port, host.get(), p::undefined());
    CLOSES.with(|closes| closes.set(closes.get() + 1));
    p::undefined()
}

#[test]
fn tls_wrapper_close_listener_can_reopen_its_parent_before_the_parent_close_listener() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        CLOSES.with(|count| count.set(0));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let scope = TransientRootScope::enter();
        let parent = scope.root_nanbox(socket::new_socket(io::ROUTE, p::undefined()));
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        let port = listener.local_addr().unwrap().port() as f64;
        socket::connect(parent.get(), port, host.get(), p::undefined());
        let parent_link = socket::link(parent.get());
        let wrapper = scope.root_nanbox(socket::new_tls_wrapper(parent.get()));
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(reopen_parent_on_wrapper_close, 1; with_flags(perry_ffi::FN_BUILTIN)), 2) as i64);
        unsafe {
            perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 0, parent.get());
            perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 1, port);
        }
        socket::once(wrapper.get(), "close", p::boxed_addr(callback.get()));
        socket::destroy(wrapper.get(), p::undefined());
        close_listener(parent.get());
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while CLOSES.with(Cell::get) != 2 {
            assert!(std::time::Instant::now() < limit, "TLS wrapper reentrant reopen stalled");
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            perry_runtime::promise::js_promise_run_microtasks();
        }
        assert_eq!(socket::link(parent.get()), parent_link);
        assert_eq!(perry_ffi::native_payload::lifecycle(parent.get(), &p::SOCKET), Ok(perry_ffi::native_payload::Lifecycle::Open));
        assert!(unsafe { perry_ffi::turnloop_net::link_handle_parts(&mut *p::socket_core(parent_link).unwrap(), parent_link) }.is_some(), "the old wrapper/parent close may not retire the replacement handle");
        let old_write = scope.root_nanbox(events::string("old wrapper"));
        assert_eq!(socket::write(wrapper.get(), old_write.get(), p::undefined(), p::undefined()).to_bits(), perry_ffi::JsValue::FALSE.bits(), "a disposed wrapper may not write to its reopened parent's new handle");
        assert_ne!(socket::link(wrapper.get()), parent_link);
        socket::destroy(parent.get(), p::undefined());
        perry_runtime::agent::retire_agent(agent);
    }).join().unwrap();
}
