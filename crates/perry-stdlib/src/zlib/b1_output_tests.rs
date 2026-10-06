use super::*;

#[test]
fn stream_chunk_conversion_and_sync_roundtrip() {
    perry_runtime::gc::gc_init();
    for length in [17, 4096] {
        let input: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
        let chunk = unsafe { make_buffer(&input).unwrap() };
        assert_eq!(unsafe { crate::buffer_b1_test_support::read(chunk) }, input);
        let compressed = unsafe {
            js_zlib_gzip_sync(
                chunk.to_bits() as i64,
                f64::from_bits(perry_runtime::JSValue::undefined().bits()),
            )
        };
        let compressed_bits = perry_runtime::JSValue::pointer(compressed.cast()).bits();
        let decompressed = unsafe { js_zlib_gunzip_sync(compressed_bits as i64) };
        let value = f64::from_bits(perry_runtime::JSValue::pointer(decompressed.cast()).bits());
        assert_eq!(unsafe { crate::buffer_b1_test_support::read(value) }, input);
    }
}
