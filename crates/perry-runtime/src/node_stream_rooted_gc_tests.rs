//! Native stream code that walks JS values while it runs code that can
//! collect must reread them afterwards. Each test makes one specific callback
//! (or, for a window with no user code, the allocation seam
//! [`allocation_point`]) run a moving minor inside that window, asserts that
//! the values under test actually moved, and poisons the from-space so a
//! stale address reads as no object instead of passing as the live one.
use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use std::cell::{Cell, RefCell};

thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
    static COLLECTIONS: Cell<usize> = const { Cell::new(0) };
    static RECORDED_CALLS: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    static ALLOCATION_POINT_HOOK: Cell<Option<fn()>> = const { Cell::new(None) };
}

/// The seam [`allocation_point`] calls: runs the installed hook, if any.
pub(super) fn at_allocation_point() {
    if let Some(hook) = ALLOCATION_POINT_HOOK.with(Cell::get) {
        hook();
    }
}

struct MovingGc {
    _nursery: crate::gc::CopyingNurseryTestGuard,
    _triggers: crate::gc::GcTriggerThresholdTestGuard,
    _evacuate: crate::gc::knob_overrides::ForcedEvacuationTestGuard,
    _poison: crate::arena::ProtectionModeGuard,
}

impl Drop for MovingGc {
    fn drop(&mut self) {
        ALLOCATION_POINT_HOOK.with(|hook| hook.set(None));
    }
}

fn moving_gc() -> MovingGc {
    let guards = MovingGc {
        _nursery: crate::gc::CopyingNurseryTestGuard::new(0),
        _triggers: crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers(),
        _evacuate: crate::gc::knob_overrides::ForcedEvacuationTestGuard::on(),
        _poison: crate::arena::ProtectionModeGuard::set(
            crate::arena::FromSpaceProtection::PoisonOnly,
        ),
    };
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    crate::gc::gc_register_mutable_root_scanner(
        crate::object::descriptor_state::scan_descriptor_roots_mut,
    );
    CALLS.with(|count| count.set(0));
    COLLECTIONS.with(|count| count.set(0));
    RECORDED_CALLS.with(|seen| seen.borrow_mut().clear());
    ALLOCATION_POINT_HOOK.with(|hook| hook.set(None));
    guards
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn boxed<T>(ptr: *const T) -> f64 {
    box_pointer(ptr as *const u8)
}

fn string(bytes: &[u8]) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ) as i64)
}

/// Run one moving minor, the first time only. `COLLECTIONS` counts the
/// collections it ran.
fn collect_once() {
    if COLLECTIONS.with(Cell::get) == 0 {
        COLLECTIONS.with(|count| count.set(1));
        crate::gc::gc_collect_minor();
    }
}

/// Make the next [`allocation_point`] collect (once).
fn collect_at_next_allocation_point() {
    ALLOCATION_POINT_HOOK.with(|hook| hook.set(Some(collect_once)));
}

/// A listener that collects on its first call.
extern "C" fn collecting_listener(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    collect_once();
    undefined()
}

/// A listener that records `(this, first argument)`.
extern "C" fn recording_listener(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    arg: f64,
) -> f64 {
    RECORDED_CALLS.with(|seen| {
        seen.borrow_mut()
            .push((this.as_f64().to_bits(), arg.to_bits()))
    });
    undefined()
}

fn closure_value(info: *const crate::closure::JsFunctionInfo) -> f64 {
    boxed(js_closure_alloc(info, 0))
}

fn listen(stream: f64, event: &'static [u8], listener: f64) {
    let _ = js_node_stream_method_on(
        raw_ptr_from_value(stream) as i64,
        literal_string_value(event),
        listener,
    );
}

