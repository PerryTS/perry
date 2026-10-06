use super::callable_export_arity_table::native_callable_export_arity;
use super::*;
mod buffer_constructor;
mod buffer_species;
pub(crate) use buffer_constructor::{
    buffer_constructor_value, buffer_intrinsic_prototype_parent, buffer_intrinsic_prototype_value,
    buffer_original_prototype_value, cached_buffer_intrinsic_prototype_value,
    is_buffer_constructor_value,
};
mod builtin_closure_metadata;
mod module_cjs;
pub(crate) use builtin_closure_metadata::*;
use module_cjs::attach_module_cjs_constructor_statics;
pub(crate) use module_cjs::{
    module_builtin_modules_value, module_cjs_cache_value, module_cjs_extensions_value,
    module_cjs_global_paths_value, module_cjs_path_cache_value, module_cjs_prototype_for_instance,
    module_constants_value,
};

#[cfg(test)]
thread_local! {
    static TEST_COLLECT_NATIVE_EXPORT_AFTER_ALLOC: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn test_collect_native_export_after_alloc() {
    TEST_COLLECT_NATIVE_EXPORT_AFTER_ALLOC.with(|armed| armed.set(true));
}

/// The bound export already minted under its exact cache key
/// (`"<module>\0<property>"`, canonical names only), without minting one or
/// formatting a key: `None` when it has not been minted yet.
pub(crate) fn minted_native_callable_export(canonical_key: &str) -> Option<f64> {
    NATIVE_CALLABLE_EXPORTS
        .with(|c| c.borrow().get(canonical_key).copied())
        .map(f64::from_bits)
}

pub fn bound_native_callable_export_value(module_name: &str, property_name: &str) -> f64 {
    // Bound-native closures carry (module, method) metadata that the
    // generic property/call paths resolve through the vtable — and they
    // can be minted via the codegen NativeModuleRef fast path without any
    // namespace object existing. Install here too.
    install_native_module_vtable();
    let module_name = if module_name == "wasi.default" {
        "wasi"
    } else {
        cjs_default_base_module(module_name).unwrap_or(module_name)
    };
    let module_name = assert_instance_base_module(module_name).unwrap_or(module_name);
    let property_name = canonical_native_callable_property(module_name, property_name);
    // Codegen can request a callable directly, bypassing namespace lookup.
    #[cfg(windows)]
    if module_name == "process"
        && matches!(
            property_name,
            "getuid"
                | "geteuid"
                | "getgid"
                | "getegid"
                | "getgroups"
                | "setuid"
                | "seteuid"
                | "setgid"
                | "setegid"
                | "setgroups"
                | "initgroups"
        )
    {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // node:inspector/promises is the callback namespace with Session replaced.
    if module_name == "inspector/promises" && property_name != "Session" {
        return bound_native_callable_export_value("inspector", property_name);
    }
    let export_module_name = if property_name == "Assert" && module_name == "assert/strict" {
        "assert"
    } else {
        module_name
    };
    let callable_module_name = if export_module_name == "util.types" {
        "util/types"
    } else {
        export_module_name
    };
    // `node:module`'s attach handler (Module statics, SourceMap prototype) is
    // registered by `js_nm_install_module()`, which codegen emits for every
    // import form — named, default, namespace and `require("module")` — and
    // the dynamic `require`/`getBuiltinModule` paths reach through the
    // install-all hook. Calling it from here as well made this always-live
    // hub the retainer of the whole `module` bucket (~130 KB in every
    // binary). The one runtime-internal path that reaches a `module` export
    // without an import, `MODULE_CJS` instance `.constructor`, is armed by the
    // instance constructor (`process::node_module::js_module_module_new`).
    let key = format!("{callable_module_name}\0{property_name}");
    if let Some(bits) = NATIVE_CALLABLE_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let method_bytes: &'static [u8] = property_name.as_bytes().to_vec().leak();
    let scope = crate::gc::RuntimeHandleScope::new();
    let ns = scope.root_nanbox_f64(js_create_native_module_namespace(
        callable_module_name.as_ptr(),
        callable_module_name.len(),
    ));
    let closure = crate::closure::js_closure_alloc(&crate::closure::BOUND_METHOD_INFO, 3);
    let closure = scope.root_raw_mut_ptr(closure);
    closure.with_mut_ptr(|c: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(c, 0, ns.get_nanbox_f64());
    });
    closure.with_mut_ptr(|c: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_ptr(c, 1, method_bytes.as_ptr() as i64);
    });
    closure.with_mut_ptr(|c: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_ptr(c, 2, method_bytes.len() as i64);
    });
    #[cfg(test)]
    TEST_COLLECT_NATIVE_EXPORT_AFTER_ALLOC.with(|armed| {
        if armed.replace(false) {
            crate::gc::gc_collect_minor();
        }
    });
    let exposed_name = if export_module_name == "fs" {
        native_callable_export_display_name(export_module_name, property_name)
    } else if export_module_name == "url" && property_name == "resolveObject" {
        "urlResolveObject"
    } else if export_module_name == "http" && property_name == "_connectionListener" {
        "connectionListener"
    } else if export_module_name == "module" && property_name == "runMain" {
        "executeUserEntryPoint"
    } else if export_module_name == "fs" && property_name == "_toUnixTimestamp" {
        "toUnixTimestamp"
    } else {
        property_name
    };
    closure.with_mut_ptr(|c: *mut crate::closure::ClosureHeader| {
        set_bound_native_closure_name(c, exposed_name);
    });
    if let Some(length) = native_callable_export_arity(export_module_name, property_name) {
        // Re-read: naming the closure allocates its name string, so the
        // address above can be stale for the address-keyed length table.
        closure.with_mut_ptr(|c: *mut crate::closure::ClosureHeader| {
            set_builtin_closure_length(c as usize, length);
        });
    }
    let value = scope.root_nanbox_f64(closure.with_mut_ptr(
        |c: *mut crate::closure::ClosureHeader| crate::value::js_nanbox_pointer(c as i64),
    ));

    // Publish the rooted constructor before resolving its children/parent.
    // Stream.Readable forms a cycle with its constructor parent;
    // recursive materialization must reuse this exact constructor shape.
    NATIVE_CALLABLE_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.get_nanbox_u64());
        crate::gc::runtime_write_barrier_root_nanbox(value.get_nanbox_u64());
    });

    // Per-module prototype/statics decoration, routed through the attach
    // registry (see `native_module_registry::nm_attach_lookup`): each
    // module's handler is registered by its `js_nm_install_<module>()`, and
    // this path is only reachable through that module's namespace — so a
    // binary links exactly the attach machinery of the modules it imports.
    if export_module_name == "perf_hooks" {
        // These constructors are also installed as globals during realm
        // bootstrap, before an explicit perf_hooks import necessarily arms the
        // optional module hook. Their prototypes are intrinsic constructor
        // state, so install them at this common callable-creation seam.
        unsafe {
            crate::perf_hooks::attach_perf_hooks_constructor(
                property_name,
                value.get_nanbox_f64(),
                crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize,
            );
        }
    } else if export_module_name == "bun" && property_name == "plugin" {
        // Decorate once at creation so named imports, namespaces, and saved
        // Bun.plugin values share the same callable and clearAll property.
        crate::bun_compat::decorate_bun_plugin(value.get_nanbox_f64());
    } else if let Some(attach) =
        super::super::native_module_registry::nm_attach_lookup(export_module_name)
    {
        // SAFETY: registry only ever holds the `nm_attach_*` handlers below.
        unsafe {
            attach(
                property_name,
                value.get_nanbox_f64(),
                crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize,
            )
        };
    }

    let value = value.get_nanbox_f64();

    value
}

