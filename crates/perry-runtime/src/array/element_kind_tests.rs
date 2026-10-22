//! The dense prefix P of the element-kind word (EK design §3.1, §5): one
//! test per store family. Each asserts the exact prefix the family's rule
//! gives and runs the word check (`ek_verify`), so dropping a rule either
//! leaves a stale (too high) prefix the check rejects or a lower prefix the
//! exact assertion rejects.

use super::*;

const HOLE: u64 = crate::value::TAG_HOLE;

unsafe fn prefix(arr: *mut ArrayHeader) -> u32 {
    let live = clean_arr_ptr_mut(arr);
    element_kind::ek_verify(live);
    (*live).ek_prefix
}

/// A length-`len` array whose `present` slots hold their index + 1 (stored
/// front to back, so the frontier row raises P over the leading run).
fn holey(len: u32, present: &[u32]) -> *mut ArrayHeader {
    let arr = js_array_alloc_with_length(len);
    for &i in present {
        js_array_set_f64(arr, i, f64::from(i + 1));
    }
    arr
}

fn pushed(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(0);
    for &v in values {
        arr = js_array_push_f64(arr, v);
    }
    arr
}

#[test]
fn frontier_store_raises_and_a_hole_lowers() {
    unsafe {
        let arr = holey(7, &[0, 1, 2, 6]);
        assert_eq!(
            prefix(arr),
            3,
            "stores at the frontier raise P; a store past it does not"
        );
        assert_eq!(js_array_delete(arr, 1), 1);
        assert_eq!(
            prefix(arr),
            1,
            "delete inside the prefix lowers P to the index"
        );
        assert_eq!(*array_elements_ptr(arr).add(1), HOLE);
    }
}

#[test]
fn push_raises_after_the_length_is_published() {
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0]);
        assert_eq!(prefix(arr), 3);
        let arr = js_array_push_hole(arr);
        assert_eq!(prefix(arr), 3, "a pushed hole is past the frontier's proof");
    }
}

#[test]
fn length_pop_and_truncate_lower_to_the_new_length() {
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        js_array_set_length(arr, 2.0);
        assert_eq!(prefix(arr), 2, "length = n");
        js_array_pop_f64(arr);
        assert_eq!(prefix(arr), 1, "pop");
        js_array_pop_f64(arr);
        assert_eq!(prefix(arr), 0, "the last pop resets");
        let arr = js_array_push_f64(arr, 9.0);
        assert_eq!(prefix(arr), 1, "a push after the reset starts over");
    }
}

#[test]
fn shift_drops_the_prefix_by_one() {
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0]);
        js_array_shift_f64(arr);
        assert_eq!(prefix(arr), 2);
        js_array_shift_f64(arr);
        js_array_shift_f64(arr);
        assert_eq!(prefix(arr), 0, "the last shift resets");
        let arr = js_array_push_f64(arr, 4.0);
        assert_eq!(prefix(arr), 1);
    }
}

#[test]
fn splice_in_the_prefix_follows_the_moved_hole() {
    unsafe {
        // [1,2,3,·,·,·,7], P = 3; splice(0, 1) gives [2,3,·,·,·,7]: slot 2 is a hole.
        let arr = holey(7, &[0, 1, 2, 6]);
        assert_eq!(prefix(arr), 3);
        let mut out = std::ptr::null_mut();
        js_array_splice(arr, 0, 1, std::ptr::null(), 0, &mut out);
        assert_eq!(prefix(out), 2);
        // splice(1, 2, 9) on the same start gives [1,9,·,·,·,·,7]: P = 2.
        let arr = holey(7, &[0, 1, 2, 6]);
        let items = [9.0];
        let mut out = std::ptr::null_mut();
        js_array_splice(arr, 1, 2, items.as_ptr(), 1, &mut out);
        assert_eq!(prefix(out), 2);
        // An insertion inside a dense prefix moves it up: P + k.
        let arr = pushed(&[1.0, 2.0, 3.0]);
        let items = [7.0, 8.0];
        let mut out = std::ptr::null_mut();
        js_array_splice(arr, 1, 0, items.as_ptr(), 2, &mut out);
        assert_eq!(prefix(out), 5);
    }
}