/// A new GC array of `values`, rooted in `scope`.
fn rooted_values<'s>(scope: &'s RuntimeHandleScope, values: &[f64]) -> RuntimeHandle<'s> {
    let values = scope.root_nanbox_f64_slice(values);
    let arr = scope.root_nanbox_f64(boxed(crate::array::js_array_alloc(0)));
    for value in &values {
        let grown = crate::array::js_array_push_f64(
            raw_ptr_from_value(arr.get_nanbox_f64()) as *mut crate::array::ArrayHeader,
            value.get_nanbox_f64(),
        );
        arr.set_nanbox_f64(boxed(grown));
    }
    arr
}

fn array_values(array: f64) -> Vec<u64> {
    let arr = raw_ptr_from_value(array) as *const crate::array::ArrayHeader;
    (0..crate::array::js_array_length(arr))
        .map(|i| crate::array::js_array_get_f64(arr, i).to_bits())
        .collect()
}

fn bits(handles: &[&RuntimeHandle<'_>]) -> Vec<u64> {
    handles
        .iter()
        .map(|h| h.get_nanbox_f64().to_bits())
        .collect()
}

fn assert_all_moved(before: &[u64], after: &[u64]) {
    for (i, (before, after)) in before.iter().zip(after).enumerate() {
        assert_ne!(
            before, after,
            "value {i} must actually move in the collection"
        );
    }
}

fn chunk_text(value: f64) -> String {
    let mut bytes = Vec::new();
    append_chunk_bytes(value, &mut bytes, 0);
    String::from_utf8_lossy(&bytes).into_owned()
}

// ── stream.pipeline ─────────────────────────────────────────────────────

/// `pipeline(a, b, c, cb)`: the `'pipe'` listener on `b` collects while the
/// first pair is wired. The second pair must be wired between the live `b`
/// and `c`, so `c`'s `'pipe'` listener sees both at their new addresses.
#[test]
fn classic_pipeline_rereads_stages_after_collecting_pipe_listener() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let a = scope.root_nanbox_f64(js_node_stream_passthrough_new(undefined()));
    let b = scope.root_nanbox_f64(js_node_stream_passthrough_new(undefined()));
    let c = scope.root_nanbox_f64(js_node_stream_writable_new(undefined()));
    let collecting = closure_value(crate::fn_info!(collecting_listener, 1; with_declared(1)));
    listen(b.get_nanbox_f64(), b"pipe", collecting);
    let recording = closure_value(crate::fn_info!(recording_listener, 1; with_declared(1)));
    listen(c.get_nanbox_f64(), b"pipe", recording);
    let callback = closure_value(crate::fn_info!(noop_callback, 1; with_declared(1)));
    let args = rooted_values(
        &scope,
        &[
            a.get_nanbox_f64(),
            b.get_nanbox_f64(),
            c.get_nanbox_f64(),
            callback,
        ],
    );
    let before = bits(&[&b, &c]);

    let _ = js_node_stream_pipeline(raw_ptr_from_value(args.get_nanbox_f64()) as *const _);

    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "b's 'pipe' listener must collect"
    );
    assert_all_moved(&before, &bits(&[&b, &c]));
    assert_eq!(
        RECORDED_CALLS.with(|seen| seen.borrow().clone()),
        vec![(c.get_nanbox_f64().to_bits(), b.get_nanbox_f64().to_bits())],
        "c's 'pipe' listener must run once, on the live c, with the live b"
    );
}

/// A pipeline function stage: collects on its first call and returns its
/// source unchanged.
extern "C" fn collecting_stage(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    source: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(source);
    collect_once();
    source.get_nanbox_f64()
}

/// `write(chunk, encoding, cb)`: records `(this, chunk)`, collects on its
/// first call, then completes.
extern "C" fn collecting_write(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    _encoding: f64,
    cb: f64,
) -> f64 {
    RECORDED_CALLS.with(|seen| {
        seen.borrow_mut()
            .push((this.as_f64().to_bits(), chunk.to_bits()))
    });
    let scope = RuntimeHandleScope::new();
    let cb = scope.root_nanbox_f64(cb);
    collect_once();
    if is_callable_value(cb.get_nanbox_f64()) {
        call_listener_args(undefined(), cb.get_nanbox_f64(), &[]);
    }
    undefined()
}

