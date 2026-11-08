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
            region: self.region(),
        });
        (id, ret)
    }

    /// A call of the module member `member` (`node:fs.existsSync`) the HIR
    /// names directly (`FsExistsSync`): when the runtime declares the
    /// member's effects at its body, a call of its function value
    /// (`Native::Member`) with `args`, whose declaration the solver runs;
    /// `None` (nothing evaluated): undeclared.
    pub(crate) fn declared_member_call(
        &mut self,
        fx: &Fx,
        member: &str,
        args: &[&Expr],
    ) -> Option<NodeId> {
        let e = crate::builtins::declared_member(member)?;
        let args = args.iter().map(|a| self.expr(fx, a)).collect();
        let t = self
            .m
            .tok(TokKind::BuiltinFn(Native::Member { identity: e.name }));
        let callee = self.tok_node(t);
        let (call, ret) = self.new_call(ThisArg::Undef, args, vec![]);
        self.m.cons(callee, Cons::Call { call });
        Some(ret)
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
                    // The argument list of a spread (CreateListFromIterable,
                    // ECMA-262 13.3.8.1): what iterating `v` yields, as the
                    // elements of a list. A call site's spread entry is read
                    // at its elements, whatever made it.
                    let v = self.expr(fx, e);
                    let tm = self.new_arr_birth(fx.module, "spread-args", true);
                    let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
                    let fa = self.m.field_any(t);
                    let it = self.iterate(v);
                    self.m.edge(it, fa);
                    s.push(self.tok_node(t));
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
            Key::Any(shape) => self.m.cons(
                key_value,
                Cons::StoreIndexed {
                    o: recv,
                    src,
                    site,
                    shape,
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

    pub(crate) fn element_site(
        &mut self,
        fx: &Fx,
        expr: &Expr,
        recv: NodeId,
        idx: NodeId,
        key: Key,
        op: crate::derive::ElementOp,
    ) {
        let byte_offset = match expr {
            Expr::IndexGet { element_offset, .. }
            | Expr::IndexSet { element_offset, .. }
            | Expr::IndexUpdate { element_offset, .. }
            | Expr::PutValueSet { element_offset, .. } => *element_offset,
            _ => 0,
        };
        let site = crate::gen::ElementSite {
            module: fx.module,
            byte_offset,
            op,
            recv,
            idx,
            key,
        };
        #[cfg(test)]
        self.m.packet_observe(
            site.byte_offset,
            format!("element {:?}", site.op),
            self.m.key_name(site.key),
            site.recv,
            Some(site.idx),
        );
        self.element_sites.push(site);
    }

    /// The key a computed access names: a string literal, or a binding that
    /// always holds one (`const k = "name"; o[k]`: `Gen::const_key`, the
    /// resolver constant stores and definitions use). A binding read before
    /// its initialization throws (TDZ) and names no key at all.
    /// The key of `super[key]` (a read, store or call): the key expression
    /// runs, then ToPropertyKey, whose `@@toPrimitive`, `toString` or
    /// `valueOf` of an object key is program code.
    pub(crate) fn super_key(&mut self, fx: &Fx, key: &Expr) -> Key {
        let kn = self.expr(fx, key);
        self.to_property_key(kn);
        self.key_of(fx.module, key)
    }

    pub(crate) fn key_of(&mut self, mi: u32, index: &Expr) -> Key {
        match index {
            Expr::String(s) => Key::Named(self.m.atom(s)),
            _ => match self.const_key(mi, index) {
                Some(s) => Key::Named(self.m.atom(&s)),
                None => Key::any(),
            },
        }
    }

    /// The key of a computed READ or method call. `Symbol.iterator` & co.:
    /// HIR folds a well-known symbol to the runtime's `@@__perry_wk_<name>`
    /// sentinel, one symbol per name, so the read names it. Stores keep
    /// `Key::Any`, which every named read sees; as an atom it can only
    /// merge with a string key of the same spelling (more flow, never less).
    pub(crate) fn read_key(&mut self, mi: u32, index: &Expr) -> Key {
        match index {
            Expr::SymbolFor(name) => match name.as_ref() {
                Expr::String(s) if s.starts_with(crate::builtins::WK_PREFIX) => {
                    Key::Named(self.m.atom(s))
                }
                _ => Key::any(),
            },
            _ => self.key_of(mi, index),
        }
    }

    /// `E.prototype.K` (`object` is `E.prototype`, `property` is `K`):
    /// `E`'s node, the `E.prototype` read (its read site as any read's) and
    /// `K`'s atom. `None`: another shape.
    fn proto_member(
        &mut self,
        fx: &Fx,
        object: &Expr,
        property: &str,
    ) -> Option<(NodeId, NodeId, Atom)> {
        let Expr::PropertyGet {
            object: inner,
            property: p,
            byte_offset,
            ..
        } = object
        else {
            return None;
        };
        if p != "prototype" || matches!(inner.as_ref(), Expr::PrivateGuard { .. }) {
            return None;
        }
        let mi = fx.module;
        let e = match self.ns_member(mi, inner, p, false) {
            Some(e) => e,
            None => self.expr(fx, inner),
        };
        let pa = self.member_atom(inner, p);
        if *byte_offset != 0 {
            self.read_sites.push(ReadSite {
                module: mi,
                byte_offset: *byte_offset,
                key: pa,
                recv: e,
                is_call: false,
                value_call: false,
                func: fx.f,
            });
        }
        let o = self.load(e, Key::Named(pa));
        if self.diagnostic.is_some() {
            self.diagnostic_expr(fx, object, o);
        }
        let a = self.member_atom(object, property);
        Some((e, o, a))
    }

    pub(crate) fn load(&mut self, recv: NodeId, key: Key) -> NodeId {
        let dst = self.tmp("load");
        self.m.cons(recv, Cons::Load { key, dst });
        dst
    }

    /// The values `Object.values` / `Object.entries` read from `recv`:
    /// EnumerableOwnProperties takes the keys of `[[OwnPropertyKeys]]` and
    /// `[[Get]]`s each one, so an inherited member (Object.prototype's
    /// `constructor` and methods) is never among them (`Cons::OwnValues`).
    pub(crate) fn own_values(&mut self, recv: NodeId) -> NodeId {
        let dst = self.tmp("ownvalues");
        self.m.cons(recv, Cons::OwnValues { dst });
        dst
    }

    /// The static blocks of class expression `template` that one
    /// evaluation runs (`ClassFreshStaticInit::Block`, an index into the
    /// template's `__perry_static_init_N` functions in source order): a
    /// call of each with `this` the evaluated class object `cv`.
    fn fresh_static_blocks(
        &mut self,
        fx: &Fx,
        template: &str,
        order: &[perry_hir::ClassFreshStaticInit],
        cv: NodeId,
    ) {
        let mi = fx.module;
        let module = self.modules[mi as usize].1;
        let Some(c) = module.classes.iter().find(|c| c.name == template) else {
            return;
        };
        let blocks: Vec<u32> = c
            .static_methods
            .iter()
            .filter(|m| m.name.starts_with("__perry_static_init_"))
            .map(|m| m.id)
            .collect();
        for step in order {
            let perry_hir::ClassFreshStaticInit::Block(i) = step else {
                continue;
            };
            let Some(&f) = blocks
                .get(*i as usize)
                .and_then(|id| self.mods[mi as usize].funcs.get(id))
            else {
                continue;
            };
            let t = self.m.funcs[f as usize].token;
            let callee = self.tok_node(t);
            let _ = self.call_value(callee, ThisArg::Node(cv), vec![], vec![]);
        }
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
            chain_miss: false,
            chain_miss_open: false,
            species: None,
            per_evaluation: false,
            static_methods: Vec::new(),
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
        let it = self.iterator_of(v);
        let na = self.m.atom("next");
        let r1 = self.mcall(it, Key::Named(na), vec![], vec![]);
        // The step of `v` itself: the runtime's (`js_get_iterator` returns a
        // value it finds no iterator for unchanged), a protocol call as in
        // `builtins::iterate`.
        let (sc, r2) = self.new_call(ThisArg::Node(v), vec![], vec![]);
        self.m.protocol_calls.insert(sc);
        self.m.cons(
            v,
            Cons::MCall {
                key: Key::Named(na),
                call: sc,
            },
        );
        // Only results seen not done: `Model::yield_key`.
        let va = self.m.yield_key();
        self.m.cons(r1, Cons::Load { key: va, dst: out });
        self.m.cons(r2, Cons::Load { key: va, dst: out });
        out
    }

    /// What `v[Symbol.iterator]()` returns (GetIterator, ECMA-262 7.4.3):
    /// a protocol call.
    pub(crate) fn iterator_of(&mut self, v: NodeId) -> NodeId {
        let (pc, it) = self.new_call(ThisArg::Node(v), vec![], vec![]);
        let wk = Key::Named(self.m.atom(crate::builtins::WK_ITERATOR));
        self.m.protocol_calls.insert(pc);
        self.m.rel(|| format!("protocolcall {pc}"));
        self.m.cons(v, Cons::MCall { key: wk, call: pc });
        it
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
            Expr::Number(v) => self.num_literal(*v),
            Expr::Integer(v) => self.num_literal(*v as f64),
            Expr::BigInt(_) => self.prim_node(prim::BIGINT),
            Expr::String(s) => self.str_literal(s),
            Expr::WtfString(_) => self.prim_node(prim::STR),
            Expr::I18nString { params, .. } => {
                let ops: Vec<&Expr> = params.iter().map(|(_, e)| e.as_ref()).collect();
                self.prim_op(fx, &ops, prim::STR, "i18n param", Protocol::ToPrimitive)
            }
            Expr::LocalGet(id) => match self.for_binding {
                Some((m, l, n)) if m == mi && l == *id => n,
                _ => match self.narrowed_read(mi, *id) {
                    Some(n) => n,
                    None => self.local(mi, *id),
                },
            },
            // Program code never writes a `const` (lowering throws
            // `Assignment to constant variable`): `undefined` written into
            // one that was initialized is the compiler's release of a
            // private binding after its last read (`IteratorRecord::release`),
            // which no read sees.
            Expr::LocalSet(id, v)
                if matches!(v.as_ref(), Expr::Undefined)
                    && self.initialized_consts.contains(&(mi, *id)) =>
            {
                self.prim_node(prim::UNDEF)
            }
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
                self.narrowed_write(mi, *id, vn);
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
            Expr::Update { id, op, .. } => {
                let l = match self.narrowed_read(mi, *id) {
                    Some(n) => n,
                    None => self.local(mi, *id),
                };
                let r = self.m.reason("ToNumeric");
                self.m.cons(
                    l,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let out = self.tmp("update");
                let step = match op {
                    perry_hir::UpdateOp::Increment => 1,
                    perry_hir::UpdateOp::Decrement => -1,
                };
                self.range_rule(crate::model::RangeOp::Step(step), l, None, out);
                self.m.cons(l, Cons::Arith { dst: out });
                let w = self.local_write(mi, *id);
                self.m.edge(out, w);
                self.narrowed_write(mi, *id, out);
                out
            }
            Expr::Binary { op, left, right } => {
                let shift = matches!(op, perry_hir::BinaryOp::UShr);
                let out = self.tmp("binary");
                let r = self.m.reason("ToPrimitive operand");
                let l = self.expr(fx, left);
                let rv = self.expr(fx, right);
                {
                    use crate::model::RangeOp as R;
                    use perry_hir::BinaryOp as B;
                    let rop = match op {
                        B::Add => Some(R::Add),
                        B::Sub => Some(R::Sub),
                        B::Mul => Some(R::Mul),
                        B::Mod => Some(R::Mod),
                        B::BitAnd => Some(R::BitAnd),
                        B::BitOr => Some(R::BitOr),
                        B::BitXor => Some(R::BitXor),
                        B::Shl => Some(R::Shl),
                        B::Shr => Some(R::Shr),
                        B::UShr => Some(R::UShr),
                        B::Div | B::Pow => None,
                    };
                    // Every Number `out` holds is the operator's: without a
                    // rule (`/`, `**`) it is ⊤ by the fail-closed default.
                    if let Some(rop) = rop {
                        self.range_rule(rop, l, Some(rv), out);
                    }
                }
                for v in [l, rv] {
                    self.m.cons(
                        v,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::ToPrimitive,
                        },
                    );
                }
                if matches!(op, perry_hir::BinaryOp::Add) {
                    // ECMA-262 13.15.3: after ToPrimitive a String operand
                    // concatenates, so a String operand makes the result a
                    // String, and the numeric kinds of one operand reach
                    // the result only when the other may be no String.
                    use crate::model::guard::NOT_STR;
                    let nl = self.tmp("plus-num");
                    let nr = self.tmp("plus-num");
                    self.m.cons(l, Cons::Plus { str: out, num: nl });
                    self.m.cons(rv, Cons::Plus { str: out, num: nr });
                    self.m.cons(
                        rv,
                        Cons::Guard {
                            test: NOT_STR,
                            from: nl,
                            dst: out,
                        },
                    );
                    self.m.cons(
                        l,
                        Cons::Guard {
                            test: NOT_STR,
                            from: nr,
                            dst: out,
                        },
                    );
                } else {
                    for v in [l, rv] {
                        self.m.cons(v, Cons::Arith { dst: out });
                    }
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
                    let rop = match op {
                        UnaryOp::Neg => crate::model::RangeOp::Neg,
                        _ => crate::model::RangeOp::BitNot,
                    };
                    self.range_rule(rop, v, None, out);
                    self.m.cons(v, Cons::Arith { dst: out });
                    out
                }
            },
            Expr::Compare { op, left, right } => match op {
                CompareOp::Eq | CompareOp::Ne => {
                    // IsLooselyEqual runs ToPrimitive on an object operand.
                    let l = self.expr(fx, left);
                    let r = self.expr(fx, right);
                    self.raise_on(l);
                    self.raise_on(r);
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
                let plan = match op {
                    LogicalOp::And => self.own_guard_plan(left, crate::gen::Region::Expr(right)),
                    LogicalOp::Or => match left.as_ref() {
                        Expr::Unary {
                            op: UnaryOp::Not,
                            operand,
                        } => self.own_guard_plan(operand, crate::gen::Region::Expr(right)),
                        _ => None,
                    },
                    _ => None,
                };
                let l = self.expr(fx, left);
                let own = self.own_guard_arm(plan, l);
                let k = match op {
                    LogicalOp::And => self.narrow_guard(fx, left, true),
                    LogicalOp::Or => self.narrow_guard(fx, left, false),
                    LogicalOp::Coalesce => 0,
                };
                let r = self.expr(fx, right);
                self.narrow_pop(k);
                self.own_guard_done(own);
                let out = self.tmp("logical");
                // `l ?? r` is `l` when it is not nullish; `l && r` is `l`
                // when it is falsy.
                match op {
                    LogicalOp::Coalesce => self.m.cons(l, Cons::NotNullish { dst: out }),
                    LogicalOp::And => self.m.cons(l, Cons::Falsy { dst: out }),
                    LogicalOp::Or => self.m.edge(l, out),
                }
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
                let plan = self.own_guard_plan(condition, crate::gen::Region::Expr(then_expr));
                let (t, then_test, else_test) = self.guard_test(fx, condition);
                let own = self.own_guard_arm(plan, t);
                let k = self.narrow_guard(fx, condition, true);
                let a = self.expr(fx, then_expr);
                self.narrow_pop(k);
                self.own_guard_done(own);
                let k = self.narrow_guard(fx, condition, false);
                let b = self.expr(fx, else_expr);
                self.narrow_pop(k);
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
            Expr::InstanceOf { expr, ty, ty_expr } => {
                let lhs = self.expr(fx, expr);
                // The right-hand side is a value: the dynamic type
                // expression, or the user class `ty` names (codegen
                // `class_ids`: a local or imported user class, called with
                // its class value). InstanceofOperator runs its
                // `@@hasInstance` (GetMethod) either way.
                let rhs = match ty_expr {
                    Some(t) => Some(self.expr(fx, t)),
                    None => self.instanceof_class(mi, ty),
                };
                if let Some(tv) = rhs {
                    // Symbol.hasInstance is a protocol method, called with
                    // the left-hand side.
                    let r = self.m.reason("Symbol.hasInstance");
                    self.m.cons(
                        tv,
                        Cons::HasInstance {
                            reason: r,
                            arg: lhs,
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
                // HasProperty on a Proxy runs its `has` trap.
                let o = self.expr(fx, object);
                self.raise_on(o);
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
                // `E.prototype.K`: the read of `K` runs per value of `E`
                // (`Cons::ProtoKey`), so that a builtin namespace's prototype
                // member is the native it names.
                if let Some((e, o, a)) = self.proto_member(fx, object, property) {
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
                    let dst = self.tmp("load");
                    self.m.cons(e, Cons::ProtoKey { key: a, dst });
                    return dst;
                }
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
                // A guarded private method read (`this.#m`) is the declaring
                // brand's member, as its call is: the runtime takes it from
                // the class registry, never from the receiver's chain.
                if let Some(callee) = self.private_method_callee(mi, object) {
                    return callee;
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
                            self.store_phased(
                                fx,
                                e,
                                target,
                                o,
                                key,
                                v,
                                what.into(),
                                owner,
                                Some(k),
                            );
                        }
                    }
                } else {
                    // A key built from literal parts writes only names
                    // matching them (`Model::key_shape`).
                    let key = match self.key_of(fx.module, key) {
                        Key::Any(_) => Key::Any(self.key_shape(fx.module, key)),
                        k => k,
                    };
                    // A receiver other than the target gets the own data store.
                    for (o, what) in [(t, "putvalue"), (rv, "putvalue-receiver")] {
                        let site = self.store_site(fx.module, fx.f, what.into(), owner, false);
                        self.mark_fn_phase(fx, e, target, site);
                        self.mark_rooted(e, site);
                        self.m.stores[site as usize].key_value = Some(k);
                        match key {
                            Key::Named(_) => self.m.cons(o, Cons::Store { key, src: v, site }),
                            // The key's values decide: an index or any key.
                            Key::Any(shape) => self.m.cons(
                                k,
                                Cons::StoreIndexed {
                                    o,
                                    src: v,
                                    site,
                                    shape,
                                },
                            ),
                        }
                    }
                }
                // Observing a key must not intern atoms or add solver work.
                let atom = match key.as_ref() {
                    Expr::String(s) => self.m.atom_index.get(s).copied(),
                    Expr::SymbolFor(name) => match name.as_ref() {
                        Expr::String(s) if s.starts_with(crate::builtins::WK_PREFIX) => {
                            self.m.atom_index.get(s).copied()
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let element_key = atom.map_or(Key::any(), Key::Named);
                let idx = self.for_in_key_of(fx.module, key).unwrap_or(k);
                self.element_site(fx, e, rv, idx, element_key, crate::derive::ElementOp::Write);
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
                self.m.cons(cur, Cons::Arith { dst: nv2 });
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
                let key = self.read_key(fx.module, index);
                let idx = self.for_in_key_of(fx.module, index).unwrap_or(i);
                self.element_site(fx, e, o, idx, key, crate::derive::ElementOp::Read);
                // A for-of walk's element (`Gen::for_of_walks`): the counter
                // is below the length, so the read is what the iteration of
                // the array yields (a hole's `undefined` included), as the
                // IteratorRecord loop's element is, and never the past-the-end
                // `undefined` of an arbitrary index.
                let walk = match (object.as_ref(), index.as_ref()) {
                    (Expr::LocalGet(a), Expr::LocalGet(k)) => {
                        self.for_of_walks.contains(&(fx.module, *a, *k))
                    }
                    _ => false,
                };
                if walk {
                    return self.iterate(o);
                }
                match key {
                    Key::Named(a) => self.load(o, Key::Named(a)),
                    Key::Any(_) => {
                        let dst = self.tmp("idxload");
                        // A for-in run's key is its key parameter (FVA 4.3b).
                        let idx = self.for_in_key_of(fx.module, index).unwrap_or(i);
                        let shape = self.key_shape(fx.module, index);
                        match self.own_reads.remove(&(e as *const Expr as usize)) {
                            // Behind an own-property test (`gen/own_guard.rs`).
                            Some(bad) => {
                                self.m.cons(o, Cons::IdxOwnLoad { idx, dst, shape });
                                self.m.cons(
                                    bad,
                                    Cons::OwnGate {
                                        obj: o,
                                        idx,
                                        dst,
                                        shape,
                                    },
                                );
                            }
                            // After a store of the same object and key
                            // (`gen/own_store.rs`).
                            None if self.own_store_reads.contains(&(e as *const Expr as usize)) => {
                                self.m.cons(o, Cons::IdxOwnLoad { idx, dst, shape })
                            }
                            None => self.m.cons(o, Cons::IdxLoad { idx, dst, shape }),
                        }
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
                    // A key built from literal parts writes only names
                    // matching them (`Model::key_shape`).
                    let key = match self.key_of(fx.module, index) {
                        Key::Any(_) => Key::Any(self.key_shape(fx.module, index)),
                        k => k,
                    };
                    self.store_elem(fx, e, o, key, i, v, "[]=");
                }
                let key = match index.as_ref() {
                    Expr::String(s) => Key::Named(self.m.atom_index[s]),
                    _ => Key::any(),
                };
                let idx = self.for_in_key_of(fx.module, index).unwrap_or(i);
                self.element_site(fx, e, o, idx, key, crate::derive::ElementOp::Write);
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
                let key = self.key_of(fx.module, index);
                self.element_site(fx, e, o, i, key, crate::derive::ElementOp::Read);
                self.element_site(fx, e, o, i, key, crate::derive::ElementOp::Write);
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
                self.m.cons(cur, Cons::Arith { dst: nv });
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
                let tm = self.new_arr_birth(mi, "lit", true);
                if self.arr_lengths {
                    self.m.arr_birth_len.insert(tm, elems.len() as i64);
                }
                let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
                let fa = self.m.field_any(t);
                for el in elems {
                    let v = self.expr(fx, el);
                    self.m.edge(v, fa);
                }
                self.tok_node(t)
            }
            Expr::ArraySpread(elems) => {
                let dense = !elems.iter().any(|e| matches!(e, ArrayElement::Hole));
                let tm = self.new_arr_birth(mi, "spread", dense);
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
            } => self.call_expr(fx, callee, args, &[], *byte_offset),
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
                class_name,
                args,
                cap_args_appended,
                ..
            } => {
                // A chunked literal birth: its chunk calls receive the object
                // being born, whose chunked slots they initialize.
                let chunked = args.iter().any(|a| matches!(a, Expr::LiteralChunks(_)));
                if chunked {
                    let x = self.tmp("literal chunk object");
                    self.chunk_objects.push(x);
                }
                // The appended capture forwards are snapshot reads.
                let caps = args.len().saturating_sub(*cap_args_appended as usize);
                let a: Vec<NodeId> = args
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        if i >= caps {
                            self.snapshot_read(fx, e)
                        } else {
                            self.expr(fx, e)
                        }
                    })
                    .collect();
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
            Expr::EnumMember {
                enum_name,
                member_name,
            } => {
                let p = self.enum_member_prim(mi, enum_name, member_name);
                self.prim_node(p)
            }
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
                self.store(fx, c, Key::any(), v, "static[sym]".into(), None, false);
                v
            }
            Expr::RegisterClassParentDynamic {
                class_name,
                parent_expr,
            } => {
                let p = self.expr(fx, parent_expr);
                let publish = |g: &mut Self| {
                    let tm = g.resolve_class_name(mi, class_name);
                    if !g.parent_to_cell(mi, class_name, tm, p) {
                        g.feed_parent(p, tm, "E11 dynamic class parent");
                    }
                };
                #[cfg(test)]
                self.packet_class_effect(
                    mi,
                    class_name,
                    crate::summary::packet::ClassAction::DynamicParent(p),
                    publish,
                );
                #[cfg(not(test))]
                publish(self);
                self.prim_node(prim::UNDEF)
            }
            Expr::RegisterClassCaptures {
                class_name,
                captures,
            } => {
                for (i, c) in captures.iter().enumerate() {
                    let v = self.snapshot_read(fx, c);
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
                    let v = self.snapshot_read(fx, c);
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
                let key_value = self.expr(fx, key_expr);
                #[cfg(not(test))]
                let _ = key_value;
                let publish = |g: &mut Self| {
                    if let Some(tm) = g.resolve_class_name(mi, class_name) {
                        let holder = if *is_static {
                            g.m.tok(TokKind::ClassObj(tm))
                        } else {
                            g.m.tok(TokKind::Proto(tm))
                        };
                        let a = g.m.atom(method_name);
                        let src = g.m.field(holder, a);
                        let fa = g.m.field_any(holder);
                        g.m.edge(src, fa);
                    }
                };
                #[cfg(test)]
                self.packet_class_effect(
                    mi,
                    class_name,
                    crate::summary::packet::ClassAction::ComputedMethod {
                        key: key_value,
                        method: method_name.clone(),
                        is_static: *is_static,
                    },
                    publish,
                );
                #[cfg(not(test))]
                publish(self);
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
                let key_value = self.expr(fx, key_expr);
                #[cfg(not(test))]
                let _ = key_value;
                // Under a constant key it is that key's accessor, installed
                // with the class (`Gen::named_computed`).
                let named = !*is_static && self.const_key(mi, key_expr).is_some();
                let publish = |g: &mut Self| {
                    if let (false, Some(tm)) = (named, g.resolve_class_name(mi, class_name)) {
                        g.computed_accessor(mi, tm, *is_static, getter_name, setter_name);
                    }
                };
                #[cfg(test)]
                self.packet_class_effect(
                    mi,
                    class_name,
                    crate::summary::packet::ClassAction::ComputedAccessor {
                        key: key_value,
                        getter: getter_name.clone(),
                        setter: setter_name.clone(),
                        is_static: *is_static,
                        named,
                    },
                    publish,
                );
                #[cfg(not(test))]
                publish(self);
                self.prim_node(prim::UNDEF)
            }
            Expr::ClassExprFresh {
                template,
                evaluation_owner,
                named_statics,
                computed_keys,
                computed_statics,
                static_init_order,
                captured_args,
                shared_first_evaluation,
                evaluated_parent,
                definition_steps,
                ..
            } => {
                let cv = self.class_value(mi, template);
                // Rule C (DECISIONS 251): one template, one class object per
                // evaluation.
                let publish = |g: &mut Self| {
                    if let Some(tm) = g.resolve_class_name(mi, template) {
                        g.m.tmpls[tm as usize].per_evaluation = true;
                    }
                };
                #[cfg(test)]
                self.packet_class_effect(
                    mi,
                    template,
                    crate::summary::packet::ClassAction::PerEvaluation,
                    publish,
                );
                #[cfg(not(test))]
                publish(self);
                if let Some(id) = evaluation_owner {
                    // The class's self binding gets the class before its
                    // statics run. A binding the HIR made a cell (codegen's
                    // `store_evaluation_owner`) gets it in element 0, not in
                    // the binding: the binding holds the cell (DECISIONS 50).
                    let l = self.local(mi, *id);
                    if self.cell_locals.contains(&(mi, *id)) {
                        self.store(fx, l, Key::any(), cv, "owner[0]=".into(), None, false);
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
                    self.store(fx, cv, Key::any(), vn, "static[k]".into(), None, false);
                }
                // Each static block runs at this evaluation with `this` the
                // evaluated class object: codegen calls the template's block
                // function directly (`js_static_this_arm_value`), no lookup.
                self.fresh_static_blocks(fx, template, static_init_order, cv);
                for (i, a) in captured_args.iter().enumerate() {
                    let v = self.snapshot_read(fx, a);
                    let env = self.class_env(mi, template, i);
                    self.m.edge(v, env);
                }
                if let Some(s) = shared_first_evaluation {
                    let _ = self.expr(fx, s);
                }
                if let Some(p) = evaluated_parent {
                    let v = self.expr(fx, p);
                    let publish = |g: &mut Self| {
                        let tm = g.resolve_class_name(mi, template);
                        if !g.parent_to_cell(mi, template, tm, v) {
                            g.feed_parent(v, tm, "E11 class expression with an evaluated parent");
                        }
                    };
                    #[cfg(test)]
                    self.packet_class_effect(
                        mi,
                        template,
                        crate::summary::packet::ClassAction::EvaluatedParent(v),
                        publish,
                    );
                    #[cfg(not(test))]
                    publish(self);
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
                let publish = |g: &mut Self| match g.resolve_class_name(mi, class_name) {
                    Some(tm) => {
                        let p = g.m.tok(TokKind::Proto(tm));
                        let a = g.m.atom(method_name);
                        let f = g.m.field(p, a);
                        g.m.edge(v, f);
                    }
                    None => g.to_sink(v, "method registered on an unresolved class"),
                };
                #[cfg(test)]
                self.packet_class_effect(
                    mi,
                    class_name,
                    crate::summary::packet::ClassAction::PrototypeMethod {
                        key: method_name.clone(),
                        value: v,
                    },
                    publish,
                );
                #[cfg(not(test))]
                publish(self);
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
                let homes = self.super_homes(fx);
                self.super_access(homes, fx.this, Key::Named(a), ChainAct::Load(dst));
                dst
            }
            Expr::SuperPropertySet { key, value, .. } => {
                let k = self.super_key(fx, key);
                let v = self.expr(fx, value);
                self.store(fx, fx.this, k, v, "super.x=".into(), None, false);
                let homes = self.super_homes(fx);
                self.super_access(homes, fx.this, k, ChainAct::Store(v));
                v
            }

            // ----------------------------------------------- the rest
            _ => self.expr_rest(fx, e),
        }
    }
}

impl<'a> Gen<'a> {
    /// The shape of computed key `e` of module `mi`: its literal parts
    /// (`KeyShape::of`), for a call of one of the module's functions
    /// (HIR `FuncRef`: a declaration the program reassigns is a local, so
    /// the call runs this body) what that body returns
    /// (`KeyShape::of_returns`), for a parameter what its calls pass
    /// (`Gen::key_shape_of`).
    pub(crate) fn key_shape(&mut self, mi: u32, e: &Expr) -> KeyShapeId {
        let s = self.key_shape_of(mi, e, &mut Vec::new());
        self.m.key_shape(s.filter(|s| !s.globs.is_empty()))
    }
}
