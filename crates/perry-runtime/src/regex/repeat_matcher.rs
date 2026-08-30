//! ECMA-262 `RepeatMatcher` compatibility path.
//!
//! Rust's linear `regex` engine intentionally does not implement JavaScript's
//! backtracking capture semantics. In particular, captures nested below a
//! quantified group must be cleared before every iteration, and an optional
//! iteration that matches the empty string must be discarded. Keep the linear
//! engine as the default, but compile patterns where those captures are
//! observable with `regress`, an ECMAScript-native backtracking matcher.

/// A compiled matcher plus the capture-name ordering that the public `regress`
/// match API does not expose directly.
pub(super) struct RepeatMatcherRegex {
    pub(super) regex: regress::Regex,
    pub(super) capture_names: Vec<Option<String>>,
}

impl RepeatMatcherRegex {
    fn named_group_range(
        &self,
        matched: &regress::Match,
        name: &str,
    ) -> Option<std::ops::Range<usize>> {
        self.capture_names
            .iter()
            .position(|candidate| candidate.as_deref() == Some(name))
            .and_then(|index| matched.group(index + 1))
    }

    pub(super) fn expand_replacement(
        &self,
        replacement: &str,
        matched: &regress::Match,
        subject: &str,
    ) -> String {
        let bytes = replacement.as_bytes();
        let group_count = matched.captures.len() + 1;
        let has_named_groups = self.capture_names.iter().any(Option::is_some);
        let mut out = String::with_capacity(replacement.len() + 16);
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'$' {
                let start = index;
                while index < bytes.len() && bytes[index] != b'$' {
                    index += 1;
                }
                out.push_str(&replacement[start..index]);
                continue;
            }
            if index + 1 >= bytes.len() {
                out.push('$');
                break;
            }
            match bytes[index + 1] {
                b'$' => {
                    out.push('$');
                    index += 2;
                }
                b'&' => {
                    out.push_str(&subject[matched.range()]);
                    index += 2;
                }
                b'`' => {
                    out.push_str(&subject[..matched.start()]);
                    index += 2;
                }
                b'\'' => {
                    out.push_str(&subject[matched.end()..]);
                    index += 2;
                }
                b'0'..=b'9' => {
                    let first = (bytes[index + 1] - b'0') as usize;
                    let (group, consumed) =
                        if index + 2 < bytes.len() && bytes[index + 2].is_ascii_digit() {
                            let two = first * 10 + (bytes[index + 2] - b'0') as usize;
                            if (1..group_count).contains(&two) {
                                (Some(two), 2)
                            } else if (1..group_count).contains(&first) {
                                (Some(first), 1)
                            } else {
                                (None, 0)
                            }
                        } else if (1..group_count).contains(&first) {
                            (Some(first), 1)
                        } else {
                            (None, 0)
                        };
                    if let Some(group) = group {
                        if let Some(range) = matched.group(group) {
                            out.push_str(&subject[range]);
                        }
                        index += 1 + consumed;
                    } else {
                        out.push('$');
                        index += 1;
                    }
                }
                b'<' if has_named_groups => {
                    if let Some(relative_end) = replacement[index + 2..].find('>') {
                        let name = &replacement[index + 2..index + 2 + relative_end];
                        if let Some(range) = self.named_group_range(matched, name) {
                            out.push_str(&subject[range]);
                        }
                        index += relative_end + 3;
                    } else {
                        out.push('$');
                        index += 1;
                    }
                }
                _ => {
                    out.push('$');
                    index += 1;
                }
            }
        }
        out
    }

    pub(super) fn replace(&self, subject: &str, replacement: &str, global: bool) -> String {
        let mut out = String::new();
        let mut last_end = 0;
        for matched in self.regex.find_iter(subject) {
            out.push_str(&subject[last_end..matched.start()]);
            out.push_str(&self.expand_replacement(replacement, &matched, subject));
            last_end = matched.end();
            if !global {
                break;
            }
        }
        out.push_str(&subject[last_end..]);
        out
    }

    pub(super) fn split(&self, subject: &str, limit: i32) -> Vec<Option<String>> {
        let mut out = Vec::new();
        let unbounded = limit < 0;
        let push = |out: &mut Vec<Option<String>>, value: Option<String>| -> bool {
            out.push(value);
            !unbounded && out.len() as i32 >= limit
        };
        if subject.is_empty() {
            if self.regex.find(subject).is_none() {
                out.push(Some(String::new()));
            }
            return out;
        }

        let mut pending_start = 0;
        let mut cursor = 0;
        while cursor < subject.len() {
            let Some(matched) = self.regex.find_from(subject, cursor).next() else {
                break;
            };
            if matched.start() != cursor {
                cursor = matched.start();
                continue;
            }
            let end = matched.end().min(subject.len());
            if end == pending_start {
                cursor += subject[cursor..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
                continue;
            }
            if push(&mut out, Some(subject[pending_start..cursor].to_string())) {
                return out;
            }
            for capture in matched.captures {
                let value = capture.map(|range| subject[range].to_string());
                if push(&mut out, value) {
                    return out;
                }
            }
            pending_start = end;
            cursor = end;
        }
        if unbounded || (out.len() as i32) < limit {
            out.push(Some(subject[pending_start..].to_string()));
        }
        out
    }
}

