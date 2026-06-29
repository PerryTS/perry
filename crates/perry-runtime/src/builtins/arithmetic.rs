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

/// Whether `v` has ECMAScript Type Object (not a primitive). True for plain
/// objects, arrays, functions, Dates and boxed primitive wrappers; false for
/// Symbols (which are NaN-boxed pointers but are primitives) and for the
/// native-handle id-space (sockets, zlib streams, … — see
/// `value::addr_class`) which must not be dereferenced as heap objects in a
/// coercion path.
fn eq_is_object(v: JSValue) -> bool {
    if !v.is_pointer() {
        return false;
    }
    let ptr = v.as_pointer::<u8>();
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return false;
    }
    !crate::symbol::is_registered_symbol(ptr as usize)
}

/// JS abstract equality (==). Implements the coercion rules:
/// - Same type: use strict equality
/// - null == undefined: true
/// - number == string: coerce string to number
/// - boolean == anything: coerce boolean to number, recurse
/// - string == number: coerce string to number
#[no_mangle]
pub extern "C" fn js_loose_eq(a: JSValue, b: JSValue) -> JSValue {
    // Normalize raw module-slot object pointers (top16 == 0) to their
    // POINTER_TAG'd form so reference equality sees one representation.
    let a = JSValue::from_bits(crate::value::equality::normalize_raw_object_bits(a.bits()));
    let b = JSValue::from_bits(crate::value::equality::normalize_raw_object_bits(b.bits()));
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
    // Object == Object → reference equality (ES2024 §7.2.15 step 1, same Type).
    // Distinct object identities already failed the same-bits fast path above,
    // so two objects here are never equal — and we must NOT unwrap a boxed
    // wrapper when the other side is also an object (`new Boolean(true) !=
    // new Boolean(true)` is `true`).
    if eq_is_object(a) && eq_is_object(b) {
        return JSValue::bool(false);
    }
    // Boxed primitives compare via their wrapped primitive value under
    // abstract equality (`new Number(5) == 5`, and sloppy primitive accessors
    // return boxed receivers).
    if let Some((_, payload)) = boxed_primitive_payload(f64::from_bits(a.bits())) {
        return js_loose_eq(JSValue::from_bits(payload.to_bits()), b);
    }
    if let Some((_, payload)) = boxed_primitive_payload(f64::from_bits(b.bits())) {
        return js_loose_eq(a, JSValue::from_bits(payload.to_bits()));
    }
    // Object == primitive → ToPrimitive(object), then retry (ES2024 §7.2.15
    // steps 10-11). Object-vs-object was settled above; symbols are primitives
    // (`eq_is_object` excludes them) and correctly fall through to not-equal.
    // Done before the BigInt block so `0n == { valueOf() { return 0n } }` works.
    if eq_is_object(a) {
        let pa = unsafe { rel_to_primitive(f64::from_bits(a.bits())) };
        return js_loose_eq(JSValue::from_bits(pa.to_bits()), b);
    }
    if eq_is_object(b) {
        let pb = unsafe { rel_to_primitive(f64::from_bits(b.bits())) };
        return js_loose_eq(a, JSValue::from_bits(pb.to_bits()));
    }
    // BigInt abstract equality (ES2024 §7.2.15). Neither side is
    // null/undefined here and boxed wrappers (incl. `Object(0n)`) have already
    // been unwrapped above.
    if a.is_bigint() || b.is_bigint() {
        // BigInt == BigInt → compare by mathematical value.
        if a.is_bigint() && b.is_bigint() {
            return JSValue::bool(
                crate::bigint::js_bigint_cmp(a.as_bigint_ptr(), b.as_bigint_ptr()) == 0,
            );
        }
        let (big, other) = if a.is_bigint() { (a, b) } else { (b, a) };
        // BigInt == Boolean → ToNumber(boolean) then BigInt == Number.
        let other = if other.is_bool() {
            JSValue::number(if other.as_bool() { 1.0 } else { 0.0 })
        } else {
            other
        };
        // BigInt == Number → exact integer comparison (NaN/±Infinity/fractional
        // are never equal). `bigint_cmp_f64` returns 0 only on exact equality.
        if other.is_number() {
            return JSValue::bool(
                crate::bigint::bigint_cmp_f64(big.as_bigint_ptr(), other.as_number()) == 0,
            );
        }
        // BigInt == String → StringToBigInt(string); a non-numeric string makes
        // the result `false` (StringToBigInt is undefined → not equal).
        if other.is_any_string() {
            let s = unsafe { string_content_for_bigint(f64::from_bits(other.bits())) };
            return match crate::bigint::string_to_bigint(&s) {
                Some(ny) => {
                    JSValue::bool(crate::bigint::js_bigint_cmp(big.as_bigint_ptr(), ny) == 0)
                }
                None => JSValue::bool(false),
            };
        }
        // BigInt == Symbol / anything else → not equal.
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

// ----------------------------------------------------------------------------
// Abstract Relational Comparison (ES2024 §7.2.13: `IsLessThan(x, y, LeftFirst)`)
// ----------------------------------------------------------------------------
//
// The previous `js_lt`/`le`/`gt`/`ge` did a bare `a.to_number() < b.to_number()`,
// which is wrong for every non-numeric operand: it never runs `ToPrimitive`
// (`{ valueOf() {…} } < 1`), never lexicographically compares two strings, and
// derefs BigInt / object operands as raw doubles (NaN → unordered → always
// `false`). The codegen keeps a bare-`fcmp` fast path for *statically numeric*
// operands; everything else now routes through `js_rel_{lt,le,gt,ge}` which call
// the full abstract relational comparison below.

const REL_FALSE: i32 = 0;
const REL_TRUE: i32 = 1;
const REL_UNDEFINED: i32 = 2;

const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE_BITS: u64 = 0x7FFC_0000_0000_0003;

#[inline]
fn rel_bool_f64(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE_BITS } else { TAG_FALSE_BITS })
}

