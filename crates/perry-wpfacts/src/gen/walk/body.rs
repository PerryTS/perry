//! Constraint generation, part 1c: walking function bodies and parameters.

use super::*;

impl<'a> Gen<'a> {
    pub(crate) fn walk_fn(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &'a Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        let fn_ctor = class.is_none() && ctor_of.is_none();
        if let Some(ks) = crate::born_with::store_only_keys(f) {
            let ks = ks.iter().map(|k| self.m.atom(k)).collect();
            self.m.store_only.insert(fxi, ks);
        }
        if fn_ctor {
            self.note_fn_phase(fxi, crate::ctor_phase::analyze_fn(f, self.m.ns_pairs));
        }
        self.rooted_stores.extend(crate::rooted::stores(&f.body));
        // DECISIONS 225: a body generation walks once it can run.
        if self.defer_walk(fxi, PendingWalk::Fn(mi, fxi, f, class, ctor_of)) {
            return;
        }
        self.walk_fn_body(mi, fxi, f, class, ctor_of);
    }

    /// DECISIONS 225: whether the body of `f` waits (it is not active):
    /// kept for `walk_activated`, or, with the set final, never walked.
    pub(crate) fn defer_walk(&mut self, f: FnIdx, w: PendingWalk<'a>) -> bool {
        #[cfg(test)]
        if self.m.packets_inert && self.m.packet_scope.as_ref().is_some_and(|b| b.f != f) {
            self.pending_walk.entry(f).or_default().push(w);
            return true;
        }
        // D2: every body is generated; one not active is inert until it
        // activates.
        if let Some(a) = self.m.act.as_ref().filter(|a| a.inert) {
            let on = a.on.contains(&self.m.act_key(f));
            let a = self.m.act.as_mut().unwrap();
            let i = f as usize;
            if a.state.len() <= i {
                a.state.resize(i + 1, crate::model::FN_LIVE);
            }
            if !on && a.state[i] == crate::model::FN_LIVE {
                a.state[i] = crate::model::FN_INERT;
            }
            if a.state[i] == crate::model::FN_INERT && self.m.family_reach.on {
                self.inert_walks.push((f, w));
            }
            return false;
        }
        if self.m.body_active(f) {
            return false;
        }
        if !self.m.act.as_ref().is_some_and(|a| a.fixed) {
            self.pending_walk.entry(f).or_default().push(w);
        } else if self.m.family_reach.on {
            self.unwalked.push(w);
        }
        true
    }

    /// DECISIONS 225: walk the bodies activated since the last walk (their
    /// rules apply as generation emits them). Whether any was walked.
    pub(crate) fn walk_activated(&mut self) -> bool {
        let mut any = false;
        loop {
            let q = match self.m.act.as_mut() {
                Some(a) if !a.queue.is_empty() => std::mem::take(&mut a.queue),
                _ => return any,
            };
            for f in q {
                if self.m.aborted {
                    return any;
                }
                for w in self.pending_walk.remove(&f).unwrap_or_default() {
                    any = true;
                    match w {
                        PendingWalk::Fn(mi, fxi, func, class, ctor_of) => {
                            self.walk_fn_body(mi, fxi, func, class, ctor_of)
                        }
                        PendingWalk::Fields(mi, c) => self.walk_fields_body(mi, c),
                        PendingWalk::Closure(cfx, e) => self.walk_closure_body(&cfx, e),
                        #[cfg(test)]
                        PendingWalk::OwnedClosure(cfx, e) => self.walk_closure_body(&cfx, &e),
                        PendingWalk::Module(mi) => {
                            let f = self.init_fn_of[&mi];
                            self.walk_module(mi, f);
                        }
                    }
                }
            }
        }
    }

