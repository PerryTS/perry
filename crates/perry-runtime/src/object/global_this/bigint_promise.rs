use super::super::*;
use super::*;

fn nanbox_array_or_undef(arr: *mut crate::array::ArrayHeader) -> f64 {
    if arr.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(arr as i64)
    }
}

pub(crate) extern "C" fn object_keys_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    nanbox_array_or_undef(super::super::js_object_keys_value(value))
}

pub(crate) extern "C" fn object_values_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    nanbox_array_or_undef(super::super::js_object_values_value(value))
}

pub(crate) extern "C" fn object_entries_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    nanbox_array_or_undef(super::super::js_object_entries_value(value))
}

pub(crate) extern "C" fn object_freeze_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_freeze(value)
}

pub(crate) extern "C" fn object_create_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    props: f64,
) -> f64 {
    if props.to_bits() == crate::value::TAG_UNDEFINED {
        super::super::js_object_create(value)
    } else {
        super::super::js_object_create_with_props(value, props)
    }
}

pub(crate) extern "C" fn object_seal_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_seal(value)
}

pub(crate) extern "C" fn object_is_sealed_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_is_sealed(value)
}

pub(crate) extern "C" fn object_is_frozen_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_is_frozen(value)
}

pub(crate) extern "C" fn object_is_extensible_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_is_extensible(value)
}

pub(crate) extern "C" fn object_prevent_extensions_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_prevent_extensions(value)
}

pub(crate) extern "C" fn object_is_thunk(
    _closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
) -> f64 {
    super::super::js_object_is(a, b)
}

pub(crate) extern "C" fn object_set_prototype_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    proto: f64,
) -> f64 {
    super::super::js_object_set_prototype_of(obj, proto)
}

pub(crate) extern "C" fn object_get_own_property_symbols_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let arr = unsafe { crate::symbol::js_object_get_own_property_symbols(value) };
    crate::value::js_nanbox_pointer(arr)
}

pub(crate) extern "C" fn object_get_own_property_descriptors_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_get_own_property_descriptors(value)
}

pub(crate) extern "C" fn object_define_properties_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    descriptors: f64,
) -> f64 {
    super::super::js_object_define_properties(target, descriptors)
}

pub(crate) extern "C" fn object_group_by_thunk(
    _closure: *const crate::closure::ClosureHeader,
    items: f64,
    callback: f64,
) -> f64 {
    super::super::js_object_group_by(items, callback)
}

pub(crate) extern "C" fn object_get_prototype_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_get_prototype_of(value)
}

pub(crate) extern "C" fn object_get_own_property_names_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_get_own_property_names(value)
}

pub(crate) extern "C" fn object_get_own_property_descriptor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    key: f64,
) -> f64 {
    super::super::js_object_get_own_property_descriptor(obj, key)
}

pub(crate) extern "C" fn object_define_property_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    key: f64,
    descriptor: f64,
) -> f64 {
    super::super::js_object_define_property(obj, key, descriptor)
}

pub(crate) extern "C" fn object_from_entries_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::super::js_object_from_entries(value)
}

pub(crate) extern "C" fn object_assign_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    rest: f64,
) -> f64 {
    let validated = unsafe { super::super::js_object_assign_validate_target(target) };
    for source in global_this_rest_array_values(rest) {
        unsafe { super::super::js_object_assign_one(validated, source) };
    }
    validated
}

/// `Object.hasOwn(obj, key)` (ES2022) reified as a callable value so the
/// feature-detect idiom `typeof Object.hasOwn === "undefined" ? … :
/// Object.hasOwn` (iconv-lite's merge-exports, #3527) binds a real callable
/// instead of a non-callable handle. Backed by the same runtime helper as
/// `Object.prototype.hasOwnProperty.call(obj, key)`.
pub(crate) extern "C" fn object_hasown_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    key: f64,
) -> f64 {
    super::super::object_ops::js_object_has_own(obj, key)
}

pub(crate) extern "C" fn array_is_array_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::array::js_array_is_array(value)
}

