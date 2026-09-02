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
    // Object == primitive → ToPrimitive(object), then retry (ES2024 §7.2.15
    // steps 10-11). Object-vs-object was settled above; symbols are primitives
    // (`eq_is_object` excludes them) and correctly fall through to not-equal.
    // Done before the BigInt block so `0n == { valueOf() { return 0n } }` works.
    // #6655: `rel_to_primitive` runs a user `valueOf`/`toString`, so it can
    // allocate, collect and evacuate. The *other* operand is a raw NaN-boxed
    // local here — not a GC root — so it must be rooted across the coercion and
    // re-read through its handle before the recursive call, or `==` compares
    // against a forwarded address.
    if eq_is_object(a) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let b_handle = scope.root_nanbox_f64(f64::from_bits(b.bits()));
        let pa = unsafe { rel_to_primitive(f64::from_bits(a.bits())) };
        let pa_handle = scope.root_nanbox_f64(pa);
        return js_loose_eq(
            JSValue::from_bits(pa_handle.get_nanbox_u64()),
            JSValue::from_bits(b_handle.get_nanbox_u64()),
        );
    }
    if eq_is_object(b) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let a_handle = scope.root_nanbox_f64(f64::from_bits(a.bits()));
        let pb = unsafe { rel_to_primitive(f64::from_bits(b.bits())) };
        let pb_handle = scope.root_nanbox_f64(pb);
        return js_loose_eq(
            JSValue::from_bits(a_handle.get_nanbox_u64()),
            JSValue::from_bits(pb_handle.get_nanbox_u64()),
        );
    }
    // BigInt abstract equality (ES2024 §7.2.15). Neither side is
    // null/undefined here and boxed wrappers have already gone through the
    // observable ToPrimitive operation above.
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
    // #6655: `rel_to_primitive` runs a user `Symbol.toPrimitive` / `valueOf` /
    // `toString`, which can allocate, trigger a GC and *evacuate* live objects.
    // Every raw NaN-boxed `f64` in a local here is invisible to the collector,
    // so pre-fix the second operand was held unrooted across the first
    // coercion, and `px` — frequently a *freshly allocated* heap string from
    // the `DefaultString` arm — was held unrooted across the second. Root both
    // inputs before the first coercion and both primitives as they are
    // produced, then read every value back through its handle. Same discipline
    // as `dynamic_bigint_binary_op` / `js_dynamic_ushr` in `value/dynamic_arith.rs`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let x_in = scope.root_nanbox_f64(x);
    let y_in = scope.root_nanbox_f64(y);
    let (px_handle, py_handle) = if x_first {
        let px = scope.root_nanbox_f64(rel_to_primitive(x_in.get_nanbox_f64()));
        let py = scope.root_nanbox_f64(rel_to_primitive(y_in.get_nanbox_f64()));
        (px, py)
    } else {
        let py = scope.root_nanbox_f64(rel_to_primitive(y_in.get_nanbox_f64()));
        let px = scope.root_nanbox_f64(rel_to_primitive(x_in.get_nanbox_f64()));
        (px, py)
    };
    let px = px_handle.get_nanbox_f64();
    let py = py_handle.get_nanbox_f64();

    // NOTE: `vx` / `vy` are *snapshots*. Tag predicates (`is_any_string`,
    // `is_bigint`, …) stay valid across a GC because evacuation preserves the
    // tag, but any pointer payload read out of them (`as_bigint_ptr`) must be
    // re-derived from the handle at the point of use — see the BigInt arms below.
    let vx = JSValue::from_bits(px.to_bits());
    let vy = JSValue::from_bits(py.to_bits());

    // Both String → code-unit (byte) compare; never `undefined`.
    if vx.is_any_string() && vy.is_any_string() {
        return if rel_string_compare(px_handle.get_nanbox_f64(), py_handle.get_nanbox_f64()) < 0 {
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
        let s = string_content_for_bigint(py_handle.get_nanbox_f64());
        // `string_to_bigint` allocates the parsed BigInt, so re-derive the `x`
        // pointer from its handle *after* that call — the snapshot in `vx` may
        // name a forwarded address by now (#6655).
        return match crate::bigint::string_to_bigint(&s) {
            None => REL_UNDEFINED,
            Some(ny) => {
                let px_ptr =
                    JSValue::from_bits(px_handle.get_nanbox_f64().to_bits()).as_bigint_ptr();
                if crate::bigint::js_bigint_cmp(px_ptr, ny) < 0 {
                    REL_TRUE
                } else {
                    REL_FALSE
                }
            }
        };
    }
    if vx.is_any_string() && y_big {
        let s = string_content_for_bigint(px_handle.get_nanbox_f64());
        return match crate::bigint::string_to_bigint(&s) {
            None => REL_UNDEFINED,
            Some(nx) => {
                let py_ptr =
                    JSValue::from_bits(py_handle.get_nanbox_f64().to_bits()).as_bigint_ptr();
                if crate::bigint::js_bigint_cmp(nx, py_ptr) < 0 {
                    REL_TRUE
                } else {
                    REL_FALSE
                }
            }
        };
    }

    // Both BigInt → exact integer compare. `js_bigint_cmp` does not allocate,
    // but re-read both pointers through the handles anyway so this arm stays
    // correct if it ever grows an allocating step.
    if x_big && y_big {
        let px_ptr = JSValue::from_bits(px_handle.get_nanbox_f64().to_bits()).as_bigint_ptr();
        let py_ptr = JSValue::from_bits(py_handle.get_nanbox_f64().to_bits()).as_bigint_ptr();
        return if crate::bigint::js_bigint_cmp(px_ptr, py_ptr) < 0 {
            REL_TRUE
        } else {
            REL_FALSE
        };
    }

    // BigInt vs Number (mixed): exact mathematical compare. `js_number_coerce`
    // is `ToNumber` and throws on a Symbol operand, as the spec requires.
    if x_big {
        // `js_number_coerce` on a string primitive can allocate; re-derive the
        // BigInt pointer from its handle after the coercion (#6655).
        let yn = js_number_coerce(py_handle.get_nanbox_f64());
        let px_ptr = JSValue::from_bits(px_handle.get_nanbox_f64().to_bits()).as_bigint_ptr();
        return match crate::bigint::bigint_cmp_f64(px_ptr, yn) {
            2 => REL_UNDEFINED,
            c if c < 0 => REL_TRUE,
            _ => REL_FALSE,
        };
    }
    if y_big {
        let xn = js_number_coerce(px_handle.get_nanbox_f64());
        let py_ptr = JSValue::from_bits(py_handle.get_nanbox_f64().to_bits()).as_bigint_ptr();
        // `bigint_cmp_f64(y, xn)` is the sign of (y − x); x < y ⇔ that is positive.
        return match crate::bigint::bigint_cmp_f64(py_ptr, xn) {
            2 => REL_UNDEFINED,
            c if c > 0 => REL_TRUE,
            _ => REL_FALSE,
        };
    }

    // Both Number (after ToNumber). NaN on either side → undefined.
    let xn = js_number_coerce(px_handle.get_nanbox_f64());
    let yn = js_number_coerce(py_handle.get_nanbox_f64());
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

/// Can this primitive operand be converted to Number without allocation,
/// user code, or observable coercion ordering?
///
/// `abstract_relational` opens a `RuntimeHandleScope`, roots both operands and
/// runs `ToPrimitive` on each — necessary only because a *heap* operand can run
/// user `valueOf`/`toString`. Numbers, undefined, null, and booleans contain no
/// pointer and their ToPrimitive/ToNumber results are fixed by the spec, so the
/// whole apparatus is dead weight. Strings stay on the full path because two
/// strings compare lexicographically; BigInts, Symbols, objects, and internal
/// sentinels stay there for their distinct semantics and errors.
///
/// NaN must stay `false` for all four operators, which Rust's `<`/`>`/`<=`/`>=`
/// on `f64` already deliver.
#[inline(always)]
fn rel_numeric_operand(v: f64) -> Option<f64> {
    const TAG_BAND_FLOOR: u64 = 0x7FF9_0000_0000_0000;
    if (v.to_bits() & 0x7FFF_0000_0000_0000) < TAG_BAND_FLOOR {
        return Some(v);
    }
    let jv = crate::value::JSValue::from_bits(v.to_bits());
    if jv.is_int32() {
        return Some(jv.as_int32() as f64);
    }
    match v.to_bits() {
        crate::value::TAG_UNDEFINED => Some(f64::NAN),
        crate::value::TAG_NULL | crate::value::TAG_FALSE => Some(0.0),
        crate::value::TAG_TRUE => Some(1.0),
        _ => None,
    }
}

/// `x < y` — codegen routes here for any relational `<` whose operands are not
/// both statically numeric. Returns a NaN-boxed boolean (`f64`).
#[no_mangle]
pub extern "C" fn js_rel_lt(x: f64, y: f64) -> f64 {
    if let (Some(a), Some(b)) = (rel_numeric_operand(x), rel_numeric_operand(y)) {
        return rel_bool_f64(a < b);
    }
    rel_bool_f64(unsafe { abstract_relational(x, y, true) } == REL_TRUE)
}

/// `x > y` ⇔ `IsLessThan(y, x, false)` is true (right operand `ToPrimitive`'d first).
#[no_mangle]
pub extern "C" fn js_rel_gt(x: f64, y: f64) -> f64 {
    if let (Some(a), Some(b)) = (rel_numeric_operand(x), rel_numeric_operand(y)) {
        return rel_bool_f64(a > b);
    }
    rel_bool_f64(unsafe { abstract_relational(y, x, false) } == REL_TRUE)
}

/// `x <= y` ⇔ `IsLessThan(y, x, false)` is `false` (not `true`, not `undefined`).
#[no_mangle]
pub extern "C" fn js_rel_le(x: f64, y: f64) -> f64 {
    if let (Some(a), Some(b)) = (rel_numeric_operand(x), rel_numeric_operand(y)) {
        return rel_bool_f64(a <= b);
    }
    rel_bool_f64(unsafe { abstract_relational(y, x, false) } == REL_FALSE)
}

/// `x >= y` ⇔ `IsLessThan(x, y, true)` is `false` (not `true`, not `undefined`).
#[no_mangle]
pub extern "C" fn js_rel_ge(x: f64, y: f64) -> f64 {
    if let (Some(a), Some(b)) = (rel_numeric_operand(x), rel_numeric_operand(y)) {
        return rel_bool_f64(a >= b);
    }
    rel_bool_f64(unsafe { abstract_relational(x, y, true) } == REL_FALSE)
}

// The `js_rel_*` helpers are reached only from Perry-emitted LLVM (the relational
// fallthrough in codegen), so a bitcode/auto-optimize link can dead-strip them
// and leave `undefined _js_rel_lt …`. Pin them with `#[used]` statics — same
// pattern as the write-barrier roots in `gc/barrier.rs`.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REL_LT: extern "C" fn(f64, f64) -> f64 = js_rel_lt;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REL_GT: extern "C" fn(f64, f64) -> f64 = js_rel_gt;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REL_LE: extern "C" fn(f64, f64) -> f64 = js_rel_le;
#[cfg(feature = "keepalive-anchors")]
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

// `typeof` returns one of eight strings, so each is allocated once and cached
// rather than rebuilt per call.
//
// #7211: these cells are GC ROOTS and are registered as such
// (`scan_typeof_string_roots_mut`, wired in `gc/mod.rs`). They are at module
// scope rather than inside `js_value_typeof` for exactly that reason — a
// scanner has to be able to reach them.
//
// Before that registration this cache was a deterministic use-after-free, and
// it is the bug that killed `sfw-registry --help` 10/10 under a
// `PERRY_GC_MOVING_LOOP_POLLS=1` build. `js_string_from_bytes` allocates in the
// NURSERY; nothing else references the result; so the first minor either swept
// it or evacuated it, and this raw pointer named the abandoned bytes from then
// on. Every later `typeof x === "string"` handed `js_string_equals` a from-space
// address. `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`
// reported it precisely: `obj_type=3 size=40 retired_by_minor=#0` — a string,
// 32-byte header plus `"string"`, retired by the very first collection.
//
// `retired_by_minor=#0` is the tell for this whole shape, and it is worth
// recognising: an ordinary #7154-class stale register goes bad at whichever
// collection lands inside a few-instruction window, so it is timing-dependent.
// A cache that is never rooted goes bad at the FIRST collection and stays bad,
// which is why this reproduced 10/10 while the register bugs needed a zod
// workload and ten rounds.
//
// It is invisible to `scripts/gc_root_dominance_check.py` by construction: that
// tool reads emitted LLVM IR, and this is a runtime-side table. The static
// checker could never have found it, which is why the runtime instruments had
// to be pointed at the registry first.
//
// One hot-TLS array rather than eight `std::thread_local!`s: `typeof` on a
// branchy fast path (`typeof merge !== "undefined"` per command in an ECS
// apply loop) paid a `_tlv_get_addr` resolution per call just to reach the
// cached pointer. The slot constants keep the eight names.
const TYPEOF_UNDEFINED: usize = 0;
const TYPEOF_OBJECT: usize = 1;
const TYPEOF_BOOLEAN: usize = 2;
const TYPEOF_NUMBER: usize = 3;
const TYPEOF_STRING: usize = 4;
const TYPEOF_FUNCTION: usize = 5;
const TYPEOF_BIGINT: usize = 6;
const TYPEOF_SYMBOL: usize = 7;
const TYPEOF_CACHE_SLOTS: usize = 8;

crate::perry_thread_local! {
    static TYPEOF_CACHE: [std::cell::Cell<*mut StringHeader>; TYPEOF_CACHE_SLOTS] =
        const { [const { std::cell::Cell::new(std::ptr::null_mut()) }; TYPEOF_CACHE_SLOTS] };
}

/// Get or initialize a cached `typeof` string.
fn get_cached(slot: usize, s: &str) -> *mut StringHeader {
    TYPEOF_CACHE.with(|cells| {
        let cell = &cells[slot];
        let ptr = cell.get();
        if !ptr.is_null() {
            return ptr;
        }
        let new_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        cell.set(new_ptr);
        new_ptr
    })
}

/// GC mutable-root scanner for the eight cached `typeof` strings (#7211).
///
/// Marks them, so an unreferenced cache entry is never swept, AND rewrites
/// them, so an evacuating minor that relocates one leaves the cell naming the
/// new address instead of from-space. Both halves matter: marking alone would
/// still hand out a pre-move pointer after a copying minor, which is the
/// distinction `gc-rooting-invariant.md` keeps having to make.
///
/// `STRING_TAG` rather than the default `POINTER_TAG` because these are
/// `StringHeader`s, matching `json::scan_parse_roots_mut`'s interned-key
/// treatment.
pub fn scan_typeof_string_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    TYPEOF_CACHE.with(|cells| {
        for cell in cells {
            let mut ptr = cell.get() as *const StringHeader;
            if ptr.is_null() {
                continue;
            }
            if visitor.visit_tagged_raw_const_ptr_slot(&mut ptr, crate::value::STRING_TAG) {
                cell.set(ptr as *mut StringHeader);
            }
        }
    });
}

