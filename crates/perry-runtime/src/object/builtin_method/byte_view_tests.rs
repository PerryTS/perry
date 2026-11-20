//! The byte-view `copyWithin` entry: its receiver proof and the memmove body.
use super::*;

fn boxed<T>(p: *mut T) -> f64 {
    crate::value::js_nanbox_pointer(p as i64)
}

fn write(view: f64, bytes: &[u8]) {
    crate::buffer::bytes::no_gc(|scope| unsafe {
        crate::buffer::bytes::bytes_mut(view, scope)
            .unwrap()
            .copy_from_slice(bytes)
    });
}

fn read(view: f64) -> Vec<u8> {
    crate::buffer::bytes::no_gc(|scope| crate::buffer::bytes::bytes(view, scope).unwrap().to_vec())
}

fn copy_within(view: f64, args: &[f64]) -> f64 {
    let site = 0;
    let key = crate::value::TAG_UNDEFINED;
    unsafe {
        match *args {
            [t] => js_bm_copyWithin1(site, view, key, 0, t),
            [t, s] => js_bm_copyWithin2(site, view, key, 0, t, s),
            [t, s, e] => js_bm_copyWithin3(site, view, key, 0, t, s, e),
            _ => unreachable!(),
        }
    }
}

#[test]
fn byte_view_copy_within_moves_the_selected_range() {
    let u = boxed(crate::buffer::js_uint8array_alloc(6));
    let cases: &[(&[f64], [u8; 6])] = &[
        (&[2.0], [1, 2, 1, 2, 3, 4]),
        (&[0.0, 3.0], [4, 5, 6, 4, 5, 6]),
        (&[2.0, 0.0, 4.0], [1, 2, 1, 2, 3, 4]),
        (&[0.0, 2.0, 6.0], [3, 4, 5, 6, 5, 6]),
        (&[-2.0, -4.0, -1.0], [1, 2, 3, 4, 3, 4]),
        (&[0.0, 3.0, UNDEFINED], [4, 5, 6, 4, 5, 6]),
        (&[f64::NAN, 4.0, f64::INFINITY], [5, 6, 3, 4, 5, 6]),
        (&[0.0, 4.0, 2.0], [1, 2, 3, 4, 5, 6]),
    ];
    for (args, expected) in cases {
        write(u, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(copy_within(u, args).to_bits(), u.to_bits(), "{args:?}");
        assert_eq!(read(u), expected, "{args:?}");
    }
}

#[test]
fn a_buffer_and_a_subarray_view_are_byte_views() {
    let b = boxed(crate::buffer::js_buffer_alloc(5, 0));
    write(b, &[1, 2, 3, 4, 5]);
    copy_within(b, &[0.0, 3.0, 5.0]);
    assert_eq!(read(b), [4, 5, 3, 4, 5]);

    let owner = crate::buffer::js_uint8array_alloc(6);
    write(boxed(owner), &[9, 8, 7, 6, 5, 4]);
    let view = boxed(crate::buffer::js_buffer_slice(owner, 1, 5));
    unsafe { assert!(byte_view_ok(view)) };
    copy_within(view, &[0.0, 2.0, 4.0]);
    assert_eq!(read(view), [6, 5, 6, 5]);
    assert_eq!(read(boxed(owner)), [9, 6, 5, 6, 5, 4]);
}

#[test]
fn a_receiver_with_a_property_bag_or_another_kind_is_declined() {
    unsafe {
        let plain = crate::buffer::js_uint8array_alloc(3);
        assert!(byte_view_ok(boxed(plain)));
        let own = crate::buffer::js_uint8array_alloc(3);
        crate::buffer::buffer_define_own_data_prop(own as usize, "copyWithin", 1.0);
        assert!(!byte_view_ok(boxed(own)));
        let int16 = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_INT16, 3);
        assert!(!byte_view_ok(boxed(int16)));
        let ab = crate::buffer::js_array_buffer_new(3);
        assert!(!byte_view_ok(boxed(ab)));
        assert!(!byte_view_ok(boxed(crate::object::js_object_alloc(0, 0))));
        assert!(!byte_view_ok(3.0));
    }
}
