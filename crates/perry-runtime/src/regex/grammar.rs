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

#[inline]
fn is_surrogate(v: u32) -> bool {
    (0xD800..=0xDFFF).contains(&v)
}
#[inline]
fn is_high_surrogate(v: u32) -> bool {
    (0xD800..=0xDBFF).contains(&v)
}
#[inline]
fn is_low_surrogate(v: u32) -> bool {
    (0xDC00..=0xDFFF).contains(&v)
}

/// Parse a `\uXXXX` (exactly four hex digits, no braces) escape at `chars[i]`.
/// Returns the code-unit value and the index just past the escape.
fn parse_u4_escape(chars: &[char], i: usize) -> Option<(u32, usize)> {
    if chars.get(i) != Some(&'\\') || chars.get(i + 1) != Some(&'u') {
        return None;
    }
    let mut v = 0u32;
    for k in 0..4 {
        let d = chars.get(i + 2 + k)?.to_digit(16)?;
        v = v * 16 + d;
    }
    Some((v, i + 6))
}

/// Parse a "surrogate unit" at `chars[i]`: either a single `\uXXXX` escape or a
/// `[...]` class whose every element is a `\uXXXX` escape (singletons or
/// `\uA-\uB` ranges). Returns the code-unit ranges and the index just past the
/// unit — but ONLY when *every* code unit is a UTF-16 surrogate
/// (`0xD800..=0xDFFF`). Returns `None` for anything else, so ordinary escapes
/// and character classes pass through `fold_surrogate_pairs` untouched.
fn parse_surrogate_unit(chars: &[char], i: usize) -> Option<(Vec<(u32, u32)>, usize)> {
    if let Some((v, j)) = parse_u4_escape(chars, i) {
        return is_surrogate(v).then_some((vec![(v, v)], j));
    }
    if chars.get(i) != Some(&'[') {
        return None;
    }
    let mut k = i + 1;
    if chars.get(k) == Some(&'^') {
        return None; // negated class is never a plain surrogate set
    }
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    while chars.get(k).is_some_and(|c| *c != ']') {
        let (lo, k2) = parse_u4_escape(chars, k)?;
        if chars.get(k2) == Some(&'-') && chars.get(k2 + 1) == Some(&'\\') {
            let (hi, k3) = parse_u4_escape(chars, k2 + 1)?;
            ranges.push((lo, hi));
            k = k3;
        } else {
            ranges.push((lo, lo));
            k = k2;
        }
    }
    if chars.get(k) != Some(&']') || ranges.is_empty() {
        return None;
    }
    ranges
        .iter()
        .all(|(a, b)| is_surrogate(*a) && is_surrogate(*b))
        .then_some((ranges, k + 1))
}

/// Combine adjacent high-surrogate ranges with low-surrogate ranges into the
/// equivalent astral (supplementary-plane) scalar ranges, coalescing the
/// result. `cp = 0x10000 + (high - 0xD800) * 0x400 + (low - 0xDC00)`.
fn combine_surrogate_ranges(hi: &[(u32, u32)], lo: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut pts: Vec<(u32, u32)> = Vec::new();
    for &(h1, h2) in hi {
        for h in h1..=h2 {
            let base = 0x10000 + (h - 0xD800) * 0x400;
            for &(l1, l2) in lo {
                pts.push((base + (l1 - 0xDC00), base + (l2 - 0xDC00)));
            }
        }
    }
    pts.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (a, b) in pts {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 1 => {
                if b > last.1 {
                    last.1 = b;
                }
            }
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// Emit astral scalar ranges as a Rust-regex `\x{..}` class (or a bare `\x{..}`
/// for a single scalar).
fn emit_astral_class(out: &mut String, ranges: &[(u32, u32)]) {
    if let [(a, b)] = ranges {
        if a == b {
            out.push_str(&format!("\\x{{{a:x}}}"));
            return;
        }
    }
    out.push('[');
    for &(a, b) in ranges {
        if a == b {
            out.push_str(&format!("\\x{{{a:x}}}"));
        } else {
            out.push_str(&format!("\\x{{{a:x}}}-\\x{{{b:x}}}"));
        }
    }
    out.push(']');
}

/// Rewrite UTF-16 surrogate-pair escape sequences into the astral scalar values
/// they encode, so the Rust `regex` crate (which works on Unicode scalars and
/// rejects lone-surrogate code points) can compile them.
///
/// JS regexes that target the supplementary planes without the `u` flag spell
/// each astral code point as a high-surrogate escape immediately followed by a
/// low-surrogate escape — either as bare `\uXXXX` escapes or as `[...]` classes
/// of them, e.g. `\uD800[\uDC00-\uDC0B]` or
/// `[\uD80C\uD81C-\uD820][\uDC00-\uDFFF]`. Test262's `nativeFunctionMatcher.js`
/// (the `\p{ID_Start}` / `\p{ID_Continue}` shims used across `built-ins/`)
/// relies on this form; before this fold every `Function.prototype.toString`
/// conformance case threw `SyntaxError: invalid pattern` at regex-literal
/// evaluation. The transform only fires when a high-surrogate unit is directly
/// followed by a low-surrogate unit (a genuine pair); anything else is left
/// byte-for-byte unchanged, so patterns that compile today are unaffected.
fn fold_surrogate_pairs(pattern: &str) -> String {
    if !pattern.contains("\\u") {
        return pattern.to_string();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0;
    while i < chars.len() {
        let at_unit_start = (chars[i] == '\\' && chars.get(i + 1) == Some(&'u')) || chars[i] == '[';
        if at_unit_start {
            if let Some((hi, j)) = parse_surrogate_unit(&chars, i) {
                if hi
                    .iter()
                    .all(|(a, b)| is_high_surrogate(*a) && is_high_surrogate(*b))
                {
                    if let Some((lo, k)) = parse_surrogate_unit(&chars, j) {
                        if lo
                            .iter()
                            .all(|(a, b)| is_low_surrogate(*a) && is_low_surrogate(*b))
                        {
                            emit_astral_class(&mut out, &combine_surrogate_ranges(&hi, &lo));
                            i = k;
                            continue;
                        }
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Translate a JavaScript regex pattern to a Rust regex-crate compatible pattern.
/// Handles JS-specific escape sequences not supported by the Rust regex crate.
/// Also converts JS-style named groups `(?<name>...)` to Rust-style `(?P<name>...)`.
pub(super) fn js_regex_to_rust(pattern: &str) -> String {
    let folded = fold_surrogate_pairs(pattern);
    let mut result = String::with_capacity(folded.len());
    let chars: Vec<char> = folded.chars().collect();
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
                i += 1;
            } else if chars.get(i + 1) == Some(&']') {
                // JS: `[]` is an *empty* character class that never matches
                // (the `]` immediately after `[` closes the class). The Rust
                // `regex` crate rejects `[]`, so emit an unsatisfiable class.
                result.push_str("[^\\s\\S]");
                i += 2;
            } else if chars.get(i + 1) == Some(&'^') && chars.get(i + 2) == Some(&']') {
                // JS: `[^]` is a negated empty class — it matches *any* code
                // point, including line terminators. Rust rejects `[^]`, so
                // emit the equivalent `[\s\S]`.
                result.push_str("[\\s\\S]");
                i += 3;
            } else {
                in_class = true;
                result.push('[');
                i += 1;
            }
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
