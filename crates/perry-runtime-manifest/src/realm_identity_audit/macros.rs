//! The functions a `macro_rules!` invocation defines: a getter body the
//! runtime makes with a macro (`url_getter!(get_href, "href", URL_HREF)`,
//! `getter!(ports, state::MESSAGE_PORTS)`, a repetition of
//! `fetch_getter_bodies! { fetch_url => "url", .., }`) is found by its name,
//! with the body the invocation expands to.
//!
//! A matcher and transcriber of `macro_rules!` for the fragments the
//! runtime's macros use. Like the rest of the audit it is a precision aid:
//! an invocation no rule matches defines nothing here. A macro is taken from
//! the invoking file when that file defines one of the name (textual scope),
//! else from any file that does (`#[macro_export]`, a parent module).
use std::collections::{BTreeMap, BTreeSet};

/// A token tree: an identifier, keyword, lifetime or number (`Word`), a
/// string, byte-string or char literal (`Lit`), a punctuation character, a
/// metavariable `$name`, or a delimited group.
#[derive(Clone, Debug, PartialEq)]
enum Tt {
    Word(String),
    Lit(String),
    Punct(char),
    Var(String),
    Group(char, Vec<Tt>),
}

#[derive(Clone, Debug)]
enum Bound {
    One(Vec<Tt>),
    Rep(Vec<Binds>),
}

type Binds = BTreeMap<String, Bound>;

