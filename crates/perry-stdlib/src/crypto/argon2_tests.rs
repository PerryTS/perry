use super::*;

unsafe fn derive(secret: Option<&[u8]>, data: Option<&[u8]>) -> String {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let object = js_object_alloc(0, 8);
    let root = scope.root_nanbox_f64(f64::from_bits(
        JSValue::object_ptr(object as *mut u8).bits(),
    ));
    let set = |name: &str, value: f64| {
        let value = scope.root_nanbox_f64(value);
        let name = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let object = JSValue::from_bits(root.get_nanbox_f64().to_bits())
            .as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        js_object_set_field_by_name(object, name, value.get_nanbox_f64());
    };
    for (name, bytes) in [
        ("message", b"password".as_slice()),
        ("nonce", b"0123456789abcdef".as_slice()),
    ] {
        set(
            name,
            f64::from_bits(JSValue::pointer(alloc_buffer_from_slice(bytes) as *const u8).bits()),
        );
    }
    for (name, number) in [
        ("parallelism", 1.0),
        ("tagLength", 16.0),
        ("memory", 8.0),
        ("passes", 1.0),
    ] {
        set(name, number);
    }
    for (name, bytes) in [("secret", secret), ("associatedData", data)] {
        if let Some(bytes) = bytes {
            set(
                name,
                f64::from_bits(
                    JSValue::pointer(alloc_buffer_from_slice(bytes) as *const u8).bits(),
                ),
            );
        }
    }
    let algorithm = js_string_from_bytes(b"argon2id".as_ptr(), 8);
    let key = js_crypto_argon2_sync(algorithm as i64, root.get_nanbox_f64());
    perry_hex::encode(bytes_from_ptr(key as i64))
}

#[test]
fn argon2_public_sync_includes_secret_and_associated_data() {
    unsafe {
        assert_eq!(derive(None, None), "83947cb26635e8b2500b348925b220dc");
        assert_eq!(
            derive(Some(b"pepper"), None),
            "3e72ec885ed9f4637910e1e9aaee403b"
        );
        assert_eq!(
            derive(None, Some(b"ad")),
            "3741472183da524860bdccc2d27028e1"
        );
        assert_eq!(
            derive(Some(b"pepper"), Some(b"ad")),
            "d7aab58ed8561dda350519e5d7d7f2a5"
        );
        assert_eq!(
            derive(Some(b""), Some(b"")),
            "83947cb26635e8b2500b348925b220dc"
        );
    }
}
