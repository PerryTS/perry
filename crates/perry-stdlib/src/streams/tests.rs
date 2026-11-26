use super::transform::{run_web_compression_codec, split_utf8_prefix};
use super::*;

/// Stream tests share process-global registries and allocator state. Serialize
/// tests that mutate allocator pools or install scanner fixtures with fake
/// roots so parallel collections cannot observe or erase their temporary data.
static ALLOCATOR_TEST_SERIAL: Mutex<()> = Mutex::new(());

pub(crate) fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    let guard = ALLOCATOR_TEST_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    crate::test_runtime::initialize();
    guard
}

fn alloc_closed_readable() -> usize {
    let id = alloc_readable(0, 0, 0, 1.0);
    with_rs(RsRef(id), |s| s.state = ReadableState::Closed).unwrap();
    id
}

/// A closed writable stream (the band ids recycle through `idalloc`).
fn alloc_closed_writable() -> usize {
    let id = alloc_writable(0, 0, 0, 1.0);
    with_ws(WsRef(id), |s| s.state = WritableState::Closed).unwrap();
    id
}

#[test]
fn stream_ids_live_outside_pointer_tag_small_handle_band() {
    // Allocates from the shared pools — serialize so this can't steal the
    // free-list id the recycle test below asserts on.
    let _serial = serial_guard();
    let id = next_stream_id();
    assert!(
        (STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id),
        "stream id {id:#x} must stay in the stream band"
    );
    assert!(
        perry_runtime::value::addr_class::is_stream_id_band(id)
            && !perry_runtime::value::addr_class::is_proxy_cell(id)
            && !perry_runtime::value::addr_class::is_fetch_handle_band(id),
        "stream id {id:#x} must not overlap the other handle sub-bands"
    );
}

#[test]
fn terminal_stream_ids_recycle_after_quarantine_eviction() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    idalloc::set_quarantine_limit_for_test(2);
    let ids: Vec<usize> = (0..3)
        .map(|_| {
            let id = alloc_closed_writable();
            idalloc::retire_writable_terminal(id);
            id
        })
        .collect();
    idalloc::set_quarantine_limit_for_test(idalloc::DEFAULT_QUARANTINE);
    // The third retire overflowed the quarantine of 2: the oldest id lost its
    // registry entry (eviction really cleans up)…
    assert!(
        !ws_exists(WsRef(ids[0])),
        "evicted id must leave the registry"
    );
    assert!(
        ws_exists(WsRef(ids[2])),
        "quarantined id keeps its registry entry"
    );
    // …and the allocator hands it back, oldest first, before any fresh id.
    let reused = next_stream_id();
    assert_eq!(reused, ids[0], "evicted id recycles FIFO");
    assert!((STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&reused));
    // With the free list drained the allocator is back on fresh ids.
    let fresh = alloc_writable(0, 0, 0, 1.0);
    assert_ne!(fresh, reused);
}

#[test]
fn retire_is_idempotent_per_id() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    idalloc::set_quarantine_limit_for_test(idalloc::DEFAULT_QUARANTINE);
    let id = alloc_closed_writable();
    idalloc::retire_writable_terminal(id);
    idalloc::retire_writable_terminal(id);
    assert_eq!(
        idalloc::test_pool_occurrences(id),
        1,
        "an id may sit in the pools at most once — twice means double-alloc"
    );
}

#[test]
fn live_stream_ids_never_enter_the_pool() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    let id = alloc_writable(0, 0, 0, 1.0);
    idalloc::retire_writable_terminal(id); // Writable → not terminal
    assert_eq!(idalloc::test_pool_occurrences(id), 0);
}

/// #12862: a keep-alive fetch whose small body is fully queued and closed
/// before `text()` runs: the body promise resolves with the whole body.
#[test]
fn whole_body_collected_after_close_resolves() {
    fn no_source(_: usize) {}
    fn byte_count(bytes: Vec<u8>, _: String) -> Result<f64, f64> {
        Ok(bytes.len() as f64)
    }
    let _serial = serial_guard();
    for _ in 0..64 {
        let (id, ctx) = alloc_native_readable(js_promise_new(), no_source, no_source, None);
        take_native_promise(id);
        let body = js_promise_new();
        unsafe {
            native_enqueue(id, b"0123456789abcdef");
            js_readable_stream_controller_close(id.controller_value());
            collect_body(id, body, String::new(), byte_count);
        }
        assert_eq!(js_promise_state(body), 1, "the body promise resolves");
        assert_eq!(js_promise_value(body), 16.0, "with the whole queued body");
        release_native(ctx);
    }
}

/// Native bodies paused at zero credit whose only references are dropped on
/// return: their sink contexts.
#[inline(never)]
fn paused_native_bodies(count: usize, dropped: &std::rc::Rc<std::cell::Cell<u32>>) -> Vec<usize> {
    fn no_source(_: usize) {}
    let chunk = vec![7u8; native::NATIVE_BODY_WINDOW];
    (0..count)
        .map(|_| {
            let (id, ctx) = alloc_native_readable(
                js_promise_new(),
                no_source,
                no_source,
                Some(dropped.clone()),
            );
            take_native_promise(id);
            unsafe { native_enqueue(id, &chunk) };
            assert_eq!(native_capacity(ctx), 0, "a full window pauses the producer");
            ctx
        })
        .collect()
}