pub(crate) extern "C" fn array_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    // Reflective `Array.from.call(C, items)` / `Array.from.apply(C, [items])`
    // binds `C` as the implicit `this`. Read it FIRST (before any nested call
    // can overwrite it) and run the spec algorithm — when `C IsConstructor`,
    // the result is built via `Construct(C)`. A plain reflective call (no
    // explicit receiver) leaves `this` as undefined / a non-constructor, so
    // the default `%Array%` path is taken.
    let c = crate::object::js_implicit_this_get();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    crate::array::array_from_full(c, value, undefined, undefined)
}

pub(crate) extern "C" fn array_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    // Reflective `Array.of.call(C, ...items)` binds `C` as the implicit `this`.
    // Read it FIRST (before any nested call can overwrite it); when `C
    // IsConstructor` the result is built via `Construct(C, «len»)`, otherwise the
    // default `%Array%` path is taken. See `array_of_full` (ECMA-262 §23.1.2.3).
    let c = crate::object::js_implicit_this_get();
    let vals = global_this_rest_array_values(rest);
    crate::array::array_of_full(c, &vals)
}

pub(crate) extern "C" fn number_is_nan_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_nan(value)
}

pub(crate) extern "C" fn number_is_finite_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_finite(value)
}

pub(crate) extern "C" fn number_is_integer_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_integer(value)
}

/// Shared impl for `BigInt.asIntN`/`asUintN` (both the ctor-static thunks and
/// the `("bigint", ...)` native-module dispatch). Coerces `bits` via ToIndex
/// (RangeError on negative/non-integer), brand-checks `value` is a BigInt
/// (TypeError otherwise), and returns the NaN-boxed result. `signed` selects
/// asIntN vs asUintN. Diverges (`!`) on bad input, matching Node.
/// `ToBigInt(value)` for `BigInt.asIntN`/`asUintN`'s second argument. BigInt
/// passes through; Boolean → 0n/1n; String → StringToBigInt; an object is first
/// reduced through ToPrimitive("number") (running its `valueOf`/`toString`) and
/// re-coerced; a Number/undefined/null/Symbol throws a TypeError. The
/// primitive cases reuse the same `to_bigint_for_store` helper that backs
/// `BigInt64Array` element writes.
fn bigint_to_bigint_arg(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() && !jv.is_bigint() {
        // Array → ToPrimitive finds no `valueOf` override and falls to
        // `Array.prototype.toString` = `join(",")`, then ToBigInt on that string
        // (`[] => "" => 0n`, `[10n] => "10" => 10n`, `[1,2] => "1,2" => throws`).
        // `js_to_primitive` doesn't apply array join, so handle it first —
        // mirrors the array arm in `js_number_coerce`. #2378.
        const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
        if crate::array::js_array_is_array(value).to_bits() == TAG_TRUE_BITS {
            let arr_ptr = jv.as_pointer::<crate::array::ArrayHeader>();
            let comma = crate::string::js_string_from_bytes(b",".as_ptr(), 1);
            let joined = unsafe { crate::array::js_array_join(arr_ptr, comma) };
            return bigint_to_bigint_arg(crate::value::js_nanbox_string(joined as i64));
        }
        // Object: ToPrimitive("number") then re-coerce. Try a custom
        // [Symbol.toPrimitive] first, then OrdinaryToPrimitive
        // (valueOf-before-toString). A primitive result recurses; anything
        // unconvertible falls through to the TypeError in `to_bigint_for_store`.
        let prim = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if prim.to_bits() != value.to_bits() {
            return bigint_to_bigint_arg(prim);
        }
        if let crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) =
            unsafe { crate::value::ordinary_to_primitive_number_for_add(value) }
        {
            if p.to_bits() != value.to_bits() {
                return bigint_to_bigint_arg(p);
            }
        }
    }
    crate::typedarray::bigint::to_bigint_for_store(value)
}

