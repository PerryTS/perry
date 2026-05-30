//! `util.parseEnv(content)` (#2514) — parse `.env`-format text into a plain
//! object. Mirrors Node's built-in parser: skip blank / `#`-comment lines,
//! strip an optional `export ` prefix, split on the first `=`, trim key+value,
//! and keep quoted (`"`, `'`, backtick) value contents verbatim except that
//! double-quoted `\n` sequences expand to newlines. Quoted values may span
//! lines; unquoted values stop at `#`. Last duplicate key wins.

use crate::url::{create_string_f64, get_string_content};

/// Parse `.env` text → sorted `(key, value)` pairs (last duplicate wins).
fn parse_env(content: &str) -> Vec<(String, String)> {
    let normalized = content.replace('\r', "");
    let chars: Vec<char> = normalized.chars().collect();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut pos = 0;
    while pos < chars.len() {
        let line_end = find_next_newline(&chars, pos);
        let line_start = skip_spaces(&chars, pos, line_end);
        if line_start == line_end || chars[line_start] == '#' {
            pos = next_line_start(&chars, line_end);
            continue;
        }

        let Some(eq_idx) = chars[line_start..line_end]
            .iter()
            .position(|&c| c == '=')
            .map(|rel| line_start + rel)
        else {
            pos = next_line_start(&chars, line_end);
            continue;
        };

        let (key_start, key_end) = trim_spaces(&chars, line_start, eq_idx);
        let mut key: String = chars[key_start..key_end].iter().collect();
        if let Some(rest) = key.strip_prefix("export ") {
            key = rest
                .trim_start_matches(|c| c == ' ' || c == '\t')
                .to_string();
        }
        if key.is_empty() {
            pos = next_line_start(&chars, line_end);
            continue;
        }

        let value_start = skip_spaces(&chars, eq_idx + 1, line_end);
        if value_start < line_end && is_quote(chars[value_start]) {
            let quote = chars[value_start];
            if let Some(close_idx) = find_closing_quote(&chars, value_start + 1, quote) {
                let inner: String = chars[value_start + 1..close_idx].iter().collect();
                let value = if quote == '"' {
                    expand_double_newlines(&inner)
                } else {
                    inner
                };
                upsert_env(&mut out, &key, value);
                pos = next_line_start(&chars, find_next_newline(&chars, close_idx + 1));
                continue;
            }
        }

        let raw_value: String = chars[eq_idx + 1..line_end].iter().collect();
        let value = parse_unquoted_value(&raw_value);
        upsert_env(&mut out, &key, value);
        pos = next_line_start(&chars, line_end);
    }
    // Node's C++ parser stores into a sorted map, so the result object's keys
    // come out byte-lexicographically sorted (e.g. `A`,`M`,`Z`,`m`), NOT in
    // insertion order. Match that.
    out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    out
}

fn upsert_env(out: &mut Vec<(String, String)>, key: &str, value: String) {
    if let Some(slot) = out.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value; // last duplicate wins
    } else {
        out.push((key.to_string(), value));
    }
}

fn find_next_newline(chars: &[char], from: usize) -> usize {
    chars[from..]
        .iter()
        .position(|&c| c == '\n')
        .map(|rel| from + rel)
        .unwrap_or(chars.len())
}

fn next_line_start(chars: &[char], line_end: usize) -> usize {
    if line_end < chars.len() {
        line_end + 1
    } else {
        line_end
    }
}

fn skip_spaces(chars: &[char], mut start: usize, end: usize) -> usize {
    while start < end && (chars[start] == ' ' || chars[start] == '\t') {
        start += 1;
    }
    start
}

fn trim_spaces(chars: &[char], mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end && (chars[start] == ' ' || chars[start] == '\t' || chars[start] == '\n') {
        start += 1;
    }
    while end > start && (chars[end - 1] == ' ' || chars[end - 1] == '\t' || chars[end - 1] == '\n')
    {
        end -= 1;
    }
    (start, end)
}

fn is_quote(c: char) -> bool {
    c == '"' || c == '\'' || c == '`'
}

fn find_closing_quote(chars: &[char], from: usize, quote: char) -> Option<usize> {
    chars[from..]
        .iter()
        .position(|&c| c == quote)
        .map(|rel| from + rel)
}

fn parse_unquoted_value(v: &str) -> String {
    let v = v.trim_matches(|c| c == ' ' || c == '\t' || c == '\n');
    strip_inline_comment(v)
        .trim_end_matches(|c| c == ' ' || c == '\t' || c == '\n')
        .to_string()
}

/// Drop an inline `# comment`.
fn strip_inline_comment(v: &str) -> &str {
    if let Some(idx) = v.find('#') {
        &v[..idx]
    } else {
        v
    }
}

/// Node only expands `\n` escape sequences inside double-quoted values.
fn expand_double_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'n') {
            chars.next();
            out.push('\n');
            continue;
        }
        out.push(c);
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
        assert_eq!(parse_env("A=foo#bar"), vec![("A".into(), "foo".into())]);
        assert_eq!(
            parse_env("A=\"one\ntwo\"\nB=3"),
            vec![("A".into(), "one\ntwo".into()), ("B".into(), "3".into())]
        );
        assert_eq!(
            parse_env("A='one\ntwo'\nB=3"),
            vec![("A".into(), "one\ntwo".into()), ("B".into(), "3".into())]
        );
        assert_eq!(
            parse_env("A=`one\ntwo`\nB=3"),
            vec![("A".into(), "one\ntwo".into()), ("B".into(), "3".into())]
        );
        assert_eq!(
            parse_env("A=\"one\r\ntwo\"\r\nB=3"),
            vec![("A".into(), "one\ntwo".into()), ("B".into(), "3".into())]
        );
        assert_eq!(
            parse_env("A=\"a\\nb\"\nB=\"a\\tb\"\nC=\"a\\\\b\""),
            vec![
                ("A".into(), "a\nb".into()),
                ("B".into(), "a\\tb".into()),
                ("C".into(), "a\\\\b".into())
            ]
        );
        assert_eq!(
            parse_env("A=\"one\nB=2"),
            vec![("A".into(), "\"one".into()), ("B".into(), "2".into())]
        );
    }
}
