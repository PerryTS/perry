//! Native pipeline snapshots must remain current across user callbacks.
use super::*;
use crate::gc::RuntimeHandleScope;
use std::cell::{Cell, RefCell};

thread_local! {
    static WRITES: Cell<usize> = const { Cell::new(0) };
    static ENDS: Cell<usize> = const { Cell::new(0) };
    static MOVED: Cell<bool> = const { Cell::new(false) };
    static STALE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn collecting_write(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
    _encoding: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let expected = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
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
        MOVED.with(|moved| moved.set(before != expected.get_nanbox_f64().to_bits()));
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

#[test]
fn pipeline_chunk_snapshot_is_refreshed_after_collecting_write() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    WRITES.with(|count| count.set(0));
    ENDS.with(|count| count.set(0));
    MOVED.with(|moved| moved.set(false));
    STALE.with(|stale| stale.borrow_mut().clear());

    let scope = RuntimeHandleScope::new();
    let destination = scope.root_nanbox_f64(value_from_ptr(js_object_alloc(0, 4).cast()));
    // Promote only the destination first: this test isolates the native chunk
    // snapshot from the separate problem of refreshing a moving receiver.
    crate::gc::gc_collect_minor();
    crate::gc::gc_collect_minor();
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
    let write = js_closure_alloc(crate::fn_info!(collecting_write, 2; with_declared(2)), 1);
    js_closure_set_capture_f64(write, 0, expected.get_nanbox_f64());
    let end = js_closure_alloc(crate::fn_info!(count_end, 1; with_declared(1)), 0);
    for (key, method) in [(b"write".as_slice(), write), (b"end".as_slice(), end)] {
        js_object_set_field_by_name(
            crate::value::js_nanbox_get_pointer(destination.get_nanbox_f64()) as *mut ObjectHeader,
            js_string_from_bytes(key.as_ptr(), key.len() as u32),
            value_from_ptr(method.cast()),
        );
    }

    write_chunks_to_destination(destination.get_nanbox_f64(), &chunks);
    assert!(
        MOVED.with(Cell::get),
        "callback must actually relocate the expected young array"
    );
    assert_eq!(
        destination.get_nanbox_f64().to_bits(),
        destination_before,
        "destination must already be old"
    );
    assert_eq!(WRITES.with(Cell::get), 3);
    assert_eq!(ENDS.with(Cell::get), 1);
    assert!(
        STALE.with(|stale| stale.borrow().is_empty()),
        "native snapshot passed stale chunk addresses at {:?}",
        STALE.with(|stale| stale.borrow().clone())
    );
}
