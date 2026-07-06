use super::super::*;
use super::*;

pub(crate) fn global_this_rest_array_values(rest: f64) -> Vec<f64> {
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

pub(crate) extern "C" fn function_prototype_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    rest: f64,
) -> f64 {
    let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let args = global_this_rest_array_values(rest);
    let (args_ptr, args_len) = if args.is_empty() {
        (std::ptr::null::<f64>(), 0)
    } else {
        (args.as_ptr(), args.len())
    };
    let this_arg = crate::closure::coerce_call_this(target, this_arg);
    // Concise/object-literal methods read `this` from a baked capture slot, not
    // IMPLICIT_THIS; rebind so the explicit `.call(thisArg)` receiver is honored.
    let target = crate::closure::rebind_explicit_this(target, this_arg);
    let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
    let result = unsafe { crate::closure::js_native_call_value(target, args_ptr, args_len) };
    IMPLICIT_THIS.with(|c| c.set(prev_this));
    result
}

/// `Function.prototype.bind` as a real callable thunk. Reads the target
/// function from `IMPLICIT_THIS` (set by `.call`/`.apply`/`Reflect.apply`),
/// flattens `(thisArg, ...boundArgs)` into one argument list, and delegates to
/// `js_function_bind` (which builds the BOUND_FUNCTION closure).
///
/// Previously `bind` was installed as a *no-op* proto method, so calling it as
/// a value — `Reflect.apply(Function.prototype.bind, fn, [thisArg])` or
/// `Function.prototype.bind.apply(fn, …)` — returned `undefined` instead of a
/// bound function. The `Function.prototype.call.bind(method)` uncurry idiom in
/// `call-bind-apply-helpers` (used by call-bound → side-channel → qs → Stripe)
/// hit exactly this: `Reflect.apply(bind, call, [fn])` yielded `undefined`.
pub(crate) extern "C" fn function_prototype_bind_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    rest: f64,
) -> f64 {
    let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let mut args: Vec<f64> = Vec::with_capacity(1);
    args.push(this_arg);
    args.extend(global_this_rest_array_values(rest));
    unsafe { crate::closure::js_function_bind(target, args.as_ptr(), args.len()) }
}

pub(crate) extern "C" fn global_this_set_timeout_thunk(
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

pub(crate) extern "C" fn global_this_clear_timeout_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) extern "C" fn global_this_set_interval_thunk(
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

pub(crate) extern "C" fn global_this_clear_interval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_interval_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) extern "C" fn global_this_set_immediate_thunk(
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

pub(crate) extern "C" fn global_this_clear_immediate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_immediate_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) extern "C" fn global_this_queue_microtask_thunk(
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
/// numbers/bools/functions → primitive/builtin tags, generic objects →
/// `"[object Object]"`.
///
/// Unblocks ramda's `_isArguments.js` IIFE which evaluates
/// `Object.prototype.toString.call(arguments)` at module-init time
/// — pre-fix the chained `Object.prototype.toString` read returned
/// `undefined`, so the `.call` access threw before the IIFE body ran.
pub(crate) extern "C" fn object_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    // Delegate to the canonical `js_object_to_string` so this callable form
    // (`const f = Object.prototype.toString; f.call(x)`) shares the full brand
    // table (Map/Set/WeakMap/Promise/RegExp/Symbol/BigInt/typed arrays/Date/
    // buffers/…). Previously this thunk duplicated a coarse discrimination that
    // mis-tagged typed arrays as `[object Number]` and everything beyond
    // Array/Error/Date as `[object Object]`.
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    unsafe { crate::object::js_object_to_string(f64::from_bits(this_bits)) }
}

