//! The exhaustive remainder of `Expr`, part: arrays, maps and sets.

use super::*;

impl<'a> Gen<'a> {
    pub(super) fn expr_rest_collections(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;
        match e {
            // ------------------------------------------------------ arrays
            Expr::ArrayPush {
                array_id, value, ..
            }
            | Expr::ArrayUnshift { array_id, value } => {
                let a = self.local(mi, *array_id);
                let v = self.expr(fx, value);
                self.elem_store(fx, a, v, "push");
                self.prim_node(prim::NUM)
            }
            Expr::ArrayPushSpread { array_id, source } => {
                let a = self.local(mi, *array_id);
                let s = self.expr(fx, source);
                let it = self.iterate(s);
                self.elem_store(fx, a, it, "push...");
                self.prim_node(prim::NUM)
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                let a = self.local(mi, *id);
                let out = self.elem_load(a);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::ArraySlice {
                array, start, end, ..
            } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(
                    fx,
                    &[start],
                    prim::NUM,
                    "slice index",
                    Protocol::ToPrimitive,
                );
                if let Some(x) = end {
                    let _ = self.prim_op(fx, &[x], prim::NUM, "slice index", Protocol::ToPrimitive);
                }
                let el = self.elem_load(a);
                self.new_species_arr(mi, "slice", &[el], a)
            }
            Expr::ArraySplice {
                array_id,
                start,
                delete_count,
                items,
            } => {
                let a = self.local(mi, *array_id);
                let _ = self.prim_op(
                    fx,
                    &[start],
                    prim::NUM,
                    "splice index",
                    Protocol::ToPrimitive,
                );
                if let Some(d) = delete_count {
                    let _ =
                        self.prim_op(fx, &[d], prim::NUM, "splice count", Protocol::ToPrimitive);
                }
                for it in items {
                    let v = self.expr(fx, it);
                    self.elem_store(fx, a, v, "splice");
                }
                let el = self.elem_load(a);
                self.new_species_arr(mi, "splice", &[el], a)
            }
            Expr::ArrayForEach { array, callback } => {
                let _ = self.array_cb(fx, array, callback);
                self.prim_node(prim::UNDEF)
            }
            Expr::ArrayMap { array, callback } => {
                let (a, _, ret) = self.array_cb(fx, array, callback);
                self.new_species_arr(mi, "map", &[ret], a)
            }
            Expr::ArrayFlatMap { array, callback } => {
                let (a, _, ret) = self.array_cb(fx, array, callback);
                let inner = self.elem_load(ret);
                self.new_species_arr(mi, "flatMap", &[ret, inner], a)
            }
            Expr::ArrayFilter { array, callback } => {
                let (a, el, _) = self.array_cb(fx, array, callback);
                self.new_species_arr(mi, "filter", &[el], a)
            }
            Expr::ArrayFind { array, callback } | Expr::ArrayFindLast { array, callback } => {
                let (_, el, _) = self.array_cb(fx, array, callback);
                let out = self.tmp("find");
                self.m.edge(el, out);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::ArrayFindIndex { array, callback }
            | Expr::ArrayFindLastIndex { array, callback } => {
                let _ = self.array_cb(fx, array, callback);
                self.prim_node(prim::NUM)
            }
            Expr::ArraySome { array, callback } | Expr::ArrayEvery { array, callback } => {
                let _ = self.array_cb(fx, array, callback);
                self.prim_node(prim::BOOL)
            }
            Expr::ArraySort { array, comparator } => {
                let a = self.expr(fx, array);
                let c = self.expr(fx, comparator);
                let el = self.elem_load(a);
                let r = self.callback(c, vec![el, el]);
                let why = self.m.reason("sort comparator result");
                self.m.cons(
                    r,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::ToPrimitive,
                    },
                );
                a
            }
            Expr::ArrayToSorted { array, comparator } => {
                let a = self.expr(fx, array);
                let el = self.elem_load(a);
                if let Some(c) = comparator {
                    let cn = self.expr(fx, c);
                    let _ = self.callback(cn, vec![el, el]);
                }
                self.new_arr_from(mi, "toSorted", &[el])
            }
            Expr::ArrayReduce {
                array,
                callback,
                initial,
            }
            | Expr::ArrayReduceRight {
                array,
                callback,
                initial,
            } => {
                let a = self.expr(fx, array);
                let cb = self.expr(fx, callback);
                let el = self.elem_load(a);
                let acc = self.tmp("acc");
                match initial {
                    Some(i) => {
                        let iv = self.expr(fx, i);
                        self.m.edge(iv, acc);
                    }
                    None => self.m.edge(el, acc),
                }
                let num = self.prim_node(prim::NUM);
                let ret = self.callback(cb, vec![acc, el, num, a]);
                self.m.edge(ret, acc);
                acc
            }
            Expr::ArrayFlat { array } => {
                let a = self.expr(fx, array);
                let el = self.elem_load(a);
                let inner = self.elem_load(el);
                self.new_species_arr(mi, "flat", &[el, inner], a)
            }
            Expr::ArrayToReversed { array } => {
                let a = self.expr(fx, array);
                let el = self.elem_load(a);
                self.new_arr_from(mi, "copy", &[el])
            }
            Expr::ArrayValues(array) => {
                let a = self.expr(fx, array);
                let ek = self.m.elem_key();
                let el = self.load(a, ek);
                self.new_iter_from(mi, crate::builtins::ITERATOR_KIND, &[el])
            }
            Expr::ArrayToSpliced {
                array,
                start,
                delete_count,
                items,
            } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(
                    fx,
                    &[start, delete_count],
                    prim::NUM,
                    "toSpliced index",
                    Protocol::ToPrimitive,
                );
                let el = self.elem_load(a);
                let mut parts = vec![el];
                for it in items {
                    parts.push(self.expr(fx, it));
                }
                self.new_arr_from(mi, "toSpliced", &parts)
            }
            Expr::ArrayWith {
                array,
                index,
                value,
            } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(fx, &[index], prim::NUM, "with index", Protocol::ToPrimitive);
                let v = self.expr(fx, value);
                let el = self.elem_load(a);
                self.new_arr_from(mi, "with", &[el, v])
            }
            Expr::ArrayAt { array, index } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(fx, &[index], prim::NUM, "at index", Protocol::ToPrimitive);
                let out = self.elem_load(a);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::ArrayReverseValue { receiver } => self.expr(fx, receiver),
            Expr::ArrayCopyWithin {
                array_id,
                target,
                start,
                end,
            } => {
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
                self.local(mi, *array_id)
            }
            Expr::ArrayCopyWithinValue {
                receiver,
                target,
                start,
                end,
            } => {
                let r = self.expr(fx, receiver);
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
            Expr::ArrayEntries(array) => {
                let a = self.expr(fx, array);
                let ek = self.m.elem_key();
                let el = self.load(a, ek);
                let num = self.prim_node(prim::NUM);
                let pair = self.new_arr_from(mi, "entry", &[el, num]);
                self.new_iter_from(mi, crate::builtins::ITERATOR_KIND, &[pair])
            }
            Expr::ArrayKeys(array) => {
                let _ = self.expr(fx, array);
                let num = self.prim_node(prim::NUM);
                self.new_iter_from(mi, crate::builtins::ITERATOR_KIND, &[num])
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
            Expr::MapNew => self.bobj(mi, "Map").1,
            Expr::SetNew => self.bobj(mi, "Set").1,
            Expr::MapNewFromArray(x) => {
                // Entries `[key, value]`: the map holds their contents.
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                let ek = self.m.elem_key();
                let inner = self.load(it, ek);
                let (t, n) = self.bobj(mi, "Map");
                let fa = self.m.field_any(t);
                self.m.edge(inner, fa);
                n
            }
            Expr::SetNewFromArray(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                let (t, n) = self.bobj(mi, "Set");
                let fa = self.m.field_any(t);
                self.m.edge(it, fa);
                n
            }
            Expr::MapSet { map, key, value } => {
                let m = self.expr(fx, map);
                let k = self.expr(fx, key);
                let v = self.expr(fx, value);
                self.elem_store(fx, m, k, "map.set key");
                self.elem_store(fx, m, v, "map.set");
                m
            }
            Expr::SetAdd { set_id, value } => {
                let s = self.local(mi, *set_id);
                let v = self.expr(fx, value);
                self.elem_store(fx, s, v, "set.add");
                s
            }
            Expr::MapGet { map, key } => {
                let m = self.expr(fx, map);
                let _ = self.expr(fx, key);
                let out = self.load(m, Key::any());
                self.m.add_prim(out, prim::UNDEF);
                out
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
                let it = self.iterate(v);
                let inner = self.load(it, Key::any());
                let (t, n) = self.bobj(mi, "fromEntries");
                let fa = self.m.field_any(t);
                self.m.edge(inner, fa);
                n
            }
            Expr::ObjectValues(x) => {
                let v = self.expr(fx, x);
                let el = self.load(v, Key::any());
                self.new_arr_from(mi, "values", &[el])
            }
            Expr::ObjectEntries(x) => {
                let v = self.expr(fx, x);
                let el = self.load(v, Key::any());
                let s = self.prim_node(prim::STR);
                let pair = self.new_arr_from(mi, "entry", &[el, s]);
                self.new_arr_from(mi, "entries", &[pair])
            }
            Expr::ObjectGroupBy { items, key_fn } | Expr::MapGroupBy { items, key_fn } => {
                let v = self.expr(fx, items);
                let it = self.iterate(v);
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
                let (t, n) = self.bobj(mi, "groups");
                let fa = self.m.field_any(t);
                self.m.edge(group, fa);
                self.m.edge(k, fa);
                n
            }
            Expr::ArrayFrom(x, _) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
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
                let it = self.iterate(v);
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
                let it = self.iterate(v);
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
                let it = self.iterate(v);
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
