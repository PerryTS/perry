//! Translation from the `regex` crate's syntax into ECMAScript that means the
//! same thing.
//!
//! The two dialects share most of their spelling and differ in what several of
//! the shared spellings mean. Passing a `regex` pattern through unchanged
//! compiles, and then answers differently on exactly the inputs nobody tests:
//!
//! | Spelling | `regex` | ECMAScript | Emitted |
//! |---|---|---|---|
//! | `\d` | Unicode `Nd` | ASCII digits | `\p{Nd}` |
//! | `\s` | Unicode `White_Space` | a different set: adds U+FEFF, drops U+0085 | `\p{White_Space}` |
//! | `\w` | Unicode word characters | ASCII word characters | the `regex` definition, spelt out |
//! | `\b` | boundary of Unicode `\w` | boundary of ASCII `\w` | lookarounds on that class |
//! | `.` | anything but `\n` | anything but `\n`, `\r`, U+2028, U+2029 | `[^\n]` |
//! | `(?m)^` `(?m)$` | at `\n` only | also at `\r`, U+2028, U+2029 | lookarounds on `\n` |
//! | `(?i)` | Unicode simple case folding | ASCII-only unless `u` | the `u` flag |
//! | `[^x]{0,9}` | counts code points | counts UTF-16 units unless `u` | the `u` flag |
//!
//! `.` and the `m` anchors matter more than they look: a Windows checkout has
//! `\r\n` line endings, and every `(?m)^` pattern would otherwise start its
//! match one byte earlier, at the `\n`.
//!
//! Everything the translation cannot carry across with its meaning intact is
//! refused with the offset and the reason, rather than passed through. Two of
//! those are differences of *semantics* rather than spelling: ECMAScript resets
//! a capture on each repetition of its enclosing group and `regex` keeps the
//! last value, so a capture inside a repeated group is refused; and scoped or
//! mid-pattern inline flags would need the translation to track flag scope,
//! which nothing here needs, so they are refused too.

use std::fmt;

/// Where, and why, a pattern could not be translated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialectError {
    /// Byte offset into the pattern.
    pub offset: usize,
    pub reason: &'static str,
}

impl fmt::Display for DialectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at byte {}: {}", self.offset, self.reason)
    }
}

/// A translated pattern and the ECMAScript flags it must be compiled with.
#[derive(Debug, Eq, PartialEq)]
pub struct Translated {
    pub source: String,
    pub flags: &'static str,
}

/// `regex`'s Unicode `\w`, spelt as the items of a class.
const WORD: &str = r"\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}";
const NOT_DIGIT: &str = r"\P{Nd}";
const DIGIT: &str = r"\p{Nd}";
const SPACE: &str = r"\p{White_Space}";
const NOT_SPACE: &str = r"\P{White_Space}";
/// ECMAScript syntax characters, plus `/`: the only punctuation `u` mode lets
/// an escape keep. Any other escaped punctuation is emitted as itself.
const SYNTAX: &str = r"^$\.*+?()[]{}|/";

#[derive(Clone, Copy)]
struct Flags {
    case_insensitive: bool,
    multi_line: bool,
    dot_all: bool,
}

#[derive(Clone, Copy)]
struct Group {
    contains_capture: bool,
}

fn fail<T>(offset: usize, reason: &'static str) -> Result<T, DialectError> {
    Err(DialectError { offset, reason })
}

