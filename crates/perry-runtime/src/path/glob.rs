//! `path.matchesGlob`: minimatch 10.2.6 (bundled with Node 26) as Node's
//! `lib/internal/fs/glob.js` configures it (nonegate, nocomment, windowsPathsNoEscape,
//! optimizationLevel 2), matching case-sensitively. Node also sets `nocase` on macOS and
//! Windows hosts; Perry does not, so a wildcard segment matches as Node on Linux does
//! (`*.ts` does not match `README.TS`). Brace expansion follows the bundled
//! brace-expansion, limits included; a segment compiles to a regular expression the way
//! minimatch's AST does; the globstar walk is minimatch's. The web target's `matchGlob`
//! in `perry-codegen-wasm/src/wasm_runtime.js` is the same algorithm.
//!
//! Segment patterns are compiled by the caller (`compile`), so this module holds no
//! regular-expression engine and no heap value.

use std::collections::HashSet;

/// One pattern segment.
enum Part<P> {
    Literal(String),
    Pattern(P),
    Globstar,
}

/// Why `matches` stopped without an answer.
pub(crate) enum GlobError<E> {
    /// The caller's `compile` or `test` failed.
    Engine(E),
    /// minimatch's `assertValidPattern`: a pattern longer than 65536 UTF-16 units.
    PatternTooLong,
    /// A segment's regular expression grew past `MAX_SOURCE` (each negative extglob
    /// repeats its continuation; Node fails such patterns with a SyntaxError from V8).
    TooLarge,
}

const MAX_PATTERN_LENGTH: usize = 65536;
const MAX_SOURCE: usize = 1 << 20;
// brace-expansion's limits: at most 100000 expansions, 4000000 characters in all.
const EXPANSION_MAX: usize = 100_000;
const EXPANSION_MAX_LENGTH: usize = 4_000_000;
// Literal `{` / `}` and NUL around brace expansion: NUL is escaped first, so a marker
// can never be confused with the pattern's own text.
const ESC_OPEN: &str = "\0o";
const ESC_CLOSE: &str = "\0c";
const ESC_NUL: &str = "\0z";

fn unescape_braces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\0' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('o') => out.push('{'),
            Some('c') => out.push('}'),
            Some('z') => out.push('\0'),
            other => {
                out.push('\0');
                out.extend(other);
            }
        }
    }
    out
}

