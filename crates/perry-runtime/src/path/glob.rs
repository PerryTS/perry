//! `path.matchesGlob`: minimatch 10 as Node's `lib/internal/fs/glob.js` configures it
//! (nonegate, nocomment, windowsPathsNoEscape, optimizationLevel 2), case-sensitive.
//! Brace expansion follows brace-expansion; a segment compiles to a regular expression
//! the way minimatch's AST does; the globstar walk is minimatch's. The web target's
//! `matchGlob` in `perry-codegen-wasm/src/wasm_runtime.js` is the same algorithm.
//!
//! Segment patterns are compiled by the caller (`compile`), so this module holds no
//! regular-expression engine and no heap value.

/// One pattern segment.
enum Part<P> {
    Literal(String),
    Pattern(P),
    Globstar,
}

const ESC_OPEN: &str = "\0o";
const ESC_CLOSE: &str = "\0c";

/// Whether `path` matches `pattern`. `compile` turns a segment's regular-expression
/// source (anchored, Unicode mode) into a program and `test` runs it on a segment.
pub(crate) fn matches<P, E>(
    path: &str,
    pattern: &str,
    win32: bool,
    compile: &mut dyn FnMut(&str) -> Result<P, E>,
    test: &mut dyn FnMut(&P, &str) -> Result<bool, E>,
) -> Result<bool, E> {
    if pattern.is_empty() {
        return Ok(path.is_empty());
    }
    let raw = split(
        &if win32 {
            path.replace('\\', "/")
        } else {
            path.to_string()
        },
        win32,
    );
    let mut file = raw.clone();
    fold_segments(&mut file, win32, false);
    let slashed = pattern.replace('\\', "/");
    let expanded = if has_brace_group(&slashed) {
        let source = match slashed.strip_prefix("{}") {
            Some(rest) => format!("{ESC_OPEN}{ESC_CLOSE}{rest}"),
            None => slashed.clone(),
        };
        expand_braces(&source, true)
            .into_iter()
            .map(|s| s.replace(ESC_OPEN, "{").replace(ESC_CLOSE, "}"))
            .collect()
    } else {
        vec![slashed]
    };
    let mut unique: Vec<String> = Vec::new();
    for e in expanded {
        if !unique.contains(&e) {
            unique.push(e);
        }
    }
    let patterns = preprocess(unique.iter().map(|p| split(p, win32)).collect(), win32);
    for parts in patterns {
        let mut compiled = Vec::with_capacity(parts.len());
        for s in &parts {
            compiled.push(if s == "**" {
                Part::Globstar
            } else {
                let segment: Vec<char> = s.chars().collect();
                let source = glob_source(&segment, true);
                match source.literal {
                    Some(literal) => Part::Literal(literal),
                    None => Part::Pattern(compile(&format!("^{}$", source.src))?),
                }
            });
        }
        if let (Some(fd), Some(Part::Literal(pd))) = (raw.first(), compiled.first_mut()) {
            if win32 && is_drive(fd) && is_drive(pd) && pd.eq_ignore_ascii_case(fd) {
                *pd = fd.clone();
            }
        }
        if match_parts(&file, &compiled, test)? {
            return Ok(true);
        }
    }
    Ok(false)
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

fn brace_sequence(body: &str, alpha: bool) -> Vec<String> {
    let n: Vec<&str> = body.split("..").collect();
    let num = |s: &str| -> i64 {
        s.parse::<i64>()
            .unwrap_or_else(|_| s.encode_utf16().next().map_or(0, i64::from))
    };
    let (x, y) = (num(n[0]), num(n[1]));
    let width = n[0].chars().count().max(n[1].chars().count());
    let mut step = if n.len() == 3 {
        num(n[2]).abs().max(1)
    } else {
        1
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
    let mut i = x;
    while if step > 0 { i <= y } else { i >= y } {
        let c = if alpha {
            match char::from_u32(i as u32) {
                Some('\\') | None => String::new(),
                Some(c) => c.to_string(),
            }
        } else {
            let mut c = i.to_string();
            if pad && width > c.len() {
                let z = "0".repeat(width - c.len());
                c = if i < 0 {
                    format!("-{z}{}", &c[1..])
                } else {
                    format!("{z}{c}")
                };
            }
            c
        };
        out.push(c);
        if out.len() >= 100_000 {
            break;
        }
        i += step;
    }
    out
}

fn combine(acc: &[String], pre: &str, values: &[String], drop_empties: bool) -> Vec<String> {
    let mut out = Vec::new();
    for a in acc {
        for v in values {
            let expansion = format!("{a}{pre}{v}");
            if !drop_empties || !expansion.is_empty() {
                out.push(expansion);
            }
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
            n.iter()
                .flat_map(|part| expand_braces(part, false))
                .filter(|v| !drops || !v.is_empty())
                .collect()
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

/// A regular-expression literal for `c` that is valid in Unicode mode.
fn push_escaped(out: &mut String, c: char) {
    if "^$\\.*+?()[]{}|/".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

const POSIX_CLASSES: &[(&str, &str, bool)] = &[
    ("[:alnum:]", "\\p{L}\\p{Nl}\\p{Nd}", false),
    ("[:alpha:]", "\\p{L}\\p{Nl}", false),
    ("[:ascii:]", "\\x00-\\x7f", false),
    ("[:blank:]", "\\p{Zs}\\t", false),
    ("[:cntrl:]", "\\p{Cc}", false),
    ("[:digit:]", "\\p{Nd}", false),
    ("[:graph:]", "\\p{Z}\\p{C}", true),
    ("[:lower:]", "\\p{Ll}", false),
    ("[:print:]", "\\p{C}", false),
    ("[:punct:]", "\\p{P}", false),
    ("[:space:]", "\\p{Z}\\t\\r\\n\\v\\f", false),
    ("[:upper:]", "\\p{Lu}", false),
    ("[:word:]", "\\p{L}\\p{Nl}\\p{Nd}\\p{Pc}", false),
    ("[:xdigit:]", "A-Fa-f0-9", false),
];

/// A `[...]` class at `pos`: (source, literal when it is one plain character, chars
/// consumed). `None` when the class is not closed.
fn glob_class(glob: &[char], pos: usize) -> Option<(String, Option<char>, usize)> {
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
            for &(name, source, negated) in POSIX_CLASSES {
                let name: Vec<char> = name.chars().collect();
                if glob[i..].starts_with(&name) {
                    if range_start.is_some() {
                        return Some(("$.".into(), None, glob.len() - pos));
                    }
                    i += name.len();
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
        return Some(("$.".into(), None, glob.len() - pos));
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
            return Some((source, Some(c), end_pos - pos));
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
    Some((source, None, end_pos - pos))
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

struct Source {
    src: String,
    literal: Option<String>,
}

fn glob_source(segment: &[char], at_start: bool) -> Source {
    let (mut src, mut literal, mut magic) = (String::new(), String::new(), false);
    let no_empty = at_start && !segment.is_empty() && segment.iter().all(|&c| c == '*');
    let mut i = 0;
    while i < segment.len() {
        let c = segment[i];
        if "@+*?!".contains(c) && segment.get(i + 1) == Some(&'(') {
            let (mut depth, mut j) = (0usize, i + 2);
            let mut from = j;
            let mut arms: Vec<&[char]> = Vec::new();
            let mut closed = false;
            while j < segment.len() {
                match segment[j] {
                    '(' => depth += 1,
                    ')' if depth == 0 => {
                        closed = true;
                        break;
                    }
                    ')' => depth -= 1,
                    '|' if depth == 0 => {
                        arms.push(&segment[from..j]);
                        from = j + 1;
                    }
                    _ => {}
                }
                j += 1;
            }
            if closed {
                arms.push(&segment[from..j]);
                let guarded = at_start && i == 0;
                let parts: Vec<String> = arms.iter().map(|a| glob_source(a, false).src).collect();
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
                    let rest = glob_source(&segment[j + 1..], false).src;
                    let guard = if guarded { "(?!\\.)" } else { "" };
                    src.push_str(&format!("(?:(?!(?:{open}){rest}$){guard}[^/]*?)"));
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
                i = j + 1;
                continue;
            }
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
            '[' if glob_class(segment, i).is_some() => {
                let (class_src, class_literal, consumed) = glob_class(segment, i).unwrap();
                src.push_str(&class_src);
                match class_literal {
                    Some(l) => literal.push(l),
                    None => magic = true,
                }
                i += consumed;
                continue;
            }
            _ => {
                push_escaped(&mut src, c);
                literal.push(c);
            }
        }
        i += 1;
    }
    let extglob_start = segment.len() >= 2 && "@+*?!".contains(segment[0]) && segment[1] == '(';
    let dots = segment == ['.'] || segment == ['.', '.'];
    if at_start && !extglob_start && !dots {
        src = format!("{}{src}", start_guard(&src));
    }
    Source {
        src,
        literal: (!magic).then_some(literal),
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
