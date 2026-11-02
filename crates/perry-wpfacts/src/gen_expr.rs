//! Constraint generation, part 2: expressions.
//!
//! `expr` returns the node holding the expression's value. The match in
//! `expr` has no wildcard arm: every HIR variant is either modeled here,
//! grouped as a primitive-only operation (`prim_op`), or fails closed
//! (`fail_closed`: operands escape to ⊤, the result is ⊤). A new HIR variant
//! is therefore a compile error, never a silent hole (design 3.5).

use crate::gen::{Fx, Gen, ReadSite};
use crate::model::*;
use perry_hir::{ArrayElement, CallArg, CompareOp, Expr, LogicalOp, UnaryOp};

/// The sink reason of an unmodeled construct's operands (`Gen::fail_closed`).
pub(crate) const UNMODELED_OPERAND: &str = "operand of unmodeled HIR construct";

impl<'a> Gen<'a> {
    pub(crate) fn tmp(&mut self, what: &'static str) -> NodeId {
        let n = self.m.node_l(what);
        self.m.own(n);
        n
    }

    pub(crate) fn tok_node(&mut self, t: Tok) -> NodeId {
        let n = self.m.node_l("tok");
        self.m.own(n);
        self.m.add_tok(n, t);
        n
    }

    pub(crate) fn to_sink(&mut self, v: NodeId, why: &str) {
        let r = self.m.reason(why);
        self.m.sink_node(v, r);
    }

    pub(crate) fn new_call(
        &mut self,
        this: ThisArg,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
    ) -> (CallId, NodeId) {
        let ret = self.tmp("callret");
        let id = self.m.calls.len() as CallId;
        if let Some(d) = &mut self.diagnostic {
            d.calls.insert(ret, id);
        }
        if let Some(d) = &mut self.m.diagnostic {
            d.source_calls.insert(id);
        }
        self.m.calls.push(CallSite {
            this,
            args,
            spread,
            ret,
            site: id,
        });
        (id, ret)
    }

    /// Operands only ever read as primitives by the runtime: objects get the
    /// protocol touch (ToPrimitive / toString …), functions escape (the
    /// runtime may call them back).
    pub(crate) fn prim_op(
        &mut self,
        fx: &Fx,
        operands: &[&Expr],
        result: u16,
        why: &str,
        proto: Protocol,
    ) -> NodeId {
        let r = self.m.reason(why);
        for o in operands {
            let v = self.expr(fx, o);
            self.m.cons(v, Cons::Implicit { reason: r, proto });
        }
        let out = self.tmp("primop");
        self.m.add_prim(out, result);
        out
    }

    /// A primitive operation whose result may be a runtime container of
    /// primitives (path.parse, os.cpus, process.memoryUsage …).
    pub(crate) fn prim_box_op(
        &mut self,
        fx: &Fx,
        operands: &[&Expr],
        why: &str,
        proto: Protocol,
    ) -> NodeId {
        let out = self.prim_op(fx, operands, prim::ALL, why, proto);
        let tm = self.new_bobj_template(fx.module, "primbox");
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let fa = self.m.field_any(t);
        self.m.add_prim(fa, prim::ALL);
        // Nested containers (os.cpus() elements ...) are the site's own.
        self.m.add_tok(fa, t);
        self.m.add_tok(out, t);
        out
    }

    /// Fail closed: every operand escapes, the result is ⊤.
    /// The node a conditional's test reads and the guards of its two
    /// branches: a comparison with `undefined`/`null` tests its operand,
    /// anything else tests its own value's truthiness.
    pub(crate) fn guard_test(&mut self, fx: &Fx, condition: &Expr) -> (NodeId, u8, u8) {
        use crate::model::guard::*;
        if let Expr::Compare { op, left, right } = condition {
            let (operand, nil) = match (left.as_ref(), right.as_ref()) {
                (x, Expr::Undefined) | (Expr::Undefined, x) => (Some(x), Expr::Undefined),
                (x, Expr::Null) | (Expr::Null, x) => (Some(x), Expr::Null),
                _ => (None, Expr::Undefined),
            };
            if let Some(x) = operand {
                let strict_undef = matches!(nil, Expr::Undefined);
                let tests = match op {
                    CompareOp::Eq if strict_undef => Some((UNDEF, NOT_UNDEF)),
                    CompareOp::Ne if strict_undef => Some((NOT_UNDEF, UNDEF)),
                    CompareOp::LooseEq => Some((NULLISH, NOT_NULLISH)),
                    CompareOp::LooseNe => Some((NOT_NULLISH, NULLISH)),
                    _ => None,
                };
                if let Some((then_test, else_test)) = tests {
                    let v = self.expr(fx, x);
                    return (v, then_test, else_test);
                }
            }
        }
        (self.expr(fx, condition), TRUTHY, FALSY)
    }

    pub(crate) fn fail_closed(&mut self, fx: &Fx, e: &Expr, name: &str) -> NodeId {
        let why = format!("{UNMODELED_OPERAND} {name}");
        let mut kids: Vec<&Expr> = Vec::new();
        perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
        for c in kids {
            let v = self.expr(fx, c);
            self.to_sink(v, &why);
        }
        self.m
            .top_val(&format!("result of unmodeled HIR construct {name}"))
    }

    pub(crate) fn evaluate_all(&mut self, fx: &Fx, es: &[Expr]) -> Vec<NodeId> {
        es.iter().map(|e| self.expr(fx, e)).collect()
    }

