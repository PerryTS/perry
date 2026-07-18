//! BigInt runtime support for Perry
//!
//! Provides 1024-bit integer arithmetic for cryptocurrency operations.
//! Uses 16 x u64 limbs in little-endian order.
//! 1024 bits is needed because secp256k1 (used by ethers.js/noble-curves)
//! has a ~256-bit prime, and intermediate products (a*b before mod reduction)
//! can be ~512 bits. With 512-bit two's complement, bit 511 is the sign bit,
//! causing false negatives. 1024 bits keeps the sign bit at bit 1023.

/// Number of 64-bit limbs in a BigInt (1024 bits total)
pub const BIGINT_LIMBS: usize = 16;
/// Total number of bits
const BIGINT_BITS: usize = BIGINT_LIMBS * 64;

const ZERO_LIMBS: [u64; BIGINT_LIMBS] = [0; BIGINT_LIMBS];
const DIVISION_BY_ZERO_MESSAGE: &[u8] = b"Division by zero";

/// Throw a `TypeError` with the given message (matches Node's BigInt coercion
/// and operator errors). Never returns.
#[cold]
fn throw_bigint_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Throw a `RangeError` with the given message. Never returns.
#[cold]
fn throw_bigint_range_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Throw a `SyntaxError` with the given message. Never returns.
#[cold]
fn throw_bigint_syntax_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// V8 caps BigInt precision and throws `RangeError: Maximum BigInt size
/// exceeded` past it. Perry's representation is a fixed 1024-bit two's
/// complement value (issue #6073); rather than silently wrapping past
/// ±2^1023 like an `i1024`, arithmetic that would overflow throws this same
/// `RangeError` — a loud, catchable failure until true arbitrary precision
/// lands. Its threshold is lower than V8's, but the observable shape matches.
const MAX_BIGINT_SIZE_MESSAGE: &str = "Maximum BigInt size exceeded";

/// Throw the "Maximum BigInt size exceeded" `RangeError` (#6073). Never returns.
#[cold]
#[inline(never)]
fn throw_bigint_overflow() -> ! {
    throw_bigint_range_error(MAX_BIGINT_SIZE_MESSAGE);
}

/// A signed 1024-bit BigInt spans `[-2^1023, 2^1023 - 1]`. Given a
/// non-negative magnitude in little-endian limbs (`mag.len() >= BIGINT_LIMBS`)
/// and the sign the value will carry, report whether it is representable
/// without wrapping. Any set bit at or above 2^1024 (a nonzero limb past the
/// low 16) never fits; a magnitude in `[2^1023, 2^1024)` fits only when it is
/// exactly `2^1023` *and* negative — the single `-2^1023` endpoint.
fn magnitude_fits_1024(mag: &[u64], negative: bool) -> bool {
    debug_assert!(mag.len() >= BIGINT_LIMBS);
    if mag[BIGINT_LIMBS..].iter().any(|&l| l != 0) {
        return false;
    }
    let top = mag[BIGINT_LIMBS - 1];
    if top >> 63 == 0 {
        // magnitude < 2^1023 — representable with either sign.
        return true;
    }
    // magnitude >= 2^1023 — only exactly -2^1023 is representable.
    negative && top == 1u64 << 63 && mag[..BIGINT_LIMBS - 1].iter().all(|&l| l == 0)
}

/// Build a 1024-bit two's-complement limb array from a finite-integer f64.
/// Node converts any finite integer Number to a BigInt of the same value,
/// not just those that fit in i64. Caller must have already verified the
/// value is finite and has no fractional part.
fn limbs_from_integer_f64(value: f64) -> [u64; BIGINT_LIMBS] {
    if value == 0.0 {
        return ZERO_LIMBS;
    }
    let negative = value < 0.0;
    let mut mag = value.abs();
    // Decompose the magnitude into base-2^64 limbs, low limb first.
    let mut limbs = ZERO_LIMBS;
    let two_pow_64 = 18446744073709551616.0f64; // 2^64
    let mut i = 0;
    while mag >= 1.0 && i < BIGINT_LIMBS {
        let limb = mag % two_pow_64;
        limbs[i] = limb as u64;
        mag = (mag / two_pow_64).floor();
        i += 1;
    }
    // Overflow (#6073): either the magnitude needed more than 16 limbs (`mag`
    // is still >= 1 after the loop) or it landed in [2^1023, 2^1024) with a
    // sign that cannot represent it. `BigInt(1e309)` and friends throw instead
    // of silently truncating to the low 1024 bits.
    if mag >= 1.0 || !magnitude_fits_1024(&limbs, negative) {
        throw_bigint_overflow();
    }
    if negative {
        negate_limbs(&limbs)
    } else {
        limbs
    }
}

/// Decode a 1024-bit two's-complement value into a host i64 if it fits.
/// Layout: positive small → all upper limbs zero AND limb[0] high bit clear;
/// negative small → all upper limbs `u64::MAX` AND limb[0] high bit set.
/// Returns None for anything that needs more than 64 bits to represent.
#[inline(always)]
fn fits_in_i64(limbs: &[u64; BIGINT_LIMBS]) -> Option<i64> {
    let lo = limbs[0];
    let hi_bit = lo >> 63;
    let expected_fill = if hi_bit == 0 { 0u64 } else { u64::MAX };
    for &l in &limbs[1..] {
        if l != expected_fill {
            return None;
        }
    }
    Some(lo as i64)
}

/// Write a host i64 into a 1024-bit two's-complement limb array,
/// sign-extending the upper limbs.
#[inline(always)]
fn write_i64(value: i64, limbs: &mut [u64; BIGINT_LIMBS]) {
    let fill = if value < 0 { u64::MAX } else { 0u64 };
    *limbs = [fill; BIGINT_LIMBS];
    limbs[0] = value as u64;
}

/// Write a host i128 into a 1024-bit two's-complement limb array,
/// sign-extending the upper 14 limbs.
#[inline(always)]
fn write_i128(value: i128, limbs: &mut [u64; BIGINT_LIMBS]) {
    let fill = if value < 0 { u64::MAX } else { 0u64 };
    *limbs = [fill; BIGINT_LIMBS];
    let bits = value as u128;
    limbs[0] = bits as u64;
    limbs[1] = (bits >> 64) as u64;
}

/// BigInt is stored as a heap-allocated 1024-bit integer
/// Layout: 128 bytes (16 x u64)
#[repr(C)]
pub struct BigIntHeader {
    /// The 1024-bit value stored as 16 x u64 in little-endian order
    pub limbs: [u64; BIGINT_LIMBS],
}

/// Allocate a BigInt from the arena (bump-pointer, no per-object Vec/HashSet tracking).
///
/// Switching from gc_malloc to arena_alloc_gc eliminates the dominant per-call
/// overhead: system malloc (~30 ns) + MALLOC_STATE Vec push (~10 ns) +
/// HashSet insert (~30 ns) = ~70 ns → reduced to ~20 ns bump-pointer.
/// Arena objects are discovered by linear block walking at GC time; the mark
/// phase already handles GC_TYPE_BIGINT (no child references to trace).
#[inline]
fn bigint_alloc() -> *mut BigIntHeader {
    let raw = crate::arena::arena_alloc_gc(
        std::mem::size_of::<BigIntHeader>(),
        std::mem::align_of::<BigIntHeader>(),
        crate::gc::GC_TYPE_BIGINT,
    );
    raw as *mut BigIntHeader
}

#[inline]
pub(crate) fn bigint_alloc_with_limbs(limbs: [u64; BIGINT_LIMBS]) -> *mut BigIntHeader {
    let ptr = bigint_alloc();
    unsafe {
        (*ptr).limbs = limbs;
    }
    ptr
}

#[inline(always)]
fn bigint_limbs_or_zero(a: *const BigIntHeader) -> [u64; BIGINT_LIMBS] {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        ZERO_LIMBS
    } else {
        unsafe { (*a).limbs }
    }
}

/// Strip NaN-boxing tags from a BigInt pointer (defensive guard).
/// Returns null if the value is not a valid bigint pointer.
#[inline(always)]
pub fn clean_bigint_ptr(p: *const BigIntHeader) -> *const BigIntHeader {
    let bits = p as u64;
    let top16 = bits >> 48;
    if top16 >= 0x7FF8 {
        // NaN-boxed value — extract lower 48 bits
        let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const BigIntHeader;
        if (raw as usize) < 0x10000 {
            return std::ptr::null();
        }
        raw
    } else if bits < 0x10000 {
        std::ptr::null()
    } else if top16 != 0 {
        // Non-zero upper 16 bits but not NaN-boxed — not a valid heap pointer
        // (e.g., raw f64 bits from js_nanbox_get_bigint fallback)
        std::ptr::null()
    } else {
        p
    }
}

#[inline(always)]
pub fn clean_bigint_ptr_mut(p: *mut BigIntHeader) -> *mut BigIntHeader {
    clean_bigint_ptr(p as *const BigIntHeader) as *mut BigIntHeader
}

