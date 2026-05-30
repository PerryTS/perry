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
fn is_iterable(value: f64) -> bool {
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
