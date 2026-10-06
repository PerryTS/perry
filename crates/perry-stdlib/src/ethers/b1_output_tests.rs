use super::*;

#[test]
fn empty_keccak_bytes_match_the_known_digest() {
    perry_runtime::gc::gc_init();
    let out = unsafe { js_keccak256_native_bytes(0) };
    let value = f64::from_bits(perry_runtime::JSValue::pointer(out.cast()).bits());
    let bytes = unsafe { crate::buffer_b1_test_support::read(value) };
    assert_eq!(
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
    );
}