/// `ToPrimitive(value, NUMBER)` returning the primitive as a NaN-boxed `f64`.
/// A `Date` coerces to its millisecond timestamp; an object with no usable
/// `valueOf`/`toString` primitive falls back to the ordinary `ToString`
/// (`"[object Object]"`, a function's source, …). Propagates any user
/// exception or `TypeError` by unwinding.
unsafe fn rel_to_primitive(value: f64) -> f64 {
    if crate::date::is_date_value(value) {
        // ToPrimitive(date, number) → Date.prototype.valueOf → the ms timestamp,
        // which is itself a Number (a plain `f64` is its own NaN-box).
        return crate::date::js_date_coerce_number(value);
    }
    // ToPrimitive(temporal, NUMBER) → the type's `valueOf`, which is a hard
    // `TypeError` for every `Temporal.*` value (the spec bans relational ordering
    // of Temporal values: `plainDate < plainDate` throws). Without this the cell
    // fell through to the `DefaultString` arm and compared ISO strings silently.
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(value) {
        return crate::temporal::dispatch::call_method(value, "valueOf", &[]);
    }
    match crate::value::to_primitive_number(value) {
        crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => p,
        crate::value::OrdinaryToPrimitiveOutcome::DefaultString => {
            let s = crate::value::js_jsvalue_to_string(value);
            crate::value::js_nanbox_string(s as i64)
        }
        crate::value::OrdinaryToPrimitiveOutcome::TypeError => {
            crate::collection_iter::throw_type_error("Cannot convert object to primitive value")
        }
    }
}

/// Lexicographic (byte-order) compare of two already-`ToPrimitive`'d string
/// values. Returns `< 0`, `0`, `> 0` like `memcmp`. (Lone-surrogate / true
/// UTF-16 code-unit ordering is a separate pre-existing WTF-8 gap.)
unsafe fn rel_string_compare(a: f64, b: f64) -> i32 {
    let pa = crate::value::js_get_string_pointer_unified(a) as *const crate::string::StringHeader;
    let pb = crate::value::js_get_string_pointer_unified(b) as *const crate::string::StringHeader;
    crate::string::js_string_compare(pa, pb)
}