/// #6692: install `stream.pipeline[util.promisify.custom]` (or `.finished`'s)
/// pointing at the promise-based `stream/promises` export, matching Node. With
/// the hook present, `promisify(stream.pipeline)` resolves through
/// `custom_promisified_value` to the promise implementation instead of the
/// generic wrapper (whose appended callback the `promisify.custom`-aware caller
/// in `pi`'s bundled node-fetch never provides). `property_name` is `"pipeline"`
/// or `"finished"` — the matching `stream/promises` export name.
///
/// Returns the (possibly relocated) receiver value: the allocations below can
/// trigger a GC that evacuates the closure, and only the `scope` handle tracks
/// the move, so the caller must adopt the returned pointer.
fn attach_stream_promisify_custom(pipeline_value: f64, property_name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(pipeline_value);
    let promise_impl = crate::node_submodules::stream_promises_export_callable(property_name);
    // The submodule may be unavailable (returns the `TAG_TRUE` sentinel); only
    // wire the hook when it resolved to a real callable closure, otherwise leave
    // the generic promisify fallback in place.
    let impl_bits = promise_impl.to_bits();
    let impl_addr = (impl_bits & crate::value::POINTER_MASK) as usize;
    if (impl_bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr(impl_addr)
    {
        let impl_handle = scope.root_nanbox_f64(promise_impl);
        let custom_symbol = crate::util_promisify::promisify_custom_symbol();
        let symbol_handle = scope.root_nanbox_f64(custom_symbol);
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                target.get_nanbox_f64(),
                symbol_handle.get_nanbox_f64(),
                impl_handle.get_nanbox_f64(),
            );
        }
    }
    target.get_nanbox_f64()
}

/// `info` is the rest body's (`with_rest(fixed)`); `.length` is `length`.
fn async_hooks_static_method_value(
    info: *const crate::closure::JsFunctionInfo,
    name: &str,
    length: u32,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let closure_handle = scope.root_raw_mut_ptr(closure);
    set_bound_native_closure_name(
        closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>(),
        name,
    );
    set_builtin_closure_length(
        closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        length,
    );
    set_builtin_closure_non_constructable(
        closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
    );
    crate::value::js_nanbox_pointer(
        closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    )
}

extern "C" fn fs_namespace_descriptor_getter_thunk(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    unsafe {
        let property_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const u8;
        let property_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        js_native_module_property_by_name(b"fs".as_ptr(), 2, property_ptr, property_len)
    }
}

extern "C" fn fs_namespace_descriptor_setter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _value: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn fs_namespace_descriptor_getter_value(property_name: &str) -> f64 {
    let key = format!("fs\0get\0{property_name}");
    if let Some(bits) = NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let property_bytes: &'static [u8] = property_name.as_bytes().to_vec().leak();
    let func_ptr = crate::fn_info!(fs_namespace_descriptor_getter_thunk, 0; with_declared(0));
    let closure = crate::closure::js_closure_alloc(func_ptr, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, property_bytes.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 1, property_bytes.len() as i64);
    let name = if property_name == "promises" {
        "get".to_string()
    } else {
        format!("get {property_name}")
    };
    set_bound_native_closure_name(closure, &name);
    let value = crate::value::js_nanbox_pointer(closure as i64);

    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

pub(crate) fn fs_namespace_descriptor_setter_value(property_name: &str) -> f64 {
    let key = format!("fs\0set\0{property_name}");
    if let Some(bits) = NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let func_ptr = crate::fn_info!(fs_namespace_descriptor_setter_thunk, 1; with_declared(1));
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let name = format!("set {property_name}");
    set_bound_native_closure_name(closure, &name);
    let value = crate::value::js_nanbox_pointer(closure as i64);

    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

/// The EventEmitter method names `node:cluster`'s default import exposes
/// (#3687). Kept narrow so a typo'd `cluster.foo` still reads `undefined`.
pub(crate) fn is_cluster_emitter_method(prop: &str) -> bool {
    matches!(
        prop,
        "on" | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "emit"
            | "eventNames"
            | "listenerCount"
            | "listeners"
            | "rawListeners"
            | "getMaxListeners"
            | "setMaxListeners"
    )
}

fn native_callable_export_display_name<'a>(module: &str, prop: &'a str) -> &'a str {
    if module == "fs" {
        match prop {
            "_toUnixTimestamp" => "toUnixTimestamp",
            "Stats" => "deprecated",
            _ => prop,
        }
    } else {
        prop
    }
}

