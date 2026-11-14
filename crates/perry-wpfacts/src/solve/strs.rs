//! The string constants a node holds (`Node::strs`, DESIGN SEEDKV): one
//! value lattice beside `prim`. A string literal is a source node holding
//! its one atom (`Gen::str_literal`); copy edges and the filters that pass a
//! string join sets; every other string birth is any string (`STRS_TOP`),
//! the default of `add_prim`. A set past `STRS_K` atoms is any string.
use crate::model::*;

/// The spelling of a rule-M3 prefix entry: `\u{1}...` is never a program
/// constant (`str_const_atom`).
pub(crate) const PREFIX_MARK: &str = "\u{1}prefix\u{1}";
/// Past this many `c + d` prefixes one `+` keeps the left constants only.
/// The spelling of a rule-K7 infix entry, `INFIX_MARK` + `c`: "a String
/// that contains the ASCII character `c`", `c` no IdentifierPart (ECMA-262
/// 12.7: not `[A-Za-z0-9_$]`), so no built-in member name contains it.
pub(crate) const INFIX_MARK: &str = "\u{1}infix\u{1}";
/// The spelling of a rule-M4 suffix entry, `SUFFIX_MARK` + `s`: "a String
/// that ends with `s`" (`s` not empty).
pub(crate) const SUFFIX_MARK: &str = "\u{1}suffix\u{1}";

/// The end of every NativeFunction text perry's runtime prints for a
/// builtin function (`function_source_for_func_ptr`, the class and closure
/// forms): `function NAME() { [native code] }` (ECMA-262 20.2.3.5).
pub(crate) const NATIVE_CODE_TAIL: &str = "[native code] }";
/// The spelling of an exclusion entry, `EXCL_MARK` + 32 hex digits (a set
/// E of ASCII characters, bit c for c) + `\u{1}` + `s`: "a String `X + s`
/// where X holds no character of E" (`s` may be empty; it may hold E's
/// characters).
pub(crate) const EXCL_MARK: &str = "\u{1}excl\u{1}";
const PLUS_PREFIXES_K: usize = 256;
/// Every character of a Number's text (ECMA-262 6.1.6.1.20: digits, the
/// sign and exponent characters, "NaN", "Infinity").
const NUM_TEXT_CHARS: &str = "0123456789+-.eNaInfity";
/// Every character of an integer's text below 10^21, and of a BigInt's.
const INT_TEXT_CHARS: &str = "0123456789-";
/// The ASCII letters: the only ASCII characters a case mapping produces
/// from, or maps to, another code point (Unicode Default Case Conversion:
/// letters map to letters and combining marks).
const ASCII_LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The ASCII characters of `s` (bit c for c).
pub(crate) fn ascii_mask(s: &str) -> u128 {
    s.chars().filter(|c| c.is_ascii()).fold(0, |m, c| m | 1u128 << c as u32)
}

impl Model {
    /// The interned set of `atoms` (sorted, distinct; `STRS_TOP` past
    /// `STRS_K`).
    pub(crate) fn str_set(&mut self, atoms: &[Atom]) -> StrSet {
        if atoms.is_empty() {
            return STRS_NONE;
        }
        if atoms.len() > STRS_K {
            return STRS_TOP;
        }
        if let Some(&s) = self.str_set_index.get(atoms) {
            return s;
        }
        let s = STRS_SET0 + self.str_sets.len() as StrSet;
        let b: Box<[Atom]> = atoms.into();
        self.str_sets.push(b.clone());
        self.str_set_index.insert(b, s);
        s
    }

    /// The atoms of a finite set (empty for none and ⊤).
    pub(crate) fn str_atoms(&self, s: StrSet) -> &[Atom] {
        if s < STRS_SET0 {
            return &[];
        }
        &self.str_sets[(s - STRS_SET0) as usize]
    }

