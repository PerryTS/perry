use super::*;
use perry_runtime::buffer::{
    self,
    bytes::{self, Brand},
};

fn value<T>(ptr: *const T) -> f64 {
    perry_runtime::value::js_nanbox_pointer(ptr as i64)
}

#[test]
fn byte_outputs_and_digest_inputs_use_the_visible_window() {
    // Both sides of the design's 256-byte test boundary. B3 supplies the
    // placement rule; these consumers must work independently of that rule.
    for len in [0, 1, 255, 256, 257, 1024 * 1024] {
        let input: Vec<u8> = (0..len).map(|n| (n % 251) as u8).collect();
        let ptr = unsafe { alloc_buffer_from_slice(&input) };
        bytes::no_gc(|scope| assert_eq!(bytes::bytes(value(ptr), scope).unwrap(), input));
    }
    let source = bytes::from_slice(Brand::Buffer, b"prefix-payload-suffix");
    let ptr = JSValue::from_bits(source.to_bits()).as_pointer::<buffer::BufferHeader>();
    let view = buffer::js_buffer_slice(ptr, 7, 14);
    let result = unsafe { js_crypto_sha256_bytes(view as i64) };
    let expected = Sha256::digest(b"payload");
    bytes::no_gc(|scope| {
        assert_eq!(
            bytes::bytes(value(result), scope).unwrap(),
            expected.as_slice()
        )
    });
}

#[test]
fn random_fill_preserves_bytes_outside_the_view_and_range() {
    let source = bytes::from_slice(Brand::Buffer, &[0x25; 32]);
    let ptr = JSValue::from_bits(source.to_bits()).as_pointer::<buffer::BufferHeader>();
    let view = buffer::js_buffer_slice(ptr, 8, 24);
    assert_eq!(
        js_crypto_random_fill_sync(value(view), 4.0, 8.0).to_bits(),
        value(view).to_bits()
    );
    bytes::no_gc(|scope| {
        let data = bytes::bytes(source, scope).unwrap();
        assert_eq!(&data[..12], &[0x25; 12]);
        assert_eq!(&data[20..], &[0x25; 12]);
    });
    for len in [0, 255, 257, 4096] {
        let ptr = js_crypto_random_bytes_buffer(len as f64);
        bytes::no_gc(|scope| assert_eq!(bytes::bytes(value(ptr), scope).unwrap().len(), len));
    }
}

#[test]
fn each_b2c_sabotage_turns_its_consumer_witness_red() {
    for (fault, witness) in [
        (
            "crypto_output",
            "byte_outputs_and_digest_inputs_use_the_visible_window",
        ),
        (
            "crypto_borrow",
            "byte_outputs_and_digest_inputs_use_the_visible_window",
        ),
        (
            "random_fill_range",
            "random_fill_preserves_bytes_outside_the_view_and_range",
        ),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("crypto::bytes_contract_tests::{witness}"),
                "--nocapture",
            ])
            .env("PERRY_B2C_SABOTAGE", fault)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "sabotage {fault} left {witness} green"
        );
        eprintln!("B2c sabotage {fault}: RED");
    }
}
