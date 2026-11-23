//! A renamed export carries its Node `name` on both mints: the namespace
//! property read and the ESM import binding.
use super::static_exports::{
    js_native_export_68747470_5f636f6e6e656374696f6e4c697374656e6572 as esm_http_connection_listener,
    js_native_export_6d6f64756c65_72756e4d61696e as esm_module_run_main,
    js_native_export_75726c_7265736f6c76654f626a656374 as esm_url_resolve_object,
};

fn name_of(value: f64) -> String {
    let c = crate::value::JSValue::from_bits(value.to_bits()).as_pointer::<u8>() as usize;
    let name = crate::closure::closure_get_dynamic_prop(c, "name");
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) =
        crate::string::str_bytes_from_jsvalue(name, &mut scratch).expect("string name");
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len as usize) }).into_owned()
}

#[test]
fn renamed_exports_carry_the_node_name_on_both_mints() {
    std::thread::spawn(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        assert_eq!(name_of(esm_url_resolve_object(0)), "urlResolveObject");
        assert_eq!(
            name_of(esm_http_connection_listener(0)),
            "connectionListener"
        );
        assert_eq!(name_of(esm_module_run_main(0)), "executeUserEntryPoint");
        for (module, property, name) in [
            ("url", "resolveObject", "urlResolveObject"),
            ("http", "_connectionListener", "connectionListener"),
            ("module", "runMain", "executeUserEntryPoint"),
        ] {
            assert_eq!(
                name_of(super::bound_native_callable_export_value(module, property)),
                name
            );
        }
    })
    .join()
    .unwrap();
}
