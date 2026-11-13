//! Rule RT (capture by tail), ECMA-262 22.2.7.2 RegExpBuiltinExec and
//! 22.2.2 pattern semantics: the strings a capture of `exec` may be when
//! the input's end is known (a suffix entry, `Model::plus_strs`) and the
//! pattern is a regex literal of the class below.
//!
//! The class: flags only `d` and `g` (no `i`, `m`, `s`, `u`, `v`, `y`); an
//! optional leading `^`, a trailing `$`; atoms are literal characters,
//! `.`, character classes, the class escapes `\d \w \s \D \W \S`, the
//! character escapes `\n \r \t \f \v \0 \xHH \uXXXX`, identity escapes of
//! ASCII punctuation, capturing, named and non-capturing groups, the
//! quantifiers `* + ? {n} {n,} {n,m}`; alternation only inside a group
//! before group k. A backreference, lookaround, `\b`/`\B`, a `^`/`$`
//! elsewhere or anything else: no rule (every capture stays any string).
//!
//! Group k is the last top-level capture `(C+)` / `(C*)` (C one class or
//! character, greedy) after a part P whose every match ends with a
//! character not in C (a literal L, or e.g. `(?:([^.]*)\.)?`), or is empty
//! only at the input's start (`^`),
//! followed by a tail T of top-level atoms with no alternation and no lazy
//! quantifier. Since `$` without `m` holds only at the input's end
//! (22.2.2.6), every match ends T at the end of the input. Matching T
//! backwards over a known suffix enumerates every way T may match (every
//! split, every iteration count; the last iteration's captures stand,
//! RepeatMatcher 22.2.2.3.1 clears them each iteration). Backtracking takes
//! one of those ways, so their union is a superset of what `exec` gives.
//! Given where T starts, group k is the C-run ending there, and the
//! character before it must be one P may end with (none is in C): its
//! left edge is pinned, since a group start inside the run would leave a C
//! character as P's last.
//! A way that needs a character left of the known suffix (T, or the C-run
//! reaching the suffix's start) may see any string there: every derived
//! capture is any string for that entry.
use crate::model::*;

#[derive(Clone, Debug)]
enum Item {
    Lit(u16),
    Class(Class),
    Group { cap: Option<u32>, alts: Vec<Vec<Quant>> },
}

#[derive(Clone, Debug)]
struct Quant {
    item: Item,
    min: u32,
    max: Option<u32>,
    greedy: bool,
}

/// A set of UTF-16 code units: sorted inclusive ranges.
#[derive(Clone, Debug)]
struct Class(Vec<(u16, u16)>);

impl Class {
    fn has(&self, c: u16) -> bool {
        self.0.iter().any(|&(a, b)| a <= c && c <= b)
    }
    fn norm(mut r: Vec<(u16, u16)>) -> Class {
        r.sort_unstable();
        let mut out: Vec<(u16, u16)> = Vec::new();
        for (a, b) in r {
            match out.last_mut() {
                Some(l) if a as u32 <= l.1 as u32 + 1 => l.1 = l.1.max(b),
                _ => out.push((a, b)),
            }
        }
        Class(out)
    }
    fn not(&self) -> Class {
        let mut out = Vec::new();
        let mut next: u32 = 0;
        for &(a, b) in &self.0 {
            if (a as u32) > next {
                out.push((next as u16, a - 1));
            }
            next = b as u32 + 1;
        }
        if next <= 0xFFFF {
            out.push((next as u16, 0xFFFF));
        }
        Class(out)
    }
}

/// 22.2.2.9: `\d`, `\w` (no `u`/`i`: CharacterClassEscape), `\s`
/// (WhiteSpace and LineTerminator, 12.2 / 12.3).
fn class_escape(c: u8) -> Option<Class> {
    let base = match c.to_ascii_lowercase() {
        b'd' => vec![(0x30, 0x39)],
        b'w' => vec![(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)],
        b's' => vec![
            (0x09, 0x0D),
            (0x20, 0x20),
            (0xA0, 0xA0),
            (0x1680, 0x1680),
            (0x2000, 0x200A),
            (0x2028, 0x2029),
            (0x202F, 0x202F),
            (0x205F, 0x205F),
            (0x3000, 0x3000),
            (0xFEFF, 0xFEFF),
        ],
        _ => return None,
    };
    let k = Class::norm(base);
    Some(if c.is_ascii_uppercase() { k.not() } else { k })
}