/// Native bodies still delivering (pinned by their producer): their sink
/// contexts.
#[inline(never)]
fn delivering_native_bodies(count: usize) -> Vec<usize> {
    fn no_source(_: usize) {}
    (0..count)
        .map(|_| alloc_native_readable(js_promise_new(), no_source, no_source, None).1)
        .collect()
}

#[inline(never)]
fn scrub_stack() -> u64 {
    let words = std::hint::black_box([0u64; 512]);
    words.iter().sum()
}

/// GC objects: an unreferenced ReadableStream is reclaimed by the collector
/// (its payload drops), and a dropped fetch body paused at capacity tells its
/// producer so (`alive` turns false; the engine then aborts the exchange). A
/// body that is still delivering stays pinned by its producer.
#[test]
fn unreferenced_paused_native_body_is_reclaimed_and_reports_dead() {
    let _serial = serial_guard();
    let dropped = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let paused = paused_native_bodies(32, &dropped);
    let delivering = delivering_native_bodies(8);
    scrub_stack();
    perry_runtime::gc::js_gc_collect();
    perry_runtime::gc::js_gc_collect();
    let dead = paused.iter().filter(|&&ctx| !native_alive(ctx)).count();
    assert_eq!(
        dropped.get() as usize,
        dead,
        "every reclaimed body counts its death on the producer's cell"
    );
    // A stray conservative stack word may pin one; reclaim is the rule.
    assert!(
        dead >= 24,
        "only {dead} of 32 dropped paused bodies were reclaimed"
    );
    assert!(
        paused
            .iter()
            .all(|&ctx| native_alive(ctx) || native_stream(ctx).is_none()),
        "a dead body names no stream"
    );
    assert!(
        delivering.iter().all(|&ctx| native_alive(ctx)),
        "a pinned body must survive collection"
    );
    for ctx in paused.into_iter().chain(delivering) {
        release_native(ctx);
    }
}

/// A stream's JS values live in its slot array: they survive collections
/// (traced through the object) and are what its accessors read back.
#[test]
fn readable_object_slots_hold_queue_requests_and_controller() {
    let _serial = serial_guard();
    // Unit tests have no conservative stack scan: root the stream.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream_value(alloc_readable(0, 0, 0, 1.0)));
    let chunk = scope.root_nanbox_u64(unsafe { alloc_uint8array_from_bytes(b"abc") });
    let promise = scope.root_raw_mut_ptr(js_promise_new());
    with_rs(RsRef::from_value(stream.get_nanbox_f64()), |s| {
        s.push_chunk(chunk.get_nanbox_u64(), 3.0);
        s.push_read_request(promise.get_raw_mut_ptr());
    })
    .unwrap();
    perry_runtime::gc::gc_collect_minor();
    perry_runtime::gc::js_gc_collect();
    let id = stream_id(stream.get_nanbox_f64());
    with_rs(RsRef(id), |s| {
        assert_eq!(s.chunk_count(), 1);
        assert_eq!(s.queue_total_size, 3.0);
        assert_eq!(s.read_request_count(), 1);
        let popped = s.pop_chunk().unwrap();
        assert_eq!(
            unsafe { read_bytes_from_chunk(popped) }.as_deref(),
            Some(&b"abc"[..])
        );
        assert!(s.shift_read_request().is_some());
        assert!(s.chunks_empty());
    })
    .unwrap();
    assert_eq!(
        objects::class_id_of(RsRef(id).value()),
        perry_runtime::native_class_ids::WEB_READABLE_STREAM
    );
    assert_eq!(
        controller_stream_id(controller_value(id)),
        id,
        "the controller names its stream"
    );
}

#[test]
fn transform_root_scanner_writes_relocated_pointers_back() {
    struct RelocatingVisitor;

    impl super::gc::StreamRootVisitor for RelocatingVisitor {
        fn visit_i64_slot(&mut self, slot: &mut i64) {
            *slot += 0x1000;
        }

        fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T) {
            *slot = ((*slot as usize) + 0x1000) as *mut T;
        }

        fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) {
            *slot += 0x1000;
        }
    }

    // The transform bookkeeping of a writable (deferred close) is traced with
    // its record, and a moving visitor's rewrite lands in the record (a
    // readable's lives in its object's slots).
    let mut writable =
        WritableStreamData::new(0, 0, 0, 0, 1.0, std::ptr::null_mut(), std::ptr::null_mut());
    writable.transform_pending_close = Some(0x3100 as *mut Promise);
    writable.trace(&mut RelocatingVisitor);
    assert_eq!(
        writable.transform_pending_close,
        Some(0x4100 as *mut Promise)
    );
}

