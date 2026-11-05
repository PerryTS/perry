use super::*;

#[test]
fn method_observation_rejects_numbers_without_losing_internal_raw_abi() {
    for addr in [0x20_0000u64, 0x20_0001, 0x7fff_ffff_ffff] {
        assert!(f64::from_bits(addr).is_subnormal());
        assert_eq!(boxed_method_receiver_addr(addr), 0);
        assert_eq!(normalize_raw_object_addr(addr), addr as usize);
        for tag in [POINTER_TAG, STRING_TAG, BIGINT_TAG] {
            assert_eq!(boxed_method_receiver_addr(tag | addr), addr as usize);
        }
    }
    for value in [
        0.0f64,
        -0.0,
        0.25,
        -0.25,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert_eq!(boxed_method_receiver_addr(value.to_bits()), 0);
    }
    for bits in [
        TAG_NULL,
        TAG_UNDEFINED,
        SHORT_STRING_TAG | 0x55_6677_8899,
        INT32_TAG | 0x7fff_ffff,
    ] {
        assert_eq!(boxed_method_receiver_addr(bits), 0);
    }
}