pub(crate) extern "C" fn object_prototype_is_prototype_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    // Spec 20.1.3.3 step order: if V is not an Object, return false FIRST —
    // `Object.prototype.isPrototypeOf.call(undefined, 1)` is `false`, not a
    // TypeError. Symbols are POINTER_TAG'd in Perry but are primitives.
    let value_jsv = JSValue::from_bits(value.to_bits());
    if !value_jsv.is_pointer() || unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return f64::from_bits(JSValue::bool(false).bits());
    }
    // Step 2, ToObject(this): `.call(null, obj)` / `.call(undefined, obj)`
    // must throw a TypeError, matching the sibling Object.prototype methods.
    let this_jsv = JSValue::from_bits(this_value.to_bits());
    if this_jsv.is_null() || this_jsv.is_undefined() {
        super::super::object_ops::throw_object_type_error(
            b"Object.prototype.isPrototypeOf called on null or undefined",
        );
    }
    f64::from_bits(
        JSValue::bool(unsafe { super::super::js_object_is_prototype_of_value(this_value, value) })
            .bits(),
    )
}

/// #4533: native error subclass constructors whose `[[Prototype]]` is `Error`
/// (their `.prototype.[[Prototype]]` already links to `Error.prototype`).
pub(crate) fn is_native_error_subclass_constructor(name: &str) -> bool {
    matches!(
        name,
        "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
    )
}

pub(crate) extern "C" fn date_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    // ECMA-262 21.4.4.41 Date.prototype.toString does `thisTimeValue(this)`,
    // which throws a TypeError when the receiver has no [[DateValue]] slot
    // (`Date.prototype.toString.call(0)`, `.call({})`, `Date.prototype.toString()`
    // with this === Date.prototype). test262 Date/prototype/toString/non-date-receiver.
    if !crate::date::is_date_value(this_value) {
        super::super::object_ops::throw_object_type_error(b"this is not a Date object.");
    }
    let string = crate::date::js_date_to_string(this_value);
    crate::value::js_nanbox_string(string as i64)
}

pub(crate) extern "C" fn object_prototype_has_own_property_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::super::object_ops::js_object_has_own(this_value, key)
}

pub(crate) extern "C" fn object_prototype_property_is_enumerable_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::super::js_object_property_is_enumerable(this_value, key)
}

// Annex B §B.2.2 Object.prototype accessor methods — real thunks so reflective
// access (`Object.prototype.__defineGetter__.call(o, k, fn)`, `typeof`) works,
// not just the direct `o.__defineGetter__(...)` native-dispatch path.
pub(crate) extern "C" fn object_prototype_define_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
    getter: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::super::js_object_define_getter(this_value, key, getter)
}

pub(crate) extern "C" fn object_prototype_define_setter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
    setter: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::super::js_object_define_setter(this_value, key, setter)
}

pub(crate) extern "C" fn object_prototype_lookup_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::super::js_object_lookup_getter(this_value, key)
}

pub(crate) extern "C" fn object_prototype_lookup_setter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::super::js_object_lookup_setter(this_value, key)
}

pub(crate) extern "C" fn error_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let this_jsv = crate::value::JSValue::from_bits(this_value.to_bits());
    // ECMA-262 20.5.3.4 step 2: `If Type(O) is not Object, throw a TypeError`.
    // A Symbol is a POINTER_TAG value (registered-symbol id), so it slips past
    // the `is_pointer` gate — reject it explicitly (test262 Error/prototype/
    // toString/invalid-receiver, which drives undefined/null/1/true/string/
    // Symbol through `.call`).
    if !this_jsv.is_pointer()
        || this_jsv.is_null()
        || this_jsv.is_undefined()
        || unsafe { crate::symbol::js_is_symbol(this_value) } != 0
    {
        super::super::object_ops::throw_object_type_error(
            b"Error.prototype.toString called on non-object",
        );
    }
    let raw = crate::value::js_nanbox_get_pointer(this_value) as *const u8;
    if raw.is_null() || !crate::object::is_valid_obj_ptr(raw) {
        super::super::object_ops::throw_object_type_error(
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
    // ECMA-262 20.5.3.4 step 6/10: `msg`/`name` are coerced with ToString,
    // which throws a TypeError for a Symbol (test262 Error/prototype/toString/
    // tostring-message-throws-symbol). `js_jsvalue_to_string` otherwise renders
    // `Symbol(desc)` and swallows the throw.
    crate::builtins::reject_symbol_to_string(value);
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

pub(crate) extern "C" fn object_prototype_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    unsafe { super::super::js_object_default_value_of(this_value) }
}

pub(crate) extern "C" fn object_prototype_to_locale_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    unsafe { super::super::js_object_default_to_locale_string(this_value) }
}

