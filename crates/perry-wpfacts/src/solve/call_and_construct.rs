//! Call binding, constructor flow and escaping receivers.
use super::*;

impl Model {
    /// Bind `call` to callee token `t`. `this_tok` overrides the call's this.
    pub fn call_tok(&mut self, call: CallId, t: Tok, this_tok: Option<Tok>) {
        self.diagnostic_callee(call, t);
        // A bare marker is another unit's value (DECISIONS 119): its call
        // is its contract's escape, and the call itself is one of an
        // unknown function (the reader cannot follow what it is given).
        if self.bare_marker(t) {
            // DECISIONS 140: a plain call of a parameter's own marker is a
            // call the caller's unit runs on the function it passed, with
            // the arguments as the contract describes them (checked as the
            // result is, `contract_check`): nothing escapes here. The
            // result is that unknown function's. An argument marker is a
            // call of each parameter it stands for (DECISIONS 44); a member
            // with values beside it (a published method's receiver and what
            // it reaches, DECISIONS 107) is described by them and adds no
            // callee of its own (DECISIONS 143): the values are called.
            let cs = &self.calls[call as usize];
            let plain =
                this_tok.is_none() && matches!(cs.this, ThisArg::Undef) && cs.spread.is_empty();
            if let Some(own) = plain.then(|| self.own_param_members(t)).flatten() {
                if self.bind_done.insert((call, t, u32::MAX - 3)) {
                    self.mark_use(t, crate::model::MARK_READ);
                    self.marker_call(own, None, call);
                }
                return;
            }
            self.mark_use(t, crate::model::MARK_ESCAPE);
            return self.call_tok(call, TOP, this_tok);
        }
        if matches!(self.toks[t as usize], TokKind::Top | TokKind::BuiltinFn) {
            // An unknown callee sees only its own receiver token: that one
            // escapes; arguments and result are per call.
            if let Some(x) = this_tok {
                let r = if self.explain {
                    let ret = self.calls[call as usize].ret;
                    let f = self.node_fn[ret as usize];
                    let why = format!(
                        "receiver of a call of an unknown function value ({} callee in {}, call {call}{}, receiver {})",
                        if t == TOP { "⊤" } else { "builtin-fn" },
                        self.funcs.get(f as usize).map_or("(no function)", |f| f.name.as_str()),
                        if self.protocol_calls.contains(&call) { ", protocol" } else { "" },
                        self.tok_name(x)
                    );
                    self.reason(&why)
                } else {
                    self.reason("receiver of a call of an unknown function value")
                };
                self.escape(x, r);
            }
            if !self.bind_done.insert((call, t, u32::MAX - 2)) {
                return;
            }
            self.unknown_callee_args(call, t, this_tok.is_none());
            return;
        }
        let key_this = this_tok.unwrap_or(u32::MAX);
        if !self.bind_done.insert((call, t, key_this)) {
            return;
        }
        match self.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn => unreachable!(),
            TokKind::Func(f, _) => {
                self.bind_args(call, t);
                if !self.funcs[f as usize].lexical_this {
                    self.this_into(call, t, this_tok);
                }
            }
            TokKind::Bound(_) => self.call_bound(call, t),
            TokKind::ClassObj(_) => {
                // Calling a class without `new` throws; nothing flows.
            }
            TokKind::Builtin(_) if self.protocol_calls.contains(&call) => {
                // The runtime's protocols (ToPrimitive, iteration, toJSON,
                // then) never call a constructor found under `constructor`.
            }
            TokKind::Builtin(ns) => self.with_b(|m| {
                let prev = std::mem::replace(&mut m.cur_this, this_tok);
                builtins::call_builtin_ns(m, ns, call);
                m.cur_this = prev;
            }),
            TokKind::Obj(_, _)
            | TokKind::Proto(_)
            | TokKind::ObjectProto
            | TokKind::NativeProto(_)
            | TokKind::ModNs(_) => {
                // Not callable (TypeError) — unless it is a builtin object
                // that is a function (Proxy over a function etc., ⊤ anyway).
                // A parameter marker stands for the caller's values: one may
                // be a function.
                self.mark_use(t, crate::model::MARK_ESCAPE);
            }
        }
    }

    /// The own parameters marker `t` stands for (DECISIONS 140): its
    /// members, each a contract function's own parameter marker that has
    /// not escaped; a member with values beside it adds none (DECISIONS
    /// 143). `None`: some member is no such marker, or none is left.
    pub(crate) fn own_param_members(&self, t: Tok) -> Option<Vec<(FnIdx, u32)>> {
        self.marker_members(t)
            .into_iter()
            .map(|m| match self.param_marker.get(&m) {
                _ if self.beside_values(m) && !self.unrun_param(m) => Some(None),
                Some(&(f, i))
                    if i != u32::MAX
                        && self.marker_of.get(&(f, i)) == Some(&m)
                        && !self.marker_is_top(m) =>
                {
                    Some(Some((f, i)))
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(|v| v.into_iter().flatten().collect::<Vec<_>>())
            .filter(|v| !v.is_empty())
    }

    /// Records a call through the parameters `own` (`key`: a method of
    /// theirs) for the caller's unit to run (DECISIONS 140/144); its
    /// result is that unknown code's.
    fn marker_call(&mut self, own: Vec<(FnIdx, u32)>, key: Option<Atom>, call: CallId) {
        for (f, i) in own {
            if !self.marker_calls.contains(&(f, i, key, call)) {
                self.marker_calls.push((f, i, key, call));
            }
        }
        let ret = self.calls[call as usize].ret;
        self.add_tok(ret, TOP);
        self.add_prim(ret, prim::ALL);
    }

    /// Unknown callee (⊤ or an opaque builtin function value): arguments
    /// escape, the result is ⊤; with `sink_this` (no receiver token given),
    /// the call's own `this` escapes too.
    pub(super) fn unknown_callee_args(&mut self, call: CallId, t: Tok, sink_this: bool) {
        self.diagnostic_callee(call, t);
        if t == TOP {
            self.top_calls += 1;
        }
        let r = if t == TOP {
            self.reason("passed to a call of an unknown function value")
        } else {
            self.reason("passed to an opaque builtin function value")
        };
        let na = self.calls[call as usize].args.len();
        for i in 0..na {
            let a = self.calls[call as usize].args[i];
            self.sink_node(a, r);
        }
        let ns = self.calls[call as usize].spread.len();
        for i in 0..ns {
            let a = self.calls[call as usize].spread[i];
            self.sink_node(a, r);
        }
        if sink_this {
            match self.calls[call as usize].this {
                ThisArg::Node(n) => self.sink_node(n, r),
                ThisArg::Tok(x) => self.escape(x, r),
                ThisArg::Undef => {}
            }
        }
        let ret = self.calls[call as usize].ret;
        if t == TOP {
            self.add_tok(ret, TOP);
        } else {
            self.add_top(ret, |_| "result of an opaque builtin function value".into());
        }
        self.add_prim(ret, prim::ALL);
    }

    /// Arguments and result of `call` into function token `t`: once per
    /// (call, function); only `this` differs between receivers.
    pub(crate) fn bind_args(&mut self, call: CallId, t: Tok) {
        self.diagnostic_callee(call, t);
        let TokKind::Func(f, _) = self.toks[t as usize] else {
            return;
        };
        self.activate(f, "call");
        if self.diagnostic.is_some() {
            let kind = if self.ctor_calls.contains(&call) {
                "new"
            } else {
                "call"
            };
            self.diagnostic_reach(f, kind, call);
        }
        if !self.ctor_calls.contains(&call) {
            self.called.insert(f);
        }
        if !self.sum.ready {
            // Summaries do not exist yet: bind once they do.
            self.sum
                .deferred
                .push(crate::summary::Deferred::Bind(call, t));
            return;
        }
        if self.sum.templates.contains(&call) {
            // A summary's call: its instances bind.
            return;
        }
        if !self.bind_done.insert((call, t, u32::MAX - 1)) {
            return;
        }
        if self.summarized(f) {
            self.bind_summarized(call, t);
            return;
        }
        let f = f as usize;
        let c = call as usize;
        let rest = self.funcs[f].rest;
        let args_all = self.funcs[f]
            .has_arguments
            .then_some(self.funcs[f].args_all);
        let nparams = self.funcs[f].params.len();
        let nargs = self.calls[c].args.len();
        // A hidden capture parameter holds its class environment slot,
        // not what the call passes (DECISIONS 117, 127).
        let caps: Vec<Option<NodeId>> = (0..nparams)
            .map(|i| self.capture_param(f as FnIdx, i))
            .collect();
        for i in 0..nargs {
            let a = self.calls[c].args[i];
            if i < nparams {
                let p = self.funcs[f].params[i];
                self.edge(caps[i].unwrap_or(a), p);
            } else if let Some(rest) = rest {
                let fa = self.field_any_node_of_node(rest);
                self.edge(a, fa);
            }
            if let Some(args_all) = args_all {
                self.edge(a, args_all);
            }
        }
        // Missing arguments are undefined.
        for i in nargs..nparams {
            let p = self.funcs[f].params[i];
            match caps[i] {
                Some(env) => self.edge(env, p),
                None => self.add_prim(p, prim::UNDEF),
            }
        }
        let nspread = self.calls[c].spread.len();
        for j in 0..nspread {
            let s = self.calls[c].spread[j];
            // Spread arrays: their elements may be any parameter.
            let elems = self.node_l("spread-elems");
            self.cons(
                s,
                Cons::Load {
                    key: Key::Any,
                    dst: elems,
                },
            );
            for i in 0..nparams {
                if caps[i].is_none() {
                    let p = self.funcs[f].params[i];
                    self.edge(elems, p);
                }
            }
            if let Some(rest) = rest {
                let fa = self.field_any_node_of_node(rest);
                self.edge(elems, fa);
            }
            if let Some(args_all) = args_all {
                self.edge(elems, args_all);
            }
        }
        let fret = self.funcs[f].ret;
        let cret = self.calls[c].ret;
        let params = self.funcs[f].params.clone();
        if !self.rel_into(call, FN_RUN | f as u32, &params) {
            self.edge(fret, cret);
        }
    }

    /// Calling a bound function: its target with the bound this; bound +
    /// call args land in any parameter (positions shift).
    pub(super) fn call_bound(&mut self, call: CallId, t: Tok) {
        self.diagnostic_callee(call, t);
        let TokKind::Bound(b) = self.toks[t as usize] else {
            return;
        };
        if self.bound_synth.contains(&call) {
            self.into_merged(call, b, false);
            return;
        }
        let c = self.calls[call as usize].clone();
        let (target, bthis, bargs) = self.bound[b as usize].clone();
        let all_args = self.node_l("bound-args");
        for &a in bargs.iter().chain(c.args.iter()).chain(c.spread.iter()) {
            self.edge(a, all_args);
        }
        let mut args: Vec<NodeId> = bargs.clone();
        args.extend(c.args.iter().copied());
        let call2 = self.solver_call(
            call,
            CallSite {
                this: ThisArg::Node(bthis),
                args,
                spread: c.spread.clone(),
                ret: c.ret,
                site: c.site,
            },
        );
        self.bound_synth.insert(call2);
        self.rel(|| format!("boundcall {call2} {call} {b}"));
        // Positions shift when the bound args are not a prefix of the
        // target's params; the conservative cover: every arg to every
        // param, done by spreading `all_args` as an array.
        let arr = self.node_l("bound-arr");
        let holder = self.tok(TokKind::Bound(b));
        let fa = self.field_any(holder);
        self.edge(all_args, fa);
        self.add_tok(arr, holder);
        self.calls[call2 as usize].spread.push(arr);
        self.cons(target, Cons::Call { call: call2 });
    }

    /// Bound value `b`'s merged call (`new` or not): bound args first, then
    /// every argument of every call routed here as a spread (the target
    /// may itself hold bound values, so these calls must not nest).
    fn bound_merged(&mut self, b: u32, new: bool) -> CallId {
        if let Some(&c) = self.bound_merged.get(&(b, new)) {
            return c;
        }
        let (target, bthis, bargs) = self.bound[b as usize].clone();
        let holder = self.tok(TokKind::Bound(b));
        let arr = self.node_l("bound-arr");
        self.add_tok(arr, holder);
        let sp = self.node_l("bound-spread");
        let ret = self.node_l("bound-ret");
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            this: if new {
                ThisArg::Undef
            } else {
                ThisArg::Node(bthis)
            },
            args: bargs,
            spread: vec![arr, sp],
            ret,
            site: 0,
        });
        self.bound_merged.insert((b, new), call);
        self.bound_synth.insert(call);
        self.rel(|| format!("boundmerged {call} {b} {}", new as u8));
        self.cons(
            target,
            if new {
                Cons::New { call }
            } else {
                Cons::Call { call }
            },
        );
        call
    }

    /// A solver-made call `call` through bound value `b`: its arguments
    /// join the merged call's, its result is the merged call's.
    fn into_merged(&mut self, call: CallId, b: u32, new: bool) {
        let m = self.bound_merged(b, new);
        if m == call {
            return;
        }
        let c = self.calls[call as usize].clone();
        let (sp, ret) = (self.calls[m as usize].spread[1], self.calls[m as usize].ret);
        let holder = self.tok(TokKind::Bound(b));
        let fa = self.field_any(holder);
        for &a in &c.args {
            self.edge(a, fa);
        }
        for &s in &c.spread {
            self.edge(s, sp);
        }
        self.edge(ret, c.ret);
    }

    pub(super) fn field_any_node_of_node(&mut self, arr_node: NodeId) -> NodeId {
        // Elements of whatever array the node holds: a store relay.
        let relay = self.node_named("elems-into", || format!("elems-into({arr_node})"));
        let site = self.anon_store_site();
        self.cons(
            arr_node,
            Cons::Store {
                key: Key::Any,
                src: relay,
                site,
            },
        );
        relay
    }

    pub fn anon_store_site(&mut self) -> SiteId {
        // A store performed by call binding (rest arrays): never a template
        // writer, because rest arrays are Arr templates.
        let id = self.stores.len() as SiteId;
        self.stores.push(StoreSite {
            module: 0,
            func: 0,
            desc: "binding".into(),
            ctor_phase_of: None,
            birth: true,
            ctor_fn: None,
            rooted: false,
            deep: false,
            key_value: None,
        });
        id
    }

    /// `new t(args)`.
    /// `new Name(...)` of a builtin constructor, once per call.
    pub(crate) fn new_named(&mut self, name: Atom, call: CallId) {
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::NewNamed(name, call));
            return;
        }
        if !self.construct_done.insert((call, TOP)) {
            return;
        }
        let name = self.atoms[name as usize].clone();
        self.with_b(|m| builtins::construct_named(m, &name, call));
    }

    /// The token `new` at `call` gives template `tm`: one per birth site. A
    /// template-wide mark already on the template's own token holds for it.
    pub(crate) fn site_tok(&mut self, tm: TmplId, call: CallId) -> Tok {
        // Another unit's class: one token for all its instances here (its
        // facts are its defining unit's). The instance other units construct
        // of a published class is its own token (DECISIONS 125): what the
        // constructor does to it happens to the instance its methods run on.
        let call = if self.foreign.contains_key(&tm) || self.contract_new.contains(&call) {
            NO_CTX
        } else {
            call
        };
        let k = TokKind::Obj(tm, call);
        if let Some(&t) = self.tok_index.get(&k) {
            return t;
        }
        let t = self.tok(k);
        if let Some(&w) = self.tok_index.get(&TokKind::Obj(tm, NO_CTX)) {
            if let Some(&r) = self.escaped.get(&w) {
                self.escape(t, r);
            }
            if let Some(&(kind, r)) = self.hard.get(&w) {
                self.mark_hard(t, kind, r);
            }
        }
        t
    }

    /// Birth-site tokens a template-wide mark on `t` also holds for.
    fn wide_sites(&self, t: Tok) -> Vec<Tok> {
        match self.toks[t as usize] {
            TokKind::Obj(tm, NO_CTX) => self.site_toks.get(&tm).cloned().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn chain_has_foreign_returning_ctor(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if self.foreign_ctor_returns.contains(&c) {
                return true;
            }
            cur = self.tmpls[c as usize].parent;
        }
        false
    }

    pub fn construct(&mut self, call: CallId, t: Tok) {
        self.diagnostic_callee(call, t);
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::Construct(call, t));
            return;
        }
        if !self.construct_done.insert((call, t)) {
            return;
        }
        // `new` of a bare marker: of an unknown constructor (DECISIONS 119).
        if self.bare_marker(t) {
            self.mark_use(t, crate::model::MARK_ESCAPE);
            return self.construct(call, TOP);
        }
        let c = self.calls[call as usize].clone();
        match self.toks[t as usize].clone() {
            TokKind::ClassObj(tm) => {
                let ctx = self.birth_ctx(call);
                let inst = self.site_tok(tm, ctx);
                self.add_tok(c.ret, inst);
                self.run_ctor_chain(call, tm, inst);
                // Another unit's constructor up the chain returns an object:
                // `new` yields that value, which this unit cannot name.
                if self.chain_has_foreign_returning_ctor(tm) {
                    self.add_top(c.ret, |_| {
                        "`new` of a class whose constructor (another unit's) returns an object"
                            .into()
                    });
                }
            }
            // DECISIONS 210: another unit's contract function: its instances
            // are its owner's `new <f>` layout, which the owner constructs
            // with what this unit passes (published, `constructed`). The
            // contract's argument effects happen here; no receiver of this
            // unit runs the body (a receiver of this unit escaped the
            // function through the `.constructor` rule and broke it). Until
            // the owner publishes the layout the result is unknown.
            TokKind::Func(f, _) if self.foreign_ctor.contains_key(&f) => {
                self.constructed.insert(f);
                self.ctor_calls.insert(call);
                match self.foreign_ctor[&f] {
                    Some((n, _)) => self.edge(n, c.ret),
                    None => self.add_top(c.ret, |_| {
                        "instance of another unit's constructor function (layout not published)"
                            .into()
                    }),
                }
                self.call_tok(call, t, None);
                self.ctor_ret_objects(call, t, c.ret);
            }
            TokKind::Func(f, _) => {
                let tm = self.fn_inst_template(f);
                let ctx = self.birth_ctx(call);
                let inst = self.site_tok(tm, ctx);
                self.add_tok(c.ret, inst);
                self.call_tok(call, t, Some(inst));
                // E12: a constructor function returning an object.
                self.ctor_ret_objects(call, t, c.ret);
            }
            TokKind::Top | TokKind::BuiltinFn => {
                if t == TOP {
                    self.top_calls += 1;
                }
                let r = self.reason("passed to `new` of an unknown constructor");
                for &a in c.args.iter().chain(c.spread.iter()) {
                    self.sink_node(a, r);
                }
                if t == TOP {
                    self.add_tok(c.ret, TOP);
                } else {
                    self.add_top(c.ret, |_| {
                        "`new` of an opaque builtin function value".into()
                    });
                }
            }
            TokKind::Bound(b) => {
                if self.bound_synth.contains(&call) {
                    self.into_merged(call, b, true);
                    return;
                }
                let (target, _bthis, bargs) = self.bound[b as usize].clone();
                let mut args = bargs;
                args.extend(c.args.iter().copied());
                let call2 = self.solver_call(
                    call,
                    CallSite {
                        this: ThisArg::Undef,
                        args,
                        spread: c.spread.clone(),
                        ret: c.ret,
                        site: c.site,
                    },
                );
                self.bound_synth.insert(call2);
                self.rel(|| format!("boundnew {call2} {call} {b}"));
                self.cons(target, Cons::New { call: call2 });
            }
            TokKind::Builtin(ns) => self.with_b(|m| builtins::new_builtin_ns(m, ns, call)),
            _ => {}
        }
    }

    /// `super(...)` from a subclass constructor: the parent chain runs on
    /// the same instance; the `new` result is the subclass's.
    pub fn run_ctor_chain_this(&mut self, call: CallId, tm: TmplId, inst: Tok) {
        self.run_ctor_chain(call, tm, inst);
    }

    /// Run the constructor chain of class template `tm` for instance `inst`.
    pub fn run_ctor_chain(&mut self, call: CallId, tm: TmplId, inst: Tok) {
        if !self.constructor_this_done.insert((call, tm, inst)) {
            return;
        }
        let c = self.calls[call as usize].clone();
        if let Some(fi) = self.tmpls[tm as usize].field_init {
            // The field initializers run for this `new`, on its instance.
            let ft = self.funcs[fi as usize].token;
            self.this_into(call, ft, Some(inst));
        }
        let t = &self.tmpls[tm as usize];
        let parent_value = t.parent_value.is_some();
        match (t.ctor, t.parent, t.native_parent.clone()) {
            (Some(ctor), _, _) => {
                let ftok = self.funcs[ctor as usize].token;
                self.call_tok(call, ftok, Some(inst));
                self.ctor_ret_objects(call, ftok, c.ret);
                if self.funcs[ctor as usize].forwards_super {
                    // `constructor(...a) { super(...a) }`: the parent runs
                    // with this `new` call's arguments (DECISIONS 49).
                    let receiver = self.node_l("forwarded-super-this");
                    self.cons(receiver, Cons::SuperCtor { class: tm, call });
                    self.add_tok(receiver, inst);
                }
            }
            (None, Some(_), _) => {
                // Implicit `constructor(...a) { super(...a) }`.
                let receiver = self.node_l("implicit-super-this");
                self.cons(receiver, Cons::SuperCtor { class: tm, call });
                self.add_tok(receiver, inst);
            }
            (None, None, Some(_)) => {
                self.with_b(|m| builtins::native_parent_ctor(m, tm, call, inst))
            }
            (None, None, None) if parent_value => {
                // The same, the parent being the value of `extends` (§17.2).
                let receiver = self.node_l("implicit-super-this");
                self.cons(receiver, Cons::SuperCtor { class: tm, call });
                self.add_tok(receiver, inst);
            }
            (None, None, None) => {}
        }
    }

    /// Whether constructor `f` may return a value `new` keeps instead of its
    /// `this`: an object (or ⊤) other than the receiver itself. A primitive
    /// return is ignored by `new`.
    pub(crate) fn ctor_returns_object(&self, f: FnIdx) -> bool {
        let fi = &self.funcs[f as usize];
        if fi.ret == NO_NODE {
            return false;
        }
        let this = (fi.this != NO_NODE).then(|| self.nodes[fi.this as usize].pts.to_sorted());
        self.nodes[fi.ret as usize]
            .pts
            .to_sorted()
            .iter()
            .any(|t| !this.as_ref().is_some_and(|ts| ts.contains(t)))
    }

    /// The objects constructor `f` returns for `call` (its instance's
    /// result when summarized, so known once bindings run).
    pub(crate) fn ctor_ret_objects(&mut self, call: CallId, f: Tok, dst: NodeId) {
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::RetObjects(call, f, dst));
            return;
        }
        let r = self.ret_for(call, f);
        if r != NO_NODE {
            self.ret_objects(r, dst);
        }
    }

    /// Objects a constructor returns replace `this` as the `new` result.
    pub fn ret_objects(&mut self, ret: NodeId, dst: NodeId) {
        // Object tokens only (a primitive return is ignored by `new`).
        let filt = self.node_named("objret", || format!("objret({ret})"));
        self.edge(ret, filt);
        // `filt` keeps prims; dst gets them too, which is a harmless
        // over-approximation for kinds only when the ctor returns
        // primitives — but it would pollute `new` results, so relay
        // tokens only through a Load-free copy: we accept the prims.
        self.edge(filt, dst);
    }

    pub fn fn_inst_template(&mut self, f: FnIdx) -> TmplId {
        if let Some(&tm) = self.fn_inst.get(&f) {
            return tm;
        }
        let name = self.funcs[f as usize].name.clone();
        let id = self.tmpls.len() as TmplId;
        let (birth_keys, definite) = self.fn_birth.get(&f).cloned().unwrap_or_default();
        self.tmpls.push(Template {
            kind: TmplKind::FnInst { func: f },
            display: format!("new {name}"),
            birth_keys,
            definite,
            declared: Vec::new(),
            parent: None,
            ctor: Some(f),
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
        });
        self.fn_inst.insert(f, id);
        // DECISIONS 210: a layout made for a function that already left.
        let ft = self.funcs[f as usize].token;
        if let Some(&r) = self.escaped.get(&ft) {
            self.construct_unknown(ft, f, r);
        }
        id
    }

    /// `recv.key(args)` on token `t`.
    pub(super) fn method_on(&mut self, t: Tok, key: Key, call: CallId) {
        // A method of a published receiver's class (DECISIONS 59).
        let virt = match key {
            Key::Named(a) if t != TOP => self.virt_load(t, a),
            _ => None,
        };
        // A builtin protocol call (ToPrimitive, iteration ...) is a touch.
        // A call of a receiver marker's class method is a read: the callee
        // runs on the marker, so what it does to it is its own use.
        let bits = if self.protocol_calls.contains(&call) {
            crate::model::MARK_TOUCH
        } else if virt.is_some() {
            crate::model::MARK_READ
        } else {
            crate::model::MARK_ESCAPE
        };
        // DECISIONS 144: a method call on a parameter's own marker
        // (`p.forEach(cb)`, `key.compareTo(k)`) is a call the caller's unit
        // runs on the value it passed, as a plain call through it is
        // (DECISIONS 140): its receiver use is a read of the key, the
        // arguments go to the caller's unit described as the result is.
        if let (Key::Named(a), None, false) = (key, virt, self.protocol_calls.contains(&call)) {
            let plain = self.calls[call as usize].spread.is_empty();
            if let Some(own) = plain.then(|| self.own_param_members(t)).flatten() {
                if self.bind_done.insert((call, t, u32::MAX - 4)) {
                    self.mark_use(t, crate::model::MARK_READ);
                    let r = self.keyed_reach_of(t, a);
                    self.mark_use(r, crate::model::MARK_READ);
                    self.marker_call(own, Some(a), call);
                }
                return;
            }
        }
        self.mark_use(t, bits);
        if t == TOP {
            self.call_tok(call, TOP, Some(TOP));
            return;
        }
        // A marker is use tracking, not a value (DECISIONS 111): the values
        // it stands for sit beside it and are called there. It has no chain
        // and no accessors; its method is what it reaches under the key,
        // and calling that is an escape of it (`call_tok`). One with its
        // callers' values beside it (a described parameter: DECISIONS 13,
        // 107, 148) adds no callee of its own (DECISIONS 143): the call is
        // made on those values, whose contract carries it to their unit.
        if self.is_marker(t) {
            if self.beside_values(t) {
                return;
            }
            let m = match key {
                Key::Named(a) => self.marker_load(t, Some(a)),
                Key::Any => self.reach_of(t),
            };
            self.mark_use(m, crate::model::MARK_ESCAPE);
            // That method is an unknown function, as a plain call through a
            // marker is (`call_tok`): the arguments escape, the result is
            // unknown (DECISIONS 202).
            if self.bind_done.insert((call, TOP, u32::MAX - 2)) {
                self.unknown_callee_args(call, TOP, false);
            }
            return;
        }
        // Own function-valued fields, then the prototype chain.
        let own = match (key, virt) {
            (Key::Named(_), Some(v)) => v,
            (Key::Named(a), None) => self.field(t, a),
            (Key::Any, _) => self.all_fields(t),
        };
        self.bind_via(own, call, t);
        // A key only another unit's subclass defines runs that subclass's
        // method (DECISIONS 97).
        if let (Key::Named(a), Some(_)) = (key, virt) {
            for n in self.virt_call(t, a) {
                self.bind_via(n, call, t);
            }
        }
        // A parameter marker has no prototype here (DECISIONS 119,
        // `load_from`); a receiver marker has its classes'.
        if self.is_marker(t) && !self.virt_protos.contains_key(&t) {
            return;
        }
        // A receiver token / marker is no builtin object: its methods are
        // its classes' (`virt_protos`).
        if !self.virt_protos.contains_key(&t) {
            self.with_b(|m| builtins::own_method(m, t, key, call));
        }
        // Methods on the chain (shared), builtin prototype methods, and
        // getters returning functions (per receiver, accessor keys only).
        let cv = self.cv(t, key);
        self.bind_via(cv, call, t);
        let cp = self.cp(t);
        self.cons(
            cp,
            Cons::ProtoMethods {
                origin: t,
                key,
                call,
            },
        );
        self.accessor_walk(t, t, key, ChainAct::Call(call));
    }

    // --------------------------------------------------------------- escape

    /// Escapes `t`, and whether that took effect: a stand-in published
    /// onward does not escape (`escape`), so only an escape that took
    /// effect is a change of a unit's contract check (DECISIONS 180).
    pub fn escape_changed(&mut self, t: Tok, reason: u32) -> bool {
        if self.is_escaped(t) {
            return false;
        }
        self.escape(t, reason);
        self.is_escaped(t)
    }

    pub fn escape(&mut self, t: Tok, reason: u32) {
        // DECISIONS 225: unknown code may call an escaped function.
        if self.act.is_some() {
            if let TokKind::Func(f, _) = self.toks[t as usize] {
                self.activate(f, "escape");
            }
        }
        if self.diagnostic.is_none() {
            return self.escape_body(t, reason);
        }
        let first = self.diagnostic_escape_begin(t, reason);
        self.escape_body(t, reason);
        self.diagnostic_escape_end(t, first);
    }

    fn escape_body(&mut self, t: Tok, reason: u32) {
        if t == TOP {
            return;
        }
        // Another unit's function the census broke, forwarded where no
        // contract describes it: its owner already lets unknown code call it
        // (any receiver, any arguments, DECISIONS 209), so its leaving tells
        // nothing more, and no class it is a member of leaves with it
        // (DECISIONS 212).
        if let TokKind::Func(f, _) = self.toks[t as usize] {
            let fi = &self.funcs[f as usize];
            if fi.func_id == u32::MAX
                && !self.broken_fns.is_empty()
                && self.broken_fns.contains(&format!("fn:{}", fi.name))
                && self.reasons[reason as usize] == crate::gen::UNDESCRIBED
            {
                return;
            }
        }
        if let Some(&old) = self.escaped.get(&t) {
            let contract = |m: &Self, r: u32| {
                m.pub_reason == Some(r)
                    || m.field_fn_reason == Some(r)
                    || m.closed_pub_reason == Some(r)
            };
            if contract(self, old) && !contract(self, reason) {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    // DECISIONS 212: a contract function that left through
                    // its contract only and now also through no contract (a
                    // value no contract describes, DECISIONS 203; unknown
                    // code) is called by that code with any receiver and
                    // any arguments, which no unit publishes: its `this` and
                    // parameters take ⊤ beside what the contract gave them.
                    if self.contract_fns.contains(&f) && self.funcs[f as usize].func_id != u32::MAX
                    {
                        self.escaped.insert(t, reason);
                        if !self.funcs[f as usize].lexical_this {
                            self.escape_this(t, reason);
                        }
                        self.escape_fn_params(t, reason);
                    }
                    // DECISIONS 210: one that left with its properties
                    // (`escape_props`): that code may construct it, which no
                    // unit publishes (`ctor_runs`).
                    if self.props_escaped.contains_key(&t) {
                        self.construct_unknown(t, f, reason);
                    }
                }
            }
            return;
        }
        // Another unit's class, instance, prototype or function (a
        // stand-in) published onward leaves with its owner's contract, as a
        // result forwards it (DECISIONS 79/104; DECISIONS 120: every
        // publication): the next unit reads it through that contract,
        // nothing of this unit leaves with it, and it is no census report
        // of this unit. What this unit stores into it is (`Gen::reports`).
        // A closed contract's users are known to its owner: it escapes.
        // Interpreted code would hold it beyond any contract (decision 150).
        if self.interp_free
            && self.dyn_eval.is_none()
            && (self.pub_reason == Some(reason) || self.closed_pub_reason == Some(reason))
            && !self.is_cell_arr(t)
            && self.stand_in(t)
        {
            return;
        }
        // DECISIONS 174: another unit's value read through cell `thrown`
        // reaches interpreted code as that unit threw it. The first unit to
        // throw it escaped it (each thrower sinks what it throws), and that
        // is its write; the others re-escaped it, each publishing ⊤ for
        // what it reaches (opencode: every unit ⊤ for the ~210 arrays the
        // program throws, 27M cell writes in round 3).
        if self.thrown_reason == Some(reason) && self.thrown_in.contains(&t) && self.stand_in(t) {
            return;
        }
        if self.closed_pub_reason == Some(reason) {
            if let TokKind::Func(f, _) = self.toks[t as usize] {
                if self.closed_params.contains_key(&f) {
                    if self.closed_pub.insert(t) {
                        self.escape_fn_params(t, reason);
                    }
                    return;
                }
            }
        }
        // DECISIONS 210: a function of this unit published through its
        // contract (`contract_fns`; an export, a published literal's field,
        // a contract result): its code is the contract (markers on its
        // parameters, any receiver), and its own properties leave with it
        // (`escape_props`), all but `prototype`: a reader reaches that only
        // as the owner's published `new <f>` layout's (`foreign_ctor`), a
        // store into it or a read of it before the layout exists is its
        // census report (`fn_touched`). So the instances this unit
        // constructs keep their chain, as a published class's do.
        if self.pub_reason == Some(reason) || self.field_fn_reason == Some(reason) {
            if let TokKind::Func(f, _) = self.toks[t as usize] {
                if self.contract_fns.contains(&f) && self.funcs[f as usize].func_id != u32::MAX {
                    self.mark_use(t, crate::model::MARK_ESCAPE);
                    self.mark_escaped(t, reason);
                    self.rel(|| format!("escaped {t}"));
                    // Its own properties are read by code no contract
                    // describes (DECISIONS 212).
                    let rp = self.beyond_contract(reason);
                    self.escape_props(t, rp);
                    if !self.funcs[f as usize].lexical_this {
                        self.escape_this(t, reason);
                    }
                    self.escape_fn_params(t, reason);
                    return;
                }
            }
        }
        // A builtin prototype as the operand of a compiled construct the
        // model does not follow (`Gen::fail_closed`) may get any key written
        // (`fnproto_write` fails closed). The runtime's own operations and
        // callbacks are not that: they never write the intrinsics, and a
        // compiled function called from unknown code sees ⊤ parameters, whose
        // writes `store_into` records.
        if self.fnproto_write.is_none()
            && matches!(self.toks[t as usize], TokKind::ObjectProto)
            && self.reasons[reason as usize].starts_with(crate::gen_expr::UNMODELED_OPERAND)
        {
            let what = format!("reaches unmodeled code ({})", self.reasons[reason as usize]);
            self.note_fnproto_write(t, None, &what);
        }
        if matches!(
            self.toks[t as usize],
            TokKind::ObjectProto | TokKind::NativeProto(_)
        ) && self.reasons[reason as usize].starts_with(crate::gen_expr::UNMODELED_OPERAND)
        {
            let what = format!("reaches unmodeled code ({})", self.reasons[reason as usize]);
            crate::builtins::note_native_write(self, t, None, &what);
            if let Some(s) = self.shared_of(t) {
                self.note_shared(SharedWrite::Unmodeled(s));
            }
        }
        if matches!(
            self.toks[t as usize],
            TokKind::Builtin(BuiltinNs::GlobalThis)
        ) {
            crate::builtins::note_global_in_top(self);
            self.note_shared(SharedWrite::Unmodeled(Shared::Builtin(
                BuiltinNs::GlobalThis,
            )));
        }
        // Per unit, unknown code includes the other units' code: a
        // runtime-owned object escaping reaches code that holds it as ⊤, and
        // that code's stores into ⊤ may land in it (`units::shared_closure`).
        // A value contract names one instead (`ValueContract::shared`): its
        // readers hold it, nothing escapes. The runtime's own code never
        // writes one (above). One reached along an escaped value's prototype
        // chain is not given: a store into ⊤ is no store into a builtin
        // prototype, as whole-program facts hold of ⊤ from unknown code. The
        // global object takes no part: escaped, it already puts ⊤ in reach
        // of stores into ⊤ (`global_in_top`).
        if !self.chain_escape {
            if let Some(s) = self
                .shared_of(t)
                .filter(|s| *s != Shared::Builtin(BuiltinNs::GlobalThis))
            {
                self.note_shared(SharedWrite::Handed(s));
            }
        }
        // Runtime-owned objects do not "escape": unknown code already holds
        // them. Object.prototype's unmodeled mutations are E5/E6 (modeled
        // operations) or E14 (interpreted code), never a side effect of some
        // value reaching unknown code; its user-stored members escape where
        // they are stored (`builtins::tok_store`).
        if matches!(
            self.toks[t as usize],
            TokKind::ObjectProto
                | TokKind::BuiltinFn
                | TokKind::Builtin(_)
                | TokKind::NativeProto(_)
        ) {
            return;
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("escapeseed {t} {reason}"));
        }
        self.with_s(|m| m.escape_inner(t, reason));
        for s in self.wide_sites(t) {
            self.escape(s, reason);
        }
    }

    /// Another unit's class, instance, prototype, function or array this
    /// unit holds through a contract (a stand-in).
    fn stand_in(&self, t: Tok) -> bool {
        let fwd = |f: Option<FnIdx>| f.is_some_and(|f| self.funcs[f as usize].forwarded);
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) if self.is_cell_arr(t) => matches!(
                self.tmpls[tm as usize].kind,
                TmplKind::Arr { site: u32::MAX, .. } | TmplKind::BObj { site: u32::MAX, .. }
            ),
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) | TokKind::Proto(tm) => {
                self.foreign.contains_key(&tm) && fwd(self.tmpls[tm as usize].ctor)
            }
            TokKind::Func(f, _) => fwd(Some(f)),
            _ => false,
        }
    }

    /// DECISIONS 212: the reason what a value escaping with `reason`
    /// reaches escapes with: a publication (`pub_reason`, a published
    /// literal's field function) describes the value itself, not what
    /// unknown code then reads from it (DECISIONS 203, `UNDESCRIBED`).
    pub(crate) fn beyond_contract(&mut self, reason: u32) -> u32 {
        match self.pub_reason == Some(reason) || self.field_fn_reason == Some(reason) {
            true => self.reason(crate::gen::UNDESCRIBED),
            false => reason,
        }
    }

    pub(super) fn mark_escaped(&mut self, t: Tok, reason: u32) {
        self.escaped.insert(t, reason);
        let w = (t >> 6) as usize;
        if self.esc_bits.len() <= w {
            self.esc_bits.resize(w + 1, 0);
        }
        self.esc_bits[w] |= 1 << (t & 63);
    }

    /// Unknown code calls `f`: unknown arguments, the result escapes.
    pub(crate) fn escape_fn_params(&mut self, ft: Tok, reason: u32) {
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::Escape(ft, reason));
            return;
        }
        let f = self.tok_fn(ft);
        if self.summarized(f) {
            self.escape_summarized(ft, reason);
            return;
        }
        let fi = self.funcs[f as usize].clone();
        let why = |m: &Self| format!("param of escaped function: {}", m.reasons[reason as usize]);
        // A closed function's callers are known: what they pass. Only
        // through its publication (decision 19); any other escape lets
        // unknown code call it.
        let closed = if self.closed_pub_reason == Some(reason) {
            self.closed_params.get(&f).cloned()
        } else {
            None
        };
        // DECISIONS 119: published through its contract only, a parameter
        // is its marker, the whole argument: no ⊤, no primitive kinds.
        let contract_only = self.contract_fns.contains(&f) && self.pub_reason == Some(reason);
        for (i, &p) in fi.params.iter().enumerate() {
            // A hidden capture parameter holds its class environment slot
            // whoever calls (DECISIONS 117).
            if let Some(env) = self.capture_param(f, i) {
                self.edge(env, p);
                continue;
            }
            match closed.as_ref().and_then(|c| c.get(i).copied()) {
                Some(a) => self.edge(a, p),
                None => {
                    if !contract_only {
                        self.add_top(p, why);
                        self.add_prim(p, prim::ALL);
                    }
                }
            }
        }
        // The rest parameter is the fresh array the call makes, whoever
        // calls: unknown arguments are its elements, never the array itself
        // (a store into its elements is no store into ⊤).
        if let Some(rest) = fi.rest {
            let r = self.field_any_node_of_node(rest);
            self.add_top(r, why);
            self.add_prim(r, prim::ALL);
        }
        if self.contract_fns.contains(&f) {
            // Design §11.2: other units call it through its contract.
            for (i, &p) in fi.params.iter().enumerate() {
                if self.capture_param(f, i).is_some() {
                    continue;
                }
                let mk = self.param_marker_tok(f, i as u32);
                self.add_tok(p, mk);
                // DECISIONS 148: what the running units pass, beside it.
                if contract_only {
                    if let Some(&a) = self.method_params.get(&f).and_then(|v| v.get(i)) {
                        if a != NO_NODE {
                            self.edge(a, p);
                        }
                    }
                }
            }
            self.add_top(fi.args_all, why);
            self.contract_rets.push((f, fi.ret));
            // Beyond its publication (an export or a published literal's
            // field, DECISIONS 210, or known closed callers, DECISIONS 19),
            // unknown code calls it and retains its result.
            if self.pub_reason != Some(reason)
                && self.field_fn_reason != Some(reason)
                && closed.is_none()
            {
                self.sink_node(fi.ret, reason);
            }
            return;
        }
        self.add_top(fi.args_all, why);
        self.sink_node(fi.ret, reason);
    }

    /// A class published to other units as a contract (design §11.2), not
    /// an escape: they may construct it with unknown arguments, read every
    /// field of what they construct (getters run, contents leave), call its
    /// methods and accessors on those instances, and use its static members
    /// as unknown code does. Its methods run with `this` = ⊤ (any receiver)
    /// and that instance. Everything else they do to it (stores, opening
    /// operations, the prototype or a method used as a value, passing it to
    /// code outside the program) is their census report; a broken class is
    /// published as an escape instead.
    ///
    /// A method called by another unit runs on one of those instances: its
    /// `this` holds the class's receiver token. Reading through it gives ⊤
    /// (as reading through ⊤ does: the method sees whatever other units did),
    /// and storing into it, an opening operation on it or its escape happen
    /// to the constructed instance (`contract_recv`). Reading the instance's
    /// own contents there would run every method over every value other
    /// units can put in it (commander's Command: 217 receiver layouts per
    /// method).
    ///
    /// When a class of another unit extends it (`extended`), its
    /// constructor is a function contract as well, run on any receiver (a
    /// subclass instance's `super()`): `this` is ⊤ there, so its sites are
    /// proven behind the entry guard only (DECISIONS 57). A class with no
    /// constructor code publishes an empty constructor contract.
    ///
    /// A `sealed` class (DECISIONS 116) is one no other unit names: it is
    /// published because this unit gives its instances out. No other unit
    /// constructs or extends it, so its methods run on those instances
    /// only (`Model::escape_this`), never on an unknown receiver.
    pub(crate) fn publish_class(
        &mut self,
        tm: TmplId,
        reason: u32,
        extended: bool,
        no_code: bool,
        sealed: bool,
    ) -> Vec<(Atom, NodeId)> {
        if sealed {
            self.sealed.insert(tm);
        }
        let cobj = self.tok(TokKind::ClassObj(tm));
        let inst = self.tok(TokKind::Obj(tm, NO_CTX));
        let rt = self.tmpls.len() as TmplId;
        let display = format!("{}:receiver", self.tmpls[tm as usize].display);
        self.tmpls.push(Template {
            kind: TmplKind::BObj {
                module: 0,
                site: 0,
                what: "contract receiver",
            },
            display,
            birth_keys: Vec::new(),
            definite: Vec::new(),
            declared: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: Vec::new(),
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
        });
        let recv = self.tok(TokKind::Obj(rt, NO_CTX));
        let any = self.field_any(recv);
        self.add_top(any, |m| {
            format!(
                "receiver of a method another unit calls: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(any, prim::ALL);
        // The instance other units construct has the class's prototype.
        self.proto_node(inst);
        // A sealed class's receiver stands for the instances given out
        // only: no other unit constructs one.
        self.contract_recv
            .insert(recv, if sealed { Vec::new() } else { vec![inst] });
        self.contract_recv_of.insert(tm, recv);
        self.recv_class.insert(recv, tm);
        // `this.m()` through the receiver finds the class's `m` (DECISIONS 59).
        self.register_virt(recv, &[tm]);
        // Static members and the class object's own fields.
        let pa = self.atom("prototype");
        let mut named: Vec<(Atom, NodeId)> = self
            .fields
            .iter()
            .filter(|(&(h, a), _)| h == cobj && a != pa)
            .map(|(&(_, a), &n)| (a, n))
            .collect();
        named.sort_unstable();
        let mut statics: Vec<NodeId> = named.iter().map(|&(_, n)| n).collect();
        statics.extend(self.field_any.get(&cobj).copied());
        statics.sort_unstable();
        // Other units call a static method on the class object, through
        // its function contract (§11.2: a static member is part of the
        // class contract; its result is checked like any contract result).
        for &n in &statics {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    let broken = self
                        .broken_fns
                        .contains(&format!("fn:{}", self.funcs[f as usize].name));
                    if !self.is_escaped(t) && self.funcs[f as usize].func_id != u32::MAX && !broken
                    {
                        self.contract_fns.insert(f);
                        // A static function leaves through its contract
                        // (markers on its parameters), whatever else the
                        // static holds (DECISIONS 173).
                        self.escape(t, reason);
                    }
                    if !self.funcs[f as usize].lexical_this {
                        let this = self.funcs[f as usize].this;
                        self.add_tok(this, cobj);
                    }
                }
            }
        }
        // A named static leaves as a published value binding does (DECISIONS
        // 173): the caller checks it after the solve (`Gen::contract_check`),
        // so an instance of this class it holds is given out, not escaped
        // with its prototype chain. What a computed key holds is unknown.
        if let Some(&any) = self.field_any.get(&cobj) {
            self.sink_node(any, reason);
        }
        // Methods along the chain inside the unit: they run on `inst` and on
        // unknown receivers.
        let ca = self.atom("constructor");
        let mut cur = Some(tm);
        let mut members: Vec<NodeId> = Vec::new();
        while let Some(c) = cur {
            // Another unit's class (a stand-in parent): its methods are that
            // unit's, called through its contract (DECISIONS 57).
            if self.foreign.contains_key(&c) {
                break;
            }
            let proto = self.tok(TokKind::Proto(c));
            members.extend(
                self.fields
                    .iter()
                    .filter(|(&(h, a), _)| h == proto && a != ca)
                    .map(|(_, &n)| n),
            );
            members.extend(self.field_any.get(&proto).copied());
            cur = self.tmpls[c as usize].parent;
        }
        members.sort_unstable();
        // Each method (and the constructor) is also a function contract
        // (DECISIONS 57): other units pass its arguments by their marks,
        // and its receiver marker publishes what it does to `this`, so a
        // subclass instance of another unit can run it.
        for &n in &members {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    let rs = self.published_this.entry(f).or_default();
                    if !rs.contains(&recv) {
                        rs.push(recv);
                    }
                    let broken = self
                        .broken_fns
                        .contains(&format!("fn:{}", self.funcs[f as usize].name));
                    if !self.is_escaped(t) && self.funcs[f as usize].func_id != u32::MAX && !broken
                    {
                        self.contract_fns.insert(f);
                    }
                }
            }
        }
        let t = &self.tmpls[tm as usize];
        match t.ctor {
            Some(ctor) if extended => {
                let ft = self.funcs[ctor as usize].token;
                let broken = self
                    .broken_fns
                    .contains(&format!("fn:{}", self.funcs[ctor as usize].name));
                if !self.is_escaped(ft) && self.funcs[ctor as usize].func_id != u32::MAX && !broken
                {
                    self.published_this.entry(ctor).or_default().push(recv);
                    self.contract_fns.insert(ctor);
                    self.escape_fn_params(ft, reason);
                    self.escape_this(ft, reason);
                    self.ctor_published.insert(tm);
                }
            }
            // No constructor code: what construction does to the receiver
            // is the class's own shape (its field initializers), whoever
            // extends it; an extension through a value (a CommonJS
            // `require`) names no import the census could match
            // (DECISIONS 99).
            None if no_code => {
                self.ctor_published.insert(tm);
            }
            _ => {}
        }
        for n in members {
            self.sink_node(n, reason);
        }
        if sealed {
            return named;
        }
        // Other units construct it with unknown arguments.
        let ret = self.node_l("contract-new");
        let args_n = self.node_l("contract-args");
        self.add_top(args_n, |m| {
            format!(
                "constructor args from another unit: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(args_n, prim::ALL);
        // A closed class (design §18.13): what the constructing units pass.
        let ctor = self.tmpls[tm as usize].ctor;
        let mut args = match ctor.and_then(|f| self.closed_params.get(&f)) {
            Some(nodes) => nodes.clone(),
            None => vec![args_n; 8],
        };
        // Its hidden capture parameters take the class's environment, not
        // what the constructing unit passes (DECISIONS 117).
        if let Some(f) = ctor {
            for i in 0..self.funcs[f as usize].params.len() {
                if let Some(env) = self.capture_param(f, i) {
                    if args.len() <= i {
                        args.resize(i + 1, args_n);
                    }
                    args[i] = env;
                }
            }
        }
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            this: ThisArg::Undef,
            args,
            spread: vec![],
            ret,
            site: 0,
        });
        self.contract_new.insert(call);
        self.construct(call, cobj);
        // ... and read its fields: they leave key by key, described by the
        // class contract (`Gen::fields_leave`, DECISIONS 88).
        named
    }

    /// DECISIONS 210/211: unknown code constructs a function of this unit
    /// that escaped, with unknown arguments, and its instance escapes, as
    /// for an escaped class (`escape_inner`). The one path for "unknown code
    /// constructs an escaped function", in both modes: whole program, code
    /// nothing models holds it (an escaped `F` nothing here constructs
    /// still has instances of its layout); per unit, other units' code holds
    /// it too and may construct it (the whole program sees that `new`;
    /// fastify `new Reply`, escaped through `Module._cache`: the units
    /// constructing it hold it as an unknown value, so only the units
    /// writing `Reply.prototype` hold the layout, with no instance of their
    /// own). Callers: an escape (`escape_inner`), a template made after the
    /// escape (`fn_inst_template`), and a contract-only escape that later
    /// leaves through no contract (`escape`). Only a function with a construction layout: one
    /// whose body stores into `this` (`fn_birth`) or whose `new <f>`
    /// template exists (`fn_inst_template` constructs it when made later);
    /// any other one's instances would hold nothing a layout claims. Never
    /// an arrow, a generator step, a class constructor (its class escapes)
    /// or another unit's stand-in (its owner constructs it on the census
    /// report, `Gen::published_ctors`).
    pub(crate) fn construct_unknown(&mut self, t: Tok, f: FnIdx, reason: u32) {
        let fi = &self.funcs[f as usize];
        let np = fi.params.len().max(1);
        // Published through its contract, the units constructing it pass
        // their arguments (`ctor_runs`): constructed by `Gen::published_ctors`.
        let published = self.pub_reason == Some(reason) || self.field_fn_reason == Some(reason);
        if published
            || fi.lexical_this
            || fi.gen_step
            || fi.ctor_of.is_some()
            || fi.func_id == u32::MAX
            || !(self.fn_birth.contains_key(&f) || self.fn_inst.contains_key(&f))
            || !self.unknown_new.insert(f)
        {
            return;
        }
        let ret = self.node_l("top-new");
        let args_n = self.node_l("top-args");
        self.add_top(args_n, |m| {
            format!(
                "constructor args from unknown code: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(args_n, prim::ALL);
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            this: ThisArg::Undef,
            args: vec![args_n; np],
            spread: vec![],
            ret,
            site: 0,
        });
        self.construct(call, t);
        self.sink_node(ret, reason);
    }

    /// DECISIONS 210: unknown code holds function `t` (published through
    /// its contract): it reads and writes any own property but `prototype`
    /// (that one is the published layout's chain, `escape`). Each named
    /// property leaves; what a computed key stores leaves; a read under an
    /// unknown key reads what unknown code wrote; its accessors run as for
    /// any escaped object.
    pub(crate) fn escape_props(&mut self, t: Tok, reason: u32) {
        if self.props_escaped.insert(t, reason).is_some() {
            return;
        }
        let pa = self.atom("prototype");
        let keys: Vec<Atom> = self.field_keys.get(&t).cloned().unwrap_or_default();
        for a in keys {
            if a != pa {
                let n = self.fields[&(t, a)];
                self.escape_prop(t, n, reason);
            }
        }
        let any = self.field_any(t);
        self.cons(any, Cons::TopSink { reason });
        let all = self.all_fields(t);
        self.add_top(all, |m| {
            format!(
                "property of an escaped function: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(all, prim::ALL);
        let w = self.node_l("escaped-store");
        self.add_top(w, |m| {
            format!(
                "stored into an escaped function: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(w, prim::ALL);
        self.accessor_walk(t, t, Key::Any, ChainAct::Store(w));
        let r = self.node_l("escaped-load");
        self.sink_node(r, reason);
        self.accessor_walk(t, t, Key::Any, ChainAct::Load(r));
    }

    /// A named property `n` of function `t` whose properties left
    /// (`escape_props`): unknown code writes it and reads what it holds.
    pub(crate) fn escape_prop(&mut self, t: Tok, n: NodeId, reason: u32) {
        self.add_top(n, |m| {
            format!(
                "property of an escaped function {t}: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(n, prim::ALL);
        self.cons(n, Cons::TopSink { reason });
    }

    /// DECISIONS 210: the instances of this unit's constructor function
    /// `f` (template `tm`), constructed here for the units that construct
    /// it, are a layout those units hold (`contracts::fn_inst_contract`).
    /// The functions on its prototype (`Proto(tm)`, what `f.prototype`
    /// holds) run on `inst` and on unknown receivers, each a contract
    /// function, as a published class's methods do (`publish_class`); any
    /// other value there leaves.
    pub(crate) fn publish_fn_inst(&mut self, f: FnIdx, tm: TmplId, inst: Tok, reason: u32) {
        let ca = self.atom("constructor");
        let proto = self.tok(TokKind::Proto(tm));
        let mut members: Vec<NodeId> = self
            .field_keys
            .get(&proto)
            .into_iter()
            .flatten()
            .filter(|&&a| a != ca)
            .map(|&a| self.fields[&(proto, a)])
            .collect();
        members.extend(self.field_any.get(&proto).copied());
        members.sort_unstable();
        members.dedup();
        for &n in &members {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(g, _) = self.toks[t as usize] {
                    let broken = self
                        .broken_fns
                        .contains(&format!("fn:{}", self.funcs[g as usize].name));
                    if !self.is_escaped(t) && self.funcs[g as usize].func_id != u32::MAX && !broken
                    {
                        self.contract_fns.insert(g);
                    }
                    if !self.funcs[g as usize].lexical_this {
                        for this in self.this_nodes(t) {
                            self.add_tok(this, inst);
                        }
                    }
                }
            }
        }
        for n in members {
            self.sink_node(n, reason);
        }
        let _ = f;
    }

    /// This unit gives `t`, an instance of its published class `tm`, to
    /// other units (a contract result or value): they call its methods, so
    /// what those do to their receiver happens to `t` as well, replayed
    /// from the receiver's log and forwarded from now on. True when `t` is
    /// new here.
    pub(crate) fn give_out(&mut self, tm: TmplId, t: Tok) -> bool {
        let Some(&recv) = self.contract_recv_of.get(&tm) else {
            return false;
        };
        let given = self.contract_recv.entry(recv).or_default();
        if given.contains(&t) {
            return false;
        }
        given.push(t);
        // A sealed class's methods (DECISIONS 116) and the methods run on
        // published receivers (DECISIONS 125) run on `t` from now on.
        let sealed = self.sealed.contains(&tm);
        let ds: Vec<NodeId> = self
            .given_this
            .iter()
            .filter(|(f, _)| {
                self.published_this
                    .get(f)
                    .is_some_and(|rs| rs.contains(&recv))
            })
            .filter(|(&f, _)| sealed || self.join_holds(f, t))
            .flat_map(|(_, ds)| ds.iter().copied())
            .collect();
        for d in ds {
            self.add_tok(d, t);
        }
        for e in self.recv_log.get(&recv).cloned().unwrap_or_default() {
            match e {
                crate::model::RecvEffect::Store(key, src, site) => {
                    self.store_into(t, key, src, site)
                }
                crate::model::RecvEffect::Hard(kind, reason) => self.mark_hard(t, kind, reason),
                crate::model::RecvEffect::Escape(reason) => self.escape(t, reason),
            }
        }
        true
    }

    pub(super) fn escape_inner(&mut self, t: Tok, reason: u32) {
        // DECISIONS 212: a value that escapes beyond any contract under the
        // publication reason (no stand-in forwarded, no contract function)
        // is held by code no contract describes: what it reaches leaves
        // beyond any contract too (DECISIONS 203), its contract functions
        // included (`escape`).
        let reason = self.beyond_contract(reason);
        self.mark_use(t, crate::model::MARK_ESCAPE);
        self.virt_revoke(t, None);
        if let Some(is) = self.contract_recv.get(&t).cloned() {
            self.recv_log
                .entry(t)
                .or_default()
                .push(crate::model::RecvEffect::Escape(reason));
            for i in is {
                self.escape(i, reason);
            }
        }
        // A marker: unknown code gets what it stands for, and what that
        // reaches (its fields are its reach markers). The values sit beside
        // it and escape there (DECISIONS 111).
        if self.is_marker(t) {
            let r = self.reach_of(t);
            self.mark_use(r, crate::model::MARK_ESCAPE);
            self.mark_escaped(t, reason);
            return;
        }
        if matches!(self.toks[t as usize], TokKind::Proto(_)) {
            self.mark_escaped(t, reason);
            self.resume_stopped(t);
        }
        self.mark_escaped(t, reason);
        self.rel(|| format!("escaped {t}"));
        // Unknown code reads every field and can write any key.
        let all = self.all_fields(t);
        self.cons(all, Cons::TopSink { reason });
        // A module namespace object is not writable: [[Set]] and
        // [[DefineOwnProperty]] fail, [[Delete]] of an export fails (ES
        // 10.4.6); its keys hold what its module's bindings hold.
        if matches!(self.toks[t as usize], TokKind::ModNs(_)) {
            return;
        }
        let any = self.any_key_node(t);
        self.add_top(any, |m| {
            format!(
                "field of escaped {}: {}",
                m.tok_kind_word(t),
                m.reasons[reason as usize]
            )
        });
        self.add_prim(any, prim::ALL);
        if let Some(tm) = self.tmpl_of(t) {
            *self.adds_keys.entry(tm).or_default() += 1;
        }
        // Its prototypes are reachable (`Object.getPrototypeOf`).
        let pn = self.proto_node(t);
        self.chain_sinks.insert(pn);
        self.cons(pn, Cons::TopSink { reason });
        // Unknown code calls the functions it reaches through `t` (its
        // fields and prototype sink above, so each escapes): they run in
        // their unknown-code instance with `this` = ⊤, never in the
        // instance of `t`'s layout (an escaped value binds no layout).
        // Unknown code can also write any key of it: every setter on its
        // chain (its own accessors included) runs with `this` = it and an
        // unknown value (a direct eval writes the caller's locals exactly so,
        // through the setters of its bindings object); and read any key:
        // every getter runs and its result reaches unknown code.
        let w = self.node_l("escaped-store");
        self.add_top(w, |m| {
            format!(
                "stored into an escaped {}: {}",
                m.tok_kind_word(t),
                m.reasons[reason as usize]
            )
        });
        self.add_prim(w, prim::ALL);
        self.accessor_walk(t, t, Key::Any, ChainAct::Store(w));
        let r = self.node_l("escaped-load");
        self.sink_node(r, reason);
        self.accessor_walk(t, t, Key::Any, ChainAct::Load(r));
        match self.toks[t as usize].clone() {
            TokKind::Func(f, _) => {
                let fi = self.funcs[f as usize].clone();
                if !fi.lexical_this {
                    self.escape_this(t, reason);
                }
                self.escape_fn_params(t, reason);
                self.construct_unknown(t, f, reason);
            }
            TokKind::ClassObj(tm) => {
                let p = self.tok(TokKind::Proto(tm));
                self.escape(p, reason);
                // Unknown code may construct it with unknown arguments.
                let ret = self.node_l("top-new");
                let args_n = self.node_l("top-args");
                self.add_top(args_n, |m| {
                    format!(
                        "constructor args from unknown code: {}",
                        m.reasons[reason as usize]
                    )
                });
                self.add_prim(args_n, prim::ALL);
                let call = self.calls.len() as CallId;
                self.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args: vec![args_n; 8],
                    spread: vec![],
                    ret,
                    site: 0,
                });
                self.construct(call, t);
                self.sink_node(ret, reason);
                // DEC221: only what unknown construction returns escapes.
                // NO_CTX is a template-wide mark, which would also expose
                // every existing private birth of this layout.
            }
            TokKind::Obj(tm, _) => {
                // `.constructor` reaches the class value.
                if let TmplKind::Class { .. } = self.tmpls[tm as usize].kind {
                    let c = self.tok(TokKind::ClassObj(tm));
                    self.escape(c, reason);
                }
                if let TmplKind::FnInst { func } = self.tmpls[tm as usize].kind {
                    let ft = self.funcs[func as usize].token;
                    self.escape(ft, reason);
                }
            }
            TokKind::Proto(tm) => {
                let c = self.tok(TokKind::ClassObj(tm));
                // `.constructor` on the prototype.
                if matches!(self.tmpls[tm as usize].kind, TmplKind::Class { .. }) {
                    self.escape(c, reason);
                }
            }
            TokKind::Bound(b) => {
                let (target, bthis, bargs) = self.bound[b as usize].clone();
                for n in [target, bthis].into_iter().chain(bargs) {
                    self.sink_node(n, reason);
                }
            }
            _ => {}
        }
    }

    pub fn mark_hard(&mut self, t: Tok, kind: Hard, reason: u32) {
        self.virt_revoke(t, None);
        if kind == Hard::E6SetPrototypeOf {
            self.super_relink.insert(t);
        }
        if matches!(
            kind,
            Hard::E5DefineProperty | Hard::E7Delete | Hard::E15With
        ) {
            self.note_fnproto_write(t, None, kind.code());
        }
        if matches!(
            kind,
            Hard::E5DefineProperty | Hard::E6SetPrototypeOf | Hard::E7Delete | Hard::E15With
        ) {
            let what = match self.explain {
                true => format!("{} ({})", kind.code(), self.reasons[reason as usize]),
                false => kind.code().to_string(),
            };
            crate::builtins::note_native_write(self, t, None, &what);
        }
        if t == TOP {
            return;
        }
        if let Some(is) = self.contract_recv.get(&t).cloned() {
            self.recv_log
                .entry(t)
                .or_default()
                .push(crate::model::RecvEffect::Hard(kind, reason));
            for i in is {
                self.mark_hard(i, kind, reason);
            }
        }
        self.mark_use(t, crate::model::MARK_ESCAPE);
        // `Object.setPrototypeOf(Object.prototype, p)` throws unless `p` is
        // null (its [[Prototype]] already): nothing changes (immutable
        // prototype exotic object). A ⊤ target that may be it is no E6 on it.
        if kind == Hard::E6SetPrototypeOf && matches!(self.toks[t as usize], TokKind::ObjectProto) {
            return;
        }
        // DECISIONS 217: another unit's lookups see a HARD shared object.
        if let Some(s) = self.shared_of(t) {
            self.note_shared(SharedWrite::Hard(s, kind));
        }
        if self.hard.contains_key(&t) {
            return;
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("hardseed {t} {} {reason}", kind.code()));
        }
        if matches!(self.toks[t as usize], TokKind::Proto(_)) {
            self.hard.insert(t, (kind, reason));
            self.with_s(|m| m.resume_stopped(t));
        }
        self.hard.insert(t, (kind, reason));
        self.rel(|| format!("hardmark {t} {}", kind.code()));
        // Deleting / redefining can move values or install accessors: the
        // object's fields are now written by unknown code as well.
        let any = self.any_key_node(t);
        self.add_prim(any, prim::ALL);
        if let Some(tm) = self.tmpl_of(t) {
            *self.adds_keys.entry(tm).or_default() += 1;
        }
        for s in self.wide_sites(t) {
            self.mark_hard(s, kind, reason);
        }
    }
}
