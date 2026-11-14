//! Constraint generation: the promise and iteration protocols.
//!
//! What `await`, promise adoption, an untransformed async body, `for-of`
//! and GetIterator contribute to the model. Split from `gen_expr.rs` (file
//! size cap); the methods stay on `Gen`.

use crate::gen::{Fx, Gen};
use crate::model::*;

impl<'a> Gen<'a> {
    /// `await v` / promise adoption: the value itself (non-thenables), the
    /// settled value of promise objects, and for thenables whatever their
    /// `then` passes to the resolve function.
    pub(crate) fn awaited(&mut self, v: NodeId) -> NodeId {
        let out = self.tmp("awaited");
        if !self.m.reach_thenable {
            return self.awaited_plain(v, out);
        }
        // One rule with `builtins::adopt`: a value fulfils as itself only
        // when its `then` read is not callable (`Cons::Fulfil`).
        self.m.cons(v, Cons::Fulfil { dst: out });
        let pa = crate::builtins::promise_result_atom(&mut self.m);
        self.m.cons(
            v,
            Cons::Load {
                key: Key::Named(pa),
                dst: out,
            },
        );
        // Thenables: `then(resolve, reject)` with runtime resolvers.
        let rn = self.tmp("resolver");
        let jn = self.tmp("rejecter");
        let then = self.m.atom("then");
        let (call, ret) = self.new_call(ThisArg::Node(v), vec![rn, jn], vec![]);
        let res = crate::builtins::resolver(&mut self.m, call, out);
        self.m.add_tok(rn, res);
        let thrown = self.thrown;
        let rej = crate::builtins::rejecter(&mut self.m, thrown);
        self.m.add_tok(jn, rej);
        self.m.cons(
            v,
            Cons::MCall {
                key: Key::Named(then),
                call,
            },
        );
        let _ = ret;
        out
    }

    /// `PERRY_FACTS_REACH_THENABLE=0` (`reach::thenable`): the previous
    /// `awaited`, where the value also flows into `out` as itself and the
    /// resolve function copies what a thenable passes it.
    fn awaited_plain(&mut self, v: NodeId, out: NodeId) -> NodeId {
        self.m.edge(v, out);
        let pa = crate::builtins::promise_result_atom(&mut self.m);
        self.m.cons(
            v,
            Cons::Load {
                key: Key::Named(pa),
                dst: out,
            },
        );
        let t = crate::builtins::plain_resolver(&mut self.m, out);
        let resolver = self.tok_node(t);
        let thrown = self.thrown;
        let t = crate::builtins::plain_resolver(&mut self.m, thrown);
        let rej = self.tok_node(t);
        let then = self.m.atom("then");
        let (call, ret) = self.new_call(ThisArg::Node(v), vec![resolver, rej], vec![]);
        self.m.cons(
            v,
            Cons::MCall {
                key: Key::Named(then),
                call,
            },
        );
        let _ = ret;
        out
    }

    /// The body of an async function that runs untransformed: one without
    /// `await`, which `async_to_generator` leaves `is_async` and codegen
    /// lowers direct (`lower_async_rejecting_stmts`): a call returns a fresh
    /// promise (`js_async_fn_result`) settled with what the body returns, a
    /// returned thenable adopted, and rejected with what the body throws; a
    /// call never throws. Returns the body's context, whose `ret` is the
    /// settled value, and its throw region, the realm's rejection cell (as
    /// for the async-step driver's step closure, `async_callback`).
    pub(crate) fn async_body(&mut self, fx: &Fx) -> (Fx, NodeId) {
        let settled = self.tmp("async-settled");
        let p = self.promise_of(fx.module, settled);
        self.m.edge(p, fx.ret);
        (
            Fx {
                ret: settled,
                ..fx.clone()
            },
            self.thrown,
        )
    }

    /// A fresh promise template whose settled value is `value`.
    pub(crate) fn promise_of(&mut self, mi: u32, value: NodeId) -> NodeId {
        let tm = self.new_bobj_template(mi, "Promise");
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let a = crate::builtins::promise_result_atom(&mut self.m);
        let f = self.m.field(t, a);
        let aw = self.awaited(value);
        self.m.edge(aw, f);
        self.tok_node(t)
    }

    /// The values produced by iterating whatever `v` holds: elements of
    /// arrays/collections/primitive boxes, and for iterables the protocol:
    /// `v[Symbol.iterator]()`, then `.next()` on the result and on `v`
    /// itself, then `.value`.
    pub(crate) fn iterate(&mut self, v: NodeId) -> NodeId {
        self.iterate_with(v, false)
    }

    /// `iterate` by a consumer that can stop before the iterator is done:
    /// what it does with a value may throw (the mapper of `Array.from`, a
    /// `groupBy` callback, the adder of a collection constructor on an
    /// entry that is no object, a property definition), or it hands the
    /// iterator on (`Iterator.from`), and the runtime then closes the
    /// iterator (IteratorClose, ECMA-262 7.4.11): `return` is called on it,
    /// a protocol call as the for-of record's close is (`gen/glue.rs`).
    pub(crate) fn iterate_closed(&mut self, v: NodeId) -> NodeId {
        let close = self.m.reach_iter_close;
        self.iterate_with(v, close)
    }

    fn iterate_with(&mut self, v: NodeId, close: bool) -> NodeId {
        let out = self.tmp("iterated");
        let ik = self.m.iter_key();
        self.m.cons(v, Cons::Load { key: ik, dst: out });
        let it = self.iterator_of(v);
        let na = self.m.atom("next");
        let r1 = self.mcall(it, Key::Named(na), vec![], vec![]);
        // The step of `v` itself: the runtime's (`js_get_iterator` returns a
        // value it finds no iterator for unchanged), a protocol call as in
        // `builtins::iterate`.
        let (sc, r2) = self.new_call(ThisArg::Node(v), vec![], vec![]);
        self.m.protocol_calls.insert(sc);
        self.m.cons(
            v,
            Cons::MCall {
                key: Key::Named(na),
                call: sc,
            },
        );
        // Only results seen not done: `Model::yield_key`.
        let va = self.m.yield_key();
        self.m.cons(r1, Cons::Load { key: va, dst: out });
        self.m.cons(r2, Cons::Load { key: va, dst: out });
        self.step_done(r1);
        self.step_done(r2);
        if close {
            // The iterator is what `v[Symbol.iterator]()` returned, or `v`
            // itself (a value the runtime finds no iterator for).
            let ra = Key::Named(self.m.atom("return"));
            for recv in [it, v] {
                let (rc, _) = self.new_call(ThisArg::Node(recv), vec![], vec![]);
                self.m.protocol_calls.insert(rc);
                self.m.cons(recv, Cons::MCall { key: ra, call: rc });
            }
        }
        out
    }

    /// The consumer's read of `done` on the step results in `result`
    /// (`Model::step_done`), a node of the body being walked.
    pub(crate) fn step_done(&mut self, result: NodeId) {
        if let Some(d) = self.m.step_done(result) {
            self.m.own(d);
        }
    }

    /// What `v[Symbol.iterator]()` returns (GetIterator, ECMA-262 7.4.3):
    /// a protocol call.
    pub(crate) fn iterator_of(&mut self, v: NodeId) -> NodeId {
        let (pc, it) = self.new_call(ThisArg::Node(v), vec![], vec![]);
        let wk = Key::Named(self.m.atom(crate::builtins::WK_ITERATOR));
        self.m.protocol_calls.insert(pc);
        self.m.rel(|| format!("protocolcall {pc}"));
        self.m.cons(v, Cons::MCall { key: wk, call: pc });
        it
    }
}
