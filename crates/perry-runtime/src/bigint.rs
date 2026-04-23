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

/// Check if a BigInt pointer is valid (not null, not NaN-boxed, in user address space).
/// Protects against accidental use of NaN-boxed values (e.g., TAG_UNDEFINED) as BigInt pointers.
#[inline(always)]
fn is_valid_bigint_ptr(p: *const BigIntHeader) -> bool {
    let bits = p as usize;
    // Valid heap pointers: non-null, >= 0x10000, upper 16 bits must be 0 (48-bit address space)
    bits >= 0x10000 && (bits as u64) >> 48 == 0
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
        if (raw as usize) < 0x10000 { return std::ptr::null(); }
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

/// Create a BigInt from a u64 value
#[no_mangle]
pub extern "C" fn js_bigint_from_u64(value: u64) -> *mut BigIntHeader {
    let ptr = bigint_alloc();
    unsafe {
        (*ptr).limbs = ZERO_LIMBS;
        (*ptr).limbs[0] = value;
    }
    ptr
}

/// Create a BigInt from a signed i64 value
#[no_mangle]
pub extern "C" fn js_bigint_from_i64(value: i64) -> *mut BigIntHeader {
    let ptr = bigint_alloc();
    unsafe {
        if value >= 0 {
            (*ptr).limbs = ZERO_LIMBS;
            (*ptr).limbs[0] = value as u64;
        } else {
            // Two's complement for negative numbers: sign-extend with u64::MAX
            (*ptr).limbs = [0u64; BIGINT_LIMBS];
            (*ptr).limbs[0] = value as u64;
            for k in 1..BIGINT_LIMBS {
                (*ptr).limbs[k] = u64::MAX;
            }
        }
        ptr
    }
}

/// Create a BigInt from an f64 value (BigInt() coercion)
/// Converts f64 to i64 then to BigInt. Handles NaN-boxed values too.
#[no_mangle]
pub extern "C" fn js_bigint_from_f64(value: f64) -> *mut BigIntHeader {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());

    // If already a BigInt (NaN-boxed), just return the pointer
    if jsval.is_bigint() {
        return jsval.as_bigint_ptr() as *mut BigIntHeader;
    }

    // If it's an INT32 (NaN-boxed i32), extract and convert
    if jsval.is_int32() {
        let int_value = jsval.as_int32() as i64;
        return js_bigint_from_i64(int_value);
    }

    // If it's a string, parse as BigInt (e.g., BigInt("1000000"))
    if jsval.is_string() {
        let ptr = jsval.as_string_ptr();
        if !ptr.is_null() {
            unsafe {
                let len = (*ptr).byte_len as u32;
                let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let result = js_bigint_from_string(data, len);
                return result;
            }
        }
        return js_bigint_from_i64(0);
    }

    // If it's undefined or null, return 0 (JavaScript throws TypeError, but we're lenient)
    if jsval.is_undefined() || jsval.is_null() {
        return js_bigint_from_i64(0);
    }

    // Convert f64 to BigInt
    let int_value = value as i64;
    js_bigint_from_i64(int_value)
}

/// Create a BigInt from a string (decimal or hex with 0x prefix)
#[no_mangle]
pub extern "C" fn js_bigint_from_string(data: *const u8, len: u32) -> *mut BigIntHeader {
    unsafe {
        let bytes = std::slice::from_raw_parts(data, len as usize);
        let s = std::str::from_utf8_unchecked(bytes);

        // Fast path: decimal string that fits in i64. Postgres `int8`
        // results, Node `Date.now()` timestamps, app IDs — the common
        // BigInt input in real code is well under 2^63. For those we
        // skip the per-digit 16-limb multiply (~300 u128 muls for a
        // 20-char input) and let Rust's native str→i64 handle parsing
        // in a single pass.
        //
        // `i64::from_str` returns Err on overflow / non-digit, and we
        // fall through to the general path so hex, floats-of-ints, and
        // arbitrary-precision still work exactly as before.
        if !s.starts_with("0x") && !s.starts_with("0X") {
            if let Ok(v) = s.parse::<i64>() {
                return js_bigint_from_i64(v);
            }
        }

        // Handle negative prefix
        let (is_negative, s) = if s.starts_with('-') {
            (true, &s[1..])
        } else {
            (false, s)
        };

        // Parse the string
        let (is_hex, s) = if s.starts_with("0x") || s.starts_with("0X") {
            (true, &s[2..])
        } else {
            (false, s)
        };

        let ptr = bigint_alloc();
        let mut limbs = ZERO_LIMBS;

        if is_hex {
            // Parse hex string
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
        } else {
            // Parse decimal string using long multiplication
            for c in s.chars() {
                if let Some(digit) = c.to_digit(10) {
                    // Multiply by 10 and add digit
                    let mut carry = digit as u64;
                    for limb in limbs.iter_mut() {
                        let product = (*limb as u128) * 10 + carry as u128;
                        *limb = product as u64;
                        carry = (product >> 64) as u64;
                    }
                }
            }
        }

        (*ptr).limbs = limbs;

        if is_negative && !limbs.iter().all(|&l| l == 0) {
            return js_bigint_neg(ptr);
        }
        ptr
    }
}

