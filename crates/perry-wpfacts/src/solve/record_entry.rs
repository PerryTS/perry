//! Rule RP2: the protocol arm of a guarded iterator record never sees an
//! ordinary Array whose iteration is the intrinsic one (`Cons::RecordEntry`).
//!
//! A destructuring record takes its protocol arm only when
//! `arrayRecordNeedsIterator(s)` is true. The runtime
//! (`js_array_record_needs_iterator`, `array_record_enter`) answers false
//! for every Array object while the program never makes the `@@iterator` of an Array
//! other than the intrinsic one and never changes
//! `%ArrayIteratorPrototype%.next` (`Model::array_iteration_writes`), and
//! for an Array whose own and inherited `@@iterator` is the intrinsic one
//! otherwise. So the `GetIterator(s)` and the steps of that arm never see such an
//! Array: the filter holds it back while
//! - its template is born an Array (`Model::ordinary_arrays`: a literal, a
//!   spread literal, an `Object.entries` pair; never an iterator model),
//! - no code outside the program may hold it (not escaped, not HARD),
//! - it has no own `@@iterator` (no field or accessor under it, no write
//!   under a String or Symbol key no name lists; a Number key names a
//!   numeric String, never `@@iterator`, ECMA-262 7.1.19) and its chain is
//!   the one it was born with,
//! - and `array_iteration_writes()` is `(None, None)`.
//! Once one of them fails, `settle_record_entry` passes the token
//! (monotone: a released token stays released).
use crate::model::*;

impl Model {
    /// Does the protocol arm of a guarded record never see `t` now?
    pub(crate) fn record_entry_holds(&self, t: Tok) -> bool {
        if t == TOP {
            return false;
        }
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return false;
        };
        if !self.ordinary_arrays.contains(&tm) || self.is_escaped(t) || self.hard.contains_key(&t) {
            return false;
        }
        let wk = self.atom_index.get(crate::builtins::WK_ITERATOR).copied();
        let star = |a: Atom| self.atoms[a as usize] == crate::STAR_KEY;
        // A field node a read made holds nothing until a store fills it.
        let filled = |f: NodeId| {
            let nd = &self.nodes[f as usize];
            nd.prim != 0 || !nd.pts.to_sorted().is_empty()
        };
        if wk.is_some_and(|a| self.fields.get(&(t, a)).is_some_and(|&f| filled(f)))
            || self
                .accessor_keys
                .get(&t)
                .is_some_and(|ks| ks.iter().any(|&a| Some(a) == wk || star(a)))
            || self.any_key_written(t)
        {
            return false;
        }
        let born_chain = self.proto_node.get(&t).is_none_or(|&pn| {
            let nd = &self.nodes[pn as usize];
            nd.prim == 0
                && nd
                    .pts
                    .to_sorted()
                    .iter()
                    .all(|&p| matches!(self.toks[p as usize], TokKind::ObjectProto))
        });
        born_chain && self.array_iteration_writes() == (None, None)
    }

    /// Once the worklist is empty: pass every held token that no longer
    /// qualifies (`record_entry_holds`).
    pub(crate) fn settle_record_entry(&mut self) -> bool {
        if self.record_entry_held.is_empty() {
            return false;
        }
        let held = std::mem::take(&mut self.record_entry_held);
        let mut kept = Vec::with_capacity(held.len());
        for (t, dst) in held {
            if self.record_entry_holds(t) {
                kept.push((t, dst));
            } else {
                self.add_tok(dst, t);
            }
        }
        self.record_entry_held = kept;
        !self.wl.is_empty()
    }
}
