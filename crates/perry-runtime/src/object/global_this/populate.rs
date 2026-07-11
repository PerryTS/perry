use super::super::*;
use super::*;

/// Populate the freshly-allocated globalThis singleton with built-in
/// constructor / namespace properties. Called exactly once from the CAS
/// winner in `js_get_global_this`. Constructors get a ClosureHeader-
/// backed value so `typeof globalThis.Array === "function"`; namespaces
/// (`Math`, `JSON`, `Reflect`) get a plain ObjectHeader (`typeof ===
/// "object"`). Both shapes carry a `prototype` dynamic property pointing
/// at an empty object so `<Builtin>.prototype` reads return a real
/// pointer instead of undefined, which is what unblocks lodash's
/// `var arrayProto = Array.prototype` chained read inside
/// `runInContext`.
pub(crate) fn populate_global_this_builtins(singleton: *mut ObjectHeader) {
    if singleton.is_null() {
        return;
    }
    let proto_key_bytes = b"prototype";
    let proto_key =
        crate::string::js_string_from_bytes(proto_key_bytes.as_ptr(), proto_key_bytes.len() as u32);
    {
        let name = b"globalThis";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = crate::value::js_nanbox_pointer(singleton as i64);
        js_object_set_field_by_name(singleton, key, value);
    }
    {
        // #4511: Node exposes the global object as `global` too
        // (`global === globalThis`). Install the same self-reference so bare
        // `global` / `(global as any).x` reads resolve to the real singleton
        // instead of the unknown-identifier sentinel.
        let name = b"global";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = crate::value::js_nanbox_pointer(singleton as i64);
        js_object_set_field_by_name(singleton, key, value);
        super::super::set_builtin_property_attrs(
            singleton as usize,
            "global".to_string(),
            super::super::PropertyAttrs::new(true, true, true),
        );
    }
    // #2145: pre-allocate the shared `%TypedArray%` intrinsic so per-kind
    // typed-array constructors can link their `__proto__` to it as they're
    // built below, and the per-kind `.prototype` objects can be flagged with
    // `OBJ_FLAG_TYPED_ARRAY_PROTO` for `Object.getPrototypeOf` resolution.
    let (typed_array_intrinsic_ctor, _) = ensure_typed_array_intrinsic();
    // #3664: build the generator / async-generator intrinsic prototype towers
    // so `Object.getPrototypeOf(function*(){})`, `g.constructor`, and the
    // `%Generator(.prototype)%` chains resolve to real objects.
    ensure_generator_intrinsics();
    // Constructors: ClosureHeader-backed so typeof is "function".
    // #4533: native error subclasses must link to `Error` / `Error.prototype`.
    // `Error` is listed before its subclasses in GLOBAL_THIS_BUILTIN_CONSTRUCTORS,
    // so these are populated before the subclass iterations consume them.
    let mut error_ctor_bits: Option<u64> = None;
    let mut error_proto_bits: Option<u64> = None;
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        if name == "Buffer" {
            let name_bytes = name.as_bytes();
            let name_key =
                crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            let ctor_value = super::super::native_module::buffer_constructor_value();
            js_object_set_field_by_name(singleton, name_key, ctor_value);
            super::super::set_builtin_property_attrs(
                singleton as usize,
                name.to_string(),
                super::super::PropertyAttrs::new(true, false, true),
            );
            continue;
        }
        let func_ptr = match name {
            "Array" => global_this_array_thunk as *const u8,
            "Object" => global_this_object_thunk as *const u8,
            "String" => global_this_string_thunk as *const u8,
            // #2889: call-form `Number(x)` / `Boolean(x)` through a rebound
            // global value coerce like the bare-call lowering does.
            "Number" => global_this_number_thunk as *const u8,
            "Boolean" => global_this_boolean_thunk as *const u8,
            "Error" => error_constructor_call_thunk as *const u8,
            "TypeError" => type_error_constructor_call_thunk as *const u8,
            "RangeError" => range_error_constructor_call_thunk as *const u8,
            "ReferenceError" => reference_error_constructor_call_thunk as *const u8,
            "SyntaxError" => syntax_error_constructor_call_thunk as *const u8,
            "EvalError" => eval_error_constructor_call_thunk as *const u8,
            "URIError" => uri_error_constructor_call_thunk as *const u8,
            "MessageChannel" => {
                crate::messaging::js_message_channel_constructor_call_error as *const u8
            }
            "MessagePort" => crate::messaging::js_message_port_constructor_call_error as *const u8,
            "BroadcastChannel" => {
                crate::messaging::js_broadcast_channel_constructor_call_error as *const u8
            }
            "Date" => global_this_date_thunk as *const u8,
            "Blob" => global_this_blob_thunk as *const u8,
            "File" => global_this_file_thunk as *const u8,
            "Headers" => global_this_headers_thunk as *const u8,
            "Request" => global_this_request_thunk as *const u8,
            "Response" => global_this_response_thunk as *const u8,
            "URLPattern" => global_this_url_pattern_call_thunk as *const u8,
            "Storage" => crate::web_storage::storage_constructor_illegal as *const u8,
            "Crypto" | "CryptoKey" | "SubtleCrypto" => {
                webcrypto_illegal_constructor_thunk as *const u8
            }
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => typed_array_constructor_call_thunk as *const u8,
            // #4569: collection constructors throw when called without `new`.
            "Map" => map_constructor_call_thunk as *const u8,
            "Set" => set_constructor_call_thunk as *const u8,
            "WeakMap" => weak_map_constructor_call_thunk as *const u8,
            "WeakSet" => weak_set_constructor_call_thunk as *const u8,
            "WeakRef" => weak_ref_constructor_call_thunk as *const u8,
            "Promise" => promise_constructor_call_thunk as *const u8,
            _ => global_this_builtin_noop_thunk as *const u8,
        };
        let closure_ptr = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure_ptr.is_null() {
            continue;
        }
        match name {
            "Array" => {
                crate::closure::js_register_closure_rest(func_ptr, 0);
            }
            "Date" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "Object" | "String" | "Number" | "Boolean" | "BroadcastChannel" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "Headers" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "Blob" | "Request" | "Response" => {
                crate::closure::js_register_closure_arity(func_ptr, 2);
            }
            "File" => {
                crate::closure::js_register_closure_arity(func_ptr, 3);
            }
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "EvalError" | "URIError" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "MessageChannel" | "MessagePort" | "Storage" => {
                crate::closure::js_register_closure_arity(func_ptr, 0);
            }
            "URLPattern" => {
                crate::closure::js_register_closure_arity(func_ptr, 2);
            }
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => {
                crate::closure::js_register_closure_arity(func_ptr, 0);
            }
            _ => {}
        }
        // #2889: install static methods (`Object.keys`, `Array.isArray`, ...)
        // on the constructor closure so rebound usage like
        // `const O = Object; O.keys(x)` dispatches through the real helpers.
        install_builtin_constructor_statics(name, closure_ptr);
        if name == "Number" {
            install_number_static_data_properties(closure_ptr);
        }
        // #3655: every constructor carries spec-correct own `name`/`length`
        // data properties (`{ writable:false, enumerable:false,
        // configurable:true }`). The shared no-op thunk can't carry a name via
        // the func-ptr registry (every constructor would read the same one),
        // so record both per-closure. Without this, a rebound constructor read
        // `Date.name === ""` / `Date.length === 0` and test262's
        // `verifyProperty(Ctor, 'name'|'length', …)` failed "should be an own
        // property".
        super::super::native_module::set_bound_native_closure_name(closure_ptr, name);
        if let Some(len) = builtin_constructor_spec_length(name) {
            super::super::native_module::set_builtin_closure_length(closure_ptr as usize, len);
        }
        super::super::set_builtin_property_attrs(
            closure_ptr as usize,
            "name".to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
        super::super::set_builtin_property_attrs(
            closure_ptr as usize,
            "length".to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
        if name == "Error" {
            install_error_static_methods(closure_ptr);
        }
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        // #4533: `Object.getPrototypeOf(TypeError) === Error`. The constructor's
        // `[[Prototype]]` is `Error` itself (not `Function.prototype`).
        if name == "Error" {
            error_ctor_bits = Some(ctor_value.to_bits());
        } else if is_native_error_subclass_constructor(name) {
            if let Some(proto_bits) = error_ctor_bits {
                crate::closure::closure_set_static_prototype(closure_ptr as usize, proto_bits);
            }
        }
        // Stash `prototype` on the closure's dynamic-prop side table.
        // `js_object_set_field_by_name` detects the CLOSURE_MAGIC tag
        // at offset 12 and dispatches into `closure_set_dynamic_prop`
        // for us; both reads and writes share that side table.
        let proto_obj = if name == "Array" {
            crate::array::js_array_alloc(0) as *mut ObjectHeader
        } else {
            js_object_alloc(0, 0)
        };
        if !proto_obj.is_null() {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            js_object_set_field_by_name(closure_ptr as *mut ObjectHeader, proto_key, proto_value);
            super::super::set_builtin_property_attrs(
                closure_ptr as usize,
                "prototype".to_string(),
                super::super::PropertyAttrs::new(false, false, false),
            );
            let ctor_key = crate::string::js_string_from_bytes(
                b"constructor".as_ptr(),
                "constructor".len() as u32,
            );
            js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
            super::super::set_builtin_property_attrs(
                proto_obj as usize,
                "constructor".to_string(),
                super::super::PropertyAttrs::new(true, false, true),
            );
            if is_web_fetch_constructor(name) {
                js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
                super::super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "constructor".to_string(),
                    super::super::PropertyAttrs::new(true, false, true),
                );
            }
            if name == "Array" {
                let constructor_key =
                    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                js_object_set_field_by_name(proto_obj, constructor_key, ctor_value);
                super::super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "constructor".to_string(),
                    super::super::PropertyAttrs::new(true, false, true),
                );
            }
            if matches!(
                name,
                "Navigator"
                    | "TextEncoderStream"
                    | "TextDecoderStream"
                    | "CompressionStream"
                    | "DecompressionStream"
            ) {
                let constructor_key =
                    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                js_object_set_field_by_name(proto_obj, constructor_key, ctor_value);
            }
            // Populate well-known method properties on the prototype
            // (currently just `Array.prototype.slice`). Methods are
            // ClosureHeader-backed thunks that read their receiver from
            // `IMPLICIT_THIS` and dispatch to the corresponding native
            // entry point — works in tandem with `.call`/`.apply` since
            // those arms (#970) rebind IMPLICIT_THIS before forwarding.
            populate_builtin_prototype_methods(name, proto_obj);
            install_error_prototype_data_properties(name, proto_obj);
            // ECMA-262 20.5.6.3: the [[Prototype]] of each NativeError prototype
            // object is %Error.prototype% (not %Object.prototype%). `Error` is
            // listed before its subclasses, so its prototype object is stashed
            // here and linked into each subclass prototype's chain. Without this
            // `Object.getPrototypeOf(TypeError.prototype) !== Error.prototype`
            // (test262 NativeErrors/*/prototype/proto.js).
            if name == "Error" {
                error_proto_bits =
                    Some(crate::value::js_nanbox_pointer(proto_obj as i64).to_bits());
            } else if is_native_error_subclass_constructor(name) {
                if let Some(proto_bits) = error_proto_bits {
                    super::super::prototype_chain::object_set_static_prototype(
                        proto_obj as usize,
                        proto_bits,
                    );
                }
            }
            if matches!(name, "MessageChannel" | "MessagePort" | "BroadcastChannel") {
                crate::messaging::populate_messaging_prototype(name, proto_obj, ctor_value);
            }
            if name == "Storage" {
                crate::web_storage::install_storage_globals(
                    singleton,
                    closure_ptr,
                    proto_obj,
                    ctor_value,
                );
            }
            if matches!(name, "Crypto" | "CryptoKey" | "SubtleCrypto") {
                super::super::native_module::install_webcrypto_constructor_proto(
                    proto_obj, ctor_value,
                );
            }
            if name == "WebSocket" {
                websocket_global::install_constructor_shape(closure_ptr, proto_obj);
            }
            // #2145: link per-kind typed-array constructors into the
            // `%TypedArray%` chain. `Int8Array.__proto__ === %TypedArray%`
            // and `Object.getPrototypeOf(Int8Array.prototype) ===
            // %TypedArray%.prototype`. Both reads are resolved off this
            // wiring (closure static-prototype side-table for the ctor;
            // `OBJ_FLAG_TYPED_ARRAY_PROTO` + the cached
            // `TYPED_ARRAY_INTRINSIC_PROTO_PTR` for the per-kind proto).
            if !typed_array_intrinsic_ctor.is_null()
                && matches!(
                    name,
                    "Int8Array"
                        | "Uint8Array"
                        | "Uint8ClampedArray"
                        | "Int16Array"
                        | "Uint16Array"
                        | "Int32Array"
                        | "Uint32Array"
                        | "Float16Array"
                        | "Float32Array"
                        | "Float64Array"
                        | "BigInt64Array"
                        | "BigUint64Array"
                )
            {
                let intrinsic_bits =
                    crate::value::js_nanbox_pointer(typed_array_intrinsic_ctor as i64).to_bits();
                crate::closure::closure_set_static_prototype(closure_ptr as usize, intrinsic_bits);
                unsafe {
                    let gc = (proto_obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE)
                        as *mut crate::gc::GcHeader;
                    (*gc)._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
                }
                // Record the per-kind proto's `[[Prototype]]` as the shared
                // `%TypedArray%.prototype` so the ordinary property-get chain
                // walk (`resolve_inherited_field`) finds the inherited methods
                // (`map`, `filter`, `toString`, …) that no longer live on the
                // per-kind proto as own properties. `Object.getPrototypeOf`
                // already resolves via the flag above; this link drives value
                // reads like `Int8Array.prototype.map`.
                let intrinsic_proto =
                    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
                if intrinsic_proto != 0 {
                    let proto_bits = crate::value::js_nanbox_pointer(intrinsic_proto).to_bits();
                    super::super::prototype_chain::object_set_static_prototype(
                        proto_obj as usize,
                        proto_bits,
                    );
                }
            }
            // #4140: per-kind `BYTES_PER_ELEMENT` own data property on BOTH the
            // constructor and its prototype, matching Node's descriptor
            // `{ value, writable:false, enumerable:false, configurable:false }`.
            // The bare `Uint8Array.BYTES_PER_ELEMENT` read folds at compile time
            // (#2902), but the reflective forms — `getOwnPropertyDescriptor`,
            // `hasOwnProperty`, and the chained `Float64Array.prototype
            // .BYTES_PER_ELEMENT` — resolve off these installed own properties.
            let ta_bytes_per_element = match name {
                "Int8Array" | "Uint8Array" | "Uint8ClampedArray" => Some(1.0),
                "Int16Array" | "Uint16Array" | "Float16Array" => Some(2.0),
                "Int32Array" | "Uint32Array" | "Float32Array" => Some(4.0),
                "Float64Array" | "BigInt64Array" | "BigUint64Array" => Some(8.0),
                _ => None,
            };
            if let Some(bytes) = ta_bytes_per_element {
                let bpe_attrs = super::super::PropertyAttrs::new(false, false, false);
                for target in [closure_ptr as *mut ObjectHeader, proto_obj] {
                    let bpe_key = crate::string::js_string_from_bytes(
                        b"BYTES_PER_ELEMENT".as_ptr(),
                        b"BYTES_PER_ELEMENT".len() as u32,
                    );
                    js_object_set_field_by_name(target, bpe_key, bytes);
                    super::super::set_builtin_property_attrs(
                        target as usize,
                        "BYTES_PER_ELEMENT".to_string(),
                        bpe_attrs,
                    );
                }
            }
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        js_object_set_field_by_name(singleton, name_key, ctor_value);
        super::super::set_builtin_property_attrs(
            singleton as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    // Callable global functions: ClosureHeader-backed values with real
    // dispatch so direct property reads and rebound calls match bare calls.
    for name in GLOBAL_THIS_BUILTIN_FUNCTIONS.iter().copied() {
        let (func_ptr, arity, has_rest, enumerable) = match name {
            "eval" => (global_this_eval_thunk as *const u8, 1, false, false),
            "fetch" => (
                super::super::global_fetch::global_this_fetch_thunk as *const u8,
                1,
                true,
                true,
            ),
            "structuredClone" => (
                global_this_structured_clone_thunk as *const u8,
                2,
                false,
                true,
            ),
            "atob" => (global_this_atob_thunk as *const u8, 1, false, true),
            "btoa" => (global_this_btoa_thunk as *const u8, 1, false, true),
            "setTimeout" => (global_this_set_timeout_thunk as *const u8, 2, true, true),
            "clearTimeout" => (global_this_clear_timeout_thunk as *const u8, 1, false, true),
            "setInterval" => (global_this_set_interval_thunk as *const u8, 2, true, true),
            "clearInterval" => (
                global_this_clear_interval_thunk as *const u8,
                1,
                false,
                true,
            ),
            "setImmediate" => (global_this_set_immediate_thunk as *const u8, 1, true, true),
            "clearImmediate" => (
                global_this_clear_immediate_thunk as *const u8,
                1,
                false,
                true,
            ),
            "queueMicrotask" => (
                global_this_queue_microtask_thunk as *const u8,
                1,
                false,
                true,
            ),
            // `gc([force])` — value form of the bare `gc()` call-intrinsic.
            // Non-enumerable (a debug/diagnostic global). The optional `force`
            // arg is accepted (arity 1) but ignored — Perry's gc is full.
            "gc" => (global_this_gc_thunk as *const u8, 1, false, false),
            // #2905: standard global helper functions.
            "parseInt" => (global_this_parse_int_thunk as *const u8, 2, false, false),
            "parseFloat" => (global_this_parse_float_thunk as *const u8, 1, false, false),
            "isNaN" => (global_this_is_nan_thunk as *const u8, 1, false, false),
            "isFinite" => (global_this_is_finite_thunk as *const u8, 1, false, false),
            "encodeURI" => (global_this_encode_uri_thunk as *const u8, 1, false, false),
            "decodeURI" => (global_this_decode_uri_thunk as *const u8, 1, false, false),
            "encodeURIComponent" => (
                global_this_encode_uri_component_thunk as *const u8,
                1,
                false,
                false,
            ),
            "decodeURIComponent" => (
                global_this_decode_uri_component_thunk as *const u8,
                1,
                false,
                false,
            ),
            // #4511: legacy escape/unescape (ES Annex B).
            // #4511: legacy escape/unescape (ES Annex B).
            "escape" => (global_this_escape_thunk as *const u8, 1, false, false),
            "unescape" => (global_this_unescape_thunk as *const u8, 1, false, false),
            _ => continue,
        };
        let closure_ptr = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure_ptr.is_null() {
            continue;
        }
        if has_rest {
            crate::closure::js_register_closure_rest(func_ptr, arity);
        } else {
            crate::closure::js_register_closure_arity(func_ptr, arity);
        }
        unsafe {
            crate::builtins::js_register_function_name(func_ptr, name.as_ptr(), name.len() as u32);
        }
        super::super::native_module::set_builtin_closure_length(closure_ptr as usize, arity);
        // Every global helper installed here (parseInt/parseFloat/isNaN/
        // isFinite/{en,de}codeURI{,Component}/escape/unescape/setTimeout/…) is a
        // built-in *non-constructor* function: per spec it has no `.prototype`
        // (reads back `undefined`) and `new fn()` throws a TypeError. Mark the
        // closure so the `new`/`.prototype` paths honor that — otherwise these
        // functions defaulted to an ordinary `.prototype` object and silently
        // accepted `new` (test262 built-ins/{decodeURI,isNaN,parseFloat,…}
        // */A5.6/A5.7/A2.6/A2.7/A7.6/A7.7).
        super::super::native_module::set_builtin_closure_non_constructable(closure_ptr as usize);
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let fn_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        js_object_set_field_by_name(singleton, name_key, fn_value);
        super::super::set_builtin_property_attrs(
            singleton as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(true, enumerable, true),
        );
    }
    // ECMA-262 21.1.2.12 / 21.1.2.13: `Number.parseFloat` and `Number.parseInt`
    // are the SAME function objects as the global `parseFloat` / `parseInt`
    // (`Number.parseFloat === parseFloat`). The Number constructor statics were
    // installed above with fresh thunks — before the global helpers existed —
    // so re-point them now at the global closures we just created on the
    // singleton. A value-read of `Number.parseFloat` resolves to the Number
    // constructor's own `parseFloat` field (see expr_member.rs reroute-undo),
    // which now holds the identical closure the bare `parseFloat` resolves to.
    alias_number_static_to_global_function(singleton, "parseFloat");
    alias_number_static_to_global_function(singleton, "parseInt");
    // Namespaces: plain ObjectHeader so typeof is "object" per spec.
    for name in GLOBAL_THIS_BUILTIN_NAMESPACES.iter().copied() {
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let ns_value = if matches!(name, "console" | "process") {
            // #6230: install the module's runtime dispatch bucket so dynamic
            // method calls on the namespace *value* resolve — `const p = process;
            // p.exit(1)`, `process["exit"](1)`, `p.cwd()`, dynamic `console.log`.
            // These globals are never `import`ed, so nothing else emits the
            // `js_nm_install_<module>()` call; `import`ed modules get theirs from
            // the import. Reached only when the globalThis singleton is
            // materialized — i.e. exactly when process/console are used as values.
            match name {
                "process" => crate::object::native_module_registry::js_nm_install_process(),
                "console" => crate::object::native_module_registry::js_nm_install_console(),
                _ => {}
            }
            js_create_native_module_namespace(name_bytes.as_ptr(), name_bytes.len())
        } else if name == "WebAssembly" {
            super::global_this_webassembly::create_webassembly_namespace()
        } else {
            let ns_obj = js_object_alloc(0, 0);
            if ns_obj.is_null() {
                continue;
            }
            // #4139 + #4149: reify each namespace's own members as real
            // properties so the reflection APIs (`getOwnPropertyDescriptor`,
            // `getOwnPropertyNames`) observe them. Call sites (`Math.max(...)`,
            // `JSON.stringify(...)`, `Reflect.get(...)`) are codegen intrinsics
            // gated on the AST shape and never read these fields. Math uses the
            // richer install that also exposes per-method name/length descriptors.
            match name {
                "Math" => {
                    install_math_namespace(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Math");
                }
                "JSON" => {
                    install_json_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "JSON");
                }
                "Reflect" => {
                    install_reflect_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Reflect");
                }
                "Atomics" => {
                    install_atomics_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Atomics");
                }
                "Intl" => crate::intl::install_intl_namespace(ns_obj),
                #[cfg(feature = "temporal")]
                "Temporal" => {
                    install_temporal_namespace(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Temporal");
                }
                _ => {}
            }
            crate::value::js_nanbox_pointer(ns_obj as i64)
        };
        js_object_set_field_by_name(singleton, name_key, ns_value);
        super::super::set_builtin_property_attrs(
            singleton as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    // node:perf_hooks `performance` global — bind it to the same singleton the
    // named import resolves to, so `globalThis.performance ===
    // require("perf_hooks").performance` (#1327). typeof stays "object".
    {
        let pname = b"performance";
        let pkey = crate::string::js_string_from_bytes(pname.as_ptr(), pname.len() as u32);
        let pval = crate::perf_hooks::performance_namespace();
        js_object_set_field_by_name(singleton, pkey, pval);
    }
    // Perf_hooks constructors are globals identical to the module exports.
    for name in [
        "Performance",
        "PerformanceEntry",
        "PerformanceMark",
        "PerformanceMeasure",
        "PerformanceObserver",
        "PerformanceObserverEntryList",
        "PerformanceResourceTiming",
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value =
            super::super::native_module::bound_native_callable_export_value("perf_hooks", name);
        js_object_set_field_by_name(singleton, key, value);
    }
    super::super::native_module::install_global_webcrypto(singleton);
    let func_ptr = global_this_crypto_getter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let getter = crate::closure::js_closure_alloc(func_ptr, 0);
    let getter_bits = if getter.is_null() {
        0
    } else {
        crate::value::js_nanbox_pointer(getter as i64).to_bits()
    };
    super::super::set_builtin_accessor_descriptor(
        singleton as usize,
        "crypto".to_string(),
        super::super::AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        super::super::PropertyAttrs::new(true, true, true),
    );
    // #2923: `globalThis.navigator` — Node's browser-compatible runtime
    // metadata object. typeof is "object". Built once per process.
    {
        let nname = b"navigator";
        let nkey = crate::string::js_string_from_bytes(nname.as_ptr(), nname.len() as u32);
        // Read the `Navigator` constructor we installed on the singleton above
        // and hand it to the navigator builder directly. We must NOT call
        // `js_navigator_object()` here: it re-fetches the constructor via
        // `js_get_global_this_builtin_value` → `js_get_global_this`, which would
        // re-enter this very lazy-init (GLOBAL_THIS_READY is still false until we
        // return) and recurse/spin forever.
        let nav_ctor_key = crate::string::js_string_from_bytes(b"Navigator".as_ptr(), 9);
        let nav_ctor = js_object_get_field_by_name(singleton, nav_ctor_key);
        let nval =
            crate::navigator::navigator_object_with_constructor(f64::from_bits(nav_ctor.bits()));
        js_object_set_field_by_name(singleton, nkey, nval);
    }
    // ECMA-262 19.1/19.2/19.3: NaN, Infinity, and undefined are own data
    // properties of the global object with {writable:false, enumerable:false,
    // configurable:false}.  Install them so that
    // `Object.getOwnPropertyDescriptor(globalThis, "NaN")` returns a real
    // descriptor (test262 15.2.3.3-4-178/179/180) and
    // `Object.getOwnPropertyNames(globalThis)` includes them (15.2.3.4-4-1).
    {
        let non_writable = super::super::PropertyAttrs::new(false, false, false);
        for (name, value) in [("NaN", f64::NAN), ("Infinity", f64::INFINITY)] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(singleton, key, value);
            super::super::set_builtin_property_attrs(
                singleton as usize,
                name.to_string(),
                non_writable,
            );
        }
        let undef_key = crate::string::js_string_from_bytes(b"undefined".as_ptr(), 9);
        let undef_val = f64::from_bits(crate::value::TAG_UNDEFINED);
        js_object_set_field_by_name(singleton, undef_key, undef_val);
        super::super::set_builtin_property_attrs(
            singleton as usize,
            "undefined".to_string(),
            super::super::PropertyAttrs::new(false, false, false),
        );
    }
    // ECMA-262 §23.2.3.33: `%TypedArray%.prototype.toString` must be the
    // same function object as `Array.prototype.toString`. Alias it now that
    // both the Array constructor and the TypedArray intrinsic are set up.
    alias_typed_array_proto_to_string(singleton);
}

/// Install `%TypedArray%.prototype.toString` as the same closure object as
/// `Array.prototype.toString` (ECMA-262 §23.2.3.33).
fn alias_typed_array_proto_to_string(singleton: *mut ObjectHeader) {
    let ta_proto_addr = crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
    if ta_proto_addr == 0 {
        return;
    }
    let ta_proto = ta_proto_addr as *mut ObjectHeader;
    // Read Array constructor from globalThis, then Array.prototype.toString.
    let arr_key = crate::string::js_string_from_bytes(b"Array".as_ptr(), 5);
    let arr_ctor = js_object_get_field_by_name(singleton, arr_key);
    if (arr_ctor.bits() >> 48) != 0x7FFD {
        return;
    }
    let arr_ctor_ptr = (arr_ctor.bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if arr_ctor_ptr.is_null() {
        return;
    }
    let proto_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let arr_proto = js_object_get_field_by_name(arr_ctor_ptr, proto_key);
    if (arr_proto.bits() >> 48) != 0x7FFD {
        return;
    }
    let arr_proto_ptr = (arr_proto.bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if arr_proto_ptr.is_null() {
        return;
    }
    let ts_key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    let to_string_fn = js_object_get_field_by_name(arr_proto_ptr, ts_key);
    if to_string_fn.bits() == crate::value::TAG_UNDEFINED {
        return;
    }
    let ts_key2 = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    js_object_set_field_by_name(ta_proto, ts_key2, f64::from_bits(to_string_fn.bits()));
    super::super::set_builtin_property_attrs(
        ta_proto as usize,
        "toString".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

/// Re-point a `Number.<name>` static at the global function of the same name so
/// the two are the identical object (`Number.parseFloat === parseFloat`). Both
/// the global helper and the `Number` constructor are already installed on the
/// `singleton` by the time this runs. No-op if either lookup fails.
fn alias_number_static_to_global_function(singleton: *mut ObjectHeader, name: &str) {
    let global_key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let global_fn = js_object_get_field_by_name(singleton, global_key);
    if (global_fn.bits() >> 48) != 0x7FFD {
        return;
    }
    let number_key = crate::string::js_string_from_bytes(b"Number".as_ptr(), 6);
    let number_ctor = js_object_get_field_by_name(singleton, number_key);
    if (number_ctor.bits() >> 48) != 0x7FFD {
        return;
    }
    let ctor_ptr = (number_ctor.bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if ctor_ptr.is_null() {
        return;
    }
    let static_key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(ctor_ptr, static_key, f64::from_bits(global_fn.bits()));
    super::super::set_builtin_property_attrs(
        ctor_ptr as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

thread_local! {
    /// Raw address of THIS thread's `Error` constructor closure, captured at
    /// install. Read by `error::error_prepare_stack_trace_override` so
    /// `captureStackTrace` / `error.stack` can honor a user-set
    /// `Error.prepareStackTrace`. Thread-local, not a process-global: each
    /// `perry/thread` agent has its own arena + realm, and an `Error`
    /// constructor / `prepareStackTrace` from another thread's arena can be a
    /// foreign or freed pointer — the same reason `globalThis` is per-thread.
    pub(crate) static ERROR_CONSTRUCTOR_PTR: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// The default `Error.prepareStackTrace` thunk's address — used to tell a
/// user override apart from Perry's built-in default.
pub(crate) fn default_prepare_stack_trace_func_ptr() -> usize {
    global_this_error_prepare_stack_trace_thunk as *const u8 as usize
}

fn install_error_static_methods(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    ERROR_CONSTRUCTOR_PTR.with(|c| c.set(ctor as usize));
    let func_ptr = global_this_error_capture_stack_trace_thunk as *const u8;
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 2);
    super::super::native_module::set_bound_native_closure_name(closure, "captureStackTrace");

    let key = crate::string::js_string_from_bytes(b"captureStackTrace".as_ptr(), 17);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::super::set_builtin_property_attrs(
        ctor as usize,
        "captureStackTrace".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    // #2904: `Error.isError` — V8/Node Error duck-check.
    install_error_static_fn(
        ctor,
        "isError",
        global_this_error_is_error_thunk as *const u8,
        1,
    );

    // #2904: `Error.prepareStackTrace` — default stack-formatting hook.
    install_error_static_fn(
        ctor,
        "prepareStackTrace",
        global_this_error_prepare_stack_trace_thunk as *const u8,
        2,
    );

    // #2904: `Error.stackTraceLimit` — writable number controlling captured
    // frame count. Node's default is 10; Perry's stacks are coarse but the
    // property must read as a number and be writable.
    let limit_key = crate::string::js_string_from_bytes(b"stackTraceLimit".as_ptr(), 15);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, limit_key, 10.0);
    super::super::set_builtin_property_attrs(
        ctor as usize,
        "stackTraceLimit".to_string(),
        super::super::PropertyAttrs::new(true, true, true),
    );
}

/// #2904: install a callable static method on the `Error` constructor closure
/// as a non-enumerable, writable, configurable data property (matching Node's
/// property descriptors for the V8 static helpers).
fn install_error_static_fn(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::super::native_module::set_bound_native_closure_name(closure, name);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::super::set_builtin_property_attrs(
        ctor as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

// =====================================================================
// #2889: static methods on rebound global built-in constructor values.
//
// `const O = Object; O.keys(x)` reads `keys` off the `Object` constructor
// closure's dynamic-prop side table, then calls it. Pre-fix nothing was
// installed there, so the read returned `undefined`. These thunks delegate
// to the same runtime helpers the direct `Object.keys(x)` lowering uses.
// =====================================================================