/// Whether `path` matches `pattern`. `compile` turns a segment's regular-expression
/// source (anchored; Unicode mode when its flag is set, UTF-16 code units otherwise, as
/// minimatch builds it) into a program and `test` runs it on a segment.
pub(crate) fn matches<P, E>(
    path: &str,
    pattern: &str,
    win32: bool,
    compile: &mut dyn FnMut(&str, bool) -> Result<P, E>,
    test: &mut dyn FnMut(&P, &str) -> Result<bool, E>,
) -> Result<bool, GlobError<E>> {
    if pattern.encode_utf16().count() > MAX_PATTERN_LENGTH {
        return Err(GlobError::PatternTooLong);
    }
    if pattern.is_empty() {
        return Ok(path.is_empty());
    }
    // minimatch folds the split path in place while matching the first pattern, so a
    // later pattern sees (and aligns its drive root against) the folded path.
    let mut file = split(
        &if win32 {
            path.replace('\\', "/")
        } else {
            path.to_string()
        },
        win32,
    );
    let slashed = pattern.replace('\\', "/");
    let expanded: Vec<String> = if has_brace_group(&slashed) {
        let escaped = slashed.replace('\0', ESC_NUL);
        let source = match escaped.strip_prefix("{}") {
            Some(rest) => format!("{ESC_OPEN}{ESC_CLOSE}{rest}"),
            None => escaped,
        };
        expand_braces(&source, true)
            .iter()
            .map(|s| unescape_braces(s))
            .collect()
    } else {
        vec![slashed]
    };
    let mut seen = HashSet::new();
    let unique: Vec<String> = expanded
        .into_iter()
        .filter(|e| seen.insert(e.clone()))
        .collect();
    let patterns = preprocess(unique.iter().map(|p| split(p, win32)).collect(), win32);
    for parts in patterns {
        let mut compiled = Vec::with_capacity(parts.len());
        for s in &parts {
            compiled.push(if s == "**" {
                Part::Globstar
            } else {
                let segment: Vec<char> = s.chars().collect();
                let source = glob_source(&segment, true, "", 0, None).ok_or(GlobError::TooLarge)?;
                match source.literal {
                    Some(literal) => Part::Literal(literal),
                    None => Part::Pattern(
                        compile(&format!("^{}$", source.src), source.unicode)
                            .map_err(GlobError::Engine)?,
                    ),
                }
            });
        }
        let (file_start, pattern_start) = if win32 {
            align_win32_roots(&file, &parts, &mut compiled)
        } else {
            (0, 0)
        };
        fold_segments(&mut file, win32, false);
        let rest = &file[file_start.min(file.len())..];
        if match_parts(rest, &compiled[pattern_start..], test).map_err(GlobError::Engine)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// minimatch's win32 roots: `//?/C:` is the drive `C:`, and a drive matches its other
/// spelling without case. Answers where matching starts in the file and the pattern.
fn align_win32_roots<P>(
    raw: &[String],
    parts: &[String],
    compiled: &mut [Part<P>],
) -> (usize, usize) {
    fn literal<P>(p: &[Part<P>], i: usize) -> Option<&str> {
        match p.get(i) {
            Some(Part::Literal(l)) => Some(l),
            _ => None,
        }
    }
    if parts.len() > 3
        && parts[0].is_empty()
        && parts[1].is_empty()
        && parts[2] == "?"
        && literal(compiled, 3).is_some_and(is_drive)
    {
        compiled[2] = Part::Literal("?".into());
    }
    let file_unc = raw.len() > 3
        && raw[0].is_empty()
        && raw[1].is_empty()
        && raw[2] == "?"
        && is_drive(&raw[3]);
    let fdi = if is_drive(&raw[0]) {
        Some(0)
    } else if file_unc {
        Some(3)
    } else {
        None
    };
    let pattern_unc = literal(compiled, 0) == Some("")
        && literal(compiled, 1) == Some("")
        && literal(compiled, 2) == Some("?")
        && literal(compiled, 3).is_some_and(is_drive);
    let pdi = if literal(compiled, 0).is_some_and(is_drive) {
        Some(0)
    } else if pattern_unc {
        Some(3)
    } else {
        None
    };
    if let (Some(fdi), Some(pdi)) = (fdi, pdi) {
        if literal(compiled, pdi).is_some_and(|l| l.eq_ignore_ascii_case(&raw[fdi])) {
            compiled[pdi] = Part::Literal(raw[fdi].clone());
            return (fdi, pdi);
        }
    }
    (0, 0)
}

fn is_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

fn split(p: &str, win32: bool) -> Vec<String> {
    // `split(/\/+/)`: a run of slashes is one separator.
    let mut out = vec![String::new()];
    let mut prev_slash = false;
    for c in p.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(String::new());
            }
            prev_slash = true;
        } else {
            out.last_mut().unwrap().push(c);
            prev_slash = false;
        }
    }
    if win32 && p.starts_with("//") && p[2..].chars().next().is_some_and(|c| c != '/') {
        out.insert(0, String::new());
    }
    out
}

/// `/\{(?:(?!\{).)*\}/`: a `{` followed, before any other `{` or newline, by `}`.
fn has_brace_group(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    (0..chars.len()).any(|i| {
        chars[i] == '{'
            && chars[i + 1..]
                .iter()
                .take_while(|&&c| c != '{' && !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
                .any(|&c| c == '}')
    })
}

struct Balanced<'a> {
    pre: &'a str,
    body: &'a str,
    post: &'a str,
}

/// balanced-match's `range('{', '}', str)`.
fn balanced_braces(s: &str) -> Option<Balanced<'_>> {
    let find = |c: char, from: usize| s.get(from..).and_then(|t| t.find(c)).map(|i| i + from);
    let mut ai = Some(s.find('{')?);
    let mut bi = Some(find('}', ai? + 1)?);
    let mut begs: Vec<usize> = Vec::new();
    let (mut left, mut right) = (s.len(), None);
    let mut result = None;
    let mut i = ai;
    while let (Some(at), None) = (i, result) {
        if Some(at) == ai {
            begs.push(at);
            ai = find('{', at + 1);
        } else if begs.len() == 1 {
            result = begs.pop().zip(bi);
        } else {
            if let Some(beg) = begs.pop() {
                if beg < left {
                    left = beg;
                    right = bi;
                }
            }
            bi = find('}', at + 1);
        }
        i = match (ai, bi) {
            (Some(a), Some(b)) if a < b => Some(a),
            (_, b) => b,
        };
    }
    if !begs.is_empty() {
        if let Some(r) = right {
            result = Some((left, r));
        }
    }
    result.map(|(a, b)| Balanced {
        pre: &s[..a],
        body: &s[a + 1..b],
        post: &s[b + 1..],
    })
}