/// The pipeline completion callback: counts calls and records its error.
extern "C" fn counting_callback(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    CALLS.with(|count| count.set(count.get() + 1));
    assert_eq!(err.to_bits(), TAG_UNDEFINED, "pipeline must not fail");
    undefined()
}

/// A no-op completion callback.
extern "C" fn noop_callback(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _err: f64,
) -> f64 {
    undefined()
}

fn writable_with_write(scope: &RuntimeHandleScope, object_mode: bool) -> RuntimeHandle<'_> {
    let opts = scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 2)));
    let write = closure_value(crate::fn_info!(collecting_write, 3; with_declared(3)));
    let obj = || raw_ptr_from_value(opts.get_nanbox_f64()) as *mut crate::object::ObjectHeader;
    crate::object::js_object_set_field_by_name(obj(), hidden_key(b"write"), write);
    if object_mode {
        let on = f64::from_bits(TAG_TRUE);
        crate::object::js_object_set_field_by_name(obj(), hidden_key(b"objectMode"), on);
    }
    scope.root_nanbox_f64(js_node_stream_writable_new(opts.get_nanbox_f64()))
}

/// `pipeline(source, fn, destination, cb)` takes the collected path. The
/// function stage collects; the destination and the callback must still be
/// used at their live addresses afterwards.
#[test]
fn collected_pipeline_rereads_stages_and_callback_after_collecting_stage() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let chunks = rooted_values(&scope, &[string(b"one"), string(b"two")]);
    let source = scope.root_nanbox_f64(js_node_stream_readable_from(chunks.get_nanbox_f64()));
    let stage = scope.root_nanbox_f64(closure_value(
        crate::fn_info!(collecting_stage, 1; with_declared(1)),
    ));
    let destination = writable_with_write(&scope, false);
    let callback = scope.root_nanbox_f64(closure_value(
        crate::fn_info!(counting_callback, 1; with_declared(1)),
    ));
    let args = rooted_values(
        &scope,
        &[
            source.get_nanbox_f64(),
            stage.get_nanbox_f64(),
            destination.get_nanbox_f64(),
            callback.get_nanbox_f64(),
        ],
    );
    let before = bits(&[&destination, &callback]);

    let _ = js_node_stream_pipeline(raw_ptr_from_value(args.get_nanbox_f64()) as *const _);

    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "the function stage must collect"
    );
    assert_all_moved(&before, &bits(&[&destination, &callback]));
    let seen = RECORDED_CALLS.with(|seen| seen.borrow().clone());
    assert_eq!(
        seen.len(),
        2,
        "both chunks must reach the destination: {seen:?}"
    );
    for (this, _) in &seen {
        assert_eq!(*this, destination.get_nanbox_f64().to_bits());
    }
    assert_eq!(CALLS.with(Cell::get), 1, "the live callback must run once");
}

/// `write_pipeline_chunks_to_stream`: the destination's first `write`
/// collects. Every later chunk must be the live chunk, written to the live
/// destination.
#[test]
fn pipeline_chunk_writes_reread_chunks_and_stream_after_collecting_write() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let destination = writable_with_write(&scope, true);
    let objects: Vec<f64> = (0..3)
        .map(|_| boxed(crate::object::js_object_alloc(0, 1)))
        .collect();
    let expected = rooted_values(&scope, &objects);
    let chunks = rooted_values(&scope, &[]);
    for value in array_values(expected.get_nanbox_f64()) {
        rooted_array_push_for_tests(&chunks, f64::from_bits(value));
    }
    let before = array_values(expected.get_nanbox_f64());
    let destination_before = destination.get_nanbox_f64().to_bits();

    let result = write_pipeline_chunks_to_stream(
        destination.get_nanbox_f64(),
        chunks.get_nanbox_f64(),
        false,
    );

    assert!(result.is_ok());
    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "the first write must collect"
    );
    let after = array_values(expected.get_nanbox_f64());
    assert_all_moved(&before, &after);
    assert_ne!(destination.get_nanbox_f64().to_bits(), destination_before);
    let seen = RECORDED_CALLS.with(|seen| seen.borrow().clone());
    let written: Vec<u64> = seen.iter().map(|(_, chunk)| *chunk).collect();
    assert_eq!(written.len(), 3, "every chunk must be written: {seen:?}");
    assert_eq!(
        &written[1..],
        &after[1..],
        "chunks written after the collection must be the live chunks"
    );
    assert_eq!(seen[2].0, destination.get_nanbox_f64().to_bits());
}