struct Parser {
    s: Vec<u16>,
    i: usize,
    groups: u32,
}

impl Parser {
    fn peek(&self) -> Option<u16> {
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c as u16) {
            self.i += 1;
            return true;
        }
        false
    }
    fn hex(&mut self, n: usize) -> Option<u16> {
        let mut v: u32 = 0;
        for _ in 0..n {
            let d = (self.peek()? as u8 as char).to_digit(16)?;
            if self.peek()? > 0x7F {
                return None;
            }
            self.i += 1;
            v = v * 16 + d;
        }
        Some(v as u16)
    }
    /// A character escape after `\` (both inside and outside a class).
    fn char_escape(&mut self, in_class: bool) -> Option<u16> {
        let c = self.peek()?;
        if c > 0x7F {
            return None;
        }
        self.i += 1;
        Some(match c as u8 {
            b'n' => 0x0A,
            b'r' => 0x0D,
            b't' => 0x09,
            b'f' => 0x0C,
            b'v' => 0x0B,
            b'b' if in_class => 0x08,
            b'0' if !self.peek().is_some_and(|d| (0x30..=0x39).contains(&d)) => 0,
            b'x' => self.hex(2)?,
            b'u' if self.peek() != Some(b'{' as u16) => self.hex(4)?,
            b if b.is_ascii_punctuation() => b as u16,
            _ => return None,
        })
    }
    fn class(&mut self) -> Option<Class> {
        let neg = self.eat(b'^');
        let mut r: Vec<(u16, u16)> = Vec::new();
        // One class atom: a character or a class escape.
        enum A {
            C(u16),
            K(Class),
        }
        let atom = |p: &mut Parser| -> Option<A> {
            let c = p.peek()?;
            p.i += 1;
            if c != b'\\' as u16 {
                return Some(A::C(c));
            }
            let e = p.peek()?;
            if e <= 0x7F {
                if let Some(k) = class_escape(e as u8) {
                    p.i += 1;
                    return Some(A::K(k));
                }
            }
            p.char_escape(true).map(A::C)
        };
        loop {
            if self.eat(b']') {
                break;
            }
            let a = atom(self)?;
            let dash = self.peek() == Some(b'-' as u16) && self.s.get(self.i + 1) != Some(&(b']' as u16));
            match (a, dash) {
                (A::C(lo), true) => {
                    self.i += 1;
                    match atom(self)? {
                        A::C(hi) if lo <= hi => r.push((lo, hi)),
                        _ => return None,
                    }
                }
                (A::C(c), false) => r.push((c, c)),
                (A::K(k), false) => r.extend(k.0),
                (A::K(_), true) => return None,
            }
        }
        let k = Class::norm(r);
        Some(if neg { k.not() } else { k })
    }
    /// A sequence of alternatives up to `)` or the end.
    fn alts(&mut self) -> Option<Vec<Vec<Quant>>> {
        let mut alts = vec![self.seq()?];
        while self.eat(b'|') {
            alts.push(self.seq()?);
        }
        Some(alts)
    }
    fn seq(&mut self) -> Option<Vec<Quant>> {
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            if c == b'|' as u16 || c == b')' as u16 {
                break;
            }
            if c == b'$' as u16 && self.i + 1 == self.s.len() {
                break;
            }
            self.i += 1;
            let item = match c as u32 {
                0x28 => {
                    let cap = if self.eat(b'?') {
                        if self.eat(b':') {
                            None
                        } else if self.eat(b'<') {
                            let p = self.peek()?;
                            if p == b'=' as u16 || p == b'!' as u16 {
                                return None;
                            }
                            while !self.eat(b'>') {
                                let n = self.peek()?;
                                if !(n < 0x80 && ((n as u8).is_ascii_alphanumeric() || n == b'_' as u16 || n == b'$' as u16)) {
                                    return None;
                                }
                                self.i += 1;
                            }
                            self.groups += 1;
                            Some(self.groups)
                        } else {
                            return None;
                        }
                    } else {
                        self.groups += 1;
                        Some(self.groups)
                    };
                    let alts = self.alts()?;
                    if !self.eat(b')') {
                        return None;
                    }
                    Item::Group { cap, alts }
                }
                0x5B => Item::Class(self.class()?),
                0x2E => Item::Class(Class::norm(vec![(0x0A, 0x0A), (0x0D, 0x0D), (0x2028, 0x2029)]).not()),
                0x5C => {
                    let e = self.peek()?;
                    match (e <= 0x7F).then(|| class_escape(e as u8)).flatten() {
                        Some(k) => {
                            self.i += 1;
                            Item::Class(k)
                        }
                        None => Item::Lit(self.char_escape(false)?),
                    }
                }
                0x5E | 0x24 | 0x2A | 0x2B | 0x3F | 0x7B => return None,
                _ => Item::Lit(c),
            };
            let n0 = self.i;
            let (min, max) = if self.eat(b'*') {
                (0, None)
            } else if self.eat(b'+') {
                (1, None)
            } else if self.eat(b'?') {
                (0, Some(1))
            } else if self.eat(b'{') {
                let num = |p: &mut Parser| -> Option<u32> {
                    let st = p.i;
                    while p.peek().is_some_and(|d| (0x30..=0x39).contains(&d)) {
                        p.i += 1;
                    }
                    String::from_utf16(&p.s[st..p.i]).ok()?.parse().ok()
                };
                let lo = num(self)?;
                let hi = if self.eat(b',') {
                    if self.peek() == Some(b'}' as u16) { None } else { Some(num(self)?) }
                } else {
                    Some(lo)
                };
                if !self.eat(b'}') || hi.is_some_and(|h| h < lo) {
                    return None;
                }
                (lo, hi)
            } else {
                (1, Some(1))
            };
            let greedy = self.i == n0 || !self.eat(b'?');
            out.push(Quant { item, min, max, greedy });
        }
        Some(out)
    }
}

