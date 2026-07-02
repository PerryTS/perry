//! Character-level access: charCodeAt, charAt, codePointAt, fromCharCode,
//! fromCodePoint, at, plus spread-into-array (`toCharArray`).

use super::*;

/// JS index coercion for the String character-access methods (#2787).
/// Applies `ToIntegerOrInfinity`: a non-numeric argument is first run through
/// the full `ToNumber` (`js_number_coerce`) so an object index with a custom
/// `valueOf`/`toString` (`"lego".charAt({toString:()=>1})` → `"e"`), a numeric
/// string (`"1"`), a boolean, `null`, etc. coerce per spec — and a `Symbol`
/// index throws `TypeError` (ToNumber(Symbol)). `undefined`/`NaN` map to `0`;
/// finite values truncate toward zero; the result is clamped into `i32` so the
/// integer-index helpers below see a safe value (a far-out-of-range magnitude
/// clamps to a still-OOB index, which the helpers already handle). Codegen
/// routes the raw NaN-boxed index through here instead of `fptosi`, which is
/// undefined behavior on a NaN.
#[no_mangle]
pub extern "C" fn js_string_index_to_i32(index: f64) -> i32 {
    let jsval = crate::value::JSValue::from_bits(index.to_bits());
    // Fast path: a real number / int32 needs no ToNumber. Anything else
    // (object, string, bool, null, undefined, bigint, symbol) goes through
    // `ToNumber` first (may throw on Symbol, may run user `valueOf`/`toString`).
    let n = if jsval.is_int32() {
        jsval.as_int32() as f64
    } else if jsval.is_number() {
        index
    } else {
        crate::builtins::js_number_coerce(index)
    };
    if n.is_nan() {
        return 0;
    }
    let truncated = n.trunc();
    if truncated <= i32::MIN as f64 {
        i32::MIN
    } else if truncated >= i32::MAX as f64 {
        i32::MAX
    } else {
        truncated as i32
    }
}

/// `end`-argument coercion for `slice`/`substring`. Per ECMA-262 §22.1.3.20 /
/// §22.1.3.24, an `undefined` `end` (whether the arg is omitted or explicitly
/// `undefined`) means "to the end of the string" — `len`, NOT `ToInteger(
/// undefined) === 0`. So `"abc".substring(0, undefined)` is `"abc"`, not `""`.
/// Any other value goes through the ordinary `ToIntegerOrInfinity`.
#[no_mangle]
pub extern "C" fn js_string_end_index_to_i32(value: f64, len: i32) -> i32 {
    if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        return len;
    }
    js_string_index_to_i32(value)
}

/// Get character code at index (returns UTF-16 code unit, or NaN if out of bounds).
/// Index is in UTF-16 code units (matches JS spec). For ASCII strings this is
/// equivalent to byte indexing; for multi-byte UTF-8 we walk codepoints without
/// allocating — the old `encode_utf16().collect()` path made hashing a 68 MB
/// string O(n²) (issue #65).
#[no_mangle]
pub extern "C" fn js_string_char_code_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) || index < 0 {
        return f64::NAN;
    }

    let u16len = unsafe { (*s).utf16_len } as usize;
    let idx = index as usize;
    if idx >= u16len {
        return f64::NAN;
    }

    // ASCII fast path: byte_len == utf16_len means every byte is one
    // UTF-16 code unit. Direct byte index, no scan, no allocation.
    if is_ascii_string(s) {
        unsafe {
            return *string_data(s).add(idx) as f64;
        }
    }

    // Non-ASCII: walk codepoints counting UTF-16 units. Allocation-free.
    let str_data = string_as_str(s);
    let mut utf16_pos = 0usize;
    for ch in str_data.chars() {
        let clen = ch.len_utf16();
        if utf16_pos + clen > idx {
            if clen == 1 {
                return ch as u32 as f64;
            }
            let mut buf = [0u16; 2];
            ch.encode_utf16(&mut buf);
            return buf[idx - utf16_pos] as f64;
        }
        utf16_pos += clen;
    }
    f64::NAN
}