#[cold]
fn throw_bigint_division_by_zero() -> ! {
    let msg = crate::string::js_string_from_bytes(
        DIVISION_BY_ZERO_MESSAGE.as_ptr(),
        DIVISION_BY_ZERO_MESSAGE.len() as u32,
    );
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Create a BigInt from a u64 value
#[no_mangle]
pub extern "C" fn js_bigint_from_u64(value: u64) -> *mut BigIntHeader {
    let mut limbs = ZERO_LIMBS;
    limbs[0] = value;
    bigint_alloc_with_limbs(limbs)
}

/// Create a BigInt from a signed i64 value
#[no_mangle]
pub extern "C" fn js_bigint_from_i64(value: i64) -> *mut BigIntHeader {
    let fill = if value < 0 { u64::MAX } else { 0u64 };
    let mut limbs = [fill; BIGINT_LIMBS];
    limbs[0] = value as u64;
    bigint_alloc_with_limbs(limbs)
}

/// Create a BigInt from a compiler-owned signed 128-bit temporary, passed as
/// raw low/high 64-bit words so generated LLVM can keep small BigInt literal
/// arithmetic native until the JS-visible BigInt object boundary.
#[no_mangle]
pub extern "C" fn js_bigint_from_i128_parts(lo: u64, hi: i64) -> *mut BigIntHeader {
    let bits = ((hi as u64 as u128) << 64) | (lo as u128);
    let value = bits as i128;
    let mut limbs = ZERO_LIMBS;
    write_i128(value, &mut limbs);
    bigint_alloc_with_limbs(limbs)
}

#[used]
static KEEP_JS_BIGINT_FROM_I128_PARTS: extern "C" fn(u64, i64) -> *mut BigIntHeader =
    js_bigint_from_i128_parts;

/// Create a BigInt from a JS value (the `BigInt(value)` coercion).
///
/// Matches Node/ECMAScript `ToBigInt` semantics (#2754, #2907):
///   - `undefined` / `null`  → `TypeError`
///   - `true` / `false`      → `1n` / `0n`
///   - existing BigInt       → pass-through
///   - Number (incl. int32)  → must be a finite integer, else `RangeError`;
///                             the full integer value is preserved (not
///                             truncated/saturated to i64)
///   - string                → parsed; invalid syntax → `SyntaxError`
///
/// The argument arrives NaN-boxed, so a real Number is a plain f64 while
/// booleans/null/undefined/strings/bigints carry Perry tag bits.
#[no_mangle]
pub extern "C" fn js_bigint_from_f64(value: f64) -> *mut BigIntHeader {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());

    // If already a BigInt (NaN-boxed), just return the pointer
    if jsval.is_bigint() {
        return jsval.as_bigint_ptr() as *mut BigIntHeader;
    }

    // Boolean: BigInt(true) === 1n, BigInt(false) === 0n.
    if jsval.is_bool() {
        return js_bigint_from_i64(if jsval.as_bool() { 1 } else { 0 });
    }

    // If it's an INT32 (NaN-boxed i32), extract and convert
    if jsval.is_int32() {
        let int_value = jsval.as_int32() as i64;
        return js_bigint_from_i64(int_value);
    }

    // If it's a string, parse as BigInt (e.g., BigInt("1000000")).
    // #1781: accept inline SSO short strings too — `BigInt("123")` is a
    // 3-byte SSO value that `is_string()` (STRING_TAG-only) would reject,
    // dropping it to the `value as i64` fallback (NaN → 0n). Route through
    // the unified decoder, which materializes SSO bytes onto the heap.
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value)
            as *const crate::string::StringHeader;
        if !ptr.is_null() {
            unsafe {
                let len = (*ptr).byte_len;
                let data =
                    (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let result = js_bigint_from_string(data, len);
                return result;
            }
        }
        // Empty / unmaterializable string → 0n, matching `BigInt("")`.
        return js_bigint_from_i64(0);
    }

    // undefined / null are not convertible — Node throws a TypeError.
    if jsval.is_undefined() {
        throw_bigint_type_error("Cannot convert undefined to a BigInt");
    }
    if jsval.is_null() {
        throw_bigint_type_error("Cannot convert null to a BigInt");
    }

    // Object / Symbol pointer. ECMAScript ToBigInt step 1 is
    // `ToPrimitive(value, number)`, so `valueOf` / `toString` /
    // `@@toPrimitive` must run (and propagate their exceptions) *before* the
    // integer check — `BigInt({valueOf(){throw}})` rethrows, and
    // `BigInt({valueOf(){return 2n}})` is 2n. Previously a non-string,
    // non-bigint pointer fell through to the Number branch below, where its
    // NaN-boxed bits read as NaN and threw a (premature) RangeError. A Symbol
    // has no primitive conversion → TypeError. Mirrors `js_number_coerce`.
    if jsval.is_pointer() {
        let ptr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
        if crate::symbol::is_registered_symbol(ptr) {
            throw_bigint_type_error("Cannot convert a Symbol value to a BigInt");
        }
        // `@@toPrimitive("number")` first.
        let primitive = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if primitive.to_bits() != value.to_bits() {
            return js_bigint_from_f64(primitive);
        }
        // OrdinaryToPrimitive(O, "number"): valueOf then toString.
        match unsafe { crate::value::ordinary_to_primitive_number_for_add(value) } {
            crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => {
                if p.to_bits() != value.to_bits() {
                    return js_bigint_from_f64(p);
                }
            }
            crate::value::OrdinaryToPrimitiveOutcome::TypeError
            | crate::value::OrdinaryToPrimitiveOutcome::DefaultString => {}
        }
        // Fall back to string coercion (e.g. an array → join → parse:
        // `BigInt([5])` === 5n, `BigInt([])` === 0n).
        let str_ptr = crate::value::js_jsvalue_to_string(value);
        if !str_ptr.is_null() {
            return js_bigint_from_f64(crate::value::js_nanbox_string(str_ptr as i64));
        }
    }

    // Remaining case: a real Number. Node only converts finite integers;
    // NaN, ±Infinity, and any value with a fractional part throw RangeError.
    if !value.is_finite() || value.fract() != 0.0 {
        let label = if value.is_nan() {
            "NaN".to_string()
        } else if value.is_infinite() {
            if value > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else {
            // Only finite non-integers reach here (e.g. 1.5). ECMAScript
            // NumberToString switches to scientific notation outside
            // [1e-6, 1e21); for the common fractional inputs Rust's `{}`
            // already matches Node.
            let abs = value.abs();
            if !(1e-6..1e21).contains(&abs) {
                format!("{:e}", value)
            } else {
                format!("{}", value)
            }
        };
        throw_bigint_range_error(&format!(
            "The number {label} cannot be converted to a BigInt because it is not an integer"
        ));
    }
    bigint_alloc_with_limbs(limbs_from_integer_f64(value))
}

/// Create a BigInt from a string (the `BigInt("…")` coercion path).
///
/// Matches ECMAScript `StringToBigInt` (#2907): leading/trailing whitespace
/// is trimmed; an empty (or all-whitespace) string is `0n`; a decimal string
/// may carry an optional `+`/`-` sign; the radix prefixes `0x`/`0X`, `0o`/`0O`,
/// `0b`/`0B` are accepted (without a sign). Any other content — stray
/// characters, a lone sign, a sign on a prefixed literal — throws a
/// `SyntaxError` instead of silently dropping the invalid characters.
#[no_mangle]
pub extern "C" fn js_bigint_from_string(data: *const u8, len: u32) -> *mut BigIntHeader {
    unsafe {
        let bytes = std::slice::from_raw_parts(data, len as usize);
        let raw = std::str::from_utf8_unchecked(bytes);
        match parse_bigint_string(raw) {
            Ok(limbs) => bigint_alloc_with_limbs(limbs),
            Err(()) => throw_bigint_syntax_error(&format!("Cannot convert {raw} to a BigInt")),
        }
    }
}