#[test]
fn web_compression_formats_round_trip() {
    let input = b"hello stream/web compression";
    let mut formats = vec![
        WebCompressionFormat::Gzip,
        WebCompressionFormat::Deflate,
        WebCompressionFormat::DeflateRaw,
    ];
    #[cfg(feature = "streams-brotli")]
    formats.push(WebCompressionFormat::Brotli);
    for format in formats {
        let compressed = run_web_compression_codec(format, false, input).unwrap();
        assert!(!compressed.is_empty());
        let decompressed = run_web_compression_codec(format, true, &compressed).unwrap();
        assert_eq!(decompressed, input);
    }
}

#[test]
fn utf8_split_prefix_tracks_incomplete_sequence() {
    assert_eq!(split_utf8_prefix(&[0x68, 0xc3]).unwrap(), (1, true));
    assert_eq!(split_utf8_prefix(&[0xc3, 0xa9]).unwrap(), (2, false));
    assert!(split_utf8_prefix(&[0xff]).is_err());
}

#[test]
fn transform_terminate_closes_readable_and_errors_writable() {
    let _serial = serial_guard();
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let transform =
        unsafe { js_transform_stream_new(undefined, undefined, undefined, undefined, undefined) };
    let readable = unsafe { js_transform_stream_readable(transform) };
    let writable = unsafe { js_transform_stream_writable(transform) };

    assert!(unsafe { dispatch_stream_method(readable, "terminate", &[]) }.is_none());
    let controller = controller_value(stream_id(readable));
    assert!(unsafe { dispatch_stream_method(controller, "terminate", &[]) }.is_some());
    assert!(matches!(
        rs_rec(RsRef(stream_id(readable))).as_deref().unwrap().state,
        ReadableState::Closed
    ));
    assert!(matches!(
        ws_rec(WsRef(stream_id(writable))).as_deref().unwrap().state,
        WritableState::Errored
    ));
}

#[test]
fn enqueue_rejects_closed_readable_controller() {
    let _serial = serial_guard();
    let readable = alloc_closed_readable();

    assert_eq!(
        readable_controller_enqueue_error(readable),
        Some("Invalid state: Controller is already closed")
    );
}

#[test]
fn pipe_through_rejects_locked_endpoints_before_starting() {
    let _serial = serial_guard();
    let source = alloc_readable(0, 0, 0, 1.0);
    let output = alloc_readable(0, 0, 0, 1.0);
    let destination = alloc_writable(0, 0, 0, 1.0);

    rs_rec(RsRef(source))
        .as_deref_mut()
        .unwrap()
        .set_reader_handle(Some(1));
    assert_eq!(
        pipe_through_validation_error(source, destination, output),
        Some("ReadableStream is locked")
    );

    rs_rec(RsRef(source))
        .as_deref_mut()
        .unwrap()
        .set_reader_handle(None);
    ws_rec(WsRef(destination))
        .as_deref_mut()
        .unwrap()
        .writer_handle = Some(2);
    assert_eq!(
        pipe_through_validation_error(source, destination, output),
        Some("WritableStream is locked")
    );

    let invalid_signal = js_object_alloc(0, 0);
    assert_eq!(
        pipe_through_signal_error(f64::from_bits(
            JSValue::object_ptr(invalid_signal as *mut u8).bits(),
        )),
        Some("The options.signal property must be an AbortSignal")
    );
}

#[test]
fn buffered_tee_demand_skips_only_the_cold_source_hop() {
    let _serial = serial_guard();
    // Keep this assertion about jobs scheduled by the calls below, not work
    // left behind by an earlier stream test on the same worker thread.
    perry_runtime::promise::js_promise_run_microtasks();

    let buffered = alloc_readable(0, 0, 0, 1.0);
    rs_rec(RsRef(buffered))
        .as_deref_mut()
        .unwrap()
        .push_chunk(TAG_UNDEFINED, 1.0);
    unsafe {
        tee::tee_schedule_pull_demand(buffered);
    }
    let buffered_jobs = perry_runtime::promise::js_promise_run_microtasks();

    let empty = alloc_readable(0, 0, 0, 1.0);
    unsafe {
        tee::tee_schedule_pull_demand(empty);
    }
    let empty_jobs = perry_runtime::promise::js_promise_run_microtasks();

    assert_eq!(
        buffered_jobs, 1,
        "a pre-buffered source needs only the fanout reaction job"
    );
    assert_eq!(
        empty_jobs, 2,
        "an empty cold source must retain the Flight-calibrated demand hop"
    );

    state::test_remove(&[buffered, empty]);
}

/// Not a NaN-boxed value any call passes: "`next()` never ran".
const NEXT_NOT_CALLED: f64 = 0.5;

/// Records the receiver it was called with in its own capture slot 0 (a slot
/// the collector sees), so no static holds a heap value.
extern "C" fn next_recording_this(
    closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
) -> f64 {
    perry_runtime::closure::js_closure_set_capture_f64(
        closure as *mut ClosureHeader,
        0,
        this.as_f64(),
    );
    f64::from_bits(TAG_UNDEFINED)
}

