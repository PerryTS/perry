//! Node class inheritance is stored on the constructor and prototype shapes.
//! No property resolver or instanceof path needs an inheritance side table.
use super::*;

#[inline(always)]
fn native_parent(module: &str, name: &str) -> Option<(&'static str, &'static str)> {
    Some(match (module, name) {
        ("child_process", "ChildProcess")
        | ("cluster", "Worker")
        | ("dgram", "Socket")
        | ("domain", "Domain")
        | ("events", "EventEmitterAsyncResource")
        | ("fs", "Utf8Stream")
        | ("http", "Agent")
        | ("inspector", "Session")
        | ("net", "Server")
        | ("worker_threads", "Worker")
        | ("readline", "#InterfaceConstructor") => ("events", "EventEmitter"),
        ("stream", "Readable" | "Writable")
        | ("http", "OutgoingMessage")
        | ("http2", "Http2ServerResponse") => ("stream", "Stream"),
        ("stream", "Duplex")
        | ("fs", "ReadStream")
        | ("http", "IncomingMessage")
        | ("http2", "Http2ServerRequest") => ("stream", "Readable"),
        ("stream", "Transform") | ("net", "Socket") => ("stream", "Duplex"),
        ("stream", "PassThrough") | ("crypto", "#LazyTransform") | ("zlib", "#ZlibBase") => {
            ("stream", "Transform")
        }
        ("fs", "WriteStream") | ("crypto", "Sign" | "Verify") => ("stream", "Writable"),
        ("crypto", "Cipheriv" | "Decipheriv" | "#Hash" | "#Hmac") => ("crypto", "#LazyTransform"),
        ("crypto", "Hash") => ("crypto", "#Hash"),
        ("crypto", "Hmac") => ("crypto", "#Hmac"),
        ("http", "ClientRequest" | "ServerResponse") => ("http", "OutgoingMessage"),
        ("http", "Server") | ("tls", "Server") => ("net", "Server"),
        ("https", "Agent") => ("http", "Agent"),
        ("https", "Server") => ("tls", "Server"),
        ("inspector/promises", "Session") => ("inspector", "Session"),
        ("tls", "TLSSocket") | ("tty", "ReadStream" | "WriteStream") => ("net", "Socket"),
        ("readline", "Interface" | "InterfacePromises") => ("readline", "#Interface"),
        ("readline", "#Interface") => ("readline", "#InterfaceConstructor"),
        ("repl", "REPLServer") => ("readline", "Interface"),
        (
            "zlib",
            "Deflate" | "Inflate" | "Gzip" | "Gunzip" | "DeflateRaw" | "InflateRaw" | "Unzip"
            | "#Brotli",
        ) => ("zlib", "#Zlib"),
        ("zlib", "BrotliCompress" | "BrotliDecompress") => ("zlib", "#Brotli"),
        ("zlib", "ZstdCompress" | "ZstdDecompress") => ("zlib", "#Zstd"),
        ("zlib", "#Zlib" | "#Zstd") => ("zlib", "#ZlibBase"),
        _ => return None,
    })
}

fn finish_prototype(constructor: f64, proto: f64) -> f64 {
    // Minted function prototypes and internal aliases use Node's data
    // descriptors. Repeated parent visits reuse the attributes on the shape.
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    let proto = scope.root_nanbox_f64(proto);
    for (is_constructor, key, bits) in [(true, "prototype", 1), (false, "constructor", 5)] {
        let value = if is_constructor {
            constructor.get_nanbox_f64()
        } else {
            proto.get_nanbox_f64()
        };
        let addr = crate::value::js_nanbox_get_pointer(value) as usize;
        if crate::object::get_property_attrs(addr, key).is_none_or(|attrs| attrs.bits != bits) {
            crate::object::set_builtin_property_attrs(
                addr,
                key.to_string(),
                crate::object::PropertyAttrs { bits },
            );
        }
    }
    proto.get_nanbox_f64()
}

