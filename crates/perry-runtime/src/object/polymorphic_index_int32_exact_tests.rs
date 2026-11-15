//! A `0x7FFE` word (an internal word, never a JS value) is no numeric key.

const PROTO_REF_5: u64 = 0x7FFE_0001_0000_0005;
const HIGH_BITS: u64 = 0x7FFE_00FF_0000_0001;

#[test]
fn a_non_number_0x7ffe_word_is_no_numeric_key() {
    for bits in [PROTO_REF_5, HIGH_BITS] {
        assert_eq!(super::numeric_key_u32_index(f64::from_bits(bits)), None);
        assert_eq!(super::numeric_key_i32_index(f64::from_bits(bits)), None);
    }
    // Nor is an I32 slot word: no JS value is in the band (P3 B4b).
    assert_eq!(
        super::numeric_key_u32_index(f64::from_bits(0x7FFE_0000_0000_0005)),
        None
    );
}
