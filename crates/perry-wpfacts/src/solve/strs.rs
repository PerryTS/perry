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
const PLUS_PREFIXES_K: usize = 256;

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
        let t = self.atom_prefix(a).or_else(|| self.atom_suffix(a)).unwrap_or(&self.atoms[a as usize]);
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
            // Rule M4: a known suffix reads the names of its glob `*s`.
            if let Some(s) = self.atom_suffix(a) {
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
    pub(crate) fn plus_strs(&mut self, l: NodeId, r: NodeId) -> StrSet {
        if let Some(s) = self.plus_const(l, r) {
            return s;
        }
        let m3 = self.plus_strs_m3(l, r);
        if m3 != STRS_TOP {
            return m3;
        }
        if let Some(s) = self.suffix_strs(r) {
            return s;
        }
        // Rule K7, 13.15.3: when one operand is String-only, the result is
        // the concatenation of both ToStrings, so it holds every character
        // that operand surely holds.
        match self.infix_strs(l).or_else(|| self.infix_strs(r)) {
            Some(s) => s,
            None => STRS_TOP,
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
                None if self.atom_infix(a).is_some() || self.atom_suffix(a).is_some() => String::new(),
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
            // A left suffix entry: the result's start is unknown.
            if self.atom_suffix(a).is_some() {
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
