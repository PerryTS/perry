#[derive(Clone, Copy)]
struct CaptureSpan {
    close: usize,
}

fn parse_decimal_escape(chars: &[char], mut i: usize) -> (usize, usize) {
    let start = i;
    let mut value = 0usize;
    while i < chars.len() && chars[i].is_ascii_digit() {
        value = value * 10 + (chars[i] as u8 - b'0') as usize;
        i += 1;
    }
    (value, i - start)
}

fn collect_capture_spans(chars: &[char]) -> Vec<CaptureSpan> {
    let mut spans = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i += 2;
            }
            '[' => {
                in_class = true;
                i += 1;
            }
            ']' => {
                in_class = false;
                i += 1;
            }
            '(' if !in_class => {
                let non_capturing = i + 1 < chars.len()
                    && chars[i + 1] == '?'
                    && !matches!(chars.get(i + 2), Some('<'));
                let named_lookbehind = i + 2 < chars.len()
                    && chars[i + 1] == '?'
                    && chars[i + 2] == '<'
                    && matches!(chars.get(i + 3), Some('=') | Some('!'));
                if !non_capturing && !named_lookbehind {
                    let idx = spans.len();
                    spans.push(CaptureSpan { close: usize::MAX });
                    stack.push((idx, i));
                }
                i += 1;
            }
            ')' if !in_class => {
                if let Some((idx, _)) = stack.pop() {
                    spans[idx].close = i;
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    spans
}

fn is_forward_backreference(spans: &[CaptureSpan], escape_pos: usize, group: usize) -> bool {
    if group == 0 || group > spans.len() {
        return false;
    }
    let span = spans[group - 1];
    span.close == usize::MAX || escape_pos < span.close
}

fn is_regex_identity_escape(ch: char) -> bool {
    matches!(
        ch,
        '~' | '`'
            | '!'
            | '@'
            | '#'
            | '%'
            | '&'
            | '-'
            | '='
            | ':'
            | ';'
            | '\''
            | '"'
            | ','
            | '<'
            | '>'
            | '/'
    )
}

fn push_escaped_literal(out: &mut String, ch: char) {
    match ch {
        '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\' => {
            out.push('\\');
            out.push(ch);
        }
        _ => out.push(ch),
    }
}

fn control_escape_value(ch: char) -> Option<u8> {
    if ch.is_ascii_alphabetic() {
        Some((ch.to_ascii_uppercase() as u8) % 32)
    } else {
        None
    }
}

fn push_hex_escape(out: &mut String, value: u8) {
    out.push_str("\\x{");
    out.push_str(&format!("{:02X}", value));
    out.push('}');
}

fn is_decimal_escape(chars: &[char], i: usize) -> bool {
    i + 1 < chars.len() && chars[i] == '\\' && chars[i + 1].is_ascii_digit()
}

fn parse_braced_quantifier(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + 1;
    let first_digits_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == first_digits_start {
        return None;
    }
    if i < chars.len() && chars[i] == ',' {
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < chars.len() && chars[i] == '}' {
        Some(i)
    } else {
        None
    }
}

pub(super) fn has_invalid_repeated_quantifier(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let mut in_class = false;
    let mut can_quantify = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += if is_decimal_escape(&chars, i) {
                1 + parse_decimal_escape(&chars, i + 1).1
            } else {
                2
            };
            can_quantify = true;
            continue;
        }
        if in_class {
            if chars[i] == ']' {
                in_class = false;
                can_quantify = true;
            }
            i += 1;
            continue;
        }
        match chars[i] {
            '[' => {
                in_class = true;
                i += 1;
            }
            '*' | '+' | '?' => {
                if !can_quantify {
                    return true;
                }
                i += 1;
                if i < chars.len() && chars[i] == '?' {
                    i += 1;
                }
                can_quantify = false;
            }
            '{' => {
                if let Some(end) = parse_braced_quantifier(&chars, i) {
                    if !can_quantify {
                        return true;
                    }
                    i = end + 1;
                    if i < chars.len() && chars[i] == '?' {
                        i += 1;
                    }
                    can_quantify = false;
                } else {
                    can_quantify = true;
                    i += 1;
                }
            }
            '|' => {
                can_quantify = false;
                i += 1;
            }
            ')' => {
                can_quantify = true;
                i += 1;
            }
            _ => {
                can_quantify = true;
                i += 1;
            }
        }
    }
    false
}

