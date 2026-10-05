use super::access::js_ta_read_receiver_is_kind;
use super::*;

#[test]
fn typed_array_reads_use_resolved_slot_for_every_numeric_kind() {
    for kind in [0, 2, 3, 4, 5, 6, 7, 8, 11] {
        let ta = typed_array_alloc(kind, 4);
        unsafe {
            store_at(ta, 0, -7.5);
            store_at(ta, 1, 13.25);
        }
        let expected = unsafe { [load_at(ta, 0), load_at(ta, 1)] };
        let boxed = ta_receiver_value(ta);
        assert_eq!(js_ta_read_receiver_is_kind(boxed, kind as i32), 1);
        let backing = crate::typedarray_view::js_typed_array_backing_buffer(ta);
        unsafe {
            assert_eq!((*ta).storage, TA_STORAGE_RESOLVED);
            assert_eq!(resolved_data(ta), crate::buffer::buffer_data_mut(backing));
            assert_eq!([load_at(ta, 0), load_at(ta, 1)], expected);
            // Change the resolved slot: hot reads must use the
            // slot, not recover it from a side table. Restored below.
            set_resolved_data(ta, data_ptr_mut(ta).add(elem_size_for_kind(kind)));
            assert_eq!(load_at(ta, 0), expected[1]);
            set_resolved_data(ta, crate::buffer::buffer_data_mut(backing));
        }
        assert_eq!(
            js_ta_read_receiver_is_kind(boxed, ((kind + 1) % 12) as i32),
            0
        );
    }
    assert_eq!(js_ta_read_receiver_is_kind(42.0, 4), 0);
    let byte = crate::buffer::js_uint8array_alloc(4);
    assert_eq!(
        js_ta_read_receiver_is_kind(crate::value::js_nanbox_pointer(byte as i64), 4),
        0
    );
}

#[test]
fn empty_and_short_arrays_reserve_a_pointer_slot() {
    for kind in [0, 2, 3, 4, 5, 6, 7, 8, 11] {
        for len in [0, 1] {
            let ta = typed_array_alloc(kind, len);
            let backing = crate::typedarray_view::js_typed_array_backing_buffer(ta);
            unsafe {
                assert_eq!((*ta).storage, TA_STORAGE_RESOLVED);
                assert_eq!(resolved_data(ta), crate::buffer::buffer_data_mut(backing));
            }
        }
    }
}

#[test]
fn resolved_slot_tracks_gc_backing_rewrite() {
    let ta = typed_array_alloc(KIND_FLOAT64, 2);
    let first = crate::typedarray_view::js_typed_array_backing_buffer(ta);
    let second = crate::buffer::buffer_alloc(16);
    unsafe {
        *(crate::buffer::buffer_data_mut(second) as *mut f64) = 19.5;
    }
    let valid = crate::gc::build_valid_pointer_set();
    unsafe {
        let header = (first as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        crate::gc::set_forwarding_address(header, second as *mut u8);
    }
    let mut visitor = crate::gc::RuntimeRootVisitor::for_rewrite(&valid);
    crate::typedarray_view::scan_typed_array_view_meta_roots_mut(&mut visitor);
    unsafe {
        assert_eq!(load_at(ta, 0), 19.5);
    }
}