/// `s[key]` indexed read with ECMAScript CanonicalNumericIndexString semantics
/// (#3987): returns the single-UTF-16-code-unit string at `key` only when `key`
/// is `"length"` or a canonical array index — a non-negative integer (or a
/// numeric string that round-trips, e.g. `"1"`) within `[0, length)`. Other
/// keys return `undefined`: `NaN`, `Infinity`, negatives, fractions like
/// `1.5`, out-of-range indices, non-canonical strings like `"01"` / `" 1"` /
/// `"1.0"`, and non-index property names. Previously codegen `fptosi`'d the
/// key and called `js_string_char_at`, which truncated `1.5`→`1`, mapped
/// `NaN`→`0`, returned `""` (not `undefined`) for OOB/negatives, and
/// mis-resolved string keys.
#[no_mangle]
pub extern "C" fn js_string_index_get(s: *const StringHeader, key: f64) -> f64 {
    const UNDEFINED: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);
    if !is_valid_string_ptr(s) {
        return UNDEFINED;
    }
    let len = unsafe { (*s).utf16_len } as u64;
    let jsval = crate::value::JSValue::from_bits(key.to_bits());
    let raw_key_bits = key.to_bits();
    let raw_key_ptr = raw_key_bits as *const StringHeader;

    let idx: u64 = if jsval.is_int32() {
        let i = jsval.as_int32();
        if i < 0 {
            return UNDEFINED;
        }
        i as u64
    } else if jsval.is_number() {
        // Real double: only a finite, non-negative integer is an array index.
        if !key.is_finite() || key < 0.0 || key.fract() != 0.0 {
            return UNDEFINED;
        }
        key as u64 // saturating; an out-of-range magnitude fails the bound below
    } else if (raw_key_bits >> 48) == 0 && is_valid_string_ptr(raw_key_ptr) {
        let key_content = string_as_str(raw_key_ptr).to_string();
        if key_content == "length" {
            return len as f64;
        }
        match canonical_string_index(&key_content) {
            Some(i) => i,
            None => return UNDEFINED,
        }
    } else {
        let key_content = if jsval.is_any_string() {
            let Some(content) = crate::builtins::jsvalue_string_content(key) else {
                return UNDEFINED;
            };
            content
        } else {
            let direct_key_ptr = crate::value::js_jsvalue_to_string(key);
            if is_valid_string_ptr(direct_key_ptr) {
                string_as_str(direct_key_ptr).to_string()
            } else {
                let property_key = unsafe { crate::object::js_to_property_key(key) };
                if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
                    return UNDEFINED;
                }
                let key_ptr = crate::value::js_jsvalue_to_string(property_key);
                if !is_valid_string_ptr(key_ptr) {
                    return UNDEFINED;
                }
                string_as_str(key_ptr).to_string()
            }
        };
        if key_content == "length" {
            return len as f64;
        }
        match canonical_string_index(&key_content) {
            Some(i) => i,
            None => return UNDEFINED,
        }
    };

    if idx >= len {
        return UNDEFINED;
    }
    let ptr = js_string_char_at(s, idx as i32);
    crate::value::js_nanbox_string(ptr as i64)
}

/// Parse a property-key string into a canonical array index per
/// `CanonicalNumericIndexString`: the string must equal the exact `ToString` of
/// the resulting non-negative integer, so `"0"`→0 and `"12"`→12 are canonical
/// but `"01"`, `"1.0"`, `"+1"`, `" 1"`, `"1e0"`, and `""` are not. Indices must
/// be below `2^32 - 1` (the array-index ceiling).
fn canonical_string_index(s: &str) -> Option<u64> {
    if s == "0" {
        return Some(0);
    }
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] == b'0' || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = s.parse().ok()?;
    if n >= u32::MAX as u64 {
        return None;
    }
    Some(n)
}