/// The derived part of an analysable regex literal (rule RT).
#[derive(Clone, Debug)]
pub(crate) struct Tail {
    /// The characters what precedes group k may end with (none in C), C,
    /// and `(C*)` (`true`) or `(C+)`.
    l: Class,
    c: Class,
    star: bool,
    /// Group k's number.
    k: u32,
    /// The capture groups numbered inside the tail.
    tail_groups: Vec<u32>,
    tail: Vec<Quant>,
    groups: u32,
}

/// The characters a match of `seq` may end with, and whether it may be
/// empty.
fn last_set(seq: &[Quant]) -> (Vec<(u16, u16)>, bool) {
    let mut acc = Vec::new();
    for q in seq.iter().rev() {
        let (ls, nl) = match &q.item {
            _ if q.max == Some(0) => (Vec::new(), true),
            Item::Lit(c) => (vec![(*c, *c)], false),
            Item::Class(k) => (k.0.clone(), false),
            Item::Group { alts, .. } => {
                let mut r = Vec::new();
                let mut nl = false;
                for a in alts {
                    let (l, n) = last_set(a);
                    r.extend(l);
                    nl |= n;
                }
                (r, nl)
            }
        };
        acc.extend(ls);
        if !(nl || q.min == 0) {
            return (acc, false);
        }
    }
    (acc, true)
}

fn tail_clean(q: &[Quant], out: &mut Vec<u32>) -> bool {
    q.iter().all(|q| {
        q.greedy
            && match &q.item {
                Item::Lit(_) | Item::Class(_) => true,
                Item::Group { cap, alts } => {
                    out.extend(*cap);
                    alts.len() == 1 && tail_clean(&alts[0], out)
                }
            }
    })
}

