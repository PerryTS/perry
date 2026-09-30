use super::transform::{run_web_compression_codec, split_utf8_prefix};
use super::*;

/// #6602: the id-allocator tests below mutate shared pool state (quarantine
/// limit, free list) — serialize them so cargo's parallel test threads don't
/// interleave allocations between a retire and its recycle assertion.
static ALLOCATOR_TEST_SERIAL: Mutex<()> = Mutex::new(());

pub(super) fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    ALLOCATOR_TEST_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn alloc_closed_readable() -> usize {
    let id = alloc_readable(0, 0, 0, 1.0);
    let mut g = READABLE_STREAMS.lock().unwrap();
    g.get_mut(&id).unwrap().state = ReadableState::Closed;
    drop(g);
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
        "stream id {id:#x} must stay in the raw numeric stream band"
    );
    assert!(
        id >= perry_runtime::value::addr_class::HANDLE_BAND_MAX,
        "stream id {id:#x} must not overlap pointer-tagged small handles"
    );
}

#[test]
fn terminal_stream_ids_recycle_after_quarantine_eviction() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    idalloc::set_quarantine_limit_for_test(2);
    let ids: Vec<usize> = (0..3)
        .map(|_| {
            let id = alloc_closed_readable();
            idalloc::retire_readable_terminal(id);
            id
        })
        .collect();
    idalloc::set_quarantine_limit_for_test(idalloc::DEFAULT_QUARANTINE);
    // The third retire overflowed the quarantine of 2: the oldest id lost its
    // registry entry (eviction really cleans up)…
    assert!(
        !READABLE_STREAMS.lock().unwrap().contains_key(&ids[0]),
        "evicted id must leave the registry"
    );
    assert!(
        READABLE_STREAMS.lock().unwrap().contains_key(&ids[2]),
        "quarantined id keeps its registry entry"
    );
    // …and the allocator hands it back, oldest first, before any fresh id.
    let reused = next_stream_id();
    assert_eq!(reused, ids[0], "evicted id recycles FIFO");
    assert!((STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&reused));
    // With the free list drained the allocator is back on fresh ids.
    let fresh = alloc_readable(0, 0, 0, 1.0);
    assert_ne!(fresh, reused);
}

#[test]
fn retire_is_idempotent_per_id() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    idalloc::set_quarantine_limit_for_test(idalloc::DEFAULT_QUARANTINE);
    let id = alloc_closed_readable();
    idalloc::retire_readable_terminal(id);
    idalloc::retire_readable_terminal(id);
    // The pipe-lock path must also skip it: it carries no ownership mark.
    idalloc::retire_pipe_lock_id(id);
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
    let id = alloc_readable(0, 0, 0, 1.0);
    idalloc::retire_readable_terminal(id); // Readable → not terminal
    idalloc::retire_pipe_lock_id(id); // no ownership mark → guarded
    assert_eq!(idalloc::test_pool_occurrences(id), 0);
    {
        // Closed but undrained (slow consumer with queued chunks) stays live.
        let mut g = READABLE_STREAMS.lock().unwrap();
        let s = g.get_mut(&id).unwrap();
        s.state = ReadableState::Closed;
        s.push_chunk(TAG_UNDEFINED, 1.0);
    }
    idalloc::retire_readable_terminal(id);
    assert_eq!(idalloc::test_pool_occurrences(id), 0);
}

#[test]
fn pipe_lock_ids_retire_once_via_ownership_mark() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    let id = idalloc::next_pipe_lock_id();
    idalloc::retire_pipe_lock_id(id);
    idalloc::retire_pipe_lock_id(id); // stale duplicate release: mark gone
    assert_eq!(idalloc::test_pool_occurrences(id), 1);
    // A live registered id is untouchable through the pipe path even though
    // it, too, has no ownership mark.
    let live = alloc_readable(0, 0, 0, 1.0);
    idalloc::retire_pipe_lock_id(live);
    assert_eq!(idalloc::test_pool_occurrences(live), 0);
}

#[test]
fn attached_reader_dies_with_terminal_stream_but_not_before() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    let stream = alloc_readable(0, 0, 0, 1.0);
    let reader = next_stream_id();
    READERS.lock().unwrap().insert(
        reader,
        ReaderData {
            stream_handle: stream,
            locked: true,
            closed_promise: 0x2345_6780 as *mut Promise,
            is_byob: false,
        },
    );
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        let s = g.get_mut(&stream).unwrap();
        s.reader_handle = Some(reader);
        s.state = ReadableState::Closed;
    }
    // A still-locked reader does not retire on its own…
    idalloc::retire_reader_if_released(reader);
    assert_eq!(idalloc::test_pool_occurrences(reader), 0);
    // …but goes down with its terminal stream.
    idalloc::retire_readable_terminal(stream);
    assert_eq!(idalloc::test_pool_occurrences(stream), 1);
    assert_eq!(idalloc::test_pool_occurrences(reader), 1);
}

