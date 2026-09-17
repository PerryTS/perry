//! Stream-specific native-module constructor metadata.

use super::*;

crate::perry_thread_local! {
    static STREAM_EVENT_EMITTER_PROTOTYPES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

const STREAM_STATIC_READABLE_FROM: f64 = 1.0;
const STREAM_STATIC_DUPLEX_FROM: f64 = 2.0;
const STREAM_STATIC_READABLE_TO_WEB: f64 = 3.0;
const STREAM_STATIC_WRITABLE_TO_WEB: f64 = 4.0;
const STREAM_STATIC_DUPLEX_TO_WEB: f64 = 5.0;
const STREAM_STATIC_READABLE_FROM_WEB: f64 = 6.0;
const STREAM_STATIC_WRITABLE_FROM_WEB: f64 = 7.0;
const STREAM_STATIC_DUPLEX_FROM_WEB: f64 = 8.0;
const STREAM_STATIC_IS_DISTURBED: f64 = 9.0;
const STREAM_STATIC_IS_ERRORED: f64 = 10.0;

pub(crate) fn scan_stream_event_emitter_prototype_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| {
        let mut protos = protos.borrow_mut();
        for bits in protos.iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
}

/// The own properties Node hangs off `require('stream')` — which IS the legacy
/// `Stream` constructor (`lib/stream.js`: `module.exports = Stream`, then
/// `Stream.Readable = …` etc.) — in Node's own-key order. `Stream` is the
/// constructor itself and is filled in by the caller, not re-resolved.
const STREAM_MODULE_EXPORT_KEYS: &[&str] = &[
    "isDestroyed",
    "isDisturbed",
    "isErrored",
    "isReadable",
    "isWritable",
    "Readable",
    "Writable",
    "Duplex",
    "Transform",
    "PassThrough",
    "duplexPair",
    "pipeline",
    "addAbortSignal",
    "finished",
    "destroy",
    "compose",
    "setDefaultHighWaterMark",
    "getDefaultHighWaterMark",
    "promises",
    "Stream",
    "_isArrayBufferView",
    "_isUint8Array",
    "_uint8ArrayToBuffer",
];

fn closure_addr_of(value: f64) -> usize {
    (value.to_bits() & crate::value::POINTER_MASK) as usize
}

/// The NaN-boxed value of a rooted object pointer, read at the call site.
fn proto_value(proto: &crate::gc::RuntimeHandle<'_>) -> f64 {
    proto.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64))
}

