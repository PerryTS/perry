//! Strict/loose equality, SameValueZero, relational comparison, and the
//! dynamic-string-equals shim used by codegen for mixed boxed/raw pointers.

use super::*;

/// True when `bits` decode to a JS Number whose value is NaN — including the
/// *negative* qNaN (top16 `0xFFF8`) that libm `acosh`/`atanh`/`log`/`sqrt`
/// return for out-of-domain inputs, not just the positive canonical `0x7FF8`.
/// `is_number()` already excludes every Perry tag, so this never mis-fires on a
/// NaN-boxed pointer/string/etc.
#[inline(always)]
fn is_number_nan(bits: u64) -> bool {
    JSValue::from_bits(bits).is_number() && f64::from_bits(bits).is_nan()
}

/// Compare two NaN-boxed f64 values for equality (JavaScript `===` semantics).
/// SameValueZero algorithm (ECMA-262) — used by `Array.prototype.includes`,
/// `Map` / `Set` keys. Same as strict equality except `NaN` is considered
/// equal to itself. Returns 1 if equal, 0 if not.
#[no_mangle]
pub extern "C" fn js_jsvalue_same_value_zero(a: f64, b: f64) -> i32 {
    // NaN-equals-NaN under SameValueZero (the only difference from ===).
    let abits = a.to_bits();
    let bbits = b.to_bits();
    if is_number_nan(abits) && is_number_nan(bbits) {
        return 1;
    }
    js_jsvalue_equals(a, b)
}

/// Module-level object/array variables store their heap pointer RAW in an
/// f64 slot (top16 == 0 — see the NaN-boxing notes in CLAUDE.md), while the
/// same object circulates POINTER_TAG'd everywhere else. An identity compare
/// between the two representations must see the same bits, or
/// `var a = 2; f(){ a = o; } f(); a === o` is permanently false (#3576
/// probe family). Tagging is safe for genuine numbers: only subnormals have
/// top16 == 0 with bits >= 0x10000, identical subnormals already hit the
/// same-bits fast path, and distinct subnormals are unequal either way.
#[inline(always)]
pub(crate) fn normalize_raw_object_bits(bits: u64) -> u64 {
    if bits >> 48 == 0 && bits >= 0x10000 {
        POINTER_TAG | bits
    } else {
        bits
    }
}