/// `IsLessThan(x, y, LeftFirst)` — the abstract relational comparison.
/// `x_first` is `LeftFirst`: it controls only the order in which `ToPrimitive`
/// runs on the two operands (observable when a `valueOf`/`toString` has side
/// effects). Returns [`REL_TRUE`], [`REL_FALSE`], or [`REL_UNDEFINED`].
unsafe fn abstract_relational(x: f64, y: f64, x_first: bool) -> i32 {
    let (px, py) = if x_first {
        let px = rel_to_primitive(x);
        let py = rel_to_primitive(y);
        (px, py)
    } else {
        let py = rel_to_primitive(y);
        let px = rel_to_primitive(x);
        (px, py)
    };

    let vx = JSValue::from_bits(px.to_bits());
    let vy = JSValue::from_bits(py.to_bits());

    // Both String → code-unit (byte) compare; never `undefined`.
    if vx.is_any_string() && vy.is_any_string() {
        return if rel_string_compare(px, py) < 0 {
            REL_TRUE
        } else {
            REL_FALSE
        };
    }

    let x_big = vx.is_bigint();
    let y_big = vy.is_bigint();

    // BigInt vs String / String vs BigInt: parse the string as a BigInt
    // (StringToBigInt); a non-numeric string makes the comparison `undefined`.
    if x_big && vy.is_any_string() {
        let s = string_content_for_bigint(py);
        return match crate::bigint::string_to_bigint(&s) {
            None => REL_UNDEFINED,
            Some(ny) => {
                if crate::bigint::js_bigint_cmp(vx.as_bigint_ptr(), ny) < 0 {
                    REL_TRUE
                } else {
                    REL_FALSE
                }
            }
        };
    }
    if vx.is_any_string() && y_big {
        let s = string_content_for_bigint(px);
        return match crate::bigint::string_to_bigint(&s) {
            None => REL_UNDEFINED,
            Some(nx) => {
                if crate::bigint::js_bigint_cmp(nx, vy.as_bigint_ptr()) < 0 {
                    REL_TRUE
                } else {
                    REL_FALSE
                }
            }
        };
    }

    // Both BigInt → exact integer compare.
    if x_big && y_big {
        return if crate::bigint::js_bigint_cmp(vx.as_bigint_ptr(), vy.as_bigint_ptr()) < 0 {
            REL_TRUE
        } else {
            REL_FALSE
        };
    }

    // BigInt vs Number (mixed): exact mathematical compare. `js_number_coerce`
    // is `ToNumber` and throws on a Symbol operand, as the spec requires.
    if x_big {
        let yn = js_number_coerce(py);
        return match crate::bigint::bigint_cmp_f64(vx.as_bigint_ptr(), yn) {
            2 => REL_UNDEFINED,
            c if c < 0 => REL_TRUE,
            _ => REL_FALSE,
        };
    }
    if y_big {
        let xn = js_number_coerce(px);
        // `bigint_cmp_f64(y, xn)` is the sign of (y − x); x < y ⇔ that is positive.
        return match crate::bigint::bigint_cmp_f64(vy.as_bigint_ptr(), xn) {
            2 => REL_UNDEFINED,
            c if c > 0 => REL_TRUE,
            _ => REL_FALSE,
        };
    }

    // Both Number (after ToNumber). NaN on either side → undefined.
    let xn = js_number_coerce(px);
    let yn = js_number_coerce(py);
    if xn.is_nan() || yn.is_nan() {
        return REL_UNDEFINED;
    }
    if xn < yn {
        REL_TRUE
    } else {
        REL_FALSE
    }
}

/// Materialize a string primitive's bytes into an owned `String` for
/// `StringToBigInt`. Handles both heap (`STRING_TAG`) and inline SSO strings.
unsafe fn string_content_for_bigint(value: f64) -> String {
    let ptr =
        crate::value::js_get_string_pointer_unified(value) as *const crate::string::StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    String::from_utf8_lossy(bytes).into_owned()
}

/// `x < y` — codegen routes here for any relational `<` whose operands are not
/// both statically numeric. Returns a NaN-boxed boolean (`f64`).
#[no_mangle]
pub extern "C" fn js_rel_lt(x: f64, y: f64) -> f64 {
    rel_bool_f64(unsafe { abstract_relational(x, y, true) } == REL_TRUE)
}

/// `x > y` ⇔ `IsLessThan(y, x, false)` is true (right operand `ToPrimitive`'d first).
#[no_mangle]
pub extern "C" fn js_rel_gt(x: f64, y: f64) -> f64 {
    rel_bool_f64(unsafe { abstract_relational(y, x, false) } == REL_TRUE)
}

/// `x <= y` ⇔ `IsLessThan(y, x, false)` is `false` (not `true`, not `undefined`).
#[no_mangle]
pub extern "C" fn js_rel_le(x: f64, y: f64) -> f64 {
    rel_bool_f64(unsafe { abstract_relational(y, x, false) } == REL_FALSE)
}

/// `x >= y` ⇔ `IsLessThan(x, y, true)` is `false` (not `true`, not `undefined`).
#[no_mangle]
pub extern "C" fn js_rel_ge(x: f64, y: f64) -> f64 {
    rel_bool_f64(unsafe { abstract_relational(x, y, true) } == REL_FALSE)
}