fn rooted_array_push_for_tests(array: &RuntimeHandle<'_>, value: f64) {
    let grown = crate::array::js_array_push_f64(
        raw_ptr_from_value(array.get_nanbox_f64()) as *mut crate::array::ArrayHeader,
        value,
    );
    array.set_nanbox_f64(boxed(grown));
}

// ── stream.compose ──────────────────────────────────────────────────────

/// The stage list a composed duplex captured in its `write` closure.
fn composed_stage_list(composite: f64) -> Vec<u64> {
    let write = get_hidden_value(composite, hidden_write_key()).expect("composite write");
    let closure = raw_ptr_from_value(write) as *const ClosureHeader;
    array_values(crate::closure::js_closure_get_capture_f64(closure, 1))
}

/// `compose(source, a, b)`: `Readable.from(source)` allocates while the
/// arguments are held. A collection there must not leave the stage list
/// naming the stages' old addresses.
#[test]
fn compose_rereads_stages_after_collecting_source_normalization() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let source = rooted_values(&scope, &[string(b"x")]);
    let a = scope.root_nanbox_f64(js_node_stream_passthrough_new(undefined()));
    let b = scope.root_nanbox_f64(js_node_stream_passthrough_new(undefined()));
    let before = bits(&[&a, &b]);
    collect_at_next_allocation_point();

    let composite = scope.root_nanbox_f64(build_node_stream_compose(vec![
        source.get_nanbox_f64(),
        a.get_nanbox_f64(),
        b.get_nanbox_f64(),
    ]));

    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "the allocation point must collect"
    );
    assert_all_moved(&before, &bits(&[&a, &b]));
    assert_eq!(
        composed_stage_list(composite.get_nanbox_f64()),
        bits(&[&a, &b]),
        "the composed stage list must hold the live stages"
    );
}

/// A composed duplex's `write` runs its stages; the function stage collects.
/// The output must land in the live composite, and `cb` must be called on it.
#[test]
fn compose_write_rereads_composite_after_collecting_stage() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let stage = closure_value(crate::fn_info!(collecting_stage, 1; with_declared(1)));
    let composite = scope.root_nanbox_f64(build_node_stream_compose(vec![stage]));
    let write = scope.root_nanbox_f64(
        get_hidden_value(composite.get_nanbox_f64(), hidden_write_key()).expect("write"),
    );
    let cb = scope.root_nanbox_f64(closure_value(
        crate::fn_info!(recording_listener, 1; with_declared(1)),
    ));
    let chunk = scope.root_nanbox_f64(string(b"composed chunk"));
    let before = composite.get_nanbox_f64().to_bits();

    let _ = crate::closure::js_closure_call3(
        raw_ptr_from_value(write.get_nanbox_f64()) as *const ClosureHeader,
        crate::closure::JsThis::from_f64(composite.get_nanbox_f64()),
        chunk.get_nanbox_f64(),
        undefined(),
        cb.get_nanbox_f64(),
    );

    assert_eq!(COLLECTIONS.with(Cell::get), 1, "the stage must collect");
    assert_ne!(
        composite.get_nanbox_f64().to_bits(),
        before,
        "the composite must move"
    );
    let seen = RECORDED_CALLS.with(|seen| seen.borrow().clone());
    assert_eq!(seen.len(), 1, "cb must run once: {seen:?}");
    assert_eq!(seen[0].0, composite.get_nanbox_f64().to_bits(), "cb's this");
    let buffered = readable_hidden_chunks(composite.get_nanbox_f64()).expect("output");
    let texts: Vec<String> = array_values(buffered)
        .into_iter()
        .map(|v| chunk_text(f64::from_bits(v)))
        .collect();
    assert_eq!(texts, vec!["composed chunk".to_string()]);
}

