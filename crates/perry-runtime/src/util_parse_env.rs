//! `util.parseEnv(content)` (#2514) — parse `.env`-format text into a plain
//! object. Mirrors Node's built-in parser: skip blank / `#`-comment lines,
//! strip an optional `export ` prefix, split on the first `=`, trim key+value;
//! quoted values (`"`, `'`, backtick) keep their inner content and may span
//! physical lines, double-quoted values process `\n` escapes, and unquoted
//! values drop an inline `# comment`. Last duplicate key wins.

use crate::string::StringHeader;
use crate::url::create_string_f64;
use crate::value::JSValue;

/// Parse `.env` text → ordered `(key, value)` pairs (insertion order, last
/// duplicate's value, first occurrence's position).
pub(crate) fn parse_env(content: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let bytes = content.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        let (line_end, next_pos) = line_bounds(bytes, pos);
        let mut line_start = skip_horizontal_ws(bytes, pos, line_end);
        if line_start >= line_end || bytes[line_start] == b'#' {
            pos = next_pos;
            continue;
        }
        // Optional `export ` prefix (Node strips it).
        if content[line_start..line_end].starts_with("export ") {
            line_start = skip_horizontal_ws(bytes, line_start + "export ".len(), line_end);
        }
        let Some(eq) = find_byte(bytes, line_start, line_end, b'=') else {
            pos = next_pos;
            continue;
        };
        let key = content[line_start..eq].trim();
        if key.is_empty() {
            pos = next_pos;
            continue;
        }
        let value_start = skip_horizontal_ws(bytes, eq + 1, line_end);
        let (value, after_value) = parse_value(content, value_start, line_end, next_pos);
        if let Some(slot) = out.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value; // last duplicate wins
        } else {
            out.push((key.to_string(), value));
        }
        pos = after_value;
    }
    // Node's C++ parser stores into a sorted map, so the result object's keys
    // come out byte-lexicographically sorted (e.g. `A`,`M`,`Z`,`m`), NOT in
    // insertion order. Match that.
    out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    out
}

fn line_bounds(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => return (i, i + 1),
            b'\r' => {
                let next = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i + 2
                } else {
                    i + 1
                };
                return (i, next);
            }
            _ => i += 1,
        }
    }
    (bytes.len(), bytes.len())
}

fn skip_horizontal_ws(bytes: &[u8], mut pos: usize, end: usize) -> usize {
    while pos < end && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }
    pos
}

fn find_byte(bytes: &[u8], mut pos: usize, end: usize, needle: u8) -> Option<usize> {
    while pos < end {
        if bytes[pos] == needle {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

fn next_line_start(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() {
        match bytes[pos] {
            b'\n' => return pos + 1,
            b'\r' => {
                return if pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
                    pos + 2
                } else {
                    pos + 1
                };
            }
            _ => pos += 1,
        }
    }
    bytes.len()
}

fn parse_value(
    content: &str,
    value_start: usize,
    line_end: usize,
    next_pos: usize,
) -> (String, usize) {
    let bytes = content.as_bytes();
    if value_start < line_end {
        let quote = bytes[value_start];
        if matches!(quote, b'"' | b'\'' | b'`') {
            if let Some(end) = find_byte(bytes, value_start + 1, bytes.len(), quote) {
                let inner = &content[value_start + 1..end];
                let value = if quote == b'"' {
                    unescape_double(inner)
                } else {
                    inner.to_string()
                };
                return (value, next_line_start(bytes, end + 1));
            }
        }
    }

    let raw = content[value_start..line_end].trim();
    (strip_inline_comment(raw).trim_end().to_string(), next_pos)
}

/// Drop an inline `# comment`.
fn strip_inline_comment(v: &str) -> &str {
    v.split_once('#').map(|(head, _)| head).unwrap_or(v)
}

/// Process backslash escapes inside a double-quoted value.
fn unescape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn string_content(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// `util.parseEnv(content)` → plain object of parsed key/value strings.
#[no_mangle]
pub extern "C" fn js_util_parse_env(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        let message = format!(
            "The \"content\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let content = string_content(value);
    let entries = parse_env(&content);
    let obj = crate::object::js_object_alloc(0, (entries.len() as u32).max(1));
    for (k, v) in &entries {
        let key_ptr = crate::string::js_string_from_bytes(k.as_ptr(), k.len() as u32);
        let val = create_string_f64(v);
        crate::object::js_object_set_field_by_name(obj, key_ptr, val);
    }
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_compatible() {
        assert_eq!(
            parse_env("A=1\nB=2"),
            vec![("A".into(), "1".into()), ("B".into(), "2".into())]
        );
        assert_eq!(parse_env("A=b # c"), vec![("A".into(), "b".into())]);
        assert_eq!(parse_env("A=\"b # c\""), vec![("A".into(), "b # c".into())]);
        assert_eq!(parse_env("A="), vec![("A".into(), "".into())]);
        assert_eq!(parse_env("A=b=c"), vec![("A".into(), "b=c".into())]);
        assert_eq!(parse_env("A = b "), vec![("A".into(), "b".into())]);
        assert_eq!(parse_env("export A=b"), vec![("A".into(), "b".into())]);
        assert_eq!(parse_env("A='x y'"), vec![("A".into(), "x y".into())]);
        assert_eq!(
            parse_env("A=\"l1\\nl2\""),
            vec![("A".into(), "l1\nl2".into())]
        );
        assert_eq!(
            parse_env("A=\"l1\nl2\""),
            vec![("A".into(), "l1\nl2".into())]
        );
        assert_eq!(parse_env("A=abc#def"), vec![("A".into(), "abc".into())]);
        assert_eq!(parse_env("JUSTKEY\nA=1"), vec![("A".into(), "1".into())]);
        assert_eq!(
            parse_env("\n# hi\n  # ind\nA=1"),
            vec![("A".into(), "1".into())]
        );
        assert_eq!(parse_env("A=1\nA=2"), vec![("A".into(), "2".into())]);
    }
}
