//! Shared iterable-consumption + validation for the `Map`/`Set`/`WeakMap`/
//! `WeakSet` constructors (issues #2770/#2771/#2772).
//!
//! JS collection constructors run `AddEntriesFromIterable` / the Set
//! constructor's iterable loop. Both require the init argument to be a real
//! iterable: `null`/`undefined` mean "empty", and any other value that is
//! not iterable throws
//! `TypeError: <type> [<value> ]is not iterable (cannot read property
//! Symbol(Symbol.iterator))` — matching Node exactly.
//!
//! This module centralizes (a) the iterability classification + Node error
//! message, and (b) materializing the yielded values into a plain Array via
//! the existing [`crate::array::js_for_of_to_array`] machinery (which already
//! handles Array / String / Map / Set / custom `[Symbol.iterator]` objects).
//! The collection-specific runtime helpers (`js_map_from_iterable`,
//! `js_set_from_iterable`, `js_weakmap_init_iterable`, `js_weakset_init_iterable`)
//! call into here so the throw-vs-empty-vs-consume decision lives in one place.

use crate::array::ArrayHeader;
use crate::value::{js_jsvalue_to_string, js_nanbox_get_pointer, JSValue, TAG_NULL, TAG_UNDEFINED};
use std::os::raw::c_int;

/// Outcome of classifying a constructor init argument.
pub(crate) enum InitIter {
    /// `null` / `undefined` — treat as empty init, no entries.
    Empty,
    /// An iterable; the yielded values have been materialized into this
    /// (NaN-box-stripped) Array pointer.
    Values(*mut ArrayHeader),
}

/// `typeof`-style word for a non-iterable value, used to build the Node
/// "<type> is not iterable" message. `null`/`undefined` are handled by the
/// caller (they never throw), so they are not produced here.
fn typeof_word(value: f64) -> &'static str {
    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_number() || jsv.is_int32() {
        "number"
    } else if jsv.is_bool() {
        "boolean"
    } else if jsv.is_bigint() {
        "bigint"
    } else if jsv.is_any_string() {
        // Strings are iterable; never reaches the throw path. Kept for
        // completeness.
        "string"
    } else {
        let raw = js_nanbox_get_pointer(value);
        if raw != 0 && crate::symbol::is_registered_symbol(raw as usize) {
            "symbol"
        } else if raw != 0 && crate::closure::is_closure_ptr(raw as usize) {
            "function"
        } else {
            "object"
        }
    }
}

/// Build the Node TypeError message prefix for a non-iterable value.
///
/// Node prints the *value* only for `number` and `boolean` (`number 5`,
/// `boolean true`); for `bigint`/`symbol`/`function`/`object` it prints just
/// the type word.
fn not_iterable_message(value: f64) -> String {
    let word = typeof_word(value);
    let with_value = match word {
        "number" | "boolean" => {
            let s_ptr = js_jsvalue_to_string(value);
            if s_ptr.is_null() {
                None
            } else {
                let s = unsafe {
                    let byte_len = (*s_ptr).byte_len as usize;
                    let data = (s_ptr as *const u8)
                        .add(std::mem::size_of::<crate::string::StringHeader>());
                    String::from_utf8_lossy(std::slice::from_raw_parts(data, byte_len)).into_owned()
                };
                Some(format!("{} {}", word, s))
            }
        }
        _ => None,
    };
    let head = with_value.unwrap_or_else(|| word.to_string());
    format!(
        "{} is not iterable (cannot read property Symbol(Symbol.iterator))",
        head
    )
}

