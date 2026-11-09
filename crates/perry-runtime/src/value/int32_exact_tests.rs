//! The one int32-Number predicate (`tags::is_int32_bits`) and the shared
//! consumers built on it: a `0x7FFE` word with any of bits 32..47 set is not
//! an int32 Number.

/// A `C.prototype` ref word (`CLASS_PROTOTYPE_REF_FLAG`, bit 32) for class
/// 5, and a `0x7FFE` word with high payload bits: neither is an int32
/// Number, and a top-16-only test reads both as one.
const PROTO_REF_5: u64 = 0x7FFE_0001_0000_0005;
const HIGH_BITS: u64 = 0x7FFE_00FF_0000_0001;

const INT32_5: u64 = 0x7FFE_0000_0000_0005;

#[test]
fn only_a_clear_high_payload_is_an_int32_number() {
    assert!(super::is_int32_bits(INT32_5));
    assert!(super::JSValue::from_bits(INT32_5).is_int32());
    for bits in [PROTO_REF_5, HIGH_BITS] {
        assert!(!super::is_int32_bits(bits));
        assert!(!super::JSValue::from_bits(bits).is_int32());
    }
}

#[test]
fn a_prototype_ref_word_is_truthy_and_no_integer_key() {
    // Class id 0 in the payload: a top-16-only test reads int32 0 (falsy).
    let proto_ref_0 = 0x7FFE_0001_0000_0000u64;
    assert_eq!(super::js_is_truthy(f64::from_bits(proto_ref_0)), 1);
    assert_eq!(
        super::js_is_truthy(f64::from_bits(0x7FFE_0000_0000_0000)),
        0
    );
    assert_eq!(
        crate::object::field_get_set::runtime_read_site::integer_key(f64::from_bits(PROTO_REF_5)),
        None
    );
    assert_eq!(
        crate::object::field_get_set::runtime_read_site::integer_key(f64::from_bits(INT32_5)),
        Some(5)
    );
}
