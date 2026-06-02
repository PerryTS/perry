//! `globalThis` singleton plus built-in constructor/namespace population.

use super::*;

#[path = "global_this_webassembly.rs"]
mod global_this_webassembly;

/// Issue #611: lazily allocate shared `globalThis` for computed global access.
#[no_mangle]
pub extern "C" fn js_get_global_this() -> f64 {
    let cached = GLOBAL_THIS_PTR.load(Ordering::Acquire);
    let ptr = if cached != 0 {
        while !GLOBAL_THIS_READY.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
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
                // Populate constructor values for `globalThis.Array` /
                // `context.Array` style reads without changing bare `new Array`.
                populate_global_this_builtins(new_ptr as *mut ObjectHeader);
                GLOBAL_THIS_READY.store(true, Ordering::Release);
                new_ptr
            }
            Err(other) => {
                while !GLOBAL_THIS_READY.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                other
            }
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

// Note: `navigator` (#2923) is installed on the singleton directly (see
// `populate_global_this_builtins`) rather than via this generic namespace
// loop because it needs its own field-populated object, not an empty stub.

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

extern "C" fn global_this_date_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    let string = crate::date::js_date_to_string(crate::date::js_date_new());
    crate::value::js_nanbox_string(string as i64)
}

extern "C" fn global_this_eval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    source: f64,
) -> f64 {
    let source = crate::builtins::js_string_coerce(source);
    let Some(body) = (unsafe { super::has_own_helpers::str_from_string_header(source) }) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    match normalize_eval_this_body(body).as_deref() {
        Some("this" | "globalThis") => js_get_global_this(),
        Some("typeof this") => {
            let s = b"object";
            let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            crate::value::js_nanbox_string(ptr as i64)
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

fn normalize_eval_this_body(body: &str) -> Option<String> {
    let mut src = body.trim().trim_end_matches(';').trim();
    for directive in ["\"use strict\"", "'use strict'"] {
        if let Some(rest) = src.strip_prefix(directive) {
            let rest = rest.trim_start();
            if let Some(after_semicolon) = rest.strip_prefix(';') {
                src = after_semicolon.trim().trim_end_matches(';').trim();
            }
        }
    }
    if matches!(src, "this" | "globalThis" | "typeof this") {
        Some(src.to_string())
    } else {
        None
    }
}

pub(crate) extern "C" fn typed_array_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::object_ops::throw_object_type_error(b"Constructor %TypedArray% requires 'new'")
}

fn error_constructor_call(kind: u32, message: f64) -> f64 {
    let error = crate::error::js_error_new_kind_from_value(kind, message);
    crate::value::js_nanbox_pointer(error as i64)
}

extern "C" fn error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_ERROR, message)
}

extern "C" fn type_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_TYPE_ERROR, message)
}

extern "C" fn range_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_RANGE_ERROR, message)
}

extern "C" fn reference_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_REFERENCE_ERROR, message)
}

extern "C" fn syntax_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_SYNTAX_ERROR, message)
}

extern "C" fn eval_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_EVAL_ERROR, message)
}

extern "C" fn uri_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_URI_ERROR, message)
}

pub(crate) extern "C" fn webcrypto_illegal_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "Illegal constructor",
        "ERR_ILLEGAL_CONSTRUCTOR",
    )
}

#[no_mangle]
pub extern "C" fn js_webcrypto_illegal_constructor() -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "Illegal constructor",
        "ERR_ILLEGAL_CONSTRUCTOR",
    )
}

extern "C" fn global_this_crypto_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    super::native_module::webcrypto_namespace()
}

fn require_webcrypto_this() -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let jv = crate::value::JSValue::from_bits(this_value.to_bits());
    if jv.is_pointer() {
        let obj = jv.as_pointer::<ObjectHeader>();
        if !obj.is_null()
            && unsafe { (*obj).class_id } == super::native_module::NATIVE_MODULE_CLASS_ID
            && unsafe { super::native_module::read_native_module_name(obj) }
                .is_some_and(|name| name == "crypto.webcrypto")
        {
            return this_value;
        }
    }
    crate::fs::validate::throw_type_error_with_code(
        "Value of \"this\" must be of type Crypto",
        "ERR_INVALID_THIS",
    )
}

