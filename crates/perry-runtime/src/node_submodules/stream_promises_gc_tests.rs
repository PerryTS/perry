//! Native pipeline snapshots must remain current across user callbacks.
use super::*;
use crate::gc::RuntimeHandleScope;
use std::cell::{Cell, RefCell};

thread_local! {
    static WRITES: Cell<usize> = const { Cell::new(0) };
    static ENDS: Cell<usize> = const { Cell::new(0) };
    static MOVED: Cell<bool> = const { Cell::new(false) };
    static STALE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static STALE_RECEIVER: Cell<bool> = const { Cell::new(false) };
}

extern "C" fn collecting_write(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    _encoding: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let expected = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    if this.as_f64().to_bits() != js_closure_get_capture_f64(closure, 1).to_bits() {
        STALE_RECEIVER.with(|stale| stale.set(true));
    }
    let index = WRITES.with(|count| {
        let index = count.get();
        count.set(index + 1);
        index
    });
    let array = crate::value::js_nanbox_get_pointer(expected.get_nanbox_f64())
        as *const crate::array::ArrayHeader;
    let current = crate::array::js_array_get_f64(array, index as u32);
    // Compare addresses without dereferencing a stale callback argument. The
    // independent GC array keeps every expected string alive and is rewritten.
    if chunk.to_bits() != current.to_bits() {
        STALE.with(|stale| stale.borrow_mut().push(index));
    }
    if index == 0 {
        let before = expected.get_nanbox_f64().to_bits();
        crate::gc::gc_collect_minor();
        MOVED.with(|moved| moved.set(moved.get() || before != expected.get_nanbox_f64().to_bits()));
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

extern "C" fn collecting_write_getter(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let method = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let expected = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let before = expected.get_nanbox_f64().to_bits();
    crate::gc::gc_collect_minor();
    MOVED.with(|moved| moved.set(moved.get() || before != expected.get_nanbox_f64().to_bits()));
    method.get_nanbox_f64()
}

#[test]
fn pipeline_chunk_snapshot_is_refreshed_after_collecting_write() {
    pipeline_snapshot(true, false);
}

#[test]
fn pipeline_receiver_is_refreshed_after_collecting_write() {
    pipeline_snapshot(false, false);
}

#[test]
fn pipeline_arguments_are_refreshed_after_collecting_method_getter() {
    pipeline_snapshot(false, true);
}

fn pipeline_snapshot(promote_destination: bool, use_getter: bool) {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::gc::gc_register_mutable_root_scanner(
        crate::object::descriptor_state::scan_descriptor_roots_mut,
    );
    WRITES.with(|count| count.set(0));
    ENDS.with(|count| count.set(0));
    MOVED.with(|moved| moved.set(false));
    STALE.with(|stale| stale.borrow_mut().clear());
    STALE_RECEIVER.with(|stale| stale.set(false));

    let scope = RuntimeHandleScope::new();
    let destination = scope.root_nanbox_f64(value_from_ptr(js_object_alloc(0, 4).cast()));
    // Promote only the destination first: this test isolates the native chunk
    // snapshot from the separate problem of refreshing a moving receiver.
    if promote_destination {
        crate::gc::gc_collect_minor();
        crate::gc::gc_collect_minor();
    }
    let destination_before = destination.get_nanbox_f64().to_bits();
    let expected = scope.root_nanbox_f64(value_from_ptr(crate::array::js_array_alloc(3).cast()));
    let mut chunks = Vec::new();
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
        chunks.push(chunk);
    }
    let write = js_closure_alloc(crate::fn_info!(collecting_write, 2; with_declared(2)), 2);
    js_closure_set_capture_f64(write, 0, expected.get_nanbox_f64());
    js_closure_set_capture_f64(write, 1, destination.get_nanbox_f64());
    let end = js_closure_alloc(crate::fn_info!(count_end, 1; with_declared(1)), 0);
    for (key, method) in [(b"write".as_slice(), write), (b"end".as_slice(), end)] {
        js_object_set_field_by_name(
            crate::value::js_nanbox_get_pointer(destination.get_nanbox_f64()) as *mut ObjectHeader,
            js_string_from_bytes(key.as_ptr(), key.len() as u32),
            value_from_ptr(method.cast()),
        );
    }
    if use_getter {
        let getter = js_closure_alloc(
            crate::fn_info!(collecting_write_getter, 0; with_declared(0)),
            2,
        );
        js_closure_set_capture_f64(getter, 0, value_from_ptr(write.cast()));
        js_closure_set_capture_f64(getter, 1, expected.get_nanbox_f64());
        crate::object::set_accessor_descriptor(
            crate::value::js_nanbox_get_pointer(destination.get_nanbox_f64()) as usize,
            "write".to_string(),
            crate::object::AccessorDescriptor {
                get: value_from_ptr(getter.cast()).to_bits(),
                set: 0,
            },
        );
    }

    write_chunks_to_destination(destination.get_nanbox_f64(), &chunks);
    assert!(
        MOVED.with(Cell::get),
        "callback must actually relocate the expected young array"
    );
    if promote_destination {
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
    assert_eq!(WRITES.with(Cell::get), 3);
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
