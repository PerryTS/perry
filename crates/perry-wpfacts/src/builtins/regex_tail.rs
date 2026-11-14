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
#[allow(dead_code)]
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
    /// The whole pattern (rule RH) and whether it begins with `^`.
    full: Vec<Quant>,
    anchored: bool,
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

/// Rule M9: the ASCII characters (bit c for c) of a pattern that is one
/// class or character, under flags `g` (and `d`) only: what a global
/// `replace` removes.
pub(crate) fn global_class(src: &str, flags: &str) -> Option<u128> {
    if !flags.contains('g') || !flags.chars().all(|f| f == 'g' || f == 'd') {
        return None;
    }
    let mut p = Parser {
        s: src.encode_utf16().collect(),
        i: 0,
        groups: 0,
    };
    let seq = p.seq()?;
    if p.i != p.s.len() {
        return None;
    }
    let [q] = seq.as_slice() else { return None };
    if (q.min, q.max) != (1, Some(1)) {
        return None;
    }
    let k = match &q.item {
        Item::Class(k) => k.clone(),
        Item::Lit(c) => Class(vec![(*c, *c)]),
        Item::Group { .. } => return None,
    };
    Some((0u16..128).filter(|&c| k.has(c)).fold(0, |m, c| m | 1u128 << c))
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
            full: seq.clone(),
            anchored,
        });
    }
    None
}

/// No rule for the input (kept for a pattern class the matcher refuses).
#[allow(dead_code)]
struct Escape;

/// Rule RH: where a backward match stands. `K(p)` before the known
/// suffix's code unit `p` (p > 0); `X(i)` inside the head X after `i`
/// steps that took a character of X (`X(0)`: the head's end, the known
/// suffix's start).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pos {
    K(usize),
    X(u32),
}

fn kp(p: usize) -> Pos {
    if p == 0 { Pos::X(0) } else { Pos::K(p) }
}

/// What the input's head (the part before its known suffix) may be.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Head {
    /// Empty: the input is the suffix (a constant).
    Empty,
    /// Any string with no ASCII character of the mask (0: any string).
    Excl(u128),
}

type FCaps = Vec<Option<(Pos, Pos)>>;

/// Rule RH, ECMA-262 22.2.2 / 22.2.7.2: every way the whole pattern
/// matches `X + s` (X described by `h`), matched backwards from the
/// input's end (`$`). A step at `X(i)` takes one character of X when the
/// atom admits a character X may hold; X is any such string, so taking
/// or not taking one is each a way. More iterations of a quantifier inside
/// X change no capture of a way (the last iteration's stand, RepeatMatcher
/// 22.2.2.3.1) and no "is it empty" of a span that crosses them, so one is
/// enough (finite).
struct Full<'a> {
    s: &'a [u16],
    h: Head,
}

impl Full<'_> {
    fn x_takes(&self, it: &Item) -> bool {
        let Head::Excl(e) = self.h else { return false };
        let free = |c: u16| c >= 128 || e & (1u128 << c) == 0;
        match it {
            Item::Lit(c) => free(*c),
            Item::Class(k) => k.0.iter().any(|&(a, b)| b >= 128 || (a..=b).any(free)),
            Item::Group { .. } => false,
        }
    }

    fn seq(&self, seq: &[Quant], e: Pos, caps: FCaps, out: &mut Vec<(Pos, FCaps)>) -> Result<(), Escape> {
        let Some((last, rest)) = seq.split_last() else {
            out.push((e, caps));
            return Ok(());
        };
        let mut ws = Vec::new();
        self.quant(last, e, caps, &mut ws)?;
        for (p, c) in ws {
            self.seq(rest, p, c, out)?;
        }
        Ok(())
    }

    fn quant(&self, q: &Quant, e: Pos, caps: FCaps, out: &mut Vec<(Pos, FCaps)>) -> Result<(), Escape> {
        if q.min == 0 {
            out.push((e, caps.clone()));
        }
        let mut frontier = vec![(e, caps, 0u32)];
        while let Some((p, c, n)) = frontier.pop() {
            if q.max.is_some_and(|m| n >= m) {
                continue;
            }
            let mut ws = Vec::new();
            self.item(&q.item, p, c.clone(), &mut ws)?;
            for (p2, c2) in ws {
                // RepeatMatcher 22.2.2.3.1 step 2.b: an iteration past the
                // minimum that matches nothing fails; one within it leaves
                // the position, so its count is bounded by the minimum.
                if p2 == p && n >= q.min {
                    continue;
                }
                let c3 = if n == 0 { c2 } else { c.clone() };
                if n + 1 >= q.min {
                    out.push((p2, c3.clone()));
                }
                let inside = matches!(p, Pos::X(_)) && matches!(p2, Pos::X(_));
                if !inside || n + 1 < q.min {
                    frontier.push((p2, c3, n + 1));
                }
            }
        }
        Ok(())
    }

    fn item(&self, it: &Item, p: Pos, caps: FCaps, out: &mut Vec<(Pos, FCaps)>) -> Result<(), Escape> {
        match it {
            Item::Lit(_) | Item::Class(_) => match p {
                Pos::K(p) => {
                    let ok = match it {
                        Item::Lit(c) => self.s[p - 1] == *c,
                        Item::Class(k) => k.has(self.s[p - 1]),
                        _ => false,
                    };
                    if ok {
                        out.push((kp(p - 1), caps));
                    }
                }
                Pos::X(i) => {
                    if self.x_takes(it) {
                        out.push((Pos::X(i + 1), caps));
                    }
                }
            },
            Item::Group { cap, alts } => {
                for alt in alts {
                    let mut ws = Vec::new();
                    self.seq(alt, p, caps.clone(), &mut ws)?;
                    for (st, mut c) in ws {
                        if let Some(g) = cap {
                            c[*g as usize] = Some((st, p));
                        }
                        out.push((st, c));
                    }
                }
            }
        }
        Ok(())
    }
}