fn comma_parts(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    let Some(m) = balanced_braces(s) else {
        return s.split(',').map(str::to_string).collect();
    };
    let mut p: Vec<String> = m.pre.split(',').map(str::to_string).collect();
    p.last_mut().unwrap().push_str(&format!("{{{}}}", m.body));
    let mut post = comma_parts(m.post);
    if !m.post.is_empty() {
        let first = post.remove(0);
        p.last_mut().unwrap().push_str(&first);
        p.extend(post);
    }
    p
}

/// brace-expansion's sequence `x..y[..step]`, computed in JavaScript numbers as it is.
fn brace_sequence(body: &str, alpha: bool) -> Vec<String> {
    let n: Vec<&str> = body.split("..").collect();
    let num = |s: &str| -> f64 {
        if alpha && !s.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            f64::from(s.encode_utf16().next().unwrap_or(0))
        } else {
            s.parse::<f64>().unwrap_or(0.0)
        }
    };
    let (x, y) = (num(n[0]), num(n[1]));
    let width = n[0].len().max(n[1].len());
    let mut step = if n.len() == 3 {
        num(n[2]).abs().max(1.0)
    } else {
        1.0
    };
    if y < x {
        step = -step;
    }
    let padded = |s: &str| {
        let t = s.strip_prefix('-').unwrap_or(s);
        t.len() >= 2 && t.starts_with('0') && t.as_bytes()[1].is_ascii_digit()
    };
    let pad = n.iter().any(|s| padded(s));
    let mut out = Vec::new();
    let mut length = 0;
    let mut i = x;
    // A step too small to change `i` repeats it until the count limit, as in Node.
    while (if step > 0.0 { i <= y } else { i >= y }) && out.len() < EXPANSION_MAX {
        let c = if alpha {
            match char::from_u32(u32::from(i as u16)) {
                Some('\\') | None => String::new(),
                Some(c) => c.to_string(),
            }
        } else {
            let mut c = js_integer_string(i);
            if pad && width > c.len() {
                let z = "0".repeat(width - c.len());
                c = if i < 0.0 {
                    format!("-{z}{}", &c[1..])
                } else {
                    format!("{z}{c}")
                };
            }
            c
        };
        if length + c.len() > EXPANSION_MAX_LENGTH {
            break;
        }
        length += c.len();
        out.push(c);
        i += step;
    }
    out
}

/// `String(v)` for an integral JavaScript number.
fn js_integer_string(v: f64) -> String {
    if v == 0.0 {
        "0".into()
    } else if v.abs() < 1e21 {
        format!("{v:.0}")
    } else {
        let e = format!("{v:e}");
        match e.split_once('e') {
            Some((mantissa, exp)) if !exp.starts_with('-') => format!("{mantissa}e+{exp}"),
            _ => e,
        }
    }
}

fn combine(acc: &[String], pre: &str, values: &[String], drop_empties: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut length = 0;
    for a in acc {
        for v in values {
            if out.len() >= EXPANSION_MAX {
                return out;
            }
            let expansion = format!("{a}{pre}{v}");
            if drop_empties && expansion.is_empty() {
                continue;
            }
            if length + expansion.len() > EXPANSION_MAX_LENGTH {
                return out;
            }
            length += expansion.len();
            out.push(expansion);
        }
    }
    out
}

fn is_numeric_sequence(body: &str) -> bool {
    let int = |s: &str| {
        let t = s.strip_prefix('-').unwrap_or(s);
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
    };
    let n: Vec<&str> = body.split("..").collect();
    (n.len() == 2 || n.len() == 3) && n.iter().all(|s| int(s))
}

fn is_alpha_sequence(body: &str) -> bool {
    let n: Vec<&str> = body.split("..").collect();
    let letter = |s: &str| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic();
    let int = |s: &str| {
        let t = s.strip_prefix('-').unwrap_or(s);
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
    };
    (n.len() == 2 || n.len() == 3) && letter(n[0]) && letter(n[1]) && (n.len() == 2 || int(n[2]))
}

