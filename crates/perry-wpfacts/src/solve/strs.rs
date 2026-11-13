//! The string constants a node holds (`Node::strs`, DESIGN SEEDKV): one
//! value lattice beside `prim`. A string literal is a source node holding
//! its one atom (`Gen::str_literal`); copy edges and the filters that pass a
//! string join sets; every other string birth is any string (`STRS_TOP`),
//! the default of `add_prim`. A set past `STRS_K` atoms is any string.
use crate::model::*;

impl Model {
    /// The interned set of `atoms` (sorted, distinct; `STRS_TOP` past
    /// `STRS_K`).
    // `STRS_K` is a tuning bound; at `usize::MAX` the cap is off and the
    // comparison is always false, which clippy flags.
    #[allow(clippy::absurd_extreme_comparisons)]
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
    // See `str_set`: the `STRS_K` comparison is always false while the cap is off.
    #[allow(clippy::absurd_extreme_comparisons)]
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
