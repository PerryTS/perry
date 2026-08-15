//! #8100: an element **READ** helper handed a receiver that is not a
//! %TypedArray% must fall back to the ordinary `[[Get]]`, not read foreign
//! memory as a `TypedArrayHeader`.
//!
//! ## Why this file exists
//!
//! `perry-codegen`'s `is_width_tracked_typed_array_receiver` (#7494) keeps a
//! local's DECLARED typed-array kind even after the binding is reassigned —
//! deliberately, because dropping the hint sends a REAL typed array through
//! `is_array_expr`'s plain-array layout (element 0 at byte 8 instead of the
//! data region at byte 16). It pays for that with an explicit promise that the
//! runtime helper "re-validates the object's actual GC kind before touching
//! memory".
//!
//! `js_typed_array_get` did not. Its only receiver check was `clean_ta_ptr`,
//! which rejects nothing but an address below `0x1000`. So
//!
//! ```text
//! let P: Int32Array = new Int32Array(1);
//! P = [99, 101] as any;
//! P[0]                                   // perry 0, node 99
//! ```
//!
//! read an `ArrayHeader` as a `TypedArrayHeader`: `length` matched (offset 0
//! in both), `kind`/`elem_size` came from the low bytes of element 0's NaN
//! box, and the data pointer sat 8 bytes past the real element region. Every
//! read answered `0`, with no error and no diagnostic, in the shipped default
//! configuration.
//!
//! ## What each test asserts, and how it can fail
//!
//! * `clean_ta_ptr_does_not_validate_a_plain_array_receiver` pins the
//!   *precondition*. If `clean_ta_ptr` ever starts rejecting non-typed-array
//!   receivers on its own, the classifier below is redundant — that test
//!   failing is a signal to re-read the guard, not to delete the test.
//! * the plain-array read tests assert the ELEMENT VALUES (`99`, `101`), not
//!   merely "did not panic". The pre-#8100 code returns `0.0` for both, so a
//!   regression cannot pass them (CLAUDE.md's fourth way a gate cannot fail).
//! * the typed-array controls store a value that only survives per-kind
//!   truncation (`70000 & 0xFFFF == 4464`), so a fallback that hijacked the
//!   typed path — reading boxed f64 slots instead of 16-bit lanes — fails
//!   them.
//! * `classify_element_read_receiver_retags_a_heap_string` covers the one
//!   branch the end-to-end tests cannot distinguish cheaply: codegen masks the
//!   NaN-box tag off the receiver, so a heap string must be RE-tagged
//!   `STRING_TAG` or `js_dyn_index_get` walks a `StringHeader` as an
//!   `ObjectHeader`.

use super::*;

use crate::array::{js_array_alloc, js_array_push_f64, ArrayHeader};
use crate::value::{POINTER_MASK, SHORT_STRING_TAG, STRING_TAG, TAG_UNDEFINED};

const UINT16: u8 = KIND_UINT16;

fn plain_array(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(values.len() as u32);
    for v in values {
        arr = js_array_push_f64(arr, *v);
    }
    arr
}

/// A plain array handed to a typed-array element helper, exactly as codegen
/// emits it: the NaN-box tag is masked off (`and i64 %bits, POINTER_MASK`), so
/// the helper receives a bare address.
fn as_typed(arr: *mut ArrayHeader) -> *const TypedArrayHeader {
    ((arr as u64) & POINTER_MASK) as *const TypedArrayHeader
}

fn typed(kind: u8, values: &[f64]) -> *mut TypedArrayHeader {
    let ta = typed_array_alloc(kind, values.len() as u32);
    for (i, v) in values.iter().enumerate() {
        js_typed_array_set(ta, i as i32, *v);
    }
    ta
}

fn is_undefined(v: f64) -> bool {
    v.to_bits() == TAG_UNDEFINED
}

fn top16(v: f64) -> u64 {
    v.to_bits() >> 48
}