/// brace-expansion's `expand_`.
fn expand_braces(input: &str, mut is_top: bool) -> Vec<String> {
    let mut s = input.to_string();
    let mut acc = vec![String::new()];
    let (mut drop_empties, mut first_group) = (false, true);
    loop {
        let Some(m) = balanced_braces(&s) else {
            return combine(&acc, &s, &[String::new()], drop_empties);
        };
        let (pre, body, post) = (m.pre.to_string(), m.body.to_string(), m.post.to_string());
        if pre.ends_with('$') {
            acc = combine(
                &acc,
                &format!("{pre}{{{body}}}"),
                &[String::new()],
                drop_empties && post.is_empty(),
            );
            first_group = false;
            if post.is_empty() {
                break;
            }
            s = post;
            continue;
        }
        let (numeric, alpha) = (is_numeric_sequence(&body), is_alpha_sequence(&body));
        if !numeric && !alpha && !body.contains(',') {
            if post_has_later_comma_group(&post) {
                s = format!("{pre}{{{body}{ESC_CLOSE}{post}");
                is_top = true;
                continue;
            }
            return combine(
                &acc,
                &format!("{pre}{{{body}}}{post}"),
                &[String::new()],
                drop_empties,
            );
        }
        if first_group {
            drop_empties = is_top && !numeric && !alpha;
            first_group = false;
        }
        let values: Vec<String> = if numeric || alpha {
            brace_sequence(&body, alpha)
        } else {
            let mut n = comma_parts(&body);
            if n.len() == 1 {
                n = expand_braces(&n[0], false)
                    .into_iter()
                    .map(|s| format!("{{{s}}}"))
                    .collect();
                if n.len() == 1 {
                    acc = combine(
                        &acc,
                        &format!("{pre}{}", n[0]),
                        &[String::new()],
                        drop_empties && post.is_empty(),
                    );
                    if post.is_empty() {
                        break;
                    }
                    s = post;
                    continue;
                }
            }
            let drops = drop_empties
                && post.is_empty()
                && pre.is_empty()
                && !acc.iter().any(|a| !a.is_empty());
            let mut values = Vec::new();
            let mut length = 0;
            'outer: for part in &n {
                for v in expand_braces(part, false) {
                    if drops && v.is_empty() {
                        continue;
                    }
                    if values.len() >= EXPANSION_MAX || length + v.len() > EXPANSION_MAX_LENGTH {
                        break 'outer;
                    }
                    length += v.len();
                    values.push(v);
                }
            }
            values
        };
        acc = combine(&acc, &pre, &values, drop_empties && post.is_empty());
        if post.is_empty() {
            break;
        }
        s = post;
    }
    acc
}

/// `/,(?!,).*\}/`: a comma not followed by a comma, and a `}` later on the line.
fn post_has_later_comma_group(post: &str) -> bool {
    let chars: Vec<char> = post.chars().collect();
    (0..chars.len()).any(|i| {
        chars[i] == ','
            && chars.get(i + 1) != Some(&',')
            && chars[i + 1..]
                .iter()
                .take_while(|&&c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
                .any(|&c| c == '}')
    })
}

/// A regular-expression literal for `c`, valid with or without Unicode mode.
fn push_escaped(out: &mut String, c: char) {
    if "^$\\.*+?()[]{}|/".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

fn is_extglob(segment: &[char], i: usize) -> bool {
    segment.get(i).is_some_and(|c| "@+*?!".contains(*c)) && segment.get(i + 1) == Some(&'(')
}

/// The depth at which minimatch parses an extglob of type `c` met at `depth` inside one
/// of type `outer` (`None` at the segment level), or `None` when it is left literal:
/// three levels deep, and deeper only where the outer type adopts the inner one (its
/// adoptionAnyMap), which costs no level.
fn extglob_depth(outer: Option<char>, depth: usize, c: char) -> Option<usize> {
    let adopts = match outer {
        Some('!' | '?' | '@') => "?@".contains(c),
        Some('*') => "*+?@".contains(c),
        Some('+') => "+@?*".contains(c),
        _ => false,
    };
    if adopts {
        Some(depth)
    } else if depth <= 2 {
        Some(depth + 1)
    } else {
        None
    }
}

struct Group<'a> {
    arms: Vec<&'a [char]>,
    close: usize,
}

/// The arms of the extglob of type `kind` whose `(` is at `open`, as minimatch's parser
/// finds them: a `|` or `)` inside a `[...]` class or a nested extglob does not end an
/// arm, and a bare `(` is a literal. `None` when the group is not closed.
fn extglob_arms(segment: &[char], open: usize, depth: usize, kind: char) -> Option<Group<'_>> {
    let mut arms = Vec::new();
    let (mut start, mut in_class, mut class_start, mut class_neg) = (open + 1, false, 0, false);
    let mut i = open + 1;
    while i < segment.len() {
        let c = segment[i];
        i += 1;
        if in_class {
            if i == class_start + 1 {
                if c == '^' || c == '!' {
                    class_neg = true;
                }
            } else if c == ']' && !(i == class_start + 2 && class_neg) {
                in_class = false;
            }
            continue;
        }
        if c == '[' {
            in_class = true;
            class_start = i;
            class_neg = false;
        } else if is_extglob(segment, i - 1) && extglob_depth(Some(kind), depth, c).is_some() {
            let inner = extglob_depth(Some(kind), depth, c).unwrap();
            i = extglob_arms(segment, i, inner, c)?.close + 1;
        } else if c == '|' || c == ')' {
            arms.push(&segment[start..i - 1]);
            start = i;
            if c == ')' {
                return Some(Group { arms, close: i - 1 });
            }
        }
    }
    None
}