/// Get character at UTF-16 code unit index (returns single-character string).
/// For a BMP character this returns the character itself; for a surrogate half
/// of an astral character this returns the lone surrogate (matching JS behavior).
#[no_mangle]
pub extern "C" fn js_string_char_at(s: *const StringHeader, index: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) || index < 0 {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    let u16len = unsafe { (*s).utf16_len };
    if index as u32 >= u16len {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    // ASCII fast path: skip utf16_len scan
    if is_ascii_string(s) {
        unsafe {
            let data = string_data(s);
            let char_ptr = data.add(index as usize);
            return js_string_from_ascii_bytes(char_ptr, 1);
        }
    }

    // UTF-16 path: walk the raw (WTF-8) bytes counting UTF-16 code units and
    // return the single code unit at `index`. Astral code points split into
    // their surrogate halves, so `"😀"[0]` is the lone high surrogate (length
    // 1) — matching JS UTF-16 indexing (#4793). Walking bytes directly (rather
    // than `str::chars()`) keeps this sound on inputs that already hold lone
    // surrogates.
    let target = index as usize;
    let bytes = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) };
    let mut u16_pos = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Decode one WTF-8 sequence into its code point and UTF-16 width.
        let (seq_len, units, cp) = if b < 0x80 {
            (1usize, 1usize, b as u32)
        } else if b < 0xE0 {
            let b1 = bytes.get(i + 1).copied().unwrap_or(0);
            (2, 1, ((b as u32 & 0x1F) << 6) | (b1 as u32 & 0x3F))
        } else if b < 0xF0 {
            let b1 = bytes.get(i + 1).copied().unwrap_or(0);
            let b2 = bytes.get(i + 2).copied().unwrap_or(0);
            (
                3,
                1,
                ((b as u32 & 0x0F) << 12) | ((b1 as u32 & 0x3F) << 6) | (b2 as u32 & 0x3F),
            )
        } else {
            let b1 = bytes.get(i + 1).copied().unwrap_or(0);
            let b2 = bytes.get(i + 2).copied().unwrap_or(0);
            let b3 = bytes.get(i + 3).copied().unwrap_or(0);
            (
                4,
                2,
                ((b as u32 & 0x07) << 18)
                    | ((b1 as u32 & 0x3F) << 12)
                    | ((b2 as u32 & 0x3F) << 6)
                    | (b3 as u32 & 0x3F),
            )
        };
        if target < u16_pos + units {
            let unit = if units == 2 {
                // Astral: emit the requested surrogate half.
                let v = cp - 0x10000;
                if target == u16_pos {
                    0xD800 + (v >> 10) as u16
                } else {
                    0xDC00 + (v & 0x3FF) as u16
                }
            } else {
                cp as u16
            };
            return string_from_code_unit(unit);
        }
        u16_pos += units;
        i += seq_len;
    }
    js_string_from_bytes(std::ptr::null(), 0)
}

/// Split a string into an array of single-character strings.
/// Used by the spread operator: `[..."hello"]` → `["h","e","l","l","o"]`.
/// JS spread iterates by codepoints (not UTF-16 units), so "😀" → ["😀"] (1 element).
/// Returns an ArrayHeader pointer with NaN-boxed STRING_TAG elements.
#[no_mangle]
pub extern "C" fn js_string_to_char_array(s: i64) -> i64 {
    let str_ptr = (s as u64 & crate::value::POINTER_MASK) as *const StringHeader;
    if str_ptr.is_null() || !is_valid_string_ptr(str_ptr) {
        return crate::array::js_array_alloc(0) as i64;
    }
    let str_data = string_as_str(str_ptr);
    let char_count = str_data.chars().count();
    let arr = crate::array::js_array_alloc_with_length(char_count as u32);
    let elements = unsafe { (arr as *mut u8).add(8) as *mut f64 };
    for (i, ch) in str_data.chars().enumerate() {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        let ch_ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
        let nanboxed =
            f64::from_bits(crate::value::STRING_TAG | (ch_ptr as u64 & crate::value::POINTER_MASK));
        unsafe {
            // GC_STORE_AUDIT(BARRIERED): char array slot is immediately recorded via note_array_slot.
            *elements.add(i) = nanboxed;
            crate::array::note_array_slot(arr, i, nanboxed.to_bits());
        }
    }
    arr as i64
}

/// JS `ToUint16` for `String.fromCharCode` (#2788): a non-finite value
/// (`NaN`/`±Inf`) maps to `0`; otherwise truncate toward zero and reduce
/// modulo 2^16 into `[0, 65535]`. `rem_euclid` keeps the result non-negative,
/// so `-1` wraps to `65535` and `0x1F600` wraps to `0xF600`.
fn to_uint16(code: f64) -> u16 {
    if !code.is_finite() {
        return 0;
    }
    code.trunc().rem_euclid(65536.0) as u16
}