/// An async-iterable source's `next()` is a METHOD call: the iterator is its
/// receiver. The body reads `this` only from its receiver parameter, so this
/// fails if `call_iterator_next` makes a plain call (the body would see
/// `undefined`).
#[test]
fn iterator_next_is_called_with_the_iterator_as_its_receiver() {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let next = scope.root_raw_mut_ptr(js_closure_alloc(
        perry_runtime::fn_info!(next_recording_this, 0; with_declared(0)),
        1,
    ));
    perry_runtime::closure::js_closure_set_capture_f64(
        next.get_raw_mut_ptr::<ClosureHeader>(),
        0,
        NEXT_NOT_CALLED,
    );
    let iterator_obj = scope.root_raw_mut_ptr(js_object_alloc(0, 1));
    let key = js_string_from_bytes(b"next".as_ptr(), 4);
    let next_value = f64::from_bits(
        JSValue::pointer(next.get_raw_mut_ptr::<ClosureHeader>() as *const u8).bits(),
    );
    perry_runtime::object::js_object_set_field_by_name(
        iterator_obj.get_raw_mut_ptr::<ObjectHeader>(),
        key,
        next_value,
    );
    let iterator = scope.root_nanbox_f64(f64::from_bits(
        JSValue::object_ptr(iterator_obj.get_raw_mut_ptr::<ObjectHeader>() as *mut u8).bits(),
    ));

    let result = unsafe { call_iterator_next(iterator.get_nanbox_f64()) };
    assert!(result.is_some());
    let seen = perry_runtime::closure::js_closure_get_capture_f64(
        next.get_raw_mut_ptr::<ClosureHeader>(),
        0,
    );
    assert_eq!(
        seen.to_bits(),
        iterator.get_nanbox_f64().to_bits(),
        "next() must see the iterator as `this`"
    );
}

extern "C" fn collecting_pair_getter(
    closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
) -> f64 {
    // Only a numeric stream handle is held in Rust across the collection.
    let endpoint = perry_runtime::closure::js_closure_get_capture_f64(closure, 0);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let getter = scope.root_raw_const_ptr(closure);
    let receiver = scope.root_nanbox_f64(this.as_f64());
    let receiver_before = receiver.get_nanbox_f64().to_bits();
    let calls = perry_runtime::closure::js_closure_get_capture_f64(closure, 1);
    perry_runtime::closure::js_closure_set_capture_f64(
        closure as *mut ClosureHeader,
        1,
        calls + 1.0,
    );
    perry_runtime::gc::gc_collect_minor();
    getter.with_const_ptr::<ClosureHeader, _>(|closure| {
        perry_runtime::closure::js_closure_set_capture_f64(
            closure as *mut ClosureHeader,
            2,
            (receiver.get_nanbox_f64().to_bits() != receiver_before) as u8 as f64,
        );
    });
    endpoint
}