/// Parse a string per ECMAScript `StringToBigInt`. Returns the limb array on
/// success, or `Err(())` for invalid BigInt syntax. The original (untrimmed)
/// string is used by the caller to build Node's error message.
fn parse_bigint_string(raw: &str) -> Result<[u64; BIGINT_LIMBS], ()> {
    // ECMAScript trims StrWhiteSpace from both ends; the empty string is 0n.
    let s = raw.trim();
    if s.is_empty() {
        return Ok(ZERO_LIMBS);
    }

    // Radix-prefixed forms do not allow a leading sign.
    let lower_prefix = s.as_bytes().get(0..2).map(|p| {
        let mut buf = [p[0], p[1]];
        buf.make_ascii_lowercase();
        buf
    });
    if let Some([b'0', tag]) = lower_prefix {
        let radix = match tag {
            b'x' => Some(16u32),
            b'o' => Some(8u32),
            b'b' => Some(2u32),
            _ => None,
        };
        if let Some(radix) = radix {
            let digits = &s[2..];
            if digits.is_empty() {
                return Err(());
            }
            return parse_radix_digits(digits, radix, false);
        }
    }

    // Optional sign, then decimal digits only.
    let (is_negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if digits.is_empty() {
        return Err(());
    }
    parse_radix_digits(digits, 10, is_negative)
}

/// Parse `digits` in the given radix (2/8/10/16), rejecting any out-of-range
/// character. Applies two's-complement negation when `is_negative`.
fn parse_radix_digits(
    digits: &str,
    radix: u32,
    is_negative: bool,
) -> Result<[u64; BIGINT_LIMBS], ()> {
    let mut limbs = ZERO_LIMBS;
    let radix_u128 = radix as u128;
    for c in digits.chars() {
        let digit = c.to_digit(radix).ok_or(())?;
        let mut carry = digit as u64;
        for limb in limbs.iter_mut() {
            let product = (*limb as u128) * radix_u128 + carry as u128;
            *limb = product as u64;
            carry = (product >> 64) as u64;
        }
        // A carry out of the top limb means the magnitude passed 2^1024 —
        // a literal too large for the fixed representation (#6073).
        if carry != 0 {
            throw_bigint_overflow();
        }
    }
    // The magnitude fits in 16 limbs but may still exceed the signed range
    // (e.g. a positive literal in [2^1023, 2^1024) would read back negative).
    if !magnitude_fits_1024(&limbs, is_negative) {
        throw_bigint_overflow();
    }
    if is_negative && !limbs.iter().all(|&l| l == 0) {
        limbs = negate_limbs(&limbs);
    }
    Ok(limbs)
}

/// Create a BigInt from a string with a given radix (for BN.js compatibility)
/// Handles decimal (10), hex (16), and other bases.
#[no_mangle]
pub extern "C" fn js_bigint_from_string_radix(
    data: *const u8,
    len: u32,
    radix: i32,
) -> *mut BigIntHeader {
    if data.is_null() || len == 0 {
        // Null input
        return js_bigint_from_i64(0);
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(data, len as usize);
        let s = std::str::from_utf8_unchecked(bytes);
        // Debug removed

        // Handle negative
        let (is_negative, s) = if s.starts_with('-') {
            (true, &s[1..])
        } else {
            (false, s)
        };

        // Strip 0x prefix for hex
        let s = if radix == 16 && (s.starts_with("0x") || s.starts_with("0X")) {
            &s[2..]
        } else {
            s
        };

        let mut limbs = ZERO_LIMBS;
        let radix = radix as u64;

        if radix == 16 {
            // Optimized hex parsing
            let mut chars = s.chars().rev();
            for limb in limbs.iter_mut() {
                let mut value = 0u64;
                for i in 0..16 {
                    if let Some(c) = chars.next() {
                        let digit = match c {
                            '0'..='9' => c as u64 - '0' as u64,
                            'a'..='f' => c as u64 - 'a' as u64 + 10,
                            'A'..='F' => c as u64 - 'A' as u64 + 10,
                            _ => continue,
                        };
                        value |= digit << (i * 4);
                    } else {
                        break;
                    }
                }
                *limb = value;
            }
            // The reversed stream fed the low 16 limbs first, so any leftover
            // chars are the high-order digits. Only a *nonzero* hex digit there
            // sets a bit at or above 2^1024 — excess leading zeros are harmless
            // (`0x0…0<256 digits>` still fits), so ignore '0' and non-hex chars (#6073).
            if chars.any(|c| matches!(c, '1'..='9' | 'a'..='f' | 'A'..='F')) {
                throw_bigint_overflow();
            }
        } else {
            // General radix parsing using long multiplication
            for c in s.chars() {
                let digit = match c {
                    '0'..='9' => (c as u64) - ('0' as u64),
                    'a'..='z' => (c as u64) - ('a' as u64) + 10,
                    'A'..='Z' => (c as u64) - ('A' as u64) + 10,
                    _ => continue,
                };
                if digit >= radix {
                    continue;
                }
                let mut carry = digit;
                for limb in limbs.iter_mut() {
                    let product = (*limb as u128) * (radix as u128) + carry as u128;
                    *limb = product as u64;
                    carry = (product >> 64) as u64;
                }
                // Carry out of the top limb → magnitude passed 2^1024 (#6073).
                if carry != 0 {
                    throw_bigint_overflow();
                }
            }
        }

        // The magnitude fits in 16 limbs but may still exceed the signed range.
        if !magnitude_fits_1024(&limbs, is_negative) {
            throw_bigint_overflow();
        }
        if is_negative && !limbs.iter().all(|&l| l == 0) {
            limbs = negate_limbs(&limbs);
        }
        bigint_alloc_with_limbs(limbs)
    }
}

/// Convert BigInt to a byte array (big-endian, for BN.toArrayLike/toArray)
/// Returns a buffer of the specified length, zero-padded on the left.
#[no_mangle]
pub extern "C" fn js_bigint_to_buffer(
    a: *const BigIntHeader,
    length: i32,
) -> *mut crate::buffer::BufferHeader {
    let limbs = bigint_limbs_or_zero(a);
    let length = if length <= 0 { 32 } else { length as usize };

    let result = crate::buffer::buffer_alloc(length as u32);
    unsafe {
        (*result).length = length as u32;
        let data = crate::buffer::buffer_data_mut(result);

        // Extract bytes from the pre-allocation limb snapshot
        // (little-endian in memory) and write in big-endian order.
        std::ptr::write_bytes(data, 0, length);
        let significant = (BIGINT_LIMBS * 8).min(length);
        for i in 0..significant {
            let limb = limbs[i / 8];
            let byte = ((limb >> ((i % 8) * 8)) & 0xff) as u8;
            *data.add(length - 1 - i) = byte;
        }
    }
    result
}

/// Check if BigInt is negative (MSB set in two's complement)
#[no_mangle]
pub extern "C" fn js_bigint_is_negative(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        return 0;
    }
    unsafe {
        // In two's complement, negative numbers have MSB set in highest limb
        let msb = (*a).limbs[BIGINT_LIMBS - 1];
        if msb & (1u64 << 63) != 0 {
            1
        } else {
            0
        }
    }
}

/// Negate a BigInt (two's complement: flip all bits and add 1)
#[no_mangle]
pub extern "C" fn js_bigint_neg(a: *const BigIntHeader) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let mut result = ZERO_LIMBS;
    let mut carry = 1u64;

    for i in 0..BIGINT_LIMBS {
        let flipped = !a_limbs[i];
        let sum = (flipped as u128) + (carry as u128);
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }

    // -2^1023 is the sole nonzero value whose two's-complement negation is
    // itself; its true negation (+2^1023) is out of range, so throw rather
    // than silently returning -2^1023 unchanged (#6073).
    if result == a_limbs && a_limbs != ZERO_LIMBS {
        throw_bigint_overflow();
    }

    bigint_alloc_with_limbs(result)
}

/// Bitwise NOT of a BigInt (`~a`).
#[no_mangle]
pub extern "C" fn js_bigint_not(a: *const BigIntHeader) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let mut result = ZERO_LIMBS;
    for i in 0..BIGINT_LIMBS {
        result[i] = !a_limbs[i];
    }
    bigint_alloc_with_limbs(result)
}

/// Check if a BigInt is zero (all limbs are zero). Returns 1 for zero, 0 for non-zero.
#[no_mangle]
pub extern "C" fn js_bigint_is_zero(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        return 1;
    }
    unsafe {
        for i in 0..BIGINT_LIMBS {
            if (*a).limbs[i] != 0 {
                return 0;
            }
        }
        1
    }
}

/// Add two BigInts
#[no_mangle]
pub extern "C" fn js_bigint_add(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // Fast path: both operands fit in i64. i64 + i64 fits in i128 with
    // no overflow possible, then write the result back as a sign-extended
    // 1024-bit two's-complement value.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        let mut result = ZERO_LIMBS;
        write_i128((av as i128) + (bv as i128), &mut result);
        return bigint_alloc_with_limbs(result);
    }

    // Slow path: 16-limb add with carry.
    let mut result = ZERO_LIMBS;
    let mut carry = 0u64;
    for i in 0..BIGINT_LIMBS {
        let sum = (a_limbs[i] as u128) + (b_limbs[i] as u128) + (carry as u128);
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    // Signed overflow (#6073): equal-signed operands whose sum takes the
    // opposite sign wrapped past ±2^1023. Throw rather than return the ring
    // value. (Opposite-signed operands can never overflow.)
    if is_negative(&a_limbs) == is_negative(&b_limbs)
        && is_negative(&result) != is_negative(&a_limbs)
    {
        throw_bigint_overflow();
    }
    bigint_alloc_with_limbs(result)
}

/// Subtract two BigInts (a - b)
#[no_mangle]
pub extern "C" fn js_bigint_sub(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // Fast path: both operands fit in i64. i64 - i64 fits in i128.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        let mut result = ZERO_LIMBS;
        write_i128((av as i128) - (bv as i128), &mut result);
        return bigint_alloc_with_limbs(result);
    }

    // Slow path: 16-limb subtract with borrow.
    let mut result = ZERO_LIMBS;
    let mut borrow = 0i128;
    for i in 0..BIGINT_LIMBS {
        let diff = (a_limbs[i] as i128) - (b_limbs[i] as i128) - borrow;
        if diff < 0 {
            result[i] = (diff + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            result[i] = diff as u64;
            borrow = 0;
        }
    }
    // Signed overflow (#6073): opposite-signed operands whose difference takes
    // the sign opposite the minuend wrapped past ±2^1023. (Equal-signed
    // operands can never overflow — this also catches negating -2^1023.)
    if is_negative(&a_limbs) != is_negative(&b_limbs)
        && is_negative(&result) != is_negative(&a_limbs)
    {
        throw_bigint_overflow();
    }
    bigint_alloc_with_limbs(result)
}

/// Multiply two BigInts
#[no_mangle]
pub extern "C" fn js_bigint_mul(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // Fast path: both operands fit in i64. i64 * i64 fits exactly in i128
    // (max |product| = (2^63)^2 = 2^126 < 2^127). This eliminates the 16×16
    // schoolbook loop for the common case where values fit in a host word.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        let mut result = ZERO_LIMBS;
        write_i128((av as i128) * (bv as i128), &mut result);
        return bigint_alloc_with_limbs(result);
    }

    // Slow path: sign-and-magnitude schoolbook multiply that retains the full
    // 2N-limb product so overflow past ±2^1023 is detected, not wrapped (#6073).
    bigint_alloc_with_limbs(mul_limbs_checked(&a_limbs, &b_limbs))
}