pub(crate) fn bigint_as_n_dispatch(bits_arg: f64, value_arg: f64, signed: bool) -> f64 {
    // Step 1: `bits = ? ToIndex(bits)`. ToIndex = ToIntegerOrInfinity(ToNumber)
    // with a `0 <= n <= 2^53-1` range check. `js_number_coerce` is the full
    // ToNumber (strings, booleans, null/undefined, and objects via
    // ToPrimitive("number") — so a `bits` object's `valueOf`/`toString` runs
    // here, BEFORE `value` is touched, preserving the spec coercion order).
    let bits_num = crate::builtins::js_number_coerce(bits_arg);
    let bits_int = if bits_num.is_nan() {
        0.0
    } else {
        bits_num.trunc()
    };
    if !(0.0..=9_007_199_254_740_991.0).contains(&bits_int) {
        crate::fs::validate::throw_range_error_with_code(
            "The number of bits is invalid (must be a non-negative integer)",
        );
    }
    // Step 2: `bigint = ? ToBigInt(bigint)`. ToBigInt coerces BigInt / Boolean /
    // String (and objects via ToPrimitive); a Number/undefined/null/Symbol
    // throws a TypeError. Runs strictly after ToIndex(bits) above.
    let value_bigint = bigint_to_bigint_arg(value_arg);
    let jv = JSValue::from_bits(value_bigint.to_bits());
    let bits = bits_int as u32;
    let ptr = jv.as_bigint_ptr() as *const crate::bigint::BigIntHeader;
    let r = if signed {
        crate::bigint::js_bigint_as_int_n(bits, ptr)
    } else {
        crate::bigint::js_bigint_as_uint_n(bits, ptr)
    };
    f64::from_bits(crate::value::js_nanbox_bigint(r as i64).to_bits())
}

/// FFI entry for the codegen-lowered `BigInt.asIntN(bits, x)` direct call.
#[no_mangle]
pub extern "C" fn js_bigint_as_int_n_call(bits: f64, value: f64) -> f64 {
    bigint_as_n_dispatch(bits, value, true)
}

/// FFI entry for the codegen-lowered `BigInt.asUintN(bits, x)` direct call.
#[no_mangle]
pub extern "C" fn js_bigint_as_uint_n_call(bits: f64, value: f64) -> f64 {
    bigint_as_n_dispatch(bits, value, false)
}

pub(crate) extern "C" fn bigint_as_int_n_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bits: f64,
    value: f64,
) -> f64 {
    bigint_as_n_dispatch(bits, value, true)
}

pub(crate) extern "C" fn bigint_as_uint_n_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bits: f64,
    value: f64,
) -> f64 {
    bigint_as_n_dispatch(bits, value, false)
}

pub(crate) extern "C" fn json_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    text: f64,
    reviver: f64,
) -> f64 {
    let text_ptr = crate::value::js_get_string_pointer_unified(text) as *const crate::StringHeader;
    let reviver_value = JSValue::from_bits(reviver.to_bits());
    let parsed = unsafe {
        if reviver_value.is_pointer()
            && crate::closure::is_closure_ptr(reviver_value.as_pointer::<u8>() as usize)
        {
            crate::json::js_json_parse_with_reviver(
                text_ptr,
                reviver_value.as_pointer::<crate::closure::ClosureHeader>() as i64,
            )
        } else {
            crate::json::js_json_parse(text_ptr)
        }
    };
    f64::from_bits(parsed.bits())
}

pub(crate) extern "C" fn json_stringify_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    replacer: f64,
    space: f64,
) -> f64 {
    f64::from_bits(unsafe { crate::json::js_json_stringify_full(value, replacer, space) as u64 })
}

pub(crate) extern "C" fn json_raw_json_thunk(
    _closure: *const crate::closure::ClosureHeader,
    text: f64,
) -> f64 {
    unsafe { crate::json::js_json_raw_json(text) }
}

pub(crate) extern "C" fn json_is_raw_json_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    unsafe { crate::json::js_json_is_raw_json(value) }
}

pub(crate) extern "C" fn reflect_apply_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    this_arg: f64,
    args: f64,
) -> f64 {
    crate::proxy::js_reflect_apply(target, this_arg, args)
}

/// #5989: the reified `Reflect.construct` VALUE was a no-op stub, so any
/// call through a captured binding — Next.js 16's cacheComponents Date
/// extension does `const construct = Reflect.construct; ...
/// construct(OriginalDate, arguments, new.target)` inside its installed
/// wrapper — silently returned `undefined` and the construction result
/// fell back to the unbranded implicit `this` ("Invalid Date" instances
/// everywhere once the wrapper installs). Route to the real
/// `js_reflect_construct`; a missing third argument arrives as
/// `undefined`, which it already resolves to `target` per spec.
pub(crate) extern "C" fn reflect_construct_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    args_like: f64,
    new_target: f64,
) -> f64 {
    crate::proxy::js_reflect_construct(target, args_like, new_target)
}

