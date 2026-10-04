//! `stream/promises.pipeline` must keep every value current across the user
//! code it runs (#11882). Each test drives `thunk_streamP_pipeline`, makes
//! one specific callback run a moving minor, asserts that the collection
//! really moved the values under test, and poisons the from-space so a stale
//! address cannot pass as the live object.
use super::*;
use crate::gc::RuntimeHandleScope;
use std::cell::{Cell, RefCell};

thread_local! {
    static WRITES: Cell<usize> = const { Cell::new(0) };
    static ENDS: Cell<usize> = const { Cell::new(0) };
    static COLLECT_IN_WRITE: Cell<bool> = const { Cell::new(false) };
    static MOVED_BY_WRITE: Cell<bool> = const { Cell::new(false) };
    static WRITE_GETTER_CALLS: Cell<usize> = const { Cell::new(0) };
    static MOVED_BY_GETTER: Cell<bool> = const { Cell::new(false) };
    static READS: Cell<usize> = const { Cell::new(0) };
    static MOVED_BY_READ: Cell<bool> = const { Cell::new(false) };
    static STALE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static STALE_RECEIVER: Cell<bool> = const { Cell::new(false) };
    static HOOK_CALLS: Cell<usize> = const { Cell::new(0) };
    static STAGES: RefCell<Vec<(u8, u64)>> = const { RefCell::new(Vec::new()) };
}

struct MovingGc {
    _nursery: crate::gc::CopyingNurseryTestGuard,
    _triggers: crate::gc::GcTriggerThresholdTestGuard,
    _evacuate: crate::gc::knob_overrides::ForcedEvacuationTestGuard,
    _poison: crate::arena::ProtectionModeGuard,
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
    crate::gc::gc_register_mutable_root_scanner(
        crate::object::descriptor_state::scan_descriptor_roots_mut,
    );
    WRITES.with(|count| count.set(0));
    ENDS.with(|count| count.set(0));
    COLLECT_IN_WRITE.with(|flag| flag.set(false));
    MOVED_BY_WRITE.with(|moved| moved.set(false));
    WRITE_GETTER_CALLS.with(|count| count.set(0));
    MOVED_BY_GETTER.with(|moved| moved.set(false));
    READS.with(|count| count.set(0));
    MOVED_BY_READ.with(|moved| moved.set(false));
    STALE.with(|stale| stale.borrow_mut().clear());
    STALE_RECEIVER.with(|stale| stale.set(false));
    HOOK_CALLS.with(|count| count.set(0));
    STAGES.with(|stages| stages.borrow_mut().clear());
    guards
}

/// Collect once, recording in `moved` whether `expected` (a closure capture,
/// which the collector rewrites) changed address.
fn collect_watching(expected: f64, moved: &'static std::thread::LocalKey<Cell<bool>>) {
    let scope = RuntimeHandleScope::new();
    let expected = scope.root_nanbox_f64(expected);
    let before = expected.get_nanbox_f64().to_bits();
    crate::gc::gc_collect_minor();
    let after = expected.get_nanbox_f64().to_bits();
    moved.with(|moved| moved.set(moved.get() || before != after));
}

/// `write(chunk)`: captures `[expected chunks, destination]`. Compares the
/// chunk and receiver it got against the live captures; the first call
/// collects when `COLLECT_IN_WRITE` is set.
extern "C" fn checking_write(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    _encoding: f64,
) -> f64 {
    if this.as_f64().to_bits() != js_closure_get_capture_f64(closure, 1).to_bits() {
        STALE_RECEIVER.with(|stale| stale.set(true));
    }
    let index = WRITES.with(|count| count.replace(count.get() + 1));
    let expected = js_closure_get_capture_f64(closure, 0);
    let array = crate::value::js_nanbox_get_pointer(expected) as *const crate::array::ArrayHeader;
    let current = crate::array::js_array_get_f64(array, index as u32);
    // Compare bits only: a stale argument must not be dereferenced.
    if chunk.to_bits() != current.to_bits() {
        STALE.with(|stale| stale.borrow_mut().push(index));
    }
    if index == 0 && COLLECT_IN_WRITE.with(Cell::get) {
        collect_watching(expected, &MOVED_BY_WRITE);
    }
    undefined_value()
}

