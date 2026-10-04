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
    let Some((info, spec_length)) = super::bigint_promise::promise_static_function_info(name)
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

    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
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

extern "C" fn abort_signal_abort_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    reason: f64,
) -> f64 {
    let signal = crate::url::abort::js_abort_signal_abort(reason);
    crate::value::js_nanbox_pointer(signal as i64)
}

extern "C" fn abort_signal_timeout_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    ms: f64,
) -> f64 {
    let signal = crate::url::abort::js_abort_signal_timeout(ms);
    crate::value::js_nanbox_pointer(signal as i64)
}

extern "C" fn abort_signal_any_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    signals: f64,
) -> f64 {
    let is_array = crate::array::js_array_is_array(signals).to_bits() == 0x7FFC_0000_0000_0004;
    let array = if is_array {
        crate::value::js_nanbox_get_pointer(signals) as *mut crate::array::ArrayHeader
    } else {
        std::ptr::null_mut()
    };
    if array.is_null() {
        crate::validators::throw_invalid_arg_type("signals", "Array", signals);
    }
    let signal = crate::url::abort::js_abort_signal_any(array);
    crate::value::js_nanbox_pointer(signal as i64)
}

extern "C" fn url_can_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
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
    _this: crate::closure::JsThis,
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

// #6674: static-factory thunks for `Uint8Array.fromBase64` / `fromHex`. The
// `str_handle` is the argument's raw NaN-boxed bits reinterpreted as `i64`, the
// convention `js_u8_from_base64` / `js_u8_from_hex` expect (matching the
// instance `setFromBase64` dispatch in `buffer_dispatch.rs`). The returned
// `BufferHeader*` is nan-boxed as a pointer — identical to the value the
// compile-time `Uint8Array.fromBase64(str)` call path produces.
extern "C" fn uint8array_from_base64_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    input: f64,
    opts: f64,
) -> f64 {
    let buf = crate::buffer::js_u8_from_base64(input.to_bits() as i64, opts);
    crate::value::js_nanbox_pointer(buf as i64)
}

extern "C" fn uint8array_from_hex_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    input: f64,
) -> f64 {
    let buf = crate::buffer::js_u8_from_hex(input.to_bits() as i64);
    crate::value::js_nanbox_pointer(buf as i64)
}

extern "C" fn subtle_crypto_supports_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
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

fn subtle_crypto_dispatch_rest(this: crate::closure::JsThis, method_name: &str, rest: f64) -> f64 {
    let this_value = f64::from_bits(this.bits());
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
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest(this, "encapsulateBits", rest)
}

pub(crate) extern "C" fn subtle_crypto_decapsulate_bits_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest(this, "decapsulateBits", rest)
}

pub(crate) extern "C" fn subtle_crypto_encapsulate_key_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest(this, "encapsulateKey", rest)
}

pub(crate) extern "C" fn subtle_crypto_decapsulate_key_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest(this, "decapsulateKey", rest)
}

