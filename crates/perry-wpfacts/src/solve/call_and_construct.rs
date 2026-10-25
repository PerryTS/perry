//! Call binding, constructor flow and escaping receivers.
use super::*;

impl Model {
    /// Bind `call` to callee token `t`. `this_tok` overrides the call's this.
    pub fn call_tok(&mut self, call: CallId, t: Tok, this_tok: Option<Tok>) {
        if matches!(self.toks[t as usize], TokKind::Top | TokKind::BuiltinFn) {
            // An unknown callee sees only its own receiver token: that one
            // escapes; arguments and result are per call.
            if let Some(x) = this_tok {
                let r = self.reason("receiver of a call of an unknown function value");
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

    /// Unknown callee (⊤ or an opaque builtin function value): arguments
    /// escape, the result is ⊤; with `sink_this` (no receiver token given),
    /// the call's own `this` escapes too.
    pub(super) fn unknown_callee_args(&mut self, call: CallId, t: Tok, sink_this: bool) {
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
        let TokKind::Func(f, _) = self.toks[t as usize] else {
            return;
        };
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
        for i in 0..nargs {
            let a = self.calls[c].args[i];
            if i < nparams {
                let p = self.funcs[f].params[i];
                self.edge(a, p);
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
            self.add_prim(p, prim::UNDEF);
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
                let p = self.funcs[f].params[i];
                self.edge(elems, p);
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
        // facts are its defining unit's).
        let call = if self.foreign.contains_key(&tm) {
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

    pub fn construct(&mut self, call: CallId, t: Tok) {
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::Construct(call, t));
            return;
        }
        if !self.construct_done.insert((call, t)) {
            return;
        }
        let c = self.calls[call as usize].clone();
        match self.toks[t as usize].clone() {
            TokKind::ClassObj(tm) => {
                let ctx = self.birth_ctx(call);
                let inst = self.site_tok(tm, ctx);
                self.add_tok(c.ret, inst);
                self.run_ctor_chain(call, tm, inst);
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
        });
        self.fn_inst.insert(f, id);
        id
    }

    /// `recv.key(args)` on token `t`.
    pub(super) fn method_on(&mut self, t: Tok, key: Key, call: CallId) {
        // A builtin protocol call (ToPrimitive, iteration ...) is a touch.
        let bits = if self.protocol_calls.contains(&call) {
            crate::model::MARK_TOUCH
        } else {
            crate::model::MARK_ESCAPE
        };
        self.mark_use(t, bits);
        if t == TOP {
            self.call_tok(call, TOP, Some(TOP));
            return;
        }
        // A marker is use tracking, not a value (DECISIONS 111): the values
        // it stands for sit beside it and are called there. It has no chain
        // and no accessors; its method is what it reaches under the key,
        // and calling that is an escape of it (`call_tok`).
        if self.is_marker(t) {
            let m = match key {
                Key::Named(a) => self.marker_load(t, Some(a)),
                Key::Any => self.reach_of(t),
            };
            self.mark_use(m, crate::model::MARK_ESCAPE);
            return;
        }
        // Own function-valued fields, then the prototype chain.
        let own = match key {
            Key::Named(a) => self.field(t, a),
            Key::Any => self.all_fields(t),
        };
        self.bind_via(own, call, t);
        self.with_b(|m| builtins::own_method(m, t, key, call));
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

    pub fn escape(&mut self, t: Tok, reason: u32) {
        if t == TOP || self.is_escaped(t) {
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
        }
        if matches!(
            self.toks[t as usize],
            TokKind::Builtin(BuiltinNs::GlobalThis)
        ) {
            crate::builtins::note_global_in_top(self);
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
        for (i, &p) in fi.params.iter().enumerate() {
            match closed.as_ref().and_then(|c| c.get(i).copied()) {
                Some(a) => self.edge(a, p),
                None => {
                    self.add_top(p, why);
                    self.add_prim(p, prim::ALL);
                }
            }
        }
        for &p in fi.rest.iter() {
            self.add_top(p, why);
            self.add_prim(p, prim::ALL);
        }
        if self.contract_fns.contains(&f) {
            // Design §11.2: other units call it through its contract.
            for (i, &p) in fi.params.iter().enumerate() {
                let mk = self.param_marker_tok(f, i as u32);
                self.add_tok(p, mk);
            }
            self.add_top(fi.args_all, why);
            self.contract_rets.push((f, fi.ret));
            return;
        }
        if let Some(rest) = fi.rest {
            let r = self.field_any_node_of_node(rest);
            self.add_top(r, why);
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
    pub(crate) fn publish_class(&mut self, tm: TmplId, reason: u32) {
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
        self.contract_recv.insert(recv, vec![inst]);
        self.contract_recv_of.insert(tm, recv);
        // Static members and the class object's own fields.
        let pa = self.atom("prototype");
        let mut statics: Vec<NodeId> = self
            .fields
            .iter()
            .filter(|(&(h, a), _)| h == cobj && a != pa)
            .map(|(_, &n)| n)
            .collect();
        statics.extend(self.field_any.get(&cobj).copied());
        statics.sort_unstable();
        // Other units call a static method on the class object, through
        // its function contract (§11.2: a static member is part of the
        // class contract; its result is checked like any contract result).
        for &n in &statics {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    if !self.is_escaped(t) && self.funcs[f as usize].func_id != u32::MAX {
                        self.contract_fns.insert(f);
                    }
                    if !self.funcs[f as usize].lexical_this {
                        let this = self.funcs[f as usize].this;
                        self.add_tok(this, cobj);
                    }
                }
            }
        }
        for n in statics {
            self.sink_node(n, reason);
        }
        // Methods along the chain inside the unit: they run on `inst` and on
        // unknown receivers.
        let ca = self.atom("constructor");
        let mut cur = Some(tm);
        let mut members: Vec<NodeId> = Vec::new();
        while let Some(c) = cur {
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
        for &n in &members {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    self.published_this.insert(f, recv);
                }
            }
        }
        for n in members {
            self.sink_node(n, reason);
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
        let args = match self.tmpls[tm as usize]
            .ctor
            .and_then(|f| self.closed_params.get(&f))
        {
            Some(nodes) => nodes.clone(),
            None => vec![args_n; 8],
        };
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            this: ThisArg::Undef,
            args,
            spread: vec![],
            ret,
            site: 0,
        });
        self.construct(call, cobj);
        // ... and read every field of what they construct.
        let all = self.all_fields(inst);
        self.cons(all, Cons::TopSink { reason });
        let r = self.node_l("contract-load");
        self.sink_node(r, reason);
        self.accessor_walk(inst, inst, Key::Any, ChainAct::Load(r));
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
        self.mark_use(t, crate::model::MARK_ESCAPE);
        // A marker: unknown code gets what it stands for, and what that
        // reaches (its fields are its reach markers). The values sit beside
        // it and escape there (DECISIONS 111).
        if self.is_marker(t) {
            let r = self.reach_of(t);
            self.mark_use(r, crate::model::MARK_ESCAPE);
            self.mark_escaped(t, reason);
            return;
        }
        if let Some(is) = self.contract_recv.get(&t).cloned() {
            self.recv_log
                .entry(t)
                .or_default()
                .push(crate::model::RecvEffect::Escape(reason));
            for i in is {
                self.escape(i, reason);
            }
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
        let any = self.field_any(t);
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
            }
            TokKind::ClassObj(tm) => {
                let p = self.tok(TokKind::Proto(tm));
                self.escape(p, reason);
                // Unknown code may construct it with unknown arguments.
                let inst = self.tok(TokKind::Obj(tm, NO_CTX));
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
                self.escape(inst, reason);
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
            crate::builtins::note_native_write(self, t, None, kind.code());
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
        let any = self.field_any(t);
        self.add_prim(any, prim::ALL);
        if let Some(tm) = self.tmpl_of(t) {
            *self.adds_keys.entry(tm).or_default() += 1;
        }
        for s in self.wide_sites(t) {
            self.mark_hard(s, kind, reason);
        }
    }
}