extern "C" fn count_end(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _encoding: f64,
) -> f64 {
    ENDS.with(|count| count.set(count.get() + 1));
    undefined_value()
}

/// The only `write` in the getter test: captures `[write, expected]`,
/// collects on every lookup and returns the method.
extern "C" fn collecting_write_getter(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    WRITE_GETTER_CALLS.with(|count| count.set(count.get() + 1));
    let scope = RuntimeHandleScope::new();
    let closure = scope.root_raw_const_ptr(closure);
    let expected = closure.with_const_ptr(|closure| js_closure_get_capture_f64(closure, 1));
    collect_watching(expected, &MOVED_BY_GETTER);
    closure.with_const_ptr(|closure| js_closure_get_capture_f64(closure, 0))
}

/// The source's own `_read`: captures `[expected]` and collects.
extern "C" fn collecting_read(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _size: f64,
) -> f64 {
    READS.with(|count| count.set(count.get() + 1));
    collect_watching(js_closure_get_capture_f64(closure, 0), &MOVED_BY_READ);
    undefined_value()
}

fn set_field(object: &crate::gc::RuntimeHandle<'_>, key: &[u8], value: f64) {
    let key = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(object.get_nanbox_f64()) as *mut ObjectHeader,
        key,
        value,
    );
}

#[derive(Clone, Copy, PartialEq)]
enum Trigger {
    /// The first `write` collects; the destination was promoted first, so
    /// this isolates the chunk snapshot.
    WriteChunks,
    /// The first `write` collects and the destination is young.
    WriteReceiver,
    /// A `write` getter (the only `write`) collects on every lookup.
    WriteGetter,
    /// The source's `_read` collects before its chunks are read.
    SourceRead,
}