fn prototype(constructor: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(constructor);
    let proto = crate::object::js_function_prototype_value_for_read(ctor.get_nanbox_f64());
    if proto.to_bits() != crate::value::TAG_UNDEFINED {
        return finish_prototype(ctor.get_nanbox_f64(), proto);
    }
    // Internal Node constructors have no public module export. Their actual
    // prototype shapes still participate in the same chain.
    let proto = scope.root_raw_mut_ptr(js_object_alloc_with_shape(
        0,
        1,
        b"constructor\0".as_ptr(),
        12,
    ));
    proto.with_mut_ptr(|p| js_object_set_field(p, 0, JSValue::from_bits(ctor.get_nanbox_u64())));
    let value =
        proto.with_mut_ptr(|p: *mut ObjectHeader| crate::value::js_nanbox_pointer(p as i64));
    crate::closure::closure_set_dynamic_prop(
        crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
        "prototype",
        value,
    );
    finish_prototype(ctor.get_nanbox_f64(), value)
}

pub(crate) fn link_parent(value: f64, parent: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(value);
    let parent = scope.root_nanbox_f64(parent);
    let proto = scope.root_nanbox_f64(prototype(ctor.get_nanbox_f64()));
    let parent_proto = scope.root_nanbox_f64(prototype(parent.get_nanbox_f64()));
    crate::object::js_object_set_prototype_of(ctor.get_nanbox_f64(), parent.get_nanbox_f64());
    if proto.get_nanbox_u64() != parent_proto.get_nanbox_u64() {
        crate::object::js_object_set_prototype_of(
            proto.get_nanbox_f64(),
            parent_proto.get_nanbox_f64(),
        );
    }
    ctor.get_nanbox_f64()
}

#[inline(always)]
unsafe fn attach(module: &str, name: &str, value: f64) -> f64 {
    if native_parent(module, name).is_none()
        && !matches!(
            (module, name),
            ("net", "BlockList" | "SocketAddress") | ("crypto", "ECDH") | ("repl", "Recoverable")
        )
    {
        return value;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(value);
    if let Some(display) = name.strip_prefix('#') {
        callable_exports::set_bound_native_closure_name(
            crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64())
                as *mut crate::closure::ClosureHeader,
            display,
        );
    }
    if module == "crypto" && matches!(name, "Hash" | "Hmac") {
        callable_exports::set_bound_native_closure_name(
            crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64())
                as *mut crate::closure::ClosureHeader,
            "deprecated",
        );
    }
    match (module, name) {
        ("net", "BlockList") => {
            own_static(ctor.get_nanbox_f64(), "isBlockList", 0, 1);
        }
        ("net", "SocketAddress") => {
            own_static(ctor.get_nanbox_f64(), "isSocketAddress", 1, 1);
            own_static(ctor.get_nanbox_f64(), "parse", 2, 1);
        }
        ("crypto", "ECDH") => {
            own_static(ctor.get_nanbox_f64(), "convertKey", 3, 5);
        }
        _ => {}
    }
    if module == "repl" && name == "Recoverable" {
        let parent = crate::object::js_get_global_this_builtin_value(b"SyntaxError".as_ptr(), 11);
        return link_parent(ctor.get_nanbox_f64(), parent);
    }
    if let Some((parent_module, parent_name)) = native_parent(module, name) {
        let parent = scope.root_nanbox_f64(bound_native_callable_export_value(
            parent_module,
            parent_name,
        ));
        if module == "crypto" && matches!(name, "Hash" | "Hmac") {
            // Deprecated wrappers share the original constructor's prototype.
            // Store the alias once; prototype reads use the actual own field.
            let proto = scope.root_nanbox_f64(prototype(parent.get_nanbox_f64()));
            crate::closure::closure_set_dynamic_prop(
                crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
                "prototype",
                proto.get_nanbox_f64(),
            );
        }
        link_parent(ctor.get_nanbox_f64(), parent.get_nanbox_f64());
    }
    ctor.get_nanbox_f64()
}

