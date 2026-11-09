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
        byte_end: u32,
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
                #[cfg(test)]
                let direct_birth = self
                    .m
                    .packet_scope
                    .as_ref()
                    .map(|_| self.m.packet_direct_template(lit));
                #[cfg(not(test))]
                let direct_birth: Option<Option<TmplId>> = None;
                let tm = direct_birth.unwrap_or_else(|| {
                    self.m.nodes[lit as usize]
                        .pts
                        .to_sorted()
                        .into_iter()
                        .find_map(|t| self.m.tmpl_of(t))
                });
                if let Some(tm) = tm {
                    let pn = self.local(fx.module, params[0].id);
                    self.iife_birth.insert(pn, tm);
                }
                #[cfg(test)]
                if let Expr::Closure { func_id, .. } = callee {
                    self.m
                        .packet_shared_birth(self.modules[fx.module as usize].0, *func_id, lit);
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
        if self.int_ranges && byte_offset != 0 && matches!(callee, Expr::PropertyGet { .. }) {
            self.call_args_at.push((fx.module, byte_offset, a.clone()));
        }
        self.call_lit_arg = if matches!(callee, Expr::PropertyGet { .. } | Expr::IndexGet { .. }) {
            args.first()
                .and_then(|x| self.const_property_key(fx.module, x))
                .map(|s| self.m.atom(&s))
        } else {
            match args.first().and_then(|x| self.const_eval(fx.module, x)) {
                Some(crate::gen_const::Const::Str(s)) => Some(self.m.atom(&s)),
                _ => None,
            }
        };
        if let Expr::ExternFuncRef { name, .. } = callee {
            if self.is_runtime_extern(fx.module, name) {
                let lits: Vec<Option<String>> = args
                    .iter()
                    .map(|x| match x {
                        Expr::String(s) => Some(s.clone()),
                        _ if matches!(
                            name.as_str(),
                            "js_object_define_getter" | "js_object_define_setter"
                        ) =>
                        {
                            self.const_property_key(fx.module, x)
                        }
                        _ => None,
                    })
                    .collect();
                return self.runtime_extern(fx, name, a, vec![], lits);
            }
        }
        let result = self.call_with(fx, callee, a, vec![], (byte_offset, byte_end));
        if byte_offset != 0 && matches!(callee, Expr::PropertyGet { .. }) {
            self.call_results_at.push((fx.module, byte_offset, result));
        }
        result
    }

    /// The key of member access `object.property`: a guarded private
    /// field access (`obj.#x`, lowered with the field's storage key) names
    /// the private field, any other the public key of that spelling.
    pub(crate) fn member_atom(&mut self, object: &Expr, property: &str) -> Atom {
        match object {
            Expr::PrivateGuard { kind: 0, .. } => self.m.private_atom(property),
            _ => self.m.atom(property),
        }
    }

    pub(crate) fn call_with(
        &mut self,
        fx: &Fx,
        callee: &Expr,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
        (byte_offset, byte_end): (u32, u32),
    ) -> NodeId {
        let lit = self.call_lit_arg.take();
        match callee {
            Expr::PropertyGet {
                object,
                property,
                byte_offset: pb,
                byte_end: pe,
                ..
            } => {
                let o = match self.ns_member(fx.module, object, property, true) {
                    Some(o) => o,
                    None => self.expr(fx, object),
                };
                self.own_test_recv = Some(o);
                let private = self.private_method_callee(fx.module, object);
                let a = self.member_atom(object, property);
                let (off, end) = if *pb != 0 {
                    (*pb, *pe)
                } else {
                    (byte_offset, byte_end)
                };
                if off != 0 {
                    self.call_sites_at.push(crate::signatures::CallSiteRec {
                        module: fx.module,
                        offset: off,
                        end,
                        args: args.clone(),
                        spread: !spread.is_empty(),
                        callee: None,
                    });
                }
                if off != 0 {
                    self.read_sites.push(ReadSite {
                        module: fx.module,
                        byte_offset: off,
                        key: a,
                        recv: private.unwrap_or(o),
                        is_call: true,
                        value_call: private.is_some(),
                        func: fx.f,
                    });
                }
                let (call, ret) = self.new_call(ThisArg::Node(o), args, spread);
                if let Some(a) = lit {
                    self.m.call_lit.insert(call, a);
                }
                match private {
                    Some(callee) => self.m.cons(callee, Cons::Call { call }),
                    None => self.m.cons(
                        o,
                        Cons::MCall {
                            key: Key::Named(a),
                            call,
                        },
                    ),
                }
                ret
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
                let key = self.read_key(fx.module, index);
                let idx = self.for_in_key_of(fx.module, index).unwrap_or(i);
                self.element_site(fx, callee, o, idx, key, crate::derive::ElementOp::Call);
                let (call, ret) = self.new_call(ThisArg::Node(o), args, spread);
                if let Some(a) = lit {
                    self.m.call_lit.insert(call, a);
                }
                match self.read_key(fx.module, index) {
                    Key::Named(a) => {
                        self.m.cons(
                            o,
                            Cons::MCall {
                                key: Key::Named(a),
                                call,
                            },
                        );
                        ret
                    }
                    // DECISIONS 217: the key's values decide which members
                    // it may name (`Cons::IdxMCall`).
                    Key::Any(_) => {
                        let shape = self.key_shape(fx.module, index);
                        self.m.cons(
                            o,
                            Cons::IdxMCall {
                                idx: i,
                                call,
                                shape,
                            },
                        );
                        ret
                    }
                }
            }
            // Direct host timer calls use the same guarded global lookup
            // as globalThis.setTimeout. Import/module bindings shadow them.
            // Unit contracts retain their direct model. The whole-program
            // path below reads the same value an alias reads.
            Expr::ExternFuncRef { name, .. }
                if self.m.unit_mode
                    && crate::builtins::global_function(&self.m, name)
                    && !self.is_import(fx.module, name)
                    && !self.mods[fx.module as usize].scope.contains_key(name) =>
            {
                let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                let recv = self.tok_node(t);
                let key = self.m.atom(name);
                self.mcall(recv, Key::Named(key), args, spread)
            }
            Expr::ExternFuncRef { name, .. } if self.is_runtime_extern(fx.module, name) => {
                self.runtime_extern(fx, name, args, spread, vec![])
            }
            _ => {
                let c = self.expr(fx, callee);
                self.own_test_recv = Some(c);
                if byte_offset != 0 {
                    self.call_sites_at.push(crate::signatures::CallSiteRec {
                        module: fx.module,
                        offset: byte_offset,
                        end: byte_end,
                        args: args.clone(),
                        spread: !spread.is_empty(),
                        callee: Some(c),
                    });
                }
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
                // The literal is the call's before its callee resolves it:
                // a callee node that already holds the callee (the program's
                // `require` from `createRequire`) runs the call at `cons`.
                let (call, ret) = self.new_call(ThisArg::Undef, args, spread);
                if let Some(a) = lit {
                    self.m.call_lit.insert(call, a);
                }
                self.m.cons(c, Cons::Call { call });
                ret
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
        if !self.m.unit_mode && crate::builtins::global_function(&self.m, name) {
            let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
            let dst = self.m.node_l("global-function-value");
            let key = Key::Named(self.m.atom(name));
            self.m.load_from(t, t, key, dst);
            return dst;
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
        let Expr::Closure {
            is_async,
            is_generator,
            ..
        } = canonical
        else {
            unreachable!()
        };
        let module = self.modules[mi as usize].1;
        let kind = perry_hir::fn_proto::FnProtoKind::of_closure(
            module,
            *func_id,
            *is_async,
            *is_generator,
            body,
        );
        let f = self.register_fn(mi, *func_id, &name, params, *is_arrow, true, Some(kind));
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
                None => {
                    #[cfg(test)]
                    {
                        if self.m.packets_inert {
                            self.pending_walk.entry(f).or_default().push(
                                crate::gen::PendingWalk::OwnedClosure(
                                    cfx.clone(),
                                    Box::new(canonical.clone()),
                                ),
                            );
                            true
                        } else {
                            false
                        }
                    }
                    #[cfg(not(test))]
                    {
                        false
                    }
                }
            };
            if !deferred {
                self.walk_closure_body(&cfx, canonical);
            }
        }
        self.tok_node(info.token)
    }

    /// The body of closure `e` in its context `cfx` (once per function).
    pub(crate) fn walk_closure_body(&mut self, cfx: &Fx, e: &Expr) {
        if self.walked.contains(&cfx.f) {
            return;
        }
        let r = self.m.raise_of(cfx.f);
        let region = std::mem::replace(&mut self.m.gen_region, Some(r));
        self.walk_closure_body_region(cfx, e);
        self.m.gen_region = region;
    }

    fn walk_closure_body_region(&mut self, cfx: &Fx, e: &Expr) {
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
            is_arrow,
            is_strict,
            captures,
            ..
        } = e
        else {
            return;
        };
        let ret = self.m.funcs[f as usize].ret;
        // A sloppy function expression's body reads its `this` through the
        // view (`sloppy_this`); an arrow's `this` is its creator's.
        let sloppy = !*is_strict && !*is_arrow && crate::ctor_phase::body_mentions_this(body);
        let mut sfx;
        let cfx = if sloppy {
            sfx = cfx.clone();
            let prev = std::mem::replace(&mut self.m.cur_fn, f);
            sfx.this = self.sloppy_this(cfx.this);
            self.m.cur_fn = prev;
            &sfx
        } else {
            cfx
        };
        // RPG1b: its statements are ordered in its own body; a parameter
        // default is not.
        let saved = (self.order_ok, self.loop_depth);
        (self.order_ok, self.loop_depth) = (false, 0);
        #[cfg(test)]
        let packet_prior = self.m.packet_body(f);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
        #[cfg(test)]
        self.m.packet_declarations(body);
        self.params(cfx, params);
        if *is_async || *is_generator {
            self.m
                .add_top(ret, |_| "async/generator closure result".into());
        }
        self.order_ok = true;
        let own = self.own_helper_shape(params, body, *is_async || *is_generator);
        let scope = self.narrow_enter_body(cfx.module, f, params, body, captures);
        self.stmts(cfx, body);
        if crate::gen::narrow::may_complete(body) {
            // The body's normal completion returns `undefined`.
            self.m.add_prim(ret, prim::UNDEF);
        }
        self.narrow_exit_body(scope);
        self.own_helper_finish(f, own);
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
        }
        (self.order_ok, self.loop_depth) = saved;
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
            let c = self.tok_node(t);
            // A default-derived specialization may be constructed as the
            // class it was specialized from (codegen `super_constructor::new`
            // constructs that class value when the heritage is live): the
            // program then builds an instance of the class as written and
            // runs its default constructor and field initializers. Which of
            // the two the program builds is decided by facts about the
            // heritage, so the callee holds both.
            for origin in self.written_classes(mi, name) {
                let o = self.m.tok(TokKind::ClassObj(origin));
                self.m.add_tok(c, o);
            }
            Some(c)
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

    /// The classes a default-derived specialization `name` of module `mi`
    /// was specialized from, nearest first (`Class::is_default_derived`).
    fn written_classes(&self, mi: u32, name: &str) -> Vec<TmplId> {
        let module = self.modules[mi as usize].1;
        let mut out = Vec::new();
        let mut cur = module.classes.iter().find(|c| c.name == name);
        for _ in 0..module.classes.len() {
            let Some(c) = cur.filter(|c| c.is_default_derived()) else {
                break;
            };
            let Some(origin) = c.specialized_from.as_deref() else {
                break;
            };
            let Some(&tm) = self.mods[mi as usize].classes.get(origin) else {
                break;
            };
            out.push(tm);
            cur = module.classes.iter().find(|k| k.name == origin);
        }
        out
    }

    /// `new Name(...)` of a name that is not a program class: a builtin.
    pub(crate) fn new_builtin(
        &mut self,
        _fx: &Fx,
        name: &str,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
    ) -> NodeId {
        // `new Array(k)` of an integer literal: the length its birth has.
        let birth_len = match args.as_slice() {
            [a] if self.arr_lengths && name == "Array" && spread.is_empty() => self
                .num_lits
                .iter()
                .find(|(_, &n)| n == *a)
                .map(|(&k, _)| k)
                .filter(|k| (0..=i64::from(u32::MAX)).contains(k)),
            _ => None,
        };
        let (call, ret) = self.new_call(ThisArg::Undef, args, spread);
        if let Some(k) = birth_len {
            let site = self.m.calls[call as usize].site;
            self.m.arr_len_at_site.insert(site, k);
        }
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
        // No binding of the program: the global object's property, when
        // the realm has one under the name (`AbortSignal`): a load of it,
        // which reads the realm value while the program writes no value
        // under the key (`native_written`) and what it wrote once it does.
        if !self.m.unit_mode
            && self
                .m
                .namespace_property(BuiltinNs::GlobalThis, name)
                .is_some_and(|p| p.native.is_some())
        {
            let g = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
            let gn = self.tok_node(g);
            let a = self.m.atom(name);
            return self.load(gn, Key::Named(a));
        }
        self.m.top_val(&format!("unresolved class value {name}"))
    }

    /// The class value of `instanceof Name` when `Name` is a user class
    /// (local, or imported): codegen tests against that value
    /// (`js_instanceof_dynamic`), not a class id. `None`: a builtin name.
    pub(crate) fn instanceof_class(&mut self, mi: u32, name: &str) -> Option<NodeId> {
        if let Some(&tm) = self.mods[mi as usize].classes.get(name) {
            let t = self.m.tok(TokKind::ClassObj(tm));
            return Some(self.tok_node(t));
        }
        self.import_value(mi, name)
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

    /// The home objects of a `super` access in a method of `fx`'s class:
    /// its prototype and its class object (`Fx` does not say whether the
    /// method is static); ⊤ outside a known class.
    pub(crate) fn super_homes(&mut self, fx: &Fx) -> NodeId {
        let n = self.tmp("super.home");
        match fx.class {
            Some(tm) => {
                let p = self.m.tok(TokKind::Proto(tm));
                self.m.add_tok(n, p);
                let c = self.m.tok(TokKind::ClassObj(tm));
                self.m.add_tok(n, c);
            }
            None => self.m.add_tok(n, TOP),
        }
        n
    }

    /// `super[key]` (a read, store or call) in a method whose home object
    /// is in `homes`: the lookup starts at the home object's
    /// [[GetPrototypeOf]] at the access (`Cons::ProtoOf`: its declared
    /// parent and what `Object.setPrototypeOf` of the home wrote), with each
    /// token of `recv` as the receiver.
    pub(crate) fn super_access(&mut self, homes: NodeId, recv: NodeId, key: Key, act: ChainAct) {
        let base = self.tmp("super.base");
        self.m.cons(homes, Cons::ProtoOf { dst: base });
        self.m.cons(base, Cons::SuperFrom { recv, key, act });
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
        let homes = self.super_homes(fx);
        self.super_access(homes, fx.this, Key::Named(a), ChainAct::Call(call));
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
                    _ => Key::any(),
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
                self.m.define_on.insert((o, a, false));
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
                let vals = self.load(s, Key::any());
                if let Some(tm) = birth_tm {
                    self.m.tmpls[tm as usize].birth_keys.clear();
                    self.m.tmpls[tm as usize].definite.clear();
                    self.m.tmpls[tm as usize].has_computed_members = true;
                }
                self.store(
                    fx,
                    o,
                    Key::any(),
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
            "js_object_define_getter" | "js_object_define_setter" => {
                let (Some(o), Some(k), Some(f)) = (arg(0), arg(1), arg(2)) else {
                    return self.prim_node(prim::UNDEF);
                };
                let (call, ret) = self.new_call(ThisArg::Node(o), vec![k, f], spread);
                if let Some(k) = lit(1) {
                    let a = self.m.atom(&k);
                    self.m.call_lit.insert(call, a);
                }
                self.m.cons(
                    o,
                    Cons::AnnexBDefine {
                        call,
                        setter: name == "js_object_define_setter",
                    },
                );
                self.m.add_prim(ret, prim::UNDEF);
                ret
            }
            "js_object_define_property" => {
                // E5 on the target.
                let r = self.m.reason("E5 defineProperty");
                if let Some(o) = arg(0) {
                    let star = self.m.atom(crate::STAR_KEY);
                    self.m.define_on.insert((o, star, false));
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
                let Some(o) = arg(0) else {
                    return self.m.top_val("accessor lookup without a receiver");
                };
                let (call, ret) = self.new_call(ThisArg::Node(o), args[1..].to_vec(), spread);
                if let Some(k) = lit(1) {
                    let a = self.m.atom(&k);
                    self.m.call_lit.insert(call, a);
                }
                self.m.cons(
                    o,
                    Cons::AnnexBLookup {
                        call,
                        setter: name == "js_object_lookup_setter",
                    },
                );
                ret
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
                        key: Key::any(),
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
                let out = self.load(mp, Key::any());
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            "js_map_cursor_next" | "js_set_cursor_next" => {
                let Some(mp) = arg(0) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let out = self.load(mp, Key::any());
                self.m.add_prim(out, prim::ALL);
                out
            }
            n if n.starts_with("js_throw_") => {
                let r = self.region();
                for a in &args {
                    self.m.edge(*a, r);
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
            // DEC 255 (hunk 2): the strict existing-global store `NS = v`
            // (`perry-hir` `strict_global_assign_existing_or_throw`) is a
            // store into the global object under that name, as
            // `globalThis.NS = v` is: the same `native_writes[(GlobalThis,
            // NS)]` the namespace call's generic arm reads. The assignment
            // expression evaluates to its RHS.
            "js_global_assign_existing_or_throw" => {
                let (Some(k), Some(v)) = (lit(0), arg(1)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                let recv = self.tok_node(t);
                let key = Key::Named(self.m.atom(&k));
                self.store(fx, recv, key, v, name.to_string(), None, false);
                v
            }
            // Rule G: an unresolved identifier is `[[Get]](globalThis, NAME)`
            // (`perry-runtime` `error/global_ref.rs`; absent = ReferenceError,
            // `typeof` = "undefined"), as the load `globalThis.NAME` is.
            // Whole-program mode only: in per-unit mode another unit's
            // stores are unseen, so the read stays unknown.
            "js_global_get_or_throw_unresolved" | "js_global_get_optional" if !self.m.unit_mode => {
                let Some(k) = lit(0) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                let recv = self.tok_node(t);
                let key = Key::Named(self.m.atom(&k));
                self.load(recv, key)
            }
            // A named ESM import of a node module read as a value
            // (`import { performance } from "perf_hooks"`, or a destructured
            // `require` HIR binds the same way): node's builtin ESM export
            // cell holds `module.exports[k]` at load and changes only by
            // `syncBuiltinESMExports()`, which copies `module.exports[k]`. So
            // it holds what key `k` of the module object reads: the realm's
            // declaration, or what the program stored there.
            "js_native_module_named_esm_export_value" if !self.m.unit_mode => {
                let (Some(module), Some(k)) = (lit(0), lit(1)) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let Some(ns) = crate::builtins::node_module_ns(&module) else {
                    return self.extern_fail(name, &args, &spread);
                };
                let t = self.m.tok(TokKind::Builtin(ns));
                let recv = self.tok_node(t);
                let key = Key::Named(self.m.atom(&k));
                self.load(recv, key)
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
