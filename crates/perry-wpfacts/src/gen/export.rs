//! Constraint generation, part 1b: the export side (cross-module links,
//! contracts and store effects).

use super::*;

impl<'a> Gen<'a> {
    // --------------------------------------------------------- export side

    /// The node holding module `mi`'s export `name` (created on demand).
    pub(crate) fn export_node(&mut self, mi: u32, name: Atom) -> NodeId {
        if let Some(&n) = self.exports.get(&(mi, name)) {
            return n;
        }
        let n = self
            .m
            .node_named("export", || format!("export({mi},{name})"));
        self.exports.insert((mi, name), n);
        n
    }

    /// Connect every module's export declarations to their export nodes.
    pub fn link_exports(&mut self) {
        let modules = self.modules.clone();
        for (mi, (_, module)) in modules.iter().enumerate() {
            let mi = mi as u32;
            let ns = self.mods[mi as usize].ns_tok;
            for e in self.exports_of(mi as usize) {
                match e {
                    Export::Named { local, exported } => {
                        let a = self.m.atom(exported);
                        let en = self.export_node(mi, a);
                        if let Some(&tm) = self.mods[mi as usize].classes.get(local) {
                            self.export_class.insert((mi, a), tm);
                        }
                        if let Some(f) = self
                            .functions_of(mi as usize)
                            .into_iter()
                            .find(|f| &f.name == local)
                        {
                            if let Some(&fx) = self.mods[mi as usize].funcs.get(&f.id) {
                                self.export_fn.insert((mi, a), fx);
                            }
                        }
                        let src = self.scope_value(mi, local);
                        self.m.edge(src, en);
                        // An export to the module's other units only is
                        // not on the namespace (§12.3).
                        if !self.view.is_some_and(|v| v.unit_exports.contains(exported)) {
                            let f = self.m.field(ns, a);
                            self.m.edge(en, f);
                        }
                    }
                    Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        let a = self.m.atom(exported);
                        let en = self.export_node(mi, a);
                        match self.resolve_source(mi, module, source) {
                            Some(t) => {
                                let b = self.m.atom(imported);
                                let src = self.export_node(t, b);
                                self.m.edge(src, en);
                            }
                            None => self
                                .m
                                .add_top(en, |_| format!("unresolved re-export source {source}")),
                        }
                        let f = self.m.field(ns, a);
                        self.m.edge(en, f);
                    }
                    Export::ExportAll { source } => match self.resolve_source(mi, module, source) {
                        Some(t) => {
                            // Every export of `t` is visible through `mi`.
                            let tns = self.mods[t as usize].ns_tok;
                            let all = self.m.all_fields(tns);
                            let fa = self.m.field_any(ns);
                            self.m.edge(all, fa);
                            let _ = all;
                        }
                        None => {
                            let fa = self.m.field_any(ns);
                            self.m
                                .add_top(fa, |_| format!("unresolved export * source {source}"));
                        }
                    },
                    Export::NamespaceReExport { source, name } => {
                        let a = self.m.atom(name);
                        let en = self.export_node(mi, a);
                        match self.resolve_source(mi, module, source) {
                            Some(t) => {
                                let tns = self.mods[t as usize].ns_tok;
                                self.m.add_tok(en, tns);
                            }
                            None => self
                                .m
                                .add_top(en, |_| format!("unresolved re-export source {source}")),
                        }
                        let f = self.m.field(ns, a);
                        self.m.edge(en, f);
                    }
                }
            }
            for (name, fid) in self.exported_functions_of(mi as usize) {
                if let Some(&fx) = self.mods[mi as usize].funcs.get(fid) {
                    let a = self.m.atom(name);
                    let en = self.export_node(mi, a);
                    self.export_fn.insert((mi, a), fx);
                    let t = self.m.funcs[fx as usize].token;
                    self.m.add_tok(en, t);
                    let f = self.m.field(ns, a);
                    self.m.edge(en, f);
                }
            }
            for f in self.functions_of(mi as usize) {
                if f.is_exported {
                    let a = self.m.atom(&f.name);
                    let en = self.export_node(mi, a);
                    let fx = self.mods[mi as usize].funcs[&f.id];
                    self.export_fn.insert((mi, a), fx);
                    let t = self.m.funcs[fx as usize].token;
                    self.m.add_tok(en, t);
                    let fl = self.m.field(ns, a);
                    self.m.edge(en, fl);
                }
            }
            for c in self.classes_of(mi as usize) {
                if c.is_exported {
                    let a = self.m.atom(&c.name);
                    let en = self.export_node(mi, a);
                    let tm = self.mods[mi as usize].classes[&c.name];
                    self.export_class.insert((mi, a), tm);
                    let t = self.m.tok(TokKind::ClassObj(tm));
                    self.m.add_tok(en, t);
                    let fl = self.m.field(ns, a);
                    self.m.edge(en, fl);
                }
            }
            for name in self.exported_objects_of(mi as usize) {
                let a = self.m.atom(name);
                let en = self.export_node(mi, a);
                let src = self.scope_value(mi, name);
                self.m.edge(src, en);
                let fl = self.m.field(ns, a);
                self.m.edge(en, fl);
            }
        }
        // An export a module never declares, asked for by an importer, reads
        // through `export *` chains: route every export node through the
        // namespace's computed-key node (the `export *` targets feed it).
        // A name the module declares itself is not read through them: an
        // own export shadows every `export *` (ECMAScript ResolveExport,
        // §18.14).
        let mut own: crate::pts::FxHashSet<(u32, Atom)> = Default::default();
        for mi in 0..modules.len() {
            let mut names: Vec<&str> = Vec::new();
            for e in self.exports_of(mi) {
                match e {
                    Export::Named { exported, .. } | Export::ReExport { exported, .. } => {
                        names.push(exported)
                    }
                    Export::NamespaceReExport { name, .. } => names.push(name),
                    Export::ExportAll { .. } => {}
                }
            }
            names.extend(
                self.exported_functions_of(mi)
                    .iter()
                    .map(|(n, _)| n.as_str()),
            );
            for f in self.functions_of(mi) {
                if f.is_exported {
                    names.push(&f.name);
                }
            }
            for c in self.classes_of(mi) {
                if c.is_exported {
                    names.push(&c.name);
                }
            }
            names.extend(self.exported_objects_of(mi).iter().map(|n| n.as_str()));
            for n in names {
                let a = self.m.atom(n);
                own.insert((mi as u32, a));
            }
        }
        let keys: Vec<(u32, Atom)> = self.exports.keys().copied().collect();
        for (mi, a) in keys {
            if self
                .view
                .is_some_and(|v| v.unit_exports.contains(&self.m.atoms[a as usize]))
                || own.contains(&(mi, a))
            {
                continue;
            }
            let ns = self.mods[mi as usize].ns_tok;
            let en = self.exports[&(mi, a)];
            let fl = self.m.field(ns, a);
            self.m.edge(fl, en);
        }
    }

    /// Per-unit mode: these modules are one unit of a larger program, and
    /// every other unit is unknown compiled code. What the unit publishes
    /// (exports, namespaces, CommonJS records, thrown values, the global
    /// object) reaches that code; what it reads from it is ⊤ (imports of
    /// other units already are; CommonJS paths of other units and the
    /// registry are here); and that code stores through what it holds, the
    /// intrinsic prototypes and globals included: one ⊤ store under any key
    /// through ⊤, as a store in compiled code with a ⊤ receiver is.
    ///
    /// Only what another unit can name is published (`published`, design
    /// §11.1): an export no module imports, re-exports or reaches as a
    /// namespace stays inside the unit (the CommonJS wrap exports every
    /// hoisted class, named by the module's source or not). `None` publishes
    /// every export and namespace.
    pub(crate) fn unit_boundary(&mut self, published: Option<&crate::units::Published>) {
        let r = self.m.reason("published to another unit");
        let rc = self
            .m
            .reason("published to another unit (closed: its calls are known)");
        self.m.closed_pub_reason = Some(rc);
        let path_of = |g: &Self, mi: u32| g.modules[mi as usize].0;
        let mut exports: Vec<((u32, Atom), NodeId)> =
            self.exports.iter().map(|(&k, &n)| (k, n)).collect();
        exports.sort_unstable();
        for ((mi, a), n) in exports {
            let name = &self.m.atoms[a as usize];
            if !published.is_none_or(|p| p.names(path_of(self, mi), name)) {
                continue;
            }
            // A published class is a contract (§11.2), unless the census
            // broke it: then it escapes, as everything did in P1.
            match self.export_class.get(&(mi, a)).copied() {
                Some(tm)
                    if published.is_some_and(|p| !p.broken(&self.m.tmpls[tm as usize].display)) =>
                {
                    self.m.publish_class(tm, r);
                    self.class_contracts.push(((mi, a), n, tm));
                }
                _ => {
                    // A published function is a contract (§11.2): it still
                    // escapes (anyone may call it), but its parameters carry
                    // markers and its result is checked after the solve.
                    if let Some(&f) = self.export_fn.get(&(mi, a)) {
                        let name = format!("fn:{}", self.m.funcs[f as usize].name);
                        if published.is_some_and(|p| !p.broken(&name)) {
                            self.m.contract_fns.insert(f);
                            self.fn_contracts.push(((mi, a), f));
                        }
                        // A closed function (decision 19) is published
                        // through its argument contract, not escaped.
                        let fid = self.m.funcs[f as usize].func_id;
                        if self
                            .view
                            .is_some_and(|v| v.closed.values().any(|&id| id == fid))
                        {
                            self.m.sink_node(n, rc);
                            continue;
                        }
                    } else if published.is_some() && !self.export_class.contains_key(&(mi, a)) {
                        // A published value binding is a value contract
                        // (§12): checked after the solve, escaped there if
                        // it holds what no contract describes.
                        self.value_contracts.push(((mi, a), n));
                        continue;
                    }
                    self.m.sink_node(n, r)
                }
            }
        }
        // A static namespace import of another unit sees the exports'
        // contracts (§18.2); the namespace object itself escapes only where
        // it reaches other units as a value.
        for mi in 0..self.mods.len() {
            if published.is_none_or(|p| p.ns_object(path_of(self, mi as u32))) {
                let t = self.mods[mi].ns_tok;
                self.m.escape(t, r);
            }
        }
        let mut paths: Vec<(String, NodeId)> = self
            .cjs_paths
            .iter()
            .map(|(k, &n)| (k.clone(), n))
            .collect();
        paths.sort_unstable();
        let foreign = |g: &Self, k: &str| !g.path_index.contains_key(k);
        for (k, n) in paths {
            if foreign(self, &k) {
                self.m
                    .add_top(n, |_| "CommonJS module of another unit".into());
            }
            self.m.sink_node(n, r);
        }
        for n in [self.cjs_registry, self.cjs_unknown_path]
            .into_iter()
            .chain(self.cjs_main)
        {
            self.m
                .add_top(n, |_| "CommonJS registry shared with other units".into());
            self.m.sink_node(n, r);
        }
        let thrown = self.thrown;
        self.m.sink_node(thrown, r);
        let gt = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
        self.m.escape(gt, r);
        let holder = self.m.node_l("other-unit-receiver");
        self.m.add_tok(holder, TOP);
        let src = self.m.top_tok_node;
        self.m.cons(
            holder,
            Cons::Store {
                key: Key::Any,
                src,
                site: 0,
            },
        );
    }

    /// Decision 13: a closed function of this unit (`UnitView::closed`) is
    /// called only at the module's direct calls, so when it is published to
    /// the module's other units its parameters hold the stand-ins of what
    /// their calls pass (the union of their argument contracts), not ⊤
    /// (`Model::escape_fn_params`). No contract yet (its callers solve after
    /// it): nothing, and the driver solves it again once they publish one.
    /// A caller that may pass anything (`top`) leaves the parameters ⊤.
    pub(crate) fn closed_params(&mut self) {
        let (Some(view), Some(unit_in)) = (self.view, self.unit_in) else {
            return;
        };
        if unit_in.eval_scopes() {
            return;
        }
        let path = self.modules[0].0;
        let mut closed: Vec<(&'a String, &'a u32)> = view.closed.iter().collect();
        closed.sort();
        let mut local: Vec<FnIdx> = Vec::new();
        for (nm, fid) in closed {
            let Some(&fx) = self.mods[0].funcs.get(fid) else {
                continue;
            };
            let ac = unit_in.arg_contract(path, nm);
            // A broken closed function (decision 19: a unit let it escape)
            // is called by unknown code: ⊤ parameters.
            // A closed local's contract name carries its function id (`fn_contract_of`).
            let name = match nm.contains('#') {
                true => format!("fn:{}#{fid}", self.m.funcs[fx as usize].name),
                false => format!("fn:{}", self.m.funcs[fx as usize].name),
            };
            // A closed class (design §18.13) is broken by its display; its
            // constructor reads no `arguments` (`lib::closed_of`).
            let broken = match self.m.funcs[fx as usize].ctor_of {
                Some(tm) => {
                    let fi = &self.m.funcs[fx as usize];
                    fi.has_arguments
                        || fi.rest.is_some()
                        || unit_in.broken(&self.m.tmpls[tm as usize].display)
                }
                None => unit_in.broken(&name),
            };
            if ac.is_some_and(|a| a.top) || broken {
                continue;
            }
            let n = self.m.funcs[fx as usize].params.len();
            let described = (0..n)
                .map(|i| {
                    !ac.and_then(|a| a.params.get(i))
                        .is_some_and(|vc| vc.unknown)
                })
                .collect();
            self.m.closed_described.insert(fx, described);
            let mut nodes = Vec::with_capacity(n);
            for i in 0..n {
                // A described position holds undefined when a call passes
                // fewer arguments: its contract's kinds carry it (DECISIONS 77).
                let node = match ac.and_then(|a| a.params.get(i)) {
                    Some(vc) => self.stand_in_value(0, &format!("{nm}:arg{i}"), vc),
                    None => {
                        let node = self.m.node_named("arg", || format!("arg{i}({nm})"));
                        self.m.add_prim(node, prim::UNDEF);
                        node
                    }
                };
                nodes.push(node);
            }
            self.m.closed_params.insert(fx, nodes);
            // A closed local (decision 23) no other unit can name: published
            // where it is created, as a contract function through its closed
            // contract (markers on its parameters, not an escape), so a
            // passing call's argument contract can describe it.
            if nm.contains('#') {
                local.push(fx);
            }
        }
        if local.is_empty() {
            return;
        }
        let rc = self
            .m
            .reason("published to another unit (closed: its calls are known)");
        self.m.closed_pub_reason = Some(rc);
        for &fx in &local {
            self.m.contract_fns.insert(fx);
        }
        let toks: Vec<Tok> = (0..self.m.toks.len() as Tok)
            .filter(
                |&t| matches!(self.m.toks[t as usize], TokKind::Func(f, _) if local.contains(&f)),
            )
            .collect();
        for t in toks {
            self.m.escape(t, rc);
        }
    }

    /// After a solve: a published class export holds its class value only
    /// (a module may rebind the name); otherwise it escapes after all. True
    /// when something changed (solve again).
    pub(crate) fn contract_check(&mut self) -> bool {
        let r = self.m.reason("published to another unit");
        let mut changed = self.apply_store_effects();
        for (e, n, tm) in std::mem::take(&mut self.class_contracts) {
            let c = self.m.tok(TokKind::ClassObj(tm));
            let node = &self.m.nodes[n as usize];
            if node.prim != 0 || node.pts.to_sorted().iter().any(|&t| t != c) {
                self.m.sink_node(n, r);
                changed = true;
            } else {
                self.class_contracts.push((e, n, tm));
            }
        }
        // A contract function's result: literal-shape objects (their
        // fields go to the caller's unit, so they leave) and what is
        // reachable from its own arguments (their markers: a result
        // transfer, the caller's objects come back to the caller, §11.2
        // "the return"); anything else and it escapes as before.
        let rets = self.m.contract_rets.clone();
        for (f, ret) in rets {
            if self.ret_top.contains(&f) {
                continue;
            }
            // An argument marker reads as the markers it stands for (DECISIONS 44).
            let toks: Vec<Tok> = self.m.nodes[ret as usize]
                .pts
                .to_sorted()
                .into_iter()
                .flat_map(|t| self.m.marker_members(t))
                .collect();
            let ok = toks.iter().all(|&t| match self.m.toks[t as usize] {
                _ if self.m.param_marker.get(&t).is_some_and(|&(g, _)| g == f) => true,
                // An escaped marker reads as ⊤ (DECISIONS 42), of any
                // contract function: another unit's value.
                _ if self.m.marker_is_top(t) => true,
                // An unknown value or a builtin function in the result is
                // the caller's ⊤, nothing of this unit leaves through it.
                TokKind::Top | TokKind::BuiltinFn => true,
                // An instance of a contract class: given out (`give_out`).
                TokKind::Obj(tm, _) if self.contract_class(tm) => true,
                // Another unit's instance (a stand-in): forwarded with that
                // unit's contract (`fn_contract_of`), its proof (§17.10 4).
                TokKind::Obj(tm, _) if self.stand_in_cc.contains_key(&tm) => !self
                    .unit_in
                    .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display)),
                TokKind::Obj(tm, _) => self.literal_ok(tm),
                _ => false,
            });
            if !ok {
                self.ret_top.insert(f);
                self.m.sink_node(ret, r);
                changed = true;
                continue;
            }
            for t in toks {
                if matches!(self.m.toks[t as usize], TokKind::Top | TokKind::BuiltinFn)
                    || self.m.param_marker.contains_key(&t)
                {
                    continue;
                }
                if let TokKind::Obj(tm, _) = self.m.toks[t as usize] {
                    if self.contract_class(tm) {
                        changed |= self.m.give_out(tm, t);
                    } else if self.stand_in_cc.contains_key(&tm) {
                        // Another unit's layout, forwarded with its owner's
                        // contract: nothing of this unit leaves through its
                        // fields, as for a published value (DECISIONS 79).
                        continue;
                    } else if !self.ret_ok.contains(&t) {
                        // A literal result is the same layout contract as a
                        // published literal: its field functions are contract
                        // functions the caller calls through their contract
                        // (DECISIONS 78, as the value path below).
                        changed |= self.note_field_fns(t);
                    }
                }
                changed |= self.fields_leave(t, r);
            }
        }
        // A published value binding: what it may hold must be described by
        // a contract (a literal-shape object of this unit whose fields then
        // leave, an instance of a contract class, another unit's stand-in,
        // an unknown value); anything else and it escapes as before.
        for (e, n) in std::mem::take(&mut self.value_contracts) {
            match self.value_contract_holds(n, r, false) {
                Some(c) => changed |= c,
                None => {
                    changed = true;
                    continue;
                }
            }
            self.value_contracts.push((e, n));
        }
        // What this unit writes into other units' bindings (`local_write`):
        // a value contract as a published binding's, its values leave the
        // same way; one no contract describes escapes.
        let mut writes: Vec<(u32, NodeId)> = self
            .binding_writes
            .iter()
            .map(|(&id, &n)| (id, n))
            .filter(|(id, _)| !self.write_top.contains(id))
            .collect();
        writes.sort_unstable();
        for (id, n) in writes {
            match self.value_contract_holds(n, r, true) {
                Some(c) => changed |= c,
                None => {
                    self.write_top.insert(id);
                    changed = true;
                }
            }
        }
        // Its store effects into other units' objects (decision 17): the
        // stored values are published the same way.
        let mut stores: Vec<(TmplId, Atom, NodeId)> =
            self.m.foreign_stores.iter().copied().collect();
        stores.sort_unstable();
        for (tm, a, n) in stores {
            if self.store_top.contains(&(tm, a)) {
                continue;
            }
            match self.value_contract_holds(n, r, true) {
                Some(c) => changed |= c,
                None => {
                    self.store_top.insert((tm, a));
                    changed = true;
                }
            }
        }
        changed |= self.nested_fields(r);
        // A literal of this unit passed to another unit's closed function
        // (decision 13) is described by the argument contract as a value
        // contract describes a published literal: its field functions are
        // contract functions the callee calls through their contract.
        for t in self.closed_arg_literals() {
            if !self.field_fns.keys().any(|&(ft, _)| ft == t) {
                changed |= self.note_field_fns(t);
            }
        }
        changed
    }

    /// A value published as a contract (a published binding, a write of
    /// another unit's binding, a store effect): what node `n` may hold must
    /// be described by a contract (a literal-shape object of this unit whose
    /// fields then leave, an instance of a contract class, another unit's
    /// stand-in, an unknown value). `None`: it is not, and it escaped; else
    /// whether something changed. `back`: an effect, held by a unit solved
    /// before this one in a round; it reads no layout of this unit by
    /// display (the driver publishes those level by level, after it), so
    /// an object's fields leave whole, never as nested layouts.
    fn value_contract_holds(&mut self, n: NodeId, r: u32, back: bool) -> Option<bool> {
        let mut changed = false;
        {
            let toks = self.m.nodes[n as usize].pts.to_sorted();
            // A function of this unit the binding holds, which nothing let
            // escape yet, is a contract function (§11.2, as a published
            // literal's field functions are, `note_field_fns`): other units
            // call it through its contract.
            for &t in &toks {
                if let TokKind::Func(f, _) = self.m.toks[t as usize] {
                    if self.m.funcs[f as usize].func_id != u32::MAX && !self.m.is_escaped(t) {
                        changed |= self.m.contract_fns.insert(f);
                    }
                }
            }
            if !toks.iter().all(|&t| self.value_tok_ok(t)) {
                self.m.sink_node(n, r);
                return None;
            }
            for t in toks {
                if let TokKind::Func(f, _) = self.m.toks[t as usize] {
                    if self.m.funcs[f as usize].func_id != u32::MAX {
                        // A closed function: through its argument contract
                        // (decision 19, `Model::closed_pub_reason`).
                        let rf = match self.m.closed_params.contains_key(&f) {
                            true => self.m.closed_pub_reason.unwrap_or(r),
                            false => r,
                        };
                        self.m.escape(t, rf);
                    }
                }
                if let TokKind::Obj(tm, _) = self.m.toks[t as usize] {
                    // An instance of a contract class other units hold:
                    // given out, they read its fields and call its methods.
                    if self.contract_class(tm) {
                        changed |= self.m.give_out(tm, t);
                        changed |= self.fields_leave(t, r);
                    } else if !self.stand_in_cc.contains_key(&tm) {
                        if !self.ret_ok.contains(&t) {
                            changed |= self.note_field_fns(t);
                        }
                        changed |= self.fields_leave(t, r);
                    } else {
                        continue;
                    }
                    if back && !self.all_sunk.contains(&t) {
                        self.sink_all_fields(t, r);
                        changed = true;
                    }
                }
            }
        }
        Some(changed)
    }

    /// This unit's literal objects its calls pass to other units' closed
    /// functions (`arg_contract_of`): the arguments of their stand-ins.
    fn closed_arg_literals(&self) -> Vec<Tok> {
        let Some(view) = self.view else {
            return Vec::new();
        };
        let mut fns: Vec<Tok> = view
            .closed_refs
            .iter()
            .filter_map(|nm| self.mods[0].imports.get(nm))
            .flat_map(|&n| self.m.nodes[n as usize].pts.to_sorted())
            .chain(
                self.stand_in_fns
                    .iter()
                    .filter(|((_, holder), &(_, t))| {
                        holder.is_none()
                            && self
                                .stand_in_fc
                                .get(&t)
                                .is_some_and(|fc| fc.closed.is_some())
                    })
                    .map(|(_, &(_, t))| t),
            )
            .filter(|t| self.stand_in_fc.contains_key(t))
            .collect();
        fns.sort_unstable();
        fns.dedup();
        let mut out: Vec<Tok> = fns
            .into_iter()
            .flat_map(|t| self.m.funcs[self.m.tok_fn(t) as usize].params.clone())
            .flat_map(|p| self.m.nodes[p as usize].pts.to_sorted())
            .filter(|&t| match self.m.toks[t as usize] {
                TokKind::Obj(tm, _) => {
                    !self.stand_in_cc.contains_key(&tm)
                        && !self.contract_class(tm)
                        && !self.m.is_marker(t)
                }
                _ => false,
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Before a published literal's fields leave: a field holding only
    /// functions of this unit makes them contract functions (design §11.2:
    /// other units call them through their contract, parameters carry
    /// markers, the result is checked). True when it made a new contract
    /// function (the solve continues).
    fn note_field_fns(&mut self, t: Tok) -> bool {
        let mut changed = false;
        let r = self
            .m
            .reason("a field function of a literal given to another unit");
        let mut fields: Vec<(Atom, NodeId)> = self
            .m
            .field_keys
            .get(&t)
            .into_iter()
            .flatten()
            .filter_map(|&a| self.m.fields.get(&(t, a)).map(|&n| (a, n)))
            .collect();
        fields.sort_unstable();
        for (a, n) in fields {
            let node = &self.m.nodes[n as usize];
            let toks = node.pts.to_sorted();
            if node.prim != 0 || toks.is_empty() {
                continue;
            }
            let fs: Option<Vec<(FnIdx, Tok)>> = toks
                .iter()
                .map(|&ft| match self.m.toks[ft as usize] {
                    TokKind::Func(f, _)
                        if self.m.funcs[f as usize].func_id != u32::MAX || self.open_fn(ft) =>
                    {
                        Some((f, ft))
                    }
                    _ => None,
                })
                .collect();
            if let Some(fs) = fs {
                for &(f, ft) in &fs {
                    // Another unit's open function: the field forwards its
                    // contract (design §18.9).
                    if self.m.funcs[f as usize].func_id == u32::MAX {
                        continue;
                    }
                    // Other units call it with what they have: it escapes
                    // as a contract function (⊤ parameters that carry
                    // markers, its result checked). One that escaped
                    // already, as no contract function (a closed callee's
                    // contract lets what its argument reaches escape, or
                    // another closure context of it escaped), gets its
                    // markers now; a result that escaped then is named by
                    // no contract (`ret_top`).
                    let new = self.m.contract_fns.insert(f);
                    changed |= new;
                    if new {
                        match self.m.escaped.get(&ft).copied() {
                            Some(r0) => self.m.escape_fn_params(ft, r0),
                            None => self.m.escape(ft, r),
                        }
                        if self.m.sunk_nodes.contains(&self.m.funcs[f as usize].ret) {
                            self.ret_top.insert(f);
                        }
                    }
                    // Another unit calls it as a method of this object.
                    if !self.m.funcs[f as usize].lexical_this {
                        let this = self.m.funcs[f as usize].this;
                        self.m.add_tok(this, t);
                    }
                }
                self.field_fns
                    .insert((t, a), fs.into_iter().map(|(f, _)| f).collect());
            }
        }
        changed
    }

    /// What other units stored into existing fields of this unit's literal
    /// layouts and class instances (decision 17): a value of what the stored
    /// values' contracts describe (the kinds, DECISIONS 73; the stand-ins of
    /// the layouts, classes and functions; ⊤), stored into that field of
    /// every instance of the layout by an anonymous writer. True when this
    /// added constraints.
    fn apply_store_effects(&mut self) -> bool {
        let Some(u) = self.unit_in else { return false };
        let mut want: Vec<(TmplId, &'a String, &'a crate::units::ValueContract)> = Vec::new();
        for (tm, t) in self.m.tmpls.iter().enumerate() {
            if !matches!(
                t.kind,
                TmplKind::Class { .. }
                    | TmplKind::Lit {
                        enum_image: true,
                        ..
                    }
            ) || self.stand_in_cc.contains_key(&(tm as TmplId))
            {
                continue;
            }
            for (k, vc) in u.stored_keys(&t.display) {
                want.push((tm as TmplId, k, vc));
            }
        }
        if want.is_empty() {
            return false;
        }
        let want: Vec<(TmplId, Atom, &'a crate::units::ValueContract)> = want
            .into_iter()
            .map(|(tm, k, vc)| (tm, self.m.atom(k), vc))
            .collect();
        let site = match self.effect_site {
            Some(x) => x,
            None => {
                let site = self.m.anon_post_site("store by another unit");
                self.effect_site = Some(site);
                site
            }
        };
        let mut changed = false;
        for t in 0..self.m.toks.len() as Tok {
            let TokKind::Obj(tm, _) = self.m.toks[t as usize] else {
                continue;
            };
            for &(_, a, vc) in want.iter().filter(|w| w.0 == tm) {
                if self.effects_done.insert((t, a)) {
                    let src = self.effect_src(tm, a, vc);
                    self.m.store_into(t, crate::model::Key::Named(a), src, site);
                    changed = true;
                }
            }
        }
        changed
    }

    /// The value a store effect stores into field `a` of layout `tm`: what
    /// its contract describes, as an import of that value (one node per
    /// field).
    fn effect_src(&mut self, tm: TmplId, a: Atom, vc: &'a crate::units::ValueContract) -> NodeId {
        if let Some(&n) = self.effect_srcs.get(&(tm, a)) {
            return n;
        }
        let n = self.at_module_scope(|g| g.stand_in_value(0, "stored by another unit", vc));
        self.effect_srcs.insert((tm, a), n);
        n
    }

    /// A literal-shape object or an enum's object (§18.2) of this unit no
    /// census report broke.
    fn literal_ok(&self, tm: TmplId) -> bool {
        let t = &self.m.tmpls[tm as usize];
        matches!(
            t.kind,
            TmplKind::Class {
                literal_shape: true,
                ..
            } | TmplKind::Lit {
                enum_image: true,
                ..
            }
        ) && !self.unit_in.is_some_and(|u| u.broken(&t.display))
    }

    fn contract_class(&self, tm: TmplId) -> bool {
        self.class_contracts.iter().any(|&(_, _, c)| c == tm)
    }

    /// A token a value contract describes (`contract_check`).
    fn value_tok_ok(&self, t: Tok) -> bool {
        match self.m.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn => true,
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) if self.stand_in_cc.contains_key(&tm) => {
                !self
                    .unit_in
                    .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display))
            }
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) if self.contract_class(tm) => true,
            TokKind::Obj(tm, _) => self.literal_ok(tm),
            // A contract function of this unit (made one above).
            TokKind::Func(f, _) if self.m.funcs[f as usize].func_id != u32::MAX => {
                self.m.contract_fns.contains(&f)
            }
            _ => self.stand_in_fc.contains_key(&t),
        }
    }

    /// A published literal object's fields go to another unit: their
    /// contents leave once (accessors run with unknown receivers). True when
    /// this added constraints.
    ///
    /// A literal of this unit (`nested_ok`) leaves key by key instead
    /// (design §18.7, `nested_fields`): what it holds under a computed key
    /// leaves here, each named field once `contract_check` decided it.
    fn fields_leave(&mut self, t: Tok, r: u32) -> bool {
        if !self.ret_ok.insert(t) {
            return false;
        }
        match self.m.toks[t as usize] {
            TokKind::Obj(tm, _) if self.nested_ok(tm) => {
                self.leaving.push(t);
                let any = self.m.field_any(t);
                self.m.sink_node(any, r);
            }
            _ => self.sink_all_fields(t, r),
        }
        let rl = self.m.node_l("contract-ret-load");
        self.m.sink_node(rl, r);
        self.m.accessor_walk(
            t,
            t,
            crate::model::Key::Any,
            crate::model::ChainAct::Load(rl),
        );
        true
    }

    fn sink_all_fields(&mut self, t: Tok, r: u32) {
        if self.all_sunk.insert(t) {
            let all = self.m.all_fields(t);
            self.m.cons(all, Cons::TopSink { reason: r });
        }
    }

    /// A literal layout of this unit a published value's field may hold
    /// as a nested contract (design §18.7): no census broke it, it is no
    /// stand-in or contract class. (A closed callee of another unit that
    /// gets it as an argument sees no nested fields; what it does to them
    /// comes back through its parameter's reach uses, decision 13.)
    fn nested_ok(&self, tm: TmplId) -> bool {
        self.literal_ok(tm) && !self.stand_in_cc.contains_key(&tm) && !self.contract_class(tm)
    }

    /// Design §18.7: the named fields of the published literals that leave
    /// key by key, decided per template and key over every instance that
    /// leaves. A field holding only instances of literal layouts this unit
    /// may publish (`nested_ok`, none escaped) is described by those
    /// layouts, which leave the same way; any other content and the field
    /// is sunk for good. A field holding only primitives so far stays
    /// undecided (nothing leaves through it; a reader sees ⊤). Re-run to
    /// the fixpoint by `contract_check`: a described field whose contents
    /// grow is checked again. True when this added constraints.
    fn nested_fields(&mut self, r: u32) -> bool {
        let mut changed = false;
        loop {
            let n0 = self.leaving.len();
            let mut by: FxHashMap<TmplId, Vec<Tok>> = FxHashMap::default();
            for &t in &self.leaving {
                if let Some(tm) = self.m.tmpl_of(t) {
                    by.entry(tm).or_default().push(t);
                }
            }
            let mut tms: Vec<TmplId> = by.keys().copied().collect();
            tms.sort_unstable();
            for tm in tms {
                let ts = by.remove(&tm).unwrap_or_default();
                if !self.nested_ok(tm) {
                    // Broken by the census: every field leaves as before.
                    self.field_layouts.retain(|&(t2, _), _| t2 != tm);
                    for &t in &ts {
                        if !self.all_sunk.contains(&t) {
                            self.sink_all_fields(t, r);
                            changed = true;
                        }
                    }
                    continue;
                }
                let mut keys: Vec<Atom> = ts
                    .iter()
                    .flat_map(|t| self.m.field_keys.get(t).into_iter().flatten().copied())
                    .collect();
                keys.sort_unstable();
                keys.dedup();
                for a in keys {
                    let mut ok = !self.sunk_fields.contains(&(tm, a));
                    let mut inner: Vec<(TmplId, Tok)> = Vec::new();
                    let mut prim = false;
                    if ok {
                        'toks: for &t in &ts {
                            let Some(&n) = self.m.fields.get(&(t, a)) else {
                                continue;
                            };
                            prim |= self.m.nodes[n as usize].prim != 0;
                            for x in self.m.nodes[n as usize].pts.to_sorted() {
                                match self.m.toks[x as usize] {
                                    TokKind::Obj(itm, _)
                                        if self.nested_ok(itm) && !self.m.is_escaped(x) =>
                                    {
                                        inner.push((itm, x))
                                    }
                                    _ => {
                                        ok = false;
                                        break 'toks;
                                    }
                                }
                            }
                        }
                    }
                    if ok && inner.is_empty() {
                        continue;
                    }
                    if ok {
                        let mut itms: Vec<TmplId> = inner.iter().map(|&(i, _)| i).collect();
                        itms.sort_unstable();
                        itms.dedup();
                        self.field_layouts.insert((tm, a), itms);
                        if prim {
                            self.field_prims.insert((tm, a));
                        }
                        for (_, x) in inner {
                            changed |= self.fields_leave(x, r);
                        }
                        continue;
                    }
                    self.sunk_fields.insert((tm, a));
                    self.field_layouts.remove(&(tm, a));
                    self.field_prims.remove(&(tm, a));
                    for &t in &ts {
                        if let Some(&n) = self.m.fields.get(&(t, a)) {
                            if self.sunk_field_toks.insert((t, a)) {
                                self.m.sink_node(n, r);
                                changed = true;
                            }
                        }
                    }
                }
            }
            if self.leaving.len() == n0 {
                break;
            }
        }
        changed
    }

    /// The module an `export … from source` of module `mi` reads: the
    /// driver's resolution of the source, else an import of the same source.
    /// Unresolved, the export is ⊤ (a native or unknown module); a program
    /// module must never land there, or its namespace members are called
    /// through a ⊤ that never escaped them.
    fn resolve_source(&self, mi: u32, module: &Module, source: &str) -> Option<u32> {
        let path = self.modules[mi as usize].0;
        if let Some(p) = self.export_sources.get(path).and_then(|m| m.get(source)) {
            return self.path_index.get(p).copied();
        }
        let imp = module.imports.iter().find(|i| i.source == source)?;
        let p = imp.resolved_path.as_ref()?;
        self.path_index.get(p).copied()
    }

    /// A module-scope name's value node.
    pub(crate) fn scope_value(&mut self, mi: u32, name: &str) -> NodeId {
        if let Some(&n) = self.mods[mi as usize].scope.get(name) {
            return n;
        }
        if let Some(n) = self.import_value(mi, name) {
            return n;
        }
        self.m.top_val(&format!("unresolved scope name {name}"))
    }
}