pub(crate) extern "C" fn symbol_for_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    unsafe { crate::symbol::js_symbol_for(key) }
}

pub(crate) extern "C" fn symbol_key_for_thunk(
    _closure: *const crate::closure::ClosureHeader,
    symbol: f64,
) -> f64 {
    unsafe { crate::symbol::js_symbol_key_for(symbol) }
}

pub(crate) extern "C" fn number_is_safe_integer_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_safe_integer(value)
}

// #4627: reified `String.fromCharCode(...units)` / `fromCodePoint(...points)`.
// Both collect all arguments into `rest` (call-arity 0), so `rest` is already
// the array-like the array-form runtime helpers expect.
pub(crate) extern "C" fn string_from_char_code_static(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let s = crate::string::js_string_from_char_code_array(rest);
    crate::value::js_nanbox_string(s as i64)
}

pub(crate) extern "C" fn string_from_code_point_static(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let s = crate::string::js_string_from_code_point_array(rest);
    crate::value::js_nanbox_string(s as i64)
}

// #4521: reified `Promise` statics so `Promise.all` / `Promise.resolve` / etc.
// are first-class function values (correct `.name` / `.length`, usable via
// reference, `.call`, `.apply`, spread). Direct calls (`Promise.all([...])`)
// still take the codegen fast path in `lower_call/console_promise.rs`; these
// thunks back value reads and rebound/`.call` usage by delegating to the same
// runtime entry points the direct-call path emits. Spec-internal observable
// semantics (per-iteration `this.resolve`, real resolve-element closures with
// `[[AlreadyCalled]]`, `NewPromiseCapability(this)`) are a follow-up — these
// thunks intentionally use the native Promise machinery regardless of `this`.
extern "C" fn promise_resolve_static(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_resolve_spec(this_ctor, value)
}

extern "C" fn promise_reject_static(
    _closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_reject_spec(this_ctor, reason)
}

extern "C" fn promise_all_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_all_spec(this_ctor, iterable)
}

extern "C" fn promise_race_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_race_spec(this_ctor, iterable)
}

extern "C" fn promise_all_settled_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_all_settled_spec(this_ctor, iterable)
}

extern "C" fn promise_any_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_any_spec(this_ctor, iterable)
}

extern "C" fn promise_with_resolvers_static(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_with_resolvers_spec(this_ctor)
}

// `Promise.try(fn, ...args)`: call-arity 1 (callback) + rest (forwarded args).
extern "C" fn promise_try_static(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    rest: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_try_spec(this_ctor, callback, rest)
}

// #4627: reified `String.raw(callSite, ...substitutions)` tag function. One
// fixed param (the template/cooked object) then a rest of substitutions, which
// `js_string_raw` reads by numeric index — so `rest` (the collected array) is
// passed straight through as the substitutions array-like.
pub(crate) extern "C" fn string_raw_static(
    _closure: *const crate::closure::ClosureHeader,
    call_site: f64,
    rest: f64,
) -> f64 {
    let s = crate::string::js_string_raw(call_site, rest);
    crate::value::js_nanbox_string(s as i64)
}

pub(crate) extern "C" fn number_parse_float_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    global_this_parse_float_thunk(closure, value)
}

pub(crate) extern "C" fn number_parse_int_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
    radix: f64,
) -> f64 {
    global_this_parse_int_thunk(closure, value, radix)
}

