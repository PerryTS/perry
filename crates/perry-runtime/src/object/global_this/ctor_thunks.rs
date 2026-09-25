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

pub(crate) extern "C" fn construct_only_builtin_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    super::super::object_ops::throw_object_type_error(b"Constructor requires 'new'")
}

/// `RegExp(pattern, flags)` called WITHOUT `new` — unlike Map/Set below,
/// RegExp IS callable: ECMA-262 22.2.4 makes the call form construct exactly
/// like `new RegExp(pattern, flags)`, with one identity shortcut — `RegExp(re)`
/// with an existing RegExp and undefined flags returns `re` unchanged.
///
/// The noop-thunk fallback returned `undefined` here, which is how lodash's
/// module init died: `runInContext` rebinds the global (`var RegExp =
/// context.RegExp`) and builds its native-function probe through the call form
/// (`var reIsNative = RegExp('^' + …)`) — the very next `reIsNative.test(...)`
/// threw "Cannot read properties of undefined". Construction mirrors the
/// dynamic-`new` RegExp arm in class_registry/construct.rs.
#[cfg(feature = "regex-engine")]
pub(crate) extern "C" fn regexp_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    pattern: f64,
    flags: f64,
) -> f64 {
    crate::value::js_nanbox_pointer(crate::regex::js_regexp_construct_call(pattern, flags) as i64)
}

/// Without the regex engine there is no RegExp to construct — keep the
/// pre-existing noop behavior rather than referencing a compiled-out ctor.
#[cfg(not(feature = "regex-engine"))]
pub(crate) extern "C" fn regexp_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _pattern: f64,
    _flags: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
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