// The `js_rel_*` helpers are reached only from Perry-emitted LLVM (the relational
// fallthrough in codegen), so a bitcode/auto-optimize link can dead-strip them
// and leave `undefined _js_rel_lt …`. Pin them with `#[used]` statics — same
// pattern as the write-barrier roots in `gc/barrier.rs`.
#[used]
static KEEP_REL_LT: extern "C" fn(f64, f64) -> f64 = js_rel_lt;
#[used]
static KEEP_REL_GT: extern "C" fn(f64, f64) -> f64 = js_rel_gt;
#[used]
static KEEP_REL_LE: extern "C" fn(f64, f64) -> f64 = js_rel_le;
#[used]
static KEEP_REL_GE: extern "C" fn(f64, f64) -> f64 = js_rel_ge;

#[no_mangle]
pub extern "C" fn js_lt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::from_bits(js_rel_lt(f64::from_bits(a.bits()), f64::from_bits(b.bits())).to_bits())
}

#[no_mangle]
pub extern "C" fn js_le(a: JSValue, b: JSValue) -> JSValue {
    JSValue::from_bits(js_rel_le(f64::from_bits(a.bits()), f64::from_bits(b.bits())).to_bits())
}

#[no_mangle]
pub extern "C" fn js_gt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::from_bits(js_rel_gt(f64::from_bits(a.bits()), f64::from_bits(b.bits())).to_bits())
}

#[no_mangle]
pub extern "C" fn js_ge(a: JSValue, b: JSValue) -> JSValue {
    JSValue::from_bits(js_rel_ge(f64::from_bits(a.bits()), f64::from_bits(b.bits())).to_bits())
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
        // The `>= 0x100000` floor (raised from 0x10000, #1843) skips the deref
        // for native-module registry handles (net.Socket, zlib stream, crypto,
        // …) — small ids bit-OR'd with POINTER_TAG, not real heap pointers,
        // which always live above 0x100000. `typeof aHandle` is "object".
        // Reading a fake handle's `[ptr+12]` type tag otherwise segfaults
        // (e.g. zlib's reserved stream base).
        let ptr = jsval.as_pointer::<u8>();
        // A Proxy id is a SMALL pointer (below the heap floor), so check the
        // registry before the floor gate. typeof proxy is "function" iff its
        // (possibly nested) [[ProxyTarget]] is callable.
        if crate::proxy::js_proxy_is_proxy(value) == 1 {
            return if crate::proxy::proxy_wraps_callable(value) {
                get_cached(&TYPEOF_FUNCTION, "function")
            } else {
                get_cached(&TYPEOF_OBJECT, "object")
            };
        }
        if crate::value::addr_class::is_above_handle_band(ptr as usize) {
            // Symbols: registered in SYMBOL_POINTERS (handles both gc_malloc'd
            // and Box-leaked symbols, which have no GcHeader).
            if crate::symbol::is_registered_symbol(ptr as usize) {
                get_cached(&TYPEOF_SYMBOL, "symbol")
            } else if crate::date::is_date_cell_addr(ptr as usize) {
                // Date is a NaN-boxed pointer to an 8-byte `DateCell` (#2089).
                // `typeof aDate === "object"`. Check this BEFORE reading the
                // `type_tag` at offset 12 below — the cell is only 8 bytes, so
                // that read would fall off the end of the allocation.
                get_cached(&TYPEOF_OBJECT, "object")
            } else {
                // ClosureHeader has type_tag at offset 12 (after func_ptr:8 + capture_count:4)
                let type_tag =
                    unsafe { *(ptr.add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32) };
                if type_tag == crate::closure::CLOSURE_MAGIC {
                    get_cached(&TYPEOF_FUNCTION, "function")
                } else if crate::object::is_class_object_ptr(ptr) {
                    // #1789: a class-expression VALUE is a heap object stamped
                    // with OBJECT_TYPE_CLASS — `typeof aClassObject ===
                    // "function"` (classes are callable in JS), matching the
                    // INT32 ClassRef case below.
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
        // Date is now a NaN-boxed `DateCell` pointer (#2089), handled in the
        // `is_pointer()` arm above — it no longer reaches this numeric
        // fallthrough.
        // #1650: Web Streams handles (ReadableStream / WritableStream /
        // reader / writer) are returned as a raw `id as f64` whole number in
        // a high id range (#1545), so they reach this fallthrough and would
        // otherwise report "number". Consult the stdlib kind-probe — the same
        // side-channel `instanceof ReadableStream` uses — so `typeof
        // res.body === "object"` matches the spec (Response.body is a
        // ReadableStream object).
        if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
            if let Some(probe) = crate::object::stream_handle_kind_probe() {
                if unsafe { probe(value as usize) } != 0 {
                    return get_cached(&TYPEOF_OBJECT, "object");
                }
            }
        }
        // Regular f64 number
        get_cached(&TYPEOF_NUMBER, "number")
    }
}
