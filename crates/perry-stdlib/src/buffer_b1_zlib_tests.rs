//! Provider-independent B1 witness, retained when the bundled provider is deleted.
#[test]
fn stream_chunk_conversion_and_sync_roundtrip() {
    perry_runtime::gc::gc_init();
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    for length in [17, 4096] {
        let input: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
        let chunk = scope.root_nanbox_f64(perry_runtime::buffer::bytes::from_slice(
            perry_runtime::buffer::bytes::Brand::Buffer,
            &input,
        ));
        unsafe {
            crate::buffer_b1_test_support::sabotage_output("zlib", chunk.get_nanbox_f64());
        }
        assert_eq!(
            unsafe { crate::buffer_b1_test_support::read(chunk.get_nanbox_f64()) },
            input
        );
        let compressed = scope.root_nanbox_f64(unsafe {
            perry_ext_zlib::js_zlib_gzip_sync(
                chunk.get_nanbox_u64() as i64,
                f64::from_bits(perry_runtime::JSValue::undefined().bits()),
            )
        });
        let output =
            unsafe { perry_ext_zlib::js_zlib_gunzip_sync(compressed.get_nanbox_u64() as i64) };
        assert_eq!(
            unsafe { crate::buffer_b1_test_support::read(output) },
            input
        );
    }
}
