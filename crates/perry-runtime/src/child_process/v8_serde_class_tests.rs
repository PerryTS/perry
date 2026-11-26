//! The class `Serializer` / `Deserializer` state behind `transferArrayBuffer`
//! and `getWireFormatVersion` (#15227).

use super::*;

fn array_buffer(bytes: &[u8]) -> f64 {
    crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::ArrayBuffer, bytes)
}

#[test]
fn a_transferred_array_buffer_is_written_as_its_id_and_read_as_the_given_buffer() {
    let _gc = GcSuppressGuard::new();
    let source = array_buffer(&[1, 2, 3]);
    let ser = v8_class_serializer_new();
    v8_class_serializer_write_header(ser);
    v8_class_serializer_transfer_array_buffer(ser, 7, source);
    v8_class_serializer_write_value(ser, source);
    let bytes = v8_class_serializer_release(ser);
    // Header, then kArrayBufferTransfer and the id: none of the source bytes.
    assert_eq!(bytes, [TAG_VERSION, 15, TAG_ARRAY_BUFFER_TRANSFER, 7]);

    let target = array_buffer(&[9, 8, 7]);
    let de = v8_class_deserializer_new(bytes);
    assert_eq!(v8_class_deserializer_wire_format_version(de), 0);
    v8_class_deserializer_read_header(de);
    assert_eq!(v8_class_deserializer_wire_format_version(de), 15);
    v8_class_deserializer_transfer_array_buffer(de, 7, target);
    assert_eq!(
        v8_class_deserializer_read_value(de).to_bits(),
        target.to_bits()
    );
}

#[test]
fn reads_past_the_end_report_failure() {
    let de = v8_class_deserializer_new(Vec::new());
    assert_eq!(v8_class_deserializer_read_uint32(de), None);
    assert_eq!(v8_class_deserializer_read_uint64(de), None);
    assert_eq!(v8_class_deserializer_read_double(de), None);
    assert_eq!(v8_class_deserializer_read_raw_bytes(de, 1), None);
    assert_eq!(
        v8_class_deserializer_read_raw_bytes(de, 0),
        Some(Vec::new())
    );
}