extern "C" fn buffer_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
    encoding_or_offset: f64,
    length: f64,
) -> f64 {
    let value_js = crate::value::JSValue::from_bits(value.to_bits());
    let buf = if value_js.is_undefined() || value_js.is_null() {
        crate::buffer::js_buffer_alloc(0, 0)
    } else if value_js.is_int32() || value_js.is_number() {
        let size = if value_js.is_int32() {
            value_js.as_int32()
        } else {
            value as i32
        };
        crate::buffer::js_buffer_alloc_unsafe(size)
    } else {
        let second = crate::value::JSValue::from_bits(encoding_or_offset.to_bits());
        let third = crate::value::JSValue::from_bits(length.to_bits());
        let second_is_offset =
            !second.is_undefined() && !second.is_null() && !second.is_any_string();
        if !third.is_undefined() || second_is_offset {
            let len = if third.is_undefined() {
                -1
            } else if third.is_int32() {
                third.as_int32()
            } else {
                length as i32
            };
            let offset = if second.is_int32() {
                second.as_int32()
            } else {
                encoding_or_offset as i32
            };
            crate::buffer::js_buffer_from_arraybuffer_slice(value.to_bits() as i64, offset, len)
        } else {
            let enc = if second.is_undefined() {
                0
            } else {
                crate::buffer::js_encoding_tag_from_value(encoding_or_offset)
            };
            crate::buffer::js_buffer_from_value(value.to_bits() as i64, enc)
        }
    };
    crate::value::js_nanbox_pointer(buf as i64)
}

extern "C" fn buffer_prototype_method_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `this` for a `Buffer.prototype.offset`/`.parent` accessor read, resolved
/// from the getter's `this` argument (the same mechanism `require_webcrypto_this` in
/// `ctor_thunks.rs` uses for Web Crypto getters). `None` for a non-buffer
/// receiver — Node's real getters answer `undefined` rather than throwing
/// (`isInstance(this, Buffer) ? … : undefined`), and ordinary Buffer/typed-
/// array instance reads never reach this getter at all (they resolve
/// `.offset`/`.parent` directly — see `get_field_by_name_tail.rs`); this only
/// matters for reflection (`Object.getOwnPropertyDescriptor(Buffer.prototype,
/// "offset").get.call(x)`) and enumeration.
fn buffer_prototype_this_addr(this: crate::closure::JsThis) -> Option<usize> {
    let this_bits = this.bits();
    let jv = crate::value::JSValue::from_bits(this_bits);
    if !jv.is_pointer() {
        return None;
    }
    let addr = (this_bits & crate::value::POINTER_MASK) as usize;
    if addr == 0 || crate::buffer::js_buffer_is_buffer(addr as i64) == 0 {
        return None;
    }
    Some(addr)
}