// ── readable.read() ─────────────────────────────────────────────────────

/// `_read(size)`: pushes `"abc"` onto `this`, then collects.
extern "C" fn pushing_collecting_read(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    _size: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let chunk = string(b"abc");
    let _ = push_chunk(stream.get_nanbox_f64(), chunk);
    collect_once();
    undefined()
}

fn readable_with_collecting_read(scope: &RuntimeHandleScope) -> RuntimeHandle<'_> {
    let stream = scope.root_nanbox_f64(js_node_stream_readable_new(undefined()));
    let read = closure_value(crate::fn_info!(pushing_collecting_read, 1; with_declared(1)));
    let key = b"_read";
    crate::object::js_object_set_field_by_name(
        raw_ptr_from_value(stream.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
        crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32),
        read,
    );
    stream
}

/// `read()` and `read(n)` run `_read` first; it collects. The rest of the
/// read must use the live stream, where `_read` buffered its chunk.
fn read_after_collecting_read(size: f64, expected: &str) {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let stream = readable_with_collecting_read(&scope);
    let before = stream.get_nanbox_f64().to_bits();

    let result = read_stream_with_size_arg(stream.get_nanbox_f64(), size);

    assert_eq!(COLLECTIONS.with(Cell::get), 1, "_read must collect");
    assert_ne!(
        stream.get_nanbox_f64().to_bits(),
        before,
        "the stream must move"
    );
    assert_ne!(
        result.to_bits(),
        TAG_NULL,
        "read must see the buffered chunk"
    );
    assert_eq!(chunk_text(result), expected);
}

#[test]
fn read_rereads_stream_after_collecting_read() {
    read_after_collecting_read(undefined(), "abc");
}

#[test]
fn sized_read_rereads_stream_after_collecting_read() {
    read_after_collecting_read(2.0, "ab");
}

/// A paused readable with buffered chunks; returns the stream and a rooted
/// copy of its chunk list (the same chunk objects).
fn buffered_readable<'s>(
    scope: &'s RuntimeHandleScope,
    chunks: &[&[u8]],
    encoding: Option<&[u8]>,
) -> (RuntimeHandle<'s>, RuntimeHandle<'s>) {
    let stream = scope.root_nanbox_f64(js_node_stream_readable_new(undefined()));
    if let Some(encoding) = encoding {
        let _ = ns_set_encoding1(
            std::ptr::null(),
            crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
            string(encoding),
        );
    }
    for chunk in chunks {
        let value = buffer_value_from_bytes(chunk);
        let _ = push_chunk(stream.get_nanbox_f64(), value);
    }
    let buffered = readable_hidden_chunks(stream.get_nanbox_f64()).expect("buffered chunks");
    let copy = rooted_values(&scope, &[]);
    for value in array_values(buffered) {
        rooted_array_push_for_tests(&copy, f64::from_bits(value));
    }
    (stream, copy)
}

/// Paused `read()` on a decoding stream drains the whole buffer: it clears
/// the stream's buffer first, so the chunks it then decodes are held only by
/// the read. A collection at the decode allocation must not lose them.
#[test]
fn draining_read_keeps_chunks_current_across_decode_allocation() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let (stream, chunks) = buffered_readable(&scope, &[b"abc", b"def"], Some(b"utf8"));
    let before = array_values(chunks.get_nanbox_f64());
    collect_at_next_allocation_point();

    let result = read_stream_with_size_arg(stream.get_nanbox_f64(), undefined());

    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "the decode allocation must collect"
    );
    assert_all_moved(&before, &array_values(chunks.get_nanbox_f64()));
    assert_eq!(chunk_text(result), "abcdef");
}

