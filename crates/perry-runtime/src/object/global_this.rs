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
        match crate::gc::runtime_compare_exchange_root_atomic_raw_i64(
            &GLOBAL_THIS_PTR,
            0,
            new_ptr,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
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
    "AggregateError",
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
    "Float16Array",
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
    "EventTarget",
    "Crypto",
    "CryptoKey",
    "SubtleCrypto",
    "FormData",
    "Blob",
    "File",
    "Headers",
    "Request",
    "Response",
    "MessageChannel",
    "MessagePort",
    "BroadcastChannel",
    "FinalizationRegistry",
    // #2875: TC39 explicit-resource-management globals. Backed by the
    // no-op constructor thunk so `typeof DisposableStack === "function"`;
    // real `new DisposableStack()` / `new SuppressedError(...)` flow through
    // codegen's `lower_builtin_new` to the dedicated runtime ctors.
    "DisposableStack",
    "AsyncDisposableStack",
    "SuppressedError",
    "Buffer",
];

/// #3655: spec `length` (declared-parameter count) for each built-in
/// constructor, so `Ctor.length` reads the right arity through the runtime
/// value path (`const C = DataView; C.length === 1`) and
/// `Object.getOwnPropertyDescriptor(Ctor, 'length').value` matches Node. The
/// HIR also folds bare `Ctor.length` constants (`analysis::builtin_constructor_length`);
/// these are the runtime fallback for rebound / passed-as-value constructors.
/// Values verified against `node --experimental-strip-types`. Unlisted names
/// fall through to the closure arity registry (0).
pub(crate) fn builtin_constructor_spec_length(name: &str) -> Option<u32> {
    let len = match name {
        "Symbol"
        | "Map"
        | "Set"
        | "WeakMap"
        | "WeakSet"
        | "TextEncoder"
        | "TextDecoder"
        | "TextEncoderStream"
        | "TextDecoderStream"
        | "URLSearchParams"
        | "AbortController"
        | "AbortSignal"
        | "FormData"
        | "Blob"
        | "Headers"
        | "Response"
        | "MessageChannel"
        | "MessagePort"
        | "DisposableStack"
        | "AsyncDisposableStack" => 0,
        "Array"
        | "Object"
        | "String"
        | "Number"
        | "Boolean"
        | "Function"
        | "Error"
        | "TypeError"
        | "RangeError"
        | "SyntaxError"
        | "ReferenceError"
        | "EvalError"
        | "URIError"
        | "WeakRef"
        | "BigInt"
        | "ArrayBuffer"
        | "SharedArrayBuffer"
        | "DataView"
        | "URL"
        | "Request"
        | "BroadcastChannel"
        | "FinalizationRegistry"
        | "Promise" => 1,
        "RegExp" | "Proxy" | "AggregateError" | "File" => 2,
        "Date" => 7,
        "SuppressedError" | "Buffer" | "Uint8Array" | "Int8Array" | "Uint16Array"
        | "Int16Array" | "Uint32Array" | "Int32Array" | "Float16Array" | "Float32Array"
        | "Float64Array" | "Uint8ClampedArray" | "BigInt64Array" | "BigUint64Array" => 3,
        _ => return None,
    };
    Some(len)
}

/// JS built-in namespaces (typeof === "object", not "function"). Same
/// shape on the singleton — a backing object with `prototype` so chained
/// reads degrade gracefully — but typeof reports "object".
pub(crate) const GLOBAL_THIS_BUILTIN_NAMESPACES: &[&str] =
    &["console", "process", "Math", "JSON", "Reflect"];

// Note: `navigator` (#2923) is installed on the singleton directly (see
// `populate_global_this_builtins`) rather than via this generic namespace
// loop because it needs its own field-populated object, not an empty stub.

