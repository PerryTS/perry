use super::*;
use perry_base64::Engine;

#[test]
fn certificate_raw_output_preserves_der() {
    crate::gc::gc_init();
    let encoded: String = roots::bundled_certificates()[0]
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let der = perry_base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let cert = unsafe { tls_legacy_certificate_object(&der, false) };
    let raw = get_field(
        JSValue::from_bits(cert.to_bits())
            .as_pointer::<ObjectHeader>()
            .cast_mut(),
        "raw",
    );
    let ptr = JSValue::from_bits(raw.to_bits()).as_pointer::<crate::buffer::BufferHeader>();
    let actual =
        crate::buffer::bytes::ReadLease::new(crate::value::js_nanbox_pointer(ptr as i64)).unwrap();
    assert_eq!(&actual[..], der);
}
