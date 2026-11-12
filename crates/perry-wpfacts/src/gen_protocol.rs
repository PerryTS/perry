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
        self.m.edge(v, out);
        let pa = self.m.atom("#value");
        self.m.cons(
            v,
            Cons::Load {
                key: Key::Named(pa),
                dst: out,
            },
        );
        // Thenables: `then(resolve, reject)` with runtime resolvers.
        let resolver = self.resolver_fn(out);
        let rej = self.resolver_fn(self.thrown);
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

    /// A synthetic function whose first parameter flows into `dst` (a
    /// promise resolver handed to user code).
    pub(crate) fn resolver_fn(&mut self, dst: NodeId) -> NodeId {
        let t = crate::builtins::resolver(&mut self.m, dst);
        self.tok_node(t)
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
        let a = self.m.atom("#value");
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