macro_rules! attach_handler {
    ($name:ident, $module:literal, $own:ident) => {
        pub(crate) unsafe fn $name(name: &str, value: f64, addr: usize) -> f64 {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(value);
            // Decorators mutate this value; allocations may relocate it.
            // Reread our root rather than overwriting it with a raw return.
            callable_exports::$own(name, ctor.get_nanbox_f64(), addr);
            attach($module, name, ctor.get_nanbox_f64())
        }
    };
    ($name:ident, $module:literal) => {
        pub(crate) unsafe fn $name(name: &str, value: f64, _addr: usize) -> f64 {
            attach($module, name, value)
        }
    };
}
attach_handler!(
    nm_attach_child_process,
    "child_process",
    nm_attach_child_process
);
attach_handler!(nm_attach_cluster, "cluster", nm_attach_cluster);
attach_handler!(nm_attach_crypto, "crypto", nm_attach_crypto);
attach_handler!(nm_attach_events, "events", nm_attach_events);
attach_handler!(nm_attach_stream, "stream", nm_attach_stream);
attach_handler!(nm_attach_tls, "tls", nm_attach_tls);
attach_handler!(nm_attach_tty, "tty", nm_attach_tty);
attach_handler!(nm_attach_dgram, "dgram");
attach_handler!(nm_attach_domain, "domain");
attach_handler!(nm_attach_fs, "fs");
pub(crate) unsafe fn nm_attach_inspector(name: &str, value: f64, _addr: usize) -> f64 {
    let (module, _) = callable_exports::bound_native_callable_module_and_method(value).unwrap();
    attach(&module, name, value)
}
attach_handler!(nm_attach_net, "net");
attach_handler!(nm_attach_readline, "readline");
attach_handler!(nm_attach_repl, "repl");
attach_handler!(nm_attach_zlib, "zlib");
attach_handler!(nm_attach_worker, "worker_threads");

// These two existing buckets serve multiple module namespaces. Their parent
// differs with the namespace, so read the constructor's captured namespace.
pub(crate) unsafe fn nm_attach_http(name: &str, value: f64, addr: usize) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(value);
    callable_exports::nm_attach_http(name, ctor.get_nanbox_f64(), addr);
    let (module, _) =
        callable_exports::bound_native_callable_module_and_method(ctor.get_nanbox_f64()).unwrap();
    attach(&module, name, ctor.get_nanbox_f64())
}