pub(crate) fn attach_stream_legacy_prototype(constructor_value: f64) {
    // Every step below allocates (the prototype object, its EventEmitter
    // method closures, each export value, the EventEmitter constructor), so
    // both the constructor and the prototype are re-read from their handles at
    // each use rather than held as raw addresses across a collection point.
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor_value);
    let proto = scope.root_raw_mut_ptr(js_object_alloc_with_shape(
        0x7FFF_FF33,
        1,
        b"constructor\0".as_ptr(),
        b"constructor\0".len() as u32,
    ));
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        js_object_set_field(p, 0, JSValue::from_bits(constructor.get_nanbox_u64()))
    });
    // readable-stream's `Readable.prototype.on` borrows `Stream.prototype.on`
    // via `.call(this)`; expose the EventEmitter methods on the legacy
    // `Stream.prototype` as receiver-from-`this` values so the borrow works.
    // (The installer roots `proto` itself.)
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::node_stream::install_event_emitter_prototype_methods(p)
    });
    let proto_bits = proto_value(&proto).to_bits();
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| {
        let mut protos = protos.borrow_mut();
        if !protos.contains(&proto_bits) {
            protos.push(proto_bits);
        }
    });
    crate::closure::closure_set_dynamic_prop(
        closure_addr_of(constructor.get_nanbox_f64()),
        "prototype",
        proto_value(&proto),
    );

    // #10431: the module value (`require('stream')`, `import Stream from
    // "node:stream"`) is this constructor, so it must also carry every module
    // export — `Stream.Readable`, `Stream.pipeline`, `Stream.promises`, and
    // `Stream.Stream === Stream`. Resolve each through the namespace resolver
    // `import * as ns` reads use, so `Stream.pipeline === ns.pipeline`.
    for &name in STREAM_MODULE_EXPORT_KEYS {
        let value = match name {
            "Stream" => constructor.get_nanbox_f64(),
            // The `stream_promises` submodule namespace — what
            // `require('stream').promises` resolved to while the module value
            // was a namespace object — whose `pipeline`/`finished` are the
            // promise-returning implementations.
            "promises" => unsafe {
                let submodule = b"stream_promises";
                crate::node_submodules::js_node_submodule_namespace(
                    submodule.as_ptr(),
                    submodule.len() as u32,
                )
            },
            _ => unsafe {
                let module = b"stream";
                super::js_native_module_property_by_name(
                    module.as_ptr(),
                    module.len(),
                    name.as_ptr(),
                    name.len(),
                )
            },
        };
        if JSValue::from_bits(value.to_bits()).is_undefined() {
            continue;
        }
        crate::closure::closure_set_dynamic_prop(
            closure_addr_of(constructor.get_nanbox_f64()),
            name,
            value,
        );
    }

    // #10430: Node's `lib/internal/streams/legacy.js` does
    // `ObjectSetPrototypeOf(Stream.prototype, EE.prototype)` and
    // `ObjectSetPrototypeOf(Stream, EE)`. The constructor edge is what makes
    // the inherited statics resolve (`require('stream').EventEmitter ===
    // require('events')`, `.defaultMaxListeners`, `.once`, …); the prototype
    // edge makes `Object.create(Stream.prototype) instanceof EventEmitter`.
    // Arm the events attach first: minting `EventEmitter` before it would
    // cache a constructor without its statics for the whole process.
    super::native_module_registry::js_nm_install_events();
    let event_emitter =
        scope.root_nanbox_f64(bound_native_callable_export_value("events", "EventEmitter"));
    crate::object::js_object_set_prototype_of(
        constructor.get_nanbox_f64(),
        event_emitter.get_nanbox_f64(),
    );
    let event_emitter_proto = scope.root_nanbox_f64(
        crate::object::js_function_prototype_value_for_read(event_emitter.get_nanbox_f64()),
    );
    if JSValue::from_bits(event_emitter_proto.get_nanbox_u64()).is_pointer() {
        crate::object::js_object_set_prototype_of(
            proto_value(&proto),
            event_emitter_proto.get_nanbox_f64(),
        );
    }
}

pub(crate) fn attach_stream_constructor_prototype(constructor_value: f64, name: &str) {
    let shape_id = match name {
        "Readable" => 0x7FFF_FF34,
        "Writable" => 0x7FFF_FF35,
        "Duplex" => 0x7FFF_FF36,
        "Transform" => 0x7FFF_FF37,
        "PassThrough" => 0x7FFF_FF38,
        _ => return,
    };
    let proto = js_object_alloc_with_shape(
        shape_id,
        1,
        b"constructor\0".as_ptr(),
        b"constructor\0".len() as u32,
    );
    js_object_set_field(proto, 0, JSValue::from_bits(constructor_value.to_bits()));
    // `Readable`/`Writable`/`Duplex`/`Transform`/`PassThrough` also chain their
    // own prototype methods onto `<Ctor>.prototype.<m>.call(this, …)` (e.g.
    // `Duplex.prototype.on` ↔ readable-stream borrows). Expose the EventEmitter
    // methods on these prototypes too.
    crate::node_stream::install_event_emitter_prototype_methods(proto);
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| {
        let mut protos = protos.borrow_mut();
        if !protos.contains(&proto_value.to_bits()) {
            protos.push(proto_value.to_bits());
        }
    });
    crate::closure::closure_set_dynamic_prop(
        (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype",
        proto_value,
    );
    attach_stream_constructor_statics(constructor_value, name);
}

pub(crate) fn is_stream_event_emitter_prototype_value(value: f64) -> bool {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if !jsval.is_pointer() {
        return false;
    }
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| protos.borrow().contains(&bits))
}

