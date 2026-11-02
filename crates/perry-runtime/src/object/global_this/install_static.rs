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
    super::super::native_module::set_bound_native_closure_metadata(closure, name, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);

    let value = crate::value::js_nanbox_pointer(closure as i64);
    if !ctor_ptr.is_null() {
        crate::closure::closure_define_dynamic_prop(ctor_ptr as usize, name, value);
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
    let signal = crate::event_target_hook_slots::js_abort_signal_abort(reason);
    crate::value::js_nanbox_pointer(signal as i64)
}

extern "C" fn abort_signal_timeout_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    ms: f64,
) -> f64 {
    let signal = crate::event_target_hook_slots::js_abort_signal_timeout(ms);
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
    let signal = crate::event_target_hook_slots::js_abort_signal_any(array);
    crate::value::js_nanbox_pointer(signal as i64)
}

extern "C" fn url_can_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    input: f64,
    base: f64,
) -> f64 {
    let input_ptr = crate::url_hooks::url_coerce_string(input);
    let ok = if base.to_bits() == crate::value::TAG_UNDEFINED {
        crate::url_hooks::url_can_parse(input_ptr)
    } else {
        let base_ptr = crate::url_hooks::url_coerce_string(base);
        crate::url_hooks::url_can_parse_with_base(input_ptr, base_ptr)
    };
    f64::from_bits(crate::value::JSValue::bool(ok != 0).bits())
}

extern "C" fn url_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    input: f64,
    base: f64,
) -> f64 {
    let input_ptr = crate::url_hooks::url_coerce_string(input);
    let url = if base.to_bits() == crate::value::TAG_UNDEFINED {
        crate::url_hooks::url_parse(input_ptr)
    } else {
        let base_ptr = crate::url_hooks::url_coerce_string(base);
        crate::url_hooks::url_parse_with_base(input_ptr, base_ptr)
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
    let Some(value) = builtin_static_function(name, info, spec_length) else {
        return;
    };
    define_constructor_static(ctor, name, value, STATIC_METHOD_ATTRS);
}

/// `{ writable: true, enumerable: false, configurable: true }`: Node's
/// descriptor for a built-in static method.
const STATIC_METHOD_ATTRS: super::super::PropertyAttrs =
    super::super::PropertyAttrs::new(true, false, true);

/// A builtin static method's function object: `name`, spec `.length`, not a
/// constructor. `None` when the allocation failed.
fn builtin_static_function(
    name: &str,
    info: *const crate::closure::JsFunctionInfo,
    spec_length: u32,
) -> Option<f64> {
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return None;
    }
    super::super::native_module::set_bound_native_closure_metadata(closure, name, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    Some(crate::value::js_nanbox_pointer(closure as i64))
}

/// Define one own data property `name` on the constructor function `ctor`.
fn define_constructor_static(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    value: f64,
    attrs: super::super::PropertyAttrs,
) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    super::super::define_builtin_data_property(
        ctor as *mut ObjectHeader,
        key,
        value,
        name.to_string(),
        attrs,
    );
}

/// A builtin constructor's static properties, collected and then defined
/// together: a fresh constructor's own-property bag is born holding all of
/// them in ONE shape ([`crate::closure::closure_define_first_props`]), not
/// grown by one key-add transition per static. Each keeps its attributes.
/// A constructor that already has own properties defines them one by one.
///
/// The collected values are raw function pointers, so collection is
/// suppressed from [`ConstructorStatics::new`] to [`ConstructorStatics::finish`].
pub(crate) struct ConstructorStatics {
    ctor: *mut crate::closure::ClosureHeader,
    entries: Vec<(&'static str, f64)>,
    attrs: Vec<u8>,
    _no_move: crate::gc::GcSuppressScope,
}

impl ConstructorStatics {
    pub(crate) fn new(ctor: *mut crate::closure::ClosureHeader) -> Self {
        ConstructorStatics {
            ctor,
            entries: Vec::new(),
            attrs: Vec::new(),
            _no_move: crate::gc::GcSuppressScope::new(),
        }
    }

    /// A static method (`{ writable, !enumerable, configurable }`).
    pub(crate) fn method(
        &mut self,
        name: &'static str,
        info: *const crate::closure::JsFunctionInfo,
        spec_length: u32,
    ) {
        self.method_with_attrs(name, info, spec_length, STATIC_METHOD_ATTRS);
    }

