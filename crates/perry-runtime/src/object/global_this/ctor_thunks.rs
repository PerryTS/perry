use super::super::*;
use super::*;

pub(crate) fn normalize_eval_this_body(body: &str) -> Option<String> {
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
    super::super::object_ops::throw_object_type_error(b"Constructor %TypedArray% requires 'new'")
}

// #4569: Map/Set/WeakMap/WeakSet/WeakRef are constructors — calling them
// without `new` is a TypeError (ECMA-262: an undefined newTarget throws). The
// bare-call form previously fell through to `global_this_builtin_noop_thunk`
// and silently returned `undefined`. (`new Map()` uses the separate
// construct-expression path and is unaffected.)
pub(crate) extern "C" fn map_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(b"Constructor Map requires 'new'")
}

pub(crate) extern "C" fn set_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(b"Constructor Set requires 'new'")
}

pub(crate) extern "C" fn weak_map_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(b"Constructor WeakMap requires 'new'")
}

pub(crate) extern "C" fn weak_set_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(b"Constructor WeakSet requires 'new'")
}

pub(crate) extern "C" fn weak_ref_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(b"Constructor WeakRef requires 'new'")
}

pub(crate) extern "C" fn promise_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(b"Constructor Promise requires 'new'")
}

pub(crate) extern "C" fn global_this_url_pattern_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    base: f64,
) -> f64 {
    crate::url::js_url_pattern_constructor_call(input, base)
}

fn error_constructor_call(kind: u32, message: f64) -> f64 {
    let error = crate::error::js_error_new_kind_from_value(kind, message);
    crate::value::js_nanbox_pointer(error as i64)
}

pub(crate) extern "C" fn error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_ERROR, message)
}

pub(crate) extern "C" fn type_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_TYPE_ERROR, message)
}

pub(crate) extern "C" fn range_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_RANGE_ERROR, message)
}

pub(crate) extern "C" fn reference_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_REFERENCE_ERROR, message)
}

pub(crate) extern "C" fn syntax_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_SYNTAX_ERROR, message)
}

pub(crate) extern "C" fn eval_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_EVAL_ERROR, message)
}

pub(crate) extern "C" fn uri_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_URI_ERROR, message)
}

/// Whether `value` is the %Function.prototype% intrinsic object. It is the
/// one ordinary-object-shaped value that is itself a Function: callable
/// (returns `undefined`), tagged `[object Function]`, but NOT a constructor.
/// Only consulted on slow paths (failed call dispatch, `Object.prototype.
/// toString`), so the per-call re-resolution through the global registry is
/// fine — and safer than caching a raw pointer across GC cycles.
pub(crate) fn is_function_prototype_object_value(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let proto = builtin_prototype_value("Function");
    proto.to_bits() == value.to_bits()
}

pub(crate) fn builtin_prototype_value(name: &str) -> f64 {
    let ctor = js_get_global_this_builtin_value(name.as_ptr(), name.len());
    let ctor_bits = ctor.to_bits();
    if (ctor_bits >> 48) != 0x7FFD {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let ctor_ptr = (ctor_bits & crate::value::POINTER_MASK) as usize;
    if ctor_ptr == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype")
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

pub(crate) extern "C" fn global_this_crypto_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    super::super::native_module::webcrypto_namespace()
}

fn require_webcrypto_this() -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let jv = crate::value::JSValue::from_bits(this_value.to_bits());
    if jv.is_pointer() {
        let obj = jv.as_pointer::<ObjectHeader>();
        if !obj.is_null()
            && unsafe { (*obj).class_id } == super::super::native_module::NATIVE_MODULE_CLASS_ID
            && unsafe { super::super::native_module::read_native_module_name(obj) }
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

pub(crate) extern "C" fn webcrypto_subtle_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    require_webcrypto_this();
    super::super::native_module::subtle_crypto_namespace()
}

fn cryptokey_receiver_addr() -> Option<usize> {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = crate::value::JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & crate::value::POINTER_MASK) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    crate::buffer::crypto_key_meta(raw).map(|_| raw)
}

fn cryptokey_brand_error() -> ! {
    super::super::object_ops::throw_object_type_error(
        b"Value of CryptoKey getter must be an instance of CryptoKey",
    )
}

fn cryptokey_property_getter(key: &[u8]) -> f64 {
    let addr = cryptokey_receiver_addr().unwrap_or_else(|| cryptokey_brand_error());
    unsafe {
        super::super::crypto_key_property_value(addr, key)
            .map(|value| f64::from_bits(value.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
    }
}

pub(crate) extern "C" fn cryptokey_algorithm_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    cryptokey_property_getter(b"algorithm")
}

pub(crate) extern "C" fn cryptokey_extractable_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    cryptokey_property_getter(b"extractable")
}

pub(crate) extern "C" fn cryptokey_type_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    cryptokey_property_getter(b"type")
}

pub(crate) extern "C" fn cryptokey_usages_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    cryptokey_property_getter(b"usages")
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
    super::super::native_module::set_bound_native_closure_name(closure, property_name);
    super::super::native_module::set_builtin_closure_length(closure as usize, arity);
    Some(crate::value::js_nanbox_pointer(closure as i64))
}

fn subtle_crypto_method_spec(property_name: &str) -> Option<(*const u8, u32)> {
    match property_name {
        "encapsulateBits" => Some((subtle_crypto_encapsulate_bits_thunk as *const u8, 2)),
        "decapsulateBits" => Some((subtle_crypto_decapsulate_bits_thunk as *const u8, 3)),
        "encapsulateKey" => Some((subtle_crypto_encapsulate_key_thunk as *const u8, 5)),
        "decapsulateKey" => Some((subtle_crypto_decapsulate_key_thunk as *const u8, 6)),
        _ => None,
    }
}

pub(crate) fn subtle_crypto_method_value(property_name: &str) -> Option<f64> {
    let (func_ptr, length) = subtle_crypto_method_spec(property_name)?;
    crate::closure::js_register_closure_rest(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    super::super::native_module::set_bound_native_closure_name(closure, property_name);
    super::super::native_module::set_builtin_closure_length(closure as usize, length);
    Some(crate::value::js_nanbox_pointer(closure as i64))
}