pub(crate) extern "C" fn webcrypto_get_random_values_thunk(
    _closure: *const crate::closure::ClosureHeader,
    array: f64,
) -> f64 {
    let this_value = require_webcrypto_this();
    unsafe {
        js_native_call_method(
            this_value,
            b"getRandomValues".as_ptr() as *const i8,
            "getRandomValues".len(),
            &array,
            1,
        )
    }
}

pub(crate) extern "C" fn webcrypto_random_uuid_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = require_webcrypto_this();
    unsafe {
        js_native_call_method(
            this_value,
            b"randomUUID".as_ptr() as *const i8,
            "randomUUID".len(),
            std::ptr::null(),
            0,
        )
    }
}

extern "C" fn webcrypto_subtle_getter_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    require_webcrypto_this();
    super::native_module::subtle_crypto_namespace()
}

extern "C" fn cryptokey_property_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn webcrypto_method_value(property_name: &str) -> Option<f64> {
    let (func_ptr, arity) = match property_name {
        "getRandomValues" => (webcrypto_get_random_values_thunk as *const u8, 1),
        "randomUUID" => (webcrypto_random_uuid_thunk as *const u8, 0),
        _ => return None,
    };
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    super::native_module::set_bound_native_closure_name(closure, property_name);
    super::native_module::set_builtin_closure_length(closure as usize, arity);
    Some(crate::value::js_nanbox_pointer(closure as i64))
}

extern "C" fn global_this_array_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let rest_value = crate::value::JSValue::from_bits(rest.to_bits());
    let args_arr = if rest_value.is_pointer() {
        rest_value.as_pointer::<crate::array::ArrayHeader>()
    } else {
        std::ptr::null()
    };
    let argc = crate::array::js_array_length(args_arr);
    if argc == 1 {
        let first = crate::array::js_array_get_f64(args_arr, 0);
        let arr = crate::array::js_array_constructor_single(first);
        return crate::value::js_nanbox_pointer(arr as i64);
    }
    let arr = crate::array::js_array_alloc(argc);
    unsafe {
        (*arr).length = argc;
        for i in 0..argc {
            let value = crate::array::js_array_get_f64(args_arr, i);
            crate::array::js_array_set_f64(arr, i, value);
        }
    }
    crate::value::js_nanbox_pointer(arr as i64)
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
    crate::object::js_object_coerce(value)
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
    if let Some(tag) = crate::builtins::boxed_primitive_to_string_tag(f64::from_bits(this_bits)) {
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
                if gc_type == crate::gc::GC_TYPE_DATE_CELL {
                    b"[object Date]"
                } else if gc_type == crate::gc::GC_TYPE_ARRAY
                    || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY
                {
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

extern "C" fn date_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let string = crate::date::js_date_to_string(this_value);
    crate::value::js_nanbox_string(string as i64)
}

extern "C" fn object_prototype_has_own_property_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    unsafe {
        super::js_native_call_method(
            this_value,
            b"hasOwnProperty".as_ptr() as *const i8,
            "hasOwnProperty".len(),
            &key as *const f64,
            1,
        )
    }
}

extern "C" fn error_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let this_jsv = crate::value::JSValue::from_bits(this_value.to_bits());
    if !this_jsv.is_pointer() || this_jsv.is_null() || this_jsv.is_undefined() {
        super::object_ops::throw_object_type_error(
            b"Error.prototype.toString called on non-object",
        );
    }
    let raw = crate::value::js_nanbox_get_pointer(this_value) as *const u8;
    if raw.is_null() || !crate::object::is_valid_obj_ptr(raw) {
        super::object_ops::throw_object_type_error(
            b"Error.prototype.toString called on non-object",
        );
    }

    let name = error_to_string_property(this_value, b"name", "Error");
    let message = error_to_string_property(this_value, b"message", "");
    let result = if name.is_empty() {
        message
    } else if message.is_empty() {
        name
    } else {
        format!("{name}: {message}")
    };
    let s = crate::string::js_string_from_bytes(result.as_ptr(), result.len() as u32);
    crate::value::js_nanbox_string(s as i64)
}