/// Install a single callable static method on a constructor closure as a
/// `{ writable: true, enumerable: false, configurable: true }` data property
/// (matching Node's descriptors for built-in statics). `info` is the body's
/// (its call arity or rest parameter included); `spec_length` is the
/// function's `.length`.
pub(crate) fn install_constructor_static(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    info: *const crate::closure::JsFunctionInfo,
    spec_length: u32,
) {
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return;
    }
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    super::super::define_builtin_data_property(
        ctor as *mut ObjectHeader,
        key,
        value,
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
        super::super::define_builtin_data_property(
            ctor as *mut ObjectHeader,
            key,
            value,
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
            install_constructor_static(
                ctor,
                "keys",
                crate::fn_info!(object_keys_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "values",
                crate::fn_info!(object_values_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "entries",
                crate::fn_info!(object_entries_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "freeze",
                crate::fn_info!(object_freeze_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "create",
                crate::fn_info!(object_create_thunk, 2; with_declared(2)),
                2,
            );
            install_constructor_static(
                ctor,
                "seal",
                crate::fn_info!(object_seal_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "isSealed",
                crate::fn_info!(object_is_sealed_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "isFrozen",
                crate::fn_info!(object_is_frozen_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "isExtensible",
                crate::fn_info!(object_is_extensible_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "preventExtensions",
                crate::fn_info!(object_prevent_extensions_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "is",
                crate::fn_info!(object_is_thunk, 2; with_declared(2)),
                2,
            );
            install_constructor_static(
                ctor,
                "setPrototypeOf",
                crate::fn_info!(object_set_prototype_of_thunk, 2; with_declared(2)),
                2,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertySymbols",
                crate::fn_info!(object_get_own_property_symbols_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertyDescriptors",
                crate::fn_info!(object_get_own_property_descriptors_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "defineProperties",
                crate::fn_info!(object_define_properties_thunk, 2; with_declared(2)),
                2,
            );
            install_constructor_static(
                ctor,
                "groupBy",
                crate::fn_info!(object_group_by_thunk, 2; with_declared(2)),
                2,
            );
            install_constructor_static(
                ctor,
                "getPrototypeOf",
                crate::fn_info!(object_get_prototype_of_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertyNames",
                crate::fn_info!(object_get_own_property_names_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertyDescriptor",
                crate::fn_info!(object_get_own_property_descriptor_thunk, 2; with_declared(2)),
                2,
            );
            install_constructor_static(
                ctor,
                "defineProperty",
                crate::fn_info!(object_define_property_thunk, 3; with_declared(3)),
                3,
            );
            install_constructor_static(
                ctor,
                "fromEntries",
                crate::fn_info!(object_from_entries_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "assign",
                crate::fn_info!(object_assign_thunk, 2; with_rest(1)),
                2,
            );
            install_constructor_static(
                ctor,
                "hasOwn",
                crate::fn_info!(object_hasown_thunk, 2; with_declared(2)),
                2,
            );
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
                crate::fn_info!(object_prototype_has_own_property_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "isPrototypeOf",
                crate::fn_info!(object_prototype_is_prototype_of_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "propertyIsEnumerable",
                crate::fn_info!(object_prototype_property_is_enumerable_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "toString",
                crate::fn_info!(object_prototype_to_string_thunk, 0; with_declared(0)),
                0,
            );
            install_constructor_static(
                ctor,
                "toLocaleString",
                crate::fn_info!(object_prototype_to_locale_string_thunk, 0; with_declared(0)),
                0,
            );
            install_constructor_static(
                ctor,
                "valueOf",
                crate::fn_info!(object_prototype_value_of_thunk, 0; with_declared(0)),
                0,
            );
        }
        "Array" => {
            install_constructor_static(
                ctor,
                "isArray",
                crate::fn_info!(array_is_array_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "from",
                crate::fn_info!(array_from_thunk, 3; with_declared(3)),
                1,
            );
            install_constructor_static(
                ctor,
                "of",
                crate::fn_info!(array_of_thunk, 1; with_rest(0)),
                0,
            );
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
                if let Some((info, spec_length)) =
                    super::bigint_promise::promise_static_function_info(static_name)
                {
                    install_constructor_static(ctor, static_name, info, spec_length);
                }
            }
        }
        "Map" => install_constructor_static(
            ctor,
            "groupBy",
            crate::fn_info!(map_group_by_thunk, 2; with_declared(2)),
            2,
        ),
        "RegExp" => install_constructor_static(
            ctor,
            "escape",
            crate::fn_info!(regexp_escape_thunk, 1; with_declared(1)),
            1,
        ),
        "Date" => {
            // `Date.now` / `Date.parse` / `Date.UTC` as real own data props
            // (thunks live in `date_proto_thunks`). The functional calls are
            // codegen intrinsics, so this only affects value reads + reflection.
            date_proto_thunks::install_date_constructor_statics(ctor);
        }
        "Number" => {
            install_constructor_static(
                ctor,
                "isNaN",
                crate::fn_info!(number_is_nan_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "isFinite",
                crate::fn_info!(number_is_finite_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "isInteger",
                crate::fn_info!(number_is_integer_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "isSafeInteger",
                crate::fn_info!(number_is_safe_integer_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "parseFloat",
                crate::fn_info!(number_parse_float_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "parseInt",
                crate::fn_info!(number_parse_int_thunk, 2; with_declared(2)),
                2,
            );
        }
        "BigInt" => {
            // BigInt.asIntN(bits, bigint) / asUintN(bits, bigint) — spec length 2.
            install_constructor_static(
                ctor,
                "asIntN",
                crate::fn_info!(bigint_as_int_n_thunk, 2; with_declared(2)),
                2,
            );
            install_constructor_static(
                ctor,
                "asUintN",
                crate::fn_info!(bigint_as_uint_n_thunk, 2; with_declared(2)),
                2,
            );
        }
        "Symbol" => {
            install_constructor_static(
                ctor,
                "for",
                crate::fn_info!(symbol_for_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "keyFor",
                crate::fn_info!(symbol_key_for_thunk, 1; with_declared(1)),
                1,
            );
            for name in ["iterator", "asyncIterator"] {
                let symbol = crate::symbol::well_known_symbol(name);
                crate::closure::closure_set_dynamic_prop(
                    ctor as usize,
                    name,
                    crate::value::js_nanbox_pointer(symbol as i64),
                );
                super::super::set_builtin_property_attrs(
                    ctor as usize,
                    name.to_string(),
                    super::super::PropertyAttrs::new(false, false, false),
                );
            }
        }
        "String" => {
            // #4627: reify the variadic `String.fromCharCode` / `fromCodePoint`
            // statics so they are real function values (correct `.name` /
            // `.length`, usable via reference / spread). Call-arity 0 (all args
            // collected into `rest`) with spec `.length` 1. `String.raw` (a tag
            // function) is left on its intrinsic path for now.
            install_constructor_static(
                ctor,
                "fromCharCode",
                crate::fn_info!(string_from_char_code_static, 1; with_rest(0)),
                1,
            );
            install_constructor_static(
                ctor,
                "fromCodePoint",
                crate::fn_info!(string_from_code_point_static, 1; with_rest(0)),
                1,
            );
            // #4627: `String.raw` (tag function) — 1 fixed param (template
            // object) + rest substitutions; spec `.length` 1.
            install_constructor_static(
                ctor,
                "raw",
                crate::fn_info!(string_raw_static, 2; with_rest(1)),
                1,
            );
        }
        "ArrayBuffer" => {
            install_constructor_static(
                ctor,
                "isView",
                crate::fn_info!(array_buffer_is_view_thunk, 1; with_declared(1)),
                1,
            );
        }
        "AbortSignal" => {
            // The call forms are codegen intrinsics; these are the real own
            // data properties a value read (`const f = AbortSignal.abort`)
            // and reflection see.
            install_constructor_static(
                ctor,
                "abort",
                crate::fn_info!(abort_signal_abort_thunk, 1; with_declared(0)),
                0,
            );
            install_constructor_static(
                ctor,
                "timeout",
                crate::fn_info!(abort_signal_timeout_thunk, 1; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "any",
                crate::fn_info!(abort_signal_any_thunk, 1; with_declared(1)),
                1,
            );
        }
        "Response" => {
            install_constructor_static(
                ctor,
                "error",
                crate::fn_info!(global_this_response_error_thunk, 0; with_declared(0)),
                0,
            );
            install_constructor_static(
                ctor,
                "json",
                crate::fn_info!(global_this_response_json_thunk, 2; with_declared(2)),
                1,
            );
            install_constructor_static(
                ctor,
                "redirect",
                crate::fn_info!(global_this_response_redirect_thunk, 2; with_declared(2)),
                1,
            );
        }
        #[cfg(feature = "global-url")]
        "URL" => {
            install_constructor_static(
                ctor,
                "canParse",
                crate::fn_info!(url_can_parse_thunk, 2; with_declared(1)),
                1,
            );
            install_constructor_static(
                ctor,
                "parse",
                crate::fn_info!(url_parse_thunk, 2; with_declared(1)),
                1,
            );
        }
        #[cfg(feature = "global-webcrypto")]
        "SubtleCrypto" => {
            install_constructor_static(
                ctor,
                "supports",
                crate::fn_info!(subtle_crypto_supports_thunk, 1; with_rest(0)),
                2,
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
                crate::fn_info!(proxy_revocable_thunk, 2; with_declared(2)),
                2,
            );
        }
        // #6674: TC39 `Uint8Array.fromBase64(str, opts)` / `fromHex(str)` static
        // factories, materialized as readable own statics on the `Uint8Array`
        // constructor closure. The direct call form is already intercepted at
        // compile time (HIR `module_static.rs`), so these thunks back the value
        // read (`typeof Uint8Array.fromBase64 === "function"`, the jose/Auth.js
        // feature-detection gate) and the rebound-call form
        // (`const f = Uint8Array.fromBase64; f(s)`). They route to the same
        // `js_u8_from_base64` / `js_u8_from_hex` runtime helpers the compile-time
        // path uses, so the produced value is identical. `fromBase64.length` is
        // 1 (the optional opts is the 2nd, undefined-padded, ABI slot).
        "Uint8Array" => {
            install_constructor_static(
                ctor,
                "fromBase64",
                crate::fn_info!(uint8array_from_base64_thunk, 2; with_declared(2)),
                1,
            );
            install_constructor_static(
                ctor,
                "fromHex",
                crate::fn_info!(uint8array_from_hex_thunk, 1; with_declared(1)),
                1,
            );
        }
        _ => {}
    }
}

/// Install a method on a prototype object as a callable closure value with
/// the proper `name` property and `.length`. Used to reify built-in
/// prototype methods so `Array.prototype.map`, `Date.prototype.toISOString`,
/// etc. read back as `typeof === "function"` (issue #2142) — the actual
/// method *call* path is already covered by codegen's NativeMethodCall and
/// the `try_builtin_prototype_method_apply_call` HIR rewrite, so the no-op
/// thunk backing here is only invoked when user code calls the method
/// through indirection (`const m = Array.prototype.map; m.call(arr, fn)`),
/// a rare pattern. The reification is the value-read parity win.
///
/// The body defaults to `global_this_builtin_noop_thunk` (returns
/// undefined) for methods we don't have a dedicated thunk for; callers
/// that want spec-accurate call behavior pass a custom thunk instead
/// (`array_prototype_slice_thunk`, `object_prototype_to_string_thunk`).
/// `info` is the body's (a built-in: `FN_BUILTIN`, declared `arity`).
pub(crate) fn install_proto_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    info: *const crate::closure::JsFunctionInfo,
    arity: u32,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    super::super::native_module::set_bound_native_closure_name(closure, method_name);
    // #3143: record this method's spec `.length` per closure instance — many
    // methods share the noop body. Read back by the `.length` value-accessor
    // and `getOwnPropertyDescriptor`.
    super::super::native_module::set_builtin_closure_length(closure as usize, arity);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    // Built-in prototype methods are `{ writable: true, enumerable: false,
    // configurable: true }` per spec. Record that descriptor (reflection-only,
    // no hot-path gate flip) so `Object.getOwnPropertyDescriptor`, `Object.keys`
    // and `for-in` all observe them as non-enumerable — Test262's `verifyProperty`
    // checks every built-in method this way. See `set_builtin_property_attrs`.
    super::super::define_builtin_data_property(
        proto_obj,
        key,
        value,
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
    super::super::define_builtin_data_property(
        proto_obj,
        key,
        value,
        alias_name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

/// [`install_proto_method`] for a rest body: `info` carries its rest
/// parameter (`with_rest(fixed_arity)`) and `FN_BUILTIN`; `.length` is
/// `fixed_arity`.
pub(crate) fn install_proto_method_rest(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    info: *const crate::closure::JsFunctionInfo,
    fixed_arity: u32,
) {
    install_proto_method_rest_with_length(proto_obj, method_name, info, fixed_arity);
}

/// [`install_proto_method_rest`] with a `.length` other than the fixed
/// arity.
pub(crate) fn install_proto_method_rest_with_length(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    info: *const crate::closure::JsFunctionInfo,
    spec_length: u32,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    super::super::native_module::set_bound_native_closure_name(closure, method_name);
    super::super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    super::super::define_builtin_data_property(
        proto_obj,
        key,
        value,
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
    const METHODS: &[(&str, *const crate::closure::JsFunctionInfo, u32)] = &[
        (
            "parse",
            crate::fn_info!(json_parse_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "stringify",
            crate::fn_info!(json_stringify_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "rawJSON",
            crate::fn_info!(json_raw_json_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        (
            "isRawJSON",
            crate::fn_info!(json_is_raw_json_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
    ];
    for (name, info, arity) in METHODS.iter().copied() {
        install_proto_method(ns_obj, name, info, arity);
    }
}

/// #4139: reify the `Reflect` namespace's own methods for reflection parity.
/// See `install_math_namespace` for the rationale.
pub(crate) fn install_reflect_namespace_members(ns_obj: *mut ObjectHeader) {
    // Every member is REAL as a value (#5989 for `construct`, then the rest):
    // primordials-style captures (`const ReflectOwnKeys = Reflect.ownKeys`)
    // call these through stored bindings, and a noop stub silently returned
    // `undefined` — a hardened-primordials library's `recursiveFreeze` walked
    // `Reflect.ownKeys(obj)` at class-static-init time and threw on
    // destructuring the stub's `undefined` result.
    let methods = [
        (
            "defineProperty",
            crate::fn_info!(reflect_define_property_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "deleteProperty",
            crate::fn_info!(reflect_delete_property_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "apply",
            crate::fn_info!(reflect_apply_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "construct",
            crate::fn_info!(reflect_construct_thunk, 3; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "get",
            crate::fn_info!(reflect_get_thunk, 3; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "getOwnPropertyDescriptor",
            crate::fn_info!(reflect_get_own_property_descriptor_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "getPrototypeOf",
            crate::fn_info!(reflect_get_prototype_of_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        (
            "has",
            crate::fn_info!(reflect_has_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "isExtensible",
            crate::fn_info!(reflect_is_extensible_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        (
            "ownKeys",
            crate::fn_info!(reflect_own_keys_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        (
            "preventExtensions",
            crate::fn_info!(reflect_prevent_extensions_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        (
            "set",
            crate::fn_info!(reflect_set_thunk, 4; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "setPrototypeOf",
            crate::fn_info!(reflect_set_prototype_of_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
    ];
    for (name, info, arity) in methods {
        install_proto_method(ns_obj, name, info, arity);
    }
}

pub(crate) fn install_atomics_namespace_members(ns_obj: *mut ObjectHeader) {
    for (name, info, arity) in [
        (
            "load",
            crate::fn_info!(crate::atomics::js_atomics_load, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "isLockFree",
            crate::fn_info!(crate::atomics::js_atomics_is_lock_free, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        (
            "store",
            crate::fn_info!(crate::atomics::js_atomics_store, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "add",
            crate::fn_info!(crate::atomics::js_atomics_add, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "sub",
            crate::fn_info!(crate::atomics::js_atomics_sub, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "and",
            crate::fn_info!(crate::atomics::js_atomics_and, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "or",
            crate::fn_info!(crate::atomics::js_atomics_or, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "xor",
            crate::fn_info!(crate::atomics::js_atomics_xor, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "exchange",
            crate::fn_info!(crate::atomics::js_atomics_exchange, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "compareExchange",
            crate::fn_info!(crate::atomics::js_atomics_compare_exchange, 4; with_declared(4), with_flags(crate::closure::FN_BUILTIN)),
            4,
        ),
        (
            "notify",
            crate::fn_info!(crate::atomics::js_atomics_notify, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "wait",
            crate::fn_info!(crate::atomics::js_atomics_wait, 4; with_declared(4), with_flags(crate::closure::FN_BUILTIN)),
            4,
        ),
        (
            "waitAsync",
            crate::fn_info!(crate::atomics::js_atomics_wait_async, 4; with_declared(4), with_flags(crate::closure::FN_BUILTIN)),
            4,
        ),
    ] {
        install_proto_method(ns_obj, name, info, arity);
    }
}

/// Install a list of `(method_name, arity)` pairs on a prototype object.
/// Most entries are reflection-only methods backed by
/// `global_this_builtin_noop_thunk`, but inherited Object methods with
/// observable receiver-sensitive behavior use their real thunk.
pub(crate) fn install_noop_proto_methods(proto_obj: *mut ObjectHeader, methods: &[(&str, u32)]) {
    // One info per body: the per-method `.length` rides on each closure
    // (`install_proto_method` records it), so the bodies need no declared
    // arity of their own.
    use crate::closure::FN_BUILTIN;
    use crate::fn_info;
    for (name, arity) in methods.iter().copied() {
        let info = match name {
            "isPrototypeOf" => {
                fn_info!(object_prototype_is_prototype_of_thunk, 1; with_flags(FN_BUILTIN))
            }
            // Annex B accessor methods get real thunks (reflective `.call`).
            "__defineGetter__" => {
                fn_info!(object_prototype_define_getter_thunk, 2; with_flags(FN_BUILTIN))
            }
            "__defineSetter__" => {
                fn_info!(object_prototype_define_setter_thunk, 2; with_flags(FN_BUILTIN))
            }
            "__lookupGetter__" => {
                fn_info!(object_prototype_lookup_getter_thunk, 1; with_flags(FN_BUILTIN))
            }
            "__lookupSetter__" => {
                fn_info!(object_prototype_lookup_setter_thunk, 1; with_flags(FN_BUILTIN))
            }
            _ => fn_info!(global_this_builtin_noop_thunk, 1; with_flags(FN_BUILTIN)),
        };
        install_proto_method(proto_obj, name, info, arity);
    }
}

pub(crate) extern "C" fn url_pattern_test_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    input: f64,
    rest: f64,
) -> f64 {
    let base = rest_first_arg(rest);
    let this_value = this.as_f64();
    let pattern = crate::value::js_nanbox_get_pointer(this_value) as *mut ObjectHeader;
    crate::url::js_url_pattern_test(pattern, input, base)
}

pub(crate) extern "C" fn url_pattern_exec_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    input: f64,
    rest: f64,
) -> f64 {
    let base = rest_first_arg(rest);
    let this_value = this.as_f64();
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

/// `get [Symbol.species]` shared by every built-in constructor that carries
/// one: returns the `this` value, so a subclass that inherits the accessor
/// answers itself (ECMA-262 23.1.2.5, 24.1.2.3, 24.2.2.2, 25.1.5.3,
/// 27.2.4.8, 22.2.5.2, 23.2.2.4).
pub(crate) extern "C" fn builtin_species_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(this.bits())
}

/// Install the standard own `get [Symbol.species]` accessor
/// (`{ get, set: undefined, enumerable: false, configurable: true }`) on a
/// built-in constructor (#11193). Each constructor gets its own getter
/// function object, as in Node.
pub(crate) fn install_builtin_species_accessor(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    let sym = crate::symbol::well_known_symbol("species");
    if sym.is_null() {
        return;
    }
    let getter = crate::closure::js_closure_alloc(
        crate::fn_info!(builtin_species_getter_thunk, 0; with_declared(0)),
        0,
    );
    if getter.is_null() {
        return;
    }
    super::super::native_module::set_bound_native_closure_name(getter, "get [Symbol.species]");
    super::super::native_module::set_builtin_closure_length(getter as usize, 0);
    let get_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
    let ctor_value = crate::value::js_nanbox_pointer(ctor as i64);
    let sym_value = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
    unsafe {
        crate::symbol::set_symbol_accessor_property(ctor_value, sym_value, get_bits, 0);
        crate::symbol::set_symbol_property_attrs(
            ctor as usize,
            sym as usize,
            super::super::PropertyAttrs::new(false, false, true),
        );
    }
}