/// Handles string comparison by comparing actual string contents.
/// Handles BigInt comparison by comparing underlying bigint values (not pointers).
/// Returns 1 if equal, 0 if not.
#[no_mangle]
pub extern "C" fn js_jsvalue_equals(a: f64, b: f64) -> i32 {
    let abits = normalize_raw_object_bits(a.to_bits());
    let bbits = normalize_raw_object_bits(b.to_bits());
    let a = f64::from_bits(abits);
    let b = f64::from_bits(bbits);

    // NaN === NaN is false in JS (strict equality follows IEEE 754).
    // Raw IEEE NaN has top16 = 0x7FF8 (the canonical quiet-NaN). NaN-
    // boxed tagged values use top16 0x7FFA–0x7FFF and never collide.
    // Must come BEFORE the abits==bbits fast path: `[1, NaN, 3].indexOf(NaN)`
    // routes through this helper, both sides decode to f64::NAN (same
    // bit pattern), and pre-fix the fast path returned 1 (wrongly equal)
    // so indexOf reported index 1 instead of -1.
    // Any IEEE NaN — positive 0x7FF8 OR the negative 0xFFF8 payload that libm
    // returns from out-of-domain acosh/atanh/log/sqrt — is unequal to itself
    // and to every other value under ===. Must precede the bit-pattern fast
    // path: two identical negative-NaN bit patterns would otherwise compare
    // equal (`Math.acosh(0) === Math.acosh(0)` wrongly true → `x !== x` false).
    if is_number_nan(abits) || is_number_nan(bbits) {
        return 0;
    }

    // Fast path: same bit pattern → equal (same number, same pointer, same boolean, etc.)
    if abits == bbits {
        return 1;
    }

    let a_val = JSValue::from_bits(abits);
    let b_val = JSValue::from_bits(bbits);

    // BigInt comparison: compare by value, not by pointer
    // Two BigInt allocations with the same value must be equal under ===
    if a_val.is_bigint() && b_val.is_bigint() {
        let a_ptr = a_val.as_bigint_ptr();
        let b_ptr = b_val.as_bigint_ptr();
        return crate::bigint::js_bigint_eq(a_ptr, b_ptr);
    }

    // String comparison: compare by content, not by pointer. Must
    // accept both STRING_TAG heap strings and SHORT_STRING_TAG
    // inline SSO values, in any combination.
    if a_val.is_any_string() && b_val.is_any_string() {
        // Fast path: both SSO → identical bits ↔ identical content,
        // because SSO encoding is canonical (same bytes + same
        // length ⇒ same bit pattern).
        if a_val.is_short_string() && b_val.is_short_string() {
            return if abits == bbits { 1 } else { 0 };
        }
        // Decode each side to a (ptr, len) view via a stack scratch
        // buffer for the SSO side; compare by bytes.
        let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let a_view = crate::string::str_bytes_from_jsvalue(a, &mut a_scratch);
        let b_view = crate::string::str_bytes_from_jsvalue(b, &mut b_scratch);
        if let (Some((a_ptr, a_len)), Some((b_ptr, b_len))) = (a_view, b_view) {
            if a_len != b_len {
                return 0;
            }
            if a_len == 0 {
                return 1;
            }
            unsafe {
                let a_slice = std::slice::from_raw_parts(a_ptr, a_len as usize);
                let b_slice = std::slice::from_raw_parts(b_ptr, b_len as usize);
                return if a_slice == b_slice { 1 } else { 0 };
            }
        }
        return 0;
    }

    // Helper: check if bits represent a plain IEEE 754 number (not a NaN-boxed tagged value).
    // NaN-boxing uses tags 0x7FF8-0x7FFF in the upper 16 bits. Regular numbers (positive,
    // negative, zero, infinities) have upper16 outside this range. Negative numbers have
    // sign bit set (upper16 >= 0x8000), so the old check `bits < 0x7FF8...` missed them.
    #[inline(always)]
    fn is_plain_number(bits: u64) -> bool {
        let tag = bits >> 48;
        !(0x7FF8..=0x7FFF).contains(&tag)
    }

    // INT32 comparison: one or both operands may be NaN-boxed INT32 (0x7FFE tag).
    // Convert INT32 to f64 for numeric comparison (e.g., INT32(5) === 5.0 should be true).
    // This mirrors the conversion in js_jsvalue_compare.
    if a_val.is_int32() || b_val.is_int32() {
        let af = if a_val.is_int32() {
            a_val.as_int32() as f64
        } else if is_plain_number(abits) {
            a
        } else {
            return 0;
        }; // non-numeric type → not equal
        let bf = if b_val.is_int32() {
            b_val.as_int32() as f64
        } else if is_plain_number(bbits) {
            b
        } else {
            return 0;
        }; // non-numeric type → not equal
        return if af == bf { 1 } else { 0 };
    }

    // Regular f64 numbers (not NaN-boxed): use IEEE 754 equality
    // This handles -0.0 === 0.0 correctly (both are equal per IEEE 754)
    // Also correctly handles NaN !== NaN (IEEE 754 NaN comparison returns false)
    if is_plain_number(abits) && is_plain_number(bbits) {
        return if a == b { 1 } else { 0 };
    }

    // Array-grow / GC-evacuation forwarding identity. After `js_array_grow`
    // (or a copying-GC evacuation) relocates an object, an aliased reference —
    // e.g. the result of `Object(arr)`, or a `var b = arr` captured before the
    // grow — can still hold the stale pre-grow pointer while another reference
    // holds the relocated head. They are the SAME object, so `===` must be
    // true. Both operands are POINTER_TAG here (numbers/strings/bigint/int32
    // returned above); resolve each through its `GC_FLAG_FORWARDED` chain (the
    // same follow `clean_arr_ptr` performs) and compare the resolved addresses.
    // Gated on the forwarding flag: distinct, non-forwarded objects keep their
    // own address and fall straight through to `0`, so the common case is a
    // couple of cheap GcHeader flag loads. (test262 Object S15.2.1.1_A2_T10 /
    // S15.2.2.1_A2_T3: `Object(arr) === arr`.)
    if a_val.is_pointer() && b_val.is_pointer() {
        let ra = resolve_forwarding((abits & crate::value::POINTER_MASK) as usize);
        let rb = resolve_forwarding((bbits & crate::value::POINTER_MASK) as usize);
        if ra != 0 && ra == rb {
            return 1;
        }
    }

    // Different types or different NaN-boxed values → not equal
    0
}