pub fn translate(pattern: &str) -> Result<Translated, DialectError> {
    let (flags, body_start) = leading_flags(pattern)?;
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern[body_start..]
        .char_indices()
        .map(|(at, c)| (at + body_start, c))
        .peekable();
    let mut stack: Vec<Group> = Vec::new();
    // The group that closed immediately before the current character, which is
    // what a quantifier there would repeat.
    let mut closed: Option<Group> = None;

    while let Some((at, c)) = chars.next() {
        let just_closed = closed.take();
        match c {
            '\\' => {
                let (at, e) = chars.next().map_or(fail(at, "trailing backslash"), Ok)?;
                match e {
                    'd' => out.push_str(DIGIT),
                    'D' => out.push_str(NOT_DIGIT),
                    's' => out.push_str(SPACE),
                    'S' => out.push_str(NOT_SPACE),
                    'w' => {
                        out.push('[');
                        out.push_str(WORD);
                        out.push(']');
                    }
                    'W' => {
                        out.push_str("[^");
                        out.push_str(WORD);
                        out.push(']');
                    }
                    'b' => {
                        let w = format!("[{WORD}]");
                        out.push_str(&format!("(?:(?<={w})(?!{w})|(?<!{w})(?={w}))"));
                    }
                    'B' => {
                        let w = format!("[{WORD}]");
                        out.push_str(&format!("(?:(?<={w})(?={w})|(?<!{w})(?!{w}))"));
                    }
                    // No `m` flag is ever passed, so these are text anchors.
                    'A' => out.push('^'),
                    'z' => out.push('$'),
                    _ => common_escape(&mut out, &mut chars, at, e, false)?,
                }
            }
            '[' => class(&mut out, &mut chars, at)?,
            '.' => out.push_str(if flags.dot_all { "[^]" } else { r"[^\n]" }),
            '^' => out.push_str(if flags.multi_line { r"(?<![^\n])" } else { "^" }),
            '$' => out.push_str(if flags.multi_line { r"(?![^\n])" } else { "$" }),
            '(' => {
                let capture = open_group(&mut out, &mut chars, at)?;
                stack.push(Group {
                    contains_capture: capture,
                });
            }
            ')' => {
                let Some(group) = stack.pop() else {
                    return fail(at, "unopened group");
                };
                if let Some(parent) = stack.last_mut() {
                    parent.contains_capture |= group.contains_capture;
                }
                out.push(')');
                closed = Some(group);
            }
            '*' | '+' => {
                repeat(just_closed, at)?;
                out.push(c);
            }
            // Optional, or the lazy marker after another quantifier: neither
            // repeats a group more than once.
            '?' => out.push('?'),
            '{' => {
                let (text, many) = counted(&mut chars, at)?;
                if many {
                    repeat(just_closed, at)?;
                }
                out.push_str(&text);
            }
            // Literal in `regex`, a syntax error unescaped in `u` mode.
            ']' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    if !stack.is_empty() {
        return fail(pattern.len(), "unclosed group");
    }
    Ok(Translated {
        source: out,
        flags: if flags.case_insensitive { "iu" } else { "u" },
    })
}

/// A leading `(?flags)` group, which `regex` uses for flags ECMAScript spells
/// outside the pattern.
fn leading_flags(pattern: &str) -> Result<(Flags, usize), DialectError> {
    let mut flags = Flags {
        case_insensitive: false,
        multi_line: false,
        dot_all: false,
    };
    let Some(rest) = pattern.strip_prefix("(?") else {
        return Ok((flags, 0));
    };
    let Some(end) = rest.find(')') else {
        return Ok((flags, 0));
    };
    let group = &rest[..end];
    // `(?:`, `(?P<`, `(?i-m)` and friends are not a bare flag set, and are left
    // to the group parser, which refuses the ones it cannot translate.
    if group.is_empty() || !group.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok((flags, 0));
    }
    for (i, flag) in group.char_indices() {
        match flag {
            'i' => flags.case_insensitive = true,
            'm' => flags.multi_line = true,
            's' => flags.dot_all = true,
            'x' => return fail(2 + i, "the `x` flag has no ECMAScript equivalent"),
            'U' => return fail(2 + i, "the `U` flag has no ECMAScript equivalent"),
            'R' => return fail(2 + i, "the `R` flag has no ECMAScript equivalent"),
            _ => return fail(2 + i, "unknown inline flag"),
        }
    }
    Ok((flags, 2 + end + 1))
}

/// Escapes that mean the same inside and outside a class.
fn common_escape<I: Iterator<Item = (usize, char)>>(
    out: &mut String,
    chars: &mut std::iter::Peekable<I>,
    at: usize,
    e: char,
    in_class: bool,
) -> Result<(), DialectError> {
    match e {
        'n' | 'r' | 't' | 'f' | 'v' => {
            out.push('\\');
            out.push(e);
        }
        'x' => {
            if chars.peek().map(|&(_, c)| c) == Some('{') {
                chars.next();
                let mut hex = String::new();
                loop {
                    match chars.next() {
                        Some((_, '}')) => break,
                        Some((_, c)) if c.is_ascii_hexdigit() => hex.push(c),
                        _ => return fail(at, "malformed `\\x{…}` escape"),
                    }
                }
                if hex.is_empty() {
                    return fail(at, "malformed `\\x{…}` escape");
                }
                out.push_str(&format!("\\u{{{hex}}}"));
            } else {
                let mut hex = String::new();
                for _ in 0..2 {
                    match chars.next() {
                        Some((_, c)) if c.is_ascii_hexdigit() => hex.push(c),
                        _ => return fail(at, "malformed `\\x` escape"),
                    }
                }
                out.push_str(&format!("\\x{hex}"));
            }
        }
        'p' | 'P' => {
            out.push('\\');
            out.push(e);
            match chars.next() {
                Some((_, '{')) => {
                    out.push('{');
                    loop {
                        match chars.next() {
                            Some((_, '}')) => break,
                            Some((_, c)) if c.is_ascii_alphanumeric() || c == '_' || c == '=' => {
                                out.push(c)
                            }
                            _ => return fail(at, "a property name ECMAScript cannot spell"),
                        }
                    }
                    out.push('}');
                }
                Some((_, c)) if c.is_ascii_alphabetic() => {
                    out.push('{');
                    out.push(c);
                    out.push('}');
                }
                _ => return fail(at, "malformed property escape"),
            }
        }
        c if c.is_ascii_punctuation() => {
            // `-` keeps its escape only where it could otherwise form a range.
            if SYNTAX.contains(c) || (in_class && c == '-') {
                out.push('\\');
            }
            out.push(c);
        }
        ' ' => out.push(' '),
        '0'..='9' => return fail(at, "an octal escape or backreference"),
        _ => return fail(at, "an escape with no ECMAScript equivalent"),
    }
    Ok(())
}

fn class<I: Iterator<Item = (usize, char)>>(
    out: &mut String,
    chars: &mut std::iter::Peekable<I>,
    open: usize,
) -> Result<(), DialectError> {
    out.push('[');
    if chars.peek().map(|&(_, c)| c) == Some('^') {
        chars.next();
        out.push('^');
    }
    let mut first = true;
    loop {
        let Some((at, c)) = chars.next() else {
            return fail(open, "unclosed class");
        };
        let next = chars.peek().map(|&(_, c)| c);
        match c {
            // A `]` first in a class is a literal in `regex`.
            ']' if first => out.push_str(r"\]"),
            ']' => {
                out.push(']');
                return Ok(());
            }
            '[' => return fail(at, "a nested or POSIX class"),
            '&' if next == Some('&') => return fail(at, "class intersection"),
            '-' if next == Some('-') => return fail(at, "class difference"),
            '~' if next == Some('~') => return fail(at, "class symmetric difference"),
            '\\' => {
                let (at, e) = chars.next().map_or(fail(at, "trailing backslash"), Ok)?;
                match e {
                    'd' => out.push_str(DIGIT),
                    'D' => out.push_str(NOT_DIGIT),
                    's' => out.push_str(SPACE),
                    'S' => out.push_str(NOT_SPACE),
                    'w' => out.push_str(WORD),
                    'W' => return fail(at, "`\\W` inside a class has no single-class spelling"),
                    _ => common_escape(out, chars, at, e, true)?,
                }
            }
            _ => out.push(c),
        }
        first = false;
    }
}

/// Opens a group and reports whether it captures.
fn open_group<I: Iterator<Item = (usize, char)>>(
    out: &mut String,
    chars: &mut std::iter::Peekable<I>,
    at: usize,
) -> Result<bool, DialectError> {
    if chars.peek().map(|&(_, c)| c) != Some('?') {
        out.push('(');
        return Ok(true);
    }
    chars.next();
    match chars.next().map(|(_, c)| c) {
        Some(':') => {
            out.push_str("(?:");
            Ok(false)
        }
        Some('P') => match chars.next().map(|(_, c)| c) {
            Some('<') => named(out, chars, at),
            _ => fail(at, "malformed named group"),
        },
        Some('<') => match chars.peek().map(|&(_, c)| c) {
            Some('=' | '!') => fail(at, "lookbehind, which `regex` does not have"),
            _ => named(out, chars, at),
        },
        Some('=' | '!') => fail(at, "lookahead, which `regex` does not have"),
        _ => fail(
            at,
            "inline flags are only translated as the whole of a leading group",
        ),
    }
}

fn named<I: Iterator<Item = (usize, char)>>(
    out: &mut String,
    chars: &mut std::iter::Peekable<I>,
    at: usize,
) -> Result<bool, DialectError> {
    let mut name = String::new();
    loop {
        match chars.next() {
            Some((_, '>')) => break,
            Some((_, c)) if c.is_ascii_alphanumeric() || c == '_' => name.push(c),
            _ => return fail(at, "a group name ECMAScript cannot spell"),
        }
    }
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return fail(at, "a group name ECMAScript cannot spell");
    }
    out.push_str("(?<");
    out.push_str(&name);
    out.push('>');
    Ok(true)
}