/// `pipeline(Readable.from(chunks), destination)` through the direct path.
fn direct_pipeline(trigger: Trigger) {
    let _gc = moving_gc();
    COLLECT_IN_WRITE.with(|flag| {
        flag.set(matches!(
            trigger,
            Trigger::WriteChunks | Trigger::WriteReceiver
        ))
    });
    let scope = RuntimeHandleScope::new();
    let destination = scope.root_nanbox_f64(value_from_ptr(js_object_alloc(0, 4).cast()));
    if trigger == Trigger::WriteChunks {
        crate::gc::gc_collect_minor();
        crate::gc::gc_collect_minor();
    }
    let destination_before = destination.get_nanbox_f64().to_bits();
    let expected = scope.root_nanbox_f64(value_from_ptr(crate::array::js_array_alloc(3).cast()));
    for bytes in [
        b"first pipeline chunk".as_slice(),
        b"second pipeline chunk",
        b"third pipeline chunk",
    ] {
        let chunk = crate::value::js_nanbox_string(js_string_from_bytes(
            bytes.as_ptr(),
            bytes.len() as u32,
        ) as i64);
        let array = crate::value::js_nanbox_get_pointer(expected.get_nanbox_f64())
            as *mut crate::array::ArrayHeader;
        let array = crate::array::js_array_push_f64(array, chunk);
        expected.set_nanbox_f64(value_from_ptr(array.cast()));
    }
    // The source gets its own copy of the chunk list.
    let copy = scope.root_raw_mut_ptr(crate::array::js_array_alloc(3));
    for i in 0..3 {
        let chunk = crate::array::js_array_get_f64(
            crate::value::js_nanbox_get_pointer(expected.get_nanbox_f64())
                as *const crate::array::ArrayHeader,
            i,
        );
        copy.set_raw_mut_ptr(
            copy.with_mut_ptr(|copy| crate::array::js_array_push_f64(copy, chunk)),
        );
    }
    let source = scope.root_nanbox_f64(crate::node_stream::js_node_stream_readable_from(
        copy.with_const_ptr(value_from_ptr),
    ));
    let source_before = source.get_nanbox_f64().to_bits();
    if trigger == Trigger::SourceRead {
        let read = js_closure_alloc(crate::fn_info!(collecting_read, 1; with_declared(1)), 1);
        js_closure_set_capture_f64(read, 0, expected.get_nanbox_f64());
        set_field(&source, b"_read", value_from_ptr(read.cast()));
    }

    let write = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(checking_write, 2; with_declared(2)),
        2,
    ));
    write.with_mut_ptr(|write| {
        js_closure_set_capture_f64(write, 0, expected.get_nanbox_f64());
        js_closure_set_capture_f64(write, 1, destination.get_nanbox_f64());
    });
    let end = js_closure_alloc(crate::fn_info!(count_end, 1; with_declared(1)), 0);
    set_field(&destination, b"end", value_from_ptr(end.cast()));
    if trigger == Trigger::WriteGetter {
        // An accessor and no data property: if the lookup bypassed the
        // getter, there would be no `write` to call.
        let getter = js_closure_alloc(
            crate::fn_info!(collecting_write_getter, 0; with_declared(0)),
            2,
        );
        js_closure_set_capture_f64(getter, 0, write.with_const_ptr(value_from_ptr));
        js_closure_set_capture_f64(getter, 1, expected.get_nanbox_f64());
        let key = b"write";
        crate::object::js_object_define_accessor(
            destination.get_nanbox_f64(),
            crate::value::js_nanbox_string(
                js_string_from_bytes(key.as_ptr(), key.len() as u32) as i64
            ),
            value_from_ptr(getter.cast()),
            undefined_value(),
        );
    } else {
        set_field(&destination, b"write", write.with_const_ptr(value_from_ptr));
    }

    let promise = thunk_streamP_pipeline(
        std::ptr::null(),
        crate::closure::plain_call_receiver(),
        source.get_nanbox_f64(),
        destination.get_nanbox_f64(),
        undefined_value(),
    );

    match trigger {
        Trigger::WriteChunks | Trigger::WriteReceiver => assert!(
            MOVED_BY_WRITE.with(Cell::get),
            "the first write must actually relocate the young chunks"
        ),
        Trigger::WriteGetter => {
            assert_eq!(
                WRITE_GETTER_CALLS.with(Cell::get),
                3,
                "the getter must run once per write lookup"
            );
            assert!(
                MOVED_BY_GETTER.with(Cell::get),
                "the getter's own collection must relocate the young chunks"
            );
        }
        Trigger::SourceRead => {
            assert_eq!(READS.with(Cell::get), 1, "_read must run once");
            assert!(
                MOVED_BY_READ.with(Cell::get),
                "_read's collection must relocate the young chunks"
            );
            assert_ne!(
                source.get_nanbox_f64().to_bits(),
                source_before,
                "_read's collection must move the source"
            );
        }
    }
    if trigger == Trigger::WriteChunks {
        assert_eq!(
            destination.get_nanbox_f64().to_bits(),
            destination_before,
            "destination must already be old"
        );
    } else {
        assert_ne!(
            destination.get_nanbox_f64().to_bits(),
            destination_before,
            "destination must actually move"
        );
    }
    let promise = crate::value::js_nanbox_get_pointer(promise) as *mut crate::promise::Promise;
    assert_eq!(
        crate::promise::js_promise_state(promise),
        1,
        "pipeline must fulfil"
    );
    assert_eq!(WRITES.with(Cell::get), 3, "every chunk must be written");
    assert_eq!(ENDS.with(Cell::get), 1);
    assert!(
        !STALE_RECEIVER.with(Cell::get),
        "write must receive the current destination"
    );
    assert!(
        STALE.with(|stale| stale.borrow().is_empty()),
        "native snapshot passed stale chunk addresses at {:?}",
        STALE.with(|stale| stale.borrow().clone())
    );
}