    /// The join (union) of two sets.
    pub(crate) fn strs_join(&mut self, a: StrSet, b: StrSet) -> StrSet {
        if a == b || b == STRS_NONE || a == STRS_TOP {
            return a;
        }
        if a == STRS_NONE || b == STRS_TOP {
            return b;
        }
        let x = self.str_atoms(a);
        let y = self.str_atoms(b);
        let mut v: Vec<Atom> = Vec::with_capacity(x.len() + y.len());
        let (mut i, mut j) = (0, 0);
        while i < x.len() || j < y.len() {
            let next = match (x.get(i), y.get(j)) {
                (Some(&p), Some(&q)) if p == q => {
                    i += 1;
                    j += 1;
                    p
                }
                (Some(&p), Some(&q)) if p < q => {
                    i += 1;
                    p
                }
                (Some(_), Some(&q)) => {
                    j += 1;
                    q
                }
                (Some(&p), None) => {
                    i += 1;
                    p
                }
                (None, Some(&q)) => {
                    j += 1;
                    q
                }
                (None, None) => unreachable!(),
            };
            v.push(next);
            if v.len() > STRS_K {
                return STRS_TOP;
            }
        }
        self.str_set(&v)
    }

    /// Joins `s` into `n`'s strings; true when they grew.
    pub(crate) fn join_strs(&mut self, n: NodeId, s: StrSet) -> bool {
        let old = self.nodes[n as usize].strs;
        let new = self.strs_join(old, s);
        if new == old {
            return false;
        }
        let node = &mut self.nodes[n as usize];
        node.strs = new;
        node.dstrs = true;
        true
    }

    /// Rule M3: the entry for "a string that starts with `p`" (a solver
    /// spelling: no program constant is spelled so, `str_const_atom`).
    pub(crate) fn prefix_atom(&mut self, p: &str) -> Atom {
        self.atom(&format!("{PREFIX_MARK}{p}"))
    }

    /// The known prefix an entry stands for (`None`: a constant).
    pub(crate) fn atom_prefix(&self, a: Atom) -> Option<&str> {
        self.atoms[a as usize].strip_prefix(PREFIX_MARK)
    }

    /// Rule K7: the entry for "a String that contains `c`".
    pub(crate) fn infix_atom(&mut self, c: char) -> Atom {
        self.atom(&format!("{INFIX_MARK}{c}"))
    }

    /// The suffix entry of `s`.
    pub(crate) fn suffix_atom(&mut self, s: &str) -> Atom {
        self.atom(&format!("{SUFFIX_MARK}{s}"))
    }

    /// The known suffix of a suffix entry (`None`: no suffix entry).
    pub(crate) fn atom_suffix(&self, a: Atom) -> Option<&str> {
        self.atoms[a as usize].strip_prefix(SUFFIX_MARK)
    }

    /// The exclusion entry of `e` and `s`; with no excluded character a
    /// suffix entry (`None` for `s` empty: any string).
    pub(crate) fn excl_atom(&mut self, e: u128, s: &str) -> Option<Atom> {
        if e == 0 {
            return (!s.is_empty()).then(|| self.suffix_atom(s));
        }
        Some(self.atom(&format!("{EXCL_MARK}{e:032x}\u{1}{s}")))
    }

    /// The excluded set and the known suffix of an exclusion entry.
    pub(crate) fn atom_excl(&self, a: Atom) -> Option<(u128, &str)> {
        let r = self.atoms[a as usize].strip_prefix(EXCL_MARK)?;
        let e = u128::from_str_radix(r.get(..32)?, 16).ok()?;
        Some((e, r.get(33..)?))
    }

    /// The character an infix entry stands for (`None`: no infix entry).
    pub(crate) fn atom_infix(&self, a: Atom) -> Option<char> {
        self.atoms[a as usize].strip_prefix(INFIX_MARK).and_then(|r| r.chars().next())
    }

    /// Is the entry a solver spelling (a prefix or an infix), not a
    /// program constant (`str_const_atom`)?
    pub(crate) fn atom_is_mark(&self, a: Atom) -> bool {
        self.atoms[a as usize].starts_with('\u{1}')
    }

    /// Rule K7: a character no IdentifierPart that the entry surely holds
    /// (a constant or a known prefix holding one, or an infix entry).
    fn atom_sure_char(&self, a: Atom) -> Option<char> {
        if let Some(c) = self.atom_infix(a) {
            return Some(c);
        }
        let t = self
            .atom_prefix(a)
            .or_else(|| self.atom_suffix(a))
            .or_else(|| self.atom_excl(a).map(|x| x.1))
            .unwrap_or(&self.atoms[a as usize]);
        t.chars().find(|&c| c.is_ascii() && !c.is_ascii_alphanumeric() && c != '_' && c != '$')
    }

