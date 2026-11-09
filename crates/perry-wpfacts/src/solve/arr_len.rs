//! Array lengths as intervals (`model::range`): the `length` an Array token
//! holds.
//!
//! A program array literal of `n` elements is born with length `n`, and
//! `new Array(k)` of an integer literal `k` with length `k` when the runtime
//! backs it densely (`k <= perry_abi::MAX_FRESH_DENSE_ARRAY_LENGTH`):
//! `Model::arr_birth_len`, recorded only with intervals on. Either birth
//! allocates at least that many slots, so its capacity starts at or above
//! its length.
//!
//! With the length floor on (the default; `Model::arr_floor_off`,
//! `PERRY_FACTS_ARRAY_FLOOR=0`, restores the rules after the list) the
//! interval `[lo, hi]` keeps its birth `lo` for good, and only `hi` grows:
//! - an index store whose index interval is `[a, b]` with `b >= 0`
//!   (`Cons::LenIndex`) raises `hi` to `b + 1` (at most `u32::MAX`): it
//!   sets `length` to at most `max(length, b + 1)`;
//! - an index store whose every index is negative changes nothing: a
//!   negative key is a named property in the array's separate bag
//!   (`perry-runtime/src/array/named_props.rs`), which moves neither the
//!   length nor the element storage;
//! - an append (`push`, a spread into it, a native's element write: an
//!   element store at a site that is not an index store) raises `hi` to
//!   `u32::MAX`. A rule runs once per token, not once per runtime append,
//!   so `hi + 1` would be a false upper bound;
//! - `reverse` and `copyWithin` keep the length, but once `hi > lo` they
//!   make it ⊤ (`Cons::LenPermuted`): with `lo == hi` at the fixpoint no
//!   index below the length is ever a hole, so a permutation moves none
//!   there; past the birth length it may.
//!
//! Every other event makes the length ⊤, and nothing narrows it again:
//! - a store of `length`, of a numeric name, or under a computed key
//!   (`arr_len_store`);
//! - an index store whose index is not an integer interval (a fraction,
//!   NaN, an unbounded key);
//! - a builtin method that may change the length (`array_method`), and
//!   the HIR's own `pop` (`Expr::ArrayPop`, a ⊤ `Cons::LenIndex`);
//! - a copy of another object's own properties into it (`Object.assign`:
//!   `Cons::SpreadFrom`) and a `defineProperty` of an index or `length`;
//! - an escape or a hard marker (`mark_escaped`, `mark_hard`);
//! - another unit's array, whose stores this unit does not see.
//!
//! So `length >= lo` and `capacity >= lo` hold for the token's whole life
//! while the interval is not ⊤ (every `capacity` write in the runtime
//! keeps it: `perry-runtime/src/array/capacity_writes_tests.rs`), and
//! `lo == hi` means the element storage never grows, so it is never
//! reallocated or forwarded.
//!
//! With the floor off the length stays `[n, n]` while every write that could
//! move it is an index store whose index interval lies inside `[0, n - 1]`;
//! every other event, growth and appends included, makes it ⊤.
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
        if t == TOP || !self.is_arr(t) || self.arr_floor_sabotage {
            return;
        }
        let n = self.arr_len(t);
        self.arr_len_grow(n, Iv::TOP);
    }

    /// Token `t`'s length may be up to `hi` from now on (the floor).
    fn arr_len_raise(&mut self, t: Tok, hi: i64) {
        let n = self.arr_len(t);
        let len = self.nodes[n as usize].range;
        if len.is_top() || hi <= len.hi {
            return;
        }
        self.arr_len_grow(n, Iv::int(len.lo, hi));
    }

    fn arr_len_grow(&mut self, n: NodeId, iv: Iv) {
        if crate::solve::grow_range(&mut self.nodes[n as usize], iv) {
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
        if !moves {
            return;
        }
        // An element store at a site that is no index store appends.
        if elem && matches!(key, Key::Any(_)) {
            self.arr_len_append(t);
        } else {
            self.arr_len_top(t);
        }
    }

    /// An append to Array `t` (`push`, an element store at a site that is
    /// no index store): it may run any number of times.
    pub(crate) fn arr_len_append(&mut self, t: Tok) {
        if t == TOP || !self.is_arr(t) {
            return;
        }
        if self.arr_floor_off {
            self.arr_len_top(t);
        } else if self.arr_append_sabotage {
            let n = self.arr_len(t);
            let len = self.nodes[n as usize].range;
            self.arr_len_raise(t, len.hi.saturating_add(1));
        } else if !self.arr_fwd_sabotage {
            self.arr_len_raise(t, i64::from(u32::MAX));
        }
    }

    /// An index store whose index lies in `iv`, into Array `t`.
    pub(crate) fn arr_len_index(&mut self, t: Tok, iv: Iv) {
        if t == TOP || !self.is_arr(t) {
            return;
        }
        let n = self.arr_len(t);
        let len = self.nodes[n as usize].range;
        if self.arr_floor_off {
            // Off: the length is kept only inside the birth length.
            let inside = !len.is_top()
                && (self.arr_len_sabotage || len.lo >= 1 && iv.within(Iv::int(0, len.lo - 1)));
            if !inside {
                self.arr_len_top(t);
            }
            return;
        }
        if self.arr_len_sabotage || iv.is_empty() || (!iv.is_top() && iv.hi < 0) {
            return;
        }
        if iv.is_top() {
            self.arr_len_top(t);
            return;
        }
        // `a[i] = v` at an index `i <= b` sets `length` to at most
        // `max(length, b + 1)`; an index of `u32::MAX` or more is a named
        // property.
        self.arr_len_raise(t, iv.hi.saturating_add(1).min(i64::from(u32::MAX)));
    }

    /// `reverse` or `copyWithin` of Array `t` (Q1): its length becomes ⊤
    /// once its interval is not a point (`Cons::LenPermuted`). A rule on the
    /// length node, so a later growth re-runs it.
    pub(crate) fn arr_len_permuted(&mut self, t: Tok) {
        if self.arr_floor_off || t == TOP || !self.is_arr(t) {
            return;
        }
        let n = self.arr_len(t);
        let seen = self.nodes[n as usize]
            .cons
            .iter()
            .any(|c| matches!(c, Cons::LenPermuted { t: x } if *x == t));
        if !seen {
            self.cons(n, Cons::LenPermuted { t });
        }
    }

    /// `Cons::LenPermuted` on token `t`'s length node `n`.
    pub(crate) fn arr_len_permuted_check(&mut self, n: NodeId, t: Tok) {
        let len = self.nodes[n as usize].range;
        if !len.is_top() && !len.is_empty() && len.hi > len.lo {
            self.arr_len_top(t);
        }
    }
}