/// Rule RT: the tail analysis of pattern `src` with flags `flags`, or
/// `None` (no rule).
/// Rule M8, ECMA-262 22.2.6.13.1: `source` is EscapeRegExpPattern
/// (22.2.6.10) of [[OriginalSource]], as the runtime renders it
/// (perry-runtime regex/escape.rs, regex/perex_display.rs): "(?:)" for "",
/// a `/` outside a class as `\/`, a line terminator escaped, every other
/// code unit (and each `\` with the unit after it) as it is. So a
/// non-empty pattern with no `/` outside a class and outside an escape and
/// no line terminator is its own `source`.
pub(crate) fn source_is_pattern(p: &str) -> bool {
    if p.is_empty() {
        return false;
    }
    let (mut class, mut esc) = (false, false);
    for ch in p.chars() {
        if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            return false;
        }
        if esc {
            esc = false;
            continue;
        }
        match ch {
            '\\' => esc = true,
            '[' if !class => class = true,
            ']' if class => class = false,
            '/' if !class => return false,
            _ => {}
        }
    }
    true
}

impl Model {
    /// Rule M7: the pattern node `n` of the `new RegExp(n)` that made `t`.
    /// While `n` holds one String constant P and nothing else, `t`'s
    /// [[OriginalSource]] is P and its flags "" (22.2.3.2): its tail is
    /// `analyse(P, "")`, as a literal's. Any other content: no tail, and
    /// each `exec` record that took P's tail is index-merged. Monotone: one
    /// constant, then none.
    pub(crate) fn regexp_pattern(&mut self, n: NodeId, t: Tok) {
        let x = &self.nodes[n as usize];
        if x.prim == 0 && x.pts.is_empty() {
            return;
        }
        let one = self.one_str_const(n);
        match self.regexp_ctor_pat.get(&t) {
            Some(&(None, _)) => return,
            Some(&(a, _)) if a == one => return,
            Some(_) => {
                self.regexp_ctor_pat.insert(t, (None, None));
                for rec in self.regexp_ctor_recs.get(&t).cloned().unwrap_or_default() {
                    self.index_slots_merge(rec);
                }
                return;
            }
            None => {}
        }
        let pat = one.and_then(|a| analyse(&self.atoms[a as usize].clone(), "")).map(|tail| {
            let i = self.regexp_tails.len() as u32;
            self.regexp_tails.push(tail);
            i
        });
        self.regexp_ctor_pat.insert(t, (one, pat));
        for (rec, s) in self.regexp_pending.remove(&t).unwrap_or_default() {
            self.exec_attach(t, rec, pat, s);
        }
    }
}

pub(crate) fn analyse(src: &str, flags: &str) -> Option<Tail> {
    if !flags.chars().all(|f| f == 'd' || f == 'g') {
        return None;
    }
    let mut p = Parser {
        s: src.encode_utf16().collect(),
        i: 0,
        groups: 0,
    };
    let anchored = p.eat(b'^');
    let seq = p.seq()?;
    if !p.eat(b'$') || p.i != p.s.len() {
        return None;
    }
    for i in (1..seq.len()).rev() {
        let g = &seq[i];
        let Item::Group { cap: Some(k), alts } = &g.item else { continue };
        if (g.min, g.max) != (1, Some(1)) || alts.len() != 1 || alts[0].len() != 1 {
            continue;
        }
        let body = &alts[0][0];
        let c = match &body.item {
            Item::Class(c) => c.clone(),
            Item::Lit(x) => Class(vec![(*x, *x)]),
            Item::Group { .. } => continue,
        };
        let star = match (body.min, body.max, body.greedy) {
            (0, None, true) => true,
            (1, None, true) => false,
            _ => continue,
        };
        // What precedes group k ends with a character not in C, or is
        // empty only at the input's start (`^`).
        let (last, nullable) = last_set(&seq[..i]);
        let last = Class::norm(last);
        if (nullable && !anchored) || last.0.iter().any(|&(a, b)| c.0.iter().any(|&(x, y)| a <= y && x <= b)) {
            continue;
        }
        let mut tail_groups = Vec::new();
        if !tail_clean(&seq[i + 1..], &mut tail_groups) {
            return None;
        }
        return Some(Tail {
            l: last,
            c,
            star,
            k: *k,
            tail_groups,
            tail: seq[i + 1..].to_vec(),
            groups: p.groups,
        });
    }
    None
}