/// One derived capture: the strings it may be and whether it may be
/// `undefined`; `None`: any string.
type Derived = Vec<(u32, Option<Vec<String>>, bool)>;

impl Tail {
    /// The groups rules RT/RH derive: every capture group.
    pub(crate) fn derived(&self) -> Vec<u32> {
        (1..=self.groups).collect()
    }

    /// Rule RH over the inputs `entries` (a head and a known suffix each):
    /// per group, the union over every way of every entry of its strings
    /// (`None`: any string) and whether it may be `undefined`. No way for
    /// an entry: `exec` returns null for it (it adds nothing). A span that
    /// takes characters of the head is any string; one inside the head
    /// that takes none is "".
    pub(crate) fn captures(&self, entries: &[(Head, Vec<u16>)]) -> Derived {
        let groups = self.derived();
        let mut acc: Derived = groups.iter().map(|&g| (g, Some(Vec::new()), false)).collect();
        for (h, sigma) in entries {
            let f = Full { s: sigma, h: *h };
            let mut ws = Vec::new();
            if f.seq(&self.full, kp(sigma.len()), vec![None; self.groups as usize + 1], &mut ws).is_err() {
                return groups.iter().map(|&g| (g, None, true)).collect();
            }
            for (st, caps) in ws {
                // `^` holds only at the input's start (22.2.2.6).
                if self.anchored && matches!(st, Pos::K(_)) {
                    continue;
                }
                for (g, strs, undef) in acc.iter_mut() {
                    let at = |p: Pos| match p {
                        Pos::K(p) => Some(p),
                        Pos::X(0) => Some(0),
                        Pos::X(_) => None,
                    };
                    match caps[*g as usize] {
                        None => *undef = true,
                        Some((a, b)) => {
                            let t = match (at(a), at(b)) {
                                (Some(a), Some(b)) => String::from_utf16(&sigma[a..b]).ok(),
                                _ if a == b => Some(String::new()),
                                _ => None,
                            };
                            match (strs.as_mut(), t) {
                                (Some(v), Some(t)) => v.push(t),
                                _ => *strs = None,
                            }
                        }
                    }
                }
            }
        }
        for (_, strs, _) in acc.iter_mut() {
            if let Some(v) = strs.as_mut() {
                v.sort();
                v.dedup();
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
        let mut entries = Vec::new();
        for &a in self.str_atoms(node.strs) {
            let (h, t) = match (self.atom_suffix(a), self.atom_excl(a)) {
                (Some(t), _) => (Head::Excl(0), t),
                (_, Some((e, t))) => (Head::Excl(e), t),
                _ if self.atom_is_mark(a) => return any(&tail),
                _ => (Head::Empty, self.atoms[a as usize].as_str()),
            };
            entries.push((h, t.encode_utf16().collect::<Vec<u16>>()));
        }
        let mut out = Vec::new();
        for (g, strs, undef) in tail.captures(&entries) {
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
        let t = analyse(src, "").expect("analysable");
        let mut d = t.captures(&[(Head::Excl(0), sigma.encode_utf16().collect())]);
        d.retain(|x| x.0 >= t.k);
        d
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
    fn global_class_masks() {
        let m = |s: &str| s.chars().fold(0u128, |m, c| m | 1u128 << c as u32);
        assert_eq!(global_class(r"[\[\]\.:\/]", "g"), Some(m("[].:/")));
        assert_eq!(global_class(r"\s", "g"), Some(m(" \t\n\u{b}\u{c}\r")));
        assert_eq!(global_class(r"[\[\]\.:\/]", ""), None);
        assert_eq!(global_class(r"[ab]c", "g"), None);
    }

    #[test]
    fn refused() {
        assert!(analyse(r"^(?:([^.]*)\.)?([^.]+)(a|b)$", "").is_none());
        assert!(analyse(r"^(.)\.([^.]+)\1$", "").is_none());
        assert!(analyse(r"^\.([^.]+)$", "i").is_none());
        // The C-run reaches the start of the known suffix: any string.
        assert_eq!(caps(r"^(?:([^.]*)\.)?([^.]+)$", "position"), vec![(2, None, false)]);
    }
}