/// JS global built-in functions exposed as function-valued properties on
/// `globalThis`. Unlike constructor sentinels, these call through to Perry's
/// real direct-call runtime helpers so rebinding works:
/// `const clone = globalThis.structuredClone; clone(value)`.
pub(crate) const GLOBAL_THIS_BUILTIN_FUNCTIONS: &[&str] = &[
    "fetch",
    "structuredClone",
    "atob",
    "btoa",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "setImmediate",
    "clearImmediate",
    "queueMicrotask",
    // #2905: standard global helper functions. These route through Perry's
    // real direct-call runtime helpers, so `const p = parseInt; p("42px")`
    // and `globalThis.encodeURIComponent("a b")` match Node.
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "encodeURI",
    "decodeURI",
    "encodeURIComponent",
    "decodeURIComponent",
];

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

extern "C" fn global_this_object_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() || js_value.is_null() {
        return crate::value::js_nanbox_pointer(js_object_alloc(0, 0) as i64);
    }
    if js_value.is_bigint() {
        return crate::builtins::js_boxed_bigint_new(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return crate::builtins::js_boxed_symbol_new(value);
    }
    if crate::value::js_nanbox_get_pointer(value) != 0 {
        return value;
    }
    crate::value::js_nanbox_pointer(js_object_alloc(0, 0) as i64)
}

extern "C" fn global_this_structured_clone_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _options: f64,
) -> f64 {
    crate::builtins::js_structured_clone(value)
}

extern "C" fn global_this_atob_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let decoded = crate::string::js_atob(value);
    crate::value::js_nanbox_string(decoded as i64)
}

extern "C" fn global_this_btoa_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let encoded = crate::string::js_btoa(value);
    crate::value::js_nanbox_string(encoded as i64)
}

extern "C" fn math_f16round_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::math::js_math_f16round(value)
}

// #2905: thunks for the standard global helper functions. Each coerces its
// arguments the same way the bare-call HIR lowering does and forwards to the
// shared runtime helper so a rebound / property-read reference matches Node.

extern "C" fn global_this_parse_int_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    radix: f64,
) -> f64 {
    let s = crate::builtins::js_string_coerce(value);
    crate::builtins::js_parse_int(s, radix)
}

extern "C" fn global_this_parse_float_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let s = crate::builtins::js_string_coerce(value);
    crate::builtins::js_parse_float(s)
}

extern "C" fn global_this_is_nan_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_is_nan(value)
}

extern "C" fn global_this_is_finite_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_is_finite(value)
}

extern "C" fn global_this_encode_uri_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_encode_uri(value))
}

extern "C" fn global_this_decode_uri_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_decode_uri(value))
}

extern "C" fn global_this_encode_uri_component_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_encode_uri_component(value))
}

extern "C" fn global_this_decode_uri_component_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_decode_uri_component(value))
}

// #2889: call-form thunks for `Number`/`Boolean` global constructor values.
// `Object`/`String` already have dedicated thunks above; these mirror the
// bare-call HIR lowering (`Expr::NumberCoerce` / `Expr::BooleanCoerce`) so
// `const N = Number; N("42")` and `const B = Boolean; B(0)` match Node.
extern "C" fn global_this_number_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let jsv = crate::value::JSValue::from_bits(value.to_bits());
    if jsv.is_undefined() {
        // `Number()` with no args returns 0; an explicit `undefined` arg → NaN.
        // The closure-call path zero-fills missing args with TAG_UNDEFINED, so
        // we can't distinguish — match the common `Number()` → 0 case.
        return f64::from_bits(crate::value::JSValue::number(0.0).bits());
    }
    crate::builtins::js_number_coerce(value)
}

extern "C" fn global_this_boolean_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let b = crate::value::js_is_truthy(value) != 0;
    f64::from_bits(crate::value::JSValue::bool(b).bits())
}

extern "C" fn global_this_error_capture_stack_trace_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    constructor_opt: f64,
) -> f64 {
    crate::error::js_error_capture_stack_trace(target, constructor_opt)
}

/// #2904: `Error.isError(value)` thunk — delegates to the runtime duck-check.
extern "C" fn global_this_error_is_error_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::error::js_error_is_error(value)
}