/// `String.fromCharCode` per-argument coercion (ECMA-262 §22.1.2.1):
/// `nextCU = ToUint16(next)`, where `ToUint16` first applies the abstract
/// `ToNumber`. A bare numeric value short-circuits; everything else is run
/// through the full `ToNumber` so a boxed `new Boolean(true)` / a `{ valueOf }`
/// object coerce (→ 1 / their numeric value) instead of mapping to `0`. A
/// `BigInt` throws `TypeError` (abstract `ToNumber(BigInt)`, unlike the lenient
/// `Number(bigint)`); a `Symbol` throws via `js_number_coerce`.
fn fromcharcode_arg_to_uint16(code: f64) -> u16 {
    let jv = crate::value::JSValue::from_bits(code.to_bits());
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        code
    } else if jv.is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    } else {
        crate::builtins::js_number_coerce(code)
    };
    to_uint16(n)
}

/// Generic 3-byte WTF-8 encoding of a single BMP code point / lone surrogate
/// (`0x800..=0xFFFF`). For a lone surrogate (`0xD800..=0xDFFF`) this is the
/// same byte layout the UTF-8 encoder would produce if surrogates were scalar
/// values, which is exactly the WTF-8 representation Perry round-trips.
#[inline]
fn encode_3byte_wtf8(unit: u16) -> [u8; 3] {
    [
        0xE0 | (unit >> 12) as u8,
        0x80 | ((unit >> 6) & 0x3F) as u8,
        0x80 | (unit & 0x3F) as u8,
    ]
}