#[test]
fn clean_ta_ptr_does_not_validate_a_plain_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let arr = plain_array(&[99.0, 101.0]);

    // The registry knows this is NOT a typed array...
    assert!(
        lookup_typed_array_kind(arr as usize).is_none(),
        "a plain array must not be in the typed-array registry, or the \
         classifier below is being asked the wrong question"
    );
    // ...and yet the receiver funnel every element helper used to rely on
    // waves it straight through. This is WHY the classifier has to run first:
    // there is no later point at which the wrong receiver is caught.
    assert!(
        !clean_ta_ptr(as_typed(arr)).is_null(),
        "clean_ta_ptr only rejects addresses below 0x1000 — if it starts \
         validating the receiver, re-read the guard before touching #8100"
    );
}

#[test]
fn js_typed_array_get_reads_a_plain_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let arr = plain_array(&[99.0, 101.0]);

    // Pre-#8100 both of these were 0.0: `(*ta).length` read the ArrayHeader's
    // length (2, so the bounds check passed), `(*ta).kind` read the low byte
    // of element 0's NaN box (0 == KIND_INT8) and `elem_size` the next (0).
    assert_eq!(js_typed_array_get(as_typed(arr), 0), 99.0);
    assert_eq!(js_typed_array_get(as_typed(arr), 1), 101.0);
}

#[test]
fn js_typed_array_get_is_undefined_past_a_plain_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let arr = plain_array(&[99.0, 101.0]);

    assert!(is_undefined(js_typed_array_get(as_typed(arr), 2)));
    assert!(is_undefined(js_typed_array_get(as_typed(arr), -1)));
}

#[test]
fn js_typed_array_get_is_undefined_for_a_non_pointer_receiver() {
    // `let P: Int32Array = new Int32Array(1); P = 42 as any; P[0]` — codegen
    // masks POINTER_MASK over the f64 bits of `42`, which leaves 0. The
    // answer is `undefined` (what node prints), not the `0.0` this used to
    // return.
    assert!(is_undefined(js_typed_array_get(std::ptr::null(), 0)));
}

#[test]
fn js_typed_array_get_still_reads_a_real_typed_array_element_typed() {
    let ta = typed(UINT16, &[1.0, 2.0]);
    js_typed_array_set(ta, 0, 70000.0);
    // 70000 truncated to 16 bits == 4464: proof the per-kind lane load ran and
    // the new fallback did not hijack the typed path into boxed-f64 slots.
    assert_eq!(js_typed_array_get(ta, 0), 4464.0);
    assert_eq!(js_typed_array_get(ta, 1), 2.0);
    assert!(is_undefined(js_typed_array_get(ta, 2)));
}

#[test]
fn js_typed_array_index_get_dynamic_reads_a_plain_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let arr = plain_array(&[99.0, 101.0]);
    let recv = as_typed(arr);

    // Numeric key — pre-#8100 this answered `undefined` for every index.
    assert_eq!(js_typed_array_index_get_dynamic(recv, 0.0), 99.0);
    assert_eq!(js_typed_array_index_get_dynamic(recv, 1.0), 101.0);
    assert!(is_undefined(js_typed_array_index_get_dynamic(recv, 2.0)));

    // Canonical numeric-index STRING key is an element read, not a named
    // property (IntegerIndexedExotic `[[Get]]`).
    let key = crate::string::js_string_from_str("1");
    let key_value = crate::value::js_nanbox_string(key as i64);
    assert_eq!(js_typed_array_index_get_dynamic(recv, key_value), 101.0);
}

#[test]
fn js_typed_array_index_get_dynamic_still_reads_a_real_typed_array() {
    let ta = typed(UINT16, &[1.0, 2.0]);
    js_typed_array_set(ta, 1, 70000.0);
    assert_eq!(js_typed_array_index_get_dynamic(ta, 1.0), 4464.0);
    assert!(is_undefined(js_typed_array_index_get_dynamic(ta, 9.0)));
}

#[test]
fn classify_element_read_receiver_keeps_a_registered_typed_array() {
    let ta = typed(UINT16, &[1.0]);
    match classify_element_read_receiver(ta as u64) {
        ElementReadReceiver::TypedArray(addr) => assert_eq!(addr, ta as usize),
        _ => panic!("a registered typed array must stay on the typed path"),
    }
}

