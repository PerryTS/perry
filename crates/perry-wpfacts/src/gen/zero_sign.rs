//! −0 tolerance, the generator's half (RFC §10.3 as amended, PR D; the
//! solve's half is `crate::zero_sign`).
//!
//! A Number node may keep an integer interval while it may hold `-0` only
//! when no use of its value can observe the sign of a zero. The
//! classification is exhaustive by construction: every value the generator
//! evaluates goes through `Gen::expr`, which marks the node it returns as one
//! whose uses the generator sees (`Node::zs_known`) and, unless the parent
//! asked for the operand with [`Gen::expr_zs`], as sign-observing
//! (`Node::zs_use`). A parent asks only where its use is one the solve
//! classifies: a compare, a truthiness test, an index key, an integer rule
//! (`Cons::Range`, decided by its operator), a copy (a binding, a return, a
//! store into a field or element, a logical or conditional result) or a call
//! argument (decided by the callee: a program function binds it to its
//! parameter; a native, an unknown or a bound callee observes it,
//! `Model::with_native`). Any other parent, a division, `**`, `+x`,
//! `Object.is`, a template literal, `throw`, a native's operand, leaves the
//! default: observed.
use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    /// `expr` of an operand whose use the parent classifies.
    pub(crate) fn expr_zs(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        self.zs_ok = true;
        self.expr(fx, e)
    }

    /// `Gen::expr`'s bookkeeping for node `n`: its uses are seen; one is
    /// sign-observing unless the parent classified it (`ok`).
    pub(crate) fn zs_mark(&mut self, n: NodeId, ok: bool) {
        if !self.m.zero_sign || n == NO_NODE {
            return;
        }
        let nd = &mut self.m.nodes[n as usize];
        nd.zs_known = true;
        nd.zs_use |= !ok;
    }

    /// A node of the generator's own whose values reach only nodes the
    /// generator classifies or rules: a narrowing's clip and end
    /// (`gen/narrow.rs`), whose reads are expressions, and a narrowing
    /// bound, which only a loop test's clip reads.
    pub(crate) fn zs_carrier(&mut self, n: NodeId) {
        if self.m.zero_sign {
            self.m.nodes[n as usize].zs_known = true;
        }
    }

    /// Values a native operation the generator lowers itself reads
    /// (`runtime_extern`): sign-observing.
    pub(crate) fn zs_observe(&mut self, nodes: &[NodeId]) {
        self.m.zs_observe(nodes);
    }

    /// PR D: `"lit".indexOf(x)` / `lastIndexOf` is `-1` or an index of the
    /// literal (or its length, for an empty search string past the end):
    /// the native's result interval (`Model::str_index_len`).
    pub(crate) fn str_index_bound(&mut self, object: &Expr, property: &str, call: CallId) {
        if !self.m.zero_sign || !matches!(property, "indexOf" | "lastIndexOf") {
            return;
        }
        if let Expr::String(s) = object {
            let len = s.encode_utf16().count() as i64;
            self.m.str_index_len.insert(call, len);
        }
    }

    /// `prim_op` of a relational compare: ToPrimitive on each operand, a
    /// Boolean result. `-0 < x` is `0 < x`, so the operands' use is
    /// classified (absorbing).
    pub(crate) fn prim_op_zs(
        &mut self,
        fx: &Fx,
        operands: &[&Expr],
        result: u16,
        why: &str,
        proto: Protocol,
    ) -> NodeId {
        let r = self.m.reason(why);
        for o in operands {
            let v = self.expr_zs(fx, o);
            self.m.cons(v, Cons::Implicit { reason: r, proto });
        }
        let out = self.tmp("primop");
        self.m.add_prim(out, result);
        out
    }
}

impl Model {
    /// The sign of a zero these nodes hold is observed (a native's
    /// argument, an unknown callee's).
    pub(crate) fn zs_observe(&mut self, nodes: &[NodeId]) {
        if !self.zero_sign {
            return;
        }
        for &n in nodes {
            if n != NO_NODE {
                self.nodes[n as usize].zs_use = true;
            }
        }
    }

    /// Run builtin code for `call` (`with_b`). Its arguments are
    /// sign-observing when the builtin acted on the call (it left a
    /// footprint: the result, an argument, new nodes or calls, solve work),
    /// unless it declared them absorbing while it ran (`zs_absorbing`: every
    /// operand ToString'd or ToIntegerOrInfinity'd, which map `-0` and `0`
    /// to the same value). A lookup that found no builtin member leaves no
    /// footprint. Once a builtin declared a call's arguments absorbing, a
    /// later builtin for the call that leaves no footprint observes them
    /// (its effects may be the absorbing one's).
    pub(crate) fn with_native<R>(&mut self, call: CallId, f: impl FnOnce(&mut Self) -> R) -> R {
        if !self.zero_sign {
            return self.with_b(f);
        }
        let before = self.zs_footprint(call);
        let prev = std::mem::replace(&mut self.zs_absorbing, false);
        let r = self.with_b(f);
        let absorbing = std::mem::replace(&mut self.zs_absorbing, prev);
        if absorbing {
            self.zs_absorbed.insert(call);
        } else if self.zs_footprint(call) != before || self.zs_absorbed.contains(&call) {
            self.zs_call_args(call);
        }
        r
    }

    /// What a builtin acting on `call` changes (`with_native`).
    fn zs_footprint(&self, call: CallId) -> (usize, usize, u64, u64) {
        let c = &self.calls[call as usize];
        let mut h: u64 = 0;
        for &n in std::iter::once(&c.ret).chain(&c.args).chain(&c.spread) {
            if n == NO_NODE {
                continue;
            }
            let nd = &self.nodes[n as usize];
            for x in [
                nd.prim as u64,
                nd.cons.len() as u64,
                nd.succ.len() as u64,
                nd.pts.len() as u64,
            ] {
                h = h.wrapping_mul(0x100000001b3).wrapping_add(x);
            }
        }
        (self.nodes.len(), self.calls.len(), self.work.get(), h)
    }

    /// Every argument of `call` is sign-observing, and so is every element
    /// of its spread arrays (the callee reads them unseen; the solve expands
    /// `zs_spread` once their tokens are known, `crate::zero_sign`).
    pub(crate) fn zs_call_args(&mut self, call: CallId) {
        if !self.zero_sign {
            return;
        }
        let c = &self.calls[call as usize];
        let (args, spread) = (c.args.clone(), c.spread.clone());
        self.zs_observe(&args);
        self.zs_spread.extend(spread);
    }
}
