//! Stream-specific native-module constructor metadata.

use super::*;

const STREAM_STATIC_READABLE_FROM: f64 = 1.0;
const STREAM_STATIC_DUPLEX_FROM: f64 = 2.0;
const STREAM_STATIC_READABLE_TO_WEB: f64 = 3.0;
const STREAM_STATIC_WRITABLE_TO_WEB: f64 = 4.0;
const STREAM_STATIC_DUPLEX_TO_WEB: f64 = 5.0;
const STREAM_STATIC_READABLE_FROM_WEB: f64 = 6.0;
const STREAM_STATIC_WRITABLE_FROM_WEB: f64 = 7.0;
const STREAM_STATIC_DUPLEX_FROM_WEB: f64 = 8.0;

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

pub(crate) fn attach_stream_legacy_prototype(constructor_value: f64) {
    // Every step below allocates (the prototype object, its method closures,
    // each export value, the EventEmitter constructor), so the constructor is
    // re-read from its handle at each use rather than held as a raw address
    // across a collection point.
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor_value);
    // #10430: Node's `lib/internal/streams/legacy.js` does
    // `ObjectSetPrototypeOf(Stream.prototype, EE.prototype)` and
    // `ObjectSetPrototypeOf(Stream, EE)`. `Stream.prototype`'s layout names
    // `EventEmitter.prototype` as its [[Prototype]], so that prototype is
    // built first. Arm the events attach before minting `EventEmitter`:
    // minting it earlier would cache a constructor without its statics for
    // the whole process.
    super::native_module_registry::js_nm_install_events();
    let event_emitter =
        scope.root_nanbox_f64(bound_native_callable_export_value("events", "EventEmitter"));
    let _ = crate::object::js_function_prototype_value_for_read(event_emitter.get_nanbox_f64());
    // `Stream.prototype`: node's `constructor`, `pipe` and `eventNames`, born
    // on its static layout and sealed (readable-stream's `.call(this)` borrows
    // of the emitter methods reach them on `EventEmitter.prototype`).
    let proto = scope.root_nanbox_f64(crate::node_stream::build_legacy_stream_prototype(
        constructor.get_nanbox_f64(),
    ));
    crate::closure::closure_set_dynamic_prop(
        closure_addr_of(constructor.get_nanbox_f64()),
        "prototype",
        proto.get_nanbox_f64(),
    );

    // #10431: the module value (`require('stream')`, `import Stream from
    // "node:stream"`) is this constructor, so it must also carry every module
    // export — `Stream.Readable`, `Stream.pipeline`, `Stream.promises`, and
    // `Stream.Stream === Stream`. Resolve each through the namespace resolver
    // `import * as ns` reads use, so `Stream.pipeline === ns.pipeline`.
    for &name in STREAM_MODULE_EXPORT_KEYS {
        let value = match name {
            "Stream" => constructor.get_nanbox_f64(),
            // The cached `stream/promises` namespace — the object
            // `require('stream/promises')` and `import * as` hand out.
            "promises" => {
                let module = "stream/promises";
                super::js_create_native_module_namespace(module.as_ptr(), module.len())
            }
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

    // The constructor edge makes the inherited statics resolve
    // (`require('stream').EventEmitter === require('events')`,
    // `.defaultMaxListeners`, `.once`, …); the prototype edge is the layout's.
    crate::object::js_object_set_prototype_of(
        constructor.get_nanbox_f64(),
        event_emitter.get_nanbox_f64(),
    );
}

pub(crate) fn attach_stream_constructor_prototype(constructor_value: f64, name: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor_value);
    // G1 (STREAM-PAYLOAD-DESIGN): the classic stream methods live here, on
    // `Readable`/`Writable`/`Duplex`/`Transform.prototype`, one
    // receiver-from-`this` closure each per realm, as in node; each prototype
    // is born on its static layout (node's keys, order and attributes) and
    // sealed. `PassThrough` inherits them through `Transform.prototype`.
    // `<Ctor>.prototype.<m>.call(this, …)` borrows (readable-stream) reach the
    // same bodies.
    let kind = match name {
        "Readable" => Some(crate::node_stream::StreamProto::Readable),
        "Writable" => Some(crate::node_stream::StreamProto::Writable),
        "Duplex" => Some(crate::node_stream::StreamProto::Duplex),
        "Transform" => Some(crate::node_stream::StreamProto::Transform),
        _ => None,
    };
    let proto_value = match kind {
        Some(kind) => {
            crate::node_stream::build_stream_prototype(constructor.get_nanbox_f64(), kind)
        }
        None if name == "PassThrough" => {
            crate::node_stream::build_passthrough_prototype(constructor.get_nanbox_f64())
        }
        None => return,
    };
    crate::closure::closure_set_dynamic_prop(
        closure_addr_of(constructor.get_nanbox_f64()),
        "prototype",
        proto_value,
    );
    attach_stream_constructor_statics(constructor.get_nanbox_f64(), name);
}