#[test]
fn classify_element_read_receiver_retags_a_heap_string() {
    // Codegen masked the tag off, so the classifier has to reconstruct it from
    // the managed header. Boxing a StringHeader as POINTER_TAG would send
    // `js_dyn_index_get` down its ObjectHeader walk instead of its string arm.
    let s = crate::string::js_string_from_str("perry element read");
    match classify_element_read_receiver((s as u64) & POINTER_MASK) {
        ElementReadReceiver::Ordinary(value) => assert_eq!(
            top16(value),
            STRING_TAG >> 48,
            "a heap string receiver must come back STRING_TAG'd, not \
             POINTER_TAG'd"
        ),
        _ => panic!("a heap string is an ordinary indexable receiver"),
    }

    // End-to-end: the read answers a one-character string, not `0.0`.
    let ch = js_typed_array_get(((s as u64) & POINTER_MASK) as *const TypedArrayHeader, 0);
    assert!(
        top16(ch) == STRING_TAG >> 48 || top16(ch) == SHORT_STRING_TAG >> 48,
        "`s[0]` on a string receiver must be a string, got bits {:#x}",
        ch.to_bits()
    );
}

#[test]
fn classify_element_read_receiver_rejects_garbage_bits() {
    // Neither a plausible heap address nor a registered anything: the answer
    // is `undefined`, and nothing is dereferenced on the way there.
    assert!(matches!(
        classify_element_read_receiver(0),
        ElementReadReceiver::Absent
    ));
    assert!(matches!(
        classify_element_read_receiver(0x3F),
        ElementReadReceiver::Absent
    ));
}

// --------------------------------------------------------------------------
// #8111: the `Uint8Array`-specialized twin of the same defect.
//
// `js_uint8array_get` / `js_uint8array_index_get_value` / `js_uint8array_set`
// are a SEPARATE emission path: codegen picks them from
// `is_uint8array_receiver` (`perry-codegen/src/expr/index_{get,set}.rs`),
// which keys on `receiver_class_name` rather than the `local_type_hint`
// predicate #8100 is about — but it fires for a reassigned `Uint8Array` local
// just the same. Each helper had a three-way shape (registered typed array of
// the right kind / registered buffer / fall off the end) and TWO of those
// arms answered for a receiver that is perfectly readable:
//
//   * the trailing arm — a plain array or object — answered `0` /
//     `undefined` / dropped the store;
//   * the wrong-KIND arm — a registered typed array that is not
//     `Uint8Array` / `Uint8ClampedArray` — did the same, although
//     `js_typed_array_get` / `js_typed_array_set` are kind-generic and node
//     reads and writes the real element there.
//
// The store half matters most: `Q[0] = 5` left no trace at all.
//
// Each test asserts the RECOVERED VALUE, never merely "did not panic". The
// pre-fix code answers `0` / `undefined` / no-op for every one of them, so
// none can pass against the old body.
// --------------------------------------------------------------------------

use crate::typedarray::access::{
    js_uint8array_get, js_uint8array_index_get_value, js_uint8array_set,
};

/// A plain array handed to a `Uint8Array`-specialized helper, exactly as
/// codegen emits it (`unbox_to_i64` masks the NaN-box tag off).
fn as_u8(arr: *mut ArrayHeader) -> *const TypedArrayHeader {
    as_typed(arr)
}

/// A registered typed array with the NaN-box tag masked off, the shape every
/// one of these helpers is actually handed.
fn as_recv(ta: *mut TypedArrayHeader) -> *const TypedArrayHeader {
    ((ta as u64) & POINTER_MASK) as *const TypedArrayHeader
}

#[test]
fn js_uint8array_index_get_value_reads_a_plain_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let arr = plain_array(&[9.0, 10.0]);
    assert_eq!(js_uint8array_index_get_value(as_u8(arr), 0), 9.0);
    assert_eq!(js_uint8array_index_get_value(as_u8(arr), 1), 10.0);
    // Out of range is `undefined`, the IntegerIndexedExotic answer node
    // prints — not the `0` byte sentinel.
    assert!(is_undefined(js_uint8array_index_get_value(as_u8(arr), 2)));
}