#[derive(Clone, Copy)]
struct GroupFrame {
    captures_before: usize,
    negative_lookaround: bool,
}

fn named_capture_end(bytes: &[u8], open: usize) -> Option<usize> {
    if bytes.get(open + 1) != Some(&b'?') || bytes.get(open + 2) != Some(&b'<') {
        return None;
    }
    if matches!(bytes.get(open + 3), Some(b'=') | Some(b'!')) {
        return None;
    }
    bytes[open + 3..]
        .iter()
        .position(|byte| *byte == b'>')
        .map(|offset| open + 3 + offset)
}

fn is_capturing_group(bytes: &[u8], open: usize) -> bool {
    bytes.get(open + 1) != Some(&b'?') || named_capture_end(bytes, open).is_some()
}

fn is_negative_lookaround(bytes: &[u8], open: usize) -> bool {
    bytes.get(open + 1) == Some(&b'?')
        && (bytes.get(open + 2) == Some(&b'!')
            || (bytes.get(open + 2) == Some(&b'<') && bytes.get(open + 3) == Some(&b'!')))
}

fn has_braced_quantifier(bytes: &[u8], mut index: usize) -> bool {
    if bytes.get(index) != Some(&b'{') {
        return false;
    }
    index += 1;
    let digits_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if index == digits_start {
        return false;
    }
    if bytes.get(index) == Some(&b',') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    bytes.get(index) == Some(&b'}')
}

fn quantifier_follows(bytes: &[u8], index: usize) -> bool {
    matches!(bytes.get(index), Some(b'*') | Some(b'+') | Some(b'?'))
        || has_braced_quantifier(bytes, index)
}

