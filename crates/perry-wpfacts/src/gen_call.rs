//! Constraint generation, part 3: calls, closures, `new`, `super`, class
//! environments and runtime externs (`ExternFuncRef js_*`).

use crate::gen::{Fx, Gen, ReadSite};
use crate::model::*;
use perry_hir::{Expr, Stmt};

impl<'a> Gen<'a> {
    pub(crate) fn fn_by_id(&mut self, mi: u32, id: u32) -> FnIdx {
        if let Some(&f) = self.mods[mi as usize].funcs.get(&id) {
            return f;
        }
        // A FuncRef to a body we have not registered (should not happen):
        // a fresh function whose behaviour is unknown.
        let f = self.synthetic_fn(mi, &format!("<unknown fn {id}>"));
        let ret = self.m.funcs[f as usize].ret;
        self.m.add_top(ret, |_| "unregistered function id".into());
        let t = self.m.funcs[f as usize].token;
        let r = self.m.reason("unregistered function id");
        self.m.escape(t, r);
        self.mods[mi as usize].funcs.insert(id, f);
        f
    }

    pub(crate) fn call_expr(
        &mut self,
        fx: &Fx,
        callee: &Expr,
        args: &[Expr],
        _extra: &[NodeId],
        byte_offset: u32,
    ) -> NodeId {
        // The object-literal IIFE: `(o => { js_object_literal_define(o, …);
        // …; return o })(seed)`. Its defines are the literal's birth.
        if let (Expr::Closure { params, .. }, [seed]) = (callee, args) {
            if crate::decl_units::obj_literal_iife(params) && literal_seed(seed) {
                // An exported method literal's seed is its shape class,
                // its constructor storing `undefined` into every field the
                // defines then store, in order, before the object is
                // returned: no code sees a placeholder (the value
                // expressions cannot name the literal). The birth is the
                // layout's object, born as a literal is, without running
                // that constructor (design §18.9).
                let shape = match seed {
                    Expr::New { class_name, .. } => self.mods[fx.module as usize]
                        .classes
                        .get(class_name)
                        .copied(),
                    _ => None,
                };
                let lit = match shape {
                    Some(tm) => {
                        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
                        self.tok_node(t)
                    }
                    None => self.expr(fx, seed),
                };
                let tm = self.m.nodes[lit as usize]
                    .pts
                    .to_sorted()
                    .into_iter()
                    .find_map(|t| self.m.tmpl_of(t));
                if let Some(tm) = tm {
                    let pn = self.local(fx.module, params[0].id);
                    self.iife_birth.insert(pn, tm);
                }
                let c = self.expr(fx, callee);
                if let Expr::Closure { func_id, .. } = callee {
                    // Its parameter is the literal's birth, not a call input.
                    let f = self.fn_by_id(fx.module, *func_id);
                    self.m.funcs[f as usize].no_summary = true;
                }
                return self.call_value(c, ThisArg::Undef, vec![lit], vec![]);
            }
        }
        let a: Vec<NodeId> = args.iter().map(|x| self.expr(fx, x)).collect();
        self.call_lit_arg = match args.first().map(|x| self.const_eval(fx.module, x)) {
            Some(Some(crate::gen_const::Const::Str(s))) => Some(self.m.atom(&s)),
            _ => None,
        };
        if let Expr::ExternFuncRef { name, .. } = callee {
            if self.is_runtime_extern(fx.module, name) {
                let lits: Vec<Option<String>> = args
                    .iter()
                    .map(|x| match x {
                        Expr::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                return self.runtime_extern(fx, name, a, vec![], lits);
            }
        }
        self.call_with(fx, callee, a, vec![], byte_offset)
    }

    pub(crate) fn call_with(
        &mut self,
        fx: &Fx,
        callee: &Expr,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
        byte_offset: u32,
    ) -> NodeId {
        let lit = self.call_lit_arg.take();
        match callee {
            Expr::PropertyGet {
                object,
                property,
                byte_offset: pb,
                ..
            } => {
                let o = match self.ns_member(fx.module, object, property, true) {
                    Some(o) => o,
                    None => self.expr(fx, object),
                };
                let a = self.m.atom(property);
                let off = if *pb != 0 { *pb } else { byte_offset };
                if off != 0 {
                    self.read_sites.push(ReadSite {
                        module: fx.module,
                        byte_offset: off,
                        key: a,
                        recv: o,
                        is_call: true,
                        value_call: false,
                        func: fx.f,
                    });
                }
                self.mcall(o, Key::Named(a), args, spread)
            }
            Expr::IndexGet { object, index, .. } => {
                let o = self.expr(fx, object);
                let i = self.expr(fx, index);
                let r = self.m.reason("ToPropertyKey");
                self.m.cons(
                    i,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                match self.read_key(index) {
                    Key::Named(a) => self.mcall(o, Key::Named(a), args, spread),
                    // DECISIONS 217: the key's values decide which members
                    // it may name (`Cons::IdxMCall`).
                    Key::Any => {
                        let (call, ret) = self.new_call(ThisArg::Node(o), args, spread);
                        self.m.cons(o, Cons::IdxMCall { idx: i, call });
                        ret
                    }
                }
            }
            // Direct host timer calls use the same guarded global lookup
            // as globalThis.setTimeout. Import/module bindings shadow them.
            Expr::ExternFuncRef { name, .. }
                if matches!(name.as_str(), "setTimeout" | "setInterval" | "setImmediate" | "clearTimeout" | "clearInterval" | "clearImmediate" | "queueMicrotask")
                    && !self.is_import(fx.module, name)
                    && !self.mods[fx.module as usize].scope.contains_key(name) => {
                let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                let recv = self.tok_node(t); let key = self.m.atom(name);
                self.mcall(recv, Key::Named(key), args, spread)
            }
            Expr::ExternFuncRef { name, .. } if self.is_runtime_extern(fx.module, name) => {
                self.runtime_extern(fx, name, args, spread, vec![])
            }
            _ => {
                let c = self.expr(fx, callee);
                // A call through a value is a site: the bodies the callee
                // node holds. A function named by the source (a declaration,
                // an import binding, a closure expression) is no value call.
                if byte_offset != 0
                    && !matches!(
                        callee,
                        Expr::FuncRef(_) | Expr::ExternFuncRef { .. } | Expr::Closure { .. }
                    )
                {
                    let key = self.m.atom("()");
                    self.read_sites.push(ReadSite {
                        module: fx.module,
                        byte_offset,
                        key,
                        recv: c,
                        is_call: true,
                        value_call: true,
                        func: fx.f,
                    });
                }
                let r = self.call_value(c, ThisArg::Undef, args, spread);
                if let Some(a) = lit {
                    let call = self.m.calls.len() as CallId - 1;
                    self.m.call_lit.insert(call, a);
                }
                r
            }
        }
    }

    fn is_runtime_extern(&self, mi: u32, name: &str) -> bool {
        name.starts_with("js_")
            && !self.is_import(mi, name)
            && !self.mods[mi as usize].scope.contains_key(name)
    }

    /// `ExternFuncRef { name }` read as a value: an import binding, a
    /// module-scope name, or an unknown runtime/global value (⊤).
    pub(crate) fn extern_value(&mut self, mi: u32, name: &str) -> NodeId {
        if let Some(n) = self.import_value(mi, name) {
            return n;
        }
        if let Some(&n) = self.mods[mi as usize].scope.get(name) {
            return n;
        }
        self.m.top_val(&format!("extern value {name}"))
    }

    /// A closure expression: register, walk its body, return its token.
    pub(crate) fn closure(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let Expr::Closure { func_id: id, .. } = e else {
            return self.m.top_tok_node;
        };
        let canonical = self.emitted_closure(fx.module, *id).unwrap_or(e);
        self.feed_closure_captures(fx.module, e, canonical);
        let Expr::Closure {
            func_id,
            params,
            body,
            captures_this,
            is_arrow,
            ..
        } = canonical
        else {
            return self.m.top_tok_node;
        };
        let mi = fx.module;
        let name = self.modules[mi as usize]
            .1
            .closure_display_names
            .get(func_id)
            // DECISIONS 108: local functions of one name are different
            // functions (decision 23): the name carries the function id.
            .map(|n| format!("{n}#{func_id}"))
            .unwrap_or_else(|| format!("closure{func_id}"));
        let f = self.register_fn(mi, *func_id, &name, params, *is_arrow, true);
        let info = self.m.funcs[f as usize].clone();
        // An arrow captures the enclosing `this` only when its body (or an
        // arrow nested in it) reads `this` or `super` (design §14.2 rule
        // 1): a `this` nothing reads carries nothing, and capturing it
        // would key the closure's context by its creator's frame.
        if *captures_this || (*is_arrow && body.iter().any(stmt_reads_this)) {
            // Captured `this` (arrow semantics, or a slot the runtime
            // patches): the enclosing `this` flows in.
            self.m.edge(fx.this, info.this);
            self.m.captures.insert((fx.this, f));
        }
        let fn_ctor = !*is_arrow;
        let cfx = Fx {
            guard: None,
            module: mi,
            f,
            this: info.this,
            ret: info.ret,
            class: fx.class,
            ctor_of: None,
            new_target: fx.new_target,
            fn_ctor,
        };
        if !self.walked.contains(&f) {
            if fn_ctor {
                let ph = crate::ctor_phase::analyze_closure(params, body);
                self.note_fn_phase(f, ph);
            }
            // DECISIONS 225: its body is walked once the closure can run.
            let deferred = match self.emitted_closure(mi, *func_id) {
                Some(c) => self.defer_walk(f, crate::gen::PendingWalk::Closure(cfx.clone(), c)),
                None => false,
            };
            if !deferred {
                self.walk_closure_body(&cfx, canonical);
            }
        }
        self.tok_node(info.token)
    }

    /// The body of closure `e` in its context `cfx` (once per function).
    pub(crate) fn walk_closure_body(&mut self, cfx: &Fx, e: &Expr) {
        let f = cfx.f;
        if !self.walked.insert(f) {
            return;
        }
        self.m.note_walked(f);
        let Expr::Closure {
            params,
            body,
            is_async,
            is_generator,
            ..
        } = e
        else {
            return;
        };
        let ret = self.m.funcs[f as usize].ret;
        self.params(cfx, params);
        if *is_async || *is_generator {
            self.m
                .add_top(ret, |_| "async/generator closure result".into());
        }
        self.stmts(cfx, body);
        let _: &Vec<Stmt> = body;
    }

    /// `new Name(args)`.
    pub(crate) fn new_named(
        &mut self,
        fx: &Fx,
        name: &str,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
    ) -> NodeId {
        let mi = fx.module;
        let callee = if let Some(&tm) = self.mods[mi as usize].classes.get(name) {
            let t = self.m.tok(TokKind::ClassObj(tm));
            Some(self.tok_node(t))
        } else if let Some(n) = self.import_value(mi, name) {
            Some(n)
        } else {
            self.mods[mi as usize].scope.get(name).copied()
        };
        match callee {
            Some(c) => {
                let (call, ret) = self.new_call(ThisArg::Undef, args, spread);
                self.m.cons(c, Cons::New { call });
                ret
            }
            None => self.new_builtin(fx, name, args, spread),
        }
    }

    /// `new Name(...)` of a name that is not a program class: a builtin.
    pub(crate) fn new_builtin(
        &mut self,
        _fx: &Fx,
        name: &str,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
    ) -> NodeId {
        let (call, ret) = self.new_call(ThisArg::Undef, args, spread);
        let h = self.m.node_l("new-named");
        let mark = self.m.tok(TokKind::Builtin(BuiltinNs::Other));
        self.m.add_tok(h, mark);
        let name = self.m.atom(name);
        self.m.cons(h, Cons::NewNamed { name, call });
        ret
    }

    pub(crate) fn class_value(&mut self, mi: u32, name: &str) -> NodeId {
        if let Some(&tm) = self.mods[mi as usize].classes.get(name) {
            let t = self.m.tok(TokKind::ClassObj(tm));
            return self.tok_node(t);
        }
        if let Some(n) = self.import_value(mi, name) {
            return n;
        }
        if let Some(&n) = self.mods[mi as usize].scope.get(name) {
            return n;
        }
        self.m.top_val(&format!("unresolved class value {name}"))
    }

    /// One captured value of a class body (a slot of the class's capture
    /// environment): each slot is its own node, as each is its own binding.
    pub(crate) fn class_env(&mut self, mi: u32, name: &str, index: usize) -> NodeId {
        let key = format!("{mi}:{name}:{index}");
        if let Some(&n) = self.class_envs.get(&key) {
            return n;
        }
        let n = self
            .m
            .node_named("classenv", || format!("classenv({name}.{index})"));
        self.class_envs.insert(key, n);
        n
    }

    pub(crate) fn computed_accessor(
        &mut self,
        mi: u32,
        tm: TmplId,
        is_static: bool,
        getter: &Option<String>,
        setter: &Option<String>,
    ) {
        let holder = if is_static {
            self.m.tok(TokKind::ClassObj(tm))
        } else {
            self.m.tok(TokKind::Proto(tm))
        };
        let find = |gen: &Self, n: &str| -> Option<FnIdx> {
            let module = gen.modules[mi as usize].1;
            let TmplKind::Class { name, .. } = &gen.m.tmpls[tm as usize].kind else {
                return None;
            };
            let c = module.classes.iter().find(|c| &c.name == name)?;
            c.methods
                .iter()
                .chain(c.static_methods.iter())
                .chain(c.getters.iter().map(|(_, f)| f))
                .chain(c.setters.iter().map(|(_, f)| f))
                .chain(c.computed_members.iter().map(|m| &m.function))
                .find(|f| f.name == n)
                .and_then(|f| gen.mods[mi as usize].funcs.get(&f.id).copied())
        };
        // A computed-key accessor may answer any key: it runs as unknown
        // code, and any key read through the holder may yield anything (the
        // derivation kills the templates whose chain reaches it).
        let r = self.m.reason("accessor under a computed key");
        for name in [getter, setter].into_iter().flatten() {
            if let Some(f) = find(self, name) {
                let ft = self.m.funcs[f as usize].token;
                self.m.escape(ft, r);
            }
        }
        let fa = self.m.field_any(holder);
        self.m
            .add_top(fa, |_| "computed-key accessor on a class".into());
        self.m.add_prim(fa, prim::ALL);
        let star = self.m.atom(crate::STAR_KEY);
        crate::gen::push_unique(self.m.accessor_keys.entry(holder).or_default(), star);
        self.m.rel(|| format!("accstar {holder}"));
        self.m.tmpls[tm as usize].has_computed_members = true;
    }

    pub(crate) fn super_starts(&mut self, fx: &Fx) -> Vec<Tok> {
        match fx.class {
            Some(tm) => self.super_starts_of(tm),
            None => vec![TOP],
        }
    }

    pub(crate) fn super_starts_of(&mut self, tm: TmplId) -> Vec<Tok> {
        match self.m.tmpls[tm as usize].parent {
            Some(p) => vec![
                self.m.tok(TokKind::Proto(p)),
                self.m.tok(TokKind::ClassObj(p)),
            ],
            None => {
                // A parent value (§17.2): `super.x` starts at a prototype
                // the solve computes; unknown here.
                if self.m.tmpls[tm as usize].native_parent.is_some()
                    || self.m.tmpls[tm as usize].parent_value.is_some()
                {
                    vec![TOP]
                } else {
                    vec![self.m.tok(TokKind::ObjectProto)]
                }
            }
        }
    }

    pub(crate) fn super_ctor(&mut self, fx: &Fx, args: Vec<NodeId>, spread: Vec<NodeId>) -> NodeId {
        let (call, ret) = self.new_call(ThisArg::Node(fx.this), args, spread);
        match fx.class {
            Some(tm) => {
                self.m.cons(fx.this, Cons::SuperCtor { class: tm, call });
            }
            None => {
                let c = self.m.calls[call as usize].clone();
                for a in c.args.iter().chain(c.spread.iter()) {
                    self.to_sink(*a, "super() outside a known class");
                }
            }
        }
        // A superclass can return a replacement object, including a builtin
        // array. Subsequent derived-body this accesses must retain that flow.
        self.m.edge(ret, fx.this);
        self.m.edge(ret, fx.ret);
        ret
    }

    pub(crate) fn super_method(
        &mut self,
        fx: &Fx,
        method: &str,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
    ) -> NodeId {
        let (call, ret) = self.new_call(ThisArg::Node(fx.this), args, spread);
        let a = self.m.atom(method);
        for start in self.super_starts(fx) {
            self.m.cons(
                fx.this,
                Cons::Super {
                    start,
                    key: Key::Named(a),
                    act: ChainAct::Call(call),
                },
            );
        }
        ret
    }

    // ------------------------------------------------- runtime externs

    /// Direct calls to runtime entries that HIR lowering emits by name.
    fn runtime_extern(
        &mut self,
        fx: &Fx,
        name: &str,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
        lits: Vec<Option<String>>,
    ) -> NodeId {
        let lit = |i: usize| lits.get(i).cloned().flatten();
        let arg = |i: usize| args.get(i).copied();
        // Is the first argument the parameter of an object-literal IIFE?
        let birth_tm = self.iife_param_template(fx, &args);
        match name {
            "js_object_literal_define"
            | "js_object_set_method_by_name"
            | "js_object_set_symbol_method"
            | "js_object_set_property_key_method"
            | "js_object_literal_set_computed" => {
                let (Some(o), Some(v)) = (arg(0), arg(2)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let key = match (name, lit(1)) {
                    ("js_object_literal_define" | "js_object_set_method_by_name", Some(k)) => {
                        Key::Named(self.m.atom(&k))
                    }
                    _ => Key::Any,
                };
                if let Key::Named(a) = key {
                    if let Some(tm) = birth_tm {
                        crate::gen::push_unique(&mut self.m.tmpls[tm as usize].birth_keys, a);
                        crate::gen::push_unique(&mut self.m.tmpls[tm as usize].definite, a);
                    }
                } else if let Some(tm) = birth_tm {
                    self.m.tmpls[tm as usize].birth_keys.clear();
                    self.m.tmpls[tm as usize].definite.clear();
                    self.m.tmpls[tm as usize].has_computed_members = true;
                }
                self.store(
                    fx,
                    o,
                    key,
                    v,
                    name.to_string(),
                    birth_tm,
                    birth_tm.is_some(),
                );
                // Method closures see the object as `this`.
                if name != "js_object_literal_define" && name != "js_object_literal_set_computed" {
                    self.m.cons(v, Cons::ThisFrom { recv: o });
                }
                self.prim_node(prim::UNDEF)
            }
            "js_object_define_accessor" => {
                let Some(o) = arg(0) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let k = lit(1);
                let a = match &k {
                    Some(k) => self.m.atom(k),
                    None => self.m.atom(crate::STAR_KEY),
                };
                if k.is_some() {
                    self.m.add_accessor_atom(a);
                }
                if let Some(tm) = birth_tm {
                    // An accessor is never a data field (E25).
                    crate::gen::push_unique(&mut self.m.tmpls[tm as usize].accessor_keys, a);
                    if k.is_none() {
                        self.m.tmpls[tm as usize].has_computed_members = true;
                    }
                } else {
                    let r = self.m.reason("E5 accessor defined on an existing object");
                    self.m.cons(
                        o,
                        Cons::Hard {
                            kind: Hard::E5DefineProperty,
                            reason: r,
                        },
                    );
                }
                for (i, which) in [(2usize, 0u8), (3, 1)] {
                    if let Some(f) = arg(i) {
                        self.m.cons(
                            f,
                            Cons::Accessor {
                                holder: o,
                                key: a,
                                setter: which == 1,
                            },
                        );
                    }
                }
                self.prim_node(prim::UNDEF)
            }
            "js_object_literal_spread" => {
                let (Some(o), Some(s)) = (arg(0), arg(1)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let vals = self.load(s, Key::Any);
                if let Some(tm) = birth_tm {
                    self.m.tmpls[tm as usize].birth_keys.clear();
                    self.m.tmpls[tm as usize].definite.clear();
                    self.m.tmpls[tm as usize].has_computed_members = true;
                }
                self.store(
                    fx,
                    o,
                    Key::Any,
                    vals,
                    name.to_string(),
                    birth_tm,
                    birth_tm.is_some(),
                );
                self.prim_node(prim::UNDEF)
            }
            "js_object_literal_set_prototype" => {
                let (Some(o), Some(p)) = (arg(0), arg(1)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                if birth_tm.is_some() {
                    self.m.cons(o, Cons::ProtoFrom { src: p });
                } else {
                    let r = self.m.reason("E6 __proto__ set");
                    self.m.cons(
                        o,
                        Cons::Hard {
                            kind: Hard::E6SetPrototypeOf,
                            reason: r,
                        },
                    );
                    self.m.cons(o, Cons::ProtoFrom { src: p });
                }
                self.prim_node(prim::UNDEF)
            }
            "js_object_define_getter" | "js_object_define_setter" | "js_object_define_property" => {
                // E5 on the target.
                let r = self.m.reason("E5 defineProperty/__defineGetter__");
                if let Some(o) = arg(0) {
                    self.m.cons(
                        o,
                        Cons::Hard {
                            kind: Hard::E5DefineProperty,
                            reason: r,
                        },
                    );
                    self.m.cons(o, Cons::TopSink { reason: r });
                }
                for a in args.iter().skip(1).chain(spread.iter()) {
                    self.m.cons(*a, Cons::TopSink { reason: r });
                }
                arg(0).unwrap_or(self.m.top_tok_node)
            }
            "js_object_lookup_getter" | "js_object_lookup_setter" => {
                let r = self.m.reason("__lookupGetter__ result");
                for a in &args {
                    self.m.cons(
                        *a,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::Any,
                        },
                    );
                }
                self.m.top_val("__lookupGetter__ result")
            }
            "js_tdz_check_binding" | "js_tdz_check_binding_then" => {
                let out = self.tmp("tdz");
                if let Some(v) = arg(0) {
                    self.m.edge(v, out);
                }
                for a in args.iter().skip(1) {
                    self.m.edge(*a, out);
                }
                out
            }
            "js_to_property_key"
            | "js_object_literal_to_property_key"
            | "js_to_numeric"
            | "js_object_to_string"
            | "js_object_has_own"
            | "js_object_property_is_enumerable"
            | "js_process_exit_code_get"
            | "js_process_exit_code_set"
            | "js_weakset_has"
            | "js_weakmap_has"
            | "js_weakset_delete"
            | "js_weakmap_delete"
            | "js_weakset_add"
            | "js_map_compaction_epoch"
            | "js_set_compaction_epoch" => {
                let r = self.m.reason("ToPrimitive in runtime helper");
                for a in &args {
                    self.m.cons(
                        *a,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::ToPrimitive,
                        },
                    );
                }
                let out = self.tmp("rt-prim");
                self.m.add_prim(out, prim::ALL);
                if name == "js_weakset_add" {
                    if let Some(v) = arg(0) {
                        self.m.edge(v, out);
                    }
                }
                out
            }
            // SetFunctionName for a computed key (`{ [k]: f }`): the runtime
            // names the closure after the key, which
            // `js_object_literal_to_property_key` already converted. It calls
            // nothing, so the function does not escape: the value
            // (argument 1) passes through.
            "js_object_literal_infer_computed_function_name" => {
                let out = self.tmp("rt-name");
                if let Some(v) = arg(1) {
                    self.m.edge(v, out);
                }
                out
            }
            "js_weakmap_set" => {
                let (Some(mp), Some(v)) = (arg(0), arg(2)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let site = self.store_site(fx.module, fx.f, "weakmap.set".into(), None, false);
                self.m.cons(
                    mp,
                    Cons::Store {
                        key: Key::Any,
                        src: v,
                        site,
                    },
                );
                mp
            }
            "js_weakmap_get" => {
                let Some(mp) = arg(0) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let out = self.load(mp, Key::Any);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            "js_map_cursor_next" | "js_set_cursor_next" => {
                let Some(mp) = arg(0) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let out = self.load(mp, Key::Any);
                self.m.add_prim(out, prim::ALL);
                out
            }
            n if n.starts_with("js_throw_") => {
                for a in &args {
                    self.m.edge(*a, self.thrown);
                }
                self.prim_node(prim::UNDEF)
            }
            // `yield*`'s pull (`perry-runtime` `iterator_close.rs`): Call(next,
            // iterator, « value »), or for a sync generator delegate stepped
            // through its intrinsic `next`, the value that result would carry
            // (`value`), read from its record. The kind reads a record control.
            "js_iterator_delegate_step" => {
                let (Some(it), Some(_next), Some(v)) = (arg(0), arg(1), arg(2)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                // `next` was read from `it` once (the captured method): the
                // pull is that method call on `it`, which also runs a
                // generator's step closure (a generator object is its step).
                let nk = Key::Named(self.m.atom("next"));
                let res = self.mcall(it, nk, vec![v], vec![]);
                let va = self.m.atom("value");
                let raw = self.load(res, Key::Named(va));
                let out = self.tmp("delegate-step");
                self.m.edge(res, out);
                self.m.edge(raw, out);
                out
            }
            // A `yield*` re-yield returns its pull's kind: this step closure
            // may hand back an inner result object (kind 2).
            "js_iterator_delegate_kind" => {
                self.m.funcs[fx.f as usize].gen_reyields = true;
                self.prim_node(prim::NUM)
            }
            // The runtime calls the delegate's own `throw` / `return` with
            // the value (`perry-runtime` `iterator_close.rs`); a delegate
            // without `throw` is closed through `return` first.
            "js_iterator_delegate_throw" | "js_iterator_delegate_return" => {
                let (Some(it), Some(v)) = (arg(0), arg(1)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let k = if name == "js_iterator_delegate_throw" {
                    "throw"
                } else {
                    "return"
                };
                let a = self.m.atom(k);
                let out = self.mcall(it, Key::Named(a), vec![v], vec![]);
                if k == "throw" {
                    let ra = self.m.atom("return");
                    let _ = self.mcall(it, Key::Named(ra), vec![], vec![]);
                }
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            // A module-singleton `{}` (`perry-transform` `prototype_birth.rs`).
            "js_object_literal_prototype_birth" => {
                let (_, t) = self.new_lit_tmpl(fx.module, &[]);
                self.tok_node(t)
            }
            "js_vm_module_call" => {
                crate::builtins::set_dyn_eval(&mut self.m, "E14 node:vm");
                self.extern_fail(name, &args, &spread)
            }
            // `module.createRequire` as a value call: the program's require.
            // Its loader resolves compiled modules, builtins, addons and JSON;
            // it never interprets code (`perry-runtime` `module_require.rs`).
            "js_module_create_require_devirt" => {
                let t = self.m.tok(TokKind::Builtin(BuiltinNs::Require));
                self.tok_node(t)
            }
            _ => self.extern_fail(name, &args, &spread),
        }
    }

    pub(crate) fn extern_fail(&mut self, name: &str, args: &[NodeId], spread: &[NodeId]) -> NodeId {
        let why = format!("argument of unmodeled runtime entry {name}");
        for a in args.iter().chain(spread.iter()) {
            self.to_sink(*a, &why);
        }
        self.m
            .top_val(&format!("result of unmodeled runtime entry {name}"))
    }

    /// The literal template born by an object-literal IIFE when the first
    /// argument is that IIFE's parameter.
    fn iife_param_template(&self, _fx: &Fx, args: &[NodeId]) -> Option<TmplId> {
        let first = *args.first()?;
        self.iife_birth.get(&first).copied()
    }
}

/// The first value of an object-literal IIFE's parameter: `{}`, or the
/// shape-class seed of an exported method literal (`new Shape(undefined,
/// …)`, `perry-hir` `expr_object.rs` `static_shape_seed`).
fn literal_seed(seed: &Expr) -> bool {
    match seed {
        Expr::Object(props) => props.is_empty(),
        Expr::New { args, .. } => args.iter().all(|a| matches!(a, Expr::Undefined)),
        _ => false,
    }
}

/// `stmt` reads the `this` an arrow inherits: `this` or `super`, in it or
/// in a nested arrow. A non-arrow closure has its own `this`. The HIR's
/// `captures_this` says the same for parsed arrows; synthetic arrows set
/// it to false, so the body is the authority.
pub(crate) fn stmt_reads_this(stmt: &Stmt) -> bool {
    perry_hir::walker::stmt_any_expr(stmt, &mut expr_reads_this)
}

fn expr_reads_this(e: &Expr) -> bool {
    match e {
        Expr::This
        | Expr::SuperCall(_)
        | Expr::SuperCallSpread(_)
        | Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. }
        | Expr::SuperPropertyGet { .. }
        | Expr::SuperPropertySet { .. }
        | Expr::ObjectSuperPropertyGet { .. }
        | Expr::ObjectSuperPropertySet { .. }
        | Expr::ObjectSuperMethodCall { .. } => true,
        Expr::Closure {
            is_arrow: false, ..
        } => false,
        Expr::Closure {
            captures_this: true,
            ..
        } => true,
        Expr::Closure { body, .. } => body.iter().any(stmt_reads_this),
        _ => {
            let mut found = false;
            perry_hir::walker::walk_expr_children(e, &mut |c| {
                if !found && expr_reads_this(c) {
                    found = true;
                }
            });
            found
        }
    }
}