/// Create a BigInt from a string with a given radix (for BN.js compatibility)
/// Handles decimal (10), hex (16), and other bases.
#[no_mangle]
pub extern "C" fn js_bigint_from_string_radix(data: *const u8, len: u32, radix: i32) -> *mut BigIntHeader {
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

        let ptr = bigint_alloc();
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
        } else {
            // General radix parsing using long multiplication
            for c in s.chars() {
                let digit = match c {
                    '0'..='9' => (c as u64) - ('0' as u64),
                    'a'..='z' => (c as u64) - ('a' as u64) + 10,
                    'A'..='Z' => (c as u64) - ('A' as u64) + 10,
                    _ => continue,
                };
                if digit >= radix { continue; }
                let mut carry = digit;
                for limb in limbs.iter_mut() {
                    let product = (*limb as u128) * (radix as u128) + carry as u128;
                    *limb = product as u64;
                    carry = (product >> 64) as u64;
                }
            }
        }

        (*ptr).limbs = limbs;

        if is_negative && !limbs.iter().all(|&l| l == 0) {
            // Negate: two's complement
            return js_bigint_neg(ptr);
        }
        ptr
    }
}

/// Convert BigInt to a byte array (big-endian, for BN.toArrayLike/toArray)
/// Returns a buffer of the specified length, zero-padded on the left.
#[no_mangle]
pub extern "C" fn js_bigint_to_buffer(a: *const BigIntHeader, length: i32) -> *mut crate::buffer::BufferHeader {
    let a = clean_bigint_ptr(a);
    let length = if length <= 0 { 32 } else { length as usize };

    let result = crate::buffer::buffer_alloc(length as u32);
    unsafe {
        (*result).length = length as u32;
        let data = crate::buffer::buffer_data_mut(result);

        if !a.is_null() {
            // Extract bytes from limbs (little-endian in memory)
            let limbs = &(*a).limbs;
            let mut all_bytes = Vec::with_capacity(BIGINT_LIMBS * 8);
            for limb in limbs.iter() {
                all_bytes.extend_from_slice(&limb.to_le_bytes());
            }
            // Write in big-endian: pad on left with zeros
            let significant = all_bytes.len().min(length);
            // Zero-fill the output
            std::ptr::write_bytes(data, 0, length);
            // Copy bytes in big-endian order
            for i in 0..significant {
                *data.add(length - 1 - i) = all_bytes[i];
            }
        } else {
            std::ptr::write_bytes(data, 0, length);
        }
    }
    result
}

/// Check if BigInt is negative (MSB set in two's complement)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_is_negative(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    if a.is_null() { return 0; }
    unsafe {
        // In two's complement, negative numbers have MSB set in highest limb
        let msb = (*a).limbs[BIGINT_LIMBS - 1];
        if msb & (1u64 << 63) != 0 { 1 } else { 0 }
    }
}

/// Negate a BigInt (two's complement: flip all bits and add 1)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_neg(a: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    if a.is_null() { return bigint_alloc(); }
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = (*a).limbs;
        let mut result = ZERO_LIMBS;
        let mut carry = 1u64;

        for i in 0..BIGINT_LIMBS {
            let flipped = !a_limbs[i];
            let sum = (flipped as u128) + (carry as u128);
            result[i] = sum as u64;
            carry = (sum >> 64) as u64;
        }

        (*ptr).limbs = result;
        ptr
    }
}

