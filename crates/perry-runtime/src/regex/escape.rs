//! `RegExp.escape(str)` (ECMAScript `EncodeForRegExpEscape`) and its helpers.
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use super::{js_string_from_str, string_as_str};
use crate::string::StringHeader;
use crate::value::js_nanbox_string;

/// ECMAScript `WhiteSpace` set: TAB/VT/FF/SP, NBSP, ZWNBSP, and all
/// Unicode `Space_Separator` (Zs) code points. (TAB/VT/FF are handled by
/// the named control-escape table first; included here for completeness.)
fn regexp_escape_is_whitespace(cp: u32) -> bool {
    matches!(
        cp,
        0x0009 // TAB
            | 0x000B // VT
            | 0x000C // FF
            | 0x0020 // SP
            | 0x00A0 // NBSP
            | 0xFEFF // ZWNBSP
            // Unicode Space_Separator (Zs):
            | 0x1680
            | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000
    )
}

/// ECMAScript `LineTerminator` set: LF, CR, LS (U+2028), PS (U+2029).
fn regexp_escape_is_line_terminator(cp: u32) -> bool {
    matches!(cp, 0x000A | 0x000D | 0x2028 | 0x2029)
}

/// `EncodeForRegExpEscape` unicode-escape emitter: `\xHH` for ≤ 0xFF,
/// `\uHHHH` otherwise (callers only pass BMP code units here).
fn regexp_escape_unicode(out: &mut String, unit: u16) {
    if unit <= 0xFF {
        out.push_str(&format!("\\x{:02x}", unit));
    } else {
        out.push_str(&format!("\\u{:04x}", unit));
    }
}

/// `RegExp.escape(str)` — escape `str` so it can be embedded literally in a
/// regular expression pattern without changing match semantics. Operates on
/// UTF-16 code units to match JS string semantics. The argument MUST be a
/// string (TypeError otherwise). Returns a NaN-boxed string.
#[no_mangle]
pub extern "C" fn js_regexp_escape(input: f64) -> f64 {
    let jsv = crate::value::JSValue::from_bits(input.to_bits());
    if !jsv.is_any_string() {
        let msg = js_string_from_str("input argument must be a string");
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }

    let str_ptr = crate::value::js_get_string_pointer_unified(input) as *const StringHeader;
    let s = string_as_str(str_ptr);

    // Encode to UTF-16 code units: JS escaping is defined per code unit.
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out = String::with_capacity(units.len() * 2);

    for (i, &unit) in units.iter().enumerate() {
        let c = char::from_u32(unit as u32);

        // First code unit: if ASCII alphanumeric, force a unicode escape so a
        // leading letter/digit can't combine with a preceding backslash when
        // concatenated (e.g. avoid forming `\c`, `\1`, etc.).
        if i == 0 {
            if let Some(ch) = c {
                if ch.is_ascii_alphanumeric() {
                    regexp_escape_unicode(&mut out, unit);
                    continue;
                }
            }
        }

        match c {
            // Syntax characters and `/` → backslash escape.
            Some('^') | Some('$') | Some('\\') | Some('.') | Some('*') | Some('+') | Some('?')
            | Some('(') | Some(')') | Some('[') | Some(']') | Some('{') | Some('}') | Some('|')
            | Some('/') => {
                out.push('\\');
                out.push(c.unwrap());
            }
            // Named control escapes.
            Some('\t') => out.push_str("\\t"),
            Some('\n') => out.push_str("\\n"),
            Some('\u{000B}') => out.push_str("\\v"),
            Some('\u{000C}') => out.push_str("\\f"),
            Some('\r') => out.push_str("\\r"),
            _ => {
                let cp = unit as u32;
                let is_other_punctuator = matches!(
                    c,
                    Some(',')
                        | Some('-')
                        | Some('=')
                        | Some('<')
                        | Some('>')
                        | Some('#')
                        | Some('&')
                        | Some('!')
                        | Some('%')
                        | Some(':')
                        | Some(';')
                        | Some('@')
                        | Some('~')
                        | Some('\'')
                        | Some('`')
                        | Some('"')
                );
                if is_other_punctuator
                    || regexp_escape_is_whitespace(cp)
                    || regexp_escape_is_line_terminator(cp)
                {
                    regexp_escape_unicode(&mut out, unit);
                } else {
                    // Pass through. Use the original code unit so lone
                    // surrogates round-trip (char::from_u32 returns None for
                    // surrogate halves; push the decoded char when valid).
                    match c {
                        Some(ch) => out.push(ch),
                        None => {
                            // Lone surrogate: re-encode the single code unit.
                            let mut buf = [0u16; 1];
                            buf[0] = unit;
                            out.push_str(&String::from_utf16_lossy(&buf));
                        }
                    }
                }
            }
        }
    }

    let result = js_string_from_str(&out);
    js_nanbox_string(result as i64)
}

/// Keepalive anchor: `js_regexp_escape` is only called from codegen-emitted
/// `.o`, so the auto-optimize whole-program LLVM rebuild would dead-strip it
/// without this `#[used]` reference (see #3320).
#[used]
static KEEP_REGEXP_ESCAPE: extern "C" fn(f64) -> f64 = js_regexp_escape;
