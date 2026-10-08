use super::access::js_ta_read_receiver_is_kind;
use super::*;

#[test]
fn buffer_access_preserves_every_numeric_kind_and_inline_pointer() {
    for kind in [0, 2, 3, 4, 5, 6, 7, 8, 11] {
        let ta = typed_array_alloc(kind, 4);
        unsafe {
            store_at(ta, 0, -7.5);
            store_at(ta, 1, 13.25);
        }
        let expected = unsafe { [load_at(ta, 0), load_at(ta, 1)] };
        let before = data_ptr(ta);
        let backing = crate::typedarray_view::js_typed_array_backing_buffer(ta);
        assert_eq!(
            js_ta_read_receiver_is_kind(ta_receiver_value(ta), kind as i32),
            1
        );
        unsafe {
            assert_eq!(
                data_ptr(ta),
                before,
                ".buffer must never externalize inline bytes"
            );
            crate::buffer::bytes::no_gc(|_| {
                assert_eq!(
                    data_ptr(ta),
                    crate::buffer::bytes::span(
                        crate::value::js_nanbox_pointer(backing as i64),
                        false
                    )
                    .unwrap()
                    .ptr as *const u8
                )
            });
            assert_eq!([load_at(ta, 0), load_at(ta, 1)], expected);
            assert_eq!(crate::buffer::store::owner(backing as usize), ta as usize);
        }
        assert_eq!(
            js_ta_read_receiver_is_kind(ta_receiver_value(ta), ((kind + 1) % 12) as i32),
            0
        );
    }
    assert_eq!(js_ta_read_receiver_is_kind(42.0, 4), 0);
}

#[test]
fn empty_and_short_arrays_keep_exactly_the_common_cell_and_bytes() {
    for kind in 0..12 {
        for len in [0, 1] {
            let ta = typed_array_alloc(kind, len);
            let before = data_ptr(ta);
            let backing = crate::typedarray_view::js_typed_array_backing_buffer(ta);
            unsafe {
                assert_eq!(data_ptr(ta), before);
                crate::buffer::bytes::no_gc(|_| {
                    assert_eq!(
                        crate::buffer::bytes::span(
                            crate::value::js_nanbox_pointer(backing as i64),
                            false
                        )
                        .unwrap()
                        .ptr as *const u8,
                        before
                    )
                });
                assert_eq!(
                    crate::buffer::store::capacity(ta as usize) as usize,
                    len as usize * elem_size_for_kind(kind)
                );
            }
        }
    }
}

#[test]
fn view_link_is_the_only_authority_for_the_resolved_bytes() {
    let first = typed_array_alloc(KIND_FLOAT64, 2);
    let second = typed_array_alloc(KIND_FLOAT64, 2);
    unsafe {
        store_at(first, 0, 1.5);
        store_at(second, 0, 19.5);
    }
    let view =
        crate::buffer::store::new_view(type_for_kind(KIND_FLOAT64), first as usize, 0, 2, false);
    unsafe {
        assert_eq!(load_at(view, 0), 1.5);
        crate::buffer::store::set_test_link(view as usize, second as usize);
        assert_eq!(load_at(view, 0), 19.5);
    }
}

#[test]
fn native_resolution_uses_the_view_owner_and_offset() {
    let owner = typed_array_alloc(KIND_UINT32, 8);
    let view =
        crate::buffer::store::new_view(type_for_kind(KIND_UINT32), owner as usize, 8, 2, false);
    assert_eq!(data_ptr(view), unsafe { data_ptr(owner).add(8) });
}