/// Return the capture-name layout when a pattern needs ECMAScript backtracking
/// capture semantics. Besides quantified captures, this includes captures in a
/// negative lookaround: after a successful negative assertion those captures
/// are unmatched, so a later backreference must match the empty string.
fn quantified_capture_layout(pattern: &str) -> Option<Vec<Option<String>>> {
    let bytes = pattern.as_bytes();
    let mut captures = Vec::new();
    let mut groups = Vec::new();
    let mut needs_repeat_matcher = false;
    let mut in_class = false;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = (index + 2).min(bytes.len()),
            b'[' if !in_class => {
                in_class = true;
                index += 1;
            }
            b']' if in_class => {
                in_class = false;
                index += 1;
            }
            b'(' if !in_class => {
                let captures_before = captures.len();
                if is_capturing_group(bytes, index) {
                    let name = named_capture_end(bytes, index)
                        .map(|end| pattern[index + 3..end].to_string());
                    captures.push(name);
                }
                groups.push(GroupFrame {
                    captures_before,
                    negative_lookaround: is_negative_lookaround(bytes, index),
                });
                index += 1;
            }
            b')' if !in_class => {
                let Some(group) = groups.pop() else {
                    index += 1;
                    continue;
                };
                if captures.len() > group.captures_before
                    && (quantifier_follows(bytes, index + 1) || group.negative_lookaround)
                {
                    needs_repeat_matcher = true;
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    needs_repeat_matcher.then_some(captures)
}

pub(super) fn compile(pattern: &str, flags: &str) -> Option<RepeatMatcherRegex> {
    let capture_names = quantified_capture_layout(pattern)?;
    let regex = regress::Regex::with_flags(pattern, flags).ok()?;
    Some(RepeatMatcherRegex {
        regex,
        capture_names,
    })
}

fn source_and_flags(re: *const super::RegExpHeader) -> (String, String) {
    // One definition, shared with the lazy first-use builder: both need the
    // `(source, flags)` a header was constructed from, and a second copy of
    // the side-table-then-header fallback would be a place for them to drift.
    super::lazy::source_and_flags(re)
}

fn decode_wtf8_units(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let (advance, utf16_units, code_point) = crate::string::wtf8_step(bytes, offset);
        if utf16_units == 2 && code_point >= 0x10000 {
            let astral = code_point - 0x10000;
            units.push(0xD800 + (astral >> 10) as u16);
            units.push(0xDC00 + (astral & 0x3FF) as u16);
        } else if utf16_units == 1 {
            units.push(code_point as u16);
        }
        offset = (offset + advance).min(bytes.len());
    }
    units
}

fn append_wtf8_unit(out: &mut Vec<u8>, unit: u16) {
    if let Some(ch) = char::from_u32(unit as u32) {
        let mut encoded = [0u8; 3];
        out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
    } else {
        out.extend_from_slice(&[
            0xE0 | ((unit >> 12) as u8),
            0x80 | (((unit >> 6) & 0x3F) as u8),
            0x80 | ((unit & 0x3F) as u8),
        ]);
    }
}

fn append_unit_range(out: &mut Vec<u8>, units: &[u16], range: std::ops::Range<usize>) {
    for &unit in &units[range] {
        append_wtf8_unit(out, unit);
    }
}

fn append_replacement(
    out: &mut Vec<u8>,
    replacement: &[u8],
    units: &[u16],
    matched: &regress::Match,
) {
    let group_count = matched.groups().len();
    let has_named_groups = matched.named_groups().next().is_some();
    let mut index = 0usize;
    while index < replacement.len() {
        if replacement[index] != b'$' || index + 1 == replacement.len() {
            out.push(replacement[index]);
            index += 1;
            continue;
        }
        match replacement[index + 1] {
            b'$' => {
                out.push(b'$');
                index += 2;
            }
            b'&' => {
                append_unit_range(out, units, matched.range());
                index += 2;
            }
            b'`' => {
                append_unit_range(out, units, 0..matched.start());
                index += 2;
            }
            b'\'' => {
                append_unit_range(out, units, matched.end()..units.len());
                index += 2;
            }
            b'0'..=b'9' => {
                let first = (replacement[index + 1] - b'0') as usize;
                let (group, consumed) =
                    if index + 2 < replacement.len() && replacement[index + 2].is_ascii_digit() {
                        let two = first * 10 + (replacement[index + 2] - b'0') as usize;
                        if (1..group_count).contains(&two) {
                            (Some(two), 2)
                        } else if (1..group_count).contains(&first) {
                            (Some(first), 1)
                        } else {
                            (None, 0)
                        }
                    } else if (1..group_count).contains(&first) {
                        (Some(first), 1)
                    } else {
                        (None, 0)
                    };
                if let Some(group) = group {
                    if let Some(range) = matched.group(group) {
                        append_unit_range(out, units, range);
                    }
                    index += 1 + consumed;
                } else {
                    out.push(b'$');
                    index += 1;
                }
            }
            b'<' if has_named_groups => {
                if let Some(relative_end) = replacement[index + 2..]
                    .iter()
                    .position(|&byte| byte == b'>')
                {
                    let name =
                        std::str::from_utf8(&replacement[index + 2..index + 2 + relative_end])
                            .unwrap_or_default();
                    if let Some(range) = matched.named_group(name) {
                        append_unit_range(out, units, range);
                    }
                    index += 3 + relative_end;
                } else {
                    out.push(b'$');
                    index += 1;
                }
            }
            _ => {
                out.push(b'$');
                index += 1;
            }
        }
    }
}

/// Replace on a WTF-8 subject by exposing its exact JavaScript UTF-16 code
/// units to the ECMAScript matcher. The returned bytes remain WTF-8 and are
/// canonicalized by the caller's string builder.
pub(super) fn replace_wtf8_subject(
    re: *const super::RegExpHeader,
    subject: &[u8],
    replacement: &[u8],
    global: bool,
) -> Option<Vec<u8>> {
    let (source, flags) = source_and_flags(re);
    let regex = regress::Regex::with_flags(&source, flags.as_str()).ok()?;
    let units = decode_wtf8_units(subject);
    let matches: Vec<regress::Match> = if flags.contains('u') || flags.contains('v') {
        regex.find_from_utf16(&units, 0).collect()
    } else {
        regex.find_from_ucs2(&units, 0).collect()
    };

    let mut out = Vec::with_capacity(subject.len().saturating_add(replacement.len()));
    let mut last_end = 0usize;
    for matched in matches.iter().take(if global { usize::MAX } else { 1 }) {
        append_unit_range(&mut out, &units, last_end..matched.start());
        append_replacement(&mut out, replacement, &units, matched);
        last_end = matched.end();
    }
    append_unit_range(&mut out, &units, last_end..units.len());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_only_quantified_groups_with_captures() {
        assert!(quantified_capture_layout(r"(a?b??)*").is_some());
        assert!(quantified_capture_layout(r"(?:(?=(abc))){0,1}a").is_some());
        assert!(quantified_capture_layout(r"(?!(a)b)\1").is_some());
        assert!(quantified_capture_layout(r"(?<!(a)b)\1").is_some());
        assert!(quantified_capture_layout(r"[()]\\(literal\\)").is_none());
        assert!(quantified_capture_layout(r"(?:ab)*").is_none());
        assert!(quantified_capture_layout(r"(ab)c").is_none());
    }

    #[test]
    fn records_named_capture_indices() {
        assert_eq!(
            quantified_capture_layout(r"(?:(?<first>a)(b))*"),
            Some(vec![Some("first".to_string()), None])
        );
    }
}