/// One rule: its matcher and its transcriber (the groups' contents).
type Rule = (Vec<Tt>, Vec<Tt>);

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn close_of(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

/// The token trees of `s` from byte `*i` until the closer of the enclosing
/// group (consumed) or the end.
fn lex_seq(s: &str, i: &mut usize) -> Vec<Tt> {
    let mut out = Vec::new();
    while let Some(t) = lex_tt(s, i) {
        match t {
            Some(t) => out.push(t),
            None => break,
        }
    }
    out
}

/// One token tree at `*i`: `None` at the end, `Some(None)` at a closer
/// (consumed).
fn lex_tt(s: &str, i: &mut usize) -> Option<Option<Tt>> {
    loop {
        let rest = &s[*i..];
        let c = rest.chars().next()?;
        if c.is_whitespace() {
            *i += c.len_utf8();
        } else if rest.starts_with("//") {
            *i += rest.find('\n').unwrap_or(rest.len());
        } else if rest.starts_with("/*") {
            *i += rest[2..].find("*/").map_or(rest.len(), |k| k + 4);
        } else {
            break;
        }
    }
    let rest = &s[*i..];
    let c = rest.chars().next()?;
    match c {
        ')' | ']' | '}' => {
            *i += 1;
            Some(None)
        }
        '(' | '[' | '{' => {
            *i += 1;
            let inner = lex_seq(s, i);
            Some(Some(Tt::Group(c, inner)))
        }
        '"' => {
            let n = string_len(rest, 0);
            *i += n;
            Some(Some(Tt::Lit(rest[..n].to_string())))
        }
        '\'' => {
            // A char literal (`'a'`, `'\n'`) or a lifetime (`'a`).
            let mut cs = rest[1..].char_indices();
            let lit = match cs.next() {
                Some((_, '\\')) => rest[2..].find('\'').map(|k| k + 3),
                Some((k, ch)) => {
                    let after = 1 + k + ch.len_utf8();
                    rest[after..].starts_with('\'').then_some(after + 1)
                }
                None => None,
            };
            if let Some(n) = lit {
                *i += n;
                return Some(Some(Tt::Lit(rest[..n].to_string())));
            }
            let n = 1 + rest[1..].find(|c: char| !word_char(c)).unwrap_or(rest.len() - 1);
            *i += n;
            Some(Some(Tt::Word(rest[..n].to_string())))
        }
        '$' if rest[1..].starts_with(word_char) => {
            let n = 1 + rest[1..].find(|c: char| !word_char(c)).unwrap_or(rest.len() - 1);
            *i += n;
            Some(Some(Tt::Var(rest[1..n].to_string())))
        }
        c if word_char(c) => {
            let mut n = rest.find(|c: char| !word_char(c)).unwrap_or(rest.len());
            // A prefixed string (`b"..."`, `r#"..."#`, `br"..."`, `c"..."`).
            if matches!(&rest[..n], "b" | "r" | "br" | "c" | "cr")
                && rest[n..].starts_with(['"', '#'])
            {
                let raw = rest[..n].contains('r');
                let m = if raw { raw_len(rest, n) } else { string_len(rest, n) };
                *i += m;
                return Some(Some(Tt::Lit(rest[..m].to_string())));
            }
            // A number with a fraction (`1.5`).
            if c.is_ascii_digit() && rest[n..].starts_with('.') && rest[n + 1..].starts_with(|c: char| c.is_ascii_digit()) {
                n += 1 + rest[n + 1..].find(|c: char| !word_char(c)).unwrap_or(rest.len() - n - 1);
            }
            *i += n;
            Some(Some(Tt::Word(rest[..n].to_string())))
        }
        c => {
            *i += c.len_utf8();
            Some(Some(Tt::Punct(c)))
        }
    }
}

/// The length of the string literal whose `"` is at `at` (escapes skipped).
fn string_len(s: &str, at: usize) -> usize {
    let b = s.as_bytes();
    let mut k = at + 1;
    while k < b.len() {
        match b[k] {
            b'\\' => k += 2,
            b'"' => return k + 1,
            _ => k += 1,
        }
    }
    b.len()
}

/// The length of the raw string literal whose `#`s or `"` start at `at`.
fn raw_len(s: &str, at: usize) -> usize {
    let hashes = s[at..].find(|c: char| c != '#').unwrap_or(0);
    let end = format!("\"{}", "#".repeat(hashes));
    let open = at + hashes + 1;
    s.get(open..)
        .and_then(|r| r.find(&end))
        .map_or(s.len(), |k| open + k + end.len())
}

fn text(t: &Tt, out: &mut String) {
    match t {
        Tt::Word(w) | Tt::Lit(w) => push(out, w),
        Tt::Punct(c) => push(out, &c.to_string()),
        Tt::Var(v) => push(out, &format!("${v}")),
        Tt::Group(d, inner) => {
            push(out, &d.to_string());
            for t in inner {
                text(t, out);
            }
            push(out, &close_of(*d).to_string());
        }
    }
}

/// Appends `s`, with a space where two words or literals would join.
fn push(out: &mut String, s: &str) {
    let wordy = |c: char| word_char(c) || c == '"' || c == '\'';
    if out.chars().last().is_some_and(wordy) && s.chars().next().is_some_and(wordy) {
        out.push(' ');
    }
    out.push_str(s);
}

fn same(a: &Tt, b: &Tt) -> bool {
    a == b
}

/// The metavariables `ts` names, at any depth.
fn vars(ts: &[Tt], out: &mut BTreeSet<String>) {
    for t in ts {
        match t {
            Tt::Var(v) => {
                out.insert(v.clone());
            }
            Tt::Group(_, inner) => vars(inner, out),
            _ => {}
        }
    }
}

/// The repetition operator (and separator) after a `$( .. )` at `p`, and
/// the tokens they take.
fn rep_op(ts: &[Tt], p: usize) -> Option<(Option<Tt>, char, usize)> {
    let op = |t: Option<&Tt>| match t {
        Some(Tt::Punct(c @ ('*' | '+' | '?'))) => Some(*c),
        _ => None,
    };
    if let Some(c) = op(ts.get(p)) {
        return Some((None, c, 1));
    }
    let c = op(ts.get(p + 1))?;
    Some((Some(ts[p].clone()), c, 2))
}

/// The input fragment `frag` takes at `*at`.
fn fragment(frag: &str, inp: &[Tt], at: &mut usize) -> Option<Vec<Tt>> {
    let one = |at: &mut usize| {
        let t = inp.get(*at)?.clone();
        *at += 1;
        Some(vec![t])
    };
    match frag {
        "ident" | "lifetime" => match inp.get(*at) {
            Some(Tt::Word(_)) => one(at),
            _ => None,
        },
        "tt" => one(at),
        "block" => match inp.get(*at) {
            Some(Tt::Group('{', _)) => one(at),
            _ => None,
        },
        "literal" => {
            let neg = matches!(inp.get(*at), Some(Tt::Punct('-'))) as usize;
            match inp.get(*at + neg) {
                Some(Tt::Lit(_)) => {}
                Some(Tt::Word(w)) if w.starts_with(|c: char| c.is_ascii_digit()) || w == "true" || w == "false" => {}
                _ => return None,
            }
            let got = inp[*at..*at + neg + 1].to_vec();
            *at += neg + 1;
            Some(got)
        }
        "vis" => {
            let mut got = Vec::new();
            if matches!(inp.get(*at), Some(Tt::Word(w)) if w == "pub") {
                got.push(inp[*at].clone());
                *at += 1;
                if let Some(g @ Tt::Group('(', _)) = inp.get(*at) {
                    got.push(g.clone());
                    *at += 1;
                }
            }
            Some(got)
        }
        // `expr`, `ty`, `path`, `pat`, `stmt`, `item`, `meta`: up to the
        // next `,`, `;` or `=>` outside generic brackets.
        _ => {
            let angles = matches!(frag, "ty" | "path");
            let mut depth = 0i32;
            let start = *at;
            while let Some(t) = inp.get(*at) {
                match t {
                    Tt::Punct(',' | ';') if depth == 0 => break,
                    Tt::Punct('=') if depth == 0 && inp.get(*at + 1) == Some(&Tt::Punct('>')) => break,
                    Tt::Punct('<') if angles => depth += 1,
                    Tt::Punct('>') if angles => depth -= 1,
                    _ => {}
                }
                *at += 1;
            }
            (*at > start).then(|| inp[start..*at].to_vec())
        }
    }
}

/// Matches pattern `pat` against `inp` from `*at`, binding into `b`.
fn match_seq(pat: &[Tt], inp: &[Tt], at: &mut usize, b: &mut Binds) -> bool {
    let mut p = 0;
    while p < pat.len() {
        match &pat[p] {
            Tt::Var(v) if pat.get(p + 1) == Some(&Tt::Punct(':')) => {
                let Some(Tt::Word(frag)) = pat.get(p + 2) else {
                    return false;
                };
                let Some(got) = fragment(frag, inp, at) else {
                    return false;
                };
                b.insert(v.clone(), Bound::One(got));
                p += 3;
            }
            Tt::Punct('$') if matches!(pat.get(p + 1), Some(Tt::Group('(', _))) => {
                let Some(Tt::Group(_, inner)) = pat.get(p + 1) else {
                    unreachable!()
                };
                let Some((sep, op, n)) = rep_op(pat, p + 2) else {
                    return false;
                };
                p += 2 + n;
                let mut reps: Vec<Binds> = Vec::new();
                loop {
                    let mut a = *at;
                    if let (false, Some(s)) = (reps.is_empty(), &sep) {
                        if !inp.get(a).is_some_and(|t| same(t, s)) {
                            break;
                        }
                        a += 1;
                    }
                    let mut b2 = Binds::new();
                    if !match_seq(inner, inp, &mut a, &mut b2) || a == *at {
                        break;
                    }
                    *at = a;
                    reps.push(b2);
                    if op == '?' {
                        break;
                    }
                }
                if op == '+' && reps.is_empty() {
                    return false;
                }
                let mut vs = BTreeSet::new();
                vars(inner, &mut vs);
                for v in vs {
                    b.insert(v, Bound::Rep(reps.clone()));
                }
            }
            Tt::Group(d, inner) => {
                let Some(Tt::Group(d2, inner2)) = inp.get(*at) else {
                    return false;
                };
                let mut a = 0;
                if d != d2 || !match_seq(inner, inner2, &mut a, b) || a != inner2.len() {
                    return false;
                }
                *at += 1;
                p += 1;
            }
            t => {
                if !inp.get(*at).is_some_and(|i| same(t, i)) {
                    return false;
                }
                *at += 1;
                p += 1;
            }
        }
    }
    true
}

fn lookup<'a>(scope: &[&'a Binds], v: &str) -> Option<&'a Bound> {
    scope.iter().rev().find_map(|b| b.get(v))
}

