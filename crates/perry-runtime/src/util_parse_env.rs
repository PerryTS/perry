//! `util.parseEnv(content)` (#2514) — parse `.env`-format text into a
//! null-prototype object. Mirrors Node's built-in parser: skip blank / `#`-comment lines,
//! strip an optional `export ` prefix, split on the first `=`, trim key+value;
//! quoted values (`"`, `'`, backtick) keep their inner content across lines
//! and treat `#` literally, double-quoted `\n` escapes become newlines, and
//! unquoted values drop an inline `# comment`. Last duplicate key wins.

use crate::url::{create_string_f64, get_string_content};

/// Parse `.env` text → ordered `(key, value)` pairs (sorted by key after
/// parsing; last duplicate's value wins).
fn parse_env(content: &str) -> Vec<(String, String)> {
    let bytes = content.as_bytes();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'\n' {
            i += 1;
            continue;
        }
        if bytes[i] == b'#' {
            skip_to_next_line(bytes, &mut i);
            continue;
        }

        if bytes[i..].starts_with(b"export")
            && bytes
                .get(i + b"export".len())
                .is_some_and(|b| matches!(*b, b' ' | b'\t'))
        {
            i += b"export".len();
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
                i += 1;
            }
        }

        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && bytes[i] != b'\n' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'\n' {
            skip_to_next_line(bytes, &mut i);
            continue;
        }

        let key = content[key_start..i].trim();
        i += 1; // '='
        if key.is_empty() {
            skip_to_next_line(bytes, &mut i);
            continue;
        }

        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
            i += 1;
        }

        let value = if i < bytes.len() && matches!(bytes[i], b'"' | b'\'' | b'`') {
            let quote = bytes[i];
            i += 1;
            let value_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let inner = &content[value_start..i];
            let value = if quote == b'"' {
                unescape_double(inner)
            } else {
                inner.to_string()
            };
            if i < bytes.len() {
                i += 1;
            }
            skip_to_next_line(bytes, &mut i);
            value
        } else {
            let value_start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                if bytes[i] == b'#' && (i == value_start || matches!(bytes[i - 1], b' ' | b'\t')) {
                    break;
                }
                i += 1;
            }
            let value = content[value_start..i].trim_end().to_string();
            skip_to_next_line(bytes, &mut i);
            value
        };

        push_entry(&mut out, key, value);
    }

    // Node's C++ parser stores into a sorted map, so the result object's keys
    // come out byte-lexicographically sorted (e.g. `A`,`M`,`Z`,`m`), NOT in
    // insertion order. Match that.
    out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    out
}

fn push_entry(out: &mut Vec<(String, String)>, key: &str, value: String) {
    if let Some(slot) = out.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value;
    } else {
        out.push((key.to_string(), value));
    }
}

fn skip_to_next_line(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && bytes[*i] != b'\n' {
        *i += 1;
    }
    if *i < bytes.len() {
        *i += 1;
    }
}

/// Process the only escape Node's dotenv parser decodes in double quotes.
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

/// `util.parseEnv(content)` → null-prototype object of parsed key/value strings.
#[no_mangle]
pub extern "C" fn js_util_parse_env(value: f64) -> f64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        let message = format!(
            "The \"content\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let content = get_string_content(value);
    let entries = parse_env(&content);
    let obj = crate::object::js_object_alloc_null_proto(0, (entries.len() as u32).max(1));
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
        assert_eq!(parse_env("A=\"x\\ty\""), vec![("A".into(), "x\\ty".into())]);
        assert_eq!(parse_env("JUSTKEY\nA=1"), vec![("A".into(), "1".into())]);
        assert_eq!(
            parse_env("\n# hi\n  # ind\nA=1"),
            vec![("A".into(), "1".into())]
        );
        assert_eq!(parse_env("A=1\nA=2"), vec![("A".into(), "2".into())]);
    }
}