struct Source {
    src: String,
    literal: Option<String>,
    unicode: bool,
}

/// A segment pattern. `tail` is the source of what follows this pattern up to the end of
/// the segment, which a negative extglob inside it must also exclude (minimatch's
/// fillNegs). `None` when the source would exceed `MAX_SOURCE`.
fn glob_source(
    segment: &[char],
    at_start: bool,
    tail: &str,
    depth: usize,
    outer: Option<char>,
) -> Option<Source> {
    let (mut src, mut literal) = (String::new(), String::new());
    let (mut magic, mut unicode, mut in_open_class) = (false, false, false);
    let no_empty = at_start && !segment.is_empty() && segment.iter().all(|&c| c == '*');
    let mut i = 0;
    while i < segment.len() {
        if src.len() > MAX_SOURCE {
            return None;
        }
        let c = segment[i];
        let inner = if !in_open_class && is_extglob(segment, i) {
            extglob_depth(outer, depth, c)
        } else {
            None
        };
        let group = inner.and_then(|d| extglob_arms(segment, i + 1, d, c));
        // A whole-segment extglob with an empty body is the literal text (`@()`).
        let empty_whole = group.as_ref().is_some_and(|g| {
            outer.is_none()
                && i == 0
                && g.close == segment.len() - 1
                && c != '!'
                && g.arms.iter().all(|a| a.is_empty())
        });
        if let (Some(g), false) = (&group, empty_whole) {
            let rest = &segment[g.close + 1..];
            // Only a negative extglob reads the continuation.
            let needs_tail =
                c == '!' || g.arms.iter().any(|a| a.windows(2).any(|w| w == ['!', '(']));
            let rest_tail = if needs_tail {
                format!(
                    "{}{tail}",
                    glob_source(rest, false, tail, depth, outer)?.src
                )
            } else {
                String::new()
            };
            let guarded = at_start && i == 0;
            let arm_tail = if c == '!' { "" } else { rest_tail.as_str() };
            let mut parts = Vec::with_capacity(g.arms.len());
            for arm in &g.arms {
                let part = glob_source(arm, false, arm_tail, inner?, Some(c))?;
                unicode |= part.unicode;
                parts.push(part.src);
            }
            let open = parts.join("|");
            let first = if guarded {
                parts
                    .iter()
                    .map(|p| format!("{}{p}", start_guard(p)))
                    .collect::<Vec<_>>()
                    .join("|")
            } else {
                open.clone()
            };
            if c == '!' {
                let guard = if guarded { "(?!\\.)" } else { "" };
                src.push_str(&format!("(?:(?!(?:{open}){rest_tail}$){guard}[^/]*?)"));
            } else if (c == '+' || c == '*') && first != open {
                let opt = if c == '*' { "?" } else { "" };
                src.push_str(&format!("(?:(?:{first})(?:{open})*?){opt}"));
            } else {
                let q = if c == '@' {
                    String::new()
                } else {
                    c.to_string()
                };
                src.push_str(&format!("(?:{first}){q}"));
            }
            magic = true;
            i = g.close + 1;
            continue;
        }
        match c {
            '*' => {
                while segment.get(i + 1) == Some(&'*') {
                    i += 1;
                }
                src.push_str(if no_empty { "[^/]+?" } else { "[^/]*?" });
                magic = true;
            }
            '?' => {
                src.push_str("[^/]");
                magic = true;
            }
            '[' => match glob_class(segment, i) {
                Some((class_src, class_literal, consumed, needs_unicode)) => {
                    src.push_str(&class_src);
                    match class_literal {
                        Some(l) => literal.push(l),
                        None => magic = true,
                    }
                    unicode |= needs_unicode;
                    i += consumed;
                    continue;
                }
                None => {
                    // An unclosed class: a literal `[`, and no extglob after it here.
                    push_escaped(&mut src, c);
                    literal.push(c);
                    in_open_class = true;
                }
            },
            _ => {
                push_escaped(&mut src, c);
                literal.push(c);
            }
        }
        i += 1;
    }
    let dots = segment == ['.'] || segment == ['.', '.'];
    if at_start && !is_extglob(segment, 0) && !dots {
        src = format!("{}{src}", start_guard(&src));
    }
    if src.len() > MAX_SOURCE {
        return None;
    }
    Some(Source {
        src,
        literal: (!magic).then_some(literal),
        unicode,
    })
}