/// Multiply two two's-complement limb arrays, throwing `RangeError` when the
/// product exceeds the signed 1024-bit range (#6073). Shared by
/// `js_bigint_mul`'s slow path and `js_bigint_pow`.
///
/// Multiplies the magnitudes (so the sign-extension words never pollute the
/// schoolbook rows) into a `2 * BIGINT_LIMBS` accumulator, checks the result
/// fits, then re-applies the sign. Using two's-complement limbs directly would
/// require carrying the sign extension through every row — sign-and-magnitude
/// is cleaner.
fn mul_limbs_checked(
    a_limbs: &[u64; BIGINT_LIMBS],
    b_limbs: &[u64; BIGINT_LIMBS],
) -> [u64; BIGINT_LIMBS] {
    let a_neg = is_negative(a_limbs);
    let b_neg = is_negative(b_limbs);
    let a_mag = if a_neg {
        negate_limbs(a_limbs)
    } else {
        *a_limbs
    };
    let b_mag = if b_neg {
        negate_limbs(b_limbs)
    } else {
        *b_limbs
    };

    // Skip trailing all-zero limbs so e.g. a 3-limb value times a 2-limb value
    // only does 3×2 word multiplies. `effective_limb_len` never under-counts an
    // in-range magnitude (<= 2^1023), so no significant limb is dropped.
    let a_len = effective_limb_len(&a_mag);
    let b_len = effective_limb_len(&b_mag);
    // The full product of two <2^1024 magnitudes is <2^2048 — it fits in the
    // 2N-limb accumulator, and the carry never escapes index `a_len+b_len-1`.
    let mut wide = [0u64; 2 * BIGINT_LIMBS];
    for i in 0..a_len {
        let mut carry = 0u128;
        for j in 0..b_len {
            let product = (a_mag[i] as u128) * (b_mag[j] as u128) + (wide[i + j] as u128) + carry;
            wide[i + j] = product as u64;
            carry = product >> 64;
        }
        let mut k = i + b_len;
        while carry != 0 && k < 2 * BIGINT_LIMBS {
            let sum = (wide[k] as u128) + carry;
            wide[k] = sum as u64;
            carry = sum >> 64;
            k += 1;
        }
    }

    let negative = a_neg != b_neg;
    if !magnitude_fits_1024(&wide, negative) {
        throw_bigint_overflow();
    }
    let mut result = ZERO_LIMBS;
    result.copy_from_slice(&wide[..BIGINT_LIMBS]);
    if negative {
        result = negate_limbs(&result);
    }
    result
}

/// Magnitude of significant limbs for an unsigned-style limb pattern.
/// For a positive value, returns 1 + index of highest non-zero limb (or 1
/// for zero, since we always need at least one word multiplied). Negative
/// values are handled by their two's-complement: limbs[15] has bit 63 set
/// and the upper limbs may be u64::MAX, so we walk from the top until we
/// find a limb that's neither all-zero nor all-ones.
#[inline(always)]
fn effective_limb_len(limbs: &[u64; BIGINT_LIMBS]) -> usize {
    // Walk from the high end, skipping consecutive all-zero or all-ones
    // limbs (the sign-extension fill). The first limb that breaks the
    // pattern is the highest "real" limb. This is sound for the
    // schoolbook multiplier because we only read the first `len` limbs.
    let fill = if (limbs[BIGINT_LIMBS - 1] >> 63) == 1 {
        u64::MAX
    } else {
        0u64
    };
    for i in (0..BIGINT_LIMBS).rev() {
        if limbs[i] != fill {
            // We need one more limb than i+1 if the next one isn't already
            // the fill (it always is, by construction), so just i+1 plus
            // one safety limb capped at BIGINT_LIMBS.
            return (i + 2).min(BIGINT_LIMBS);
        }
    }
    // All limbs are fill — value is 0 or -1. Either way we only need 1.
    1
}

/// Unsigned binary long division on magnitude limbs
fn unsigned_div_limbs(
    a: &[u64; BIGINT_LIMBS],
    b: &[u64; BIGINT_LIMBS],
) -> ([u64; BIGINT_LIMBS], [u64; BIGINT_LIMBS]) {
    let mut quotient = ZERO_LIMBS;
    let mut remainder = ZERO_LIMBS;

    for i in (0..BIGINT_BITS).rev() {
        // Shift remainder left by 1
        let mut carry = 0u64;
        for limb in remainder.iter_mut() {
            let new_carry = *limb >> 63;
            *limb = (*limb << 1) | carry;
            carry = new_carry;
        }

        // Set LSB of remainder from dividend
        let limb_idx = i / 64;
        let bit_idx = i % 64;
        remainder[0] |= (a[limb_idx] >> bit_idx) & 1;

        // If remainder >= divisor, subtract and set quotient bit
        // Use unsigned comparison for magnitude comparison
        let mut ge = true;
        for j in (0..BIGINT_LIMBS).rev() {
            if remainder[j] > b[j] {
                break;
            }
            if remainder[j] < b[j] {
                ge = false;
                break;
            }
        }
        if ge {
            subtract_limbs(&mut remainder, b);
            let q_limb_idx = i / 64;
            let q_bit_idx = i % 64;
            quotient[q_limb_idx] |= 1u64 << q_bit_idx;
        }
    }

    (quotient, remainder)
}

/// Divide two BigInts (a / b) — truncates toward zero like JavaScript
#[no_mangle]
pub extern "C" fn js_bigint_div(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    if b_limbs == ZERO_LIMBS {
        throw_bigint_division_by_zero();
    }

    // Fast path: both fit in i64. Rust's `/` on i64 truncates toward
    // zero, which is JavaScript's BigInt division semantics. The only
    // overflow case is i64::MIN / -1, which we handle via i128.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        if bv != 0 {
            let mut result = ZERO_LIMBS;
            write_i128((av as i128) / (bv as i128), &mut result);
            return bigint_alloc_with_limbs(result);
        }
    }

    let a_neg = is_negative(&a_limbs);
    let b_neg = is_negative(&b_limbs);

    // Get magnitudes
    let abs_a = if a_neg {
        negate_limbs(&a_limbs)
    } else {
        a_limbs
    };
    let abs_b = if b_neg {
        negate_limbs(&b_limbs)
    } else {
        b_limbs
    };

    let (quotient, _) = unsigned_div_limbs(&abs_a, &abs_b);

    // Result is negative if signs differ
    let result = if a_neg != b_neg && quotient != ZERO_LIMBS {
        negate_limbs(&quotient)
    } else {
        quotient
    };
    bigint_alloc_with_limbs(result)
}

/// Modulo of two BigInts (a % b) — result has sign of dividend (like JavaScript)
#[no_mangle]
pub extern "C" fn js_bigint_mod(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    if b_limbs == ZERO_LIMBS {
        throw_bigint_division_by_zero();
    }

    // Fast path: both fit in i64. JavaScript's `%` returns the sign of
    // the dividend, which is what Rust's `%` on i64 does already.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        // bv != 0 because b_limbs != ZERO_LIMBS for a positive small;
        // for a negative small we still won't hit divide-by-zero.
        if bv != 0 {
            let mut result = ZERO_LIMBS;
            write_i64(av % bv, &mut result);
            return bigint_alloc_with_limbs(result);
        }
    }

    let a_neg = is_negative(&a_limbs);
    let b_neg = is_negative(&b_limbs);

    // Get magnitudes
    let abs_a = if a_neg {
        negate_limbs(&a_limbs)
    } else {
        a_limbs
    };
    let abs_b = if b_neg {
        negate_limbs(&b_limbs)
    } else {
        b_limbs
    };

    let (_, remainder) = unsigned_div_limbs(&abs_a, &abs_b);

    // Remainder has sign of dividend
    let result = if a_neg && remainder != ZERO_LIMBS {
        negate_limbs(&remainder)
    } else {
        remainder
    };
    bigint_alloc_with_limbs(result)
}

/// Power of two BigInts (a ** b) using binary exponentiation
/// Note: b is interpreted as a u64 (only lower 64 bits are used)
#[no_mangle]
pub extern "C" fn js_bigint_pow(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // ECMaScript: a negative BigInt exponent is a RangeError.
    if is_negative(&b_limbs) {
        throw_bigint_range_error("Exponent must be positive");
    }

    // Get exponent as u64 (only lower 64 bits)
    let exp = b_limbs[0];

    if exp == 0 {
        // Anything to the power of 0 is 1
        let mut result = ZERO_LIMBS;
        result[0] = 1;
        return bigint_alloc_with_limbs(result);
    }

    // Binary exponentiation
    let mut result = ZERO_LIMBS;
    result[0] = 1;
    let mut base = a_limbs;
    let mut e = exp;

    while e > 0 {
        if e & 1 == 1 {
            result = mul_limbs_checked(&result, &base);
        }
        e >>= 1;
        // Square only while another bit remains. The final squaring after the
        // top set bit is never used, and squaring it could overflow past a
        // result that itself still fits (e.g. (2^300)^3) — a false throw (#6073).
        if e > 0 {
            base = mul_limbs_checked(&base, &base);
        }
    }

    bigint_alloc_with_limbs(result)
}

/// Left-shift a two's-complement BigInt by `shift` bits (`a << shift`),
/// throwing `RangeError` when the result exceeds the signed 1024-bit range
/// (#6073). Shifting a magnitude left by N bits multiplies it by 2^N, so any
/// significant bit pushed to >= 2^1023 (positive) / > 2^1023 (negative)
/// overflows instead of silently falling off the top.
fn shl_checked(a_limbs: &[u64; BIGINT_LIMBS], shift: usize) -> [u64; BIGINT_LIMBS] {
    if shift == 0 || a_limbs == &ZERO_LIMBS {
        return *a_limbs;
    }
    // A shift of >= 1024 bits pushes even the lowest set bit of a nonzero
    // magnitude to >= 2^1024, which never fits.
    if shift >= BIGINT_BITS {
        throw_bigint_overflow();
    }

    let a_neg = is_negative(a_limbs);
    let mag = if a_neg {
        negate_limbs(a_limbs)
    } else {
        *a_limbs
    };

    // shift < 1024 and mag < 2^1023 → product < 2^2046, always within the
    // 2N-limb buffer: the top written index is <= 30, plus one carry limb.
    let mut wide = [0u64; 2 * BIGINT_LIMBS];
    let limb_shift = shift / 64;
    let bit_shift = (shift % 64) as u32;
    for i in 0..BIGINT_LIMBS {
        if mag[i] == 0 {
            continue;
        }
        let dst = i + limb_shift;
        if bit_shift == 0 {
            wide[dst] |= mag[i];
        } else {
            wide[dst] |= mag[i] << bit_shift;
            wide[dst + 1] |= mag[i] >> (64 - bit_shift);
        }
    }

    if !magnitude_fits_1024(&wide, a_neg) {
        throw_bigint_overflow();
    }
    let mut result = ZERO_LIMBS;
    result.copy_from_slice(&wide[..BIGINT_LIMBS]);
    if a_neg {
        result = negate_limbs(&result);
    }
    result
}