#[test]
fn pipeline_chunk_snapshot_is_refreshed_after_collecting_write() {
    direct_pipeline(Trigger::WriteChunks);
}

#[test]
fn pipeline_receiver_is_refreshed_after_collecting_write() {
    direct_pipeline(Trigger::WriteReceiver);
}

#[test]
fn pipeline_arguments_are_refreshed_after_collecting_method_getter() {
    direct_pipeline(Trigger::WriteGetter);
}

#[test]
fn pipeline_source_is_refreshed_after_collecting_read() {
    direct_pipeline(Trigger::SourceRead);
}

/// `promiseHooks.onInit`: collects on its first call only.
extern "C" fn collecting_init_hook(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _promise: f64,
    _parent: f64,
) -> f64 {
    if HOOK_CALLS.with(|count| count.replace(count.get() + 1)) == 0 {
        crate::gc::gc_collect_minor();
    }
    undefined_value()
}

/// A pipeline function stage: records which stage ran and the closure address
/// it was called through. The last stage returns 42.
fn record_stage(id: u8, closure: *const ClosureHeader) -> f64 {
    STAGES.with(|stages| stages.borrow_mut().push((id, closure as u64)));
    if id == 2 {
        42.0
    } else {
        undefined_value()
    }
}

extern "C" fn stage_a(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _source: f64,
) -> f64 {
    record_stage(0, closure)
}

extern "C" fn stage_b(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _source: f64,
) -> f64 {
    record_stage(1, closure)
}

extern "C" fn stage_c(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _source: f64,
) -> f64 {
    record_stage(2, closure)
}

/// The current address a raw-pointer handle holds.
fn address_of(handle: &crate::gc::RuntimeHandle<'_>) -> usize {
    handle.with_const_ptr(|ptr: *const u8| ptr as usize)
}

/// The collecting init hook, promoted first: the hook table holds it raw, so
/// it must not move once installed ([`HookOff::check`] asserts that).
fn promoted_hook(scope: &RuntimeHandleScope) -> crate::gc::RuntimeHandle<'_> {
    let hook = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(collecting_init_hook, 2; with_declared(2)),
        0,
    ));
    crate::gc::gc_collect_minor();
    crate::gc::gc_collect_minor();
    hook
}

fn install_hook<'a>(
    scope: &'a RuntimeHandleScope,
    hook: &'a crate::gc::RuntimeHandle<'a>,
) -> HookOff<'a> {
    let address = address_of(hook);
    let stop = crate::v8::js_v8_promise_hooks_on_init(hook.with_const_ptr(value_from_ptr));
    HookOff {
        stop: scope.root_nanbox_f64(stop),
        hook,
        address,
    }
}

/// Disables the installed promise hook even if an assertion fails.
struct HookOff<'a> {
    stop: crate::gc::RuntimeHandle<'a>,
    hook: &'a crate::gc::RuntimeHandle<'a>,
    address: usize,
}

impl HookOff<'_> {
    /// The hook ran, collected, and stayed where the table points.
    fn check(&self) {
        assert_eq!(
            address_of(self.hook),
            self.address,
            "test premise: the installed hook must not move"
        );
        assert!(HOOK_CALLS.with(Cell::get) >= 1, "the init hook must run");
    }
}

impl Drop for HookOff<'_> {
    fn drop(&mut self) {
        let stop =
            crate::value::js_nanbox_get_pointer(self.stop.get_nanbox_f64()) as *const ClosureHeader;
        crate::closure::js_closure_call0(stop, crate::closure::plain_call_receiver());
    }
}

