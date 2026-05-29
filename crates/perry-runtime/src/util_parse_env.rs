//! `util.parseEnv(content)` (#2514) — parse `.env`-format text into a plain
//! object. Mirrors Node's built-in parser: skip blank / `#`-comment lines,
//! strip an optional `export ` prefix, split on the first `=`, trim key+value;
//! quoted values (`"`, `'`, backtick) keep their inner content (double-quoted
//! values process `\n`/`\t`/`\r`/`\\`/`\"` escapes and treat `#` literally),
//! unquoted values drop an inline `# comment`. Last duplicate key wins.

use crate::url::{create_string_f64, get_string_content};

/// Parse `.env` text → ordered `(key, value)` pairs (insertion order, last
/// duplicate's value, first occurrence's position).
fn parse_env(content: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Optional `export ` prefix (Node strips it).
        let line = match line.strip_prefix("export ") {
            Some(rest) => rest.trim_start(),
            None => line,
        };
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }
        let value = parse_value(raw_value.trim());
        if let Some(slot) = out.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value; // last duplicate wins
        } else {
            out.push((key.to_string(), value));
        }
    }
    // Node's C++ parser stores into a sorted map, so the result object's keys
    // come out byte-lexicographically sorted (e.g. `A`,`M`,`Z`,`m`), NOT in
    // insertion order. Match that.
    out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    out
}

fn parse_value(v: &str) -> String {
    let chars: Vec<char> = v.chars().collect();
    if let Some(&first) = chars.first() {
        if first == '"' || first == '\'' || first == '`' {
            if let Some(end_rel) = chars[1..].iter().position(|&c| c == first) {
                let inner: String = chars[1..1 + end_rel].iter().collect();
                return if first == '"' {
                    unescape_double(&inner)
                } else {
                    inner
                };
            }
            // No closing quote — fall through to the unquoted handling.
        }
    }
    strip_inline_comment(v).trim_end().to_string()
}

/// Drop an inline `# comment` — a `#` at the start or preceded by whitespace.
fn strip_inline_comment(v: &str) -> &str {
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' && (i == 0 || b[i - 1] == b' ' || b[i - 1] == b'\t') {
            return &v[..i];
        }
        i += 1;
    }
    v
}

/// Process backslash escapes inside a double-quoted value.
fn unescape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
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

/// `util.parseEnv(content)` → plain object of parsed key/value strings.
#[no_mangle]
pub extern "C" fn js_util_parse_env(value: f64) -> f64 {
    let content = get_string_content(value);
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
        assert_eq!(parse_env("JUSTKEY\nA=1"), vec![("A".into(), "1".into())]);
        assert_eq!(
            parse_env("\n# hi\n  # ind\nA=1"),
            vec![("A".into(), "1".into())]
        );
        assert_eq!(parse_env("A=1\nA=2"), vec![("A".into(), "2".into())]);
    }
}