type Caps = Vec<Option<(usize, usize)>>;
/// A way needs a character left of the known suffix.
struct Escape;

fn back_seq(s: &[u16], seq: &[Quant], e: usize, caps: Caps, out: &mut Vec<(usize, Caps)>) -> Result<(), Escape> {
    let Some((last, rest)) = seq.split_last() else {
        out.push((e, caps));
        return Ok(());
    };
    let mut ws = Vec::new();
    back_quant(s, last, e, caps, &mut ws)?;
    for (p, c) in ws {
        back_seq(s, rest, p, c, out)?;
    }
    Ok(())
}

/// Every way `q` ends at `e`: the rightmost iteration is the last one, so
/// its captures stand; an iteration matching nothing is no rule
/// (`Escape`).
fn back_quant(s: &[u16], q: &Quant, e: usize, caps: Caps, out: &mut Vec<(usize, Caps)>) -> Result<(), Escape> {
    if q.min == 0 {
        out.push((e, caps.clone()));
    }
    let mut frontier = vec![(e, caps, 0u32)];
    while let Some((p, c, n)) = frontier.pop() {
        if q.max.is_some_and(|m| n >= m) {
            continue;
        }
        let mut ws = Vec::new();
        back_item(s, &q.item, p, c.clone(), &mut ws)?;
        for (p2, c2) in ws {
            if p2 == p {
                return Err(Escape);
            }
            let c3 = if n == 0 { c2 } else { c.clone() };
            if n + 1 >= q.min {
                out.push((p2, c3.clone()));
            }
            frontier.push((p2, c3, n + 1));
        }
    }
    Ok(())
}

fn back_item(s: &[u16], it: &Item, p: usize, caps: Caps, out: &mut Vec<(usize, Caps)>) -> Result<(), Escape> {
    match it {
        Item::Lit(_) | Item::Class(_) => {
            if p == 0 {
                return Err(Escape);
            }
            let ok = match it {
                Item::Lit(c) => s[p - 1] == *c,
                Item::Class(k) => k.has(s[p - 1]),
                _ => false,
            };
            if ok {
                out.push((p - 1, caps));
            }
        }
        Item::Group { cap, alts } => {
            let mut ws = Vec::new();
            back_seq(s, &alts[0], p, caps, &mut ws)?;
            for (st, mut c) in ws {
                if let Some(g) = cap {
                    c[*g as usize] = Some((st, p));
                }
                out.push((st, c));
            }
        }
    }
    Ok(())
}

/// One derived capture: the strings it may be and whether it may be
/// `undefined`; `None`: any string.
type Derived = Vec<(u32, Option<Vec<String>>, bool)>;

impl Tail {
    /// The groups rule RT derives: group k and the tail's groups.
    pub(crate) fn derived(&self) -> Vec<u32> {
        let mut g = vec![self.k];
        g.extend(&self.tail_groups);
        g
    }

    /// The captures of every way the input `X + sigma` (X any string)
    /// matches; `None` when a way reaches into X.
    fn ways(&self, sigma: &[u16]) -> Option<Vec<((usize, usize), Caps)>> {
        let mut ws = Vec::new();
        back_seq(sigma, &self.tail, sigma.len(), vec![None; self.groups as usize + 1], &mut ws).ok()?;
        let mut out = Vec::new();
        for (p, caps) in ws {
            let mut j = p;
            while j > 0 && self.c.has(sigma[j - 1]) {
                j -= 1;
            }
            if j == 0 {
                return None;
            }
            if !self.l.has(sigma[j - 1]) || (!self.star && j == p) {
                continue;
            }
            out.push(((j, p), caps));
        }
        Some(out)
    }