extern "C" fn own_static_thunk(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
) -> f64 {
    let kind = crate::closure::js_closure_get_capture_f64(closure, 0) as u8;
    let (provider, method) = if kind == 3 {
        (&crate::value::JS_NATIVE_CRYPTO_DISPATCH, "convertKey")
    } else {
        (
            &crate::value::JS_NATIVE_NET_DISPATCH,
            match kind {
                0 => "BlockList.isBlockList",
                1 => "SocketAddress.isSocketAddress",
                _ => "SocketAddress.parse",
            },
        )
    };
    let ptr = provider.load(std::sync::atomic::Ordering::SeqCst);
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // The provider reads these slots across allocations; expose rewriteable
    // argument cells to the collector rather than copying unrooted doubles.
    let args = [a, b, c, d, e].map(std::cell::UnsafeCell::new);
    struct Frame(u64);
    impl Drop for Frame {
        fn drop(&mut self) {
            crate::gc::js_shadow_frame_pop(self.0);
        }
    }
    let frame = Frame(crate::gc::js_shadow_frame_push(5));
    for (index, arg) in args.iter().enumerate() {
        crate::gc::js_shadow_slot_bind(index as u32, arg.get().cast());
    }
    let result = crate::exception::catch_js_throw(|| unsafe {
        let dispatch: crate::value::JsNativeNetDispatchFn = std::mem::transmute(ptr);
        dispatch(
            method.as_ptr(),
            method.len(),
            args.as_ptr().cast(),
            args.len(),
        )
    });
    drop(frame);
    match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

fn own_static(ctor: f64, name: &str, kind: u8, length: u32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(ctor);
    let method = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(own_static_thunk, 5; with_declared(5)),
        1,
    ));
    method.with_mut_ptr(|p| crate::closure::js_closure_set_capture_f64(p, 0, kind as f64));
    method.with_mut_ptr(|p| callable_exports::set_bound_native_closure_name(p, name));
    method.with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
        callable_exports::set_builtin_closure_length(p as usize, length);
        if kind != 3 {
            callable_exports::set_builtin_closure_non_constructable(p as usize);
        }
    });
    crate::closure::closure_set_dynamic_prop(
        crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
        name,
        method.with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
            crate::value::js_nanbox_pointer(p as i64)
        }),
    );
    crate::object::set_builtin_property_attrs(
        crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
        name.to_string(),
        crate::object::PropertyAttrs::new(true, kind == 3, true),
    );
    ctor.get_nanbox_f64()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructor_statics2_parents_live_on_shapes() {
        crate::object::native_module_registry::js_nm_install_stream();
        crate::object::native_module_registry::js_nm_install_net();
        crate::object::native_module_registry::js_nm_install_worker_threads();
        for (module, name, pm, pn) in [
            ("stream", "Readable", "stream", "Stream"),
            ("stream", "Writable", "stream", "Stream"),
            ("stream", "Duplex", "stream", "Readable"),
            ("stream", "Transform", "stream", "Duplex"),
            ("stream", "PassThrough", "stream", "Transform"),
            ("net", "Socket", "stream", "Duplex"),
            ("net", "Server", "events", "EventEmitter"),
            ("worker_threads", "Worker", "events", "EventEmitter"),
        ] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(bound_native_callable_export_value(module, name));
            let parent = scope.root_nanbox_f64(bound_native_callable_export_value(pm, pn));
            assert_eq!(
                crate::object::js_object_get_prototype_of(ctor.get_nanbox_f64()).to_bits(),
                parent.get_nanbox_u64(),
                "{module}.{name}"
            );
            let proto = scope.root_nanbox_f64(prototype(ctor.get_nanbox_f64()));
            let pp = scope.root_nanbox_f64(prototype(parent.get_nanbox_f64()));
            assert_eq!(
                crate::object::js_object_get_prototype_of(proto.get_nanbox_f64()).to_bits(),
                pp.get_nanbox_u64()
            );
            let key = crate::string::js_string_from_bytes(b"defaultMaxListeners".as_ptr(), 19);
            assert_eq!(
                crate::object::js_object_get_field_by_name(
                    crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64())
                        as *const ObjectHeader,
                    key
                )
                .as_number(),
                10.0
            );
        }
    }
    #[test]
    fn constructor_statics2_stream_statics_share_inherited_identity() {
        crate::object::native_module_registry::js_nm_install_stream();
        let scope = crate::gc::RuntimeHandleScope::new();
        let duplex = scope.root_nanbox_f64(bound_native_callable_export_value("stream", "Duplex"));
        let pass =
            scope.root_nanbox_f64(bound_native_callable_export_value("stream", "PassThrough"));
        let da = crate::value::js_nanbox_get_pointer(duplex.get_nanbox_f64()) as usize;
        let pa = crate::value::js_nanbox_get_pointer(pass.get_nanbox_f64()) as usize;
        for name in ["from", "fromWeb", "toWeb"] {
            assert_eq!(
                crate::closure::closure_get_dynamic_prop(da, name).to_bits(),
                crate::closure::closure_get_dynamic_prop(pa, name).to_bits()
            );
            assert!(!crate::closure::closure_has_own_dynamic_prop(pa, name));
        }
    }
    #[test]
    fn constructor_statics2_native_brands_also_follow_prototypes() {
        crate::object::native_module_registry::js_nm_install_crypto();
        crate::object::native_module_registry::js_nm_install_http();
        for (module, name) in [
            ("crypto", "Hash"),
            ("crypto", "Hmac"),
            ("crypto", "Cipheriv"),
            ("crypto", "Decipheriv"),
            ("http", "Agent"),
            ("https", "Agent"),
        ] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(bound_native_callable_export_value(module, name));
            let proto = scope.root_nanbox_f64(prototype(ctor.get_nanbox_f64()));
            let instance = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
            let value = instance
                .with_mut_ptr(|p: *mut ObjectHeader| crate::value::js_nanbox_pointer(p as i64));
            crate::object::js_object_set_prototype_of(value, proto.get_nanbox_f64());
            assert_eq!(
                crate::object::js_instanceof_dynamic(
                    instance.with_mut_ptr(|p: *mut ObjectHeader| crate::value::js_nanbox_pointer(
                        p as i64
                    )),
                    ctor.get_nanbox_f64()
                )
                .to_bits(),
                crate::value::TAG_TRUE,
                "{module}.{name}"
            );
        }
    }
    #[test]
    fn constructor_statics2_user_class_keeps_its_native_static_parent() {
        const CHILD: u32 = 0x7611_0021;
        crate::object::native_module_registry::js_nm_install_stream();
        let scope = crate::gc::RuntimeHandleScope::new();
        let parent =
            scope.root_nanbox_f64(bound_native_callable_export_value("stream", "Transform"));
        unsafe {
            crate::object::js_register_class_name(CHILD, b"Statics2Child".as_ptr(), 13);
        }
        crate::object::js_register_class_parent_dynamic(CHILD, parent.get_nanbox_f64());
        let child = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::class_value::class_value_ptr(CHILD) as i64,
        ));
        assert_eq!(
            crate::object::js_object_get_prototype_of(child.get_nanbox_f64()).to_bits(),
            parent.get_nanbox_u64()
        );
        let key = crate::string::js_string_from_bytes(b"getMaxListeners".as_ptr(), 15);
        let method = crate::object::js_object_get_field_by_name(
            crate::value::js_nanbox_get_pointer(child.get_nanbox_f64()) as *const ObjectHeader,
            key,
        );
        assert!(crate::closure::is_closure_ptr(
            crate::value::js_nanbox_get_pointer(f64::from_bits(method.bits())) as usize
        ));
    }
    #[test]
    fn constructor_statics2_deprecated_wrappers_share_parent_prototypes() {
        crate::object::native_module_registry::js_nm_install_crypto();
        for name in ["Hash", "Hmac"] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(bound_native_callable_export_value("crypto", name));
            let parent = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(
                ctor.get_nanbox_f64(),
            ));
            let cp = scope.root_nanbox_f64(prototype(ctor.get_nanbox_f64()));
            assert_eq!(
                cp.get_nanbox_u64(),
                prototype(parent.get_nanbox_f64()).to_bits()
            );
            let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
            assert_eq!(
                crate::object::js_object_get_field_by_name(
                    crate::value::js_nanbox_get_pointer(cp.get_nanbox_f64()) as *const ObjectHeader,
                    key
                )
                .bits(),
                parent.get_nanbox_u64()
            );
        }
    }
    #[test]
    fn constructor_statics2_prototype_descriptors_match_node() {
        crate::object::native_module_registry::js_nm_install_crypto();
        crate::object::native_module_registry::js_nm_install_stream();
        for (module, name, parent) in [
            ("crypto", "Hash", false),
            ("crypto", "Hash", true),
            ("crypto", "Hmac", false),
            ("crypto", "Hmac", true),
            ("stream", "Transform", false),
        ] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(bound_native_callable_export_value(module, name));
            let ctor = scope.root_nanbox_f64(if parent {
                crate::object::js_object_get_prototype_of(ctor.get_nanbox_f64())
            } else {
                ctor.get_nanbox_f64()
            });
            let proto = scope.root_nanbox_f64(prototype(ctor.get_nanbox_f64()));
            for (value, key, bits) in [
                (ctor.get_nanbox_f64(), "prototype", 1),
                (proto.get_nanbox_f64(), "constructor", 5),
            ] {
                assert_eq!(
                    crate::object::get_property_attrs(
                        crate::value::js_nanbox_get_pointer(value) as usize,
                        key
                    )
                    .expect("Node descriptor")
                    .bits,
                    bits
                );
            }
        }
    }
    #[test]
    fn constructor_statics2_own_static_descriptors_match_node() {
        crate::object::native_module_registry::js_nm_install_net();
        crate::object::native_module_registry::js_nm_install_crypto();
        for (module, name, method, bits) in [
            ("net", "BlockList", "isBlockList", 5),
            ("net", "SocketAddress", "isSocketAddress", 5),
            ("net", "SocketAddress", "parse", 5),
            ("crypto", "ECDH", "convertKey", 7),
        ] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(bound_native_callable_export_value(module, name));
            assert_eq!(
                crate::object::get_property_attrs(
                    crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
                    method
                )
                .expect("own descriptor")
                .bits,
                bits
            );
        }
    }
    #[test]
    fn constructor_statics2_assignments_follow_constructor_setters() {
        crate::object::native_module_registry::js_nm_install_stream();
        let scope = crate::gc::RuntimeHandleScope::new();
        let ee =
            scope.root_nanbox_f64(bound_native_callable_export_value("events", "EventEmitter"));
        let child = scope.root_nanbox_f64(bound_native_callable_export_value("stream", "Readable"));
        let before = crate::closure::closure_get_dynamic_prop(
            crate::value::js_nanbox_get_pointer(ee.get_nanbox_f64()) as usize,
            "defaultMaxListeners",
        );
        let key = scope.root_raw_mut_ptr(crate::string::js_string_from_bytes(
            b"defaultMaxListeners".as_ptr(),
            19,
        ));
        crate::object::js_object_set_field_by_name(
            crate::value::js_nanbox_get_pointer(child.get_nanbox_f64()) as *mut ObjectHeader,
            key.get_raw_mut_ptr(),
            23.0,
        );
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(
                crate::value::js_nanbox_get_pointer(ee.get_nanbox_f64()) as usize,
                "defaultMaxListeners"
            ),
            23.0
        );
        assert!(!crate::closure::closure_has_own_dynamic_prop(
            crate::value::js_nanbox_get_pointer(child.get_nanbox_f64()) as usize,
            "defaultMaxListeners"
        ));
        crate::object::js_object_set_field_by_name(
            crate::value::js_nanbox_get_pointer(ee.get_nanbox_f64()) as *mut ObjectHeader,
            key.get_raw_mut_ptr(),
            before,
        );
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(
                crate::value::js_nanbox_get_pointer(ee.get_nanbox_f64()) as usize,
                "defaultMaxListeners"
            ),
            before
        );
    }
    #[test]
    fn constructor_statics2_async_statics_are_class_methods() {
        crate::object::native_module_registry::js_nm_install_async_hooks();
        for (name, method) in [
            ("AsyncResource", "bind"),
            ("AsyncLocalStorage", "bind"),
            ("AsyncLocalStorage", "snapshot"),
        ] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor =
                scope.root_nanbox_f64(bound_native_callable_export_value("async_hooks", name));
            let addr = crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize;
            assert_eq!(
                crate::object::get_property_attrs(addr, method)
                    .expect("class static descriptor")
                    .bits,
                5
            );
            let value = scope.root_nanbox_f64(
                crate::closure::closure_get_own_dynamic_prop(addr, method).expect("own static"),
            );
            assert_eq!(
                crate::object::js_function_prototype_value_for_read(value.get_nanbox_f64())
                    .to_bits(),
                crate::value::TAG_UNDEFINED
            );
        }
    }
    #[test]
    fn constructor_statics2_shared_default_accessor() {
        crate::object::native_module_registry::js_nm_install_stream();
        let scope = crate::gc::RuntimeHandleScope::new();
        let ee =
            scope.root_nanbox_f64(bound_native_callable_export_value("events", "EventEmitter"));
        let child = scope.root_nanbox_f64(bound_native_callable_export_value("stream", "Readable"));
        let addr = crate::value::js_nanbox_get_pointer(ee.get_nanbox_f64()) as usize;
        let descriptor = crate::object::get_accessor_descriptor(addr, "defaultMaxListeners")
            .expect("Node shared default is an accessor");
        let setter = scope.root_nanbox_u64(descriptor.set);
        let before = crate::closure::closure_get_dynamic_prop(addr, "defaultMaxListeners");
        crate::closure::js_closure_call1(
            crate::value::js_nanbox_get_pointer(setter.get_nanbox_f64())
                as *const crate::closure::ClosureHeader,
            crate::closure::JsThis::from_f64(child.get_nanbox_f64()),
            23.0,
        );
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(
                crate::value::js_nanbox_get_pointer(child.get_nanbox_f64()) as usize,
                "defaultMaxListeners"
            ),
            23.0
        );
        assert!(!crate::closure::closure_has_own_dynamic_prop(
            crate::value::js_nanbox_get_pointer(child.get_nanbox_f64()) as usize,
            "defaultMaxListeners"
        ));
        crate::closure::js_closure_call1(
            crate::value::js_nanbox_get_pointer(setter.get_nanbox_f64())
                as *const crate::closure::ClosureHeader,
            crate::closure::JsThis::from_f64(ee.get_nanbox_f64()),
            before,
        );
    }
    #[test]
    fn constructor_statics2_own_values_are_functions() {
        crate::object::native_module_registry::js_nm_install_net();
        crate::object::native_module_registry::js_nm_install_crypto();
        for (module, name, method) in [
            ("net", "BlockList", "isBlockList"),
            ("net", "SocketAddress", "isSocketAddress"),
            ("net", "SocketAddress", "parse"),
            ("crypto", "ECDH", "convertKey"),
        ] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let ctor = scope.root_nanbox_f64(bound_native_callable_export_value(module, name));
            let addr = crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize;
            let value = crate::closure::closure_get_dynamic_prop(addr, method);
            assert!(
                crate::closure::is_closure_ptr(crate::value::js_nanbox_get_pointer(value) as usize),
                "{module}.{name}.{method}"
            );
            assert_eq!(
                callable_exports::builtin_closure_is_non_constructable_value(value),
                module != "crypto"
            );
        }
    }
}

