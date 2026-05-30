//! `node:punycode` — the deprecated Punycode/IDNA conversion module (#2513).
//!
//! A direct port of the reference `punycode.js` (RFC 3492 Bootstring), exposed
//! as C-ABI runtime helpers. Top-level string surface only:
//! `decode`/`encode`/`toASCII`/`toUnicode`/`version`. The `ucs2.decode` /
//! `ucs2.encode` array helpers (sub-namespace + array marshalling) are a
//! separate follow-up.

use crate::url::{create_string_f64, get_string_content};
use crate::value::JSValue;

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
        let lower = label.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("xn--") {
            decode(rest)
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

/// `punycode.ucs2.decode(string)` — split a string into an array of Unicode
/// code points (astral code points become a single number, combining the
/// UTF-16 surrogate pair). Rust `char` is already a Unicode scalar value, so
/// `chars()` yields code points directly.
#[no_mangle]
pub extern "C" fn js_punycode_ucs2_decode(value: f64) -> f64 {
    let s = get_string_content(value);
    let arr = crate::array::js_array_alloc(s.chars().count() as u32);
    for c in s.chars() {
        crate::array::js_array_push_f64(arr, c as u32 as f64);
    }
    f64::from_bits(crate::value::JSValue::array_ptr(arr).bits())
}

/// `punycode.ucs2.encode(codePoints)` — build a string from an array of code
/// points (each astral code point is emitted as its character; `char::from_u32`
/// handles the surrogate-pair recombination since Rust strings are UTF-8).
#[no_mangle]
pub extern "C" fn js_punycode_ucs2_encode(value: f64) -> f64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return create_string_f64("");
    }
    let arr = jsval.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() {
        return create_string_f64("");
    }
    let len = unsafe { crate::array::js_array_length(arr) } as u32;
    let mut out = String::new();
    for i in 0..len {
        let v = unsafe { crate::array::js_array_get_f64(arr, i) };
        if let Some(c) = char::from_u32(v as u32) {
            out.push(c);
        }
    }
    create_string_f64(&out)
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