fn error_to_string_property(this_value: f64, key: &'static [u8], default: &str) -> String {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let obj = crate::value::js_nanbox_get_pointer(this_value) as *const ObjectHeader;
    let value = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
    let value_jsv = crate::value::JSValue::from_bits(value.to_bits());
    if value_jsv.is_undefined() {
        return default.to_string();
    }
    let string = crate::value::js_jsvalue_to_string(value);
    unsafe { string_header_to_owned(string) }
}

unsafe fn string_header_to_owned(ptr: *const crate::StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let len = (*ptr).byte_len as usize;
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
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

fn array_buffer_receiver_addr() -> Option<usize> {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    if crate::buffer::is_registered_buffer(raw) && crate::buffer::is_array_buffer(raw) {
        Some(raw)
    } else {
        None
    }
}

fn array_buffer_brand_error() -> ! {
    super::object_ops::throw_object_type_error(
        b"Method get ArrayBuffer.prototype.byteLength called on incompatible receiver",
    )
}

extern "C" fn array_buffer_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match array_buffer_receiver_addr() {
        Some(addr) => {
            let buf = addr as *const crate::buffer::BufferHeader;
            f64::from_bits(
                crate::value::JSValue::number(crate::buffer::js_buffer_length(buf) as f64).bits(),
            )
        }
        None => array_buffer_brand_error(),
    }
}

extern "C" fn array_buffer_is_view_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    let addr = if jv.is_pointer() {
        (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if value.to_bits() >> 48 == 0 && value.to_bits() > 0x10000 {
        value.to_bits() as usize
    } else {
        0
    };
    let is_view = (addr != 0
        && !crate::buffer::is_any_array_buffer(addr)
        && (crate::buffer::is_uint8array_buffer(addr) || crate::buffer::is_data_view(addr)))
        || jsvalue_extends_data_view(value)
        || crate::typedarray::lookup_typed_array_kind(addr).is_some();
    f64::from_bits(crate::value::JSValue::bool(is_view).bits())
}

fn jsvalue_extends_data_view(value: f64) -> bool {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return false;
    }
    let ptr = v.as_pointer::<u8>();
    if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr) {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        let obj = ptr as *const ObjectHeader;
        let class_id = (*obj).class_id;
        class_id != 0 && crate::object::extends_builtin_data_view(class_id)
    }
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

fn typed_array_brand_error() -> ! {
    super::object_ops::throw_object_type_error(
        b"Method get %TypedArray%.prototype accessor called on incompatible receiver",
    )
}

fn string_value_to_owned(value: f64) -> Option<String> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let s = crate::builtins::js_string_coerce(value);
    if s.is_null() {
        return None;
    }
    unsafe {
        let bytes = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let len = (*s).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(bytes, len))
            .ok()
            .map(ToOwned::to_owned)
    }
}

fn typed_array_constructor_this_kind() -> Option<u8> {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let ptr = crate::value::js_nanbox_get_pointer(this_value) as usize;
    if ptr == 0 || !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    let name_value = crate::closure::closure_get_dynamic_prop(ptr, "name");
    let name = string_value_to_owned(f64::from_bits(name_value.to_bits()))?;
    crate::typedarray::kind_for_name(&name)
}

fn require_typed_array_constructor_this() -> u8 {
    typed_array_constructor_this_kind().unwrap_or_else(|| {
        super::object_ops::throw_object_type_error(
            b"%TypedArray%.from/of requires a concrete typed array constructor",
        )
    })
}