/// Arithmetic right-shift a limb array by `shift` bits (sign-extending).
fn shr_limbs(a_limbs: &[u64; BIGINT_LIMBS], shift: usize) -> [u64; BIGINT_LIMBS] {
    let neg = is_negative(a_limbs);
    let fill: u64 = if neg { !0u64 } else { 0u64 };
    if shift >= BIGINT_BITS {
        return [fill; BIGINT_LIMBS];
    }
    let mut result = [fill; BIGINT_LIMBS];
    let limb_shift = shift / 64;
    let bit_shift = (shift % 64) as u32;
    if bit_shift == 0 {
        for i in 0..(BIGINT_LIMBS - limb_shift) {
            result[i] = a_limbs[i + limb_shift];
        }
    } else {
        for i in 0..(BIGINT_LIMBS - limb_shift) {
            let src_idx = i + limb_shift;
            result[i] = a_limbs[src_idx] >> bit_shift;
            if src_idx + 1 < BIGINT_LIMBS {
                result[i] |= a_limbs[src_idx + 1] << (64 - bit_shift);
            } else {
                result[i] |= fill << (64 - bit_shift);
            }
        }
    }
    result
}

/// Interpret a two's-complement shift count as a signed magnitude. Returns
/// `(magnitude, count_is_negative)`. Counts beyond `BIGINT_BITS` saturate.
fn shift_count(b_limbs: &[u64; BIGINT_LIMBS]) -> (usize, bool) {
    if is_negative(b_limbs) {
        let mag = negate_limbs(b_limbs);
        // Only the low limb matters for any realistic shift; if any upper
        // limb is set the count is enormous → saturate past BIGINT_BITS.
        if mag[1..].iter().any(|&l| l != 0) {
            (BIGINT_BITS, true)
        } else {
            (mag[0] as usize, true)
        }
    } else {
        if b_limbs[1..].iter().any(|&l| l != 0) {
            (BIGINT_BITS, false)
        } else {
            (b_limbs[0] as usize, false)
        }
    }
}

/// Left shift BigInt by b bits (a << b). A negative shift count reverses
/// direction (`a << -n` === `a >> n`), matching ECMAScript.
#[no_mangle]
pub extern "C" fn js_bigint_shl(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let (shift, negative) = shift_count(&b_limbs);
    let result = if negative {
        shr_limbs(&a_limbs, shift)
    } else {
        shl_checked(&a_limbs, shift)
    };
    bigint_alloc_with_limbs(result)
}

/// Right shift BigInt by b bits (a >> b), arithmetic / sign-extending. A
/// negative shift count reverses direction (`a >> -n` === `a << n`), matching
/// ECMAScript.
#[no_mangle]
pub extern "C" fn js_bigint_shr(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let (shift, negative) = shift_count(&b_limbs);
    let result = if negative {
        shl_checked(&a_limbs, shift)
    } else {
        shr_limbs(&a_limbs, shift)
    };
    bigint_alloc_with_limbs(result)
}

/// Bitwise AND of two BigInts (a & b)
#[no_mangle]
pub extern "C" fn js_bigint_and(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let mut result = ZERO_LIMBS;

    for i in 0..BIGINT_LIMBS {
        result[i] = a_limbs[i] & b_limbs[i];
    }

    bigint_alloc_with_limbs(result)
}

/// Bitwise OR of two BigInts (a | b)
#[no_mangle]
pub extern "C" fn js_bigint_or(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let mut result = ZERO_LIMBS;
    for i in 0..BIGINT_LIMBS {
        result[i] = a_limbs[i] | b_limbs[i];
    }
    bigint_alloc_with_limbs(result)
}

/// Bitwise XOR of two BigInts (a ^ b)
#[no_mangle]
pub extern "C" fn js_bigint_xor(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let mut result = ZERO_LIMBS;
    for i in 0..BIGINT_LIMBS {
        result[i] = a_limbs[i] ^ b_limbs[i];
    }
    bigint_alloc_with_limbs(result)
}

/// Compare two BigInts (-1 if a < b, 0 if equal, 1 if a > b)
#[no_mangle]
pub extern "C" fn js_bigint_cmp(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    if a.is_null() || b.is_null() {
        return 0;
    }
    unsafe {
        let a_limbs = (*a).limbs;
        let b_limbs = (*b).limbs;
        // Fast path: both fit in i64. The vast majority of comparisons in
        // hot loops (factorial bounds, postgres int8 inequality, app id
        // ordering) hit this case.
        if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
            return match av.cmp(&bv) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
        }
        compare_limbs(&a_limbs, &b_limbs)
    }
}

/// `StringToBigInt` (ES2024 §7.1.14), non-throwing. Returns `None` when the
/// string is not a valid BigInt literal — the abstract relational comparison
/// treats that as `undefined` (so the comparison yields `false`) rather than a
/// thrown `SyntaxError`. Leading/trailing whitespace is trimmed, an empty
/// string is `0n`, and the `0x`/`0o`/`0b` radix prefixes are accepted.
pub(crate) fn string_to_bigint(raw: &str) -> Option<*mut BigIntHeader> {
    parse_bigint_string(raw).ok().map(bigint_alloc_with_limbs)
}

/// Mathematically compare a BigInt `x` against a Number `y` (ES2024 §7.2.13,
/// the mixed BigInt/Number step). Comparison is exact — no precision loss from
/// `BigInt → f64` — because `y` is decomposed into its integer floor (converted
/// to a BigInt without rounding) and any leftover fraction.
///
/// Returns `-1` (x < y), `0` (x == y), `1` (x > y), or `2` (undefined: `y` is
/// `NaN`, the one incomparable case).
pub(crate) fn bigint_cmp_f64(x: *const BigIntHeader, y: f64) -> i32 {
    if y.is_nan() {
        return 2;
    }
    if y == f64::INFINITY {
        return -1; // x < +Infinity for every finite BigInt
    }
    if y == f64::NEG_INFINITY {
        return 1; // x > -Infinity
    }
    // Perry's BigInt is a fixed 1024-bit two's-complement value, so any Number
    // with |y| >= 2^1023 (e.g. `Number.MAX_VALUE` ~2^1024) does not round-trip
    // through `js_bigint_from_f64`: its top magnitude bit lands on the sign bit
    // and reads back as negative. Two sub-cases:
    //
    //  * `x` is a *small* BigInt (fits in i64) — far below 2^1023 in magnitude,
    //    so the comparison is decided by the sign of `y` alone. This is the
    //    `1n {<,<=,>,>=} Number.MAX_VALUE` fix (#5894): without it the lossy
    //    round-trip made `1n >= MAX_VALUE` wrongly true.
    //
    //  * `x` is a *large* BigInt (a literal near 2^1024). Such a literal already
    //    overflowed the signed 1024-bit width on parse, so `x` and the
    //    round-tripped `y` overflow *identically* — comparing their raw
    //    two's-complement limbs reproduces the exact mathematical order,
    //    including equality when the literal is exactly `MAX_VALUE` (test262
    //    `bigint-and-number-extremes` equals/does-not-equals/relational). Fall
    //    through to the raw compare; a narrower short-circuit here would flip
    //    that equality back to less-than and re-break those cases.
    //
    // 2^1023 as an exact IEEE-754 double (biased exponent 2046, zero mantissa).
    let bigint_mag_bound = f64::from_bits(0x7FE0_0000_0000_0000);
    if y.abs() >= bigint_mag_bound {
        let small = unsafe {
            let xp = clean_bigint_ptr(x);
            !xp.is_null() && fits_in_i64(&(*xp).limbs).is_some()
        };
        if small {
            return if y > 0.0 { -1 } else { 1 };
        }
        // else: large `x` — raw compare below is exact for the overflow regime.
    }
    // `y` is finite. Compare `x` with `floor(y)` as exact integers; if equal,
    // a positive fractional part of `y` makes `x` strictly smaller.
    let floor = y.floor();
    let floor_big = js_bigint_from_f64(floor);
    let c = js_bigint_cmp(x, floor_big);
    if c != 0 {
        return c;
    }
    if y > floor {
        -1
    } else {
        0
    }
}

/// Check if two BigInts are equal
#[no_mangle]
pub extern "C" fn js_bigint_eq(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    if a.is_null() || b.is_null() {
        return if a == b { 1 } else { 0 }; // both null = equal, one null = not equal
    }
    unsafe {
        if (*a).limbs == (*b).limbs {
            1
        } else {
            0
        }
    }
}

/// Convert BigInt to f64 (may lose precision)
#[no_mangle]
pub extern "C" fn js_bigint_to_f64(a: *const BigIntHeader) -> f64 {
    unsafe {
        if a.is_null() {
            return 0.0;
        }
        let limbs = (*a).limbs;
        let neg = is_negative(&limbs);
        let abs_limbs = if neg { negate_limbs(&limbs) } else { limbs };
        let mut result = 0.0f64;
        let mut multiplier = 1.0f64;
        for limb in abs_limbs.iter() {
            result += (*limb as f64) * multiplier;
            multiplier *= 18446744073709551616.0; // 2^64
        }
        if neg {
            -result
        } else {
            result
        }
    }
}

/// Helper to convert limbs to decimal string
/// Check if a bigint value is negative (high bit of highest limb is set = two's complement negative)
fn is_negative(limbs: &[u64; BIGINT_LIMBS]) -> bool {
    (limbs[BIGINT_LIMBS - 1] >> 63) == 1
}

