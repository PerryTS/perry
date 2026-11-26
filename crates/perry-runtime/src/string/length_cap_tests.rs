//! Every string allocation path refuses a result longer than
//! `MAX_STRING_LENGTH` UTF-16 units with node's `RangeError: Invalid string
//! length`, and accepts one of exactly that length.
//!
//! The operands are short strings whose header claims a length near the
//! limit: every concat/append path sums the operands' header lengths, so the
//! check is exercised without allocating half a gigabyte.

use super::{
    js_string_append, js_string_append_chain, js_string_concat, js_string_concat_chain,
    js_string_from_bytes, js_string_from_bytes_with_capacity, StringHeader, MAX_STRING_LENGTH,
};
use crate::error::{ErrorHeader, ERROR_KIND_RANGE_ERROR};

/// A heap string of `text` whose header reports `units` UTF-16 units.
fn claiming(text: &str, units: usize) -> *mut StringHeader {
    let s = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    unsafe { (*s).utf16_len = units as u32 };
    s
}

/// A uniquely owned accumulator (refcount 1) with room to append in place.
fn accumulator(text: &str, units: usize) -> *mut StringHeader {
    let s = js_string_from_bytes_with_capacity(text.as_ptr(), text.len() as u32, 64);
    unsafe {
        (*s).utf16_len = units as u32;
        (*s).refcount = 1;
    }
    s
}

fn boxed(s: *mut StringHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
}

fn assert_invalid_string_length<R>(f: impl FnOnce() -> R) {
    let caught = crate::exception::catch_thrown(f)
        .err()
        .expect("an over-long string throws");
    let addr = crate::value::js_nanbox_get_pointer(caught.value()) as usize;
    unsafe {
        assert!(
            crate::error::ptr_is_native_error(addr),
            "not an ErrorHeader"
        );
        let err = addr as *const ErrorHeader;
        assert_eq!((*err).error_kind, ERROR_KIND_RANGE_ERROR);
        let message = (*err).message;
        let bytes =
            std::slice::from_raw_parts(super::string_data(message), (*message).byte_len as usize);
        assert_eq!(bytes, b"Invalid string length");
    }
    assert_eq!(crate::exception::js_has_exception(), 0);
}

fn units(s: *const StringHeader) -> usize {
    unsafe { (*s).utf16_len as usize }
}

#[test]
fn concat_reaches_the_limit_and_throws_past_it() {
    let a = claiming("ab", MAX_STRING_LENGTH - 1);
    let b = js_string_from_bytes(b"c".as_ptr(), 1);
    assert_eq!(units(js_string_concat(a, b)), MAX_STRING_LENGTH);
    let bc = js_string_from_bytes(b"cd".as_ptr(), 2);
    assert_invalid_string_length(|| js_string_concat(a, bc));
}

#[test]
fn concat_box_throws_past_the_limit() {
    let a = claiming("ab", MAX_STRING_LENGTH);
    let b = js_string_from_bytes(b"c".as_ptr(), 1);
    assert_invalid_string_length(|| super::js_string_concat_box(boxed(a), boxed(b)));
}

#[test]
fn concat_chain_throws_past_the_limit() {
    let a = claiming("ab", MAX_STRING_LENGTH - 2);
    let b = js_string_from_bytes(b"c".as_ptr(), 1);
    let ok = [boxed(a), boxed(b), boxed(b)];
    assert_eq!(
        units(js_string_concat_chain(ok.as_ptr(), 3)),
        MAX_STRING_LENGTH
    );
    let over = [boxed(a), boxed(b), boxed(b), boxed(b)];
    assert_invalid_string_length(|| js_string_concat_chain(over.as_ptr(), 4));
    let number = [boxed(a), 123.0];
    assert_invalid_string_length(|| js_string_concat_chain(number.as_ptr(), 2));
}

#[test]
fn append_in_place_throws_past_the_limit() {
    let dest = accumulator("ab", MAX_STRING_LENGTH - 1);
    let src = js_string_from_bytes(b"c".as_ptr(), 1);
    let grown = js_string_append(dest, src);
    assert_eq!(grown, dest, "in-place arm");
    assert_eq!(units(grown), MAX_STRING_LENGTH);
    assert_invalid_string_length(|| js_string_append(grown, src));
    assert_eq!(
        units(grown),
        MAX_STRING_LENGTH,
        "the refused append left dest as it was"
    );
}

#[test]
fn append_growth_throws_past_the_limit() {
    // Shared (refcount 0) dest: the growth arm, which writes the new header itself.
    let dest = claiming("ab", MAX_STRING_LENGTH);
    let src = js_string_from_bytes(b"c".as_ptr(), 1);
    assert_invalid_string_length(|| js_string_append(dest, src));
}

#[test]
fn append_chain_in_place_throws_past_the_limit() {
    let dest = accumulator("ab", MAX_STRING_LENGTH - 2);
    let b = js_string_from_bytes(b"c".as_ptr(), 1);
    let ok = [boxed(dest), boxed(b), boxed(b)];
    let grown = js_string_append_chain(ok.as_ptr(), 3);
    assert_eq!(grown, dest, "in-place arm");
    assert_eq!(units(grown), MAX_STRING_LENGTH);
    let over = [boxed(grown), boxed(b)];
    assert_invalid_string_length(|| js_string_append_chain(over.as_ptr(), 2));
}

#[test]
fn every_birth_checks_the_length() {
    super::check_string_length(MAX_STRING_LENGTH);
    assert_invalid_string_length(|| super::check_string_length(MAX_STRING_LENGTH + 1));
    let (ptr, _) = super::string_storage_alloc(1);
    assert_invalid_string_length(|| unsafe {
        super::init_string_header(ptr, MAX_STRING_LENGTH as u32 + 1, 1, 1, 0, 0)
    });
}
