//! `Array.from` API-level validation + mapped-call semantics (#2773) and the
//! spec-complete variadic, non-mutating `Array.prototype.concat` (#2805).
//!
//! These are the JS-API entry points. The low-level materialization helpers
//! (`js_array_clone`, `js_array_from_arraylike`, ...) stay in their own
//! modules; this module layers the spec validation (`TypeError` for nullish
//! sources, callability checks, `Symbol.isConcatSpreadable`) on top of them.
use super::{
    clean_arr_ptr, js_array_alloc, js_array_clone, js_array_is_array, js_array_push_f64,
    js_array_set_f64_extend, ArrayHeader,
};
use crate::closure::ClosureHeader;
use crate::value::JSValue;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

/// Throw `TypeError: <receiver> is not iterable` (matches Node's wording for
/// nullish `Array.from` / `Uint8Array.from` sources).
#[cold]
fn throw_not_iterable(receiver: &str) -> ! {
    let msg = format!(
        "{} is not iterable (cannot read property Symbol(Symbol.iterator))",
        receiver
    );
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value));
}

/// Throw `TypeError: <value> is not a function` for a non-callable mapFn,
/// matching Node's `Array.from([1], 1)` → "number 1 is not a function".
#[cold]
fn throw_map_fn_not_callable(map_fn: f64) -> ! {
    let value_str = {
        let sp = crate::value::js_jsvalue_to_string(map_fn);
        if sp.is_null() {
            String::new()
        } else {
            unsafe {
                let header = &*(sp as *const crate::string::StringHeader);
                let bytes_ptr =
                    (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
                std::str::from_utf8(slice).unwrap_or("").to_string()
            }
        }
    };
    let jv = JSValue::from_bits(map_fn.to_bits());
    let type_name = if jv.is_null() || jv.is_pointer() {
        "object"
    } else if map_fn.to_bits() >> 48 == 0x7FFF {
        "string"
    } else {
        "number"
    };
    let msg = format!("{} {} is not a function", type_name, value_str);
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value));
}

/// `Array.from(source)` — the non-mapped form. Per ECMA-262 §23.1.2.1,
/// `null`/`undefined` sources throw `TypeError` (they have no
/// `Symbol.iterator`), while numbers/booleans/symbols are non-iterable
/// non-objects that materialize to an empty array. All valid object/iterable
/// sources delegate to the existing `js_array_clone` materialization (which
/// covers arrays, sets, maps, strings, typed arrays, buffers, iterators, and
/// array-likes).
///
/// Takes the raw NaN-boxed f64 value (NOT a pre-unboxed pointer) so it can
/// inspect the tag bits before stripping.
#[no_mangle]
pub extern "C" fn js_array_from_value(boxed: f64) -> *mut ArrayHeader {
    let bits = boxed.to_bits();
    if bits == TAG_UNDEFINED {
        throw_not_iterable("undefined");
    }
    if bits == TAG_NULL {
        throw_not_iterable("object null");
    }
    // Numbers / booleans / strings handled inside js_array_clone:
    //  - numbers/booleans aren't pointers → empty array.
    //  - strings → per-codepoint materialization.
    // Pointers (objects/arrays/iterables) materialize via js_array_clone.
    let ptr_bits = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    unsafe {
        if let Some(arr) =
            crate::object::arguments_object_to_array(ptr_bits as *const crate::object::ObjectHeader)
        {
            return arr;
        }
    }
    js_array_clone(ptr_bits as *const ArrayHeader)
}

#[used]
static KEEP_ARRAY_FROM_VALUE: extern "C" fn(f64) -> *mut ArrayHeader = js_array_from_value;

/// `Array.from(source, mapFn, thisArg)` — the mapped form. Throws for nullish
/// sources (like the non-mapped form), validates that `mapFn` is callable,
/// then materializes the source and calls `mapFn(value, index)` for each
/// element with `thisArg` bound as the function's `this`.
///
/// All three arguments are raw NaN-boxed f64 values. `this_arg` may be
/// `undefined` (no binding).
#[no_mangle]
pub extern "C" fn js_array_from_mapped(
    src_boxed: f64,
    map_fn: f64,
    this_arg: f64,
) -> *mut ArrayHeader {
    // Nullish source → TypeError (before validating mapFn, matching Node).
    let bits = src_boxed.to_bits();
    if bits == TAG_UNDEFINED {
        throw_not_iterable("undefined");
    }
    if bits == TAG_NULL {
        throw_not_iterable("object null");
    }
    let cb = resolve_callable(map_fn);
    // Materialize the source (arrays, strings, iterables, array-likes, ...).
    let ptr_bits = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    let src = js_array_clone(ptr_bits as *const ArrayHeader);
    map_with_this(src, cb, this_arg)
}

#[used]
static KEEP_ARRAY_FROM_MAPPED: extern "C" fn(f64, f64, f64) -> *mut ArrayHeader =
    js_array_from_mapped;

/// Validate a mapFn argument is callable, returning its `ClosureHeader*`.
/// Throws `TypeError` for any non-callable value.
fn resolve_callable(map_fn: f64) -> *const ClosureHeader {
    let jv = JSValue::from_bits(map_fn.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<ClosureHeader>();
        if !ptr.is_null() && crate::closure::is_closure_ptr(ptr as usize) {
            return ptr;
        }
    }
    throw_map_fn_not_callable(map_fn);
}