/// #2904: `Error.prepareStackTrace` default — Node leaves a hook here that
/// formats the stack from structured frames. Perry's stack strings are
/// coarse; the installed default returns the existing `error.stack` string
/// (or empty) so `typeof Error.prepareStackTrace === "function"` holds and
/// callers that invoke it get a usable string rather than a crash.
extern "C" fn global_this_error_prepare_stack_trace_thunk(
    _closure: *const crate::closure::ClosureHeader,
    error: f64,
    _structured_stack: f64,
) -> f64 {
    let jsval = crate::value::JSValue::from_bits(error.to_bits());
    if jsval.is_pointer() {
        let ptr = crate::value::js_nanbox_get_pointer(error) as *mut crate::error::ErrorHeader;
        if !ptr.is_null() {
            let stack = crate::error::js_error_get_stack(ptr);
            if !stack.is_null() {
                return crate::value::js_nanbox_string(stack as i64);
            }
        }
    }
    let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    crate::value::js_nanbox_string(empty as i64)
}

pub(super) fn global_this_rest_array_values(rest: f64) -> Vec<f64> {
    let value = crate::value::JSValue::from_bits(rest.to_bits());
    if !value.is_pointer() {
        return Vec::new();
    }
    let arr = value.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(arr);
    (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect()
}

extern "C" fn global_this_set_timeout_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    delay: f64,
    rest: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 0) };
    let args = global_this_rest_array_values(rest);
    if args.is_empty() {
        crate::value::js_nanbox_pointer(crate::timer::js_set_timeout_callback(callback, delay))
    } else {
        crate::value::js_nanbox_pointer(unsafe {
            crate::timer::js_set_timeout_callback_args(
                callback,
                delay,
                args.as_ptr(),
                args.len() as i32,
            )
        })
    }
}

extern "C" fn global_this_clear_timeout_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn global_this_set_interval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    delay: f64,
    rest: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 1) };
    let args = global_this_rest_array_values(rest);
    if args.is_empty() {
        crate::value::js_nanbox_pointer(crate::timer::setInterval(callback, delay))
    } else {
        crate::value::js_nanbox_pointer(unsafe {
            crate::timer::js_set_interval_callback_args(
                callback,
                delay,
                args.as_ptr(),
                args.len() as i32,
            )
        })
    }
}

extern "C" fn global_this_clear_interval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_interval_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn global_this_set_immediate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    rest: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 2) };
    let args = global_this_rest_array_values(rest);
    if args.is_empty() {
        crate::value::js_nanbox_pointer(crate::timer::js_set_immediate_callback(callback))
    } else {
        crate::value::js_nanbox_pointer(unsafe {
            crate::timer::js_set_immediate_callback_args(callback, args.as_ptr(), args.len() as i32)
        })
    }
}

extern "C" fn global_this_clear_immediate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_immediate_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn global_this_queue_microtask_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 3) };
    crate::builtins::js_queue_microtask(callback);
    f64::from_bits(crate::value::TAG_UNDEFINED)
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

extern "C" fn object_prototype_is_prototype_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    f64::from_bits(
        JSValue::bool(unsafe { super::js_object_is_prototype_of_value(this_value, value) }).bits(),
    )
}

extern "C" fn object_prototype_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    unsafe { super::js_object_default_value_of(this_value) }
}

extern "C" fn object_prototype_to_locale_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    unsafe { super::js_object_default_to_locale_string(this_value) }
}

unsafe fn function_apply_args(args_array: f64) -> Vec<f64> {
    let value = JSValue::from_bits(args_array.to_bits());
    if value.is_undefined() || value.is_null() {
        return Vec::new();
    }
    let is_array = JSValue::from_bits(crate::array::js_array_is_array(args_array).to_bits());
    if !is_array.is_bool() || !is_array.as_bool() {
        return Vec::new();
    }
    let arr = if value.is_pointer() {
        value.as_pointer::<crate::array::ArrayHeader>()
    } else if (args_array.to_bits() >> 48) == 0 {
        args_array.to_bits() as *const crate::array::ArrayHeader
    } else {
        std::ptr::null()
    };
    if arr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(arr) as usize;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        out.push(f64::from_bits(
            crate::array::js_array_get(arr, i as u32).bits(),
        ));
    }
    out
}

