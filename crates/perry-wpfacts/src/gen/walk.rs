//! Constraint generation, part 1c: walking modules, classes and statements.

use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    // ----------------------------------------------------------- walking

    pub fn walk_all(&mut self) {
        let modules = self.modules.clone();
        // Constructor phase analysis first (needs every class).
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                let ph = crate::ctor_phase::analyze(c);
                self.ctor_phase.insert(tm, ph);
            }
        }
        // Parent leaks close the child's phase at `super()`.
        let tms: Vec<TmplId> = self.ctor_phase.keys().copied().collect();
        for tm in tms.iter().copied() {
            let leaks = self.parent_chain_leaks(tm);
            if let Some(ph) = self.ctor_phase.get_mut(&tm) {
                ph.parent_leaks = leaks;
            }
        }
        // Birth keys: root → leaf.
        for (mi, (_, module)) in modules.iter().enumerate() {
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                self.compute_birth_keys(tm, module);
            }
        }
        // DECISIONS 225: a module only `import()` loads runs when an active
        // body loads it; its body is walked then (`PendingWalk::Module`).
        if self.m.act.is_some() {
            self.dynamic_only = self.dynamic_only_modules();
        }
        if !self.dynamic_only.is_empty() {
            for mi in 0..modules.len() as u32 {
                let f = self.synthetic_fn(mi, "<init>");
                self.init_fn_of.insert(mi, f);
            }
        }
        for mi in 0..modules.len() {
            // Over a bound: the program gets no facts; stop generating.
            if self.m.aborted {
                return;
            }
            let mi = mi as u32;
            let init_fn = match self.init_fn_of.get(&mi) {
                Some(&f) => f,
                None => {
                    let f = self.synthetic_fn(mi, "<init>");
                    self.init_fn_of.insert(mi, f);
                    f
                }
            };
            if self.dynamic_only.contains(&mi) && self.defer_walk(init_fn, PendingWalk::Module(mi))
            {
                continue;
            }
            self.walk_module(mi, init_fn);
            if self.m.aborted {
                return;
            }
        }
    }

    /// The modules that load only through `import()`: no static import or
    /// re-export reaches them from a module no edge enters (the entry, a
    /// worker). Empty when a re-export source does not resolve (every
    /// module loads).
    fn dynamic_only_modules(&self) -> crate::pts::FxHashSet<u32> {
        let n = self.modules.len();
        let mut entered = vec![false; n];
        let mut succ: Vec<Vec<u32>> = vec![Vec::new(); n];
        for mi in 0..n {
            let imports = self.imports_of(mi);
            let path = self.modules[mi].0;
            // A re-export source as the compile driver resolved it
            // (`export_sources`), else through an import of the same source.
            let target = |source: &str| {
                self.export_sources
                    .get(path)
                    .and_then(|m| m.get(source))
                    .and_then(|r| self.path_index.get(r).copied())
                    .or_else(|| {
                        imports
                            .iter()
                            .find(|i| i.source == source)
                            .and_then(|i| i.resolved_path.as_ref())
                            .and_then(|r| self.path_index.get(r).copied())
                    })
            };
            for i in &imports {
                if i.type_only || i.runtime_erased {
                    continue;
                }
                let Some(t) = i
                    .resolved_path
                    .as_ref()
                    .and_then(|r| self.path_index.get(r).copied())
                else {
                    continue;
                };
                entered[t as usize] = true;
                if !i.is_dynamic {
                    succ[mi].push(t);
                }
            }
            for e in &self.modules[mi].1.exports {
                let source = match e {
                    perry_hir::Export::ReExport { source, .. }
                    | perry_hir::Export::ExportAll { source }
                    | perry_hir::Export::NamespaceReExport { source, .. } => source,
                    perry_hir::Export::Named { .. } => continue,
                };
                if source.starts_with("node:")
                    || !(source.starts_with('.') || source.starts_with('/'))
                {
                    if let Some(t) = target(source) {
                        entered[t as usize] = true;
                        succ[mi].push(t);
                    }
                    continue;
                }
                match target(source) {
                    Some(t) => {
                        entered[t as usize] = true;
                        succ[mi].push(t);
                    }
                    None => {
                        return Default::default();
                    }
                }
            }
        }
        let mut eager = vec![false; n];
        let mut work: Vec<u32> = (0..n as u32)
            .filter(|&m| {
                !entered[m as usize] || self.roots.iter().any(|r| r == self.modules[m as usize].0)
            })
            .collect();
        for &m in &work {
            eager[m as usize] = true;
        }
        while let Some(m) = work.pop() {
            for &t in &succ[m as usize] {
                if !eager[t as usize] {
                    eager[t as usize] = true;
                    work.push(t);
                }
            }
        }
        (0..n as u32).filter(|&m| !eager[m as usize]).collect()
    }

    /// The body of module `mi`: its globals, top-level statements, functions
    /// and classes. A module it loads statically loads with it.
    pub(crate) fn walk_module(&mut self, mi: u32, init_fn: FnIdx) {
        if !self.dynamic_only.is_empty() {
            for i in self.imports_of(mi as usize) {
                if i.is_dynamic || i.type_only || i.runtime_erased {
                    continue;
                }
                let t = i
                    .resolved_path
                    .as_ref()
                    .and_then(|r| self.path_index.get(r).copied());
                if let Some(t) = t.filter(|t| self.dynamic_only.contains(t)) {
                    let f = self.init_fn_of[&t];
                    self.m.activate(f, "import");
                }
            }
        }
        self.m.note_walked(init_fn);
        let this = self.m.funcs[init_fn as usize].this;
        // Module top-level `this`: CJS `module.exports`-like object or
        // undefined in ESM; ⊤-free: a fresh literal-ish object.
        self.m.add_prim(this, prim::UNDEF);
        let fx = Fx {
            guard: None,
            module: mi,
            f: init_fn,
            this,
            ret: self.m.funcs[init_fn as usize].ret,
            class: None,
            ctor_of: None,
            new_target: self.m.all_prim_node,
            fn_ctor: false,
        };
        for g in self.globals_of(mi as usize) {
            if let Some(init) = &g.init {
                let v = self.expr(&fx, init);
                let gn = self.global(mi, g.id);
                self.m.edge(v, gn);
            }
        }
        for s in self.init_of(mi as usize) {
            self.stmt(&fx, s);
            self.name_enum_object(mi, s);
        }
        for f in self.functions_of(mi as usize) {
            if self.m.aborted {
                return;
            }
            let fxi = self.mods[mi as usize].funcs[&f.id];
            self.walk_fn(mi, fxi, f, None, None);
        }
        for c in self.classes_of(mi as usize) {
            if self.m.aborted {
                return;
            }
            self.walk_class(mi, c, &fx);
        }
    }

    fn compute_birth_keys(&mut self, tm: TmplId, module: &Module) {
        // Another unit's class: its keys are its contract's.
        if !self.m.tmpls[tm as usize].birth_keys.is_empty() || self.stand_in_cc.contains_key(&tm) {
            return;
        }
        let mut keys = Vec::new();
        let mut definite = Vec::new();
        let mut reserved = Vec::new();
        if let Some(p) = self.m.tmpls[tm as usize].parent {
            if let TmplKind::Class { module: pm, .. } = self.m.tmpls[p as usize].kind {
                let pmod = self.modules[pm as usize].1;
                self.compute_birth_keys(p, pmod);
            }
            keys = self.m.tmpls[p as usize].birth_keys.clone();
            definite = self.m.tmpls[p as usize].definite.clone();
            reserved = self.m.tmpls[p as usize].reserved.clone();
        }
        if let Some((_, np)) = self.m.tmpls[tm as usize].native_parent.clone() {
            // Keys the native parent constructor defines (summary, E21).
            for k in crate::builtins::native_parent_keys(&np) {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
            }
        }
        let TmplKind::Class { name, .. } = self.m.tmpls[tm as usize].kind.clone() else {
            return;
        };
        let Some(c) = module.classes.iter().find(|c| c.name == name) else {
            return;
        };
        let mut declared = Vec::new();
        let phase = self.ctor_phase.get(&tm).cloned();
        // A parent constructor that leaks `this` lets an observer see the
        // instance before any key of this class is stored or defined. Asked
        // of the chain as it stands: `CtorPhase::parent_leaks` was taken
        // before the solve, when a parent value not yet settled counts as a
        // leak; `settle_parent_layouts` recomputes these keys once it is.
        let parent_leaks = !self.parent_pending(tm) && self.parent_chain_leaks(tm);
        for f in &c.fields {
            if f.is_private || f.key_expr.is_some() {
                continue;
            }
            let a = self.m.atom(&f.name);
            push_unique(&mut keys, a);
            match f.origin {
                perry_hir::ClassFieldOrigin::Declared => push_unique(&mut declared, a),
                // A reservation holds the hole until the constructor stores
                // it: present from birth on only when the construction phase
                // stores it before anyone can observe the instance
                // (`CtorPhase::definite`, below).
                perry_hir::ClassFieldOrigin::ConstructorStore => push_unique(&mut reserved, a),
                // A field is defined at the birth, in order: present from
                // birth on unless the instance could be observed before its
                // definition (`CtorPhase::late_fields`, a leaking parent);
                // then it is a reservation like a constructor store.
                perry_hir::ClassFieldOrigin::Definition
                    if parent_leaks
                        || phase
                            .as_ref()
                            .is_some_and(|ph| ph.late_fields.contains(&f.name)) =>
                {
                    push_unique(&mut reserved, a)
                }
                perry_hir::ClassFieldOrigin::Definition => push_unique(&mut definite, a),
            }
        }
        self.m.tmpls[tm as usize].declared = declared;
        if let Some(ph) = phase {
            for k in &ph.assigned_order {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
            }
            if !parent_leaks {
                for k in &ph.definite {
                    let a = self.m.atom(k);
                    push_unique(&mut definite, a);
                }
            }
        }
        // A parent that is a value (§17.2) is not known before the solve
        // settles it: the keys its constructor adds first are unknown, so
        // no key of this class has a known slot or is known present (a read
        // may see `undefined` while the parent runs). Settled, the parent's
        // keys are the prefix (`settle_parent_layouts`).
        if self.parent_pending(tm) {
            definite.clear();
            reserved.clear();
        } else if self.value_parent_on_chain(tm) && self.parent_chain_leaks(tm) {
            let n = self.m.tmpls[tm as usize]
                .parent
                .map_or(0, |p| self.m.tmpls[p as usize].definite.len());
            definite.truncate(n);
        }
        reserved.retain(|a| !definite.contains(a));
        self.m.tmpls[tm as usize].birth_keys = keys;
        self.m.tmpls[tm as usize].definite = definite;
        self.m.tmpls[tm as usize].reserved = reserved;
    }

    /// Some class on `tm`'s chain has a parent value not settled to one
    /// class (§17.2).
    fn parent_pending(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            let t = &self.m.tmpls[c as usize];
            if t.parent_value.is_some() && t.parent.is_none() {
                return true;
            }
            cur = t.parent;
        }
        false
    }

    /// Some class on `tm`'s chain takes its parent from a value (§17.2).
    fn value_parent_on_chain(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if self.m.tmpls[c as usize].parent_value.is_some() {
                return true;
            }
            cur = self.m.tmpls[c as usize].parent;
        }
        false
    }

    /// A constructor up `tm`'s chain may leak `this` before `tm`'s own
    /// stores run: one that leaks, a native parent, a parent value not
    /// settled, or another unit's class (its constructor is not seen).
    /// Whether `tm`'s constructor or one up its chain leaks `this`
    /// (published as `ClassContract::ctor_leaks`).
    pub(crate) fn chain_leaks(&self, tm: TmplId) -> bool {
        self.ctor_phase.get(&tm).map_or(true, |ph| ph.leaks) || self.parent_chain_leaks(tm)
    }

    /// Whether `tm`'s constructor, or one up its chain, may return an object
    /// other than its `this` (published as `ClassContract::ctor_returns`).
    /// `new` then yields the returned value, so the class's instance is not
    /// the whole result. Unsettled parents and another unit's constructors
    /// that return answer yes.
    pub(crate) fn chain_returns(&self, tm: TmplId) -> bool {
        if self.parent_pending(tm) {
            return true;
        }
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if self.stand_in_cc.get(&c).is_some_and(|cc| cc.ctor_returns) {
                return true;
            }
            if let Some(f) = self.m.tmpls[c as usize].ctor {
                if self.m.ctor_returns_object(f) {
                    return true;
                }
            }
            cur = self.m.tmpls[c as usize].parent;
        }
        false
    }

    fn parent_chain_leaks(&self, tm: TmplId) -> bool {
        if self.m.tmpls[tm as usize].native_parent.is_some() || self.parent_pending(tm) {
            return true;
        }
        let mut cur = self.m.tmpls[tm as usize].parent;
        while let Some(p) = cur {
            if self.ctor_phase.get(&p).is_some_and(|ph| ph.leaks)
                || self.stand_in_cc.get(&p).is_some_and(|c| c.ctor_leaks)
                || self.m.tmpls[p as usize].native_parent.is_some()
            {
                return true;
            }
            cur = self.m.tmpls[p as usize].parent;
        }
        false
    }

    /// After the solve (§17.2 item 3, DECISIONS 47): each class whose parent
    /// value settled to one class takes that class's keys as its layout
    /// prefix, and every class below it in turn; its own keys are definite
    /// only when no constructor up the chain leaks `this`. The solve ran
    /// with no definite keys for them (`compute_birth_keys`): it saw every
    /// read as possibly `undefined`, which holds for any settled parent.
    pub(crate) fn settle_parent_layouts(&mut self) {
        self.m.settle_parent_values();
        let modules = self.modules.clone();
        let mut redo: Vec<(TmplId, usize)> = Vec::new();
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                if self.value_parent_on_chain(tm) {
                    redo.push((tm, mi));
                }
            }
        }
        for &(tm, _) in &redo {
            self.m.tmpls[tm as usize].birth_keys.clear();
            self.m.tmpls[tm as usize].definite.clear();
            self.m.tmpls[tm as usize].reserved.clear();
        }
        for (tm, mi) in redo {
            self.compute_birth_keys(tm, modules[mi].1);
        }
    }

    pub(crate) fn walk_fn(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &'a Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        let fn_ctor = class.is_none() && ctor_of.is_none();
        if fn_ctor {
            self.note_fn_phase(fxi, crate::ctor_phase::analyze_fn(f));
        }
        self.rooted_stores.extend(crate::rooted::stores(&f.body));
        // DECISIONS 225: a body generation walks once it can run.
        if self.defer_walk(fxi, PendingWalk::Fn(mi, fxi, f, class, ctor_of)) {
            return;
        }
        self.walk_fn_body(mi, fxi, f, class, ctor_of);
    }

    /// DECISIONS 225: whether the body of `f` waits (it is not active):
    /// kept for `walk_activated`, or, with the set final, never walked.
    pub(crate) fn defer_walk(&mut self, f: FnIdx, w: PendingWalk<'a>) -> bool {
        if self.m.body_active(f) {
            return false;
        }
        if !self.m.act.as_ref().is_some_and(|a| a.fixed) {
            self.pending_walk.entry(f).or_default().push(w);
        }
        true
    }

    /// DECISIONS 225: walk the bodies activated since the last walk (their
    /// rules apply as generation emits them). Whether any was walked.
    pub(crate) fn walk_activated(&mut self) -> bool {
        let mut any = false;
        loop {
            let q = match self.m.act.as_mut() {
                Some(a) if !a.queue.is_empty() => std::mem::take(&mut a.queue),
                _ => return any,
            };
            for f in q {
                if self.m.aborted {
                    return any;
                }
                for w in self.pending_walk.remove(&f).unwrap_or_default() {
                    any = true;
                    match w {
                        PendingWalk::Fn(mi, fxi, func, class, ctor_of) => {
                            self.walk_fn_body(mi, fxi, func, class, ctor_of)
                        }
                        PendingWalk::Fields(mi, c) => self.walk_fields_body(mi, c),
                        PendingWalk::Closure(cfx, e) => self.walk_closure_body(&cfx, e),
                        PendingWalk::Module(mi) => {
                            let f = self.init_fn_of[&mi];
                            self.walk_module(mi, f);
                        }
                    }
                }
            }
        }
    }

    fn walk_fn_body(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &'a Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        self.m.note_walked(fxi);
        let info = self.m.funcs[fxi as usize].clone();
        let fn_ctor = class.is_none() && ctor_of.is_none();
        let fx = Fx {
            guard: None,
            module: mi,
            f: fxi,
            this: info.this,
            ret: info.ret,
            class,
            ctor_of,
            new_target: self.m.all_prim_node,
            fn_ctor,
        };
        self.params(&fx, &f.params);
        if f.is_async || f.is_generator {
            // Not transformed (rare at this point): results are runtime
            // objects.
            self.m.add_top(info.ret, |_| {
                "async/generator function result (untransformed)".into()
            });
        }
        if self.m.funcs[fxi as usize].forwards_super {
            // `run_ctor_chain` constructs the parent with the `new` call;
            // the rest of the body (capture rebinds and stashes) runs here.
            let rest = f.params[0].id;
            let body: Vec<Stmt> = f
                .body
                .iter()
                .filter(|s| !crate::gen::is_forward_of(s, rest))
                .cloned()
                .collect();
            self.stmts(&fx, &body);
            return;
        }
        self.stmts(&fx, &f.body);
    }

    pub(crate) fn params(&mut self, fx: &Fx, params: &[perry_hir::Param]) {
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        self.params_inner(fx, params);
        self.m.cur_fn = prev;
    }

    fn params_inner(&mut self, fx: &Fx, params: &[perry_hir::Param]) {
        for p in params {
            let n = self.local(fx.module, p.id);
            if let Some(d) = &p.default {
                // The default is the parameter's value only when a call
                // passes `undefined` or nothing (missing arguments are
                // `undefined`, `bind_args`).
                let v = self.expr(fx, d);
                self.m.cons(
                    n,
                    Cons::Guard {
                        test: crate::model::guard::UNDEF,
                        from: v,
                        dst: n,
                    },
                );
            }
        }
    }

    fn walk_class(&mut self, mi: u32, c: &'a Class, outer: &Fx) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // DECISIONS 225: the field initializers run when an instance is
        // constructed (`run_ctor_chain`).
        let fi = self.field_init_fn[&tm];
        if !self.defer_walk(fi, PendingWalk::Fields(mi, c)) {
            self.walk_fields_body(mi, c);
        }
        self.walk_class_rest(mi, c, outer);
    }

    fn walk_fields_body(&mut self, mi: u32, c: &'a Class) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // Instance field initializers run in the field-init context.
        let fi = self.field_init_fn[&tm];
        self.m.note_walked(fi);
        let fthis = self.m.funcs[fi as usize].this;
        let ffx = Fx {
            guard: None,
            module: mi,
            f: fi,
            this: fthis,
            ret: self.m.funcs[fi as usize].ret,
            class: Some(tm),
            ctor_of: Some(tm),
            new_target: self.m.all_prim_node,
            fn_ctor: false,
        };
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let ctor_definite: Vec<String> = self
            .ctor_phase
            .get(&tm)
            .filter(|ph| !ph.parent_leaks)
            .map(|ph| ph.definite.clone())
            .unwrap_or_default();
        for f in &c.fields {
            let site = self.store_site(mi, fi, format!("field {}", f.name), Some(tm), true);
            let v = match &f.init {
                Some(e) => self.expr(&ffx, e),
                // `x: T;` defines `undefined` first; when the constructor
                // assigns it before anyone can observe the instance, that
                // `undefined` is never seen.
                None if f.key_expr.is_none()
                    && (ctor_definite.contains(&f.name) || c.is_literal_shape()) =>
                {
                    // An object literal's shape: its constructor stores
                    // every key from the literal's values first.
                    continue;
                }
                None => self.prim_node(prim::UNDEF),
            };
            match &f.key_expr {
                Some(k) => {
                    let _ = self.expr(&ffx, k);
                    let thisn = self.m.node_l("field-this");
                    self.m.add_tok(thisn, inst);
                    self.m.edge(fthis, thisn);
                    self.m.cons(
                        thisn,
                        Cons::Store {
                            key: Key::Any,
                            src: v,
                            site,
                        },
                    );
                }
                None => {
                    // `#x`: the class's private field, a key apart from
                    // every public one (`Model::private_atom`).
                    let a = match f.is_private {
                        true => {
                            let key = self.private_field_key(mi, c, &f.name);
                            self.m.private_atom(&key)
                        }
                        false => self.m.atom(&f.name),
                    };
                    let thisn = self.m.node_l("field-this");
                    self.m.edge(fthis, thisn);
                    self.m.cons(
                        thisn,
                        Cons::Store {
                            key: Key::Named(a),
                            src: v,
                            site,
                        },
                    );
                }
            }
        }
    }

    fn walk_class_rest(&mut self, mi: u32, c: &'a Class, outer: &Fx) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // Static fields and static blocks: `this` is the class object.
        let si = self.static_init_fn[&tm];
        self.m.note_walked(si);
        let sfx = Fx {
            guard: None,
            module: mi,
            f: si,
            this: self.m.funcs[si as usize].this,
            ret: self.m.funcs[si as usize].ret,
            class: Some(tm),
            ctor_of: None,
            new_target: self.m.all_prim_node,
            fn_ctor: false,
        };
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        for f in &c.static_fields {
            // A declared static field (`Model::static_fields`), whether the
            // walk sees its initializer here or where a nested class is
            // evaluated.
            let named = match f.is_private {
                true => {
                    let key = self.private_field_key(mi, c, &f.name);
                    self.m.private_atom(&key)
                }
                false => self.m.atom(&f.name),
            };
            if f.key_expr.is_none() {
                self.m.static_fields.insert((tm, named));
            }
            if c.is_nested && (f.init.is_some() || f.key_expr.is_some()) {
                // A nested class's static initializers run where the class
                // is evaluated (`StaticFieldSet`, `ClassStaticSymbolSet`,
                // the statics of `ClassExprFresh`), never from the
                // declaration (codegen `static_fields.rs`); this copy is
                // not rewritten for shared-mutable captures (DECISIONS 50).
                continue;
            }
            let v = match &f.init {
                Some(e) => self.expr(&sfx, e),
                None => self.prim_node(prim::UNDEF),
            };
            let fl = match &f.key_expr {
                Some(k) => {
                    let _ = self.expr(&sfx, k);
                    self.m.field_any(cobj)
                }
                None => self.m.field(cobj, named),
            };
            self.m.edge(v, fl);
        }
        for d in &c.decorators {
            for a in &d.args {
                let v = self.expr(outer, a);
                let r = self.m.reason("decorator argument");
                self.m.sink_node(v, r);
            }
            // Decorators receive the class value: unmodeled code.
            let r = self.m.reason("class decorator");
            self.m.escape(cobj, r);
        }
        if let Some(e) = &c.extends_expr {
            let _ = self.expr(outer, e);
        }
        if let Some(ctor) = &c.constructor {
            let f = self.mods[mi as usize].funcs[&ctor.id];
            self.walk_fn(mi, f, ctor, Some(tm), Some(tm));
        }
        for meth in c.methods.iter().chain(c.static_methods.iter()) {
            let f = self.mods[mi as usize].funcs[&meth.id];
            self.walk_fn(mi, f, meth, Some(tm), None);
        }
        for (_, g) in c.getters.iter().chain(c.setters.iter()) {
            let f = self.mods[mi as usize].funcs[&g.id];
            self.walk_fn(mi, f, g, Some(tm), None);
        }
        for cm in &c.computed_members {
            let _ = self.expr(outer, &cm.key_expr);
            let f = self.mods[mi as usize].funcs[&cm.function.id];
            // DECISIONS 225: a computed accessor runs on reads through the
            // prototype under any key (its `this` is seeded at
            // registration), never through a call: active with its class.
            self.m.activate(f, "computed");
            self.walk_fn(mi, f, &cm.function, Some(tm), None);
        }
        for d in c.fields.iter().chain(c.static_fields.iter()) {
            for dec in &d.decorators {
                let r = self.m.reason("field decorator");
                self.m.escape(cobj, r);
                for a in &dec.args {
                    let _ = self.expr(outer, a);
                }
            }
        }
        for meth in c.methods.iter().chain(c.static_methods.iter()) {
            if !meth.decorators.is_empty() {
                let r = self.m.reason("method decorator");
                self.m.escape(cobj, r);
            }
        }
    }

    pub(crate) fn store_site(
        &mut self,
        mi: u32,
        f: FnIdx,
        desc: String,
        ctor_phase_of: Option<TmplId>,
        birth: bool,
    ) -> SiteId {
        let id = self.m.stores.len() as SiteId;
        self.m.stores.push(StoreSite {
            module: mi,
            func: f,
            desc,
            ctor_phase_of,
            birth,
            ctor_fn: None,
            rooted: false,
            deep: false,
            key_value: None,
        });
        id
    }

    /// Record the construction-phase analysis of a function that `new` may
    /// call: its `this` stores before a leak are birth writers of `new f()`.
    pub(crate) fn note_fn_phase(&mut self, f: FnIdx, ph: CtorPhase) {
        if ph.assigned_order.is_empty() {
            return;
        }
        let birth: Vec<Atom> = ph.assigned_order.iter().map(|k| self.m.atom(k)).collect();
        let definite: Vec<Atom> = ph.definite.iter().map(|k| self.m.atom(k)).collect();
        self.m.fn_birth.insert(f, (birth, definite));
        self.fn_phase.insert(f, ph);
    }

    // ---------------------------------------------------------- statements

    pub(crate) fn stmts(&mut self, fx: &Fx, ss: &[Stmt]) {
        for s in ss {
            self.stmt(fx, s);
        }
    }

    pub(crate) fn stmt(&mut self, fx: &Fx, s: &Stmt) {
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        self.stmt_inner(fx, s);
        self.m.cur_fn = prev;
    }

    /// Per unit: a module-scope TS enum's object (`let E = { … }`, the
    /// lowering of `enum E`) is named `<module>:enum E`, the same in every
    /// unit (a site number is not), so another unit can name its contract
    /// (§18.2). An enum name is one module-scope binding.
    fn name_enum_object(&mut self, mi: u32, s: &Stmt) {
        let Stmt::Let {
            name,
            init: Some(e),
            ..
        } = s
        else {
            return;
        };
        if self.view.is_none()
            || !self.modules[mi as usize]
                .1
                .enums
                .iter()
                .any(|x| &x.name == name)
        {
            return;
        }
        let Some(&(tm, _)) = self.lit_births.get(&(e as *const perry_hir::Expr as usize)) else {
            return;
        };
        let display = format!("{}:enum {name}", self.mods[mi as usize].path_short());
        let t = &mut self.m.tmpls[tm as usize];
        if let TmplKind::Lit { enum_image, .. } = &mut t.kind {
            *enum_image = true;
            t.display = display;
        }
    }

    fn stmt_inner(&mut self, fx: &Fx, s: &Stmt) {
        match s {
            Stmt::Let { id, init, ty, .. } => {
                let n = self.local(fx.module, *id);
                if matches!(ty, perry_hir::types::Type::Array(_)) {
                    self.cell_locals.insert((fx.module, *id));
                }
                if let (Some(perry_hir::Expr::ClassMethodHome { .. }), Some(tm)) = (init, fx.class)
                {
                    self.class_homes.insert((fx.module, *id), tm);
                }
                match init {
                    Some(e) => {
                        let v = self.expr(fx, e);
                        self.m.edge(v, n);
                    }
                    None => self.m.add_prim(n, prim::UNDEF),
                }
                if let Some(perry_hir::Expr::ForInKeys(o)) = init {
                    if let perry_hir::Expr::LocalGet(obj) = o.as_ref() {
                        self.for_in_keys_of.insert((fx.module, *id), *obj);
                    }
                }
            }
            Stmt::Expr(e) => {
                let _ = self.expr(fx, e);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    let v = self.expr(fx, e);
                    self.m.edge(v, fx.ret);
                } else {
                    self.m.add_prim(fx.ret, prim::UNDEF);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let (t, then_test, else_test) = self.guard_test(fx, condition);
                let then_fx = Fx {
                    guard: Some((t, then_test)),
                    ..fx.clone()
                };
                self.stmts(&then_fx, then_branch);
                if let Some(e) = else_branch {
                    let else_fx = Fx {
                        guard: Some((t, else_test)),
                        ..fx.clone()
                    };
                    self.stmts(&else_fx, e);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                let _ = self.expr(fx, condition);
                self.stmts(fx, body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(fx, i);
                }
                if let Some(c) = condition {
                    let _ = self.expr(fx, c);
                }
                if let Some(u) = update {
                    let _ = self.expr(fx, u);
                }
                match self.for_in_of(fx, body) {
                    Some(f) => self.for_in_body(fx, &f),
                    None => self.stmts(fx, body),
                }
            }
            Stmt::Labeled { body, .. } => self.stmt(fx, body),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
            Stmt::PreallocateTdzBoxes(ids) => {
                for id in ids {
                    let n = self.local(fx.module, *id);
                    self.m.add_prim(n, prim::UNDEF);
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(fx, e);
                self.m.edge(v, self.thrown);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(fx, body);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        let n = self.local(fx.module, *id);
                        self.m.edge(self.thrown, n);
                    }
                    self.stmts(fx, &c.body);
                }
                if let Some(f) = finally {
                    self.stmts(fx, f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                let d = self.expr(fx, discriminant);
                // `===` against case values: no coercion.
                let _ = d;
                for c in cases {
                    if let Some(t) = &c.test {
                        let _ = self.expr(fx, t);
                    }
                    self.stmts(fx, &c.body);
                }
            }
        }
    }
}

/// The variant name of an expression, without formatting its children: a
/// `fmt::Write` that stops at the first non-identifier character.
pub(crate) fn variant_name(e: &Expr) -> String {
    struct Head(String, bool);
    impl std::fmt::Write for Head {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            if self.1 {
                return Err(std::fmt::Error);
            }
            for c in s.chars() {
                if c.is_alphanumeric() || c == '_' {
                    self.0.push(c);
                } else {
                    self.1 = true;
                    return Err(std::fmt::Error);
                }
            }
            Ok(())
        }
    }
    let mut h = Head(String::new(), false);
    let _ = std::fmt::write(&mut h, format_args!("{e:?}"));
    h.0
}