/// Follow an object's `GC_FLAG_FORWARDED` chain to its current heap address,
/// mirroring `clean_arr_ptr`'s forwarding walk. Returns `addr` unchanged when
/// it is not a forwarded heap pointer (handle/proxy-band registry ids, small
/// slab allocations, out-of-heap addresses, or live non-forwarded objects), so
/// callers can compare resolved addresses for object identity. `try_read_gc_-
/// header` performs the band/heap classification (never dereferencing a
/// non-heap id). Depth-capped to defend against corrupted GC cycles.
fn resolve_forwarding(mut addr: usize) -> usize {
    unsafe {
        let mut steps = 0u32;
        while let Some(header) = crate::value::addr_class::try_read_gc_header(addr) {
            if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
                break;
            }
            let new_user = crate::gc::forwarding_address(header) as usize;
            if new_user == 0 || new_user == addr {
                break;
            }
            addr = new_user;
            steps += 1;
            if steps > 64 {
                break;
            }
        }
    }
    addr
}

/// JS Abstract Equality Comparison (==).
/// Implements the type coercion rules from ECMA-262 §7.2.14:
/// - null == undefined → true
/// - string == number → ToNumber(string) == number
/// - boolean == anything → ToNumber(boolean) == anything
/// - Same type → strict equality
#[no_mangle]
pub extern "C" fn js_jsvalue_loose_equals(a: f64, b: f64) -> i32 {
    let abits = normalize_raw_object_bits(a.to_bits());
    let bbits = normalize_raw_object_bits(b.to_bits());
    let a = f64::from_bits(abits);
    let b = f64::from_bits(bbits);

    // Fast path: same bit pattern
    if abits == bbits {
        return 1;
    }

    let a_val = JSValue::from_bits(abits);
    let b_val = JSValue::from_bits(bbits);

    // null == undefined (and vice versa)
    let a_null = a_val.is_null() || a_val.is_undefined();
    let b_null = b_val.is_null() || b_val.is_undefined();
    if a_null && b_null {
        return 1;
    }
    // null/undefined != anything else
    if a_null || b_null {
        return 0;
    }

    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(a) {
        return js_jsvalue_loose_equals(payload, b);
    }
    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(b) {
        return js_jsvalue_loose_equals(a, payload);
    }

    #[inline(always)]
    fn is_plain_number(bits: u64) -> bool {
        let tag = bits >> 48;
        !(0x7FF8..=0x7FFF).contains(&tag)
    }

    // Helper: convert a JSValue to f64 for numeric comparison
    fn to_number(val: &JSValue, bits: u64, raw: f64) -> Option<f64> {
        if val.is_int32() {
            Some(val.as_int32() as f64)
        } else if is_plain_number(bits) {
            Some(raw)
        } else if val.is_bool() {
            Some(if val.as_bool() { 1.0 } else { 0.0 })
        } else if val.is_any_string() {
            // Accept both STRING_TAG heap strings and SHORT_STRING_TAG
            // inline SSO values via str_bytes_from_jsvalue (decodes SSO
            // into the scratch buffer). Using is_string() alone here
            // misses SSO strings, so `"5" == 5` returned 0 for short
            // string operands.
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(raw, &mut scratch) else {
                return Some(f64::NAN);
            };
            let s = unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len as usize))
            };
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Some(0.0)
            } else {
                trimmed.parse::<f64>().ok()
            }
        } else {
            None
        }
    }

    // If both are same type, delegate to strict equals
    let a_is_num = a_val.is_int32() || is_plain_number(abits);
    let b_is_num = b_val.is_int32() || is_plain_number(bbits);
    // Must accept SHORT_STRING_TAG (SSO) operands too, not just
    // STRING_TAG heap strings — otherwise short strings fall through
    // every arm below to `return 0`, so `"ab" == "ab"` (and loose
    // assert.equal on SSO values) wrongly reported not-equal. Mirrors
    // the strict-equality path above, which already uses is_any_string.
    let a_is_str = a_val.is_any_string();
    let b_is_str = b_val.is_any_string();
    let a_is_bool = a_val.is_bool();
    let b_is_bool = b_val.is_bool();

    // Both strings: compare by content, handling SSO on either side via
    // a stack scratch buffer (js_string_equals only accepts heap ptrs).
    if a_is_str && b_is_str {
        let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let a_view = crate::string::str_bytes_from_jsvalue(a, &mut a_scratch);
        let b_view = crate::string::str_bytes_from_jsvalue(b, &mut b_scratch);
        if let (Some((a_ptr, a_len)), Some((b_ptr, b_len))) = (a_view, b_view) {
            if a_len != b_len {
                return 0;
            }
            if a_len == 0 {
                return 1;
            }
            unsafe {
                let a_slice = std::slice::from_raw_parts(a_ptr, a_len as usize);
                let b_slice = std::slice::from_raw_parts(b_ptr, b_len as usize);
                return if a_slice == b_slice { 1 } else { 0 };
            }
        }
        return 0;
    }

    // Both numbers: numeric comparison
    if a_is_num && b_is_num {
        let af = if a_val.is_int32() {
            a_val.as_int32() as f64
        } else {
            a
        };
        let bf = if b_val.is_int32() {
            b_val.as_int32() as f64
        } else {
            b
        };
        return if af == bf { 1 } else { 0 };
    }

    // Boolean == anything: convert boolean to number, then recurse
    if a_is_bool {
        let a_num = if a_val.as_bool() { 1.0 } else { 0.0 };
        return js_jsvalue_loose_equals(a_num, b);
    }
    if b_is_bool {
        let b_num = if b_val.as_bool() { 1.0 } else { 0.0 };
        return js_jsvalue_loose_equals(a, b_num);
    }

    // String == Number: convert string to number
    if a_is_str && b_is_num {
        if let Some(af) = to_number(&a_val, abits, a) {
            let bf = if b_val.is_int32() {
                b_val.as_int32() as f64
            } else {
                b
            };
            return if af == bf { 1 } else { 0 };
        }
        return 0;
    }
    if a_is_num && b_is_str {
        if let Some(bf) = to_number(&b_val, bbits, b) {
            let af = if a_val.is_int32() {
                a_val.as_int32() as f64
            } else {
                a
            };
            return if af == bf { 1 } else { 0 };
        }
        return 0;
    }

    // BigInt comparisons
    if a_val.is_bigint() && b_val.is_bigint() {
        let a_ptr = a_val.as_bigint_ptr();
        let b_ptr = b_val.as_bigint_ptr();
        return crate::bigint::js_bigint_eq(a_ptr, b_ptr);
    }

    0
}

