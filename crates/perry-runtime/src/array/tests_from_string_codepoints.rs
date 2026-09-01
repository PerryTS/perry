//! #9431: `Array.from(str)` on a WTF-8 payload holding a lone surrogate.
//!
//! The walk used to validate the payload with `std::str::from_utf8` and return
//! an EMPTY array on `Err`, so any lone surrogate lost the whole string. These
//! pin the three properties the fixture asserts at the JS level: the element
//! COUNT is the code-point count, each element carries the source bytes
//! verbatim, and a part carved out of a WTF-8 source keeps
//! `STRING_FLAG_HAS_LONE_SURROGATES` so `isWellFormed()` still reports false.

use super::*;
use crate::string::{
    js_string_from_bytes, js_string_from_wtf8_bytes, string_data, StringHeader,
    STRING_FLAG_HAS_LONE_SURROGATES,
};

/// The `index`-th element of the result, as (bytes, flags).
unsafe fn element(arr: *mut ArrayHeader, index: usize) -> (Vec<u8>, u32) {
    let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    let value = *elements.add(index);
    let part = crate::value::js_nanbox_get_pointer(value) as *const StringHeader;
    assert!(!part.is_null(), "element {index} is not a string");
    let bytes = std::slice::from_raw_parts(string_data(part), (*part).byte_len as usize).to_vec();
    (bytes, (*part).flags)
}

#[test]
fn lone_surrogate_keeps_every_code_point() {
    // "a" + U+D83D encoded as WTF-8 (ED A0 BD) + "b".
    let src = [b'a', 0xED, 0xA0, 0xBD, b'b'];
    let s = js_string_from_wtf8_bytes(src.as_ptr(), src.len() as u32);
    let arr = unsafe { js_array_from_string_codepoints(s) };
    assert_eq!(
        unsafe { (*arr).length },
        3,
        "lone surrogate lost the string"
    );
    unsafe {
        assert_eq!(element(arr, 0).0, b"a");
        assert_eq!(element(arr, 1).0, [0xED, 0xA0, 0xBD]);
        assert_eq!(element(arr, 2).0, b"b");
        // The carved-out half must stay marked as a broken half.
        assert_eq!(
            element(arr, 1).1 & STRING_FLAG_HAS_LONE_SURROGATES,
            STRING_FLAG_HAS_LONE_SURROGATES
        );
        // ASCII neighbours are ordinary well-formed strings.
        assert_eq!(element(arr, 0).1 & STRING_FLAG_HAS_LONE_SURROGATES, 0);
    }
}

#[test]
fn an_astral_pair_is_still_one_element() {
    // U+1F600 — two UTF-16 code units, ONE code point.
    let src = [b'a', 0xF0, 0x9F, 0x98, 0x80, b'b'];
    let s = js_string_from_bytes(src.as_ptr(), src.len() as u32);
    let arr = unsafe { js_array_from_string_codepoints(s) };
    assert_eq!(unsafe { (*arr).length }, 3);
    unsafe {
        assert_eq!(element(arr, 1).0, [0xF0, 0x9F, 0x98, 0x80]);
    }
}

#[test]
fn ascii_and_empty_are_unchanged() {
    let s = js_string_from_bytes(b"abc".as_ptr(), 3);
    let arr = unsafe { js_array_from_string_codepoints(s) };
    assert_eq!(unsafe { (*arr).length }, 3);
    unsafe {
        assert_eq!(element(arr, 2).0, b"c");
    }
    let empty = js_string_from_bytes(b"".as_ptr(), 0);
    let arr = unsafe { js_array_from_string_codepoints(empty) };
    assert_eq!(unsafe { (*arr).length }, 0);
}

#[test]
fn a_truncated_multibyte_tail_still_yields_a_part() {
    // #6085: an exact-sized payload ending in a truncated 4-byte lead. The
    // bounded decoder must not read past the allocation, and the byte must
    // not vanish from the result.
    let src = [b'a', 0xF0];
    let s = js_string_from_bytes(src.as_ptr(), src.len() as u32);
    let arr = unsafe { js_array_from_string_codepoints(s) };
    assert_eq!(unsafe { (*arr).length }, 2);
    unsafe {
        assert_eq!(element(arr, 1).0, [0xF0]);
    }
}
