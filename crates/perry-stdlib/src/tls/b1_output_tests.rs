use super::*;

#[test]
fn ticket_keys_and_certificate_byte_conversion() {
    perry_runtime::gc::gc_init();
    let keys = unsafe { js_tls_server_get_ticket_keys(-1) };
    assert_eq!(
        unsafe { crate::buffer_b1_test_support::read(keys) },
        vec![0; 48]
    );
    for length in [17, 4096] {
        let der: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
        let value = unsafe { buffer_from_bytes(&der) };
        assert_eq!(unsafe { crate::buffer_b1_test_support::read(value) }, der);
    }
}
