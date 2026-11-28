//! Constraint generation, part 1c: walking modules.

use super::*;
use perry_hir::Expr;

mod body;
mod class;
mod layout;
mod stmt;

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
        self.ctor_phase_present(&modules, None);
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