extern "C" fn function_prototype_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    rest_array: f64,
) -> f64 {
    unsafe {
        let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
        let args = function_apply_args(rest_array);
        let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
        let result = crate::closure::js_native_call_value(target, args.as_ptr(), args.len());
        IMPLICIT_THIS.with(|c| c.set(prev_this));
        result
    }
}

extern "C" fn function_prototype_apply_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    args_array: f64,
) -> f64 {
    unsafe {
        let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
        let args = function_apply_args(args_array);
        let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
        let result = crate::closure::js_native_call_value(target, args.as_ptr(), args.len());
        IMPLICIT_THIS.with(|c| c.set(prev_this));
        result
    }
}

/// Thunk for `Array.prototype.slice` exposed as a real callable closure
/// value. Reads the array receiver from `IMPLICIT_THIS` (set by
/// `Function.prototype.call`/`.apply`'s runtime arm in
/// `js_native_call_method`) and forwards to the shared slice-value helper.
///
/// Coerces start/end through the shared array slice helper, with
/// `undefined` mapping to `0` for start and end-of-array for end — matching
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
    let result = crate::array::js_array_slice_values(arr_ptr, start_val, end_val);
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

/// Allocate the shared `%TypedArray%` intrinsic constructor (a closure) and
/// its `.prototype` object, cache both in the GC-rooted atomics, and wire the
/// closure's `prototype` dynamic-prop to point at the shared prototype.
///
/// Spec: `%TypedArray%` is the abstract parent constructor for `Int8Array`,
/// `Uint8Array`, … — `Int8Array.__proto__ === %TypedArray%` and
/// `Object.getPrototypeOf(Int8Array.prototype) === %TypedArray%.prototype`.
/// Perry didn't model this before #2145, so test262's TypedArray-prototype
/// walks read `null.prototype` and the constructor's `__proto__` returned the
/// `0.0` no-value placeholder (`typeof Int8Array.__proto__ === "number"`).
///
/// Idempotent: subsequent calls return the cached pointer. Called from
/// `populate_global_this_builtins` (single-threaded under the singleton CAS),
/// so the AtomicI64 stores don't need to race-resolve.
fn ensure_typed_array_intrinsic() -> (*mut crate::closure::ClosureHeader, *mut ObjectHeader) {
    let existing_ctor = crate::object::TYPED_ARRAY_INTRINSIC_PTR.load(Ordering::Acquire);
    let existing_proto = crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
    if existing_ctor != 0 && existing_proto != 0 {
        return (
            existing_ctor as *mut crate::closure::ClosureHeader,
            existing_proto as *mut ObjectHeader,
        );
    }
    let ctor = crate::closure::js_closure_alloc(global_this_builtin_noop_thunk as *const u8, 0);
    let proto = js_object_alloc(0, 0);
    if ctor.is_null() || proto.is_null() {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    // Wire `%TypedArray%.prototype` so `getPrototypeOf(Int8Array).prototype`
    // hits a real object instead of undefined.
    let proto_key_bytes = b"prototype";
    let proto_key =
        crate::string::js_string_from_bytes(proto_key_bytes.as_ptr(), proto_key_bytes.len() as u32);
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, proto_key, proto_value);
    super::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        super::PropertyAttrs::new(false, false, false),
    );
    // #2060: the four reflectable `length`/`byteLength`/`byteOffset`/`buffer`
    // accessor descriptors are own properties of `%TypedArray%.prototype` per
    // spec, NOT of the per-kind proto. Pre-#2145 they were installed on each
    // per-kind proto because `getPrototypeOf(per_kind_proto)` returned the
    // per-kind proto itself (identity), so the same lookup happened to land
    // there. After #2145 wires the per-kind protos to share the intrinsic
    // proto, the descriptors must live on the intrinsic itself for
    // `Object.getOwnPropertyDescriptor(getPrototypeOf(Int8Array.prototype),
    // "length")` to keep working.
    install_typed_array_proto_accessors(proto);
    crate::object::TYPED_ARRAY_INTRINSIC_PTR.store(ctor as i64, Ordering::Release);
    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.store(proto as i64, Ordering::Release);
    (ctor, proto)
}

