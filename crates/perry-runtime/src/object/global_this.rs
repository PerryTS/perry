//! `globalThis` singleton plus the built-in constructor / prototype-method
//! population that backs `globalThis.Array`, `globalThis.Object`,
//! `globalThis.console`, etc. Also home for
//! `js_global_or_console_property_by_name`, the codegen-emitted
//! property-read shortcut.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

/// Issue #611 (Effect): `globalThis[<computed>] = value` and the
/// `(globalThis as any)[id] ??= new Map()` pattern (used by hono / Effect /
/// most ESM libraries that ship a CJS-compat global side-store) wrote to
/// a 0-pointer sentinel and read back undefined — `globalStore` was always
/// undefined, callers SIGSEGV'd at the next `.has()` / `.get()` call. This
/// function lazily allocates a single shared ObjectHeader (one per process,
/// initialised on first access) and returns a NaN-boxed POINTER to it. The
/// codegen-side IndexGet / IndexSet on `Expr::GlobalGet` routes through
/// this helper instead of through the 0.0 sentinel so reads / writes
/// actually persist. Existing AST-shape patterns like
/// `PropertyGet { GlobalGet, "log" }` (console.log dispatch) match on the
/// HIR node, not the SSA value, so they continue to fire even though the
#[no_mangle]
pub extern "C" fn js_get_global_this() -> f64 {
    let cached = GLOBAL_THIS_PTR.load(Ordering::Acquire);
    let ptr = if cached != 0 {
        cached
    } else {
        // First access — allocate. Race-tolerant: if two threads race the
        // initial alloc, the loser's allocation leaks (never freed) but
        // both threads see the winner's pointer afterward via CAS.
        let new_ptr = js_object_alloc(0, 0) as i64;
        // GC_STORE_AUDIT(ROOT): GLOBAL_THIS_PTR is a mutable root visited by scan_object_cache_roots_mut.
        match GLOBAL_THIS_PTR.compare_exchange(0, new_ptr, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                // Winner: populate built-in constructor properties on the
                // singleton so `globalThis.Array` / `context.Array` (lodash's
                // `runInContext` pattern) return non-undefined values. Each
                // value is a tiny ObjectHeader carrying a `prototype` field
                // pointing at another empty object — enough that
                // `var arrayProto = Array.prototype` doesn't throw and the
                // chained `.toString` reads return undefined rather than
                // tripping the "Cannot read properties of undefined" gate at
                // module-init time. Full constructor dispatch on these
                // sentinels still falls through to existing code paths (bare
                // `new Array(n)` continues to work through `lower_new`); the
                // goal here is just to unblock libraries that read the
                // constructors off `globalThis` as values. Refs lodash
                // `runInContext` blocker after PR #963.
                populate_global_this_builtins(new_ptr as *mut ObjectHeader);
                new_ptr
            }
            Err(other) => other,
        }
    };
    crate::value::js_nanbox_pointer(ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_global_or_console_property_by_name(
    key: *const crate::StringHeader,
) -> f64 {
    if !key.is_null() {
        let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        let property_name =
            std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
        if is_native_module_callable_export("console", property_name) {
            return js_native_module_property_by_name(
                b"console".as_ptr(),
                "console".len(),
                key_ptr,
                key_len,
            );
        }
    }

    let global_box = js_get_global_this();
    let global = crate::value::JSValue::from_bits(global_box.to_bits());
    if global.is_pointer() {
        let obj = global.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        return js_object_get_field_by_name_f64(obj, key);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// JS built-in constructor names exposed on `globalThis`. Pre-populated by
/// the singleton init in `js_get_global_this` so libraries that read these
/// off the global (lodash's `var Array = context.Array; var arrayProto =
/// Array.prototype`, the same `(globalThis as any).X` read shape) see a
/// non-undefined backing object. Codegen mirrors this list in
/// `perry-codegen/src/expr.rs::is_global_this_builtin_name` to decide when
/// `globalThis.<Name>` should route through the singleton instead of the
/// legacy `0.0` no-value placeholder.
pub(crate) const GLOBAL_THIS_BUILTIN_CONSTRUCTORS: &[&str] = &[
    "Array",
    "Object",
    "String",
    "Number",
    "Boolean",
    "Function",
    "RegExp",
    "Date",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "Symbol",
    "Promise",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "Proxy",
    "BigInt",
    "Uint8Array",
    "Int8Array",
    "Uint16Array",
    "Int16Array",
    "Uint32Array",
    "Int32Array",
    "Float32Array",
    "Float64Array",
    "Uint8ClampedArray",
    "BigInt64Array",
    "BigUint64Array",
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "TextEncoder",
    "TextDecoder",
    "TextEncoderStream",
    "TextDecoderStream",
    "URL",
    "URLSearchParams",
    "AbortController",
    "AbortSignal",
    "FormData",
    "Blob",
    "Headers",
    "Request",
    "Response",
    "FinalizationRegistry",
];

/// JS built-in namespaces (typeof === "object", not "function"). Same
/// shape on the singleton — a backing object with `prototype` so chained
/// reads degrade gracefully — but typeof reports "object".
pub(crate) const GLOBAL_THIS_BUILTIN_NAMESPACES: &[&str] =
    &["console", "process", "Math", "JSON", "Reflect"];

/// No-op thunk used as the function body for most singleton globalThis
/// built-in constructor values. Lets `globalThis.Array` carry a real
/// ClosureHeader (so `typeof globalThis.Array === "function"`) without
/// implementing actual constructor dispatch through this path — bare
/// `new Array(n)` continues to flow through codegen's `lower_new` arm and
/// the runtime `js_array_alloc` machinery, so callers that follow the
/// usual `new <Ident>(...)` pattern are unaffected. Calling these
/// sentinels directly (e.g. `globalThis.Array(3)`) returns undefined —
/// best-effort no-op rather than throwing — and remains a known gap for
/// non-String call-form constructors after re-binding the global to a local.
pub(crate) extern "C" fn global_this_builtin_noop_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn global_this_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let string_ptr = crate::builtins::js_string_coerce(value);
    crate::value::js_nanbox_string(string_ptr as i64)
}

/// Thunk for `Object.prototype.toString` exposed as a callable closure
/// value. Mirrors `Object.prototype.toString.call(x)` — returns the
/// `"[object Tag]"` string for the receiver in IMPLICIT_THIS.
///
/// Tag detection uses the same coarse NaN-box / GC-type discrimination
/// the rest of the runtime relies on: arrays → `"[object Array]"`,
/// strings → `"[object String]"`, null/undefined → matching tags,
/// numbers/bools → primitive tags, generic objects/closures →
/// `"[object Object]"`.
///
/// Unblocks ramda's `_isArguments.js` IIFE which evaluates
/// `Object.prototype.toString.call(arguments)` at module-init time
/// — pre-fix the chained `Object.prototype.toString` read returned
/// `undefined`, so the `.call` access threw before the IIFE body ran.
extern "C" fn object_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    if let Some(tag) = crate::object::web_stream_to_string_tag(f64::from_bits(this_bits)) {
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
        let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(crate::js_nanbox_string(s as i64).to_bits());
    }
    let this_jsv = JSValue::from_bits(this_bits);
    let tag: &[u8] = if this_jsv.is_undefined() {
        b"[object Undefined]"
    } else if this_jsv.is_null() {
        b"[object Null]"
    } else if this_jsv.is_bool() {
        b"[object Boolean]"
    } else if this_jsv.is_any_string() {
        b"[object String]"
    } else if this_jsv.is_int32() || this_jsv.is_number() {
        b"[object Number]"
    } else {
        // Discriminate by GC header type for heap-allocated values.
        // Accept both NaN-boxed pointers and raw-i64 pointers (the
        // codegen's two representations for non-numeric values — see
        // CLAUDE.md "Module-level variables"). Module-level arrays
        // arrive here as raw i64 because the codegen stores them
        // unboxed; function-arg-passed arrays arrive NaN-boxed.
        let raw = if this_jsv.is_pointer() {
            (this_bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
        } else {
            this_bits as *const u8
        };
        if !raw.is_null() && (raw as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            unsafe {
                let gc_header = raw.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                let gc_type = (*gc_header).obj_type;
                if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                    b"[object Array]"
                } else if gc_type == crate::gc::GC_TYPE_ERROR {
                    b"[object Error]"
                } else {
                    b"[object Object]"
                }
            }
        } else {
            b"[object Object]"
        }
    };
    let s = crate::string::js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    f64::from_bits(crate::js_nanbox_string(s as i64).to_bits())
}

/// Thunk for `Array.prototype.slice` exposed as a real callable closure
/// value. Reads the array receiver from `IMPLICIT_THIS` (set by
/// `Function.prototype.call`/`.apply`'s runtime arm in
/// `js_native_call_method`) and forwards to `js_array_slice`.
///
/// Coerces start/end through `JSValue::to_number`, with `undefined`
/// mapping to `0` for start and `i32::MAX` for end — matching
/// `Array.prototype.slice`'s ECMA-262 defaults.
///
/// Unblocks the `Array.prototype.slice.call(list, …)` pattern that
/// ramda's curry/variadic helpers use heavily (refs `_curry1`,
/// `_curry2`, and every variadic op like `addIndex`/`addIndexRight`/
/// `useWith`/`unapply`/`flip`/`call`). Without this, `Array.prototype.slice`
/// read off the singleton's empty proto object as `undefined` and the
/// chained `.call` access threw
/// `Cannot read properties of undefined (reading 'call')` at module init.
extern "C" fn array_prototype_slice_thunk(
    _closure: *const crate::closure::ClosureHeader,
    start_val: f64,
    end_val: f64,
) -> f64 {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let arr_ptr = if this_jsv.is_pointer() {
        this_jsv.as_pointer::<crate::array::ArrayHeader>()
    } else {
        // Tolerate raw-i64-encoded array receivers (some module-init
        // call sites stash array pointers in IMPLICIT_THIS without
        // NaN-boxing). The clean_arr_ptr check inside js_array_slice
        // re-validates.
        let raw = this_bits as *const crate::array::ArrayHeader;
        if (raw as usize) > 0x10000 {
            raw
        } else {
            std::ptr::null()
        }
    };
    if arr_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let start_jsv = JSValue::from_bits(start_val.to_bits());
    let end_jsv = JSValue::from_bits(end_val.to_bits());
    let start_i32 = if start_jsv.is_undefined() {
        0
    } else {
        let n = start_jsv.to_number();
        if n.is_nan() {
            0
        } else {
            n as i32
        }
    };
    let end_i32 = if end_jsv.is_undefined() {
        i32::MAX
    } else {
        let n = end_jsv.to_number();
        if n.is_nan() {
            0
        } else {
            n as i32
        }
    };
    let result = crate::array::js_array_slice(arr_ptr, start_i32, end_i32);
    f64::from_bits(crate::value::js_nanbox_pointer(result as i64).to_bits())
}

/// Resolve the `IMPLICIT_THIS` receiver to a `(typed-array ptr, kind)` if it
/// is a typed array, else `None`. Backs the `%TypedArray%.prototype` accessor
/// getters installed for reflection (#2060) — these fire when user code does
/// `desc.get.call(int8arr)` after pulling the descriptor out via
/// `Object.getOwnPropertyDescriptor`. Mirrors the receiver-extraction the
/// `Array.prototype.slice` thunk uses (NaN-boxed pointer or raw-i64 form).
fn typed_array_receiver() -> Option<(*const crate::typedarray::TypedArrayHeader, u8)> {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    let kind = crate::typedarray::lookup_typed_array_kind(raw)?;
    Some((raw as *const crate::typedarray::TypedArrayHeader, kind))
}

/// `%TypedArray%.prototype.length` getter — element count of the receiver.
extern "C" fn typed_array_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, _)) => {
            let len = crate::typedarray::js_typed_array_length(ta);
            f64::from_bits(crate::value::JSValue::number(len as f64).bits())
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// `%TypedArray%.prototype.byteLength` getter — `length * BYTES_PER_ELEMENT`.
extern "C" fn typed_array_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, kind)) => {
            let len = crate::typedarray::js_typed_array_length(ta) as usize;
            let elem_size = crate::typedarray::elem_size_for_kind(kind);
            f64::from_bits(crate::value::JSValue::number((len * elem_size) as f64).bits())
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// `%TypedArray%.prototype.byteOffset` getter — always 0 (Perry views are not
/// backed by an offset into a shared `ArrayBuffer`).
extern "C" fn typed_array_byte_offset_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some(_) => f64::from_bits(crate::value::JSValue::number(0.0).bits()),
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// `%TypedArray%.prototype.buffer` getter. Perry does not yet model a
/// first-class `ArrayBuffer` behind a view, so this returns `undefined` for
/// now (matching the existing `int8arr.buffer` data-path behavior). The
/// accessor still exists so reflection sees a real getter — closing the
/// `getOwnPropertyDescriptor(...).get` cascade in #2060.
extern "C" fn typed_array_buffer_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Install the four `%TypedArray%.prototype` accessor descriptors
/// (`length`, `byteLength`, `byteOffset`, `buffer`) on a typed-array
/// constructor's prototype object so `Object.getOwnPropertyDescriptor`
/// reflects them as `{ get, set: undefined, enumerable: false,
/// configurable: true }`. #2060.
fn install_typed_array_proto_accessors(proto_obj: *mut ObjectHeader) {
    unsafe {
        // 0-arg getters: `.call(this)` forwards 0 user args.
        let mk = |f: *const u8| -> u64 {
            crate::closure::js_register_closure_arity(f, 0);
            let c = crate::closure::js_closure_alloc(f, 0);
            if c.is_null() {
                0
            } else {
                crate::value::js_nanbox_pointer(c as i64).to_bits()
            }
        };
        install_builtin_getter(
            proto_obj,
            "length",
            mk(typed_array_length_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "byteLength",
            mk(typed_array_byte_length_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "byteOffset",
            mk(typed_array_byte_offset_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "buffer",
            mk(typed_array_buffer_getter_thunk as *const u8),
        );
    }
}

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
fn populate_global_this_builtins(singleton: *mut ObjectHeader) {
    if singleton.is_null() {
        return;
    }
    let proto_key_bytes = b"prototype";
    let proto_key =
        crate::string::js_string_from_bytes(proto_key_bytes.as_ptr(), proto_key_bytes.len() as u32);
    // Constructors: ClosureHeader-backed so typeof is "function".
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        let func_ptr = if name == "String" {
            global_this_string_thunk as *const u8
        } else {
            global_this_builtin_noop_thunk as *const u8
        };
        let closure_ptr = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure_ptr.is_null() {
            continue;
        }
        if name == "String" {
            crate::closure::js_register_closure_arity(func_ptr, 1);
        }
        // Stash `prototype` on the closure's dynamic-prop side table.
        // `js_object_set_field_by_name` detects the CLOSURE_MAGIC tag
        // at offset 12 and dispatches into `closure_set_dynamic_prop`
        // for us; both reads and writes share that side table.
        let proto_obj = js_object_alloc(0, 0);
        if !proto_obj.is_null() {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            js_object_set_field_by_name(closure_ptr as *mut ObjectHeader, proto_key, proto_value);
            // Populate well-known method properties on the prototype
            // (currently just `Array.prototype.slice`). Methods are
            // ClosureHeader-backed thunks that read their receiver from
            // `IMPLICIT_THIS` and dispatch to the corresponding native
            // entry point — works in tandem with `.call`/`.apply` since
            // those arms (#970) rebind IMPLICIT_THIS before forwarding.
            populate_builtin_prototype_methods(name, proto_obj);
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        js_object_set_field_by_name(singleton, name_key, ctor_value);
    }
    // Namespaces: plain ObjectHeader so typeof is "object" per spec.
    for name in GLOBAL_THIS_BUILTIN_NAMESPACES.iter().copied() {
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let ns_value = if matches!(name, "console" | "process") {
            js_create_native_module_namespace(name_bytes.as_ptr(), name_bytes.len())
        } else {
            let ns_obj = js_object_alloc(0, 0);
            if ns_obj.is_null() {
                continue;
            }
            crate::value::js_nanbox_pointer(ns_obj as i64)
        };
        js_object_set_field_by_name(singleton, name_key, ns_value);
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
}

/// Install a method on a prototype object as a callable closure value with
/// the proper `name` property and registered arity. Used to reify built-in
/// prototype methods so `Array.prototype.map`, `Date.prototype.toISOString`,
/// etc. read back as `typeof === "function"` (issue #2142) — the actual
/// method *call* path is already covered by codegen's NativeMethodCall and
/// the `try_builtin_prototype_method_apply_call` HIR rewrite, so the no-op
/// thunk backing here is only invoked when user code calls the method
/// through indirection (`const m = Array.prototype.map; m.call(arr, fn)`),
/// a rare pattern. The reification is the value-read parity win.
///
/// `func_ptr` defaults to `global_this_builtin_noop_thunk` (returns
/// undefined) for methods we don't have a dedicated thunk for; callers
/// that want spec-accurate call behavior pass a custom thunk instead
/// (`array_prototype_slice_thunk`, `object_prototype_to_string_thunk`).
fn install_proto_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::native_module::set_bound_native_closure_name(closure, method_name);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(proto_obj, key, value);
}

/// Install a list of `(method_name, arity)` pairs on a prototype object,
/// each backed by `global_this_builtin_noop_thunk`. The shared no-op thunk
/// is fine because every method shares the same backing func pointer (the
/// arity registration on that pointer is overwritten harmlessly with each
/// call — the last winner is whichever arity matches the dominant call
/// site, but no current code path depends on the registered arity for the
/// noop thunk; the real dispatch arms each register their own arity on
/// their own thunk pointer).
fn install_noop_proto_methods(proto_obj: *mut ObjectHeader, methods: &[(&str, u32)]) {
    for (name, arity) in methods.iter().copied() {
        install_proto_method(
            proto_obj,
            name,
            global_this_builtin_noop_thunk as *const u8,
            arity,
        );
    }
}

/// Universal `Object.prototype` methods inherited by every receiver in
/// JS. Installed on every built-in constructor's prototype since Perry's
/// prototype chain on these built-ins doesn't walk back up to a shared
/// `Object.prototype` — so `Number.prototype.hasOwnProperty` would
/// otherwise be missing.
const OBJECT_PROTO_METHODS: &[(&str, u32)] = &[
    ("hasOwnProperty", 1),
    ("isPrototypeOf", 1),
    ("propertyIsEnumerable", 1),
    ("toLocaleString", 0),
    ("valueOf", 0),
    // `toString` is installed separately on Object/typed arrays etc. with
    // dedicated thunks; do not include it here to avoid clobbering those.
];

/// Populate well-known method properties on a built-in constructor's
/// prototype object. Each registered method is a closure carrying a
/// proper `name` property so feature-detection idioms like
/// `typeof Array.prototype.map === "function"` and `.name === "map"`
/// agree with Node when the value is read through indirection.
///
/// Two of these methods retain dedicated thunks for spec-accurate call
/// behavior — `Array.prototype.slice` (ramda's curry/variadic helpers
/// reach through `Array.prototype.slice.call(args, …)` and depend on it
/// returning a real sliced array, even via indirection) and
/// `Object.prototype.toString` (ramda's `_isArguments.js` IIFE calls
/// `Object.prototype.toString.call(arguments)` at module-init time).
/// All other methods are noop-backed: typeof + `.name` introspection
/// works, but a stored-and-called-indirect reference returns undefined.
/// The common forms — `arr.map(fn)` (codegen's NativeMethodCall) and
/// `Array.prototype.map.call(arr, fn)` (HIR rewrite, see
/// `try_builtin_prototype_method_apply_call`) — are unaffected.
fn populate_builtin_prototype_methods(builtin_name: &str, proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    match builtin_name {
        "Array" => {
            install_proto_method(
                proto_obj,
                "slice",
                array_prototype_slice_thunk as *const u8,
                2,
            );
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("at", 1),
                    ("concat", 1),
                    ("copyWithin", 2),
                    ("entries", 0),
                    ("every", 1),
                    ("fill", 1),
                    ("filter", 1),
                    ("find", 1),
                    ("findIndex", 1),
                    ("findLast", 1),
                    ("findLastIndex", 1),
                    ("flat", 0),
                    ("flatMap", 1),
                    ("forEach", 1),
                    ("includes", 1),
                    ("indexOf", 1),
                    ("join", 1),
                    ("keys", 0),
                    ("lastIndexOf", 1),
                    ("map", 1),
                    ("pop", 0),
                    ("push", 1),
                    ("reduce", 1),
                    ("reduceRight", 1),
                    ("reverse", 0),
                    ("shift", 0),
                    ("some", 1),
                    ("sort", 1),
                    ("splice", 2),
                    ("toLocaleString", 0),
                    ("toReversed", 0),
                    ("toSorted", 1),
                    ("toSpliced", 2),
                    ("toString", 0),
                    ("unshift", 1),
                    ("values", 0),
                    ("with", 2),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Object" => {
            install_proto_method(
                proto_obj,
                "toString",
                object_prototype_to_string_thunk as *const u8,
                0,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Function" => {
            install_noop_proto_methods(
                proto_obj,
                &[("apply", 2), ("bind", 1), ("call", 1), ("toString", 0)],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "String" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("at", 1),
                    ("charAt", 1),
                    ("charCodeAt", 1),
                    ("codePointAt", 1),
                    ("concat", 1),
                    ("endsWith", 1),
                    ("includes", 1),
                    ("indexOf", 1),
                    ("isWellFormed", 0),
                    ("lastIndexOf", 1),
                    ("localeCompare", 1),
                    ("match", 1),
                    ("matchAll", 1),
                    ("normalize", 0),
                    ("padEnd", 1),
                    ("padStart", 1),
                    ("repeat", 1),
                    ("replace", 2),
                    ("replaceAll", 2),
                    ("search", 1),
                    ("slice", 2),
                    ("split", 2),
                    ("startsWith", 1),
                    ("substring", 2),
                    ("toLocaleLowerCase", 0),
                    ("toLocaleUpperCase", 0),
                    ("toLowerCase", 0),
                    ("toString", 0),
                    ("toUpperCase", 0),
                    ("toWellFormed", 0),
                    ("trim", 0),
                    ("trimEnd", 0),
                    ("trimStart", 0),
                    ("valueOf", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Number" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("toExponential", 1),
                    ("toFixed", 1),
                    ("toLocaleString", 0),
                    ("toPrecision", 1),
                    ("toString", 1),
                    ("valueOf", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Boolean" => {
            install_noop_proto_methods(proto_obj, &[("toString", 0), ("valueOf", 0)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Date" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("getDate", 0),
                    ("getDay", 0),
                    ("getFullYear", 0),
                    ("getHours", 0),
                    ("getMilliseconds", 0),
                    ("getMinutes", 0),
                    ("getMonth", 0),
                    ("getSeconds", 0),
                    ("getTime", 0),
                    ("getTimezoneOffset", 0),
                    ("getUTCDate", 0),
                    ("getUTCDay", 0),
                    ("getUTCFullYear", 0),
                    ("getUTCHours", 0),
                    ("getUTCMilliseconds", 0),
                    ("getUTCMinutes", 0),
                    ("getUTCMonth", 0),
                    ("getUTCSeconds", 0),
                    ("getYear", 0),
                    ("setDate", 1),
                    ("setFullYear", 3),
                    ("setHours", 4),
                    ("setMilliseconds", 1),
                    ("setMinutes", 3),
                    ("setMonth", 2),
                    ("setSeconds", 2),
                    ("setTime", 1),
                    ("setUTCDate", 1),
                    ("setUTCFullYear", 3),
                    ("setUTCHours", 4),
                    ("setUTCMilliseconds", 1),
                    ("setUTCMinutes", 3),
                    ("setUTCMonth", 2),
                    ("setUTCSeconds", 2),
                    ("setYear", 1),
                    ("toDateString", 0),
                    ("toISOString", 0),
                    ("toJSON", 1),
                    ("toLocaleDateString", 0),
                    ("toLocaleString", 0),
                    ("toLocaleTimeString", 0),
                    ("toString", 0),
                    ("toTimeString", 0),
                    ("toUTCString", 0),
                    ("valueOf", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "RegExp" => {
            install_noop_proto_methods(
                proto_obj,
                &[("exec", 1), ("test", 1), ("toString", 0), ("compile", 2)],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Promise" => {
            install_noop_proto_methods(proto_obj, &[("catch", 1), ("finally", 1), ("then", 2)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Map" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("clear", 0),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("get", 1),
                    ("has", 1),
                    ("keys", 0),
                    ("set", 2),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Set" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("add", 1),
                    ("clear", 0),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("has", 1),
                    ("keys", 0),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "WeakMap" => {
            install_noop_proto_methods(
                proto_obj,
                &[("delete", 1), ("get", 1), ("has", 1), ("set", 2)],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "WeakSet" => {
            install_noop_proto_methods(proto_obj, &[("add", 1), ("delete", 1), ("has", 1)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError"
        | "URIError" => {
            install_noop_proto_methods(proto_obj, &[("toString", 0)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        // Typed-array constructors: install the `%TypedArray%.prototype`
        // accessor descriptors (`length`/`byteLength`/`byteOffset`/`buffer`)
        // on the per-kind prototype object (#2060), plus the shared
        // %TypedArray%.prototype method set (#2142).
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array" | "BigInt64Array"
        | "BigUint64Array" => {
            install_typed_array_proto_accessors(proto_obj);
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("at", 1),
                    ("copyWithin", 2),
                    ("entries", 0),
                    ("every", 1),
                    ("fill", 1),
                    ("filter", 1),
                    ("find", 1),
                    ("findIndex", 1),
                    ("findLast", 1),
                    ("findLastIndex", 1),
                    ("forEach", 1),
                    ("includes", 1),
                    ("indexOf", 1),
                    ("join", 1),
                    ("keys", 0),
                    ("lastIndexOf", 1),
                    ("map", 1),
                    ("reduce", 1),
                    ("reduceRight", 1),
                    ("reverse", 0),
                    ("set", 2),
                    ("slice", 2),
                    ("some", 1),
                    ("sort", 1),
                    ("subarray", 2),
                    ("toLocaleString", 0),
                    ("toReversed", 0),
                    ("toSorted", 1),
                    ("toString", 0),
                    ("values", 0),
                    ("with", 2),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        _ => {}
    }
}