/// minimatch's POSIX classes: (name, class body, negated, needs Unicode mode).
const POSIX_CLASSES: &[(&str, &str, bool, bool)] = &[
    ("[:alnum:]", "\\p{L}\\p{Nl}\\p{Nd}", false, true),
    ("[:alpha:]", "\\p{L}\\p{Nl}", false, true),
    ("[:ascii:]", "\\x00-\\x7f", false, false),
    ("[:blank:]", "\\p{Zs}\\t", false, true),
    ("[:cntrl:]", "\\p{Cc}", false, true),
    ("[:digit:]", "\\p{Nd}", false, true),
    ("[:graph:]", "\\p{Z}\\p{C}", true, true),
    ("[:lower:]", "\\p{Ll}", false, true),
    ("[:print:]", "\\p{C}", false, true),
    ("[:punct:]", "\\p{P}", false, true),
    ("[:space:]", "\\p{Z}\\t\\r\\n\\v\\f", false, true),
    ("[:upper:]", "\\p{Lu}", false, true),
    ("[:word:]", "\\p{L}\\p{Nl}\\p{Nd}\\p{Pc}", false, true),
    ("[:xdigit:]", "A-Fa-f0-9", false, false),
];

/// A `[...]` class at `pos`: (source, literal when it is one plain character, chars
/// consumed, needs Unicode mode). `None` when the class is not closed.
fn glob_class(glob: &[char], pos: usize) -> Option<(String, Option<char>, usize, bool)> {
    let esc = |c: char| -> String {
        if matches!(c, '[' | ']' | '\\' | '-') {
            format!("\\{c}")
        } else {
            c.to_string()
        }
    };
    let (mut ranges, mut negs): (Vec<String>, Vec<&str>) = (Vec::new(), Vec::new());
    let (mut i, mut saw_start, mut negate, mut end_pos) = (pos + 1, false, false, pos);
    let mut range_start: Option<char> = None;
    let mut unicode = false;
    'scan: while i < glob.len() {
        let c = glob[i];
        if (c == '!' || c == '^') && i == pos + 1 {
            negate = true;
            i += 1;
            continue;
        }
        if c == ']' && saw_start {
            end_pos = i + 1;
            break;
        }
        saw_start = true;
        if c == '[' {
            for &(name, source, negated, needs_unicode) in POSIX_CLASSES {
                let name: Vec<char> = name.chars().collect();
                if glob[i..].starts_with(&name) {
                    if range_start.is_some() {
                        return Some(("$.".into(), None, glob.len() - pos, false));
                    }
                    i += name.len();
                    unicode |= needs_unicode;
                    if negated {
                        negs.push(source);
                    } else {
                        ranges.push(source.to_string());
                    }
                    continue 'scan;
                }
            }
        }
        if let Some(start) = range_start.take() {
            if c > start {
                ranges.push(format!("{}-{}", esc(start), esc(c)));
            } else if c == start {
                ranges.push(esc(c));
            }
            i += 1;
        } else if glob.get(i + 1) == Some(&'-') && glob.get(i + 2) == Some(&']') {
            ranges.push(format!("{}\\-", esc(c)));
            i += 2;
        } else if glob.get(i + 1) == Some(&'-') {
            range_start = Some(c);
            i += 2;
        } else {
            ranges.push(esc(c));
            i += 1;
        }
    }
    if end_pos < i {
        return None;
    }
    if ranges.is_empty() && negs.is_empty() {
        return Some(("$.".into(), None, glob.len() - pos, false));
    }
    if negs.is_empty() && ranges.len() == 1 && !negate {
        let r = &ranges[0];
        let plain = r.chars().count() == 1 || (r.chars().count() == 2 && r.starts_with('\\'));
        if plain {
            let c = r.chars().last().unwrap();
            let mut source = String::new();
            push_escaped(&mut source, c);
            if c == '-' {
                source = "-".into();
            }
            return Some((source, Some(c), end_pos - pos, false));
        }
    }
    let sranges = format!("[{}{}]", if negate { "^" } else { "" }, ranges.concat());
    let snegs = format!("[{}{}]", if negate { "" } else { "^" }, negs.concat());
    let source = if !ranges.is_empty() && !negs.is_empty() {
        format!("({sranges}|{snegs})")
    } else if !ranges.is_empty() {
        sranges
    } else {
        snegs
    };
    Some((source, None, end_pos - pos, unicode))
}