/// A counted repetition, returned verbatim, and whether it can repeat more
/// than once.
fn counted<I: Iterator<Item = (usize, char)>>(
    chars: &mut std::iter::Peekable<I>,
    at: usize,
) -> Result<(String, bool), DialectError> {
    let number = |chars: &mut std::iter::Peekable<I>| {
        let mut digits = String::new();
        while let Some(&(_, c)) = chars.peek() {
            if !c.is_ascii_digit() {
                break;
            }
            digits.push(c);
            chars.next();
        }
        digits
    };
    let min = number(chars);
    if min.is_empty() {
        return fail(at, "not a counted repetition");
    }
    let (text, max) = match chars.next().map(|(_, c)| c) {
        Some('}') => (format!("{{{min}}}"), Some(min.clone())),
        Some(',') => {
            let max = number(chars);
            match chars.next().map(|(_, c)| c) {
                Some('}') if max.is_empty() => (format!("{{{min},}}"), None),
                Some('}') => (format!("{{{min},{max}}}"), Some(max)),
                _ => return fail(at, "not a counted repetition"),
            }
        }
        _ => return fail(at, "not a counted repetition"),
    };
    // Unbounded, or a bound too large to parse, can repeat more than once.
    let many = max.is_none_or(|m| m.parse::<u64>().map_or(true, |m| m > 1));
    Ok((text, many))
}

fn repeat(group: Option<Group>, at: usize) -> Result<(), DialectError> {
    match group {
        Some(g) if g.contains_capture => fail(
            at,
            "a capture inside a repeated group: ECMAScript resets it on each repetition and `regex` keeps the last value",
        ),
        _ => Ok(()),
    }
}
