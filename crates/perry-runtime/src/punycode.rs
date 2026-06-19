//! `node:punycode` — the deprecated Punycode/IDNA conversion module (#2513).
//!
//! A direct port of the reference `punycode.js` (RFC 3492 Bootstring), exposed
//! as C-ABI runtime helpers. Top-level string surface only:
//! `decode`/`encode`/`toASCII`/`toUnicode`/`version`. The `ucs2.decode` /
//! `ucs2.encode` array helpers (sub-namespace + array marshalling) are a
//! separate follow-up.

use crate::string::{string_data, StringHeader};
use crate::url::{create_string_f64, get_string_content};
use crate::value::JSValue;
use std::slice;

/// Bundled punycode.js version Node reports via `punycode.version`.
pub const PUNYCODE_VERSION: &str = "2.1.0";

// RFC 3492 Bootstring parameters.
const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;
const DELIMITER: char = '-';
const ERR_INVALID_INPUT: &str = "Invalid input";
const ERR_NON_BASIC: &str = "Illegal input >= 0x80 (not a basic code point)";

#[cold]
fn throw_range_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_rangeerror_new(msg);
    let bits = JSValue::pointer(error as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

#[cold]
fn throw_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_typeerror_new(msg);
    let bits = JSValue::pointer(error as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

#[cold]
fn throw_invalid_code_point(code: f64) -> ! {
    throw_range_error(&format!("Invalid code point {}", code))
}

/// Bias adaptation (RFC 3492 §6.1).
fn adapt(mut delta: u32, num_points: u32, first_time: bool) -> u32 {
    delta = if first_time { delta / DAMP } else { delta / 2 };
    delta += delta / num_points;
    let mut k = 0u32;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (BASE - TMIN + 1) * delta / (delta + SKEW)
}

/// Map a base-36 digit (0..36) to its basic code point.
fn digit_to_basic(digit: u32) -> char {
    // 0..25 -> 'a'..'z', 26..35 -> '0'..'9'
    let c = if digit < 26 {
        digit + 97
    } else {
        digit - 26 + 48
    };
    char::from_u32(c).unwrap_or('?')
}

/// Map a basic code point to its base-36 digit value (or None if not a digit).
fn basic_to_digit(cp: u32) -> Option<u32> {
    match cp {
        0x30..=0x39 => Some(cp - 0x30 + 26), // '0'..'9' -> 26..35
        0x41..=0x5A => Some(cp - 0x41),      // 'A'..'Z' -> 0..25
        0x61..=0x7A => Some(cp - 0x61),      // 'a'..'z' -> 0..25
        _ => None,
    }
}

/// `punycode.decode(string)` — decode a Punycode string (no `xn--` prefix) to
/// its Unicode form.
fn decode(input: &str) -> Result<String, &'static str> {
    let input_cps: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let mut output: Vec<u32> = Vec::new();

    let mut n = INITIAL_N;
    let mut i: u32 = 0;
    let mut bias = INITIAL_BIAS;

    // Handle the basic code points: copy everything before the last delimiter.
    let basic = input.rfind(DELIMITER).map(|b| {
        // byte index -> count of chars before it (DELIMITER is ASCII, 1 byte)
        input[..b].chars().count()
    });
    let basic_len = basic.unwrap_or(0);
    if basic == Some(0) {
        return Err(ERR_INVALID_INPUT);
    }
    for &cp in input_cps.iter().take(basic_len) {
        if cp >= 0x80 {
            return Err(ERR_NON_BASIC);
        }
        output.push(cp);
    }

    // Start consuming after the delimiter (if any).
    let mut idx = if basic.is_some() { basic_len + 1 } else { 0 };

    while idx < input_cps.len() {
        let oldi = i;
        let mut w: u32 = 1;
        let mut k = BASE;
        loop {
            if idx >= input_cps.len() {
                return Err(ERR_INVALID_INPUT);
            }
            let digit = match basic_to_digit(input_cps[idx]) {
                Some(d) => d,
                None => return Err(ERR_INVALID_INPUT),
            };
            idx += 1;
            let increment = digit.checked_mul(w).ok_or(ERR_INVALID_INPUT)?;
            i = i.checked_add(increment).ok_or(ERR_INVALID_INPUT)?;
            let t = if k <= bias {
                TMIN
            } else if k >= bias + TMAX {
                TMAX
            } else {
                k - bias
            };
            if digit < t {
                break;
            }
            w = w.checked_mul(BASE - t).ok_or(ERR_INVALID_INPUT)?;
            k = k.checked_add(BASE).ok_or(ERR_INVALID_INPUT)?;
        }
        let out_len = output.len() as u32 + 1;
        bias = adapt(i - oldi, out_len, oldi == 0);
        n = n.checked_add(i / out_len).ok_or(ERR_INVALID_INPUT)?;
        i %= out_len;
        // insert n at position i
        output.insert(i as usize, n);
        i = i.checked_add(1).ok_or(ERR_INVALID_INPUT)?;
    }

    let mut decoded = String::new();
    for cp in output {
        let ch = char::from_u32(cp).ok_or(ERR_INVALID_INPUT)?;
        decoded.push(ch);
    }
    Ok(decoded)
}

/// `punycode.encode(string)` — encode a Unicode string to Punycode (no `xn--`).
fn encode(input: &str) -> String {
    let input_cps: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let mut output: Vec<char> = Vec::new();

    // Copy basic code points to the output.
    for &cp in &input_cps {
        if cp < 0x80 {
            if let Some(c) = char::from_u32(cp) {
                output.push(c);
            }
        }
    }
    let basic_length = output.len() as u32;
    let mut handled = basic_length;

    if basic_length > 0 {
        output.push(DELIMITER);
    }

    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;

    let total = input_cps.len() as u32;
    while handled < total {
        // Find the minimum code point >= n.
        let mut m = u32::MAX;
        for &cp in &input_cps {
            if cp >= n && cp < m {
                m = cp;
            }
        }
        delta = delta.wrapping_add((m - n).wrapping_mul(handled + 1));
        n = m;
        for &cp in &input_cps {
            if cp < n {
                delta = delta.wrapping_add(1);
            }
            if cp == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    output.push(digit_to_basic(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                output.push(digit_to_basic(q));
                bias = adapt(delta, handled + 1, handled == basic_length);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        n += 1;
    }

    output.into_iter().collect()
}

/// Whether a label contains any non-ASCII code point.
fn has_non_ascii(label: &str) -> bool {
    label.bytes().any(|b| b >= 0x80)
}

/// `punycode.toASCII(domain)` — encode each non-ASCII label as `xn--` + encode.
fn to_ascii(domain: &str) -> String {
    map_labels(domain, |label| {
        if has_non_ascii(label) {
            format!("xn--{}", encode(label))
        } else {
            label.to_string()
        }
    })
}

/// `punycode.toUnicode(domain)` — decode each `xn--` label back to Unicode.
fn to_unicode(domain: &str) -> Result<String, &'static str> {
    map_labels_result(domain, |label| {
        if let Some(rest) = label.strip_prefix("xn--") {
            decode(&rest.to_ascii_lowercase())
        } else {
            Ok(label.to_string())
        }
    })
}

/// Apply `f` to each `.`-separated label, preserving Node's split behavior.
/// Node splits on `[.。．｡]`; we split on those four dot variants
/// and rejoin with `.` only between mapped labels of the original split (the
/// original separators are normalized to `.`, matching Node's output).
fn map_labels(domain: &str, f: impl Fn(&str) -> String) -> String {
    let (prefix, domain) = split_email_local_part(domain);
    let parts: Vec<&str> = domain
        .split(['.', '\u{3002}', '\u{FF0E}', '\u{FF61}'])
        .collect();
    format!(
        "{}{}",
        prefix.unwrap_or(""),
        parts.into_iter().map(f).collect::<Vec<_>>().join(".")
    )
}

fn map_labels_result<E>(domain: &str, f: impl Fn(&str) -> Result<String, E>) -> Result<String, E> {
    let (prefix, domain) = split_email_local_part(domain);
    let parts: Vec<&str> = domain
        .split(['.', '\u{3002}', '\u{FF0E}', '\u{FF61}'])
        .collect();
    let mut mapped = Vec::with_capacity(parts.len());
    for part in parts {
        mapped.push(f(part)?);
    }
    Ok(format!("{}{}", prefix.unwrap_or(""), mapped.join(".")))
}

fn split_email_local_part(domain: &str) -> (Option<&str>, &str) {
    if let Some(at) = domain.find('@') {
        let rest_start = at + '@'.len_utf8();
        let rest_end = domain[rest_start..]
            .find('@')
            .map(|next_at| rest_start + next_at)
            .unwrap_or(domain.len());
        (Some(&domain[..rest_start]), &domain[rest_start..rest_end])
    } else {
        (None, domain)
    }
}

// ── C-ABI runtime entry points (NaN-boxed string in/out) ──

#[no_mangle]
pub extern "C" fn js_punycode_decode(value: f64) -> f64 {
    match decode(&get_string_content(value)) {
        Ok(decoded) => create_string_f64(&decoded),
        Err(message) => throw_range_error(message),
    }
}

#[no_mangle]
pub extern "C" fn js_punycode_encode(value: f64) -> f64 {
    create_string_f64(&encode(&get_string_content(value)))
}

#[no_mangle]
pub extern "C" fn js_punycode_to_ascii(value: f64) -> f64 {
    create_string_f64(&to_ascii(&get_string_content(value)))
}

#[no_mangle]
pub extern "C" fn js_punycode_to_unicode(value: f64) -> f64 {
    match to_unicode(&get_string_content(value)) {
        Ok(decoded) => create_string_f64(&decoded),
        Err(message) => throw_range_error(message),
    }
}

fn string_ptr_if_string(value: f64) -> Option<*const StringHeader> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
        if crate::string::is_valid_string_ptr(ptr) {
            return Some(ptr);
        }
    }
    None
}

fn string_bytes(ptr: *const StringHeader) -> &'static [u8] {
    if !crate::string::is_valid_string_ptr(ptr) {
        return &[];
    }
    unsafe { slice::from_raw_parts(string_data(ptr), (*ptr).byte_len as usize) }
}