/// Check if a BigInt is zero (all limbs are zero). Returns 1 for zero, 0 for non-zero.
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_is_zero(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    if a.is_null() { return 1; }
    unsafe {
        for i in 0..BIGINT_LIMBS {
            if (*a).limbs[i] != 0 { return 0; }
        }
        1
    }
}

/// Add two BigInts
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_add(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    if a.is_null() && b.is_null() { return bigint_alloc(); }
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let b_limbs = if b.is_null() { ZERO_LIMBS } else { (*b).limbs };
        let mut result = ZERO_LIMBS;
        let mut carry = 0u64;

        for i in 0..BIGINT_LIMBS {
            let sum = (a_limbs[i] as u128) + (b_limbs[i] as u128) + (carry as u128);
            result[i] = sum as u64;
            carry = (sum >> 64) as u64;
        }

        (*ptr).limbs = result;
        ptr
    }
}

/// Subtract two BigInts (a - b)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_sub(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let b_limbs = if b.is_null() { ZERO_LIMBS } else { (*b).limbs };
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

        (*ptr).limbs = result;
        ptr
    }
}

/// Multiply two BigInts
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_mul(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = std::hint::black_box(clean_bigint_ptr(a));
    let b = std::hint::black_box(clean_bigint_ptr(b));
    let ptr = bigint_alloc();
    unsafe {
        if a.is_null() || b.is_null() {
            (*ptr).limbs = ZERO_LIMBS;
            return ptr;
        }
        let a_limbs = std::hint::black_box((*a).limbs);
        let b_limbs = std::hint::black_box((*b).limbs);
        let mut result = ZERO_LIMBS;

        // School multiplication (keeping lower 1024 bits)
        for i in 0..BIGINT_LIMBS {
            let mut carry = 0u128;
            for j in 0..(BIGINT_LIMBS - i) {
                let product = (a_limbs[i] as u128) * (b_limbs[j] as u128)
                    + (result[i + j] as u128)
                    + carry;
                result[i + j] = product as u64;
                carry = product >> 64;
            }
        }

        (*ptr).limbs = std::hint::black_box(result);
        ptr
    }
}

/// Unsigned binary long division on magnitude limbs
fn unsigned_div_limbs(a: &[u64; BIGINT_LIMBS], b: &[u64; BIGINT_LIMBS]) -> ([u64; BIGINT_LIMBS], [u64; BIGINT_LIMBS]) {
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
            if remainder[j] > b[j] { break; }
            if remainder[j] < b[j] { ge = false; break; }
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
#[inline(never)]
pub extern "C" fn js_bigint_div(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let b_limbs = if b.is_null() { ZERO_LIMBS } else { (*b).limbs };

        // Division by zero: return 0 instead of panicking (panic can't unwind in extern "C")
        if b_limbs == ZERO_LIMBS {
            (*ptr).limbs = ZERO_LIMBS;
            return ptr;
        }

        let a_neg = is_negative(&a_limbs);
        let b_neg = is_negative(&b_limbs);

        // Get magnitudes
        let abs_a = if a_neg { negate_limbs(&a_limbs) } else { a_limbs };
        let abs_b = if b_neg { negate_limbs(&b_limbs) } else { b_limbs };

        let (quotient, _) = unsigned_div_limbs(&abs_a, &abs_b);

        // Result is negative if signs differ
        (*ptr).limbs = if a_neg != b_neg && quotient != ZERO_LIMBS {
            negate_limbs(&quotient)
        } else {
            quotient
        };
        ptr
    }
}

