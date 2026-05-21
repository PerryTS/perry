//! Arithmetic / comparison / `typeof` JSValue operations.
//!
//! Split out of the original monolithic `builtins.rs` (#topic: split-large-files).
//! Covers the FFI helpers the codegen lowers binary operators to (`js_add`,
//! `js_sub`, `js_mul`, `js_div`, `js_mod`, `js_eq`/`loose_eq`, `js_lt`/`le`/
//! `gt`/`ge`) plus `js_value_typeof` (cached typeof-string returns).

use super::*;

// Arithmetic operations on JSValue (with type coercion)

#[no_mangle]
pub extern "C" fn js_add(a: JSValue, b: JSValue) -> JSValue {
    // For MVP, just handle number + number
    JSValue::number(a.to_number() + b.to_number())
}

#[no_mangle]
pub extern "C" fn js_sub(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() - b.to_number())
}

#[no_mangle]
pub extern "C" fn js_mul(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() * b.to_number())
}

#[no_mangle]
pub extern "C" fn js_div(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() / b.to_number())
}

#[no_mangle]
pub extern "C" fn js_mod(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() % b.to_number())
}

// Comparison operations

#[no_mangle]
pub extern "C" fn js_eq(a: JSValue, b: JSValue) -> JSValue {
    // Delegate to the SSO-aware strict-equality entry in value.rs,
    // which already handles cross-representation string compares
    // (heap STRING_TAG + inline SHORT_STRING_TAG, in any order) plus
    // BigInt-by-value, INT32-vs-f64, and the negative-zero / NaN
    // edge cases. The previous implementation was bit-equality with
    // a number-only special case — `JSON.parse(...).foo === "perry"`
    // returned `false` because the JSON parser emits SSO for ≤ 5-byte
    // strings while `"perry"` literals are interned to heap strings,
    // and the bits diverge across representations even when the text
    // is identical.
    let result =
        crate::value::js_jsvalue_equals(f64::from_bits(a.bits()), f64::from_bits(b.bits()));
    JSValue::bool(result != 0)
}

/// JS abstract equality (==). Implements the coercion rules:
/// - Same type: use strict equality
/// - null == undefined: true
/// - number == string: coerce string to number
/// - boolean == anything: coerce boolean to number, recurse
/// - string == number: coerce string to number
#[no_mangle]
pub extern "C" fn js_loose_eq(a: JSValue, b: JSValue) -> JSValue {
    // Both numbers FIRST: IEEE 754 equality correctly handles NaN!=NaN
    // (NaN has well-defined bits, so the later same-bits fast path
    // would otherwise incorrectly return true for NaN==NaN). Also
    // handles +0 == -0 correctly (different bits, IEEE 754 says equal).
    if a.is_number() && b.is_number() {
        return JSValue::bool(a.as_number() == b.as_number());
    }
    // Same bits → always equal (handles null==null, undefined==undefined,
    // identical pointers, identical SSO encodings, etc.)
    if a.bits() == b.bits() {
        return JSValue::bool(true);
    }
    // null == undefined (and vice versa)
    if (a.is_null() && b.is_undefined()) || (a.is_undefined() && b.is_null()) {
        return JSValue::bool(true);
    }
    // null/undefined != anything else
    if a.is_null() || a.is_undefined() || b.is_null() || b.is_undefined() {
        return JSValue::bool(false);
    }
    // Both strings (heap STRING_TAG and/or inline SHORT_STRING_TAG):
    // content compare. The previous `is_string() && is_string()` test
    // missed any SSO operand — `JSON.parse(...).foo == "perry"` returned
    // false because the JSON parser emits SSO for ≤5-byte strings while
    // string literals are interned to heap strings, and the bit patterns
    // diverged across representations even with identical text.
    if a.is_any_string() && b.is_any_string() {
        let result =
            crate::value::js_jsvalue_equals(f64::from_bits(a.bits()), f64::from_bits(b.bits()));
        return JSValue::bool(result != 0);
    }
    // Boolean on either side: coerce to number and recurse
    if a.is_bool() {
        let a_num = if a.as_bool() { 1.0 } else { 0.0 };
        return js_loose_eq(JSValue::number(a_num), b);
    }
    if b.is_bool() {
        let b_num = if b.as_bool() { 1.0 } else { 0.0 };
        return js_loose_eq(a, JSValue::number(b_num));
    }
    // String vs number: coerce string to number. `is_any_string` so
    // SSO operands get the same coercion as heap strings.
    if a.is_number() && b.is_any_string() {
        let b_num = js_number_coerce(f64::from_bits(b.bits()));
        return JSValue::bool(a.as_number() == b_num);
    }
    if a.is_any_string() && b.is_number() {
        let a_num = js_number_coerce(f64::from_bits(a.bits()));
        return JSValue::bool(a_num == b.as_number());
    }
    // Fallback: not equal
    JSValue::bool(false)
}

#[no_mangle]
pub extern "C" fn js_lt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() < b.to_number())
}

#[no_mangle]
pub extern "C" fn js_le(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() <= b.to_number())
}

#[no_mangle]
pub extern "C" fn js_gt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() > b.to_number())
}

#[no_mangle]
pub extern "C" fn js_ge(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() >= b.to_number())
}

