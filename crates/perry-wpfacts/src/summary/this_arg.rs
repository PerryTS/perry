//! The `this` of a summarized call: its token, lookups and escapes.

use super::*;

impl Model {
    /// The `this` term of summarized `f`'s instances, if it has one.
    pub(super) fn has_this_term(&self, f: FnIdx) -> bool {
        self.sum.of.get(&f).is_some_and(|s| s.this != NO_NODE)
    }

    /// `this` of `call` into `f`: the shared node observes it; `f`'s
    /// instance for the call gets the explicit token (the call's own
    /// operand reaches it in `bind_summarized`).
    pub(crate) fn this_into(&mut self, call: CallId, ft: Tok, this_tok: Option<Tok>) {
        if self.act.is_some() {
            let f = self.tok_fn(ft);
            self.activate(f, "this");
        }
        if self.diagnostic.is_some() {
            let f = self.tok_fn(ft);
            self.diagnostic_reach(f, "this", call);
        }
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::This(call, ft, this_tok));
            return;
        }
        let f = self.tok_fn(ft);
        let fthis = self.funcs[f as usize].this;
        match (this_tok, &self.calls[call as usize].this) {
            (Some(x), _) => self.add_tok(fthis, x),
            (None, &ThisArg::Tok(x)) => self.add_tok(fthis, x),
            (None, &ThisArg::Node(n)) => self.edge(n, fthis),
            (None, ThisArg::Undef) => self.add_prim(fthis, prim::UNDEF),
        }
        if !self.has_this_term(f) || self.sum.templates.contains(&call) {
            return;
        }
        // The instance half: each receiver binds its layout's instance.
        match (this_tok, self.calls[call as usize].this.clone()) {
            (Some(x), _) | (None, ThisArg::Tok(x)) => self.bind_layout(call, ft, Some(x), 0),
            (None, ThisArg::Undef) => self.bind_layout(call, ft, None, prim::UNDEF),
            // A call bound through a method lookup gets each receiver from
            // the lookup (`this_calls`).
            (None, ThisArg::Node(_)) if self.relays.contains_key(&call) => {}
            (None, ThisArg::Node(n)) => {
                if self.sum.this_inst.insert((call, ft)) {
                    self.cons(n, Cons::ThisInst { call, ft });
                }
            }
        }
    }

    /// `this` of `f` from a method lookup on receiver `origin`: the shared
    /// node observes it; the instances get the call's receiver operand.
    pub(crate) fn this_lookup(&mut self, ft: Tok, origin: Tok) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::ThisTo(ft, origin));
            return;
        }
        let f = self.tok_fn(ft);
        let this = self.funcs[f as usize].this;
        self.add_tok(this, origin);
    }

    /// The node that takes `this` values of `f` no call is behind: every
    /// instance of `f` (and its shared instance, so the effect runs even
    /// if no call ever does) sees them.
    pub(crate) fn this_any(&mut self, ft: Tok) -> NodeId {
        let f = self.tok_fn(ft);
        let fthis = self.funcs[f as usize].this;
        if !self.sum.ready || !self.has_this_term(f) {
            return fthis;
        }
        if let Some(&u) = self.sum.this_any.get(&ft) {
            return u;
        }
        let u = self.node_named("this-any", || format!("this-any({f})"));
        self.sum.this_any.insert(ft, u);
        self.edge(u, fthis);
        // The receiver-less instance runs the effect even if no value does.
        if self.instantiable(ft) {
            self.instance((1, LAY_NONE, ft));
        }
        self.cons(u, Cons::ThisInst { call: NO_NODE, ft });
        u
    }

    /// An instance of `ft` has its outer terms: a closure with outer terms
    /// is instantiated only through a context (the token without one lives
    /// on observation nodes only, which never run effects).
    pub(super) fn instantiable(&self, ft: Tok) -> bool {
        match self.toks[ft as usize] {
            TokKind::Func(f, NO_CTX) => self.sum.of.get(&f).is_some_and(|s| s.outer.is_empty()),
            TokKind::Func(_, _) => true,
            _ => false,
        }
    }

    /// Unknown code calls `f`: its `this` is ⊤ (the unknown-code instance's
    /// and the shared node).
    /// DECISIONS 116: a contract method published with sealed classes'
    /// receivers only.
    pub(crate) fn sealed_method(&self, f: FnIdx) -> bool {
        self.contract_fns.contains(&f)
            && self.published_this.get(&f).is_some_and(|rs| {
                !rs.is_empty()
                    && rs.iter().all(|r| {
                        self.recv_class
                            .get(r)
                            .is_some_and(|c| self.sealed.contains(c))
                    })
            })
    }

    /// DECISIONS 125: a method run on published receivers runs on an
    /// instance `t` this unit gave out when the join names `t`'s class (its
    /// own token `Obj(tm, NO_CTX)` is in the join, `Gen::published_receivers`).
    pub(crate) fn join_holds(&self, f: FnIdx, t: Tok) -> bool {
        let Some(&n) = self.this_args.get(&f) else {
            return true;
        };
        let Some(tm) = self.tmpl_of(t) else {
            return false;
        };
        self.tok_index
            .get(&TokKind::Obj(tm, NO_CTX))
            .is_some_and(|&w| self.nodes[n as usize].pts.contains_tok(w))
    }

    /// The nodes `this` of function value `ft` reaches its body through:
    /// the shared node, and its instance's when the body is summarized.
    pub(crate) fn this_nodes(&mut self, ft: Tok) -> Vec<NodeId> {
        let f = self.tok_fn(ft);
        let mut dsts = vec![self.funcs[f as usize].this];
        if self.has_this_term(f) {
            let inst = self.instance((2, 0, ft));
            dsts.push(self.sum.insts[inst as usize].this);
        }
        dsts
    }

    pub(crate) fn escape_this(&mut self, ft: Tok, reason: u32) {
        self.diagnostic_entry_begin(ft, reason);
        self.escape_this_run(ft, reason);
        self.diagnostic_entry_end();
    }
    fn escape_this_run(&mut self, ft: Tok, reason: u32) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::EscapeThis(ft, reason));
            return;
        }
        let f = self.tok_fn(ft);
        let why = |m: &Self| format!("this of escaped function: {}", m.reasons[reason as usize]);
        let mut dsts = self.this_nodes(ft);
        // A constructor's field initializers run on every object it runs on
        // (before its body, as `run_ctor_chain` runs them for a `new`):
        // another unit's `super()` reaches them through the constructor's
        // contract, so their stores into the receiver are the contract's
        // (`this_stores`, DECISIONS 179), not only the body's.
        if let Some(fi) = self.funcs[f as usize]
            .ctor_of
            .filter(|&tm| self.tmpls[tm as usize].ctor == Some(f))
            .and_then(|tm| self.tmpls[tm as usize].field_init)
        {
            let fit = self.funcs[fi as usize].token;
            dsts.extend(self.this_nodes(fit));
        }
        let insts = self.published_this.get(&f).cloned().unwrap_or_default();
        // DECISIONS 116: a method of sealed classes only, published through
        // its contract, runs on the instances this unit gave out (and the
        // ones it gives out later, `give_out`): no other unit constructs or
        // extends them, and a reader's `ContractThis` admits nothing else.
        if self.pub_reason == Some(reason) && self.sealed_method(f) {
            let given: Vec<Tok> = insts
                .iter()
                .flat_map(|r| self.contract_recv.get(r).cloned().unwrap_or_default())
                .collect();
            for &d in &dsts {
                for &t in &given {
                    self.add_tok(d, t);
                }
            }
            self.given_this.entry(f).or_default().extend(dsts);
            return;
        }
        // A published method's receiver is one more contract parameter
        // (DECISIONS 57): its marker records what the body does to it, and
        // stands for instances of the classes it is published with
        // (DECISIONS 59).
        let mk = (!insts.is_empty() && self.contract_fns.contains(&f)).then(|| {
            let mk = self.param_marker_tok(f, u32::MAX);
            let classes: Vec<TmplId> = insts
                .iter()
                .filter_map(|r| self.recv_class.get(r).copied())
                .collect();
            self.register_virt(mk, &classes);
            mk
        });
        // Published through its contract, the receiver is the class's
        // instances (the receiver token) or what the marker stands for: a
        // reader's `ContractThis` admits nothing else, and reports a use
        // beyond that (DECISIONS 85). Any other escape lets unknown code
        // call it with any receiver.
        let contract_only = mk.is_some() && self.pub_reason == Some(reason);
        let joined = contract_only && self.this_args.contains_key(&f);
        if joined {
            self.given_this
                .entry(f)
                .or_default()
                .extend(dsts.iter().copied());
        }
        for d in dsts {
            if !contract_only {
                self.add_top(d, why);
                self.add_prim(d, prim::ALL);
            }
            // DECISIONS 107: the receivers the running units published.
            // DECISIONS 125: they include the instances this unit gave out
            // (a published class's instance another unit holds may be one
            // of them), and the ones it gives out later (`give_out`).
            match self.this_args.get(&f) {
                Some(&n) if joined => {
                    self.edge(n, d);
                    let given: Vec<Tok> = insts
                        .iter()
                        .flat_map(|r| self.contract_recv.get(r).cloned().unwrap_or_default())
                        .filter(|&t| self.join_holds(f, t))
                        .collect();
                    for t in given {
                        self.add_tok(d, t);
                    }
                }
                // DECISIONS 231(a): a constructor runs on its receiver
                // marker (another unit's `super()`) and on the instances
                // `new` makes (the contract's construction, DECISIONS 125),
                // never on the receiver token: no unit calls a constructor
                // as a method of an instance it holds.
                _ if self.funcs[f as usize].ctor_of.is_some() => {}
                _ => {
                    for &i in &insts {
                        self.add_tok(d, i);
                    }
                }
            }
            if let Some(mk) = mk {
                self.add_tok(d, mk);
            }
        }
    }
}
