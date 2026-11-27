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
//!
//! Once one of them fails, `settle_record_entry` passes the token
//! (monotone: a released token stays released).
//!
//! Rule RP2-false: the array arm (`arrayRecordNeedsIterator(s)` false) sees
//! `s` only as a value the runtime answers false for
//! (`Cons::ArrayRecordEntry`). `array_record_enter` answers false only for
//! an object whose header is an array (`GC_TYPE_ARRAY`: an Array, an
//! `arguments` object, an instance a native `Array` parent allocates) or a
//! lazy JSON array; it refuses every other value (a primitive, a function,
//! a Proxy, a Map or Set, an ordinary object) and the protocol arm runs.
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

    /// Rule RP2-false: may the array arm of a guarded record see `t`
    /// (`array_record_enter` answer false for it)? ⊤ and markers (values of
    /// other units) may be Arrays; so may an Array or `arguments` birth, a
    /// builtin value whose kind may be an array (`builtins::bobj_may_be_array`,
    /// `builtins::ns_may_be_array`), an instance of a class whose chain
    /// reaches a native parent or a value parent (`class A extends Array`),
    /// and `ObjectProto`, which stands for `%Array.prototype%` too (an Array
    /// exotic object, ECMA-262 23.1.3). No function, Proxy, prototype object,
    /// module namespace, ordinary object or plain class instance is.
    pub(crate) fn array_arm_may_see(&self, t: Tok) -> bool {
        if t == TOP || self.is_marker(t) {
            return true;
        }
        match &self.toks[t as usize] {
            TokKind::Top | TokKind::ObjectProto => true,
            TokKind::Obj(tm, _) => match &self.tmpls[*tm as usize].kind {
                TmplKind::Arr { .. } | TmplKind::Args { .. } => true,
                TmplKind::BObj { what, .. } => crate::builtins::bobj_may_be_array(what),
                TmplKind::Class { .. } => !self.plain_class_chain(*tm),
                TmplKind::Lit { .. } | TmplKind::FnInst { .. } | TmplKind::ObjCreate { .. } => {
                    false
                }
            },
            TokKind::Builtin(ns) => crate::builtins::ns_may_be_array(*ns),
            TokKind::Func(..)
            | TokKind::Bound(_)
            | TokKind::Proxy(_)
            | TokKind::ClassObj(_)
            | TokKind::Proto(_)
            | TokKind::BuiltinFn(_)
            | TokKind::ModNs(_)
            | TokKind::NativeProto(..) => false,
        }
    }

    /// Is every link of class `c`'s `extends` chain a program class with
    /// neither a native parent nor a value parent? Its instances are then
    /// ordinary objects the program allocates.
    fn plain_class_chain(&self, mut c: TmplId) -> bool {
        let mut seen = 0usize;
        loop {
            let tp = &self.tmpls[c as usize];
            if tp.native_parent.is_some() || tp.parent_value.is_some() {
                return false;
            }
            match tp.parent {
                None => return true,
                Some(p) if p != c && seen < self.tmpls.len() => {
                    c = p;
                    seen += 1;
                }
                Some(_) => return false,
            }
        }
    }
}