/// Negate limbs in place (two's complement: flip all bits and add 1)
fn negate_limbs(limbs: &[u64; BIGINT_LIMBS]) -> [u64; BIGINT_LIMBS] {
    let mut result = ZERO_LIMBS;
    let mut carry = 1u64;
    for i in 0..BIGINT_LIMBS {
        let flipped = !limbs[i];
        let sum = (flipped as u128) + (carry as u128);
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    result
}

fn limbs_to_decimal_string(limbs: &[u64; BIGINT_LIMBS]) -> String {
    let mut digits = Vec::new();

    // Check if zero
    if *limbs == ZERO_LIMBS {
        return "0".to_string();
    }

    // Check if negative (two's complement)
    let negative = is_negative(limbs);
    let mut temp = if negative {
        negate_limbs(limbs)
    } else {
        *limbs
    };

    while temp != ZERO_LIMBS {
        let mut remainder = 0u128;
        for i in (0..BIGINT_LIMBS).rev() {
            let dividend = (remainder << 64) + temp[i] as u128;
            temp[i] = (dividend / 10) as u64;
            remainder = dividend % 10;
        }
        digits.push((remainder as u8 + b'0') as char);
    }

    digits.reverse();
    let s: String = digits.into_iter().collect();
    if negative {
        format!("-{}", s)
    } else {
        s
    }
}

fn limbs_to_radix_string(limbs: &[u64; BIGINT_LIMBS], radix: u32) -> String {
    let radix = if !(2..=36).contains(&radix) {
        10
    } else {
        radix
    };
    if radix == 10 {
        return limbs_to_decimal_string(limbs);
    }

    let mut digits = Vec::new();

    if *limbs == ZERO_LIMBS {
        return "0".to_string();
    }

    let negative = is_negative(limbs);
    let mut temp = if negative {
        negate_limbs(limbs)
    } else {
        *limbs
    };

    let radix_u128 = radix as u128;
    while temp != ZERO_LIMBS {
        let mut remainder = 0u128;
        for i in (0..BIGINT_LIMBS).rev() {
            let dividend = (remainder << 64) + temp[i] as u128;
            temp[i] = (dividend / radix_u128) as u64;
            remainder = dividend % radix_u128;
        }
        let digit = remainder as u8;
        let ch = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + (digit - 10)
        };
        digits.push(ch as char);
    }

    digits.reverse();
    let s: String = digits.into_iter().collect();
    if negative {
        format!("-{}", s)
    } else {
        s
    }
}

/// Convert BigInt to string
#[no_mangle]
pub extern "C" fn js_bigint_to_string(a: *const BigIntHeader) -> *mut crate::string::StringHeader {
    unsafe {
        if a.is_null() || (a as usize) < 0x10000 || (a as u64) >> 48 != 0 {
            return std::ptr::null_mut();
        }
        let s = limbs_to_decimal_string(&(*a).limbs);
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }
}

/// Convert BigInt to string with radix
#[no_mangle]
pub extern "C" fn js_bigint_to_string_radix(
    a: *const BigIntHeader,
    radix: i32,
) -> *mut crate::string::StringHeader {
    unsafe {
        if a.is_null() || (a as usize) < 0x10000 || (a as u64) >> 48 != 0 {
            return std::ptr::null_mut();
        }
        let s = limbs_to_radix_string(&(*a).limbs, radix as u32);
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }
}

/// Print BigInt to stdout (for debugging)
#[no_mangle]
pub extern "C" fn js_bigint_print(a: *const BigIntHeader) {
    unsafe {
        let s = limbs_to_decimal_string(&(*a).limbs);
        println!("{}n", s);
    }
}

/// Print BigInt to stderr (console.error)
#[no_mangle]
pub extern "C" fn js_bigint_error(a: *const BigIntHeader) {
    unsafe {
        let s = limbs_to_decimal_string(&(*a).limbs);
        let _ = s;
    }
}

/// Print BigInt to stderr (console.warn)
#[no_mangle]
pub extern "C" fn js_bigint_warn(a: *const BigIntHeader) {
    unsafe {
        let s = limbs_to_decimal_string(&(*a).limbs);
        let _ = s;
    }
}

// Helper functions

fn compare_limbs(a: &[u64; BIGINT_LIMBS], b: &[u64; BIGINT_LIMBS]) -> i32 {
    let a_neg = is_negative(a);
    let b_neg = is_negative(b);

    // Different signs: negative < positive
    if a_neg && !b_neg {
        return -1;
    }
    if !a_neg && b_neg {
        return 1;
    }

    // Same sign: unsigned comparison (works for both positive and negative in two's complement)
    for i in (0..BIGINT_LIMBS).rev() {
        if a[i] > b[i] {
            return 1;
        }
        if a[i] < b[i] {
            return -1;
        }
    }
    0
}

