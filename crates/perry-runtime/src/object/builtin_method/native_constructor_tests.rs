use crate::closure::native_constructor::*;

#[test]
fn sealed_native_constructor_words_prove_and_withdraw_every_fixed_chain() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::js_get_global_this();
    crate::object::native_module_registry::js_nm_install_events();
    crate::object::native_module_registry::js_nm_install_stream();
    crate::object::native_module_registry::js_nm_install_http();
    unsafe {
        let ctor = |module, name| {
            crate::object::native_module::bound_native_callable_export_value(module, name)
        };
        let ptr = |value| {
            crate::value::js_nanbox_get_pointer(value) as *mut crate::closure::ClosureHeader
        };
        let ee = ctor("events", "EventEmitter");
        let stream = ctor("stream", "Stream");
        let readable = ctor("stream", "Readable");
        let writable = ctor("stream", "Writable");
        let duplex = ctor("stream", "Duplex");
        let transform = ctor("stream", "Transform");
        let pass = ctor("stream", "PassThrough");
        let outgoing = ctor("http", "OutgoingMessage");
        let response = ctor("http", "ServerResponse");
        let incoming = ctor("http", "IncomingMessage");
        for value in [
            ee, stream, readable, writable, duplex, transform, pass, outgoing, response, incoming,
        ] {
            for key in [b"call".as_slice(), b"apply", b"bind"] {
                assert!(
                    inherits_function(ptr(value), key),
                    "positive admission must be live before mutation"
                );
            }
            assert!(!inherits_function(ptr(value), b"from"));
        }

        // An existing own value replaced through the ordinary direct store.
        crate::closure::closure_set_dynamic_prop(ptr(writable) as usize, "fromWeb", 1.0);
        assert!(!inherits_function(ptr(writable), b"call"));
        assert!(inherits_function(ptr(readable), b"call"));

        // A new data descriptor and a deletion of an existing own key.
        crate::closure::closure_define_data_with_attrs(
            ptr(response) as usize,
            "call",
            2.0,
            crate::object::PropertyAttrs::new(true, false, true),
        );
        assert!(!inherits_function(ptr(response), b"call"));
        assert!(crate::closure::closure_delete_own_dynamic_prop(
            ptr(duplex) as usize,
            "from"
        ));
        assert!(!inherits_function(ptr(duplex), b"call"));
        assert!(
            !inherits_function(ptr(transform), b"call"),
            "an unchanged child observes its written ancestor"
        );

        // Relinking the receiver and then a pristine receiver's ancestor.
        crate::closure::closure_set_static_prototype(
            ptr(incoming) as usize,
            crate::value::TAG_NULL,
        );
        assert!(!inherits_function(ptr(incoming), b"call"));
        assert!(inherits_function(ptr(outgoing), b"call"));
        crate::closure::closure_set_static_prototype(ptr(stream) as usize, crate::value::TAG_NULL);
        assert!(!inherits_function(ptr(outgoing), b"call"));
        assert!(!inherits_function(ptr(readable), b"call"));
        assert!(inherits_function(ptr(ee), b"call"));
    }
}