/// minimatch's guard at the start of a segment: a wildcard does not match a leading
/// `.`, and a pattern starting with `.` and a wildcard does not match `.` or `..`.
fn start_guard(src: &str) -> &'static str {
    let b = src.as_bytes();
    let wild = |i: usize| b.get(i).is_some_and(|&c| c == b'[' || c == b'.');
    if (src.starts_with("\\.") && wild(2)) || (src.starts_with("\\.\\.") && wild(4)) {
        "(?!\\.\\.?$)"
    } else if wild(0) {
        "(?!\\.)"
    } else {
        ""
    }
}

/// minimatch's optimizationLevel 2 on a split path or pattern, in place: drop `.` and
/// empty inner segments and fold `x/..`. Answers whether anything changed.
fn fold_segments(parts: &mut Vec<String>, win32: bool, is_pattern: bool) -> bool {
    let mut changed = false;
    loop {
        let mut again = false;
        let mut i = 1;
        while i + 1 < parts.len() {
            if i == 1 && parts[1].is_empty() && parts[0].is_empty() {
                i += 1;
                continue;
            }
            if parts[i] == "." || parts[i].is_empty() {
                parts.remove(i);
                again = true;
            } else {
                i += 1;
            }
        }
        if parts.len() == 2 && parts[0] == "." && (parts[1] == "." || parts[1].is_empty()) {
            parts.pop();
            again = true;
        }
        for dd in 1..parts.len() {
            let p = &parts[dd - 1];
            if parts[dd] == ".."
                && !p.is_empty()
                && p != "."
                && p != ".."
                && p != "**"
                && (is_pattern || !(win32 && is_drive(p)))
            {
                let keep_dot =
                    is_pattern && dd == 1 && parts.get(dd + 1).is_some_and(|n| n == "**");
                parts.drain(dd - 1..=dd);
                if keep_dot {
                    parts.insert(dd - 1, ".".into());
                }
                again = true;
                break;
            }
        }
        changed |= again;
        if !again {
            break;
        }
    }
    if parts.is_empty() {
        parts.push(String::new());
    }
    changed
}

/// minimatch's first preprocessing phase: collapse `**/**`, and split `**/../a/b`
/// into `../a/b` and `**/a/b`.
fn preprocess(mut patterns: Vec<Vec<String>>, win32: bool) -> Vec<Vec<String>> {
    let named = |s: Option<&String>| s.is_some_and(|s| !s.is_empty() && s != "." && s != "..");
    let mut changed = true;
    while changed {
        changed = false;
        let mut k = 0;
        while k < patterns.len() {
            let mut gs = 0usize;
            while let Some(found) = patterns[k][gs..]
                .iter()
                .position(|s| s == "**")
                .map(|p| p + gs)
            {
                let parts = &mut patterns[k];
                let mut gss = found;
                while parts.get(gss + 1).is_some_and(|s| s == "**") {
                    gss += 1;
                }
                if gss > found {
                    parts.drain(found + 1..=gss);
                }
                if parts.get(found + 1).is_some_and(|s| s == "..")
                    && named(parts.get(found + 2))
                    && named(parts.get(found + 3))
                {
                    changed = true;
                    parts.remove(found);
                    let mut other = parts.clone();
                    other[found] = "**".into();
                    patterns.push(other);
                    gs = found;
                } else {
                    gs = found + 1;
                }
            }
            changed = fold_segments(&mut patterns[k], win32, true) || changed;
            k += 1;
        }
    }
    patterns
}