#[test]
fn pipe_through_pair_survives_a_moving_getter() {
    let _serial = serial_guard();
    // Rust unit tests bypass the generated program's startup. Register the
    // handle/accessor roots before the getters deliberately collect.
    perry_runtime::gc::gc_init();
    struct RestoreGc(i32);
    impl Drop for RestoreGc {
        fn drop(&mut self) {
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
            perry_runtime::gc::js_gc_force_evacuation_test_override(self.0);
        }
    }
    let _restore = RestoreGc(perry_runtime::gc::js_gc_force_evacuation_test_override(1));
    perry_runtime::gc::js_gc_write_barriers_emitted(1);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    unsafe {
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let transform =
            js_transform_stream_new(undefined, undefined, undefined, undefined, undefined);
        let readable = js_transform_stream_readable(transform);
        let writable = js_transform_stream_writable(transform);
        let pair = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
        let getter = scope.root_raw_mut_ptr(js_closure_alloc(
            perry_runtime::fn_info!(collecting_pair_getter, 0; with_declared(0)),
            3,
        ));
        perry_runtime::closure::js_closure_set_capture_f64(
            getter.get_raw_mut_ptr::<ClosureHeader>(),
            0,
            readable,
        );
        for slot in [1, 2] {
            perry_runtime::closure::js_closure_set_capture_f64(
                getter.get_raw_mut_ptr::<ClosureHeader>(),
                slot,
                0.0,
            );
        }
        let key = js_string_from_bytes(b"readable".as_ptr(), 8);
        perry_runtime::object::js_object_define_accessor(
            f64::from_bits(
                JSValue::pointer(pair.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits(),
            ),
            f64::from_bits(JSValue::string_ptr(key).bits()),
            f64::from_bits(
                JSValue::pointer(getter.get_raw_mut_ptr::<ClosureHeader>() as *const u8).bits(),
            ),
            undefined,
        );
        let writer_getter = scope.root_raw_mut_ptr(js_closure_alloc(
            perry_runtime::fn_info!(collecting_pair_getter, 0; with_declared(0)),
            3,
        ));
        perry_runtime::closure::js_closure_set_capture_f64(
            writer_getter.get_raw_mut_ptr::<ClosureHeader>(),
            0,
            writable,
        );
        for slot in [1, 2] {
            perry_runtime::closure::js_closure_set_capture_f64(
                writer_getter.get_raw_mut_ptr::<ClosureHeader>(),
                slot,
                0.0,
            );
        }
        let key = js_string_from_bytes(b"writable".as_ptr(), 8);
        perry_runtime::object::js_object_define_accessor(
            f64::from_bits(
                JSValue::pointer(pair.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits(),
            ),
            f64::from_bits(JSValue::string_ptr(key).bits()),
            f64::from_bits(
                JSValue::pointer(writer_getter.get_raw_mut_ptr::<ClosureHeader>() as *const u8)
                    .bits(),
            ),
            undefined,
        );
        let options = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
        let pair_before = pair.get_raw_mut_ptr::<ObjectHeader>();
        // Streams are GC objects: a unit test has no conservative stack scan
        // (no registered stack top), so every stream held across the moving
        // getters is rooted.
        let readable = scope.root_nanbox_f64(readable);
        let source = scope.root_nanbox_f64(stream_value(alloc_closed_readable()));
        let output = js_readable_stream_pipe_through_pair(
            source.get_nanbox_f64(),
            f64::from_bits(
                JSValue::pointer(pair.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits(),
            ),
            f64::from_bits(
                JSValue::pointer(options.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits(),
            ),
        );
        assert_eq!(output.to_bits(), readable.get_nanbox_f64().to_bits());
        assert_ne!(
            pair.get_raw_mut_ptr::<ObjectHeader>(),
            pair_before,
            "the getters must have moved the transform pair"
        );
        for getter in [getter, writer_getter] {
            getter.with_mut_ptr::<ClosureHeader, _>(|closure| {
                assert_eq!(
                    perry_runtime::closure::js_closure_get_capture_f64(closure, 1),
                    1.0,
                    "each endpoint getter must run exactly once"
                );
                assert_eq!(
                    perry_runtime::closure::js_closure_get_capture_f64(closure, 2),
                    1.0,
                    "each endpoint getter must move its receiver"
                );
            });
        }
    }
}

#[test]
fn generic_stream_dispatch_preserves_owner_and_controller_alias() {
    let _serial = serial_guard();
    let undefined = f64::from_bits(TAG_UNDEFINED);
    unsafe {
        let transform =
            js_transform_stream_new(undefined, undefined, undefined, undefined, undefined);
        let readable = js_transform_stream_readable(transform);
        let writable = js_transform_stream_writable(transform);
        let reader = js_readable_stream_get_reader_with_options(readable, undefined);
        let writer = js_writable_stream_get_writer(writable);
        for (handle, wrong_method) in [
            (readable, "write"),
            (readable, "releaseLock"),
            (writable, "enqueue"),
            (writable, "read"),
            (reader, "close"),
            (reader, "getReader"),
            (writer, "cancel"),
            (writer, "enqueue"),
            (transform, "close"),
        ] {
            assert!(dispatch_stream_method(handle, wrong_method, &[]).is_none());
            assert!(dispatch_stream_method(handle, "unknown", &[]).is_none());
        }
        // RSTREAM R3: controller methods answer on the controller, never on
        // the stream; a TransformStream's controller has terminate, not close.
        let controller = controller_value(stream_id(readable));
        for method in ["enqueue", "close", "terminate", "error"] {
            assert!(dispatch_stream_method(readable, method, &[42.0]).is_none());
        }
        assert!(dispatch_stream_method(controller, "getReader", &[]).is_none());
        assert!(dispatch_stream_method(controller, "close", &[]).is_none());
        assert_eq!(js_stream_handle_kind(stream_id(controller)), 7);
        assert!(dispatch_stream_method(controller, "enqueue", &[42.0]).is_some());
        assert!(dispatch_stream_method(reader, "releaseLock", &[]).is_some());
        assert!(dispatch_stream_method(writer, "releaseLock", &[]).is_some());
        assert!(dispatch_stream_method(controller, "terminate", &[]).is_some());
        assert!(matches!(
            rs_rec(RsRef(stream_id(readable))).as_deref().unwrap().state,
            ReadableState::Closed
        ));
    }
}

/// Stream chunks use Uint8Array placement at birth, including Native backing.
#[test]
fn stream_chunk_uses_uint8array_placement_at_birth() {
    use perry_runtime::buffer::bytes;
    use perry_runtime::codegen_abi::{BYTES_OUT_OF_LINE, BYTES_TYPE_VIEW};
    let input: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
    for len in [0, 1, 127, 12 * 1024, input.len()] {
        let bits = unsafe { alloc_uint8array_from_bytes(&input[..len]) };
        let value = f64::from_bits(bits);
        let pin = bytes::pin(value).expect("stream chunk must be pinnable");
        let owner = JSValue::from_bits(bits).as_pointer::<u8>();
        let header = unsafe {
            &*owner
                .sub(perry_runtime::gc::GC_HEADER_SIZE)
                .cast::<perry_runtime::gc::GcHeader>()
        };
        assert_eq!(
            header.obj_type,
            perry_runtime::gc::GC_TYPE_BUFFER_UINT8ARRAY
        );
        assert_eq!(
            header.obj_type & BYTES_TYPE_VIEW,
            0,
            "a stream chunk must never join Buffer's pool"
        );
        assert_eq!(header._reserved & BYTES_OUT_OF_LINE != 0, len > 4096);
        assert_eq!(pin.len(), len);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(pin.as_ptr(), pin.len()) },
            &input[..len]
        );
    }
}

/// #9202: `drain_readable_into_bytes` creates a reader read, polls its state
/// directly and drops it. An errored stream rejects that read, and the drain
/// is its only observer — the caller reports the error through its own path
/// (fetch's `drain_body_stream`), so the rejection must count as handled.
/// Before the fix it leaked as an "Uncaught (in promise)" for an error the
/// program already caught, and exited 1.
#[test]
fn draining_an_errored_stream_leaves_no_unhandled_rejection() {
    let _serial = serial_guard();
    let id = alloc_readable(0, 0, 0, 1.0);
    // Each string is handed to the stream before the next allocation, so no
    // raw pointer is held across a collection point.
    {
        let mut rec = rs_rec(RsRef(id)).unwrap();
        let s = &mut *rec;
        let chunk = js_string_from_bytes(b"partial".as_ptr(), 7);
        s.push_chunk(JSValue::string_ptr(chunk).bits(), 1.0);
        s.started = true;
        s.state = ReadableState::Readable;
        // A read delivering the last chunk flips the stream to Errored, so the
        // NEXT read is the one that rejects.
        let reason = js_string_from_bytes(b"boom".as_ptr(), 4);
        s.set_pending_error_after_chunks(Some(JSValue::string_ptr(reason).bits()));
    }
    let before = perry_runtime::promise::rejection::unhandled_rejection_count();
    let bytes = drain_readable_into_bytes(RsRef(id));
    assert_eq!(
        bytes, b"partial",
        "the chunk yielded before the error is still delivered"
    );
    assert_eq!(
        perry_runtime::promise::rejection::unhandled_rejection_count(),
        before,
        "the drain consumed the errored read; it must not stay unhandled"
    );
}

/// A readable's controller is its own object (born with the stream, the
/// same value every time), names its stream, and a plain readable's
/// controller is the default family. No readable ref is a band id.
#[test]
fn controller_value_round_trips_and_stays_out_of_the_allocator() {
    let _serial = serial_guard();
    let id = alloc_readable(0, 0, 0, 1.0);
    assert!(!(STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id));
    let controller = controller_value(id);
    assert_eq!(controller.to_bits(), controller_value(id).to_bits());
    assert_ne!(stream_id(controller), id);
    assert_eq!(controller_stream_id(controller), id);
    assert_eq!(controller_stream_id(stream_value(id)), 0);
    assert_eq!(js_stream_handle_kind(stream_id(controller)), 6);
    assert_eq!(js_stream_handle_kind(id), 1);
}

// WHATWG ClearAlgorithms: a stream that closed or errored drops its
// underlying source/sink/transformer algorithms (they stop being GC roots),
// and no later operation calls them.
static ALGORITHM_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

extern "C" fn counting_algorithm0(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    ALGORITHM_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn counting_algorithm1(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _arg: f64,
) -> f64 {
    ALGORITHM_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn counting_algorithm2(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _arg0: f64,
    _arg1: f64,
) -> f64 {
    ALGORITHM_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(TAG_UNDEFINED)
}

fn counting_closure0() -> i64 {
    js_closure_alloc(
        perry_runtime::fn_info!(counting_algorithm0, 0; with_declared(0)),
        0,
    ) as i64
}

fn counting_closure1() -> i64 {
    js_closure_alloc(
        perry_runtime::fn_info!(counting_algorithm1, 1; with_declared(1)),
        0,
    ) as i64
}

fn counting_closure2() -> i64 {
    js_closure_alloc(
        perry_runtime::fn_info!(counting_algorithm2, 2; with_declared(2)),
        0,
    ) as i64
}

fn closure_value(closure: i64) -> f64 {
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn algorithm_calls() -> usize {
    ALGORITHM_CALLS.load(std::sync::atomic::Ordering::Relaxed)
}

fn readable_algorithms(id: usize) -> [i64; 4] {
    with_rs(RsRef(id), |s| {
        [
            s.start_cb(),
            s.pull_cb(),
            s.cancel_cb(),
            s.strategy_size_cb(),
        ]
    })
    .unwrap()
}

fn writable_algorithms(id: usize) -> [i64; 4] {
    with_ws(WsRef(id), |s| {
        [s.write_cb, s.close_cb, s.abort_cb, s.strategy_size_cb]
    })
    .unwrap()
}

#[test]
fn readable_keeps_algorithms_while_close_requested_and_clears_them_once_drained() {
    let _serial = serial_guard();
    let id = alloc_readable_with_strategy(
        counting_closure1(),
        counting_closure1(),
        counting_closure1(),
        1.0,
        false,
        counting_closure1(),
    );
    {
        let mut rec = rs_rec(RsRef(id)).unwrap();
        let s = &mut *rec;
        s.started = true;
        s.push_chunk(1.0f64.to_bits(), 1.0);
    }
    unsafe { js_readable_stream_controller_close(controller_value(id)) };
    // Close requested with a chunk queued: cancel may still run the source's
    // cancel algorithm, so nothing is cleared yet.
    assert!(readable_algorithms(id).iter().all(|cb| *cb != 0));
    let reader = unsafe { js_readable_stream_get_reader(stream_value(id)) };
    let read = unsafe { js_reader_read(reader) };
    js_promise_run_microtasks();
    assert_eq!(perry_runtime::promise::js_promise_state(read), 1);
    assert_eq!(readable_algorithms(id), [0; 4], "drained close clears");
    let calls = algorithm_calls();
    let cancel = unsafe { js_reader_cancel(reader, 2.0) };
    js_promise_run_microtasks();
    assert_eq!(perry_runtime::promise::js_promise_state(cancel), 1);
    assert_eq!(
        algorithm_calls(),
        calls,
        "a closed stream never calls cancel"
    );
}

#[test]
fn readable_error_clears_algorithms_and_cancel_answers_the_stored_error() {
    let _serial = serial_guard();
    let id = alloc_readable(
        counting_closure1(),
        counting_closure1(),
        counting_closure1(),
        1.0,
    );
    unsafe { js_readable_stream_controller_error(controller_value(id), 5.0) };
    assert_eq!(readable_algorithms(id), [0; 4]);
    let calls = algorithm_calls();
    let cancel = unsafe { js_readable_stream_cancel(stream_value(id), 2.0) };
    js_promise_run_microtasks();
    assert_eq!(perry_runtime::promise::js_promise_state(cancel), 2);
    assert_eq!(perry_runtime::promise::js_promise_reason(cancel), 5.0);
    assert_eq!(algorithm_calls(), calls);
    assert!(matches!(
        rs_rec(RsRef(id)).as_deref().unwrap().state,
        ReadableState::Errored
    ));
}

#[test]
fn readable_cancel_runs_the_cancel_algorithm_then_clears() {
    let _serial = serial_guard();
    let id = alloc_readable(
        counting_closure1(),
        counting_closure1(),
        counting_closure1(),
        1.0,
    );
    let calls = algorithm_calls();
    let cancel = unsafe { js_readable_stream_cancel(stream_value(id), 2.0) };
    js_promise_run_microtasks();
    assert_eq!(algorithm_calls(), calls + 1, "cancel algorithm runs once");
    assert_eq!(perry_runtime::promise::js_promise_state(cancel), 1);
    assert_eq!(readable_algorithms(id), [0; 4]);
}

#[test]
fn writable_close_clears_after_the_close_algorithm_and_abort_keeps_closed() {
    let _serial = serial_guard();
    let id = alloc_writable_with_strategy(
        counting_closure1(),
        counting_closure0(),
        counting_closure1(),
        1.0,
        counting_closure1(),
    );
    let calls = algorithm_calls();
    let close = unsafe { js_writable_stream_close(stream_value(id)) };
    assert_eq!(algorithm_calls(), calls + 1, "close algorithm runs once");
    assert_eq!(writable_algorithms(id), [0; 4]);
    js_promise_run_microtasks();
    assert_eq!(perry_runtime::promise::js_promise_state(close), 1);
    let abort = unsafe { js_writable_stream_abort(stream_value(id), 3.0) };
    js_promise_run_microtasks();
    assert_eq!(perry_runtime::promise::js_promise_state(abort), 1);
    assert_eq!(
        algorithm_calls(),
        calls + 1,
        "a closed stream never calls abort"
    );
    assert!(matches!(
        ws_rec(WsRef(id)).as_deref().unwrap().state,
        WritableState::Closed
    ));
}

#[test]
fn writable_abort_runs_the_abort_algorithm_once_and_keeps_the_stored_error() {
    let _serial = serial_guard();
    let id = alloc_writable(
        counting_closure1(),
        counting_closure0(),
        counting_closure1(),
        1.0,
    );
    let calls = algorithm_calls();
    let first = unsafe { js_writable_stream_abort(stream_value(id), 4.0) };
    assert_eq!(algorithm_calls(), calls + 1);
    assert_eq!(writable_algorithms(id), [0; 4]);
    let second = unsafe { js_writable_stream_abort(stream_value(id), 6.0) };
    js_promise_run_microtasks();
    assert_eq!(perry_runtime::promise::js_promise_state(first), 1);
    assert_eq!(perry_runtime::promise::js_promise_state(second), 1);
    assert_eq!(algorithm_calls(), calls + 1);
    assert_eq!(
        ws_rec(WsRef(id)).as_deref().unwrap().error_value,
        4.0f64.to_bits()
    );
}

#[test]
fn transform_flush_clears_the_transformer() {
    let _serial = serial_guard();
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let transform = counting_closure2();
    let flush = counting_closure1();
    let stream = unsafe {
        js_transform_stream_new(
            undefined,
            closure_value(transform),
            closure_value(flush),
            undefined,
            undefined,
        )
    };
    let transform_id = stream_id(stream);
    let writable = stream_id(unsafe { js_transform_stream_writable(stream) });
    let calls = algorithm_calls();
    let close = unsafe { transform_close(writable) };
    js_promise_run_microtasks();
    assert_eq!(algorithm_calls(), calls + 1, "flush runs once");
    assert_eq!(perry_runtime::promise::js_promise_state(close), 1);
    let algorithms = with_ts(TsRef(transform_id), |t| (t.transform_cb, t.flush_cb)).unwrap();
    assert_eq!(algorithms, (0, 0));
}

#[test]
fn transform_readable_error_clears_the_transformer() {
    let _serial = serial_guard();
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let stream = unsafe {
        js_transform_stream_new(
            undefined,
            closure_value(counting_closure2()),
            closure_value(counting_closure1()),
            undefined,
            undefined,
        )
    };
    let transform_id = stream_id(stream);
    let readable = stream_id(unsafe { js_transform_stream_readable(stream) });
    unsafe { js_readable_stream_controller_error(controller_value(readable), 9.0) };
    let algorithms = with_ts(TsRef(transform_id), |t| (t.transform_cb, t.flush_cb)).unwrap();
    assert_eq!(algorithms, (0, 0));
}

/// A sink algorithm that runs a moving minor collection, as a JS `write()` /
/// `close()` that allocates does at one of its safepoints.
extern "C" fn collecting_sink_algorithm(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    perry_runtime::gc::gc_collect_minor();
    f64::from_bits(TAG_UNDEFINED)
}

/// The write and close request promises settle although the sink's write()
/// and close() move them (Next.js route responses stalled after the body
/// chunk: the write promise was resolved at its old address).
#[test]
fn writable_requests_settle_across_a_moving_sink() {
    let _serial = serial_guard();
    perry_runtime::gc::gc_init();
    struct RestoreGc(i32);
    impl Drop for RestoreGc {
        fn drop(&mut self) {
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
            perry_runtime::gc::js_gc_force_evacuation_test_override(self.0);
        }
    }
    let _restore = RestoreGc(perry_runtime::gc::js_gc_force_evacuation_test_override(1));
    perry_runtime::gc::js_gc_write_barriers_emitted(1);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    unsafe {
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let sink = || {
            let cb = js_closure_alloc(
                perry_runtime::fn_info!(collecting_sink_algorithm, 0; with_declared(0)),
                0,
            );
            f64::from_bits(JSValue::pointer(cb as *const u8).bits())
        };
        let write = scope.root_nanbox_f64(sink());
        let close = scope.root_nanbox_f64(sink());
        let stream = scope.root_nanbox_f64(js_writable_stream_new(
            undefined,
            write.get_nanbox_f64(),
            close.get_nanbox_f64(),
            undefined,
            1.0,
        ));
        let writer = scope.root_nanbox_f64(js_writable_stream_get_writer(stream.get_nanbox_f64()));
        let written = scope.root_raw_mut_ptr(js_writer_write(writer.get_nanbox_f64(), 1.0));
        for _ in 0..4 {
            perry_runtime::promise::js_promise_run_microtasks();
        }
        assert_eq!(
            perry_runtime::promise::js_promise_state(written.get_raw_mut_ptr()),
            1,
            "the write request settles after a moving write()"
        );
        let closed = scope.root_raw_mut_ptr(js_writer_close(writer.get_nanbox_f64()));
        for _ in 0..4 {
            perry_runtime::promise::js_promise_run_microtasks();
        }
        assert_eq!(
            perry_runtime::promise::js_promise_state(closed.get_raw_mut_ptr()),
            1,
            "the close request settles after a moving close()"
        );
    }
}

/// A subclass `this` that attached the ReadableStream payload in `super()`
/// carries its own class id; its payload still names it (the link), so the
/// payload's slot reads reach `this`.
#[test]
fn attached_subclass_stream_payload_names_its_object() {
    let _serial = serial_guard();
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(
        perry_runtime::object::js_object_alloc(0, 0) as i64,
    ));
    let id = attach_readable(this.get_nanbox_f64(), 0, 0, 0, 1.0);
    assert_ne!(id, 0, "a plain object takes the payload");
    assert_eq!(
        with_rs(RsRef(id), |s| s.obj().to_bits()),
        Some(this.get_nanbox_f64().to_bits()),
        "the payload reaches its subclass object"
    );
}
