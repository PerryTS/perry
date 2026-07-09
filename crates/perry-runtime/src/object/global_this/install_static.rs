use super::super::*;
use super::*;

#[no_mangle]
pub extern "C" fn js_promise_static_function_value(name_ptr: *const u8, name_len: usize) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let Ok(name) = std::str::from_utf8(name_bytes) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let Some((func_ptr, spec_length, call_arity, has_rest)) = promise_static_function_spec(name)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };

    let ctor_value = js_get_global_this_builtin_value(b"Promise".as_ptr(), 7);
    let ctor_ptr =
        crate::value::js_nanbox_get_pointer(ctor_value) as *mut crate::closure::ClosureHeader;
    if !ctor_ptr.is_null() {
        let existing = crate::closure::closure_get_dynamic_prop(ctor_ptr as usize, name);
        if existing.to_bits() != crate::value::TAG_UNDEFINED {
            return existing;
        }
    }

    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if has_rest {
        crate::closure::js_register_closure_rest(func_ptr, call_arity);
    } else {
        crate::closure::js_register_closure_arity(func_ptr, call_arity);
    }
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);

    let value = crate::value::js_nanbox_pointer(closure as i64);
    if !ctor_ptr.is_null() {
        crate::closure::closure_set_dynamic_prop(ctor_ptr as usize, name, value);
        super::super::set_builtin_property_attrs(
            ctor_ptr as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    value
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

extern "C" fn subtle_crypto_supports_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let args = global_this_rest_array_values(rest);
    if args.len() < 2 {
        let message = format!(
            "Failed to execute 'supports' on 'SubtleCrypto': 2 arguments required, but only {} present.",
            args.len()
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_MISSING_ARGS");
    }

    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let op = args[0];
    let algorithm = args[1];
    let length = args.get(2).copied().unwrap_or(undefined);
    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH.load(Ordering::SeqCst);
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
        unsafe { std::mem::transmute(ptr) };
    let dispatch_args = [op, algorithm, length];
    unsafe {
        dispatch(
            b"supports".as_ptr(),
            "supports".len(),
            dispatch_args.as_ptr(),
            dispatch_args.len(),
        )
    }
}

fn is_subtle_crypto_this(value: f64) -> bool {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let obj = js_value.as_pointer::<ObjectHeader>();
    !obj.is_null()
        && unsafe { (*obj).class_id } == super::super::native_module::NATIVE_MODULE_CLASS_ID
        && unsafe { super::super::native_module::read_native_module_name(obj) }
            .is_some_and(|name| name == "crypto.subtle")
}

fn rejected_type_error_with_code_promise(message: &str, code: &'static str) -> f64 {
    let reason = crate::fs::validate::build_type_error_with_code_value(message, code);
    let promise = crate::promise::js_promise_rejected(reason);
    crate::value::js_nanbox_pointer(promise as i64)
}

fn subtle_crypto_dispatch_rest(method_name: &str, rest: f64) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if !is_subtle_crypto_this(this_value) {
        return rejected_type_error_with_code_promise(
            "Value of \"this\" must be of type SubtleCrypto",
            "ERR_INVALID_THIS",
        );
    }

    let args = global_this_rest_array_values(rest);
    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH.load(Ordering::SeqCst);
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
        unsafe { std::mem::transmute(ptr) };
    unsafe {
        dispatch(
            method_name.as_ptr(),
            method_name.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

pub(crate) extern "C" fn subtle_crypto_encapsulate_bits_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("encapsulateBits", rest)
}

pub(crate) extern "C" fn subtle_crypto_decapsulate_bits_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("decapsulateBits", rest)
}

pub(crate) extern "C" fn subtle_crypto_encapsulate_key_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("encapsulateKey", rest)
}

pub(crate) extern "C" fn subtle_crypto_decapsulate_key_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("decapsulateKey", rest)
}