/// Throw the Node `TypeError: <type> is not iterable (...)` for a
/// non-iterable constructor init value. Never returns.
pub(crate) fn throw_not_iterable(value: f64) -> ! {
    let msg = not_iterable_message(value);
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// True if `value` is a JS iterable (array, string, Map, Set, or any heap
/// object carrying a callable `[Symbol.iterator]`). `null`/`undefined` are
/// NOT iterable here (the caller handles them as empty before calling).
pub(crate) fn is_iterable(value: f64) -> bool {
    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_any_string() {
        return true;
    }
    if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
        return true;
    }
    let raw = js_nanbox_get_pointer(value);
    if raw == 0 {
        return false;
    }
    let addr = raw as usize;
    if crate::map::is_registered_map(addr) || crate::set::is_registered_set(addr) {
        return true;
    }
    // A built-in iterator object — an array/map/set/string/buffer/iterator-
    // helper iterator — IS already an iterator and is therefore iterable
    // (`[Symbol.iterator]()` returns itself). Mirrors the equivalent
    // `is_builtin_iterator_class_id` arm in `js_get_iterator`.
    if crate::array::is_builtin_iterator_class_id(addr) {
        return true;
    }
    // A generator object (`g()` from `function* g(){}`) is lowered by Perry to a
    // plain object with own closure-valued `next`/`return`/`throw` methods and
    // NO `[Symbol.iterator]` symbol property, so the generic check below misses
    // it. Generators ARE iterable (`[Symbol.iterator]()` returns themselves), so
    // without this `js_array_like_to_array` — call / `new` / super spread of a
    // generator (`f(...g())`, `new C(...g())`) and `new Map/Set(g())` — fell
    // through to the array-reinterpret garbage path. `js_get_iterator` returns
    // the generator unchanged (its own `.next()` drives the state machine).
    if crate::object::js_util_types_is_generator_object(value).to_bits() == crate::value::TAG_TRUE {
        return true;
    }
    // Generic object: iterable iff it exposes a callable `[Symbol.iterator]`.
    if !crate::object::is_valid_obj_ptr(raw as *const u8) {
        return false;
    }
    let iter_wk = crate::symbol::well_known_symbol("iterator");
    if iter_wk.is_null() {
        return false;
    }
    let sym_f64 = f64::from_bits(JSValue::pointer(iter_wk as *const u8).bits());
    let iter_fn = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_f64) };
    if iter_fn.to_bits() == TAG_UNDEFINED {
        return false;
    }
    let fn_raw = js_nanbox_get_pointer(iter_fn);
    fn_raw != 0 && crate::closure::is_closure_ptr(fn_raw as usize)
}

pub(crate) fn is_null_or_undefined(value: f64) -> bool {
    matches!(value.to_bits(), TAG_UNDEFINED | TAG_NULL)
}

pub(crate) fn throw_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

pub(crate) fn is_callable(value: f64) -> bool {
    let raw = js_nanbox_get_pointer(value) & !0x7;
    if raw >= 0x10000 {
        return true;
    }
    crate::proxy::js_proxy_is_proxy(value) == 1
}

pub(crate) fn require_callable(value: f64, name: &str) -> f64 {
    if is_callable(value) {
        return value;
    }
    throw_type_error(&format!("{name} is not a function"));
}

pub(crate) fn normalize_callable_value(value: f64) -> f64 {
    let raw = js_nanbox_get_pointer(value) & !0x7;
    if raw >= 0x10000 {
        return crate::value::js_nanbox_pointer(raw);
    }
    value
}

