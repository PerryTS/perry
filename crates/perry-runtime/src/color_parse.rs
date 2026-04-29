//! Runtime CSS color parser (issue #185 Phase C step 7).
//!
//! Used by codegen when `apply_inline_style` sees a non-literal color
//! value (e.g., `backgroundColor: someStringVar`). String literals get
//! parsed at HIR time via the same algorithm at
//! `crates/perry-codegen/src/lower_call.rs::parse_color_string`; this
//! module is the runtime version for dynamic values.
//!
//! The codegen emits 4 calls (one per channel) so each piece of LLVM
//! IR is trivial — single function call returning a double. Slight
//! redundancy (parses the string 4 times per dynamic-color use) is
//! acceptable because dynamic colors are the rare case; the common
//! case (string/object literals) goes through compile-time parsing
//! with zero runtime cost.

use crate::value::js_get_string_pointer_unified;

/// Parse a CSS color string into 4 channels in 0..=1. Mirrors the
/// `parse_color_string` helper in codegen exactly.
fn parse_css_color(s: &str) -> Option<(f64, f64, f64, f64)> {
    let lower = s.trim().to_ascii_lowercase();
    let named = match lower.as_str() {
        "white" => Some((1.0, 1.0, 1.0, 1.0)),
        "black" => Some((0.0, 0.0, 0.0, 1.0)),
        "red" => Some((1.0, 0.0, 0.0, 1.0)),
        "green" => Some((0.0, 0.502, 0.0, 1.0)),
        "blue" => Some((0.0, 0.0, 1.0, 1.0)),
        "yellow" => Some((1.0, 1.0, 0.0, 1.0)),
        "cyan" => Some((0.0, 1.0, 1.0, 1.0)),
        "magenta" => Some((1.0, 0.0, 1.0, 1.0)),
        "gray" | "grey" => Some((0.502, 0.502, 0.502, 1.0)),
        "transparent" => Some((0.0, 0.0, 0.0, 0.0)),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(hex) = lower.strip_prefix('#') {
        let pair = |s: &str| u8::from_str_radix(s, 16).ok().map(|b| b as f64 / 255.0);
        let nibble = |c: char| c.to_digit(16).map(|n| (n as f64) * 17.0 / 255.0);
        match hex.len() {
            3 => {
                let chs: Vec<char> = hex.chars().collect();
                return Some((nibble(chs[0])?, nibble(chs[1])?, nibble(chs[2])?, 1.0));
            }
            4 => {
                let chs: Vec<char> = hex.chars().collect();
                return Some((
                    nibble(chs[0])?,
                    nibble(chs[1])?,
                    nibble(chs[2])?,
                    nibble(chs[3])?,
                ));
            }
            6 => {
                return Some((pair(&hex[0..2])?, pair(&hex[2..4])?, pair(&hex[4..6])?, 1.0));
            }
            8 => {
                return Some((
                    pair(&hex[0..2])?,
                    pair(&hex[2..4])?,
                    pair(&hex[4..6])?,
                    pair(&hex[6..8])?,
                ));
            }
            _ => {}
        }
    }
    None
}

/// Extract a single color channel from a JSValue containing a CSS
/// color string. `channel`: 0=r, 1=g, 2=b, 3=a. Returns 0 for
/// unparseable input on r/g/b channels and 1 (opaque) for a, so a
/// missing/bad string gives a black opaque sentinel rather than a
/// transparent surprise.
///
/// Codegen calls this 4 times per dynamic color use (one per channel)
/// — per-channel call lets LLVM IR stay trivial without needing
/// stack-alloca-of-array machinery.
#[no_mangle]
pub extern "C" fn js_color_parse_channel(value: f64, channel: i32) -> f64 {
    let s_ptr_i64 = js_get_string_pointer_unified(value);
    if s_ptr_i64 == 0 {
        return if channel == 3 { 1.0 } else { 0.0 };
    }
    // Read StringHeader-prefixed UTF-8 bytes.
    let s_ptr = s_ptr_i64 as *const u8;
    let s = unsafe {
        let header = s_ptr as *const crate::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = s_ptr.add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("")
    };
    let parsed = parse_css_color(s);
    match (parsed, channel) {
        (Some((r, _, _, _)), 0) => r,
        (Some((_, g, _, _)), 1) => g,
        (Some((_, _, b, _)), 2) => b,
        (Some((_, _, _, a)), 3) => a,
        // Unparseable string → opaque black sentinel.
        (None, 3) => 1.0,
        (None, _) => 0.0,
        // Channel out of range — should never happen from codegen.
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_6() {
        let (r, g, b, a) = parse_css_color("#3B82F6").unwrap();
        assert!((r - 0.2313).abs() < 0.001);
        assert!((g - 0.5098).abs() < 0.001);
        assert!((b - 0.9647).abs() < 0.001);
        assert_eq!(a, 1.0);
    }

    #[test]
    fn parses_hex_8_alpha() {
        let (_, _, _, a) = parse_css_color("#00000080").unwrap();
        assert!((a - 0.5019).abs() < 0.001);
    }

    #[test]
    fn parses_named_white() {
        assert_eq!(parse_css_color("white"), Some((1.0, 1.0, 1.0, 1.0)));
    }

    #[test]
    fn parses_named_transparent() {
        assert_eq!(parse_css_color("transparent"), Some((0.0, 0.0, 0.0, 0.0)));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_css_color("not-a-color").is_none());
    }
}
