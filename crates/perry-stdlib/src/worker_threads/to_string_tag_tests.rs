/// #12629: the messaging prototypes carry Node's `@@toStringTag`, so the
/// prototypes, a channel the worker_threads factory builds and its port all
/// print their interface through `Object.prototype.toString`.
#[test]
fn messaging_prototypes_carry_their_to_string_tag() {
    std::thread::spawn(|| {
        extern "C" {
            fn js_runtime_install_messaging();
        }
        perry_runtime::gc::gc_init();
        // A compiled program that names a messaging global installs this
        // feature before its realm births the messaging prototypes.
        unsafe { js_runtime_install_messaging() };
        unsafe { crate::common::dispatch::js_stdlib_init_dispatch() };
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let get = |object: f64, name: &str| {
            let key = perry_runtime::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let object = perry_runtime::value::js_nanbox_get_pointer(object) as *const _;
            f64::from_bits(perry_runtime::object::js_object_get_field_by_name(object, key).bits())
        };
        let tag = |value: f64| {
            let text = unsafe { perry_runtime::object::object_prototype_to_string(value) };
            let text = perry_runtime::value::js_get_string_pointer_unified(text);
            let text =
                unsafe { perry_ffi::JsString::from_raw(text as *mut perry_ffi::StringHeader) };
            String::from_utf8(perry_ffi::read_bytes(text).unwrap().to_vec()).unwrap()
        };
        let global = scope.root_nanbox_f64(perry_runtime::object::js_get_global_this());
        for name in ["MessageChannel", "MessagePort", "BroadcastChannel"] {
            let ctor = scope.root_nanbox_f64(get(global.get_nanbox_f64(), name));
            let proto = get(ctor.get_nanbox_f64(), "prototype");
            assert_eq!(tag(proto), format!("[object {name}]"));
        }
        let ctor = scope.root_nanbox_f64(get(global.get_nanbox_f64(), "MessageChannel"));
        let args = perry_runtime::array::js_array_alloc(0);
        let args = perry_runtime::value::js_nanbox_pointer(args as i64);
        let channel = scope.root_nanbox_f64(perry_runtime::proxy::js_reflect_construct(
            ctor.get_nanbox_f64(),
            args,
            ctor.get_nanbox_f64(),
        ));
        assert_eq!(tag(channel.get_nanbox_f64()), "[object MessageChannel]");
        let port = get(channel.get_nanbox_f64(), "port1");
        assert_eq!(tag(port), "[object MessagePort]");
    })
    .join()
    .unwrap();
}