/// `read(3)` from `["ab", "cd", "ef"]` splits `"cd"`, allocating its rest.
/// A collection there must leave the stream current: the next `read(3)`
/// returns `"def"`. (Buffer chunks do not move; the stream does.)
#[test]
fn sized_read_keeps_stream_current_across_split_allocation() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let (stream, _) = buffered_readable(&scope, &[b"ab", b"cd", b"ef"], None);
    let stream_before = stream.get_nanbox_f64().to_bits();
    collect_at_next_allocation_point();

    let first = read_stream_with_size_arg(stream.get_nanbox_f64(), 3.0);
    let first = scope.root_nanbox_f64(first);

    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "the split allocation must collect"
    );
    assert_ne!(
        stream.get_nanbox_f64().to_bits(),
        stream_before,
        "the stream must move"
    );
    assert_eq!(chunk_text(first.get_nanbox_f64()), "abc");
    let second = read_stream_with_size_arg(stream.get_nanbox_f64(), 3.0);
    assert_eq!(chunk_text(second), "def", "the live stream keeps the rest");
}

// ── readable.map ────────────────────────────────────────────────────────

/// A mapper that collects on its first call and returns its argument.
extern "C" fn collecting_identity(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    collect_once();
    value.get_nanbox_f64()
}

/// `readable.map(f)`: the mapper collects on the first chunk. The later
/// chunks must be read from the live chunk list into the live output.
#[test]
fn readable_map_rereads_chunks_after_collecting_mapper() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let objects: Vec<f64> = (0..3)
        .map(|_| boxed(crate::object::js_object_alloc(0, 1)))
        .collect();
    let expected = rooted_values(&scope, &objects);
    let stream = scope.root_nanbox_f64(js_node_stream_readable_from(expected.get_nanbox_f64()));
    let chunks = readable_hidden_chunks(stream.get_nanbox_f64()).expect("chunks");
    assert_eq!(
        array_values(chunks),
        array_values(expected.get_nanbox_f64())
    );
    let before = array_values(expected.get_nanbox_f64());
    let mapper = closure_value(crate::fn_info!(collecting_identity, 1; with_declared(1)));

    let result = ns_iter_map(
        std::ptr::null(),
        crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
        mapper,
        undefined(),
    );

    assert_eq!(COLLECTIONS.with(Cell::get), 1, "the mapper must collect");
    let after = array_values(expected.get_nanbox_f64());
    assert_all_moved(&before, &after);
    let mapped = readable_hidden_chunks(result).expect("mapped chunks");
    assert_eq!(
        array_values(mapped),
        after,
        "map must yield the live chunks"
    );
}

// ── settling a pipeline value ───────────────────────────────────────────

/// A queued job that collects (once) and settles nothing.
extern "C" fn collecting_job(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    collect_once();
    undefined()
}

/// `settle_pipeline_value_with_origin` runs the queued jobs while it waits on
/// a pending promise, and one of them collects. The wait must check the live
/// promise, and the still-pending promise it hands back must be the promise at
/// its new address.
#[test]
fn settling_a_pending_promise_rereads_it_after_a_collecting_job() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let promise = scope.root_nanbox_f64(boxed(crate::promise::js_promise_new()));
    let job = js_closure_alloc(crate::fn_info!(collecting_job, 0; with_declared(0)), 0);
    crate::promise::enqueue_queue_microtask(job as i64);
    let before = promise.get_nanbox_f64().to_bits();

    let settled =
        super::super::pipeline::settle_pipeline_value_with_origin(promise.get_nanbox_f64())
            .unwrap_or_else(|_| panic!("a pending promise must not settle as rejected"));

    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "the queued job must collect"
    );
    assert_ne!(
        before,
        promise.get_nanbox_f64().to_bits(),
        "the promise must actually move in the collection"
    );
    assert!(!settled.fulfilled_promise);
    assert_eq!(
        settled.value.to_bits(),
        promise.get_nanbox_f64().to_bits(),
        "the pending promise handed back must be the live promise"
    );
}