/// The text transcriber `body` makes under the bindings `scope`.
fn transcribe<'a>(body: &[Tt], scope: &[&'a Binds], out: &mut String) {
    let mut i = 0;
    while i < body.len() {
        match &body[i] {
            Tt::Var(v) => match lookup(scope, v) {
                Some(Bound::One(ts)) => ts.iter().for_each(|t| text(t, out)),
                Some(Bound::Rep(_)) => {}
                // `$crate`.
                None => push(out, v),
            },
            Tt::Punct('$') if matches!(body.get(i + 1), Some(Tt::Group('(', _))) => {
                let Some(Tt::Group(_, inner)) = body.get(i + 1) else {
                    unreachable!()
                };
                let (sep, n) = match rep_op(body, i + 2) {
                    Some((sep, _, n)) => (sep, n),
                    None => (None, 0),
                };
                i += 2 + n;
                let mut vs = BTreeSet::new();
                vars(inner, &mut vs);
                let reps = vs.iter().find_map(|v| match lookup(scope, v) {
                    Some(Bound::Rep(r)) => Some(r),
                    _ => None,
                });
                for (k, r) in reps.into_iter().flatten().enumerate() {
                    if let (true, Some(s)) = (k > 0, &sep) {
                        text(s, out);
                    }
                    let mut sc = scope.to_vec();
                    sc.push(r);
                    transcribe(inner, &sc, out);
                }
                continue;
            }
            Tt::Group(d, inner) => {
                push(out, &d.to_string());
                transcribe(inner, scope, out);
                push(out, &close_of(*d).to_string());
            }
            t => text(t, out),
        }
        i += 1;
    }
}