/// Compare two JSValues for relational ordering (< <= > >=).
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
/// Handles BigInt, String, Number, and INT32 types.
#[no_mangle]
pub extern "C" fn js_jsvalue_compare(a: f64, b: f64) -> i32 {
    let abits = a.to_bits();
    let bbits = b.to_bits();

    let a_val = JSValue::from_bits(abits);
    let b_val = JSValue::from_bits(bbits);

    // BigInt comparison
    if a_val.is_bigint() && b_val.is_bigint() {
        let a_ptr = a_val.as_bigint_ptr();
        let b_ptr = b_val.as_bigint_ptr();
        return crate::bigint::js_bigint_cmp(a_ptr, b_ptr);
    }

    // String comparison (lexicographic). Accepts SSO in either
    // operand — decode via `str_bytes_from_jsvalue` into stack
    // scratch, then compare slices.
    if a_val.is_any_string() && b_val.is_any_string() {
        let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let a_view = crate::string::str_bytes_from_jsvalue(a, &mut a_scratch);
        let b_view = crate::string::str_bytes_from_jsvalue(b, &mut b_scratch);
        if let (Some((a_ptr, a_len)), Some((b_ptr, b_len))) = (a_view, b_view) {
            if (!a_ptr.is_null() || a_len == 0) && (!b_ptr.is_null() || b_len == 0) {
                unsafe {
                    let a_bytes = std::slice::from_raw_parts(a_ptr, a_len as usize);
                    let b_bytes = std::slice::from_raw_parts(b_ptr, b_len as usize);
                    // ECMAScript compares strings by UTF-16 code unit, not raw
                    // UTF-8 byte / code-point order (differs for astral chars).
                    return match crate::string::utf16_cmp_bytes(a_bytes, b_bytes) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    };
                }
            }
        }
    }

    // INT32 comparison
    if a_val.is_int32() && b_val.is_int32() {
        let ai = a_val.as_int32();
        let bi = b_val.as_int32();
        return if ai < bi {
            -1
        } else if ai > bi {
            1
        } else {
            0
        };
    }

    // Convert to f64 for numeric comparison (handles Number, INT32 mixed with Number, etc.)
    // Return 2 (sentinel) for undefined/null — makes all comparisons false
    // Convert to f64 — use tag check that correctly handles negative numbers
    // (sign bit set → upper16 >= 0x8000, which is > 0x7FFF, not a NaN-box tag)
    let a_tag = abits >> 48;
    let b_tag = bbits >> 48;
    let af = if a_val.is_int32() {
        a_val.as_int32() as f64
    } else if a_val.is_bigint() {
        crate::bigint::js_bigint_to_f64(a_val.as_bigint_ptr())
    } else if !(0x7FF8..=0x7FFF).contains(&a_tag) {
        a
    } else {
        return 2;
    }; // undefined/null/boolean → incomparable sentinel
    let bf = if b_val.is_int32() {
        b_val.as_int32() as f64
    } else if b_val.is_bigint() {
        crate::bigint::js_bigint_to_f64(b_val.as_bigint_ptr())
    } else if !(0x7FF8..=0x7FFF).contains(&b_tag) {
        b
    } else {
        return 2;
    }; // undefined/null/boolean → incomparable sentinel

    if af < bf {
        -1
    } else if af > bf {
        1
    } else {
        0
    }
}

