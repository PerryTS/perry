//! Call binding and class constructor chains for the inclusion solver.

use crate::builtins;
use crate::model::*;

impl Model {
    // ---------------------------------------------------------------- calls

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
    fn unknown_callee_args(&mut self, call: CallId, t: Tok, sink_this: bool) {
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
        let TokKind::Func(f) = self.toks[t as usize] else {
            return;
        };
        if !self.bind_done.insert((call, t, u32::MAX - 1)) {
            return;
        }
        let f = f as usize;
        let c = call as usize;
        let rest = self.funcs[f].rest;
        let args_all = self.funcs[f].args_all;
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
            self.edge(a, args_all);
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
            self.edge(elems, args_all);
        }
        let fret = self.funcs[f].ret;
        let cret = self.calls[c].ret;
        self.edge(fret, cret);
    }

    /// Calling a bound function: its target with the bound this; bound +
    /// call args land in any parameter (positions shift).
    fn call_bound(&mut self, call: CallId, t: Tok) {
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

    pub(crate) fn field_any_node_of_node(&mut self, arr_node: NodeId) -> NodeId {
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
        if !self.ctor_chain_done.insert((call, tm, inst)) {
            return;
        }
        let c = self.calls[call as usize].clone();
        if let Some(fi) = self.tmpls[tm as usize].field_init {
            let this = self.funcs[fi as usize].this;
            self.add_tok(this, inst);
        }
        let t = &self.tmpls[tm as usize];
        if t.ctor.is_none() {
            if let Some(parent) = t.evaluated_parent {
                self.cons(parent, Cons::ParentCtor { call, this: inst });
                return;
            }
        }
        match (t.ctor, t.parent, t.native_parent.clone()) {
            (Some(ctor), _, _) => {
                let ftok = self.funcs[ctor as usize].token;
                self.call_tok(call, ftok, Some(inst));
                let ret = self.funcs[ctor as usize].ret;
                self.ret_objects(ret, c.ret);
            }
            (None, Some(p), _) => {
                // Implicit `constructor(...a) { super(...a) }`.
                self.run_ctor_chain(call, p, inst);
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
            evaluated_parent: None,
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
}