#[test]
fn root_scanner_emits_callbacks_chunks_and_promises() {
    // Clears READABLE_STREAMS wholesale — serialize against the allocator
    // tests, whose assertions read that registry.
    let _serial = serial_guard();
    {
        let mut readable = READABLE_STREAMS.lock().unwrap();
        readable.clear();
        readable.insert(
            1,
            ReadableStreamData {
                state: ReadableState::Errored,
                chunks: VecDeque::from([0x7FFD_0000_0000_1234, 0x7FFA_0000_0000_2345]),
                chunk_sizes: VecDeque::from([1.0, 1.0]),
                queue_total_size: 2.0,
                pending_reads: VecDeque::from([0x2345_6780 as *mut Promise]),
                start_cb: 0x3456_7890,
                pull_cb: 0,
                cancel_cb: 0,
                high_water_mark: 1.0,
                strategy_size_cb: 0,
                is_byte_stream: false,
                pull_returns_byte_chunk: false,
                pulling: false,
                started: false,
                reader_handle: None,
                error_value: 0x7FFF_0000_0000_4567,
                pending_error_after_chunks: None,
                canceled: false,
            },
        );
    }

    let mut emitted = Vec::new();
    scan_stream_roots(&mut |value| emitted.push(value.to_bits()));

    assert!(emitted.contains(&0x7FFD_0000_0000_1234));
    assert!(emitted.contains(&0x7FFA_0000_0000_2345));
    assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
    assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x3456_7890)));
    assert!(emitted.contains(&0x7FFF_0000_0000_4567));
    READABLE_STREAMS.lock().unwrap().clear();
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

    let _serial = serial_guard();
    transform::TRANSFORM_WRITE_RELEASES
        .lock()
        .unwrap()
        .insert(0xA001, vec![0x2100]);
    transform::TRANSFORM_PENDING_CLOSE
        .lock()
        .unwrap()
        .insert(0xA002, 0x3100);
    transform::TRANSFORM_BACKPRESSURED_JOBS
        .lock()
        .unwrap()
        .insert(0xA003, vec![0x4100]);

    super::gc::scan_transform_deferred_roots(&mut RelocatingVisitor);

    assert_eq!(
        transform::TRANSFORM_WRITE_RELEASES.lock().unwrap()[&0xA001],
        vec![0x3100]
    );
    assert_eq!(
        transform::TRANSFORM_PENDING_CLOSE.lock().unwrap()[&0xA002],
        0x4100
    );
    assert_eq!(
        transform::TRANSFORM_BACKPRESSURED_JOBS.lock().unwrap()[&0xA003],
        vec![0x5100]
    );

    transform::TRANSFORM_WRITE_RELEASES
        .lock()
        .unwrap()
        .remove(&0xA001);
    transform::TRANSFORM_PENDING_CLOSE
        .lock()
        .unwrap()
        .remove(&0xA002);
    transform::TRANSFORM_BACKPRESSURED_JOBS
        .lock()
        .unwrap()
        .remove(&0xA003);
}

#[test]
fn stream_runtime_owned_calls_cannot_bypass_provider_abi() {
    let streams_source = include_str!("../streams.rs");
    for forbidden in [
        "perry_runtime::array::js_",
        "perry_runtime::closure::js_",
        "perry_runtime::promise::js_",
    ] {
        assert!(
            !streams_source.contains(forbidden),
            "streams.rs bypasses the provider ABI through {forbidden}"
        );
    }

    let gc_source = include_str!("gc.rs");
    assert!(!gc_source
        .contains("perry_runtime::node_submodules::js_register_stream_consumer_callbacks"));
    assert!(!gc_source.contains("perry_runtime::object::js_register_stream_expando_set"));
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

    assert!(unsafe { dispatch_stream_method(readable, "terminate", &[]) }.is_some());
    assert!(matches!(
        READABLE_STREAMS
            .lock()
            .unwrap()
            .get(&(readable as usize))
            .unwrap()
            .state,
        ReadableState::Closed
    ));
    assert!(matches!(
        WRITABLE_STREAMS
            .lock()
            .unwrap()
            .get(&(writable as usize))
            .unwrap()
            .state,
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

    READABLE_STREAMS
        .lock()
        .unwrap()
        .get_mut(&source)
        .unwrap()
        .reader_handle = Some(1);
    assert_eq!(
        pipe_through_validation_error(source, destination, output),
        Some("ReadableStream is locked")
    );

    READABLE_STREAMS
        .lock()
        .unwrap()
        .get_mut(&source)
        .unwrap()
        .reader_handle = None;
    WRITABLE_STREAMS
        .lock()
        .unwrap()
        .get_mut(&destination)
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
    READABLE_STREAMS
        .lock()
        .unwrap()
        .get_mut(&buffered)
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

    let mut streams = READABLE_STREAMS.lock().unwrap();
    streams.remove(&buffered);
    streams.remove(&empty);
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
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    // Only a numeric stream handle is held in Rust across the collection.
    let endpoint = unsafe { perry_runtime::closure::js_closure_get_capture_f64(closure, 0) };
    perry_runtime::gc::gc_collect_minor();
    endpoint
}

#[test]
fn pipe_through_pair_survives_a_moving_getter() {
    let _serial = serial_guard();
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
        let child = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
        let getter = scope.root_raw_mut_ptr(js_closure_alloc(
            perry_runtime::fn_info!(collecting_pair_getter, 0; with_declared(0)),
            1,
        ));
        perry_runtime::closure::js_closure_set_capture_f64(
            getter.get_raw_mut_ptr::<ClosureHeader>(),
            0,
            readable,
        );
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
            1,
        ));
        perry_runtime::closure::js_closure_set_capture_f64(
            writer_getter.get_raw_mut_ptr::<ClosureHeader>(),
            0,
            writable,
        );
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
        let child_before = child.get_raw_mut_ptr::<ObjectHeader>();
        let source = alloc_closed_readable() as f64;
        let output = js_readable_stream_pipe_through_pair(
            source,
            f64::from_bits(
                JSValue::pointer(pair.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits(),
            ),
            f64::from_bits(
                JSValue::pointer(options.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits(),
            ),
        );
        assert_eq!(output, readable);
        assert_ne!(
            child.get_raw_mut_ptr::<ObjectHeader>(),
            child_before,
            "the getter must have performed a moving collection"
        );
    }
}