fn push_utf16_units_from_scalar(units: &mut Vec<u16>, cp: u32) {
    if cp <= 0xFFFF {
        units.push(cp as u16);
    } else {
        let n = cp - 0x10000;
        units.push((0xD800 + ((n >> 10) & 0x3FF)) as u16);
        units.push((0xDC00 + (n & 0x3FF)) as u16);
    }
}

fn decode_wtf8_to_utf16_units(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            units.push(b as u16);
            i += 1;
        } else if b < 0xE0 && i + 1 < bytes.len() {
            let cp = (((b & 0x1F) as u32) << 6) | ((bytes[i + 1] & 0x3F) as u32);
            push_utf16_units_from_scalar(&mut units, cp);
            i += 2;
        } else if b < 0xF0 && i + 2 < bytes.len() {
            let cp = (((b & 0x0F) as u32) << 12)
                | (((bytes[i + 1] & 0x3F) as u32) << 6)
                | ((bytes[i + 2] & 0x3F) as u32);
            push_utf16_units_from_scalar(&mut units, cp);
            i += 3;
        } else if b < 0xF8 && i + 3 < bytes.len() {
            let cp = (((b & 0x07) as u32) << 18)
                | (((bytes[i + 1] & 0x3F) as u32) << 12)
                | (((bytes[i + 2] & 0x3F) as u32) << 6)
                | ((bytes[i + 3] & 0x3F) as u32);
            push_utf16_units_from_scalar(&mut units, cp);
            i += 4;
        } else {
            units.push(0xFFFD);
            i += 1;
        }
    }
    units
}

