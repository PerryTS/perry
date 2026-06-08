//! Annex B §B.2.2 — `String.prototype` HTML wrapper methods.
//!
//! These legacy, web-compat methods wrap the receiver string in an HTML tag.
//! Per the `CreateHTML` abstract operation (§B.2.2.1) the receiver is
//! `ToString(this)` (the *body*, never escaped) and the optional attribute
//! value is `ToString(value)` with every `"` replaced by `&quot;`.
//!
//! The caller (codegen fast path or the `js_native_call_method` string tower)
//! supplies the already-coerced attribute-value `StringHeader`; a null value
//! is treated as the empty string.

use super::*;

/// `CreateHTML(string, tag, attribute, value)` (ECMA-262 Annex B §B.2.2.1).
/// `attr == ""` means no attribute (`big`/`bold`/…); otherwise the attribute
/// is emitted as `attr="<escaped value>"`.
fn create_html(
    s: *const StringHeader,
    tag: &str,
    attr: &str,
    value: *const StringHeader,
) -> *mut StringHeader {
    let body = if is_valid_string_ptr(s) {
        string_as_str(s)
    } else {
        ""
    };
    let mut out = String::with_capacity(body.len() + tag.len() * 2 + attr.len() + 8);
    out.push('<');
    out.push_str(tag);
    if !attr.is_empty() {
        let v = if is_valid_string_ptr(value) {
            string_as_str(value)
        } else {
            ""
        };
        out.push(' ');
        out.push_str(attr);
        out.push_str("=\"");
        // Only `"` is escaped (to `&quot;`); `<`, `>`, `&` pass through, per spec.
        for ch in v.chars() {
            if ch == '"' {
                out.push_str("&quot;");
            } else {
                out.push(ch);
            }
        }
        out.push('"');
    }
    out.push('>');
    out.push_str(body);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    js_string_from_str(&out)
}

macro_rules! html_noarg {
    ($name:ident, $tag:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(s: *const StringHeader) -> *mut StringHeader {
            create_html(s, $tag, "", ptr::null())
        }
    };
}

macro_rules! html_attr {
    ($name:ident, $tag:literal, $attr:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(
            s: *const StringHeader,
            value: *const StringHeader,
        ) -> *mut StringHeader {
            create_html(s, $tag, $attr, value)
        }
    };
}

html_noarg!(js_string_big, "big");
html_noarg!(js_string_blink, "blink");
html_noarg!(js_string_bold, "b");
html_noarg!(js_string_fixed, "tt");
html_noarg!(js_string_italics, "i");
html_noarg!(js_string_small, "small");
html_noarg!(js_string_strike, "strike");
html_noarg!(js_string_sub, "sub");
html_noarg!(js_string_sup, "sup");

html_attr!(js_string_anchor, "a", "name");
html_attr!(js_string_link, "a", "href");
html_attr!(js_string_fontcolor, "font", "color");
html_attr!(js_string_fontsize, "font", "size");
