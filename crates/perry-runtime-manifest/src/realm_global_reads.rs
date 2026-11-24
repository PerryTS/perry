//! The global names the runtime reads at run time (owner decision 2026-11-24,
//! RTREAD): derived from the runtime's own lookup sites, never written down.
//!
//! The runtime reads a property of the global object by name through one
//! reader pair, `js_get_global_this_builtin_value(ptr, len)` and its realm-core
//! twin `builtin_value_core(name)` (perry-runtime object_ops/prototype.rs).
//! Every call of a reader names its global. The name argument is evaluated
//! over the source: a string literal; a local bound to a value; a `match` /
//! `if` / block whose values are literals; `Some(..)`; a call of a function
//! whose returned values are literals; or a parameter of the calling function
//! (bare, a field of it, or a value selected by it: `match p`, `G(p)`, a
//! `matches!(p, ..)` guard around the site), which makes the caller a reader
//! whose own callers name the global (to a fixpoint).
//!
//! Two kinds of names (CWN ruling within owner 11-24):
//! (A) the runtime picks the name: a literal, or a value selected by runtime
//!     state; these are RUNTIME_GLOBAL_READS.
//! (B) compiled code picks it: the selecting parameter reaches an extern ABI
//!     entry (an `instanceof` class id, a native-module export getter, a bare
//!     identifier read). That read happens at a program site, which the
//!     compiler models there; it names no runtime global.
//! A call in realm setup (the `populate_*` roots and every function only they
//! reach) runs before any program code and names nothing either. Any other
//! name argument is a build error: the set is exact or the build stops.
use super::{rust_tokens, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The runtime's by-name global readers and the index of their name argument.
const READERS: &[(&str, usize)] = &[
    ("js_get_global_this_builtin_value", 0),
    ("builtin_value_core", 0),
];
/// The realm setup roots (perry-runtime object/global_this/populate.rs).
const SETUP_ROOTS: &[&str] = &[
    "populate_global_this_builtins",
    "populate_realm_core",
    "populate_realm_rest",
];
/// Parameter types that carry a JS value: a name selected by one is chosen by
/// runtime state (the value's brand), never by compiled code.
const VALUE_TYPES: &[&str] = &["f64", "u64", "JSValue", "RuntimeHandle"];

/// What the extraction found: each run-time global name with its sites, and
/// the extern ABI entries through which compiled code names a global (B),
/// with the names they can select (`?` when unknown).
#[derive(Debug, Default)]
pub struct GlobalReads {
    pub names: BTreeMap<String, BTreeSet<String>>,
    pub abi_readers: BTreeMap<String, BTreeSet<String>>,
}

struct Param {
    name: String,
    value: bool,
}

struct Func {
    file: usize,
    name: String,
    params: Vec<Param>,
    body: (usize, usize),
    abi: bool,
}

struct Src {
    path: PathBuf,
    text: String,
}

fn files(root: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() {
            if name != "tests" {
                files(&path, out)?;
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn close(ts: &[Token<'_>], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, t) in ts.iter().enumerate().skip(open) {
        match t.text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The first token in `a..b` at bracket depth 0 that satisfies `p`.
fn find0(ts: &[Token<'_>], a: usize, b: usize, p: impl Fn(&str) -> bool) -> Option<usize> {
    let mut depth = 0i32;
    for j in a..b {
        let t = ts[j].text;
        if depth == 0 && p(t) {
            return Some(j);
        }
        match t {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Split `a..b` at depth-0 tokens equal to `sep`.
fn split0(ts: &[Token<'_>], a: usize, b: usize, sep: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = a;
    while let Some(j) = find0(ts, start, b, |t| t == sep) {
        out.push((start, j));
        start = j + 1;
    }
    out.push((start, b));
    out
}

/// Token ranges that are not runtime code: `#[cfg(test)]` items and `use`
/// declarations (a `use` names a function without calling it).
fn skipped(ts: &[Token<'_>]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < ts.len() {
        let w: Vec<&str> = ts[i..ts.len().min(i + 7)].iter().map(|t| t.text).collect();
        if w == ["#", "[", "cfg", "(", "test", ")", "]"] {
            let end = (i + 7..ts.len()).find(|&j| matches!(ts[j].text, "{" | ";"));
            let end = match end {
                Some(j) if ts[j].text == "{" => close(ts, j).unwrap_or(ts.len() - 1),
                Some(j) => j,
                None => ts.len() - 1,
            };
            out.push((i, end));
            i = end + 1;
            continue;
        }
        if ts[i].text == "use" && (i == 0 || matches!(ts[i - 1].text, ";" | "}" | "{" | "]" | "pub" | ")")) {
            let end = (i..ts.len()).find(|&j| ts[j].text == ";").unwrap_or(ts.len() - 1);
            out.push((i, end));
            i = end + 1;
            continue;
        }
        i += 1;
    }
    out
}

fn in_ranges(r: &[(usize, usize)], i: usize) -> bool {
    r.iter().any(|&(a, b)| a <= i && i <= b)
}

fn functions(file: usize, ts: &[Token<'_>], skip: &[(usize, usize)], out: &mut Vec<Func>) {
    for i in 0..ts.len().saturating_sub(3) {
        if ts[i].text != "fn" || in_ranges(skip, i) {
            continue;
        }
        // `fn $getter` inside a macro body: an item the macro stamps out.
        let (name, after) = if ts[i + 1].text == "$" {
            (format!("${}", ts[i + 2].text), i + 3)
        } else {
            (ts[i + 1].text.to_string(), i + 2)
        };
        if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == '$') {
            continue;
        }
        let Some(open) = (after..ts.len()).find(|&j| ts[j].text == "(") else {
            continue;
        };
        let Some(pclose) = close(ts, open) else { continue };
        let mut params = Vec::new();
        for (a, b) in split0(ts, open + 1, pclose, ",") {
            let Some(colon) = (a..b).find(|&j| ts[j].text == ":") else { continue };
            let p = ts[colon - 1].text;
            if p == "self" {
                continue;
            }
            let value = ts[colon + 1..b].iter().any(|t| VALUE_TYPES.contains(&t.text));
            params.push(Param { name: p.to_string(), value });
        }
        let Some(bopen) = (pclose..ts.len()).find(|&j| matches!(ts[j].text, "{" | ";")) else {
            continue;
        };
        if ts[bopen].text == ";" {
            continue;
        }
        let Some(bclose) = close(ts, bopen) else { continue };
        let abi = ts[i.saturating_sub(4)..i].iter().any(|t| t.text == "extern");
        out.push(Func { file, name, params, body: (bopen, bclose), abi });
    }
}

/// The arguments of the call whose `(` is at `open`.
fn arguments(ts: &[Token<'_>], open: usize) -> Option<Vec<(usize, usize)>> {
    let end = close(ts, open)?;
    if end == open + 1 {
        return Some(Vec::new());
    }
    Some(split0(ts, open + 1, end, ","))
}

/// The value of an expression: the global names it can be, and the
/// (non-value) parameters of its function that select it.
#[derive(Clone, Default)]
struct Val {
    lits: BTreeSet<String>,
    /// Parameters whose value selects the result.
    deps: BTreeSet<usize>,
    /// The expression is exactly this parameter.
    direct: Option<usize>,
    /// A static the extern ABI entry itself passes (a macro-stamped getter's
    /// own export record): compiled code picked it by calling this entry.
    abi_static: bool,
    /// The value is `direct`'s parameter under a `matches!` guard (`lits`).
    guard: bool,
}

impl Val {
    fn join(mut self, o: Val) -> Val {
        self.lits.extend(o.lits);
        self.deps.extend(o.deps);
        self.deps.extend(self.direct.take());
        self.deps.extend(o.direct);
        self.abi_static |= o.abi_static;
        self.guard = false;
        self
    }
}

/// How a reader's parameter names the global it reads (field-wise union
/// over the reader's paths).
#[derive(Clone, Default, PartialEq)]
struct Kind {
    /// The parameter is the name.
    direct: bool,
    /// The parameter is the name, read only when it is one of these
    /// (a `matches!` guard).
    guard: BTreeSet<String>,
    /// The parameter selects the name among these literals (`Some(None)`:
    /// among names not known here).
    derived: Option<Option<BTreeSet<String>>>,
}

impl Kind {
    fn join(&self, o: &Kind) -> Kind {
        let derived = match (&self.derived, &o.derived) {
            (Some(None), _) | (_, Some(None)) => Some(None),
            (Some(Some(a)), Some(Some(b))) => Some(Some(a.union(b).cloned().collect())),
            (x @ Some(_), None) | (None, x @ Some(_)) => x.clone(),
            (None, None) => None,
        };
        Kind { direct: self.direct || o.direct, guard: self.guard.union(&o.guard).cloned().collect(), derived }
    }
    /// The names a caller selects with an argument of value `lits`, and
    /// whether some path names one the extraction cannot know.
    fn names(&self, lits: &BTreeSet<String>) -> (BTreeSet<String>, bool) {
        let mut out = BTreeSet::new();
        let mut unknown = false;
        if self.direct {
            unknown |= lits.is_empty();
            out.extend(lits.iter().cloned());
        }
        out.extend(lits.intersection(&self.guard).cloned());
        match &self.derived {
            Some(Some(l)) => out.extend(l.iter().cloned()),
            Some(None) => unknown = true,
            None => {}
        }
        (out, unknown)
    }
    /// All names this reader can read (any argument).
    fn all(&self) -> Option<BTreeSet<String>> {
        if self.direct || self.derived == Some(None) {
            return None;
        }
        let mut out = self.guard.clone();
        if let Some(Some(l)) = &self.derived {
            out.extend(l.iter().cloned());
        }
        Some(out)
    }
}

struct Cx<'s> {
    srcs: &'s [Src],
    toks: Vec<Vec<Token<'s>>>,
    fs: Vec<Func>,
    owners: Vec<Vec<Option<usize>>>,
    by_name: BTreeMap<String, Vec<usize>>,
}

const DIVERGE: &[&str] = &["unreachable", "panic", "return", "continue", "break", "todo", "unimplemented"];

impl<'s> Cx<'s> {
    fn line(&self, file: usize, i: usize) -> String {
        let src = &self.srcs[file];
        let at = self.toks[file][i].start;
        format!("{}:{}", src.path.display(), src.text[..at].matches('\n').count() + 1)
    }

    fn param(&self, f: usize, id: &str) -> Option<usize> {
        self.fs[f].params.iter().position(|p| p.name == id)
    }

    /// Parameters (that carry no JS value) mentioned in `a..b`.
    fn deps(&self, f: usize, a: usize, b: usize) -> BTreeSet<usize> {
        let ts = &self.toks[self.fs[f].file];
        (a..b)
            .filter(|&j| j == 0 || ts[j - 1].text != ".")
            .filter_map(|j| self.param(f, ts[j].text))
            .filter(|&p| !self.fs[f].params[p].value)
            .collect()
    }

    /// A local `id` is bound before `at` in `f`: the bound value's range.
    fn binding(&self, f: usize, at: usize, id: &str) -> Option<(usize, usize)> {
        let ts = &self.toks[self.fs[f].file];
        let j = (self.fs[f].body.0..at).rev().find(|&j| {
            ts[j].text == "let"
                && (ts[j + 1].text == id
                    || (ts[j + 1].text == "mut" && ts[j + 2].text == id)
                    || (matches!(ts[j + 1].text, "Some" | "Ok")
                        && ts[j + 2].text == "("
                        && ts[j + 3].text == id
                        && ts[j + 4].text == ")"))
        })?;
        let eq = (j..at).find(|&k| ts[k].text == "=")?;
        let iflet = j > 0 && matches!(ts[j - 1].text, "if" | "while");
        let end = if iflet {
            find0(ts, eq + 1, at, |t| t == "{")?
        } else {
            find0(ts, eq + 1, at, |t| t == ";" || t == "else")?
        };
        Some((eq + 1, end))
    }

    /// A `matches!(p, lits)` / `matches!((.., p, ..), (.., lits, ..) | ..)`
    /// guard of an `if` or a match arm whose region holds `at`.
    fn guard(&self, f: usize, at: usize, id: &str) -> Option<BTreeSet<String>> {
        let ts = &self.toks[self.fs[f].file];
        let (b0, b1) = self.fs[f].body;
        for m in b0..at {
            if ts[m].text != "matches" || ts[m + 1].text != "!" || ts[m + 2].text != "(" {
                continue;
            }
            if !matches!(ts[m - 1].text, "if" | "&&") {
                continue;
            }
            let Some(mc) = close(ts, m + 2) else { continue };
            let Some(args) = arguments(ts, m + 2) else { continue };
            if args.len() != 2 {
                continue;
            }
            let (s0, s1) = args[0];
            let pos = if s1 == s0 + 1 && ts[s0].text == id {
                None
            } else if ts[s0].text == "(" && close(ts, s0) == Some(s1 - 1) {
                let elems = split0(ts, s0 + 1, s1 - 1, ",");
                match elems.iter().position(|&(a, b)| b == a + 1 && ts[a].text == id) {
                    Some(k) => Some((k, elems.len())),
                    None => continue,
                }
            } else {
                continue;
            };
            // The pattern's literals at the parameter's position.
            let (p0, p1) = args[1];
            let mut lits = BTreeSet::new();
            let mut ok = true;
            for (a, b) in split0(ts, p0, p1, "|") {
                let alt = match pos {
                    None => (a, b),
                    Some((k, n)) => {
                        if ts[a].text != "(" || close(ts, a) != Some(b - 1) {
                            ok = false;
                            break;
                        }
                        let elems = split0(ts, a + 1, b - 1, ",");
                        if elems.len() != n {
                            ok = false;
                            break;
                        }
                        elems[k]
                    }
                };
                for (x, y) in split0(ts, alt.0, alt.1, "|") {
                    match (y == x + 1).then(|| ts[x].string.clone()).flatten() {
                        Some(s) => {
                            lits.insert(s);
                        }
                        None => ok = false,
                    }
                }
            }
            if !ok {
                continue;
            }
            // The guarded region: the `if` block or the arm's value.
            let Some(stop) = find0(ts, mc + 1, b1, |t| matches!(t, "{" | "=>" | "||" | ";" | ",")) else {
                continue;
            };
            let region = match ts[stop].text {
                "{" => (stop, close(ts, stop)?),
                "=>" if ts[stop + 1].text == "{" => (stop + 1, close(ts, stop + 1)?),
                "=>" => (stop + 1, find0(ts, stop + 1, b1, |t| t == ",").unwrap_or(b1)),
                _ => continue,
            };
            if region.0 < at && at < region.1 {
                return Some(lits);
            }
        }
        None
    }

    /// The value of `a..b` in function `f`.
    fn eval(&self, f: usize, a: usize, b: usize, seen: &mut Vec<usize>) -> Option<Val> {
        let ts = &self.toks[self.fs[f].file];
        let (mut a, mut b) = (a, b);
        while a < b && matches!(ts[a].text, "&" | "mut") {
            a += 1;
        }
        loop {
            let n = b - a;
            if n >= 1 && ts[b - 1].text == "?" {
                b -= 1;
            } else if n >= 4
                && ts[b - 4].text == "."
                && matches!(ts[b - 3].text, "as_ptr" | "as_bytes" | "len" | "as_str")
                && ts[b - 2].text == "("
                && ts[b - 1].text == ")"
            {
                b -= 4;
            } else if n >= 2 && ts[a].text == "(" && close(ts, a) == Some(b - 1) {
                a += 1;
                b -= 1;
            } else {
                break;
            }
        }
        if a >= b {
            return None;
        }
        let t = &ts[a];
        if b == a + 1 {
            if let Some(s) = &t.string {
                return Some(Val { lits: [s.clone()].into(), ..Val::default() });
            }
            let id = t.text;
            if id == "None" {
                return Some(Val::default());
            }
            if let Some((x, y)) = self.binding(f, a, id) {
                return self.eval(f, x, y, seen);
            }
            if let Some(p) = self.param(f, id) {
                if self.fs[f].params[p].value {
                    return None;
                }
                if let Some(lits) = self.guard(f, a, id) {
                    return Some(Val { lits, deps: [p].into(), guard: true, ..Val::default() });
                }
                return Some(Val { direct: Some(p), ..Val::default() });
            }
            if self.fs[f].abi && id.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                return Some(Val { abi_static: true, ..Val::default() });
            }
            return None;
        }
        // A field of a parameter: the parameter selects it.
        if b == a + 3 && ts[a + 1].text == "." {
            let p = self.param(f, t.text)?;
            return (!self.fs[f].params[p].value).then(|| Val { deps: [p].into(), ..Val::default() });
        }
        if matches!(t.text, "Some" | "Ok") && ts[a + 1].text == "(" && close(ts, a + 1) == Some(b - 1) {
            return self.eval(f, a + 2, b - 1, seen);
        }
        if t.text == "unsafe" && ts[a + 1].text == "{" && close(ts, a + 1) == Some(b - 1) {
            return self.block(f, a + 1, b - 1, seen);
        }
        if t.text == "{" && close(ts, a) == Some(b - 1) {
            return self.block(f, a, b - 1, seen);
        }
        if t.text == "match" {
            let open = find0(ts, a + 1, b, |t| t == "{")?;
            if close(ts, open) != Some(b - 1) {
                return None;
            }
            let mut v = Val { deps: self.deps(f, a + 1, open), ..Val::default() };
            let mut k = open + 1;
            while let Some(arrow) = find0(ts, k, b - 1, |t| t == "=>") {
                let (x, y) = if ts[arrow + 1].text == "{" {
                    let c = close(ts, arrow + 1)?;
                    (arrow + 1, c + 1)
                } else {
                    (arrow + 1, find0(ts, arrow + 1, b - 1, |t| t == ",").unwrap_or(b - 1))
                };
                if !DIVERGE.contains(&ts[x].text) {
                    v = v.join(self.eval(f, x, y, seen)?);
                }
                k = if y < b - 1 && ts[y].text == "," { y + 1 } else { y };
            }
            return Some(v);
        }
        if t.text == "if" {
            let open = find0(ts, a + 1, b, |t| t == "{")?;
            let c = close(ts, open)?;
            let mut v = Val { deps: self.deps(f, a + 1, open), ..Val::default() };
            v = v.join(self.block(f, open, c, seen)?);
            if c + 1 >= b || ts[c + 1].text != "else" {
                return None;
            }
            return Some(v.join(self.eval(f, c + 2, b, seen)?));
        }
        // A call of a function whose values are literals.
        if ts[b - 1].text == ")" {
            let open = (a..b).find(|&j| ts[j].text == "(")?;
            if close(ts, open) != Some(b - 1) {
                return None;
            }
            let path_ok = (a..open).all(|j| {
                if (j - a) % 2 == 1 {
                    ts[j].text == "::"
                } else {
                    ts[j].text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                }
            });
            if !path_ok || (open - a) % 2 == 0 {
                return None;
            }
            let lits = self.returns(&ts[open - 1].text.to_string(), seen)?;
            return Some(Val { lits, deps: self.deps(f, open + 1, b - 1), ..Val::default() });
        }
        None
    }

    /// A block's value: its tail expression.
    fn block(&self, f: usize, open: usize, close_: usize, seen: &mut Vec<usize>) -> Option<Val> {
        let ts = &self.toks[self.fs[f].file];
        let mut start = open + 1;
        while let Some(j) = find0(ts, start, close_, |t| t == ";") {
            start = j + 1;
        }
        // Block statements (`if`, `match`, loops) need no `;` before the tail.
        loop {
            if start >= close_ {
                return None;
            }
            let kw = ts[start].text;
            if !matches!(kw, "if" | "match" | "for" | "while" | "loop" | "unsafe" | "{") {
                break;
            }
            let mut end = if kw == "{" {
                close(ts, start)?
            } else {
                close(ts, find0(ts, start + 1, close_, |t| t == "{")?)?
            };
            while kw == "if" && end + 1 < close_ && ts[end + 1].text == "else" {
                end = close(ts, find0(ts, end + 2, close_, |t| t == "{")?)?;
            }
            if end + 1 >= close_ {
                break;
            }
            start = end + 1;
        }
        self.eval(f, start, close_, seen)
    }

    /// The literals every definition of `name` returns (tail and `return`s).
    fn returns(&self, name: &str, seen: &mut Vec<usize>) -> Option<BTreeSet<String>> {
        let defs = self.by_name.get(name)?;
        let mut out = BTreeSet::new();
        for &g in defs {
            if seen.contains(&g) {
                return None;
            }
            seen.push(g);
            let ts = &self.toks[self.fs[g].file];
            let (b0, b1) = self.fs[g].body;
            let tail = self.block(g, b0, b1, seen);
            let mut v = tail?;
            for j in b0 + 1..b1 {
                if ts[j].text != "return" || self.owners[self.fs[g].file][j] != Some(g) {
                    continue;
                }
                let end = find0(ts, j + 1, b1, |t| matches!(t, ";" | "," | "}")).unwrap_or(b1);
                v = v.join(self.eval(g, j + 1, end, seen)?);
            }
            seen.pop();
            out.extend(v.lits);
        }
        Some(out)
    }

    /// The (non-value) parameters `a..b` depends on, through locals.
    fn deps_deep(&self, f: usize, a: usize, b: usize, depth: u32) -> BTreeSet<usize> {
        let ts = &self.toks[self.fs[f].file];
        let mut out = BTreeSet::new();
        for j in a..b {
            if j > 0 && ts[j - 1].text == "." {
                continue;
            }
            if let Some(p) = self.param(f, ts[j].text) {
                if !self.fs[f].params[p].value {
                    out.insert(p);
                }
            } else if depth < 4 {
                if let Some((x, y)) = self.binding(f, j, ts[j].text) {
                    out.extend(self.deps_deep(f, x, y, depth + 1));
                }
            }
        }
        out
    }

    /// `id` at `i` is a local of its function: a parameter or a `let`.
    fn local(&self, f: usize, i: usize, id: &str) -> bool {
        self.param(f, id).is_some() || self.binding(f, i, id).is_some()
    }
}

pub fn read(runtime_src: &Path) -> Result<GlobalReads, String> {
    let mut paths = Vec::new();
    files(runtime_src, &mut paths).map_err(|e| e.to_string())?;
    let stdlib = runtime_src.join("../../perry-stdlib/src");
    if stdlib.is_dir() {
        files(&stdlib, &mut paths).map_err(|e| e.to_string())?;
    }
    let srcs: Vec<Src> = paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("cargo:rerun-if-changed={}", path.display());
            Ok(Src { path, text })
        })
        .collect::<Result<_, String>>()?;
    read_sources(&srcs)
}

/// The extraction over parsed sources (also the unit tests' entry).
fn read_sources(srcs: &[Src]) -> Result<GlobalReads, String> {
    let toks: Vec<Vec<Token<'_>>> = srcs
        .iter()
        .map(|s| rust_tokens(&s.text).map_err(|e| format!("{}: {e}", s.path.display())))
        .collect::<Result<_, String>>()?;
    let skips: Vec<_> = toks.iter().map(|ts| skipped(ts)).collect();
    let mut fs = Vec::new();
    for (k, ts) in toks.iter().enumerate() {
        functions(k, ts, &skips[k], &mut fs);
    }
    // The innermost function owning each token, per file.
    let mut owners: Vec<Vec<Option<usize>>> = toks.iter().map(|ts| vec![None; ts.len()]).collect();
    let mut by_size: Vec<usize> = (0..fs.len()).collect();
    by_size.sort_by_key(|&k| std::cmp::Reverse(fs[k].body.1 - fs[k].body.0));
    for k in by_size {
        let f = &fs[k];
        for slot in &mut owners[f.file][f.body.0 + 1..f.body.1] {
            *slot = Some(k);
        }
    }
    let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (k, f) in fs.iter().enumerate() {
        by_name.entry(f.name.clone()).or_default().push(k);
    }
    let cx = Cx { srcs, toks, fs, owners, by_name };
    let (toks, fs, owners) = (&cx.toks, &cx.fs, &cx.owners);
    // References of each function name: the function they occur in (None:
    // outside every body, a table or static).
    let mut refs: BTreeMap<&str, Vec<Option<usize>>> = BTreeMap::new();
    for (k, ts) in toks.iter().enumerate() {
        for i in 1..ts.len() {
            let t = ts[i].text;
            if ts[i - 1].text == "fn" || in_ranges(&skips[k], i) || !cx.by_name.contains_key(t) {
                continue;
            }
            refs.entry(t).or_default().push(owners[k][i]);
        }
    }
    // Realm setup: the roots and every function only setup functions reach.
    let mut setup: BTreeSet<&str> = SETUP_ROOTS.iter().copied().collect();
    loop {
        let grow: Vec<&str> = refs
            .iter()
            .filter(|(n, r)| {
                !setup.contains(*n)
                    && !r.is_empty()
                    && r.iter().all(|o| o.is_some_and(|o| setup.contains(fs[o].name.as_str())))
            })
            .map(|(n, _)| *n)
            .collect();
        if grow.is_empty() {
            break;
        }
        setup.extend(grow);
    }
    // A module that includes build-generated code: its callers may live there.
    let generated: BTreeSet<usize> = (0..srcs.len())
        .filter(|&k| srcs[k].text.contains("include!(concat!(env!(\"OUT_DIR\")"))
        .collect();
    let mut out = GlobalReads::default();
    let mut errors = Vec::new();
    let mut readers: BTreeMap<String, (usize, Kind)> =
        READERS.iter().map(|&(n, i)| (n.to_string(), (i, Kind { direct: true, ..Kind::default() }))).collect();
    let mut work: Vec<String> = readers.keys().cloned().collect();
    let mut done = BTreeSet::new();
    let abi = |out: &mut GlobalReads, f: &Func, lits: Option<&BTreeSet<String>>| {
        let e = out.abi_readers.entry(f.name.clone()).or_default();
        match lits {
            Some(l) if !l.is_empty() => e.extend(l.iter().cloned()),
            _ => {
                e.insert("?".into());
            }
        }
    };
    while let Some(r) = work.pop() {
        if !done.insert(r.clone()) {
            continue;
        }
        let mut calls = 0usize;
        for (k, ts) in toks.iter().enumerate() {
            for i in 1..ts.len() {
                if ts[i].text != r || ts[i - 1].text == "fn" || in_ranges(&skips[k], i) {
                    continue;
                }
                let call = ts.get(i + 1).map(|t| t.text) == Some("(");
                if !call {
                    // A field, a parameter or a local of the same name.
                    let decl = ts.get(i + 1).map(|t| t.text) == Some(":");
                    let local = owners[k][i].is_some_and(|o| cx.local(o, i, &r));
                    if ts[i - 1].text == "." || decl || local {
                        continue;
                    }
                }
                let Some(o) = owners[k][i] else {
                    errors.push(format!("{}: reader `{r}` outside a function", cx.line(k, i)));
                    continue;
                };
                let f = &fs[o];
                if setup.contains(f.name.as_str()) {
                    continue;
                }
                if !call {
                    errors.push(format!("{}: reader `{r}` used as a value", cx.line(k, i)));
                    continue;
                }
                calls += 1;
                let (idx, kind) = &readers[&r];
                let arg = arguments(ts, i + 1).and_then(|a| a.get(*idx).copied());
                if arg.is_some_and(|(a, b)| b == a + 1 && ts[a].text == "None") {
                    // A literal `None` for the name names no global: no read.
                    continue;
                }
                // Not evaluable: a value computed from the caller's
                // parameters is selected by them.
                let val = arg.and_then(|(a, b)| {
                    cx.eval(o, a, b, &mut Vec::new()).or_else(|| {
                        let deps = cx.deps_deep(o, a, b, 0);
                        (!deps.is_empty()).then(|| Val { deps, ..Val::default() })
                    })
                });
                let carried = kind.clone();
                let fail = |errors: &mut Vec<String>| {
                    errors.push(format!(
                        "{}: cannot name the global `{r}` reads (a literal, a value selected by literals, or a parameter)",
                        cx.line(k, i)
                    ))
                };
                let name_here = |out: &mut GlobalReads, errors: &mut Vec<String>, lits: &BTreeSet<String>| {
                    let (names, unknown) = carried.names(lits);
                    if unknown {
                        fail(errors);
                    }
                    for n in names {
                        out.names.entry(n).or_default().insert(cx.line(k, i));
                    }
                };
                let Some(v) = val else {
                    // The caller picks a value the extraction cannot read.
                    match carried.all() {
                        Some(l) if !carried.direct && carried.guard.is_empty() => name_here(&mut out, &mut errors, &l),
                        _ => fail(&mut errors),
                    }
                    continue;
                };
                if v.abi_static || (f.abi && (v.direct.is_some() || !v.deps.is_empty())) {
                    // Compiled code picks the name through this entry (B).
                    let lits = carried.all().or_else(|| (!v.lits.is_empty()).then(|| v.lits.clone()));
                    abi(&mut out, f, lits.as_ref());
                    continue;
                }
                let dep = v.direct.or_else(|| (v.deps.len() == 1).then(|| *v.deps.iter().next().unwrap()));
                if v.deps.len() > 1 || (v.direct.is_some() && !v.deps.is_empty()) {
                    errors.push(format!("{}: `{}` selects the global by two parameters", cx.line(k, i), f.name));
                    continue;
                }
                let Some(p) = dep else {
                    // The runtime names it here.
                    name_here(&mut out, &mut errors, &v.lits);
                    continue;
                };
                // The caller's parameter names it: the caller is a reader.
                let new_kind = if v.direct.is_some() {
                    carried.clone()
                } else if v.guard {
                    // `p` under `matches!(p, lits)`: the name is `p` there.
                    let mut guard: BTreeSet<String> = BTreeSet::new();
                    if carried.direct {
                        guard.extend(v.lits.iter().cloned());
                    }
                    guard.extend(v.lits.intersection(&carried.guard).cloned());
                    Kind { direct: false, guard, derived: carried.derived.clone() }
                } else {
                    // `p` selects the value: every name its paths can be.
                    let (names, unknown) = carried.names(&v.lits);
                    let derived = if unknown || (carried.direct && v.lits.is_empty()) {
                        None
                    } else {
                        let mut l = names;
                        l.extend(carried.guard.iter().cloned());
                        Some(l)
                    };
                    Kind { derived: Some(derived), ..Kind::default() }
                };
                let merged = match readers.get(&f.name) {
                    None => new_kind,
                    Some((q, _)) if *q != p => {
                        errors.push(format!("{}: `{}` reads two parameters", cx.line(k, i), f.name));
                        continue;
                    }
                    Some((_, old)) => {
                        let j = old.join(&new_kind);
                        if &j == old {
                            continue;
                        }
                        j
                    }
                };
                readers.insert(f.name.clone(), (p, merged));
                done.remove(&f.name);
                work.push(f.name.clone());
            }
        }
        // A reader no scanned code calls: called from build-generated code
        // (the native export getters, extern ABI entries compiled code picks),
        // or unknown.
        if calls == 0 && !READERS.iter().any(|&(n, _)| n == r) {
            let defs = &cx.by_name[&r];
            if defs.iter().all(|&g| generated.contains(&fs[g].file)) {
                let carried = readers[&r].1.all();
                let e = out.abi_readers.entry(format!("{r} (generated callers)")).or_default();
                match carried {
                    Some(l) => e.extend(l),
                    None => {
                        e.insert("?".into());
                    }
                }
            } else {
                errors.push(format!("runtime reader `{r}` has no call site in the scanned runtime"));
            }
        }
    }
    if !errors.is_empty() {
        errors.sort();
        errors.dedup();
        return Err(format!("runtime global reads:\n{}", errors.join("\n")));
    }
    if out.names.is_empty() {
        return Err("runtime global reads: no run-time global name found".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(files: &[(&str, &str)]) -> Result<GlobalReads, String> {
        let srcs: Vec<Src> = files
            .iter()
            .map(|(p, t)| Src { path: PathBuf::from(p), text: t.to_string() })
            .collect();
        read_sources(&srcs)
    }

    const READER: &str = "#[no_mangle] pub extern \"C\" fn js_get_global_this_builtin_value(ptr: *const u8, len: usize) -> f64 { builtin_value_core(ptr) }\nfn builtin_value_core(name: &str) -> f64 { 0.0 }\n";

    #[test]
    fn literal_let_match_param_and_returns() {
        let src = format!(
            "{READER}fn a() {{ builtin_value_core(\"Temporal\"); }}\n\
             fn b(v: f64) {{ let n = match v as u8 {{ 0 => \"MessagePort\", _ => \"MessageChannel\" }}; builtin_value_core(n); }}\n\
             fn c(name: &str) {{ builtin_value_core(name); }} fn d() {{ c(\"WebAssembly\"); }}\n\
             fn brand(v: f64) -> Option<&'static str> {{ if v > 0.0 {{ return Some(\"Date\"); }} Some(\"Buffer\") }}\n\
             fn e(v: f64) -> Option<()> {{ let n = brand(v)?; builtin_value_core(n); None }}\n"
        );
        let r = run(&[("x.rs", &src)]).unwrap();
        let names: Vec<&str> = r.names.keys().map(String::as_str).collect();
        assert_eq!(names, ["Buffer", "Date", "MessageChannel", "MessagePort", "Temporal", "WebAssembly"]);
    }

    #[test]
    fn compiled_code_names_are_not_runtime_reads() {
        // An instanceof class id and a guarded export name reach extern ABI
        // entries: compiled code picks those names (kind B).
        let src = format!(
            "{READER}fn heap(id: u32) -> Option<&'static str> {{ Some(match id {{ 1 => \"EventTarget\", _ => return None }}) }}\n\
             fn inst(id: u32) {{ if let Some(n) = heap(id) {{ builtin_value_core(n); }} }}\n\
             #[no_mangle] pub extern \"C\" fn js_instanceof(v: f64, id: u32) -> f64 {{ inst(id); v }}\n\
             fn special(key: &str, name: &str) {{ if matches!(name, \"ReadableStream\" | \"Blob\") {{ builtin_value_core(name); }} }}\n\
             #[no_mangle] pub extern \"C\" fn js_import(key: &str, name: &str) {{ special(key, name); }}\n\
             fn a() {{ builtin_value_core(\"MessagePort\"); special(\"k\", \"default\"); special(\"k\", \"Blob\"); }}\n"
        );
        let r = run(&[("x.rs", &src)]).unwrap();
        // The runtime's own literal `Blob` is a run-time read; `default` is
        // outside the guard and reads nothing.
        assert_eq!(r.names.keys().collect::<Vec<_>>(), ["Blob", "MessagePort"]);
        assert!(r.abi_readers["js_instanceof"].contains("EventTarget"));
        assert!(r.abi_readers["js_import"].contains("ReadableStream"));
    }

    #[test]
    fn realm_setup_names_nothing() {
        let src = format!(
            "{READER}fn populate_realm_core() {{ helper(); }} fn helper() {{ builtin_value_core(\"Uint8Array\"); }}\n\
             fn a() {{ builtin_value_core(\"Temporal\"); }}\n"
        );
        let r = run(&[("x.rs", &src)]).unwrap();
        assert_eq!(r.names.keys().collect::<Vec<_>>(), ["Temporal"]);
    }

    #[test]
    fn a_literal_none_name_reads_nothing() {
        let src = format!(
            "{READER}fn by(v: f64, binding: Option<&'static str>) {{ if let Some(n) = binding {{ builtin_value_core(n); }} }}\n\
             fn a(v: f64) {{ by(v, None); }}\n\
             fn b(v: f64) {{ by(v, Some(\"Promise\")); }}\n"
        );
        let r = run(&[("x.rs", &src)]).unwrap();
        assert_eq!(r.names.keys().collect::<Vec<_>>(), ["Promise"]);
        assert_eq!(r.names["Promise"].len(), 1, "{:?}", r.names);
    }

    #[test]
    fn an_unnameable_read_stops_the_build() {
        let src = format!("{READER}fn a(v: f64) {{ let n = lookup(v); builtin_value_core(n); }}\nfn b() {{ builtin_value_core(\"Temporal\"); }}\n");
        let e = run(&[("x.rs", &src)]).unwrap_err();
        assert!(e.contains("x.rs:3: cannot name the global `builtin_value_core` reads"), "{e}");
    }
}