/// Modulo of two BigInts (a % b) — result has sign of dividend (like JavaScript)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_mod(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = std::hint::black_box(clean_bigint_ptr(a));
    let b = std::hint::black_box(clean_bigint_ptr(b));
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = std::hint::black_box(if a.is_null() { ZERO_LIMBS } else { (*a).limbs });
        let b_limbs = std::hint::black_box(if b.is_null() { ZERO_LIMBS } else { (*b).limbs });

        // Division by zero: return 0 instead of panicking (panic can't unwind in extern "C")
        if b_limbs == ZERO_LIMBS {
            (*ptr).limbs = ZERO_LIMBS;
            return ptr;
        }

        let a_neg = is_negative(&a_limbs);
        let b_neg = is_negative(&b_limbs);

        // Get magnitudes
        let abs_a = if a_neg { negate_limbs(&a_limbs) } else { a_limbs };
        let abs_b = if b_neg { negate_limbs(&b_limbs) } else { b_limbs };

        let (_, remainder) = unsigned_div_limbs(&abs_a, &abs_b);

        // Remainder has sign of dividend
        (*ptr).limbs = std::hint::black_box(if a_neg && remainder != ZERO_LIMBS {
            negate_limbs(&remainder)
        } else {
            remainder
        });
        ptr
    }
}

/// Power of two BigInts (a ** b) using binary exponentiation
/// Note: b is interpreted as a u64 (only lower 64 bits are used)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_pow(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        // Get exponent as u64 (only lower 64 bits)
        let exp = if b.is_null() { 0u64 } else { (*b).limbs[0] };

        if exp == 0 {
            // Anything to the power of 0 is 1
            (*ptr).limbs = ZERO_LIMBS;
            (*ptr).limbs[0] = 1;
            return ptr;
        }

        // Binary exponentiation
        let mut result = ZERO_LIMBS;
        result[0] = 1;
        let mut base = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let mut e = exp;

        while e > 0 {
            if e & 1 == 1 {
                result = mul_limbs(&result, &base);
            }
            base = mul_limbs(&base, &base);
            e >>= 1;
        }

        (*ptr).limbs = result;
        ptr
    }
}

/// Multiply two limb arrays (helper for pow)
fn mul_limbs(a: &[u64; BIGINT_LIMBS], b: &[u64; BIGINT_LIMBS]) -> [u64; BIGINT_LIMBS] {
    let mut result = ZERO_LIMBS;
    for i in 0..BIGINT_LIMBS {
        let mut carry = 0u128;
        for j in 0..(BIGINT_LIMBS - i) {
            let product = (a[i] as u128) * (b[j] as u128)
                + (result[i + j] as u128)
                + carry;
            result[i + j] = product as u64;
            carry = product >> 64;
        }
    }
    result
}

/// Left shift BigInt by b bits (a << b)
/// Note: b is interpreted as a u64 (only lower 64 bits are used)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_shl(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        let shift = if b.is_null() { 0usize } else { (*b).limbs[0] as usize };
        if shift >= BIGINT_BITS {
            (*ptr).limbs = ZERO_LIMBS;
            return ptr;
        }
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let mut result = ZERO_LIMBS;

        // Calculate full limb shifts and bit shifts within a limb
        let limb_shift = shift / 64;
        let bit_shift = (shift % 64) as u32;

        if bit_shift == 0 {
            // Simple case: only limb-aligned shift
            for i in limb_shift..BIGINT_LIMBS {
                result[i] = a_limbs[i - limb_shift];
            }
        } else {
            // General case: shift across limb boundaries
            for i in limb_shift..BIGINT_LIMBS {
                let src_idx = i - limb_shift;
                result[i] = a_limbs[src_idx] << bit_shift;
                if src_idx > 0 {
                    result[i] |= a_limbs[src_idx - 1] >> (64 - bit_shift);
                }
            }
        }

        (*ptr).limbs = result;
        ptr
    }
}