/// Public accessor for the `%TypedArray%.prototype` object. Returns the cached
/// pointer if `populate_global_this_builtins` has run (so the intrinsic is
/// initialised), else null. Used by `js_object_get_prototype_of` to resolve
/// `Object.getPrototypeOf(Int8Array.prototype)` to the shared prototype.
pub(crate) fn typed_array_intrinsic_proto_ptr() -> *mut ObjectHeader {
    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as *mut ObjectHeader
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
    {
        let name = b"globalThis";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = crate::value::js_nanbox_pointer(singleton as i64);
        js_object_set_field_by_name(singleton, key, value);
    }
    // #2145: pre-allocate the shared `%TypedArray%` intrinsic so per-kind
    // typed-array constructors can link their `__proto__` to it as they're
    // built below, and the per-kind `.prototype` objects can be flagged with
    // `OBJ_FLAG_TYPED_ARRAY_PROTO` for `Object.getPrototypeOf` resolution.
    let (typed_array_intrinsic_ctor, _) = ensure_typed_array_intrinsic();
    // Constructors: ClosureHeader-backed so typeof is "function".
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        if name == "Buffer" {
            let name_bytes = name.as_bytes();
            let name_key =
                crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            let ctor_value = super::native_module::buffer_constructor_value();
            js_object_set_field_by_name(singleton, name_key, ctor_value);
            super::set_builtin_property_attrs(
                singleton as usize,
                name.to_string(),
                super::PropertyAttrs::new(true, false, true),
            );
            continue;
        }
        let func_ptr = match name {
            "Object" => global_this_object_thunk as *const u8,
            "String" => global_this_string_thunk as *const u8,
            // #2889: call-form `Number(x)` / `Boolean(x)` through a rebound
            // global value coerce like the bare-call lowering does.
            "Number" => global_this_number_thunk as *const u8,
            "Boolean" => global_this_boolean_thunk as *const u8,
            "MessageChannel" => {
                crate::messaging::js_message_channel_constructor_call_error as *const u8
            }
            "MessagePort" => crate::messaging::js_message_port_constructor_call_error as *const u8,
            "BroadcastChannel" => {
                crate::messaging::js_broadcast_channel_constructor_call_error as *const u8
            }
            _ => global_this_builtin_noop_thunk as *const u8,
        };
        let closure_ptr = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure_ptr.is_null() {
            continue;
        }
        match name {
            "Object" | "String" | "Number" | "Boolean" | "BroadcastChannel" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "MessageChannel" | "MessagePort" => {
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
        super::native_module::set_bound_native_closure_name(closure_ptr, name);
        if let Some(len) = builtin_constructor_spec_length(name) {
            super::native_module::set_builtin_closure_length(closure_ptr as usize, len);
        }
        super::set_builtin_property_attrs(
            closure_ptr as usize,
            "name".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        super::set_builtin_property_attrs(
            closure_ptr as usize,
            "length".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        if name == "Error" {
            install_error_static_methods(closure_ptr);
        }
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        // Stash `prototype` on the closure's dynamic-prop side table.
        // `js_object_set_field_by_name` detects the CLOSURE_MAGIC tag
        // at offset 12 and dispatches into `closure_set_dynamic_prop`
        // for us; both reads and writes share that side table.
        let proto_obj = js_object_alloc(0, 0);
        if !proto_obj.is_null() {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            js_object_set_field_by_name(closure_ptr as *mut ObjectHeader, proto_key, proto_value);
            super::set_builtin_property_attrs(
                closure_ptr as usize,
                "prototype".to_string(),
                super::PropertyAttrs::new(false, false, false),
            );
            // Populate well-known method properties on the prototype
            // (currently just `Array.prototype.slice`). Methods are
            // ClosureHeader-backed thunks that read their receiver from
            // `IMPLICIT_THIS` and dispatch to the corresponding native
            // entry point — works in tandem with `.call`/`.apply` since
            // those arms (#970) rebind IMPLICIT_THIS before forwarding.
            populate_builtin_prototype_methods(name, proto_obj);
            if matches!(name, "MessageChannel" | "MessagePort" | "BroadcastChannel") {
                crate::messaging::populate_messaging_prototype(name, proto_obj, ctor_value);
            }
            if matches!(name, "Crypto" | "CryptoKey" | "SubtleCrypto") {
                super::native_module::install_webcrypto_constructor_proto(proto_obj, ctor_value);
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
            }
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        js_object_set_field_by_name(singleton, name_key, ctor_value);
    }
    // Callable global functions: ClosureHeader-backed values with real
    // dispatch so direct property reads and rebound calls match bare calls.
    for name in GLOBAL_THIS_BUILTIN_FUNCTIONS.iter().copied() {
        let (func_ptr, arity, has_rest) = match name {
            "fetch" => (
                super::global_fetch::global_this_fetch_thunk as *const u8,
                1,
                true,
            ),
            "structuredClone" => (global_this_structured_clone_thunk as *const u8, 2, false),
            "atob" => (global_this_atob_thunk as *const u8, 1, false),
            "btoa" => (global_this_btoa_thunk as *const u8, 1, false),
            "setTimeout" => (global_this_set_timeout_thunk as *const u8, 2, true),
            "clearTimeout" => (global_this_clear_timeout_thunk as *const u8, 1, false),
            "setInterval" => (global_this_set_interval_thunk as *const u8, 2, true),
            "clearInterval" => (global_this_clear_interval_thunk as *const u8, 1, false),
            "setImmediate" => (global_this_set_immediate_thunk as *const u8, 1, true),
            "clearImmediate" => (global_this_clear_immediate_thunk as *const u8, 1, false),
            "queueMicrotask" => (global_this_queue_microtask_thunk as *const u8, 1, false),
            // #2905: standard global helper functions.
            "parseInt" => (global_this_parse_int_thunk as *const u8, 2, false),
            "parseFloat" => (global_this_parse_float_thunk as *const u8, 1, false),
            "isNaN" => (global_this_is_nan_thunk as *const u8, 1, false),
            "isFinite" => (global_this_is_finite_thunk as *const u8, 1, false),
            "encodeURI" => (global_this_encode_uri_thunk as *const u8, 1, false),
            "decodeURI" => (global_this_decode_uri_thunk as *const u8, 1, false),
            "encodeURIComponent" => (
                global_this_encode_uri_component_thunk as *const u8,
                1,
                false,
            ),
            "decodeURIComponent" => (
                global_this_decode_uri_component_thunk as *const u8,
                1,
                false,
            ),
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
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let fn_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        js_object_set_field_by_name(singleton, name_key, fn_value);
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
            if name == "Math" {
                install_proto_method(ns_obj, "f16round", math_f16round_thunk as *const u8, 1);
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
        let value = super::native_module::bound_native_callable_export_value("perf_hooks", name);
        js_object_set_field_by_name(singleton, key, value);
    }
    super::native_module::install_global_webcrypto(singleton);
    // #2923: `globalThis.navigator` — Node's browser-compatible runtime
    // metadata object. typeof is "object". Built once per process.
    {
        let nname = b"navigator";
        let nkey = crate::string::js_string_from_bytes(nname.as_ptr(), nname.len() as u32);
        let nval = crate::navigator::js_navigator_object();
        js_object_set_field_by_name(singleton, nkey, nval);
    }
}

fn install_error_static_methods(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    let func_ptr = global_this_error_capture_stack_trace_thunk as *const u8;
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 2);
    super::native_module::set_bound_native_closure_name(closure, "captureStackTrace");

    let key = crate::string::js_string_from_bytes(b"captureStackTrace".as_ptr(), 17);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::set_builtin_property_attrs(
        ctor as usize,
        "captureStackTrace".to_string(),
        super::PropertyAttrs::new(true, false, true),
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
    super::set_builtin_property_attrs(
        ctor as usize,
        "stackTraceLimit".to_string(),
        super::PropertyAttrs::new(true, true, true),
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
    super::native_module::set_bound_native_closure_name(closure, name);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::set_builtin_property_attrs(
        ctor as usize,
        name.to_string(),
        super::PropertyAttrs::new(true, false, true),
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

fn nanbox_array_or_undef(arr: *mut crate::array::ArrayHeader) -> f64 {
    if arr.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(arr as i64)
    }
}

extern "C" fn object_keys_thunk(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    nanbox_array_or_undef(super::js_object_keys_value(value))
}

extern "C" fn object_values_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    nanbox_array_or_undef(super::js_object_values_value(value))
}

extern "C" fn object_entries_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    nanbox_array_or_undef(super::js_object_entries_value(value))
}

extern "C" fn object_freeze_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_freeze(value)
}

extern "C" fn object_create_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_create(value)
}

extern "C" fn object_get_prototype_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_get_prototype_of(value)
}

extern "C" fn object_get_own_property_names_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_get_own_property_names(value)
}

extern "C" fn object_from_entries_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_from_entries(value)
}

extern "C" fn object_assign_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    rest: f64,
) -> f64 {
    let validated = unsafe { super::js_object_assign_validate_target(target) };
    for source in global_this_rest_array_values(rest) {
        unsafe { super::js_object_assign_one(validated, source) };
    }
    validated
}