/// The eight cells and their payloads, in `scan_typeof_string_roots_mut`
/// order. Test-only. It exists so a test can assert the scanner reaches EVERY
/// cell: that scanner is eight hand-written `visit(...)` calls, and a dropped
/// line is invisible to any test that exercises only some of them.
#[cfg(test)]
fn typeof_cache_entries_for_test() -> [(usize, &'static str); 8] {
    [
        (TYPEOF_UNDEFINED, "undefined"),
        (TYPEOF_OBJECT, "object"),
        (TYPEOF_BOOLEAN, "boolean"),
        (TYPEOF_NUMBER, "number"),
        (TYPEOF_STRING, "string"),
        (TYPEOF_FUNCTION, "function"),
        (TYPEOF_BIGINT, "bigint"),
        (TYPEOF_SYMBOL, "symbol"),
    ]
}

/// Drop every cached `typeof` string. Test-only: a rooting test has to start
/// from an empty cache so the strings it then allocates are its own, in a
/// known arena, rather than survivors of whichever test ran first on this
/// thread.
#[cfg(test)]
// #7277: no callers anywhere in the workspace. Kept rather than deleted because
// it is the only handle on this cache's reset path, but it is dead today — if
// nothing adopts it, delete it rather than letting it rot behind this attribute.
#[allow(dead_code)]
pub(crate) fn reset_typeof_string_cache_for_test() {
    for (slot, _) in typeof_cache_entries_for_test() {
        TYPEOF_CACHE.with(|cells| cells[slot].set(std::ptr::null_mut()));
    }
}

/// Allocate all eight cached strings, exactly as eight `typeof` calls of eight
/// different value shapes would. Test-only; reaching `bigint` and `symbol`
/// from Rust otherwise means building a BigInt and a registered Symbol.
#[cfg(test)]
pub(crate) fn populate_typeof_string_cache_for_test() {
    for (slot, text) in typeof_cache_entries_for_test() {
        get_cached(slot, text);
    }
}

/// Read the eight cells without populating them. Test-only.
#[cfg(test)]
pub(crate) fn typeof_string_cache_cells_for_test() -> [*mut StringHeader; 8] {
    typeof_cache_entries_for_test().map(|(slot, _)| TYPEOF_CACHE.with(|cells| cells[slot].get()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
enum ValueTypeofTag {
    Undefined = 0,
    Object = 1,
    Boolean = 2,
    Number = 3,
    String = 4,
    Function = 5,
    BigInt = 6,
    Symbol = 7,
}

/// Classify a value once, independently of the string representation exposed
/// by the `typeof` operator. Keeping every exceptional Perry representation in
/// this one classifier makes the literal-comparison entry point below exactly
/// agree with [`js_value_typeof`]: class refs, callable proxies, raw typed-array
/// pointers, stream handles, Symbols, closures, and class-expression objects
/// cannot drift between the two APIs.
#[inline]
fn classify_value_typeof(value: f64) -> ValueTypeofTag {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() || jsval.bits() == crate::value::TAG_HOLE {
        // #9462: `TAG_HOLE` is the internal empty-slot sentinel, and its bit
        // pattern IS a NaN — without this arm it fell all the way through to
        // the "regular f64 number" tail and `typeof` answered "number". User
        // code only ever observes an unset slot as `undefined`, which is also
        // what node reports.
        ValueTypeofTag::Undefined
    } else if jsval.is_null() {
        // typeof null === "object" in JavaScript
        ValueTypeofTag::Object
    } else if jsval.is_bool() {
        ValueTypeofTag::Boolean
    } else if jsval.is_any_string() {
        // String pointer (STRING_TAG) OR inline SSO (SHORT_STRING_TAG).
        // `typeof` doesn't distinguish between representations — both
        // are observed as "string" from user code.
        ValueTypeofTag::String
    } else if crate::value::is_js_handle(value) {
        // JS handle from V8 runtime — ask V8 whether it's a callable, otherwise default
        // to "object". Issue #258: pre-fix this always returned "object" even for
        // V8 functions; the registered callback now flips it to "function" when the
        // handle wraps a v8::Function.
        if crate::value::js_handle_is_function(value) {
            ValueTypeofTag::Function
        } else {
            ValueTypeofTag::Object
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
                ValueTypeofTag::Function
            } else {
                ValueTypeofTag::Object
            };
        }
        if crate::value::addr_class::is_above_handle_band(ptr as usize) {
            // Symbols: registered in SYMBOL_POINTERS (handles both gc_malloc'd
            // and Box-leaked symbols, which have no GcHeader).
            if crate::symbol::is_registered_symbol(ptr as usize) {
                ValueTypeofTag::Symbol
            } else if crate::date::is_date_cell_addr(ptr as usize) {
                // Date is a NaN-boxed pointer to an 8-byte `DateCell` (#2089).
                // `typeof aDate === "object"`. Check this BEFORE reading the
                // `type_tag` at offset 12 below — the cell is only 8 bytes, so
                // that read would fall off the end of the allocation.
                ValueTypeofTag::Object
            } else {
                // ClosureHeader has type_tag at offset 12 (after func_ptr:8 + capture_count:4)
                let type_tag =
                    unsafe { *(ptr.add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32) };
                if type_tag == crate::closure::CLOSURE_MAGIC {
                    ValueTypeofTag::Function
                } else if crate::object::is_class_object_ptr(ptr) {
                    // #1789: a class-expression VALUE is a heap object stamped
                    // with OBJECT_TYPE_CLASS — `typeof aClassObject ===
                    // "function"` (classes are callable in JS), matching the
                    // INT32 ClassRef case below.
                    ValueTypeofTag::Function
                } else {
                    ValueTypeofTag::Object
                }
            }
        } else {
            ValueTypeofTag::Object
        }
    } else if jsval.is_bigint() {
        ValueTypeofTag::BigInt
    } else if jsval.is_int32() {
        // Refs #618 / #420 followup: class refs share INT32_TAG storage
        // shape (codegen emits `INT32_TAG | class_id` as the value form
        // for `Expr::ClassRef`). Distinguish a class id from a real int32
        // by checking the vtable registry — registered class ids return
        // "function" per JS spec; everything else is "number".
        let raw = jsval.bits() & 0xFFFF_FFFF;
        let class_id = raw as u32;
        if crate::object::is_class_id_registered(class_id) {
            ValueTypeofTag::Function
        } else {
            ValueTypeofTag::Number
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
                return ValueTypeofTag::Object;
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
                    return ValueTypeofTag::Object;
                }
            }
        }
        // Regular f64 number
        ValueTypeofTag::Number
    }
}

/// Integer form of `typeof`, for comparisons against a compile-time literal.
/// Avoids materializing a cached heap string and then comparing its contents.
/// The numeric values are part of the codegen/runtime ABI; keep them in sync
/// with `TYPEOF_LITERAL_TAGS` in `perry-codegen/src/expr/compare.rs`.
#[no_mangle]
pub extern "C" fn js_value_typeof_tag(value: f64) -> u32 {
    classify_value_typeof(value) as u32
}

/// Return the typeof a value as a string
/// Takes an f64 that uses NaN-boxing to distinguish types.
/// Returns a pointer to a string: "undefined", "boolean", "number", "string", "object", "function"
///
/// Optimization: typeof only returns 8 possible strings, so each classified
/// result is mapped to a pre-allocated cached StringHeader pointer. The cache
/// is a registered GC root — see the `thread_local!` above.
#[no_mangle]
pub extern "C" fn js_value_typeof(value: f64) -> *mut StringHeader {
    match classify_value_typeof(value) {
        ValueTypeofTag::Undefined => get_cached(TYPEOF_UNDEFINED, "undefined"),
        ValueTypeofTag::Object => get_cached(TYPEOF_OBJECT, "object"),
        ValueTypeofTag::Boolean => get_cached(TYPEOF_BOOLEAN, "boolean"),
        ValueTypeofTag::Number => get_cached(TYPEOF_NUMBER, "number"),
        ValueTypeofTag::String => get_cached(TYPEOF_STRING, "string"),
        ValueTypeofTag::Function => get_cached(TYPEOF_FUNCTION, "function"),
        ValueTypeofTag::BigInt => get_cached(TYPEOF_BIGINT, "bigint"),
        ValueTypeofTag::Symbol => get_cached(TYPEOF_SYMBOL, "symbol"),
    }
}

#[cfg(test)]
mod rel_numeric_fastpath_tests {
    use super::*;

    const INT32: u64 = 0x7FFE_0000_0000_0000;
    const UNDEF: u64 = 0x7FFC_0000_0000_0001;
    const NULLV: u64 = 0x7FFC_0000_0000_0002;
    const FALSEV: u64 = 0x7FFC_0000_0000_0003;
    const TRUEV: u64 = 0x7FFC_0000_0000_0004;

    fn i32v(n: i32) -> f64 {
        f64::from_bits(INT32 | (n as u32 as u64))
    }
    fn is_true(v: f64) -> bool {
        v.to_bits() == TAG_TRUE_BITS
    }

    #[test]
    fn integer_typeof_classifier_covers_the_primitive_tag_families() {
        let cases = [
            (f64::from_bits(UNDEF), ValueTypeofTag::Undefined),
            (f64::from_bits(NULLV), ValueTypeofTag::Object),
            (f64::from_bits(TRUEV), ValueTypeofTag::Boolean),
            (42.5, ValueTypeofTag::Number),
            (i32v(-123), ValueTypeofTag::Number),
            (
                f64::from_bits(crate::value::SHORT_STRING_TAG | (1_u64 << 40) | b'x' as u64),
                ValueTypeofTag::String,
            ),
            (
                f64::from_bits(crate::value::BIGINT_TAG | 1),
                ValueTypeofTag::BigInt,
            ),
            (
                f64::from_bits(crate::value::POINTER_TAG | 42),
                ValueTypeofTag::Object,
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(classify_value_typeof(value), expected);
            assert_eq!(js_value_typeof_tag(value), expected as u32);
        }
    }

    /// The early-out accepts exactly the operands whose ToPrimitive/ToNumber
    /// result is fixed and which have nothing to root; everything else must
    /// fall through to the full abstract relational comparison.
    #[test]
    fn fast_path_accepts_numbers_and_simple_singletons() {
        assert!(rel_numeric_operand(1.5).is_some());
        assert!(rel_numeric_operand(-0.0).is_some());
        assert!(rel_numeric_operand(f64::INFINITY).is_some());
        assert!(rel_numeric_operand(f64::NEG_INFINITY).is_some());
        assert!(rel_numeric_operand(f64::NAN).is_some());
        assert_eq!(rel_numeric_operand(i32v(7)), Some(7.0));
        assert_eq!(rel_numeric_operand(i32v(-7)), Some(-7.0));
        assert!(rel_numeric_operand(f64::from_bits(UNDEF)).unwrap().is_nan());
        assert_eq!(rel_numeric_operand(f64::from_bits(NULLV)), Some(0.0));
        assert_eq!(rel_numeric_operand(f64::from_bits(FALSEV)), Some(0.0));
        assert_eq!(rel_numeric_operand(f64::from_bits(TRUEV)), Some(1.0));
    }

    #[test]
    fn simple_singleton_relational_comparisons_match_to_number() {
        let undefined = f64::from_bits(UNDEF);
        let null = f64::from_bits(NULLV);
        let false_value = f64::from_bits(FALSEV);
        let true_value = f64::from_bits(TRUEV);

        for got in [
            js_rel_lt(undefined, 1.0),
            js_rel_gt(undefined, 1.0),
            js_rel_le(undefined, 1.0),
            js_rel_ge(undefined, 1.0),
        ] {
            assert!(!is_true(got), "undefined must compare as NaN");
        }
        assert!(is_true(js_rel_lt(null, 1.0)));
        assert!(is_true(js_rel_ge(null, 0.0)));
        assert!(is_true(js_rel_le(false_value, 0.0)));
        assert!(is_true(js_rel_gt(true_value, 0.0)));
    }

    /// NaN makes all four operators false. This is the one way a naive `fcmp`
    /// early-out silently diverges from the spec, so pin it in both operand
    /// positions.
    #[test]
    fn nan_is_false_for_every_operator() {
        let n = f64::NAN;
        for (name, got) in [
            ("NaN <  x", js_rel_lt(n, 1.0)),
            ("NaN >  x", js_rel_gt(n, 1.0)),
            ("NaN <= x", js_rel_le(n, 1.0)),
            ("NaN >= x", js_rel_ge(n, 1.0)),
            ("x <  NaN", js_rel_lt(1.0, n)),
            ("x >  NaN", js_rel_gt(1.0, n)),
            ("x <= NaN", js_rel_le(1.0, n)),
            ("x >= NaN", js_rel_ge(1.0, n)),
        ] {
            assert!(!is_true(got), "{name} must be false");
        }
    }

    /// `-0 < 0` is false while `-0 <= 0` is true.
    #[test]
    fn signed_zero_matches_the_spec() {
        assert!(!is_true(js_rel_lt(-0.0, 0.0)));
        assert!(!is_true(js_rel_gt(-0.0, 0.0)));
        assert!(is_true(js_rel_le(-0.0, 0.0)));
        assert!(is_true(js_rel_ge(-0.0, 0.0)));
    }

    /// #9462: `TAG_HOLE` is a NaN bit pattern, so without its own arm it fell
    /// through every tag test to the "regular f64 number" tail and `typeof`
    /// answered "number". User code only ever observes an unset slot as
    /// `undefined`. This arm is defence in depth once the leak source in
    /// `js_array_get_f64` is fixed — it is what keeps the NEXT producer of a
    /// stray hole from re-opening the same symptom.
    #[test]
    fn an_array_hole_is_typeof_undefined() {
        let hole = f64::from_bits(crate::value::TAG_HOLE);
        assert_eq!(
            js_value_typeof_tag(hole),
            js_value_typeof_tag(f64::from_bits(crate::value::TAG_UNDEFINED)),
            "a hole classifies exactly as undefined"
        );
        // Controls: a real NaN is still a number, and so is an ordinary double.
        assert_ne!(js_value_typeof_tag(f64::NAN), js_value_typeof_tag(hole));
        assert_eq!(js_value_typeof_tag(f64::NAN), js_value_typeof_tag(1.5));
        // TDZ shares the 0x7FFC singleton namespace and must not be swept up.
        assert_ne!(
            js_value_typeof_tag(f64::from_bits(crate::value::TAG_TDZ)),
            js_value_typeof_tag(hole)
        );
    }

    #[test]
    fn ordinary_numeric_comparisons_are_unchanged() {
        assert!(is_true(js_rel_lt(1.0, 2.0)));
        assert!(!is_true(js_rel_lt(2.0, 1.0)));
        assert!(is_true(js_rel_ge(2.0, 2.0)));
        assert!(is_true(js_rel_le(2.0, 2.0)));
        assert!(is_true(js_rel_gt(f64::INFINITY, 1e308)));
        assert!(is_true(js_rel_lt(f64::NEG_INFINITY, 0.0)));
        assert!(is_true(js_rel_lt(i32v(3), i32v(9))));
        assert!(!is_true(js_rel_gt(i32v(3), i32v(9))));
        assert!(is_true(js_rel_lt(i32v(3), 3.5)));
        assert!(is_true(js_rel_gt(3.5, i32v(3))));
    }
}