fn push_code_point_wtf8(out: &mut Vec<u8>, cp: u32) -> bool {
    if cp <= 0x7F {
        out.push(cp as u8);
        false
    } else if cp <= 0x7FF {
        out.push((0xC0 | (cp >> 6)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
        false
    } else if (0xD800..=0xDFFF).contains(&cp) {
        out.push((0xE0 | (cp >> 12)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
        true
    } else {
        let ch = char::from_u32(cp).unwrap();
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        out.extend_from_slice(encoded.as_bytes());
        false
    }
}

fn value_to_code_point_number(value: f64) -> f64 {
    JSValue::from_bits(value.to_bits()).to_number()
}

/// `punycode.ucs2.decode(string)` — split a JS string's UTF-16 code units into
/// code points, preserving lone surrogate code units like Node's bundled
/// punycode.js helper.
#[no_mangle]
pub extern "C" fn js_punycode_ucs2_decode(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        throw_type_error("Cannot read properties of undefined (reading 'length')");
    }
    if jsval.is_null() {
        throw_type_error("Cannot read properties of null (reading 'length')");
    }

    let Some(str_ptr) = string_ptr_if_string(value) else {
        let arr = crate::array::js_array_alloc(0);
        return f64::from_bits(crate::value::JSValue::array_ptr(arr).bits());
    };

    let units = decode_wtf8_to_utf16_units(string_bytes(str_ptr));
    let mut arr = crate::array::js_array_alloc(units.len() as u32);
    let mut i = 0usize;
    while i < units.len() {
        let unit = units[i];
        let cp = if (0xD800..=0xDBFF).contains(&unit) && i + 1 < units.len() {
            let next = units[i + 1];
            if (0xDC00..=0xDFFF).contains(&next) {
                i += 1;
                0x10000 + (((unit as u32) - 0xD800) << 10) + ((next as u32) - 0xDC00)
            } else {
                unit as u32
            }
        } else {
            unit as u32
        };
        arr = crate::array::js_array_push_f64(arr, cp as f64);
        i += 1;
    }
    f64::from_bits(crate::value::JSValue::array_ptr(arr).bits())
}

/// `punycode.ucs2.encode(codePoints)` — build a string from an array of code
/// points, preserving surrogate code units using Perry's WTF-8 string storage.
#[no_mangle]
pub extern "C" fn js_punycode_ucs2_encode(value: f64) -> f64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        throw_type_error("codePoints is not iterable (cannot read property undefined)");
    }
    if jsval.is_null() {
        throw_type_error("codePoints is not iterable (cannot read property null)");
    }
    if jsval.is_any_string() {
        throw_invalid_code_point(f64::NAN);
    }
    if crate::array::js_array_is_array(value).to_bits() != crate::value::TAG_TRUE {
        throw_type_error("Spread syntax requires ...iterable[Symbol.iterator] to be a function");
    }

    let arr = if jsval.is_pointer() {
        jsval.as_pointer::<crate::array::ArrayHeader>()
    } else {
        crate::value::js_nanbox_get_pointer(value) as *const crate::array::ArrayHeader
    };
    if arr.is_null() {
        throw_type_error("Spread syntax requires ...iterable[Symbol.iterator] to be a function");
    }
    let len = crate::array::js_array_length(arr);
    let mut out = Vec::new();
    let mut has_surrogate = false;
    for i in 0..len {
        let raw = crate::array::js_array_get_f64(arr, i);
        let code = value_to_code_point_number(raw);
        if !code.is_finite() || code.fract() != 0.0 || code < 0.0 || code > 0x10FFFF as f64 {
            throw_invalid_code_point(code);
        }
        has_surrogate |= push_code_point_wtf8(&mut out, code as u32);
    }
    let ptr = if has_surrogate {
        crate::string::js_string_from_wtf8_bytes(out.as_ptr(), out.len() as u32)
    } else {
        crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32)
    };
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

#[cfg(test)]
mod ucs2_tests {
    use super::*;

    #[test]
    fn wtf8_decode_preserves_lone_surrogates() {
        assert_eq!(
            decode_wtf8_to_utf16_units(&[0xED, 0xA0, 0x80]),
            vec![0xD800]
        );
        assert_eq!(
            decode_wtf8_to_utf16_units(&[b'A', 0xED, 0xB0, 0x80]),
            vec![65, 0xDC00]
        );
    }

    #[test]
    fn wtf8_encode_preserves_surrogate_units() {
        let mut out = Vec::new();
        assert!(push_code_point_wtf8(&mut out, 0xD800));
        assert_eq!(out, vec![0xED, 0xA0, 0x80]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        assert_eq!(encode("münchen"), "mnchen-3ya");
        assert_eq!(decode("mnchen-3ya").unwrap(), "münchen");
        assert_eq!(encode("bücher"), "bcher-kva");
        assert_eq!(decode("bcher-kva").unwrap(), "bücher");
        // all-ASCII encodes to itself + trailing delimiter
        assert_eq!(encode("abc"), "abc-");
        assert_eq!(decode("abc-").unwrap(), "abc");
    }

    #[test]
    fn domain_conversions() {
        assert_eq!(to_ascii("münchen.de"), "xn--mnchen-3ya.de");
        assert_eq!(to_unicode("xn--mnchen-3ya.de").unwrap(), "münchen.de");
        // pure-ASCII domains pass through unchanged
        assert_eq!(to_ascii("example.com"), "example.com");
        assert_eq!(to_unicode("example.com").unwrap(), "example.com");
    }
}
