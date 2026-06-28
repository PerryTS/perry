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

/// Annex B.1.4: emit a `\<digits>` escape that is *not* a valid backreference as
/// a `LegacyOctalEscapeSequence` (or a `NonOctalDecimalEscapeSequence` for a
/// leading `8`/`9`). `start` indexes the first digit (just past the backslash).
/// Returns the number of digit chars consumed.
///
/// `\1` with no group 1, or `\2` referencing a non-existent group, must compile
/// as the literal byte rather than throwing — the `regex`/`fancy-regex` crates
/// reject `\1` outright, so we lower it to `\x{HH}`.
fn push_legacy_octal_escape(out: &mut String, chars: &[char], start: usize) -> usize {
    let first = chars[start];
    // `\8` / `\9` are not octal: they match the literal digit.
    if first == '8' || first == '9' {
        push_escaped_literal(out, first);
        return 1;
    }
    // Up to three octal digits, but only two when the first is `4`–`7`
    // (the value must stay ≤ 0o377 = 255).
    let max = if matches!(first, '0'..='3') { 3 } else { 2 };
    let mut value: u32 = 0;
    let mut n = 0;
    while n < max && start + n < chars.len() && matches!(chars[start + n], '0'..='7') {
        value = value * 8 + (chars[start + n] as u32 - '0' as u32);
        n += 1;
    }
    push_hex_escape(out, value as u8);
    n
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

/// Annex B.1.4 does NOT apply to Unicode-mode (`/u` or `/v`) patterns: the
/// legacy escapes that `js_regex_to_rust` silently relaxes for sloppy patterns
/// (a `LegacyOctalEscapeSequence`, a `NonOctalDecimalEscapeSequence`, or a `\c`
/// not followed by an ASCII control letter) are a hard `SyntaxError` under
/// `/u`. Returns `true` if `pattern` contains any such escape, so the caller
/// can throw at construction instead of compiling a relaxed pattern.
///
/// Mirrors exactly the escapes the sloppy-mode translator would reinterpret:
/// any other escape (a valid backreference, `\d`, `\x41`, `\u{…}`, an escaped
/// metacharacter, …) is left for the normal translation path.
pub(super) fn has_unicode_forbidden_legacy_escape(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let spans = collect_capture_spans(&chars);
    let num_groups = spans.len();
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                match chars[i + 1] {
                    // `\c` is a valid control escape only when followed by an
                    // ASCII control letter (`A`–`Z` / `a`–`z`); anything else
                    // (a digit, `_`, a non-ASCII letter, end-of-pattern) is the
                    // Annex B identity escape, forbidden under `/u`.
                    'c' if !matches!(chars.get(i + 2), Some(c) if c.is_ascii_alphabetic()) => {
                        return true;
                    }
                    // `\0` is the NUL escape (valid under `/u`) only when not
                    // followed by a decimal digit; `\0DD` is a legacy octal.
                    '0' if matches!(chars.get(i + 2), Some(c) if c.is_ascii_digit()) => {
                        return true;
                    }
                    // `\1`–`\9`: a backreference to an existing group is valid;
                    // anything else is a legacy octal / non-octal decimal escape.
                    // Inside a class a decimal escape is never a backreference.
                    '1'..='9' => {
                        let (group, _) = parse_decimal_escape(&chars, i + 1);
                        if in_class || group == 0 || group > num_groups {
                            return true;
                        }
                    }
                    _ => {}
                }
                // Any backslash escape consumes the following char, so `\[`,
                // `\]`, and `\\` never toggle class state.
                i += 2;
                continue;
            }
            '[' => in_class = true,
            ']' => in_class = false,
            _ => {}
        }
        i += 1;
    }
    false
}

/// The `SyntaxCharacter` set — the only characters a bare `\X` IdentityEscape
/// may carry under `/u` (plus `/`, handled by the caller). Mirrors the spec's
/// `SyntaxCharacter :: one of ^ $ \ . * + ? ( ) [ ] { } |`.
fn is_unicode_syntax_character(c: char) -> bool {
    matches!(
        c,
        '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
    )
}

/// Length (including the leading backslash) of a well-formed escape under `/u`
/// starting at index `bs` (which must point at `\`), or `None` if the escape is
/// not permitted in Unicode mode. `in_class` selects between the AtomEscape and
/// ClassEscape grammars (e.g. `\b` is a word boundary in an Atom but a backspace
/// in a class, while `\B` and `\k<name>` are class-illegal).
///
/// Annex B.1.4's lenient IdentityEscape (`\X` for an arbitrary SourceCharacter)
/// does NOT apply under `/u`: only SyntaxCharacters, `/`, the recognised
/// character-class/control escapes, and the *complete* `\xHH` / `\uHHHH` /
/// `\u{…}` / `\cX` / `\p{…}` / `\k<…>` forms are accepted. A bare `\x`, `\u`,
/// `\c`, `\p`, `\k`, or `\A` is a `SyntaxError`.
fn unicode_escape_len(chars: &[char], bs: usize, in_class: bool) -> Option<usize> {
    let c = *chars.get(bs + 1)?; // a trailing lone backslash is never valid
    let after = bs + 2; // index just past the escape letter
    let is_hex = |idx: usize| chars.get(idx).is_some_and(|h| h.is_ascii_hexdigit());
    match c {
        // CharacterClassEscape / control escapes — fixed two-char forms.
        'd' | 'D' | 's' | 'S' | 'w' | 'W' | 'f' | 'n' | 'r' | 't' | 'v' => Some(2),
        // `\b` word boundary (Atom) / backspace (class); `\B` is class-illegal.
        'b' => Some(2),
        'B' => (!in_class).then_some(2),
        // `\0` NUL — only when not the prefix of a legacy octal (`\0DD`), which
        // `has_unicode_forbidden_legacy_escape` already rejects.
        '0' => match chars.get(after) {
            Some(d) if d.is_ascii_digit() => None,
            _ => Some(2),
        },
        // Decimal backreference (Atom only). Validity of the target group is
        // checked by `has_unicode_forbidden_legacy_escape`; here we only consume.
        '1'..='9' if !in_class => {
            let (_, digits) = parse_decimal_escape(chars, bs + 1);
            Some(1 + digits)
        }
        // `\cX` control letter.
        'c' => match chars.get(after) {
            Some(x) if x.is_ascii_alphabetic() => Some(3),
            _ => None,
        },
        // `\xHH` — exactly two hex digits.
        'x' if is_hex(after) && is_hex(after + 1) => Some(4),
        // `\uHHHH` or `\u{H+}`.
        'u' => {
            if matches!(chars.get(after), Some('{')) {
                let mut j = after + 1;
                let start = j;
                while chars.get(j).is_some_and(|h| h.is_ascii_hexdigit()) {
                    j += 1;
                }
                (j > start && matches!(chars.get(j), Some('}'))).then(|| j + 1 - bs)
            } else if (0..4).all(|k| is_hex(after + k)) {
                Some(6)
            } else {
                None
            }
        }
        // `\p{…}` / `\P{…}` property escape (Unicode mode only — handled here).
        'p' | 'P' if matches!(chars.get(after), Some('{')) => {
            let mut j = after + 1;
            while chars.get(j).is_some_and(|ch| *ch != '}') {
                j += 1;
            }
            matches!(chars.get(j), Some('}')).then(|| j + 1 - bs)
        }
        // `\k<name>` named backreference (Atom only).
        'k' if !in_class && matches!(chars.get(after), Some('<')) => {
            let mut j = after + 1;
            while chars.get(j).is_some_and(|ch| *ch != '>') {
                j += 1;
            }
            matches!(chars.get(j), Some('>')).then(|| j + 1 - bs)
        }
        // IdentityEscape: a SyntaxCharacter or `/` anywhere; `-` additionally in a
        // class (ClassEscape permits `\-`). Anything else is forbidden under `/u`.
        '/' => Some(2),
        '-' if in_class => Some(2),
        _ if is_unicode_syntax_character(c) => Some(2),
        _ => None,
    }
}