/// Install a single callable static method on a constructor closure as a
/// `{ writable: true, enumerable: false, configurable: true }` data property
/// (matching Node's descriptors for built-in statics). `has_rest` registers
/// the func pointer as a rest-arg closure so trailing args arrive as an array.
pub(crate) fn install_constructor_static(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
    has_rest: bool,
) {
    install_constructor_static_with_call_arity(ctor, name, func_ptr, arity, arity, has_rest);
}

pub(crate) fn install_constructor_static_with_call_arity(
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
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::super::set_builtin_property_attrs(
        ctor as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

pub(crate) fn install_number_static_data_properties(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    let props = [
        ("NaN", f64::NAN),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ("MAX_VALUE", f64::MAX),
        // ECMAScript Number.MIN_VALUE is the smallest *denormal* (5e-324 =
        // 2^-1074 = bit pattern 1), NOT f64::MIN_POSITIVE (smallest *normal*).
        ("MIN_VALUE", f64::from_bits(1)),
        ("EPSILON", f64::EPSILON),
        ("MAX_SAFE_INTEGER", 9007199254740991.0),
        ("MIN_SAFE_INTEGER", -9007199254740991.0),
    ];
    for (name, value) in props {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
        super::super::set_builtin_property_attrs(
            ctor as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(false, false, false),
        );
    }
}

/// #2889: install the common static methods on the `Object` / `Array`
/// constructor closures so rebound usage (`const O = Object; O.keys(x)`)
/// dispatches through the real runtime helpers. Only the high-traffic
/// statics with simple f64-in / f64-out shapes are reified here; the long
/// tail (`Object.defineProperty`, `Object.getOwnPropertyDescriptor`, …)
/// stays unreified on the rebound value and is a known scope gap.
pub(crate) fn install_builtin_constructor_statics(
    name: &str,
    ctor: *mut crate::closure::ClosureHeader,
) {
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
            install_constructor_static(ctor, "create", object_create_thunk as *const u8, 2, false);
            install_constructor_static(ctor, "seal", object_seal_thunk as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "isSealed",
                object_is_sealed_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "isFrozen",
                object_is_frozen_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "isExtensible",
                object_is_extensible_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "preventExtensions",
                object_prevent_extensions_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "is", object_is_thunk as *const u8, 2, false);
            install_constructor_static(
                ctor,
                "setPrototypeOf",
                object_set_prototype_of_thunk as *const u8,
                2,
                false,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertySymbols",
                object_get_own_property_symbols_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertyDescriptors",
                object_get_own_property_descriptors_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "defineProperties",
                object_define_properties_thunk as *const u8,
                2,
                false,
            );
            install_constructor_static(
                ctor,
                "groupBy",
                object_group_by_thunk as *const u8,
                2,
                false,
            );
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
                "getOwnPropertyDescriptor",
                object_get_own_property_descriptor_thunk as *const u8,
                2,
                false,
            );
            install_constructor_static(
                ctor,
                "defineProperty",
                object_define_property_thunk as *const u8,
                3,
                false,
            );
            install_constructor_static(
                ctor,
                "fromEntries",
                object_from_entries_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "assign",
                object_assign_thunk as *const u8,
                2,
                1,
                true,
            );
            install_constructor_static(ctor, "hasOwn", object_hasown_thunk as *const u8, 2, false);
            // `Object` is a function, so reading a non-static member resolves up
            // its prototype chain (Function.prototype → Object.prototype). In
            // particular `Object.hasOwnProperty` IS `Object.prototype.hasOwnProperty`
            // — a callable. immer's `O.hasOwnProperty.call(proto, "constructor")`
            // (with `const O = Object`) relied on this; without the inherited
            // methods installed on the reified ctor value the read returned
            // `undefined` and `.call` threw "Function.prototype.call on a value
            // that is not a function". Install the Object.prototype methods that
            // are reachable on the constructor by inheritance.
            install_constructor_static(
                ctor,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "propertyIsEnumerable",
                object_prototype_property_is_enumerable_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "toString",
                object_prototype_to_string_thunk as *const u8,
                0,
                false,
            );
            install_constructor_static(
                ctor,
                "toLocaleString",
                object_prototype_to_locale_string_thunk as *const u8,
                0,
                false,
            );
            install_constructor_static(
                ctor,
                "valueOf",
                object_prototype_value_of_thunk as *const u8,
                0,
                false,
            );
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
        "Promise" => {
            for static_name in [
                "resolve",
                "reject",
                "all",
                "race",
                "allSettled",
                "any",
                "withResolvers",
                "try",
            ] {
                if let Some((func_ptr, spec_length, call_arity, has_rest)) =
                    promise_static_function_spec(static_name)
                {
                    install_constructor_static_with_call_arity(
                        ctor,
                        static_name,
                        func_ptr,
                        spec_length,
                        call_arity,
                        has_rest,
                    );
                }
            }
        }
        "Date" => {
            // `Date.now` / `Date.parse` / `Date.UTC` as real own data props
            // (thunks live in `date_proto_thunks`). The functional calls are
            // codegen intrinsics, so this only affects value reads + reflection.
            date_proto_thunks::install_date_constructor_statics(ctor);
        }
        "Number" => {
            install_constructor_static(ctor, "isNaN", number_is_nan_thunk as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "isFinite",
                number_is_finite_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "isInteger",
                number_is_integer_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "isSafeInteger",
                number_is_safe_integer_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "parseFloat",
                number_parse_float_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "parseInt",
                number_parse_int_thunk as *const u8,
                2,
                false,
            );
        }
        "BigInt" => {
            // BigInt.asIntN(bits, bigint) / asUintN(bits, bigint) — spec length 2.
            install_constructor_static(
                ctor,
                "asIntN",
                bigint_as_int_n_thunk as *const u8,
                2,
                false,
            );
            install_constructor_static(
                ctor,
                "asUintN",
                bigint_as_uint_n_thunk as *const u8,
                2,
                false,
            );
        }
        "Symbol" => {
            install_constructor_static(ctor, "for", symbol_for_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "keyFor", symbol_key_for_thunk as *const u8, 1, false);
        }
        "String" => {
            // #4627: reify the variadic `String.fromCharCode` / `fromCodePoint`
            // statics so they are real function values (correct `.name` /
            // `.length`, usable via reference / spread). Call-arity 0 (all args
            // collected into `rest`) with spec `.length` 1. `String.raw` (a tag
            // function) is left on its intrinsic path for now.
            install_constructor_static_with_call_arity(
                ctor,
                "fromCharCode",
                string_from_char_code_static as *const u8,
                1,
                0,
                true,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "fromCodePoint",
                string_from_code_point_static as *const u8,
                1,
                0,
                true,
            );
            // #4627: `String.raw` (tag function) — 1 fixed param (template
            // object) + rest substitutions; spec `.length` 1.
            install_constructor_static_with_call_arity(
                ctor,
                "raw",
                string_raw_static as *const u8,
                1,
                1,
                true,
            );
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
                global_this_response_error_thunk as *const u8,
                0,
                false,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "json",
                global_this_response_json_thunk as *const u8,
                1,
                2,
                false,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "redirect",
                global_this_response_redirect_thunk as *const u8,
                1,
                2,
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
        "SubtleCrypto" => {
            install_constructor_static_with_call_arity(
                ctor,
                "supports",
                subtle_crypto_supports_thunk as *const u8,
                2,
                0,
                true,
            );
            super::super::set_builtin_property_attrs(
                ctor as usize,
                "supports".to_string(),
                super::super::PropertyAttrs::new(true, true, true),
            );
        }
        "Proxy" => {
            install_constructor_static(
                ctor,
                "revocable",
                proxy_revocable_thunk as *const u8,
                2,
                false,
            );
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
pub(crate) fn install_proto_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    arity: u32,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::super::native_module::set_bound_native_closure_name(closure, method_name);
    // #3143: record this method's spec `.length` per closure instance — all
    // noop-backed methods share one func_ptr, so the func-ptr arity registry
    // can't distinguish `map` (1) from `slice` (2). Read back by the `.length`
    // value-accessor and `getOwnPropertyDescriptor`.
    super::super::native_module::set_builtin_closure_length(closure as usize, arity);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(proto_obj, key, value);
    // Built-in prototype methods are `{ writable: true, enumerable: false,
    // configurable: true }` per spec. Record that descriptor (reflection-only,
    // no hot-path gate flip) so `Object.getOwnPropertyDescriptor`, `Object.keys`
    // and `for-in` all observe them as non-enumerable — Test262's `verifyProperty`
    // checks every built-in method this way. See `set_builtin_property_attrs`.
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    // #3143: the method's own `.name` / `.length` data properties are
    // `{ writable: false, enumerable: false, configurable: true }` per spec.
    // Register those on the closure itself so `getOwnPropertyDescriptor(
    // Array.prototype.map, "name")` reports `writable: false` (it previously
    // read the dynamic-prop slot and defaulted to writable). Reflection-only —
    // no hot-path gate flip.
    super::super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    value
}

/// Install `alias_name` on `proto_obj` as the SAME function object as an
/// already-installed method (`value` is that method's installed property
/// value). Annex B legacy aliases — `trimLeft`→`trimStart`,
/// `trimRight`→`trimEnd`, `toGMTString`→`toUTCString` — are required to be the
/// very same function object (`String.prototype.trimLeft === trimStart`, and
/// `.name` reports the canonical method's name), with the standard
/// `{ writable: true, enumerable: false, configurable: true }` method
/// descriptor. See test262 `annexB/built-ins/{String,Date}` (#5346).
pub(crate) fn install_proto_method_alias(
    proto_obj: *mut ObjectHeader,
    alias_name: &str,
    value: f64,
) {
    let key = crate::string::js_string_from_bytes(alias_name.as_ptr(), alias_name.len() as u32);
    js_object_set_field_by_name(proto_obj, key, value);
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        alias_name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

pub(crate) fn install_proto_method_rest(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    fixed_arity: u32,
) {
    install_proto_method_rest_with_length(
        proto_obj,
        method_name,
        func_ptr,
        fixed_arity,
        fixed_arity,
    );
}

pub(crate) fn install_proto_method_rest_with_length(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    spec_length: u32,
    call_fixed_arity: u32,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::js_register_closure_rest(func_ptr, call_fixed_arity);
    super::super::native_module::set_bound_native_closure_name(closure, method_name);
    super::super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(proto_obj, key, value);
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    super::super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    value
}

/// #4139/#4437: reify the `JSON` namespace's own methods for reflection parity
/// and detached value calls. Direct call sites are still codegen intrinsics.
pub(crate) fn install_json_namespace_members(ns_obj: *mut ObjectHeader) {
    const METHODS: &[(&str, *const u8, u32)] = &[
        ("parse", json_parse_thunk as *const u8, 2),
        ("stringify", json_stringify_thunk as *const u8, 3),
        ("rawJSON", json_raw_json_thunk as *const u8, 1),
        ("isRawJSON", json_is_raw_json_thunk as *const u8, 1),
    ];
    for (name, func_ptr, arity) in METHODS.iter().copied() {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
}

/// #4139: reify the `Reflect` namespace's own methods for reflection parity.
/// See `install_math_namespace` for the rationale.
pub(crate) fn install_reflect_namespace_members(ns_obj: *mut ObjectHeader) {
    let noop = global_this_builtin_noop_thunk as *const u8;
    let methods = [
        ("defineProperty", noop, 3),
        ("deleteProperty", noop, 2),
        ("apply", reflect_apply_thunk as *const u8, 3),
        // #5989: `construct` must be REAL as a value — Next.js's Date
        // extension calls it through a captured binding (see
        // `reflect_construct_thunk`).
        ("construct", reflect_construct_thunk as *const u8, 2),
        ("get", noop, 2),
        ("getOwnPropertyDescriptor", noop, 2),
        ("getPrototypeOf", noop, 1),
        ("has", noop, 2),
        ("isExtensible", noop, 1),
        ("ownKeys", noop, 1),
        ("preventExtensions", noop, 1),
        ("set", noop, 3),
        ("setPrototypeOf", noop, 2),
    ];
    for (name, func_ptr, arity) in methods {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
}

pub(crate) fn install_atomics_namespace_members(ns_obj: *mut ObjectHeader) {
    for (name, func_ptr, arity) in [
        ("load", crate::atomics::js_atomics_load as *const u8, 2),
        (
            "isLockFree",
            crate::atomics::js_atomics_is_lock_free as *const u8,
            1,
        ),
        ("store", crate::atomics::js_atomics_store as *const u8, 3),
        ("add", crate::atomics::js_atomics_add as *const u8, 3),
        ("sub", crate::atomics::js_atomics_sub as *const u8, 3),
        ("and", crate::atomics::js_atomics_and as *const u8, 3),
        ("or", crate::atomics::js_atomics_or as *const u8, 3),
        ("xor", crate::atomics::js_atomics_xor as *const u8, 3),
        (
            "exchange",
            crate::atomics::js_atomics_exchange as *const u8,
            3,
        ),
        (
            "compareExchange",
            crate::atomics::js_atomics_compare_exchange as *const u8,
            4,
        ),
        ("notify", crate::atomics::js_atomics_notify as *const u8, 3),
        ("wait", crate::atomics::js_atomics_wait as *const u8, 4),
        (
            "waitAsync",
            crate::atomics::js_atomics_wait_async as *const u8,
            4,
        ),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
}

/// Install a list of `(method_name, arity)` pairs on a prototype object.
/// Most entries are reflection-only methods backed by
/// `global_this_builtin_noop_thunk`, but inherited Object methods with
/// observable receiver-sensitive behavior use their real thunk.
pub(crate) fn install_noop_proto_methods(proto_obj: *mut ObjectHeader, methods: &[(&str, u32)]) {
    for (name, arity) in methods.iter().copied() {
        let func_ptr = match name {
            "isPrototypeOf" => object_prototype_is_prototype_of_thunk as *const u8,
            // Annex B accessor methods get real thunks (reflective `.call`).
            "__defineGetter__" => object_prototype_define_getter_thunk as *const u8,
            "__defineSetter__" => object_prototype_define_setter_thunk as *const u8,
            "__lookupGetter__" => object_prototype_lookup_getter_thunk as *const u8,
            "__lookupSetter__" => object_prototype_lookup_setter_thunk as *const u8,
            _ => global_this_builtin_noop_thunk as *const u8,
        };
        install_proto_method(proto_obj, name, func_ptr, arity);
    }
}

pub(crate) extern "C" fn url_pattern_test_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    rest: f64,
) -> f64 {
    let base = rest_first_arg(rest);
    let this_value = crate::object::js_implicit_this_get();
    let pattern = crate::value::js_nanbox_get_pointer(this_value) as *mut ObjectHeader;
    crate::url::js_url_pattern_test(pattern, input, base)
}

pub(crate) extern "C" fn url_pattern_exec_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    rest: f64,
) -> f64 {
    let base = rest_first_arg(rest);
    let this_value = crate::object::js_implicit_this_get();
    let pattern = crate::value::js_nanbox_get_pointer(this_value) as *mut ObjectHeader;
    crate::url::js_url_pattern_exec(pattern, input, base)
}

fn rest_first_arg(rest: f64) -> f64 {
    let value = crate::value::JSValue::from_bits(rest.to_bits());
    if !value.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let arr = value.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() || crate::array::js_array_length(arr) == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::array::js_array_get_f64(arr, 0)
}