/// Return the typeof a value as a string
/// Takes an f64 that uses NaN-boxing to distinguish types.
/// Returns a pointer to a string: "undefined", "boolean", "number", "string", "object", "function"
///
/// Optimization: typeof only returns 7 possible strings, so we cache them as
/// pre-allocated StringHeader pointers to avoid heap allocation on every call.
#[no_mangle]
pub extern "C" fn js_value_typeof(value: f64) -> *mut StringHeader {
    use std::cell::Cell;

    thread_local! {
        static TYPEOF_UNDEFINED: Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_OBJECT:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_BOOLEAN:   Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_NUMBER:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_STRING:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_FUNCTION:  Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_BIGINT:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_SYMBOL:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
    }

    /// Get or initialize a cached typeof string.
    fn get_cached(
        cache: &'static std::thread::LocalKey<Cell<*mut StringHeader>>,
        s: &str,
    ) -> *mut StringHeader {
        cache.with(|cell| {
            let ptr = cell.get();
            if !ptr.is_null() {
                return ptr;
            }
            let new_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            cell.set(new_ptr);
            new_ptr
        })
    }

    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        get_cached(&TYPEOF_UNDEFINED, "undefined")
    } else if jsval.is_null() {
        // typeof null === "object" in JavaScript
        get_cached(&TYPEOF_OBJECT, "object")
    } else if jsval.is_bool() {
        get_cached(&TYPEOF_BOOLEAN, "boolean")
    } else if jsval.is_any_string() {
        // String pointer (STRING_TAG) OR inline SSO (SHORT_STRING_TAG).
        // `typeof` doesn't distinguish between representations — both
        // are observed as "string" from user code.
        get_cached(&TYPEOF_STRING, "string")
    } else if crate::value::is_js_handle(value) {
        // JS handle from V8 runtime — ask V8 whether it's a callable, otherwise default
        // to "object". Issue #258: pre-fix this always returned "object" even for
        // V8 functions; the registered callback now flips it to "function" when the
        // handle wraps a v8::Function.
        if crate::value::js_handle_is_function(value) {
            get_cached(&TYPEOF_FUNCTION, "function")
        } else {
            get_cached(&TYPEOF_OBJECT, "object")
        }
    } else if jsval.is_pointer() {
        // Object/array/closure/symbol pointer - check via the side-table first.
        let ptr = jsval.as_pointer::<u8>();
        if !ptr.is_null() && (ptr as usize) > 0x10000 {
            // Symbols: registered in SYMBOL_POINTERS (handles both gc_malloc'd
            // and Box-leaked symbols, which have no GcHeader).
            if crate::symbol::is_registered_symbol(ptr as usize) {
                get_cached(&TYPEOF_SYMBOL, "symbol")
            } else {
                // ClosureHeader has type_tag at offset 12 (after func_ptr:8 + capture_count:4)
                let type_tag = unsafe { *(ptr.add(12) as *const u32) };
                if type_tag == crate::closure::CLOSURE_MAGIC {
                    get_cached(&TYPEOF_FUNCTION, "function")
                } else {
                    get_cached(&TYPEOF_OBJECT, "object")
                }
            }
        } else {
            get_cached(&TYPEOF_OBJECT, "object")
        }
    } else if jsval.is_bigint() {
        get_cached(&TYPEOF_BIGINT, "bigint")
    } else if jsval.is_int32() {
        // Refs #618 / #420 followup: class refs share INT32_TAG storage
        // shape (codegen emits `INT32_TAG | class_id` as the value form
        // for `Expr::ClassRef`). Distinguish a class id from a real int32
        // by checking the vtable registry — registered class ids return
        // "function" per JS spec; everything else is "number".
        let raw = jsval.bits() & 0xFFFF_FFFF;
        let class_id = raw as u32;
        if crate::object::is_class_id_registered(class_id) {
            get_cached(&TYPEOF_FUNCTION, "function")
        } else {
            get_cached(&TYPEOF_NUMBER, "number")
        }
    } else {
        // Issue #654: typed-array pointers arrive as a raw `i64 → f64`
        // bitcast (no NaN-box tag) per the codegen for `new Float64Array(...)`
        // et al. Without this arm, `typeof a` returned "number" because the
        // raw pointer bits flow through the `is_pointer()` check above
        // (POINTER_TAG fails) and land in this fallthrough. Match against
        // the typed-array registry — addresses recorded by `typed_array_alloc`
        // — so `typeof` reports "object" per JS spec.
        let bits = value.to_bits();
        let top16 = bits >> 48;
        if top16 == 0 && bits >= 0x10000 {
            let addr = bits as usize;
            if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
                return get_cached(&TYPEOF_OBJECT, "object");
            }
        }
        // Date — stored as a raw f64 millisecond timestamp, registered
        // in `DATE_REGISTRY` because the value carries no tag of its
        // own. `typeof new Date()` must be "object" per ECMA-262 — any
        // duck-type test (`typeof v === "object" && v instanceof Date`)
        // gates Date handling on this returning the right thing.
        if crate::date::is_registered_date_bits(bits) {
            return get_cached(&TYPEOF_OBJECT, "object");
        }
        // Regular f64 number
        get_cached(&TYPEOF_NUMBER, "number")
    }
}