    /// Rule K7: when node `n` holds only Strings, each surely holding a
    /// character no IdentifierPart, the infix entries of its strings.
    pub(crate) fn infix_strs(&mut self, n: NodeId) -> Option<StrSet> {
        let node = &self.nodes[n as usize];
        if node.prim != prim::STR || !node.pts.is_empty() || node.strs < STRS_SET0 {
            return None;
        }
        let mut cs: Vec<char> = Vec::new();
        for &a in self.str_atoms(node.strs) {
            cs.push(self.atom_sure_char(a)?);
        }
        cs.sort_unstable();
        cs.dedup();
        let mut out: Vec<Atom> = cs.into_iter().map(|c| self.infix_atom(c)).collect();
        out.sort_unstable();
        Some(self.str_set(&out))
    }

    /// Rule K7, ECMA-262 22.1.3.28 / .29: `toLowerCase` / `toUpperCase`
    /// (`upper`) of receiver `n` map each code point of ToString(receiver)
    /// on its own, and an ASCII code point by the ASCII case map whatever
    /// follows it. So a String-only receiver (ToString is the identity) of
    /// a known set maps each entry exactly: an ASCII constant to its mapped
    /// constant, a known ASCII prefix to its mapped prefix, an infix entry
    /// (an ASCII character no letter) to itself. A set with any other entry
    /// keeps its infix entries (`infix_strs`); any other receiver gives any
    /// string.
    pub(crate) fn case_map_strs(&mut self, n: NodeId, upper: bool) -> StrSet {
        let node = &self.nodes[n as usize];
        let (p, bare, s) = (node.prim, node.pts.is_empty(), node.strs);
        if p == prim::STR && bare && s >= STRS_SET0 {
            let map = |t: &str| if upper { t.to_ascii_uppercase() } else { t.to_ascii_lowercase() };
            let mut out: Vec<Atom> = Vec::new();
            let mut exact = true;
            for a in self.str_atoms(s).to_vec() {
                if self.atom_infix(a).is_some() {
                    out.push(a);
                    continue;
                }
                // ECMA-262 22.1.3.28 / .30: no code point maps to an ASCII
                // character other than a letter, and none of those maps to
                // something else: X·s keeps X's excluded non-letters.
                if let Some((e, t)) = self.atom_excl(a) {
                    let m = if t.is_ascii() {
                        let t = map(t);
                        self.excl_atom(e & !ascii_mask(ASCII_LETTERS), &t)
                    } else {
                        None
                    };
                    match m {
                        Some(m) => {
                            out.push(m);
                            continue;
                        }
                        None => {
                            exact = false;
                            break;
                        }
                    }
                }
                let m = match self.atom_prefix(a) {
                    Some(q) if q.is_ascii() => Some(self.prefix_atom(&map(q))),
                    Some(_) => None,
                    None if self.atom_is_mark(a) => None,
                    None => match self.atoms[a as usize].clone() {
                        t if t.is_ascii() => self.str_const_atom(&map(&t)),
                        _ => None,
                    },
                };
                match m {
                    Some(m) => out.push(m),
                    None => {
                        exact = false;
                        break;
                    }
                }
            }
            if exact {
                out.sort_unstable();
                out.dedup();
                return self.str_set(&out);
            }
        }
        self.infix_strs(n).unwrap_or(STRS_TOP)
    }

    /// May the set hold a string that is no constant of it (⊤, or a known
    /// prefix)? Every reader of constants asks this, not `== STRS_TOP`.
    pub(crate) fn strs_any(&self, s: StrSet) -> bool {
        s == STRS_TOP || self.str_atoms(s).iter().any(|&a| self.atom_is_mark(a))
    }

    /// The key shape of one entry: its exact name, or the glob `p*` of a
    /// known prefix; 0 (any key) when it may spell a Number key.
    pub(crate) fn atom_key_shape(&mut self, a: Atom) -> KeyShapeId {
        if self.atom_infix(a).is_some() {
            // Rule K7: as a store key an infix entry is any key.
            return 0;
        }
        if let Some((_, s)) = self.atom_excl(a) {
            if s.is_empty() {
                return 0;
            }
            let glob = vec![String::new(), s.to_string()];
            return self.key_shape(Some(KeyShape { globs: vec![glob] }));
        }
        let glob = match (self.atom_prefix(a), self.atom_suffix(a)) {
            (Some(p), _) => vec![p.to_string(), String::new()],
            (_, Some(s)) => vec![String::new(), s.to_string()],
            _ => vec![self.atoms[a as usize].clone()],
        };
        self.key_shape(Some(KeyShape { globs: vec![glob] }))
    }

