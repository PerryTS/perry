//! The exhaustive remainder of `Expr`, part: arrays, maps and sets.

use super::*;

impl<'a> Gen<'a> {
    /// The receiver of a generic in-place permutation (`reverse`,
    /// `copyWithin` through `.call`): its arrays' lengths are ⊤ with the
    /// length floor on (`solve::arr_len`).
    fn permuted_len(&mut self, fx: &Fx, r: NodeId) {
        self.len_index_site(fx, r, "permuted");
        self.m.arr_permuted.insert(r);
        if !self.m.arr_floor_off {
            self.m.cons(
                r,
                Cons::LenIndex {
                    lo: crate::model::Iv::TOP.lo,
                    hi: crate::model::Iv::TOP.hi,
                },
            );
        }
    }

    /// Where the storeless `Cons::LenIndex` of receiver `r` runs
    /// (`Model::len_index_sites`). One site per node: a second function's
    /// rule on a node the first owns is a capture, which ends the fact.
    fn len_index_site(&mut self, fx: &Fx, r: NodeId, what: &str) {
        if (self.m.held_lengths || self.m.named_lengths) && !self.m.len_index_sites.contains_key(&r)
        {
            let site = self.store_site(fx.module, fx.f, what.into(), None, false);
            self.m.len_index_sites.insert(r, site);
        }
    }