/// Translate a JavaScript regex pattern to a Rust regex-crate compatible pattern.
/// Handles JS-specific escape sequences not supported by the Rust regex crate.
/// Also converts JS-style named groups `(?<name>...)` to Rust-style `(?P<name>...)`.
pub(super) fn js_regex_to_rust(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let capture_spans = collect_capture_spans(&chars);
    let mut i = 0;
    // Track whether we're inside a `[...]` character class. JS and the Rust
    // `regex` crate disagree on how a bare `[` inside a class is read, so we
    // reconcile it below.
    let mut in_class = false;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                // JS allows \/ to escape forward slash — Rust regex doesn't need it
                '/' => {
                    result.push('/');
                    i += 2;
                }
                'c' if i + 2 < chars.len() => {
                    if let Some(value) = control_escape_value(chars[i + 2]) {
                        push_hex_escape(&mut result, value);
                        i += 3;
                    } else {
                        result.push('\\');
                        result.push('c');
                        i += 2;
                    }
                }
                '0' if i + 2 >= chars.len() || !chars[i + 2].is_ascii_digit() => {
                    push_hex_escape(&mut result, 0);
                    i += 2;
                }
                '1'..='9' => {
                    let (group, digits) = parse_decimal_escape(&chars, i + 1);
                    if is_forward_backreference(&capture_spans, i, group) {
                        i += 1 + digits;
                    } else {
                        result.push('\\');
                        for ch in &chars[i + 1..i + 1 + digits] {
                            result.push(*ch);
                        }
                        i += 1 + digits;
                    }
                }
                ch if is_regex_identity_escape(ch) => {
                    // Inside a character class an escaped hyphen `\-` is always a
                    // literal hyphen, but the Rust `regex` crate reads a bare `-`
                    // flanked by members as a range operator (so `[a\- ]` would
                    // become the invalid range `[a- ]`). Keep the escape so it
                    // stays a literal regardless of position. `marked`'s GFM
                    // table-delimiter regex `[:\- ]` relies on this.
                    if in_class && ch == '-' {
                        result.push('\\');
                        result.push('-');
                    } else {
                        push_escaped_literal(&mut result, ch);
                    }
                    i += 2;
                }
                // Pass through all other backslash sequences as-is. (An escaped
                // `\[` / `\]` is consumed here and so never toggles `in_class`.)
                _ => {
                    result.push('\\');
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else if chars[i] == '[' {
            // In JS, an unescaped `[` inside a character class is a literal `[`
            // (e.g. `/[[]/` matches a single `[`). The Rust `regex` crate rejects
            // a bare `[` inside `[...]`, so escape it. A `[` outside a class opens
            // one. This is what Hono's RegExpRouter relies on
            // (`/[.\\+*[^\]$()]/g`), so every Hono app hit it before this fix.
            if in_class {
                result.push('\\');
                result.push('[');
            } else {
                in_class = true;
                result.push('[');
            }
            i += 1;
        } else if chars[i] == ']' {
            // An unescaped `]` closes the current class (an escaped `\]` was
            // consumed by the backslash branch above and never reaches here).
            in_class = false;
            result.push(']');
            i += 1;
        } else if !in_class && chars[i] == '(' && i + 2 < chars.len() && chars[i + 1] == '?' {
            // Check for JS named group (?<name>...) — convert to (?P<name>...)
            // But NOT (?<=...) (lookbehind) or (?<!...) (negative lookbehind).
            // Parens inside a character class are literals, so only outside a class.
            if chars[i + 2] == '<'
                && i + 3 < chars.len()
                && chars[i + 3] != '='
                && chars[i + 3] != '!'
            {
                result.push_str("(?P<");
                i += 3; // skip past "(?<"
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}
