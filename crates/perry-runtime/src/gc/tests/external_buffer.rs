//! Honest external-buffer storage: the wrapper is managed; its bytes are native.
use super::super::*;
use super::support::*;

#[test]
fn external_buffer_producer_returns_headered_distinct_cells_and_exact_bytes() {
    let _guard = GcTestIsolationGuard::new();
    let mut bytes = [11u8, 22, 33, 44];
    let first = crate::buffer::buffer_alloc_foreign(bytes.as_mut_ptr(), 4);
    let second = crate::buffer::buffer_alloc_foreign(bytes.as_mut_ptr(), 4);
    // Gate B precedes any dereference: both small handles and headerless Boxes fail.
    for ptr in [first, second] {
        assert!(!crate::value::addr_class::is_handle_band(ptr as usize));
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(ptr as usize) }
            .expect("external-buffer producer must return an allocator-owned cell");
        assert_eq!(unsafe { header.as_ref().obj_type }, GC_TYPE_BUFFER);
        let payload_size =
            std::mem::size_of::<crate::buffer::BufferHeader>() + std::mem::size_of::<*mut u8>();
        assert!(unsafe { header.as_ref().size as usize } >= GC_HEADER_SIZE + payload_size);
        assert_eq!(
            unsafe { *((ptr as *const u8).add(8) as *const *mut u8) },
            bytes.as_mut_ptr(),
            "the native address must live in this cell, not a side table"
        );
        assert!(crate::buffer::is_foreign_backed_buffer(ptr as usize));
        assert_eq!(crate::buffer::buffer_data(ptr), bytes.as_ptr());
        assert_eq!(unsafe { (*ptr).length }, 4);
    }
    assert_ne!(first, second, "two wrappers must have distinct identity");
    crate::buffer::js_buffer_set(first, 1, 99);
    assert_eq!(bytes, [11, 99, 33, 44]);
    assert_eq!(crate::buffer::js_buffer_get(second, 1), 99);
    let plain = crate::buffer::buffer_alloc(4);
    assert!(!crate::buffer::is_foreign_backed_buffer(plain as usize));
}

#[test]
fn external_buffer_cell_survives_gc_then_releases_its_registration() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let mut bytes = [17u8, 29];
    let ptr = crate::buffer::buffer_alloc_foreign(bytes.as_mut_ptr(), 2);
    let addr = ptr as usize;
    js_shadow_slot_set(0, ptr_bits(addr));
    let _ = gc_collect_minor();
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(
        js_shadow_slot_get(0),
        ptr_bits(addr),
        "FFI wrapper cannot move"
    );
    assert!(crate::buffer::is_registered_buffer(addr));
    assert!(crate::buffer::is_foreign_backed_buffer(addr));
    assert_eq!(crate::buffer::buffer_data(ptr), bytes.as_ptr());
    assert_eq!(crate::buffer::js_buffer_get(ptr, 1), 29);
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(
        !crate::buffer::is_registered_buffer(addr),
        "dead wrapper registration must disappear"
    );
    // A borrowed byte span is not the collector's resource to free or overwrite.
    assert_eq!(bytes, [17, 29]);
}

#[test]
fn external_buffer_crosses_a_worker_by_value_like_an_ordinary_buffer() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let mut bytes = [42u8, 43];
    let ptr = crate::buffer::buffer_alloc_foreign(bytes.as_mut_ptr(), 2);
    js_shadow_slot_set(0, ptr_bits(ptr as usize));
    let plain = crate::buffer::buffer_alloc(2);
    js_shadow_slot_set(1, ptr_bits(plain as usize));
    crate::buffer::js_buffer_set(plain, 0, 42);
    crate::buffer::js_buffer_set(plain, 1, 43);
    // The native backing must be invisible at the boundary: a foreign wrapper
    // serializes exactly as an ordinary buffer holding the same bytes (a
    // structured clone of an external ArrayBuffer copies it in node).
    for mark_uint8 in [false, true] {
        if mark_uint8 {
            crate::buffer::mark_as_uint8array(ptr as usize);
            crate::buffer::mark_as_uint8array(plain as usize);
        }
        let foreign = unsafe { crate::thread::serialize_nanbox_for_thread(ptr_bits(ptr as usize)) };
        let ordinary =
            unsafe { crate::thread::serialize_nanbox_for_thread(ptr_bits(plain as usize)) };
        assert_eq!(
            crate::thread::first_unsupported_transfer_type(&foreign),
            crate::thread::first_unsupported_transfer_type(&ordinary)
        );
        if mark_uint8 {
            match foreign {
                crate::thread::SerializedValue::Uint8Array(copy) => assert_eq!(copy, [42, 43]),
                _ => panic!("a foreign Uint8Array must cross as a byte copy"),
            }
        }
    }
    // The copy is a snapshot; the native span stays caller-owned.
    assert_eq!(bytes, [42, 43]);
}