    pub(super) fn expr_rest_collections(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;
        match e {
            // ------------------------------------------------------ arrays
            Expr::ArrayPushSpread { array_id, source } => {
                let a = self.local(mi, *array_id);
                let s = self.expr(fx, source);
                let it = self.iterate(s);
                self.list_push(a, None);
                self.elem_store(fx, a, it, "push...");
                self.prim_node(prim::NUM)
            }
            Expr::ArrayPop(id) => {
                let a = self.local(mi, *id);
                self.len_index_site(fx, a, "pop");
                // `pop` sets `length` (ArraySetLength): the array's length
                // moves (`solve::arr_len`).
                self.m.cons(
                    a,
                    Cons::LenIndex {
                        lo: crate::model::Iv::TOP.lo,
                        hi: crate::model::Iv::TOP.hi,
                    },
                );
                let out = self.elem_load(a);
                self.m.add_prim(out, prim::UNDEF);
                // The node is the call `a.pop()`: a receiver that holds its
                // own `pop` (an own property, an own prototype, an object
                // behind the Array annotation) runs that method, and its
                // result is the node's value (codegen: the checked entry
                // `js_array_m_pop0`).
                if super::array_method_tiers() {
                    let pop = self.m.atom("pop");
                    self.folded_site(fx, &perry_hir::folded_call::Receiver::Local(*id), pop, a);
                    let ret = self.mcall(a, Key::Named(pop), vec![], vec![]);
                    self.m.edge(ret, out);
                }
                out
            }
            // `Array.prototype.reverse.call(r)`: with the length floor on, a
            // permutation of an Array whose length may grow is ⊤ (Q1,
            // `Model::arr_len_permuted`); this generic form makes it ⊤ for
            // every array receiver, as `pop` does.
            Expr::ArrayReverseValue { receiver } => {
                let r = self.expr(fx, receiver);
                self.permuted_len(fx, r);
                r
            }
            Expr::ArrayCopyWithinValue {
                receiver,
                target,
                start,
                end,
            } => {
                let r = self.expr(fx, receiver);
                self.permuted_len(fx, r);
                let _ = self.prim_op(
                    fx,
                    &[target, start],
                    prim::NUM,
                    "copyWithin",
                    Protocol::ToPrimitive,
                );
                if let Some(x) = end {
                    let _ = self.prim_op(fx, &[x], prim::NUM, "copyWithin", Protocol::ToPrimitive);
                }
                r
            }
            Expr::ArrayLikeMethod { receiver, args, .. } => {
                let r = self.expr(fx, receiver);
                let why = self.m.reason("Array.prototype method on an array-like");
                self.m.cons(
                    r,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::Any,
                    },
                );
                let el = self.load(r, Key::any());
                let num = self.prim_node(prim::NUM);
                let mut outs = vec![el];
                for a in args {
                    let an = self.expr(fx, a);
                    let ret = self.callback(an, vec![el, num, r]);
                    outs.push(ret);
                    // Array-likes may also receive the arguments as elements.
                    self.elem_store(fx, r, an, "array-like method");
                }
                let arr = self.new_arr_from(mi, "arraylike", &outs);
                let out = self.tmp("arraylike-out");
                self.m.edge(arr, out);
                self.m.edge(r, out);
                self.m.add_prim(out, prim::ALL);
                out
            }

            // ---------------------------------------------- maps and sets
            Expr::MapNew => {
                let (t, n) = self.bobj(mi, "Map");
                self.m.map_born(t);
                n
            }
            Expr::SetNew => self.bobj(mi, "Set").1,
            Expr::MapNewFromArray(x) => {
                // Entries `[key, value]`: the map holds their contents.
                let v = self.expr(fx, x);
                let it = self.iterate_closed(v);
                let ek = self.m.elem_key();
                let inner = self.load(it, ek);
                let (t, n) = self.bobj(mi, "Map");
                let fa = self.m.field_any(t);
                self.m.edge(inner, fa);
                // AddEntriesFromIterable (24.1.1.2 step 4.f): each value is
                // Get(entry, "1"), as the constructor call does
                // (`builtins/constructors.rs`).
                self.m.map_born(t);
                let one = Key::Named(self.m.atom("1"));
                let val = self.load(it, one);
                let mv = self.m.map_vals(t);
                self.m.edge(val, mv);
                let zero = Key::Named(self.m.atom("0"));
                let key = self.load(it, zero);
                let mk = self.m.map_keys(t);
                self.m.edge(key, mk);
                n
            }
            Expr::SetNewFromArray(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate_closed(v);
                let (t, n) = self.bobj(mi, "Set");
                let fa = self.m.field_any(t);
                self.m.edge(it, fa);
                n
            }
            Expr::MapEntryKeyAt { map, idx }
            | Expr::MapEntryValueAt { map, idx }
            | Expr::SetValueAt { set: map, idx } => {
                let m = self.expr(fx, map);
                let _ = self.expr(fx, idx);
                let out = self.load(m, Key::any());
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::MapEntries(x) => {
                let m = self.expr(fx, x);
                let ek = self.m.elem_key();
                let el = self.load(m, ek);
                let pair = self.new_arr_from(mi, "entry", &[el]);
                self.new_arr_from(mi, "entries", &[pair])
            }
            Expr::MapKeys(x) | Expr::MapValues(x) | Expr::SetValues(x) => {
                let m = self.expr(fx, x);
                let ek = self.m.elem_key();
                let el = self.load(m, ek);
                self.new_arr_from(mi, "values", &[el])
            }
            Expr::ObjectFromEntries(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate_closed(v);
                // Each entry's key and value: its `"0"` and `"1"` (a
                // [[Get]] under a number key), not every member of its chain.
                let num = Key::Named(self.m.atom(crate::model::NUM_KEY));
                let inner = self.load(it, num);
                let (t, n) = self.bobj(mi, "fromEntries");
                let fa = self.m.field_any(t);
                self.m.edge(inner, fa);
                n
            }
            Expr::ObjectValues(x) => {
                let v = self.expr(fx, x);
                let el = self.own_values(v);
                self.new_arr_from(mi, "values", &[el])
            }
            Expr::ObjectEntries(x) => {
                let v = self.expr(fx, x);
                let el = self.own_values(v);
                let s = self.prim_node(prim::STR);
                let pair = self.new_arr_from(mi, "entry", &[el, s]);
                self.new_arr_from(mi, "entries", &[pair])
            }
            e @ (Expr::ObjectGroupBy { items, key_fn } | Expr::MapGroupBy { items, key_fn }) => {
                let v = self.expr(fx, items);
                let it = self.iterate_closed(v);
                let f = self.expr(fx, key_fn);
                let num = self.prim_node(prim::NUM);
                let k = self.callback(f, vec![it, num]);
                let why = self.m.reason("groupBy key");
                self.m.cons(
                    k,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let group = self.new_arr_from(mi, "group", &[it]);
                // `Object.groupBy` makes a null-prototype object
                // (`members::box_chain`); `Map.groupBy` a Map, whose keys and
                // groups are its contents. Born from groups, not by `new
                // Map()`, it is never closed (`solve/map_vals.rs`): `get`
                // reads its contents.
                let kind = if matches!(e, Expr::MapGroupBy { .. }) {
                    "Map"
                } else {
                    "groups"
                };
                let (t, n) = self.bobj(mi, kind);
                let fa = self.m.field_any(t);
                self.m.edge(group, fa);
                self.m.edge(k, fa);
                if kind == "Map" {
                    self.m.map_born(t, false);
                }
                n
            }
            Expr::ArrayFrom(x, _) => {
                let v = self.expr(fx, x);
                let it = self.iterate_closed(v);
                self.m.cons(v, Cons::FromArrayLike { dst: it });
                self.new_arr_from(mi, "from", &[it])
            }
            Expr::IteratorToArray(x) | Expr::ForOfToArray(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                self.new_arr_from(mi, "from", &[it])
            }
            Expr::ForAwaitToArray(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate_closed(v);
                let aw = self.awaited(it);
                let arr = self.new_arr_from(mi, "fromAsync", &[aw]);
                self.promise_of(mi, arr)
            }
            Expr::ArrayFromArrayLikeHoley(x) => {
                let v = self.expr(fx, x);
                let el = self.load(v, Key::any());
                self.m.add_prim(el, prim::UNDEF);
                self.new_arr_from(mi, "fromLike", &[el])
            }
            Expr::ArrayFromMapped {
                iterable,
                map_fn,
                this_arg,
            } => {
                let v = self.expr(fx, iterable);
                let it = self.iterate_closed(v);
                self.m.cons(v, Cons::FromArrayLike { dst: it });
                let f = self.expr(fx, map_fn);
                let this = match this_arg {
                    Some(t) => ThisArg::Node(self.expr(fx, t)),
                    None => ThisArg::Undef,
                };
                let num = self.prim_node(prim::NUM);
                let ret = self.call_value(f, this, vec![it, num], vec![]);
                self.new_arr_from(mi, "fromMapped", &[ret])
            }
            Expr::IteratorFrom(x) | Expr::GetIterator(x) | Expr::GetAsyncIterator(x) => {
                let v = self.expr(fx, x);
                // The iterator yields what iterating `v` yields (holes
                // included, `Model::hole` of `v`), and nothing else.
                let it = self.iterate_closed(v);
                let out = self.new_dense_arr_from(mi, "iterator", &[it]);
                self.m.edge(v, out);
                out
            }
            Expr::ObjectCoerce(x) => {
                let v = self.expr(fx, x);
                let (_, b) = self.bobj(mi, "Object(prim)");
                let out = self.tmp("objcoerce");
                self.m.edge(v, out);
                self.m.edge(b, out);
                out
            }
            _ => unreachable!("internal compiler error: expr_rest: not a collections variant"),
        }
    }
}