/// Right shift BigInt by b bits (a >> b)
/// Note: b is interpreted as a u64 (only lower 64 bits are used)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_shr(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {

    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let neg = is_negative(&a_limbs);
        // Fill value for sign extension: 0xFF..FF for negative, 0x00..00 for positive
        let fill: u64 = if neg { !0u64 } else { 0u64 };

        let shift = if b.is_null() { 0usize } else { (*b).limbs[0] as usize };
        if shift >= BIGINT_BITS {
            // Arithmetic: negative → all 1s (-1), positive → all 0s (0)
            (*ptr).limbs = [fill; BIGINT_LIMBS];
            return ptr;
        }

        let mut result = [fill; BIGINT_LIMBS];

        // Calculate full limb shifts and bit shifts within a limb
        let limb_shift = shift / 64;
        let bit_shift = (shift % 64) as u32;

        if bit_shift == 0 {
            // Simple case: only limb-aligned shift
            for i in 0..(BIGINT_LIMBS - limb_shift) {
                result[i] = a_limbs[i + limb_shift];
            }
        } else {
            // General case: shift across limb boundaries
            for i in 0..(BIGINT_LIMBS - limb_shift) {
                let src_idx = i + limb_shift;
                result[i] = a_limbs[src_idx] >> bit_shift;
                if src_idx + 1 < BIGINT_LIMBS {
                    result[i] |= a_limbs[src_idx + 1] << (64 - bit_shift);
                } else {
                    // Top limb: sign-extend the vacated bits
                    result[i] |= fill << (64 - bit_shift);
                }
            }
        }

        (*ptr).limbs = result;
        ptr
    }
}

/// Bitwise AND of two BigInts (a & b)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_and(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {

    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let b_limbs = if b.is_null() { ZERO_LIMBS } else { (*b).limbs };
        let mut result = ZERO_LIMBS;

        for i in 0..BIGINT_LIMBS {
            result[i] = a_limbs[i] & b_limbs[i];
        }

        (*ptr).limbs = result;
        ptr
    }
}

/// Bitwise OR of two BigInts (a | b)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_or(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let b_limbs = if b.is_null() { ZERO_LIMBS } else { (*b).limbs };
        let mut result = ZERO_LIMBS;
        for i in 0..BIGINT_LIMBS {
            result[i] = a_limbs[i] | b_limbs[i];
        }
        (*ptr).limbs = result;
        ptr
    }
}

/// Bitwise XOR of two BigInts (a ^ b)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_xor(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    let ptr = bigint_alloc();
    unsafe {
        let a_limbs = if a.is_null() { ZERO_LIMBS } else { (*a).limbs };
        let b_limbs = if b.is_null() { ZERO_LIMBS } else { (*b).limbs };
        let mut result = ZERO_LIMBS;
        for i in 0..BIGINT_LIMBS {
            result[i] = a_limbs[i] ^ b_limbs[i];
        }
        (*ptr).limbs = result;
        ptr
    }
}

/// Compare two BigInts (-1 if a < b, 0 if equal, 1 if a > b)
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_cmp(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    let a = std::hint::black_box(clean_bigint_ptr(a));
    let b = std::hint::black_box(clean_bigint_ptr(b));
    if a.is_null() || b.is_null() {
        return 0;
    }
    unsafe {
        compare_limbs(&(*a).limbs, &(*b).limbs)
    }
}

/// Check if two BigInts are equal
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_bigint_eq(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    if a.is_null() || b.is_null() {
        return if a == b { 1 } else { 0 }; // both null = equal, one null = not equal
    }
    unsafe {
        if (*a).limbs == (*b).limbs { 1 } else { 0 }
    }
}

/// Convert BigInt to f64 (may lose precision)
#[no_mangle]
#[inline(never)]
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
        if neg { -result } else { result }
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
    let radix = if radix < 2 || radix > 36 { 10 } else { radix };
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
        let ch = if digit < 10 { b'0' + digit } else { b'a' + (digit - 10) };
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
pub extern "C" fn js_bigint_to_string_radix(a: *const BigIntHeader, radix: i32) -> *mut crate::string::StringHeader {
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
        assert!((a_f64 - 1e39).abs() / 1e39 < 1e-15, "a parse wrong: {}", a_f64);
        assert!((b_f64 - 2e39).abs() / 2e39 < 1e-15, "b parse wrong: {}", b_f64);

        let c = js_bigint_mul(a, b);
        let c_f64 = js_bigint_to_f64(c);
        assert!((c_f64 - 2e78).abs() / 2e78 < 1e-15,
            "3L*3L multiply wrong: got {}, expected 2e78", c_f64);
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
        assert!(product_f64 > 1e80,
            "shifted*b too small: got {}, expected ~1.458e81", product_f64);
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
        assert!((q_f64 - 2e39).abs() / 2e39 < 1e-15,
            "division wrong: got {}, expected 2e39", q_f64);
    }
}