/// #10426: `Buffer.prototype.parent` — deprecated legacy alias for
/// `.buffer` (the backing `ArrayBuffer`), still a real own accessor on
/// Node's `Buffer.prototype`.
extern "C" fn buffer_prototype_parent_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match buffer_prototype_this_addr(this) {
        Some(addr) => {
            crate::value::js_nanbox_pointer(crate::buffer::buffer_backing_array_buffer(addr) as i64)
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// #10426: `Buffer.prototype.offset` — deprecated legacy alias for
/// `.byteOffset`, still a real own accessor on Node's `Buffer.prototype`.
extern "C" fn buffer_prototype_offset_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    match buffer_prototype_this_addr(this) {
        Some(addr) => crate::buffer::buffer_byte_offset(addr) as f64,
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// Install a `Buffer.prototype` accessor (`offset`/`parent`) — `{
/// enumerable: true, configurable: false }`, matching Node's
/// `ObjectDefineProperty(Buffer.prototype, name, { enumerable: true, get()
/// {…} })` (no `configurable: true`, so it defaults false). Mirrors
/// `install_webcrypto_proto_getter`'s shape for a `*mut ObjectHeader` proto.
fn install_buffer_prototype_getter(
    proto_obj: *mut ObjectHeader,
    name: &str,
    info: *const crate::closure::JsFunctionInfo,
) {
    if proto_obj.is_null() {
        return;
    }
    let closure = crate::closure::js_closure_alloc(info, 0);
    let value = if closure.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        set_bound_native_closure_name(closure, &format!("get {name}"));
        crate::value::js_nanbox_pointer(closure as i64)
    };
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto_obj, key, f64::from_bits(crate::value::TAG_UNDEFINED));
    super::set_builtin_accessor_descriptor(
        proto_obj as usize,
        name.to_string(),
        super::AccessorDescriptor {
            get: value.to_bits(),
            set: 0,
        },
        super::PropertyAttrs::new(true, true, false),
    );
}

const BUFFER_STATIC_METHODS: &[&str] = &[
    "from",
    "alloc",
    "allocUnsafe",
    "allocUnsafeSlow",
    "concat",
    "of",
    "isBuffer",
    "isEncoding",
    "byteLength",
    "compare",
    "copyBytesFrom",
];

/// Node's actual `Buffer.prototype` own-property surface — checked
/// property-by-property against `Object.getOwnPropertyNames(Buffer.prototype)`
/// on Node 26.5.1 (96 names there; 93 real callable methods here, plus
/// `constructor` and the `offset`/`parent` accessors installed separately
/// below = 96). Perry used to install ELEVEN, which quietly broke any code
/// that walks the prototype: mysql2 sizes every outgoing packet by no-op'ing
/// the write methods of a zero-length Buffer
/// (`for (const k in Buffer.prototype) if (typeof mock[k] === "function") mock[k] = noop`),
/// so `writeUInt32LE` — absent from the stub list — stayed live, wrote into the
/// empty measuring buffer, and killed the MySQL handshake with
/// RangeError [ERR_OUT_OF_RANGE].
///
/// #10426: this list is DELIBERATELY NOT the same as
/// `buffer_dispatch::is_buffer_method_name` (a comment here used to say it
/// was "generated from" that table "so the two can't drift" — that coupling
/// was the bug). `is_buffer_method_name` answers a different question — "does
/// a property read on a Buffer *instance* need to synthesize a bound-method
/// closure" — and is deliberately broad: it also recognizes names Buffer
/// instances answer only by INHERITANCE (`Uint8Array.prototype.at`/`set`/
/// `entries`/`keys`/`values`/`copyWithin`/`toBase64`/`toHex`/`setFromBase64`/
/// `setFromHex`), by DataView accessors on a DataView-marked buffer
/// (`getInt32`/`setFloat64`/…), and by `Object.prototype`
/// (`hasOwnProperty`/`isPrototypeOf`/`propertyIsEnumerable`/`valueOf`) so
/// duck-type probes on an INSTANCE keep working (see that table's own
/// comments). None of those belong on `Buffer.prototype` itself as OWN
/// properties — Node inherits them further up the chain — so installing this
/// list from that one copied 36 names Node never puts here (plus two bare
/// string literals, `"function"` and `"undefined"`, that had drifted in from
/// nearby prose/JS-idiom comments and were never real method names at all).
const BUFFER_PROTOTYPE_METHODS: &[&str] = &[
    "toString",
    "inspect",
    "slice",
    "subarray",
    "copy",
    "write",
    "toJSON",
    "fill",
    "equals",
    "compare",
    "indexOf",
    "lastIndexOf",
    "includes",
    "swap16",
    "swap32",
    "swap64",
    "toLocaleString",
    "readUInt8",
    "readUint8",
    "readInt8",
    "readUInt16BE",
    "readUint16BE",
    "readUInt16LE",
    "readUint16LE",
    "readInt16BE",
    "readInt16LE",
    "readUInt32BE",
    "readUint32BE",
    "readUInt32LE",
    "readUint32LE",
    "readInt32BE",
    "readInt32LE",
    "readFloatBE",
    "readFloatLE",
    "readDoubleBE",
    "readDoubleLE",
    "readBigInt64BE",
    "readBigInt64LE",
    "readBigUInt64BE",
    "readBigUint64BE",
    "readBigUInt64LE",
    "readBigUint64LE",
    "readUIntBE",
    "readUintBE",
    "readUIntLE",
    "readUintLE",
    "readIntBE",
    "readIntLE",
    "writeUInt8",
    "writeUint8",
    "writeInt8",
    "writeUInt16BE",
    "writeUint16BE",
    "writeUInt16LE",
    "writeUint16LE",
    "writeInt16BE",
    "writeInt16LE",
    "writeUInt32BE",
    "writeUint32BE",
    "writeUInt32LE",
    "writeUint32LE",
    "writeInt32BE",
    "writeInt32LE",
    "writeFloatBE",
    "writeFloatLE",
    "writeDoubleBE",
    "writeDoubleLE",
    "writeBigInt64BE",
    "writeBigInt64LE",
    "writeBigUInt64BE",
    "writeBigUint64BE",
    "writeBigUInt64LE",
    "writeBigUint64LE",
    "writeUIntBE",
    "writeUintBE",
    "writeUIntLE",
    "writeUintLE",
    "writeIntBE",
    "writeIntLE",
    "asciiSlice",
    "asciiWrite",
    "base64Slice",
    "base64Write",
    "base64urlSlice",
    "base64urlWrite",
    "hexSlice",
    "hexWrite",
    "latin1Slice",
    "latin1Write",
    "ucs2Slice",
    "ucs2Write",
    "utf8Slice",
    "utf8Write",
];

const ASSERT_PROTOTYPE_METHODS: &[&str] = &[
    "fail",
    "ok",
    "equal",
    "notEqual",
    "deepEqual",
    "notDeepEqual",
    "deepStrictEqual",
    "notDeepStrictEqual",
    "strictEqual",
    "notStrictEqual",
    "partialDeepStrictEqual",
    "throws",
    "rejects",
    "doesNotThrow",
    "doesNotReject",
    "ifError",
    "match",
    "doesNotMatch",
];

/// Materialize Node's CommonJS `assert` export: a callable alias for `ok`
/// carrying the rest of the assert namespace as properties. Static ESM
/// namespace imports stay ordinary namespace objects; this shape is used by
/// dynamic `require` / `module.createRequire` resolution (#4975).
pub(crate) fn assert_cjs_export_value(module_name: &str) -> f64 {
    let module_name = if module_name == "assert/strict" {
        "assert/strict"
    } else {
        "assert"
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let callable = scope.root_nanbox_f64(bound_native_callable_export_value(module_name, "ok"));

    for method in ASSERT_PROTOTYPE_METHODS {
        let method_value = bound_native_callable_export_value(module_name, method);
        let closure = crate::value::JSValue::from_bits(callable.get_nanbox_f64().to_bits())
            .as_pointer::<crate::closure::ClosureHeader>() as usize;
        crate::closure::closure_set_dynamic_prop(closure, method, method_value);
    }

    let strict = if module_name == "assert/strict" {
        callable.get_nanbox_f64()
    } else {
        assert_cjs_export_value("assert/strict")
    };
    let closure = crate::value::JSValue::from_bits(callable.get_nanbox_f64().to_bits())
        .as_pointer::<crate::closure::ClosureHeader>() as usize;
    crate::closure::closure_set_dynamic_prop(closure, "strict", strict);
    crate::closure::closure_set_dynamic_prop(closure, "default", callable.get_nanbox_f64());
    callable.get_nanbox_f64()
}

fn attach_assert_prototype(constructor_value: f64) {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return;
    }
    let closure = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if closure == 0 {
        return;
    }

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }

    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    super::define_builtin_data_property(
        proto,
        constructor_key,
        constructor_value,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    for method in ASSERT_PROTOTYPE_METHODS {
        let method_value = bound_native_callable_export_value("assert", method);
        let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
        super::define_builtin_data_property(
            proto,
            key,
            method_value,
            (*method).to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

fn attach_crypto_key_object_shape(closure_addr: usize, constructor_value: f64) {
    let from_value = bound_native_callable_export_value("crypto.KeyObject", "from");
    crate::closure::closure_set_dynamic_prop(closure_addr, "from", from_value);
    super::set_builtin_property_attrs(
        closure_addr,
        "from".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }
    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    super::define_builtin_data_property(
        proto,
        constructor_key,
        constructor_value,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure_addr,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

extern "C" fn x509_issuer_certificate_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn attach_crypto_x509_certificate_shape(closure_addr: usize, constructor_value: f64) {
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }
    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    super::define_builtin_data_property(
        proto,
        constructor_key,
        constructor_value,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    unsafe {
        let getter = crate::closure::js_closure_alloc(
            crate::fn_info!(x509_issuer_certificate_getter_thunk, 0; with_declared(0)),
            0,
        );
        if !getter.is_null() {
            let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
            super::object_ops::install_builtin_getter(proto, "issuerCertificate", getter_bits);
        }
    }

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure_addr,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

pub(crate) fn native_string_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn native_bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn native_object_value(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn native_set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(native_object_value(obj));
    let value = scope.root_nanbox_f64(value);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(obj.get_nanbox_f64()) as *mut ObjectHeader,
        key,
        value.get_nanbox_f64(),
    );
}

fn native_color_tuple(open: i32, close: i32) -> f64 {
    let arr = crate::array::js_array_alloc_with_length(2);
    crate::array::js_array_set_f64(arr, 0, open as f64);
    crate::array::js_array_set_f64(arr, 1, close as f64);
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn util_inspect_custom_symbol() -> f64 {
    unsafe { crate::symbol::js_symbol_for(native_string_value("nodejs.util.inspect.custom")) }
}

pub(crate) fn util_inspect_default_options_value() -> f64 {
    UTIL_INSPECT_DEFAULT_OPTIONS.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        native_set_field(obj, "showHidden", native_bool_value(false));
        native_set_field(obj, "depth", 2.0);
        native_set_field(obj, "colors", native_bool_value(false));
        native_set_field(obj, "customInspect", native_bool_value(true));
        native_set_field(obj, "showProxy", native_bool_value(false));
        native_set_field(obj, "maxArrayLength", 100.0);
        native_set_field(obj, "maxStringLength", 10000.0);
        native_set_field(obj, "breakLength", 80.0);
        native_set_field(obj, "compact", 3.0);
        native_set_field(obj, "sorted", native_bool_value(false));
        native_set_field(obj, "getters", native_bool_value(false));
        native_set_field(obj, "numericSeparator", native_bool_value(false));

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn util_inspect_styles() -> f64 {
    UTIL_INSPECT_STYLES.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        native_set_field(obj, "special", native_string_value("cyan"));
        native_set_field(obj, "number", native_string_value("yellow"));
        native_set_field(obj, "bigint", native_string_value("yellow"));
        native_set_field(obj, "boolean", native_string_value("yellow"));
        native_set_field(obj, "undefined", native_string_value("grey"));
        native_set_field(obj, "null", native_string_value("bold"));
        native_set_field(obj, "string", native_string_value("green"));
        native_set_field(obj, "symbol", native_string_value("green"));
        native_set_field(obj, "date", native_string_value("magenta"));
        native_set_field(obj, "regexp", native_string_value("red"));
        native_set_field(obj, "module", native_string_value("underline"));

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn util_inspect_colors() -> f64 {
    UTIL_INSPECT_COLORS.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        for style in crate::util_style_text::INSPECT_COLOR_STYLES {
            native_set_field(obj, style.name, native_color_tuple(style.open, style.close));
        }

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

pub(crate) fn zlib_codes_object() -> f64 {
    const ZLIB_RETURN_CODES: &[(&str, i32)] = &[
        ("Z_OK", 0),
        ("Z_STREAM_END", 1),
        ("Z_NEED_DICT", 2),
        ("Z_ERRNO", -1),
        ("Z_STREAM_ERROR", -2),
        ("Z_DATA_ERROR", -3),
        ("Z_MEM_ERROR", -4),
        ("Z_BUF_ERROR", -5),
        ("Z_VERSION_ERROR", -6),
    ];

    ZLIB_CODES_OBJECT.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        for (name, value) in ZLIB_RETURN_CODES.iter().take(3) {
            native_set_field(obj, &value.to_string(), native_string_value(name));
        }
        for (name, value) in ZLIB_RETURN_CODES {
            native_set_field(obj, name, *value as f64);
        }
        for (name, value) in ZLIB_RETURN_CODES.iter().skip(3) {
            native_set_field(obj, &value.to_string(), native_string_value(name));
        }

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

pub(crate) fn timers_promises_parent_namespace() -> f64 {
    TIMERS_PROMISES_PARENT_NAMESPACE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let module_name = "timers/promises";
        let value = js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn attach_tty_stream_prototype(constructor_value: f64, name: &str) {
    crate::tty::attach_tty_constructor_prototype(constructor_value, name);
}

fn attach_tls_secure_context_prototype(constructor_value: f64) {
    crate::tls::attach_secure_context_constructor_prototype(constructor_value);
}

pub(super) const TLS_SOCKET_PROTOTYPE_METHODS: &[(&str, u32)] = &[
    ("setKeyCert", 1),
    ("getSharedSigalgs", 0),
    ("getX509Certificate", 0),
    ("getPeerX509Certificate", 0),
];

thread_local! {
    static TLS_DERIVED_PROTOTYPES: RefCell<Vec<(u64, u8)>> = const { RefCell::new(Vec::new()) };
}

const TLS_PARENT_EVENT_EMITTER: u8 = 1;
const TLS_PARENT_DUPLEX: u8 = 2;

pub(crate) fn scan_tls_derived_prototype_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    TLS_DERIVED_PROTOTYPES.with(|prototypes| {
        for (bits, _) in prototypes.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
}

extern "C" fn tls_prototype_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    unsafe {
        let name_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const i8;
        let name_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        let receiver = this.as_f64();
        let args_array = crate::value::js_nanbox_get_pointer(rest);
        crate::object::js_native_call_method_apply(receiver, name_ptr, name_len, args_array)
    }
}

fn attach_tls_constructor_prototype(constructor_value: f64, constructor_name: &str) -> f64 {
    let methods = if constructor_name == "TLSSocket" {
        TLS_SOCKET_PROTOTYPE_METHODS
    } else {
        &[]
    };
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return constructor_value;
    }
    let constructor = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if constructor == 0 {
        return constructor_value;
    }

    // Every allocator below can move objects. Hold only updateable handles
    // across allocations and reload the current address at each use.
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor_handle =
        scope.root_raw_mut_ptr(constructor as *mut crate::closure::ClosureHeader);
    let prototype = js_object_alloc(0, 0);
    if prototype.is_null() {
        return crate::value::js_nanbox_pointer(
            constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        );
    }
    let prototype_handle = scope.root_raw_mut_ptr(prototype);
    let constructor_key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), "constructor".len() as u32);
    let constructor_key_handle = scope.root_string_ptr(constructor_key);
    js_object_set_field_by_name(
        prototype_handle.get_raw_mut_ptr(),
        constructor_key_handle.get_raw_mut_ptr(),
        crate::value::js_nanbox_pointer(
            constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    super::super::set_builtin_property_attrs(
        prototype_handle.get_raw_mut_ptr::<ObjectHeader>() as usize,
        "constructor".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    let thunk = crate::fn_info!(tls_prototype_method_thunk, 1; with_rest(0));
    for &(name, length) in methods {
        let method = crate::closure::js_closure_alloc(thunk, 2);
        if method.is_null() {
            continue;
        }
        let method_handle = scope.root_raw_mut_ptr(method);
        crate::closure::js_closure_set_capture_ptr(
            method_handle.get_raw_mut_ptr(),
            0,
            name.as_ptr() as i64,
        );
        crate::closure::js_closure_set_capture_ptr(
            method_handle.get_raw_mut_ptr(),
            1,
            name.len() as i64,
        );
        let name_string = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let name_handle = scope.root_string_ptr(name_string);
        crate::closure::closure_define_dynamic_prop(
            method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            "name",
            f64::from_bits(JSValue::string_ptr(name_handle.get_raw_mut_ptr()).bits()),
        );
        super::super::set_builtin_property_attrs(
            method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            "name".to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
        set_builtin_closure_length(
            method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            length,
        );
        js_object_set_field_by_name(
            prototype_handle.get_raw_mut_ptr(),
            name_handle.get_raw_mut_ptr(),
            crate::value::js_nanbox_pointer(
                method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
        super::super::set_builtin_property_attrs(
            prototype_handle.get_raw_mut_ptr::<ObjectHeader>() as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }

    crate::closure::closure_set_dynamic_prop(
        constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        "prototype",
        crate::value::js_nanbox_pointer(prototype_handle.get_raw_mut_ptr::<ObjectHeader>() as i64),
    );
    let parent_kind = match constructor_name {
        "Server" => TLS_PARENT_EVENT_EMITTER,
        "TLSSocket" => TLS_PARENT_DUPLEX,
        _ => 0,
    };
    if parent_kind != 0 {
        let bits = crate::value::js_nanbox_pointer(
            prototype_handle.get_raw_mut_ptr::<ObjectHeader>() as i64,
        )
        .to_bits();
        crate::gc::runtime_write_barrier_root_nanbox(bits);
        TLS_DERIVED_PROTOTYPES.with(|prototypes| {
            let mut prototypes = prototypes.borrow_mut();
            if !prototypes.iter().any(|(existing, _)| *existing == bits) {
                prototypes.push((bits, parent_kind));
            }
        });
    }
    super::super::set_builtin_property_attrs(
        constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        "prototype".to_string(),
        super::super::PropertyAttrs::new(true, false, false),
    );
    crate::value::js_nanbox_pointer(
        constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    )
}

pub(crate) fn tls_constructor_prototype_is_instance_of(value: f64, parent_name: &str) -> bool {
    let parent_kind = match parent_name {
        "EventEmitter" => TLS_PARENT_EVENT_EMITTER,
        "Duplex" => TLS_PARENT_DUPLEX,
        _ => return false,
    };
    TLS_DERIVED_PROTOTYPES.with(|prototypes| {
        prototypes
            .borrow()
            .iter()
            .any(|(bits, kind)| *bits == value.to_bits() && *kind == parent_kind)
    })
}

pub(crate) unsafe fn bound_native_callable_module_and_method(
    value: f64,
) -> Option<(String, String)> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let closure = jv.as_pointer::<crate::closure::ClosureHeader>();
    // A POINTER-tagged value is not necessarily a heap closure: a Proxy is
    // POINTER-tagged over a SMALL encoded id that lands in the handle band
    // (`class X extends new Proxy(fn, …)`). Dereferencing its `type_tag` would
    // read offset 0xc of a bogus ~handle-sized address and segfault. Classify
    // through `is_closure_ptr`, which rejects the handle band AND every
    // non-heap address on EVERY platform (the macOS-only `is_valid_obj_ptr`
    // heap floor lets a `0xf0000`-band id through on Linux — the #5592 sweep
    // host) before probing `CLOSURE_MAGIC`. Refs test262
    // class/subclass/superclass-{arrow,async,generator,async-generator}-function.
    if !crate::closure::is_closure_ptr(closure as usize)
        || (*closure).code() != crate::closure::BOUND_METHOD_FUNC_PTR
    {
        return None;
    }
    let ns = crate::closure::js_closure_get_capture_f64(closure, 0);
    let module = get_module_name_from_namespace(ns);
    let method_ptr = crate::closure::js_closure_get_capture_ptr(closure, 1) as *const u8;
    let method_len = crate::closure::js_closure_get_capture_ptr(closure, 2) as usize;
    if method_ptr.is_null() {
        return None;
    }
    let method = std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len))
        .ok()?
        .to_string();
    Some((module, method))
}

pub(crate) unsafe fn bound_native_callable_value_arity(value: f64) -> Option<u32> {
    let (module, method) = bound_native_callable_module_and_method(value)?;
    let module = normalize_native_module_alias(&module);
    match (module, method.as_str()) {
        ("console", "Console") => Some(1),
        ("util", "isArray") => Some(1),
        ("module", "isBuiltin") => Some(1),
        ("process", "getBuiltinModule") => Some(1),
        _ => native_callable_export_arity(module, method.as_str()),
    }
}

pub(crate) fn builtin_closure_is_non_constructable_value(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
    if ptr.is_null() {
        return false;
    }
    builtin_closure_is_non_constructable(ptr as usize)
}

// ---------------------------------------------------------------------------
// Per-module attach handlers (bodies moved verbatim from the former inline
// ladder). Referenced ONLY from the attach registry via each module's
// `js_nm_install_<module>()`, so unimported modules' machinery dead-strips.
// ---------------------------------------------------------------------------

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_module(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    if property_name == "Module" {
        value = attach_module_cjs_constructor_statics(value);
    }
    if property_name == "SourceMap" {
        // SourceMap's prototype is intrinsic constructor state; every import
        // form installs this handler before the callable can be minted.
        crate::process::module_source_map_attach_constructor(crate::value::js_nanbox_get_pointer(
            value,
        ) as usize);
    }
    if matches!(property_name, "flushCompileCache" | "isBuiltin") {
        set_builtin_closure_non_constructable(crate::value::js_nanbox_get_pointer(value) as usize);
    }
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_tty(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    if matches!(property_name, "ReadStream" | "WriteStream") {
        attach_tty_stream_prototype(value, property_name);
    }
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_tls(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    if property_name == "SecureContext" {
        attach_tls_secure_context_prototype(value);
    } else if matches!(property_name, "Server" | "TLSSocket") {
        value = attach_tls_constructor_prototype(value, property_name);
    }
    value
}

/// #10454: `http.ServerResponse.prototype` carries its methods so
/// `util.inherits` / `setPrototypeOf` / `extends` subclasses inherit them.
pub(crate) unsafe fn nm_attach_http(property_name: &str, value: f64, _closure_addr: usize) -> f64 {
    if property_name == "ServerResponse" {
        return crate::object::native_this_alias::attach_http_server_response_prototype(value);
    }
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_wasi(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    if property_name == "WASI" {
        crate::wasi::attach_wasi_constructor_prototype(value);
    }
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_stream(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    if property_name == "Stream" {
        attach_stream_legacy_prototype(value);
    }
    if true
        && matches!(
            property_name,
            "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
        )
    {
        attach_stream_constructor_prototype(value, property_name);
    }
    // #6692: Node defines `stream.pipeline[util.promisify.custom]` and
    // `stream.finished[util.promisify.custom]` pointing at the promise-based
    // `stream/promises` implementations, so `promisify(stream.pipeline)` returns
    // that impl rather than the generic callback-appending wrapper. Wire the
    // same hooks so `custom_promisified_value` (util_promisify.rs) honors them.
    if matches!(property_name, "pipeline" | "finished") {
        // Reassign: the attach helper roots `value` and allocates (which may
        // evacuate the closure), so it returns the possibly-relocated pointer.
        value = attach_stream_promisify_custom(value, property_name);
    }
    value
}

/// Ensure the public `ChildProcess` constructor has its prototype. The direct
/// codegen path can create the bound constructor without installing the module
/// attach hook first.
pub(crate) unsafe fn ensure_child_process_prototype(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(value);
    let closure_addr =
        (constructor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    if closure_addr == 0 || !crate::closure::is_closure_ptr(closure_addr) {
        return constructor.get_nanbox_f64();
    }
    if crate::value::JSValue::from_bits(
        crate::closure::closure_get_dynamic_prop(closure_addr, "prototype").to_bits(),
    )
    .is_pointer()
    {
        return constructor.get_nanbox_f64();
    }
    let proto = js_object_alloc_with_shape(
        0x7FFF_FDA0,
        1,
        b"constructor\0".as_ptr(),
        b"constructor\0".len() as u32,
    );
    js_object_set_field(
        proto,
        0,
        JSValue::from_bits(constructor.get_nanbox_f64().to_bits()),
    );
    let closure_addr =
        (constructor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    crate::closure::closure_set_dynamic_prop(
        closure_addr,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
    );
    constructor.get_nanbox_f64()
}

/// Attach the public `ChildProcess.prototype` so low-level constructed
/// instances participate in ordinary `instanceof ChildProcess` checks.
pub(crate) unsafe fn nm_attach_child_process(
    property_name: &str,
    value: f64,
    _closure_addr: usize,
) -> f64 {
    if property_name == "ChildProcess" {
        ensure_child_process_prototype(value)
    } else {
        value
    }
}

pub(crate) unsafe fn nm_attach_cluster(
    property_name: &str,
    value: f64,
    _closure_addr: usize,
) -> f64 {
    if property_name == "Worker" {
        crate::cluster::ensure_worker_constructor(value)
    } else {
        value
    }
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_sqlite(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    // `DatabaseSync` / `StatementSync` / `Session` are native-payload
    // families (#11919): materialize the family prototype now, so the export
    // carries its `prototype` (with the methods) before any instance exists.
    // The family installs it on this very export value.
    if matches!(property_name, "DatabaseSync" | "StatementSync" | "Session") {
        let ptr = crate::value::JS_NATIVE_SQLITE_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
        if !ptr.is_null() {
            let dispatch: crate::value::JsNativeSqliteDispatchFn = std::mem::transmute(ptr);
            dispatch(
                property_name.as_ptr(),
                property_name.len(),
                std::ptr::null(),
                0,
                crate::value::NATIVE_SQLITE_DISPATCH_PROTOTYPE,
            );
        }
    }
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_assert(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    if property_name == "Assert" {
        attach_assert_prototype(value);
    }
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_crypto(
    property_name: &str,
    mut value: f64,
    closure_addr: usize,
) -> f64 {
    if property_name == "KeyObject" {
        attach_crypto_key_object_shape(closure_addr, value);
    }
    if property_name == "X509Certificate" {
        attach_crypto_x509_certificate_shape(closure_addr, value);
    }
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_perf_hooks(
    _property_name: &str,
    value: f64,
    _closure_addr: usize,
) -> f64 {
    value
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_async_hooks(
    property_name: &str,
    mut value: f64,
    _closure_addr: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor_handle = scope.root_nanbox_f64(value);
    if property_name == "AsyncLocalStorage" {
        constructor_handle.set_nanbox_f64(
            super::async_hooks_exports::attach_async_local_storage_prototype(
                constructor_handle.get_nanbox_f64(),
            ),
        );
        let bind = scope.root_nanbox_f64(async_hooks_static_method_value(
            crate::fn_info!(crate::async_hooks::js_async_local_storage_static_bind_method, 2; with_rest(1)),
            "bind",
            1,
        ));
        crate::closure::closure_set_dynamic_prop(
            crate::value::js_nanbox_get_pointer(constructor_handle.get_nanbox_f64()) as usize,
            "bind",
            bind.get_nanbox_f64(),
        );
        super::set_builtin_property_attrs(
            crate::value::js_nanbox_get_pointer(constructor_handle.get_nanbox_f64()) as usize,
            "bind".to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
        let snapshot = scope.root_nanbox_f64(async_hooks_static_method_value(
            crate::fn_info!(crate::async_hooks::js_async_local_storage_static_snapshot_method, 1; with_rest(0)),
            "snapshot",
            0,
        ));
        crate::closure::closure_set_dynamic_prop(
            crate::value::js_nanbox_get_pointer(constructor_handle.get_nanbox_f64()) as usize,
            "snapshot",
            snapshot.get_nanbox_f64(),
        );
        super::set_builtin_property_attrs(
            crate::value::js_nanbox_get_pointer(constructor_handle.get_nanbox_f64()) as usize,
            "snapshot".to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }

    if property_name == "AsyncResource" {
        constructor_handle.set_nanbox_f64(
            super::async_hooks_exports::attach_async_resource_prototype(
                constructor_handle.get_nanbox_f64(),
            ),
        );
        let bind = scope.root_nanbox_f64(async_hooks_static_method_value(
            crate::fn_info!(crate::async_hooks::js_async_resource_static_bind_method, 4; with_rest(3)),
            "bind",
            3,
        ));
        crate::closure::closure_set_dynamic_prop(
            crate::value::js_nanbox_get_pointer(constructor_handle.get_nanbox_f64()) as usize,
            "bind",
            bind.get_nanbox_f64(),
        );
        super::set_builtin_property_attrs(
            crate::value::js_nanbox_get_pointer(constructor_handle.get_nanbox_f64()) as usize,
            "bind".to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }
    value = constructor_handle.get_nanbox_f64();
    value
}

pub(crate) unsafe fn nm_attach_events(name: &str, value: f64, _addr: usize) -> f64 {
    if name == "EventEmitter" {
        super::constructor_shapes::install_event_emitter_statics(value)
    } else {
        value
    }
}

#[allow(unused_mut)]
pub(crate) unsafe fn nm_attach_util(
    property_name: &str,
    mut value: f64,
    closure_addr: usize,
) -> f64 {
    if property_name == "promisify" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "custom",
            crate::util_promisify::promisify_custom_symbol(),
        );
    }
    if property_name == "inspect" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "custom",
            util_inspect_custom_symbol(),
        );
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "defaultOptions",
            util_inspect_default_options_value(),
        );
        crate::closure::closure_set_dynamic_prop(closure_addr, "styles", util_inspect_styles());
        crate::closure::closure_set_dynamic_prop(closure_addr, "colors", util_inspect_colors());
    }
    value
}