extern "C" fn stream_static_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
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
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

fn stream_static_method_value(method: &str, kind: f64, exposed_length: u32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(#[realm("native_module_stream.realm.json#body:stream_static_method_thunk")] stream_static_method_thunk, 2; with_declared(2)),
        1,
    ));
    closure.with_mut_ptr(|p| crate::closure::js_closure_set_capture_f64(p, 0, kind));
    closure.with_mut_ptr(|p| set_bound_native_closure_name(p, method));
    closure.with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
        set_builtin_closure_length(p as usize, exposed_length)
    });
    closure.with_mut_ptr(|p: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(p as i64)
    })
}

fn attach_stream_static(
    constructor: &crate::gc::RuntimeHandle<'_>,
    method: &str,
    kind: f64,
    exposed_length: u32,
) {
    let value = stream_static_method_value(method, kind, exposed_length);
    crate::closure::closure_set_dynamic_prop(
        closure_addr_of(constructor.get_nanbox_f64()),
        method,
        value,
    );
}

fn attach_stream_constructor_statics(constructor_value: f64, name: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor_value);

    match name {
        "Readable" => {
            attach_stream_static(&constructor, "from", STREAM_STATIC_READABLE_FROM, 2);
            attach_stream_static(&constructor, "fromWeb", STREAM_STATIC_READABLE_FROM_WEB, 2);
            attach_stream_static(&constructor, "toWeb", STREAM_STATIC_READABLE_TO_WEB, 2);
        }
        "Writable" => {
            attach_stream_static(&constructor, "fromWeb", STREAM_STATIC_WRITABLE_FROM_WEB, 2);
            attach_stream_static(&constructor, "toWeb", STREAM_STATIC_WRITABLE_TO_WEB, 1);
        }
        "Duplex" => {
            attach_stream_static(&constructor, "from", STREAM_STATIC_DUPLEX_FROM, 1);
            attach_stream_static(&constructor, "fromWeb", STREAM_STATIC_DUPLEX_FROM_WEB, 2);
            attach_stream_static(&constructor, "toWeb", STREAM_STATIC_DUPLEX_TO_WEB, 2);
        }
        _ => {}
    }
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
        // `Stream.prototype`'s [[Prototype]] is `EventEmitter.prototype` (its
        // layout's), which `instanceof` walks.
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
            crate::closure::js_native_call_value(
                readable_to_web,
                crate::closure::plain_call_receiver(),
                [node_readable].as_ptr(),
                1,
            )
        };
        assert_object_property_function(web_readable, b"getReader");

        let node_writable = crate::node_stream::js_node_stream_writable_new(f64::from_bits(
            crate::value::TAG_UNDEFINED,
        ));
        let writable_to_web = static_method_value(writable, "toWeb");
        let web_writable = unsafe {
            crate::closure::js_native_call_value(
                writable_to_web,
                crate::closure::plain_call_receiver(),
                [node_writable].as_ptr(),
                1,
            )
        };
        assert_object_property_function(web_writable, b"getWriter");

        let node_duplex = crate::node_stream::js_node_stream_duplex_new(f64::from_bits(
            crate::value::TAG_UNDEFINED,
        ));
        let duplex_to_web = static_method_value(duplex, "toWeb");
        let web_pair = unsafe {
            crate::closure::js_native_call_value(
                duplex_to_web,
                crate::closure::plain_call_receiver(),
                [node_duplex].as_ptr(),
                1,
            )
        };
        let web_pair_readable = unsafe { property(web_pair, b"readable") };
        let web_pair_writable = unsafe { property(web_pair, b"writable") };
        assert_object_property_function(web_pair_readable, b"getReader");
        assert_object_property_function(web_pair_writable, b"getWriter");
    }
}
