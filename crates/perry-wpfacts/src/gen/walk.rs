//! Constraint generation, part 1c: walking modules, classes and statements.

use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    // ----------------------------------------------------------- walking

    #[cfg(test)]
    pub(crate) fn packet_reads(&mut self, start: usize) {
        let Some(scope) = &self.m.packet_scope else {
            return;
        };
        let f = scope.f;
        let reads: Vec<_> = self.read_sites[start..]
            .iter()
            .filter(|s| s.func == f)
            .cloned()
            .collect();
        for site in reads {
            let key = self.m.atoms[site.key as usize].clone();
            self.m.packet_observe(
                site.byte_offset,
                format!("read call={} value={}", site.is_call, site.value_call),
                key,
                site.recv,
                None,
            );
        }
    }

    pub fn walk_all(&mut self) {
        let modules = self.modules.clone();
        // RPG1b: the once closures and the evaluation order (`derive`'s
        // ordered writers).
        self.once_ids = modules
            .iter()
            .map(|(_, m)| once::once_closures(m))
            .collect();
        self.m.eval_deps = (0..modules.len()).map(|mi| self.eval_deps_of(mi)).collect();
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
            if !modules[mi as usize].1.has_top_level_await {
                self.m.once_fns.insert(init_fn, (mi, false));
            }
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

    /// RPG1b: module `mi`'s static evaluation dependencies (`Model::eval_deps`).
    fn eval_deps_of(&self, mi: usize) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .imports_of(mi)
            .iter()
            .filter(|i| {
                !i.is_dynamic && !i.type_only && !i.runtime_erased && !i.is_deferred_require
            })
            .filter_map(|i| i.resolved_path.as_ref())
            .filter_map(|r| self.path_index.get(r).copied())
            .filter(|&t| t as usize != mi)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
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
            let target = |source: &str| self.reexport_target(mi as u32, source);
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

    /// A re-export source of module `mi` as the compile driver resolved it
    /// (`export_sources`), else through an import of the same source.
    fn reexport_target(&self, mi: u32, source: &str) -> Option<u32> {
        let path = self.modules[mi as usize].0;
        self.export_sources
            .get(path)
            .and_then(|m| m.get(source))
            .and_then(|r| self.path_index.get(r).copied())
            .or_else(|| {
                self.imports_of(mi as usize)
                    .iter()
                    .find(|i| i.source == source)
                    .and_then(|i| i.resolved_path.as_ref())
                    .and_then(|r| self.path_index.get(r).copied())
            })
    }

    /// The body of module `mi`: its globals, top-level statements, functions
    /// and classes. A module it loads statically, by an import or a
    /// re-export, loads with it (`dynamic_only_modules` counts both).
    pub(crate) fn walk_module(&mut self, mi: u32, init_fn: FnIdx) {
        // The module body's throw region: uncaught at evaluation, except a
        // dynamically imported module's, which rejects the import.
        let r = self.m.raise_of(init_fn);
        if self.dynamic_only.contains(&mi) {
            self.m.edge(r, self.thrown);
        }
        // A target this module requires lazily (a deferred CommonJS require)
        // runs its body when the require runs, through the path registry.
        for i in self.imports_of(mi as usize) {
            if !i.is_deferred_require {
                continue;
            }
            let t = i
                .resolved_path
                .as_ref()
                .and_then(|p| self.path_index.get(p).copied());
            if let Some(t) = t {
                let rt = self.module_raise(t);
                self.m.edge(rt, self.m.lazy_raise);
            }
        }
        let region = std::mem::replace(&mut self.m.gen_region, Some(r));
        self.walk_module_inner(mi, init_fn);
        self.m.gen_region = region;
    }

    /// The throw region of module `t`'s body (its `<init>` function's).
    pub(crate) fn module_raise(&mut self, t: u32) -> NodeId {
        let f = match self.init_fn_of.get(&t) {
            Some(&f) => f,
            None => {
                let f = self.synthetic_fn(t, "<init>");
                self.init_fn_of.insert(t, f);
                f
            }
        };
        self.m.raise_of(f)
    }

    fn walk_module_inner(&mut self, mi: u32, init_fn: FnIdx) {
        #[cfg(test)]
        let packet_prior = self.m.packet_body(init_fn);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
        #[cfg(test)]
        {
            let body = self.init_of(mi as usize);
            for stmt in body {
                self.m.packet_declarations(std::slice::from_ref(stmt));
            }
        }
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
            let sources: Vec<u32> = self.modules[mi as usize]
                .1
                .exports
                .iter()
                .filter_map(|e| match e {
                    perry_hir::Export::ReExport { source, .. }
                    | perry_hir::Export::ExportAll { source }
                    | perry_hir::Export::NamespaceReExport { source, .. } => {
                        self.reexport_target(mi, source)
                    }
                    perry_hir::Export::Named { .. } => None,
                })
                .filter(|t| self.dynamic_only.contains(t))
                .collect();
            for t in sources {
                let f = self.init_fn_of[&t];
                self.m.activate(f, "import");
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
        let saved = (self.order_ok, self.loop_depth);
        (self.order_ok, self.loop_depth) = (true, 0);
        for s in self.init_of(mi as usize) {
            self.stmt(&fx, s);
            self.name_enum_object(mi, s);
        }
        (self.order_ok, self.loop_depth) = saved;
        // Codegen invokes this owned body when the module evaluates. Follow
        // that explicit FuncId edge only after the owning init is activated.
        if let Some(id) = self.modules[mi as usize].1.async_init_function {
            if let Some(&f) = self.mods[mi as usize].funcs.get(&id) {
                self.m.activate(f, "module-init");
            }
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
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
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
        let TmplKind::Class {
            module: mi, name, ..
        } = self.m.tmpls[tm as usize].kind.clone()
        else {
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
        // The keys in the order construction defines them
        // (`perry_hir::private_names::construction_entries`): a private
        // field's entry is claimed at its declaration, so it holds the next
        // slot like a public field's (#8969).
        for f in perry_hir::private_names::construction_entries([c.fields.as_slice()]) {
            let a = if f.is_private {
                let key = self.private_field_key(mi, c, &f.name);
                self.m.private_atom(&key)
            } else {
                self.m.atom(&f.name)
            };
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
                perry_hir::ClassFieldOrigin::Definition | perry_hir::ClassFieldOrigin::Capture
                    if parent_leaks
                        || phase
                            .as_ref()
                            .is_some_and(|ph| ph.late_fields.contains(&f.name)) =>
                {
                    push_unique(&mut reserved, a)
                }
                perry_hir::ClassFieldOrigin::Definition | perry_hir::ClassFieldOrigin::Capture => {
                    push_unique(&mut definite, a)
                }
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
        #[cfg(test)]
        if self.m.packets_inert && self.m.packet_scope.as_ref().is_some_and(|b| b.f != f) {
            self.pending_walk.entry(f).or_default().push(w);
            return true;
        }
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
                        #[cfg(test)]
                        PendingWalk::OwnedClosure(cfx, e) => self.walk_closure_body(&cfx, &e),
                        PendingWalk::Module(mi) => {
                            let f = self.init_fn_of[&mi];
                            self.walk_module(mi, f);
                        }
                    }
                }
            }
        }
    }

    /// A declared function's or method's body: never a once body, so its
    /// stores are unordered (RPG1b).
    fn walk_fn_body(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &'a Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        let saved = (self.order_ok, self.loop_depth);
        (self.order_ok, self.loop_depth) = (false, 0);
        #[cfg(test)]
        let packet_prior = self.m.packet_body(fxi);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
        #[cfg(test)]
        self.m.packet_declarations(&f.body);
        let r = self.m.raise_of(fxi);
        let region = std::mem::replace(&mut self.m.gen_region, Some(r));
        self.walk_fn_body_inner(mi, fxi, f, class, ctor_of);
        self.m.gen_region = region;
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
        }
        (self.order_ok, self.loop_depth) = saved;
    }

    fn walk_fn_body_inner(
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
        let mut fx = Fx {
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
        if !f.is_strict && fn_ctor && crate::ctor_phase::body_mentions_this(&f.body) {
            // The view belongs to the function's body, like its parameters.
            let prev = std::mem::replace(&mut self.m.cur_fn, fxi);
            fx.this = self.sloppy_this(info.this);
            self.m.cur_fn = prev;
        }
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
        let own = self.own_helper_shape(&f.params, &f.body, f.is_async || f.is_generator);
        self.note_own_stores(f);
        let scope = self.narrow_enter_body(fx.module, fxi, &f.params, &f.body, &f.captures);
        self.stmts(&fx, &f.body);
        if ctor_of.is_none() && narrow::may_complete(&f.body) {
            // The body's normal completion returns `undefined` (a class
            // constructor's completion is never its result: `new` and
            // `super()` yield `this`).
            self.m.add_prim(info.ret, prim::UNDEF);
        }
        self.narrow_exit_body(scope);
        self.own_helper_finish(fxi, own);
    }

    /// The `this` a sloppy function's body reads: its callers' `this`,
    /// where a nullish one is the global object and a primitive one is
    /// boxed, an object the model does not hold (anything). The function's
    /// own `this` node, its interface, stays what callers pass.
    pub(crate) fn sloppy_this(&mut self, this: NodeId) -> NodeId {
        let view = self.m.node_l("sloppy-this");
        self.m.own(view);
        self.m.edge(this, view);
        let global_tok = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
        let global = self.tok_node(global_tok);
        for (test, from) in [
            (crate::model::guard::NULLISH, global),
            (crate::model::guard::BOXED, self.m.top_tok_node),
        ] {
            self.m.cons(
                view,
                Cons::Guard {
                    test,
                    from,
                    dst: view,
                },
            );
        }
        view
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
                // `undefined`, `bind_args`). The calls pass to the argument
                // node (`register_fn`): the local holds what they pass but
                // `undefined`, and the default when one may pass it.
                let v = self.expr(fx, d);
                let arg = self
                    .default_args
                    .get(&(fx.module, p.id))
                    .copied()
                    .unwrap_or(n);
                if arg != n {
                    self.m.cons(arg, Cons::NotUndefined { dst: n });
                }
                self.m.cons(
                    arg,
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

    /// Field initializers run per construction: unordered (RPG1b).
    fn walk_fields_body(&mut self, mi: u32, c: &'a Class) {
        // Field initializers run inside the constructor call: the shared
        // cell, which every catch reads.
        let region = std::mem::replace(&mut self.m.gen_region, Some(self.thrown));
        self.walk_fields_body_region(mi, c);
        self.m.gen_region = region;
    }

    fn walk_fields_body_region(&mut self, mi: u32, c: &'a Class) {
        let saved = (self.order_ok, self.loop_depth);
        (self.order_ok, self.loop_depth) = (false, 0);
        #[cfg(test)]
        let packet_prior = self
            .m
            .packet_body(self.field_init_fn[&self.mods[mi as usize].classes[&c.name]]);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
        self.walk_fields_body_inner(mi, c);
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
        }
        (self.order_ok, self.loop_depth) = saved;
    }

    fn walk_fields_body_inner(&mut self, mi: u32, c: &'a Class) {
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
        // A declared field's `undefined` is unobservable only when the
        // constructor body stores the key on every path first (`stored`; its
        // key being present from birth, `definite`, is not enough).
        let ctor_definite: Vec<String> = self
            .ctor_phase
            .get(&tm)
            .filter(|ph| !ph.parent_leaks)
            .map(|ph| ph.stored.clone())
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
                            key: Key::any(),
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
        #[cfg(test)]
        let packet_prior = self.m.packet_body(si);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
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
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
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
            in_loop: self.order_ok.then_some(self.loop_depth > 0),
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
        let mut pushed = 0;
        let mut i = 0;
        while i < ss.len() {
            if let Some(value) = for_of_binding(&ss[i..]) {
                // `value = arrayRecordForValue(…); state = n; let s = value`:
                // the binding reads the value just written, not every write
                // of the value local (its `undefined` initializer and
                // release, the step's output).
                let Stmt::Expr(set) = &ss[i] else {
                    unreachable!()
                };
                let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
                let n = self.expr(fx, set);
                self.m.cur_fn = prev;
                self.stmt(fx, &ss[i + 1]);
                self.for_binding = Some((fx.module, value, n));
                self.stmt(fx, &ss[i + 2]);
                self.for_binding = None;
                i += 3;
                continue;
            }
            self.stmt(fx, &ss[i]);
            // An `if` whose one branch always leaves: what follows runs
            // under the other outcome of its test (`narrow_guard`).
            if let Stmt::If {
                condition,
                then_branch,
                else_branch,
            } = &ss[i]
            {
                let else_exits = else_branch.as_deref().is_some_and(narrow::always_exits);
                if narrow::always_exits(then_branch) && !else_exits {
                    pushed += self.narrow_guard(fx, condition, false);
                } else if else_exits && !narrow::always_exits(then_branch) {
                    pushed += self.narrow_guard(fx, condition, true);
                }
            }
            i += 1;
        }
        self.narrow_pop(pushed);
    }

    pub(crate) fn stmt(&mut self, fx: &Fx, s: &Stmt) {
        if let Some((array, counter)) = guarded_for_of_walk(s) {
            self.for_of_walks.insert((fx.module, array, counter));
        }
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
            Stmt::Let {
                id,
                init,
                ty,
                mutable,
                ..
            } => {
                let n = self.local(fx.module, *id);
                if !*mutable && init.is_some() {
                    self.initialized_consts.insert((fx.module, *id));
                }
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
                let plan = self.own_guard_plan(condition, Region::Stmts(then_branch));
                let (t, then_test, else_test) = self.guard_test(fx, condition);
                let own = self.own_guard_arm(plan, t);
                let then_fx = Fx {
                    guard: Some((t, then_test)),
                    ..fx.clone()
                };
                let k = self.narrow_guard(fx, condition, true);
                self.stmts(&then_fx, then_branch);
                self.narrow_pop(k);
                self.own_guard_done(own);
                if let Some(e) = else_branch {
                    let else_fx = Fx {
                        guard: Some((t, else_test)),
                        ..fx.clone()
                    };
                    let k = self.narrow_guard(fx, condition, false);
                    self.stmts(&else_fx, e);
                    self.narrow_pop(k);
                }
            }
            Stmt::While { condition, body } => {
                self.narrow_enter_loop(s);
                self.loop_depth += 1;
                let _ = self.expr(fx, condition);
                let k = self.narrow_guard(fx, condition, true);
                self.stmts(fx, body);
                self.narrow_pop(k);
                self.loop_depth -= 1;
            }
            Stmt::DoWhile { body, condition } => {
                self.narrow_enter_loop(s);
                self.loop_depth += 1;
                let _ = self.expr(fx, condition);
                self.stmts(fx, body);
                self.loop_depth -= 1;
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                self.narrow_enter_loop(s);
                if let Some(i) = init {
                    self.stmt(fx, i);
                }
                self.loop_depth += 1;
                if let Some(c) = condition {
                    let _ = self.expr(fx, c);
                }
                // The update runs after the body: it reads the end node.
                let narrowed = self.narrow_for(
                    fx,
                    init.as_deref(),
                    condition.as_ref(),
                    update.as_ref(),
                    body,
                );
                if narrowed {
                    self.narrow_in_update(true);
                }
                if let Some(u) = update {
                    let _ = self.expr(fx, u);
                }
                if narrowed {
                    self.narrow_in_update(false);
                }
                match self.for_in_of(fx, body) {
                    Some(f) => self.for_in_body(fx, &f),
                    None => self.stmts(fx, body),
                }
                if narrowed {
                    self.narrow_end();
                }
                self.loop_depth -= 1;
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
                    let n = self.dead_zone(fx.module, *id);
                    self.m.add_prim(n, prim::UNDEF);
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(fx, e);
                let r = self.region();
                self.m.edge(v, r);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                // The try body is a throw region of its own; its catch and
                // finally run in the enclosing one.
                let rt = self.m.node_l("RAISE-try");
                let outer = std::mem::replace(&mut self.m.gen_region, Some(rt));
                self.stmts(fx, body);
                self.m.gen_region = outer;
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        let n = self.local(fx.module, *id);
                        self.m.edge(rt, n);
                        self.m.edge(self.thrown, n);
                    }
                    self.stmts(fx, &c.body);
                } else {
                    let r = self.region();
                    self.m.edge(rt, r);
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

impl<'a> Gen<'a> {
    /// The throw region generation is in (`thrown` outside a body).
    pub(crate) fn region(&self) -> NodeId {
        self.m.gen_region.unwrap_or(self.thrown)
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

/// The counted walk of a for-of source that is an ordinary array
/// (`perry_hir::lower::for_of_guard`): `for (let __forof_lit_k_N = 0;
/// __forof_lit_k_N < a.length; __forof_lit_k_N++) { const x =
/// a[__forof_lit_k_N]; body }`, where nothing in the body writes the
/// counter or the array binding. Its array and counter locals. The whole
/// shape is checked, not only the temp's name, because the binding read is
/// then taken to lie below the length.
fn for_of_walk(s: &Stmt) -> Option<(u32, u32)> {
    let Stmt::For {
        init: Some(init),
        condition: Some(condition),
        update: Some(update),
        body,
    } = s
    else {
        return None;
    };
    let Stmt::Let {
        id: k,
        name,
        init: Some(Expr::Integer(0)),
        ..
    } = init.as_ref()
    else {
        return None;
    };
    let k = *k;
    if !name.starts_with("__forof_lit_k_") {
        return None;
    }
    let Some(Stmt::Let {
        init: Some(Expr::IndexGet { object, index, .. }),
        ..
    }) = body.first()
    else {
        return None;
    };
    let (Expr::LocalGet(a), Expr::LocalGet(i)) = (object.as_ref(), index.as_ref()) else {
        return None;
    };
    let a = *a;
    if *i != k {
        return None;
    }
    let bounded = matches!(condition, Expr::Compare {
        op: perry_hir::CompareOp::Lt,
        left,
        right,
    } if matches!(left.as_ref(), Expr::LocalGet(l) if *l == k)
        && matches!(right.as_ref(), Expr::PropertyGet { object, property, .. }
            if property == "length" && matches!(object.as_ref(), Expr::LocalGet(o) if *o == a)));
    let stepped = matches!(update, Expr::Update {
        id,
        op: perry_hir::UpdateOp::Increment,
        ..
    } if *id == k);
    let untouched = !body
        .iter()
        .any(|s| perry_hir::walker::stmt_any_expr(s, &mut |e| writes_local(e, k, a)));
    (bounded && stepped && untouched).then_some((a, k))
}

/// The proven arm of a for-of over a source that is not a literal
/// (`perry_hir::lower::for_of_guard::source_for_of`):
/// `if (arrayRecordNeedsIterator(t) === false) { [const a = t;] walk }`.
/// The runtime test holds only for an ordinary Array whose iteration
/// members nobody replaced, so inside the arm `a` is an Array and the walk's
/// element is what iterating it yields. A loop of the walk's shape outside
/// that arm (over an array-like, say) is not a walk.
fn guarded_for_of_walk(s: &Stmt) -> Option<(u32, u32)> {
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Eq,
                left,
                right,
            },
        then_branch,
        ..
    } = s
    else {
        return None;
    };
    let Expr::NativeMethodCall {
        module,
        method,
        args,
        object: None,
        ..
    } = left.as_ref()
    else {
        return None;
    };
    let (Some(Expr::LocalGet(t)), Expr::Bool(false)) = (args.first(), right.as_ref()) else {
        return None;
    };
    if module != "__perry_runtime" || method != "arrayRecordNeedsIterator" {
        return None;
    }
    let (array, walk) = match then_branch.as_slice() {
        [walk] => (*t, walk),
        [Stmt::Let {
            id,
            init: Some(Expr::LocalGet(src)),
            ..
        }, walk]
            if src == t =>
        {
            (*id, walk)
        }
        _ => return None,
    };
    for_of_walk(walk).filter(|(a, _)| *a == array)
}

/// `e` writes local `k` or `a`, or holds a closure that may (a mutable
/// capture of either).
fn writes_local(e: &Expr, k: u32, a: u32) -> bool {
    match e {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } if *id == k || *id == a => true,
        Expr::Closure {
            mutable_captures, ..
        } if mutable_captures.iter().any(|id| *id == k || *id == a) => true,
        _ => {
            let mut hit = false;
            perry_hir::walker::walk_expr_children(e, &mut |c| hit = hit || writes_local(c, k, a));
            hit
        }
    }
}

/// The head of a for-of body as HIR lowers it (`IteratorRecord::drive`,
/// the shape codegen's `record_binding_prefix` reads): `value =
/// arrayRecordForValue(mode, value, element); state = n; let s = value`.
/// The value local.
fn for_of_binding(ss: &[Stmt]) -> Option<u32> {
    let [Stmt::Expr(Expr::LocalSet(value, read)), Stmt::Expr(Expr::LocalSet(state, n)), Stmt::Let {
        init: Some(Expr::LocalGet(bound)),
        ..
    }, ..] = ss
    else {
        return None;
    };
    let Expr::NativeMethodCall {
        module,
        method,
        args,
        ..
    } = read.as_ref()
    else {
        return None;
    };
    let ok = module == "__perry_runtime"
        && method == "arrayRecordForValue"
        && matches!(args.get(1), Some(Expr::LocalGet(v)) if v == value)
        && matches!(n.as_ref(), Expr::Number(_))
        && state != value
        && bound == value;
    ok.then_some(*value)
}