    /// Rule RT over the suffixes `sigmas`: per derived group, its strings
    /// (`None`: any string) and whether it may be `undefined`.
    pub(crate) fn captures(&self, sigmas: &[Vec<u16>]) -> Derived {
        let groups = self.derived();
        let mut acc: Vec<(u32, Option<Vec<String>>, bool)> = groups.iter().map(|&g| (g, Some(Vec::new()), false)).collect();
        for sigma in sigmas {
            let Some(ws) = self.ways(sigma) else {
                return groups.iter().map(|&g| (g, None, true)).collect();
            };
            for ((j, p), caps) in ws {
                for (g, strs, undef) in acc.iter_mut() {
                    let span = if *g == self.k { Some((j, p)) } else { caps[*g as usize] };
                    match span {
                        None => *undef = true,
                        Some((a, b)) => match (strs.as_mut(), String::from_utf16(&sigma[a..b])) {
                            (Some(v), Ok(t)) => v.push(t),
                            _ => *strs = None,
                        },
                    }
                }
            }
        }
        acc
    }
}

impl Model {
    /// Rule RT for the input node `n` of an `exec` whose receiver is the
    /// regex literal of tail `pat`: per derived group, the prim bits and
    /// strings its record slot holds. Each constant and each suffix entry
    /// is a known suffix (a constant is a suffix of itself, so this is a
    /// superset for it); any other value gives any string.
    pub(crate) fn capture_strs(&mut self, n: NodeId, pat: u32) -> Vec<(u32, u16, StrSet)> {
        let tail = self.regexp_tails[pat as usize].clone();
        let node = &self.nodes[n as usize];
        let any = |t: &Tail| t.derived().into_iter().map(|g| (g, prim::STR | prim::UNDEF, STRS_TOP)).collect();
        if node.prim == 0 && node.pts.is_empty() {
            return Vec::new();
        }
        if node.prim != prim::STR || !node.pts.is_empty() || node.strs < STRS_SET0 {
            return any(&tail);
        }
        let mut sigmas = Vec::new();
        for &a in self.str_atoms(node.strs) {
            let t = match self.atom_suffix(a) {
                Some(t) => t,
                None if self.atom_is_mark(a) => return any(&tail),
                None => self.atoms[a as usize].as_str(),
            };
            sigmas.push(t.encode_utf16().collect::<Vec<u16>>());
        }
        let mut out = Vec::new();
        for (g, strs, undef) in tail.captures(&sigmas) {
            let mut p = if undef { prim::UNDEF } else { 0 };
            let s = match strs {
                None => STRS_TOP,
                Some(v) => {
                    let mut atoms = Vec::new();
                    let mut top = false;
                    for t in v {
                        match self.str_const_atom(&t) {
                            Some(a) => atoms.push(a),
                            None => {
                                top = true;
                                break;
                            }
                        }
                    }
                    atoms.sort_unstable();
                    atoms.dedup();
                    match (top, atoms.is_empty()) {
                        (true, _) => STRS_TOP,
                        (false, true) => STRS_NONE,
                        (false, false) => self.str_set(&atoms),
                    }
                }
            };
            if s != STRS_NONE {
                p |= prim::STR;
            }
            out.push((g, p, s));
        }
        out
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    fn caps(src: &str, sigma: &str) -> Derived {
        analyse(src, "").expect("analysable").captures(&[sigma.encode_utf16().collect()])
    }

    #[test]
    fn simple_suffix() {
        let d = caps(r"^(?:([^.]*)\.)?([^.]+)$", ".position");
        assert_eq!(d, vec![(2, Some(vec!["position".to_string()]), false)]);
    }

    #[test]
    fn track_re() {
        let src = r"^((?:[^\[\]\.:\/]+[\/:])*)([^\[\]:\/]+)?(?:\.([^\[\]\.:\/]+)(?:\[(.+)\])?)?\.([^\[\]\.:\/]+)(?:\[(.+)\])?$";
        let d = caps(src, ".quaternion");
        assert_eq!(d, vec![(5, Some(vec!["quaternion".to_string()]), false), (6, Some(vec![]), true)]);
    }

    #[test]
    fn refused() {
        assert!(analyse(r"^(?:([^.]*)\.)?([^.]+)(a|b)$", "").is_none());
        assert!(analyse(r"^(.)\.([^.]+)\1$", "").is_none());
        assert!(analyse(r"^\.([^.]+)$", "i").is_none());
        // The C-run reaches the start of the known suffix: any string.
        assert_eq!(caps(r"^(?:([^.]*)\.)?([^.]+)$", "position"), vec![(2, None, true)]);
    }
}