pub(crate) extern "C" fn typed_array_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    source: f64,
    map_fn: f64,
    this_arg: f64,
) -> f64 {
    // §%TypedArray%.from step 1-2: `C` is the `this` value; if `IsConstructor(C)`
    // is false, throw a TypeError — BEFORE the source is read. Invoked as a plain
    // function (`var from = TA.from; from([])`) the sloppy `this` is `globalThis`
    // (not a constructor), so this must fire even though a source is supplied
    // (test262 `from/invoked-as-func`). A concrete TA `this` (kind known) is a
    // constructor by definition.
    let kind_opt = typed_array_constructor_this_kind();
    if kind_opt.is_none() {
        require_typed_array_from_of_constructor();
    }
    // Spec order: validate the map callback BEFORE the source is read.
    let mapped = map_fn.to_bits() != crate::value::TAG_UNDEFINED;
    let map_closure = if mapped {
        crate::array::js_validate_array_callback(map_fn) as *const crate::closure::ClosureHeader
    } else {
        std::ptr::null()
    };
    // Read the source's RAW kValues — its `@@iterator` invoked, or its
    // `ToLength(length)` + indexed elements evaluated — any throwing user
    // iterator/getter propagates (test262 from/arylk-*-error).
    let raw = unsafe { crate::typedarray::typed_array_from_source_raw_values(source) };
    // Per-element `mappedValue = Call(mapfn, T, «kValue, k»)` then
    // `Set(target, k, mappedValue)` — the map call and the (observable,
    // possibly throwing) element coercion INTERLEAVE per spec, so an abrupt
    // coercion at element k means the map callback never ran for k+1
    // (test262 from/set-value-abrupt-completion).
    let map_at = |k: usize, v: f64| -> f64 {
        if map_closure.is_null() {
            return v;
        }
        let prev = crate::object::js_implicit_this_set(this_arg);
        let r = crate::closure::js_closure_call2(map_closure, v, k as f64);
        crate::object::js_implicit_this_set(prev);
        r
    };
    if let Some(kind) = kind_opt {
        let out = crate::typedarray::typed_array_alloc(kind, raw.len() as u32);
        for (k, &v) in raw.iter().enumerate() {
            let m = map_at(k, v);
            unsafe { crate::typedarray_props::species_result_store(out as usize, k, m) };
        }
        return crate::value::js_nanbox_pointer(out as i64);
    }
    // Custom `this` constructor: TypedArrayCreate(C, «len») then per-element
    // [[Set]] (same interleave).
    let len = raw.len();
    let len_arg = [f64::from_bits(
        crate::value::JSValue::number(len as f64).bits(),
    )];
    let ctor = crate::object::js_implicit_this_get();
    let target = unsafe { super::super::js_new_function_construct(ctor, len_arg.as_ptr(), 1) };
    let addr = crate::typedarray_props::typed_array_addr_from_value(target).unwrap_or_else(|| {
        super::super::object_ops::throw_object_type_error(
            b"TypedArray.from/of constructor did not return a TypedArray",
        )
    });
    let ta_ptr = addr as *mut crate::typedarray::TypedArrayHeader;
    let target_len = unsafe { crate::typedarray::js_typed_array_length(ta_ptr) } as usize;
    if target_len < len {
        super::super::object_ops::throw_object_type_error(
            b"Derived TypedArray constructor created an array which was too small",
        );
    }
    for (k, &v) in raw.iter().enumerate() {
        let m = map_at(k, v);
        unsafe { crate::typedarray_props::species_result_store(addr, k, m) };
    }
    target
}

/// `%TypedArray%.from`/`.of` step "If IsConstructor(`this`) is false, throw a
/// TypeError". Only called when the `this` value is not a concrete typed-array
/// constructor (kind unknown); a user constructor passes, anything else throws.
fn require_typed_array_from_of_constructor() {
    let this_ctor = crate::object::js_implicit_this_get();
    if !value_is_constructor(this_ctor) {
        super::super::object_ops::throw_object_type_error(
            b"TypedArray.from/of called with a `this` that is not a constructor",
        );
    }
}

