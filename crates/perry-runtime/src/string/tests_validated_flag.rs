//! `STRING_FLAG_WTF8_VALIDATED` describes one header's payload (#10166). A
//! RegExp trusts it to bind in constant work without decoding, so it must never
//! reach any other string: a wrong bit gives wrong answers or a panic.
use super::*;

fn heap(text: &str) -> *mut StringHeader {
    js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

/// A shared heap string marked the way the regex binding marks one.
fn validated(text: &str) -> *mut StringHeader {
    let s = heap(text);
    unsafe {
        (*s).refcount = 0;
        (*s).flags |= STRING_FLAG_WTF8_VALIDATED;
    }
    s
}

fn carries(s: *const StringHeader) -> bool {
    unsafe { (*s).flags & STRING_FLAG_WTF8_VALIDATED != 0 }
}

fn assert_not_inherited(name: &str, source: *const StringHeader, derived: *const StringHeader) {
    if derived != source {
        assert!(
            !carries(derived),
            "{name} copied the validation of its source"
        );
    }
}

#[test]
fn string_validated_flag_is_never_inherited_by_a_derived_string() {
    let v = validated("abcdef");
    let w = validated("xyz");
    assert!(carries(v) && carries(w));
    assert_not_inherited("slice", v, js_string_slice(v, 1, 4));
    assert_not_inherited("substring", v, js_string_substring(v, 0, 2));
    assert_not_inherited("trim", v, js_string_trim(validated("  abc  ")));
    assert_not_inherited("concat", v, js_string_concat(v, w));
    assert_not_inherited("repeat", v, js_string_repeat(v, 3.0));
    assert_not_inherited("padStart", v, js_string_pad_start(v, 12.0, w));
    assert_not_inherited("toUpperCase", v, crate::string::js_string_to_upper_case(v));
    let array = crate::array::js_array_alloc(2);
    let array = crate::array::js_array_push_f64(array, crate::value::js_nanbox_string(v as i64));
    let array = crate::array::js_array_push_f64(array, crate::value::js_nanbox_string(w as i64));
    assert_not_inherited(
        "join",
        v,
        crate::array::js_array_join(array, validated(",")),
    );
    // Constructors that take a caller-computed flags word, passed the source's
    // whole word on purpose: the funnel must still strip the validation.
    let whole = unsafe { (*v).flags };
    assert_not_inherited("string_copy_range", v, string_copy_range(v, 0, 3, 3, whole));
    assert_not_inherited(
        "js_string_from_bytes_known_utf16",
        v,
        js_string_from_bytes_known_utf16(b"abc".as_ptr(), 3, 3, whole),
    );
}

#[test]
fn string_in_place_append_clears_the_destinations_validation() {
    // A unique destination with spare capacity is appended in place. It cannot
    // normally be validated (validation happens on shared strings); mark it
    // anyway to prove the writers clear the bit when the payload changes.
    let dest = js_string_from_bytes_with_capacity(b"ab".as_ptr(), 2, 64);
    let piece = validated("cd");
    unsafe {
        (*dest).refcount = 1;
        (*dest).flags |= STRING_FLAG_WTF8_VALIDATED;
    }
    let appended = js_string_append(dest, piece);
    assert_eq!(appended, dest, "the test needs the in-place path");
    assert!(
        !carries(appended),
        "js_string_append kept a stale validation"
    );

    let chain_dest = js_string_from_bytes_with_capacity(b"ab".as_ptr(), 2, 64);
    unsafe {
        (*chain_dest).refcount = 1;
        (*chain_dest).flags |= STRING_FLAG_WTF8_VALIDATED;
    }
    let parts = [
        crate::value::js_nanbox_string(chain_dest as i64),
        crate::value::js_nanbox_string(validated("cd") as i64),
        crate::value::js_nanbox_string(validated("ef") as i64),
    ];
    let chained = js_string_append_chain(parts.as_ptr(), 3);
    assert_eq!(
        chained, chain_dest,
        "the test needs the in-place chain path"
    );
    assert!(
        !carries(chained),
        "js_string_append_chain kept a stale validation"
    );
}