/// #5902: is `value` literally `Array.prototype[method]`?
///
/// The array-like engine treats *any* borrowed builtin closure stored in an
/// own slot as a borrowed **Array** builtin (`classify_own_slot`), which is
/// only true when the borrow actually came off `Array.prototype`.
/// `obj.concat = String.prototype.concat` stores a non-constructable builtin
/// closure too, so it was misread and ran the array algorithm — returning
/// `[obj, "two", undefined]` where the spec (and the working
/// `String.prototype.concat.call(obj, …)` form) gives `"onetwoundefined"`.
///
/// Compared by closure FUNCTION POINTER, not by closure identity: reading
/// `Array.prototype.concat` can hand back a freshly reified closure, but every
/// reification of the same builtin shares one code address. A func ptr is a
/// code address rather than a heap pointer, so nothing here is GC-visible and
/// no root scanner is required.
pub(crate) fn is_array_prototype_method_value(value: f64, method: &str) -> bool {
    let slot = crate::value::JSValue::from_bits(value.to_bits());
    if !slot.is_pointer() {
        return false;
    }
    let slot_ptr = slot.as_pointer::<crate::closure::ClosureHeader>();
    if slot_ptr.is_null() {
        return false;
    }
    let slot_fp = crate::closure::get_valid_func_ptr(slot_ptr);
    if slot_fp.is_null() {
        return false;
    }

    let proto = builtin_prototype_value("Array");
    let proto_bits = proto.to_bits();
    if (proto_bits >> 48) != 0x7FFD {
        return false;
    }
    let proto_ptr = (proto_bits & crate::value::POINTER_MASK) as *const super::super::ObjectHeader;
    if proto_ptr.is_null() {
        return false;
    }
    let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
    let canonical = super::super::js_object_get_field_by_name_f64(proto_ptr, key);
    let canonical_jv = crate::value::JSValue::from_bits(canonical.to_bits());
    if !canonical_jv.is_pointer() {
        return false;
    }
    let canonical_ptr = canonical_jv.as_pointer::<crate::closure::ClosureHeader>();
    if canonical_ptr.is_null() {
        return false;
    }
    let canonical_fp = crate::closure::get_valid_func_ptr(canonical_ptr);
    !canonical_fp.is_null() && canonical_fp == slot_fp
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

/// #10523: the receiver is already brand-checked, so go straight to
/// perry-stdlib's crypto dispatcher (the same `("crypto.webcrypto", _)` arm
/// `js_native_call_method` ends at) instead of re-resolving `randomUUID` by
/// name through the whole native-call tower on every UUID.
pub(crate) extern "C" fn webcrypto_random_uuid_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    require_webcrypto_this();
    let ptr = crate::value::JS_NATIVE_CRYPTO_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let dispatch: crate::value::JsNativeCryptoDispatchFn = std::mem::transmute(ptr);
        dispatch(
            b"randomUUID".as_ptr(),
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

/// #10427: `globalThis.crypto.<method>` is a property READ, resolved fresh
/// on every access through `vt_get_own_field` (there is no real `ObjectHeader`
/// backing `globalThis.crypto` for the read to land an own slot on — see
/// `crypto.webcrypto`'s NATIVE_MODULE_CLASS_ID namespace). Plain
/// `js_closure_alloc` mints a brand-new `ClosureHeader` on every call, so
/// `crypto.randomUUID === crypto.randomUUID` was `false` and every read
/// allocated. `js_closure_alloc_singleton` (the same func-ptr-keyed cache PR
/// #10630 traced the closure-identity contract back to) returns the SAME
/// closure for the same `func_ptr` every time — the func_ptr IS the method
/// identity here since these thunks take no captures.
pub(crate) fn webcrypto_method_value(property_name: &str) -> Option<f64> {
    let (func_ptr, arity) = match property_name {
        "getRandomValues" => (webcrypto_get_random_values_thunk as *const u8, 1),
        "randomUUID" => (webcrypto_random_uuid_thunk as *const u8, 0),
        _ => return None,
    };
    Some(webcrypto_singleton_method(
        func_ptr,
        property_name,
        arity,
        || crate::closure::js_register_closure_arity(func_ptr, arity),
    ))
}

/// #10523: decorate a Web Crypto method's singleton closure (body registry
/// entry, `name`, `length`) only when it is first minted. Redoing it on every
/// read allocated a name string, reinstalled the `name` descriptor and
/// invalidated the thunk's cached call-dispatch strategy each time, which was
/// most of what `crypto.randomUUID ? crypto.randomUUID() : …` spent in Perry.
fn webcrypto_singleton_method(
    func_ptr: *const u8,
    name: &str,
    length: u32,
    register_body: impl FnOnce(),
) -> f64 {
    if let Some(closure) = crate::closure::singleton_closure_if_cached(func_ptr) {
        return crate::value::js_nanbox_pointer(closure as i64);
    }
    register_body();
    let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    super::super::native_module::set_bound_native_closure_name(closure, name);
    // The name install allocates; re-read the GC-rewritten singleton slot.
    let closure = crate::closure::singleton_closure_if_cached(func_ptr).unwrap_or(closure);
    super::super::native_module::set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
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

/// Same per-read allocation defect as `webcrypto_method_value` above, for
/// `crypto.subtle`'s KEM methods (`encapsulateBits` and friends — the rest of
/// SubtleCrypto's surface is already cached via `bound_native_callable_export_value`,
/// see #10427's PR body for which paths were and weren't affected).
pub(crate) fn subtle_crypto_method_value(property_name: &str) -> Option<f64> {
    let (func_ptr, length) = subtle_crypto_method_spec(property_name)?;
    Some(webcrypto_singleton_method(
        func_ptr,
        property_name,
        length,
        || crate::closure::js_register_closure_rest(func_ptr, 0),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::super::native_module::{builtin_closure_length, set_builtin_closure_length};

    fn closure_addr(value: f64) -> usize {
        (value.to_bits() & crate::value::POINTER_MASK) as usize
    }

    // #10523: a repeat read hands back the same singleton WITHOUT decorating
    // it again (the tampered length survives; the old per-read install reset
    // it), and a re-minted singleton is decorated afresh.
    #[test]
    fn webcrypto_methods_are_decorated_once_per_singleton() {
        for (name, length, read) in [
            (
                "randomUUID",
                0,
                super::webcrypto_method_value as fn(&str) -> Option<f64>,
            ),
            ("getRandomValues", 1, super::webcrypto_method_value),
            ("encapsulateBits", 2, super::subtle_crypto_method_value),
        ] {
            crate::closure::test_clear_singleton_closure_caches();
            let first = read(name).unwrap();
            assert_eq!(
                builtin_closure_length(closure_addr(first)),
                Some(length),
                "{name}"
            );
            set_builtin_closure_length(closure_addr(first), 99);
            let second = read(name).unwrap();
            assert_eq!(second.to_bits(), first.to_bits(), "{name} identity");
            assert_eq!(
                builtin_closure_length(closure_addr(first)),
                Some(99),
                "{name} redecorated"
            );
            crate::closure::test_clear_singleton_closure_caches();
            let reminted = read(name).unwrap();
            assert_eq!(
                builtin_closure_length(closure_addr(reminted)),
                Some(length),
                "{name}"
            );
        }
    }
}