/// `IsConstructor(value)` for the typed-array `from`/`of` `this` check: a class
/// ref, a proxy, or a non-arrow user closure that is not a flagged
/// non-constructable builtin.
fn value_is_constructor(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE {
        return true; // class-ref constructor
    }
    if crate::proxy::js_proxy_is_proxy(value) == 1 {
        return true;
    }
    if (bits >> 48) == 0x7FFD {
        let raw = (bits & crate::value::POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(raw) {
            if crate::closure::closure_is_arrow(raw as *const crate::closure::ClosureHeader) {
                return false;
            }
            return !super::super::native_module::builtin_closure_is_non_constructable_value(value);
        }
    }
    false
}

/// Build the result of `%TypedArray%.from` / `%TypedArray%.of` from a
/// materialized values array, honoring a custom `this` constructor.
///
/// When `this` is a concrete typed-array constructor (`Int8Array`, …) the
/// fast path builds the view directly. Otherwise (`%TypedArray%.from.call(
/// userCtor, …)`) the spec's `TypedArrayCreate(C, «len»)` is realized by
/// `Construct(C, [len])` and the values are written into the result via the
/// element [[Set]] path — so a user constructor that throws propagates, and one
/// that returns an arbitrary (sufficiently long) typed array is used verbatim
/// (test262 `from/of` `custom-ctor*`).
fn typed_array_create_from_values(
    kind_opt: Option<u8>,
    arr: *mut crate::array::ArrayHeader,
) -> f64 {
    if let Some(kind) = kind_opt {
        let ta = crate::typedarray::js_typed_array_new_from_array(kind as i32, arr);
        return crate::value::js_nanbox_pointer(ta as i64);
    }
    let ctor = crate::object::js_implicit_this_get();
    let len = crate::array::js_array_length(arr) as usize;
    let len_arg = [f64::from_bits(
        crate::value::JSValue::number(len as f64).bits(),
    )];
    let target = unsafe { super::super::js_new_function_construct(ctor, len_arg.as_ptr(), 1) };
    // `TypedArrayCreate` requires the constructed object to be a typed array
    // with at least `len` elements.
    let addr = crate::typedarray_props::typed_array_addr_from_value(target).unwrap_or_else(|| {
        super::super::object_ops::throw_object_type_error(
            b"TypedArray.from/of constructor did not return a TypedArray",
        )
    });
    let ta_ptr = addr as *mut crate::typedarray::TypedArrayHeader;
    let target_len = unsafe { crate::typedarray::js_typed_array_length(ta_ptr) } as usize;
    if target_len < len {
        // `TypedArrayCreate(C, «len»)` throws a *TypeError* (not RangeError)
        // when the constructed typed array is shorter than the requested length
        // (test262 `from/of` `custom-ctor-returns-smaller-instance-throws`).
        super::super::object_ops::throw_object_type_error(
            b"Derived TypedArray constructor created an array which was too small",
        );
    }
    for k in 0..len {
        let v = crate::array::js_array_get(arr, k as u32);
        crate::typedarray::js_typed_array_set(ta_ptr, k as i32, f64::from_bits(v.bits()));
    }
    target
}

pub(crate) extern "C" fn typed_array_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let kind_opt = typed_array_constructor_this_kind();
    if kind_opt.is_none() {
        require_typed_array_from_of_constructor();
    }
    let vals = global_this_rest_array_values(rest);
    let len = vals.len() as u32;
    let arr = crate::array::js_array_alloc(len);
    unsafe {
        (*arr).length = len;
        for (i, &v) in vals.iter().enumerate() {
            crate::array::js_array_set_f64(arr, i as u32, v);
        }
    }
    typed_array_create_from_values(kind_opt, arr)
}

pub(crate) fn promise_static_function_spec(name: &str) -> Option<(*const u8, u32, u32, bool)> {
    // All eight statics use the spec-aware `*_static` thunks, which honor the
    // `this` constructor via `NewPromiseCapability(this)` — so a `Promise`
    // subclass (`class P extends Promise{}; P.all([...])`) or a valid custom
    // constructor (`Promise.all.call(C, ...)`) is accepted, while a
    // non-constructor `this` throws a TypeError from the capability flow.
    match name {
        "resolve" => Some((promise_resolve_static as *const u8, 1, 1, false)),
        "reject" => Some((promise_reject_static as *const u8, 1, 1, false)),
        "all" => Some((promise_all_static as *const u8, 1, 1, false)),
        "race" => Some((promise_race_static as *const u8, 1, 1, false)),
        "allSettled" => Some((promise_all_settled_static as *const u8, 1, 1, false)),
        "any" => Some((promise_any_static as *const u8, 1, 1, false)),
        "withResolvers" => Some((promise_with_resolvers_static as *const u8, 0, 0, false)),
        "try" => Some((promise_try_static as *const u8, 1, 1, true)),
        _ => None,
    }
}