/// Spec `CreateListFromArrayLike`'s implementation-defined cap on the
/// generic index-walk (Node/V8 throw a `RangeError` — "Maximum call stack
/// size exceeded" or "Invalid array length" depending on magnitude — well
/// before honoring a huge `length`). Chosen generously above any legitimate
/// argument list while still bounding the loop/allocation below.
const MAX_GENERIC_ARRAY_LIKE_LENGTH: i64 = 1_000_000;

fn throw_apply_range_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

unsafe fn function_apply_args(args_array: f64) -> Vec<f64> {
    let value = JSValue::from_bits(args_array.to_bits());
    if value.is_undefined() || value.is_null() {
        return Vec::new();
    }
    // An arguments OBJECT is array-like but fails the IsArray check below —
    // unpack it via its registry (`fn.apply(this, arguments)`).
    if value.is_pointer() {
        let raw = (value.bits() & crate::value::POINTER_MASK) as usize;
        if let Some(values) =
            super::super::arguments_object_to_vec(raw as *const super::super::ObjectHeader)
        {
            return values;
        }
    }
    let is_array = JSValue::from_bits(crate::array::js_array_is_array(args_array).to_bits());
    if is_array.is_bool() && is_array.as_bool() {
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
        return out;
    }
    // Spec `CreateListFromArrayLike` (7.3.24) step 2: a non-nullish,
    // non-object `argArray` is a `TypeError` — a Symbol/Number/String/
    // Boolean/BigInt was passed directly to `.apply` (test262
    // apply/argarray-not-object). Nullish was already handled above; a real
    // Array and an arguments object were handled above too, so anything left
    // that isn't an Object must be a primitive. A `Symbol` is POINTER_TAG'd
    // like a real heap object, so `!value.is_pointer()` alone doesn't catch
    // it — check `js_is_symbol` explicitly.
    if !value.is_pointer() || crate::symbol::js_is_symbol(args_array) != 0 {
        throw_type_error_message(b"CreateListFromArrayLike called on non-object");
    }
    generic_array_like_to_vec(args_array)
}

