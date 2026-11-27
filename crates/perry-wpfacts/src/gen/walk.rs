//! Constraint generation, part 1c: walking modules, classes and statements.

use super::*;
use perry_hir::Expr;

mod body;
mod layout;

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
            let shapes = crate::ctor_phase::plain_shapes(&modules[mi].1.classes);
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                let setters = self.chain_setters(tm);
                let ph = crate::ctor_phase::analyze_in_present_ms(
                    c,
                    &shapes,
                    &Default::default(),
                    true,
                    &setters,
                    self.m.ns_pairs,
                );
                self.ctor_phase.insert(tm, ph);
            }
        }
        self.settle_rule_m(&modules, None);
        self.ctor_phase_present(&modules);
        // The push-filled births (`push_fills`), with the birth lengths.
        if self.m.push_fills
            && self.m.rest_elems
            && self.m.array_reps
            && self.arr_lengths
            && !self.m.unit_mode
            && !self.m.arr_floor_off
            // A direct eval in the constructor reaches `this` without naming it.
            && !self.m.interp_linked
        {
            for mi in 0..modules.len() {
                for c in self.classes_of(mi) {
                    let fills = crate::push_fills::analyze(c, self.m.push_fill_sabotage);
                    self.push_fill.add(fills);
                }
            }
            self.m.push_fill_lens = self.push_fill.lens.clone();
        }
        // Parent leaks close the child's phase at `super()`.
        let tms: Vec<TmplId> = self.ctor_phase.keys().copied().collect();
        for tm in tms.iter().copied() {
            let leaks = self.parent_chain_leaks(tm);
            if let Some(ph) = self.ctor_phase.get_mut(&tm) {
                ph.parent_leaks = leaks;
            }
        }
        // R9BIRTH6: settled top-level prototype stores (`gen::settled`).
        for (mi, (_, module)) in modules.iter().enumerate() {
            for (cname, key, globals, proto) in super::settled::settled_proto_stores(module) {
                let Some(&tm) = self.mods[mi].classes.get(&cname) else {
                    continue;
                };
                let Some(classes) = globals
                    .classes
                    .iter()
                    .map(|c| self.mods[mi].classes.get(c).copied())
                    .collect::<Option<Vec<TmplId>>>()
                else {
                    continue;
                };
                let a = self.m.atom(&key);
                let gs: Vec<Atom> = globals.globals.iter().map(|g| self.m.atom(g)).collect();
                let ks: Vec<Atom> = globals.keys.iter().map(|k| self.m.atom(k)).collect();
                let into = if proto {
                    &mut self.m.settled_proto
                } else {
                    &mut self.m.settled_static
                };
                let w = into.entry((tm, a)).or_default();
                w.globals.extend(gs);
                w.keys.extend(ks);
                w.classes.extend(classes);
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
        self.m.worker_side = self.worker_side();
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
        // RTREAD14: the runtime's calls of the globals it reads by name,
        // after the program's own calls (their ids stay the program's).
        self.runtime_global_handlers();
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

    /// Per module: may a Worker of the program run it? A Worker runs the
    /// compiled entry module its filename names (perry-stdlib
    /// worker_threads entry table: the driver registers each entry
    /// `new Worker(<literal>)` / `new URL(<literal>, import.meta.url)`
    /// names, `FactsConfig::roots`) and every module that entry loads, by
    /// any import edge or re-export. A Worker whose filename names no entry
    /// (`paths` empty or partial; eval mode) may run any module: then every
    /// module is one.
    fn worker_side(&self) -> Vec<bool> {
        let n = self.modules.len();
        let mut unnamed = false;
        // The Worker entries: each `new Worker( spec )` names its module
        // through the import the driver resolved for that specifier (as
        // the driver's `module_roots` does). `FactsConfig::roots` holds the
        // program entry too, so it does not name them. A specifier with no
        // such module: unnamed.
        let mut entries: Vec<u32> = Vec::new();
        for (_, module) in &self.modules {
            perry_hir::for_each_worker_new(module, &mut |e| {
                if let perry_hir::Expr::WorkerNew {
                    paths,
                    partial,
                    is_eval,
                    ..
                } = e
                {
                    unnamed |= *is_eval || *partial || paths.is_empty();
                    for p in paths {
                        let t = module
                            .imports
                            .iter()
                            .filter(|i| &i.source == p)
                            .find_map(|i| i.resolved_path.as_ref())
                            .and_then(|r| self.path_index.get(r).copied());
                        match t {
                            Some(t) => entries.push(t),
                            None => unnamed = true,
                        }
                    }
                }
            });
        }
        if unnamed || self.m.unit_mode {
            return vec![true; n];
        }
        let mut side = vec![false; n];
        entries.sort_unstable();
        entries.dedup();
        let mut work = entries;
        for &m in &work {
            side[m as usize] = true;
        }
        while let Some(mi) = work.pop() {
            let mut next: Vec<u32> = self
                .imports_of(mi as usize)
                .iter()
                .filter(|i| !i.type_only && !i.runtime_erased)
                .filter_map(|i| i.resolved_path.as_ref())
                .filter_map(|r| self.path_index.get(r).copied())
                .collect();
            for e in &self.modules[mi as usize].1.exports {
                let source = match e {
                    perry_hir::Export::ReExport { source, .. }
                    | perry_hir::Export::ExportAll { source }
                    | perry_hir::Export::NamespaceReExport { source, .. } => source,
                    perry_hir::Export::Named { .. } => continue,
                };
                match self.reexport_target(mi, source) {
                    Some(t) => next.push(t),
                    // An unresolved re-export: any module may load.
                    None if source.starts_with('.') || source.starts_with('/') => {
                        return vec![true; n];
                    }
                    None => {}
                }
            }
            for t in next {
                if !side[t as usize] {
                    side[t as usize] = true;
                    work.push(t);
                }
            }
        }
        side
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
        } else {
            // R-HOSTREPORT: the runtime reports it (exception.rs
            // `exit_on_uncaught`).
            let h = self.m.host_report;
            self.m.edge(r, h);
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
                    self.m
                        .effect_in(init_fn, crate::model::Effect::Activate(f, "import"));
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
                self.m
                    .effect_in(init_fn, crate::model::Effect::Activate(f, "import"));
            }
        }
        self.m.note_walked(init_fn);
        let this = self.m.funcs[init_fn as usize].this;
        // Module top-level `this`: CJS `module.exports`-like object or
        // undefined in ESM; ⊤-free: a fresh literal-ish object.
        self.m.add_prim(this, prim::UNDEF);
        let fx = Fx {
            guard: None,
            live: None,
            module: mi,
            f: init_fn,
            this,
            ret: self.m.funcs[init_fn as usize].ret,
            class: None,
            ctor_of: None,
            new_target: None,
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
        let list_saved = self.list_enter_body();
        // The whole top-level body is one narrowing body; a unit's slice of
        // it is not (its locals are the whole body's).
        let top = self
            .unit_view(mi as usize)
            .is_none()
            .then(|| self.narrow_enter_top(mi, init_fn));
        for s in self.init_of(mi as usize) {
            self.stmt(&fx, s);
            self.name_enum_object(mi, s);
        }
        if let Some(top) = top {
            self.narrow_exit_body(top);
        }
        (self.order_ok, self.loop_depth) = saved;
        self.list_body = list_saved;
        // Codegen invokes this owned body when the module evaluates. Follow
        // that explicit FuncId edge only after the owning init is activated.
        if let Some(id) = self.modules[mi as usize].1.async_init_function {
            if let Some(&f) = self.mods[mi as usize].funcs.get(&id) {
                self.m
                    .effect_in(init_fn, crate::model::Effect::Activate(f, "module-init"));
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
        let list_saved = self.list_enter_body();
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
        self.list_body = list_saved;
    }

    fn walk_fields_body_inner(&mut self, mi: u32, c: &'a Class) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // Instance field initializers run in the field-init context.
        let fi = self.field_init_fn[&tm];
        self.m.note_walked(fi);
        let fthis = self.m.funcs[fi as usize].this;
        let ffx = Fx {
            guard: None,
            live: None,
            module: mi,
            f: fi,
            this: fthis,
            ret: self.m.funcs[fi as usize].ret,
            class: Some(tm),
            ctor_of: Some(tm),
            new_target: None,
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
                Some(e) => {
                    // A push-filled `new Array()`: the builtin's birth at
                    // its call site is the fill's (`new_builtin` names the
                    // site before the call exists: the solve is online).
                    if matches!(e, Expr::New { .. }) {
                        self.push_fill_new = self
                            .push_fill
                            .births
                            .get(&(e as *const Expr as usize))
                            .copied();
                    }
                    if matches!(e, Expr::Array(x, _) if x.is_empty())
                        && f.key_expr.is_none()
                        && !f.is_private
                    {
                        self.direct_arr =
                            Some(crate::records::ArrBirth::Field(mi, c.id, f.name.clone()));
                    }
                    let v = self.expr(&ffx, e);
                    self.push_fill_new = None;
                    self.direct_arr = None;
                    v
                }
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
                // R9READ2 rule I: a reservation the construction stores
                // first once its chain reads run no code: its `undefined`
                // only while that may fail (`Model::inert_undef`).
                None if f.key_expr.is_none()
                    && !f.is_private
                    && matches!(f.origin, perry_hir::ClassFieldOrigin::ConstructorStore)
                    && self.m.atom_index.get(f.name.as_str()).is_some_and(|&a| {
                        self.m.tmpls[tm as usize].inert_definite.contains(&a)
                    }) =>
                {
                    let a = self.m.atom(&f.name);
                    let v = self.m.node_l("inert-birth");
                    self.m.inert_undef(tm, a, v);
                    v
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
                            region: self.region(),
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
                            region: self.region(),
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
            live: None,
            module: mi,
            f: si,
            this: self.m.funcs[si as usize].this,
            ret: self.m.funcs[si as usize].ret,
            class: Some(tm),
            ctor_of: None,
            new_target: None,
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
            let at = self.m.cur_fn;
            self.m
                .effect_in(at, crate::model::Effect::Activate(f, "computed"));
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
            proto_first: self.m.proto_first_cur,
            module: mi,
            func: f,
            desc,
            ctor_phase_of,
            birth,
            ctor_fn: None,
            rooted: false,
            deep: false,
            key_value: None,
            proto_named: false,
            assign: false,
            proto_of: None,
            in_loop: self.order_ok.then_some(self.loop_depth > 0),
            define_attrs: None,
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
        // ONCHANGE3 rule A: the born-with runs of this list (`born_with.rs`).
        for r in crate::born_with::runs(ss) {
            let e = self.local(fx.module, r.e);
            let news = r.news.iter().map(|&x| self.local(fx.module, x)).collect();
            let calls = r
                .calls
                .iter()
                .map(|(o, k, on_e)| (*o, self.m.atom(k), *on_e))
                .collect();
            let module = fx.module;
            self.m.born_recs.push(crate::born_with::BornRec {
                module,
                e,
                news,
                calls,
            });
        }
        // Rule RP1: the positional steps of each array pattern record.
        for (i, s) in ss.iter().enumerate() {
            if let Some(c) = super::record_steps::cursor_let(s) {
                let mut steps = Vec::new();
                super::record_steps::positional_steps(c, &ss[i + 1..], &mut steps);
                self.record_steps.extend(steps);
            }
        }
        let mut pushed = 0;
        let mut i = 0;
        // A local derived below a guarded counter in this block
        // (`gen/in_bounds.rs`) is one until the block ends.
        let derived = self.in_bounds.derived.len();
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
            self.last_local_set = None;
            let da = self.def_assign_begin(fx, &ss[i]);
            self.stmt(fx, &ss[i]);
            pushed += self.narrow_assigned(fx, &ss[i]);
            pushed += self.def_assign_end(fx, da);
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
        self.in_bounds.derived.truncate(derived);
        self.narrow_pop(pushed);
    }

    pub(crate) fn stmt(&mut self, fx: &Fx, s: &Stmt) {
        if let Some((array, counter)) = guarded_for_of_walk(s) {
            self.for_of_walks.insert((fx.module, array, counter));
        }
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        let first = self
            .m
            .proto_first_stmts
            .get(&(s as *const Stmt as usize))
            .copied();
        let prev_first = std::mem::replace(&mut self.m.proto_first_cur, first);
        // A declaration's own operations stand at the declaration.
        let kind0 =
            self.m.stop_top.as_mut().map(|st| {
                std::mem::replace(&mut st.gen_kind, crate::topstop::variant(s, "Stmt::"))
            });
        let pos0 = match s {
            Stmt::Let { id, .. } if self.m.stop_top.is_some() => self.modules[fx.module as usize]
                .1
                .local_source_spans
                .get(id)
                .map(|sp| {
                    std::mem::replace(
                        &mut self.m.stop_top.as_mut().unwrap().gen_pos,
                        (fx.module, sp.start),
                    )
                }),
            Stmt::Let { .. } => None,
            _ => match (
                self.m.stop_top.as_mut(),
                s.byte_offset().filter(|&o| o != 0),
            ) {
                (Some(st), Some(o)) => Some(std::mem::replace(&mut st.gen_pos, (fx.module, o))),
                _ => None,
            },
        };
        self.stmt_inner(fx, s);
        if let Some(p0) = pos0 {
            self.m.stop_top.as_mut().unwrap().gen_pos = p0;
        }
        if let (Some(k0), Some(st)) = (kind0, self.m.stop_top.as_mut()) {
            st.gen_kind = k0;
        }
        self.m.proto_first_cur = prev_first;
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
                name,
                init,
                ty,
                mutable,
                ..
            } => {
                let n = self.local(fx.module, *id);
                if let (
                    false,
                    Some(
                        perry_hir::Expr::IndexGet { object, index, .. }
                        | perry_hir::Expr::RegionOut { object, index, .. },
                    ),
                ) = (*mutable, init)
                {
                    if let perry_hir::Expr::LocalGet(k) = index.as_ref() {
                        self.same_key_reads
                            .insert((fx.module, *id), (*k, object.as_ref().clone(), fx.f));
                    }
                }
                if [
                    "__forof_lit_src_",
                    "__forof_lit_arr_",
                    "__iterator_record_",
                    "__arr_",
                ]
                .iter()
                .any(|p| name.starts_with(p))
                {
                    self.m.for_of_relays.insert(n);
                }
                if matches!(init, Some(perry_hir::Expr::Array(x, _)) if x.is_empty()) {
                    self.direct_arr = Some(crate::records::ArrBirth::Local(fx.module, *id));
                }
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
                let list_tmpl = self.m.tmpls.len() as TmplId;
                match init {
                    Some(e) => {
                        let v = self.expr_zs(fx, e);
                        self.m.edge(v, n);
                        // A `var` redeclared with an initializer writes the
                        // same local: it ends every narrowing of it.
                        self.narrowed_write(fx.module, *id, v);
                        // The local's node takes the value unguarded, and so
                        // does its version (`narrow_assigned`).
                        self.last_local_set = Some((fx.module, *id, v, None));
                    }
                    None => self.m.add_prim(n, prim::UNDEF),
                }
                self.in_bounds_let(fx, *id, init.as_ref());
                // `let id = []`: a list birth (`list_caps`). The empty literal
                // bore one template, the first made since the statement began.
                if matches!(init, Some(perry_hir::Expr::Array(x, _)) if x.is_empty())
                    && self.m.tmpls.len() as TmplId > list_tmpl
                {
                    self.list_birth(fx.module, *id, list_tmpl);
                }
                self.direct_arr = None;
                // FVA 4.3b / E1: the keys local of a for-in, and the source of
                // a for-of over `Object.keys(o)` / `getOwnPropertyNames(o)`
                // written in the loop head (a fresh Array no other code sees;
                // the counted walk binds exactly its elements, `for_of_walk`).
                use crate::model::KeyList;
                // The namespace pair `NamespacePatched(Object) ? Object.keys(o)
                // : <intrinsic>` is one call of the realm's own member while
                // its generic arm yields nothing (`namespace_pair`): its keys
                // are `o`'s; what a replaced member returns runs the body with
                // every key (`Gen::keys_generic`, `for_in_body`).
                let generic = self.last_ns_generic.take();
                let (kinit, generic) = match init {
                    Some(perry_hir::Expr::Conditional {
                        condition,
                        else_expr,
                        ..
                    }) if self.m.ns_pairs
                        && matches!(condition.as_ref(), perry_hir::Expr::NamespacePatched(ns) if ns == "Object")
                        && generic.is_some() =>
                    {
                        (Some(else_expr.as_ref()), generic)
                    }
                    _ => (init.as_ref(), None),
                };
                let keys_src = match kinit {
                    Some(perry_hir::Expr::ForInKeys(o)) => Some((o, KeyList::ForIn)),
                    Some(perry_hir::Expr::ObjectKeys(o)) if name.starts_with("__forof_lit_src_") => {
                        Some((o, KeyList::OwnKeys))
                    }
                    Some(perry_hir::Expr::ObjectGetOwnPropertyNames(o))
                        if name.starts_with("__forof_lit_src_") =>
                    {
                        Some((o, KeyList::OwnNames))
                    }
                    _ => None,
                };
                if let Some((o, list)) = keys_src {
                    if let perry_hir::Expr::LocalGet(obj) = o.as_ref() {
                        self.for_in_keys_of.insert((fx.module, *id), (*obj, list));
                        if let Some(g) = generic {
                            self.keys_generic.insert((fx.module, *id), g);
                        }
                    }
                }
                if let (Some(perry_hir::Expr::LocalGet(src)), true) =
                    (kinit, name.starts_with("__forof_lit_arr_"))
                {
                    if let Some(&v) = self.for_in_keys_of.get(&(fx.module, *src)) {
                        self.for_in_keys_of.insert((fx.module, *id), v);
                    }
                    if let Some(&g) = self.keys_generic.get(&(fx.module, *src)) {
                        self.keys_generic.insert((fx.module, *id), g);
                    }
                }
                if let Some(perry_hir::Expr::LocalGet(src)) = init {
                    self.local_copies.insert((fx.module, *id), *src);
                }
                if name.starts_with("__forin_obj_") {
                    if let Some(p) = init.as_ref().and_then(super::for_in::subj_path) {
                        self.for_in_paths.insert((fx.module, *id), p);
                    }
                }
            }
            Stmt::Expr(e) => {
                let _ = self.expr_zs(fx, e);
            }
            Stmt::Return(e, _) => {
                if let Some(e) = e {
                    let v = self.expr_zs(fx, e);
                    let v = self.live_gate(fx, v);
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
                // `if (__step_is_error)` / `if (IterResultGetDone)`: plain-async
                // step arms keep the enclosing arm's `live` (not gated).
                let special = matches!(condition, Expr::IterResultGetDone)
                    || matches!(condition, Expr::LocalGet(id) if self.step.by_flag.contains_key(&(fx.module, *id)));
                let then_live = if special {
                    fx.live
                } else {
                    self.arm_live(fx, t, then_test)
                };
                let then_fx = Fx {
                    guard: Some((t, then_test)),
                    live: then_live,
                    ..fx.clone()
                };
                // A branch that always leaves: its writes reach only where
                // it jumps to (`narrow_leave`).
                let pre = self.narrow_snapshot();
                let k = self.narrow_guard(fx, condition, true);
                // `if (__step_is_error)`: a plain-async step's error arm.
                let arm = match condition {
                    Expr::LocalGet(id) => self.step.by_flag.get(&(fx.module, *id)).copied(),
                    _ => None,
                };
                if let Some(v) = arm {
                    self.step.arms.push((fx.module, v));
                }
                // `if (IterResultGetDone)`: the plain-async step's resolve
                // arm, entered only after a `return` wrote the slot.
                let done_arm = matches!(condition, Expr::IterResultGetDone)
                    .then(|| self.iter_done_arm.replace(fx.f));
                self.stmts(&then_fx, then_branch);
                if let Some(p) = done_arm {
                    self.iter_done_arm = p;
                }
                if arm.is_some() {
                    self.step.arms.pop();
                }
                self.narrow_pop(k);
                self.narrow_leave(then_branch, pre);
                self.own_guard_done(own);
                if let Some(e) = else_branch {
                    let else_live = if special {
                        fx.live
                    } else {
                        self.arm_live(fx, t, else_test)
                    };
                    let else_fx = Fx {
                        guard: Some((t, else_test)),
                        live: else_live,
                        ..fx.clone()
                    };
                    let pre = self.narrow_snapshot();
                    let k = self.narrow_guard(fx, condition, false);
                    self.stmts(&else_fx, e);
                    self.narrow_pop(k);
                    self.narrow_leave(e, pre);
                }
            }
            Stmt::While { condition, body } => {
                self.narrow_enter_loop(s);
                self.loop_depth += 1;
                self.jumps += 1;
                let _ = self.expr_zs(fx, condition);
                let k = self.narrow_guard(fx, condition, true);
                self.stmts(fx, body);
                self.narrow_pop(k);
                self.jumps -= 1;
                self.loop_depth -= 1;
            }
            Stmt::DoWhile { body, condition } => {
                self.narrow_enter_loop(s);
                self.loop_depth += 1;
                self.jumps += 1;
                let _ = self.expr_zs(fx, condition);
                self.stmts(fx, body);
                self.jumps -= 1;
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
                self.jumps += 1;
                if let Some(c) = condition {
                    let _ = self.expr_zs(fx, c);
                }
                // The update runs after the body: it reads the end node.
                let narrowed = self.narrow_for(
                    fx,
                    init.as_deref(),
                    condition.as_ref(),
                    update.as_ref(),
                    body,
                );
                // `for (init; c; u) B` is `init; while (c) { B; u }`: B and
                // u see `c` true, as a `while` body does. The counter
                // `narrow_for` narrows keeps its ranges alone.
                let counter = match (narrowed, init.as_deref()) {
                    (true, Some(Stmt::Let { id, .. })) => Some(*id),
                    _ => None,
                };
                let k = match condition {
                    Some(c) => self.narrow_guard_skip(fx, c, true, counter),
                    None => 0,
                };
                let pushed = k + usize::from(narrowed);
                self.narrow_in_update(pushed, true);
                if let Some(u) = update {
                    let _ = self.expr_zs(fx, u);
                }
                self.narrow_in_update(pushed, false);
                let scope = self.enum_scope(fx, body);
                let scoped = scope.is_some();
                if let Some(s) = scope {
                    self.for_in_reads.push(s);
                }
                match self.for_in_of(fx, body) {
                    Some(f) => self.for_in_body(fx, &f),
                    None => self.stmts(fx, body),
                }
                if scoped {
                    self.for_in_reads.pop();
                }
                self.narrow_pop(k);
                if narrowed {
                    self.narrow_end();
                }
                self.jumps -= 1;
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
                ..
            } => {
                // The try body is a throw region of its own; its catch and
                // finally run in the enclosing one.
                let rt = self.m.node_l("RAISE-try");
                let outer = std::mem::replace(&mut self.m.gen_region, Some(rt));
                self.tries += 1;
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
                self.tries -= 1;
            }
            Stmt::Switch {
                discriminant,
                cases,
                byte_offset,
            } => {
                let d = self.expr_zs(fx, discriminant);
                // `===` against case values: no coercion, identity.
                self.jumps += 1;
                self.m.identity(d, IdentityRoute::Switch);
                // Rule M1: a switch outside every loop, try and other switch
                // of the body (blocks and `if` around it only), switching on
                // a parameter (its own node: no default value, no
                // `arguments` object, never assigned: no `=` or `++` of it)
                // with string-literal case tests only.
                // Per clause, the runs that reach it: the run entering at
                // clause j runs j and each next clause while the one before
                // may complete normally (ECMA-262 14.12.4: fallthrough; the
                // default clause is entered when no case test is equal).
                let f = self.m.cur_fn;
                // `jumps` counts this switch itself.
                let top = self.jumps == 1 && self.tries == 0 && self.m.clause.is_none();
                let param = match discriminant {
                    Expr::LocalGet(id)
                        if top
                            && f != NO_FN
                            && cases.len() < 64
                            && !self.m.funcs[f as usize].has_arguments
                            && !self.m.switch_runs.contains_key(&f)
                            && !self.written_locals(fx.module).contains(id) =>
                    {
                        let pn = self.local(fx.module, *id);
                        self.m.funcs[f as usize]
                            .params
                            .iter()
                            .position(|&q| q == pn)
                    }
                    _ => None,
                };
                let mut labels: Vec<Option<crate::model::Atom>> = Vec::new();
                let mut masks: Vec<u64> = Vec::new();
                if param.is_some() {
                    for c in cases.iter() {
                        match &c.test {
                            None => labels.push(None),
                            Some(Expr::String(t)) => labels.push(Some(self.m.atom(t))),
                            Some(_) => {
                                labels.clear();
                                break;
                            }
                        }
                    }
                }
                if labels.len() == cases.len()
                    && !cases.is_empty()
                    && labels.iter().filter(|l| l.is_none()).count() <= 1
                {
                    if let Some(j) = param {
                        let ends = |c: &perry_hir::SwitchCase| {
                            matches!(
                                c.body.last(),
                                Some(
                                    Stmt::Break
                                        | Stmt::Continue
                                        | Stmt::LabeledBreak(_)
                                        | Stmt::LabeledContinue(_)
                                        | Stmt::Return(_, _)
                                        | Stmt::Throw(_)
                                )
                            )
                        };
                        for i in 0..cases.len() {
                            let mut m = 0u64;
                            let mut k = i;
                            loop {
                                m |= 1 << k;
                                if k == 0 || ends(&cases[k - 1]) {
                                    break;
                                }
                                k -= 1;
                            }
                            masks.push(m);
                        }
                        self.m.switch_runs.insert(f, (j as u32, labels));
                    }
                }
                for (i, c) in cases.iter().enumerate() {
                    if let Some(t) = &c.test {
                        let tv = self.expr_zs(fx, t);
                        self.m.identity(tv, IdentityRoute::Switch);
                        // N1: a case test is the strict equality it means.
                        self.equality_demand(discriminant, t, d, tv);
                    }
                    let clause = self.m.clause;
                    if let Some(&m) = masks.get(i) {
                        self.m.clause = Some((f, m));
                    }
                    // Rule G3: a clause only literal tests select narrows
                    // the host of the discriminant (`Gen::case_narrow`).
                    let g3 = self.case_narrow(fx, discriminant, cases, i, *byte_offset);
                    self.stmts(fx, &c.body);
                    self.narrow_pop(g3);
                    self.m.clause = clause;
                }
                self.jumps -= 1;
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
        init: Some(Expr::IndexGet { object, index, .. } | Expr::RegionOut { object, index, .. }),
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
        right, ..
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
                ..
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