fn hidden(segment: Option<&String>) -> bool {
    segment.is_some_and(|s| s.starts_with('.'))
}

fn match_segments<P, E>(
    file: &[String],
    pattern: &[Part<P>],
    mut fi: usize,
    test: &mut dyn FnMut(&P, &str) -> Result<bool, E>,
) -> Result<bool, E> {
    let mut pi = 0;
    while fi < file.len() && pi < pattern.len() {
        let hit = match &pattern[pi] {
            Part::Literal(l) => file[fi] == *l,
            Part::Pattern(p) => test(p, &file[fi])?,
            Part::Globstar => false,
        };
        if !hit {
            return Ok(false);
        }
        fi += 1;
        pi += 1;
    }
    if pi < pattern.len() {
        return Ok(false);
    }
    Ok(fi == file.len() || (fi + 1 == file.len() && file[fi].is_empty()))
}

/// minimatch's globstar walk: a head before the first `**`, a tail after the last, and
/// the sections between them placed left to right.
fn match_parts<P, E>(
    file: &[String],
    pattern: &[Part<P>],
    test: &mut dyn FnMut(&P, &str) -> Result<bool, E>,
) -> Result<bool, E> {
    let globstar = |p: &Part<P>| matches!(p, Part::Globstar);
    let Some(first) = pattern.iter().position(globstar) else {
        return match_segments(file, pattern, 0, test);
    };
    let last = pattern.iter().rposition(globstar).unwrap();
    let (head, tail) = (&pattern[..first], &pattern[last + 1..]);
    let fi = head.len();
    if fi > 0 && !match_segments(&file[..fi.min(file.len())], head, 0, test)? {
        return Ok(false);
    }
    let mut tail_match = 0;
    if !tail.is_empty() {
        if tail.len() + fi > file.len() {
            return Ok(false);
        }
        if match_segments(file, tail, file.len() - tail.len(), test)? {
            tail_match = tail.len();
        } else {
            if !file.last().is_some_and(|s| s.is_empty())
                || fi + tail.len() == file.len()
                || !match_segments(file, tail, file.len() - tail.len() - 1, test)?
            {
                return Ok(false);
            }
            tail_match = tail.len() + 1;
        }
    }
    let end = file.len() - tail_match;
    if first == last {
        if (fi..end).any(|i| hidden(file.get(i))) {
            return Ok(false);
        }
        return Ok(tail_match > 0 || fi < end);
    }
    let mut sections: Vec<&[Part<P>]> = Vec::new();
    let mut start = first + 1;
    for i in first + 1..=last {
        if globstar(&pattern[i]) {
            sections.push(&pattern[start..i]);
            start = i + 1;
        }
    }
    let total: usize = sections.iter().map(|s| s.len()).sum();
    let mut after = end as isize - total as isize;
    let bounds: Vec<isize> = sections
        .iter()
        .map(|s| {
            after += s.len() as isize;
            after - s.len() as isize
        })
        .collect();
    Ok(match_sections(file, &sections, &bounds, fi, 0, tail_match > 0, test)? == Some(true))
}

/// `Some(answer)` when the walk is decided, `None` when this placement cannot match.
fn match_sections<P, E>(
    file: &[String],
    sections: &[&[Part<P>]],
    bounds: &[isize],
    mut fi: usize,
    si: usize,
    mut saw_tail: bool,
    test: &mut dyn FnMut(&P, &str) -> Result<bool, E>,
) -> Result<Option<bool>, E> {
    let Some(body) = sections.get(si) else {
        while fi < file.len() {
            saw_tail = true;
            if hidden(file.get(fi)) {
                return Ok(Some(false));
            }
            fi += 1;
        }
        return Ok(Some(saw_tail));
    };
    while fi as isize <= bounds[si] {
        let upto = (fi + body.len()).min(file.len());
        if match_segments(&file[..upto], body, fi, test)? && si < 200 {
            match match_sections(
                file,
                sections,
                bounds,
                fi + body.len(),
                si + 1,
                saw_tail,
                test,
            )? {
                Some(false) => {}
                decided => return Ok(decided),
            }
        }
        if hidden(file.get(fi)) {
            return Ok(Some(false));
        }
        fi += 1;
    }
    Ok(None)
}