/// Does a transcriber define a function under a metavariable's name
/// (`fn $name`)?
fn defines_fn(ts: &[Tt]) -> bool {
    ts.windows(2)
        .any(|w| matches!(w, [Tt::Word(f), Tt::Var(_)] if f == "fn"))
        || ts.iter().any(|t| matches!(t, Tt::Group(_, inner) if defines_fn(inner)))
}

/// The `macro_rules!` definitions of `text` that define functions by a
/// metavariable's name: name -> rules.
fn definitions(text: &str) -> Vec<(String, Vec<Rule>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(k) = text[at..].find("macro_rules!") {
        let mut i = at + k + "macro_rules!".len();
        at = i;
        let Some(Some(Tt::Word(name))) = lex_tt(text, &mut i) else {
            continue;
        };
        let Some(Some(Tt::Group(_, rules))) = lex_tt(text, &mut i) else {
            continue;
        };
        at = i;
        let mut parsed = Vec::new();
        let mut r = 0;
        while r < rules.len() {
            if let (Some(Tt::Group(_, m)), Some(Tt::Punct('=')), Some(Tt::Punct('>')), Some(Tt::Group(_, t))) =
                (rules.get(r), rules.get(r + 1), rules.get(r + 2), rules.get(r + 3))
            {
                parsed.push((m.clone(), t.clone()));
                r += 4;
            } else {
                r += 1;
            }
        }
        if parsed.iter().any(|(_, t)| defines_fn(t)) {
            out.push((name, parsed));
        }
    }
    out
}

/// The source text every invocation of a function-defining macro in
/// `files` expands to.
pub(crate) fn expansions(files: &[String]) -> Vec<String> {
    let defs: Vec<Vec<(String, Vec<Rule>)>> = files.iter().map(|t| definitions(t)).collect();
    let mut any: BTreeMap<&str, Vec<&Vec<Rule>>> = BTreeMap::new();
    for (n, rules) in defs.iter().flatten() {
        any.entry(n.as_str()).or_default().push(rules);
    }
    let mut out = Vec::new();
    for (fi, text) in files.iter().enumerate() {
        let mut at = 0;
        while let Some(k) = text[at..].find('!') {
            let bang = at + k;
            at = bang + 1;
            let name_start = text[..bang]
                .rfind(|c: char| !word_char(c))
                .map_or(0, |s| s + 1);
            let name = &text[name_start..bang];
            let Some(global) = any.get(name) else {
                continue;
            };
            let local: Vec<&Vec<Rule>> = defs[fi]
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, r)| r)
                .collect();
            let cands = if local.is_empty() { global } else { &local };
            let mut i = bang + 1;
            let Some(Some(Tt::Group(_, inp))) = lex_tt(text, &mut i) else {
                continue;
            };
            'def: for rules in cands {
                for (m, t) in rules.iter() {
                    let mut a = 0;
                    let mut b = Binds::new();
                    if match_seq(m, &inp, &mut a, &mut b) && a == inp.len() {
                        let mut s = String::new();
                        transcribe(t, &[&b], &mut s);
                        out.push(s);
                        break 'def;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invocation_expands_to_the_functions_its_rule_defines() {
        let src = "macro_rules! url_getter {\n\
                   ($fn_name:ident, $name:literal, $slot:expr) => {\n\
                   extern \"C\" fn $fn_name(this: f64) -> f64 { read($name, this, $slot) }\n\
                   };\n}\n\
                   url_getter!(get_href, \"href\", crate::URL_HREF);\n\
                   macro_rules! bodies {\n\
                   ($($body:ident => $name:literal, $info:expr),+ $(,)?) => {\n\
                   $(extern \"C\" fn $body(t: f64) -> f64 { fetch(t, $name) })+\n\
                   };\n}\n\
                   bodies! { fetch_a => \"a\", info!(#[x(\"y\")] a::b, 0), fetch_b => \"b\", info!(z), }\n";
        let got = expansions(&[src.to_string()]);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].contains("fn get_href(this:f64)->f64{read(\"href\",this,crate::URL_HREF)}"), "{got:?}");
        assert!(got[1].contains("fn fetch_a(t:f64)->f64{fetch(t,\"a\")}"), "{got:?}");
        assert!(got[1].contains("fn fetch_b(t:f64)->f64{fetch(t,\"b\")}"), "{got:?}");
    }
}