    /// A declared function's or method's body: never a once body, so its
    /// stores are unordered (RPG1b).
    fn walk_fn_body(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &'a Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        let saved = (self.order_ok, self.loop_depth);
        (self.order_ok, self.loop_depth) = (false, 0);
        let list_saved = self.list_enter_body();
        #[cfg(test)]
        let packet_prior = self.m.packet_body(fxi);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
        #[cfg(test)]
        self.m.packet_declarations(&f.body);
        // An async function body that runs untransformed throws into its
        // promise (`async_body`), never into its caller.
        let r = if f.is_async && !f.is_generator {
            self.thrown
        } else {
            self.m.raise_of(fxi)
        };
        let region = std::mem::replace(&mut self.m.gen_region, Some(r));
        self.walk_fn_body_inner(mi, fxi, f, class, ctor_of);
        self.m.gen_region = region;
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
        }
        (self.order_ok, self.loop_depth) = saved;
        self.list_body = list_saved;
    }

    fn walk_fn_body_inner(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &'a Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        self.m.note_walked(fxi);
        self.proto_first_stmts(mi, f);
        let info = self.m.funcs[fxi as usize].clone();
        let fn_ctor = class.is_none() && ctor_of.is_none();
        let mut fx = Fx {
            guard: None,
            live: None,
            module: mi,
            f: fxi,
            this: info.this,
            ret: info.ret,
            class,
            ctor_of,
            new_target: fn_ctor.then_some(fxi),
            fn_ctor,
        };
        if !f.is_strict && fn_ctor && crate::ctor_phase::body_mentions_this(&f.body) {
            // The view belongs to the function's body, like its parameters.
            let prev = std::mem::replace(&mut self.m.cur_fn, fxi);
            fx.this = self.sloppy_this(mi, info.this);
            self.m.cur_fn = prev;
        }
        if f.is_async && !f.is_generator {
            let (afx, rejected) = self.async_body(&fx);
            fx = afx;
            self.m.gen_region = Some(rejected);
        }
        self.params(&fx, &f.params);
        if f.is_generator {
            // Not transformed (rare at this point): results are runtime
            // objects.
            self.m.add_top(info.ret, |_| {
                "async/generator function result (untransformed)".into()
            });
        }
        if self.m.funcs[fxi as usize].forwards_super {
            // `run_ctor_chain` constructs the parent with the `new` call;
            // the rest of the body (capture rebinds and stashes) runs here.
            let rest = f.params[0].id;
            let body: Vec<Stmt> = f
                .body
                .iter()
                .filter(|s| !crate::gen::is_forward_of(s, rest))
                .cloned()
                .collect();
            self.stmts(&fx, &body);
            // Its normal completion returns `undefined`, which `new` turns
            // into the this binding (ECMA-262 10.2.2 step 12).
            if narrow::may_complete(&body) {
                self.m.add_prim(fx.ret, prim::UNDEF);
            }
            return;
        }
        let own = self.own_helper_shape(&f.params, &f.body, f.is_async || f.is_generator);
        self.note_own_stores(f);
        let scope = self.narrow_enter_body(fx.module, fxi, &f.params, &f.body, &f.captures);
        // Rule M1: a clause of an enclosing function's switch is not this
        // body's.
        let clause = self.m.clause.take();
        self.stmts(&fx, &f.body);
        self.m.clause = clause;
        if narrow::may_complete(&f.body) {
            // The body's normal completion returns `undefined`; for a
            // constructor `new` turns it into `this` (ECMA-262 10.2.2 steps
            // 10-12, `Model::ctor_this_result`).
            self.m.add_prim(fx.ret, prim::UNDEF);
        }
        self.narrow_exit_body(scope);
        self.own_helper_finish(fxi, own);
    }

    /// The `this` a sloppy function's body reads: its callers' `this`,
    /// where a nullish one is the realm's global this value and a primitive
    /// one is ToObject of it (ECMA-262 10.2.1.2 OrdinaryCallBindThis steps
    /// 5-6): a fresh wrapper object of the primitive's kind (7.1.18), one
    /// per function and kind. The function's own `this` node, its
    /// interface, stays what callers pass.
    pub(crate) fn sloppy_this(&mut self, mi: u32, this: NodeId) -> NodeId {
        let view = self.m.node_l("sloppy-this");
        self.m.own(view);
        self.m.edge(this, view);
        let global_tok = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
        let global = self.tok_node(global_tok);
        use crate::model::guard;
        let num = self.bobj(mi, "BoxedNumber").1;
        let str_ = self.bobj(mi, "BoxedString").1;
        let boolean = self.bobj(mi, "BoxedBoolean").1;
        // A BigInt or Symbol wrapper: an object whose kind the model does
        // not name, like `Object(prim)`.
        let other = self.bobj(mi, "Object(prim)").1;
        for (test, from) in [
            (guard::NULLISH, global),
            (guard::BOXED_NUM, num),
            (guard::BOXED_STR, str_),
            (guard::BOXED_BOOL, boolean),
            (guard::BOXED_OTHER, other),
        ] {
            self.m.cons(
                view,
                Cons::Guard {
                    test,
                    from,
                    dst: view,
                },
            );
        }
        view
    }

    pub(crate) fn params(&mut self, fx: &Fx, params: &[perry_hir::Param]) {
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        self.params_inner(fx, params);
        self.m.cur_fn = prev;
    }

    fn params_inner(&mut self, fx: &Fx, params: &[perry_hir::Param]) {
        for p in params {
            let n = self.local(fx.module, p.id);
            if let Some(d) = &p.default {
                // The default is the parameter's value only when a call
                // passes `undefined` or nothing (missing arguments are
                // `undefined`, `bind_args`). The calls pass to the argument
                // node (`register_fn`): the local holds what they pass but
                // `undefined`, and the default when one may pass it.
                let v = self.expr(fx, d);
                let arg = self
                    .default_args
                    .get(&(fx.module, p.id))
                    .copied()
                    .unwrap_or(n);
                if arg != n {
                    self.m.cons(arg, Cons::NotUndefined { dst: n });
                }
                self.m.cons(
                    arg,
                    Cons::Guard {
                        test: crate::model::guard::UNDEF,
                        from: v,
                        dst: n,
                    },
                );
            }
        }
    }
}