    /// A static method with its own attributes.
    pub(crate) fn method_with_attrs(
        &mut self,
        name: &'static str,
        info: *const crate::closure::JsFunctionInfo,
        spec_length: u32,
        attrs: super::super::PropertyAttrs,
    ) {
        if let Some(value) = builtin_static_function(name, info, spec_length) {
            self.data(name, value, attrs);
        }
    }

    /// A static data property.
    pub(crate) fn data(
        &mut self,
        name: &'static str,
        value: f64,
        attrs: super::super::PropertyAttrs,
    ) {
        self.entries.push((name, value));
        self.attrs
            .push(super::super::key_attrs::attr_bits_to_entry(attrs.bits));
    }

    /// Define everything collected on the constructor.
    pub(crate) fn finish(self) {
        let ctor = self.ctor as usize;
        if self.entries.is_empty() {
            return;
        }
        if crate::closure::closure_define_first_props_with_attrs(ctor, &self.entries, &self.attrs) {
            // What the per-key define's store does besides storing: a
            // function taking a named property arms the own-override guard
            // (builtin form), and each key gets its attributes.
            super::super::own_override::as_builtin_definition(|| {
                super::super::own_override::note_exotic_named_prop_install(ctor)
            });
            return;
        }
        for (&(name, value), &entry) in self.entries.iter().zip(&self.attrs) {
            let attrs = super::super::PropertyAttrs {
                bits: super::super::key_attrs::entry_to_attr_bits(entry),
            };
            define_constructor_static(self.ctor, name, value, attrs);
        }
    }
}

/// `Number`'s constant statics (`{ !writable, !enumerable, !configurable }`),
/// born in the constructor's one attributed layout with its methods.
const NUMBER_STATIC_DATA: [(&str, f64); 8] = [
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

/// #2889: install the common static methods on the `Object` / `Array`
/// constructor closures so rebound usage (`const O = Object; O.keys(x)`)
/// dispatches through the real runtime helpers. Only the high-traffic
/// statics with simple f64-in / f64-out shapes are reified here; the long
/// tail (`Object.defineProperty`, `Object.getOwnPropertyDescriptor`, …)
/// stays unreified on the rebound value and is a known scope gap.
// Only intrinsic construction calls this; the installed functions keep the
// runtime's speed optimization and ordinary inlining policy (#13662).
#[optimize(size)]
pub(crate) fn install_builtin_constructor_statics(
    name: &str,
    ctor: *mut crate::closure::ClosureHeader,
    prototype: Option<f64>,
) {
    if ctor.is_null() {
        return;
    }
    let mut statics = ConstructorStatics::new(ctor);
    // Intrinsics and statics belong to the same final own-property layout.
    let length = builtin_constructor_spec_length(name)
        .or_else(|| crate::closure::closure_length(ctor))
        .unwrap_or(realm_stream_constructor_public!(length));
    statics.data(
        "length",
        length as f64,
        super::super::PropertyAttrs::new(false, false, true),
    );
    let name_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    statics.data(
        "name",
        crate::value::js_nanbox_string(name_ptr as i64),
        super::super::PropertyAttrs::new(false, false, true),
    );
    let s = &mut statics;
    match name {
        "Object" => {
            s.method(
                "keys",
                crate::fn_info!(#[realm(Object.keys)] export crate::object::global_this::bigint_promise::object_keys_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "values",
                crate::fn_info!(#[realm(Object.values)] export crate::object::global_this::bigint_promise::object_values_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "entries",
                crate::fn_info!(#[realm(Object.entries)] export crate::object::global_this::bigint_promise::object_entries_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "freeze",
                crate::fn_info!(#[realm(Object.freeze)] export crate::object::global_this::bigint_promise::object_freeze_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "create",
                crate::fn_info!(#[realm(Object.create)] export crate::object::global_this::bigint_promise::object_create_thunk, 2; with_declared(2)),
                2,
            );
            s.method(
                "seal",
                crate::fn_info!(#[realm(Object.seal)] export crate::object::global_this::bigint_promise::object_seal_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "isSealed",
                crate::fn_info!(#[realm(Object.isSealed)] export crate::object::global_this::bigint_promise::object_is_sealed_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "isFrozen",
                crate::fn_info!(#[realm(Object.isFrozen)] export crate::object::global_this::bigint_promise::object_is_frozen_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "isExtensible",
                crate::fn_info!(#[realm(Object.isExtensible)] export crate::object::global_this::bigint_promise::object_is_extensible_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "preventExtensions",
                crate::fn_info!(#[realm(Object.preventExtensions)] export crate::object::global_this::bigint_promise::object_prevent_extensions_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "is",
                crate::fn_info!(#[realm(Object.is)] export crate::object::global_this::bigint_promise::object_is_thunk, 2; with_declared(2)),
                2,
            );
            s.method(
                "setPrototypeOf",
                crate::fn_info!(#[realm(Object.setPrototypeOf)] export crate::object::global_this::bigint_promise::object_set_prototype_of_thunk, 2; with_declared(2)),
                2,
            );
            s.method(
                "getOwnPropertySymbols",
                crate::fn_info!(#[realm(Object.getOwnPropertySymbols)] export crate::object::global_this::bigint_promise::object_get_own_property_symbols_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "getOwnPropertyDescriptors",
                crate::fn_info!(#[realm(Object.getOwnPropertyDescriptors)] export crate::object::global_this::bigint_promise::object_get_own_property_descriptors_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "defineProperties",
                crate::fn_info!(#[realm(Object.defineProperties)] export crate::object::global_this::bigint_promise::object_define_properties_thunk, 2; with_declared(2)),
                2,
            );
            s.method(
                "groupBy",
                crate::fn_info!(#[realm(Object.groupBy)] export crate::object::global_this::bigint_promise::object_group_by_thunk, 2; with_declared(2)),
                2,
            );
            s.method(
                "getPrototypeOf",
                crate::fn_info!(#[realm(Object.getPrototypeOf)] export crate::object::global_this::bigint_promise::object_get_prototype_of_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "getOwnPropertyNames",
                crate::fn_info!(#[realm(Object.getOwnPropertyNames)] export crate::object::global_this::bigint_promise::object_get_own_property_names_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "getOwnPropertyDescriptor",
                crate::fn_info!(#[realm(Object.getOwnPropertyDescriptor)] export crate::object::global_this::bigint_promise::object_get_own_property_descriptor_thunk, 2; with_declared(2)),
                2,
            );
            s.method(
                "defineProperty",
                crate::fn_info!(#[realm(Object.defineProperty)] export crate::object::global_this::bigint_promise::object_define_property_thunk, 3; with_declared(3)),
                3,
            );
            s.method(
                "fromEntries",
                crate::fn_info!(#[realm(Object.fromEntries)] export crate::object::global_this::bigint_promise::object_from_entries_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "assign",
                crate::fn_info!(#[realm(Object.assign, spec = {length = 2}, emit = metadata(realm_object_assign_metadata))] export crate::object::global_this::bigint_promise::object_assign_thunk, 2; with_rest(1)),
                realm_object_assign_metadata!(length),
            );
            s.method(
                "hasOwn",
                crate::fn_info!(#[realm(Object.hasOwn)] export crate::object::global_this::bigint_promise::object_hasown_thunk, 2; with_declared(2)),
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
            s.method(
                "hasOwnProperty",
                crate::fn_info!(#[realm(Object.hasOwnProperty)] export crate::object::global_this::array_error::object_prototype_has_own_property_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "isPrototypeOf",
                crate::fn_info!(#[realm(Object.isPrototypeOf)] export crate::object::global_this::array_error::object_prototype_is_prototype_of_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "propertyIsEnumerable",
                crate::fn_info!(#[realm(Object.propertyIsEnumerable)] export crate::object::global_this::array_error::object_prototype_property_is_enumerable_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "toString",
                crate::fn_info!(#[realm(Object.toString)] export crate::object::global_this::array_error::object_prototype_to_string_thunk, 0; with_declared(0)),
                0,
            );
            s.method(
                "toLocaleString",
                crate::fn_info!(#[realm(Object.toLocaleString)] export crate::object::global_this::array_error::object_prototype_to_locale_string_thunk, 0; with_declared(0)),
                0,
            );
            s.method(
                "valueOf",
                crate::fn_info!(#[realm(Object.valueOf)] export crate::object::global_this::array_error::object_prototype_value_of_thunk, 0; with_declared(0)),
                0,
            );
        }
        "Array" => {
            s.method(
                "isArray",
                crate::fn_info!(#[realm(Array.isArray)] export crate::object::global_this::bigint_promise::array_is_array_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "from",
                crate::fn_info!(#[realm(Array.from, spec = {length = 1}, emit = metadata(realm_array_from_metadata))] export crate::object::global_this::bigint_promise::array_from_thunk, 3; with_declared(3)),
                realm_array_from_metadata!(length),
            );
            #[rustfmt::skip] s.method("of", crate::fn_info!(#[realm(Array.of)] export crate::object::global_this::bigint_promise::array_of_thunk, 1; with_rest(0)), 0);
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
                    s.method(static_name, info, spec_length);
                }
            }
        }
        "Map" => install_constructor_static(
            ctor,
            "groupBy",
            crate::fn_info!(#[realm(Map.groupBy)] export crate::object::global_this::bigint_promise::map_group_by_thunk, 2; with_declared(2)),
            2,
        ),
        "RegExp" => install_constructor_static(
            ctor,
            "escape",
            crate::fn_info!(#[realm(RegExp.escape)] export crate::object::global_this::bigint_promise::regexp_escape_thunk, 1; with_declared(1)),
            1,
        ),
        "Date" => {
            // `Date.now` / `Date.parse` / `Date.UTC` as real own data props
            // (thunks live in `date_proto_thunks`). The functional calls are
            // codegen intrinsics, so this only affects value reads + reflection.
            date_proto_thunks::install_date_constructor_statics(s);
        }
        "Number" => {
            s.method(
                "isNaN",
                crate::fn_info!(#[realm(Number.isNaN)] export crate::object::global_this::bigint_promise::number_is_nan_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "isFinite",
                crate::fn_info!(#[realm(Number.isFinite)] export crate::object::global_this::bigint_promise::number_is_finite_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "isInteger",
                crate::fn_info!(#[realm(Number.isInteger)] export crate::object::global_this::bigint_promise::number_is_integer_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "isSafeInteger",
                crate::fn_info!(#[realm(Number.isSafeInteger)] export crate::object::global_this::bigint_promise::number_is_safe_integer_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "parseFloat",
                crate::fn_info!(number_parse_float_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "parseInt",
                crate::fn_info!(number_parse_int_thunk, 2; with_declared(2)),
                2,
            );
            for (name, value) in NUMBER_STATIC_DATA {
                s.data(
                    name,
                    value,
                    super::super::PropertyAttrs::new(false, false, false),
                );
            }
        }
        "BigInt" => {
            // BigInt.asIntN(bits, bigint) / asUintN(bits, bigint) — spec length 2.
            s.method(
                "asIntN",
                crate::fn_info!(#[realm(BigInt.asIntN)] export crate::object::global_this::bigint_promise::bigint_as_int_n_thunk, 2; with_declared(2)),
                2,
            );
            s.method(
                "asUintN",
                crate::fn_info!(#[realm(BigInt.asUintN)] export crate::object::global_this::bigint_promise::bigint_as_uint_n_thunk, 2; with_declared(2)),
                2,
            );
        }
        "Symbol" => {
            s.method(
                "for",
                crate::fn_info!(#[realm(Symbol.for)] export crate::object::global_this::bigint_promise::symbol_for_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "keyFor",
                crate::fn_info!(#[realm(Symbol.keyFor)] export crate::object::global_this::bigint_promise::symbol_key_for_thunk, 1; with_declared(1)),
                1,
            );
            for name in ["iterator", "asyncIterator"] {
                let symbol = crate::symbol::well_known_symbol(name);
                s.data(
                    name,
                    crate::value::js_nanbox_pointer(symbol as i64),
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
            s.method(
                "fromCharCode",
                crate::fn_info!(#[realm(String.fromCharCode, spec = {length = 1}, emit = metadata(realm_string_fromcharcode_metadata))] export crate::object::global_this::bigint_promise::string_from_char_code_static, 1; with_rest(0)),
                realm_string_fromcharcode_metadata!(length),
            );
            s.method(
                "fromCodePoint",
                crate::fn_info!(#[realm(String.fromCodePoint, spec = {length = 1}, emit = metadata(realm_string_fromcodepoint_metadata))] export crate::object::global_this::bigint_promise::string_from_code_point_static, 1; with_rest(0)),
                realm_string_fromcodepoint_metadata!(length),
            );
            // #4627: `String.raw` (tag function) — 1 fixed param (template
            // object) + rest substitutions; spec `.length` 1.
            s.method(
                "raw",
                crate::fn_info!(#[realm(String.raw)] export crate::object::global_this::bigint_promise::string_raw_static, 2; with_rest(1)),
                1,
            );
        }
        "ArrayBuffer" => {
            s.method(
                "isView",
                crate::fn_info!(#[realm(ArrayBuffer.isView)] export crate::object::global_this::typed_array::array_buffer_is_view_thunk, 1; with_declared(1)),
                1,
            );
        }
        "AbortSignal" => {
            // The call forms are codegen intrinsics; these are the real own
            // data properties a value read (`const f = AbortSignal.abort`)
            // and reflection see.
            s.method(
                "abort",
                crate::fn_info!(#[realm(AbortSignal.abort)] export crate::object::global_this::install_static::abort_signal_abort_thunk, 1; with_declared(0)),
                0,
            );
            s.method(
                "timeout",
                crate::fn_info!(#[realm(AbortSignal.timeout)] export crate::object::global_this::install_static::abort_signal_timeout_thunk, 1; with_declared(1)),
                1,
            );
            s.method(
                "any",
                crate::fn_info!(#[realm(AbortSignal.any)] export crate::object::global_this::install_static::abort_signal_any_thunk, 1; with_declared(1)),
                1,
            );
        }
        "Response" => {
            s.method(
                "error",
                crate::fn_info!(#[realm(Response.error)] export crate::object::global_this::fetch_globals::global_this_response_error_thunk, 0; with_declared(0)),
                0,
            );
            s.method(
                "json",
                crate::fn_info!(#[realm(Response.json, spec = {length = 1}, emit = metadata(realm_response_json_metadata))] export crate::object::global_this::fetch_globals::global_this_response_json_thunk, 2; with_declared(2)),
                realm_response_json_metadata!(length),
            );
            s.method(
                "redirect",
                crate::fn_info!(#[realm(Response.redirect, spec = {length = 1}, emit = metadata(realm_response_redirect_metadata))] export crate::object::global_this::fetch_globals::global_this_response_redirect_thunk, 2; with_declared(2)),
                realm_response_redirect_metadata!(length),
            );
        }
        "Iterator" => crate::iterator_helpers::install_iterator_statics(s),
        "URL" => {
            super::web_hooks::statics(super::web_hooks::WebGroup::Url, name, s);
        }
        "SubtleCrypto" => {
            super::web_hooks::statics(super::web_hooks::WebGroup::Webcrypto, name, s);
        }
        "Proxy" => {
            s.method(
                "revocable",
                crate::fn_info!(#[realm(Proxy.revocable)] export crate::object::global_this::builtin_thunks::proxy_revocable_thunk, 2; with_declared(2)),
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
            s.method(
                "fromBase64",
                crate::fn_info!(#[realm(Uint8Array.fromBase64, spec = {length = 1}, emit = metadata(realm_uint8array_frombase64_metadata))] export crate::object::global_this::install_static::uint8array_from_base64_thunk, 2; with_declared(2)),
                realm_uint8array_frombase64_metadata!(length),
            );
            s.method(
                "fromHex",
                crate::fn_info!(#[realm(Uint8Array.fromHex)] export crate::object::global_this::install_static::uint8array_from_hex_thunk, 1; with_declared(1)),
                1,
            );
        }
        _ => {}
    }
    // Keep the existing insertion order: prototype is last. The generic
    // small-key lookup scans backwards, so it remains a one-key probe.
    if let Some(prototype) = prototype {
        statics.data(
            "prototype",
            prototype,
            super::super::PropertyAttrs::new(false, false, false),
        );
    }
    statics.finish();
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
    install_proto_method_with_key(proto_obj, method_name, info, arity, std::ptr::null())
}

/// Allocate the same builtin function for individual installs and bulk births.
/// Keep the individual installer's allocation/boxing order and inlined body.
#[inline(always)]
pub(super) fn create_builtin_method(
    method_name: &str,
    info: *const crate::closure::JsFunctionInfo,
    arity: u32,
) -> *mut crate::closure::ClosureHeader {
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return closure;
    }
    super::super::native_module::set_bound_native_closure_metadata(closure, method_name, arity);
    // The allocation's body record already owns this immutable capability.
    // Shared bodies without the bit still need an instance-specific entry.
    if info.is_null() || unsafe { (*info).flags & crate::closure::FN_NON_CONSTRUCTOR == 0 } {
        super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    }
    closure
}

/// Install through the same attributed store with an already owned key atom.
pub(crate) fn install_proto_method_with_key(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    info: *const crate::closure::JsFunctionInfo,
    arity: u32,
    key: *const crate::StringHeader,
) -> f64 {
    let closure = create_builtin_method(method_name, info, arity);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = if key.is_null() {
        crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32)
    } else {
        key as *mut crate::StringHeader
    };
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
    super::super::native_module::set_bound_native_closure_metadata(
        closure,
        method_name,
        spec_length,
    );
    // The allocation's body record already owns this immutable capability.
    // Shared bodies without the bit still need an instance-specific entry.
    if info.is_null() || unsafe { (*info).flags & crate::closure::FN_NON_CONSTRUCTOR == 0 } {
        super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    }
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    super::super::define_builtin_data_property(
        proto_obj,
        key,
        value,
        method_name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    value
}

pub(crate) fn install_atomics_namespace_members(ns_obj: *mut ObjectHeader) {
    for (name, info, arity) in [
        (
            "load",
            crate::fn_info!(#[realm(Atomics.load)] export crate::atomics::js_atomics_load, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
            2,
        ),
        (
            "isLockFree",
            crate::fn_info!(#[realm(Atomics.isLockFree)] export crate::atomics::js_atomics_is_lock_free, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        (
            "store",
            crate::fn_info!(#[realm(Atomics.store)] export crate::atomics::js_atomics_store, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "add",
            crate::fn_info!(#[realm(Atomics.add)] export crate::atomics::js_atomics_add, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "sub",
            crate::fn_info!(#[realm(Atomics.sub)] export crate::atomics::js_atomics_sub, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "and",
            crate::fn_info!(#[realm(Atomics.and)] export crate::atomics::js_atomics_and, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "or",
            crate::fn_info!(#[realm(Atomics.or)] export crate::atomics::js_atomics_or, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "xor",
            crate::fn_info!(#[realm(Atomics.xor)] export crate::atomics::js_atomics_xor, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "exchange",
            crate::fn_info!(#[realm(Atomics.exchange)] export crate::atomics::js_atomics_exchange, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "compareExchange",
            crate::fn_info!(#[realm(Atomics.compareExchange)] export crate::atomics::js_atomics_compare_exchange, 4; with_declared(4), with_flags(crate::closure::FN_BUILTIN)),
            4,
        ),
        (
            "notify",
            crate::fn_info!(#[realm(Atomics.notify)] export crate::atomics::js_atomics_notify, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
            3,
        ),
        (
            "wait",
            crate::fn_info!(#[realm(Atomics.wait)] export crate::atomics::js_atomics_wait, 4; with_declared(4), with_flags(crate::closure::FN_BUILTIN)),
            4,
        ),
        (
            "waitAsync",
            crate::fn_info!(#[realm(Atomics.waitAsync)] export crate::atomics::js_atomics_wait_async, 4; with_declared(4), with_flags(crate::closure::FN_BUILTIN)),
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
            // A call reads `hasOwnProperty` / `propertyIsEnumerable` off the
            // receiver like any method, so the copy a builtin prototype
            // carries must be the real method, never a placeholder.
            "hasOwnProperty" => {
                fn_info!(object_prototype_has_own_property_thunk, 1; with_flags(FN_BUILTIN))
            }
            "propertyIsEnumerable" => {
                fn_info!(object_prototype_property_is_enumerable_thunk, 1; with_flags(FN_BUILTIN))
            }
            "valueOf" => {
                fn_info!(#[realm(realm_decl! { AggregateError.prototype.valueOf; Array.prototype.valueOf; ArrayBuffer.prototype.valueOf; AsyncDisposableStack.prototype.valueOf; Blob.prototype.valueOf; Crypto.prototype.valueOf; CryptoKey.prototype.valueOf; DataView.prototype.valueOf; DisposableStack.prototype.valueOf; Error.prototype.valueOf; EvalError.prototype.valueOf; File.prototype.valueOf; FinalizationRegistry.prototype.valueOf; FormData.prototype.valueOf; Headers.prototype.valueOf; Map.prototype.valueOf; Promise.prototype.valueOf; RangeError.prototype.valueOf; ReferenceError.prototype.valueOf; RegExp.prototype.valueOf; Request.prototype.valueOf; Response.prototype.valueOf; Set.prototype.valueOf; SharedArrayBuffer.prototype.valueOf; SubtleCrypto.prototype.valueOf; SyntaxError.prototype.valueOf; TextDecoder.prototype.valueOf; TextEncoder.prototype.valueOf; TypeError.prototype.valueOf; URIError.prototype.valueOf; URLSearchParams.prototype.valueOf; WeakMap.prototype.valueOf; WeakRef.prototype.valueOf; WeakSet.prototype.valueOf; WebSocket.prototype.valueOf; })] export crate::object::global_this::array_error::object_prototype_value_of_thunk, 0; with_flags(FN_BUILTIN))
            }
            // Annex B accessor methods get real thunks (reflective `.call`).
            "__defineGetter__" => {
                fn_info!(#[realm(Object.prototype.__defineGetter__)] export crate::object::global_this::array_error::object_prototype_define_getter_thunk, 2; with_flags(FN_BUILTIN))
            }
            "__defineSetter__" => {
                fn_info!(#[realm(Object.prototype.__defineSetter__)] export crate::object::global_this::array_error::object_prototype_define_setter_thunk, 2; with_flags(FN_BUILTIN))
            }
            "__lookupGetter__" => {
                fn_info!(#[realm(Object.prototype.__lookupGetter__)] export crate::object::global_this::array_error::object_prototype_lookup_getter_thunk, 1; with_flags(FN_BUILTIN))
            }
            "__lookupSetter__" => {
                fn_info!(#[realm(Object.prototype.__lookupSetter__)] export crate::object::global_this::array_error::object_prototype_lookup_setter_thunk, 1; with_flags(FN_BUILTIN))
            }
            _ => fn_info!(#[realm(realm_decl! { FormData.prototype.delete; FormData.prototype.forEach; FormData.prototype.get; FormData.prototype.getAll; FormData.prototype.has; Headers.prototype.delete; Headers.prototype.forEach; Headers.prototype.get; Headers.prototype.has; AggregateError.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Array.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Array.prototype.toSpliced, spec = {length = 2}, emit = metadata(realm_noop_tospliced_2_metadata); ArrayBuffer.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); AsyncDisposableStack.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Blob.prototype.arrayBuffer, spec = {length = 0}, emit = metadata(realm_noop_arraybuffer_0_metadata); Blob.prototype.bytes, spec = {length = 0}, emit = metadata(realm_noop_bytes_0_metadata); Blob.prototype.slice, spec = {length = 0}, emit = metadata(realm_noop_slice_0_metadata); Blob.prototype.stream, spec = {length = 0}, emit = metadata(realm_noop_stream_0_metadata); Blob.prototype.text, spec = {length = 0}, emit = metadata(realm_noop_text_0_metadata); Blob.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Boolean.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Crypto.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); CryptoKey.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); DataView.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Date.prototype.toDateString, spec = {length = 0}, emit = metadata(realm_noop_todatestring_0_metadata); Date.prototype.toTimeString, spec = {length = 0}, emit = metadata(realm_noop_totimestring_0_metadata); DisposableStack.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Error.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); EvalError.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); File.prototype.arrayBuffer, spec = {length = 0}, emit = metadata(realm_noop_arraybuffer_0_metadata); File.prototype.bytes, spec = {length = 0}, emit = metadata(realm_noop_bytes_0_metadata); File.prototype.slice, spec = {length = 0}, emit = metadata(realm_noop_slice_0_metadata); File.prototype.stream, spec = {length = 0}, emit = metadata(realm_noop_stream_0_metadata); File.prototype.text, spec = {length = 0}, emit = metadata(realm_noop_text_0_metadata); File.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); FinalizationRegistry.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); FormData.prototype.append, spec = {length = 2}, emit = metadata(realm_noop_append_2_metadata); FormData.prototype.entries, spec = {length = 0}, emit = metadata(realm_noop_entries_0_metadata); FormData.prototype.keys, spec = {length = 0}, emit = metadata(realm_noop_keys_0_metadata); FormData.prototype.set, spec = {length = 2}, emit = metadata(realm_noop_set_2_metadata); FormData.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); FormData.prototype.values, spec = {length = 0}, emit = metadata(realm_noop_values_0_metadata); Headers.prototype.append, spec = {length = 2}, emit = metadata(realm_noop_append_2_metadata); Headers.prototype.getSetCookie, spec = {length = 0}, emit = metadata(realm_noop_getsetcookie_0_metadata); Headers.prototype.keys, spec = {length = 0}, emit = metadata(realm_noop_keys_0_metadata); Headers.prototype.set, spec = {length = 2}, emit = metadata(realm_noop_set_2_metadata); Headers.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Headers.prototype.values, spec = {length = 0}, emit = metadata(realm_noop_values_0_metadata); Promise.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); RangeError.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); ReferenceError.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); RegExp.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Request.prototype.arrayBuffer, spec = {length = 0}, emit = metadata(realm_noop_arraybuffer_0_metadata); Request.prototype.blob, spec = {length = 0}, emit = metadata(realm_noop_blob_0_metadata); Request.prototype.bytes, spec = {length = 0}, emit = metadata(realm_noop_bytes_0_metadata); Request.prototype.clone, spec = {length = 0}, emit = metadata(realm_noop_clone_0_metadata); Request.prototype.formData, spec = {length = 0}, emit = metadata(realm_noop_formdata_0_metadata); Request.prototype.json, spec = {length = 0}, emit = metadata(realm_noop_json_0_metadata); Request.prototype.text, spec = {length = 0}, emit = metadata(realm_noop_text_0_metadata); Request.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Response.prototype.arrayBuffer, spec = {length = 0}, emit = metadata(realm_noop_arraybuffer_0_metadata); Response.prototype.blob, spec = {length = 0}, emit = metadata(realm_noop_blob_0_metadata); Response.prototype.bytes, spec = {length = 0}, emit = metadata(realm_noop_bytes_0_metadata); Response.prototype.clone, spec = {length = 0}, emit = metadata(realm_noop_clone_0_metadata); Response.prototype.formData, spec = {length = 0}, emit = metadata(realm_noop_formdata_0_metadata); Response.prototype.json, spec = {length = 0}, emit = metadata(realm_noop_json_0_metadata); Response.prototype.text, spec = {length = 0}, emit = metadata(realm_noop_text_0_metadata); Response.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); SharedArrayBuffer.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); String.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); SubtleCrypto.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); Symbol.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); SyntaxError.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); TextDecoder.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); TextEncoder.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); TypeError.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); URIError.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); URLSearchParams.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); WeakRef.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); WebSocket.prototype.toLocaleString, spec = {length = 0}, emit = metadata(realm_noop_tolocalestring_0_metadata); })] export crate::object::global_this::fetch_globals::global_this_builtin_noop_thunk, 1; with_flags(FN_BUILTIN)),
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
    crate::url_hooks::url_pattern_test(pattern, input, base)
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
    crate::url_hooks::url_pattern_exec(pattern, input, base)
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
        crate::fn_info!(#[realm(realm_decl! { "%TypedArray%[\"@@species\"].<get>", constructible = true; "ArrayBuffer[\"@@species\"].<get>", constructible = true; "Array[\"@@species\"].<get>", constructible = true; "Map[\"@@species\"].<get>", constructible = true; "Promise[\"@@species\"].<get>", constructible = true; "RegExp[\"@@species\"].<get>", constructible = true; "Set[\"@@species\"].<get>", constructible = true; "SharedArrayBuffer[\"@@species\"].<get>", constructible = true; })] export crate::object::global_this::install_static::builtin_species_getter_thunk, 0; with_declared(0)),
        0,
    );
    if getter.is_null() {
        return;
    }
    super::super::native_module::set_bound_native_closure_metadata(
        getter,
        "get [Symbol.species]",
        0,
    );
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

/// The `global-url` constructor statics, installed by `web_hooks`.
pub(super) fn web_url_statics(name: &str, s: &mut ConstructorStatics) {
    match name {
        "URL" => {
            s.method(
                "canParse",
                crate::fn_info!(#[realm(URL.canParse)] export crate::object::global_this::install_static::url_can_parse_thunk, 2; with_declared(1)),
                1,
            );
            s.method(
                "parse",
                crate::fn_info!(#[realm(URL.parse)] export crate::object::global_this::install_static::url_parse_thunk, 2; with_declared(1)),
                1,
            );
        }
        _ => {}
    }
}

/// The `global-webcrypto` constructor statics, installed by `web_hooks`.
pub(super) fn web_webcrypto_statics(name: &str, s: &mut ConstructorStatics) {
    match name {
        "SubtleCrypto" => {
            s.method_with_attrs(
                "supports",
                crate::fn_info!(#[realm(SubtleCrypto.supports, spec = {length = 2}, emit = metadata(realm_subtlecrypto_supports_metadata))] export crate::object::global_this::install_static::subtle_crypto_supports_thunk, 1; with_rest(0)),
                realm_subtlecrypto_supports_metadata!(length),
                super::super::PropertyAttrs::new(true, true, true),
            );
        }
        _ => {}
    }
}