/// Dynamic string comparison that handles both NaN-boxed strings and raw pointer bitcasts.
/// This is needed when comparing a PropertyGet result (NaN-boxed) with a string literal (raw bitcast).
/// Returns 1 if equal, 0 if not.
#[no_mangle]
pub extern "C" fn js_dynamic_string_equals(a: f64, b: f64) -> i32 {
    // Extract string pointers from both values, handling both representations
    let a_ptr = extract_string_ptr(a);
    let b_ptr = extract_string_ptr(b);

    if a_ptr.is_null() && b_ptr.is_null() {
        return 1;
    }
    if a_ptr.is_null() || b_ptr.is_null() {
        return 0;
    }

    if crate::string::js_string_equals(a_ptr, b_ptr) != 0 {
        1
    } else {
        0
    }
}

/// Extract a string pointer from an f64 value that might be:
/// - NaN-boxed with STRING_TAG
/// - NaN-boxed with POINTER_TAG (for strings stored as generic pointers)
/// - Raw pointer bits (from bitcast)
fn extract_string_ptr(value: f64) -> *const crate::StringHeader {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    // Check for STRING_TAG first (e.g., from PropertyGet)
    if jsval.is_string() {
        return jsval.as_string_ptr();
    }

    // Check for POINTER_TAG (generic pointer that might be a string)
    if jsval.is_pointer() {
        return jsval.as_pointer::<crate::StringHeader>();
    }

    // Assume raw pointer bits (from bitcast of string literal)
    // In a 64-bit system, valid heap pointers are typically in the range
    // 0x0000_0000_0000_0000 to 0x0000_7FFF_FFFF_FFFF
    // Check if it looks like a valid pointer (not NaN, not a small number)
    if bits > 0x1000 && bits < 0x0001_0000_0000_0000 {
        return bits as *const crate::StringHeader;
    }

    std::ptr::null()
}

#[cfg(test)]
mod loose_eq_sso_tests {
    //! Regression: `js_jsvalue_loose_equals` must treat SHORT_STRING_TAG
    //! (SSO-inlined) operands as strings. Before the fix it used
    //! `is_string()` (heap-only), so two short strings fell through every
    //! arm to `return 0` — e.g. loose `assert.equal` on JSON-parsed short
    //! strings wrongly reported not-equal.
    use super::*;

    fn sso(bytes: &[u8]) -> f64 {
        f64::from_bits(JSValue::short_string_unchecked(bytes).bits())
    }

    fn heap(bytes: &[u8]) -> f64 {
        let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(ptr as i64)
    }

    #[test]
    fn sso_eq_sso_same_content() {
        assert_eq!(js_jsvalue_loose_equals(sso(b"ab"), sso(b"ab")), 1);
    }

    #[test]
    fn sso_neq_sso_different_content() {
        assert_eq!(js_jsvalue_loose_equals(sso(b"ab"), sso(b"ac")), 0);
        assert_eq!(js_jsvalue_loose_equals(sso(b"ab"), sso(b"abc")), 0);
    }

    #[test]
    fn sso_eq_heap_same_content() {
        assert_eq!(js_jsvalue_loose_equals(sso(b"ab"), heap(b"ab")), 1);
        assert_eq!(js_jsvalue_loose_equals(heap(b"ab"), sso(b"ab")), 1);
    }

    #[test]
    fn sso_string_loose_eq_number() {
        // ToNumber("5") == 5
        assert_eq!(js_jsvalue_loose_equals(sso(b"5"), 5.0), 1);
        assert_eq!(js_jsvalue_loose_equals(5.0, sso(b"5")), 1);
        assert_eq!(js_jsvalue_loose_equals(sso(b"5"), 6.0), 0);
    }

    #[test]
    fn empty_sso_loose_eq_zero() {
        // ToNumber("") == 0
        assert_eq!(js_jsvalue_loose_equals(sso(b""), 0.0), 1);
        assert_eq!(js_jsvalue_loose_equals(sso(b""), sso(b"")), 1);
    }
}
