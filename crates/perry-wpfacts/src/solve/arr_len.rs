//! Array lengths as intervals (`model::range`): the `length` an Array token
//! holds.
//!
//! A program array literal of `n` elements is born with length `n`
//! (`Model::arr_birth_len`, recorded only with intervals on). Its length
//! stays `[n, n]` while every write that could move it is an index store
//! whose index interval lies inside `[0, n - 1]` (`Cons::LenIndex`). Every
//! other event makes it ⊤, and nothing narrows it again:
//! - a store of `length`, of a numeric name, or under a computed key
//!   (`arr_len_store`);
//! - an element store other than such an index store (`push`, a spread
//!   into it, a native's element write: `arr_len_store`);
//! - a builtin method that may change the length (`array_method`), and
//!   the HIR's own `pop` (`Expr::ArrayPop`, a ⊤ `Cons::LenIndex`);
//! - a copy of another object's own properties into it (`Object.assign`:
//!   `Cons::SpreadFrom`) and a `defineProperty` of an index or `length`;
//! - an escape or a hard marker (`mark_escaped`, `mark_hard`);
//! - another unit's array, whose stores this unit does not see.
//!
//! A read of an Array's `length` (`arr_len_into`) is an edge from the
//! token's length node, so it carries the interval and follows its growth.

use super::*;

impl Model {
    /// The node holding token `t`'s `length` (an Array of the program).
    pub(crate) fn arr_len(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.arr_len.get(&t) {
            return n;
        }
        let n = self.node_named("arr-len", || format!("arr-len({t})"));
        self.nodes[n as usize].ruled = true;
        self.arr_len.insert(t, n);
        let birth = match self.toks[t as usize] {
            TokKind::Obj(tm, _) if !self.foreign.contains_key(&tm) && !self.is_escaped(t) => {
                self.arr_birth_len.get(&tm).copied()
            }
            _ => None,
        };
        let iv = birth.map_or(Iv::TOP, |k| Iv::int(k, k));
        self.nodes[n as usize].range = iv;
        self.add_prim(n, prim::NUM);
        n
    }

    /// Token `t`'s length may be any Number from now on.
    pub(crate) fn arr_len_top(&mut self, t: Tok) {
        if t == TOP || !self.is_arr(t) {
            return;
        }
        let n = self.arr_len(t);
        if crate::solve::grow_range(&mut self.nodes[n as usize], Iv::TOP) {
            let d = &mut self.nodes[n as usize];
            if !d.in_wl {
                d.in_wl = true;
                self.wl.push_back(n);
            }
        }
    }

    /// A read of Array `t`'s `length` into `dst`.
    pub(crate) fn arr_len_into(&mut self, t: Tok, dst: NodeId) {
        let n = self.arr_len(t);
        self.edge(n, dst);
    }

    /// A store into Array `t` under `key` (`elem`: the element key, which
    /// `store_into` has turned into the any key) at `site`.
    pub(crate) fn arr_len_store(&mut self, t: Tok, key: Key, elem: bool, site: SiteId) {
        let moves = match key {
            Key::Named(a) => {
                let k = &self.atoms[a as usize];
                k == "length" || numeric_key(k)
            }
            Key::Any(_) => !elem || !self.indexed_store_sites.contains(&site),
        };
        if moves {
            self.arr_len_top(t);
        }
    }

    /// An index store whose index lies in `iv`, into Array `t`: it keeps
    /// the length only inside the birth length.
    pub(crate) fn arr_len_index(&mut self, t: Tok, iv: Iv) {
        if t == TOP || !self.is_arr(t) {
            return;
        }
        let n = self.arr_len(t);
        let len = self.nodes[n as usize].range;
        let inside = !len.is_top()
            && (self.arr_len_sabotage || len.lo >= 1 && iv.within(Iv::int(0, len.lo - 1)));
        if !inside {
            self.arr_len_top(t);
        }
    }

    /// PR-C1 (DESIGN-p4l1 §2.8): does a computed-key read of `t` take its
    /// past-the-end `undefined` from `Cons::ElemUndef`? A program Array of
    /// this unit whose birth recorded a length and whose length is not ⊤
    /// yet. Any other token's read adds it at once, as before.
    pub(crate) fn elem_undef_watched(&mut self, t: Tok) -> bool {
        if !self.elem_undef || t == TOP || !self.is_arr(t) || self.is_marker(t) {
            return false;
        }
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return false;
        };
        if !self.arr_birth_len.contains_key(&tm) || self.foreign.contains_key(&tm) {
            return false;
        }
        let n = self.arr_len(t);
        !self.nodes[n as usize].range.is_top()
    }

    /// `Cons::ElemUndef`: the read of `t` under key node `key` into `dst`
    /// may be at or past the end of `t` once its length is ⊤ or the key may
    /// be anything but an integer Number below the length's lower bound
    /// (Invariant L: the length never drops below it). Monotone: a length
    /// only widens, a key only grows. A key with no value yet decides
    /// nothing; the rule runs again when one arrives.
    pub(crate) fn elem_undef(&mut self, t: Tok, key: NodeId, dst: NodeId) {
        if self.nodes[dst as usize].prim & prim::UNDEF != 0 {
            return;
        }
        if self.elem_in_floor(t, key) == Some(false) {
            self.add_prim(dst, prim::UNDEF);
        }
    }

    /// The `ElemUndef` verdict of reading `t` under `key` now: `Some(true)`
    /// when every key is an integer in `[0, lo)` of `t`'s length interval
    /// `[lo, hi]`, `None` while the key holds nothing. The negative control
    /// `arrundef` admits a key that reaches `lo`.
    pub(crate) fn elem_in_floor(&self, t: Tok, key: NodeId) -> Option<bool> {
        let Some(&ln) = self.arr_len.get(&t) else {
            return Some(false);
        };
        let len = self.nodes[ln as usize].range;
        let k = self.range_input(key);
        if k.is_empty() {
            return None;
        }
        if len.is_top() || k.is_top() {
            return Some(false);
        }
        let last = if self.elem_undef_sabotage {
            len.lo
        } else {
            len.lo - 1
        };
        Some(k.lo >= 0 && k.hi <= last)
    }
}