/// `pipeline(a, b, [c])` takes the stream-list path. The promise it creates
/// runs a collecting `promiseHooks` init hook, which moves every young input;
/// each stage must still be called through its live address.
#[test]
fn stream_list_pipeline_inputs_are_refreshed_after_collecting_promise_hook() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let hook = promoted_hook(&scope);
    let stage = |info| scope.root_nanbox_f64(value_from_ptr(js_closure_alloc(info, 0).cast()));
    let a = stage(crate::fn_info!(stage_a, 1; with_declared(1)));
    let b = stage(crate::fn_info!(stage_b, 1; with_declared(1)));
    let c = stage(crate::fn_info!(stage_c, 1; with_declared(1)));
    let rest = scope.root_raw_mut_ptr(crate::array::js_array_alloc(1));
    rest.set_raw_mut_ptr(
        rest.with_mut_ptr(|rest| crate::array::js_array_push_f64(rest, c.get_nanbox_f64())),
    );
    let before = [
        a.get_nanbox_f64().to_bits(),
        b.get_nanbox_f64().to_bits(),
        c.get_nanbox_f64().to_bits(),
        address_of(&rest) as u64,
    ];

    let hook_off = install_hook(&scope, &hook);
    let promise = thunk_streamP_pipeline(
        std::ptr::null(),
        crate::closure::plain_call_receiver(),
        a.get_nanbox_f64(),
        b.get_nanbox_f64(),
        rest.with_const_ptr(value_from_ptr),
    );

    hook_off.check();
    let after = [
        a.get_nanbox_f64().to_bits(),
        b.get_nanbox_f64().to_bits(),
        c.get_nanbox_f64().to_bits(),
        address_of(&rest) as u64,
    ];
    for (i, (before, after)) in before.iter().zip(after.iter()).enumerate() {
        assert_ne!(
            before, after,
            "input {i} must actually move in the hook's collection"
        );
    }
    let live = |handle: &crate::gc::RuntimeHandle<'_>| {
        handle.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK
    };
    assert_eq!(
        STAGES.with(|stages| stages.borrow().clone()),
        vec![(0, live(&a)), (1, live(&b)), (2, live(&c))],
        "every stage must run once, through its live address"
    );
    let promise = crate::value::js_nanbox_get_pointer(promise) as *mut crate::promise::Promise;
    assert_eq!(
        crate::promise::js_promise_state(promise),
        1,
        "pipeline must fulfil"
    );
    assert_eq!(crate::promise::js_promise_value(promise), 42.0);
}

/// A source holding an error rejects the pipeline with it. Creating the
/// rejected promise runs a collecting init hook, which moves the young error;
/// the promise must hold the error's live address.
#[test]
fn pipeline_rejection_reason_is_refreshed_after_collecting_promise_hook() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let hook = promoted_hook(&scope);
    let undef = undefined_value();
    let source = scope.root_nanbox_f64(crate::node_stream::js_node_stream_readable_from(
        value_from_ptr(crate::array::js_array_alloc(0).cast()),
    ));
    let error = scope.root_nanbox_f64(value_from_ptr(js_object_alloc(0, 1).cast()));
    crate::node_stream::test_set_hidden_error(source.get_nanbox_f64(), error.get_nanbox_f64());
    let destination = scope.root_nanbox_f64(value_from_ptr(js_object_alloc(0, 1).cast()));
    let error_before = error.get_nanbox_f64().to_bits();

    let hook_off = install_hook(&scope, &hook);
    let promise = thunk_streamP_pipeline(
        std::ptr::null(),
        crate::closure::plain_call_receiver(),
        source.get_nanbox_f64(),
        destination.get_nanbox_f64(),
        undef,
    );

    hook_off.check();
    assert_ne!(
        error.get_nanbox_f64().to_bits(),
        error_before,
        "the error must actually move in the hook's collection"
    );
    let promise = crate::value::js_nanbox_get_pointer(promise) as *mut crate::promise::Promise;
    assert_eq!(
        crate::promise::js_promise_state(promise),
        2,
        "pipeline must reject"
    );
    assert_eq!(
        crate::promise::js_promise_reason(promise).to_bits(),
        error.get_nanbox_f64().to_bits(),
        "the rejection must carry the error's live address"
    );
}