    /// Rule M3 for a computed key node: when it holds Strings, each a
    /// constant, of a known prefix or suffix or an infix entry, the shape of
    /// each entry (ECMA-262 7.1.19: a String key is itself, so a set of
    /// constants names exactly those keys); else `None` (the site's own
    /// read). Rule K8 (ECMA-262
    /// 7.1.19, 7.1.17): beside the Strings it may hold `undefined`, `null`
    /// and booleans, whose keys are the named "undefined", "null", "true"
    /// and "false" the `IdxKey` rule reads for them.
    pub(crate) fn key_entry_shapes(&mut self, n: NodeId) -> Option<Vec<(Atom, KeyShapeId)>> {
        let node = &self.nodes[n as usize];
        let named = prim::STR | prim::BOOL | prim::NULL | prim::UNDEF;
        if node.prim & prim::STR == 0
            || node.prim & !named != 0
            || !node.pts.is_empty()
            || node.strs < STRS_SET0
        {
            return None;
        }
        let atoms = self.str_atoms(node.strs).to_vec();
        let mut out = Vec::with_capacity(atoms.len());
        for a in atoms {
            // Rule K7: an infix entry reads the names of glob `*c*`.
            if let Some(c) = self.atom_infix(a) {
                out.push((a, self.infix_shape(c)));
                continue;
            }
            // A known prefix reads the names of its glob `p*`, and the
            // elements and numeric keys only when `p` may begin ToString of
            // a Number (`glob_may_be_numeric`): never any key.
            if let Some(p) = self.atom_prefix(a) {
                let glob = vec![p.to_string(), String::new()];
                out.push((a, self.glob_shape(glob)));
                continue;
            }
            // Rule M4: a known suffix reads the names of its glob `*s`
            // (an exclusion entry's suffix too).
            if let Some(s) = self.atom_suffix(a).or_else(|| self.atom_excl(a).map(|x| x.1).filter(|s| !s.is_empty())) {
                let glob = vec![String::new(), s.to_string()];
                out.push((a, self.glob_shape(glob)));
                continue;
            }
            match self.atom_key_shape(a) {
                0 => return None,
                s => out.push((a, s)),
            }
        }
        Some(out)
    }

    /// Rule M3, ECMA-262 13.15.3: the strings `l + r` may be. A String left
    /// operand `c` (a constant) gives strings that start with `c` followed
    /// by ToString of `r`: `c + d` for each constant or known prefix `d` of
    /// a String-only `r`, else `c`; a left prefix stays the prefix. A left
    /// operand that may be no String (a Number, an object's ToPrimitive)
    /// while the result may be a String gives any string. Prefixes are
    /// only made from constants, never extended, so loops end.
    ///
    /// `.1`: the result used a text that holds only while no unknown code
    /// exists (rule M8's builtin-function form, `excl_parts`).
    pub(crate) fn plus_strs(&mut self, l: NodeId, r: NodeId) -> (StrSet, bool) {
        if let Some(s) = self.plus_const(l, r) {
            return (s, false);
        }
        let m3 = self.plus_strs_m3(l, r);
        if m3 != STRS_TOP {
            return (m3, false);
        }
        if let Some(s) = self.excl_plus(l, r) {
            return s;
        }
        if let Some(s) = self.suffix_strs(r) {
            return (s, false);
        }
        // Rule K7, 13.15.3: when one operand is String-only, the result is
        // the concatenation of both ToStrings, so it holds every character
        // that operand surely holds.
        match self.infix_strs(l).or_else(|| self.infix_strs(r)) {
            Some(s) => (s, false),
            None => (STRS_TOP, false),
        }
    }