    pub(crate) fn call_args(&mut self, fx: &Fx, args: &[CallArg]) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut a = Vec::new();
        let mut s = Vec::new();
        for arg in args {
            match arg {
                CallArg::Expr(e) => a.push(self.expr(fx, e)),
                CallArg::Spread(e) => {
                    let v = self.expr(fx, e);
                    let r = self.m.reason("spread argument iteration");
                    self.m.cons(
                        v,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::Iterate,
                        },
                    );
                    s.push(v);
                }
            }
        }
        (a, s)
    }

    /// Is this store a construction-phase writer of the current ctor?
    fn phase_owner(&self, fx: &Fx, e: &Expr, object: &Expr) -> Option<TmplId> {
        let tm = fx.ctor_of?;
        if !matches!(object, Expr::This) {
            return None;
        }
        let ph = self.ctor_phase.get(&tm)?;
        if ph.parent_leaks {
            return None;
        }
        ph.phase_stores
            .contains(&(e as *const Expr as usize))
            .then_some(tm)
    }

    pub(crate) fn store(
        &mut self,
        fx: &Fx,
        recv: NodeId,
        key: Key,
        src: NodeId,
        desc: String,
        owner: Option<TmplId>,
        birth: bool,
    ) -> SiteId {
        let site = self.store_site(fx.module, fx.f, desc, owner, birth);
        self.m.cons(recv, Cons::Store { key, src, site });
        site
    }

    /// A store whose site may be a construction-phase writer of `new f()`
    /// (`mark_fn_phase`). The phase is set before the constraint exists:
    /// generation solves online, so a `Store` already applies to what `recv`
    /// holds, and a site marked afterwards would also count as a
    /// post-construction writer of those instances.
    #[allow(clippy::too_many_arguments)]
    fn store_phased(
        &mut self,
        fx: &Fx,
        e: &Expr,
        object: &Expr,
        recv: NodeId,
        key: Key,
        src: NodeId,
        desc: String,
        owner: Option<TmplId>,
        key_value: Option<NodeId>,
    ) -> SiteId {
        let site = self.store_site(fx.module, fx.f, desc, owner, false);
        // The key node is on the site before its constraint exists: the
        // solve is online, so the store may fire inside `cons`.
        self.m.stores[site as usize].key_value = key_value;
        self.mark_fn_phase(fx, e, object, site);
        self.mark_rooted(e, site);
        self.m.cons(recv, Cons::Store { key, src, site });
        site
    }

    /// DECISIONS 149b: a receiver-rooted store's site says so before its
    /// constraint exists (generation solves online).
    fn mark_rooted(&mut self, e: &Expr, site: SiteId) {
        if let Some(&deep) = self.rooted_stores.get(&(e as *const Expr as usize)) {
            self.m.stores[site as usize].rooted = true;
            self.m.stores[site as usize].deep = deep;
        }
    }

    /// An element store (`o[k] = v`): no construction phase, maybe rooted.
    fn store_elem(
        &mut self,
        fx: &Fx,
        e: &Expr,
        recv: NodeId,
        key: Key,
        key_value: NodeId,
        src: NodeId,
        desc: &str,
    ) {
        let site = self.store_site(fx.module, fx.f, desc.into(), None, false);
        self.m.stores[site as usize].key_value = Some(key_value);
        self.mark_rooted(e, site);
        match key {
            Key::Named(_) => self.m.cons(recv, Cons::Store { key, src, site }),
            // The key's values decide: an index or any key (`StoreIndexed`).
            Key::Any => self.m.cons(
                key_value,
                Cons::StoreIndexed {
                    o: recv,
                    src,
                    site,
                },
            ),
        }
    }

    /// A `this.k = v` store in a function `new` may call, before `this`
    /// leaks: a birth writer of `new f()` instances.
    fn mark_fn_phase(&mut self, fx: &Fx, e: &Expr, object: &Expr, site: SiteId) {
        if !fx.fn_ctor || !matches!(object, Expr::This) {
            return;
        }
        if self
            .fn_phase
            .get(&fx.f)
            .is_some_and(|ph| ph.phase_stores.contains(&(e as *const Expr as usize)))
        {
            self.m.stores[site as usize].ctor_fn = Some(fx.f);
        }
    }

    pub(crate) fn key_of(&mut self, index: &Expr) -> Key {
        match index {
            Expr::String(s) => Key::Named(self.m.atom(s)),
            _ => Key::Any,
        }
    }

    /// The key of a computed READ or method call. `Symbol.iterator` & co.:
    /// HIR folds a well-known symbol to the runtime's `@@__perry_wk_<name>`
    /// sentinel, one symbol per name, so the read names it. Stores keep
    /// `Key::Any`, which every named read sees; as an atom it can only
    /// merge with a string key of the same spelling (more flow, never less).
    pub(crate) fn read_key(&mut self, index: &Expr) -> Key {
        match index {
            Expr::SymbolFor(name) => match name.as_ref() {
                Expr::String(s) if s.starts_with(crate::builtins::WK_PREFIX) => {
                    Key::Named(self.m.atom(s))
                }
                _ => Key::Any,
            },
            _ => self.key_of(index),
        }
    }

    pub(crate) fn load(&mut self, recv: NodeId, key: Key) -> NodeId {
        let dst = self.tmp("load");
        self.m.cons(recv, Cons::Load { key, dst });
        dst
    }

    pub(crate) fn mcall(
        &mut self,
        recv: NodeId,
        key: Key,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
    ) -> NodeId {
        let (call, ret) = self.new_call(ThisArg::Node(recv), args, spread);
        self.m.cons(recv, Cons::MCall { key, call });
        ret
    }

    pub(crate) fn call_value(
        &mut self,
        callee: NodeId,
        this: ThisArg,
        args: Vec<NodeId>,
        spread: Vec<NodeId>,
    ) -> NodeId {
        let (call, ret) = self.new_call(this, args, spread);
        self.m.cons(callee, Cons::Call { call });
        ret
    }

    fn new_lit(&mut self, mi: u32, e: &Expr, keys: &[&str]) -> (TmplId, Tok) {
        // One template per literal expression.
        let at = e as *const Expr as usize;
        if let Some(&tt) = self.lit_births.get(&at) {
            return tt;
        }
        let tt = self.new_lit_tmpl(mi, keys);
        self.lit_births.insert(at, tt);
        tt
    }

    pub(crate) fn new_lit_tmpl(&mut self, mi: u32, keys: &[&str]) -> (TmplId, Tok) {
        let site = self.next_site(mi);
        let tm = self.m.tmpls.len() as TmplId;
        self.note_birth_fn(tm);
        let mut atoms = Vec::new();
        for k in keys {
            let a = self.m.atom(k);
            crate::gen::push_unique(&mut atoms, a);
        }
        self.m.tmpls.push(Template {
            kind: TmplKind::Lit {
                module: mi,
                site,
                enum_image: false,
            },
            // Per unit the site ordinal counts this unit's literals only, so
            // the unit names the literal (DECISIONS 94: a published plain
            // literal is a layout contract, its display the key).
            display: format!(
                "{}{}:{{lit#{site}}}",
                self.mods[mi as usize].path_short(),
                self.unit_suffix()
            ),
            birth_keys: atoms.clone(),
            definite: atoms,
            declared: Vec::new(),
            reserved: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: vec![],
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
            species: None,
        });
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        (tm, t)
    }

    /// `await v` / promise adoption: the value itself (non-thenables), the
    /// settled value of promise objects, and for thenables whatever their
    /// `then` passes to the resolve function.
    pub(crate) fn awaited(&mut self, v: NodeId) -> NodeId {
        let out = self.tmp("awaited");
        self.m.edge(v, out);
        let pa = self.m.atom("#value");
        self.m.cons(
            v,
            Cons::Load {
                key: Key::Named(pa),
                dst: out,
            },
        );
        // Thenables: `then(resolve, reject)` with runtime resolvers.
        let resolver = self.resolver_fn(out);
        let rej = self.resolver_fn(self.thrown);
        let then = self.m.atom("then");
        let (call, ret) = self.new_call(ThisArg::Node(v), vec![resolver, rej], vec![]);
        self.m.cons(
            v,
            Cons::MCall {
                key: Key::Named(then),
                call,
            },
        );
        let _ = ret;
        out
    }

    /// A synthetic function whose first parameter flows into `dst` (a
    /// promise resolver handed to user code).
    pub(crate) fn resolver_fn(&mut self, dst: NodeId) -> NodeId {
        let t = crate::builtins::resolver(&mut self.m, dst);
        self.tok_node(t)
    }

    /// A fresh promise template whose settled value is `value`.
    pub(crate) fn promise_of(&mut self, mi: u32, value: NodeId) -> NodeId {
        let tm = self.new_bobj_template(mi, "Promise");
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let a = self.m.atom("#value");
        let f = self.m.field(t, a);
        let aw = self.awaited(value);
        self.m.edge(aw, f);
        self.tok_node(t)
    }

    /// The values produced by iterating whatever `v` holds: elements of
    /// arrays/collections/primitive boxes, and for iterables the protocol:
    /// `v[Symbol.iterator]()`, then `.next()` on the result and on `v`
    /// itself, then `.value`.
    pub(crate) fn iterate(&mut self, v: NodeId) -> NodeId {
        let out = self.tmp("iterated");
        let ik = self.m.iter_key();
        self.m.cons(v, Cons::Load { key: ik, dst: out });
        let (pc, it) = self.new_call(ThisArg::Node(v), vec![], vec![]);
        let wk = Key::Named(self.m.atom(crate::builtins::WK_ITERATOR));
        self.m.protocol_calls.insert(pc);
        self.m.rel(|| format!("protocolcall {pc}"));
        self.m.cons(v, Cons::MCall { key: wk, call: pc });
        let na = self.m.atom("next");
        let r1 = self.mcall(it, Key::Named(na), vec![], vec![]);
        let r2 = self.mcall(v, Key::Named(na), vec![], vec![]);
        let va = self.m.atom("value");
        self.m.cons(
            r1,
            Cons::Load {
                key: Key::Named(va),
                dst: out,
            },
        );
        self.m.cons(
            r2,
            Cons::Load {
                key: Key::Named(va),
                dst: out,
            },
        );
        out
    }

    /// Every node an expression makes belongs to the function walking it.
    pub(crate) fn expr(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        let n = self.expr_inner(fx, e);
        if self.diagnostic.is_some() {
            self.diagnostic_expr(fx, e, n);
        }
        self.m.cur_fn = prev;
        n
    }

    fn expr_inner(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;
        match e {
            Expr::Undefined => self.prim_node(prim::UNDEF),
            Expr::Null => self.prim_node(prim::NULL),
            Expr::Bool(_) => self.prim_node(prim::BOOL),
            Expr::Number(_) | Expr::Integer(_) => self.prim_node(prim::NUM),
            Expr::BigInt(_) => self.prim_node(prim::BIGINT),
            Expr::String(_) | Expr::WtfString(_) => self.prim_node(prim::STR),
            Expr::I18nString { params, .. } => {
                let ops: Vec<&Expr> = params.iter().map(|(_, e)| e.as_ref()).collect();
                self.prim_op(fx, &ops, prim::STR, "i18n param", Protocol::ToPrimitive)
            }
            Expr::LocalGet(id) => self.local(mi, *id),
            Expr::LocalSet(id, v) => {
                let vn = self.expr(fx, v);
                let l = self.local_write(mi, *id);
                match fx.guard {
                    Some((t, test)) => self.m.cons(
                        t,
                        Cons::Guard {
                            test,
                            from: vn,
                            dst: l,
                        },
                    ),
                    None => self.m.edge(vn, l),
                }
                vn
            }
            Expr::ScopedTemp { id, value, body } => {
                let vn = self.expr(fx, value);
                let l = self.local(mi, *id);
                self.m.edge(vn, l);
                self.expr(fx, body)
            }
            Expr::GlobalGet(id) => {
                let has_real = self.modules[mi as usize]
                    .1
                    .globals
                    .iter()
                    .any(|g| g.id == *id);
                if *id == 0 && !has_real {
                    let t = self.m.tok(TokKind::Builtin(BuiltinNs::Other));
                    self.tok_node(t)
                } else {
                    self.global(mi, *id)
                }
            }
            Expr::GlobalSet(id, v) => {
                let vn = self.expr(fx, v);
                let g = self.global(mi, *id);
                self.m.edge(vn, g);
                vn
            }
            Expr::WithGet {
                object,
                property,
                fallback,
            } => {
                // E15: `with (o)` resolves names dynamically on `o`.
                let o = self.expr(fx, object);
                let r = self.m.reason("E15 with-statement object");
                self.m.cons(
                    o,
                    Cons::Hard {
                        kind: Hard::E15With,
                        reason: r,
                    },
                );
                let a = self.m.atom(property);
                let out = self.load(o, Key::Named(a));
                let fb = self.expr(fx, fallback);
                self.m.edge(fb, out);
                out
            }
            Expr::WithSet {
                object,
                property,
                value,
                fallback,
                ..
            } => {
                let o = self.expr(fx, object);
                let r = self.m.reason("E15 with-statement object");
                self.m.cons(
                    o,
                    Cons::Hard {
                        kind: Hard::E15With,
                        reason: r,
                    },
                );
                let v = self.expr(fx, value);
                let a = self.m.atom(property);
                self.store(
                    fx,
                    o,
                    Key::Named(a),
                    v,
                    format!("with.{property}"),
                    None,
                    false,
                );
                match fallback {
                    perry_hir::WithSetFallback::Local(id)
                    | perry_hir::WithSetFallback::SloppyImplicit(id) => {
                        let l = self.local_write(mi, *id);
                        self.m.edge(v, l);
                    }
                    _ => {}
                }
                v
            }
            Expr::Update { id, .. } => {
                let l = self.local(mi, *id);
                let r = self.m.reason("ToNumeric");
                self.m.cons(
                    l,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let out = self.tmp("update");
                self.m.cons(
                    l,
                    Cons::Arith {
                        dst: out,
                        add: false,
                    },
                );
                let w = self.local_write(mi, *id);
                self.m.edge(out, w);
                out
            }
            Expr::Binary { op, left, right } => {
                let add = matches!(op, perry_hir::BinaryOp::Add);
                let shift = matches!(op, perry_hir::BinaryOp::UShr);
                let out = self.tmp("binary");
                let r = self.m.reason("ToPrimitive operand");
                for o in [left, right] {
                    let v = self.expr(fx, o);
                    self.m.cons(
                        v,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::ToPrimitive,
                        },
                    );
                    self.m.cons(v, Cons::Arith { dst: out, add });
                }
                if shift {
                    // `>>>` is never a BigInt result (TypeError instead).
                    self.m.add_prim(out, prim::NUM);
                }
                out
            }
            Expr::Unary { op, operand } => match op {
                UnaryOp::Not => {
                    let _ = self.expr(fx, operand);
                    self.prim_node(prim::BOOL)
                }
                UnaryOp::Pos => {
                    self.prim_op(fx, &[operand], prim::NUM, "ToNumber", Protocol::ToPrimitive)
                }
                _ => {
                    let v = self.expr(fx, operand);
                    let r = self.m.reason("ToNumeric");
                    self.m.cons(
                        v,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::ToPrimitive,
                        },
                    );
                    let out = self.tmp("unary");
                    self.m.cons(
                        v,
                        Cons::Arith {
                            dst: out,
                            add: false,
                        },
                    );
                    out
                }
            },
            Expr::Compare { op, left, right } => match op {
                CompareOp::Eq | CompareOp::Ne => {
                    let _ = self.expr(fx, left);
                    let _ = self.expr(fx, right);
                    self.prim_node(prim::BOOL)
                }
                _ => self.prim_op(
                    fx,
                    &[left, right],
                    prim::BOOL,
                    "ToPrimitive compare",
                    Protocol::ToPrimitive,
                ),
            },
            Expr::Logical { op, left, right } => {
                let l = self.expr(fx, left);
                let r = self.expr(fx, right);
                let out = self.tmp("logical");
                self.m.edge(l, out);
                // The right operand is the result only when the left one
                // selects it.
                let test = match op {
                    LogicalOp::And => crate::model::guard::TRUTHY,
                    LogicalOp::Or => crate::model::guard::FALSY,
                    LogicalOp::Coalesce => crate::model::guard::NULLISH,
                };
                self.m.cons(
                    l,
                    Cons::Guard {
                        test,
                        from: r,
                        dst: out,
                    },
                );
                out
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let (t, then_test, else_test) = self.guard_test(fx, condition);
                let a = self.expr(fx, then_expr);
                let b = self.expr(fx, else_expr);
                let out = self.tmp("cond");
                self.m.cons(
                    t,
                    Cons::Guard {
                        test: then_test,
                        from: a,
                        dst: out,
                    },
                );
                self.m.cons(
                    t,
                    Cons::Guard {
                        test: else_test,
                        from: b,
                        dst: out,
                    },
                );
                out
            }
            Expr::TypeOf(x) => {
                let _ = self.expr(fx, x);
                self.prim_node(prim::STR)
            }
            Expr::Void(x) => {
                let _ = self.expr(fx, x);
                self.prim_node(prim::UNDEF)
            }
            Expr::Sequence(es) => {
                let mut last = self.prim_node(prim::UNDEF);
                for x in es {
                    last = self.expr(fx, x);
                }
                last
            }
            Expr::InstanceOf { expr, ty_expr, .. } => {
                let _ = self.expr(fx, expr);
                if let Some(t) = ty_expr {
                    let tv = self.expr(fx, t);
                    // Symbol.hasInstance is a protocol method.
                    let r = self.m.reason("Symbol.hasInstance");
                    self.m.cons(
                        tv,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::HasInstance,
                        },
                    );
                }
                self.prim_node(prim::BOOL)
            }
            Expr::In { property, object } => {
                let p = self.expr(fx, property);
                let r = self.m.reason("ToPropertyKey");
                self.m.cons(
                    p,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let _ = self.expr(fx, object);
                self.prim_node(prim::BOOL)
            }
            Expr::Await(x) => {
                let v = self.expr(fx, x);
                self.awaited(v)
            }
            Expr::Yield { value, .. } => {
                if let Some(v) = value {
                    let vn = self.expr(fx, v);
                    self.to_sink(vn, "untransformed yield");
                }
                self.m.top_val("untransformed yield result")
            }

            // ------------------------------------------------ properties
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                ..
            } => {
                let o = match self.ns_member(mi, object, property, false) {
                    Some(o) => o,
                    None => self.expr(fx, object),
                };
                let a = self.member_atom(object, property);
                if *byte_offset != 0 {
                    self.read_sites.push(ReadSite {
                        module: mi,
                        byte_offset: *byte_offset,
                        key: a,
                        recv: o,
                        is_call: false,
                        value_call: false,
                        func: fx.f,
                    });
                }
                self.load(o, Key::Named(a))
            }
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } => {
                let o = self.expr(fx, object);
                let v = self.expr(fx, value);
                let a = self.member_atom(object, property);
                let owner = self.phase_owner(fx, e, object);
                self.store_phased(
                    fx,
                    e,
                    object,
                    o,
                    Key::Named(a),
                    v,
                    format!(".{property}="),
                    owner,
                    None,
                );
                v
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                let t = self.expr(fx, target);
                let k = self.expr(fx, key);
                let r = self.m.reason("ToPropertyKey");
                self.m.cons(
                    k,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let v = self.expr(fx, value);
                let rv = self.expr(fx, receiver);
                let owner = self.phase_owner(fx, e, target);
                if let Some(kp) = self.for_in_key_of(fx.module, key) {
                    // A for-in run's key decides the stored key (FVA 4.3b).
                    for (o, what) in [(t, "putvalue"), (rv, "putvalue-receiver")] {
                        let site = self.store_site(fx.module, fx.f, what.into(), owner, false);
                        self.mark_fn_phase(fx, e, target, site);
                        self.m.cons(kp, Cons::StoreKeyed { o, src: v, site });
                    }
                } else if let Some(ks) = self.const_key_set(fx.module, key) {
                    // One of a few constant keys: a named store under each.
                    for s in ks {
                        let key = Key::Named(self.m.atom(&s));
                        for (o, what) in [(t, "putvalue"), (rv, "putvalue-receiver")] {
                            self.store_phased(fx, e, target, o, key, v, what.into(), owner, Some(k));
                        }
                    }
                } else {
                    let key = self.key_of(key);
                    // A receiver other than the target gets the own data store.
                    for (o, what) in [(t, "putvalue"), (rv, "putvalue-receiver")] {
                        let site = self.store_site(fx.module, fx.f, what.into(), owner, false);
                        self.mark_fn_phase(fx, e, target, site);
                        self.mark_rooted(e, site);
                        self.m.stores[site as usize].key_value = Some(k);
                        match key {
                            Key::Named(_) => self.m.cons(o, Cons::Store { key, src: v, site }),
                            // The key's values decide: an index or any key.
                            Key::Any => self.m.cons(k, Cons::StoreIndexed { o, src: v, site }),
                        }
                    }
                }
                v
            }
            Expr::PropertyUpdate {
                object, property, ..
            } => {
                let o = self.expr(fx, object);
                let a = self.member_atom(object, property);
                let cur = self.load(o, Key::Named(a));
                let r = self.m.reason("ToNumeric");
                self.m.cons(
                    cur,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let nv2 = self.tmp("update-val");
                self.m.cons(
                    cur,
                    Cons::Arith {
                        dst: nv2,
                        add: false,
                    },
                );
                let owner = self.phase_owner(fx, e, object);
                self.store_phased(
                    fx,
                    e,
                    object,
                    o,
                    Key::Named(a),
                    nv2,
                    format!(".{property}++"),
                    owner,
                    None,
                );
                nv2
            }
            Expr::BufferLength(array) | Expr::Uint8ArrayLength(array) => {
                let o = self.expr(fx, array);
                let key = self.m.atom("length");
                self.load(o, Key::Named(key))
            }
            Expr::IndexGet { object, index, .. }
            | Expr::Uint8ArrayGet {
                array: object,
                index,
            }
            | Expr::BufferIndexGet {
                buffer: object,
                index,
            } => {
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
                    Key::Named(a) => self.load(o, Key::Named(a)),
                    Key::Any => {
                        let dst = self.tmp("idxload");
                        // A for-in run's key is its key parameter (FVA 4.3b).
                        let idx = self.for_in_key_of(fx.module, index).unwrap_or(i);
                        self.m.cons(o, Cons::IdxLoad { idx, dst });
                        dst
                    }
                }
            }
            Expr::IndexSet {
                object,
                index,
                value,
                ..
            }
            | Expr::Uint8ArraySet {
                array: object,
                index,
                value,
            }
            | Expr::BufferIndexSet {
                buffer: object,
                index,
                value,
            } => {
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
                let v = self.expr(fx, value);
                if let Some(kp) = self.for_in_key_of(fx.module, index) {
                    // A for-in run's key decides the stored key (FVA 4.3b);
                    // rooted as any element store (DECISIONS 149b).
                    let site = self.store_site(fx.module, fx.f, "[]=".into(), None, false);
                    self.mark_rooted(e, site);
                    self.m.cons(kp, Cons::StoreKeyed { o, src: v, site });
                } else if let Some(ks) = self.const_key_set(fx.module, index) {
                    // One of a few constant keys: a named store under each.
                    for s in ks {
                        let key = Key::Named(self.m.atom(&s));
                        self.store_elem(fx, e, o, key, i, v, "[]=");
                    }
                } else {
                    let key = self.key_of(index);
                    self.store_elem(fx, e, o, key, i, v, "[]=");
                }
                v
            }
            Expr::IndexUpdate { object, index, .. } => {
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
                let key = self.key_of(index);
                let cur = self.load(o, key);
                let r2 = self.m.reason("ToNumeric");
                self.m.cons(
                    cur,
                    Cons::Implicit {
                        reason: r2,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let nv = self.tmp("update-val");
                self.m.cons(
                    cur,
                    Cons::Arith {
                        dst: nv,
                        add: false,
                    },
                );
                self.store_elem(fx, e, o, key, i, nv, "[]++");
                nv
            }
            Expr::Delete(target) => {
                // E7: the receiver of the deleted reference (explain: in
                // which function).
                let r = if self.m.explain {
                    let f = self
                        .m
                        .funcs
                        .get(fx.f as usize)
                        .map_or("(no function)", |f| f.name.as_str());
                    let why = format!("E7 delete in {f}");
                    self.m.reason(&why)
                } else {
                    self.m.reason("E7 delete")
                };
                match target.as_ref() {
                    Expr::PropertyGet {
                        object, property, ..
                    } => {
                        let o = self.expr(fx, object);
                        let key = self.m.atom(property);
                        self.m.cons(o, Cons::DeleteKey { key, reason: r });
                    }
                    Expr::IndexGet { object, index, .. } => {
                        let o = self.expr(fx, object);
                        let _ = self.expr(fx, index);
                        self.m.cons(
                            o,
                            Cons::Hard {
                                kind: Hard::E7Delete,
                                reason: r,
                            },
                        );
                    }
                    other => {
                        let _ = self.expr(fx, other);
                    }
                }
                self.prim_node(prim::BOOL)
            }

            // --------------------------------------------- literals
            Expr::Object(props) => {
                let keys: Vec<&str> = props
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .filter(|k| *k != "__proto__")
                    .collect();
                let (tm, t) = self.new_lit(mi, e, &keys);
                let obj = self.tok_node(t);
                for (k, v) in props {
                    let vn = self.expr(fx, v);
                    if k == "__proto__" {
                        // E6 literal form: a proto-carrying literal template.
                        let pn = self.m.proto_node(t);
                        self.m.edge(vn, pn);
                        continue;
                    }
                    let a = self.m.atom(k);
                    self.store(
                        fx,
                        obj,
                        Key::Named(a),
                        vn,
                        format!("lit.{k}"),
                        Some(tm),
                        true,
                    );
                }
                obj
            }
            // A fresh enum object: a literal of its stores' keys, each
            // holding the numbers and strings stored there.
            Expr::EnumImage { stores, .. } => {
                let mut seen = std::collections::HashSet::new();
                let keys: Vec<&str> = stores
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .filter(|k| seen.insert(*k))
                    .collect();
                let (tm, t) = self.new_lit(mi, e, &keys);
                let obj = self.tok_node(t);
                for (k, v) in stores {
                    let vn = self.prim_node(match v {
                        perry_hir::EnumValue::Number(_) | perry_hir::EnumValue::Computed => {
                            prim::NUM
                        }
                        perry_hir::EnumValue::String(_) => prim::STR,
                    });
                    let a = self.m.atom(k);
                    self.store(
                        fx,
                        obj,
                        Key::Named(a),
                        vn,
                        format!("enum.{k}"),
                        Some(tm),
                        true,
                    );
                }
                obj
            }
            Expr::ObjectSpread { parts } => {
                let named: Vec<&str> = parts.iter().filter_map(|(k, _)| k.as_deref()).collect();
                let has_spread = parts.iter().any(|(k, _)| k.is_none());
                let (tm, t) = self.new_lit(mi, e, if has_spread { &[] } else { &named });
                let obj = self.tok_node(t);
                if has_spread {
                    let named = named.iter().map(|k| self.m.atom(k)).collect();
                    self.m.lit_spread.insert(
                        tm,
                        crate::model::LitSpread {
                            named,
                            sites: Vec::new(),
                        },
                    );
                }
                for (k, v) in parts {
                    let vn = self.expr(fx, v);
                    match k {
                        Some(k) => {
                            let a = self.m.atom(k);
                            self.store(
                                fx,
                                obj,
                                Key::Named(a),
                                vn,
                                format!("lit.{k}"),
                                Some(tm),
                                true,
                            );
                        }
                        None => {
                            // Own enumerable keys of the source, key by key
                            // (getters run): `Cons::Spread`.
                            let site = self.store_site(mi, fx.f, "lit...".into(), Some(tm), true);
                            if let Some(l) = self.m.lit_spread.get_mut(&tm) {
                                l.sites.push((site, None));
                            }
                            self.m.lit_spread_site.insert(site, tm);
                            self.m.cons(vn, Cons::Spread { into: obj, site });
                        }
                    }
                }
                obj
            }
            Expr::ObjectAssign { target, sources } => {
                // E4: a writer of every key the sources can carry.
                let t = self.expr(fx, target);
                for s in sources {
                    let sv = self.expr(fx, s);
                    let site = self.store_site(mi, fx.f, "Object.assign".into(), None, false);
                    self.m.cons(sv, Cons::Spread { into: t, site });
                }
                t
            }
            Expr::ObjectRest { object, .. } => {
                let o = self.expr(fx, object);
                let (tm, t) = self.new_lit(mi, e, &[]);
                let obj = self.tok_node(t);
                let site = self.store_site(mi, fx.f, "rest".into(), Some(tm), true);
                self.m.cons(o, Cons::Spread { into: obj, site });
                obj
            }
            Expr::Array(elems, _) => {
                let tm = self.new_arr_template(mi, "lit");
                let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
                let fa = self.m.field_any(t);
                for el in elems {
                    let v = self.expr(fx, el);
                    self.m.edge(v, fa);
                }
                self.tok_node(t)
            }
            Expr::ArraySpread(elems) => {
                let tm = self.new_arr_template(mi, "spread");
                let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
                let fa = self.m.field_any(t);
                for el in elems {
                    match el {
                        ArrayElement::Expr(x) => {
                            let v = self.expr(fx, x);
                            self.m.edge(v, fa);
                        }
                        ArrayElement::Spread(x) => {
                            let v = self.expr(fx, x);
                            let it = self.iterate(v);
                            self.m.edge(it, fa);
                        }
                        ArrayElement::Hole => self.m.add_prim(fa, prim::UNDEF),
                    }
                }
                self.tok_node(t)
            }

            // ------------------------------------------------- calls
            Expr::Call {
                callee,
                args,
                byte_offset,
                ..
            } => {
                self.call_expr(fx, callee, args, &[], *byte_offset)
            }
            Expr::CallSpread { callee, args, .. } => {
                let (a, s) = self.call_args(fx, args);
                let _ = (&a, &s);
                self.call_with(fx, callee, a, s, 0)
            }
            Expr::FuncRef(id) if self.view.is_some_and(|v| v.foreign_funcs.contains_key(id)) => {
                let name = &self.view.expect("view").foreign_funcs[id];
                self.extern_value(mi, name)
            }
            Expr::FuncRef(id) => {
                let f = self.fn_by_id(mi, *id);
                let t = self.m.funcs[f as usize].token;
                self.tok_node(t)
            }
            Expr::ExternFuncRef { name, .. } => self.extern_value(mi, name),
            Expr::Closure { .. } => self.closure(fx, e),

            // -------------------------------------------- classes / new
            Expr::New {
                class_name, args, ..
            } => {
                // A chunked literal birth: its chunk calls receive the object
                // being born, whose chunked slots they initialize.
                let chunked = args.iter().any(|a| matches!(a, Expr::LiteralChunks(_)));
                if chunked {
                    let x = self.tmp("literal chunk object");
                    self.chunk_objects.push(x);
                }
                let a = self.evaluate_all(fx, args);
                let x = if chunked {
                    self.chunk_objects.pop()
                } else {
                    None
                };
                let born = self.new_named(fx, class_name, a, vec![]);
                if let Some(x) = x {
                    self.m.edge(born, x);
                }
                born
            }
            // The chunked slots' values reach the object through the chunk
            // functions' `LiteralSlotInit` stores, not through the argument.
            Expr::LiteralChunks(calls) => {
                for c in calls {
                    self.expr(fx, c);
                }
                self.tmp("literal chunk slot")
            }
            Expr::LiteralChunkObject => match self.chunk_objects.last() {
                Some(&x) => x,
                None => self.m.top_val("literal chunk object outside its birth"),
            },
            // A define of a slot of an unpublished literal birth: a birth
            // initializer, like the literal's own positional stores.
            Expr::LiteralSlotInit {
                object, key, value, ..
            } => {
                let o = self.expr(fx, object);
                let v = self.expr(fx, value);
                let a = self.m.atom(key);
                self.store(
                    fx,
                    o,
                    Key::Named(a),
                    v,
                    format!(".{key} (literal chunk)"),
                    None,
                    true,
                );
                self.prim_node(prim::UNDEF)
            }
            Expr::NewDynamic { callee, args, .. } => {
                let c = self.expr(fx, callee);
                let a = self.evaluate_all(fx, args);
                let (call, ret) = self.new_call(ThisArg::Undef, a, vec![]);
                self.m.cons(c, Cons::New { call });
                ret
            }
            Expr::NewDynamicSpread { callee, args, .. } => {
                let c = self.expr(fx, callee);
                let (a, s) = self.call_args(fx, args);
                let (call, ret) = self.new_call(ThisArg::Undef, a, s);
                self.m.cons(c, Cons::New { call });
                ret
            }
            Expr::NewTarget => self.m.top_val("new.target"),
            Expr::ClassRef(name) => self.class_value(mi, name),
            Expr::EnumMember { .. } => self.prim_node(prim::NUM | prim::STR),
            Expr::StaticFieldGet {
                class_name,
                field_name,
            } => {
                let c = self.class_value(mi, class_name);
                let a = self.static_field_atom(mi, class_name, field_name);
                self.load(c, Key::Named(a))
            }
            Expr::StaticFieldSet {
                class_name,
                field_name,
                value,
            } => {
                let c = self.class_value(mi, class_name);
                let v = self.expr(fx, value);
                let a = self.static_field_atom(mi, class_name, field_name);
                self.store(
                    fx,
                    c,
                    Key::Named(a),
                    v,
                    format!("static.{field_name}"),
                    None,
                    false,
                );
                v
            }
            Expr::ClassStaticSymbolSet {
                class_name,
                key,
                value,
            }
            | Expr::RegisterClassStaticSymbol {
                class_name,
                key_expr: key,
                value_expr: value,
            } => {
                let c = self.class_value(mi, class_name);
                let _ = self.expr(fx, key);
                let v = self.expr(fx, value);
                self.store(fx, c, Key::Any, v, "static[sym]".into(), None, false);
                v
            }
            Expr::RegisterClassParentDynamic {
                class_name,
                parent_expr,
            } => {
                let p = self.expr(fx, parent_expr);
                let tm = self.resolve_class_name(mi, class_name);
                if !self.parent_to_cell(mi, class_name, tm, p) {
                    self.feed_parent(p, tm, "E11 dynamic class parent");
                }
                self.prim_node(prim::UNDEF)
            }
            Expr::RegisterClassCaptures {
                class_name,
                captures,
            } => {
                for (i, c) in captures.iter().enumerate() {
                    let v = self.expr(fx, c);
                    let env = self.class_env(mi, class_name, i);
                    self.m.edge(v, env);
                }
                self.prim_node(prim::UNDEF)
            }
            Expr::RefreshClassExprCaptures {
                class_value,
                captures,
                env_class,
            } => {
                let cv = self.expr(fx, class_value);
                let name = env_class.clone().unwrap_or_default();
                for (i, c) in captures.iter().enumerate() {
                    let v = self.expr(fx, c);
                    let env = self.class_env(mi, &name, i);
                    self.m.edge(v, env);
                    if env_class.is_none() {
                        self.to_sink(v, "class capture of an unnamed class expression");
                    }
                }
                cv
            }
            Expr::ClassEnvGet {
                class_name, index, ..
            } => self.class_env(mi, class_name, *index as usize),
            Expr::ClassEnvSet {
                class_name,
                index,
                value,
                ..
            } => {
                let v = self.expr(fx, value);
                let env = self.class_env(mi, class_name, *index as usize);
                self.m.edge(v, env);
                v
            }
            Expr::ClassEnvStamp {
                instance,
                evaluation,
                ..
            } => {
                let i = self.expr(fx, instance);
                let _ = self.expr(fx, evaluation);
                i
            }
            Expr::ClassEnvCurrent { .. } => self.m.top_val("ClassEnvCurrent"),
            Expr::ClassCaptureValue {
                class_name,
                index,
                fallback,
                ..
            } => {
                let env = self.class_env(mi, class_name, *index as usize);
                let out = self.tmp("capture");
                self.m.edge(env, out);
                if let Some(f) = fallback {
                    let v = self.expr(fx, f);
                    self.m.edge(v, out);
                }
                out
            }
            Expr::RegisterClassComputedMethod {
                class_name,
                key_expr,
                method_name,
                is_static,
                ..
            } => {
                let _ = self.expr(fx, key_expr);
                if let Some(tm) = self.resolve_class_name(mi, class_name) {
                    let holder = if *is_static {
                        self.m.tok(TokKind::ClassObj(tm))
                    } else {
                        self.m.tok(TokKind::Proto(tm))
                    };
                    let a = self.m.atom(method_name);
                    let src = self.m.field(holder, a);
                    let fa = self.m.field_any(holder);
                    self.m.edge(src, fa);
                }
                self.prim_node(prim::UNDEF)
            }
            Expr::RegisterClassComputedAccessor {
                class_name,
                key_expr,
                getter_name,
                setter_name,
                is_static,
                ..
            } => {
                let _ = self.expr(fx, key_expr);
                // Under a constant key it is that key's accessor, installed
                // with the class (`Gen::named_computed`).
                let named = !*is_static && self.const_key(mi, key_expr).is_some();
                if let (false, Some(tm)) = (named, self.resolve_class_name(mi, class_name)) {
                    self.computed_accessor(mi, tm, *is_static, getter_name, setter_name);
                }
                self.prim_node(prim::UNDEF)
            }
            Expr::ClassExprFresh {
                template,
                evaluation_owner,
                named_statics,
                computed_keys,
                computed_statics,
                captured_args,
                shared_first_evaluation,
                evaluated_parent,
                definition_steps,
                ..
            } => {
                let cv = self.class_value(mi, template);
                if let Some(id) = evaluation_owner {
                    // The class's self binding gets the class before its
                    // statics run. A binding the HIR made a cell (codegen's
                    // `store_evaluation_owner`) gets it in element 0, not in
                    // the binding: the binding holds the cell (DECISIONS 50).
                    let l = self.local(mi, *id);
                    if self.cell_locals.contains(&(mi, *id)) {
                        self.store(fx, l, Key::Any, cv, "owner[0]=".into(), None, false);
                    } else {
                        self.m.edge(cv, l);
                    }
                }
                for (name, v) in named_statics {
                    let vn = self.expr(fx, v);
                    let a = self.m.atom(name);
                    self.store(
                        fx,
                        cv,
                        Key::Named(a),
                        vn,
                        format!("static.{name}"),
                        None,
                        false,
                    );
                }
                for (_, v) in computed_keys {
                    let _ = self.expr(fx, v);
                }
                for (_, v) in computed_statics {
                    let vn = self.expr(fx, v);
                    self.store(fx, cv, Key::Any, vn, "static[k]".into(), None, false);
                }
                for (i, a) in captured_args.iter().enumerate() {
                    let v = self.expr(fx, a);
                    let env = self.class_env(mi, template, i);
                    self.m.edge(v, env);
                }
                if let Some(s) = shared_first_evaluation {
                    let _ = self.expr(fx, s);
                }
                if let Some(p) = evaluated_parent {
                    let v = self.expr(fx, p);
                    let tm = self.resolve_class_name(mi, template);
                    if !self.parent_to_cell(mi, template, tm, v) {
                        self.feed_parent(v, tm, "E11 class expression with an evaluated parent");
                    }
                }
                for d in definition_steps {
                    let _ = self.expr(fx, d);
                }
                cv
            }
            Expr::ClassIsFirstEvaluation { value, .. } => {
                let _ = self.expr(fx, value);
                self.prim_node(prim::BOOL)
            }
            Expr::SetFunctionPrototype { func, proto, .. } => {
                let f = self.expr(fx, func);
                let p = self.expr(fx, proto);
                let a = self.m.atom("prototype");
                self.store(fx, f, Key::Named(a), p, "fn.prototype=".into(), None, false);
                f
            }
            Expr::RegisterPrototypeMethod {
                class_name,
                method_name,
                value,
            } => {
                let v = self.expr(fx, value);
                match self.resolve_class_name(mi, class_name) {
                    Some(tm) => {
                        let p = self.m.tok(TokKind::Proto(tm));
                        let a = self.m.atom(method_name);
                        let f = self.m.field(p, a);
                        self.m.edge(v, f);
                    }
                    None => self.to_sink(v, "method registered on an unresolved class"),
                }
                v
            }
            Expr::RegisterFunctionPrototypeMethod {
                func,
                method_name,
                value,
            } => {
                let f = self.expr(fx, func);
                let v = self.expr(fx, value);
                let pa = self.m.atom("prototype");
                let proto = self.load(f, Key::Named(pa));
                let a = self.m.atom(method_name);
                self.store(
                    fx,
                    proto,
                    Key::Named(a),
                    v,
                    "fn.prototype.m=".into(),
                    None,
                    false,
                );
                v
            }
            Expr::GetFunctionPrototypeMethod { func, method_name } => {
                let f = self.expr(fx, func);
                let pa = self.m.atom("prototype");
                let proto = self.load(f, Key::Named(pa));
                let a = self.m.atom(method_name);
                self.load(proto, Key::Named(a))
            }
            Expr::StaticMethodCall {
                class_name,
                method_name,
                args,
            } => {
                let c = self.class_value(mi, class_name);
                let a = self.evaluate_all(fx, args);
                let k = self.m.atom(method_name);
                self.mcall(c, Key::Named(k), a, vec![])
            }
            Expr::This => fx.this,
            Expr::ModuleTopThis => fx.this,
            Expr::SuperCall(args) => {
                let a = self.evaluate_all(fx, args);
                self.super_ctor(fx, a, vec![])
            }
            Expr::SuperCallSpread(args) => {
                let (a, s) = self.call_args(fx, args);
                self.super_ctor(fx, a, s)
            }
            Expr::SuperMethodCall { method, args } => {
                let a = self.evaluate_all(fx, args);
                self.super_method(fx, method, a, vec![])
            }
            Expr::SuperMethodCallSpread { method, args } => {
                let (a, s) = self.call_args(fx, args);
                self.super_method(fx, method, a, s)
            }
            Expr::SuperPropertyGet { property } => {
                let dst = self.tmp("super.get");
                let a = self.m.atom(property);
                for start in self.super_starts(fx) {
                    self.m.cons(
                        fx.this,
                        Cons::Super {
                            start,
                            key: Key::Named(a),
                            act: ChainAct::Load(dst),
                        },
                    );
                }
                dst
            }
            Expr::SuperPropertySet { key, value, .. } => {
                let _ = self.expr(fx, key);
                let v = self.expr(fx, value);
                let k = self.key_of(key);
                self.store(fx, fx.this, k, v, "super.x=".into(), None, false);
                for start in self.super_starts(fx) {
                    self.m.cons(
                        fx.this,
                        Cons::Super {
                            start,
                            key: k,
                            act: ChainAct::Store(v),
                        },
                    );
                }
                v
            }

            // ----------------------------------------------- the rest
            _ => self.expr_rest(fx, e),
        }
    }
}