/// Build a fresh string holding exactly one UTF-16 code unit. Lone surrogates
/// (`0xD800..=0xDFFF`) are encoded as WTF-8 and the result is flagged
/// `HAS_LONE_SURROGATES` (so `isWellFormed()` / `JSON.stringify` see them);
/// every other unit is ordinary UTF-8. This is the round-tripping replacement
/// for the old `char::from_u32(..).unwrap_or('\u{FFFD}')` lossy path.
pub(crate) fn string_from_code_unit(unit: u16) -> *mut StringHeader {
    if unit < 0x80 {
        let byte = unit as u8;
        return js_string_from_bytes(&byte as *const u8, 1);
    }
    if (0xD800..=0xDFFF).contains(&unit) {
        let buf = encode_3byte_wtf8(unit);
        return crate::string::js_string_from_wtf8_bytes(buf.as_ptr(), 3);
    }
    // BMP, non-surrogate → a valid Unicode scalar value.
    let ch = unsafe { char::from_u32_unchecked(unit as u32) };
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

/// Append the WTF-8/UTF-8 bytes of one UTF-16 code unit to `out`, returning
/// `true` if a lone surrogate was appended (so the caller knows to use the
/// WTF-8 string-construction path that sets `HAS_LONE_SURROGATES`).
#[inline]
pub(super) fn push_code_unit_wtf8(out: &mut Vec<u8>, unit: u16) -> bool {
    if unit < 0x80 {
        out.push(unit as u8);
        false
    } else if (0xD800..=0xDFFF).contains(&unit) {
        out.extend_from_slice(&encode_3byte_wtf8(unit));
        true
    } else {
        let ch = unsafe { char::from_u32_unchecked(unit as u32) };
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        false
    }
}

/// Append the WTF-8/UTF-8 bytes of one (range-validated) code point to `out`,
/// for `String.fromCodePoint`. A BMP surrogate code point is emitted as a lone
/// surrogate (returns `true`); astral code points encode as ordinary UTF-8.
#[inline]
fn push_code_point_wtf8(out: &mut Vec<u8>, cp: u32) -> bool {
    if cp <= 0xFFFF {
        push_code_unit_wtf8(out, cp as u16)
    } else {
        let ch = unsafe { char::from_u32_unchecked(cp) };
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        false
    }
}

/// Encode a sequence of code points / UTF-16 code units as canonical WTF-8.
/// A high surrogate immediately followed by a low surrogate is combined into
/// its astral code point and emitted as ordinary 4-byte UTF-8 (so
/// `String.fromCharCode(0xD83D, 0xDE00)` is the emoji, with `codePointAt(0)`
/// = 0x1F600). Only a *genuinely lone* surrogate stays as a 3-byte WTF-8
/// sequence. Returns the bytes and whether any lone surrogate was emitted
/// (so the caller can pick the flag-setting construction path). (#4793)
fn encode_code_points_wtf8(cps: &[u32]) -> (Vec<u8>, bool) {
    let mut out: Vec<u8> = Vec::with_capacity(cps.len());
    let mut has_lone_surrogate = false;
    let mut i = 0;
    while i < cps.len() {
        let cp = cps[i];
        if (0xD800..=0xDBFF).contains(&cp)
            && i + 1 < cps.len()
            && (0xDC00..=0xDFFF).contains(&cps[i + 1])
        {
            let astral = 0x10000 + ((cp - 0xD800) << 10) + (cps[i + 1] - 0xDC00);
            let ch = unsafe { char::from_u32_unchecked(astral) };
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            i += 2;
            continue;
        }
        has_lone_surrogate |= push_code_point_wtf8(&mut out, cp);
        i += 1;
    }
    (out, has_lone_surrogate)
}

/// Create a string from a character code (String.fromCharCode).
/// The argument is coerced with `ToUint16` (#2788), so out-of-range and
/// negative values wrap modulo 65536 rather than returning `""`. Codegen
/// passes the raw NaN-boxed `f64` (not `fptosi`, which is UB on a NaN).
/// Lone surrogates (`0xD800..=0xDFFF`) round-trip via WTF-8 (#4793).
#[no_mangle]
pub extern "C" fn js_string_from_char_code(code: f64) -> *mut StringHeader {
    let unit = fromcharcode_arg_to_uint16(code);
    string_from_code_unit(unit)
}

/// Create a string from a spread/apply argument source:
/// `String.fromCharCode(...arrayLike)` / `String.fromCharCode.apply(_, bytes)`.
#[no_mangle]
pub extern "C" fn js_string_from_char_code_array(value: f64) -> *mut StringHeader {
    let arr = crate::object::js_array_like_to_array(value);
    if arr.is_null() {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    let len = crate::array::js_array_length(arr) as usize;
    if len == 0 {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    let mut cps: Vec<u32> = Vec::with_capacity(len);
    for i in 0..len {
        let unit = fromcharcode_arg_to_uint16(crate::array::js_array_get_f64(arr, i as u32));
        cps.push(unit as u32);
    }
    let (out, has_lone_surrogate) = encode_code_points_wtf8(&cps);
    if has_lone_surrogate {
        crate::string::js_string_from_wtf8_bytes(out.as_ptr(), out.len() as u32)
    } else {
        js_string_from_bytes(out.as_ptr(), out.len() as u32)
    }
}

/// Throw `RangeError: Invalid code point <n>` for `String.fromCodePoint`,
/// matching Node's message. Rust's `f64` Display drops the trailing `.0` for
/// integer-valued floats (`1114112.0` -> "1114112") and keeps fractional
/// digits (`3.14` -> "3.14"), so it matches JS number formatting here.
fn throw_invalid_code_point(code: f64) -> ! {
    let msg = format!("Invalid code point {}", code);
    let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_rangeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Create a string from a Unicode code point (String.fromCodePoint).
/// Supports the full Unicode range (0..=0x10FFFF), unlike fromCharCode
/// (0..=0xFFFF). Per spec (#2788), a negative, non-integer, or `> 0x10FFFF`
/// argument throws `RangeError`. Codegen passes the raw NaN-boxed `f64` so the
/// non-integer/non-finite cases are observable (a prior `fptosi` truncated
/// `3.14` to `3` and silently produced a character).
#[no_mangle]
pub extern "C" fn js_string_from_code_point(code: f64) -> *mut StringHeader {
    if !code.is_finite() || code.fract() != 0.0 || code < 0.0 || code > 0x10FFFF as f64 {
        throw_invalid_code_point(code);
    }
    let cp = code as u32;
    // A surrogate code point (`0xD800..=0xDFFF`) is a valid `fromCodePoint`
    // argument but not a Rust `char`; `string_from_code_unit` round-trips it
    // through WTF-8 instead of substituting U+FFFD (#4793). Astral code points
    // (`> 0xFFFF`) are ordinary scalar values.
    if cp <= 0xFFFF {
        return string_from_code_unit(cp as u16);
    }
    let ch = unsafe { char::from_u32_unchecked(cp) };
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

/// `String.fromCodePoint(...codePoints)` — variadic form. Builds a string from
/// an array-like of code points, validating each (RangeError on a non-integer /
/// negative / > 0x10FFFF value) per ECMAScript. A lone surrogate emits U+FFFD
/// (WTF-8 categorical gap), matching `js_string_from_code_point`. Used by the
/// reified `String.fromCodePoint` constructor static so value reads / spread
/// calls work. (#4627)
pub fn js_string_from_code_point_array(value: f64) -> *mut StringHeader {
    let arr = crate::object::js_array_like_to_array(value);
    if arr.is_null() {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let len = crate::array::js_array_length(arr) as usize;
    let mut cps: Vec<u32> = Vec::with_capacity(len);
    for i in 0..len {
        let code = crate::array::js_array_get_f64(arr, i as u32);
        if !code.is_finite() || code.fract() != 0.0 || code < 0.0 || code > 0x10FFFF as f64 {
            throw_invalid_code_point(code);
        }
        cps.push(code as u32);
    }
    let (out, has_lone_surrogate) = encode_code_points_wtf8(&cps);
    if has_lone_surrogate {
        crate::string::js_string_from_wtf8_bytes(out.as_ptr(), out.len() as u32)
    } else {
        js_string_from_bytes(out.as_ptr(), out.len() as u32)
    }
}

/// String.prototype.at(index) — supports negative indices.
/// Returns NaN-boxed single-char string, or NaN-boxed undefined if out of bounds.
/// Index is in UTF-16 code units (matches JS spec).
#[no_mangle]
pub extern "C" fn js_string_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let str_data = string_as_str(s);
    let utf16: Vec<u16> = str_data.encode_utf16().collect();
    let len = utf16.len() as i32;
    let resolved = if index < 0 { len + index } else { index };
    if resolved < 0 || resolved >= len {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // #2948: `at()` is UTF-16 *code-unit* based, exactly like `charAt`/`[i]` —
    // NOT code-point based like `codePointAt`. For an astral char stored as a
    // surrogate pair, each index returns the lone surrogate code unit (Node:
    // `"😀".at(0).charCodeAt(0) === 0xd83d`), it does NOT decode the pair.
    let unit = utf16[resolved as usize];
    // Encode the single code unit. A lone surrogate (0xD800..=0xDFFF) is not a
    // valid Rust `char`, so it materializes as U+FFFD — the documented WTF-8 /
    // lone-surrogate categorical gap (same shim as `fromCharCode`). BMP units
    // round-trip exactly.
    let ch = char::from_u32(unit as u32).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

/// String.prototype.codePointAt(index) — returns the Unicode code point at the given
/// UTF-16 code unit position, or NaN-boxed undefined if out of bounds.
#[no_mangle]
pub extern "C" fn js_string_code_point_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) || index < 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let u16len = unsafe { (*s).utf16_len } as usize;
    let idx = index as usize;
    if idx >= u16len {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    // ASCII fast path — identical to charCodeAt's.
    if is_ascii_string(s) {
        unsafe {
            return *string_data(s).add(idx) as f64;
        }
    }

    // Non-ASCII: walk codepoints without allocating a Vec<u16>.
    let str_data = string_as_str(s);
    let mut utf16_pos = 0usize;
    for ch in str_data.chars() {
        let clen = ch.len_utf16();
        if utf16_pos + clen > idx {
            if clen == 1 || utf16_pos == idx {
                // Either a BMP char, or the start of a surrogate pair
                // (which is the whole codepoint per the spec).
                return ch as u32 as f64;
            }
            // Index lands on the low surrogate — return the bare unit.
            let mut buf = [0u16; 2];
            ch.encode_utf16(&mut buf);
            return buf[idx - utf16_pos] as f64;
        }
        utf16_pos += clen;
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