/// True if the `\` at `backslash` opens a CharacterClassEscape (`\d \D \s \S
/// \w \W` or a `\p{…}` / `\P{…}` property escape) — i.e. a class member that
/// may not be an endpoint of a `-` range under `/u` (`[\d-a]`, `[\p{L}-a]`).
fn class_escape_is_set(chars: &[char], backslash: usize) -> bool {
    matches!(
        chars.get(backslash + 1),
        Some('d')
            | Some('D')
            | Some('s')
            | Some('S')
            | Some('w')
            | Some('W')
            | Some('p')
            | Some('P')
    )
}

/// Annex B.1.4's leniencies that ECMAScript forbids under `/u` (beyond the
/// legacy escapes handled by [`has_unicode_forbidden_legacy_escape`]). Returns
/// `true` — so the caller throws a `SyntaxError` at construction — when the
/// pattern relies on any sloppy-mode extension that `js_regex_to_rust` would
/// otherwise quietly relax into a valid `regex`-crate pattern:
///
/// * a lone `]` or `}` PatternCharacter (must be `\]` / `\}` under `/u`);
/// * an incomplete or standalone `{` that is not a complete `{n}` / `{n,}` /
///   `{n,m}` quantifier;
/// * a `-` range whose endpoint is a `\d`-style CharacterClassEscape
///   (`[\d-a]`, `[a-\w]`, `[\s-\S]`);
/// * a quantifier applied directly to a lookaround assertion (`(?=.)*`);
/// * a forbidden IdentityEscape (`\A`, `\-` outside a class, a bare `\x` / `\u`
///   / `\p` / `\k`), via [`unicode_escape_len`].
///
/// Only sound to call when the `u` flag is set. (The `v` flag's ClassSetExpression
/// grammar differs and is validated separately.)
pub(super) fn has_unicode_forbidden_pattern(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = 0usize;
    // Whether the previous class member was a CharacterClassEscape (`\d`, …), so
    // a following `-` range can flag it. Reset on entering a class / past a `]`.
    let mut prev_member_was_class_escape = false;
    let mut prev_member_exists = false;
    // For each open `(`, whether it begins a lookaround assertion — so its `)`
    // can reject an immediately-following quantifier.
    let mut paren_is_assertion: Vec<bool> = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            match unicode_escape_len(&chars, i, in_class) {
                Some(len) => {
                    if in_class {
                        prev_member_was_class_escape = class_escape_is_set(&chars, i);
                        prev_member_exists = true;
                    }
                    i += len;
                    continue;
                }
                None => return true,
            }
        }
        if in_class {
            match c {
                ']' => {
                    in_class = false;
                    prev_member_exists = false;
                    prev_member_was_class_escape = false;
                }
                '-' => {
                    let is_first = i == class_start;
                    let is_last = matches!(chars.get(i + 1), Some(']'));
                    if !is_first && !is_last && prev_member_exists {
                        let right_is_class_escape = matches!(chars.get(i + 1), Some('\\'))
                            && class_escape_is_set(&chars, i + 1);
                        if prev_member_was_class_escape || right_is_class_escape {
                            return true;
                        }
                    }
                    // The `-` itself becomes the previous member (a literal hyphen).
                    prev_member_was_class_escape = false;
                    prev_member_exists = true;
                }
                _ => {
                    prev_member_was_class_escape = false;
                    prev_member_exists = true;
                }
            }
            i += 1;
            continue;
        }
        match c {
            '[' => {
                in_class = true;
                class_start = i + 1;
                prev_member_exists = false;
                prev_member_was_class_escape = false;
                i += 1;
            }
            // A lone `]` / `}` PatternCharacter is forbidden under `/u`; the valid
            // occurrences (a class close, a quantifier or `\u{…}` / `\p{…}` brace)
            // are consumed before reaching here.
            ']' | '}' => return true,
            '{' => match parse_braced_quantifier(&chars, i) {
                Some(end) => i = end + 1,
                None => return true,
            },
            '(' => {
                let is_assertion = matches!(chars.get(i + 1), Some('?'))
                    && (matches!(chars.get(i + 2), Some('=') | Some('!'))
                        || (matches!(chars.get(i + 2), Some('<'))
                            && matches!(chars.get(i + 3), Some('=') | Some('!'))));
                paren_is_assertion.push(is_assertion);
                i += 1;
            }
            ')' => {
                let was_assertion = paren_is_assertion.pop().unwrap_or(false);
                if was_assertion {
                    let quantified = matches!(chars.get(i + 1), Some('*') | Some('+') | Some('?'))
                        || (matches!(chars.get(i + 1), Some('{'))
                            && parse_braced_quantifier(&chars, i + 1).is_some());
                    if quantified {
                        return true;
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    false
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

/// Parse a character-class member at `chars[i]` that is a lone-surrogate
/// `\uXXXX` escape or a `\uXXXX-\uYYYY` range of them. Returns the astral
/// scalar ranges the member matches (see `surrogate_units_to_astral`) and the
/// index just past the member. `None` leaves the escape untouched.
fn parse_class_surrogate_member(chars: &[char], i: usize) -> Option<(Vec<(u32, u32)>, usize)> {
    let (lo, j) = parse_u4_escape(chars, i)?;
    if !is_surrogate(lo) {
        return None;
    }
    if chars.get(j) == Some(&'-') && chars.get(j + 1) != Some(&']') {
        // Range form: only rewrite when the upper bound is also a surrogate
        // escape; a mixed `\ud800-z` range passes through unchanged (and fails
        // downstream exactly as before) rather than mis-translating.
        let (hi, k) = parse_u4_escape(chars, j + 1)?;
        if !is_surrogate(hi) || lo > hi {
            return None;
        }
        return Some((surrogate_units_to_astral(lo, hi), k));
    }
    Some((surrogate_units_to_astral(lo, lo), j))
}

/// Map a UTF-16 surrogate code-unit range `[a, b]` to the Unicode scalar
/// values whose UTF-16 encoding contains a unit in that range — the set a
/// non-`u` JS class member like `[\ud800-\udfff]` matches when run over a
/// well-formed string. A unit in the high half (`D800-DBFF`) selects the
/// contiguous astral block it leads; a unit in the low half (`DC00-DFFF`)
/// selects one offset out of every 0x400-wide astral block (a striped set,
/// emitted exactly; the full low half collapses to "all astral"). Lone
/// surrogates in the *subject* string remain unmatchable — the Rust `regex`
/// crate only matches scalar values (the WTF-8 categorical gap).
fn surrogate_units_to_astral(a: u32, b: u32) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    let (h1, h2) = (a.max(0xD800), b.min(0xDBFF));
    if h1 <= h2 {
        out.push((
            0x10000 + (h1 - 0xD800) * 0x400,
            0x10000 + (h2 - 0xD800) * 0x400 + 0x3FF,
        ));
    }
    let (l1, l2) = (a.max(0xDC00), b.min(0xDFFF));
    if l1 <= l2 {
        if (l1, l2) == (0xDC00, 0xDFFF) {
            out.push((0x10000, 0x10FFFF));
        } else {
            for block in 0..0x400u32 {
                let base = 0x10000 + block * 0x400;
                out.push((base + (l1 - 0xDC00), base + (l2 - 0xDC00)));
            }
        }
    }
    out.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (x, y) in out {
        match merged.last_mut() {
            Some(last) if x <= last.1.saturating_add(1) => {
                if y > last.1 {
                    last.1 = y;
                }
            }
            _ => merged.push((x, y)),
        }
    }
    merged
}

/// Emit astral ranges as members of an already-open character class.
fn emit_astral_class_members(out: &mut String, ranges: &[(u32, u32)]) {
    for &(x, y) in ranges {
        if x == y {
            out.push_str(&format!("\\x{{{x:x}}}"));
        } else {
            out.push_str(&format!("\\x{{{x:x}}}-\\x{{{y:x}}}"));
        }
    }
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
                    // Distribute a high-surrogate unit over an immediately
                    // following non-capturing group: `H(?:A|B)` ≡ `(?:HA|HB)`.
                    // emoji-regex (and string-width / ink, #348) factor the
                    // leading high surrogate out before a group of
                    // low-surrogate-led alternatives, e.g.
                    // `\uD83C(?:[\uDC04…]️?|\uDDE6\uD83C[\uDDE8…]|…)`, so
                    // the high surrogate is no longer directly adjacent to its
                    // low half and the pair fold above can't see it — the lone
                    // `\uD83C` then reaches the `regex` crate as a surrogate
                    // scalar and the whole pattern is rejected as `invalid
                    // pattern`. Re-prepend H to each alternative and recurse,
                    // restoring adjacency so each `HA` folds normally. Only
                    // fires when every alternative begins with a low-surrogate
                    // unit and the group is unquantified, so patterns that
                    // compile today are byte-for-byte unaffected.
                    if let Some((rebuilt, next)) = distribute_high_over_group(&chars, i, j) {
                        out.push_str(&fold_surrogate_pairs(&rebuilt));
                        i = next;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Whether `s` begins with a low-surrogate unit (a `\uDCxx` escape or a `[…]`
/// class of only low-surrogate escapes).
fn starts_with_low_unit(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    matches!(
        parse_surrogate_unit(&c, 0),
        Some((ref r, _)) if r.iter().all(|(a, b)| is_low_surrogate(*a) && is_low_surrogate(*b))
    )
}

/// Parse a low-surrogate unit at `chars[p]` (single escape or class). Returns its
/// source text and the index just past it, or `None` if it is not a low unit.
fn take_low_unit(chars: &[char], p: usize) -> Option<(String, usize)> {
    let (r, end) = parse_surrogate_unit(chars, p)?;
    r.iter()
        .all(|(a, b)| is_low_surrogate(*a) && is_low_surrogate(*b))
        .then(|| (chars[p..end].iter().collect(), end))
}

/// Distribute the high-surrogate unit `chars[i..j]` into an immediately following
/// non-capturing group so each low half becomes adjacent to it (the existing
/// pair fold then collapses them to astral scalars). emoji-regex / string-width
/// (ink, #348) factor a shared high surrogate out before a group of
/// low-surrogate-led alternatives:
///
/// * plain group `H(?:A|B)` ≡ `(?:HA|HB)` — H pairs inside the group;
/// * optional group followed by a low unit `H(?:G)?L` ≡ `(?:HGL|HL)` — H pairs
///   with the group's first low half (when present) or with `L` (when absent),
///   as in the ZWJ "kiss"/"family" sequences `\uD83D(?:\uDC8B‍\uD83D)?[\uDC68\uDC69]`.
///
/// Returns the rewritten `(?:…)` and the index to resume scanning at, or `None`
/// (leaving the segment byte-for-byte unchanged) when the shape does not match —
/// so patterns that compile today are unaffected. The rewrite is re-folded by the
/// caller, which resolves any nested high-surrogate groups recursively.
fn distribute_high_over_group(chars: &[char], i: usize, j: usize) -> Option<(String, usize)> {
    if chars.get(j) != Some(&'(')
        || chars.get(j + 1) != Some(&'?')
        || chars.get(j + 2) != Some(&':')
    {
        return None;
    }
    let close = matching_paren(chars, j)?;
    let alts = split_alternatives(chars, j + 3, close);
    if alts.is_empty() || !alts.iter().all(|a| starts_with_low_unit(a)) {
        return None;
    }
    let high_src: String = chars[i..j].iter().collect();
    match chars.get(close + 1) {
        // Optional group: pull a trailing low unit into both branches.
        Some('?') => {
            let (low_src, after) = take_low_unit(chars, close + 2)?;
            let mut rebuilt = String::from("(?:");
            for a in &alts {
                rebuilt.push_str(&high_src);
                rebuilt.push_str(a);
                rebuilt.push_str(&low_src);
                rebuilt.push('|');
            }
            rebuilt.push_str(&high_src);
            rebuilt.push_str(&low_src);
            rebuilt.push(')');
            Some((rebuilt, after))
        }
        // Other quantifiers can't be hoisted into the group; bail.
        Some('*' | '+' | '{') => None,
        // Plain group: H pairs with each alternative's leading low half.
        _ => {
            let mut rebuilt = String::from("(?:");
            for (idx, a) in alts.iter().enumerate() {
                if idx > 0 {
                    rebuilt.push('|');
                }
                rebuilt.push_str(&high_src);
                rebuilt.push_str(a);
            }
            rebuilt.push(')');
            Some((rebuilt, close + 1))
        }
    }
}

/// Index of the `)` matching the `(` at `chars[open]`, scanning depth-aware and
/// skipping `\`-escapes and `[…]` classes. `None` if unbalanced.
fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_class = false;
    let mut k = open;
    while k < chars.len() {
        let c = chars[k];
        if c == '\\' {
            k += 2;
            continue;
        }
        if in_class {
            if c == ']' {
                in_class = false;
            }
        } else if c == '[' {
            in_class = true;
        } else if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(k);
            }
        }
        k += 1;
    }
    None
}

/// Split `chars[start..end]` into its top-level `|`-separated alternatives,
/// honoring `\`-escapes, `[…]` classes, and `(…)` nesting.
fn split_alternatives(chars: &[char], start: usize, end: usize) -> Vec<String> {
    let mut alts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_class = false;
    let mut k = start;
    while k < end {
        let c = chars[k];
        if c == '\\' {
            cur.push(c);
            if k + 1 < end {
                cur.push(chars[k + 1]);
            }
            k += 2;
            continue;
        }
        if in_class {
            cur.push(c);
            if c == ']' {
                in_class = false;
            }
        } else if c == '[' {
            in_class = true;
            cur.push(c);
        } else if c == '(' {
            depth += 1;
            cur.push(c);
        } else if c == ')' {
            depth -= 1;
            cur.push(c);
        } else if c == '|' && depth == 0 {
            alts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        k += 1;
    }
    alts.push(cur);
    alts
}

/// Parse a `\p{...}` / `\P{...}` Unicode property escape starting at `chars[i]`
/// (which must be the backslash, with `chars[i+1]` a `p`/`P` and `chars[i+2]` a
/// `{`). Returns `(property_value, negated, end)` where `end` is the index just
/// past the closing `}` and `property_value` is the lowercased value with any
/// `gc=` / `general_category=` prefix stripped and `_`/spaces removed (loose
/// matching). Returns `None` if the brace form is malformed.
fn parse_unicode_property(chars: &[char], i: usize) -> Option<(String, bool, usize)> {
    let negated = match chars.get(i + 1) {
        Some('p') => false,
        Some('P') => true,
        _ => return None,
    };
    if chars.get(i + 2) != Some(&'{') {
        return None;
    }
    let mut k = i + 3;
    let mut body = String::new();
    while let Some(&c) = chars.get(k) {
        if c == '}' {
            break;
        }
        body.push(c);
        k += 1;
    }
    if chars.get(k) != Some(&'}') {
        return None;
    }
    let lower = body.to_ascii_lowercase();
    // Strip a `gc=` / `general_category=` prefix; leave other `key=value`
    // properties (e.g. `script=greek`) intact so they pass through unchanged.
    let value = match lower.split_once('=') {
        Some((key, val))
            if matches!(
                key.trim().replace(['_', ' '], "").as_str(),
                "gc" | "generalcategory"
            ) =>
        {
            val.to_string()
        }
        _ => lower,
    };
    Some((value.trim().replace(['_', ' '], ""), negated, k + 1))
}

/// `\p{Surrogate}` / `\p{gc=Cs}` — the only general category consisting entirely
/// of UTF-16 surrogate code points (U+D800..=U+DFFF). The Rust `regex` crate
/// matches over Unicode *scalar values*, which exclude surrogates, so it rejects
/// this property outright instead of treating it as never-matching.
fn is_surrogate_property(value: &str) -> bool {
    value == "surrogate" || value == "cs"
}

/// ES2024 Unicode *properties of strings* (`/v` / unicodeSets mode, UTS #51
/// emoji sequence sets). Unlike ordinary `\p{…}` character properties these
/// can match a multi-code-point cluster (ZWJ sequences, flag pairs, keycaps,
/// skin-tone modifier sequences). The Rust `regex` crate has no notion of
/// properties of strings, so it rejects them as `invalid pattern`. Expand each
/// set into an alternation over the single-code-point emoji properties the
/// crate does support (`Emoji`, `Emoji_Presentation`, `Emoji_Modifier`,
/// `Emoji_Modifier_Base`).
///
/// The expansion follows the UTS #51 sequence *grammar*, not the enumerated
/// RGI sequence data files, so it over-matches at rare edges (unlisted flag
/// pairs / ZWJ combinations) but classifies real emoji clusters the way Node
/// does — which is what `string-width@7+` (→ ink, #348) needs for its
/// module-top-level `/^\p{RGI_Emoji}$/v` "is this cluster one emoji → width 2"
/// predicate (#4889). Takes the already-normalized (lowercased, `_`/space
/// stripped) property value; returns `None` for everything else.
fn emoji_string_property_expansion(value: &str) -> Option<String> {
    // One emoji-sequence element: a skin-tone modifier sequence, an emoji
    // presentation sequence (text-default emoji + VS16), or a character with
    // default emoji presentation. Regional indicators are excluded — they
    // only count in pairs, as a flag sequence.
    const ELEMENT: &str = "(?:\\p{Emoji_Modifier_Base}\\p{Emoji_Modifier}\
         |\\p{Emoji}\\x{FE0F}\
         |[\\p{Emoji_Presentation}&&[^\\x{1F1E6}-\\x{1F1FF}]])";
    const FLAG_SEQ: &str = "[\\x{1F1E6}-\\x{1F1FF}]{2}";
    const KEYCAP_SEQ: &str = "[0-9#*]\\x{FE0F}\\x{20E3}";
    const TAG_SEQ: &str = "\\x{1F3F4}[\\x{E0020}-\\x{E007E}]+\\x{E007F}";
    Some(match value {
        // RGI_Emoji = Basic_Emoji | Emoji_Keycap_Sequence |
        // RGI_Emoji_Flag_Sequence | RGI_Emoji_Tag_Sequence |
        // RGI_Emoji_Modifier_Sequence | RGI_Emoji_ZWJ_Sequence. The trailing
        // ELEMENT(ZWJ ELEMENT)* branch covers Basic_Emoji, modifier sequences,
        // and ZWJ sequences in one.
        "rgiemoji" => {
            format!("(?:{FLAG_SEQ}|{KEYCAP_SEQ}|{TAG_SEQ}|{ELEMENT}(?:\\x{{200D}}{ELEMENT})*)")
        }
        // ELEMENT minus the modifier-sequence branch: a default-presentation
        // emoji (incl. standalone skin tones) or text-default emoji + VS16.
        "basicemoji" => "(?:\\p{Emoji}\\x{FE0F}\
             |[\\p{Emoji_Presentation}&&[^\\x{1F1E6}-\\x{1F1FF}]])"
            .to_string(),
        "emojikeycapsequence" => format!("(?:{KEYCAP_SEQ})"),
        "rgiemojiflagsequence" => format!("(?:{FLAG_SEQ})"),
        "rgiemojitagsequence" => format!("(?:{TAG_SEQ})"),
        "rgiemojimodifiersequence" => "(?:\\p{Emoji_Modifier_Base}\\p{Emoji_Modifier})".to_string(),
        "rgiemojizwjsequence" => format!("(?:{ELEMENT}(?:\\x{{200D}}{ELEMENT})+)"),
        _ => return None,
    })
}

/// Does the already-emitted translation `out` end in a character-class
/// shorthand (`\d`/`\w`/`\s` and their negations, or a `\p{…}`/`\P{…}` Unicode
/// property)? Such an element cannot be the bound of a range, so a `-`
/// immediately after it is a *literal* hyphen in JS, not a range operator.
fn out_ends_with_class_shorthand(out: &str) -> bool {
    let b = out.as_bytes();
    // `\p{…}` / `\P{…}` property: ends with `}` preceded by a `{…` opened by
    // an unescaped `\p` / `\P`.
    if b.last() == Some(&b'}') {
        if let Some(open) = out.rfind('{') {
            let pre = &out[..open];
            let pb = pre.as_bytes();
            if pb.len() >= 2
                && matches!(pb[pb.len() - 1], b'p' | b'P')
                && pb[pb.len() - 2] == b'\\'
                && !is_escaped_backslash(pb, pb.len() - 2)
            {
                return true;
            }
        }
    }
    if b.len() < 2 {
        return false;
    }
    let last = b[b.len() - 1];
    b[b.len() - 2] == b'\\'
        && !is_escaped_backslash(b, b.len() - 2)
        && matches!(last, b'd' | b'D' | b'w' | b'W' | b's' | b'S')
}

/// Is the backslash at `b[bs]` itself escaped (i.e. preceded by an odd run of
/// backslashes)? Used so `\\d` (literal backslash + `d`) isn't mistaken for the
/// `\d` shorthand.
fn is_escaped_backslash(b: &[u8], bs: usize) -> bool {
    let mut count = 0usize;
    let mut k = bs;
    while k > 0 && b[k - 1] == b'\\' {
        count += 1;
        k -= 1;
    }
    count % 2 == 1
}

/// Will the next class member at `chars[i..]` (where `chars[i]` is a `\`) be a
/// shorthand class (`\d`/`\w`/`\s` & negations, or `\p{…}`/`\P{…}`)? A `-`
/// directly before such an element is a literal hyphen in JS.
fn next_is_class_shorthand(chars: &[char], i: usize) -> bool {
    if chars.get(i) != Some(&'\\') {
        return false;
    }
    matches!(
        chars.get(i + 1),
        Some('d' | 'D' | 'w' | 'W' | 's' | 'S' | 'p' | 'P')
    )
}

/// Translate a JavaScript regex pattern to a Rust regex-crate compatible pattern.
/// Handles JS-specific escape sequences not supported by the Rust regex crate.
/// Also converts JS-style named groups `(?<name>...)` to Rust-style `(?P<name>...)`.
/// JS allows a quantifier directly on a zero-width assertion group
/// (`(?=…)?`, `(?!…)*`, `(?<=…)+`, `(?<!…){0,3}`, …). V8 accepts these and
/// treats them per the quantifier's lower bound: a lower bound of 0 makes the
/// assertion optional, which — since the assertion consumes nothing — is a
/// pure no-op; a lower bound of ≥1 is identical to the bare assertion (the
/// assertion either holds or it doesn't, and matching it more than once at the
/// same position adds nothing). The `regex` crate has no lookaround at all, and
/// `fancy-regex` (which Perry falls back to for lookaround) *rejects a
/// quantifier applied to a lookaround group* — so a JS-valid pattern like Next's
/// UA-parser table (`(?=lg)?[vl]k…`) would otherwise throw `SyntaxError: invalid
/// pattern` at regex construction, aborting the whole module that defines it.
///
/// This pre-pass rewrites each quantified lookaround into the equivalent
/// fancy-regex-acceptable form before the main translation runs:
///   * quantifier lower bound 0 (`?`, `*`, `{0,…}`, `{0}`) → drop the assertion
///     and its quantifier entirely (no-op).
///   * quantifier lower bound ≥1 (`+`, `{1,…}`, `{2}`, …) → keep the assertion,
///     drop the quantifier.
/// A trailing lazy `?` on the quantifier (`(?=…)*?`) is consumed too. Group
/// nesting is matched so an inner `(...)` inside the lookaround doesn't confuse
/// the close-paren scan.
fn normalize_quantified_lookaround(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    let mut in_class = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            // Copy an escape pair verbatim — a `\(` is a literal paren.
            out.push(c);
            if i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_class {
            if c == ']' {
                in_class = false;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == '[' {
            in_class = true;
            out.push(c);
            i += 1;
            continue;
        }
        // Detect a lookaround group opener: `(?=`, `(?!`, `(?<=`, `(?<!`.
        let look_len = lookaround_opener_len(&chars, i);
        if let Some(open_len) = look_len {
            // Find the matching close paren for this group (nesting-aware).
            if let Some(close) = matching_group_close(&chars, i) {
                // Is there a quantifier right after the close paren?
                if let Some((qstart, qend, lower_zero)) = quantifier_after(&chars, close + 1) {
                    let _ = open_len;
                    if lower_zero {
                        // No-op: drop the whole assertion + quantifier.
                    } else {
                        // Keep the assertion verbatim, drop the quantifier.
                        for ch in &chars[i..=close] {
                            out.push(*ch);
                        }
                    }
                    let _ = qstart;
                    i = qend;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// If `chars[i]` begins a lookaround opener (`(?=`, `(?!`, `(?<=`, `(?<!`),
/// return the opener length (3 or 4); otherwise `None`.
fn lookaround_opener_len(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) != Some(&'(') || chars.get(i + 1) != Some(&'?') {
        return None;
    }
    match chars.get(i + 2) {
        Some('=') | Some('!') => Some(3),
        Some('<') => match chars.get(i + 3) {
            Some('=') | Some('!') => Some(4),
            _ => None,
        },
        _ => None,
    }
}

/// Index of the `)` that closes the group opened at `chars[open]` (`open` must
/// point at `(`), honoring nesting, escapes, and `[...]` classes. `None` if
/// unbalanced.
fn matching_group_close(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut j = open;
    let mut in_class = false;
    while j < chars.len() {
        let c = chars[j];
        if c == '\\' {
            j += 2;
            continue;
        }
        if in_class {
            if c == ']' {
                in_class = false;
            }
            j += 1;
            continue;
        }
        match c {
            '[' => in_class = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// If a quantifier starts at `chars[start]`, return `(start, end_exclusive,
/// lower_bound_is_zero)`; otherwise `None`. Consumes a trailing lazy `?`.
fn quantifier_after(chars: &[char], start: usize) -> Option<(usize, usize, bool)> {
    let (mut end, lower_zero) = match chars.get(start) {
        Some('?') | Some('*') => (start + 1, true),
        Some('+') => (start + 1, false),
        Some('{') => {
            let close = parse_braced_quantifier(chars, start)?;
            // Lower bound is the digits between `{` and `,`/`}`.
            let mut k = start + 1;
            let mut lower = 0u64;
            let mut saw_digit = false;
            while k < chars.len() && chars[k].is_ascii_digit() {
                lower = lower
                    .saturating_mul(10)
                    .saturating_add(chars[k].to_digit(10).unwrap_or(0) as u64);
                saw_digit = true;
                k += 1;
            }
            (close + 1, saw_digit && lower == 0)
        }
        _ => return None,
    };
    // Lazy modifier on the quantifier (`*?`, `+?`, `{1,2}?`).
    if chars.get(end) == Some(&'?') {
        end += 1;
    }
    Some((start, end, lower_zero))
}

pub(super) fn js_regex_to_rust(pattern: &str) -> String {
    let folded = fold_surrogate_pairs(pattern);
    // Rewrite JS-valid quantified lookaround (`(?=…)?` etc.) that the Rust
    // `regex`/`fancy-regex` engines reject, before the main translation. See
    // `normalize_quantified_lookaround`.
    let folded = normalize_quantified_lookaround(&folded);
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
                'c' => {
                    if let Some(value) = chars.get(i + 2).copied().and_then(control_escape_value) {
                        push_hex_escape(&mut result, value);
                        i += 3;
                    } else {
                        // Annex B.1.4: a `\c` not followed by an ASCII control
                        // letter (e.g. `\cА` with a Cyrillic letter, `\c$`, or a
                        // trailing `\c`) is *not* a control escape — it is the
                        // literal two-character sequence `\` `c`. Emit an escaped
                        // backslash plus a literal `c` (the `regex`/`fancy-regex`
                        // crates reject a bare `\c`); the following char, if any,
                        // is processed normally so quantifiers/class members keep
                        // their meaning. Works the same inside a `[...]` class.
                        result.push('\\');
                        result.push('\\');
                        result.push('c');
                        i += 2;
                    }
                }
                '0' => {
                    // `\0` (NUL) and the legacy octal forms `\0DD` (Annex B.1.4)
                    // — `push_legacy_octal_escape` consumes the octal run and
                    // emits `\x{HH}`; a bare `\0` yields `\x00`.
                    let consumed = push_legacy_octal_escape(&mut result, &chars, i + 1);
                    i += 1 + consumed;
                }
                '1'..='9' => {
                    let (group, digits) = parse_decimal_escape(&chars, i + 1);
                    // Inside a `[...]` class a decimal escape is never a
                    // backreference — it is always a legacy octal/identity
                    // escape (e.g. `[\12-\14]` is the range `\x0A`–`\x0C`).
                    // Outside a class, `\<n>` is a backreference only when group
                    // `n` actually exists; otherwise Annex B.1.4 reinterprets it.
                    if !in_class && group <= capture_spans.len() {
                        if is_forward_backreference(&capture_spans, i, group) {
                            // A not-yet-closed group can't be matched by the
                            // `regex`/`fancy-regex` engines; drop the reference.
                            i += 1 + digits;
                        } else {
                            // A real backward backreference — keep it for
                            // fancy-regex (the `regex` crate has no backrefs).
                            result.push('\\');
                            for ch in &chars[i + 1..i + 1 + digits] {
                                result.push(*ch);
                            }
                            i += 1 + digits;
                        }
                    } else {
                        let consumed = push_legacy_octal_escape(&mut result, &chars, i + 1);
                        i += 1 + consumed;
                    }
                }
                'p' | 'P' if chars.get(i + 2) == Some(&'{') => {
                    // `\p{Surrogate}` / `\p{gc=Cs}` (and the `\P{...}` negation)
                    // name the UTF-16 surrogate code points, which can't occur in
                    // the Unicode *scalar values* the Rust `regex` crate matches
                    // over — so the crate rejects them as `invalid pattern`. Treat
                    // the positive form as a never-matching class and the negated
                    // form as "any scalar value". `string-width@7+` builds two
                    // module-top-level regexes that include `\p{Surrogate}`, so
                    // without this rewrite importing it (→ ink) throws at init.
                    //
                    // Properties of strings (`\p{RGI_Emoji}` and friends, #4889)
                    // are likewise unrepresentable in the crate and expand to an
                    // alternation over supported emoji properties. Negated or
                    // in-class uses stay unsupported and pass through, so RegExp
                    // construction throws a clear SyntaxError instead of
                    // mis-compiling (Node also rejects `\P{RGI_Emoji}`).
                    // All other properties pass through to the crate unchanged.
                    match parse_unicode_property(&chars, i) {
                        Some((value, negated, end)) if is_surrogate_property(&value) => {
                            if in_class {
                                // A never-matching member contributes nothing to a
                                // class union; the negation matches every scalar.
                                if negated {
                                    result.push_str("\\s\\S");
                                }
                            } else if negated {
                                result.push_str("[\\s\\S]");
                            } else {
                                result.push_str("[^\\s\\S]");
                            }
                            i = end;
                        }
                        Some((value, false, end))
                            if !in_class && emoji_string_property_expansion(&value).is_some() =>
                        {
                            result.push_str(&emoji_string_property_expansion(&value).unwrap());
                            i = end;
                        }
                        _ => {
                            result.push('\\');
                            result.push(chars[i + 1]);
                            i += 2;
                        }
                    }
                }
                // A lone-surrogate `\uXXXX` (or `\uXXXX-\uYYYY` range) inside a
                // character class: the crate rejects surrogate code points, so
                // rewrite to the astral scalars whose UTF-16 encoding contains
                // such a unit. es-toolkit's `truncate.js` builds
                // `[‍\ud800-\udfff̀-...]` at module init (→ ink/#348);
                // before this rewrite importing it threw `SyntaxError: invalid
                // pattern`. Pairs (`💍`) were already folded by
                // `fold_surrogate_pairs` and never reach this arm.
                'u' if in_class => {
                    if let Some((ranges, end)) = parse_class_surrogate_member(&chars, i) {
                        emit_astral_class_members(&mut result, &ranges);
                        i = end;
                    } else {
                        result.push('\\');
                        result.push('u');
                        i += 2;
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
        } else if in_class
            && chars[i] == '-'
            && (out_ends_with_class_shorthand(&result) || next_is_class_shorthand(&chars, i + 1))
        {
            // Inside a class, a `-` adjacent to a shorthand class (`\d`, `\w`,
            // `\s`, …, or a `\p{…}` property) is a *literal* hyphen in JS — a
            // shorthand can't be a range bound. The Rust `regex` crate instead
            // tries to read `\w-\.` / `\d-z` as a range and rejects it with a
            // `ClassRangeLiteral` parse error. Escape the hyphen so it stays a
            // literal. joi's URI validator (`[\w-\.~%\dA-Fa-f…]`, IPv6 host)
            // and many other real-world classes rely on this.
            result.push('\\');
            result.push('-');
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::has_unicode_forbidden_pattern;
    use super::js_regex_to_rust;
    use super::normalize_quantified_lookaround;

    #[test]
    fn unicode_forbidden_pattern_rejects_annex_b_leniencies() {
        // test262 built-ins/RegExp/unicode_restricted_* — Annex B.1.4 does NOT
        // apply under `/u`, so each of these must be a SyntaxError.
        let forbidden = [
            // Standalone brackets (unicode_restricted_brackets).
            "]",
            "}",
            "{",
            // Incomplete quantifiers (unicode_restricted_incomplete_quantifier).
            "a{",
            "a{1",
            "a{1,",
            "a{1,2",
            "{1",
            "{1,",
            "{1,2",
            // ClassEscape in a range (unicode_restricted_character_class_escape).
            "[\\d-a]",
            "[\\D-a]",
            "[\\s-a]",
            "[\\S-a]",
            "[\\w-a]",
            "[\\W-a]",
            "[a-\\d]",
            "[a-\\w]",
            "[\\d-\\d]",
            "[\\s-\\S]",
            // Property escapes are CharacterClassEscapes too (CodeRabbit #5749).
            "[\\p{L}-a]",
            "[a-\\p{L}]",
            "[\\p{L}-\\p{N}]",
            "[\\P{L}-a]",
            // Quantified assertions (unicode_restricted_quantifiable_assertion).
            "(?=.)*",
            "(?=.)+",
            "(?=.)?",
            "(?=.){1}",
            "(?=.){1,2}",
            "(?=.)*?",
            "(?!.)*",
            "(?<=.)*",
            "(?<!.)*",
            // Forbidden IdentityEscapes (unicode_restricted_identity_escape[_alpha]).
            "\\A",
            "\\T",
            "\\a",
            "\\z",
            "\\-",
            "\\@",
            "\\~",
            // Bare incomplete escapes — also forbidden under `/u`.
            "\\x",
            "\\u",
            "\\p",
            "\\k",
            "\\x4",
            "\\u{}",
            "[\\B]",
        ];
        for p in forbidden {
            assert!(
                has_unicode_forbidden_pattern(p),
                "expected `/{p}/u` to be rejected"
            );
        }
    }

    #[test]
    fn unicode_forbidden_pattern_accepts_valid_unicode_patterns() {
        // None of these may be falsely rejected — they are all valid under `/u`.
        let allowed = [
            "abc",
            "a{1,2}",
            "a{2}",
            "a{2,}",
            "(?=.)",
            "(?:ab)*",
            "(ab)?c",
            "(?<name>x)",
            "[a-z]",
            "[\\d]",
            "[\\d-]",
            "[-\\d]",
            "[\\p{L}-]",
            "[-\\p{L}]",
            "[\\w_]",
            "[\\b]",
            "[\\p{L}]",
            "\\d+",
            "\\w\\s\\b\\B",
            "\\.",
            "\\/",
            "\\]",
            "\\}",
            "\\{",
            "\\x41",
            "\\u0041",
            "\\u{1F600}",
            "\\p{L}",
            "\\P{N}",
            "\\cA",
            "\\n\\r\\t\\v\\f",
            "\\k<n>(?<n>x)",
            "[\\-]",
            "a|b",
            "^$",
        ];
        for p in allowed {
            assert!(
                !has_unicode_forbidden_pattern(p),
                "expected `/{p}/u` to be accepted"
            );
        }
    }

    #[test]
    fn quantified_lookaround_normalizes() {
        // #5437: a quantifier on a zero-width assertion is JS-valid but rejected
        // by both the `regex` crate and fancy-regex. Lower bound 0 → drop the
        // whole assertion (no-op); lower bound ≥1 → keep the assertion, drop the
        // quantifier. Next's UA-parser table (`(?=lg)?[vl]k…`) hits this.
        assert_eq!(normalize_quantified_lookaround("(?=lg)?x"), "x");
        assert_eq!(normalize_quantified_lookaround("(?!a)*b"), "b");
        assert_eq!(normalize_quantified_lookaround("(?<=a){0,3}b"), "b");
        assert_eq!(normalize_quantified_lookaround("(?<!a){0}b"), "b");
        assert_eq!(normalize_quantified_lookaround("(?=a)+b"), "(?=a)b");
        assert_eq!(normalize_quantified_lookaround("(?=a){2,3}b"), "(?=a)b");
        // Lazy modifier on the quantifier is consumed too.
        assert_eq!(normalize_quantified_lookaround("(?=a)*?b"), "b");
        assert_eq!(normalize_quantified_lookaround("(?=a)+?b"), "(?=a)b");
        // A bare (unquantified) lookaround is left untouched.
        assert_eq!(normalize_quantified_lookaround("(?=a)b"), "(?=a)b");
        assert_eq!(normalize_quantified_lookaround("(?!a)b"), "(?!a)b");
        // The exact Next UA-parser fragment: the inner optional lookahead drops,
        // the surrounding capture group and the rest stay intact.
        assert_eq!(
            normalize_quantified_lookaround(r"((?=lg)?[vl]k\-?\d{3}) bui"),
            r"([vl]k\-?\d{3}) bui"
        );
        // Nested group inside the lookaround: close-paren matching is depth-aware.
        assert_eq!(normalize_quantified_lookaround("(?=(ab)c)?d"), "d");
        // A literal escaped paren must not be treated as a group.
        assert_eq!(normalize_quantified_lookaround(r"\(?=a\)?b"), r"\(?=a\)?b");
        // Inside a character class, `(?=` is literal — leave it alone.
        assert_eq!(normalize_quantified_lookaround("[(?=a)]?b"), "[(?=a)]?b");
        // A quantifier on a normal (non-lookaround) group is untouched.
        assert_eq!(normalize_quantified_lookaround("(ab)?c"), "(ab)?c");
        assert_eq!(normalize_quantified_lookaround("(?:ab)?c"), "(?:ab)?c");
    }

    #[test]
    fn surrogate_property_rewrites_to_never_match() {
        // #4884: the Rust `regex` crate matches Unicode scalar values, which
        // exclude surrogate code points, so it rejects `\p{Surrogate}` outright.
        // The positive form is rewritten to a never-matching class and the
        // negation to "any scalar value".
        assert_eq!(js_regex_to_rust(r"\p{Surrogate}"), r"[^\s\S]");
        assert_eq!(js_regex_to_rust(r"\P{Surrogate}"), r"[\s\S]");
        // The `gc=Cs` / `General_Category=Surrogate` spellings normalize the same.
        assert_eq!(js_regex_to_rust(r"\p{gc=Cs}"), r"[^\s\S]");
        assert_eq!(
            js_regex_to_rust(r"\p{General_Category=Surrogate}"),
            r"[^\s\S]"
        );
        // Inside a class the positive form drops (a never-matching member adds
        // nothing to the union); the negation contributes "any scalar value".
        assert_eq!(
            js_regex_to_rust(r"[\p{Control}\p{Surrogate}]"),
            r"[\p{Control}]"
        );
        assert_eq!(js_regex_to_rust(r"[\P{Surrogate}]"), r"[\s\S]");
        // Every other property passes through to the crate unchanged.
        assert_eq!(js_regex_to_rust(r"\p{Control}"), r"\p{Control}");
        assert_eq!(js_regex_to_rust(r"\p{Script=Greek}"), r"\p{Script=Greek}");
        assert_eq!(js_regex_to_rust(r"\pL"), r"\pL");

        // The two `string-width@7+` module-top-level regexes (→ ink, #348) that
        // threw `SyntaxError: invalid pattern` at import must now compile under
        // the Rust `regex` crate.
        for pat in [
            r"^(?:\p{Default_Ignorable_Code_Point}|\p{Control}|\p{Format}|\p{Mark}|\p{Surrogate})+$",
            r"^[\p{Default_Ignorable_Code_Point}\p{Control}\p{Format}\p{Mark}\p{Surrogate}]+",
        ] {
            let translated = js_regex_to_rust(pat);
            assert!(
                regex::Regex::new(&translated).is_ok(),
                "string-width pattern failed to compile: {pat} -> {translated}"
            );
        }
    }

    #[test]
    fn class_lone_surrogate_range_rewrites_to_astral() {
        // es-toolkit `truncate.js` (→ ink, #348/#4950 verification): a class
        // mixing BMP members with the full surrogate range must compile and
        // detect astral characters like Node does (over well-formed strings).
        let pat =
            "[\\u200d\\ud800-\\udfff\\u0300-\\u036f\\ufe20-\\ufe2f\\u20d0-\\u20ff\\ufe0e\\ufe0f]";
        let translated = js_regex_to_rust(pat);
        let re = regex::Regex::new(&translated)
            .unwrap_or_else(|e| panic!("es-toolkit class failed to compile: {translated}: {e}"));
        assert!(re.is_match("a\u{1F48D}b"), "astral char must match");
        assert!(re.is_match("a\u{200D}b"), "ZWJ member must still match");
        assert!(!re.is_match("plain ascii"), "ASCII must not match");

        // Full surrogate range alone → all astral scalars.
        assert_eq!(
            js_regex_to_rust("[\\ud800-\\udfff]"),
            "[\\x{10000}-\\x{10ffff}]"
        );
        // High-only subrange → the contiguous astral block(s) it leads.
        assert_eq!(
            js_regex_to_rust("[\\ud800-\\ud801]"),
            "[\\x{10000}-\\x{107ff}]"
        );
        // A high singleton matches the astral block led by that unit:
        // `/[\ud83d]/.test("💍")` is true in JS.
        let high_single = js_regex_to_rust("[\\ud83d]");
        let re = regex::Regex::new(&high_single).unwrap();
        assert!(re.is_match("\u{1F48D}"));
        assert!(!re.is_match("\u{10000}"));
        // Low-only subranges are striped sets, emitted exactly:
        // `/[\udc8d]/.test("💍")` is true in JS (matches the low unit).
        let low_single = js_regex_to_rust("[\\udc8d]");
        let re = regex::Regex::new(&low_single).unwrap();
        assert!(re.is_match("\u{1F48D}"));
        assert!(!re.is_match("\u{1F48E}"));
        // Surrogate-to-nonsurrogate ranges stay untouched (still invalid).
        assert_eq!(js_regex_to_rust("[\\ud800-z]"), "[\\ud800-z]");
    }

    #[test]
    fn rgi_emoji_string_property_expands_and_matches_like_node() {
        // #4889: `string-width@7+` builds `/^\p{RGI_Emoji}$/v` at module top
        // level (→ ink, #348). The expected values below were verified against
        // Node's /v implementation.
        let translated = js_regex_to_rust(r"^\p{RGI_Emoji}$");
        let re = regex::Regex::new(&translated)
            .unwrap_or_else(|e| panic!("RGI_Emoji expansion failed to compile: {translated}: {e}"));
        for s in [
            "\u{1F44D}",                                   // 👍 default presentation
            "\u{1F600}",                                   // 😀
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", // 👨‍👩‍👧 ZWJ family
            "\u{1F1EC}\u{1F1E7}",                          // 🇬🇧 flag pair
            "1\u{FE0F}\u{20E3}",                           // 1️⃣ keycap
            "#\u{FE0F}\u{20E3}",                           // #️⃣ keycap
            "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}", // 🏴󠁧󠁢󠁥󠁮󠁧󠁿 tag seq
            "\u{1F44D}\u{1F3FB}",                          // 👍🏻 skin tone
            "\u{2764}\u{FE0F}",                            // ❤️ text-default + VS16
            "\u{2764}\u{FE0F}\u{200D}\u{1F525}",           // ❤️‍🔥 VS16 inside ZWJ seq
            "\u{1F3F4}\u{200D}\u{2620}\u{FE0F}",           // 🏴‍☠️ pirate flag
            "\u{1F9D4}\u{200D}\u{2640}\u{FE0F}",           // 🧔‍♀️ modifier-base, unmodified
            "\u{1F3FB}",                                   // 🏻 lone skin tone IS Basic_Emoji
        ] {
            assert!(re.is_match(s), "expected RGI_Emoji match for {s:?}");
        }
        for s in [
            "ab",
            "a",
            "0", // keycap base alone is not an emoji
            "#",
            "\u{1F1EC}",  // 🇬 lone regional indicator
            "\u{1F44D}x", // anchored: emoji + trailing char
            "",
            "\u{2601}", // ☁ text-default without VS16
            "\u{A9}",   // © text-default without VS16
        ] {
            assert!(!re.is_match(s), "expected no RGI_Emoji match for {s:?}");
        }

        // The sibling properties of strings expand too.
        let zwj = regex::Regex::new(&js_regex_to_rust(r"^\p{RGI_Emoji_ZWJ_Sequence}$")).unwrap();
        assert!(zwj.is_match("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"));
        assert!(!zwj.is_match("\u{1F44D}")); // a ZWJ sequence needs ≥2 elements
        let keycap = regex::Regex::new(&js_regex_to_rust(r"^\p{Emoji_Keycap_Sequence}$")).unwrap();
        assert!(keycap.is_match("1\u{FE0F}\u{20E3}"));
        assert!(!keycap.is_match("1"));
        let basic = regex::Regex::new(&js_regex_to_rust(r"^\p{Basic_Emoji}$")).unwrap();
        assert!(basic.is_match("\u{2601}\u{FE0F}"));
        assert!(!basic.is_match("\u{2601}"));
        let flag = regex::Regex::new(&js_regex_to_rust(r"^\p{RGI_Emoji_Flag_Sequence}$")).unwrap();
        assert!(flag.is_match("\u{1F1EC}\u{1F1E7}"));
        let tag = regex::Regex::new(&js_regex_to_rust(r"^\p{RGI_Emoji_Tag_Sequence}$")).unwrap();
        assert!(tag.is_match("\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}"));
        let modseq =
            regex::Regex::new(&js_regex_to_rust(r"^\p{RGI_Emoji_Modifier_Sequence}$")).unwrap();
        assert!(modseq.is_match("\u{1F44D}\u{1F3FB}"));

        // Negated / in-class forms stay unsupported: they pass through
        // unchanged so RegExp construction throws a clear SyntaxError instead
        // of mis-compiling. (Node rejects `\P{RGI_Emoji}` too.)
        assert_eq!(js_regex_to_rust(r"\P{RGI_Emoji}"), r"\P{RGI_Emoji}");
        assert_eq!(js_regex_to_rust(r"[\p{RGI_Emoji}]"), r"[\p{RGI_Emoji}]");
        assert!(regex::Regex::new(r"\P{RGI_Emoji}").is_err());
        assert!(fancy_regex::Regex::new(r"\P{RGI_Emoji}").is_err());
    }

    #[test]
    fn trivial_char_class_compiles_and_matches() {
        // Regression for the winston/`@colors/colors` tail: a trivial, valid
        // character class must translate and compile (and *not* be rejected by
        // the invalid-pattern guard). `[0m]` matches `0` or `m`.
        let translated = js_regex_to_rust("[0m]");
        let re = regex::Regex::new(&translated).expect("[0m] must compile");
        assert!(re.is_match("0"));
        assert!(re.is_match("m"));
        assert!(!re.is_match("x"));
        // The `@colors/colors` ANSI-strip literal `\x1B\[\d+m` and the
        // escaped-bracket form `\x1B\[0m` (`escapeStringRegexp` output) compile.
        for pat in [r"\x1B\[\d+m", r"\x1B\[0m"] {
            assert!(
                regex::Regex::new(&js_regex_to_rust(pat)).is_ok(),
                "ANSI pattern must compile: {pat}"
            );
        }
        // Neither trips the bounded-quantifier false-positive guard.
        assert!(!super::has_invalid_repeated_quantifier("[0m]"));
        assert!(!super::has_invalid_repeated_quantifier(r"\x1B\[\d+m"));
    }

    #[test]
    fn bounded_quantifier_in_class_not_rejected() {
        // semver's ReDoS-hardened `safeRe` rewrites `\d+`→`\d{1,N}`,
        // `\s*`→`\s{0,1}`, `[…]*`→`[…]{0,N}`. These bounded quantifiers are
        // valid and must NOT be flagged by `has_invalid_repeated_quantifier`.
        for pat in [
            r"\d{1,16}",
            r"\s{0,1}",
            r"\d{0,256}",
            r"[a-zA-Z0-9-]{0,250}",
            r"(?:<|>)?=?",
            r"a{0,1}",
        ] {
            assert!(
                !super::has_invalid_repeated_quantifier(pat),
                "valid bounded quantifier wrongly rejected: {pat}"
            );
        }
        // A genuinely-dangling quantifier (no preceding atom) is still caught.
        assert!(super::has_invalid_repeated_quantifier("{0,1}"));
        assert!(super::has_invalid_repeated_quantifier("*abc"));
    }

    #[test]
    fn class_hyphen_adjacent_to_shorthand_is_literal() {
        // joi's URI validator builds classes like `[\w-\.~%\dA-Fa-f…]` where a
        // `-` sits next to a `\w`/`\d` shorthand. In JS that `-` is a literal
        // hyphen (a shorthand can't bound a range); the Rust `regex` crate
        // would otherwise reject `\w-` as `ClassRangeLiteral`. The hyphen must
        // be escaped to `\-`.
        for (src, expect) in [
            (r"[\w-\.]", r"[\w\-\.]"),
            (r"[\d-z]", r"[\d\-z]"),
            (r"[a\w-]", r"[a\w\-]"),
            (r"[a-\d]", r"[a\-\d]"),
            (r"[\p{L}-x]", r"[\p{L}\-x]"),
        ] {
            assert_eq!(js_regex_to_rust(src), expect, "src={src}");
            assert!(
                regex::Regex::new(&js_regex_to_rust(src)).is_ok(),
                "must compile: {src}"
            );
        }
        // An ordinary `a-z` range between two single literals is untouched.
        assert_eq!(js_regex_to_rust("[a-z]"), "[a-z]");
        // Outside a class, `-` is never escaped.
        assert_eq!(js_regex_to_rust(r"\d-\w"), r"\d-\w");
        // A `\w-\.` member must match `\w`, a literal `-`, and `.`.
        let re = regex::Regex::new(&js_regex_to_rust(r"^[\w-\.]+$")).unwrap();
        assert!(re.is_match("a-b.c_d"));
        assert!(!re.is_match("a b"));
    }
}