    /// The values of `n` as concatenation parts (ECMA-262 13.15.3 step 4:
    /// each operand's ToString, 7.1.17): each constant (`Ok`) or exclusion
    /// entry (`Err((E, s))`; a suffix entry is one with nothing excluded);
    /// `None` when one value's text may be any string. `undefined`, `null`
    /// and the booleans are their constant texts; a Symbol throws (no
    /// text). A builtin function is ToPrimitive (7.1.1: no @@toPrimitive,
    /// `Object.prototype.valueOf` gives the object, then
    /// `Function.prototype.toString`, 20.2.3.5): a NativeFunction text,
    /// which perry's runtime always spells ending in `[native code] }`,
    /// while no unknown code exists and the program writes none of those
    /// keys on it or the prototypes; `.1` says such a form was used (it
    /// is any string once the realm is held, `realm_str_dsts`). A Number
    /// or a BigInt is an exclusion entry of the characters its text never
    /// holds (rule NUM-TEXT). `Object.prototype` (the token is every
    /// builtin prototype) and every other object may be any string.
    fn excl_parts(&mut self, n: NodeId) -> Option<(Vec<Result<String, (u128, String)>>, bool)> {
        let (p, toks, strs, rg) = {
            let x = &self.nodes[n as usize];
            (x.prim, x.pts.to_sorted(), x.strs, x.range)
        };
        let mut out = Vec::new();
        // Rule NUM-TEXT, ECMA-262 6.1.6.1.20 Number::toString(x, 10): the
        // text is "NaN", "Infinity", "-Infinity" or digits with `-`, `.`,
        // `e` and `+`; for an integer of magnitude below 10^21 digits and
        // `-` alone. The interval (`range::Iv`, every Number an integer,
        // never NaN or an infinity) within ±2^53 says the latter.
        // 6.1.6.2.14 BigInt::toString: digits and `-`.
        if p & prim::NUM != 0 {
            let int = !rg.is_empty() && rg.lo >= -(1i64 << 53) && rg.hi <= 1i64 << 53;
            out.push(Err((!ascii_mask(if int { INT_TEXT_CHARS } else { NUM_TEXT_CHARS }), String::new())));
        }
        if p & prim::BIGINT != 0 {
            out.push(Err((!ascii_mask(INT_TEXT_CHARS), String::new())));
        }
        let mut gated = false;
        if p & prim::STR != 0 {
            if strs < STRS_SET0 {
                return None;
            }
            for &a in self.str_atoms(strs) {
                match self.atom_excl(a).or_else(|| self.atom_suffix(a).map(|s| (0, s))) {
                    Some((e, s)) => out.push(Err((e, s.to_string()))),
                    None if self.atom_is_mark(a) => return None,
                    None => out.push(Ok(self.atoms[a as usize].clone())),
                }
            }
        }
        for (bit, text) in [
            (prim::UNDEF, "undefined"),
            (prim::NULL, "null"),
            (prim::TRUE, "true"),
            (prim::FALSE, "false"),
        ] {
            if p & bit != 0 {
                out.push(Ok(text.to_string()));
            }
        }
        if !toks.is_empty() {
            if self.realm_held {
                return None;
            }
            let fp = crate::builtins::native_proto(self, "Function");
            let keys = [crate::model::WK_TO_PRIMITIVE, "valueOf", "toString"].map(|k| self.atom(k));
            for t in toks {
                let native_fn = t != TOP
                    && !self.is_marker(t)
                    && match self.toks[t as usize] {
                        TokKind::BuiltinFn(_) => true,
                        TokKind::Builtin(_) => crate::builtins::surely_callable(self, t),
                        _ => false,
                    };
                if !native_fn {
                    return None;
                }
                for &a in &keys {
                    let f = self.field(t, a);
                    let x = &self.nodes[f as usize];
                    if crate::builtins::native_written(self, fp, a) || x.prim != 0 || !x.pts.is_empty() {
                        return None;
                    }
                }
            }
            out.push(Err((0, NATIVE_CODE_TAIL.to_string())));
            gated = true;
        }
        Some((out, gated))
    }

