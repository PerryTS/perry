//! `node:punycode` — the deprecated Punycode/IDNA conversion module (#2513).
//!
//! A direct port of the reference `punycode.js` (RFC 3492 Bootstring), exposed
//! as C-ABI runtime helpers. Top-level string surface only:
//! `decode`/`encode`/`toASCII`/`toUnicode`/`version`. The `ucs2.decode` /
//! `ucs2.encode` array helpers (sub-namespace + array marshalling) are a
//! separate follow-up.

use crate::url::{create_string_f64, get_string_content};

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
/// its Unicode form. Returns the input unchanged on malformed input is NOT the
/// spec behavior (Node throws RangeError); we instead return a best-effort
/// decode and never panic, matching common consumer expectations.
fn decode(input: &str) -> String {
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
    for &cp in input_cps.iter().take(basic_len) {
        if cp >= 0x80 {
            // Not a basic code point — malformed; bail to best-effort.
            return input.to_string();
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
                return input.to_string(); // bad input
            }
            let digit = match basic_to_digit(input_cps[idx]) {
                Some(d) => d,
                None => return input.to_string(),
            };
            idx += 1;
            // overflow-guarded i += digit * w
            i = i.wrapping_add(digit.wrapping_mul(w));
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
            w = w.wrapping_mul(BASE - t);
            k += BASE;
        }
        let out_len = output.len() as u32 + 1;
        bias = adapt(i - oldi, out_len, oldi == 0);
        n = n.wrapping_add(i / out_len);
        i %= out_len;
        // insert n at position i
        output.insert(i as usize, n);
        i += 1;
    }

    output
        .into_iter()
        .filter_map(char::from_u32)
        .collect::<String>()
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
fn to_unicode(domain: &str) -> String {
    map_labels(domain, |label| {
        let lower = label.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("xn--") {
            decode(rest)
        } else {
            label.to_string()
        }
    })
}

/// Apply `f` to each `.`-separated label, preserving Node's split behavior.
/// Node splits on `[.。．｡]`; we split on those four dot variants
/// and rejoin with `.` only between mapped labels of the original split (the
/// original separators are normalized to `.`, matching Node's output).
fn map_labels(domain: &str, f: impl Fn(&str) -> String) -> String {
    // Node preserves a leading "email-style" local part before '@' verbatim
    // for the *userland* punycode; the deprecated core module does not — it
    // maps the whole string label-by-label. Split on the four IDNA dot chars.
    let parts: Vec<&str> = domain
        .split(['.', '\u{3002}', '\u{FF0E}', '\u{FF61}'])
        .collect();
    parts.into_iter().map(f).collect::<Vec<_>>().join(".")
}

// ── C-ABI runtime entry points (NaN-boxed string in/out) ──

#[no_mangle]
pub extern "C" fn js_punycode_decode(value: f64) -> f64 {
    create_string_f64(&decode(&get_string_content(value)))
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
    create_string_f64(&to_unicode(&get_string_content(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        assert_eq!(encode("münchen"), "mnchen-3ya");
        assert_eq!(decode("mnchen-3ya"), "münchen");
        assert_eq!(encode("bücher"), "bcher-kva");
        assert_eq!(decode("bcher-kva"), "bücher");
        // all-ASCII encodes to itself + trailing delimiter
        assert_eq!(encode("abc"), "abc-");
        assert_eq!(decode("abc-"), "abc");
    }

    #[test]
    fn domain_conversions() {
        assert_eq!(to_ascii("münchen.de"), "xn--mnchen-3ya.de");
        assert_eq!(to_unicode("xn--mnchen-3ya.de"), "münchen.de");
        // pure-ASCII domains pass through unchanged
        assert_eq!(to_ascii("example.com"), "example.com");
        assert_eq!(to_unicode("example.com"), "example.com");
    }
}