fn throw_type_error_message(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Generic `CreateListFromArrayLike` (spec 7.3.24) for an `argArray` that is
/// neither an arguments object nor a real Array — notably a Proxy or plain
/// object with a numeric `length` and indexed properties. Reads `length` via
/// `Get` (ToNumber-coerced, clamped to `ToLength`), then reads each index
/// `0..length` via `Get` in order. `js_get_property` is the generic
/// dynamic-property-get entry point (Proxy-trap aware, throws through on
/// failure) also used by codegen for computed member access — so a throwing
/// `length`/indexed `Get` trap propagates as the abrupt completion the spec
/// requires (test262 built-ins/Function/prototype/apply/get-index-abrupt),
/// instead of being silently swallowed into an empty list.
///
/// `args_array` and every collected element are rooted in a
/// `RuntimeHandleScope` for the duration of the walk: each `js_get_property`
/// call can run arbitrary JS (a getter, a Proxy trap) that may trigger a GC
/// cycle, and a plain `Vec<f64>` accumulator lives on the Rust heap — outside
/// the conservative stack scanner's reach — so an already-collected heap
/// pointer would go stale under a moving/evacuating cycle without explicit
/// rooting.
pub(crate) unsafe fn generic_array_like_to_vec(args_array: f64) -> Vec<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let array_handle = scope.root_nanbox_f64(args_array);
    let len_key = b"length";
    let len_val = crate::value::js_get_property(
        array_handle.get_nanbox_f64(),
        len_key.as_ptr() as i64,
        len_key.len() as i64,
    );
    // Spec `ToLength` runs `ToIntegerOrInfinity`, which itself runs
    // `ToNumber` — and `ToNumber(BigInt)` is a `TypeError`, not an implicit
    // narrowing conversion (test262 length-property-is-bigint-value).
    // `js_number_coerce` doesn't special-case BigInt, so reject it here first.
    if JSValue::from_bits(len_val.to_bits()).is_bigint() {
        throw_type_error_message(b"Cannot convert a BigInt value to a number");
    }
    let len_num = crate::builtins::js_number_coerce(len_val);
    let len = if len_num.is_nan() {
        0
    } else {
        let n = len_num.trunc();
        if n <= 0.0 {
            0
        } else if n > 9_007_199_254_740_991.0 {
            9_007_199_254_740_991_i64
        } else {
            n as i64
        }
    };
    if len > MAX_GENERIC_ARRAY_LIKE_LENGTH {
        throw_apply_range_error(b"Maximum call stack size exceeded");
    }
    let mut handles = Vec::with_capacity(len as usize);
    for i in 0..len {
        let key = i.to_string();
        let v = crate::value::js_get_property(
            array_handle.get_nanbox_f64(),
            key.as_ptr() as i64,
            key.len() as i64,
        );
        handles.push(scope.root_nanbox_f64(v));
    }
    crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&handles)
}

pub(crate) extern "C" fn function_prototype_apply_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    args_array: f64,
) -> f64 {
    unsafe {
        let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
        let args = function_apply_args(args_array);
        let this_arg = crate::closure::coerce_call_this(target, this_arg);
        // Rebind a concise/object-literal method's baked `this` slot to the
        // explicit `.apply(thisArg)` receiver (no-op for arrows / plain fns).
        let target = crate::closure::rebind_explicit_this(target, this_arg);
        let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
        let result = crate::closure::js_native_call_value(target, args.as_ptr(), args.len());
        IMPLICIT_THIS.with(|c| c.set(prev_this));
        result
    }
}

/// #4101: `Function.prototype.toString` as a real callable thunk. Reads the
/// receiver from `IMPLICIT_THIS` (set by `.call`/`.apply`'s runtime arm), then:
///   • throws a `TypeError` when `this` is not callable (the spec brand check
///     deferred from #4098 — `Function.prototype.toString.call({})`), and
///   • otherwise returns the function's reconstructed source text.
/// A dedicated thunk (rather than the shared no-op) so the brand check is
/// scoped to `Function.prototype.toString` and never fires for the lenient
/// `Object.prototype.toString` (which keeps its own real thunk).
pub(crate) extern "C" fn function_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        0
    };
    if raw == 0 || !crate::closure::is_closure_ptr(raw) {
        // A Proxy whose target is callable is itself callable; its source is
        // never introspectable, so the spec mandates the NativeFunction form.
        let this_val = f64::from_bits(this_bits);
        if crate::proxy::js_proxy_is_proxy(this_val) == 1
            && crate::proxy::proxy_wraps_callable(this_val)
        {
            let s = "function () { [native code] }";
            let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
        }
        // A class reference (INT32-tagged registered class id) is a function
        // value; Perry retains no class source, so emit the NativeFunction
        // form with the class name.
        if super::super::class_prototype_ref_id(this_val).is_none() {
            if let Some(cid) = super::super::native_module::class_ref_id(this_val) {
                let name = super::super::class_registry::class_name_for_id(cid).unwrap_or_default();
                let s = format!("function {name}() {{ [native code] }}");
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            }
        }
        super::super::object_ops::throw_object_type_error(
            b"Function.prototype.toString requires that 'this' be a Function",
        );
    }
    let func_ptr = unsafe { (*(raw as *const crate::closure::ClosureHeader)).func_ptr as usize };
    let s = crate::builtins::function_source_for_func_ptr(func_ptr);
    let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(str_ptr).bits())
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
pub(crate) extern "C" fn array_prototype_slice_thunk(
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
    let result = unsafe {
        if let Some(arr) =
            crate::object::arguments_object_to_array(arr_ptr as *const crate::object::ObjectHeader)
        {
            crate::array::js_array_slice_values(arr, start_val, end_val)
        } else {
            crate::array::js_array_slice_values(arr_ptr, start_val, end_val)
        }
    };
    f64::from_bits(crate::value::js_nanbox_pointer(result as i64).to_bits())
}