    /// Rule M8, ECMA-262 13.15.3 (two Strings give their concatenation):
    /// with an exclusion entry on either side and only constants and
    /// exclusion entries on both, `X·s + t` is `X·(s t)`; `c + X·s` is
    /// `(c X)·s`, whose head holds no character of E that `c` lacks;
    /// `X1·s1 + X2·s2` is `(X1 s1 X2)·s2`, whose head holds no character
    /// excluded on both sides and absent from `s1`; `X·s + t` of a left
    /// operand with more values is `(X s)·t` likewise. Exact for the first,
    /// a superset for the others. Suffixes grow only from a left entry
    /// with an empty suffix or from the left operand's one value (a node
    /// holds one value alone during one stretch at most, sets only grow),
    /// so loops end; two constants are rule M5's.
    fn excl_plus(&mut self, l: NodeId, r: NodeId) -> Option<(StrSet, bool)> {
        let (lp, lg) = self.excl_parts(l)?;
        let (rp, rg) = self.excl_parts(r)?;
        if lp.iter().chain(&rp).all(|p| p.is_ok()) {
            return None;
        }
        let alone = lp.len() == 1;
        let mut out = Vec::new();
        for a in &lp {
            for b in &rp {
                let at = match (a, b) {
                    (Err((e, s)), Ok(t)) if s.is_empty() || alone => self.excl_atom(*e, &format!("{s}{t}")),
                    (Err((e, s)), Ok(t)) => self.excl_atom(e & !ascii_mask(s), t),
                    (Ok(c), Err((e, s))) => self.excl_atom(e & !ascii_mask(c), s),
                    (Err((e1, s1)), Err((e2, s2))) => self.excl_atom(e1 & e2 & !ascii_mask(s1), s2),
                    _ => None,
                };
                out.push(at?);
            }
        }
        out.sort_unstable();
        out.dedup();
        Some((self.str_set(&out), lg || rg))
    }

