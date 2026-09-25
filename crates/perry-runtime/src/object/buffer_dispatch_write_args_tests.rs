//! `buf.write` argument handling (#11291). Only the non-throwing shapes are
//! covered here; the error paths throw through `js_throw` and are covered
//! end to end by `test_gap_11291_buffer_write_args.ts`.
use super::buffer_write_args;
use crate::value::TAG_UNDEFINED;

fn undef() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn string(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

fn tag(name: &str) -> i32 {
    crate::buffer::js_encoding_tag_from_value(string(name))
}

#[test]
fn omitted_and_undefined_offset_write_utf8_over_the_whole_buffer() {
    assert_eq!(buffer_write_args(8, &[]), (0, 8, 0));
    assert_eq!(buffer_write_args(8, &[undef()]), (0, 8, 0));
    // Node ignores everything after an undefined offset, even the encoding.
    assert_eq!(
        buffer_write_args(8, &[undef(), 3.0, string("hex")]),
        (0, 8, 0)
    );
}

#[test]
fn undefined_length_means_the_remaining_bytes() {
    assert_eq!(buffer_write_args(8, &[2.0]), (2, 6, 0));
    assert_eq!(buffer_write_args(8, &[2.0, undef()]), (2, 6, 0));
    assert_eq!(
        buffer_write_args(8, &[2.0, undef(), string("latin1")]),
        (2, 6, tag("latin1"))
    );
    // bson's encodeUTF8Into shape.
    assert_eq!(
        buffer_write_args(64, &[4.0, undef(), string("utf8")]),
        (4, 60, tag("utf8"))
    );
}

#[test]
fn a_string_in_the_offset_or_length_slot_is_the_encoding() {
    assert_eq!(buffer_write_args(8, &[string("hex")]), (0, 8, tag("hex")));
    assert_eq!(
        buffer_write_args(8, &[string("hex"), undef()]),
        (0, 8, tag("hex"))
    );
    assert_eq!(
        buffer_write_args(8, &[1.0, string("hex")]),
        (1, 7, tag("hex"))
    );
}

#[test]
fn numeric_length_is_clamped_to_the_remaining_bytes() {
    assert_eq!(buffer_write_args(8, &[2.0, 3.0]), (2, 3, 0));
    assert_eq!(buffer_write_args(8, &[6.0, 5.0]), (6, 2, 0));
    assert_eq!(buffer_write_args(8, &[8.0, 0.0]), (8, 0, 0));
}

#[test]
fn falsy_encodings_mean_utf8() {
    let null = f64::from_bits(crate::value::TAG_NULL);
    assert_eq!(buffer_write_args(8, &[0.0, 2.0, null]), (0, 2, 0));
    assert_eq!(buffer_write_args(8, &[0.0, 2.0, string("")]), (0, 2, 0));
    assert_eq!(buffer_write_args(8, &[0.0, 2.0, undef()]), (0, 2, 0));
}