/// `Get(receiver, method_name)` resolved off `builtin_name`'s `.prototype`,
/// honoring an accessor descriptor installed there
/// (`Object.defineProperty(WeakMap.prototype, 'set', {get(){throw ...}})`) so
/// a throwing getter propagates per the constructor spec's
/// `ReturnIfAbrupt(adder)` step. Falls back to the plain installed method
/// value — [`builtin_prototype_method`] — for the common case (a normal data
/// property, whether the built-in thunk or a user override).
pub(crate) fn builtin_prototype_adder(builtin_name: &str, method_name: &str, receiver: f64) -> f64 {
    let proto = crate::object::builtin_prototype_value(builtin_name);
    let proto_addr = js_nanbox_get_pointer(proto) as usize;
    if proto_addr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if let Some(acc) = crate::object::get_accessor_descriptor(proto_addr, method_name) {
        if acc.get == 0 {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return f64::from_bits(
            unsafe { crate::object::invoke_accessor_getter(acc.get, receiver) }.bits(),
        );
    }
    builtin_prototype_method(builtin_name, method_name)
}

pub(crate) fn builtin_prototype_method(builtin_name: &str, method_name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let proto = crate::object::builtin_prototype_value(builtin_name);
    let proto_ptr = js_nanbox_get_pointer(proto) as *const crate::object::ObjectHeader;
    if proto_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        crate::object::own_data_field_by_name(proto_ptr, key)
            .map(|value| f64::from_bits(value.bits()))
            .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_builtin_prototype_method_value(
    builtin_ptr: *const u8,
    builtin_len: usize,
    method_ptr: *const u8,
    method_len: usize,
) -> f64 {
    let builtin = match std::str::from_utf8(std::slice::from_raw_parts(builtin_ptr, builtin_len)) {
        Ok(value) => value,
        Err(_) => return f64::from_bits(TAG_UNDEFINED),
    };
    let method = match std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)) {
        Ok(value) => value,
        Err(_) => return f64::from_bits(TAG_UNDEFINED),
    };
    // Prefer the actual method object installed on the built-in prototype so a
    // folded `Number.prototype.toString` value-read resolves to the SAME
    // closure that a boxed wrapper reaches via its [[Prototype]] chain
    // (`(new Number()).toString === Number.prototype.toString`, test262
    // S15.7.5_A1_T0*). The primitive-wrapper prototypes install their real
    // thunks (`primitive_proto_thunks::install_primitive_proto_methods`), so
    // the proto field is authoritative; only synthesize a fresh closure when
    // the prototype carries no own method under that name.
    let installed = builtin_prototype_method(builtin, method);
    if installed.to_bits() != TAG_UNDEFINED {
        return installed;
    }
    crate::object::primitive_proto_method_value(builtin, method)
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
}

pub(crate) enum ConstructorIter {
    Empty,
    Array(f64),
    Iterator(f64),
}

pub(crate) fn constructor_iter(value: f64) -> ConstructorIter {
    if is_null_or_undefined(value) {
        return ConstructorIter::Empty;
    }
    let jsv = JSValue::from_bits(value.to_bits());
    let is_array = crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE;
    if is_array {
        return ConstructorIter::Array(value);
    }
    if jsv.is_any_string() {
        let arr_f64 = crate::array::js_for_of_to_array(value);
        return ConstructorIter::Array(arr_f64);
    }
    if !is_iterable(value) {
        throw_not_iterable(value);
    }
    let iter = crate::symbol::js_get_iterator(value);
    ConstructorIter::Iterator(iter)
}

pub(crate) fn call_capturing_throw(call: impl FnOnce() -> f64) -> Result<f64, f64> {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    let result = if jumped == 0 {
        Ok(call())
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(exc)
    };
    crate::exception::js_try_end();
    result
}

pub(crate) fn call_with_this_capturing_throw(
    callee: f64,
    this_value: f64,
    args: &[f64],
) -> Result<f64, f64> {
    let prev_this = crate::object::js_implicit_this_set(this_value);
    let result = call_capturing_throw(|| unsafe {
        crate::closure::js_native_call_value(callee, args.as_ptr(), args.len())
    });
    crate::object::js_implicit_this_set(prev_this);
    result
}

pub(crate) fn iterator_close(iter: f64) {
    let _ = call_capturing_throw(|| unsafe {
        crate::object::js_native_call_method(
            iter,
            b"return".as_ptr() as *const i8,
            "return".len(),
            std::ptr::null(),
            0,
        )
    });
}