fn typed_array_buffer_value(ta: *const crate::typedarray::TypedArrayHeader) -> f64 {
    let buf = crate::typedarray::typed_array_to_array_buffer(ta);
    if buf.is_null() {
        typed_array_brand_error();
    }
    crate::value::js_nanbox_pointer(buf as i64)
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
        None => typed_array_brand_error(),
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
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.byteOffset` getter — always 0 (Perry views are not
/// backed by an offset into a shared `ArrayBuffer`).
extern "C" fn typed_array_byte_offset_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some(_) => f64::from_bits(crate::value::JSValue::number(0.0).bits()),
        None => typed_array_brand_error(),
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
    match typed_array_receiver() {
        Some((ta, _)) => typed_array_buffer_value(ta),
        None => typed_array_brand_error(),
    }
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

fn install_typed_array_iterator_symbol(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    install_proto_method(
        proto_obj,
        "values",
        global_this_builtin_noop_thunk as *const u8,
        0,
    );
    unsafe {
        let values_key = crate::string::js_string_from_bytes(b"values".as_ptr(), 6);
        let values = js_object_get_field_by_name(proto_obj, values_key);
        let iter = crate::symbol::well_known_symbol("iterator");
        if !iter.is_null() && values.bits() != crate::value::TAG_UNDEFINED {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            let iter_value =
                f64::from_bits(crate::value::JSValue::pointer(iter as *const u8).bits());
            crate::symbol::js_object_set_symbol_property(
                proto_value,
                iter_value,
                f64::from_bits(values.bits()),
            );
        }
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
    let ctor = crate::closure::js_closure_alloc(typed_array_constructor_call_thunk as *const u8, 0);
    let proto = js_object_alloc(0, 0);
    if ctor.is_null() || proto.is_null() {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    crate::closure::js_register_closure_arity(typed_array_constructor_call_thunk as *const u8, 0);
    super::native_module::set_bound_native_closure_name(ctor, "TypedArray");
    super::native_module::set_builtin_closure_length(ctor as usize, 0);
    super::set_builtin_property_attrs(
        ctor as usize,
        "name".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    super::set_builtin_property_attrs(
        ctor as usize,
        "length".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
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
    install_typed_array_iterator_symbol(proto);
    install_constructor_static_with_call_arity(
        ctor,
        "from",
        typed_array_from_thunk as *const u8,
        1,
        3,
        false,
    );
    install_constructor_static(ctor, "of", typed_array_of_thunk as *const u8, 0, true);
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
            "Storage" => crate::web_storage::storage_constructor_illegal as *const u8,
            "Crypto" | "CryptoKey" | "SubtleCrypto" => {
                webcrypto_illegal_constructor_thunk as *const u8
            }
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => typed_array_constructor_call_thunk as *const u8,
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
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "EvalError" | "URIError" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "MessageChannel" | "MessagePort" | "Storage" => {
                crate::closure::js_register_closure_arity(func_ptr, 0);
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
        if name == "Error" {
            install_error_static_methods(closure_ptr);
        }
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
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
            super::set_builtin_property_attrs(
                closure_ptr as usize,
                "prototype".to_string(),
                super::PropertyAttrs::new(false, false, false),
            );
            let ctor_key = crate::string::js_string_from_bytes(
                b"constructor".as_ptr(),
                "constructor".len() as u32,
            );
            js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
            super::set_builtin_property_attrs(
                proto_obj as usize,
                "constructor".to_string(),
                super::PropertyAttrs::new(true, false, true),
            );
            if is_web_fetch_constructor(name) {
                js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
                super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "constructor".to_string(),
                    super::PropertyAttrs::new(true, false, true),
                );
            }
            if name == "Array" {
                let constructor_key =
                    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                js_object_set_field_by_name(proto_obj, constructor_key, ctor_value);
                super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "constructor".to_string(),
                    super::PropertyAttrs::new(true, false, true),
                );
            }
            if name == "Navigator" || name == "TextEncoderStream" || name == "TextDecoderStream" {
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
                super::native_module::install_webcrypto_constructor_proto(proto_obj, ctor_value);
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
            }
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        js_object_set_field_by_name(singleton, name_key, ctor_value);
        super::set_builtin_property_attrs(
            singleton as usize,
            name.to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }
    // Callable global functions: ClosureHeader-backed values with real
    // dispatch so direct property reads and rebound calls match bare calls.
    for name in GLOBAL_THIS_BUILTIN_FUNCTIONS.iter().copied() {
        let (func_ptr, arity, has_rest) = match name {
            "eval" => (global_this_eval_thunk as *const u8, 1, false),
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
        } else if name == "WebAssembly" {
            global_this_webassembly::create_webassembly_namespace()
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
        if name == "WebAssembly" {
            super::set_builtin_property_attrs(
                singleton as usize,
                name.to_string(),
                super::PropertyAttrs::new(true, false, true),
            );
        }
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
    let func_ptr = global_this_crypto_getter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let getter = crate::closure::js_closure_alloc(func_ptr, 0);
    let getter_bits = if getter.is_null() {
        0
    } else {
        crate::value::js_nanbox_pointer(getter as i64).to_bits()
    };
    super::set_builtin_accessor_descriptor(
        singleton as usize,
        "crypto".to_string(),
        super::AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        super::PropertyAttrs::new(true, true, true),
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

extern "C" fn typed_array_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    source: f64,
    map_fn: f64,
    this_arg: f64,
) -> f64 {
    let kind = require_typed_array_constructor_this();
    let mapped = map_fn.to_bits() != crate::value::TAG_UNDEFINED;
    let arr = if mapped {
        crate::array::js_array_from_mapped(source, map_fn, this_arg)
    } else {
        crate::array::js_array_from_value(source)
    };
    let ta = crate::typedarray::js_typed_array_new_from_array(kind as i32, arr);
    crate::value::js_nanbox_pointer(ta as i64)
}

extern "C" fn typed_array_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let kind = require_typed_array_constructor_this();
    let vals = global_this_rest_array_values(rest);
    let len = vals.len() as u32;
    let arr = crate::array::js_array_alloc(len);
    unsafe {
        (*arr).length = len;
        for (i, &v) in vals.iter().enumerate() {
            crate::array::js_array_set_f64(arr, i as u32, v);
        }
    }
    let ta = crate::typedarray::js_typed_array_new_from_array(kind as i32, arr);
    crate::value::js_nanbox_pointer(ta as i64)
}

extern "C" fn url_can_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    base: f64,
) -> f64 {
    let input_ptr = crate::url::js_url_coerce_string(input);
    let ok = if base.to_bits() == crate::value::TAG_UNDEFINED {
        crate::url::js_url_can_parse(input_ptr)
    } else {
        let base_ptr = crate::url::js_url_coerce_string(base);
        crate::url::js_url_can_parse_with_base(input_ptr, base_ptr)
    };
    f64::from_bits(crate::value::JSValue::bool(ok != 0).bits())
}

extern "C" fn url_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    base: f64,
) -> f64 {
    let input_ptr = crate::url::js_url_coerce_string(input);
    let url = if base.to_bits() == crate::value::TAG_UNDEFINED {
        crate::url::js_url_parse(input_ptr)
    } else {
        let base_ptr = crate::url::js_url_coerce_string(base);
        crate::url::js_url_parse_with_base(input_ptr, base_ptr)
    };
    if url.is_null() {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        crate::value::js_nanbox_pointer(url as i64)
    }
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
    install_constructor_static_with_call_arity(ctor, name, func_ptr, arity, arity, has_rest);
}

fn install_constructor_static_with_call_arity(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    spec_length: u32,
    call_arity: u32,
    has_rest: bool,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    if has_rest {
        crate::closure::js_register_closure_rest(func_ptr, call_arity);
    } else {
        crate::closure::js_register_closure_arity(func_ptr, call_arity);
    }
    super::native_module::set_bound_native_closure_name(closure, name);
    super::native_module::set_builtin_closure_length(closure as usize, spec_length);
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
        "ArrayBuffer" => {
            install_constructor_static(
                ctor,
                "isView",
                array_buffer_is_view_thunk as *const u8,
                1,
                false,
            );
        }
        "Response" => {
            install_constructor_static(
                ctor,
                "error",
                global_this_builtin_noop_thunk as *const u8,
                0,
                false,
            );
            install_constructor_static(
                ctor,
                "json",
                global_this_builtin_noop_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "redirect",
                global_this_builtin_noop_thunk as *const u8,
                1,
                false,
            );
        }
        "URL" => {
            install_constructor_static(
                ctor,
                "canParse",
                url_can_parse_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "parse", url_parse_thunk as *const u8, 1, false);
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
        "ArrayBuffer" => {
            install_noop_proto_methods(proto_obj, &[("slice", 2)]);
            unsafe {
                crate::closure::js_register_closure_arity(
                    array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                let getter = crate::closure::js_closure_alloc(
                    array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                if !getter.is_null() {
                    let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
                    install_builtin_getter(proto_obj, "byteLength", getter_bits);
                    set_accessor_descriptor(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        AccessorDescriptor {
                            get: getter_bits,
                            set: 0,
                        },
                    );
                    set_property_attrs(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        PropertyAttrs::new(true, false, true),
                    );
                }
            }
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
            install_proto_method(
                proto_obj,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
            install_noop_proto_methods(proto_obj, &[("propertyIsEnumerable", 1)]);
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
                    ("toTimeString", 0),
                    ("toUTCString", 0),
                    ("valueOf", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "toString",
                date_prototype_to_string_thunk as *const u8,
                0,
            );
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
        "Headers" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("append", 2),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("get", 1),
                    ("getSetCookie", 0),
                    ("has", 1),
                    ("keys", 0),
                    ("set", 2),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Request" | "Response" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("arrayBuffer", 0),
                    ("blob", 0),
                    ("bytes", 0),
                    ("clone", 0),
                    ("formData", 0),
                    ("json", 0),
                    ("text", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Blob" | "File" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("arrayBuffer", 0),
                    ("bytes", 0),
                    ("slice", 0),
                    ("stream", 0),
                    ("text", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "FormData" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("append", 2),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("get", 1),
                    ("getAll", 1),
                    ("has", 1),
                    ("keys", 0),
                    ("set", 2),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "WebSocket" => {
            websocket_global::install_proto_methods(proto_obj);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Crypto" => {
            install_webcrypto_proto_getter(
                proto_obj,
                "subtle",
                webcrypto_subtle_getter_thunk as *const u8,
            );
            install_webcrypto_proto_method(
                proto_obj,
                "getRandomValues",
                webcrypto_get_random_values_thunk as *const u8,
                1,
            );
            install_webcrypto_proto_method(
                proto_obj,
                "randomUUID",
                webcrypto_random_uuid_thunk as *const u8,
                0,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "CryptoKey" => {
            for name in ["algorithm", "extractable", "type", "usages"] {
                install_webcrypto_proto_getter(
                    proto_obj,
                    name,
                    cryptokey_property_getter_thunk as *const u8,
                );
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError"
        | "AggregateError" | "EvalError" | "URIError" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            install_proto_method(
                proto_obj,
                "toString",
                error_prototype_to_string_thunk as *const u8,
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
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
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

fn install_error_prototype_data_properties(builtin_name: &str, proto_obj: *mut ObjectHeader) {
    let name = match builtin_name {
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError"
        | "AggregateError" | "EvalError" | "URIError" => builtin_name,
        _ => return,
    };
    if proto_obj.is_null() {
        return;
    }

    let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let name_value =
        crate::string::js_string_from_bytes(name.as_bytes().as_ptr(), name.len() as u32);
    js_object_set_field_by_name(
        proto_obj,
        name_key,
        crate::value::js_nanbox_string(name_value as i64),
    );
    super::set_builtin_property_attrs(
        proto_obj as usize,
        "name".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let message_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
    let message_value = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    js_object_set_field_by_name(
        proto_obj,
        message_key,
        crate::value::js_nanbox_string(message_value as i64),
    );
    super::set_builtin_property_attrs(
        proto_obj as usize,
        "message".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

fn install_webcrypto_proto_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    install_proto_method(proto_obj, method_name, func_ptr, arity);
    super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::PropertyAttrs::new(true, true, true),
    );
}

fn install_webcrypto_proto_getter(proto_obj: *mut ObjectHeader, name: &str, func_ptr: *const u8) {
    if proto_obj.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let value = if closure.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        super::native_module::set_bound_native_closure_name(closure, &format!("get {name}"));
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
        super::PropertyAttrs::new(true, true, true),
    );
}