    /// Is `t` an object of the RegExp family (a literal's or a
    /// constructor's)?
    fn is_regexp_tok(&self, t: Tok) -> bool {
        t != TOP
            && matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                if matches!(self.tmpls[tm as usize].kind, TmplKind::BObj { what: "RegExp", .. }))
    }

    /// Rule M9, ECMA-262 22.2.6.11 RegExp.prototype[@@replace] with a
    /// global regex `t` whose pattern is one class (or character) C and a
    /// String replacement `r` with no `$` (GetSubstitution is `r` itself)
    /// and no ASCII character of C: every match is one C character, and
    /// each one is replaced, so the result holds no ASCII character of C
    /// (an exclusion entry). Only while no unknown code exists and nothing
    /// writes what @@replace reads (`REGEXP_REPLACE_READS`) on
    /// RegExp.prototype or `t`; else any string. `None` while an operand
    /// holds no value yet.
    pub(crate) fn class_replace_strs(&mut self, t: Tok, repl: NodeId) -> Option<StrSet> {
        let empty = |m: &Self, n: NodeId| {
            let x = &m.nodes[n as usize];
            x.prim == 0 && x.pts.is_empty()
        };
        if empty(self, repl) {
            return None;
        }
        if self.realm_held {
            return Some(STRS_TOP);
        }
        let lit = match self.toks[t as usize] {
            TokKind::Obj(tm, _) => self.regexp_class_of.get(&tm).copied(),
            _ => None,
        };
        let mask = match (lit, self.regexp_ctor_args.get(&t).copied()) {
            (Some(mk), _) => mk,
            (None, Some((p, f))) => {
                if empty(self, p) || empty(self, f) {
                    return None;
                }
                let (Some(pa), Some(fa)) = (self.one_str_const(p), self.one_str_const(f)) else {
                    return Some(STRS_TOP);
                };
                let (ps, fs) = (self.atoms[pa as usize].clone(), self.atoms[fa as usize].clone());
                match crate::builtins::regex_tail::global_class(&ps, &fs) {
                    Some(mk) => mk,
                    None => return Some(STRS_TOP),
                }
            }
            _ => return Some(STRS_TOP),
        };
        let np = crate::builtins::native_proto(self, "RegExp");
        for key in crate::builtins::string_methods::REGEXP_REPLACE_READS {
            let a = self.atom(key);
            let f = self.field(t, a);
            if crate::builtins::native_written(self, np, a) || !empty(self, f) {
                return Some(STRS_TOP);
            }
        }
        let Some(r) = self.one_str_const(repl).map(|a| self.atoms[a as usize].clone()) else {
            return Some(STRS_TOP);
        };
        if r.contains('$') || ascii_mask(&r) & mask != 0 {
            return Some(STRS_TOP);
        }
        match self.excl_atom(mask, "") {
            Some(a) => Some(self.str_set(&[a])),
            None => Some(STRS_TOP),
        }
    }

    /// Rule M4, ECMA-262 13.15.3: when the right operand `r` holds only
    /// Strings, each a non-empty constant or a known suffix, `l + r` is
    /// ToString(l) followed by one of them: it ends with it.
    fn suffix_strs(&mut self, r: NodeId) -> Option<StrSet> {
        let node = &self.nodes[r as usize];
        if node.prim != prim::STR || !node.pts.is_empty() || node.strs < STRS_SET0 {
            return None;
        }
        let mut out = Vec::new();
        for a in self.str_atoms(node.strs).to_vec() {
            if self.atom_suffix(a).is_some() {
                out.push(a);
            } else if let Some((_, s)) = self.atom_excl(a).filter(|x| !x.1.is_empty()) {
                let s = s.to_string();
                out.push(self.suffix_atom(&s));
            } else if self.atom_is_mark(a) || self.atoms[a as usize].is_empty() {
                return None;
            } else {
                let c = self.atoms[a as usize].clone();
                out.push(self.suffix_atom(&c));
            }
        }
        out.sort_unstable();
        out.dedup();
        Some(self.str_set(&out))
    }

    /// The one constant a String-only node holds when it holds exactly one
    /// (no prefix, infix or suffix entry, no other value).
    pub(crate) fn one_str_const(&self, n: NodeId) -> Option<Atom> {
        self.one_str_const_but(n, 0)
    }

    /// `one_str_const` of the node's values other than the primitives
    /// `but` (values the caller's operation throws on).
    pub(crate) fn one_str_const_but(&self, n: NodeId, but: u16) -> Option<Atom> {
        let x = &self.nodes[n as usize];
        if x.prim & !but != prim::STR || !x.pts.is_empty() || x.strs < STRS_SET0 {
            return None;
        }
        match self.str_atoms(x.strs) {
            &[a] if !self.atom_is_mark(a) => Some(a),
            _ => None,
        }
    }

    /// Rule M5, ECMA-262 13.15.3 (ApplyStringOrNumericBinaryOperator: two
    /// Strings give their concatenation): a left operand that holds only
    /// the String `c` and a right operand that holds only the String `d`
    /// give the one String `cd`. Finite with no bound: sets only grow, so a
    /// node holds one constant alone during one stretch at most, and each
    /// `+` makes at most one constant by this rule; every other pair is
    /// rule M3's (prefixes) and M4's (suffixes).
    fn plus_const(&mut self, l: NodeId, r: NodeId) -> Option<StrSet> {
        let (a, b) = (self.one_str_const(l)?, self.one_str_const(r)?);
        let s = format!("{}{}", self.atoms[a as usize], self.atoms[b as usize]);
        let c = self.str_const_atom(&s)?;
        Some(self.str_set(&[c]))
    }

    /// Rule M6, ECMA-262 22.1.3.19 `String.prototype.replace(searchValue,
    /// replaceValue)` on a String receiver with a String searchValue (a
    /// primitive has no own @@replace; a program @@replace on
    /// String.prototype is the `sym_call` result beside this) and a String
    /// replaceValue (not callable): the first occurrence of the search
    /// string (StringIndexOf from 0) replaced by GetSubstitution
    /// (22.1.3.19.1 with no captures and namedCaptures undefined: `$$`,
    /// `$&`, `` $` ``, `$'` substitute; every other `$` is itself); no
    /// occurrence gives the receiver. Exact when each operand holds one
    /// ASCII constant (code units are bytes); else any string. `None` while
    /// an operand holds no value yet (its arrival runs the rule again).
    pub(crate) fn replace_strs(&mut self, recv: NodeId, search: NodeId, repl: NodeId) -> Option<StrSet> {
        if [recv, search, repl].iter().any(|&n| {
            let x = &self.nodes[n as usize];
            x.prim == 0 && x.pts.is_empty()
        }) {
            return None;
        }
        // Step 2: a search value of RegExp objects only is their
        // @@replace's call (rule M9, `sym_call`), not this rule's.
        let sx = &self.nodes[search as usize];
        if sx.prim == 0 && sx.pts.to_sorted().into_iter().all(|t| self.is_regexp_tok(t)) {
            return None;
        }
        // Step 1 RequireObjectCoercible: an undefined or null receiver
        // throws, it gives no result.
        let one = |m: &Self, n: NodeId, but: u16| {
            m.one_str_const_but(n, but).map(|a| m.atoms[a as usize].clone()).filter(|s| s.is_ascii())
        };
        let nullish = prim::UNDEF | prim::NULL;
        let (Some(s), Some(q), Some(r)) = (one(self, recv, nullish), one(self, search, 0), one(self, repl, 0)) else {
            return Some(STRS_TOP);
        };
        let out = match s.find(q.as_str()) {
            None => s,
            Some(p) => {
                let (head, rest) = (&s[..p], &s[p + q.len()..]);
                let mut o = String::from(head);
                let b = r.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    if b[i] == b'$' && i + 1 < b.len() {
                        let sub = match b[i + 1] {
                            b'$' => Some("$"),
                            b'&' => Some(q.as_str()),
                            b'`' => Some(head),
                            b'\'' => Some(rest),
                            _ => None,
                        };
                        if let Some(t) = sub {
                            o.push_str(t);
                            i += 2;
                            continue;
                        }
                    }
                    o.push(b[i] as char);
                    i += 1;
                }
                o.push_str(rest);
                o
            }
        };
        Some(match self.str_const_atom(&out) {
            Some(a) => self.str_set(&[a]),
            None => STRS_TOP,
        })
    }

    fn plus_strs_m3(&mut self, l: NodeId, r: NodeId) -> StrSet {
        let (ln, rn) = (&self.nodes[l as usize], &self.nodes[r as usize]);
        let (lp, lt, ls) = (ln.prim, !ln.pts.is_empty(), ln.strs);
        let (rp, rt, rs) = (rn.prim, !rn.pts.is_empty(), rn.strs);
        if (lp == 0 && !lt) || (rp == 0 && !rt) {
            return STRS_NONE;
        }
        let r_str = rt || rp & prim::STR != 0;
        if lt || (lp & !prim::STR != 0 && r_str) {
            return STRS_TOP;
        }
        if lp & prim::STR == 0 {
            return STRS_NONE;
        }
        if ls == STRS_TOP || ls < STRS_SET0 {
            return STRS_TOP;
        }
        // The right operand's known heads: its constants and prefixes, and
        // "" when it may be anything else.
        let mut tails: Vec<String> = Vec::new();
        if rp & !prim::STR != 0 || rt || rs == STRS_TOP || rs < STRS_SET0 {
            tails.push(String::new());
        }
        for &a in self.str_atoms(rs) {
            let t = match self.atom_prefix(a) {
                Some(p) => p.to_string(),
                None if self.atom_infix(a).is_some() || self.atom_suffix(a).is_some() || self.atom_excl(a).is_some() => {
                    String::new()
                }
                None => self.atoms[a as usize].clone(),
            };
            tails.push(t);
        }
        let left = self.str_atoms(ls).to_vec();
        if left.len().saturating_mul(tails.len()) > PLUS_PREFIXES_K {
            tails = vec![String::new()];
        }
        let mut out: Vec<Atom> = Vec::new();
        for a in left {
            // A left suffix or exclusion entry: the result's start is unknown.
            if self.atom_suffix(a).is_some() || self.atom_excl(a).is_some() {
                return STRS_TOP;
            }
            if self.atom_is_mark(a) {
                out.push(a);
                continue;
            }
            let c = self.atoms[a as usize].clone();
            for t in &tails {
                let p = format!("{c}{t}");
                if p.is_empty() {
                    return STRS_TOP;
                }
                out.push(self.prefix_atom(&p));
            }
        }
        out.sort_unstable();
        out.dedup();
        self.str_set(&out)
    }

    /// The atom of a program string constant `s`, or `None` when it is any
    /// string: the solver's own key spellings (`#elem`, the star key, the
    /// well-known symbol sentinels) are not the program's key of that
    /// spelling.
    pub(crate) fn str_const_atom(&mut self, s: &str) -> Option<Atom> {
        if s.starts_with('#') || s.starts_with('\u{1}') || s.starts_with("@@__perry_") {
            return None;
        }
        Some(self.atom(s))
    }
}

/// ToPropertyKey of a string `s` is an array index, which the element store
/// covers as it does a Number key's (`StoreIndexed`): "0", or a digit run
/// without a leading zero below 2^32 - 1.
pub(crate) fn index_string(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return false;
    }
    if b.len() > 1 && b[0] == b'0' {
        return false;
    }
    s.parse::<u64>().is_ok_and(|v| v < u32::MAX as u64)
}