/// `Object.hasOwn(obj, key)` (ES2022) reified as a callable value so the
/// feature-detect idiom `typeof Object.hasOwn === "undefined" ? … :
/// Object.hasOwn` (iconv-lite's merge-exports, #3527) binds a real callable
/// instead of a non-callable handle. Backed by the same runtime helper as
/// `Object.prototype.hasOwnProperty.call(obj, key)`.
extern "C" fn object_hasown_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    key: f64,
) -> f64 {
    super::object_ops::js_object_has_own(obj, key)
}

extern "C" fn array_is_array_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::array::js_array_is_array(value)
}

extern "C" fn array_from_thunk(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    nanbox_array_or_undef(crate::array::js_array_from_value(value))
}

extern "C" fn array_of_thunk(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
    let vals = global_this_rest_array_values(rest);
    let len = vals.len() as u32;
    let arr = crate::array::js_array_alloc(len);
    unsafe {
        (*arr).length = len;
        for (i, &v) in vals.iter().enumerate() {
            crate::array::js_array_set_f64(arr, i as u32, v);
        }
    }
    crate::value::js_nanbox_pointer(arr as i64)
}

/// Install a single callable static method on a constructor closure as a
/// `{ writable: true, enumerable: false, configurable: true }` data property
/// (matching Node's descriptors for built-in statics). `has_rest` registers
/// the func pointer as a rest-arg closure so trailing args arrive as an array.
fn install_constructor_static(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
    has_rest: bool,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    if has_rest {
        crate::closure::js_register_closure_rest(func_ptr, arity);
    } else {
        crate::closure::js_register_closure_arity(func_ptr, arity);
    }
    super::native_module::set_bound_native_closure_name(closure, name);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::set_builtin_property_attrs(
        ctor as usize,
        name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

fn install_number_static_data_properties(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    let props = [
        ("NaN", f64::NAN),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ("MAX_VALUE", f64::MAX),
        ("MIN_VALUE", f64::MIN_POSITIVE),
        ("EPSILON", f64::EPSILON),
        ("MAX_SAFE_INTEGER", 9007199254740991.0),
        ("MIN_SAFE_INTEGER", -9007199254740991.0),
    ];
    for (name, value) in props {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
        super::set_builtin_property_attrs(
            ctor as usize,
            name.to_string(),
            super::PropertyAttrs::new(false, false, false),
        );
    }
}

/// #2889: install the common static methods on the `Object` / `Array`
/// constructor closures so rebound usage (`const O = Object; O.keys(x)`)
/// dispatches through the real runtime helpers. Only the high-traffic
/// statics with simple f64-in / f64-out shapes are reified here; the long
/// tail (`Object.defineProperty`, `Object.getOwnPropertyDescriptor`, …)
/// stays unreified on the rebound value and is a known scope gap.
fn install_builtin_constructor_statics(name: &str, ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    match name {
        "Object" => {
            install_constructor_static(ctor, "keys", object_keys_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "values", object_values_thunk as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "entries",
                object_entries_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "freeze", object_freeze_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "create", object_create_thunk as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "getPrototypeOf",
                object_get_prototype_of_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertyNames",
                object_get_own_property_names_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "fromEntries",
                object_from_entries_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "assign", object_assign_thunk as *const u8, 1, true);
            install_constructor_static(ctor, "hasOwn", object_hasown_thunk as *const u8, 2, false);
        }
        "Array" => {
            install_constructor_static(
                ctor,
                "isArray",
                array_is_array_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "from", array_from_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "of", array_of_thunk as *const u8, 0, true);
        }
        _ => {}
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
pub(super) fn install_proto_method(
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
    // #3143: record this method's spec `.length` per closure instance — all
    // noop-backed methods share one func_ptr, so the func-ptr arity registry
    // can't distinguish `map` (1) from `slice` (2). Read back by the `.length`
    // value-accessor and `getOwnPropertyDescriptor`.
    super::native_module::set_builtin_closure_length(closure as usize, arity);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(proto_obj, key, value);
    // Built-in prototype methods are `{ writable: true, enumerable: false,
    // configurable: true }` per spec. Record that descriptor (reflection-only,
    // no hot-path gate flip) so `Object.getOwnPropertyDescriptor`, `Object.keys`
    // and `for-in` all observe them as non-enumerable — Test262's `verifyProperty`
    // checks every built-in method this way. See `set_builtin_property_attrs`.
    super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
    // #3143: the method's own `.name` / `.length` data properties are
    // `{ writable: false, enumerable: false, configurable: true }` per spec.
    // Register those on the closure itself so `getOwnPropertyDescriptor(
    // Array.prototype.map, "name")` reports `writable: false` (it previously
    // read the dynamic-prop slot and defaulted to writable). Reflection-only —
    // no hot-path gate flip.
    super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
}

fn install_proto_method_rest(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    fixed_arity: u32,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_rest(func_ptr, fixed_arity);
    super::native_module::set_bound_native_closure_name(closure, method_name);
    super::native_module::set_builtin_closure_length(closure as usize, fixed_arity);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(proto_obj, key, value);
    super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
    super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
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
    // #3662: Map/Set/WeakMap/WeakSet prototypes get brand-checking thunks
    // (own module, to keep this file under the 2000-line gate).
    if collection_proto_thunks::install_collection_proto_methods(builtin_name, proto_obj) {
        install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
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
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "toLocaleString",
                object_prototype_to_locale_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                object_prototype_value_of_thunk as *const u8,
                0,
            );
            install_noop_proto_methods(
                proto_obj,
                &[("hasOwnProperty", 1), ("propertyIsEnumerable", 1)],
            );
        }
        "Function" => {
            install_proto_method(
                proto_obj,
                "apply",
                function_prototype_apply_thunk as *const u8,
                2,
            );
            install_noop_proto_methods(proto_obj, &[("bind", 1), ("toString", 0)]);
            install_proto_method_rest(
                proto_obj,
                "call",
                function_prototype_call_thunk as *const u8,
                1,
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
                    ("substr", 2),
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
        "TextEncoder" => {
            install_noop_proto_methods(proto_obj, &[("encode", 1), ("encodeInto", 2)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "TextDecoder" => {
            install_noop_proto_methods(proto_obj, &[("decode", 1)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError"
        | "URIError" => {
            install_noop_proto_methods(proto_obj, &[("toString", 0)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        // Typed-array constructors: keep the reified per-kind prototype
        // method set (#2142) on each per-kind `.prototype` so direct
        // reads like `Int8Array.prototype.at` continue to return a
        // function. The accessor descriptors
        // (`length`/`byteLength`/`byteOffset`/`buffer`) are installed
        // *only* on the shared `%TypedArray%.prototype` (#2145, in
        // `ensure_typed_array_intrinsic`) — reached via
        // `Object.getPrototypeOf(Int8Array.prototype) ===
        // %TypedArray%.prototype`. Pre-#2145 they were also stamped on
        // each per-kind proto because `getPrototypeOf(per_kind)`
        // returned identity; now that it walks to the intrinsic, they
        // belong on the parent (matches Node's
        // `getOwnPropertyDescriptor(Int8Array.prototype, "length")` =
        // `undefined`).
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array" => {
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