/// Real callable thunks for the generic `Array.prototype` mutators
/// (`pop`/`shift`/`reverse` — no positional args; `push`/`unshift`/`splice` —
/// variadic). Each reads the call-site receiver from `IMPLICIT_THIS` (set by
/// the own-field dispatch and `Function.prototype.call`/`.apply`) and forwards
/// to the shared engine, which mutates a real array via the dense helpers or a
/// plain array-like object via live `Get`/`Set`/`Delete`. Without these, the
/// methods were noop-backed (`global_this_builtin_noop_thunk`), so a borrowed
/// reference (`obj.pop = Array.prototype.pop; obj.pop()` or
/// `Array.prototype.pop.call(obj)`) returned `undefined` / looped.
pub(crate) extern "C" fn array_prototype_pop_thunk(
    _c: *const crate::closure::ClosureHeader,
    _a: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    crate::array::array_proto_mutator(this, "pop", std::ptr::null(), 0)
}
pub(crate) extern "C" fn array_prototype_shift_thunk(
    _c: *const crate::closure::ClosureHeader,
    _a: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    crate::array::array_proto_mutator(this, "shift", std::ptr::null(), 0)
}
pub(crate) extern "C" fn array_prototype_reverse_thunk(
    _c: *const crate::closure::ClosureHeader,
    _a: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    crate::array::array_proto_mutator(this, "reverse", std::ptr::null(), 0)
}
pub(crate) extern "C" fn array_prototype_push_thunk(
    _c: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    let args = global_this_rest_array_values(rest);
    crate::array::array_proto_mutator(this, "push", args.as_ptr(), args.len())
}
pub(crate) extern "C" fn array_prototype_unshift_thunk(
    _c: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    let args = global_this_rest_array_values(rest);
    crate::array::array_proto_mutator(this, "unshift", args.as_ptr(), args.len())
}
pub(crate) extern "C" fn array_prototype_splice_thunk(
    _c: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    let args = global_this_rest_array_values(rest);
    crate::array::array_proto_mutator(this, "splice", args.as_ptr(), args.len())
}
pub(crate) extern "C" fn array_prototype_sort_thunk(
    _c: *const crate::closure::ClosureHeader,
    comparator: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    crate::array::js_arraylike_sort(this, comparator)
}

