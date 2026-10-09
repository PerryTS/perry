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

#[test]
fn hash_update_borrows_the_visible_byte_span_and_ignores_string_encoding() {
    use super::hash_handles::with_hash_update_bytes;
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let enc = scope.root_nanbox_f64(unsafe { string_value(b"hex") });
    for brand in [Brand::Buffer, Brand::Uint8Array, Brand::DataView] {
        for len in [1, 255, 256, 257, 1024 * 1024] {
            let data = vec![b'a'; len];
            let source = bytes::from_slice(brand, &data);
            let visible = bytes::no_gc(|scope| bytes::bytes(source, scope).unwrap().as_ptr());
            unsafe {
                with_hash_update_bytes(&[source, enc.get_nanbox_f64()], |input| {
                    assert_eq!(
                        input.as_ptr(),
                        visible,
                        "Hash.update copied {brand:?}, {len} bytes"
                    );
                    assert_eq!(input, data);
                });
            }
        }
    }
    let source = bytes::from_slice(Brand::Buffer, b"prefix-payload-suffix");
    let ptr = JSValue::from_bits(source.to_bits()).as_pointer::<buffer::BufferHeader>();
    let view = buffer::js_buffer_slice(ptr, 7, 14);
    unsafe {
        with_hash_update_bytes(&[value(view), enc.get_nanbox_f64()], |input| {
            assert_eq!(input, b"payload")
        });
        with_hash_update_bytes(&[string_value(b"616263"), enc.get_nanbox_f64()], |input| {
            assert_eq!(input, b"abc")
        });
    }
}

#[test]
fn one_shot_hash_allocates_only_its_byte_result() {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let data = scope.root_nanbox_f64(bytes::from_slice(Brand::Buffer, b"payload"));
    let encoding = scope.root_nanbox_f64(unsafe { string_value(b"buffer") });
    for algorithm in [b"sha256".as_slice(), b"sha384", b"sha512"] {
        let alg = scope.root_nanbox_f64(unsafe { string_value(algorithm) });
        let args = || {
            [
                alg.get_nanbox_f64(),
                data.get_nanbox_f64(),
                encoding.get_nanbox_f64(),
            ]
        };
        let call = || unsafe {
            let values = args();
            js_crypto_native_dispatch(b"hash".as_ptr(), 4, values.as_ptr(), values.len())
        };
        // Warm lazy prototype/module initialization outside the contract.
        let warm = scope.root_nanbox_f64(call());
        let expected = bytes::no_gc(|s| bytes::bytes(warm.get_nanbox_f64(), s).unwrap().to_vec());
        let before = perry_runtime::arena::arena_live_allocated_bytes();
        let control = scope.root_nanbox_f64(bytes::from_slice(Brand::Buffer, &expected));
        let result_bytes = perry_runtime::arena::arena_live_allocated_bytes() - before;
        assert!(result_bytes > 0, "control must allocate the byte result");
        let before = perry_runtime::arena::arena_live_allocated_bytes();
        let result = scope.root_nanbox_f64(call());
        let allocated = perry_runtime::arena::arena_live_allocated_bytes() - before;
        assert_eq!(
            allocated, result_bytes,
            "crypto.hash allocated a temporary JS object"
        );
        bytes::no_gc(|s| {
            assert_eq!(
                bytes::bytes(result.get_nanbox_f64(), s).unwrap(),
                bytes::bytes(control.get_nanbox_f64(), s).unwrap()
            )
        });
    }
}