pub(crate) fn iterator_next_value(iter: f64) -> Option<f64> {
    let result = unsafe {
        crate::object::js_native_call_method(
            iter,
            b"next".as_ptr() as *const i8,
            "next".len(),
            std::ptr::null(),
            0,
        )
    };
    if !is_entry_object(result) {
        crate::array::iter_bt_dump("collection_iter_next_value", result);
        throw_type_error("Iterator result is not an object");
    }
    let result_ptr = js_nanbox_get_pointer(result) as *const crate::object::ObjectHeader;
    let done_key = crate::string::js_string_from_bytes(b"done".as_ptr(), 4);
    let done = crate::object::js_object_get_field_by_name_f64(result_ptr, done_key);
    if crate::value::js_is_truthy(done) != 0 {
        return None;
    }
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let value = crate::object::js_object_get_field_by_name_f64(result_ptr, value_key);
    Some(value)
}

/// True if a yielded value is a JS *object* (array, plain object, function,
/// Map, Set, …) — i.e. anything `Type(v) === Object`. Primitives (number,
/// boolean, string, bigint, symbol, null, undefined) are NOT entry objects,
/// matching Node's `new Map([1])` / `new Map(['ab'])` throws.
pub(crate) fn is_entry_object(value: f64) -> bool {
    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_number()
        || jsv.is_int32()
        || jsv.is_bool()
        || jsv.is_bigint()
        || jsv.is_any_string()
        || jsv.is_undefined()
        || jsv.is_null()
    {
        return false;
    }
    let raw = js_nanbox_get_pointer(value);
    if raw == 0 {
        return false;
    }
    // Symbols are primitives despite being pointer-tagged.
    !crate::symbol::is_registered_symbol(raw as usize)
}

/// Throw `TypeError: Iterator value <v> is not an entry object` (used by the
/// Map / WeakMap constructors when a yielded value is not an object). Never
/// returns.
pub(crate) fn throw_not_entry_object(value: f64) -> ! {
    let v_str = value_display(value);
    let msg = format!("Iterator value {} is not an entry object", v_str);
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// Node's display string for a value embedded in a TypeError message
/// (`Symbol(s)`, `5`, `ab`, `null`, `undefined`, `true`). Uses the standard
/// `js_jsvalue_to_string` for everything except symbols, which it renders as
/// `Symbol(<desc>)`.
fn value_display(value: f64) -> String {
    let raw = js_nanbox_get_pointer(value);
    let s_ptr = if raw != 0 && crate::symbol::is_registered_symbol(raw as usize) {
        // `js_symbol_to_string` already renders `Symbol(<desc>)`.
        unsafe { crate::symbol::js_symbol_to_string(value) as *mut crate::string::StringHeader }
    } else {
        js_jsvalue_to_string(value)
    };
    if s_ptr.is_null() {
        return String::new();
    }
    unsafe {
        let byte_len = (*s_ptr).byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, byte_len)).into_owned()
    }
}

/// Classify a collection-constructor init argument:
/// - `null`/`undefined` → [`InitIter::Empty`],
/// - any iterable → materialize the yielded values into an Array
///   ([`InitIter::Values`]),
/// - anything else → throw the Node "not iterable" `TypeError`.
///
/// The returned Array pointer is NaN-box-stripped (a raw `*mut ArrayHeader`).
pub(crate) fn classify_init(value: f64) -> InitIter {
    let bits = value.to_bits();
    if bits == TAG_UNDEFINED || bits == TAG_NULL {
        return InitIter::Empty;
    }
    if !is_iterable(value) {
        throw_not_iterable(value);
    }
    // `js_for_of_to_array` returns a NaN-boxed (POINTER_TAG) Array f64 whose
    // elements are exactly what `for...of value` would yield.
    let arr_f64 = crate::array::js_for_of_to_array(value);
    let arr_ptr = js_nanbox_get_pointer(arr_f64) as *mut ArrayHeader;
    InitIter::Values(arr_ptr)
}