#[test]
fn js_uint8array_set_stores_into_a_plain_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let arr = plain_array(&[9.0, 10.0]);
    js_uint8array_set(as_u8(arr) as *mut TypedArrayHeader, 0, 5);
    assert_eq!(
        js_uint8array_index_get_value(as_u8(arr), 0),
        5.0,
        "the store must land in the plain array — it was silently dropped"
    );
    // And it is visible through the ordinary array accessor too, i.e. it is a
    // real `[[Set]]` and not a shadow write somewhere else.
    assert_eq!(crate::array::js_array_get_element(arr as i64, 0), 5.0);
    assert_eq!(crate::array::js_array_get_element(arr as i64, 1), 10.0);
}

#[test]
fn js_uint8array_get_reads_a_plain_array_receiver_as_a_byte() {
    let _serialized = crate::array::test_serialize();
    let arr = plain_array(&[9.0, 10.0]);
    assert_eq!(js_uint8array_get(as_u8(arr), 0), 9);
    assert_eq!(js_uint8array_get(as_u8(arr), 1), 10);
    // This accessor's ABI is a byte-typed i32, so out of range stays the `0`
    // sentinel (#6088) rather than becoming `undefined`.
    assert_eq!(js_uint8array_get(as_u8(arr), 2), 0);
}

#[test]
fn uint8_helpers_read_and_write_a_wrong_kind_typed_array() {
    let _serialized = crate::array::test_serialize();
    // A real Int32Array behind a `Uint8Array` static hint. `js_typed_array_
    // {get,set}` are kind-generic, so there is nothing unsafe about serving
    // it — the old `!matches!(kind, UINT8 | UINT8_CLAMPED)` arm just answered
    // `undefined` and dropped the store. node reads and writes the element.
    let ta = typed(KIND_INT32, &[11.0, 12.0]);
    let recv = as_recv(ta);

    assert_eq!(js_uint8array_index_get_value(recv, 1), 12.0);
    js_uint8array_set(recv as *mut TypedArrayHeader, 0, 77);
    assert_eq!(js_uint8array_index_get_value(recv, 0), 77.0);
    // Through the kind-correct accessor as well: the lane really holds 77.
    assert_eq!(js_typed_array_get(ta, 0), 77.0);
}

#[test]
fn js_uint8array_index_get_value_is_undefined_for_a_non_pointer_receiver() {
    let _serialized = crate::array::test_serialize();
    // `Q = 42 as any` — codegen masks the tag off, so the helper sees a small
    // integer. Nothing indexable: `undefined`, and a store is dropped.
    let bogus = 42u64 as *const TypedArrayHeader;
    assert!(is_undefined(js_uint8array_index_get_value(bogus, 0)));
    assert_eq!(js_uint8array_get(bogus, 0), 0);
    js_uint8array_set(bogus as *mut TypedArrayHeader, 0, 5);
}

// --------------------------------------------------------------------------
// Controls: the receivers these helpers were WRITTEN for must be untouched.
// --------------------------------------------------------------------------

#[test]
fn uint8_helpers_still_serve_a_real_uint8_array() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(KIND_UINT8, &[3.0, 4.0]);
    let recv = as_recv(ta);

    assert_eq!(js_uint8array_index_get_value(recv, 0), 3.0);
    assert_eq!(js_uint8array_get(recv, 1), 4);
    js_uint8array_set(recv as *mut TypedArrayHeader, 1, 250);
    assert_eq!(js_uint8array_index_get_value(recv, 1), 250.0);
    // A Uint8Array lane is 1 byte: 300 wraps to 44. If the store had been
    // diverted to a plain-array `[[Set]]` it would read back 300.
    js_uint8array_set(recv as *mut TypedArrayHeader, 0, 300);
    assert_eq!(js_uint8array_index_get_value(recv, 0), 44.0);
    assert!(is_undefined(js_uint8array_index_get_value(recv, 2)));
}

#[test]
fn uint8_helpers_still_serve_a_uint8_clamped_array() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(KIND_UINT8_CLAMPED, &[1.0, 2.0]);
    let recv = as_recv(ta);
    // Clamped, not wrapped: 300 -> 255. The kind-specific store is still the
    // one running.
    js_uint8array_set(recv as *mut TypedArrayHeader, 0, 300);
    assert_eq!(js_uint8array_index_get_value(recv, 0), 255.0);
}