#[test]
fn copy_within_over_a_hole_lowers_the_prefix() {
    unsafe {
        // [1,2,·,4], P = 2; copyWithin(0, 1) gives [2,·,4,4]: P = 1.
        let arr = holey(4, &[0, 1, 3]);
        assert_eq!(prefix(arr), 2);
        let arr = js_array_copy_within(arr, 0.0, 1.0, 0, 0.0);
        assert_eq!(prefix(arr), 1);
        assert_eq!(*array_elements_ptr(clean_arr_ptr_mut(arr)).add(1), HOLE);
    }
}

#[test]
fn unshift_moves_the_prefix_up_by_the_item_count() {
    unsafe {
        // [1,·], P = 1; unshift(8, 9) gives [8,9,1,·]: P = 3, slot 3 a hole.
        let arr = holey(2, &[0]);
        let items = [8.0, 9.0];
        let arr = js_array_unshift_variadic(arr, items.as_ptr(), 2);
        assert_eq!(prefix(arr), 3);
        let arr = js_array_unshift_f64(arr, 7.0);
        assert_eq!(prefix(arr), 4);
    }
}

#[test]
fn reverse_keeps_only_a_fully_dense_prefix() {
    unsafe {
        let arr = js_array_reverse(pushed(&[1.0, 2.0, 3.0]));
        assert_eq!(prefix(arr), 3);
        // [1,2,·] reversed is [·,2,1].
        let arr = js_array_reverse(holey(3, &[0, 1]));
        assert_eq!(prefix(arr), 0);
    }
}

#[test]
fn sort_packs_values_ahead_of_the_holes() {
    unsafe {
        // [3,·,1] sorts to [1,3,·]: P = 2.
        let arr = js_array_alloc_with_length(3);
        js_array_set_f64(arr, 0, 3.0);
        js_array_set_f64(arr, 2, 1.0);
        assert_eq!(prefix(arr), 1);
        let arr = js_array_sort_default(arr);
        assert_eq!(prefix(arr), 2);
    }
}

#[test]
fn fill_extends_a_prefix_it_starts_in() {
    unsafe {
        let arr = js_array_fill(js_array_alloc_with_length(5), 0.0);
        assert_eq!(prefix(arr), 5);
        let arr = holey(6, &[0, 1]);
        let arr = js_array_fill_range(arr, 0.0, 1.0, 4.0);
        assert_eq!(prefix(arr), 4, "fill(v, 1, 4) from inside the prefix");
        let arr = js_array_fill_range(arr, 0.0, 5.0, 6.0);
        assert_eq!(
            prefix(arr),
            4,
            "a range past the prefix proves nothing below it"
        );
    }
}

fn define_read_only(arr: *mut ArrayHeader, index: u32, value: f64) {
    let descriptor = crate::object::js_object_alloc_null_proto(0, 0);
    crate::object::js_object_set_field_by_name(
        descriptor,
        crate::string::intern_ascii_literal(b"value"),
        value,
    );
    crate::object::js_object_set_field_by_name(
        descriptor,
        crate::string::intern_ascii_literal(b"writable"),
        f64::from_bits(crate::value::TAG_FALSE),
    );
    let key = index.to_string();
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_define_property(
        crate::value::js_nanbox_pointer(arr as i64),
        crate::value::js_nanbox_string(key as i64),
        crate::value::js_nanbox_pointer(descriptor as i64),
    );
}

#[test]
fn an_index_descriptor_lowers_the_prefix_and_clamps_every_rise() {
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0, 4.0]);
        define_read_only(arr, 1, 42.0);
        assert_eq!(prefix(arr), 1, "a define on an index inside the prefix");
        // A descriptor at key 5, past the prefix: no raising row may cross it.
        let arr = pushed(&[1.0, 2.0, 3.0]);
        define_read_only(arr, 5, 42.0);
        assert_eq!(prefix(arr), 3);
        js_array_set_f64(arr, 3, 0.0);
        js_array_set_f64(arr, 4, 0.0);
        assert_eq!(prefix(arr), 3, "frontier stores under the descriptor clamp");
        let arr = js_array_fill(arr, 0.0);
        assert_eq!(prefix(arr), 3, "fill under the descriptor clamp");
        let items = [7.0, 8.0, 9.0];
        let mut out = std::ptr::null_mut();
        js_array_splice(arr, 0, 0, items.as_ptr(), 3, &mut out);
        assert_eq!(prefix(out), 3, "splice under the descriptor clamp");
        // (`unshift` here would throw: its Set of the read-only key 5 fails.)
    }
}
