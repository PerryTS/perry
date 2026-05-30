//! `util.parseEnv(content)` (#2514) — parse `.env`-format text into a plain
//! object. Mirrors Node's built-in parser: skip blank / `#`-comment lines,
//! strip an optional `export` prefix, split on the first `=`, trim key+value;
//! quoted values (`"`, `'`, backtick) keep their inner content and can span
//! lines, while unquoted values drop inline `#` comments. Last duplicate key
//! wins.

use crate::url::{create_string_f64, get_string_content};

/// Parse `.env` text → ordered `(key, value)` pairs. Node returns object keys
/// in byte-lexicographic order, with the last duplicate value winning.
pub(crate) fn parse_env(content: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let raw_line = lines[index];
        index += 1;
        let line = raw_line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = strip_export_prefix(line);
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }
        let value = parse_value(raw_value, &lines, &mut index);
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

fn strip_export_prefix(line: &str) -> &str {
    let Some(rest) = line.strip_prefix("export") else {
        return line;
    };
    if rest
        .chars()
        .next()
        .map(|c| c == ' ' || c == '\t')
        .unwrap_or(false)
    {
        rest.trim_start()
    } else {
        line
    }
}

fn parse_value(raw: &str, lines: &[&str], index: &mut usize) -> String {
    let value = raw.trim_start();
    if let Some(quote) = value
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\'' | '`'))
    {
        let saved_index = *index;
        if let Some(inner) = parse_quoted_value(&value[quote.len_utf8()..], quote, lines, index) {
            return if quote == '"' {
                unescape_double(&inner)
            } else {
                inner
            };
        }
        *index = saved_index;
    }
    strip_inline_comment(value).trim_end().to_string()
}

fn parse_quoted_value(
    first_fragment: &str,
    quote: char,
    lines: &[&str],
    index: &mut usize,
) -> Option<String> {
    if let Some(end) = first_fragment.find(quote) {
        return Some(first_fragment[..end].to_string());
    }

    let mut out = first_fragment.to_string();
    while *index < lines.len() {
        let line = lines[*index];
        *index += 1;
        out.push('\n');
        if let Some(end) = line.find(quote) {
            out.push_str(&line[..end]);
            return Some(out);
        }
        out.push_str(line);
    }

    None
}

/// Drop an inline `#` comment. Node's parser starts a comment at the first
/// unquoted `#`, even without preceding whitespace.
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
        assert_eq!(
            parse_env("A=\"l1\nl2\""),
            vec![("A".into(), "l1\nl2".into())]
        );
        assert_eq!(parse_env("A='l1\nl2'"), vec![("A".into(), "l1\nl2".into())]);
        assert_eq!(parse_env("A=`l1\nl2`"), vec![("A".into(), "l1\nl2".into())]);
        assert_eq!(
            parse_env("A=\"x\nB=2"),
            vec![("A".into(), "\"x".into()), ("B".into(), "2".into())]
        );
        assert_eq!(parse_env("A=b#c"), vec![("A".into(), "b".into())]);
        assert_eq!(parse_env("export   A=b"), vec![("A".into(), "b".into())]);
        assert_eq!(parse_env("JUSTKEY\nA=1"), vec![("A".into(), "1".into())]);
        assert_eq!(
            parse_env("\n# hi\n  # ind\nA=1"),
            vec![("A".into(), "1".into())]
        );
        assert_eq!(parse_env("A=1\nA=2"), vec![("A".into(), "2".into())]);
    }
}