extern "C" fn stream_static_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    arg0: f64,
    arg1: f64,
) -> f64 {
    let kind = crate::closure::js_closure_get_capture_f64(closure, 0);
    if kind == STREAM_STATIC_READABLE_FROM {
        crate::node_stream::js_node_stream_readable_from_options(arg0, arg1)
    } else if kind == STREAM_STATIC_DUPLEX_FROM {
        crate::node_stream::js_node_stream_duplex_from_options(arg0, arg1)
    } else if kind == STREAM_STATIC_READABLE_TO_WEB {
        crate::node_stream::js_node_stream_readable_to_web_method_value(arg0)
    } else if kind == STREAM_STATIC_WRITABLE_TO_WEB {
        crate::node_stream::js_node_stream_writable_to_web_method_value(arg0)
    } else if kind == STREAM_STATIC_DUPLEX_TO_WEB {
        crate::node_stream::js_node_stream_duplex_to_web_method_value(arg0)
    } else if kind == STREAM_STATIC_READABLE_FROM_WEB {
        crate::node_stream::js_node_stream_readable_from_web(arg0, arg1)
    } else if kind == STREAM_STATIC_WRITABLE_FROM_WEB {
        crate::node_stream::js_node_stream_writable_from_web(arg0, arg1)
    } else if kind == STREAM_STATIC_DUPLEX_FROM_WEB {
        crate::node_stream::js_node_stream_duplex_from_web(arg0, arg1)
    } else if kind == STREAM_STATIC_IS_DISTURBED {
        crate::node_stream::js_node_stream_is_disturbed(arg0)
    } else if kind == STREAM_STATIC_IS_ERRORED {
        crate::node_stream::js_node_stream_is_errored(arg0)
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

fn stream_static_method_value(method: &str, kind: f64, exposed_length: u32) -> f64 {
    let func_ptr = stream_static_method_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 2);
    let closure = crate::closure::js_closure_alloc(func_ptr, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, kind);
    set_bound_native_closure_name(closure, method);
    set_builtin_closure_length(closure as usize, exposed_length);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn attach_stream_static(closure: usize, method: &str, kind: f64, exposed_length: u32) {
    let value = stream_static_method_value(method, kind, exposed_length);
    crate::closure::closure_set_dynamic_prop(closure, method, value);
}

fn attach_stream_constructor_statics(constructor_value: f64, name: &str) {
    let closure = (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize;
    if closure == 0 {
        return;
    }

    match name {
        "Readable" => {
            attach_stream_static(closure, "from", STREAM_STATIC_READABLE_FROM, 2);
            attach_stream_static(closure, "fromWeb", STREAM_STATIC_READABLE_FROM_WEB, 2);
            attach_stream_static(closure, "toWeb", STREAM_STATIC_READABLE_TO_WEB, 2);
        }
        "Writable" => {
            attach_stream_static(closure, "fromWeb", STREAM_STATIC_WRITABLE_FROM_WEB, 2);
            attach_stream_static(closure, "toWeb", STREAM_STATIC_WRITABLE_TO_WEB, 1);
        }
        "Duplex" | "Transform" | "PassThrough" => {
            attach_stream_static(closure, "from", STREAM_STATIC_DUPLEX_FROM, 1);
            attach_stream_static(closure, "fromWeb", STREAM_STATIC_DUPLEX_FROM_WEB, 2);
            attach_stream_static(closure, "toWeb", STREAM_STATIC_DUPLEX_TO_WEB, 2);
        }
        _ => {}
    }

    attach_stream_static(closure, "isDisturbed", STREAM_STATIC_IS_DISTURBED, 1);
    attach_stream_static(closure, "isErrored", STREAM_STATIC_IS_ERRORED, 1);
}

pub(crate) unsafe fn dispatch_stream_native_module_method(
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            f64::from_bits(JSValue::undefined().bits())
        }
    };
    let pack_args = || -> *mut crate::array::ArrayHeader {
        let mut arr = crate::array::js_array_alloc(args_len as u32);
        for i in 0..args_len {
            arr = crate::array::js_array_push_f64(arr, arg(i));
        }
        arr
    };

    Some(match method_name {
        "compose" => crate::node_stream::js_node_stream_compose_args(pack_args()),
        "duplexPair" => crate::node_stream::js_node_stream_duplex_pair(arg(0)),
        "pipeline" => crate::node_stream::js_node_stream_pipeline(pack_args()),
        "finished" => crate::node_stream::js_node_stream_finished(pack_args()),
        "isDisturbed" => crate::node_stream::js_node_stream_is_disturbed(arg(0)),
        "isErrored" => crate::node_stream::js_node_stream_is_errored(arg(0)),
        "isReadable" => crate::node_stream::js_node_stream_is_readable(arg(0)),
        "isWritable" => crate::node_stream::js_node_stream_is_writable(arg(0)),
        "getDefaultHighWaterMark" => crate::node_stream::js_node_stream_get_default_hwm(arg(0)),
        "setDefaultHighWaterMark" => {
            crate::node_stream::js_node_stream_set_default_hwm(arg(0), arg(1))
        }
        "addAbortSignal" => crate::node_stream::js_node_stream_add_abort_signal(arg(0), arg(1)),
        "_isArrayBufferView" => crate::node_stream::js_node_stream_is_array_buffer_view(arg(0)),
        "_isUint8Array" => crate::node_stream::js_node_stream_is_uint8_array(arg(0)),
        "_uint8ArrayToBuffer" => crate::node_stream::js_node_stream_uint8_array_to_buffer(arg(0)),
        "isDestroyed" => crate::node_stream::js_node_stream_is_destroyed(arg(0)),
        "Readable" => crate::node_stream::js_node_stream_readable_new(arg(0)),
        "Writable" => crate::node_stream::js_node_stream_writable_new(arg(0)),
        "Duplex" => crate::node_stream::js_node_stream_duplex_new(arg(0)),
        "Transform" => crate::node_stream::js_node_stream_transform_new(arg(0)),
        "PassThrough" => crate::node_stream::js_node_stream_passthrough_new(arg(0)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closure_addr(value: f64) -> usize {
        (value.to_bits() & crate::value::POINTER_MASK) as usize
    }

    fn assert_static_method(constructor: f64, name: &str, length: u32) {
        let ctor_ptr = closure_addr(constructor);
        let method = crate::closure::closure_get_dynamic_prop(ctor_ptr, name);
        let method_ptr = closure_addr(method);
        assert_ne!(method.to_bits(), crate::value::TAG_UNDEFINED);
        assert_ne!(method_ptr, 0);
        assert_eq!(builtin_closure_length(method_ptr), Some(length));
    }

    fn static_method_value(constructor: f64, name: &str) -> f64 {
        let ctor_ptr = closure_addr(constructor);
        crate::closure::closure_get_dynamic_prop(ctor_ptr, name)
    }

    unsafe fn property(value: f64, name: &[u8]) -> f64 {
        crate::value::js_get_property(value, name.as_ptr() as i64, name.len() as i64)
    }

    fn assert_object_property_function(value: f64, name: &[u8]) {
        let prop = unsafe { property(value, name) };
        assert_ne!(prop.to_bits(), crate::value::TAG_UNDEFINED);
        assert_ne!(closure_addr(prop), 0);
    }

    #[test]
    fn legacy_stream_prototype_is_event_emitter_instanceof_candidate() {
        // CLOSURE_PROPS is PROCESS-global and the gc test guards' state reset
        // (`test_clear_closure_side_tables`) clears it from parallel test
        // threads, so a static method value read here comes back
        // TAG_UNDEFINED. Serialize against those guards.
        let _global = crate::gc::global_side_table_test_lock();
        let stream_ctor = bound_native_callable_export_value("stream", "Stream");
        let stream_ptr = (stream_ctor.to_bits() & crate::value::POINTER_MASK) as usize;
        let stream_proto = crate::closure::closure_get_dynamic_prop(stream_ptr, "prototype");
        assert!(is_stream_event_emitter_prototype_value(stream_proto));

        let event_emitter = bound_native_callable_export_value("events", "EventEmitter");
        assert_eq!(
            js_instanceof_dynamic(stream_proto, event_emitter).to_bits(),
            crate::value::TAG_TRUE,
        );
        assert_eq!(
            js_instanceof(stream_proto, 0xFFFF0076).to_bits(),
            crate::value::TAG_TRUE,
        );
    }

    /// #10430 / #10431: `require('stream')` IS the legacy `Stream`
    /// constructor. It carries the module exports as statics, extends
    /// EventEmitter on both the constructor and the prototype edge, and
    /// `new Stream()` inherits `Stream.prototype`.
    #[test]
    fn stream_module_value_is_the_legacy_constructor_extending_event_emitter() {
        let _global = crate::gc::global_side_table_test_lock();
        // Mint a fresh constructor so its decoration runs inside this test
        // instead of being served from an earlier test's cache entry.
        NATIVE_CALLABLE_EXPORTS.with(|c| c.borrow_mut().remove("stream\0Stream"));
        let stream_ctor = bound_native_callable_export_value("stream", "Stream");
        let ctor_ptr = closure_addr(stream_ctor);
        assert_ne!(ctor_ptr, 0);

        assert_eq!(
            cjs_default_export_value("stream").map(f64::to_bits),
            Some(stream_ctor.to_bits()),
            "the CommonJS module value must be the Stream constructor itself"
        );
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(ctor_ptr, "Stream").to_bits(),
            stream_ctor.to_bits(),
            "Stream.Stream === Stream"
        );
        for name in [
            "Readable",
            "PassThrough",
            "pipeline",
            "finished",
            "promises",
        ] {
            let value = crate::closure::closure_get_dynamic_prop(ctor_ptr, name);
            assert!(
                JSValue::from_bits(value.to_bits()).is_pointer(),
                "Stream.{name} must be an own static of the module value"
            );
        }
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(ctor_ptr, "Readable").to_bits(),
            bound_native_callable_export_value("stream", "Readable").to_bits()
        );

        let event_emitter = bound_native_callable_export_value("events", "EventEmitter");
        assert_eq!(
            js_object_get_prototype_of(stream_ctor).to_bits(),
            event_emitter.to_bits(),
            "Object.getPrototypeOf(Stream) must be EventEmitter"
        );
        let stream_proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        assert_eq!(
            js_object_get_prototype_of(stream_proto).to_bits(),
            js_function_prototype_value_for_read(event_emitter).to_bits(),
            "Object.getPrototypeOf(Stream.prototype) must be EventEmitter.prototype"
        );

        super::super::native_module_registry::js_nm_install_stream();
        let instance = unsafe { js_new_function_construct(stream_ctor, std::ptr::null(), 0) };
        assert_eq!(
            js_object_get_prototype_of(instance).to_bits(),
            stream_proto.to_bits(),
            "new Stream() must inherit Stream.prototype"
        );
    }

    #[test]
    fn stream_constructors_expose_static_method_values() {
        // CLOSURE_PROPS is PROCESS-global and the gc test guards' state reset
        // (`test_clear_closure_side_tables`) clears it from parallel test
        // threads, so a static method value read here comes back
        // TAG_UNDEFINED. Serialize against those guards.
        let _global = crate::gc::global_side_table_test_lock();
        let readable = bound_native_callable_export_value("stream", "Readable");
        let writable = bound_native_callable_export_value("stream", "Writable");
        let duplex = bound_native_callable_export_value("stream", "Duplex");
        let transform = bound_native_callable_export_value("stream", "Transform");
        let passthrough = bound_native_callable_export_value("stream", "PassThrough");

        assert_static_method(readable, "from", 2);
        assert_static_method(readable, "fromWeb", 2);
        assert_static_method(readable, "toWeb", 2);
        assert_static_method(readable, "isDisturbed", 1);
        assert_static_method(readable, "isErrored", 1);

        assert_static_method(writable, "fromWeb", 2);
        assert_static_method(writable, "toWeb", 1);
        assert_static_method(writable, "isDisturbed", 1);
        assert_static_method(writable, "isErrored", 1);

        for constructor in [duplex, transform, passthrough] {
            assert_static_method(constructor, "from", 1);
            assert_static_method(constructor, "fromWeb", 2);
            assert_static_method(constructor, "toWeb", 2);
            assert_static_method(constructor, "isDisturbed", 1);
            assert_static_method(constructor, "isErrored", 1);
        }

        let node_readable = crate::node_stream::js_node_stream_readable_new(f64::from_bits(
            crate::value::TAG_UNDEFINED,
        ));
        let readable_to_web = static_method_value(readable, "toWeb");
        let web_readable = unsafe {
            crate::closure::js_native_call_value(readable_to_web, [node_readable].as_ptr(), 1)
        };
        assert_object_property_function(web_readable, b"getReader");

        let node_writable = crate::node_stream::js_node_stream_writable_new(f64::from_bits(
            crate::value::TAG_UNDEFINED,
        ));
        let writable_to_web = static_method_value(writable, "toWeb");
        let web_writable = unsafe {
            crate::closure::js_native_call_value(writable_to_web, [node_writable].as_ptr(), 1)
        };
        assert_object_property_function(web_writable, b"getWriter");

        let node_duplex = crate::node_stream::js_node_stream_duplex_new(f64::from_bits(
            crate::value::TAG_UNDEFINED,
        ));
        let duplex_to_web = static_method_value(duplex, "toWeb");
        let web_pair = unsafe {
            crate::closure::js_native_call_value(duplex_to_web, [node_duplex].as_ptr(), 1)
        };
        let web_pair_readable = unsafe { property(web_pair, b"readable") };
        let web_pair_writable = unsafe { property(web_pair, b"writable") };
        assert_object_property_function(web_pair_readable, b"getReader");
        assert_object_property_function(web_pair_writable, b"getWriter");
    }
}
