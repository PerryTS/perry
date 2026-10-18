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
            TokKind::Func(f) => {
                if !self.funcs[f as usize].lexical_this {
                    let fthis = self.funcs[f as usize].this;
                    match (this_tok, &self.calls[call as usize].this) {
                        (Some(x), _) => self.add_tok(fthis, x),
                        (None, &ThisArg::Tok(x)) => self.add_tok(fthis, x),
                        (None, &ThisArg::Node(n)) => self.edge(n, fthis),
                        (None, ThisArg::Undef) => self.add_prim(fthis, prim::UNDEF),
                    }
                }
                self.bind_args(call, t);
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
            TokKind::Obj(_)
            | TokKind::Proto(_)
            | TokKind::ObjectProto
            | TokKind::PrimBox
            | TokKind::NativeProto(_)
            | TokKind::ModNs(_) => {
                // Not callable (TypeError) — unless it is a builtin object
                // that is a function (Proxy over a function etc., ⊤ anyway).
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
    pub(super) fn bind_args(&mut self, call: CallId, t: Tok) {
        let TokKind::Func(f) = self.toks[t as usize] else {
            return;
        };
        if !self.bind_done.insert((call, t, u32::MAX - 1)) {
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
        self.edge(fret, cret);
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
        let call2 = self.calls.len() as CallId;
        let mut args: Vec<NodeId> = bargs.clone();
        args.extend(c.args.iter().copied());
        self.calls.push(CallSite {
            this: ThisArg::Node(bthis),
            args,
            spread: c.spread.clone(),
            ret: c.ret,
            site: c.site,
        });
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
    pub fn construct(&mut self, call: CallId, t: Tok) {
        if !self.construct_done.insert((call, t)) {
            return;
        }
        let c = self.calls[call as usize].clone();
        match self.toks[t as usize].clone() {
            TokKind::ClassObj(tm) => {
                let inst = self.tok(TokKind::Obj(tm));
                self.add_tok(c.ret, inst);
                self.run_ctor_chain(call, tm, inst);
            }
            TokKind::Func(f) => {
                let tm = self.fn_inst_template(f);
                let inst = self.tok(TokKind::Obj(tm));
                self.add_tok(c.ret, inst);
                let fi = self.funcs[f as usize].clone();
                self.call_tok(call, t, Some(inst));
                // E12: a constructor function returning an object.
                self.ret_objects(fi.ret, c.ret);
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
                let call2 = self.calls.len() as CallId;
                let mut args = bargs;
                args.extend(c.args.iter().copied());
                self.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args,
                    spread: c.spread.clone(),
                    ret: c.ret,
                    site: c.site,
                });
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
            let this = self.funcs[fi as usize].this;
            self.add_tok(this, inst);
        }
        let t = &self.tmpls[tm as usize];
        match (t.ctor, t.parent, t.native_parent.clone()) {
            (Some(ctor), _, _) => {
                let ftok = self.funcs[ctor as usize].token;
                self.call_tok(call, ftok, Some(inst));
                let ret = self.funcs[ctor as usize].ret;
                self.ret_objects(ret, c.ret);
            }
            (None, Some(p), _) => {
                // Implicit `constructor(...a) { super(...a) }`.
                let _ = p;
                let receiver = self.node_l("implicit-super-this");
                self.cons(receiver, Cons::SuperCtor { class: tm, call });
                self.add_tok(receiver, inst);
            }
            (None, None, Some(_)) => {
                self.with_b(|m| builtins::native_parent_ctor(m, tm, call, inst))
            }
            (None, None, None) => {}
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
            parent: None,
            ctor: Some(f),
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
        });
        self.fn_inst.insert(f, id);
        id
    }

    /// `recv.key(args)` on token `t`.
    pub(super) fn method_on(&mut self, t: Tok, key: Key, call: CallId) {
        if t == TOP {
            self.call_tok(call, TOP, Some(TOP));
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
    }

    pub(super) fn mark_escaped(&mut self, t: Tok, reason: u32) {
        self.escaped.insert(t, reason);
        let w = (t >> 6) as usize;
        if self.esc_bits.len() <= w {
            self.esc_bits.resize(w + 1, 0);
        }
        self.esc_bits[w] |= 1 << (t & 63);
    }

    pub(super) fn escape_inner(&mut self, t: Tok, reason: u32) {
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
        // Own accessors run with `this` = ⊤-reached object: already modeled
        // by the accessor rule when unknown code reads; call them now.
        self.implicit(t, reason);
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
            TokKind::Func(f) => {
                let fi = self.funcs[f as usize].clone();
                let why =
                    |m: &Self| format!("param of escaped function: {}", m.reasons[reason as usize]);
                for &p in fi.params.iter().chain(fi.rest.iter()) {
                    self.add_top(p, why);
                    self.add_prim(p, prim::ALL);
                }
                if let Some(rest) = fi.rest {
                    let r = self.field_any_node_of_node(rest);
                    self.add_top(r, why);
                }
                self.add_top(fi.args_all, why);
                if !fi.lexical_this {
                    self.add_top(fi.this, |m| {
                        format!("this of escaped function: {}", m.reasons[reason as usize])
                    });
                    self.add_prim(fi.this, prim::ALL);
                }
                self.sink_node(fi.ret, reason);
            }
            TokKind::ClassObj(tm) => {
                let p = self.tok(TokKind::Proto(tm));
                self.escape(p, reason);
                // Unknown code may construct it with unknown arguments.
                let inst = self.tok(TokKind::Obj(tm));
                let ret = self.node_l("top-new");
                let args_n = self.node_l("top-args");
                self.add_top(args_n, |_| "constructor args from unknown code".into());
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
            TokKind::Obj(tm) => {
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
        if t == TOP {
            return;
        }
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
    }
}