/// Map `src` into a fresh array by calling `cb(value, index)` for each element
/// with `this_arg` bound as the callback's `this`. Per ECMA-262 the mapFn
/// receives exactly `(value, index)` (no array argument).
fn map_with_this(
    src: *mut ArrayHeader,
    cb: *const ClosureHeader,
    this_arg: f64,
) -> *mut ArrayHeader {
    let src = clean_arr_ptr(src);
    if src.is_null() {
        return js_array_alloc(0);
    }
    let prev_this = crate::object::js_implicit_this_set(this_arg);
    let result = unsafe {
        let length = (*src).length;
        let src_elements = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let result = js_array_alloc(length);
        for i in 0..length as usize {
            let element = *src_elements.add(i);
            // mapFn receives (value, index) only.
            let mapped = crate::closure::js_closure_call2(cb, element, i as f64);
            js_array_set_f64_extend(result, i as u32, mapped);
        }
        (*result).length = length;
        result
    };
    crate::object::js_implicit_this_set(prev_this);
    result
}

/// `Array.prototype.concat(...args)` — spec-complete, non-mutating.
///
/// Returns a NEW array; the receiver is never mutated. Each argument is
/// appended in order, spreading per ECMA-262 §23.1.3.1 / IsConcatSpreadable:
///   - the receiver and array arguments spread by default,
///   - an array with `Symbol.isConcatSpreadable === false` is a single element,
///   - a non-array object with `Symbol.isConcatSpreadable === true` is spread
///     as an array-like (`length` + indexed reads),
///   - every other value (primitives, plain objects) is a single element.
///
/// `args_ptr` points at `count` raw NaN-boxed f64 argument values (alloca buffer
/// built by codegen / passed straight from the dynamic dispatcher).
#[no_mangle]
pub extern "C" fn js_array_concat_variadic(
    recv: *const ArrayHeader,
    args_ptr: *const f64,
    count: i32,
) -> *mut ArrayHeader {
    let result = js_array_alloc(0);
    // The receiver itself is always spread (it's the array on which `.concat`
    // was invoked). Materialize a clone to read its elements safely.
    let result = append_spread_array(result, recv as *const ArrayHeader);
    let mut result = result;
    if !args_ptr.is_null() && count > 0 {
        for i in 0..count as usize {
            let value = unsafe { *args_ptr.add(i) };
            result = append_concat_arg(result, value);
        }
    }
    result
}

#[used]
static KEEP_ARRAY_CONCAT_VARIADIC: extern "C" fn(
    *const ArrayHeader,
    *const f64,
    i32,
) -> *mut ArrayHeader = js_array_concat_variadic;

/// Append a single concat argument to `result`, applying spreadability rules.
fn append_concat_arg(result: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    let bits = value.to_bits();
    let jv = JSValue::from_bits(bits);
    if !jv.is_pointer() {
        // Primitive (number / bool / undefined / null / string-by-tag): one element.
        return js_array_push_f64(result, value);
    }
    let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;

    // Is the spreadable flag explicitly set?
    let spreadable = read_concat_spreadable(value);

    // Arrays (and set/map/typed-array/buffer that concat treats array-like via
    // js_array_concat) spread by default, unless @@isConcatSpreadable === false.
    let is_array = js_array_is_array(value).to_bits() == 0x7FFC_0000_0000_0004;
    if is_array {
        if spreadable == Some(false) {
            return js_array_push_f64(result, value);
        }
        return append_spread_array(result, raw_addr as *const ArrayHeader);
    }
    // Non-array object explicitly marked spreadable → spread as array-like.
    if spreadable == Some(true) {
        let arr = unsafe {
            super::js_array_from_arraylike(raw_addr as *const crate::object::ObjectHeader)
        };
        return append_spread_array(result, arr as *const ArrayHeader);
    }
    // Everything else (plain object, function, etc.) is a single element.
    js_array_push_f64(result, value)
}

/// Read `value[Symbol.isConcatSpreadable]`. Returns `Some(true)`/`Some(false)`
/// when the property is a defined boolean (using JS truthiness), or `None` when
/// the property is absent/undefined (→ default behavior).
fn read_concat_spreadable(value: f64) -> Option<bool> {
    let sym = crate::symbol::well_known_symbol("isConcatSpreadable");
    if sym.is_null() {
        return None;
    }
    let sym_f64 = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let flag = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_f64) };
    let fbits = flag.to_bits();
    if fbits == TAG_UNDEFINED {
        return None;
    }
    Some(crate::value::js_is_truthy(flag) != 0)
}

/// Append every element of the (already-materializable) source array `src`
/// into `result`, returning the (possibly reallocated) result. `src` is
/// materialized via `js_array_clone` so sets/maps/typed-arrays/buffers spread
/// to their element values, matching `[...x]`.
fn append_spread_array(result: *mut ArrayHeader, src: *const ArrayHeader) -> *mut ArrayHeader {
    let materialized = js_array_clone(src);
    let materialized = clean_arr_ptr(materialized);
    if materialized.is_null() {
        return result;
    }
    unsafe {
        let len = (*materialized).length;
        let elems =
            (materialized as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let mut out = result;
        for i in 0..len as usize {
            out = js_array_push_f64(out, *elems.add(i));
        }
        out
    }
}