extern "C" fn default_max_listeners_get(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}
extern "C" fn default_max_listeners_set(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    let value = if js.is_int32() {
        js.as_int32() as f64
    } else {
        value
    };
    if (!js.is_number() && !js.is_int32()) || value.is_nan() || value < 0.0 {
        crate::fs::validate::throw_range_error_with_code(
            "The value of defaultMaxListeners is out of range. It must be a non-negative number.",
        );
    }
    let getter = crate::closure::js_closure_get_capture_f64(closure, 0);
    crate::closure::js_closure_set_capture_f64(
        crate::value::js_nanbox_get_pointer(getter) as *mut crate::closure::ClosureHeader,
        0,
        value,
    );
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn install_event_emitter_statics(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(value);
    for method in [
        "addAbortListener",
        "once",
        "on",
        "getEventListeners",
        "getMaxListeners",
        "listenerCount",
        "setMaxListeners",
        "init",
    ] {
        let method_value =
            scope.root_nanbox_f64(bound_native_callable_export_value("events", method));
        crate::closure::closure_set_dynamic_prop(
            crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
            method,
            method_value.get_nanbox_f64(),
        );
    }
    for (name, val) in [
        ("EventEmitter", ctor.get_nanbox_f64()),
        ("usingDomains", f64::from_bits(crate::value::TAG_FALSE)),
        ("captureRejections", f64::from_bits(crate::value::TAG_FALSE)),
    ] {
        crate::closure::closure_set_dynamic_prop(
            crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
            name,
            val,
        );
    }
    for (name, symbol) in [
        ("captureRejectionSymbol", "nodejs.rejection"),
        ("errorMonitor", "events.errorMonitor"),
    ] {
        let text = crate::string::js_string_from_bytes(symbol.as_ptr(), symbol.len() as u32);
        let sym =
            unsafe { crate::symbol::js_symbol_for(crate::value::js_nanbox_string(text as i64)) };
        crate::closure::closure_set_dynamic_prop(
            crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize,
            name,
            sym,
        );
    }
    // Node exposes this as a lazy accessor. Besides preserving its shape,
    // laziness avoids eagerly materializing EventEmitter.prototype just to
    // link a child which the program may never request.
    let async_get = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(event_async_resource_get, 0; with_declared(0)),
        0,
    ));
    async_get.with_mut_ptr(|p| {
        callable_exports::set_bound_native_closure_name(p, "lazyEventEmitterAsyncResource")
    });
    async_get.with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
        callable_exports::set_builtin_closure_non_constructable(p as usize)
    });
    let addr = crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize;
    crate::closure::closure_set_dynamic_prop(
        addr,
        "EventEmitterAsyncResource",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    crate::object::set_builtin_accessor_descriptor(
        addr,
        "EventEmitterAsyncResource".to_string(),
        crate::object::AccessorDescriptor {
            get: async_get
                .with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
                    crate::value::js_nanbox_pointer(p as i64)
                })
                .to_bits(),
            set: 0,
        },
        crate::object::PropertyAttrs::new(false, true, true),
    );
    let get = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(default_max_listeners_get, 0; with_declared(0)),
        1,
    ));
    get.with_mut_ptr(|p| crate::closure::js_closure_set_capture_f64(p, 0, 10.0));
    get.with_mut_ptr(|p| {
        callable_exports::set_bound_native_closure_name(p, "get defaultMaxListeners")
    });
    get.with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
        callable_exports::set_builtin_closure_non_constructable(p as usize)
    });
    let set = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(default_max_listeners_set, 1; with_declared(1)),
        1,
    ));
    set.with_mut_ptr(|p| {
        callable_exports::set_bound_native_closure_name(p, "set defaultMaxListeners")
    });
    set.with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
        callable_exports::set_builtin_closure_non_constructable(p as usize)
    });
    set.with_mut_ptr(|p| {
        crate::closure::js_closure_set_capture_f64(
            p,
            0,
            get.with_mut_ptr(|g: *mut crate::closure::ClosureHeader| {
                crate::value::js_nanbox_pointer(g as i64)
            }),
        )
    });
    let addr = crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize;
    // The accessor pair lives in the constructor's own property shape.
    crate::closure::closure_set_dynamic_prop(
        addr,
        "defaultMaxListeners",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    crate::object::set_builtin_accessor_descriptor(
        addr,
        "defaultMaxListeners".to_string(),
        crate::object::AccessorDescriptor {
            get: get
                .with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
                    crate::value::js_nanbox_pointer(p as i64)
                })
                .to_bits(),
            set: set
                .with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
                    crate::value::js_nanbox_pointer(p as i64)
                })
                .to_bits(),
        },
        crate::object::PropertyAttrs::new(false, true, true),
    );
    ctor.get_nanbox_f64()
}

extern "C" fn event_async_resource_get(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    bound_native_callable_export_value("events", "EventEmitterAsyncResource")
}