fn subtract_limbs(a: &mut [u64; BIGINT_LIMBS], b: &[u64; BIGINT_LIMBS]) {
    let mut borrow = 0i128;
    for i in 0..BIGINT_LIMBS {
        let diff = (a[i] as i128) - (b[i] as i128) - borrow;
        if diff < 0 {
            a[i] = (diff + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            a[i] = diff as u64;
            borrow = 0;
        }
    }
}

/// Mask a 1024-bit two's-complement limb array down to its low `bits` bits,
/// zeroing everything at or above bit index `bits`. `bits >= BIGINT_BITS`
/// leaves the value unchanged.
fn mask_low_bits(mut limbs: [u64; BIGINT_LIMBS], bits: usize) -> [u64; BIGINT_LIMBS] {
    if bits >= BIGINT_BITS {
        return limbs;
    }
    let full_limbs = bits / 64;
    let rem = bits % 64;
    for (i, l) in limbs.iter_mut().enumerate() {
        if i < full_limbs {
            // keep
        } else if i == full_limbs && rem != 0 {
            *l &= (1u64 << rem) - 1;
        } else {
            *l = 0;
        }
    }
    limbs
}

/// `BigInt.asUintN(bits, bigint)` — wrap `value` to a `bits`-wide UNSIGNED
/// integer: `value mod 2^bits`, always non-negative. (`asUintN(0, x)` → 0n.)
#[no_mangle]
pub extern "C" fn js_bigint_as_uint_n(bits: u32, value: *const BigIntHeader) -> *mut BigIntHeader {
    let limbs = bigint_limbs_or_zero(value);
    let masked = mask_low_bits(limbs, bits as usize);
    bigint_alloc_with_limbs(masked)
}

/// `BigInt.asIntN(bits, bigint)` — wrap `value` to a `bits`-wide SIGNED
/// two's-complement integer: `value mod 2^bits`, then interpret the top bit
/// (bit `bits-1`) as the sign and sign-extend. (`asIntN(0, x)` → 0n.)
#[no_mangle]
pub extern "C" fn js_bigint_as_int_n(bits: u32, value: *const BigIntHeader) -> *mut BigIntHeader {
    let bits = bits as usize;
    if bits == 0 {
        return bigint_alloc_with_limbs(ZERO_LIMBS);
    }
    if bits >= BIGINT_BITS {
        // No truncation possible within our width; value already two's-complement.
        return bigint_alloc_with_limbs(bigint_limbs_or_zero(value));
    }
    let mut masked = mask_low_bits(bigint_limbs_or_zero(value), bits);
    // If the sign bit (bit bits-1) is set, sign-extend: set all bits >= bits-1.
    let sign_limb = (bits - 1) / 64;
    let sign_pos = (bits - 1) % 64;
    let sign_set = (masked[sign_limb] >> sign_pos) & 1 == 1;
    if sign_set {
        // Set every bit from `bits` upward to 1 (two's-complement negative).
        let full_limbs = bits / 64;
        let rem = bits % 64;
        for (i, l) in masked.iter_mut().enumerate() {
            if i < full_limbs {
                // low full limbs: keep
            } else if i == full_limbs && rem != 0 {
                *l |= !((1u64 << rem) - 1);
            } else if i >= full_limbs {
                *l = u64::MAX;
            }
        }
    }
    bigint_alloc_with_limbs(masked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bigint_from_u64() {
        let bi = js_bigint_from_u64(12345);
        unsafe {
            assert_eq!((*bi).limbs[0], 12345);
            assert_eq!((*bi).limbs[1], 0);
        }
    }

    #[test]
    fn test_bigint_add() {
        let a = js_bigint_from_u64(100);
        let b = js_bigint_from_u64(200);
        let c = js_bigint_add(a, b);
        unsafe {
            assert_eq!((*c).limbs[0], 300);
        }
    }

    #[test]
    fn test_bigint_mul() {
        let a = js_bigint_from_u64(1000);
        let b = js_bigint_from_u64(2000);
        let c = js_bigint_mul(a, b);
        unsafe {
            assert_eq!((*c).limbs[0], 2_000_000);
        }
    }

    #[test]
    fn test_bigint_from_i128_parts_preserves_wide_small_result() {
        let value = (i64::MAX as i128) + 1;
        let lo = value as u128 as u64;
        let hi = ((value as u128) >> 64) as u64 as i64;
        let bi = js_bigint_from_i128_parts(lo, hi);
        unsafe {
            assert_eq!((*bi).limbs[0], 0x8000_0000_0000_0000);
            assert_eq!((*bi).limbs[1], 0);
            assert!(fits_in_i64(&(*bi).limbs).is_none());
        }

        let negative = -((i64::MAX as i128) + 2);
        let lo = negative as u128 as u64;
        let hi = ((negative as u128) >> 64) as u64 as i64;
        let bi = js_bigint_from_i128_parts(lo, hi);
        unsafe {
            assert_eq!((*bi).limbs[0], 0x7fff_ffff_ffff_ffff);
            assert_eq!((*bi).limbs[1], u64::MAX);
            assert_eq!((*bi).limbs[BIGINT_LIMBS - 1], u64::MAX);
            assert!(fits_in_i64(&(*bi).limbs).is_none());
        }
    }

    #[test]
    fn test_bigint_from_string() {
        let s = "123456789";
        let bi = js_bigint_from_string(s.as_ptr(), s.len() as u32);
        unsafe {
            assert_eq!((*bi).limbs[0], 123456789);
        }
    }

    #[test]
    fn test_bigint_from_hex() {
        let s = "0xFFFFFFFFFFFFFFFF"; // max u64
        let bi = js_bigint_from_string(s.as_ptr(), s.len() as u32);
        unsafe {
            assert_eq!((*bi).limbs[0], u64::MAX);
            assert_eq!((*bi).limbs[1], 0);
        }
    }

    #[test]
    fn test_bigint_mul_3limb() {
        // 1e39 * 2e39 = 2e78
        let s1 = "1000000000000000000000000000000000000000";
        let s2 = "2000000000000000000000000000000000000000";
        let a = js_bigint_from_string(s1.as_ptr(), s1.len() as u32);
        let b = js_bigint_from_string(s2.as_ptr(), s2.len() as u32);

        let a_f64 = js_bigint_to_f64(a);
        let b_f64 = js_bigint_to_f64(b);
        assert!(
            (a_f64 - 1e39).abs() / 1e39 < 1e-15,
            "a parse wrong: {}",
            a_f64
        );
        assert!(
            (b_f64 - 2e39).abs() / 2e39 < 1e-15,
            "b parse wrong: {}",
            b_f64
        );

        let c = js_bigint_mul(a, b);
        let c_f64 = js_bigint_to_f64(c);
        assert!(
            (c_f64 - 2e78).abs() / 2e78 < 1e-15,
            "3L*3L multiply wrong: got {}, expected 2e78",
            c_f64
        );
    }

    #[test]
    fn test_bigint_mul_shifted() {
        // Reproduce: a = 46903565894391149, shifted = a << 96, b = 392217725163781510767080209313900517
        // shifted * b should be ~1.458e81
        let sa = "46903565894391149";
        let sb = "392217725163781510767080209313900517";
        let a = js_bigint_from_string(sa.as_ptr(), sa.len() as u32);
        let b96 = js_bigint_from_u64(96);
        let shifted = js_bigint_shl(a, b96);
        let b = js_bigint_from_string(sb.as_ptr(), sb.len() as u32);

        let product = js_bigint_mul(shifted, b);
        let product_f64 = js_bigint_to_f64(product);

        // Expected: ~1.458e81
        assert!(
            product_f64 > 1e80,
            "shifted*b too small: got {}, expected ~1.458e81",
            product_f64
        );
    }

    #[test]
    fn test_bigint_div_large() {
        // Test division: (1e39 * 2e39) / 1e39 = 2e39
        let s1 = "1000000000000000000000000000000000000000";
        let s2 = "2000000000000000000000000000000000000000";
        let a = js_bigint_from_string(s1.as_ptr(), s1.len() as u32);
        let b = js_bigint_from_string(s2.as_ptr(), s2.len() as u32);
        let product = js_bigint_mul(a, b);
        let quotient = js_bigint_div(product, a);
        let q_f64 = js_bigint_to_f64(quotient);
        assert!(
            (q_f64 - 2e39).abs() / 2e39 < 1e-15,
            "division wrong: got {}, expected 2e39",
            q_f64
        );
    }

    // -- Tests added with the small-int fast path (v0.5.730) --
    //
    // These verify that the fast path agrees byte-for-byte with the
    // existing 16-limb slow path on the boundaries (positive×positive,
    // negative×positive, factorial growth, comparison ordering, modulo
    // sign-of-dividend semantics, and the i64 boundary where the path
    // promotes to the schoolbook multiplier).

    /// Helper to read a freshly-allocated bigint as i64 (panics if it
    /// doesn't fit — used in tests for clarity).
    fn read_as_i64(p: *const BigIntHeader) -> i64 {
        unsafe { fits_in_i64(&(*p).limbs).expect("expected to fit in i64") }
    }

    #[test]
    fn fast_path_mul_positive_positive() {
        let a = js_bigint_from_i64(1_000_000);
        let b = js_bigint_from_i64(2_500_000);
        let c = js_bigint_mul(a, b);
        assert_eq!(read_as_i64(c), 2_500_000_000_000);
    }

    #[test]
    fn fast_path_mul_negative_positive() {
        let a = js_bigint_from_i64(-7);
        let b = js_bigint_from_i64(11);
        let c = js_bigint_mul(a, b);
        assert_eq!(read_as_i64(c), -77);
    }

    #[test]
    fn fast_path_mul_negative_negative() {
        let a = js_bigint_from_i64(-1234);
        let b = js_bigint_from_i64(-5678);
        let c = js_bigint_mul(a, b);
        assert_eq!(read_as_i64(c), 1234 * 5678);
    }

    #[test]
    fn fast_path_factorial_20_within_i64() {
        // 20! = 2432902008176640000 < i64::MAX, exercises the fast path
        // through every step of the loop.
        let mut acc = js_bigint_from_i64(1);
        for i in 2..=20i64 {
            let nb = js_bigint_from_i64(i);
            acc = js_bigint_mul(acc, nb);
        }
        assert_eq!(read_as_i64(acc), 2_432_902_008_176_640_000);
    }

    #[test]
    fn slow_path_factorial_21_overflows_i64() {
        // 21! = 51090942171709440000 > i64::MAX, exercises the
        // promotion from fast path to slow path mid-multiply.
        let mut acc = js_bigint_from_i64(1);
        for i in 2..=21i64 {
            let nb = js_bigint_from_i64(i);
            acc = js_bigint_mul(acc, nb);
        }
        unsafe {
            // 21! = 51_090_942_171_709_440_000 = (limbs[1]<<64) | limbs[0]
            //     = 2 * 2^64 + 14_197_454_024_290_336_768
            let limbs = (*acc).limbs;
            assert_eq!(limbs[0], 14_197_454_024_290_336_768);
            assert_eq!(limbs[1], 2);
            for &l in &limbs[2..] {
                assert_eq!(l, 0);
            }
        }
    }

    #[test]
    fn fast_path_add_sub_signed() {
        // (a, b) ∈ {±} test grid
        let a = js_bigint_from_i64(100);
        let b = js_bigint_from_i64(-30);
        assert_eq!(read_as_i64(js_bigint_add(a, b)), 70);
        assert_eq!(read_as_i64(js_bigint_sub(a, b)), 130);
        let a = js_bigint_from_i64(-100);
        let b = js_bigint_from_i64(-30);
        assert_eq!(read_as_i64(js_bigint_add(a, b)), -130);
        assert_eq!(read_as_i64(js_bigint_sub(a, b)), -70);
    }

    #[test]
    fn fast_path_cmp_signed() {
        let a = js_bigint_from_i64(5);
        let b = js_bigint_from_i64(10);
        assert_eq!(js_bigint_cmp(a, b), -1);
        assert_eq!(js_bigint_cmp(b, a), 1);
        assert_eq!(js_bigint_cmp(a, a), 0);

        let neg = js_bigint_from_i64(-1);
        let pos = js_bigint_from_i64(1);
        assert_eq!(js_bigint_cmp(neg, pos), -1);
        assert_eq!(js_bigint_cmp(pos, neg), 1);
    }

    #[test]
    fn fast_path_mod_sign_of_dividend() {
        // ECMAScript: BigInt `%` returns sign of dividend.
        // 17n % 5n === 2n; -17n % 5n === -2n; 17n % -5n === 2n.
        let m = |a: i64, b: i64| -> i64 {
            let av = js_bigint_from_i64(a);
            let bv = js_bigint_from_i64(b);
            read_as_i64(js_bigint_mod(av, bv))
        };
        assert_eq!(m(17, 5), 2);
        assert_eq!(m(-17, 5), -2);
        assert_eq!(m(17, -5), 2);
        assert_eq!(m(-17, -5), -2);
        assert_eq!(m(0, 5), 0);
    }

    #[test]
    fn fast_path_div_truncate_toward_zero() {
        // ECMAScript BigInt `/` truncates toward zero.
        let d = |a: i64, b: i64| -> i64 {
            let av = js_bigint_from_i64(a);
            let bv = js_bigint_from_i64(b);
            read_as_i64(js_bigint_div(av, bv))
        };
        assert_eq!(d(7, 2), 3);
        assert_eq!(d(-7, 2), -3);
        assert_eq!(d(7, -2), -3);
        assert_eq!(d(-7, -2), 3);
    }

    // -- #2754 / #2907: BigInt() coercion semantics --

    #[test]
    fn coerce_boolean_inputs() {
        use crate::value::JSValue;
        let t = js_bigint_from_f64(f64::from_bits(JSValue::bool(true).bits()));
        assert_eq!(read_as_i64(t), 1);
        let f = js_bigint_from_f64(f64::from_bits(JSValue::bool(false).bits()));
        assert_eq!(read_as_i64(f), 0);
    }

    #[test]
    fn coerce_finite_integer_number() {
        // Plain f64 (real Number) integer → exact BigInt.
        let b = js_bigint_from_f64(42.0);
        assert_eq!(read_as_i64(b), 42);
        let b = js_bigint_from_f64(-7.0);
        assert_eq!(read_as_i64(b), -7);
    }

    #[test]
    fn coerce_large_integer_number_preserved() {
        // 2^60 fits in f64 exactly and exceeds nothing; verify the full
        // value is preserved (not saturated/truncated).
        let v = (1u64 << 60) as f64;
        let b = js_bigint_from_f64(v);
        assert_eq!(read_as_i64(b), 1i64 << 60);
    }

    // -- #2907: string parsing validation --

    fn parse(s: &str) -> Result<i64, ()> {
        parse_bigint_string(s).map(|limbs| fits_in_i64(&limbs).expect("fits"))
    }

    #[test]
    fn parse_radix_prefixes_and_whitespace() {
        assert_eq!(parse("0x10"), Ok(16));
        assert_eq!(parse("0o17"), Ok(15));
        assert_eq!(parse("0b101"), Ok(5));
        assert_eq!(parse("  42  "), Ok(42));
        assert_eq!(parse(""), Ok(0));
        assert_eq!(parse("  "), Ok(0));
        assert_eq!(parse("+5"), Ok(5));
        assert_eq!(parse("-5"), Ok(-5));
    }

    #[test]
    fn parse_invalid_strings_reject() {
        assert_eq!(parse("bad"), Err(()));
        assert_eq!(parse("12abc34"), Err(()));
        assert_eq!(parse("0x"), Err(()));
        assert_eq!(parse("0xG"), Err(()));
        assert_eq!(parse("1_000"), Err(()));
        assert_eq!(parse("+"), Err(()));
    }

    // -- #2908: shift direction-reversing + pow --

    #[test]
    fn shift_negative_count_reverses_direction() {
        // 1n << -1n === 1n >> 1n === 0n
        let one = js_bigint_from_i64(1);
        let neg_one = js_bigint_from_i64(-1);
        assert_eq!(read_as_i64(js_bigint_shl(one, neg_one)), 0);
        // 8n >> -1n === 8n << 1n === 16n
        let eight = js_bigint_from_i64(8);
        assert_eq!(read_as_i64(js_bigint_shr(eight, neg_one)), 16);
        // Sanity: positive counts still work.
        let four = js_bigint_from_i64(4);
        assert_eq!(read_as_i64(js_bigint_shl(one, four)), 16);
        let two = js_bigint_from_i64(2);
        assert_eq!(read_as_i64(js_bigint_shr(eight, two)), 2);
    }

    #[test]
    fn permission_bitwise_values_match_node() {
        let bitfield = js_bigint_from_i64(9216);
        let zero = js_bigint_from_i64(0);
        let one = js_bigint_from_i64(1);
        let eleven = js_bigint_from_i64(11);
        let thirteen = js_bigint_from_i64(13);

        let manage_messages = js_bigint_shl(one, thirteen);
        let send_messages = js_bigint_shl(one, eleven);
        let and_result = js_bigint_and(bitfield, manage_messages);
        let or_result = js_bigint_or(zero, send_messages);
        let not_result = js_bigint_not(send_messages);

        assert_eq!(read_as_i64(manage_messages), 8192);
        assert_eq!(read_as_i64(send_messages), 2048);
        assert_eq!(read_as_i64(and_result), 8192);
        assert_eq!(read_as_i64(or_result), 2048);
        assert_eq!(read_as_i64(not_result), -2049);
    }

    #[test]
    fn pow_non_negative() {
        let two = js_bigint_from_i64(2);
        let three = js_bigint_from_i64(3);
        assert_eq!(read_as_i64(js_bigint_pow(two, three)), 8);
        let zero = js_bigint_from_i64(0);
        assert_eq!(read_as_i64(js_bigint_pow(two, zero)), 1);
    }

    #[test]
    fn fits_in_i64_boundary() {
        // i64::MIN encodes as limbs[0]=0x80...0 limbs[1..]=u64::MAX.
        let min = js_bigint_from_i64(i64::MIN);
        assert_eq!(read_as_i64(min), i64::MIN);
        // i64::MAX encodes as limbs[0]=0x7F...F limbs[1..]=0.
        let max = js_bigint_from_i64(i64::MAX);
        assert_eq!(read_as_i64(max), i64::MAX);
        // 2^63 (= i64::MAX + 1, doesn't fit in i64) must NOT fit.
        // Build it via add to avoid going through js_bigint_from_i64
        // (which only takes i64).
        let one = js_bigint_from_i64(1);
        let beyond = js_bigint_add(max, one);
        unsafe {
            assert!(
                fits_in_i64(&(*beyond).limbs).is_none(),
                "i64::MAX + 1 should not fit in i64"
            );
        }
    }

    // -- #6073: fixed 1024-bit range — overflow detection --
    //
    // The throwing side aborts the test process (`js_throw` with no active try
    // frame calls `std::process::exit`), so these cover the pure fit predicate
    // and that large-but-in-range arithmetic still succeeds without a false
    // overflow. End-to-end `RangeError` behavior is exercised by a `.ts` probe.

    /// Build 2^n (n < 1023 so it stays representable).
    fn pow2(n: i64) -> *mut BigIntHeader {
        js_bigint_pow(js_bigint_from_i64(2), js_bigint_from_i64(n))
    }

    #[test]
    fn magnitude_fits_1024_boundaries() {
        let mut m = [0u64; 2 * BIGINT_LIMBS];
        // magnitude < 2^1023 → representable with either sign.
        m[BIGINT_LIMBS - 1] = 0x7fff_ffff_ffff_ffff;
        assert!(magnitude_fits_1024(&m, false));
        assert!(magnitude_fits_1024(&m, true));

        // magnitude == 2^1023 → only the negative endpoint (-2^1023) fits.
        let mut m = [0u64; 2 * BIGINT_LIMBS];
        m[BIGINT_LIMBS - 1] = 1u64 << 63;
        assert!(!magnitude_fits_1024(&m, false));
        assert!(magnitude_fits_1024(&m, true));

        // magnitude in (2^1023, 2^1024) → fits neither sign.
        m[0] = 1;
        assert!(!magnitude_fits_1024(&m, false));
        assert!(!magnitude_fits_1024(&m, true));

        // any bit at or above 2^1024 → never fits.
        let mut m = [0u64; 2 * BIGINT_LIMBS];
        m[BIGINT_LIMBS] = 1;
        assert!(!magnitude_fits_1024(&m, false));
        assert!(!magnitude_fits_1024(&m, true));
    }

    #[test]
    fn pow_large_in_range_no_false_overflow() {
        // 2^1000 < 2^1023: single bit at index 1000, positive.
        let p = pow2(1000);
        unsafe {
            let limbs = (*p).limbs;
            assert_eq!(limbs[1000 / 64], 1u64 << (1000 % 64));
            assert!(!is_negative(&limbs));
            for (i, &l) in limbs.iter().enumerate() {
                if i != 1000 / 64 {
                    assert_eq!(l, 0, "limb {i} should be zero");
                }
            }
        }
        // (2^300)^3 = 2^900: the guarded loop must not throw on the final
        // unused base squaring (2^300)^4 = 2^1200.
        let cube = js_bigint_pow(pow2(300), js_bigint_from_i64(3));
        unsafe {
            assert_eq!((*cube).limbs[900 / 64], 1u64 << (900 % 64));
        }
    }

    #[test]
    fn mul_large_in_range_no_false_overflow() {
        // (2^500) * (2^500) = 2^1000 via the schoolbook slow path.
        let c = js_bigint_mul(pow2(500), pow2(500));
        unsafe {
            assert_eq!((*c).limbs[1000 / 64], 1u64 << (1000 % 64));
        }
    }

    #[test]
    fn add_sub_large_in_range_no_false_overflow() {
        // 2^1000 + 2^999 = bits 1000 and 999 set (both in limb 15).
        let sum = js_bigint_add(pow2(1000), pow2(999));
        unsafe {
            let limbs = (*sum).limbs;
            assert_eq!(
                limbs[999 / 64],
                (1u64 << (999 % 64)) | (1u64 << (1000 % 64))
            );
        }
        // 2^1000 - 2^999 = 2^999.
        let diff = js_bigint_sub(pow2(1000), pow2(999));
        unsafe {
            assert_eq!((*diff).limbs[999 / 64], 1u64 << (999 % 64));
        }
    }

    #[test]
    fn shl_large_in_range_no_false_overflow() {
        // 1n << 1022n = 2^1022, still positive and in range.
        let r = js_bigint_shl(js_bigint_from_i64(1), js_bigint_from_i64(1022));
        unsafe {
            let limbs = (*r).limbs;
            assert_eq!(limbs[1022 / 64], 1u64 << (1022 % 64));
            assert!(!is_negative(&limbs));
        }
    }

    #[test]
    fn from_string_radix_hex_ignores_excess_leading_zeros() {
        // 304 hex digits: 300 leading zeros + "beef". More than 256 digits, but
        // the excess are all zero, so the value (0xbeef) fits and must NOT
        // false-throw on the >1024-bit stream (regression for the CodeRabbit
        // finding on #6073 — leftover high-order zeros are harmless).
        let s = format!("{}beef", "0".repeat(300));
        let bi = js_bigint_from_string_radix(s.as_ptr(), s.len() as u32, 16);
        unsafe {
            let limbs = (*bi).limbs;
            assert_eq!(limbs[0], 0xbeef);
            for &l in &limbs[1..] {
                assert_eq!(l, 0);
            }
        }

        // Decimal leading zeros likewise must not throw (the long-mult path
        // never carries out of the top limb while the magnitude stays small).
        assert_eq!(parse("000000123").unwrap(), 123);
    }
}

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    /// #1781: `BigInt("123")` — a numeric string <= 5 bytes is an inline SSO
    /// value (tag 0x7FF9). `is_string()` is STRING_TAG-only, so pre-fix it
    /// fell through to the `value as i64` arm (the SSO f64 is NaN → 0), and
    /// `BigInt("123")` produced `0n`. Route through the unified decoder.
    #[test]
    fn bigint_from_f64_parses_sso_numeric_strings() {
        for s in ["0", "1", "42", "123", "12345"] {
            let v = crate::value::JSValue::try_short_string(s.as_bytes())
                .expect("numeric string <= 5 bytes encodes as inline SSO");
            assert!(v.is_short_string(), "{s:?} should be an inline SSO value");
            let bi = js_bigint_from_f64(f64::from_bits(v.bits()));
            assert!(!bi.is_null(), "null BigInt for {s:?}");
            let out = js_bigint_to_string(bi);
            let got = unsafe {
                let len = (*out).byte_len as usize;
                let data =
                    (out as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                std::str::from_utf8(std::slice::from_raw_parts(data, len))
                    .unwrap()
                    .to_string()
            };
            assert_eq!(got, s, "BigInt({s:?}) mismatch");
        }
    }
}
