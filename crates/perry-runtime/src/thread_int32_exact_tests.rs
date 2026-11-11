//! Worker truthiness: a `C.prototype` ref word is an object (truthy), even
//! with class id 0 in its payload.

#[test]
fn a_prototype_ref_word_is_truthy() {
    assert!(super::is_truthy_bits(0x7FFE_0001_0000_0000));
    assert!(super::is_truthy_bits(0x7FFE_0001_0000_0005));
    assert!(!super::is_truthy_bits(0x7FFE_0000_0000_0000));
    assert!(super::is_truthy_bits(0x7FFE_0000_0000_0005));
}