/// Real thunks for the generic `Array.prototype` iteration / search methods,
/// each routing the call-site receiver (IMPLICIT_THIS) through the
/// `js_arraylike_*` engine. These replace the previous noop thunks so a
/// reflective resolution — `Array.prototype.map.call(x, …)` through a stored
/// reference, or a method reached through an object whose [[Prototype]] chain
/// contains a real array (`foo.prototype = new Array(…)`; test262
/// filter/15.4.4.20-6-*, some/15.4.4.17-8-*) — runs the real algorithm
/// instead of returning garbage. Rest-arg shape (like `push`/`splice` above)
/// keeps the closure call convention independent of the spec `.length`.
macro_rules! array_proto_arraylike_cb_thunk {
    ($name:ident, $engine:path) => {
        pub(crate) extern "C" fn $name(_c: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
            let this = crate::object::js_implicit_this_get();
            let args = global_this_rest_array_values(rest);
            let a = |i: usize| {
                args.get(i)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
            };
            $engine(this, a(0), a(1))
        }
    };
}
array_proto_arraylike_cb_thunk!(
    array_proto_forEach_thunk,
    crate::array::js_arraylike_forEach
);
array_proto_arraylike_cb_thunk!(array_proto_map_thunk, crate::array::js_arraylike_map);
array_proto_arraylike_cb_thunk!(array_proto_filter_thunk, crate::array::js_arraylike_filter);
array_proto_arraylike_cb_thunk!(array_proto_some_thunk, crate::array::js_arraylike_some);
array_proto_arraylike_cb_thunk!(array_proto_every_thunk, crate::array::js_arraylike_every);
array_proto_arraylike_cb_thunk!(array_proto_find_thunk, crate::array::js_arraylike_find);
array_proto_arraylike_cb_thunk!(
    array_proto_findIndex_thunk,
    crate::array::js_arraylike_findIndex
);
array_proto_arraylike_cb_thunk!(
    array_proto_findLast_thunk,
    crate::array::js_arraylike_findLast
);
array_proto_arraylike_cb_thunk!(
    array_proto_findLastIndex_thunk,
    crate::array::js_arraylike_findLastIndex
);

macro_rules! array_proto_arraylike_optarg_thunk {
    ($name:ident, $engine:path) => {
        pub(crate) extern "C" fn $name(_c: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
            let this = crate::object::js_implicit_this_get();
            let args = global_this_rest_array_values(rest);
            let a = |i: usize| {
                args.get(i)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
            };
            $engine(this, a(0), (args.len() > 1) as i32, a(1))
        }
    };
}
array_proto_arraylike_optarg_thunk!(array_proto_reduce_thunk, reduce_engine);
array_proto_arraylike_optarg_thunk!(array_proto_reduceRight_thunk, reduce_right_engine);

// `js_arraylike_reduce*` take (recv, cb, has_init, init) — adapt arg order.
fn reduce_engine(recv: f64, cb: f64, has_init: i32, init: f64) -> f64 {
    crate::array::js_arraylike_reduce(recv, cb, has_init, init)
}
fn reduce_right_engine(recv: f64, cb: f64, has_init: i32, init: f64) -> f64 {
    crate::array::js_arraylike_reduceRight(recv, cb, has_init, init)
}

macro_rules! array_proto_arraylike_search_thunk {
    ($name:ident, $engine:path) => {
        pub(crate) extern "C" fn $name(_c: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
            let this = crate::object::js_implicit_this_get();
            let args = global_this_rest_array_values(rest);
            let a = |i: usize| {
                args.get(i)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
            };
            $engine(this, a(0), a(1), (args.len() > 1) as i32)
        }
    };
}
array_proto_arraylike_search_thunk!(
    array_proto_indexOf_thunk,
    crate::array::js_arraylike_indexOf
);
array_proto_arraylike_search_thunk!(
    array_proto_lastIndexOf_thunk,
    crate::array::js_arraylike_lastIndexOf
);
array_proto_arraylike_search_thunk!(
    array_proto_includes_thunk,
    crate::array::js_arraylike_includes
);

pub(crate) extern "C" fn array_proto_at_thunk(
    _c: *const crate::closure::ClosureHeader,
    idx: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    crate::array::js_arraylike_at(this, idx)
}
pub(crate) extern "C" fn array_proto_join_thunk(
    _c: *const crate::closure::ClosureHeader,
    sep: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    crate::array::js_arraylike_join(this, sep)
}
pub(crate) extern "C" fn array_prototype_concat_thunk(
    _c: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    let args = global_this_rest_array_values(rest);
    crate::array::js_arraylike_concat(this, args.as_ptr(), args.len() as i32)
}
