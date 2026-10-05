use super::*;

extern "C" fn readable_stream_from_static(
    _closure: *const ClosureHeader,
    _this: JsThis,
    iterable: f64,
) -> f64 {
    unsafe { js_readable_stream_from_iterable(iterable) }
}

pub(crate) unsafe fn install_readable_stream_from_static() {
    extern "C" {
        fn js_install_readable_stream_from(info: *const JsFunctionInfo);
    }
    js_install_readable_stream_from(
        perry_runtime::fn_info!(readable_stream_from_static, 1; with_declared(1)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn statics2_readable_from_saved_method_uses_stream_backend() {
        unsafe {
            perry_runtime::node_submodules::js_node_submod_install_stream_web();
            install_readable_stream_from_static();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(
                perry_runtime::node_submodules::js_node_submodule_export_as_function(
                    b"stream_web".as_ptr(),
                    10,
                    b"ReadableStream".as_ptr(),
                    14,
                ),
            );
            let method = scope.root_nanbox_f64(
                perry_runtime::closure::closure_get_own_dynamic_prop(
                    perry_runtime::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
                    "from",
                )
                .expect("from is own"),
            );
            let items = scope.root_raw_mut_ptr(perry_runtime::array::js_array_alloc(2));
            items.with_mut_ptr(|p| perry_runtime::array::js_array_push_f64(p, 7.0));
            let source = perry_runtime::closure::js_closure_call1(
                perry_runtime::js_nanbox_get_pointer(method.get_nanbox_f64())
                    as *const ClosureHeader,
                JsThis::from_f64(f64::from_bits(TAG_UNDEFINED)),
                items.with_mut_ptr(|p: *mut ArrayHeader| {
                    f64::from_bits(JSValue::pointer(p as *const u8).bits())
                }),
            );
            perry_runtime::object::js_register_stream_handle_kind_probe(js_stream_handle_kind);
            assert_eq!(
                perry_runtime::object::js_instanceof_dynamic(source, ctor.get_nanbox_f64())
                    .to_bits(),
                perry_runtime::JSValue::bool(true).bits()
            );
            let id = source;
            assert!(js_stream_handle_is_registered(id as usize));
            let reader = js_readable_stream_get_reader(id);
            let result = js_reader_read(reader);
            assert!(!result.is_null());
            let result = scope.root_nanbox_f64(perry_runtime::promise::js_promise_result(result));
            let key = perry_runtime::js_string_from_bytes(b"value".as_ptr(), 5);
            let value = perry_runtime::js_object_get_field_by_name(
                perry_runtime::js_nanbox_get_pointer(result.get_nanbox_f64())
                    as *const ObjectHeader,
                key,
            );
            assert_eq!(value.as_number(), 7.0);
        }
    }
}
