//! Call binding and constructor flow (escaping receivers are in `escape`).
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
            // A receiver node of this unit's (`o.k()` with the marker under
            // `k`) travels to the caller's unit described as the call's
            // `this` (`ParamCall::this`), as the arguments do; a receiver
            // token given from outside the call (`this_tok`, `ThisArg::Tok`)
            // is the escape below.
            let plain = this_tok.is_none()
                && matches!(cs.this, ThisArg::Undef | ThisArg::Node(_))
                && cs.spread.is_empty();
            if let Some(own) = plain.then(|| self.own_param_members(t)).flatten() {
                if self.bind_done.insert((call, t, u32::MAX - 3)) {
                    self.mark_use(t, crate::model::MARK_READ);
                    let own = own.into_iter().map(|(f, i)| (f, i, None)).collect();
                    self.marker_call(own, None, call);
                }
                return;
            }
            let spread = !cs.spread.is_empty();
            self.mark_escape_why(t, &|m: &Self| {
                if spread {
                    "call: spread arguments".into()
                } else if !plain {
                    "call: with a receiver".into()
                } else if m.reach_marker.contains(&t) {
                    "call: of a reach".into()
                } else {
                    "call: of a marker that is no own parameter".into()
                }
            });
            return self.call_tok(call, TOP, this_tok);
        }
        let prev = self.cur_native_call.replace(call);
        let native = self.native_call(call, t, this_tok);
        self.cur_native_call = prev;
        if native {
            return;
        }
        if matches!(self.toks[t as usize], TokKind::Top | TokKind::BuiltinFn(_)) {
            if let Some(d) = self.diagnostic.as_mut() {
                let n = d.cause_node;
                d.unknown_calls.entry(call).or_insert((n, t == TOP));
            }
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
                } else if self.diagnostic.is_some() {
                    self.reason(&format!(
                        "receiver of a call of an unknown function value [call {call}]"
                    ))
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
            TokKind::Top | TokKind::BuiltinFn(_) => unreachable!(),
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
                let pc = m.cur_native_call.replace(call);
                builtins::call_builtin_ns(m, ns, call);
                m.cur_native_call = pc;
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
                self.mark_escape_why(t, &|_| "call: of a non-callable".into());
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

    /// DECISIONS 236: the members a method call on `t` is recorded for:
    /// `own_param_members`, and a receiver marker's keyed reach (`this.f`,
    /// a path marker under a named key of an unescaped receiver marker) as
    /// (function, `u32::MAX`, `Some(f)`): each unit holding a receiver runs
    /// the call on what its object holds under `f` (its own chain, its own
    /// stores, ⊤ where its object escaped or `f` was written through ⊤).
    /// `None`: some member is neither, or none is left.
    pub(crate) fn mcall_members(&self, t: Tok) -> Option<Vec<(FnIdx, u32, Option<Atom>)>> {
        self.marker_members(t)
            .into_iter()
            .map(|m| match self.param_marker.get(&m) {
                _ if self.beside_values(m) && !self.unrun_param(m) => Some(None),
                Some(&(f, i))
                    if i != u32::MAX
                        && self.marker_of.get(&(f, i)) == Some(&m)
                        && !self.marker_is_top(m) =>
                {
                    Some(Some((f, i, None)))
                }
                Some(&(f, u32::MAX)) => match self.path_parent.get(&m) {
                    Some(&(rm, Some(k)))
                        if self.marker_of.get(&(f, u32::MAX)) == Some(&rm)
                            && !self.marker_is_top(rm)
                            && !self.marker_is_top(m) =>
                    {
                        Some(Some((f, u32::MAX, Some(k))))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(|v| v.into_iter().flatten().collect::<Vec<_>>())
            .filter(|v| !v.is_empty())
    }

    /// Records a call through the parameters `own` (`key`: a method of
    /// theirs; a path: of the receiver's field, DECISIONS 236) for the
    /// caller's unit to run (DECISIONS 140/144); its result is that
    /// unknown code's. DECISIONS 165: a parameter of a published method no
    /// unit runs (`unrun_param`) stands for no value of another unit, so
    /// a call through it runs nowhere and has no result; the call stays
    /// recorded for a unit that will run the method, whose run solves this
    /// unit again with the parameter described or bare.
    fn marker_call(
        &mut self,
        own: Vec<(FnIdx, u32, Option<Atom>)>,
        key: Option<Atom>,
        call: CallId,
    ) {
        let mut runs = false;
        for (f, i, path) in own {
            runs |= path.is_some()
                || i == u32::MAX
                || !self
                    .marker_of
                    .get(&(f, i))
                    .is_some_and(|&m| self.unrun_param(m));
            if !self.marker_calls.contains(&(f, i, path, key, call)) {
                self.marker_calls.push((f, i, path, key, call));
            }
        }
        if runs {
            let ret = self.calls[call as usize].ret;
            self.add_tok(ret, TOP);
            self.add_prim(ret, prim::ALL);
        }
    }

    /// Unknown callee (⊤ or an opaque builtin function value): arguments
    /// escape, the result is ⊤; with `sink_this` (no receiver token given),
    /// the call's own `this` escapes too.
    pub(super) fn unknown_callee_args(&mut self, call: CallId, t: Tok, sink_this: bool) {
        self.diagnostic_callee(call, t);
        if t == TOP {
            self.top_calls += 1;
        }
        let r = if t == TOP && self.diagnostic.is_some() {
            self.reason(&format!(
                "passed to a call of an unknown function value [call {call}]"
            ))
        } else if t == TOP {
            self.reason("passed to a call of an unknown function value")
        } else {
            crate::builtins::anyfall(
                "opaque callee",
                &self.tok_name(t),
                &format!("call {call} this {:?}", self.calls[call as usize].this),
            );
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
        let region = self.calls[call as usize].region;
        self.raise_top(
            region,
            ret,
            if t == TOP {
                "a call of an unknown function value may throw anything"
            } else {
                "an opaque builtin call may throw anything"
            },
        );
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
        // What the callee's body throws reaches the call's region.
        let raise = self.raise_of(f);
        let region = self.calls[call as usize].region;
        self.edge(raise, region);
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
            // Spread arrays: their elements may be any parameter. A spread
            // yields elements, never a method a computed key could name.
            let elems = self.node_l("spread-elems");
            let ek = self.elem_key();
            self.cons(
                s,
                Cons::Load {
                    key: ek,
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
                region: self.thrown,
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
            region: self.thrown,
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
        // Elements of whatever array the node holds: a store relay. A rest
        // array is made from the arguments at indices 0..n: the store is an
        // element store (`elem_key`), which no named non-index key of the
        // array reads (`is_elem_array`), so an argument is never its
        // `indexOf` or `@@iterator`.
        let relay = self.node_named("elems-into", || format!("elems-into({arr_node})"));
        let site = self.anon_store_site();
        let ek = self.elem_key();
        self.cons(
            arr_node,
            Cons::Store {
                key: ek,
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
            in_loop: None,
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
    pub(super) fn wide_sites(&self, t: Tok) -> Vec<Tok> {
        match self.toks[t as usize] {
            TokKind::Obj(tm, NO_CTX) => self.site_toks.get(&tm).cloned().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub(super) fn chain_has_foreign_returning_ctor(&self, tm: TmplId) -> bool {
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
            self.mark_escape_why(t, &|_| "new: of a marker".into());
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
                    Some((n, tm)) => {
                        self.edge(n, c.ret);
                        // Its owner's function may return an object: `new`
                        // yields that value, which this unit cannot name.
                        if self.foreign_ctor_returns.contains(&tm) {
                            self.add_top(c.ret, |_| {
                                "`new` of another unit's constructor function that returns an object"
                                    .into()
                            });
                        }
                    }
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
            // A method of an intrinsic prototype is no constructor
            // (TypeError): nothing flows.
            TokKind::BuiltinFn(Native::Member { .. })
                if !builtins::native_constructible(self, t) => {}
            // A realm constructor whose declaration says what `new` does
            // (declared at the body its dispatch arm calls): the declaration
            // runs, as for a global constructor by name. Any value of it
            // (`const S = inspector.Session; new S()`) is that constructor.
            TokKind::BuiltinFn(Native::Member { identity, .. })
                if !self.unit_mode && builtins::declared_construct(identity).is_some() =>
            {
                let e = builtins::declared_construct(identity).expect("guarded");
                self.diagnostic_callee(call, t);
                self.with_b(|m| builtins::run_construct(m, e, call));
            }
            TokKind::Top | TokKind::BuiltinFn(_) => {
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
                        region: self.thrown,
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

    /// `Cons::NewProgram`: `call` constructs `t` when `t` is a program
    /// constructor, a class or function of the program's code (a bound one
    /// through its target). A realm constructor (`%Array%`, a builtin
    /// function value, ⊤'s) is the built-in's own result, already made.
    pub fn construct_program(&mut self, call: CallId, t: Tok) {
        match self.toks[t as usize] {
            TokKind::ClassObj(_) | TokKind::Func(_, _) => self.construct(call, t),
            TokKind::Bound(b) => {
                if !self.construct_done.insert((call, t)) {
                    return;
                }
                let (target, _bthis, bargs) = self.bound[b as usize].clone();
                let c = self.calls[call as usize].clone();
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
                        region: self.thrown,
                    },
                );
                self.cons(target, Cons::NewProgram { call: call2 });
            }
            _ => {}
        }
    }

    /// `Cons::FromArrayLike` of token `t`: `Array.from` reads a program
    /// object whose `@@iterator` may be nullish as an array-like (ECMA-262
    /// 23.1.2.1 step 5): ToLength(Get(t, "length")), then Get(t, k) for each
    /// index, into `dst`. Only a program value (an object of a program
    /// template, a function, a class, a prototype): an Array, a builtin's
    /// object (a collection, an iterator, all of them iterable) or ⊤ is the
    /// iteration's (`load_from` under the iteration key).
    pub fn from_array_like(&mut self, t: Tok, dst: NodeId) {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _)
                if self.tmpls[tm as usize].kind.is_fact_unit() && !self.is_array_like(t) => {}
            TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::ClassObj(_) | TokKind::Proto(_) => {}
            _ => return,
        }
        // GetMethod(t, @@iterator) is nullish when t's own value under the
        // key is, or when t holds none and its chain's is. Each half may be
        // nullish: the own fields under the key (stored values, no read of
        // the absence: an instance never holds a class's method), and the
        // read of the key from t's prototypes, which ends `undefined` where
        // no link holds it.
        let src = self.node_l("from-source");
        self.add_tok(src, t);
        let wk = self.atom(crate::builtins::WK_ITERATOR);
        let protos = self.node_l("from-prototypes");
        self.cons(src, Cons::ProtoOf { dst: protos });
        let inherited = self.node_l("from-inherited-iterator");
        self.cons(
            protos,
            Cons::Load {
                key: Key::Named(wk),
                dst: inherited,
            },
        );
        let like = self.node_l("from-array-like");
        let own = self.field(t, wk);
        let any = self.field_any(t);
        for test in [inherited, own, any] {
            self.cons(
                test,
                Cons::Guard {
                    test: guard::NULLISH,
                    from: src,
                    dst: like,
                },
            );
        }
        let la = self.atom("length");
        let len = self.node_l("from-length");
        self.cons(
            like,
            Cons::Load {
                key: Key::Named(la),
                dst: len,
            },
        );
        let r = self.reason("ToLength of an Array.from array-like length");
        self.cons(
            len,
            Cons::Implicit {
                reason: r,
                proto: Protocol::ToPrimitive,
            },
        );
        let ek = self.elem_key();
        self.cons(like, Cons::Load { key: ek, dst });
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
        // A body that completes returns `undefined` (`narrow::may_complete`),
        // which `new` never yields.
        self.cons(ret, Cons::NotNullish { dst: filt });
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
            reserved: Vec::new(),
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
            chain_miss: false,
            chain_miss_open: false,
            species: None,
            per_evaluation: false,
            static_methods: Vec::new(),
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
        // An array is never the iterator the iteration protocol steps or
        // closes: the step of the iterated value itself (`v.next()`,
        // `Gen::iterate`) is made only on a value `js_get_iterator` returns
        // unchanged, never an array (it gives a fresh array iterator); a
        // for-of record's payload holds the source array beside the
        // iterator, and closes it through a fresh array iterator
        // (`js_array_record_close`), not itself. The same predicate as
        // `Cons::IterNext`'s: the runtime makes no such call on an array,
        // whatever its chain holds.
        if let Key::Named(a) = key {
            if self.protocol_calls.contains(&call)
                && matches!(self.atoms[a as usize].as_str(), "next" | "return")
                && matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                    if matches!(self.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
            {
                return;
            }
        }
        // DECISIONS 242(2): a call of a key the receiver may not own.
        if t != TOP {
            self.note_chain_miss(t, key);
        }
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
            if let Some(own) = plain.then(|| self.mcall_members(t)).flatten() {
                if self.bind_done.insert((call, t, u32::MAX - 4)) {
                    self.mark_use(t, crate::model::MARK_READ);
                    let r = self.keyed_reach_of(t, a);
                    self.mark_use(r, crate::model::MARK_READ);
                    self.marker_call(own, Some(a), call);
                }
                return;
            }
        }
        // A marker with its callers' values beside it (decision 13,
        // DECISIONS 107, 148): the call is made on those values (below),
        // whose contracts carry what it does to them to their units
        // (DECISIONS 143). Its own use is a read of the method under the key,
        // as for a parameter's own marker (DECISIONS 144), not an escape:
        // nothing of the receiver reaches code the solve does not see. A
        // reach marker is its own reach under every key (`marker_load`).
        if bits == crate::model::MARK_ESCAPE
            && t != TOP
            && self.is_marker(t)
            && self.beside_values(t)
        {
            self.mark_use(t, crate::model::MARK_READ);
            let r = match key {
                Key::Named(a) => self.marker_load(t, Some(a)),
                Key::Any(_) => self.reach_of(t),
            };
            self.mark_use(r, crate::model::MARK_READ);
            return;
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
                Key::Any(_) => self.reach_of(t),
            };
            self.mark_escape_why(m, &|mm: &Self| match key {
                Key::Named(a)
                    if matches!(mm.atoms[a as usize].as_str(), "call" | "apply" | "bind") =>
                {
                    format!("method: .{}", mm.atoms[a as usize])
                }
                Key::Named(_) if mm.reach_marker.contains(&t) => "method: of a reach".into(),
                Key::Named(_) => "method: of a marker that is no own parameter".into(),
                Key::Any(_) => "method: computed key".into(),
            });
            // That method is an unknown function, as a plain call through a
            // marker is (`call_tok`): the arguments escape, the result is
            // unknown (DECISIONS 202).
            if self.bind_done.insert((call, TOP, u32::MAX - 2)) {
                self.unknown_callee_args(call, TOP, false);
            }
            return;
        }
        // Native own statics, fields and inherited methods are the same
        // ordinary read whether the source immediately calls the value or
        // saves it first. This also covers constructors without a specialized
        // namespace model.
        if !self.unit_mode
            && matches!(self.toks[t as usize], TokKind::BuiltinFn(_))
            && matches!(key, Key::Named(_))
        {
            let callee = self.node_l("builtin-function-read");
            self.load_from(t, t, key, callee);
            self.bind_via(callee, call, t);
            return;
        }
        // A described namespace method is the ordinary member value,
        // called with its receiver. Its aliases use the very same model.
        if !self.unit_mode {
            if let (TokKind::Builtin(ns), Key::Named(a)) = (&self.toks[t as usize], key) {
                if builtins::namespace_native(self, t, *ns, &self.atoms[a as usize]).is_some()
                    || builtins::namespace_receiver_getter(self, t, &self.atoms[a as usize])
                {
                    let callee = self.node_l("builtin-function-read");
                    self.load_from(t, t, key, callee);
                    self.bind_via(callee, call, t);
                    return;
                }
            }
        }
        // A missing intrinsic member calls exactly the ordinary read's
        // values, including program fields, getters and reopened chains.
        // No callable value means a TypeError, with no argument escape.
        if let Key::Named(a) = key {
            if builtins::undeclared(self, t, a) {
                let callee = self.node_l("builtin-member-read");
                self.load_from(t, t, key, callee);
                self.bind_via(callee, call, t);
                return;
            }
        }
        // Own function-valued fields, then the prototype chain.
        let own = match (key, virt) {
            (Key::Named(_), Some(v)) => v,
            (Key::Named(a), None) => self.field(t, a),
            (Key::Any(_), _) => self.key_fields(t, key),
        };
        self.bind_via(own, call, t);
        self.builtin_field_use(t, key, ChainAct::Call(call));
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
        let own_members = !self.virt_protos.contains_key(&t);
        // DECISIONS 234: the callee lookup is a [[Get]]. A key every object
        // `t` denotes holds as an own data property stops it at the object,
        // as for a read: the call binds the program prototypes' methods (a
        // superset), not the builtin members, which it takes once the key
        // no longer shadows them (`flush_shadowed`).
        match self.shadow_key(t, key) {
            Some(a) => {
                let cv = self.cv_of(t, key, false);
                self.bind_via(cv, call, t);
                self.note_shadowed(t, a, ChainAct::Call(call), own_members);
            }
            None => self.builtin_members(t, key, ChainAct::Call(call), own_members),
        }
        // Getters returning functions (per receiver, accessor keys only).
        self.accessor_walk(t, t, key, ChainAct::Call(call));
    }
}
