//! #8117/#6887: an SSO string receiver reaching the polymorphic index funnel.
//!
//! `js_object_get_index_polymorphic` opens by asking "does this receiver's low
//! 48 bits hold a heap pointer?" and answering `undefined` when they do not.
//! That conflates *not a pointer* with *not indexable*. An inline
//! `SHORT_STRING_TAG` (SSO) string — every ASCII concatenation of five bytes or
//! fewer, so `"ab" + "c"` — carries its characters in the payload rather than an
//! address, so it fell into the reject arm and `s[0]` read `undefined` while the
//! byte-identical heap string read `"a"`.
//!
//! # Why a direct call witnesses this bug
//!
//! The sibling file `polymorphic_index_symbol_tests.rs` opens with a warning
//! that its obvious end-to-end shape passes without the fix, because a direct
//! call reaches a different sub-arm than the compiled path. That hazard does
//! not apply here, and the reason is worth stating rather than assuming:
//! codegen's generic index dispatcher hands this helper the receiver's **raw
//! NaN-boxed bits** whenever the tag is not `POINTER_TAG` — the emitted IR is
//!
//! ```text
//!   %r7 = icmp eq i64 %tag, 32765            ; POINTER_TAG?
//!   %r9 = select i1 %r7, i64 %masked, i64 %raw_bits
//!   %rN = call double @js_object_get_index_polymorphic(i64 %r9, double 0.0)
//! ```
//!
//! so for an SSO receiver the argument these tests construct by hand is
//! bit-for-bit the argument the compiled program passes. Verified by sabotage:
//! with the `0x7FF9` arm removed, `an_sso_string_receiver_reads_its_characters`
//! fails `left: None  right: Some("a")`, which is the same wrong answer
//! `test_gap_sso_concat_string_index.ts` reports against node.

use crate::builtins::jsvalue_string_content;
use crate::value::JSValue;

/// Boxed bits exactly as codegen's dispatcher passes them for a non-pointer
/// receiver: the whole NaN-boxed value reinterpreted as `i64`, unmasked.
fn handle_of(value: JSValue) -> i64 {
    value.bits() as i64
}

/// THE regression. An SSO receiver must yield its characters, not `undefined`.
#[test]
fn an_sso_string_receiver_reads_its_characters() {
    let _serialized = crate::array::test_serialize();
    let sso = JSValue::try_short_string(b"abc").expect("3 ASCII bytes fit SSO");
    assert!(
        sso.is_short_string(),
        "precondition: the receiver under test must really be an inline SSO \
         value — if concat ever stops producing one, this test is measuring \
         the wrong thing and must be updated, not deleted"
    );

    for (idx, expected) in [(0.0, "a"), (1.0, "b"), (2.0, "c")] {
        let got = jsvalue_string_content(crate::object::js_object_get_index_polymorphic(
            handle_of(sso),
            idx,
        ));
        assert_eq!(
            got.as_deref(),
            Some(expected),
            "sso[{idx}] must read the inline character; pre-fix the tag match \
             rejected SHORT_STRING_TAG as 'not a heap pointer' and returned \
             undefined"
        );
    }
}

/// Control for the 0x7FFF arm: the fix must not disturb a heap string receiver,
/// which reaches `js_string_index_get` via the `GC_TYPE_STRING` dispatch below
/// the tag match. A concatenation longer than the SSO threshold is heap-backed,
/// which is why the gap test's 80-character case never regressed.
#[test]
fn a_heap_string_receiver_still_reads_its_characters() {
    let _serialized = crate::array::test_serialize();
    let bytes = b"abcdefghij";
    let hdr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let heap = JSValue::string_ptr(hdr);
    assert!(
        !heap.is_short_string() && heap.is_string(),
        "precondition: this control must exercise the heap arm, not the new one"
    );

    let got = jsvalue_string_content(crate::object::js_object_get_index_polymorphic(
        handle_of(heap),
        0.0,
    ));
    assert_eq!(got.as_deref(), Some("a"));
}

/// Control against over-reach in the other direction: the new arm is keyed on
/// `SHORT_STRING_TAG` alone. Every other non-pointer tag in the reject arm is
/// a genuine primitive whose indexed read is `undefined` per JS, and widening
/// the arm to "any non-pointer tag" would break that.
#[test]
fn a_non_string_primitive_receiver_still_reads_undefined() {
    let _serialized = crate::array::test_serialize();
    let undefined_bits = crate::value::TAG_UNDEFINED;

    for handle in [
        undefined_bits as i64,
        crate::value::TAG_NULL as i64,
        crate::value::TAG_TRUE as i64,
    ] {
        let got = crate::object::js_object_get_index_polymorphic(handle, 0.0);
        assert_eq!(
            got.to_bits(),
            undefined_bits,
            "indexing a non-string primitive is `undefined` in JS; the SSO arm \
             must not have widened the accepted tag set"
        );
    }
}

/// An out-of-range index on an SSO receiver is `undefined`, same as on a heap
/// string — the delegation must carry the CanonicalNumericIndexString
/// semantics, not merely return *some* character.
#[test]
fn an_out_of_range_index_on_an_sso_receiver_reads_undefined() {
    let _serialized = crate::array::test_serialize();
    let sso = JSValue::try_short_string(b"abc").expect("3 ASCII bytes fit SSO");

    for idx in [3.0, 99.0, -1.0, 1.5, f64::NAN] {
        let got = crate::object::js_object_get_index_polymorphic(handle_of(sso), idx);
        assert_eq!(
            got.to_bits(),
            crate::value::TAG_UNDEFINED,
            "sso[{idx}] is not a canonical in-range array index"
        );
    }
}
